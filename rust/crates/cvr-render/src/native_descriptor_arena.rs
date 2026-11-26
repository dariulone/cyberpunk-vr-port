//! The native descriptor arena (formerly src/Render/NativeDescriptorArena.cpp, `native_descriptors`): a retained
//! source's owned CPU descriptor tables copied into the engine's shader-visible camera heap at the draw -- the
//! native camera context's flush state read twice, the bound tables checked against the source's bindings, tables
//! reused from earlier sources of the same recording (the per-eye cache), the fresh ones either placed after the
//! camera heap's cursor (advanced with a guarded compare-exchange) or reserved from the engine's descriptor ring
//! (its reserve function at 0x419DF4), the flush state and recording checked again, the descriptors copied, a COM
//! owner kept by the list -- then Bind (the tables set on the list after the source's bindings and recording are
//! checked again) and ValidateSubmission (the owner, recording and rings still alive at the queue). The pure rules
//! are descriptor_arena.rs's.

use crate::camera_binding::Context;
use crate::camera_flush;
use crate::descriptor_arena::{self as rules, Range, Ring};
use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12DescriptorHeap, ID3D12Device, ID3D12GraphicsCommandList, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, D3D12_GPU_DESCRIPTOR_HANDLE,
};

/// native_descriptors::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const CODE: u32 = 2;
    pub const RECORDING: u32 = 3;
    pub const STATE: u32 = 4;
    pub const HEAP: u32 = 5;
    pub const TABLE: u32 = 6;
    pub const CAPACITY: u32 = 7;
    pub const CHANGED: u32 = 8;
    pub const RETENTION: u32 = 9;
    pub const RESERVATION: u32 = 10;
    pub const EXCEPTION: u32 = 11;
}
thread_local! {
    static FAILURE: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
fn set_failure(value: u32) {
    FAILURE.with(|f| f.set(value));
}
pub fn last_failure() -> u32 {
    FAILURE.with(|f| f.get())
}
fn fail<T>(value: u32) -> Option<T> {
    set_failure(value);
    None
}

// ---- the source's tables, leases and the owner

/// descriptors::OwnedSourceTable (its three references raw here; owned copies are `Table`).
#[repr(C)]
pub struct SourceTable {
    pub parameter: u32,
    pub count: u32,
    pub cpu_heap: *mut c_void,
    pub owner: *mut c_void,
    pub identity: u64,
    pub version: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub provenance: *mut c_void,
}
const _: () = assert!(core::mem::size_of::<SourceTable>() == 64);
/// An owned copy of a source table (its three references held).
struct Table {
    parameter: u32,
    count: u32,
    cpu_heap: Option<ID3D12DescriptorHeap>,
    owner: Option<IUnknown>,
    identity: u64,
    version: u64,
    source_heap_unused: u64,
    provenance: Option<IUnknown>,
}
fn borrowed<T: Interface + Clone>(raw: *mut c_void) -> Option<T> {
    // SAFETY: a live object of the source (or null); a new reference.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
impl Table {
    fn copy(value: &SourceTable) -> Table {
        Table {
            parameter: value.parameter,
            count: value.count,
            cpu_heap: borrowed(value.cpu_heap),
            owner: borrowed(value.owner),
            identity: value.identity,
            version: value.version,
            source_heap_unused: value.source_heap,
            provenance: borrowed(value.provenance),
        }
    }
    fn raw(&self) -> *mut c_void {
        self.cpu_heap.as_ref().map_or(null_mut(), |h| h.as_raw())
    }
}
/// native_recording::Lease (its token owned).
#[derive(Clone, Default)]
pub struct Recording {
    pub token: Option<IUnknown>,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
impl Recording {
    fn raw(&self) -> crate::native_recording::Lease {
        crate::native_recording::Lease { token: self.token.as_ref().map_or(null_mut(), |t| t.as_raw()), generation: self.generation, context: self.context, list: self.list, registry: self.registry }
    }
    fn same(&self, b: &Recording) -> bool {
        self.token.is_some() && self.token == b.token && self.list == b.list && self.generation == b.generation && self.context == b.context && self.registry == b.registry
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct RingLease {
    registry: u64,
    ring: u64,
    heap: u64,
    capacity: u64,
    begin: u64,
    end: u64,
}
/// The owner kept alive by the command list: the recording, the camera context, the original bindings, the heaps
/// and the tables it placed, the reused owners, its ring lease and the borrowed rings, the bindings it sets.
#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    references: AtomicU32,
    recording: Recording,
    context: Context,
    original: cvr_gpu::scene_state::Bindings,
    heap: Option<ID3D12DescriptorHeap>,
    root: Option<IUnknown>,
    heaps: [usize; 2],
    heap_refs: [Option<IUnknown>; 2],
    device: Option<ID3D12Device>,
    tables: Vec<Table>,
    prior: Vec<IUnknown>,
    ring: Option<RingLease>,
    borrowed_rings: Vec<RingLease>,
    bindings: Vec<(u32, u64)>,
    range: Range,
}
const OWNER_TYPE: GUID = GUID::from_values(0x13c4cd91, 0x2a6b, 0x4591, [0xbb, 0x1d, 0x14, 0x72, 0x69, 0x5c, 0x81, 0xc8]);
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's pointers.
    unsafe {
        *out = null_mut();
        if *iid != IUnknown::IID && *iid != OWNER_TYPE {
            return E_NOINTERFACE;
        }
        *out = this;
        owner_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live owner.
    unsafe { (*this.cast::<Owner>()).references.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: a live owner; the last reference frees it.
    let left = unsafe { (*this.cast::<Owner>()).references.fetch_sub(1, SeqCst) - 1 };
    if left == 0 {
        // SAFETY: made by Box::into_raw in copy_tables.
        drop(unsafe { Box::from_raw(this.cast::<Owner>()) });
    }
    left
}
/// The owner behind an IUnknown (QueryInterface for the owner type), held.
fn owner_of(value: Option<&IUnknown>) -> Option<OwnerRef> {
    let value = value?;
    let mut out = null_mut();
    // SAFETY: a live object; a QueryInterface for the owner type.
    let hr = unsafe { (Interface::vtable(value).QueryInterface)(value.as_raw(), &OWNER_TYPE, &mut out) };
    (hr.is_ok() && !out.is_null()).then(|| OwnerRef(out))
}
/// A held owner reference.
struct OwnerRef(*mut c_void);
impl OwnerRef {
    fn get(&self) -> &Owner {
        // SAFETY: a live owner while this reference is held.
        unsafe { &*self.0.cast::<Owner>() }
    }
    fn unknown(&self) -> IUnknown {
        // SAFETY: a live COM object; a new reference.
        unsafe { IUnknown::from_raw_borrowed(&self.0) }.cloned().expect("owner")
    }
}
impl Drop for OwnerRef {
    fn drop(&mut self) {
        // SAFETY: the reference this value holds.
        unsafe { owner_release(self.0) };
    }
}

// ---- the cache and the tables

struct Entry {
    heap: usize,
    source: Table,
    gpu: u64,
    ordinal: u64,
    owner: IUnknown,
}
/// native_descriptors::Cache: the recording's placed tables.
#[derive(Default)]
pub struct Cache {
    storage: Option<(Recording, Vec<Entry>)>,
}
/// native_descriptors::Tables.
#[derive(Default)]
pub struct Tables {
    pub owner: Option<IUnknown>,
    pub bindings: Vec<(u32, u64)>,
    pub recording: Recording,
    pub range: Range,
    pub heap: u64,
    pub base: u64,
    pub capacity: u64,
    pub borrowed_mask: u64,
    pub descriptors: u32,
    pub entries: Vec<[u64; 8]>,
    pub ring: [u64; 16],
}

// ---- the engine's ring

const RING_OFFSET: u64 = 0x14F1E40;
const OUTSTANDING_OFFSET: u64 = 0x1410730;
fn read<T: Copy>(address: u64) -> Option<T> {
    if address == 0 || address > u64::MAX - core::mem::size_of::<T>() as u64 {
        return None;
    }
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    let mut copied = 0usize;
    // SAFETY: a read of this process' memory that may fail.
    let ok = unsafe {
        windows::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            windows::Win32::System::Threading::GetCurrentProcess(),
            address as *const c_void,
            value.as_mut_ptr().cast(),
            core::mem::size_of::<T>(),
            Some(&mut copied),
        )
    }
    .is_ok();
    // SAFETY: fully written when the read succeeded.
    (ok && copied == core::mem::size_of::<T>()).then(|| unsafe { value.assume_init() })
}
unsafe extern "C-unwind" fn read_state(address: usize, out: *mut c_void, bytes: usize) -> bool {
    if address == 0 || address > usize::MAX - bytes {
        return false;
    }
    let mut copied = 0usize;
    // SAFETY: a read of this process' memory that may fail into the caller's buffer.
    unsafe {
        windows::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            windows::Win32::System::Threading::GetCurrentProcess(),
            address as *const c_void,
            out,
            bytes,
            Some(&mut copied),
        )
    }
    .is_ok()
        && copied == bytes
}
fn read_ring(registry: u64) -> Option<Ring> {
    if registry == 0 || registry > u64::MAX - RING_OFFSET - 72 {
        return None;
    }
    let ring = registry + RING_OFFSET;
    let count: u32 = read(ring + 64)?;
    if count != 1 {
        return None;
    }
    let heap: u64 = read(ring + 56)?;
    let capacity: u64 = read(ring + 16)?;
    let tail: u64 = read(ring + 8)?;
    let head: u64 = read(ring)?;
    let tail_again: u64 = read(ring + 8)?;
    if tail != tail_again {
        return None;
    }
    let outstanding: u32 = read(registry + OUTSTANDING_OFFSET)?;
    Some(Ring { address: ring, heap, capacity, head, tail: tail_again, outstanding: u64::from(outstanding) })
}
fn ring_alive(lease: &RingLease) -> bool {
    read_ring(lease.registry).is_some_and(|now| {
        rules::valid_ring(&now, lease.heap, 1_048_576) && now.address == lease.ring && now.capacity == lease.capacity && now.tail <= lease.begin && now.head >= lease.end
    })
}
/// ReserveRing: `count` descriptors reserved from the engine's ring; the range, the lease and the receipt.
fn reserve_ring(base: u64, recording: &Recording, heap: &ID3D12DescriptorHeap, count: u32, stride: u32) -> Option<(Range, RingLease, [u64; 16])> {
    let identity = heap.as_raw() as u64;
    // SAFETY: a live heap.
    let desc = unsafe { heap.GetDesc() };
    let Some(before) = read_ring(recording.registry) else { return fail(failure::HEAP) };
    if !rules::valid_ring(&before, identity, desc.NumDescriptors)
        || count == 0
        || count > 8192
        || u64::from(count) > before.capacity
        || before.head > u64::MAX - before.capacity
    {
        return fail(failure::HEAP);
    }
    type Reserve = unsafe extern "system" fn(*mut c_void, u64, u64, *mut c_void) -> u64;
    // SAFETY: the engine's reserve function (code table 4 verified it), the ring its object.
    let reserve: Reserve = unsafe { core::mem::transmute::<usize, Reserve>((base + 0x419DF4) as usize) };
    // SAFETY: as above.
    let first = unsafe { reserve(before.address as *mut c_void, u64::from(count), 1, null_mut()) };
    if first == before.capacity {
        return fail(failure::CAPACITY);
    }
    let after = if first > u64::from(u32::MAX) { None } else { read_ring(recording.registry) };
    let Some(absolute) = after.and_then(|after| rules::ring_result(&before, &after, first as u32, count)) else { return fail(failure::RESERVATION) };
    let after = after.expect("read");
    // SAFETY: a live heap.
    let (cpu, gpu) = unsafe { (heap.GetCPUDescriptorHandleForHeapStart(), heap.GetGPUDescriptorHandleForHeapStart()) };
    let end = (first + u64::from(count)) * u64::from(stride);
    if end > u64::MAX - cpu.ptr as u64 || end > u64::MAX - gpu.ptr {
        return fail(failure::HEAP);
    }
    let range = Range { first: first as u32, count, before: 0, after: count, cpu: cpu.ptr + first as usize * stride as usize, gpu: gpu.ptr + first * u64::from(stride) };
    let lease = RingLease { registry: recording.registry, ring: before.address, heap: identity, capacity: before.capacity, begin: absolute, end: absolute + u64::from(count) };
    let receipt = [
        0,
        recording.registry,
        before.address,
        identity,
        before.capacity,
        before.head,
        before.tail,
        after.head,
        after.tail,
        first,
        u64::from(count),
        absolute,
        before.outstanding,
        after.outstanding,
        1,
        0,
    ];
    Some((range, lease, receipt))
}
fn same_binding(a: &cvr_gpu::scene_state::Bindings, b: &cvr_gpu::scene_state::Bindings) -> bool {
    a.root == b.root && a.heap_count == b.heap_count && a.heaps == b.heaps && a.table_mask == b.table_mask && a.tables == b.tables
}
fn same_device(device: &ID3D12Device, child: *mut c_void) -> bool {
    // SAFETY: a live device child (or null).
    let Some(child) = (unsafe { windows::Win32::Graphics::Direct3D12::ID3D12DeviceChild::from_raw_borrowed(&child) }) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live object.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.as_ref() == Some(device)
}
fn capture_bindings(list: *mut c_void) -> Option<cvr_gpu::scene_state::Bindings> {
    // SAFETY: plain bindings, written whole on success.
    let mut live: cvr_gpu::scene_state::Bindings = unsafe { core::mem::zeroed() };
    // SAFETY: a live list; a writable value.
    unsafe { cvr_gpu::scene_state::cvr_scene_capture_source_bindings(list as usize, &mut live) }.then_some(live)
}
fn current_context(base: u64) -> Context {
    let mut context = Context::default();
    // SAFETY: a writable context.
    unsafe { crate::camera_upload::cvr_ncu_current(base, &mut context) };
    context
}
fn validate_recording(recording: &Recording, list: *mut c_void, context: &Context) -> bool {
    let raw = recording.raw();
    // SAFETY: the lease and the context (native_recording::Context's layout).
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(&raw, list, (context as *const Context).cast()) }
}
fn keep(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: a live list and object.
    unsafe { cvr_gpu::command_resources::cvr_gpu_keep_command_resources(list, &object, 1) }
}
/// Advance: the camera heap's cursor moved from `before` to `after` (a guarded 32-bit compare-exchange at state+0x14).
fn advance(state: u64, before: u32, after: u32) -> bool {
    state != 0
        && state & 3 == 0
        && state <= u64::MAX - 0x18
        // SAFETY: a guarded compare-exchange on the engine's camera state.
        && unsafe { cvr_engine::code::guarded_cmpxchg32((state + 0x14) as usize, after, before) } == Some(before)
}
/// CopyTables: the source's tables placed in the camera heap (see the module's documentation).
pub fn copy_tables(list: *mut c_void, bindings: &cvr_gpu::scene_state::Bindings, tables: &[SourceTable], ordinal: u64, cache: &mut Cache) -> Option<Tables> {
    set_failure(failure::ARGUMENTS);
    if cvr_host::probe::internal() || list.is_null() || tables.is_empty() || tables.len() > 64 || bindings.root == 0 || bindings.heap_count == 0 || bindings.heap_count > 2 {
        return None;
    }
    // SAFETY: the module's own image base.
    let base = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map_or(0, |h| h.0 as u64);
    if !crate::code_tables::cvr_code_table(4, base) {
        return fail(failure::CODE);
    }
    let context = current_context(base);
    let mut raw = crate::native_recording::Lease { token: null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
    // SAFETY: the list and the context (native_recording::Context's layout); a writable lease.
    let captured = context.valid() && context.list == list as u64 && unsafe { crate::native_recording::cvr_native_recording_capture(list, (&context as *const Context).cast(), &mut raw) };
    let recording = Recording {
        // SAFETY: the capture's token reference, handed over.
        token: (!raw.token.is_null()).then(|| unsafe { IUnknown::from_raw(raw.token) }),
        generation: raw.generation,
        context: raw.context,
        list: raw.list,
        registry: raw.registry,
    };
    if !captured || !validate_recording(&recording, list, &context) {
        return fail(failure::RECORDING);
    }
    if cache.storage.as_ref().is_some_and(|(r, _)| !r.same(&recording)) {
        cache.storage = None;
    }
    if cache.storage.is_none() {
        cache.storage = Some((recording.clone(), Vec::new()));
    }
    let Some(live) = capture_bindings(list) else { return fail(failure::STATE) };
    if !same_binding(bindings, &live) {
        return fail(failure::STATE);
    }
    let Some(before) = camera_flush::read(Some(read_state), context, 3) else { return fail(failure::STATE) };
    let mut owner = Box::new(Owner {
        vtbl: &OWNER_VTBL,
        references: AtomicU32::new(1),
        recording: recording.clone(),
        context,
        original: *bindings,
        heap: None,
        root: borrowed(bindings.root as *mut c_void),
        heaps: [0; 2],
        heap_refs: [None, None],
        device: None,
        tables: Vec::new(),
        prior: Vec::new(),
        ring: None,
        borrowed_rings: Vec::new(),
        bindings: Vec::new(),
        range: Range::default(),
    });
    for i in 0..bindings.heap_count as usize {
        owner.heaps[i] = bindings.heaps[i];
        owner.heap_refs[i] = borrowed(bindings.heaps[i] as *mut c_void);
        if bindings.heaps[i] as u64 == before.heap {
            owner.heap = borrowed(bindings.heaps[i] as *mut c_void);
        }
    }
    let device = list_device(list);
    let Some(heap) = owner.heap.clone() else { return fail(failure::HEAP) };
    let Some(device) = device.filter(|d| same_device(d, heap.as_raw())) else { return fail(failure::HEAP) };
    owner.device = Some(device.clone());
    // SAFETY: a live heap and device.
    let (desc, cpu, gpu_start) = unsafe { (heap.GetDesc(), heap.GetCPUDescriptorHandleForHeapStart(), heap.GetGPUDescriptorHandleForHeapStart()) };
    // SAFETY: as above.
    let stride = unsafe { device.GetDescriptorHandleIncrementSize(desc.Type) };
    if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV
        || desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
        || cpu.ptr == 0
        || gpu_start.ptr == 0
        || before.stride != stride
        || before.capacity == 0
        || before.cursor > before.capacity
        || u64::from(before.base) + u64::from(before.capacity) > u64::from(desc.NumDescriptors)
        || (before.cpu_valid != 0 && before.cached_cpu != cpu.ptr as u64)
        || (before.gpu_valid != 0 && before.cached_gpu != gpu_start.ptr)
    {
        return fail(failure::HEAP);
    }
    let (mut total, mut fresh, mut mask) = (0u32, 0u32, 0u64);
    let mut reused: Vec<Option<usize>> = Vec::new();
    let entries = &cache.storage.as_ref().expect("cache").1;
    for table in tables {
        if table.cpu_heap.is_null()
            || table.owner.is_null()
            || table.provenance.is_null()
            || table.parameter >= 64
            || bindings.table_mask & (1u64 << table.parameter) == 0
            || table.source_gpu != bindings.tables[table.parameter as usize]
            || !same_device(&device, table.cpu_heap)
        {
            return fail(failure::TABLE);
        }
        // SAFETY: a live CPU heap.
        let source = unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&table.cpu_heap) }.map(|h| unsafe { h.GetDesc() }).expect("heap");
        if source.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE || table.count == 0 || table.count > source.NumDescriptors {
            return fail(failure::TABLE);
        }
        if source.Type == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER {
            continue;
        }
        if source.Type != desc.Type || table.count > 8192 - total || mask & (1u64 << table.parameter) != 0 {
            return fail(failure::TABLE);
        }
        mask |= 1u64 << table.parameter;
        total += table.count;
        owner.tables.push(Table::copy(table));
        let existing = entries.iter().position(|e| {
            e.heap == heap.as_raw() as usize
                && e.ordinal < ordinal
                && e.source.raw() == table.cpu_heap
                && e.source.owner.as_ref().map_or(null_mut(), |o| o.as_raw()) == table.owner
                && e.source.count == table.count
                && e.source.identity == table.identity
                && e.source.version == table.version
        });
        reused.push(existing);
        if let Some(at) = existing {
            let entry = &entries[at];
            owner.prior.push(entry.owner.clone());
            let Some(prior) = owner_of(Some(&entry.owner)) else { return fail(failure::CHANGED) };
            if let Some(ring) = prior.get().ring {
                owner.borrowed_rings.push(ring);
            }
        } else {
            fresh += table.count;
        }
    }
    if total == 0 {
        return fail(failure::TABLE);
    }
    if before.dirty & before.graphics_mask != 0 || before.rebind != 0 {
        return fail(failure::STATE);
    }
    let mut extra_ring = false;
    if fresh != 0 {
        match rules::plan(&before, fresh, desc.NumDescriptors, stride, cpu.ptr as u64, gpu_start.ptr) {
            Some(range) => owner.range = range,
            None => {
                owner.range = Range::default();
                extra_ring = true;
            }
        }
    } else {
        if u64::from(before.base) + u64::from(before.capacity) > u64::from(desc.NumDescriptors) || before.cursor > before.capacity {
            return fail(failure::HEAP);
        }
        owner.range = Range { first: before.base + before.cursor, count: 0, before: before.cursor, after: before.cursor, cpu: 0, gpu: 0 };
    }
    let mut offset = 0u32;
    let mut next = Tables::default();
    for (index, table) in owner.tables.iter().enumerate() {
        let old = reused[index].map(|at| &entries[at]);
        let handle = old.map_or(owner.range.gpu + u64::from(offset) * u64::from(stride), |e| e.gpu);
        owner.bindings.push((table.parameter, handle));
        next.entries.push([
            ordinal,
            u64::from(table.parameter),
            u64::from(table.count),
            table.raw() as u64,
            handle,
            old.map_or(ordinal, |e| e.ordinal),
            old.map_or(u64::from(table.parameter), |e| u64::from(e.source.parameter)),
            u64::from(old.is_some()),
        ]);
        if old.is_none() {
            offset += table.count;
        }
    }
    next.recording = recording.clone();
    next.range = owner.range;
    next.bindings = owner.bindings.clone();
    next.descriptors = total;
    next.heap = before.heap;
    next.base = u64::from(before.base);
    next.capacity = u64::from(before.capacity);
    next.borrowed_mask = bindings.table_mask & !mask;
    let check = camera_flush::read(Some(read_state), context, 3);
    if check.as_ref() != Some(&before) || current_context(base) != context || !validate_recording(&recording, list, &context) {
        return fail(failure::CHANGED);
    }
    // The owner as a COM object from here on (the list keeps it).
    let raw_owner = Box::into_raw(owner).cast::<c_void>();
    // SAFETY: the new owner's first reference becomes this IUnknown.
    let owner_unknown = unsafe { IUnknown::from_raw(raw_owner) };
    // SAFETY: the owner is ours until returned.
    let owner = unsafe { &mut *raw_owner.cast::<Owner>() };
    if !keep(list, raw_owner) {
        return fail(failure::RETENTION);
    }
    if extra_ring {
        let (range, lease, receipt) = reserve_ring(base, &recording, &heap, fresh, stride)?;
        owner.range = range;
        owner.ring = Some(lease);
        next.ring = receipt;
        next.ring[0] = ordinal;
    } else if fresh != 0 && !advance(context.state, before.cursor, owner.range.after) {
        return fail(failure::RESERVATION);
    }
    let check = camera_flush::read(Some(read_state), context, 3);
    let consistent = match check.as_ref() {
        Some(check) if fresh != 0 && !extra_ring => rules::advanced(&before, check, &owner.range),
        Some(check) => *check == before,
        None => false,
    };
    if !consistent {
        return fail(failure::CHANGED);
    }
    if extra_ring {
        next.ring[15] = 1;
        next.range = owner.range;
        next.base = u64::from(owner.range.first);
        next.capacity = u64::from(owner.range.count);
        offset = 0;
        for index in 0..owner.tables.len() {
            if next.entries[index][7] == 0 {
                owner.bindings[index].1 = owner.range.gpu + u64::from(offset) * u64::from(stride);
                next.entries[index][4] = owner.bindings[index].1;
                next.bindings[index] = owner.bindings[index];
                offset += owner.tables[index].count;
            }
        }
    }
    {
        let _internal = cvr_host::probe::Internal::new();
        offset = 0;
        for (index, table) in owner.tables.iter().enumerate() {
            if next.entries[index][7] != 0 {
                continue;
            }
            let Some(source) = table.cpu_heap.as_ref() else { continue };
            // SAFETY: live device and heaps; the destination inside the owner's range.
            unsafe {
                device.CopyDescriptorsSimple(
                    table.count,
                    D3D12_CPU_DESCRIPTOR_HANDLE { ptr: owner.range.cpu + offset as usize * stride as usize },
                    source.GetCPUDescriptorHandleForHeapStart(),
                    desc.Type,
                )
            };
            offset += table.count;
        }
    }
    let entries = &mut cache.storage.as_mut().expect("cache").1;
    entries.reserve(owner.tables.len());
    for index in 0..owner.tables.len() {
        if next.entries[index][7] == 0 {
            let table = &owner.tables[index];
            entries.push(Entry {
                heap: heap.as_raw() as usize,
                source: Table {
                    parameter: table.parameter,
                    count: table.count,
                    cpu_heap: table.cpu_heap.clone(),
                    owner: table.owner.clone(),
                    identity: table.identity,
                    version: table.version,
                    source_heap_unused: table.source_heap_unused,
                    provenance: table.provenance.clone(),
                },
                gpu: owner.bindings[index].1,
                ordinal,
                owner: owner_unknown.clone(),
            });
        }
    }
    next.owner = Some(owner_unknown);
    set_failure(failure::NONE);
    Some(next)
}
fn list_device(list: *mut c_void) -> Option<ID3D12Device> {
    // SAFETY: the hook's live list.
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list.
    unsafe { list.GetDevice(&mut device) }.ok()?;
    device
}
fn same_tables(tables: &Tables, owner: &Owner) -> bool {
    tables.bindings == owner.bindings
}
/// Bind: the owner's tables set on the list.
pub fn bind(list: *mut c_void, tables: &Tables) -> bool {
    let Some(owner) = owner_of(tables.owner.as_ref()) else { return false };
    let o = owner.get();
    let object = owner.unknown();
    // SAFETY: a live list and owner.
    if list.is_null()
        || !same_tables(tables, o)
        || !tables.recording.same(&o.recording)
        || list as u64 != o.recording.list
        || !unsafe { cvr_gpu::command_resources::cvr_gpu_has_command_resource(list, object.as_raw()) }
    {
        return false;
    }
    // SAFETY: the module's own image base.
    let base = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map_or(0, |h| h.0 as u64);
    let context = current_context(base);
    if context != o.context || !validate_recording(&o.recording, list, &context) {
        return false;
    }
    let Some(current) = capture_bindings(list) else { return false };
    if !same_binding(&o.original, &current) {
        return false;
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY: the hook's live list.
    if let Some(l) = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) } {
        for (parameter, handle) in &o.bindings {
            // SAFETY: as above.
            unsafe { l.SetGraphicsRootDescriptorTable(*parameter, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: *handle }) };
        }
    }
    true
}
/// ValidateSubmission: the owner, its recording and its rings alive at the queue.
pub fn validate_submission(tables: &Tables, queue: *mut c_void) -> bool {
    let Some(owner) = owner_of(tables.owner.as_ref()) else { return false };
    let o = owner.get();
    let raw = o.recording.raw();
    same_tables(tables, o)
        && tables.recording.same(&o.recording)
        // SAFETY: the owner's lease and the hook's queue.
        && unsafe { crate::native_recording::cvr_native_recording_validate_submission(&raw, queue) }
        && o.ring.as_ref().is_none_or(ring_alive)
        && o.borrowed_rings.iter().all(ring_alive)
}
