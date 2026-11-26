//! The early depth pass's state and its D3D12 work (src/Render/EarlyDepthPass.cpp, `EarlyDepthPass`): the pass's
//! scalar state (submissions, recordings, native counts and differences, instance and shader snapshot progress, the
//! alpha and shader statistics, the optimized counters, GPU times, the serial, the readback planes and the comparison
//! result); `Create`'s objects -- timestamp queries and their readbacks, five DSVs over the paired and reference
//! arrays, the readback planes (depth 32/64-bit, stencil 8/64-bit, a third native set for a whole target) and their
//! buffer, the two fences; `EndTiming`; `Read` (both fences done, the GPU times, the gate's or the planes' comparison);
//! `Readback`'s plane copies; `Submitted`'s signals and timestamp frequencies; `Reset`'s retired recordings;
//! `Restart`'s state; the instance capture (the source buffer's shape and device, the readable states, the private
//! snapshots, a combined read's augmented state and its release) and the readable buffer copy.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the pass's Storage holds this state as its base (the C++ objects -- the target
//! arrays, the commit gate, the binding arenas, the command resources -- stay there and are reached through callbacks);
//! `tools/single_pass_tests/early_depth_pass_test` runs its GPU modes against it in its Rust variant, and the unit tests
//! below cover the state rules.

use core::ffi::c_void;

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, S_FALSE, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_DEVICE_REMOVED;

use crate::early_depth_compare::{compare, Comparison, Plane};

/// early_depth::MaxDraws, MaxInstanceBytes.
pub const MAX_DRAWS: usize = 1024;
pub const MAX_INSTANCE_BYTES: u32 = 8 * 1024 * 1024;

/// The pass's scalar state (the C++ Storage's base, CvrEdpState).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct State {
    pub submission_pending: [bool; 2],
    pub recording_retired: [bool; 2],
    pub native_dsv: [u64; 2],
    /// enabled (the native mode), MAIN draws, VR draws, MAIN fallback, VR runs, MAIN export, VR export, imports.
    pub native_counts: [u32; 8],
    /// depth per eye, stencil per eye.
    pub native_differences: [u64; 4],
    pub instance_bytes: u32,
    pub instances_readable: bool,
    pub shader_bytes: u32,
    pub shaders_readable: bool,
    pub shader_ready: bool,
    pub alpha_captures: u32,
    pub alpha_pending_sequence: u32,
    pub skin_pending_sequence: u32,
    pub alpha_stats: [u64; 6],
    pub shader_stats: [u64; 4],
    pub instance_before: i32,
    pub instance_augmented: bool,
    /// mode, GPU decision, MAIN draw commands, unconditional peer commands, optimized peer ranges, fallback peer
    /// batches, native reference draws, commit admitted.
    pub optimized: [u64; 8],
    pub gpu_times: [u64; 6],
    pub native_finished: [bool; 2],
    pub timing_ended: [bool; 2],
    pub serial: u64,
    pub planes: [Plane; 12],
    pub stride: u32,
    pub bytes: u64,
    pub initialized: bool,
    pub copied: bool,
    pub main_submitted: bool,
    pub peer_submitted: bool,
    pub failed: bool,
    pub result: Comparison,
}
const _: () = assert!(
    core::mem::size_of::<State>() == 928
        && core::mem::offset_of!(State, native_counts) == 24
        && core::mem::offset_of!(State, optimized) == 208
        && core::mem::offset_of!(State, planes) == 336
        && core::mem::offset_of!(State, result) == 840
);
impl Default for State {
    fn default() -> Self {
        // SAFETY: plain data, all zero valid.
        let mut s: State = unsafe { core::mem::zeroed() };
        s.shader_ready = true;
        s.serial = 1;
        s
    }
}

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live D3D12 object the caller holds for the call.
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
fn transition(resource: *mut c_void, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}
fn buffer(width: u64) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: width,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    }
}
fn committed(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, desc: &D3D12_RESOURCE_DESC, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource, HRESULT> {
    let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; the descriptions outlive the call.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, desc, state, None, &mut out) }.map_err(|e| e.code())?;
    out.ok_or(E_FAIL)
}
fn into_raw<T: Interface>(value: Option<T>) -> *mut c_void {
    value.map_or(core::ptr::null_mut(), |v| v.into_raw())
}

/// Create's objects, handed to the C++ Storage (each with the creation's reference).
#[repr(C)]
pub struct Created {
    pub timestamps: *mut c_void,
    pub timing: [*mut c_void; 2],
    pub dsvs: *mut c_void,
    pub readback: *mut c_void,
    pub main_fence: *mut c_void,
    pub done_fence: *mut c_void,
}
/// The C++ half's target arrays (both initialized from the native target's description; their resources out).
pub type InitTargets = unsafe extern "C-unwind" fn(*mut c_void, *const D3D12_RESOURCE_DESC, *const D3D12_CLEAR_VALUE, *mut [*mut c_void; 2]) -> HRESULT;
/// The C++ half's commit gate over the paired array's initial depth.
pub type CreateGate = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void) -> HRESULT;

fn dsv(base: usize, index: u32, stride: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base + index as usize * stride as usize }
}
/// Create (after the C++ half's argument checks and the counters' modes).
///
/// # Safety
/// `device` is live; the callbacks are the C++ half's.
pub unsafe fn create(device: &ID3D12Device, width: u32, height: u32, mode: u32, s: &mut State, init: InitTargets, gate: CreateGate, user: *mut c_void) -> Result<Created, HRESULT> {
    let whole = mode != 0;
    let (mut timestamps, mut timing): (Option<ID3D12QueryHeap>, [Option<ID3D12Resource>; 2]) = (None, [None, None]);
    // SAFETY: a live device; the descriptions outlive each call.
    unsafe {
        if mode != 0 {
            let query = D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: 4, NodeMask: 0 };
            device.CreateQueryHeap(&query, &mut timestamps).map_err(|e| e.code())?;
            for output in &mut timing {
                *output = Some(committed(device, D3D12_HEAP_TYPE_READBACK, &buffer(16), D3D12_RESOURCE_STATE_COPY_DEST)?);
            }
        }
        let (mut dsvs, mut readback): (Option<ID3D12DescriptorHeap>, Option<ID3D12Resource>) = (None, None);
        if mode != 3 {
            let heap = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV, NumDescriptors: 5, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE, NodeMask: 0 };
            let created: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&heap).map_err(|e| e.code())?;
            s.stride = device.GetDescriptorHandleIncrementSize(heap.Type);
            let mut target = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: width as u64,
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_R32G8X24_TYPELESS,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
                ..Default::default()
            };
            let clear = D3D12_CLEAR_VALUE { Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT, ..Default::default() };
            let mut resources = [core::ptr::null_mut(); 2];
            let hr = init(user, &target, &clear, &mut resources);
            if hr.is_err() {
                return Err(hr);
            }
            target.DepthOrArraySize = 2;
            let base = created.GetCPUDescriptorHandleForHeapStart().ptr;
            for i in 0..5u32 {
                let mut view = D3D12_DEPTH_STENCIL_VIEW_DESC { Format: clear.Format, ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2DARRAY, ..Default::default() };
                view.Anonymous.Texture2DArray.ArraySize = if i != 0 { 1 } else { 2 };
                view.Anonymous.Texture2DArray.FirstArraySlice = if i == 2 || i == 4 { 1 } else { 0 };
                let resource = borrowed::<ID3D12Resource>(resources[if i == 1 || i == 2 { 1 } else { 0 }]);
                device.CreateDepthStencilView(resource.as_ref(), Some(&view), dsv(base, i, s.stride));
            }
            dsvs = Some(created);
            if mode == 2 {
                let hr = gate(user, resources[1]);
                if hr.is_err() {
                    return Err(hr);
                }
            }
            if mode != 2 {
                for plane in 0..2u32 {
                    for eye in 0..2u32 {
                        for m in 0..(if whole { 3u32 } else { 2 }) {
                            let p = &mut s.planes[if m == 2 { 8 + plane * 2 + eye } else { plane * 4 + eye * 2 + m } as usize];
                            let (mut rows, mut row_bytes, mut bytes) = (0u32, 0u64, 0u64);
                            device.GetCopyableFootprints(&target, eye + plane * 2, 1, s.bytes, Some(&mut p.layout), Some(&mut rows), Some(&mut row_bytes), Some(&mut bytes));
                            if bytes == u64::MAX || rows != height || row_bytes % width as u64 != 0 {
                                return Err(E_INVALIDARG);
                            }
                            p.stride = (row_bytes / width as u64) as u32;
                            if (plane == 0 && p.stride != 4 && p.stride != 8) || (plane != 0 && p.stride != 1 && p.stride != 8) {
                                return Err(E_NOTIMPL);
                            }
                            p.component = if plane != 0 && p.stride == 8 { 4 } else { 0 };
                            s.bytes = (p.layout.Offset + p.layout.Footprint.RowPitch as u64 * rows as u64 + 511) & !511;
                        }
                    }
                }
                if s.bytes > 512 * 1024 * 1024 {
                    return Err(E_OUTOFMEMORY);
                }
                let mut desc = target;
                desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
                desc.Width = s.bytes;
                desc.Height = 1;
                desc.DepthOrArraySize = 1;
                desc.Format = DXGI_FORMAT_UNKNOWN;
                desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
                desc.Flags = D3D12_RESOURCE_FLAG_NONE;
                readback = Some(committed(device, D3D12_HEAP_TYPE_READBACK, &desc, D3D12_RESOURCE_STATE_COPY_DEST)?);
            }
        }
        let main: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(|e| e.code())?;
        let done: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(|e| e.code())?;
        let [first, second] = timing;
        Ok(Created {
            timestamps: into_raw(timestamps),
            timing: [into_raw(first), into_raw(second)],
            dsvs: into_raw(dsvs),
            readback: into_raw(readback),
            main_fence: main.into_raw(),
            done_fence: done.into_raw(),
        })
    }
}

/// EndTiming.
///
/// # Safety
/// `list`, `timestamps` and `readback` are live (the queries possibly null).
pub unsafe fn end_timing(s: &mut State, list: &ID3D12GraphicsCommandList, timestamps: *mut c_void, readback: *mut c_void, side: u32) {
    let Some(queries) = borrowed::<ID3D12QueryHeap>(timestamps) else { return };
    if s.timing_ended[side as usize] {
        return;
    }
    let Some(target) = borrowed::<ID3D12Resource>(readback) else { return };
    // SAFETY: a live list recording on the pass's queries.
    unsafe {
        list.EndQuery(&queries, D3D12_QUERY_TYPE_TIMESTAMP, side * 2 + 1);
        list.ResolveQueryData(&queries, D3D12_QUERY_TYPE_TIMESTAMP, side * 2, 2, &target, 0);
    }
    s.timing_ended[side as usize] = true;
}

