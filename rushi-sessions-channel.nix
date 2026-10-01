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

let
  tab = "\t";
  untab = s: builtins.replaceStrings [ "@TAB@" ] [ tab ] s;

  sourceCommand = "rushi-sessions source";

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
    requirements = [
      "fd"
      "rushi-sessions"
    ];
  };

  source = {
    shell = "bash";
    command = sourceCommand;
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
      size = 50;
      word_wrap = true;
    };
  };

  keybindings = {
    "ctrl-e" = "actions:open";
    "ctrl-t" = "actions:tail";
    "ctrl-shift-k" = "actions:kill";
  };

  actions.open = {
    description = "Open this session in rushi-tui (full interaction, forked)";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; n=$(basename "$s"); cdw=$(cat "$s/cwd" 2>/dev/null); if [ -z "$cdw" ] || [ ! -d "$cdw" ]; then cdw=$(dirname "$(dirname "$s")"); fi; cd "$cdw" && rushi-tui "$n"' sh '{split:@TAB@:6}'
    '';
  };

  actions.tail = {
    description = "Follow the session event log (Ctrl-C returns to tv)";
    shell = "bash";
    mode = "fork";
    command = "tail -f '{split:\t:6}/events.jsonl'";
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
