//! Single pass's depth commit gate (formerly src/Render/DepthCommitGate.cpp): before a predicated paired draw, the GPU
//! compares the two layers of the initial depth/stencil array, and every byte of each declared buffer proof (MAIN's and
//! the peer's owned snapshots), and clears the decision the CPU wrote when anything differs. The predicated work then
//! runs only on an equal start. The caller serialises recording and admission and reads only after its queue fence.
//!
//! Saving and restoring the list's graphics bindings around the gate's compute work is StereoSceneState's (still C++),
//! attached here as two functions over an opaque buffer of its Bindings.
use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::core::{Interface, HRESULT, PCSTR};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_UNEXPECTED, S_OK};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

pub const MAX_BUFFER_PROOFS: usize = 256;

const DEPTH_SHADER: &str = r#"
Texture2DArray<float> Depth:register(t0);
Texture2DArray<uint2> Stencil:register(t1);
RWByteAddressBuffer Decision:register(u0);
[numthreads(16,16,1)] void CS(uint3 id:SV_DispatchThreadID) {
    uint w,h,n;Depth.GetDimensions(w,h,n);if(id.x>=w || id.y>=h)return;
    int4 a=int4(id.xy,0,0),b=int4(id.xy,1,0);
    if(asuint(Depth.Load(a))!=asuint(Depth.Load(b)) || Stencil.Load(a).y!=Stencil.Load(b).y){
        uint ignored;Decision.InterlockedExchange(0,0,ignored);
    }
}"#;
const BUFFER_SHADER: &str = r#"
ByteAddressBuffer Main:register(t0);
ByteAddressBuffer Peer:register(t1);
RWByteAddressBuffer Decision:register(u0);
cbuffer Size:register(b0){uint Words;uint RowBytes;uint RowPitch;uint Rows;};
[numthreads(64,1,1)] void CS(uint3 id:SV_DispatchThreadID) {
    if(id.x>=Words || id.y>=Rows)return;
    uint at=id.y*RowPitch+id.x*4,remaining=RowBytes-id.x*4;
    uint mask=remaining>=4?0xffffffff:((1u<<(remaining*8))-1);
    if(((Main.Load(at)^Peer.Load(at))&mask)!=0){
        uint ignored;Decision.InterlockedExchange(0,0,ignored);
    }
}"#;

/// DepthCommitGate::BufferProof: owned, immutable snapshots in NON_PIXEL_SHADER_RESOURCE. Zero row fields mean one
/// contiguous DWORD-aligned range; texture snapshots may give a row layout to leave copy-footprint padding out.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BufferProof {
    pub main: *mut c_void,
    pub peer: *mut c_void,
    pub bytes: u32,
    pub row_bytes: u32,
    pub row_pitch: u32,
    pub rows: u32,
    pub main_offset: u32,
    pub peer_offset: u32,
}
const _: () = assert!(core::mem::size_of::<BufferProof>() == 40);

/// What the C++ half attaches: StereoSceneState's Bindings (size, snapshot into a buffer, restore from it).
#[repr(C)]
pub struct Attach {
    pub bindings_bytes: usize,
    pub snapshot: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    pub restore: unsafe extern "C" fn(*const c_void, *mut c_void),
}
static ATTACHED: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own table (formerly lent by src/Render/DepthCommitGate.cpp): scene_state.rs' Bindings. The Rust test
/// variants attach theirs.
#[cfg(all(feature = "plugin-tables", not(test)))]
mod plugin {
    use super::Attach;
    use crate::scene_state::{cvr_scene_bindings_restore, cvr_scene_snapshot, Bindings};
    use core::ffi::c_void;
    unsafe extern "C" fn snapshot(list: *mut c_void, storage: *mut c_void) -> bool {
        let out = storage.cast::<Bindings>();
        // SAFETY: the caller's storage of bindings_bytes (a fresh record, as the .cpp's placement new).
        unsafe {
            out.write(Bindings::default());
            cvr_scene_snapshot(list as usize, out)
        }
    }
    unsafe extern "C" fn restore(storage: *const c_void, list: *mut c_void) {
        // SAFETY: a snapshot's storage and the live list.
        unsafe { cvr_scene_bindings_restore(storage.cast(), list) };
    }
    pub static TABLE: Attach = Attach { bindings_bytes: core::mem::size_of::<Bindings>(), snapshot, restore };
}
fn attached() -> Option<&'static Attach> {
    // SAFETY: the attached table (a static of the C++ half).
    let table = unsafe { ATTACHED.load(Ordering::Acquire).as_ref() };
    #[cfg(all(feature = "plugin-tables", not(test)))]
    let table = table.or(Some(&plugin::TABLE));
    table
}
/// # Safety
/// `table` lives for the rest of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_depth_gate_attach(table: *const Attach) {
    ATTACHED.store(table.cast_mut(), Ordering::Release);
}

