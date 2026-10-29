//! Resolve a binary from a project's flake devShell (decision 89).
//!
//! A devShell's binaries are content-addressed store paths that
//! change only when the project flake changes. Re-running the
//! `nix develop` probe on every call is heavy (an evaluate and
//! activate, and a build when the store is cold), so it would
//! freeze a preview panel that re-fetches on redraw. Instead, the
//! binary's store path is resolved once per devShell generation,
//! cached with a generation fingerprint, and exec'd directly on
//! later calls. The heavy materialize runs only when the generation
//! is out of date.

use std::path::{Path, PathBuf};

/// The wall-clock cap on the `nix develop` materialize call. A flake
/// evaluate or build that runs past it aborts the resolve and the
/// caller falls back to the PATH binary.
pub(crate) const MATERIALIZE_TIMEOUT_SECS: u64 = 60;

/// How long a materialized entry is trusted before the probe
/// re-runs. The devShell context-window value also tracks the
/// project's config and model, which can change without touching
/// the flake files. The TTL bounds that staleness: an entry older
/// than it re-materializes, so the displayed window cannot drift
/// past the TTL.
pub(crate) const CACHE_TTL_SECS: u64 = 1800;

/// Resolve the store path of `bin` in the devShell of the project
/// dir `dir` (decision 89). A fresh cache entry (the generation
/// still matches, the path still exists, and it is inside the TTL)
/// is returned without touching nix. Otherwise the path is
/// materialized once and cached. None when the binary does not
/// resolve.
pub(crate) fn resolve(dir: &Path, bin: &str) -> Option<PathBuf> {
    let gen = generation(dir);
    let f = cache_path(dir, bin);
    if let Some(e) = entry_load(&f) {
        if is_fresh(&e, &gen, now_unix()) && Path::new(&e.path).exists() {
            return Some(PathBuf::from(e.path));
        }
    }
    let path = materialize(dir, bin)?;
    entry_store(&f, &path, &gen);
    Some(PathBuf::from(path))
}

/// The flake dir that `dir`'s effective `.envrc` loads (decision 95).
/// The `.envrc` of `dir` itself is first priority; if it is absent,
/// the parent dirs are walked up to the first `.envrc`. The
/// `use flake <path>` line names the flake: a bare `use flake` (or
/// `use flake .`) loads the `.envrc`'s own dir; a path form loads
/// that dir (`~` expanded, relative to the `.envrc` dir). None when
/// no `.envrc` is found on the chain.
pub(crate) fn envrc_flake_dir(dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(dir.to_path_buf());
    while let Some(d) = cur {
        if let Some(fd) = envrc_flake_dir_in(&d) {
            return Some(norm_lexical(fd));
        }
        cur = d.parent().map(Path::to_path_buf);
    }
    None
}

/// Lexically normalize a path: drop `.` and resolve `..` against the
/// components already seen (never past the root). An input from
/// `envrc_flake_dir` is absolute, so the result stays absolute.
fn norm_lexical(p: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::RootDir => out.push(c.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // `Path::pop` is a no-op at the root, so this never
                // climbs past it.
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(std::ffi::OsString::from("."));
    }
    out
}

/// The flake dir named by `<d>/.envrc`, when that file exists.
fn envrc_flake_dir_in(d: &Path) -> Option<PathBuf> {
    let envrc = d.join(".envrc");
    if !envrc.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(envrc).ok()?;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(rest) = t.strip_prefix("use flake") {
            return flake_ref_dir(rest.trim(), d);
        }
    }
    None
}

/// The flake dir of a `use flake` reference. A bare `.` (or empty)
/// is the `.envrc`'s own dir. A local path is `~`-expanded and
/// resolved against the `.envrc` dir. A non-local reference (a
/// `github:` or URL form) cannot be materialized locally, so it
/// resolves to None and the caller falls back to the global PATH.
fn flake_ref_dir(ref_: &str, envrc_dir: &Path) -> Option<PathBuf> {
    let ref_ = ref_.trim();
    if ref_.is_empty() || ref_ == "." {
        return Some(envrc_dir.to_path_buf());
    }
    if let Some(rest) = ref_.strip_prefix("~/") {
        let home = std::env::var("HOME").ok()?;
        return Some(PathBuf::from(home).join(rest));
    }
    if ref_ == "~" {
        return std::env::var("HOME").ok().map(PathBuf::from);
    }
    if ref_.starts_with("github:") || ref_.contains("://") || ref_.starts_with("git+") {
        return None;
    }
    let pb = PathBuf::from(ref_);
    Some(if pb.is_absolute() { pb } else { envrc_dir.join(pb) })
}

