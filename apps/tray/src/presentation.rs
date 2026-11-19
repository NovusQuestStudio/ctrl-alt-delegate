//! The only code that decides what leaves this machine for Discord: the tool label, phase,
//! optional project name, start time, and optional image. Nothing else.

use crate::protocol::{tool_label, Agent, State};
use crate::store::Session;
use crate::APP_NAME;
use serde_json::{json, Value};

pub fn activity_for(session: Option<&Session>, show_project: bool, large_image: &str) -> Option<Value> {
    let s = session?;
    let phase = if s.agent == Agent::Vscode {
        match s.state {
            State::Working => "Editing",
            State::Closed => "Closed",
            _ => "Idle",
        }
    } else {
        match s.state {
            State::Idle => "Ready for a prompt",
            State::Working => "Working",
            State::Waiting => "Waiting for input",
            State::Error => "Needs attention",
            State::Closed => "Closed",
        }
    };
    let state = match (&s.project, show_project) {
        (Some(project), true) => project.clone(),
        _ if s.agent == Agent::Vscode => "In the editor".into(),
        _ => "Delegating to an AI agent".into(),
    };
    let mut activity = json!({
        "type": 0, "name": APP_NAME, "details": format!("{} · {}", tool_label(s.agent, s.edition), phase), "state": state,
        "timestamps": {"start": s.started_at / 1000},
    });
    if !large_image.is_empty() {
        activity["assets"] = json!({"large_image": large_image, "large_text": APP_NAME});
    }
    Some(activity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{ev, store};

    #[test]
    fn branded_private_by_default_and_without_identifiers() {
        let mut s = store();
        s.update(ev("claude-code", "secret-id", "working", 1000, json!({"project": "client-x"})), 1000).unwrap();
        s.update(ev("claude-code", "secret-id", "idle", 3000, json!({})), 3000).unwrap();
        let session = s.sessions.values().next();
        let hidden = activity_for(session, false, "").unwrap();
        assert_eq!(hidden, json!({"type": 0, "name": "Ctrl Alt Delegate", "details": "Claude Code · Ready for a prompt", "state": "Delegating to an AI agent", "timestamps": {"start": 1}}));
        assert!(!hidden.to_string().contains("client-x") && !hidden.to_string().contains("secret-id"));
        let shown = activity_for(session, true, "icon").unwrap();
        assert_eq!(shown["state"], "client-x");
        assert_eq!(shown["assets"], json!({"large_image": "icon", "large_text": "Ctrl Alt Delegate"}));
        assert_eq!(activity_for(None, false, ""), None);
    }

    #[test]
    fn editor_phases_and_editions() {
        let mut s = store();
        s.update(ev("vscode", "w", "working", 1000, json!({"edition": "insiders"})), 1000).unwrap();
        let a = activity_for(s.sessions.values().next(), false, "").unwrap();
        assert_eq!(a["details"], "VS Code Insiders · Editing");
        assert_eq!(a["state"], "In the editor");
    }
}
