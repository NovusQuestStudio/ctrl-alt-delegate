//! Talks to the Node presence service over its local pipe, on a worker thread.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

pub enum Command {
    Pause(bool),
    Setting(&'static str, bool),
    Pin(Option<String>),
    Start,
    Restart,
    Quit,
}

pub struct Update {
    pub status: Option<Value>,
    pub error: Option<String>,
}

pub struct Service {
    node: String,
    cli: Option<PathBuf>,
    pipe: Option<String>,
    last_focus: Option<(u32, Instant)>,
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden(command: &mut Process) -> &mut Process {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Find `dist/cli.js` of this checkout: `CAD_CLI`, or walk up from the executable.
fn find_cli() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CAD_CLI") {
        return Some(PathBuf::from(path));
    }
    let exe = std::env::current_exe().ok()?;
    let mut dir: Option<&Path> = exe.parent();
    while let Some(d) = dir {
        let cli = d.join("dist").join("cli.js");
        let package = std::fs::read_to_string(d.join("package.json")).unwrap_or_default();
        if cli.is_file() && package.contains("\"name\": \"ctrl-alt-delegate\"") {
            return Some(cli);
        }
        dir = d.parent();
    }
    None
}

impl Service {
    pub fn discover() -> Self {
        Service { node: std::env::var("CAD_NODE").unwrap_or_else(|_| "node".into()), cli: find_cli(), pipe: None, last_focus: None }
    }

    fn cli(&self, args: &[&str]) -> Result<String, String> {
        let cli = self.cli.as_ref().ok_or("dist/cli.js not found; run npm run build")?;
        let output = hidden(Process::new(&self.node).arg(cli).args(args)).output().map_err(|e| format!("cannot run node: {e}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().lines().last().unwrap_or("service command failed").to_string())
        }
    }

    fn pipe(&mut self) -> Result<String, String> {
        if self.pipe.is_none() {
            self.pipe = Some(match std::env::var("CAD_SOCKET") {
                Ok(path) => path,
                Err(_) => self.cli(&["socket-path"])?,
            });
        }
        Ok(self.pipe.clone().unwrap())
    }

    /// One request/response over the local pipe. The service replies with a single JSON line.
    fn request(&mut self, message: Value) -> Result<Value, String> {
        let path = self.pipe()?;
        let mut stream = open(&path)?;
        stream.write_all(format!("{message}\n").as_bytes()).map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(stream).take_line(&mut line)?;
        let value: Value = serde_json::from_str(&line).map_err(|_| "invalid service response".to_string())?;
        if value.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(value)
        } else {
            Err(value.get("error").and_then(Value::as_str).unwrap_or("request failed").to_string())
        }
    }

    fn start(&mut self) -> Result<(), String> {
        self.cli(&["start", "--background"]).map(|_| ())
    }

    fn handle(&mut self, command: Command) -> bool {
        let result = match command {
            Command::Pause(paused) => self.request(json!({"type": "pause", "paused": paused})).map(|_| ()),
            Command::Setting(key, value) => self.request(json!({"type": "settings", "patch": {key: value}})).map(|_| ()),
            Command::Pin(key) => self.request(json!({"type": "pin", "key": key})).map(|_| ()),
            Command::Start => self.start(),
            Command::Restart => self.cli(&["stop"]).and_then(|_| self.start()),
            Command::Quit => {
                let _ = self.cli(&["stop"]);
                return false;
            }
        };
        if let Err(error) = result {
            eprintln!("{error}");
        }
        true
    }

    /// Report the foreground window's process when it changes (and every 30 s, so a restarted service learns it).
    fn report_focus(&mut self) {
        let Some(pid) = foreground_pid() else { return };
        if pid == std::process::id() {
            return;
        }
        let due = match self.last_focus {
            Some((last, at)) => last != pid || at.elapsed() > Duration::from_secs(30),
            None => true,
        };
        if due && self.request(json!({"type": "focus", "pid": pid})).is_ok() {
            self.last_focus = Some((pid, Instant::now()));
        }
    }

    pub fn run(mut self, commands: Receiver<Command>, updates: Sender<Update>) {
        // Start the service if it is not running yet.
        if self.request(json!({"type": "status"})).is_err() {
            if let Err(error) = self.start() {
                let _ = updates.send(Update { status: None, error: Some(error) });
            }
        }
        let mut last_status = Instant::now() - Duration::from_secs(10);
        loop {
            match commands.recv_timeout(Duration::from_millis(400)) {
                Ok(command) => {
                    if !self.handle(command) {
                        return;
                    }
                    last_status = Instant::now() - Duration::from_secs(10);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            self.report_focus();
            if last_status.elapsed() >= Duration::from_millis(900) {
                last_status = Instant::now();
                let update = match self.request(json!({"type": "status"})) {
                    Ok(status) => Update { status: Some(status), error: None },
                    Err(error) => Update { status: None, error: Some(error) },
                };
                self.last_focus = self.last_focus.filter(|_| update.status.is_some());
                if updates.send(update).is_err() {
                    return;
                }
            }
        }
    }
}

trait TakeLine {
    fn take_line(self, line: &mut String) -> Result<(), String>;
}
impl<R: std::io::Read> TakeLine for BufReader<R> {
    fn take_line(mut self, line: &mut String) -> Result<(), String> {
        let mut limited = (&mut self).take(256 * 1024);
        limited.read_line(line).map_err(|e| e.to_string())?;
        if line.ends_with('\n') { Ok(()) } else { Err("service closed the connection".into()) }
    }
}
use std::io::Read as _;

#[cfg(windows)]
fn open(path: &str) -> Result<std::fs::File, String> {
    // All pipe instances can be briefly busy; retry for up to ~1 s.
    for _ in 0..50 {
        match std::fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => return Ok(file),
            Err(error) if error.raw_os_error() == Some(231) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("service pipe is busy".into())
}

#[cfg(unix)]
fn open(path: &str) -> Result<std::os::unix::net::UnixStream, String> {
    std::os::unix::net::UnixStream::connect(path).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        GetWindowThreadProcessId(window, Some(&mut pid));
    }
    (pid != 0).then_some(pid)
}

#[cfg(not(windows))]
fn foreground_pid() -> Option<u32> {
    None // macOS: NSWorkspace frontmost application, in a later stage.
}

/// Only one tray per user session.
#[cfg(windows)]
pub fn single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    unsafe {
        match CreateMutexW(None, true, w!("Local\\CtrlAltDelegateTray")) {
            // The handle is intentionally kept open for the life of the process.
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => false,
        }
    }
}

#[cfg(not(windows))]
pub fn single_instance() -> bool {
    true
}

/// Win32 message loop (the tray icon needs one on its thread), calling `tick` on a timer until it returns false.
#[cfg(windows)]
pub fn run_loop(interval: Duration, mut tick: impl FnMut() -> bool) {
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, KillTimer, SetTimer, TranslateMessage, MSG, WM_TIMER};
    unsafe {
        let timer = SetTimer(None, 0, interval.as_millis() as u32, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            if msg.message == WM_TIMER && !tick() {
                break;
            }
        }
        let _ = KillTimer(None, timer);
    }
}

#[cfg(not(windows))]
pub fn run_loop(interval: Duration, mut tick: impl FnMut() -> bool) {
    while tick() {
        std::thread::sleep(interval);
    }
}
