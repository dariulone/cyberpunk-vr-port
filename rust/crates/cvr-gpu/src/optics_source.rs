//! The lens source resolve (formerly src/Render/LensSourceResolve.cpp; include/Render/PhysicalOpticsShaders.hpp's
//! optics::source calls it, lens_source_resolve_cs.hlsl).
//!
//! The glass magnifies the jittered pre-upscale scene copy, so sub-pixel lights and grate bars blink in M x M blobs the
//! upscaler cannot settle. Before the list holding the glass pass runs, the port accumulates the scene over frames and
//! hands the glass a temporally stable image jittered like the frame, copied over the scene copy by that list.
//!
//! ajax.rdc, in GPU order: the list holding the glass pass STARTS with the scene copy (barrier, CopyTextureRegion t32
//! <- scene colour), then draws the glass. The scene colour already rests in COPY_SOURCE|PIXEL|NON_PIXEL (2240) and
//! the engine velocity in PIXEL|NON_PIXEL (192) when that list starts, so a list of the port's own, executed right
//! before it, reads both as they are and never touches the engine's recording state. The engine list only gets one
//! extra copy (the resolved image over t32) right after its own; the glass keeps its own shader, since that image is
//! jittered like the frame.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::optics;
use core::ffi::c_void;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};
use std::sync::{Mutex, MutexGuard};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SCENE_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R11G11B10_FLOAT;
const HISTORY_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;
const RING: usize = 3;
const DESCRIPTORS: u32 = 5;
/// The view a node renders sits at work context +0x18 (NativeInputs ViewOf); its scene camera descriptor
/// (view_layout::kCamera, +0xE0) +0x370/+0x374 is its jitter in pixels, y up (Inputs.cpp ViewCamera, the values handed
/// to FSR 3 as (x, -y)).
const VIEW_JITTER: usize = 0xE0 + 0x370;

fn log(text: &str) {
    cvr_host::log::write(text);
}

/// The C++ exports (src/Render/LensSourceResolve.cpp attaches them): the switch, the history weight and gamma, the
/// resolve count, the scene copies registered per side, the smoothed GPU time of one eye's resolve (us), the jitter
/// mode ((x,-y), (-x,y), (x,y), (-x,-y)) and the clean lens' CAS strength.
#[repr(C)]
pub struct Controls {
    pub enabled: *const AtomicI32,
    pub alpha: *const AtomicU32,
    pub gamma: *const AtomicU32,
    pub count: *const AtomicU64,
    pub sides: *const AtomicU64,
    pub microseconds: *const AtomicU32,
    pub jitter_mode: *const AtomicI32,
    pub sharpen: *const AtomicU32,
}
static CONTROLS: AtomicPtr<Controls> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `controls` and its pointers are the C++ exports, live for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_source_attach(controls: *const Controls) {
    CONTROLS.store(controls as *mut Controls, Ordering::Release);
}
fn controls() -> Option<&'static Controls> {
    // SAFETY: the attached exports.
    unsafe { CONTROLS.load(Ordering::Acquire).as_ref() }
}
fn enabled() -> bool {
    // SAFETY: an attached export.
    controls().is_some_and(|c| unsafe { (*c.enabled).load(Ordering::Relaxed) } != 0)
}
fn float(at: *const AtomicU32, fallback: f32) -> f32 {
    // SAFETY: an attached std::atomic<float>.
    unsafe { at.as_ref() }.map_or(fallback, |a| f32::from_bits(a.load(Ordering::Relaxed)))
}

