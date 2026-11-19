//! The "Connect your tools" window: one checkbox per tool, where ticked means connected.
//! Apply runs each tool's own installer on a background thread; see `setup.rs` for what runs.

use crate::setup::{self, Change, Env, Status, Tool};
use crate::{autostart, log, APP_NAME};
use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, CreateSolidBrush, DeleteObject, FillRect, SetBkMode, SetTextColor, HBRUSH, HDC, HFONT, LOGFONTW, TRANSPARENT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

/// One row per tool, then "Start with Windows".
const ROWS: usize = Tool::ALL.len() + 1;
const ID_CHECK: i32 = 100;
const ID_NAME: i32 = 200;
const ID_DETAIL: i32 = 300;
const ID_INTRO: i32 = 11;
const ID_RESULT: i32 = 400;
const ID_REFRESH: i32 = 501;
const ID_APPLY: i32 = 502;
const ID_CLOSE: i32 = 503;
const ID_SHOW: i32 = 504;
const ID_COPY: i32 = 505;
const ID_COMMANDS: i32 = 600;
const WM_DONE: u32 = WM_APP + 1;
const TIMEOUT: Duration = Duration::from_secs(120);

struct Model {
    env: Env,
    root: Option<PathBuf>,
    vsix: Option<PathBuf>,
    statuses: Vec<Option<Status>>,
    busy: bool,
}

#[derive(Clone, Copy)]
struct Paint {
    background: HBRUSH,
    text: COLORREF,
    muted: COLORREF,
    /// Highlights rows that Apply will change.
    accent: COLORREF,
    dark: bool,
    font: HFONT,
    bold: HFONT,
    title: HFONT,
    mono: HFONT,
    dpi: i32,
}

thread_local! {
    static WINDOW: Cell<Option<HWND>> = const { Cell::new(None) };
    static MODEL: RefCell<Option<Model>> = const { RefCell::new(None) };
    static PAINT: Cell<Option<Paint>> = const { Cell::new(None) };
    /// Rows the user can change right now (drawn in the normal text colour).
    static AVAILABLE: Cell<[bool; ROWS]> = const { Cell::new([false; ROWS]) };
    /// True while the commands view replaces the tool rows.
    static SHOWING: Cell<bool> = const { Cell::new(false) };
    /// Each row's normal description, shown whenever Apply wouldn't change that row.
    static DETAILS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Rows Apply will change (drawn bold, in the accent colour).
    static PENDING: Cell<[bool; ROWS]> = const { Cell::new([false; ROWS]) };
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn s(v: i32) -> i32 {
    let dpi = PAINT.get().map_or(96, |p| p.dpi);
    v * dpi / 96
}

fn item(id: i32) -> HWND {
    WINDOW.get().and_then(|w| unsafe { GetDlgItem(Some(w), id) }.ok()).unwrap_or_default()
}

fn set_text(id: i32, text: &str) {
    unsafe {
        let _ = SetWindowTextW(item(id), &HSTRING::from(text));
    }
}

fn checked(id: i32) -> bool {
    unsafe { SendMessageW(item(id), BM_GETCHECK, None, None).0 == 1 }
}

fn set_checked(id: i32, on: bool) {
    unsafe {
        SendMessageW(item(id), BM_SETCHECK, Some(WPARAM(on as usize)), None);
    }
}

fn enable(id: i32, on: bool) {
    unsafe {
        let _ = EnableWindow(item(id), on);
    }
}

/// Call once at startup, before any window or menu exists, so text is sharp on scaled displays.
pub fn init_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
    }
}

pub fn is_open() -> bool {
    WINDOW.get().is_some()
}

/// Lets Tab, Enter and Esc work in the window. Returns true when the message was handled.
pub fn pre_translate(msg: &MSG) -> bool {
    match WINDOW.get() {
        Some(hwnd) => unsafe { IsDialogMessageW(hwnd, msg).as_bool() },
        None => false,
    }
}

