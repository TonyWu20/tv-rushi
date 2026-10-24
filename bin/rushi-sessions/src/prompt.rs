//! The television prompt actions: `new_session` and `send_message`.
//!
//! Ports the channel's shell oneliners so no shell quoting crosses the
//! tmux boundary. Each action is two stages:
//!
//! - Stage 1 runs in the television fork: resolve the session row,
//!   the host prefix, the working dir, the editor; create the prompt
//!   file; then either spawn a floating pane (tmux 3.8+) or fall
//!   back to the fork's pane.
//! - Stage 2 runs the editor and the create/send step. In pane mode
//!   it is the pane's own command (the binary's absolute path, so
//!   the tmux server's PATH is irrelevant); in fallback mode it runs
//!   in-process in the fork's pane.
//!
//! The create/send step resolves its `rushi` binary on the PATH.
//! When the session's project dir carries a `flake.nix` and nix
//! is on PATH, the step runs in that devShell instead:
//! `nix develop -c rushi run ...` from the project dir (decision
//! 87/88). The remote rows run the same check on the remote
//! host. The devShell shadows the inherited PATH, so a binary the
//! devShell does not ship falls back to the inherited PATH
//! entry.
//!
//! Stage 2 receives its inputs from a JSON args file (the pane
//! command is one path, so no value ever crosses a shell). The
//! args file is deleted after the read.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// The resolved inputs of one prompt action, shared by both stages.
#[derive(Debug, Clone)]
pub(crate) struct PromptCfg {
    /// "new" (new_session) or "message" (send_message).
    pub kind: &'static str,
    /// The session row (path, or host:path), or the session dir.
    pub rp: String,
    /// The ssh alias when the row is remote.
    pub host: Option<String>,
    /// The message-mode working dir (the session's cwd file).
    pub w: Option<String>,
    /// The new-mode project root (dirname(dirname(rp))).
    pub r_root: Option<String>,
    /// The prompt file (the editor's target, a .md temp file).
    pub file: String,
    /// The editor command.
    pub editor: String,
    /// The new-mode hint line.
    pub hint: Option<String>,
    /// The rushi binary (absolute, resolved in stage 1).
    pub rushi: String,
    /// The ssh binary (absolute, resolved in stage 1).
    pub ssh: String,
    /// The detach tool: "setsid" or "nohup".
    pub detach: String,
    /// True when the create/send step runs in the project flake's
    /// devShell (`nix develop -c rushi run ...`), not the PATH
    /// `rushi` (decision 87).
    pub nix_dev: bool,
    /// True when stage 2 runs as the floating pane's command.
    pub pane: bool,
}

impl PromptCfg {
    /// Write the cfg to a JSON args file and return its path. The
    /// caller deletes it after reading.
    pub(crate) fn to_args_file(&self) -> Result<PathBuf, String> {
        let d = tmp_dir();
        let name = format!("tv-rushi-pargs.{}", random_suffix());
        let p = d.join(name);
        let j = |v: &str| Value::String(v.to_string());
        let v = Value::Object(serde_json::Map::from_iter([
            ("kind".into(), j(self.kind)),
            ("rp".into(), j(&self.rp)),
            ("host".into(), j(&self.host.clone().unwrap_or_default())),
            ("w".into(), j(&self.w.clone().unwrap_or_default())),
            ("r".into(), j(&self.r_root.clone().unwrap_or_default())),
            ("file".into(), j(&self.file)),
            ("editor".into(), j(&self.editor)),
            ("hint".into(), j(&self.hint.clone().unwrap_or_default())),
            ("rushi".into(), j(&self.rushi)),
            ("ssh".into(), j(&self.ssh)),
            ("detach".into(), j(&self.detach)),
            ("nix_dev".into(), j(&self.nix_dev.to_string())),
        ]));
        let text = serde_json::to_string(&v).map_err(|e| e.to_string())?;
        std::fs::write(&p, text).map_err(|e| e.to_string())?;
        Ok(p)
    }

    /// Read the cfg from an args file and delete the file.
    pub(crate) fn from_args_file(p: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let g = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let cfg = Self {
            kind: if g("kind") == "new" { "new" } else { "message" },
            rp: g("rp"),
            host: if g("host").is_empty() {
                None
            } else {
                Some(g("host"))
            },
            w: if g("w").is_empty() {
                None
            } else {
                Some(g("w"))
            },
            r_root: if g("r").is_empty() {
                None
            } else {
                Some(g("r"))
            },
            file: g("file"),
            editor: g("editor"),
            hint: if g("hint").is_empty() {
                None
            } else {
                Some(g("hint"))
            },
            rushi: g("rushi"),
            ssh: g("ssh"),
            detach: g("detach"),
            nix_dev: g("nix_dev") == "true",
            pane: true,
        };
        let _ = std::fs::remove_file(p);
        Ok(cfg)
    }
}

