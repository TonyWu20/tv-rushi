{
  description = "rushi television channel: rushi-sessions (channel data, backend binary, home-manager module)";

  # Standalone use fetches nixpkgs. A consuming flake pins its own copy:
  #   tv-rushi = { url = "..."; inputs.nixpkgs.follows = "nixpkgs"; };
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  # Rust toolchain for the `rushi-sessions` backend binary. Follows the
  # top-level nixpkgs so a consumer keeps one nixpkgs for everything.
  inputs.fenix = {
    url = "github:nix-community/fenix";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs @ { self, nixpkgs, fenix, ... }:
    let
      pkgLib = nixpkgs.lib;

      # The channel is one data file. Expose it as a package so a
      # home-manager config can install it verbatim into the cable dir:
      #   home.file.".config/television/cable/rushi-sessions.toml".source
      #     = inputs.tv-rushi.packages."x86_64-linux".rushi-sessions-channel;
      toml = pkgLib.readFile ./rushi-sessions.toml;

      perSystem = system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ fenix.overlays.default ];
          };
          rustToolchain = fenix.packages.${system}.stable.withComponents [
            "cargo"
            "rust-src"
            "rustc"
          ];

          # One data file, the manual install path into the cable dir.
          channelFile = pkgs.writeText "rushi-sessions-television-channel" toml;

          # The `rushi-sessions` backend binary. The source honors
          # .gitignore (sessions/ and target/ stay out of the store).
          # The binary hard-depends on `fd` at run time (the channel's
          # requirements list it).
          rushiSessions = pkgs.rustPlatform.buildRustPackage {
            pname = "rushi-sessions";
            version = "0.1.0";
            src = pkgLib.cleanSource ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
            };
            nativeBuildInputs = [ rustToolchain ];
            cargoBuildFlags = [ "--workspace" ];
            doCheck = false;
            meta = with pkgLib; {
              description = "rushi-sessions television channel backend";
              homepage = "https://github.com/TonyWu20/tv-rushi";
              license = licenses.mit;
              mainProgram = "rushi-sessions";
            };
          };
        in
        {
          rushi-sessions-channel = channelFile;
          default = channelFile;
          # The backend binary. The home-manager module (below) adds it to
          # home.packages.
          rushi-sessions = rushiSessions;
        };
    in
    {
      packages = {
        x86_64-linux = perSystem "x86_64-linux";
        aarch64-linux = perSystem "aarch64-linux";
        aarch64-darwin = perSystem "aarch64-darwin";
      };

      # The home-manager module wires the channel into
      # programs.television.channels."rushi-sessions" and adds the binary to
      # home.packages. It is partially applied with the per-system binary
      # derivation, so the module is self-contained: importing it is the
      # whole integration.
      #
      # The set is keyed by system, so a consumer picks its own:
      #   tv-rushi.homeManagerModules."x86_64-linux".default
      #   tv-rushi.homeManagerModules."aarch64-darwin".default
      # There is deliberately no bare `default`: it would have to pick one
      # system's binary and would silently install the wrong one elsewhere.
      homeManagerModules = pkgLib.genAttrs [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ] (system: {
        default = (import ./home-module.nix) {
          bin = (perSystem system)."rushi-sessions";
        };
      });
    };
}
