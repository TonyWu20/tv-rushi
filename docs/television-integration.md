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
2. Distribution: copy-install. The repo copy is the source of truth.
   `cp rushi-sessions.toml ~/.config/television/cable/` installs it.
3. The list shows the last `loop_phase` (wait/tools/...) per session.
4. Actions: `open` (rushi-tui), `tail` (follow events), `kill` (SIGTERM the
   loop pid). Full interaction stays in rushi-tui. This channel is a peek.
5. Design notes live in this repo's `docs/`.

## How it works

- Source command: one `python3` process, forced `shell = "bash"`. The
  login shell is fish, which breaks the script. One `fd` call finds
  `sessions/` dirs under the scan root (the `tv` CWD).
- Python does the per-session work in process. It forks no subprocess per
  session. It skips `target`, `.git`, `node_modules`, `scratch`. It
  emits one TSV line per session dir that has `events.jsonl`,
  `loop.pid`, or `cwd`.
  Fields: `status`, `name`, `repo`, `phase`, `last`, `epoch`, `abs-path`.
- `status`: `ACTIVE` when `loop.pid` names a live pid (`kill(pid, 0)`).
  Otherwise `IDLE`.
- `phase`: last `loop_phase` value in the tail 100KB of `events.jsonl`.
- `last` and `epoch`: mtime of `events.jsonl`. Fallback is `loop.pid`.
- Sort: ACTIVE first, then newest activity first. `frecency = false`.
- The source and preview commands are brace-free python. television runs
  every command string through its template engine. An unknown brace token
  falls back to raw substitution and corrupts the script. So both scripts
  avoid literal braces.
- Preview output: a status card and the last 8 meaningful events. Hook
  plumbing with dict values is filtered out. `cached = false` so live
  sessions refresh.
- Actions use `{split:\t:6}` (abs session path). All use `mode = "fork"`
  so tv resumes after each.
  - `ctrl-e` open: `cd` into the session's `cwd` file value. Fallback is
    the repo root derived from the path. Then run `rushi-tui <session>`.
  - `ctrl-t` tail: `tail -f events.jsonl`. Ctrl-C returns to tv.
  - `ctrl-k` kill: SIGTERM the `loop.pid`. Prints "no live loop" when the
    pid is stale. A SIGTERMed loop restarts. State stays in the session
    log.

## Usage

```sh
tv rushi-sessions                    # scan CWD
tv rushi-sessions ~/programming      # scan a given dir
tv rushi-sessions ../                # relative dirs work too
tv rushi-sessions /                  # full disk (slow)
```

## Install / re-sync

```sh
cp rushi-sessions.toml ~/.config/television/cable/rushi-sessions.toml
```

## Verified

- Source: 268 sessions from `~/programming` in 0.16s. ACTIVE lines sort
  first. Phases populate for live and idle sessions.
- Preview: renders the card and filtered events for a live and an idle
  session.
- `tv rushi-sessions ~/programming` and `tv rushi-sessions ../` pty smoke
  tests: entries appear in about 0.2s. The preview renders. Esc exits.
  No errors.
- `open` (stubbed `rushi-tui`), `tail`, and `kill` are verified.
  `kill` was tested with a stale pid and a live pid in a fake session dir.
- `kill` also hit a real session by mistake during testing. The loop died
  and restarted. The session log stayed intact. The user restarted that
  loop by hand.

## Caveats

- Session paths in the list are absolute. Actions resolve them directly.
  They work from any CWD.
- `kill` sends SIGTERM to the pid in `loop.pid`. A reused pid would hit
  the wrong process. The preview shows ACTIVE/IDLE first. The user
  confirms before pressing ctrl-k.
- Full-disk scans are slow. Prefer a repo or `~/programming`.
