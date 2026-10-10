//! The `preview` command: render the TOML preview card for one session.
//! The card scans keep only the active path: the kernel's rewind
//! markers mask the abandoned branch, so the card shows what the
//! model context sees.
use serde_json::Value;
use std::fs;
use std::path::Path;

use rushi_common::rewind::seq_in_ranges;

use crate::events::{log_active_ranges, numbered_events};
use crate::format::{brief, clip, json_str, ts_local, value_to_str, which};
use crate::remote::{preview_remote, split_remote};
use crate::state::alive;
use crate::usage::{context_window, last_usage, usage_line};

/// The env var that carries the ssh host to the card render. The
/// local binary sets it inline in the ssh command (`preview_remote`);
/// the remote card render reads it and puts a `host` key right under
/// the `repo` key. A local render without the variable shows no
/// host line.
const HOST_ENV: &str = "RUSHI_SESSIONS_PREVIEW_HOST";

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
        // The TSV repo column carries the `host:` prefix. The remote
        // card renders the bare repo name plus its own `host` key, so
        // the prefix is stripped before the ssh call.
        let bare = repo
            .strip_prefix(host)
            .and_then(|rest| rest.strip_prefix(':'))
            .unwrap_or(repo);
        preview_remote(host, &[status, name, bare, phase, last, mtime, rpath]);
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

    // The numbered rows keep the 1-based seqs the kernel's rewind
    // markers point at. The active-path filter drops the abandoned
    // branch, so every card scan shows what the model context sees.
    let rows = numbered_events(Path::new(path));
    let ranges = log_active_ranges(&rows);
    let events: Vec<Value> = rows
        .into_iter()
        .filter(|(s, _)| seq_in_ranges(*s as usize, &ranges))
        .map(|(_, v)| v)
        .collect();

    let (think, last_user, last_assistant) = card_scans(&events);

    // The remote binary renders this same card with the host key,
    // fed by the env var the local binary set over ssh.
    let host = std::env::var(HOST_ENV).ok().filter(|h| !h.is_empty());
    let card = render_card(
        status,
        name,
        repo,
        host.as_deref(),
        &pid,
        phase,
        &think,
        last,
        &last_user,
        &last_assistant,
        &events,
    );

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

