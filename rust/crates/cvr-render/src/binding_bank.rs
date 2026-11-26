//! The stereo binding bank (src/Render/StereoBindingBank.cpp, `StereoBindingBank`): one shader-visible descriptor heap
//! (and a sampler heap when needed) per pass holding, for every draw, the paired roots' added CBV/SRV tables (each
//! binding's two eye slots), the shared CPU tables copied once per identical range, per-eye snapshots of native SRV
//! sources (copied on the recording at CaptureEye) and an upload buffer of deduplicated CBV constants; a source pass
//! instead copies the original roots' finite / admitted tables once per range (legacy identity or storage scope labels
//! deduplicate them) with its rejection's stage, input, parameter and reason; an arena appends immutable per-draw CBV
//! constants for one paired root. Binds set the heaps, the root and every table of a draw.
//!
//! The storage is a COM object (an IUnknown the command resources retain through the recording) owned by the .cpp's
//! ComPtr<Storage>; in the Rust build (`CVR_RUST_VRIK`) every member forwards to `cvr_bank_*` (the inputs lent as
//! mirrors, the command resources' Keep / Has as callbacks). `tools/single_pass_tests/binding_bank_test` and
//! `source_binding_assembly_test` run their cases against it in their Rust variants.

use core::ffi::c_void;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

use crate::bank_layout::{cbv_registers, component_mapping, read_source_layout, table_fits, SourceLayout, SourcePassFailure};
use crate::paired_root::{Binding, Location, Plan};

const DESCRIPTOR_LIMIT: u32 = 1_000_000;
const CONSTANT_LIMIT: u64 = 16 * 1024 * 1024;
const SNAPSHOT_LIMIT: u64 = 256 * 1024 * 1024;
pub const MAX_ARENA_DRAWS: u32 = 1024;

// ---- the lent inputs

/// PairedRootPlan, lent.
#[repr(C)]
pub struct PlanView {
    pub serialized: *const u8,
    pub serialized_len: usize,
    pub bindings: *const Location,
    pub binding_count: usize,
    pub parameters: [u32; 2],
    pub descriptors: [u32; 2],
    pub original_parameters: u32,
    pub root_dwords: u32,
}
impl PlanView {
    fn serialized(&self) -> &[u8] {
        if self.serialized.is_null() || self.serialized_len == 0 {
            &[]
        } else {
            // SAFETY: the .cpp's plan bytes, alive for the call.
            unsafe { core::slice::from_raw_parts(self.serialized, self.serialized_len) }
        }
    }
    fn bindings(&self) -> &[Location] {
        if self.bindings.is_null() || self.binding_count == 0 {
            &[]
        } else {
            // SAFETY: the .cpp's plan bindings, alive for the call.
            unsafe { core::slice::from_raw_parts(self.bindings, self.binding_count) }
        }
    }
    fn to_plan(&self) -> Plan {
        Plan {
            serialized: self.serialized().to_vec(),
            bindings: self.bindings().to_vec(),
            parameters: self.parameters,
            descriptors: self.descriptors,
            original_parameters: self.original_parameters,
            root_dwords: self.root_dwords,
        }
    }
}
/// BankValue, lent.
#[repr(C)]
pub struct Value {
    pub constants: *const u8,
    pub constants_len: usize,
    pub source: *mut c_void,
    pub srv: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub state: D3D12_RESOURCE_STATES,
    pub gpu_cbv: D3D12_CONSTANT_BUFFER_VIEW_DESC,
    pub owner: *mut c_void,
    pub borrowed_srv: bool,
    pub implicit_srv: bool,
}
impl Value {
    fn constants(&self) -> &[u8] {
        if self.constants.is_null() || self.constants_len == 0 {
            &[]
        } else {
            // SAFETY: the caller's constant bytes, alive for the call.
            unsafe { core::slice::from_raw_parts(self.constants, self.constants_len) }
        }
    }
}
/// BankPair, lent.
#[repr(C)]
pub struct Pair {
    pub binding: Binding,
    pub eyes: [Value; 2],
}
/// SharedBankTable, lent (its leases as raw pointers).
#[repr(C)]
pub struct Table {
    pub parameter: u32,
    pub offset: u32,
    pub count: u32,
    pub cpu_heap: *mut c_void,
    pub leases: *const *mut c_void,
    pub lease_count: usize,
    pub identity: u64,
    pub version: u64,
    pub storage_scope: u64,
    pub storage_extent: u32,
}
impl Table {
    fn leases(&self) -> &[*mut c_void] {
        if self.leases.is_null() || self.lease_count == 0 {
            &[]
        } else {
            // SAFETY: the .cpp's lease pointers, alive for the call.
            unsafe { core::slice::from_raw_parts(self.leases, self.lease_count) }
        }
    }
}
/// BankDraw, lent.
#[repr(C)]
pub struct DrawInput {
    pub root: *mut c_void,
    pub plan: *const PlanView,
    pub pairs: *const Pair,
    pub pair_count: usize,
    pub shared: *const Table,
    pub shared_count: usize,
    pub source_root: *mut c_void,
}
/// SourceBankDraw, lent.
#[repr(C)]
pub struct SourceInput {
    pub root: *mut c_void,
    pub serialized: *const u8,
    pub serialized_len: usize,
    pub tables: *const Table,
    pub table_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<PlanView>() == 56
        && core::mem::size_of::<Value>() == 104
        && core::mem::size_of::<Pair>() == 224
        && core::mem::size_of::<Table>() == 72
        && core::mem::size_of::<DrawInput>() == 56
        && core::mem::size_of::<SourceInput>() == 40
);
fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: the .cpp's lent array, alive for the call.
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}

// ---- the storage

