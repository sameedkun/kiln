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
    ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, LocalFree, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNIX, INVALID_SOCKET, SOCK_STREAM, SOCKADDR, SOCKADDR_UN, SOCKET,
    WSADATA, WSAGetLastError, WSAStartup, closesocket, connect, socket,
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
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, RRF_NOEXPAND,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegDeleteKeyValueW, RegDeleteTreeW,
    RegFlushKey, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_HIDE, SendMessageTimeoutW, WM_SETTINGCHANGE,
};

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
// Processes
// ---------------------------------------------------------------------------------------------

/// An open handle to another process, for checking what it is and stopping it.
///
/// Holding the handle pins the process object, so a PID checked through [`Process::image_path`]
/// can't be reused by an unrelated process before [`Process::terminate`] acts on it.
#[derive(Debug)]
pub struct Process {
    handle: OwnedHandle,
}

impl Process {
    /// Opens process `pid` with query, terminate and wait rights. `Ok(None)` if no such process
    /// exists (any more).
    pub fn open(pid: u32) -> io::Result<Option<Self>> {
        // SAFETY: plain call; a null return is handled below.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            // OpenProcess reports a PID that doesn't exist as ERROR_INVALID_PARAMETER.
            if error.raw_os_error() == i32::try_from(ERROR_INVALID_PARAMETER).ok() {
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some(Self {
            handle: owned(handle),
        }))
    }

    /// Opens process `pid` for inspection only (image path, running state); no terminate or wait
    /// rights, so it also works on processes that refuse those, such as packaged apps. `Ok(None)`
    /// if no such process exists.
    pub fn open_for_query(pid: u32) -> io::Result<Option<Self>> {
        // SAFETY: plain call; a null return is handled below.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == i32::try_from(ERROR_INVALID_PARAMETER).ok() {
                return Ok(None);
            }
            return Err(error);
        }
        Ok(Some(Self {
            handle: owned(handle),
        }))
    }

    /// Full path of the process's executable (`QueryFullProcessImageNameW`, Win32 form).
    pub fn image_path(&self) -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 32_768];
        let mut len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: valid process handle; `buffer` holds `len` u16s and `len` is a valid in/out
        // pointer.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                raw(&self.handle),
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &raw mut len,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(usize::try_from(len).unwrap_or(0));
        Ok(PathBuf::from(OsString::from_wide(&buffer)))
    }

    /// Whether the process is still running.
    pub fn is_running(&self) -> io::Result<bool> {
        Ok(self.wait(Duration::ZERO)?.is_none())
    }

    /// Ends the process immediately with `exit_code` (`TerminateProcess`).
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: valid process handle opened with PROCESS_TERMINATE.
        if unsafe { TerminateProcess(raw(&self.handle), exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Waits up to `timeout`; returns the exit code, or `None` if still running.
    pub fn wait(&self, timeout: Duration) -> io::Result<Option<u32>> {
        wait_for_exit(&self.handle, timeout)
    }
}

fn wait_for_exit(handle: &OwnedHandle, timeout: Duration) -> io::Result<Option<u32>> {
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: valid process handle owned by the caller.
    match unsafe { WaitForSingleObject(raw(handle), ms) } {
        WAIT_OBJECT_0 => {
            let mut code = 0u32;
            // SAFETY: valid process handle; `code` is a valid out pointer.
            if unsafe { GetExitCodeProcess(raw(handle), &raw mut code) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Some(code))
        }
        WAIT_TIMEOUT => Ok(None),
        _ => Err(io::Error::last_os_error()),
    }
}

// ---------------------------------------------------------------------------------------------
// TCP listeners and process parents (for `kiln doctor`, ADR 0017)
// ---------------------------------------------------------------------------------------------

/// Every listening TCP socket on the machine (IPv4 and IPv6) with its owning process ID.
pub fn tcp_listeners() -> io::Result<Vec<(std::net::SocketAddr, u32)>> {
    let mut found = Vec::new();
    for family in [AF_INET, AF_INET6] {
        let table = extended_tcp_table(u32::from(family))?;
        if table.len() < 4 {
            continue;
        }
        let count = u32::from_ne_bytes([table[0], table[1], table[2], table[3]]) as usize;
        // Rows start after the u32 count, padded to the row's alignment (4 for both kinds).
        let rows = &table[4..];
        if family == AF_INET {
            let size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
            for row in rows.chunks_exact(size).take(count) {
                // SAFETY: `row` is exactly one MIB_TCPROW_OWNER_PID as filled in by the API;
                // read_unaligned copes with the byte buffer's alignment.
                let row: MIB_TCPROW_OWNER_PID =
                    unsafe { std::ptr::read_unaligned(row.as_ptr().cast()) };
                let ip = std::net::Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes());
                let port = u16::from_be(u16::try_from(row.dwLocalPort & 0xFFFF).unwrap_or(0));
                found.push((std::net::SocketAddr::from((ip, port)), row.dwOwningPid));
            }
        } else {
            let size = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
            for row in rows.chunks_exact(size).take(count) {
                // SAFETY: as above, one MIB_TCP6ROW_OWNER_PID.
                let row: MIB_TCP6ROW_OWNER_PID =
                    unsafe { std::ptr::read_unaligned(row.as_ptr().cast()) };
                let ip = std::net::Ipv6Addr::from(row.ucLocalAddr);
                let port = u16::from_be(u16::try_from(row.dwLocalPort & 0xFFFF).unwrap_or(0));
                found.push((std::net::SocketAddr::from((ip, port)), row.dwOwningPid));
            }
        }
    }
    Ok(found)
}