struct Storage {
    device: ID3D12Device,
    initial: ID3D12Resource,
    upload: ID3D12Resource,
    decision: ID3D12Resource,
    readback: ID3D12Resource,
    root: ID3D12RootSignature,
    pipeline: ID3D12PipelineState,
    heap: ID3D12DescriptorHeap,
    buffer_root: ID3D12RootSignature,
    buffer_pipeline: ID3D12PipelineState,
    mapped: *mut c_void,
    width: u32,
    height: u32,
    prepared: bool,
    frozen: bool,
    copied: bool,
    reused: bool,
}
impl Drop for Storage {
    fn drop(&mut self) {
        if !self.mapped.is_null() {
            let written = D3D12_RANGE { Begin: 0, End: 8 };
            // SAFETY: the upload buffer this storage mapped.
            unsafe { self.upload.Unmap(0, Some(&written)) };
        }
    }
}

/// DepthCommitGate.
#[derive(Default)]
pub struct Gate {
    storage: Option<Storage>,
}

fn compile(source: &str) -> Result<ID3DBlob, HRESULT> {
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source's bytes and length, NUL-terminated entry and target, out-blobs.
    unsafe {
        D3DCompile(
            source.as_ptr() as *const c_void,
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(c"CS".as_ptr().cast()),
            PCSTR(c"cs_5_0".as_ptr().cast()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    }
    .map_err(|e| e.code())?;
    code.ok_or(E_FAIL)
}
fn root_signature(device: &ID3D12Device, params: &[D3D12_ROOT_PARAMETER]) -> Result<ID3D12RootSignature, HRESULT> {
    let desc = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: params.len() as u32,
        pParameters: params.as_ptr(),
        NumStaticSamplers: 0,
        pStaticSamplers: core::ptr::null(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    let mut blob: Option<ID3DBlob> = None;
    // SAFETY: the description and its parameters on the caller's stack.
    unsafe {
        D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).map_err(|e| e.code())?;
        let blob = blob.ok_or(E_FAIL)?;
        device
            .CreateRootSignature(0, core::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()))
            .map_err(|e| e.code())
    }
}
fn compute_pipeline(device: &ID3D12Device, root: &ID3D12RootSignature, code: &crate::dxil::Code) -> Result<ID3D12PipelineState, HRESULT> {
    // SAFETY: the live device; the root signature borrowed without a reference for the call.
    unsafe {
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(root),
            CS: code.bytecode(),
            ..Default::default()
        };
        device.CreateComputePipelineState(&desc).map_err(|e| e.code())
    }
}
fn buffer(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource, HRESULT> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: 8,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: flags,
        ..Default::default()
    };
    let properties = D3D12_HEAP_PROPERTIES {
        Type: heap,
        ..Default::default()
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: the live device; descriptions on the stack.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut resource) }.map_err(|e| e.code())?;
    resource.ok_or(E_FAIL)
}
fn device_of<T: Interface>(object: &T) -> Option<ID3D12Device> {
    let child: ID3D12DeviceChild = object.cast().ok()?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child writing its device.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
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
fn keep(list: &ID3D12GraphicsCommandList, objects: &[*mut c_void]) -> bool {
    crate::command_resources::keep(list, objects)
}

impl Storage {
    /// Every object the gate's recorded work uses, kept for as long as the list's submissions (KeepCommandResources
    /// of the C++ storage object).
    fn objects(&self) -> [*mut c_void; 9] {
        [
            self.initial.as_raw(),
            self.upload.as_raw(),
            self.decision.as_raw(),
            self.readback.as_raw(),
            self.root.as_raw(),
            self.pipeline.as_raw(),
            self.heap.as_raw(),
            self.buffer_root.as_raw(),
            self.buffer_pipeline.as_raw(),
        ]
    }
}

impl Gate {
    /// Create: once, for the initial depth array (R32G8X24, two layers, up to 4096 square).
    pub fn create(&mut self, device: &ID3D12Device, initial: &ID3D12Resource) -> HRESULT {
        if self.storage.is_some() {
            return E_INVALIDARG;
        }
        // SAFETY: a live resource.
        let d = unsafe { initial.GetDesc() };
        if device_of(initial).is_none_or(|a| a.as_raw() != device.as_raw())
            || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || d.DepthOrArraySize != 2
            || d.MipLevels != 1
            || d.Width == 0
            || d.Width > 4096
            || d.Height == 0
            || d.Height > 4096
            || d.Format != DXGI_FORMAT_R32G8X24_TYPELESS
            || d.SampleDesc.Count != 1
            || d.SampleDesc.Quality != 0
            || (d.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) != D3D12_RESOURCE_FLAG_NONE
        {
            return E_INVALIDARG;
        }
        let _internal = cvr_host::probe::Internal::new();
        match Self::build(device, initial, d.Width as u32, d.Height) {
            Ok(s) => {
                self.storage = Some(s);
                S_OK
            }
            Err(e) => e,
        }
    }
    fn build(device: &ID3D12Device, initial: &ID3D12Resource, width: u32, height: u32) -> Result<Storage, HRESULT> {
        // the shaders made from Rust (rust/shaders/passes/src/single_pass.rs); the HLSL above with CVR_SHADER_HLSL
        let code = crate::dxil::pick(crate::dxil::DEPTH_COMMIT_GATE_DEPTH_CS, || compile(DEPTH_SHADER).ok()).ok_or(E_FAIL)?;
        let range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 2,
            BaseShaderRegister: 0,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };
        let descriptor = |register: u32| D3D12_ROOT_PARAMETER_0 {
            Descriptor: D3D12_ROOT_DESCRIPTOR {
                ShaderRegister: register,
                RegisterSpace: 0,
            },
        };
        let params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_UAV,
                Anonymous: descriptor(0),
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
        ];
        let root = root_signature(device, &params)?;
        let pipeline = compute_pipeline(device, &root, &code)?;
        let code = crate::dxil::pick(crate::dxil::DEPTH_COMMIT_GATE_BUFFER_CS, || compile(BUFFER_SHADER).ok()).ok_or(E_FAIL)?;
        let buffer_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: descriptor(0),
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: descriptor(1),
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_UAV,
                Anonymous: descriptor(0),
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
        let buffer_root = root_signature(device, &buffer_params)?;
        let buffer_pipeline = compute_pipeline(device, &buffer_root, &code)?;
        let hd = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 2,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        // SAFETY (block): the live device and resource; descriptions on the stack.
        let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&hd) }.map_err(|e| e.code())?;
        unsafe {
            let mut handle = heap.GetCPUDescriptorHandleForHeapStart();
            let stride = device.GetDescriptorHandleIncrementSize(hd.Type) as usize;
            for plane in 0..2u32 {
                let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
                    Format: if plane != 0 {
                        DXGI_FORMAT_X32_TYPELESS_G8X24_UINT
                    } else {
                        DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS
                    },
                    ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
                    Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                    Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                            MipLevels: 1,
                            ArraySize: 2,
                            PlaneSlice: plane,
                            ..Default::default()
                        },
                    },
                };
                device.CreateShaderResourceView(initial, Some(&view), handle);
                handle.ptr += stride;
            }
        }
        let upload = buffer(device, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_GENERIC_READ)?;
        let readback = buffer(device, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST)?;
        let decision = buffer(
            device,
            D3D12_HEAP_TYPE_DEFAULT,
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_COPY_DEST,
        )?;
        let mut mapped = core::ptr::null_mut();
        let none = D3D12_RANGE { Begin: 0, End: 0 };
        // SAFETY: the new upload buffer, mapped for the gate's lifetime.
        unsafe { upload.Map(0, Some(&none), Some(&mut mapped)) }.map_err(|e| e.code())?;
        if mapped.is_null() {
            return Err(E_FAIL);
        }
        // SAFETY: eight mapped bytes.
        unsafe { core::ptr::write_bytes(mapped as *mut u8, 0, 8) };
        Ok(Storage {
            device: device.clone(),
            initial: initial.clone(),
            upload,
            decision,
            readback,
            root,
            pipeline,
            heap,
            buffer_root,
            buffer_pipeline,
            mapped,
            width,
            height,
            prepared: false,
            frozen: false,
            copied: false,
            reused: false,
        })
    }

    /// Prepare: once per admission, the comparisons recorded before the draw (`pipeline` restored after them).
    pub fn prepare(&mut self, list: &ID3D12GraphicsCommandList, flags: u32, pipeline: *mut c_void, proofs: &[BufferProof]) -> bool {
        let Some(s) = self.storage.as_mut() else { return false };
        // SAFETY: a live list.
        if s.prepared || flags & 64 != 0 || pipeline.is_null() || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT {
            return false;
        }
        if proofs.len() > MAX_BUFFER_PROOFS {
            return false;
        }
        let Some(t) = attached() else {
            return false;
        };
        let mut saved = vec![0u64; t.bindings_bytes.div_ceil(8)];
        let Some(device) = device_of(list) else { return false };
        // SAFETY: the C++ half's snapshot into a buffer of its Bindings' size (8-aligned).
        if device.as_raw() != s.device.as_raw() || !unsafe { (t.snapshot)(list.as_raw(), saved.as_mut_ptr().cast()) } || !keep(list, &s.objects()) {
            return false;
        }
        let mut total = 0u32;
        for proof in proofs {
            if proof.bytes == 0 || proof.bytes % 4 != 0 || proof.bytes > 8 * 1024 * 1024 || proof.bytes > 32 * 1024 * 1024 - total {
                return false;
            }
            total += proof.bytes;
            if proof.row_bytes != 0 || proof.row_pitch != 0 || proof.rows != 0 {
                let row = (proof.row_bytes as u64).div_ceil(4) * 4;
                if proof.row_bytes == 0
                    || proof.row_bytes > proof.bytes
                    || proof.rows == 0
                    || proof.rows > 65535
                    || proof.row_pitch % 4 != 0
                    || (proof.row_pitch as u64) < row
                    || (proof.rows as u64 - 1) * proof.row_pitch as u64 + row > proof.bytes as u64
                {
                    return false;
                }
            }
            if proof.main_offset % 16 != 0 || proof.peer_offset % 16 != 0 {
                return false;
            }
            for (source, offset) in [(proof.main, proof.main_offset), (proof.peer, proof.peer_offset)] {
                // SAFETY: the caller's live resource (or null), borrowed for the call.
                let Some(source) = (unsafe { ID3D12Resource::from_raw_borrowed(&source) }) else {
                    return false;
                };
                if device_of(source).is_none_or(|a| a.as_raw() != device.as_raw()) {
                    return false;
                }
                // SAFETY: a live resource.
                let desc = unsafe { source.GetDesc() };
                if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
                    || offset as u64 > desc.Width
                    || proof.bytes as u64 > desc.Width - offset as u64
                    || (desc.Flags & D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) != D3D12_RESOURCE_FLAG_NONE
                    || unsafe { source.GetGPUVirtualAddress() } == 0
                {
                    return false;
                }
            }
        }
        if proofs.iter().any(|p| !keep(list, &[p.main, p.peer])) {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY (block): the open list; the gate's objects are kept for its submissions.
        unsafe {
            if s.reused {
                list.ResourceBarrier(&[transition(&s.decision, D3D12_RESOURCE_STATE_PREDICATION, D3D12_RESOURCE_STATE_COPY_DEST)]);
            }
            list.CopyBufferRegion(&s.decision, 0, &s.upload, 0, 8);
            list.ResourceBarrier(&[
                transition(&s.decision, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_UNORDERED_ACCESS),
                transition(&s.initial, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE),
            ]);
            list.SetPipelineState(&s.pipeline);
            list.SetComputeRootSignature(&s.root);
            list.SetDescriptorHeaps(&[Some(s.heap.clone())]);
            list.SetComputeRootDescriptorTable(0, s.heap.GetGPUDescriptorHandleForHeapStart());
            list.SetComputeRootUnorderedAccessView(1, s.decision.GetGPUVirtualAddress());
            list.Dispatch(s.width.div_ceil(16), s.height.div_ceil(16), 1);
            for proof in proofs {
                let uav = D3D12_RESOURCE_BARRIER {
                    Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
                    Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                    Anonymous: D3D12_RESOURCE_BARRIER_0 {
                        UAV: core::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                            pResource: core::mem::transmute_copy(&s.decision),
                        }),
                    },
                };
                list.ResourceBarrier(&[uav]);
                list.SetPipelineState(&s.buffer_pipeline);
                list.SetComputeRootSignature(&s.buffer_root);
                let (main, peer) = (ID3D12Resource::from_raw_borrowed(&proof.main), ID3D12Resource::from_raw_borrowed(&proof.peer));
                let address = |r: Option<&ID3D12Resource>| r.map_or(0, |r| r.GetGPUVirtualAddress());
                list.SetComputeRootShaderResourceView(0, address(main) + proof.main_offset as u64);
                list.SetComputeRootShaderResourceView(1, address(peer) + proof.peer_offset as u64);
                list.SetComputeRootUnorderedAccessView(2, s.decision.GetGPUVirtualAddress());
                let row_bytes = if proof.row_bytes != 0 { proof.row_bytes } else { proof.bytes };
                let layout = [
                    row_bytes.div_ceil(4),
                    row_bytes,
                    if proof.row_bytes != 0 { proof.row_pitch } else { proof.bytes },
                    if proof.row_bytes != 0 { proof.rows } else { 1 },
                ];
                list.SetComputeRoot32BitConstants(3, 4, layout.as_ptr().cast(), 0);
                list.Dispatch(layout[0].div_ceil(64), layout[3], 1);
            }
            list.ResourceBarrier(&[
                transition(&s.decision, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_PREDICATION),
                transition(&s.initial, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_DEPTH_WRITE),
            ]);
            (t.restore)(saved.as_ptr().cast(), list.as_raw());
            list.SetPipelineState(ID3D12PipelineState::from_raw_borrowed(&pipeline));
        }
        s.prepared = true;
        true
    }

    /// Decide: the CPU's admission, written for the next prepared comparison (false once frozen).
    pub fn decide(&mut self, allow: bool) -> bool {
        let Some(s) = self.storage.as_ref().filter(|s| !s.frozen) else { return false };
        // SAFETY: the mapped upload buffer's first eight bytes.
        unsafe {
            (s.mapped as *mut u64).write_volatile(allow as u64);
            core::arch::x86_64::_mm_sfence();
        }
        true
    }
    pub fn freeze(&mut self) {
        if let Some(s) = self.storage.as_mut() {
            s.frozen = true;
        }
    }
    /// Bind: the list's predication on the decision (`optimized`: work that runs on admission).
    pub fn bind(&self, list: &ID3D12GraphicsCommandList, optimized: bool) -> bool {
        let Some(s) = self.storage.as_ref().filter(|s| s.prepared) else {
            return false;
        };
        if !keep(list, &s.objects()) {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        let op = if optimized {
            D3D12_PREDICATION_OP_EQUAL_ZERO
        } else {
            D3D12_PREDICATION_OP_NOT_EQUAL_ZERO
        };
        // SAFETY: the open list; the decision is kept for its submissions.
        unsafe { list.SetPredication(&s.decision, 0, op) };
        true
    }
    /// Readback: predication off and the decision copied for Read.
    pub fn readback(&mut self, list: &ID3D12GraphicsCommandList) -> bool {
        let Some(s) = self.storage.as_mut().filter(|s| s.prepared && !s.copied) else {
            return false;
        };
        if !keep(list, &s.objects()) {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the open list; the gate's objects are kept for its submissions.
        unsafe {
            list.SetPredication(None::<&ID3D12Resource>, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
            list.ResourceBarrier(&[transition(&s.decision, D3D12_RESOURCE_STATE_PREDICATION, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            list.CopyBufferRegion(&s.readback, 0, &s.decision, 0, 8);
            list.ResourceBarrier(&[transition(&s.decision, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PREDICATION)]);
        }
        s.copied = true;
        true
    }
    /// Read: the GPU's decision (0 or 1), after the list's fence.
    pub fn read(&self, value: &mut u64) -> HRESULT {
        let Some(s) = self.storage.as_ref().filter(|s| s.copied) else {
            return E_UNEXPECTED;
        };
        let mut data = core::ptr::null_mut();
        let range = D3D12_RANGE { Begin: 0, End: 8 };
        let none = D3D12_RANGE { Begin: 0, End: 0 };
        // SAFETY: the readback buffer's eight bytes, after the caller's fence.
        unsafe {
            if let Err(e) = s.readback.Map(0, Some(&range), Some(&mut data)) {
                return e.code();
            }
            if data.is_null() {
                s.readback.Unmap(0, Some(&none));
                return E_FAIL;
            }
            *value = (data as *const u64).read_unaligned();
            s.readback.Unmap(0, Some(&none));
        }
        if *value <= 1 {
            S_OK
        } else {
            E_FAIL
        }
    }
    /// Restart: for the next admission, once every previous GPU user has completed (the owning pass proves it).
    pub fn restart(&mut self) -> bool {
        let Some(s) = self.storage.as_mut().filter(|s| s.prepared && s.frozen && s.copied) else {
            return false;
        };
        s.prepared = false;
        s.frozen = false;
        s.copied = false;
        s.reused = true;
        self.decide(false)
    }
}

// The header's wrapper (DepthCommitGate.hpp in the Rust build).

fn gate<'a>(g: *mut Gate) -> Option<&'a mut Gate> {
    // SAFETY: the wrapper's gate.
    unsafe { g.as_mut() }
}
fn list_of(list: &*mut c_void) -> Option<&ID3D12GraphicsCommandList> {
    // SAFETY: the caller's live list (or null), borrowed for the call.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(list) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_new() -> *mut Gate {
    Box::into_raw(Box::default())
}
/// # Safety
/// `g` came from cvr_depth_gate_new (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_depth_gate_free(g: *mut Gate) {
    if !g.is_null() {
        // SAFETY: the wrapper's gate, dropped once.
        drop(unsafe { Box::from_raw(g) });
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_create(g: *mut Gate, device: *mut c_void, initial: *mut c_void) -> HRESULT {
    // SAFETY: the caller's device and resource (or null), borrowed for the call.
    match (gate(g), unsafe { ID3D12Device::from_raw_borrowed(&device) }, unsafe {
        ID3D12Resource::from_raw_borrowed(&initial)
    }) {
        (Some(g), Some(d), Some(i)) => g.create(d, i),
        _ => E_INVALIDARG,
    }
}
/// # Safety
/// `proofs` points to `count` proofs (or is null with a count of 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_depth_gate_prepare(
    g: *mut Gate,
    list: *mut c_void,
    flags: u32,
    pipeline: *mut c_void,
    proofs: *const BufferProof,
    count: usize,
) -> bool {
    // SAFETY: the caller's proofs.
    let proofs = if count == 0 || proofs.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(proofs, count) }
    };
    match (gate(g), list_of(&list)) {
        (Some(g), Some(list)) => g.prepare(list, flags, pipeline, proofs),
        _ => false,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_decide(g: *mut Gate, allow: bool) -> bool {
    gate(g).is_some_and(|g| g.decide(allow))
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_freeze(g: *mut Gate) {
    if let Some(g) = gate(g) {
        g.freeze();
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_bind(g: *mut Gate, list: *mut c_void, optimized: bool) -> bool {
    match (gate(g), list_of(&list)) {
        (Some(g), Some(list)) => g.bind(list, optimized),
        _ => false,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_readback(g: *mut Gate, list: *mut c_void) -> bool {
    match (gate(g), list_of(&list)) {
        (Some(g), Some(list)) => g.readback(list),
        _ => false,
    }
}
/// # Safety
/// `value` points to a u64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_depth_gate_read(g: *mut Gate, value: *mut u64) -> HRESULT {
    // SAFETY: the caller's output.
    match (gate(g), unsafe { value.as_mut() }) {
        (Some(g), Some(v)) => g.read(v),
        _ => E_UNEXPECTED,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_depth_gate_restart(g: *mut Gate) -> bool {
    gate(g).is_some_and(Gate::restart)
}