struct Copy {
    source: ID3D12Resource,
    owned: ID3D12Resource,
    state: D3D12_RESOURCE_STATES,
    eye: u32,
}
#[derive(Default)]
struct Draw {
    root: Option<ID3D12RootSignature>,
    source_root: Option<ID3D12RootSignature>,
    tables: Vec<(u32, u32)>,
    sampler_tables: Vec<(u32, u32)>,
    source_tables: [Vec<(u32, u32)>; 2],
}
pub struct State {
    device: ID3D12Device,
    draws: Vec<Draw>,
    source_layouts: Vec<Vec<u8>>,
    source_pass: bool,
    heap: Option<ID3D12DescriptorHeap>,
    samplers: Option<ID3D12DescriptorHeap>,
    constants: Option<ID3D12Resource>,
    copies: Vec<Copy>,
    leases: Vec<IUnknown>,
    recording: usize,
    descriptors: u32,
    stride: u32,
    sampler_stride: u32,
    captured_mask: u32,
    constant_bytes: u64,
    arena_root: Option<ID3D12RootSignature>,
    arena_plan: Plan,
    mapped: *mut c_void,
    max_draws: u32,
    per_draw_descriptors: u32,
    capacity: u64,
}
impl State {
    fn new(device: ID3D12Device) -> Self {
        State {
            device,
            draws: Vec::new(),
            source_layouts: Vec::new(),
            source_pass: false,
            heap: None,
            samplers: None,
            constants: None,
            copies: Vec::new(),
            leases: Vec::new(),
            recording: 0,
            descriptors: 0,
            stride: 0,
            sampler_stride: 0,
            captured_mask: 0,
            constant_bytes: 0,
            arena_root: None,
            arena_plan: Plan::default(),
            mapped: core::ptr::null_mut(),
            max_draws: 0,
            per_draw_descriptors: 0,
            capacity: 0,
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if !self.mapped.is_null() {
            if let Some(c) = &self.constants {
                let written = D3D12_RANGE { Begin: 0, End: self.capacity as usize };
                // SAFETY: the arena's own persistent mapping.
                unsafe { c.Unmap(0, Some(&written)) };
            }
        }
    }
}
/// The storage: an IUnknown whose last release frees it.
#[repr(C)]
pub struct Storage {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    state: State,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn storage_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live storage.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *mut Storage)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn storage_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live storage.
    unsafe { (*(this as *mut Storage)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn storage_release(this: *mut c_void) -> u32 {
    // SAFETY: a live storage, freed at zero.
    unsafe {
        let n = (*(this as *mut Storage)).refs.fetch_sub(1, SeqCst) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Storage));
        }
        n
    }
}
static STORAGE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: storage_query, AddRef: storage_add_ref, Release: storage_release };
fn boxed(state: State) -> *mut Storage {
    Box::into_raw(Box::new(Storage { vtbl: &STORAGE_VTBL, refs: AtomicU32::new(1), state }))
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
fn borrowed<T: Interface>(raw: &*mut c_void) -> Option<&T> {
    // SAFETY: a live object of the caller's of that interface, or null.
    unsafe { T::from_raw_borrowed(raw) }
}
fn held<T: Interface + Clone>(raw: *mut c_void) -> Option<T> {
    borrowed::<T>(&raw).cloned()
}
fn unknown(raw: *mut c_void) -> Option<IUnknown> {
    held::<IUnknown>(raw)
}
/// The decoded version 1.1 description of a plan's serialized root (alive with the deserializer).
struct Decoded {
    _reader: ID3D12VersionedRootSignatureDeserializer,
    desc: *const D3D12_VERSIONED_ROOT_SIGNATURE_DESC,
}
impl Decoded {
    fn root(&self) -> &D3D12_ROOT_SIGNATURE_DESC1 {
        // SAFETY: a 1.1 description, as asked for; alive with the deserializer.
        unsafe { &(*self.desc).Anonymous.Desc_1_1 }
    }
    fn parameter(&self, index: u32) -> &D3D12_ROOT_PARAMETER1 {
        // SAFETY: an index the caller checked against NumParameters.
        unsafe { &*self.root().pParameters.add(index as usize) }
    }
}
fn decode(bytes: &[u8]) -> Result<Decoded, HRESULT> {
    let mut raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: the plan's bytes; an out-interface.
    unsafe {
        D3D12CreateVersionedRootSignatureDeserializer(bytes.as_ptr() as *const c_void, bytes.len(), &ID3D12VersionedRootSignatureDeserializer::IID, &mut raw)
            .map_err(|e| e.code())?;
    }
    if raw.is_null() {
        return Err(E_FAIL);
    }
    // SAFETY: the reference the creation added.
    let reader = unsafe { ID3D12VersionedRootSignatureDeserializer::from_raw(raw) };
    // SAFETY: a live deserializer.
    let desc = unsafe { reader.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) }.map_err(|e| e.code())?;
    Ok(Decoded { _reader: reader, desc })
}
fn heap(device: &ID3D12Device, kind: D3D12_DESCRIPTOR_HEAP_TYPE, count: u32) -> Result<ID3D12DescriptorHeap, HRESULT> {
    let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: kind, NumDescriptors: count, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, NodeMask: 0 };
    // SAFETY: a live device.
    unsafe { device.CreateDescriptorHeap(&desc) }.map_err(|e| e.code())
}
fn upload_buffer(device: &ID3D12Device, bytes: u64) -> Result<ID3D12Resource, HRESULT> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; plain descriptions.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut out) }
        .map_err(|e| e.code())?;
    out.ok_or(E_FAIL)
}
fn cpu(start: D3D12_CPU_DESCRIPTOR_HANDLE, at: u32, stride: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + at as usize * stride as usize }
}
struct Prepared {
    value: *const Value,
    slot: u32,
    constant_offset: u64,
    snapshot: *mut c_void,
    srv: bool,
}
fn constant_size(len: usize) -> u64 {
    (len as u64 + 255) & !255
}

// ---- CreatePass