/// The raw listener table for one address family.
fn extended_tcp_table(family: u32) -> io::Result<Vec<u8>> {
    let mut size = 0u32;
    // The table can grow between calls; retry a few times with the size the API asks for.
    for _ in 0..5 {
        let mut buffer = vec![0u8; size as usize];
        let pointer = if buffer.is_empty() {
            std::ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast::<c_void>()
        };
        // SAFETY: `pointer` is null (size query) or points to `size` writable bytes.
        let status = unsafe {
            GetExtendedTcpTable(
                pointer,
                &raw mut size,
                0,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        match status {
            0 => return Ok(buffer),
            ERROR_INSUFFICIENT_BUFFER => {}
            other => {
                return Err(io::Error::from_raw_os_error(
                    i32::try_from(other).unwrap_or(-1),
                ));
            }
        }
    }
    Err(io::Error::other("the TCP table kept changing size"))
}

/// Each running process's parent, by process ID (a snapshot; PIDs may be reused).
pub fn process_parents() -> io::Result<std::collections::HashMap<u32, u32>> {
    // SAFETY: plain call; INVALID_HANDLE_VALUE is handled below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = owned(snapshot);
    let mut parents = std::collections::HashMap::new();
    // SAFETY: all-zero is a valid PROCESSENTRY32W apart from dwSize, set next.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0);
    // SAFETY: valid snapshot handle and entry with dwSize set.
    let mut ok = unsafe { Process32FirstW(raw(&snapshot), &raw mut entry) };
    while ok != 0 {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        // SAFETY: as above.
        ok = unsafe { Process32NextW(raw(&snapshot), &raw mut entry) };
    }
    Ok(parents)
}

// ---------------------------------------------------------------------------------------------
// Current-user registry strings (user environment variables)
// ---------------------------------------------------------------------------------------------

/// The current user's environment variables live here (`HKCU\Environment`).
pub const USER_ENVIRONMENT_KEY: &str = "Environment";

/// A string value: its text, and whether it is `REG_EXPAND_SZ` (holds `%VARS%`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegString {
    pub value: String,
    pub expandable: bool,
}

/// Reads `HKCU\<subkey>\<name>` as stored (`%VARS%` are **not** expanded). `Ok(None)` if the
/// key or value doesn't exist.
pub fn read_user_reg_string(subkey: &str, name: &str) -> io::Result<Option<RegString>> {
    let subkey_w = wide(OsStr::new(subkey));
    let name_w = wide(OsStr::new(name));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut kind = 0u32;
    let mut bytes = 0u32;
    // SAFETY: valid NUL-terminated strings; a null buffer asks for the size in bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            name_w.as_ptr(),
            flags,
            &raw mut kind,
            std::ptr::null_mut(),
            &raw mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    let mut buffer = vec![0u16; usize::try_from(bytes).unwrap_or(0) / 2 + 1];
    let mut len = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: `buffer` holds `len` bytes; the other pointers are valid as above.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            name_w.as_ptr(),
            flags,
            &raw mut kind,
            buffer.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    let text: Vec<u16> = buffer.into_iter().take_while(|&c| c != 0).collect();
    Ok(Some(RegString {
        value: String::from_utf16_lossy(&text),
        expandable: kind == REG_EXPAND_SZ,
    }))
}

/// Writes `HKCU\<subkey>\<name>` (creating the key if needed) as `REG_EXPAND_SZ` or `REG_SZ`.
pub fn write_user_reg_string(subkey: &str, name: &str, value: &RegString) -> io::Result<()> {
    let subkey_w = wide(OsStr::new(subkey));
    let name_w = wide(OsStr::new(name));
    let data = wide(OsStr::new(&value.value));
    let kind = if value.expandable {
        REG_EXPAND_SZ
    } else {
        REG_SZ
    };
    // SAFETY: valid NUL-terminated strings; `data` is passed with its size in bytes, including
    // the terminating NUL.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            name_w.as_ptr(),
            kind,
            data.as_ptr().cast::<c_void>(),
            u32::try_from(data.len() * 2).unwrap_or(u32::MAX),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    flush_user_key(subkey)
}

/// Writes `HKCU\<subkey>` to disk now. Windows otherwise writes registry changes lazily, so
/// a value set just before a crash or power loss can be gone afterwards, while the journal
/// says it was applied (found with the Run value in P2-02).
fn flush_user_key(subkey: &str) -> io::Result<()> {
    let subkey_w = wide(OsStr::new(subkey));
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated string and a valid out pointer for the key handle.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            KEY_SET_VALUE,
            &raw mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if status != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    // SAFETY: `key` is the open key from above.
    let flushed = unsafe { RegFlushKey(key) };
    // SAFETY: `key` is open and closed exactly once.
    unsafe { RegCloseKey(key) };
    if flushed != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(flushed).unwrap_or(-1),
        ));
    }
    Ok(())
}

