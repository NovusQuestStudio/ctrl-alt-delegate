//! Claude Code and Codex lifecycle hooks: read the hook payload from stdin and report state only.
//! Reads the session id, event name, notification type, compaction source, and the folder name of `cwd`.
//! Prompts, tool input, transcripts, and assistant text are never read.

use crate::protocol::{project_name, Agent, Event, State};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

const CONTINUATIONS: [&str; 5] = ["PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "Notification"];

pub fn hook_event(agent: Agent, payload: &Value, now: i64) -> Option<Event> {
    let session_id = payload.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty() && s.chars().count() <= 256)?;
    let name = payload.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
    let compact = payload.get("source").and_then(Value::as_str) == Some("compact");
    let state = match name {
        // Compaction continues a busy session; it must not reset it to idle.
        "SessionStart" if compact => State::Working,
        "SessionStart" => State::Idle,
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => State::Working,
        "PermissionRequest" => State::Waiting,
        // idle_prompt means the session has sat idle: still ready, not waiting on a decision.
        "Notification" if matches!(payload.get("notification_type").and_then(Value::as_str), Some("permission_prompt" | "elicitation_dialog")) => State::Waiting,
        "Stop" | "Interrupt" => State::Idle,
        "StopFailure" => State::Error,
        "SessionEnd" => State::Closed,
        _ => return None,
    };
    Some(Event {
        agent,
        session_id: session_id.to_string(),
        state,
        updated_at: now,
        pid: None,
        heartbeat: false,
        continuation: CONTINUATIONS.contains(&name),
        interaction: name == "UserPromptSubmit" || (name == "SessionStart" && !compact),
        ppid: None,
        project: payload.get("cwd").and_then(Value::as_str).and_then(project_name),
        edition: None,
    })
}

/// Hooks are advisory: they never print, never fail the agent, and give up quickly.
pub fn run(agent: Agent) {
    let now = crate::now_ms(); // Stamp before reading stdin; delivery order need not match hook order.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = std::io::stdin().take(1024 * 1024 + 1).read_to_end(&mut data);
        let _ = tx.send(data);
    });
    let Ok(data) = rx.recv_timeout(Duration::from_millis(750)) else { return };
    if data.len() > 1024 * 1024 {
        return;
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&data) else { return };
    let Some(mut event) = hook_event(agent, &payload, now) else { return };
    if event.state != State::Closed {
        // The parent (a shell, or the agent itself) is alive while this hook runs, so the service can
        // walk up from it to the agent process once per session.
        event.ppid = crate::process::parent_pid();
    }
    let message = serde_json::json!({"type": "event", "event": event.to_json()});
    let _ = crate::ipc::request(&crate::ipc::service_path(), &message, Duration::from_millis(1500));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(name: &str, extra: Value) -> Option<Event> {
        let mut p = json!({"session_id": "s", "hook_event_name": name, "cwd": "C:\\work\\my-app", "prompt": "secret"});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        hook_event(Agent::ClaudeCode, &p, 1000)
    }

    #[test]
    fn states_interactions_compaction_closure_and_folder_only() {
        assert_eq!(ev("PostToolUse", json!({})).unwrap().state, State::Working);
        assert!(!ev("PostToolUse", json!({})).unwrap().interaction);
        assert!(ev("UserPromptSubmit", json!({})).unwrap().interaction);
        assert!(ev("SessionStart", json!({})).unwrap().interaction);
        let compact = ev("SessionStart", json!({"source": "compact"})).unwrap();
        assert_eq!((compact.state, compact.interaction), (State::Working, false));
        assert_eq!(ev("PermissionRequest", json!({})).unwrap().state, State::Waiting);
        assert_eq!(ev("Stop", json!({})).unwrap().state, State::Idle);
        assert_eq!(ev("Interrupt", json!({})).unwrap().state, State::Idle);
        assert_eq!(ev("SessionEnd", json!({})).unwrap().state, State::Closed);
        assert_eq!(ev("StopFailure", json!({})).unwrap().state, State::Error);
        assert!(ev("SubagentStop", json!({})).is_none());
        assert!(ev("Notification", json!({"notification_type": "auth_success"})).is_none());
        assert!(ev("Notification", json!({"notification_type": "idle_prompt"})).is_none(), "idle reminders keep the session Ready");
        assert_eq!(ev("Notification", json!({"notification_type": "permission_prompt"})).unwrap().state, State::Waiting);
        let e = ev("UserPromptSubmit", json!({})).unwrap();
        assert_eq!(e.project.as_deref(), Some("my-app"));
        assert!(!e.to_json().to_string().contains("secret"));
        assert!(hook_event(Agent::Codex, &json!({"hook_event_name": "Stop"}), 1).is_none(), "no session id, no event");
    }
}
