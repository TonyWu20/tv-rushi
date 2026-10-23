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

/// The tail scan window of the plain layout: the last 100 KB of the
/// log text.
const PLAIN_TAIL_BYTES: usize = 100_000;

/// The line window of the indexed probe: the last 4096 lines of
/// text, the kernel's own tail-scan window.
const TAIL_LINES: usize = 4096;

/// The stream-byte budget of the no-index zstd fallback
/// (`read_log_tail`, P8): the last 512 KiB of the stream.
const TAIL_BUDGET: u64 = 512 * 1024;

/// The value of the most recent `loop_phase` marker in the log.
/// Mirrors the old string scan: the last `"id":"loop_phase"` marker,
/// then the following `"value":"..."`.
///
/// The probe consults the phase cache first: an unchanged log (the
/// same mtime and size as the last probe) returns its cached value
/// without re-reading the log, so a `--watch` refresh costs only
/// the changed sessions (decision 84). The read then follows the
/// log's layout (kernel PR #54, PR #56):
///
/// - plain layout: a 100 KB tail read, the marker scan on its
///   lines.
/// - zstd stream with a frame index (P8): `tail_lines`, which
///   decodes only the record ranges at the tail.
/// - zstd stream without an index (pre-P8 logs): the shared
///   `read_log_tail` window scan, its full read on a window that
///   holds no frame start.
pub(crate) fn last_phase(ev: &str) -> String {
    let p = std::path::Path::new(ev);
    let meta = match fs::metadata(p) {
        Ok(m) => m,
        Err(_) => return "?".into(),
    };
    if let Some((mtime_ns, size)) = log_key(&meta) {
        if let Some(cached) = crate::phasecache::lookup(p, mtime_ns, size) {
            return cached;
        }
    }
    let (phase, read_ok) = probe_log(p, &meta);
    if read_ok {
        if let Some((mtime_ns, size)) = log_key(&meta) {
            crate::phasecache::store(p, mtime_ns, size, &phase);
        }
    }
    phase
}

/// The (mtime_ns, size) identity of the log for the phase cache.
/// None when the mtime does not resolve to an epoch duration.
fn log_key(meta: &fs::Metadata) -> Option<(u128, u64)> {
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|mt| mt.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())?;
    Some((mtime_ns, meta.len()))
}

/// Probe the log without the cache. The second return says whether
/// the probe read the log at all: only a read that ran stores the
/// result, so a permission failure never caches its `?`.
fn probe_log(p: &std::path::Path, _meta: &fs::Metadata) -> (String, bool) {
    if rushi_common::logstream::is_zstd_stream(&sniff_head(p)) {
        match probe_zstd(p) {
            Some(ph) => (ph, true),
            None => ("?".into(), false),
        }
    } else {
        match read_tail(p.to_str().unwrap_or(""), PLAIN_TAIL_BYTES) {
            Ok(d) => (
                phase_scan_lines(std::str::from_utf8(&d).unwrap_or("").lines())
                    .unwrap_or_else(|| "?".into()),
                true,
            ),
            Err(_) => ("?".into(), false),
        }
    }
}

/// The zstd probe: the indexed tail read when the log carries a
/// validated frame index (P8), the shared window scan otherwise.
/// None when the read did not run (a missing or unreadable log).
///
/// The sidecar existence check keeps the no-index path to the one
/// whole-file read of the window scan: the pre-P8 logs of the
/// corpus carry no index, and `open_indexed` would add a second
/// whole-file read before it knows the index is absent.
fn probe_zstd(p: &std::path::Path) -> Option<String> {
    let lines: Vec<String> = if sidecar_exists(p) {
        let stream = rushi_common::logstream::open_indexed(p).ok()?;
        if stream.has_index() {
            stream.tail_lines(TAIL_LINES)
        } else {
            window_scan_lines(p)?
        }
    } else {
        window_scan_lines(p)?
    };
    Some(phase_scan_lines(lines.iter().map(|s| s.as_str())).unwrap_or_else(|| "?".into()))
}

