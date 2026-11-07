//! The tree-view actions on one session: `rewind` (append a `rewind`
//! marker, the tree's "Rewind without summary") and `summarize`
//! (the one-shot fork-and-summarize, the two branch-summarize options
//! of the tree view, rushi-tui #37/#38: append the `rewind` marker
//! at the picked event, then run the kernel's `compact --branch`).
//!
//! Both call the kernel's own binaries, the way the TUI does: the
//! marker is appended through the kernel's log-appender binary
//! (the one locked single-write append, FT-005), the branch
//! summary through the kernel's `compact` binary. The binaries are
//! the siblings of `rushi`: the kernel's Nix package ships one bin
//! dir, and the per-user symlink farm does not expose the kernel
//! bins individually. The log appender is `rushi-log` since the
//! kernel's 58 rename (the reserved name `log` of 57); the legacy
//! `log` name of the older kernels is kept as the second entry of
//! the name list. When the session's project dir carries a flake
//! and `nix` is on PATH and a name resolves from the devShell
//! PATH, the launch is `nix develop -c <bin>` in that dir
//! (decision 88, the 90 resolvability fallback); otherwise the
//! PATH sibling of the resolved `rushi`.
//!
//! Both actions need the loop to be idle, the TUI's guard. The probe
//! is the kernel's `.loop.lock` flock, the same authority `rushi
//! run` branches on: a held lock means a live loop.
//!
//! The settled target (kernel PR #66, the P8 write guard): the
//! kernel refuses a `rewind` marker whose projected context
//! strands a tool pair, at the write boundary
//! (`rushi_common::rewind::guard_rewind`, P8 in
//! rewind-fork-design.md). So a pick on a step whose tool results
//! sit in the abandoned span is adjusted: the marker lands on the
//! step's last result, the design's finished-step target (section
//! 1), and the kernel keeps it. The adjustment is visible on
//! stdout. The unfixable case (a stranded pair the forward move
//! cannot cover) is refused before the write, and the refusal
//! names the stranded pair, the kernel's P8 phrasing.

