//! Per-terminal observability: working directory, git, foreground process and
//! AI agent state.
//!
//! Every shell spawned by sshx receives three environment variables:
//!
//! - [`ENV_SESSION`]: the sshx session name,
//! - [`ENV_TERMINAL_ID`]: the terminal (shell) ID inside that session,
//! - [`ENV_HOOK_SOCKET`]: a private Unix socket owned by the sshx process.
//!
//! Agent hooks (Claude Code `hooks` in `~/.claude/settings.json`, installed by
//! `sshx claude-hooks install`) run `sshx hook`, which inherits these variables
//! from the terminal and forwards the hook payload to the socket. The running
//! sshx process then knows precisely *which terminal* an agent event belongs
//! to, which the transcript-based tracker in `workspace.rs` cannot tell.
//!
//! The [`TerminalRegistry`] aggregates this per terminal and notifies
//! subscribers (the platform reporter) on every change.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;
use tracing::{debug, warn};

/// Environment variable holding the sshx session name.
pub const ENV_SESSION: &str = "SSHX_SESSION";
/// Environment variable holding the terminal ID inside the session.
pub const ENV_TERMINAL_ID: &str = "SSHX_TERMINAL_ID";
/// Environment variable holding the path of the hook socket.
pub const ENV_HOOK_SOCKET: &str = "SSHX_HOOK_SOCKET";

/// Maximum size of a hook payload accepted on the socket.
const MAX_HOOK_PAYLOAD: usize = 1 << 20;

/// How often working directories, process names and git info are refreshed.
const PROC_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// A "working" agent with no hook event for this long gets its transcript
/// checked for an interruption (Claude Code fires no hook on Esc / Ctrl-C).
const INTERRUPT_CHECK_AFTER: Duration = Duration::from_secs(15);

/// An agent is considered gone when the shell is back in the foreground and no
/// hook event arrived for this long (it exited without a `SessionEnd` hook).
const AGENT_GONE_AFTER: Duration = Duration::from_secs(10);

/// Process names considered to be an interactive shell at its prompt.
const SHELL_NAMES: &[&str] = &["bash", "zsh", "fish", "sh", "dash", "ksh", "tcsh", "csh", "nu", "login"];

/// Current time as UNIX milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Coarse state of an AI agent running in a terminal.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// No agent detected in this terminal.
    #[default]
    None,
    /// Agent started, waiting for a first prompt.
    Idle,
    /// Agent is working on a prompt.
    Working,
    /// Agent asks the user a question (AskUserQuestion, plan approval…).
    Question,
    /// Agent waits for a permission approval.
    Permission,
    /// Agent finished its turn and waits for the user.
    Done,
}

impl AgentState {
    /// Wire name of the state, shared with the platform.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Question => "question",
            Self::Permission => "permission",
            Self::Done => "done",
        }
    }
}

/// Agent information for one terminal.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Agent kind, e.g. `"claude"`.
    pub kind: String,
    /// Current state.
    pub state: AgentState,
    /// UNIX ms when the current state started.
    pub since: u64,
    /// Tool currently running or awaiting permission.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Short human-readable detail: question text, notification, last answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Agent's own session ID (Claude Code session UUID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Working directory reported by the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Transcript file, used to detect interruptions.
    #[serde(skip)]
    pub transcript: Option<PathBuf>,
    /// UNIX ms of the last hook event.
    #[serde(skip)]
    pub last_event: u64,
}

/// Git repository information for a working directory.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    /// Repository root directory.
    pub root: String,
    /// Repository name: from the `origin` remote if any, else the root name.
    pub repo: String,
    /// Current branch, or short commit hash when detached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// URL of the `origin` remote.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

/// Everything known about one terminal.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    /// Shell process ID.
    #[serde(skip)]
    pub pid: u32,
    /// Foreground process group of the TTY.
    #[serde(skip)]
    pub fg_pgid: Option<u32>,
    /// UNIX ms when the terminal was created.
    pub started_at: u64,
    /// Working directory of the foreground process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Name of the foreground process (`zsh`, `claude`, `vim`…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    /// Git information for `cwd`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    /// Project name used for grouping: git repo name, else folder name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// AI agent running in this terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
    /// Consecutive polls where the foreground process was a plain shell.
    #[serde(skip)]
    shell_polls: u8,
}

