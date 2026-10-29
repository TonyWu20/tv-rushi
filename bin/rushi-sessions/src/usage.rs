//! Token usage and context-window resolution for the preview card.
use serde_json::Value;

/// The usage of the last `assistant_message` that carries real usage:
/// `(input_tokens, output_tokens, cached_tokens)`. Messages whose `usage`
/// is missing, null, or all-zero are skipped.
pub(crate) fn last_usage(events: &[Value]) -> Option<(u64, u64, u64)> {
    for o in events.iter().rev() {
        if o.get("type").and_then(|v| v.as_str()) != Some("assistant_message") {
            continue;
        }
        let u = match o.get("usage") {
            Some(Value::Object(_)) => o.get("usage").unwrap(),
            _ => continue,
        };
        let f = |k: &str| u.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        let in_tok = f("input_tokens");
        let out_tok = f("output_tokens");
        let cached = f("cached_tokens");
        if in_tok == 0 && out_tok == 0 {
            continue;
        }
        return Some((in_tok, out_tok, cached));
    }
    None
}
/// The text of the `usage` line: `used/window tokens (pct%)` when the
/// context window is known, else `used tokens`.
pub(crate) fn usage_line(used: u64, window: Option<u64>) -> String {
    match window {
        Some(w) if w > 0 => {
            let pct = (u128::from(used).saturating_mul(100) + u128::from(w) / 2) / u128::from(w);
            format!("{used}/{w} tokens ({pct}%)")
        }
        _ => format!("{used} tokens"),
    }
}
/// The context window in tokens. Resolution order: the
/// `RUSHI_CONTEXT_WINDOW` env var (a positive integer), then the
/// `rushi config` of the session's project. The project's effective
/// `.envrc` (this dir, then its ancestors) names the devShell flake
/// (decision 95): that flake's devShell `rushi` is probed; a dir
/// with no `.envrc` on the chain probes the `rushi` on PATH. The
/// probe reads the active model's `context_tokens`, then
/// `limits.context_budget_tokens`. None when nothing resolves.
///
/// The devShell binary is a content-addressed store path that
/// changes only when the flake changes (decision 89). It is
/// resolved once, cached with a generation fingerprint, and exec'd
/// directly on later calls, so a repeated preview never re-runs the
/// heavy `nix develop` probe. A probe that cannot resolve the
/// devShell binary falls back to the PATH `rushi config`.
pub(crate) fn context_window(dir: Option<&std::path::Path>) -> Option<u64> {
    if let Ok(v) = std::env::var("RUSHI_CONTEXT_WINDOW") {
        if let Ok(n) = v.trim().parse::<u64>() {
            if n > 0 {
                return Some(n);
            }
        }
    }
    match dir {
        // The `.envrc` gate lives inside `devshell_context_window`: a
        // dir with no `.envrc` on the chain resolves to None there,
        // and the PATH probe takes over.
        Some(d) => devshell_context_window(d).or_else(path_context_window),
        None => path_context_window(),
    }
}

/// The fast `rushi config` probe on PATH (the pre-88 path).
fn path_context_window() -> Option<u64> {
    let out = std::process::Command::new("rushi")
        .arg("config")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    context_window_from_config(&String::from_utf8_lossy(&out.stdout))
}

/// The devShell probe (decision 89): run the project's own `rushi`
/// and read its config. The `.envrc` flake dir (decision 95) is the
/// flake to materialize; the binary's store path is resolved once
/// per devShell generation and cached by `devshell::resolve`, so a
/// repeated preview execs the cached path directly instead of
/// re-running the heavy `nix develop` probe. None when no `.envrc`
/// is found on the chain, or when the devShell binary does not
/// resolve; the caller falls back to the PATH probe.
fn devshell_context_window(dir: &std::path::Path) -> Option<u64> {
    let fd = crate::devshell::envrc_flake_dir(dir)?;
    let bin = crate::devshell::resolve(fd.as_path(), "rushi")?;
    run_config(&bin)
}

/// Run `<bin> config` and parse the context window.
fn run_config(bin: &std::path::Path) -> Option<u64> {
    let out = std::process::Command::new(bin)
        .arg("config")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    context_window_from_config(&String::from_utf8_lossy(&out.stdout))
}

