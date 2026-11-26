//! A metadata snapshot of a render graph definition's native object tree (include/Render/NativeDefinitionTree.hpp).
//!
//! From the caller's root identities: every node's vtable, destructor and work function; for the guarded known layouts
//! (scene driver, render elements, description owner -- by their exact destructors, never by vtable or view index) the
//! owned vector with its trailer allocator and free callback, the scene driver's shared handles with their control
//! blocks (positive counts, covering every captured slot) and children, the CString storage with its exact free route
//! (custom dispatch takes RCX = the allocator slot; the fallback is a direct adapter call taking &block), and the two
//! global allocators. Every range is bounded and checked for overlap; paths are a DAG of bounded depth. The whole tree is
//! read twice and compared (reference counts may move between passes). Metadata observation only: the caller guards the
//! native destructor/layout bodies before enabling known-layout reads, and equal snapshots exclude no ABA, establish no
//! ownership, and certify no destructor, allocator lifetime, graph completion or borrowed work input.
use std::collections::BTreeMap;

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

pub const MAX_NODES: u32 = 4096;
pub const MAX_HANDLES: u32 = 8192;
pub const MAX_ROOTS: u32 = 8192;
pub const MAX_DEPTH: u32 = 64;
pub const MAX_ARRAY_CAPACITY: u32 = 8192;
pub const NO_INDEX: u32 = u32::MAX;
pub const MAX_OWNED_BYTES: u64 = 16 * 1024 * 1024;
pub const SCENE_DRIVER_DESTRUCTOR_RVA: u64 = 0x8E913C;
pub const RENDER_ELEMENTS_DESTRUCTOR_RVA: u64 = 0x8EA4E0;
pub const DESCRIPTION_OWNER_DESTRUCTOR_RVA: u64 = 0x8EAB98;
pub const CSTRING_DESTRUCTOR_RVA: u64 = 0x8EA51C;
pub const OBJECT_ALLOCATOR_RVA: u64 = 0x391C2B0;
pub const CONTROL_ALLOCATOR_RVA: u64 = 0x3913850;
pub const STRING_ALLOCATOR_RVA: u64 = 0x39138C8;
pub const STRING_FREE_ADAPTER_RVA: u64 = 0x275070;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeVector16 {
    pub data: u64,
    pub capacity: u32,
    pub count: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeHandle {
    pub object: u64,
    pub control_block: u64,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Unknown = 0,
    SceneDriver = 1,
    RenderElements = 2,
    DescriptionOwner = 3,
    CString = 4,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AllocatorKind {
    #[default]
    Object,
    ControlBlock,
    CString,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Layout,
    Root,
    Node,
    Header,
    Bounds,
    Handles,
    Control,
    References,
    Pairing,
    Allocator,
    Overlap,
    NodeBudget,
    HandleBudget,
    StorageBudget,
    Depth,
    Cycle,
    Changed,
    Allocation,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root {
    pub object: u64,
    pub slot: u32,
    pub node: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub object: u64,
    pub vtable: u64,
    pub destructor: u64,
    pub work: u64,
    pub control_block: u64,
    pub kind: Kind,
    pub array: u32,
    pub first_handle: u32,
    pub handle_count: u32,
    pub max_depth: u32,
    pub reserved: u32,
}
impl Default for Node {
    fn default() -> Self {
        Node {
            object: 0,
            vtable: 0,
            destructor: 0,
            work: 0,
            control_block: 0,
            kind: Kind::Unknown,
            array: NO_INDEX,
            first_handle: NO_INDEX,
            handle_count: 0,
            max_depth: 0,
            reserved: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub address: u64,
    pub object: u64,
    pub control_block: u64,
    pub parent: u32,
    pub child: u32,
    pub slot: u32,
    pub reserved: u32,
    pub strong: i32,
    pub weak: i32,
}
impl Default for Handle {
    fn default() -> Self {
        Handle {
            address: 0,
            object: 0,
            control_block: 0,
            parent: NO_INDEX,
            child: NO_INDEX,
            slot: 0,
            reserved: 0,
            strong: 0,
            weak: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Array {
    pub header_address: u64,
    pub data: u64,
    pub trailer_address: u64,
    pub allocator: u64,
    pub free_callback: u64,
    pub capacity: u32,
    pub count: u32,
    pub stride: u32,
    pub node: u32,
}
impl Default for Array {
    fn default() -> Self {
        Array {
            header_address: 0,
            data: 0,
            trailer_address: 0,
            allocator: 0,
            free_callback: 0,
            capacity: 0,
            count: 0,
            stride: 0,
            node: NO_INDEX,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlobalAllocator {
    pub address: u64,
    pub raw: u64,
    pub allocator: u64,
    pub kind: AllocatorKind,
    pub reserved: u32,
}
/// A CString's storage. `free_callback` is populated only for an active custom dispatch, whose native RCX is
/// `allocator_slot_address`; `fallback_free_adapter` is a separate direct-call ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringStorage {
    pub storage_address: u64,
    pub data: u64,
    pub allocation_bytes: u64,
    pub allocator_slot_address: u64,
    pub allocator_value: u64,
    pub free_callback: u64,
    pub fallback_global_address: u64,
    pub fallback_raw: u64,
    pub fallback_allocator: u64,
    pub fallback_free_adapter: u64,
    pub storage: [u8; 32],
    pub node: u32,
    pub mode: u32,
    pub length: u32,
    pub capacity: u32,
}
impl Default for StringStorage {
    fn default() -> Self {
        StringStorage {
            storage_address: 0,
            data: 0,
            allocation_bytes: 0,
            allocator_slot_address: 0,
            allocator_value: 0,
            free_callback: 0,
            fallback_global_address: 0,
            fallback_raw: 0,
            fallback_allocator: 0,
            fallback_free_adapter: 0,
            storage: [0; 32],
            node: NO_INDEX,
            mode: 0,
            length: 0,
            capacity: 0,
        }
    }
}
const _: () = {
    assert!(
        core::mem::size_of::<NativeVector16>() == 16 && core::mem::size_of::<NativeHandle>() == 16
    );
    assert!(core::mem::size_of::<Root>() == 16 && core::mem::size_of::<Node>() == 64);
    assert!(core::mem::size_of::<Handle>() == 48 && core::mem::size_of::<Array>() == 56);
    assert!(
        core::mem::size_of::<GlobalAllocator>() == 32
            && core::mem::size_of::<StringStorage>() == 128
    );
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Data {
    roots: Vec<Root>,
    nodes: Vec<Node>,
    handles: Vec<Handle>,
    arrays: Vec<Array>,
    strings: Vec<StringStorage>,
    allocators: [GlobalAllocator; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RangeKind {
    Object,
    Control,
    Array,
    String,
    Vtable,
    AllocatorIdentity,
    AllocatorMethod,
    Global,
}
#[derive(Clone, Copy)]
struct Range {
    end: u64,
    kind: RangeKind,
}
#[derive(Clone, Copy)]
struct Control {
    object: u64,
    strong: i32,
    weak: i32,
    handles: u32,
}
#[derive(Default)]
struct State {
    data: Data,
    objects: BTreeMap<u64, u32>,
    controls: BTreeMap<u64, Control>,
    ranges: BTreeMap<u64, Range>,
    string_allocator: GlobalAllocator,
    owned_bytes: u64,
    child_capacity: u64,
    string_allocator_captured: bool,
}

fn structural_equal(a: &Data, b: &Data) -> bool {
    if a.roots != b.roots
        || a.nodes != b.nodes
        || a.arrays != b.arrays
        || a.strings != b.strings
        || a.allocators != b.allocators
        || a.handles.len() != b.handles.len()
    {
        return false;
    }
    // Other shared owners may adjust both counts; each complete pass independently validates positive counts and the
    // captured slots' cover.
    a.handles.iter().zip(&b.handles).all(|(x, y)| {
        Handle {
            strong: 0,
            weak: 0,
            ..*x
        } == Handle {
            strong: 0,
            weak: 0,
            ..*y
        }
    })
}
fn address(address: u64, bytes: u64) -> bool {
    address != 0 && address <= u64::MAX - bytes
}
fn pointer(a: u64, bytes: u64) -> bool {
    address(a, bytes) && a & 7 == 0
}
fn code(address: u64) -> bool {
    address != 0
}
fn read_u64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn le32(b: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(b[offset..offset + 4].try_into().expect("four bytes"))
}
fn le64(b: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(b[offset..offset + 8].try_into().expect("eight bytes"))
}

#[derive(Clone, Default)]
pub struct Snapshot {
    data: Data,
    image_base: u64,
    layout_guarded: bool,
    ready: bool,
    failure: Failure,
}
impl Snapshot {
    /// The tree under `roots` (0 slots allowed), read twice; known layouts parsed only when `native_layout_guarded`.
    pub fn capture(
        &mut self,
        roots: &[u64],
        image_base: u64,
        read: Option<Read<'_>>,
        native_layout_guarded: bool,
    ) -> bool {
        *self = Snapshot {
            failure: Failure::Arguments,
            ..Snapshot::default()
        };
        let Some(read) = read else { return false };
        if !pointer(image_base, 8)
            || image_base > u64::MAX - OBJECT_ALLOCATOR_RVA - 8
            || roots.len() > MAX_ROOTS as usize
        {
            return false;
        }
        self.image_base = image_base;
        self.layout_guarded = native_layout_guarded;
        let (mut first, mut second) = (State::default(), State::default());
        if !self.read_once(roots, read, &mut first) || !self.read_once(roots, read, &mut second) {
            return false;
        }
        self.failure = Failure::Changed;
        if !structural_equal(&first.data, &second.data) {
            return false;
        }
        self.data = second.data;
        self.ready = true;
        self.failure = Failure::None;
        true
    }
    /// The same roots captured again and structurally equal.
    pub fn matches_before(&self, read: Read<'_>) -> bool {
        if !self.ready {
            return false;
        }
        let roots: Vec<u64> = self.data.roots.iter().map(|r| r.object).collect();
        let mut current = Snapshot::default();
        current.capture(&roots, self.image_base, Some(read), self.layout_guarded)
            && structural_equal(&current.data, &self.data)
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
    pub fn image_base(&self) -> u64 {
        self.image_base
    }
    pub fn native_ownership_proved(&self) -> bool {
        false
    }
    pub fn destructor_safety_proved(&self) -> bool {
        false
    }
    pub fn roots(&self) -> &[Root] {
        &self.data.roots
    }
    pub fn nodes(&self) -> &[Node] {
        &self.data.nodes
    }
    pub fn handles(&self) -> &[Handle] {
        &self.data.handles
    }
    pub fn arrays(&self) -> &[Array] {
        &self.data.arrays
    }
    pub fn strings(&self) -> &[StringStorage] {
        &self.data.strings
    }
    pub fn allocators(&self) -> &[GlobalAllocator; 2] {
        &self.data.allocators
    }
    /// A ready snapshot of data captured before (the C++ face keeps a capture's records in its own vectors and
    /// hands them back to revalidate or bind them).
    #[allow(clippy::too_many_arguments)]
    pub fn restored(
        roots: Vec<Root>,
        nodes: Vec<Node>,
        handles: Vec<Handle>,
        arrays: Vec<Array>,
        strings: Vec<StringStorage>,
        allocators: [GlobalAllocator; 2],
        image_base: u64,
        native_layout_guarded: bool,
    ) -> Snapshot {
        Snapshot {
            data: Data { roots, nodes, handles, arrays, strings, allocators },
            image_base,
            layout_guarded: native_layout_guarded,
            ready: true,
            failure: Failure::None,
        }
    }

    fn reject(&mut self, reason: Failure) -> bool {
        self.failure = reason;
        false
    }
    fn add_range(&mut self, state: &mut State, begin: u64, bytes: u64, kind: RangeKind) -> bool {
        if !address(begin, bytes) || (kind != RangeKind::String && begin & 7 != 0) {
            return self.reject(Failure::Bounds);
        }
        let end = begin + bytes;
        if let Some((&at, next)) = state.ranges.range(begin..).next() {
            if at == begin
                && next.end == end
                && next.kind == kind
                && matches!(
                    kind,
                    RangeKind::Vtable
                        | RangeKind::AllocatorIdentity
                        | RangeKind::AllocatorMethod
                        | RangeKind::Global
                )
            {
                return true;
            }
            if at < end {
                return self.reject(Failure::Overlap);
            }
        }
        if state
            .ranges
            .range(..begin)
            .next_back()
            .is_some_and(|(_, previous)| previous.end > begin)
        {
            return self.reject(Failure::Overlap);
        }
        // Every range belongs to a bounded node, control, array or global.
        if state.ranges.len() >= MAX_NODES as usize * 5 + MAX_HANDLES as usize + 6 {
            return self.reject(Failure::StorageBudget);
        }
        state.ranges.insert(begin, Range { end, kind });
        true
    }
    fn add_node(
        &mut self,
        state: &mut State,
        object: u64,
        control: u64,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    ) -> Option<u32> {
        if !pointer(object, 8) {
            self.reject(Failure::Node);
            return None;
        }
        if let Some(&index) = state.objects.get(&object) {
            if state.data.nodes[index as usize].control_block != control {
                self.reject(Failure::Pairing);
                return None;
            }
            return Some(index);
        }
        if state.data.nodes.len() >= MAX_NODES as usize {
            self.reject(Failure::NodeBudget);
            return None;
        }
        let mut node = Node {
            object,
            control_block: control,
            ..Node::default()
        };
        self.failure = Failure::Node;
        node.vtable = read_u64(read, object)?;
        if !pointer(node.vtable, 16) {
            return None;
        }
        node.destructor = read_u64(read, node.vtable)?;
        node.work = read_u64(read, node.vtable + 8)?;
        if !code(node.destructor) || !code(node.work) {
            return None;
        }
        let base = self.image_base;
        node.kind = match node.destructor {
            d if d == base + SCENE_DRIVER_DESTRUCTOR_RVA => Kind::SceneDriver,
            d if d == base + RENDER_ELEMENTS_DESTRUCTOR_RVA => Kind::RenderElements,
            d if d == base + DESCRIPTION_OWNER_DESTRUCTOR_RVA => Kind::DescriptionOwner,
            d if d == base + CSTRING_DESTRUCTOR_RVA => Kind::CString,
            _ => Kind::Unknown,
        };
        if node.kind != Kind::Unknown && !self.layout_guarded {
            self.reject(Failure::Layout);
            return None;
        }
        let known_bytes = match node.kind {
            Kind::SceneDriver => 0x38,
            Kind::RenderElements | Kind::CString => 0x40,
            Kind::DescriptionOwner => 0x30,
            Kind::Unknown => 8,
        };
        if !self.add_range(state, object, known_bytes, RangeKind::Object)
            || !self.add_range(state, node.vtable, 16, RangeKind::Vtable)
        {
            return None;
        }
        let index = state.data.nodes.len() as u32;
        state.objects.insert(object, index);
        state.data.nodes.push(node);
        Some(index)
    }
    fn read_control(
        &mut self,
        state: &mut State,
        handle: &mut Handle,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    ) -> bool {
        if handle.control_block == 0 {
            // 1444C4 skips an empty control; a live object with no control is no evidence of the shared ownership this
            // capture audits.
            return handle.object == 0 || self.reject(Failure::Pairing);
        }
        if !pointer(handle.control_block, 8) {
            return self.reject(Failure::Control);
        }
        if !state.controls.contains_key(&handle.control_block) {
            if !self.add_range(state, handle.control_block, 8, RangeKind::Control) {
                return false;
            }
            let mut counts = [0u8; 8];
            self.failure = Failure::Control;
            if !read(handle.control_block, &mut counts) {
                return false;
            }
            let control = Control {
                object: handle.object,
                strong: i32::from_le_bytes(counts[0..4].try_into().expect("four bytes")),
                weak: i32::from_le_bytes(counts[4..8].try_into().expect("four bytes")),
                handles: 0,
            };
            if control.strong <= 0 || control.weak <= 0 {
                return self.reject(Failure::References);
            }
            state.controls.insert(handle.control_block, control);
        }
        let control = state
            .controls
            .get_mut(&handle.control_block)
            .expect("the control");
        if control.object != handle.object {
            return self.reject(Failure::Pairing);
        }
        control.handles += 1;
        let (handles, strong, weak) = (control.handles, control.strong, control.weak);
        if handles > strong as u32 {
            return self.reject(Failure::References);
        }
        handle.strong = strong;
        handle.weak = weak;
        true
    }
    fn read_array(
        &mut self,
        state: &mut State,
        node_index: u32,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    ) -> bool {
        let (kind, object) = {
            let node = &state.data.nodes[node_index as usize];
            (node.kind, node.object)
        };
        let offset = match kind {
            Kind::SceneDriver => 0x28,
            Kind::DescriptionOwner => 0x20,
            _ => 0x30,
        };
        let mut array = Array {
            node: node_index,
            stride: if kind == Kind::SceneDriver { 16 } else { 0x30 },
            header_address: object + offset,
            ..Array::default()
        };
        let mut header = [0u8; 16];
        self.failure = Failure::Header;
        if !read(array.header_address, &mut header) {
            return false;
        }
        array.data = le64(&header, 0);
        array.capacity = le32(&header, 8);
        array.count = le32(&header, 12);
        if !pointer(array.data, 8)
            || array.capacity > MAX_ARRAY_CAPACITY
            || array.count > array.capacity
        {
            return self.reject(Failure::Bounds);
        }
        if kind == Kind::SceneDriver {
            if array.count as u64 > MAX_HANDLES as u64 - state.data.handles.len() as u64 {
                return self.reject(Failure::HandleBudget);
            }
            if array.capacity as u64 > MAX_HANDLES as u64 - state.child_capacity {
                return self.reject(Failure::StorageBudget);
            }
            state.child_capacity += array.capacity as u64;
        }
        if array.capacity != 0 {
            let bytes = (array.capacity as u64 * array.stride as u64 + 7) & !7;
            if bytes > u32::MAX as u64 || bytes + 8 > MAX_OWNED_BYTES - state.owned_bytes {
                return self.reject(Failure::StorageBudget);
            }
            if !self.add_range(state, array.data, bytes + 8, RangeKind::Array) {
                return false;
            }
            state.owned_bytes += bytes + 8;
            array.trailer_address = array.data + bytes;
            self.failure = Failure::Allocator;
            let Some(allocator) = read_u64(read, array.trailer_address) else {
                return false;
            };
            array.allocator = allocator;
        } else {
            array.allocator = array.data;
        }
        if !pointer(array.allocator, 0x28) {
            return self.reject(Failure::Allocator);
        }
        if !self.add_range(state, array.allocator, 8, RangeKind::AllocatorIdentity)
            || !self.add_range(state, array.allocator + 0x20, 8, RangeKind::AllocatorMethod)
        {
            return false;
        }
        self.failure = Failure::Allocator;
        let Some(free_callback) = read_u64(read, array.allocator + 0x20) else {
            return false;
        };
        if !code(free_callback) {
            return false;
        }
        array.free_callback = free_callback;
        state.data.nodes[node_index as usize].array = state.data.arrays.len() as u32;
        state.data.arrays.push(array);
        if kind != Kind::SceneDriver {
            return true;
        }
        {
            let first_handle = state.data.handles.len() as u32;
            let parent = &mut state.data.nodes[node_index as usize];
            parent.first_handle = first_handle;
            parent.handle_count = array.count;
        }
        let mut raw = vec![0u8; array.count as usize * 16];
        self.failure = Failure::Handles;
        if array.count != 0 && !read(array.data, &mut raw) {
            return false;
        }
        for slot in 0..array.count {
            let entry = &raw[slot as usize * 16..slot as usize * 16 + 16];
            let mut handle = Handle {
                address: array.data + slot as u64 * 16,
                object: le64(entry, 0),
                control_block: le64(entry, 8),
                parent: node_index,
                slot,
                ..Handle::default()
            };
            if !self.read_control(state, &mut handle, read) {
                return false;
            }
            if handle.object != 0 {
                let Some(child) = self.add_node(state, handle.object, handle.control_block, read)
                else {
                    return false;
                };
                handle.child = child;
            }
            state.data.handles.push(handle);
        }
        true
    }
    fn read_string_allocator(
        &mut self,
        state: &mut State,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    ) -> bool {
        if state.string_allocator_captured {
            return true;
        }
        let address = self.image_base + STRING_ALLOCATOR_RVA;
        state.string_allocator = GlobalAllocator {
            address,
            kind: AllocatorKind::CString,
            ..GlobalAllocator::default()
        };
        if !self.add_range(state, address, 8, RangeKind::Global) {
            return false;
        }
        self.failure = Failure::Allocator;
        let Some(raw) = read_u64(read, address) else {
            return false;
        };
        state.string_allocator.raw = raw;
        state.string_allocator.allocator = raw & !7;
        if !pointer(state.string_allocator.allocator, 8) {
            return self.reject(Failure::Allocator);
        }
        if !self.add_range(
            state,
            state.string_allocator.allocator,
            8,
            RangeKind::AllocatorIdentity,
        ) {
            return false;
        }
        state.string_allocator_captured = true;
        true
    }
    fn read_string(
        &mut self,
        state: &mut State,
        node_index: u32,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    ) -> bool {
        if state.data.strings.len() >= MAX_NODES as usize {
            return self.reject(Failure::NodeBudget);
        }
        let storage_address = state.data.nodes[node_index as usize].object + 0x20;
        let mut string = StringStorage {
            node: node_index,
            storage_address,
            allocator_slot_address: storage_address + 0x18,
            ..StringStorage::default()
        };
        self.failure = Failure::Header;
        if !read(string.storage_address, &mut string.storage) {
            return false;
        }
        let flags = le32(&string.storage, 0x14);
        string.mode = flags >> 30;
        string.length = flags & 0x3FFF_FFFF;
        string.allocator_value = le64(&string.storage, 0x18);
        if string.mode == 3 {
            return self.reject(Failure::Bounds);
        }
        if string.mode == 0 {
            string.data = string.storage_address;
            string.capacity = 19;
            if string.length > string.capacity {
                return self.reject(Failure::Bounds);
            }
        } else {
            string.data = le64(&string.storage, 0);
            string.capacity = le32(&string.storage, 0x10);
            if string.mode == 1 {
                if string.length > string.capacity || string.capacity > u32::MAX - 8 {
                    return self.reject(Failure::Bounds);
                }
                if string.capacity != 0 {
                    string.allocation_bytes = (string.capacity as u64 + 8) & !7;
                    if string.allocation_bytes > MAX_OWNED_BYTES - state.owned_bytes {
                        return self.reject(Failure::StorageBudget);
                    }
                    if !self.add_range(
                        state,
                        string.data,
                        string.allocation_bytes,
                        RangeKind::String,
                    ) {
                        return false;
                    }
                    state.owned_bytes += string.allocation_bytes;
                    if string.allocator_value != 0 {
                        if !pointer(string.allocator_value, 0x28) {
                            return self.reject(Failure::Allocator);
                        }
                        if !self.add_range(
                            state,
                            string.allocator_value,
                            8,
                            RangeKind::AllocatorIdentity,
                        ) || !self.add_range(
                            state,
                            string.allocator_value + 0x20,
                            8,
                            RangeKind::AllocatorMethod,
                        ) {
                            return false;
                        }
                        self.failure = Failure::Allocator;
                        let Some(callback) = read_u64(read, string.allocator_value + 0x20) else {
                            return false;
                        };
                        if !code(callback) {
                            return false;
                        }
                        string.free_callback = callback;
                    } else {
                        if !self.read_string_allocator(state, read) {
                            return false;
                        }
                        string.fallback_global_address = state.string_allocator.address;
                        string.fallback_raw = state.string_allocator.raw;
                        string.fallback_allocator = state.string_allocator.allocator;
                        // Direct 1444B0 takes &block in RCX; custom dispatch instead calls free_callback with
                        // RCX = &S.allocator.
                        string.fallback_free_adapter = self.image_base + STRING_FREE_ADAPTER_RVA;
                    }
                }
            }
            // A borrowed mode 2 does not dereference its data or allocator and implies neither an allocation range
            // nor an eventual free route.
        }
        state.data.strings.push(string);
        true
    }
    fn validate_paths(&mut self, state: &mut State) -> bool {
        let mut incoming = vec![0u32; state.data.nodes.len()];
        for handle in &state.data.handles {
            if handle.child != NO_INDEX {
                incoming[handle.child as usize] += 1;
            }
        }
        let mut queue: Vec<u32> = (0..incoming.len() as u32)
            .filter(|&i| incoming[i as usize] == 0)
            .collect();
        let mut cursor = 0;
        while cursor < queue.len() {
            let parent = state.data.nodes[queue[cursor] as usize];
            cursor += 1;
            for slot in 0..parent.handle_count {
                let child = state.data.handles[(parent.first_handle + slot) as usize].child;
                if child == NO_INDEX {
                    continue;
                }
                let node = &mut state.data.nodes[child as usize];
                node.max_depth = node.max_depth.max(parent.max_depth + 1);
                if node.max_depth > MAX_DEPTH {
                    return self.reject(Failure::Depth);
                }
                incoming[child as usize] -= 1;
                if incoming[child as usize] == 0 {
                    queue.push(child);
                }
            }
        }
        queue.len() == state.data.nodes.len() || self.reject(Failure::Cycle)
    }
    fn read_once(
        &mut self,
        roots: &[u64],
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        state: &mut State,
    ) -> bool {
        state.data.roots.reserve(roots.len());
        for (i, rva) in [OBJECT_ALLOCATOR_RVA, CONTROL_ALLOCATOR_RVA]
            .into_iter()
            .enumerate()
        {
            let address = self.image_base + rva;
            let kind = if i != 0 {
                AllocatorKind::ControlBlock
            } else {
                AllocatorKind::Object
            };
            state.data.allocators[i] = GlobalAllocator {
                address,
                kind,
                ..GlobalAllocator::default()
            };
            if !self.add_range(state, address, 8, RangeKind::Global) {
                return false;
            }
            self.failure = Failure::Allocator;
            let Some(raw) = read_u64(read, address) else {
                return false;
            };
            let allocator = raw & !7;
            state.data.allocators[i].raw = raw;
            state.data.allocators[i].allocator = allocator;
            if !pointer(allocator, 8) {
                return self.reject(Failure::Allocator);
            }
            if !self.add_range(state, allocator, 8, RangeKind::AllocatorIdentity) {
                return false;
            }
        }
        for (slot, &object) in roots.iter().enumerate() {
            let mut root = Root {
                object,
                slot: slot as u32,
                node: NO_INDEX,
            };
            if object != 0 {
                if state.objects.contains_key(&object) {
                    return self.reject(Failure::Root);
                }
                let Some(node) = self.add_node(state, object, 0, read) else {
                    return false;
                };
                root.node = node;
            }
            state.data.roots.push(root);
        }
        let mut i = 0u32;
        while (i as usize) < state.data.nodes.len() {
            let kind = state.data.nodes[i as usize].kind;
            let read_ok = match kind {
                Kind::CString => self.read_string(state, i, read),
                Kind::Unknown => true,
                _ => self.read_array(state, i, read),
            };
            if !read_ok {
                return false;
            }
            i += 1;
        }
        self.validate_paths(state)
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/definition_tree_test.cpp's cases.
    use super::*;
    use std::cell::RefCell;

    const BASE: u64 = 0x1_0000_0000;
    const ROOT0: u64 = 0x10000;
    const ROOT1: u64 = 0x10100;
    const CHILD: u64 = 0x20000;
    const DRIVER_VTABLE: u64 = BASE + 0x2B3A008;
    const ELEMENTS_VTABLE: u64 = BASE + 0x2B39F70;
    const UNKNOWN_VTABLE: u64 = BASE + 0x2B3A100;
    const DESCRIPTION_VTABLE: u64 = BASE + 0x2B39DF0;
    const CSTRING_VTABLE: u64 = BASE + 0x2B3A038;
    const OBJECT_ALLOCATOR: u64 = 0x800000;
    const CONTROL_ALLOCATOR: u64 = 0x810000;
    const ARRAY_ALLOCATOR: u64 = 0x820000;
    const STRING_ALLOCATOR: u64 = 0x830000;
    const HANDLES0: u64 = 0x30000;
    const HANDLES1: u64 = 0x31000;
    const DESCRIPTIONS: u64 = 0x32000;
    const CONTROL0: u64 = 0x40000;

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        read_calls: u32,
        trigger_calls: u32,
        trigger_at: u32,
        trigger_address: u64,
        mutation_address: u64,
        mutation_value: u64,
        mutation_bytes: usize,
        fail_address: u64,
        fail_at: u32,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        W.with(|w| f(&mut w.borrow_mut()))
    }
    fn put(address: u64, b: &[u8]) {
        with(|w| {
            for (i, v) in b.iter().enumerate() {
                w.memory.insert(address + i as u64, *v);
            }
        });
    }
    fn put64(address: u64, v: u64) {
        put(address, &v.to_le_bytes());
    }
    fn put32(address: u64, v: u32) {
        put(address, &v.to_le_bytes());
    }
    fn erase(address: u64) {
        with(|w| {
            w.memory.remove(&address);
        });
    }
    fn memory() -> BTreeMap<u64, u8> {
        with(|w| w.memory.clone())
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        with(|w| {
            w.read_calls += 1;
            if w.read_calls == w.fail_at
                || address == w.fail_address
                || address > u64::MAX - out.len() as u64
            {
                return false;
            }
            for (i, o) in out.iter_mut().enumerate() {
                match w.memory.get(&(address + i as u64)) {
                    Some(v) => *o = *v,
                    None => return false,
                }
            }
            if address == w.trigger_address {
                w.trigger_calls += 1;
                if w.trigger_calls == w.trigger_at {
                    let value = w.mutation_value.to_le_bytes();
                    for (i, v) in value.iter().take(w.mutation_bytes).enumerate() {
                        let a = w.mutation_address + i as u64;
                        w.memory.insert(a, *v);
                    }
                }
            }
            true
        })
    }
    fn controls() {
        with(|w| {
            w.read_calls = 0;
            w.trigger_calls = 0;
            w.trigger_at = 0;
            w.fail_at = 0;
            w.trigger_address = 0;
            w.mutation_address = 0;
            w.mutation_value = 0;
            w.fail_address = 0;
            w.mutation_bytes = 0;
        });
    }
    fn methods(vtable: u64, destructor: u64, work: u64) {
        put64(vtable, destructor);
        put64(vtable + 8, work);
    }
    fn object(object: u64, vtable: u64) {
        put64(object, vtable);
    }
    fn references(control: u64, strong: i32, weak: i32) {
        put(control, &strong.to_le_bytes());
        put(control + 4, &weak.to_le_bytes());
    }
    fn handle(address: u64, object: u64, control: u64) {
        put64(address, object);
        put64(address + 8, control);
    }
    fn header(address: u64, data: u64, capacity: u32, count: u32) {
        put64(address, data);
        put32(address + 8, capacity);
        put32(address + 12, count);
    }
    fn vector(object: u64, offset: u64, data: u64, capacity: u32, count: u32, stride: u32) {
        header(
            object + offset,
            if capacity != 0 { data } else { ARRAY_ALLOCATOR },
            capacity,
            count,
        );
        if capacity != 0 {
            put64(data + capacity as u64 * stride as u64, ARRAY_ALLOCATOR);
        }
    }
    fn cstring_object(
        object_address: u64,
        mode: u32,
        length: u32,
        capacity: u32,
        data: u64,
        allocator: u64,
    ) {
        object(object_address, CSTRING_VTABLE);
        let mut storage = [0u8; 32];
        let flags = (mode << 30) | length;
        storage[0..8].copy_from_slice(&data.to_le_bytes());
        storage[0x10..0x14].copy_from_slice(&capacity.to_le_bytes());
        storage[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
        storage[0x18..0x20].copy_from_slice(&allocator.to_le_bytes());
        put(object_address + 0x20, &storage);
    }
    fn fresh() {
        with(|w| w.memory.clear());
        controls();
        put64(BASE + OBJECT_ALLOCATOR_RVA, OBJECT_ALLOCATOR | 5);
        put64(BASE + CONTROL_ALLOCATOR_RVA, CONTROL_ALLOCATOR | 3);
        put64(ARRAY_ALLOCATOR + 0x20, BASE + 0x985ABC);
        methods(
            DRIVER_VTABLE,
            BASE + SCENE_DRIVER_DESTRUCTOR_RVA,
            BASE + 0x1EC1D0,
        );
        methods(
            ELEMENTS_VTABLE,
            BASE + RENDER_ELEMENTS_DESTRUCTOR_RVA,
            BASE + 0x23A938,
        );
        methods(UNKNOWN_VTABLE, BASE + 0xABCDEF, BASE + 0x123457);
        methods(
            DESCRIPTION_VTABLE,
            BASE + DESCRIPTION_OWNER_DESTRUCTOR_RVA,
            BASE + 0xC26DB8,
        );
        methods(
            CSTRING_VTABLE,
            BASE + CSTRING_DESTRUCTOR_RVA,
            BASE + 0x58DA9C,
        );
    }
    fn dag() {
        fresh();
        object(ROOT0, DRIVER_VTABLE);
        object(ROOT1, DRIVER_VTABLE);
        object(CHILD, ELEMENTS_VTABLE);
        vector(ROOT0, 0x28, HANDLES0, 2, 2, 16);
        vector(ROOT1, 0x28, HANDLES1, 1, 1, 16);
        vector(CHILD, 0x30, DESCRIPTIONS, 2, 1, 0x30);
        handle(HANDLES0, CHILD, CONTROL0);
        handle(HANDLES0 + 16, 0, 0);
        handle(HANDLES1, CHILD, CONTROL0);
        references(CONTROL0, 2, 1);
    }
    fn try_capture(snapshot: &mut Snapshot, roots: &[u64], guarded: bool) -> bool {
        snapshot.capture(roots, BASE, Some(&mut read), guarded)
    }
    fn capture(roots: &[u64]) -> Snapshot {
        let mut snapshot = Snapshot::default();
        assert!(
            try_capture(&mut snapshot, roots, true),
            "stable tree capture failed: {:?}",
            snapshot.rejection()
        );
        snapshot
    }
    fn matches(snapshot: &Snapshot) -> bool {
        snapshot.matches_before(&mut read)
    }
    fn rejected(roots: &[u64], reason: Failure) {
        let mut snapshot = Snapshot::default();
        let accepted = try_capture(&mut snapshot, roots, true);
        assert!(
            !accepted && !snapshot.ready() && snapshot.rejection() == reason,
            "expected {reason:?}, got {:?}",
            snapshot.rejection()
        );
        assert!(
            snapshot.nodes().is_empty() && snapshot.handles().is_empty() && !matches(&snapshot)
        );
        assert!(snapshot.arrays().is_empty() && snapshot.strings().is_empty());
    }
    /// The third allocator callback read is the last native read of dag's first pass.
    fn arm(target: u64, value: u64, bytes: usize) {
        arm_at(ARRAY_ALLOCATOR + 0x20, 3, target, value, bytes);
    }
    fn arm_at(trigger: u64, at: u32, target: u64, value: u64, bytes: usize) {
        with(|w| {
            w.trigger_address = trigger;
            w.trigger_at = at;
            w.mutation_address = target;
            w.mutation_value = value;
            w.mutation_bytes = bytes;
        });
    }
    fn read_calls() -> u32 {
        with(|w| w.read_calls)
    }

    #[test]
    fn positive() {
        dag();
        let roots = [ROOT0, 0, ROOT1];
        let before = memory();
        let snapshot = capture(&roots);
        assert!(memory() == before && snapshot.ready() && snapshot.rejection() == Failure::None);
        assert!(
            snapshot.nodes().len() == 3
                && snapshot.handles().len() == 3
                && snapshot.arrays().len() == 3
        );
        assert!(
            snapshot.roots().len() == 3
                && snapshot.roots()[1].object == 0
                && snapshot.roots()[1].node == NO_INDEX
        );
        let child = snapshot.nodes()[2];
        assert!(child.object == CHILD && child.kind == Kind::RenderElements);
        assert!(
            child.destructor == BASE + RENDER_ELEMENTS_DESTRUCTOR_RVA
                && child.work == BASE + 0x23A938
        );
        assert!(child.control_block == CONTROL0 && child.max_depth == 1);
        let handles = snapshot.handles();
        assert!(handles[0].child == handles[2].child && handles[1].child == NO_INDEX);
        assert!(handles[0].strong == 2 && handles[0].weak == 1);
        let arrays = snapshot.arrays();
        assert!(arrays[0].header_address == ROOT0 + 0x28 && arrays[0].data == HANDLES0);
        assert!(
            arrays[0].trailer_address == HANDLES0 + 32 && arrays[0].allocator == ARRAY_ALLOCATOR
        );
        assert!(arrays[0].free_callback == BASE + 0x985ABC && arrays[2].stride == 0x30);
        let allocators = snapshot.allocators();
        assert!(
            allocators[0].address == BASE + OBJECT_ALLOCATOR_RVA
                && allocators[0].raw == OBJECT_ALLOCATOR | 5
        );
        assert!(
            allocators[0].allocator == OBJECT_ALLOCATOR
                && allocators[1].allocator == CONTROL_ALLOCATOR
        );
        assert!(matches(&snapshot) && memory() == before);
        assert!(!snapshot.native_ownership_proved() && !snapshot.destructor_safety_proved());

        dag();
        handle(HANDLES0 + 16, 0, CONTROL0 + 16);
        references(CONTROL0 + 16, 1, 2);
        let controlled_null = capture(&roots);
        let h = controlled_null.handles()[1];
        assert!(
            h.object == 0
                && h.control_block == CONTROL0 + 16
                && h.child == NO_INDEX
                && h.strong == 1
        );

        fresh();
        object(ROOT0, UNKNOWN_VTABLE);
        let unknown_root = [ROOT0];
        let mut unknown = Snapshot::default();
        assert!(
            try_capture(&mut unknown, &unknown_root, false)
                && unknown.nodes()[0].kind == Kind::Unknown
        );
        assert!(
            unknown.nodes()[0].destructor == BASE + 0xABCDEF
                && unknown.nodes()[0].work == BASE + 0x123457
        );
        assert!(unknown.arrays().is_empty());
        dag();
        object(CHILD, UNKNOWN_VTABLE);
        let unknown_child = capture(&roots);
        assert!(
            unknown_child.nodes()[2].kind == Kind::Unknown
                && unknown_child.nodes()[2].control_block == CONTROL0
        );
        assert_eq!(unknown_child.arrays().len(), 2);
        fresh();
        methods(
            UNKNOWN_VTABLE,
            BASE + SCENE_DRIVER_DESTRUCTOR_RVA,
            BASE + 0x123457,
        );
        object(ROOT0, UNKNOWN_VTABLE);
        object(CHILD, ELEMENTS_VTABLE);
        references(CONTROL0, 1, 1);
        vector(ROOT0, 0x28, HANDLES0, 1, 1, 16);
        handle(HANDLES0, CHILD, CONTROL0);
        vector(CHILD, 0x30, 0, 0, 0, 0x30);
        let by_destructor = capture(&unknown_root);
        assert!(
            by_destructor.nodes()[0].kind == Kind::SceneDriver && by_destructor.nodes().len() == 2
        );
        assert_eq!(by_destructor.nodes()[1].kind, Kind::RenderElements);
        dag();
        vector(ROOT0, 0x28, 0, 0, 0, 16);
        vector(ROOT1, 0x28, 0, 0, 0, 16);
        let empty = capture(&roots);
        assert!(empty.handles().is_empty() && empty.arrays()[0].trailer_address == 0);
        assert_eq!(empty.arrays()[0].allocator, ARRAY_ALLOCATOR);
        fresh();
        assert!(capture(&[0, 0]).nodes().is_empty());
    }
    #[test]
    fn counts_and_changes() {
        let roots = [ROOT0, ROOT1];
        dag();
        arm(CONTROL0, 4, 4);
        let advancing = capture(&roots);
        assert!(advancing.handles()[0].strong == 4 && advancing.handles()[2].strong == 4);
        controls();
        references(CONTROL0, 3, 5);
        assert!(
            matches(&advancing),
            "harmless later reference changes rejected"
        );
        dag();
        arm(CONTROL0 + 4, 4, 4);
        assert_eq!(capture(&roots).handles()[0].weak, 4);
        dag();
        arm(CONTROL0, 0, 4);
        rejected(&roots, Failure::References);
        dag();
        arm(CONTROL0 + 4, 0, 4);
        rejected(&roots, Failure::References);
        dag();
        arm(CONTROL0, 1, 4);
        rejected(&roots, Failure::References);
        dag();
        references(CONTROL0, -1, 1);
        rejected(&roots, Failure::References);
        dag();
        references(CONTROL0, 2, -1);
        rejected(&roots, Failure::References);

        let changes: [(u64, u64, usize); 7] = [
            (ROOT0 + 0x34, 1, 4),
            (HANDLES0 + 16, 0, 8),
            (HANDLES0 + 32, ARRAY_ALLOCATOR + 0x100, 8),
            (ARRAY_ALLOCATOR + 0x20, BASE + 0x985AB8, 8),
            (ELEMENTS_VTABLE + 8, BASE + 0x23A939, 8),
            (BASE + OBJECT_ALLOCATOR_RVA, OBJECT_ALLOCATOR | 1, 8),
            (BASE + CONTROL_ALLOCATOR_RVA, CONTROL_ALLOCATOR + 8, 8),
        ];
        for (address, value, bytes) in changes {
            dag();
            if address == HANDLES0 + 16 {
                handle(HANDLES0 + 16, 0, CONTROL0 + 16);
                references(CONTROL0 + 16, 1, 1);
            }
            if address == HANDLES0 + 32 {
                put64(ARRAY_ALLOCATOR + 0x120, BASE + 0x985ABC);
            }
            // The null object's control changes between two valid snapshots.
            if address == HANDLES0 + 16 {
                arm(HANDLES0 + 24, 0, 8);
            } else {
                arm(address, value, bytes);
            }
            rejected(&roots, Failure::Changed);
        }
        dag();
        object(CHILD + 0x100, UNKNOWN_VTABLE);
        references(CONTROL0 + 16, 1, 1);
        handle(HANDLES0 + 16, CHILD + 0x100, CONTROL0 + 16);
        arm(HANDLES0 + 16, 0, 8);
        rejected(&roots, Failure::Changed);
        dag();
        let saved = capture(&roots);
        put64(ELEMENTS_VTABLE + 8, BASE + 0x23A939);
        assert!(
            !matches(&saved),
            "method identity drift accepted by revalidation"
        );
        dag();
        let stable = capture(&roots);
        controls();
        with(|w| w.fail_address = ROOT0);
        assert!(!matches(&stable), "a failed revalidation read accepted");
        dag();
        let mut reset = Snapshot::default();
        assert!(try_capture(&mut reset, &roots, true));
        put(CONTROL0, &0i32.to_le_bytes());
        assert!(
            !try_capture(&mut reset, &roots, true) && !reset.ready() && reset.nodes().is_empty()
        );
    }
    #[test]
    fn invalid_structure() {
        let roots = [ROOT0, ROOT1];
        let mut snapshot = Snapshot::default();
        dag();
        assert!(
            !try_capture(&mut snapshot, &roots, false) && snapshot.rejection() == Failure::Layout
        );
        assert!(
            !snapshot.capture(&roots, 0, Some(&mut read), true)
                && snapshot.rejection() == Failure::Arguments
        );
        assert!(!snapshot.capture(&roots, u64::MAX - 7, Some(&mut read), true));
        assert_eq!(snapshot.rejection(), Failure::Arguments);
        assert!(
            !snapshot.capture(&roots, BASE, None, true)
                && snapshot.rejection() == Failure::Arguments
        );
        let too_many = vec![0u64; MAX_ROOTS as usize + 1];
        assert!(
            !try_capture(&mut snapshot, &too_many, true)
                && snapshot.rejection() == Failure::Arguments
        );
        dag();
        rejected(&[ROOT0, ROOT0], Failure::Root);
        for (data, capacity, count) in [
            (HANDLES0, 1, 2),
            (HANDLES0, MAX_ARRAY_CAPACITY + 1, 0),
            (0, 2, 2),
            (HANDLES0 + 1, 2, 2),
            (ARRAY_ALLOCATOR, 0, 1),
            (u64::MAX - 7, 2, 0),
        ] {
            dag();
            header(ROOT0 + 0x28, data, capacity, count);
            rejected(&roots, Failure::Bounds);
        }
        let pairs: [(u64, u64, u64, Failure); 4] = [
            (HANDLES0, CHILD, 0, Failure::Pairing),
            (HANDLES0, CHILD, CONTROL0 + 1, Failure::Control),
            (HANDLES0, CHILD + 1, CONTROL0, Failure::Node),
            (HANDLES1, CHILD, CONTROL0 + 16, Failure::Pairing),
        ];
        for (at, object_address, control, reason) in pairs {
            dag();
            handle(at, object_address, control);
            if control == CONTROL0 + 16 {
                references(CONTROL0 + 16, 1, 1);
            }
            rejected(&roots, reason);
        }
        dag();
        handle(HANDLES1, CHILD + 0x100, CONTROL0);
        object(CHILD + 0x100, UNKNOWN_VTABLE);
        rejected(&roots, Failure::Pairing);
        dag();
        handle(HANDLES1, 0, CONTROL0);
        rejected(&roots, Failure::Pairing);
        dag();
        handle(HANDLES0, ROOT1, CONTROL0);
        rejected(&roots, Failure::Pairing);
        dag();
        handle(HANDLES0, CHILD, CHILD);
        rejected(&roots, Failure::Overlap);
        dag();
        header(ROOT1 + 0x28, HANDLES0, 2, 1);
        rejected(&roots, Failure::Overlap);
        dag();
        header(ROOT0 + 0x28, ROOT1, 1, 0);
        rejected(&roots, Failure::Overlap);
        dag();
        header(CHILD + 0x30, HANDLES0, 1, 0);
        rejected(&roots, Failure::Overlap);
        dag();
        put64(HANDLES0 + 32, HANDLES0);
        rejected(&roots, Failure::Overlap);
        dag();
        put64(BASE + OBJECT_ALLOCATOR_RVA, ROOT0);
        rejected(&roots, Failure::Overlap);
        dag();
        put64(HANDLES0 + 32, 0);
        rejected(&roots, Failure::Allocator);
        dag();
        put64(HANDLES0 + 32, ARRAY_ALLOCATOR + 1);
        rejected(&roots, Failure::Allocator);
        dag();
        put64(ARRAY_ALLOCATOR + 0x20, 0);
        rejected(&roots, Failure::Allocator);
        dag();
        put64(BASE + CONTROL_ALLOCATOR_RVA, 7);
        rejected(&roots, Failure::Allocator);
        for (address, reason) in [
            (ROOT0 + 0x28, Failure::Header),
            (HANDLES0, Failure::Handles),
            (CONTROL0, Failure::Control),
            (HANDLES0 + 32, Failure::Allocator),
            (ARRAY_ALLOCATOR + 0x20, Failure::Allocator),
            (ELEMENTS_VTABLE, Failure::Node),
        ] {
            dag();
            erase(address);
            rejected(&roots, reason);
        }
        dag();
        capture(&roots);
        let total = read_calls();
        for i in 1..=total {
            dag();
            with(|w| w.fail_at = i);
            assert!(
                !try_capture(&mut snapshot, &roots, true) && !snapshot.ready(),
                "a failed read accepted a partial tree"
            );
        }
    }
    fn chain(edges: u32) {
        fresh();
        for i in 0..=edges {
            let object_address = ROOT0 + i as u64 * 0x100;
            object(
                object_address,
                if i == edges {
                    UNKNOWN_VTABLE
                } else {
                    DRIVER_VTABLE
                },
            );
            if i == edges {
                continue;
            }
            let (data, control) = (0x100000 + i as u64 * 0x100, 0x200000 + i as u64 * 16);
            vector(object_address, 0x28, data, 1, 1, 16);
            handle(data, object_address + 0x100, control);
            references(control, 1, 1);
        }
    }
    #[test]
    fn paths_and_budgets() {
        let roots = [ROOT0];
        chain(MAX_DEPTH);
        assert_eq!(
            capture(&roots).nodes().last().map(|n| n.max_depth),
            Some(MAX_DEPTH)
        );
        chain(MAX_DEPTH + 1);
        rejected(&roots, Failure::Depth);
        chain(3);
        const SHORT_ROOT: u64 = 0x50000;
        object(SHORT_ROOT, DRIVER_VTABLE);
        vector(SHORT_ROOT, 0x28, 0x300000, 1, 1, 16);
        handle(0x300000, ROOT0 + 0x300, 0x200020);
        references(0x200020, 2, 1);
        let deep_shared = capture(&[ROOT0, SHORT_ROOT]);
        let shared = deep_shared
            .nodes()
            .iter()
            .find(|n| n.object == ROOT0 + 0x300);
        assert_eq!(
            shared.map(|n| n.max_depth),
            Some(3),
            "a shared DAG lost its longest path"
        );
        // A cycle wholly below a separately owned root uses consistent shared pairs.
        fresh();
        object(ROOT0, DRIVER_VTABLE);
        object(CHILD, DRIVER_VTABLE);
        object(CHILD + 0x100, DRIVER_VTABLE);
        vector(ROOT0, 0x28, HANDLES0, 1, 1, 16);
        vector(CHILD, 0x28, HANDLES1, 1, 1, 16);
        vector(CHILD + 0x100, 0x28, DESCRIPTIONS, 1, 1, 16);
        handle(HANDLES0, CHILD, CONTROL0);
        handle(HANDLES1, CHILD + 0x100, CONTROL0 + 16);
        handle(DESCRIPTIONS, CHILD, CONTROL0);
        references(CONTROL0, 2, 1);
        references(CONTROL0 + 16, 1, 1);
        rejected(&roots, Failure::Cycle);

        fresh();
        let mut many_roots = Vec::new();
        for i in 0..=MAX_NODES {
            let object_address = 0x100000 + i as u64 * 16;
            object(object_address, UNKNOWN_VTABLE);
            many_roots.push(object_address);
        }
        rejected(&many_roots, Failure::NodeBudget);
        many_roots.pop();
        assert_eq!(capture(&many_roots).nodes().len(), MAX_NODES as usize);
        fresh();
        object(ROOT0, DRIVER_VTABLE);
        vector(ROOT0, 0x28, 0x100000, MAX_HANDLES, MAX_HANDLES, 16);
        put(0x100000, &vec![0u8; MAX_HANDLES as usize * 16]);
        assert_eq!(capture(&roots).handles().len(), MAX_HANDLES as usize);
        object(ROOT1, DRIVER_VTABLE);
        vector(ROOT1, 0x28, HANDLES1, 1, 1, 16);
        handle(HANDLES1, 0, 0);
        let two_roots = [ROOT0, ROOT1];
        rejected(&two_roots, Failure::HandleBudget);
        fresh();
        object(ROOT0, DRIVER_VTABLE);
        object(ROOT1, DRIVER_VTABLE);
        vector(ROOT0, 0x28, 0x100000, MAX_HANDLES, 0, 16);
        vector(ROOT1, 0x28, HANDLES1, 1, 0, 16);
        rejected(&two_roots, Failure::StorageBudget);
        fresh();
        let mut storage_roots = Vec::new();
        for i in 0..43u64 {
            let (object_address, data) = (0x100000 + i * 0x100, 0x1000000 + i * 0x100000);
            object(object_address, ELEMENTS_VTABLE);
            vector(object_address, 0x30, data, MAX_ARRAY_CAPACITY, 0, 0x30);
            storage_roots.push(object_address);
        }
        rejected(&storage_roots, Failure::StorageBudget);
    }
    #[test]
    fn description_and_string_storage() {
        let roots = [ROOT0];
        let mut snapshot = Snapshot::default();
        fresh();
        object(ROOT0, DESCRIPTION_VTABLE);
        vector(ROOT0, 0x20, DESCRIPTIONS, 2, 1, 48);
        let description = capture(&roots);
        let a = description.arrays()[0];
        assert!(
            description.nodes()[0].kind == Kind::DescriptionOwner
                && description.arrays().len() == 1
        );
        assert!(
            a.header_address == ROOT0 + 0x20
                && a.stride == 48
                && a.trailer_address == DESCRIPTIONS + 96
        );
        assert!(a.allocator == ARRAY_ALLOCATOR && a.free_callback == BASE + 0x985ABC);
        assert!(
            !try_capture(&mut snapshot, &roots, false) && snapshot.rejection() == Failure::Layout
        );
        fresh();
        object(ROOT0, DESCRIPTION_VTABLE);
        vector(ROOT0, 0x20, 0, 0, 0, 48);
        assert_eq!(capture(&roots).arrays()[0].allocator, ARRAY_ALLOCATOR);

        fresh();
        cstring_object(ROOT0, 0, 19, u32::MAX, u64::MAX, u64::MAX);
        let before = memory();
        let inline_string = capture(&roots);
        let small = inline_string.strings()[0];
        assert!(
            inline_string.nodes()[0].kind == Kind::CString
                && inline_string.nodes()[0].array == NO_INDEX
        );
        assert!(
            inline_string.arrays().is_empty()
                && small.node == 0
                && small.storage_address == ROOT0 + 0x20
        );
        assert!(
            small.data == ROOT0 + 0x20
                && small.mode == 0
                && small.length == 19
                && small.capacity == 19
        );
        assert!(
            small.allocation_bytes == 0
                && small.free_callback == 0
                && small.fallback_global_address == 0
        );
        assert!(small.fallback_free_adapter == 0 && memory() == before);
        assert!(matches(&inline_string) && memory() == before);
        assert!(
            !try_capture(&mut snapshot, &roots, false) && snapshot.rejection() == Failure::Layout
        );
        fresh();
        cstring_object(ROOT0, 2, 123, 0, 0x12345, u64::MAX);
        let borrowed = capture(&roots).strings()[0];
        assert!(borrowed.mode == 2 && borrowed.data == 0x12345 && borrowed.length == 123);
        assert!(
            borrowed.allocation_bytes == 0
                && borrowed.free_callback == 0
                && borrowed.fallback_global_address == 0
        );
        fresh();
        cstring_object(ROOT0, 1, 0, 0, 0x12345, u64::MAX);
        let zero_owned = capture(&roots).strings()[0];
        assert!(
            zero_owned.mode == 1
                && zero_owned.allocation_bytes == 0
                && zero_owned.free_callback == 0
        );
        assert_eq!(zero_owned.fallback_global_address, 0);

        fresh();
        cstring_object(ROOT0, 1, 5, 31, DESCRIPTIONS + 1, 0);
        put64(BASE + STRING_ALLOCATOR_RVA, STRING_ALLOCATOR | 7);
        let heap = capture(&roots).strings()[0];
        assert!(
            heap.mode == 1
                && heap.length == 5
                && heap.capacity == 31
                && heap.data == DESCRIPTIONS + 1
        );
        assert!(
            heap.allocation_bytes == 32
                && heap.allocator_slot_address == ROOT0 + 0x38
                && heap.allocator_value == 0
        );
        assert!(
            heap.free_callback == 0 && heap.fallback_global_address == BASE + STRING_ALLOCATOR_RVA
        );
        assert!(
            heap.fallback_raw == STRING_ALLOCATOR | 7
                && heap.fallback_allocator == STRING_ALLOCATOR
        );
        assert_eq!(heap.fallback_free_adapter, BASE + STRING_FREE_ADAPTER_RVA);
        fresh();
        cstring_object(ROOT0, 1, 13, 15, DESCRIPTIONS + 3, ARRAY_ALLOCATOR);
        let custom = capture(&roots).strings()[0];
        assert!(
            custom.allocator_value == ARRAY_ALLOCATOR
                && custom.allocator_slot_address == ROOT0 + 0x38
        );
        assert!(custom.free_callback == BASE + 0x985ABC && custom.allocation_bytes == 16);
        assert!(custom.fallback_global_address == 0 && custom.fallback_free_adapter == 0);
        put64(ARRAY_ALLOCATOR + 0x20, BASE + 0xABCDE1);
        let unknown_callback = capture(&roots);
        assert!(unknown_callback.strings()[0].free_callback == BASE + 0xABCDE1);
        assert!(!unknown_callback.destructor_safety_proved());

        fresh();
        cstring_object(ROOT0, 1, 5, 31, DESCRIPTIONS, 0);
        cstring_object(ROOT1, 1, 6, 31, DESCRIPTIONS + 64, 0);
        put64(BASE + STRING_ALLOCATOR_RVA, STRING_ALLOCATOR | 3);
        let two_roots = [ROOT0, ROOT1];
        let repeated = capture(&two_roots);
        assert!(
            repeated.strings().len() == 2
                && repeated.strings()[0].fallback_raw == repeated.strings()[1].fallback_raw
        );
        fresh();
        object(ROOT0, DRIVER_VTABLE);
        vector(ROOT0, 0x28, HANDLES0, 1, 1, 16);
        handle(HANDLES0, CHILD, CONTROL0);
        references(CONTROL0, 1, 1);
        cstring_object(CHILD, 0, 7, u32::MAX, u64::MAX, u64::MAX);
        let child_name = capture(&roots);
        assert!(child_name.nodes()[1].kind == Kind::CString && child_name.strings()[0].node == 1);
        assert_eq!(child_name.nodes()[1].control_block, CONTROL0);
        fresh();
        object(ROOT0, DESCRIPTION_VTABLE);
        vector(ROOT0, 0x20, DESCRIPTIONS, 1, 1, 48);
        cstring_object(ROOT1, 0, 0, u32::MAX, 0, u64::MAX);
        let mixed = capture(&two_roots);
        assert!(
            mixed.arrays().len() == 1 && mixed.strings().len() == 1 && mixed.strings()[0].node == 1
        );
    }
    #[test]
    fn invalid_string_storage_and_changes() {
        let roots = [ROOT0];
        let cases: [(u32, u32, u32, u64, u64, Failure); 8] = [
            (3, 0, 0, 0, 0, Failure::Bounds),
            (0, 20, 0, 0, 0, Failure::Bounds),
            (1, 16, 15, DESCRIPTIONS, ARRAY_ALLOCATOR, Failure::Bounds),
            (
                1,
                0,
                u32::MAX,
                DESCRIPTIONS,
                ARRAY_ALLOCATOR,
                Failure::Bounds,
            ),
            (
                1,
                0,
                MAX_OWNED_BYTES as u32,
                DESCRIPTIONS,
                ARRAY_ALLOCATOR,
                Failure::StorageBudget,
            ),
            (1, 1, 15, 0, ARRAY_ALLOCATOR, Failure::Bounds),
            (1, 1, 15, u64::MAX - 3, ARRAY_ALLOCATOR, Failure::Bounds),
            (
                1,
                1,
                15,
                DESCRIPTIONS,
                ARRAY_ALLOCATOR + 1,
                Failure::Allocator,
            ),
        ];
        for (mode, length, capacity, data, allocator, reason) in cases {
            fresh();
            cstring_object(ROOT0, mode, length, capacity, data, allocator);
            rejected(&roots, reason);
        }
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, DESCRIPTIONS);
        rejected(&roots, Failure::Overlap);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, ARRAY_ALLOCATOR);
        put64(ARRAY_ALLOCATOR + 0x20, 0);
        rejected(&roots, Failure::Allocator);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, 0);
        rejected(&roots, Failure::Allocator);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, 0);
        put64(BASE + STRING_ALLOCATOR_RVA, 7);
        rejected(&roots, Failure::Allocator);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, 0);
        put64(BASE + STRING_ALLOCATOR_RVA, DESCRIPTIONS);
        rejected(&roots, Failure::Overlap);
        fresh();
        cstring_object(ROOT0, 0, 0, 0, 0, 0);
        erase(ROOT0 + 0x3F);
        rejected(&roots, Failure::Header);
        let two_roots = [ROOT0, ROOT1];
        fresh();
        cstring_object(ROOT0, 1, 1, 15, ROOT1 + 1, ARRAY_ALLOCATOR);
        object(ROOT1, UNKNOWN_VTABLE);
        rejected(&two_roots, Failure::Overlap);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, ARRAY_ALLOCATOR);
        cstring_object(ROOT1, 1, 1, 15, DESCRIPTIONS, ARRAY_ALLOCATOR);
        rejected(&two_roots, Failure::Overlap);
        fresh();
        cstring_object(
            ROOT0,
            1,
            0,
            (MAX_OWNED_BYTES - 1) as u32,
            0x10000000,
            ARRAY_ALLOCATOR,
        );
        assert_eq!(
            capture(&roots).strings()[0].allocation_bytes,
            MAX_OWNED_BYTES
        );
        cstring_object(ROOT1, 1, 0, 1, DESCRIPTIONS, ARRAY_ALLOCATOR);
        rejected(&two_roots, Failure::StorageBudget);
        fresh();
        object(ROOT0, DESCRIPTION_VTABLE);
        vector(ROOT0, 0x20, DESCRIPTIONS, 1, 0, 48);
        cstring_object(ROOT1, 1, 1, 15, DESCRIPTIONS + 8, ARRAY_ALLOCATOR);
        rejected(&two_roots, Failure::Overlap);

        fresh();
        cstring_object(ROOT0, 0, 2, u32::MAX, 0, u64::MAX);
        arm_at(ROOT0 + 0x20, 1, ROOT0 + 0x34, 3, 4);
        rejected(&roots, Failure::Changed);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, ARRAY_ALLOCATOR);
        arm_at(ROOT0 + 0x20, 1, ROOT0 + 0x20, DESCRIPTIONS + 32, 8);
        rejected(&roots, Failure::Changed);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, ARRAY_ALLOCATOR);
        arm_at(
            ARRAY_ALLOCATOR + 0x20,
            1,
            ARRAY_ALLOCATOR + 0x20,
            BASE + 0xABCDE1,
            8,
        );
        rejected(&roots, Failure::Changed);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, 0);
        put64(BASE + STRING_ALLOCATOR_RVA, STRING_ALLOCATOR | 1);
        arm_at(
            BASE + STRING_ALLOCATOR_RVA,
            1,
            BASE + STRING_ALLOCATOR_RVA,
            STRING_ALLOCATOR + 8,
            8,
        );
        rejected(&roots, Failure::Changed);
        fresh();
        cstring_object(ROOT0, 1, 0, 0, DESCRIPTIONS, 0);
        put64(BASE + STRING_ALLOCATOR_RVA, STRING_ALLOCATOR);
        arm_at(ROOT0 + 0x20, 1, ROOT0 + 0x30, 1, 4);
        rejected(&roots, Failure::Changed);
        fresh();
        cstring_object(ROOT0, 1, 1, 15, DESCRIPTIONS, ARRAY_ALLOCATOR);
        let saved = capture(&roots);
        put32(ROOT0 + 0x34, 0x4000_0002);
        assert!(
            !matches(&saved),
            "CString structural drift accepted by revalidation"
        );
        for fallback in [false, true] {
            let setup = || {
                fresh();
                cstring_object(
                    ROOT0,
                    1,
                    1,
                    15,
                    DESCRIPTIONS,
                    if fallback { 0 } else { ARRAY_ALLOCATOR },
                );
                put64(BASE + STRING_ALLOCATOR_RVA, STRING_ALLOCATOR);
            };
            setup();
            capture(&roots);
            let total = read_calls();
            for i in 1..=total {
                setup();
                with(|w| w.fail_at = i);
                let mut snapshot = Snapshot::default();
                assert!(
                    !try_capture(&mut snapshot, &roots, true)
                        && !snapshot.ready()
                        && snapshot.strings().is_empty()
                );
            }
        }
    }
}
