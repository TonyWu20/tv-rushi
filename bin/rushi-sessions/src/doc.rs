//! The `event-preview` command: render one events.jsonl entry as a
//! document (markdown for messages, toml for everything else).
use serde_json::Value;
use std::path::Path;

use crate::events::numbered_events;
use crate::format::{rfc3339_local, ts_local, value_inline};

/// The document language of an event: markdown for the message types,
/// toml for everything else.
fn doc_lang(o: &Value) -> &'static str {
    match o.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "user_message" | "assistant_message" | "compaction_summary" => "markdown",
        _ => "toml",
    }
}
/// The document for one event. Markdown entries open with a title and
/// a metadata block, then the content, then optional reasoning and
/// tool_calls sections. Every other event renders as a toml table
/// named by its type. The ts metadata is local wall-clock time. The
/// id of a user_message does not print. The call ids of a tool_call
/// (the id and call_id keys) do not print.
fn doc_for(o: &Value) -> String {
    if doc_lang(o) == "markdown" {
        markdown_doc(o)
    } else {
        toml_doc(o)
    }
}
fn markdown_doc(o: &Value) -> String {
    let ty = o.get("type").and_then(|v| v.as_str()).unwrap_or("event");
    let mut out = format!("# {ty}\n");
    // The id of a user message is its queue id, not useful in the
    // panel. The other types keep it.
    let keys: &[&str] = if ty == "user_message" {
        &["ts", "queue", "reason"]
    } else {
        &["ts", "id", "queue", "reason"]
    };
    for k in keys {
        if let Some(v) = o.get(k).filter(|v| !v.is_null()) {
            if *k == "ts" {
                out.push_str(&format!("ts: {}\n", ts_local(v)));
            } else {
                out.push_str(&format!("{k}: {}\n", value_inline(v)));
            }
        }
    }
    if let Some(n) = o.get("tokens_before").and_then(|v| v.as_u64()) {
        out.push_str(&format!("tokens_before: {n}\n"));
    }
    out.push('\n');
    if ty == "compaction_summary" {
        let body = o.get("summary").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(body);
        if !body.is_empty() && !body.ends_with('\n') {
            out.push('\n');
        }
        for fk in ["modified_files", "read_files"] {
            if let Some(arr) = o.get(fk) {
                out.push_str("\n## files\n\n```toml\n");
                if let Some(t) = json_to_toml(arr) {
                    let mut m = toml::map::Map::new();
                    m.insert(fk.to_string(), t);
                    out.push_str(&toml::to_string(&toml::Value::Table(m)).unwrap_or_default());
                }
                out.push_str("```\n");
            }
        }
    } else {
        let body = o.get("content").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(body);
        if !body.is_empty() && !body.ends_with('\n') {
            out.push('\n');
        }
    }
    if let Some(r) = o.get("reasoning").and_then(|v| v.as_str()) {
        if !r.is_empty() {
            out.push_str("\n## reasoning\n\n");
            out.push_str(r);
            if !r.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    if let Some(tc) = o.get("tool_calls").filter(|v| !v.is_null()) {
        out.push_str("\n## tool_calls\n\n```toml\n");
        if let Some(t) = json_to_toml(tc) {
            let mut m = toml::map::Map::new();
            m.insert("tool_calls".to_string(), t);
            out.push_str(&toml::to_string(&toml::Value::Table(m)).unwrap_or_default());
        }
        out.push_str("```\n");
    }
    out
}
fn toml_doc(o: &Value) -> String {
    let header = match o.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "tool_call" => "tool_call",
        "tool_result" => "tool_result",
        _ => "event",
    };
    // The ts of the table prints as local wall-clock time. The
    // conversion rewrites the JSON value before the toml conversion.
    let mut o2 = o.clone();
    if let Value::Object(m) = &mut o2 {
        if let Some(Value::String(t)) = m.get("ts").cloned() {
            if let Some(l) = rfc3339_local(&t) {
                m.insert("ts".to_string(), Value::String(l));
            }
        }
    }
    let mut table = toml::map::Map::new();
    if let Some(toml::Value::Table(inner)) = json_to_toml(&o2) {
        table = inner;
    }
    // The call ids of a tool_call are the id and call_id keys.
    // They map the call to its result; the panel shows the result as
    // its own entry.
    if header == "tool_call" {
        table.remove("id");
        table.remove("call_id");
    }
    let mut doc = toml::map::Map::new();
    doc.insert(header.to_string(), toml::Value::Table(table));
    toml::to_string(&toml::Value::Table(doc)).unwrap_or_default()
}
/// JSON to toml, with nulls dropped. toml has no null type, so a JSON
/// null anywhere makes the conversion fail; dropping them keeps the
/// rest. JSON literals are valid toml values, so the remaining values
/// convert directly.
fn json_to_toml(v: &Value) -> Option<toml::Value> {
    let cleaned = strip_nulls(v);
    serde_json::from_value(cleaned).ok()
}
fn strip_nulls(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut m2 = serde_json::Map::new();
            for (k, val) in m {
                if !val.is_null() {
                    m2.insert(k.clone(), strip_nulls(val));
                }
            }
            Value::Object(m2)
        }
        Value::Array(a) => Value::Array(
            a.iter().filter(|x| !x.is_null()).map(strip_nulls).collect(),
        ),
        other => other.clone(),
    }
}
pub fn event_preview(dir: &Path, seq: u64, print_lang: bool) {
    let found = numbered_events(dir)
        .into_iter()
        .find(|(n, _)| *n == seq)
        .map(|(_, v)| v);
    let o = match found {
        Some(v) => v,
        None => {
            eprintln!("rushi-sessions: no event at line {seq}");
            std::process::exit(1);
        }
    };
    if print_lang {
        println!("{}", doc_lang(&o));
        return;
    }
    print!("{}", doc_for(&o));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_doc_omits_its_id() {
        let o = serde_json::json!({"type": "user_message", "id": "u1",
            "ts": "2026-10-01T01:30:59Z", "content": "hi"});
        let doc = markdown_doc(&o);
        assert!(!doc.contains("\nid: "), "{doc}");
        assert!(doc.contains("ts: "), "{doc}");
        assert!(doc.contains("hi"), "{doc}");
    }


    #[test]
    fn assistant_message_doc_keeps_its_id() {
        let o = serde_json::json!({"type": "assistant_message", "id": "a1",
            "content": "yo"});
        let doc = markdown_doc(&o);
        assert!(doc.contains("\nid: a1"), "{doc}");
    }


    #[test]
    fn tool_call_doc_omits_its_call_ids() {
        let o = serde_json::json!({"type": "tool_call", "id": "call_1",
            "call_id": "call_1", "name": "bash",
            "arguments": {"command": "ls"}, "ts": "2026-10-01T01:31:09Z"});
        let doc = toml_doc(&o);
        assert!(!doc.contains("call_1"), "{doc}");
        assert!(!doc.contains("id"), "{doc}");
        assert!(doc.contains("name = \"bash\""), "{doc}");
    }


    #[test]
    fn tool_result_doc_keeps_its_id() {
        let o = serde_json::json!({"type": "tool_result", "id": "call_1",
            "is_error": false});
        let doc = toml_doc(&o);
        assert!(doc.contains("id = \"call_1\""), "{doc}");
    }


    #[test]
    fn doc_lang_per_type() {
        let v = |t: &str| Value::Object(serde_json::json!({ "type": t }).as_object().unwrap().clone());
        assert_eq!(doc_lang(&v("user_message")), "markdown");
        assert_eq!(doc_lang(&v("assistant_message")), "markdown");
        assert_eq!(doc_lang(&v("compaction_summary")), "markdown");
        assert_eq!(doc_lang(&v("tool_call")), "toml");
        assert_eq!(doc_lang(&v("tool_result")), "toml");
        assert_eq!(doc_lang(&v("ext_status")), "toml");
        assert_eq!(doc_lang(&Value::Object(serde_json::Map::new())), "toml");
    }


    #[test]
    fn json_to_toml_drops_nulls() {
        let v = serde_json::json!({"a": 1, "b": null, "c": {"d": null, "e": [1, null, 2]}});
        let t = json_to_toml(&v).expect("convert");
        let text = toml::to_string(&t).unwrap();
        assert!(text.contains("a = 1"), "{text}");
        assert!(!text.contains("b"), "{text}");
        assert!(text.contains("e = [1, 2]"), "{text}");
        let back: serde_json::Value = toml::from_str(&text).unwrap();
        assert_eq!(back, serde_json::json!({"a": 1, "c": {"e": [1, 2]}}));
    }


    #[test]
    fn markdown_doc_sections() {
        let o = serde_json::json!({"type": "assistant_message", "ts": "T", "id": "I",
            "content": "body", "reasoning": "think",
            "tool_calls": [{"id": "c1", "name": "bash", "arguments": {"command": "ls"}}]});
        let d = doc_for(&o);
        assert!(d.starts_with("# assistant_message\n"));
        assert!(d.contains("ts: T"));
        assert!(d.contains("id: I"));
        assert!(d.contains("\n## reasoning\n\nthink\n"));
        assert!(d.contains("## tool_calls\n\n```toml\n"));
        assert!(d.contains("name = \"bash\""));
    }


    #[test]
    fn toml_doc_tool_result_table() {
        let o = serde_json::json!({"type": "tool_result", "id": "c1", "ts": "T",
            "is_error": false, "bytes": 3, "value": {"text": "ok"}});
        let d = doc_for(&o);
        assert!(d.starts_with("[tool_result]\n"), "{d}");
        assert!(d.contains("is_error = false"));
        assert!(d.contains("text = \"ok\""));
    }


    #[test]
    fn toml_doc_unknown_type_event_table() {
        let o = serde_json::json!({"type": "cancel", "ts": "T"});
        let d = doc_for(&o);
        assert!(d.starts_with("[event]\n"), "{d}");
        assert!(d.contains("type = \"cancel\""));
    }


    #[test]
    fn event_preview_print_lang() {
        let d = std::env::temp_dir().join(format!("rushi-evtest2-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("events.jsonl"),
            "{\"type\":\"tool_result\"}\n{\"type\":\"user_message\",\"content\":\"x\"}\n")
            .unwrap();
        let rows = numbered_events(&d);
        assert_eq!(rows.len(), 2);
        assert_eq!(doc_lang(&rows[0].1), "toml");
        assert_eq!(doc_lang(&rows[1].1), "markdown");
        let _ = std::fs::remove_dir_all(&d);
    }


}
