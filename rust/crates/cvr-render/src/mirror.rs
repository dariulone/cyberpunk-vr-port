//! THE DESKTOP MIRROR (formerly src/Stereo/Mirror.cpp's two functions): the window that shows what the second eye
//! gets, driven by a thread of its own -- its own queue, D3D12 swapchain and window with a message pump, so DWM keeps
//! drawing it and presenting never costs the headset a frame. The game side does exactly one thing for it: when the
//! game submits, a small list of ours (the copy into the mirror texture, or the HUD composite over it, plus the
//! readback probes riding on the same fence) is executed on the game's queue, and the thread GPU-waits that fence and
//! presents through a format-converting pass (the engine's own SDR swapchain arithmetic). No CPU stall crosses over.
//!
//! The objects stay with their other readers (Mirror.cpp, SyncStereo.cpp, LumaProbe.cpp); Mirror.cpp forwards
//! d12_submit_mirror_copy here.
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::mem::{transmute, transmute_copy, ManuallyDrop};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::sync::LazyLock;
use windows::core::{s, w, Interface, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_SHADER_MACRO,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::{CreateEventW, Sleep, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::capture_stable::{cvr_capture_d12_append_mirror_copy, cvr_capture_d12_mirror_ensure};
use crate::hud::Barrier;
use crate::luma_probe::{
    cvr_cb_probe_collect, cvr_cb_probe_ensure, cvr_luma_probe_collect, cvr_luma_probe_ensure,
    cvr_ti_probe_collect, cvr_ti_probe_ensure, cvr_vision_dump_write,
};
use crate::single_pass::Hook;
use cvr_gpu::color_blit::{
    cvr_gpu_color_blit_ensure, cvr_gpu_color_blit_record_dot,
    cvr_gpu_color_blit_record_hud_composite, cvr_gpu_color_blit_record_overlay, Blit, HUD_PARAMS,
};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_MirrorOutput: AtomicU32;
    static CyberpunkVR_VrcamOwnTarget: AtomicI32;
    static CyberpunkVR_StableCopy: AtomicI32;
    static CyberpunkVR_LumaProbe: AtomicI32;
    static CyberpunkVR_VisionDump: AtomicI32;
    static CyberpunkVR_DebugMirrorSrcState: AtomicU32;
    static CyberpunkVR_NoStateLies: AtomicI32;
    static CyberpunkVR_DebugForeignStateRefusals: AtomicU64;
    static CyberpunkVR_VisionToSecondEye: AtomicI32;
    static CyberpunkVR_VisionDebug: AtomicI32;
    static CyberpunkVR_VisionFit: AtomicI32;
    static CyberpunkVR_VisionOffX: AtomicU32;
    static CyberpunkVR_VisionOffY: AtomicU32;
    static CyberpunkVR_DebugVisionOverlays: AtomicU64;
    static CyberpunkVR_BarrelDotSecondEye: AtomicI32;
    static CyberpunkVR_BarrelDotTick: AtomicU64;
    static CyberpunkVR_BarrelDotNdcX2: AtomicU32;
    static CyberpunkVR_BarrelDotNdcY: AtomicU32;
    static CyberpunkVR_BarrelDotRadiusPx: AtomicU32;
    static CyberpunkVR_DebugBarrelDotDraws: AtomicU64;
    static CyberpunkVR_DebugFinNatural: AtomicU32;
    static CyberpunkVR_DebugMirrorReadyFence: AtomicU64;
    static CyberpunkVR_DebugMirrorPendingHits: AtomicU64;
    static CyberpunkVR_MirrorTestPattern: AtomicU32;
    static CyberpunkVR_DebugMirrorState: AtomicU32;
    static CyberpunkVR_DebugMirrorLastHr: AtomicU32;
    static CyberpunkVR_DebugMirrorFrames: AtomicU64;
    static g_game_device: *mut c_void;
    static g_luma_rb: [*mut c_void; 4];
    static g_luma_map: [*mut u8; 4];
    static mut g_luma_parity: [u32; 4];
    static mut g_luma_frame: [u32; 4];
    static mut g_luma_finidx: [u32; 4];
    static g_cb_rb: [*mut c_void; 4];
    static g_cb_map: [*mut u8; 4];
    static g_cb_reset_pending: AtomicBool;
    static g_ti_rb: [*mut c_void; 4];
    static g_ti_map: [*mut u8; 4];
    static mut g_ti_count: [u32; 4];
    static mut g_ti_src: [[*mut c_void; 24]; 4];
    static mut g_ti_fmt: [[u32; 24]; 4];
    static mut g_ti_tag: [[u32; 24]; 4];
    static mut g_visdump_map: *mut c_void;
    static mut g_visdump_w: u32;
    static mut g_visdump_h: u32;
    static mut g_visdump_pitch: u32;
    fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const Hook;
    fn CyberpunkVR_GetHudTexture() -> *mut c_void;
    fn CyberpunkVR_GetHudBlurTexture() -> *mut c_void;
    fn CyberpunkVR_GetHudExposureBuffer() -> *mut c_void;
    fn CyberpunkVR_GetFrameConstantBuffer() -> *mut c_void;
    fn CyberpunkVR_GetHudConstantBuffer() -> *mut c_void;
    fn CyberpunkVR_GetVisionTexture() -> *mut c_void;
    fn CyberpunkVR_NoteHudCompositeInputs(
        hud: *const c_void,
        blur: *const c_void,
        expo: *const c_void,
        frame_cb: *const c_void,
        hud_cb: *const c_void,
    );
}


#[repr(C)]
pub struct Runtime {
    get_desc: extern "C" fn(*mut c_void, *mut D3D12_RESOURCE_DESC) -> bool,
    own_target: *const *mut c_void,
    stable_tex: *const *mut c_void,
    stable_lock: extern "C" fn(bool),
    stable_fresh: *const AtomicBool,
    captured: *const AtomicPtr<c_void>,
    /// g_d12_copy_mtx: try (true, answers whether it was taken) or release (false).
    copy_lock: extern "C" fn(bool) -> bool,
    d12_mtex: *const *mut c_void,
    d12_mtex_is_rt: *const bool,
    d12_fence: *const *mut c_void,
    d12_w: *const u32,
    d12_h: *const u32,
    d12_fmt: *const DXGI_FORMAT,
    /// The game-side copy list, its four allocators, each slot's fence value and the frame counter (SyncStereo.cpp's).
    copy_list: *mut *mut c_void,
    copy_alloc: *mut *mut c_void,
    copy_slot_fence: *mut u64,
    copy_frame: *mut u32,
    luma_valid: *mut bool,
    cb_valid: *mut bool,
    ti_valid: *mut bool,
    visdump_slot: *mut i32,
    visdump_rb: *mut *mut c_void,
    cb_res: *const AtomicPtr<c_void>,
    cb_last_res: *const AtomicPtr<c_void>,
    cb_off: *const AtomicU64,
    expo_vrcam: *const AtomicPtr<c_void>,
    expo_main: *const AtomicPtr<c_void>,
    mirror_pending_list: *const AtomicPtr<c_void>,
    /// g_orig_CreateRTV (0 before the device hook is in).
    orig_create_rtv: *const usize,
    /// command_queue_original_execute(queue): the queue's own ExecuteCommandLists, past our hook.
    queue_original_execute: extern "C" fn(*mut c_void) -> usize,
    /// hud_composite_params() into the blit's parameter block.
    hud_params: extern "C" fn(*mut [f32; HUD_PARAMS]),
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    get_desc: s::get_desc,
    own_target: &raw const s::g_own_target,
    stable_tex: &raw const s::g_stable_tex,
    stable_lock: s::stable_lock,
    stable_fresh: &raw const m::g_stable_fresh,
    captured: &raw const s::g_captured_vrcam_res,
    copy_lock: s::copy_lock,
    d12_mtex: &raw const s::g_d12_mtex,
    d12_mtex_is_rt: &raw const s::g_d12_mtex_is_rt,
    d12_fence: &raw const s::g_d12_fence,
    d12_w: &raw const s::g_d12_w,
    d12_h: &raw const s::g_d12_h,
    d12_fmt: &raw const s::g_d12_fmt,
    copy_list: &raw mut s::g_d12_copy_list,
    copy_alloc: (&raw mut s::g_d12_copy_alloc).cast(),
    copy_slot_fence: (&raw const m::g_d12_copy_slot_fence).cast_mut().cast(),
    copy_frame: (&raw const m::g_d12_copy_frame).cast_mut().cast(),
    luma_valid: (&raw const m::g_luma_valid).cast_mut().cast(),
    cb_valid: (&raw const m::g_cb_valid).cast_mut().cast(),
    ti_valid: (&raw const m::g_ti_valid).cast_mut().cast(),
    visdump_slot: &raw mut s::g_visdump_slot,
    visdump_rb: &raw mut s::g_visdump_rb,
    cb_res: &raw const s::g_cb_res,
    cb_last_res: &raw const s::g_cb_last_res,
    cb_off: &raw const m::g_cb_off,
    expo_vrcam: &raw const s::g_expo_vrcam,
    expo_main: &raw const s::g_expo_main,
    mirror_pending_list: &raw const s::g_mirror_pending_list,
    orig_create_rtv: &raw const s::g_orig_CreateRTV,
    queue_original_execute: s::queue_original_execute,
    hud_params: s::hud_params,
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn diag(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn float(bits: &AtomicU32) -> f32 {
    f32::from_bits(bits.load(Relaxed))
}
const fn state(s: D3D12_RESOURCE_STATES) -> u32 {
    s.0 as u32
}
/// A C++ std::mutex held for a scope, through its adapter.
struct Held(extern "C" fn(bool));
impl Held {
    fn take(lock: extern "C" fn(bool)) -> Self {
        lock(true);
        Held(lock)
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        (self.0)(false);
    }
}
/// g_d12_copy_mtx, taken with try_lock, released at scope end.
struct CopyHeld(extern "C" fn(bool) -> bool);
impl Drop for CopyHeld {
    fn drop(&mut self) {
        (self.0)(false);
    }
}
/// A borrowed resource pointer in the shape the D3D12 structs carry it (no reference taken or dropped).
unsafe fn res_field(p: *mut c_void) -> ManuallyDrop<Option<ID3D12Resource>> {
    transmute::<*mut c_void, ManuallyDrop<Option<ID3D12Resource>>>(p)
}
unsafe fn subresource0(res: *mut c_void) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: res_field(res),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    }
}
unsafe fn footprint(
    res: *mut c_void,
    offset: u64,
    format: DXGI_FORMAT,
    width: u32,
    height: u32,
    pitch: u32,
) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION {
        pResource: res_field(res),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: offset,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: format,
                    Width: width,
                    Height: height,
                    Depth: 1,
                    RowPitch: pitch,
                },
            },
        },
    }
}
/// The 8x8 block at the centre of a `width` x `height` surface (unsigned arithmetic, as the GPU box is).
fn centre_box(width: u64, height: u32) -> D3D12_BOX {
    let (cx, cy) = ((width / 2) as u32, height / 2);
    D3D12_BOX {
        left: cx.wrapping_sub(4),
        top: cy.wrapping_sub(4),
        front: 0,
        right: cx.wrapping_add(4),
        bottom: cy.wrapping_add(4),
        back: 1,
    }
}
unsafe fn list_barriers(list: &ID3D12GraphicsCommandList, b: &[Barrier]) {
    list.ResourceBarrier(core::slice::from_raw_parts(
        b.as_ptr().cast::<D3D12_RESOURCE_BARRIER>(),
        b.len(),
    ));
}

