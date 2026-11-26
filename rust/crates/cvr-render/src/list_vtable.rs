//! The command lists' vtable hooks (formerly src/Stereo/DeviceHooks.cpp's patch_command_list_vtable,
//! command_list_hook_entry and command_list_hook_entries): per distinct list vtable (at most 16) the record of its
//! originals (the census' CommandListVtableHook, mirrored here), the interface versions sharing that vtable told
//! apart (List1/2/4/5/6/7 by QueryInterface on the same object and table), the slots up to the newest shared version
//! made writable once and pointed at the detours as their table says (always, with or without the native probe,
//! per interface version), the pair probe and single pass installed over it, the protection restored.
//!
//! The detours and their table are list_detours'; the .cpp lends the probe's switch, the observers' unavailability,
//! the pair probe and the log.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};
use std::sync::Mutex;
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::E_NOINTERFACE;
use windows::Win32::Graphics::Direct3D12::*;

const CAPACITY: usize = 16;
const FIRST_SLOT: usize = 9;

/// CommandListVtableHook (include/Stereo/StereoInternal.hpp).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Entry {
    /// vtable, then the originals of slots 46, 26, 26, 17, 16, 59, 12, 13, 15, 14, 21, 22, 10, 25, 44, 47, 29, 31,
    /// 30, 32, 38, 43, 55, 28, 20, 24, 77 and 78.
    pub head: [usize; 29],
    pub shading_state_known: bool,
    /// The originals of slots 9, 11, 27, 51, 23, 54, 66, 80, 45, 18, 60, 61, 48, 19, 64, 68, 69, 49, 50, 70, 71,
    /// 72, 73, 74, 76 and 79.
    pub tail: [usize; 26],
}
const _: () = assert!(core::mem::size_of::<Entry>() == 448);
const HEAD_SLOTS: [usize; 28] = [46, 26, 26, 17, 16, 59, 12, 13, 15, 14, 21, 22, 10, 25, 44, 47, 29, 31, 30, 32, 38, 43, 55, 28, 20, 24, 77, 78];
/// The tail's slots with the interface version each needs (0 any).
const TAIL_SLOTS: [(usize, u32); 26] = [
    (9, 0),
    (11, 0),
    (27, 0),
    (51, 0),
    (23, 0),
    (54, 0),
    (66, SAME2),
    (80, SAME7),
    (45, 0),
    (18, 0),
    (60, SAME1),
    (61, SAME1),
    (48, 0),
    (19, 0),
    (64, SAME1),
    (68, SAME4),
    (69, SAME4),
    (49, 0),
    (50, 0),
    (70, SAME4),
    (71, SAME4),
    (72, SAME4),
    (73, SAME4),
    (74, SAME4),
    (76, SAME4),
    (79, SAME6),
];
// A detour's (or an original's) conditions: all its bits must hold.
pub const PROBE: u32 = 1;
pub const NO_PROBE: u32 = 2;
pub const SAME1: u32 = 4;
pub const SAME2: u32 = 8;
pub const SAME4: u32 = 16;
pub const SAME6: u32 = 32;
pub const SAME7: u32 = 64;
pub const LIST5: u32 = 128;
/// A table row's index for the pair probe's installation (when the probe is off and the pair probe wanted).
pub const PAIR_INSTALL: u32 = u32::MAX;
/// A table row's index for the binding hooks' count (when the probe is on).
pub const BINDING_COUNT: u32 = u32::MAX - 1;

