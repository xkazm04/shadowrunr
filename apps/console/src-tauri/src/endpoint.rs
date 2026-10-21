//! The daemon endpoint the console window talks to, resolved once at boot.
//!
//! One declared precedence chain: `--daemon <url>` (Arg) > `SHADOWRUNR_DAEMON_URL` (Env) >
//! `http://127.0.0.1:7717` (Default). An absent source is skipped; a present but malformed or
//! non-loopback value STOPS - it never falls through to a lower source. The window's CSP is
//! derived from the result, so it admits exactly one loopback origin (never a wildcard, never a
//! remote host) and, for the default, is byte-identical to the policy in `tauri.conf.json`.

use std::fmt;

/// The default daemon (crates/shadowrunr-daemon: `[daemon] port`, default 7717).
pub const DEFAULT_URL: &str = "http://127.0.0.1:7717";
/// The command-line flag: `--daemon <url>` or `--daemon=<url>`.
pub const ARG: &str = "--daemon";
/// The environment variable consulted when the flag is absent.
pub const ENV: &str = "SHADOWRUNR_DAEMON_URL";

/// Hosts the CSP may admit: loopback only, as today's static policy does.
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Arg,
    Env,
    Default,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Arg => ARG,
            Source::Env => ENV,
            Source::Default => "default",
        }
    }
}

/// A resolved endpoint: `url` is the normalized origin `http://<loopback-host>[:<port>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub url: String,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointError {
    pub source: Source,
    pub value: String,
    pub rule: &'static str,
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "daemon endpoint from {} is refused ({}): {:?}; expected an origin like {}",
            self.source.name(),
            self.rule,
            self.value,
            DEFAULT_URL
        )
    }
}

impl std::error::Error for EndpointError {}

const RULE_MALFORMED: &str = "malformed url";
const RULE_LOOPBACK: &str = "loopback only";
const RULE_ORIGIN: &str = "origin only: http, no credentials, path, query or fragment";
const RULE_MISSING: &str = "missing value";

/// Resolves the endpoint from the process arguments (without the program name) and an env
/// lookup. Pure: the caller passes `std::env::args().skip(1)` and `std::env::var`.
pub fn resolve<F>(args: &[String], env: F) -> Result<Endpoint, EndpointError>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = arg_value(args)? {
        return parse(&value, Source::Arg);
    }
    match env(ENV) {
        Some(value) if !value.is_empty() => parse(&value, Source::Env),
        _ => Ok(Endpoint {
            url: DEFAULT_URL.to_owned(),
            source: Source::Default,
        }),
    }
}

/// The window's CSP for `endpoint`: today's static policy with the daemon origin substituted.
pub fn csp_for(endpoint: &Endpoint) -> String {
    let http = &endpoint.url;
    let ws = format!("ws://{}", &http["http://".len()..]);
    format!(
        "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
         media-src 'self' {http}; connect-src 'self' ipc: http://ipc.localhost {http} {ws}"
    )
}

/// The script that hands the resolved endpoint to the console before any of its code runs.
pub fn init_script(endpoint: &Endpoint) -> String {
    // The url is a validated origin (scheme, loopback host, digits), so it needs no escaping
    // beyond the quotes Debug adds.
    format!(
        "Object.defineProperty(window, \"__SHADOWRUNR_DAEMON__\", {{ value: {:?}, writable: false }});",
        endpoint.url
    )
}

fn arg_value(args: &[String]) -> Result<Option<String>, EndpointError> {
    let mut found = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == ARG {
            match it.next() {
                Some(v) => found = Some(v.clone()),
                None => {
                    return Err(EndpointError {
                        source: Source::Arg,
                        value: String::new(),
                        rule: RULE_MISSING,
                    })
                }
            }
        } else if let Some(v) = a.strip_prefix("--daemon=") {
            found = Some(v.to_owned());
        }
    }
    Ok(found)
}

fn parse(value: &str, source: Source) -> Result<Endpoint, EndpointError> {
    let refuse = |rule| EndpointError {
        source,
        value: value.to_owned(),
        rule,
    };
    let rest = value
        .strip_prefix("http://")
        .ok_or_else(|| refuse(RULE_MALFORMED))?;
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() {
        return Err(refuse(RULE_MALFORMED));
    }
    if authority.contains(['/', '?', '#', '@']) {
        return Err(refuse(RULE_ORIGIN));
    }
    let (host, port) = split_host_port(authority).ok_or_else(|| refuse(RULE_MALFORMED))?;
    let host = host.to_ascii_lowercase();
    if !LOOPBACK_HOSTS.contains(&host.as_str()) {
        return Err(refuse(RULE_LOOPBACK));
    }
    let url = match port {
        Some(p) => format!("http://{host}:{p}"),
        None => format!("http://{host}"),
    };
    Ok(Endpoint { url, source })
}

