//! `rushi-sessions` — the television-channel backend.
//!
//! Replaces the inline Python embedded in the `rushi-sessions` television
//! channel (`rushi-sessions-channel.nix` in this repo). Four subcommands:
//!
//! - `rushi-sessions source [ROOT...]`
//!   Scan for rushi session directories under the given roots and print
//!   one TSV row per session. Roots may be local or remote. A remote
//!   root is `host:/path` or `user@host:/path` (the host is an ssh
//!   alias). Each distinct host runs the remote `rushi-sessions source`
//!   over ssh once; its rows join the list with the repo and path columns
//!   prefixed by `host:`. An unreachable host is skipped with a
//!   stderr warning.:
//!   `status<TAB>name<TAB>repo<TAB>phase<TAB>last<TAB>mtime<TAB>path`.
//!   The roots are directories. Omit them to scan the CWD. Relative
//!   roots join the CWD. Missing roots are skipped with a warning on
//!   stderr. Overlapping roots (one inside another) dedupe by
//!   canonical path. The television channel's `display`/`output` split
//!   on that tab. `--active-only` keeps only the sessions whose loop
//!   pid is alive (ACTIVE). The channel exposes it as a second,
//!   cycling source command.
//!
//! - `rushi-sessions preview <status> <name> <repo> <phase> <last> <mtime> <path>`
//!   Render the TOML preview card for one session: the last user/assistant
//!   messages, the context window usage, and the most recent events. When
//!   `bat` is on PATH the card is
//!   colored with it ($RUSHI_PREVIEW_THEME selects the theme, default
//!   "Catppuccin Macchiato"); otherwise plain TOML is printed.
//! - `rushi-sessions events [DIR] [--chat]`
//!   Print one TSV row per entry of the session's events.jsonl, newest
//!   first. Columns: seq, tag (the short marker of the entry type), ts,
//!   summary, and the tree-depth indent (the TUI tree view's box
//!   connector, empty on the trunk). `--chat` keeps only the active-path
//!   user and assistant messages. A row masked by a rewind marker carries
//!   a `[masked]` summary prefix in the full view.
//!
//! - `rushi-sessions rewind [DIR] SEQ [--events]`
//!   Append a `rewind` marker (the tree view's "Rewind without summary")
//!   through the kernel's `log` binary. A user-message target rewinds
//!   `before` it, every other target rewinds `on` it. Needs an idle
//!   loop (the kernel's `.loop.lock` probe). With `--events` the CWD is
//!   the session dir.
//!
//! - `rushi-sessions summarize [DIR] SEQ [--events] [--prompt]`
//!   Summarize the abandoned branch of the last top-level rewind through
//!   the kernel's `compact --branch` (the tree view's "Summarize the
//!   branch"). The picked SEQ must sit on the active path; the loop must
//!   be idle. With `--prompt`, the instruction is typed in the editor on
//!   a .md file first (a blank file cancels) and passed as `--prompt`.
//!   With `--events` the CWD is the session dir.
//!
//! - `rushi-sessions event-preview DIR SEQ [--print-lang]`
//!   Render one events.jsonl entry as a document: markdown for message
//!   entries, toml for everything else. `--print-lang` prints only the
//!   document language.
//!
//! - `rushi-sessions copy-entry DIR SEQ`
//!   Copy one entry's full document (newlines preserved) to the
//!   clipboard. The OSC 52 terminal clipboard escape goes to the
//!   controlling tty first. A local clipboard tool (pbcopy,
//!   termux-clipboard-set, wl-copy, xclip, xsel) takes the pipe
//!   when one is on PATH. Without either route the command exits
//!   1 with a stderr line.
//!
//! `source` calls `fd` for the directory walk (a hard dependency of the
//! channel) and replaces the channel's previous `python3` reader.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod branch;
mod doc;
mod events;
mod format;
mod preview;
mod prompt;
mod remote;
mod scan;
mod state;
mod tree;
mod usage;
mod yank;

