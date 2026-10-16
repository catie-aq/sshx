//! Install Claude Code hooks that report agent state to sshx.
//!
//! The hooks run `sshx hook claude`, which is a silent no-op outside an sshx
//! terminal (see `crate::agent::forward_hook`), so installing them globally in
//! `~/.claude/settings.json` is harmless for other terminals.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

/// Hook events sshx listens to. Hooks are synchronous (not `async`) so that
/// events reach sshx in order, e.g. `PreToolUse` before `PermissionRequest`.
pub const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

/// Timeout given to each hook, in seconds. `sshx hook` takes milliseconds.
const HOOK_TIMEOUT_SECS: u64 = 5;

/// Marker identifying hook commands installed by sshx.
const MARKER: &str = " hook claude";

/// Path of the user-level Claude Code settings file.
pub fn settings_path() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(dir).join("settings.json"));
    }
    let home = dirs::home_dir().context("cannot find the home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

/// Command line run by the hooks: this executable, by absolute path.
pub fn hook_command() -> Result<String> {
    let exe = std::env::current_exe().context("cannot locate the sshx executable")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let exe = exe.to_string_lossy();
    // Quote the path for the hook shell if needed.
    let exe = if exe.contains(char::is_whitespace) {
        format!("'{}'", exe.replace('\'', r"'\''"))
    } else {
        exe.into_owned()
    };
    Ok(format!("{exe}{MARKER}"))
}

fn is_sshx_command(cmd: &str) -> bool {
    cmd.ends_with(MARKER) && cmd.contains("sshx")
}

fn load(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("{} is not valid JSON", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn save(path: &Path, value: &Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        let backup = path.with_extension("json.sshx-backup");
        std::fs::copy(path, &backup)
            .with_context(|| format!("cannot back up settings to {}", backup.display()))?;
    }
    let tmp = path.with_extension("json.sshx-tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)? + "\n")?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Remove sshx hook handlers from a settings value. Returns how many were removed.
fn strip(settings: &mut Value) -> usize {
    let mut removed = 0;
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return 0;
    };
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else { continue };
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                let before = handlers.len();
                handlers.retain(|h| {
                    !h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(is_sshx_command)
                });
                removed += before - handlers.len();
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(|h| h.as_array())
                .map(|h| !h.is_empty())
                .unwrap_or(true)
        });
    }
    hooks.retain(|_, groups| groups.as_array().map(|g| !g.is_empty()).unwrap_or(true));
    removed
}

/// Add sshx hooks to a settings value (replacing previous sshx hooks).
fn add(settings: &mut Value, command: &str) -> Result<()> {
    strip(settings);
    let root = settings
        .as_object_mut()
        .context("settings.json must contain a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("\"hooks\" must be a JSON object")?;
    for event in HOOK_EVENTS {
        let groups = hooks
            .entry(*event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .with_context(|| format!("hooks.{event} must be an array"))?;
        groups.push(json!({
            "hooks": [{ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS }]
        }));
    }
    Ok(())
}

/// Count installed sshx handlers per event.
fn installed_events(settings: &Value) -> Vec<&'static str> {
    HOOK_EVENTS
        .iter()
        .copied()
        .filter(|event| {
            settings
                .get("hooks")
                .and_then(|h| h.get(*event))
                .and_then(|g| g.as_array())
                .is_some_and(|groups| {
                    groups.iter().any(|g| {
                        g.get("hooks").and_then(|h| h.as_array()).is_some_and(|hs| {
                            hs.iter().any(|h| {
                                h.get("command")
                                    .and_then(|c| c.as_str())
                                    .is_some_and(is_sshx_command)
                            })
                        })
                    })
                })
        })
        .collect()
}

/// Install (or refresh) the sshx hooks.
pub fn install() -> Result<PathBuf> {
    let path = settings_path()?;
    let mut settings = load(&path)?;
    add(&mut settings, &hook_command()?)?;
    save(&path, &settings)?;
    Ok(path)
}

/// Remove the sshx hooks. Returns the number of removed handlers.
pub fn uninstall() -> Result<(PathBuf, usize)> {
    let path = settings_path()?;
    let mut settings = load(&path)?;
    let removed = strip(&mut settings);
    if removed > 0 {
        save(&path, &settings)?;
    }
    Ok((path, removed))
}

/// Whether all sshx hooks are installed.
pub fn status() -> Result<(PathBuf, Vec<&'static str>)> {
    let path = settings_path()?;
    let settings = load(&path)?;
    Ok((path, installed_events(&settings)))
}

/// Distinct sshx hook commands found in the settings.
fn installed_commands(settings: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(hooks) = settings.get("hooks").and_then(|h| h.as_object()) else {
        return out;
    };
    for groups in hooks.values().filter_map(|g| g.as_array()) {
        for handlers in groups.iter().filter_map(|g| g.get("hooks").and_then(|h| h.as_array())) {
            for cmd in handlers.iter().filter_map(|h| h.get("command").and_then(|c| c.as_str())) {
                if is_sshx_command(cmd) && !out.iter().any(|c| c == cmd) {
                    out.push(cmd.to_string());
                }
            }
        }
    }
    out
}

/// Executable path of an sshx hook command (inverse of [`hook_command`]).
pub fn command_executable(cmd: &str) -> PathBuf {
    let exe = cmd.strip_suffix(MARKER).unwrap_or(cmd).trim();
    let exe = match exe.strip_prefix('\'').and_then(|e| e.strip_suffix('\'')) {
        Some(quoted) => quoted.replace(r"'\''", "'"),
        None => exe.to_string(),
    };
    PathBuf::from(exe)
}

/// Detailed state of the installed hooks, for `sshx platform status`.
pub struct HooksReport {
    /// Settings file.
    pub path: PathBuf,
    /// Events with an sshx handler.
    pub events: Vec<&'static str>,
    /// Distinct sshx commands configured.
    pub commands: Vec<String>,
}

/// Inspect the installed hooks.
pub fn report() -> Result<HooksReport> {
    let path = settings_path()?;
    let settings = load(&path)?;
    Ok(HooksReport {
        events: installed_events(&settings),
        commands: installed_commands(&settings),
        path,
    })
}

/// True if every hook event is installed.
pub fn is_installed() -> bool {
    status()
        .map(|(_, events)| events.len() == HOOK_EVENTS.len())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent_and_preserves_user_hooks() {
        let mut settings = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
            }
        });
        let cmd = "/usr/local/bin/sshx hook claude";
        add(&mut settings, cmd).unwrap();
        add(&mut settings, cmd).unwrap();
        assert_eq!(installed_events(&settings).len(), HOOK_EVENTS.len());
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "user hook kept, sshx hook added once");
        // Key order is preserved (serde_json preserve_order).
        assert_eq!(settings.as_object().unwrap().keys().next().unwrap(), "model");

        assert_eq!(installed_commands(&settings), vec![cmd.to_string()]);
        assert_eq!(command_executable(cmd), PathBuf::from("/usr/local/bin/sshx"));
        assert_eq!(
            command_executable("'/Applications/My Tools/sshx' hook claude"),
            PathBuf::from("/Applications/My Tools/sshx")
        );

        assert_eq!(strip(&mut settings), HOOK_EVENTS.len());
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(settings["hooks"].get("PreToolUse").is_none());
    }
}