/// Opens the window, or brings it to the front. On first run, tools that were found start ticked.
pub fn open(dark: bool, first_run: bool) {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        return;
    }
    let env = Env::system();
    let root = std::env::current_exe().ok().and_then(|exe| setup::bundle_root(&exe));
    let vsix = root.as_deref().and_then(setup::vsix);
    MODEL.set(Some(Model { env, root, vsix, statuses: vec![None; Tool::ALL.len()], busy: false }));
    unsafe { create(dark) };
    refresh(first_run);
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

unsafe fn create(dark: bool) {
    let instance = GetModuleHandleW(None).unwrap_or_default();
    let class = w!("CtrlAltDelegateSetup");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: instance.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        // Icon resource 1 from assets/app.rc.
        hIcon: LoadIconW(Some(instance.into()), PCWSTR(std::ptr::without_provenance(1))).unwrap_or_default(),
        ..Default::default()
    };
    RegisterClassExW(&wc); // Fails harmlessly when already registered.

    let mut metrics = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    let _ = SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, metrics.cbSize, Some(&mut metrics as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    let base = metrics.lfMessageFont;
    let bold = LOGFONTW { lfWeight: 600, ..base };
    let title = LOGFONTW { lfWeight: 600, lfHeight: base.lfHeight * 3 / 2, ..base };
    let mut mono = LOGFONTW { lfWeight: 400, ..base };
    let face: Vec<u16> = "Consolas".encode_utf16().collect();
    mono.lfFaceName = [0; 32];
    mono.lfFaceName[..face.len()].copy_from_slice(&face);
    // The accent matches the blue of Windows 11's ticked checkboxes.
    let (bg, text, muted, accent) = if dark {
        (rgb(32, 32, 32), rgb(240, 240, 240), rgb(170, 170, 170), rgb(96, 205, 255))
    } else {
        (rgb(249, 249, 249), rgb(26, 26, 26), rgb(96, 96, 96), rgb(0, 95, 184))
    };
    PAINT.set(Some(Paint {
        background: CreateSolidBrush(bg),
        text,
        muted,
        accent,
        dark,
        font: CreateFontIndirectW(&base),
        bold: CreateFontIndirectW(&bold),
        title: CreateFontIndirectW(&title),
        mono: CreateFontIndirectW(&mono),
        dpi: GetDpiForSystem() as i32,
    }));

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let ex = WS_EX_CONTROLPARENT | WS_EX_APPWINDOW;
    let mut rect = RECT { left: 0, top: 0, right: s(800), bottom: s(604) };
    let _ = AdjustWindowRectEx(&mut rect, style, false, ex);
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
    let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
    let Ok(hwnd) = CreateWindowExW(ex, class, &HSTRING::from(format!("{APP_NAME} setup")), style, x, y, w, h, None, None, Some(instance.into()), None) else {
        log::write("Couldn't open the setup window");
        return;
    };
    WINDOW.set(Some(hwnd));
    if dark {
        crate::theme::allow_dark(hwnd.0 as isize);
        let on: i32 = 1;
        // DWMWA_USE_IMMERSIVE_DARK_MODE
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(20), &on as *const i32 as *const _, 4);
    }

    let p = PAINT.get().unwrap();
    control(hwnd, w!("STATIC"), "Connect your tools", 0, 10, (24, 16, 752, 32), p.title);
    control(
        hwnd,
        w!("STATIC"),
        "Tick a tool to show it on Discord, or untick it to disconnect. Only Ctrl Alt Delegate's own plugin is added or removed.",
        0,
        ID_INTRO,
        (24, 52, 752, 24),
        p.font,
    );
    for row in 0..ROWS {
        // Two columns: the name on the left, what it means on the right.
        let y = 88 + row as i32 * 42;
        let name = Tool::ALL.get(row).map_or("Start with Windows", |t| t.label());
        control(hwnd, w!("BUTTON"), "", WS_TABSTOP.0 | 3 /* BS_AUTOCHECKBOX */, ID_CHECK + row as i32, (24, y + 2, 20, 20), p.font);
        control(hwnd, w!("STATIC"), name, 0x100 /* SS_NOTIFY */, ID_NAME + row as i32, (52, y + 2, 180, 20), p.bold);
        control(hwnd, w!("STATIC"), "", 0x100, ID_DETAIL + row as i32, (240, y + 3, 536, 38), p.font);
    }
    // The commands view: hidden until "Show commands", then it takes the place of the tool rows.
    let edit = 0x4 /* ES_MULTILINE */ | 0x40 /* ES_AUTOVSCROLL */ | 0x800 /* ES_READONLY */;
    control(hwnd, w!("EDIT"), "", WS_TABSTOP.0 | WS_VSCROLL.0 | WS_BORDER.0 | edit, ID_COMMANDS, (24, 88, 752, 370), p.mono);
    control(hwnd, w!("BUTTON"), "Copy", WS_TABSTOP.0, ID_COPY, (676, 466, 100, 30), p.font);
    for id in [ID_COMMANDS, ID_COPY] {
        let _ = ShowWindow(item(id), SW_HIDE);
    }
    control(hwnd, w!("STATIC"), "", 0, ID_RESULT, (24, 512, 752, 40), p.font);
    control(hwnd, w!("BUTTON"), "Refresh", WS_TABSTOP.0, ID_REFRESH, (24, 560, 104, 32), p.font);
    control(hwnd, w!("BUTTON"), "Show commands", WS_TABSTOP.0, ID_SHOW, (136, 560, 150, 32), p.font);
    control(hwnd, w!("BUTTON"), "Apply", WS_TABSTOP.0 | 1 /* BS_DEFPUSHBUTTON */, ID_APPLY, (520, 560, 152, 32), p.font);
    control(hwnd, w!("BUTTON"), "Close", WS_TABSTOP.0, ID_CLOSE, (680, 560, 96, 32), p.font);
}

