# The tv-rushi home-manager module.
#
# Wires the rushi-sessions television channel into a home-manager config,
# doing what the kernel's `programs.rushi.enableTelevisionIntegration` used to
# do, but owned by this repo:
#
#   - adds the `rushi-sessions` backend binary to `home.packages` so it is on
#     the user's PATH (the channel lists `fd` and `rushi-sessions` as
#     requirements, both resolved on PATH);
#   - sets `programs.television.channels."rushi-sessions"` to the channel
#     attrset (rushi-sessions-channel.nix), called with the configured
#     `sourceRoots`. The channel exposes one source command per ROOT set,
#     cycled in the order `Local`, `All`, `CWD`. `CWD` scans no roots and
#     is always present, last. The home-manager television module
#     serializes it to
#     ~/.config/television/cable/rushi-sessions.toml;
#   - declares `programs."rushi-sessions".sourceRoots` (a list of paths,
#     default empty). Local and remote roots mix in the same list.
#   - registers the rushi-sessions-events channel (the fuzzy search
#     over one session's events.jsonl) and the `eventPreviewer` /
#     `eventTomlPreviewer` options that pick its preview renderers.
#     The `eventViewer` / `eventTomlViewer` options pick the
#     viewers of its `view_message` action (bound to enter):
#     markdown entries go to `eventViewer`, toml entries go to
#     `eventTomlViewer`.
#
# This module does not declare `programs.television` itself. The host config
# must also load the television home-manager module (the one that declares
# `programs.television` and writes the cable dir). Without it, evaluation
# fails with the standard "The option programs.television does not exist"
# error.
#
# Usage from a flake:
#   inputs.tv-rushi.url = "github:TonyWu20/tv-rushi";
#   ...
#   home-manager.users.tony = {
#     ...
#     homeModules = [ inputs.tv-rushi.homeManagerModules."x86_64-linux".default ];
#     programs."rushi-sessions" = {
#       enable = true;
#       sourceRoots = [ "/export" "~/Downloads" ];
#     };
#   };
#
# The module is partially applied with the per-system binary derivation
# (see flake.nix `homeManagerModules`), so `bin` is bound here.

{ bin, ... }:

{ config, lib, ... }:

let
  cfg = config.programs."rushi-sessions";
in
{
  options.programs."rushi-sessions".enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "The rushi-sessions television channel: a peek at rushi sessions (status, loop phase, last activity) with open, send_message, new_session, kill and browse_events actions.";
  };

  options.programs."rushi-sessions".sourceRoots = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = "The scan roots for the rushi-sessions channel. The channel always exposes a CWD view (no roots). When roots are set it also exposes a Local view (local roots only) and an All view (local + remote roots). An entry can be a local path or a remote path in the form host:/path or user@host:/path. A remote entry must be reachable by an ssh alias. The shell expands a leading ~. Paths with spaces are not supported.";
  };

  options.programs."rushi-sessions".eventPreviewer = lib.mkOption {
    type = lib.types.str;
    default = "bat";
    description = "The pipe command the rushi-sessions-events channel uses to render the markdown documents (user_message, assistant_message, compaction_summary) in the preview panel. It reads the document from stdin. The default bat keeps the per-entry language flag, the RUSHI_PREVIEW_THEME override and the plain-text fallback when bat is absent. Any other value is one command line, for example mdcat or glow.";
  };

  options.programs."rushi-sessions".eventTomlPreviewer = lib.mkOption {
    type = lib.types.str;
    default = "bat";
    description = "The pipe command the rushi-sessions-events channel uses to render the toml documents (tool_call, tool_result and every other entry) in the preview panel. It reads the document from stdin. See eventPreviewer for the bat baseline and the custom-value semantics.";
  };

  options.programs."rushi-sessions".eventViewer = lib.mkOption {
    type = lib.types.str;
    default = "mdfried";
    description = "The markdown viewer of the rushi-sessions-events channel's view_message action (bound to enter). The fork pipes a markdown entry's document (user_message, assistant_message and compaction_summary) into the viewer, and the viewer takes over the pane until it exits. The value is one command line that reads the document from stdin, like eventPreviewer. The default mdfried is a fullscreen markdown viewer. Pick another viewer, for example glow or mdcat.";
  };

  options.programs."rushi-sessions".eventTomlViewer = lib.mkOption {
    type = lib.types.str;
    default = "bat";
    description = "The toml viewer of the rushi-sessions-events channel's view_message action (bound to enter). The fork pipes a toml entry's document (tool_call, tool_result and every other entry) into the viewer, and the viewer takes over the pane until it exits. The value is one command line that reads the document from stdin, like eventPreviewer. The default is the plain bat pipe. Set another command line, for example `bat -p`, for a paged view.";
  };


  config = lib.mkIf cfg.enable {
    home.packages = [ bin ];
    # One channel. Its source commands cover the CWD, local-only, and
    # local + remote views (see rushi-sessions-channel.nix). It is
    # registered even with empty roots: it then exposes the CWD view only.
    programs.television.channels."rushi-sessions" =
      (import ./rushi-sessions-channel.nix) { sourceRoots = cfg.sourceRoots; };
    # The events channel fuzzy-searches one session's events.jsonl.
    # It is registered next to the main channel: the browse_events
    # action (alt-e) of the main channel launches it with the session
    # dir as the tv [PATH] argument. The renderer options feed its
    # preview command. The viewer options feed its view_message
    # action.
    programs.television.channels."rushi-sessions-events" =
      (import ./rushi-sessions-events-channel.nix) {
        eventPreviewer = cfg.eventPreviewer;
        eventTomlPreviewer = cfg.eventTomlPreviewer;
        eventViewer = cfg.eventViewer;
        eventTomlViewer = cfg.eventTomlViewer;
      };
  };
}