/// The objects Read reads.
#[repr(C)]
pub struct ReadObjects {
    pub done_fence: *mut c_void,
    pub main_fence: *mut c_void,
    pub timestamps: *mut c_void,
    pub timing: [*mut c_void; 2],
    pub readback: *mut c_void,
}
/// The C++ half's gate read (its decision into `*value`).
pub type GateRead = unsafe extern "C-unwind" fn(*mut c_void, *mut u64) -> HRESULT;
/// Maps [0, end) of a readback (None when it fails; the HRESULT of a failed map).
fn map(resource: &ID3D12Resource, end: usize) -> Result<*mut c_void, HRESULT> {
    let range = D3D12_RANGE { Begin: 0, End: end };
    let mut data = core::ptr::null_mut();
    // SAFETY: a live readback buffer.
    unsafe { resource.Map(0, Some(&range), Some(&mut data)) }.map_err(|e| e.code())?;
    Ok(data)
}
fn unmap(resource: &ID3D12Resource) {
    // SAFETY: a buffer this read mapped; nothing written.
    unsafe { resource.Unmap(0, Some(&D3D12_RANGE::default())) };
}
/// Read (the C++ half has checked the pass exists).
///
/// # Safety
/// The objects are live; the callback is the C++ half's.
pub unsafe fn read(s: &mut State, o: &ReadObjects, gate: GateRead, user: *mut c_void, out: &mut Comparison) -> HRESULT {
    if s.failed {
        return E_FAIL;
    }
    let completed = |fence: *mut c_void| {
        // SAFETY: the pass's live fences.
        borrowed::<ID3D12Fence>(fence).map_or(0, |f| unsafe { f.GetCompletedValue() })
    };
    let (done, main) = (completed(o.done_fence), completed(o.main_fence));
    if done == u64::MAX || main == u64::MAX {
        return DXGI_ERROR_DEVICE_REMOVED;
    }
    if !s.peer_submitted || done < s.serial || main < s.serial {
        return S_FALSE;
    }
    if !o.timestamps.is_null() && s.result.complete == 0 {
        for side in 0..2usize {
            if !s.timing_ended[side] || s.gpu_times[side * 3 + 2] == 0 {
                return E_FAIL;
            }
            let Some(readback) = borrowed::<ID3D12Resource>(o.timing[side]) else { return E_FAIL };
            let data = match map(&readback, 16) {
                Ok(d) => d,
                Err(hr) => return hr,
            };
            if !data.is_null() {
                // SAFETY: the mapped 16 bytes: two timestamps.
                let times = unsafe { core::ptr::read_unaligned(data as *const [u64; 2]) };
                s.gpu_times[side * 3] = times[0];
                s.gpu_times[side * 3 + 1] = times[1];
            }
            unmap(&readback);
            if data.is_null() || s.gpu_times[side * 3 + 1] < s.gpu_times[side * 3] {
                return E_FAIL;
            }
        }
    }
    if s.native_counts[0] == 3 {
        s.result.complete = 1;
        *out = s.result;
        return S_OK;
    }
    if s.native_counts[0] == 2 {
        // SAFETY: the C++ half's gate.
        let hr = unsafe { gate(user, &mut s.optimized[1]) };
        if hr.is_err() {
            return hr;
        }
        s.result.complete = 1;
        *out = s.result;
        return S_OK;
    }
    if s.result.complete != 0 {
        *out = s.result;
        return S_OK;
    }
    let Some(readback) = borrowed::<ID3D12Resource>(o.readback) else { return E_FAIL };
    let data = match map(&readback, s.bytes as usize) {
        Ok(d) => d,
        Err(hr) => return hr,
    };
    if data.is_null() {
        unmap(&readback);
        return E_FAIL;
    }
    // SAFETY: the mapped readback, [0, bytes).
    let image = unsafe { core::slice::from_raw_parts(data as *const u8, s.bytes as usize) };
    let native = s.native_counts[0] != 0;
    let (planes, mut result, mut differences) = (s.planes, s.result, s.native_differences);
    if !compare(image, &planes, native, &mut result, &mut differences) {
        unmap(&readback);
        return E_FAIL;
    }
    s.result = result;
    s.native_differences = differences;
    unmap(&readback);
    s.result.complete = 1;
    *out = s.result;
    S_OK
}

/// The C++ half's gate readback on the peer list.
pub type GateReadback = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void) -> bool;
/// Readback after the C++ half's checks, its recording token and the peer list.
///
/// # Safety
/// The list and objects are live; the callback is the C++ half's.
#[allow(clippy::too_many_arguments)]
pub unsafe fn readback(s: &mut State, list: &ID3D12GraphicsCommandList, targets: [*mut c_void; 2], buffer: *mut c_void, timestamps: *mut c_void, timing: *mut c_void, gate: GateReadback, user: *mut c_void) -> bool {
    if s.native_counts[0] >= 2 {
        // SAFETY: the C++ half's gate.
        if !s.native_finished[0] || !s.native_finished[1] || (s.native_counts[0] == 2 && !unsafe { gate(user, list.as_raw()) }) {
            return false;
        }
        // SAFETY: as documented.
        unsafe { end_timing(s, list, timestamps, timing, 1) };
        s.copied = true;
        return true;
    }
    // SAFETY: the list recording; the arrays and the readback are the pass's.
    unsafe {
        for &target in &targets {
            list.ResourceBarrier(&[transition(target, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
        }
        for plane in 0..2u32 {
            for eye in 0..2u32 {
                for m in 0..2u32 {
                    let from = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(targets[if m != 0 { 0 } else { 1 }]),
                        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: eye + plane * 2 },
                    };
                    let to = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(buffer),
                        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: s.planes[(plane * 4 + eye * 2 + m) as usize].layout },
                    };
                    list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
                }
            }
        }
        end_timing(s, list, timestamps, timing, 1);
    }
    s.copied = true;
    true
}

/// Submitted; true when the main list's queue becomes the pass's.
///
/// # Safety
/// The queue and fences are live; `lists` holds `count` lists.
#[allow(clippy::too_many_arguments)]
pub unsafe fn submitted(s: &mut State, queue: &ID3D12CommandQueue, lists: &[*mut c_void], main_list: *mut c_void, peer_list: *mut c_void, timestamps: bool, main_fence: *mut c_void, done_fence: *mut c_void) -> bool {
    let mut main_queue = false;
    let signal = |fence: *mut c_void, value: u64| {
        // SAFETY: the pass's live fences on a live queue.
        borrowed::<ID3D12Fence>(fence).is_some_and(|f| unsafe { queue.Signal(&f, value) }.is_ok())
    };
    // SAFETY: a live queue.
    let frequency = || unsafe { queue.GetTimestampFrequency() };
    for &list in lists {
        if !s.main_submitted && s.submission_pending[0] && list == main_list {
            main_queue = true;
            if timestamps {
                match frequency() {
                    Ok(f) => {
                        s.gpu_times[2] = f;
                        s.failed |= f == 0;
                    }
                    Err(_) => s.failed = true,
                }
            }
            s.failed |= !signal(main_fence, s.serial);
            s.main_submitted = true;
            s.submission_pending[0] = false;
        } else if !s.peer_submitted && s.submission_pending[1] && list == peer_list {
            if timestamps {
                match frequency() {
                    Ok(f) => {
                        s.gpu_times[5] = f;
                        s.failed |= f == 0;
                    }
                    Err(_) => s.failed = true,
                }
            }
            s.failed |= !signal(done_fence, s.serial);
            s.peer_submitted = true;
            s.submission_pending[1] = false;
        }
    }
    main_queue
}

/// Reset of a recorded list: its recording retired (failed when it was never submitted).
pub fn reset(s: &mut State, list: *mut c_void, main_list: *mut c_void, peer_list: *mut c_void, tokens: [bool; 2]) {
    for side in 0..2 {
        if list == if side != 0 { peer_list } else { main_list } && !s.recording_retired[side] && tokens[side] {
            if !(if side != 0 { s.peer_submitted } else { s.main_submitted }) {
                s.failed = true;
            }
            s.recording_retired[side] = true;
        }
    }
}
/// Whether Restart may run (the optimized mode, not failed, no augmented read, a serial left).
pub fn restart_ready(s: &State) -> bool {
    s.native_counts[0] == 2 && !s.failed && !s.instance_augmented && s.serial < u64::MAX - 1
}
/// Restart's state (after the read, the gate and the arenas restarted): the snapshot progress, the permits zeroed,
/// the next serial, the native and optimized counters, the submissions and recordings, the result but its size.
///
/// # Safety
/// The permits are the pass's mapped UPLOAD words (or null).
pub unsafe fn restart(s: &mut State, permits: [*mut c_void; 2]) {
    s.instance_bytes = 0;
    s.shader_bytes = 0;
    s.shader_ready = true;
    s.shader_stats = [0; 4];
    s.alpha_captures = 0;
    s.alpha_pending_sequence = 0;
    s.skin_pending_sequence = 0;
    s.alpha_stats = [0; 6];
    for mapped in permits {
        if !mapped.is_null() {
            // SAFETY: as documented.
            unsafe { core::ptr::write_volatile(mapped as *mut u64, 0) };
        }
    }
    // SAFETY: a store fence for the write-combined permits.
    unsafe { core::arch::x86_64::_mm_sfence() };
    s.serial += 1;
    s.native_dsv = [0; 2];
    s.native_counts = [2, 0, 0, 0, 0, 0, 0, 0];
    s.native_differences = [0; 4];
    s.optimized = [1, u64::MAX, 0, 0, 0, 0, 0, 0];
    s.gpu_times = [0; 6];
    s.native_finished = [false; 2];
    s.timing_ended = [false; 2];
    s.submission_pending = [false; 2];
    s.recording_retired = [false; 2];
    s.initialized = false;
    s.copied = false;
    s.main_submitted = false;
    s.peer_submitted = false;
    let (width, height) = (s.result.width, s.result.height);
    s.result = Comparison { width, height, ..Comparison::default() };
}

/// A copied buffer range (std::pair<UINT64, UINT>).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub from: u64,
    pub bytes: u32,
}
const READ_STATES: i32 = D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0
    | D3D12_RESOURCE_STATE_INDEX_BUFFER.0
    | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0
    | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0
    | D3D12_RESOURCE_STATE_COPY_SOURCE.0
    | D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT.0;