/// The context window in tokens parsed from `rushi config` output:
/// `[model."<active>"] context_tokens`, then the base `[model]`
/// `context_tokens`, then `limits.context_budget_tokens`.
fn context_window_from_config(text: &str) -> Option<u64> {
    let v: toml::Value = toml::from_str(text).ok()?;
    let num = |v: &toml::Value| v.as_integer().filter(|n| *n > 0).map(|n| n as u64);
    let model = v.get("model");
    let active = v
        .get("active")
        .and_then(|a| a.get("model"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if !active.is_empty() {
        if let Some(n) = model
            .and_then(|m| m.get(active))
            .and_then(|p| p.get("context_tokens"))
            .and_then(num)
        {
            return Some(n);
        }
    }
    if let Some(n) = model.and_then(|m| m.get("context_tokens")).and_then(num) {
        return Some(n);
    }
    v.get("limits")
        .and_then(|l| l.get("context_budget_tokens"))
        .and_then(num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_usage_picks_last_with_data() {
        let e1 = serde_json::json!({
            "type": "assistant_message",
            "usage": {"input_tokens": 100, "output_tokens": 10, "cached_tokens": 0}
        });
        let e2 = serde_json::json!({
            "type": "assistant_message",
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });
        let e3 = serde_json::json!({
            "type": "assistant_message",
            "usage": {"input_tokens": 200, "output_tokens": 20, "cached_tokens": 5}
        });
        let evts = vec![e1.clone(), e2.clone(), e3.clone()];
        assert_eq!(last_usage(&evts), Some((200, 20, 5)));
        // All-zero and missing usage are skipped.
        let evts = vec![e2, e1];
        assert_eq!(last_usage(&evts), Some((100, 10, 0)));
        let e4 = serde_json::json!({"type": "assistant_message"});
        assert_eq!(last_usage(&[e4]), None);
        assert_eq!(last_usage(&[]), None);
    }

    #[test]
    fn usage_line_formats_with_and_without_window() {
        assert_eq!(
            usage_line(109_967, Some(262_144)),
            "109967/262144 tokens (42%)"
        );
        assert_eq!(usage_line(500, None), "500 tokens");
        // A window smaller than the usage reports over 100%.
        assert_eq!(usage_line(200, Some(100)), "200/100 tokens (200%)");
        assert_eq!(usage_line(10_000, Some(0)), "10000 tokens");
    }

    #[test]
    fn config_window_prefers_active_model() {
        let text = r#"
[active]
model = "m1"
[model]
context_tokens = 111
[model.m1]
context_tokens = 222
[limits]
context_budget_tokens = 333
"#;
        assert_eq!(context_window_from_config(text), Some(222));
    }

    /// Probe routing (decision 89, re-gated by 95): a session
    /// project dir routes to the devShell probe only when an
    /// effective `.envrc` (this dir, then ancestors) names a flake;
    /// a dir with no `.envrc` on the chain routes to the PATH
    /// `rushi config`.
    #[test]
    fn probe_routing_gate() {
        let base = std::env::temp_dir().join(format!("rsrs-probe-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);

        // No `.envrc` on the chain: the dir does not route to the
        // devShell probe.
        let bare = base.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        assert_eq!(crate::devshell::envrc_flake_dir(bare.as_path()), None);

        // An `.envrc` loading this dir's flake routes to the
        // devShell probe.
        let proj = base.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join(".envrc"), "use flake .\n").unwrap();
        assert_eq!(
            crate::devshell::envrc_flake_dir(proj.as_path()),
            Some(proj.canonicalize().unwrap())
        );
    }

    #[test]
    fn config_window_falls_backs_down() {
        // No active model: the base [model] section wins.
        let text = r#"
[model]
context_tokens = 111
[limits]
context_budget_tokens = 333
"#;
        assert_eq!(context_window_from_config(text), Some(111));
        // No [model] at all: the limits budget wins.
        let text = r#"
[limits]
context_budget_tokens = 333
"#;
        assert_eq!(context_window_from_config(text), Some(333));
        // Nothing usable: None.
        let text = r#"
[limits]
read_limit = 2000
"#;
        assert_eq!(context_window_from_config(text), None);
        // Invalid TOML: None.
        assert_eq!(context_window_from_config("not toml [[["), None);
    }
}
