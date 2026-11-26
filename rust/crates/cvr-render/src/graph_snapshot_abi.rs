//! The C ABI of the compiled render graph snapshot (native::graph_snapshot) for include/Render/NativeGraphSnapshot.hpp:
//! in the Rust build the header's Snapshot::Capture hands the graph and the reader here and copies the nodes and edges
//! into its own vectors. tools/single_pass_tests/graph_snapshot_test runs its cases against it in its Rust variant.
use core::ffi::c_void;

use crate::native::graph_snapshot::{Edge, Node, Snapshot};

/// graph_snapshot::Read.
pub type Read = unsafe extern "C-unwind" fn(u64, *mut c_void, usize) -> bool;

/// A captured graph's records, borrowed until cvr_gs_free.
#[repr(C)]
pub struct View {
    pub nodes: *const Node,
    pub node_count: usize,
    pub edges: *const Edge,
    pub edge_count: usize,
}

/// Snapshot::Capture: the snapshot with the records read so far (all of them when `failure` is 0; through `view`,
/// free it with cvr_gs_free), or null without a reader. `failure`: 1 no header, 2 counts or tables, 3 a node, 4 an
/// edge, 5 changed while read, 0 captured.
///
/// # Safety
/// `view` and `failure` are writable; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gs_capture(
    graph: u64,
    read: Option<Read>,
    view: *mut View,
    failure: *mut u32,
) -> *mut Snapshot {
    unsafe {
        *view = View {
            nodes: core::ptr::null(),
            node_count: 0,
            edges: core::ptr::null(),
            edge_count: 0,
        };
        *failure = 1;
        let Some(read) = read else {
            return core::ptr::null_mut();
        };
        let mut snapshot = Snapshot::default();
        let mut adapter =
            move |at: u64, out: &mut [u8]| read(at, out.as_mut_ptr().cast(), out.len());
        snapshot.capture(graph, &mut adapter);
        *failure = snapshot.failure;
        let snapshot = Box::into_raw(Box::new(snapshot));
        *view = View {
            nodes: (*snapshot).nodes.as_ptr(),
            node_count: (*snapshot).nodes.len(),
            edges: (*snapshot).edges.as_ptr(),
            edge_count: (*snapshot).edges.len(),
        };
        snapshot
    }
}

/// # Safety
/// `snapshot` comes from cvr_gs_capture (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gs_free(snapshot: *mut Snapshot) {
    if !snapshot.is_null() {
        drop(unsafe { Box::from_raw(snapshot) });
    }
}
