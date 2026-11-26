//! Single pass's finer shading-rate image (formerly src/Render/GpuFinerVrs.cpp): the engine's per-eye shading-rate
//! images (owned original-position snapshots in SHADING_RATE_SOURCE) reduced on the GPU to the finest rate of every
//! tile, so one image serves both eyes; a status word counts invalid rates. No source pixel is read on the CPU while
//! creating or recording; the sources' states are restored after the reduction. The shader stays HLSL.
use core::ffi::c_void;
use std::collections::HashSet;
use std::sync::Mutex;

use cvr_host::probe::Internal;
use windows::core::{Interface, HRESULT, PCSTR};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const MAX_SOURCES: u32 = 512;
/// E_PENDING (winerror.h).
const E_PENDING: HRESULT = HRESULT(0x8000000Au32 as i32);
const D3DCOMPILE_OPTIMIZATION_LEVEL3: u32 = 1 << 15;
const SHADER: &str = r#"
Texture2D<uint> inputs[512] : register(t0);
RWTexture2D<uint> outputRate : register(u0);
RWByteAddressBuffer status : register(u1);
cbuffer Parameters : register(b0) { uint width,height,count,padding; };
[numthreads(1,1,1)] void Initialize() { status.Store2(0,uint2(0,0)); }
[numthreads(8,8,1)] void Reduce(uint3 id : SV_DispatchThreadID) {
    if(id.x>=width || id.y>=height)return;
    uint2 finest=uint2(2,2);
    for(uint i=0;i<count;++i) {
        uint rate=inputs[i].Load(int3(id.xy,0));
        bool valid=rate==0 || rate==1 || rate==4 || rate==5 || rate==6 || rate==9 || rate==10;
        if(!valid) { uint ignored;status.InterlockedOr(0,1,ignored);rate=0; }
        finest=min(finest,uint2(rate>>2,rate&3));
    }
    outputRate[id.xy]=(finest.x<<2)|finest.y;
}
"#;

