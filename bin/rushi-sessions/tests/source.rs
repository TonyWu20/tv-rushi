//! The `source` command sorts its rows by the session mtime, newest
//! first, regardless of status. The channel's `no_sort` setting keeps
//! this order for television's typed-query results.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

fn bin() -> PathBuf {
    // The test executable lives at <target>/<profile>/deps/; the built
    // binary lives at <target>/<profile>/rushi-sessions. Resolving from
    // the test executable tracks both the target dir and the profile.
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let profile = deps.parent().unwrap();
    profile.join("rushi-sessions")
}

fn unique_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rushi-sessions-src-{}-{}", std::process::id(), tag));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Force the file mtime to a fixed epoch second so the order is
/// deterministic. The rows carry the reference file's mtime.
fn set_mtime(p: &Path, secs: u64) {
    let f = std::fs::File::open(p).unwrap();
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    f.set_modified(t).unwrap();
}

fn make_session(dir: &Path, _name: &str, mtime_secs: u64, live: bool) {
    std::fs::create_dir_all(dir).unwrap();
    let ev = dir.join("events.jsonl");
    let mut f = std::fs::File::create(&ev).unwrap();
    f.write_all(b"{}\n").unwrap();
    drop(f);
    set_mtime(&ev, mtime_secs);
    if live {
        std::fs::write(dir.join("loop.pid"), std::process::id().to_string()).unwrap();
    } else {
        // A stale pid keeps the row IDLE.
        std::fs::write(dir.join("loop.pid"), "999999999").unwrap();
    }
}

#[test]
fn source_rows_sort_newest_mtime_first() {
    // Requires `fd` on PATH (the binary's hard dependency for the
    // local walk).
    let d = unique_dir("sort");
    // Three sessions under one repo root. The mtimes are deliberately
    // in a different order than the directory order, and the newest
    // one is ACTIVE while the others are IDLE: the sort must not
    // group by status.
    let sessions = d.join("proj/sessions");
    make_session(&sessions.join("old"), "old", 1_000, false);
    make_session(&sessions.join("new"), "new", 3_000, true);
    make_session(&sessions.join("mid"), "mid", 2_000, false);

    let out = Command::new(bin())
        .args(["source", d.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let rows: Vec<Vec<&str>> = stdout.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows.len(), 3, "rows: {rows:?}");
    let names: Vec<&str> = rows.iter().map(|f| f[1]).collect();
    assert_eq!(names, vec!["new", "mid", "old"], "rows: {rows:?}");
    // The newest row is the live one; the statuses interleave by time.
    assert_eq!(rows[0][0], "ACTIVE", "rows: {rows:?}");
    assert_eq!(rows[1][0], "IDLE", "rows: {rows:?}");
    assert_eq!(rows[2][0], "IDLE", "rows: {rows:?}");
}
