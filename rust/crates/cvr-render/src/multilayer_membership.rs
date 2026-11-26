//! The GPU multilayer membership check (src/Render/GpuMultilayerMembership.cpp, `GpuMultilayerMembership`): for each
//! multilayer source the exact PS's actual B4 / T5 bytes are checked on the GPU against an owned descriptor bank -- the
//! base record from B4's float at +128 (finite, non-negative, below 2^32), the mask at +224, and for each set lane its
//! record's five selector indices (T5 +100..+108) each below 32768 and present in the source's 1024-word membership
//! bitmap; one 32-lane group per source, one dispatch per distinct held (B4, T5) pair in first-seen order. A zero
//! producer gate disables both predicates. Outputs: per source a predicate pair (accepted, rejected) and eight result
//! words (enabled, error, base, mask, records, selectors, first failure bit, first failure index).
//!
//! The storage is a COM object (an IUnknown the command resources retain through the recording) owned by the .cpp's
//! ComPtr<Storage>; in the Rust build (`CVR_RUST_VRIK`) Create, Record and the accessors forward to `cvr_mlm_*` (the
//! family check and the command resources stay with their modules: the latter is a callback).
//! `tools/single_pass_tests/multilayer_membership_test` runs its cases against it in its Rust variant.

use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};

use windows::core::{IUnknown_Vtbl, Interface, GUID, HRESULT, PCSTR};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_IEEE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;

const SHADER: &str = r#"
ByteAddressBuffer b4 : register(t0);
ByteAddressBuffer t5 : register(t1);
ByteAddressBuffer membership : register(t2);
ByteAddressBuffer producer : register(t3);
ByteAddressBuffer metadata : register(t4);
RWByteAddressBuffer predicates : register(u0);
RWByteAddressBuffer results : register(u1);
cbuffer Parameters : register(b0) {
    uint firstMetadata;
};
groupshared uint enabled, baseRecord, mask, invalidBase;
groupshared uint failures[32], missing[32], recordCounts[32], selectorCounts[32];
[numthreads(32,1,1)]
void Check(uint lane : SV_GroupIndex,uint3 group : SV_GroupID) {
    uint metadataOffset=(firstMetadata+group.x)*32;
    uint4 addresses=metadata.Load4(metadataOffset);
    uint2 identities=metadata.Load2(metadataOffset+16);
    uint b4Offset=addresses.x,t5Offset=addresses.y,firstElement=addresses.z,numElements=addresses.w;
    uint membershipOffset=identities.x,sourceIndex=identities.y;
    failures[lane]=0;missing[lane]=0xffffffffu;
    recordCounts[lane]=0;selectorCounts[lane]=0;
    if(lane==0) {
        enabled=any(producer.Load2(0)!=0)?1:0;
        baseRecord=0;mask=0;invalidBase=0;
        [branch] if(enabled) {
            float raw=asfloat(b4.Load(b4Offset+128));
            mask=b4.Load(b4Offset+224);
            if(isfinite(raw) && raw>=0.0f && raw<4294967296.0f)baseRecord=(uint)raw;
            else invalidBase=1;
        }
    }
    GroupMemoryBarrierWithGroupSync();
    [branch] if(enabled && !invalidBase && (mask&(1u<<lane))) {
        if(baseRecord>0xffffffffu-lane)failures[lane]=2;
        else {
            uint record=baseRecord+lane;
            [branch] if(record>=numElements)failures[lane]=4;
            else {
                uint address=t5Offset+(firstElement+record)*128;
                uint a=t5.Load(address+100), b=t5.Load(address+104), c=t5.Load(address+108);
                uint indices[5]={a,c>>16,c&65535u,b&65535u,b>>16};
                recordCounts[lane]=1;selectorCounts[lane]=5;
                [unroll] for(uint i=0;i<5;++i) {
                    uint index=indices[i],reason=0;
                    [branch] if(index>=32768u)reason=8;
                    else if(!(membership.Load(membershipOffset+(index>>5)*4)&(1u<<(index&31))))reason=16;
                    failures[lane]|=reason;
                    if(reason && missing[lane]==0xffffffffu)missing[lane]=index;
                }
            }
        }
    }
    GroupMemoryBarrierWithGroupSync();
    if(lane==0) {
        uint error=invalidBase,records=0,selectors=0,firstBit=0xffffffffu,firstIndex=0xffffffffu;
        [unroll] for(uint i=0;i<32;++i) {
            error|=failures[i];records+=recordCounts[i];selectors+=selectorCounts[i];
            if(failures[i] && firstBit==0xffffffffu){firstBit=i;firstIndex=missing[i];}
        }
        predicates.Store4(sourceIndex*16,uint4(enabled && !error?1:0,0,enabled && error?1:0,0));
        results.Store4(sourceIndex*32,uint4(enabled,error,baseRecord,mask));
        results.Store4(sourceIndex*32+16,uint4(records,selectors,firstBit,firstIndex));
    }
}
"#;
/// multilayer_selectors::B4Bytes and Stride.
const B4_BYTES: u64 = 228;
const STRIDE: u64 = 128;

