//! The descriptor audit's source slot walk (src/Render/StereoDescriptorAudit.cpp, `descriptors::CaptureSourceTablesAtDraw`
//! and `CaptureSourceSlotsAtDraw`): under the metadata lock, for each selected slot of a captured source table (in slot
//! order, at its offset in the table's range) its canonical value in the store (5 when absent or past 64 bits), a valid
//! descriptor of the slot's kind (6), its stamp (the slot's GPU address, the range's heap, the value), and the owners it
//! needs alive (7 when one is missing): a CBV's known origin for exactly that value, an SRV's resource and a UAV's
//! resource and counter -- a tracked resource of a live lifetime, on the capture's device, still carrying its cookie.
//!
//! In the Rust build (`CVR_RUST_VRIK`) both captures' slot loops forward to `cvr_sda_source_walk` on the .cpp's Rust
//! store; the owners come back as references the .cpp attaches. `tools/single_pass_tests/source_table_capture_test` and
//! `source_sparse_capture_test` run their cases against it in their Rust variants.

use core::ffi::c_void;

use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Object, ID3D12Resource};

use crate::audit_lifetimes::live_resource;
use crate::audit_origins;
use crate::audit_values::held_cookie;
use crate::descriptor_store::{RangeIdentity, Store, Value};
use crate::source_layout::source_descriptor_valid;

/// One slot of a captured table as the .cpp lends it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SlotIn {
    pub offset: u32,
    pub kind: u32,
    pub used: bool,
}
const _: () = assert!(core::mem::size_of::<SlotIn>() == 12);

/// One walked slot: its value, its stamp's address and heap, and the owners (references the .cpp takes over).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SlotOut {
    pub value: Value,
    pub gpu: u64,
    pub heap: u64,
    pub origin: *mut c_void,
    pub resource: *mut c_void,
    pub counter: *mut c_void,
}
const _: () = assert!(core::mem::size_of::<SlotOut>() == 200);

/// A failure: the source error's reason and the slot's offset.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Failure {
    pub reason: u32,
    pub offset: u32,
}

/// The walked slot's owners, held until the .cpp takes them.
#[derive(Default)]
struct Owners {
    origin: Option<IUnknown>,
    resource: Option<ID3D12Resource>,
    counter: Option<ID3D12Resource>,
}

/// SourceResource: no cookie needs nothing; otherwise the tracked resource of a live lifetime, on `device`, still
/// carrying the cookie.
fn source_resource(cookie: u64, device: &ID3D12Device) -> Result<Option<ID3D12Resource>, ()> {
    if cookie == 0 {
        return Ok(None);
    }
    let raw = live_resource(cookie).ok_or(())?;
    // SAFETY: a resource whose lifetime is alive; the reference taken here keeps it.
    let held = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.ok_or(())?.clone();
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live resource.
    if unsafe { held.GetDevice(&mut actual) }.is_err() || actual.as_ref().map(|d| d.as_raw()) != Some(device.as_raw()) {
        return Err(());
    }
    if held_cookie(&held.cast::<ID3D12Object>().map_err(|_| ())?) != Some(cookie) {
        return Err(());
    }
    Ok(Some(held))
}

/// The walk: `out` filled for every used slot, or the first failing slot's failure.
fn walk_owned(store: &Store, range: &RangeIdentity, device: &ID3D12Device, slots: &[SlotIn], out: &mut [SlotOut]) -> Result<Vec<Owners>, Failure> {
    let mut owners: Vec<Owners> = (0..slots.len()).map(|_| Owners::default()).collect();
    let stride = range.stride as u64;
    for (n, slot) in slots.iter().enumerate() {
        if !slot.used {
            continue;
        }
        let fail = |reason| Failure { reason, offset: slot.offset };
        let delta = slot.offset as u64 * stride;
        if range.cpu > u64::MAX - delta || range.gpu > u64::MAX - delta {
            return Err(fail(5));
        }
        let value = *store.get(store.at(range.cpu + delta)).ok_or(fail(5))?;
        if !source_descriptor_valid(&value, slot.kind) {
            return Err(fail(6));
        }
        out[n].value = value;
        out[n].gpu = range.gpu + delta;
        out[n].heap = range.heap;
        if slot.kind == 1 && value[3] != 0 {
            let (known, owner) = audit_origins::find(value[3]).ok_or(fail(7))?;
            if known != value {
                return Err(fail(7));
            }
            // SAFETY: the origin's owner, alive while the origin is kept (under the .cpp's metadata lock).
            owners[n].origin = unsafe { IUnknown::from_raw_borrowed(&owner) }.cloned();
        }
        if slot.kind == 2 || slot.kind == 3 {
            owners[n].resource = source_resource(value[2], device).map_err(|_| fail(7))?;
            if slot.kind == 3 {
                owners[n].counter = source_resource(value[3], device).map_err(|_| fail(7))?;
            }
        }
    }
    Ok(owners)
}
/// The walk with the owners handed to `out` as references.
pub fn walk(store: &Store, range: &RangeIdentity, device: &ID3D12Device, slots: &[SlotIn], out: &mut [SlotOut]) -> Result<(), Failure> {
    let owners = walk_owned(store, range, device, slots, out)?;
    for (slot, held) in out.iter_mut().zip(owners) {
        slot.origin = held.origin.map_or(core::ptr::null_mut(), |o| o.into_raw());
        slot.resource = held.resource.map_or(core::ptr::null_mut(), |r| r.into_raw());
        slot.counter = held.counter.map_or(core::ptr::null_mut(), |r| r.into_raw());
    }
    Ok(())
}

// ---- the C ABI

/// The walk: true with `out` filled (the owners as references the caller takes over), otherwise false with `failure`
/// set and no references handed out.
///
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `range` is valid; `device` is a live device; `slots` and
/// `out` hold `count` entries; `failure` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_source_walk(
    store: *const Store,
    range: *const RangeIdentity,
    device: *mut c_void,
    slots: *const SlotIn,
    count: usize,
    out: *mut SlotOut,
    failure: *mut Failure,
) -> bool {
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        unsafe { *failure = Failure { reason: 1, offset: u32::MAX } };
        return false;
    };
    let (slots, out) = if count == 0 {
        (&[][..], &mut [][..])
    } else {
        unsafe { (core::slice::from_raw_parts(slots, count), core::slice::from_raw_parts_mut(out, count)) }
    };
    for slot in out.iter_mut() {
        *slot = SlotOut { value: [0; 20], gpu: 0, heap: 0, origin: core::ptr::null_mut(), resource: core::ptr::null_mut(), counter: core::ptr::null_mut() };
    }
    match walk(unsafe { &*store }, unsafe { &*range }, device, slots, out) {
        Ok(()) => true,
        Err(error) => {
            unsafe { *failure = error };
            false
        }
    }
}
