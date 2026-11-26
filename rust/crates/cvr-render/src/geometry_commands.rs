//! The stereo geometry commands (src/Render/StereoGeometryCommands.cpp, `StereoGeometryCommands`): paired draws whose two
//! eyes share every IA view but the instance stream (slot 7, 48 or 64 bytes per instance), owned as one compact upload
//! buffer of deduplicated instance data; source draws replaying exact native IA views (indexed direct, or a pure DRAW /
//! DRAW_INDEXED ExecuteIndirect with its signature's receipt); and the capture of a native indirect draw's argument
//! (and count) bytes into owned DEFAULT buffers on the recording, after its copy proofs and their caller validation.
//! Every storage is a COM object the command resources retain through the recording.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's members forward to `cvr_sgc_*` (the inputs lent as mirrors, the command
//! resources' Keep, the signature receipts and the copy validation as callbacks).
//! `tools/single_pass_tests/geometry_commands_test` runs its cases against it in its Rust variant.

use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_ACCESSDENIED, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT, DXGI_SAMPLE_DESC};

use crate::indirect_policy::{draw_kind, pure_draw_stride, Receipt};

pub const MAX_SOURCE_COMMANDS: usize = 2048;
pub const MAX_INDIRECT_SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;

/// native_probe::DrawRecord (1096 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DrawRecord {
    pub sequence: u64,
    pub qpc: u64,
    pub list: u64,
    pub pipeline: u64,
    pub root: u64,
    pub frame: u32,
    pub node: u32,
    pub side: u32,
    pub indices: u32,
    pub instances: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
    pub vertex_mask: u32,
    pub table_mask: u32,
    pub cbv_mask: u32,
    pub flags: u32,
    pub rt_count: u32,
    pub rt_contiguous: u32,
    pub index: D3D12_INDEX_BUFFER_VIEW,
    pub vertices: [D3D12_VERTEX_BUFFER_VIEW; 16],
    pub tables: [u64; 32],
    pub cbvs: [u64; 32],
    pub targets: [u64; 8],
    pub depth: u64,
    pub viewport: D3D12_VIEWPORT,
    pub scissor: windows::Win32::Foundation::RECT,
    pub instance_resource: u64,
    pub instance_gpu_base: u64,
    pub instance_bytes_total: u64,
    pub instance_data_bytes: u32,
    pub instance_heap_type: u32,
    pub instance_data: [u8; 64],
    pub thread: u32,
    pub input_slot_mask: u32,
}
const _: () = assert!(
    core::mem::size_of::<DrawRecord>() == 1096
        && core::mem::offset_of!(DrawRecord, index) == 96
        && core::mem::offset_of!(DrawRecord, viewport) == 952
        && core::mem::offset_of!(DrawRecord, input_slot_mask) == 1092
);
fn bytes_of<T>(value: &T) -> &[u8] {
    // SAFETY: plain data read as bytes.
    unsafe { core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>()) }
}

/// geometry::BufferLease, lent: the held resource and its GPU span.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Lease {
    pub resource: *mut c_void,
    pub gpu: u64,
    pub bytes: u64,
}
/// geometry::DrawResources, lent.
#[repr(C)]
pub struct Resources {
    pub index: Lease,
    pub vertices: [Lease; 16],
    pub draw: *const DrawRecord,
    pub instances: *const u8,
    pub instance_count: usize,
}
/// PureIndirectSource, lent.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IndirectView {
    pub signature: *mut c_void,
    pub max_command_count: u32,
    pub arguments: *mut c_void,
    pub argument_offset: u64,
    pub count_buffer: *mut c_void,
    pub count_offset: u64,
}
/// GeometryCommandPair, lent.
#[repr(C)]
pub struct PairInput {
    pub eyes: [Resources; 2],
    pub instance_rebase_safe: bool,
}
/// A source command, lent (GeometrySourceCommand, or a direct geometry::DrawResources with `indirect` null).
#[repr(C)]
pub struct SourceInput {
    pub geometry: Resources,
    pub indirect: *const IndirectView,
}
/// IndirectCopyProof, lent.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Proof {
    pub resource: *mut c_void,
    pub state_known: bool,
    pub state: D3D12_RESOURCE_STATES,
    pub recording_generation: u64,
    pub command_position: u64,
    pub allocation_owner: *mut c_void,
}
/// IndirectSnapshotReceipt.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotReceipt {
    pub list: u64,
    pub recording_generation: u64,
    pub command_position: u64,
    pub kind: u32,
    pub stride: u32,
    pub source_offsets: [u64; 2],
    pub copied_bytes: [u64; 2],
}
const _: () = assert!(
    core::mem::size_of::<Lease>() == 24
        && core::mem::size_of::<Resources>() == 432
        && core::mem::size_of::<IndirectView>() == 48
        && core::mem::size_of::<PairInput>() == 872
        && core::mem::size_of::<SourceInput>() == 440
        && core::mem::size_of::<Proof>() == 40
        && core::mem::size_of::<SnapshotReceipt>() == 64
);
impl Resources {
    fn draw(&self) -> &DrawRecord {
        // SAFETY: the .cpp's record, alive for the call.
        unsafe { &*self.draw }
    }
    fn instances(&self) -> &[u8] {
        if self.instances.is_null() || self.instance_count == 0 {
            &[]
        } else {
            // SAFETY: the .cpp's instance bytes, alive for the call.
            unsafe { core::slice::from_raw_parts(self.instances, self.instance_count) }
        }
    }
}

