//! Discord Rich Presence over the local Discord desktop client's IPC pipe. No network, tokens, or secrets.

use crate::ipc::Conn;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn frame(op: u32, body: &Value) -> Vec<u8> {
    let data = body.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}

impl Decoder {
    /// Split a byte stream into frames. Refuses frames over 1 MiB.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<(u32, Value)>, String> {
        self.buffer.extend_from_slice(data);
        let mut frames = Vec::new();
        while self.buffer.len() >= 8 {
            let op = u32::from_le_bytes(self.buffer[0..4].try_into().unwrap());
            let len = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
            if len > 1024 * 1024 {
                return Err("Discord frame too large".into());
            }
            if self.buffer.len() < 8 + len {
                break;
            }
            let body = serde_json::from_slice(&self.buffer[8..8 + len]).map_err(|e| e.to_string())?;
            self.buffer.drain(..8 + len);
            frames.push((op, body));
        }
        Ok(frames)
    }
}

pub fn discord_paths() -> Vec<String> {
    if let Ok(path) = std::env::var("DISCORD_IPC_PATH") {
        return vec![path];
    }
    if cfg!(windows) {
        return (0..10).map(|n| format!(r"\\.\pipe\discord-ipc-{n}")).collect();
    }
    let mut prefixes: Vec<String> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(|k| std::env::var(k).ok()).collect();
    prefixes.push("/tmp".into());
    prefixes.dedup();
    prefixes.iter().flat_map(|p| (0..10).map(move |n| format!("{}/discord-ipc-{n}", p.trim_end_matches('/')))).collect()
}

#[derive(Default)]
struct Shared {
    desired: Option<Value>,
    status: String,
    user: Option<Value>,
    ready: bool,
    last_sent: Option<String>,
}

/// Keeps one connection to Discord on a background thread and publishes the latest desired activity.
pub struct Discord {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static NONCE: AtomicU64 = AtomicU64::new(1);
fn nonce() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{t:x}-{}", NONCE.fetch_add(1, Ordering::Relaxed))
}

