//! The definition tree's allocator binding receipt (the logic of include/Render/NativeAllocatorBinding.hpp).
//!
//! A structural identity / range receipt of the engine's common allocator: the three flagged allocator globals pointing
//! at it, its live pool component's vptr and the small / middle region bounds, read twice inside one open lifecycle
//! sample of the caller's allocator gate. Bound to a saved definition tree with its exact retained owner header, it
//! lists every allocation that tree implies -- the top vector, the node objects and control blocks at their parsed
//! minimums unless an audited exact extent is supplied, the owned arrays with their trailer, the CString blocks --
//! each inside one region, without crossing it and without overlap. Version guards and lifecycle samples come from the
//! caller's gate; readable globals or a live vptr prove no birth, native liveness, ownership or native-free rights.
use std::collections::{BTreeMap, BTreeSet};

use super::definition_tree::{self as tree, Kind as NodeKind, NativeVector16, Snapshot};

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;
/// The caller's allocator gate's lifecycle sample.
pub type Observe<'a> = &'a mut dyn FnMut(&mut Lifecycle) -> bool;

pub const COMMON_RVA: u64 = 0x3BCDD80;
pub const LIVE_POOL_VTABLE_RVA: u64 = 0x3572028;
pub const POOL_OFFSET: u64 = 0xC928;
pub const SMALL_BEGIN_OFFSET: u64 = 0xC900;
pub const SMALL_END_OFFSET: u64 = 0xC908;
pub const MIDDLE_BEGIN_OFFSET: u64 = 0xCDA8;
pub const MIDDLE_END_OFFSET: u64 = 0xCDB0;
pub const VECTOR_ALLOCATOR_RVA: u64 = 0x30947E8;
pub const VECTOR_FREE_RVA: u64 = 0x120F6F0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lifecycle {
    pub incarnation: u64,
    pub reset_epoch: u64,
    pub in_flight: u32,
    pub closing: bool,
    pub coverage_complete: bool,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutFacts {
    pub globals_and_component_guarded: bool,
    pub allocation_routes_guarded: bool,
    pub exact_extent_facts_audited: bool,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Region {
    #[default]
    Unsupported,
    Small,
    Middle,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    #[default]
    TopVector,
    Object,
    Control,
    Array,
    CString,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtentKind {
    #[default]
    ParsedMinimum,
    ExactAllocation,
}
/// An audited exact extent of a node object or control block. `kind` stays a raw word: a fact of another kind is
/// refused.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ExtentFact {
    pub kind: u32,
    pub address: u64,
    pub bytes: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allocation {
    pub kind: Kind,
    pub region: Region,
    pub extent: ExtentKind,
    pub address: u64,
    pub bytes: u64,
    pub parsed_minimum: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub small_begin: u64,
    pub small_end: u64,
    pub middle_begin: u64,
    pub middle_end: u64,
}
/// The receipt's identity: the three allocator globals, the pool component's vptr and the region bounds.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub globals: [u64; 3],
    pub vptr: u64,
    pub bounds: Bounds,
}
const _: () = {
    assert!(core::mem::size_of::<Lifecycle>() == 24 && core::mem::size_of::<LayoutFacts>() == 3);
    assert!(core::mem::size_of::<ExtentFact>() == 24 && core::mem::size_of::<Allocation>() == 40);
    assert!(core::mem::offset_of!(Allocation, address) == 16);
    assert!(core::mem::size_of::<Bounds>() == 32 && core::mem::size_of::<Identity>() == 64);
};

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Layout,
    Tracking,
    Read,
    Changed,
    Globals,
    Component,
    Bounds,
    Tree,
    Header,
    FreeRoute,
    ExtentFacts,
    UnsupportedPointer,
    ExtentOverflow,
    ExtentCrossesRegion,
    Overlap,
    Allocation,
}

fn address(at: u64, bytes: u64) -> bool {
    at != 0 && at <= u64::MAX - bytes && at <= usize::MAX as u64
}
fn open(value: &Lifecycle) -> bool {
    value.incarnation != 0 && value.in_flight == 0 && !value.closing && value.coverage_complete
}
fn read_u64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, at: u64, out: &mut u64) -> bool {
    let mut b = [0u8; 8];
    let readable = read(at, &mut b);
    if readable {
        *out = u64::from_le_bytes(b);
    }
    readable
}

#[derive(Clone, Debug, Default)]
pub struct Receipt {
    image_base: u64,
    identity: Identity,
    lifecycle: Lifecycle,
    facts: LayoutFacts,
    saved_header: NativeVector16,
    allocations: Vec<Allocation>,
    captured: bool,
    saved_bound: bool,
    failure: Failure,
}

