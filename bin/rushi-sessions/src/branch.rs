//! The tree-view actions on one session: `rewind` (append a `rewind`
//! marker, the tree's "Rewind without summary") and `summarize`
//! (the one-shot fork-and-summarize, the two branch-summarize options
//! of the tree view, rushi-tui #37/#38: append the `rewind` marker
//! at the picked event, then run the kernel's `compact --branch`).
//!
//! Both call the kernel's own binaries, the way the TUI does: the
//! marker is appended through the kernel's `log` binary (the one
//! locked single-write append, FT-005), the branch summary through
//! the kernel's `compact` binary. The binaries are the siblings of
//! `rushi` on PATH: the kernel's Nix package ships one bin dir, and
//! the per-user symlink farm does not expose `log` or `compact`
//! individually.
//!
//! Both actions need the loop to be idle, the TUI's guard. The probe
//! is the kernel's `.loop.lock` flock, the same authority `rushi
//! run` branches on: a held lock means a live loop.

use std::fs;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::events::numbered_events;
use crate::prompt::{is_blank, new_prompt_file, stty_save, which};

/// The stage entry of the `rewind` command. Without DIR, the CWD is
/// the session dir (the events channel runs in it).
pub(crate) fn rewind(dir: Option<PathBuf>, seq: u64) -> i32 {
    let base = match resolve_session_dir(dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("rushi-sessions-events: {e}");
            return 1;
        }
    };
    match probe_idle(&base) {
        Ok(true) => {}
        Ok(false) => {
            println!("loop busy, wait for the step");
            return 1;
        }
        Err(e) => {
            eprintln!("rushi-sessions: {e}");
            return 1;
        }
    }
    // The mode follows the TUI's rule: a user-message target rewinds
    // `before` it (the message leaves the context; the user re-sends
    // it), every other target rewinds `on` it (the target stays in
    // the context).
    let rows = numbered_events(&base);
    let ty = rows
        .iter()
        .find(|(s, _)| *s == seq)
        .and_then(|(_, v)| v.get("type").and_then(|t| t.as_str()));
    let last = rows.last().map(|(s, _)| *s).unwrap_or(0);
    if seq > last {
        eprintln!("rushi-sessions: seq {seq} is past the end of the log ({last})");
        return 1;
    }
    let mode = if ty == Some("user_message") { "before" } else { "on" };
    if let Err(e) = append_rewind_marker(&base, seq, mode) {
        eprintln!("rushi-sessions: {e}");
        return 1;
    }
    println!("rewound to seq {seq} ({mode})");
    if mode == "before" {
        println!(
            "the user message at seq {seq} left the context; send it again to re-ask"
        );
    }
    0
}

/// The stage entry of the `summarize` command. With `prompt`, the
/// instruction is first typed in the editor on a .md file (the
/// send_message prompt design, in the fork's pane); a blank file
/// cancels.
pub(crate) fn summarize(dir: Option<PathBuf>, seq: u64, prompt: bool) -> i32 {
    let base = match resolve_session_dir(dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("rushi-sessions-events: {e}");
            return 1;
        }
    };
    match probe_idle(&base) {
        Ok(true) => {}
        Ok(false) => {
            println!("loop busy, wait for the step");
            return 1;
        }
        Err(e) => {
            eprintln!("rushi-sessions: {e}");
            return 1;
        }
    }
    // The one-shot fork-and-summarize (rushi-tui #37/#38): the
    // picked entry becomes the rewind target. No pre-existing marker
    // is needed, and the pick may sit on the active path or in an
    // abandoned span (the kernel re-enters the branch). The picked
    // event stays in the context, so the marker is always mode `on`.
    let rows = numbered_events(&base);
    let last = rows.last().map(|(s, _)| *s).unwrap_or(0);
    if seq > last {
        eprintln!("rushi-sessions: seq {seq} is past the end of the log ({last})");
        return 1;
    }
    let prompt_text: Option<String> = if prompt {
        match editor_instruction(&base) {
            Some(t) => Some(t),
            None => {
                println!("No instruction entered, nothing summarized.");
                return 1;
            }
        }
    } else {
        None
    };
    // The marker must land in the log before the compact child reads
    // it, so the append comes first and blocks.
    if let Err(e) = append_rewind_marker(&base, seq, "on") {
        eprintln!("rushi-sessions: {e}");
        return 1;
    }
    // One `compact --branch` call, the TUI outcome. It runs in the
    // session's recorded working dir so the config tier finds the
    // same project config the loop uses.
    let compact = kernel_bin("compact");
    let w = local_workdir(&base);
    let mut c = Command::new(&compact);
    c.arg(&base).arg("--branch");
    if let Some(p) = &prompt_text {
        c.arg("--prompt").arg(p);
    }
    c.current_dir(&w);
    match c.status() {
        Ok(s) if s.success() => {
            println!("summarized the branch left of seq {seq}");
            0
        }
        Ok(s) => {
            eprintln!("rushi-sessions: {compact} exited with {s}");
            1
        }
        Err(e) => {
            eprintln!("rushi-sessions: cannot start {compact}: {e}");
            1
        }
    }
}

