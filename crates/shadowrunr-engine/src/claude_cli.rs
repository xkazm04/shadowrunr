//! The `claude` driver: argv builder, child environment, and the one-shot call.
//!
//! Every flag used here is listed by `claude --help` on Claude Code 2.1.302, except
//! `--system-prompt-file`, which the help names only inside the `--bare` description
//! ("--system-prompt[-file]"); it was verified to exist without spending quota
//! (`claude -p --system-prompt-file <missing>` answers "System prompt file not found").
//! `--dangerously-skip-permissions` is never passed.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use shadowrunr_types::{Effort, EngineError, EngineRequest, ModelTier};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::admission::Admission;
use crate::limits::Failure;
use crate::model::{effort_str, model_id};
use crate::stream::{Feed, TurnOutcome, TurnParser};

/// `CREATE_NO_WINDOW`: spawned CLIs never flash a console window on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Env vars stripped from every CLI child: API credentials (so the plan login is used) and the
/// session variables a parent Claude Code session exports. `CLAUDE_CODE_PLUGIN_DIRS` in
/// particular loads the operator's own dev-mod plugins into every child, even with an
/// isolated `CLAUDE_CONFIG_DIR` (found live, 2026-10-18).
pub const STRIPPED_ENV: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_PLUGIN_DIRS",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "AI_AGENT",
];

/// One fleet account, by label and config dir only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub label: String,
    /// CLAUDE_CONFIG_DIR; `None` = the default login.
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallMode {
    /// `claude -p`, prompt on stdin, stateless (`--no-session-persistence`).
    OneShot,
    /// `claude -p --input-format stream-json`, one long-lived process. `resume` re-attaches
    /// to an earlier session after a crash.
    Warm { resume: Option<String> },
}

#[derive(Debug, Clone)]
pub struct ArgvSpec<'a> {
    pub tier: ModelTier,
    pub effort: Effort,
    pub system_prompt_file: &'a Path,
    pub allowed_tools: &'a [String],
    pub json_schema: Option<&'a Value>,
    pub mode: CallMode,
    pub safe_mode: bool,
}

