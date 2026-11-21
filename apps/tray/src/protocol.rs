//! Event protocol shared with the VS Code extension (src/protocol.ts) and agent hooks.
//! Only whitelisted fields are kept: never prompts, paths, file names, or tool arguments.

use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Agent {
    ClaudeCode,
    Codex,
    Devin,
    Antigravity,
    Vscode,
}

impl Agent {
    pub fn parse(s: &str) -> Option<Agent> {
        match s {
            "claude-code" => Some(Agent::ClaudeCode),
            "codex" => Some(Agent::Codex),
            "devin" => Some(Agent::Devin),
            "antigravity" => Some(Agent::Antigravity),
            "vscode" => Some(Agent::Vscode),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude-code",
            Agent::Codex => "codex",
            Agent::Devin => "devin",
            Agent::Antigravity => "antigravity",
            Agent::Vscode => "vscode",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
            Agent::Devin => "Devin",
            Agent::Antigravity => "Antigravity",
            Agent::Vscode => "VS Code",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Waiting,
    Error,
    Closed,
}

impl State {
    pub fn parse(s: &str) -> Option<State> {
        match s {
            "idle" => Some(State::Idle),
            "working" => Some(State::Working),
            "waiting" => Some(State::Waiting),
            "error" => Some(State::Error),
            "closed" => Some(State::Closed),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Working => "working",
            State::Waiting => "waiting",
            State::Error => "error",
            State::Closed => "closed",
        }
    }
    pub fn active(self) -> bool {
        matches!(self, State::Working | State::Waiting | State::Error)
    }
}

/// Editor build within the VS Code family. `None` means VS Code stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edition {
    Insiders,
    Vscodium,
    Cursor,
    Windsurf,
    Devin,
    Antigravity,
}

impl Edition {
    pub fn parse(s: &str) -> Option<Edition> {
        match s {
            "insiders" => Some(Edition::Insiders),
            "vscodium" => Some(Edition::Vscodium),
            "cursor" => Some(Edition::Cursor),
            "windsurf" => Some(Edition::Windsurf),
            "devin" => Some(Edition::Devin),
            "antigravity" => Some(Edition::Antigravity),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Edition::Insiders => "insiders",
            Edition::Vscodium => "vscodium",
            Edition::Cursor => "cursor",
            Edition::Windsurf => "windsurf",
            Edition::Devin => "devin",
            Edition::Antigravity => "antigravity",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Edition::Insiders => "VS Code Insiders",
            Edition::Vscodium => "VSCodium",
            Edition::Cursor => "Cursor",
            Edition::Windsurf => "Windsurf",
            Edition::Devin => "Devin Desktop",
            Edition::Antigravity => "Antigravity",
        }
    }
}

pub fn tool_label(agent: Agent, edition: Option<Edition>) -> &'static str {
    match (agent, edition) {
        (Agent::Vscode, Some(edition)) => edition.label(),
        _ => agent.label(),
    }
}

/// Discord art-asset key of the tool's badge (assets/discord/badges, built by scripts/make-badges.ps1).
/// Devin Desktop and Antigravity's editor share their agent's badge.
pub fn badge_key(agent: Agent, edition: Option<Edition>) -> &'static str {
    match (agent, edition) {
        (Agent::ClaudeCode, _) => "tool_claude_code",
        (Agent::Codex, _) => "tool_codex",
        (Agent::Devin, _) | (Agent::Vscode, Some(Edition::Devin)) => "tool_devin",
        (Agent::Antigravity, _) | (Agent::Vscode, Some(Edition::Antigravity)) => "tool_antigravity",
        (Agent::Vscode, None) => "tool_vscode",
        (Agent::Vscode, Some(Edition::Insiders)) => "tool_vscode_insiders",
        (Agent::Vscode, Some(Edition::Vscodium)) => "tool_vscodium",
        (Agent::Vscode, Some(Edition::Cursor)) => "tool_cursor",
        (Agent::Vscode, Some(Edition::Windsurf)) => "tool_windsurf",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub agent: Agent,
    pub session_id: String,
    pub state: State,
    /// Milliseconds since the Unix epoch.
    pub updated_at: i64,
    /// Long-lived reporter process (e.g. the VS Code extension host).
    pub pid: Option<u32>,
    pub heartbeat: bool,
    pub continuation: bool,
    /// A deliberate user action (prompt, session start, editor activity).
    pub interaction: bool,
    /// Parent of a hook or reporter process; used once to find the agent/editor process, never stored.
    pub ppid: Option<u32>,
    /// Folder name only. Shown in Discord only when the user enables it.
    pub project: Option<String>,
    pub edition: Option<Edition>,
}

pub fn session_key(agent: Agent, session_id: &str) -> String {
    format!("{}:{}", agent.as_str(), session_id)
}

/// Reduce a path to a safe display name: last segment, no control characters, at most 64 characters.
pub fn project_name(value: &str) -> Option<String> {
    let trimmed = value.trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = last.chars().filter(|c| !c.is_control()).take(64).collect();
    let clean = clean.trim().to_string();
    let drive = clean.len() == 2 && clean.as_bytes()[1] == b':' && clean.as_bytes()[0].is_ascii_alphabetic();
    (!clean.is_empty() && !drive).then_some(clean)
}