/// Programs started at the user's login (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
pub const USER_RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Deletes the value `HKCU\<subkey>\<name>`. A missing key or value is not an error.
pub fn delete_user_reg_value(subkey: &str, name: &str) -> io::Result<()> {
    let subkey_w = wide(OsStr::new(subkey));
    let name_w = wide(OsStr::new(name));
    // SAFETY: valid NUL-terminated strings.
    let status =
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey_w.as_ptr(), name_w.as_ptr()) };
    if status != 0 && status != ERROR_FILE_NOT_FOUND {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    flush_user_key(subkey)
}

/// Deletes `HKCU\<subkey>` and everything under it. Only for test keys Kiln created.
pub fn delete_user_reg_tree(subkey: &str) -> io::Result<()> {
    let subkey_w = wide(OsStr::new(subkey));
    // SAFETY: valid NUL-terminated string.
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, subkey_w.as_ptr()) };
    // With a subkey name, the key itself goes too, not only its contents.
    if status != 0 && status != ERROR_FILE_NOT_FOUND {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(status).unwrap_or(-1),
        ));
    }
    Ok(())
}

/// Tells running programs (Explorer, new terminals) that environment variables changed.
pub fn broadcast_environment_change() {
    let what = wide(OsStr::new(USER_ENVIRONMENT_KEY));
    let mut result = 0usize;
    // SAFETY: `what` is a valid NUL-terminated string that outlives the call; the timeout
    // keeps a hung window from blocking us.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            what.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &raw mut result,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Console control events
// ---------------------------------------------------------------------------------------------

unsafe extern "system" fn swallow_ctrl_event(_ctrl_type: u32) -> windows_sys::core::BOOL {
    1 // handled: this process stays alive; the child sharing the console decides for itself
}

/// Keeps this process alive on Ctrl+C / Ctrl+Break, for a wrapper that waits for a child on
/// the same console ([ADR 0016] §9).
///
/// Unlike `SetConsoleCtrlHandler(NULL, TRUE)`, a registered handler isn't inherited, so the
/// child still gets (and handles) Ctrl+C normally.
///
/// [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
pub fn survive_ctrl_c() -> io::Result<()> {
    // SAFETY: `swallow_ctrl_event` matches PHANDLER_ROUTINE and lives for the whole process.
    if unsafe { SetConsoleCtrlHandler(Some(swallow_ctrl_event), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Job objects
// ---------------------------------------------------------------------------------------------

/// A job object that ends every process in it when the last handle to it closes, i.e. when
/// its owner exits for any reason ([ADR 0014] §3).
///
/// [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
#[derive(Debug)]
pub struct KillOnCloseJob {
    handle: OwnedHandle,
}

impl KillOnCloseJob {
    pub fn new() -> io::Result<Self> {
        // SAFETY: null attributes and name create an unnamed job with default security.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Self {
            handle: owned(handle),
        };
        // SAFETY: a plain C struct for which all-zero means "no limits".
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: valid job handle; `info` is the struct this information class expects, and
        // its size is passed with it.
        let ok = unsafe {
            SetInformationJobObject(
                raw(&job.handle),
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .unwrap_or(u32::MAX),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Puts `child` in the job, so it ends when the job closes.
    pub fn assign(&self, child: &std::process::Child) -> io::Result<()> {
        // SAFETY: both handles are valid: the job's is owned by `self`, and the child's is
        // owned by `child` for the duration of this call.
        let ok = unsafe { AssignProcessToJobObject(raw(&self.handle), child.as_raw_handle()) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Unix domain sockets (AF_UNIX, Windows 10 1803+)
// ---------------------------------------------------------------------------------------------

/// Longest socket path `sockaddr_un` can hold, excluding the terminating NUL.
pub const MAX_UNIX_SOCKET_PATH: usize = 107;

fn winsock_started() -> io::Result<()> {
    static STARTED: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    let result = *STARTED.get_or_init(|| {
        // SAFETY: WSADATA is a plain C struct; all-zero is a valid out buffer.
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        // SAFETY: `data` is a valid out pointer. Winsock start-up is reference-counted, so a
        // start that is never cleaned up only keeps the DLL initialised for the process.
        unsafe { WSAStartup(0x0202, &raw mut data) }
    });
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result))
    }
}

fn unix_address(path: &Path) -> io::Result<SOCKADDR_UN> {
    // UTF-8 bytes, as Go (and so Caddy) passes them, so both sides name the same file.
    let bytes = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket path is not Unicode"))?
        .as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_UNIX_SOCKET_PATH || bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "socket path must be 1 to {MAX_UNIX_SOCKET_PATH} bytes without NULs: {}",
                path.display()
            ),
        ));
    }
    let mut address = SOCKADDR_UN {
        sun_family: AF_UNIX,
        sun_path: [0; 108],
    };
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = i8::from_ne_bytes([*byte]);
    }
    Ok(address)
}

/// A Winsock socket that is closed on drop unless handed off.
struct OwnedSocket(SOCKET);

impl OwnedSocket {
    fn new() -> io::Result<Self> {
        winsock_started()?;
        // SAFETY: plain call; INVALID_SOCKET is handled below.
        let socket = unsafe { socket(i32::from(AF_UNIX), SOCK_STREAM, 0) };
        if socket == INVALID_SOCKET {
            return Err(last_wsa_error());
        }
        Ok(Self(socket))
    }

    fn into_stream(self) -> std::net::TcpStream {
        use std::os::windows::io::FromRawSocket;
        let socket = self.0;
        std::mem::forget(self);
        // SAFETY: `socket` is a valid, connected socket that nothing else owns; the stream
        // becomes its only owner and closes it on drop.
        unsafe { std::net::TcpStream::from_raw_socket(socket as u64) }
    }
}

impl Drop for OwnedSocket {
    fn drop(&mut self) {
        // SAFETY: we own this socket and close it exactly once.
        unsafe { closesocket(self.0) };
    }
}

fn last_wsa_error() -> io::Error {
    // SAFETY: plain call reading the calling thread's last Winsock error.
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() })
}