/// The built-in tool names of an allowlist whose entries may carry a permission specifier
/// (`WebFetch(domain:sec.gov)` names the tool `WebFetch`), deduplicated in order.
pub fn tool_names(allowed: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in allowed {
        let name = t.split('(').next().unwrap_or(t).trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Build the argv (without the executable) for one `claude` process.
///
/// `allowed_tools` entries may carry a permission specifier: `WebFetch(domain:sec.gov)`
/// enables the `WebFetch` tool (`--tools`) and pre-approves it for that domain only
/// (`--allowedTools`); with `--permission-prompts none` a fetch of any other domain is denied.
pub fn claude_argv(spec: &ArgvSpec<'_>) -> Vec<String> {
    let mut a: Vec<String> = vec!["-p".into()];
    if matches!(spec.mode, CallMode::Warm { .. }) {
        a.extend(["--input-format".into(), "stream-json".into()]);
    }
    a.extend([
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        "--model".into(),
        model_id(spec.tier).into(),
        "--effort".into(),
        effort_str(spec.effort).into(),
        "--system-prompt-file".into(),
        spec.system_prompt_file.to_string_lossy().into_owned(),
        // The built-in tool set: exactly the allowlist's tools ("" disables every tool) ...
        "--tools".into(),
        tool_names(spec.allowed_tools).join(","),
    ]);
    // ... and the same list pre-approved, so nothing waits on a permission prompt.
    if !spec.allowed_tools.is_empty() {
        a.extend(["--allowedTools".into(), spec.allowed_tools.join(",")]);
    }
    a.extend([
        // Anything that would still prompt is denied, never left hanging.
        "--permission-prompts".into(),
        "none".into(),
        // No user, project or local settings files.
        "--setting-sources".into(),
        String::new(),
        "--strict-mcp-config".into(),
    ]);
    match &spec.mode {
        CallMode::OneShot => a.push("--no-session-persistence".into()),
        CallMode::Warm { resume: Some(id) } => a.extend(["--resume".into(), id.clone()]),
        CallMode::Warm { resume: None } => {}
    }
    if spec.safe_mode {
        a.push("--safe-mode".into());
    }
    if let Some(schema) = spec.json_schema {
        a.extend(["--json-schema".into(), schema.to_string()]);
    }
    a
}

/// FNV-1a, 64-bit: a stable content hash for system-prompt file names.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Process-level settings shared by one-shot calls and warm sessions.
#[derive(Debug, Clone)]
pub struct ClaudeCli {
    pub bin: String,
    pub work_dir: PathBuf,
    pub safe_mode: bool,
    /// The cap on live CLI processes (claude, codex, agy), split into a live reserve and a
    /// shared lane. Every spawn site admits through it before starting a process.
    pub admission: Admission,
}

impl ClaudeCli {
    pub fn new(bin: String, work_dir: PathBuf, safe_mode: bool, admission: Admission) -> Self {
        Self {
            bin,
            work_dir,
            safe_mode,
            admission,
        }
    }

    /// Write the system prompt to a content-addressed file and return its path. The bytes
    /// never change for the same prompt, so the provider's prompt cache holds.
    pub fn system_prompt_file(&self, system: &str) -> Result<PathBuf, EngineError> {
        let dir = self.work_dir.join("prompts");
        let path = dir.join(format!("{:016x}.txt", fnv1a64(system.as_bytes())));
        if path.is_file() {
            return Ok(path);
        }
        let io = |e: std::io::Error| EngineError::Io {
            detail: format!("system prompt file: {e}"),
        };
        std::fs::create_dir_all(&dir).map_err(io)?;
        let tmp = dir.join(format!(
            "{:016x}.{}.tmp",
            fnv1a64(system.as_bytes()),
            std::process::id()
        ));
        std::fs::write(&tmp, system.as_bytes()).map_err(io)?;
        if std::fs::rename(&tmp, &path).is_err() {
            // Another writer won the race with identical bytes.
            let _ = std::fs::remove_file(&tmp);
        }
        Ok(path)
    }

    /// A `Command` for this account: plan login only, no console window, isolated cwd.
    pub fn command(&self, account: &Account, argv: &[String]) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.args(argv);
        for k in STRIPPED_ENV {
            cmd.env_remove(k);
        }
        match &account.config_dir {
            Some(dir) => {
                cmd.env("CLAUDE_CONFIG_DIR", dir);
            }
            None => {
                cmd.env_remove("CLAUDE_CONFIG_DIR");
            }
        }
        let _ = std::fs::create_dir_all(&self.work_dir);
        if self.work_dir.is_dir() {
            cmd.current_dir(&self.work_dir);
        }
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }

    pub fn argv_for(&self, req: &EngineRequest, sp: &Path, mode: CallMode) -> Vec<String> {
        claude_argv(&ArgvSpec {
            tier: req.tier,
            effort: req.effort,
            system_prompt_file: sp,
            allowed_tools: &req.allowed_tools,
            json_schema: req.json_schema.as_ref(),
            mode,
            safe_mode: self.safe_mode,
        })
    }

    /// One stateless call: spawn, write the prompt, parse the stream until the result.
    /// `req.timeout_ms` bounds the read. The caller has already admitted the process
    /// ([`Admission::admit`]) and passes the budget left after its wait.
    pub async fn run_once(
        &self,
        account: &Account,
        req: &EngineRequest,
    ) -> Result<TurnOutcome, Failure> {
        let sp = self
            .system_prompt_file(&req.system)
            .map_err(Failure::Fatal)?;
        let argv = self.argv_for(req, &sp, CallMode::OneShot);
        let mut cmd = self.command(account, &argv);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let started = Instant::now();
        let mut child = cmd.spawn().map_err(|e| Failure::Transient {
            detail: format!("spawn {}: {e}", self.bin),
        })?;
        let stderr_task = child
            .stderr
            .take()
            .map(|s| tokio::spawn(read_tail(s, 2048)));
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(req.prompt.as_bytes()).await {
                let _ = child.kill().await;
                return Err(Failure::Transient {
                    detail: format!("write prompt: {e}"),
                });
            }
            drop(stdin);
        }
        let Some(stdout) = child.stdout.take() else {
            return Err(Failure::Transient {
                detail: "no stdout".into(),
            });
        };
        let mut lines = BufReader::new(stdout).lines();
        let mut parser = TurnParser::new();
        let read = async {
            while let Some(line) = lines.next_line().await? {
                if parser.feed(&line, elapsed_ms(started)) == Feed::Done {
                    break;
                }
            }
            Ok::<(), std::io::Error>(())
        };
        match tokio::time::timeout(Duration::from_millis(req.timeout_ms), read).await {
            Err(_) => {
                let _ = child.kill().await;
                return Err(Failure::Fatal(EngineError::Timeout {
                    after_ms: req.timeout_ms,
                }));
            }
            Ok(Err(e)) => {
                let _ = child.kill().await;
                return Err(Failure::Transient {
                    detail: format!("read stdout: {e}"),
                });
            }
            Ok(Ok(())) => {}
        }
        let exit = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            Ok(Ok(st)) => st.code(),
            _ => {
                let _ = child.kill().await;
                None
            }
        };
        let stderr_tail = match stderr_task {
            Some(t) => t.await.unwrap_or_default(),
            None => String::new(),
        };
        parser.finish().map_err(|f| match f {
            Failure::Transient { detail } => Failure::Transient {
                detail: format!(
                    "{detail} (exit {}; stderr: {})",
                    exit.map_or("?".into(), |c| c.to_string()),
                    stderr_tail.trim().chars().take(300).collect::<String>()
                ),
            },
            other => other,
        })
    }
}