struct Registration {
    list: usize,
    side: usize,
    scene: ID3D12Resource,
    velocity: ID3D12Resource,
    jitter: [f32; 2],
}
#[derive(Default)]
struct Entry {
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    heap: Option<ID3D12DescriptorHeap>,
    fence: u64,
    timed: bool,
}
#[derive(Default)]
struct Side {
    history: [Option<ID3D12Resource>; 2],
    output: Option<ID3D12Resource>,
    width: u32,
    height: u32,
    previous: u32,
    primed: bool,
    tick: u64,
    ring: [Entry; RING],
}
struct Retired {
    _resource: ID3D12Resource,
    fence: u64,
}
#[derive(Default)]
struct State {
    device: usize,
    root: Option<ID3D12RootSignature>,
    pipeline: Option<ID3D12PipelineState>,
    fence: Option<ID3D12Fence>,
    queries: Option<ID3D12QueryHeap>,
    timestamps: Option<ID3D12Resource>,
    stamps: usize,
    value: u64,
    stride: u32,
    failed: bool,
    sides: [Side; 2],
    pending: Vec<Registration>,
    retired: Vec<Retired>,
}
// SAFETY: free-threaded D3D12 objects, under the mutex.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static PENDING: AtomicUsize = AtomicUsize::new(0);
/// Each eye's view jitter as the node dispatches see it, packed x|y.
static VIEW_JITTER_PACKED: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
static STABILIZE: AtomicI32 = AtomicI32::new(80);
static NO_LIST_LOGGED: AtomicBool = AtomicBool::new(false);
static JITTER_LOGGED: AtomicI32 = AtomicI32::new(0);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
#[repr(C)]
struct Constants {
    width: u32,
    height: u32,
    inverse_width: f32,
    inverse_height: f32,
    alpha: f32,
    gamma: f32,
    reset: u32,
    clean: u32,
    jitter_x: f32,
    jitter_y: f32,
    sharpen: f32,
}

fn fail(s: &mut State, why: &str) -> bool {
    s.failed = true;
    log(&format!("[optics] Lens source resolve off: {why}\n"));
    false
}
unsafe fn initialize(s: &mut State, device: &ID3D12Device) -> bool {
    if s.failed {
        return false;
    }
    if s.device != 0 {
        return s.device == device.as_raw() as usize;
    }
    let cs = optics::shader(3);
    if cs.is_empty() {
        return fail(s, "shader missing");
    }
    let mut support = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
        Format: SCENE_FORMAT,
        ..Default::default()
    };
    let checked = device.CheckFeatureSupport(
        D3D12_FEATURE_FORMAT_SUPPORT,
        &mut support as *mut _ as *mut c_void,
        core::mem::size_of::<D3D12_FEATURE_DATA_FORMAT_SUPPORT>() as u32,
    );
    if checked.is_err() || support.Support2.0 & D3D12_FORMAT_SUPPORT2_UAV_TYPED_STORE.0 == 0 {
        return fail(s, "no R11G11B10 UAV store");
    }
    let _internal = cvr_host::probe::Internal::new();
    let Ok(root) = device.CreateRootSignature::<ID3D12RootSignature>(0, cs) else {
        return fail(s, "root signature");
    };
    let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
        pRootSignature: core::mem::transmute_copy(&root),
        CS: D3D12_SHADER_BYTECODE {
            pShaderBytecode: cs.as_ptr().cast(),
            BytecodeLength: cs.len(),
        },
        ..Default::default()
    };
    let Ok(pipeline) = device.CreateComputePipelineState::<ID3D12PipelineState>(&desc) else {
        return fail(s, "pipeline");
    };
    let Ok(fence) = device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) else {
        return fail(s, "fence");
    };
    s.stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    // Two timestamps per ring entry, read back once the entry comes round again.
    let queries_desc = D3D12_QUERY_HEAP_DESC {
        Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
        Count: (2 * RING * 2) as u32,
        NodeMask: 0,
    };
    let readback = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let buffer = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: (2 * RING * 2 * 8) as u64,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut queries: Option<ID3D12QueryHeap> = None;
    let mut timestamps: Option<ID3D12Resource> = None;
    let mut mapped: *mut c_void = core::ptr::null_mut();
    let timed = device.CreateQueryHeap(&queries_desc, &mut queries).is_ok()
        && device
            .CreateCommittedResource(
                &readback,
                D3D12_HEAP_FLAG_NONE,
                &buffer,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut timestamps,
            )
            .is_ok()
        && timestamps
            .as_ref()
            .is_some_and(|t| t.Map(0, None, Some(&mut mapped)).is_ok());
    if timed && queries.is_some() && !mapped.is_null() {
        s.queries = queries;
        s.timestamps = timestamps;
        s.stamps = mapped as usize;
    }
    s.root = Some(root);
    s.pipeline = Some(pipeline);
    s.fence = Some(fence);
    s.device = device.as_raw() as usize;
    true
}
unsafe fn ensure_side(
    s: &mut State,
    side: usize,
    width: u32,
    height: u32,
    device: &ID3D12Device,
) -> bool {
    let value = s.value;
    let x = &mut s.sides[side];
    if x.output.is_some() && x.width == width && x.height == height {
        return true;
    }
    let mut old: Vec<ID3D12Resource> = x.history.iter_mut().filter_map(Option::take).collect();
    old.extend(x.output.take());
    x.width = 0;
    x.height = 0;
    x.primed = false;
    x.previous = 0;
    s.retired.extend(old.into_iter().map(|r| Retired {
        _resource: r,
        fence: value,
    }));
    let x = &mut s.sides[side];
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let mut desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: width as u64,
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        Format: HISTORY_FORMAT,
        ..Default::default()
    };
    for history in x.history.iter_mut() {
        if device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
                None,
                history,
            )
            .is_err()
        {
            return false;
        }
    }
    desc.Format = SCENE_FORMAT;
    if device
        .CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            None,
            &mut x.output,
        )
        .is_err()
    {
        return false;
    }
    let (Some(h0), Some(h1), Some(output)) = (
        x.history[0].as_ref(),
        x.history[1].as_ref(),
        x.output.as_ref(),
    ) else {
        return false;
    };
    let _ = h0.SetName(windows::core::w!("CVR lens source history 0"));
    let _ = h1.SetName(windows::core::w!("CVR lens source history 1"));
    let _ = output.SetName(windows::core::w!("CVR lens source resolved"));
    x.width = width;
    x.height = height;
    log(&format!("[optics] Lens source resolve {width}x{height}\n"));
    true
}
fn transition(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
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
pub type ExecuteFn = unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void);