// ---- callbacks

/// The command resources' retention of an object on a recording (KeepCommandResources).
pub type Keep = unsafe extern "C-unwind" fn(list: *mut c_void, object: *mut c_void) -> bool;
/// indirect_policy::Describe: the signature's receipt.
pub type Describe = unsafe extern "C-unwind" fn(signature: *mut c_void, receipt: *mut Receipt);
/// The caller's read-only validation of proof `which` (0 arguments, 1 count) for [offset, offset + bytes).
pub type Validate = unsafe extern "C-unwind" fn(context: *mut c_void, list: *mut c_void, which: u32, offset: u64, bytes: u64) -> bool;

fn receipt(describe: Describe, signature: *mut c_void) -> Receipt {
    let mut value = Receipt::default();
    // SAFETY: the .cpp's callback over a live signature.
    unsafe { describe(signature, &mut value) };
    value
}

// ---- the storages

struct Indirect {
    signature: ID3D12CommandSignature,
    max_command_count: u32,
    arguments: ID3D12Resource,
    argument_offset: u64,
    count_buffer: Option<ID3D12Resource>,
    count_offset: u64,
}
struct Command {
    index: D3D12_INDEX_BUFFER_VIEW,
    vertices: [D3D12_VERTEX_BUFFER_VIEW; 16],
    mask: u32,
    indices: u32,
    instances: u32,
    first: u32,
    start: u32,
    base: i32,
    instance_offset: u64,
    index_bound: bool,
    indirect: Option<Indirect>,
    signature_receipt: Receipt,
    _owners: Vec<ID3D12Resource>,
}
pub struct State {
    device: ID3D12Device,
    instances: Option<ID3D12Resource>,
    draws: Vec<Command>,
    sources: Vec<Command>,
    bytes: u64,
}
struct IndirectState {
    _device: ID3D12Device,
    _held: Vec<IUnknown>,
}
#[repr(C)]
struct Object<T> {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    state: T,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn object_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live object (refs at +8).
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *const AtomicU32).byte_add(8)).fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn object_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live object (refs at +8).
    unsafe { (*(this as *const AtomicU32).byte_add(8)).fetch_add(1, SeqCst) + 1 }
}
unsafe fn release<T>(this: *mut c_void) -> u32 {
    // SAFETY: a live object, freed at zero.
    unsafe {
        let n = (*(this as *mut Object<T>)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Object<T>));
        }
        n
    }
}
unsafe extern "system" fn state_release(this: *mut c_void) -> u32 {
    // SAFETY: forwarded.
    unsafe { release::<State>(this) }
}
unsafe extern "system" fn indirect_release(this: *mut c_void) -> u32 {
    // SAFETY: forwarded.
    unsafe { release::<IndirectState>(this) }
}
static STATE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: object_query, AddRef: object_add_ref, Release: state_release };
static INDIRECT_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: object_query, AddRef: object_add_ref, Release: indirect_release };
fn boxed<T>(vtbl: &'static IUnknown_Vtbl, state: T) -> *mut c_void {
    Box::into_raw(Box::new(Object { vtbl, refs: AtomicU32::new(1), state })).cast()
}
fn state<'a>(storage: *mut c_void) -> &'a State {
    // SAFETY: a live State storage of the caller's.
    unsafe { &(*(storage as *mut Object<State>)).state }
}

