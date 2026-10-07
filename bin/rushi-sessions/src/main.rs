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
//!   first. `--chat` keeps only the user and assistant messages.
//!
//! - `rushi-sessions event-preview DIR SEQ [--print-lang]`
//!   Render one events.jsonl entry as a document: markdown for message
//!   entries, toml for everything else. `--print-lang` prints only the
//!   document language.
//!
//! `source` calls `fd` for the directory walk (a hard dependency of the
//! channel) and replaces the channel's previous `python3` reader.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod doc;
mod events;
mod format;
mod preview;
mod remote;
mod scan;
mod state;
mod usage;

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
    /// Columns: seq, type, ts, summary. The rows come newest first.
    /// Without DIR, the CWD is the session dir.
    Events {
        /// The session dir (the dir that holds events.jsonl).
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Only the user and assistant messages.
        #[arg(long)]
        chat: bool,
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
        Command::EventPreview {
            dir,
            seq,
            print_lang,
        } => doc::event_preview(&dir, seq, print_lang),
    }
}
