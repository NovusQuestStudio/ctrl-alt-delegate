//! Connects Ctrl Alt Delegate to VS Code, Claude Code, Codex, Devin and Antigravity.
//!
//! Detection only reads files. Changes go through each tool's own command-line installer
//! (`code --install-extension`, `claude plugin`, `codex plugin`), so each tool keeps managing its
//! own configuration. Devin has no such installer, so `cad devin connect` adds only our own entries
//! to the "hooks" section of Devin's config.json and leaves everything else in it as it was.

use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PLUGIN: &str = "ctrl-alt-delegate@ctrl-alt-delegate";
pub const MARKETPLACE: &str = "ctrl-alt-delegate";
pub const EXTENSION_ID: &str = "ctrl-alt-delegate.ctrl-alt-delegate-vscode";
pub const VSIX: &str = "ctrl-alt-delegate-vscode.vsix";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    VsCode,
    VsCodeInsiders,
    ClaudeCode,
    Codex,
    Devin,
    DevinDesktop,
    Antigravity,
    AntigravityIde,
}

impl Tool {
    pub const ALL: [Tool; 8] = [Tool::VsCode, Tool::VsCodeInsiders, Tool::ClaudeCode, Tool::Codex, Tool::Devin, Tool::DevinDesktop, Tool::Antigravity, Tool::AntigravityIde];

    pub fn label(self) -> &'static str {
        match self {
            Tool::VsCode => "VS Code",
            Tool::VsCodeInsiders => "VS Code Insiders",
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Devin => "Devin agent",
            Tool::DevinDesktop => "Devin Desktop editor",
            Tool::Antigravity => "Antigravity agent",
            Tool::AntigravityIde => "Antigravity editor",
        }
    }

    /// Connected through the VS Code extension (copied in, so its folder doesn't matter).
    pub fn uses_extension(self) -> bool {
        matches!(self, Tool::VsCode | Tool::VsCodeInsiders | Tool::DevinDesktop | Tool::AntigravityIde)
    }
}

/// What is on this PC for one tool.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// The tool's command-line installer; `None` means the tool was not found.
    pub cli: Option<PathBuf>,
    pub installed: bool,
    /// The folder the plugin marketplace is registered from (Claude Code and Codex).
    pub linked_to: Option<PathBuf>,
    /// Codex only: (approved, expected) hook entries.
    pub hooks: Option<(usize, usize)>,
}

/// Where to look. Built from the real environment, or by hand in tests.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub home: PathBuf,
    pub local_app_data: PathBuf,
    pub app_data: PathBuf,
    pub program_files: PathBuf,
    pub path: Vec<PathBuf>,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
    /// Codex Desktop package folders (Microsoft Store), newest first.
    pub codex_packages: Vec<PathBuf>,
    /// This app's cad.exe, which Devin's hooks run.
    pub cad: Option<PathBuf>,
}

impl Env {
    pub fn system() -> Env {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let home = var("USERPROFILE").or_else(|| var("HOME")).unwrap_or_default();
        Env {
            local_app_data: var("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local")),
            app_data: var("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming")),
            program_files: var("ProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files")),
            path: std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(),
            claude_dir: var("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")),
            codex_dir: var("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            codex_packages: codex_packages(),
            cad: std::env::current_exe().ok().map(|exe| exe.with_file_name("cad.exe")).filter(|p| p.is_file()),
            home,
        }
    }
}

/// The folder that holds the plugin marketplaces: the unzipped release folder, or the repository
/// root when running a development build from `apps/tray/target/<profile>`.
pub fn bundle_root(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .take(6)
        .find(|dir| dir.join(".claude-plugin").join("marketplace.json").is_file() && dir.join(".agents").join("plugins").join("marketplace.json").is_file())
        .map(Path::to_path_buf)
}

pub fn vsix(root: &Path) -> Option<PathBuf> {
    [root.join(VSIX), root.join("apps").join("vscode").join(VSIX)].into_iter().find(|p| p.is_file())
}

/// Plugin files the agents run; missing in a source checkout that hasn't been built yet.
pub fn plugin_ready(root: &Path, tool: Tool) -> bool {
    let dir = match tool {
        Tool::ClaudeCode => "claude-code",
        Tool::Codex => "codex",
        Tool::Antigravity => "antigravity",
        _ => return true,
    };
    root.join("plugins").join(dir).join("bin").join("cad.exe").is_file()
}

fn on_path(env: &Env, names: &[&str]) -> Option<PathBuf> {
    env.path.iter().flat_map(|dir| names.iter().map(move |n| dir.join(n))).find(|p| p.is_file())
}

