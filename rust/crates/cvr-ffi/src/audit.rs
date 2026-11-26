//! The silent failures said once: a hooked call's argument or result read as a type it is not, a dynamic call made
//! with arguments its function's parameters do not take, a field that is not there. Each kind of mistake at each site
//! is logged once ("[audit] ..."), so a game session's log lists every place the port reads the game's data wrong,
//! whether or not anything visible failed. `tools/audit/check_reads.py` sorts the lines out of a log.
//!
//! The signatures the hooks run with are written beside the log (`audit_signatures.txt`, every hooked or defined
//! function's parameters and result by their RTTI types) for the offline check against what each hook reads -- with the
//! launcher's DEBUG (or CVR_LOG_VERBOSE=1) only; the "[audit]" lines themselves are always written.
use std::collections::HashSet;
use std::sync::Mutex;

static SAID: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
/// The number of distinct mistakes said (exported for the test tools: a session's count at a glance).
#[no_mangle]
pub static CyberpunkVR_AuditCount: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// A key from a site's words (FNV-1a over them).
pub(crate) fn key(words: &[u64]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for w in words {
        for b in w.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}
/// `message` logged the first time `key` is seen.
pub(crate) fn once(key: u64, message: impl FnOnce() -> String) {
    let fresh = SAID.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashSet::new).insert(key);
    if fresh {
        CyberpunkVR_AuditCount.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        cvr_host::log!("[audit] {}\n", message());
    }
}
/// This module's base (cyberpunkvrport.dll), for addresses as RVAs the build's PDB resolves.
pub(crate) fn own_base() -> usize {
    static BASE: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    let mut b = BASE.load(core::sync::atomic::Ordering::Relaxed);
    if b == 0 {
        let mut m: windows_sys::Win32::Foundation::HMODULE = core::ptr::null_mut();
        // SAFETY: the module holding this function (FROM_ADDRESS | UNCHANGED_REFCOUNT), no reference taken.
        unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleExW(6, own_base as *const u16, &mut m);
        }
        b = m as usize;
        BASE.store(b, core::sync::atomic::Ordering::Relaxed);
    }
    b
}
/// The port's own callers on this thread's stack, as RVAs (tools/audit/check_reads.py names them with the PDB).
pub(crate) fn callers() -> String {
    let mut frames = [core::ptr::null_mut::<core::ffi::c_void>(); 12];
    // SAFETY: the frames written into the local array.
    let n = unsafe { windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, frames.len() as u32, frames.as_mut_ptr(), core::ptr::null_mut()) };
    let base = own_base();
    let rvas: Vec<String> = frames[..n as usize]
        .iter()
        .map(|f| *f as usize)
        .filter(|a| *a > base && *a - base < 0x4000_0000)
        .map(|a| format!("{:#x}", a - base))
        .collect();
    format!("dll+[{}]", rvas.join(","))
}
/// A test of the guarded read from outside (tools/audit): an address written here is read once on the game thread
/// with cvr_engine::code::read; the result lands in CyberpunkVR_AuditProbeResult (1 read, 2 faulted and survived).
#[no_mangle]
pub static CyberpunkVR_AuditProbeRead: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_AuditProbeResult: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// The guarded copies' faults the first-chance handler took (cvr_engine::GUARDED_FAULTS, mirrored each frame).
#[no_mangle]
pub static CyberpunkVR_GuardedFaults: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub(crate) fn probe_read() {
    use core::sync::atomic::Ordering::Relaxed;
    CyberpunkVR_GuardedFaults.store(cvr_engine::GUARDED_FAULTS.load(Relaxed), Relaxed);
    let at = CyberpunkVR_AuditProbeRead.swap(0, Relaxed);
    if at != 0 {
        let ok = cvr_engine::code::read::<u64>(at as usize).is_some();
        CyberpunkVR_AuditProbeResult.store(if ok { 1 } else { 2 }, Relaxed);
        cvr_host::log!("[audit] probe read at {at:#x}: {}\n", if ok { "read" } else { "faulted, taken by the first-chance handler" });
    }
}
/// A type's RTTI name and kind, for a line.
pub(crate) fn type_text(ty: &red4ext_abi::rtti::IType) -> String {
    format!("{} (kind {}, {} bytes)", ty.name().text(), ty.kind(), ty.size())
}