fn positive_u32(v: &Value, key: &str) -> Result<Option<u32>, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => x.as_u64().filter(|n| *n > 0 && *n <= u32::MAX as u64).map(|n| Some(n as u32)).ok_or(format!("Invalid {key}")),
    }
}

fn flag(v: &Value, key: &str) -> Result<bool, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("Invalid {key}")),
    }
}

pub fn parse_event(v: &Value, now: i64) -> Result<Event, String> {
    if !v.is_object() {
        return Err("Expected event object".into());
    }
    let agent = (v.get("version") == Some(&json!(1)))
        .then(|| v.get("agent").and_then(Value::as_str).and_then(Agent::parse))
        .flatten()
        .ok_or("Unsupported protocol or agent")?;
    let session_id = v.get("sessionId").and_then(Value::as_str).filter(|s| !s.is_empty() && s.chars().count() <= 256).ok_or("Invalid sessionId")?;
    let state = v.get("state").and_then(Value::as_str).and_then(State::parse).ok_or("Invalid state")?;
    let updated_at = v.get("updatedAt").and_then(Value::as_i64).filter(|t| *t >= 0 && *t <= now + 60_000).ok_or("Invalid updatedAt")?;
    let pid = positive_u32(v, "pid")?;
    let ppid = positive_u32(v, "ppid")?;
    let heartbeat = flag(v, "heartbeat")?;
    let continuation = flag(v, "continuation")?;
    let interaction = flag(v, "interaction")?;
    let project = match v.get("project") {
        None | Some(Value::Null) => None,
        Some(Value::String(p)) if project_name(p).as_deref() == Some(p.as_str()) => Some(p.clone()),
        _ => return Err("Invalid project".into()),
    };
    let edition = match v.get("edition") {
        None | Some(Value::Null) => None,
        Some(Value::String(e)) if agent == Agent::Vscode => Some(Edition::parse(e).ok_or("Invalid edition")?),
        _ => return Err("Invalid edition".into()),
    };
    Ok(Event { agent, session_id: session_id.to_string(), state, updated_at, pid, heartbeat, continuation, interaction, ppid, project, edition })
}

impl Event {
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("version".into(), json!(1));
        m.insert("agent".into(), json!(self.agent.as_str()));
        m.insert("sessionId".into(), json!(self.session_id));
        m.insert("state".into(), json!(self.state.as_str()));
        m.insert("updatedAt".into(), json!(self.updated_at));
        if let Some(p) = self.pid { m.insert("pid".into(), json!(p)); }
        if let Some(p) = self.ppid { m.insert("ppid".into(), json!(p)); }
        if self.heartbeat { m.insert("heartbeat".into(), json!(true)); }
        if self.continuation { m.insert("continuation".into(), json!(true)); }
        if self.interaction { m.insert("interaction".into(), json!(true)); }
        if let Some(p) = &self.project { m.insert("project".into(), json!(p)); }
        if let Some(e) = self.edition { m.insert("edition".into(), json!(e.as_str())); }
        Value::Object(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(extra: Value) -> Value {
        let mut base = json!({"version": 1, "agent": "codex", "sessionId": "a", "state": "idle", "updatedAt": 1000});
        for (k, v) in extra.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    #[test]
    fn whitelists_fields_and_rejects_bad_data() {
        let parsed = parse_event(&e(json!({"prompt": "secret", "cwd": "/secret", "tool": "Bash"})), 1000).unwrap();
        let out = parsed.to_json().to_string();
        assert!(!out.contains("secret") && !out.contains("Bash"));
        for bad in [json!({"version": 2}), json!({"agent": "x"}), json!({"agent": "opencode"}), json!({"state": "x"}), json!({"updatedAt": 99999999}),
            json!({"pid": -1}), json!({"ppid": 0}), json!({"interaction": "yes"}), json!({"sessionId": ""}), json!({"project": "a/b"}), json!({"project": "C:\\x"}),
            json!({"edition": "insiders"})] {
            assert!(parse_event(&e(bad.clone()), 1000).is_err(), "{bad}");
        }
    }

    #[test]
    fn project_name_keeps_only_the_folder() {
        assert_eq!(project_name("C:\\Users\\me\\Code\\my-app\\").as_deref(), Some("my-app"));
        assert_eq!(project_name("/home/me/secret-client/project").as_deref(), Some("project"));
        assert_eq!(project_name("C:\\"), None);
        assert_eq!(project_name(""), None);
    }

    #[test]
    fn editions_are_vscode_only_and_labelled() {
        let ok = parse_event(&e(json!({"agent": "vscode", "edition": "insiders"})), 1000).unwrap();
        assert_eq!(tool_label(ok.agent, ok.edition), "VS Code Insiders");
        assert!(parse_event(&e(json!({"agent": "vscode", "edition": "nightly"})), 1000).is_err());
        assert_eq!(tool_label(Agent::Vscode, Some(Edition::Devin)), "Devin Desktop");
        assert_eq!(tool_label(Agent::Vscode, None), "VS Code");
        assert_eq!(tool_label(Agent::ClaudeCode, None), "Claude Code");
    }
}