/// One source as the .cpp lends it (its family already checked).
#[repr(C)]
pub struct Source {
    pub b4: *mut c_void,
    pub t5: *mut c_void,
    pub b4_offset: u64,
    pub t5_offset: u64,
    pub t5_bytes: u64,
    pub first_element: u32,
    pub num_elements: u32,
    pub membership: *const u32,
    pub source_id: u64,
    pub eye: u32,
}
const _: () = assert!(core::mem::size_of::<Source>() == 72);

struct Group {
    first: u32,
    count: u32,
    source: u32,
}
struct State {
    root: ID3D12RootSignature,
    pipeline: ID3D12PipelineState,
    gate: ID3D12Resource,
    gate_offset: u64,
    membership: ID3D12Resource,
    metadata: ID3D12Resource,
    predicates: ID3D12Resource,
    results: ID3D12Resource,
    groups: Vec<Group>,
    sources: Vec<(ID3D12Resource, ID3D12Resource)>,
    device: ID3D12Device,
    recorded: bool,
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
/// Range: a shader-readable buffer on the device holding [offset, offset + bytes) below 4 GiB, 4-aligned.
fn range(device: &ID3D12Device, raw: *mut c_void, offset: u64, bytes: u64) -> bool {
    if !same_device(device, raw) || bytes == 0 || offset % 4 != 0 || offset > u32::MAX as u64 || bytes > u32::MAX as u64 - offset {
        return false;
    }
    // SAFETY: a live resource (same_device held).
    let r = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.expect("resource");
    // SAFETY: a live resource.
    let (desc, gpu) = unsafe { (r.GetDesc(), r.GetGPUVirtualAddress()) };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && (desc.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE).0 == 0
        && gpu != 0
        && gpu & 3 == 0
        && gpu <= u64::MAX - desc.Width
        && offset <= desc.Width
        && bytes <= desc.Width - offset
}
fn resource(raw: *mut c_void) -> ID3D12Resource {
    // SAFETY: a live resource of the caller's (checked by range).
    unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.expect("resource").clone()
}
fn buffer(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, width: u64, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource, HRESULT> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: width,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: flags,
        ..Default::default()
    };
    let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; plain descriptions.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.map_err(|e| e.code())?;
    out.ok_or(E_FAIL)
}
fn upload(target: &ID3D12Resource, bytes: &[u8]) -> Result<(), HRESULT> {
    let no_read = D3D12_RANGE::default();
    let mut mapped: *mut c_void = core::ptr::null_mut();
    // SAFETY: a live upload buffer of at least `bytes.len()` bytes.
    unsafe {
        target.Map(0, Some(&no_read), Some(&mut mapped)).map_err(|e| e.code())?;
        if mapped.is_null() {
            return Err(E_FAIL);
        }
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped as *mut u8, bytes.len());
        core::arch::x86_64::_mm_sfence();
        let written = D3D12_RANGE { Begin: 0, End: bytes.len() };
        target.Unmap(0, Some(&written));
    }
    Ok(())
}

