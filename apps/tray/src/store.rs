//! Session store: one entry per tool session, with lifecycle, liveness, and restart persistence.

use crate::protocol::{parse_event, project_name, session_key, Agent, Edition, Event, State};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub agent: Agent,
    pub session_id: String,
    pub state: State,
    pub updated_at: i64,
    pub pid: Option<u32>,
    pub continuation: bool,
    pub project: Option<String>,
    pub edition: Option<Edition>,
    pub started_at: i64,
    pub last_seen: i64,
    /// Time of the last deliberate user action in this session.
    pub last_interaction: i64,
    /// Long-lived agent/editor process found from the reporter's ancestry; used for liveness and focus.
    pub agent_pid: Option<u32>,
    pub agent_name: Option<String>,
    /// Agent pid and its ancestors (terminal, editor, desktop app), used to match the focused window.
    pub ancestors: Vec<u32>,
}

impl Session {
    pub fn key(&self) -> String {
        session_key(self.agent, &self.session_id)
    }
}

pub struct AgentProcess {
    pub agent_pid: u32,
    pub agent_name: String,
    pub ancestors: Vec<u32>,
}

pub type Alive = Box<dyn Fn(u32) -> bool + Send>;

pub struct SessionStore {
    pub sessions: HashMap<String, Session>,
    ended: HashMap<String, i64>,
    ended_order: VecDeque<String>,
    pub pinned: Option<String>,
    /// Bumped on every change worth persisting.
    pub revision: u64,
    pub stale_ms: i64,
    alive: Alive,
}

impl SessionStore {
    pub fn new(stale_ms: i64, alive: Alive) -> Self {
        SessionStore { sessions: HashMap::new(), ended: HashMap::new(), ended_order: VecDeque::new(), pinned: None, revision: 0, stale_ms, alive }
    }

    /// Apply an event. Returns the session key when the session was created or updated.
    pub fn update(&mut self, event: Event, now: i64) -> Result<Option<String>, String> {
        let key = session_key(event.agent, &event.session_id);
        let prior = self.sessions.get(&key);
        if event.updated_at <= self.ended.get(&key).copied().unwrap_or(-1) || prior.is_some_and(|p| event.updated_at < p.updated_at) {
            return Ok(None);
        }
        if event.state == State::Closed {
            self.drop_session(&key, event.updated_at);
            return Ok(None);
        }
        if prior.is_none() && self.sessions.len() >= 256 {
            return Err("Session limit reached".into());
        }
        let last_interaction = match prior {
            Some(p) if !event.interaction => p.last_interaction,
            Some(p) => event.updated_at.max(p.last_interaction),
            None => event.updated_at,
        };
        let session = Session {
            agent: event.agent,
            session_id: event.session_id.clone(),
            state: event.state,
            updated_at: event.updated_at,
            pid: event.pid,
            continuation: event.continuation,
            project: event.project.clone().or_else(|| prior.and_then(|p| p.project.clone())),
            edition: event.edition.or_else(|| prior.and_then(|p| p.edition)),
            started_at: prior.map_or(event.updated_at, |p| p.started_at),
            last_seen: now,
            last_interaction,
            agent_pid: prior.and_then(|p| p.agent_pid),
            agent_name: prior.and_then(|p| p.agent_name.clone()),
            ancestors: prior.map(|p| p.ancestors.clone()).unwrap_or_default(),
        };
        if !event.heartbeat || prior.is_none() {
            self.revision += 1;
        }
        self.sessions.insert(key.clone(), session);
        Ok(Some(key))
    }

    pub fn attach(&mut self, key: &str, agent: AgentProcess) {
        if let Some(s) = self.sessions.get_mut(key) {
            s.agent_pid = Some(agent.agent_pid);
            s.agent_name = Some(agent.agent_name);
            s.ancestors = agent.ancestors;
            self.revision += 1;
        }
    }

