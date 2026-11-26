//! The launcher's DEBUG gate (formerly src/Utils/DebugGate.cpp): every probe, census, dump and diagnostic log in one
//! table (the C++ half's, as it holds the flags' addresses), set once at plugin init -- to each flag's "on" value when
//! DEBUG is ticked, else to zero. A one-shot rather than a test at every read: the flags sit in the hottest paths, and
//! with DEBUG off a single probe can still be switched on live from a debugger.
use core::ffi::c_char;

/// DebugFlag (the C++ table's row).
#[repr(C)]
pub struct Flag {
    pub name: *const c_char,
    pub addr: *mut i32,
    pub on_value: i32,
    /// int32_t/uint32_t rather than int (the same 32 bits on Windows; kept as the table says).
    pub wide: bool,
}

/// ApplyLauncherDebugGate: every flag to its "on" value (or zero), the verbose channel with them, one log line.
///
/// # Safety
/// `flags` is `count` rows whose addresses are live 32-bit flags; `verbose` is the verbose channel or null.
pub unsafe fn apply(flags: &[Flag], on: bool, verbose: *mut i32) {
    for f in flags {
        if !f.addr.is_null() {
            // SAFETY: the caller's flag.
            unsafe { f.addr.write_volatile(if on { f.on_value } else { 0 }) };
        }
    }
    if !verbose.is_null() {
        // SAFETY: the caller's channel.
        unsafe { verbose.write_volatile(on as i32) };
    }
    crate::log::set_debug(on);
    crate::log!(
        "Debug gate: launcher DEBUG={} -> {} probes {}. {}\n",
        on as i32,
        flags.len(),
        if on { "ARMED" } else { "silenced" },
        if on {
            "Expect a large log and a heavy frame time; this is for diagnosis, not play."
        } else {
            "Flip one live in x64dbg if you need a single probe without restarting."
        }
    );
}

/// # Safety
/// As [`apply`]; `flags` is `count` rows or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_apply_launcher_debug_gate(flags: *const Flag, count: usize, on: bool, verbose: *mut i32) {
    let flags = if flags.is_null() {
        &[]
    } else {
        // SAFETY: the caller's table.
        unsafe { core::slice::from_raw_parts(flags, count) }
    };
    // SAFETY: the caller's flags and channel.
    unsafe { apply(flags, on, verbose) }
}
