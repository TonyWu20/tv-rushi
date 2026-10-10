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

    // Decision 53: the live branch appends with a plain `rushi run`
    // (the kernel branches on the session lock itself). No action
    // carries the dropped `--no-run` flag.
    let sm = v["actions"]["send_message"]["command"].as_str().unwrap();
    assert!(!sm.contains("--no-run"), "send_message: {sm}");
    assert!(sm.contains("cd \"$w\" &&"), "send_message cds to the working dir: {sm}");
    assert!(sm.contains("setsid rushi run"), "idle branch starts the loop: {sm}");

    // The preview command still carries the seven split tokens.
    let prev = v["preview"]["command"].as_str().unwrap();
    assert!(prev.contains("rushi-sessions preview"), "preview: {prev}");

    // Decision 55: new_session creates a sibling session in the
    // entry's project dir with an explicit --sessions-root (rushi
    // 0.1.1+). The prompt is an $EDITOR file (line 1 the name, the
    // rest the message); an unusable name cancels; the new loop
    // starts detached, like the send_message idle branch.
    let ns = v["actions"]["new_session"]["command"].as_str().unwrap();
    assert!(
        ns.contains("--sessions-root \"$r/sessions\""),
        "new_session pins the sessions root: {ns}"
    );
    assert!(ns.contains("sh '{split:\t:6}'"), "tab token: {ns}");
    assert!(ns.contains("setsid rushi run"), "new loop starts detached: {ns}");
    assert!(ns.contains("\"#\"*"), "unusable-name guard: {ns}");
    assert!(
        ns.contains("mktemp \"$d/tv-rushi-new.XXXXXX\""),
        "prompt file: {ns}"
    );
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
    // Decision 52/53: the action cds to the session's recorded
    // working dir (the CWD of this channel is the session dir, not
    // the project dir) and appends with a plain `rushi run`.
    let sm = v["actions"]["send_message"]["command"].as_str().unwrap();
    assert!(sm.contains("rp=$(pwd)"), "session dir from CWD: {sm}");
    assert!(sm.contains("cat \"$rp/cwd\""), "cwd file read: {sm}");
    assert!(!sm.contains("--no-run"), "no dropped flag: {sm}");
}
