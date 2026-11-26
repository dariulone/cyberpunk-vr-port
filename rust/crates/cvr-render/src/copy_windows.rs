//! The IA copy windows (`include/Render/NativeCopyWindows.hpp`, `copy_windows`): per recording (list, generation), the
//! immutable spans of source bytes already copied into a private arena, found again only by a later occurrence for a
//! fully contained range of the same resource; conservatively invalidated by any write to the resource or an
//! overlapping GPU address, unknown commands on the list, transition / UAV barriers on the resource and every aliasing
//! barrier; closed with the recording's reset; a missing observation fails everything closed. At most 128 windows and
//! 32768 live spans per window.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the registry and the windows live here: the C++ Window is a face over one
//! (owning from Create, borrowed inside Each), a Token holds one strong reference to a span's entry, and the read-only
//! scope's thread-local depth stays in C++. `tools/single_pass_tests/copy_windows_test` runs its cases against these in
//! its Rust variant.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

const MAX_WINDOWS: usize = 128;
const MAX_ENTRIES: usize = 32768;

/// copy_windows::Span.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    pub resource: u64,
    pub gpu: u64,
    pub backing: u64,
    pub offset: u64,
    pub bytes: u64,
    pub destination: u64,
    pub occurrence: u64,
}
/// copy_windows::Entry (std::atomic<bool> has a bool's layout).
#[repr(C)]
#[derive(Debug)]
pub struct Entry {
    pub span: Span,
    pub valid: AtomicBool,
}
const _: () = assert!(core::mem::size_of::<Span>() == 56 && core::mem::size_of::<Entry>() == 64);

struct Inner {
    entries: Vec<Arc<Entry>>,
    open: bool,
}
/// One recording's window.
pub struct Window {
    inner: Mutex<Inner>,
    list: u64,
    generation: u64,
}
struct Registry {
    windows: Mutex<Vec<Weak<Window>>>,
    lost: AtomicBool,
}
static REGISTRY: Registry = Registry { windows: Mutex::new(Vec::new()), lost: AtomicBool::new(false) };
/// The open windows (the C++ face reads it as a std::atomic<UINT> on every draw before calling here).
#[no_mangle]
#[allow(non_upper_case_globals)]
pub static cvr_cw_active_windows: AtomicU32 = AtomicU32::new(0);

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn lost() -> bool {
    REGISTRY.lost.load(Ordering::SeqCst)
}

impl Window {
    /// The list this window belongs to.
    pub fn list(&self) -> u64 {
        self.list
    }
    /// The list generation this window belongs to.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    fn new(list: u64, generation: u64) -> Self {
        cvr_cw_active_windows.fetch_add(1, Ordering::SeqCst);
        Window { inner: Mutex::new(Inner { entries: Vec::new(), open: true }), list, generation }
    }
    /// Find: a valid earlier occurrence's span fully containing the range.
    pub fn find(&self, resource: u64, offset: u64, bytes: u64, occurrence: u64) -> Option<Arc<Entry>> {
        if lost() || bytes == 0 || offset > u64::MAX - bytes {
            return None;
        }
        let inner = lock(&self.inner);
        if !inner.open {
            return None;
        }
        inner
            .entries
            .iter()
            .find(|entry| {
                let s = &entry.span;
                entry.valid.load(Ordering::SeqCst)
                    && s.resource == resource
                    && s.occurrence < occurrence
                    && offset >= s.offset
                    && offset - s.offset <= s.bytes
                    && bytes <= s.bytes - (offset - s.offset)
            })
            .cloned()
    }
    pub fn publish(&self, span: Span) {
        if lost()
            || span.resource == 0
            || span.gpu == 0
            || span.backing == 0
            || span.gpu > u64::MAX - span.backing
            || span.bytes == 0
            || span.offset > span.backing
            || span.bytes > span.backing - span.offset
            || span.occurrence == 0
            || span.destination > u64::MAX - span.bytes
        {
            return;
        }
        let mut inner = lock(&self.inner);
        if !inner.open {
            return;
        }
        inner.entries.retain(|entry| entry.valid.load(Ordering::SeqCst));
        if inner.entries.len() >= MAX_ENTRIES {
            return;
        }
        inner.entries.push(Arc::new(Entry { span, valid: AtomicBool::new(true) }));
    }
    pub fn write(&self, resource: u64) {
        for entry in &lock(&self.inner).entries {
            if resource == 0 || entry.span.resource == resource {
                entry.valid.store(false, Ordering::SeqCst);
            }
        }
    }
    pub fn address(&self, address: u64, bytes: u64) {
        for entry in &lock(&self.inner).entries {
            let s = &entry.span;
            let start = s.gpu.wrapping_add(s.offset);
            if address == 0
                || bytes == 0
                || address > u64::MAX - bytes
                || (address < start.wrapping_add(s.bytes) && start < address.wrapping_add(bytes))
            {
                entry.valid.store(false, Ordering::SeqCst);
            }
        }
    }
    pub fn close(&self) {
        let mut inner = lock(&self.inner);
        if inner.open {
            inner.open = false;
            cvr_cw_active_windows.fetch_sub(1, Ordering::SeqCst);
        }
        for entry in &inner.entries {
            entry.valid.store(false, Ordering::SeqCst);
        }
        inner.entries.clear();
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        self.close();
    }
}

