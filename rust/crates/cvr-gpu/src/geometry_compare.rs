//! Single pass's GPU geometry comparison (formerly src/Render/GpuGeometryCompare.cpp): byte equality of buffer range
//! pairs computed on the GPU into complementary draw predicates per pair -- the "same" branch at offset 16*pair, the
//! fallback at +8 -- with no CPU readback. A zero external gate disables both branches; otherwise exactly one runs.
//! All ranges sharing one resource pair go through one metadata-driven dispatch (1 KiB per row). The caller certifies
//! immutable inputs in NON_PIXEL_SHADER_RESOURCE and the producer order; the results end in a state that can feed
//! another comparison's gate in the same recording. The shader stays HLSL.
use core::ffi::c_void;
use std::collections::BTreeMap;
use std::sync::Mutex;

use cvr_host::probe::Internal;
use windows::core::{Interface, HRESULT, PCSTR};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

const D3DCOMPILE_OPTIMIZATION_LEVEL3: u32 = 1 << 15;
const SHADER: &str = r#"
ByteAddressBuffer leftBytes : register(t0);
ByteAddressBuffer rightBytes : register(t1);
ByteAddressBuffer permission : register(t2);
ByteAddressBuffer ranges : register(t3);
RWByteAddressBuffer results : register(u0);
cbuffer Parameters : register(b0) { uint batchFirst, batchCount, batchWidth, pairCount; };
[numthreads(256,1,1)]
void Initialize(uint3 id : SV_DispatchThreadID) {
    if(id.x>=pairCount)return;
    uint2 allowed=permission.Load2(0);
    results.Store4(id.x*16,uint4(any(allowed!=0)?1:0,0,0,0));
}
uint Left(uint address,uint count) {
    uint shift=(address&3)*8;
    uint value=leftBytes.Load(address&~3u)>>shift;
    if((address&3)+count>4)value|=leftBytes.Load((address&~3u)+4)<<(32-shift);
    return value;
}
uint Right(uint address,uint count) {
    uint shift=(address&3)*8;
    uint value=rightBytes.Load(address&~3u)>>shift;
    if((address&3)+count>4)value|=rightBytes.Load((address&~3u)+4)<<(32-shift);
    return value;
}
[numthreads(256,1,1)]
void Compare(uint3 thread : SV_GroupThreadID,uint3 group : SV_GroupID) {
    uint index=group.x+batchWidth*group.y;
    if(index>=batchCount || !any(permission.Load2(0)!=0))return;
    uint4 range=ranges.Load4((batchFirst+index)*16);
    uint at=thread.x*4;
    if(at>=range.z)return;
    uint count=min(4u,range.z-at);
    uint mask=0xffffffffu>>(32-count*8);
    if(((Left(range.x+at,count)^Right(range.y+at,count))&mask)!=0) {
        uint ignored;
        results.InterlockedAnd(range.w*16,0,ignored);
        results.InterlockedOr(range.w*16+8,1,ignored);
    }
}
"#;

/// GeometryCompareRange, as the C++ wrapper hands it over (the resources borrowed).
#[repr(C)]
pub struct RangeIn {
    pub pair: u32,
    pub resources: [*mut c_void; 2],
    pub offsets: [u64; 2],
    pub bytes: u64,
}
/// One range pair to compare (owned references).
pub struct Range {
    pub pair: u32,
    pub resources: [ID3D12Resource; 2],
    pub offsets: [u64; 2],
    pub bytes: u64,
}
struct Batch {
    resources: [ID3D12Resource; 2],
    first: u32,
    count: u32,
}
struct Storage {
    device: ID3D12Device,
    root: ID3D12RootSignature,
    initialize: ID3D12PipelineState,
    compare: ID3D12PipelineState,
    predicates: ID3D12Resource,
    gate: ID3D12Resource,
    metadata: ID3D12Resource,
    batches: Vec<Batch>,
    ranges: Vec<Range>,
    pairs: u32,
    bytes: u64,
    gate_offset: u64,
    recorded: bool,
}
// SAFETY: free-threaded D3D12 objects under the mutex.
unsafe impl Send for Storage {}

/// GpuGeometryCompare.
#[derive(Default)]
pub struct GeometryCompare {
    storage: Mutex<Option<Storage>>,
}

