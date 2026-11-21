//! Small local log for troubleshooting. Never contains prompts, code, or project names. Setup entries
//! include the commands it runs, with your user and app folder paths.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());
const MAX_BYTES: u64 = 512 * 1024;

pub fn path() -> PathBuf {
    crate::settings::state_dir().join("ctrl-alt-delegate.log")
}

/// Append one line; when the file passes 512 KB it is moved to `.old` (one generation kept).
pub fn write(message: &str) {
    let _guard = LOCK.lock();
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(file, "{secs} {message}");
    }
}
