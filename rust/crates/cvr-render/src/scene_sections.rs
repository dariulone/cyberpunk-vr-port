//! The scene section snapshot's rules (`include/Render/NativeSceneSections.hpp`): one GBuffer_Discard scene driver's
//! exact 11 ordered child handles -- the source-bound section plan, not a native clone or execution receipt. The graph
//! node's shape (handle, order, view flag, the scene driver vtable 0x3094850 / work 0x332D5C, its definition naming
//! GBuffer_Discard, a 56-byte object), the unique tree root of its object (a scene driver with the graph's vtable and
//! work and the exact destructor), its handle array (16-byte stride, 11 handles, inside the tree's handles); then the
//! driver's 56-byte header and the 11 handles read twice and equal, each handle naming its captured tree node with the
//! exact child pattern Setup / Bind / Render / Special / Unbind / End / Setup / Bind / Render / Unbind / End (vtable,
//! work, destructor, the 32 or 24 bytes its constructor allocates, the parameter word where it has one), each child's
//! vtable re-read live; and the two render-elements children's stage definitions (two stages before the split, four
//! after it, at the 2.5 stage callbacks) matching their child's raw prefix.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `Snapshot::Capture` keeps its order of reads and only orchestrates: the checks and
//! reads up to the second equal observation (`cvr_ss_capture`), each render element's stage rules (`cvr_ss_render`,
//! after the render-elements snapshot itself, already Rust's) and the final revalidation (`cvr_ss_recheck`) are Rust's;
//! `tools/single_pass_tests/native_scene_sections_test` runs its cases against them in its Rust variant.

use crate::definition_tree_abi::{reader, tree_of, ReadMemory, View};
use crate::native::definition_tree::{
    self as tree, Kind, NativeHandle, Snapshot as Tree, NO_INDEX,
};
use crate::native::graph_snapshot::{field_u32, field_u64, Node};
use crate::native::stage_definition::{Header as StageHeader, Stage};
use crate::paired_frontier::name;

pub const VTABLE_RVA: u64 = 0x3094850;
pub const WORK_RVA: u64 = 0x332D5C;
pub const CHILD_COUNT: usize = 11;
pub const PREFIX_CHILDREN: u32 = 6;
pub const TAIL_CHILDREN: u32 = 5;
pub type Header = [u8; 56];

/// scene_sections::Child.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Child {
    pub object: u64,
    pub control_block: u64,
    pub vtable: u64,
    pub work: u64,
    pub destructor: u64,
    pub slot: u32,
    pub captured_bytes: u32,
    pub raw_prefix: [u8; 32],
}
const _: () = assert!(core::mem::size_of::<Child>() == 80);

/// scene_sections::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Graph,
    Tree,
    Root,
    Array,
    Bounds,
    Handle,
    Child,
    Pattern,
    Stages,
    Changed,
    Read,
    Allocation,
}

/// detail::Profile.
struct Profile {
    vtable: u64,
    work: u64,
    destructor: u64,
    bytes: u32,
    parameter: u32,
}
// Constructors 9852F4/9844DC/1505BC0/984540/985354 explicitly allocate
// 32/32/24/32/32 bytes. No guessed fourth QWORD is read from the 24-byte child.
const SETUP: Profile = Profile {
    vtable: 0x36D1CD8,
    work: 0x33BD84,
    destructor: 0x8E7E38,
    bytes: 32,
    parameter: 7,
};
const BIND: Profile = Profile {
    vtable: 0x36D1858,
    work: 0xA00830,
    destructor: 0x8E7E38,
    bytes: 32,
    parameter: 0x200010,
};
const RENDER: Profile = Profile {
    vtable: 0x30947B8,
    work: 0x33DDA4,
    destructor: 0x8EA4E0,
    bytes: 32,
    parameter: u32::MAX,
};
const SPECIAL: Profile = Profile {
    vtable: 0x36D1708,
    work: 0x7F6A60,
    destructor: 0x8EA620,
    bytes: 24,
    parameter: u32::MAX,
};
const UNBIND: Profile = Profile {
    vtable: 0x36D1888,
    work: 0x41ADE8,
    destructor: 0x8E7E38,
    bytes: 32,
    parameter: 0x200010,
};
const END: Profile = Profile {
    vtable: 0x36D1C78,
    work: 0x33D510,
    destructor: 0x8E7E38,
    bytes: 32,
    parameter: 7,
};
const PATTERN: [&Profile; CHILD_COUNT] = [
    &SETUP, &BIND, &RENDER, &SPECIAL, &UNBIND, &END, &SETUP, &BIND, &RENDER, &UNBIND, &END,
];