fn same_device(device: &ID3D12Device, child: &impl Interface) -> bool {
    let Ok(child) = child.cast::<ID3D12DeviceChild>() else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: the live child.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
fn valid_range(device: &ID3D12Device, value: &ID3D12Resource, offset: u64, bytes: u64) -> bool {
    if !same_device(device, value) || bytes == 0 || offset > u32::MAX as u64 || bytes > u32::MAX as u64 - offset {
        return false;
    }
    // SAFETY: the live resource.
    let (desc, address) = unsafe { (value.GetDesc(), value.GetGPUVirtualAddress()) };
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && (desc.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE).0 == 0
        && address != 0
        && ((offset + bytes + 3) & !3) <= desc.Width
}

fn create(device: &ID3D12Device, pairs: u32, input: &[Range], gate: &ID3D12Resource, gate_offset: u64) -> Result<Storage, HRESULT> {
    if pairs == 0 || pairs > 512 || input.is_empty() || input.len() > 8192 || !gate_offset.is_multiple_of(8) || !valid_range(device, gate, gate_offset, 8) {
        return Err(E_INVALIDARG);
    }
    let mut bytes = 0u64;
    let mut covered = vec![false; pairs as usize];
    for r in input {
        if r.pair >= pairs
            || r.bytes > 32 * 1024 * 1024
            || bytes > 512 * 1024 * 1024 - r.bytes
            || !valid_range(device, &r.resources[0], r.offsets[0], r.bytes)
            || !valid_range(device, &r.resources[1], r.offsets[1], r.bytes)
        {
            return Err(E_INVALIDARG);
        }
        bytes += r.bytes;
        covered[r.pair as usize] = true;
    }
    if covered.iter().any(|c| !c) {
        return Err(E_INVALIDARG);
    }
    let mut groups: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    for (i, r) in input.iter().enumerate() {
        groups
            .entry((r.resources[0].as_raw() as usize, r.resources[1].as_raw() as usize))
            .or_default()
            .push(i);
    }
    let mut metadata: Vec<[u32; 4]> = Vec::new();
    let mut batches = Vec::new();
    for indices in groups.values() {
        let first = metadata.len() as u32;
        for &i in indices {
            let r = &input[i];
            let mut offset = 0u64;
            while offset < r.bytes {
                metadata.push([
                    (r.offsets[0] + offset) as u32,
                    (r.offsets[1] + offset) as u32,
                    1024u64.min(r.bytes - offset) as u32,
                    r.pair,
                ]);
                offset += 1024;
            }
        }
        let resources = input[indices[0]].resources.clone();
        batches.push(Batch {
            resources,
            first,
            count: metadata.len() as u32 - first,
        });
    }
    let _internal = Internal::new();
    // SAFETY (whole block): the live device; descriptions on the stack outliving the calls.
    unsafe {
        let srv = |register: u32| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: register,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let parameters = [
            srv(0),
            srv(1),
            srv(2),
            srv(3),
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_UAV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                        Num32BitValues: 4,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: parameters.len() as u32,
            pParameters: parameters.as_ptr(),
            ..Default::default()
        };
        let (mut signature, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut signature, Some(&mut error)).map_err(|e| e.code())?;
        let signature = signature.ok_or(E_FAIL)?;
        let root: ID3D12RootSignature = device
            .CreateRootSignature(
                0,
                core::slice::from_raw_parts(signature.GetBufferPointer() as *const u8, signature.GetBufferSize()),
            )
            .map_err(|e| e.code())?;
        let mut pipelines = Vec::new();
        // the shaders made from Rust (rust/shaders/passes/src/single_pass.rs); the HLSL above with CVR_SHADER_HLSL
        for (entry, dxil) in [(c"Initialize", crate::dxil::GEOMETRY_COMPARE_INITIALIZE_CS), (c"Compare", crate::dxil::GEOMETRY_COMPARE_CS)] {
            let hlsl = || {
                let mut shader: Option<ID3DBlob> = None;
                D3DCompile(
                    SHADER.as_ptr() as *const c_void,
                    SHADER.len(),
                    PCSTR::null(),
                    None,
                    None,
                    PCSTR(entry.as_ptr() as *const u8),
                    PCSTR(c"cs_5_0".as_ptr() as *const u8),
                    D3DCOMPILE_OPTIMIZATION_LEVEL3,
                    0,
                    &mut shader,
                    Some(&mut error),
                )
                .ok()?;
                shader
            };
            let shader = crate::dxil::pick(dxil, hlsl).ok_or(E_FAIL)?;
            let pipeline = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: core::mem::transmute_copy(&root),
                CS: shader.bytecode(),
                ..Default::default()
            };
            let made: ID3D12PipelineState = device.CreateComputePipelineState(&pipeline).map_err(|e| e.code())?;
            pipelines.push(made);
        }
        let mut buffer = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: metadata.len() as u64 * 16,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            ..Default::default()
        };
        let mut meta: Option<ID3D12Resource> = None;
        device
            .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut meta)
            .map_err(|e| e.code())?;
        let meta = meta.ok_or(E_FAIL)?;
        let mut mapped: *mut c_void = core::ptr::null_mut();
        meta.Map(0, Some(&D3D12_RANGE::default()), Some(&mut mapped)).map_err(|e| e.code())?;
        if mapped.is_null() {
            return Err(E_FAIL);
        }
        core::ptr::copy_nonoverlapping(metadata.as_ptr() as *const u8, mapped as *mut u8, buffer.Width as usize);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        meta.Unmap(
            0,
            Some(&D3D12_RANGE {
                Begin: 0,
                End: buffer.Width as usize,
            }),
        );
        buffer.Width = pairs as u64 * 16;
        buffer.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        heap.Type = D3D12_HEAP_TYPE_DEFAULT;
        let mut predicates: Option<ID3D12Resource> = None;
        device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &buffer,
                D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                None,
                &mut predicates,
            )
            .map_err(|e| e.code())?;
        let compare = pipelines.pop().ok_or(E_FAIL)?;
        let initialize = pipelines.pop().ok_or(E_FAIL)?;
        let ranges = input
            .iter()
            .map(|r| Range {
                pair: r.pair,
                resources: r.resources.clone(),
                offsets: r.offsets,
                bytes: r.bytes,
            })
            .collect();
        Ok(Storage {
            device: device.clone(),
            root,
            initialize,
            compare,
            predicates: predicates.ok_or(E_FAIL)?,
            gate: gate.clone(),
            metadata: meta,
            batches,
            ranges,
            pairs,
            bytes,
            gate_offset,
            recorded: false,
        })
    }
}