/// One row of the .cpp's detour table.
#[repr(C)]
pub struct Slot {
    pub index: u32,
    pub conditions: u32,
    pub detour: *const c_void,
}
/// What the subsystems lend (formerly src/Stereo/DeviceHooks.cpp's table, now built here).
#[repr(C)]
pub struct Runtime {
    pub slots: *const Slot,
    pub slot_count: usize,
    /// native_probe::ObserveBindings() || native_probe::Enabled() || families::Enabled().
    pub probe_enabled: extern "C" fn() -> bool,
    /// texture/buffer reads' and target access's observers made unavailable.
    pub read_unavailable: extern "C" fn(),
    /// target_access::ObserverUnavailable.
    pub access_unavailable: extern "C" fn(),
    /// pair_probe::Wanted, then pair_probe::Install(vtable).
    pub pair_wanted: extern "C" fn() -> bool,
    pub pair_install: extern "C" fn(vtable: *mut c_void),
    pub binding_hooks: *const AtomicU32,
    pub log: extern "C" fn(text: *const u8),
}
// SAFETY: immutable; its pointers name process-lifetime tables, exports and functions.
unsafe impl Sync for Runtime {}
mod lent {
    //! The probes' switches, the observers' unavailability, the pair probe and the stereo log.
    use core::ffi::c_void;
    unsafe extern "C" {
        #[link_name = "cvr_nsf2_enabled"]
        fn families_enabled() -> bool;
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const core::ffi::c_char);
    }
    /// native_probe::ObserveBindings() || native_probe::Enabled() || families::Enabled().
    pub(super) extern "C" fn probe_enabled() -> bool {
        // SAFETY: a plain C++ query (families::Enabled); native_probe::Enabled is stereo_probe_creation.rs'.
        crate::stereo_probe::cvr_observe_bindings() || crate::stereo_probe_creation::enabled() || unsafe { families_enabled() }
    }
    /// The reads' observers unavailable: the targets' access, the textures' and the buffers'.
    pub(super) extern "C" fn read_unavailable() {
        // SAFETY: plain calls.
        unsafe { crate::list_detours::cvr_lh_reads_unavailable() };
    }
    pub(super) extern "C" fn access_unavailable() {
        crate::target_access_observer::events::observer_unavailable();
    }
    pub(super) extern "C" fn pair_wanted() -> bool {
        crate::pair_probe::cvr_pair_probe_wanted()
    }
    pub(super) extern "C" fn pair_install(vtable: *mut c_void) {
        // SAFETY: the list's vtable.
        unsafe { crate::pair_probe::cvr_pair_probe_install(vtable.cast()) };
    }
    pub(super) extern "C" fn log(text: *const u8) {
        // SAFETY: a terminated line.
        unsafe { cvr_cpp_stereo_log(text.cast()) };
    }
}
static RT: Runtime = Runtime {
    slots: core::ptr::null(),
    slot_count: 0,
    probe_enabled: lent::probe_enabled,
    read_unavailable: lent::read_unavailable,
    access_unavailable: lent::access_unavailable,
    pair_wanted: lent::pair_wanted,
    pair_install: lent::pair_install,
    binding_hooks: &raw const crate::declared_moved_exports::CyberpunkVR_NativeStereoBindingHooks,
    log: lent::log,
};
fn rt() -> Option<&'static Runtime> {
    Some(&RT)
}

struct Entries(core::cell::UnsafeCell<[Entry; CAPACITY]>);
// SAFETY: an entry is written once under INSTALL before COUNT publishes it, then only read.
unsafe impl Sync for Entries {}
static ENTRIES: Entries = Entries(core::cell::UnsafeCell::new([Entry { head: [0; 29], shading_state_known: false, tail: [0; 26] }; CAPACITY]));
static COUNT: AtomicU32 = AtomicU32::new(0);
static INSTALL: Mutex<()> = Mutex::new(());

fn entries() -> &'static [Entry] {
    let n = (COUNT.load(Acquire) as usize).min(CAPACITY);
    // SAFETY: the published prefix, never written again.
    unsafe { &(&*ENTRIES.0.get())[..n] }
}
/// command_list_hook_entry: the record of the list's vtable.
pub fn entry(list: *mut c_void) -> Option<&'static Entry> {
    if list.is_null() {
        return None;
    }
    // SAFETY: a live list's vtable pointer.
    let table = unsafe { *list.cast::<usize>() };
    entries().iter().find(|e| e.head[0] == table)
}

