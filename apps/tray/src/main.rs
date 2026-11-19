//! Ctrl Alt Delegate tray app: runs the presence service in-process and shows its status and controls.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cad::core::{Core, Options};
use cad::discord::{discord_paths, Discord};
use cad::ipc::{service_path, Server};
use cad::selector::Focus;
use cad::{autostart, log, now_ms, APP_NAME, DISCORD_CLIENT_ID};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const ICON: &[u8] = include_bytes!("../assets/icon-32.rgba");

fn icon(dimmed: bool) -> Icon {
    let mut rgba = ICON.to_vec();
    if dimmed {
        for px in rgba.chunks_exact_mut(4) {
            let grey = ((px[0] as u32 * 30 + px[1] as u32 * 59 + px[2] as u32 * 11) / 100) as u8;
            px[0] = grey;
            px[1] = grey;
            px[2] = grey;
            px[3] /= 2;
        }
    }
    Icon::from_rgba(rgba, 32, 32).expect("valid 32x32 RGBA icon")
}

/// Everything the menu shows, derived from one status snapshot.
#[derive(Default, PartialEq, Clone)]
struct View {
    running: bool,
    discord: String,
    showing: String,
    enabled: bool,
    paused: bool,
    show_project: bool,
    autostart: bool,
    pinned: Option<String>,
    sessions: Vec<(String, String)>,
    error: Option<String>,
}

fn label<'a>(agent: &'a str, edition: Option<&'a str>) -> &'a str {
    match (agent, edition.and_then(cad::protocol::Edition::parse)) {
        ("vscode", Some(e)) => e.label(),
        _ => cad::protocol::Agent::parse(agent).map_or("Unknown", |a| a.label()),
    }
}

fn view(status: &Value, error: Option<&str>) -> View {
    let text = |key: &str| status.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let flag = |key: &str| status.get(key).and_then(Value::as_bool).unwrap_or(false);
    let showing = status.pointer("/activity/details").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| "Nothing".into());
    let mut sessions: Vec<(String, String)> = status
        .get("sessions")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    let key = s.get("key")?.as_str()?.to_string();
                    let agent = label(s.get("agent")?.as_str()?, s.get("edition").and_then(Value::as_str));
                    let state = s.get("state")?.as_str()?;
                    // The project name stays on this machine: the tray is local UI, not Discord.
                    let project = s.get("project").and_then(Value::as_str).map(|p| format!(" — {p}")).unwrap_or_default();
                    Some((key, format!("{agent} · {state}{project}")))
                })
                .collect()
        })
        .unwrap_or_default();
    sessions.sort();
    let error = error.map(str::to_string).or_else(|| status.get("settingsError").and_then(Value::as_str).map(str::to_string));
    View {
        running: status.get("ok") == Some(&json!(true)),
        discord: text("discord"),
        showing,
        enabled: flag("enabled"),
        paused: flag("paused"),
        show_project: flag("showProject"),
        autostart: autostart::enabled(),
        pinned: status.get("pinned").and_then(Value::as_str).map(str::to_string),
        sessions,
        error,
    }
}

struct Ui {
    tray: TrayIcon,
    discord: MenuItem,
    showing: MenuItem,
    error: MenuItem,
    share: CheckMenuItem,
    pause: CheckMenuItem,
    project: CheckMenuItem,
    startup: CheckMenuItem,
    pin: Submenu,
    shown: Option<View>,
}