fn address(at: u64, bytes: u64) -> bool {
    at != 0 && at & 7 == 0 && at <= u64::MAX - bytes
}

/// Snapshot::Observation: the driver's header, its handles and its children, as one read saw them.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub header: Header,
    pub handles: [NativeHandle; CHILD_COUNT],
    pub children: [Child; CHILD_COUNT],
}
impl Default for Observation {
    fn default() -> Self {
        Observation {
            header: [0; 56],
            handles: [NativeHandle::default(); CHILD_COUNT],
            children: [Child::default(); CHILD_COUNT],
        }
    }
}
const _: () = assert!(core::mem::size_of::<Observation>() == 1112);

fn read_u64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, at: u64) -> Option<u64> {
    let mut bytes = [0u8; 8];
    read(at, &mut bytes).then(|| u64::from_le_bytes(bytes))
}

/// Snapshot::ReadOnce: the driver's header, handles and children read once. A successful read leaves C++'s failure at
/// `Pattern` (its last stage), which a later mismatch reports.
pub fn read_once(
    graph: &Node,
    tree: &Tree,
    root: u32,
    read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
) -> Result<Observation, Failure> {
    let base = tree.image_base();
    let node = &tree.nodes()[root as usize];
    let array = &tree.arrays()[node.array as usize];
    let mut out = Observation::default();
    if !read(node.object, &mut out.header) {
        return Err(Failure::Read);
    }
    if graph.definition[..] != out.header[..32]
        || field_u64(&out.header, 0) != graph.vtable
        || field_u64(&out.header, 24) != name("GBuffer_Discard")
        || field_u32(&out.header, 32) != 1
        || field_u64(&out.header, 40) != array.data
        || field_u32(&out.header, 48) != array.capacity
        || field_u32(&out.header, 52) != array.count
    {
        return Err(Failure::Root);
    }
    let mut handles = [0u8; CHILD_COUNT * 16];
    if !read(array.data, &mut handles) {
        return Err(Failure::Read);
    }
    for (slot, handle) in out.handles.iter_mut().enumerate() {
        *handle = NativeHandle {
            object: field_u64(&handles, slot * 16),
            control_block: field_u64(&handles, slot * 16 + 8),
        };
    }
    for slot in 0..CHILD_COUNT {
        let handle = out.handles[slot];
        let captured = &tree.handles()[(node.first_handle as usize) + slot];
        if handle.object == 0
            || handle.control_block == 0
            || handle.object != captured.object
            || handle.control_block != captured.control_block
            || captured.slot as usize != slot
            || captured.parent != root
            || captured.address != array.data + slot as u64 * 16
            || captured.child as usize >= tree.nodes().len()
        {
            return Err(Failure::Handle);
        }
        let child = &tree.nodes()[captured.child as usize];
        let expected = PATTERN[slot];
        if child.object != handle.object
            || child.control_block != handle.control_block
            || child.vtable != base + expected.vtable
            || child.work != base + expected.work
            || child.destructor != base + expected.destructor
            || !address(child.object, expected.bytes as u64)
        {
            return Err(Failure::Child);
        }
        let mut row = Child {
            object: child.object,
            control_block: child.control_block,
            vtable: child.vtable,
            work: child.work,
            destructor: child.destructor,
            slot: slot as u32,
            captured_bytes: expected.bytes,
            raw_prefix: [0; 32],
        };
        if !read(row.object, &mut row.raw_prefix[..expected.bytes as usize]) {
            return Err(Failure::Read);
        }
        let Some(vtable) = read_u64(read, row.object) else {
            return Err(Failure::Read);
        };
        if !address(vtable, 16) {
            return Err(Failure::Read);
        }
        let Some(destructor) = read_u64(read, vtable) else {
            return Err(Failure::Read);
        };
        let Some(work) = read_u64(read, vtable + 8) else {
            return Err(Failure::Read);
        };
        if vtable != row.vtable
            || destructor != row.destructor
            || work != row.work
            || field_u64(&row.raw_prefix, 0) != vtable
        {
            return Err(Failure::Changed);
        }
        if expected.parameter != u32::MAX && field_u32(&row.raw_prefix, 24) != expected.parameter {
            return Err(Failure::Pattern);
        }
        out.children[slot] = row;
    }
    Ok(out)
}

