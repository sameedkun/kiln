//! Windows-only safe wrappers used by the privileged helper ([ADR 0009]).
//!
//! [ADR 0009]: ../../../docs/adr/0009-windows-privileged-helper.md

use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn owned(handle: HANDLE) -> OwnedHandle {
    // SAFETY: callers pass a valid handle that they own and have not closed or shared; the
    // returned `OwnedHandle` becomes its only owner and closes it on drop.
    unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

// ---------------------------------------------------------------------------------------------
// System directory
// ---------------------------------------------------------------------------------------------

/// The Windows system directory (usually `C:\Windows\System32`), from `GetSystemDirectoryW`.
///
/// Deliberately **not** derived from `SystemRoot` or any other environment variable: an
/// elevated process inherits its caller's environment, so environment-derived paths would let
/// an unprivileged caller redirect privileged writes.
pub fn system_directory() -> io::Result<PathBuf> {
    let mut buf = vec![0u16; 260];
    loop {
        let capacity = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        // SAFETY: `buf` is a writable buffer of exactly `capacity` UTF-16 units.
        let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), capacity) };
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        let len = len as usize;
        if len < buf.len() {
            buf.truncate(len);
            return Ok(PathBuf::from(OsString::from_wide(&buf)));
        }
        buf.resize(len + 1, 0); // `len` is the required size including the terminator.
    }
}

// ---------------------------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------------------------

