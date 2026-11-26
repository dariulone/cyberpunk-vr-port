//! The native IA snapshot's shared backing rule (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Collect`): native
//! tracked maps are per handle while a buffer transition is whole-COM, so two sources on one COM resource whose kinds need
//! a uniform-state proof (either tracked per subresource) and that are not the same native buffer are both deferred
//! (BackingDeferred).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Collect's pairwise walk forwards to `cvr_iab_shared_backing` (the reasons written
//! back); `tools/single_pass_tests/native_ia_snapshot_test` runs its cases against it in its Rust variant, and the unit
//! test below covers the pairs.

use crate::allocation_identity::same_native;
use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot::requires_uniform_state_proof;

/// Failure::BackingDeferred.
pub const BACKING_DEFERRED: u64 = 19;
const _: () = assert!(BACKING_DEFERRED == crate::ia_snapshot::Failure::BackingDeferred as u64);

/// The pairwise walk: each deferred source's reason set.
pub fn shared_backing(resources: &[u64], natives: &[NativeBuffer], reasons: &mut [u64]) {
    for i in 0..resources.len() {
        for j in 0..i {
            if resources[i] == resources[j] && requires_uniform_state_proof(natives[i].kind, natives[j].kind) && !same_native(&natives[i], &natives[j]) {
                reasons[i] = BACKING_DEFERRED;
                reasons[j] = BACKING_DEFERRED;
            }
        }
    }
}

// ---- the C ABI

/// # Safety
/// The arrays hold `count` entries each.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iab_shared_backing(resources: *const u64, natives: *const NativeBuffer, reasons: *mut u64, count: usize) {
    if count == 0 {
        return;
    }
    let (resources, natives, reasons) =
        unsafe { (core::slice::from_raw_parts(resources, count), core::slice::from_raw_parts(natives, count), core::slice::from_raw_parts_mut(reasons, count)) };
    shared_backing(resources, natives, reasons);
}

#[cfg(test)]
mod tests {
    //! Shared resources with and without a proof requirement, and the same native buffer twice.
    use super::*;

    #[test]
    fn pairs() {
        let a = NativeBuffer { resource: 1, handle: 1, kind: 0, ..Default::default() };
        let b = NativeBuffer { resource: 1, handle: 2, kind: 1, ..Default::default() };
        let c = NativeBuffer { resource: 2, handle: 3, kind: 1, ..Default::default() };
        let mut reasons = [0u64, 0, 0];
        shared_backing(&[10, 10, 11], &[a, b, c], &mut reasons);
        assert_eq!(reasons, [BACKING_DEFERRED, BACKING_DEFERRED, 0]);
        let mut reasons = [0u64, 7];
        shared_backing(&[10, 10], &[a, a], &mut reasons);
        assert_eq!(reasons, [0, 7]);
        let d = NativeBuffer { kind: 1, handle: 9, ..b };
        let mut reasons = [0u64, 0];
        shared_backing(&[10, 10], &[b, d], &mut reasons);
        assert_eq!(reasons, [0, 0]);
    }
}