impl GeometryCompare {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Storage>> {
        self.storage.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn create(&self, device: &ID3D12Device, pairs: u32, ranges: &[Range], gate: &ID3D12Resource, gate_offset: u64) -> HRESULT {
        let mut s = self.lock();
        if s.is_some() {
            return E_INVALIDARG;
        }
        match create(device, pairs, ranges, gate, gate_offset) {
            Ok(storage) => {
                *s = Some(storage);
                HRESULT(0)
            }
            Err(hr) => hr,
        }
    }
    /// Record: the single recording, on a direct list (changes the compute root and pipeline, clears predication).
    pub fn record(&self, list: &ID3D12GraphicsCommandList) -> bool {
        let mut g = self.lock();
        let Some(s) = g.as_mut() else { return false };
        // SAFETY (whole block): the recording list and the storage's live objects.
        unsafe {
            if s.recorded || list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT || !same_device(&s.device, list) {
                return false;
            }
            let mut keep: Vec<*mut c_void> = vec![
                s.root.as_raw(),
                s.initialize.as_raw(),
                s.compare.as_raw(),
                s.predicates.as_raw(),
                s.gate.as_raw(),
                s.metadata.as_raw(),
            ];
            for r in &s.ranges {
                keep.push(r.resources[0].as_raw());
                keep.push(r.resources[1].as_raw());
            }
            if !crate::command_resources::keep(list, &keep) {
                return false;
            }
            s.recorded = true;
            let _internal = Internal::new();
            list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            list.SetComputeRootSignature(&s.root);
            list.SetPipelineState(&s.initialize);
            list.SetComputeRootShaderResourceView(0, s.ranges[0].resources[0].GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(1, s.ranges[0].resources[1].GetGPUVirtualAddress());
            list.SetComputeRootShaderResourceView(2, s.gate.GetGPUVirtualAddress() + s.gate_offset);
            list.SetComputeRootShaderResourceView(3, s.metadata.GetGPUVirtualAddress());
            list.SetComputeRootUnorderedAccessView(4, s.predicates.GetGPUVirtualAddress());
            let initial = [0u32, 0, 0, s.pairs];
            list.SetComputeRoot32BitConstants(5, 4, initial.as_ptr() as *const c_void, 0);
            list.Dispatch(s.pairs.div_ceil(256), 1, 1);
            let uav = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                    UAV: core::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                        pResource: core::mem::transmute_copy(&s.predicates),
                    }),
                },
            };
            list.ResourceBarrier(&[uav]);
            list.SetPipelineState(&s.compare);
            for b in &s.batches {
                list.SetComputeRootShaderResourceView(0, b.resources[0].GetGPUVirtualAddress());
                list.SetComputeRootShaderResourceView(1, b.resources[1].GetGPUVirtualAddress());
                let width = b.count.min(1024);
                let values = [b.first, b.count, width, 0];
                list.SetComputeRoot32BitConstants(5, 4, values.as_ptr() as *const c_void, 0);
                // Atomic AND/OR from independent ranges commute; only initialisation and the final predication depend
                // on their order, not adjacent compare dispatches.
                list.Dispatch(width, b.count.div_ceil(width), 1);
            }
            let done = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                        // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                        pResource: core::mem::transmute_copy(&s.predicates),
                        Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                        StateBefore: D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                        StateAfter: D3D12_RESOURCE_STATE_PREDICATION | D3D12_RESOURCE_STATE_COPY_SOURCE | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                    }),
                },
            };
            list.ResourceBarrier(&[done]);
        }
        true
    }
    /// The predicates buffer (borrowed) and the pair, range and dispatch counts and compared bytes.
    pub fn info(&self) -> (*mut c_void, [u64; 4]) {
        self.lock().as_ref().map_or((core::ptr::null_mut(), [0; 4]), |s| {
            (
                s.predicates.as_raw(),
                [
                    s.pairs as u64,
                    s.ranges.len() as u64,
                    if s.recorded { s.batches.len() as u64 + 1 } else { 0 },
                    s.bytes,
                ],
            )
        })
    }
}

