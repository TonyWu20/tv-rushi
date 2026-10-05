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
#     attrset (rushi-sessions-channel.nix), called with no roots: the main
#     channel scans the CWD (the tv [PATH] argument). The home-manager
#     television module serializes it to
#     ~/.config/television/cable/rushi-sessions.toml;
#   - declares `programs."rushi-sessions".sourceRoots` (a list of
#     directories, default empty) and `programs."rushi-sessions".allChannel`
#     (bool, default true). When the list is non-empty, a second channel
#     `rushi-sessions-all` scans the configured trees.
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
    description = "The rushi-sessions television channel: a peek at rushi sessions (status, loop phase, last activity) with open/tail/kill actions.";
  };

  options.programs."rushi-sessions".sourceRoots = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = "The directories the rushi-sessions-all channel scans for session trees. Empty means that channel is not registered. The main rushi-sessions channel always scans the CWD (the tv [PATH] argument). Example: [ \"/export\" \"~/Downloads\" ]. The shell expands a leading ~ when the command runs. Roots with spaces are not supported.";
  };

  options.programs."rushi-sessions".allChannel = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Register a second channel rushi-sessions-all that scans the directories in sourceRoots. It is registered only when sourceRoots is non-empty. The main channel always scans the CWD (the tv [PATH] argument).";
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ bin ];
    programs.television.channels = {
      # The main channel always scans the CWD (the tv [PATH] argument).
      "rushi-sessions" =
        (import ./rushi-sessions-channel.nix) { sourceRoots = [ ]; };
    } // lib.optionalAttrs (cfg.allChannel && cfg.sourceRoots != [ ]) {
      # The -all channel scans the configured trees. It is absent when
      # sourceRoots is empty: it would then duplicate the main channel.
      "rushi-sessions-all" = let
        ch = (import ./rushi-sessions-channel.nix) { sourceRoots = cfg.sourceRoots; };
      in ch // {
        metadata = ch.metadata // {
          name = "rushi-sessions-all";
          description = "Peek at rushi sessions under the configured sourceRoots";
        };
      };
    };
  };
}
