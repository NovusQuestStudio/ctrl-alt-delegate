//! The presence service: applies events, chooses the activity, talks to Discord, persists sessions.

use crate::process::{ancestry, find_agent, ProcessTable};
use crate::protocol::parse_event;
use crate::selector::{Focus, Selector};
use crate::settings::{self, Settings};
use crate::store::{AgentProcess, Alive, SessionStore};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Where the chosen activity goes: Discord in production, a recorder in tests.
pub trait Transport: Send {
    fn set_activity(&mut self, activity: Option<Value>);
    fn status(&self) -> String;
    fn user(&self) -> Option<Value> {
        None
    }
    fn acknowledged(&self) -> Option<bool> {
        None
    }
    /// Clear the activity and disconnect.
    fn stop(&mut self) {}
}

impl Transport for crate::discord::Discord {
    fn set_activity(&mut self, activity: Option<Value>) {
        crate::discord::Discord::set_activity(self, activity)
    }
    fn status(&self) -> String {
        crate::discord::Discord::status(self)
    }
    fn user(&self) -> Option<Value> {
        crate::discord::Discord::user(self)
    }
    fn acknowledged(&self) -> Option<bool> {
        Some(crate::discord::Discord::acknowledged(self))
    }
    fn stop(&mut self) {
        crate::discord::Discord::stop(self)
    }
}

pub type Snapshot = Box<dyn Fn() -> Result<ProcessTable, String> + Send>;

pub struct Options {
    pub settings_path: PathBuf,
    /// Where sessions are saved across restarts; None disables persistence.
    pub state_path: Option<PathBuf>,
    /// How long a non-interactive change must stay on top before the card switches.
    pub settle_ms: i64,
    /// Process table used to find agent processes; None disables lookups.
    pub processes: Option<Snapshot>,
    pub alive: Alive,
}

impl Options {
    pub fn system() -> Options {
        Options {
            settings_path: settings::settings_path(),
            state_path: Some(settings::state_path()),
            settle_ms: 5000,
            processes: Some(Box::new(crate::process::snapshot)),
            alive: Box::new(crate::process::alive),
        }
    }
}

pub struct Core {
    pub store: SessionStore,
    selector: Selector,
    pub settings: Settings,
    settings_error: Option<String>,
    pub paused: bool,
    focus: Option<Focus>,
    transport: Box<dyn Transport>,
    options: Options,
    saved_revision: u64,
    /// Set by a `stop` request; the app exits on its next tick.
    pub quit_requested: bool,
}

