## 75. Events channel yank_entry: the full-document copy on ctrl-y (2026-10-19)

The native ctrl-y of television copies the raw source row of the
selected entry (the one-line TSV). It cannot carry the
newline-preserving document. The copy content is the raw row,
fixed in code (tv 0.15.9 and main both join `e.raw`), and the
source is line-based. No channel knob changes it. The `[source]
output` template feeds only the exit-printed text, and it is a
template over the same one-line row.

Decision: a custom action `yank_entry` bound to ctrl-y (the
channel keybinding overrides the built-in, like ctrl-t to
send_message). The fork runs the binary's new
`copy-entry DIR SEQ` subcommand. It renders the entry the way
`event-preview` does (markdown for messages, toml for
everything else). It copies the document through two routes: the
OSC 52 terminal clipboard escape is written to the controlling
tty first, then a local clipboard tool takes the pipe when one is
on PATH. The tool order is pbcopy, termux-clipboard-set,
wl-copy, xclip, and xsel (television's own order). The tool
write lands last, so it wins on hosts that have one. Without
either route the subcommand exits 1 with a stderr line.

The host baseline has no clipboard tool (a TTY-only session).
The live route there is the OSC 52 escape. tmux 3.8 with
`set-clipboard on` forwards it to the outer terminal's system
clipboard. The escape matches television's own write: the
standard-alphabet base64 payload, the ST terminator.

The channel TOML gains the action, the ctrl-y binding, and the
header note. The Nix mirror and the `toml_channel` lock track
them. The README's two binding lists name it. No new channel
requirement: the action needs only `rushi-sessions`, already
required.

Verified: `cargo test -p rushi-sessions` is green (the yank unit
tests carry the RFC 4648 base64 vectors, the escape shape, and
the missing-seq guard). A pty smoke test shows the exit-0
status line and the decoded document with newlines. A fake
`wl-copy` on PATH proves the tool route. The Nix attrset
decodes to the same value as the repo TOML (`nix eval` JSON
against the TOML decode, leaf for leaf).

## 74. Events channel tree view: the depth column and the branch actions (2026-10-17)

The events channel list now carries a fifth source column: the
tree-depth box connector of the session's branch structure (the
`rushi-tui` tree view's indent, ported to the flat television list
in `src/tree.rs`). The display template leads with the indent
column; the seq token stays in column 0.

The three tree-view options of the TUI are channel actions
(`src/branch.rs`): `rewind` (alt-r, append a rewind marker through
the kernel's `log` binary), `summarize` (alt-s, the kernel's
`compact --branch`), and `summarize-prompt` (alt-p, the same
through the editor prompt flow, a blank file cancels). Both need
the loop idle: the binary probes the kernel's `.loop.lock` flock
and refuses a live loop.

The `requirements` gain `rushi` (the binary resolves the kernel's
`log` and `compact` siblings through it). The Nix mirror tracks
the new actions, bindings, requirements and display template.

The list marker column now prints a short tag of at most two
capitalized letters (`short_tag` in `src/format.rs`): `U` is
user_message, `A` is assistant_message, `C` is tool_call, `R` is
tool_result, `S` is ext_status, `W` is rewind, `T` is
user_message_retract, `X` is cancel, `E` is error, `M` is
compaction_summary, `K` is compaction_started, `F` is
compaction_failed, `CE` is context_exhausted, `AR` is
approval_request, and `D` is approval. An unknown type prints its
first two letters. The full type name stays in the preview
document; the list column carries the tag.

The source row is now five TSV columns: seq, tag, ts, summary,
indent. The seq stays in column 0, so the `output` token and
every action command keep reading `{split:\t:0}`.

Verified: `cargo test -p rushi-sessions` is green (the tree depth
unit tests carry the kernel's three-rewind chain; the branch tests
run against a stubbed kernel bin dir with the idle guard held
through a real flock). The flake `rushi-sessions-events-channel`
derivation is byte-identical to the repo TOML (`cmp`). The Nix
attrset decodes to the same value as the repo TOML (`nix eval`
JSON against the TOML decode).

## 73. Prompt actions move into the rushi-sessions binary (2026-10-17)

The three prompt actions (main send_message, main new_session, events
send_message) ran as shell oneliners. They layered four escapes: the
channel TOML string, the shell oneliner, the heredoc body, and the
base64 blob for remote rows. The unquoted heredoc broke on `$`
expansion. The remote base64 line failed with `base64: invalid input`
on a stale build. Each fix risked the next layer.

Decision: move all prompt action logic into the `rushi-sessions` Rust
binary (the new `src/prompt.rs`). The action commands become one
binary call each. No shell oneliner, no heredoc, no base64 in the
command string. The values cross the tmux boundary through a JSON
args file, not positional shell args.

Two-stage design:

- Stage 1 runs in the television fork. It resolves the row, the
  host, the working dir, the project root, the editor, the prompt
  file, and the args file. It spawns `tmux new-pane -f` with the
  binary's own absolute path as the pane command. No shell quoting
  crosses the tmux boundary.
- Stage 2 is the pane's own command (`run-pane --args file`). It
  reads and deletes the args file, runs the editor, then runs the
  create or send step. The floating pane auto-closes two seconds
  after the step.

The send step detaches (`setsid nohup rushi run`) only in pane
mode, so the pane close does not kill the pipeline. The inline
fallback (no tmux, or below the 3.8 gate) runs the step in this
process, matching the shell design. The new_session create step is
one `rushi run <name> --sessions-root` call. The kernel creates the
session dir. No separate `tmux new-session`.

Verified end to end on tmux 3.8: the floating pane opens, the
editor persists, the message sends, the new session creates, the
pane auto-closes. Cargo tests pass (including the args-file round
trip). The Nix module stays in lockstep with the repo TOML. The
`nix build` channels are byte-identical to the repo mirrors.

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
