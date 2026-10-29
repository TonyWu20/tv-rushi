## 96. The preview card shows the worktree status brief of the session's git repo (2026-10-29)

The `[rushi-session]` table of the preview card gains a `worktree`
key. It is the one-line `git status` brief of the session's
recorded working dir. It is the sibling of decision 86's `branch`
key. The key sits under the `branch` line, or under the `host`/`repo`
slot when the branch key is absent.

Decision:

- The binary probes the dir at render time with `git -C <dir>
  status --porcelain` and counts the files per status.
- The buckets, in their fixed line order: `added`, `modified`,
  `deleted`, `renamed`, `copied`, `conflict`, `untracked`.
- `added` is an index add. `modified` is a worktree change, an
  index change, or a type change. `conflict` is an unmerged
  state: a `U` column, or an `AA`/`DD` pair.
- A file with an index change and a worktree change counts in
  both buckets.
- The line is `clean` when the porcelain output is empty. Else it
  joins one `<N> <status>` group per non-zero bucket with a comma.
  Example: `1 added, 2 modified, 4 untracked`.
- The key is omitted when git is missing, the dir is not in a
  repo, or the probe fails. No new channel plumbing: the card keys
  come from the binary. Remote rows probe on the remote host (the
  remote binary renders the card there).
- No cache. Decision 86 put a branch cache out of scope, and the
  status probe follows the same render-time rule.

Verification: `cargo test -p rushi-sessions` is green (119 unit
tests, up from 114). The fixture git repo pins `clean`, the
per-bucket counts with the `AM` line in two buckets, and the
outside-repo None. The card tests pin the key under `branch` and
under `repo`, and the omission on None. A smoke render shows
`worktree = "1 added, 1 modified, 1 untracked"` on a dirty repo.
A clean repo shows `worktree = "clean"`. A non-git dir shows no
key.

Out of scope: a status cache, ignored-file counts
(`--ignored`), and a status column in the source list.

## 95. devShell resolution follows the project's `.envrc` (2026-10-27)