/// The card's three text scans over the active-path events: the live
/// `model_thinking` status, the last user message, and the last
/// assistant message.
pub(crate) fn card_scans(events: &[Value]) -> (String, String, String) {
    let mut think = String::new();
    let mut last_user = String::new();
    let mut last_assistant = String::new();
    for o in events {
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
    (think, last_user, last_assistant)
}
/// The TOML card text for one session. `host` (the ssh host of a
/// remote session, set by the local binary through `HOST_ENV`)
/// renders a `host` key right under `repo`.
pub(crate) fn render_card(
    status: &str,
    name: &str,
    repo: &str,
    host: Option<&str>,
    pid: &str,
    phase: &str,
    think: &str,
    last: &str,
    last_user: &str,
    last_assistant: &str,
    events: &[Value],
) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push("[rushi-session]".into());
    out.push(format!("name    = {}", json_str(name)));
    out.push(format!("repo    = {}", json_str(repo)));
    if let Some(h) = host {
        out.push(format!("host    = {}", json_str(h)));
    }
    out.push(format!("status  = {}", json_str(status)));
    out.push(format!("pid     = {}", json_str(pid)));
    out.push(format!("phase   = {}", json_str(phase)));
    if !think.is_empty() {
        out.push(format!("think   = {}", json_str(think)));
    }
    out.push(format!("updated = {}", json_str(last)));
    if let Some((in_tok, out_tok, _cached)) = last_usage(events) {
        let used = in_tok.saturating_add(out_tok);
        out.push(format!(
            "usage   = {}",
            json_str(&usage_line(used, context_window()))
        ));
    }
    out.push(String::new());
    out.push("[last-user-message]".into());
    out.push(format!("content = {}", json_str(&clip(last_user, 400))));
    out.push(String::new());
    out.push("[last-assistant-message]".into());
    out.push(format!(
        "content = {}",
        json_str(&clip(last_assistant, 400))
    ));

    // The last 40 events, reduced to (ts, type, brief), then the last 5 in
    // log order (the old `events[-5:]`), so ts ties keep log order.
    let mut last40: Vec<&Value> = events.iter().rev().take(40).collect();
    last40.reverse();
    let tuples: Vec<(String, String, String)> = last40
        .iter()
        .filter_map(|o| {
            let brief = brief(o)?;
            // Local wall-clock ts, the same ts_local the events rows
            // use. A ts that is not RFC3339 prints as is.
            let ts = o.get("ts").map(ts_local).unwrap_or_default();
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

    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_EVENTS: &[Value] = &[];

    #[test]
    fn card_hosts_line_sits_under_repo() {
        let card = render_card(
            "ACTIVE", "foo", "repoR", Some("host"), "123", "tools", "", "10-02 14:11", "u", "a",
            EMPTY_EVENTS,
        );
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| l.starts_with("repo    = "))
            .expect("repo line");
        assert_eq!(lines[i + 1], "host    = \"host\"");
    }


    #[test]
    fn card_without_host_has_no_host_line() {
        let card = render_card(
            "IDLE", "foo", "repoR", None, "123", "tools", "", "10-02 14:11", "u", "a",
            EMPTY_EVENTS,
        );
        assert!(!card.lines().any(|l| l.starts_with("host    = ")));
        // The repo line is followed straight by the status line.
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| l.starts_with("repo    = "))
            .expect("repo line");
        assert!(lines[i + 1].starts_with("status  = "));
    }

    #[test]
    fn card_scans_ignore_abandoned_branch() {
        let d = std::env::temp_dir().join(format!("rushi-cardscan-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("events.jsonl"), concat!(
            "{\"type\":\"user_message\",\"content\":\"first user msg\"}\n",
            "{\"type\":\"assistant_message\",\"content\":\"branch A answer\"}\n",
            "{\"type\":\"rewind\",\"target_seq\":1,\"mode\":\"on\",\"reason\":\"tui_pick\"}\n",
            "{\"type\":\"user_message\",\"content\":\"re-asked question\"}\n",
        )).unwrap();
        let rows = numbered_events(&d);
        let ranges = log_active_ranges(&rows);
        let active: Vec<Value> = rows
            .into_iter()
            .filter(|(s, _)| seq_in_ranges(*s as usize, &ranges))
            .map(|(_, v)| v)
            .collect();
        let (think, last_user, last_assistant) = card_scans(&active);
        assert_eq!(think, "");
        assert_eq!(last_user, "re-asked question");
        // The abandoned branch A answer is masked. No active assistant
        // message exists yet after the marker.
        assert_eq!(last_assistant, "");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn card_recent_event_ts_is_local_wall_clock() {
        let evts: Vec<Value> = vec![serde_json::json!({
            "type": "user_message",
            "ts": "2026-10-01T01:30:59Z",
            "content": "hi"
        })];
        let card = render_card(
            "IDLE", "foo", "repoR", None, "", "tools", "", "10-02 14:11", "u", "",
            &evts,
        );
        let ts = card
            .lines()
            .find(|l| l.starts_with("ts    = "))
            .expect("a recent-event ts line");
        let v = ts.trim_start_matches("ts    = ");
        // The json_str quotes wrap a 19-char wall clock. The shape
        // check holds for every running-host offset; a UTC
        // pass-through would carry the Z suffix and a date part.
        assert_eq!(v.len(), 21, "{v}");
        let inner = &v[1..20];
        let shape = inner.chars().enumerate().all(|(i, c)| match i {
            4 | 7 => c == '-',
            10 => c == ' ',
            13 | 16 => c == ':',
            _ => c.is_ascii_digit(),
        });
        assert!(shape, "{v}");
        assert!(!inner.contains('Z'), "{v}");
    }

}