/// Snapshot::Capture up to the second equal observation: the scene driver's root index and what it saw.
pub fn capture(
    graph: &Node,
    tree: &Tree,
    read: Option<&mut dyn FnMut(u64, &mut [u8]) -> bool>,
) -> Result<(u32, Observation), Failure> {
    let Some(read) = read else {
        return Err(Failure::Arguments);
    };
    let base = tree.image_base();
    if !tree.ready() || base == 0 || base > u64::MAX - 0x3BCDCA8 {
        return Err(Failure::Arguments);
    }
    let object = field_u64(&graph.bytes, 16);
    let id = graph.handle & 0xDFFF_FFFF;
    if id >= 65536
        || (graph.handle != id && graph.handle != (id | 0x2000_0000))
        || graph.order != field_u32(&graph.bytes, 0)
        || field_u32(&graph.bytes, 24) > 1
        || graph.vtable != base + VTABLE_RVA
        || graph.work != base + WORK_RVA
        || field_u64(&graph.definition, 0) != graph.vtable
        || field_u64(&graph.definition, 24) != name("GBuffer_Discard")
        || !address(object, 56)
    {
        return Err(Failure::Graph);
    }
    if !tree.matches_before(read) {
        return Err(Failure::Tree);
    }
    let mut root = NO_INDEX;
    for value in tree.roots() {
        if value.object == object && (value.node as usize) < tree.nodes().len() {
            if root != NO_INDEX {
                return Err(Failure::Tree);
            }
            root = value.node;
        }
    }
    if root == NO_INDEX {
        return Err(Failure::Root);
    }
    let node = &tree.nodes()[root as usize];
    if node.object != object
        || node.kind != Kind::SceneDriver
        || node.vtable != graph.vtable
        || node.work != graph.work
        || node.destructor != base + tree::SCENE_DRIVER_DESTRUCTOR_RVA
    {
        return Err(Failure::Root);
    }
    if node.array as usize >= tree.arrays().len() {
        return Err(Failure::Array);
    }
    let array = &tree.arrays()[node.array as usize];
    let handles = tree.handles().len() as u64;
    if array.node != root
        || array.header_address != object + 0x28
        || array.stride != 16
        || array.data == 0
        || array.count > array.capacity
        || array.capacity > tree::MAX_ARRAY_CAPACITY
        || node.handle_count != array.count
        || node.first_handle == NO_INDEX
        || node.first_handle as u64 > handles
        || node.handle_count as u64 > handles - node.first_handle as u64
    {
        return Err(Failure::Array);
    }
    if array.count as usize != CHILD_COUNT {
        return Err(Failure::Pattern);
    }
    if !address(array.data, array.count as u64 * 16) {
        return Err(Failure::Bounds);
    }
    let first = read_once(graph, tree, root, read)?;
    let second = read_once(graph, tree, root, read)?;
    if first != second {
        return Err(Failure::Changed);
    }
    Ok((root, second))
}