impl Ui {
    fn apply(&mut self, next: View) {
        if self.shown.as_ref() == Some(&next) {
            return;
        }
        self.discord.set_text(match next.discord.as_str() {
            "connected" => "Discord: connected".to_string(),
            "" => "Discord: not started".to_string(),
            other => format!("Discord: {other} (is Discord open?)"),
        });
        self.showing.set_text(if next.paused {
            "Showing: nothing (paused)".to_string()
        } else if !next.enabled {
            "Showing: nothing (sharing off)".to_string()
        } else {
            format!("Showing: {}", next.showing)
        });
        self.error.set_text(next.error.as_deref().map(|e| format!("⚠ {}", e.chars().take(80).collect::<String>())).unwrap_or_default());
        self.error.set_enabled(false);
        for item in [&self.share, &self.pause, &self.project] {
            item.set_enabled(next.running);
        }
        self.pin.set_enabled(next.running);
        self.share.set_checked(next.enabled);
        self.pause.set_checked(next.paused);
        self.project.set_checked(next.show_project);
        self.startup.set_checked(next.autostart);
        if self.shown.as_ref().is_none_or(|s| s.sessions != next.sessions || s.pinned != next.pinned) {
            while self.pin.remove_at(0).is_some() {}
            let _ = self.pin.append(&CheckMenuItem::with_id("pin:", "Automatic", true, next.pinned.is_none(), None));
            if !next.sessions.is_empty() {
                let _ = self.pin.append(&PredefinedMenuItem::separator());
            }
            for (key, text) in &next.sessions {
                let _ = self.pin.append(&CheckMenuItem::with_id(format!("pin:{key}"), text, true, next.pinned.as_deref() == Some(key), None));
            }
        }
        let dimmed = !next.running || next.paused || !next.enabled || next.discord != "connected";
        let _ = self.tray.set_icon(Some(icon(dimmed)));
        let tip = if !next.running {
            format!("{APP_NAME} — not running")
        } else if next.paused {
            format!("{APP_NAME} — paused")
        } else {
            format!("{APP_NAME} — {}", next.showing)
        };
        let _ = self.tray.set_tooltip(Some(tip));
        self.shown = Some(next);
    }
}

fn open_in_notepad(path: &std::path::Path) {
    let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
}

fn new_discord() -> Discord {
    Discord::start(DISCORD_CLIENT_ID.into(), discord_paths(), Duration::from_secs(5), log::write)
}