/// The game-side fence values: the next to signal, the last signalled (what the present thread waits on).
static FENCE_NEXT: AtomicU64 = AtomicU64::new(1);
static READY: AtomicU64 = AtomicU64::new(0);
static PRESENT_STARTED: AtomicBool = AtomicBool::new(false);
static CLOSE_FAILS: AtomicU64 = AtomicU64::new(0);

/// What each copy slot's submitted work reads (main's 8255f379b and c41b2e23d): the copy's source and the tonemap
/// inputs the stage sampling read, references of the mirror's own until the slot's fence passes. A failed Signal
/// cannot prove completion: the leases are then kept and the copies stop.
struct Leases {
    sources: [Option<ID3D12Resource>; 4],
    tm: [Vec<ID3D12Resource>; 4],
    copy_failed: bool,
}
static LEASES: std::sync::Mutex<Leases> = std::sync::Mutex::new(Leases { sources: [None, None, None, None], tm: [Vec::new(), Vec::new(), Vec::new(), Vec::new()], copy_failed: false });
fn leases() -> std::sync::MutexGuard<'static, Leases> {
    LEASES.lock().unwrap_or_else(|e| e.into_inner())
}
/// tm_capture_enabled: the tonemap input capture on with the luma probe, the mirror output and the stable copy; off,
/// its publication is cleared.
pub fn tm_capture_enabled() -> bool {
    // SAFETY: exports read whole.
    let on = unsafe {
        CyberpunkVR_LumaProbe.load(Relaxed) != 0 && CyberpunkVR_MirrorOutput.load(Relaxed) != 0 && CyberpunkVR_StableCopy.load(Relaxed) != 0
    };
    s::TM_INPUTS.set_enabled(on)
}
/// The slots the GPU is done with let go of what they read; driver destruction outside the copy lock. Polled by each
/// copy and by the present loop, even while the mirror is off or no new image arrives.
unsafe fn retire_mirror_sources() {
    tm_capture_enabled();
    let r = rt();
    let mut retired: Vec<ID3D12Resource> = Vec::new();
    {
        if !(r.copy_lock)(true) {
            return;
        }
        let _copy = CopyHeld(r.copy_lock);
        let fence_raw = *r.d12_fence;
        let Some(fence) = ID3D12Fence::from_raw_borrowed(&fence_raw) else { return };
        let mut l = leases();
        if l.copy_failed {
            return;
        }
        let completed = fence.GetCompletedValue();
        for i in 0..4 {
            if completed >= *r.copy_slot_fence.add(i) {
                retired.extend(l.sources[i].take());
                retired.append(&mut l.tm[i]);
            }
        }
    }
    drop(retired);
}
/// The fullscreen premultiplied-alpha pass for the HUD over the mirror image; it runs on our own mirror list.
static HUD_BLIT: LazyLock<Blit> = LazyLock::new(Blit::default);

