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
    let cmd = v["actions"]["open"]["command"]
        .as_str()
        .expect("open command");
    // The remote branch of open runs the remote TUI over ssh in a
    // local pane, and the template token keeps a real tab.
    assert!(cmd.contains("ssh -t"), "remote branch: {cmd}");
    assert!(cmd.contains("sh '{split:\t:6}'"), "tab token: {cmd}");
    let cmdv = v["actions"]["open_v"]["command"].as_str().unwrap();
    assert!(cmdv.contains("split-window -v"), "open_v: {cmdv}");

    // Decision 88: the plain devShell gate. When the session's
    // project dir carries a flake.nix and nix is on PATH, the local
    // branch runs the TUI as `nix develop -c rushi-tui`. The remote
    // branch runs the same check over one read-only ssh -T call
    // (the remote flake file plus `command -v nix`) and runs the
    // remote TUI in the remote devShell. A failed gate keeps the
    // PATH `rushi-tui` (the `tu`/`rt` defaults). No env-var
    // opt-out, no probe, no absolute nix path: the plain
    // `nix develop` inherits the environment, so a binary the
    // devShell does not ship falls back to the inherited PATH
    // entry.
    for a in ["open", "open_v"] {
        let c = v["actions"][a]["command"].as_str().unwrap();
        // The local gate and launch.
        assert!(
            c.contains(
                "tu=rushi-tui; if [ -f \"$r/flake.nix\" ] && command -v nix >/dev/null 2>&1; then tu=\"nix develop -c rushi-tui\"; fi;"
            ),
            "{a} local gate: {c}"
        );
        // The remote gate and launch (the decoded value carries the
        // backslash quotes of the local shell line).
        assert!(
            c.contains(
                r#"rt=rushi-tui; rf=$(ssh -T "$host" "test -f \"$r/flake.nix\" && command -v nix >/dev/null 2>&1 && echo yes" 2>/dev/null); [ -n "$rf" ] && rt="nix develop -c rushi-tui";"#
            ),
            "{a} remote gate: {c}"
        );
        // The fork fallback and the tmux pane command use the
        // resolved local TUI command.
        assert!(
            c.contains("cd \"$r\" && $tu"),
            "{a} fork uses the resolved TUI command: {c}"
        );
        assert!(
            c.contains("cd \\\"$r\\\" && $tu"),
            "{a} pane uses the resolved TUI command: {c}"
        );
        // The remote branch uses the resolved remote TUI command.
        assert!(
            c.contains("cd $r && $rt"),
            "{a} remote branch uses the resolved TUI command: {c}"
        );
        // The simplification is pinned: the pre-88 machinery is
        // gone.
        assert!(
            !c.contains("RUSHI_SESSIONS_NIX_DEV"),
            "{a}: the env-var opt-out is gone: {c}"
        );
        assert!(
            !c.contains("--ignore-environment"),
            "{a}: the probe is gone: {c}"
        );
        assert!(
            !c.contains("nu="),
            "{a}: the absolute nix path is gone: {c}"
        );
    }

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
        assert!(
            !c.contains("tmux \"tmux"),
            "{a} must not spawn remote tmux: {c}"
        );
    }

    // open_dir: the remote branch cds to the project dir, not the
    // session dir.
    let od = v["actions"]["open_dir"]["command"].as_str().unwrap();
    assert!(
        od.contains("cd \\\"$r\\\" && exec"),
        "open_dir project dir: {od}"
    );

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
    assert!(
        sm.contains("rushi-sessions send-message"),
        "binary call: {sm}"
    );
    assert!(sm.contains("'{split:\t:6}'"), "tab token: {sm}");
    assert!(!sm.contains(";"), "no shell sequencing: {sm}");
    assert!(!sm.contains("$(printf"), "no base64 blob: {sm}");
    assert!(!sm.contains("base64"), "no base64: {sm}");
    assert!(
        !sm.contains("setsid rushi run"),
        "no detached send in the command: {sm}"
    );
    assert!(!sm.contains("loop.pid"), "no loop pid probe: {sm}");
    assert!(!sm.contains("kill -0"), "no pid liveness check: {sm}");
    let smd = v["actions"]["send_message"]["description"]
        .as_str()
        .unwrap();
    assert!(
        smd.contains("rushi-sessions"),
        "description names the binary: {smd}"
    );

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
    assert!(
        ns.contains("rushi-sessions new-session"),
        "binary call: {ns}"
    );
    assert!(ns.contains("'{split:\t:6}'"), "tab token: {ns}");
    assert!(!ns.contains(";"), "no shell sequencing: {ns}");
    assert!(
        !ns.contains("setsid rushi run"),
        "no detached send in the command: {ns}"
    );
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
    assert!(
        sm.contains("rushi-sessions send-message --events"),
        "binary call: {sm}"
    );
    // The events source rows carry only the seq token (the first of the
    // five columns), so the send command takes no split placeholder.
    // television treats a stray {group} as a template token.
    assert!(!sm.contains("split:"), "no split placeholder: {sm}");
    assert!(!sm.contains(";"), "no shell sequencing: {sm}");
    assert!(
        !sm.contains("setsid rushi run"),
        "no detached send in the command: {sm}"
    );
    assert!(!sm.contains("loop.pid"), "no loop pid probe: {sm}");
    assert!(!sm.contains("kill -0"), "no pid liveness check: {sm}");
    assert!(sm.contains("--events"), "events variant flag: {sm}");

    // Decision 74: the tree port. The source rows carry a fifth
    // column (the tree-depth box connector), so the display template
    // puts the indent first. The seq token stays in column 0, so
    // every action still reads `{split:\t:0}`.
    let disp = v["source"]["display"].as_str().expect("display template");
    assert!(
        disp.contains("split:\t:4"),
        "display leads with the indent column: {disp}"
    );
    assert!(
        disp.contains("split:\t:1"),
        "display keeps the tag column: {disp}"
    );
    assert!(
        disp.starts_with("{split:\t:4}"),
        "the indent column leads the display: {disp}"
    );
    let out = v["source"]["output"].as_str().unwrap();
    assert!(out.contains("split:\t:0"), "output is the seq: {out}");

    // The three tree-view actions: rewind and summarize (with and
    // without the custom prompt). Each is one binary call with the
    // seq token, fork mode, and no shell sequencing.
    for (a, extra) in [
        ("rewind", ""),
        ("summarize", ""),
        ("summarize-prompt", " --prompt"),
    ] {
        let act = v["actions"][a].clone();
        let c = act["command"].as_str().unwrap();
        assert!(c.contains("rushi-sessions"), "{a} binary call: {c}");
        assert!(c.contains("'{split:\t:0}'"), "{a} takes the seq token: {c}");
        assert!(c.ends_with(extra), "{a} flags: {c}");
        assert!(!c.contains(";"), "{a} no shell sequencing: {c}");
        assert_eq!(act["mode"].as_str().unwrap(), "fork", "{a} fork mode");
        assert!(
            act["shell"].as_str().unwrap().contains("bash"),
            "{a} bash shell"
        );
    }

    // The keybindings bind the three actions.
    assert_eq!(
        v["keybindings"]["alt-r"].as_str().unwrap(),
        "actions:rewind"
    );
    assert_eq!(
        v["keybindings"]["alt-s"].as_str().unwrap(),
        "actions:summarize"
    );
    assert_eq!(
        v["keybindings"]["alt-p"].as_str().unwrap(),
        "actions:summarize-prompt"
    );

    // The yank_entry action: the full-document copy that replaces
    // the native ctrl-y (the raw-row copy). One binary call with
    // the seq token, fork mode, no shell sequencing.
    let ye = v["actions"]["yank_entry"].clone();
    let yc = ye["command"].as_str().unwrap();
    assert!(
        yc.contains("rushi-sessions copy-entry"),
        "binary call: {yc}"
    );
    assert!(yc.contains("'{split:\t:0}'"), "seq token: {yc}");
    assert!(!yc.contains(";"), "no shell sequencing: {yc}");
    assert_eq!(ye["mode"].as_str().unwrap(), "fork", "fork mode");
    assert!(ye["shell"].as_str().unwrap().contains("bash"), "bash shell");
    assert_eq!(
        v["keybindings"]["ctrl-y"].as_str().unwrap(),
        "actions:yank_entry",
        "ctrl-y binds yank_entry"
    );

    // The branch_view action (alt-b): re-open the channel on the
    // picked row's parent branch. The fork patches a temp cable
    // dir: it copies the channel file, rewrites the two source
    // commands to the binary's `--branch` focus flag
    // (`rushi-sessions events --branch $s`, with and without
    // `--chat`), and runs `tv --cable-dir`. Both focused source
    // views (the Ctrl+S User+Assistant and All cycle) stay, and
    // so do the display template, preview and actions. The nested
    // tv runs in a floating pane (`tmux new-pane -f`, the tmux
    // 3.8+ tier); the split-pane form stays as the fallback tier
    // for older tmux.
    let bv = v["actions"]["branch_view"].clone();
    let bc = bv["command"].as_str().unwrap();
    assert!(
        bc.contains("--cable-dir"),
        "the nested tv loads the temp cable dir: {bc}"
    );
    assert!(
        bc.contains("rushi-sessions events --branch"),
        "the binary focus flag: {bc}"
    );
    assert!(
        bc.contains("--branch $s --chat"),
        "the focused chat view keeps the flag: {bc}"
    );
    assert!(
        bc.contains("rushi-sessions-events.toml"),
        "the channel file lands in the temp cable dir: {bc}"
    );
    assert!(
        bc.contains("mktemp -d"),
        "the temp cable dir: {bc}"
    );
    assert!(bc.contains("'{split:\t:0}'"), "seq token: {bc}");
    // The pane lookup keeps the browse_events control-char -F
    // format (the pane id and floating flag assembled without
    // braces, so television's template pipeline sees no stray
    // group token).
    assert!(
        bc.contains("pane_floating_flag"),
        "the pane lookup keeps the control-char -F format: {bc}"
    );
    // The floating-pane tier (tmux 3.8+): the nested tv runs in
    // a `new-pane -f` pane over the tv window, sized and
    // labeled; the split-pane fallback stays behind it. The pane
    // shell deletes the temp cable dir after the nested tv exits.
    assert!(
        bc.contains("new-pane -f"),
        "the floating pane (tmux 3.8+ tier): {bc}"
    );
    assert!(
        bc.contains("tmux split-window"),
        "the split-pane fallback tier: {bc}"
    );
    assert!(
        bc.contains("rm -rf"),
        "the pane shell deletes the temp cable dir: {bc}"
    );
    assert_eq!(
        bc.matches('{').count(),
        1,
        "one split token, no stray brace: {bc}"
    );
    assert_eq!(
        bv["mode"].as_str().unwrap(),
        "fork",
        "branch_view fork mode"
    );
    assert!(
        bv["shell"].as_str().unwrap().contains("bash"),
        "branch_view bash shell"
    );
    assert_eq!(
        v["keybindings"]["alt-b"].as_str().unwrap(),
        "actions:branch_view",
        "alt-b binds branch_view"
    );
}
