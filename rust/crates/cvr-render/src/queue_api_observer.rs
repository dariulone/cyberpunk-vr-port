//! constants::ObserveQueueApi and its vtable hooks (formerly src/Render/ConstantQueueObserver.cpp): a command queue's
//! Signal and Wait (slots 14, 15) and a fence's CPU Signal (slot 10) patched once per vtable, so the registered
//! constant copies learn the queue and fence order. Each hook takes the registered queue order's recursive mutex
//! (C++'s, shared with the prepared pair and the ECL dispatch) around the original and its report; the queue calls nest
//! a depth the fence's CPU signal reports. The registered constants' API stays C++ (RegisteredConstantUpload.cpp),
//! called by its MSVC names.

use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};
use std::sync::Mutex;

type Queue = *mut c_void;
type Fence = *mut c_void;
type QueueFn = unsafe extern "system" fn(Queue, Fence, u64) -> i32;
type FenceFn = unsafe extern "system" fn(Fence, u64) -> i32;
const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
const QUEUE_SIGNAL_SLOT: usize = 14;
const FENCE_SIGNAL_SLOT: usize = 10;
const ENTRIES: usize = 16;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "cvr_nsf2_enabled"]
    fn families_enabled() -> bool;
    /// DeviceHooks.cpp: the registered queue order's recursive mutex locked (true) or unlocked.
    fn cvr_cpp_ecl_registered_order(lock: bool);
    /// The fence hook's entry (below).
    fn cvr_queue_api_fence_signal_entry();
}

/// One patched vtable: its address and the originals of its slots.
struct Entry {
    table: AtomicUsize,
    first: AtomicUsize,
    second: AtomicUsize,
}
const EMPTY: Entry = Entry { table: AtomicUsize::new(0), first: AtomicUsize::new(0), second: AtomicUsize::new(0) };
static QUEUES: [Entry; ENTRIES] = [EMPTY; ENTRIES];
static FENCES: [Entry; ENTRIES] = [EMPTY; ENTRIES];
static QUEUE_COUNT: AtomicU32 = AtomicU32::new(0);
static FENCE_COUNT: AtomicU32 = AtomicU32::new(0);
static INSTALL: Mutex<()> = Mutex::new(());
thread_local! {
    /// The queue calls in progress on this thread (the fence's CPU signal reports it).
    static QUEUE_API_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// The vtable of a COM object.
///
/// # Safety
/// `object` is a live COM object.
unsafe fn vtable(object: *mut c_void) -> *mut usize {
    // SAFETY: as documented.
    unsafe { *(object as *const *mut usize) }
}
/// The entry patched for `object`'s vtable.
///
/// # Safety
/// `object` is a live COM object.
unsafe fn find(object: *mut c_void, entries: &'static [Entry; ENTRIES], count: &AtomicU32) -> Option<&'static Entry> {
    // SAFETY: as documented.
    let table = unsafe { vtable(object) } as usize;
    entries[..count.load(Acquire) as usize].iter().find(|e| e.table.load(Relaxed) == table)
}
/// The registered queue order's recursive mutex, held for a scope.
struct OrderLock;
impl OrderLock {
    fn new() -> Self {
        // SAFETY: the C++ mutex, released in drop.
        unsafe { cvr_cpp_ecl_registered_order(true) };
        OrderLock
    }
}
impl Drop for OrderLock {
    fn drop(&mut self) {
        // SAFETY: held by this scope.
        unsafe { cvr_cpp_ecl_registered_order(false) };
    }
}
/// A queue call's depth on this thread, for its scope.
struct QueueApiScope;
impl QueueApiScope {
    fn new() -> Self {
        QUEUE_API_DEPTH.with(|d| d.set(d.get() + 1));
        QueueApiScope
    }
}
impl Drop for QueueApiScope {
    fn drop(&mut self) {
        QUEUE_API_DEPTH.with(|d| d.set(d.get() - 1));
    }
}
/// `count` slots from `slot` of `table` made writable, `write` run, the protection restored.
///
/// # Safety
/// `table` is a vtable with those slots.
unsafe fn patch(table: *mut usize, slot: usize, count: usize, write: impl FnOnce()) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};
    let mut old = 0u32;
    // SAFETY: as documented.
    if unsafe { VirtualProtect(table.add(slot).cast(), count * 8, PAGE_READWRITE, &mut old) } == 0 {
        return false;
    }
    write();
    let mut ignored = 0u32;
    // SAFETY: as above.
    unsafe { VirtualProtect(table.add(slot).cast(), count * 8, old, &mut ignored) };
    true
}

