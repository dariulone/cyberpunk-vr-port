//! The descriptor audit's tracked tables (src/Render/StereoDescriptorAudit.cpp, `descriptors::tables`): while recording
//! or observing bindings, each command list's root signature and the descriptor tables bound to it (a new root forgets
//! them; at most 512 lists, a known list is still updated), and the descriptor address a tracked table of a list's root
//! puts a shader-visible slot at (the table bound, base + offset * stride inside 64 bits).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Root, Table, ClearBindings, Reset's and the capture boundaries' forgetting and
//! the resolutions' table addresses forward to `cvr_sda_tables_*`; a lookup lends the list's tables, which stay put under
//! the .cpp's metadata mutex (every change takes it too). `tools/single_pass_tests/descriptor_audit_test` runs its
//! cases against it in its Rust variant.

use cvr_host::fast::FastMap;
use std::sync::{Mutex, MutexGuard};

use crate::audit_capture::Tables;

const LIST_LIMIT: usize = 512;

static TABLES: Mutex<Option<FastMap<u64, Box<Tables>>>> = Mutex::new(None);
fn tables() -> MutexGuard<'static, Option<FastMap<u64, Box<Tables>>>> {
    TABLES.lock().unwrap_or_else(|e| e.into_inner())
}
fn empty(root: u64) -> Box<Tables> {
    Box::new(Tables { root, mask: 0, addresses: [0; 64] })
}

/// Root: the list's root; a different one forgets its tables.
pub fn root(list: u64, root: u64) {
    let mut guard = tables();
    let map = guard.get_or_insert_with(cvr_host::fast::fast_map);
    if map.len() >= LIST_LIMIT && !map.contains_key(&list) {
        return;
    }
    let entry = map.entry(list).or_insert_with(|| empty(0));
    if entry.root != root {
        *entry = empty(root);
    }
}
/// Table: one bound table of a tracked list.
pub fn table(list: u64, index: u32, address: u64) {
    if index >= 64 {
        return;
    }
    if let Some(entry) = tables().as_mut().and_then(|map| map.get_mut(&list)) {
        entry.addresses[index as usize] = address;
        entry.mask |= 1 << index;
    }
}
/// ClearBindings: the list's tables forgotten, its root kept.
pub fn clear_bindings(list: u64) {
    if let Some(entry) = tables().as_mut().and_then(|map| map.get_mut(&list)) {
        entry.addresses = [0; 64];
        entry.mask = 0;
    }
}
/// Reset: the list forgotten.
pub fn erase(list: u64) {
    if let Some(map) = tables().as_mut() {
        map.remove(&list);
    }
}
/// The capture boundaries: every list forgotten.
pub fn clear() {
    if let Some(map) = tables().as_mut() {
        map.clear();
    }
}
/// The descriptor address of slot `offset` in the list's table `parameter`, when the list's root is `root` and the
/// table is bound.
pub fn address(list: u64, root: u64, parameter: u32, offset: u32, stride: u32) -> Option<u64> {
    let guard = tables();
    let entry = guard.as_ref()?.get(&list)?;
    if parameter >= 64 || entry.root != root || entry.mask & (1 << parameter) == 0 {
        return None;
    }
    let base = entry.addresses[parameter as usize];
    let delta = offset as u64 * stride as u64;
    if base > u64::MAX - delta {
        return None;
    }
    Some(base + delta)
}

// ---- the C ABI

/// `which`: 0 Root (`value` the root), 1 Table (`index`, `value` the address), 2 ClearBindings, 3 Reset's erase,
/// 4 every list forgotten.
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_tables_update(which: u32, list: u64, index: u32, value: u64) {
    match which {
        0 => root(list, value),
        1 => table(list, index, value),
        2 => clear_bindings(list),
        3 => erase(list),
        4 => clear(),
        _ => {}
    }
}
/// The list's tables lent (null when untracked): valid until the next change, which the .cpp makes under the same
/// metadata mutex as its lookups.
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_tables_find(list: u64) -> *const Tables {
    tables().as_ref().and_then(|map| map.get(&list)).map_or(core::ptr::null(), |entry| &**entry as *const Tables)
}
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_tables_address(list: u64, root: u64, parameter: u32, offset: u32, stride: u32, out: *mut u64) -> bool {
    match address(list, root, parameter, offset, stride) {
        Some(value) => {
            unsafe { *out = value };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! A list's root change, the list limit and the table address's bounds.
    use super::*;

    #[test]
    fn tracks_roots_tables_and_addresses() {
        clear();
        root(1, 0x10);
        table(1, 3, 0x1000);
        assert_eq!(address(1, 0x10, 3, 2, 32), Some(0x1040));
        assert_eq!(address(1, 0x20, 3, 2, 32), None);
        assert_eq!(address(1, 0x10, 4, 0, 32), None);
        root(1, 0x10);
        assert_eq!(address(1, 0x10, 3, 0, 32), Some(0x1000));
        root(1, 0x20);
        assert_eq!(address(1, 0x20, 3, 0, 32), None);
        table(1, 63, u64::MAX - 8);
        assert_eq!(address(1, 0x20, 63, 1, 8), Some(u64::MAX));
        assert_eq!(address(1, 0x20, 63, 1, 9), None);
        clear_bindings(1);
        assert_eq!(address(1, 0x20, 63, 0, 8), None);
        for list in 2..=LIST_LIMIT as u64 {
            root(list, 0x30);
        }
        root(10_000, 0x30);
        assert!(cvr_sda_tables_find(10_000).is_null());
        root(1, 0x40);
        assert_eq!(unsafe { (*cvr_sda_tables_find(1)).root }, 0x40);
        erase(1);
        assert!(cvr_sda_tables_find(1).is_null());
        root(10_000, 0x30);
        assert!(!cvr_sda_tables_find(10_000).is_null());
        clear();
        assert!(cvr_sda_tables_find(10_000).is_null());
    }
}