The 88 through 94 devShell gates used the local `flake.nix` as the
gate. But a project's devShell is named by its `.envrc` (`use flake
<path>`), which can differ from the local `flake.nix`. In
`rushi-config` the local flake has no devShell. `nix develop .` fell
back to a default shell of the flake packages. The `.envrc`
(`use flake ~/programming/`) named the real devShell. This replaces
the local-flake gate with the `.envrc` gate.

Decision (the `.envrc` is the devShell source of truth):

- The effective `.envrc` is the project dir's own, else the first
  one up the parent chain (to `/`).
- Its `use flake <ref>` names the flake dir. A bare `use flake` (or
  `use flake .`) is the `.envrc` dir. A path resolves against it,
  with `~` expanded and relative to the dir. A non-local ref
  (`github:`, a URL) resolves to None.
- No `.envrc` on the chain: the global PATH binary applies, the
  pre-flake behavior. This replaces the 88 local-flake gate.
- The flake dir feeds `devshell::resolve` (the 89 cache and
  materialize) and the 90 kernel launch. The `rushi run` step loads
  the flake by ref so the loop still runs in the working dir.
- The TUI `open`/`open_v` one-liners run the same walk in shell and
  launch `nix develop <flake> -c rushi-tui`. With no `.envrc` they
  keep the PATH `rushi-tui`. The `nix develop` inherits the
  environment, so a binary the devShell lacks keeps the inherited
  PATH entry.
- The remote arm keeps the 88 remote gate: the remote `.envrc` is
  not cheaply readable from the local host.

Confirmed (user): the no-`.envrc` behavior replaces the 88 local-flake
gate. A project with a `flake.nix` but no `.envrc` does not use its
own flake's devShell.

Note: inside a nix `''` string, a `${...}` whose content holds `/`,
`%`, or is empty breaks the parse. So the shell `.envrc` resolver
uses the substring form `${ref:2}` instead of `${ref#"~/"}`.

Verification: `envrc_flake_dir` is unit-tested for the local
`.envrc`, the tilde expansion, the walk-up, the no-`.envrc` None,
and the relative ref. The probe, kernel launch, and `rushi run`
gate follow it. `local_nix_flake_dir` is unit-tested for the
no-`.envrc` PATH case and the seeded devShell case. 146 tests are
green and the three channel derivations build clean in `nix build`.
The `open`/`open_v` one-liners parse under `sh -n`. A
`rushi-config` session now probes the `~/programming` devShell, not
the local flake.

## 94. The `rushi run` send/new-session step keeps the devShell only when `rushi` resolves (2026-10-27)

The 88 devShell extension ran the `rushi run` step of
`send_message` and `new_session` as `nix develop -c rushi run ...`
whenever the 88 gate held. That step is detached (setsid, stdio to
/dev/null), so a `nix develop` that errors (a broken flake, a failed
build) dies with no observable signal: the message is never sent and
the session is never created, and the caller cannot tell. The 89
probe and the 90 kernel launch already fall back to the global PATH
binary on a resolve miss. This extends that fallback to the
`rushi run` step.

Decision (the `prompt` step's local decision; the 88 gate and the
devshell module are unchanged):

- The local arm of the step's `nix_dev` decision is
  `local_nix_dev(dir)`: the 88 gate, plus the requirement that the
  devShell actually resolves `rushi` (the cached
  `devshell::resolve(dir, "rushi")` of 89, the 90 pattern). A broken
  flake fails the bounded materialize, the resolve is a miss, and the
  step falls back to the global PATH `rushi run` instead of a
  `nix develop` that dies with an error.
- The remote arm keeps the 88 remote gate: the remote flake cannot be
  cheaply resolved from the local host, so the remote step still runs
  `nix develop -c rushi run` when the remote gate holds.
- `rushi_run_tail` and `rushi_run_label` key off the same `nix_dev`
  flag, so the argv and the spawn-failure label follow the fallback
  automatically.

Verification: `local_nix_dev` is unit-tested (a flake-less dir stays
on the PATH binary; a flake dir with a seeded resolvable `rushi`
matches the host gate without a nix call). The `rushi run` argv and
label builders are covered by the existing tail/args-file tests. 141
tests green; `nix build` of the three channel derivations is clean.
The TUI `open`/`open_v` one-liners are the remaining `nix develop`
site without a global-PATH fallback; that is a separate change.

## 93. The open* actions land in the recorded working dir, and the TUI gets the absolute session dir (2026-10-27)

The open, open_v, open_dir, open_dir_tmux_h and open_dir_tmux_v
actions resolve the session's working dir from the session's
recorded `cwd` file. The decision 52 resolution walks up from the
`cwd` value to the nearest ancestor that holds a `sessions` dir.
That walk-up assumes the sessions tree sits in the project root.
It breaks for a git worktree without a `sessions` symlink. The
session dir stays in the main worktree's tree, and the worktree
holds no `sessions` dir. The walk-up climbs to the main worktree
root. The `check-placeholder-hardcode` session records its `cwd`
in the `code-check` worktree, but its `open` landed in the repo
root.

Decision (all five actions, local and remote branches):

- The resolution block drops the walk-up. The path fallback
  `dirname(dirname(session_dir))` stays for a missing or stale
  `cwd` file. The `cwd` file value then applies as-is:
  `w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] && r=$w`.
- The remote branch keeps the single read-only `ssh -T` read.
  Its one-liner is `w=\$(cat \"$rp/cwd\" 2>/dev/null);
  [ -d \"\$w\" ] || w=\$(dirname \"\$(dirname \"$rp\")\");
  printf \"%s\" \"\$w\"`.
- open and open_v hand the TUI the absolute session dir `$rp`,
  not a bare name. The TUI uses an explicit session dir as-is
  (kernel issue 31), so the pane and fork tiers carry `$rp`.
  The remote tiers carry the remote session dir the same way.
- open_dir, open_dir_tmux_h and open_dir_tmux_v only cd and
  exec the shell. They change only in the resolution block.

Files: `rushi-sessions.toml` (the five commands, their comment
blocks and descriptions), `rushi-sessions-channel.nix` (the
byte-for-byte mirror), `bin/rushi-sessions/tests/toml_channel.rs`
(the decision 52 pins move to the new semantics: the `cwd` file
read, the as-is use, the dirname fallback, the no-walk-up
negative, and the session-dir handoff in open and open_v), and
`docs/television-integration.md` (the walk-up wording in three
places, plus a new item 70 that supersedes the item 52 walk-up
wording), and the README's devShell line (the gate keys off the
working dir).