/// CreatePass: the pass storage, or the failure.
///
/// # Safety
/// The lent inputs are valid for the call.
pub unsafe fn create_pass(device_raw: *mut c_void, draws: &[DrawInput]) -> Result<State, HRESULT> {
    let device: ID3D12Device = held(device_raw).ok_or(E_INVALIDARG)?;
    if draws.is_empty() || draws.len() > 32768 {
        return Err(E_INVALIDARG);
    }
    let _internal = cvr_host::probe::Internal::new();
    let mut s = State::new(device.clone());
    // SAFETY: a live device.
    s.stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
    if s.stride == 0 {
        return Err(E_INVALIDARG);
    }
    let (mut cursor, mut sampler_cursor, mut snapshot_bytes) = (0u32, 0u32, 0u64);
    let mut shared_offsets: BTreeMap<(usize, u32, u32), u32> = BTreeMap::new();
    let mut snapshots: BTreeMap<(u32, usize), usize> = BTreeMap::new();
    let mut constant_offsets: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    let mut leased: BTreeSet<usize> = BTreeSet::new();
    let mut values: Vec<Prepared> = Vec::new();
    let mut null_cbvs: Vec<u32> = Vec::new();
    for input in draws {
        if input.plan.is_null() || !same_device(&device, input.root) {
            return Err(E_INVALIDARG);
        }
        // SAFETY: the .cpp's plan, alive for the call.
        let plan = unsafe { &*input.plan };
        let pairs = slice(input.pairs, input.pair_count);
        let shared = slice(input.shared, input.shared_count);
        if pairs.len() != plan.bindings().len() || pairs.len() > 512 || shared.len() > 64 {
            return Err(E_INVALIDARG);
        }
        let mut draw = Draw { root: held(input.root), source_root: held(input.source_root), ..Draw::default() };
        if !input.source_root.is_null() && (!same_device(&device, input.source_root) || !shared.is_empty()) {
            return Err(E_INVALIDARG);
        }
        let bytes = plan.serialized();
        if bytes.is_empty() || bytes.len() > 1024 * 1024 {
            return Err(E_INVALIDARG);
        }
        let decoded = decode(bytes).map_err(|_| E_INVALIDARG)?;
        let layout = decoded.root();
        if layout.NumParameters > 64 || plan.original_parameters > layout.NumParameters {
            return Err(E_INVALIDARG);
        }
        // Only descriptor-table shared state is copied: root CBVs / constants the transformed shaders still read need
        // explicit materialization.
        let mut parameters: BTreeSet<u32> = BTreeSet::new();
        for table in shared {
            if table.cpu_heap.is_null()
                || table.count == 0
                || table.parameter >= plan.original_parameters
                || !parameters.insert(table.parameter)
                || !same_device(&device, table.cpu_heap)
            {
                return Err(E_INVALIDARG);
            }
            let cpu_heap = borrowed::<ID3D12DescriptorHeap>(&table.cpu_heap).ok_or(E_INVALIDARG)?;
            // SAFETY: a live heap.
            let d = unsafe { cpu_heap.GetDesc() };
            if (d.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && d.Type != D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER)
                || d.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE
                || table.offset > d.NumDescriptors
                || table.count > d.NumDescriptors - table.offset
            {
                return Err(E_INVALIDARG);
            }
            let sampler = d.Type == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
            if !table_fits(layout, table.parameter, table.count, sampler) {
                return Err(E_INVALIDARG);
            }
            if !sampler && table.leases().is_empty() {
                return Err(E_INVALIDARG);
            }
            let limit = if sampler { 2048 } else { DESCRIPTOR_LIMIT };
            let next = if sampler { &mut sampler_cursor } else { &mut cursor };
            let key = (table.cpu_heap as usize, table.offset, table.count);
            let offset = match shared_offsets.get(&key) {
                Some(&at) => at,
                None => {
                    if table.count > limit - *next {
                        return Err(E_INVALIDARG);
                    }
                    let at = *next;
                    *next += table.count;
                    shared_offsets.insert(key, at);
                    at
                }
            };
            for &lease in table.leases() {
                if lease.is_null() {
                    return Err(E_INVALIDARG);
                }
                if leased.insert(lease as usize) {
                    s.leases.push(unknown(lease).ok_or(E_INVALIDARG)?);
                }
            }
            if sampler { &mut draw.sampler_tables } else { &mut draw.tables }.push((table.parameter, offset));
        }
        let mut base = [0u32; 2];
        for stage in 0..2 {
            base[stage] = cursor;
            if plan.descriptors[stage] != 0 {
                if !table_fits(layout, plan.parameters[stage], plan.descriptors[stage], false)
                    || !parameters.insert(plan.parameters[stage])
                    || plan.descriptors[stage] > DESCRIPTOR_LIMIT - cursor
                {
                    return Err(E_INVALIDARG);
                }
                draw.tables.push((plan.parameters[stage], cursor));
                cursor += plan.descriptors[stage];
            }
        }
        let mut used = vec![false; pairs.len()];
        let mut private_slots: BTreeSet<u32> = BTreeSet::new();
        #[derive(Default)]
        struct SourceTable {
            registers: Vec<u32>,
            base: [u32; 2],
            used: BTreeSet<u32>,
        }
        let mut source_tables: BTreeMap<u32, SourceTable> = BTreeMap::new();
        for binding in plan.bindings() {
            let Some(index) = pairs.iter().position(|p| p.binding == binding.binding) else {
                return Err(E_INVALIDARG);
            };
            if used[index] {
                return Err(E_INVALIDARG);
            }
            used[index] = true;
            let stage = if binding.binding.visibility == D3D12_SHADER_VISIBILITY_VERTEX { 0 } else { 1 };
            if binding.destination_parameter != plan.parameters[stage]
                || binding.destination_offset > plan.descriptors[stage]
                || plan.descriptors[stage] - binding.destination_offset < 2
            {
                return Err(E_INVALIDARG);
            }
            if !input.source_root.is_null() {
                if binding.binding.kind != D3D12_DESCRIPTOR_RANGE_TYPE_CBV
                    || binding.source_type != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE
                    || binding.source_parameter >= plan.original_parameters
                {
                    return Err(E_INVALIDARG);
                }
                let p = decoded.parameter(binding.source_parameter);
                if p.ShaderVisibility != binding.binding.visibility && p.ShaderVisibility != D3D12_SHADER_VISIBILITY_ALL {
                    return Err(E_INVALIDARG);
                }
                let inserted = !source_tables.contains_key(&binding.source_parameter);
                let table = source_tables.entry(binding.source_parameter).or_default();
                if inserted {
                    if !cbv_registers(p, &mut table.registers) {
                        return Err(E_INVALIDARG);
                    }
                    for eye in 0..2 {
                        if table.registers.len() as u64 > (DESCRIPTOR_LIMIT - cursor) as u64 {
                            return Err(E_INVALIDARG);
                        }
                        table.base[eye] = cursor;
                        draw.source_tables[eye].push((binding.source_parameter, cursor));
                        for n in 0..table.registers.len() as u32 {
                            null_cbvs.push(cursor + n);
                        }
                        cursor += table.registers.len() as u32;
                    }
                }
                if binding.source_offset as usize >= table.registers.len()
                    || table.registers[binding.source_offset as usize] != binding.binding.source_register
                    || !table.used.insert(binding.source_offset)
                {
                    return Err(E_INVALIDARG);
                }
            }
            for eye in 0..2u32 {
                let value = &pairs[index].eyes[eye as usize];
                let mut entry = Prepared {
                    value,
                    slot: base[stage] + binding.destination_offset + eye,
                    constant_offset: 0,
                    snapshot: core::ptr::null_mut(),
                    srv: false,
                };
                if !private_slots.insert(entry.slot) {
                    return Err(E_INVALIDARG);
                }
                if binding.binding.kind == D3D12_DESCRIPTOR_RANGE_TYPE_CBV {
                    if value.borrowed_srv || value.implicit_srv {
                        return Err(E_INVALIDARG);
                    }
                    if !value.source.is_null() {
                        if !value.constants().is_empty() || value.owner.is_null() || !same_device(&device, value.source) {
                            return Err(E_INVALIDARG);
                        }
                        let source = borrowed::<ID3D12Resource>(&value.source).ok_or(E_INVALIDARG)?;
                        // SAFETY: a live resource.
                        let (desc, gpu) = unsafe { (source.GetDesc(), source.GetGPUVirtualAddress()) };
                        let cbv = &value.gpu_cbv;
                        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
                            || gpu == 0
                            || gpu > u64::MAX - desc.Width
                            || cbv.BufferLocation == 0
                            || cbv.BufferLocation & 255 != 0
                            || cbv.SizeInBytes == 0
                            || cbv.SizeInBytes > 65536
                            || cbv.SizeInBytes & 255 != 0
                            || cbv.BufferLocation < gpu
                            || cbv.BufferLocation - gpu > desc.Width
                            || cbv.SizeInBytes as u64 > desc.Width - (cbv.BufferLocation - gpu)
                        {
                            return Err(E_INVALIDARG);
                        }
                        s.leases.push(unknown(value.source).ok_or(E_INVALIDARG)?);
                        s.leases.push(unknown(value.owner).ok_or(E_INVALIDARG)?);
                    } else {
                        let constants = value.constants();
                        if value.gpu_cbv.BufferLocation != 0 || value.gpu_cbv.SizeInBytes != 0 || !value.owner.is_null() || constants.is_empty() || constants.len() > 65536 {
                            return Err(E_INVALIDARG);
                        }
                        let size = constant_size(constants.len());
                        match constant_offsets.get(constants) {
                            Some(&at) => entry.constant_offset = at,
                            None => {
                                if size > CONSTANT_LIMIT - s.constant_bytes {
                                    return Err(E_INVALIDARG);
                                }
                                entry.constant_offset = s.constant_bytes;
                                s.constant_bytes += size;
                                constant_offsets.insert(constants.to_vec(), entry.constant_offset);
                            }
                        }
                    }
                } else if binding.binding.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SRV {
                    entry.srv = true;
                    if !value.constants().is_empty()
                        || value.gpu_cbv.BufferLocation != 0
                        || value.gpu_cbv.SizeInBytes != 0
                        || (!value.source.is_null() && !same_device(&device, value.source))
                    {
                        return Err(E_INVALIDARG);
                    }
                    if value.implicit_srv {
                        if !value.borrowed_srv || value.source.is_null() {
                            return Err(E_INVALIDARG);
                        }
                    } else if value.srv.ViewDimension.0 < D3D12_SRV_DIMENSION_BUFFER.0
                        || value.srv.ViewDimension.0 > D3D12_SRV_DIMENSION_TEXTURECUBEARRAY.0
                        || !component_mapping(value.srv.Shader4ComponentMapping)
                    {
                        return Err(E_INVALIDARG);
                    }
                    if value.borrowed_srv {
                        let source = borrowed::<ID3D12Resource>(&value.source);
                        // SAFETY: a live resource.
                        let denied = source.is_none_or(|r| (unsafe { r.GetDesc() }.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE).0 != 0);
                        if value.source.is_null() || value.owner.is_null() || denied {
                            return Err(E_INVALIDARG);
                        }
                        entry.snapshot = value.source;
                        s.leases.push(unknown(value.source).ok_or(E_INVALIDARG)?);
                        s.leases.push(unknown(value.owner).ok_or(E_INVALIDARG)?);
                        values.push(entry);
                        continue;
                    }
                    if !value.owner.is_null() {
                        return Err(E_INVALIDARG);
                    }
                    if value.source.is_null() {
                        // An explicit typed null descriptor.
                        values.push(entry);
                        continue;
                    }
                    let key = (eye, value.source as usize);
                    if let Some(&found) = snapshots.get(&key) {
                        let existing = &s.copies[found];
                        if existing.state != value.state {
                            return Err(E_INVALIDARG);
                        }
                        entry.snapshot = existing.owned.as_raw();
                    } else {
                        let source = borrowed::<ID3D12Resource>(&value.source).ok_or(E_INVALIDARG)?;
                        // SAFETY (whole block): live objects.
                        unsafe {
                            let d = source.GetDesc();
                            let mut heap = D3D12_HEAP_PROPERTIES::default();
                            let mut flags = D3D12_HEAP_FLAGS::default();
                            if d.Dimension == D3D12_RESOURCE_DIMENSION_UNKNOWN
                                || (d.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE).0 != 0
                                || source.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_err()
                                || heap.Type != D3D12_HEAP_TYPE_DEFAULT
                            {
                                return Err(E_INVALIDARG);
                            }
                            let allocation = device.GetResourceAllocationInfo(0, &[d]);
                            if allocation.SizeInBytes == u64::MAX || allocation.SizeInBytes > SNAPSHOT_LIMIT - snapshot_bytes {
                                return Err(E_OUTOFMEMORY);
                            }
                            snapshot_bytes += allocation.SizeInBytes;
                            let properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
                            let mut owned: Option<ID3D12Resource> = None;
                            device
                                .CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut owned)
                                .map_err(|e| e.code())?;
                            let owned = owned.ok_or(E_FAIL)?;
                            entry.snapshot = owned.as_raw();
                            snapshots.insert(key, s.copies.len());
                            s.copies.push(Copy { source: source.clone(), owned, state: value.state, eye });
                        }
                    }
                } else {
                    return Err(E_INVALIDARG);
                }
                let again = (!input.source_root.is_null()).then(|| Prepared {
                    slot: source_tables[&binding.source_parameter].base[eye as usize] + binding.source_offset,
                    ..entry_copy(&entry)
                });
                values.push(entry);
                if let Some(again) = again {
                    values.push(again);
                }
            }
        }
        if private_slots.len() != plan.descriptors[0] as usize + plan.descriptors[1] as usize {
            return Err(E_INVALIDARG);
        }
        s.draws.push(draw);
    }
    if cursor == 0 {
        return Err(E_INVALIDARG);
    }
    s.descriptors = cursor;
    let resources = heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, cursor)?;
    // SAFETY (whole block): live objects created or held here.
    unsafe {
        let start = resources.GetCPUDescriptorHandleForHeapStart();
        s.heap = Some(resources);
        if sampler_cursor != 0 {
            s.samplers = Some(heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, sampler_cursor)?);
            s.sampler_stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
        }
        for (&(heap_raw, offset, count), &at) in &shared_offsets {
            let heap_pointer = heap_raw as *mut c_void;
            let source_heap = borrowed::<ID3D12DescriptorHeap>(&heap_pointer).ok_or(E_INVALIDARG)?;
            let kind = source_heap.GetDesc().Type;
            let sampler = kind == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
            let stride = if sampler { s.sampler_stride } else { s.stride };
            let source = cpu(source_heap.GetCPUDescriptorHandleForHeapStart(), offset, stride);
            let destination_start = if sampler { s.samplers.as_ref().ok_or(E_FAIL)?.GetCPUDescriptorHandleForHeapStart() } else { start };
            device.CopyDescriptorsSimple(count, cpu(destination_start, at, stride), source, kind);
        }
        if s.constant_bytes != 0 {
            let constants = upload_buffer(&device, s.constant_bytes)?;
            let none = D3D12_RANGE::default();
            let mut mapped: *mut c_void = core::ptr::null_mut();
            constants.Map(0, Some(&none), Some(&mut mapped)).map_err(|e| e.code())?;
            if mapped.is_null() {
                constants.Unmap(0, Some(&none));
                return Err(E_FAIL);
            }
            core::ptr::write_bytes(mapped as *mut u8, 0, s.constant_bytes as usize);
            for (bytes, &offset) in &constant_offsets {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), (mapped as *mut u8).add(offset as usize), bytes.len());
            }
            let written = D3D12_RANGE { Begin: 0, End: s.constant_bytes as usize };
            constants.Unmap(0, Some(&written));
            s.constants = Some(constants);
        }
        for &slot in &null_cbvs {
            device.CreateConstantBufferView(None, cpu(start, slot, s.stride));
        }
        let constants_gpu = s.constants.as_ref().map_or(0, |c| c.GetGPUVirtualAddress());
        for entry in &values {
            let value = &*entry.value;
            if entry.srv {
                let snapshot = borrowed::<ID3D12Resource>(&entry.snapshot);
                let view = if value.implicit_srv { None } else { Some(&value.srv as *const D3D12_SHADER_RESOURCE_VIEW_DESC) };
                device.CreateShaderResourceView(snapshot, view, cpu(start, entry.slot, s.stride));
            } else {
                let cbv = if !value.source.is_null() {
                    value.gpu_cbv
                } else {
                    D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: constants_gpu + entry.constant_offset, SizeInBytes: constant_size(value.constants_len) as u32 }
                };
                device.CreateConstantBufferView(Some(&cbv), cpu(start, entry.slot, s.stride));
            }
        }
    }
    Ok(s)
}
fn entry_copy(entry: &Prepared) -> Prepared {
    Prepared { value: entry.value, slot: entry.slot, constant_offset: entry.constant_offset, snapshot: entry.snapshot, srv: entry.srv }
}