/// A vertex buffer read state the copy may borrow (and nothing but read states).
pub fn readable(state: i32) -> bool {
    state & D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0 != 0 && state & !READ_STATES == 0
}
/// CopyReadableBuffer: the ranges copied back to back from `offset`, the source through COPY_SOURCE when it is not.
///
/// # Safety
/// The list and resources are live.
pub unsafe fn copy_readable(list: &ID3D12GraphicsCommandList, source: *mut c_void, state: i32, destination: *mut c_void, ranges: &[Range], mut offset: u64) -> bool {
    if !readable(state) {
        return false;
    }
    let (Some(from), Some(to)) = (borrowed::<ID3D12Resource>(source), borrowed::<ID3D12Resource>(destination)) else { return false };
    let transition_needed = state & D3D12_RESOURCE_STATE_COPY_SOURCE.0 == 0;
    // SAFETY: the list recording on live resources.
    unsafe {
        if transition_needed {
            list.ResourceBarrier(&[transition(source, D3D12_RESOURCE_STATES(state), D3D12_RESOURCE_STATE_COPY_SOURCE)]);
        }
        for r in ranges {
            list.CopyBufferRegion(&to, offset, &from, r.from, r.bytes as u64);
            offset += r.bytes as u64;
        }
        if transition_needed {
            list.ResourceBarrier(&[transition(source, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATES(state))]);
        }
    }
    true
}
/// ReleaseInstanceRead: a combined read's augmented source back to its own state.
///
/// # Safety
/// The list and the instance source are live.
pub unsafe fn release_instance_read(s: &mut State, list: *mut c_void, main_list: *mut c_void, source: *mut c_void) {
    if !s.instance_augmented || list != main_list {
        return;
    }
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) else { return };
    let before = D3D12_RESOURCE_STATES(s.instance_before);
    // SAFETY: the list recording on the live source.
    unsafe { list.ResourceBarrier(&[transition(source, before | D3D12_RESOURCE_STATE_COPY_SOURCE, before)]) };
    s.instance_augmented = false;
}
/// The instance capture's objects (in and out).
#[repr(C)]
pub struct Instances {
    pub snapshots: [*mut c_void; 2],
    pub source: *mut c_void,
    pub range_count: usize,
}
/// The C++ half's command resources: the list keeps the objects alive through its submission.
pub type Keep = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, *const *mut c_void, usize) -> bool;
/// CaptureInstances; true when the range was recorded (the C++ half then keeps it). New snapshots come out with their
/// creation's reference.
///
/// # Safety
/// The device, list and source are live; the callbacks are the C++ half's.
#[allow(clippy::too_many_arguments)]
pub unsafe fn capture_instances(
    s: &mut State,
    device: &ID3D12Device,
    list: *mut c_void,
    main_list: *mut c_void,
    source: *mut c_void,
    offset: u64,
    bytes: u32,
    state: i32,
    combined: bool,
    o: &mut Instances,
    keep: Keep,
    storage: *mut c_void,
    user: *mut c_void,
) -> bool {
    if list.is_null() || source.is_null() || s.native_counts[0] != 2 || main_list != list || s.main_submitted || s.native_finished[0] || s.failed || bytes == 0 || bytes % 4 != 0 || offset % 4 != 0 {
        return false;
    }
    let (Some(resource), Some(recording)) = (borrowed::<ID3D12Resource>(source), borrowed::<ID3D12GraphicsCommandList>(list)) else { return false };
    // SAFETY: a live resource.
    let desc = unsafe { resource.GetDesc() };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: as above.
    let same = unsafe { resource.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|d| d.as_raw() == device.as_raw());
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0
        || offset > desc.Width
        || bytes as u64 > desc.Width - offset
        || o.range_count >= MAX_DRAWS
        || bytes > MAX_INSTANCE_BYTES - s.instance_bytes
        || (!o.source.is_null() && o.source != source)
        || !same
    {
        return false;
    }
    if !readable(state) || (s.instance_augmented && state != s.instance_before) {
        return false;
    }
    if o.snapshots[0].is_null() {
        let mut pending = [core::ptr::null_mut(); 2];
        for item in &mut pending {
            match committed(device, D3D12_HEAP_TYPE_DEFAULT, &buffer(MAX_INSTANCE_BYTES as u64), D3D12_RESOURCE_STATE_COPY_DEST) {
                Ok(r) => *item = r.into_raw(),
                Err(_) => {
                    for made in pending.iter().filter(|p| !p.is_null()) {
                        // SAFETY: the reference this creation took.
                        drop(unsafe { ID3D12Resource::from_raw(*made) });
                    }
                    return false;
                }
            }
        }
        o.snapshots = pending;
    }
    // SAFETY: the C++ half's command resources.
    if !unsafe { keep(user, list, [storage, source].as_ptr(), 2) } {
        return false;
    }
    // SAFETY: the list recording on the pass's snapshots and the live source.
    unsafe {
        if s.instances_readable {
            recording.ResourceBarrier(&[
                transition(o.snapshots[0], D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
                transition(o.snapshots[1], D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
            ]);
            s.instances_readable = false;
        }
        let range = Range { from: offset, bytes };
        if !combined {
            release_instance_read(s, list, main_list, o.source);
            if !copy_readable(&recording, source, state, o.snapshots[0], &[range], s.instance_bytes as u64) {
                return false;
            }
        } else {
            if !s.instance_augmented && state & D3D12_RESOURCE_STATE_COPY_SOURCE.0 == 0 {
                recording.ResourceBarrier(&[transition(source, D3D12_RESOURCE_STATES(state), D3D12_RESOURCE_STATES(state | D3D12_RESOURCE_STATE_COPY_SOURCE.0))]);
                s.instance_before = state;
                s.instance_augmented = true;
            }
            let Some(snapshot) = borrowed::<ID3D12Resource>(o.snapshots[0]) else { return false };
            recording.CopyBufferRegion(&snapshot, s.instance_bytes as u64, &resource, offset, bytes as u64);
        }
    }
    o.source = source;
    s.instance_bytes += bytes;
    true
}

// ---- the C ABI (every call under the C++ half's checks and its probe scope)

/// # Safety
/// `device` is live; `state` and `out` are writable; the callbacks are the C++ half's.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_create(device: *mut c_void, width: u32, height: u32, mode: u32, state: *mut State, out: *mut Created, init: InitTargets, gate: CreateGate, user: *mut c_void) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    // SAFETY: as documented.
    match unsafe { create(&device, width, height, mode, &mut *state, init, gate, user) } {
        Ok(created) => {
            // SAFETY: as documented.
            unsafe { *out = created };
            S_OK
        }
        Err(hr) => hr,
    }
}
/// # Safety
/// `state` is the pass's; the objects are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_end_timing(state: *mut State, list: *mut c_void, timestamps: *mut c_void, readback: *mut c_void, side: u32) {
    if let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) {
        // SAFETY: as documented.
        unsafe { end_timing(&mut *state, &list, timestamps, readback, side) };
    }
}
/// # Safety
/// As for read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_read(state: *mut State, objects: *const ReadObjects, gate: GateRead, user: *mut c_void, out: *mut Comparison) -> HRESULT {
    // SAFETY: as documented.
    unsafe { read(&mut *state, &*objects, gate, user, &mut *out) }
}
/// # Safety
/// As for readback.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_readback(state: *mut State, list: *mut c_void, targets: *const [*mut c_void; 2], buffer: *mut c_void, timestamps: *mut c_void, timing: *mut c_void, gate: GateReadback, user: *mut c_void) -> bool {
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    // SAFETY: as documented.
    unsafe { readback(&mut *state, &list, *targets, buffer, timestamps, timing, gate, user) }
}
/// # Safety
/// As for submitted.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_submitted(state: *mut State, queue: *mut c_void, lists: *const *mut c_void, count: u32, main_list: *mut c_void, peer_list: *mut c_void, timestamps: bool, main_fence: *mut c_void, done_fence: *mut c_void) -> bool {
    let Some(queue) = borrowed::<ID3D12CommandQueue>(queue) else { return false };
    // SAFETY: as documented.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    // SAFETY: as documented.
    unsafe { submitted(&mut *state, &queue, lists, main_list, peer_list, timestamps, main_fence, done_fence) }
}
/// # Safety
/// `state` is the pass's.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_reset(state: *mut State, list: *mut c_void, main_list: *mut c_void, peer_list: *mut c_void, main_token: bool, peer_token: bool) {
    // SAFETY: as documented.
    reset(unsafe { &mut *state }, list, main_list, peer_list, [main_token, peer_token]);
}
/// # Safety
/// `state` is the pass's.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_restart_ready(state: *const State) -> bool {
    // SAFETY: as documented.
    restart_ready(unsafe { &*state })
}
/// # Safety
/// As for restart.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_restart(state: *mut State, permits: *const [*mut c_void; 2]) {
    // SAFETY: as documented.
    unsafe { restart(&mut *state, *permits) };
}
/// # Safety
/// As for copy_readable; `ranges` holds `count` ranges.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_copy_readable(list: *mut c_void, source: *mut c_void, state: i32, destination: *mut c_void, ranges: *const Range, count: usize, offset: u64) -> bool {
    let Some(list) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    // SAFETY: as documented.
    let ranges = if count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(ranges, count) } };
    // SAFETY: as documented.
    unsafe { copy_readable(&list, source, state, destination, ranges, offset) }
}
/// # Safety
/// As for release_instance_read.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_release_instance_read(state: *mut State, list: *mut c_void, main_list: *mut c_void, source: *mut c_void) {
    // SAFETY: as documented.
    unsafe { release_instance_read(&mut *state, list, main_list, source) };
}
/// # Safety
/// As for capture_instances.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_capture_instances(
    state: *mut State,
    device: *mut c_void,
    list: *mut c_void,
    main_list: *mut c_void,
    source: *mut c_void,
    offset: u64,
    bytes: u32,
    resource_state: i32,
    combined: bool,
    objects: *mut Instances,
    keep: Keep,
    storage: *mut c_void,
    user: *mut c_void,
) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    // SAFETY: as documented.
    unsafe { capture_instances(&mut *state, &device, list, main_list, source, offset, bytes, resource_state, combined, &mut *objects, keep, storage, user) }
}

// ---- the native drawing and submission (B278: FinishNative, Original, PeerRemainder, ObserveNative, CommitNative,
// AbortNative, BeforeSubmit)

/// The C++ half's objects and operations the drawing reaches: the command resources, the target arrays (every state
/// DEPTH_WRITE), the commit gate and the shader sources' submission checks. `user` is the Storage (its IUnknown).
#[repr(C)]
pub struct Host {
    pub user: *mut c_void,
    /// KeepCommandResources(list, {objects}).
    pub keep: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, *const *mut c_void, usize) -> bool,
    /// HasCommandResource(list, object).
    pub has: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, *mut c_void) -> bool,
    /// The target array `array`'s operation (0 CopyEye, 1 ImportEye, 2 ImportEyes) on `list` for `eye` (a mask for
    /// ImportEyes) with the native target.
    pub target: unsafe extern "C-unwind" fn(*mut c_void, u32, u32, *mut c_void, u32, *mut c_void) -> HRESULT,
    /// The gate's operation: 0 Bind(list, flag), 1 Decide(flag), 2 Freeze.
    pub gate: unsafe extern "C-unwind" fn(*mut c_void, u32, *mut c_void, bool) -> bool,
    /// A shader source's submission check on `queue` for `side`: 0 valid, 1 its R8 texture invalid, 2 its texture
    /// invalid, 3 its material snapshot invalid.
    pub shader_source: unsafe extern "C-unwind" fn(*mut c_void, usize, *mut c_void, u32) -> u32,
    pub shader_sources: usize,
}
/// The pass's objects the drawing reads (raw: the Storage holds them).
#[repr(C)]
pub struct Objects {
    pub main_list: *mut c_void,
    pub peer_list: *mut c_void,
    pub native: [*mut c_void; 2],
    pub dsvs: *mut c_void,
    pub timestamps: *mut c_void,
    pub timing: [*mut c_void; 2],
    pub readback: *mut c_void,
    pub instance_source: *mut c_void,
    pub tokens: [*mut c_void; 2],
    pub permits: [*mut c_void; 2],
    pub main_queue: *mut c_void,
    pub main_fence: *mut c_void,
}
/// InstanceRun.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Run {
    pub first: u32,
    pub count: u32,
}
use crate::geometry_commands::DrawRecord;

fn draw(list: &ID3D12GraphicsCommandList, d: &DrawRecord) {
    // SAFETY: a live list recording.
    unsafe { list.DrawIndexedInstanced(d.indices, d.instances, d.first_index, d.base_vertex, d.first_instance) };
}
fn depth_target(list: &ID3D12GraphicsCommandList, handle: u64) {
    let target = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: handle as usize };
    // SAFETY: a live list recording; the handle is the pass's or the draw's DSV.
    unsafe { list.OMSetRenderTargets(0, None, false, Some(&target)) };
}
fn pass_dsv(s: &State, o: &Objects, index: u32) -> u64 {
    // SAFETY: the pass's live DSV heap.
    let base = borrowed::<ID3D12DescriptorHeap>(o.dsvs).map_or(0, |h| unsafe { h.GetCPUDescriptorHandleForHeapStart() }.ptr);
    (base + index as usize * s.stride as usize) as u64
}
fn keep_storage(host: &Host, list: *mut c_void) -> bool {
    // SAFETY: the C++ half's command resources.
    unsafe { (host.keep)(host.user, list, [host.user].as_ptr(), 1) }
}
fn gate(host: &Host, op: u32, list: *mut c_void, flag: bool) -> bool {
    // SAFETY: the C++ half's gate.
    unsafe { (host.gate)(host.user, op, list, flag) }
}
fn target(host: &Host, array: u32, op: u32, list: *mut c_void, eye: u32, native: *mut c_void) -> bool {
    // SAFETY: the C++ half's target arrays.
    unsafe { (host.target)(host.user, array, op, list, eye, native) }.is_ok()
}

/// FinishNative (the C++ half's probe scope around it).
///
/// # Safety
/// The objects are live.
pub unsafe fn finish_native(s: &mut State, o: &Objects, list: *mut c_void, side: u32, host: &Host) -> bool {
    let i = side as usize;
    if list.is_null() || side > 1 || s.native_counts[0] == 0 || o.native[i].is_null() || s.copied || list != if side != 0 { o.peer_list } else { o.main_list } || s.native_finished[i] {
        return false;
    }
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    if !keep_storage(host, list) {
        return false;
    }
    // SAFETY: as documented.
    unsafe {
        if side == 0 {
            release_instance_read(s, list, o.main_list, o.instance_source);
        }
        if s.native_counts[0] == 3 {
            s.native_finished[i] = true;
            if side == 0 {
                end_timing(s, &recording, o.timestamps, o.timing[0], 0);
            }
            return true;
        }
        if s.native_counts[0] == 2 {
            if side == 0 && !target(host, 0, 0, list, 1, o.native[0]) {
                return false;
            }
            depth_target(&recording, s.native_dsv[i]);
            s.native_finished[i] = true;
            if side == 0 {
                end_timing(s, &recording, o.timestamps, o.timing[0], 0);
            }
            s.native_counts[5 + i] += 1;
            return true;
        }
        // Capture actual native 2D output, then verify a round-trip through the array. The paired result remains
        // independent, including its view type.
        let eye = if side != 0 { 0 } else { 1 };
        if !target(host, 1, 1, list, eye, o.native[i]) || !target(host, 1, 0, list, eye, o.native[i]) {
            return false;
        }
        recording.ResourceBarrier(&[transition(o.native[i], D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
        for plane in 0..2u32 {
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(o.native[i]),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: plane },
            };
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(o.readback),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: s.planes[(8 + plane * 2 + if side != 0 { 0 } else { 1 }) as usize].layout },
            };
            recording.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        }
        recording.ResourceBarrier(&[transition(o.native[i], D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_DEPTH_WRITE)]);
        s.native_finished[i] = true;
        if side == 0 {
            end_timing(s, &recording, o.timestamps, o.timing[0], 0);
        }
        s.native_counts[5 + i] += 1;
        true
    }
}