fn main() {
    if !single_instance() {
        return;
    }
    log::write(&format!("{APP_NAME} {} starting", env!("CARGO_PKG_VERSION")));
    let core = Arc::new(Mutex::new(Core::new(Options::system(), Box::new(new_discord()), now_ms())));
    let handler_core = core.clone();
    let mut startup_error: Option<String> = None;
    let mut server = match Server::start(&service_path(), Arc::new(move |line| handler_core.lock().unwrap().handle(line, now_ms()))) {
        Ok(s) => Some(s),
        Err(e) => {
            // Most likely an older Node version of the service is still running on the same pipe.
            let message = format!("Agents cannot connect: {e}");
            log::write(&message);
            startup_error = Some(message);
            None
        }
    };

    let menu = Menu::new();
    let title = MenuItem::new(APP_NAME, false, None);
    let discord = MenuItem::new("Discord: starting…", false, None);
    let showing = MenuItem::new("", false, None);
    let error = MenuItem::new("", false, None);
    let share = CheckMenuItem::with_id("share", "Share activity on Discord", true, true, None);
    let pause = CheckMenuItem::with_id("pause", "Pause", true, false, None);
    let project = CheckMenuItem::with_id("project", "Show project name", true, false, None);
    let pin = Submenu::new("Pin session", true);
    let startup = CheckMenuItem::with_id("startup", "Start with Windows", true, autostart::enabled(), None);
    let open_settings = MenuItem::with_id("settings", "Open settings file", true, None);
    let open_log = MenuItem::with_id("log", "Open log", true, None);
    let reconnect = MenuItem::with_id("reconnect", "Reconnect to Discord", true, None);
    let quit = MenuItem::with_id("quit", "Quit (stops sharing)", true, None);
    let items: [&dyn IsMenuItem; 4] = [&title, &discord, &showing, &error];
    for item in items {
        let _ = menu.append(item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let items: [&dyn IsMenuItem; 5] = [&share, &pause, &project, &pin, &startup];
    for item in items {
        let _ = menu.append(item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let items: [&dyn IsMenuItem; 4] = [&open_settings, &open_log, &reconnect, &quit];
    for item in items {
        let _ = menu.append(item);
    }
    let tray = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(format!("{APP_NAME} — starting")).with_icon(icon(true)).build().expect("create tray icon");
    let mut ui = Ui { tray, discord, showing, error, share, pause, project, startup, pin, shown: None };

    let mut last_refresh = Instant::now() - Duration::from_secs(5);
    let mut last_focus: Option<u32> = None;
    let mut action_error: Option<String> = None;
    run_loop(Duration::from_millis(150), || {
        let mut quit_now = false;
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let mut c = core.lock().unwrap();
            let now = now_ms();
            let result: Result<(), String> = match event.id.0.as_str() {
                "share" => { let on = !c.settings.enabled; c.save_toggles(&json!({"enabled": on})) }
                "project" => { let on = !c.settings.show_project; c.save_toggles(&json!({"showProject": on})) }
                "pause" => { c.paused = !c.paused; Ok(()) }
                "startup" => autostart::set(!autostart::enabled()),
                "settings" => {
                    let path = cad::settings::settings_path();
                    if !path.exists() {
                        let _ = cad::settings::write_atomic(&path, &(serde_json::to_string_pretty(&cad::settings::to_json(&c.settings)).unwrap() + "\n"));
                    }
                    open_in_notepad(&path);
                    Ok(())
                }
                "log" => { open_in_notepad(&log::path()); Ok(()) }
                "reconnect" => { c.replace_transport(Box::new(new_discord())); Ok(()) }
                "quit" => { quit_now = true; Ok(()) }
                id => match id.strip_prefix("pin:") {
                    Some(key) => c.store.pin((!key.is_empty()).then_some(key)),
                    None => Ok(()),
                },
            };
            if let Err(e) = &result {
                log::write(&format!("Menu action failed: {e}"));
            }
            action_error = result.err();
            c.refresh(now);
            drop(c);
            ui.shown = None; // Check items flip themselves on click; re-apply the real state.
        }
        if quit_now || core.lock().unwrap().quit_requested {
            return false;
        }
        // Report the foreground window's process so the focused session is shown first.
        let focused = cad::process::foreground_pid().filter(|p| *p != std::process::id());
        if focused.is_some() && focused != last_focus {
            last_focus = focused;
            core.lock().unwrap().set_focus(Some(Focus { pid: focused, key: None }));
        }
        if last_refresh.elapsed() >= Duration::from_secs(1) || ui.shown.is_none() {
            last_refresh = Instant::now();
            let status = {
                let mut c = core.lock().unwrap();
                c.refresh(now_ms());
                c.status(now_ms())
            };
            ui.apply(view(&status, action_error.as_deref().or(startup_error.as_deref())));
        }
        true
    });
    if let Some(s) = server.as_mut() {
        s.stop();
    }
    core.lock().unwrap().shutdown();
    log::write("stopped");
}

/// Only one tray per user session.
#[cfg(windows)]
fn single_instance() -> bool {
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
fn single_instance() -> bool {
    true
}

/// Win32 message loop (the tray icon needs one on its thread), calling `tick` on a timer until it returns false.
#[cfg(windows)]
fn run_loop(interval: Duration, mut tick: impl FnMut() -> bool) {
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
fn run_loop(interval: Duration, mut tick: impl FnMut() -> bool) {
    while tick() {
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_maps_to_menu_view() {
        let status = json!({"ok": true, "discord": "connected", "enabled": true, "paused": false, "showProject": false,
            "pinned": "codex:b", "activity": {"details": "Claude Code · Working"},
            "sessions": [{"key": "claude-code:a", "agent": "claude-code", "state": "working", "project": "app"},
                         {"key": "codex:b", "agent": "codex", "state": "idle"},
                         {"key": "vscode:w", "agent": "vscode", "edition": "insiders", "state": "working"}]});
        let v = view(&status, None);
        assert!(v.running && v.enabled && !v.paused && !v.show_project);
        assert_eq!(v.showing, "Claude Code · Working");
        assert_eq!(v.pinned.as_deref(), Some("codex:b"));
        assert_eq!(v.sessions[0], ("claude-code:a".into(), "Claude Code · working — app".into()));
        assert_eq!(v.sessions[2].1, "VS Code Insiders · working");
    }

    #[test]
    fn errors_and_cleared_activity() {
        let v = view(&json!({"ok": true, "activity": null, "sessions": [], "settingsError": "Invalid settings file"}), None);
        assert_eq!(v.showing, "Nothing");
        assert_eq!(v.error.as_deref(), Some("Invalid settings file"));
        assert_eq!(view(&json!({"ok": true}), Some("pipe busy")).error.as_deref(), Some("pipe busy"));
        assert_eq!(label("mystery", None), "Unknown");
        assert_eq!(label("vscode", Some("devin")), "Devin Desktop");
    }

    #[test]
    fn dimmed_icon_has_the_same_size() {
        assert_eq!(ICON.len(), 32 * 32 * 4);
        let _ = icon(true);
        let _ = icon(false);
    }
}
