//! The C ABI of the definition tree snapshot and its allocator binding receipt (native::definition_tree,
//! native::allocator_binding) for include/Render/NativeDefinitionTree.hpp and NativeAllocatorBinding.hpp: in the Rust
//! build the headers' Snapshot and Receipt keep their records in their own C++ members and hand them here to capture,
//! revalidate, bind and classify. tools/single_pass_tests/definition_tree_test, allocator_binding_test and
//! native_stage_definition_test run their cases against these in their Rust variants.
use core::ffi::c_void;

use crate::native::allocator_binding::{
    self as binding, Allocation, Bounds, ExtentFact, Identity, LayoutFacts, Lifecycle, Receipt,
};
use crate::native::definition_tree::{
    Array, Failure, GlobalAllocator, Handle, NativeVector16, Node, Root, Snapshot, StringStorage,
};

/// definition_tree::ReadMemory.
pub type ReadMemory = unsafe extern "C-unwind" fn(u64, *mut c_void, usize) -> bool;
/// allocator_binding::ObserveLifecycle.
pub type ObserveLifecycle = unsafe extern "C-unwind" fn(*mut c_void, *mut Lifecycle) -> bool;

/// definition_tree::RustView: a snapshot's records, borrowed. Every record came from cvr_dt_capture.
#[repr(C)]
pub struct View {
    pub roots: *const Root,
    pub root_count: usize,
    pub nodes: *const Node,
    pub node_count: usize,
    pub handles: *const Handle,
    pub handle_count: usize,
    pub arrays: *const Array,
    pub array_count: usize,
    pub strings: *const StringStorage,
    pub string_count: usize,
    pub allocators: *const GlobalAllocator,
}

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}

/// The view's records as a ready snapshot.
unsafe fn restored(view: &View, image_base: u64, guarded: bool) -> Snapshot {
    unsafe {
        let mut allocators = [GlobalAllocator::default(); 2];
        allocators.copy_from_slice(slice(view.allocators, 2));
        Snapshot::restored(
            slice(view.roots, view.root_count).to_vec(),
            slice(view.nodes, view.node_count).to_vec(),
            slice(view.handles, view.handle_count).to_vec(),
            slice(view.arrays, view.array_count).to_vec(),
            slice(view.strings, view.string_count).to_vec(),
            allocators,
            image_base,
            guarded,
        )
    }
}

pub(crate) fn reader(read: ReadMemory) -> impl FnMut(u64, &mut [u8]) -> bool {
    move |at, out| unsafe { read(at, out.as_mut_ptr().cast(), out.len()) }
}

/// Snapshot::Capture: the ready snapshot (read its records with cvr_dt_view, free it with cvr_dt_free) or null with
/// the failure.
///
/// # Safety
/// `roots` holds `count` values; `failure` is writable; `read` follows definition_tree::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dt_capture(
    roots: *const u64,
    count: usize,
    image_base: u64,
    read: Option<ReadMemory>,
    guarded: bool,
    failure: *mut u32,
) -> *mut Snapshot {
    unsafe {
        let mut snapshot = Snapshot::default();
        let mut adapter = read.map(reader);
        let captured = snapshot.capture(
            slice(roots, count),
            image_base,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            guarded,
        );
        *failure = snapshot.rejection() as u32;
        if captured {
            Box::into_raw(Box::new(snapshot))
        } else {
            core::ptr::null_mut()
        }
    }
}

/// A captured snapshot's records, valid until cvr_dt_free.
///
/// # Safety
/// `snapshot` comes from cvr_dt_capture; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dt_view(snapshot: *const Snapshot, out: *mut View) {
    unsafe {
        let snapshot = &*snapshot;
        *out = View {
            roots: snapshot.roots().as_ptr(),
            root_count: snapshot.roots().len(),
            nodes: snapshot.nodes().as_ptr(),
            node_count: snapshot.nodes().len(),
            handles: snapshot.handles().as_ptr(),
            handle_count: snapshot.handles().len(),
            arrays: snapshot.arrays().as_ptr(),
            array_count: snapshot.arrays().len(),
            strings: snapshot.strings().as_ptr(),
            string_count: snapshot.strings().len(),
            allocators: snapshot.allocators().as_ptr(),
        };
    }
}

/// # Safety
/// `snapshot` comes from cvr_dt_capture (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dt_free(snapshot: *mut Snapshot) {
    if !snapshot.is_null() {
        drop(unsafe { Box::from_raw(snapshot) });
    }
}

/// Snapshot::MatchesBefore of a ready snapshot's records.
///
/// # Safety
/// The view holds its counts of records from cvr_dt_capture; `read` follows definition_tree::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_dt_matches_before(
    view: *const View,
    image_base: u64,
    read: Option<ReadMemory>,
    guarded: bool,
) -> bool {
    let Some(read) = read else { return false };
    unsafe {
        let snapshot = restored(&*view, image_base, guarded);
        let mut adapter = reader(read);
        snapshot.matches_before(&mut adapter)
    }
}

/// Receipt::Capture: the identity, or false with the failure.
///
/// # Safety
/// The pointers are valid; `read` / `observe` follow the header's contracts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ab_capture(
    image_base: u64,
    facts: *const LayoutFacts,
    expected: *const Lifecycle,
    read: Option<ReadMemory>,
    observe: Option<ObserveLifecycle>,
    context: *mut c_void,
    identity: *mut Identity,
    failure: *mut u32,
) -> bool {
    unsafe {
        let mut receipt = Receipt::default();
        let mut read_adapter = read.map(reader);
        let mut observe_adapter =
            observe.map(|observe| move |value: &mut Lifecycle| observe(context, value));
        let captured = receipt.capture(
            image_base,
            *facts,
            *expected,
            read_adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            observe_adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(&mut Lifecycle) -> bool),
        );
        *failure = receipt.rejection() as u32;
        if captured {
            *identity = *receipt.identity();
        }
        captured
    }
}