/// Submitted when the game submits the list that wrote the second view's output: the copy (or the HUD composite)
/// into the mirror texture and the readback probes, on our own list, executed on the game's queue past our hook and
/// signalled on the mirror fence. Single-flight and never stalls: busy lock or in-flight slot skips the frame.
#[no_mangle]
pub unsafe extern "C" fn cvr_mirror_submit_copy(queue: *mut c_void) {
    retire_mirror_sources();
    let r = rt();
    let device = g_game_device;
    if CyberpunkVR_MirrorOutput.load(Relaxed) == 0 || queue.is_null() || device.is_null() {
        return;
    }
    let own = *r.own_target;
    let use_own = CyberpunkVR_VrcamOwnTarget.load(Relaxed) != 0 && !own.is_null();
    // The stable committed snapshot (filled inline in the valid window, never aliased, resting in COMMON) is
    // preferred over the transient whose heap MAIN re-uses.
    // the source, a reference of the copy's own until its slot's fence passes
    let mut lease: Option<ID3D12Resource> = None;
    if !use_own && CyberpunkVR_StableCopy.load(Relaxed) != 0 && (*r.stable_fresh).load(Acquire) {
        let _lock = Held::take(r.stable_lock);
        let stable = *r.stable_tex;
        lease = ID3D12Resource::from_raw_borrowed(&stable).cloned();
    }
    let use_stable = lease.is_some();
    if use_own {
        lease = ID3D12Resource::from_raw_borrowed(&own).cloned();
    } else if !use_stable {
        lease = s::MIRROR_OUTPUT.acquire();
    }
    let dtex = lease.as_ref().map_or(null_mut(), Interface::as_raw);
    if dtex.is_null() {
        return;
    }
    if !(r.copy_lock)(true) {
        return;
    }
    let _copy = CopyHeld(r.copy_lock);
    if leases().copy_failed {
        return;
    }
    let mut d = D3D12_RESOURCE_DESC::default();
    if !(r.get_desc)(dtex, &mut d) {
        return;
    }
    if !cvr_capture_d12_mirror_ensure(&d) {
        return;
    }
    let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
        return;
    };
    if (*r.copy_list).is_null() {
        for i in 0..4 {
            match dev
                .CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
            {
                Ok(a) => *r.copy_alloc.add(i) = a.into_raw(),
                Err(_) => return,
            }
        }
        let alloc0 = *r.copy_alloc;
        let Some(alloc0) = ID3D12CommandAllocator::from_raw_borrowed(&alloc0) else {
            return;
        };
        match dev.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            alloc0,
            None::<&ID3D12PipelineState>,
        ) {
            Ok(list) => {
                let _ = list.SetName(w!("CVR_mirror_copy_list"));
                let _ = list.Close();
                *r.copy_list = list.into_raw();
            }
            Err(_) => return,
        }
    }
    let list_raw = *r.copy_list;
    let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&list_raw) else {
        return;
    };
    let fence_raw = *r.d12_fence;
    let Some(fence) = ID3D12Fence::from_raw_borrowed(&fence_raw) else {
        return;
    };
    let frame = *r.copy_frame;
    let idx = (frame & 3) as usize;
    if fence.GetCompletedValue() < *r.copy_slot_fence.add(idx) {
        return; // in flight
    }
    // the slot's previous leases: done on the GPU, let go after this submission
    let (_retired_source, _retired_tm) = {
        let mut l = leases();
        (l.sources[idx].take(), core::mem::take(&mut l.tm[idx]))
    };
    // Slot idx's previous submission is GPU-complete, so its readbacks are coherent.
    let probing = CyberpunkVR_LumaProbe.load(Relaxed) != 0;
    if probing && *r.luma_valid.add(idx) && !g_luma_map[idx].is_null() {
        let l = cvr_luma_probe_collect(idx as u32);
        *r.luma_valid.add(idx) = false;
        if *r.cb_valid.add(idx) && !g_cb_map[idx].is_null() {
            cvr_cb_probe_collect(idx as u32, l);
        }
        if *r.ti_valid.add(idx) && !g_ti_map[idx].is_null() {
            cvr_ti_probe_collect(idx as u32, l);
        }
    }
    *r.cb_valid.add(idx) = false;
    *r.ti_valid.add(idx) = false;
    if *r.visdump_slot == idx as i32 && CyberpunkVR_VisionDump.load(Relaxed) == 2 {
        cvr_vision_dump_write();
        *r.visdump_slot = -1;
        CyberpunkVR_VisionDump.store(0, Relaxed);
    }
    let alloc_raw = *r.copy_alloc.add(idx);
    let Some(alloc) = ID3D12CommandAllocator::from_raw_borrowed(&alloc_raw) else {
        return;
    };
    if alloc.Reset().is_err() {
        return;
    }
    if list.Reset(alloc, None::<&ID3D12PipelineState>).is_err() {
        return;
    }
    let hook = cvr_cpp_command_list_hook_entry(list_raw).as_ref();
    let Some(hook) = hook.filter(|h| h.barrier_call != 0 && h.copyres != 0) else {
        let _ = list.Close();
        return;
    };
    let barrier_call: unsafe extern "system" fn(*mut c_void, u32, *const Barrier) =
        transmute(hook.barrier_call);
    let common = state(D3D12_RESOURCE_STATE_COMMON);
    let copy_source = state(D3D12_RESOURCE_STATE_COPY_SOURCE);
    // The state the barrier hook tracked for THIS resource, never a guess: our own target always rests in
    // RENDER_TARGET, the stable snapshot in COMMON; the engine's transient has a tracked state or no copy this frame
    // (a skipped mirror frame costs a frame, a wrong barrier costs the device).
    let mut copy_src_state = if use_own {
        state(D3D12_RESOURCE_STATE_RENDER_TARGET)
    } else {
        common
    };
    if !use_own && !use_stable {
        let tracked = CyberpunkVR_DebugMirrorSrcState.load(Relaxed);
        if tracked == 0 && CyberpunkVR_NoStateLies.load(Relaxed) != 0 {
            diag(&CyberpunkVR_DebugForeignStateRefusals);
            return;
        }
        if tracked != 0 {
            copy_src_state = tracked;
        }
    }
    // ---- the HUD on the mirror: what eye 1 gets, visible at a desk. When it runs it REPLACES the plain copy.
    let mtex = *r.d12_mtex;
    let none = null_mut();
    let hud_tex = if *r.d12_mtex_is_rt {
        CyberpunkVR_GetHudTexture()
    } else {
        none
    };
    let hud_blur = if hud_tex.is_null() {
        none
    } else {
        CyberpunkVR_GetHudBlurTexture()
    };
    let hud_expo = if hud_tex.is_null() {
        none
    } else {
        CyberpunkVR_GetHudExposureBuffer()
    };
    let frame_cb = if hud_tex.is_null() {
        none
    } else {
        CyberpunkVR_GetFrameConstantBuffer()
    };
    // Optional: when missing, the shader's own validity check rejects what is bound in its place and falls back to
    // the captured constants.
    let mut hud_cb = if hud_tex.is_null() {
        none
    } else {
        CyberpunkVR_GetHudConstantBuffer()
    };
    CyberpunkVR_NoteHudCompositeInputs(hud_tex, hud_blur, hud_expo, frame_cb, hud_cb);
    if hud_cb.is_null() {
        hud_cb = frame_cb;
    }
    let blit: *const Blit = &*HUD_BLIT;
    let mut hud_composited = false;
    if !hud_tex.is_null()
        && !hud_blur.is_null()
        && !hud_expo.is_null()
        && !frame_cb.is_null()
        && !hud_cb.is_null()
        && cvr_gpu_color_blit_ensure(blit, device, (*r.d12_fmt).0, *r.d12_w, *r.d12_h)
    {
        let render_target = state(D3D12_RESOURCE_STATE_RENDER_TARGET);
        let pixel_read = state(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
        // The scene normally rests in COMMON, which promotes implicitly; only another state needs a transition.
        let mut mb = vec![Barrier::transition(mtex, common, render_target)];
        if copy_src_state != common {
            mb.push(Barrier::transition(dtex, copy_src_state, pixel_read));
        }
        barrier_call(list_raw, mb.len() as u32, mb.as_ptr());
        let mut params = [0f32; HUD_PARAMS];
        (r.hud_params)(&mut params);
        hud_composited = cvr_gpu_color_blit_record_hud_composite(
            blit, list_raw, dtex, hud_tex, hud_blur, hud_expo, frame_cb, hud_cb, mtex, &params,
        );
        // The scanner's outline on top, while the mirror texture is still a render target: this window IS the
        // desk-side view of eye 1.
        if hud_composited && CyberpunkVR_VisionToSecondEye.load(Relaxed) != 0 {
            let vis = CyberpunkVR_GetVisionTexture();
            if !vis.is_null()
                && cvr_gpu_color_blit_record_overlay(
                    blit,
                    list_raw,
                    vis,
                    mtex,
                    CyberpunkVR_VisionDebug.load(Relaxed),
                    CyberpunkVR_VisionFit.load(Relaxed) != 0,
                    float(&CyberpunkVR_VisionOffX),
                    float(&CyberpunkVR_VisionOffY),
                    0.0,
                    0.0,
                    core::ptr::null(),
                    0,
                    0,
                    0,
                    0.0,
                )
            {
                diag(&CyberpunkVR_DebugVisionOverlays);
            }
        }
        // The barrel dot, at the NDC the overlay drew it at on the backbuffer.
        let dot_tick = CyberpunkVR_BarrelDotTick.load(Relaxed);
        if hud_composited
            && CyberpunkVR_BarrelDotSecondEye.load(Relaxed) != 0
            && dot_tick != 0
            && tick().wrapping_sub(dot_tick) < 250
            && cvr_gpu_color_blit_record_dot(
                blit,
                list_raw,
                mtex,
                float(&CyberpunkVR_BarrelDotNdcX2),
                float(&CyberpunkVR_BarrelDotNdcY),
                float(&CyberpunkVR_BarrelDotRadiusPx),
                1.0,
                0.045,
                0.045,
                1.0,
            )
        {
            diag(&CyberpunkVR_DebugBarrelDotDraws);
        }
        // The armed dump of the same layer, on the same list and fence; the snapshot rests in COMMON, which promotes
        // implicitly for a copy source.
        if CyberpunkVR_VisionDump.load(Relaxed) == 1 {
            vision_dump_record(dev, list, idx);
        }
        let mut mb = vec![Barrier::transition(mtex, render_target, common)];
        if copy_src_state != common {
            mb.push(Barrier::transition(dtex, pixel_read, copy_src_state));
        }
        barrier_call(list_raw, mb.len() as u32, mb.as_ptr());
    }
    if hud_composited {
        (*r.mirror_pending_list).store(list_raw, Release);
    } else {
        cvr_capture_d12_append_mirror_copy(
            (hook as *const Hook).cast(),
            list_raw,
            dtex,
            copy_src_state,
        );
    }
    // Luma probe: the 8x8 centre of the stable snapshot into this slot's readback, same list, same fence.
    if probing && use_stable && cvr_luma_probe_ensure(idx as u32) {
        let one = |before, after| [Barrier::transition(dtex, before, after)];
        barrier_call(list_raw, 1, one(common, copy_source).as_ptr());
        let src = subresource0(dtex);
        let dst = footprint(g_luma_rb[idx], 0, d.Format, 8, 8, 256);
        let bx = centre_box(d.Width, d.Height);
        list.CopyTextureRegion(&dst, 0, 0, 0, &src, Some(&bx));
        barrier_call(list_raw, 1, one(copy_source, common).as_ptr());
        g_luma_parity[idx] = frame & 1;
        g_luma_frame[idx] = frame;
        g_luma_finidx[idx] = CyberpunkVR_DebugFinNatural.load(Relaxed);
        *r.luma_valid.add(idx) = true;
    }
    // CB-flap probe: the captured tonemap CB (848 B) and both views' adapted-exposure accumulators (28 B each) on the
    // same fence. A changed CB resource is a rebuilt graph: the chain-capture set restarts.
    let cbres = (*r.cb_res).load(Acquire);
    if !cbres.is_null() && (*r.cb_last_res).swap(cbres, AcqRel) != cbres {
        g_cb_reset_pending.store(true, Release);
        // a rebuilt graph: a new publication; submitted readbacks keep only their own sources through the slot fence
        s::TM_INPUTS.reset();
    }
    if probing && use_stable && cvr_cb_probe_ensure(idx as u32) {
        let rb = g_cb_rb[idx];
        let mut any = false;
        if !cbres.is_null() {
            let cb_rest = state(D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER);
            barrier_call(
                list_raw,
                1,
                [Barrier::transition(cbres, cb_rest, copy_source)].as_ptr(),
            );
            if let (Some(dst), Some(src)) = (
                ID3D12Resource::from_raw_borrowed(&rb),
                ID3D12Resource::from_raw_borrowed(&cbres),
            ) {
                list.CopyBufferRegion(dst, 0, src, (*r.cb_off).load(Acquire), 848);
            }
            barrier_call(
                list_raw,
                1,
                [Barrier::transition(cbres, copy_source, cb_rest)].as_ptr(),
            );
            any = true;
        }
        let expo_rest = state(
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE
                | D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        );
        let expo = [(*r.expo_vrcam).load(Acquire), (*r.expo_main).load(Acquire)];
        for (ei, e) in expo.into_iter().enumerate() {
            if e.is_null() {
                continue;
            }
            barrier_call(
                list_raw,
                1,
                [Barrier::transition(e, expo_rest, copy_source)].as_ptr(),
            );
            if let (Some(dst), Some(src)) = (
                ID3D12Resource::from_raw_borrowed(&rb),
                ID3D12Resource::from_raw_borrowed(&e),
            ) {
                list.CopyBufferRegion(dst, if ei == 0 { 856 } else { 896 }, src, 0, 28);
            }
            barrier_call(
                list_raw,
                1,
                [Barrier::transition(e, copy_source, expo_rest)].as_ptr(),
            );
            any = true;
        }
        *r.cb_valid.add(idx) = any;
    }
    // Stage sampling: the 8x8 centre of each captured chain texture, in its last tracked state (restored after);
    // TYPELESS mapped to a concrete member, and anything outside the whitelist skipped -- an invalid copy fails the
    // list's Close every frame and freezes the mirror and every probe with it.
    let mut tm_leases: Vec<ID3D12Resource> = Vec::new();
    if tm_capture_enabled() && use_stable && !s::TM_INPUTS.is_empty() && cvr_ti_probe_ensure(idx as u32) {
        let inputs = s::TM_INPUTS.acquire();
        let mut out_i = 0usize;
        for cap in &inputs {
            if out_i >= 24 {
                break;
            }
            let res = cap.resource.as_raw();
            let st = cap.state;
            let mut rd = D3D12_RESOURCE_DESC::default();
            if !(r.get_desc)(res, &mut rd) {
                continue;
            }
            let cf = match rd.Format.0 as u32 {
                23 => 24, // R10G10B10A2_TYPELESS -> UNORM
                9 => 10,  // R16G16B16A16_TYPELESS -> FLOAT
                27 => 28, // R8G8B8A8_TYPELESS -> UNORM
                39 => 41, // R32_TYPELESS -> R32_FLOAT
                f => f,
            };
            if !matches!(cf, 10 | 24 | 26 | 28 | 41 | 87) {
                continue;
            }
            let need = st != copy_source;
            if need {
                barrier_call(
                    list_raw,
                    1,
                    [Barrier::transition(res, st, copy_source)].as_ptr(),
                );
            }
            let src = subresource0(res);
            let dst = footprint(
                g_ti_rb[idx],
                out_i as u64 * 2048,
                DXGI_FORMAT(cf as i32),
                8,
                8,
                256,
            );
            let bx = centre_box(rd.Width, rd.Height);
            list.CopyTextureRegion(&dst, 0, 0, 0, &src, Some(&bx));
            if need {
                barrier_call(
                    list_raw,
                    1,
                    [Barrier::transition(res, copy_source, st)].as_ptr(),
                );
            }
            g_ti_src[idx][out_i] = res;
            g_ti_fmt[idx][out_i] = cf;
            g_ti_tag[idx][out_i] = cap.tag;
            tm_leases.push(cap.resource.clone());
            out_i += 1;
        }
        g_ti_count[idx] = out_i as u32;
        *r.ti_valid.add(idx) = out_i != 0;
    }
    if list.Close().is_err() {
        let n = CLOSE_FAILS.fetch_add(1, SeqCst) + 1;
        if n == 1 || n % 512 == 0 {
            stereo_log(&format!(
                "[mirror] deferred list Close FAILED (n={n}) -- invalid recorded command; probes+mirror stall until fixed"
            ));
        }
        return;
    }
    let execute = (r.queue_original_execute)(queue);
    if execute == 0 {
        return;
    }
    let execute: unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void) =
        transmute(execute);
    let lists = [list_raw];
    {
        let mut l = leases();
        l.sources[idx] = lease;
        l.tm[idx] = tm_leases;
    }
    execute(queue, 1, lists.as_ptr()); // the queue's own original, past our hook
    let v = FENCE_NEXT.fetch_add(1, Relaxed);
    let signalled = ID3D12CommandQueue::from_raw_borrowed(&queue).is_some_and(|q| q.Signal(fence, v).is_ok());
    if signalled {
        *r.copy_slot_fence.add(idx) = v;
        READY.store(v, Release);
        CyberpunkVR_DebugMirrorReadyFence.store(v, Relaxed);
    } else {
        leases().copy_failed = true;
        stereo_log("[mirror] copy fence Signal failed; retaining submitted sources and stopping copies");
    }
    *r.copy_frame = frame.wrapping_add(1);
    diag(&CyberpunkVR_DebugMirrorPendingHits);
    if PRESENT_STARTED
        .compare_exchange(false, true, SeqCst, SeqCst)
        .is_ok()
    {
        std::thread::spawn(|| {
            cvr_host::name_this_thread("CVR desktop mirror");
            // SAFETY: the present thread's own objects and the attached runtime.
            unsafe { present_thread() }
        });
    }
}

