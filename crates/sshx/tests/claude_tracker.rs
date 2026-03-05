//! Integration test for the Claude activity tracker.
//!
//! Simulates a fake project with a README.md and a Claude session
//! that reads it, verifying event detection, PID lifecycle, and reconnect.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;

use sshx::workspace::{tail_transcript, track_pid_lifecycle};
use sshx_core::proto::client_update::ClientMessage;

fn fake_tool_use_line(tool: &str, path: &str, session: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "say",
        "isMeta": false,
        "message": {
            "role": "assistant",
            "content": [{"type": "tool_use", "name": tool, "input": {"file_path": path}}],
            "usage": {"input_tokens": 120, "output_tokens": 18}
        },
        "sessionId": session,
        "timestamp": ts
    })
    .to_string()
}

fn fake_tool_result_line(content: &str, session: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "tool_result",
        "isMeta": false,
        "message": {
            "role": "user",
            "content": [{"type": "tool_result", "content": content}]
        },
        "sessionId": session,
        "timestamp": ts
    })
    .to_string()
}

fn fake_user_message_line(text: &str, session: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "say",
        "isMeta": false,
        "message": {"role": "user", "content": text},
        "sessionId": session,
        "timestamp": ts
    })
    .to_string()
}

fn fake_assistant_text_line(text: &str, session: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "say",
        "isMeta": false,
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
            "usage": {"input_tokens": 200, "output_tokens": 35}
        },
        "sessionId": session,
        "timestamp": ts
    })
    .to_string()
}

/// Decode a `ClientMessage::ClaudeEvent` into `(kind, tool, content, input_tokens, output_tokens)`.
fn decode_event(
    msg: ClientMessage,
) -> Option<(String, Option<String>, String, Option<u32>, Option<u32>)> {
    if let ClientMessage::ClaudeEvent(bytes) = msg {
        let event: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let kind = event["kind"].as_str()?.to_string();
        let tool = event["tool"].as_str().map(String::from);
        let content = event["content"].as_str()?.to_string();
        let input_tokens = event["inputTokens"].as_u64().map(|v| v as u32);
        let output_tokens = event["outputTokens"].as_u64().map(|v| v as u32);
        Some((kind, tool, content, input_tokens, output_tokens))
    } else {
        None
    }
}

/// Append JSONL lines to the transcript after a short delay.
async fn append_lines(transcript: PathBuf, lines: Vec<String>) {
    // Give the watcher time to initialize.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .expect("open transcript");
    for line in lines {
        writeln!(f, "{}", line).expect("write line");
    }
}

#[tokio::test]
async fn test_read_readme_activity_detected() -> anyhow::Result<()> {
    // 1. Fake project with README.md
    let dir = TempDir::new()?;
    std::fs::write(dir.path().join("README.md"), "# Test Project\nHello world\n")?;

    // 2. Empty JSONL transcript
    let transcript = dir.path().join("session-test.jsonl");
    std::fs::write(&transcript, "")?;

    let (tx, mut rx) = mpsc::channel::<ClientMessage>(64);

    // Start tracker in background
    let t_path = transcript.clone();
    let tracker = tokio::spawn(async move {
        tail_transcript(t_path, tx).await.ok();
    });

    // Write fake session lines after watcher is ready
    let t_path2 = transcript.clone();
    tokio::spawn(append_lines(
        t_path2,
        vec![
            fake_user_message_line(
                "Read the contents of readme.md",
                "sid-1",
                "2026-03-03T12:00:00Z",
            ),
            fake_tool_use_line("Read", "README.md", "sid-1", "2026-03-03T12:00:01Z"),
            fake_tool_result_line(
                "# Test Project\nHello world\n",
                "sid-1",
                "2026-03-03T12:00:02Z",
            ),
            fake_assistant_text_line(
                "The README.md contains 'Hello world'.",
                "sid-1",
                "2026-03-03T12:00:03Z",
            ),
        ],
    ));

    // Collect 4 non-meta events (up to 5 s)
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while events.len() < 4 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, rx.recv())
            .await?
            .expect("channel closed");
        // Skip the synthetic "transcript" path event.
        if let ClientMessage::ClaudeEvent(ref b) = msg {
            let v: serde_json::Value = serde_json::from_slice(b)?;
            if matches!(v["kind"].as_str(), Some("transcript") | Some("claude_pid")) {
                continue;
            }
        }
        if let Some(ev) = decode_event(msg) {
            events.push(ev);
        }
    }

    // Assertions
    assert_eq!(events[0].0, "user_message", "first event: user prompt");
    assert!(events[0].2.contains("readme.md"), "user message content");

    assert_eq!(events[1].0, "tool_use", "second event: Claude uses Read tool");
    assert_eq!(events[1].1.as_deref(), Some("Read"), "tool name");
    assert!(events[1].2.contains("README.md"), "tool_use path in content");
    assert_eq!(events[1].3, Some(120), "input tokens on tool_use");
    assert_eq!(events[1].4, Some(18), "output tokens on tool_use");

    assert_eq!(events[2].0, "tool_result", "third event: tool result");
    assert!(events[2].2.contains("Hello world"), "tool result content");

    assert_eq!(events[3].0, "assistant_message", "fourth: assistant reply");
    assert_eq!(events[3].3, Some(200), "input tokens on assistant_message");
    assert_eq!(events[3].4, Some(35), "output tokens on assistant_message");

    tracker.abort();
    Ok(())
}

#[tokio::test]
async fn test_pid_lifecycle_connect_disconnect() -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<ClientMessage>(32);

    // Spawn a dummy process (just sleep).
    let mut child1 = tokio::process::Command::new("sleep")
        .arg("60")
        .spawn()?;
    let pid1 = child1.id().unwrap();

    // Start PID tracker with known PID — should immediately emit "running".
    let tx2 = tx.clone();
    let pid_tracker = tokio::spawn(async move {
        track_pid_lifecycle(pid1, tx2).await.ok();
    });

    // Should immediately receive "running" event.
    let msg = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await?
        .expect("channel closed");
    if let ClientMessage::ClaudeEvent(ref b) = msg {
        let v: serde_json::Value = serde_json::from_slice(b)?;
        assert_eq!(v["kind"], "claude_pid");
        assert_eq!(v["tool"], "running");
        assert_eq!(v["content"].as_str().unwrap(), pid1.to_string());
    } else {
        panic!("expected ClaudeEvent, got something else");
    }

    // Kill the first process.
    child1.kill().await?;
    child1.wait().await?;

    // Tracker should detect death within the next poll cycle (≤ 3 s + margin).
    let mut got_dead = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !got_dead {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(Some(msg)) = tokio::time::timeout(remaining, rx.recv()).await {
            if let ClientMessage::ClaudeEvent(ref b) = msg {
                let v: serde_json::Value = serde_json::from_slice(b)?;
                if v["kind"] == "claude_pid" && v["tool"] == "dead" {
                    got_dead = true;
                }
            }
        }
    }
    assert!(got_dead, "should receive dead event when process exits");

    pid_tracker.abort();
    Ok(())
}
