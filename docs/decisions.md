## 72. send_message: one detached `rushi run` call for both loop states (2026-10-17)

The `send_message` actions of `rushi-sessions` and `rushi-sessions-events`
probed the loop pid and branched on it. They read `loop.pid` and checked it
with `kill -0` (remote rows via ssh). A live loop took the plain `rushi run`
append. An idle session took the detached start. The echo diverged:
`message sent to` on the live branch, `message queued for` on the idle
branch.

The harness now makes `rushi run <session> <task>` the single canonical
poke (rust-unix-harness commit fb8c521 retired `run --no-run`). The kernel
branches on the session lock (`.loop.lock`) itself: a live loop appends
the message and exits 0, a free lock starts the loop. The caller never
branches on the loop state.

The channel now issues one detached `rushi run` call for both states, in
the session's recorded working dir. The call keeps the detach form. It
uses `setsid` + `&` with stdio to `/dev/null`, or the `nohup` fallback
when setsid is absent. The idle call still blocks, because the kernel
starts the loop. The live call appends and exits fast, so the detach is
harmless.

The pid probe, the `LIVE` and `$live` plumbing, and both `if [ "$live" =
yes ]` branches are gone from all four channel files. The branches sat in
the w-region, the tmux popup pipeline body, and the plain tail. The pane
pipeline args drop the live flag: `RP HOST W F ED` (main) and `RP W F ED`
(events). One echo line: `message queued for <session>`. The header
comments and `description` strings state the new contract.

Locked in `tests/toml_channel.rs`: the send commands carry no `loop.pid`,
no `kill -0`, no `$live`, and they keep the detached `setsid rushi run`
call.

## 71. Events-channel test: fix the stale rp assertion in the events test (2026-10-16)

The events test asserted `rp=$(pwd)`, but the channel command resolves
the session dir with `rp=$(pwd 2>/dev/null)` plus a guard. The
assertion was stale since that guard was added. The test now asserts
`rp=$(pwd 2>/dev/null)`, matching the command and verifying the guard.

## 70. Events-channel send_message: open the prompt as a .md file in the project dir (2026-10-16)

The events channel got the same prompt design as the main channel
(#69). The `send_message` prompt file now carries a `.md` extension,
so `$EDITOR` applies the markdown filetype. The pane editor now `cd`s
to the session's recorded working dir before opening `$EDITOR`, so
typed relative paths complete against it. The file is made with
`mktemp` then renamed to `.md` with `mv`. A `.md` suffix on the
`mktemp` template is GNU-only. It breaks BSD/macOS `mktemp`, which
does no randomization when a suffix follows the X-run.

Updated `rushi-sessions-events.toml`, its Nix mirror
`rushi-sessions-events-channel.nix` (command and description), and
locked in `tests/toml_channel.rs`.

## 69. Main-channel send_message and new_session: markdown prompt file, editor opens in the project dir (2026-10-15)

The `send_message` (ctrl-t) and `new_session` (alt-n) actions of the
`rushi-sessions` channel open a temp file in `$EDITOR` (fallback `nvim`).
The user wanted two refinements, applied to the main channel only:

1. The prompt file now carries a `.md` extension so `$EDITOR` applies the
   markdown filetype. The markdown features and plugins then work. The
   file is made with `mktemp` then renamed to `.md` with `mv`. A `.md`
   suffix on the `mktemp` template is GNU-only. It breaks BSD/macOS
   `mktemp`, which does no randomization when a suffix follows the X-run.
2. The pane editor now `cd`s to the project dir before opening `$EDITOR`.
   Typed relative paths then complete against that dir. For `send_message`
   the project dir is the session's recorded working dir (`$w`). For
   `new_session` it is the entry's project dir (`$r`). The `cd` is
   local-only. A remote row has no local dir to `cd` to. The tmux
   `display-popup` path already sets the start dir with `-d`.

Updated `rushi-sessions.toml`, its Nix mirror `rushi-sessions-channel.nix`
(command and description), and locked in `tests/toml_channel.rs`.

## 68. Events-channel send_message: remove the extra trailing fi from the popup pipeline (2026-07-14)

The events-channel `send_message` popup pipeline (the `cat > "$g" <<PIPE`
body) carried a leftover trailing `fi` from the main channel's remote-wrapped
form: the main channel's pipeline wraps the local section in
`if [ -n "$HOST" ]; then ... else ... fi; fi`, but the events version dropped
the wrapper while keeping its closing `fi`. The result: `sh "$g"` inside
`display-popup -E` always hit a syntax error (exit 2), `display-popup`
returned 2, and the command's `if [ "$rc" -eq 0 ]` check failed and fell
through to the pane editor silently. Observed symptom: the popup flashed and
the editor reopened in the main pane; the main channel was unaffected (its
pipelines parse clean). Fixed by removing one trailing `fi` in
`rushi-sessions-events-channel.nix` and the repo TOML mirror. Verified:
`sh -n` on the parsed pipeline body passes, the built channel TOML is
byte-identical to the repo mirror, and an end-to-end probe in the live tmux
(3.7c) with the rebuilt command shows `display-popup -E` returning 0 and no
fallback-echo in the fork stdout. The user must rebuild tv-rushi to deploy.