// The fence hook's entry: the caller's return address handed to the hook as its third argument. A leaf jump with no
// stack of its own, so it needs no unwind data.
core::arch::global_asm!(
    ".globl cvr_queue_api_fence_signal_entry",
    ".def cvr_queue_api_fence_signal_entry",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_queue_api_fence_signal_entry:",
    "    mov r8, [rsp]",
    "    jmp {target}",
    target = sym cpu_signal,
);
/// ID3D12Fence::Signal: the registered constants told of a CPU signal (with the queue calls in progress) once it
/// succeeded.
unsafe extern "system" fn cpu_signal(fence: Fence, value: u64, caller: u64) -> i32 {
    // SAFETY: the engine's fence.
    let Some(entry) = (unsafe { find(fence, &FENCES, &FENCE_COUNT) }) else { return E_UNEXPECTED };
    // SAFETY: the original slot taken at the patch.
    let original: FenceFn = unsafe { core::mem::transmute(entry.first.load(Relaxed)) };
    // SAFETY: plain C++ queries and the fence's own call.
    unsafe {
        if !registered_tracking() {
            return original(fence, value);
        }
        let _order = OrderLock::new();
        register_fence(fence);
        let result = original(fence, value);
        if result >= 0 {
            registered_cpu_signal(fence, value, caller, QUEUE_API_DEPTH.with(Cell::get));
        }
        result
    }
}
/// ObserveFenceApi: the fence's vtable patched once (when the table is full or cannot be written, the fence reported
/// as one whose CPU signals go unobserved).
///
/// # Safety
/// `fence` is a live ID3D12Fence or null.
unsafe fn observe_fence_api(fence: Fence) {
    // SAFETY: as documented.
    if fence.is_null() || unsafe { find(fence, &FENCES, &FENCE_COUNT) }.is_some() {
        return;
    }
    let _install = INSTALL.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: as above.
    if unsafe { find(fence, &FENCES, &FENCE_COUNT) }.is_some() {
        return;
    }
    let n = FENCE_COUNT.load(SeqCst) as usize;
    // SAFETY: as above.
    let table = unsafe { vtable(fence) };
    let hooked = n < ENTRIES
        // SAFETY: the fence's vtable slot.
        && unsafe {
            patch(table, FENCE_SIGNAL_SLOT, 1, || {
                let e = &FENCES[n];
                e.table.store(table as usize, Relaxed);
                e.first.store(*table.add(FENCE_SIGNAL_SLOT), Relaxed);
                FENCE_COUNT.store(n as u32 + 1, Release);
                core::sync::atomic::AtomicUsize::from_ptr(table.add(FENCE_SIGNAL_SLOT))
                    .swap(cvr_queue_api_fence_signal_entry as *const () as usize, SeqCst);
            })
        };
    if !hooked {
        // SAFETY: the C++ report with its defaults.
        unsafe { registered_cpu_signal(fence, 0, 0, 0) };
    }
}
/// ID3D12CommandQueue::Signal.
unsafe extern "system" fn queue_signal(queue: Queue, fence: Fence, value: u64) -> i32 {
    // SAFETY: the engine's queue and fence.
    unsafe { queue_call(queue, fence, value, false) }
}
/// ID3D12CommandQueue::Wait.
unsafe extern "system" fn queue_wait(queue: Queue, fence: Fence, value: u64) -> i32 {
    // SAFETY: as above.
    unsafe { queue_call(queue, fence, value, true) }
}
/// Signal or Wait: the fence registered and its vtable observed, the original under the order lock and the queue call's
/// depth, its success reported.
///
/// # Safety
/// The engine's queue and fence.
unsafe fn queue_call(queue: Queue, fence: Fence, value: u64, wait: bool) -> i32 {
    // SAFETY: as documented.
    let Some(entry) = (unsafe { find(queue, &QUEUES, &QUEUE_COUNT) }) else { return E_UNEXPECTED };
    let slot = if wait { &entry.second } else { &entry.first };
    // SAFETY: the original slot taken at the patch.
    let original: QueueFn = unsafe { core::mem::transmute(slot.load(Relaxed)) };
    // SAFETY: plain C++ queries and the queue's own call.
    unsafe {
        if !registered_tracking() {
            return original(queue, fence, value);
        }
        let _order = OrderLock::new();
        register_fence(fence);
        observe_fence_api(fence);
        let _scope = QueueApiScope::new();
        let result = original(queue, fence, value);
        if result >= 0 {
            if wait {
                registered_wait(queue, fence, value);
            } else {
                registered_signal(queue, fence, value);
            }
        }
        result
    }
}
/// constants::ObserveQueueApi: while bindings are observed and the families are on, the queue's vtable patched once.
///
/// # Safety
/// `queue` is a live ID3D12CommandQueue or null.
pub unsafe fn observe_queue_api(queue: Queue) {
    // SAFETY: plain queries; the queue as documented.
    if queue.is_null()
        || !crate::stereo_probe::cvr_observe_bindings()
        || !unsafe { families_enabled() }
        || unsafe { find(queue, &QUEUES, &QUEUE_COUNT) }.is_some()
    {
        return;
    }
    let _install = INSTALL.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: as above.
    if unsafe { find(queue, &QUEUES, &QUEUE_COUNT) }.is_some() {
        return;
    }
    let n = QUEUE_COUNT.load(SeqCst) as usize;
    if n >= ENTRIES {
        return;
    }
    // SAFETY: as above.
    let table = unsafe { vtable(queue) };
    // SAFETY: the queue's vtable slots.
    unsafe {
        patch(table, QUEUE_SIGNAL_SLOT, 2, || {
            let e = &QUEUES[n];
            e.table.store(table as usize, Relaxed);
            e.first.store(*table.add(QUEUE_SIGNAL_SLOT), Relaxed);
            e.second.store(*table.add(QUEUE_SIGNAL_SLOT + 1), Relaxed);
            QUEUE_COUNT.store(n as u32 + 1, Release);
            AtomicUsize::from_ptr(table.add(QUEUE_SIGNAL_SLOT)).swap(queue_signal as *const () as usize, SeqCst);
            AtomicUsize::from_ptr(table.add(QUEUE_SIGNAL_SLOT + 1)).swap(queue_wait as *const () as usize, SeqCst);
        })
    };
}

// constants::* (formerly src/Render/RegisteredConstantUpload.cpp's entry points): registered_copies.rs' entries.
unsafe fn registered_tracking() -> bool {
    crate::registered_copies::entries::cvr_rc2_tracking()
}
unsafe fn register_fence(fence: Fence) {
    crate::registered_copies::entries::cvr_rc2_register_fence(fence);
}
unsafe fn registered_cpu_signal(fence: Fence, value: u64, caller: u64, nested: u32) {
    crate::registered_copies::entries::cvr_rc2_cpu_signal(fence, value, caller, nested);
}
unsafe fn registered_signal(queue: Queue, fence: Fence, value: u64) {
    crate::registered_copies::entries::cvr_rc2_signal(queue, fence, value);
}
unsafe fn registered_wait(queue: Queue, fence: Fence, value: u64) {
    crate::registered_copies::entries::cvr_rc2_wait(queue, fence, value);
}
