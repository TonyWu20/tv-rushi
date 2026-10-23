## 86. The preview card shows the working dir's git branch (2026-10-24)

The `[rushi-session]` table of the preview card gains a `branch`
key: the checked-out branch of the session's working dir. The key
sits under the `host` line, under `repo` for local rows.

Decision:

- The binary probes the dir with git at render time. `git -C <dir>
  rev-parse --abbrev-ref HEAD` gives the branch name. A detached
  HEAD renders as `detached@<short sha>`, the short form of
  `rev-parse --short HEAD`.
- The dir is the session's recorded working dir. The `cwd` file in
  the session dir wins. A missing or stale `cwd` file falls back
  to the project dir, dirname(dirname(session dir)), the open
  actions' rule. The probe passes the recorded dir to git, not the
  repo root. Git finds the enclosing repo itself.
- The key is omitted when git is missing, the dir is not in a
  repo, the repo has no HEAD (an unborn branch), or no usable dir
  resolves at all. No new channel plumbing. The card keys come
  from the binary.
- Remote rows probe on the remote host. The remote binary renders
  the card there, over the remote working dir. The local binary
  ships no branch value.

Verification: `cargo test -p rushi-sessions` is green (124 tests).
The card tests pin the key under `host` (remote) and under `repo`
(local), and the omission on None. A fixture git repo pins the
checked-out name and the detached rendering. A smoke render of a
git workdir shows the key; a non-git workdir omits it.

Out of scope: a branch column in the source list, and a branch
cache like the phase probe's stat cache.

## 85. The consumer reads move to the shared P8 reader (2026-10-23)