/// The armed one-shot dump of the outline layer: its pixels copied into a readback buffer (made or remade for its
/// size) on this slot, written out once the slot's fence has passed.
unsafe fn vision_dump_record(dev: &ID3D12Device, list: &ID3D12GraphicsCommandList, idx: usize) {
    let r = rt();
    let vis = CyberpunkVR_GetVisionTexture();
    let mut vdd = D3D12_RESOURCE_DESC::default();
    if vis.is_null() || !(r.get_desc)(vis, &mut vdd) {
        stereo_log("[vision] dump armed but no layer available -- scan something first");
        CyberpunkVR_VisionDump.store(0, Relaxed);
        return;
    }
    let (w, h) = (vdd.Width as u32, vdd.Height);
    let pitch = (w.wrapping_mul(4).wrapping_add(255)) & !255u32;
    let need = pitch as u64 * h as u64;
    let rb = &mut *r.visdump_rb;
    if !rb.is_null() && (g_visdump_w != w || g_visdump_h != h) {
        let old = ID3D12Resource::from_raw(*rb);
        old.Unmap(0, None);
        drop(old);
        *rb = null_mut();
        g_visdump_map = null_mut();
    }
    if rb.is_null() {
        let rp = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
            ..Default::default()
        };
        let bd = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: need,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut made: Option<ID3D12Resource> = None;
        if dev
            .CreateCommittedResource(
                &rp,
                D3D12_HEAP_FLAG_NONE,
                &bd,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut made,
            )
            .is_ok()
        {
            if let Some(res) = made {
                let mut map = null_mut();
                if res.Map(0, None, Some(&mut map)).is_err() {
                    map = null_mut();
                }
                g_visdump_map = map;
                g_visdump_w = w;
                g_visdump_h = h;
                g_visdump_pitch = pitch;
                *rb = res.into_raw();
            }
        }
    }
    if !rb.is_null() && !g_visdump_map.is_null() {
        let src = subresource0(vis);
        let dst = footprint(*rb, 0, DXGI_FORMAT_R8G8B8A8_UNORM, w, h, pitch);
        list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
        *r.visdump_slot = idx as i32;
        CyberpunkVR_VisionDump.store(2, Relaxed);
    }
}

