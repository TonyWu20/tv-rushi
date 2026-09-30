{
  description = "television cable channel that lists rushi sessions at a glance";

  # Standalone use fetches nixpkgs. A consuming flake pins its own copy:
  #   tv-rushi = { url = "..."; inputs.nixpkgs.follows = "nixpkgs"; };
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = inputs @ { self, nixpkgs, ... }:
    let
      # The channel is one data file. Expose it as a package so a
      # home-manager config can install it verbatim into the cable dir:
      #   home.file.".config/television/cable/rushi-sessions.toml".source
      #     = inputs.tv-rushi.packages."x86_64-linux".rushi-sessions-channel;
      toml = builtins.readFile ./rushi-sessions.toml;
      perSystem = system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          # one data file, exposed as a store path a home-manager config can
          # install verbatim into the cable dir.
          c = pkgs.writeText "rushi-sessions-television-channel" toml;
        in
        {
          rushi-sessions-channel = c;
          default = c;
        };
    in
    {
      packages = {
        x86_64-linux = perSystem "x86_64-linux";
        aarch64-linux = perSystem "aarch64-linux";
      };
    };
}
