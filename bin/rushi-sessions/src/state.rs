//! Session-state probes: loop pid liveness, file mtime, and the last
//! `loop_phase` value in the event log tail.
use chrono::{Local, TimeZone};
use std::fs;

/// Read the last `n` bytes of a file.
fn read_tail(p: &str, n: usize) -> Result<Vec<u8>, std::io::Error> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(p)?;
    let size = f.metadata()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(n as u64)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}
/// The value of the most recent `loop_phase` ext_status in the log tail.
/// Mirrors the old string scan: the last `"id":"loop_phase"` marker, then
/// the following `"value":"..."`. The scan runs on the log text: the
/// plain layout is tail-read, the zstd layout decodes in full (a zstd
/// stream is not random access), and the tail of the decoded text
/// carries the scan (kernel PR #54, P3).
pub(crate) fn last_phase(ev: &str) -> String {
    use std::path::Path;
    let p = Path::new(ev);
    let tail: Box<[u8]> = if zstd_stream_head(p) {
        match rushi_common::logstream::read_log_text(p) {
            Ok(t) => tail_of(t.as_bytes()),
            Err(_) => return "?".into(),
        }
    } else {
        match read_tail(ev, 100_000) {
            Ok(d) => d.into_boxed_slice(),
            Err(_) => return "?".into(),
        }
    };
    match phase_scan(&tail) {
        Some(v) => v,
        None => "?".into(),
    }
}

/// True when the file head carries the zstd magic bytes, the
/// shared kernel constant. The sniff reads the first four bytes.
/// A missing or short file is not a stream.
fn zstd_stream_head(path: &std::path::Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    match f.read_exact(&mut head) {
        Ok(()) => rushi_common::logstream::is_zstd_stream(&head),
        Err(_) => false,
    }
}

/// The last 100 KB of the log text, the scan window of both
/// layouts.
fn tail_of(data: &[u8]) -> Box<[u8]> {
    let start = data.len().saturating_sub(100_000);
    data[start..].to_vec().into_boxed_slice()
}

/// The value of the most recent `loop_phase` marker in `data`, or
/// None when no marker or no value is present.
fn phase_scan(data: &[u8]) -> Option<String> {
    let idm = b"\"id\":\"loop_phase\"";
    let i = data.windows(idm.len()).rposition(|w| w == idm)?;
    let vmark = b"\"value\":\"";
    let jrel = data[i..].windows(vmark.len()).position(|w| w == vmark)?;
    let vstart = i + jrel + vmark.len();
    let kend = data[vstart..].iter().position(|b| *b == b'"')? + vstart;
    Some(String::from_utf8_lossy(&data[vstart..kend]).into_owned())
}
/// Whether a pid is alive (`kill(pid, 0)` succeeds).
pub(crate) fn alive(pid: &str) -> bool {
    if pid.is_empty() {
        return false;
    }
    let p: i32 = match pid.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    unsafe { libc::kill(p, 0) == 0 }
}
/// Whole-epoch-seconds mtime of a path, or 0 if it cannot be read.
pub(crate) fn mtime_secs(p: &str) -> i64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|mt| {
            mt.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}
/// Format epoch seconds as local `mm-dd HH:MM`.
pub(crate) fn fmt_local(secs: i64) -> String {
    match Local.timestamp_opt(secs, 0).single() {
        Some(dt) => dt.format("%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One temp dir per test, unique against the sibling tests of
    /// this binary (same pid, parallel threads).
    static DIR_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn zst_dir() -> std::path::PathBuf {
        let n = DIR_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "rushi-zst-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One `loop_phase` marker line, the kernel's ext_status shape.
    fn phase_line(value: &str) -> String {
        format!(
            "{{\"type\":\"ext_status\",\"id\":\"loop_phase\",\"value\":\"{}\"}}\n",
            value
        )
    }

    #[test]
    fn last_phase_plain_tail_scan() {
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let mut text = phase_line("think");
        text.push_str(&phase_line("tool"));
        std::fs::write(&p, text).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_zstd_stream_decodes() {
        // The zstd layout: one frame per line (P1). The scan runs on
        // the decoded text.
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let mut buf = zstd::encode_all(
            phase_line("think").as_bytes(),
            9,
        )
        .unwrap();
        buf.extend(
            zstd::encode_all(phase_line("tool").as_bytes(), 9).unwrap(),
        );
        std::fs::write(&p, &buf).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_torn_tail_keeps_the_marker() {
        // P4: the marker of a complete frame is found even when the
        // final frame is torn.
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let ok = zstd::encode_all(phase_line("think").as_bytes(), 9).unwrap();
        let torn = zstd::encode_all(&b"garbage\n"[..], 9).unwrap();
        let mut buf = ok;
        buf.extend_from_slice(&torn[..torn.len() / 2]);
        std::fs::write(&p, &buf).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "think");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_missing_file_is_unknown() {
        assert_eq!(last_phase("/no/such/events.jsonl"), "?");
    }
}
