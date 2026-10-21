//! The session-log read layer of the channel backend.
//!
//! A session log (`events.jsonl`, `tools.jsonl`) is one of two
//! layouts (kernel PR #54, property P2): a legacy plain-text JSONL
//! file, or a zstd stream — a concatenation of zstd frames, one
//! frame per commit (P1), one level-19 frame after the idle
//! consolidation pass (P5). The layout is decided at creation: a
//! legacy plain-text log stays plain for its life. The reader
//! sniffs the four zstd magic bytes (P3) and reads both layouts.
//! A torn tail frame stops the read at the last complete frame
//! (P4); the partial line drops at parse time.
//!
//! The read semantics mirror the kernel's `logstream::read_log_text`
//! (rushi PR #54). The consumer keeps only the read side: no frame
//! compression, no consolidation.
//!
//! This file is the interim mirror (decision 82). `logstream` is
//! public in `rushi-common`, the kernel's published crate. Main
//! carries 0.1.7 at the PR merge. The next release after the
//! merge carries the module. When that release publishes, this
//! module becomes a re-export of
//! `rushi_common::logstream::read_log_text` and then goes away.

use std::fs;
use std::io;
use std::path::Path;

/// The four zstd magic bytes. A session log carrying them is a
/// zstd stream; anything else is legacy plain text. The kernel's
/// `logstream::ZSTD_MAGIC`.
pub(crate) const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// True when the head of `buf` carries the zstd magic bytes.
pub(crate) fn is_zstd_stream(buf: &[u8]) -> bool {
    buf.starts_with(&ZSTD_MAGIC[..])
}

/// True when the file at `path` is a zstd stream. The sniff reads
/// the first four bytes. A missing file or a file shorter than the
/// magic is not a stream.
pub(crate) fn is_zstd_file(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    match f.read_exact(&mut head) {
        Ok(()) => is_zstd_stream(&head),
        Err(_) => false,
    }
}

/// Read a session log into JSONL text.
///
/// Plain-text logs read verbatim. Zstd streams decompress in frame
/// order. A torn final frame stops the read at the last complete
/// frame; the partial line drops at parse time (P4). A missing file
/// is an error.
pub(crate) fn read_log_text(path: &Path) -> io::Result<String> {
    let buf = fs::read(path)?;
    if !is_zstd_stream(&buf) {
        return String::from_utf8(buf).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "session log is not valid UTF-8",
            )
        });
    }
    decompress_stream(path, &buf)
}

/// Decompress a zstd log stream. A torn final frame keeps every
/// complete frame and reports the truncation (P4).
fn decompress_stream(path: &Path, buf: &[u8]) -> io::Result<String> {
    use std::io::Read;
    let mut dec = zstd::stream::read::Decoder::new(buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
    let mut out = Vec::with_capacity(buf.len().saturating_mul(8));
    // The read fills `out` frame by frame. A truncation in the
    // final frame stops the read; `out` keeps every complete frame
    // plus any complete block of the torn one. The partial line
    // drops at parse time (P4). One stderr note, no error to the
    // caller.
    if dec.read_to_end(&mut out).is_err() {
        eprintln!(
            "logtext: {}: torn tail frame; the read stops at the last complete frame",
            path.display()
        );
    }
    String::from_utf8(out).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "zstd log stream is not valid UTF-8")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One temp dir per test, unique against the sibling tests of
    /// this binary (same pid, parallel threads).
    static DIR_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn logtext_dir() -> std::path::PathBuf {
        let n = DIR_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "rushi-logtext-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One zstd frame per line, the kernel's per-commit layout (P1).
    fn per_commit_frames(lines: &[&[u8]]) -> Vec<u8> {
        let mut buf = Vec::new();
        for l in lines {
            buf.extend(zstd::encode_all(*l, 9).unwrap());
        }
        buf
    }

    #[test]
    fn plain_text_reads_verbatim() {
        let dir = logtext_dir();
        let p = dir.join("events.jsonl");
        std::fs::write(&p, "plain\nline\n").unwrap();
        assert!(!is_zstd_file(&p));
        assert_eq!(read_log_text(&p).unwrap(), "plain\nline\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn per_commit_frames_read_in_order() {
        let dir = logtext_dir();
        let p = dir.join("events.jsonl");
        std::fs::write(&p, per_commit_frames(&[b"a\n", b"b\n", b"c\n"])).unwrap();
        assert!(is_zstd_file(&p));
        assert_eq!(read_log_text(&p).unwrap(), "a\nb\nc\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn consolidated_single_frame_reads() {
        // The idle pass rewrites the log as one level-19 frame (P5).
        let dir = logtext_dir();
        let p = dir.join("events.jsonl");
        std::fs::write(&p, zstd::encode_all(&b"a\nb\n"[..], 19).unwrap()).unwrap();
        assert_eq!(read_log_text(&p).unwrap(), "a\nb\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_tail_stops_at_the_last_complete_frame() {
        // P4: the final frame is cut mid-frame. The complete frames
        // still read; the torn line drops at parse time.
        let dir = logtext_dir();
        let p = dir.join("events.jsonl");
        let ok = zstd::encode_all(&b"good\n"[..], 9).unwrap();
        let torn = zstd::encode_all(&b"tear\n"[..], 9).unwrap();
        let mut buf = ok;
        buf.extend_from_slice(&torn[..torn.len() / 2]);
        std::fs::write(&p, &buf).unwrap();
        let text = read_log_text(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.first() == Some(&"good"), "{lines:?}");
        // The torn frame contributes nothing, or a partial line.
        // The partial line is not a parseable entry.
        let last = lines.last().copied().unwrap_or("");
        assert!(last == "good" || serde_json::from_str::<serde_json::Value>(last).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_an_error() {
        let p = std::path::Path::new("/no/such/events.jsonl");
        assert!(read_log_text(p).is_err());
        assert!(!is_zstd_file(p));
    }

    #[test]
    fn empty_file_is_plain_text() {
        let dir = logtext_dir();
        let p = dir.join("events.jsonl");
        std::fs::File::create(&p).unwrap();
        assert!(!is_zstd_file(&p));
        assert_eq!(read_log_text(&p).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
