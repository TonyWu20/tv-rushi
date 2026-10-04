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
  source`. `Active` adds the `--active-only` flag, and the list keeps
  live-loop sessions only. Forced `shell = "bash"`: the login shell is
  fish, which breaks shell scripts, so every command forces bash.
- The binary calls one `fd` walk to find `sessions/` dirs under the scan
  root (the CWD). `fd` is a hard dependency of the channel.
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
  - `ctrl-e` open: `cd` into the directory that holds the sessions tree,
    `dirname(dirname(session_dir))`, then run `rushi-tui <session>`. The
    TUI resolves a bare name against a relative `sessions_root`, so it
    must start in that base dir. The session's tool `cwd` file is where
    tools run, not where the TUI resolves the session, so it is not used
    as the launch dir.
  - `ctrl-d` open_dir: exit tv and land in a shell at the session's
    project dir (`dirname(dirname(session_dir))`). The command cds there,
    then execs `$SHELL` (fallback `bash`). Type `exit` to return to the
    shell that launched tv. This mode matters: a forked `cd` dies with
    the child, so only `execute` mode can hand the terminal over.
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

## Install / re-sync

### home-manager module (declarative)

This repo exposes a home-manager module:

```nix
inputs.tv-rushi.url = "github:TonyWu20/tv-rushi";
# ...
home-manager.users.tony = {
  # the host config must also load the television home-manager module
  homeModules = [ inputs.tv-rushi.homeManagerModules."x86_64-linux".default ];
  programs."rushi-sessions".enable = true;
};
```

The module adds the `rushi-sessions` binary to `home.packages` and sets
`programs.television.channels."rushi-sessions"`. The television module
serializes that attrset to `~/.config/television/cable/rushi-sessions.toml`.

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

## Caveats

- Session paths in the list are absolute. `send_message` and `kill`
  resolve them directly, from any CWD. `open` cd's into the
  directory that holds the sessions tree, then runs `rushi-tui` with
  the session name. The TUI resolves a bare name against a relative
  `sessions_root`, so the CWD must be that base dir.
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