/// Create: a new window for the recording (at most 128 live ones; none once observation was lost).
/// Valid: the entry still valid and observation not lost.
pub fn valid(entry: &Entry) -> bool {
    entry.valid.load(Ordering::Acquire) && !lost()
}
pub fn create(list: u64, generation: u64) -> Option<Arc<Window>> {
    if list == 0 || generation == 0 {
        return None;
    }
    let mut windows = lock(&REGISTRY.windows);
    windows.retain(|window| window.strong_count() != 0);
    if windows.len() >= MAX_WINDOWS || lost() {
        return None;
    }
    let window = Arc::new(Window::new(list, generation));
    windows.push(Arc::downgrade(&window));
    Some(window)
}
/// Each: every live window, under the registry's lock.
pub fn each(mut action: impl FnMut(&Window)) {
    if cvr_cw_active_windows.load(Ordering::Relaxed) == 0 {
        return;
    }
    let mut windows = lock(&REGISTRY.windows);
    let mut i = 0;
    while i < windows.len() {
        match windows[i].upgrade() {
            Some(window) => {
                action(&window);
                i += 1;
            }
            None => {
                windows.remove(i);
            }
        }
    }
}
std::thread_local! {
    /// copy_windows::ReadOnlyScope's depth on this thread: the internal draws that only read what they bind.
    static READ_ONLY_DEPTH: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
/// ReadOnlyScope: `enter` true enters one, false leaves it.
pub fn read_only(enter: bool) {
    READ_ONLY_DEPTH.with(|d| d.set(if enter { d.get().wrapping_add(1) } else { d.get().wrapping_sub(1) }));
}
/// UnknownDraw: a draw the windows cannot see into writes what it may, unless inside a read-only scope.
pub fn unknown_draw(list: u64) {
    if READ_ONLY_DEPTH.with(|d| d.get()) == 0 && cvr_cw_active_windows.load(Ordering::Relaxed) != 0 {
        cvr_cw_global(2, list, 0);
    }
}
pub fn unavailable() {
    REGISTRY.lost.store(true, Ordering::SeqCst);
}
/// Barriers: transitions and UAV barriers write their resource; an aliasing barrier writes everything.
pub fn barriers(barriers: &[[u64; 4]], missing: bool) {
    if cvr_cw_active_windows.load(Ordering::Relaxed) == 0 {
        return;
    }
    if missing {
        unavailable();
        return;
    }
    for barrier in barriers {
        let kind = barrier[0] as u32;
        match kind {
            0 | 2 => {
                let resource = barrier[1];
                each(|window| window.write(resource));
            }
            1 => each(|window| window.write(0)),
            _ => {}
        }
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Create: an owned window (Arc::into_raw), or null.
#[no_mangle]
pub extern "C-unwind" fn cvr_cw_create(list: u64, generation: u64) -> *const Window {
    match create(list, generation) {
        Some(window) => Arc::into_raw(window),
        None => core::ptr::null(),
    }
}
/// Drops the owning face's window reference (closing it first, as the C++ destructor did).
///
/// # Safety
/// `window` came from `cvr_cw_create` and is not used again by that face.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_window_drop(window: *const Window) {
    if !window.is_null() {
        let window = unsafe { Arc::from_raw(window) };
        window.close();
    }
}
/// A window's operation by `which`: Write (0: `a` resource), Address (1: `a`, `b` bytes), Close (2), List (3),
/// Generation (4).
///
/// # Safety
/// `window` is a live window.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_window(window: *const Window, which: u32, a: u64, b: u64) -> u64 {
    let window = unsafe { &*window };
    match which {
        0 => window.write(a),
        1 => window.address(a, b),
        2 => window.close(),
        3 => return window.list,
        _ => return window.generation,
    }
    0
}
/// Find: a strong entry reference (Arc::into_raw), or null.
///
/// # Safety
/// `window` is a live window.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_find(window: *const Window, resource: u64, offset: u64, bytes: u64, occurrence: u64) -> *const Entry {
    match unsafe { (*window).find(resource, offset, bytes, occurrence) } {
        Some(entry) => Arc::into_raw(entry),
        None => core::ptr::null(),
    }
}
/// Publish.
///
/// # Safety
/// `window` is a live window; `span` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_publish(window: *const Window, span: *const Span) {
    unsafe { (*window).publish(*span) };
}
/// An entry reference: retain (`which` 0) or release (1); Valid (2: the entry valid and observation not lost).
///
/// # Safety
/// `entry` is a strong reference from `cvr_cw_find` (or retained from one).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_entry(entry: *const Entry, which: u32) -> bool {
    match which {
        0 => {
            unsafe { Arc::increment_strong_count(entry) };
            true
        }
        1 => {
            unsafe { Arc::decrement_strong_count(entry) };
            true
        }
        _ => (unsafe { (*entry).valid.load(Ordering::Acquire) }) && !lost(),
    }
}
/// The C++ trampoline of an Each action: (context, borrowed window) -> completed without an exception.
pub type EachAction = unsafe extern "C-unwind" fn(*mut c_void, *const Window) -> bool;
/// Each with a C++ action; an action's exception (reported false) fails observation closed, as in C++.
///
/// # Safety
/// `action` is callable with `context`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_each(action: EachAction, context: *mut c_void) {
    let mut failed = false;
    each(|window| {
        if !failed && !unsafe { action(context, window) } {
            failed = true;
        }
    });
    if failed {
        unavailable();
    }
}
/// The global operations by `which`: Write (0: `a` resource), Address (1: `a`, `b` bytes), Unknown (2: `a` list),
/// Reset (3: `a` list), Unavailable (4).
#[no_mangle]
pub extern "C-unwind" fn cvr_cw_global(which: u32, a: u64, b: u64) {
    match which {
        0 => each(|window| window.write(a)),
        1 => each(|window| window.address(a, b)),
        2 => each(|window| {
            if window.list == a {
                window.write(0)
            }
        }),
        3 => each(|window| {
            if window.list == a {
                window.close()
            }
        }),
        _ => unavailable(),
    }
}
/// Barriers over D3D12_RESOURCE_BARRIER records (32 bytes: the type, then the union's first pointer at +8).
///
/// # Safety
/// `records` holds `count` barriers, or is null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_cw_barriers(count: u32, records: *const [u64; 4]) {
    let missing = count != 0 && records.is_null();
    let records = if records.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(records, count as usize) } };
    barriers(records, missing);
}
