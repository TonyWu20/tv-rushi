//! The `events` command: one TSV row per entry of events.jsonl, plus the
//! log access helpers shared with the other commands.
//!
//! Rewound sessions carry the kernel's `rewind` markers in the same
//! log. The active-path math comes from `rushi-common`, the kernel's
//! shared crate: `log_active_ranges` projects the log onto the spans
//! that survive every marker. The `chat` view shows the active path
//! only. The full view keeps every row and tags the masked ones.
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use rushi_common::rewind::{parse_rewind_event, seq_in_ranges};

use crate::format::{flatten_ws, marker_summary, ts_local, value_inline};

/// The 1-based line numbers of the parseable lines of the session's
/// events.jsonl, paired with those events. Unparseable lines are
/// skipped but still occupy a line number, so the numbering is stable
/// against the raw file.
pub(crate) fn numbered_events(dir: &Path) -> Vec<(u64, Value)> {
    let path = dir.join("events.jsonl");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .enumerate()
        .filter_map(|(i, l)| serde_json::from_str::<Value>(l.trim()).ok().map(|v| (i as u64 + 1, v)))
        .collect()
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
/// One TSV row per numbered event, oldest first. The caller prints
/// them reversed. The columns are seq, type, ts, summary. With
/// `chat`, only the active-path user and assistant messages print.
/// A row masked by a rewind marker carries a `[masked]` summary
/// prefix. The marker itself never carries it.
pub(crate) fn event_rows(
    rows: &[(u64, Value)],
    ranges: &[(usize, usize)],
    chat: bool,
) -> Vec<String> {
    rows.iter()
        .filter_map(|(seq, o)| {
            let ty = o.get("type").and_then(|v| v.as_str()).unwrap_or("?");
            let active = seq_in_ranges(*seq as usize, ranges);
            let is_message = matches!(ty, "user_message" | "assistant_message");
            if chat && (!active || !is_message) {
                return None;
            }
            let ts = o.get("ts").map(ts_local).unwrap_or_default();
            let mut summary = event_summary(o);
            if !chat && !active && ty != "rewind" {
                summary = format!("[masked] {summary}");
            }
            Some(format!("{seq}\t{ty}\t{ts}\t{summary}"))
        })
        .collect()
}
/// One TSV row per entry of the session's events.jsonl. The columns
/// are seq (the 1-based line number), type, ts (local wall-clock
/// time), and a one-line summary. The summary carries no tabs or
/// newlines, so the row stays a single TSV line. Nothing is clipped;
/// the list side truncates. The rows come newest first. With `chat`,
/// only the active-path user and assistant messages print. A row
/// masked by a rewind marker carries a `[masked]` summary prefix.
pub fn events(dir: Option<PathBuf>, chat: bool) {
    let base = match dir {
        Some(d) => d,
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let rows = numbered_events(&base);
    let ranges = log_active_ranges(&rows);
    for row in event_rows(&rows, &ranges, chat).iter().rev() {
        println!("{row}");
    }
}
/// The one-line summary of an event for the list column. It keeps the
/// whole payload; it only flattens tabs and newlines to spaces so the
/// TSV row cannot break.
fn event_summary(o: &Value) -> String {
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
        std::fs::write(d.join("events.jsonl"),
            "{\"type\":\"user_message\",\"content\":\"one\"}\nnot json\n{\"type\":\"cancel\"}\n")
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
            (6, serde_json::json!({"type": "rewind", "target_seq": 4,
                "mode": "on", "reason": "tui_pick"})),
            (7, v("user_message")),
        ];
        // The marker at seq 3 is malformed (no target_seq), so the
        // kernel skips it. The marker at seq 6 (target 4, on mode)
        // keeps 1..4 and 7..end. Seq 5 is the abandoned branch.
        let ranges = log_active_ranges(&rows);
        assert_eq!(ranges, vec![(1, 4), (7, 7)]);
    }

    #[test]
    fn event_rows_chat_keeps_active_messages_only() {
        let v = |t: &str| serde_json::json!({"type": t});
        let rows: Vec<(u64, Value)> = vec![
            (1, v("user_message")),
            (2, v("assistant_message")),
            (3, v("tool_call")),
            (4, serde_json::json!({"type": "rewind", "target_seq": 2,
                "mode": "on"})),
            (5, v("user_message")),
            (6, v("assistant_message")),
        ];
        let ranges = log_active_ranges(&rows);
        let chat: Vec<u64> = event_rows(&rows, &ranges, true)
            .into_iter()
            .map(|r| r.split('\t').next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(chat, vec![1, 2, 5, 6]);
        let all = event_rows(&rows, &ranges, false);
        assert_eq!(all.len(), 6);
        // The abandoned tool_call at seq 3 is tagged. The marker at
        // seq 4 is never tagged.
        assert!(all[2].contains("[masked]"), "{}", all[2]);
        assert!(!all[3].contains("[masked]"), "{}", all[3]);
        assert!(all[3].contains("rewind -> seq 2 (on)"), "{}", all[3]);
    }


}