/// The devShell generation fingerprint: the flake file mtimes plus
/// the host arch. It is stable while the flake is unchanged and
/// shifts when the flake (or lock) is edited, which rebuilds the
/// devShell to a new store path.
fn generation(dir: &Path) -> String {
    let mtime = |p: &Path| -> u64 {
        std::fs::metadata(p)
            .ok()
            .and_then(|md| md.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    };
    format!(
        "{}:{}:{}",
        mtime(&dir.join("flake.nix")),
        mtime(&dir.join("flake.lock")),
        std::env::consts::ARCH
    )
}

/// The cache file for one project dir and bin.
fn cache_path(dir: &Path, bin: &str) -> PathBuf {
    let key = dir
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| dir.to_string_lossy().into_owned());
    let h = fnv1a_64(&key);
    cache_dir()
        .join("rushi-sessions")
        .join("devshell")
        .join(format!("{h:016x}"))
        .join(bin)
}

/// The system cache dir, for the devShell path cache.
fn cache_dir() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CACHE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x);
        }
    }
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return PathBuf::from(h).join(".cache");
        }
    }
    std::env::temp_dir()
}

fn fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Debug, Clone)]
struct Entry {
    path: String,
    gen: String,
    ts: u64,
}

fn entry_load(f: &Path) -> Option<Entry> {
    let text = std::fs::read_to_string(f).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Entry {
        path: v.get("path")?.as_str()?.to_string(),
        gen: v.get("gen")?.as_str()?.to_string(),
        ts: v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0),
    })
}

fn entry_store(f: &Path, path: &str, gen: &str) {
    if let Some(p) = f.parent() {
        std::fs::create_dir_all(p).ok();
    }
    let v = serde_json::json!({ "path": path, "gen": gen, "ts": now_unix() });
    std::fs::write(f, v.to_string()).ok();
}

/// The test hook of the resolve probe: store a fresh cache entry for
/// `bin` in `dir`'s devShell cache (the current generation, a
/// wall-clock now), so a later `resolve` serves the seeded path
/// without a nix call.
#[cfg(test)]
pub(crate) fn seed(dir: &Path, bin: &str, path: &str) {
    entry_store(&cache_path(dir, bin), path, &generation(dir));
}

/// The wall-clock time in seconds, for the cache TTL.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A cache entry is fresh when the generation still matches and the
/// entry is younger than the TTL. The caller also checks the stored
/// path still exists in the store.
fn is_fresh(e: &Entry, gen: &str, now: u64) -> bool {
    e.gen == gen && now.saturating_sub(e.ts) < CACHE_TTL_SECS
}

/// Materialize the store path of `bin` in `dir`'s devShell: run
/// `nix develop` once and ask the shell where `bin` resolves to.
/// The result is the devShell's content-addressed path.
fn materialize(dir: &Path, bin: &str) -> Option<String> {
    let out = run_with_timeout(&materialize_argv(bin), Some(dir), MATERIALIZE_TIMEOUT_SECS).ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| !l.trim().is_empty())?.trim().to_string();
    if !line.starts_with('/') {
        return None;
    }
    Some(line)
}

/// The materialize argv: ask the devShell shell where `bin` is.
fn materialize_argv(bin: &str) -> Vec<String> {
    vec![
        "nix".to_string(),
        "develop".to_string(),
        "-c".to_string(),
        "bash".to_string(),
        "-c".to_string(),
        format!("command -v {bin}"),
    ]
}