/// PeerRemainder: the peer draw's remaining instance runs (the gate around a partial set).
///
/// # Safety
/// The objects are live.
pub unsafe fn peer_remainder(s: &mut State, o: &Objects, list: *mut c_void, d: &DrawRecord, remaining: &[Run], host: &Host) -> bool {
    if list.is_null()
        || s.native_counts[0] == 0
        || s.native_counts[0] > 2
        || s.native_finished[1]
        || o.native[1].is_null()
        || o.peer_list != list
        || !s.main_submitted
        || d.side != 1
        || d.rt_count != 0
        || d.depth != s.native_dsv[1]
        || s.failed
        || s.copied
        || d.indices == 0
        || d.instances == 0
        || d.first_instance > u32::MAX - d.instances
    {
        return false;
    }
    let mut end = 0u32;
    for run in remaining {
        if run.count == 0 || run.first < end || run.first > d.instances || run.count > d.instances - run.first {
            return false;
        }
        end = run.first + run.count;
    }
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    if !keep_storage(host, list) {
        return false;
    }
    let part = |run: &Run| {
        let mut p = *d;
        p.first_instance += run.first;
        p.instances = run.count;
        p
    };
    if s.native_counts[0] == 2 {
        depth_target(&recording, d.depth);
        if remaining.len() == 1 && remaining[0].first == 0 && remaining[0].count == d.instances {
            draw(&recording, d);
            s.optimized[3] += 1;
        } else {
            if !gate(host, 0, list, true) {
                return false;
            }
            for run in remaining {
                draw(&recording, &part(run));
            }
            // SAFETY: a live list recording.
            let unpredicate = || unsafe { recording.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO) };
            if !gate(host, 0, list, false) {
                unpredicate();
                return false;
            }
            draw(&recording, d);
            unpredicate();
            s.optimized[4] += remaining.len() as u64;
            s.optimized[5] += 1;
        }
        s.native_counts[2] += 1;
        s.native_counts[4] += remaining.len() as u32;
        return true;
    }
    depth_target(&recording, pass_dsv(s, o, 3));
    for run in remaining {
        draw(&recording, &part(run));
    }
    depth_target(&recording, d.depth);
    draw(&recording, d);
    s.optimized[6] += 1;
    depth_target(&recording, d.depth);
    s.native_counts[2] += 1;
    s.native_counts[4] += remaining.len() as u32;
    true
}
/// Original: the main draw into the pass's reference (and the native target outside the optimized mode); a peer draw
/// is its whole remainder.
///
/// # Safety
/// The objects are live.
pub unsafe fn original(s: &mut State, o: &Objects, list: *mut c_void, d: &DrawRecord, host: &Host) -> bool {
    if d.side == 1 {
        // SAFETY: as documented.
        return unsafe { peer_remainder(s, o, list, d, &[Run { first: 0, count: d.instances }], host) };
    }
    if list.is_null()
        || s.native_counts[0] == 0
        || s.native_counts[0] > 2
        || s.native_finished[0]
        || o.native[0].is_null()
        || o.main_list != list
        || s.main_submitted
        || d.side != 0
        || d.rt_count != 0
        || d.depth != s.native_dsv[0]
        || s.failed
        || d.indices == 0
        || d.instances == 0
    {
        return false;
    }
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
    if !keep_storage(host, list) {
        return false;
    }
    depth_target(&recording, pass_dsv(s, o, 4));
    draw(&recording, d);
    if s.native_counts[0] != 2 {
        depth_target(&recording, d.depth);
        draw(&recording, d);
        s.optimized[6] += 1;
    }
    if s.native_counts[0] == 2 {
        s.optimized[2] += 1;
    }
    s.skin_pending_sequence = 0;
    s.result.draws += 1;
    s.native_counts[1] += 1;
    s.native_counts[3] += 1;
    true
}
/// ObserveNative: a native draw of the observe-only mode counted.
pub fn observe_native(s: &mut State, o: &Objects, list: *mut c_void, d: &DrawRecord) -> bool {
    let side = d.side as usize;
    if list.is_null()
        || d.side > 1
        || s.native_counts[0] != 3
        || s.failed
        || s.copied
        || s.native_finished[side]
        || o.native[side].is_null()
        || d.depth != s.native_dsv[side]
        || list != if side != 0 { o.peer_list } else { o.main_list }
    {
        return false;
    }
    s.native_counts[1 + side] += 1;
    if side == 0 {
        s.result.draws += 1;
    }
    true
}
/// CommitNative: the gate's decision (never allowed once the shader snapshots were rejected).
pub fn commit_native(s: &mut State, allow: bool, host: &Host) -> bool {
    if s.native_counts[0] != 2 {
        return false;
    }
    let allow = allow && s.shader_ready;
    if !gate(host, 1, core::ptr::null_mut(), allow) {
        return false;
    }
    s.optimized[7] = allow as u64;
    true
}
/// RejectShaderSnapshots.
pub fn reject_shader_snapshots(s: &mut State, host: &Host) {
    s.shader_ready = false;
    commit_native(s, false, host);
}
/// BeforeSubmit: each recorded side submitted once (a replay or repeat fails the pass), its shader sources checked on
/// the queue and its snapshot permit written; the peer waits on the main fence across queues.
///
/// # Safety
/// The objects are live; `lists` holds the submission.
pub unsafe fn before_submit(s: &mut State, o: &Objects, queue: &ID3D12CommandQueue, lists: &[*mut c_void], host: &Host) {
    for side in 0..2usize {
        let submitted = if side != 0 { s.peer_submitted } else { s.main_submitted };
        let expected = if side != 0 { o.peer_list } else { o.main_list };
        let occurrences = lists.iter().filter(|&&l| l == expected).count();
        // SAFETY: the C++ half's command resources.
        if occurrences == 0 || s.recording_retired[side] || o.tokens[side].is_null() || !unsafe { (host.has)(host.user, expected, o.tokens[side]) } {
            continue;
        }
        // Never rewrite an UPLOAD permit which an earlier submission may still read. Replay violates the caller's
        // one-shot native recording contract.
        if submitted || s.submission_pending[side] {
            s.failed = true;
            continue;
        }
        s.submission_pending[side] = true;
        if occurrences != 1 {
            s.failed = true;
        }
        if !o.permits[side].is_null() {
            if s.alpha_pending_sequence != 0 || s.skin_pending_sequence != 0 {
                s.alpha_stats[5] = 9;
                reject_shader_snapshots(s, host);
            }
            for index in 0..host.shader_sources {
                // SAFETY: the C++ half's checks.
                match unsafe { (host.shader_source)(host.user, index, queue.as_raw(), side as u32) } {
                    0 => continue,
                    2 => s.alpha_stats[5] = 4000 + side as u64,
                    3 => s.alpha_stats[5] = 3000 + side as u64,
                    _ => {}
                }
                reject_shader_snapshots(s, host);
                break;
            }
            // SAFETY: the pass's mapped UPLOAD permit; a store fence for the write-combined word.
            unsafe {
                core::ptr::write_volatile(o.permits[side] as *mut u64, (s.shader_ready && !s.failed) as u64);
                core::arch::x86_64::_mm_sfence();
            }
        }
    }
    if s.main_submitted && !s.peer_submitted && s.submission_pending[1] {
        if s.native_counts[0] == 2 {
            gate(host, 2, core::ptr::null_mut(), false);
        }
        if s.native_counts[0] != 3 && queue.as_raw() != o.main_queue {
            let waited = borrowed::<ID3D12Fence>(o.main_fence).is_some_and(|f| unsafe { queue.Wait(&f, s.serial) }.is_ok());
            if !waited {
                s.failed = true;
            }
        }
    }
}

/// # Safety
/// As for finish_native.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_finish_native(state: *mut State, objects: *const Objects, list: *mut c_void, side: u32, host: *const Host) -> bool {
    // SAFETY: as documented.
    unsafe { finish_native(&mut *state, &*objects, list, side, &*host) }
}
/// # Safety
/// As for original; `draw` is the native draw.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_original(state: *mut State, objects: *const Objects, list: *mut c_void, draw: *const DrawRecord, host: *const Host) -> bool {
    // SAFETY: as documented.
    unsafe { original(&mut *state, &*objects, list, &*draw, &*host) }
}
/// # Safety
/// As for peer_remainder; `runs` holds `count` runs.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_peer_remainder(state: *mut State, objects: *const Objects, list: *mut c_void, draw: *const DrawRecord, runs: *const Run, count: usize, host: *const Host) -> bool {
    // SAFETY: as documented.
    let runs = if count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(runs, count) } };
    // SAFETY: as documented.
    unsafe { peer_remainder(&mut *state, &*objects, list, &*draw, runs, &*host) }
}
/// # Safety
/// `state`, `objects` and `draw` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_observe_native(state: *mut State, objects: *const Objects, list: *mut c_void, draw: *const DrawRecord) -> bool {
    // SAFETY: as documented.
    observe_native(unsafe { &mut *state }, unsafe { &*objects }, list, unsafe { &*draw })
}
/// # Safety
/// `state` and `host` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_commit_native(state: *mut State, allow: bool, host: *const Host) -> bool {
    // SAFETY: as documented.
    commit_native(unsafe { &mut *state }, allow, unsafe { &*host })
}
/// # Safety
/// As for before_submit; `lists` holds `count` lists.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_before_submit(state: *mut State, objects: *const Objects, queue: *mut c_void, lists: *const *mut c_void, count: u32, host: *const Host) {
    let Some(queue) = borrowed::<ID3D12CommandQueue>(queue) else { return };
    // SAFETY: as documented.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    // SAFETY: as documented.
    unsafe { before_submit(&mut *state, &*objects, &queue, lists, &*host) };
}

// ---- StartNative and Main (B278)

