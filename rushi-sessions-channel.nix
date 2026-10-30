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
# The source command is a list of named commands, one per ROOT set.
# television runs the first on startup and cycles the rest on
# `cycle_sources` (Ctrl+S). The order is `Local`, then `All`, then
# `CWD`. `Local` scans the local roots only. `All` scans every
# configured root (local + remote). `CWD` scans the current directory
# (no roots) and is always present, last.
#
# The channel takes two arguments: `sourceRoots`, a list of local and
# remote paths the source commands scan, and `sessionPreviewer`, the
# preview panel's document renderer. The home-manager options
# `programs."rushi-sessions".sourceRoots` and
# `programs."rushi-sessions".sessionPreviewer` feed them. The module
# calls this file once, at `programs.television.channels."rushi-sessions"`,
# with the configured roots and renderer. An empty roots list leaves
# only the `CWD` view. The preview section carries two preview
# commands (the card and the recent-events view, cycled with
# `cycle_previews`). The default `sessionPreviewer` (bat) is the
# repo TOML baseline: the bare binary commands, whose internal bat
# pass keeps the RUSHI_PREVIEW_THEME override and the plain fallback
# when bat is absent. Any other value is one command line, run
# through `sh -c` on the plain document (`--plain`), reading the
# document from stdin (for example `mdcat` or `glow`).

{ sourceRoots ? [ ], sessionPreviewer ? "bat", ... }:

let
  tab = "\t";
  untab = s: builtins.replaceStrings [ "@TAB@" ] [ tab ] s;
  # Strip one trailing newline, when present. The repo TOML holds
  # preview.command as a single-line basic string, whose value has no
  # trailing newline. The untab string adds one.
  rstripNewline = v: let
    len = builtins.stringLength v;
  in
    if len > 0 && (builtins.substring (len - 1) 1 v) == "\n" then
    builtins.substring 0 (len - 1) v
    else v;

  # A root is remote when it matches the binary's `split_remote`: a colon
  # whose host part is non-empty, holds no slash, does not start with `@`,
  # and holds at most one `@`. This regex mirrors that test, so the Nix
  # side and the binary agree on which roots are remote. A root may be
  # local (`/export`) or remote (`host:/export` or `user@host:/export`, an
  # ssh alias). The binary runs remote roots over ssh. Roots with spaces
  # are not supported.
  isRemote = r: builtins.match "^[^/@][^/@]*@?[^/@]*:.+" r != null;
  localRoots = builtins.filter (r: ! (isRemote r)) sourceRoots;
  remoteRoots = builtins.filter (r: isRemote r) sourceRoots;
  joinRoots = rs: builtins.concatStringsSep " " rs;

  # One channel, several named source commands. Each passes a different ROOT
  # set to the same binary. No new flags: a view is a different path list.
  # The order is `Local`, then `All`, then `CWD`. `Local` scans the local
  # roots only. `All` scans every configured root (local + remote). `CWD`
  # scans the current directory (no roots) and is always present, last.
  # `Local` is emitted only when it differs from `All` (remote roots exist)
  # and there is a local root to scan; otherwise it would duplicate `CWD` or
  # `All`. television runs the first command on startup and cycles the rest
  # on `cycle_sources` (Ctrl+S).
  cwdCmd = { name = "CWD"; run = "rushi-sessions source"; };
  allCmd = { name = "All"; run = "rushi-sessions source ${joinRoots sourceRoots}"; };
  localCmd = { name = "Local"; run = "rushi-sessions source ${joinRoots localRoots}"; };
  sourceCommands =
    (if localRoots != [ ] && remoteRoots != [ ] then [ localCmd ] else [ ])
    ++ (if sourceRoots != [ ] then [ allCmd ] else [ ])
    ++ [ cwdCmd ];

  # The two preview commands of the channel. television runs the
  # first on startup and cycles to the next on `cycle_previews`
  # (Ctrl+F by default): the session card, then the recent-events
  # table view (`--events`, decision 99).
  #
  # The repo TOML holds each command as a single-line basic string,
  # so the values carry no trailing newline. rstripNewline strips
  # the one the untab string adds.
  #
  # The default `sessionPreviewer` (bat) is that baseline: the bare
  # binary commands, whose internal bat pass keeps the theme
  # override and the plain fallback. Any other value pipes the
  # plain document (`--plain`) into the renderer, run through
  # `sh -c` and reading the document from stdin.
  previewCommands =
    if sessionPreviewer == "bat" then
      [
        (rstripNewline (untab ''
          rushi-sessions preview '{split:@TAB@:0}' '{split:@TAB@:1}' '{split:@TAB@:2}' '{split:@TAB@:3}' '{split:@TAB@:4}' '{split:@TAB@:5}' '{split:@TAB@:6}'
        ''))
        (rstripNewline (untab ''
          rushi-sessions preview --events '{split:@TAB@:0}' '{split:@TAB@:1}' '{split:@TAB@:2}' '{split:@TAB@:3}' '{split:@TAB@:4}' '{split:@TAB@:5}' '{split:@TAB@:6}'
        ''))
      ]
    else
      [
        (untab ("rushi-sessions preview --plain '{split:@TAB@:0}' '{split:@TAB@:1}' '{split:@TAB@:2}' '{split:@TAB@:3}' '{split:@TAB@:4}' '{split:@TAB@:5}' '{split:@TAB@:6}' | sh -c \"${sessionPreviewer}\""))
        (untab ("rushi-sessions preview --events --plain '{split:@TAB@:0}' '{split:@TAB@:1}' '{split:@TAB@:2}' '{split:@TAB@:3}' '{split:@TAB@:4}' '{split:@TAB@:5}' '{split:@TAB@:6}' | sh -c \"${sessionPreviewer}\""))
      ];