/// Run `argv` in `cwd` and capture the output, bounded by `secs`.
/// A child that runs past the bound is killed and the call is an
/// error, so a heavy flake build cannot block the caller forever.
fn run_with_timeout(
    argv: &[String],
    cwd: Option<&Path>,
    secs: u64,
) -> Result<std::process::Output, String> {
    let mut c = std::process::Command::new(&argv[0]);
    for a in &argv[1..] {
        c.arg(a);
    }
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let child = c
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", &argv[0]))?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(format!("wait: {e}")),
        Err(RecvTimeoutError::Timeout) => {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            Err(format!("timed out after {secs}s"))
        }
        Err(RecvTimeoutError::Disconnected) => Err("probe channel closed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envrc_flake_dir_local_dot() {
        let d = std::env::temp_dir().join(format!("rsrs-envrc-dot-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join(".envrc"), "use flake .\n").unwrap();
        assert_eq!(envrc_flake_dir(d.as_path()), Some(d.canonicalize().unwrap()));
    }

    #[test]
    fn envrc_flake_dir_tilde_expands() {
        let d = std::env::temp_dir().join(format!("rsrs-envrc-tilde-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let home = std::env::var("HOME").unwrap();
        std::fs::write(d.join(".envrc"), "use flake ~/someflake\n").unwrap();
        assert_eq!(
            envrc_flake_dir(d.as_path()),
            Some(PathBuf::from(home).join("someflake"))
        );
    }

    #[test]
    fn envrc_flake_dir_walks_up_to_first_envrc() {
        let base =
            std::env::temp_dir().join(format!("rsrs-envrc-walk-{}", std::process::id()));
        let proj = base.join("sub").join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        // No .envrc in proj or sub; the .envrc is in base.
        std::fs::write(base.join(".envrc"), "use flake .\n").unwrap();
        assert_eq!(envrc_flake_dir(proj.as_path()), Some(base.canonicalize().unwrap()));
    }

    #[test]
    fn envrc_flake_dir_none_when_no_envrc() {
        let d = std::env::temp_dir().join(format!("rsrs-envrc-none-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // No .envrc anywhere up the /tmp chain.
        assert_eq!(envrc_flake_dir(d.as_path()), None);
    }

    #[test]
    fn envrc_flake_dir_relative_ref_resolves() {
        let d = std::env::temp_dir().join(format!("rsrs-envrc-rel-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".envrc"), "use flake ./myflake\n").unwrap();
        assert_eq!(
            envrc_flake_dir(d.as_path()),
            Some(d.canonicalize().unwrap().join("myflake"))
        );
    }

    #[test]
    fn fnv1a_is_stable_and_distinct() {
        assert_eq!(fnv1a_64(""), 0xcbf2_9ce4_8422_2325);
        assert_ne!(fnv1a_64("/a"), fnv1a_64("/b"));
        assert_eq!(fnv1a_64("/x/y"), fnv1a_64("/x/y"));
    }

    #[test]
    fn is_fresh_tracks_generation_and_ttl() {
        let gen = "1:2:x86_64";
        let mk = |gen: &str, ts: u64| Entry {
            path: "/nix/store/x/bin/rushi".to_string(),
            gen: gen.to_string(),
            ts,
        };
        let now = 1_000_000;
        // Same generation, young: fresh.
        assert!(is_fresh(&mk(gen, now - 10), gen, now));
        // Same generation, past the TTL: stale.
        assert!(!is_fresh(&mk(gen, now - CACHE_TTL_SECS - 1), gen, now));
        // A different generation (the flake changed): stale even if young.
        assert!(!is_fresh(&mk("9:9:x86_64", now - 10), gen, now));
    }

    #[test]
    fn materialize_argv_asks_the_shell() {
        let want: Vec<String> = vec![
            "nix",
            "develop",
            "-c",
            "bash",
            "-c",
            "command -v rushi",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(materialize_argv("rushi"), want);
    }

    #[test]
    fn run_with_timeout_binds_slow_children() {
        let ok = run_with_timeout(
            &["/bin/sh".to_string(), "-c".to_string(), "printf done".to_string()],
            None,
            5,
        )
        .unwrap();
        assert_eq!(String::from_utf8_lossy(&ok.stdout), "done");
        let slow = run_with_timeout(
            &["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            None,
            1,
        );
        assert!(slow.is_err());
    }

    /// A fresh cache entry (matching generation, path present) is
    /// returned without running nix: seed the cache and resolve.
    #[test]
    fn resolve_serves_a_fresh_cache_entry() {
        let base =
            std::env::temp_dir().join(format!("rsrs-devshell-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let proj = base.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("flake.nix"), "{}").unwrap();
        // A real, existing path so the entry passes the exists check.
        let bin = std::env::temp_dir().join("rsrs-devshell-bin");
        std::fs::write(&bin, "x").unwrap();
        let f = cache_path(&proj, "rushi");
        entry_store(&f, bin.to_str().unwrap(), &generation(&proj));
        // Point the env override off so the cache path is what is
        // served (a set override would shadow the cache).
        let got = resolve(&proj, "rushi").unwrap();
        assert_eq!(got, bin);
    }
}
