//! The `events` command: one TSV row per entry of events.jsonl, plus the
//! log access helpers shared with the other commands.
//!
//! The list is the collapsed branch view of `branchview`: the active
//! branch plus the history before the diverging node, with the
//! inactive spans folded onto the rows that hide them. The default
//! view takes the cutoff at the log's last seq. With `--branch SEQ`
//! the view takes the picked seq as the cutoff and the picked
//! block as the focus: the parent-branch view of the `branch_view`
//! action. The `chat` view keeps the user and assistant messages and
//! the markers. The active-range projection of the preview card
//! still runs on the kernel's `log_active_ranges`.
use serde_json::Value;
use std::path::{Path, PathBuf};

use rushi_common::rewind::parse_rewind_event;

use crate::format::{flatten_ws, marker_summary, value_inline};

/// The 1-based line numbers of the parseable lines of the session's
/// events.jsonl, paired with those events. Unparseable lines are
/// skipped but still occupy a line number, so the numbering is stable
/// against the log text. The log text is the plain layout read
/// verbatim, or the zstd stream decoded in frame order, through
/// the shared `rushi_common::logstream` reader. A torn tail frame
/// stops at the last complete frame. This is the full-log read:
/// the seq-numbered consumers of a single line read through
/// `event_at` instead, which takes the P8 indexed path.
pub(crate) fn numbered_events(dir: &Path) -> Vec<(u64, Value)> {
    let path = dir.join("events.jsonl");
    let content = match rushi_common::logstream::read_log_text(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            serde_json::from_str::<Value>(l.trim())
                .ok()
                .map(|v| (i as u64 + 1, v))
        })
        .collect()
}

/// The parseable entry at 1-based line `seq`, read through the
/// shared logstream. With a frame index (P8) the read decodes the
/// one record range that holds the line. Without one, or on a
/// plain log, the read falls back to the full text, as
/// `numbered_events` does. An unparseable line at the seq is
/// absent, matching the `numbered_events` numbering.
pub(crate) fn event_at(dir: &Path, seq: u64) -> Option<Value> {
    let path = dir.join("events.jsonl");
    let stream = rushi_common::logstream::open_indexed(&path).ok()?;
    let line = stream.line_at(seq)?;
    serde_json::from_str(line.trim()).ok()
}
/// The active path of one numbered log: the 1-based seq ranges that
/// survive every `rewind` marker, per the kernel's `active_ranges`.
/// A malformed marker is skipped, as the kernel skips it.
pub(crate) fn log_active_ranges(rows: &[(u64, Value)]) -> Vec<(usize, usize)> {
    let refs: Vec<_> = rows
        .iter()
        .filter_map(|(s, v)| parse_rewind_event(v, *s as usize))
        .collect();
    rushi_common::rewind::active_ranges(rows.len(), &refs)
}

