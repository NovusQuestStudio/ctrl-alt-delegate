//! "Start with Windows": a per-user Run entry (HKCU), no administrator rights or scheduled tasks.

#[cfg(windows)]
mod imp {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ};

    const RUN: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const NAME: windows::core::PCWSTR = w!("Ctrl Alt Delegate");

    fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
        let mut key = HKEY::default();
        (unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN, None, access, &mut key) } == ERROR_SUCCESS).then_some(key)
    }

    pub fn enabled() -> bool {
        let Some(key) = open(KEY_QUERY_VALUE) else { return false };
        let found = unsafe { RegQueryValueExW(key, NAME, None, None, None, None) } == ERROR_SUCCESS;
        unsafe { let _ = RegCloseKey(key); }
        found
    }

    pub fn set(on: bool) -> Result<(), String> {
        let key = open(KEY_SET_VALUE).ok_or("cannot open the Run key")?;
        let result = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let command = HSTRING::from(format!("\"{}\"", exe.display()));
            let bytes = unsafe { std::slice::from_raw_parts(command.as_ptr() as *const u8, (command.len() + 1) * 2) };
            unsafe { RegSetValueExW(key, NAME, None, REG_SZ, Some(bytes)) }
        } else {
            unsafe { RegDeleteValueW(key, NAME) }
        };
        unsafe { let _ = RegCloseKey(key); }
        if result == ERROR_SUCCESS || (!on && result.0 == 2) { Ok(()) } else { Err(format!("registry error {}", result.0)) }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn enabled() -> bool {
        false
    }
    pub fn set(_: bool) -> Result<(), String> {
        Err("Start at login is not available on this platform yet".into())
    }
}

pub use imp::{enabled, set};
