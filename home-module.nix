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
#     attrset (rushi-sessions-channel.nix). The home-manager television
#     module serializes it to ~/.config/television/cable/rushi-sessions.toml.
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
#     programs."rushi-sessions".enable = true;
#   };
#
# The module is partially applied with the per-system binary derivation
# (see flake.nix `homeManagerModules`), so `bin` is bound here.

{ bin, ... }:

{ config, lib, ... }:

let
  cfg = config.programs."rushi-sessions";
  channel = import ./rushi-sessions-channel.nix;
in
{
  options.programs."rushi-sessions".enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "The rushi-sessions television channel: a peek at rushi sessions (status, loop phase, last activity) with open/tail/kill actions.";
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ bin ];
    programs.television.channels."rushi-sessions" = channel;
  };
}
