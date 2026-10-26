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
        r#"{"type":"assistant_message","id":"a1","ts":"2026-10-01T01:31:09Z","content":"the assistant replies","v":1}"#,
        r#"{"type":"tool_call","id":"c1","ts":"2026-10-01T01:31:09Z","name":"bash","arguments":{"command":"ls -la"},"v":1}"#,
        r#"{"type":"tool_result","id":"c1","ts":"2026-10-01T01:31:17Z","is_error":false,"bytes":1537,"value":{"text":"ok output","details":{"exit_code":0,"stderr":null}},"v":1}"#,
        r#"{"type":"cancel","ts":"2026-10-01T01:31:20Z","v":1}"#,
    ]
    .join("\n");
    std::fs::write(d.join("events.jsonl"), lines + "\n").unwrap();
    std::fs::write(d.join("loop.pid"), "999999").unwrap();
    d
}

fn rewind_fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rushi-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let lines = [
        r#"{"type":"user_message","ts":"2026-10-08T07:00:00Z","content":"first user msg","v":1}"#,
        r#"{"type":"assistant_message","ts":"2026-10-08T07:00:10Z","content":"first answer","v":1}"#,
        r#"{"type":"tool_call","ts":"2026-10-08T07:00:20Z","name":"bash","arguments":{"command":"ls"},"v":1}"#,
        r#"{"type":"rewind","ts":"2026-10-08T07:01:00Z","target_seq":2,"mode":"on","reason":"tui_pick","v":1}"#,
        r#"{"type":"user_message","ts":"2026-10-08T07:01:10Z","content":"re-asked","v":1}"#,
        r#"{"type":"assistant_message","ts":"2026-10-08T07:01:20Z","content":"branch B","v":1}"#,
    ]
    .join("\n");
    std::fs::write(d.join("events.jsonl"), lines + "\n").unwrap();
    d
}