fn addr_len() -> i32 {
    i32::try_from(std::mem::size_of::<SOCKADDR_UN>()).unwrap_or(i32::MAX)
}

/// Connects to the Unix domain socket at `path`.
///
/// The connection comes back as a [`std::net::TcpStream`] because that is std's wrapper for a
/// connected Winsock stream socket: reads, writes, timeouts and shutdown behave the same for
/// `AF_UNIX`. Methods that are about TCP or IP addresses (`peer_addr`, `set_nodelay`, …)
/// don't apply and may fail.
pub fn connect_unix(path: &Path) -> io::Result<std::net::TcpStream> {
    let address = unix_address(path)?;
    let socket = OwnedSocket::new()?;
    // SAFETY: `address` is a fully initialised SOCKADDR_UN and its size is passed with it.
    let result = unsafe {
        connect(
            socket.0,
            (&raw const address).cast::<SOCKADDR>(),
            addr_len(),
        )
    };
    if result != 0 {
        return Err(last_wsa_error());
    }
    Ok(socket.into_stream())
}

/// A listening Unix domain socket (for `kilnd`'s API, [ADR 0026] §3).
///
/// Neither it nor the connections it accepts are inherited by child processes, so a program
/// the daemon starts can't keep its socket open (as with I-60).
///
/// [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md
#[derive(Debug)]
pub struct UnixListener(SOCKET);

