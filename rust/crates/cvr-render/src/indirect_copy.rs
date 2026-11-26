//! The native indirect copy's rules (`include/Render/NativeIndirectCopy.hpp`, `indirect_copy::detail`): the per-thread
//! ledger of actual native source callback scopes (a nested occurrence breaks its outer one: a scope labels an
//! occurrence, it does not order GPU aliases), the tracked read states (an indirect argument read, nothing but generic
//! reads, a copy source when the copy is ready), the graph parent record's shape, the admission of one observed
//! ExecuteIndirect argument read (the 2.5 caller 0x342B11, one 20-byte draw signature, its native record at registry +
//! 0x5C0AF8 + (handle - 1) * 0xB0, the parent and recording current, the native origin on the same device, no pending
//! state, a default-heap UAV buffer fully backing the native view) and the physical allocation's form (own or aliased).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions and `ScopeLedger`'s methods forward to `cvr_icp_*`; the
//! ledger's members stay in C++ (thread_local in its users), mirrored here. `tools/single_pass_tests/
//! indirect_copy_contract_test` runs its cases against them in its Rust variant.

/// detail::ScopeStamp.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeStamp {
    pub list: u64,
    pub generation: u64,
    pub position: u64,
    pub nonce: u64,
    pub thread: u32,
}
/// detail::ScopeLedger's members.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScopeLedger {
    pub frames: [ScopeStamp; 16],
    pub next: u64,
    pub depth: usize,
    pub broken: bool,
}
const _: () =
    assert!(core::mem::size_of::<ScopeStamp>() == 40 && core::mem::size_of::<ScopeLedger>() == 664);

impl ScopeLedger {
    /// ScopeLedger::Enter.
    pub fn enter(&mut self, list: u64, generation: u64, position: u64, thread: u32) -> ScopeStamp {
        if self.broken
            || list == 0
            || generation == 0
            || position == 0
            || thread == 0
            || self.depth == self.frames.len()
            || self.next == u64::MAX
        {
            return ScopeStamp::default();
        }
        self.next += 1;
        let stamp = ScopeStamp {
            list,
            generation,
            position,
            nonce: self.next,
            thread,
        };
        self.frames[self.depth] = stamp;
        self.depth += 1;
        stamp
    }
    /// ScopeLedger::Exit.
    pub fn exit(&mut self, stamp: &ScopeStamp) -> bool {
        if self.depth == 0 || self.frames[self.depth - 1] != *stamp {
            self.broken = true;
            return false;
        }
        self.depth -= 1;
        self.frames[self.depth] = ScopeStamp::default();
        true
    }
    /// ScopeLedger::Current.
    pub fn current(&self) -> ScopeStamp {
        if !self.broken && self.depth != 0 {
            self.frames[self.depth - 1]
        } else {
            ScopeStamp::default()
        }
    }
    /// ScopeLedger::Matches.
    pub fn matches(
        &self,
        stamp: &ScopeStamp,
        list: u64,
        generation: u64,
        position: u64,
        thread: u32,
    ) -> bool {
        stamp.nonce != 0
            && stamp.nonce == self.next
            && self.current() == *stamp
            && stamp.list == list
            && stamp.generation == generation
            && stamp.position == position
            && stamp.thread == thread
    }
}

/// geometry::NativeBuffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeBuffer {
    pub resource: u64,
    pub gpu: u64,
    pub bytes: u64,
    pub registry: u64,
    pub context: u64,
    pub handle: u32,
    pub kind: u32,
}
/// geometry::ReadState.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadState {
    pub value: u32,
    pub status: u32,
    pub kind: u32,
}
/// preparation::GraphParentRecord.
pub use crate::preparation_trace::GraphParentRecord;
/// detail::Observation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Observation {
    pub native: NativeBuffer,
    pub list: u64,
    pub generation: u64,
    pub position: u64,
    pub resource: u64,
    pub backing_gpu: u64,
    pub backing_bytes: u64,
    pub offset: u64,
    pub bytes: u64,
    pub record_address: u64,
    pub caller: u32,
    pub max_count: u32,
    pub signature_kind: u32,
    pub stride: u32,
    pub heap: u32,
    pub flags: u32,
    pub pending: u32,
    pub context_kind: u32,
    pub low_type: u32,
    pub state: ReadState,
    pub full_backing: bool,
    pub parent_current: bool,
    pub recording_current: bool,
    pub native_origin: bool,
    pub same_device: bool,
}
/// detail::Allocation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allocation {
    pub address: u64,
    pub vtable: u64,
    pub pimpl: u64,
    pub bytes: u64,
    pub resource: u64,
    pub heap: u64,
    pub block: u64,
    pub handle: u64,
    pub kind: u32,
}
/// detail::AllocationBinding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationBinding {
    pub allocation: u64,
    pub payload: u64,
    pub control: u64,
    pub flags: u64,
    pub alignment: u64,
    pub bytes: u64,
    pub alias: bool,
}
const _: () = assert!(
    core::mem::size_of::<NativeBuffer>() == 48
        && core::mem::size_of::<GraphParentRecord>() == 80
        && core::mem::size_of::<Observation>() == 176
        && core::mem::size_of::<Allocation>() == 72
        && core::mem::size_of::<AllocationBinding>() == 56
);

/// indirect_copy::Failure (the values the admission reports).
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const PARENT: u32 = 4;
    pub const BINDING: u32 = 5;
    pub const STATE: u32 = 6;
    pub const BACKING: u32 = 7;
}
const GENERIC_READ: u32 = 0xAC3;
const INDIRECT_ARGUMENT: u32 = 0x200;
const COPY_SOURCE: u32 = 0x800;
const HEAP_TYPE_DEFAULT: u32 = 1;
const ALLOW_UNORDERED_ACCESS: u32 = 0x4;

