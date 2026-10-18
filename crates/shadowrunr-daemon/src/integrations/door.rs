//! The Personas loopback door. The desktop app serves `/dev-tools`, `/project-tracking` and
//! friends on `127.0.0.1:<port>` and publishes `{pid, port, token}` in its handshake file
//! (`$PERSONAS_DATA_DIR/local-http.json` for an isolated instance, else
//! `~/.personas/local-http.json`). Every request carries `X-Personas-Local-Token` and a
//! loopback `Host`. The token is held in [`Token`], whose `Debug` is redacted; nothing here
//! prints it.
//!
//! Only read endpoints are called (`GET /dev-tools/projects`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// The header the door checks.
pub const TOKEN_HEADER: &str = "X-Personas-Local-Token";

/// The handshake file: `<PERSONAS_DATA_DIR>/local-http.json` when that variable is set and
/// not blank, else `<home>/.personas/local-http.json` (Personas `local_http::handshake_path`).
pub fn default_handshake_path(data_dir: Option<&str>, home: Option<PathBuf>) -> Option<PathBuf> {
    match data_dir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => Some(PathBuf::from(d).join("local-http.json")),
        None => home.map(|h| h.join(".personas").join("local-http.json")),
    }
}

/// The door's shared secret. Never printed: `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub pid: Option<u32>,
    pub port: u16,
    pub token: Token,
}

/// Why there is no usable handshake. Every variant reads as "Personas is not running".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    /// No file: the app is not running (it clears the file on a clean exit).
    Missing(PathBuf),
    /// A file that does not parse or lacks `port` / `token`.
    Malformed(PathBuf, String),
    /// A file whose port does not answer: the app died without clearing it.
    Stale { port: u16, pid: Option<u32> },
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(f, "no handshake at {}", p.display()),
            Self::Malformed(p, why) => write!(f, "handshake {} is malformed: {why}", p.display()),
            Self::Stale { port, pid } => match pid {
                Some(pid) => write!(f, "stale handshake: pid {pid}, port {port} does not answer"),
                None => write!(f, "stale handshake: port {port} does not answer"),
            },
        }
    }
}

#[derive(Deserialize)]
struct RawHandshake {
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    token: Option<String>,
}

/// Parse handshake text. Error messages never include the text itself (it holds the token).
pub fn parse_handshake(path: &Path, text: &str) -> Result<Handshake, HandshakeError> {
    let malformed = |why: &str| HandshakeError::Malformed(path.to_path_buf(), why.into());
    let raw: RawHandshake = serde_json::from_str(text).map_err(|_| malformed("not JSON"))?;
    let port = raw
        .port
        .filter(|p| *p != 0)
        .ok_or_else(|| malformed("no port"))?;
    let token = raw
        .token
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| malformed("no token"))?;
    Ok(Handshake {
        pid: raw.pid,
        port,
        token: Token(token),
    })
}

/// Read and parse the handshake at `path` (no network).
pub fn read_handshake(path: &Path) -> Result<Handshake, HandshakeError> {
    let text =
        std::fs::read_to_string(path).map_err(|_| HandshakeError::Missing(path.to_path_buf()))?;
    parse_handshake(path, &text)
}

/// `.personas/project.json`, written by Personas when the project was registered.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectFile {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

pub fn project_file_path(root: &Path) -> PathBuf {
    root.join(".personas").join("project.json")
}