/// StartNative's and Main's C++ operations: the recording token (Track), the peer's gate (the instance and shader
/// proofs, the shader sources' copies, Prepare and Bind), and for Main the bank's capture and bind and the restore
/// of the game's bindings.
#[repr(C)]
pub struct DrawHost {
    pub user: *mut c_void,
    pub track: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32) -> bool,
    pub peer_gate: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, *const DrawRecord) -> bool,
    pub capture: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void) -> bool,
    pub bind: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32) -> bool,
    pub restore: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void),
}
fn device_of(child: *mut c_void) -> u64 {
    let Some(child) = borrowed::<ID3D12DeviceChild>(child) else { return 0 };
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child.
    if unsafe { child.GetDevice(&mut device) }.is_err() {
        return 0;
    }
    device.map_or(0, |d| d.as_raw() as u64)
}
/// StartNative's outcome: refused, accepted (the side's native target already the pass's), or the side's native
/// target taken (the C++ half keeps it and the list).
pub const NATIVE_REFUSED: u32 = 0;
pub const NATIVE_SAME: u32 = 1;
pub const NATIVE_TAKEN: u32 = 2;
/// StartNative.
///
/// # Safety
/// The objects are live; the callbacks are the C++ half's.
#[allow(clippy::too_many_arguments)]
pub unsafe fn start_native(
    s: &mut State,
    o: &Objects,
    device: *mut c_void,
    list: *mut c_void,
    d: &DrawRecord,
    target_resource: *mut c_void,
    instance_source: *mut c_void,
    instance_state: i32,
    ranges: &[Range],
    snapshots: [*mut c_void; 2],
    host: &Host,
    draws: &DrawHost,
) -> u32 {
    if list.is_null() || target_resource.is_null() || d.side > 1 || d.rt_count != 0 || d.depth == 0 || d.flags & 64 != 0 || s.native_counts[0] == 0 || s.failed || s.copied {
        return NATIVE_REFUSED;
    }
    let side = d.side as usize;
    let active = if side != 0 { o.peer_list } else { o.main_list };
    if !o.native[side].is_null() {
        return if o.native[side] == target_resource && s.native_dsv[side] == d.depth && active == list { NATIVE_SAME } else { NATIVE_REFUSED };
    }
    if (side == 0 && s.main_submitted) || (side != 0 && !s.main_submitted) || (!active.is_null() && active != list) {
        return NATIVE_REFUSED;
    }
    let (Some(recording), Some(target_object)) = (borrowed::<ID3D12GraphicsCommandList>(list), borrowed::<ID3D12Resource>(target_resource)) else { return NATIVE_REFUSED };
    // SAFETY: live objects.
    let (desc, kind) = unsafe { (target_object.GetDesc(), recording.GetType()) };
    let device = device as u64;
    if kind != D3D12_COMMAND_LIST_TYPE_DIRECT || device_of(list) != device {
        return NATIVE_REFUSED;
    }
    if device_of(target_resource) != device
        || desc.Width != s.result.width as u64
        || desc.Height != s.result.height
        || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.DepthOrArraySize != 1
        || desc.MipLevels != 1
        || desc.SampleDesc.Count != 1
        || desc.SampleDesc.Quality != 0
        || (desc.Format != DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format != DXGI_FORMAT_D32_FLOAT_S8X24_UINT)
        || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL).0 == 0
    {
        return NATIVE_REFUSED;
    }
    // SAFETY: the C++ half's recording token and command resources.
    if !unsafe { (draws.track)(draws.user, list, side as u32) } || !unsafe { (host.keep)(host.user, list, [host.user, target_resource].as_ptr(), 2) } {
        return NATIVE_REFUSED;
    }
    if let Some(queries) = borrowed::<ID3D12QueryHeap>(o.timestamps) {
        // SAFETY: the list recording on the pass's queries.
        unsafe { recording.EndQuery(&queries, D3D12_QUERY_TYPE_TIMESTAMP, side as u32 * 2) };
    }
    if s.native_counts[0] == 3 {
        s.native_dsv[side] = d.depth;
        s.initialized = true;
        return NATIVE_TAKEN;
    }
    // Every paired layer is fully imported before its first draw. The reference array is overwritten at
    // FinishNative, or initialized here for the GPU gate.
    s.initialized = true;
    let eye = if side != 0 { 0 } else { 1 };
    if s.native_counts[0] == 2 && !target(host, 1, 1, list, eye, target_resource) {
        return NATIVE_REFUSED;
    }
    if side == 0 {
        if !target(host, 0, 2, list, 3, target_resource) {
            return NATIVE_REFUSED;
        }
    } else if s.native_counts[0] == 2 {
        if s.instance_bytes != 0 {
            // SAFETY: the list recording on the pass's snapshots and the live instance source.
            if instance_source != o.instance_source || !unsafe { copy_readable(&recording, instance_source, instance_state, snapshots[1], ranges, 0) } {
                return NATIVE_REFUSED;
            }
            // SAFETY: as above.
            unsafe {
                recording.ResourceBarrier(&[
                    transition(snapshots[0], D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE),
                    transition(snapshots[1], D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE),
                ])
            };
            s.instances_readable = true;
        }
        // SAFETY: the C++ half's proofs, shader copies and gate.
        if !unsafe { (draws.peer_gate)(draws.user, list, d) } {
            return NATIVE_REFUSED;
        }
        let copied = target(host, 0, 0, list, 0, target_resource);
        // SAFETY: a live list recording.
        unsafe { recording.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO) };
        if !copied {
            return NATIVE_REFUSED;
        }
    }
    s.native_dsv[side] = d.depth;
    s.native_counts[7] += 1;
    NATIVE_TAKEN
}

/// Main's admission inputs beyond the draw: the family's contracts and pass, the bank's counts, the masks and the
/// peer-only instance bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Admission {
    pub masks: *const u8,
    pub mask_count: usize,
    pub peer_only: usize,
    pub bank_draw: u32,
    pub bank_draws: u32,
    pub bank_snapshots: u32,
    pub alpha_depth_contract: u32,
    pub skinning_read_contract: u32,
    pub native_camera_stages: u32,
    pub render_targets: u32,
    pub depth_format: i32,
    pub depth_func: i32,
    pub depth_write: i32,
    pub samples_count: u32,
    pub samples_quality: u32,
    pub source_root: u64,
    pub native_camera: bool,
    pub alpha_reference: bool,
    pub alpha_optimized: bool,
    pub family_pipeline: bool,
    pub ordinal_independent: bool,
    pub native_camera_pipeline: bool,
    pub pixel_shader: bool,
    pub fixed_raster: bool,
    pub depth_known: bool,
    pub depth_enable: bool,
}
const _: () = assert!(core::mem::size_of::<DrawHost>() == 48 && core::mem::size_of::<Admission>() == 96);
/// Main's admission: the main draw of a depth-only (or admitted alpha) family into the pass's whole target.
pub fn main_admitted(s: &State, main_list: *mut c_void, native0: bool, list: *mut c_void, d: &DrawRecord, a: &Admission) -> bool {
    // SAFETY: the lent masks.
    let masks = if a.mask_count == 0 || a.masks.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(a.masks, a.mask_count) } };
    if list.is_null()
        || s.native_counts[0] > 2
        || s.native_finished[0]
        || d.side != 0
        || d.rt_count != 0
        || d.depth == 0
        || !a.family_pipeline
        || s.failed
        || s.main_submitted
        || (!main_list.is_null() && main_list != list)
        || (!masks.is_empty()
            && (masks.len() != d.instances as usize || !a.ordinal_independent || d.first_instance > u32::MAX - d.instances || masks.iter().any(|&m| m != 2 && m != 3)))
    {
        return false;
    }
    if s.native_counts[0] != 0 && (!native0 || s.native_dsv[0] != d.depth) {
        return false;
    }
    let vertex = d.vertices[7];
    let stride = vertex.StrideInBytes;
    if a.peer_only != 0
        && (!a.ordinal_independent || stride == 0 || stride > 256 || a.peer_only % stride as usize != 0 || a.peer_only > 16384 || vertex.BufferLocation == 0 || vertex.SizeInBytes == 0)
    {
        return false;
    }
    let alpha_mode = (a.alpha_reference && !a.alpha_optimized && s.native_counts[0] == 1)
        || (a.alpha_optimized && !a.alpha_reference && s.native_counts[0] == 2 && s.shader_ready && s.alpha_pending_sequence != 0 && s.alpha_pending_sequence as u64 == d.sequence);
    let combined = a.alpha_depth_contract == 2 && a.skinning_read_contract == 2;
    let contract = a.alpha_depth_contract == 1 || combined;
    let joint = !combined || !a.alpha_optimized || s.skin_pending_sequence as u64 == d.sequence;
    let allowed = alpha_mode && a.native_camera && contract && joint && a.native_camera_stages == 1 && a.pixel_shader;
    let v = &d.viewport;
    !(((a.alpha_reference || a.alpha_optimized) && !allowed)
        || (a.pixel_shader && !allowed)
        || a.render_targets != 0
        || a.depth_format != DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0
        || !a.fixed_raster
        || !a.depth_known
        || !a.depth_enable
        || a.depth_write != D3D12_DEPTH_WRITE_MASK_ALL.0
        || (a.depth_func != D3D12_COMPARISON_FUNC_GREATER.0 && a.depth_func != D3D12_COMPARISON_FUNC_GREATER_EQUAL.0)
        || a.samples_count != 1
        || a.samples_quality != 0
        || (!a.native_camera && (a.bank_draw >= a.bank_draws || a.bank_snapshots != 0))
        || (a.native_camera && (!a.native_camera_pipeline || d.root != a.source_root))
        || d.flags & 64 != 0
        || d.indices == 0
        || d.instances == 0
        || v.TopLeftX != 0.0
        || v.TopLeftY != 0.0
        || v.MinDepth != 0.0
        || v.MaxDepth != 1.0
        || v.Width != s.result.width as f32
        || v.Height != s.result.height as f32)
}
/// Main's recording outcome bits: recorded, and the main list taken (as the C++ half took it before the bank bind).
pub const MAIN_RECORDED: u32 = 1;
pub const MAIN_TAKEN: u32 = 2;
/// Main's recording (after the admission, the C++ half's restore scope, recording token and camera resources): the
/// peer-only instances' upload, the bank's capture, the arrays' first clear, the paired draw by view-instance mask runs
/// (and the peer-only additions), the reference draw outside the optimized mode.
///
/// # Safety
/// The objects are live; the callbacks are the C++ half's.
#[allow(clippy::too_many_arguments)]
pub unsafe fn main_record(
    s: &mut State,
    o: &Objects,
    device: &ID3D12Device,
    list: *mut c_void,
    d: &DrawRecord,
    masks: &[u8],
    peer_only: &[u8],
    bank_draw: u32,
    native_camera: bool,
    alpha_optimized: bool,
    pipeline: *mut c_void,
    host: &Host,
    draws: &DrawHost,
) -> u32 {
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return 0 };
    let (Ok(views), Ok(rates)) = (recording.cast::<ID3D12GraphicsCommandList1>(), recording.cast::<ID3D12GraphicsCommandList5>()) else { return 0 };
    let stride = d.vertices[7].StrideInBytes;
    let mut extra: Option<ID3D12Resource> = None;
    // SAFETY: a live device and list; the upload buffer is this call's.
    unsafe {
        if !peer_only.is_empty() {
            let Ok(buffer) = committed(device, D3D12_HEAP_TYPE_UPLOAD, &buffer(peer_only.len() as u64), D3D12_RESOURCE_STATE_GENERIC_READ) else { return 0 };
            let none = D3D12_RANGE::default();
            let mut data = core::ptr::null_mut();
            if buffer.Map(0, Some(&none), Some(&mut data)).is_err() {
                return 0;
            }
            if data.is_null() {
                buffer.Unmap(0, Some(&none));
                return 0;
            }
            core::ptr::copy_nonoverlapping(peer_only.as_ptr(), data as *mut u8, peer_only.len());
            buffer.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: peer_only.len() }));
            if !(host.keep)(host.user, list, [buffer.as_raw()].as_ptr(), 1) {
                return 0;
            }
            extra = Some(buffer);
        }
        if !native_camera && !(draws.capture)(draws.user, list) {
            return 0;
        }
        let taken = MAIN_TAKEN;
        if !s.initialized {
            for i in 0..3 {
                let view = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: pass_dsv(s, o, i) as usize };
                recording.ClearDepthStencilView(view, D3D12_CLEAR_FLAG_DEPTH | D3D12_CLEAR_FLAG_STENCIL, 0.0, 0, None);
            }
            s.initialized = true;
        }
        let paired = pass_dsv(s, o, 0);
        let reference = if s.native_counts[0] != 0 { d.depth } else { pass_dsv(s, o, 2) };
        rates.RSSetShadingRateImage(None::<&ID3D12Resource>);
        recording.SetPipelineState(borrowed::<ID3D12PipelineState>(pipeline).as_ref());
        if !native_camera && !(draws.bind)(draws.user, list, bank_draw) {
            return taken;
        }
        depth_target(&recording, paired);
        let mut runs = 0u64;
        if masks.is_empty() {
            views.SetViewInstanceMask(3);
            draw(&recording, d);
            runs += 1;
        } else {
            let mut first = 0u32;
            while first < d.instances {
                let mut end = first + 1;
                while end < d.instances && masks[end as usize] == masks[first as usize] {
                    end += 1;
                }
                let mut run = *d;
                run.first_instance += first;
                run.instances = end - first;
                views.SetViewInstanceMask(masks[first as usize] as u32);
                draw(&recording, &run);
                first = end;
                runs += 1;
            }
        }
        if let Some(buffer) = &extra {
            let view = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: buffer.GetGPUVirtualAddress(), SizeInBytes: peer_only.len() as u32, StrideInBytes: stride };
            let mut additional = *d;
            additional.instances = (peer_only.len() / stride as usize) as u32;
            additional.first_instance = 0;
            recording.IASetVertexBuffers(7, Some(&[view]));
            views.SetViewInstanceMask(1);
            draw(&recording, &additional);
            recording.IASetVertexBuffers(7, Some(&[d.vertices[7]]));
            runs += 1;
        }
        if s.native_counts[0] != 2 {
            if !native_camera {
                (draws.restore)(draws.user, list);
            }
            views.SetViewInstanceMask(u32::MAX);
            recording.SetPipelineState(borrowed::<ID3D12PipelineState>(d.pipeline as *mut c_void).as_ref());
            depth_target(&recording, reference);
            draw(&recording, d);
            s.optimized[6] += 1;
        } else {
            s.optimized[2] += runs;
        }
        if alpha_optimized {
            s.alpha_pending_sequence = 0;
        }
        s.skin_pending_sequence = 0;
        s.result.draws += 1;
        if s.native_counts[0] != 0 {
            s.native_counts[1] += 1;
        }
        MAIN_RECORDED | taken
    }
}