/// The interface version's QueryInterface result and whether it is the same object and table.
fn version<T: Interface>(list: &ID3D12GraphicsCommandList, table: usize) -> (HRESULT, bool) {
    match list.cast::<T>() {
        // SAFETY: the interface's vtable pointer.
        Ok(v) => (HRESULT(0), v.as_raw() == list.as_raw() && unsafe { *v.as_raw().cast::<usize>() } == table),
        Err(e) => (e.code(), false),
    }
}
/// patch_command_list_vtable.
pub fn patch(list: *mut c_void) {
    let Some(r) = rt() else { return };
    // SAFETY: the caller's live list.
    let Some(object) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }) else { return };
    let vtable = unsafe { *list.cast::<*mut usize>() };
    let table = vtable as usize;
    let _g = INSTALL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let count = COUNT.load(Relaxed) as usize;
    if entries().iter().any(|e| e.head[0] == table) {
        return;
    }
    if count >= CAPACITY {
        (r.read_unavailable)();
        return;
    }
    let probe = (r.probe_enabled)();
    if !probe {
        (r.access_unavailable)();
    }
    let (list5_result, same5) = version::<ID3D12GraphicsCommandList5>(object, table);
    let (v1, same1) = version::<ID3D12GraphicsCommandList1>(object, table);
    let (v2, same2) = version::<ID3D12GraphicsCommandList2>(object, table);
    let (v4, same4) = version::<ID3D12GraphicsCommandList4>(object, table);
    let (v7, same7) = version::<ID3D12GraphicsCommandList7>(object, table);
    let (v6, same6) = version::<ID3D12GraphicsCommandList6>(object, table);
    if !same6 && v6 != E_NOINTERFACE {
        (r.read_unavailable)();
    }
    if (!same1 && v1 != E_NOINTERFACE) || (!same2 && v2 != E_NOINTERFACE) || (!same4 && v4 != E_NOINTERFACE) || (!same7 && v7 != E_NOINTERFACE) {
        (r.read_unavailable)();
    }
    let mut held = 0u32;
    for (same, bit) in [(same1, SAME1), (same2, SAME2), (same4, SAME4), (same6, SAME6), (same7, SAME7), (same5, LIST5)] {
        if same {
            held |= bit;
        }
    }
    held |= if probe { PROBE } else { NO_PROBE };
    // SAFETY: the vtable's slots up to the newest shared version (read before any is replaced).
    let slot = |i: usize| unsafe { *vtable.add(i) };
    let mut entry = Entry { head: [0; 29], shading_state_known: probe && (same5 || list5_result == E_NOINTERFACE), tail: [0; 26] };
    entry.head[0] = table;
    for (i, &s) in HEAD_SLOTS.iter().enumerate() {
        entry.head[i + 1] = if s >= 77 && !same5 { 0 } else { slot(s) };
    }
    for (i, &(s, needs)) in TAIL_SLOTS.iter().enumerate() {
        entry.tail[i] = if held & needs == needs { slot(s) } else { 0 };
    }
    let last = if same7 {
        80
    } else if same6 {
        79
    } else if same5 {
        78
    } else if same4 {
        76
    } else if same2 {
        66
    } else if same1 {
        64
    } else {
        59
    };
    let bytes = (last - FIRST_SLOT + 1) * 8;
    let mut old = 0u32;
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};
    // SAFETY: the vtable's slot range.
    if unsafe { VirtualProtect(vtable.add(FIRST_SLOT).cast(), bytes, PAGE_READWRITE, &mut old) } == 0 {
        (r.read_unavailable)();
        (r.log)(format!("[mirror] cannot make command-list vtable writable: {:016X}\0", vtable as usize).as_ptr());
        return;
    }
    // SAFETY: the next unpublished entry, under INSTALL.
    unsafe { (*ENTRIES.0.get())[count] = entry };
    COUNT.store(count as u32 + 1, Release);
    // SAFETY: the .cpp's table (its own detours), then the Rust detours' (list_detours).
    let rows = if r.slots.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(r.slots, r.slot_count) } };
    for row in rows.iter().chain(crate::list_detours::slots()) {
        if held & row.conditions != row.conditions {
            continue;
        }
        match row.index {
            PAIR_INSTALL => {
                if (r.pair_wanted)() {
                    (r.pair_install)(vtable.cast());
                }
            }
            BINDING_COUNT => {
                // SAFETY: the export.
                if let Some(c) = unsafe { r.binding_hooks.as_ref() } {
                    c.fetch_add(1, Release);
                }
            }
            index => {
                // SAFETY: a writable slot of the range.
                unsafe { (*vtable.add(index as usize).cast::<AtomicUsize>()).swap(row.detour as usize, SeqCst) };
            }
        }
    }
    // SAFETY: the writable range; single pass installs over the hooks.
    unsafe { crate::single_pass::cvr_single_pass_install(vtable, last) };
    let mut ignored = 0u32;
    unsafe { VirtualProtect(vtable.add(FIRST_SLOT).cast(), bytes, old, &mut ignored) };
    (r.log)(
        format!(
            "[mirror] command-list hooked list={:016X} vt={:016X} om={:016X} rb={:016X} cr={:016X}\0",
            list as usize, vtable as usize, entry.head[1], entry.head[2], entry.head[4]
        )
        .as_ptr(),
    );
}

// ---- the C ABI
/// # Safety
/// A live list (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_lvt_entry(list: *mut c_void) -> *const Entry {
    entry(list).map_or(core::ptr::null(), |e| e as *const Entry)
}
/// The census' hook record of a list (formerly src/Stereo/SinglePass.cpp's service over DeviceHooks.cpp's
/// command_list_hook_entry): capture_stable.rs and census_draws.rs read its originals.
///
/// # Safety
/// A live list (or null).
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const c_void {
    // SAFETY: as documented.
    unsafe { cvr_lvt_entry(list) }.cast()
}
/// # Safety
/// `out` holds `max` pointers.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_lvt_entries(out: *mut *const Entry, max: u32) -> u32 {
    let all = entries();
    let n = all.len().min(max as usize);
    for (i, e) in all[..n].iter().enumerate() {
        unsafe { *out.add(i) = e };
    }
    n as u32
}
/// # Safety
/// A live list (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_lvt_patch(list: *mut c_void) {
    if !list.is_null() {
        patch(list);
    }
}