impl UnixListener {
    /// Binds `path` and listens. The file must not exist yet: remove a stale one first.
    pub fn bind(path: &Path) -> io::Result<Self> {
        use windows_sys::Win32::Networking::WinSock::{
            WSA_FLAG_NO_HANDLE_INHERIT, WSA_FLAG_OVERLAPPED, WSASocketW, bind, listen,
        };
        let address = unix_address(path)?;
        winsock_started()?;
        // SAFETY: plain call with no protocol info; INVALID_SOCKET is handled below.
        let raw = unsafe {
            WSASocketW(
                i32::from(AF_UNIX),
                SOCK_STREAM,
                0,
                std::ptr::null(),
                0,
                WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT,
            )
        };
        if raw == INVALID_SOCKET {
            return Err(last_wsa_error());
        }
        let socket = OwnedSocket(raw);
        // SAFETY: `socket` is valid and `address` is a fully initialised SOCKADDR_UN of the
        // size passed with it.
        let bound = unsafe {
            bind(
                socket.0,
                (&raw const address).cast::<SOCKADDR>(),
                addr_len(),
            )
        };
        if bound != 0 {
            return Err(last_wsa_error());
        }
        // SAFETY: `socket` is a valid, bound socket.
        if unsafe { listen(socket.0, 16) } != 0 {
            return Err(last_wsa_error());
        }
        let raw = socket.0;
        std::mem::forget(socket);
        Ok(Self(raw))
    }

    /// Waits for the next connection.
    pub fn accept(&self) -> io::Result<std::net::TcpStream> {
        use windows_sys::Win32::Networking::WinSock::accept;
        // SAFETY: `self.0` is a valid listening socket; null address pointers are allowed.
        let client = unsafe { accept(self.0, std::ptr::null_mut(), std::ptr::null_mut()) };
        if client == INVALID_SOCKET {
            return Err(last_wsa_error());
        }
        let client = OwnedSocket(client);
        // SAFETY: a socket is a kernel handle; only its inherit flag changes.
        let set = unsafe { SetHandleInformation(client.0 as HANDLE, HANDLE_FLAG_INHERIT, 0) };
        if set == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(client.into_stream())
    }
}

impl Drop for UnixListener {
    fn drop(&mut self) {
        // SAFETY: we own this socket and close it exactly once.
        unsafe { closesocket(self.0) };
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
        wait_for_exit(&self.handle, timeout)
    }
}