unsafe fn resolve(queue: &ID3D12CommandQueue, r: &Registration, execute: ExecuteFn) {
    if queue.GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return;
    }
    let mut device: Option<ID3D12Device> = None;
    if r.scene.GetDevice(&mut device).is_err() {
        return;
    }
    let Some(device) = device else { return };
    let mut guard = state();
    let s = guard.get_or_insert_with(State::default);
    if !initialize(s, &device) {
        return;
    }
    let completed = s.fence.as_ref().expect("the fence").GetCompletedValue();
    s.retired.retain(|item| item.fence > completed);
    let _internal = cvr_host::probe::Internal::new();
    let scene = r.scene.GetDesc();
    if !ensure_side(s, r.side, scene.Width as u32, scene.Height, &device) {
        return;
    }
    let stamps = s.stamps as *const u64;
    let index = s.sides[r.side]
        .ring
        .iter()
        .position(|e| e.fence <= completed);
    let query = index.map_or(0, |i| (2 * (r.side * RING + i)) as u32);
    if let Some(i) = index {
        let entry = &mut s.sides[r.side].ring[i];
        if entry.timed && !stamps.is_null() {
            let (begin, end) = (*stamps.add(query as usize), *stamps.add(query as usize + 1));
            if let (Ok(frequency), true) = (queue.GetTimestampFrequency(), end > begin) {
                if frequency != 0 {
                    let us = ((end - begin) as f64 * 1e6 / frequency as f64) as f32;
                    if let Some(c) = controls() {
                        let previous = float(c.microseconds, 0.0);
                        let smoothed = if previous > 0.0 {
                            previous + 0.05 * (us - previous)
                        } else {
                            us
                        };
                        (*c.microseconds).store(smoothed.to_bits(), Ordering::Relaxed);
                    }
                }
            }
            entry.timed = false;
        }
    }
    let Some(i) = index else {
        if !NO_LIST_LOGGED.swap(true, Ordering::Relaxed) {
            log("[optics] Lens source resolve: no free command list, frame skipped\n");
        }
        return;
    };
    let stride = s.stride as usize;
    let (pipeline, root) = (
        s.pipeline.clone().expect("the pipeline"),
        s.root.clone().expect("the root"),
    );
    let (queries, timestamps) = (s.queries.clone(), s.timestamps.clone());
    let side = &mut s.sides[r.side];
    let entry = &mut side.ring[i];
    if entry.allocator.is_none() {
        let heap_desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: DESCRIPTORS,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };
        let created = (|| -> windows::core::Result<Entry> {
            let allocator: ID3D12CommandAllocator =
                device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            list.Close()?;
            let heap: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&heap_desc)?;
            Ok(Entry {
                allocator: Some(allocator),
                list: Some(list),
                heap: Some(heap),
                fence: 0,
                timed: false,
            })
        })();
        match created {
            Ok(e) => *entry = e,
            Err(_) => {
                *entry = Entry::default();
                return;
            }
        }
    }
    let (Some(allocator), Some(list), Some(heap)) = (
        entry.allocator.clone(),
        entry.list.clone(),
        entry.heap.clone(),
    ) else {
        return;
    };
    if allocator.Reset().is_err() || list.Reset(&allocator, &pipeline).is_err() {
        return;
    }
    let next = side.previous ^ 1;
    let cpu = heap.GetCPUDescriptorHandleForHeapStart();
    let at = |i: usize| D3D12_CPU_DESCRIPTOR_HANDLE {
        ptr: cpu.ptr + i * stride,
    };
    let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: SCENE_FORMAT,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MipLevels: 1,
                ..Default::default()
            },
        },
    };
    device.CreateShaderResourceView(&r.scene, Some(&view), at(0));
    view.Format = DXGI_FORMAT_R16G16_FLOAT;
    device.CreateShaderResourceView(&r.velocity, Some(&view), at(1));
    view.Format = HISTORY_FORMAT;
    let (Some(history_previous), Some(history_next), Some(output)) = (
        side.history[side.previous as usize].clone(),
        side.history[next as usize].clone(),
        side.output.clone(),
    ) else {
        return;
    };
    device.CreateShaderResourceView(&history_previous, Some(&view), at(2));
    let mut uav = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: HISTORY_FORMAT,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    device.CreateUnorderedAccessView(&history_next, None, Some(&uav), at(3));
    uav.Format = SCENE_FORMAT;
    device.CreateUnorderedAccessView(&output, None, Some(&uav), at(4));
    let now = windows::Win32::System::SystemInformation::GetTickCount64();
    let c = controls();
    let constants = Constants {
        width: side.width,
        height: side.height,
        inverse_width: 1.0 / side.width as f32,
        inverse_height: 1.0 / side.height as f32,
        alpha: float(c.map_or(core::ptr::null(), |c| c.alpha), 0.2).clamp(0.02, 1.0),
        gamma: float(c.map_or(core::ptr::null(), |c| c.gamma), 1.0).clamp(0.25, 4.0),
        reset: (!side.primed || now.wrapping_sub(side.tick) > 250) as u32,
        clean: optics::mask_active(r.side) as u32,
        jitter_x: r.jitter[0],
        jitter_y: r.jitter[1],
        sharpen: float(c.map_or(core::ptr::null(), |c| c.sharpen), 0.35).clamp(0.0, 1.0),
    };
    if let Some(q) = queries.as_ref() {
        list.EndQuery(q, D3D12_QUERY_TYPE_TIMESTAMP, query);
    }
    list.ResourceBarrier(&[
        transition(
            &history_next,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        ),
        transition(
            &output,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
        ),
    ]);
    list.SetDescriptorHeaps(&[Some(heap.clone())]);
    list.SetComputeRootSignature(&root);
    list.SetComputeRoot32BitConstants(
        0,
        (core::mem::size_of::<Constants>() / 4) as u32,
        &constants as *const Constants as *const c_void,
        0,
    );
    list.SetComputeRootDescriptorTable(1, heap.GetGPUDescriptorHandleForHeapStart());
    list.Dispatch(side.width.div_ceil(8), side.height.div_ceil(8), 1);
    list.ResourceBarrier(&[
        transition(
            &history_next,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        ),
        transition(
            &output,
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        ),
    ]);
    if let (Some(q), Some(t)) = (queries.as_ref(), timestamps.as_ref()) {
        list.EndQuery(q, D3D12_QUERY_TYPE_TIMESTAMP, query + 1);
        list.ResolveQueryData(q, D3D12_QUERY_TYPE_TIMESTAMP, query, 2, t, query as u64 * 8);
        side.ring[i].timed = true;
    }
    if list.Close().is_err() {
        return;
    }
    let lists = [list.as_raw()];
    execute(queue.as_raw(), 1, lists.as_ptr());
    let s = guard.as_mut().expect("the state");
    s.value += 1;
    let value = s.value;
    if queue
        .Signal(s.fence.as_ref().expect("the fence"), value)
        .is_err()
    {
        return;
    }
    let side = &mut s.sides[r.side];
    side.ring[i].fence = value;
    side.previous = next;
    side.primed = true;
    side.tick = now;
    if let Some(c) = controls() {
        (*c.count).fetch_add(1, Ordering::Relaxed);
    }
}