/// Create: the checked storage, or the failure.
///
/// # Safety
/// The sources' resources and membership words (1024 each) are valid for the call.
pub unsafe fn create(device_raw: *mut c_void, sources: &[Source], gate: *mut c_void, gate_offset: u64) -> Result<*mut Storage, HRESULT> {
    // SAFETY: a live device of the caller's, or null.
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device_raw) }.ok_or(E_INVALIDARG)?.clone();
    // SAFETY: a live device.
    if unsafe { device.GetNodeCount() } != 1 || sources.is_empty() || sources.len() > 512 || gate_offset % 8 != 0 || !range(&device, gate, gate_offset, 8) {
        return Err(E_INVALIDARG);
    }
    for s in sources {
        if s.eye > 1
            || s.source_id == 0
            || s.num_elements == 0
            || s.t5_bytes % STRIDE != 0
            || s.t5_bytes > 1024 * 1024
            || !range(&device, s.b4, s.b4_offset, B4_BYTES)
            || !range(&device, s.t5, s.t5_offset, s.t5_bytes)
        {
            return Err(E_INVALIDARG);
        }
        let first = s.first_element as u64 * STRIDE;
        let bytes = s.num_elements as u64 * STRIDE;
        if first > s.t5_bytes || bytes > s.t5_bytes - first {
            return Err(E_INVALIDARG);
        }
    }
    let _internal = cvr_host::probe::Internal::new();
    let mut grouped: Vec<Vec<u32>> = Vec::new();
    for (i, s) in sources.iter().enumerate() {
        match grouped.iter_mut().find(|g| {
            let first = &sources[g[0] as usize];
            first.b4 == s.b4 && first.t5 == s.t5
        }) {
            Some(group) => group.push(i as u32),
            None => grouped.push(vec![i as u32]),
        }
    }
    let mut metadata: Vec<[u32; 8]> = Vec::with_capacity(sources.len());
    let mut groups = Vec::with_capacity(grouped.len());
    for group in &grouped {
        groups.push(Group { first: metadata.len() as u32, count: group.len() as u32, source: group[0] });
        for &index in group {
            let s = &sources[index as usize];
            metadata.push([s.b4_offset as u32, s.t5_offset as u32, s.first_element, s.num_elements, index * 4096, index, 0, 0]);
        }
    }
    if metadata.len() != sources.len() || groups.is_empty() || groups.len() > 512 {
        return Err(E_INVALIDARG);
    }
    let mut parameters: [D3D12_ROOT_PARAMETER; 8] = Default::default();
    for (i, p) in parameters.iter_mut().enumerate().take(5) {
        p.ParameterType = D3D12_ROOT_PARAMETER_TYPE_SRV;
        p.Anonymous.Descriptor = D3D12_ROOT_DESCRIPTOR { ShaderRegister: i as u32, RegisterSpace: 0 };
    }
    for i in 0..2 {
        parameters[5 + i].ParameterType = D3D12_ROOT_PARAMETER_TYPE_UAV;
        parameters[5 + i].Anonymous.Descriptor = D3D12_ROOT_DESCRIPTOR { ShaderRegister: i as u32, RegisterSpace: 0 };
    }
    parameters[7].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    parameters[7].Anonymous.Constants = D3D12_ROOT_CONSTANTS { ShaderRegister: 0, RegisterSpace: 0, Num32BitValues: 1 };
    let root_desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: parameters.len() as u32,
        pParameters: parameters.as_ptr(),
        NumStaticSamplers: 0,
        pStaticSamplers: core::ptr::null(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    // SAFETY (whole block): a live device; descriptions and blobs alive for their calls.
    unsafe {
        let mut blob: Option<ID3DBlob> = None;
        D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).map_err(|e| e.code())?;
        let blob = blob.ok_or(E_FAIL)?;
        let root: ID3D12RootSignature =
            device.CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).map_err(|e| e.code())?;
        // the shader made from Rust (rust/shaders/passes/src/single_pass.rs); the HLSL above with CVR_SHADER_HLSL
        let hlsl = || {
            let mut code: Option<ID3DBlob> = None;
            let mut errors: Option<ID3DBlob> = None;
            D3DCompile(
                SHADER.as_ptr() as *const c_void,
                SHADER.len(),
                PCSTR::null(),
                None,
                None,
                PCSTR(c"Check".as_ptr().cast()),
                PCSTR(c"cs_5_0".as_ptr().cast()),
                D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_IEEE_STRICTNESS,
                0,
                &mut code,
                Some(&mut errors),
            )
            .ok()?;
            code
        };
        let code = cvr_gpu::dxil::pick(cvr_gpu::dxil::MULTILAYER_MEMBERSHIP_CS, hlsl).ok_or(E_FAIL)?;
        let pipeline_desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(&root),
            CS: code.bytecode(),
            ..Default::default()
        };
        let pipeline: ID3D12PipelineState = device.CreateComputePipelineState(&pipeline_desc).map_err(|e| e.code())?;
        let membership = buffer(&device, D3D12_HEAP_TYPE_UPLOAD, sources.len() as u64 * 4096, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_GENERIC_READ)?;
        let mut bits = Vec::with_capacity(sources.len() * 4096);
        for s in sources {
            bits.extend_from_slice(core::slice::from_raw_parts(s.membership as *const u8, 4096));
        }
        upload(&membership, &bits)?;
        let metadata_buffer =
            buffer(&device, D3D12_HEAP_TYPE_UPLOAD, (metadata.len() * 32) as u64, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_GENERIC_READ)?;
        upload(&metadata_buffer, core::slice::from_raw_parts(metadata.as_ptr() as *const u8, metadata.len() * 32))?;
        let predicates = buffer(
            &device,
            D3D12_HEAP_TYPE_DEFAULT,
            sources.len() as u64 * 16,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        )?;
        let results = buffer(
            &device,
            D3D12_HEAP_TYPE_DEFAULT,
            sources.len() as u64 * 32,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        )?;
        let state = State {
            root,
            pipeline,
            gate: resource(gate),
            gate_offset,
            membership,
            metadata: metadata_buffer,
            predicates,
            results,
            groups,
            sources: sources.iter().map(|s| (resource(s.b4), resource(s.t5))).collect(),
            device,
            recorded: false,
        };
        Ok(Box::into_raw(Box::new(Storage { vtbl: &STORAGE_VTBL, refs: AtomicU32::new(1), state })))
    }
}

