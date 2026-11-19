//! Local IPC: the service's per-user named pipe (one JSON line per request) and raw connections to Discord.

use serde_json::Value;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const MAX_REQUEST: usize = 32 * 1024;
const MAX_RESPONSE: usize = 256 * 1024;
const MAX_HANDLERS: usize = 64;

/// The service endpoint. `CAD_SOCKET` overrides it (tests); otherwise it is derived from the user name,
/// exactly like `socketPath()` in src/client.ts, which the VS Code extension uses.
pub fn service_path() -> String {
    if let Ok(path) = std::env::var("CAD_SOCKET") {
        return path;
    }
    let id = &crate::sha256::hex(imp::username().as_bytes())[..12];
    imp::default_path(id)
}

/// A connected, non-blocking-readable stream (named pipe on Windows, Unix socket elsewhere).
pub struct Conn {
    inner: imp::Stream,
}

impl Conn {
    pub fn open(path: &str) -> std::io::Result<Conn> {
        imp::open(path).map(|inner| Conn { inner })
    }
    /// Read whatever is available without blocking. Ok(0) means nothing yet; Err means the peer is gone.
    pub fn read_available(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        imp::read_available(&mut self.inner, buf)
    }
    pub fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.inner.write_all(data)?;
        self.inner.flush()
    }
}