Verification: 139 tests green. The TOML and the Nix mirror
compare byte for byte across the full channel attrset. The
resolution block ran in a sandbox across four cases. The
worktree session lands in the worktree. The plain session lands
in the project root. The missing `cwd` file falls back to the
path rule. The stale `cwd` file falls back the same way. On the
real tree, `check-placeholder-hardcode` resolves to
`.worktree/code-check`, where the old walk-up gave the repo
root.


## 91. The events channel shows the collapsed branch view, and branch_view re-opens the picked parent branch (2026-10-26)

The user asked for three things in the `rushi-sessions-events`
channel. The branch connectors pointed the wrong way for the
newest-first listing (the `└─` ran up to the target row, but the
branch block sits above its target). The listing printed every
historical entry, including the abandoned branch spans that a
rewind left behind; the user wanted only the active branch plus
the history before each diverging node, the hidden spans folded
into counts and labels. And a fork-mode action that opens the
picked entry's parent branch in a nested tv, so the channel's
preview, refresh and the rewind/summarize/send_message actions
work inside that view; closing the nested tv restores the
original.

Decisions (all in the `rushi-sessions` binary, no new channel):

- Newest-first plus down-pointing connectors: the indent column
  now emits `┌─ ` at depth d (three spaces per level) and `┌+ `
  when the row hides a folded span; the trunk stays empty (the
  `+` marks the fold). `tree_depths` and the depth math are
  unchanged; only the glyph output changed.
- `rushi-sessions events` runs the collapsed view. The binary
  computes the active ranges from the rewind markers (the
  `rushi-common` math), keeps the active branch plus the trunk
  history, and folds every hidden span onto the row that owns
  it. One summary column carries the annotations: a `(Folded N)`
  count of the folded span, a `branched from #T` label where a
  hidden branch left the trunk, a `branched out to #M` label
  where a hidden branch leaves the shown span, and a trailing
  `K hidden` for the gap behind the newest row. The `#N`
  numbers are 0-based item numbers (the log seq minus 1). The
  `User+Assistant` view adds the rewind markers to the message
  rows so the annotations stay visible; markers were never
  `[masked]` and stay untagged.
- One new flag, `--branch SEQ`, carries both the cutoff and the
  focus of a focused view: the listing folds every span outside
  the focus and marks the focus row. The focus is the deepest
  block that contains the pick's branch point; a marker pick
  focuses its target, so the abandoned gap folds onto the
  marker row and the latest branch's user intention stays
  visible. An out-of-range focus prints
  `rushi-sessions: seq F is out of range (the log has L
  entries)` on stderr and exits 1. The stable kernel consumers
  (`numbered_events`, `event_at`, `log_active_ranges`) are
  unchanged.
- The `branch_view` action (bound to alt-b) is the fork-mode
  shell body. It guards the channel CWD (`pwd` empty aborts
  with a message) and the channel file
  (`~/.config/television/cable/rushi-sessions-events.toml`
  missing aborts). It builds a temp cable dir (`mktemp -d`):
  a copy of the channel file with the two source commands
  patched to the focused forms (`rushi-sessions events
  --branch <seq> --chat` and `--branch <seq>`), then mirrors
  the main channel's browse_events pane plumbing: the
  control-char `-F` pane lookup (the pane id and floating flag
  assembled without braces, so television's template pipeline
  sees one split token and no stray group). The nested tv runs
  `tv --cable-dir <tmp> rushi-sessions-events` from the
  session dir. The patched cable dir keeps both source views,
  so ctrl-s cycles the focused User+Assistant view and the
  focused All view. The channel's display template, preview
  and actions stay, so ctrl-r re-sources the view and the
  rewind/summarize/send_message actions work there. When tv
  runs inside tmux, the nested tv opens in a floating pane
  over the tv window: `tmux new-pane -f` in the multi-arg
  command form (the same idiom as the prompt actions'
  floating pane, the tmux 3.8+ tier), centered, 62% width and
  90% height, labeled `rushi branch view`. The pane shell
  deletes the temp cable dir when the nested tv exits; the
  pane dies with it, restoring the original view with no
  layout change. When the floating pane is unavailable (older
  tmux) the browse_events split below the newest pane is the
  fallback tier; outside tmux the nested tv runs in the
  fork's pane.

