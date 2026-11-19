//! Chooses which session the Discord card shows, with hysteresis so it does not flicker.

use crate::store::{Session, SessionStore};
use std::cmp::Ordering;

/// What the user is looking at: the foreground window's process, or a session key a reporter says is focused.
#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub pid: Option<u32>,
    pub key: Option<String>,
}

pub fn is_focused(s: &Session, focus: Option<&Focus>) -> bool {
    let Some(focus) = focus else { return false };
    if let Some(key) = &focus.key {
        return *key == s.key();
    }
    focus.pid.is_some_and(|pid| s.pid == Some(pid) || s.agent_pid == Some(pid) || s.ancestors.contains(&pid))
}

/// Focused first, then active over idle, then the most recent user interaction, then the latest event.
pub fn compare(a: &Session, b: &Session, focus: Option<&Focus>) -> Ordering {
    is_focused(b, focus)
        .cmp(&is_focused(a, focus))
        .then(b.state.active().cmp(&a.state.active()))
        .then(b.last_interaction.cmp(&a.last_interaction))
        .then(b.updated_at.cmp(&a.updated_at))
        .then(a.key().cmp(&b.key()))
}

pub fn rank<'a>(sessions: impl Iterator<Item = &'a Session>, focus: Option<&Focus>) -> Vec<&'a Session> {
    let mut list: Vec<&Session> = sessions.collect();
    list.sort_by(|a, b| compare(a, b, focus));
    list
}

/// - a pin always wins, and unpinning re-ranks immediately;
/// - if the shown session disappears, the next best is shown immediately;
/// - a fresh user interaction switches immediately;
/// - any other change (state flips, focus changes) must stay on top for `settle_ms` first.
pub struct Selector {
    current: Option<String>,
    chosen_at: i64,
    candidate: Option<String>,
    candidate_since: i64,
    was_pinned: bool,
    pub settle_ms: i64,
}

impl Selector {
    pub fn new(settle_ms: i64) -> Self {
        Selector { current: None, chosen_at: 0, candidate: None, candidate_since: 0, was_pinned: false, settle_ms }
    }

    pub fn pick(&mut self, store: &SessionStore, focus: Option<&Focus>, now: i64) -> Option<String> {
        if let Some(pinned) = store.pinned.as_deref().filter(|p| store.sessions.contains_key(*p)) {
            self.was_pinned = true;
            return Some(self.commit(pinned.to_string(), now));
        }
        let ranked = rank(store.sessions.values(), focus);
        let Some(top) = ranked.first() else {
            self.current = None;
            self.candidate = None;
            self.was_pinned = false;
            return None;
        };
        let top_key = top.key();
        let shown = self.current.as_ref().filter(|k| store.sessions.contains_key(*k));
        let unpinned = std::mem::take(&mut self.was_pinned);
        if shown.is_none() || unpinned || Some(&top_key) == self.current.as_ref() || top.last_interaction > self.chosen_at || self.settle_ms <= 0 {
            return Some(self.commit(top_key, now));
        }
        if self.candidate.as_ref() != Some(&top_key) {
            self.candidate = Some(top_key.clone());
            self.candidate_since = now;
        }
        if now - self.candidate_since >= self.settle_ms {
            return Some(self.commit(top_key, now));
        }
        self.current.clone()
    }