/// Stage 2: run the editor, then the create/send step. Used by the
/// hidden `run-pane` subcommand (pane mode) and in-process (fallback
/// mode, via `run_body`).
pub(crate) fn run_pane(args_file: &Path) -> i32 {
    let cfg = match PromptCfg::from_args_file(args_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rushi-sessions: cannot read the prompt args: {e}");
            return 1;
        }
    };
    run_body(&cfg)
}

/// The in-process stage 2 body (fallback mode): the stty save/restore
/// wraps the editor; no pane sleep at the end.
pub(crate) fn run_inline_body(cfg: &PromptCfg) -> i32 {
    let st = stty_save();
    run_body(cfg);
    // The editor restores its own terminal state on a clean exit. The
    // save covers a crash: restore the cooked state of the television
    // pane (matching the shell oneliner's fallback logic).
    if st.is_empty() {
        let _ = Command::new("stty").args(["raw", "-echo"]).output();
    } else {
        let _ = Command::new("stty").arg(&st).output();
    }
    0
}

fn run_body(cfg: &PromptCfg) -> i32 {
    run_editor(cfg);
    let (name, m) = read_prompt(cfg);
    let _ = std::fs::remove_file(&cfg.file);
    let ok = dispatch(cfg, name.as_deref(), &m);
    if cfg.pane {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    if ok {
        0
    } else {
        1
    }
}

/// Run the editor on the prompt file, inheriting the controlling
/// terminal. A missing or failing editor is reported but not fatal
/// (the rest of the flow then sees an empty prompt).
fn run_editor(cfg: &PromptCfg) {
    let mut c = Command::new(&cfg.editor);
    c.arg(&cfg.file).stdin(Stdio::inherit());
    if let Some(d) = editor_cwd(cfg) {
        c.current_dir(d);
    }
    match c.status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("rushi-sessions: editor {} exited with {}", cfg.editor, s),
        Err(e) => eprintln!("rushi-sessions: cannot start editor {}: {e}", cfg.editor),
    }
}

/// The editor opens in the project dir (the shell oneliner cd'd into
/// it before running the editor).
fn editor_cwd(cfg: &PromptCfg) -> Option<String> {
    match cfg.kind {
        "new" => cfg.r_root.clone(),
        "message" => cfg.w.clone(),
        _ => None,
    }
}

/// Read the prompt file. new mode splits line 1 (the name, the hint
/// stripped) from the rest (the message); message mode is the whole
/// file.
fn read_prompt(cfg: &PromptCfg) -> (Option<String>, String) {
    let text = std::fs::read_to_string(&cfg.file).unwrap_or_default();
    if cfg.kind == "new" {
        let (first, rest) = match text.split_once('\n') {
            Some((a, b)) => (a, b.to_string()),
            None => (text.as_str(), String::new()),
        };
        let mut name = first.replace(cfg.hint.as_deref().unwrap_or(""), "");
        name = name.trim().to_string();
        (Some(name), rest)
    } else {
        (None, text)
    }
}

