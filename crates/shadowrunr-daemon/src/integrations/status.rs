//! `shadowrunr personas status`: is Personas running, is this repo registered as the
//! project `.personas/project.json` names, and what the integration is configured to do.
//! Exit codes: 0 verified, 3 Personas not running (no, malformed or stale handshake),
//! 1 running but the project is not verified. The door token is never printed.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use super::door::{self, Door, HandshakeError};
use super::settings::PersonasSettings;
use crate::config::Config;

pub const EXIT_OK: u8 = 0;
pub const EXIT_UNVERIFIED: u8 = 1;
pub const EXIT_NOT_RUNNING: u8 = 3;

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    /// `running` or `not_running`.
    pub personas: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `verified`, `not_registered`, `id_mismatch`, `no_project_file`, `door_error` or
    /// `unknown` (Personas not running).
    pub project: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    pub root: String,
    pub detail: String,
    /// `off`, `on` or `misconfigured`.
    pub activity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_detail: Option<String>,
    pub mcp_allow_writes: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_error: Option<String>,
    #[serde(skip)]
    pub exit: u8,
}

fn activity_summary(s: &PersonasSettings) -> (&'static str, Option<String>) {
    let Some(a) = s.activity_on() else {
        return ("off", None);
    };
    let problems = a.problems();
    if !problems.is_empty() {
        return ("misconfigured", Some(problems.join("; ")));
    }
    let secret = if std::env::var(a.secret_env.trim()).is_ok_and(|v| !v.trim().is_empty()) {
        "set"
    } else {
        "NOT SET"
    };
    (
        "on",
        Some(format!(
            "{} (secret env {}: {secret}); events {}",
            a.endpoint(),
            a.secret_env.trim(),
            a.events.join(", ")
        )),
    )
}

/// Gather the status. Only `GET /dev-tools/projects` is called.
pub async fn status(cfg: &Config, timeout: Duration) -> StatusReport {
    let (settings, settings_error) = match PersonasSettings::load(cfg) {
        Ok(s) => (s, None),
        Err(e) => (PersonasSettings::default(), Some(e)),
    };
    let (activity, activity_detail) = activity_summary(&settings);
    let mut r = StatusReport {
        personas: "not_running",
        pid: None,
        port: None,
        project: "unknown",
        project_id: None,
        project_name: None,
        root: cfg.root.display().to_string(),
        detail: String::new(),
        activity,
        activity_detail,
        mcp_allow_writes: settings.mcp_allow_writes,
        settings_error,
        exit: EXIT_NOT_RUNNING,
    };
    let Some(path) = settings.handshake_path(&cfg.root) else {
        r.detail = "Personas not running (no home directory to find the handshake in)".into();
        return r;
    };
    let h = match door::read_handshake(&path) {
        Ok(h) => h,
        Err(e) => {
            r.detail = format!("Personas not running ({e})");
            return r;
        }
    };
    let projects = match Door::new(&h, timeout) {
        Ok(d) => d.projects().await,
        Err(e) => Err(Some(e)),
    };
    let projects = match projects {
        Ok(p) => p,
        Err(None) => {
            let stale = HandshakeError::Stale {
                port: h.port,
                pid: h.pid,
            };
            r.detail = format!("Personas not running ({stale})");
            return r;
        }
        Err(Some(msg)) => {
            r.personas = "running";
            r.pid = h.pid;
            r.port = Some(h.port);
            r.project = "door_error";
            r.detail = msg;
            r.exit = EXIT_UNVERIFIED;
            return r;
        }
    };
    r.personas = "running";
    r.pid = h.pid;
    r.port = Some(h.port);
    r.exit = EXIT_UNVERIFIED;
    verify_project(&mut r, &cfg.root, &projects);
    r
}

fn verify_project(r: &mut StatusReport, root: &Path, projects: &[door::DevProject]) {
    let file = match door::read_project_file(root) {
        Ok(f) => f,
        Err(e) => {
            r.project = "no_project_file";
            r.detail = e;
            return;
        }
    };
    r.project_id = Some(file.id.clone());
    r.project_name = file.name.clone();
    let at_root = projects
        .iter()
        .find(|p| door::same_root(&p.root_path, root));
    match at_root {
        Some(p) if p.id == file.id => {
            r.project = "verified";
            r.project_name = Some(p.name.clone());
            r.detail = format!(
                "registered as {} ({}){}",
                p.name,
                p.id,
                p.status
                    .as_deref()
                    .map(|s| format!(", status {s}"))
                    .unwrap_or_default()
            );
            r.exit = EXIT_OK;
        }
        Some(p) => {
            r.project = "id_mismatch";
            r.detail = format!(
                ".personas/project.json names {} but Personas registers this root as {} ({})",
                file.id, p.id, p.name
            );
        }
        None => {
            r.project = "not_registered";
            r.detail = match projects.iter().find(|p| p.id == file.id) {
                Some(p) => format!(
                    "project {} is registered at {}, not at this root",
                    p.id, p.root_path
                ),
                None => format!(
                    "no Personas project has root_path {} (register it in Personas)",
                    root.display()
                ),
            };
        }
    }
}

