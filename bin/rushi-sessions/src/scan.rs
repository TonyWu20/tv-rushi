//! The `source` command: scan local roots for rushi session directories
//! and print one TSV row per session.
use std::fs;
use std::path::{Path, PathBuf};

use crate::remote::{parse_remote_line, remote_source_lines, split_roots};
use crate::state::{alive, fmt_local, last_phase, mtime_secs};

pub(crate) struct Row {
    pub(crate) status: &'static str,
    pub(crate) name: String,
    pub(crate) repo: String,
    pub(crate) phase: String,
    pub(crate) last: String,
    pub(crate) mtime: i64,
    pub(crate) path: String,
}
pub fn source(active_only: bool, args: Vec<PathBuf>) {
    let no_args = args.is_empty();
    let (local_requested, remote_groups) = split_roots(args);
    let local_roots = resolve_roots(local_requested, no_args);
    let mut rows: Vec<Row> = Vec::new();
    for sdir in find_sessions(&local_roots) {
        let base = sdir.trim_end_matches('/').to_string();
        let entries = match fs::read_dir(&base) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let repo = repo_of(&base);
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let p = format!("{}/{}", base, name);
            if !Path::new(&p).is_dir() {
                continue;
            }
            let ev = format!("{}/events.jsonl", p);
            let pidf = format!("{}/loop.pid", p);
            let cwf = format!("{}/cwd", p);
            let has = |s: &str| Path::new(s).exists();
            if !(has(&ev) || has(&pidf) || has(&cwf)) {
                continue;
            }
            let pid = fs::read_to_string(&pidf)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let status = if alive(&pid) { "ACTIVE" } else { "IDLE" };
            let phase = if has(&ev) {
                last_phase(&ev)
            } else {
                "?".into()
            };
            let refp = if has(&ev) {
                &ev
            } else if has(&pidf) {
                &pidf
            } else {
                &cwf
            };
            let mtime = mtime_secs(refp);
            rows.push(Row {
                status,
                name,
                repo: repo.clone(),
                phase,
                last: fmt_local(mtime),
                mtime,
                path: p,
            });
        }
    }
    for g in &remote_groups {
        for line in remote_source_lines(g, active_only) {
            if let Some(row) = parse_remote_line(&line, &g.host) {
                rows.push(row);
            }
        }
    }
    // The active-only source view: keep the sessions with a live loop.
    if active_only {
        rows.retain(|r| r.status == "ACTIVE");
    }
    // ACTIVE first, then by mtime descending (the old sort key).
    rows.sort_by(|a, b| {
        let ka = a.status != "ACTIVE";
        let kb = b.status != "ACTIVE";
        ka.cmp(&kb).then_with(|| b.mtime.cmp(&a.mtime))
    });
    for r in &rows {
        let line = [
            r.status,
            &r.name,
            &r.repo,
            &r.phase,
            &r.last,
            &r.mtime.to_string(),
            &r.path,
        ]
        .join("\t");
        println!("{}", line);
    }
}
/// Resolve the requested scan roots into an absolute list. No roots
/// means the CWD when `cwd_fallback` is set (the original
/// single-root behavior); otherwise the list stays empty. Relative
/// roots join the CWD. A root that is missing or not a directory is
/// skipped with a warning on stderr; the remaining roots still scan.
fn resolve_roots(roots: Vec<PathBuf>, cwd_fallback: bool) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if roots.is_empty() {
        return if cwd_fallback { vec![cwd] } else { Vec::new() };
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        let p = if r.is_absolute() { r } else { cwd.join(r) };
        if !p.is_dir() {
            eprintln!("rushi-sessions: skipping missing root: {}", p.display());
            continue;
        }
        out.push(p);
    }
    out
}
/// Find every directory named `sessions` under the roots by calling
/// `fd` (hard dependency) once with all roots as start points,
/// returned as **absolute** paths (fd is given absolute start points).
/// `fd` skips hidden directories by default (no `--no-hidden`),
/// matching the old call. The flag set mirrors the old pipeline
/// exactly: `--no-ignore -td`, the five `-E` prunes, the pattern, and
/// the start points. A missing or failing `fd` yields an empty list
/// (the channel's `requirements` show `fd` as missing to the user).
/// Overlapping roots can find the same directory twice, so the lines
/// are deduped by canonical path.
fn find_sessions(roots: &[PathBuf]) -> Vec<String> {
    // No roots at all (every requested root missing): scan nothing.
    // The CWD fallback lives in `resolve_roots`, not here. A bare fd
    // call with no start points would walk the CWD by itself.
    if roots.is_empty() {
        return Vec::new();
    }
    let out = std::process::Command::new("fd")
        .arg("--no-ignore")
        .arg("-t")
        .arg("d")
        .args(["-E", "target"])
        .args(["-E", ".git"])
        .args(["-E", "node_modules"])
        .args(["-E", "scratch"])
        .args(["-E", ".nix"])
        .arg("sessions")
        .args(roots.iter().map(|p| p.as_os_str()))
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    let lines = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    dedup_paths(lines)
}
/// Drop lines that name a directory already listed (overlapping
/// roots, or two spellings of the same directory). The canonical path
/// is the key. A line that cannot be canonicalized (a broken
/// symlink, a race) keys on itself.
fn dedup_paths(lines: Vec<String>) -> Vec<String> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let key = fs::canonicalize(&line).unwrap_or_else(|_| PathBuf::from(line.clone()));
        let key = key.to_string_lossy().into_owned();
        if seen.insert(key) {
            out.push(line);
        }
    }
    out
}
/// The repo a `sessions` directory belongs to: the basename of the
/// directory that contains it. Falls back to `.` when the parent has no
/// basename (a bare top-level `sessions`).
fn repo_of(base: &str) -> String {
    Path::new(base)
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_of_top_level() {
        assert_eq!(repo_of("sessions"), ".");
        assert_eq!(repo_of("repo/sessions"), "repo");
        assert_eq!(repo_of("/home/u/repo/sessions"), "repo");
    }


    #[test]
    fn resolve_roots_empty_falls_back_to_cwd() {
        let roots = resolve_roots(Vec::new(), true);
        assert_eq!(roots, vec![std::env::current_dir().unwrap()]);
    }


    #[test]
    fn resolve_roots_empty_without_flag() {
        assert!(resolve_roots(Vec::new(), false).is_empty());
    }


    #[test]
    fn resolve_roots_joins_relative_to_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let roots = resolve_roots(vec![PathBuf::from(".")], true);
        assert_eq!(roots, vec![cwd.join(".")]);
    }


    #[test]
    fn resolve_roots_skips_missing() {
        let cwd = std::env::current_dir().unwrap();
        // A missing root is skipped; a present absolute root is kept.
        let roots = resolve_roots(vec![PathBuf::from("/no/rushi/such/dir"), cwd.clone()], false);
        assert_eq!(roots, vec![cwd]);
    }


    #[test]
    fn find_sessions_no_roots_no_walk() {
        // With zero roots, no fd walk happens (no CWD fallback here).
        assert!(find_sessions(&[]).is_empty());
    }


    #[test]
    fn dedup_paths_drops_repeats() {
        let t = std::env::temp_dir();
        let s = t.to_string_lossy().into_owned();
        let out = dedup_paths(vec![s.clone(), s.clone(), s.clone()]);
        assert_eq!(out, vec![s]);
    }


    #[test]
    fn dedup_paths_keeps_distinct() {
        let t = std::env::temp_dir();
        let a = t.join("dedup-a").to_string_lossy().into_owned();
        let b = t.join("dedup-b").to_string_lossy().into_owned();
        let out = dedup_paths(vec![a.clone(), b.clone(), a.clone()]);
        assert_eq!(out.len(), 2);
    }


}