fn first_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.is_file())
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Compares folders the way Windows does: no `\\?\` prefix, no trailing slash, any case.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    fn norm(p: &Path) -> String {
        let s = p.to_string_lossy().replace('/', "\\");
        s.trim_start_matches(r"\\?\").trim_end_matches('\\').to_lowercase()
    }
    norm(a) == norm(b)
}

pub fn detect(tool: Tool, env: &Env, root: Option<&Path>) -> Status {
    match tool {
        Tool::VsCode | Tool::VsCodeInsiders => {
            let (folder, cmd, data) = if tool == Tool::VsCode {
                ("Microsoft VS Code", "code.cmd", ".vscode")
            } else {
                ("Microsoft VS Code Insiders", "code-insiders.cmd", ".vscode-insiders")
            };
            let cli = first_file([env.local_app_data.join("Programs").join(folder).join("bin").join(cmd), env.program_files.join(folder).join("bin").join(cmd)])
                .or_else(|| on_path(env, &[cmd]));
            Status { cli, installed: vscode_extension_installed(&env.home.join(data).join("extensions")), ..Status::default() }
        }
        Tool::ClaudeCode => {
            let cli = on_path(env, &["claude.exe"]).or_else(|| first_file([env.home.join(".local").join("bin").join("claude.exe")]));
            let mut status = claude_status(&env.claude_dir);
            status.cli = cli;
            status
        }
        Tool::Codex => {
            let cli = first_file(env.codex_packages.iter().map(|p| p.join("app").join("resources").join("codex.exe"))).or_else(|| on_path(env, &["codex.exe", "codex.cmd"]));
            let expected = root.map_or_else(Vec::new, |r| codex_hooks(&r.join("plugins").join("codex").join("hooks").join("hooks.json")));
            let config = std::fs::read_to_string(env.codex_dir.join("config.toml")).unwrap_or_default();
            let mut status = codex_status(&config, &expected);
            status.cli = cli;
            status
        }
        Tool::Devin => {
            // Devin Desktop's agent sidebar and the Devin CLI share one config; our cad.exe edits it.
            let local = &env.local_app_data;
            let found = devin_config(env).is_file()
                || first_file([local.join("Programs").join("Devin").join("Devin.exe"), local.join("devin").join("cli").join("bin").join("devin.exe"), local.join("devin").join("bin").join("devin.exe")]).is_some()
                || on_path(env, &["devin.exe"]).is_some();
            let mut status = devin_status(&devin_config(env));
            status.cli = env.cad.clone().filter(|_| found);
            status
        }
        Tool::Antigravity => {
            // The IDE ships agy in ~/.gemini/bin; the CLI comes from winget or Google's installer.
            let winget = env.local_app_data.join("Microsoft").join("WinGet").join("Packages").join("Google.AntigravityCLI_Microsoft.Winget.Source_8wekyb3d8bbwe").join("agy.exe");
            let cli = first_file([env.home.join(".gemini").join("bin").join("agy.exe"), winget, env.local_app_data.join("agy").join("bin").join("agy.exe")]).or_else(|| on_path(env, &["agy.exe"]));
            // agy plugin install copies the plugin here, so its source folder doesn't matter.
            let installed = env.home.join(".gemini").join("config").join("plugins").join("ctrl-alt-delegate").join("hooks.json").is_file();
            Status { cli, installed, ..Status::default() }
        }
        Tool::AntigravityIde => {
            let cli = first_file([env.local_app_data.join("Programs").join("Antigravity IDE").join("bin").join("antigravity-ide.cmd")]).or_else(|| on_path(env, &["antigravity-ide.cmd"]));
            Status { cli, installed: vscode_extension_installed(&env.home.join(".antigravity-ide").join("extensions")), ..Status::default() }
        }
        Tool::DevinDesktop => {
            let cli = first_file([env.local_app_data.join("Programs").join("Devin").join("bin").join("devin-desktop.cmd")]).or_else(|| on_path(env, &["devin-desktop.cmd"]));
            Status { cli, installed: vscode_extension_installed(&env.home.join(".devin").join("extensions")), ..Status::default() }
        }
    }
}

/// Devin's user settings file: %APPDATA%\devin\config.json on Windows, ~/.config/devin/config.json elsewhere.
pub fn devin_config(env: &Env) -> PathBuf {
    if cfg!(windows) {
        env.app_data.join("devin").join("config.json")
    } else {
        env.home.join(".config").join("devin").join("config.json")
    }
}

/// Devin events we listen to; Devin uses Claude Code's names.
const DEVIN_EVENTS: [&str; 6] = ["SessionStart", "UserPromptSubmit", "PostToolUse", "PermissionRequest", "Stop", "SessionEnd"];

fn devin_command(cad: &Path) -> String {
    format!("\"{}\" hook devin", cad.display())
}

/// The cad.exe in one of our hook commands, or None for anyone else's hook.
fn our_cad(command: &str) -> Option<PathBuf> {
    let (path, rest) = command.trim().strip_prefix('"')?.split_once('"')?;
    (rest.trim() == "hook devin" && Path::new(path).file_name()?.eq_ignore_ascii_case("cad.exe")).then(|| PathBuf::from(path))
}

fn devin_hook_commands(config: &Value) -> Vec<String> {
    let Some(events) = config.get("hooks").and_then(Value::as_object) else { return Vec::new() };
    events
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn devin_status(config_path: &Path) -> Status {
    let config = read_json(config_path).unwrap_or(Value::Null);
    let ours: Vec<PathBuf> = devin_hook_commands(&config).iter().filter_map(|c| our_cad(c)).collect();
    // Linked to the app folder holding that cad.exe, so a moved folder shows as "relink".
    let linked_to = ours.first().map(|cad| bundle_root(cad).unwrap_or_else(|| cad.parent().unwrap_or(cad).to_path_buf()));
    Status { installed: !ours.is_empty(), linked_to, ..Status::default() }
}

/// Adds (with `cad`) or removes (without) our hooks in Devin's config.json. Other keys and other
/// hooks stay as they were. The first change saves the original next to it as config.json.cad-backup.
pub fn devin_hooks(config_path: &Path, cad: Option<&Path>) -> Result<String, String> {
    let text = match std::fs::read_to_string(config_path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read {}: {e}", config_path.display())),
    };
    let mut config: Value = match &text {
        Some(t) => serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|_| format!("{} isn't plain JSON (comments?), so it was left alone", config_path.display()))?,
        None => serde_json::json!({}),
    };
    let root = config.as_object_mut().ok_or("Devin's config.json isn't a JSON object, so it was left alone")?;
    let hooks = root.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let events = hooks.as_object_mut().ok_or("the \"hooks\" entry in Devin's config.json isn't an object, so it was left alone")?;
    // Drop our entries everywhere first, so connecting twice or from a moved folder leaves one copy.
    for list in events.values_mut().filter_map(Value::as_array_mut) {
        for group in list.iter_mut() {
            if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                hooks.retain(|h| h.get("command").and_then(Value::as_str).and_then(our_cad).is_none());
            }
        }
        list.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|h| !h.is_empty()));
    }
    events.retain(|_, list| list.as_array().is_none_or(|l| !l.is_empty()));
    if let Some(cad) = cad {
        let command = devin_command(cad);
        for event in DEVIN_EVENTS {
            if let Some(list) = events.entry(event).or_insert_with(|| serde_json::json!([])).as_array_mut() {
                list.push(serde_json::json!({"hooks": [{"type": "command", "command": command, "timeout": 5}]}));
            }
        }
    }
    if events.is_empty() {
        root.remove("hooks");
    }
    if let Some(original) = &text {
        let backup = config_path.with_extension("json.cad-backup");
        if !backup.exists() {
            std::fs::write(&backup, original).map_err(|e| format!("can't save a backup: {e}"))?;
        }
    }
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::settings::write_atomic(config_path, &(serde_json::to_string_pretty(&config).unwrap() + "\n")).map_err(|e| e.to_string())?;
    Ok(if cad.is_some() { "Devin hooks added. Start a new chat in Devin to use them." } else { "Devin hooks removed." }.to_string())
}

fn vscode_extension_installed(extensions: &Path) -> bool {
    // extensions.json is VS Code's own list; an uninstalled extension's folder lingers until it restarts.
    if let Some(list) = read_json(&extensions.join("extensions.json")).and_then(|j| j.as_array().cloned()) {
        return list.iter().any(|e| e.pointer("/identifier/id").and_then(Value::as_str).is_some_and(|id| id.eq_ignore_ascii_case(EXTENSION_ID)));
    }
    // Older layout: folders, with uninstalled ones listed in .obsolete.
    let obsolete = read_json(&extensions.join(".obsolete")).unwrap_or(Value::Null);
    let prefix = format!("{EXTENSION_ID}-");
    std::fs::read_dir(extensions)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.starts_with(&prefix) && e.path().is_dir() && obsolete.get(&name).is_none()
            })
        })
        .unwrap_or(false)
}

fn claude_status(dir: &Path) -> Status {
    let enabled = read_json(&dir.join("settings.json")).and_then(|s| s.pointer("/enabledPlugins").and_then(|p| p.get(PLUGIN)).and_then(Value::as_bool)).unwrap_or(false);
    let listed = read_json(&dir.join("plugins").join("installed_plugins.json")).is_some_and(|j| j.get("plugins").unwrap_or(&j).get(PLUGIN).is_some());
    let linked_to = read_json(&dir.join("plugins").join("known_marketplaces.json")).and_then(|j| {
        let m = j.get(MARKETPLACE)?;
        m.get("installLocation").or_else(|| m.pointer("/source/path")).and_then(Value::as_str).map(PathBuf::from)
    });
    Status { installed: enabled && listed, linked_to, ..Status::default() }
}

/// Reads the few Codex settings we need from config.toml without a TOML library.
/// Codex records each approved hook under `[hooks.state."<plugin>:<file>:<event>:<group>:<index>"]`.
/// Only approvals for the hooks the current plugin defines count, so leftovers from an older version don't.
fn codex_status(config: &str, expected_hooks: &[String]) -> Status {
    let plugin_table = format!("[plugins.\"{PLUGIN}\"]");
    let marketplace_table = format!("[marketplaces.{MARKETPLACE}]");
    let hook_prefix = format!("[hooks.state.\"{PLUGIN}:");
    let (mut table, mut installed, mut linked_to) = (String::new(), false, None);
    let mut approved = std::collections::BTreeSet::new();
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line.to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if table == plugin_table && key == "enabled" {
            installed = value == "true";
        } else if table == marketplace_table && key == "source" {
            linked_to = Some(PathBuf::from(value.trim_matches(|c| c == '\'' || c == '"')));
        } else if key == "trusted_hash" {
            let id = table.strip_prefix(&hook_prefix).and_then(|rest| rest.strip_suffix("\"]"));
            if let Some(id) = id.filter(|id| expected_hooks.iter().any(|e| e == id)) {
                approved.insert(id.to_string());
            }
        }
    }
    Status { installed, linked_to, hooks: Some((approved.len(), expected_hooks.len())), ..Status::default() }
}

/// The hook ids Codex will ask to approve for our hooks.json, e.g. `hooks/hooks.json:post_tool_use:0:0`.
fn codex_hooks(hooks_json: &Path) -> Vec<String> {
    let Some(json) = read_json(hooks_json) else { return Vec::new() };
    let Some(events) = json.get("hooks").and_then(Value::as_object) else { return Vec::new() };
    let mut ids = Vec::new();
    for (event, groups) in events {
        let snake: String = event.chars().enumerate().flat_map(|(i, c)| {
            let sep = (i > 0 && c.is_ascii_uppercase()).then_some('_');
            sep.into_iter().chain(std::iter::once(c.to_ascii_lowercase()))
        }).collect();
        for (g, group) in groups.as_array().into_iter().flatten().enumerate() {
            for h in 0..group.get("hooks").and_then(Value::as_array).map_or(0, Vec::len) {
                ids.push(format!("hooks/hooks.json:{snake}:{g}:{h}"));
            }
        }
    }
    ids
}

/// Codex Desktop is a Microsoft Store app; its package folder is listed in the user's registry.
#[cfg(windows)]
fn codex_packages() -> Vec<PathBuf> {
    use windows::core::{w, HSTRING, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ};
    let mut found: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        let base = w!("Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages");
        if RegOpenKeyExW(HKEY_CURRENT_USER, base, None, KEY_READ, &mut key) != ERROR_SUCCESS {
            return Vec::new();
        }
        let mut index = 0;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(key, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) != ERROR_SUCCESS {
                break;
            }
            index += 1;
            let package = String::from_utf16_lossy(&name[..len as usize]);
            // e.g. OpenAI.Codex_26.1111.9664.0_x64__2p2nqsd0c76g0 (not the separate runtime packages).
            let Some(rest) = package.strip_prefix("OpenAI.Codex_") else { continue };
            let version: Vec<u32> = rest.split('_').next().unwrap_or("").split('.').filter_map(|n| n.parse().ok()).collect();
            let mut buf = [0u16; 1024];
            let mut size = (buf.len() * 2) as u32;
            let value = RegGetValueW(key, &HSTRING::from(package.as_str()), w!("PackageRootFolder"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size));
            if value == ERROR_SUCCESS {
                let chars = (size as usize / 2).saturating_sub(1);
                let dir = PathBuf::from(String::from_utf16_lossy(&buf[..chars]));
                if dir.is_dir() {
                    found.push((version, dir));
                }
            }
        }
        let _ = RegCloseKey(key);
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, dir)| dir).collect()
}

#[cfg(not(windows))]
fn codex_packages() -> Vec<PathBuf> {
    Vec::new()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Install,
    /// Installed, but registered from a different folder (the app was moved).
    Relink,
    Remove,
}

impl Change {
    pub fn verb(self) -> &'static str {
        match self {
            Change::Install => "connect",
            Change::Relink => "relink",
            Change::Remove => "disconnect",
        }
    }
}

/// The other app folder a tool is connected to, if it isn't this one.
fn other_folder<'a>(tool: Tool, status: &'a Status, root: &Path) -> Option<&'a Path> {
    status.linked_to.as_deref().filter(|p| !tool.uses_extension() && !same_folder(p, root))
}

/// The folder as Windows shows it, without the `\\?\` prefix some tools store.
fn plain(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\"))
}

pub fn change(tool: Tool, status: &Status, wanted: bool, root: &Path) -> Option<Change> {
    // Relink on its own only when the other folder is gone (the app was moved). Another copy that
    // still exists keeps working, so leave it be; untick and tick again to switch to this copy.
    let moved = other_folder(tool, status, root).is_some_and(|p| !plain(p).exists());
    match (wanted, status.installed) {
        (true, false) => Some(Change::Install),
        (true, true) if moved => Some(Change::Relink),
        (false, true) => Some(Change::Remove),
        _ => None,
    }
}

/// The commands for one change, in order. Each starts with the program to run.
pub fn commands(tool: Tool, change: Change, status: &Status, cli: &Path, root: &Path, vsix: Option<&Path>) -> Vec<Vec<OsString>> {
    let cmd = |args: &[&dyn AsRef<std::ffi::OsStr>]| -> Vec<OsString> { std::iter::once(cli.as_os_str().to_owned()).chain(args.iter().map(|a| a.as_ref().to_owned())).collect() };
    if tool == Tool::Devin {
        // `cli` is our own cad.exe. Connecting again also relinks a moved folder.
        return vec![cmd(&[&"devin", &if change == Change::Remove { "disconnect" } else { "connect" }])];
    }
    if tool == Tool::Antigravity {
        // agy copies the plugin into ~/.gemini/config/plugins, replacing an older copy.
        return match change {
            Change::Remove => vec![cmd(&[&"plugin", &"uninstall", &MARKETPLACE])],
            _ => vec![cmd(&[&"plugin", &"install", &root.join("plugins").join("antigravity")])],
        };
    }
    if tool.uses_extension() {
        return match (change, vsix) {
            (Change::Remove, _) => vec![cmd(&[&"--uninstall-extension", &EXTENSION_ID])],
            (_, Some(vsix)) => vec![cmd(&[&"--install-extension", &vsix, &"--force"])],
            (_, None) => Vec::new(),
        };
    }
    // Same shape for both agents; only the install/uninstall verbs differ.
    let (install, uninstall) = if tool == Tool::ClaudeCode { ("install", "uninstall") } else { ("add", "remove") };
    let plugin = |verb: &str| cmd(&[&"plugin", &verb, &PLUGIN]);
    let market = |verb: &str, arg: &dyn AsRef<std::ffi::OsStr>| cmd(&[&"plugin", &"marketplace", &verb, arg]);
    let registered_here = status.linked_to.as_deref().is_some_and(|p| same_folder(p, root));
    let mut out = Vec::new();
    match change {
        Change::Remove => {
            out.push(plugin(uninstall));
            if status.linked_to.is_some() {
                out.push(market("remove", &MARKETPLACE));
            }
        }
        Change::Install | Change::Relink => {
            if status.installed {
                out.push(plugin(uninstall));
            }
            if status.linked_to.is_some() && !registered_here {
                out.push(market("remove", &MARKETPLACE));
            }
            if registered_here {
                // Pick up a newer copy of the plugin files from the same folder.
                if tool == Tool::ClaudeCode {
                    out.push(market("update", &MARKETPLACE));
                }
            } else {
                out.push(market("add", &root));
            }
            out.push(plugin(install));
        }
    }
    out
}

/// One line under the tool's name in the setup window.
pub fn describe(tool: Tool, status: &Status, root: Option<&Path>, vsix_found: bool) -> String {
    if status.cli.is_none() {
        return match tool {
            Tool::ClaudeCode => "Not found. Install Claude Code, then click Refresh.".into(),
            Tool::Codex => "Not found. Install Codex, then click Refresh.".into(),
            Tool::Devin | Tool::DevinDesktop => "Not found. Install Devin Desktop or the Devin CLI, then click Refresh.".into(),
            Tool::Antigravity | Tool::AntigravityIde => "Not found. Install Antigravity, then click Refresh.".into(),
            _ => "Not found on this PC.".into(),
        };
    }
    let Some(root) = root else { return "Can't find the plugin files next to the app. Unzip the whole release folder.".into() };
    if tool.uses_extension() && !vsix_found && !status.installed {
        return "The extension file isn't next to the app. Unzip the whole release folder.".into();
    }
    if !plugin_ready(root, tool) && !status.installed {
        return "Plugin files aren't built yet. Run scripts\\build.ps1 first.".into();
    }
    if status.installed {
        if let Some(other) = other_folder(tool, status, root).map(plain) {
            return if other.exists() {
                format!("Connected, using another copy of the app ({}).", other.display())
            } else {
                format!("Connected to a folder that's gone ({}). Apply relinks it here.", other.display())
            };
        }
        return match (tool, status.hooks) {
            (Tool::Codex, Some((approved, expected))) if approved < expected => {
                "Connected, but its hooks aren't approved. In Codex: Settings → Hooks, approve Ctrl Alt Delegate, then Refresh.".into()
            }
            (Tool::Codex, _) => "Connected. Hooks approved.".into(),
            _ => "Connected.".into(),
        };
    }
    match tool {
        Tool::VsCode | Tool::VsCodeInsiders => "Not connected. Reload open VS Code windows after connecting.".into(),
        Tool::ClaudeCode => "Not connected. Restart open Claude Code sessions after connecting.".into(),
        Tool::Codex => "Not connected. After connecting, restart Codex and approve its hooks in Settings → Hooks.".into(),
        Tool::Devin => "Not connected. Covers Devin Desktop's agent sidebar and the Devin CLI. Start a new chat after connecting.".into(),
        Tool::DevinDesktop => "Not connected. Shows Devin Desktop while you edit. Reload its windows after connecting.".into(),
        Tool::Antigravity => "Not connected. Covers Antigravity's agent and CLI (Working and Ready only). Close Antigravity before changing this.".into(),
        Tool::AntigravityIde => "Not connected. Shows Antigravity while you edit. Reload its windows after connecting.".into(),
    }
}

/// What to tell the user when a change fails.
pub fn failure(tool: Tool, change: Change, error: &str) -> String {
    // Antigravity keeps its plugin folder open while it runs, so replacing the plugin fails.
    if tool == Tool::Antigravity && (error.contains("rename") || error.contains("Access is denied")) {
        return format!("{}: couldn't {} because Antigravity is open. Close it completely and click Apply again.", tool.label(), change.verb());
    }
    format!("{}: couldn't {} ({error}).", tool.label(), change.verb())
}

/// What to tell the user after a successful change.
pub fn after(tool: Tool, change: Change) -> &'static str {
    match (tool, change) {
        (_, Change::Remove) => "disconnected.",
        (Tool::ClaudeCode, _) => "connected. Restart open Claude Code sessions.",
        (Tool::Codex, _) => "connected. Restart Codex, then approve its hooks in Settings → Hooks.",
        (Tool::Devin, _) => "connected. Start a new chat in Devin.",
        (Tool::DevinDesktop, _) => "connected. Reload Devin Desktop windows.",
        (Tool::Antigravity, _) => "connected. Start a new conversation in Antigravity.",
        (Tool::AntigravityIde, _) => "connected. Reload Antigravity windows.",
        _ => "connected. Reload open VS Code windows.",
    }
}

/// A command as you'd type it in PowerShell: `& "C:\path\tool.exe" plugin add name`.
pub fn display(command: &[OsString]) -> String {
    let quote = |a: &OsString| {
        let a = a.to_string_lossy();
        if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "&()'\"`;,{}$@#|<>".contains(c)) {
            format!("\"{}\"", a.replace('`', "``").replace('"', "`\"").replace('$', "`$"))
        } else {
            a.into_owned()
        }
    };
    match command.split_first() {
        Some((program, args)) => std::iter::once(format!("& \"{}\"", program.to_string_lossy())).chain(args.iter().map(quote)).collect::<Vec<_>>().join(" "),
        None => String::new(),
    }
}

