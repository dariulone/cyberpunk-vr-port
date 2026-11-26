//! The camera-scope source resolve (formerly src/Render/CameraSourceResolve.cpp; include/Render/
//! PhysicalOpticsShaders.hpp's optics::camera calls it, camera_source_resolve_cs.hlsl).
//!
//! The scope's RTT view renders without temporal anti-aliasing (no jitter, no history), so the effects built for a
//! temporal resolve -- reflections, AO, contact shadows -- leave dark single-pixel dots that move every frame. The view
//! ends with its CopyToTexture node (0x3FE078), which writes the camera image into the DynamicTexture the scope UI
//! displays. That node records on its own thread; the target it binds and the transitions it (and the rest of its
//! list) records give the image and the state it is left in when the list ends. Right after that list is submitted
//! the port runs a list of its own on the same queue: the image is read where it rests, accumulated with the history
//! reprojected by the camera's own rotation (clipped to the current neighbourhood, isolated dark outliers replaced),
//! and the result copied back over it -- before anything that samples it can run. The engine's lists are never
//! touched.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{optics, optics_source};
use core::ffi::c_void;
use std::cell::RefCell;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const RING: usize = 3;
const DESCRIPTORS: u32 = 3;
const UNKNOWN: u32 = u32::MAX;
const HISTORY_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;
/// The scene camera descriptor of a view (view_layout::kCamera).
const CAMERA: usize = 0xE0;

fn log(text: &str) {
    cvr_host::log::write(text);
}