impl TerminalInfo {
    /// Coarse agent state, `None` when no agent is present.
    pub fn agent_state(&self) -> AgentState {
        self.agent.as_ref().map(|a| a.state).unwrap_or_default()
    }

    fn refresh_project(&mut self) {
        let from_agent = self.agent.as_ref().and_then(|a| a.cwd.clone());
        self.project = match &self.git {
            Some(git) => Some(git.repo.clone()),
            None => from_agent.or_else(|| self.cwd.clone()).and_then(|cwd| {
                Path::new(&cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            }),
        };
    }
}

/// Registry of the live terminals of this sshx process.
pub struct TerminalRegistry {
    inner: Mutex<BTreeMap<u32, TerminalInfo>>,
    version: watch::Sender<u64>,
}

impl TerminalRegistry {
    /// Create an empty registry.
    pub fn new() -> Arc<Self> {
        let (version, _) = watch::channel(0);
        Arc::new(Self {
            inner: Mutex::new(BTreeMap::new()),
            version,
        })
    }

    /// Subscribe to change notifications.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.version.subscribe()
    }

    fn bump(&self) {
        self.version.send_modify(|v| *v += 1);
    }

    fn with<R>(&self, f: impl FnOnce(&mut BTreeMap<u32, TerminalInfo>) -> R) -> R {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    /// Record a newly spawned shell.
    pub fn register(&self, sid: u32, pid: u32) {
        self.with(|map| {
            map.insert(
                sid,
                TerminalInfo {
                    pid,
                    started_at: now_ms(),
                    ..Default::default()
                },
            );
        });
        self.bump();
    }

    /// Forget a closed shell.
    pub fn unregister(&self, sid: u32) {
        if self.with(|map| map.remove(&sid)).is_some() {
            self.bump();
        }
    }

    /// Update the foreground process group of a terminal.
    pub fn set_foreground(&self, sid: u32, pgid: Option<u32>) {
        // Not a visible change by itself: the process poller resolves names.
        self.with(|map| {
            if let Some(t) = map.get_mut(&sid) {
                t.fg_pgid = pgid;
            }
        });
    }

    /// Snapshot of all terminals, ordered by ID.
    pub fn snapshot(&self) -> Vec<(u32, TerminalInfo)> {
        self.with(|map| map.iter().map(|(k, v)| (*k, v.clone())).collect())
    }

    /// Apply an agent hook event to a terminal. Returns true on change.
    ///
    /// `sent_at` is the UNIX ms at which `sshx hook` started: events older than
    /// the last applied one are dropped, so a late process cannot roll the
    /// state back.
    pub fn apply_hook(&self, sid: u32, kind: &str, payload: &Value, sent_at: u64) -> bool {
        let changed = self.with(|map| {
            let Some(term) = map.get_mut(&sid) else {
                return false;
            };
            if term.agent.as_ref().is_some_and(|a| a.last_event > sent_at) {
                debug!(sid, "out-of-order hook event dropped");
                return false;
            }
            let next = next_agent_info(term.agent.as_ref(), kind, payload, sent_at);
            term.shell_polls = 0;
            if next != term.agent {
                term.agent = next;
                term.refresh_project();
                true
            } else {
                false
            }
        });
        if changed {
            self.bump();
        }
        changed
    }
}

// ---------------------------------------------------------------------------
// Agent state machine
// ---------------------------------------------------------------------------

fn str_field(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Text of the first question of an `AskUserQuestion` tool call.
fn ask_user_question_text(tool_input: Option<&Value>) -> Option<String> {
    let input = tool_input?;
    if let Some(q) = input
        .get("questions")
        .and_then(|q| q.as_array())
        .and_then(|a| a.first())
        .and_then(|q| q.get("question"))
        .and_then(|q| q.as_str())
    {
        return Some(q.to_string());
    }
    input.get("question").and_then(|q| q.as_str()).map(String::from)
}

/// Compute the next agent info for a terminal from a hook event.
///
/// `payload` is the raw JSON given by the agent on the hook's stdin; for
/// Claude Code it contains `hook_event_name`, `session_id`, `cwd`,
/// `transcript_path` and event-specific fields.
pub fn next_agent_info(
    prev: Option<&AgentInfo>,
    kind: &str,
    payload: &Value,
    now: u64,
) -> Option<AgentInfo> {
    let event = str_field(payload, "hook_event_name").unwrap_or_default();
    let mut info = prev.cloned().unwrap_or_else(|| AgentInfo {
        kind: kind.to_string(),
        since: now,
        ..Default::default()
    });
    info.kind = kind.to_string();
    info.last_event = now;
    if let Some(id) = str_field(payload, "session_id") {
        info.session_id = Some(id);
    }
    if let Some(cwd) = str_field(payload, "cwd") {
        info.cwd = Some(cwd);
    }
    if let Some(path) = str_field(payload, "transcript_path") {
        info.transcript = Some(PathBuf::from(path));
    }

    let tool_name = str_field(payload, "tool_name");
    let (state, tool, message): (AgentState, Option<String>, Option<String>) = match event.as_str() {
        "SessionStart" => (AgentState::Idle, None, None),
        "UserPromptSubmit" => {
            let prompt = str_field(payload, "prompt").map(|p| truncate(&p, 300));
            (AgentState::Working, None, prompt)
        }
        "PreToolUse" => match tool_name.as_deref() {
            Some("AskUserQuestion") => (
                AgentState::Question,
                tool_name.clone(),
                ask_user_question_text(payload.get("tool_input")).map(|q| truncate(&q, 500)),
            ),
            Some("ExitPlanMode") => (
                AgentState::Question,
                tool_name.clone(),
                Some("Plan prêt, en attente de validation".into()),
            ),
            _ => (AgentState::Working, tool_name.clone(), info.message.clone()),
        },
        "PermissionRequest" => (
            AgentState::Permission,
            tool_name.clone(),
            tool_name.as_ref().map(|t| format!("Autoriser {t} ?")),
        ),
        "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" | "PreCompact" | "SubagentStop" => {
            // Back to work once a question or permission has been answered.
            (AgentState::Working, None, info.message.clone())
        }
        "Notification" => {
            let message = str_field(payload, "message");
            let ntype = str_field(payload, "notification_type").unwrap_or_default();
            let lower = message.clone().unwrap_or_default().to_lowercase();
            if ntype == "permission_prompt" || lower.contains("permission") {
                (AgentState::Permission, info.tool.clone(), message)
            } else if ntype == "elicitation_dialog" || ntype == "agent_needs_input" {
                (AgentState::Question, info.tool.clone(), message)
            } else if ntype == "idle_prompt" || lower.contains("waiting for your input") {
                // Reminder that the agent waits: keep a finished turn as is.
                match info.state {
                    AgentState::Done | AgentState::Idle => (info.state, None, info.message.clone()),
                    _ => (AgentState::Done, None, message),
                }
            } else {
                (info.state, info.tool.clone(), info.message.clone())
            }
        }
        "Stop" => {
            let last = str_field(payload, "last_assistant_message")
                .or_else(|| str_field(payload, "_sshx_last_message"))
                .map(|m| truncate(&m, 500));
            (AgentState::Done, None, last)
        }
        "StopFailure" => (
            AgentState::Done,
            None,
            Some(str_field(payload, "error").unwrap_or_else(|| "Arrêt sur erreur".into())),
        ),
        "SessionEnd" => return None,
        _ => (info.state, info.tool.clone(), info.message.clone()),
    };

    if state != info.state {
        info.since = now;
    }
    info.state = state;
    info.tool = tool;
    info.message = message;
    Some(info)
}

// ---------------------------------------------------------------------------
// Transcript helpers
// ---------------------------------------------------------------------------

/// Read the last lines of a JSONL transcript (at most `max_bytes` from the end).
fn read_tail_lines(path: &Path, max_bytes: u64) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // first line is probably partial
    }
    lines
}