pub fn elapsed_ms(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

/// Read a stream to the end, keeping at most the last `cap` bytes.
pub async fn read_tail<R: AsyncRead + Unpin>(mut r: R, cap: usize) -> String {
    let mut keep: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                keep.extend_from_slice(&buf[..n]);
                if keep.len() > cap {
                    let cut = keep.len() - cap;
                    keep.drain(..cut);
                }
            }
        }
    }
    String::from_utf8_lossy(&keep).into_owned()
}

/// Seam for the fleet: the real CLI in production, a double in unit tests.
#[async_trait]
pub trait CliRunner: Send + Sync {
    async fn run(&self, account: &Account, req: &EngineRequest) -> Result<TurnOutcome, Failure>;
}

#[async_trait]
impl CliRunner for ClaudeCli {
    async fn run(&self, account: &Account, req: &EngineRequest) -> Result<TurnOutcome, Failure> {
        self.run_once(account, req).await
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Option lines from `claude --help` on Claude Code 2.1.302 (captured 2026-10-18) for
    /// every flag the driver uses, plus the `--bare` line that names `--system-prompt[-file]`.
    pub const HELP_EXCERPT: &str = r#"
  --allowedTools, --allowed-tools <tools...>
  --bare                                Minimal mode: ... Explicitly provide context via: --system-prompt[-file],
  --effort <level>                      Effort level for the current session (low, medium, high, xhigh, max)
  --include-partial-messages            Include partial message chunks as they arrive (only works with --print and --output-format=stream-json)
  --input-format <format>               Input format (only works with --print): "text" (default), or "stream-json"
  --json-schema <schema>                JSON Schema for structured output validation.
  --model <model>                       Model for the current session.
  --no-session-persistence              Disable session persistence - sessions will not be saved to disk and cannot be resumed (only works with --print)
  --output-format <format>              Output format (only works with --print): "text" (default), "json" (single result), or "stream-json" (realtime streaming)
  --permission-prompts <target>         Who answers permission prompts with --print: "host" (the SDK host or --permission-prompt-tool) or "none"
  -p, --print                           Print response and exit (useful for pipes).
  -r, --resume [value]                  Resume a conversation by session ID, or open interactive picker with optional search term
  --safe-mode                           Start with all customizations (CLAUDE.md, skills, installed plugins, hooks, MCP servers, ...) disabled
  --setting-sources <sources>           Comma-separated list of setting sources to load (user, project, local).
  --strict-mcp-config                   Only use MCP servers from --mcp-config, ignoring all other MCP configurations
  --tools <tools...>                    Specify the list of available tools from the built-in set. Use "" to disable all tools
  --verbose                             Override verbose mode setting from config
"#;

    pub fn flag_in_help(flag: &str) -> bool {
        if flag == "--system-prompt-file" {
            return HELP_EXCERPT.contains("--system-prompt[-file]");
        }
        HELP_EXCERPT.lines().any(|l| {
            l.split(|c: char| c.is_whitespace() || c == ',')
                .any(|tok| tok == flag)
        })
    }

    pub fn assert_flags_in_help(argv: &[String]) {
        for a in argv.iter().filter(|a| a.starts_with('-')) {
            assert!(flag_in_help(a), "flag {a} is not in claude --help");
            assert_ne!(a, "--dangerously-skip-permissions");
        }
    }

    fn spec<'a>(tools: &'a [String], mode: CallMode, p: &'a Path) -> ArgvSpec<'a> {
        ArgvSpec {
            tier: ModelTier::Haiku,
            effort: Effort::Low,
            system_prompt_file: p,
            allowed_tools: tools,
            json_schema: None,
            mode,
            safe_mode: true,
        }
    }

    #[test]
    fn one_shot_argv_matches_live_capture_and_help() {
        let p = PathBuf::from("sp.txt");
        let argv = claude_argv(&spec(&[], CallMode::OneShot, &p));
        assert_eq!(
            argv,
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--model",
                "claude-haiku-5-5",
                "--effort",
                "low",
                "--system-prompt-file",
                "sp.txt",
                "--tools",
                "",
                "--permission-prompts",
                "none",
                "--setting-sources",
                "",
                "--strict-mcp-config",
                "--no-session-persistence",
                "--safe-mode"
            ]
        );
        assert_flags_in_help(&argv);
    }

