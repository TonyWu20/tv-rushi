//! Fake-ssh integration tests for the remote roots. The
//! `RUSHI_SESSIONS_SSH` override points at a shell script that stands in
//! for ssh and prints the TSV or card the remote binary would.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

const FAKE_SSH_OK: &str = r#"#!/bin/sh
if [ -n "$FAKE_SSH_LOG" ]; then
  printf '%s\n' "$*" >> "$FAKE_SSH_LOG"
fi
case "$*" in
  *preview*)
    printf '[rushi-session]\nname    = "remote-fixture"\n'
    ;;
  *)
    printf 'ACTIVE\tremote-fixture\trepoR\ttools\t10-02 14:11\t1760000000\t/export/repoR/sessions/remote-fixture\n'
    ;;
esac
"#;

const FAKE_SSH_DOWN: &str = r#"#!/bin/sh
echo "ssh: Could not resolve hostname host" >&2
exit 255
"#;

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("rushi-sessions-{}-{}", std::process::id(), tag));
    let _ = std::fs::create_dir_all(&d);
    d
}

fn write_exec(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    let _ = f.write_all(body.as_bytes());
    drop(f);
    let mut perms = std::fs::metadata(&p).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&p, perms).unwrap();
    p
}

fn bin() -> PathBuf {
    // The test executable lives at <target>/<profile>/deps/; the built
    // binary lives at <target>/<profile>/rushi-sessions. Resolving from
    // the test executable tracks both the target dir and the profile.
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let profile = deps.parent().unwrap();
    profile.join("rushi-sessions")
}

#[test]
fn source_scans_remote_root_over_fake_ssh() {
    let d = unique_dir("src");
    let fake = write_exec(&d, "fake-ssh", FAKE_SSH_OK);
    let log = d.join("ssh.log");
    let out = Command::new(bin())
        .args(["source", "host:/export"])
        .env("RUSHI_SESSIONS_SSH", fake.to_str().unwrap())
        .env("FAKE_SSH_LOG", log.to_str().unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let fields: Vec<&str> = stdout.trim().split('\t').collect();
    assert_eq!(fields.len(), 7, "stdout: {stdout}");
    assert_eq!(fields[2], "host:repoR");
    assert_eq!(fields[6], "host:/export/repoR/sessions/remote-fixture");
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(logged.contains("rushi-sessions source"), "remote command: {logged}");
    assert!(logged.contains("/export"), "remote command: {logged}");
}

#[test]
fn source_passes_active_only_to_remote() {
    let d = unique_dir("act");
    let fake = write_exec(&d, "fake-ssh", FAKE_SSH_OK);
    let log = d.join("ssh.log");
    let out = Command::new(bin())
        .args(["source", "--active-only", "host:/export"])
        .env("RUSHI_SESSIONS_SSH", fake.to_str().unwrap())
        .env("FAKE_SSH_LOG", log.to_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(logged.contains("source --active-only"), "remote command: {logged}");
}

#[test]
fn source_warns_on_failed_remote_scan() {
    let d = unique_dir("down");
    let fake = write_exec(&d, "fake-ssh", FAKE_SSH_DOWN);
    let out = Command::new(bin())
        .args(["source", "host:/export"])
        .env("RUSHI_SESSIONS_SSH", fake.to_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("skipping host: host"), "stderr: {stderr}");
    assert!(stderr.contains("Could not resolve"), "stderr: {stderr}");
}

#[test]
fn source_warns_when_ssh_missing() {
    let out = Command::new(bin())
        .args(["source", "host:/export"])
        .env("RUSHI_SESSIONS_SSH", "/no/such/ssh-binary")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("skipping unreachable host: host"),
        "stderr: {stderr}"
    );
}

#[test]
fn source_merges_local_and_remote_rows() {
    let d = unique_dir("mix");
    let local_root = d.join("empty-root");
    std::fs::create_dir_all(&local_root).unwrap();
    let fake = write_exec(&d, "fake-ssh", FAKE_SSH_OK);
    let out = Command::new(bin())
        .args(["source", local_root.to_str().unwrap(), "host:/export"])
        .env("RUSHI_SESSIONS_SSH", fake.to_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "stdout: {stdout}");
    assert!(lines[0].ends_with("host:/export/repoR/sessions/remote-fixture"));
}

#[test]
fn preview_runs_remotely_over_fake_ssh() {
    let d = unique_dir("prev");
    let fake = write_exec(&d, "fake-ssh", FAKE_SSH_OK);
    let log = d.join("ssh.log");
    let out = Command::new(bin())
        .args([
            "preview",
            "ACTIVE",
            "remote-fixture",
            "host:repoR",
            "tools",
            "10-02 14:11",
            "1760000000",
            "host:/export/repoR/sessions/remote-fixture",
        ])
        .env("RUSHI_SESSIONS_SSH", fake.to_str().unwrap())
        .env("FAKE_SSH_LOG", log.to_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("remote-fixture"), "stdout: {stdout}");
    let logged = std::fs::read_to_string(&log).unwrap();
    // The remote call carries the bare remote path, not the prefixed one.
    assert!(logged.contains("/export/repoR/sessions/remote-fixture"));
    assert!(!logged.contains("host:/export/repoR"), "remote command: {logged}");
    assert!(logged.contains("preview"), "remote command: {logged}");
    // The host reaches the remote card render through the env var,
    // not through a new argument.
    assert!(
        logged.contains("RUSHI_SESSIONS_PREVIEW_HOST='host'"),
        "remote command: {logged}"
    );
    // The repo column carries the host prefix; the remote card gets the
    // bare repo name.
    assert!(logged.contains("'repoR'"), "remote command: {logged}");
    assert!(!logged.contains("host:repoR"), "remote command: {logged}");
}

#[test]
fn preview_remote_without_host_prefix_keeps_repo() {
    let d = unique_dir("prev2");
    let fake = write_exec(&d, "fake-ssh", FAKE_SSH_OK);
    let log = d.join("ssh.log");
    let out = Command::new(bin())
        .args([
            "preview",
            "ACTIVE",
            "remote-fixture",
            "repoR",
            "tools",
            "10-02 14:11",
            "1760000000",
            "host:/export/repoR/sessions/remote-fixture",
        ])
        .env("RUSHI_SESSIONS_SSH", fake.to_str().unwrap())
        .env("FAKE_SSH_LOG", log.to_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    let logged = std::fs::read_to_string(&log).unwrap();
    // A repo column without the prefix passes through unchanged.
    assert!(logged.contains("'repoR'"), "remote command: {logged}");
}