`rushi-common` 0.1.8 (rushi PR #56, issue #55 P8) publishes the
frame-index sidecar, the indexed reads (`open_indexed`, `line_at`,
`tail_lines`, `lines_range`), and the shared tail scan
(`read_log_tail`). This is the consumer switch decision 84's
out-of-scope note pointed at: the local tail-window probe goes
away, and the seq-point reads stop decoding the whole log.

Decision:

- The workspace pins `rushi-common = "0.1.8"`.
- The `last_phase` probe keeps the decision-84 stat cache and
  dispatches the read on the layout. Plain logs keep the 100 KB
  tail read. A zstd stream with a validated sidecar takes the
  indexed read: `tail_lines` with the kernel's own 4096-line
  window. A zstd stream without a sidecar (pre-P8 logs, the
  current corpus) takes the shared `read_log_tail` window scan,
  512 KiB budget, full read when the window holds no frame start.
  A sidecar existence stat keeps the no-index path to one whole-
  file read: `open_indexed` would add a second whole-file read
  before it knows the index is absent.
- `event-preview` and `copy-entry` read their one entry through
  the new `event_at`: `open_indexed` plus `line_at`, the record
  range of the seq with a sidecar, the full-text fallback
  without. An unparseable line at the seq is absent, as the
  `numbered_events` numbering has always treated it.
- The full-log reads stay where the whole stream is the point:
  the `events` listing (seq numbering over every line), `preview`
  (active ranges over the full log), and `branch` (the total
  count behind its past-end check).
- `zstd` returns to a dev-dependency: the main build decodes no
  zstd by hand; the streaming decoder sits in `rushi-common`.
  The test fixtures still build frames, so the dev-dependency
  and the flake build input stay.

Verification: all 118 tests pass. The state suite's six window-
probe tests are replaced by the layout-split probe tests: the
indexed tail on a `LogLine::commit` fixture (the sidecar lands),
the no-index window scan, the torn tail, the deep-marker window
rule, and the cache identity round trip. `event_at` tests cover
the indexed, plain, and past-end paths. `source` rows match the
decision-84 binary row for row on the static corpus and on a
three-layout fixture (indexed, frames-only, plain). `event-
preview` renders the marker entry of the indexed fixture and
keeps its `no event at line N` error on a past-end seq. Timings
on the local corpus: the warm refresh runs 0.19 to 0.24 s
(decision 84 ran 0.2 s); the cold run runs 3.2 s (decision 84
ran 2.4 s). The shared `read_log_tail` runs equal or better in
isolation over the corpus: 1.4 s against the decision-84 probe's
1.5 s across the 308 zstd logs. The extra one-time cold delta
does not reproduce in the isolated probe loop, and no algorithmic
difference accounts for it.

Out of scope: a tail-windowed `events` view would change the
listing UX and waits on a user decision. `preview` and `branch`
keep their full-log reads. The `read_log_tail` whole-file read
is a kernel-side improvement candidate (seek to the window start
instead of reading the file) for a later `rushi-common`. The
remote hosts pick the binary up on their first ssh run after the
channel redeploys. The `rushi-tui` live-tailer migration stays
in the rushi-tui repo, as issue #55 files it.

## 84. The phase probe reads the tail window and caches the result (2026-10-22)

The zstd layout of kernel PR #54 makes `last_phase` decode the
whole session log. The plain layout read a 100 KB tail. A zstd
stream cannot be tail-read, so the probe decodes in full. Every
`--watch` refresh re-runs `source`, and `source` probes every
session. Measured on the local corpus (493 logs, 308 zstd, 81 MB
compressed): 1.8 s per refresh. The `--watch` cycle blinks.

Decision: two cuts to the probe cost, both in the binary.

- A new `zstd_tail_probe` reads the last 512 KiB of the stream.
  Every zstd frame opens with the shared magic bytes, so the probe
  decodes from the first frame start inside the window. When the
  decoded tail covers the 100 KB scan window, the marker scan runs
  on that tail alone. A consolidated single frame, or a tail too
  short to cover the window, falls back to the shared full read of
  decision 81. A false magic occurrence inside a compressed payload
  fails its decode and the next candidate takes over. The torn-tail
  note (P4) keeps its meaning: the decode stops at the last
  complete frame.
- The new `phasecache` module caches the probe result under the
  log's (mtime_ns, size) identity in `$RUSHI_SESSIONS_CACHE_DIR`
  (default `$XDG_CACHE_HOME/rushi-sessions`). An unchanged log
  returns its cached phase without touching the log. The cache is
  advisory: a temp-file plus rename write, and any read trouble
  re-runs the probe.

The `zstd` crate returns to the main dependencies. Decision 83
made it a dev-dependency when the mirror went away. The window
probe needs the streaming decoder in the main build. The flake
already carries `pkgs.zstd` in the `rushi-sessions` build inputs.

Verification: all 119 tests pass, up from 108. The new tests
cover the window probe on a per-commit stream, the consolidated
fallback, the torn tail inside the window, a false magic
candidate, and the cache round trip, the stale key, and the
corrupt entry. `source` rows match the pre-fix binary row for row
on the static corpus (496 rows; the one live session row moves
with its own log). Measured on the local corpus: the pre-fix
steady refresh runs 1.8 s; the post-fix cold run runs 2.4 s
once and builds the cache; the warm run runs 0.2 s. The six-
session zstd fixture runs 0.125 s cold and 0.023 s warm.

Out of scope (as of 84): the `events` subcommand and `preview`
still decode the whole log on every run: the seq numbering needs
the full stream. The kernel-side fix is filed as rushi issue
#55: a frame index sidecar (P8) plus the shared tail and seq
reader in `rushi-common`, the single mechanism for every consumer
crate. A loop-written phase sidecar was rejected in that issue:
it duplicates a fact the log already holds, and it cannot serve
the seq reads or the TUI tailer. PR #56 ships it as
`rushi-common` 0.1.8; decision 85 takes the consumer switch.
Remote roots still pay the ssh round trip on every refresh. The
remote host caches after its first run once the channel redeploys
there. A `ControlMaster` ssh config shortens the connection cost.
Cache entries of deleted sessions linger as one small file each.

## 83. The zstd read layer switches to the shared `rushi-common` (2026-10-21)

`rushi-common` 0.1.7 publishes with the public `logstream`
module. The interim mirror is done.

Decision: delete `logtext.rs`. `numbered_events` and `last_phase`
call `rushi_common::logstream::read_log_text` directly. The
workspace pins `rushi-common` 0.1.7. The phase sniff reads four
bytes and compares the shared `ZSTD_MAGIC` constant. That keeps
the 100 KB tail read on the plain layout. The `zstd` crate is a
dev-dependency now. Only the test fixtures build frames.

Verification: all 108 tests pass. The nix build stays hermetic.
The zstd and plain fixtures give identical rows. The torn tail
shows the complete frames only. Decision 82 is resolved by this
entry.

## 82. The zstd read layer is shared via `rushi-common` (2026-10-21) The zstd read layer is shared via `rushi-common` (2026-10-21)

The `logtext` module of this repo is a copy of the kernel's
`logstream` read layer. The TUI repo needs the same layer. Three
copies of one layout logic would scatter across repos.

Decision: the shared home is `rushi-common`. Kernel PR #54 puts
`logstream` in `crates/rushi`. That crate publishes as
`rushi-common`. The kernel `lib.rs` already exports it.

Main carries 0.1.7 at the merge. The next release after the
merge carries the module. A spec note tells consumers to pin
that release. The note names no version number.

This repo keeps the `logtext` mirror until the release
publishes. After the publish, the mirror goes away. The two
call sites use `rushi_common::logstream::read_log_text`
directly. The TUI repo gets the same switch.

The TUI also adapts its live tailer. The tailer is a
byte-offset design. It only works on plain text.

Verification: the kernel branch checks clean. The spec note
lands in the compression spec. It names no version number.
The mirror in this repo is the only copy in a consumer repo.

## 81. Read the zstd session logs (kernel PR #54) (2026-10-21) Read the zstd session logs (kernel PR #54) (2026-10-21)

Kernel PR #54 (TonyWu20/rushi) makes the session log a zstd stream.
Each commit writes one zstd frame. The idle pass consolidates the
stream into one level-19 frame. A legacy plain text log stays plain
for its life (P2). The format is decided at creation.

The backend read sites assumed plain text. The zstd layout broke
them. The `events` rows printed nothing. The phase column showed
`?`. The `events`, `event-preview`, `copy-entry`, `rewind`,
`summarize` and `preview` commands all read through
`numbered_events`.

Decision: add the `logtext` read layer. It mirrors the kernel's
`logstream::read_log_text`. It sniffs the four zstd magic bytes (P3)
and reads both layouts. It decodes the zstd frames in order. A torn
tail frame stops the read at the last complete frame (P4). The
partial line drops at parse time.

`numbered_events` reads through the layer. `last_phase` sniffs the
layout per file. The plain layout keeps the 100 KB tail read. The
zstd layout decodes in full, and the scan runs on the decoded text.
The seq numbering is the decoded line number. It is the same as the
plain layout. The kernel `target_seq` markers keep pointing at the
same lines.

The backend links the `zstd` crate at 0.14.3, the kernel's exact
pin. The build links the bundled C library statically. No new
runtime requirement. The channel TOML files stay unchanged. The
flake adds `pkgs.zstd` to the `rushi-sessions` nativeBuildInputs.
That mirrors the kernel hermetic build recipe.

Verification: `cargo test` is green. New tests cover the zstd
per-commit frames, the consolidated single frame, and the torn tail
frame of both `numbered_events` and `last_phase`. The nix-built
binary ran `events` and `source` on zstd fixtures. The seq
numbering matched the plain fixture row for row. The torn tail
showed the complete frames only.

## 80. The browse_events_v action splits the events channel below the newest pane (2026-10-21) The browse_events_v action splits the events channel below the newest pane (2026-10-21)

The user asked for the events channel to open below the newest pane.
The browse_events action opens the events channel to the right
of the newest pane (a -h split), per decisions 78 and 79.
browse_events_v opens the same channel below the newest pane instead (a -v split).

Decision: browse_events_v keeps the decision 78 newest-pane target and
changes both local tmux split-window calls from -h to -v. The tmux -v
flag puts the pane below the target, a horizontal divider. The remote-row
branch runs the nested tv over ssh -t, unchanged. Outside tmux, the
plain-fork fallback is unchanged. The Nix attrset and the repo TOML stay
in sync on the command value, the description, and the section comments.

The action binds to alt-e. That key is free because browse_events moved
to enter in 41c6432. The alt-e key is a proposed default. Confirm it or
pick another key.

Verification: nix-instantiate JSON against a TOML decode shows all four
action fields match byte for byte. sh -n and bash -n accept the command.
A live tmux 3.8 scratch session confirms the layout. The newest pane sat
to the right of the tv pane, and focus was back on the tv pane. The
action put the new pane below the newest pane, not the tv pane. The
pane ran the nested tv from the session dir.

## 79. The browse_events action also splits the newest pane (2026-10-21)

The user asked for the decision 78 lookup on the browse_events
action. Its nested tv pane fanned out of the tv pane like the old
open* panes.

Decision: the browse_events tmux branch runs the same newest-pane
lookup as the open* actions and passes it to split-window as
`-t "%$pn"`, with the same no-`-t` fallback when the lookup is
empty. The remote-row branch runs the nested tv over `ssh -t` and
keeps its shape. The lookup stays brace-free, as in decision 67.
The Nix attrset and the repo TOML stay in sync on the command
value, the description, and the section comments.

Verified the same way as decision 78: the exact TOML command text
in a scratch tmux session. The first browse_events lands next to
tv. The next one after a `select-pane` back to the tv pane
anchors to the newest pane, not the tv pane. A live floating pane
does not win the lookup. `sh -n` accepts the command. The Nix
attrset and the TOML agree byte-for-byte on every value.

This entry supersedes the out-of-scope note of decision 78.

## 78. The open* actions split the newest pane, not the tv pane (2026-10-21)

The open, open_v, open_dir_tmux_h and open_dir_tmux_v actions
created their pane with `tmux split-window` without a `-t` target.
The default target is the active pane of the current window. When
the user moved focus back to the tv pane, the next open re-anchored
to the tv pane. The panes fanned out of tv instead of stacking.
Each open landed next to tv, whatever the user wanted.

Decision: each open* action resolves the newest pane of the tv
window and passes it as the split target (`-t "%$pn"`). tmux pane
ids are monotonic per server. The largest live id is the newest
pane. The lookup runs `tmux list-panes` with the `#{pane_id}` and
`#{pane_floating_flag}` formats, keeps the regular panes only,
sorts the ids, and takes the largest. The first open lands next to
tv. Each later open stacks next to the pane it just created.

Floating panes are excluded by the flag. A prompt pane from
send_message or new_session never wins the lookup. An empty lookup
falls back to the tmux default target, the active pane (the old
behavior). Outside tmux, or without the tmux binary, the fork
behavior is unchanged.

The lookup stays brace-free, as the television template pass
requires (decision 67). The tmux format builds at runtime from the
`printf "{"` and `printf "}"` octal escapes. No brace group
appears in the command text. The `sed`, `sort`, `tail` and shell
`printf` steps run in the fork; both ship in the standard Unix
base.

Verified on tmux 3.8 with the exact TOML command text in a scratch
session. The open action put the first pane to the right of the tv
pane. The open_v action put the next pane below the open pane, not
below the tv pane. The open action after a `select-pane` back to
the tv pane anchored to the newest pane, not the tv pane. The
open_dir_tmux_h and open_dir_tmux_v actions stack the same way,
and the shell panes start in the session project dir. A live
floating pane in the window did not win the lookup; the split
anchored to the newest regular pane. The Nix attrset and the repo
TOML agree byte-for-byte on every action value (`nix-instantiate`
JSON against the TOML decode). `sh -n` accepts every rewritten
command.

Out of scope: the browse_events action kept the default target.
That note is superseded by decision 79. The remote-row branches
of the open* actions get the same targeting; they need a live
ssh host to test.

## 77. send_message: escape the task with `--`, report spawn failures honestly (2026-10-20)

The `send_message` action lost every message that starts with a
hyphen. The detached send is `setsid rushi run <session> <msg>`;
the kernel's clap rejected a hyphen-leading `task` as an unknown
flag (`unexpected argument '- ...'`, exit 2), and the detached
call sends stderr to /dev/null. The action still printed
`message queued for <session>`, so the loss was silent. A review
message that opens a bullet list (`- Too many they...`) hit this
on three attempts; a plain first character always worked, which
read as an occasional failure.

Decision: the local message-mode call becomes
`rushi run <session> -- <msg>`, and the remote form gains the
same escape before the base64 task (`rushi run '<rp>' --
"$(printf %s <b64> | base64 -d)"`). The `--` escape is valid on
kernels with and without the clap fix (the kernel now carries
`allow_hyphen_values` on `run` task and session, rushi 0.1.4, and
on `user` content, user 0.1.3). The remote escape applies to the
message mode only: a trailing `--sessions-root` after `--` would
parse as a positional.

The spawn helpers return a result. The action prints
`message queued` only when the spawn started; a failed spawn
prints `nothing sent` on stderr and exits 1, so the terminal
state is observable. The new_session spawn reports the same way.

Verified: `cargo test -p rushi-sessions` is green (the remote
command shape test locks the `--` position). An end-to-end
`run-pane` against the installed 0.1.3 kernel with a
hyphen-leading multi-line message appends the message verbatim
(newlines intact) to a live session's log. The same call against
a free lock starts the loop and seeds the message.

The kernel side is filed as rushi issue #52, fixed in
rushi PR #53 (`allow_hyphen_values` on `run` session/task and
`user` content; rushi 0.1.4, user 0.1.3).

