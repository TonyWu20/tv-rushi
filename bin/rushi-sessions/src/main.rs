//! `rushi-sessions` — the television-channel backend.
//!
//! Replaces the inline Python embedded in the `rushi-sessions` television
//! channel (`rushi-sessions-channel.nix` in this repo). Two subcommands:
//!
//! - `rushi-sessions source [ROOT...]`
//!   Scan for rushi session directories under the given roots and print
//!   one TSV row per session. Roots may be local or remote. A remote
//!   root is `host:/path` or `user@host:/path` (the host is an ssh
//!   alias). Each distinct host runs the remote `rushi-sessions source`
//!   over ssh once; its rows join the list with the path column
//!   prefixed by `host:`. An unreachable host is skipped with a
//!   stderr warning.:
//!   `status<TAB>name<TAB>repo<TAB>phase<TAB>last<TAB>mtime<TAB>path`.
//!   The roots are directories. Omit them to scan the CWD. Relative
//!   roots join the CWD. Missing roots are skipped with a warning on
//!   stderr. Overlapping roots (one inside another) dedupe by
//!   canonical path. The television channel's `display`/`output` split
//!   on that tab. `--active-only` keeps only the sessions whose loop
//!   pid is alive (ACTIVE). The channel exposes it as a second,
//!   cycling source command.
//!
//! - `rushi-sessions preview <status> <name> <repo> <phase> <last> <mtime> <path>`
//!   Render the TOML preview card for one session: the last user/assistant
//!   messages, the context window usage, and the most recent events. When
//!   `bat` is on PATH the card is
//!   colored with it ($RUSHI_PREVIEW_THEME selects the theme, default
//!   "Catppuccin Macchiato"); otherwise plain TOML is printed.
//!
//! `source` calls `fd` for the directory walk (a hard dependency of the
//! channel) and replaces the channel's previous `python3` reader.

use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// ext_status ids that always show in the preview's recent-event list.
const KEEP: &[&str] = &[
    "loop_phase",
    "model_thinking",
    "goal_paused",
    "goal_active",
    "run.refire",
];

#[derive(Parser)]
#[command(
    name = "rushi-sessions",
    about = "rushi-sessions television channel backend",
    version
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan for rushi sessions under the given roots and print TSV rows.
    /// Without roots, the CWD is the only root.
    Source {
        /// Print only sessions whose loop pid is alive (ACTIVE status).
        #[arg(long)]
        active_only: bool,
        /// Roots to scan for session directories.
        #[arg(value_name = "ROOT")]
        roots: Vec<PathBuf>,
    },
    /// Render the TOML preview card for one session.
    Preview {
        status: String,
        name: String,
        repo: String,
        phase: String,
        last: String,
        /// Epoch seconds of the reference file; carried by the channel,
        /// not used by the card.
        mtime: String,
        /// Session directory path.
        path: String,
    },
}

fn main() {
    // Behave like a normal Unix program when stdout is a pipe that the
    // reader closes early (e.g. `rushi-sessions source | head`): restore
    // the default SIGPIPE disposition so a write to a broken pipe ends the
    // process silently instead of Rust's default EPIPE panic trace.
    #[cfg(unix)]
    {
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
    }

    let args = Args::parse();
    match args.command {
        Command::Source { active_only, roots } => source(active_only, roots),
        Command::Preview {
            status,
            name,
            repo,
            phase,
            last,
            mtime,
            path,
        } => preview(&status, &name, &repo, &phase, &last, &mtime, &path),
    }
}

// ── source ───────────────────────────────────────────────────────────

struct Row {
    status: &'static str,
    name: String,
    repo: String,
    phase: String,
    last: String,
    mtime: i64,
    path: String,
}

