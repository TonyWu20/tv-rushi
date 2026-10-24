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
/// `rushi config` of the session's project. The project dir carries
/// a flake and `nix` is on PATH: the devShell launch (decision 88),
/// so the project's own config tier is probed. Otherwise the `rushi`
/// on PATH. The probe reads the active model's `context_tokens`,
/// then `limits.context_budget_tokens`. None when nothing resolves.
pub(crate) fn context_window(dir: Option<&std::path::Path>) -> Option<u64> {
    if let Ok(v) = std::env::var("RUSHI_CONTEXT_WINDOW") {
        if let Ok(n) = v.trim().parse::<u64>() {
            if n > 0 {
                return Some(n);
            }
        }
    }
    let (argv, cwd) = context_window_cmd(dir);
    let mut c = std::process::Command::new(&argv[0]);
    for a in &argv[1..] {
        c.arg(a);
    }
    if let Some(d) = &cwd {
        c.current_dir(d);
    }
    let out = c.stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    context_window_from_config(&String::from_utf8_lossy(&out.stdout))
}

/// The `rushi config` invocation (decision 88): the devShell launch
/// when the session's project dir carries a flake and `nix` is on
/// PATH (the devShell packages shadow the inherited PATH, and a
/// binary the devShell does not ship falls back to the inherited
/// entry), else the PATH `rushi`.
fn context_window_cmd(dir: Option<&std::path::Path>) -> (Vec<String>, Option<String>) {
    if let Some(d) = dir {
        if crate::prompt::nix_dev_dir(&d.to_string_lossy()) {
            return (
                vec![
                    "nix".to_string(),
                    "develop".to_string(),
                    "-c".to_string(),
                    "rushi".to_string(),
                    "config".to_string(),
                ],
                Some(d.to_string_lossy().into_owned()),
            );
        }
    }
    (vec!["rushi".to_string(), "config".to_string()], None)
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

    /// The `rushi config` invocation of the usage line (decision
    /// 88): the devShell launch when the session's project dir
    /// carries a flake and nix is on PATH, else the PATH probe.
    #[test]
    fn config_probe_cmd_selection() {
        // No dir (no recorded working dir): the PATH probe.
        let (argv, cwd) = context_window_cmd(None);
        assert_eq!(argv, vec!["rushi".to_string(), "config".to_string()]);
        assert!(cwd.is_none());

        let base = std::env::temp_dir().join(format!("rsrs-cfgprobe-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);

        // A flake-less dir: the PATH probe.
        let bare = base.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        let (argv, cwd) = context_window_cmd(Some(bare.as_path()));
        assert_eq!(argv, vec!["rushi".to_string(), "config".to_string()]);
        assert!(cwd.is_none());

        // The flake dir: the devShell launch when the host has nix
        // (the gate's last check), the PATH probe otherwise.
        let flake = base.join("flake");
        std::fs::create_dir_all(&flake).unwrap();
        std::fs::write(flake.join("flake.nix"), "{}").unwrap();
        let f = flake.to_str().unwrap().to_string();
        let (argv, cwd) = context_window_cmd(Some(flake.as_path()));
        if crate::prompt::nix_dev_dir(&f) {
            assert_eq!(
                argv,
                vec![
                    "nix".to_string(),
                    "develop".to_string(),
                    "-c".to_string(),
                    "rushi".to_string(),
                    "config".to_string(),
                ]
            );
            assert_eq!(cwd, Some(f));
        } else {
            assert_eq!(argv, vec!["rushi".to_string(), "config".to_string()]);
            assert!(cwd.is_none());
        }
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
