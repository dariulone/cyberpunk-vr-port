//! The descriptor audit's CBV origins (src/Render/StereoDescriptorAudit.cpp, `descriptors::cbvOrigins`): a CBV written
//! with a known origin (its cookie and id, its owner) keeps the owner by id together with the CBV's canonical value; an
//! id seen again must carry the same value and owner (otherwise the tracking fails); the id is forgotten when its value
//! leaves the store or the written slot does not hold it. A lookup admits the id's owner only for that exact value.
//!
//! In the Rust build (`CVR_RUST_VRIK`) Cbv's record, the store's value retirement and the lookups of CaptureCbvSource
//! and the source table / slot captures forward to `cvr_sda_origin_*` (a lookup lends the owner and copies the value
//! under the .cpp's metadata mutex, which every change takes too). `tools/single_pass_tests/descriptor_audit_test` runs
//! its cases against it in its Rust variant.

use cvr_host::fast::FastMap;
use core::ffi::c_void;
use std::sync::{Mutex, MutexGuard};

use windows::core::{IUnknown, Interface};

use crate::descriptor_store::Value;

struct Origin {
    value: Value,
    owner: IUnknown,
}
// SAFETY: the owners are the .cpp's free-threaded objects (their references taken and released from any thread there
// too).
unsafe impl Send for Origin {}
static ORIGINS: Mutex<Option<FastMap<u64, Origin>>> = Mutex::new(None);
fn origins() -> MutexGuard<'static, Option<FastMap<u64, Origin>>> {
    ORIGINS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Cbv's record: false when the id is known with another value or owner.
pub fn record(id: u64, value: &Value, owner: &IUnknown) -> bool {
    let replaced = {
        let mut guard = origins();
        let map = guard.get_or_insert_with(cvr_host::fast::fast_map);
        if let Some(found) = map.get(&id) {
            if found.value != *value || found.owner.as_raw() != owner.as_raw() {
                return false;
            }
        }
        map.insert(id, Origin { value: *value, owner: owner.clone() })
    };
    // The owner released outside the lock.
    drop(replaced);
    true
}
/// The id forgotten.
pub fn erase(id: u64) {
    let removed = origins().as_mut().and_then(|map| map.remove(&id));
    drop(removed);
}
/// The id's owner and value (the owner lent), when known.
pub fn find(id: u64) -> Option<(Value, *mut c_void)> {
    origins().as_ref()?.get(&id).map(|found| (found.value, found.owner.as_raw()))
}

// ---- the C ABI

/// # Safety
/// `value` is valid; `owner` is a live IUnknown.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_origin_record(id: u64, value: *const Value, owner: *mut c_void) -> bool {
    let Some(owner) = (unsafe { IUnknown::from_raw_borrowed(&owner) }) else {
        return false;
    };
    record(id, unsafe { &*value }, owner)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_origin_erase(id: u64) {
    erase(id);
}
/// The id's value copied into `value` and its owner lent into `owner` (valid until the id changes, which the .cpp does
/// under the same metadata mutex as its lookups): false when unknown.
///
/// # Safety
/// The outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_origin_find(id: u64, value: *mut Value, owner: *mut *mut c_void) -> bool {
    match find(id) {
        Some((found, held)) => {
            unsafe {
                *value = found;
                *owner = held;
            }
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! An id's value and owner kept, a conflicting record refused, the id forgotten and its owner released.
    use super::*;
    use windows::Win32::Graphics::Direct3D::Fxc::D3DCreateBlob;

    fn owner() -> IUnknown {
        unsafe { D3DCreateBlob(16) }.unwrap().cast().unwrap()
    }
    fn refs(object: &IUnknown) -> u32 {
        unsafe {
            let raw = object.as_raw();
            (object.vtable().AddRef)(raw);
            (object.vtable().Release)(raw)
        }
    }

    #[test]
    fn keeps_refuses_and_forgets_origins() {
        let (a, b) = (owner(), owner());
        let mut value = [0u64; 20];
        value[0] = 1;
        value[3] = 7;
        assert!(record(7, &value, &a));
        assert_eq!(refs(&a), 2);
        assert!(record(7, &value, &a));
        assert_eq!(refs(&a), 2);
        assert!(!record(7, &value, &b));
        let mut other = value;
        other[4] = 256;
        assert!(!record(7, &other, &a));
        assert_eq!(find(7), Some((value, a.as_raw())));
        erase(7);
        assert!(find(7).is_none());
        assert_eq!(refs(&a), 1);
    }
}