unsafe fn control(parent: HWND, class: PCWSTR, text: &str, style: u32, id: i32, (x, y, w, h): (i32, i32, i32, i32), font: HFONT) {
    let Ok(hwnd) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        &HSTRING::from(text),
        WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
        s(x),
        s(y),
        s(w),
        s(h),
        Some(parent),
        Some(HMENU(id as isize as *mut _)),
        None,
        None,
    ) else {
        return;
    };
    SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    if PAINT.get().is_some_and(|p| p.dark) {
        crate::theme::allow_dark(hwnd.0 as isize);
        let _ = SetWindowTheme(hwnd, w!("DarkMode_Explorer"), PCWSTR::null());
        SendMessageW(hwnd, WM_THEMECHANGED, None, None);
    }
}

/// Re-reads what's installed. Ticks follow the installed state only where it changed, so the
/// user's pending choices survive switching to Codex and back.
fn refresh(first_run: bool) {
    let rows = MODEL.with_borrow_mut(|model| {
        let m = model.as_mut()?;
        let root = m.root.clone();
        let initial = m.statuses.iter().all(Option::is_none);
        let mut rows = Vec::new();
        for (i, tool) in Tool::ALL.into_iter().enumerate() {
            let status = setup::detect(tool, &m.env, root.as_deref());
            let ready = root.as_deref().is_some_and(|r| setup::plugin_ready(r, tool)) && (!tool.uses_extension() || m.vsix.is_some());
            let available = status.cli.is_some() && (status.installed || ready);
            let detail = setup::describe(tool, &status, root.as_deref(), m.vsix.is_some());
            let changed = m.statuses[i].as_ref().is_none_or(|old| old.installed != status.installed);
            let tick = changed.then_some(status.installed || (first_run && available));
            m.statuses[i] = Some(status);
            rows.push((detail, available, tick));
        }
        Some((rows, m.busy, initial))
    });
    let Some((rows, busy, initial)) = rows else { return };
    let mut available = [false; ROWS];
    let mut details = Vec::with_capacity(ROWS);
    for (i, (detail, can, tick)) in rows.into_iter().enumerate() {
        available[i] = can;
        details.push(detail);
        if let Some(on) = tick {
            set_checked(ID_CHECK + i as i32, on);
        }
        enable(ID_CHECK + i as i32, can && !busy);
    }
    let startup = Tool::ALL.len();
    available[startup] = true;
    details.push("Opens Ctrl Alt Delegate in the tray when you sign in to Windows.".into());
    // update_apply below shows these, or a "Will …" line for rows Apply would change.
    DETAILS.set(details);
    if initial {
        // On first run it starts ticked, like the tools that were found.
        set_checked(ID_CHECK + startup as i32, autostart::enabled() || first_run);
    }
    enable(ID_CHECK + startup as i32, !busy);
    AVAILABLE.set(available);
    update_apply();
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true);
        }
    }
}