fn message_text(entry: &Value) -> Option<String> {
    let content = entry.get("message")?.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let texts: Vec<&str> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

/// Last assistant text message of a Claude Code transcript.
pub fn last_assistant_message(path: &Path) -> Option<String> {
    read_tail_lines(path, 256 * 1024)
        .iter()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("assistant"))
        .find_map(|e| message_text(&e))
}

/// Whether the last user entry of a transcript is an interruption marker.
fn transcript_interrupted(path: &Path) -> bool {
    for line in read_tail_lines(path, 64 * 1024).iter().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match entry.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                return message_text(&entry)
                    .map(|t| t.contains("[Request interrupted by user"))
                    .unwrap_or(false);
            }
            Some("assistant") => return false,
            _ => continue,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Process / directory / git polling
// ---------------------------------------------------------------------------

/// Git information for a directory, found by walking up to a `.git` entry.
pub fn git_info(dir: &Path) -> Option<GitInfo> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        let dotgit = d.join(".git");
        if dotgit.exists() {
            let git_dir = if dotgit.is_file() {
                // Worktree or submodule: "gitdir: <path>".
                let text = std::fs::read_to_string(&dotgit).ok()?;
                let rel = text.trim().strip_prefix("gitdir:")?.trim().to_string();
                let p = PathBuf::from(rel);
                if p.is_absolute() { p } else { d.join(p) }
            } else {
                dotgit
            };
            let branch = std::fs::read_to_string(git_dir.join("HEAD")).ok().map(|head| {
                let head = head.trim();
                match head.strip_prefix("ref: refs/heads/") {
                    Some(b) => b.to_string(),
                    None => head.chars().take(8).collect(),
                }
            });
            // Worktrees keep the config in the common dir.
            let common = std::fs::read_to_string(git_dir.join("commondir"))
                .ok()
                .map(|c| git_dir.join(c.trim()))
                .unwrap_or_else(|| git_dir.clone());
            let remote = std::fs::read_to_string(common.join("config"))
                .ok()
                .and_then(|cfg| origin_url(&cfg));
            let root_name = d
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "/".into());
            let repo = remote
                .as_deref()
                .and_then(repo_name_from_url)
                .unwrap_or(root_name);
            return Some(GitInfo {
                root: d.to_string_lossy().into_owned(),
                repo,
                branch,
                remote,
            });
        }
        cur = d.parent();
    }
    None
}

