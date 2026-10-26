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
# newest first, in a collapsed branch view. The binary computes the
# active ranges from the rewind markers: the listing keeps the active
# branch plus the history before each diverging node, and it folds
# every abandoned span into the annotations of the row that owns it.
# Each annotation column reads: a `(Folded N)` count after the
# row's summary, a `branched from #X` mark on the row where a hidden
# branch left the trunk, a `branched out to #Y` mark on the row
# where a hidden branch leaves the shown span, and a trailing
# `N hidden` for the gap behind the newest row. The `#N` numbers
# are item numbers, the log seq minus 1. A `+` after the indent
# marks the fold on a branch marker row. Two views: `User+Assistant`
# (the default, the first source command) lists the user and
# assistant messages plus the rewind markers that carry the branch
# annotations, and `All` lists every entry. Television runs the
# first command on startup and cycles the rest on Ctrl+S.
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
# The channel carries seven actions. `send_message` (bound to ctrl-t) is
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
# loop. When the project dir carries a `flake.nix` and nix is on
# PATH, the detached call runs in that devShell
# (`nix develop -c rushi run ...`, decision 87/88). The devShell
# shadows the inherited PATH, so a binary the devShell does not
# ship falls back to the PATH `rushi`. It needs `rushi`, `stty`, `mv` and
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
#
# `branch_view` (bound to alt-b) re-opens the channel on the parent
# branch of the picked row. The fork runs `tv rushi-sessions-events`
# with a temp cable dir: a copy of this channel file whose source
# commands are patched to `rushi-sessions events --branch SEQ` (and
# the same with `--chat`), the binary's `--branch` flag taking the
# pick as the focus. The patch keeps both source views, so Ctrl+S
# cycles the focused User+Assistant view and the focused All view.
# The listing folds every span outside the focus into the
# annotations of the row that owns it. The nested tv keeps this
# channel's display template, preview, keybindings and actions, and
# Ctrl-R re-sources the view. Closing it (esc or ctrl-c) restores
# this view. The pane shell deletes the temp cable dir when the
# nested tv exits. When tv runs inside tmux, the nested tv runs in
# a floating pane over the tv window (`tmux new-pane -f`, the
# tmux 3.8+ tier; the pane lookup keeps the control-char `-F`
# format of the main channel's browse_events): centered, 62%
# width and 90% height, labeled `rushi branch view`. The pane
# dies when the nested tv exits, restoring this view without a
# layout change. When the floating pane is unavailable (older
# tmux), the split below the newest pane is used; outside tmux,
# the nested tv runs in the fork's pane. When the channel's CWD
# is gone, or the channel file is missing, the action aborts with
# a message instead of opening a tv on a dead dir.
#
# `yank_entry` (bound to ctrl-y) copies the full document of the
# selected entry to the clipboard. It keeps the newlines, which
# the native television ctrl-y drops. The native yank copies the
# raw source row, which is one tab-separated line. The fork runs
# the binary's copy-entry subcommand on the selected seq. It
# writes the OSC 52 escape to the controlling tty and pipes the
# document into a local clipboard tool when one is on PATH. It
# adds no binary requirement.

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
      "rushi"
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
    display = untab "{split:@TAB@:4}[{split:@TAB@:1}] {split:@TAB@:2} {split:@TAB@:3}";
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
    "alt-r" = "actions:rewind";
    "alt-s" = "actions:summarize";
    "alt-p" = "actions:summarize-prompt";
    "enter" = "actions:view_message";
    # Replaces the native ctrl-y, which copies the raw source row
    # (one line, tab-separated).
    "ctrl-y" = "actions:yank_entry";
    # Re-open the channel on the parent branch of the picked row.
    "alt-b" = "actions:branch_view";
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
    description = "Open $EDITOR (falling back to nvim) on a message file, then send the result to the session with one detached `rushi run <session> <msg>` call, run in the session's recorded working dir. The kernel branches on the session lock itself: a live loop drains the appended message, an idle session starts the loop. An empty message cancels. When tv runs inside tmux 3.8+, the editor and the send step run in a floating pane (`tmux new-pane -f`) above tv: the pane closes when the step exits, and the panes below keep rendering. Copy-mode and window switching stay available while the pane is open. Outside tmux, or on tmux below 3.8, the editor opens in the fork's pane as before. The prompt file carries a .md extension, so $EDITOR applies the markdown filetype. The editor opens in the session's recorded working dir, so typed relative paths complete against it. When the project dir carries a flake and nix is on PATH, the detached call runs in that devShell (`nix develop -c rushi run ...`, decision 87/88). The devShell shadows the inherited PATH, so a binary the devShell does not ship falls back to the PATH `rushi`. The action logic runs in the rushi-sessions binary: the pane's command is the binary itself, so no shell quoting crosses the tmux boundary.";
    shell = "bash";
    mode = "fork";
        command = "rushi-sessions send-message --events";
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

  actions.yank_entry = {
    # Single-line form, like send_message: the repo TOML holds a
    # single-line basic string, whose value carries no trailing
    # newline.
    description = "Copy the selected entry's full document (newlines preserved) to the clipboard. The fork runs the binary's copy-entry subcommand. It renders the entry the way event-preview does: markdown for messages, toml for everything else. The OSC 52 terminal clipboard escape is written to the controlling tty first. A supporting terminal, or tmux 3.8+ with set-clipboard on, lands it on the system clipboard. A local clipboard tool (pbcopy, wl-copy, xclip, xsel) takes the pipe when one is on PATH.";
    shell = "bash";
    mode = "fork";
    command = untab "rushi-sessions copy-entry . '{split:@TAB@:0}'";
  };

  actions.rewind = {
    description = ''
      Rewind the session to the selected event, without a summary (the tree view's `Rewind without summary` option). The binary validates the seq, checks that the loop is idle through the kernel's .loop.lock flock, and appends the rewind marker with the kernel's log appender (`rushi-log`, the legacy `log` of older kernels). A user_message pick rewinds before it; every other type rewinds on it. If the loop is live, the binary exits without writing; the loop applies the marker on its next step.
    '';
    shell = "bash";
    mode = "fork";
    command = untab "rushi-sessions rewind '{split:@TAB@:0}'";
  };

  actions.summarize = {
    description = ''
      One-shot: append a rewind marker at the picked event (the picked event stays in the context), then summarize the abandoned branch with the kernel's compact binary (--branch) in the session's recorded working dir (the tree view's `Summarize the branch` option). No pre-existing rewind marker is needed. The loop applies the summary on its next step.
    '';
    shell = "bash";
    mode = "fork";
    command = untab "rushi-sessions summarize '{split:@TAB@:0}'";
  };

  actions."summarize-prompt" = {
    description = ''
      One-shot with a custom prompt (the tree view's `Summarize with custom prompt` option): open the editor for one instruction line (the same prompt flow as the send action; a blank file cancels and appends no marker), then append a rewind marker at the picked event and run the kernel's compact binary with --branch and --prompt in the session's recorded working dir.
    '';
    shell = "bash";
    mode = "fork";
    command = untab "rushi-sessions summarize '{split:@TAB@:0}' --prompt";
  };

  actions.branch_view = {
    # Open the picked entry's parent branch in a nested tv. The
    # nested tv's source command is the `branch` view of the
    # picked seq, via television's --source-command override: the
    # channel's display template, preview and actions stay, so
    # ctrl-r refresh and the rewind, summarize and send_message
    # actions work in the nested view. When tv runs inside tmux,
    # the nested tv runs in a floating pane over the tv window
    # (tmux new-pane -f, the tmux 3.8+ tier; the pane lookup
    # keeps the control-char -F format of the main channel's
    # browse_events): centered, 62% width and 90% height,
    # labeled `rushi branch view`. The pane dies when the nested
    # tv exits, restoring this view without a layout change.
    # When the floating pane is unavailable (older tmux), the
    # split below the newest pane of browse_events is used;
    # outside tmux it runs in the fork's pane. The source rows
    # carry the seq token, so the command takes one split
    # placeholder. The action needs `tv` on PATH, resolved like
    # the main channel's browse_events.
    #
    # Single-line form, like yank_entry: the repo TOML holds a
    # single-line basic string, whose value carries no trailing
    # newline.
    description = "Open the channel again on the parent branch of the picked row: the fork sources this channel with `rushi-sessions events --branch SEQ` as its source command (the binary's focus flag). When tv runs inside tmux, a floating pane over the tv window runs it (the tmux 3.8+ tier; the split-pane fallback on older tmux); otherwise it runs in the fork's pane. The listing keeps the picked block and the trunk up to its branch point, and folds every span outside the focus into the annotations of the row that owns it, so the active branch stays complete. The nested tv keeps this channel's preview, keybindings and actions; close it with esc or ctrl-c to restore this view.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's=$1; rp=$(pwd 2>/dev/null);
      if [ -z "$rp" ]; then echo "rushi-sessions-events: the channel CWD is gone; re-open the channel" >&2; exit 1; fi;
      cf="$HOME/.config/television/cable/rushi-sessions-events.toml";
      if [ ! -f "$cf" ]; then echo "rushi-sessions-events: the channel file is missing; re-open the channel" >&2; exit 1; fi;
      tmp=$(mktemp -d)
      if [ -z "$tmp" ]; then echo "rushi-sessions-events: cannot create a temp dir" >&2; exit 1; fi;
      sed -e "s|run = \"rushi-sessions events --chat\"|run = \"rushi-sessions events --branch $s --chat\"|" -e "s|run = \"rushi-sessions events\"|run = \"rushi-sessions events --branch $s\"|" "$cf" > "$tmp/rushi-sessions-events.toml"
      if [ $? -ne 0 ]; then rm -rf "$tmp"; echo "rushi-sessions-events: cannot build the focused channel file" >&2; exit 1; fi;
      t=$TMUX;
      if [ -n "$t" ] && command -v tmux >/dev/null; then
        ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1);
        if [ -n "$pn" ] && tmux new-pane -f -c "$rp" -x "62%" -y "90%" -X "19%" -Y "5%" -T "rushi branch view" -t "%$pn" sh -c "tv --cable-dir \"\$1\" rushi-sessions-events; rm -rf \"\$1\"" sh "$tmp"; then
          :;
        elif [ -n "$pn" ]; then
          tmux split-window -h -l "62%" -c "$rp" -t "%$pn" sh -c "tv --cable-dir \"\$1\" rushi-sessions-events; rm -rf \"\$1\"" sh "$tmp";
        else
          tmux split-window -h -l "62%" -c "$rp" sh -c "tv --cable-dir \"\$1\" rushi-sessions-events; rm -rf \"\$1\"" sh "$tmp";
        fi;
      else
        sh -c "tv --cable-dir \"\$1\" rushi-sessions-events; rm -rf \"\$1\"" sh "$tmp";
      fi' sh '{split:@TAB@:0}'
    '';
  };
}