    pub fn drop_session(&mut self, key: &str, at: i64) {
        if self.sessions.remove(key).is_some() {
            self.revision += 1;
        }
        if self.ended.insert(key.to_string(), at).is_none() {
            self.ended_order.push_back(key.to_string());
        }
        if self.pinned.as_deref() == Some(key) {
            self.pinned = None;
        }
    }

    pub fn sweep(&mut self, now: i64) {
        let expired: Vec<(String, i64)> = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                // Reporters (pid) renew a 90 s lease with heartbeats; hook sessions expire after stale_ms without events.
                let limit = if s.pid.is_some() { 90_000 } else { self.stale_ms };
                let owner = s.pid.or(s.agent_pid);
                now - s.last_seen > limit || owner.is_some_and(|pid| !(self.alive)(pid))
            })
            .map(|(k, s)| (k.clone(), s.updated_at))
            .collect();
        // Expiry is not a close: it is not recorded as ended, so a reporter whose heartbeat was only late
        // (for example across a service restart) comes back with its next heartbeat. Dead processes send nothing.
        for (key, _) in expired {
            if self.sessions.remove(&key).is_some() {
                self.revision += 1;
            }
            if self.pinned.as_deref() == Some(key.as_str()) {
                self.pinned = None;
            }
        }
        let stale_ms = self.stale_ms;
        self.ended.retain(|_, at| now - *at <= stale_ms);
        let ended = &self.ended;
        self.ended_order.retain(|k| ended.contains_key(k));
        while self.ended_order.len() > 1024 {
            if let Some(k) = self.ended_order.pop_front() {
                self.ended.remove(&k);
            }
        }
    }

    pub fn pin(&mut self, key: Option<&str>) -> Result<(), String> {
        if let Some(k) = key {
            if !self.sessions.contains_key(k) {
                return Err("Unknown session".into());
            }
        }
        self.pinned = key.map(str::to_string);
        self.revision += 1;
        Ok(())
    }

    /// The persisted form (also compatible with the earlier Node service's sessions.json).
    pub fn snapshot(&self) -> Value {
        let sessions: Vec<Value> = self.sessions.values().map(session_json).collect();
        json!({"sessions": sessions, "pinned": self.pinned})
    }

    /// Restore sessions saved by a previous run. Invalid entries are dropped; stale and dead ones are swept.
    pub fn restore(&mut self, saved: &Value, now: i64) {
        let Some(list) = saved.get("sessions").and_then(Value::as_array) else { return };
        for raw in list.iter().take(256) {
            let mut candidate = raw.clone();
            if !candidate.is_object() {
                continue;
            }
            candidate["version"] = json!(1);
            let Ok(event) = parse_event(&candidate, now) else { continue };
            let time = |k: &str| raw.get(k).and_then(Value::as_i64).filter(|t| *t >= 0 && *t <= now + 60_000);
            let (Some(started_at), Some(last_seen), Some(last_interaction)) = (time("startedAt"), time("lastSeen"), time("lastInteraction")) else { continue };
            if event.state == State::Closed {
                continue;
            }
            let agent_pid = raw.get("agentPid").and_then(Value::as_u64).filter(|p| *p > 0 && *p <= u32::MAX as u64).map(|p| p as u32);
            let ancestors: Vec<u32> = raw
                .get("ancestors")
                .and_then(Value::as_array)
                .filter(|a| a.len() <= 16)
                .map(|a| a.iter().filter_map(|p| p.as_u64().filter(|n| *n > 0 && *n <= u32::MAX as u64).map(|n| n as u32)).collect())
                .unwrap_or_default();
            let agent_name = raw.get("agentName").and_then(Value::as_str).filter(|n| n.len() <= 260).map(str::to_string);
            let session = Session {
                agent: event.agent,
                session_id: event.session_id.clone(),
                state: event.state,
                updated_at: event.updated_at,
                pid: event.pid,
                continuation: event.continuation,
                project: event.project.and_then(|p| project_name(&p)),
                edition: event.edition,
                started_at,
                // Heartbeats are not saved, so a live reporter's saved last_seen is old: restart its lease.
                last_seen: if event.pid.is_some() { last_seen.max(now) } else { last_seen },
                last_interaction,
                agent_pid,
                agent_name,
                ancestors: if agent_pid.is_some() { ancestors } else { vec![] },
            };
            self.sessions.insert(session.key(), session);
        }
        if let Some(p) = saved.get("pinned").and_then(Value::as_str) {
            if self.sessions.contains_key(p) {
                self.pinned = Some(p.to_string());
            }
        }
        self.sweep(now);
    }
}