/// The C++ exports (src/Render/CameraSourceResolve.cpp attaches them): the switch, the history weights at rest and
/// moving, gamma, the resolve count, the misses (copy nodes that found no target, resolves refused for an unobserved
/// state) and the smoothed GPU time (us).
#[repr(C)]
pub struct Controls {
    pub enabled: *const AtomicI32,
    pub alpha: *const AtomicU32,
    pub motion_alpha: *const AtomicU32,
    pub gamma: *const AtomicU32,
    pub count: *const AtomicU64,
    pub misses: *const AtomicU64,
    pub microseconds: *const AtomicU32,
}
static CONTROLS: AtomicPtr<Controls> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `controls` and its pointers are the C++ exports, live for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_camera_attach(controls: *const Controls) {
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
fn float(at: Option<*const AtomicU32>, fallback: f32) -> f32 {
    // SAFETY: an attached std::atomic<float>.
    at.and_then(|a| unsafe { a.as_ref() })
        .map_or(fallback, |a| f32::from_bits(a.load(Ordering::Relaxed)))
}
fn miss(i: usize) -> u64 {
    // SAFETY: the attached two-counter export.
    controls().map_or(0, |c| unsafe {
        (*c.misses.add(i)).fetch_add(1, Ordering::Relaxed)
    })
}

#[derive(Clone, Copy, Default)]
struct Camera {
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    tan_x: f32,
    tan_y: f32,
    valid: bool,
}
struct Capture {
    list: usize,
    target: ID3D12Resource,
    /// The target's state as the list leaves it.
    state: AtomicU32,
    camera: Camera,
}
#[derive(Default)]
struct Entry {
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    heap: Option<ID3D12DescriptorHeap>,
    fence: u64,
    timed: bool,
}
struct Retired {
    _resource: ID3D12Resource,
    fence: u64,
}
#[repr(C)]
struct Constants {
    width: u32,
    height: u32,
    inverse_width: f32,
    inverse_height: f32,
    alpha: f32,
    motion_alpha: f32,
    gamma: f32,
    reset: u32,
    rows: [[f32; 4]; 3],
}
const _: () = assert!(core::mem::size_of::<Constants>() == 80);
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
    history: [Option<ID3D12Resource>; 2],
    /// Identity only: a new camera texture restarts the history.
    source: usize,
    width: u32,
    height: u32,
    previous: u32,
    primed: bool,
    tick: u64,
    last: Camera,
    ring: [Entry; RING],
    pending: Vec<Arc<Capture>>,
    retired: Vec<Retired>,
}
// SAFETY: free-threaded D3D12 objects, under the mutex.
unsafe impl Send for State {}
unsafe impl Send for Capture {}
unsafe impl Sync for Capture {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PENDING: AtomicUsize = AtomicUsize::new(0);
/// The camera image last captured: every later transition of it, on any thread, is matched to the pending capture of
/// its list, so the state the list leaves it in stays exact however the engine records the rest of that list.
static WATCHED: AtomicUsize = AtomicUsize::new(0);
static WATCH_LOG: AtomicI32 = AtomicI32::new(0);
static CAPTURE_LOG: AtomicI32 = AtomicI32::new(0);
fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The copy node's recording thread.
#[derive(Default)]
struct Node {
    active: bool,
    camera: Camera,
    list: usize,
    target: Option<ID3D12Resource>,
    transitions: Vec<(usize, u32)>,
    /// The state the node's write implies: an RTV is drawn to in RENDER_TARGET, a copy destination is written in
    /// COPY_DEST. Not a guess -- D3D12 allows no other -- and a transition the node records after the write
    /// overrides it.
    written: u32,
}
thread_local!(static NODE: RefCell<Node> = RefCell::new(Node::default()));

fn length(v: &[f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn dot(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// The view a node renders sits at work context +0x18; its camera basis and projection are the fields
/// Framegen/Inputs.cpp ViewCamera hands to FSR 3 and DLSS-G: its scene camera descriptor -- right/forward/up
/// +0x250/+0x260/+0x270, vertical fov +0x20 degrees, aspect +0x28.
fn read_camera(work_context: usize) -> Camera {
    let read = || -> Option<Camera> {
        let view = cvr_engine::code::read::<usize>(work_context + 0x18).filter(|&v| v != 0)?;
        let d = view + CAMERA;
        let right = cvr_engine::code::read::<[f32; 3]>(d + 0x250)?;
        let forward = cvr_engine::code::read::<[f32; 3]>(d + 0x260)?;
        let up = cvr_engine::code::read::<[f32; 3]>(d + 0x270)?;
        let degrees = cvr_engine::code::read::<f32>(d + 0x20)?;
        let aspect = cvr_engine::code::read::<f32>(d + 0x28)?;
        if !degrees.is_finite()
            || !aspect.is_finite()
            || !(0.5..=170.0).contains(&degrees)
            || !(0.1..=10.0).contains(&aspect)
        {
            return None;
        }
        for v in [&right, &up, &forward] {
            if !v.iter().all(|x| x.is_finite()) || (length(v) - 1.0).abs() > 0.05 {
                return None;
            }
        }
        if dot(&right, &forward).abs() > 0.05 || dot(&up, &forward).abs() > 0.05 {
            return None;
        }
        let tan_y = (degrees * 0.5 * 3.14159265 / 180.0).tan();
        Some(Camera {
            right,
            up,
            forward,
            tan_x: tan_y * aspect,
            tan_y,
            valid: true,
        })
    };
    read().unwrap_or_default()
}
unsafe fn camera_image(resource: &ID3D12Resource) -> bool {
    let d = resource.GetDesc();
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && d.MipLevels == 1
        && d.DepthOrArraySize == 1
        && d.SampleDesc.Count == 1
        && (d.Format == DXGI_FORMAT_R16G16B16A16_FLOAT
            || d.Format == DXGI_FORMAT_R16G16B16A16_TYPELESS)
        && (64..=4096).contains(&d.Width)
        && (64..=4096).contains(&d.Height)
}

fn fail(s: &mut State, why: &str) -> bool {
    s.failed = true;
    FAILED.store(true, Ordering::Relaxed);
    log(&format!("[optics] Camera source resolve off: {why}\n"));
    false
}
unsafe fn initialize(s: &mut State, device: &ID3D12Device) -> bool {
    if s.failed {
        return false;
    }
    if s.device != 0 {
        return s.device == device.as_raw() as usize;
    }
    let cs = optics::shader(4);
    if cs.is_empty() {
        return fail(s, "shader missing");
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
    let queries_desc = D3D12_QUERY_HEAP_DESC {
        Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
        Count: (2 * RING) as u32,
        NodeMask: 0,
    };
    let readback = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let buffer = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: (2 * RING * 8) as u64,
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
unsafe fn ensure_history(s: &mut State, width: u32, height: u32, device: &ID3D12Device) -> bool {
    if s.history[0].is_some() && s.width == width && s.height == height {
        return true;
    }
    let value = s.value;
    for history in s.history.iter_mut() {
        if let Some(h) = history.take() {
            s.retired.push(Retired {
                _resource: h,
                fence: value,
            });
        }
    }
    s.width = 0;
    s.height = 0;
    s.primed = false;
    s.previous = 0;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
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
    for history in s.history.iter_mut() {
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
    let (Some(h0), Some(h1)) = (s.history[0].as_ref(), s.history[1].as_ref()) else {
        return false;
    };
    let _ = h0.SetName(windows::core::w!("CVR camera scope history 0"));
    let _ = h1.SetName(windows::core::w!("CVR camera scope history 1"));
    s.width = width;
    s.height = height;
    log(&format!(
        "[optics] Camera source resolve {width}x{height}\n"
    ));
    true
}
/// Current NDC (x, y, 1) -> previous homogeneous NDC, rotation only: a ray through the current pixel in world space,
/// expressed in the previous camera.
fn reprojection(previous: &Camera, current: &Camera) -> [[f32; 4]; 3] {
    let p = [&previous.right, &previous.up, &previous.forward];
    let ps = [1.0 / previous.tan_x, 1.0 / previous.tan_y, 1.0];
    let c = [&current.right, &current.up, &current.forward];
    let cs = [current.tan_x, current.tan_y, 1.0];
    let mut rows = [[0.0f32; 4]; 3];
    for i in 0..3 {
        for j in 0..3 {
            rows[i][j] = dot(p[i], c[j]) * ps[i] * cs[j];
        }
    }
    rows
}
fn transition(
    out: &mut Vec<D3D12_RESOURCE_BARRIER>,
    resource: &ID3D12Resource,
    before: u32,
    after: u32,
) {
    if before == after {
        return;
    }
    out.push(D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: D3D12_RESOURCE_STATES(before as i32),
                StateAfter: D3D12_RESOURCE_STATES(after as i32),
            }),
        },
    });
}
unsafe fn flush(list: &ID3D12GraphicsCommandList, barriers: &mut Vec<D3D12_RESOURCE_BARRIER>) {
    if !barriers.is_empty() {
        list.ResourceBarrier(barriers);
    }
    barriers.clear();
}

unsafe fn resolve(
    queue: &ID3D12CommandQueue,
    capture: &Capture,
    execute: optics_source::ExecuteFn,
) {
    if queue.GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return;
    }
    let resting = capture.state.load(Ordering::Acquire);
    // Never a guessed StateBefore for a texture the engine owns.
    if resting == UNKNOWN {
        miss(1);
        return;
    }
    let mut device: Option<ID3D12Device> = None;
    if capture.target.GetDevice(&mut device).is_err() {
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
    let target = capture.target.GetDesc();
    if !ensure_history(s, target.Width as u32, target.Height, &device) {
        return;
    }
    let Some(i) = s.ring.iter().position(|e| e.fence <= completed) else {
        return;
    };
    let query = (2 * i) as u32;
    let stamps = s.stamps as *const u64;
    if s.ring[i].timed && !stamps.is_null() {
        let (begin, end) = (*stamps.add(query as usize), *stamps.add(query as usize + 1));
        if let (Ok(frequency), true) = (queue.GetTimestampFrequency(), end > begin) {
            if frequency != 0 {
                let us = ((end - begin) as f64 * 1e6 / frequency as f64) as f32;
                if let Some(c) = controls() {
                    let previous = float(Some(c.microseconds), 0.0);
                    let smoothed = if previous > 0.0 {
                        previous + 0.05 * (us - previous)
                    } else {
                        us
                    };
                    (*c.microseconds).store(smoothed.to_bits(), Ordering::Relaxed);
                }
            }
        }
        s.ring[i].timed = false;
    }
    if s.ring[i].allocator.is_none() {
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
            Ok(e) => s.ring[i] = e,
            Err(_) => {
                s.ring[i] = Entry::default();
                return;
            }
        }
    }
    let entry = &s.ring[i];
    let (Some(allocator), Some(list), Some(heap)) = (
        entry.allocator.clone(),
        entry.list.clone(),
        entry.heap.clone(),
    ) else {
        return;
    };
    let pipeline = s.pipeline.clone().expect("the pipeline");
    if allocator.Reset().is_err() || list.Reset(&allocator, &pipeline).is_err() {
        return;
    }
    let next = s.previous ^ 1;
    let stride = s.stride as usize;
    let cpu = heap.GetCPUDescriptorHandleForHeapStart();
    let at = |i: usize| D3D12_CPU_DESCRIPTOR_HANDLE {
        ptr: cpu.ptr + i * stride,
    };
    let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: HISTORY_FORMAT,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MipLevels: 1,
                ..Default::default()
            },
        },
    };
    let (Some(history_previous), Some(history_next)) = (
        s.history[s.previous as usize].clone(),
        s.history[next as usize].clone(),
    ) else {
        return;
    };
    device.CreateShaderResourceView(&capture.target, Some(&view), at(0));
    device.CreateShaderResourceView(&history_previous, Some(&view), at(1));
    let uav = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: HISTORY_FORMAT,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    device.CreateUnorderedAccessView(&history_next, None, Some(&uav), at(2));
    let now = windows::Win32::System::SystemInformation::GetTickCount64();
    let reset = !s.primed
        || now.wrapping_sub(s.tick) > 250
        || s.source != capture.target.as_raw() as usize
        || !s.last.valid
        || !capture.camera.valid;
    let c = controls();
    let mut constants = Constants {
        width: s.width,
        height: s.height,
        inverse_width: 1.0 / s.width as f32,
        inverse_height: 1.0 / s.height as f32,
        alpha: float(c.map(|c| c.alpha), 0.12).clamp(0.02, 1.0),
        motion_alpha: float(c.map(|c| c.motion_alpha), 0.35).clamp(0.02, 1.0),
        gamma: float(c.map(|c| c.gamma), 1.25).clamp(0.25, 4.0),
        reset: reset as u32,
        rows: [[0.0; 4]; 3],
    };
    if !reset {
        constants.rows = reprojection(&s.last, &capture.camera);
    }
    if let Some(q) = s.queries.as_ref() {
        list.EndQuery(q, D3D12_QUERY_TYPE_TIMESTAMP, query);
    }
    // Read the image where it rests when it is already a shader resource.
    let non_pixel = D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 as u32;
    let read = if resting & non_pixel != 0 {
        resting
    } else {
        non_pixel
    };
    let mut barriers = Vec::with_capacity(4);
    transition(&mut barriers, &capture.target, resting, read);
    transition(
        &mut barriers,
        &history_next,
        non_pixel,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32,
    );
    flush(&list, &mut barriers);
    list.SetDescriptorHeaps(&[Some(heap.clone())]);
    list.SetComputeRootSignature(s.root.as_ref());
    list.SetComputeRoot32BitConstants(
        0,
        (core::mem::size_of::<Constants>() / 4) as u32,
        &constants as *const Constants as *const c_void,
        0,
    );
    list.SetComputeRootDescriptorTable(1, heap.GetGPUDescriptorHandleForHeapStart());
    list.Dispatch(s.width.div_ceil(8), s.height.div_ceil(8), 1);
    transition(
        &mut barriers,
        &history_next,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32,
        D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32,
    );
    transition(
        &mut barriers,
        &capture.target,
        read,
        D3D12_RESOURCE_STATE_COPY_DEST.0 as u32,
    );
    flush(&list, &mut barriers);
    list.CopyResource(&capture.target, &history_next);
    transition(
        &mut barriers,
        &capture.target,
        D3D12_RESOURCE_STATE_COPY_DEST.0 as u32,
        resting,
    );
    transition(
        &mut barriers,
        &history_next,
        D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32,
        non_pixel,
    );
    flush(&list, &mut barriers);
    if let (Some(q), Some(t)) = (s.queries.as_ref(), s.timestamps.as_ref()) {
        list.EndQuery(q, D3D12_QUERY_TYPE_TIMESTAMP, query + 1);
        list.ResolveQueryData(q, D3D12_QUERY_TYPE_TIMESTAMP, query, 2, t, query as u64 * 8);
        s.ring[i].timed = true;
    }
    if list.Close().is_err() {
        return;
    }
    let lists = [list.as_raw()];
    execute(queue.as_raw(), 1, lists.as_ptr());
    s.value += 1;
    if queue
        .Signal(s.fence.as_ref().expect("the fence"), s.value)
        .is_err()
    {
        return;
    }
    s.ring[i].fence = s.value;
    s.previous = next;
    s.primed = true;
    s.tick = now;
    s.last = capture.camera;
    s.source = capture.target.as_raw() as usize;
    if let Some(c) = controls() {
        (*c.count).fetch_add(1, Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "C" fn cvr_optics_camera_node_active() -> bool {
    NODE.with(|n| n.borrow().active)
}
/// CopyNodeBegin: around the scope view's CopyToTexture node, on its recording thread.
#[no_mangle]
pub extern "C" fn cvr_optics_camera_copy_node_begin(work_context: usize) {
    NODE.with(|n| {
        let mut n = n.borrow_mut();
        *n = Node {
            written: UNKNOWN,
            ..Node::default()
        };
        if work_context == 0 || !enabled() || FAILED.load(Ordering::Relaxed) {
            return;
        }
        n.camera = read_camera(work_context);
        n.active = true;
    });
}
#[no_mangle]
pub extern "C" fn cvr_optics_camera_copy_node_end() {
    let node = NODE.with(|n| {
        let mut n = n.borrow_mut();
        if !n.active {
            return None;
        }
        n.active = false;
        Some((
            n.target.take(),
            core::mem::take(&mut n.list),
            core::mem::take(&mut n.transitions),
            n.written,
            n.camera,
        ))
    });
    let Some((target, list, transitions, written, camera)) = node else {
        return;
    };
    // SAFETY: the list the node recorded into (open on this thread).
    let direct = list != 0
        && unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&(list as *mut c_void)) }
            .is_some_and(|l| unsafe { l.GetType() } == D3D12_COMMAND_LIST_TYPE_DIRECT);
    let has_target = target.is_some();
    let Some(target) = target.filter(|_| direct) else {
        if miss(0) < 2 {
            let why = if has_target {
                "not a direct list"
            } else {
                "no target"
            };
            log(&format!(
                "[optics] Camera source resolve: copy node bound no camera image ({why})\n"
            ));
        }
        return;
    };
    let resting = transitions
        .iter()
        .rev()
        .find(|t| t.0 == target.as_raw() as usize)
        .map_or(written, |t| t.1);
    let capture = Arc::new(Capture {
        list,
        target,
        state: AtomicU32::new(resting),
        camera,
    });
    {
        let mut guard = state();
        let s = guard.get_or_insert_with(State::default);
        s.pending.retain(|c| c.list != list);
        if s.pending.len() >= 8 {
            return;
        }
        s.pending.push(capture.clone());
        PENDING.store(s.pending.len(), Ordering::Release);
    }
    if CAPTURE_LOG.fetch_add(1, Ordering::Relaxed) < 3 {
        // SAFETY: the captured image, held.
        let d = unsafe { capture.target.GetDesc() };
        log(&format!(
            "[optics] Camera source resolve: image {}x{} format {} state 0x{:X} camera {} (tan {:.4} x {:.4})\n",
            d.Width,
            d.Height,
            d.Format.0,
            resting,
            if capture.camera.valid { "valid" } else { "invalid" },
            capture.camera.tan_x,
            capture.camera.tan_y
        ));
    }
    WATCHED.store(capture.target.as_raw() as usize, Ordering::Release);
}
fn note_written(list: usize, resource: *mut c_void, written: u32) {
    // SAFETY: the engine's live resource (or null), borrowed for the check.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&resource) }) else {
        return;
    };
    NODE.with(|n| {
        let mut n = n.borrow_mut();
        if !n.active || !unsafe { camera_image(r) } {
            return;
        }
        n.target = Some(r.clone());
        n.list = list;
        n.written = written;
        for t in n
            .transitions
            .iter_mut()
            .filter(|t| t.0 == resource as usize)
        {
            t.0 = 0;
        }
    });
}
/// TargetBound: a render target bound by the engine (resolved resource), any thread.
#[no_mangle]
pub extern "C" fn cvr_optics_camera_target_bound(list: usize, resource: *mut c_void) {
    note_written(list, resource, D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32);
}
#[no_mangle]
pub extern "C" fn cvr_optics_camera_copied(list: usize, destination: *mut c_void) {
    note_written(list, destination, D3D12_RESOURCE_STATE_COPY_DEST.0 as u32);
}
/// Barriers: the node's transitions, and every transition of the watched image matched to its list's capture.
///
/// # Safety
/// `barriers` holds `count` barriers or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_camera_barriers(
    list: usize,
    count: u32,
    barriers: *const D3D12_RESOURCE_BARRIER,
) {
    let image = WATCHED.load(Ordering::Acquire);
    let in_node = cvr_optics_camera_node_active();
    if barriers.is_null() || (!in_node && image == 0) {
        return;
    }
    for i in 0..count as usize {
        let b = &*barriers.add(i);
        if b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION
            || b.Flags.0 & D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY.0 != 0
        {
            continue;
        }
        let t = &b.Anonymous.Transition;
        let resource: usize = core::mem::transmute_copy(&t.pResource);
        let after = t.StateAfter.0 as u32;
        if in_node {
            NODE.with(|n| {
                let mut n = n.borrow_mut();
                if let Some(x) = n.transitions.iter_mut().find(|x| x.0 == resource) {
                    x.1 = after;
                } else if n.transitions.len() < 16 {
                    n.transitions.push((resource, after));
                }
            });
        }
        if resource != image || resource == 0 {
            continue;
        }
        let mut matched = false;
        if PENDING.load(Ordering::Acquire) != 0 {
            if let Some(s) = state().as_ref() {
                for item in s
                    .pending
                    .iter()
                    .filter(|c| c.list == list && c.target.as_raw() as usize == resource)
                {
                    item.state.store(after, Ordering::Release);
                    matched = true;
                }
            }
        }
        if WATCH_LOG.fetch_add(1, Ordering::Relaxed) < 12 {
            log(&format!(
                "[optics] Camera image transition 0x{:X} -> 0x{:X} list {:016X} thread {} in-node {} pending-list {}\n",
                t.StateBefore.0 as u32,
                after,
                list,
                windows::Win32::System::Threading::GetCurrentThreadId(),
                in_node as i32,
                matched as i32
            ));
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_optics_camera_list_reset(list: usize) {
    if cvr_host::probe::internal() || PENDING.load(Ordering::Acquire) == 0 {
        return;
    }
    if let Some(s) = state().as_mut() {
        s.pending.retain(|c| c.list != list);
        PENDING.store(s.pending.len(), Ordering::Release);
    }
}
/// Execute: a submission split after each list that wrote the camera image; the rest goes through the source
/// resolve's Execute.
///
/// # Safety
/// The arguments are ExecuteCommandLists'; `execute` is the original.
#[no_mangle]
pub unsafe extern "C" fn cvr_optics_camera_execute(
    queue: *mut c_void,
    n: u32,
    lists: *const *mut c_void,
    execute: optics_source::ExecuteFn,
) {
    let command_queue = ID3D12CommandQueue::from_raw_borrowed(&queue);
    if PENDING.load(Ordering::Acquire) == 0 || command_queue.is_none() || lists.is_null() {
        optics_source::cvr_optics_source_execute(queue, n, lists, execute);
        return;
    }
    let command_queue = command_queue.expect("the queue");
    let mut start = 0u32;
    for i in 0..n {
        let list = *lists.add(i as usize) as usize;
        let capture = {
            let mut guard = state();
            let Some(s) = guard.as_mut() else { continue };
            let Some(at) = s.pending.iter().position(|c| c.list == list) else {
                continue;
            };
            let c = s.pending.remove(at);
            PENDING.store(s.pending.len(), Ordering::Release);
            c
        };
        optics_source::cvr_optics_source_execute(
            queue,
            i + 1 - start,
            lists.add(start as usize),
            execute,
        );
        start = i + 1;
        if enabled() {
            resolve(command_queue, &capture, execute);
        }
    }
    if n > start {
        optics_source::cvr_optics_source_execute(
            queue,
            n - start,
            lists.add(start as usize),
            execute,
        );
    }
}