/// The sidecar of the log, mirroring the shared `index_path`:
/// `<log name>.idx` next to the log.
fn sidecar_exists(p: &std::path::Path) -> bool {
    match p.file_name().and_then(|n| n.to_str()) {
        Some(name) => p.with_file_name(format!("{name}.idx")).exists(),
        None => false,
    }
}

/// The shared window scan: the last 512 KiB of the stream, the
/// full read when the window holds no frame start.
fn window_scan_lines(p: &std::path::Path) -> Option<Vec<String>> {
    let (text, _covered) = rushi_common::logstream::read_log_tail(p, TAIL_BUDGET).ok()?;
    Some(text.lines().map(|l| l.to_string()).collect())
}

/// The first four bytes of the file, the layout sniff. A missing or
/// short file sniffs as not-a-stream (zero bytes).
fn sniff_head(path: &std::path::Path) -> [u8; 4] {
    use std::io::Read;
    let Ok(mut f) = fs::File::open(path) else {
        return [0; 4];
    };
    let mut head = [0u8; 4];
    match f.read_exact(&mut head) {
        Ok(()) => head,
        Err(_) => [0; 4],
    }
}

/// The value of the most recent `loop_phase` marker among `lines`,
/// oldest first. The newest line that carries the marker is the
/// answer; a marker without its value yields None, as the byte
/// scan did.
fn phase_scan_lines<'a>(
    lines: impl Iterator<Item = &'a str> + std::iter::DoubleEndedIterator,
) -> Option<String> {
    let idm = "\"id\":\"loop_phase\"";
    let hit = lines.rev().find(|l| l.contains(idm))?.as_bytes();
    let vmark = b"\"value\":\"";
    let i = hit.windows(idm.len()).rposition(|w| w == idm.as_bytes())?;
    let jrel = hit[i..].windows(vmark.len()).position(|w| w == vmark)?;
    let vstart = i + jrel + vmark.len();
    let kend = hit[vstart..].iter().position(|b| *b == b'"')? + vstart;
    Some(String::from_utf8_lossy(&hit[vstart..kend]).into_owned())
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
    use std::path::Path;

    /// One temp dir per test, unique against the sibling tests of
    /// this binary (same pid, parallel threads).
    static DIR_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn zst_dir() -> std::path::PathBuf {
        let n = DIR_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("rushi-zst-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One `loop_phase` marker line, the kernel's ext_status shape.
    fn phase_line(value: &str) -> String {
        format!(r#"{{"v":1,"type":"ext_status","id":"loop_phase","value":"{value}"}}"#)
    }

    /// One plain user_message line, the kernel's wire shape.
    fn user_line(text: &str) -> String {
        format!(r#"{{"v":1,"type":"user_message","ts":"t","content":"{text}"}}"#)
    }

    /// A per-commit zstd stream without the index sidecar: one
    /// level-9 frame per line (P1, the pre-P8 layout).
    fn frames_only(lines: &[String]) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        for l in lines {
            buf.extend(rushi_common::logstream::compress_frame(l.as_bytes()).unwrap());
        }
        buf
    }

    /// A per-commit zstd stream with its P8 index sidecar: one
    /// frame and one 32-byte record per commit, written by the
    /// kernel's `LogLine::commit`.
    fn commit_lines(dir: &Path, name: &str, lines: &[String]) -> std::path::PathBuf {
        let p = dir.join(name);
        for l in lines {
            rushi_common::logline::LogLine::from_json(l)
                .commit(&p)
                .unwrap();
        }
        p
    }

    /// Point the phase cache at a private dir for the duration of
    /// the guard, so `last_phase` probes never touch the user's
    /// cache. The env override is read on every cache call; the
    /// process-wide guard serializes the set against the sibling
    /// tests.
    fn with_private_cache() -> (std::path::PathBuf, std::sync::MutexGuard<'static, ()>) {
        use std::sync::Mutex;
        static GUARD: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
        static CACHE_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let g = GUARD.get_or_init(|| Mutex::new(()));
        let guard = g.lock().unwrap();
        let n = CACHE_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("rushi-zst-cache-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        unsafe { std::env::set_var("RUSHI_SESSIONS_CACHE_DIR", &d) };
        (d, guard)
    }

    #[test]
    fn last_phase_plain_tail_scan() {
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let mut text = phase_line("think");
        text.push_str(&phase_line("tool"));
        std::fs::write(&p, text).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_plain_no_marker_is_unknown() {
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let p = d.join("events.jsonl");
        std::fs::write(&p, user_line("hi")).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "?");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_zstd_frames_without_index() {
        // The pre-P8 zstd layout: frames only, no sidecar. The
        // probe takes the shared window scan (read_log_tail).
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let lines = vec![phase_line("think"), user_line("hi"), phase_line("tool")];
        std::fs::write(&p, frames_only(&lines)).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_zstd_indexed_tail() {
        // The P8 layout: the index sidecar rides with the frames.
        // The probe decodes only the record ranges at the tail.
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let p = commit_lines(
            &d,
            "events.jsonl",
            &[phase_line("think"), user_line("hi"), phase_line("tool")],
        );
        let stream = rushi_common::logstream::open_indexed(&p).unwrap();
        assert!(stream.has_index(), "the commit wrote the sidecar");
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_indexed_tail_window_holds_the_marker() {
        // The marker sits 4096 lines under the newest lines: the
        // tail window of the probe is the kernel's 4096, so the
        // probe finds it.
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let mut lines = vec![phase_line("think"), user_line("hi")];
        lines.push(phase_line("tool"));
        // Pad past the window.
        for _ in 0..(TAIL_LINES + 4) {
            lines.push(user_line("noise"));
        }
        lines.push(phase_line("wait"));
        let p = commit_lines(&d, "events.jsonl", &lines);
        assert_eq!(last_phase(p.to_str().unwrap()), "wait");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_torn_tail_keeps_the_marker() {
        // P4: the marker of a complete frame is found even when the
        // final frame is torn. No index here: the window scan takes
        // the torn stream.
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let ok = rushi_common::logstream::compress_frame(phase_line("think").as_bytes()).unwrap();
        let torn = rushi_common::logstream::compress_frame(b"garbage\n").unwrap();
        let mut buf = ok;
        buf.extend_from_slice(&torn[..torn.len() / 2]);
        std::fs::write(&p, buf).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "think");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_missing_file_is_unknown() {
        let (_c, _g) = with_private_cache();
        assert_eq!(last_phase("/no/such/events.jsonl"), "?");
    }

    #[test]
    fn last_phase_zstd_cache_hit_skips_the_reread() {
        let (cache, _g) = with_private_cache();
        let d = zst_dir();
        let p = d.join("events.jsonl");
        let lines = vec![phase_line("think"), phase_line("tool")];
        std::fs::write(&p, frames_only(&lines)).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        // The probe stored the entry under the log's identity.
        let key = log_key(&fs::metadata(&p).unwrap()).unwrap();
        let cached = crate::phasecache::lookup(&p, key.0, key.1);
        assert_eq!(cached, Some("tool".into()));
        // The entry file sits in the private cache dir.
        let entries: Vec<_> = std::fs::read_dir(&cache).unwrap().collect();
        assert!(!entries.is_empty(), "the probe stored its cache entry");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn last_phase_rereads_on_a_new_identity() {
        let (_c, _g) = with_private_cache();
        let d = zst_dir();
        let p = d.join("events.jsonl");
        std::fs::write(&p, frames_only(&[phase_line("think")])).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "think");
        // A new frame lands: new size, new mtime, new value.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut buf = std::fs::read(&p).unwrap();
        buf.extend(rushi_common::logstream::compress_frame(phase_line("tool").as_bytes()).unwrap());
        std::fs::write(&p, buf).unwrap();
        assert_eq!(last_phase(p.to_str().unwrap()), "tool");
        let _ = std::fs::remove_dir_all(&d);
    }
}
