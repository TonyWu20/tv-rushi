# tv-rushi

`tv-rushi` is a [television](https://github.com/alexpasmantier/television)
channel for the rushi Unix agent harness. It gives a quick-peek window over
rushi sessions. Each entry shows the session status, repo, last loop phase,
and last activity. The preview panel shows a TOML status card. The card lists
the last user and assistant messages and the recent events.

## Features

- List rushi sessions under a directory. Active sessions sort first.
- Show a status card in the preview panel. `bat` colors it when present.
- Open a session in `rushi-tui` with ctrl-e.
- Edit a message in `$EDITOR` (fallback `nvim`) and send it with ctrl-t.
  Idle: the loop starts detached. Live: `--no-run` appends.
- Stop the session loop with ctrl-shift-k (SIGTERM).

## Requirements

- [television](https://github.com/alexpasmantier/television)
- `fd`, a hard dependency of the backend binary
- the `rushi-sessions` binary, which this repo builds
- `bat`, optional. The preview prints plain TOML without it.
- `rushi` and `rushi-tui` on PATH, for the `send_message` and `open`
  actions. Nix builds and installs these with the `rushi` package.
- `setsid` and `stty`, which `send_message` uses to detach the loop and
  to restore the terminal. Both ship in the standard Unix base.
- `$EDITOR` (fallback `nvim`) plus `mktemp` and `tr`, which `send_message`
  uses for the editor, temp file, and empty check. These resolve on PATH.

## Usage

```sh
tv rushi-sessions                  # scan the current directory
tv rushi-sessions ~/programming    # scan a given directory
tv rushi-sessions /                # full disk scan (slow)
```

The directory is television's `[PATH]` argument. television changes into it
before the source command runs. Omit it to scan the current directory.

To watch a fixed set of trees, set the home-manager option
`programs."rushi-sessions".sourceRoots`:

```nix
programs."rushi-sessions" = {
  enable = true;
  sourceRoots = [ "/export" "/home/tony/programming" "~/Downloads" ];
};
```

That registers a `rushi-sessions-all` channel (the
`programs."rushi-sessions".allChannel` option, default `true`). It
scans the configured trees and is registered only when `sourceRoots`
is non-empty. The main channel stays CWD-driven, so `tv rushi-sessions
DIR` still scans DIR.

The binary alone takes any number of roots:

```sh
rushi-sessions source /export ~/programming
```

Keybindings:

| key            | action                                              |
| -------------- | --------------------------------------------------- |
| ctrl-e         | Open the session in `rushi-tui`                     |
| ctrl-t         | Edit a message in `$EDITOR` (fallback `nvim`), then send it |
| ctrl-shift-k   | Send SIGTERM to the session loop                    |

## Install

### home-manager module (recommended)

This repo exposes a home-manager module. It adds the `rushi-sessions` binary
to `home.packages` and registers the channel:

```nix
inputs.tv-rushi.url = "github:TonyWu20/tv-rushi";
# ...
home-manager.users.tony = {
  homeModules = [ inputs.tv-rushi.homeManagerModules."x86_64-linux".default ];
  programs."rushi-sessions" = {
    enable = true;
    sourceRoots = [ ]; # empty: no rushi-sessions-all channel
  };
};
```

The host config must also load the television home-manager module. The module
serializes the channel to `~/.config/television/cable/rushi-sessions.toml`.

The module set is keyed by system. Supported systems: `x86_64-linux`,
`aarch64-linux`, and `aarch64-darwin`. Pick the key for your system. There
is no bare `default`. A Mac uses this form:

```nix
homeModules = [ inputs.tv-rushi.homeManagerModules."aarch64-darwin".default ];
```

### Manual copy

```sh
cp rushi-sessions.toml ~/.config/television/cable/rushi-sessions.toml
```

The copy needs `fd` and `rushi-sessions` on PATH. Build the binary from this
repo with `cargo build --release`, or with the flake:

```sh
nix build .#rushi-sessions
```

### Nix flake

The flake exposes the channel as a single-file package:

```sh
nix build .#rushi-sessions-channel   # a store path holding the channel TOML
```

A nixos-config can install that file through home-manager:

```nix
home.file.".config/television/cable/rushi-sessions.toml".source =
  inputs.tv-rushi.packages."x86_64-linux".rushi-sessions-channel;
```

The input pins nixpkgs with `follows`. It reuses the config's own nixpkgs.

## Development

```sh
cargo build --workspace
cargo test --workspace
```

## Layout

| path                          | role                                |
| ---------------------------- | ----------------------------------- |
| `rushi-sessions.toml`        | the television channel (TOML)      |
| `bin/rushi-sessions/`        | the backend binary (Rust)          |
| `rushi-sessions-channel.nix` | the channel as a Nix attrset       |
| `home-module.nix`            | the home-manager module            |
| `flake.nix`                  | exposes the channel, binary, module|
| `docs/television-integration.md` | design notes and decision log |
