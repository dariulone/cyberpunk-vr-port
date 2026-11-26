//! The vtable slot registry (formerly PatchVtableMethod in src/Hooks/DepthCapture.cpp, the registry in
//! src/Hooks/SwapChain.cpp and include/Hooks/OriginalMethodTable.hpp). The COM objects the port intercepts -- DXGI
//! adapters and outputs, the D3D12 device, command lists and queues -- are hooked by replacing a slot of their vtable,
//! and the detour finds the method it replaced here.
//!
//! A slot is patched ONCE PER (VTABLE, SLOT): many objects share one vtable, and patching a slot that already holds
//! the detour must not record the detour as its own original (the call would recurse). The first original recorded
//! for a key wins and is never replaced or erased.
//!
//! Lookups run on the render jobs' hottest paths (CopyDescriptors, ResourceBarrier, CreateShaderResourceView ...) from
//! every dispatcher thread, so the read side is a lock-free open-addressed table: the value is stored before the key
//! is released, and a reader that acquires the key sees the value. Writers are serialized by the registry's mutex. A
//! full table or a long probe falls back to the locked map, so behaviour never depends on capacity. The exported
//! `CyberpunkVR_FastOriginalLookup` (kept by the C++ half, attached here) set to 0 takes the locked map for every
//! lookup, the previous behaviour kept for a same-launch A/B.
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use std::collections::BTreeMap;
use std::sync::Mutex;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// MakeVtableSlotKey: the vtable's address mixed with the slot.
pub fn slot_key(vtable: usize, slot: usize) -> usize {
    vtable ^ (slot as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) as usize
}

/// OriginalMethodTable: the lock-free read side. `N` is a power of two.
pub struct Table<const N: usize> {
    keys: [AtomicUsize; N],
    values: [AtomicPtr<c_void>; N],
}

impl<const N: usize> Default for Table<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Table<N> {
    pub const MAX_PROBE: usize = 64;
    const LOG2: u32 = N.trailing_zeros();

    pub const fn new() -> Self {
        assert!(N >= 16 && N.is_power_of_two());
        Self {
            keys: [const { AtomicUsize::new(0) }; N],
            values: [const { AtomicPtr::new(core::ptr::null_mut()) }; N],
        }
    }

    fn home(key: usize) -> usize {
        ((key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - Self::LOG2)) as usize
    }

    /// Caller serializes writers. False only when the key cannot be placed (a zero key or value, or the probe ran
    /// out); the locked map then remains the authority for that key.
    pub fn publish(&self, key: usize, value: *mut c_void) -> bool {
        if key == 0 || value.is_null() {
            return false;
        }
        let mut slot = Self::home(key);
        for _ in 0..Self::MAX_PROBE {
            let existing = self.keys[slot].load(Ordering::Relaxed);
            if existing == key {
                return true; // the first original wins, exactly like the map
            }
            if existing == 0 {
                self.values[slot].store(value, Ordering::Relaxed);
                self.keys[slot].store(key, Ordering::Release);
                return true;
            }
            slot = (slot + 1) & (N - 1);
        }
        false
    }

    pub fn find(&self, key: usize) -> *mut c_void {
        if key == 0 {
            return core::ptr::null_mut();
        }
        let mut slot = Self::home(key);
        for _ in 0..Self::MAX_PROBE {
            let existing = self.keys[slot].load(Ordering::Acquire);
            if existing == key {
                return self.values[slot].load(Ordering::Relaxed);
            }
            if existing == 0 {
                return core::ptr::null_mut();
            }
            slot = (slot + 1) & (N - 1);
        }
        core::ptr::null_mut()
    }
}

static TABLE: Table<4096> = Table::new();
/// The authority: key -> original (addresses as usize). Its lock also serializes the writers of TABLE.
static MAP: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());
/// CyberpunkVR_FastOriginalLookup (C++ export); unattached means the lock-free lookup.
static FAST: AtomicPtr<AtomicU32> = AtomicPtr::new(core::ptr::null_mut());