/// Human-readable lines.
pub fn render(r: &StatusReport) -> String {
    let mut out = Vec::new();
    match (r.personas, r.port) {
        ("running", Some(port)) => out.push(format!(
            "personas  running (port {port}{})",
            r.pid.map(|p| format!(", pid {p}")).unwrap_or_default()
        )),
        _ => out.push(format!("personas  not running: {}", r.detail)),
    }
    if r.personas == "running" {
        out.push(format!("project   {}: {}", r.project, r.detail));
    }
    out.push(format!("root      {}", r.root));
    out.push(match &r.activity_detail {
        Some(d) => format!("activity  {}: {d}", r.activity),
        None => format!("activity  {}", r.activity),
    });
    out.push(format!(
        "mcp       {}",
        if r.mcp_allow_writes {
            "writes allowed (start_sim)"
        } else {
            "read-only ([personas] mcp_allow_writes = true enables start_sim)"
        }
    ));
    if let Some(e) = &r.settings_error {
        out.push(format!("settings  {e}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::get;
    use axum::{Json, Router};
    use serde_json::{json, Value};

    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// A fake door: `GET /dev-tools/projects`, token-checked.
    async fn door(projects: Value) -> u16 {
        let projects = Arc::new(projects);
        let app = Router::new().route(
            "/dev-tools/projects",
            get(move |headers: HeaderMap| {
                let projects = projects.clone();
                async move {
                    let ok = headers
                        .get("x-personas-local-token")
                        .and_then(|v| v.to_str().ok())
                        == Some(TOKEN);
                    if ok {
                        (StatusCode::OK, Json((*projects).clone()))
                    } else {
                        (
                            StatusCode::UNAUTHORIZED,
                            Json(json!({"error": "unauthorized"})),
                        )
                    }
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(l, app).await });
        port
    }

    fn setup(tmp: &Path, port: Option<u16>, project_id: Option<&str>) -> Config {
        let hs = tmp.join("local-http.json");
        if let Some(port) = port {
            std::fs::write(
                &hs,
                json!({ "pid": 4242, "port": port, "token": TOKEN }).to_string(),
            )
            .unwrap();
        }
        if let Some(id) = project_id {
            std::fs::create_dir_all(tmp.join(".personas")).unwrap();
            std::fs::write(
                tmp.join(".personas/project.json"),
                json!({ "schema": 1, "id": id, "name": "overlord" }).to_string(),
            )
            .unwrap();
        }
        let cfg_path = tmp.join("shadowrunr.toml");
        std::fs::write(
            &cfg_path,
            format!(
                "[personas]\nhandshake = {:?}\n",
                hs.to_string_lossy().to_string()
            ),
        )
        .unwrap();
        Config::load(Some(&cfg_path), tmp).unwrap()
    }

    fn root_of(tmp: &Path) -> String {
        let c = tmp.canonicalize().unwrap().to_string_lossy().to_string();
        c.strip_prefix(r"\\?\").unwrap_or(&c).to_string()
    }

    fn no_token(r: &StatusReport) {
        let all = format!("{}\n{}", render(r), serde_json::to_string(r).unwrap());
        assert!(
            !all.contains(TOKEN),
            "the token must never be printed: {all}"
        );
    }

    #[tokio::test]
    async fn not_running_without_a_handshake() {
        let tmp = tempfile::tempdir().unwrap();
        let r = status(&setup(tmp.path(), None, Some("p1")), Duration::from_secs(2)).await;
        assert_eq!((r.personas, r.exit), ("not_running", EXIT_NOT_RUNNING));
        assert!(r.detail.contains("no handshake"), "{}", r.detail);
    }

    #[tokio::test]
    async fn not_running_with_a_stale_handshake() {
        let tmp = tempfile::tempdir().unwrap();
        let dead = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let r = status(
            &setup(tmp.path(), Some(dead), Some("p1")),
            Duration::from_millis(500),
        )
        .await;
        assert_eq!((r.personas, r.exit), ("not_running", EXIT_NOT_RUNNING));
        assert!(
            r.detail.contains("stale handshake: pid 4242"),
            "{}",
            r.detail
        );
        no_token(&r);
    }

    #[tokio::test]
    async fn verified_when_the_root_and_id_match() {
        let tmp = tempfile::tempdir().unwrap();
        let port = door(json!([
            { "id": "other", "name": "x", "root_path": "/elsewhere", "status": "active" },
            { "id": "p1", "name": "overlord", "root_path": root_of(tmp.path()), "status": "active" }
        ]))
        .await;
        let r = status(
            &setup(tmp.path(), Some(port), Some("p1")),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(
            (r.personas, r.project, r.exit),
            ("running", "verified", EXIT_OK),
            "{}",
            r.detail
        );
        assert_eq!(r.port, Some(port));
        no_token(&r);
    }

    #[tokio::test]
    async fn unverified_on_mismatch_or_missing_registration() {
        let tmp = tempfile::tempdir().unwrap();
        let port = door(json!([
            { "id": "p2", "name": "overlord", "root_path": root_of(tmp.path()) }
        ]))
        .await;
        let r = status(
            &setup(tmp.path(), Some(port), Some("p1")),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(
            (r.project, r.exit),
            ("id_mismatch", EXIT_UNVERIFIED),
            "{}",
            r.detail
        );

        let tmp2 = tempfile::tempdir().unwrap();
        let port = door(json!([])).await;
        let r = status(
            &setup(tmp2.path(), Some(port), Some("p1")),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!((r.project, r.exit), ("not_registered", EXIT_UNVERIFIED));

        let tmp3 = tempfile::tempdir().unwrap();
        let r = status(
            &setup(tmp3.path(), Some(port), None),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!((r.project, r.exit), ("no_project_file", EXIT_UNVERIFIED));
        no_token(&r);
    }
}
