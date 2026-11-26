//! The C ABI of a render graph node's parent receipt (native::graph_parent) for include/Render/NativeGraphParent.hpp:
//! the one-shot original-image code certificate, the native context's exact view and order slot read twice, and the
//! source-bound resolution of the parent node in a graph snapshot. tools/single_pass_tests/graph_parent_test runs its
//! cases against these in its Rust variant.
use crate::graph_snapshot_abi::Read;
use crate::native::graph_parent::{read_context, resolve, Code, CodeReceipt, Context, Receipt};
use crate::native::graph_snapshot::{Node, Snapshot};

fn reader(read: Read) -> impl FnMut(u64, &mut [u8]) -> bool {
    move |at, out| unsafe { read(at, out.as_mut_ptr().cast(), out.len()) }
}
unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}

/// CodeReceipt::Capture's certificate (the C++ face keeps the one-shot state): every function's original bytes
/// match their FNV-1a hash.
///
/// # Safety
/// `functions` holds `count` codes; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_certify(
    base: u64,
    read: Option<Read>,
    functions: *const Code,
    count: usize,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        let mut receipt = CodeReceipt::default();
        receipt.capture(
            base,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            slice(functions, count),
        )
    }
}

/// ReadContext: the context read twice and equal (`output` zeroed on a refusal).
///
/// # Safety
/// `output` is writable; `read` follows graph_snapshot::Read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_read_context(
    read: Option<Read>,
    address: u64,
    output: *mut Context,
) -> bool {
    unsafe {
        let mut adapter = read.map(reader);
        let context = read_context(
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            address,
        );
        *output = context.unwrap_or_default();
        context.is_some()
    }
}

/// Resolve against a graph snapshot's nodes and failure (`output` zeroed on a refusal).
///
/// # Safety
/// `nodes` holds `count` nodes; `context` is valid; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_resolve(
    nodes: *const Node,
    count: usize,
    failure: u32,
    context: *const Context,
    output: *mut Receipt,
) -> bool {
    unsafe {
        let source = Snapshot {
            nodes: slice(nodes, count).to_vec(),
            edges: Vec::new(),
            failure,
        };
        let receipt = resolve(&source, &*context);
        *output = receipt.unwrap_or_default();
        receipt.is_some()
    }
}
