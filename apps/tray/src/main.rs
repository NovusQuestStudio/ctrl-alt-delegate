//! Ctrl Alt Delegate tray: shows status, toggles sharing, and reports the focused window.
//!
//! The tray never talks to Discord. It sends small JSON requests over the service's local,
//! per-user named pipe, and starts the Node service if it is not running.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod service;

use service::{Command, Service, Update};
use std::sync::mpsc;
use std::time::Duration;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const APP: &str = "Ctrl Alt Delegate";
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

/// Everything the menu shows, derived from one status response.
#[derive(Default, PartialEq, Clone)]
struct View {
    running: bool,
    discord: String,
    showing: String,
    enabled: bool,
    paused: bool,
    show_project: bool,
    pinned: Option<String>,
    sessions: Vec<(String, String)>,
    error: Option<String>,
}

fn label(agent: &str) -> &str {
    match agent {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "vscode" => "VS Code",
        _ => "Unknown",
    }
}

fn view(update: &Update) -> View {
    let Some(status) = &update.status else {
        return View { error: update.error.clone(), ..View::default() };
    };
    let text = |key: &str| status.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let flag = |key: &str| status.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    let showing = status
        .get("activity")
        .and_then(|a| a.get("details"))
        .and_then(|d| d.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "Nothing".into());
    let sessions = status
        .get("sessions")
        .and_then(|s| s.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    let key = s.get("key")?.as_str()?.to_string();
                    let agent = label(s.get("agent")?.as_str()?);
                    let state = s.get("state")?.as_str()?;
                    // The project name stays on this machine: the tray is local UI, not Discord.
                    let project = s.get("project").and_then(|p| p.as_str()).map(|p| format!(" — {p}")).unwrap_or_default();
                    Some((key, format!("{agent} · {state}{project}")))
                })
                .collect()
        })
        .unwrap_or_default();
    View {
        running: true,
        discord: text("discord"),
        showing,
        enabled: flag("enabled"),
        paused: flag("paused"),
        show_project: flag("showProject"),
        pinned: status.get("pinned").and_then(|p| p.as_str()).map(str::to_string),
        sessions,
        error: None,
    }
}

struct Ui {
    tray: TrayIcon,
    discord: MenuItem,
    showing: MenuItem,
    share: CheckMenuItem,
    pause: CheckMenuItem,
    project: CheckMenuItem,
    pin: Submenu,
    start: MenuItem,
    shown: View,
    pin_ids: Vec<String>,
}

impl Ui {
    fn apply(&mut self, next: View) {
        if next == self.shown {
            return;
        }
        if !next.running {
            self.discord.set_text("Service: not running");
            self.showing.set_text(next.error.as_deref().map(|e| format!("({e})")).unwrap_or_default());
        } else {
            let discord = match next.discord.as_str() {
                "connected" => "Discord: connected".to_string(),
                "dry-run" => "Discord: dry run".to_string(),
                other => format!("Discord: {other} (is Discord open?)"),
            };
            self.discord.set_text(discord);
            self.showing.set_text(if next.paused {
                "Showing: nothing (paused)".to_string()
            } else if !next.enabled {
                "Showing: nothing (sharing off)".to_string()
            } else {
                format!("Showing: {}", next.showing)
            });
        }
        for item in [&self.share, &self.pause, &self.project] {
            item.set_enabled(next.running);
        }
        self.pin.set_enabled(next.running);
        self.start.set_text(if next.running { "Restart service" } else { "Start service" });
        self.share.set_checked(next.enabled);
        self.pause.set_checked(next.paused);
        self.project.set_checked(next.show_project);
        if next.sessions != self.shown.sessions || next.pinned != self.shown.pinned {
            self.rebuild_pins(&next);
        }
        let dimmed = !next.running || next.paused || !next.enabled || next.discord != "connected";
        let _ = self.tray.set_icon(Some(icon(dimmed)));
        let tip = if !next.running {
            format!("{APP} — service not running")
        } else if next.paused {
            format!("{APP} — paused")
        } else {
            format!("{APP} — {}", next.showing)
        };
        let _ = self.tray.set_tooltip(Some(tip));
        self.shown = next;
    }

    fn rebuild_pins(&mut self, next: &View) {
        while self.pin.remove_at(0).is_some() {}
        self.pin_ids.clear();
        let auto = CheckMenuItem::with_id("pin:", "Automatic", true, next.pinned.is_none(), None);
        let _ = self.pin.append(&auto);
        if !next.sessions.is_empty() {
            let _ = self.pin.append(&PredefinedMenuItem::separator());
        }
        for (key, text) in &next.sessions {
            let item = CheckMenuItem::with_id(format!("pin:{key}"), text, true, next.pinned.as_deref() == Some(key), None);
            let _ = self.pin.append(&item);
            self.pin_ids.push(key.clone());
        }
    }
}

