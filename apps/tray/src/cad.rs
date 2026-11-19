//! `cad`: console companion to the Ctrl Alt Delegate tray app.
//! Agent plugins call `cad hook claude-code|codex|devin|antigravity`; people can use it to check or control the service.

use cad::ipc::{request, service_path, RequestError};
use cad::protocol::Agent;
use serde_json::{json, Value};
use std::time::Duration;

const HELP: &str = "Ctrl Alt Delegate command line

  cad status                 Show connection, sessions, and the current activity as JSON
  cad pause | resume         Temporarily hide or restore Discord activity
  cad pin AGENT:SESSION_ID   Always show one session
  cad unpin                  Resume automatic selection
  cad start                  Start the tray app (and the presence service) if it is not running
  cad stop                   Quit the tray app and clear Discord activity
  cad hook claude-code|codex|devin|antigravity EVENT
                             Receive a lifecycle hook on stdin (used by the agent plugins)
  cad devin connect          Add this app's hooks to Devin's config.json (nothing else changes)
  cad devin disconnect       Remove them again
  cad paths                  Show settings, state, log, and pipe locations

Environment: CAD_SOCKET, CAD_CONFIG, CAD_STATE, DISCORD_IPC_PATH";

fn send(message: Value) -> Result<Value, String> {
    request(&service_path(), &message, Duration::from_secs(5)).map_err(|e| e.to_string())
}

fn start() -> Result<(), String> {
    if request(&service_path(), &json!({"type": "status"}), Duration::from_millis(500)).is_ok() {
        println!("Ctrl Alt Delegate is already running.");
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let app = exe.with_file_name(if cfg!(windows) { "ctrl-alt-delegate.exe" } else { "ctrl-alt-delegate" });
    std::process::Command::new(&app).spawn().map_err(|e| format!("cannot start {}: {e}", app.display()))?;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if request(&service_path(), &json!({"type": "status"}), Duration::from_millis(300)).is_ok() {
            println!("Ctrl Alt Delegate started.");
            return Ok(());
        }
    }
    Err("The tray app did not start; see the log (cad paths).".into())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    // Hooks must never print or fail.
    if arg(0) == Some("hook") {
        if let Some(agent) = arg(1).and_then(Agent::parse).filter(|a| *a != Agent::Vscode) {
            cad::hook::run(agent, arg(2));
        }
        return;
    }
    let result = match arg(0) {
        Some("status") => send(json!({"type": "status"})).map(|v| println!("{}", serde_json::to_string_pretty(&v).unwrap())),
        Some("pause") => send(json!({"type": "pause", "paused": true})).map(|_| ()),
        Some("resume") => send(json!({"type": "pause", "paused": false})).map(|_| ()),
        Some("pin") if arg(1).is_some() => send(json!({"type": "pin", "key": arg(1)})).map(|_| ()),
        Some("unpin") => send(json!({"type": "pin", "key": null})).map(|_| ()),
        Some("start") => start(),
        Some("devin") if matches!(arg(1), Some("connect" | "disconnect")) => {
            let config = cad::setup::devin_config(&cad::setup::Env::system());
            let cad = std::env::current_exe().map_err(|e| e.to_string());
            cad.and_then(|cad| cad::setup::devin_hooks(&config, (arg(1) == Some("connect")).then_some(cad.as_path()))).map(|message| println!("{message}"))
        }
        Some("stop") => match request(&service_path(), &json!({"type": "stop"}), Duration::from_secs(5)) {
            Ok(_) => Ok(println!("Ctrl Alt Delegate stopped.")),
            Err(RequestError::NotRunning) => Ok(println!("Ctrl Alt Delegate is not running.")),
            Err(e) => Err(e.to_string()),
        },
        Some("paths") => Ok(println!(
            "settings: {}\nsessions: {}\nlog:      {}\npipe:     {}",
            cad::settings::settings_path().display(),
            cad::settings::state_path().display(),
            cad::log::path().display(),
            service_path()
        )),
        _ => Ok(println!("{HELP}")),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