/// The create/send step. Returns whether anything was sent.
fn dispatch(cfg: &PromptCfg, name: Option<&str>, m: &str) -> bool {
    if cfg.kind == "message" {
        if is_blank(m) {
            println!("No message entered, nothing sent.");
            return false;
        }
        let host = cfg.host.as_deref();
        let w = cfg.w.clone().unwrap_or_default();
        let sent = if host.is_some() {
            let m64 = b64_encode(m.as_bytes());
            let cmd = remote_run_command(&w, &cfg.rp, Some(&m64), None, cfg.nix_dev);
            detached(&cfg.ssh, &cfg.detach, &["-T", host.unwrap(), &cmd])
        } else {
            // The `--` escape keeps a hyphen-leading message (a
            // bullet list, a dash) as the task positional: kernels
            // without allow_hyphen_values reject it as an unknown
            // flag, and the detached stderr goes to /dev/null, so
            // the loss would be silent. The escape stays valid on
            // the fixed kernels (allow_hyphen_values).
            detached_rushi(cfg, &[&cfg.rp, "--", m], &w)
        };
        if let Some(h) = host {
            if sent {
                println!("message queued for {} on {}", cfg.rp, h);
            } else {
                eprintln!(
                    "rushi-sessions: nothing sent for {} on {} (the remote send call did not start).",
                    cfg.rp, h
                );
            }
        } else if sent {
            println!("message queued for {}", cfg.rp);
        } else {
            eprintln!(
                "rushi-sessions: nothing sent for {} (the send call did not start).",
                cfg.rp
            );
        }
        sent
    } else {
        let n = name.unwrap_or("");
        if !valid_session_name(n) {
            println!("new_session: empty or unusable session name, nothing created.");
            return false;
        }
        let msg = !is_blank(m);
        let r = cfg.r_root.clone().unwrap_or_default();
        if let Some(host) = cfg.host.as_deref() {
            let msg64 = if msg {
                Some(b64_encode(m.as_bytes()))
            } else {
                None
            };
            let cmd = remote_run_command(&r, n, msg64.as_deref(), Some(&r), cfg.nix_dev);
            let ok = detached(&cfg.ssh, &cfg.detach, &["-T", host, &cmd]);
            if !ok {
                eprintln!("rushi-sessions: nothing created for {n} on {host} (the remote call did not start).");
                return false;
            }
            if msg {
                println!("new session {} starting in {}/sessions on {host}", n, r);
            } else {
                println!(
                    "new idle session {} starting in {}/sessions on {host}",
                    n, r
                );
            }
        } else {
            if !Path::new(&r).is_dir() {
                eprintln!("new_session: project dir {} missing, nothing created.", r);
                return false;
            }
            let sr = format!("{}/sessions", r);
            let args: Vec<&str> = if msg {
                vec![n, m, "--sessions-root", &sr]
            } else {
                vec![n, "--sessions-root", &sr]
            };
            let ok = detached_rushi(cfg, &args, &r);
            if !ok {
                eprintln!("rushi-sessions: nothing created for {n} (the send call did not start).");
                return false;
            }
            if msg {
                println!("new session {n} starting in {r}/sessions (working dir {r})");
            } else {
                println!("new idle session {n} starting in {r}/sessions (working dir {r})");
            }
        }
        true
    }
}

/// The remote `rushi run` command string. The message crosses the ssh
/// boundary as a base64 blob (no newlines, no shell metacharacters),
/// decoded on the remote side. The dir and name are single-quoted
/// for the remote shell.
pub(crate) fn remote_run_command(
    workdir: &str,
    target: &str,
    msg64: Option<&str>,
    sessions_root: Option<&str>,
    nix_dev: bool,
) -> String {
    let mut parts = vec![format!("cd {}", remote_quote(workdir))];
    // The devShell launch (decision 88) runs the `rushi` of the
    // remote project flake: the plain `nix develop` inherits the
    // remote environment, so a devShell without the binary keeps
    // the remote PATH one.
    let mut run: Vec<String> = if nix_dev {
        vec![
            "nix".into(),
            "develop".into(),
            "-c".into(),
            "rushi".into(),
            "run".into(),
        ]
    } else {
        vec!["rushi".into(), "run".into()]
    };
    run.push(remote_quote(target));
    if let Some(b) = msg64 {
        // The `--` escape keeps a hyphen-leading message (a bullet
        // list, a dash) as the task positional: kernels whose clap
        // lacks allow_hyphen_values reject `-...` as an unknown
        // flag, and the remote loss would be silent. Only the message
        // mode (no --sessions-root after the task) may escape: a
        // trailing flag after `--` would parse as a positional.
        if sessions_root.is_none() {
            run.push("--".to_string());
        }
        // Double-quoted: the remote shell must not word-split the
        // decoded message (it may hold spaces).
        run.push(format!("\"$(printf %s {b} | base64 -d)\""));
    }
    if let Some(sr) = sessions_root {
        run.push("--sessions-root".to_string());
        run.push(remote_quote(sr));
    }
    parts.push(run.join(" "));
    parts.join(" && ")
}

/// Quote one word for a remote shell (single quotes).
pub(crate) fn remote_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The remote devShell gate (decision 88): the remote project dir
/// carries a flake and `nix` is reachable by the non-interactive
/// ssh of the remote host. A `nix` the remote PATH lacks (a Nix
/// profile is not on the non-interactive PATH) keeps the plain
/// remote PATH command.
pub(crate) fn remote_nix_dev(host: &str, dir: &str) -> bool {
    remote_stdout(host, &remote_nix_dev_command(dir)) == "yes"
}

/// The gate command string (one read-only ssh -T call).
pub(crate) fn remote_nix_dev_command(dir: &str) -> String {
    format!(
        "if [ -f {} ] && command -v nix >/dev/null 2>&1; then echo yes; fi",
        remote_quote(&format!("{}/flake.nix", dir))
    )
}