/// # Safety
/// As for start_native; `ranges` holds `range_count` ranges.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_start_native(
    state: *mut State,
    objects: *const Objects,
    device: *mut c_void,
    list: *mut c_void,
    draw: *const DrawRecord,
    target_resource: *mut c_void,
    instance_source: *mut c_void,
    instance_state: i32,
    ranges: *const Range,
    range_count: usize,
    snapshots: *const [*mut c_void; 2],
    host: *const Host,
    draws: *const DrawHost,
) -> u32 {
    // SAFETY: as documented.
    unsafe {
        let ranges = if range_count == 0 { &[][..] } else { core::slice::from_raw_parts(ranges, range_count) };
        start_native(&mut *state, &*objects, device, list, &*draw, target_resource, instance_source, instance_state, ranges, *snapshots, &*host, &*draws)
    }
}
/// # Safety
/// `state`, `draw` and `admission` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_main_admitted(state: *const State, main_list: *mut c_void, native0: bool, list: *mut c_void, draw: *const DrawRecord, admission: *const Admission) -> bool {
    // SAFETY: as documented.
    main_admitted(unsafe { &*state }, main_list, native0, list, unsafe { &*draw }, unsafe { &*admission })
}
/// # Safety
/// As for main_record; the masks and peer-only bytes hold their counts.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_main_record(
    state: *mut State,
    objects: *const Objects,
    device: *mut c_void,
    list: *mut c_void,
    draw: *const DrawRecord,
    masks: *const u8,
    mask_count: usize,
    peer_only: *const u8,
    peer_only_bytes: usize,
    bank_draw: u32,
    native_camera: bool,
    alpha_optimized: bool,
    pipeline: *mut c_void,
    host: *const Host,
    draws: *const DrawHost,
) -> u32 {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return 0 };
    // SAFETY: as documented.
    unsafe {
        let masks = if mask_count == 0 || masks.is_null() { &[][..] } else { core::slice::from_raw_parts(masks, mask_count) };
        let peer = if peer_only_bytes == 0 || peer_only.is_null() { &[][..] } else { core::slice::from_raw_parts(peer_only, peer_only_bytes) };
        main_record(&mut *state, &*objects, &device, list, &*draw, masks, peer, bank_draw, native_camera, alpha_optimized, pipeline, &*host, &*draws)
    }
}

// ---- the shader snapshots (B279: CaptureSkinning, CaptureAlpha, EnsureShaderStorage)