Updated `rushi-sessions-events.toml` (the header, the alt-b
binding and the action), its Nix mirror
`rushi-sessions-events-channel.nix`, and locked the action in
`tests/toml_channel.rs`. The events tests carry the user's
nine-entry example (the items 0-8 log with two rewinds) as a
fixture: the main view, the focused views and the out-of-range
error are asserted line by line.

Verification: 139 tests green. `nix eval` of the channel
attrset reproduces the repo TOML command byte for byte
(`untab` restores the real tab of the split token; the
multi-line `''` string keeps the trailing newline of the TOML
multi-line value). The action body parses (`sh -n` on the
decoded inner script) and ran end to end in a live tmux 3.8
server: the floating pane landed over the tv window, the
nested tv kept both focused source views under the ctrl-s
cycle, and closing it deleted the temp cable dir and restored
the original view. The live session e2e (alt-b on a marker
row, rewind and summarize in the nested view, ctrl-r
refresh, esc to close the floating pane and restore the
original) passes with the deployed binary.

Fix (same day, same entry): the first commit ran the nested
tv through television's `--source-command` override. The
override replaces the channel's source commands with one, so
the nested view lost the ctrl-s switch between the
User+Assistant and All views. The patched temp cable dir
above restores both focused views; the pane shell's cleanup
of the temp dir came with it.

Design revision (same day, same entry): the user judged the
patched temp cable dir overcomplicating. The nested tv now
runs a dedicated sibling channel:
`rushi-sessions-events-branch`. The home-manager module
registers it next to the events channel, and the flake
builds its data file. Its two source views expand the
picked seq at run time. The branch_view body hands the seq
over as the `RUSHI_BRANCH_SEQ` env var. The channel's
source commands run `sh -c 'rushi-sessions events
--branch "$RUSHI_BRANCH_SEQ" [--chat]'`. The static channel
keeps both focused views and the ctrl-s cycle without a
temp dir, a sed patch or cleanup. The body guards for the
channel's file in the cable dir and aborts with a message
when it is missing. A manual launch without the env var
fails at the binary level (an empty seq is out of range).

Verified live in tmux 3.8: the floating pane opened the
branch channel. The focused view showed the `(Folded N)`
and branch annotations. The ctrl-s cycled both views, and
closing the nested tv restored the original view.

## 92. The log-appender name tracks the kernel's 58 rename: `rushi-log`, legacy `log` (2026-10-26)

The kernel's 58 (closes 57) renames the log-appender binary from
`log` to `rushi-log`: the reserved buildEnv name of 57. No
`bin/log` alias: the builder drops the alias too. Kernel builds
older than 0.2.0 still ship `log`, so a deployed host can carry
either generation of the kernel.

Decision (the branch's `kernel_launch` and `kernel_bin`; the 90
mechanism is unchanged):

- The log-appender launch resolves a name list, new kernel first:
  `["rushi-log", "log"]`. The gated devShell probe and the
  PATH-sibling fallback walk the list in turn; the first resolvable
  name wins the launch. The new-kernel host resolves `rushi-log`,
  the older-kernel host resolves the legacy `log`. No kernel
  version check: the resolution is the version check.
- The `summarize` call is unchanged: `compact` keeps its name in
  the 58 rename. The name list is the log appender's alone.

