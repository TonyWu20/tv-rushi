//! The `copy-entry` command: copy one entry's full document to the
//! clipboard, newlines preserved. The document is the plain
//! `event-preview` render (markdown for messages, toml for
//! everything else).
//!
//! The transport follows television's own order: the OSC 52 terminal
//! clipboard escape is written to the controlling tty first (a
//! supporting terminal, or tmux 3.8+ with `set-clipboard on`, lands
//! it on the system clipboard). A local clipboard tool then takes
//! the pipe when one is on PATH. The tool write lands last, so it
//! wins on hosts that have one. With neither route, the command
//! exits 1 with a stderr line.
use std::path::Path;

use crate::doc::doc_for;
use crate::events::numbered_events;
use crate::format::which;

/// The local clipboard tools, in television's own try order. The
/// first one on PATH wins.
const CLIPBOARD_TOOLS: [(&str, &[&str]); 5] = [
    ("pbcopy", &[]),
    ("termux-clipboard-set", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["-ib"]),
];

/// Copy the entry at SEQ of the session dir's events.jsonl to the
/// clipboard. Returns the process exit code: 0 when at least one
/// route carried the document, 1 when the entry is absent or no
/// route is usable.
pub fn copy_entry(dir: &Path, seq: u64) -> i32 {
    let o = match numbered_events(dir)
        .into_iter()
        .find(|(n, _)| *n == seq)
        .map(|(_, v)| v)
    {
        Some(v) => v,
        None => {
            eprintln!("rushi-sessions: no event at line {seq}");
            return 1;
        }
    };
    let doc = doc_for(&o);

    let osc = osc52_write(&doc);
    let tool = pipe_to_first_tool(&doc);
    match tool {
        Some(name) => println!("entry {seq} yanked to clipboard via {name}"),
        None if osc => println!("entry {seq} yanked to clipboard via OSC 52"),
        None => {
            eprintln!("rushi-sessions: entry {seq}: no clipboard route (no clipboard tool on PATH, no controlling tty)");
            return 1;
        }
    }
    0
}

/// Write the OSC 52 clipboard set escape to the controlling tty.
/// Returns true when the write landed. Without a controlling
/// tty (a detached context) the route is simply unavailable.
fn osc52_write(content: &str) -> bool {
    use std::io::Write;
    let Ok(mut file) = std::fs::OpenOptions::new().write(true).open("/dev/tty") else {
        return false;
    };
    let esc = osc52_escape(content);
    file.write_all(esc.as_bytes())
        .and_then(|_| file.flush())
        .is_ok()
}

/// The OSC 52 clipboard set escape for one content: the standard
/// alphabet base64 payload, the ST terminator, the same escape
/// television itself writes.
fn osc52_escape(content: &str) -> String {
    format!(
        "\u{1b}]52;c;{}\u{1b}\\",
        base64_encode(content.as_bytes())
    )
}

/// Standard alphabet base64 (RFC 4648, with padding). The OSC 52
/// payloads of television and of the terminals that implement the
/// escape use this alphabet.
fn base64_encode(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0] as u32,
            chunk.get(1).copied().unwrap_or(0) as u32,
            chunk.get(2).copied().unwrap_or(0) as u32,
        ];
        let n = (b[0] << 16) | (b[1] << 8) | b[2];
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(A[(n >> 6 & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(A[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Pipe the document into the first clipboard tool on PATH (the
/// `CLIPBOARD_TOOLS` order). Returns the tool's name when the pipe
/// and the tool's exit both succeeded.
fn pipe_to_first_tool(doc: &str) -> Option<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    for (bin, args) in CLIPBOARD_TOOLS {
        let Some(path) = which(bin) else {
            continue;
        };
        let mut child = Command::new(path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        // The guard drop closes the tool's stdin, then the wait
        // reaps it. A partial write keeps the route down: the tool
        // holds a truncated document, so the next tool may still
        // take it.
        let stdin_ok = child
            .stdin
            .take()
            .and_then(|mut s| s.write_all(doc.as_bytes()).ok())
            .is_some();
        let status_ok = child.wait().is_ok_and(|s| s.success());
        if stdin_ok && status_ok {
            return Some(bin.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_standard_vectors() {
        // The RFC 4648 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn osc52_escape_shape() {
        // The same escape television writes (the ST terminator, the
        // standard alphabet, "hi" is aGk=).
        assert_eq!(osc52_escape("hi"), "\u{1b}]52;c;aGk=\u{1b}\\");
    }

    #[test]
    fn copy_entry_missing_seq_exits_1() {
        let d =
            std::env::temp_dir().join(format!("rushi-sessions-yank-{:-}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("events.jsonl"),
            "{\"type\":\"user_message\",\"ts\":\"2026-10-01T01:30:59Z\",\"content\":\"x\",\"v\":1}\n",
        )
        .unwrap();
        // Seq 42 is absent, so the guard fires before any clipboard
        // work.
        assert_eq!(copy_entry(&d, 42), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