fn run(args: &[&str], dir: &Path) -> String {
    let out = Command::new(bin())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn events_lists_every_entry_newest_first() {
    let d = fixture("events-list");
    let out = run(&["events"], &d);
    let rows: Vec<&str> = out.trim().lines().collect();
    assert_eq!(rows.len(), 6, "{out}");
    // The bad second line is skipped, but its number stays in the file.
    // The rows come newest first: the highest line number prints first.
    let seqs: Vec<&str> = rows.iter().map(|r| r.split('\t').next().unwrap()).collect();
    assert_eq!(seqs, vec!["7", "6", "5", "4", "3", "1"], "{out}");
    // Output row 3 is file line 5, the tool_call. The summary keeps
    // the command unclipped. The log has no rewind marker, so every
    // row sits on the trunk: the indent column is empty.
    let call = rows[2];
    let f: Vec<&str> = call.split('\t').collect();
    assert_eq!(f[1], "C");
    assert_eq!(f[3], "bash ls -la");
    assert_eq!(f[4], "", "trunk row carries no indent: {out}");
    // The user message content flattened to one line, unclipped.
    let user = rows[5].split('\t').collect::<Vec<&str>>();
    assert_eq!(user[1], "U");
    assert_eq!(user[3], "line one line two");
    // The ts column is the local wall-clock time: 19 chars, no Z.
    let ts = f[2];
    assert_eq!(ts.len(), 19, "{out}");
    assert!(!ts.contains('Z'), "{out}");
}

#[test]
fn events_chat_lists_only_the_messages() {
    let d = fixture("events-chat");
    let out = run(&["events", "--chat"], &d);
    let rows: Vec<&str> = out.trim().lines().collect();
    // Only the user and assistant messages, newest first.
    let seqs: Vec<&str> = rows.iter().map(|r| r.split('\t').next().unwrap()).collect();
    assert_eq!(seqs, vec!["4", "1"], "{out}");
    let tags: Vec<&str> = rows.iter().map(|r| r.split('\t').nth(1).unwrap()).collect();
    assert_eq!(tags, vec!["A", "U"], "{out}");
    // The unfiltered list still carries every entry.
    let all = run(&["events"], &d);
    assert_eq!(all.trim().lines().count(), 6, "{all}");
}

#[test]
fn event_preview_renders_markdown_for_messages() {
    let d = fixture("ev-md");
    assert_eq!(
        run(&["event-preview", ".", "1", "--print-lang"], &d),
        "markdown\n"
    );
    let doc = run(&["event-preview", ".", "1"], &d);
    assert!(doc.starts_with("# user_message\n"), "{doc}");
    // The id of a user_message does not print.
    assert!(!doc.contains("\nid: "), "{doc}");
    assert!(doc.contains("line one\nline two"), "{doc}");
    // The ts metadata is local wall-clock time: 19 chars, no Z.
    let ts = doc
        .lines()
        .find(|l| l.starts_with("ts: "))
        .map(|l| l.trim_start_matches("ts: "))
        .unwrap();
    assert_eq!(ts.len(), 19, "{doc}");
    assert!(!ts.contains('Z'), "{doc}");
}

#[test]
fn event_preview_assistant_message_keeps_its_id() {
    let d = fixture("ev-md2");
    let doc = run(&["event-preview", ".", "4"], &d);
    assert!(doc.starts_with("# assistant_message\n"), "{doc}");
    assert!(doc.contains("\nid: a1"), "{doc}");
}

#[test]
fn event_preview_renders_toml_for_tool_events() {
    let d = fixture("ev-toml");
    assert_eq!(
        run(&["event-preview", ".", "5", "--print-lang"], &d),
        "toml\n"
    );
    let doc = run(&["event-preview", ".", "5"], &d);
    assert!(doc.starts_with("[tool_call]\n"), "{doc}");
    assert!(doc.contains("name = \"bash\""), "{doc}");
    assert!(doc.contains("command = \"ls -la\""), "{doc}");
    // The call ids of a tool_call do not print.
    assert!(!doc.contains("id"), "{doc}");
    assert!(!doc.contains("call_id"), "{doc}");
    // The ts of the table is local wall-clock time: 19 chars, no Z.
    let ts = doc
        .lines()
        .find(|l| l.starts_with("ts = "))
        .and_then(|l| l.trim_start_matches("ts = ").split('"').nth(1))
        .unwrap();
    assert_eq!(ts.len(), 19, "{doc}");
    assert!(!ts.contains('Z'), "{doc}");
    // The result keeps its value table and drops the null stderr.
    let res = run(&["event-preview", ".", "6"], &d);
    assert!(res.starts_with("[tool_result]\n"), "{res}");
    assert!(res.contains("text = \"ok output\""), "{res}");
    assert!(!res.contains("stderr"), "{res}");
    // The tool_result keeps its id: only the tool_call drops it.
    assert!(res.contains("id = \"c1\""), "{res}");
}

#[test]
fn event_preview_table_named_by_type() {
    let d = fixture("ev-other");
    let doc = run(&["event-preview", ".", "7"], &d);
    assert!(doc.starts_with("[cancel]\n"), "{doc}");
    assert!(doc.contains("type = \"cancel\""), "{doc}");
}

#[test]
fn events_all_view_is_the_collapsed_main_view() {
    let d = rewind_fixture("ev-rewind-all");
    let out = run(&["events"], &d);
    let rows: Vec<&str> = out.trim().lines().collect();
    // The active path {1, 2, 5, 6} plus the marker row 4. The
    // abandoned tool_call (seq 3) does not print; it folds onto the
    // branch-point row 2.
    let seqs: Vec<&str> = rows.iter().map(|r| r.split('\t').next().unwrap()).collect();
    assert_eq!(seqs, vec!["6", "5", "4", "2", "1"], "{out}");
    let marker = rows.iter().find(|r| r.starts_with("4\t")).unwrap();
    assert!(
        marker.contains("rewind -> seq 2 (on, tui_pick)"),
        "{marker}"
    );
    assert!(marker.contains("branched from #1"), "{marker}");
    assert!(marker.contains("1 hidden"), "{marker}");
    let branchpoint = rows.iter().find(|r| r.starts_with("2\t")).unwrap();
    assert!(branchpoint.contains("(Folded 1)"), "{branchpoint}");
    assert!(branchpoint.contains("branched out to #3"), "{branchpoint}");
    assert!(branchpoint.contains('+'), "the fold row carries the + marker: {branchpoint}");
}

#[test]
fn events_branch_view_shows_the_parent_branch() {
    let d = branch_fixture("ev-branch-view");
    // The picked marker (seq 8, item 7) is the cutoff; the focus is
    // the block that contains its target: the branch at seq 5. The
    // view shows the trunk down to the branch point, the focus
    // block's interior, and the picked marker.
    let out = run(&["events", "--branch", "8"], &d);
    let seqs: Vec<&str> = out
        .trim()
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    assert_eq!(seqs, vec!["8", "7", "6", "5", "2", "1"], "{out}");
    let rows1 = rows_of(&out);
    let cutoff = rows1.get("8").unwrap();
    assert!(cutoff.contains("(Folded 1)"), "{cutoff}");
    let marker4 = rows1.get("5").unwrap();
    assert!(!marker4.contains("(Folded"), "{marker4}");
    // The picked block's child marker past the cutoff shows as a
    // folded line when the focus is the parent (the trunk).
    let out2 = run(&["events", "--branch", "5"], &d);
    let seqs2: Vec<&str> = out2
        .trim()
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    assert_eq!(seqs2, vec!["8", "5", "4", "3", "2", "1"], "{out2}");
    let rows2 = rows_of(&out2);
    let child = rows2.get("8").unwrap();
    assert!(child.contains("(Folded 1)"), "{child}");
    assert!(child.contains("branched from #4"), "{child}");
}

fn rows_of(out: &str) -> std::collections::HashMap<&str, &str> {
    out.trim()
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .map(|t| t)
        .map(|k| {
            let v = out
                .trim()
                .lines()
                .find(|l| l.starts_with(&format!("{k}\t")))
                .unwrap();
            (k, v)
        })
        .collect()
}

fn branch_fixture(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rushi-sessions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let lines = [
        r#"{"type":"user_message","ts":"2026-10-08T07:00:00Z","content":"item 0","v":1}"#,
        r#"{"type":"user_message","ts":"2026-10-08T07:00:05Z","content":"item 1","v":1}"#,
        r#"{"type":"assistant_message","ts":"2026-10-08T07:00:10Z","content":"item 2","v":1}"#,
        r#"{"type":"assistant_message","ts":"2026-10-08T07:00:15Z","content":"item 3","v":1}"#,
        r#"{"type":"rewind","ts":"2026-10-08T07:01:00Z","target_seq":2,"mode":"on","reason":"tv_pick","v":1}"#,
        r#"{"type":"user_message","ts":"2026-10-08T07:01:10Z","content":"item 5","v":1}"#,
        r#"{"type":"assistant_message","ts":"2026-10-08T07:01:20Z","content":"item 6","v":1}"#,
        r#"{"type":"rewind","ts":"2026-10-08T07:02:00Z","target_seq":5,"mode":"on","reason":"tv_pick","v":1}"#,
        r#"{"type":"user_message","ts":"2026-10-08T07:02:10Z","content":"item 8","v":1}"#,
    ]
    .join("\n");
    std::fs::write(d.join("events.jsonl"), lines + "\n").unwrap();
    d
}

#[test]
fn events_without_dir_uses_cwd() {
    let d = fixture("ev-cwd");
    let with_dir = run(&["events", "."], &d);
    let cwd = run(&["events"], &d);
    assert_eq!(with_dir, cwd);
}