/// detail::TrackedClass.
pub fn tracked_class(kind: u32) -> bool {
    kind == 0 || kind == 3
}
/// detail::ReadMask.
pub fn read_mask(state: &ReadState, kind: u32, copy_ready: bool) -> bool {
    tracked_class(kind)
        && state.kind == kind
        && state.status == 2
        && state.value & INDIRECT_ARGUMENT != 0
        && state.value & !GENERIC_READ == 0
        && (!copy_ready || state.value & COPY_SOURCE != 0)
}
/// detail::Parent.
pub fn parent(parent: &GraphParentRecord) -> bool {
    parent.status == 0
        && parent.context != 0
        && parent.manager != 0
        && parent.fingerprint != 0
        && parent.object != 0
        && parent.work != 0
        && parent.handle < 65536
        && parent.ordinal < 220
        && parent.slot < 220
        && parent.view_index < 2
}
/// detail::Admit.
pub fn admit(value: &Observation, graph_parent: &GraphParentRecord) -> u32 {
    let native = &value.native;
    if value.list == 0
        || value.generation == 0
        || value.position == 0
        || native.resource == 0
        || native.registry == 0
        || native.context == 0
        || native.handle == 0
        || native.handle > 0x100000
        || native.resource != value.resource
        || value.low_type != 0xB
        || value.caller != 0x342B11
        || value.max_count != 1
        || value.signature_kind != 1
        || value.stride != 20
        || value.bytes != 20
        || value.offset % 4 != 0
    {
        return failure::ARGUMENTS;
    }
    let relative = 0x5C0AF8 + (native.handle - 1) as u64 * 0xB0;
    if native.registry > u64::MAX - relative - 0xB0
        || value.record_address != native.registry + relative
    {
        return failure::ARGUMENTS;
    }
    if !parent(graph_parent) || !value.parent_current {
        return failure::PARENT;
    }
    if !value.recording_current || !value.native_origin || !value.same_device {
        return failure::BINDING;
    }
    if value.pending != 0
        || (value.context_kind != 1 && value.context_kind != 2)
        || !read_mask(&value.state, native.kind, false)
    {
        return failure::STATE;
    }
    if value.heap != HEAP_TYPE_DEFAULT
        || value.flags != ALLOW_UNORDERED_ACCESS
        || !value.full_backing
        || value.backing_gpu == 0
        || value.backing_bytes == 0
        || value.backing_gpu > u64::MAX - value.backing_bytes
        || native.gpu != value.backing_gpu
        || native.bytes != value.backing_bytes
        || value.offset > value.backing_bytes
        || value.bytes > value.backing_bytes - value.offset
    {
        return failure::BACKING;
    }
    failure::NONE
}
/// detail::AllocationForm.
pub fn allocation_form(
    binding: &AllocationBinding,
    allocation: &Allocation,
    source: u64,
    bytes: u64,
) -> bool {
    if source == 0
        || bytes == 0
        || binding.allocation == 0
        || binding.allocation != allocation.address
        || allocation.pimpl == 0
        || allocation.bytes == 0
        || allocation.bytes < bytes
    {
        return false;
    }
    if !binding.alias {
        return binding.payload == 0
            && binding.control == 0
            && ((allocation.kind == 0 && allocation.heap == 0)
                || (allocation.kind == 1 && allocation.heap != 0))
            && allocation.resource == source;
    }
    binding.payload != 0
        && binding.control != 0
        && binding.flags == 0xC0
        && binding.alignment == 0x10000
        && binding.bytes == allocation.bytes
        && (allocation.kind == 1 || allocation.kind == 2)
        && allocation.resource == 0
        && allocation.heap != 0
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// ScopeLedger::Enter (`which` 0: `out` the new stamp), Current (1: `out`).
///
/// # Safety
/// `ledger` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_scope(
    ledger: *mut ScopeLedger,
    which: u32,
    list: u64,
    generation: u64,
    position: u64,
    thread: u32,
    out: *mut ScopeStamp,
) {
    unsafe {
        *out = if which == 0 {
            (*ledger).enter(list, generation, position, thread)
        } else {
            (*ledger).current()
        };
    }
}
/// ScopeLedger::Exit.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_scope_exit(
    ledger: *mut ScopeLedger,
    stamp: *const ScopeStamp,
) -> bool {
    unsafe { (*ledger).exit(&*stamp) }
}
/// ScopeLedger::Matches.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_scope_matches(
    ledger: *const ScopeLedger,
    stamp: *const ScopeStamp,
    list: u64,
    generation: u64,
    position: u64,
    thread: u32,
) -> bool {
    unsafe { (*ledger).matches(&*stamp, list, generation, position, thread) }
}
/// TrackedClass (`state` null) / ReadMask.
///
/// # Safety
/// `state` is null or valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_read_mask(
    state: *const ReadState,
    kind: u32,
    copy_ready: bool,
) -> bool {
    if state.is_null() {
        tracked_class(kind)
    } else {
        unsafe { read_mask(&*state, kind, copy_ready) }
    }
}
/// Parent.
///
/// # Safety
/// `record` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_parent(record: *const GraphParentRecord) -> bool {
    unsafe { parent(&*record) }
}
/// Admit.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_admit(
    value: *const Observation,
    record: *const GraphParentRecord,
) -> u32 {
    unsafe { admit(&*value, &*record) }
}
/// AllocationForm.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_icp_allocation_form(
    binding: *const AllocationBinding,
    allocation: *const Allocation,
    source: u64,
    bytes: u64,
) -> bool {
    unsafe { allocation_form(&*binding, &*allocation, source, bytes) }
}
