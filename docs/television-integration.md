# television-rushi channel — design and decision log

`rushi-sessions.toml` is a [television](https://github.com/alexpasmantier/television)
channel. It gives a quick-peek window over rushi sessions. It lists sessions
with status. It shows a status card and recent events in the preview panel.
It hands off to `rushi-tui` for full interaction.

## Decisions (user, 2026-09-30)

1. Scan scope: recursive under the CWD, or under a directory passed to
   `tv` (its `[PATH]` argument), or the full disk. `tv rushi-sessions DIR`
   chdirs into DIR before the source command runs. The source then scans
   `.` recursively. Omitting DIR scans the CWD. Passing `/` scans the full
   disk.
2. Distribution: the repo copy is the source of truth. Install it with
   `cp rushi-sessions.toml ~/.config/television/cable/`, or let
   nixos-config install it from this repo's flake (see Install).
3. The list shows the last `loop_phase` (wait/tools/...) per session.
4. Actions: `open` (rushi-tui), `tail` (follow events), `kill` (SIGTERM the
   loop pid). Full interaction stays in rushi-tui. This channel is a peek.
5. Design notes live in this repo's `docs/`.
6. Preview: a structured TOML card, syntax-highlighted by `bat` when
   available. It shows the last user and last assistant messages. `bat`
   is optional. The card prints plain without it.

## Decisions (user, 2026-10-01)

7. The backend is now the `rushi-sessions` Rust binary. The `source` and
   `preview` commands run it. It replaces the inline `python3` scripts.
8. The binary hard-depends on `fd` for the directory walk. The channel
   requirements are `fd` and `rushi-sessions`. `python3` is no longer
   needed.
9. This repo owns the channel and its backend. The kernel no longer carries
   the TV channel.
10. This repo provides a home-manager module
   (`homeManagerModules.<system>.default`). It sets
   `programs.television.channels."rushi-sessions"` and adds the binary to
   `home.packages`. Importing the module is the whole integration.
11. Added `aarch64-darwin` support: `packages` and `homeManagerModules`
   expose `x86_64-linux`, `aarch64-linux`, and `aarch64-darwin`.
   `homeManagerModules` stays keyed by system. There is no bare
   `default`. A bare default would pick one system's binary and would
   install the wrong binary elsewhere. The Mac config uses
   `homeManagerModules."aarch64-darwin".default`.
12. The `tail` action was not useful. Replaced it with `send_message`
   (bound to `ctrl-t`): it prompts for a message on the terminal, then runs
   `rushi run <session> <msg>`. When the session is idle it starts the loop.
   When the loop is live it appends with `--no-run` (lock-free, returns
   immediately). Empty input cancels. This makes the channel a peek plus a
   one-line poke, not just read-only.
13. The idle branch of `send_message` now detaches the loop. It runs
   `rushi run` under `setsid`, in the background, with stdio redirected to
   `/dev/null`. tv resumes at once. The loop registers itself in `loop.pid`,
   so the next send takes the live path.
14. The prompt for `send_message` is now `$EDITOR` (fallback `nvim`) on a
   `mktemp` file. It is not a new ratatui binary, and not the earlier
   in-terminal read loop. No new dependencies. The user edits with their own
   editor. The stty save/restore now brackets the editor launch. An empty or
   whitespace-only file cancels the send.
 15. The command avoids shell `${...}` expansions. Nix string interpolation
    consumes a `$` before a `{` inside the `''...''` body, which corrupted
    the generated command. The shipped script uses plain `$VAR` reads with
    explicit fallbacks (`d=$TMPDIR; [ -n "$d" ] || d=/tmp`).
  16. Added `open_dir` (bound to `ctrl-d`), requested by the user. It exits
     tv and cds into the session's project dir (`dirname(dirname(session_dir))`),
     then execs `$SHELL` (falling back to `bash`) so the shell owns the
     terminal. It runs in `execute` mode: a forked `cd` dies with the child,
     so only `execute` hands the terminal over. `open` stays as is.

## Decisions (user, 2026-10-04)

17. The source command is now a list of two named commands. `All` runs
    `rushi-sessions source`. `Active` runs `rushi-sessions source
    --active-only`. television runs `All` on startup. The `cycle_sources`
    key (default `ctrl-s`) switches between the two. Each press toggles
    the list between all found sessions and live-loop sessions only. The
    binary flag keeps the filter in the backend. The channel adds no pipe.
18. The `open` action now detects tmux. When tv runs inside a tmux pane,
    the action's fork inherits `$TMUX`. It creates a new pane in the
    current window, starts it in the session's repo dir, and runs
    `rushi-tui <session>` in that pane. The repo dir is
    `dirname(dirname(session_dir))`, passed to `tmux split-window -c`.
    The new pane closes when rushi-tui exits. Outside tmux, or without
    the `tmux` binary on PATH, it keeps the old behavior. It runs `cd`
    to the sessions root, then rushi-tui in the fork. The shell body
    stays brace-free. A stray brace group would break the
    `{split:\t:6}` template.
19. `alt-o` is the correct keybinding for `open_dir`. The TOML file had
    it as `alt-d`. The docs had it as `ctrl-d`. Both are now aligned
    with the Nix file.

## Decisions (user, 2026-10-05)

20. The `open` pane is now a vertical split. The new pane sits to the
    right of tv, side by side. The command passes `-h` to
    `tmux split-window`. That flag means the new pane goes to the
    right of the current one, despite its name.
21. Added a second open action, `open_v`, bound to `ctrl-v`. It
    opens the session with `tmux split-window -v`, so the new pane
    stacks below tv. The user picks the split direction on purpose.
    `open` keeps `-h`. Outside tmux, both run the plain fork.
22. The `source` subcommand now takes any number of roots:
    `rushi-sessions source ROOT...`. Without roots it keeps the old
    behavior (the CWD is the single root). One `fd` call takes all
    roots as start points. Overlapping roots (one inside another, or
    the same dir spelled twice) dedupe by canonical path. A missing
    root is skipped with a stderr warning; the other roots still
    scan. The roots come from the channel's source commands (see 23).
23. The scan roots are a home-manager option, not channel data.
    `programs."rushi-sessions".sourceRoots` is a list of strings,
    default empty. It feeds the `sourceRoots` argument of
    `rushi-sessions-channel.nix` (now a function, not a bare attrset).
    An empty list runs `rushi-sessions source` with no roots (the CWD).
    A non-empty list appends the roots to both source commands. The
    unquoted join keeps shell expansion of `~/...` roots. The shipped
    TOML manual copy stays rootless. Its `run` lines take roots by
    hand. The option keeps each user's trees out of the channel data.
24. `tv [PATH]` cannot override `sourceRoots` on the same channel.
    television 0.15.9 exposes the argument to no channel surface. It
    only chdirs the process. A source read and an env dump of a probe
    channel found no env var, token, or config field for it. A DIR-
    less launch from any dir is indistinguishable from `tv DIR`. The
    module therefore registers a second channel `rushi-sessions-cwd`
    (the `programs."rushi-sessions".cwdChannel` option, default
    enabled). It scans the CWD only, so `tv rushi-sessions-cwd DIR`
    is the override. The main channel keeps its roots.
25. The user preferred the split the other way. The main channel
    `rushi-sessions` keeps the CWD behavior: no roots, it scans the
    CWD (the `tv [PATH]` argument). The configured trees move to a
    `rushi-sessions-all` channel fed by `sourceRoots` (the
    `programs."rushi-sessions".allChannel` option, default enabled,
    registered only when `sourceRoots` is non-empty). Supersedes the
    channel split of 24.
26. `open_dir` now detects tmux, like `open`. It moves from `execute`
    to `fork` mode: inside tmux, the fork runs `tmux split-window -h -c`
    with the session's repo dir and starts `$SHELL` (fallback `bash`)
    in the new pane, to the right of tv. tv stays open, and the pane
    closes when the shell exits. Outside tmux, the fork cds to the repo
    dir and execs the shell; tv resumes when the shell exits, so tv no
    longer quits. The mode move is required: `execute` would quit tv
    before the split. The split direction is `-h`, matching `open`.
    The `-h` choice and the non-tmux resume behavior are implementation
    defaults, pending user confirmation. Superseded by 27.
27. `open_dir` keeps its original `execute` behavior. Two new fork
    actions do the tmux pane split instead. `open_dir_tmux_h` opens
    a pane to the right of tv (-h). `open_dir_tmux_v` opens a pane
    below tv (-v). Each pane starts `$SHELL` (fallback `bash`) in the
    session's repo dir. tv stays open. The pane closes when the shell
    exits. Outside tmux, each action falls back to the plain fork: cd
    to the repo dir, exec the shell, tv resumes. The alt-d and alt-v
    keybindings and the plain-fork fallback are implementation
    defaults, pending user confirmation. Supersedes 26.

## Diagnosis: tmux residue and post-exit recovery (user report 2026-10-04)

The user reported two symptoms for `open` inside a tmux pane and asked
who is to blame. Findings below come from source reading plus live tmux
reproductions (tmux 3.7b, television 0.15.9, real `tv` and `rushi-tui`
in detached scratch sessions). `tmux capture-pane` was the ground truth.

Symptom 1: residue of the tv UI inside `rushi-tui` (confirmed).

- tv runs fullscreen on tmux's alternate screen. tmux saves the shell's
  main screen and clears the alt screen on entry.
- The forked `open` action runs `sh -c ... rushi-tui` while tv is still on
  that alt screen. tv keeps its own UI on that screen. It clears only on
  resume, after the child exits. The path is
  `run_external_command_fork`: Pause, `child.wait()`, Resume. The
  `RenderingTask::Resume` handler in `television` `render.rs` calls
  `tui.enter()`, which re-sends `1049h` and clears.
- `rushi-tui`'s own `EnterAlternateScreen` (`CSI ?1049h`) is a no-op in
  tmux. `screen_alternate_on` in tmux 3.7b `screen.c` returns early when
  the pane is already in alternate mode. No re-save and no clear. On a
  direct terminal, xterm semantics clear the alt screen on entry. That is
  why the residue only appears under tmux.
- `rushi-tui` never clears on startup. `bin/tui/src/main.rs` has no
  `term.clear()` after `Terminal::new`. Clears exist only in the
  suspend-resume and exit paths. Its first ratatui frame is a diff
  against an empty internal buffer. Only the cells the UI paints get
  written. tv's leftover text stays visible in the unpainted cells, such
  as the one-column side margins and the transcript area.
- A pane resize clears the residue. SIGWINCH triggers ratatui
  `autoresize`, which calls `clear_viewport()`. That is the workaround
  the user found.
- Verified live: the residue reproduces with the real `tv` and the real
  `rushi-tui`. Adding one `term.clear()` after `Terminal::new` in
  `rushi-tui` removes it completely. That fix is `rushi-tui` commit
  `203e556`.

Blame split for symptom 1:

- tmux: the trigger. Nested `1049h` is a no-op under tmux 3.7b. A child
  started inside a tmux pane does not get a clean alternate screen.
- tv: the main cause. It leaves its own UI on the shared alt screen while
  the child runs. The child starts on a dirty screen.
- `rushi-tui`: the last line of defense. It owns the screen it draws
  into and should clear it on entry. The cleanest self-contained fix is
  the `rushi-tui` clear on startup.

Symptom 2: long blank after a long `rushi-tui` run (not reproduced).

- The user reports the pane stays blank for a long time after a long
  `rushi-tui` run. The delay seems proportional to the run length.
- Measured in this environment, not reproduced. Runs of 4, 9, and 14
  minutes were tested, with idle and streaming event feeds and one
  mid-run pane resize. The TUI exit path took about 2.1 s. That is the
  bounded extension-group stop grace. The tv UI was back within 1 s of
  the child exit. The blank window was under 1 s in every case.
- The resume paths are all bounded in code. tv renderer Resume:
  re-enter plus clear, immediate. The next `Render` tick lands within
  `RENDERING_INTERVAL` = 25 ticks at 50 Hz, so 0.5 s. tmux alternate
  enter/exit is O(viewport), no time-proportional cost. tmux history is
  disabled while in the alt screen, so nothing accumulates over time.
  `history_size` stayed 0 across all long runs.
- Conclusion: no component here (tmux, tv, `rushi-tui`) explains a
  proportional delay. If the user still sees it, the cause is outside
  these three. Likely the outer terminal or client side during a long
  run. Or a much longer, hour-scale run.

Open question for the user: during a long blank, does
`tmux capture-pane -p -t <tv pane>` already show the tv UI? If yes, only
the user's terminal is stale. If no, the pane content is still blank.

Decisions (user, 2026-10-04):

- Commit the `rushi-tui` `term.clear()` fix only. Done as
  `rushi-tui` commit `203e556` (`fix(tui): clear the alternate
  screen on startup`).
- Do not file the upstream `tv` clear-before-fork issue. The
  `rushi-tui` fix covers the `open` action. No tmux action needed.
- Symptom 2 stays open. The user should capture
  `tmux capture-pane -p -t <tv pane>` during a long blank to
  separate server-side state from terminal-side state.

## How it works

- The source command is a list of two named commands. television runs
  `All` on startup. The `cycle_sources` key (default `ctrl-s`) switches
  to `Active`, and a second press goes back. `All` runs `rushi-sessions
  source` with the scan roots. `Active` adds the `--active-only`
  flag, and the list keeps live-loop sessions only. Forced
  `shell = "bash"`: the login shell is fish, which breaks shell
  scripts, so every command forces bash.
- The binary calls one `fd` walk to find `sessions/` dirs under the
  scan roots. The roots come from the home-manager option
  `programs."rushi-sessions".sourceRoots` (default empty). They feed
  the `rushi-sessions-all` channel. The main channel has no roots and
  scans the CWD. `fd` is a hard dependency of the channel.
- The main channel `rushi-sessions` always scans the CWD (the `tv
  [PATH]` argument), so `tv rushi-sessions DIR` scans DIR.
- A second channel `rushi-sessions-all` (module option
  `programs."rushi-sessions".allChannel`, default enabled) is
  registered when `sourceRoots` is non-empty. It scans the configured
  trees. A DIR argument does not reach it.
- The binary does the per-session work in process. It forks no subprocess
  per session. It skips hidden dirs and `target`, `.git`, `node_modules`,
  `scratch`, `.nix`. It emits one TSV line per session dir that has
  `events.jsonl`, `loop.pid`, or `cwd`.
  Fields: `status`, `name`, `repo`, `phase`, `last`, `epoch`, `abs-path`.
- `status`: `ACTIVE` when `loop.pid` names a live pid (`kill(pid, 0)`).
  Otherwise `IDLE`.
- `phase`: last `loop_phase` value in the tail 100KB of `events.jsonl`.
- `last` and `epoch`: mtime of `events.jsonl`. Fallback is `loop.pid`.
- Sort: ACTIVE first, then newest activity first. `frecency = false`.
- The source and preview commands are the two `rushi-sessions` subcommands.
  television runs every command string through its template engine. The
  `{split:\t:N}` tokens carry a real tab. The binary output supplies the
  seven tab-separated fields they split on.
- Preview output: a structured TOML card. It has a `[rushi-session]`
  header (name, repo, status, pid, phase, think, updated). It has a
  `[last-user-message]` and a `[last-assistant-message]`. It has a
  `[[recent-event]]` array of the last 5 meaningful events.
- The card is syntax-highlighted by `bat` when it is on PATH. The default
  theme is `Catppuccin Macchiato` to match the dark catppuccin tv theme.
  Set `RUSHI_PREVIEW_THEME` to override it. When `bat` is absent, the card
  prints as plain text.
- Hook plumbing with dict values is filtered out. `cached = true`, so the
  preview card re-renders only when the selected entry changes.
- Actions use `{split:\t:6}` (abs session path). All use `mode = "fork"`
  so tv resumes after each. `open_dir` is the exception: it runs in
  `execute` mode, so tv exits and the command takes over the terminal.
  - `ctrl-e` open: inside tmux, the fork runs `tmux split-window -h -c`.
    The new pane sits to the right of tv, in the same window. It starts
    in the directory that holds the sessions tree
    (`dirname(dirname(session_dir))`). It
    runs `rushi-tui <session>` in that pane. The pane closes on exit.
    Outside tmux, or without the `tmux` binary, the fork runs `cd` to
    that base dir, then rushi-tui, as before. The TUI resolves a bare
    name against a relative `sessions_root`, so the pane or fork must
    start in that base dir. The session's tool `cwd` file is where
    tools run, not where the TUI resolves the session, so it is not
    used as the launch dir.
  - `ctrl-v` open_v: same as open, but the new pane stacks below tv.
    It runs `tmux split-window -v -c` in the fork. The pane sits
    below the tv pane, in the same window. It starts in the same base
    dir and runs `rushi-tui <session>` there. Outside tmux, it is the
    plain fork, like open.
  - `alt-o` open_dir: exit tv and land in a shell at the session's
    project dir (`dirname(dirname(session_dir))`). The command cds there,
    then execs `$SHELL` (fallback `bash`). Type `exit` to return to the
    shell that launched tv. This mode matters: a forked `cd` dies with
    the child, so only `execute` mode can hand the terminal over.
  - `alt-d` open_dir_tmux_h: open a shell ($SHELL, fallback `bash`) at
    the session's project dir, in a tmux pane. The fork runs
    `tmux split-window -h -c`. The new pane sits to the right of tv,
    in the same window. It starts the shell in the repo dir. tv stays
    open. The pane closes when the shell exits. Outside tmux, the fork
    cds to the dir and execs the shell. tv resumes when the shell exits.
  - `alt-v` open_dir_tmux_v: same, but the new pane stacks below tv.
    It runs `tmux split-window -v -c`. The pane sits below the tv pane,
    in the same window. It starts the shell in the same repo dir.
    Outside tmux, it is the plain fork, like open_dir_tmux_h.
  - `ctrl-t` send_message: open `$EDITOR` (fallback `nvim`) on a `mktemp`
    file, then `rushi run <abs-session-dir> <msg>`. Idle: it starts the
    loop detached (`setsid`, stdio to `/dev/null`), so tv resumes at once.
    Live: it appends with `--no-run`. An empty file cancels. The command
    saves and restores the stty state around the editor launch.
    Two separate locks are involved. The loop holds an exclusive
    `.loop.lock` for its whole life. Log appends take only a brief
    `events.jsonl` line lock. So `--no-run` appends into a live session
    without touching `.loop.lock` and without writing `loop.pid`. The
    live loop's pid lock stays in place for the TUI to read.
  - `ctrl-shift-k` kill: SIGTERM the `loop.pid`. Prints "no live loop"
    when the pid is stale. A SIGTERMed loop restarts. State stays in the
    session log.

## Usage

```sh
tv rushi-sessions                    # scan CWD
tv rushi-sessions ~/programming      # scan a given dir
tv rushi-sessions ../                # relative dirs work too
tv rushi-sessions /                  # full disk (slow)
```

The main channel always scans the CWD (the `tv [PATH]` argument). To
watch a fixed set of trees, set the home-manager option
`programs."rushi-sessions".sourceRoots` (a list of directories,
default empty). It feeds the `rushi-sessions-all` channel (the
`allChannel` option, default enabled), registered only when the list
is non-empty. The binary alone
takes any number of roots:

```sh
rushi-sessions source            # scan the CWD
rushi-sessions source /export ~/programming
```

## Install / re-sync

### home-manager module (declarative)

This repo exposes a home-manager module:

```nix
inputs.tv-rushi.url = "github:TonyWu20/tv-rushi";
# ...
home-manager.users.tony = {
  # the host config must also load the television home-manager module
  homeModules = [ inputs.tv-rushi.homeManagerModules."x86_64-linux".default ];
  programs."rushi-sessions" = {
    enable = true;
    sourceRoots = [ ]; # empty: the source commands scan the CWD
  };
};
```

The module adds the `rushi-sessions` binary to `home.packages` and sets
`programs.television.channels."rushi-sessions"`. The television module
serializes that attrset to `~/.config/television/cable/rushi-sessions.toml`.
The `sourceRoots` option (default empty) feeds the
`rushi-sessions-all` channel, which scans the configured trees. It is
registered only when the list is non-empty (the `allChannel` option,
default enabled). The main channel always scans the CWD.

### Manual (copy)

```sh
cp rushi-sessions.toml ~/.config/television/cable/rushi-sessions.toml
```

The manual copy needs `fd` and the `rushi-sessions` binary on PATH. The
binary builds from this repo (`cargo build --release` or the flake).

### Nix flake (nixos-config, data file only)

This repo is a flake. It exposes the channel as a single-file package:

```sh
nix build .#rushi-sessions-channel   # -> a store path holding the TOML
nix build .#rushi-sessions           # -> the backend binary
```

nixos-config adds this repo as an input and installs the file with
home-manager:

```nix
home.file.".config/television/cable/rushi-sessions.toml".source =
  inputs.tv-rushi.packages."x86_64-linux".rushi-sessions-channel;
```

The input pins nixpkgs with `follows`, so it reuses the config's own
nixpkgs. Each activation links the store file into the cable dir. This
replaces the manual `cp`. The channel stays pure data in the store.

## Verified

- Source: 268 sessions from `~/programming` in 0.16s. ACTIVE lines sort
  first. Phases populate for live and idle sessions.
- Binary parity: `rushi-sessions source` matches the old `fd`+`python3`
  pipeline byte-for-byte (same 7-field TSV) at `~/` and at repo roots.
- Binary timing: about 0.10s at `~/` scale, beating the old pipeline
  (about 0.17s). The `fd` walk is the fast path.
- Preview: renders the card and filtered events for a live and an idle
  session.
- `tv rushi-sessions ~/programming` and `tv rushi-sessions ../` pty smoke
  tests: entries appear in about 0.2s. The preview renders. Esc exits.
  No errors.
- `open`, `send_message`, and `kill` were verified in pty tests using a
  stubbed `rushi` and `rushi-tui`.
- `send_message` (editor flow, pty test with a stubbed `$EDITOR` and a
  stubbed `rushi`): live branch passes `--no-run`. Idle branch starts the
  loop detached in its own session. A stale `loop.pid` is replaced by the
  new detached start. Empty or whitespace-only editor file cancels.
  Terminal state restored.
- `kill` was tested with a stale pid and a live pid in a fake session dir.
- The `--no-run` path was verified against the real `rushi` binary. It
  wrote one `user_message` event and left no `loop.pid`.
- `kill` also hit a real session by mistake during testing. The loop died
  and restarted. The session log stayed intact. The user restarted that
  loop by hand.
- Flake: `nix build .#rushi-sessions-channel` yields a store path that
  is byte-identical to the repo TOML. `nix build .#rushi-sessions` yields
  the backend binary.
- `--active-only`: a fixture tree with one live and two stale sessions
  prints only the live row. With zero live sessions it prints no rows and
  exits 0.
- Source cycling (tmux pty test, tv 0.15.9): the `All` view lists three
  fixture sessions. `ctrl-s` switches to `Active`, which keeps only the
  live session. A second `ctrl-s` returns to `All`. The header shows the
  source name and the `ctrl-s` hint.
- Multi-root source: `rushi-sessions source /export /home/tony/programming
  ~/Downloads` prints the sum of the per-root row counts (9 + 271 + 1 at
  first check). Nested or repeated roots dedupe to the same rows
  (9, not 18). A missing root prints one stderr warning and the other
  roots still scan. Without roots the CWD is the single root, as before.
- open_dir_tmux_h E2E (2026-10-05): scratch tmux session, scratch HOME
  points tv at the repo TOML. tv lists the fixture session. Press
  `alt-d`. tv keeps running in its own pane. A new pane appears to
  the right of tv.
- open_dir_tmux_h E2E, continued: that pane starts `$SHELL` in the
  session's repo dir. The pane cwd and live content confirm it.
  Type `exit` in the shell pane. The pane closes. The tv pane and
  its UI stay unchanged.
- open_dir_tmux_v E2E (2026-10-05): press `alt-v`. The new pane
  stacks below the tv pane. The pane sizes confirm it. Both panes
  keep the full window width, each half the height.
- open_dir execute mode (2026-10-05): press `alt-o`. tv quits. The
  same pane now runs `$SHELL` at the session's repo dir.
- open_dir_tmux_h/v non-tmux branch (2026-10-05): verified at the
  shell level. The fork cds to the repo dir and execs the shell. The
  parent, a tv-fork stand-in, resumes after the shell exits.
- `sourceRoots` option: full module evaluation (`lib.evalModules` with
  a stubbed `programs.television` option) with the default empty list
  yields the main channel rootless (`rushi-sessions source` and
  `rushi-sessions source --active-only`). With three roots, the
  `rushi-sessions-all` channel's `run` lines carry them. The
  generated `All` command ran under `bash -c` and listed sessions from
  all three trees.

- `rushi-sessions-all` channel: with the default empty list the module
  evaluation registers the main channel only. With three roots, both
  channels register: the main one rootless, and
  `rushi-sessions-all` with the roots in both `run` lines and
  `metadata.name` `rushi-sessions-all`. With `allChannel = false`
  the second channel is absent.
- `tv [PATH]` exposure (tv 0.15.9): a probe channel env dump with and
  without the DIR argument differs only in `PWD`. A television source
  read found the same: one `set_current_dir`, no env var, no config
  field, no template token for the argument.


- Session paths in the list are absolute. `send_message` and `kill`
  resolve them directly, from any CWD. `open` starts `rushi-tui` from
  the directory that holds the sessions tree. Inside tmux, it does so
  in a new pane to the right of tv. Outside tmux, it does so in the
  fork. The TUI resolves a bare name against a relative `sessions_root`,
  so the CWD must be that base dir.
- `kill` sends SIGTERM to the pid in `loop.pid`. A reused pid would hit
  the wrong process. The preview shows ACTIVE/IDLE first. The user
  confirms before pressing ctrl-shift-k.
- `send_message` on an idle session starts the loop detached, so tv
  resumes at once. The loop's stdout and stderr go to `/dev/null`, so
  watch progress in the preview panel. The loop writes its own `loop.pid`
  on start, so the next send takes the live path.
- `send_message` opens `$EDITOR` (fallback `nvim`) on a temp file. An empty
  or whitespace-only file cancels the send.
- Action command strings pass through television's string-pipeline templater.
  A literal shell brace group such as `{ echo x }` breaks the whole parse.
  television then falls back to raw pass-through and leaves the `{split:...}`
  placeholder un-substituted. The command reaches the shell with the literal
  placeholder text. Keep `sh -c` bodies brace-free (use `if/then/fi`).
- Full-disk scans are slow. Prefer a repo or `~/programming`.
