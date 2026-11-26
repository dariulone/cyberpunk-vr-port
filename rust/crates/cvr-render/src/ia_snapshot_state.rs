//! The native IA snapshot's state inspection (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Inspect`): a source's read
//! state at the draw recorded in its observation (value, status, kind), the reads its role needs, the state required
//! for the copy (the prior state plus copy source), the admission's reason, then a resident buffer's residency
//! (ResidencyDeferred) and, for a buffer not already a copy source that is not an upload, the context's barrier capacity
//! (Capacity).
//!
//! In the Rust build (`CVR_RUST_VRIK`) that tail forwards to `cvr_iai_inspect_state`;
//! `tools/single_pass_tests/native_ia_snapshot_test` runs its cases against it in its Rust variant, and the unit test
//! below covers the reasons.

use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_COPY_SOURCE;

use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot::{admit, read_bit, Observation};
use crate::ia_snapshot_native::{residency, transition_capacity};
use crate::source_cbv_details::ReadState;

/// Failure::Capacity and Failure::ResidencyDeferred.
pub const CAPACITY: u64 = 23;
pub const RESIDENCY_DEFERRED: u64 = 32;

/// Inspect's state tail: (before, reads, required).
pub fn inspect_state(a: &mut Observation, state: ReadState, native: &NativeBuffer) -> (u32, u32, u32) {
    a.state_value = state.value as u64;
    a.state_status = state.status as u64;
    a.state_kind = state.kind as u64;
    let copy_source = D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;
    let (before, reads) = (state.value, read_bit(a.role));
    a.reason = admit(a) as u64;
    if a.reason == 0 && !residency(native, a.record_address) {
        a.reason = RESIDENCY_DEFERRED;
    }
    if a.reason == 0 && native.kind != 1 && state.value & copy_source == 0 && !transition_capacity(native) {
        a.reason = CAPACITY;
    }
    (before, reads, before | copy_source)
}

// ---- the C ABI

/// # Safety
/// All pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iai_inspect_state(a: *mut Observation, state: *const ReadState, native: *const NativeBuffer, before: *mut u32, reads: *mut u32, required: *mut u32) {
    let (b, r, q) = inspect_state(unsafe { &mut *a }, unsafe { *state }, unsafe { &*native });
    unsafe {
        *before = b;
        *reads = r;
        *required = q;
    }
}

#[cfg(test)]
mod tests {
    //! The recorded state, the reads and required state, and the admission's first reason.
    use super::*;

    #[test]
    fn state_tail() {
        let mut a = Observation { role: 0, ..Default::default() };
        let (before, reads, required) = inspect_state(&mut a, ReadState { value: 0x2, status: 2, kind: 0 }, &NativeBuffer::default());
        assert_eq!((a.state_value, a.state_status, a.state_kind), (0x2, 2, 0));
        assert_eq!((before, reads, required), (0x2, read_bit(0), 0x2 | 0x800));
        assert_eq!(a.reason, admit(&a) as u64);
        assert_ne!(a.reason, 0);
    }
}