pub fn read_project_file(root: &Path) -> Result<ProjectFile, String> {
    let path = project_file_path(root);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "{} not found (register the project in Personas)",
            path.display()
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// One row of `GET /dev-tools/projects` (only the fields used here).
#[derive(Debug, Clone, Deserialize)]
pub struct DevProject {
    pub id: String,
    pub name: String,
    pub root_path: String,
    #[serde(default)]
    pub status: Option<String>,
}

/// Paths as Personas stores them: canonical, without the `\\?\` prefix. Compared with
/// separators unified and, on Windows, case folded.
pub fn same_root(a: &str, b: &Path) -> bool {
    fn norm(s: &str) -> String {
        let s = s.strip_prefix(r"\\?\").unwrap_or(s);
        let s = s.replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    }
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    norm(a) == norm(&b.to_string_lossy())
}

/// A client for the door at a handshake's port.
pub struct Door {
    base: String,
    token: Token,
    http: reqwest::Client,
}

impl Door {
    pub fn new(h: &Handshake, timeout: Duration) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .no_proxy()
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Self {
            // `127.0.0.1` is on the door's Host allowlist, as is the port it bound.
            base: format!("http://127.0.0.1:{}", h.port),
            token: h.token.clone(),
            http,
        })
    }

    /// GET a read endpoint. `Err(None)` = the door did not answer (stale handshake);
    /// `Err(Some(msg))` = it answered with an error.
    async fn get(&self, path: &str) -> Result<String, Option<String>> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .header(TOKEN_HEADER, self.token.as_str())
            .send()
            .await
            .map_err(|_| None)?;
        let status = resp.status();
        let body = resp.text().await.map_err(|_| None)?;
        if status.is_success() {
            Ok(body)
        } else if status.as_u16() == 401 || status.as_u16() == 403 {
            Err(Some(format!(
                "door refused the token (HTTP {status}); the handshake may belong to another instance"
            )))
        } else {
            let snippet: String = body.chars().take(200).collect();
            Err(Some(format!("HTTP {status}: {snippet}")))
        }
    }

    /// `GET /dev-tools/projects`.
    pub async fn projects(&self) -> Result<Vec<DevProject>, Option<String>> {
        let body = self.get("/dev-tools/projects").await?;
        serde_json::from_str(&body).map_err(|e| Some(format!("/dev-tools/projects: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "local-http.json";

    #[test]
    fn handshake_path_prefers_an_isolated_data_dir() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            default_handshake_path(None, home.clone()),
            Some(PathBuf::from("/home/u/.personas/local-http.json"))
        );
        assert_eq!(
            default_handshake_path(Some("  "), home.clone()),
            Some(PathBuf::from("/home/u/.personas/local-http.json"))
        );
        assert_eq!(
            default_handshake_path(Some("/data/x"), home),
            Some(PathBuf::from("/data/x/local-http.json"))
        );
        assert_eq!(default_handshake_path(None, None), None);
    }

    #[test]
    fn a_good_handshake_parses_and_hides_its_token() {
        let h = parse_handshake(
            Path::new(P),
            r#"{"pid": 43640, "port": 17400, "token": "abcdef0123456789"}"#,
        )
        .unwrap();
        assert_eq!((h.pid, h.port), (Some(43640), 17400));
        assert_eq!(h.token.as_str(), "abcdef0123456789");
        let shown = format!("{h:?}");
        assert!(!shown.contains("abcdef0123456789"), "{shown}");
    }

    #[test]
    fn malformed_handshakes_name_the_problem_not_the_content() {
        for (text, why) in [
            ("not json at all, token=abcdef0123456789", "not JSON"),
            (r#"{"pid": 1, "token": "abcdef0123456789"}"#, "no port"),
            (r#"{"port": 0, "token": "abcdef0123456789"}"#, "no port"),
            (r#"{"port": 17400, "token": "   "}"#, "no token"),
            (r#"{"port": 17400}"#, "no token"),
        ] {
            let e = parse_handshake(Path::new(P), text).unwrap_err();
            let shown = e.to_string();
            assert!(shown.contains(why), "{shown}");
            assert!(!shown.contains("abcdef0123456789"), "{shown}");
        }
    }

    #[test]
    fn a_missing_handshake_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(P);
        assert_eq!(read_handshake(&p), Err(HandshakeError::Missing(p.clone())));
    }

    #[tokio::test]
    async fn a_stale_handshake_does_not_answer() {
        // A port nothing listens on: bind, note it, drop the listener.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let h = parse_handshake(
            Path::new(P),
            &format!(r#"{{"pid": 99999, "port": {port}, "token": "abcdef0123456789"}}"#),
        )
        .unwrap();
        let door = Door::new(&h, Duration::from_millis(500)).unwrap();
        assert_eq!(door.projects().await.unwrap_err(), None);
    }

    #[test]
    fn roots_compare_like_personas_stores_them() {
        let tmp = tempfile::tempdir().unwrap();
        let canon = tmp.path().canonicalize().unwrap();
        let stored = canon.to_string_lossy().to_string();
        let stripped = stored.strip_prefix(r"\\?\").unwrap_or(&stored).to_string();
        assert!(same_root(&stripped, tmp.path()));
        assert!(same_root(&format!("{stripped}/"), tmp.path()));
        assert!(!same_root("/somewhere/else", tmp.path()));
        if cfg!(windows) {
            assert!(same_root(&stripped.to_uppercase(), tmp.path()));
        }
    }
}
