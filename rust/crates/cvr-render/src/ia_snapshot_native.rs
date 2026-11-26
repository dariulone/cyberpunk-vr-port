//! The native IA snapshot's engine reads (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Record`, `Residency` and
//! `TransitionCapacity`): a native buffer's 0xB0-byte record in its registry (+0x5C0AF8, 0xB0 a handle from 1, at most
//! 0x100000 handles; two agreeing reads naming the buffer's bytes, GPU address, resource and kind), its residency (no
//! resident resource, or the buffer's own with the context's tracker not rejecting it) and the context's barrier
//! capacity.
//!
//! In the Rust build (`CVR_RUST_VRIK`) Record, Residency and TransitionCapacity forward to `cvr_ian_*`;
//! `tools/single_pass_tests/native_ia_snapshot_test` runs its cases against them in its Rust variant, and the unit tests
//! below read fabricated layouts.

use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot::barrier_capacity;
use crate::instance_layout::{read, read_u32, read_u64};

const RECORD: usize = 0xB0;

fn field_u32(raw: &[u8; RECORD], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
fn field_u64(raw: &[u8; RECORD], at: usize) -> u64 {
    u64::from_le_bytes(raw[at..at + 8].try_into().unwrap())
}

/// Record: `raw` and `address` left as the .cpp leaves them (zero before the guards, the first read after); true when
/// both reads agree and name the buffer.
pub fn record(native: &NativeBuffer, raw: &mut [u8; RECORD], address: &mut u64) -> bool {
    *raw = [0; RECORD];
    *address = 0;
    if native.registry == 0 || native.context == 0 || native.handle == 0 || native.handle > 0x100000 {
        return false;
    }
    let offset = 0x5C0AF8 + (native.handle - 1) as u64 * RECORD as u64;
    if native.registry > u64::MAX - offset - RECORD as u64 {
        return false;
    }
    *address = native.registry + offset;
    let mut again = [0u8; RECORD];
    if !read(*address, raw) || !read(*address, &mut again) || *raw != again {
        return false;
    }
    field_u32(raw, 0) as u64 == native.bytes && field_u64(raw, 8) == native.gpu && field_u64(raw, 16) == native.resource && (raw[6] >> 4) as u32 == native.kind
}
/// Residency.
pub fn residency(source: &NativeBuffer, address: u64) -> bool {
    if address == 0 || address > u64::MAX - 0x70 {
        return false;
    }
    let Some(resident) = read_u64(address + 0x68) else { return false };
    if resident == 0 {
        return true;
    }
    if resident != source.resource || source.context > u64::MAX - 0x638 {
        return false;
    }
    let Some(tracker) = read_u64(source.context + 0x630) else { return false };
    if tracker == 0 || tracker > u64::MAX - 38 {
        return false;
    }
    let mut rejected = [0u8; 1];
    read(tracker + 37, &mut rejected) && rejected[0] == 0
}
/// TransitionCapacity.
pub fn transition_capacity(source: &NativeBuffer) -> bool {
    if source.context == 0 || source.context > u64::MAX - 0x538 {
        return false;
    }
    match (read_u64(source.context + 0x528), read_u32(source.context + 0x530), read_u32(source.context + 0x534)) {
        (Some(data), Some(capacity), Some(pending)) => barrier_capacity(data, capacity, pending),
        _ => false,
    }
}

// ---- the C ABI

/// # Safety
/// `native` is valid; `raw` has room for 0xB0 bytes; `address` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ian_record(native: *const NativeBuffer, raw: *mut [u8; RECORD], address: *mut u64) -> bool {
    record(unsafe { &*native }, unsafe { &mut *raw }, unsafe { &mut *address })
}
/// # Safety
/// `source` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ian_residency(source: *const NativeBuffer, address: u64) -> bool {
    residency(unsafe { &*source }, address)
}
/// # Safety
/// `source` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ian_transition_capacity(source: *const NativeBuffer) -> bool {
    transition_capacity(unsafe { &*source })
}

#[cfg(test)]
mod tests {
    //! A fabricated record, residency tracker and context.
    use super::*;

    #[test]
    fn records() {
        let mut memory = vec![0u8; 0x400];
        let base = memory.as_mut_ptr() as u64;
        let native = NativeBuffer { resource: 0x77, gpu: 0x1000, bytes: 256, registry: base.wrapping_sub(0x5C0AF8 + 0xB0), context: 1, handle: 2, kind: 3 };
        unsafe {
            *(base as *mut u32) = 256;
            *((base + 8) as *mut u64) = 0x1000;
            *((base + 16) as *mut u64) = 0x77;
            *((base + 6) as *mut u8) = 0x36;
        }
        let (mut raw, mut address) = ([0u8; RECORD], 0u64);
        assert!(record(&native, &mut raw, &mut address));
        assert_eq!((address, raw[6]), (base, 0x36));
        assert!(!record(&NativeBuffer { kind: 2, ..native }, &mut raw, &mut address));
        assert_eq!(address, base);
        assert!(!record(&NativeBuffer { bytes: 255, ..native }, &mut raw, &mut address));
        assert!(!record(&NativeBuffer { context: 0, ..native }, &mut raw, &mut address));
        assert_eq!((address, raw[6]), (0, 0));
        drop(memory);
    }
    #[test]
    fn residency_and_capacity() {
        let mut record = vec![0u64; 16];
        let at = record.as_mut_ptr() as u64;
        let mut context = vec![0u8; 0x700];
        let ctx = context.as_mut_ptr() as u64;
        let mut tracker = vec![0u8; 64];
        let tr = tracker.as_mut_ptr() as u64;
        let source = NativeBuffer { resource: 0x55, context: ctx, ..Default::default() };
        assert!(residency(&source, at));
        record[0x68 / 8] = 0x66;
        assert!(!residency(&source, at));
        record[0x68 / 8] = 0x55;
        assert!(!residency(&source, at));
        unsafe { *((ctx + 0x630) as *mut u64) = tr };
        assert!(residency(&source, at));
        tracker[37] = 1;
        assert!(!residency(&source, at));
        assert!(!residency(&source, 0));
        unsafe {
            *((ctx + 0x528) as *mut u64) = 0x1234;
            *((ctx + 0x530) as *mut u32) = 16;
        }
        assert_eq!(transition_capacity(&source), barrier_capacity(0x1234, 16, 0));
        assert!(!transition_capacity(&NativeBuffer { context: 0, ..source }));
        drop((record, context, tracker));
    }
}
