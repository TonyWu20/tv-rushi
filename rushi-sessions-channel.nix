# The rushi-sessions channel as a Nix value (a TOML-serializable attrset).
#
# The `rushi-sessions` home-manager module (home-module.nix) sets this attrset
# at `programs.television.channels.rushi-sessions`. The home-manager
# television module then serializes it to
# ~/.config/television/cable/rushi-sessions.toml.
#
# The source and preview commands run the `rushi-sessions` binary, which this
# flake builds (packages.<system>.rushi-sessions) and which the home module
# adds to `home.packages`. The binary hard-depends on `fd` for the directory
# walk and on `bat` optionally for preview coloring.
#
# The source command is a list of two named commands. television runs the
# first on startup and cycles between them on `cycle_sources` (Ctrl+S):
# "All" lists every found session, "Active" the live-loop ones only.

let
  tab = "\t";
  untab = s: builtins.replaceStrings [ "@TAB@" ] [ tab ] s;

  # Two source commands for the channel. television cycles between them with
  # the `cycle_sources` keybinding (default: Ctrl+S): only the first one
  # runs on startup. "All" lists every found session; "Active" keeps the
  # sessions whose loop pid is alive (the binary's `--active-only` filter).
  sourceCommands = [
    {
      name = "All";
      run = "rushi-sessions source";
    }
    {
      name = "Active";
      run = "rushi-sessions source --active-only";
    }
  ];

  previewCommand = untab ''
    rushi-sessions preview '{split:@TAB@:0}' '{split:@TAB@:1}' '{split:@TAB@:2}' '{split:@TAB@:3}' '{split:@TAB@:4}' '{split:@TAB@:5}' '{split:@TAB@:6}'
  '';
in
{
  metadata = {
    name = "rushi-sessions";
    description = "Peek at rushi sessions: status, loop phase, last activity";
    # `fd` is a hard dependency of the `rushi-sessions` binary. `bat` is
    # optional; the preview falls back to plain TOML when it is absent.
    # `tmux` is optional; open uses it only when tv runs inside a tmux
    # session (it detects $TMUX and falls back to the plain fork).
    # (The interactive actions call binaries resolved on PATH, which are
    # not listed as core requirements: `rushi` and `rushi-tui` for
    # send_message/open, and `tmux` for open. send_message uses `mktemp`,
    # `stty`, `setsid`, `tr` and $EDITOR (fallback `nvim`), all in the
    # standard Unix base.)
    requirements = [
      "fd"
      "rushi-sessions"
    ];
  };

  source = {
    shell = "bash";
    # A list of named source commands. television runs the first one on
    # startup and cycles to the next on `cycle_sources` (Ctrl+S by
    # default). "All" -> "Active" -> "All", so each press swaps the list
    # between every found session and the live-loop sessions only.
    command = sourceCommands;
    display = "[{split:\t:0}] {split:\t:2}/{split:\t:1} [{split:\t:3}] {split:\t:4}";
    output = "{split:\t:6}";
    frecency = false;
  };

  preview = {
    shell = "bash";
    cached = true;
    command = previewCommand;
  };

  ui = {
    preview_panel = {
      size = 75;
      word_wrap = true;
    };
  };

  keybindings = {
    "ctrl-e" = "actions:open";
    "alt-o" = "actions:open_dir";
    "ctrl-t" = "actions:send_message";
    "alt-k" = "actions:kill";
  };

  actions.open = {
    # The TUI resolves a bare session name against a *relative*
    # sessions_root, so it must start in the directory that contains the
    # sessions tree: dirname(dirname(session_dir)). The session's tool
    # cwd file is where tools run, not where the TUI resolves the session,
    # so it is deliberately NOT used as the launch dir.
    #
    # tv forks this action, so the fork inherits the pane environment.
    # When tv runs inside tmux, TMUX is set: create a new pane in the
    # current window, start it in the repo dir (-c), and run rushi-tui
    # there. The pane closes when rushi-tui exits. Outside tmux, or when
    # the tmux binary is missing, fall back to the original behavior: cd
    # to the sessions root and run rushi-tui in this fork.
    #
    # The shell body stays brace-free: television's string pipeline
    # treats a stray {group} as a template token and then leaves the
    # split placeholder unsubstituted. That is why the check reads
    # plain $TMUX instead of ${TMUX:-}.
    description = "Open this session in rushi-tui. Inside tmux, a new pane in the current window runs it from the session's repo dir. Outside tmux, it runs in this fork.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; n=$(basename "$s"); r=$(dirname "$(dirname "$s")"); t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -c "$r" "rushi-tui \"$n\""; else cd "$r" && rushi-tui "$n"; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_dir = {
    # Exit tv and land in a shell at the session's project dir: the
    # directory that contains the sessions tree,
    # dirname(dirname(session_dir)). Uses mode = "execute" so tv quits
    # and the command takes over the terminal; a fork would cd in a dead
    # child and tv would just resume. Falls back to bash when $SHELL is
    # unset. The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them.
    description = "Exit tv and cd into the session's project dir; a $SHELL shell (bash fallback) takes over the terminal";
    shell = "bash";
    mode = "execute";
    command = untab ''
      sh -c 's="$1"; r=$(dirname "$(dirname "$s")"); sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; cd "$r" && exec "$sh2"' sh '{split:@TAB@:6}'
    '';
  };

  actions.send_message = {
    description = "Open $EDITOR (falling back to nvim) on a message file, then send the result to the session with `rushi run <session> <msg>`: it starts the loop detached when the session is idle, and appends with `--no-run` when the loop is live. An empty message cancels.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; p=$(cat "$s/loop.pid" 2>/dev/null); live=no; [ -n "$p" ] && kill -0 "$p" 2>/dev/null && live=yes; d=$TMPDIR; [ -n "$d" ] || d=/tmp; f=$(mktemp "$d/tv-rushi-msg.XXXXXX"); st=$(stty -g 2>/dev/null); ed=$EDITOR; [ -n "$ed" ] || ed=nvim; $ed "$f"; [ -n "$st" ] && stty "$st" 2>/dev/null || stty raw -echo 2>/dev/null; m=$(cat "$f" 2>/dev/null); rm -f "$f"; if [ -z "$(printf "%s" "$m" | tr -d "[:space:]")" ]; then echo "No message entered, nothing sent."; exit 0; fi; if [ "$live" = yes ]; then rushi run "$s" "$m" --no-run; else setsid rushi run "$s" "$m" </dev/null >/dev/null 2>&1 & fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.kill = {
    description = "Stop the session loop (SIGTERM to the loop pid)";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 'p=$(cat "$1/loop.pid" 2>/dev/null); if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then kill -TERM "$p"; echo "sent SIGTERM to $p ("$(basename "$1")")"; else echo "no live loop for "$(basename "$1")" (stale or absent pid)"; fi' sh '{split:@TAB@:6}'
    '';
  };
}