    fn commit(&mut self, key: String, now: i64) -> String {
        if self.current.as_ref() != Some(&key) {
            self.current = Some(key.clone());
            self.chosen_at = now;
        }
        self.candidate = None;
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{ev, store};
    use crate::store::AgentProcess;
    use serde_json::json;

    fn top(s: &SessionStore, focus: Option<&Focus>) -> String {
        rank(s.sessions.values(), focus)[0].session_id.clone()
    }

    #[test]
    fn working_outranks_idle_and_pin_overrides() {
        let mut s = store();
        let mut sel = Selector::new(0);
        s.update(ev("codex", "a", "working", 1000, json!({})), 1000).unwrap();
        s.update(ev("codex", "b", "idle", 2000, json!({})), 2000).unwrap();
        assert_eq!(sel.pick(&s, None, 2000).as_deref(), Some("codex:a"));
        s.update(ev("codex", "b", "working", 3000, json!({"interaction": true})), 3000).unwrap();
        s.update(ev("codex", "a", "working", 1000, json!({"heartbeat": true})), 4000).unwrap();
        assert_eq!(sel.pick(&s, None, 4000).as_deref(), Some("codex:b"));
        s.pin(Some("codex:a")).unwrap();
        assert_eq!(sel.pick(&s, None, 4001).as_deref(), Some("codex:a"));
        s.update(ev("codex", "a", "closed", 5000, json!({})), 5000).unwrap();
        assert_eq!(s.pinned, None);
        assert_eq!(sel.pick(&s, None, 5000).as_deref(), Some("codex:b"));
    }

    #[test]
    fn background_agent_does_not_steal_from_the_prompted_session() {
        let mut s = store();
        s.update(ev("claude-code", "bg", "working", 1000, json!({"interaction": true})), 1000).unwrap();
        s.update(ev("codex", "mine", "working", 2000, json!({"interaction": true})), 2000).unwrap();
        s.update(ev("claude-code", "bg", "working", 3000, json!({"continuation": true})), 3000).unwrap();
        assert_eq!(top(&s, None), "mine");
        s.update(ev("claude-code", "bg", "working", 4000, json!({"interaction": true})), 4000).unwrap();
        assert_eq!(top(&s, None), "bg");
    }

    #[test]
    fn focus_by_ancestor_pid_or_key() {
        let mut s = store();
        s.update(ev("claude-code", "term", "idle", 1000, json!({})), 1000).unwrap();
        s.attach("claude-code:term", AgentProcess { agent_pid: 500, agent_name: "claude.exe".into(), ancestors: vec![500, 400, 300] });
        s.update(ev("codex", "other", "working", 2000, json!({"interaction": true})), 2000).unwrap();
        assert_eq!(top(&s, None), "other");
        assert_eq!(top(&s, Some(&Focus { pid: Some(300), key: None })), "term");
        assert_eq!(top(&s, Some(&Focus { pid: Some(999), key: None })), "other");
        assert_eq!(top(&s, Some(&Focus { pid: None, key: Some("claude-code:term".into()) })), "term");
    }

    #[test]
    fn hysteresis() {
        let mut s = store();
        let mut sel = Selector::new(5000);
        let mut pick = |s: &SessionStore, now| sel.pick(s, None, now).map(|k| k.trim_start_matches("codex:").to_string());
        s.update(ev("codex", "a", "working", 1000, json!({"interaction": true})), 1000).unwrap();
        s.update(ev("codex", "b", "idle", 900, json!({})), 1000).unwrap();
        assert_eq!(pick(&s, 1000).as_deref(), Some("a"));
        s.update(ev("codex", "a", "idle", 2000, json!({})), 2000).unwrap();
        s.update(ev("codex", "b", "working", 2001, json!({"continuation": true})), 2001).unwrap();
        assert_eq!(pick(&s, 2001).as_deref(), Some("a"));
        assert_eq!(pick(&s, 6000).as_deref(), Some("a"));
        assert_eq!(pick(&s, 7001).as_deref(), Some("b"));
        s.update(ev("codex", "a", "working", 8000, json!({"interaction": true})), 8000).unwrap();
        assert_eq!(pick(&s, 8000).as_deref(), Some("a"));
        s.pin(Some("codex:b")).unwrap();
        assert_eq!(pick(&s, 8001).as_deref(), Some("b"));
        s.pin(None).unwrap();
        assert_eq!(pick(&s, 8002).as_deref(), Some("a"));
        s.update(ev("codex", "a", "closed", 9000, json!({})), 9000).unwrap();
        assert_eq!(pick(&s, 9000).as_deref(), Some("b"));
    }

    #[test]
    fn simultaneous_agents_keep_their_own_timers() {
        let mut s = store();
        s.update(ev("claude-code", "c1", "working", 1000, json!({"interaction": true})), 1000).unwrap();
        s.update(ev("codex", "x1", "working", 5000, json!({"interaction": true})), 5000).unwrap();
        assert_eq!(top(&s, None), "x1");
        s.update(ev("claude-code", "c1", "working", 9000, json!({"interaction": true})), 9000).unwrap();
        assert_eq!(top(&s, None), "c1");
        assert_eq!(s.sessions["claude-code:c1"].started_at, 1000);
        assert_eq!(s.sessions.len(), 2);
    }
}
