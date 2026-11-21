//! Claude Code, Codex, Devin and Antigravity lifecycle hooks: parse the hook payload from stdin and
//! report state only. Devin uses the same event names as Claude Code, and also runs Claude Code's hooks
//! by default. Keeps the session id, event name, notification type, compaction source, and the folder
//! name of `cwd`. Prompts, tool input, transcripts, and assistant text are dropped, never stored or sent.

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
        project: payload.get("cwd").and_then(Value::as_str).and_then(project_name).or_else(|| {
            // Devin's payload has no cwd; it puts the project folder in the environment instead.
            (agent == Agent::Devin).then(|| std::env::var("DEVIN_PROJECT_DIR").ok()).flatten().as_deref().and_then(project_name)
        }),
        edition: None,
    })
}

/// Antigravity's hooks don't say which event fired (the plugin passes it as an argument) and use
/// their own field names. This turns one into the Claude Code shape `hook_event` reads, or None for
/// events that change nothing. Antigravity has no prompt-submitted, waiting or session-end event.
pub fn antigravity_payload(event: &str, raw: &Value) -> Option<Value> {
    let name = match event {
        "SessionStart" => "SessionStart",
        // The first model call after the user's message starts a turn; later calls continue it.
        "PreInvocation" if raw.get("invocationNum").and_then(Value::as_u64) == Some(0) => "UserPromptSubmit",
        "PreInvocation" | "PreToolUse" | "PostToolUse" => "PostToolUse",
        // Background tasks still running mean the agent isn't done yet.
        "Stop" if raw.get("fullyIdle").and_then(Value::as_bool) == Some(false) => return None,
        "Stop" => "Stop",
        _ => return None,
    };
    let cwd = raw.get("workspacePaths").and_then(Value::as_array).and_then(|p| p.first()).cloned().unwrap_or(Value::Null);
    Some(serde_json::json!({"session_id": raw.get("conversationId")?, "hook_event_name": name, "cwd": cwd}))
}

/// Hooks are advisory: they never print, never fail the agent, and give up quickly.
pub fn run(agent: Agent, event: Option<&str>) {
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
    let Ok(mut payload) = serde_json::from_slice::<Value>(&data) else { return };
    if agent == Agent::Antigravity {
        let Some(converted) = event.and_then(|e| antigravity_payload(e, &payload)) else { return };
        payload = converted;
    }
    let agent = actual_agent(agent);
    let Some(mut event) = hook_event(agent, &payload, now) else { return };
    if event.state != State::Closed {
        // The parent (a shell, or the agent itself) is alive while this hook runs, so the service can
        // walk up from it to the agent process once per session.
        event.ppid = crate::process::parent_pid();
    }
    let message = serde_json::json!({"type": "event", "event": event.to_json()});
    let _ = crate::ipc::request(&crate::ipc::service_path(), &message, Duration::from_millis(1500));
}

/// Devin runs Claude Code's hooks too (its `read_config_from.claude` default), so a "claude-code" hook
/// may really be Devin. The agent is whichever program is the hook's nearest non-shell ancestor.
fn actual_agent(declared: Agent) -> Agent {
    if declared != Agent::ClaudeCode {
        return declared;
    }
    let Some(parent) = crate::process::parent_pid() else { return declared };
    let Ok(table) = crate::process::snapshot() else { return declared };
    let chain = crate::process::ancestry(&table, parent, 16);
    match crate::process::find_agent(&chain) {
        Some(p) if is_devin(&p.name) => Agent::Devin,
        _ => declared,
    }
}

fn is_devin(process: &str) -> bool {
    let lower = process.to_ascii_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower) == "devin"
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

    #[test]
    fn antigravity_events_map_to_states() {
        let raw = |extra: Value| {
            let mut v = json!({"conversationId": "conv-1", "workspacePaths": ["C:\\work\\my-app"], "lastUserInput": "secret prompt"});
            for (k, x) in extra.as_object().unwrap() {
                v[k] = x.clone();
            }
            v
        };
        let event = |name: &str, extra: Value| antigravity_payload(name, &raw(extra)).and_then(|p| hook_event(Agent::Antigravity, &p, 9));
        let first = event("PreInvocation", json!({"invocationNum": 0})).unwrap();
        assert_eq!((first.state, first.interaction, first.session_id.as_str()), (State::Working, true, "conv-1"));
        assert_eq!(first.project.as_deref(), Some("my-app"));
        assert!(!first.to_json().to_string().contains("secret"), "the prompt is never passed on");
        let later = event("PreInvocation", json!({"invocationNum": 3})).unwrap();
        assert_eq!((later.state, later.interaction), (State::Working, false));
        assert_eq!(event("PostToolUse", json!({})).unwrap().state, State::Working);
        assert_eq!(event("SessionStart", json!({})).unwrap().state, State::Idle);
        assert_eq!(event("Stop", json!({"fullyIdle": true})).unwrap().state, State::Idle);
        assert!(event("Stop", json!({"fullyIdle": false})).is_none(), "background work keeps it working");
        assert!(event("PostInvocation", json!({})).is_none());
        assert!(antigravity_payload("Stop", &json!({"fullyIdle": true})).is_none(), "no conversation id, no event");
    }

    #[test]
    fn devin_uses_the_claude_events() {
        let start = hook_event(Agent::Devin, &json!({"session_id": "d", "hook_event_name": "UserPromptSubmit", "prompt_id": "p"}), 5).unwrap();
        assert_eq!((start.agent, start.state, start.interaction), (Agent::Devin, State::Working, true));
        assert_eq!(hook_event(Agent::Devin, &json!({"session_id": "d", "hook_event_name": "PermissionRequest"}), 6).unwrap().state, State::Waiting);
        assert_eq!(hook_event(Agent::Devin, &json!({"session_id": "d", "hook_event_name": "Stop", "stop_hook_active": false}), 7).unwrap().state, State::Idle);
        assert!(is_devin("devin.exe") && is_devin("Devin.exe") && !is_devin("devin-desktop.exe") && !is_devin("claude.exe"));
    }
}