/// Run `detach cmd args...` detached (new session when setsid is
/// available), stdio to /dev/null. The caller does not wait.
/// Returns whether the spawn succeeded (the caller must not report
/// a send on a failed spawn: the caller cannot observe the
/// detached failure any other way).
fn detached(tool: &str, detach: &str, args: &[&str]) -> bool {
    let mut c = Command::new(detach);
    c.arg(tool);
    for a in args {
        c.arg(a);
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match c.spawn() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("rushi-sessions: cannot start {detach} {tool}: {e}");
            false
        }
    }
}

/// The detached local `rushi run` from the working dir.
/// Returns whether the spawn succeeded. The step's argv after the
/// detach tool is the PATH `rushi` binary, or the nix devShell
/// launch when the project flake ships its own build (decision 87).
fn detached_rushi(cfg: &PromptCfg, args: &[&str], workdir: &str) -> bool {
    let mut c = Command::new(&cfg.detach);
    for t in rushi_run_tail(cfg) {
        c.arg(&t);
    }
    for a in args {
        c.arg(a);
    }
    c.current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match c.spawn() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("rushi-sessions: cannot start {}: {e}", rushi_run_label(cfg));
            false
        }
    }
}

/// Save the terminal state (`stty -g`), empty on failure.
pub(crate) fn stty_save() -> String {
    Command::new("stty")
        .arg("-g")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Find a binary on PATH.
pub(crate) fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for d in std::env::split_paths(&path) {
        let c = d.join(name);
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

/// The nix devShell resolution (decision 87/88): true when the
/// session's project dir carries a flake and `nix` is on PATH. The
/// step then runs as `nix develop -c <binary>` from that dir. The
/// plain `nix develop` inherits the environment, so the devShell
/// packages shadow the inherited PATH, and a binary the devShell
/// does not ship falls back to the inherited PATH entry naturally.
/// A failed gate keeps the plain PATH binary, the pre-flake
/// behavior.
pub(crate) fn nix_dev_dir(dir: &str) -> bool {
    if dir.is_empty() {
        return false;
    }
    if !Path::new(dir).join("flake.nix").is_file() {
        return false;
    }
    which("nix").is_some()
}

/// The step's argv after the detach tool: the PATH `rushi` binary,
/// or the nix devShell launch when `nix_dev` is set. The launch
/// resolves the flake from the step's working dir (the project dir,
/// the flake dir). The devShell packages shadow the inherited
/// PATH, so a devShell without the binary keeps the inherited one.
/// The args after the tail cross into the step verbatim; the
/// message-mode `--` escape survives the nix arg pass.
pub(crate) fn rushi_run_tail(cfg: &PromptCfg) -> Vec<String> {
    if cfg.nix_dev {
        vec![
            "nix".to_string(),
            "develop".to_string(),
            "-c".to_string(),
            "rushi".to_string(),
            "run".to_string(),
        ]
    } else {
        vec![cfg.rushi.clone(), "run".to_string()]
    }
}

/// The step label of the spawn-failure report: the `rushi` path, or
/// the nix launch when `nix_dev` is set.
pub(crate) fn rushi_run_label(cfg: &PromptCfg) -> String {
    if cfg.nix_dev {
        "the nix devShell launch".to_string()
    } else {
        cfg.rushi.clone()
    }
}

/// The prompt-file dir: $TMPDIR, else /tmp.
fn tmp_dir() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Six random alphanumeric chars (the mktemp XXXXXX equivalent).
fn random_suffix() -> String {
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut buf = [0u8; 6];
    // Read exactly the six bytes needed: /dev/urandom never signals
    // EOF, so a read-to-end would loop forever.
    match std::fs::File::open("/dev/urandom") {
        Ok(mut f) => {
            use std::io::Read;
            f.read_exact(&mut buf).unwrap_or_default();
        }
        Err(_) => buf.fill(0x41),
    }
    buf.iter().map(|b| CH[(*b as usize) % 62] as char).collect()
}

/// Create the prompt temp file (`tv-rushi-<what>.XXXXXX` + `.md`) and
/// return its path.
pub(crate) fn new_prompt_file(
    what: &str,
    hint: Option<&str>,
    workdir: Option<&str>,
) -> Result<PathBuf, String> {
    let d = workdir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(tmp_dir);
    let p = d.join(format!("tv-rushi-{what}.{}", random_suffix()));
    let text = hint.map(|h| format!("{h}\n")).unwrap_or_default();
    std::fs::write(&p, &text).map_err(|e| format!("write {}: {e}", p.display()))?;
    // The file name gets a .md suffix (the editor's filetype plugin
    // applies), matching the shell oneliner's `mv "$f" "$f.md"`.
    let mut md = p.file_name().unwrap_or_default().to_os_string();
    md.push(".md");
    let mp = p.with_file_name(&md);
    std::fs::rename(&p, &mp).map_err(|e| e.to_string())?;
    Ok(mp)
}

/// The session row: `host:path` when the part before the first `:`
/// has no `/` (an ssh alias, possibly `user@host`); else local.
pub(crate) fn split_row(row: &str) -> (Option<String>, String) {
    if let Some((h, rest)) = row.split_once(':') {
        if !h.is_empty() && !rest.is_empty() && !h.contains('/') {
            return (Some(h.to_string()), rest.to_string());
        }
    }
    (None, row.to_string())
}

/// A usable session name: non-empty, not a comment, not a path, not
/// `.` or `..` (the shell oneliner's case list).
pub(crate) fn valid_session_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('#') && !n.contains('/') && n != "." && n != ".."
}

/// All-POSIX-space check (the shell `tr -d "[:space:]"` test).
pub(crate) fn is_blank(s: &str) -> bool {
    s.bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c'))
}

/// RFC 4648 base64 (std only, no dependency).
pub(crate) fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for c in data.chunks(3) {
        let b = [
            c.first().copied().unwrap_or(0),
            c.get(1).copied().unwrap_or(0),
            c.get(2).copied().unwrap_or(0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6 & 63) as usize]
        } else {
            b'='
        } as char);
        out.push(if c.len() > 2 {
            T[(n & 63) as usize]
        } else {
            b'='
        } as char);
    }
    out
}

