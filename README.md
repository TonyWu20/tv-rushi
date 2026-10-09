# tv-rushi

`tv-rushi` is a [television](https://github.com/alexpasmantier/television)
channel for the rushi Unix agent harness. It gives a quick-peek window over
rushi sessions. Each entry shows the session status, repo, last loop phase,
and last activity. The preview panel shows a TOML status card. The card lists
the last user and assistant messages and the recent events. A second channel,
`rushi-sessions-events`, fuzzy-searches one session's `events.jsonl` entries
and previews each entry as a markdown or toml document. The `browse_events`
action (alt-e) launches it from the session list.

## Features

- List rushi sessions under a directory. Active sessions sort first.
- Show a status card in the preview panel. `bat` colors it when present.
- Open a session in `rushi-tui` with ctrl-e.
- Edit a message in `$EDITOR` (fallback `nvim`) and send it with ctrl-t.
  The send runs in the session's recorded working dir (the `cwd`
  file, so a git worktree session stays in the worktree). Idle:
  the loop starts detached. Live: `rushi run` appends (the kernel
  branches on the session lock).
- Stop the session loop with alt-k (SIGTERM).
- Fuzzy-search a session's `events.jsonl` with alt-e. A nested
  `tv rushi-sessions-events` lists the entries and previews the
  selected one. The default view lists the user and assistant
  messages, newest first. Ctrl+S cycles to the `All` view (every
  entry). Inside tmux, a pane to the right of tv runs it.
  Outside tmux, it runs in this terminal. Remote rows run it over
  ssh. The previews drop the id of a user message and the call ids
  of a tool call, and every timestamp shows as local time. The view
  also binds ctrl-t to its own `send_message`, which sends a
  message to the session it was launched from.

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
- `ssh`, only for remote source roots. Each remote host also needs
  `rushi-sessions`, `rushi` and `rushi-tui` reachable by the
  non-interactive ssh. The open actions run the remote TUI over
  `ssh -t` in a local pane. The open_dir_tmux actions open a
  local pane that runs the remote shell over `ssh -t`, so no
  remote tmux is needed. A Nix profile on the remote is not on PATH, so set
  `RUSHI_SESSIONS_REMOTE_BIN` on the remote to name the binary, or use an
  absolute path.
- `tv` itself, for the `browse_events` action. It launches the nested
  events channel. The action resolves it on PATH, like the other
  interactive actions. A remote row needs the events channel file on
  the remote host. The same flake deployment carries it.

## Usage

```sh
tv rushi-sessions                  # scan the current directory
tv rushi-sessions ~/programming    # scan a given directory
tv rushi-sessions /                # full disk scan (slow)
```

The events channel takes the session dir (the dir that holds
`events.jsonl`):

```sh
tv rushi-sessions-events DIR       # DIR is a session dir
tv rushi-sessions-events           # the CWD is the session dir
```

The directory is television's `[PATH]` argument. television changes into it
before the source command runs. Omit it to scan the current directory.

To watch a fixed set of trees, set the home-manager option
`programs."rushi-sessions".sourceRoots`:

```nix
programs."rushi-sessions" = {
  enable = true;
  sourceRoots = [ "/export" "build:/export" "~/Downloads" ];
};
```

The events channel preview pipes each entry document into a renderer.
The options are strings, default `bat`:

```nix
programs."rushi-sessions" = {
  eventPreviewer = "mdcat --ansi";    # markdown documents
  eventTomlPreviewer = "bat";         # toml documents
};
```

A value other than `bat` is one command line, run through `sh -c` and
reading the document from stdin. Pipe filters only. `mdfried` does not
fit. It is a fullscreen TUI viewer, and the preview panel captures
stdout.

The events channel carries no reload by default: a reload resets the
preview panel's scroll position, which blocks reading a rendered
message. Television's CLI flag `tv rushi-sessions-events --watch N`
enables the reload for one run.

A root may be local or remote. A remote root is `host:/path` or
`user@host:/path`, where `host` is an ssh alias. The binary runs the
remote `rushi-sessions` over ssh and marks those rows with a `host:`
The actions read the prefix: the open and open_dir_tmux actions split
panes in the local tmux server that run the remote TUI or shell over
`ssh -t`. The send_message and kill actions act over `ssh -T`. In the
example, `build:/export` scans the `build` host.

The channel is always registered. It exposes one source command per ROOT
set, cycled with `Ctrl+S` in the order `Local`, `All`, `CWD`. `CWD`
(no roots, the current directory) is always present and last. When
`sourceRoots` is non-empty, two more views are added: `Local` scans the
local roots and `All` scans the local + remote roots. `tv rushi-sessions
DIR` still scans DIR via the `CWD` view.

The binary alone takes any number of roots:

```sh
rushi-sessions source /export ~/programming
```

Keybindings:

| key            | action                                              |
| -------------- | --------------------------------------------------- |
| ctrl-e         | Open the session in `rushi-tui`                     |
| ctrl-t         | Edit a message in `$EDITOR` (fallback `nvim`), then send it |
| alt-k          | Send SIGTERM to the session loop                    |
| alt-e          | Fuzzy-search the session's `events.jsonl` in a nested `tv` |

The events channel binds the same `ctrl-t` to its own `send_message`.
It acts on the session dir the channel was launched from (its CWD).

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
    sourceRoots = [ ]; # empty: only the CWD view
  };
};
```

The host config must also load the television home-manager module. The module
serializes the channels to
`~/.config/television/cable/rushi-sessions.toml` and
`~/.config/television/cable/rushi-sessions-events.toml`.

The module set is keyed by system. Supported systems: `x86_64-linux`,
`aarch64-linux`, and `aarch64-darwin`. Pick the key for your system. There
is no bare `default`. A Mac uses this form:

```nix
homeModules = [ inputs.tv-rushi.homeManagerModules."aarch64-darwin".default ];
```

### Manual copy

```sh
cp rushi-sessions.toml ~/.config/television/cable/rushi-sessions.toml
cp rushi-sessions-events.toml ~/.config/television/cable/rushi-sessions-events.toml
```

The copy needs `fd` and `rushi-sessions` on PATH. Build the binary from this
repo with `cargo build --release`, or with the flake:

```sh
nix build .#rushi-sessions
```

### Nix flake

The flake exposes each channel as a single-file package:

```sh
nix build .#rushi-sessions-channel          # a store path holding the channel TOML
nix build .#rushi-sessions-events-channel   # the events channel TOML
```

A nixos-config can install those files through home-manager:

```nix
home.file.".config/television/cable/rushi-sessions.toml".source =
  inputs.tv-rushi.packages."x86_64-linux".rushi-sessions-channel;
home.file.".config/television/cable/rushi-sessions-events.toml".source =
  inputs.tv-rushi.packages."x86_64-linux".rushi-sessions-events-channel;
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