impl Discord {
    pub fn start(client_id: String, paths: Vec<String>, interval: Duration, log: fn(&str)) -> Discord {
        let shared = Arc::new(Mutex::new(Shared { status: "disconnected".into(), ..Default::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, flag) = (shared.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("discord".into())
            .spawn(move || run(client_id, paths, interval, s, flag, log))
            .expect("spawn discord thread");
        Discord { shared, stop, thread: Some(thread) }
    }
    pub fn set_activity(&self, activity: Option<Value>) {
        self.shared.lock().unwrap().desired = activity;
    }
    pub fn status(&self) -> String {
        self.shared.lock().unwrap().status.clone()
    }
    pub fn user(&self) -> Option<Value> {
        self.shared.lock().unwrap().user.clone()
    }
    /// Discord has confirmed the latest desired activity.
    pub fn acknowledged(&self) -> bool {
        let s = self.shared.lock().unwrap();
        s.ready && s.last_sent.as_deref() == Some(serialize(&s.desired).as_str())
    }
    /// Clear the activity (best effort) and close the connection.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Discord {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serialize(activity: &Option<Value>) -> String {
    activity.as_ref().map_or("null".into(), Value::to_string)
}

fn run(client_id: String, paths: Vec<String>, interval: Duration, shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>, log: fn(&str)) {
    let mut index = 0usize;
    let mut backoff = Duration::from_secs(1);
    while !stop.load(Ordering::SeqCst) {
        if let Ok(conn) = Conn::open(&paths[index]) {
            if session(conn, &client_id, interval, &shared, &stop, log) {
                backoff = Duration::from_secs(1);
            }
        }
        {
            let mut s = shared.lock().unwrap();
            s.ready = false;
            if s.status != "error" {
                s.status = "disconnected".into();
            }
        }
        index = (index + 1) % paths.len();
        let delay = if index == 0 {
            let d = backoff;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            d
        } else {
            Duration::from_millis(10)
        };
        let until = Instant::now() + delay;
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// One connection. Returns true if the handshake completed.
fn session(mut conn: Conn, client_id: &str, interval: Duration, shared: &Mutex<Shared>, stop: &AtomicBool, log: fn(&str)) -> bool {
    if conn.write_all(&frame(0, &json!({"v": 1, "client_id": client_id}))).is_err() {
        return false;
    }
    let started = Instant::now();
    let mut decoder = Decoder::default();
    let mut buf = [0u8; 8192];
    let mut ready = false;
    let mut pending: Option<(String, String, Instant)> = None;
    let mut last_write: Option<Instant> = None;
    loop {
        if stop.load(Ordering::SeqCst) {
            if ready {
                let _ = conn.write_all(&frame(1, &json!({"cmd": "SET_ACTIVITY", "args": {"pid": std::process::id(), "activity": null}, "nonce": nonce()})));
                std::thread::sleep(Duration::from_millis(100));
            }
            return ready;
        }
        let n = match conn.read_available(&mut buf) {
            Ok(n) => n,
            Err(_) => return ready,
        };
        let frames = match decoder.feed(&buf[..n]) {
            Ok(f) => f,
            Err(_) => return ready,
        };
        for (op, body) in frames {
            match op {
                3 => {
                    if conn.write_all(&frame(4, &body)).is_err() {
                        return ready;
                    }
                }
                2 => return ready,
                1 => {
                    let evt = body.get("evt").and_then(Value::as_str);
                    if evt == Some("READY") {
                        let user = body.pointer("/data/user");
                        let profile = user.filter(|u| u.get("id").and_then(Value::as_str).is_some() && u.get("username").and_then(Value::as_str).is_some()).map(|u| {
                            // Only public profile fields are kept (never email or other account data).
                            let mut p = json!({"id": u["id"], "username": u["username"]});
                            if let Some(g) = u.get("global_name").and_then(Value::as_str) { p["globalName"] = json!(g); }
                            if let Some(a) = u.get("avatar").and_then(Value::as_str) { p["avatar"] = json!(a); }
                            p
                        });
                        let mut s = shared.lock().unwrap();
                        s.user = profile;
                        s.ready = true;
                        s.status = "connected".into();
                        s.last_sent = None;
                        ready = true;
                        last_write = None;
                    } else if evt == Some("ERROR") {
                        log(&format!("Discord RPC error: {}", body.get("data").map(Value::to_string).unwrap_or_default()));
                        shared.lock().unwrap().status = "error".into();
                        return ready;
                    } else if let Some((nonce, serialized, _)) = &pending {
                        if body.get("nonce").and_then(Value::as_str) == Some(nonce.as_str()) {
                            shared.lock().unwrap().last_sent = Some(serialized.clone());
                            pending = None;
                        }
                    }
                }
                _ => {}
            }
        }
        if !ready && started.elapsed() > Duration::from_secs(3) {
            return false;
        }
        if ready {
            if let Some((_, _, at)) = &pending {
                if at.elapsed() > Duration::from_secs(10) {
                    return ready; // Reconnect; the latest activity is re-sent after READY.
                }
            } else {
                let (desired, serialized, sent) = {
                    let s = shared.lock().unwrap();
                    (s.desired.clone(), serialize(&s.desired), s.last_sent.clone())
                };
                // Discord allows about 5 updates per 20 s; send at most one per interval.
                if sent.as_deref() != Some(serialized.as_str()) && last_write.is_none_or(|t| t.elapsed() >= interval) {
                    let id = nonce();
                    let message = json!({"cmd": "SET_ACTIVITY", "args": {"pid": std::process::id(), "activity": desired}, "nonce": id});
                    if conn.write_all(&frame(1, &message)).is_err() {
                        return ready;
                    }
                    last_write = Some(Instant::now());
                    pending = Some((id, serialized, Instant::now()));
                }
            }
        }
        if n == 0 {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::tests::test_path;

    #[test]
    fn decoder_handles_split_and_joined_frames_and_refuses_huge_ones() {
        let mut d = Decoder::default();
        let one = frame(1, &json!({"evt": "READY"}));
        let two = frame(3, &json!({"nonce": "ping"}));
        assert!(d.feed(&one[..5]).unwrap().is_empty());
        let mut rest = one[5..].to_vec();
        rest.extend_from_slice(&two);
        assert_eq!(d.feed(&rest).unwrap(), vec![(1, json!({"evt": "READY"})), (3, json!({"nonce": "ping"}))]);
        let mut bad = vec![0u8; 8];
        bad[4..8].copy_from_slice(&(1u32 << 30).to_le_bytes());
        assert!(Decoder::default().feed(&bad).is_err());
    }

    /// A fake Discord: completes the handshake, pings, acknowledges activities, and can drop connections.
    #[test]
    fn handshake_ack_ping_coalescing_reconnect_and_clear() {
        use std::sync::mpsc;
        let path = test_path("discord");
        let (tx, rx) = mpsc::channel::<(u32, Value)>();
        let listener_path = path.clone();
        let drop_after = Arc::new(AtomicBool::new(false));
        let drop_flag = drop_after.clone();
        let fake = std::thread::spawn(move || {
            let mut listener = crate::ipc::tests::raw_listener(&listener_path);
            for _ in 0..2 {
                let mut conn = listener();
                let mut d = Decoder::default();
                let mut buf = [0u8; 4096];
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(8) {
                    let n = match conn.read_available(&mut buf) { Ok(n) => n, Err(_) => break };
                    for (op, body) in d.feed(&buf[..n]).unwrap() {
                        if op == 0 {
                            let ready = frame(1, &json!({"evt": "READY", "data": {"user": {"id": "123456789012345678", "username": "presence-test", "email": "must-not-keep@example.com"}}}));
                            conn.write_all(&ready[..3]).unwrap();
                            conn.write_all(&ready[3..]).unwrap();
                            conn.write_all(&frame(3, &json!({"nonce": "alive"}))).unwrap();
                        }
                        if body.get("cmd").and_then(Value::as_str) == Some("SET_ACTIVITY") {
                            conn.write_all(&frame(1, &json!({"cmd": "SET_ACTIVITY", "nonce": body["nonce"], "data": null}))).unwrap();
                        }
                        let _ = tx.send((op, body));
                    }
                    if drop_flag.swap(false, Ordering::SeqCst) {
                        break; // Simulate Discord closing the pipe.
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let mut client = Discord::start("1572816615429640333".into(), vec![path], Duration::from_millis(50), |_| {});
        let activity = json!({"details": "Codex · Working", "state": "a"});
        client.set_activity(Some(activity.clone()));
        let mut seen = Vec::new();
        let wait = |pred: &dyn Fn(&[(u32, Value)]) -> bool, seen: &mut Vec<(u32, Value)>| {
            let deadline = Instant::now() + Duration::from_secs(6);
            while !pred(seen) {
                assert!(Instant::now() < deadline, "timed out; saw {seen:?}");
                if let Ok(m) = rx.recv_timeout(Duration::from_millis(50)) { seen.push(m); }
            }
        };
        wait(&|s| s.iter().any(|(op, b)| *op == 0 && b["client_id"] == "1572816615429640333"), &mut seen);
        wait(&|s| s.iter().any(|(op, b)| *op == 4 && b["nonce"] == "alive"), &mut seen);
        wait(&|s| s.iter().any(|(_, b)| b["args"]["activity"]["state"] == "a"), &mut seen);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !client.acknowledged() { assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10)); }
        assert_eq!(client.user(), Some(json!({"id": "123456789012345678", "username": "presence-test"})));
        client.set_activity(Some(json!({"state": "b"})));
        client.set_activity(Some(json!({"state": "c"})));
        wait(&|s| s.iter().any(|(_, b)| b["args"]["activity"]["state"] == "c"), &mut seen);
        assert!(!seen.iter().any(|(_, b)| b["args"]["activity"]["state"] == "b"), "rapid updates are coalesced");
        drop_after.store(true, Ordering::SeqCst);
        wait(&|s| s.iter().filter(|(op, _)| *op == 0).count() >= 2, &mut seen);
        wait(&|s| s.iter().filter(|(_, b)| b["args"]["activity"]["state"] == "c").count() >= 2, &mut seen);
        client.stop();
        wait(&|s| s.iter().any(|(_, b)| b.get("cmd").is_some() && b["args"]["activity"].is_null()), &mut seen);
        let _ = fake.join();
    }
}