// ---- helpers

fn device_of(raw: *mut c_void) -> Option<ID3D12Device> {
    // SAFETY: a live device child of the caller's, or null.
    let child = unsafe { ID3D12DeviceChild::from_raw_borrowed(&raw) }?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live object.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}
fn same_device(device: &ID3D12Device, raw: *mut c_void) -> bool {
    device_of(raw).is_some_and(|d| d.as_raw() == device.as_raw())
}
fn resource(raw: *mut c_void) -> Option<ID3D12Resource> {
    // SAFETY: a live resource of the caller's, or null.
    unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.cloned()
}
/// Range: the lease's resource holds [address, address + bytes) of its span.
fn range(device: &ID3D12Device, lease: &Lease, address: u64, bytes: u64) -> bool {
    same_device(device, lease.resource) && bytes != 0 && address >= lease.gpu && address - lease.gpu <= lease.bytes && bytes <= lease.bytes - (address - lease.gpu)
}
/// SourceRange: Range of a lease that is exactly its whole buffer's span.
fn source_range(device: &ID3D12Device, lease: &Lease, address: u64, bytes: u64) -> bool {
    if lease.gpu == 0 || lease.bytes == 0 || lease.bytes > u64::MAX - lease.gpu || address == 0 || bytes > u64::MAX - address || !range(device, lease, address, bytes) {
        return false;
    }
    let r = resource(lease.resource).expect("lease");
    // SAFETY: a live resource.
    let (desc, gpu) = unsafe { (r.GetDesc(), r.GetGPUVirtualAddress()) };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER && lease.gpu == gpu && lease.bytes <= desc.Width
}
fn indirect_range(device: &ID3D12Device, raw: *mut c_void, offset: u64, bytes: u64) -> bool {
    if offset % 4 != 0 || !same_device(device, raw) {
        return false;
    }
    let r = resource(raw).expect("resource");
    // SAFETY: a live resource.
    let desc = unsafe { r.GetDesc() };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER && offset <= desc.Width && bytes <= desc.Width - offset
}
fn copy_proof(device: &ID3D12Device, proof: &Proof, raw: *mut c_void, offset: u64, bytes: u64) -> bool {
    if proof.resource.is_null()
        || proof.resource != raw
        || !proof.state_known
        || proof.recording_generation == 0
        || proof.command_position == 0
        || proof.allocation_owner.is_null()
        || !indirect_range(device, raw, offset, bytes)
    {
        return false;
    }
    let r = resource(raw).expect("resource");
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource.
    if unsafe { r.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err() {
        return false;
    }
    if heap.Type == D3D12_HEAP_TYPE_UPLOAD {
        return proof.state == D3D12_RESOURCE_STATE_GENERIC_READ;
    }
    let reads = D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32;
    let before = proof.state.0 as u32;
    heap.Type == D3D12_HEAP_TYPE_DEFAULT
        && (before & !reads == 0
            || proof.state == D3D12_RESOURCE_STATE_COPY_DEST
            || proof.state == D3D12_RESOURCE_STATE_UNORDERED_ACCESS
            || proof.state == D3D12_RESOURCE_STATE_STREAM_OUT)
}
fn buffer(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, width: u64, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource, HRESULT> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: width,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; plain descriptions.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.map_err(|e| e.code())?;
    out.ok_or(E_FAIL)
}
fn index_bytes(view: &D3D12_INDEX_BUFFER_VIEW) -> u32 {
    if view.Format == DXGI_FORMAT_R16_UINT {
        2
    } else if view.Format == DXGI_FORMAT_R32_UINT {
        4
    } else {
        0
    }
}
/// EqualGeometry: the right (side 1) and left (side 0) records of one frame's same indexed instanced draw.
fn equal_geometry(a: &DrawRecord, b: &DrawRecord) -> bool {
    if a.side != 1
        || b.side != 0
        || a.frame == 0
        || a.frame != b.frame
        || a.indices == 0
        || a.instances == 0
        || a.pipeline != b.pipeline
        || a.root != b.root
        || a.indices != b.indices
        || a.instances != b.instances
        || a.first_index != b.first_index
        || a.base_vertex != b.base_vertex
        || a.input_slot_mask != b.input_slot_mask
        || a.flags & 191 != 191
        || b.flags & 191 != 191
        || (a.flags | b.flags) & 64 != 0
        || a.input_slot_mask & 128 == 0
        || a.input_slot_mask & !65535 != 0
        || bytes_of(&a.index) != bytes_of(&b.index)
        || bytes_of(&a.viewport) != bytes_of(&b.viewport)
        || bytes_of(&a.scissor) != bytes_of(&b.scissor)
    {
        return false;
    }
    (0..16).all(|slot| slot == 7 || a.input_slot_mask & (1 << slot) == 0 || bytes_of(&a.vertices[slot]) == bytes_of(&b.vertices[slot]))
}
fn command(draw: &DrawRecord) -> Command {
    Command {
        index: draw.index,
        vertices: draw.vertices,
        mask: draw.input_slot_mask,
        indices: draw.indices,
        instances: draw.instances,
        first: draw.first_index,
        start: 0,
        base: draw.base_vertex,
        instance_offset: 0,
        index_bound: false,
        indirect: None,
        signature_receipt: Receipt::default(),
        _owners: Vec::new(),
    }
}

// ---- Create

/// Create: the paired draws with their compact instance stream.
///
/// # Safety
/// The lent inputs are valid for the call.
pub unsafe fn create(device_raw: *mut c_void, inputs: &[PairInput]) -> Result<State, HRESULT> {
    // SAFETY: a live device of the caller's, or null.
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device_raw) }.ok_or(E_INVALIDARG)?.clone();
    if inputs.is_empty() || inputs.len() > 2048 {
        return Err(E_INVALIDARG);
    }
    let mut s = State { device: device.clone(), instances: None, draws: Vec::new(), sources: Vec::new(), bytes: 0 };
    let mut data: BTreeMap<(u32, Vec<u8>), u64> = BTreeMap::new();
    for input in inputs {
        let (a, b) = (&input.eyes[0], &input.eyes[1]);
        let draw = a.draw();
        if !input.instance_rebase_safe || !equal_geometry(draw, b.draw()) || a.instances() != b.instances() || a.instances().is_empty() || a.instances().len() > 16 * 1024 {
            return Err(E_INVALIDARG);
        }
        let stride = draw.vertices[7].StrideInBytes;
        if (stride != 48 && stride != 64) || b.draw().vertices[7].StrideInBytes != stride || a.instances().len() as u64 != draw.instances as u64 * stride as u64 {
            return Err(E_INVALIDARG);
        }
        for eye in [a, b] {
            let d = eye.draw();
            let v = &d.vertices[7];
            if (d.first_instance as u64 + d.instances as u64) * stride as u64 > v.SizeInBytes as u64 || !range(&device, &eye.vertices[7], v.BufferLocation, v.SizeInBytes as u64) {
                return Err(E_INVALIDARG);
            }
        }
        let size = index_bytes(&draw.index);
        if size == 0
            || (draw.first_index as u64 + draw.indices as u64) * size as u64 > draw.index.SizeInBytes as u64
            || a.index.resource != b.index.resource
            || !range(&device, &a.index, draw.index.BufferLocation, draw.index.SizeInBytes as u64)
        {
            return Err(E_INVALIDARG);
        }
        let mut c = command(draw);
        c._owners.push(resource(a.index.resource).ok_or(E_INVALIDARG)?);
        for slot in 0..16 {
            if slot == 7 || c.mask & (1 << slot) == 0 {
                continue;
            }
            let view = &draw.vertices[slot];
            if a.vertices[slot].resource != b.vertices[slot].resource || !range(&device, &a.vertices[slot], view.BufferLocation, view.SizeInBytes as u64) {
                return Err(E_INVALIDARG);
            }
            c._owners.push(resource(a.vertices[slot].resource).ok_or(E_INVALIDARG)?);
        }
        let key = (stride, a.instances().to_vec());
        if let Some(&at) = data.get(&key) {
            c.instance_offset = at;
        } else {
            if a.instances().len() as u64 > 32 * 1024 * 1024 - s.bytes {
                return Err(E_OUTOFMEMORY);
            }
            c.instance_offset = s.bytes;
            data.insert(key, s.bytes);
            s.bytes += a.instances().len() as u64;
        }
        c.vertices[7] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0, SizeInBytes: a.instances().len() as u32, StrideInBytes: stride };
        s.draws.push(c);
    }
    let _internal = cvr_host::probe::Internal::new();
    let instances = buffer(&device, D3D12_HEAP_TYPE_UPLOAD, s.bytes, D3D12_RESOURCE_STATE_GENERIC_READ)?;
    // SAFETY (whole block): the new upload buffer of `s.bytes` bytes.
    unsafe {
        let none = D3D12_RANGE::default();
        let mut mapped: *mut c_void = core::ptr::null_mut();
        instances.Map(0, Some(&none), Some(&mut mapped)).map_err(|e| e.code())?;
        if mapped.is_null() {
            instances.Unmap(0, Some(&none));
            return Err(E_FAIL);
        }
        for ((_, bytes), &offset) in &data {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), (mapped as *mut u8).add(offset as usize), bytes.len());
        }
        let written = D3D12_RANGE { Begin: 0, End: s.bytes as usize };
        instances.Unmap(0, Some(&written));
        let base = instances.GetGPUVirtualAddress();
        for draw in &mut s.draws {
            draw.vertices[7].BufferLocation = base + draw.instance_offset;
        }
    }
    s.instances = Some(instances);
    Ok(s)
}

