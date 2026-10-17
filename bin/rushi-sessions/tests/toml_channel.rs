//! The repo channel TOML files must parse. The action commands are
//! multi-line basic strings with heavy quoting (escaped quotes,
//! doubled backslashes, `\t` template tokens). A broken escape
//! breaks television's template parse, so the guard lives here, at
//! build time, not at tv start.

use std::fs;

fn repo() -> std::path::PathBuf {
    // <repo>/bin/rushi-sessions -> <repo>
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The actions that resolve the session's project dir from the
/// session's recorded `cwd` file (walked up to the nearest ancestor
/// that holds a `sessions` dir), with the path-based
/// dirname(dirname(session_dir)) as the fallback.
const PROJECT_DIR_ACTIONS: [&str; 5] = [
    "open",
    "open_v",
    "open_dir",
    "open_dir_tmux_h",
    "open_dir_tmux_v",
];

#[test]
fn session_channel_toml_parses() {
    let text = fs::read_to_string(repo().join("rushi-sessions.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("rushi-sessions.toml must parse");
    let cmd = v["actions"]["open"]["command"].as_str().expect("open command");
    // The remote branch of open runs the remote TUI over ssh in a
    // local pane, and the template token keeps a real tab.
    assert!(cmd.contains("ssh -t"), "remote branch: {cmd}");
    assert!(cmd.contains("sh '{split:\t:6}'"), "tab token: {cmd}");
    let cmdv = v["actions"]["open_v"]["command"].as_str().unwrap();
    assert!(cmdv.contains("split-window -v"), "open_v: {cmdv}");

    // Decision 52: the project dir comes from the session's recorded
    // `cwd` file (walked up to the nearest `sessions` ancestor), not
    // from the session path. The dirname fallback stays for a missing
    // or stale `cwd` file.
    for a in PROJECT_DIR_ACTIONS {
        let c = v["actions"][a]["command"].as_str().unwrap();
        assert!(
            c.contains("cat \"$rp/cwd\""),
            "{a} must read the session's cwd file: {c}"
        );
        assert!(
            c.contains("[ ! -d \"$w/sessions\" ]"),
            "{a} must walk up to the sessions ancestor: {c}"
        );
        assert!(
            c.contains("$(dirname \"$(dirname "),
            "{a} keeps the dirname fallback: {c}"
        );
    }

    // open_dir_tmux_h/v: the remote branch opens a local pane that
    // runs the remote shell over ssh -t, so no remote tmux. The
    // project dir resolves on the remote host with one read-only
    // ssh -T call (the cwd file); the pane program stays plain.
    for a in ["open_dir_tmux_h", "open_dir_tmux_v"] {
        let c = v["actions"][a]["command"].as_str().unwrap();
        assert!(c.contains("ssh -t"), "{a} remote pane: {c}");
        assert!(c.contains("ssh -T"), "{a} remote cwd resolve: {c}");
        assert!(
            c.contains("c=\"cd $r && exec $sh2\""),
            "{a} pane program runs the shell only: {c}"
        );
        assert!(!c.contains("tmux \"tmux"), "{a} must not spawn remote tmux: {c}");
    }

    // open_dir: the remote branch cds to the project dir, not the
    // session dir.
    let od = v["actions"]["open_dir"]["command"].as_str().unwrap();
    assert!(od.contains("cd \\\"$r\\\" && exec"), "open_dir project dir: {od}");

    // Decision 72: the send is one detached `rushi run` call for both
    // loop states. The kernel branches on the session lock itself, so
    // no action probes the loop pid and no action carries the dropped
    // `--no-run` flag.
    let sm = v["actions"]["send_message"]["command"].as_str().unwrap();
    // The whole action is one binary call: the binary resolves the row,
    // the working dir, the editor and the send step itself. No shell
    // oneliner, no heredoc, no base64, no detached `rushi run` in the
    // command string (the binary runs the send step as the pane's own
    // process, detached only when the pane closes).
    assert!(sm.contains("rushi-sessions send-message"), "binary call: {sm}");
    assert!(sm.contains("'{split:\t:6}'"), "tab token: {sm}");
    assert!(!sm.contains(";"), "no shell sequencing: {sm}");
    assert!(!sm.contains("$(printf"), "no base64 blob: {sm}");
    assert!(!sm.contains("base64"), "no base64: {sm}");
    assert!(!sm.contains("setsid rushi run"), "no detached send in the command: {sm}");
    assert!(!sm.contains("loop.pid"), "no loop pid probe: {sm}");
    assert!(!sm.contains("kill -0"), "no pid liveness check: {sm}");
    let smd = v["actions"]["send_message"]["description"].as_str().unwrap();
    assert!(smd.contains("rushi-sessions"), "description names the binary: {smd}");

    // The preview command still carries the seven split tokens.
    let prev = v["preview"]["command"].as_str().unwrap();
    assert!(prev.contains("rushi-sessions preview"), "preview: {prev}");

    // Decision 55: new_session creates a sibling session in the
    // entry's project dir with an explicit --sessions-root (rushi
    // 0.1.1+). The prompt is an $EDITOR file (line 1 the name, the
    // rest the message); an unusable name cancels; the new loop
    // starts detached, like the send_message call.
    let ns = v["actions"]["new_session"]["command"].as_str().unwrap();
    // Like send_message: one binary call, the binary owns the prompt
    // file, the name validation and the session create.
    assert!(ns.contains("rushi-sessions new-session"), "binary call: {ns}");
    assert!(ns.contains("'{split:\t:6}'"), "tab token: {ns}");
    assert!(!ns.contains(";"), "no shell sequencing: {ns}");
    assert!(!ns.contains("setsid rushi run"), "no detached send in the command: {ns}");
    assert!(!ns.contains("loop.pid"), "no loop pid probe: {ns}");
    let kb = v["keybindings"]["alt-n"].as_str().unwrap();
    assert_eq!(kb, "actions:new_session", "alt-n binds new_session: {kb}");
}

#[test]
fn events_channel_toml_parses() {
    let text = fs::read_to_string(repo().join("rushi-sessions-events.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("rushi-sessions-events.toml must parse");
    // Decision 45: the channel carries no top-level `watch` key. A
    // reload resets the preview panel's scroll position, so reloads
    // stay one-run CLI flags (`tv rushi-sessions-events --watch N`).
    assert!(v.get("watch").is_none());
    // Decision 52/72: the action cds to the session's recorded
    // working dir (the CWD of this channel is the session dir, not
    // the project dir) and pokes with one detached `rushi run` call.
    // The kernel branches on the session lock itself.
    let sm = v["actions"]["send_message"]["command"].as_str().unwrap();
    // The CWD-mode variant: the binary reads the session's recorded
    // working dir itself (the cwd file read moved into the binary).
    assert!(sm.contains("rushi-sessions send-message --events"), "binary call: {sm}");
    // The events source rows carry only the seq token (four fields),
    // so the command takes no split placeholder. It stays brace-free:
    // television treats a stray {group} as a template token.
    assert!(!sm.contains("split:"), "no split placeholder: {sm}");
    assert!(!sm.contains(";"), "no shell sequencing: {sm}");
    assert!(!sm.contains("setsid rushi run"), "no detached send in the command: {sm}");
    assert!(!sm.contains("loop.pid"), "no loop pid probe: {sm}");
    assert!(!sm.contains("kill -0"), "no pid liveness check: {sm}");
    assert!(
        sm.contains("--events"),
        "events variant flag: {sm}"
    );
}
