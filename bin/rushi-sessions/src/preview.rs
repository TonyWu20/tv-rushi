//! The `preview` command: render the markdown preview card for one
//! session. The card scans keep only the active path: the kernel's
//! rewind markers mask the abandoned branch, so the card shows what
//! the model context sees. The session key table carries a `branch`
//! row: the git branch of the session's recorded working dir, probed
//! with git at render time. It carries a `worktree` row: the one-line
//! `git status` brief of that worktree (`clean`, or the `<N>
//! <status>` file counts), probed the same way. The card shows the
//! full last user message and the last on-idle assistant message
//! (the last assistant message with text, the in-flight turn excluded
//! while the loop is live). It shows a `recent-events` table: the
//! last 5 meaningful events.
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use rushi_common::rewind::seq_in_ranges;

use crate::events::{log_active_ranges, numbered_events};
use crate::format::{brief, clip, md_cell, ts_local, value_to_str, which};
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

    let (think, last_user) = card_scans(&events);
    let last_assistant = last_idle_assistant(&events, status);

    // The remote binary renders this same card with the host key,
    // fed by the env var the local binary set over ssh.
    let host = std::env::var(HOST_ENV).ok().filter(|h| !h.is_empty());
    // The session's recorded working dir. The git branch probe runs
    // on it; the `rushi config` probe of the usage line runs in its
    // project dir when the project flake's devShell carries the
    // build (decision 88). The remote binary runs both probes on
    // the remote host, over the remote working dir.
    let workdir = session_workdir(Path::new(path));
    let branch = workdir.as_ref().and_then(|w| git_branch(w));
    let worktree = workdir.as_ref().and_then(|w| git_status_brief(w));
    let card = render_card(
        status,
        name,
        repo,
        host.as_deref(),
        branch.as_deref(),
        worktree.as_deref(),
        &pid,
        phase,
        &think,
        last,
        &last_user,
        &last_assistant,
        &events,
        workdir.as_deref(),
    );

    // Optional colorization through `bat` when it is available.
    match which("bat") {
        Some(bat) => {
            let theme = std::env::var("RUSHI_PREVIEW_THEME")
                .unwrap_or_else(|_| "Catppuccin Macchiato".to_string());
            let mut child = std::process::Command::new(&bat)
                .args([
                    "-l", "markdown", "--theme", &theme, "--color", "always", "--style", "plain",
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

/// The card's two text scans over the active-path events: the live
/// `model_thinking` status and the last user message.
pub(crate) fn card_scans(events: &[Value]) -> (String, String) {
    let mut think = String::new();
    let mut last_user = String::new();
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
    }
    (think, last_user)
}

/// The last on-idle assistant message: the last assistant message
/// with non-empty text content, on the active path. While the loop
/// is live (`ACTIVE`), the in-flight turn opens at the last user
/// message, so the scan stops before it. An idle loop scans the
/// whole active path. Empty when no such message exists.
pub(crate) fn last_idle_assistant(events: &[Value], status: &str) -> String {
    let end = if status == "ACTIVE" {
        events
            .iter()
            .rposition(|o| {
                o.get("type").and_then(|v| v.as_str()) == Some("user_message")
            })
            .unwrap_or(0)
    } else {
        events.len()
    };
    events[..end]
        .iter()
        .rev()
        .find(|o| {
            o.get("type").and_then(|v| v.as_str()) == Some("assistant_message")
                && !o.get("content").and_then(|v| v.as_str()).unwrap_or("").is_empty()
        })
        .map(|o| o.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string())
        .unwrap_or_default()
}

/// The session's recorded working dir (the `cwd` file), or the
/// project dir (dirname(dirname(session dir))) when the file is
/// missing or stale. The open actions' rule without the
/// sessions-ancestor walk: git finds the repo root from the
/// recorded dir itself.
fn session_workdir(path: &Path) -> Option<PathBuf> {
    let recorded = fs::read_to_string(Path::new(path).join("cwd"))
        .ok()
        .map(|s| s.trim().to_string());
    if let Some(w) = &recorded {
        if !w.is_empty() && Path::new(w).is_dir() {
            return Some(PathBuf::from(w.as_str()));
        }
    }
    Path::new(path)
        .parent()
        .and_then(|d| d.parent())
        .map(PathBuf::from)
}

/// The git branch of a working dir: the checked-out branch name, or
/// `detached@<short sha>` when the HEAD is detached. None when git
/// is missing, the dir is not in a repo, or the repo has no HEAD
/// (an unborn branch).
fn git_branch(dir: &Path) -> Option<String> {
    let arg = dir.to_string_lossy();
    let out = std::process::Command::new("git")
        .args(["-C", arg.as_ref(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name == "HEAD" {
        // Detached: the short commit identifies the position.
        let o = std::process::Command::new("git")
            .args(["-C", arg.as_ref(), "rev-parse", "--short", "HEAD"])
            .output()
            .ok()?;
        if !o.status.success() {
            return None;
        }
        let sha = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if sha.is_empty() {
            return None;
        }
        return Some(format!("detached@{sha}"));
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// The one-line `git status` brief of a working dir's worktree.
/// `clean` when the porcelain output is empty. Else one
/// `<N> <status>` group per non-zero bucket, comma-joined in the
/// fixed order of the `BUCKETS` table: the index and worktree
/// changes, and the untracked files. A file with an index change
/// and a worktree change counts in both buckets. None when git
/// is missing, the dir is not in a repo, or the probe fails.
fn git_status_brief(dir: &Path) -> Option<String> {
    let arg = dir.to_string_lossy();
    let out = std::process::Command::new("git")
        .args(["-C", arg.as_ref(), "status", "--porcelain"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    // The fixed bucket order of the comma-joined line.
    const BUCKETS: [&str; 7] = [
        "added",
        "modified",
        "deleted",
        "renamed",
        "copied",
        "conflict",
        "untracked",
    ];
    let mut counts = [0usize; 7];
    for line in stdout.lines() {
        let x = *line.as_bytes().first().unwrap_or(&b' ');
        let y = line.as_bytes().get(1).copied().unwrap_or(b' ');
        if x == b'?' && y == b'?' {
            counts[6] += 1;
            continue;
        }
        let unmerged = x == b'U' || y == b'U' || (x == y && (x == b'A' || x == b'D'));
        if unmerged {
            counts[5] += 1;
            continue;
        }
        if x == b'A' || y == b'A' {
            counts[0] += 1;
        }
        if x == b'M' || y == b'M' || x == b'T' || y == b'T' {
            counts[1] += 1;
        }
        if x == b'D' || y == b'D' {
            counts[2] += 1;
        }
        if x == b'R' || y == b'R' {
            counts[3] += 1;
        }
        if x == b'C' || y == b'C' {
            counts[4] += 1;
        }
    }
    let groups: Vec<String> = (0..BUCKETS.len())
        .filter(|&i| counts[i] > 0)
        .map(|i| format!("{} {}", counts[i], BUCKETS[i]))
        .collect();
    if groups.is_empty() {
        Some("clean".to_string())
    } else {
        Some(groups.join(", "))
    }
}

/// The markdown card text for one session. `host` (the ssh host of a
/// remote session, set by the local binary through `HOST_ENV`)
/// renders a `host` row right under `repo`. `branch` (the git
/// branch of the session's recorded working dir, or None) renders a
/// `branch` row under the host row. `worktree` (the one-line git
/// status brief of that worktree, or None) renders a `worktree` row
/// under the branch row, or under the host slot. `workdir` (the
/// recorded working dir, or None) is the project dir of the usage
/// line's `rushi config` probe (decision 88).
///
/// The card is a markdown document: the `# rushi-session` key table,
/// the `last-user-message` section, the `last-assistant-message`
/// section (the last on-idle message, un-clipped), and the
/// `recent-events` table. An empty section or table omits itself.
pub(crate) fn render_card(
    status: &str,
    name: &str,
    repo: &str,
    host: Option<&str>,
    branch: Option<&str>,
    worktree: Option<&str>,
    pid: &str,
    phase: &str,
    think: &str,
    last: &str,
    last_user: &str,
    last_assistant: &str,
    events: &[Value],
    workdir: Option<&Path>,
) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push("# rushi-session".into());
    out.push(String::new());
    // The two-column key table. The rows keep the old key order.
    let mut keys: Vec<(&str, String)> = Vec::new();
    keys.push(("name", md_cell(name)));
    keys.push(("repo", md_cell(repo)));
    if let Some(h) = host {
        keys.push(("host", md_cell(h)));
    }
    if let Some(b) = branch {
        keys.push(("branch", md_cell(b)));
    }
    if let Some(wt) = worktree {
        keys.push(("worktree", md_cell(wt)));
    }
    keys.push(("status", md_cell(status)));
    keys.push(("pid", md_cell(pid)));
    keys.push(("phase", md_cell(phase)));
    if !think.is_empty() {
        keys.push(("think", md_cell(think)));
    }
    keys.push(("updated", md_cell(last)));
    if let Some((in_tok, out_tok, _cached)) = last_usage(events) {
        let used = in_tok.saturating_add(out_tok);
        keys.push((
            "usage",
            md_cell(&usage_line(used, context_window(workdir))),
        ));
    }
    out.push("| key | value |".into());
    out.push("| --- | --- |".into());
    for (k, v) in &keys {
        out.push(format!("| {} | {} |", k, v));
    }

    if !last_user.is_empty() {
        out.push(String::new());
        out.push("## last-user-message".into());
        out.push(String::new());
        out.push(last_user.trim_end().to_string());
    }
    if !last_assistant.is_empty() {
        out.push(String::new());
        out.push("## last-assistant-message".into());
        out.push(String::new());
        out.push(last_assistant.trim_end().to_string());
    }

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
    if !recent.is_empty() {
        out.push(String::new());
        out.push("## recent-events".into());
        out.push(String::new());
        out.push("| ts | type | brief |".into());
        out.push("| --- | --- | --- |".into());
        for (ts, ty, brief) in recent {
            out.push(format!(
                "| {} | {} | {} |",
                md_cell(ts),
                md_cell(ty),
                md_cell(&clip(brief, 80))
            ));
        }
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
            "ACTIVE",
            "foo",
            "repoR",
            Some("host"),
            None,
            None,
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| *l == "| repo | repoR |")
            .expect("repo row");
        assert_eq!(lines[i + 1], "| host | host |");
    }

    #[test]
    fn card_without_host_has_no_host_line() {
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        assert!(!card.lines().any(|l| l.starts_with("| host |")));
        // The repo row is followed straight by the status row.
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| *l == "| repo | repoR |")
            .expect("repo row");
        assert_eq!(lines[i + 1], "| status | IDLE |");
    }

    #[test]
    fn card_scans_ignore_abandoned_branch() {
        let d = std::env::temp_dir().join(format!("rushi-cardscan-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("events.jsonl"),
            concat!(
                "{\"type\":\"user_message\",\"content\":\"first user msg\"}\n",
                "{\"type\":\"assistant_message\",\"content\":\"branch A answer\"}\n",
                "{\"type\":\"rewind\",\"target_seq\":1,\"mode\":\"on\",\"reason\":\"tui_pick\"}\n",
                "{\"type\":\"user_message\",\"content\":\"re-asked question\"}\n",
            ),
        )
        .unwrap();
        let rows = numbered_events(&d);
        let ranges = log_active_ranges(&rows);
        let active: Vec<Value> = rows
            .into_iter()
            .filter(|(s, _)| seq_in_ranges(*s as usize, &ranges))
            .map(|(_, v)| v)
            .collect();
        let (think, last_user) = card_scans(&active);
        assert_eq!(think, "");
        assert_eq!(last_user, "re-asked question");
        // The abandoned branch A answer is masked. No active assistant
        // message with text exists after the marker.
        assert_eq!(last_idle_assistant(&active, "IDLE"), "");
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
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "",
            &evts,
            None,
        );
        let lines: Vec<&str> = card.lines().collect();
        let h = lines
            .iter()
            .position(|l| *l == "| ts | type | brief |")
            .expect("recent-events header");
        // The separator row is h+1, the first data row h+2. The
        // first cell is the ts.
        let line = lines[h + 2].to_string();
        let v = line.trim_start_matches('|').split('|').next().unwrap().trim();
        // The 19-char wall clock. The shape check holds for every
        // running-host offset; a UTC pass-through would carry the Z
        // suffix and a date part.
        assert_eq!(v.len(), 19, "{v}");
        let shape = v.chars().enumerate().all(|(i, c)| match i {
            4 | 7 => c == '-',
            10 => c == ' ',
            13 | 16 => c == ':',
            _ => c.is_ascii_digit(),
        });
        assert!(shape, "{v}");
        assert!(!v.contains('Z'), "{v}");
    }

    #[test]
    fn card_messages_print_full_text_unclipped() {
        let long = "w".repeat(5000);
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "",
            "tools",
            "",
            "10-02 14:11",
            "line one\nline two",
            &long,
            EMPTY_EVENTS,
            None,
        );
        // The user section keeps the newline. The 400-char clip is
        // gone: the 5000-char assistant text prints in full.
        assert!(card.contains("## last-user-message\n\nline one\nline two"), "{card}");
        assert!(card.contains(&long));
        assert!(!card.contains(" ..."));
    }

    #[test]
    fn card_omits_empty_message_sections() {
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "",
            "tools",
            "",
            "10-02 14:11",
            "",
            "",
            EMPTY_EVENTS,
            None,
        );
        assert!(!card.contains("## last-user-message"));
        assert!(!card.contains("## last-assistant-message"));
        // No events, no recent-events table.
        assert!(!card.contains("## recent-events"));
    }

    #[test]
    fn card_recent_events_table_shape() {
        let evts: Vec<Value> = vec![
            serde_json::json!({"type": "ext_status", "id": "loop_phase", "value": "wait"}),
            serde_json::json!({"type": "user_message", "content": "hi"}),
        ];
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "",
            "tools",
            "",
            "10-02 14:11",
            "",
            "",
            &evts,
            None,
        );
        let lines: Vec<&str> = card.lines().collect();
        let h = lines
            .iter()
            .position(|l| *l == "| ts | type | brief |")
            .expect("recent-events header");
        assert_eq!(lines[h + 1], "| --- | --- | --- |");
        // Two brief-bearing events: two data rows, in log order. The
        // events carry no ts, so the ts cell is empty.
        assert_eq!(lines[h + 2], "|  | ext_status | loop_phase=wait |");
        assert_eq!(lines[h + 3], "|  | user_message | hi |");
    }

    #[test]
    fn card_recent_events_cells_escape_pipes() {
        let evts: Vec<Value> = vec![serde_json::json!({
            "type": "tool_call",
            "name": "bash",
            "arguments": {"command": "echo a|b"}
        })];
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "",
            "tools",
            "",
            "10-02 14:11",
            "",
            "",
            &evts,
            None,
        );
        assert!(card.contains(r"| bash echo a\|b |"), "{card}");
    }

    #[test]
    fn last_idle_assistant_idle_takes_last_with_text() {
        let evts: Vec<Value> = vec![
            serde_json::json!({"type": "user_message", "content": "one"}),
            serde_json::json!({"type": "assistant_message", "content": "first answer"}),
            // The tool-step message has no text. It never wins.
            serde_json::json!({"type": "assistant_message", "content": ""}),
        ];
        assert_eq!(last_idle_assistant(&evts, "IDLE"), "first answer");
    }

    #[test]
    fn last_idle_assistant_active_stops_before_last_user() {
        let evts: Vec<Value> = vec![
            serde_json::json!({"type": "user_message", "content": "one"}),
            serde_json::json!({"type": "assistant_message", "content": "prev answer"}),
            serde_json::json!({"type": "user_message", "content": "two"}),
            serde_json::json!({"type": "assistant_message", "content": "in-flight"}),
        ];
        // The live loop: the in-flight turn is open at the last user
        // message. The on-idle message is the previous turn's answer.
        assert_eq!(last_idle_assistant(&evts, "ACTIVE"), "prev answer");
        // The idle loop: the last text-carrying answer of the path.
        assert_eq!(last_idle_assistant(&evts, "IDLE"), "in-flight");
    }

    #[test]
    fn last_idle_assistant_empty_when_no_text() {
        let evts: Vec<Value> = vec![
            serde_json::json!({"type": "user_message", "content": "one"}),
            serde_json::json!({"type": "assistant_message", "content": ""}),
        ];
        assert_eq!(last_idle_assistant(&evts, "IDLE"), "");
        assert_eq!(last_idle_assistant(&[], "IDLE"), "");
    }

    #[test]
    fn card_branch_line_sits_under_host() {
        let card = render_card(
            "ACTIVE",
            "foo",
            "repoR",
            Some("host"),
            Some("main"),
            Some("clean"),
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| *l == "| host | host |")
            .expect("host row");
        assert_eq!(lines[i + 1], "| branch | main |");
        assert_eq!(lines[i + 2], "| worktree | clean |");
    }

    #[test]
    fn card_worktree_line_sits_under_repo_when_no_branch() {
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            Some("clean"),
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| *l == "| repo | repoR |")
            .expect("repo row");
        assert_eq!(lines[i + 1], "| worktree | clean |");
        // The worktree row is followed straight by the status row.
        let j = lines
            .iter()
            .position(|l| *l == "| worktree | clean |")
            .expect("worktree row");
        assert_eq!(lines[j + 1], "| status | IDLE |");
    }

    #[test]
    fn card_without_worktree_has_no_worktree_line() {
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            Some("dev"),
            None,
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        assert!(!card.lines().any(|l| l.starts_with("| worktree |")));
    }

    #[test]
    fn card_branch_line_sits_under_repo_when_no_host() {
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            Some("dev"),
            None,
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        let lines: Vec<&str> = card.lines().collect();
        let i = lines
            .iter()
            .position(|l| *l == "| repo | repoR |")
            .expect("repo row");
        assert_eq!(lines[i + 1], "| branch | dev |");
        // The branch row is followed straight by the status row.
        let j = lines
            .iter()
            .position(|l| *l == "| branch | dev |")
            .expect("branch row");
        assert_eq!(lines[j + 1], "| status | IDLE |");
    }

    #[test]
    fn card_without_branch_has_no_branch_line() {
        let card = render_card(
            "IDLE",
            "foo",
            "repoR",
            None,
            None,
            None,
            "123",
            "tools",
            "",
            "10-02 14:11",
            "u",
            "a",
            EMPTY_EVENTS,
            None,
        );
        assert!(!card.lines().any(|l| l.starts_with("| branch |")));
    }

    #[test]
    fn git_branch_reads_the_checked_out_branch() {
        // The probe forks git; skip when git is missing (the card
        // omits the key there).
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        let d = std::env::temp_dir().join(format!("rushi-gitbranch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let run = |args: &[&str]| -> String {
            String::from_utf8(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&d)
                    .output()
                    .expect("git ran")
                    .stdout,
            )
            .expect("utf-8")
        };
        run(&["init", "-q", "-b", "main", "."]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(d.join("f"), "x").unwrap();
        run(&["add", "f"]);
        run(&["commit", "-q", "-m", "one"]);
        assert_eq!(git_branch(&d), Some("main".to_string()));
        // Detached HEAD: the short commit, tagged.
        run(&["checkout", "-q", "--detach", "HEAD"]);
        let sha = run(&["rev-parse", "--short", "HEAD"]).trim().to_string();
        assert_eq!(git_branch(&d), Some(format!("detached@{sha}")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn git_branch_none_outside_a_repo() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        let d = std::env::temp_dir().join(format!("rushi-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(git_branch(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Run one git command in `dir` and return its stdout.
    fn git_run(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git ran");
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// A fixture git repo in a temp dir: `init -b main`, a local
    /// user identity, and one committed file (`f1`).
    fn git_fixture(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rushi-gitstatus-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        git_run(&d, &["init", "-q", "-b", "main", "."]);
        git_run(&d, &["config", "user.email", "t@example.com"]);
        git_run(&d, &["config", "user.name", "t"]);
        std::fs::write(d.join("f1"), "one").unwrap();
        git_run(&d, &["add", "f1"]);
        git_run(&d, &["commit", "-q", "-m", "one"]);
        d
    }

    fn skip_if_no_git() -> bool {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: git is not on PATH");
            true
        } else {
            false
        }
    }

    #[test]
    fn git_status_brief_clean_repo_reports_clean() {
        if skip_if_no_git() {
            return;
        }
        let d = git_fixture("clean");
        assert_eq!(git_status_brief(&d), Some("clean".to_string()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn git_status_brief_counts_each_status() {
        if skip_if_no_git() {
            return;
        }
        let d = git_fixture("counts");
        // f3 joins the tracked set, then gets a worktree delete.
        std::fs::write(d.join("f3"), "three").unwrap();
        git_run(&d, &["add", "f3"]);
        git_run(&d, &["commit", "-q", "-m", "three"]);
        std::fs::remove_file(d.join("f3")).unwrap();
        // f1 gets a worktree change.
        std::fs::write(d.join("f1"), "one changed").unwrap();
        // f2 is a staged new file with a worktree change (`AM`: it
        // counts in both the added and the changed buckets).
        std::fs::write(d.join("f2"), "new").unwrap();
        git_run(&d, &["add", "f2"]);
        std::fs::write(d.join("f2"), "newer").unwrap();
        // u1 is untracked.
        std::fs::write(d.join("u1"), "u").unwrap();
        assert_eq!(
            git_status_brief(&d),
            Some("1 added, 2 modified, 1 deleted, 1 untracked".to_string())
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn git_status_brief_none_outside_a_repo() {
        if skip_if_no_git() {
            return;
        }
        let d = std::env::temp_dir().join(format!("rushi-gitsb-norepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(git_status_brief(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn session_workdir_reads_the_cwd_file() {
        let d = std::env::temp_dir().join(format!("rushi-wd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let proj = d.join("proj");
        let sess = proj.join("sessions/foo");
        std::fs::create_dir_all(&sess).unwrap();
        // The recorded cwd wins over the dirname fallback.
        let w2 = d.join("w2");
        std::fs::create_dir_all(&w2).unwrap();
        std::fs::write(sess.join("cwd"), w2.display().to_string()).unwrap();
        assert_eq!(session_workdir(&sess), Some(w2.clone()));
        // A stale cwd file falls back to the project dir.
        std::fs::write(sess.join("cwd"), "/no/such/dir").unwrap();
        assert_eq!(session_workdir(&sess), Some(proj.clone()));
        // A missing cwd file falls back the same way.
        std::fs::remove_file(sess.join("cwd")).unwrap();
        assert_eq!(session_workdir(&sess), Some(proj.clone()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