// ---- CreateSources

/// CreateSources / CreateMixedSources: the exact native source draws.
///
/// # Safety
/// The lent inputs are valid for the call.
pub unsafe fn create_sources(device_raw: *mut c_void, inputs: &[SourceInput], describe: Describe) -> Result<State, HRESULT> {
    // SAFETY: a live device of the caller's, or null.
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device_raw) }.ok_or(E_INVALIDARG)?.clone();
    if inputs.is_empty() || inputs.len() > MAX_SOURCE_COMMANDS {
        return Err(E_INVALIDARG);
    }
    let mut s = State { device: device.clone(), instances: None, draws: Vec::new(), sources: Vec::with_capacity(inputs.len()), bytes: 0 };
    for input in inputs {
        let geometry = &input.geometry;
        // SAFETY: the .cpp's indirect view, alive for the call, or null.
        let indirect = unsafe { input.indirect.as_ref() };
        let draw = geometry.draw();
        let mask = draw.input_slot_mask;
        if draw.flags & 128 == 0 || mask & !65535 != 0 || (indirect.is_none() && (draw.indices == 0 || draw.instances == 0 || mask & 128 == 0)) {
            return Err(E_INVALIDARG);
        }
        let mut c = command(draw);
        let mut kind = 0u32;
        if let Some(view) = indirect {
            c.signature_receipt = receipt(describe, view.signature);
            kind = draw_kind(&c.signature_receipt);
            let stride = pure_draw_stride(&c.signature_receipt);
            if kind == 0
                || !same_device(&device, view.signature)
                || view.count_offset % 4 != 0
                || !indirect_range(&device, view.arguments, view.argument_offset, view.max_command_count as u64 * stride as u64)
                || (!view.count_buffer.is_null() && !indirect_range(&device, view.count_buffer, view.count_offset, 4))
            {
                return Err(E_INVALIDARG);
            }
            // SAFETY: a live signature (same_device held).
            let signature = unsafe { ID3D12CommandSignature::from_raw_borrowed(&view.signature) }.ok_or(E_INVALIDARG)?.clone();
            c.indirect = Some(Indirect {
                signature,
                max_command_count: view.max_command_count,
                arguments: resource(view.arguments).ok_or(E_INVALIDARG)?,
                argument_offset: view.argument_offset,
                count_buffer: resource(view.count_buffer),
                count_offset: view.count_offset,
            });
        }
        // Pure DRAW does not consume the current IB; its diagnostic view may still describe a previous indexed draw.
        c.index_bound = indirect.is_none() || kind == 1 || !geometry.index.resource.is_null();
        let size = index_bytes(&draw.index) as u64;
        if c.index_bound
            && (size == 0
                || draw.index.BufferLocation % size.max(1) != 0
                || draw.index.SizeInBytes as u64 % size.max(1) != 0
                || (indirect.is_none() && (draw.first_index as u64 + draw.indices as u64) * size > draw.index.SizeInBytes as u64)
                || !source_range(&device, &geometry.index, draw.index.BufferLocation, draw.index.SizeInBytes as u64))
        {
            return Err(E_INVALIDARG);
        }
        if mask & 128 != 0 {
            let instance = &draw.vertices[7];
            let stride = instance.StrideInBytes;
            if (stride != 48 && stride != 64) || (indirect.is_none() && (draw.first_instance as u64 + draw.instances as u64) * stride as u64 > instance.SizeInBytes as u64) {
                return Err(E_INVALIDARG);
            }
        }
        c.start = draw.first_instance;
        if c.index_bound {
            c._owners.push(resource(geometry.index.resource).ok_or(E_INVALIDARG)?);
        }
        for slot in 0..16 {
            if mask & (1 << slot) == 0 {
                continue;
            }
            let view = &draw.vertices[slot];
            if !source_range(&device, &geometry.vertices[slot], view.BufferLocation, view.SizeInBytes as u64) {
                return Err(E_INVALIDARG);
            }
            c._owners.push(resource(geometry.vertices[slot].resource).ok_or(E_INVALIDARG)?);
        }
        s.sources.push(c);
    }
    Ok(s)
}