/// Extract the `origin` remote URL from a git config file.
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line == "[remote \"origin\"]";
        } else if in_origin {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == "url" {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// `git@github.com:org/name.git` or `https://host/org/name` → `name`.
fn repo_name_from_url(url: &str) -> Option<String> {
    let last = url.trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_string())
}

/// Resolve process names for a set of PIDs with a single `ps` call.
async fn process_names(pids: &[u32]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    if pids.is_empty() {
        return out;
    }
    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let Ok(output) = tokio::process::Command::new("ps")
        .args(["-o", "pid=,comm=", "-p", &list])
        .output()
        .await
    else {
        return out;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        let Some((pid, comm)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.parse() else { continue };
        let comm = comm.trim();
        let name = comm.rsplit('/').next().unwrap_or(comm).trim_start_matches('-');
        out.insert(pid, name.to_string());
    }
    out
}

/// Resolve working directories for a set of PIDs.
async fn process_cwds(pids: &[u32]) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    if pids.is_empty() {
        return out;
    }
    if cfg!(target_os = "linux") {
        for pid in pids {
            if let Ok(p) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
                out.insert(*pid, p.to_string_lossy().into_owned());
            }
        }
        return out;
    }
    // macOS and other BSDs: one lsof call for all PIDs.
    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let Ok(output) = tokio::process::Command::new("lsof")
        .args(["-a", "-d", "cwd", "-Fpn", "-p", &list])
        .output()
        .await
    else {
        return out;
    };
    let mut current = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current = pid.parse::<u32>().ok();
        } else if let (Some(path), Some(pid)) = (line.strip_prefix('n'), current) {
            out.insert(pid, path.to_string());
        }
    }
    out
}