fn same_device(device: &ID3D12Device, child: &ID3D12DeviceChild) -> bool {
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: the live child.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: 0,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

struct Storage {
    root: ID3D12RootSignature,
    initialize: ID3D12PipelineState,
    reduce: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    image: ID3D12Resource,
    status: ID3D12Resource,
    readback: ID3D12Resource,
    sources: Vec<ID3D12Resource>,
    device: ID3D12Device,
    footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    width: u32,
    height: u32,
    stride: u32,
    status_offset: u64,
    bytes: u64,
    recorded: bool,
}
// SAFETY: free-threaded D3D12 objects under the mutex.
unsafe impl Send for Storage {}

/// GpuFinerVrs.
#[derive(Default)]
pub struct FinerVrs {
    storage: Mutex<Option<Storage>>,
}

fn create(device: &ID3D12Device, images: &[ID3D12Resource]) -> Result<Storage, HRESULT> {
    // SAFETY (whole body): the live device and images; descriptions on the stack outliving the calls.
    unsafe {
        if device.GetNodeCount() != 1 || images.is_empty() || images.len() > MAX_SOURCES as usize {
            return Err(E_INVALIDARG);
        }
        let (mut width, mut height) = (0u32, 0u32);
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for image in images {
            let child: ID3D12DeviceChild = image.cast().map_err(|_| E_INVALIDARG)?;
            if !same_device(device, &child) {
                return Err(E_INVALIDARG);
            }
            let d = image.GetDesc();
            if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || d.Format != DXGI_FORMAT_R8_UINT
                || d.MipLevels != 1
                || d.DepthOrArraySize != 1
                || d.SampleDesc.Count != 1
                || d.SampleDesc.Quality != 0
                || d.Width == 0
                || d.Height == 0
                || d.Width > 512
                || d.Height > 512
                || (d.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE).0 != 0
            {
                return Err(E_INVALIDARG);
            }
            if width != 0 && (d.Width != width as u64 || d.Height != height) {
                return Err(E_INVALIDARG);
            }
            width = d.Width as u32;
            height = d.Height;
            if seen.insert(image.as_raw() as usize) {
                sources.push(image.clone());
            }
        }
        let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
            Format: DXGI_FORMAT_R8_UINT,
            ..Default::default()
        };
        if device
            .CheckFeatureSupport(
                D3D12_FEATURE_FORMAT_SUPPORT,
                &mut support as *mut _ as *mut c_void,
                core::mem::size_of::<D3D12_FEATURE_DATA_FORMAT_SUPPORT>() as u32,
            )
            .is_err()
            || (support.Support1 & D3D12_FORMAT_SUPPORT1_SHADER_LOAD).0 == 0
            || (support.Support2 & D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE).0 == 0
        {
            return Err(E_NOTIMPL);
        }
        let _internal = Internal::new();
        let ranges = [
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
                NumDescriptors: MAX_SOURCES,
                ..Default::default()
            },
            D3D12_DESCRIPTOR_RANGE {
                RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
                NumDescriptors: 1,
                ..Default::default()
            },
        ];
        let table = |r: &D3D12_DESCRIPTOR_RANGE| D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: r,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        };
        let parameters = [
            table(&ranges[0]),
            table(&ranges[1]),
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_UAV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 1,
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
        let root_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: 4,
            pParameters: parameters.as_ptr(),
            ..Default::default()
        };
        let (mut serialized, mut error): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
        D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut serialized, Some(&mut error)).map_err(|e| e.code())?;
        let serialized = serialized.ok_or(E_FAIL)?;
        let root: ID3D12RootSignature = device
            .CreateRootSignature(
                0,
                core::slice::from_raw_parts(serialized.GetBufferPointer() as *const u8, serialized.GetBufferSize()),
            )
            .map_err(|e| e.code())?;
        let mut pipelines = Vec::new();
        // the shaders made from Rust (rust/shaders/passes/src/finer_vrs.rs); the HLSL above with CVR_SHADER_HLSL
        for (entry, dxil) in [(c"Initialize", crate::dxil::FINER_VRS_INITIALIZE_CS), (c"Reduce", crate::dxil::FINER_VRS_REDUCE_CS)] {
            let hlsl = || {
                let mut shader: Option<ID3DBlob> = None;
                D3DCompile(
                    SHADER.as_ptr() as *const c_void,
                    SHADER.len(),
                    PCSTR::null(),
                    None,
                    None,
                    PCSTR(entry.as_ptr() as *const u8),
                    PCSTR(c"cs_5_1".as_ptr() as *const u8),
                    D3DCOMPILE_OPTIMIZATION_LEVEL3,
                    0,
                    &mut shader,
                    Some(&mut error),
                )
                .ok()?;
                shader
            };
            let shader = crate::dxil::pick(dxil, hlsl).ok_or(E_FAIL)?;
            let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: core::mem::transmute_copy(&root),
                CS: shader.bytecode(),
                ..Default::default()
            };
            let pipeline: ID3D12PipelineState = device.CreateComputePipelineState(&desc).map_err(|e| e.code())?;
            pipelines.push(pipeline);
        }
        let mut desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Format: DXGI_FORMAT_R8_UINT,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let mut memory = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let make = |memory: &D3D12_HEAP_PROPERTIES, desc: &D3D12_RESOURCE_DESC, state| -> Result<ID3D12Resource, HRESULT> {
            let mut r: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(memory, D3D12_HEAP_FLAG_NONE, desc, state, None, &mut r)
                .map_err(|e| e.code())?;
            r.ok_or(E_FAIL)
        };
        let image = make(&memory, &desc, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)?;
        let mut footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let mut total = 0u64;
        device.GetCopyableFootprints(&desc, 0, 1, 0, Some(&mut footprint), None, None, Some(&mut total));
        if total == u64::MAX || total > 1024 * 1024 {
            return Err(E_INVALIDARG);
        }
        let status_offset = (total + 7) & !7;
        let bytes = status_offset + 8;
        desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 8,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let status = make(&memory, &desc, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)?;
        desc.Width = bytes;
        desc.Flags = D3D12_RESOURCE_FLAG_NONE;
        memory.Type = D3D12_HEAP_TYPE_READBACK;
        let readback = make(&memory, &desc, D3D12_RESOURCE_STATE_COPY_DEST)?;
        let heap_desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: MAX_SOURCES + 1,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        let heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&heap_desc).map_err(|e| e.code())?;
        let stride = device.GetDescriptorHandleIncrementSize(heap_desc.Type);
        let start = heap.GetCPUDescriptorHandleForHeapStart();
        let srv = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R8_UINT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        for i in 0..MAX_SOURCES as usize {
            let handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: start.ptr + i * stride as usize,
            };
            device.CreateShaderResourceView(sources.get(i), Some(&srv), handle);
        }
        let uav = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R8_UINT,
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        device.CreateUnorderedAccessView(
            &image,
            None,
            Some(&uav),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: start.ptr + MAX_SOURCES as usize * stride as usize,
            },
        );
        let reduce = pipelines.pop().ok_or(E_FAIL)?;
        let initialize = pipelines.pop().ok_or(E_FAIL)?;
        Ok(Storage {
            root,
            initialize,
            reduce,
            heap,
            image,
            status,
            readback,
            sources,
            device: device.clone(),
            footprint,
            width,
            height,
            stride,
            status_offset,
            bytes,
            recorded: false,
        })
    }
}

