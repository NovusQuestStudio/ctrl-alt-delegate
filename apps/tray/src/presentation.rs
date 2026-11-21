//! The only code that decides what leaves this machine for Discord: the tool label, phase,
//! optional project name, start time, optional image, and optional tool badge. Nothing else.

use crate::protocol::{badge_key, tool_label, Agent, State};
use crate::store::Session;
use crate::APP_NAME;
use serde_json::{json, Value};

pub fn activity_for(session: Option<&Session>, show_project: bool, large_image: &str, tool_badge: bool) -> Option<Value> {
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
    // Discord draws the badge on the large image's corner, so it needs a large image.
    if !large_image.is_empty() {
        activity["assets"] = json!({"large_image": large_image, "large_text": APP_NAME});
        if tool_badge {
            activity["assets"]["small_image"] = json!(badge_key(s.agent, s.edition));
            activity["assets"]["small_text"] = json!(tool_label(s.agent, s.edition));
        }
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
        let hidden = activity_for(session, false, "", true).unwrap();
        assert_eq!(hidden, json!({"type": 0, "name": "Ctrl Alt Delegate", "details": "Claude Code · Ready for a prompt", "state": "Delegating to an AI agent", "timestamps": {"start": 1}}));
        assert!(!hidden.to_string().contains("client-x") && !hidden.to_string().contains("secret-id"));
        let shown = activity_for(session, true, "icon", false).unwrap();
        assert_eq!(shown["state"], "client-x");
        assert_eq!(shown["assets"], json!({"large_image": "icon", "large_text": "Ctrl Alt Delegate"}));
        assert_eq!(activity_for(None, false, "", true), None);
    }

    #[test]
    fn tool_badge_names_the_tool_on_the_large_image() {
        let mut s = store();
        s.update(ev("claude-code", "c", "working", 1000, json!({})), 1000).unwrap();
        let a = activity_for(s.sessions.values().next(), false, "icon", true).unwrap();
        assert_eq!(
            a["assets"],
            json!({"large_image": "icon", "large_text": "Ctrl Alt Delegate", "small_image": "tool_claude_code", "small_text": "Claude Code"})
        );
        // No large image means no badge: Discord has nothing to draw it on.
        assert!(activity_for(s.sessions.values().next(), false, "", true).unwrap().get("assets").is_none());
    }

    #[test]
    fn every_tool_has_a_badge() {
        use crate::protocol::{Edition, Agent::*};
        let cases = [
            (ClaudeCode, None, "tool_claude_code"),
            (Codex, None, "tool_codex"),
            (Devin, None, "tool_devin"),
            (Antigravity, None, "tool_antigravity"),
            (Vscode, None, "tool_vscode"),
            (Vscode, Some(Edition::Insiders), "tool_vscode_insiders"),
            (Vscode, Some(Edition::Vscodium), "tool_vscodium"),
            (Vscode, Some(Edition::Cursor), "tool_cursor"),
            (Vscode, Some(Edition::Windsurf), "tool_windsurf"),
            (Vscode, Some(Edition::Devin), "tool_devin"),
            (Vscode, Some(Edition::Antigravity), "tool_antigravity"),
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/discord/badges");
        for (agent, edition, key) in cases {
            assert_eq!(badge_key(agent, edition), key);
            assert!(dir.join(format!("{key}.png")).is_file(), "missing badge image {key}.png");
        }
    }

    #[test]
    fn editor_phases_and_editions() {
        let mut s = store();
        s.update(ev("vscode", "w", "working", 1000, json!({"edition": "insiders"})), 1000).unwrap();
        let a = activity_for(s.sessions.values().next(), false, "", true).unwrap();
        assert_eq!(a["details"], "VS Code Insiders · Editing");
        assert_eq!(a["state"], "In the editor");
    }
}