/// The tmux floating-pane gate: $TMUX set, tmux present, version at
/// least 3.8 (3.8 adds the -x/-y/-X/-Y geometry flags).
pub(crate) fn tmux_floating_ok() -> bool {
    if std::env::var_os("TMUX").is_none() {
        return false;
    }
    let out = match Command::new("tmux").arg("-V").output() {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let v = match s.rsplit(' ').next() {
        Some(v) => v,
        None => return false,
    };
    // "3.8", "3.8a", "4" ...: major, then the leading digits of the
    // minor (tmux stamps like 3.8a keep the minor usable).
    let mut it = v.split('.');
    let maj: u32 = match it.next().and_then(|m| m.parse().ok()) {
        Some(m) => m,
        None => return false,
    };
    if maj > 3 {
        return true;
    }
    if maj != 3 {
        return false;
    }
    let digits: u32 = it
        .next()
        .map(|m| {
            m.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    digits >= 8
}

/// The stage 1 entry: new_session for a row.
pub(crate) fn new_session(row: &str) -> i32 {
    prompt_action("new", Some(row))
}

/// The stage 1 entry: send_message. With `events`, the session dir is
/// the CWD (the events channel runs in it); else the row.
pub(crate) fn send_message(row: Option<&str>, events: bool) -> i32 {
    if events {
        let cwd = std::env::current_dir().ok();
        match &cwd {
            Some(d) if d.is_dir() => prompt_action("message", Some(d.to_string_lossy().as_ref())),
            _ => {
                eprintln!("rushi-sessions-events: cannot resolve the session dir (the channel CWD is gone); re-open it from a live directory.");
                1
            }
        }
    } else {
        prompt_action("message", row)
    }
}

fn prompt_action(kind: &'static str, row: Option<&str>) -> i32 {
    let (host, rp) = match row {
        Some(r) => split_row(r),
        None => (
            None,
            std::env::current_dir()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    };
    let w = if kind == "message" {
        Some(resolve_workdir(&host, &rp))
    } else {
        None
    };
    let r_root = if kind == "new" {
        Some(resolve_project_root(&host, &rp))
    } else {
        None
    };
    // The nix devShell resolution (decision 87/88): the create/send
    // step runs in the project flake's devShell. The dir is the
    // recorded working dir (message) or the project root (new): the
    // flake dir of the project. A remote row checks the gate on the
    // remote host (the remote flake); a local row checks the local
    // dir.
    let nix_dir = match (kind, w.as_deref(), r_root.as_deref()) {
        ("message", Some(d), _) if !d.is_empty() => Some(d),
        ("new", _, Some(d)) if !d.is_empty() => Some(d),
        _ => None,
    };
    let nix_dev = match (host.as_deref(), nix_dir.as_deref()) {
        (Some(h), Some(d)) => remote_nix_dev(h, d),
        (None, Some(d)) => nix_dev_dir(d),
        _ => false,
    };
    let prompt_dir = match kind {
        "message" if host.is_none() => w.as_deref(),
        "new" if host.is_none() => r_root.as_deref(),
        _ => None,
    };
    let file = match new_prompt_file(
        kind,
        if kind == "new" { Some(HINT_LINE) } else { None },
        prompt_dir,
    ) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(e) => {
            eprintln!("rushi-sessions: {e}");
            return 1;
        }
    };
    let editor = std::env::var_os("EDITOR")
        .filter(|e| !e.is_empty())
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "nvim".into());
    let rushi = which("rushi")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rushi".into());
    let ssh = which("ssh")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ssh".into());
    let detach = if which("setsid").is_some() {
        "setsid"
    } else {
        "nohup"
    };
    let cfg = PromptCfg {
        kind,
        rp: rp.clone(),
        host: host.clone(),
        w: w.clone(),
        r_root: r_root.clone(),
        file: file.clone(),
        editor: editor.clone(),
        hint: if kind == "new" {
            Some(HINT_LINE.to_string())
        } else {
            None
        },
        rushi,
        ssh,
        detach: detach.to_string(),
        nix_dev,
        pane: false,
    };
    let floating = tmux_floating_ok();
    if floating {
        let args_file = match cfg.to_args_file() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("rushi-sessions: {e}");
                return 1;
            }
        };
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("rushi-sessions: cannot resolve the binary: {e}");
                return 1;
            }
        };
        let mut tm = Command::new("tmux");
        tm.arg("new-pane").arg("-f");
        // An empty -c value makes tmux kill the fresh pane; pass it
        // only when the cwd resolves.
        if let Some(cwd) = pane_cwd(kind, &r_root, &w) {
            if !cwd.is_empty() {
                tm.arg("-c").arg(&cwd);
            }
        }
        tm.arg("-x")
            .arg("80%")
            .arg("-y")
            .arg("60%")
            .arg("-X")
            .arg("10%")
            .arg("-Y")
            .arg("20%")
            .arg("-T")
            .arg(if kind == "new" {
                "tv-rushi new session"
            } else {
                "tv-rushi message"
            })
            .arg(exe)
            .arg("run-pane")
            .arg("--args")
            .arg(&args_file);
        match tm.status() {
            Ok(s) if s.success() => {
                return 0;
            }
            Ok(s) => {
                eprintln!("rushi-sessions: tmux new-pane failed ({s}); using the fork's pane");
            }
            Err(e) => eprintln!("rushi-sessions: cannot start tmux: {e}; using the fork's pane"),
        }
        // The fallback deletes the stale args file (stage 2 did not
        // read it).
        let _ = std::fs::remove_file(&args_file);
    }
    let mut inline = cfg;
    inline.pane = false;
    run_inline_body(&inline)
}

