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
/// the following `"value":"..."`.
pub(crate) fn last_phase(ev: &str) -> String {
    let data = match read_tail(ev, 100_000) {
        Ok(d) => d,
        Err(_) => return "?".into(),
    };
    let idm = b"\"id\":\"loop_phase\"";
    let i = match data.windows(idm.len()).rposition(|w| w == idm) {
        Some(x) => x,
        None => return "?".into(),
    };
    let vmark = b"\"value\":\"";
    let jrel = match data[i..].windows(vmark.len()).position(|w| w == vmark) {
        Some(x) => x,
        None => return "?".into(),
    };
    let vstart = i + jrel + vmark.len();
    let kend = match data[vstart..].iter().position(|b| *b == b'"') {
        Some(x) => vstart + x,
        None => return "?".into(),
    };
    String::from_utf8_lossy(&data[vstart..kend]).into_owned()
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
