//! Integration tests for the `rewind` and `summarize` subcommands,
//! run against a fixture session dir and a stubbed kernel bin dir
//! (the `rushi`/`rushi-log`/`log`/`compact` stubs, the real
//! kernel's siblings across the 58 rename).

use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let profile = deps.parent().unwrap();
    profile.join("rushi-sessions")
}

struct Fixture {
    /// The session dir.
    dir: PathBuf,
    /// The project dir (the `cwd` file's target).
    project: PathBuf,
    /// The stub kernel bin dir (the `rushi`/`rushi-log`/`log`/
    /// `compact` stubs).
    stubs: PathBuf,
}

/// One fixture session: a log with a fork, a `cwd` file, and the
/// stub kernel bins. The stubs record their calls through the
/// `STUB_*` env vars the test sets.
fn fixture(tag: &str, with_marker: bool) -> Fixture {
    let d = std::env::temp_dir().join(format!("rushi-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let session = d.join("sessions").join(tag);
    let project = d.join("project");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let mut lines = vec![
        r#"{"type":"user_message","ts":"2026-10-08T07:00:00Z","content":"first user msg","v":1}"#,
        r#"{"type":"assistant_message","ts":"2026-10-08T07:00:10Z","content":"first answer","v":1}"#,
        r#"{"type":"tool_call","ts":"2026-10-08T07:00:20Z","name":"bash","arguments":{"command":"ls"},"v":1}"#,
    ];
    if with_marker {
        lines.push(
            r#"{"type":"rewind","ts":"2026-10-08T07:01:00Z","target_seq":2,"mode":"on","reason":"tui_pick","v":1}"#,
        );
        lines.push(
            r#"{"type":"user_message","ts":"2026-10-08T07:01:10Z","content":"re-asked","v":1}"#,
        );
        lines.push(
            r#"{"type":"assistant_message","ts":"2026-10-08T07:01:20Z","content":"branch B","v":1}"#,
        );
    }
    std::fs::write(session.join("events.jsonl"), lines.join("\n") + "\n").unwrap();
    let cwd = project.to_string_lossy().into_owned();
    std::fs::write(session.join("cwd"), cwd).unwrap();
    // The stub kernel bin dir: `rushi` (the resolver's anchor),
    // `rushi-log` and `log` (the marker appender across the 58
    // rename, new name first in the channel's name list),
    // `compact` (the branch summary). The stubs record their
    // calls through the STUB_* env vars; the stub echo labels the
    // name that won the resolve.
    let stubs = d.join("bin");
    std::fs::create_dir_all(&stubs).unwrap();
    std::fs::write(stubs.join("rushi"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(
        stubs.join("rushi-log"),
        "#!/bin/sh\ncat > \"$STUB_LOG_FILE\"; echo \"stub-rushi-log $@\" >> \"$STUB_LOG_FILE\"\n",
    )
    .unwrap();
    std::fs::write(
        stubs.join("log"),
        "#!/bin/sh\ncat > \"$STUB_LOG_FILE\"; echo \"stub-log $@\" >> \"$STUB_LOG_FILE\"\n",
    )
    .unwrap();
    std::fs::write(
        stubs.join("compact"),
        "#!/bin/sh\necho \"$@\" > \"$STUB_COMPACT_FILE\"; echo \"$(pwd)\" >> \"$STUB_COMPACT_FILE\"\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    for name in ["rushi", "rushi-log", "log", "compact"] {
        let p = stubs.join(name);
        let mut perms = std::fs::metadata(&p).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&p, perms).unwrap();
    }
    Fixture {
        dir: session,
        project,
        stubs,
    }
}

/// Run the binary with the stub bin dir on PATH: the kernel binaries
/// resolve to the stubs. The session dir is passed through `--dir`.
fn run(f: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> (i32, String, String) {
    let mut c = Command::new(bin());
    c.args(args).current_dir(&f.dir);
    c.env(
        "PATH",
        format!(
            "{}:{}",
            f.stubs.to_string_lossy(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    for (k, v) in extra {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn dir_arg(f: &Fixture) -> String {
    f.dir.to_str().unwrap().to_string()
}

fn hold_loop_lock(dir: &Path) -> std::fs::File {
    // Hold the kernel's loop lock for the test's duration: the probe
    // sees a held flock and reports a live loop.
    let p = dir.join(".loop.lock");
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&p)
        .unwrap();
    let fd = f.as_raw_fd();
    assert_eq!(
        unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) },
        0,
        "the test holds the loop lock"
    );
    f
}

#[test]
fn rewind_refuses_a_live_loop() {
    let f = fixture("br-busy", true);
    let _lock = hold_loop_lock(&f.dir);
    let (code, out, err) = run(
        &f,
        &["rewind", "3", "--dir", &dir_arg(&f)],
        &[("STUB_LOG_FILE", f.dir.to_str().unwrap())],
    );
    assert_eq!(code, 1, "stdout: {out} stderr: {err}");
    assert!(out.contains("loop busy, wait for the step"), "{out}");
}

#[test]
fn rewind_appends_the_marker_for_a_user_message_target() {
    let f = fixture("br-um", true);
    let log_file = f.dir.join("log-calls.txt");
    let (code, out, err) = run(
        &f,
        &["rewind", "5", "--dir", &dir_arg(&f)],
        &[("STUB_LOG_FILE", log_file.to_str().unwrap())],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    assert!(out.contains("rewound to seq 5 (before)"), "{out}");
    assert!(out.contains("left the context"), "{out}");
    let calls = std::fs::read_to_string(&log_file).unwrap();
    // The marker JSON and the `--session` arg of the appender call.
    // With both stubs present, the new name wins the name list.
    let marker = calls.lines().next().unwrap();
    let v: serde_json::Value = serde_json::from_str(marker).unwrap();
    assert_eq!(v["type"], "rewind");
    assert_eq!(v["target_seq"], 5);
    assert_eq!(v["mode"], "before");
    assert_eq!(v["reason"], "tv_pick");
    assert!(calls.contains("--session"), "{calls}");
}

/// Without the new `rushi-log` stub (an older kernel build) the
/// name list falls through to the legacy `log` stub: both
/// generations of the kernel stay launchable.
#[test]
fn rewind_resolves_the_legacy_log_name_without_rushi_log() {
    let f = fixture("br-legacy", true);
    let _ = std::fs::remove_file(f.stubs.join("rushi-log"));
    let log_file = f.dir.join("log-calls.txt");
    let (code, out, err) = run(
        &f,
        &["rewind", "5", "--dir", &dir_arg(&f)],
        &[("STUB_LOG_FILE", log_file.to_str().unwrap())],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    let calls = std::fs::read_to_string(&log_file).unwrap();
    let marker = calls.lines().next().unwrap();
    let v: serde_json::Value = serde_json::from_str(marker).unwrap();
    assert_eq!(v["type"], "rewind");
    assert!(calls.contains("stub-log"), "{calls}");
    assert!(!calls.contains("stub-rushi-log"), "{calls}");
}

#[test]
fn rewind_appends_the_marker_for_a_tool_target() {
    let f = fixture("br-tool", true);
    let log_file = f.dir.join("log-calls.txt");
    let (code, out, err) = run(
        &f,
        &["rewind", "3", "--dir", &dir_arg(&f)],
        &[("STUB_LOG_FILE", log_file.to_str().unwrap())],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    let calls = std::fs::read_to_string(&log_file).unwrap();
    let marker = calls.lines().next().unwrap();
    let v: serde_json::Value = serde_json::from_str(marker).unwrap();
    assert_eq!(v["mode"], "on");
    assert_eq!(v["target_seq"], 3);
}

#[test]
fn rewind_rejects_a_seq_past_the_end() {
    let f = fixture("br-past", true);
    let (code, out, err) = run(
        &f,
        &["rewind", "99", "--dir", &dir_arg(&f)],
        &[("STUB_LOG_FILE", f.dir.to_str().unwrap())],
    );
    assert_eq!(code, 1, "stdout: {out} stderr: {err}");
    assert!(err.contains("past the end"), "{err}");
}

#[test]
fn summarize_without_a_marker_is_one_shot() {
    // No rewind marker in the log: the one-shot appends one at the
    // picked seq, then runs the compact (rushi-tui #37/#38).
    let f = fixture("br-nomarker", false);
    let log_file = f.dir.join("log-calls.txt");
    let compact_file = f.dir.join("compact-calls.txt");
    let (code, out, err) = run(
        &f,
        &["summarize", "1", "--dir", &dir_arg(&f)],
        &[
            ("STUB_LOG_FILE", log_file.to_str().unwrap()),
            ("STUB_COMPACT_FILE", compact_file.to_str().unwrap()),
        ],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    assert!(out.contains("summarized the branch left of seq 1"), "{out}");
    // The marker append comes first: mode `on` (the picked event
    // stays in the context), the channel's reason.
    let calls = std::fs::read_to_string(&log_file).unwrap();
    let marker = calls.lines().next().unwrap();
    let v: serde_json::Value = serde_json::from_str(marker).unwrap();
    assert_eq!(v["type"], "rewind");
    assert_eq!(v["target_seq"], 1);
    assert_eq!(v["mode"], "on");
    assert_eq!(v["reason"], "tv_pick");
    let compact = std::fs::read_to_string(&compact_file).unwrap();
    let args = compact.lines().next().unwrap();
    assert!(args.contains("--branch"), "{args}");
}

#[test]
fn summarize_on_a_masked_pick_reenters_the_branch() {
    // Seq 3 is in the abandoned span of the marker at seq 4. The
    // one-shot commits: the marker re-enters the abandoned branch,
    // the compact runs on the new open span.
    let f = fixture("br-masked", true);
    let log_file = f.dir.join("log-calls.txt");
    let compact_file = f.dir.join("compact-calls.txt");
    let (code, out, err) = run(
        &f,
        &["summarize", "3", "--dir", &dir_arg(&f)],
        &[
            ("STUB_LOG_FILE", log_file.to_str().unwrap()),
            ("STUB_COMPACT_FILE", compact_file.to_str().unwrap()),
        ],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    let calls = std::fs::read_to_string(&log_file).unwrap();
    let marker = calls.lines().next().unwrap();
    let v: serde_json::Value = serde_json::from_str(marker).unwrap();
    assert_eq!(v["target_seq"], 3);
    assert_eq!(v["mode"], "on");
}

#[test]
fn summarize_rejects_a_seq_past_the_end() {
    let f = fixture("br-past-sum", false);
    let (code, out, err) = run(
        &f,
        &["summarize", "99", "--dir", &dir_arg(&f)],
        &[("STUB_LOG_FILE", f.dir.to_str().unwrap())],
    );
    assert_eq!(code, 1, "stdout: {out} stderr: {err}");
    assert!(err.contains("past the end"), "{err}");
}

#[test]
fn summarize_calls_compact_branch_in_the_recorded_dir() {
    let f = fixture("br-compact", true);
    let compact_file = f.dir.join("compact-calls.txt");
    let log_file = f.dir.join("log-calls.txt");
    let (code, out, err) = run(
        &f,
        &["summarize", "6", "--dir", &dir_arg(&f)],
        &[
            ("STUB_LOG_FILE", log_file.to_str().unwrap()),
            ("STUB_COMPACT_FILE", compact_file.to_str().unwrap()),
        ],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    // The one-shot marker append comes before the compact.
    let logs = std::fs::read_to_string(&log_file).unwrap();
    let m: serde_json::Value = serde_json::from_str(logs.lines().next().unwrap()).unwrap();
    assert_eq!(m["target_seq"], 6);
    assert_eq!(m["mode"], "on");
    let calls = std::fs::read_to_string(&compact_file).unwrap();
    // The args: the session dir, `--branch`, no `--prompt`.
    let args = calls.lines().next().unwrap();
    assert!(args.contains("--branch"), "{args}");
    assert!(!args.contains("--prompt"), "{args}");
    assert!(args.contains(f.dir.to_str().unwrap()), "{args}");
    // The CWD is the session's recorded working dir.
    let cwd = calls.lines().nth(1).unwrap();
    assert_eq!(Path::new(cwd), f.project.as_path(), "{cwd}");
}

#[test]
fn summarize_prompt_uses_the_editor_file() {
    let f = fixture("br-prompt", true);
    let compact_file = f.dir.join("compact-calls.txt");
    // The editor stub writes the instruction into its file argument.
    let editor = f.stubs.join("fake-editor");
    std::fs::write(&editor, "#!/bin/sh\nprintf 'focus the summary' > \"$1\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&editor).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&editor, perms).unwrap();
    let (code, out, err) = run(
        &f,
        &["summarize", "6", "--dir", &dir_arg(&f), "--prompt"],
        &[
            (
                "STUB_LOG_FILE",
                f.dir.join("log-calls.txt").to_str().unwrap(),
            ),
            ("STUB_COMPACT_FILE", compact_file.to_str().unwrap()),
            ("EDITOR", editor.to_str().unwrap()),
        ],
    );
    assert_eq!(code, 0, "stdout: {out} stderr: {err}");
    let calls = std::fs::read_to_string(&compact_file).unwrap();
    let args = calls.lines().next().unwrap();
    assert!(args.contains("--prompt focus the summary"), "{args}");
}

#[test]
fn summarize_prompt_cancels_on_a_blank_file() {
    let f = fixture("br-blank", true);
    let editor = f.stubs.join("fake-editor-blank");
    std::fs::write(&editor, "#!/bin/sh\n: > \"$1\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&editor).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&editor, perms).unwrap();
    let (code, out, err) = run(
        &f,
        &["summarize", "6", "--dir", &dir_arg(&f), "--prompt"],
        &[
            (
                "STUB_LOG_FILE",
                f.dir.join("log-calls.txt").to_str().unwrap(),
            ),
            (
                "STUB_COMPACT_FILE",
                f.dir.join("compact-calls.txt").to_str().unwrap(),
            ),
            ("EDITOR", editor.to_str().unwrap()),
        ],
    );
    assert_eq!(code, 1, "stdout: {out} stderr: {err}");
    assert!(out.contains("No instruction entered"), "{out}");
}

#[test]
fn events_channel_mode_uses_the_cwd() {
    // No `--dir`: the CWD (the fixture session dir) is the session
    // dir, the events channel's mode.
    let f = fixture("br-cwd", true);
    let log_file = f.dir.join("log-calls.txt");
    let mut c = Command::new(bin());
    c.args(["rewind", "5"])
        .current_dir(&f.dir)
        .env(
            "PATH",
            format!(
                "{}:{}",
                f.stubs.to_string_lossy(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("STUB_LOG_FILE", log_file.as_os_str());
    let out = c.output().unwrap();
    assert_eq!(out.status.code().unwrap(), 0);
    let calls = std::fs::read_to_string(&log_file).unwrap();
    assert!(calls.contains("\"target_seq\":5"), "{calls}");
}