/// SceneCopied: at every engine CopyTextureRegion, after the original copy. For the glass pass' scene copy (whole
/// R11G11B10 copy of the scene colour, a glass drawn recently, view side 0/1) the list is registered for a resolve
/// and the image to copy over the scene copy is returned (null: copy nothing, e.g. the first frame). Owned by the
/// port; the caller keeps it alive with the list.
///
/// # Safety
/// The arguments are CopyTextureRegion's, `dst` and `src` valid or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_source_scene_copied(
    list: *mut c_void,
    side: i32,
    dst: *const D3D12_TEXTURE_COPY_LOCATION,
    x: u32,
    y: u32,
    z: u32,
    src: *const D3D12_TEXTURE_COPY_LOCATION,
    has_box: bool,
) -> *mut c_void {
    let null = core::ptr::null_mut();
    let Some(command_list) = ID3D12GraphicsCommandList::from_raw_borrowed(&list) else {
        return null;
    };
    let (Some(dst), Some(src)) = (dst.as_ref(), src.as_ref()) else {
        return null;
    };
    if !(0..=1).contains(&side)
        || has_box
        || x != 0
        || y != 0
        || z != 0
        || !enabled()
        || !optics::glass_recent()
    {
        return null;
    }
    if command_list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return null;
    }
    if dst.Type != D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX
        || src.Type != D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX
    {
        return null;
    }
    if dst.Anonymous.SubresourceIndex != 0 || src.Anonymous.SubresourceIndex != 0 {
        return null;
    }
    let (Some(destination), Some(source)) = (dst.pResource.as_ref(), src.pResource.as_ref()) else {
        return null;
    };
    let (d, s) = (destination.GetDesc(), source.GetDesc());
    let writable = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET.0
        | D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL.0
        | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0;
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Format != SCENE_FORMAT
        || d.MipLevels != 1
        || d.DepthOrArraySize != 1
        || d.SampleDesc.Count != 1
        || d.Flags.0 & writable != 0
        || s.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || s.Format != SCENE_FORMAT
        || s.Width != d.Width
        || s.Height != d.Height
        || s.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET.0 == 0
    {
        return null;
    }
    let Some(velocity) = optics::motion_vectors() else {
        return null;
    };
    let v = velocity.GetDesc();
    if v.Width != d.Width || v.Height != d.Height {
        return null;
    }
    let mut guard = state();
    let state = guard.get_or_insert_with(State::default);
    if state.failed {
        return null;
    }
    let key = list as usize;
    state.pending.retain(|r| r.list != key);
    if state.pending.len() >= 16 {
        return null;
    }
    let side = side as usize;
    if let Some(c) = controls() {
        (*c.sides.add(side)).fetch_add(1, Ordering::Relaxed);
    }
    // Content shift in pixels, y down: raw pixel p shows the world at p - shift.
    let packed = VIEW_JITTER_PACKED[side].load(Ordering::Relaxed);
    let view = [
        f32::from_bits(packed as u32),
        f32::from_bits((packed >> 32) as u32),
    ];
    // SAFETY: an attached export.
    let mode = controls().map_or(0, |c| (*c.jitter_mode).load(Ordering::Relaxed));
    let jitter = [
        if mode == 1 || mode == 3 {
            -view[0]
        } else {
            view[0]
        },
        if mode == 0 || mode == 3 {
            -view[1]
        } else {
            view[1]
        },
    ];
    if JITTER_LOGGED.fetch_add(1, Ordering::Relaxed) < 4 {
        log(&cvr_crt::format(
            c"[optics] Lens source resolve side %d jitter %.4f %.4f px\n",
            &[
                cvr_crt::Arg::I(side as i32),
                cvr_crt::Arg::F(jitter[0] as f64),
                cvr_crt::Arg::F(jitter[1] as f64),
            ],
        )
        .to_string_lossy());
    }
    let target = &state.sides[side];
    let output = match (&target.output, target.primed) {
        (Some(o), true) if target.width as u64 == d.Width && target.height == d.Height => {
            o.as_raw()
        }
        _ => null,
    };
    state.pending.push(Registration {
        list: key,
        side,
        scene: source.clone(),
        velocity,
        jitter,
    });
    PENDING.store(state.pending.len(), Ordering::Release);
    output
}
/// SetStabilize: xr_scope_stabilize, the percent of history in a moving lens (0 off).
#[no_mangle]
pub extern "C" fn cvr_optics_source_set_stabilize(percent: i32) {
    let percent = percent.clamp(0, 100);
    STABILIZE.store(percent, Ordering::Relaxed);
    if let Some(c) = controls() {
        // SAFETY: attached exports.
        unsafe {
            (*c.enabled).store((percent > 0) as i32, Ordering::Relaxed);
            if percent > 0 {
                (*c.alpha).store(
                    (1.0f32 - percent as f32 * 0.01).max(0.05).to_bits(),
                    Ordering::Relaxed,
                );
            }
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_optics_source_get_stabilize() -> i32 {
    STABILIZE.load(Ordering::Relaxed)
}
/// ViewDispatched: every render-graph node dispatch of an eye (side 0/1), its view's jitter, so the resolve places
/// each jittered sample where it really lies.
#[no_mangle]
pub extern "C" fn cvr_optics_source_view_dispatched(side: i32, work_context: usize) {
    if !(0..=1).contains(&side) || work_context == 0 || !enabled() || !optics::glass_recent() {
        return;
    }
    let Some(view) = cvr_engine::code::read::<usize>(work_context + 0x18).filter(|&v| v != 0)
    else {
        return;
    };
    let Some(jitter) = cvr_engine::code::read::<[f32; 2]>(view + VIEW_JITTER) else {
        return;
    };
    if !jitter.iter().all(|j| j.is_finite() && j.abs() <= 2.0) {
        return;
    }
    let packed = jitter[0].to_bits() as u64 | ((jitter[1].to_bits() as u64) << 32);
    VIEW_JITTER_PACKED[side as usize].store(packed, Ordering::Relaxed);
}
/// ListReset: the resolve's own lists reset through the same hook, under its lock.
#[no_mangle]
pub extern "C" fn cvr_optics_source_list_reset(list: usize) {
    if cvr_host::probe::internal() || PENDING.load(Ordering::Acquire) == 0 {
        return;
    }
    let mut guard = state();
    if let Some(s) = guard.as_mut() {
        s.pending.retain(|r| r.list != list);
        PENDING.store(s.pending.len(), Ordering::Release);
    }
}
/// Execute: a submission through the original ExecuteCommandLists, each registered list run right after its own
/// resolve.
///
/// # Safety
/// The arguments are ExecuteCommandLists'; `execute` is the original.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_source_execute(
    queue: *mut c_void,
    n: u32,
    lists: *const *mut c_void,
    execute: ExecuteFn,
) {
    let command_queue = ID3D12CommandQueue::from_raw_borrowed(&queue);
    if PENDING.load(Ordering::Acquire) == 0 || command_queue.is_none() || lists.is_null() {
        execute(queue, n, lists);
        return;
    }
    let command_queue = command_queue.expect("the queue");
    let mut start = 0u32;
    for i in 0..n {
        let list = *lists.add(i as usize) as usize;
        let registration = {
            let mut guard = state();
            let Some(s) = guard.as_mut() else { continue };
            let Some(at) = s.pending.iter().position(|r| r.list == list) else {
                continue;
            };
            let r = s.pending.remove(at);
            PENDING.store(s.pending.len(), Ordering::Release);
            r
        };
        if i > start {
            execute(queue, i - start, lists.add(start as usize));
        }
        start = i;
        resolve(command_queue, &registration, execute);
    }
    if n > start {
        execute(queue, n - start, lists.add(start as usize));
    }
}
