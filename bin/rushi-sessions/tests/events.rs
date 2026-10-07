//! Integration tests for the `events` and `event-preview` subcommands,
//! run against a fixture session dir on disk.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let profile = deps.parent().unwrap();
    profile.join("rushi-sessions")
}

fn fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rushi-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // The second line is intentionally not JSON. It is written raw so
    // it stays unparseable; a json! array would quote it into a valid
    // JSON string.
    let lines = [
        r#"{"type":"user_message","id":"u1","ts":"2026-10-01T01:30:59Z","content":"line one\nline two","v":1}"#,
        "not json at all",
        r#"{"type":"ext_status","id":"loop_phase","ts":"2026-10-01T01:31:08Z","value":"wait","v":1}"#,
        r#"{"type":"tool_call","id":"c1","ts":"2026-10-01T01:31:09Z","name":"bash","arguments":{"command":"ls -la"},"v":1}"#,
        r#"{"type":"tool_result","id":"c1","ts":"2026-10-01T01:31:17Z","is_error":false,"bytes":1537,"value":{"text":"ok output","details":{"exit_code":0,"stderr":null}},"v":1}"#,
        r#"{"type":"cancel","ts":"2026-10-01T01:31:20Z","v":1}"#,
    ]
    .join("\n");
    std::fs::write(d.join("events.jsonl"), lines + "\n").unwrap();
    std::fs::write(d.join("loop.pid"), "999999").unwrap();
    d
}

fn run(args: &[&str], dir: &Path) -> String {
    let out = Command::new(bin())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn events_lists_every_entry_with_seq() {
    let d = fixture("events-list");
    let out = run(&["events"], &d);
    let rows: Vec<&str> = out.trim().lines().collect();
    assert_eq!(rows.len(), 5, "{out}");
    // The bad second line is skipped, but its number stays in the file.
    let seqs: Vec<&str> = rows.iter().map(|r| r.split('\t').next().unwrap()).collect();
    assert_eq!(seqs, vec!["1", "3", "4", "5", "6"], "{out}");
    // Output row 3 is file line 4, the tool_call. The summary keeps
    // the command unclipped.
    let call = rows[2];
    let f: Vec<&str> = call.split('\t').collect();
    assert_eq!(f[1], "tool_call");
    assert_eq!(f[3], "bash ls -la");
    // The user message content flattened to one line, unclipped.
    let user = rows[0].split('\t').collect::<Vec<&str>>();
    assert_eq!(user[3], "line one line two");
}

#[test]
fn event_preview_renders_markdown_for_messages() {
    let d = fixture("ev-md");
    assert_eq!(run(&["event-preview", ".", "1", "--print-lang"], &d), "markdown\n");
    let doc = run(&["event-preview", ".", "1"], &d);
    assert!(doc.starts_with("# user_message\n"), "{doc}");
    assert!(doc.contains("id: u1"), "{doc}");
    assert!(doc.contains("line one\nline two"), "{doc}");
}

#[test]
fn event_preview_renders_toml_for_tool_events() {
    let d = fixture("ev-toml");
    assert_eq!(run(&["event-preview", ".", "4", "--print-lang"], &d), "toml\n");
    let doc = run(&["event-preview", ".", "4"], &d);
    assert!(doc.starts_with("[tool_call]\n"), "{doc}");
    assert!(doc.contains("name = \"bash\""), "{doc}");
    assert!(doc.contains("command = \"ls -la\""), "{doc}");
    // The result keeps its value table and drops the null stderr.
    let res = run(&["event-preview", ".", "5"], &d);
    assert!(res.starts_with("[tool_result]\n"), "{res}");
    assert!(res.contains("text = \"ok output\""), "{res}");
    assert!(!res.contains("stderr"), "{res}");
}

#[test]
fn event_preview_unknown_type_gets_event_table() {
    let d = fixture("ev-other");
    let doc = run(&["event-preview", ".", "6"], &d);
    assert!(doc.starts_with("[event]\n"), "{doc}");
    assert!(doc.contains("type = \"cancel\""), "{doc}");
}

#[test]
fn event_preview_missing_seq_fails() {
    let d = fixture("ev-missing");
    let out = Command::new(bin())
        .args(["event-preview", ".", "99"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn events_without_dir_uses_cwd() {
    let d = fixture("ev-cwd");
    let with_dir = run(&["events", "."], &d);
    let cwd = run(&["events"], &d);
    assert_eq!(with_dir, cwd);
}