fn main() {
    if !service::single_instance() {
        return;
    }
    let menu = Menu::new();
    let title = MenuItem::new(APP, false, None);
    let discord = MenuItem::new("Starting…", false, None);
    let showing = MenuItem::new("", false, None);
    let share = CheckMenuItem::with_id("share", "Share activity on Discord", false, true, None);
    let pause = CheckMenuItem::with_id("pause", "Pause", false, false, None);
    let project = CheckMenuItem::with_id("project", "Show project name", false, false, None);
    let pin = Submenu::new("Pin session", false);
    let start = MenuItem::with_id("start", "Start service", true, None);
    let quit = MenuItem::with_id("quit", "Quit (stops sharing)", true, None);
    for item in [&title as &dyn tray_icon::menu::IsMenuItem, &discord, &showing] {
        let _ = menu.append(item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    for item in [&share as &dyn tray_icon::menu::IsMenuItem, &pause, &project, &pin] {
        let _ = menu.append(item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&start);
    let _ = menu.append(&quit);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(format!("{APP} — starting"))
        .with_icon(icon(true))
        .build()
        .expect("create tray icon");

    let (to_worker, commands) = mpsc::channel::<Command>();
    let (updates_tx, updates) = mpsc::channel::<Update>();
    let worker = std::thread::spawn(move || Service::discover().run(commands, updates_tx));

    let mut ui = Ui { tray, discord, showing, share, pause, project, pin, start, shown: View { discord: "?".into(), ..View::default() }, pin_ids: vec![] };
    ui.rebuild_pins(&View::default());

    service::run_loop(Duration::from_millis(150), || {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.0.as_str();
            let command = match id {
                "share" => Some(Command::Setting("enabled", !ui.shown.enabled)),
                "pause" => Some(Command::Pause(!ui.shown.paused)),
                "project" => Some(Command::Setting("showProject", !ui.shown.show_project)),
                "start" => Some(if ui.shown.running { Command::Restart } else { Command::Start }),
                "quit" => {
                    let _ = to_worker.send(Command::Quit);
                    return false;
                }
                _ => id.strip_prefix("pin:").map(|key| Command::Pin((!key.is_empty()).then(|| key.to_string()))),
            };
            if let Some(command) = command {
                let _ = to_worker.send(command);
            }
            // Check items flip themselves on click; re-apply the last known state until the service confirms.
            let last = std::mem::take(&mut ui.shown);
            ui.apply(last);
        }
        let mut latest = None;
        while let Ok(update) = updates.try_recv() {
            latest = Some(update);
        }
        if let Some(update) = latest {
            ui.apply(view(&update));
        }
        true
    });
    let _ = worker.join();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_maps_to_menu_view() {
        let status = json!({"ok": true, "discord": "connected", "enabled": true, "paused": false, "showProject": false,
            "pinned": "codex:b", "activity": {"details": "Claude Code · Working"},
            "sessions": [{"key": "claude-code:a", "agent": "claude-code", "state": "working", "project": "app"},
                         {"key": "codex:b", "agent": "codex", "state": "idle"}]});
        let v = view(&Update { status: Some(status), error: None });
        assert!(v.running && v.enabled && !v.paused && !v.show_project);
        assert_eq!(v.showing, "Claude Code · Working");
        assert_eq!(v.pinned.as_deref(), Some("codex:b"));
        assert_eq!(v.sessions, vec![("claude-code:a".into(), "Claude Code · working — app".into()), ("codex:b".into(), "Codex · idle".into())]);
    }

    #[test]
    fn missing_service_and_cleared_activity() {
        let down = view(&Update { status: None, error: Some("not running".into()) });
        assert!(!down.running);
        assert_eq!(down.error.as_deref(), Some("not running"));
        let idle = view(&Update { status: Some(json!({"ok": true, "activity": null, "sessions": []})), error: None });
        assert_eq!(idle.showing, "Nothing");
        assert_eq!(label("mystery"), "Unknown");
    }

    #[test]
    fn dimmed_icon_keeps_size_and_halves_alpha() {
        let bright = ICON.to_vec();
        let mut dim = ICON.to_vec();
        for px in dim.chunks_exact_mut(4) { px[3] /= 2; }
        assert_eq!(bright.len(), 32 * 32 * 4);
        let _ = icon(true);
        assert!(dim.iter().skip(3).step_by(4).zip(bright.iter().skip(3).step_by(4)).all(|(d, b)| *d == b / 2));
    }
}
