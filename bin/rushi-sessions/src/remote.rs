//! Remote (ssh) roots: run the `source` and `preview` commands on remote
//! hosts and merge their results.
use std::path::PathBuf;

use crate::scan::Row;

/// A remote root written as `host:/path` or `user@host:/path`. The
/// host is an ssh alias from the user's `~/.ssh/config`. The text
/// before the first `:` is the host, the rest is the remote path.
/// Local TSV paths are absolute, so a colon before the first `/` can
/// only come from a remote prefix.
pub(crate) fn split_remote(path: &str) -> Option<(&str, &str)> {
    let (host, rest) = path.split_once(':')?;
    if host.is_empty() || rest.is_empty() {
        return None;
    }
    if host.contains('/') || host.starts_with('@') || host.matches('@').count() > 1 {
        return None;
    }
    Some((host, rest))
}
/// The remote roots of one host, in request order.
pub(crate) struct RemoteGroup {
    pub(crate) host: String,
    pub(crate) roots: Vec<String>,
}
/// Split requested roots into a local list and per-host remote
/// groups. Group order is first-seen; their roots keep request order.
pub(crate) fn split_roots(roots: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<RemoteGroup>) {
    let mut local: Vec<PathBuf> = Vec::new();
    let mut groups: Vec<RemoteGroup> = Vec::new();
    for r in roots {
        let s = r.to_string_lossy().into_owned();
        if let Some((host, rp)) = split_remote(&s) {
            let host = host.to_string();
            let rp = rp.to_string();
            let mut matched = false;
            for g in groups.iter_mut() {
                if g.host == host {
                    g.roots.push(rp.clone());
                    matched = true;
                    break;
                }
            }
            if !matched {
                groups.push(RemoteGroup { host, roots: vec![rp] });
            }
        } else {
            local.push(r);
        }
    }
    (local, groups)
}
/// The ssh executable: `ssh`, or the `RUSHI_SESSIONS_SSH` override
/// (tests use a fake).
fn ssh_binary() -> String {
    std::env::var("RUSHI_SESSIONS_SSH").unwrap_or_else(|_| "ssh".into())
}
/// The remote `rushi-sessions` binary name. A non-interactive ssh
/// shell does not source shell init files, so a Nix profile binary is
/// not on PATH. Set `RUSHI_SESSIONS_REMOTE_BIN` to the remote absolute
/// path when the binary lives in a Nix profile. The value must be a
/// bare path without shell metacharacters.
fn remote_binary() -> String {
    std::env::var("RUSHI_SESSIONS_REMOTE_BIN").unwrap_or_else(|_| "rushi-sessions".into())
}
/// Single-quote a word for a remote shell command string.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
/// Run one remote command string over ssh (stdout and stderr captured).
fn run_remote(host: &str, command: &str) -> std::io::Result<std::process::Output> {
    std::process::Command::new(ssh_binary())
        .arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg(host)
        .arg(command)
        .output()
}
/// Run `source` on a remote host and return its TSV lines. An
/// unreachable host or a failed remote scan warns on stderr and yields
/// no lines; the other hosts still scan.
pub(crate) fn remote_source_lines(group: &RemoteGroup, active_only: bool) -> Vec<String> {
    // The remote command string, run by the remote login shell. Roots
    // are bare words, so the remote shell expands a leading `~`
    // (matching the local channel behavior).
    let flag = if active_only { "--active-only " } else { "" };
    let mut cmd = String::new();
    cmd.push_str(&remote_binary());
    cmd.push_str(" source ");
    cmd.push_str(flag);
    let mut first = true;
    for r in &group.roots {
        if !first {
            cmd.push(' ');
        }
        first = false;
        cmd.push_str(r);
    }
    match run_remote(&group.host, &cmd) {
        Ok(out) => {
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                eprintln!(
                    "rushi-sessions: skipping host: {} (remote source failed: {})",
                    group.host,
                    err.trim()
                );
                return Vec::new();
            }
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim_end)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        }
        Err(e) => {
            eprintln!(
                "rushi-sessions: skipping unreachable host: {} ({})",
                group.host, e
            );
            Vec::new()
        }
    }
}
/// One remote TSV line into a local `Row`, with the `host:` prefix on
/// the repo and the path columns. The row list shows the host in front
/// of the repo name; the preview strips it back off. A short line or a
/// bad mtime drops the row. A status that is not `ACTIVE` normalizes
/// to `IDLE` (an older remote binary must not break the list).
pub(crate) fn parse_remote_line(line: &str, host: &str) -> Option<Row> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() != 7 {
        return None;
    }
    let mtime: i64 = f[5].parse().ok()?;
    Some(Row {
        status: if f[0] == "ACTIVE" { "ACTIVE" } else { "IDLE" },
        name: f[1].to_string(),
        repo: format!("{}:{}", host, f[2]),
        phase: f[3].to_string(),
        last: f[4].to_string(),
        mtime,
        path: format!("{}:{}", host, f[6]),
    })
}
/// The remote preview: run the remote binary's preview over ssh and
/// stream its card to stdout. A failed ssh warns on stderr.
///
/// The command sets `RUSHI_SESSIONS_PREVIEW_HOST` inline (the remote
/// shell exports it for that one command). The remote card render
/// reads it and puts a `host` key right under the `repo` key. An
/// older remote binary ignores the variable; the card then has no
/// host line, but the source row still shows the host in the repo
/// column.
pub(crate) fn preview_remote(host: &str, cols: &[&str]) {
    let mut cmd = String::new();
    cmd.push_str("RUSHI_SESSIONS_PREVIEW_HOST=");
    cmd.push_str(&shell_quote(host));
    cmd.push(' ');
    cmd.push_str(&remote_binary());
    cmd.push_str(" preview");
    for c in cols {
        cmd.push(' ');
        cmd.push_str(&shell_quote(c));
    }
    match run_remote(host, &cmd) {
        Ok(out) => {
            use std::io::Write;
            let mut so = std::io::stdout();
            let _ = so.write_all(&out.stdout);
            let _ = so.flush();
            if !out.stderr.is_empty() {
                let mut se = std::io::stderr();
                let _ = se.write_all(&out.stderr);
            }
            std::process::exit(out.status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!(
                "rushi-sessions: remote preview failed for {} ({e})",
                host
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_remote_forms() {
        assert_eq!(split_remote("host:/x"), Some(("host", "/x")));
        assert_eq!(split_remote("u@host:/x"), Some(("u@host", "/x")));
        assert_eq!(split_remote("/a:b"), None);
        assert_eq!(split_remote("/x"), None);
        assert_eq!(split_remote("@host:/x"), None);
        assert_eq!(split_remote("a@b@h:/x"), None);
        assert_eq!(split_remote("host:"), None);
        assert_eq!(split_remote(":x"), None);
        assert_eq!(split_remote("a:b/x"), Some(("a", "b/x")));
    }


    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("ab"), "'ab'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }


    #[test]
    fn split_roots_groups_by_host() {
        let (local, groups) = split_roots(vec![
            PathBuf::from("/local"),
            PathBuf::from("h1:/a"),
            PathBuf::from("u@h2:/c"),
            PathBuf::from("h1:/b"),
        ]);
        assert_eq!(local, vec![PathBuf::from("/local")]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].host, "h1");
        assert_eq!(groups[0].roots, vec!["/a".to_string(), "/b".to_string()]);
        assert_eq!(groups[1].host, "u@h2");
        assert_eq!(groups[1].roots, vec!["/c".to_string()]);
    }


    #[test]
    fn parse_remote_line_prefixes_repo_and_path() {
        let line = "ACTIVE\tfoo\trepoA\ttools\t10-02 14:11\t1760000000\t/export/repoA/sessions/foo";
        let row = parse_remote_line(line, "host").unwrap();
        assert_eq!(row.repo, "host:repoA");
        assert_eq!(row.path, "host:/export/repoA/sessions/foo");
        assert_eq!(row.status, "ACTIVE");
        assert_eq!(row.mtime, 1760000000);
        assert_eq!(row.name, "foo");
    }


    #[test]
    fn parse_remote_line_user_at_host_prefix() {
        let line = "ACTIVE\tfoo\trepoA\ttools\t10-02 14:11\t1760000000\t/export/repoA/sessions/foo";
        let row = parse_remote_line(line, "u@host").unwrap();
        assert_eq!(row.repo, "u@host:repoA");
        assert_eq!(row.path, "u@host:/export/repoA/sessions/foo");
    }


    #[test]
    fn parse_remote_line_rejects_short_lines() {
        assert!(parse_remote_line("a\tb", "host").is_none());
    }


    #[test]
    fn parse_remote_line_normalizes_status() {
        let line = "WEIRD\tfoo\trepo\t?\t?\t5\t/p";
        let row = parse_remote_line(line, "h").unwrap();
        assert_eq!(row.status, "IDLE");
        assert_eq!(row.repo, "h:repo");
    }


}