/// `host[:port]` or `[v6][:port]`; the port, when present, is 1..=65535 in plain digits.
fn split_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (h, tail) = authority.split_at(end + 1);
        match tail {
            "" => (h, None),
            t => (h, Some(t.strip_prefix(':')?)),
        }
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        }
    };
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'))
    {
        return None;
    }
    let port = match port {
        None => None,
        Some(p) if !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) => {
            match p.parse::<u16>() {
                Ok(0) | Err(_) => return None,
                Ok(n) => Some(n),
            }
        }
        Some(_) => return None,
    };
    Some((host, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The policy `tauri.conf.json` carries, read from the file itself.
    fn conf_csp() -> String {
        let conf = include_str!("../tauri.conf.json");
        let start = conf
            .find("\"csp\": \"")
            .expect("tauri.conf.json carries a csp")
            + 8;
        let len = conf[start..].find('"').expect("csp string ends");
        conf[start..start + len].to_owned()
    }

    // GUARD: the default build's CSP is byte-identical to the static policy of HEAD 25c6267.
    #[test]
    fn default_endpoint_reproduces_todays_csp_byte_for_byte() {
        let e = resolve(&[], env(&[])).unwrap();
        assert_eq!(
            e,
            Endpoint {
                url: "http://127.0.0.1:7717".to_owned(),
                source: Source::Default
            }
        );
        let today = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; media-src 'self' http://127.0.0.1:7717; connect-src 'self' ipc: http://ipc.localhost http://127.0.0.1:7717 ws://127.0.0.1:7717";
        assert_eq!(csp_for(&e), today);
        assert_eq!(csp_for(&e), conf_csp());
    }

    #[test]
    fn the_arg_wins_over_the_env_and_the_csp_admits_only_its_port() {
        let e = resolve(
            &args(&["--daemon", "http://127.0.0.1:9000"]),
            env(&[(ENV, "http://127.0.0.1:8000")]),
        )
        .unwrap();
        assert_eq!(e.url, "http://127.0.0.1:9000");
        assert_eq!(e.source, Source::Arg);
        let csp = csp_for(&e);
        assert!(csp.contains(
            "connect-src 'self' ipc: http://ipc.localhost http://127.0.0.1:9000 ws://127.0.0.1:9000"
        ));
        assert!(csp.contains("media-src 'self' http://127.0.0.1:9000;"));
        assert!(!csp.contains("7717"));
        assert!(!csp.contains("8000"));
        assert!(!csp.contains('*'));
    }

    #[test]
    fn a_non_loopback_env_value_stops_with_its_source_and_rule() {
        let err = resolve(&[], env(&[(ENV, "http://10.0.0.5:7717")])).unwrap_err();
        assert_eq!(err.source, Source::Env);
        assert_eq!(err.rule, "loopback only");
        let msg = err.to_string();
        assert!(msg.contains("SHADOWRUNR_DAEMON_URL"), "{msg}");
        assert!(msg.contains("loopback only"), "{msg}");
    }

    #[test]
    fn a_malformed_env_value_stops_and_never_falls_through() {
        let err = resolve(&[], env(&[(ENV, "not a url")])).unwrap_err();
        assert_eq!(err.source, Source::Env);
        assert!(err.to_string().contains("SHADOWRUNR_DAEMON_URL"));
    }

    #[test]
    fn nothing_widens_the_policy_beyond_one_loopback_origin() {
        for bad in [
            "http://*:7717",
            "http://127.0.0.1.evil.com:7717",
            "http://evil.com",
            "http://localhost@evil.com:7717",
            "https://127.0.0.1:7717",
            "ws://127.0.0.1:7717",
            "http://127.0.0.1:7717/rpc",
            "http://127.0.0.1:7717?x=1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:70000",
            "http://127.0.0.1:77 17",
            "http://127.0.0.1:7717; script-src *",
            "http://[::2]:7717",
            "http://",
        ] {
            assert!(
                resolve(&args(&["--daemon", bad]), env(&[])).is_err(),
                "{bad} must be refused"
            );
        }
        assert!(resolve(&args(&["--daemon"]), env(&[])).is_err());
        for (good, url) in [
            ("http://localhost:9000/", "http://localhost:9000"),
            ("http://[::1]:9000", "http://[::1]:9000"),
            ("http://LOCALHOST:9000", "http://localhost:9000"),
        ] {
            let e = resolve(&args(&[&format!("--daemon={good}")]), env(&[])).unwrap();
            assert_eq!(e.url, url);
        }
    }

    #[test]
    fn an_empty_env_value_is_absent() {
        let e = resolve(&[], env(&[(ENV, "")])).unwrap();
        assert_eq!(e.source, Source::Default);
    }

    #[test]
    fn the_init_script_carries_the_url_as_a_string_literal() {
        let e = resolve(&args(&["--daemon", "http://127.0.0.1:9000"]), env(&[])).unwrap();
        assert!(init_script(&e).contains("value: \"http://127.0.0.1:9000\""));
    }
}
