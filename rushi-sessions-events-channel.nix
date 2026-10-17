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
# The channel takes four arguments: `eventPreviewer`,
# `eventTomlPreviewer`, `eventViewer` and `eventTomlViewer`, a
# string each (defaults `"bat"`, `"bat"`, `"mdfried"` and
# `"bat"`). The previewer options control the preview panel's
# renderer per language. The preview command pipes the binary's
# plain document into the renderer. The binary renders markdown
# for user_message, assistant_message and compaction_summary, and
# toml for tool_call, tool_result and every other entry. Each
# renderer option controls one language. The `eventViewer` and
# `eventTomlViewer` options are the commands the `view_message`
# action (bound to enter) runs on the entry document: the fork
# picks the command from the entry's language, pipes the document
# into it, and the viewer takes over the pane until it exits. Any
# value is one command line, reading the document from stdin. The
# defaults (mdfried for markdown, bat for toml) are the baseline
# direct pipes the repo TOML carries.
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
# The channel carries two actions. `send_message` (bound to ctrl-t) is
# the same design as the main channel's, local only. The session dir
# is the CWD of the channel, so the body reads it with `$(pwd)` and
# takes no split placeholder. It cds to the session's recorded
# working dir (the `cwd` file, or the
# dirname(dirname(session_dir)) fallback) before it runs `rushi
# run`: the loop re-anchors the `cwd` file to the starter's CWD on
# every start, and the fork's CWD is the session dir, not the
# project dir. The send is one detached `rushi run` call for both
# loop states: the kernel branches on the session lock itself, a
# live loop drains the appended message, an idle session starts the
# loop. It needs `rushi`, `stty`, `mv` and
# `$EDITOR` (fallback `nvim`) on PATH. The detach uses `setsid`
# when it is on PATH and falls back to `nohup` otherwise (the
# macOS base has no setsid). The body resolves the
# session dir with `pwd` and guards it: when the channel's CWD is
# gone (a deleted dir, or a tmux pane on a server whose own cwd was
# deleted) `pwd` prints nothing, and the action aborts with a message
# instead of sending to a ghost session.
#
# When tv runs inside tmux 3.8+, the prompt runs in a floating
# pane (`tmux new-pane -f`) above tv: the editor and the send
# step run in the pane, the pane dies on exit, the panes below
# keep rendering, and the fork returns at pane creation. The
# floating-pane path adds `cat` and a second `mktemp` (the
# pipeline file); the new-pane flags it uses are -f, -c, -x,
# -y, -X, -Y, -T and the multi-arg command form. The pane
# is not modal: copy-mode, window switching and the prefix
# bindings stay available while it is open. Below 3.8, or
# outside tmux, the pane editor runs instead.
#
# `view_message` (bound to enter) forks a fullscreen viewer on the
# selected entry's document: the body pipes
# `rushi-sessions event-preview . SEQ` into a viewer chosen from
# the entry's language, and the viewer takes over the pane until
# it exits. The body asks the binary for the language once
# (`--print-lang`), then branches: markdown to the `eventViewer`
# command, toml to the `eventTomlViewer` command. The body takes
# the seq through one split placeholder (`{split:\t:0}`).

{ eventPreviewer ? "bat", eventTomlPreviewer ? "bat", eventViewer ? "mdfried", eventTomlViewer ? "bat", ... }:

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

  # The per-language pipeline of the view_message action. The
  # defaults (mdfried for markdown, bat for toml) are the baseline
  # form, a direct pipe, and are what the repo TOML carries. Every
  # other value is one command line, run through `sh -c`, reading
  # the document from stdin.
  viewPipe = viewer:
    if viewer == "mdfried" || viewer == "bat" then
      "rushi-sessions event-preview . \"$q\" | ${viewer}"
    else
      "rushi-sessions event-preview . \"$q\" | sh -c \"${viewer}\"";

  # The view_message command: the fork asks the binary for the
  # entry's language once, then pipes the document into the viewer
  # of that language. The defaults are the repo TOML baseline.
  viewCommand =
    if eventViewer == "mdfried" && eventTomlViewer == "bat" then
      untab ''
        sh -c 'q=$1; l=$(rushi-sessions event-preview . "$q" --print-lang); if [ "$l" = toml ]; then rushi-sessions event-preview . "$q" | bat; else rushi-sessions event-preview . "$q" | mdfried; fi' sh '{split:@TAB@:0}'
      ''
    else
      untab ("sh -c 'q=$1; l=$(rushi-sessions event-preview . \"$q\" --print-lang); if [ \"$l\" = toml ]; then " + viewPipe eventTomlViewer + "; else " + viewPipe eventViewer + "; fi' sh '{split:@TAB@:0}'");
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
    "enter" = "actions:view_message";
  };

  actions.send_message = {
    #
    # The session dir is the CWD of this channel: television chdirs to the
# [PATH] argument, and the main channel's browse_events action starts
# the nested tv in the session dir. The fork of this action inherits
# it. The source rows carry only the seq token, so the command takes
# no split placeholder.
#
# The action logic runs in the rushi-sessions binary: `--events` mode
# resolves the session dir from the CWD, reads the recorded working dir
# (the `cwd` file, or the project dir when the file is missing), runs
# the editor in a floating pane (tmux 3.8+) or in the fork's pane,
# then one detached `rushi run` for both loop states: the kernel
# branches on the session lock, a live loop drains the appended
# message, an idle session starts the loop.
    # Single-line form: the repo TOML holds a single-line basic string,
    # whose value carries no trailing newline.
    description = "Open $EDITOR (falling back to nvim) on a message file, then send the result to the session with one detached `rushi run <session> <msg>` call, run in the session's recorded working dir. The kernel branches on the session lock itself: a live loop drains the appended message, an idle session starts the loop. An empty message cancels. When tv runs inside tmux 3.8+, the editor and the send step run in a floating pane (`tmux new-pane -f`) above tv: the pane closes when the step exits, and the panes below keep rendering. Copy-mode and window switching stay available while the pane is open. Outside tmux, or on tmux below 3.8, the editor opens in the fork's pane as before. The prompt file carries a .md extension, so $EDITOR applies the markdown filetype. The editor opens in the session's recorded working dir, so typed relative paths complete against it. The action logic runs in the rushi-sessions binary: the pane's command is the binary itself, so no shell quoting crosses the tmux boundary.";
    shell = "bash";
    mode = "fork";
        command = "rushi-sessions send-message --events '{split:\t:6}'";
  };

  actions.view_message = {
    # Multi-line form, like send_message: the value carries a
    # trailing newline, matching the repo TOML leaf for leaf.
    description = ''
      View the selected entry in the channel's viewer. The fork picks the viewer from the entry's language: markdown goes to the eventViewer, toml goes to the eventTomlViewer. The viewer takes over the pane, and television resumes when the viewer exits. The baseline copy pipes markdown into mdfried and toml into bat. Set the options of the rushi-sessions home-manager module to pick the viewers.
    '';

    shell = "bash";
    mode = "fork";
    command = viewCommand;
  };
}
