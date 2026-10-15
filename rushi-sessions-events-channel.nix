# The rushi-sessions-events channel as a Nix value (a
# TOML-serializable attrset).
#
# The `rushi-sessions` home-manager module (home-module.nix) sets this
# attrset at `programs.television.channels."rushi-sessions-events"`.
# The home-manager television module then serializes it to
# ~/.config/television/cable/rushi-sessions-events.toml.
#
# The channel fuzzy-searches one session's events.jsonl. It is launched
# by the `browse_events` action of the rushi-sessions channel (which
# gives the session dir as the CWD) or directly as
# `tv rushi-sessions-events DIR`. The source commands run the
# `rushi-sessions` binary's `events` subcommand against the CWD,
# newest first. Two views: `User+Assistant` (the default, the first
# source command) lists only the user and assistant messages, and
# `All` lists every entry. Television runs the first command on
# startup and cycles the rest on Ctrl+S.
#
# The channel takes two arguments: `eventPreviewer` and
# `eventTomlPreviewer`, a string each (default `"bat"`). The preview
# command pipes the
# binary's plain document into the renderer. The binary renders
# markdown for user_message, assistant_message and
# compaction_summary, and toml for tool_call, tool_result and every
# other entry. Each renderer option controls one language.
# The channel carries no top-level `watch` key: a reload resets the
# preview panel's scroll position. The CLI flag
# `tv rushi-sessions-events --watch N` enables the reload for one run.
# The default `bat` keeps the per-entry language flag, the
# RUSHI_PREVIEW_THEME override and the plain-text fallback when bat is
# absent. Any other value is one command line, run through `sh -c`,
# reading the document from stdin (for example `mdcat` or `glow`).
# The repo copy rushi-sessions-events.toml carries the default (bat)
# and is the manual-install baseline.
#
# The channel carries one action, `send_message` (bound to ctrl-t):
# the same design as the main channel's, local only. The session dir
# is the CWD of the channel, so the body reads it with `$(pwd)` and
# takes no split placeholder. It cds to the session's recorded
# working dir (the `cwd` file, or the
# dirname(dirname(session_dir)) fallback) before it runs `rushi
# run`: the loop re-anchors the `cwd` file to the starter's CWD on
# every start, and the fork's CWD is the session dir, not the
# project dir. It needs `rushi`, `stty` and
# `$EDITOR` (fallback `nvim`) on PATH. The detached loop start
# uses `setsid` when it is on PATH and falls back to `nohup`
# otherwise (the macOS base has no setsid). The body resolves the
# session dir with `pwd` and guards it: when the channel's CWD is
# gone (a deleted dir, or a tmux pane on a server whose own cwd was
# deleted) `pwd` prints nothing, and the action aborts with a message
# instead of sending to a ghost session.
#
# When tv runs inside tmux 3.7+, the prompt runs in a
# `display-popup` window over tv: the editor and the send step
# run in the popup, the popup closes on exit, the pane below
# keeps rendering, the fork waits for the close. The popup path
# adds `cat` and a second `mktemp` (the pipeline file); the
# display-popup flags it uses are -E, -T, -w, -h, -d, -e and
# the multi-arg command form. Below 3.7, or outside tmux, the
# pane editor runs instead.

{ eventPreviewer ? "bat", eventTomlPreviewer ? "bat", ... }:

let
  tab = "\t";
  untab = s: builtins.replaceStrings [ "@TAB@" ] [ tab ] s;

  # The document pipeline of the preview panel, per language. `bat`
  # keeps the language flag and the theme argument. Every other value
  # is one command line run through `sh -c`, reading the document
  # from stdin. It takes no theme flag: the user owns the full line.
  render = renderer: lang:
    if renderer == "bat" then
      "rushi-sessions event-preview . \"$q\" | bat --color=always -l ${lang} --style=plain"
    else
      "rushi-sessions event-preview . \"$q\" | sh -c \"${renderer}\"";

  # The default (bat, bat) is the repo baseline. It keeps the
  # RUSHI_PREVIEW_THEME override and falls back to the plain document
  # when bat is absent. Any other combination uses the sh -c form.
  previewCommand =
    if eventPreviewer == "bat" && eventTomlPreviewer == "bat" then
      untab ''
        sh -c 'q=$1; l=$(rushi-sessions event-preview . "$q" --print-lang); t=$RUSHI_PREVIEW_THEME; if command -v bat >/dev/null 2>&1; then if [ -n "$t" ]; then th="--theme $t"; else th=""; fi; if [ "$l" = toml ]; then rushi-sessions event-preview . "$q" | bat --color=always -l toml --style=plain $th; else rushi-sessions event-preview . "$q" | bat --color=always -l markdown --style=plain $th; fi; else rushi-sessions event-preview . "$q"; fi' sh '{split:@TAB@:0}'
      ''
    else
      untab ("sh -c 'q=$1; l=$(rushi-sessions event-preview . \"$q\" --print-lang); if [ \"$l\" = toml ]; then " + render eventTomlPreviewer "toml" + "; else " + render eventPreviewer "markdown" + "; fi' sh '{split:@TAB@:0}'");