impl Receipt {
    /// The common allocator's identity, read twice inside one open lifecycle sample equal to `expected`.
    pub fn capture(
        &mut self,
        image_base: u64,
        facts: LayoutFacts,
        expected: Lifecycle,
        read: Option<Read<'_>>,
        observe: Option<Observe<'_>>,
    ) -> bool {
        *self = Receipt {
            failure: Failure::Arguments,
            ..Receipt::default()
        };
        let (Some(read), Some(observe)) = (read, observe) else {
            return false;
        };
        if !address(image_base, COMMON_RVA + MIDDLE_END_OFFSET + 8) || image_base & 7 != 0 {
            return false;
        }
        self.failure = Failure::Layout;
        if !facts.globals_and_component_guarded || !facts.allocation_routes_guarded {
            return false;
        }
        self.failure = Failure::Tracking;
        if !open(&expected) {
            return false;
        }
        let (mut before, mut after) = (Lifecycle::default(), Lifecycle::default());
        if !observe(&mut before) || before != expected || !open(&before) {
            return false;
        }
        let (mut first, mut second) = (Identity::default(), Identity::default());
        if !self.read_identity(image_base, read, &mut first)
            || !self.read_identity(image_base, read, &mut second)
        {
            return false;
        }
        self.failure = Failure::Changed;
        if first != second {
            return false;
        }
        self.failure = Failure::Tracking;
        if !observe(&mut after) || after != expected || !open(&after) {
            return false;
        }
        *self = Receipt::captured(image_base, first, expected, facts);
        true
    }
    /// A receipt captured before (the C++ face keeps the captured fields itself).
    pub fn captured(
        image_base: u64,
        identity: Identity,
        lifecycle: Lifecycle,
        facts: LayoutFacts,
    ) -> Receipt {
        Receipt {
            image_base,
            identity,
            lifecycle,
            facts,
            captured: true,
            ..Receipt::default()
        }
    }

    /// Binds the exact retained owner header whose validated entry slots produced `tree` (a matching count is not
    /// an entry / ownership proof) and lists the allocations it implies.
    pub fn bind_saved(
        &mut self,
        header: NativeVector16,
        tree: &Snapshot,
        exact_extents: &[ExtentFact],
    ) -> bool {
        self.saved_bound = false;
        self.saved_header = NativeVector16::default();
        self.allocations.clear();
        match self.allocations_of(header, tree, exact_extents) {
            Some(next) => {
                self.saved_header = header;
                self.allocations = next;
                self.saved_bound = true;
                self.failure = Failure::None;
                true
            }
            None => false,
        }
    }
    fn allocations_of(
        &mut self,
        header: NativeVector16,
        tree: &Snapshot,
        exact_extents: &[ExtentFact],
    ) -> Option<Vec<Allocation>> {
        let base = self.image_base;
        self.failure = Failure::Tree;
        if !self.captured
            || !tree.ready()
            || tree.image_base() != base
            || tree.roots().len() != header.count as usize
        {
            return None;
        }
        for (i, allocator) in tree.allocators().iter().enumerate() {
            let rva = if i != 0 {
                tree::CONTROL_ALLOCATOR_RVA
            } else {
                tree::OBJECT_ALLOCATOR_RVA
            };
            if allocator.address != base + rva
                || allocator.raw != self.identity.globals[i]
                || allocator.allocator != self.common()
            {
                return None;
            }
        }
        self.failure = Failure::Header;
        if header.count > header.capacity || !address(header.data, 1) || header.data & 7 != 0 {
            return None;
        }
        if header.capacity == 0 && header.data != base + VECTOR_ALLOCATOR_RVA {
            return None;
        }
        self.failure = Failure::ExtentFacts;
        if !exact_extents.is_empty() && !self.facts.exact_extent_facts_audited {
            return None;
        }
        let mut facts = BTreeMap::new();
        for fact in exact_extents {
            if (fact.kind != Kind::Object as u32 && fact.kind != Kind::Control as u32)
                || fact.bytes == 0
                || facts.contains_key(&(fact.kind, fact.address))
            {
                return None;
            }
            facts.insert((fact.kind, fact.address), *fact);
        }
        let mut next = Vec::new();
        if header.capacity != 0
            && !self.add(
                &mut next,
                Kind::TopVector,
                header.data,
                header.capacity as u64 * 8 + 8,
                ExtentKind::ExactAllocation,
                0,
            )
        {
            return None;
        }
        let mut controls = BTreeSet::new();
        for node in tree.nodes() {
            let minimum = match node.kind {
                NodeKind::SceneDriver => 0x38,
                NodeKind::RenderElements | NodeKind::CString => 0x40,
                NodeKind::DescriptionOwner => 0x30,
                NodeKind::Unknown => 8,
            };
            if !self.add_node(&mut next, Kind::Object, node.object, minimum, &mut facts) {
                return None;
            }
            if node.control_block != 0 {
                controls.insert(node.control_block);
            }
        }
        for handle in tree.handles() {
            if handle.control_block != 0 {
                controls.insert(handle.control_block);
            }
        }
        for control in controls {
            if !self.add_node(&mut next, Kind::Control, control, 8, &mut facts) {
                return None;
            }
        }
        for array in tree.arrays() {
            self.failure = Failure::FreeRoute;
            if array.allocator != base + VECTOR_ALLOCATOR_RVA
                || array.free_callback != base + VECTOR_FREE_RVA
                || array.count > array.capacity
                || (array.stride != 16 && array.stride != 48)
            {
                return None;
            }
            if array.capacity == 0 {
                if array.data != array.allocator || array.trailer_address != 0 {
                    return None;
                }
                continue;
            }
            let payload = array.capacity as u64 * array.stride as u64;
            self.failure = Failure::ExtentOverflow;
            if payload > u64::MAX - 15 {
                return None;
            }
            let rounded = (payload + 7) & !7u64;
            if array.data > u64::MAX - rounded || array.trailer_address != array.data + rounded {
                return None;
            }
            if !self.add(
                &mut next,
                Kind::Array,
                array.data,
                rounded + 8,
                ExtentKind::ExactAllocation,
                0,
            ) {
                return None;
            }
        }
        for string in tree.strings() {
            self.failure = Failure::FreeRoute;
            if string.mode != 1
                || string.capacity == 0
                || string.allocator_value != 0
                || string.free_callback != 0
                || string.fallback_global_address != base + tree::STRING_ALLOCATOR_RVA
                || string.fallback_raw != self.identity.globals[2]
                || string.fallback_allocator != self.common()
                || string.fallback_free_adapter != base + tree::STRING_FREE_ADAPTER_RVA
            {
                return None;
            }
            let bytes = (string.capacity as u64 + 8) & !7u64;
            if string.allocation_bytes != bytes
                || !self.add(
                    &mut next,
                    Kind::CString,
                    string.data,
                    bytes,
                    ExtentKind::ExactAllocation,
                    0,
                )
            {
                return None;
            }
        }
        self.failure = Failure::ExtentFacts;
        if !facts.is_empty() {
            return None;
        }
        Some(next)
    }