Out of scope: a hyphen-leading new_session name (the name is the
first positional; the trailing `--sessions-root` blocks the
escape). The TUI multi-line paste defect (newlines land at the
end of the pasted message under tmux; bracketed paste is dropped
outright) is a separate issue in the rushi-tui repo.

## 76. Events-channel summarize follows the TUI one-shot (rushi-tui #37/#38) (2026-10-19)

The two summarize actions kept the old TUI guards: a rewind
marker had to exist, and the pick had to sit on the active path.
The TUI one-shot redesign (issue #37, PR #38) dropped both. The
picked entry becomes the rewind target.

Decision: the channel's `summarize` and `summarize-prompt` follow
the one-shot flow. The binary validates the seq, then takes the
prompt when asked. A blank cancel appends no marker. It appends
the `rewind` marker at the pick through the kernel's `log`
binary. The marker is mode `on` (the picked event stays in the
context), reason `tv_pick`. It then runs the kernel's `compact
--branch` in the recorded working dir. The marker append
blocks, so it lands before the compact child reads the log. No
pre-existing marker is needed. A pick in an abandoned span
commits: the marker re-enters the branch.

The `rewind` action keeps its own semantics (a user_message pick
rewinds `before` it). The idle-loop guard stays on all three
actions.

Verified: `cargo test -p rushi-sessions` is green. The branch
suite covers the one-shot: the marker-less pick appends the
marker and runs the compact. The masked pick re-enters. The
past-end seq refuses. The prompt cancel appends nothing. The Nix
mirror keeps the same action descriptions. The flake channel file
and the home-manager attrset stay in lockstep with the repo TOML.

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
