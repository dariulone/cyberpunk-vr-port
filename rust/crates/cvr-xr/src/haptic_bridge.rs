//! The PlayStation VR2 Toolkit bridge's presence (main's src/Runtimes/HapticRouting.cpp DetectToolkitBridge): a
//! same-Windows-session psvr2_toolkit_dsx_bridge_017.exe owning a loopback UDP listener. A driver install, a mapping the
//! game made or another DSX client is not evidence. Checked once, at the session's start, never on the frame path.
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

#[link(name = "iphlpapi", kind = "raw-dylib")]
extern "system" {
    fn GetExtendedUdpTable(table: *mut core::ffi::c_void, size: *mut u32, order: i32, af: u32, class: i32, reserved: u32) -> u32;
}
#[link(name = "kernel32", kind = "raw-dylib")]
extern "system" {
    fn ProcessIdToSessionId(pid: u32, session: *mut u32) -> i32;
}

const AF_INET: u32 = 2;
const UDP_TABLE_OWNER_PID: i32 = 1;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const NO_ERROR: u32 = 0;
/// htonl(INADDR_LOOPBACK): 127.0.0.1 in network order, as the table stores it.
const LOOPBACK: u32 = u32::from_ne_bytes([127, 0, 0, 1]);
const BRIDGE: &str = "psvr2_toolkit_dsx_bridge_017.exe";

/// MIB_UDPROW_OWNER_PID.
#[repr(C)]
#[derive(Clone, Copy)]
struct Row {
    local_addr: u32,
    local_port: u32,
    owning_pid: u32,
}

fn is_bridge(pid: u32) -> bool {
    // SAFETY: a query handle to another process, closed here; a buffer of the size passed.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut path = vec![0u16; 32768];
        let mut count = path.len() as u32;
        let mut matches = false;
        if WaitForSingleObject(process, 0) == WAIT_TIMEOUT
            && QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut count) != 0
        {
            let full = String::from_utf16_lossy(&path[..count as usize]);
            let name = full.rsplit('\\').next().unwrap_or(&full);
            matches = name.eq_ignore_ascii_case(BRIDGE);
        }
        CloseHandle(process);
        matches
    }
}

/// True when the bridge's listener runs in this Windows session.
pub fn detect_toolkit_bridge() -> bool {
    let mut bytes = 0u32;
    // SAFETY: the size query (no table).
    if unsafe { GetExtendedUdpTable(core::ptr::null_mut(), &mut bytes, 0, AF_INET, UDP_TABLE_OWNER_PID, 0) } != ERROR_INSUFFICIENT_BUFFER {
        return false;
    }
    // Races while processes bind and unbind are bounded and retried.
    let mut attempt = 0;
    while attempt < 3 && bytes > 0 && bytes < 4 * 1024 * 1024 {
        attempt += 1;
        let mut storage = vec![0u64; (bytes as usize).div_ceil(8)];
        // SAFETY: a buffer of at least `bytes`.
        let result = unsafe {
            GetExtendedUdpTable(storage.as_mut_ptr().cast(), &mut bytes, 0, AF_INET, UDP_TABLE_OWNER_PID, 0)
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if result != NO_ERROR {
            return false;
        }
        let mut own_session = 0u32;
        // SAFETY: our own process.
        if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut own_session) } == 0 {
            return false;
        }
        // SAFETY: MIB_UDPTABLE_OWNER_PID: the count, then its rows, inside the buffer the call filled.
        let rows = unsafe {
            let base = storage.as_ptr().cast::<u8>();
            let entries = (*(base as *const u32)) as usize;
            let max = (bytes as usize).saturating_sub(4) / core::mem::size_of::<Row>();
            core::slice::from_raw_parts(base.add(4) as *const Row, entries.min(max))
        };
        for row in rows {
            if row.local_addr != LOOPBACK || row.local_port == 0 {
                continue;
            }
            let mut session = 0u32;
            // SAFETY: a plain query.
            if unsafe { ProcessIdToSessionId(row.owning_pid, &mut session) } == 0 || session != own_session {
                continue;
            }
            if is_bridge(row.owning_pid) {
                return true;
            }
        }
        return false;
    }
    false
}