/// Read bytes until a newline, the peer closing, `max` bytes, or the deadline.
fn read_line(conn: &mut Conn, deadline: Instant, max: usize) -> Result<String, String> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match conn.read_available(&mut buf) {
            Ok(0) => {
                if Instant::now() > deadline {
                    return Err("timed out".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                if let Some(end) = data.iter().position(|b| *b == b'\n') {
                    data.truncate(end);
                    break;
                }
                if data.len() > max {
                    return Err("message too large".into());
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8(data).map_err(|_| "invalid UTF-8".into())
}

#[derive(Debug)]
pub enum RequestError {
    NotRunning,
    Failed(String),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::NotRunning => write!(f, "Ctrl Alt Delegate is not running"),
            RequestError::Failed(e) => write!(f, "{e}"),
        }
    }
}

/// Send one request and wait for its one-line JSON reply.
pub fn request(path: &str, message: &Value, timeout: Duration) -> Result<Value, RequestError> {
    let deadline = Instant::now() + timeout;
    let mut conn = loop {
        match Conn::open(path) {
            Ok(c) => break c,
            Err(e) if imp::busy(&e) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(e) if imp::absent(&e) => return Err(RequestError::NotRunning),
            Err(e) => return Err(RequestError::Failed(e.to_string())),
        }
    };
    conn.write_all(format!("{message}\n").as_bytes()).map_err(|e| RequestError::Failed(e.to_string()))?;
    let line = read_line(&mut conn, deadline, MAX_RESPONSE).map_err(RequestError::Failed)?;
    let value: Value = serde_json::from_str(&line).map_err(|_| RequestError::Failed("Invalid service response".into()))?;
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(value)
    } else {
        Err(RequestError::Failed(value.get("error").and_then(Value::as_str).unwrap_or("Request failed").to_string()))
    }
}

pub type Handler = Arc<dyn Fn(&str) -> String + Send + Sync>;

pub struct Server {
    stop: Arc<AtomicBool>,
    path: String,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    /// Listen on `path`. Fails with "already running" if another process owns it.
    pub fn start(path: &str, handler: Handler) -> Result<Server, String> {
        let listener = imp::Listener::bind(path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("ipc-accept".into())
            .spawn(move || {
                let mut listener = listener;
                while !flag.load(Ordering::SeqCst) {
                    let Ok(mut conn) = listener.accept() else { continue };
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_HANDLERS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        continue; // Dropping closes the connection.
                    }
                    let (handler, active) = (handler.clone(), active.clone());
                    let _ = std::thread::Builder::new().name("ipc-request".into()).spawn(move || {
                        let reply = match read_line(&mut conn, Instant::now() + Duration::from_secs(3), MAX_REQUEST) {
                            Ok(line) => handler(&line),
                            Err(e) => serde_json::json!({"ok": false, "error": e}).to_string(),
                        };
                        let _ = conn.write_all(format!("{reply}\n").as_bytes());
                        imp::finish(&mut conn.inner);
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Server { stop, path: path.to_string(), thread: Some(thread) })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = Conn::open(&self.path); // Wake the blocking accept.
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
mod imp {
    use std::fs::File;
    use std::io::Read;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, INVALID_HANDLE_VALUE};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW};
    use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::Storage::FileSystem::{FlushFileBuffers, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PeekNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::System::WindowsProgramming::GetUserNameW;

    pub type Stream = File;

    pub fn default_path(id: &str) -> String {
        format!(r"\\.\pipe\ctrl-alt-delegate-{id}")
    }

    pub fn username() -> String {
        let mut buf = [0u16; 257];
        let mut len = buf.len() as u32;
        unsafe {
            if GetUserNameW(Some(PWSTR(buf.as_mut_ptr())), &mut len).is_ok() && len > 0 {
                return String::from_utf16_lossy(&buf[..len as usize - 1]);
            }
        }
        std::env::var("USERNAME").unwrap_or_default()
    }

    pub fn open(path: &str) -> std::io::Result<File> {
        std::fs::OpenOptions::new().read(true).write(true).open(path)
    }
    pub fn busy(e: &std::io::Error) -> bool {
        e.raw_os_error() == Some(231) // ERROR_PIPE_BUSY
    }
    pub fn absent(e: &std::io::Error) -> bool {
        e.raw_os_error() == Some(2) // ERROR_FILE_NOT_FOUND
    }

    /// Synchronous pipe handles serialize reads and writes, so never block in ReadFile: peek first.
    pub fn read_available(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut available = 0u32;
        unsafe { PeekNamedPipe(HANDLE(file.as_raw_handle()), None, 0, None, Some(&mut available), None) }
            .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xffff))?;
        if available == 0 {
            return Ok(0);
        }
        let n = buf.len().min(available as usize);
        file.read(&mut buf[..n])
    }

    pub fn finish(file: &mut File) {
        unsafe {
            let handle = HANDLE(file.as_raw_handle());
            let _ = FlushFileBuffers(handle);
            let _ = DisconnectNamedPipe(handle);
        }
    }

    /// Security descriptor allowing only the current user and SYSTEM.
    fn owner_only() -> Result<PSECURITY_DESCRIPTOR, String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
            let mut size = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
            let mut buffer = vec![0u8; size as usize];
            let result = GetTokenInformation(token, TokenUser, Some(buffer.as_mut_ptr() as _), size, &mut size);
            let _ = CloseHandle(token);
            result.map_err(|e| e.to_string())?;
            let user = &*(buffer.as_ptr() as *const TOKEN_USER);
            let mut sid = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(|e| e.to_string())?;
            let sddl = format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", sid.to_string().unwrap_or_default());
            let _ = LocalFree(Some(HLOCAL(sid.0 as _)));
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(sddl), 1, &mut descriptor, None).map_err(|e| e.to_string())?;
            Ok(descriptor)
        }
    }

    pub struct Listener {
        name: HSTRING,
        descriptor: PSECURITY_DESCRIPTOR,
        next: Option<HANDLE>,
    }
    // The descriptor is only read by CreateNamedPipeW on the accept thread.
    unsafe impl Send for Listener {}

    impl Listener {
        fn create(&self, first: bool) -> Result<HANDLE, String> {
            let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.descriptor.0, bInheritHandle: false.into() };
            let mode = if first { PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_DUPLEX };
            let handle = unsafe {
                CreateNamedPipeW(PCWSTR(self.name.as_ptr()), mode, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    PIPE_UNLIMITED_INSTANCES, 65536, 65536, 0, Some(&attributes))
            };
            if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
                let error = std::io::Error::last_os_error();
                return Err(if first && error.raw_os_error() == Some(5) { "Presence service is already running".into() } else { error.to_string() });
            }
            Ok(handle)
        }
        pub fn bind(path: &str) -> Result<Listener, String> {
            let mut listener = Listener { name: HSTRING::from(path), descriptor: owner_only()?, next: None };
            listener.next = Some(listener.create(true)?);
            Ok(listener)
        }
        pub fn accept(&mut self) -> Result<super::Conn, String> {
            let handle = match self.next.take() {
                Some(h) => h,
                None => self.create(false)?,
            };
            let connected = unsafe { ConnectNamedPipe(handle, None) };
            if let Err(e) = connected {
                if e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                    unsafe { let _ = CloseHandle(handle); }
                    return Err(e.to_string());
                }
            }
            self.next = self.create(false).ok();
            Ok(super::Conn { inner: unsafe { File::from_raw_handle(handle.0) } })
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            unsafe {
                if let Some(h) = self.next.take() {
                    let _ = CloseHandle(h);
                }
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub type Stream = UnixStream;

    pub fn default_path(id: &str) -> String {
        std::env::temp_dir().join(format!("ctrl-alt-delegate-{id}")).join("service.sock").to_string_lossy().into_owned()
    }
    pub fn username() -> String {
        std::env::var("USER").unwrap_or_default()
    }
    pub fn open(path: &str) -> std::io::Result<UnixStream> {
        let s = UnixStream::connect(path)?;
        s.set_nonblocking(true)?;
        Ok(s)
    }
    pub fn busy(_: &std::io::Error) -> bool {
        false
    }
    pub fn absent(e: &std::io::Error) -> bool {
        matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused)
    }
    pub fn read_available(s: &mut UnixStream, buf: &mut [u8]) -> std::io::Result<usize> {
        match s.read(buf) {
            Ok(0) => Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(e),
        }
    }
    pub fn finish(s: &mut UnixStream) {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
    pub struct Listener(UnixListener);
    impl Listener {
        pub fn bind(path: &str) -> Result<Listener, String> {
            let p = std::path::Path::new(path);
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            }
            if UnixStream::connect(p).is_ok() {
                return Err("Presence service is already running".into());
            }
            let _ = std::fs::remove_file(p);
            let l = UnixListener::bind(p).map_err(|e| e.to_string())?;
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
            Ok(Listener(l))
        }
        pub fn accept(&mut self) -> Result<super::Conn, String> {
            let (s, _) = self.0.accept().map_err(|e| e.to_string())?;
            s.set_nonblocking(true).map_err(|e| e.to_string())?;
            Ok(super::Conn { inner: s })
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn test_path(name: &str) -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        if cfg!(windows) {
            format!(r"\\.\pipe\cad-test-{name}-{}-{n}", std::process::id())
        } else {
            std::env::temp_dir().join(format!("cad-test-{name}-{}-{n}", std::process::id())).join("s.sock").to_string_lossy().into_owned()
        }
    }

    /// Accept raw connections (used to fake Discord in tests).
    pub fn raw_listener(path: &str) -> impl FnMut() -> Conn {
        let mut listener = imp::Listener::bind(path).unwrap();
        move || listener.accept().unwrap()
    }

    #[test]
    fn request_reply_rejects_duplicates_and_reports_not_running() {
        let path = test_path("ipc");
        let mut server = Server::start(&path, Arc::new(|line| json!({"ok": true, "echo": line}).to_string())).unwrap();
        assert!(Server::start(&path, Arc::new(|_| String::new())).err().unwrap().contains("already running"));
        let reply = request(&path, &json!({"type": "status"}), Duration::from_secs(2)).unwrap();
        assert_eq!(reply["echo"], r#"{"type":"status"}"#);
        let many: Vec<_> = (0..20).map(|_| { let p = path.clone(); std::thread::spawn(move || request(&p, &json!({}), Duration::from_secs(3)).is_ok()) }).collect();
        assert!(many.into_iter().all(|t| t.join().unwrap()), "concurrent clients are served");
        server.stop();
        assert!(matches!(request(&path, &json!({}), Duration::from_millis(300)), Err(RequestError::NotRunning)));
    }

    #[test]
    fn errors_and_oversized_requests() {
        let path = test_path("ipc-err");
        let _server = Server::start(&path, Arc::new(|_| json!({"ok": false, "error": "nope"}).to_string())).unwrap();
        assert_eq!(request(&path, &json!({}), Duration::from_secs(2)).unwrap_err().to_string(), "nope");
        let big = json!({"x": "a".repeat(MAX_REQUEST + 10)});
        assert!(request(&path, &big, Duration::from_secs(2)).is_err());
    }

    #[test]
    fn pipe_name_matches_the_vscode_client_scheme() {
        std::env::remove_var("CAD_SOCKET");
        let path = service_path();
        let expected = &crate::sha256::hex(imp::username().as_bytes())[..12];
        assert!(path.ends_with(expected));
    }
}
