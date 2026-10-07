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
# The channel takes one argument: `sourceRoots`, a list of local and
# remote paths the source commands scan. The home-manager option
# `programs."rushi-sessions".sourceRoots` feeds it. The module calls this
# file once, at `programs.television.channels."rushi-sessions"`, with the
# configured roots. An empty list leaves only the `CWD` view.

{ sourceRoots ? [ ], ... }:

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

  # The repo TOML holds this command as a single-line basic string, so
  # the value carries no trailing newline. rstripNewline strips the one
  # the untab string adds.
  previewCommand = rstripNewline (untab ''
    rushi-sessions preview '{split:@TAB@:0}' '{split:@TAB@:1}' '{split:@TAB@:2}' '{split:@TAB@:3}' '{split:@TAB@:4}' '{split:@TAB@:5}' '{split:@TAB@:6}'
  '');
in
{
  metadata = {
    name = "rushi-sessions";
    description = "Peek at rushi sessions: status, loop phase, last activity";
    # `fd` is a hard dependency of the `rushi-sessions` binary. `bat` is
    # optional; the preview falls back to plain TOML when it is absent.
    # `tmux` is optional; open, open_v, open_dir_tmux_h and
    # open_dir_tmux_v use it only when tv runs inside a tmux session
    # (each detects $TMUX and falls back to the plain fork).
    # (The interactive actions call binaries resolved on PATH, which are
    # not listed as core requirements: `rushi` for send_message,
    # `rushi-tui` for open/open_v, and `tmux` for open, open_v,
    # open_dir_tmux_h and open_dir_tmux_v. send_message uses `mktemp`,
    # `stty`, `setsid`, `tr` and $EDITOR (fallback `nvim`), all in the
    # standard Unix base. Remote source roots additionally need `ssh`,
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
    "ctrl-v" = "actions:open_v";
    "alt-o" = "actions:open_dir";
    "alt-d" = "actions:open_dir_tmux_h";
    "alt-v" = "actions:open_dir_tmux_v";
    "ctrl-t" = "actions:send_message";
    "alt-k" = "actions:kill";
    "alt-e" = "actions:browse_events";
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
    # current window. The pane sits to the right of tv (-h, a vertical
    # split) and starts in the repo dir (-c). It runs rushi-tui there.
    # The pane closes when rushi-tui exits. Outside tmux, or when the
    # tmux binary is missing, keep the original behavior: it runs `cd`
    # to the sessions root, then rushi-tui in this fork.
    #
    # A remote row (host: prefix) also opens a pane in the LOCAL tmux
    # server, never on the remote one. The pane runs
    # `ssh -t HOST "cd REPO && rushi-tui NAME"`: the remote TUI shows
    # up next to tv, and the pane closes when it exits. No tmux server
    # is needed on the remote host (the old behavior spawned a pane on
    # the remote server, invisible to the user). Outside local tmux the
    # fork runs the ssh command directly. The remote host needs
    # `rushi-tui` reachable by a non-interactive ssh.
    #
    # The shell body stays brace-free: television's string pipeline
    # treats a stray {group} as a template token and then leaves the
    # split placeholder unsubstituted. That is why the check reads
    # plain $TMUX instead of ${TMUX:-}.
    description = "Open this session in rushi-tui. Inside tmux, a new pane to the right of tv runs it from the session's repo dir. Outside tmux, it runs in this fork. Remote rows open the pane in the local tmux server: the pane runs the remote TUI over ssh -t.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then n=$(basename "$rp"); r=$(dirname "$(dirname "$rp")"); t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -h "ssh -t $host \"cd $r && rushi-tui \\\"$n\\\"\""; else ssh -t "$host" "cd $r && rushi-tui \"$n\""; fi; else n=$(basename "$s"); r=$(dirname "$(dirname "$s")"); t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -h -c "$r" "rushi-tui \"$n\""; else cd "$r" && rushi-tui "$n"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_v = {
    # Same as open, but the new pane stacks below tv instead of sitting
    # to the right. tmux's flags are easy to mix up: -v puts the new
    # pane below the current one (a horizontal divider). The remote-row
    # behavior is the same ssh -t local pane, with the -v split.
    description = "Open this session in rushi-tui. Inside tmux, a new pane below tv runs it from the session's repo dir. Outside tmux, it runs in this fork. Remote rows open the pane in the local tmux server: the pane runs the remote TUI over ssh -t.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then n=$(basename "$rp"); r=$(dirname "$(dirname "$rp")"); t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -v "ssh -t $host \"cd $r && rushi-tui \\\"$n\\\"\""; else ssh -t "$host" "cd $r && rushi-tui \"$n\""; fi; else n=$(basename "$s"); r=$(dirname "$(dirname "$s")"); t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -v -c "$r" "rushi-tui \"$n\""; else cd "$r" && rushi-tui "$n"; fi; fi' sh '{split:@TAB@:6}'
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
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; r=$(dirname "$(dirname "$rp")"); sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; if [ -n "$host" ]; then ssh -t "$host" "cd \"$r\" && exec $sh2"; else cd "$r" && exec "$sh2"; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_dir_tmux_h = {
    # Open a shell at the session's project dir in a tmux pane: the
    # directory that contains the sessions tree,
    # dirname(dirname(session_dir)). The shell is $SHELL, with a bash
    # fallback when $SHELL is unset.
    #
    # tv forks this action, so the fork inherits the pane environment.
    # When tv runs inside tmux, TMUX is set: create a new pane in the
    # current window. The pane sits to the right of tv (-h, a vertical
    # split) and starts in the repo dir (-c). It runs the shell there.
    # The pane closes when the shell exits. tmux runs the pane command
    # through its default shell, so the pane command is the plain shell
    # path. Outside tmux, or when the tmux binary is missing, the fork
    # cds to the repo dir and execs the shell; tv resumes when the
    # shell exits.
    #
    # A remote row (host: prefix) also opens a pane in the LOCAL tmux
    # server, never on the remote one. The pane runs
    # `ssh -t HOST "cd REPO && exec SHELL"`: a shell at the remote
    # project dir shows up next to tv. No remote tmux server is needed.
    # Outside local tmux the fork runs the ssh command directly. The
    # shell name is the local $SHELL value (bash fallback), matching the
    # old remote branch.
    #
    # The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them. The body stays
    # brace-free for the same reason as open: a stray {group} would
    # break the television template and leave the split placeholder
    # unsubstituted.
    description = "Open a $SHELL shell (bash fallback) in the session's project dir. Inside tmux, a new pane to the right of tv runs it. Outside tmux, it runs in this fork; tv resumes when the shell exits. Remote rows open the pane in the local tmux server: the pane runs the remote shell over ssh -t.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; r=$(dirname "$(dirname "$rp")"); sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; if [ -n "$host" ]; then c="cd $r && exec $sh2"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -h "ssh -t $host \"$c\""; else ssh -t "$host" "$c"; fi; else t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -h -c "$r" "$sh2"; else cd "$r" && exec "$sh2"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.open_dir_tmux_v = {
    # Same as open_dir_tmux_h, but the new pane stacks below tv instead
    # of sitting to the right. tmux's flags are easy to mix up: -v puts
    # the new pane below the current one (a horizontal divider). The
    # remote-row behavior is the same ssh -t local pane, with the -v
    # split. Outside tmux, the behavior is the same plain fork as
    # open_dir_tmux_h.
    #
    # The command keeps $vars plain (no ${...}) so the Nix
    # single-quote string does not interpolate them.
    description = "Open a $SHELL shell (bash fallback) in the session's project dir. Inside tmux, a new pane below tv runs it. Outside tmux, it runs in this fork; tv resumes when the shell exits. Remote rows open the pane in the local tmux server: the pane runs the remote shell over ssh -t.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; r=$(dirname "$(dirname "$rp")"); sh2=$SHELL; [ -n "$sh2" ] || sh2=bash; if [ -n "$host" ]; then c="cd $r && exec $sh2"; t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -v "ssh -t $host \"$c\""; else ssh -t "$host" "$c"; fi; else t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -v -c "$r" "$sh2"; else cd "$r" && exec "$sh2"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.send_message = {
    # Multi-line form: the repo TOML holds a multi-line basic string,
    # whose value carries a trailing newline. The multi-line Nix string
    # matches that value leaf for leaf.
    description = ''
      Open $EDITOR (falling back to nvim) on a message file, then send the result to the session with `rushi run <session> <msg>`: it starts the loop detached when the session is idle, and appends with `--no-run` when the loop is live. An empty message cancels.
    '';
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then p=$(ssh -T "$host" cat "$rp/loop.pid" 2>/dev/null); else p=$(cat "$rp/loop.pid" 2>/dev/null); fi; p=$(printf "%s" "$p" | tr -d "[:space:]"); live=no; if [ -n "$p" ]; then if [ -n "$host" ]; then ssh -T "$host" kill -0 "$p" 2>/dev/null && live=yes; else kill -0 "$p" 2>/dev/null && live=yes; fi; fi; d=$TMPDIR; [ -n "$d" ] || d=/tmp; f=$(mktemp "$d/tv-rushi-msg.XXXXXX"); st=$(stty -g 2>/dev/null); ed=$EDITOR; [ -n "$ed" ] || ed=nvim; $ed "$f"; [ -n "$st" ] && stty "$st" 2>/dev/null || stty raw -echo 2>/dev/null; m=$(cat "$f" 2>/dev/null); rm -f "$f"; if [ -z "$(printf "%s" "$m" | tr -d "[:space:]")" ]; then echo "No message entered, nothing sent."; exit 0; fi; if [ -n "$host" ]; then m64=$(printf "%s" "$m" | base64 | tr -d "\n"); if [ "$live" = yes ]; then ssh -T "$host" "rushi run \"$rp\" \"\$(printf %s $m64 | base64 -d)\" --no-run"; else setsid ssh -T "$host" "rushi run \"$rp\" \"\$(printf %s $m64 | base64 -d)\"" </dev/null >/dev/null 2>&1 & fi; else if [ "$live" = yes ]; then rushi run "$rp" "$m" --no-run; else setsid rushi run "$rp" "$m" </dev/null >/dev/null 2>&1 & fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.browse_events = {
    # Fuzzy-search this session's events.jsonl in a nested `tv` (the
    # `rushi-sessions-events` channel). The nested tv reads the
    # session dir's events.jsonl from its CWD, so it must start
    # there.
    #
    # tv forks this action, so the fork inherits the pane environment.
    # When tv runs inside tmux, TMUX is set: create a new pane in the
    # current window. The pane sits to the right of tv (-h, a vertical
    # split) and starts in the session dir (-c). It runs the nested
    # tv there. The pane closes when the nested tv exits. Outside
    # tmux, or when the tmux binary is missing, the fork cds to the
    # session dir and runs the nested tv here. The outer tv pauses
    # and resumes when the nested tv exits.
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
    description = "Fuzzy-search this session's events.jsonl in a nested tv (the rushi-sessions-events channel). Inside tmux, a new pane to the right of tv runs it from the session dir. Outside tmux, it runs in this fork. Remote rows run it over ssh.";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then ssh -t "$host" "cd \"$rp\" && tv rushi-sessions-events"; else t=$TMUX; if [ -n "$t" ] && command -v tmux >/dev/null; then tmux split-window -h -c "$rp" "tv rushi-sessions-events"; else cd "$rp" && tv rushi-sessions-events; fi; fi' sh '{split:@TAB@:6}'
    '';
  };

  actions.kill = {
    description = "Stop the session loop (SIGTERM to the loop pid)";
    shell = "bash";
    mode = "fork";
    command = untab ''
      sh -c 's="$1"; host=""; rp="$s"; h=$(printf "%s" "$s" | cut -d: -f1); if [ "$h" != "$s" ]; then case "$h" in */*) : ;; *) host="$h"; rp=$(printf "%s" "$s" | cut -d: -f2-);; esac; fi; if [ -n "$host" ]; then p=$(ssh -T "$host" cat "$rp/loop.pid" 2>/dev/null); p=$(printf "%s" "$p" | tr -d "[:space:]"); if [ -n "$p" ] && ssh -T "$host" kill -0 "$p" 2>/dev/null; then ssh -T "$host" kill -TERM "$p"; echo "sent SIGTERM to $p ("$(basename "$rp")") on $host"; else echo "no live loop for "$(basename "$rp")" on $host (stale or absent pid)"; fi; else p=$(cat "$rp/loop.pid" 2>/dev/null); p=$(printf "%s" "$p" | tr -d "[:space:]"); if [ -n "$p" ] && kill -0 "$p" 2>/dev/null; then kill -TERM "$p"; echo "sent SIGTERM to $p ("$(basename "$rp")")"; else echo "no live loop for "$(basename "$rp")" (stale or absent pid)"; fi; fi' sh '{split:@TAB@:6}'
    '';
  };
}