// ---- CreateSourcePass

mod stage {
    pub const ARGUMENTS: u32 = 1;
    pub const INPUT: u32 = 2;
    pub const ROOT_BYTES: u32 = 3;
    pub const TABLE: u32 = 5;
    pub const STORAGE: u32 = 6;
    pub const LEASE: u32 = 7;
    pub const CAPACITY: u32 = 8;
    pub const RESOURCE_HEAP: u32 = 9;
    pub const SAMPLER_HEAP: u32 = 10;
    pub const COPY: u32 = 11;
}
mod reason {
    pub const MISSING_DEVICE: u64 = 1 << 1;
    pub const EMPTY_INPUTS: u64 = 1 << 2;
    pub const INPUT_LIMIT: u64 = 1 << 3;
    pub const ROOT_DEVICE: u64 = 1 << 4;
    pub const TABLE_LIMIT: u64 = 1 << 5;
    pub const BLOB_EXTENT: u64 = 1 << 6;
    pub const BLOB_BUDGET: u64 = 1 << 7;
    pub const BLOB_MISMATCH: u64 = 1 << 8;
    pub const PARAMETER: u64 = 1 << 21;
    pub const DUPLICATE_PARAMETER: u64 = 1 << 22;
    pub const CPU_HEAP: u64 = 1 << 23;
    pub const CPU_HEAP_DEVICE: u64 = 1 << 24;
    pub const LEGACY_LABELS: u64 = 1 << 25;
    pub const STORAGE_LABELS: u64 = 1 << 26;
    pub const MIXED_LABELS: u64 = 1 << 27;
    pub const HEAP_TYPE: u64 = 1 << 28;
    pub const HEAP_FLAGS: u64 = 1 << 29;
    pub const SOURCE_EXTENT: u64 = 1 << 30;
    pub const SOURCE_OFFSET: u64 = 1 << 31;
    pub const STORAGE_EXTENT: u64 = 1 << 32;
    pub const MISSING_LEASES: u64 = 1 << 33;
    pub const NULL_LEASE: u64 = 1 << 34;
    pub const STORAGE_CONFLICT: u64 = 1 << 35;
    pub const LEGACY_CONFLICT: u64 = 1 << 36;
    pub const DESCRIPTOR_CAPACITY: u64 = 1 << 37;
    pub const HEAP_CREATE: u64 = 1 << 38;
    pub const HANDLE: u64 = 1 << 39;
    pub const COPY_ADDRESS: u64 = 1 << 40;
    pub const SAMPLER_CAPACITY: u64 = 1 << 42;
    pub const RESOURCE_CAPACITY: u64 = 1 << 43;
    pub const STRIDE: u64 = 1 << 44;
}

