//! PIPELINE STATE STREAMS (formerly src/Render/ViewInstancedPipeline.cpp): D3D12's subobject streams walked, read,
//! described, edited and rebuilt with view instancing, a classic graphics description turned into one, and the
//! view-instanced pipelines created from them.
//!
//! The stream ABI: each subobject is its type (a 4-byte enum) followed by its payload at the payload's own alignment,
//! the whole subobject padded to a pointer. A walk refuses a stream over 64 KiB, an unknown or repeated type and a part
//! running past the end.
//!
//! src/Render/ViewInstancedPipeline.cpp keeps the C++ names as forwarders and the owned descriptions its C++ callers
//! hold (their vectors), which walk the stream through `cvr_pipeline_stream_walk`.
#![allow(unsafe_op_in_unsafe_fn)]
// The owned descriptions and Rust-side helpers serve the single-pass registry, which only the Rust build has.
#![cfg_attr(not(feature = "rust-build"), allow(dead_code))]
use core::ffi::{c_char, c_void};
use core::mem::{align_of, size_of};
use core::ptr::{read_unaligned, write_unaligned};
use windows::core::{Interface, HRESULT};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_UNSUPPORTED;

const fn align(n: usize, alignment: usize) -> usize {
    (n + alignment - 1) & !(alignment - 1)
}
#[derive(Clone, Copy, Default)]
struct PartLayout {
    payload: usize,
    bytes: usize,
}
const fn part<T>() -> PartLayout {
    let payload = align(size_of::<i32>(), align_of::<T>());
    PartLayout {
        payload,
        bytes: align(payload + size_of::<T>(), align_of::<*const c_void>()),
    }
}
fn layout(ty: i32) -> PartLayout {
    match D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty) {
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE => part::<*mut c_void>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS => part::<D3D12_SHADER_BYTECODE>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT => part::<D3D12_STREAM_OUTPUT_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND => part::<D3D12_BLEND_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK
        | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK => part::<u32>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER => part::<D3D12_RASTERIZER_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL => part::<D3D12_DEPTH_STENCIL_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1 => part::<D3D12_DEPTH_STENCIL_DESC1>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT => part::<D3D12_INPUT_LAYOUT_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE => {
            part::<D3D12_INDEX_BUFFER_STRIP_CUT_VALUE>()
        }
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY => {
            part::<D3D12_PRIMITIVE_TOPOLOGY_TYPE>()
        }
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS => {
            part::<D3D12_RT_FORMAT_ARRAY>()
        }
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT => part::<DXGI_FORMAT>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC => part::<DXGI_SAMPLE_DESC>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO => part::<D3D12_CACHED_PIPELINE_STATE>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS => part::<D3D12_PIPELINE_STATE_FLAGS>(),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING => part::<D3D12_VIEW_INSTANCING_DESC>(),
        _ => PartLayout::default(),
    }
}

/// Walk: each subobject's type, payload offset and payload, in order; false on a malformed stream (the visitor may
/// have seen the parts before the fault) or when the visitor stops.
pub(crate) unsafe fn walk(
    source: &D3D12_PIPELINE_STATE_STREAM_DESC,
    mut visit: impl FnMut(i32, usize, *const u8) -> bool,
) -> bool {
    let size = source.SizeInBytes;
    if source.pPipelineStateSubobjectStream.is_null() || size == 0 || size > 65536 {
        return false;
    }
    let bytes = source.pPipelineStateSubobjectStream as *const u8;
    let mut offset = 0usize;
    let mut seen = 0u64;
    while offset < size {
        if size - offset < size_of::<i32>() {
            return false;
        }
        let ty = read_unaligned(bytes.add(offset) as *const i32);
        let tag = ty as u32;
        if tag >= 64 || (seen >> tag) & 1 != 0 {
            return false;
        }
        seen |= 1 << tag;
        let p = layout(ty);
        if p.bytes == 0 || p.bytes > size - offset {
            return false;
        }
        if !visit(ty, offset + p.payload, bytes.add(offset + p.payload)) {
            return false;
        }
        offset += p.bytes;
    }
    true
}
unsafe fn get<T>(value: *const u8) -> T {
    read_unaligned(value as *const T)
}