/// Keeps this process's standard handles out of the children it starts until the returned
/// guard drops. Windows hands a child every inheritable handle of its parent, not only the
/// three it is given, and a shell makes `kiln`'s stdout and stderr inheritable: a server
/// started with them would hold `kiln`'s output pipe open after `kiln` exits, so whatever reads
/// that output (`kiln web start | ...`, a script capturing it) would wait forever (I-60).
/// Children given `Stdio::inherit` are unaffected: std duplicates the handle for them.
pub fn keep_std_handles() -> StdHandlesKept {
    let mut cleared = Vec::new();
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle takes no pointers and returns a handle this process owns (or
        // null / INVALID_HANDLE_VALUE, skipped below); nothing is closed.
        let handle = unsafe { GetStdHandle(id) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE || cleared.contains(&handle) {
            continue;
        }
        if is_inheritable(handle) == Some(true)
            // SAFETY: a live handle of this process; only its inherit flag changes.
            && unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } != 0
        {
            cleared.push(handle);
        }
    }
    StdHandlesKept(cleared)
}

fn is_inheritable(handle: HANDLE) -> Option<bool> {
    let mut flags = 0;
    // SAFETY: `handle` is a live handle of this process and `flags` a u32 to write to.
    let ok = unsafe { GetHandleInformation(handle, &raw mut flags) } != 0;
    ok.then_some(flags & HANDLE_FLAG_INHERIT != 0)
}

/// Restores the standard handles [`keep_std_handles`] took out of inheritance.
#[derive(Debug)]
#[must_use = "the handles become inheritable again when this drops"]
pub struct StdHandlesKept(Vec<HANDLE>);

