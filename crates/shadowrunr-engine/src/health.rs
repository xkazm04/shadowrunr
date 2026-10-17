//! Pre-flight `PF-01 engine`. Nothing here spends quota: the CLI version comes from
//! `claude --version`, each account's login from `claude auth status --json` (listed by
//! `claude auth --help`; it reads local credentials only), with the presence of
//! `<config dir>/.credentials.json` as the fallback signal. Reports carry labels and
//! states only; the email and organisation fields of `auth status` are never read out.

use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use shadowrunr_types::{HealthReport, HealthState};

use crate::claude_cli::Account;
use crate::config::FallbackDriver;
use crate::fleet::FleetInner;

pub const CHECK_ID: &str = "PF-01";
/// The CLI version the driver's flags were verified against.
pub const VERIFIED_CLI_VERSION: &str = "2.1.302";

fn report(state: HealthState, detail: String) -> HealthReport {
    HealthReport {
        check: CHECK_ID.into(),
        state,
        detail,
    }
}

async fn run_cli(
    inner: &FleetInner,
    account: Option<&Account>,
    args: &[&str],
) -> Option<(bool, String)> {
    let default = Account {
        label: String::new(),
        config_dir: None,
    };
    let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut cmd = inner.cli.command(account.unwrap_or(&default), &argv);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .ok()?
        .ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// Parse `claude --version` output ("2.1.302 (Claude Code)").
pub fn parse_version(stdout: &str) -> Option<String> {
    let first = stdout.split_whitespace().next()?;
    first
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
        .then(|| first.to_string())
}

/// Read `loggedIn` / `authMethod` / `subscriptionType` from `auth status --json`; nothing else.
pub fn parse_auth_status(stdout: &str) -> Option<(bool, String)> {
    let start = stdout.find('{')?;
    let v: Value = serde_json::from_str(&stdout[start..]).ok()?;
    let logged_in = v.get("loggedIn")?.as_bool()?;
    let method = v.get("authMethod").and_then(Value::as_str).unwrap_or("?");
    let plan = v
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(|p| format!(", {p}"))
        .unwrap_or_default();
    Some((logged_in, format!("{method}{plan}")))
}

pub async fn preflight(inner: &FleetInner) -> Vec<HealthReport> {
    let mut out = Vec::new();
    let bin = &inner.cli.bin;

    let cli_ok = match run_cli(inner, None, &["--version"]).await {
        Some((true, stdout)) => match parse_version(&stdout) {
            Some(v) if v == VERIFIED_CLI_VERSION => {
                out.push(report(HealthState::Green, format!("cli: claude {v}")));
                true
            }
            Some(v) => {
                out.push(report(
                    HealthState::Amber,
                    format!(
                        "cli: claude {v}; flags were verified against {VERIFIED_CLI_VERSION}, re-check `claude --help`"
                    ),
                ));
                true
            }
            None => {
                out.push(report(
                    HealthState::Amber,
                    format!("cli: `{bin} --version` printed no version"),
                ));
                true
            }
        },
        _ => {
            let state = if inner.api.is_some() {
                HealthState::Amber
            } else {
                HealthState::Red
            };
            out.push(report(
                state,
                format!("cli: `{bin}` not found or not runnable"),
            ));
            false
        }
    };

    let status = inner.account_status();
    for (account, st) in inner.accounts.iter().zip(status) {
        let label = &account.label;
        if !cli_ok {
            out.push(report(
                HealthState::Red,
                format!("account {label}: cli missing"),
            ));
            continue;
        }
        let login = match run_cli(inner, Some(account), &["auth", "status", "--json"]).await {
            Some((_, stdout)) => parse_auth_status(&stdout),
            None => None,
        };
        let creds_file = account
            .config_dir
            .as_ref()
            .map(|d| d.join(".credentials.json").is_file());
        let (mut state, mut detail) = match (login, creds_file) {
            (Some((true, how)), _) => (HealthState::Green, format!("logged in ({how})")),
            (Some((false, _)), _) => (HealthState::Red, "not logged in".to_string()),
            (None, Some(true)) => (
                HealthState::Amber,
                "credentials file present; auth status unavailable".to_string(),
            ),
            (None, _) => (HealthState::Red, "login unknown".to_string()),
        };
        if let Some(until) = st.cooling_until_ms {
            if state == HealthState::Green {
                state = HealthState::Amber;
            }
            detail.push_str(&format!("; cooling until {until}"));
        }
        out.push(report(state, format!("account {label}: {detail}")));
    }
    if inner.accounts.is_empty() {
        out.push(report(
            HealthState::Amber,
            "accounts: none configured; the API key is the only engine".into(),
        ));
    }

    match &inner.api {
        Some(api) if api.key_present() => out.push(report(
            HealthState::Green,
            format!("api: {} present", api.key_env),
        )),
        Some(api) => out.push(report(
            HealthState::Amber,
            format!(
                "api: {} is not set; no fallback after the fleet",
                api.key_env
            ),
        )),
        None => out.push(report(
            HealthState::Amber,
            "api: no api_key_env configured; no fallback after the fleet".into(),
        )),
    }

    for d in &inner.fallbacks {
        let (state, detail) = match d {
            FallbackDriver::LocalGemma => (
                HealthState::Red,
                "fallback local_gemma: not implemented in v0.1".to_string(),
            ),
            FallbackDriver::Codex => (
                HealthState::Amber,
                "fallback codex: experimental".to_string(),
            ),
            FallbackDriver::Agy => (HealthState::Amber, "fallback agy: experimental".to_string()),
        };
        out.push(report(state, detail));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_and_auth_status_without_identity() {
        assert_eq!(
            parse_version("2.1.302 (Claude Code)\n").as_deref(),
            Some("2.1.302")
        );
        assert_eq!(parse_version("error: nope"), None);
        let s = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"x@example.invalid","subscriptionType":"max"}"#;
        let (ok, how) = parse_auth_status(s).unwrap();
        assert!(ok);
        assert_eq!(how, "claude.ai, max");
        assert!(!how.contains('@'));
        assert!(!parse_auth_status(r#"{"loggedIn":false}"#).unwrap().0);
    }
}