/// CreateSourcePass: the source pass storage, or the failure with `failure` describing it.
///
/// # Safety
/// The lent inputs are valid for the call.
pub unsafe fn create_source_pass(device_raw: *mut c_void, inputs: &[SourceInput], failure: &mut SourcePassFailure) -> Result<State, HRESULT> {
    macro_rules! reject {
        ($stage:expr, $bits:expr) => {
            reject!($stage, $bits, E_INVALIDARG)
        };
        ($stage:expr, $bits:expr, $result:expr) => {{
            failure.stage = $stage;
            failure.bits = $bits;
            return Err($result);
        }};
    }
    let Some(device) = held::<ID3D12Device>(device_raw) else {
        reject!(stage::ARGUMENTS, reason::MISSING_DEVICE)
    };
    if inputs.is_empty() {
        reject!(stage::ARGUMENTS, reason::EMPTY_INPUTS)
    }
    if inputs.len() > 32768 {
        reject!(stage::ARGUMENTS, reason::INPUT_LIMIT)
    }
    let _internal = cvr_host::probe::Internal::new();
    let mut s = State::new(device.clone());
    s.source_pass = true;
    // SAFETY: a live device.
    unsafe {
        s.stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        s.sampler_stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
    }
    if s.stride == 0 || s.sampler_stride == 0 {
        reject!(stage::ARGUMENTS, reason::STRIDE)
    }
    struct PendingCopy {
        heap: ID3D12DescriptorHeap,
        offset: u32,
        count: u32,
        destination: u32,
        kind: D3D12_DESCRIPTOR_HEAP_TYPE,
        input: u32,
        parameter: u32,
    }
    #[derive(Clone, Copy)]
    struct Stable {
        version: u64,
        heap: usize,
        offset: u32,
        count: u32,
        destination: u32,
        kind: i32,
    }
    type StorageRange = (usize, u32, usize, u32, u32, i32);
    let mut copies: Vec<PendingCopy> = Vec::new();
    let mut layouts: Vec<SourceLayout> = Vec::new();
    let mut roots: BTreeMap<usize, usize> = BTreeMap::new();
    let mut stable: BTreeMap<u64, Stable> = BTreeMap::new();
    let mut storage_scopes: BTreeMap<u64, StorageRange> = BTreeMap::new();
    let mut storage_ranges: BTreeMap<StorageRange, u32> = BTreeMap::new();
    let mut leased: BTreeSet<usize> = BTreeSet::new();
    let (mut cursor, mut sampler_cursor, mut layout_bytes) = (0u32, 0u32, 0u64);
    for (input_index, input) in inputs.iter().enumerate() {
        failure.input = input_index as u32;
        failure.parameter = u32::MAX;
        if !same_device(&device, input.root) {
            reject!(stage::INPUT, reason::ROOT_DEVICE)
        }
        let tables = slice(input.tables, input.table_count);
        if tables.len() > 64 {
            reject!(stage::INPUT, reason::TABLE_LIMIT)
        }
        let serialized = slice(input.serialized, input.serialized_len);
        let layout_index = match roots.get(&(input.root as usize)) {
            None => {
                if serialized.is_empty() || serialized.len() > 1024 * 1024 {
                    reject!(stage::ROOT_BYTES, reason::BLOB_EXTENT)
                }
                if serialized.len() as u64 > CONSTANT_LIMIT - layout_bytes {
                    reject!(stage::ROOT_BYTES, reason::BLOB_BUDGET, E_OUTOFMEMORY)
                }
                let captured = serialized.to_vec();
                let layout = read_source_layout(&captured, failure)?;
                failure.parameter = u32::MAX;
                layout_bytes += captured.len() as u64;
                layouts.push(layout);
                s.source_layouts.push(captured);
                roots.insert(input.root as usize, layouts.len() - 1);
                layouts.len() - 1
            }
            Some(&index) => {
                if s.source_layouts[index] != serialized {
                    reject!(stage::ROOT_BYTES, reason::BLOB_MISMATCH)
                }
                index
            }
        };
        let layout = layouts[layout_index];
        let mut draw = Draw { root: held(input.root), ..Draw::default() };
        let mut parameters: BTreeSet<u32> = BTreeSet::new();
        for table in tables {
            failure.parameter = table.parameter;
            if table.parameter >= layout.parameters {
                reject!(stage::TABLE, reason::PARAMETER)
            }
            if !parameters.insert(table.parameter) {
                reject!(stage::TABLE, reason::DUPLICATE_PARAMETER)
            }
            let Some(cpu_heap) = borrowed::<ID3D12DescriptorHeap>(&table.cpu_heap) else {
                reject!(stage::TABLE, reason::CPU_HEAP)
            };
            if !same_device(&device, table.cpu_heap) {
                reject!(stage::TABLE, reason::CPU_HEAP_DEVICE)
            }
            if (table.identity != 0) != (table.version != 0) {
                reject!(stage::TABLE, reason::LEGACY_LABELS)
            }
            if (table.storage_scope != 0) != (table.storage_extent != 0) {
                reject!(stage::STORAGE, reason::STORAGE_LABELS)
            }
            if table.storage_scope != 0 && table.identity != 0 {
                reject!(stage::STORAGE, reason::MIXED_LABELS)
            }
            let expected = layout.tables[table.parameter as usize];
            // SAFETY: a live heap.
            let heap_desc = unsafe { cpu_heap.GetDesc() };
            let fits = |count: u32| {
                if expected.unbounded {
                    count >= expected.count && expected.tail_register as u64 + count as u64 - expected.tail_offset as u64 <= u32::MAX as u64 + 1
                } else {
                    count == expected.count
                }
            };
            if heap_desc.Type.0 != expected.kind {
                reject!(stage::TABLE, reason::HEAP_TYPE)
            }
            if heap_desc.Flags != D3D12_DESCRIPTOR_HEAP_FLAG_NONE {
                reject!(stage::TABLE, reason::HEAP_FLAGS)
            }
            if !fits(table.count) {
                reject!(stage::TABLE, reason::SOURCE_EXTENT)
            }
            if table.offset > heap_desc.NumDescriptors || table.count > heap_desc.NumDescriptors - table.offset {
                reject!(stage::TABLE, reason::SOURCE_OFFSET)
            }
            let copied_extent = if table.storage_scope != 0 { table.storage_extent } else { table.count };
            if table.storage_scope != 0 && (table.count > copied_extent || !fits(copied_extent) || copied_extent > heap_desc.NumDescriptors - table.offset) {
                reject!(stage::STORAGE, reason::STORAGE_EXTENT)
            }
            let sampler = heap_desc.Type == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
            if !sampler && table.leases().is_empty() {
                reject!(stage::LEASE, reason::MISSING_LEASES)
            }
            for &owner in table.leases() {
                if owner.is_null() {
                    reject!(stage::LEASE, reason::NULL_LEASE)
                }
                if leased.insert(owner as usize) {
                    let Some(owner) = unknown(owner) else {
                        reject!(stage::LEASE, reason::NULL_LEASE)
                    };
                    s.leases.push(owner);
                }
            }
            let limit = if sampler { 2048 } else { DESCRIPTOR_LIMIT };
            let mut destination = 0u32;
            let mut copy = true;
            let range: StorageRange = (input.root as usize, table.parameter, table.cpu_heap as usize, table.offset, copied_extent, heap_desc.Type.0);
            if table.storage_scope != 0 {
                let scope = *storage_scopes.entry(table.storage_scope).or_insert(range);
                if scope != range {
                    reject!(stage::STORAGE, reason::STORAGE_CONFLICT)
                }
                if let Some(&found) = storage_ranges.get(&range) {
                    destination = found;
                    copy = false;
                }
            }
            if table.identity != 0 {
                if let Some(prior) = stable.get(&table.identity) {
                    if prior.version != table.version
                        || prior.heap != table.cpu_heap as usize
                        || prior.offset != table.offset
                        || prior.count != table.count
                        || prior.kind != heap_desc.Type.0
                    {
                        reject!(stage::STORAGE, reason::LEGACY_CONFLICT)
                    }
                    destination = prior.destination;
                    copy = false;
                }
            }
            if copy {
                let next = if sampler { &mut sampler_cursor } else { &mut cursor };
                if copied_extent > limit - *next {
                    reject!(stage::CAPACITY, reason::DESCRIPTOR_CAPACITY | if sampler { reason::SAMPLER_CAPACITY } else { reason::RESOURCE_CAPACITY })
                }
                destination = *next;
                *next += copied_extent;
                copies.push(PendingCopy {
                    heap: cpu_heap.clone(),
                    offset: table.offset,
                    count: copied_extent,
                    destination,
                    kind: heap_desc.Type,
                    input: failure.input,
                    parameter: table.parameter,
                });
                if table.identity != 0 {
                    stable.insert(
                        table.identity,
                        Stable { version: table.version, heap: table.cpu_heap as usize, offset: table.offset, count: table.count, destination, kind: heap_desc.Type.0 },
                    );
                }
                if table.storage_scope != 0 {
                    storage_ranges.insert(range, destination);
                }
            }
            if sampler { &mut draw.sampler_tables } else { &mut draw.tables }.push((table.parameter, destination));
        }
        s.draws.push(draw);
    }
    failure.input = u32::MAX;
    failure.parameter = u32::MAX;
    s.descriptors = cursor;
    if cursor != 0 {
        match heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, cursor) {
            Ok(h) => s.heap = Some(h),
            Err(hr) => reject!(stage::RESOURCE_HEAP, reason::HEAP_CREATE, hr),
        }
    }
    if sampler_cursor != 0 {
        match heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, sampler_cursor) {
            Ok(h) => s.samplers = Some(h),
            Err(hr) => reject!(stage::SAMPLER_HEAP, reason::HEAP_CREATE, hr),
        }
    }
    for copy in &copies {
        failure.input = copy.input;
        failure.parameter = copy.parameter;
        let sampler = copy.kind == D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
        let stride = if sampler { s.sampler_stride } else { s.stride } as u64;
        // SAFETY (whole block): live heaps.
        unsafe {
            let source = copy.heap.GetCPUDescriptorHandleForHeapStart();
            let target = if sampler { s.samplers.as_ref() } else { s.heap.as_ref() }.map_or(D3D12_CPU_DESCRIPTOR_HANDLE::default(), |h| h.GetCPUDescriptorHandleForHeapStart());
            if source.ptr == 0 || target.ptr == 0 {
                reject!(stage::COPY, reason::HANDLE)
            }
            if copy.offset as u64 + copy.count as u64 - 1 > (usize::MAX - source.ptr) as u64 / stride
                || copy.destination as u64 + copy.count as u64 - 1 > (usize::MAX - target.ptr) as u64 / stride
            {
                reject!(stage::COPY, reason::COPY_ADDRESS)
            }
            device.CopyDescriptorsSimple(
                copy.count,
                D3D12_CPU_DESCRIPTOR_HANDLE { ptr: target.ptr + (copy.destination as u64 * stride) as usize },
                D3D12_CPU_DESCRIPTOR_HANDLE { ptr: source.ptr + (copy.offset as u64 * stride) as usize },
                copy.kind,
            );
        }
    }
    *failure = SourcePassFailure::default();
    Ok(s)
}

