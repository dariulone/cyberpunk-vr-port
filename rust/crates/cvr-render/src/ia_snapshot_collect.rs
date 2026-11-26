//! The native IA snapshot's collection steps (src/Render/NativeIaSnapshot.cpp, `ia_snapshot::Inspect` and `Collect`): a
//! source's recording context known (read known, neither pending nor submitted, the recording's own context and
//! registry; the recording's validation stays the .cpp's), its creation recorded (kind, heap, offset, bytes, initial
//! state), and an admitted source's view still describable (an index or vertex view of the observed bytes; Arguments
//! otherwise).
//!
//! In the Rust build (`CVR_RUST_VRIK`) those steps forward to `cvr_iac_*`; `tools/single_pass_tests/
//! native_ia_snapshot_test` runs its cases against them in its Rust variant, and the unit tests below cover them.

use windows::Win32::Graphics::Direct3D12::{D3D12_INDEX_BUFFER_VIEW, D3D12_VERTEX_BUFFER_VIEW};

use crate::buffer_read::NativeBuffer;
use crate::ia_snapshot::{index_view, vertex_view, Observation};
use crate::source_cbv_details::{Context, KNOWN};

const CREATION_KNOWN: u64 = 2;
/// Failure::Arguments.
const ARGUMENTS: u64 = 3;

/// Inspect's context condition (before the recording's validation).
pub fn context_admitted(context: &Context, recording_context: u64, recording_registry: u64, native: &NativeBuffer) -> bool {
    context.observation == KNOWN && context.pending == 0 && context.submitted == 0 && recording_context == native.context && recording_registry == native.registry
}
/// Inspect's creation record.
pub fn record_creation(a: &mut Observation, kind: u32, heap: u64, offset: u64, bytes: u64, initial_known: bool, initial_state: u32) {
    a.flags |= CREATION_KNOWN;
    a.creation_kind = kind as u64;
    a.creation_heap = heap;
    a.creation_offset = offset;
    a.creation_bytes = bytes;
    a.initial_known = initial_known as u64;
    a.initial_state = initial_state as u64;
}
/// Collect's view check: an admitted source whose view cannot be rebuilt from its observed bytes becomes Arguments.
pub fn check_view(a: &mut Observation, index: &D3D12_INDEX_BUFFER_VIEW, vertex: &D3D12_VERTEX_BUFFER_VIEW) {
    if a.reason != 0 {
        return;
    }
    let valid = if a.role == 0 { index_view(a.view_gpu, 0, a.view_bytes, index).is_some() } else { vertex_view(a.view_gpu, 0, a.view_bytes, vertex).is_some() };
    if !valid {
        a.reason = ARGUMENTS;
    }
}

// ---- the C ABI

/// # Safety
/// `context` and `native` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iac_context_admitted(context: *const Context, recording_context: u64, recording_registry: u64, native: *const NativeBuffer) -> bool {
    context_admitted(unsafe { &*context }, recording_context, recording_registry, unsafe { &*native })
}
/// # Safety
/// `a` is valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_iac_record_creation(a: *mut Observation, kind: u32, heap: u64, offset: u64, bytes: u64, initial_known: bool, initial_state: u32) {
    record_creation(unsafe { &mut *a }, kind, heap, offset, bytes, initial_known, initial_state);
}
/// # Safety
/// All pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iac_check_view(a: *mut Observation, index: *const D3D12_INDEX_BUFFER_VIEW, vertex: *const D3D12_VERTEX_BUFFER_VIEW) {
    check_view(unsafe { &mut *a }, unsafe { &*index }, unsafe { &*vertex });
}

#[cfg(test)]
mod tests {
    //! The context condition, the creation record and the view check.
    use super::*;

    #[test]
    fn contexts_and_creations() {
        let native = NativeBuffer { context: 5, registry: 6, ..Default::default() };
        let known = Context { observation: KNOWN, ..Default::default() };
        assert!(context_admitted(&known, 5, 6, &native));
        assert!(!context_admitted(&known, 4, 6, &native));
        assert!(!context_admitted(&known, 5, 7, &native));
        assert!(!context_admitted(&Context { pending: 1, ..known }, 5, 6, &native));
        assert!(!context_admitted(&Context { submitted: 1, ..known }, 5, 6, &native));
        assert!(!context_admitted(&Context::default(), 5, 6, &native));
        let mut a = Observation::default();
        record_creation(&mut a, 2, 0x10, 0x20, 0x30, true, 0xAC3);
        assert_eq!((a.flags, a.creation_kind, a.creation_heap, a.creation_offset, a.creation_bytes, a.initial_known, a.initial_state), (CREATION_KNOWN, 2, 0x10, 0x20, 0x30, 1, 0xAC3));
    }
    #[test]
    fn views() {
        let vertex = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x1000, SizeInBytes: 64, StrideInBytes: 16 };
        let index = D3D12_INDEX_BUFFER_VIEW::default();
        let mut a = Observation { role: 1, view_gpu: 0x1000, view_bytes: 64, ..Default::default() };
        let rebuilt = vertex_view(a.view_gpu, 0, a.view_bytes, &vertex).is_some();
        check_view(&mut a, &index, &vertex);
        assert_eq!(a.reason, if rebuilt { 0 } else { ARGUMENTS });
        let mut refused = Observation { reason: 7, ..a };
        check_view(&mut refused, &index, &vertex);
        assert_eq!(refused.reason, 7);
        let mut empty = Observation { role: 1, ..Default::default() };
        check_view(&mut empty, &index, &D3D12_VERTEX_BUFFER_VIEW::default());
        assert_eq!(empty.reason, if vertex_view(0, 0, 0, &D3D12_VERTEX_BUFFER_VIEW::default()).is_some() { 0 } else { ARGUMENTS });
    }
}