/// Spawn the background task refreshing cwd, process and git information.
pub fn spawn_proc_poller(registry: Arc<TerminalRegistry>) {
    tokio::spawn(async move {
        let mut git_cache: HashMap<String, (Option<GitInfo>, std::time::Instant)> = HashMap::new();
        loop {
            tokio::time::sleep(PROC_POLL_INTERVAL).await;
            poll_once(&registry, &mut git_cache).await;
        }
    });
}

async fn poll_once(
    registry: &TerminalRegistry,
    git_cache: &mut HashMap<String, (Option<GitInfo>, std::time::Instant)>,
) {
    let snapshot = registry.snapshot();
    if snapshot.is_empty() {
        return;
    }
    // The foreground process is the most relevant one (e.g. `claude`).
    let targets: Vec<(u32, u32)> = snapshot
        .iter()
        .map(|(sid, t)| (*sid, t.fg_pgid.unwrap_or(t.pid)))
        .filter(|(_, pid)| *pid != 0)
        .collect();
    let mut pids: Vec<u32> = targets.iter().map(|(_, p)| *p).collect();
    pids.extend(snapshot.iter().map(|(_, t)| t.pid).filter(|p| *p != 0));
    pids.sort_unstable();
    pids.dedup();
    let names = process_names(&pids).await;
    let cwds = process_cwds(&pids).await;

    let mut updates = Vec::new();
    for (sid, target) in targets {
        let shell_pid = snapshot.iter().find(|(s, _)| *s == sid).map(|(_, t)| t.pid);
        let cwd = cwds
            .get(&target)
            .or_else(|| shell_pid.and_then(|p| cwds.get(&p)))
            .cloned();
        let git = match &cwd {
            Some(cwd) => {
                let fresh = git_cache
                    .get(cwd)
                    .filter(|(_, at)| at.elapsed() < Duration::from_secs(10))
                    .map(|(g, _)| g.clone());
                match fresh {
                    Some(g) => g,
                    None => {
                        let g = git_info(Path::new(cwd));
                        git_cache.insert(cwd.clone(), (g.clone(), std::time::Instant::now()));
                        g
                    }
                }
            }
            None => None,
        };
        updates.push((sid, cwd, names.get(&target).cloned(), git));
    }

    let now = now_ms();
    let mut interrupted_checks = Vec::new();
    let changed = registry.with(|map| {
        let mut changed = false;
        for (sid, cwd, process, git) in updates {
            let Some(t) = map.get_mut(&sid) else { continue };
            let before = t.clone();
            if cwd.is_some() {
                t.cwd = cwd;
            }
            if process.is_some() {
                t.process = process;
            }
            t.git = git;
            // The shell is at its prompt when it leads the foreground process
            // group itself; fall back to the process name if unknown.
            let at_prompt = match t.fg_pgid {
                Some(pgid) => pgid == t.pid,
                None => t
                    .process
                    .as_deref()
                    .map(|p| SHELL_NAMES.contains(&p))
                    .unwrap_or(false),
            };
            t.shell_polls = if at_prompt { t.shell_polls.saturating_add(1) } else { 0 };
            // The agent exited without a SessionEnd hook (e.g. killed).
            let quiet = t
                .agent
                .as_ref()
                .is_some_and(|a| now.saturating_sub(a.last_event) > AGENT_GONE_AFTER.as_millis() as u64);
            if quiet && t.shell_polls >= 2 {
                t.agent = None;
            }
            if let Some(agent) = &t.agent {
                if agent.state == AgentState::Working
                    && now.saturating_sub(agent.last_event) > INTERRUPT_CHECK_AFTER.as_millis() as u64
                {
                    if let Some(path) = &agent.transcript {
                        interrupted_checks.push((sid, path.clone()));
                    }
                }
            }
            t.refresh_project();
            // Ignore the internal poll counter when comparing.
            let mut cmp = t.clone();
            cmp.shell_polls = before.shell_polls;
            if cmp != before {
                changed = true;
            }
        }
        changed
    });

    let mut interrupted = false;
    for (sid, path) in interrupted_checks {
        if transcript_interrupted(&path) {
            interrupted |= registry.with(|map| {
                if let Some(agent) = map.get_mut(&sid).and_then(|t| t.agent.as_mut()) {
                    if agent.state == AgentState::Working {
                        agent.state = AgentState::Idle;
                        agent.since = now;
                        agent.tool = None;
                        agent.message = Some("Interrompu".into());
                        return true;
                    }
                }
                false
            });
        }
    }

    if changed || interrupted {
        registry.bump();
    }
}