/// include/Render/ViewInstancedPipeline.hpp's PipelineStreamInfo.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineStreamInfo {
    pub vs: D3D12_SHADER_BYTECODE,
    pub ps: D3D12_SHADER_BYTECODE,
    pub root: *mut c_void,
    pub targets: D3D12_RT_FORMAT_ARRAY,
    pub depth_format: DXGI_FORMAT,
    pub blend: D3D12_BLEND_DESC,
    pub depth: D3D12_DEPTH_STENCIL_DESC,
    pub samples: DXGI_SAMPLE_DESC,
    pub input: D3D12_INPUT_LAYOUT_DESC,
    pub have_blend: bool,
    pub have_depth: bool,
    pub depth_bounds: bool,
    pub other_stages: bool,
    pub view_instancing: bool,
    pub stream_output: bool,
    pub forced_samples: u32,
    pub topology: D3D12_PRIMITIVE_TOPOLOGY_TYPE,
    pub flags: D3D12_PIPELINE_STATE_FLAGS,
}
const _: () = assert!(
    size_of::<PipelineStreamInfo>() == 512
        && core::mem::offset_of!(PipelineStreamInfo, targets) == 40
        && core::mem::offset_of!(PipelineStreamInfo, blend) == 80
        && core::mem::offset_of!(PipelineStreamInfo, input) == 472
        && core::mem::offset_of!(PipelineStreamInfo, have_blend) == 488
        && core::mem::offset_of!(PipelineStreamInfo, forced_samples) == 496
        && core::mem::offset_of!(PipelineStreamInfo, flags) == 504
);
/// include/Render/ViewInstancedPipeline.hpp's PipelineStreamOffsets (SIZE_MAX: absent).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineStreamOffsets {
    pub vs: usize,
    pub ps: usize,
    pub cs: usize,
    pub cached: usize,
    pub ms: usize,
    pub as_: usize,
}
impl Default for PipelineStreamOffsets {
    fn default() -> Self {
        PipelineStreamOffsets {
            vs: usize::MAX,
            ps: usize::MAX,
            cs: usize::MAX,
            cached: usize::MAX,
            ms: usize::MAX,
            as_: usize::MAX,
        }
    }
}

/// The walk, for the owned descriptions src/Render/ViewInstancedPipeline.cpp keeps: `visit(context, type, payload
/// offset, payload)` per subobject.
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_walk(
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    visit: unsafe extern "C" fn(*mut c_void, i32, usize, *const u8) -> bool,
    context: *mut c_void,
) -> bool {
    let Some(source) = source.as_ref() else {
        return false;
    };
    walk(source, |ty, offset, value| {
        visit(context, ty, offset, value)
    })
}

/// ReadPipelineStream: the parts a pipeline is judged by; `output` is left alone on a malformed stream or more than
/// eight render targets.
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_read(
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    output: *mut PipelineStreamInfo,
) -> bool {
    let (Some(source), Some(output)) = (source.as_ref(), output.as_mut()) else {
        return false;
    };
    let mut r: PipelineStreamInfo = core::mem::zeroed();
    let valid = walk(source, |ty, _, value| {
        match D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty) {
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS => r.vs = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS => r.ps = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS => {
                let shader: D3D12_SHADER_BYTECODE = get(value);
                r.other_stages |= shader.BytecodeLength != 0;
            }
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER => {
                let raster: D3D12_RASTERIZER_DESC = get(value);
                r.forced_samples = raster.ForcedSampleCount;
            }
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING => r.view_instancing = true,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT => {
                let d: D3D12_STREAM_OUTPUT_DESC = get(value);
                r.stream_output = d.NumEntries != 0 || d.NumStrides != 0;
            }
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE => r.root = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS => r.targets = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT => r.depth_format = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND => {
                r.blend = get(value);
                r.have_blend = true;
            }
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC => r.samples = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY => r.topology = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS => r.flags = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT => r.input = get(value),
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL => {
                r.depth = get(value);
                r.have_depth = true;
            }
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1 => {
                let d: D3D12_DEPTH_STENCIL_DESC1 = get(value);
                r.depth = D3D12_DEPTH_STENCIL_DESC {
                    DepthEnable: d.DepthEnable,
                    DepthWriteMask: d.DepthWriteMask,
                    DepthFunc: d.DepthFunc,
                    StencilEnable: d.StencilEnable,
                    StencilReadMask: d.StencilReadMask,
                    StencilWriteMask: d.StencilWriteMask,
                    FrontFace: d.FrontFace,
                    BackFace: d.BackFace,
                };
                r.have_depth = true;
                r.depth_bounds = d.DepthBoundsTestEnable.as_bool();
            }
            _ => {}
        }
        true
    });
    if !valid || r.targets.NumRenderTargets > 8 {
        return false;
    }
    *output = r;
    true
}

/// EditPipelineStream: the stream copied into `output` (SizeInBytes long) and each subobject's payload there handed
/// to `edit(context, type, payload)`. False on a malformed stream.
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_edit(
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    output: *mut u8,
    edit: unsafe extern "C" fn(*mut c_void, i32, *mut u8),
    context: *mut c_void,
) -> bool {
    let Some(source) = source.as_ref() else {
        return false;
    };
    if !source.pPipelineStateSubobjectStream.is_null() && !output.is_null() {
        core::ptr::copy_nonoverlapping(
            source.pPipelineStateSubobjectStream as *const u8,
            output,
            source.SizeInBytes,
        );
    }
    walk(source, |ty, offset, _| {
        edit(context, ty, output.add(offset));
        true
    })
}