fn token_user_sid(token: &OwnedHandle) -> io::Result<String> {
    let mut needed = 0u32;
    // SAFETY: querying the required size with a null buffer of length 0 is the documented
    // pattern; `needed` is a valid out pointer.
    unsafe {
        GetTokenInformation(
            raw(token),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 storage keeps the buffer aligned for TOKEN_USER (which holds pointers).
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    let buf_len = u32::try_from(buf.len() * 8).unwrap_or(u32::MAX);
    // SAFETY: `buf` is a writable, suitably aligned buffer of `buf_len` bytes.
    let ok = unsafe {
        GetTokenInformation(
            raw(token),
            TokenUser,
            buf.as_mut_ptr().cast::<c_void>(),
            buf_len,
            &raw mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let user = buf.as_ptr().cast::<TOKEN_USER>();
    // SAFETY: on success the buffer starts with an initialised TOKEN_USER, and `buf` outlives
    // this read.
    let sid = unsafe { (*user).User.Sid };
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` points into `buf` (still alive); `text` is a valid out pointer.
    if unsafe { ConvertSidToStringSidW(sid, &raw mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0usize;
    loop {
        // SAFETY: `text` is a NUL-terminated UTF-16 string allocated by the call above, and
        // `len` never passes its terminator, so `text + len` stays inside the allocation.
        let unit = unsafe { text.add(len) };
        // SAFETY: `unit` points to an initialised unit of that string (see above).
        if unsafe { *unit } == 0 {
            break;
        }
        len += 1;
    }
    // SAFETY: `text` is valid for `len` units, as established by the loop above.
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: `text` was allocated with LocalAlloc by ConvertSidToStringSidW and is freed once.
    unsafe { LocalFree(text.cast::<c_void>()) };
    Ok(s)
}

fn process_token(process: HANDLE) -> io::Result<OwnedHandle> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `process` is a valid process handle; `token` is a valid out pointer.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(owned(token))
}

/// String SID (e.g. `S-1-5-21-…`) of the user this process runs as.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    let me = unsafe { GetCurrentProcess() };
    token_user_sid(&process_token(me)?)
}

/// String SID of the user that process `pid` runs as.
pub fn process_user_sid(pid: u32) -> io::Result<String> {
    // SAFETY: plain call; a null result is handled below.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = owned(process);
    token_user_sid(&process_token(raw(&process))?)
}

// ---------------------------------------------------------------------------------------------
// Named pipes
// ---------------------------------------------------------------------------------------------

fn pipe_path(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

/// The server end of a single-instance, local-only, byte-mode named pipe.
#[derive(Debug)]
pub struct ServerPipe {
    handle: OwnedHandle,
}

impl ServerPipe {
    /// Creates `\\.\pipe\<name>` with the security descriptor `sddl`.
    ///
    /// Fails if a pipe of that name already exists (`FILE_FLAG_FIRST_PIPE_INSTANCE`), so a
    /// pre-created ("squatted") name is detected instead of silently shared. Remote clients
    /// are rejected (`PIPE_REJECT_REMOTE_CLIENTS`).
    pub fn create(name: &str, sddl: &str) -> io::Result<Self> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid pipe name",
            ));
        }
        let sddl_w = wide(OsStr::new(sddl));
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `sddl_w` is NUL-terminated; `descriptor` is a valid out pointer; the size
        // out-parameter is optional.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_w.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let path = wide(OsStr::new(&pipe_path(name)));
        // SAFETY: `path` is NUL-terminated; `attributes` and the descriptor it points to are
        // valid for the duration of the call.
        let handle = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                &raw const attributes,
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: `descriptor` was allocated by ConvertStringSecurityDescriptor… and is freed
        // exactly once, after its last use above.
        unsafe { LocalFree(descriptor) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(error);
        }
        Ok(Self {
            handle: owned(handle),
        })
    }

    /// Blocks until a client connects.
    pub fn wait_for_client(&self) -> io::Result<()> {
        // SAFETY: the handle is a valid pipe handle opened without FILE_FLAG_OVERLAPPED, so a
        // null OVERLAPPED pointer selects synchronous operation.
        if unsafe { ConnectNamedPipe(raw(&self.handle), std::ptr::null_mut()) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // A client that connected between creation and this call is still a connection.
        if error.raw_os_error() == Some(i32::try_from(ERROR_PIPE_CONNECTED).unwrap_or(-1)) {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// Process id of the connected client.
    pub fn client_process_id(&self) -> io::Result<u32> {
        let mut pid = 0u32;
        // SAFETY: valid pipe handle; `pid` is a valid out pointer.
        if unsafe { GetNamedPipeClientProcessId(raw(&self.handle), &raw mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }

    /// The connection as a `File` for reading and writing.
    #[must_use]
    pub fn into_file(self) -> File {
        File::from(self.handle)
    }
}

/// Opens the client end of `\\.\pipe\<name>`, retrying while the server is not there yet or
/// busy, until `timeout`. The client only allows the server to *identify* it, never to
/// impersonate it (`SECURITY_IDENTIFICATION`).
pub fn connect_pipe(name: &str, timeout: Duration) -> io::Result<File> {
    let deadline = Instant::now() + timeout;
    let path = pipe_path(name);
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(&path)
        {
            Ok(file) => return Ok(file),
            Err(e)
                if Instant::now() < deadline
                    && matches!(
                        e.raw_os_error().and_then(|c| u32::try_from(c).ok()),
                        Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)
                    ) =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Elevation
// ---------------------------------------------------------------------------------------------

/// A process started through UAC.
#[derive(Debug)]
pub struct ElevatedProcess {
    handle: OwnedHandle,
}

/// Starts `exe` elevated (UAC consent prompt, hidden window) with `args`.
///
/// Arguments must not contain whitespace or quotes (they are joined with spaces); this keeps
/// the command line unambiguous without a quoting layer. A declined prompt is reported as
/// `io::ErrorKind::PermissionDenied`.
pub fn spawn_elevated(exe: &Path, args: &[&str]) -> io::Result<ElevatedProcess> {
    if args
        .iter()
        .any(|a| a.is_empty() || a.chars().any(|c| c.is_whitespace() || c == '"'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "elevated arguments must be non-empty and contain no whitespace or quotes",
        ));
    }
    let verb = wide(OsStr::new("runas"));
    let file = wide(exe.as_os_str());
    let params = wide(OsStr::new(&args.join(" ")));
    // SAFETY: SHELLEXECUTEINFOW is a plain C struct for which all-zero is a valid "unset"
    // value; required fields are filled in below.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0);
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: `info` is fully initialised and the strings it points to outlive the call.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(i32::try_from(ERROR_CANCELLED).unwrap_or(-1)) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the administrator prompt was declined",
            ));
        }
        return Err(error);
    }
    if info.hProcess.is_null() {
        return Err(io::Error::other("no process handle returned"));
    }
    Ok(ElevatedProcess {
        handle: owned(info.hProcess),
    })
}

impl ElevatedProcess {
    /// Waits up to `timeout`; returns the exit code, or `None` if still running.
    pub fn wait(&self, timeout: Duration) -> io::Result<Option<u32>> {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: valid process handle owned by `self`.
        match unsafe { WaitForSingleObject(raw(&self.handle), ms) } {
            WAIT_OBJECT_0 => {
                let mut code = 0u32;
                // SAFETY: valid process handle; `code` is a valid out pointer.
                if unsafe { GetExitCodeProcess(raw(&self.handle), &raw mut code) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(code))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Pipe names unique per process and test, so parallel tests never collide.
    fn unique(prefix: &str) -> String {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        format!(
            "{prefix}-{}-{}-{nanos}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn system_directory_is_system32_and_holds_the_hosts_file() {
        let dir = system_directory().unwrap();
        assert!(
            dir.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("system32")),
            "{dir:?}"
        );
        assert!(dir.join(r"drivers\etc").is_dir());
    }

    #[test]
    fn user_sid_of_this_process_matches_its_token() {
        let me = current_user_sid().unwrap();
        assert!(me.starts_with("S-1-5-"), "{me}");
        assert_eq!(process_user_sid(std::process::id()).unwrap(), me);
    }

    fn private_sddl() -> String {
        format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{})", current_user_sid().unwrap())
    }

    #[test]
    fn pipe_round_trip_and_client_pid() {
        let name = unique("kiln-sys-test");
        let server = ServerPipe::create(&name, &private_sddl()).unwrap();
        let client_name = name.clone();
        let client = std::thread::spawn(move || {
            let mut pipe = connect_pipe(&client_name, Duration::from_secs(5)).unwrap();
            pipe.write_all(b"ping").unwrap();
            let mut buf = [0u8; 4];
            pipe.read_exact(&mut buf).unwrap();
            buf
        });
        server.wait_for_client().unwrap();
        assert_eq!(server.client_process_id().unwrap(), std::process::id());
        let mut conn = server.into_file();
        let mut buf = [0u8; 4];
        conn.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        conn.write_all(b"pong").unwrap();
        assert_eq!(&client.join().unwrap(), b"pong");
    }

    #[test]
    fn a_second_server_with_the_same_name_is_refused() {
        let name = unique("kiln-sys-squat");
        let _first = ServerPipe::create(&name, &private_sddl()).unwrap();
        let second = ServerPipe::create(&name, &private_sddl()).unwrap_err();
        assert_eq!(second.kind(), io::ErrorKind::PermissionDenied, "{second:?}");
    }

    #[test]
    fn clients_not_in_the_dacl_are_refused() {
        let name = unique("kiln-sys-acl");
        let _server = ServerPipe::create(&name, "D:P(A;;GA;;;SY)").unwrap();
        let err = connect_pipe(&name, Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err:?}");
    }

    #[test]
    fn invalid_pipe_names_and_elevation_arguments_are_rejected() {
        assert!(ServerPipe::create(r"..\evil", &private_sddl()).is_err());
        let exe = system_directory().unwrap().join("cmd.exe");
        assert!(spawn_elevated(&exe, &["/c echo", "x"]).is_err());
        assert!(spawn_elevated(&exe, &["a\"b"]).is_err());
    }

    /// Manual: shows a UAC prompt. Run with
    /// `cargo test -p kiln-sys -- --ignored elevation_runs_and_reports_the_exit_code`.
    #[test]
    #[ignore = "shows a UAC prompt; run manually"]
    fn elevation_runs_and_reports_the_exit_code() {
        let exe = system_directory().unwrap().join("cmd.exe");
        let process = spawn_elevated(&exe, &["/c", "exit", "7"]).unwrap();
        assert_eq!(process.wait(Duration::from_secs(60)).unwrap(), Some(7));
    }
}