    #[test]
    fn warm_argv_with_tools_schema_and_resume() {
        let p = PathBuf::from("sp.txt");
        let tools = vec!["Read".to_string(), "Grep".to_string()];
        let schema = serde_json::json!({"type":"object"});
        let mut s = spec(
            &tools,
            CallMode::Warm {
                resume: Some("abc".into()),
            },
            &p,
        );
        s.json_schema = Some(&schema);
        let argv = claude_argv(&s);
        assert_flags_in_help(&argv);
        let joined = argv.join(" ");
        assert!(joined.contains("--input-format stream-json"));
        assert!(joined.contains("--tools Read,Grep --allowedTools Read,Grep"));
        assert!(joined.contains("--resume abc"));
        assert!(joined.contains(r#"--json-schema {"type":"object"}"#));
        assert!(!joined.contains("--no-session-persistence"));
    }

    #[test]
    fn scoped_tool_rules_enable_the_tool_and_approve_only_their_scope() {
        let p = PathBuf::from("sp.txt");
        let tools = vec![
            "WebFetch(domain:sec.gov)".to_string(),
            "WebFetch(domain:www.sec.gov)".to_string(),
        ];
        let argv = claude_argv(&spec(&tools, CallMode::OneShot, &p));
        assert_flags_in_help(&argv);
        let joined = argv.join(" ");
        assert!(
            joined.contains(
                "--tools WebFetch --allowedTools WebFetch(domain:sec.gov),WebFetch(domain:www.sec.gov)"
            ),
            "{joined}"
        );
        assert_eq!(
            tool_names(&["Read".into(), "Read".into(), "Grep(x)".into()]),
            ["Read", "Grep"]
        );
    }

    #[test]
    fn system_prompt_file_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let cli = ClaudeCli::new(
            "claude".into(),
            dir.path().to_path_buf(),
            true,
            Admission::new(2, 0),
        );
        let a = cli.system_prompt_file("I am Malakor.").unwrap();
        let b = cli.system_prompt_file("I am Malakor.").unwrap();
        let c = cli.system_prompt_file("I am someone else.").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "I am Malakor.");
    }
}