/// DepthCommitGate::MaxBufferProofs; material_binding::LogicalBytes.
pub const MAX_BUFFER_PROOFS: usize = 256;
pub const MATERIAL_BYTES: u32 = 292;
/// A planned shader source: a buffer range (bones), the material's b4 (its logical bytes) or a texture mip's
/// footprint, at its offset in the snapshot.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Planned {
    /// 0 buffer, 1 material, 2 texture.
    pub kind: u32,
    pub mip: u32,
    pub offset: u32,
    pub bytes: u32,
    pub source_offset: u64,
    pub layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    pub row_bytes: u32,
    pub rows: u32,
}
/// A shader capture's plan: its sources (in the C++ half's room, one per range or mip and one more), the snapshot's
/// new end and the counted bytes.
#[repr(C)]
pub struct Plan {
    pub sources: *mut Planned,
    pub capacity: usize,
    pub count: usize,
    pub end: u64,
    /// The bones' bytes (skinning) or the material's (alpha).
    pub buffer_bytes: u64,
    pub texture_bytes: u64,
}
impl Plan {
    fn push(&mut self, planned: Planned) -> bool {
        if self.count >= self.capacity {
            return false;
        }
        // SAFETY: the C++ half's room for `capacity` sources.
        unsafe { *self.sources.add(self.count) = planned };
        self.count += 1;
        true
    }
    fn planned(&self) -> &[Planned] {
        // SAFETY: as above, the first `count` written.
        if self.count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(self.sources, self.count) } }
    }
}
fn footprint(device: &ID3D12Device, desc: &D3D12_RESOURCE_DESC, mip: u32, offset: u64) -> (D3D12_PLACED_SUBRESOURCE_FOOTPRINT, u32, u64, u64) {
    let (mut layout, mut rows, mut row_bytes, mut total) = (D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(), 0u32, 0u64, 0u64);
    // SAFETY: a live device; the outputs are locals.
    unsafe { device.GetCopyableFootprints(desc, mip, 1, offset, Some(&mut layout), Some(&mut rows), Some(&mut row_bytes), Some(&mut total)) };
    (layout, rows, row_bytes, total)
}
fn span(layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT, rows: u32, row_bytes: u64) -> u64 {
    layout.Footprint.RowPitch as u64 * rows.saturating_sub(1) as u64 + ((row_bytes + 3) & !3)
}
fn capturing(s: &State, list: *mut c_void, main_list: *mut c_void) -> bool {
    s.native_counts[0] == 2 && main_list == list && !s.main_submitted && !s.native_finished[0] && !s.failed && s.shader_ready
}
/// CaptureSkinning's plan: the bone ranges (each 512-aligned) then the R8 texture's whole footprint.
///
/// # Safety
/// The device and resources are live; `ranges` holds the bone ranges as (offset, bytes) pairs.
#[allow(clippy::too_many_arguments)]
pub unsafe fn skinning_plan(s: &State, device: &ID3D12Device, list: *mut c_void, main_list: *mut c_void, bones: *mut c_void, ranges: &[[u64; 2]], texture: *mut c_void, sources: usize, out: &mut Plan) -> bool {
    if list.is_null() || bones.is_null() || texture.is_null() || ranges.is_empty() || !capturing(s, list, main_list) || s.alpha_pending_sequence != 0 {
        return false;
    }
    if ranges.len() >= MAX_BUFFER_PROOFS || sources + ranges.len() + 1 + s.alpha_captures as usize > MAX_BUFFER_PROOFS - 1 || ranges.len() + 1 > out.capacity {
        return false;
    }
    let (Some(bone), Some(image)) = (borrowed::<ID3D12Resource>(bones), borrowed::<ID3D12Resource>(texture)) else { return false };
    // SAFETY: live resources.
    let desc = unsafe { bone.GetDesc() };
    let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
    // SAFETY: as above.
    let placed = unsafe { bone.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok();
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0
        || device_of(bones) != device.as_raw() as u64
        || !placed
        || (heap.Type != D3D12_HEAP_TYPE_UPLOAD && heap.Type != D3D12_HEAP_TYPE_GPU_UPLOAD)
    {
        return false;
    }
    let (mut bytes, mut bone_bytes, max) = (s.shader_bytes as u64, 0u64, MAX_INSTANCE_BYTES as u64);
    out.count = 0;
    for &[offset, length] in ranges {
        bytes = (bytes + 511) & !511;
        if length == 0 || offset % 4 != 0 || length % 4 != 0 || offset > desc.Width || length > desc.Width - offset || bytes > max || length > max - bytes {
            return false;
        }
        out.push(Planned { kind: 0, offset: bytes as u32, bytes: length as u32, source_offset: offset, ..Planned::default() });
        bytes += length;
        bone_bytes += length;
    }
    // SAFETY: a live texture.
    let td = unsafe { image.GetDesc() };
    if td.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || td.Format != DXGI_FORMAT_R8_UNORM || td.MipLevels != 1 || td.DepthOrArraySize != 1 || td.SampleDesc.Count != 1 {
        return false;
    }
    let (layout, rows, row_bytes, total) = footprint(device, &td, 0, (bytes + 511) & !511);
    let extent = span(&layout, rows, row_bytes);
    if total == u64::MAX || rows != td.Height || row_bytes != td.Width || rows == 0 || row_bytes == 0 || layout.Offset > max || extent > max - layout.Offset {
        return false;
    }
    out.push(Planned { kind: 2, mip: 0, offset: layout.Offset as u32, bytes: extent as u32, source_offset: 0, layout, row_bytes: row_bytes as u32, rows });
    out.end = layout.Offset + extent;
    out.buffer_bytes = bone_bytes;
    out.texture_bytes = row_bytes * rows as u64;
    true
}
/// CaptureAlpha's plan (0, else the refusal's statistic): the material's b4, then each consecutive BC1/BC7 mip.
///
/// # Safety
/// The device and texture are live.
#[allow(clippy::too_many_arguments)]
pub unsafe fn alpha_plan(s: &State, device: &ID3D12Device, list: *mut c_void, main_list: *mut c_void, material: bool, texture: *mut c_void, mips: &[u32], expected_index: u32, sequence: u32, sources: usize, out: &mut Plan) -> u64 {
    if list.is_null()
        || !material
        || texture.is_null()
        || mips.is_empty()
        || expected_index >= 32768
        || sequence == 0
        || !capturing(s, list, main_list)
        || s.alpha_pending_sequence != 0
        || (s.skin_pending_sequence != 0 && s.skin_pending_sequence != sequence)
    {
        return 1;
    }
    if mips.len() > 15 || sources + mips.len() + 2 + s.alpha_captures as usize > MAX_BUFFER_PROOFS - 1 || mips.len() + 1 > out.capacity {
        return 2;
    }
    let Some(image) = borrowed::<ID3D12Resource>(texture) else { return 3 };
    // SAFETY: a live texture.
    let desc = unsafe { image.GetDesc() };
    if device_of(texture) != device.as_raw() as u64
        || desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.DepthOrArraySize != 1
        || desc.SampleDesc.Count != 1
        || desc.SampleDesc.Quality != 0
        || desc.Flags.0 != 0
        || (desc.Format != DXGI_FORMAT_BC1_UNORM_SRGB && desc.Format != DXGI_FORMAT_BC7_UNORM_SRGB)
    {
        return 3;
    }
    let max = MAX_INSTANCE_BYTES as u64;
    let (mut bytes, mut texture_bytes) = ((s.shader_bytes as u64 + 511) & !511, 0u64);
    if bytes > max || MATERIAL_BYTES as u64 > max - bytes {
        return 4;
    }
    out.count = 0;
    out.push(Planned { kind: 1, offset: bytes as u32, bytes: MATERIAL_BYTES, ..Planned::default() });
    bytes += MATERIAL_BYTES as u64;
    for (i, &mip) in mips.iter().enumerate() {
        if mip >= desc.MipLevels as u32 || (i != 0 && mip != mips[i - 1].wrapping_add(1)) {
            return 4;
        }
        let (layout, rows, row_bytes, total) = footprint(device, &desc, mip, (bytes + 511) & !511);
        let extent = span(&layout, rows, row_bytes);
        if total == u64::MAX || rows == 0 || row_bytes == 0 || row_bytes > u32::MAX as u64 || rows > 65535 || layout.Offset > max || extent > max - layout.Offset {
            return 4;
        }
        out.push(Planned { kind: 2, mip, offset: layout.Offset as u32, bytes: extent as u32, source_offset: 0, layout, row_bytes: row_bytes as u32, rows });
        bytes = layout.Offset + extent;
        texture_bytes += row_bytes * rows as u64;
    }
    out.end = bytes;
    out.buffer_bytes = MATERIAL_BYTES as u64;
    out.texture_bytes = texture_bytes;
    0
}
/// The shader snapshots' storage (in and out): the two snapshot buffers and the two mapped UPLOAD permits.
#[repr(C)]
pub struct Shaders {
    pub snapshots: [*mut c_void; 2],
    pub permits: [*mut c_void; 2],
    pub mappings: [*mut c_void; 2],
}
/// EnsureShaderStorage (new objects come out with their creation's reference, the permits mapped and zeroed).
pub fn ensure_shader_storage(device: &ID3D12Device, o: &mut Shaders) -> bool {
    if o.snapshots[0].is_null() {
        let mut made = [core::ptr::null_mut(); 2];
        for item in &mut made {
            match committed(device, D3D12_HEAP_TYPE_DEFAULT, &buffer(MAX_INSTANCE_BYTES as u64), D3D12_RESOURCE_STATE_COPY_DEST) {
                Ok(r) => *item = r.into_raw(),
                Err(_) => {
                    for p in made.iter().filter(|p| !p.is_null()) {
                        // SAFETY: the reference this creation took.
                        drop(unsafe { ID3D12Resource::from_raw(*p) });
                    }
                    return false;
                }
            }
        }
        o.snapshots = made;
    }
    for i in 0..2 {
        if !o.permits[i].is_null() {
            continue;
        }
        let Ok(permit) = committed(device, D3D12_HEAP_TYPE_UPLOAD, &buffer(8), D3D12_RESOURCE_STATE_GENERIC_READ) else { return false };
        let none = D3D12_RANGE::default();
        let mut mapped = core::ptr::null_mut();
        // SAFETY: a new UPLOAD buffer, kept mapped.
        unsafe {
            if permit.Map(0, Some(&none), Some(&mut mapped)).is_err() {
                return false;
            }
            if mapped.is_null() {
                permit.Unmap(0, Some(&none));
                return false;
            }
            core::ptr::write_volatile(mapped as *mut u64, 0);
            core::arch::x86_64::_mm_sfence();
        }
        o.permits[i] = permit.into_raw();
        o.mappings[i] = mapped;
    }
    true
}
/// The C++ half's copy of the planned source `index` (a material snapshot or a texture's) into the main snapshot: 0,
/// else its failure (the material's or the texture read's).
pub type CopySource = unsafe extern "C-unwind" fn(*mut c_void, usize, *mut c_void, *mut c_void) -> u32;
/// A shader capture's recording after its plan (0, else its refusal: 5 the storage, 6 the expected index constant, 7
/// the command resources, 1000+/2000+ a material's / texture's copy failure): the storage, the alpha's expected index
/// constant (out, with its reference), the command resources, the snapshots made writable, the predicated copies.
///
/// # Safety
/// The objects are live; the callbacks are the C++ half's; `keep` lists the capture's own resources.
#[allow(clippy::too_many_arguments)]
pub unsafe fn shader_record(
    s: &mut State,
    device: &ID3D12Device,
    list: *mut c_void,
    o: &mut Shaders,
    plan: &Plan,
    expected_index: Option<u32>,
    constant: &mut *mut c_void,
    keep_objects: &[*mut c_void],
    host: &Host,
    copy: CopySource,
    user: *mut c_void,
    buffer_source: *mut c_void,
) -> u64 {
    let Some(recording) = borrowed::<ID3D12GraphicsCommandList>(list) else { return 7 };
    if !ensure_shader_storage(device, o) {
        return 5;
    }
    let mut objects: Vec<*mut c_void> = vec![host.user];
    objects.extend_from_slice(keep_objects);
    if let Some(index) = expected_index {
        let Ok(made) = committed(device, D3D12_HEAP_TYPE_UPLOAD, &buffer(16), D3D12_RESOURCE_STATE_GENERIC_READ) else { return 6 };
        let none = D3D12_RANGE::default();
        let mut mapped = core::ptr::null_mut();
        // SAFETY: a new UPLOAD buffer.
        unsafe {
            if made.Map(0, Some(&none), Some(&mut mapped)).is_err() {
                return 6;
            }
            if mapped.is_null() {
                made.Unmap(0, Some(&none));
                return 6;
            }
            core::ptr::write_unaligned(mapped as *mut [u32; 4], [index, 0, 0, 0]);
            made.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 16 }));
            core::arch::x86_64::_mm_sfence();
        }
        objects.push(made.as_raw());
        *constant = made.into_raw();
    }
    // SAFETY: the C++ half's command resources (the objects as the C++ capture listed them).
    if !unsafe { (host.keep)(host.user, list, objects.as_ptr(), objects.len()) } {
        return 7;
    }
    // SAFETY: the list recording on the pass's snapshots.
    unsafe {
        if s.shaders_readable {
            recording.ResourceBarrier(&[
                transition(o.snapshots[0], D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
                transition(o.snapshots[1], D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
            ]);
            s.shaders_readable = false;
        }
        let permit = borrowed::<ID3D12Resource>(o.permits[0]);
        recording.SetPredication(permit.as_ref(), 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
        let (Some(snapshot), source) = (borrowed::<ID3D12Resource>(o.snapshots[0]), borrowed::<ID3D12Resource>(buffer_source)) else { return 7 };
        let mut failure = 0u64;
        for (index, planned) in plan.planned().iter().enumerate() {
            if planned.kind == 0 {
                if let Some(bones) = &source {
                    recording.CopyBufferRegion(&snapshot, planned.offset as u64, bones, planned.source_offset, planned.bytes as u64);
                }
                continue;
            }
            let failed = copy(user, index, list, o.snapshots[0]);
            if failed != 0 {
                failure = failed as u64;
                break;
            }
        }
        recording.SetPredication(None, 0, D3D12_PREDICATION_OP_EQUAL_ZERO);
        failure
    }
}

/// # Safety
/// As for skinning_plan; `ranges` holds `count` (offset, bytes) pairs.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_skinning_plan(state: *const State, device: *mut c_void, list: *mut c_void, main_list: *mut c_void, bones: *mut c_void, ranges: *const [u64; 2], count: usize, texture: *mut c_void, sources: usize, out: *mut Plan) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    // SAFETY: as documented.
    unsafe {
        let ranges = if count == 0 { &[][..] } else { core::slice::from_raw_parts(ranges, count) };
        skinning_plan(&*state, &device, list, main_list, bones, ranges, texture, sources, &mut *out)
    }
}
/// # Safety
/// As for alpha_plan; `mips` holds `count` mips.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_alpha_plan(state: *const State, device: *mut c_void, list: *mut c_void, main_list: *mut c_void, material: bool, texture: *mut c_void, mips: *const u32, count: usize, expected_index: u32, sequence: u32, sources: usize, out: *mut Plan) -> u64 {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return 3 };
    // SAFETY: as documented.
    unsafe {
        let mips = if count == 0 { &[][..] } else { core::slice::from_raw_parts(mips, count) };
        alpha_plan(&*state, &device, list, main_list, material, texture, mips, expected_index, sequence, sources, &mut *out)
    }
}
/// # Safety
/// `device` is live; `objects` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_edp_ensure_shader_storage(device: *mut c_void, objects: *mut Shaders) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    // SAFETY: as documented.
    ensure_shader_storage(&device, unsafe { &mut *objects })
}
/// # Safety
/// As for shader_record; `keep` holds `keep_count` objects; `expected_index` is used when `alpha`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_edp_shader_record(
    state: *mut State,
    device: *mut c_void,
    list: *mut c_void,
    objects: *mut Shaders,
    plan: *const Plan,
    alpha: bool,
    expected_index: u32,
    constant: *mut *mut c_void,
    keep: *const *mut c_void,
    keep_count: usize,
    host: *const Host,
    copy: CopySource,
    user: *mut c_void,
    buffer_source: *mut c_void,
) -> u64 {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return 5 };
    // SAFETY: as documented.
    unsafe {
        let keep = if keep_count == 0 { &[][..] } else { core::slice::from_raw_parts(keep, keep_count) };
        shader_record(&mut *state, &device, list, &mut *objects, &*plan, alpha.then_some(expected_index), &mut *constant, keep, &*host, copy, user, buffer_source)
    }
}