fn source(active_only: bool, args: Vec<PathBuf>) {
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

// ── remote (ssh) roots ──────────────────────────────────────────────

/// A remote root written as `host:/path` or `user@host:/path`. The
/// host is an ssh alias from the user's `~/.ssh/config`. The text
/// before the first `:` is the host, the rest is the remote path.
/// Local TSV paths are absolute, so a colon before the first `/` can
/// only come from a remote prefix.
fn split_remote(path: &str) -> Option<(&str, &str)> {
    let (host, rest) = path.split_once(':')?;
    if host.is_empty() || rest.is_empty() {
        return None;
    }
    if host.contains('/') || host.starts_with('@') || host.matches('@').count() > 1 {
        return None;
    }
    Some((host, rest))
}

/// The remote roots of one host, in request order.
struct RemoteGroup {
    host: String,
    roots: Vec<String>,
}

/// Split requested roots into a local list and per-host remote
/// groups. Group order is first-seen; their roots keep request order.
fn split_roots(roots: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<RemoteGroup>) {
    let mut local: Vec<PathBuf> = Vec::new();
    let mut groups: Vec<RemoteGroup> = Vec::new();
    for r in roots {
        let s = r.to_string_lossy().into_owned();
        if let Some((host, rp)) = split_remote(&s) {
            let host = host.to_string();
            let rp = rp.to_string();
            let mut matched = false;
            for g in groups.iter_mut() {
                if g.host == host {
                    g.roots.push(rp.clone());
                    matched = true;
                    break;
                }
            }
            if !matched {
                groups.push(RemoteGroup { host, roots: vec![rp] });
            }
        } else {
            local.push(r);
        }
    }
    (local, groups)
}

/// The ssh executable: `ssh`, or the `RUSHI_SESSIONS_SSH` override
/// (tests use a fake).
fn ssh_binary() -> String {
    std::env::var("RUSHI_SESSIONS_SSH").unwrap_or_else(|_| "ssh".into())
}

/// The remote `rushi-sessions` binary name. A non-interactive ssh
/// shell does not source shell init files, so a Nix profile binary is
/// not on PATH. Set `RUSHI_SESSIONS_REMOTE_BIN` to the remote absolute
/// path when the binary lives in a Nix profile. The value must be a
/// bare path without shell metacharacters.
fn remote_binary() -> String {
    std::env::var("RUSHI_SESSIONS_REMOTE_BIN").unwrap_or_else(|_| "rushi-sessions".into())
}

/// Single-quote a word for a remote shell command string.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Run one remote command string over ssh (stdout and stderr captured).
fn run_remote(host: &str, command: &str) -> std::io::Result<std::process::Output> {
    std::process::Command::new(ssh_binary())
        .arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg(host)
        .arg(command)
        .output()
}

/// Run `source` on a remote host and return its TSV lines. An
/// unreachable host or a failed remote scan warns on stderr and yields
/// no lines; the other hosts still scan.
fn remote_source_lines(group: &RemoteGroup, active_only: bool) -> Vec<String> {
    // The remote command string, run by the remote login shell. Roots
    // are bare words, so the remote shell expands a leading `~`
    // (matching the local channel behavior).
    let flag = if active_only { "--active-only " } else { "" };
    let mut cmd = String::new();
    cmd.push_str(&remote_binary());
    cmd.push_str(" source ");
    cmd.push_str(flag);
    let mut first = true;
    for r in &group.roots {
        if !first {
            cmd.push(' ');
        }
        first = false;
        cmd.push_str(r);
    }
    match run_remote(&group.host, &cmd) {
        Ok(out) => {
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                eprintln!(
                    "rushi-sessions: skipping host: {} (remote source failed: {})",
                    group.host,
                    err.trim()
                );
                return Vec::new();
            }
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim_end)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        }
        Err(e) => {
            eprintln!(
                "rushi-sessions: skipping unreachable host: {} ({})",
                group.host, e
            );
            Vec::new()
        }
    }
}

/// One remote TSV line into a local `Row`, with the `host:` prefix on
/// the path column. A short line or a bad mtime drops the row. A
/// status that is not `ACTIVE` normalizes to `IDLE` (an older remote
/// binary must not break the list).
fn parse_remote_line(line: &str, host: &str) -> Option<Row> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() != 7 {
        return None;
    }
    let mtime: i64 = f[5].parse().ok()?;
    Some(Row {
        status: if f[0] == "ACTIVE" { "ACTIVE" } else { "IDLE" },
        name: f[1].to_string(),
        repo: f[2].to_string(),
        phase: f[3].to_string(),
        last: f[4].to_string(),
        mtime,
        path: format!("{}:{}", host, f[6]),
    })
}

