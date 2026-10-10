//! Display primitives: text clipping, JSON quoting, timestamp
//! formatting, event summaries, and PATH lookup.
use chrono::Local;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// ext_status ids that always show in the preview's recent-event list.
const KEEP: &[&str] = &[
    "loop_phase",
    "model_thinking",
    "goal_paused",
    "goal_active",
    "run.refire",
];
/// Tabs and newlines to single spaces.
pub(crate) fn flatten_ws(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' | '\n' | '\r' => ' ',
            c => c,
        })
        .collect()
}
/// A JSON value for display in a document: strings as is, everything
/// else as compact JSON.
pub(crate) fn value_inline(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}
/// `str(o.get("value",""))` — the value of an ext_status as a string.
pub(crate) fn value_to_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "None".into(),
        None => String::new(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}
/// The one-line summary of the two control-record types: the `rewind`
/// marker and the `user_message_retract` marker. The kernel appends
/// both to the log; they carry no `content` key, so the default
/// content-based summary would print their raw JSON.
pub(crate) fn marker_summary(o: &Value) -> String {
    match o.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "rewind" => {
            let t = o.get("target_seq").and_then(|v| v.as_u64()).unwrap_or(0);
            let m = o.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
            let r = o.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            if r.is_empty() {
                format!("rewind -> seq {t} ({m})")
            } else {
                format!("rewind -> seq {t} ({m}, {r})")
            }
        }
        "user_message_retract" => {
            let target = o.get("target").and_then(|v| v.as_str()).unwrap_or("?");
            let r = o.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            if r.is_empty() {
                format!("retract {target}")
            } else {
                format!("retract {target} ({r})")
            }
        }
        _ => String::new(),
    }
}
/// A one-line summary of an event, or None to skip it. Mirrors the
/// channel's `brief()` for tool_call, tool_result and ext_status.
pub(crate) fn brief(o: &Value) -> Option<String> {
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
        "rewind" | "user_message_retract" => Some(marker_summary(o)),
        _ => {
            let c = o.get("content").and_then(|v| v.as_str()).unwrap_or("");
            Some(c.replace('\n', " "))
        }
    }
}
/// `s[:n]` character clip with a trailing " ..." when truncated.
pub(crate) fn clip(s: &str, n: usize) -> String {
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
/// RFC3339 to the local wall-clock time. `None` when the string is
/// not RFC3339. The local time is the running host's time zone.
pub(crate) fn rfc3339_local(s: &str) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    Some(dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
}
/// The event ts as a local wall-clock time. A value that is not
/// RFC3339 prints as is.
pub(crate) fn ts_local(v: &Value) -> String {
    match v {
        Value::String(s) => rfc3339_local(s).unwrap_or_else(|| s.clone()),
        _ => v.to_string(),
    }
}
/// A JSON string (the `q()` helper): double-quoted, ASCII-escaped the way
/// Python's `json.dumps` does, keeping non-ASCII as is.
pub(crate) fn json_str(s: &str) -> String {
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
pub(crate) fn which(name: &str) -> Option<PathBuf> {
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
    fn rfc3339_local_rejects_garbage() {
        assert!(rfc3339_local("garbage").is_none());
        assert!(rfc3339_local("").is_none());
    }


    #[test]
    fn rfc3339_local_formats_wall_clock() {
        // The date part holds for every offset from UTC-11 to UTC+12
        // for this timestamp; the Z suffix does not survive.
        let out = rfc3339_local("2026-10-01T01:30:59Z").unwrap();
        assert_eq!(out.len(), 19, "{out}");
        assert!(!out.contains('Z'), "{out}");
        assert!(out.starts_with("2026-10-"), "{out}");
    }


    #[test]
    fn ts_local_passes_through_unparseable() {
        assert_eq!(ts_local(&Value::String("garbage".to_string())), "garbage");
        assert_eq!(
            ts_local(&Value::String("2026-10-01T01:30:59Z".to_string())).len(),
            19
        );
    }


    #[test]
    fn flatten_ws_stays_one_line() {
        assert_eq!(flatten_ws("a\tb\nc\rd"), "a b c d");
        assert_eq!(flatten_ws("x"), "x");
    }

    #[test]
    fn brief_rewind_and_retract_markers() {
        let r = serde_json::json!({"type": "rewind", "target_seq": 2,
            "mode": "on", "reason": "tui_pick"});
        assert_eq!(brief(&r), Some("rewind -> seq 2 (on, tui_pick)".into()));
        let x = serde_json::json!({"type": "user_message_retract", "target": "abc"});
        assert_eq!(brief(&x), Some("retract abc".into()));
    }


}
