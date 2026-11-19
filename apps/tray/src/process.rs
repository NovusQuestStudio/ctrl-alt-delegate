//! Local process information: pids, parent pids, and executable names only.

use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

pub type ProcessTable = HashMap<u32, ProcessInfo>;

/// Wrappers that sit between an agent and its hook process; never the agent itself.
const WRAPPERS: [&str; 16] = [
    "bash", "sh", "zsh", "fish", "dash", "ksh", "cmd", "powershell", "pwsh", "conhost", "openconsole", "env", "sudo", "timeout", "winpty", "mintty",
];

fn is_wrapper(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    WRAPPERS.contains(&stem) || stem == "winpty-agent"
}

/// The process chain from `pid` upwards (inclusive), stopping at cycles, unknown parents, or `max` entries.
pub fn ancestry(table: &ProcessTable, pid: u32, max: usize) -> Vec<ProcessInfo> {
    let mut chain = Vec::new();
    let mut current = table.get(&pid);
    while let Some(p) = current {
        if chain.len() >= max || chain.iter().any(|c: &ProcessInfo| c.pid == p.pid) {
            break;
        }
        chain.push(p.clone());
        if p.ppid == p.pid || p.ppid == 0 {
            break;
        }
        current = table.get(&p.ppid);
    }
    chain
}

/// The agent is the nearest ancestor of the hook's parent that is not a shell or console wrapper.
pub fn find_agent(chain: &[ProcessInfo]) -> Option<&ProcessInfo> {
    chain.iter().find(|p| !is_wrapper(&p.name))
}

#[cfg(windows)]
mod imp {
    use super::{ProcessInfo, ProcessTable};
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, STILL_ACTIVE};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    pub fn snapshot() -> Result<ProcessTable, String> {
        let mut table = ProcessTable::new();
        unsafe {
            let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?;
            let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(handle, &mut entry).is_ok() {
                loop {
                    let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    table.insert(entry.th32ProcessID, ProcessInfo { pid: entry.th32ProcessID, ppid: entry.th32ParentProcessID, name });
                    if Process32NextW(handle, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(handle);
        }
        Ok(table)
    }

    /// True while the process exists. A process we may not inspect still counts as alive.
    pub fn alive(pid: u32) -> bool {
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(handle) => {
                    let mut code = 0u32;
                    let ok = GetExitCodeProcess(handle, &mut code).is_ok();
                    let _ = CloseHandle(handle);
                    !ok || code == STILL_ACTIVE.0 as u32
                }
                Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
            }
        }
    }

    pub fn foreground_pid() -> Option<u32> {
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
}

#[cfg(not(windows))]
mod imp {
    use super::{ProcessInfo, ProcessTable};

    pub fn snapshot() -> Result<ProcessTable, String> {
        let out = std::process::Command::new("ps").args(["-A", "-o", "pid=,ppid=,comm="]).output().map_err(|e| e.to_string())?;
        let mut table = ProcessTable::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let mut parts = line.split_whitespace();
            if let (Some(pid), Some(ppid), Some(name)) = (parts.next(), parts.next(), parts.next()) {
                if let (Ok(pid), Ok(ppid)) = (pid.parse(), ppid.parse()) {
                    let name = name.rsplit('/').next().unwrap_or(name).to_string();
                    table.insert(pid, ProcessInfo { pid, ppid, name });
                }
            }
        }
        Ok(table)
    }

    pub fn alive(pid: u32) -> bool {
        std::process::Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(true)
    }

    pub fn foreground_pid() -> Option<u32> {
        None // macOS: NSWorkspace frontmost application, in a later release.
    }
}

pub use imp::{alive, foreground_pid, snapshot};

/// This process's parent, found from the process table (std has no portable way on Windows).
pub fn parent_pid() -> Option<u32> {
    snapshot().ok()?.get(&std::process::id()).map(|p| p.ppid).filter(|p| *p != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(u32, u32, &str)]) -> ProcessTable {
        rows.iter().map(|(pid, ppid, name)| (*pid, ProcessInfo { pid: *pid, ppid: *ppid, name: name.to_string() })).collect()
    }

    #[test]
    fn agent_is_first_non_shell_ancestor() {
        let t = table(&[(73280, 9808, "node.exe"), (9808, 60784, "bash.exe"), (60784, 34848, "bash.exe"), (34848, 31000, "claude.exe"),
            (31000, 30000, "pwsh.exe"), (30000, 4, "WindowsTerminal.exe"), (4, 4, "System")]);
        let chain = ancestry(&t, 9808, 16);
        assert_eq!(chain.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["bash.exe", "bash.exe", "claude.exe", "pwsh.exe", "WindowsTerminal.exe", "System"]);
        assert_eq!(find_agent(&chain).unwrap().pid, 34848);
        assert_eq!(find_agent(&ancestry(&t, 34848, 16)).unwrap().name, "claude.exe");
        assert_eq!(ancestry(&table(&[(1, 2, "a"), (2, 1, "b")]), 1, 16).len(), 2, "cycles stop");
        assert!(find_agent(&ancestry(&table(&[(1, 0, "cmd.exe")]), 1, 16)).is_none());
    }

    #[test]
    fn real_snapshot_sees_this_process() {
        let t = snapshot().unwrap();
        assert!(t.len() > 5);
        assert!(t.contains_key(&std::process::id()));
        assert!(parent_pid().is_some());
        assert!(alive(std::process::id()));
        assert!(!alive(u32::MAX - 3));
    }
}