/// Receipt::BindSaved: the allocations (read them with cvr_ab_allocations, free them with cvr_ab_free) or null
/// with the failure.
///
/// # Safety
/// The pointers are valid; the view holds its counts of records from cvr_dt_capture; `extents` holds
/// `extent_count` facts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ab_bind_saved(
    captured: bool,
    image_base: u64,
    identity: *const Identity,
    facts: *const LayoutFacts,
    header: *const NativeVector16,
    tree_ready: bool,
    tree_image_base: u64,
    tree: *const View,
    extents: *const ExtentFact,
    extent_count: usize,
    failure: *mut u32,
) -> *mut Vec<Allocation> {
    unsafe {
        if !captured || !tree_ready {
            *failure = binding::Failure::Tree as u32;
            return core::ptr::null_mut();
        }
        let mut receipt = Receipt::captured(image_base, *identity, Lifecycle::default(), *facts);
        let saved = restored(&*tree, tree_image_base, false);
        if !receipt.bind_saved(*header, &saved, slice(extents, extent_count)) {
            *failure = receipt.rejection() as u32;
            return core::ptr::null_mut();
        }
        *failure = binding::Failure::None as u32;
        Box::into_raw(Box::new(receipt.allocations().to_vec()))
    }
}

/// The bound allocations, valid until cvr_ab_free.
///
/// # Safety
/// `bound` comes from cvr_ab_bind_saved; `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ab_allocations(
    bound: *const Vec<Allocation>,
    count: *mut usize,
) -> *const Allocation {
    unsafe {
        *count = (*bound).len();
        (*bound).as_ptr()
    }
}

/// # Safety
/// `bound` comes from cvr_ab_bind_saved (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ab_free(bound: *mut Vec<Allocation>) {
    if !bound.is_null() {
        drop(unsafe { Box::from_raw(bound) });
    }
}

/// Receipt::ClassifyPointer.
///
/// # Safety
/// `bounds` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ab_classify_pointer(
    bounds: *const Bounds,
    captured: bool,
    at: u64,
) -> u32 {
    binding::classify_pointer(unsafe { &*bounds }, captured, at) as u32
}

/// Receipt::ClassifySpan.
///
/// # Safety
/// `bounds` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ab_classify_span(
    bounds: *const Bounds,
    captured: bool,
    at: u64,
    bytes: u64,
) -> u32 {
    binding::classify_span(unsafe { &*bounds }, captured, at, bytes) as u32
}

const _: () = assert!(Failure::None as u32 == 0 && Failure::Allocation as u32 == 19);

// ---- the render-elements stage definition (native::stage_definition) for include/Render/NativeStageDefinition.hpp

use crate::native::graph_snapshot::Node as GraphNode;
use crate::native::stage_definition::{self, Stage};

/// A C++ tree as a native snapshot: its records when ready, else an empty unready one.
pub(crate) unsafe fn tree_of(view: &View, ready: bool, image_base: u64, guarded: bool) -> Snapshot {
    if ready {
        unsafe { restored(view, image_base, guarded) }
    } else {
        Snapshot::default()
    }
}

/// What Snapshot::Capture yields: the failure, and on success the header and the stages (borrowed until
/// cvr_sd_free).
#[repr(C)]
pub struct StageOut {
    pub header: [u8; 64],
    pub stages: *const Stage,
    pub count: usize,
    pub failure: u32,
}
const _: () = assert!(core::mem::size_of::<StageOut>() == 88);

/// stage_definition::Snapshot::Capture: the stages (free them with cvr_sd_free) or null with the failure.
///
/// # Safety
/// `graph` and the view are valid (the view's records from cvr_dt_capture); `out` is writable; `read` follows
/// definition_tree::ReadMemory.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sd_capture(
    graph: *const GraphNode,
    tree: *const View,
    tree_ready: bool,
    tree_image_base: u64,
    tree_guarded: bool,
    target: u32,
    read: Option<ReadMemory>,
    out: *mut StageOut,
) -> *mut Vec<Stage> {
    unsafe {
        let tree = tree_of(&*tree, tree_ready, tree_image_base, tree_guarded);
        let mut adapter = read.map(reader);
        let result = stage_definition::capture(
            &*graph,
            &tree,
            target,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        );
        *out = StageOut {
            header: [0; 64],
            stages: core::ptr::null(),
            count: 0,
            failure: 0,
        };
        match result {
            Ok((header, stages)) => {
                let stages = Box::into_raw(Box::new(stages));
                (*out).header = header;
                (*out).stages = (*stages).as_ptr();
                (*out).count = (*stages).len();
                stages
            }
            Err(reason) => {
                (*out).failure = reason as u32;
                core::ptr::null_mut()
            }
        }
    }
}

/// # Safety
/// `stages` comes from cvr_sd_capture (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sd_free(stages: *mut Vec<Stage>) {
    if !stages.is_null() {
        drop(unsafe { Box::from_raw(stages) });
    }
}

/// Snapshot::Reachable (`target` < u32::MAX) or the root index of the graph node in the tree (`root_only`).
///
/// # Safety
/// `graph` and the view are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sd_query(
    graph: *const GraphNode,
    tree: *const View,
    tree_ready: bool,
    tree_image_base: u64,
    target: u32,
    root_only: bool,
) -> u32 {
    unsafe {
        let tree = tree_of(&*tree, tree_ready, tree_image_base, false);
        if root_only {
            stage_definition::root_index(&*graph, &tree)
        } else {
            stage_definition::reachable(&*graph, &tree, target) as u32
        }
    }
}