#[derive(Parser)]
#[command(
    name = "rushi-sessions",
    about = "rushi-sessions television channel backend",
    version
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan for rushi sessions under the given roots and print TSV rows.
    /// Without roots, the CWD is the only root.
    Source {
        /// Print only sessions whose loop pid is alive (ACTIVE status).
        #[arg(long)]
        active_only: bool,
        /// Roots to scan for session directories.
        #[arg(value_name = "ROOT")]
        roots: Vec<PathBuf>,
    },
    /// Render the TOML preview card for one session.
    Preview {
        status: String,
        name: String,
        repo: String,
        phase: String,
        last: String,
        /// Epoch seconds of the reference file; carried by the channel,
        /// not used by the card.
        mtime: String,
        /// Session directory path.
        path: String,
    },
    /// Print one TSV row per entry of the session's events.jsonl.
    /// Columns: seq, tag (the short marker of the entry type), ts, summary, and the tree-depth indent. The rows come newest first.
    /// Without DIR, the CWD is the session dir. `--chat` keeps the
    /// active-path user and assistant messages. Rows masked by a
    /// rewind marker carry a `[masked]` summary prefix.
    Events {
        /// The session dir (the dir that holds events.jsonl).
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Only the user and assistant messages.
        #[arg(long)]
        chat: bool,
    },
    /// Open the new-session prompt: the editor on a temp file, then
    /// the create step (a detached `rushi run`). When tv runs inside
    /// tmux 3.8+, the prompt opens in a floating pane over tv.
    NewSession {
        /// The session row: the path, or host:path for a remote row.
        row: String,
    },
    /// Open the send-message prompt: the editor on a temp file, then
    /// the send step (a detached `rushi run`). With `--events` the
    /// session dir is the channel CWD; else it is the row.
    SendMessage {
        /// The session row: the path, or host:path for a remote row.
        row: Option<String>,
        /// Use the CWD as the session dir (the events channel).
        #[arg(long)]
        events: bool,
    },
    /// Append a `rewind` marker to the session (the tree view's
    /// "Rewind without summary"). SEQ is the picked entry's 1-based
    /// log seq: a user-message target rewinds `before` it (the
    /// message leaves the context), every other target rewinds `on`
    /// it. Needs an idle loop. The marker is appended through the
    /// kernel's `log` binary (the sibling of `rushi`). The session
    /// dir is the CWD (the events channel) or `--dir`.
    Rewind {
        /// The picked entry's 1-based log seq (the row token).
        seq: u64,
        /// The session dir. Absent: the CWD (the events channel).
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// Summarize the abandoned branch of the last top-level rewind
    /// through the kernel's `compact --branch` (the tree view's
    /// "Summarize the branch"). The picked SEQ must sit on the
    /// active path; the loop must be idle. With `--prompt`, the
    /// instruction is typed in the editor on a .md file first (a
    /// blank file cancels) and passed as `--prompt`. The session
    /// dir is the CWD (the events channel) or `--dir`.
    Summarize {
        /// The picked entry's 1-based log seq (the row token).
        seq: u64,
        /// The session dir. Absent: the CWD (the events channel).
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Open the editor for a custom summarize instruction.
        #[arg(long)]
        prompt: bool,
    },
    /// [hidden] The floating-pane stage of the prompt actions. It
    /// reads its inputs from the JSON args file, runs the editor,
    /// then the create/send step.
    #[command(hide = true)]
    RunPane {
        /// The JSON args file (created by stage 1, deleted here).
        #[arg(long)]
        args: PathBuf,
    },
    /// Render one events.jsonl entry as a document: markdown for
    /// message entries, toml for everything else. SEQ is the 1-based
    /// line number of the entry in events.jsonl.
    EventPreview {
        /// The session dir (the dir that holds events.jsonl).
        dir: PathBuf,
        /// The 1-based line number of the entry in events.jsonl.
        seq: u64,
        /// Print only the document language ("markdown" or "toml").
        #[arg(long)]
        print_lang: bool,
    },
    /// Copy one entry's full document (newlines preserved) to the
    /// clipboard. SEQ is the 1-based line number of the entry in
    /// events.jsonl. The OSC 52 escape goes to the controlling tty
    /// first; a local clipboard tool takes the pipe when one is on
    /// PATH.
    CopyEntry {
        /// The session dir (the dir that holds events.jsonl).
        dir: PathBuf,
        /// The 1-based line number of the entry in events.jsonl.
        seq: u64,
    },
}

fn main() {
    // Behave like a normal Unix program when stdout is a pipe that the
    // reader closes early (e.g. `rushi-sessions source | head`): restore
    // the default SIGPIPE disposition so a write to a broken pipe ends the
    // process silently instead of Rust's default EPIPE panic trace.
    #[cfg(unix)]
    {
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
    }

    let args = Args::parse();
    match args.command {
        Command::Source { active_only, roots } => scan::source(active_only, roots),
        Command::Preview {
            status,
            name,
            repo,
            phase,
            last,
            mtime,
            path,
        } => preview::preview(&status, &name, &repo, &phase, &last, &mtime, &path),
        Command::Events { dir, chat } => events::events(dir, chat),
        Command::NewSession { row } => std::process::exit(prompt::new_session(&row)),
        Command::SendMessage { row, events } => {
            std::process::exit(prompt::send_message(row.as_deref(), events))
        }
        Command::Rewind { dir, seq } => std::process::exit(branch::rewind(dir, seq)),
        Command::Summarize { dir, seq, prompt } => {
            std::process::exit(branch::summarize(dir, seq, prompt))
        }
        Command::RunPane { args } => std::process::exit(prompt::run_pane(&args)),
        Command::EventPreview {
            dir,
            seq,
            print_lang,
        } => doc::event_preview(&dir, seq, print_lang),
        Command::CopyEntry { dir, seq } => std::process::exit(yank::copy_entry(&dir, seq)),
    }
}
