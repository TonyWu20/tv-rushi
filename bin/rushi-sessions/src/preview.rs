//! The `preview` command: render the TOML preview card for one session.
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::events::read_events;
use crate::format::{brief, clip, json_str, ts_slice, ts_to_str, value_to_str, which};
use crate::remote::{preview_remote, split_remote};
use crate::state::alive;
use crate::usage::{context_window, last_usage, usage_line};

pub fn preview(
    status: &str,
    name: &str,
    repo: &str,
    phase: &str,
    last: &str,
    mtime: &str,
    path: &str,
) {
    if let Some((host, rpath)) = split_remote(path) {
        preview_remote(host, &[status, name, repo, phase, last, mtime, rpath]);
        return;
    }
    let pid = fs::read_to_string(Path::new(path).join("loop.pid"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // Refresh the status from the live pid when one is present.
    let status = if !pid.is_empty() {
        if alive(&pid) {
            "ACTIVE"
        } else {
            "IDLE"
        }
    } else {
        status
    };

    let epath = Path::new(path).join("events.jsonl");
    let events = if epath.exists() {
        read_events(&epath)
    } else {
        Vec::new()
    };

    let mut think = String::new();
    let mut last_user = String::new();
    let mut last_assistant = String::new();
    for o in &events {
        let t = o.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t == "ext_status" && o.get("id").and_then(|v| v.as_str()) == Some("model_thinking") {
            think = value_to_str(o.get("value"));
        }
        if t == "user_message" {
            last_user = o
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        if t == "assistant_message" {
            last_assistant = o
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
    }

    let mut out: Vec<String> = Vec::new();
    out.push("[rushi-session]".into());
    out.push(format!("name    = {}", json_str(name)));
    out.push(format!("repo    = {}", json_str(repo)));
    out.push(format!("status  = {}", json_str(status)));
    out.push(format!("pid     = {}", json_str(&pid)));
    out.push(format!("phase   = {}", json_str(phase)));
    if !think.is_empty() {
        out.push(format!("think   = {}", json_str(&think)));
    }
    out.push(format!("updated = {}", json_str(last)));
    if let Some((in_tok, out_tok, _cached)) = last_usage(&events) {
        let used = in_tok.saturating_add(out_tok);
        out.push(format!(
            "usage   = {}",
            json_str(&usage_line(used, context_window()))
        ));
    }
    out.push(String::new());
    out.push("[last-user-message]".into());
    out.push(format!("content = {}", json_str(&clip(&last_user, 400))));
    out.push(String::new());
    out.push("[last-assistant-message]".into());
    out.push(format!(
        "content = {}",
        json_str(&clip(&last_assistant, 400))
    ));

    // The last 40 events, reduced to (ts, type, brief), then the last 5 in
    // log order (the old `events[-5:]`), so ts ties keep log order.
    let mut last40: Vec<&Value> = events.iter().rev().take(40).collect();
    last40.reverse();
    let tuples: Vec<(String, String, String)> = last40
        .iter()
        .filter_map(|o| {
            let brief = brief(o)?;
            let ts_raw = o.get("ts").map(ts_to_str).unwrap_or_default();
            let ts = ts_slice(&ts_raw);
            let ty = o
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            Some((ts, ty, brief))
        })
        .collect();
    let recent: Vec<&(String, String, String)> =
        tuples.iter().skip(tuples.len().saturating_sub(5)).collect();
    for (ts, ty, brief) in recent {
        out.push(String::new());
        out.push("[[recent-event]]".into());
        out.push(format!("ts    = {}", json_str(ts)));
        out.push(format!("type  = {}", json_str(ty)));
        out.push(format!("brief = {}", json_str(&clip(brief, 80))));
    }

    let card = out.join("\n") + "\n";

    // Optional colorization through `bat` when it is available.
    match which("bat") {
        Some(bat) => {
            let theme = std::env::var("RUSHI_PREVIEW_THEME")
                .unwrap_or_else(|_| "Catppuccin Macchiato".to_string());
            let mut child = std::process::Command::new(&bat)
                .args([
                    "-l", "toml", "--theme", &theme, "--color", "always", "--style", "plain",
                ])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok();
            if let Some(mut c) = child.take() {
                use std::io::Write;
                if let Some(mut stdin) = c.stdin.take() {
                    let _ = stdin.write_all(card.as_bytes());
                }
                match c.wait_with_output() {
                    Ok(o) if o.status.success() && !o.stdout.is_empty() => {
                        print!("{}", String::from_utf8_lossy(&o.stdout));
                        return;
                    }
                    _ => {}
                }
            }
            print!("{}", card);
        }
        None => print!("{}", card),
    }
}