unsafe extern "system" fn mirror_wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLOSE {
        return LRESULT(0); // not destroyed on close
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// EXACT replica of the engine's PipelineState_1335 MODE 0 (the SDR 8-bit path): the HDR texel loaded unfiltered,
/// max(0), the engine's sRGB OETF, the animated triangular dither. No tonemap (done earlier) and no exposure scale.
use crate::mirror_shaders::CONVERT_HLSL;
type CompileFn = unsafe extern "system" fn(
    *const c_void,
    usize,
    PCSTR,
    *const D3D_SHADER_MACRO,
    *mut c_void,
    PCSTR,
    PCSTR,
    u32,
    u32,
    *mut Option<ID3DBlob>,
    *mut Option<ID3DBlob>,
) -> HRESULT;
type SerializeFn = unsafe extern "system" fn(
    *const D3D12_ROOT_SIGNATURE_DESC,
    D3D_ROOT_SIGNATURE_VERSION,
    *mut Option<ID3DBlob>,
    *mut Option<ID3DBlob>,
) -> HRESULT;
struct Convert {
    root: ID3D12RootSignature,
    pso: ID3D12PipelineState,
    srv_heap: ID3D12DescriptorHeap,
}
unsafe fn blob_bytes(b: &ID3DBlob) -> &[u8] {
    core::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())
}
/// The format-converting present pipeline: the mirror texture is a packed-float HDR target, not copy-compatible with
/// the 8-bit backbuffer, so it is sampled in a fullscreen pass that writes the backbuffer.
unsafe fn build_convert(
    dev: &ID3D12Device,
    fmt: DXGI_FORMAT,
    mtex: *mut c_void,
) -> Option<Convert> {
    let d3dc = GetModuleHandleW(w!("d3dcompiler_47.dll"))
        .or_else(|_| LoadLibraryW(w!("d3dcompiler_47.dll")))
        .ok();
    let compile: CompileFn = transmute(d3dc.and_then(|m| GetProcAddress(m, s!("D3DCompile")))?);
    let d3d12 = GetModuleHandleW(w!("d3d12.dll")).ok();
    let serialize: SerializeFn =
        transmute(d3d12.and_then(|m| GetProcAddress(m, s!("D3D12SerializeRootSignature")))?);
    let (mut vs, mut ps, mut err) = (None, None, None);
    let src = CONVERT_HLSL.as_bytes();
    let stage =
        |entry: PCSTR, target: PCSTR, out: &mut Option<ID3DBlob>, err: &mut Option<ID3DBlob>| {
            compile(
                src.as_ptr().cast(),
                src.len(),
                s!("conv"),
                core::ptr::null(),
                null_mut(),
                entry,
                target,
                0,
                0,
                out,
                err,
            )
        };
    // the stages made from Rust (rust/shaders/passes/src/mirror.rs); the HLSL above with CVR_SHADER_HLSL
    let vs = cvr_gpu::dxil::pick(cvr_gpu::dxil::MIRROR_VS, || {
        if stage(s!("VSMain"), s!("vs_5_0"), &mut vs, &mut err).is_err() {
            return None;
        }
        vs.take()
    })?;
    let ps = cvr_gpu::dxil::pick(cvr_gpu::dxil::MIRROR_PS, || {
        if stage(s!("PSMain"), s!("ps_5_0"), &mut ps, &mut err).is_err() {
            return None;
        }
        ps.take()
    })?;
    let range = D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 1,
        ..Default::default()
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
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Constants: D3D12_ROOT_CONSTANTS {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                    Num32BitValues: 1,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        },
    ];
    let sampler = D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        MaxLOD: D3D12_FLOAT32_MAX,
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        ..Default::default()
    };
    let rsd = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: 2,
        pParameters: params.as_ptr(),
        NumStaticSamplers: 1,
        pStaticSamplers: &sampler,
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    let mut rs_blob = None;
    if serialize(&rsd, D3D_ROOT_SIGNATURE_VERSION_1, &mut rs_blob, null_mut()).is_err() {
        return None;
    }
    let rs_blob = rs_blob?;
    let root: ID3D12RootSignature = dev.CreateRootSignature(0, blob_bytes(&rs_blob)).ok()?;
    let mut pd = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: transmute_copy(&root),
        VS: vs.bytecode(),
        PS: ps.bytecode(),
        SampleMask: 0xFFFF_FFFF,
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };
    pd.BlendState.RenderTarget[0].RenderTargetWriteMask = 0xF;
    pd.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    pd.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    pd.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
    let pso: ID3D12PipelineState = dev.CreateGraphicsPipelineState(&pd).ok()?;
    let shd = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: 1,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
        NodeMask: 0,
    };
    let srv_heap: ID3D12DescriptorHeap = dev.CreateDescriptorHeap(&shd).ok()?;
    let sv = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: fmt,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MipLevels: 1,
                ..Default::default()
            },
        },
    };
    dev.CreateShaderResourceView(
        ID3D12Resource::from_raw_borrowed(&mtex),
        Some(&sv),
        srv_heap.GetCPUDescriptorHandleForHeapStart(),
    );
    Some(Convert {
        root,
        pso,
        srv_heap,
    })
}