Verification: the launch worlds are unit-tested on the name list
(the gated probe hits the first seeded name, the unresolvable gate
falls back on the list). The integration stubs carry both
appender names: with both stubs the new name wins the marker
append, without the `rushi-log` stub the legacy `log` stub runs.
139 tests green. The `nix build` of the three channel derivations
fails on a co-resident uncommitted module (`branchview`, declared
in `main.rs` but untracked, outside `cleanSource`'s git filter):
the build is clean for this change's own hunks and re-runs once
the co-resident work is tracked.

## 90. The kernel bin launch keeps the devShell only when the bin resolves from the devShell PATH (2026-10-26)

The 88 kernel-bin extension launched `rewind` and `summarize` as a
bare `nix develop -c <bin>` in the session's project dir, on the
premise that a binary the devShell does not ship falls back to the
inherited PATH entry. For the kernel bins that premise is false on
the host that matters: `log` and `compact` are siblings of the
kernel's `rushi` in the package's one bin dir, and the per-user
symlink farm does not expose them as bare names. A project whose
devShell ships no kernel bin (a flake without a devShell, or a
devShell that omits the kernel package) leaves a bare `log`
unfindable on the devShell PATH and on the inherited PATH alike.
The bare launch dies with a 127, the marker never lands, and
`rewind` is broken. `summarize` survives only when the farm does
expose `compact` on the inherited PATH.

Decision (the branch's `kernel_launch`; the 88 gate and the devshell
module are unchanged):

- With the gate, `kernel_launch` keeps the `nix develop -c <bin>`
  in the flake dir only when `<bin>` resolves from the devShell
  PATH. The probe is the cached `devshell::resolve(dir, bin)` of
  89: its materialize `nix develop -c bash -c 'command -v <bin>'`
  searches the devShell PATH including the inherited PATH, and a
  hit is cached per flake generation.
- A resolve miss (the devShell and the inherited PATH both lack
  the name, or the bounded materialize times out) falls back to
  the PATH sibling of the resolved `rushi` with the caller's
  working dir, the pre-flake behavior of 87. The miss is not
  cached: a repeated manual action re-probes, and the TTL of a
  later hit still bounds the inherited-PATH staleness.
- The fallback sibling search follows the resolved `rushi` entry
  to the end of its symlink chain (the per-user farm and the nix
  profile entries are symlinks into the store's real bin dir, the
  one dir that carries the sibling). The farm does not expose a
  bare `log`, so a chain-less search on the farm dir missed the
  sibling even before 88.
- A resolve hit changes nothing about the launch: the `nix
  develop -c <bin>` in the flake dir and the full devShell
  environment of 88. The resolved store path is the resolvability
  proof only; it is not exec'd, unlike the 89 preview probe.

The kernel-side twin is the kernel issue 57: the nixpkgs buildEnv
builder skips the reserved name `log`, so a buildEnv-merged devShell
ships 16 of the 17 kernel bins. The bare `log` is absent even when
the devShell carries the kernel package. A 57 resolution changes
which world the probe lands in (a workaround restores the devShell
hit, a rename adds a new name to the channel's kernel bin names),
but it does not change this fallback. The no-devShell world (this
project) sits outside 57's scope entirely. No host where the kernel
is reachable only through the dropped devShell merge is fixed by
either side alone.

Verification: `kernel_launch` is unit-tested in the three worlds:
the gated launch with a seeded fresh cache entry (no nix call),
the gated fall-back on a bare flake whose materialize fails in
under a second, and the no-gate PATH sibling. Against the repro
project (a flake without a devShell, a farm that exposes
`compact` but no bare `log`), the pre-fix rewind died with a 127
from `nix develop -c log`; the fixed rewind appends the marker
through the PATH sibling of the resolved `rushi`. The `summarize`
`compact` launch keeps the gated form on that project (the bare
`compact` resolves from the inherited farm PATH through the
devShell PATH). 138 tests green; `nix build` of the three channel
derivations clean.

## 89. The preview devShell probe caches the binary's store path by generation (2026-10-24)

The 88 extension made the preview card's `rushi config`
context-window probe run `nix develop -c rushi config` in the
session's project dir. `nix develop` is heavy: a cold start is
about 18s, a warm run about 0.9s, and the devShell environment
cache cools back to about 18s within ~20s of idle. television
re-fetches the preview on redraw and select, so nearly every
fetch re-paid a cooled `nix develop` and froze the panel for
about 18-20s. That is the preview hang.

The devShell's binaries are content-addressed store paths that
change only when the project flake changes. Instead of re-running
the probe on every call, the binary's store path is resolved once
per devShell generation, cached with a generation fingerprint,
and exec'd directly on later calls. The heavy materialize runs
only when the generation is out of date.

Decision (new `devshell` module, decision 88 gate unchanged):

- `devshell::resolve(dir, bin)` returns the store path of `bin`
  in `dir`'s devShell. A fresh cache entry is returned without
  touching nix; otherwise the path is materialized once and
  cached. There is no env var: the mechanism is the cache plus a
  bounded materialize, nothing else to flip.
- Materialize: `nix develop -c bash -c 'command -v <bin>'` in
  the project dir, bounded by a 60s wall-clock cap so a heavy
  flake build cannot block the caller. The printed path is the
  devShell's content-addressed `bin/<bin>`.
- Cache file: `$XDG_CACHE_HOME` (else `~/.cache`, else the temp
  dir) under `rushi-sessions/devshell/<hash(project)>/<bin>`:
  `{ path, gen, ts }`.
- Generation fingerprint: the `flake.nix` and `flake.lock` mtimes
  plus the host arch. A flake edit re-materializes immediately.
- The entry is fresh when the generation matches, the stored path
  still exists in the store, and it is younger than a 30m TTL.
  The TTL bounds staleness of the context-window value, which
  also tracks the project config and model that can change
  without touching the flake files.
- `usage::context_window`: the gated dir resolves the devShell
  `rushi` and execs `<path> config` directly, then parses the
  context window. A resolve that fails falls back to the PATH
  `rushi config` (the pre-88 probe).

Verification: the devShell module is unit-tested (the hash, the
freshness check across generation and TTL, the materialize argv,
the bounded timeout, a fresh cache entry served without nix).
The probe routing gate is unit-tested. Against the repro session
in a flake project, a cold fetch materialized in ~0.9s (warm
nix; ~18s on a cold store, one-time) and filled the cache, and
every later fetch exec'd the cached store path in ~33ms with no
nix call. The card's usage line read the project's own 262144
context window, confirming the devShell `rushi` is probed. 137
tests green; `nix build` of the three channel derivations clean.

## 88. The devShell gate is the plain `flake.nix` + nix-on-PATH check, and it extends to remote rows, the kernel bins, and `rushi config` (2026-10-24)

The four-gate resolver of 87 was over-defensive. The opt-out env
var is an ad-hoc escape hatch, the `--ignore-environment` probe
adds a nix invocation to every action, and the absolute nix path
baked into the pane command is a moving target (a store GC or a
profile update invalidates it). All three are unnecessary: a plain
`nix develop` inherits the environment, so the devShell packages
shadow the inherited PATH, and a binary the devShell does not ship
falls back to the inherited PATH entry. The fallback is the desired
behavior, and it makes the probe and the opt-out pointless.

Decision:

- The gate is two cheap checks: `<project>/flake.nix` exists and
  `nix` is on PATH. No env var, no probe, no absolute nix path.
  The pane command and the detached step run a bare
  `nix develop -c <bin>` from the project dir; nix resolves from
  the same PATH the step already needs.
- The `open`/`open_v` one-liner: `tu=rushi-tui; if [ -f
  "$r/flake.nix" ] && command -v nix >/dev/null 2>&1; then
  tu="nix develop -c rushi-tui"; fi`.
- Remote rows get the same gate over one read-only `ssh -T`
  call: `test -f "$r/flake.nix" && command -v nix` on the remote
  host. The remote pane or fork ssh then runs
  `cd $r && nix develop -c rushi-tui NAME` (the devShell of the
  remote flake, when the remote project ships one). The binary
  create/send step builds the same command for the remote detached
  call.
- `rewind`/`summarize`: the `log`/`compact` kernel bins of the
  session's project dir launch as `nix develop -c <bin>` in that
  dir when the gate holds (the launch dir is the flake dir);
  otherwise the PATH sibling of the resolved `rushi`, the
  pre-flake behavior.
- The preview card's `rushi config` context-window probe: the
  project dir of the session's recorded working dir carries a
  flake and nix is on PATH: `nix develop -c rushi config` in that
  dir, so the project's own config tier is probed. Otherwise the
  PATH `rushi config`.

Verification: the gate and the launch builders are unit-tested
(the local gate, the remote gate command string, the remote run
command with and without the devShell launch, the kernel launch
argv, the `rushi config` probe command). The one-liners pass
`sh -n` after extraction. The toml mirror of the open/open_v
commands decodes to the nix content. A stub harness ran the
deployed `open` one-liner against fake `nix`, `rushi-tui` and
`ssh`: the local flake+nix row launched `nix develop -c rushi-tui`;
the flake-less and the no-nix rows kept the plain PATH TUI; the
remote flake+nix row opened the pane over `ssh -t` with
`nix develop -c rushi-tui` on the remote host, and the remote
flake-less row kept the plain remote TUI. `nix build` of the three
channel derivations is clean.

## 87. The open/send/new_session actions resolve the project flake's devShell (2026-10-24)


The `open`/`open_v` actions run `rushi-tui` from the session's
project dir, and `send_message`/`new_session` run a detached
`rushi run` in the same dir. Both resolved the binary on the global
PATH, so a project's own `rushi` build (the customized system
prompt, the project hooks, the pinned kernel/TUI pair) was ignored:
the session was always directed by the global binary. A project like
chemrust-hamiltonian ships its configured build in its flake
devShell (`rushi` + `rushi-tui` + `rushi-goal` in one package).

Decision:

- Local rows only. When the session's project dir carries a flake
  whose devShell ships the binary, the action runs it in that
  devShell: `open`/`open_v` run `nix develop -c rushi-tui NAME`
  from the project dir; `send_message`/`new_session` run a detached
  `setsid nix develop -c rushi run ...` there.
- Four gates, all of which must hold; any failure keeps the
  pre-change PATH behavior: (1) `RUSHI_SESSIONS_NIX_DEV` is not
  `0`; (2) `nix` is on PATH; (3) `<project>/flake.nix` exists;
  (4) the probe `nix develop --ignore-environment -c sh -c
  "command -v <bin>"` in the project dir exits 0. The ignored
  environment keeps an inherited PATH entry from faking a hit: a
  flake without the binary in its devShell fails the probe.
- The probe runs once, at action start: the shell one-liner runs
  it in the fork before the split; the binary runs it in stage 1
  before the editor opens. The later `nix develop` of the step is
  fast after the probe warmed the store.
- The absolute nix path (resolved in the fork) is baked into the
  pane command, so a tmux server whose own PATH lacks nix still
  launches the step; the TUI itself resolves from the devShell
  PATH. `nix develop` passes its args verbatim, so the
  message-mode `--` escape and `--sessions-root` survive.
- The open/open_v logic lives in the channel one-liner (the
  `tu`/`nu` variables); the send/create logic lives in the binary
  (`nix_dev` on the prompt args, `rushi_run_tail`).

Verification: 128 tests green (the new tail/gate/probe/args-file
unit tests plus the `toml_channel` pins of the one-liner gates).
A stub harness ran the committed `open` one-liner against a fake
`nix` and `rushi-tui`: the devShell launch on the flake+devShell
hit, the PATH fallback on every gate failure. On the real
chemrust-hamiltonian project, `nix develop -c rushi --version`
runs the devShell build.

Out of scope (as written): remote rows, the `rewind`/`summarize`
kernel binaries (`log`, `compact`), and the preview card's `rushi
config` context-window probe. Decision 88 extends the devShell to
all three and replaces the four-gate resolver with the plain gate.
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