// ---------------------------------------------------------------------------
// Hook socket (Unix only)
// ---------------------------------------------------------------------------

/// Message sent by `sshx hook` to the running sshx process.
#[derive(Serialize, Deserialize, Debug)]
pub struct HookMessage {
    /// Agent kind, e.g. "claude".
    pub agent: String,
    /// Session name from the terminal environment.
    pub session: String,
    /// Terminal ID from the terminal environment.
    pub terminal: u32,
    /// UNIX ms when `sshx hook` started, used to order events.
    #[serde(default)]
    pub sent_at: u64,
    /// Raw hook payload.
    pub payload: Value,
}

/// Directory holding hook sockets, private to the current user.
#[cfg(unix)]
fn socket_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let uid = nix::unistd::getuid().as_raw();
    let dir = base.join(format!("sshx-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = std::fs::symlink_metadata(&dir)?;
    if !meta.is_dir() || meta.uid() != uid {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not a directory owned by the current user", dir.display()),
        ));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

/// Guard removing the hook socket file when dropped.
pub struct HookSocketGuard {
    path: PathBuf,
}

impl HookSocketGuard {
    /// Path of the socket.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for HookSocketGuard {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// Start listening for hook events of `session` on a private Unix socket.
#[cfg(unix)]
pub fn spawn_hook_server(
    registry: Arc<TerminalRegistry>,
    session: String,
) -> anyhow::Result<HookSocketGuard> {
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixListener;

    let path = socket_dir()?.join(format!("hook-{}.sock", std::process::id()));
    std::fs::remove_file(&path).ok();
    let listener = UnixListener::bind(&path)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    debug!(path = %path.display(), "hook socket listening");

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let registry = registry.clone();
            let session = session.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let read = tokio::time::timeout(
                    Duration::from_secs(2),
                    (&mut stream).take(MAX_HOOK_PAYLOAD as u64).read_to_end(&mut buf),
                )
                .await;
                if !matches!(read, Ok(Ok(_))) {
                    return;
                }
                let msg: HookMessage = match serde_json::from_slice(&buf) {
                    Ok(m) => m,
                    Err(err) => {
                        warn!(%err, "invalid hook message");
                        return;
                    }
                };
                if msg.session != session {
                    debug!(session = %msg.session, "hook for another session, ignored");
                    return;
                }
                let mut payload = msg.payload;
                // Enrich Stop events with the final answer, read locally.
                let is_stop = payload.get("hook_event_name").and_then(|v| v.as_str()) == Some("Stop");
                if is_stop && payload.get("last_assistant_message").is_none() {
                    if let Some(path) = payload.get("transcript_path").and_then(|v| v.as_str()) {
                        let path = PathBuf::from(path);
                        let last = tokio::task::spawn_blocking(move || last_assistant_message(&path))
                            .await
                            .ok()
                            .flatten();
                        if let (Some(last), Some(obj)) = (last, payload.as_object_mut()) {
                            obj.insert("_sshx_last_message".into(), Value::String(last));
                        }
                    }
                }
                let sent_at = if msg.sent_at == 0 { now_ms() } else { msg.sent_at };
                registry.apply_hook(msg.terminal, &msg.agent, &payload, sent_at);
            });
        }
    });
    Ok(HookSocketGuard { path })
}