/// Runs a command with no console window. Returns its output, or the error to show.
pub fn run(command: &[OsString], timeout: Duration) -> Result<String, String> {
    let (program, args) = command.split_first().ok_or("empty command")?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let child = cmd.spawn().map_err(|e| format!("couldn't start {}: {e}", Path::new(program).display()))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx.recv_timeout(timeout).map_err(|_| "timed out".to_string())?.map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        let last = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("no details").chars().take(160).collect::<String>();
        Err(last)
    }
}

/// First run: the setup window opens once, until the user has closed it.
pub fn marker_path() -> PathBuf {
    crate::settings::state_dir().join("setup-seen")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cad-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn strings(cmds: &[Vec<OsString>]) -> Vec<String> {
        cmds.iter().map(|c| c[1..].iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ")).collect()
    }

    #[test]
    fn finds_the_bundle_from_release_and_dev_builds() {
        let root = temp("bundle");
        write(&root.join(".claude-plugin/marketplace.json"), "{}");
        write(&root.join(".agents/plugins/marketplace.json"), "{}");
        assert_eq!(bundle_root(&root.join("ctrl-alt-delegate.exe")).as_deref(), Some(root.as_path()));
        assert_eq!(bundle_root(&root.join("apps/tray/target/release/ctrl-alt-delegate.exe")).as_deref(), Some(root.as_path()));
        assert_eq!(bundle_root(&std::env::temp_dir().join("nowhere/app.exe")), None);
        assert_eq!(vsix(&root), None);
        write(&root.join("apps/vscode").join(VSIX), "x");
        assert_eq!(vsix(&root), Some(root.join("apps/vscode").join(VSIX)));
    }

    #[test]
    fn reads_codex_config_without_changing_it() {
        let config = format!(
            "[plugins.\"{PLUGIN}\"]\nenabled = true\n\n[marketplaces.{MARKETPLACE}]\nsource_type = \"local\"\nsource = '\\\\?\\C:\\Apps\\CAD'\n\n\
             [hooks.state.\"{PLUGIN}:hooks/hooks.json:stop:0:0\"]\ntrusted_hash = \"sha256:1\"\n\
             [hooks.state.\"{PLUGIN}:hooks/hooks.json:old_event:0:0\"]\ntrusted_hash = \"sha256:3\"\n\
             [hooks.state.\"other@x:hooks/hooks.json:stop:0:0\"]\ntrusted_hash = \"sha256:2\"\n"
        );
        let expected: Vec<String> = ["stop:0:0", "session_start:0:0"].iter().map(|e| format!("hooks/hooks.json:{e}")).collect();
        let s = codex_status(&config, &expected);
        assert!(s.installed);
        assert!(same_folder(s.linked_to.as_deref().unwrap(), Path::new("c:/apps/cad/")));
        // Our stop hook counts; another plugin's, and an approval for a hook we no longer have, don't.
        assert_eq!(s.hooks, Some((1, 2)));
        assert_eq!(codex_status("", &expected), Status { hooks: Some((0, 2)), ..Status::default() });
        assert!(!codex_status(&format!("[plugins.\"{PLUGIN}\"]\nenabled = false\n"), &expected).installed);
    }

    #[test]
    fn lists_codex_hook_ids_from_the_plugin() {
        let dir = temp("hooks");
        let file = dir.join("hooks.json");
        write(&file, r#"{"hooks":{"Stop":[{"hooks":[{"type":"command"}]}],"PostToolUse":[{"hooks":[{"type":"command"},{"type":"command"}]}]}}"#);
        let mut ids = codex_hooks(&file);
        ids.sort();
        assert_eq!(ids, ["hooks/hooks.json:post_tool_use:0:0", "hooks/hooks.json:post_tool_use:0:1", "hooks/hooks.json:stop:0:0"]);
        assert!(codex_hooks(&dir.join("missing.json")).is_empty());
    }

    #[test]
    fn reads_claude_and_vscode_state() {
        let dir = temp("claude");
        assert_eq!(claude_status(&dir), Status::default());
        write(&dir.join("settings.json"), &format!("\u{feff}{{\"enabledPlugins\": {{\"{PLUGIN}\": true}}}}"));
        write(&dir.join("plugins/installed_plugins.json"), &format!("{{\"version\": 2, \"plugins\": {{\"{PLUGIN}\": []}}}}"));
        write(&dir.join("plugins/known_marketplaces.json"), &format!("{{\"{MARKETPLACE}\": {{\"installLocation\": \"C:\\\\Apps\\\\CAD\"}}}}"));
        let s = claude_status(&dir);
        assert!(s.installed);
        assert_eq!(s.linked_to, Some(PathBuf::from(r"C:\Apps\CAD")));

        let ext = temp("vscode");
        assert!(!vscode_extension_installed(&ext));
        std::fs::create_dir_all(ext.join(format!("{EXTENSION_ID}-0.1.0"))).unwrap();
        assert!(vscode_extension_installed(&ext));
        write(&ext.join(".obsolete"), &format!("{{\"{EXTENSION_ID}-0.1.0\": true}}"));
        assert!(!vscode_extension_installed(&ext), "uninstalled, pending a VS Code restart");
        write(&ext.join("extensions.json"), &format!("[{{\"identifier\": {{\"id\": \"{EXTENSION_ID}\"}}}}]"));
        assert!(vscode_extension_installed(&ext), "extensions.json wins over the folder layout");
        write(&ext.join("extensions.json"), r#"[{"identifier": {"id": "other.extension"}}]"#);
        assert!(!vscode_extension_installed(&ext));
    }

    #[test]
    fn plans_changes_from_the_wanted_state() {
        let root = Path::new(r"C:\Apps\CAD");
        let here = Status { installed: true, linked_to: Some(PathBuf::from(r"\\?\C:\apps\cad\")), ..Status::default() };
        let moved = Status { installed: true, linked_to: Some(PathBuf::from(r"D:\Old")), ..Status::default() };
        assert_eq!(change(Tool::Codex, &Status::default(), true, root), Some(Change::Install));
        assert_eq!(change(Tool::Codex, &here, true, root), None);
        assert_eq!(change(Tool::Codex, &moved, true, root), Some(Change::Relink));
        assert_eq!(change(Tool::Codex, &moved, false, root), Some(Change::Remove));
        assert_eq!(change(Tool::Codex, &Status::default(), false, root), None);
        assert_eq!(change(Tool::VsCode, &moved, true, root), None, "VS Code copies the extension, so its folder doesn't matter");

        // Another copy of the app that still exists isn't relinked automatically, so unticking means
        // just "disconnect" and ticking again leaves nothing to do.
        let other_copy = temp("other-copy");
        let elsewhere = Status { installed: true, linked_to: Some(other_copy.clone()), ..Status::default() };
        assert_eq!(change(Tool::Codex, &elsewhere, true, root), None);
        assert_eq!(change(Tool::Codex, &elsewhere, false, root), Some(Change::Remove));
        let found = Status { cli: Some(PathBuf::from("x")), ..elsewhere };
        let here = temp("this-copy");
        write(&here.join("plugins/codex/bin/cad.exe"), "x");
        assert!(describe(Tool::Codex, &found, Some(&here), true).starts_with("Connected, using another copy"));
        let gone = Status { linked_to: Some(PathBuf::from(r"D:\Old")), ..found };
        assert!(describe(Tool::Codex, &gone, Some(&here), true).contains("Apply relinks it here"));
    }

    #[test]
    fn builds_each_tools_own_commands() {
        let root = Path::new(r"C:\Apps\CAD");
        let cli = Path::new("tool.exe");
        let fresh = Status::default();
        assert_eq!(strings(&commands(Tool::ClaudeCode, Change::Install, &fresh, cli, root, None)), [r"plugin marketplace add C:\Apps\CAD", "plugin install ctrl-alt-delegate@ctrl-alt-delegate"]);
        assert_eq!(strings(&commands(Tool::Codex, Change::Install, &fresh, cli, root, None)), [r"plugin marketplace add C:\Apps\CAD", "plugin add ctrl-alt-delegate@ctrl-alt-delegate"]);
        let registered = Status { linked_to: Some(root.to_path_buf()), ..Status::default() };
        assert_eq!(strings(&commands(Tool::ClaudeCode, Change::Install, &registered, cli, root, None)), ["plugin marketplace update ctrl-alt-delegate", "plugin install ctrl-alt-delegate@ctrl-alt-delegate"]);
        assert_eq!(strings(&commands(Tool::Codex, Change::Install, &registered, cli, root, None)), ["plugin add ctrl-alt-delegate@ctrl-alt-delegate"]);
        let moved = Status { installed: true, linked_to: Some(PathBuf::from(r"D:\Old")), ..Status::default() };
        assert_eq!(
            strings(&commands(Tool::Codex, Change::Relink, &moved, cli, root, None)),
            ["plugin remove ctrl-alt-delegate@ctrl-alt-delegate", "plugin marketplace remove ctrl-alt-delegate", r"plugin marketplace add C:\Apps\CAD", "plugin add ctrl-alt-delegate@ctrl-alt-delegate"]
        );
        assert_eq!(strings(&commands(Tool::ClaudeCode, Change::Remove, &moved, cli, root, None)), ["plugin uninstall ctrl-alt-delegate@ctrl-alt-delegate", "plugin marketplace remove ctrl-alt-delegate"]);
        let vsix = root.join(VSIX);
        assert_eq!(strings(&commands(Tool::VsCode, Change::Install, &fresh, cli, root, Some(&vsix))), [r"--install-extension C:\Apps\CAD\ctrl-alt-delegate-vscode.vsix --force"]);
        assert_eq!(strings(&commands(Tool::VsCodeInsiders, Change::Remove, &fresh, cli, root, None)), ["--uninstall-extension ctrl-alt-delegate.ctrl-alt-delegate-vscode"]);
        assert!(commands(Tool::VsCode, Change::Install, &fresh, cli, root, None).is_empty());
    }

    #[test]
    fn explains_each_state_and_the_codex_caveat() {
        let root = temp("describe");
        let found = Status { cli: Some(PathBuf::from("x")), ..Status::default() };
        assert!(describe(Tool::Codex, &Status::default(), Some(&root), true).starts_with("Not found"));
        assert!(describe(Tool::Codex, &found, None, true).contains("plugin files"));
        assert!(describe(Tool::Codex, &found, Some(&root), true).contains("Run scripts"));
        write(&root.join("plugins/codex/bin/cad.exe"), "x");
        assert!(describe(Tool::Codex, &found, Some(&root), true).contains("approve its hooks"));
        let pending = Status { installed: true, linked_to: Some(root.clone()), hooks: Some((0, 7)), ..found.clone() };
        assert!(describe(Tool::Codex, &pending, Some(&root), true).contains("Settings → Hooks"));
        let approved = Status { hooks: Some((7, 7)), ..pending };
        assert_eq!(describe(Tool::Codex, &approved, Some(&root), true), "Connected. Hooks approved.");
        assert!(describe(Tool::VsCode, &found, Some(&root), false).contains("extension file"));
        assert!(after(Tool::Codex, Change::Install).contains("approve"));
    }

    #[test]
    fn devin_hooks_change_only_our_entries() {
        let dir = temp("devin");
        let config = dir.join("devin").join("config.json");
        let theirs = r#"{"hooks": [{"type": "command", "command": "my-linter.exe", "timeout": 3}]}"#;
        let original = format!(r#"{{"version": 1, "permissions": {{"allow": ["Exec(rg)"]}}, "hooks": {{"Stop": [{theirs}]}}}}"#);
        write(&config, &original);
        let read = || serde_json::from_str::<Value>(&std::fs::read_to_string(&config).unwrap()).unwrap();

        let first = dir.join("Old App").join("cad.exe");
        devin_hooks(&config, Some(&first)).unwrap();
        let j = read();
        assert_eq!(j["version"], 1);
        assert_eq!(j["permissions"]["allow"][0], "Exec(rg)", "other settings are untouched");
        assert_eq!(j["hooks"]["Stop"].as_array().unwrap().len(), 2, "someone else's Stop hook stays");
        assert_eq!(j["hooks"]["Stop"][0]["hooks"][0]["command"], "my-linter.exe");
        assert_eq!(devin_hook_commands(&j).iter().filter_map(|c| our_cad(c)).count(), DEVIN_EVENTS.len());
        assert_eq!(std::fs::read_to_string(config.with_extension("json.cad-backup")).unwrap(), original, "the original is backed up");
        let status = devin_status(&config);
        assert!(status.installed);
        assert_eq!(status.linked_to.as_deref(), Some(dir.join("Old App").as_path()));

        // Connecting from a moved folder replaces our entries instead of adding a second set.
        let moved = dir.join("New App").join("cad.exe");
        devin_hooks(&config, Some(&moved)).unwrap();
        let ours: Vec<PathBuf> = devin_hook_commands(&read()).iter().filter_map(|c| our_cad(c)).collect();
        assert_eq!(ours.len(), DEVIN_EVENTS.len());
        assert!(ours.iter().all(|p| p == &moved));
        assert_eq!(std::fs::read_to_string(config.with_extension("json.cad-backup")).unwrap(), original, "the first backup is kept");

        devin_hooks(&config, None).unwrap();
        let j = read();
        assert!(!devin_status(&config).installed);
        assert_eq!(j["hooks"], serde_json::json!({"Stop": [serde_json::from_str::<Value>(theirs).unwrap()]}));
        assert_eq!(j["permissions"]["allow"][0], "Exec(rg)");
    }

    #[test]
    fn devin_hooks_create_a_config_and_refuse_files_they_cannot_read_safely() {
        let dir = temp("devin-new");
        let config = dir.join("devin").join("config.json");
        let cad = dir.join("cad.exe");
        devin_hooks(&config, Some(&cad)).unwrap();
        assert!(devin_status(&config).installed);
        devin_hooks(&config, None).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(&config).unwrap()).unwrap(), serde_json::json!({}), "no empty hooks left behind");

        let commented = dir.join("commented.json");
        write(&commented, "{\n  // my settings\n  \"version\": 1\n}\n");
        assert!(devin_hooks(&commented, Some(&cad)).unwrap_err().contains("left alone"));
        assert_eq!(std::fs::read_to_string(&commented).unwrap(), "{\n  // my settings\n  \"version\": 1\n}\n");

        assert_eq!(our_cad(r#""C:\My Apps\cad.exe" hook devin"#), Some(PathBuf::from(r"C:\My Apps\cad.exe")));
        assert_eq!(our_cad(r#""C:\x\cad.exe" hook codex"#), None);
        assert_eq!(our_cad("cad.exe hook devin"), None);
        assert_eq!(our_cad(r#""C:\x\other.exe" hook devin"#), None);
        let status = Status { cli: Some(cad.clone()), ..Status::default() };
        assert_eq!(commands(Tool::Devin, Change::Install, &status, &cad, &dir, None)[0][1..], [OsString::from("devin"), OsString::from("connect")]);
        assert_eq!(commands(Tool::Devin, Change::Remove, &status, &cad, &dir, None)[0][2], OsString::from("disconnect"));
    }

    #[test]
    fn antigravity_uses_its_plugin_command() {
        let root = Path::new(r"C:\Apps\CAD");
        let agy = Path::new("agy.exe");
        assert_eq!(strings(&commands(Tool::Antigravity, Change::Install, &Status::default(), agy, root, None)), [r"plugin install C:\Apps\CAD\plugins\antigravity"]);
        assert_eq!(strings(&commands(Tool::Antigravity, Change::Remove, &Status::default(), agy, root, None)), ["plugin uninstall ctrl-alt-delegate"]);
        let vsix = root.join(VSIX);
        assert_eq!(strings(&commands(Tool::AntigravityIde, Change::Install, &Status::default(), agy, root, Some(&vsix))), [r"--install-extension C:\Apps\CAD\ctrl-alt-delegate-vscode.vsix --force"]);
        let installed = Status { installed: true, ..Status::default() };
        assert_eq!(change(Tool::Antigravity, &installed, true, root), None, "a copied plugin never needs relinking");
        assert!(failure(Tool::Antigravity, Change::Install, "failed to copy plugin: rename C:/x").contains("Antigravity is open"));
        assert_eq!(failure(Tool::Codex, Change::Remove, "boom"), "Codex: couldn't disconnect (boom).");
    }

    #[test]
    fn shows_commands_ready_for_powershell() {
        let cmd: Vec<OsString> = [r"C:\Program Files\Codex\codex.exe", "plugin", "marketplace", "add", r"C:\My Apps\CAD"].map(OsString::from).to_vec();
        assert_eq!(display(&cmd), r#"& "C:\Program Files\Codex\codex.exe" plugin marketplace add "C:\My Apps\CAD""#);
        let odd: Vec<OsString> = ["tool.exe", "a$b", ""].map(OsString::from).to_vec();
        assert_eq!(display(&odd), r#"& "tool.exe" "a`$b" """#);
        assert_eq!(display(&[]), "");
    }

    #[test]
    fn runs_commands_and_reports_failures() {
        let ok = run(&["cmd".into(), "/c".into(), "echo hello".into()], Duration::from_secs(10)).unwrap();
        assert!(ok.contains("hello"));
        let err = run(&["cmd".into(), "/c".into(), "echo first & echo bad thing 1>&2 & exit 3".into()], Duration::from_secs(10)).unwrap_err();
        assert_eq!(err, "bad thing");
        assert!(run(&["definitely-not-a-program-cad".into()], Duration::from_secs(5)).unwrap_err().starts_with("couldn't start"));
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Prints what the setup window would show on this PC. Read-only. `cargo test live_detect -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_detect() {
        let env = Env::system();
        let root = bundle_root(&std::env::current_exe().unwrap());
        println!("root: {root:?}");
        for tool in Tool::ALL {
            let s = detect(tool, &env, root.as_deref());
            println!("{}: {:?}\n    {}", tool.label(), s, describe(tool, &s, root.as_deref(), root.as_deref().and_then(vsix).is_some()));
        }
    }
}