in
{
  metadata = {
    name = "rushi-sessions-events";
    description = "Fuzzy-search one session's events.jsonl entries and preview them";
    requirements = [
      "rushi-sessions"
    ];
  };

  source = {
    shell = "bash";
    command = [
      {
        # The default view (the first source command): only the user
        # and assistant messages. No path argument: the binary reads
        # the CWD's events.jsonl.
        name = "User+Assistant";
        run = "rushi-sessions events --chat";
      }
      {
        # Every entry of the events.jsonl. No path argument: the
        # binary reads the CWD's events.jsonl.
        name = "All";
        run = "rushi-sessions events";
      }
    ];
    display = untab "[{split:@TAB@:1}] {split:@TAB@:2} {split:@TAB@:3}";
    output = untab "{split:@TAB@:0}";
    frecency = false;
  };

  preview = {
    shell = "bash";
    cached = true;
    command = previewCommand;
  };

  ui = {
    orientation= "portrait";
    preview_panel = {
      size = 62;
      word_wrap = true;
    };
  };

  keybindings = {
    "ctrl-t" = "actions:send_message";
  };

  actions.send_message = {
    # Multi-line form: the repo TOML holds a multi-line basic string,
    # whose value carries a trailing newline. The multi-line Nix
    # string matches that value leaf for leaf.
    description = ''
      Open $EDITOR (falling back to nvim) on a message file, then send the result to the session with `rushi run <session> <msg>`: it starts the loop detached in the session's recorded working dir when the session is idle, and appends from that dir when the loop is live. An empty message cancels. When tv runs inside tmux 3.7+, the editor and the send step run in a `display-popup` window over tv: the popup closes when the step exits, and the pane below keeps rendering. Outside tmux, or on tmux below 3.7, the editor opens in the fork's pane as before.
    '';

    shell = "bash";
    mode = "fork";
    # The session dir is the CWD of this channel (television chdirs
    # to its [PATH] argument; the main channel's browse_events action
    # starts the nested tv in the session dir). The source rows carry
    # only the seq token, so the body takes no split placeholder. It
    # stays brace-free: a stray {group} would break the television
    # template.
    # The command branches on tmux, like the main channel's. When $TMUX
    # is set, the tmux binary is on PATH, and `tmux -V` reports 3.7+,
    # the prompt runs in a `display-popup` window. The editor and the
    # send step run as the popup pipeline: a sh file written through
    # an unquoted heredoc (the pipeline's $ and backslash are escaped
    # for that layer), passed over the display-popup -e flags, and
    # run by the multi-arg `sh <pipeline>` form. -E closes the popup
    # when the pipeline exits; the fork blocks on that close, then
    # television resumes. A popup failure falls through to the pane
    # editor below. Below 3.7, or outside tmux, only the pane editor
    # runs: $EDITOR on the message file with the stty state saved
    # and restored around it.
    command = ''
      sh -c 'rp=$(pwd 2>/dev/null); if [ -z "$rp" ] || [ ! -d "$rp" ]; then echo "rushi-sessions-events: cannot resolve the session dir (the channel CWD is gone); re-open it from a live directory."; exit 1; fi; w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] || w=$(dirname "$(dirname "$rp")"); p=$(cat "$rp/loop.pid" 2>/dev/null); p=$(printf "%s" "$p" | tr -d "[:space:]"); live=no; if [ -n "$p" ]; then kill -0 "$p" 2>/dev/null && live=yes; fi; d=$TMPDIR; [ -n "$d" ] || d=/tmp; f=$(mktemp "$d/tv-rushi-msg.XXXXXX"); ed=$EDITOR; [ -n "$ed" ] || ed=nvim; tv=$(tmux -V 2>/dev/null); set -- $tv; v=$2; m=$(printf "%s" "$v" | cut -d. -f1); n=$(printf "%s" "$v" | cut -d. -f2); n=$(printf "%s" "$n" | tr -cd 0-9); tmo=no; if [ -n "$TMUX" ] && command -v tmux >/dev/null && { [ "$m" -gt 3 ] || { [ "$m" -eq 3 ] && [ "$n" -ge 7 ]; }; } 2>/dev/null; then tmo=yes; fi;  if [ "$tmo" = yes ]; then g=$(mktemp "$d/tv-rushi-pipe.XXXXXX"); cat > "$g" <<PIPE
      st=\$(stty -g 2>/dev/null); \$ED "\$F"; [ -n "\$st" ] && stty "\$st" 2>/dev/null || stty raw -echo 2>/dev/null; m=\$(cat "\$F" 2>/dev/null); rm -f "\$F"; if [ -z "\$(printf "%s" "\$m" | tr -d "[:space:]")" ]; then echo "No message entered, nothing sent."; exit 0; fi; cd "\$W" && if [ "\$LIVE" = yes ]; then rushi run "\$RP" "\$m"; echo "message sent to \$RP"; sleep 2; else if command -v setsid >/dev/null 2>&1; then setsid rushi run "\$RP" "\$m" </dev/null >/dev/null 2>&1 & else nohup rushi run "\$RP" "\$m" </dev/null >/dev/null 2>&1 & fi; echo "message queued for \$RP"; sleep 2; fi; fi
      PIPE
      tmux display-popup -E -T "tv-rushi message" -w 80% -h 60% -d "$w" -e RP="$rp" -e W="$w" -e LIVE="$live" -e F="$f" -e ED="$ed" sh "$g"; rc=$?; rm -f "$g"; if [ "$rc" -eq 0 ]; then exit 0; fi; fi; st=$(stty -g 2>/dev/null); ed=$EDITOR; [ -n "$ed" ] || ed=nvim; $ed "$f"; [ -n "$st" ] && stty "$st" 2>/dev/null || stty raw -echo 2>/dev/null; m=$(cat "$f" 2>/dev/null); rm -f "$f"; if [ -z "$(printf "%s" "$m" | tr -d "[:space:]")" ]; then echo "No message entered, nothing sent."; exit 0; fi; cd "$w" && if [ "$live" = yes ]; then rushi run "$rp" "$m"; else if command -v setsid >/dev/null 2>&1; then setsid rushi run "$rp" "$m" </dev/null >/dev/null 2>&1 & else nohup rushi run "$rp" "$m" </dev/null >/dev/null 2>&1 & fi; fi'
    '';
  };
}