// ---- recording

fn record(storage: *mut c_void, list_raw: *mut c_void, index: u32, sources: bool, keep: Keep, describe: Describe) -> bool {
    let s = state(storage);
    // SAFETY: a live command list of the caller's, or null.
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list_raw) }) else {
        return false;
    };
    let commands = if sources { &s.sources } else { &s.draws };
    // SAFETY (whole block): a live list and the storage's held objects.
    unsafe {
        if index as usize >= commands.len() || list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(&s.device, list_raw) {
            return false;
        }
        let draw = &commands[index as usize];
        if let Some(indirect) = &draw.indirect {
            let current = receipt(describe, indirect.signature.as_raw());
            if current != draw.signature_receipt || draw_kind(&current) == 0 {
                return false;
            }
        }
        if !keep(list_raw, storage) {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        if !sources || draw.index_bound {
            list.IASetIndexBuffer(Some(&draw.index));
        }
        for slot in 0..16u32 {
            if draw.mask & (1 << slot) != 0 {
                list.IASetVertexBuffers(slot, Some(&[draw.vertices[slot as usize]]));
            }
        }
        match (&draw.indirect, sources) {
            (Some(indirect), true) => list.ExecuteIndirect(
                &indirect.signature,
                indirect.max_command_count,
                &indirect.arguments,
                indirect.argument_offset,
                indirect.count_buffer.as_ref(),
                indirect.count_offset,
            ),
            (_, true) => list.DrawIndexedInstanced(draw.indices, draw.instances, draw.first, draw.base, draw.start),
            (_, false) => list.DrawIndexedInstanced(draw.indices, draw.instances, draw.first, draw.base, 0),
        }
    }
    true
}