impl FinerVrs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Storage>> {
        self.storage.lock().unwrap_or_else(|p| p.into_inner())
    }
    /// Create: the reduction of `images` (same size, R8_UINT, at most 512x512, at most 512 of them).
    pub fn create(&self, device: &ID3D12Device, images: &[ID3D12Resource]) -> HRESULT {
        let mut s = self.lock();
        if s.is_some() {
            return E_INVALIDARG;
        }
        match create(device, images) {
            Ok(storage) => {
                *s = Some(storage);
                HRESULT(0)
            }
            Err(hr) => hr,
        }
    }
    /// Record: the single-use reduction on a DIRECT list (changes its heaps and compute bindings).
    pub fn record(&self, list: &ID3D12GraphicsCommandList) -> bool {
        let mut g = self.lock();
        let Some(s) = g.as_mut() else { return false };
        // SAFETY (whole body): the recording list and the storage's live objects.
        unsafe {
            if s.recorded || list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT {
                return false;
            }
            let Ok(child) = list.cast::<ID3D12DeviceChild>() else { return false };
            if !same_device(&s.device, &child) {
                return false;
            }
            let mut keep: Vec<*mut c_void> = vec![
                s.root.as_raw(),
                s.initialize.as_raw(),
                s.reduce.as_raw(),
                s.heap.as_raw(),
                s.image.as_raw(),
                s.status.as_raw(),
                s.readback.as_raw(),
            ];
            keep.extend(s.sources.iter().map(Interface::as_raw));
            if !crate::command_resources::keep(list, &keep) {
                return false;
            }
            let mut barriers: Vec<D3D12_RESOURCE_BARRIER> = s
                .sources
                .iter()
                .map(|i| transition(i, D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE))
                .collect();
            let _internal = Internal::new();
            s.recorded = true;
            list.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            list.ResourceBarrier(&barriers);
            list.SetDescriptorHeaps(&[Some(s.heap.clone())]);
            list.SetComputeRootSignature(&s.root);
            let start = s.heap.GetGPUDescriptorHandleForHeapStart();
            list.SetComputeRootDescriptorTable(0, start);
            list.SetComputeRootDescriptorTable(
                1,
                D3D12_GPU_DESCRIPTOR_HANDLE {
                    ptr: start.ptr + MAX_SOURCES as u64 * s.stride as u64,
                },
            );
            list.SetComputeRootUnorderedAccessView(2, s.status.GetGPUVirtualAddress());
            let values = [s.width, s.height, s.sources.len() as u32, 0];
            list.SetComputeRoot32BitConstants(3, 4, values.as_ptr() as *const c_void, 0);
            list.SetPipelineState(&s.initialize);
            list.Dispatch(1, 1, 1);
            let status = D3D12_RESOURCE_BARRIER {
                Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
                Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                Anonymous: D3D12_RESOURCE_BARRIER_0 {
                    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                    UAV: core::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                        pResource: core::mem::transmute_copy(&s.status),
                    }),
                },
            };
            list.ResourceBarrier(&[status]);
            list.SetPipelineState(&s.reduce);
            list.Dispatch(s.width.div_ceil(8), s.height.div_ceil(8), 1);
            for b in barriers.iter_mut() {
                let t = &mut *b.Anonymous.Transition;
                core::mem::swap(&mut t.StateBefore, &mut t.StateAfter);
            }
            barriers.push(transition(&s.image, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE));
            barriers.push(transition(&s.status, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE));
            list.ResourceBarrier(&barriers);
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(&s.image),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(&s.readback),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: s.footprint },
            };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            list.CopyBufferRegion(&s.readback, s.status_offset, &s.status, 0, 8);
            list.ResourceBarrier(&[transition(&s.image, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE)]);
        }
        true
    }
    pub fn image(&self) -> *mut c_void {
        self.lock().as_ref().map_or(core::ptr::null_mut(), |s| s.image.as_raw())
    }
    pub fn source_count(&self) -> u32 {
        self.lock().as_ref().map_or(0, |s| s.sources.len() as u32)
    }
    pub fn dispatch_count(&self) -> u32 {
        self.lock().as_ref().map_or(0, |s| if s.recorded { 2 } else { 0 })
    }
    pub fn readback_bytes(&self) -> u64 {
        self.lock().as_ref().map_or(0, |s| s.bytes)
    }
    pub fn pixel_count(&self) -> usize {
        self.lock().as_ref().map_or(0, |s| s.width as usize * s.height as usize)
    }
    /// Read (diagnostic, after the consumer's submission fence): the reduced pixels and the invalid rates seen.
    pub fn read(&self, out: &mut [u8], invalid: &mut u32) -> HRESULT {
        *invalid = 0;
        let g = self.lock();
        let Some(s) = g.as_ref().filter(|s| s.recorded) else { return E_PENDING };
        if out.len() != s.width as usize * s.height as usize {
            return E_INVALIDARG;
        }
        let mut mapped: *mut c_void = core::ptr::null_mut();
        let range = D3D12_RANGE {
            Begin: 0,
            End: s.bytes as usize,
        };
        // SAFETY (whole block): the readback buffer the GPU has finished; the mapped range holds what the reads cover.
        unsafe {
            if let Err(e) = s.readback.Map(0, Some(&range), Some(&mut mapped)) {
                return e.code();
            }
            if mapped.is_null() {
                return E_FAIL;
            }
            let base = mapped as *const u8;
            for y in 0..s.height as usize {
                core::ptr::copy_nonoverlapping(
                    base.add(s.footprint.Offset as usize + y * s.footprint.Footprint.RowPitch as usize),
                    out.as_mut_ptr().add(y * s.width as usize),
                    s.width as usize,
                );
            }
            let errors = (base.add(s.status_offset as usize) as *const u64).read_unaligned();
            s.readback.Unmap(0, Some(&D3D12_RANGE::default()));
            if errors > 1 {
                return E_FAIL;
            }
            *invalid = errors as u32;
        }
        HRESULT(0)
    }
}