// ---- the C ABI (include/Render/GpuGeometryCompare.hpp)

#[no_mangle]
pub extern "C" fn cvr_geometry_compare_new() -> *mut GeometryCompare {
    Box::into_raw(Box::default())
}
/// # Safety
/// `c` came from cvr_geometry_compare_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_compare_free(c: *mut GeometryCompare) {
    if !c.is_null() {
        // SAFETY: the caller's object, freed once.
        drop(unsafe { Box::from_raw(c) });
    }
}
/// # Safety
/// `c` is live; `device`/`gate` D3D12 objects or null; `ranges` is `count` rows (null resources refused).
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_compare_create(
    c: *const GeometryCompare,
    device: *mut c_void,
    pairs: u32,
    ranges: *const RangeIn,
    count: usize,
    gate: *mut c_void,
    gate_offset: u64,
) -> HRESULT {
    // SAFETY: the caller's objects.
    unsafe {
        let (Some(c), Some(device), Some(gate)) = (c.as_ref(), ID3D12Device::from_raw_borrowed(&device), ID3D12Resource::from_raw_borrowed(&gate)) else {
            return E_INVALIDARG;
        };
        let rows = if ranges.is_null() || count == 0 {
            &[][..]
        } else {
            core::slice::from_raw_parts(ranges, count)
        };
        let mut list = Vec::with_capacity(rows.len());
        for r in rows {
            match (
                ID3D12Resource::from_raw_borrowed(&r.resources[0]),
                ID3D12Resource::from_raw_borrowed(&r.resources[1]),
            ) {
                (Some(a), Some(b)) => list.push(Range {
                    pair: r.pair,
                    resources: [a.clone(), b.clone()],
                    offsets: r.offsets,
                    bytes: r.bytes,
                }),
                _ => return E_INVALIDARG,
            }
        }
        c.create(device, pairs, &list, gate, gate_offset)
    }
}
/// # Safety
/// `c` is live; `list` an ID3D12GraphicsCommandList or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_compare_record(c: *const GeometryCompare, list: *mut c_void) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (c.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list)) {
            (Some(c), Some(l)) => c.record(l),
            _ => false,
        }
    }
}
/// # Safety
/// `c` is live; the out-pointers writable or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_geometry_compare_info(c: *const GeometryCompare, predicates: *mut *mut c_void, counts: *mut [u64; 4]) {
    // SAFETY: the caller's object and out-pointers.
    unsafe {
        let Some(c) = c.as_ref() else { return };
        let (p, n) = c.info();
        if let Some(o) = predicates.as_mut() {
            *o = p;
        }
        if let Some(o) = counts.as_mut() {
            *o = n;
        }
    }
}