/// The pane's -c dir: the project root (new) or the working dir
/// (message), local rows only (the remote pane keeps the server's
/// cwd, matching the shell oneliner).
fn pane_cwd(kind: &str, r_root: &Option<String>, w: &Option<String>) -> Option<String> {
    match kind {
        "new" => r_root.clone().filter(|r| !r.is_empty()),
        "message" => w.clone().filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// The message-mode working dir: the session's cwd file, else the
/// project root (dirname(dirname(rp))). Remote rows resolve it over
/// ssh; a failure degrades to the empty dir (the remote cd then
/// fails, matching the shell oneliner).
fn resolve_workdir(host: &Option<String>, rp: &str) -> String {
    match host {
        None => local_workdir(rp),
        Some(h) => {
            let cmd = format!(
                "w=$(cat {} 2>/dev/null); [ -d \"$w\" ] || w=$(dirname \"$(dirname {})\"); printf \"%s\" \"$w\"",
                remote_quote(&format!("{}/cwd", rp)),
                remote_quote(rp)
            );
            remote_stdout(h, &cmd)
        }
    }
}

fn local_workdir(rp: &str) -> String {
    let w = std::fs::read_to_string(format!("{rp}/cwd"))
        .ok()
        .map(|s| s.trim().to_string());
    match w {
        Some(w) if !w.is_empty() && Path::new(&w).is_dir() => w,
        _ => parent_dir(rp),
    }
}

/// The new-mode project root: dirname(dirname(rp)), over ssh for
/// remote rows.
fn resolve_project_root(host: &Option<String>, rp: &str) -> String {
    match host {
        None => parent_dir(rp),
        Some(h) => {
            let cmd = format!(
                "printf \"%s\" \"$(dirname \"$(dirname {})\")\"",
                remote_quote(rp)
            );
            remote_stdout(h, &cmd)
        }
    }
}

fn parent_dir(rp: &str) -> String {
    let p = Path::new(rp);
    p.parent()
        .and_then(|d| d.parent())
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Run a remote shell command, return its trimmed stdout (empty on
/// failure). Uses the same ssh flags as the channel's remote reader.
fn remote_stdout(host: &str, cmd: &str) -> String {
    let out = Command::new("ssh")
        .arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg(host)
        .arg(cmd)
        .output()
        .ok();
    match out {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            eprintln!(
                "rushi-sessions: remote resolution for {host} failed; using the local fallback dir"
            );
            String::new()
        }
    }
}

/// The new-session hint line (line 1 of the prompt file).
pub(crate) const HINT_LINE: &str = "# line 1: session name, then the message";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_row_local_and_remote() {
        assert_eq!(
            split_row("/home/a/sessions/x"),
            (None, "/home/a/sessions/x".into())
        );
        assert_eq!(split_row("host:/x"), (Some("host".into()), "/x".into()));
        assert_eq!(
            split_row("user@host:/a:b"),
            (Some("user@host".into()), "/a:b".into())
        );
        // A colon inside a local absolute path with no host part.
        assert_eq!(split_row("/x:y/z"), (None, "/x:y/z".into()));
        // A host-looking first field containing a slash is not a host.
        assert_eq!(split_row("/a:b:/c"), (None, "/a:b:/c".into()));
    }

    #[test]
    fn valid_names() {
        assert!(valid_session_name("new-one"));
        assert!(!valid_session_name(""));
        assert!(!valid_session_name("#tag"));
        assert!(!valid_session_name("a/b"));
        assert!(!valid_session_name("."));
        assert!(!valid_session_name(".."));
    }

    #[test]
    fn b64_vectors() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(b64_encode(b"hi"), "aGk=");
        assert_eq!(b64_encode(b"xy"), "eHk=");
        let msg = "a message with spaces and \u{2014} unicode";
        let enc = b64_encode(msg.as_bytes());
        assert!(!enc.contains('\n'));
        // Round-trip via the system base64 (the remote side's decode).
        let tmp = std::env::temp_dir().join(format!("rsrs-b64-test-{}", std::process::id()));
        std::fs::write(&tmp, &enc).unwrap_or(());
        let out = Command::new("base64").arg("-d").arg(&tmp).output();
        let _ = std::fs::remove_file(&tmp);
        match out {
            Ok(o) if o.status.success() => assert_eq!(String::from_utf8_lossy(&o.stdout), msg),
            _ => {} // no system base64 in the test env: the vector check above suffices
        }
    }

    #[test]
    fn remote_command_shape() {
        let c = remote_run_command(
            "/w/with space",
            "my session",
            Some("QQ=="),
            Some("/r"),
            false,
        );
        assert_eq!(c, "cd '/w/with space' && rushi run 'my session' \"$(printf %s QQ== | base64 -d)\" --sessions-root '/r'");
        let c2 = remote_run_command("/w", "s", None, None, false);
        assert_eq!(c2, "cd '/w' && rushi run 's'");
    }

    /// The devShell launch of the remote row (decision 88): the
    /// same command with the `nix develop -c` prefix.
    #[test]
    fn remote_command_nix_dev_shape() {
        let c = remote_run_command(
            "/w/with space",
            "my session",
            Some("QQ=="),
            Some("/r"),
            true,
        );
        assert_eq!(c, "cd '/w/with space' && nix develop -c rushi run 'my session' \"$(printf %s QQ== | base64 -d)\" --sessions-root '/r'");
        let c2 = remote_run_command("/w", "s", None, None, true);
        assert_eq!(c2, "cd '/w' && nix develop -c rushi run 's'");
    }

    /// Message mode (no --sessions-root) escapes the task with
    /// `--`, so a hyphen-leading decoded message stays the task
    /// positional on kernels without allow_hyphen_values.
    #[test]
    fn remote_command_message_mode_escapes_the_task() {
        let c = remote_run_command("/w", "s", Some("QQ=="), None, false);
        assert_eq!(
            c,
            "cd '/w' && rushi run 's' -- \"$(printf %s QQ== | base64 -d)\""
        );
    }

    /// The remote gate command (decision 88): one read-only ssh -T
    /// call; `yes` only when the remote dir carries a flake and nix
    /// is on the remote non-interactive PATH.
    #[test]
    fn remote_nix_dev_command_shape() {
        let c = remote_nix_dev_command("/r/prj");
        assert_eq!(
            c,
            "if [ -f '/r/prj/flake.nix' ] && command -v nix >/dev/null 2>&1; then echo yes; fi"
        );
    }

    /// The local gate: an empty dir and a flake-less dir stay on
    /// the PATH binary. The flake-present arm also needs nix on
    /// PATH, so it is asserted against the host's own gate state.
    #[test]
    fn nix_dir_gates() {
        let base = std::env::temp_dir().join(format!("rsrs-nixdev-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);

        // No flake.nix: the gate stops at the file check.
        let bare = base.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        assert!(!nix_dev_dir(bare.to_str().unwrap()));

        // An empty dir (the resolution-failure shape) resolves to
        // the PATH binary.
        assert!(!nix_dev_dir(""));

        // The flake-present arm: true only when nix is on the
        // host's PATH (the gate's last check).
        let flake = base.join("flake");
        std::fs::create_dir_all(&flake).unwrap();
        std::fs::write(flake.join("flake.nix"), "{}").unwrap();
        let dir = flake.to_str().unwrap().to_string();
        assert_eq!(nix_dev_dir(&dir), which("nix").is_some());
    }

    /// A cfg fixture for the args-file and tail tests.
    fn cfg_fixture(nix_dev: bool) -> PromptCfg {
        PromptCfg {
            kind: "message",
            rp: "/w/sessions/s".to_string(),
            host: None,
            w: Some("/w".to_string()),
            r_root: None,
            file: "/w/f.md".to_string(),
            editor: "nvim".to_string(),
            hint: None,
            rushi: "/store/bin/rushi".to_string(),
            ssh: "ssh".to_string(),
            detach: "setsid".to_string(),
            nix_dev,
            pane: false,
        }
    }

    #[test]
    fn rushi_run_tail_modes() {
        let cfg = cfg_fixture(false);
        assert_eq!(
            rushi_run_tail(&cfg),
            vec!["/store/bin/rushi".to_string(), "run".to_string()]
        );
        let mut cfg2 = cfg_fixture(true);
        assert_eq!(
            rushi_run_tail(&cfg2),
            vec![
                "nix".to_string(),
                "develop".to_string(),
                "-c".to_string(),
                "rushi".to_string(),
                "run".to_string()
            ]
        );
        assert_eq!(rushi_run_label(&cfg2), "the nix devShell launch");
        cfg2.nix_dev = false;
        assert_eq!(rushi_run_label(&cfg2), "/store/bin/rushi");
    }

    #[test]
    fn args_file_round_trip_nix_dev() {
        let cfg = cfg_fixture(true);
        let p = cfg.to_args_file().unwrap();
        let back = PromptCfg::from_args_file(&p).unwrap();
        assert!(back.nix_dev, "nix_dev survives the args file");
        assert_eq!(back.rushi, "/store/bin/rushi");
        assert!(
            !std::path::Path::new(&p).is_file(),
            "the args file is deleted on read"
        );

        let cfg2 = cfg_fixture(false);
        let p2 = cfg2.to_args_file().unwrap();
        let back2 = PromptCfg::from_args_file(&p2).unwrap();
        assert!(
            !back2.nix_dev,
            "the default args file carries nix_dev false"
        );
    }

    #[test]
    fn blank_check() {
        assert!(is_blank(" \t\n\r"));
        assert!(!is_blank("x"));
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;

    #[test]
    fn args_file_roundtrip_preserves_r_root() {
        let cfg = PromptCfg {
            kind: "new",
            rp: "/tmp/proj".into(),
            host: None,
            w: None,
            r_root: Some("/tmp/proj".into()),
            file: "/tmp/f.md".into(),
            editor: "nvim".into(),
            hint: Some("hint".into()),
            rushi: "rushi".into(),
            ssh: "ssh".into(),
            detach: "setsid".into(),
            nix_dev: false,
            pane: true,
        };
        let p = cfg.to_args_file().unwrap();
        let back = PromptCfg::from_args_file(&p).unwrap();
        assert_eq!(
            back.r_root.as_deref(),
            Some("/tmp/proj"),
            "r_root must survive the args-file round trip"
        );
        assert_eq!(back.kind, "new");
        assert_eq!(back.editor, "nvim");
        let mut msg = cfg.clone();
        msg.kind = "message";
        msg.r_root = None;
        msg.w = Some("/tmp/work".into());
        let p2 = msg.to_args_file().unwrap();
        let back2 = PromptCfg::from_args_file(&p2).unwrap();
        assert_eq!(back2.r_root, None);
        assert_eq!(back2.w.as_deref(), Some("/tmp/work"));
    }
}
