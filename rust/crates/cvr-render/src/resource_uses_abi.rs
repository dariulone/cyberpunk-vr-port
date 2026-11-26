//! The C ABI of the native resource-use pool snapshot (native::resource_uses) for include/Render/NativeResourceUses.hpp:
//! in the Rust build the header's Snapshot::CapturePool hands the pool, the exact source graph and the reader here and
//! copies the captured records into its own members. tools/single_pass_tests/resource_uses_test runs its cases against
//! it in its Rust variant.
use core::ffi::c_void;

use crate::native::graph_snapshot::{self, Node};
use crate::native::resource_uses::{Declaration, Snapshot};

/// resource_uses::ReadMemory: (user, address, out, bytes) -> readable.
pub type ReadMemory = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;

/// A capture's fields and records, borrowed until cvr_ru_free.
#[repr(C)]
pub struct UsesOut {
    pub pool: u64,
    pub context_root: u64,
    pub pool_header: [u8; 4],
    pub failure: u32,
    pub kinds: [u32; 14],
    pub opaque_state_unsupported: bool,
    pub source_nodes: *const Node,
    pub source_count: usize,
    pub use_ids: *const u64,
    pub use_count: usize,
    pub boundary_ids: *const u64,
    pub boundary_count: usize,
    pub auxiliary_keys: *const u32,
    pub auxiliary_count: usize,
    pub declarations: *const Declaration,
    pub declaration_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<UsesOut>() == 168 && core::mem::offset_of!(UsesOut, source_nodes) == 88
);

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}

/// Snapshot::CapturePool: the snapshot (accepted when `out.failure` is 0; its records through `out`, free it with
/// cvr_ru_free).
///
/// # Safety
/// `nodes` holds `node_count` nodes; `out` is writable; `read` follows resource_uses::ReadMemory.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ru_capture(
    address: u64,
    nodes: *const Node,
    node_count: usize,
    graph_failure: u32,
    read: Option<ReadMemory>,
    user: *mut c_void,
    root: u64,
    out: *mut UsesOut,
) -> *mut Snapshot {
    unsafe {
        let graph = graph_snapshot::Snapshot {
            nodes: slice(nodes, node_count).to_vec(),
            edges: Vec::new(),
            failure: graph_failure,
        };
        let mut adapter = read.map(|read| {
            move |at: u64, out: &mut [u8]| read(user, at, out.as_mut_ptr().cast(), out.len())
        });
        let mut snapshot = Snapshot::default();
        snapshot.capture_pool(
            address,
            &graph,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            root,
        );
        let s = Box::into_raw(Box::new(snapshot));
        let r = &*s;
        *out = UsesOut {
            pool: r.pool,
            context_root: r.context_root,
            pool_header: r.pool_header,
            failure: r.failure as u32,
            kinds: r.kinds,
            opaque_state_unsupported: r.opaque_state_unsupported,
            source_nodes: r.source_nodes.as_ptr(),
            source_count: r.source_nodes.len(),
            use_ids: r.use_ids.as_ptr(),
            use_count: r.use_ids.len(),
            boundary_ids: r.boundary_ids.as_ptr(),
            boundary_count: r.boundary_ids.len(),
            auxiliary_keys: r.auxiliary_keys.as_ptr(),
            auxiliary_count: r.auxiliary_keys.len(),
            declarations: r.declarations.as_ptr(),
            declaration_count: r.declarations.len(),
        };
        s
    }
}

/// # Safety
/// `snapshot` comes from cvr_ru_capture (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ru_free(snapshot: *mut Snapshot) {
    if !snapshot.is_null() {
        drop(unsafe { Box::from_raw(snapshot) });
    }
}