in
{
  metadata = {
    name = "rushi-sessions";
    description = "Peek at rushi sessions: status, loop phase, last activity";
    # `fd` is a hard dependency of the `rushi-sessions` binary. `bat` is
    # optional; the preview falls back to plain markdown when it is
    # absent.
    # `tmux` is optional; open, open_v, open_dir_tmux_h and
    # open_dir_tmux_v use it only when tv runs inside a tmux session
    # (each detects $TMUX and falls back to the plain fork).
    # send_message and new_session open a `tmux new-pane` floating
    # pane for their prompts when tv runs inside tmux 3.8+; below
    # 3.8, or outside tmux, they fall back to the plain fork.
    # (The interactive actions call binaries resolved on PATH, which are
    # not listed as core requirements: `rushi` for send_message and
    # new_session (0.1.1+ for the --sessions-root flag), `rushi-tui`
    # for open/open_v, and `tmux` for open, open_v, open_dir_tmux_h
    # and open_dir_tmux_v. When the session's working dir carries a
    # `flake.nix` and `nix` is on PATH, the open actions run
    # `rushi-tui` in that devShell and the send/create actions run
    # `rushi run` in it; the remote rows run the same checks on the
    # remote host and use its devShell (decision 87/88, 93). The devShell
    # packages shadow the inherited PATH, so a binary the devShell
    # does not ship falls back to the inherited PATH entry. The
    # send_message and new_session prompts run
    # in the rushi-sessions binary: `mktemp` for the prompt file,
    # $EDITOR (fallback `nvim`), and `stty` on the fork-fallback
    # path. The detached loop start enters its own session inside
    # the binary (`setsid`, decision 102), with stdio to /dev/null.
    # No detach tool is needed on PATH. The
    # floating-pane stage is the binary itself, so no shell pipeline or
    # extra tools run there. The open* pane lookup adds `sed`, `sort`,
    # `tail` and the shell `printf` builtin, all in the standard Unix
    # base. Remote rows need `ssh` and `base64` on
    # the remote host (the message crosses the boundary as a base64
    # blob). Remote source roots additionally need `ssh`,
    # reachable by the alias in the root, and the same
    # `rushi-sessions` and `rushi` on the remote host. The open actions
    # additionally need `rushi-tui` there: the local pane runs it over
    # `ssh -t`. The open_dir_tmux actions open a local pane that runs the
    # remote shell over `ssh -t`, so no remote tmux is needed.
    # browse_events needs `tv` and this channel deployed there. A
    # non-interactive ssh does not source the shell init, so remote PATH
    # entries from a Nix profile are not visible. Set
    # RUSHI_SESSIONS_REMOTE_BIN on the remote to name the binary, or use
    # an absolute path.)
    requirements = [
      "fd"
      "rushi-sessions"
    ];
  };

  source = {
    shell = "bash";
    # A list of named source commands. television runs the first one on
    # startup and cycles to the next on `cycle_sources` (Ctrl+S by
    # default). The views are `Local`, then `All`, then `CWD` (when roots
    # are configured): each is a different ROOT set passed to the same
    # binary. `CWD` is always present and last.
    command = sourceCommands;
    display = "[{split:\t:0}] {split:\t:2}/{split:\t:1} [{split:\t:3}] {split:\t:4}";
    output = "{split:\t:6}";
    # Keep the source order of the rows, including the rows a typed
    # query filters to. The binary prints them newest-mtime first, so
    # the list and every query result show the most recently updated
    # sessions on top. Without this, television re-ranks fuzzy
    # matches by match score and the update-time order is lost.
    no_sort = true;
    # The frecency re-ranking is also off: it would float the most
    # often opened sessions to the top and break the time order.
    frecency = false;
  };

  preview = {
    shell = "bash";
    cached = true;
    # Two preview commands. television runs the first one on startup
    # and cycles to the next on `cycle_previews` (Ctrl+F by
    # default): the session card, then the recent-events table view.
    command = previewCommands;
  };

  ui = {
    orientation = "portrait";
    preview_panel = {
      size = 62;
    };
  };

  keybindings = {
    "ctrl-e" = "actions:open";
    "ctrl-v" = "actions:open_v";
    "alt-o" = "actions:open_dir";
    "alt-d" = "actions:open_dir_tmux_h";
    "alt-v" = "actions:open_dir_tmux_v";
    "ctrl-t" = "actions:send_message";
    "alt-k" = "actions:kill";
    "enter" = "actions:browse_events";
    "alt-e" = "actions:browse_events_v";
    "alt-n" = "actions:new_session";
  };

  actions.open = {
    # The TUI resolves a bare session name against a *relative*
    # sessions_root, so it must start in the directory that contains
    # the sessions tree. That dir is the session's recorded working
    # dir (the `cwd` file in the session dir, where its tools run):
    # the file's value is used as-is, with no walk-up. The path-based
    # dirname(dirname(session_dir)) stays the fallback for a missing
    # or stale `cwd` file. The session dir is handed to the TUI as an
    # absolute path (rushi-tui issue #31: an explicit session dir is
    # used as-is), so the session's identity survives wherever the
    # sessions tree lives: a git worktree without a `sessions`
    # symlink keeps the session dir in the main worktree's tree
    # while the session works in the worktree. Remote rows resolve
    # the dir on the remote host with one `ssh -T` call.
    #
    # tv forks this action, so the fork inherits the pane environment.
    # When tv runs inside tmux, TMUX is set: create a new pane in the
    # current window. The split targets the newest pane of the window,
    # not the tv pane. tmux pane ids are monotonic per server. The
    # largest live id is the newest pane. The first open lands next
    # to tv. Each later open stacks next to the pane it created.
    # Floating prompt panes are excluded from the lookup, so a
    # send_message or new_session pane never wins. The pane starts in
    # the working dir (-c). It runs rushi-tui there. The pane closes
    # when rushi-tui exits. The pane command cds to the resolved dir
    # itself. A tmux server whose own cwd was deleted fails its
    # getcwd. tmux (3.7) then skips the -c chdir for every new pane
    # on such a server. The explicit cd keeps the pane anchored. The
    # newest-pane lookup builds its tmux format at runtime from the
    # printf octal escapes. A literal brace group would break the
    # television template pass. An empty lookup falls back to the
    # tmux default target, the active pane. Outside tmux, or when
    # the tmux binary is missing, keep the original behavior: it runs
    # `cd` to the working dir, then rushi-tui in this fork.
    #
    # A remote row (host: prefix) also opens a pane in the LOCAL tmux
    # server, never on the remote one. The pane runs
    # `ssh -t HOST "cd WORKDIR && rushi-tui SESSION_DIR"`: the
    # remote TUI shows up next to tv, and the pane closes when it
    # exits. No tmux server is needed on the remote host (the old
    # behavior spawned a pane on the remote server, invisible to the
    # user). Outside local tmux the
    # fork runs the ssh command directly. The remote host needs
    # `rushi-tui` reachable by a non-interactive ssh.
    #
    # The devShell resolution (decision 87/88, 93): when the working
    # dir carries a `flake.nix` and `nix` is on PATH, the local
    # branch runs the TUI as `nix develop -c rushi-tui` from that
    # dir. The remote branch runs the same check over one
    # read-only `ssh -T` call (the remote flake file plus
    # `command -v nix`) and runs the TUI in the remote devShell. The
    # plain `nix develop` inherits the environment, so the devShell
    # packages shadow the inherited PATH, and a binary the devShell
    # does not ship falls back to the inherited PATH entry. A failed
    # gate (no flake, no nix) keeps the PATH `rushi-tui`, the
    # pre-flake behavior. The gate runs in the fork before the split;
    # the pane's own `nix develop` is fast after the gate.
    #
    # The shell body stays brace-free: television's string pipeline
    # treats a stray {group} as a template token and then leaves the
    # split placeholder unsubstituted. That is why the check reads
    # plain $TMUX instead of ${TMUX:-}.
    description = "Open this session in rushi-tui. The TUI starts in the session's recorded working dir (the `cwd` file in the session dir), falling back to the session's project dir when the file is missing or stale. The session dir is handed to the TUI as an absolute path, so the sessions tree may live elsewhere (a git worktree). Inside tmux, a new pane to the right of the newest pane in the tv window runs it. Outside tmux, it runs in this fork. Remote rows open the pane in the local tmux server: the pane runs the remote TUI over ssh -t. When the working dir's `.envrc` (or the first one up the parent chain) names a flake and nix is on PATH, local rows run the TUI in that flake's devShell; with no `.envrc` found the PATH `rushi-tui` is used (decision 95). Remote rows do the same on the remote host when the remote working dir carries a flake and nix is reachable there (decision 87/88, 93); a failed gate falls back to the PATH `rushi-tui`.";
    shell = "bash";
    mode = "fork";
    command = untab ''
sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then r=$(ssh -T "$host" "w=\$(cat \"$rp/cwd\" 2>/dev/null); [ -d \"\$w\" ] || w=\$(dirname \"\$(dirname \"$rp\")\"); printf \"%s\" \"\$w\""); rt=rushi-tui; rf=$(ssh -T "$host" "test -f \"$r/flake.nix\" && command -v nix >/dev/null 2>&1 && echo yes" 2>/dev/null); [ -n "$rf" ] && rt="nix develop -c rushi-tui"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -h -l "62%" -t "%$pn" "ssh -t $host \"cd $r && $rt \\\"$rp\\\"\""; else tmux split-window -h -l "62%" "ssh -t $host \"cd $r && $rt \\\"$rp\\\"\""; fi; else ssh -t "$host" "cd $r && $rt \"$rp\""; fi; else r=$(dirname "$(dirname "$rp")"); w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] && r=$w; tu=rushi-tui; fd=""; d=$r; while :; do if [ -f "$d/.envrc" ]; then ref=$(sed -n "s/^use flake[[:blank:]]*//p" "$d/.envrc" | head -n1); if [ -z "$ref" ] || [ "$ref" = "." ]; then fd=$d; else case $ref in "~/"*) fd="$HOME/${ref:2}";; "~") fd=$HOME;; /*) fd=$ref;; *) fd=$d/$ref;; esac; fi; break; fi; [ "$d" = "/" ] && break; d=$(dirname "$d"); done; [ -n "$fd" ] && command -v nix >/dev/null 2>&1 && tu="nix develop $fd -c rushi-tui"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -h -l "62%" -c "$r" -t "%$pn" "cd \"$r\" && $tu \"$rp\""; else tmux split-window -h -l "62%" -c "$r" "cd \"$r\" && $tu \"$rp\""; fi; else cd "$r" && $tu "$rp"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_v = {
    # Same as open, but the split is below the newest pane instead of
    # to its right. tmux's flags are easy to mix up: -v puts the new
    # pane below the target pane (a horizontal divider). The
    # remote-row behavior is the same ssh -t local pane, with the -v
    # split. The devShell resolution (decision 87/88, 93) is the
    # same as open: the TUI runs in the working dir's flake devShell
    # when the working dir carries a `flake.nix` and nix is on PATH;
    # remote rows run the same check on the remote host.
    description = "Open this session in rushi-tui. The TUI starts in the session's recorded working dir (the `cwd` file in the session dir), falling back to the session's project dir when the file is missing or stale. The session dir is handed to the TUI as an absolute path, so the sessions tree may live elsewhere (a git worktree). Inside tmux, a new pane below the newest pane in the tv window runs it. Outside tmux, it runs in this fork. Remote rows open the pane in the local tmux server: the pane runs the remote TUI over ssh -t. When the working dir's `.envrc` (or the first one up the parent chain) names a flake and nix is on PATH, local rows run the TUI in that flake's devShell; with no `.envrc` found the PATH `rushi-tui` is used (decision 95). Remote rows do the same on the remote host when the remote working dir carries a flake and nix is reachable there (decision 87/88, 93); a failed gate falls back to the PATH `rushi-tui`.";
    shell = "bash";
    mode = "fork";
    command = untab ''
sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then r=$(ssh -T "$host" "w=\$(cat \"$rp/cwd\" 2>/dev/null); [ -d \"\$w\" ] || w=\$(dirname \"\$(dirname \"$rp\")\"); printf \"%s\" \"\$w\""); rt=rushi-tui; rf=$(ssh -T "$host" "test -f \"$r/flake.nix\" && command -v nix >/dev/null 2>&1 && echo yes" 2>/dev/null); [ -n "$rf" ] && rt="nix develop -c rushi-tui"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -v -l "62%" -t "%$pn" "ssh -t $host \"cd $r && $rt \\\"$rp\\\"\""; else tmux split-window -v -l "62%" "ssh -t $host \"cd $r && $rt \\\"$rp\\\"\""; fi; else ssh -t "$host" "cd $r && $rt \"$rp\""; fi; else r=$(dirname "$(dirname "$rp")"); w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] && r=$w; tu=rushi-tui; fd=""; d=$r; while :; do if [ -f "$d/.envrc" ]; then ref=$(sed -n "s/^use flake[[:blank:]]*//p" "$d/.envrc" | head -n1); if [ -z "$ref" ] || [ "$ref" = "." ]; then fd=$d; else case $ref in "~/"*) fd="$HOME/${ref:2}";; "~") fd=$HOME;; /*) fd=$ref;; *) fd=$d/$ref;; esac; fi; break; fi; [ "$d" = "/" ] && break; d=$(dirname "$d"); done; [ -n "$fd" ] && command -v nix >/dev/null 2>&1 && tu="nix develop $fd -c rushi-tui"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -v -l "62%" -c "$r" -t "%$pn" "cd \"$r\" && $tu \"$rp\""; else tmux split-window -v -l "62%" -c "$r" "cd \"$r\" && $tu \"$rp\""; fi; else cd "$r" && $tu "$rp"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_dir = {
    # Exit tv and land in a shell at the session's recorded working
    # dir (the `cwd` file in the session dir, where the session's
    # tools run): the file's value is used as-is, with no walk-up.
    # The path-based dirname(dirname(session_dir)) stays the fallback
    # for a missing or stale `cwd` file: a git worktree without a
    # `sessions` symlink keeps the session dir in the main worktree's
    # tree while the session works in the worktree; the shell lands
    # in the worktree, where the session's relative paths complete.
    # Uses mode = "execute" so tv quits
    # and the command takes over the terminal; a fork would cd in a dead
    # child and tv would just resume. Falls back to bash when $SHELL is
    # unset. The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them.
    description = "Exit tv and cd into the session's recorded working dir (the `cwd` file in the session dir); a $SHELL shell (bash fallback) takes over the terminal. A missing or stale `cwd` file lands in the session's project dir. Remote rows cd on the remote host over ssh -t.";
    shell = "bash";
    mode = "execute";
    command = untab ''
sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; if [ -n "$host" ]; then r=$(ssh -T "$host" "w=\$(cat \"$rp/cwd\" 2>/dev/null); [ -d \"\$w\" ] || w=\$(dirname \"\$(dirname \"$rp\")\"); printf \"%s\" \"\$w\""); ssh -t "$host" "cd \"$r\" && exec $sh2"; else r=$(dirname "$(dirname "$rp")"); w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] && r=$w; cd "$r" && exec "$sh2"; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_dir_tmux_h = {
    # Open a shell at the session's recorded working dir (the `cwd`
    # file in the session dir) in a tmux pane: the file's value is
    # used as-is, with no walk-up. The
    # dirname(dirname(session_dir)) path rule stays the fallback for a
    # missing or stale `cwd` file, as in open_dir. The shell is
    # $SHELL, with a bash fallback when $SHELL is unset.
    #
    # tv forks this action, so the fork inherits the pane environment.
    # When tv runs inside tmux, TMUX is set: create a new pane in the
    # current window. The split targets the newest pane of the window,
    # not the tv pane. The first open lands next to tv. Each later
    # open stacks next to the pane it created. Floating prompt panes
    # are excluded from the lookup. The pane starts in the working dir
    # (-c). It runs the shell there. The pane closes when the shell
    # exits. tmux runs the pane command through its default shell, so
    # the pane command is the plain shell path. The pane command cds
    # to the resolved dir first. A tmux server whose own cwd was
    # deleted fails its getcwd. tmux (3.7) then skips the -c chdir
    # for every new pane on such a server. The explicit cd keeps the
    # pane anchored. The newest-pane lookup stays brace-free, as in
    # open. An empty lookup falls back to the tmux default target, the
    # active pane. Outside tmux, or when the tmux binary is missing,
    # the fork cds to the working dir and execs the shell. tv resumes
    # when the shell exits.
    #
    # A remote row (host: prefix) also opens a pane in the LOCAL tmux
    # server, never on the remote one. The pane runs
    # `ssh -t HOST "cd WORKDIR && exec SHELL"`: a shell at the
    # remote working dir shows up next to tv. No remote tmux server is needed.
    # Outside local tmux the fork runs the ssh command directly. The
    # shell name is the local $SHELL value (bash fallback), matching the
    # old remote branch.
    #
    # The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them. The body stays
    # brace-free for the same reason as open: a stray {group} would
    # break the television template and leave the split placeholder
    # unsubstituted.
    description = "Open a $SHELL shell (bash fallback) in the session's recorded working dir (the `cwd` file in the session dir). Inside tmux, a new pane to the right of the newest pane in the tv window runs it. Outside tmux, it runs in this fork. tv resumes when the shell exits. A missing or stale `cwd` file lands in the session's project dir. Remote rows open the pane in the local tmux server: the pane runs the remote shell over ssh -t.";
    shell = "bash";
    mode = "fork";
    command = untab ''
sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; if [ -n "$host" ]; then r=$(ssh -T "$host" "w=\$(cat \"$rp/cwd\" 2>/dev/null); [ -d \"\$w\" ] || w=\$(dirname \"\$(dirname \"$rp\")\"); printf \"%s\" \"\$w\""); c="cd $r && exec $sh2"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -h -l "62%" -t "%$pn" "ssh -t $host \"$c\""; else tmux split-window -h -l "62%" "ssh -t $host \"$c\""; fi; else ssh -t "$host" "$c"; fi; else r=$(dirname "$(dirname "$rp")"); w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] && r=$w; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -h -l "62%" -c "$r" -t "%$pn" "cd \"$r\" && exec \"$sh2\""; else tmux split-window -h -l "62%" -c "$r" "cd \"$r\" && exec \"$sh2\""; fi; else cd "$r" && exec "$sh2"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_dir_tmux_v = {
    # Same as open_dir_tmux_h, but the split is below the newest pane
    # instead of to its right. tmux's flags are easy to mix up: -v
    # puts the new pane below the target pane (a horizontal divider).
    # The remote-row behavior is the same ssh -t local pane, with the
    # -v split. Outside tmux, the behavior is the same plain fork as
    # open_dir_tmux_h.
    #
    # The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them.
    description = "Open a $SHELL shell (bash fallback) in the session's recorded working dir (the `cwd` file in the session dir). Inside tmux, a new pane below the newest pane in the tv window runs it. Outside tmux, it runs in this fork. tv resumes when the shell exits. A missing or stale `cwd` file lands in the session's project dir. Remote rows open the pane in the local tmux server: the pane runs the remote shell over ssh -t.";
    shell = "bash";
    mode = "fork";
    command = untab ''
sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; if [ -n "$host" ]; then r=$(ssh -T "$host" "w=\$(cat \"$rp/cwd\" 2>/dev/null); [ -d \"\$w\" ] || w=\$(dirname \"\$(dirname \"$rp\")\"); printf \"%s\" \"\$w\""); c="cd $r && exec $sh2"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -v -l "62%" -t "%$pn" "ssh -t $host \"$c\""; else tmux split-window -v -l "62%" "ssh -t $host \"$c\""; fi; else ssh -t "$host" "$c"; fi; else r=$(dirname "$(dirname "$rp")"); w=$(cat "$rp/cwd" 2>/dev/null); [ -d "$w" ] && r=$w; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -v -l "62%" -c "$r" -t "%$pn" "cd \"$r\" && exec \"$sh2\""; else tmux split-window -v -l "62%" -c "$r" "cd \"$r\" && exec \"$sh2\""; fi; else cd "$r" && exec "$sh2"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.send_message = {
    #
    # send_message (ctrl-t) opens the editor on a message file, then pokes
# the session with one detached `rushi run <session> <msg>` call, run in
# the session's recorded working dir. The kernel branches on the session
# lock itself: a live loop drains the appended message, an idle session
# starts the loop. When the project dir carries a `flake.nix` and
# nix is on PATH, the detached call runs in that devShell
# (`nix develop -c rushi run ...`, decision 87/88); remote rows run
# the same check on the remote host. The devShell shadows the
# inherited PATH, so a binary the devShell does not ship falls back
# to the PATH `rushi`.
#
# The action logic runs in the rushi-sessions binary. Stage 1 runs in
# the television fork: it resolves the row, the host prefix, the
# working dir and the editor, creates the prompt file, then spawns a
# floating pane (tmux 3.8+) or falls back to the fork's pane. Stage 2
# is the pane's own command (the binary's absolute path): it runs the
# editor, then the send step, and the pane dies when it exits. The
# handoff is a JSON args file, so no value crosses a shell.
#
# The pane is not modal: the panes below keep their layout, and
# copy-mode, window switching and the prefix bindings stay available.
# A broken step is a visible flash, not a silent fallback. Tmux below
# 3.8, or running outside tmux, falls back to the fork's pane.
    # Single-line form: the repo TOML holds a single-line basic string,
    # whose value carries no trailing newline.
    description = "Open $EDITOR (falling back to nvim) on a message file, then send the result to the session with one detached `rushi run <session> <msg>` call, run in the session's recorded working dir. The kernel branches on the session lock itself: a live loop drains the appended message, an idle session starts the loop. An empty message cancels. When tv runs inside tmux 3.8+, the editor and the send step run in a floating pane (`tmux new-pane -f`) above tv: the pane closes when the step exits, and the panes below keep rendering. Copy-mode and window switching stay available while the pane is open. Outside tmux, or on tmux below 3.8, the editor opens in the fork's pane as before. The prompt file carries a .md extension, so $EDITOR applies the markdown filetype. The editor opens in the session's recorded working dir, so typed relative paths complete against it. When the project dir's `.envrc` (or the first one up the parent chain) names a flake and nix is on PATH, the detached call runs in that flake's devShell (`nix develop <flake> -c rushi run ...`, decision 95); with no `.envrc` found the global PATH `rushi` is used. Remote rows run the same check on the remote host. The `nix develop` inherits the environment, so the devShell packages shadow the inherited PATH, and a binary the devShell does not ship falls back to the PATH `rushi`. The action logic runs in the rushi-sessions binary: the pane's command is the binary itself, so no shell quoting crosses the tmux boundary.";
    shell = "bash";
    mode = "fork";
        command = "rushi-sessions send-message '{split:	:6}'";
  };

  actions.browse_events = {
    # Fuzzy-search this session's events.jsonl in a nested `tv` (the
    # `rushi-sessions-events` channel). The nested tv reads the
    # session dir's events.jsonl from its CWD, so it must start
    # there.
    #
    # tv forks this action, so the fork inherits the pane environment.
    # When tv runs inside tmux, TMUX is set: create a new pane in the
    # current window. The split targets the newest pane of the window,
    # not the tv pane, as in the open* actions. The first open lands
    # next to tv. Each later open stacks next to the pane it created.
    # The pane starts in the session dir (-c). It runs the nested tv
    # there. The pane closes when the nested tv exits. The pane
    # command cds to the session dir first. A tmux server whose own
    # cwd was deleted fails its getcwd. tmux (3.7) then skips the
    # -c chdir for every new pane on such a server. The explicit cd
    # keeps the nested tv in the session dir. The newest-pane lookup
    # stays brace-free, as in open. An empty lookup falls back to the
    # tmux default target, the active pane. Outside tmux, or when the
    # tmux binary is missing, the fork cds to the session dir and runs
    # the nested tv here. The outer tv pauses and resumes when the
    # nested tv exits.
    #
    # Remote rows (`host:` prefix) run the nested tv on the remote
    # host over `ssh -t` (a pty, the nested tv is a TUI). The remote
    # host must carry the same deployment of this channel (the same
    # flake is deployed there).
    #
    # The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them. The body stays
    # brace-free for the same reason as open: a stray {group} would
    # break the television template.
    description = "Fuzzy-search this session's events.jsonl in a nested tv (the rushi-sessions-events channel). Inside tmux, a new pane to the right of the newest pane in the tv window runs it from the session dir. Outside tmux, it runs in this fork. Remote rows run it over ssh.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then ssh -t "$host" "cd \"$rp\" && tv rushi-sessions-events"; else t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -h -l "62%" -c "$rp" -t "%$pn" "cd \"$rp\" && tv rushi-sessions-events"; else tmux split-window -h -l "62%" -c "$rp" "cd \"$rp\" && tv rushi-sessions-events"; fi; else cd "$rp" && tv rushi-sessions-events; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.browse_events_v = {
    # Same as browse_events, but the split is below the newest pane
    # instead of to its right. tmux's flags are easy to mix up: -v
    # puts the new pane below the target pane (a horizontal divider).
    # Only the local tmux branch changes: both split-window calls swap
    # -h for -v. The remote-row branch runs the nested tv over
    # `ssh -t` unchanged, as in browse_events. Outside tmux, the
    # plain-fork behavior is unchanged.
    #
    # The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them. The body stays
    # brace-free for the same reason as open: a stray {group} would
    # break the television template.
    description = "Fuzzy-search this session's events.jsonl in a nested tv (the rushi-sessions-events channel). Inside tmux, a new pane below the newest pane in the tv window runs it from the session dir. Outside tmux, it runs in this fork. Remote rows run it over ssh.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then ssh -t "$host" "cd \"$rp\" && tv rushi-sessions-events"; else t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then ob=$(printf "\173"); cb=$(printf "\175"); F="#"$ob"pane_id"$cb" ""#"$ob"pane_floating_flag"$cb; pn=$(tmux list-panes -F "$F" | sed "/ 0\$/!d; s/ .*//; s/^%//" | sort -n | tail -1); if [ -n "$pn" ]; then tmux split-window -v -l "62%" -c "$rp" -t "%$pn" "cd \"$rp\" && tv rushi-sessions-events"; else tmux split-window -v -l "62%" -c "$rp" "cd \"$rp\" && tv rushi-sessions-events"; fi; else cd "$rp" && tv rushi-sessions-events; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.kill = {
    description = "Kill the session loop (SIGKILL to the loop pid)";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then p=$(ssh -T "$host" cat "$rp/loop.pid" 2>/dev/null); p=$(printf "%s" "$p" | tr -d "[:space:]"); if [ -n "$p" ] && ssh -T "$host" kill -0 "$p" 2>/dev/null; then ssh -T "$host" kill -KILL "$p"; echo "sent SIGKILL to $p ("$(basename "$rp")") on $host"; else echo "no live loop for "$(basename "$rp")" on $host (stale or absent pid)"; fi; else p=$(cat "$rp/loop.pid" 2>/dev/null); p=$(printf "%s" "$p" | tr -d "[:space:]"); if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then kill -KILL "$p"; echo "sent SIGKILL to $p ("$(basename "$rp")")"; else echo "no live loop for "$(basename "$rp")" (stale or absent pid)"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.new_session = {
    #
    # Create a sibling session next to the selected entry. The prompt opens
# $EDITOR (fallback nvim) on a temp file: line 1 is the session name,
# the rest is the message. The project dir is the entry's own
# dirname(dirname(session_dir)): a new session anchors to the project
# where the entry lives, not to the entry's recorded cwd file.
# rushi run takes --sessions-root <project>/sessions (rushi 0.1.1+),
# so the new session's recorded working dir is the sessions root's
# parent: the project dir itself.
#
# The action logic runs in the rushi-sessions binary (the same
# two-stage handoff as send_message): stage 1 in the television fork
# resolves the row, the project dir and the editor, writes the hint
# line, then spawns the floating pane or falls back to the fork's
# pane. Stage 2 runs the editor, strips the hint from line 1,
# validates the name, then the detached
# `rushi run NAME MSG --sessions-root <project>/sessions` (idle when
# the message is blank).
#
# A remote row (host: prefix) prompts locally, resolves the project dir
# on the remote host with one read-only ssh -T call (dirname dirname of
# the session path), and creates the session there over ssh -T: the
# message passes as a base64 blob.
    # Single-line form: the repo TOML holds a single-line basic string,
    # whose value carries no trailing newline.
    description = "Create a new session next to the selected entry. The action opens $EDITOR (fallback nvim) on a temp file: line 1 is the session name, the rest is the message. The file starts with a hint line; the hint text is stripped from line 1, so the name may sit before, after, or instead of the hint. The session lands in the entry's project dir: dirname(dirname(session dir)), sessions root <project>/sessions. It runs `rushi run NAME MSG --sessions-root <project>/sessions` from that project dir, detached in its own session (the binary calls `setsid`, decision 102), with stdio to /dev/null. When the project dir's `.envrc` (or the first one up the parent chain) names a flake and nix is on PATH, the detached call runs in that flake's devShell (`nix develop <flake> -c rushi run ...`, decision 95); with no `.envrc` found the global PATH `rushi` is used. Remote rows run the same check on the remote host. The `nix develop` inherits the environment, so the devShell packages shadow the inherited PATH, and a binary the devShell does not ship falls back to the PATH `rushi`. An empty message creates the session idle. An empty or unusable name (line 1 empty after the hint is stripped, or it holds /, . or ..) cancels. Remote rows (host: prefix) prompt locally and create the session over ssh. When tv runs inside tmux 3.8+, the editor and the create step run in a floating pane (`tmux new-pane -f`) above tv: the pane closes when the step exits, and the panes below keep rendering. Copy-mode and window switching stay available while the pane is open. Outside tmux, or on tmux below 3.8, the editor opens in the fork's pane as before. The prompt file carries a .md extension, so $EDITOR applies the markdown filetype. The editor opens in the entry's project dir, so typed relative paths complete against it. The action logic runs in the rushi-sessions binary: the pane's command is the binary itself, so no shell quoting crosses the tmux boundary.";
    shell = "bash";
    mode = "fork";
        command = "rushi-sessions new-session '{split:	:6}'";
  };
}
