//! Ctrl Alt Delegate: Discord presence for most of your coding tools.
//! One developer. Multiple agents. One workflow.

pub mod autostart;
pub mod core;
pub mod discord;
pub mod hook;
pub mod ipc;
pub mod log;
pub mod presentation;
pub mod process;
pub mod protocol;
pub mod selector;
pub mod settings;
pub mod sha256;
pub mod store;
pub mod theme;

/// Display name used consistently in Discord, the tray, and logs.
pub const APP_NAME: &str = "Ctrl Alt Delegate";
/// Discord Application ID of the "Ctrl Alt Delegate" application. Public; not a secret.
pub const DISCORD_CLIENT_ID: &str = "1572816615429640333";

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
