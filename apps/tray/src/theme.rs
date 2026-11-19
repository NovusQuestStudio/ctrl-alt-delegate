//! Dark or light tray menus, matching the Windows taskbar ("Choose your Windows mode").
//! Pop-up menus only follow dark mode through uxtheme's SetPreferredAppMode (ordinal 135) and
//! FlushMenuThemes (ordinal 136). They are undocumented but unchanged since Windows 10 1903 (build 18362),
//! and are widely used by tray apps. On older builds nothing is called and menus stay light.

#[cfg(windows)]
mod imp {
    use windows::core::{w, PCSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};

    fn dword(path: windows::core::PCWSTR, name: windows::core::PCWSTR) -> Option<u32> {
        let mut value = 0u32;
        let mut size = 4u32;
        let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, path, name, RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as _), Some(&mut size)) };
        (ok == ERROR_SUCCESS).then_some(value)
    }

    fn build() -> u32 {
        let mut buf = [0u16; 16];
        let mut size = (buf.len() * 2) as u32;
        let ok = unsafe {
            RegGetValueW(HKEY_LOCAL_MACHINE, w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"), w!("CurrentBuildNumber"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size))
        };
        if ok != ERROR_SUCCESS {
            return 0;
        }
        String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(0)]).parse().unwrap_or(0)
    }

    /// True when the taskbar uses dark mode (SystemUsesLightTheme = 0).
    pub fn taskbar_dark() -> bool {
        dword(w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), w!("SystemUsesLightTheme")) == Some(0)
    }

    /// Apply dark (true) or light (false) pop-up menus for this process.
    pub fn apply(dark: bool) {
        if build() < 18362 {
            return;
        }
        unsafe {
            let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) else { return };
            let ordinal = |n: usize| GetProcAddress(uxtheme, PCSTR(n as *const u8));
            // enum PreferredAppMode { Default, AllowDark, ForceDark, ForceLight }
            if let Some(set_mode) = ordinal(135) {
                let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
                set_mode(if dark { 2 } else { 3 });
            }
            if let Some(flush) = ordinal(136) {
                let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
                flush();
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn taskbar_dark() -> bool {
        false
    }
    pub fn apply(_: bool) {}
}

pub use imp::{apply, taskbar_dark};