impl Core {
    pub fn new(mut options: Options, transport: Box<dyn Transport>, now: i64) -> Core {
        let (settings, settings_error) = match settings::load(&options.settings_path) {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(format!("Invalid settings file; using defaults ({e})"))),
        };
        let alive = std::mem::replace(&mut options.alive, Box::new(|_| true));
        let mut store = SessionStore::new(settings.stale_minutes * 60_000, alive);
        if let Some(path) = &options.state_path {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(saved) = serde_json::from_str::<Value>(&text) {
                    store.restore(&saved, now);
                }
            }
        }
        // A restored pid may now belong to an unrelated program: drop sessions whose process name changed.
        if let Some(snapshot) = options.processes.as_ref().filter(|_| store.sessions.values().any(|s| s.agent_pid.is_some())) {
            if let Ok(table) = snapshot() {
                let mismatched: Vec<(String, i64)> = store
                    .sessions
                    .values()
                    .filter(|s| s.agent_pid.is_some_and(|pid| table.get(&pid).map(|p| &p.name) != s.agent_name.as_ref()))
                    .map(|s| (s.key(), s.updated_at))
                    .collect();
                for (key, at) in mismatched {
                    store.drop_session(&key, at);
                }
            }
        }
        let selector = Selector::new(options.settle_ms);
        let saved_revision = store.revision;
        let mut core = Core { store, selector, settings, settings_error, paused: false, focus: None, transport, options, saved_revision, quit_requested: false };
        core.refresh(now);
        core
    }

    fn reload_settings(&mut self) {
        match settings::load(&self.options.settings_path) {
            Ok(s) => {
                self.store.stale_ms = s.stale_minutes * 60_000;
                self.settings = s;
                self.settings_error = None;
            }
            Err(e) => self.settings_error = Some(format!("Invalid settings file; keeping the last valid configuration ({e})")),
        }
    }

    fn selected(&mut self, now: i64) -> Option<String> {
        self.selector.pick(&self.store, self.focus.as_ref(), now)
    }

    pub fn activity(&mut self, now: i64) -> Option<Value> {
        let key = self.selected(now);
        if self.paused || !self.settings.enabled {
            return None;
        }
        crate::presentation::activity_for(key.and_then(|k| self.store.sessions.get(&k)), self.settings.show_project, &self.settings.large_image)
    }

    fn persist(&mut self) {
        let Some(path) = &self.options.state_path else { return };
        if self.store.revision == self.saved_revision {
            return;
        }
        self.saved_revision = self.store.revision;
        let text = self.store.snapshot().to_string();
        if let Err(e) = settings::write_atomic(path, &text) {
            crate::log::write(&format!("Could not save sessions: {e}"));
        }
    }

    /// Called every second and after every request.
    pub fn refresh(&mut self, now: i64) {
        self.reload_settings();
        self.store.sweep(now);
        let activity = self.activity(now);
        self.transport.set_activity(activity);
        self.persist();
    }

    /// Find the long-lived agent/editor process above a reporter's parent, while that parent is still alive.
    fn attach_agent(&mut self, key: &str, ppid: u32) {
        let Some(snapshot) = &self.options.processes else { return };
        let Ok(table) = snapshot() else { return };
        let chain = ancestry(&table, ppid, 16);
        if let Some(agent) = find_agent(&chain) {
            let start = chain.iter().position(|p| p.pid == agent.pid).unwrap_or(0);
            let process = AgentProcess { agent_pid: agent.pid, agent_name: agent.name.clone(), ancestors: chain[start..].iter().map(|p| p.pid).collect() };
            self.store.attach(key, process);
        }
    }

    pub fn apply_event(&mut self, raw: &Value, now: i64) -> Result<(), String> {
        let event = parse_event(raw, now)?;
        let ppid = event.ppid;
        if let Some(key) = self.store.update(event, now)? {
            if let Some(ppid) = ppid {
                if self.store.sessions.get(&key).is_some_and(|s| s.agent_pid.is_none()) {
                    self.attach_agent(&key, ppid);
                }
            }
        }
        Ok(())
    }

    pub fn set_focus(&mut self, focus: Option<Focus>) {
        self.focus = focus;
    }

    pub fn save_toggles(&mut self, patch: &Value) -> Result<(), String> {
        self.settings = settings::save_toggles(&self.options.settings_path, patch)?;
        Ok(())
    }

    /// Handle one IPC request line and return the JSON reply.
    pub fn handle(&mut self, line: &str, now: i64) -> String {
        let reply = (|| -> Result<Value, String> {
            let message: Value = serde_json::from_str(line).map_err(|_| "Invalid JSON".to_string())?;
            let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
            self.store.sweep(now);
            match kind {
                "event" => self.apply_event(message.get("event").unwrap_or(&Value::Null), now)?,
                "pin" => self.store.pin(message.get("key").and_then(Value::as_str))?,
                "pause" => self.paused = message.get("paused") == Some(&json!(true)),
                "settings" => self.save_toggles(message.get("patch").unwrap_or(&Value::Null))?,
                "focus" => {
                    let pid = match message.get("pid") {
                        None | Some(Value::Null) => None,
                        Some(p) => Some(p.as_u64().filter(|p| *p > 0 && *p <= u32::MAX as u64).ok_or("Invalid focus pid")? as u32),
                    };
                    let key = match message.get("key") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(k)) if k.len() <= 300 => Some(k.clone()),
                        _ => return Err("Invalid focus key".into()),
                    };
                    self.focus = (pid.is_some() || key.is_some()).then_some(Focus { pid, key });
                }
                "stop" => self.quit_requested = true,
                "status" => {}
                _ => return Err("Unknown request type".into()),
            }
            self.refresh(now);
            Ok(if kind == "status" { self.status(now) } else { json!({"ok": true}) })
        })();
        match reply {
            Ok(v) => v.to_string(),
            Err(e) => json!({"ok": false, "error": e}).to_string(),
        }
    }

    pub fn status(&mut self, now: i64) -> Value {
        let selected = self.selected(now);
        let activity = self.activity(now);
        let sessions: Vec<Value> = self
            .store
            .sessions
            .values()
            .map(|s| json!({"key": s.key(), "agent": s.agent.as_str(), "edition": s.edition.map(|e| e.as_str()), "state": s.state.as_str(),
                "project": s.project, "startedAt": s.started_at, "updatedAt": s.updated_at, "lastSeen": s.last_seen,
                "lastInteraction": s.last_interaction, "agentPid": s.agent_pid, "agentName": s.agent_name}))
            .collect();
        json!({
            "ok": true, "discord": self.transport.status(), "profile": self.transport.user(), "activityAcknowledged": self.transport.acknowledged(),
            "enabled": self.settings.enabled, "showProject": self.settings.show_project, "paused": self.paused, "pinned": self.store.pinned,
            "focus": self.focus.as_ref().map(|f| json!({"pid": f.pid, "key": f.key})), "pid": std::process::id(),
            "settingsError": self.settings_error, "activity": activity, "selected": selected, "sessions": sessions,
            "version": env!("CARGO_PKG_VERSION"),
        })
    }

    /// Swap the Discord connection (tray "Reconnect to Discord").
    pub fn replace_transport(&mut self, transport: Box<dyn Transport>) {
        self.transport.stop();
        self.transport = transport;
    }

    /// Save sessions and clear the Discord activity.
    pub fn shutdown(&mut self) {
        self.persist();
        self.transport.set_activity(None);
        self.transport.stop();
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::process::ProcessInfo;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct Recorder(pub Arc<Mutex<Vec<Option<Value>>>>);
    impl Transport for Recorder {
        fn set_activity(&mut self, activity: Option<Value>) {
            let mut list = self.0.lock().unwrap();
            if list.last() != Some(&activity) {
                list.push(activity);
            }
        }
        fn status(&self) -> String {
            "test".into()
        }
    }
    impl Recorder {
        pub fn last(&self) -> Option<Value> {
            self.0.lock().unwrap().last().cloned().flatten()
        }
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cad-core-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn options(dir: &std::path::Path) -> Options {
        Options { settings_path: dir.join("config.json"), state_path: None, settle_ms: 0, processes: None, alive: Box::new(|_| true) }
    }

    fn event(agent: &str, id: &str, state: &str, at: i64, extra: Value) -> String {
        let mut e = json!({"version": 1, "agent": agent, "sessionId": id, "state": state, "updatedAt": at});
        for (k, v) in extra.as_object().unwrap() {
            e[k] = v.clone();
        }
        json!({"type": "event", "event": e}).to_string()
    }

    fn ok(reply: &str) -> Value {
        let v: Value = serde_json::from_str(reply).unwrap();
        assert_eq!(v["ok"], true, "{reply}");
        v
    }

    #[test]
    fn switching_privacy_pause_pin_settings_and_closure() {
        let dir = temp_dir("flow");
        let rec = Recorder::default();
        let mut core = Core::new(options(&dir), Box::new(rec.clone()), 1000);
        let cwd = json!({"project": "secret-project"});
        ok(&core.handle(&event("claude-code", "c", "working", 1000, json!({"interaction": true, "project": "secret-project"})), 1000));
        assert_eq!(rec.last().unwrap()["details"], "Claude Code · Working");
        ok(&core.handle(&event("codex", "x", "working", 2000, json!({"interaction": true})), 2000));
        assert_eq!(rec.last().unwrap()["details"], "Codex · Working");
        ok(&core.handle(&event("codex", "x", "idle", 3000, json!({})), 3000));
        assert_eq!(rec.last().unwrap()["details"], "Claude Code · Working", "a working agent outranks an idle one");
        ok(&core.handle(&json!({"type": "pin", "key": "codex:x"}).to_string(), 3001));
        assert_eq!(rec.last().unwrap()["details"], "Codex · Ready for a prompt");
        ok(&core.handle(&json!({"type": "pin", "key": null}).to_string(), 3002));
        ok(&core.handle(&json!({"type": "pause", "paused": true}).to_string(), 3003));
        assert_eq!(rec.last(), None);
        assert_eq!(ok(&core.handle(r#"{"type":"status"}"#, 3004))["paused"], true);
        ok(&core.handle(&json!({"type": "pause", "paused": false}).to_string(), 3005));
        ok(&core.handle(&json!({"type": "settings", "patch": {"showProject": true}}).to_string(), 3006));
        assert_eq!(rec.last().unwrap()["state"], "secret-project");
        ok(&core.handle(&json!({"type": "settings", "patch": {"showProject": false, "enabled": false}}).to_string(), 3007));
        assert_eq!(rec.last(), None);
        ok(&core.handle(&json!({"type": "settings", "patch": {"enabled": true}}).to_string(), 3008));
        ok(&core.handle(&event("claude-code", "c", "closed", 4000, json!({})), 4000));
        ok(&core.handle(&event("codex", "x", "closed", 4001, json!({})), 4001));
        assert_eq!(rec.last(), None);
        let bad: Value = serde_json::from_str(&core.handle(&json!({"type": "event", "event": {"version": 5}}).to_string(), 5000)).unwrap();
        assert_eq!(bad["error"], "Unsupported protocol or agent");
        let published: Vec<Value> = rec.0.lock().unwrap().iter().flatten().filter(|a| a["state"] != "secret-project").cloned().collect();
        assert!(!json!(published).to_string().contains("secret-project"));
        let _ = cwd;
    }

    #[test]
    fn agent_process_is_attached_once_and_its_exit_clears_the_session() {
        let dir = temp_dir("agent");
        let lookups = Arc::new(Mutex::new(0));
        let alive = Arc::new(Mutex::new(true));
        let (l, a) = (lookups.clone(), alive.clone());
        let mut opts = options(&dir);
        opts.processes = Some(Box::new(move || {
            *l.lock().unwrap() += 1;
            Ok([(10, 20, "bash.exe"), (20, 30, "claude.exe"), (30, 1, "WindowsTerminal.exe")].iter().map(|(p, pp, n)| (*p, ProcessInfo { pid: *p, ppid: *pp, name: n.to_string() })).collect())
        }));
        opts.alive = Box::new(move |pid| pid != 20 || *a.lock().unwrap());
        let mut core = Core::new(opts, Box::new(Recorder::default()), 1000);
        ok(&core.handle(&event("claude-code", "s", "working", 1000, json!({"ppid": 10})), 1000));
        let s = &core.store.sessions["claude-code:s"];
        assert_eq!((s.agent_pid, s.agent_name.as_deref(), s.ancestors.clone()), (Some(20), Some("claude.exe"), vec![20, 30]));
        ok(&core.handle(&event("claude-code", "s", "idle", 2000, json!({"ppid": 10})), 2000));
        assert_eq!(*lookups.lock().unwrap(), 1, "looked up once per session");
        ok(&core.handle(&json!({"type": "focus", "pid": 30}).to_string(), 2001));
        assert_eq!(ok(&core.handle(r#"{"type":"status"}"#, 2002))["focus"]["pid"], 30);
        *alive.lock().unwrap() = false;
        core.refresh(2003);
        assert!(core.store.sessions.is_empty());
        for bad in [json!({"type": "focus", "pid": -1}), json!({"type": "focus", "key": 42}), json!({"type": "nope"})] {
            assert_eq!(serde_json::from_str::<Value>(&core.handle(&bad.to_string(), 3000)).unwrap()["ok"], false);
        }
    }

    #[test]
    fn restart_keeps_timers_and_rejects_reused_pids() {
        let dir = temp_dir("restart");
        let named = |name: &'static str| -> Snapshot { Box::new(move || Ok([(10, 20, "bash.exe"), (20, 1, name)].iter().map(|(p, pp, n)| (*p, ProcessInfo { pid: *p, ppid: *pp, name: n.to_string() })).collect())) };
        let opts = |name| { let mut o = options(&dir); o.state_path = Some(dir.join("sessions.json")); o.processes = Some(named(name)); o };
        let mut first = Core::new(opts("codex.exe"), Box::new(Recorder::default()), 100_000);
        ok(&first.handle(&event("codex", "keep", "working", 40_000, json!({"interaction": true, "ppid": 10})), 100_000));
        ok(&first.handle(&event("claude-code", "hookonly", "idle", 40_000, json!({})), 100_000));
        ok(&first.handle(&json!({"type": "pin", "key": "codex:keep"}).to_string(), 100_000));
        first.shutdown();
        let rec = Recorder::default();
        let mut second = Core::new(opts("codex.exe"), Box::new(rec.clone()), 101_000);
        let status = second.status(101_000);
        assert_eq!(status["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(status["pinned"], "codex:keep");
        assert_eq!(rec.last().unwrap()["timestamps"]["start"], 40);
        second.shutdown();
        let third = Core::new(opts("notepad.exe"), Box::new(Recorder::default()), 102_000);
        assert_eq!(third.store.sessions.keys().collect::<Vec<_>>(), vec!["claude-code:hookonly"]);
    }

    #[test]
    fn invalid_settings_keep_the_last_valid_configuration() {
        let dir = temp_dir("settings");
        std::fs::write(dir.join("config.json"), r#"{"showProject": true}"#).unwrap();
        let rec = Recorder::default();
        let mut core = Core::new(options(&dir), Box::new(rec.clone()), 1000);
        std::fs::write(dir.join("config.json"), "{broken").unwrap();
        ok(&core.handle(&event("codex", "s", "working", 1000, json!({"project": "kept"})), 1000));
        let status = ok(&core.handle(r#"{"type":"status"}"#, 1001));
        assert!(status["settingsError"].as_str().unwrap().contains("Invalid settings"));
        assert_eq!(status["activity"]["state"], "kept");
    }
}