// ---- the C ABI (include/Render/GpuFinerVrs.hpp)

#[no_mangle]
pub extern "C" fn cvr_finer_vrs_new() -> *mut FinerVrs {
    Box::into_raw(Box::default())
}
/// # Safety
/// `v` came from cvr_finer_vrs_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_finer_vrs_free(v: *mut FinerVrs) {
    if !v.is_null() {
        // SAFETY: the caller's object, freed once.
        drop(unsafe { Box::from_raw(v) });
    }
}
/// # Safety
/// `v` is live; `device` an ID3D12Device or null; `images` is `count` ID3D12Resource pointers (nulls refused).
#[no_mangle]
pub unsafe extern "C" fn cvr_finer_vrs_create(v: *const FinerVrs, device: *mut c_void, images: *const *mut c_void, count: usize) -> HRESULT {
    // SAFETY: the caller's objects.
    unsafe {
        let (Some(v), Some(device)) = (v.as_ref(), ID3D12Device::from_raw_borrowed(&device)) else {
            return E_INVALIDARG;
        };
        let raw = if images.is_null() || count == 0 {
            &[][..]
        } else {
            core::slice::from_raw_parts(images, count)
        };
        let mut list = Vec::with_capacity(raw.len());
        for p in raw {
            match ID3D12Resource::from_raw_borrowed(p) {
                Some(r) => list.push(r.clone()),
                None => return E_INVALIDARG,
            }
        }
        v.create(device, &list)
    }
}
/// # Safety
/// `v` is live; `list` an ID3D12GraphicsCommandList or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_finer_vrs_record(v: *const FinerVrs, list: *mut c_void) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (v.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list)) {
            (Some(v), Some(l)) => v.record(l),
            _ => false,
        }
    }
}
/// The image, source count, dispatch count, readback bytes and pixel count.
///
/// # Safety
/// `v` is live; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_finer_vrs_info(v: *const FinerVrs, image: *mut *mut c_void, counts: *mut [u64; 4]) {
    // SAFETY: the caller's object and out-pointers.
    unsafe {
        let Some(v) = v.as_ref() else { return };
        if let Some(o) = image.as_mut() {
            *o = v.image();
        }
        if let Some(o) = counts.as_mut() {
            *o = [v.source_count() as u64, v.dispatch_count() as u64, v.readback_bytes(), v.pixel_count() as u64];
        }
    }
}
/// # Safety
/// `v` is live; `out` is `len` writable bytes; `invalid` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_finer_vrs_read(v: *const FinerVrs, out: *mut u8, len: usize, invalid: *mut u32) -> HRESULT {
    // SAFETY: the caller's object and buffers.
    unsafe {
        let Some(v) = v.as_ref() else { return E_PENDING };
        let mut bad = 0u32;
        let out = if out.is_null() {
            &mut [][..]
        } else {
            core::slice::from_raw_parts_mut(out, len)
        };
        let hr = v.read(out, &mut bad);
        if let Some(i) = invalid.as_mut() {
            *i = bad;
        }
        hr
    }
}