// ---- the arena

/// CreateArena: an append-only CBV arena for one immutable paired root.
///
/// # Safety
/// The lent plan is valid for the call.
pub unsafe fn create_arena(device_raw: *mut c_void, root: *mut c_void, plan: &PlanView, max_draws: u32, bytes: u64) -> Result<State, HRESULT> {
    let device: ID3D12Device = held(device_raw).ok_or(E_INVALIDARG)?;
    let bindings = plan.bindings();
    let serialized = plan.serialized();
    if !same_device(&device, root)
        || max_draws == 0
        || max_draws > MAX_ARENA_DRAWS
        || bytes == 0
        || bytes > CONSTANT_LIMIT
        || bytes & 255 != 0
        || bindings.is_empty()
        || bindings.len() > 32
        || serialized.is_empty()
        || serialized.len() > 1024 * 1024
    {
        return Err(E_INVALIDARG);
    }
    let descriptors = plan.descriptors[0] as u64 + plan.descriptors[1] as u64;
    if descriptors != bindings.len() as u64 * 2 || descriptors * max_draws as u64 > DESCRIPTOR_LIMIT as u64 {
        return Err(E_INVALIDARG);
    }
    let decoded = decode(serialized)?;
    let layout = decoded.root();
    if layout.NumParameters > 64 || plan.original_parameters > layout.NumParameters {
        return Err(E_INVALIDARG);
    }
    for stage in 0..2 {
        if plan.descriptors[stage] == 0 {
            continue;
        }
        if plan.parameters[stage] < plan.original_parameters || !table_fits(layout, plan.parameters[stage], plan.descriptors[stage], false) {
            return Err(E_INVALIDARG);
        }
        let table = decoded.parameter(plan.parameters[stage]);
        if table.ShaderVisibility != if stage == 1 { D3D12_SHADER_VISIBILITY_PIXEL } else { D3D12_SHADER_VISIBILITY_VERTEX } {
            return Err(E_INVALIDARG);
        }
        // SAFETY: a table parameter (table_fits held) with its ranges.
        let ranges = unsafe {
            let t = &table.Anonymous.DescriptorTable;
            core::slice::from_raw_parts(t.pDescriptorRanges, t.NumDescriptorRanges as usize)
        };
        if ranges.iter().any(|r| r.RangeType != D3D12_DESCRIPTOR_RANGE_TYPE_CBV) {
            return Err(E_INVALIDARG);
        }
    }
    if plan.descriptors[0] != 0 && plan.descriptors[1] != 0 && plan.parameters[0] == plan.parameters[1] {
        return Err(E_INVALIDARG);
    }
    let mut used = vec![false; descriptors as usize];
    for binding in bindings {
        if binding.binding.kind != D3D12_DESCRIPTOR_RANGE_TYPE_CBV
            || (binding.binding.visibility != D3D12_SHADER_VISIBILITY_VERTEX && binding.binding.visibility != D3D12_SHADER_VISIBILITY_PIXEL)
        {
            return Err(E_INVALIDARG);
        }
        let stage = if binding.binding.visibility == D3D12_SHADER_VISIBILITY_VERTEX { 0 } else { 1 };
        if binding.destination_parameter != plan.parameters[stage]
            || binding.destination_offset > plan.descriptors[stage]
            || plan.descriptors[stage] - binding.destination_offset < 2
        {
            return Err(E_INVALIDARG);
        }
        let at = (if stage == 1 { plan.descriptors[0] } else { 0 } + binding.destination_offset) as usize;
        if used[at] || used[at + 1] {
            return Err(E_INVALIDARG);
        }
        used[at] = true;
        used[at + 1] = true;
    }
    let _internal = cvr_host::probe::Internal::new();
    let mut s = State::new(device.clone());
    s.arena_root = held(root);
    s.arena_plan = plan.to_plan();
    s.max_draws = max_draws;
    s.per_draw_descriptors = descriptors as u32;
    s.capacity = bytes;
    s.draws.reserve(max_draws as usize);
    // SAFETY (whole block): a live device and the objects created here.
    unsafe {
        s.stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        s.heap = Some(heap(&device, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, descriptors as u32 * max_draws)?);
        let constants = upload_buffer(&device, bytes)?;
        let none = D3D12_RANGE::default();
        let mut mapped: *mut c_void = core::ptr::null_mut();
        constants.Map(0, Some(&none), Some(&mut mapped)).map_err(|e| e.code())?;
        if mapped.is_null() {
            constants.Unmap(0, Some(&none));
            return Err(E_FAIL);
        }
        s.constants = Some(constants);
        s.mapped = mapped;
    }
    Ok(s)
}
/// AppendConstants: one more immutable draw of the arena; its index.
///
/// # Safety
/// The lent plan and pairs are valid for the call.
pub unsafe fn append_constants(s: &mut State, plan: &PlanView, pairs: &[Pair]) -> Result<u32, HRESULT> {
    if s.arena_root.is_none() || plan.to_plan() != s.arena_plan || pairs.len() != plan.bindings().len() || s.draws.len() >= s.max_draws as usize {
        return Err(E_INVALIDARG);
    }
    let mut values: Vec<(&Value, u32, u64)> = Vec::with_capacity(pairs.len() * 2);
    let mut seen = vec![false; pairs.len()];
    let mut cursor = s.constant_bytes;
    for binding in plan.bindings() {
        let Some(at) = pairs.iter().position(|p| p.binding == binding.binding) else {
            return Err(E_INVALIDARG);
        };
        if seen[at] {
            return Err(E_INVALIDARG);
        }
        seen[at] = true;
        let stage = if binding.binding.visibility == D3D12_SHADER_VISIBILITY_VERTEX { 0 } else { 1 };
        for eye in 0..2u32 {
            let value = &pairs[at].eyes[eye as usize];
            let constants = value.constants();
            if !value.source.is_null()
                || !value.owner.is_null()
                || value.gpu_cbv.BufferLocation != 0
                || value.gpu_cbv.SizeInBytes != 0
                || value.borrowed_srv
                || value.implicit_srv
                || constants.is_empty()
                || constants.len() > 65536
            {
                return Err(E_INVALIDARG);
            }
            let size = constant_size(constants.len());
            if size > s.capacity - cursor {
                return Err(E_OUTOFMEMORY);
            }
            values.push((value, s.descriptors + if stage == 1 { plan.descriptors[0] } else { 0 } + binding.destination_offset + eye, cursor));
            cursor += size;
        }
    }
    let mut draw = Draw { root: s.arena_root.clone(), ..Draw::default() };
    for stage in 0..2 {
        if plan.descriptors[stage] != 0 {
            draw.tables.push((plan.parameters[stage], s.descriptors + if stage == 1 { plan.descriptors[0] } else { 0 }));
        }
    }
    let _internal = cvr_host::probe::Internal::new();
    // SAFETY (whole block): the arena's held objects and persistent mapping.
    unsafe {
        let start = s.heap.as_ref().ok_or(E_FAIL)?.GetCPUDescriptorHandleForHeapStart();
        let gpu = s.constants.as_ref().ok_or(E_FAIL)?.GetGPUVirtualAddress();
        for &(value, slot, offset) in &values {
            let constants = value.constants();
            let size = constant_size(constants.len());
            let target = (s.mapped as *mut u8).add(offset as usize);
            core::ptr::write_bytes(target, 0, size as usize);
            core::ptr::copy_nonoverlapping(constants.as_ptr(), target, constants.len());
            let cbv = D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: gpu + offset, SizeInBytes: size as u32 };
            s.device.CreateConstantBufferView(Some(&cbv), cpu(start, slot, s.stride));
        }
        core::arch::x86_64::_mm_sfence();
    }
    let index = s.draws.len() as u32;
    s.draws.push(draw);
    s.constant_bytes = cursor;
    s.descriptors += s.per_draw_descriptors;
    Ok(index)
}