// ---- CaptureIndirectSource

/// The captured replay: the owned argument (and count) buffers, the receipt and the owner (references for the caller).
#[repr(C)]
pub struct Captured {
    pub arguments: *mut c_void,
    pub count_buffer: *mut c_void,
    pub receipt: SnapshotReceipt,
    pub owner: *mut c_void,
}
fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}
/// CaptureIndirectSource: the native indirect draw's argument (and count) bytes copied into owned DEFAULT buffers.
///
/// # Safety
/// The lent views and proofs are valid for the call; the callbacks accept their context.
#[allow(clippy::too_many_arguments)]
pub unsafe fn capture_indirect(
    list_raw: *mut c_void,
    source: &IndirectView,
    arguments: &Proof,
    count: Option<&Proof>,
    describe: Describe,
    validate: Validate,
    context: *mut c_void,
    keep: Keep,
) -> Result<Captured, HRESULT> {
    // SAFETY: a live command list of the caller's, or null.
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list_raw) }.ok_or(E_INVALIDARG)?;
    // SAFETY: a live list.
    if unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT || source.count_buffer.is_null() != count.is_none() || source.count_offset % 4 != 0 {
        return Err(E_INVALIDARG);
    }
    let signature = receipt(describe, source.signature);
    let (kind, stride) = (draw_kind(&signature), pure_draw_stride(&signature));
    let bytes = source.max_command_count as u64 * stride as u64;
    let count_bytes = if count.is_some() { 4u64 } else { 0 };
    if kind == 0 || bytes > MAX_INDIRECT_SNAPSHOT_BYTES - count_bytes {
        return Err(E_INVALIDARG);
    }
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list.
    unsafe { list.GetDevice(&mut device) }.map_err(|e| e.code())?;
    let device = device.ok_or(E_FAIL)?;
    if !same_device(&device, source.signature)
        || !copy_proof(&device, arguments, source.arguments, source.argument_offset, bytes)
        || count.is_some_and(|c| {
            !copy_proof(&device, c, source.count_buffer, source.count_offset, 4)
                || c.recording_generation != arguments.recording_generation
                || c.command_position != arguments.command_position
                || (c.resource == arguments.resource && (c.state_known != arguments.state_known || c.state != arguments.state))
        })
    {
        return Err(E_INVALIDARG);
    }
    let validated = |offset: u64, length: u64, which: u32| unsafe { validate(context, list_raw, which, offset, length) };
    if !validated(source.argument_offset, bytes, 0) || (count.is_some() && !validated(source.count_offset, 4, 1)) {
        return Err(E_ACCESSDENIED);
    }
    let (replay_arguments, replay_count) = {
        let _internal = cvr_host::probe::Internal::new();
        let arguments = buffer(&device, D3D12_HEAP_TYPE_DEFAULT, bytes.max(stride as u64).max(4), D3D12_RESOURCE_STATE_COPY_DEST)?;
        let count_buffer = if count.is_some() { Some(buffer(&device, D3D12_HEAP_TYPE_DEFAULT, 4, D3D12_RESOURCE_STATE_COPY_DEST)?) } else { None };
        (arguments, count_buffer)
    };
    let mut held: Vec<IUnknown> = Vec::new();
    for raw in [source.signature, source.arguments, source.count_buffer, arguments.resource, arguments.allocation_owner] {
        // SAFETY: live objects of the caller's, or null.
        if let Some(object) = unsafe { IUnknown::from_raw_borrowed(&raw) } {
            held.push(object.clone());
        }
    }
    if let Some(c) = count {
        for raw in [c.resource, c.allocation_owner] {
            // SAFETY: live objects of the caller's, or null.
            if let Some(object) = unsafe { IUnknown::from_raw_borrowed(&raw) } {
                held.push(object.clone());
            }
        }
    }
    held.push(replay_arguments.cast().map_err(|e| e.code())?);
    if let Some(c) = &replay_count {
        held.push(c.cast().map_err(|e| e.code())?);
    }
    let owner = boxed(&INDIRECT_VTBL, IndirectState { _device: device.clone(), _held: held });
    // The owner's creation reference: the caller's on success, released on any failure below.
    struct Release(*mut c_void);
    impl Drop for Release {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the creation reference this holds.
                unsafe { indirect_release(self.0) };
            }
        }
    }
    let mut guard = Release(owner);
    let snapshot = SnapshotReceipt {
        list: list_raw as u64,
        recording_generation: arguments.recording_generation,
        command_position: arguments.command_position,
        kind,
        stride,
        source_offsets: [source.argument_offset, source.count_offset],
        copied_bytes: [bytes, count_bytes],
    };
    let mut sources: Vec<(ID3D12Resource, D3D12_RESOURCE_STATES, D3D12_RESOURCE_STATES)> = Vec::new();
    let mut add_source = |proof: &Proof| {
        if proof.state.0 as u32 & D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32 != 0 || sources.iter().any(|(r, _, _)| r.as_raw() == proof.resource) {
            return;
        }
        let before = proof.state.0 as u32;
        let after = if before & !(D3D12_RESOURCE_STATE_GENERIC_READ.0 as u32) != 0 {
            D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32
        } else {
            before | D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32
        };
        if let Some(r) = resource(proof.resource) {
            sources.push((r, proof.state, D3D12_RESOURCE_STATES(after as i32)));
        }
    };
    if bytes != 0 {
        add_source(arguments);
    }
    if let Some(c) = count {
        add_source(c);
    }
    let mut destinations = vec![transition(&replay_arguments, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT)];
    if let Some(c) = &replay_count {
        destinations.push(transition(c, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT));
    }
    // SAFETY: the .cpp's callback over a live list and the owner.
    if !unsafe { keep(list_raw, owner) } {
        return Err(E_OUTOFMEMORY);
    }
    let current = receipt(describe, source.signature);
    if current != signature || !validated(source.argument_offset, bytes, 0) || (count.is_some() && !validated(source.count_offset, 4, 1)) {
        return Err(E_ACCESSDENIED);
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY (whole block): a live list and held resources.
    unsafe {
        let to_copy: Vec<_> = sources.iter().map(|(r, before, after)| transition(r, *before, *after)).collect();
        if !to_copy.is_empty() {
            list.ResourceBarrier(&to_copy);
        }
        if bytes != 0 {
            list.CopyBufferRegion(&replay_arguments, 0, &resource(source.arguments).ok_or(E_INVALIDARG)?, source.argument_offset, bytes);
        }
        if let (Some(c), Some(target)) = (count, &replay_count) {
            let _ = c;
            list.CopyBufferRegion(target, 0, &resource(source.count_buffer).ok_or(E_INVALIDARG)?, source.count_offset, 4);
        }
        let back: Vec<_> = sources.iter().map(|(r, before, after)| transition(r, *after, *before)).collect();
        if !back.is_empty() {
            list.ResourceBarrier(&back);
        }
        list.ResourceBarrier(&destinations);
    }
    guard.0 = core::ptr::null_mut();
    Ok(Captured {
        arguments: replay_arguments.into_raw(),
        count_buffer: replay_count.map_or(core::ptr::null_mut(), |c| c.into_raw()),
        receipt: snapshot,
        owner,
    })
}

