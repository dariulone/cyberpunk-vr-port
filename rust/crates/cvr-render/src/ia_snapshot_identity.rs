//! The native IA snapshot's identity re-checks (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Physical`, `Identity` and
//! `Check`): a source's allocation still physically its own (kind 1 exactly when placed, no heap unless placed, the
//! source's resource, at least the source's bytes), the recording context unchanged (known, at the same address with the
//! same list, state and kind, not submitted, and not pending unless a pending copy is being recovered), and the source's
//! read state at the draw (a kind-1 upload in generic read under its kind, otherwise the read mask of its kind and reads
//! with the required bits once ready or the unchanged prior state before).
//!
//! In the Rust build (`CVR_RUST_VRIK`) those checks forward to `cvr_iai_*`; `tools/single_pass_tests/native_ia_snapshot_test`
//! runs its cases against them in its Rust variant, and the unit tests below cover each condition.

use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATE_GENERIC_READ;

use crate::ia_snapshot::read_mask;
use crate::source_cbv_details::{Allocation, Context, ReadState, KNOWN};

/// Physical's allocation checks.
pub fn physical_allocation(allocation: &Allocation, placed: bool, resource: u64, bytes: u64) -> bool {
    allocation.kind == placed as u32 && (placed || allocation.heap == 0) && allocation.resource == resource && allocation.bytes >= bytes
}
/// Identity's context checks.
pub fn same_context(current: &Context, saved: &Context, pending: bool) -> bool {
    current.observation == KNOWN
        && current.address == saved.address
        && current.list == saved.list
        && current.state == saved.state
        && current.kind == saved.kind
        && current.submitted == 0
        && (pending || current.pending == 0)
}
/// Check's state checks.
pub fn state_ready(state: ReadState, kind: u32, reads: u32, required: u32, before: u32, ready: bool) -> bool {
    if kind == 1 {
        return state.status == 8 && state.kind == 1 && state.value == D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
    }
    read_mask(state, kind, reads, ready) && if ready { state.value & required == required } else { state.value == before }
}

// ---- the C ABI

/// # Safety
/// `allocation` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iai_physical_allocation(allocation: *const Allocation, placed: bool, resource: u64, bytes: u64) -> bool {
    physical_allocation(unsafe { &*allocation }, placed, resource, bytes)
}
/// # Safety
/// Both contexts are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iai_same_context(current: *const Context, saved: *const Context, pending: bool) -> bool {
    same_context(unsafe { &*current }, unsafe { &*saved }, pending)
}
/// # Safety
/// `state` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iai_state_ready(state: *const ReadState, kind: u32, reads: u32, required: u32, before: u32, ready: bool) -> bool {
    state_ready(unsafe { *state }, kind, reads, required, before, ready)
}

#[cfg(test)]
mod tests {
    //! Each allocation, context and state condition.
    use super::*;

    #[test]
    fn allocations() {
        // SAFETY: a plain record, all zero.
        let mut a: Allocation = unsafe { core::mem::zeroed() };
        a.resource = 5;
        a.bytes = 64;
        assert!(physical_allocation(&a, false, 5, 64));
        assert!(!physical_allocation(&a, false, 5, 65));
        assert!(!physical_allocation(&a, false, 6, 64));
        assert!(!physical_allocation(&a, true, 5, 64));
        a.kind = 1;
        a.heap = 9;
        assert!(physical_allocation(&a, true, 5, 64));
        assert!(!physical_allocation(&a, false, 5, 64));
        a.kind = 0;
        assert!(!physical_allocation(&a, false, 5, 64));
    }
    #[test]
    fn contexts() {
        let saved = Context { observation: KNOWN, address: 1, list: 2, state: 3, submitted: 0, kind: 4, pending: 0 };
        assert!(same_context(&saved, &saved, false));
        assert!(!same_context(&Context { observation: 5, ..saved }, &saved, false));
        assert!(!same_context(&Context { submitted: 1, ..saved }, &saved, true));
        assert!(!same_context(&Context { pending: 1, ..saved }, &saved, false));
        assert!(same_context(&Context { pending: 1, ..saved }, &saved, true));
        assert!(!same_context(&Context { state: 9, ..saved }, &saved, false));
        assert!(same_context(&saved, &Context { submitted: 7, pending: 7, observation: 0, ..saved }, false));
    }
    #[test]
    fn states() {
        let generic = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
        assert!(state_ready(ReadState { value: generic, status: 8, kind: 1 }, 1, 0, 0, 0, false));
        assert!(!state_ready(ReadState { value: generic, status: 2, kind: 1 }, 1, 0, 0, 0, false));
        let reads = 0x2;
        assert!(state_ready(ReadState { value: reads, status: 2, kind: 0 }, 0, reads, 0, reads, false));
        assert!(!state_ready(ReadState { value: reads, status: 2, kind: 0 }, 0, reads, 0, reads | 0x1, false));
        let ready = reads | 0x800;
        assert!(state_ready(ReadState { value: ready, status: 2, kind: 0 }, 0, reads, ready, 0, true));
        assert!(!state_ready(ReadState { value: ready, status: 2, kind: 0 }, 0, reads, ready | 0x40, 0, true));
    }
}