fn fast_lookup() -> bool {
    let flag = FAST.load(Ordering::Relaxed);
    // SAFETY: an exported variable of the C++ half, alive for the process.
    flag.is_null() || unsafe { (*flag).load(Ordering::Relaxed) } != 0
}

fn map() -> std::sync::MutexGuard<'static, BTreeMap<usize, usize>> {
    MAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// GetOriginalMethod: what the slot held before the port's detour, or null when it was never patched.
pub fn original(vtable: *mut *mut c_void, slot: usize) -> *mut c_void {
    let key = slot_key(vtable as usize, slot);
    if fast_lookup() {
        let published = TABLE.find(key);
        if !published.is_null() {
            return published;
        }
    }
    map()
        .get(&key)
        .map_or(core::ptr::null_mut(), |&p| p as *mut c_void)
}

/// PatchVtableMethod: put `hook` in the slot, remembering the original first. True when the slot holds the detour
/// with a recorded original afterwards.
///
/// # Safety
/// `vtable` is a live vtable with at least `slot + 1` entries.
pub unsafe fn patch(vtable: *mut *mut c_void, slot: usize, hook: *mut c_void) -> bool {
    if vtable.is_null() || hook.is_null() {
        return false;
    }
    // SAFETY: the caller's live vtable; the slot is made writable before it is written.
    unsafe {
        let entry = vtable.add(slot);
        let key = slot_key(vtable as usize, slot);
        // The entry is read under the lock (main's 597540e3e).
        let mut map = map();
        let method = *entry;
        if method.is_null() {
            return false;
        }
        // A later overlay may own this entry now and chain to our hook: replacing it while keeping our first original
        // would cut the overlay out of the chain, and taking the overlay as the original would make a cycle (ours ->
        // overlay -> ours). An entry already registered is done.
        if map.contains_key(&key) {
            return true;
        }
        if method == hook {
            // no known continuation for this hook
            return false;
        }
        let mut old = 0u32;
        if VirtualProtect(entry as *const c_void, 8, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        map.insert(key, method as usize);
        let first = method as usize;
        // Publish the original before the detour becomes reachable.
        TABLE.publish(key, first as *mut c_void);
        *entry = hook;
        let mut ignored = 0u32;
        VirtualProtect(entry as *const c_void, 8, old, &mut ignored);
        FlushInstructionCache(GetCurrentProcess(), entry as *const c_void, 8);
        true
    }
}

/// The C++ half's exported A/B switch (Hooks/SwapChain.cpp keeps the export).
#[no_mangle]
pub extern "C" fn cvr_vtable_attach(fast_original_lookup: *mut AtomicU32) {
    FAST.store(fast_original_lookup, Ordering::Release);
}

/// PatchVtableMethod for the C++ half.
///
/// # Safety
/// As [`patch`].
#[no_mangle]
pub unsafe extern "C" fn cvr_vtable_patch(
    vtable: *mut *mut c_void,
    slot: usize,
    hook: *mut c_void,
) -> bool {
    // SAFETY: forwarded from the caller.
    unsafe { patch(vtable, slot, hook) }
}

/// GetOriginalMethod for the C++ half.
#[no_mangle]
pub extern "C" fn cvr_vtable_original(vtable: *mut *mut c_void, slot: usize) -> *mut c_void {
    original(vtable, slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    // tools/single_pass_tests/original_method_table_test.cpp
    fn key(vtable: usize, slot: usize) -> usize {
        slot_key(vtable, slot)
    }
    fn value(n: usize) -> *mut c_void {
        (0x10000 + n * 16) as *mut c_void
    }

    #[test]
    fn functional() {
        let table = Table::<64>::new();
        assert!(table.find(key(0x1000, 1)).is_null());
        assert!(!table.publish(0, value(1)));
        assert!(!table.publish(key(0x1000, 1), core::ptr::null_mut()));
        assert!(table.find(0).is_null());
        assert!(table.publish(key(0x1000, 1), value(1)));
        assert_eq!(table.find(key(0x1000, 1)), value(1));
        // First original wins, like the map: a second publish of the same key keeps the first value.
        assert!(table.publish(key(0x1000, 1), value(2)));
        assert_eq!(table.find(key(0x1000, 1)), value(1));
        // Fill to capacity; every published key stays retrievable, then publishing more fails closed.
        let mut placed = 1;
        let mut i = 2;
        while placed < 64 {
            if table.publish(key(0x1000 + i * 0x40, i % 80), value(i)) {
                placed += 1;
            }
            i += 1;
        }
        assert!(!table.publish(key(0xDEAD_0000, 3), value(999)));
        assert!(table.find(key(0xDEAD_0000, 3)).is_null());
        assert_eq!(table.find(key(0x1000, 1)), value(1));
    }

    // Writer publishes while readers probe: a reader that finds a key must see exactly its value.
    #[test]
    fn concurrent() {
        static TABLE: Table<4096> = Table::new();
        const KEYS: usize = 2000;
        let k = |i: usize| key(0x7FF6_0000_0000 + i * 0x100, i % 100);
        let done = AtomicBool::new(false);
        let wrong = AtomicUsize::new(0);
        let seen = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for t in 0..8usize {
                let (done, wrong, seen) = (&done, &wrong, &seen);
                s.spawn(move || {
                    let mut local = 0;
                    let mut pass = 0;
                    while !done.load(Ordering::Acquire) || pass < 3 {
                        let mut i = 1 + t;
                        while i <= KEYS {
                            let found = TABLE.find(k(i));
                            if !found.is_null() {
                                local += 1;
                                if found != value(i) {
                                    wrong.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            i += 8;
                        }
                        pass += 1;
                    }
                    seen.fetch_add(local, Ordering::Relaxed);
                });
            }
            let writer = Mutex::new(());
            for i in 1..=KEYS {
                let _lock = writer.lock().unwrap();
                assert!(TABLE.publish(k(i), value(i)));
                if i % 50 == 0 {
                    std::thread::yield_now();
                }
            }
            done.store(true, Ordering::Release);
        });
        assert_eq!(wrong.load(Ordering::Relaxed), 0);
        assert!(seen.load(Ordering::Relaxed) > 0);
        for i in 1..=KEYS {
            assert_eq!(TABLE.find(k(i)), value(i));
        }
    }

    // The registry: first original wins, a re-patch with the detour does not record the detour, the A/B switch.
    #[test]
    fn patch_and_original() {
        extern "C" fn first() {}
        extern "C" fn detour() {}
        extern "C" fn later() {}
        let mut vtable: Vec<*mut c_void> = vec![first as *mut c_void; 4];
        let table = vtable.as_mut_ptr();
        // SAFETY: a local four-entry table.
        unsafe {
            assert!(original(table, 2).is_null());
            assert!(patch(table, 2, detour as *mut c_void));
            assert_eq!(vtable[2], detour as *mut c_void);
            assert_eq!(original(table, 2), first as *mut c_void);
            // Already the detour: nothing recorded again, still reported patched.
            assert!(patch(table, 2, detour as *mut c_void));
            assert_eq!(original(table, 2), first as *mut c_void);
            // A later overlay put its method in the slot (chaining to ours): the entry is left to it, and the first
            // original stays the one returned -- no cut chain, no cycle.
            *table.add(2) = later as *mut c_void;
            assert!(patch(table, 2, detour as *mut c_void));
            assert_eq!(vtable[2], later as *mut c_void);
            assert_eq!(original(table, 2), first as *mut c_void);
            // The hook already in an entry never registered: no known continuation.
            *table.add(0) = detour as *mut c_void;
            assert!(!patch(table, 0, detour as *mut c_void));
            assert!(!patch(table, 3, core::ptr::null_mut()));
            *table.add(1) = core::ptr::null_mut();
            assert!(!patch(table, 1, detour as *mut c_void));
            // The locked arm answers the same.
            let flag = Box::leak(Box::new(AtomicU32::new(0)));
            cvr_vtable_attach(flag);
            assert_eq!(original(table, 2), first as *mut c_void);
            FAST.store(core::ptr::null_mut(), Ordering::Release);
        }
    }
}