// ---- recording

/// The command resources' retention (Keep) and current-recording membership (Has) of the storage on a list.
pub type Retain = unsafe extern "C-unwind" fn(list: *mut c_void, storage: *mut c_void) -> bool;

fn list<'a>(raw: &'a *mut c_void) -> Option<&'a ID3D12GraphicsCommandList> {
    borrowed::<ID3D12GraphicsCommandList>(raw)
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
/// CaptureEye: the eye's SRV snapshots copied on the recording.
///
/// # Safety
/// `storage` is a live storage; `list` is live or null.
pub unsafe fn capture_eye(storage: *mut Storage, list_raw: *mut c_void, eye: u32, keep: Retain, has: Retain) -> bool {
    // SAFETY: a live storage.
    let s = unsafe { &mut (*storage).state };
    let Some(l) = list(&list_raw) else {
        return false;
    };
    // SAFETY (whole block): a live list and the storage's held objects.
    unsafe {
        if s.source_pass || l.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || eye > 1 || s.captured_mask & (1 << eye) != 0 || !same_device(&s.device, list_raw) {
            return false;
        }
        if s.recording != 0 {
            if s.recording != list_raw as usize || !has(list_raw, storage.cast()) {
                return false;
            }
        } else if !keep(list_raw, storage.cast()) {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        let read = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        for copy in s.copies.iter().filter(|c| c.eye == eye) {
            let move_state = (copy.state & D3D12_RESOURCE_STATE_COPY_SOURCE).0 == 0;
            if move_state {
                l.ResourceBarrier(&[transition(&copy.source, copy.state, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            }
            l.CopyResource(&copy.owned, &copy.source);
            if move_state {
                l.ResourceBarrier(&[transition(&copy.source, D3D12_RESOURCE_STATE_COPY_SOURCE, copy.state)]);
            }
            l.ResourceBarrier(&[transition(&copy.owned, D3D12_RESOURCE_STATE_COPY_DEST, read)]);
        }
    }
    s.recording = list_raw as usize;
    s.captured_mask |= 1 << eye;
    true
}
fn set_tables(l: &ID3D12GraphicsCommandList, heap: &ID3D12DescriptorHeap, stride: u32, tables: &[(u32, u32)]) {
    // SAFETY: a live list and heap.
    unsafe {
        let start = heap.GetGPUDescriptorHandleForHeapStart();
        for &(parameter, offset) in tables {
            l.SetGraphicsRootDescriptorTable(parameter, D3D12_GPU_DESCRIPTOR_HANDLE { ptr: start.ptr + offset as u64 * stride as u64 });
        }
    }
}
/// Bind (`eye` None) or BindSourceEye: a captured pass draw's paired (or source) tables.
///
/// # Safety
/// `storage` is a live storage; `list` is live or null.
pub unsafe fn bind(storage: *mut Storage, list_raw: *mut c_void, eye: Option<u32>, index: u32, has: Retain) -> bool {
    // SAFETY: a live storage.
    let s = unsafe { &(*storage).state };
    let Some(l) = list(&list_raw) else {
        return false;
    };
    if s.source_pass
        || eye.is_some_and(|e| e > 1)
        || index as usize >= s.draws.len()
        || s.recording != list_raw as usize
        || s.captured_mask != 3
        || !unsafe { has(list_raw, storage.cast()) }
    {
        return false;
    }
    let draw = &s.draws[index as usize];
    let Some(heap) = s.heap.as_ref() else {
        return false;
    };
    match eye {
        None => {
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: a live list and the storage's held objects.
            unsafe {
                let heaps: Vec<Option<ID3D12DescriptorHeap>> = [Some(heap.clone()), s.samplers.clone()].into_iter().flatten().map(Some).collect();
                l.SetDescriptorHeaps(&heaps);
                l.SetGraphicsRootSignature(draw.root.as_ref());
            }
            set_tables(l, heap, s.stride, &draw.tables);
            if let Some(samplers) = &s.samplers {
                set_tables(l, samplers, s.sampler_stride, &draw.sampler_tables);
            }
        }
        Some(eye) => {
            if draw.source_root.is_none() || draw.source_tables[eye as usize].is_empty() {
                return false;
            }
            let _internal = cvr_host::probe::Internal::new();
            // SAFETY: a live list and the storage's held objects.
            unsafe {
                l.SetDescriptorHeaps(&[Some(heap.clone())]);
                l.SetGraphicsRootSignature(draw.source_root.as_ref());
            }
            set_tables(l, heap, s.stride, &draw.source_tables[eye as usize]);
        }
    }
    true
}
/// BindSource: a source pass draw's tables on a caller-certified DIRECT recording.
///
/// # Safety
/// `storage` is a live storage; `list` is live or null.
pub unsafe fn bind_source(storage: *mut Storage, list_raw: *mut c_void, index: u32, keep: Retain) -> bool {
    // SAFETY: a live storage.
    let s = unsafe { &(*storage).state };
    let Some(l) = list(&list_raw) else {
        return false;
    };
    // SAFETY (whole block): a live list and the storage's held objects.
    unsafe {
        if !s.source_pass || index as usize >= s.draws.len() || l.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(&s.device, list_raw) {
            return false;
        }
        if !keep(list_raw, storage.cast()) {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        let draw = &s.draws[index as usize];
        let heaps: Vec<Option<ID3D12DescriptorHeap>> = [s.heap.clone(), s.samplers.clone()].into_iter().filter(Option::is_some).collect();
        l.SetDescriptorHeaps(&heaps);
        l.SetGraphicsRootSignature(draw.root.as_ref());
        if let Some(heap) = &s.heap {
            set_tables(l, heap, s.stride, &draw.tables);
        }
        if let Some(samplers) = &s.samplers {
            set_tables(l, samplers, s.sampler_stride, &draw.sampler_tables);
        }
    }
    true
}

// ---- the C ABI

/// The storage's counts for the .cpp's accessors.
#[repr(C)]
pub struct Counts {
    pub draws: u32,
    pub descriptors: u32,
    pub snapshots: u32,
    pub constant_bytes: u64,
}
fn finish(result: std::thread::Result<Result<State, HRESULT>>, out: *mut *mut c_void) -> HRESULT {
    match result {
        Ok(Ok(state)) => {
            // SAFETY: the caller's out-pointer.
            unsafe { *out = boxed(state).cast() };
            S_OK
        }
        Ok(Err(hr)) => hr,
        Err(_) => E_OUTOFMEMORY,
    }
}
/// # Safety
/// The lent draws are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_create_pass(device: *mut c_void, draws: *const DrawInput, count: usize, out: *mut *mut c_void) -> HRESULT {
    let draws = slice(draws, count);
    finish(std::panic::catch_unwind(|| unsafe { create_pass(device, draws) }), out)
}
/// # Safety
/// The lent inputs are valid; `failure` and `out` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_create_source_pass(
    device: *mut c_void,
    inputs: *const SourceInput,
    count: usize,
    failure: *mut SourcePassFailure,
    out: *mut *mut c_void,
) -> HRESULT {
    let inputs = slice(inputs, count);
    let failure = unsafe { &mut *failure };
    let result = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| unsafe { create_source_pass(device, inputs, failure) }));
    if result.is_err() {
        // SourcePassFailureStage::Exception, SourcePassFailureReason::AllocationException.
        failure.stage = 12;
        failure.bits = 1 << 41;
    }
    finish(result, out)
}
/// # Safety
/// The lent plan is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_create_arena(
    device: *mut c_void,
    root: *mut c_void,
    plan: *const PlanView,
    max_draws: u32,
    bytes: u64,
    out: *mut *mut c_void,
) -> HRESULT {
    let plan = unsafe { &*plan };
    finish(std::panic::catch_unwind(|| unsafe { create_arena(device, root, plan, max_draws, bytes) }), out)
}
/// # Safety
/// `storage` is a live storage; the lent plan and pairs are valid; `index` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_append_constants(storage: *mut c_void, plan: *const PlanView, pairs: *const Pair, count: usize, index: *mut u32) -> HRESULT {
    let s = unsafe { &mut (*(storage as *mut Storage)).state };
    match unsafe { append_constants(s, &*plan, slice(pairs, count)) } {
        Ok(at) => {
            unsafe { *index = at };
            S_OK
        }
        Err(hr) => hr,
    }
}
/// RestartArena.
///
/// # Safety
/// `storage` is a live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_restart_arena(storage: *mut c_void) -> bool {
    let s = unsafe { &mut (*(storage as *mut Storage)).state };
    if s.arena_root.is_none() {
        return false;
    }
    s.draws.clear();
    s.constant_bytes = 0;
    s.descriptors = 0;
    s.captured_mask = 0;
    s.recording = 0;
    true
}
/// CapturedFor.
///
/// # Safety
/// `storage` is a live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_captured_for(storage: *mut c_void, list: *mut c_void, has: Retain) -> bool {
    let s = unsafe { &(*(storage as *mut Storage)).state };
    !list.is_null() && s.recording == list as usize && s.captured_mask == 3 && unsafe { has(list, storage) }
}
/// # Safety
/// `storage` is a live storage; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_capture_eye(storage: *mut c_void, list: *mut c_void, eye: u32, keep: Retain, has: Retain) -> bool {
    unsafe { capture_eye(storage.cast(), list, eye, keep, has) }
}
/// Bind (`eye` u32::MAX) or BindSourceEye.
///
/// # Safety
/// `storage` is a live storage; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_bind(storage: *mut c_void, list: *mut c_void, eye: u32, index: u32, has: Retain) -> bool {
    unsafe { bind(storage.cast(), list, (eye != u32::MAX).then_some(eye), index, has) }
}
/// # Safety
/// `storage` is a live storage; `list` is live or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_bind_source(storage: *mut c_void, list: *mut c_void, index: u32, keep: Retain) -> bool {
    unsafe { bind_source(storage.cast(), list, index, keep) }
}
/// # Safety
/// `storage` is a live storage; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bank_counts(storage: *mut c_void, out: *mut Counts) {
    let s = unsafe { &(*(storage as *mut Storage)).state };
    unsafe {
        *out = Counts { draws: s.draws.len() as u32, descriptors: s.descriptors, snapshots: s.copies.len() as u32, constant_bytes: s.constant_bytes };
    }
}