#[cfg(test)]
mod tests {
    //! The pass's state rules: the defaults, the readable states, a reset's retirement, Restart's state; native observation,
    //! the commit and the peer runs' refusals.
    use super::*;
    use std::cell::RefCell;
    thread_local! {
        static GATE: RefCell<Vec<(u32, bool)>> = const { RefCell::new(Vec::new()) };
    }
    unsafe extern "C-unwind" fn keep(_: *mut c_void, _: *mut c_void, _: *const *mut c_void, _: usize) -> bool {
        true
    }
    unsafe extern "C-unwind" fn has(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> bool {
        true
    }
    unsafe extern "C-unwind" fn arrays(_: *mut c_void, _: u32, _: u32, _: *mut c_void, _: u32, _: *mut c_void) -> HRESULT {
        S_OK
    }
    unsafe extern "C-unwind" fn gate_op(_: *mut c_void, op: u32, _: *mut c_void, flag: bool) -> bool {
        GATE.with(|g| g.borrow_mut().push((op, flag)));
        true
    }
    unsafe extern "C-unwind" fn sources(_: *mut c_void, _: usize, _: *mut c_void, _: u32) -> u32 {
        0
    }
    fn host() -> Host {
        Host { user: core::ptr::null_mut(), keep, has, target: arrays, gate: gate_op, shader_source: sources, shader_sources: 0 }
    }
    fn objects(main: *mut c_void, peer: *mut c_void) -> Objects {
        Objects {
            main_list: main,
            peer_list: peer,
            native: [0x100 as *mut c_void, 0x200 as *mut c_void],
            dsvs: core::ptr::null_mut(),
            timestamps: core::ptr::null_mut(),
            timing: [core::ptr::null_mut(); 2],
            readback: core::ptr::null_mut(),
            instance_source: core::ptr::null_mut(),
            tokens: [core::ptr::null_mut(); 2],
            permits: [core::ptr::null_mut(); 2],
            main_queue: core::ptr::null_mut(),
            main_fence: core::ptr::null_mut(),
        }
    }
    fn record(side: u32, depth: u64, instances: u32) -> DrawRecord {
        // SAFETY: a plain record, all zero.
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.side = side;
        d.depth = depth;
        d.indices = 3;
        d.instances = instances;
        d
    }
    fn admission() -> Admission {
        Admission {
            masks: core::ptr::null(),
            mask_count: 0,
            peer_only: 0,
            bank_draw: 0,
            bank_draws: 1,
            bank_snapshots: 0,
            alpha_depth_contract: 0,
            skinning_read_contract: 0,
            native_camera_stages: 0,
            render_targets: 0,
            depth_format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0,
            depth_func: D3D12_COMPARISON_FUNC_GREATER_EQUAL.0,
            depth_write: D3D12_DEPTH_WRITE_MASK_ALL.0,
            samples_count: 1,
            samples_quality: 0,
            source_root: 0,
            native_camera: false,
            alpha_reference: false,
            alpha_optimized: false,
            family_pipeline: true,
            ordinal_independent: true,
            native_camera_pipeline: false,
            pixel_shader: false,
            fixed_raster: true,
            depth_known: true,
            depth_enable: true,
        }
    }
    #[test]
    fn main_admission() {
        let list = 0x10 as *mut c_void;
        let mut s = State::default();
        s.result.width = 64;
        s.result.height = 32;
        let mut d = record(0, 7, 4);
        d.viewport = D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: 64.0, Height: 32.0, MinDepth: 0.0, MaxDepth: 1.0 };
        let a = admission();
        assert!(main_admitted(&s, core::ptr::null_mut(), false, list, &d, &a), "a depth-only main draw over the whole target");
        assert!(!main_admitted(&s, 0x20 as *mut c_void, false, list, &d, &a), "another main list");
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &d, &Admission { samples_count: 4, ..a }), "multisampled");
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &d, &Admission { depth_func: D3D12_COMPARISON_FUNC_LESS.0, ..a }), "a reverse depth test only");
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &d, &Admission { pixel_shader: true, ..a }), "a pixel shader without an admitted alpha mode");
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &d, &Admission { bank_draw: 1, ..a }), "a bank draw beyond the bank");
        let masks = [2u8, 3, 3, 2];
        assert!(main_admitted(&s, core::ptr::null_mut(), false, list, &d, &Admission { masks: masks.as_ptr(), mask_count: 4, ..a }));
        let bad = [2u8, 1, 3, 2];
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &d, &Admission { masks: bad.as_ptr(), mask_count: 4, ..a }), "a mask beyond both eyes");
        let mut half = d;
        half.viewport.Width = 32.0;
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &half, &a), "a partial viewport");
        s.native_counts[0] = 2;
        assert!(!main_admitted(&s, core::ptr::null_mut(), false, list, &d, &a), "a native mode needs its main target");
        s.native_dsv[0] = 7;
        assert!(main_admitted(&s, core::ptr::null_mut(), true, list, &d, &a));
    }
    #[test]
    fn native_observation_and_commit() {
        let (main, peer) = (0x10 as *mut c_void, 0x20 as *mut c_void);
        let o = objects(main, peer);
        let mut s = State { native_dsv: [7, 9], ..State::default() };
        s.native_counts[0] = 3;
        assert!(observe_native(&mut s, &o, main, &record(0, 7, 1)));
        assert!(observe_native(&mut s, &o, peer, &record(1, 9, 1)));
        assert_eq!((s.native_counts[1], s.native_counts[2], s.result.draws), (1, 1, 1), "the main draw counted in the result");
        assert!(!observe_native(&mut s, &o, peer, &record(1, 7, 1)), "another depth target");
        assert!(!observe_native(&mut s, &o, main, &record(1, 9, 1)), "the other side's list");
        s.native_finished[0] = true;
        assert!(!observe_native(&mut s, &o, main, &record(0, 7, 1)), "a finished side");
        let h = host();
        assert!(!commit_native(&mut s, true, &h), "only the optimized mode commits");
        s.native_counts[0] = 2;
        GATE.with(|g| g.borrow_mut().clear());
        assert!(commit_native(&mut s, true, &h) && s.optimized[7] == 1);
        reject_shader_snapshots(&mut s, &h);
        assert!(!s.shader_ready && s.optimized[7] == 0);
        assert!(commit_native(&mut s, true, &h) && s.optimized[7] == 0, "rejected snapshots never commit");
        assert_eq!(GATE.with(|g| g.borrow().clone()), vec![(1, true), (1, false), (1, false)]);
    }
    #[test]
    fn peer_runs() {
        let (main, peer) = (0x10 as *mut c_void, 0x20 as *mut c_void);
        let o = objects(main, peer);
        let mut s = State { native_dsv: [7, 9], main_submitted: true, ..State::default() };
        s.native_counts[0] = 2;
        let h = host();
        let d = record(1, 9, 8);
        // SAFETY: each refused before the list is used.
        unsafe {
            assert!(!peer_remainder(&mut s, &o, peer, &d, &[Run { first: 0, count: 0 }], &h), "an empty run");
            assert!(!peer_remainder(&mut s, &o, peer, &d, &[Run { first: 4, count: 2 }, Run { first: 3, count: 1 }], &h), "runs out of order");
            assert!(!peer_remainder(&mut s, &o, peer, &d, &[Run { first: 6, count: 3 }], &h), "a run beyond the instances");
            assert!(!peer_remainder(&mut s, &o, main, &d, &[Run { first: 0, count: 8 }], &h), "the main list");
            s.main_submitted = false;
            assert!(!peer_remainder(&mut s, &o, peer, &d, &[Run { first: 0, count: 8 }], &h), "before the main submission");
        }
    }

    #[test]
    fn state_rules() {
        let s = State::default();
        assert!(s.shader_ready && s.serial == 1 && !s.failed);
        let vb = D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER.0;
        assert!(readable(vb) && readable(vb | D3D12_RESOURCE_STATE_COPY_SOURCE.0));
        assert!(!readable(D3D12_RESOURCE_STATE_COPY_SOURCE.0), "a vertex read state is required");
        assert!(!readable(vb | D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0), "and only read states");
        let (main, peer, other) = (0x10 as *mut c_void, 0x20 as *mut c_void, 0x30 as *mut c_void);
        let mut s = State { main_submitted: true, ..State::default() };
        reset(&mut s, other, main, peer, [true, true]);
        assert!(s.recording_retired == [false, false]);
        reset(&mut s, main, main, peer, [true, true]);
        assert!(s.recording_retired == [true, false] && !s.failed, "a submitted main recording retires");
        reset(&mut s, peer, main, peer, [true, false]);
        assert!(s.recording_retired == [true, false], "no token: nothing recorded");
        reset(&mut s, peer, main, peer, [true, true]);
        assert!(s.recording_retired == [true, true] && s.failed, "an unsubmitted recording reset fails the pass");
    }
    #[test]
    fn restart_state() {
        let mut s = State { failed: false, serial: 7, instance_bytes: 64, shader_ready: false, alpha_captures: 2, ..State::default() };
        s.native_counts = [2, 5, 6, 0, 0, 0, 0, 1];
        s.result = Comparison { width: 8, height: 4, draws: 3, complete: 1, ..Comparison::default() };
        assert!(restart_ready(&s));
        s.instance_augmented = true;
        assert!(!restart_ready(&s), "an augmented read must be released first");
        s.instance_augmented = false;
        let mut permit = [0xFFu64; 1];
        // SAFETY: a local word as the permit.
        unsafe { restart(&mut s, [permit.as_mut_ptr() as *mut c_void, core::ptr::null_mut()]) };
        assert_eq!(permit[0], 0, "the permit closed");
        assert_eq!((s.serial, s.instance_bytes, s.alpha_captures, s.shader_ready), (8, 0, 0, true));
        assert_eq!(s.native_counts, [2, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(s.optimized, [1, u64::MAX, 0, 0, 0, 0, 0, 0]);
        assert_eq!((s.result.width, s.result.height, s.result.draws, s.result.complete), (8, 4, 0, 0), "the result but its size cleared");
        s.native_counts[0] = 1;
        assert!(!restart_ready(&s), "only the optimized mode restarts");
    }
    fn warp() -> ID3D12Device {
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn committed(device: &ID3D12Device, heap: D3D12_HEAP_TYPE, desc: &D3D12_RESOURCE_DESC, state: D3D12_RESOURCE_STATES) -> ID3D12Resource {
        let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
        let mut resource: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, desc, state, None, &mut resource) }.unwrap();
        resource.unwrap()
    }
    fn buffer_desc(width: u64) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: width,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        }
    }
    fn texture_desc(format: DXGI_FORMAT, width: u64, height: u32, mips: u16) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: mips,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        }
    }
    #[test]
    fn shader_plans() {
        let device = warp();
        let bones = committed(&device, D3D12_HEAP_TYPE_UPLOAD, &buffer_desc(1024), D3D12_RESOURCE_STATE_GENERIC_READ);
        let fixed = committed(&device, D3D12_HEAP_TYPE_DEFAULT, &buffer_desc(1024), D3D12_RESOURCE_STATE_COMMON);
        let mask = committed(&device, D3D12_HEAP_TYPE_DEFAULT, &texture_desc(DXGI_FORMAT_R8_UNORM, 16, 8, 1), D3D12_RESOURCE_STATE_COMMON);
        let bc1 = committed(&device, D3D12_HEAP_TYPE_DEFAULT, &texture_desc(DXGI_FORMAT_BC1_UNORM_SRGB, 16, 16, 3), D3D12_RESOURCE_STATE_COMMON);
        let list = 0x10 as *mut c_void;
        let mut s = State { shader_ready: true, ..State::default() };
        s.native_counts[0] = 2;
        let mut room = [Planned::default(); 4];
        let mut plan = Plan { sources: room.as_mut_ptr(), capacity: room.len(), count: 0, end: 0, buffer_bytes: 0, texture_bytes: 0 };
        let skin = |s: &State, plan: &mut Plan, list_: *mut c_void, bones_: &ID3D12Resource, ranges: &[[u64; 2]]| unsafe {
            skinning_plan(s, &device, list_, list, bones_.as_raw(), ranges, mask.as_raw(), 0, plan)
        };
        assert!(skin(&s, &mut plan, list, &bones, &[[0, 64], [256, 100]]), "two bone ranges and the mask");
        assert_eq!((plan.count, plan.end, plan.buffer_bytes, plan.texture_bytes), (3, 1024 + 256 * 7 + 16, 164, 128));
        let p = plan.planned();
        assert_eq!((p[0].kind, p[0].offset, p[0].bytes, p[0].source_offset), (0, 0, 64, 0));
        assert_eq!((p[1].kind, p[1].offset, p[1].bytes, p[1].source_offset), (0, 512, 100, 256), "each range 512-aligned");
        assert_eq!((p[2].kind, p[2].offset, p[2].rows, p[2].row_bytes, p[2].layout.Footprint.RowPitch), (2, 1024, 8, 16, 256));
        assert!(!skin(&s, &mut plan, list, &bones, &[[2, 64]]), "an unaligned range");
        assert!(!skin(&s, &mut plan, list, &bones, &[[960, 128]]), "a range beyond the bones");
        assert!(!skin(&s, &mut plan, list, &fixed, &[[0, 64]]), "bones outside an upload heap");
        assert!(!skin(&s, &mut plan, 0x20 as *mut c_void, &bones, &[[0, 64]]), "another list than the main one");
        assert!(!skin(&s, &mut plan, list, &bones, &[[0, 4]; 4]), "more ranges than the room");
        let alpha = |s: &State, plan: &mut Plan, texture: &ID3D12Resource, mips: &[u32], sequence: u32| unsafe {
            alpha_plan(s, &device, list, list, true, texture.as_raw(), mips, 3, sequence, 0, plan)
        };
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0, 1], 1), 0, "the material and two BC1 mips");
        assert_eq!((plan.count, plan.end, plan.buffer_bytes, plan.texture_bytes), (3, 1536 + 256 + 16, MATERIAL_BYTES as u64, 32 * 4 + 16 * 2));
        let p = plan.planned();
        assert_eq!((p[0].kind, p[0].offset, p[0].bytes), (1, 0, MATERIAL_BYTES));
        assert_eq!((p[1].kind, p[1].mip, p[1].offset, p[1].bytes, p[1].rows, p[1].row_bytes), (2, 0, 512, 256 * 3 + 32, 4, 32));
        assert_eq!((p[2].kind, p[2].mip, p[2].offset, p[2].bytes, p[2].rows, p[2].row_bytes), (2, 1, 1536, 256 + 16, 2, 16));
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0, 2], 1), 4, "mips not consecutive");
        assert_eq!(alpha(&s, &mut plan, &bc1, &[2, 3], 1), 4, "a mip beyond the texture");
        assert_eq!(alpha(&s, &mut plan, &mask, &[0], 1), 3, "neither BC1 nor BC7");
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0], 0), 1, "no sequence");
        s.skin_pending_sequence = 5;
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0], 1), 1, "another sequence's skinning pending");
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0], 5), 0, "the skinning's own sequence");
        s.shader_bytes = MAX_INSTANCE_BYTES;
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0], 5), 4, "no room left in the snapshot");
        s.shader_bytes = 0;
        s.alpha_pending_sequence = 5;
        assert_eq!(alpha(&s, &mut plan, &bc1, &[0], 5), 1, "an alpha capture pending");
        assert!(!skin(&s, &mut plan, list, &bones, &[[0, 64]]), "skinning after a pending alpha capture");
    }
}