// ---- the C ABI

fn finish(result: std::thread::Result<Result<State, HRESULT>>, out: *mut *mut c_void) -> HRESULT {
    match result {
        Ok(Ok(state)) => {
            // SAFETY: the caller's out-pointer.
            unsafe { *out = boxed(&STATE_VTBL, state) };
            S_OK
        }
        Ok(Err(hr)) => hr,
        Err(_) => E_OUTOFMEMORY,
    }
}
/// # Safety
/// The lent inputs are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgc_create(device: *mut c_void, inputs: *const PairInput, count: usize, out: *mut *mut c_void) -> HRESULT {
    let inputs = if inputs.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(inputs, count) } };
    finish(std::panic::catch_unwind(|| unsafe { create(device, inputs) }), out)
}
/// # Safety
/// The lent inputs are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgc_create_sources(device: *mut c_void, inputs: *const SourceInput, count: usize, describe: Describe, out: *mut *mut c_void) -> HRESULT {
    let inputs = if inputs.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(inputs, count) } };
    finish(std::panic::catch_unwind(|| unsafe { create_sources(device, inputs, describe) }), out)
}
/// RecordGeometry (`sources` false) / RecordSourceGeometry.
///
/// # Safety
/// `storage` is a live storage; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgc_record(storage: *mut c_void, list: *mut c_void, index: u32, sources: bool, keep: Keep, describe: Describe) -> bool {
    record(storage, list, index, sources, keep, describe)
}
/// The counts: draws, sources and the instance bytes.
///
/// # Safety
/// `storage` is a live storage; the outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgc_counts(storage: *mut c_void, draws: *mut u32, sources: *mut u32, bytes: *mut u64) {
    let s = state(storage);
    unsafe {
        *draws = s.draws.len() as u32;
        *sources = s.sources.len() as u32;
        *bytes = s.bytes;
    }
}
/// # Safety
/// See capture_indirect; `count` is valid or null; `out` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sgc_capture_indirect(
    list: *mut c_void,
    source: *const IndirectView,
    arguments: *const Proof,
    count: *const Proof,
    describe: Describe,
    validate: Validate,
    context: *mut c_void,
    keep: Keep,
    out: *mut Captured,
) -> HRESULT {
    match std::panic::catch_unwind(|| unsafe { capture_indirect(list, &*source, &*arguments, count.as_ref(), describe, validate, context, keep) }) {
        Ok(Ok(captured)) => {
            unsafe { *out = captured };
            S_OK
        }
        Ok(Err(hr)) => hr,
        Err(_) => E_FAIL,
    }
}