use std::fs;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rushi_common::rewind::{active_ranges, parse_rewind_event, seq_in_ranges, RewindRef};
use serde_json::Value;

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
    let mode = if ty == Some("user_message") {
        "before"
    } else {
        "on"
    };
    // The P8-safe settled target: a pick on a step whose tool
    // results sit in the abandoned span would strand a pair, and
    // the kernel's P8 write guard (kernel PR #66) refuses the
    // marker. The target moves to the step's last result, the
    // design's finished-step target, so the kernel keeps the
    // marker and the context reads up to the step plus the new
    // message.
    let p8 = settled_target(&rows, seq, mode);
    if p8.unfixable {
        let pair = p8
            .stranded
            .first_stranded()
            .unwrap_or_else(|| "a tool pair".into());
        eprintln!(
            "rushi-sessions: the rewind to seq {seq} would strand {pair}; the kernel's P8 write guard refuses the marker (rewind-fork-design.md P8) and the branch stays in the context. Pick a settled event: a user_message, a tool_result, or an assistant_message with no outstanding tool calls."
        );
        return 1;
    }
    if let Err(e) = append_rewind_marker(&base, p8.target, p8.mode) {
        eprintln!("rushi-sessions: {e}");
        return 1;
    }
    if p8.moved {
        println!(
            "the pick at seq {seq} strands the step's tool result; the marker lands on seq {}, the settled point the kernel's P8 guard keeps",
            p8.target
        );
    }
    println!("rewound to seq {} ({})", p8.target, p8.mode);
    if p8.mode == "before" {
        println!("the user message at seq {} left the context; send it again to re-ask", p8.target);
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
    // The P8-safe settled target, like the `rewind` command. The
    // kernel's P8 write guard refuses a stranded marker, so the
    // one-shot refuses an unfixable pick before the write: there is
    // no branch to summarize when the rewind cannot land.
    let p8 = settled_target(&rows, seq, "on");
    if p8.unfixable {
        let pair = p8
            .stranded
            .first_stranded()
            .unwrap_or_else(|| "a tool pair".into());
        eprintln!(
            "rushi-sessions: the pick at seq {seq} would strand {pair}; the kernel's P8 write guard refuses the marker (rewind-fork-design.md P8), so nothing is summarized. Pick a settled event: a user_message, a tool_result, or an assistant_message with no outstanding tool calls."
        );
        return 1;
    }
    // The marker must land in the log before the compact child reads
    // it, so the append comes first and blocks. A refused or failed
    // write stops the one-shot: the branch summary reads the marker
    // the append just wrote.
    if let Err(e) = append_rewind_marker(&base, p8.target, p8.mode) {
        eprintln!("rushi-sessions: {e}");
        return 1;
    }
    if p8.moved {
        println!(
            "the pick at seq {seq} strands the step's tool result; the marker lands on seq {}, the settled point the kernel's P8 guard keeps",
            p8.target
        );
    }
    // One `compact --branch` call, the TUI outcome. It runs in the
    // session's recorded working dir so the config tier finds the
    // same project config the loop uses. The project dir carries a
    // flake: the launch is `nix develop -c compact` in that dir
    // (decision 88); the launch dir is the flake dir, not the
    // recorded working dir.
    let w = local_workdir(&base);
    let projdir = base.parent().and_then(|p| p.parent());
    let (argv, cwd) = kernel_launch(&["compact"], projdir, Some(PathBuf::from(&w)));
    let label = argv[0].clone();
    let mut c = launch_command(&argv, &cwd);
    c.arg(&base).arg("--branch");
    if let Some(p) = &prompt_text {
        c.arg("--prompt").arg(p);
    }
    match c.status() {
        Ok(s) if s.success() => {
            println!("summarized the branch left of seq {}", p8.target);
            0
        }
        Ok(s) => {
            eprintln!("rushi-sessions: {label} exited with {s}");
            1
        }
        Err(e) => {
            eprintln!("rushi-sessions: cannot start {label}: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// The settled target (the kernel's P8 write guard, kernel PR #66).
// ---------------------------------------------------------------------------

/// The marker fields of a pick after the settled-target move.
/// `target` and `mode` are what the appended marker carries.
/// `moved` is true when the pick moved forward to the step's last
/// result. `unfixable` is true when the P8 guard refuses the marker
/// whatever the target (a stranded pair the forward move cannot
/// cover). `stranded` carries that case's stranded facts for the
/// refusal message (empty on a clean pick).
struct SettledTarget<'a> {
    target: u64,
    mode: &'a str,
    moved: bool,
    unfixable: bool,
    stranded: P8Strands,
}

/// The P8-safe settled target of a pick. The kernel's P8 write
/// guard (`rushi_common::rewind::guard_rewind`, kernel PR #66)
/// refuses a marker whose projected context strands a tool pair, so
/// a pick on a step whose tool results sit in the abandoned span
/// lands on the step's last result instead: the design's
/// finished-step target (rewind-fork-design.md section 1). The
/// check below is a mirror of that guard: the projected context of
/// the target over the existing rewind chain, kept events carry
/// their tool calls, and a stranded pair is a kept call without a
/// kept result, or a kept result without a kept call. When the
/// P8 `rushi-common` publishes, replace the mirror with the
/// shared function.
fn settled_target<'a>(rows: &[(u64, Value)], pick: u64, mode: &'a str) -> SettledTarget<'a> {
    // The existing chain: malformed markers degrade out of the
    // chain, exactly as the kernel's projection does.
    let existing: Vec<RewindRef> = rows
        .iter()
        .filter_map(|(s, v)| parse_rewind_event(v, *s as usize))
        .collect();
    let before = mode == "before";
    let eff0 = if before { pick.saturating_sub(1) } else { pick };
    let mut eff = eff0;
    let mut unfixable = false;
    // The stranded facts of the unfixable case (the clean exit
    // keeps the empty value).
    let mut stranded = P8Strands {
        stranded_calls: Vec::new(),
        stranded_result_ids: Vec::new(),
    };
    loop {
        // The candidate marker: at or before the original boundary,
        // the original mode; past it, mode `on` (the moved boundary
        // includes the step's results).
        let target = if eff == eff0 { pick } else { eff };
        let cand_before = eff == eff0 && before;
        let strands = p8_strands(rows, &existing, target, cand_before);
        if strands.empty() {
            break;
        }
        // The fixable strand: a stranded call whose result exists in
        // the log past the boundary. The boundary moves to the last
        // of those results.
        let fix: Option<u64> = strands
            .stranded_calls
            .iter()
            .filter_map(|(_, r)| *r)
            .filter(|r| *r > eff)
            .max();
        if strands.stranded_result_ids.is_empty() {
            if let Some(r) = fix {
                eff = r;
                continue;
            }
        }
        unfixable = true;
        stranded = strands;
        break;
    }
    let moved = eff != eff0;
    let (target, m) = if moved { (eff, "on") } else { (pick, mode) };
    SettledTarget {
        target,
        mode: m,
        moved,
        unfixable,
        stranded,
    }
}

/// The stranded facts of one candidate marker, the kernel's P8
/// guard form (`rushi_common::rewind::guard_rewind`, kernel PR
/// #66): `stranded_calls` is the kept call without a kept result,
/// with the first log result past the boundary when one exists
/// (the forward fix); `stranded_result_ids` is the kept result
/// without a kept call (the call sits behind the boundary, or is
/// masked by an earlier marker).
struct P8Strands {
    stranded_calls: Vec<(String, Option<u64>)>,
    stranded_result_ids: Vec<String>,
}

impl P8Strands {
    fn empty(&self) -> bool {
        self.stranded_calls.is_empty() && self.stranded_result_ids.is_empty()
    }

    /// The first stranded pair, the kernel's refusal phrasing:
    /// `tool call c1` or `tool result c1`.
    fn first_stranded(&self) -> Option<String> {
        if let Some((id, _)) = self.stranded_calls.first() {
            return Some(format!("tool call {id}"));
        }
        self.stranded_result_ids
            .first()
            .map(|id| format!("tool result {id}"))
    }
}

/// The P8 guard form over one log: the projected context of
/// `target` (mode `before` excludes the target, the kernel's
/// `t_eff`), over the existing rewind chain. A mirror of
/// `rushi_common::rewind::guard_rewind` without the range check
/// (the caller validates the pick against the log end).
fn p8_strands(
    rows: &[(u64, Value)],
    existing: &[RewindRef],
    target: u64,
    before: bool,
) -> P8Strands {
    let t_eff = if before { target.saturating_sub(1) } else { target };
    let ranges = active_ranges(t_eff as usize, existing);
    let mut kept_calls: Vec<String> = Vec::new();
    let mut kept_results: Vec<String> = Vec::new();
    for (s, v) in rows {
        if !seq_in_ranges(*s as usize, &ranges) {
            continue;
        }
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "assistant_message" => {
                if let Some(arr) = v.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in arr {
                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                            let id = id.to_string();
                            if !kept_calls.contains(&id) {
                                kept_calls.push(id);
                            }
                        }
                    }
                }
            }
            "tool_result" => {
                if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                    let id = id.to_string();
                    if !kept_results.contains(&id) {
                        kept_results.push(id);
                    }
                }
            }
            _ => {}
        }
    }
    let stranded_calls: Vec<(String, Option<u64>)> = kept_calls
        .iter()
        .filter(|id| !kept_results.contains(*id))
        .map(|id| {
            let fix = rows
                .iter()
                .filter(|(s, v)| {
                    v.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                        && v.get("id").and_then(|i| i.as_str()) == Some(id.as_str())
                        && *s > t_eff
                })
                .map(|(s, _)| *s)
                .min();
            (id.clone(), fix)
        })
        .collect();
    let mut seen: Vec<String> = Vec::new();
    let stranded_result_ids: Vec<String> = rows
        .iter()
        .filter(|(s, v)| {
            seq_in_ranges(*s as usize, &ranges)
                && v.get("type").and_then(|t| t.as_str()) == Some("tool_result")
        })
        .filter_map(|(_, v)| v.get("id").and_then(|i| i.as_str()).map(String::from))
        .filter(|id| !kept_calls.contains(id))
        .filter(|id| {
            if seen.contains(id) {
                false
            } else {
                seen.push(id.clone());
                true
            }
        })
        .collect();
    P8Strands {
        stranded_calls,
        stranded_result_ids,
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
/// bin dir): the per-user farm does not symlink the kernel bins,
/// so the sibling of the resolved `rushi` is the source of truth.
/// The resolved entry may be a farm or profile symlink: the chain
/// is followed to the store's real bin dir, which carries the
/// sibling. Each name is tried in turn: the sibling of the
/// resolved `rushi` first, then the bare name on PATH. When no
/// name resolves, the last bare name (the spawn fails visibly).
fn kernel_bin(names: &[&str]) -> String {
    if let Some(rushi) = which("rushi") {
        let real = std::fs::canonicalize(&rushi).unwrap_or(rushi);
        if let Some(d) = real.parent() {
            for name in names {
                let p = d.join(name);
                if p.is_file() {
                    return p.to_string_lossy().into_owned();
                }
            }
        }
    }
    for name in names {
        if which(name).is_some() {
            return name.to_string();
        }
    }
    names.last().unwrap_or(&"").to_string()
}

/// The kernel binary launch of a session (decision 88, the 90
/// resolvability fallback): the argv plus the working dir of the
/// launch. The project's effective `.envrc` (this dir, then its
/// ancestors) names the devShell flake (decision 95): the launch is
/// `nix develop -c <name>` in that flake dir, when the gate holds.
/// The bare `nix develop -c <name>` only works when `<name>`
/// resolves from the devShell PATH (the devShell packages shadowing
/// the inherited PATH). The kernel bins (`rushi-log`,
/// `rushi-compact`, and the legacy `log`) are the siblings of the
/// kernel's `rushi` in the package's one bin dir; a devShell that
/// ships no kernel bin leaves a bare name unfindable on the
/// inherited PATH too, and the bare launch dies with a 127
/// (decision 90). The probe is the cached devShell resolve of
/// `<name>` (decision 89): a resolve hit keeps the gated launch in
/// the flake dir; a miss (the devShell and the inherited PATH both
/// lack the name, or the bounded materialize times out) falls back
/// to the PATH sibling of the resolved `rushi`, the pre-flake
/// behavior. Without an `.envrc` on the chain, the fallback is
/// unconditional.
fn kernel_launch(
    names: &[&str],
    projdir: Option<&Path>,
    cwd: Option<PathBuf>,
) -> (Vec<String>, Option<PathBuf>) {
    if let Some(p) = projdir {
        if let Some(fd) = crate::devshell::envrc_flake_dir(p) {
            for name in names {
                if crate::devshell::resolve(fd.as_path(), name).is_some() {
                    return (
                        vec![
                            "nix".to_string(),
                            "develop".to_string(),
                            "-c".to_string(),
                            name.to_string(),
                        ],
                        Some(fd),
                    );
                }
            }
        }
    }
    (vec![kernel_bin(names)], cwd)
}

/// The launch's command builder: the argv as a `Command`, plus the
/// working dir when one is set.
fn launch_command(argv: &[String], cwd: &Option<PathBuf>) -> Command {
    let mut c = Command::new(&argv[0]);
    for a in &argv[1..] {
        c.arg(a);
    }
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c
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

/// The kernel's log-appender names, new kernel first: `rushi-log`
/// since the kernel's 58 rename of the reserved name (kernel 57),
/// and the legacy `log` of the older kernel builds. The first
/// name that resolves from the devShell PATH (the gated probe) or
/// from the resolved `rushi`'s sibling bin dir (the fallback)
/// wins. Both generations of the kernel stay launchable.
const LOG_BIN_NAMES: &[&str] = &["rushi-log", "log"];

/// Append the one `rewind` marker of a pick through the kernel's
/// log appender (the locked single-write append, FT-005), and wait
/// for it. The marker must land in the log before a `compact`
/// child reads it, so the append is synchronous.
fn append_rewind_marker(base: &Path, seq: u64, mode: &str) -> Result<(), String> {
    let marker = rewind_marker(seq, mode);
    let projdir = base.parent().and_then(|p| p.parent());
    let (argv, cwd) = kernel_launch(LOG_BIN_NAMES, projdir, None);
    let label = argv[0].clone();
    let mut c = launch_command(&argv, &cwd);
    c.arg("--session").arg(base);
    c.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let mut child = c
        .spawn()
        .map_err(|e| format!("cannot start {label}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut line = marker;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("cannot write the rewind marker: {e}"))?;
    }
    let s = child
        .wait()
        .map_err(|e| format!("cannot wait on {label}: {e}"))?;
    if s.success() {
        Ok(())
    } else {
        Err(format!("{label} exited with {s}"))
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
    let w = fs::read_to_string(base.join("cwd"))
        .ok()
        .map(|s| s.trim().to_string());
    match w {
        Some(w) if !w.is_empty() && Path::new(&w).is_dir() => w,
        _ => base
            .parent()
            .and_then(|d| d.parent())
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without the gate (no projdir, or a projdir with no `.envrc`
    /// on the chain) the launch is the single PATH binary with the
    /// caller's working dir, the pre-flake behavior.
    #[test]
    fn kernel_launch_without_gate_keeps_the_path_binary() {
        let (argv, cwd) = kernel_launch(LOG_BIN_NAMES, None, None);
        assert_eq!(argv.len(), 1);
        assert!(!argv[0].is_empty());
        assert!(cwd.is_none());

        let base = std::env::temp_dir().join(format!("rsrs-klaunch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let bare = base.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        let (argv, cwd) = kernel_launch(LOG_BIN_NAMES, Some(bare.as_path()), Some(bare.clone()));
        assert_eq!(argv.len(), 1);
        assert_eq!(cwd, Some(bare));
    }

    /// With the gate (an `.envrc` in the projdir loading its flake,
    /// decision 95) and a resolvable bin the launch is
    /// `nix develop -c <bin>` in the flake dir; the launch dir is
    /// the flake dir, not the caller's. The resolve is a seeded
    /// fresh cache entry, so the probe runs no nix call.
    #[test]
    fn kernel_launch_with_gate_is_the_devshell() {
        let base = std::env::temp_dir().join(format!("rsrs-klaunch-{}", std::process::id()));
        let flake = base.join("flake");
        std::fs::create_dir_all(&flake).unwrap();
        std::fs::write(flake.join(".envrc"), "use flake .\n").unwrap();
        std::fs::write(flake.join("flake.nix"), "{}").unwrap();
        // Seed a fresh cache entry per bin so the resolve probe
        // serves the seeded path without a nix call. The seeded
        // paths must exist (the freshness check re-stats them).
        let compact = base.join("compact");
        std::fs::write(&compact, "x").unwrap();
        crate::devshell::seed(flake.as_path(), "compact", compact.to_str().unwrap());
        let rushi_log = base.join("rushi-log");
        std::fs::write(&rushi_log, "x").unwrap();
        crate::devshell::seed(flake.as_path(), "rushi-log", rushi_log.to_str().unwrap());
        let w = base.join("work");
        let (argv, cwd) = kernel_launch(&["compact"], Some(flake.as_path()), Some(w.clone()));
        assert_eq!(
            argv,
            vec![
                "nix".to_string(),
                "develop".to_string(),
                "-c".to_string(),
                "compact".to_string(),
            ]
        );
        assert_eq!(cwd, Some(flake.clone()));
        // The probe walks the name list: the first name with a
        // resolve hit (the new `rushi-log`) wins the launch.
        let (argv, cwd) = kernel_launch(LOG_BIN_NAMES, Some(flake.as_path()), None);
        assert_eq!(argv[0], "nix");
        assert_eq!(argv[3], "rushi-log");
        assert_eq!(cwd, Some(flake));
    }

    /// With the gate (an `.envrc` in the projdir, decision 95) but a
    /// devShell that resolves no name of the list (no cache entry,
    /// the materialize fails on a bare `{}` flake with no devShell)
    /// the launch falls back to the PATH sibling of the resolved
    /// `rushi` with the caller's working dir, the pre-flake
    /// behavior (decision 90). The bare `nix develop -c <bin>` of
    /// decision 88 is what dies with a 127 in this world.
    #[test]
    fn kernel_launch_with_gate_unresolvable_keeps_the_path_sibling() {
        let base = std::env::temp_dir().join(format!("rsrs-klaunch-{}", std::process::id()));
        let flake = base.join("unresolvable");
        std::fs::create_dir_all(&flake).unwrap();
        std::fs::write(flake.join(".envrc"), "use flake .\n").unwrap();
        std::fs::write(flake.join("flake.nix"), "{}").unwrap();
        if which("nix").is_none() {
            return;
        }
        // No cache entry for either name: the probe materializes
        // `nix develop` on the bare flake, which lacks a devShell,
        // so every resolve is None and the fallback holds. The
        // bare flake fails the evaluate in under a second, so the
        // materialize never hits its wall-clock cap.
        let (argv, cwd) = kernel_launch(LOG_BIN_NAMES, Some(flake.as_path()), Some(flake.clone()));
        assert_eq!(argv, vec![kernel_bin(LOG_BIN_NAMES)]);
        assert_eq!(cwd, Some(flake.clone()));
    }

    // ------------------------------------------------------------------
    // The settled target (the kernel's P8 write guard, kernel PR #66).
    // ------------------------------------------------------------------

    /// One numbered log line: a typed event at its 1-based seq.
    fn line(seq: u64, json: &str) -> (u64, Value) {
        (seq, serde_json::from_str(json).unwrap())
    }

    fn user(seq: u64, content: &str) -> (u64, Value) {
        line(
            seq,
            &format!(r#"{{"type":"user_message","ts":"t","content":"{content}","v":1}}"#),
        )
    }

    fn assistant(seq: u64, content: &str) -> (u64, Value) {
        line(
            seq,
            &format!(r#"{{"type":"assistant_message","ts":"t","content":"{content}","v":1}}"#),
        )
    }

    /// The kernel's modern shape: the step carries its tool call.
    fn assistant_call(seq: u64, call_id: &str) -> (u64, Value) {
        line(
            seq,
            &format!(
                r#"{{"type":"assistant_message","ts":"t","content":"","tool_calls":[{{"id":"{call_id}","name":"bash","arguments":{{}}}}],"v":1}}"#
            ),
        )
    }

    fn tool_result(seq: u64, call_id: &str) -> (u64, Value) {
        line(
            seq,
            &format!(
                r#"{{"type":"tool_result","ts":"t","id":"{call_id}","value":{{"text":"out"}},"is_error":false,"v":1}}"#
            ),
        )
    }

    fn marker(seq: u64, target: u64, mode: &str) -> (u64, Value) {
        line(
            seq,
            &format!(
                r#"{{"type":"rewind","ts":"t","target_seq":{target},"mode":"{mode}","reason":"tui_pick","v":1}}"#
            ),
        )
    }

    /// The common case: the pick is a step whose result sits in the
    /// abandoned span. The target moves to the step's last result,
    /// and the marker keeps its mode `on`.
    #[test]
    fn settled_target_moves_to_the_step_last_result() {
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            assistant(2, "A1"),
            user(3, "branch B question"),
            assistant_call(4, "c1"),
            tool_result(5, "c1"),
        ];
        let p8 = settled_target(&rows, 4, "on");
        assert_eq!(p8.target, 5, "the target is the step's last result");
        assert_eq!(p8.mode, "on");
        assert!(p8.moved);
        assert!(!p8.unfixable);
    }

    /// A clean pick (no tool pair in play) keeps the pick and the
    /// original mode: the unadjusted behavior.
    #[test]
    fn settled_target_keeps_the_clean_pick() {
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            assistant(2, "A1"),
            user(3, "branch B question"),
            assistant(4, "branch B answer"),
        ];
        let p8 = settled_target(&rows, 4, "on");
        assert_eq!(p8.target, 4);
        assert_eq!(p8.mode, "on");
        assert!(!p8.moved);
        assert!(!p8.unfixable);

        // The before-mode pick of a user message: the context ends at
        // the step group's last result, which pairs its call.
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            assistant_call(2, "c1"),
            tool_result(3, "c1"),
            user(4, "re-ask"),
        ];
        let p8 = settled_target(&rows, 4, "before");
        assert_eq!(p8.target, 4);
        assert_eq!(p8.mode, "before");
        assert!(!p8.moved);
        assert!(!p8.unfixable);
    }

    /// The forward move re-enters an earlier branch: the stranded
    /// result sits in the span of an earlier marker, and the move
    /// lands past it.
    #[test]
    fn settled_target_move_reenters_the_branch() {
        // 1 user, 2 step-call, 3 result, 4 marker to 1 (branch B
        // forks at 5). The re-entry pick at 2 strands its result
        // (seq 3, inside the abandoned span); the move lands on 3.
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            assistant_call(2, "c1"),
            tool_result(3, "c1"),
            marker(4, 1, "on"),
            user(5, "branch B question"),
        ];
        let p8 = settled_target(&rows, 2, "on");
        assert_eq!(p8.target, 3, "the boundary covers the result");
        assert!(p8.moved);
        assert!(!p8.unfixable);
    }

    /// The steer-mid-step pick (the kernel's guard scenario E): the
    /// before-mode pick of the user message that sits between the
    /// step and its result masks the result. The move lands on the
    /// step's result.
    #[test]
    fn settled_target_moves_the_steer_mid_step_pick() {
        let rows: Vec<(u64, Value)> = vec![
            user(1, "task"),
            assistant_call(2, "c1"),
            user(3, "steer"),
            tool_result(4, "c1"),
        ];
        let p8 = settled_target(&rows, 3, "before");
        assert_eq!(p8.target, 4, "the boundary covers the result");
        assert_eq!(p8.mode, "on", "the moved boundary includes the result");
        assert!(p8.moved);
        assert!(!p8.unfixable);
    }

    /// The depth-3 fork chain (the kernel's guard scenario F): the
    /// new branch keeps its own pairs, so the pick on its tail
    /// settles without a move.
    #[test]
    fn settled_target_walks_the_fork_chain() {
        let rows: Vec<(u64, Value)> = vec![
            user(1, "task"),
            assistant_call(2, "c1"),
            tool_result(3, "c1"),
            marker(4, 3, "on"),
            assistant_call(5, "c2"),
            tool_result(6, "c2"),
            marker(7, 3, "on"),
            assistant_call(8, "c3"),
            tool_result(9, "c3"),
        ];
        let p8 = settled_target(&rows, 9, "on");
        assert_eq!(p8.target, 9, "the tail pick settles");
        assert!(!p8.moved);
        assert!(!p8.unfixable);
    }

    /// The unfixable call: the stranded call has no result in the
    /// log (a killed step). The kernel's P8 guard refuses the
    /// marker; the resolver reports the case and names the pair.
    #[test]
    fn settled_target_unfixable_missing_result() {
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            assistant_call(2, "cX"),
            user(3, "branch B question"),
        ];
        let p8 = settled_target(&rows, 2, "on");
        assert!(p8.unfixable, "the P8 guard would refuse the marker");
        assert_eq!(p8.target, 2, "the original pick stands");
        assert!(!p8.moved);
        assert_eq!(
            p8.stranded.first_stranded().as_deref(),
            Some("tool call cX"),
            "the refusal names the pair"
        );
    }

    /// The unfixable result: the kept result's call sits behind the
    /// boundary (masked by an earlier marker), so no forward move
    /// pairs it.
    #[test]
    fn settled_target_unfixable_result_without_call() {
        // 1 user, 2 step-call c1, 3 result c1, 4 marker to 1 (the
        // span 2..3 is abandoned), 5 a lone result c1 (the call is
        // masked). The pick at 5 keeps the result; its call is out
        // of the context; the move cannot cover it.
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            assistant_call(2, "c1"),
            tool_result(3, "c1"),
            marker(4, 1, "on"),
            tool_result(5, "c1"),
        ];
        let p8 = settled_target(&rows, 5, "on");
        assert!(p8.unfixable, "the stranded result cannot be paired");
        assert_eq!(
            p8.stranded.first_stranded().as_deref(),
            Some("tool result c1"),
            "the refusal names the pair"
        );
    }

    /// A malformed marker degrades out of the chain, exactly as the
    /// kernel's projection does (the kernel's guard test
    /// `guard_degrades_malformed_markers_like_the_projection`).
    #[test]
    fn settled_target_degrades_malformed_markers() {
        // The target-0 marker at seq 2 is rejected by
        // `parse_rewind_event` and drops out of the chain. The pick
        // at 4 settles over the full prefix.
        let bad = line(
            2,
            r#"{"type":"rewind","ts":"t","target_seq":0,"mode":"on","v":1}"#,
        );
        let rows: Vec<(u64, Value)> = vec![
            user(1, "Q1"),
            bad,
            assistant_call(3, "c1"),
            tool_result(4, "c1"),
        ];
        let p8 = settled_target(&rows, 4, "on");
        assert_eq!(p8.target, 4, "the malformed marker degrades out");
        assert!(!p8.moved);
        assert!(!p8.unfixable);
    }
}