/// Entry point of `sshx hook`: forward a hook payload from stdin to the sshx
/// process owning this terminal. Never fails and never prints, so that it
/// cannot disturb the agent; does nothing outside an sshx terminal.
#[cfg(unix)]
pub fn forward_hook(agent: &str) {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let sent_at = now_ms();
    let (Some(socket), Some(session), Some(terminal)) = (
        std::env::var_os(ENV_HOOK_SOCKET),
        std::env::var(ENV_SESSION).ok(),
        std::env::var(ENV_TERMINAL_ID).ok().and_then(|t| t.parse::<u32>().ok()),
    ) else {
        return;
    };
    let mut input = Vec::new();
    if std::io::stdin()
        .take(MAX_HOOK_PAYLOAD as u64)
        .read_to_end(&mut input)
        .is_err()
    {
        return;
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&input) else {
        return;
    };
    let msg = HookMessage {
        agent: agent.to_string(),
        session,
        terminal,
        sent_at,
        payload,
    };
    let Ok(bytes) = serde_json::to_vec(&msg) else {
        return;
    };
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return;
    };
    stream.set_write_timeout(Some(Duration::from_millis(500))).ok();
    stream.write_all(&bytes).ok();
}

#[cfg(not(unix))]
/// Hooks are not supported on this platform.
pub fn forward_hook(_agent: &str) {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn step(prev: Option<&AgentInfo>, payload: Value) -> Option<AgentInfo> {
        next_agent_info(prev, "claude", &payload, 1000)
    }

    #[test]
    fn claude_lifecycle() {
        let a = step(None, json!({"hook_event_name": "SessionStart", "session_id": "s1", "cwd": "/w"})).unwrap();
        assert_eq!(a.state, AgentState::Idle);
        assert_eq!(a.session_id.as_deref(), Some("s1"));

        let a = step(Some(&a), json!({"hook_event_name": "UserPromptSubmit", "prompt": "traduis"})).unwrap();
        assert_eq!(a.state, AgentState::Working);
        assert_eq!(a.message.as_deref(), Some("traduis"));

        let a = step(Some(&a), json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"})).unwrap();
        assert_eq!(a.state, AgentState::Working);
        assert_eq!(a.tool.as_deref(), Some("Bash"));

        let a = step(Some(&a), json!({"hook_event_name": "Notification", "notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"})).unwrap();
        assert_eq!(a.state, AgentState::Permission);
        assert_eq!(a.tool.as_deref(), Some("Bash"));

        let a = step(Some(&a), json!({"hook_event_name": "PostToolUse", "tool_name": "Bash"})).unwrap();
        assert_eq!(a.state, AgentState::Working);

        let a = step(Some(&a), json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": "Quelle langue ?"}]}})).unwrap();
        assert_eq!(a.state, AgentState::Question);
        assert_eq!(a.message.as_deref(), Some("Quelle langue ?"));

        let a = step(Some(&a), json!({"hook_event_name": "Stop", "_sshx_last_message": "Fini."})).unwrap();
        assert_eq!(a.state, AgentState::Done);
        assert_eq!(a.message.as_deref(), Some("Fini."));

        // The idle reminder keeps the finished turn.
        let b = step(Some(&a), json!({"hook_event_name": "Notification", "notification_type": "idle_prompt", "message": "Claude is waiting for your input"})).unwrap();
        assert_eq!(b.state, AgentState::Done);
        assert_eq!(b.message.as_deref(), Some("Fini."));

        assert!(step(Some(&a), json!({"hook_event_name": "SessionEnd"})).is_none());
    }

    #[test]
    fn repo_names() {
        assert_eq!(repo_name_from_url("git@github.com:catie-aq/sshx.git").as_deref(), Some("sshx"));
        assert_eq!(repo_name_from_url("https://github.com/ekzhang/sshx").as_deref(), Some("sshx"));
        assert_eq!(repo_name_from_url("https://host/a/b/").as_deref(), Some("b"));
        let cfg = "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:catie-aq/sshx.git\n";
        assert_eq!(origin_url(cfg).as_deref(), Some("git@github.com:catie-aq/sshx.git"));
    }

    #[test]
    fn git_info_of_this_repo() {
        let here = std::env::current_dir().unwrap();
        let info = git_info(&here).expect("tests run inside a git checkout");
        assert!(!info.repo.is_empty());
    }
}