/// One line of the subobjects and their main fields, as sprintf wrote it.
pub(crate) fn describe(source: &D3D12_PIPELINE_STATE_STREAM_DESC) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut put = |s: String| out.extend_from_slice(s.as_bytes());
    // SAFETY: the stream's payloads as walked; the input layout's elements as the stream points at them.
    unsafe {
        walk(source, |ty, _, value| {
            match D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty) {
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER => {
                    let r: D3D12_RASTERIZER_DESC = get(value);
                    put(format!(
                        " raster(fill {} cull {} ccw {} bias {} clip {} msaa {} aa {} forced {} conservative {})",
                        r.FillMode.0,
                        r.CullMode.0,
                        r.FrontCounterClockwise.0,
                        r.DepthBias,
                        r.DepthClipEnable.0,
                        r.MultisampleEnable.0,
                        r.AntialiasedLineEnable.0,
                        r.ForcedSampleCount,
                        r.ConservativeRaster.0
                    ));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND => {
                    let d: D3D12_BLEND_DESC = get(value);
                    put(format!(
                        " blend(a2c {} independent {} rt0 {} logic {})",
                        d.AlphaToCoverageEnable.0,
                        d.IndependentBlendEnable.0,
                        d.RenderTarget[0].BlendEnable.0,
                        d.RenderTarget[0].LogicOpEnable.0
                    ));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL => {
                    let d: D3D12_DEPTH_STENCIL_DESC = get(value);
                    put(format!(
                        " depth({} write {} func {} stencil {})",
                        d.DepthEnable.0, d.DepthWriteMask.0, d.DepthFunc.0, d.StencilEnable.0
                    ));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1 => {
                    let d: D3D12_DEPTH_STENCIL_DESC1 = get(value);
                    put(format!(
                        " depth1({} write {} func {} stencil {} bounds {})",
                        d.DepthEnable.0,
                        d.DepthWriteMask.0,
                        d.DepthFunc.0,
                        d.StencilEnable.0,
                        d.DepthBoundsTestEnable.0
                    ));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS => {
                    let a: D3D12_RT_FORMAT_ARRAY = get(value);
                    put(" rt(".to_string());
                    let mut i = 0u32;
                    while i < a.NumRenderTargets && i < 8 {
                        put(format!(
                            "{}{}",
                            if i != 0 { "," } else { "" },
                            a.RTFormats[i as usize].0
                        ));
                        i += 1;
                    }
                    put(")".to_string());
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT => {
                    let f: DXGI_FORMAT = get(value);
                    put(format!(" dsv {}", f.0));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC => {
                    let s: DXGI_SAMPLE_DESC = get(value);
                    put(format!(" samples {}/{}", s.Count, s.Quality));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK => {
                    let m: u32 = get(value);
                    put(format!(" mask {m:08X}"));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT => {
                    let d: D3D12_INPUT_LAYOUT_DESC = get(value);
                    put(format!(" inputs {}", d.NumElements));
                    let mut i = 0u32;
                    while i < d.NumElements && i < 16 && !d.pInputElementDescs.is_null() {
                        let e = &*d.pInputElementDescs.add(i as usize);
                        put(" [".to_string());
                        if e.SemanticName.is_null() {
                            put("?".to_string());
                        } else {
                            let name = core::ffi::CStr::from_ptr(e.SemanticName.0 as *const c_char);
                            // The name's own bytes, as %s copies them.
                            out_bytes(&mut put, name.to_bytes());
                        }
                        put(format!(
                            "{} f{} s{} o{} c{} r{}]",
                            e.SemanticIndex,
                            e.Format.0,
                            e.InputSlot,
                            e.AlignedByteOffset,
                            e.InputSlotClass.0,
                            e.InstanceDataStepRate
                        ));
                        i += 1;
                    }
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY => {
                    let t: D3D12_PRIMITIVE_TOPOLOGY_TYPE = get(value);
                    put(format!(" topology {}", t.0));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE => {
                    let v: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE = get(value);
                    put(format!(" cut {}", v.0));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS => {
                    let f: D3D12_PIPELINE_STATE_FLAGS = get(value);
                    put(format!(" flags {:X}", f.0 as u32));
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK => {
                    let m: u32 = get(value);
                    put(format!(" node {m}"));
                }
                _ => put(format!(" #{ty}")),
            }
            true
        });
    }
    out
}
/// Raw bytes through the String sink (a name is not necessarily UTF-8).
fn out_bytes(put: &mut impl FnMut(String), bytes: &[u8]) {
    // SAFETY: the sink only appends the String's bytes; they are never read back as a str.
    put(unsafe { String::from_utf8_unchecked(bytes.to_vec()) });
}
/// DescribePipelineStream: the line into `output` (up to `capacity` bytes, not terminated); its full length returned.
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_describe(
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    output: *mut c_char,
    capacity: usize,
) -> usize {
    let Some(source) = source.as_ref() else {
        return 0;
    };
    let text = describe(source);
    if !output.is_null() {
        core::ptr::copy_nonoverlapping(text.as_ptr(), output as *mut u8, text.len().min(capacity));
    }
    text.len()
}

/// FindPipelineStreamOffsets: the shader, cached-blob and mesh payloads' offsets; `output` left alone on a malformed
/// stream.
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_offsets(
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    output: *mut PipelineStreamOffsets,
) -> bool {
    let (Some(source), Some(output)) = (source.as_ref(), output.as_mut()) else {
        return false;
    };
    let mut r = PipelineStreamOffsets::default();
    let valid = walk(source, |ty, offset, _| {
        match D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty) {
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS => r.vs = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS => r.ps = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS => r.cs = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO => r.cached = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS => r.ms = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS => r.as_ = offset,
            _ => {}
        }
        true
    });
    if valid {
        *output = r;
    }
    valid
}

/// One subobject appended: its type, its payload at the payload's alignment, padding to a pointer (zeros).
fn append<T: Copy>(out: &mut Vec<u8>, ty: D3D12_PIPELINE_STATE_SUBOBJECT_TYPE, value: &T) {
    let p = part::<T>();
    let at = out.len();
    out.resize(at + p.bytes, 0);
    // SAFETY: both writes are inside the bytes just added.
    unsafe {
        write_unaligned(out.as_mut_ptr().add(at) as *mut i32, ty.0);
        write_unaligned(out.as_mut_ptr().add(at + p.payload) as *mut T, *value);
    }
}

/// BuildViewInstancedStream: the stream with `vs` (and a given `ps`, a given root) in place of its own, the cached
/// blob emptied, the root added when it had none, and a two-view instancing part from `locations` appended.
/// Refuses a stream already view-instanced, one with a compute, amplification or mesh shader, one without a vertex
/// shader (or without the pixel shader a replacement is given for).
pub(crate) unsafe fn build_view_instanced(
    source: &D3D12_PIPELINE_STATE_STREAM_DESC,
    vs: &D3D12_SHADER_BYTECODE,
    locations: *const D3D12_VIEW_INSTANCE_LOCATION,
    allow_masking: bool,
    ps: &D3D12_SHADER_BYTECODE,
    root: *mut c_void,
) -> Option<Vec<u8>> {
    if vs.pShaderBytecode.is_null() || vs.BytecodeLength == 0 {
        return None;
    }
    if ps.pShaderBytecode.is_null() != (ps.BytecodeLength == 0) {
        return None;
    }
    let mut copy: Vec<u8> = Vec::new();
    let (mut found, mut found_ps, mut found_root) = (false, false, false);
    let valid = walk(source, |ty, offset, value| {
        let ty = D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty);
        if ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING {
            return false;
        }
        if copy.is_empty() {
            copy = core::slice::from_raw_parts(
                source.pPipelineStateSubobjectStream as *const u8,
                source.SizeInBytes,
            )
            .to_vec();
        }
        let at = copy.as_mut_ptr().add(offset);
        if ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS {
            write_unaligned(at as *mut D3D12_SHADER_BYTECODE, *vs);
            found = true;
        }
        if ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS {
            found_ps = true;
            if ps.BytecodeLength != 0 {
                write_unaligned(at as *mut D3D12_SHADER_BYTECODE, *ps);
            }
        }
        if ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE {
            found_root = true;
            if !root.is_null() {
                write_unaligned(at as *mut *mut c_void, root);
            }
        }
        if ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO {
            write_unaligned(
                at as *mut D3D12_CACHED_PIPELINE_STATE,
                D3D12_CACHED_PIPELINE_STATE::default(),
            );
        }
        if ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS
            || ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS
            || ty == D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS
        {
            let shader: D3D12_SHADER_BYTECODE = get(value);
            if shader.BytecodeLength != 0 {
                return false;
            }
        }
        true
    });
    if !valid || !found || (ps.BytecodeLength != 0 && !found_ps) {
        return None;
    }
    if !root.is_null() && !found_root {
        append(
            &mut copy,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,
            &root,
        );
    }
    let views = D3D12_VIEW_INSTANCING_DESC {
        ViewInstanceCount: 2,
        pViewInstanceLocations: locations,
        Flags: if allow_masking {
            D3D12_VIEW_INSTANCING_FLAG_ENABLE_VIEW_INSTANCE_MASKING
        } else {
            D3D12_VIEW_INSTANCING_FLAG_NONE
        },
    };
    append(
        &mut copy,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING,
        &views,
    );
    Some(copy)
}
/// The longest a rebuilt stream grows: a root part and the view instancing part.
pub const BUILD_GROWTH: usize =
    part::<*mut c_void>().bytes + part::<D3D12_VIEW_INSTANCING_DESC>().bytes;
/// BuildViewInstancedStream into `output` (`capacity` at least the source's size and BUILD_GROWTH); its length in
/// `written`. `locations` is the layout's two view locations, pointed at by the stream.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_pipeline_stream_build_view_instanced(
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    vs: *const D3D12_SHADER_BYTECODE,
    locations: *const D3D12_VIEW_INSTANCE_LOCATION,
    allow_masking: bool,
    ps: *const D3D12_SHADER_BYTECODE,
    root: *mut c_void,
    output: *mut u8,
    capacity: usize,
    written: *mut usize,
) -> bool {
    let (Some(source), Some(vs), Some(ps)) = (source.as_ref(), vs.as_ref(), ps.as_ref()) else {
        return false;
    };
    let Some(bytes) = build_view_instanced(source, vs, locations, allow_masking, ps, root) else {
        return false;
    };
    if bytes.len() > capacity || output.is_null() {
        return false;
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
    if let Some(w) = written.as_mut() {
        *w = bytes.len();
    }
    true
}
#[no_mangle]
pub extern "C" fn cvr_pipeline_stream_build_growth() -> usize {
    BUILD_GROWTH
}

/// A classic description as a subobject stream, in the order the C++ Stream struct had its parts (no view
/// instancing part: the builder adds its own); pointers stay the description's.
pub(crate) fn graphics_stream(source: &D3D12_GRAPHICS_PIPELINE_STATE_DESC) -> Vec<u8> {
    let mut out = Vec::with_capacity(GRAPHICS_STREAM_BYTES);
    // SAFETY: a borrowed copy of the description's root pointer (no reference is taken).
    let root: *mut c_void = unsafe { core::mem::transmute_copy(&source.pRootSignature) };
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,
        &root,
    );
    append(&mut out, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS, &source.VS);
    append(&mut out, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS, &source.PS);
    append(&mut out, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS, &source.DS);
    append(&mut out, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS, &source.HS);
    append(&mut out, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS, &source.GS);
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT,
        &source.StreamOutput,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND,
        &source.BlendState,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK,
        &source.SampleMask,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER,
        &source.RasterizerState,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL,
        &source.DepthStencilState,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT,
        &source.InputLayout,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE,
        &source.IBStripCutValue,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY,
        &source.PrimitiveTopologyType,
    );
    let mut targets = D3D12_RT_FORMAT_ARRAY {
        NumRenderTargets: source.NumRenderTargets,
        ..Default::default()
    };
    for i in 0..(source.NumRenderTargets as usize).min(8) {
        targets.RTFormats[i] = source.RTVFormats[i];
    }
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS,
        &targets,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT,
        &source.DSVFormat,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC,
        &source.SampleDesc,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK,
        &source.NodeMask,
    );
    append(
        &mut out,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS,
        &source.Flags,
    );
    out
}
/// The classic description's stream length.
pub const GRAPHICS_STREAM_BYTES: usize = 744;
#[no_mangle]
pub extern "C" fn cvr_pipeline_stream_graphics_bytes() -> usize {
    GRAPHICS_STREAM_BYTES
}
/// GraphicsDescStream into `output` (GRAPHICS_STREAM_BYTES long); false without a vertex shader or with more than
/// eight render targets.
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_graphics(
    source: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    output: *mut u8,
) -> bool {
    let Some(source) = source.as_ref() else {
        return false;
    };
    if source.VS.pShaderBytecode.is_null()
        || source.VS.BytecodeLength == 0
        || source.NumRenderTargets > D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT
        || output.is_null()
    {
        return false;
    }
    let bytes = graphics_stream(source);
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
    true
}

unsafe fn view_instancing_supported(device: &ID3D12Device2) -> Result<(), HRESULT> {
    let mut options = D3D12_FEATURE_DATA_D3D12_OPTIONS3::default();
    device
        .CheckFeatureSupport(
            D3D12_FEATURE_D3D12_OPTIONS3,
            &mut options as *mut _ as *mut c_void,
            size_of::<D3D12_FEATURE_DATA_D3D12_OPTIONS3>() as u32,
        )
        .map_err(|e| e.code())?;
    if options.ViewInstancingTier == D3D12_VIEW_INSTANCING_TIER_NOT_SUPPORTED {
        return Err(DXGI_ERROR_UNSUPPORTED);
    }
    Ok(())
}
const E_POINTER: HRESULT = HRESULT(0x80004003u32 as i32);
const E_INVALIDARG: HRESULT = HRESULT(0x80070057u32 as i32);

/// CreateViewInstancedPipelineStream: the rebuilt stream's pipeline into `result` (an owning reference), or why not.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_pipeline_stream_create_view_instanced(
    device: *mut c_void,
    source: *const D3D12_PIPELINE_STATE_STREAM_DESC,
    vs: *const D3D12_SHADER_BYTECODE,
    locations: *const D3D12_VIEW_INSTANCE_LOCATION,
    allow_masking: bool,
    result: *mut *mut c_void,
    ps: *const D3D12_SHADER_BYTECODE,
    root: *mut c_void,
) -> HRESULT {
    let Some(result) = result.as_mut() else {
        return E_POINTER;
    };
    *result = core::ptr::null_mut();
    let Some(device) = ID3D12Device2::from_raw_borrowed(&device) else {
        return E_INVALIDARG;
    };
    if let Err(e) = view_instancing_supported(device) {
        return e;
    }
    let (Some(source), Some(vs), Some(ps)) = (source.as_ref(), vs.as_ref(), ps.as_ref()) else {
        return E_INVALIDARG;
    };
    let Some(copy) = build_view_instanced(source, vs, locations, allow_masking, ps, root) else {
        return E_INVALIDARG;
    };
    let stream = D3D12_PIPELINE_STATE_STREAM_DESC {
        SizeInBytes: copy.len(),
        pPipelineStateSubobjectStream: copy.as_ptr() as *mut c_void,
    };
    match device.CreatePipelineState::<ID3D12PipelineState>(&stream) {
        Ok(pso) => {
            *result = pso.into_raw();
            HRESULT(0)
        }
        Err(e) => e.code(),
    }
}
/// CreateViewInstancedPipeline: a classic description's view-instanced pipeline (its own shaders and root).
#[no_mangle]
pub unsafe extern "C" fn cvr_pipeline_stream_create_view_instanced_graphics(
    device: *mut c_void,
    source: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    locations: *const D3D12_VIEW_INSTANCE_LOCATION,
    allow_masking: bool,
    result: *mut *mut c_void,
) -> HRESULT {
    let Some(out) = result.as_mut() else {
        return E_POINTER;
    };
    *out = core::ptr::null_mut();
    let Some(source) = source.as_ref() else {
        return E_INVALIDARG;
    };
    if ID3D12Device2::from_raw_borrowed(&device).is_none()
        || source.VS.pShaderBytecode.is_null()
        || source.VS.BytecodeLength == 0
        || source.NumRenderTargets > D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT
    {
        return E_INVALIDARG;
    }
    if let Some(d) = ID3D12Device2::from_raw_borrowed(&device) {
        if let Err(e) = view_instancing_supported(d) {
            return e;
        }
    }
    // A cached monoscopic PSO cannot describe the changed shaders/view layout. Every other classic descriptor field
    // is preserved in the stream.
    let bytes = graphics_stream(source);
    let stream = D3D12_PIPELINE_STATE_STREAM_DESC {
        SizeInBytes: bytes.len(),
        pPipelineStateSubobjectStream: bytes.as_ptr() as *mut c_void,
    };
    let none = D3D12_SHADER_BYTECODE::default();
    cvr_pipeline_stream_create_view_instanced(
        device,
        &stream,
        &source.VS,
        locations,
        allow_masking,
        result,
        &none,
        core::ptr::null_mut(),
    )
}

// ---- for Rust callers: reading, editing, owning

/// ReadPipelineStream for Rust callers.
pub(crate) unsafe fn read(source: &D3D12_PIPELINE_STATE_STREAM_DESC) -> Option<PipelineStreamInfo> {
    let mut info: PipelineStreamInfo = core::mem::zeroed();
    cvr_pipeline_stream_read(source, &mut info).then_some(info)
}
/// EditPipelineStream for Rust callers: the copy with `edit` applied to each payload, None on a malformed stream.
pub(crate) unsafe fn edit(
    source: &D3D12_PIPELINE_STATE_STREAM_DESC,
    mut edit: impl FnMut(i32, *mut u8),
) -> Option<Vec<u8>> {
    if source.pPipelineStateSubobjectStream.is_null() {
        return None;
    }
    let mut copy = core::slice::from_raw_parts(
        source.pPipelineStateSubobjectStream as *const u8,
        source.SizeInBytes,
    )
    .to_vec();
    let base = copy.as_mut_ptr();
    walk(source, |ty, offset, _| {
        edit(ty, base.add(offset));
        true
    })
    .then_some(copy)
}

/// What an owned description points at, each in an allocation of its own (stable while the lists grow).
#[derive(Default)]
pub(crate) struct OwnedParts {
    pub shaders: Vec<Box<[u8]>>,
    inputs: Vec<Box<[D3D12_INPUT_ELEMENT_DESC]>>,
    outputs: Vec<Box<[D3D12_SO_DECLARATION_ENTRY]>>,
    strides: Vec<Box<[u32]>>,
    views: Vec<Box<[D3D12_VIEW_INSTANCE_LOCATION]>>,
    names: Vec<Box<[u8]>>,
    /// The root signatures the descriptions name, each held by a reference of its own: a twin is created later, on
    /// demand, and D3D12 reads the root signature then -- one the engine released meanwhile would be freed memory
    /// (the C++ descriptions borrowed it; a twin creation crashed in D3D12Core on a freed root's vtable).
    roots: Vec<windows::core::IUnknown>,
}
// SAFETY: plain data and COM references (free-threaded D3D12 objects); the descriptions' pointers point into the boxes
// themselves.
unsafe impl Send for OwnedParts {}
unsafe impl Sync for OwnedParts {}
/// OwnedPipelineStream: the stream with everything it points at owned (the cached blob dropped; the root held).
#[derive(Default)]
pub(crate) struct OwnedStream {
    pub stream: Vec<u8>,
    pub offsets: PipelineStreamOffsets,
    pub parts: OwnedParts,
}
/// OwnedGraphicsDesc.
pub(crate) struct OwnedGraphics {
    pub desc: D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    pub parts: OwnedParts,
}
// SAFETY: the description's pointers point into its own parts or at engine objects that outlive it.
unsafe impl Send for OwnedGraphics {}
unsafe impl Sync for OwnedGraphics {}
impl OwnedParts {
    /// A reference of the parts' own on the root signature `root` (null: none).
    unsafe fn root(&mut self, root: *mut c_void) {
        // SAFETY: the caller's description names a live root signature.
        if let Some(held) = unsafe { windows::core::IUnknown::from_raw_borrowed(&root) } {
            self.roots.push(held.clone());
        }
    }
    unsafe fn name(&mut self, name: windows::core::PCSTR) -> windows::core::PCSTR {
        if name.is_null() {
            return windows::core::PCSTR::null();
        }
        let bytes = core::ffi::CStr::from_ptr(name.0 as *const c_char).to_bytes_with_nul();
        let owned: Box<[u8]> = bytes.into();
        let at = windows::core::PCSTR(owned.as_ptr());
        self.names.push(owned);
        at
    }
    unsafe fn shader(&mut self, s: &mut D3D12_SHADER_BYTECODE) {
        if s.pShaderBytecode.is_null() || s.BytecodeLength == 0 {
            *s = D3D12_SHADER_BYTECODE::default();
            return;
        }
        let owned: Box<[u8]> =
            core::slice::from_raw_parts(s.pShaderBytecode as *const u8, s.BytecodeLength).into();
        s.pShaderBytecode = owned.as_ptr() as *const c_void;
        self.shaders.push(owned);
    }
    unsafe fn input(&mut self, d: &mut D3D12_INPUT_LAYOUT_DESC) {
        if d.pInputElementDescs.is_null() || d.NumElements == 0 {
            *d = D3D12_INPUT_LAYOUT_DESC::default();
            return;
        }
        let mut owned: Box<[D3D12_INPUT_ELEMENT_DESC]> =
            core::slice::from_raw_parts(d.pInputElementDescs, d.NumElements as usize).into();
        for e in owned.iter_mut() {
            e.SemanticName = self.name(e.SemanticName);
        }
        d.pInputElementDescs = owned.as_ptr();
        self.inputs.push(owned);
    }
    unsafe fn output(&mut self, d: &mut D3D12_STREAM_OUTPUT_DESC) {
        if !d.pSODeclaration.is_null() && d.NumEntries != 0 {
            let mut owned: Box<[D3D12_SO_DECLARATION_ENTRY]> =
                core::slice::from_raw_parts(d.pSODeclaration, d.NumEntries as usize).into();
            for e in owned.iter_mut() {
                e.SemanticName = self.name(e.SemanticName);
            }
            d.pSODeclaration = owned.as_ptr();
            self.outputs.push(owned);
        } else {
            d.pSODeclaration = core::ptr::null();
            d.NumEntries = 0;
        }
        if !d.pBufferStrides.is_null() && d.NumStrides != 0 {
            let owned: Box<[u32]> =
                core::slice::from_raw_parts(d.pBufferStrides, d.NumStrides as usize).into();
            d.pBufferStrides = owned.as_ptr();
            self.strides.push(owned);
        } else {
            d.pBufferStrides = core::ptr::null();
            d.NumStrides = 0;
        }
    }
    unsafe fn views(&mut self, d: &mut D3D12_VIEW_INSTANCING_DESC) {
        if d.pViewInstanceLocations.is_null() || d.ViewInstanceCount == 0 {
            d.pViewInstanceLocations = core::ptr::null();
            return;
        }
        let owned: Box<[D3D12_VIEW_INSTANCE_LOCATION]> =
            core::slice::from_raw_parts(d.pViewInstanceLocations, d.ViewInstanceCount as usize)
                .into();
        d.pViewInstanceLocations = owned.as_ptr();
        self.views.push(owned);
    }
}
fn is_shader_part(ty: i32) -> bool {
    matches!(
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty),
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS
            | D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS
    )
}
/// OwnPipelineStream: None on a malformed stream.
pub(crate) unsafe fn own_stream(source: &D3D12_PIPELINE_STATE_STREAM_DESC) -> Option<OwnedStream> {
    if !walk(source, |_, _, _| true) {
        return None;
    }
    let mut out = OwnedStream {
        stream: core::slice::from_raw_parts(
            source.pPipelineStateSubobjectStream as *const u8,
            source.SizeInBytes,
        )
        .to_vec(),
        ..Default::default()
    };
    let base = out.stream.as_mut_ptr();
    let parts = &mut out.parts;
    let offsets = &mut out.offsets;
    let ok = walk(source, |ty, offset, _| {
        let value = base.add(offset);
        if is_shader_part(ty) {
            let mut s: D3D12_SHADER_BYTECODE = get(value);
            parts.shader(&mut s);
            write_unaligned(value as *mut D3D12_SHADER_BYTECODE, s);
        } else {
            match D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty) {
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE => {
                    let held: *mut c_void = get(value);
                    parts.root(held);
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT => {
                    let mut d: D3D12_INPUT_LAYOUT_DESC = get(value);
                    parts.input(&mut d);
                    write_unaligned(value as *mut D3D12_INPUT_LAYOUT_DESC, d);
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT => {
                    let mut d: D3D12_STREAM_OUTPUT_DESC = get(value);
                    parts.output(&mut d);
                    write_unaligned(value as *mut D3D12_STREAM_OUTPUT_DESC, d);
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING => {
                    let mut d: D3D12_VIEW_INSTANCING_DESC = get(value);
                    parts.views(&mut d);
                    write_unaligned(value as *mut D3D12_VIEW_INSTANCING_DESC, d);
                }
                D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO => {
                    write_unaligned(
                        value as *mut D3D12_CACHED_PIPELINE_STATE,
                        D3D12_CACHED_PIPELINE_STATE::default(),
                    );
                }
                _ => {}
            }
        }
        match D3D12_PIPELINE_STATE_SUBOBJECT_TYPE(ty) {
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS => offsets.vs = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS => offsets.ps = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS => offsets.cs = offset,
            D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO => offsets.cached = offset,
            _ => {}
        }
        true
    });
    ok.then_some(out)
}
/// OwnGraphicsDesc.
pub(crate) unsafe fn own_graphics(source: &D3D12_GRAPHICS_PIPELINE_STATE_DESC) -> OwnedGraphics {
    let mut out = OwnedGraphics {
        desc: core::ptr::read(source),
        parts: OwnedParts::default(),
    };
    let d = &mut out.desc;
    let p = &mut out.parts;
    p.root(d.pRootSignature.as_ref().map_or(core::ptr::null_mut(), |held| held.as_raw()));
    p.shader(&mut d.VS);
    p.shader(&mut d.PS);
    p.shader(&mut d.DS);
    p.shader(&mut d.HS);
    p.shader(&mut d.GS);
    p.input(&mut d.InputLayout);
    p.output(&mut d.StreamOutput);
    d.CachedPSO = D3D12_CACHED_PIPELINE_STATE::default();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn part_layouts_are_the_abi() {
        assert_eq!(
            (
                layout(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE.0).payload,
                layout(0).bytes
            ),
            (8, 16)
        );
        assert_eq!(
            layout(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK.0).payload,
            4
        );
        assert_eq!(
            layout(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND.0).bytes,
            336
        );
        assert_eq!(
            layout(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1.0).bytes,
            64
        );
        assert_eq!(
            layout(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING.0).bytes,
            32
        );
        assert_eq!(layout(23).bytes, 0);
        assert_eq!(BUILD_GROWTH, 48);
        let desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC::default();
        assert_eq!(graphics_stream(&desc).len(), GRAPHICS_STREAM_BYTES);
    }
}