/// detail::Stages and the render element's header against its child's raw prefix: a ready render-elements snapshot
/// with two stages before the split (`late` false) or four after it, at the 2.5 stage callback.
pub fn render_matches(
    header: &StageHeader,
    stages: &[Stage],
    ready: bool,
    base: u64,
    late: bool,
    prefix: &[u8; 32],
) -> bool {
    if !ready || stages.len() != if late { 4 } else { 2 } {
        return false;
    }
    // 2.5: the stage callbacks are sub_1422749E0 / sub_142274AE0 (2.31 0x1134C40 / 0x1D41BE0); the stage object
    // they are allocated with grew from 0x40 to 0x48 bytes, so the header checks below are 2.31's until re-audited.
    if field_u64(header, 0x18) != base + if late { 0x22749E0 } else { 0x2274AE0 }
        || field_u64(header, 0x20) != 0
        || field_u64(header, 0x28) != 0
    {
        return false;
    }
    for (i, entry) in stages.iter().enumerate() {
        let odd = i % 2 == 1;
        let stage = if late {
            if i < 2 {
                9
            } else {
                8
            }
        } else {
            7
        };
        if field_u64(entry, 0) != 0
            || field_u32(entry, 8) != if odd { 17 } else { 33 }
            || entry[0xC] != odd as u8
            || field_u32(entry, 0x10) != stage
            || entry[0x14] != if odd { 0 } else { 2 }
            || field_u32(entry, 0x18) != 0
            || field_u32(entry, 0x1C) != 0
            || entry[0x20] != 0
            || field_u32(entry, 0x24) != u32::MAX
            || field_u32(entry, 0x28) != 0
            || field_u32(entry, 0x2C) != 0
        {
            return false;
        }
    }
    prefix[..] == header[..32]
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// What `cvr_ss_capture` saw: the root index and the second observation (on success), else the failure.
#[repr(C)]
pub struct RawCapture {
    pub observation: Observation,
    pub root: u32,
    pub failure: u32,
}
const _: () = assert!(core::mem::size_of::<RawCapture>() == 1120);

/// Snapshot::Capture up to the second equal observation.
///
/// # Safety
/// `graph` and the view are valid (the view's records from cvr_dt_capture); `out` is writable; `read` follows
/// definition_tree::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ss_capture(
    graph: *const Node,
    tree: *const View,
    tree_ready: bool,
    tree_image_base: u64,
    tree_guarded: bool,
    read: Option<ReadMemory>,
    out: *mut RawCapture,
) -> bool {
    unsafe {
        let tree = tree_of(&*tree, tree_ready, tree_image_base, tree_guarded);
        let mut adapter = read.map(reader);
        let result = capture(
            &*graph,
            &tree,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        );
        let (root, observation, failure) = match result {
            Ok((root, observation)) => (root, observation, Failure::None),
            Err(failure) => (NO_INDEX, Observation::default(), failure),
        };
        *out = RawCapture {
            observation,
            root,
            failure: failure as u32,
        };
        failure == Failure::None
    }
}
/// The final revalidation: the tree as captured and the driver read again equal to `expected`. `failure` gets what C++
/// reported: `Changed` for the tree, the read's own failure, else `Pattern` (a read's last stage).
///
/// # Safety
/// As for `cvr_ss_capture`; `root` came from it; `expected` is valid; `failure` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ss_recheck(
    graph: *const Node,
    tree: *const View,
    tree_ready: bool,
    tree_image_base: u64,
    tree_guarded: bool,
    root: u32,
    read: ReadMemory,
    expected: *const Observation,
    failure: *mut u32,
) -> bool {
    unsafe {
        let tree = tree_of(&*tree, tree_ready, tree_image_base, tree_guarded);
        let mut read = reader(read);
        *failure = Failure::Changed as u32;
        if !tree.matches_before(&mut read) {
            return false;
        }
        match read_once(&*graph, &tree, root, &mut read) {
            Err(reason) => {
                *failure = reason as u32;
                false
            }
            Ok(observation) => {
                *failure = Failure::Pattern as u32;
                observation == *expected
            }
        }
    }
}
/// detail::Stages and the render element's header against its child's raw prefix.
///
/// # Safety
/// `header`, `prefix` and `count` stages are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ss_render(
    header: *const StageHeader,
    stages: *const Stage,
    count: usize,
    ready: bool,
    base: u64,
    late: bool,
    prefix: *const [u8; 32],
) -> bool {
    unsafe {
        let stages = if count == 0 || stages.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(stages, count)
        };
        render_matches(&*header, stages, ready, base, late, &*prefix)
    }
}