/// The session dir of one command: the DIR argument, or the CWD when
/// the argument is absent (the events channel runs in the session
/// dir). The guard matches the send_message action: a gone CWD is a
/// visible error, not a ghost session.
fn resolve_session_dir(dir: Option<PathBuf>) -> Result<PathBuf, String> {
    let d = match dir {
        Some(d) => d,
        None => std::env::current_dir().map_err(|_| {
            "cannot resolve the session dir (the channel CWD is gone); re-open it from a live directory.".to_string()
        })?,
    };
    if !d.join("events.jsonl").is_file() {
        return Err(format!(
            "{} is not a session dir (no events.jsonl)",
            d.display()
        ));
    }
    std::fs::canonicalize(&d).map_err(|e| format!("cannot resolve {}: {e}", d.display()))
}

/// The loop-liveness probe: the kernel's `.loop.lock` flock. A held
/// lock means a live loop (the kernel releases the flock when the
/// holder dies, so a stale lock is impossible). The probe creates
/// the file when it is missing, the way the kernel's `rushi run`
/// does.
fn probe_idle(dir: &Path) -> Result<bool, String> {
    let p = dir.join(".loop.lock");
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&p)
        .map_err(|e| format!("cannot open {}: {e}", p.display()))?;
    let fd = f.as_raw_fd();
    let r = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        let _ = unsafe { libc::flock(fd, libc::LOCK_UN) };
        Ok(true)
    } else {
        Ok(false)
    }
}

/// The kernel binaries next to `rushi` (the Nix package ships one
/// bin dir): the per-user farm does not symlink `log` or `compact`,
/// so the sibling of the resolved `rushi` is the source of truth. A
/// missing sibling falls back to the bare name on PATH.
fn kernel_bin(name: &str) -> String {
    if let Some(rushi) = which("rushi") {
        if let Some(d) = rushi.parent() {
            let p = d.join(name);
            if p.is_file() {
                return p.to_string_lossy().into_owned();
            }
        }
    }
    name.to_string()
}

/// The `rewind` marker of one pick: the kernel's typed event, one
/// JSON line. `reason` is `tv_pick`, the channel's counterpart of the
/// TUI's `tui_pick`.
fn rewind_marker(seq: u64, mode: &str) -> String {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    serde_json::json!({
        "v": 1,
        "type": "rewind",
        "ts": ts,
        "target_seq": seq,
        "mode": mode,
        "reason": "tv_pick",
    })
    .to_string()
}

/// Append the one `rewind` marker of a pick through the kernel's
/// `log` binary (the locked single-write append, FT-005), and wait
/// for it. The marker must land in the log before a `compact` child
/// reads it, so the append is synchronous.
fn append_rewind_marker(base: &Path, seq: u64, mode: &str) -> Result<(), String> {
    let marker = rewind_marker(seq, mode);
    let log_bin = kernel_bin("log");
    let mut c = Command::new(&log_bin);
    c.arg("--session").arg(base);
    c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::inherit());
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot start {log_bin}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut line = marker;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("cannot write the rewind marker: {e}"))?;
    }
    let s = child
        .wait()
        .map_err(|e| format!("cannot wait on {log_bin}: {e}"))?;
    if s.success() {
        Ok(())
    } else {
        Err(format!("{log_bin} exited with {s}"))
    }
}

/// The custom-summarize instruction: the editor on a .md file, the
/// send_message prompt design in the fork's pane (no floating pane
/// here). `None` when the file comes back blank, the caller's
/// cancel. The editor opens in the session's recorded working dir.
fn editor_instruction(base: &Path) -> Option<String> {
    let w = local_workdir(base);
    let file = new_prompt_file("summarize", None, Some(&w)).ok()?;
    let editor = std::env::var_os("EDITOR")
        .filter(|e| !e.is_empty())
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "nvim".into());
    let st = stty_save();
    let mut c = Command::new(&editor);
    c.arg(&file)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if Path::new(&w).is_dir() {
        c.current_dir(&w);
    }
    if let Err(e) = c.status() {
        eprintln!("rushi-sessions: cannot start editor {editor}: {e}");
    }
    if !st.is_empty() {
        let _ = Command::new("stty").arg(&st).output();
    }
    let text = fs::read_to_string(&file).unwrap_or_default();
    let _ = fs::remove_file(&file);
    let t = text.trim().to_string();
    if is_blank(&t) {
        None
    } else {
        Some(t)
    }
}

/// The session's recorded working dir (the `cwd` file), or the
/// project dir (dirname(dirname(session dir))) when the file is
/// missing or stale.
fn local_workdir(base: &Path) -> String {
    let w = fs::read_to_string(base.join("cwd")).ok().map(|s| s.trim().to_string());
    match w {
        Some(w) if !w.is_empty() && Path::new(&w).is_dir() => w,
        _ => base
            .parent()
            .and_then(|d| d.parent())
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into()),
    }
}