impl Drop for StdHandlesKept {
    fn drop(&mut self) {
        for &handle in &self.0 {
            // SAFETY: a standard handle of this process, as in `keep_std_handles`.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
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
    fn process_handle_names_its_image_and_terminates_it() {
        let ping = system_directory().unwrap().join("PING.EXE");
        let mut child = std::process::Command::new(&ping)
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let process = Process::open(child.id()).unwrap().expect("child exists");
        assert!(process.is_running().unwrap());
        let image = process.image_path().unwrap();
        assert!(
            image
                .to_string_lossy()
                .eq_ignore_ascii_case(&ping.to_string_lossy()),
            "{image:?} vs {ping:?}"
        );
        process.terminate(9).unwrap();
        assert_eq!(process.wait(Duration::from_secs(5)).unwrap(), Some(9));
        assert!(!process.is_running().unwrap());
        let _ = child.wait();
    }

    #[test]
    fn unix_sockets_are_not_inherited() {
        use std::os::windows::io::AsRawSocket;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let _client = connect_unix(&path).unwrap();
        let accepted = listener.accept().unwrap();
        let accepted = usize::try_from(accepted.as_raw_socket()).unwrap();
        for socket in [listener.0, accepted] {
            let mut flags = 0;
            // SAFETY: a live socket handle of this process and a u32 to write to.
            let ok = unsafe { GetHandleInformation(socket as HANDLE, &raw mut flags) };
            assert_ne!(ok, 0);
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        }
    }

    #[test]
    fn unix_socket_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sock");
        assert!(
            path.to_str().unwrap().len() <= MAX_UNIX_SOCKET_PATH,
            "temp path too long for this test: {}",
            path.display()
        );
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            let mut buf = [0u8; 5];
            stream.read_exact(&mut buf).unwrap();
            stream
                .write_all(&buf.map(|b| b.to_ascii_uppercase()))
                .unwrap();
        });
        let mut client = connect_unix(&path).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client.write_all(b"hello").unwrap();
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).unwrap();
        assert_eq!(reply, b"HELLO");
        server.join().unwrap();
    }

    #[test]
    fn unix_socket_errors_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        assert!(connect_unix(&dir.path().join("nobody-listens.sock")).is_err());
        let long = PathBuf::from(format!("C:\\{}", "x".repeat(MAX_UNIX_SOCKET_PATH)));
        let err = connect_unix(&long).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    }

    #[test]
    fn listeners_include_our_own_socket_with_our_pid() {
        let v4 = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let v6 = std::net::TcpListener::bind("[::1]:0").ok();
        let listeners = tcp_listeners().unwrap();
        let me = std::process::id();
        assert!(
            listeners.contains(&(v4.local_addr().unwrap(), me)),
            "{:?} not in {} listeners",
            v4.local_addr(),
            listeners.len()
        );
        if let Some(v6) = v6 {
            assert!(listeners.contains(&(v6.local_addr().unwrap(), me)));
        }
    }

    #[test]
    fn our_parent_is_in_the_process_snapshot() {
        let child = std::process::Command::new(system_directory().unwrap().join("PING.EXE"))
            .args(["-n", "5", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn();
        let mut child = child.unwrap();
        let parents = process_parents().unwrap();
        assert_eq!(parents.get(&child.id()), Some(&std::process::id()));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn user_registry_strings_round_trip_in_a_private_key() {
        // Never the real HKCU\Environment: a throwaway key, deleted afterwards.
        let key = format!(r"Software\{}", unique("KilnTest"));
        assert_eq!(read_user_reg_string(&key, "Path").unwrap(), None);
        let value = RegString {
            value: r"%USERPROFILE%\bin;C:\Tools".into(),
            expandable: true,
        };
        write_user_reg_string(&key, "Path", &value).unwrap();
        assert_eq!(
            read_user_reg_string(&key, "Path").unwrap(),
            Some(value),
            "not expanded"
        );
        let plain = RegString {
            value: "C:\\Ünïcode\\bin".into(),
            expandable: false,
        };
        write_user_reg_string(&key, "Other", &plain).unwrap();
        assert_eq!(read_user_reg_string(&key, "Other").unwrap(), Some(plain));
        delete_user_reg_value(&key, "Other").unwrap();
        assert_eq!(read_user_reg_string(&key, "Other").unwrap(), None);
        assert!(
            read_user_reg_string(&key, "Path").unwrap().is_some(),
            "only that value"
        );
        delete_user_reg_value(&key, "Other").unwrap(); // already gone: fine
        delete_user_reg_tree(&key).unwrap();
        assert_eq!(
            read_user_reg_string(&key, "Path").unwrap(),
            None,
            "key removed"
        );
        delete_user_reg_tree(&key).unwrap(); // already gone: fine
    }

    #[test]
    fn closing_the_job_ends_its_processes() {
        let ping = system_directory().unwrap().join("PING.EXE");
        let mut child = std::process::Command::new(&ping)
            .args(["-n", "60", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let job = KillOnCloseJob::new().unwrap();
        job.assign(&child).unwrap();
        assert!(
            child.try_wait().unwrap().is_none(),
            "running while the job is open"
        );
        drop(job);
        let process = Process::open(child.id()).unwrap();
        let exited = process.is_none_or(|p| p.wait(Duration::from_secs(5)).unwrap().is_some());
        assert!(exited, "closing the job ended the process");
        let _ = child.wait();
    }

    #[test]
    fn opening_a_process_that_does_not_exist_is_none() {
        // PIDs are multiples of 4; an odd PID never names a process.
        assert!(Process::open(0xFFFF_FFF1).unwrap().is_none());
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
    fn std_handles_are_kept_from_children_while_the_guard_lives() {
        // SAFETY: no pointers; the handle stays this process's.
        let stderr = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        if stderr.is_null() || stderr == INVALID_HANDLE_VALUE {
            return; // no stderr to check
        }
        // SAFETY: a live handle of this process; makes it inheritable, as a shell would.
        unsafe { SetHandleInformation(stderr, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
        let guard = keep_std_handles();
        assert_eq!(is_inheritable(stderr), Some(false));
        drop(guard);
        assert_eq!(is_inheritable(stderr), Some(true));
    }

    #[test]
    #[ignore = "shows a UAC prompt; run manually"]
    fn elevation_runs_and_reports_the_exit_code() {
        let exe = system_directory().unwrap().join("cmd.exe");
        let process = spawn_elevated(&exe, &["/c", "exit", "7"]).unwrap();
        assert_eq!(process.wait(Duration::from_secs(60)).unwrap(), Some(7));
    }
}