/// The collapsed view of the session's events.jsonl: one TSV row per
/// shown entry, the columns are seq, tag, ts, the annotated summary,
/// and the tree-depth indent. The rows come oldest first; the caller
/// prints them reversed. The view is the active branch plus the
/// history before the diverging node: the trunk down to the first
/// branch point, the chain of branch markers to the focus block, the
/// focus block's interior to the cutoff, and the picked block's
/// child markers past the cutoff. The inactive spans fold onto the
/// row that hides them; the summary carries the `(Folded N)` count
/// and the branch annotations (`branched from`, `branched out to`,
/// `hidden`). With `focus`, the picked seq is the cutoff and the
/// picked block the focus (the parent-branch view); the caller
/// validates the focus against the log length. With `chat`, only the
/// user and assistant messages and the markers print.
pub fn events(dir: Option<PathBuf>, chat: bool, focus: Option<u64>) {
    let base = match dir {
        Some(d) => d,
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let rows = numbered_events(&base);
    if let Some(f) = focus {
        let last = rows.last().map(|(s, _)| *s).unwrap_or(0);
        if f < 1 || (last > 0 && f > last) {
            eprintln!(
                "rushi-sessions: seq {f} is out of range (the log has {last} entries)"
            );
            std::process::exit(1);
        }
    }
    let view = crate::branchview::view_rows(&rows, focus, chat);
    for row in view.iter().rev() {
        println!("{row}");
    }
}


/// The one-line summary of an event for the list column. It keeps the
/// whole payload; it only flattens tabs and newlines to spaces so the
/// TSV row cannot break.
pub(crate) fn event_summary(o: &Value) -> String {
    let t = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match t {
        "rewind" | "user_message_retract" => marker_summary(o),
        "tool_call" => {
            let name = o
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("?");
            let a = o.get("arguments").unwrap_or(&Value::Null);
            let cmd = a
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let extra = if !cmd.is_empty() {
                cmd
            } else if a.is_object() {
                serde_json::to_string(a).unwrap_or_default()
            } else {
                String::new()
            };
            if extra.is_empty() {
                flatten_ws(name)
            } else {
                flatten_ws(&format!("{name} {extra}"))
            }
        }
        "tool_result" => {
            let mut s = match o.get("is_error").and_then(|v| v.as_bool()) {
                Some(true) => "error".to_string(),
                _ => "ok".to_string(),
            };
            if let Some(b) = o.get("bytes").and_then(|v| v.as_i64()) {
                s = format!("{s} {b}b");
            }
            if let Some(v) = o.get("value").filter(|v| !v.is_null()) {
                s.push(' ');
                s.push_str(&serde_json::to_string(v).unwrap_or_default());
            }
            flatten_ws(&s)
        }
        "ext_status" => {
            let id = o.get("id").and_then(|v| v.as_str()).unwrap_or("status");
            let s = match o.get("value") {
                None | Some(Value::Null) => id.to_string(),
                Some(v) => format!("{id}={}", value_inline(v)),
            };
            flatten_ws(&s)
        }
        "compaction_summary" => {
            let mut s = String::new();
            if let Some(r) = o.get("reason").and_then(|v| v.as_str()) {
                s = format!("reason={r} ");
            }
            s.push_str(&flatten_ws(
                o.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
            ));
            s
        }
        _ => {
            let c = o.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if c.is_empty() {
                flatten_ws(&serde_json::to_string(o).unwrap_or_default())
            } else {
                flatten_ws(c)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_summary_tool_call_prefers_command() {
        let o = serde_json::json!({"type": "tool_call", "name": "bash",
            "arguments": {"command": "ls -la"}});
        assert_eq!(event_summary(&o), "bash ls -la");
    }

    #[test]
    fn event_summary_tool_call_falls_back_to_arguments() {
        let o = serde_json::json!({"type": "tool_call", "name": "read",
            "arguments": {"file_path": "/x"}});
        assert_eq!(event_summary(&o), "read {\"file_path\":\"/x\"}");
    }

    #[test]
    fn event_summary_tool_result_carries_value() {
        let o = serde_json::json!({"type": "tool_result", "is_error": true,
            "bytes": 12, "value": {"text": "boom"}});
        assert_eq!(event_summary(&o), "error 12b {\"text\":\"boom\"}");
    }

    #[test]
    fn event_summary_user_message_flattens() {
        let o = serde_json::json!({"type": "user_message", "content": "a\tb\nc"});
        assert_eq!(event_summary(&o), "a b c");
    }

    #[test]
    fn event_summary_ext_status_and_compaction() {
        let a = serde_json::json!({"type": "ext_status", "id": "loop_phase", "value": "wait"});
        assert_eq!(event_summary(&a), "loop_phase=wait");
        let b = serde_json::json!({"type": "compaction_summary", "reason": "threshold",
            "summary": "## Goal\nDo it"});
        assert_eq!(event_summary(&b), "reason=threshold ## Goal Do it");
    }

    #[test]
    fn numbered_events_skip_bad_lines() {
        let d = std::env::temp_dir().join(format!("rushi-evtest-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("events.jsonl"),
            "{\"type\":\"user_message\",\"content\":\"one\"}\nnot json\n{\"type\":\"cancel\"}\n",
        )
        .unwrap();
        let rows = numbered_events(&d);
        let seqs: Vec<u64> = rows.iter().map(|(n, _)| *n).collect();
        assert_eq!(seqs, vec![1, 3]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn event_summary_rewind_and_retract_markers() {
        let r = serde_json::json!({"type": "rewind", "ts": "2026-10-08T07:01:00Z",
            "target_seq": 2, "mode": "on", "reason": "tui_pick"});
        assert_eq!(event_summary(&r), "rewind -> seq 2 (on, tui_pick)");
        let r2 = serde_json::json!({"type": "rewind", "target_seq": 5, "mode": "before"});
        assert_eq!(event_summary(&r2), "rewind -> seq 5 (before)");
        let x = serde_json::json!({"type": "user_message_retract", "target": "abc",
            "reason": "user_edit"});
        assert_eq!(event_summary(&x), "retract abc (user_edit)");
    }

    #[test]
    fn log_active_ranges_masks_abandoned_spans() {
        let v = |t: &str| serde_json::json!({"type": t});
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, v("assistant_message")),
            (3, v("rewind")),
            (4, v("user_message")),
            (5, v("assistant_message")),
            (
                6,
                serde_json::json!({"type": "rewind", "target_seq": 4,
                "mode": "on", "reason": "tui_pick"}),
            ),
            (7, v("user_message")),
        ];
        // The marker at seq 3 is malformed (no target_seq), so the
        // kernel skips it. The marker at seq 6 (target 4, on mode)
        // keeps 1..4 and 7..end. Seq 5 is the abandoned branch.
        let ranges = log_active_ranges(&rows);
        assert_eq!(ranges, vec![(1, 4), (7, 7)]);
    }

    #[test]
    fn branch_view_folds_the_abandoned_span() {
        let v = |t: &str| serde_json::json!({"type": t});
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, v("assistant_message")),
            (3, v("tool_call")),
            (
                4,
                serde_json::json!({"type": "rewind", "target_seq": 2,
                "mode": "on"}),
            ),
            (5, v("user_message")),
            (6, v("assistant_message")),
        ];
        // The main view: the active path {1, 2, 5, 6} plus the
        // marker row 4. The abandoned tool_call (seq 3) folds onto
        // the branch-point row 2.
        let all: Vec<u64> = crate::branchview::view_rows(&rows, None, false)
            .iter()
            .map(|r| r.split('\t').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(all, vec![1, 2, 4, 5, 6]);
        let row2 = crate::branchview::view_rows(&rows, None, false)
            .into_iter()
            .find(|r| r.starts_with("2\t"))
            .unwrap();
        assert!(row2.contains("(Folded 1)"), "{}", row2);
        assert!(row2.contains("branched out to #3"), "{}", row2);
        let row4 = crate::branchview::view_rows(&rows, None, false)
            .into_iter()
            .find(|r| r.starts_with("4\t"))
            .unwrap();
        assert!(row4.contains("branched from #1"), "{}", row4);
        // The chat view keeps the messages and the marker, drops the
        // tool rows.
        let chat: Vec<u64> = crate::branchview::view_rows(&rows, None, true)
            .iter()
            .map(|r| r.split('\t').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(chat, vec![1, 2, 4, 5, 6]);
    }

    /// One zstd frame per line, the kernel's per-commit layout (P1).
    fn zst_line(s: &str) -> Vec<u8> {
        zstd::encode_all(s.as_bytes(), 9).unwrap()
    }

    /// One temp dir per test, unique against the sibling tests of
    /// this binary (same pid, parallel threads).
    static ZST_DIR_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn zst_dir() -> std::path::PathBuf {
        let n = ZST_DIR_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("rushi-evzst-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn numbered_events_reads_zstd_streams() {
        // The kernel per-commit layout (P1): one zstd frame per
        // line. The seq numbering is the decoded line number, the
        // same as the plain layout.
        let d = zst_dir();
        let mut buf = zst_line(
            r#"{"type":"user_message","content":"one"}
"#,
        );
        buf.extend(zst_line(
            r#"{"type":"assistant_message","content":"two"}
"#,
        ));
        buf.extend(zst_line(
            r#"{"type":"cancel"}
"#,
        ));
        std::fs::write(d.join("events.jsonl"), &buf).unwrap();
        let rows = numbered_events(&d);
        let seqs: Vec<u64> = rows.iter().map(|(n, _)| *n).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
        let types: Vec<&str> = rows
            .iter()
            .map(|(_, v)| v.get("type").and_then(|t| t.as_str()).unwrap_or(""))
            .collect();
        assert_eq!(types, vec!["user_message", "assistant_message", "cancel"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn numbered_events_keeps_complete_frames_of_a_torn_tail() {
        // P4: a torn final frame keeps the complete frames. The
        // torn line is unparseable, so it drops out of the rows.
        let d = zst_dir();
        let ok = zst_line(
            r#"{"type":"user_message","content":"one"}
"#,
        );
        let torn = zst_line(
            r#"{"type":"assistant_message","content":"two"}
"#,
        );
        let mut buf = ok;
        buf.extend_from_slice(&torn[..torn.len() / 2]);
        std::fs::write(d.join("events.jsonl"), &buf).unwrap();
        let rows = numbered_events(&d);
        // The complete frame parses. A partial line, when present,
        // fails the JSON parse and drops.
        assert!(!rows.is_empty());
        assert_eq!(
            rows[0].1.get("type").and_then(|t| t.as_str()),
            Some("user_message")
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
