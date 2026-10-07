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
    // open_dir_tmux_h: the remote branch opens a local pane that runs
    // the remote shell over ssh, no remote tmux.
    let odtmh = v["actions"]["open_dir_tmux_h"]["command"].as_str().unwrap();
    assert!(odtmh.contains("ssh -t"), "open_dir_tmux_h: {odtmh}");
    assert!(!odtmh.contains("ssh -T"), "open_dir_tmux_h must not spawn remote tmux: {odtmh}");
    let odtmv = v["actions"]["open_dir_tmux_v"]["command"].as_str().unwrap();
    assert!(!odtmv.contains("ssh -T"), "open_dir_tmux_v must not spawn remote tmux: {odtmv}");
    // open_dir: the remote branch cds to the project dir, not the
    // session dir.
    let od = v["actions"]["open_dir"]["command"].as_str().unwrap();
    assert!(od.contains("cd \\\"$r\\\" && exec"), "open_dir project dir: {od}");
    // The preview command still carries the seven split tokens.
    let prev = v["preview"]["command"].as_str().unwrap();
    assert!(prev.contains("rushi-sessions preview"), "preview: {prev}");
}

#[test]
fn events_channel_toml_parses() {
    let text = fs::read_to_string(repo().join("rushi-sessions-events.toml")).unwrap();
    let v: toml::Value = toml::from_str(&text).expect("rushi-sessions-events.toml must parse");
    assert_eq!(v["watch"].as_integer(), Some(5));
}