pub fn session_json(s: &Session) -> Value {
    json!({
        "agent": s.agent.as_str(), "sessionId": s.session_id, "state": s.state.as_str(), "updatedAt": s.updated_at,
        "pid": s.pid, "continuation": s.continuation, "project": s.project, "edition": s.edition.map(|e| e.as_str()),
        "startedAt": s.started_at, "lastSeen": s.last_seen, "lastInteraction": s.last_interaction,
        "agentPid": s.agent_pid, "agentName": s.agent_name, "ancestors": s.ancestors,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::protocol::parse_event;

    pub fn ev(agent: &str, id: &str, state: &str, at: i64, extra: Value) -> Event {
        let mut v = json!({"version": 1, "agent": agent, "sessionId": id, "state": state, "updatedAt": at});
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        parse_event(&v, i64::MAX / 2).unwrap()
    }
    pub fn store() -> SessionStore {
        SessionStore::new(30 * 60_000, Box::new(|_| true))
    }

    #[test]
    fn late_events_cannot_regress_or_resurrect() {
        let mut s = store();
        s.update(ev("codex", "a", "idle", 2000, json!({})), 2000).unwrap();
        s.update(ev("codex", "a", "working", 1000, json!({})), 2001).unwrap();
        assert_eq!(s.sessions["codex:a"].state, State::Idle);
        s.update(ev("codex", "a", "closed", 3000, json!({})), 3000).unwrap();
        s.update(ev("codex", "a", "working", 2000, json!({"heartbeat": true})), 3001).unwrap();
        assert!(s.sessions.is_empty());
        s.update(ev("codex", "a", "working", 4000, json!({})), 4000).unwrap();
        assert_eq!(s.sessions["codex:a"].started_at, 4000);
    }

    #[test]
    fn stale_sessions_and_process_death() {
        let mut s = SessionStore::new(300_000, Box::new(|pid| pid != 999 && pid != 777));
        s.update(ev("codex", "hook", "working", 1000, json!({})), 1000).unwrap();
        s.update(ev("codex", "extension", "working", 1000, json!({"pid": 123})), 1000).unwrap();
        s.update(ev("codex", "dead", "working", 1000, json!({"pid": 999})), 1000).unwrap();
        s.update(ev("codex", "agent", "idle", 1000, json!({})), 1000).unwrap();
        s.attach("codex:agent", AgentProcess { agent_pid: 777, agent_name: "codex.exe".into(), ancestors: vec![777] });
        s.sweep(2000);
        assert!(!s.sessions.contains_key("codex:dead") && !s.sessions.contains_key("codex:agent"));
        s.sweep(92_000);
        assert!(!s.sessions.contains_key("codex:extension"));
        assert!(s.sessions.contains_key("codex:hook"));
        s.sweep(302_000);
        assert!(s.sessions.is_empty());
    }

    #[test]
    fn interactions_and_heartbeats() {
        let mut s = store();
        s.update(ev("codex", "a", "working", 1000, json!({"interaction": true})), 1000).unwrap();
        s.update(ev("codex", "a", "working", 2000, json!({"continuation": true})), 2000).unwrap();
        assert_eq!(s.sessions["codex:a"].last_interaction, 1000);
        let rev = s.revision;
        s.update(ev("codex", "a", "working", 2000, json!({"heartbeat": true})), 3000).unwrap();
        assert_eq!(s.revision, rev, "heartbeats are not persisted changes");
        assert_eq!(s.sessions["codex:a"].last_seen, 3000);
    }

    #[test]
    fn restore_keeps_timers_and_drops_bad_entries() {
        let now = 1_000_000;
        let mut before = SessionStore::new(300_000, Box::new(|pid| pid != 13));
        before.update(ev("claude-code", "keep", "working", 900_000, json!({"interaction": true, "project": "app"})), 900_000).unwrap();
        before.attach("claude-code:keep", AgentProcess { agent_pid: 12, agent_name: "claude.exe".into(), ancestors: vec![12, 11] });
        before.pin(Some("claude-code:keep")).unwrap();
        let mut saved = before.snapshot();
        let base = saved["sessions"][0].clone();
        let list = saved["sessions"].as_array_mut().unwrap();
        let mut dead = base.clone(); dead["sessionId"] = json!("dead"); dead["agentPid"] = json!(13); list.push(dead);
        let mut old = base.clone(); old["sessionId"] = json!("old"); old["agentPid"] = Value::Null; old["lastSeen"] = json!(1); list.push(old);
        let mut bad = base.clone(); bad["sessionId"] = json!("bad"); bad["startedAt"] = json!("yesterday"); list.push(bad);
        let mut evil = base.clone(); evil["sessionId"] = json!("evil"); evil["agent"] = json!("opencode"); list.push(evil);
        let mut after = SessionStore::new(300_000, Box::new(|pid| pid != 13));
        after.restore(&saved, now);
        assert_eq!(after.sessions.keys().collect::<Vec<_>>(), vec!["claude-code:keep"]);
        let k = &after.sessions["claude-code:keep"];
        assert_eq!((k.started_at, k.agent_pid, k.ancestors.clone()), (900_000, Some(12), vec![12, 11]));
        assert_eq!(after.pinned.as_deref(), Some("claude-code:keep"));
        after.restore(&json!("garbage"), now);
        after.restore(&json!({"sessions": "x"}), now);
    }

    #[test]
    fn a_late_heartbeat_revives_an_expired_reporter_but_not_a_closed_session() {
        let mut s = store();
        s.update(ev("vscode", "w", "idle", 1000, json!({"pid": 5})), 1000).unwrap();
        s.sweep(100_000);
        assert!(s.sessions.is_empty(), "lease expired");
        s.update(ev("vscode", "w", "idle", 1000, json!({"pid": 5, "heartbeat": true})), 101_000).unwrap();
        assert_eq!(s.sessions["vscode:w"].started_at, 1000, "the heartbeat brings it back with its timer");
        s.update(ev("vscode", "w", "closed", 102_000, json!({"pid": 5})), 102_000).unwrap();
        s.update(ev("vscode", "w", "idle", 1000, json!({"pid": 5, "heartbeat": true})), 103_000).unwrap();
        assert!(s.sessions.is_empty(), "an explicit close is final for older events");
    }

    #[test]
    fn restore_restarts_the_lease_of_live_reporters() {
        let saved = json!({"sessions": [{"agent": "vscode", "sessionId": "w", "state": "idle", "updatedAt": 900, "pid": 5,
            "startedAt": 800, "lastSeen": 900, "lastInteraction": 900}]});
        let mut s = store();
        s.restore(&saved, 1_000_000);
        assert!(s.sessions.contains_key("vscode:w"), "kept although its saved last_seen is 16 minutes old");
        s.sweep(1_000_000 + 91_000);
        assert!(s.sessions.is_empty(), "and expires normally if heartbeats do not resume");
    }

    #[test]
    fn restores_the_node_service_file_format() {
        let saved = json!({"sessions": [{"version": 1, "agent": "vscode", "sessionId": "w", "state": "working", "updatedAt": 900, "pid": 5,
            "startedAt": 800, "lastSeen": 950, "lastInteraction": 900, "edition": "insiders"}], "pinned": null});
        let mut s = store();
        s.restore(&saved, 1000);
        assert_eq!(s.sessions["vscode:w"].edition, Some(Edition::Insiders));
    }
}