    /// The same identity captured again inside the same lifecycle.
    pub fn matches_before(&self, read: Read<'_>, observe: Observe<'_>) -> bool {
        if !self.captured || self.failure != Failure::None {
            return false;
        }
        let mut current = Receipt::default();
        current.capture(
            self.image_base,
            self.facts,
            self.lifecycle,
            Some(read),
            Some(observe),
        ) && current.identity == self.identity
            && current.lifecycle == self.lifecycle
    }
    /// A fresh receipt of the same identity in which every bound allocation keeps its region.
    pub fn matches_fresh(&self, current: &Receipt) -> bool {
        if !self.saved_bound
            || self.failure != Failure::None
            || !current.captured
            || current.failure != Failure::None
            || self.image_base != current.image_base
            || self.lifecycle != current.lifecycle
            || self.identity != current.identity
        {
            return false;
        }
        self.allocations.iter().all(|allocation| {
            current.classify_span(allocation.address, allocation.bytes) == allocation.region
        })
    }
    /// Mirrors 14455C's free-pointer route; not a statement about object extents.
    pub fn classify_pointer(&self, at: u64) -> Region {
        classify_pointer(&self.identity.bounds, self.captured, at)
    }
    pub fn classify_span(&self, at: u64, bytes: u64) -> Region {
        classify_span(&self.identity.bounds, self.captured, at, bytes)
    }
    pub fn is_captured(&self) -> bool {
        self.captured
    }
    pub fn saved_bound(&self) -> bool {
        self.saved_bound
    }
    pub fn all_exact_extents(&self) -> bool {
        self.saved_bound
            && !self.allocations.is_empty()
            && self
                .allocations
                .iter()
                .all(|allocation| allocation.extent == ExtentKind::ExactAllocation)
    }
    pub fn image_base(&self) -> u64 {
        self.image_base
    }
    pub fn common(&self) -> u64 {
        if self.captured {
            self.image_base + COMMON_RVA
        } else {
            0
        }
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn tracking(&self) -> &Lifecycle {
        &self.lifecycle
    }
    pub fn saved_header(&self) -> &NativeVector16 {
        &self.saved_header
    }
    pub fn allocations(&self) -> &[Allocation] {
        &self.allocations
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }

    fn read_identity(
        &mut self,
        base: u64,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        out: &mut Identity,
    ) -> bool {
        let common = base + COMMON_RVA;
        let globals = [
            tree::OBJECT_ALLOCATOR_RVA,
            tree::CONTROL_ALLOCATOR_RVA,
            tree::STRING_ALLOCATOR_RVA,
        ];
        for (i, rva) in globals.iter().enumerate() {
            self.failure = Failure::Read;
            if !read_u64(read, base + rva, &mut out.globals[i]) {
                return false;
            }
            self.failure = Failure::Globals;
            if out.globals[i] & !7u64 != common {
                return false;
            }
        }
        self.failure = Failure::Read;
        if !read_u64(read, common + POOL_OFFSET, &mut out.vptr) {
            return false;
        }
        self.failure = Failure::Component;
        if out.vptr != base + LIVE_POOL_VTABLE_RVA {
            return false;
        }
        self.failure = Failure::Read;
        let bounds = &mut out.bounds;
        if !read_u64(read, common + SMALL_BEGIN_OFFSET, &mut bounds.small_begin)
            || !read_u64(read, common + SMALL_END_OFFSET, &mut bounds.small_end)
            || !read_u64(read, common + MIDDLE_BEGIN_OFFSET, &mut bounds.middle_begin)
            || !read_u64(read, common + MIDDLE_END_OFFSET, &mut bounds.middle_end)
        {
            return false;
        }
        let bounds = out.bounds;
        self.failure = Failure::Bounds;
        address(bounds.small_begin, 1)
            && address(bounds.middle_begin, 1)
            && bounds.small_begin < bounds.small_end
            && bounds.middle_begin < bounds.middle_end
            && bounds.small_end <= usize::MAX as u64
            && bounds.middle_end <= usize::MAX as u64
            && (bounds.small_end <= bounds.middle_begin || bounds.middle_end <= bounds.small_begin)
    }
    fn add_node(
        &mut self,
        next: &mut Vec<Allocation>,
        kind: Kind,
        at: u64,
        minimum: u64,
        facts: &mut BTreeMap<(u32, u64), ExtentFact>,
    ) -> bool {
        let Some(found) = facts.get(&(kind as u32, at)).copied() else {
            return self.add(next, kind, at, minimum, ExtentKind::ParsedMinimum, minimum);
        };
        self.failure = Failure::ExtentFacts;
        if found.bytes < minimum {
            return false;
        }
        facts.remove(&(kind as u32, at));
        self.add(
            next,
            kind,
            at,
            found.bytes,
            ExtentKind::ExactAllocation,
            minimum,
        )
    }
    fn add(
        &mut self,
        next: &mut Vec<Allocation>,
        kind: Kind,
        at: u64,
        bytes: u64,
        extent: ExtentKind,
        minimum: u64,
    ) -> bool {
        self.failure = Failure::ExtentOverflow;
        if !address(at, bytes) || bytes == 0 {
            return false;
        }
        self.failure = Failure::UnsupportedPointer;
        let region = self.classify_pointer(at);
        if region == Region::Unsupported {
            return false;
        }
        self.failure = Failure::ExtentCrossesRegion;
        if self.classify_span(at, bytes) != region {
            return false;
        }
        self.failure = Failure::Overlap;
        if next.iter().any(|allocation| {
            at < allocation.address + allocation.bytes && allocation.address < at + bytes
        }) {
            return false;
        }
        next.push(Allocation {
            kind,
            region,
            extent,
            address: at,
            bytes,
            parsed_minimum: minimum,
        });
        true
    }
}

/// Receipt::ClassifyPointer for the bounds of a receipt (Unsupported unless captured).
pub fn classify_pointer(bounds: &Bounds, captured: bool, at: u64) -> Region {
    if !captured || at == 0 {
        return Region::Unsupported;
    }
    if at >= bounds.small_begin && at < bounds.small_end {
        return Region::Small;
    }
    if at >= bounds.middle_begin && at < bounds.middle_end {
        return Region::Middle;
    }
    Region::Unsupported
}
/// Receipt::ClassifySpan: the region holding the whole span.
pub fn classify_span(bounds: &Bounds, captured: bool, at: u64, bytes: u64) -> Region {
    if bytes == 0 || at > u64::MAX - bytes {
        return Region::Unsupported;
    }
    let region = classify_pointer(bounds, captured, at);
    let end = at + bytes;
    match region {
        Region::Small if end <= bounds.small_end => region,
        Region::Middle if end <= bounds.middle_end => region,
        _ => Region::Unsupported,
    }
}