type Job = (Tool, Change, Vec<Vec<OsString>>);

/// What Apply would do: tool changes, plus the Start with Windows setting if it differs.
fn pending() -> (Vec<Job>, Option<bool>) {
    let wanted: Vec<bool> = (0..Tool::ALL.len()).map(|i| checked(ID_CHECK + i as i32)).collect();
    let jobs = MODEL.with_borrow(|model| {
        let Some(m) = model.as_ref() else { return Vec::new() };
        let root = m.root.clone().unwrap_or_default();
        Tool::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(i, tool)| {
                let status = m.statuses[i].as_ref()?;
                let change = setup::change(tool, status, wanted[i], &root)?;
                let cli = status.cli.as_deref()?;
                let commands = setup::commands(tool, change, status, cli, &root, m.vsix.as_deref());
                (!commands.is_empty()).then_some((tool, change, commands))
            })
            .collect()
    });
    let startup = checked(ID_CHECK + Tool::ALL.len() as i32);
    (jobs, (startup != autostart::enabled()).then_some(startup))
}

fn update_apply() {
    let busy = MODEL.with_borrow(|m| m.as_ref().is_some_and(|m| m.busy));
    let (jobs, startup) = pending();
    let count = jobs.len() + startup.is_some() as usize;
    if count > 0 && !busy {
        let mut plan: Vec<String> = jobs.iter().map(|(tool, change, _)| format!("{} {}", change.verb(), tool.label())).collect();
        if let Some(on) = startup {
            plan.push(format!("turn Start with Windows {}", if on { "on" } else { "off" }));
        }
        let text = format!("Apply will {}.", plan.join(", "));
        set_text(ID_RESULT, &text);
    }
    // Rows Apply will change say so, in bold accent text; the rest show their normal description.
    let mut lines: Vec<Option<String>> = vec![None; ROWS];
    for (tool, change, _) in &jobs {
        if let Some(row) = Tool::ALL.iter().position(|t| t == tool) {
            lines[row] = Some(match change {
                Change::Install => "Will connect when you click Apply.".into(),
                Change::Relink => "Will relink to this folder when you click Apply.".into(),
                Change::Remove => "Will disconnect when you click Apply.".into(),
            });
        }
    }
    if let Some(on) = startup {
        lines[Tool::ALL.len()] = Some(format!("Will turn {} when you click Apply.", if on { "on" } else { "off" }));
    }
    let details = DETAILS.with_borrow(Clone::clone);
    let mut pending = [false; ROWS];
    let fonts = PAINT.get().map(|p| (p.font, p.bold));
    for (row, line) in lines.iter().enumerate() {
        pending[row] = line.is_some();
        let normal = details.get(row).map_or("", String::as_str);
        set_text(ID_DETAIL + row as i32, line.as_deref().unwrap_or(normal));
        if let Some((font, bold)) = fonts {
            let font = if pending[row] { bold } else { font };
            unsafe {
                SendMessageW(item(ID_DETAIL + row as i32), WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            }
        }
    }
    PENDING.set(pending);
    set_text(ID_APPLY, &match count {
        0 => "Apply".to_string(),
        1 => "Apply 1 change".to_string(),
        n => format!("Apply {n} changes"),
    });
    enable(ID_APPLY, count > 0 && !busy);
    enable(ID_REFRESH, !busy);
}

/// The exact commands Apply would run now, ready to paste into PowerShell.
fn commands_text() -> String {
    let (jobs, startup) = pending();
    if jobs.is_empty() && startup.is_none() {
        return "Nothing to run yet.\r\n\r\nTick a tool to connect it or untick one to disconnect it, then click Show commands again.".into();
    }
    let mut text = String::from("# Apply runs these commands, in order. You can also paste them into PowerShell.\r\n");
    for (tool, change, commands) in &jobs {
        text.push_str(&format!("\r\n# {}: {}\r\n", tool.label(), change.verb()));
        for command in commands {
            text.push_str(&setup::display(command));
            text.push_str("\r\n");
        }
    }
    if let Some(on) = startup {
        text.push_str(&format!(
            "\r\n# Start with Windows: {} (no command; Apply changes this directly)\r\n# {} the value \"Ctrl Alt Delegate\" under {}\r\n",
            if on { "on" } else { "off" },
            if on { "Adds" } else { "Removes" },
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
        ));
    }
    text
}

/// Swaps the tool rows for the commands view, or back.
fn show_commands(show: bool) {
    SHOWING.set(show);
    if show {
        set_text(ID_COMMANDS, &commands_text());
    }
    let rows = (0..ROWS as i32).flat_map(|i| [ID_CHECK + i, ID_NAME + i, ID_DETAIL + i]);
    unsafe {
        for id in rows {
            let _ = ShowWindow(item(id), if show { SW_HIDE } else { SW_SHOW });
        }
        for id in [ID_COMMANDS, ID_COPY] {
            let _ = ShowWindow(item(id), if show { SW_SHOW } else { SW_HIDE });
        }
    }
    set_text(ID_SHOW, if show { "Hide commands" } else { "Show commands" });
}

fn copy_commands() {
    unsafe {
        let edit = item(ID_COMMANDS);
        SendMessageW(edit, 0x00B1 /* EM_SETSEL */, Some(WPARAM(0)), Some(LPARAM(-1)));
        SendMessageW(edit, WM_COPY, None, None);
        SendMessageW(edit, 0x00B1, Some(WPARAM(usize::MAX)), Some(LPARAM(0)));
    }
    set_text(ID_RESULT, "Commands copied.");
}

fn apply() {
    let (jobs, startup) = pending();
    if jobs.is_empty() && startup.is_none() {
        return;
    }
    let mut lines = Vec::new();
    if let Some(on) = startup {
        lines.push(match autostart::set(on) {
            Ok(()) => format!("Start with Windows: {}.", if on { "on" } else { "off" }),
            Err(e) => format!("Start with Windows: couldn't change it ({e})."),
        });
    }
    if jobs.is_empty() {
        set_text(ID_RESULT, &lines.join("\n"));
        update_apply();
        return;
    }
    if SHOWING.get() {
        show_commands(false);
    }
    MODEL.with_borrow_mut(|m| {
        if let Some(m) = m.as_mut() {
            m.busy = true;
        }
    });
    for i in 0..ROWS {
        enable(ID_CHECK + i as i32, false);
    }
    update_apply();
    let names: Vec<&str> = jobs.iter().map(|j| j.0.label()).collect();
    set_text(ID_RESULT, &format!("Working on {}… this can take up to a minute.", names.join(", ")));
    let Some(hwnd) = WINDOW.get() else { return };
    let target = hwnd.0 as isize; // HWND isn't Send; the raw handle is.
    std::thread::spawn(move || {
        for (tool, change, commands) in jobs {
            let result = commands.iter().try_for_each(|c| {
                log::write(&format!("Setup: running {}", setup::display(c)));
                setup::run(c, TIMEOUT).map(drop)
            });
            let line = match &result {
                Ok(()) => format!("{}: {}", tool.label(), setup::after(tool, change)),
                Err(e) => setup::failure(tool, change, e),
            };
            log::write(&format!("Setup: {line}"));
            lines.push(line);
        }
        let text = Box::into_raw(Box::new(lines.join("\n")));
        unsafe {
            if PostMessageW(Some(HWND(target as *mut _)), WM_DONE, WPARAM(0), LPARAM(text as isize)).is_err() {
                drop(Box::from_raw(text)); // The window was closed meanwhile.
            }
        }
    });
}

fn close() {
    if let Some(hwnd) = WINDOW.get() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            let code = (wparam.0 >> 16) & 0xffff;
            let busy = MODEL.with_borrow(|m| m.as_ref().is_some_and(|m| m.busy));
            match id {
                ID_APPLY => apply(),
                ID_REFRESH if !busy => refresh(false),
                ID_CLOSE | 2 /* IDCANCEL: Esc */ => close(),
                ID_SHOW => show_commands(!SHOWING.get()),
                ID_COPY => copy_commands(),
                _ if (ID_CHECK..ID_CHECK + ROWS as i32).contains(&id) => update_apply(),
                // Clicking a tool's name or description toggles its checkbox.
                _ if code == 0 && !busy && (ID_NAME..ID_DETAIL + ROWS as i32).contains(&id) => {
                    let row = (id % 100) as usize;
                    if row < ROWS && AVAILABLE.get()[row] && !busy {
                        let check = ID_CHECK + row as i32;
                        set_checked(check, !checked(check));
                        update_apply();
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DONE => {
            let text = Box::from_raw(lparam.0 as *mut String);
            MODEL.with_borrow_mut(|m| {
                if let Some(m) = m.as_mut() {
                    m.busy = false;
                }
            });
            refresh(false);
            // After refresh, so the outcome (including any failure) isn't replaced by a pending-changes line.
            set_text(ID_RESULT, &text);
            LRESULT(0)
        }
        // Coming back from Codex after approving hooks updates the window by itself.
        WM_ACTIVATE if wparam.0 & 0xffff != 0 && MODEL.with_borrow(|m| m.as_ref().is_some_and(|m| !m.busy)) => {
            refresh(false);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let Some(p) = PAINT.get() else { return DefWindowProcW(hwnd, msg, wparam, lparam) };
            let hdc = HDC(wparam.0 as *mut _);
            let id = GetDlgCtrlID(HWND(lparam.0 as *mut _));
            let row = (id % 100) as usize;
            let pending = id / 100 == 3 && row < ROWS && PENDING.get()[row];
            let muted = id == ID_INTRO || (id / 100 == 3) || (id / 100 == 2 && row < ROWS && !AVAILABLE.get()[row]);
            SetTextColor(hdc, if pending { p.accent } else if muted { p.muted } else { p.text });
            SetBkMode(hdc, TRANSPARENT);
            LRESULT(p.background.0 as isize)
        }
        WM_ERASEBKGND => {
            let Some(p) = PAINT.get() else { return DefWindowProcW(hwnd, msg, wparam, lparam) };
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            FillRect(HDC(wparam.0 as *mut _), &rect, p.background);
            LRESULT(1)
        }
        WM_CLOSE => {
            close();
            LRESULT(0)
        }
        WM_DESTROY => {
            WINDOW.set(None);
            SHOWING.set(false);
            PENDING.set([false; ROWS]);
            DETAILS.set(Vec::new());
            MODEL.set(None);
            if let Some(p) = PAINT.take() {
                let _ = DeleteObject(p.background.into());
                for font in [p.font, p.bold, p.title, p.mono] {
                    let _ = DeleteObject(font.into());
                }
            }
            let marker = setup::marker_path();
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(marker, "");
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
