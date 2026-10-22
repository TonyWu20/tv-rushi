//! The best-effort cache of the phase probe.
//!
//! A `--watch` refresh re-runs `source`, and `source` probes the
//! event log of every session. An unchanged log (the same mtime and
//! size as the last probe) carries the same last phase, so the probe
//! returns the cached value without re-reading the log. The cache is
//! advisory only: a missing dir, an unreadable entry, or a stale key
//! falls back to the probe. Nothing in the channel errors on cache
//! trouble.

use std::path::{Path, PathBuf};

/// Store the last phase for the log at `log` under the caller's
/// fresh (mtime_ns, size) key. The entry is one line:
/// `mtime_ns<TAB>size<TAB>phase`. The write lands in a pid-suffixed
/// temp file, then renames over the entry, so two concurrent
/// writers never tear the file.
pub(crate) fn store(log: &Path, mtime_ns: u128, size: u64, phase: &str) {
    if let Some(dir) = cache_dir() {
        store_in(&dir, log, mtime_ns, size, phase);
    }
}

/// The cached last phase of the log, when the stored (mtime_ns,
/// size) key matches. A missing entry, a stale key, or an unreadable
/// or malformed entry is None: the probe re-runs.
pub(crate) fn lookup(log: &Path, mtime_ns: u128, size: u64) -> Option<String> {
    let dir = cache_dir()?;
    lookup_in(&dir, log, mtime_ns, size)
}

/// The dir form of [`store`]. The dir must exist or be creatable.
pub(crate) fn store_in(dir: &Path, log: &Path, mtime_ns: u128, size: u64, phase: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let name = entry_name(log);
    let tmp = dir.join(format!(".{name}.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, format!("{mtime_ns}\t{size}\t{phase}\n")).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, dir.join(name));
}

/// The dir form of [`lookup`].
pub(crate) fn lookup_in(dir: &Path, log: &Path, mtime_ns: u128, size: u64) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(entry_name(log))).ok()?;
    let mut fields = text.lines().next()?.split('\t');
    let m = fields.next()?.parse::<u128>().ok()?;
    let s = fields.next()?.parse::<u64>().ok()?;
    let phase = fields.next()?;
    if (m, s) == (mtime_ns, size) {
        Some(phase.to_string())
    } else {
        None
    }
}

/// The cache dir: `$RUSHI_SESSIONS_CACHE_DIR`, else
/// `$XDG_CACHE_HOME/rushi-sessions`, else `$HOME/.cache/rushi-sessions`.
/// None when no base resolves.
fn cache_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("RUSHI_SESSIONS_CACHE_DIR") {
        return Some(PathBuf::from(d));
    }
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(d) => PathBuf::from(d),
        Err(_) => std::path::PathBuf::from(std::env::var("HOME").ok()?).join(".cache"),
    };
    Some(base.join("rushi-sessions"))
}

/// The cache entry name for a log: every byte of the log path that
/// is not `[A-Za-z0-9-]` maps to `.`. A mapping longer than 200
/// bytes takes the 16-hex FNV-1a digest of the path instead.
fn entry_name(log: &Path) -> String {
    let s = log.to_string_lossy();
    let mapped: String = s
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' => b as char,
            _ => '.',
        })
        .collect();
    if mapped.len() <= 200 {
        mapped
    } else {
        format!("{:016x}", fnv1a(s.as_bytes()))
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_4842_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One temp cache dir per test, unique against the sibling tests
    /// of this binary (same pid, parallel threads).
    static DIR_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn cache_dir_fixture() -> std::path::PathBuf {
        let n = DIR_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir()
            .join(format!("rushi-phasecache-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn log_path() -> std::path::PathBuf {
        std::path::PathBuf::from("/abs/sessions/a/b/events.jsonl")
    }

    #[test]
    fn store_lookup_round_trip() {
        let d = cache_dir_fixture();
        store_in(&d, &log_path(), 123, 456, "wait");
        assert_eq!(lookup_in(&d, &log_path(), 123, 456), Some("wait".into()));
        // The entry file is named on the mapped path.
        assert!(d.join(".abs.sessions.a.b.events.jsonl").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stale_key_is_a_miss() {
        let d = cache_dir_fixture();
        store_in(&d, &log_path(), 123, 456, "wait");
        assert_eq!(lookup_in(&d, &log_path(), 123, 457), None, "new size");
        assert_eq!(lookup_in(&d, &log_path(), 124, 456), None, "new mtime");
        // The fresh probe stores the new key; the value follows.
        store_in(&d, &log_path(), 124, 456, "tool");
        assert_eq!(lookup_in(&d, &log_path(), 124, 456), Some("tool".into()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_entry_is_a_miss() {
        let d = cache_dir_fixture();
        assert_eq!(lookup_in(&d, &log_path(), 1, 1), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn malformed_entry_is_a_miss() {
        let d = cache_dir_fixture();
        let p = d.join(".abs.sessions.a.b.events.jsonl");
        std::fs::write(&p, "not-a-number\t7\twait\n").unwrap();
        assert_eq!(lookup_in(&d, &log_path(), 1, 7), None);
        std::fs::write(&p, "9\t7\n").unwrap();
        assert_eq!(lookup_in(&d, &log_path(), 9, 7), None, "no phase field");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn long_path_takes_the_digest_name() {
        let long = "x".repeat(300);
        let s = format!("/deep/{}/events.jsonl", long);
        let p = std::path::Path::new(&s);
        let name = entry_name(p);
        assert_eq!(name.len(), 16, "{name}");
        assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