/// The present thread: waits for the mirror texture, makes its queue, list, fence, window (client area = the second
/// view's resolution, so a capture tool gets native pixels) and swapchain, then forever: pump messages, follow the
/// toggle with the window itself, GPU-wait the game's fence, convert (or test-pattern, or copy) into the backbuffer
/// and present with vsync on this thread only.
unsafe fn present_thread() {
    let r = rt();
    let (mut w, mut h) = (0u32, 0u32);
    for _ in 0..6000 {
        if CyberpunkVR_MirrorOutput.load(Relaxed) == 0 {
            break;
        }
        if !(*r.d12_mtex).is_null() && !g_game_device.is_null() && !(*r.d12_fence).is_null() {
            w = *r.d12_w;
            h = *r.d12_h;
            break;
        }
        Sleep(10);
    }
    if (*r.d12_mtex).is_null() || g_game_device.is_null() || (*r.d12_fence).is_null() {
        return;
    }
    let device = g_game_device;
    let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
        return;
    };
    let qd = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };
    let Ok(q) = dev.CreateCommandQueue::<ID3D12CommandQueue>(&qd) else {
        return;
    };
    let Ok(alloc) =
        dev.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
    else {
        return;
    };
    let Ok(list) = dev.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
        0,
        D3D12_COMMAND_LIST_TYPE_DIRECT,
        &alloc,
        None::<&ID3D12PipelineState>,
    ) else {
        return;
    };
    let _ = list.Close();
    let Ok(pf) = dev.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) else {
        return;
    };
    let ev = CreateEventW(None, false, false, PCWSTR::null()).unwrap_or_default();
    let mut pv = 0u64;

    let hinstance = GetModuleHandleW(None).unwrap_or_default();
    let class = w!("CyberpunkVR_Mirror");
    let wc = WNDCLASSEXW {
        cbSize: core::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(mirror_wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassExW(&wc);
    let mut wr = RECT {
        left: 0,
        top: 0,
        right: w as i32,
        bottom: h as i32,
    };
    let _ = AdjustWindowRect(&mut wr, WS_OVERLAPPEDWINDOW, false);
    let Ok(hwnd) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("VRCAM Mirror (OBS)"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        wr.right - wr.left,
        wr.bottom - wr.top,
        None,
        None,
        Some(hinstance.into()),
        None,
    ) else {
        return;
    };
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    let dxgi = GetModuleHandleW(w!("dxgi.dll"))
        .or_else(|_| LoadLibraryW(w!("dxgi.dll")))
        .ok();
    type FactoryFn = unsafe extern "system" fn(u32, *const GUID, *mut *mut c_void) -> HRESULT;
    let Some(create_factory) = dxgi.and_then(|m| GetProcAddress(m, s!("CreateDXGIFactory2")))
    else {
        return;
    };
    let create_factory: FactoryFn = transmute(create_factory);
    let mut fac_raw = null_mut();
    if create_factory(0, &IDXGIFactory2::IID, &mut fac_raw).is_err() || fac_raw.is_null() {
        return;
    }
    let fac = IDXGIFactory2::from_raw(fac_raw);
    let sd = DXGI_SWAP_CHAIN_DESC1 {
        Width: w,
        Height: h,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        Scaling: DXGI_SCALING_STRETCH,
        ..Default::default()
    };
    let made = fac.CreateSwapChainForHwnd(&q, hwnd, &sd, None, None);
    drop(fac);
    CyberpunkVR_DebugMirrorLastHr.store(
        made.as_ref().map_or_else(|e| e.code().0 as u32, |_| 0),
        Relaxed,
    );
    let Ok(sc1) = made else {
        return;
    };
    let Ok(sc) = sc1.cast::<IDXGISwapChain3>() else {
        return;
    };
    drop(sc1);
    CyberpunkVR_DebugMirrorState.store(3, Relaxed);
    stereo_log(&format!("[mirror] d12 present-thread ready {w}x{h}"));

    // The two backbuffers' RTVs (used by the conversion pass and the red test pattern).
    let mut rtv_heap: Option<ID3D12DescriptorHeap> = None;
    let mut rtv_h = [D3D12_CPU_DESCRIPTOR_HANDLE::default(); 2];
    let hd = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
        NumDescriptors: 2,
        ..Default::default()
    };
    if let Ok(heap) = dev.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&hd) {
        let inc = dev.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) as usize;
        let base = heap.GetCPUDescriptorHandleForHeapStart();
        for (i, handle) in rtv_h.iter_mut().enumerate() {
            handle.ptr = base.ptr + inc * i;
            if let Ok(bb) = sc.GetBuffer::<ID3D12Resource>(i as u32) {
                let orig = *r.orig_create_rtv;
                if orig != 0 {
                    let create: unsafe extern "system" fn(
                        *mut c_void,
                        *mut c_void,
                        *const c_void,
                        usize,
                    ) = transmute(orig);
                    create(device, bb.as_raw(), core::ptr::null(), handle.ptr);
                } else {
                    dev.CreateRenderTargetView(&bb, None, *handle);
                }
            }
        }
        rtv_heap = Some(heap);
    }

    let fmt = *r.d12_fmt;
    let convert = build_convert(dev, fmt, *r.d12_mtex);
    stereo_log(&format!(
        "[mirror] convert-pipeline {} (mtex fmt={})",
        if convert.is_some() { "ready" } else { "FAILED" },
        fmt.0 as u32
    ));

    let render_target = state(D3D12_RESOURCE_STATE_RENDER_TARGET);
    let present = state(D3D12_RESOURCE_STATE_PRESENT);
    let mut last = 0u64;
    let mut shown = true; // the window was created visible
    loop {
        retire_mirror_sources();
        let mut m = MSG::default();
        while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        // The window follows the toggle: pausing Present alone would leave a frozen mirror on screen.
        let want = CyberpunkVR_MirrorOutput.load(Relaxed) != 0;
        if want != shown {
            shown = want;
            let _ = ShowWindow(hwnd, if want { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        if !want {
            Sleep(50);
            continue;
        }
        let testpat = CyberpunkVR_MirrorTestPattern.load(Relaxed) != 0 && rtv_heap.is_some();
        let rdy = READY.load(Acquire);
        if !testpat && rdy <= last {
            Sleep(2);
            continue;
        }
        let game_fence = *r.d12_fence;
        if !testpat {
            if let Some(f) = ID3D12Fence::from_raw_borrowed(&game_fence) {
                let _ = q.Wait(f, rdy); // GPU: our read after the game's copy
            }
        }
        let _ = alloc.Reset();
        let _ = list.Reset(&alloc, None::<&ID3D12PipelineState>);
        let idx = sc.GetCurrentBackBufferIndex() as usize;
        let Ok(bb) = sc.GetBuffer::<ID3D12Resource>(idx as u32) else {
            let _ = list.Close();
            continue;
        };
        let bbr = bb.as_raw();
        let mtex = *r.d12_mtex;
        if testpat {
            list_barriers(&list, &[Barrier::transition(bbr, present, render_target)]);
            list.ClearRenderTargetView(rtv_h[idx], &[1.0, 0.0, 0.0, 1.0], None);
            list_barriers(&list, &[Barrier::transition(bbr, render_target, present)]);
        } else if let (Some(conv), true) = (&convert, rtv_heap.is_some()) {
            // HDR mirror texture -> 8-bit backbuffer through a fullscreen sample+write: the HDR->8-bit composite
            // MAIN's RenderFinal2D does into its swapchain, for the second view's final.
            let common = state(D3D12_RESOURCE_STATE_COMMON);
            let pixel_read = state(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE);
            list_barriers(&list, &[Barrier::transition(mtex, common, pixel_read)]);
            list_barriers(&list, &[Barrier::transition(bbr, present, render_target)]);
            list.SetDescriptorHeaps(&[Some(conv.srv_heap.clone())]);
            list.SetGraphicsRootSignature(&conv.root);
            list.SetPipelineState(&conv.pso);
            list.SetGraphicsRootDescriptorTable(
                0,
                conv.srv_heap.GetGPUDescriptorHandleForHeapStart(),
            );
            let dither_t = (pv & 0x3FF) as f32; // the dither animates per present
            list.SetGraphicsRoot32BitConstants(1, 1, (&dither_t as *const f32).cast(), 0);
            let vp = D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            let rc = RECT {
                left: 0,
                top: 0,
                right: w as i32,
                bottom: h as i32,
            };
            list.RSSetViewports(&[vp]);
            list.RSSetScissorRects(&[rc]);
            list.OMSetRenderTargets(1, Some(&rtv_h[idx]), false, None);
            list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            list.DrawInstanced(3, 1, 0, 0);
            list_barriers(&list, &[Barrier::transition(bbr, render_target, present)]);
            list_barriers(&list, &[Barrier::transition(mtex, pixel_read, common)]);
        } else {
            let copy_dest = state(D3D12_RESOURCE_STATE_COPY_DEST);
            list_barriers(&list, &[Barrier::transition(bbr, present, copy_dest)]);
            if let Some(src) = ID3D12Resource::from_raw_borrowed(&mtex) {
                list.CopyResource(&bb, src); // mtex COMMON -> implicit COPY_SOURCE
            }
            list_barriers(&list, &[Barrier::transition(bbr, copy_dest, present)]);
        }
        let _ = list.Close();
        if let Ok(cl) = list.cast::<ID3D12CommandList>() {
            q.ExecuteCommandLists(&[Some(cl)]);
        }
        let _ = sc.Present(if testpat { 0 } else { 1 }, DXGI_PRESENT(0)); // vsync on OUR thread only
        drop(bb);
        pv += 1;
        let _ = q.Signal(&pf, pv); // throttles THIS thread (allocator reuse)
        if pf.GetCompletedValue() < pv {
            let _ = pf.SetEventOnCompletion(pv, ev);
            WaitForSingleObject(ev, 100);
        }
        diag(&CyberpunkVR_DebugMirrorFrames);
        last = rdy;
        if testpat {
            Sleep(16);
        }
    }
}
