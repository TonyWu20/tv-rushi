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
    description = "The scan roots for the rushi-sessions channel. The channel always exposes a CWD view (no roots). When roots are set it also exposes a Local view (local roots only) and an All view (local + remote roots). An entry can be a local path or a remote path in the form host:/path or user@host:/path. A remote entry must be reachable by an ssh alias. The shell expands a leading ~. Paths with spaces are not supported.";
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ bin ];
    # One channel. Its source commands cover the CWD, local-only, and
    # local + remote views (see rushi-sessions-channel.nix). It is
    # registered even with empty roots: it then exposes the CWD view only.
    programs.television.channels."rushi-sessions" =
      (import ./rushi-sessions-channel.nix) { sourceRoots = cfg.sourceRoots; };
  };
}