/// The remote preview: run the remote binary's preview over ssh and
/// stream its card to stdout. A failed ssh warns on stderr.
fn preview_remote(host: &str, cols: &[&str]) {
    let mut cmd = String::new();
    cmd.push_str(&remote_binary());
    cmd.push_str(" preview");
    for c in cols {
        cmd.push(' ');
        cmd.push_str(&shell_quote(c));
    }
    match run_remote(host, &cmd) {
        Ok(out) => {
            use std::io::Write;
            let mut so = std::io::stdout();
            let _ = so.write_all(&out.stdout);
            let _ = so.flush();
            if !out.stderr.is_empty() {
                let mut se = std::io::stderr();
                let _ = se.write_all(&out.stderr);
            }
            std::process::exit(out.status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!(
                "rushi-sessions: remote preview failed for {} ({e})",
                host
            );
        }
    }
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

/// Read the last `n` bytes of a file.
fn read_tail(p: &str, n: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(p)?;
    let size = f.metadata()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(n as u64)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

/// The value of the most recent `loop_phase` ext_status in the log tail.
/// Mirrors the old string scan: the last `"id":"loop_phase"` marker, then
/// the following `"value":"..."`.
fn last_phase(ev: &str) -> String {
    let data = match read_tail(ev, 100_000) {
        Ok(d) => d,
        Err(_) => return "?".into(),
    };
    let idm = b"\"id\":\"loop_phase\"";
    let i = match data.windows(idm.len()).rposition(|w| w == idm) {
        Some(x) => x,
        None => return "?".into(),
    };
    let vmark = b"\"value\":\"";
    let jrel = match data[i..].windows(vmark.len()).position(|w| w == vmark) {
        Some(x) => x,
        None => return "?".into(),
    };
    let vstart = i + jrel + vmark.len();
    let kend = match data[vstart..].iter().position(|b| *b == b'"') {
        Some(x) => vstart + x,
        None => return "?".into(),
    };
    String::from_utf8_lossy(&data[vstart..kend]).into_owned()
}

/// Whether a pid is alive (`kill(pid, 0)` succeeds).
fn alive(pid: &str) -> bool {
    if pid.is_empty() {
        return false;
    }
    let p: i32 = match pid.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    unsafe { libc::kill(p, 0) == 0 }
}

/// Whole-epoch-seconds mtime of a path, or 0 if it cannot be read.
fn mtime_secs(p: &str) -> i64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|mt| {
            mt.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// Format epoch seconds as local `mm-dd HH:MM`.
fn fmt_local(secs: i64) -> String {
    match Local.timestamp_opt(secs, 0).single() {
        Some(dt) => dt.format("%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

// ── preview ──────────────────────────────────────────────────────────

fn preview(
    status: &str,
    name: &str,
    repo: &str,
    phase: &str,
    last: &str,
    mtime: &str,
    path: &str,
) {
    if let Some((host, rpath)) = split_remote(path) {
        preview_remote(host, &[status, name, repo, phase, last, mtime, rpath]);
        return;
    }
    let pid = fs::read_to_string(Path::new(path).join("loop.pid"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // Refresh the status from the live pid when one is present.
    let status = if !pid.is_empty() {
        if alive(&pid) {
            "ACTIVE"
        } else {
            "IDLE"
        }
    } else {
        status
    };

    let epath = Path::new(path).join("events.jsonl");
    let events = if epath.exists() {
        read_events(&epath)
    } else {
        Vec::new()
    };

    let mut think = String::new();
    let mut last_user = String::new();
    let mut last_assistant = String::new();
    for o in &events {
        let t = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t == "ext_status" && o.get("id").and_then(|v| v.as_str()) == Some("model_thinking") {
            think = value_to_str(o.get("value"));
        }
        if t == "user_message" {
            last_user = o
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        if t == "assistant_message" {
            last_assistant = o
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
    }

    let mut out: Vec<String> = Vec::new();
    out.push("[rushi-session]".into());
    out.push(format!("name    = {}", json_str(name)));
    out.push(format!("repo    = {}", json_str(repo)));
    out.push(format!("status  = {}", json_str(status)));
    out.push(format!("pid     = {}", json_str(&pid)));
    out.push(format!("phase   = {}", json_str(phase)));
    if !think.is_empty() {
        out.push(format!("think   = {}", json_str(&think)));
    }
    out.push(format!("updated = {}", json_str(last)));
    if let Some((in_tok, out_tok, _cached)) = last_usage(&events) {
        let used = in_tok.saturating_add(out_tok);
        out.push(format!(
            "usage   = {}",
            json_str(&usage_line(used, context_window()))
        ));
    }
    out.push(String::new());
    out.push("[last-user-message]".into());
    out.push(format!("content = {}", json_str(&clip(&last_user, 400))));
    out.push(String::new());
    out.push("[last-assistant-message]".into());
    out.push(format!(
        "content = {}",
        json_str(&clip(&last_assistant, 400))
    ));

    // The last 40 events, reduced to (ts, type, brief), then the last 5 in
    // log order (the old `events[-5:]`), so ts ties keep log order.
    let mut last40: Vec<&Value> = events.iter().rev().take(40).collect();
    last40.reverse();
    let tuples: Vec<(String, String, String)> = last40
        .iter()
        .filter_map(|o| {
            let brief = brief(o)?;
            let ts_raw = o.get("ts").map(ts_to_str).unwrap_or_default();
            let ts = ts_slice(&ts_raw);
            let ty = o
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            Some((ts, ty, brief))
        })
        .collect();
    let recent: Vec<&(String, String, String)> =
        tuples.iter().skip(tuples.len().saturating_sub(5)).collect();
    for (ts, ty, brief) in recent {
        out.push(String::new());
        out.push("[[recent-event]]".into());
        out.push(format!("ts    = {}", json_str(ts)));
        out.push(format!("type  = {}", json_str(ty)));
        out.push(format!("brief = {}", json_str(&clip(brief, 80))));
    }

    let card = out.join("\n") + "\n";

    // Optional colorization through `bat` when it is available.
    match which("bat") {
        Some(bat) => {
            let theme = std::env::var("RUSHI_PREVIEW_THEME")
                .unwrap_or_else(|_| "Catppuccin Macchiato".to_string());
            let mut child = std::process::Command::new(&bat)
                .args([
                    "-l", "toml", "--theme", &theme, "--color", "always", "--style", "plain",
                ])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok();
            if let Some(mut c) = child.take() {
                use std::io::Write;
                if let Some(mut stdin) = c.stdin.take() {
                    let _ = stdin.write_all(card.as_bytes());
                }
                match c.wait_with_output() {
                    Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                        print!("{}", String::from_utf8_lossy(&o.stdout));
                        return;
                    }
                    _ => {}
                }
            }
            print!("{}", card);
        }
        None => print!("{}", card),
    }
}

/// Parse the JSONL event log, skipping lines that do not parse.
fn read_events(p: &Path) -> Vec<Value> {
    let content = match fs::read_to_string(p) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .collect()
}

/// The usage of the last `assistant_message` that carries real usage:
/// `(input_tokens, output_tokens, cached_tokens)`. Messages whose `usage`
/// is missing, null, or all-zero are skipped.
fn last_usage(events: &[Value]) -> Option<(u64, u64, u64)> {
    for o in events.iter().rev() {
        if o.get("type").and_then(|v| v.as_str()) != Some("assistant_message") {
            continue;
        }
        let u = match o.get("usage") {
            Some(Value::Object(_)) => o.get("usage").unwrap(),
            _ => continue,
        };
        let f = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        let in_tok = f("input_tokens");
        let out_tok = f("output_tokens");
        let cached = f("cached_tokens");
        if in_tok == 0 && out_tok == 0 {
            continue;
        }
        return Some((in_tok, out_tok, cached));
    }
    None
}

/// The text of the `usage` line: `used/window tokens (pct%)` when the
/// context window is known, else `used tokens`.
fn usage_line(used: u64, window: Option<u64>) -> String {
    match window {
        Some(w) if w > 0 => {
            let pct =
                (u128::from(used).saturating_mul(100) + u128::from(w) / 2) / u128::from(w);
            format!("{used}/{w} tokens ({pct}%)")
        }
        _ => format!("{used} tokens"),
    }
}

/// The context window in tokens. Resolution order: the
/// `RUSHI_CONTEXT_WINDOW` env var (a positive integer), then the active
/// config of the `rushi` binary on PATH (`rushi config` stdout): the
/// active model's `context_tokens`, then `limits.context_budget_tokens`.
/// None when nothing resolves.
fn context_window() -> Option<u64> {
    if let Ok(v) = std::env::var("RUSHI_CONTEXT_WINDOW") {
        if let Ok(n) = v.trim().parse::<u64>() {
            if n > 0 {
                return Some(n);
            }
        }
    }
    let out = std::process::Command::new("rushi")
        .arg("config")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    context_window_from_config(&String::from_utf8_lossy(&out.stdout))
}

/// The context window in tokens parsed from `rushi config` output:
/// `[model."<active>"] context_tokens`, then the base `[model]`
/// `context_tokens`, then `limits.context_budget_tokens`.
fn context_window_from_config(text: &str) -> Option<u64> {
    let v: toml::Value = toml::from_str(text).ok()?;
    let num = |v: &toml::Value| v.as_integer().filter(|n| *n > 0).map(|n| n as u64);
    let model = v.get("model");
    let active = v
        .get("active")
        .and_then(|a| a.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if !active.is_empty() {
        if let Some(n) = model
            .and_then(|m| m.get(active))
            .and_then(|p| p.get("context_tokens"))
            .and_then(|v| num(v))
        {
            return Some(n);
        }
    }
    if let Some(n) = model.and_then(|m| m.get("context_tokens")).and_then(num) {
        return Some(n);
    }
    v.get("limits")
        .and_then(|l| l.get("context_budget_tokens"))
        .and_then(num)
}

/// `str(o.get("value",""))` — the value of an ext_status as a string.
fn value_to_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "None".into(),
        None => String::new(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// A one-line summary of an event, or None to skip it. Mirrors the
/// channel's `brief()` for tool_call, tool_result and ext_status.
fn brief(o: &Value) -> Option<String> {
    let t = o.get("type").and_then(|v| v.as_str()).unwrap_or("?");
    match t {
        "tool_call" => {
            let a = match o.get("arguments") {
                Some(Value::Null) | None => Value::Object(serde_json::Map::new()),
                Some(v) => v.clone(),
            };
            let s = if a.is_object() {
                a.get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            };
            let s = if s.is_empty() {
                serde_json::to_string(&a).unwrap_or_default()
            } else {
                s
            };
            let name = o
                .get("name")
                .and_then(|n| n.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("?");
            Some(format!("{} {}", name, s).replace('\n', " "))
        }
        "tool_result" => {
            let is_err = o.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut s: String = if is_err { "error".into() } else { "ok".into() };
            if let Some(b) = o.get("bytes").and_then(|v| v.as_i64()) {
                s = format!("{} {}b", s, b);
            }
            Some(s)
        }
        "ext_status" => {
            let id = o.get("id").and_then(|v| v.as_str()).unwrap_or("status");
            let v = o.get("value");
            if matches!(v, Some(Value::Object(_)) | Some(Value::Array(_))) {
                return None;
            }
            let shown =
                KEEP.contains(&id) || id.ends_with(".error") || id == "hook.exhausted.handle";
            if !shown {
                return None;
            }
            match v {
                None | Some(Value::Null) => Some(id.to_string()),
                Some(Value::String(s)) => Some(format!("{}={}", id, s)),
                Some(other) => Some(format!(
                    "{}={}",
                    id,
                    serde_json::to_string(other).unwrap_or_default()
                )),
            }
        }
        _ => {
            let c = o.get("content").and_then(|v| v.as_str()).unwrap_or("");
            Some(c.replace('\n', " "))
        }
    }
}

/// `s[:n]` character clip with a trailing " ..." when truncated.
fn clip(s: &str, n: usize) -> String {
    let s: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > n {
        let mut out: String = chars[..n].iter().collect();
        out.push_str(" ...");
        out
    } else {
        s
    }
}

/// `str(ts)[11:19]` — the HH:MM:SS slice of an ISO timestamp (or the
/// leftover of a relative one), empty when the slice is out of range.
fn ts_slice(ts: &str) -> String {
    let chars: Vec<char> = ts.chars().collect();
    let start = 11.min(chars.len());
    let stop = 19.min(chars.len());
    if start < stop {
        chars[start..stop].iter().collect()
    } else {
        String::new()
    }
}

fn ts_to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

/// A JSON string (the `q()` helper): double-quoted, ASCII-escaped the way
/// Python's `json.dumps` does, keeping non-ASCII as is.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Find an executable on PATH (the `shutil.which` equivalent).
fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let paths = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&paths) {
        let cand = dir.join(name);
        if let Ok(m) = fs::metadata(&cand) {
            if m.is_file() && m.permissions().mode() & 0o111 != 0 {
                return Some(cand);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_str_escapes() {
        assert_eq!(json_str("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_str("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_str("a\nb"), "\"a\\nb\"");
        assert_eq!(json_str("héllo"), "\"héllo\"");
    }

    #[test]
    fn clip_truncates_by_chars() {
        let s = "x".repeat(200);
        let c = clip(&s, 400);
        assert_eq!(c.len(), 200);
        let c = clip(&"a".repeat(50), 10);
        assert_eq!(c, "aaaaaaaaaa ...");
    }

    #[test]
    fn ts_slice_iso() {
        assert_eq!(ts_slice("2026-09-17T21:13:24Z"), "21:13:24");
        assert_eq!(ts_slice(""), "");
    }

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
    fn split_remote_forms() {
        assert_eq!(split_remote("host:/x"), Some(("host", "/x")));
        assert_eq!(split_remote("u@host:/x"), Some(("u@host", "/x")));
        assert_eq!(split_remote("/a:b"), None);
        assert_eq!(split_remote("/x"), None);
        assert_eq!(split_remote("@host:/x"), None);
        assert_eq!(split_remote("a@b@h:/x"), None);
        assert_eq!(split_remote("host:"), None);
        assert_eq!(split_remote(":x"), None);
        assert_eq!(split_remote("a:b/x"), Some(("a", "b/x")));
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("ab"), "'ab'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn split_roots_groups_by_host() {
        let (local, groups) = split_roots(vec![
            PathBuf::from("/local"),
            PathBuf::from("h1:/a"),
            PathBuf::from("u@h2:/c"),
            PathBuf::from("h1:/b"),
        ]);
        assert_eq!(local, vec![PathBuf::from("/local")]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].host, "h1");
        assert_eq!(groups[0].roots, vec!["/a".to_string(), "/b".to_string()]);
        assert_eq!(groups[1].host, "u@h2");
        assert_eq!(groups[1].roots, vec!["/c".to_string()]);
    }

    #[test]
    fn parse_remote_line_prefixes_path() {
        let line = "ACTIVE\tfoo\trepoA\ttools\t10-02 14:11\t1760000000\t/export/repoA/sessions/foo";
        let row = parse_remote_line(line, "host").unwrap();
        assert_eq!(row.path, "host:/export/repoA/sessions/foo");
        assert_eq!(row.status, "ACTIVE");
        assert_eq!(row.mtime, 1760000000);
        assert_eq!(row.name, "foo");
    }

    #[test]
    fn parse_remote_line_rejects_short_lines() {
        assert!(parse_remote_line("a\tb", "host").is_none());
    }

    #[test]
    fn parse_remote_line_normalizes_status() {
        let line = "WEIRD\tfoo\trepo\t?\t?\t5\t/p";
        let row = parse_remote_line(line, "h").unwrap();
        assert_eq!(row.status, "IDLE");
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
    fn last_usage_picks_last_with_data() {
        let e1 = serde_json::json!({
            "type": "assistant_message",
            "usage": {"input_tokens": 100, "output_tokens": 10, "cached_tokens": 0}
        });
        let e2 = serde_json::json!({
            "type": "assistant_message",
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        let e3 = serde_json::json!({
            "type": "assistant_message",
            "usage": {"input_tokens": 200, "output_tokens": 20, "cached_tokens": 5}
        });
        let evts = vec![e1.clone(), e2.clone(), e3.clone()];
        assert_eq!(last_usage(&evts), Some((200, 20, 5)));
        // All-zero and missing usage are skipped.
        let evts = vec![e2, e1];
        assert_eq!(last_usage(&evts), Some((100, 10, 0)));
        let e4 = serde_json::json!({"type": "assistant_message"});
        assert_eq!(last_usage(&vec![e4]), None);
        assert_eq!(last_usage(&[]), None);
    }

    #[test]
    fn usage_line_formats_with_and_without_window() {
        assert_eq!(
            usage_line(109_967, Some(262_144)),
            "109967/262144 tokens (42%)"
        );
        assert_eq!(usage_line(500, None), "500 tokens");
        // A window smaller than the usage reports over 100%.
        assert_eq!(usage_line(200, Some(100)), "200/100 tokens (200%)");
        assert_eq!(usage_line(10_000, Some(0)), "10000 tokens");
    }

    #[test]
    fn config_window_prefers_active_model() {
        let text = r#"
[active]
model = "m1"
[model]
context_tokens = 111
[model.m1]
context_tokens = 222
[limits]
context_budget_tokens = 333
"#;
        assert_eq!(context_window_from_config(text), Some(222));
    }

    #[test]
    fn config_window_falls_backs_down() {
        // No active model: the base [model] section wins.
        let text = r#"
[model]
context_tokens = 111
[limits]
context_budget_tokens = 333
"#;
        assert_eq!(context_window_from_config(text), Some(111));
        // No [model] at all: the limits budget wins.
        let text = r#"
[limits]
context_budget_tokens = 333
"#;
        assert_eq!(context_window_from_config(text), Some(333));
        // Nothing usable: None.
        let text = r#"
[limits]
read_limit = 2000
"#;
        assert_eq!(context_window_from_config(text), None);
        // Invalid TOML: None.
        assert_eq!(context_window_from_config("not toml [[["), None);
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