/// The command resources' retention of the storage on the recording (KeepCommandResources).
pub type Keep = unsafe extern "C-unwind" fn(list: *mut c_void, storage: *mut c_void) -> bool;

fn transition(resource: &ID3D12Resource, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                StateAfter: after,
            }),
        },
    }
}
/// Record: the storage's one DIRECT recording (predication cleared, compute bindings changed).
///
/// # Safety
/// `storage` is a live storage; `list` is a live command list or null.
pub unsafe fn record(storage: *mut Storage, list_raw: *mut c_void, keep: Keep) -> bool {
    // SAFETY: a live storage.
    let s = unsafe { &mut (*storage).state };
    // SAFETY: a live command list of the caller's, or null.
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list_raw) }) else {
        return false;
    };
    // SAFETY (whole block): a live list and the storage's held objects.
    unsafe {
        if s.recorded || list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(&s.device, list_raw) || !keep(list_raw, storage.cast()) {
            return false;
        }
        s.recorded = true;
        let _internal = cvr_host::probe::Internal::new();
        list.SetPredication(None::<&ID3D12Resource>, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
        list.SetComputeRootSignature(&s.root);
        list.SetPipelineState(&s.pipeline);
        list.SetComputeRootShaderResourceView(2, s.membership.GetGPUVirtualAddress());
        list.SetComputeRootShaderResourceView(3, s.gate.GetGPUVirtualAddress() + s.gate_offset);
        list.SetComputeRootShaderResourceView(4, s.metadata.GetGPUVirtualAddress());
        list.SetComputeRootUnorderedAccessView(5, s.predicates.GetGPUVirtualAddress());
        list.SetComputeRootUnorderedAccessView(6, s.results.GetGPUVirtualAddress());
        for group in &s.groups {
            let (b4, t5) = &s.sources[group.source as usize];
            list.SetComputeRootShaderResourceView(0, b4.GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(1, t5.GetGPUVirtualAddress());
            list.SetComputeRoot32BitConstant(7, group.first, 0);
            list.Dispatch(group.count, 1, 1);
        }
        let read = D3D12_RESOURCE_STATE_COPY_SOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        list.ResourceBarrier(&[transition(&s.predicates, read | D3D12_RESOURCE_STATE_PREDICATION), transition(&s.results, read)]);
    }
    true
}

// ---- the C ABI

/// The storage's outputs for the .cpp's accessors.
#[repr(C)]
pub struct Info {
    pub predicates: *mut c_void,
    pub results: *mut c_void,
    pub sources: u32,
    pub dispatches: u32,
}
/// Create: S_OK and the storage (one reference for the caller), or the failure.
///
/// # Safety
/// `sources` holds `count` sources (see create); `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mlm_create(device: *mut c_void, sources: *const Source, count: usize, gate: *mut c_void, gate_offset: u64, out: *mut *mut c_void) -> HRESULT {
    let sources = if sources.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(sources, count) } };
    match std::panic::catch_unwind(|| unsafe { create(device, sources, gate, gate_offset) }) {
        Ok(Ok(storage)) => {
            unsafe { *out = storage.cast() };
            S_OK
        }
        Ok(Err(hr)) => hr,
        Err(_) => E_OUTOFMEMORY,
    }
}
/// # Safety
/// `storage` is a live storage; `list` is a live command list or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mlm_record(storage: *mut c_void, list: *mut c_void, keep: Keep) -> bool {
    unsafe { record(storage.cast(), list, keep) }
}
/// # Safety
/// `storage` is a live storage; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mlm_info(storage: *mut c_void, out: *mut Info) {
    let s = unsafe { &(*(storage as *mut Storage)).state };
    unsafe {
        *out = Info {
            predicates: s.predicates.as_raw(),
            results: s.results.as_raw(),
            sources: s.sources.len() as u32,
            dispatches: if s.recorded { s.groups.len() as u32 } else { 0 },
        };
    }
}
