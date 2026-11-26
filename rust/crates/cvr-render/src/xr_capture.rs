//! THE CAPTURE ON THE GAME'S PRESENT (formerly src/Runtimes/OpenXRCapture.cpp CaptureMonoPresentedFrame), part by part.
//!
//! The second eye's picture from the port's own second view (VRCAM), recorded on the capture list -- the same list,
//! fence and moment as MAIN's copy, which is the whole point: sync_stereo's snapshot belongs to the engine and the
//! desktop mirror also reads it from another queue; sampling it from the submit thread at display rate put two queues
//! on one resource and hung the GPU on the first VRCAM frame. Here the engine's frame is already submitted and this is
//! the Present thread, the one point where reading it is ours to do.
//!
//! It cannot be copied the way MAIN is: MAIN's backbuffer is sRGB-encoded UNORM bytes matching the eye swapchain bit
//! for bit, while this is RenderFinal2D's output (R11G11B10_FLOAT, LINEAR values). So it goes through the colour blit
//! into a slot typed as the eye swapchain's own format: the sampler resizes, and writing through an _UNORM_SRGB view
//! makes the hardware encode. The slots are MAIN's size (the blit rescales VRCAM when the two disagree),
//! R8G8B8A8_TYPELESS (the UNORM -> UNORM_SRGB view cast is legal only through a typeless resource), viewed as
//! R8G8B8A8_UNORM_SRGB. Deriving these from the XR image instead is what killed the GPU twice: that resource is
//! typeless, and a typeless RTV is invalid.
//!
//! The OpenXRManager's state stays the C++ half's for now: the slot pool, its leases and the lobby's intro overlay come
//! in through callbacks.
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

use cvr_camera::projection::{map_ndc_x, EyeWindow};
use cvr_crt::{format, Arg};
use openxr_sys as xr;
use windows::core::Interface as _;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DESC;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12GraphicsCommandList, ID3D12Resource, D3D12_BOX, D3D12_RESOURCE_BARRIER,
    D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
    D3D12_RESOURCE_BARRIER_FLAG_NONE, D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_STATES, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET,
    D3D12_RESOURCE_TRANSITION_BARRIER, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};

/// The colour blit's HUD composite parameters (ColorBlitHudParams): 33 root constants, hudShiftU the 25th.
pub const HUD_PARAMS: usize = 33;
const HUD_SHIFT_U: usize = 24;

/// What the second eye's capture needs of the capture and of OpenXRManager, and its callbacks.
#[repr(C)]
pub struct SecondEyeIn {
    /// ID3D12Device*, the capture's ID3D12GraphicsCommandList* (recording).
    pub device: *mut c_void,
    pub list: *mut c_void,
    /// The capture's GPU stage batch, or null.
    pub profile: *const c_void,
    /// The eye's size: MAIN's.
    pub eye_w: u32,
    pub eye_h: u32,
    /// m_eyeSwapchains[1].width (0 without a second view), the runtime's IPD.
    pub eye1_width: i32,
    pub ipd: f32,
    /// CyberpunkVR_MainIsRightEye; cvr::projection::Active().
    pub main_is_right: bool,
    pub asym_active: bool,
    /// The lobby's curtain and intro are drawn this frame (Render/IntroOverlay.hpp).
    pub intro: bool,
    pub ctx: *mut c_void,
    /// EnsureVrcamEyeTexture(w, h, R8G8B8A8_TYPELESS): the slot pool at this size.
    pub ensure_slots: extern "C" fn(ctx: *mut c_void, width: u32, height: u32) -> bool,
    /// The next slot no submit reads and the frame just published does not pair with, its write lease taken; null
    /// when every one is in use.
    pub take_slot: extern "C" fn(ctx: *mut c_void) -> *mut c_void,
    /// cvr::CyberpunkVR_GetHudParams().
    pub hud_params: extern "C" fn(ctx: *mut c_void, out: *mut [f32; HUD_PARAMS]),
    /// GetCurrentEyeFov(1): the second eye's FOV.
    pub eye1_fov: extern "C" fn(ctx: *mut c_void, out: *mut xr::Fovf) -> bool,
    /// GetAsymmetricEyeWindows: both eyes' windows.
    pub asym_windows: extern "C" fn(ctx: *mut c_void, out: *mut [EyeWindow; 2]) -> bool,
    /// The intro overlay recorded into the slot, seen through the second eye's image pose (the source's label).
    pub intro_record: extern "C" fn(ctx: *mut c_void, slot: *mut c_void, source: *mut c_void),
}

const _: () = assert!(
    core::mem::size_of::<SecondEyeIn>() == 104 && core::mem::offset_of!(SecondEyeIn, ctx) == 48
);

/// The second eye's capture: the source it read (for the frame's labels), and how it went.
#[repr(C)]
pub struct SecondEyeOut {
    pub source: *mut c_void,
    /// SECOND_EYE_* below.
    pub status: i32,
}
/// The slot holds the second eye's picture.
pub const SECOND_EYE_CAPTURED: i32 = 1;
/// No picture: the eye stays on MAIN's.
pub const SECOND_EYE_NONE: i32 = 0;
/// Every slot was in use: the capture stops (the list is closed unsubmitted, nothing is published).
pub const SECOND_EYE_NO_SLOT: i32 = -1;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_XrDeepDiag: i32;
    static CyberpunkVR_DebugVrcamEyeAgeMs: u32;
    static CyberpunkVR_DebugVrcamEyeAgeUs: u32;
    static CyberpunkVR_DebugEyeAgeCount: AtomicU64;
    static CyberpunkVR_DebugEyeAgeSumMs: AtomicU64;
    static CyberpunkVR_DebugEyeAgeMaxMs: AtomicU64;
    static CyberpunkVR_DebugEyeAgeNever: AtomicU64;
    static CyberpunkVR_DebugEyeAgeBuckets: [AtomicU64; 4];
    static CyberpunkVR_HudDistanceM: f32;
    static CyberpunkVR_SecondEyeCopy: i32;
    static CyberpunkVR_DebugSecondEyeCopies: AtomicU64;
    static CyberpunkVR_VisionToSecondEye: i32;
    static CyberpunkVR_VisionDebug: i32;
    static CyberpunkVR_VisionFit: i32;
    static CyberpunkVR_VisionOffX: f32;
    static CyberpunkVR_VisionOffY: f32;
    static mut CyberpunkVR_DebugVisionOverlays: u64;
    static CyberpunkVR_BarrelDotNdcX2: f32;
    static CyberpunkVR_BarrelDotNdcY: f32;
    static CyberpunkVR_BarrelDotRadiusPx: f32;
    static CyberpunkVR_BarrelDotTick: u64;
    static CyberpunkVR_BarrelDotSecondEye: i32;
    static mut CyberpunkVR_DebugBarrelDotDraws: u64;
    static CyberpunkVR_OverlaySecondEye: i32;
    static CyberpunkVR_OverlaySecondEyeDistM: f32;
    fn CyberpunkVR_GetVrcamEyeTextureFresh() -> *mut c_void;
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
extern "system" {
    fn GetTickCount64() -> u64;
}

/// A C++ export read as the other threads write it.
macro_rules! live {
    ($name:ident) => {{
        #[allow(unused_unsafe)]
        // SAFETY: an export of the C++ half, read as a plain value.
        let value = unsafe { core::ptr::read_volatile(&raw const $name) };
        value
    }};
}
fn diagnostics() -> bool {
    live!(CyberpunkVR_RuntimeDiagnostics) != 0
}

/// cvr::gpu::profile::Stage.
const STAGE_CAPTURE_SECOND: u32 = 1;
const STAGE_SECOND_HUD: u32 = 5;
const STAGE_SECOND_BLIT: u32 = 6;

/// A cvr::gpu::profile::Scope: the stage timed until dropped (nothing without a batch).
struct Scope {
    batch: *const cvr_gpu::stage_profile::Batch,
    index: u32,
}
impl Scope {
    fn new(batch: *const c_void, stage: u32) -> Self {
        let batch = batch.cast::<cvr_gpu::stage_profile::Batch>();
        // SAFETY: the capture's live batch, or null.
        let index = if batch.is_null() {
            u32::MAX
        } else {
            unsafe { cvr_gpu::stage_profile::cvr_gpu_stage_scope_begin(batch, stage) }
        };
        Self { batch, index }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if self.index != u32::MAX {
            // SAFETY: the batch the scope was opened on, alive for the capture.
            unsafe { cvr_gpu::stage_profile::cvr_gpu_stage_scope_end(self.batch, self.index) };
        }
    }
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
                // SAFETY: a borrowed reference for the barrier's recording, not released by it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}
fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    // SAFETY: a recording list and a live resource.
    unsafe { list.ResourceBarrier(&[transition(resource, before, after)]) };
}

/// The 8-bit RGBA copy family of a format: 1 R8G8B8A8, 2 B8G8R8A8, 0 none.
pub fn copy_family(f: DXGI_FORMAT) -> i32 {
    match f {
        DXGI_FORMAT_R8G8B8A8_TYPELESS
        | DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => 1,
        DXGI_FORMAT_B8G8R8A8_TYPELESS
        | DXGI_FORMAT_B8G8R8A8_UNORM
        | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => 2,
        _ => 0,
    }
}
/// CopyResource legality plus "the blit would not have scaled": same size, one mip, one layer, no MSAA, the same 8-bit
/// RGBA copy family.
fn copy_equivalent(src: &ID3D12Resource, dst: &ID3D12Resource) -> bool {
    // SAFETY: live resources.
    let (a, b) = unsafe { (src.GetDesc(), dst.GetDesc()) };
    a.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && b.Dimension == a.Dimension
        && a.Width == b.Width
        && a.Height == b.Height
        && a.DepthOrArraySize == 1
        && b.DepthOrArraySize == 1
        && a.MipLevels == 1
        && b.MipLevels == 1
        && a.SampleDesc.Count == 1
        && b.SampleDesc.Count == 1
        && copy_family(a.Format) != 0
        && copy_family(a.Format) == copy_family(b.Format)
}

/// THE AGE OF THE SECOND EYE'S CONTENT, the only quantity that can be asymmetric between the eyes. [xreye] counts
/// whether the eye got its own IMAGE, by serial -- but the serial is stamped when the blit runs, and the blit copies
/// whatever the second view last produced, so a frame the engine did not re-render for that view is copied again with
/// a fresh serial: paired by stamp, stale by content. Microseconds (the millisecond value comes off a 15.6 ms clock);
/// buckets in whole game frames (20 ms). Behind CyberpunkVR_XrDeepDiag; the staleness gate itself is not gated.
fn eye_age_diagnostics() {
    if live!(CyberpunkVR_XrDeepDiag) == 0 {
        return;
    }
    let age_us = u64::from(live!(CyberpunkVR_DebugVrcamEyeAgeUs));
    let age_ms = live!(CyberpunkVR_DebugVrcamEyeAgeMs);
    // SAFETY (all): the C++ half's counters.
    unsafe {
        if age_ms != 0xFFFF_FFFF {
            if diagnostics() {
                CyberpunkVR_DebugEyeAgeCount.fetch_add(1, Relaxed);
                CyberpunkVR_DebugEyeAgeSumMs.fetch_add(age_us, Relaxed);
            }
            CyberpunkVR_DebugEyeAgeMaxMs.fetch_max(age_us, Relaxed);
            let bucket = (age_us / 20_000).min(3) as usize;
            if diagnostics() {
                CyberpunkVR_DebugEyeAgeBuckets[bucket].fetch_add(1, Relaxed);
            }
        } else if diagnostics() {
            CyberpunkVR_DebugEyeAgeNever.fetch_add(1, Relaxed);
        }
    }
}

/// The disparity of a panel put at `distance` metres for this eye alone (MAIN's is the engine's own draw and cannot
/// move), per radian of the panel's angular offset ipd / distance: through the eye's own half-width and half-FOV, so it
/// holds at any resolution or lens. None when the distance is off or the eye's FOV or the IPD is not usable.
fn disparity_per_rad(c: &SecondEyeIn, distance: f32) -> Option<f32> {
    if distance <= 0.05 || c.eye1_width <= 0 {
        return None;
    }
    let mut f: xr::Fovf = xr::Fovf {
        angle_left: 0.0,
        angle_right: 0.0,
        angle_up: 0.0,
        angle_down: 0.0,
    };
    if !(c.eye1_fov)(c.ctx, &mut f) || !(c.ipd > 0.03 && c.ipd < 0.10) {
        return None;
    }
    let half_tan = 0.5 * (f.angle_right.tan() - f.angle_left.tan());
    (half_tan > 0.01).then_some(half_tan)
}
/// Under the asymmetric projection the second eye's window is centred elsewhere than MAIN's: where MAIN's centre sits
/// in it, in NDC (a positive move samples further left).
fn asym_move(c: &SecondEyeIn) -> Option<f32> {
    if !c.asym_active {
        return None;
    }
    let mut w = [EyeWindow::default(), EyeWindow::default()];
    if !(c.asym_windows)(c.ctx, &mut w) {
        return None;
    }
    let main = usize::from(c.main_is_right);
    Some(map_ndc_x(0.0, &w[main], &w[1 - main]))
}

/// The second eye's picture from VRCAM's view (the capture's VRCAM branch): its content age counted, the freshest VRCAM
/// image (null past the staleness limit), the slot pool and the blit made ready, a slot taken, and into it -- the HUD
/// composite (the engine's own HUD pass, PipelineState_576, ported shader for shader: it reads the scene and produces
/// the finished eye in one go) or, without a HUD, a plain copy or blit; then on top the scanner outline, the barrel
/// dot, the F10 overlay and the lobby's intro, as MAIN's backbuffer has them.
///
/// # Safety
/// `input` and `out` are valid; the device, list, blit and batch are the capture's live objects.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_second_eye(
    input: *const SecondEyeIn,
    out: *mut SecondEyeOut,
) {
    let (Some(c), Some(out)) = (unsafe { input.as_ref() }, unsafe { out.as_mut() }) else {
        return;
    };
    out.status = SECOND_EYE_NONE;
    eye_age_diagnostics();
    // SAFETY: the C++ half's getter.
    let src_raw = unsafe { CyberpunkVR_GetVrcamEyeTextureFresh() };
    out.source = src_raw;
    // SAFETY: the caller's objects, borrowed.
    let (Some(src), Some(list)) = (
        unsafe { ID3D12Resource::from_raw_borrowed(&src_raw) },
        unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&c.list) },
    ) else {
        return;
    };
    if !(c.ensure_slots)(c.ctx, c.eye_w, c.eye_h) {
        return;
    }
    // SAFETY: the capture's blit and device.
    if !unsafe {
        cvr_gpu::color_blit::cvr_gpu_color_blit_ensure(
            crate::xr_capture_state::eye_blit().cast(),
            c.device,
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0,
            c.eye_w,
            c.eye_h,
        )
    } {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Relaxed) {
            cvr_host::log::write(
                &format(
                    c"OpenXRManager: stereo ColorBlit init failed (%ux%u) -- right eye stays on MAIN\n",
                    &[Arg::U(u64::from(c.eye_w)), Arg::U(u64::from(c.eye_h))],
                )
                .to_string_lossy(),
            );
        }
        return;
    }
    // No barrier on the source: a transition asserts the resource's current state, and this one is not ours to assert
    // about -- that lie is exactly what hung the device. Resting in COMMON it is promoted to the read state implicitly
    // and decays back afterwards; sync_stereo creates it with ALLOW_SIMULTANEOUS_ACCESS so the overlap is defined.
    // ROTATE FIRST, then write: the slot taken here is the one the submit looks up by serial, so the producer never
    // touches an image a consumer is still copying out of.
    let slot_raw = (c.take_slot)(c.ctx);
    // SAFETY: the pool's slot, borrowed.
    let Some(slot) = (unsafe { ID3D12Resource::from_raw_borrowed(&slot_raw) }) else {
        out.status = SECOND_EYE_NO_SLOT;
        return;
    };
    let _scope = Scope::new(c.profile, STAGE_CAPTURE_SECOND);
    barrier(
        list,
        slot,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    // SAFETY (whole remainder): the capture's objects and the C++ half's exports.
    let captured = unsafe { compose(c, list, src_raw, src, slot_raw, slot) };
    barrier(
        list,
        slot,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
    );
    if captured {
        out.status = SECOND_EYE_CAPTURED;
    }
}

/// # Safety
/// As cvr_xr_capture_second_eye; the slot in RENDER_TARGET.
unsafe fn compose(
    c: &SecondEyeIn,
    list: &ID3D12GraphicsCommandList,
    src_raw: *mut c_void,
    src: &ID3D12Resource,
    slot_raw: *mut c_void,
    slot: &ID3D12Resource,
) -> bool {
    use cvr_gpu::color_blit as blit;
    let b = crate::xr_capture_state::eye_blit().cast();
    // SAFETY (whole body): as the caller promises.
    unsafe {
        // ---- the HUD, for the eye the engine refuses to draw it for. The composite constants are OPTIONAL: they are
        // found by fingerprinting the engine's upload traffic, which can take long and in a bad session never succeeds,
        // while the shader validates whatever is bound at b6 and falls back to its own captured constants -- so the
        // per-frame constants are bound in their place and the shader decides, exactly as the mirror does.
        let hud = CyberpunkVR_GetHudTexture();
        let hud_blur = if hud.is_null() {
            core::ptr::null_mut()
        } else {
            CyberpunkVR_GetHudBlurTexture()
        };
        let hud_expo = if hud.is_null() {
            core::ptr::null_mut()
        } else {
            CyberpunkVR_GetHudExposureBuffer()
        };
        let frame_cb = if hud.is_null() {
            core::ptr::null_mut()
        } else {
            CyberpunkVR_GetFrameConstantBuffer()
        };
        let mut hud_cb = if hud.is_null() {
            core::ptr::null_mut()
        } else {
            CyberpunkVR_GetHudConstantBuffer()
        };
        CyberpunkVR_NoteHudCompositeInputs(hud, hud_blur, hud_expo, frame_cb, hud_cb);
        if hud_cb.is_null() {
            hud_cb = frame_cb;
        }
        let mut captured = false;
        if !hud.is_null()
            && !hud_blur.is_null()
            && !hud_expo.is_null()
            && !frame_cb.is_null()
            && !hud_cb.is_null()
        {
            let mut params = [0.0f32; HUD_PARAMS];
            (c.hud_params)(c.ctx, &mut params);
            // THE HUD AT A DISTANCE INSTEAD OF AT INFINITY (CyberpunkVR_HudDistanceM, default 0: identical placement in
            // both eyes). Pasted at MAIN's pixel coordinates its disparity is zero -- optical infinity, which a Pimax
            // tester could not fuse with the world an arm's length away. Negative: an object nearer than infinity sits
            // LEFT of its infinity position in the right eye.
            let distance = live!(CyberpunkVR_HudDistanceM);
            if let Some(half_tan) = disparity_per_rad(c, distance) {
                params[HUD_SHIFT_U] = -(c.ipd / distance) * (0.5 / half_tan);
            }
            // MAIN's HUD is laid out on MAIN's window.
            if let Some(m) = asym_move(c) {
                params[HUD_SHIFT_U] -= 0.5 * m;
            }
            let _hud_scope = Scope::new(c.profile, STAGE_SECOND_HUD);
            captured = blit::cvr_gpu_color_blit_record_hud_composite(
                b, c.list, src_raw, hud, hud_blur, hud_expo, frame_cb, hud_cb, slot_raw, &params,
            );
        }
        if !captured {
            let _blit_scope = Scope::new(c.profile, STAGE_SECOND_BLIT);
            // THE SECOND EYE WITHOUT A COMPOSITE IS A COPY: the blit at the same size is an identity (apart from alpha,
            // which the projection layer ignores), measured at 134 us against 34 us for a copy.
            if live!(CyberpunkVR_SecondEyeCopy) != 0 && copy_equivalent(src, slot) {
                // The source is promoted from COMMON to COPY_SOURCE implicitly; only our own slot is transitioned.
                barrier(
                    list,
                    slot,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                );
                list.CopyResource(slot, src);
                barrier(
                    list,
                    slot,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                );
                captured = true;
                if diagnostics() {
                    CyberpunkVR_DebugSecondEyeCopies.fetch_add(1, Relaxed);
                }
            } else {
                captured = blit::cvr_gpu_color_blit_record_blit(
                    b, c.list, src_raw, slot_raw, 0.0, 0.0, 1.0, 1.0,
                );
            }
        }
        // The scanner outline ON TOP of the finished eye, premultiplied, as the engine's own composite adds it (null
        // when the layer is stale).
        if captured && live!(CyberpunkVR_VisionToSecondEye) != 0 {
            let vision = CyberpunkVR_GetVisionTexture();
            if !vision.is_null()
                && blit::cvr_gpu_color_blit_record_overlay(
                    b,
                    c.list,
                    vision,
                    slot_raw,
                    live!(CyberpunkVR_VisionDebug),
                    live!(CyberpunkVR_VisionFit) != 0,
                    live!(CyberpunkVR_VisionOffX),
                    live!(CyberpunkVR_VisionOffY),
                    0.0,
                    0.0,
                    core::ptr::null(),
                    0,
                    0,
                    0,
                    0.0,
                )
                && diagnostics()
            {
                let p = &raw mut CyberpunkVR_DebugVisionOverlays;
                p.write_volatile(p.read_volatile().wrapping_add(1));
            }
        }
        // The barrel dot: the point MAIN's overlay stamps into eye 0, at the same NDC in this eye -- with a dot in both
        // eyes the sight's reticle can be judged.
        let tick = live!(CyberpunkVR_BarrelDotTick);
        if captured
            && live!(CyberpunkVR_BarrelDotSecondEye) != 0
            && tick != 0
            && GetTickCount64().wrapping_sub(tick) < 250
        {
            if blit::cvr_gpu_color_blit_record_dot(
                b,
                c.list,
                slot_raw,
                live!(CyberpunkVR_BarrelDotNdcX2),
                live!(CyberpunkVR_BarrelDotNdcY),
                live!(CyberpunkVR_BarrelDotRadiusPx),
                1.0,
                0.045,
                0.045,
                1.0,
            ) && diagnostics()
            {
                let p = &raw mut CyberpunkVR_DebugBarrelDotDraws;
                p.write_volatile(p.read_volatile().wrapping_add(1));
            }
        }
        // AND THE OVERLAY ITSELF -- the F10 menu and the cursor, drawn by ImGui into MAIN's backbuffer -- LAST, so the
        // UI sits on top of the HUD, the outline and the dot as on the flat screen. CyberpunkVR_OverlaySecondEyeDistM
        // is CyberpunkVR_HudDistanceM's idea, in pixels.
        if captured && live!(CyberpunkVR_OverlaySecondEye) != 0 {
            let mut shift_px = 0.0f32;
            let distance = live!(CyberpunkVR_OverlaySecondEyeDistM);
            if let Some(half_tan) = disparity_per_rad(c, distance) {
                shift_px = (c.ipd / distance) * ((0.5 * c.eye_w as f32) / half_tan);
            }
            // Adding to DisplayPos moves the overlay LEFT.
            if let Some(m) = asym_move(c) {
                shift_px -= m * 0.5 * c.eye_w as f32;
            }
            crate::imgui_overlay::cvr_overlay_record_into_target(c.list, slot_raw, shift_px);
        }
        // The lobby's curtain and intro picture for this eye, as MAIN's got them.
        if captured && c.intro {
            (c.intro_record)(c.ctx, slot_raw, src_raw);
        }
        captured
    }
}

// ---- MAIN's picture, the window's, and the second eye's ------------------------------------------------------------------

/// What the capture's pictures need: the second eye's record (its callbacks serve the other branches too), the
/// presented backbuffer and the snapshot slot written, the frame's shape, and the C++ half's objects as callbacks.
#[repr(C)]
pub struct CapturePicturesIn {
    pub second: SecondEyeIn,
    /// The presented backbuffer (in PRESENT) and MAIN's snapshot slot (its write lease taken).
    pub back_buffer: *mut c_void,
    pub snapshot: *mut c_void,
    pub source_width: u64,
    pub source_height: u32,
    /// The backbuffer's DXGI_FORMAT.
    pub source_format: i32,
    /// The eye MAIN's picture is (AFW turns it over every frame); AFW's drawn eye, -1 without AFW.
    pub main_eye: u32,
    pub afw_eye: i32,
    /// A frame was published before. Unused for the barrier since main's 8255f379b: every slot starts and ends in
    /// COPY_SOURCE, including its first use and its reuse after the public serial was reset.
    pub previous_serial: bool,
    /// ENGINE STEREO's shapes (Stereo/EngineStereo.hpp): the backbuffer holds both eyes side by side (split), or a menu
    /// drawn once over the whole window is squeezed into one eye's picture.
    pub squeeze: bool,
    /// A loading screen hung in the void before the player (Render/LoadingScreen.hpp), and whether this frame's window
    /// is copied for it (not when the world is drawn under the screen).
    pub loading_in_space: bool,
    pub loading_copy: bool,
    pub engine_stereo: bool,
    pub split: bool,
    pub menu_open: bool,
    /// The boot's notice is up (Lobby/Intro.hpp).
    pub notice_shown: bool,
    /// CyberpunkVR_StereoSubmit, CyberpunkVR_MonoMenu.
    pub stereo_submit: bool,
    pub mono_menu: bool,
    pub poses: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    /// The loading screen's picture: the window's 16:9 part, and its aspect.
    pub loading_uv: [f32; 4],
    pub loading_aspect: f32,
    /// The loading screen begun -- the boot's notice at the intro's heading, or the loading screen before the head --
    /// and held when this frame's window is its picture.
    pub loading_begin: extern "C" fn(ctx: *mut c_void),
    /// The loading screen recorded into `target` (viewed as `view`) through `pose` and `fov`, the window its picture.
    pub loading_record: extern "C" fn(
        ctx: *mut c_void,
        target: *mut c_void,
        view: i32,
        rect: *const RECT,
        pose: *const xr::Posef,
        fov: *const xr::Fovf,
        window: *mut c_void,
        window_view: i32,
        uv: *const f32,
        aspect: f32,
    ) -> bool,
    /// The game's ID3D12CommandQueue* the capture list goes to.
    pub queue: *mut c_void,
    /// AFW's label of the presented picture: its pose id, tracking origin and render frame (for the frame generator's
    /// inputs, its depth and motion).
    pub afw_pose_id: u64,
    pub afw_origin: u64,
    pub afw_render_frame_id: u32,
    /// GetDesiredHalfIpd(): MAIN was put half the separation off the head centre (Hooks/PatchCamera.cpp).
    pub half_ipd: f32,
    pub afw_has_render_frame_id: bool,
    /// cvr::framegen::CaptureInputsEnabled().
    pub keep_inputs: bool,
}
const _: () = assert!(
    core::mem::size_of::<CapturePicturesIn>() == 320
        && core::mem::offset_of!(CapturePicturesIn, poses) == 156
        && core::mem::offset_of!(CapturePicturesIn, loading_begin) == 264
        && core::mem::offset_of!(CapturePicturesIn, queue) == 280
);

/// The pictures: the second eye's source (VRCAM's, for the frame's labels) and how its capture went, whether MAIN's eye
/// got the loading screen, whether the second eye is AFW's made one.
#[repr(C)]
pub struct CapturePicturesOut {
    pub source: *mut c_void,
    pub status: i32,
    pub loading_window: bool,
    pub afw_warped: bool,
}

/// cvr::gpu::profile::Stage::CaptureMain.
const STAGE_CAPTURE_MAIN: u32 = 0;

/// One eye's half of a side-by-side backbuffer into an eye-sized image (engine stereo).
fn copy_eye_half(
    list: &ID3D12GraphicsCommandList,
    dst: &ID3D12Resource,
    src: &ID3D12Resource,
    x: u32,
    width: u32,
    height: u32,
) {
    let location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
        // SAFETY: a borrowed reference for the copy's recording, not released by it.
        pResource: unsafe { core::mem::transmute_copy(r) },
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let (to, from) = (location(dst), location(src));
    let region = D3D12_BOX {
        left: x,
        top: 0,
        front: 0,
        right: x + width,
        bottom: height,
        back: 1,
    };
    // SAFETY: a recording list and live resources.
    unsafe { list.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&region)) };
}

/// The capture's pictures, on its list after the lobby's intro went into the backbuffer: MAIN's eye copied into its
/// snapshot slot -- the menu's window squeezed into it, or the loading screen hung in the dark, or the backbuffer's half
/// (engine stereo) or the whole of it; then the boot's notice made the window's own picture once the eyes have it; then
/// the second eye: the loading screen seen through that eye, the backbuffer's other half (engine stereo, copied as the
/// engine drew it -- HUD and scanner per eye), AFW's made eye, or VRCAM's view (cvr_xr_capture_second_eye).
///
/// # Safety
/// `input` and `out` are valid; the objects are the capture's live ones.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_pictures(
    input: *const CapturePicturesIn,
    out: *mut CapturePicturesOut,
) {
    let (Some(c), Some(out)) = (unsafe { input.as_ref() }, unsafe { out.as_mut() }) else {
        return;
    };
    out.source = core::ptr::null_mut();
    out.status = SECOND_EYE_NONE;
    out.loading_window = false;
    out.afw_warped = false;
    let s = &c.second;
    let ctx = s.ctx;
    // SAFETY: the capture's objects, borrowed.
    let (Some(list), Some(bb), Some(snapshot)) = (
        unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&s.list) },
        unsafe { ID3D12Resource::from_raw_borrowed(&c.back_buffer) },
        unsafe { ID3D12Resource::from_raw_borrowed(&c.snapshot) },
    ) else {
        return;
    };
    let (eye_w, eye_h) = (s.eye_w, s.eye_h);
    let main = c.main_eye as usize & 1;
    // Where an eye's half starts in the backbuffer.
    let half_x = |eye: usize| {
        if eye == 0 {
            0
        } else {
            (c.source_width - u64::from(eye_w)) as u32
        }
    };
    let source_format = DXGI_FORMAT(c.source_format);
    let uv = c.loading_uv.as_ptr();

    // ---- MAIN's eye.
    let mut window: *mut c_void = core::ptr::null_mut();
    {
        let _scope = Scope::new(s.profile, STAGE_CAPTURE_MAIN);
        let barriers = vec![
            transition(bb, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition(snapshot, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST),
        ];
        // SAFETY: a recording list.
        unsafe { list.ResourceBarrier(&barriers) };
        let mut squeezed = false;
        if c.squeeze || c.loading_in_space {
            // The whole window for a menu under engine stereo, and the blit that squeezes it into one eye's snapshot.
            let mut blit: *const c_void = core::ptr::null();
            // SAFETY: the capture's device, borrowed.
            if let Some(device) = unsafe {
                windows::Win32::Graphics::Direct3D12::ID3D12Device::from_raw_borrowed(&s.device)
            } {
                if let Some((w, b)) = crate::xr_capture_state::menu_window(
                    device,
                    c.source_width,
                    c.source_height,
                    DXGI_FORMAT(c.source_format),
                ) {
                    window = w;
                    blit = b;
                }
            }
            // SAFETY: the manager's window, borrowed.
            if let Some(win) = unsafe { ID3D12Resource::from_raw_borrowed(&window) } {
                if !c.loading_in_space || c.loading_copy {
                    // SAFETY: a recording list and live resources.
                    unsafe { list.CopyResource(win, bb) };
                }
                let there = [
                    transition(
                        win,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    ),
                    transition(
                        snapshot,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    ),
                ];
                // SAFETY: a recording list.
                unsafe { list.ResourceBarrier(&there) };
                if c.loading_in_space {
                    (c.loading_begin)(ctx);
                    let rect = RECT {
                        left: 0,
                        top: 0,
                        right: eye_w as i32,
                        bottom: eye_h as i32,
                    };
                    squeezed = (c.loading_record)(
                        ctx,
                        c.snapshot,
                        c.source_format,
                        &rect,
                        &c.poses[main],
                        &c.fovs[main],
                        window,
                        c.source_format,
                        uv,
                        c.loading_aspect,
                    );
                    out.loading_window = squeezed;
                } else {
                    // SAFETY: the menu's blit, the capture's list and live resources.
                    squeezed = unsafe {
                        cvr_gpu::color_blit::cvr_gpu_color_blit_record_blit(
                            blit.cast(),
                            s.list,
                            window,
                            c.snapshot,
                            0.0,
                            0.0,
                            1.0,
                            1.0,
                        )
                    };
                }
                let back = [
                    transition(
                        win,
                        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                    ),
                    transition(
                        snapshot,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                    ),
                ];
                // SAFETY: a recording list.
                unsafe { list.ResourceBarrier(&back) };
            }
        }
        if !squeezed {
            if c.engine_stereo {
                copy_eye_half(list, snapshot, bb, half_x(main), eye_w, eye_h);
            } else {
                // SAFETY: a recording list and live resources.
                unsafe { list.CopyResource(snapshot, bb) };
            }
        }
        let after = [
            transition(
                snapshot,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            ),
            transition(
                bb,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_PRESENT,
            ),
        ];
        // SAFETY: a recording list.
        unsafe { list.ResourceBarrier(&after) };
    }

    // ---- THE MONITOR SHOWS THE NOTICE AS THE EYES DO (Lobby/Intro.hpp): once the eyes have theirs, the window itself
    // is made the dark with the notice hung in it -- each eye's half through that eye when the engine draws both, else
    // the whole window through MAIN's eye, its view widened to the window's shape -- instead of the boot's flat screen.
    if out.loading_window && c.notice_shown {
        barrier(
            list,
            bb,
            D3D12_RESOURCE_STATE_PRESENT,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
        );
        let height = c.source_height as i32;
        if c.split {
            for eye in 0..2 {
                let x = half_x(eye) as i32;
                let rect = RECT {
                    left: x,
                    top: 0,
                    right: x + eye_w as i32,
                    bottom: height,
                };
                (c.loading_record)(
                    ctx,
                    c.back_buffer,
                    c.source_format,
                    &rect,
                    &c.poses[eye],
                    &c.fovs[eye],
                    window,
                    c.source_format,
                    uv,
                    c.loading_aspect,
                );
            }
        } else {
            let mut wide = c.fovs[main];
            let (up, down) = (wide.angle_up.tan(), wide.angle_down.tan());
            let half = 0.5 * (up - down) * c.source_width as f32 / c.source_height.max(1) as f32;
            wide.angle_left = (-half).atan();
            wide.angle_right = half.atan();
            let rect = RECT {
                left: 0,
                top: 0,
                right: c.source_width as i32,
                bottom: height,
            };
            (c.loading_record)(
                ctx,
                c.back_buffer,
                c.source_format,
                &rect,
                &c.poses[main],
                &wide,
                window,
                c.source_format,
                uv,
                c.loading_aspect,
            );
        }
        barrier(
            list,
            bb,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_PRESENT,
        );
    }

    // ---- the second eye. NOT IN A MENU (CyberpunkVR_MonoMenu): VRCAM's view is of the WORLD, the menu is one mono
    // surface in the backbuffer; the eyes disagreeing that completely, the scene is what reads.
    let rgba8 = copy_family(source_format) == 1;
    let sized = eye_w != 0 && eye_h != 0;
    let mut captured = false;
    if out.loading_window {
        // The loading screen for the other eye: the same window, hung in the same place, seen through that eye. Its slot
        // is written through the 8-bit view the window was copied in (the frame's encoding, as MAIN's eye).
        if c.stereo_submit && rgba8 && sized && (s.ensure_slots)(ctx, eye_w, eye_h) {
            let slot_raw = (s.take_slot)(ctx);
            // SAFETY: the pool's slot and the manager's window, borrowed.
            let (Some(slot), Some(win)) = (
                unsafe { ID3D12Resource::from_raw_borrowed(&slot_raw) },
                unsafe { ID3D12Resource::from_raw_borrowed(&window) },
            ) else {
                out.status = SECOND_EYE_NO_SLOT;
                return;
            };
            let eye = main ^ 1;
            let there = [
                transition(
                    win,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                ),
                transition(
                    slot,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                ),
            ];
            // SAFETY: a recording list.
            unsafe { list.ResourceBarrier(&there) };
            let rect = RECT {
                left: 0,
                top: 0,
                right: eye_w as i32,
                bottom: eye_h as i32,
            };
            captured = (c.loading_record)(
                ctx,
                slot_raw,
                DXGI_FORMAT_R8G8B8A8_UNORM.0,
                &rect,
                &c.poses[eye],
                &c.fovs[eye],
                window,
                c.source_format,
                uv,
                c.loading_aspect,
            );
            let back = [
                transition(
                    win,
                    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                ),
                transition(
                    slot,
                    D3D12_RESOURCE_STATE_RENDER_TARGET,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ),
            ];
            // SAFETY: a recording list.
            unsafe { list.ResourceBarrier(&back) };
        }
    } else if c.split {
        // ENGINE STEREO: the second eye is the other half of the same backbuffer, drawn whole by the engine, copied as it
        // is. A plain copy needs the slot's format family, the R8G8B8A8 one the swapchains are picked for.
        if !rgba8 {
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Relaxed) {
                cvr_host::log::write(
                    &format(
                        c"[engine-stereo] backbuffer format %u is not R8G8B8A8: the second eye stays on MAIN's half\n",
                        &[Arg::U(c.source_format as u32 as u64)],
                    )
                    .to_string_lossy(),
                );
            }
        }
        if c.stereo_submit && rgba8 && sized && (s.ensure_slots)(ctx, eye_w, eye_h) {
            let slot_raw = (s.take_slot)(ctx);
            // SAFETY: the pool's slot, borrowed.
            let Some(slot) = (unsafe { ID3D12Resource::from_raw_borrowed(&slot_raw) }) else {
                out.status = SECOND_EYE_NO_SLOT;
                return;
            };
            let _scope = Scope::new(s.profile, STAGE_CAPTURE_SECOND);
            let there = [
                transition(
                    bb,
                    D3D12_RESOURCE_STATE_PRESENT,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ),
                transition(
                    slot,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                ),
            ];
            // SAFETY: a recording list.
            unsafe { list.ResourceBarrier(&there) };
            copy_eye_half(list, slot, bb, half_x(main ^ 1), eye_w, eye_h);
            let back = [
                transition(
                    bb,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                    D3D12_RESOURCE_STATE_PRESENT,
                ),
                transition(
                    slot,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                ),
            ];
            // SAFETY: a recording list.
            unsafe { list.ResourceBarrier(&back) };
            captured = true;
        }
    } else if c.afw_eye >= 0 && !c.menu_open && c.stereo_submit && sized {
        // AFW (Render/Afw.hpp): MAIN drew one eye; PureDark's frame warp makes the other one, for the same moment, out
        // of this picture, its depth and motion and the other eye's previous one, into the second eye's slot, labelled
        // and paired as the second view's picture is. Nothing is composited on it: the HUD and the rest drawn into the
        // backbuffer are in the drawn eye and come across with the warp. A plain copy into the plugin's R8G8B8A8_UNORM
        // buffers needs the backbuffer in that family.
        let inputs = if c.afw_has_render_frame_id && c.keep_inputs {
            crate::framegen_inputs::cvr_framegen_acquire_inputs(
                1,
                c.afw_render_frame_id,
                c.afw_pose_id,
                c.afw_origin,
                c.queue,
            )
        } else {
            core::ptr::null()
        };
        let mut status = SECOND_EYE_NONE;
        if rgba8 && !inputs.is_null() && (s.ensure_slots)(ctx, eye_w, eye_h) {
            let slot = (s.take_slot)(ctx);
            if slot.is_null() {
                status = SECOND_EYE_NO_SLOT;
            } else {
                let _scope = Scope::new(s.profile, STAGE_CAPTURE_SECOND);
                let fov = |f: &xr::Fovf| [f.angle_left, f.angle_right, f.angle_up, f.angle_down];
                let request = crate::afw::WarpRequest {
                    device: s.device,
                    queue: c.queue,
                    list: s.list,
                    drawn: c.back_buffer,
                    inputs,
                    eye: main as u32,
                    fov: [fov(&c.fovs[0]), fov(&c.fovs[1])],
                    // The other eye is the whole separation further along the camera's right.
                    ipd: 2.0 * c.half_ipd,
                    out: slot,
                };
                // SAFETY: the capture's objects and the frame generator's inputs, held.
                if unsafe { crate::afw::cvr_afw_warp(&request) } {
                    status = SECOND_EYE_CAPTURED;
                }
            }
        } else {
            let why = if !rgba8 {
                c"backbuffer not R8G8B8A8"
            } else if inputs.is_null() {
                c"no depth and motion for the frame"
            } else {
                c"no second-eye slot"
            };
            crate::afw::cvr_afw_fallback(why.as_ptr());
        }
        // SAFETY: the reference acquired above, released once.
        unsafe { crate::framegen_inputs::cvr_framegen_release_inputs(inputs) };
        if status < 0 {
            out.status = SECOND_EYE_NO_SLOT;
            return;
        }
        captured = status > 0;
        out.afw_warped = captured;
    } else if !c.engine_stereo && c.stereo_submit && sized && !(c.menu_open && c.mono_menu) {
        let mut second = SecondEyeOut {
            source: core::ptr::null_mut(),
            status: SECOND_EYE_NONE,
        };
        // SAFETY: the same record and objects.
        unsafe { cvr_xr_capture_second_eye(s, &mut second) };
        out.source = second.source;
        if second.status < 0 {
            out.status = SECOND_EYE_NO_SLOT;
            return;
        }
        captured = second.status > 0;
    }
    out.status = if captured {
        SECOND_EYE_CAPTURED
    } else {
        SECOND_EYE_NONE
    };
}

// ---- the capture's submission: the frame's labels and its publication ----------------------------------------------------

/// cvr::camera::PoseIdentity: the pose an image was rendered from.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PoseIdentity {
    pub pose_id: u64,
    pub write_id: u64,
    pub component: usize,
    pub view: u32,
    pub head: cvr_xr::tracking::HeadPose,
    pub local_head: xr::Posef,
    pub render_frame_id: u32,
    pub has_render_frame_id: bool,
    pub external_view: bool,
}
impl PoseIdentity {
    /// `operator bool`: an identity written from a valid head.
    pub(crate) fn present(&self) -> bool {
        self.pose_id != 0 && self.write_id != 0 && self.head.valid
    }
}
/// cvr::camera::ImagePoseIdentity: an image's label (rust/crates/cvr-rig/src/vrik/image_pose.rs).
pub type ImagePose = cvr_camera::image_ledger::Image<PoseIdentity>;
const _: () = assert!(
    core::mem::size_of::<PoseIdentity>() == 136
        && core::mem::size_of::<ImagePose>() == 168
        && core::mem::offset_of!(ImagePose, pose) == 32
);

extern "C" {
    fn cvr_image_pose_read(resource: *mut c_void, out: *mut ImagePose);
    static CyberpunkVR_PoseIdCaptured: [AtomicU64; 2];
    static CyberpunkVR_PoseIdCaptureMiss: [AtomicU64; 2];
    static CyberpunkVR_PoseIdLastCapture: [AtomicU64; 2];
    static CyberpunkVR_PoseIdImageGeneration: [AtomicU64; 2];
}
fn read_image(resource: *mut c_void) -> ImagePose {
    // SAFETY: all-zero is an empty label; the ledger fills it.
    let mut out: ImagePose = unsafe { core::mem::zeroed() };
    // SAFETY: a resource or null, and the out-parameter.
    unsafe { cvr_image_pose_read(resource, &mut out) };
    out
}

/// Camera/ImagePoseMath.hpp RebaseImageEyePose: the eye's offset from the head it was located with, carried onto the
/// head the image was rendered at.
fn rebase_image_eye_pose(
    previous_head: &xr::Posef,
    previous_eye: &xr::Posef,
    image_head: &xr::Posef,
) -> xr::Posef {
    use cvr_body::xr::{conjugate_quat, multiply_quat, rotate_vector, Quat, Vec3};
    let q = |o: xr::Quaternionf| Quat {
        x: o.x,
        y: o.y,
        z: o.z,
        w: o.w,
    };
    let inverse = conjugate_quat(q(previous_head.orientation));
    let delta = Vec3 {
        x: previous_eye.position.x - previous_head.position.x,
        y: previous_eye.position.y - previous_head.position.y,
        z: previous_eye.position.z - previous_head.position.z,
    };
    let local_orientation = multiply_quat(inverse, q(previous_eye.orientation));
    let local_position = rotate_vector(inverse, delta);
    let image = q(image_head.orientation);
    let offset = rotate_vector(image, local_position);
    let o = multiply_quat(image, local_orientation);
    xr::Posef {
        orientation: xr::Quaternionf {
            x: o.x,
            y: o.y,
            z: o.z,
            w: o.w,
        },
        position: xr::Vector3f {
            x: image_head.position.x + offset.x,
            y: image_head.position.y + offset.y,
            z: image_head.position.z + offset.z,
        },
    }
}

/// What the capture's submission needs: its list, queue and ring, the frame's images and shape, the poses it was located
/// with, and the image submission mutex (cvr::camera::ImageSubmissionMutex) and AFW's fallback as callbacks.
#[repr(C)]
pub struct CaptureSubmitIn {
    /// The capture's ID3D12GraphicsCommandList* (recording), the game's ID3D12CommandQueue*, the capture ring's
    /// ID3D12Fence*, its fence value and the current allocator's fence slot.
    pub list: *mut c_void,
    pub queue: *mut c_void,
    pub fence: *mut c_void,
    pub fence_value: *mut u64,
    pub allocator_fence: *mut u64,
    pub profile: *const c_void,
    /// The presented backbuffer, the second eye's source (VRCAM's, or null), the snapshot slot written.
    pub back_buffer: *mut c_void,
    pub second_source: *mut c_void,
    pub snapshot: *mut c_void,
    /// The capture's write leases (two entries).
    pub writes: *mut usize,
    pub ctx: *mut c_void,
    /// ID3D12Device*; the event the submit thread waits on for a new frame (m_monoPresentEvent), or null.
    pub device: *mut c_void,
    pub present_event: *mut c_void,
    pub serial: u64,
    /// The pose id AFW warped the second eye for; the tracking origin's serial now.
    pub afw_pose_id: u64,
    pub origin: u64,
    /// The poses and FOVs located for the frame, and the head centres they were located with.
    pub poses: [xr::Posef; 2],
    pub head_centers: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    pub main_eye: u32,
    /// GetVrSharpmix() (0: no CAS) and GetVrSharpness().
    pub cas_amount: f32,
    pub sharpness: f32,
    pub has_view: [bool; 2],
    /// The second eye's slot holds this frame's second eye; it is AFW's made eye.
    pub second_eye: bool,
    pub afw_warped: bool,
    pub split: bool,
    /// A menu's whole window squeezed into one eye (the monitor shows it as it is).
    pub squeeze: bool,
    pub engine_stereo: bool,
    /// MAIN's eye got the loading screen (drawn through the located poses, submitted with them).
    pub loading_window: bool,
    /// Presented under a loading screen; the boot's notice is up.
    pub loading_dark: bool,
    pub notice_shown: bool,
    /// CyberpunkVR_BindPoseToImage, no external pose reset pending: the images' labels are used.
    pub bind: bool,
    /// cvr::framegen::CaptureInputsEnabled().
    pub keep_inputs: bool,
    pub lock: extern "C" fn(ctx: *mut c_void),
    pub unlock: extern "C" fn(ctx: *mut c_void),
    /// cvr::afw::Fallback(reason).
    pub afw_fallback: extern "C" fn(ctx: *mut c_void, reason: *const core::ffi::c_char),
}
const _: () = assert!(
    core::mem::size_of::<CaptureSubmitIn>() == 320
        && core::mem::offset_of!(CaptureSubmitIn, poses) == 128
        && core::mem::offset_of!(CaptureSubmitIn, has_view) == 284
        && core::mem::offset_of!(CaptureSubmitIn, lock) == 296
);

/// The capture submitted and published: the list closed; under the image submission mutex -- labels are selected at the
/// actual queue submission, not when a producer merely recorded commands, the same-queue copy sitting between the
/// labelled writes and any later replacement of the source images -- each eye's pose taken from its image's label (the
/// label's head with the located eye offset, when the label belongs to this queue and tracking origin; MAIN's label for
/// both halves under engine stereo; AFW's made eye only while MAIN's label is the one it was warped with), the frame
/// generator's inputs for the eye, the list executed and the ring's fence signalled; then the frame published
/// (cvr_xr_capture_publish).
///
/// # Safety
/// `input` is valid; its objects are the capture's live ones.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_submit(input: *const CaptureSubmitIn) {
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12CommandList, ID3D12CommandQueue, ID3D12Fence,
    };
    let Some(c) = (unsafe { input.as_ref() }) else {
        return;
    };
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let (Some(list), Some(queue), Some(fence)) = (
            ID3D12GraphicsCommandList::from_raw_borrowed(&c.list),
            ID3D12CommandQueue::from_raw_borrowed(&c.queue),
            ID3D12Fence::from_raw_borrowed(&c.fence),
        ) else {
            return;
        };
        // CAS: each owned real capture filtered once. Framegen and every later repeat see the same filtered endpoint;
        // generated images never run through CAS again.
        if c.cas_amount > 0.0 {
            let pass = crate::xr_capture_state::sharpen_pass().cast();
            let eye_slot = if c.second_eye {
                crate::xr_capture_state::cvr_xr_capture_eye_slot()
            } else {
                core::ptr::null_mut()
            };
            for color in [c.snapshot, eye_slot] {
                let Some(resource) = ID3D12Resource::from_raw_borrowed(&color) else {
                    continue;
                };
                let desc = resource.GetDesc();
                if cvr_gpu::sharpen_pass::cvr_gpu_sharpen_pass_ensure(
                    pass,
                    c.device,
                    desc.Format.0,
                    desc.Width as u32,
                    desc.Height,
                ) {
                    cvr_gpu::sharpen_pass::cvr_gpu_sharpen_pass_record(
                        pass,
                        c.list,
                        color,
                        color,
                        c.sharpness,
                        c.cas_amount,
                    );
                }
            }
        }
        let mirror_eye = crate::xr_capture::monitor_eye();
        if mirror_eye.is_none() {
            let _ = list.Close();
        }
        let mut poses = c.poses;
        let mut pose_ids = [0u64; 2];
        let mut generations = [0u64; 2];
        let mut inputs: [*const crate::framegen_inputs::Inputs; 2] = [core::ptr::null(); 2];
        let mut second_eye = c.second_eye;
        let mut afw_warped = c.afw_warped;
        (c.lock)(c.ctx);
        let main_image = read_image(c.back_buffer);
        let second_image = read_image(c.second_source);
        let second = (c.main_eye as usize & 1) ^ 1;
        if afw_warped && !(main_image.valid() && main_image.pose.pose_id == c.afw_pose_id) {
            afw_warped = false;
            second_eye = false;
            (c.afw_fallback)(
                c.ctx,
                c"the frame's label changed before its submission".as_ptr(),
            );
        }
        if let Some(eye) = mirror_eye {
            // Both XR colours have finished composition and sharpening: read them on their producer list, before its
            // fence is published to either submit host. A flat menu already fills the desktop at its intended aspect,
            // a screen placed in space is eye-specific.
            let handled = (c.squeeze && !c.loading_window) || {
                let (Some(device), Some(back)) = (
                    windows::Win32::Graphics::Direct3D12::ID3D12Device::from_raw_borrowed(&c.device),
                    ID3D12Resource::from_raw_borrowed(&c.back_buffer),
                ) else {
                    (c.unlock)(c.ctx);
                    let _ = list.Close();
                    return;
                };
                let second = if second_eye { crate::xr_capture_state::cvr_xr_capture_eye_slot() } else { core::ptr::null_mut() };
                let mut heap = DESKTOP_EYE_CLEAR_HEAP.lock().unwrap_or_else(|e| e.into_inner());
                cvr_gpu::desktop_mirror::record_captured_eye(
                    device,
                    &mut heap,
                    list,
                    back,
                    eye,
                    c.main_eye,
                    ID3D12Resource::from_raw_borrowed(&c.snapshot),
                    ID3D12Resource::from_raw_borrowed(&second),
                    c.split,
                )
            };
            DESKTOP_EYE_HANDLED.store(handled, core::sync::atomic::Ordering::Relaxed);
            if list.Close().is_err() {
                DESKTOP_EYE_HANDLED.store(false, core::sync::atomic::Ordering::Relaxed);
                (c.unlock)(c.ctx);
                return;
            }
        }
        let labelled = |source: &ImagePose, view: u32| {
            c.bind
                && source.valid()
                && source.pose.present()
                && source.pose.view == view
                && source.queue == c.queue as usize
                && source.pose.head.origin_serial == c.origin
        };
        let diag = diagnostics();
        for eye in 0..2 {
            let separate = second_eye && eye == second && !c.split && !afw_warped;
            let source = if separate { &second_image } else { &main_image };
            let view: u32 = if separate { 2 } else { 1 };
            // Framegen's inputs for this eye: under engine stereo its half's, none for a menu's squeezed window.
            let input_view = if c.split {
                eye as u32 + 1
            } else if c.engine_stereo {
                0
            } else {
                view
            };
            let v = view as usize - 1;
            if !c.loading_window && labelled(source, view) {
                poses[eye] = rebase_image_eye_pose(
                    &c.head_centers[eye],
                    &c.poses[eye],
                    &source.pose.local_head,
                );
                pose_ids[eye] = source.pose.pose_id;
                generations[eye] = source.generation;
                if input_view != 0 && c.keep_inputs && source.pose.has_render_frame_id {
                    inputs[eye] = crate::framegen_inputs::cvr_framegen_acquire_inputs(
                        input_view,
                        source.pose.render_frame_id,
                        source.pose.pose_id,
                        source.pose.head.origin_serial,
                        c.queue,
                    );
                }
                if diag {
                    CyberpunkVR_PoseIdCaptured[v].fetch_add(1, Relaxed);
                    CyberpunkVR_PoseIdLastCapture[eye].store(source.pose.pose_id, Relaxed);
                    CyberpunkVR_PoseIdImageGeneration[eye].store(source.generation, Relaxed);
                }
            } else if diag {
                CyberpunkVR_PoseIdCaptureMiss[v].fetch_add(1, Relaxed);
                CyberpunkVR_PoseIdLastCapture[eye].store(0, Relaxed);
                CyberpunkVR_PoseIdImageGeneration[eye].store(0, Relaxed);
            }
        }
        // The runtime's own queue may still be reading what this capture writes (an image whose read is retired but not
        // finished): the game's queue waits for those reads on the GPU. They are short copies submitted a display period
        // or more ago, so this rarely waits at all (main's 0.2.4).
        if cvr_xr::runtime_queue::has_own() {
            // SAFETY: the caller's write list (two entries).
            let writes = &*c.writes.cast::<[usize; 2]>();
            let pending = crate::xr_capture_state::retired_reads_of(writes);
            if pending != 0 {
                let submit = crate::xr_capture_state::SUBMIT_FENCE.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if let Some(submit) = submit {
                    if submit.GetCompletedValue() < pending {
                        let _ = queue.Wait(&submit, pending);
                    }
                }
            }
        }
        let lists = [Some(
            list.cast::<ID3D12CommandList>()
                .expect("a graphics list is a command list"),
        )];
        queue.ExecuteCommandLists(&lists);
        *c.fence_value += 1;
        let value = *c.fence_value;
        *c.allocator_fence = value;
        if queue.Signal(fence, value).is_ok() {
            cvr_gpu::stage_profile::cvr_gpu_stage_submitted(
                c.profile.cast(),
                c.queue,
                c.fence,
                value,
            );
        }
        (c.unlock)(c.ctx);

        let frame = crate::xr_capture_state::Publish {
            snapshot: c.snapshot,
            serial: c.serial,
            poses,
            fovs: c.fovs,
            has_view: c.has_view,
            // A frame hung with a screen in the dark is dark already, with the screen in it.
            dark: c.loading_dark && !c.loading_window,
            // The boot's notice in the dark: both eyes are submitted with the turn it was drawn for.
            one_turn: c.loading_window && c.notice_shown,
            main_eye: c.main_eye,
            pose_ids,
            generations,
            inputs,
            second_eye,
        };
        crate::xr_capture_state::publish_fenced(&frame, c.writes, Some(fence), value);
        if !c.present_event.is_null() {
            windows_sys::Win32::System::Threading::SetEvent(c.present_event);
        }
        if cvr_gpu::framegen_core::cvr_framegen_metrics_enabled() {
            cvr_gpu::framegen_core::cvr_framegen_on_real_frame(
                c.serial,
                cvr_gpu::framegen_core::cvr_framegen_now_ms(),
            );
        }
    }
}

// ---- the whole capture ---------------------------------------------------------------------------------------------------

/// What the C++ half hands the capture of one presented frame: the backbuffer and its description, the write leases,
/// the frame's present serial and located views, and its objects -- the capture resources,
/// the loading screen, the HUD's parameters, the second eye's FOV and the asymmetric windows -- as callbacks.
#[repr(C)]
pub struct CaptureFrameHostIn {
    pub back_buffer: *mut c_void,
    pub source_desc: *const D3D12_RESOURCE_DESC,
    pub writes: *mut usize,
    pub ctx: *mut c_void,
    pub serial: u64,
    pub poses: [xr::Posef; 2],
    pub head_centers: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    pub has_view: [bool; 2],
    pub ensure_resources: extern "C" fn(ctx: *mut c_void, eye: *const D3D12_RESOURCE_DESC) -> bool,
    pub loading_end: extern "C" fn(ctx: *mut c_void),
    pub loading_begin: extern "C" fn(ctx: *mut c_void, notice: bool, opacity: f32, hold: bool, head: *const xr::Posef),
    pub loading_record: extern "C" fn(
        ctx: *mut c_void,
        target: *mut c_void,
        view: i32,
        rect: *const RECT,
        pose: *const xr::Posef,
        fov: *const xr::Fovf,
        window: *mut c_void,
        window_view: i32,
        uv: *const f32,
        aspect: f32,
    ) -> bool,
    pub hud_params: extern "C" fn(ctx: *mut c_void, out: *mut [f32; HUD_PARAMS]),
    pub eye1_fov: extern "C" fn(ctx: *mut c_void, out: *mut xr::Fovf) -> bool,
    pub asym_windows: extern "C" fn(ctx: *mut c_void, out: *mut [EyeWindow; 2]) -> bool,
}
const _: () = assert!(
    core::mem::size_of::<CaptureFrameHostIn>() == 248
        && core::mem::offset_of!(CaptureFrameHostIn, poses) == 40
        && core::mem::offset_of!(CaptureFrameHostIn, has_view) == 184
        && core::mem::offset_of!(CaptureFrameHostIn, ensure_resources) == 192
);

/// What the capture of one presented frame reads: the host's part, and what the Rust half reads itself when the
/// capture starts -- the game's device and queue and the mono frame's event (crate::submit_state), the tracking origin's serial, the runtime's
/// IPD, the desired half IPD, the CAS and sharpness, the second eye swapchain's width, the menu, the eye and
/// submission switches, the pose binding (off while an external pose reset is pending), whether the frame
/// generator keeps the capture's inputs, and the asymmetric projection's switch.
pub struct CaptureFrameIn {
    pub device: *mut c_void,
    pub queue: *mut c_void,
    pub back_buffer: *mut c_void,
    /// The backbuffer's description.
    pub source_desc: *const D3D12_RESOURCE_DESC,
    /// The capture's write leases (two entries; the C++ half ends them when the capture stops early).
    pub writes: *mut usize,
    /// The event the submit thread waits on for a new frame (m_monoPresentEvent), or null.
    pub present_event: *mut c_void,
    pub ctx: *mut c_void,
    /// The present's serial; the tracking origin's serial now.
    pub serial: u64,
    pub origin: u64,
    /// The views located for the frame, and the head centres they were located with.
    pub poses: [xr::Posef; 2],
    pub head_centers: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    /// The runtime's IPD; GetDesiredHalfIpd(); GetVrSharpmix(), GetVrSharpness(); m_eyeSwapchains[1].width (0 none).
    pub ipd: f32,
    pub half_ipd: f32,
    pub cas_amount: f32,
    pub sharpness: f32,
    pub eye1_width: i32,
    pub has_view: [bool; 2],
    /// GetMenuRectMode() || GetMenuMode(): a menu is open.
    pub menu_mode: bool,
    /// CyberpunkVR_MainIsRightEye, CyberpunkVR_StereoSubmit, CyberpunkVR_MonoMenu; CyberpunkVR_BindPoseToImage with no
    /// external pose reset pending; cvr::framegen::CaptureInputsEnabled(); cvr::projection::Active().
    pub main_is_right: bool,
    pub stereo_submit: bool,
    pub mono_menu: bool,
    pub bind: bool,
    pub keep_inputs: bool,
    pub asym_active: bool,
    /// EnsureMonoCaptureResource(eye description): the ring and the snapshot pool.
    pub ensure_resources: extern "C" fn(ctx: *mut c_void, eye: *const D3D12_RESOURCE_DESC) -> bool,
    /// The loading screen (Render/LoadingScreen.hpp, the C++ half's face): ended; begun -- the boot's notice with its
    /// opacity, or the loading screen -- before `head`, and held when `hold`; recorded on the capture list.
    pub loading_end: extern "C" fn(ctx: *mut c_void),
    pub loading_begin: extern "C" fn(
        ctx: *mut c_void,
        notice: bool,
        opacity: f32,
        hold: bool,
        head: *const xr::Posef,
    ),
    pub loading_record: extern "C" fn(
        ctx: *mut c_void,
        target: *mut c_void,
        view: i32,
        rect: *const RECT,
        pose: *const xr::Posef,
        fov: *const xr::Fovf,
        window: *mut c_void,
        window_view: i32,
        uv: *const f32,
        aspect: f32,
    ) -> bool,
    /// cvr::CyberpunkVR_GetHudParams(), GetCurrentEyeFov(1), GetAsymmetricEyeWindows (as SecondEyeIn's).
    pub hud_params: extern "C" fn(ctx: *mut c_void, out: *mut [f32; HUD_PARAMS]),
    pub eye1_fov: extern "C" fn(ctx: *mut c_void, out: *mut xr::Fovf) -> bool,
    pub asym_windows: extern "C" fn(ctx: *mut c_void, out: *mut [EyeWindow; 2]) -> bool,
}

extern "C" {
    fn GetMenuRectMode() -> i32;
    fn GetMenuMode() -> i32;
    fn cvr_camera_desired_half_ipd() -> f32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_StereoSubmit: i32;
    static CyberpunkVR_MonoMenu: i32;
    static CyberpunkVR_BindPoseToImage: i32;
    static CyberpunkVR_AsymProjection: i32;
}

impl CaptureFrameIn {
    /// # Safety
    /// The host's input is the C++ half's live one.
    unsafe fn gather(h: &CaptureFrameHostIn) -> CaptureFrameIn {
        let submission = &cvr_xr::submit_state::SUBMIT;
        let read = |v: *const i32| {
            // SAFETY: an export, read as it changes.
            unsafe { core::ptr::read_volatile(v) }
        };
        // SAFETY (whole body): the submission's device and queue are set at InitGraphics; plain C getters.
        unsafe {
            let device = *submission.device.get();
            let queue = *submission.queue.get();
            let origin = cvr_xr::tracking::origin_serial();
            let ipd = cvr_xr::runtime_info::RUNTIME.ipd.load();
            let half_ipd = cvr_camera_desired_half_ipd();
            let cas_amount = crate::vr_core_settings::GetVrSharpmix();
            let sharpness = crate::vr_core_settings::GetVrSharpness();
            let eye1_width = crate::xr_submit::cvr_xr_submit_eye1_width();
            let menu_mode = GetMenuRectMode() != 0 || GetMenuMode() != 0;
            let bind = cvr_xr::published::bind_pose_to_image(read(&raw const CyberpunkVR_BindPoseToImage), || {
                cvr_xr::published::RESET.gate.pending()
            });
            CaptureFrameIn {
                device,
                queue,
                back_buffer: h.back_buffer,
                source_desc: h.source_desc,
                writes: h.writes,
                present_event: submission.mono_present_event(),
                ctx: h.ctx,
                serial: h.serial,
                origin,
                poses: h.poses,
                head_centers: h.head_centers,
                fovs: h.fovs,
                ipd,
                half_ipd,
                cas_amount,
                sharpness,
                eye1_width,
                has_view: h.has_view,
                menu_mode,
                main_is_right: read(&raw const CyberpunkVR_MainIsRightEye) != 0,
                stereo_submit: read(&raw const CyberpunkVR_StereoSubmit) != 0,
                mono_menu: read(&raw const CyberpunkVR_MonoMenu) != 0,
                bind,
                keep_inputs: cvr_gpu::framegen_core::cvr_framegen_capture_inputs_enabled(),
                asym_active: read(&raw const CyberpunkVR_AsymProjection) == 1,
                ensure_resources: h.ensure_resources,
                loading_end: h.loading_end,
                loading_begin: h.loading_begin,
                loading_record: h.loading_record,
                hud_params: h.hud_params,
                eye1_fov: h.eye1_fov,
                asym_windows: h.asym_windows,
            }
        }
    }
}

extern "C" {
    static CyberpunkVR_DebugCapSkipNoRes: AtomicU64;
    static CyberpunkVR_DebugCapSkipNoSlot: AtomicU64;
    fn cvr_engine_stereo_sdk_main_menus_up() -> i32;
}

/// The frame the pictures' callbacks reach.
struct FrameContext<'a> {
    c: &'a CaptureFrameIn,
    list: *mut c_void,
    allocator_index: u32,
    eye_w: u32,
    eye_h: u32,
    main: usize,
    intro_picture: bool,
    notice: crate::intro_boot::BootFrame,
    loading_copy: bool,
}
fn frame<'a>(ctx: *mut c_void) -> &'a FrameContext<'a> {
    // SAFETY: the context cvr_xr_capture_frame hands its pictures, alive for their call.
    unsafe { &*(ctx as *const FrameContext) }
}
extern "C" fn frame_ensure_slots(ctx: *mut c_void, width: u32, height: u32) -> bool {
    // SAFETY: the capture's device.
    unsafe {
        crate::xr_capture_state::cvr_xr_capture_ensure_eye_pool(
            frame(ctx).c.device,
            width,
            height,
            DXGI_FORMAT_R8G8B8A8_TYPELESS.0,
        )
    }
}
extern "C" fn frame_take_slot(ctx: *mut c_void) -> *mut c_void {
    // SAFETY: the capture's write leases.
    unsafe { crate::xr_capture_state::cvr_xr_capture_take_eye_slot(frame(ctx).c.writes) }
}
extern "C" fn frame_hud_params(ctx: *mut c_void, out: *mut [f32; HUD_PARAMS]) {
    let c = frame(ctx).c;
    (c.hud_params)(c.ctx, out)
}
extern "C" fn frame_eye1_fov(ctx: *mut c_void, out: *mut xr::Fovf) -> bool {
    let c = frame(ctx).c;
    (c.eye1_fov)(c.ctx, out)
}
extern "C" fn frame_asym_windows(ctx: *mut c_void, out: *mut [EyeWindow; 2]) -> bool {
    let c = frame(ctx).c;
    (c.asym_windows)(c.ctx, out)
}
/// The lobby's intro for the second eye, seen through its own picture's label.
extern "C" fn frame_intro_record(ctx: *mut c_void, slot: *mut c_void, source: *mut c_void) {
    let f = frame(ctx);
    let eye = f.main ^ 1;
    let seen = {
        let _lock = cvr_host::submission::lock();
        image_pose(f.c, eye, &read_image(source), 2)
    };
    let rect = RECT {
        left: 0,
        top: 0,
        right: f.eye_w as i32,
        bottom: f.eye_h as i32,
    };
    // SAFETY: the capture's device and list, the slot.
    unsafe {
        crate::lobby_intro::cvr_intro_overlay_record(
            f.c.device,
            f.list,
            slot,
            &rect,
            &seen,
            &f.c.fovs[eye],
            f.intro_picture,
            f.allocator_index,
        )
    };
}
extern "C" fn frame_loading_begin(ctx: *mut c_void) {
    let f = frame(ctx);
    (f.c.loading_begin)(
        f.c.ctx,
        f.notice.shown,
        f.notice.opacity,
        f.loading_copy,
        &f.c.head_centers[f.main],
    );
}
extern "C" fn frame_loading_record(
    ctx: *mut c_void,
    target: *mut c_void,
    view: i32,
    rect: *const RECT,
    pose: *const xr::Posef,
    fov: *const xr::Fovf,
    window: *mut c_void,
    window_view: i32,
    uv: *const f32,
    aspect: f32,
) -> bool {
    let c = frame(ctx).c;
    (c.loading_record)(
        c.ctx,
        target,
        view,
        rect,
        pose,
        fov,
        window,
        window_view,
        uv,
        aspect,
    )
}

/// The image's label belongs to this view, this queue and this tracking origin (and labels are used at all).
fn labelled(c: &CaptureFrameIn, source: &ImagePose, view: u32) -> bool {
    c.bind
        && source.valid()
        && source.pose.present()
        && source.pose.view == view
        && source.queue == c.queue as usize
        && source.pose.head.origin_serial == c.origin
}
/// The pose an eye's image was rendered at: its label's head with this frame's eye offset when labelled, else the pose
/// located for it.
fn image_pose(c: &CaptureFrameIn, eye: usize, source: &ImagePose, view: u32) -> xr::Posef {
    if labelled(c, source, view) {
        rebase_image_eye_pose(&c.head_centers[eye], &c.poses[eye], &source.pose.local_head)
    } else {
        c.poses[eye]
    }
}

/// The loading screen was the boot's notice last frame (m_loadingScreenIsNotice).
static LOADING_IS_NOTICE: AtomicBool = AtomicBool::new(false);

/// The capture recorded the monitor's eye this present (OpenXRManager::OnPresent's desktopEyeHandled): reset by the
/// present before the capture runs, read after it.
pub static DESKTOP_EYE_HANDLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// The capture's RTV for the monitor eye's margins (OpenXRManager's m_desktopEyeClearHeap).
static DESKTOP_EYE_CLEAR_HEAP: std::sync::Mutex<Option<windows::Win32::Graphics::Direct3D12::ID3D12DescriptorHeap>> =
    std::sync::Mutex::new(None);
/// The manager's shutdown lets the heap go.
pub fn release_desktop_eye_heap() {
    *DESKTOP_EYE_CLEAR_HEAP.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
/// The launcher's MONITOR OUTPUT: 0 the game's output, 1 the left eye, 2 the right eye (cvr_settings::launcher).
pub fn monitor_eye() -> Option<u32> {
    match cvr_settings::launcher::MONITOR_OUTPUT.load(core::sync::atomic::Ordering::Relaxed) {
        1 => Some(0),
        2 => Some(1),
        _ => None,
    }
}

/// cvr::engine_stereo::SharedMenuFrame: a flat menu, splash or loading screen shared by both views.
pub fn shared_menu_frame() -> bool {
    // SAFETY: plain getters.
    let menu = unsafe { GetMenuRectMode() != 0 || GetMenuMode() != 0 };
    let one_picture = crate::engine_stereo::cvr_engine_stereo_enabled()
        && (!crate::engine_stereo::cvr_engine_stereo_rendering() || crate::engine_stereo::cvr_engine_stereo_loading_shown());
    (menu || one_picture) && !crate::menu_lobby::cvr_menu_lobby_world_shown()
}
/// cvr::engine_stereo::LoadingSideBySide: the loading screen drawn into each half of a stereo frame.
pub fn loading_side_by_side() -> bool {
    crate::engine_stereo::cvr_engine_stereo_enabled()
        && crate::engine_stereo::cvr_engine_stereo_rendering()
        // SAFETY: a plain getter.
        && unsafe { cvr_engine_stereo_sdk_main_menus_up() } <= 0
        && crate::engine_stereo::cvr_engine_stereo_loading_shown()
}

/// THE CAPTURE OF ONE PRESENTED FRAME (CaptureMonoPresentedFrame, after the capture mutex is taken): the frame's shape --
/// a menu open, engine stereo split or squeezed, the loading screen in space, the boot's notice, AFW's drawn eye and its
/// label -- the capture's resources, the next snapshot slot, the ring's next allocator and its list, the GPU stage
/// profile, the lobby's curtain and intro into the backbuffer, MAIN's picture and the second eye's
/// (cvr_xr_capture_pictures), the legacy mono depth, the CAS, the submission and the publication
/// (cvr_xr_capture_submit). False when the frame was not captured.
///
/// # Safety
/// `input` is valid; its objects are the capture's live ones, its pointers OpenXRManager's members.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_capture_frame(input: *const CaptureFrameHostIn) -> bool {
    use windows::Win32::Graphics::Direct3D12::{ID3D12CommandAllocator, ID3D12PipelineState};
    let Some(h) = (unsafe { input.as_ref() }) else {
        return false;
    };
    // SAFETY: the C++ half's live input.
    let gathered = unsafe { CaptureFrameIn::gather(h) };
    let c = &gathered;
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let Some(source) = c.source_desc.as_ref() else {
            return false;
        };
        let one_picture = || {
            crate::engine_stereo::cvr_engine_stereo_enabled()
                && (!crate::engine_stereo::cvr_engine_stereo_rendering()
                    || crate::engine_stereo::cvr_engine_stereo_loading_shown())
        };
        // ENGINE STEREO (Stereo/EngineStereo.hpp): the backbuffer holds both eyes, MAIN's eye takes its half into the
        // snapshot, the other its half into the second eye's slot. A MENU is not split, nor the loading screen (ink
        // draws it once over the whole window), nor a frame the engine rendered mono: both eyes then get the whole
        // window squeezed into the snapshot. The loading screen over a frame rendered in stereo is drawn into each half
        // already squeezed (LoadingSideBySide). The main menu shown in its world is split as a scene is.
        let menu_open =
            (c.menu_mode || one_picture()) && !crate::menu_lobby::cvr_menu_lobby_world_shown();
        let loading_dark = crate::menu_lobby::cvr_menu_lobby_loading_dark();
        let engine_stereo = crate::engine_stereo::cvr_engine_stereo_enabled() && source.Width >= 2;
        let split = engine_stereo && !menu_open;
        let side_by_side = crate::engine_stereo::cvr_engine_stereo_enabled()
            && crate::engine_stereo::cvr_engine_stereo_rendering()
            && cvr_engine_stereo_sdk_main_menus_up() <= 0
            && crate::engine_stereo::cvr_engine_stereo_loading_shown();
        let squeeze = engine_stereo && menu_open && !side_by_side;
        // A loading screen outside the main menu is hung in the void before the player (Render/LoadingScreen.hpp); so
        // is the boot's screen, the intro's notice, until the intro takes it over.
        let mut notice = crate::intro_boot::BootFrame {
            shown: false,
            live: false,
            opacity: 0.0,
        };
        crate::intro_boot::cvr_intro_boot_frame(&mut notice);
        let loading_in_space =
            notice.shown || (engine_stereo && crate::menu_lobby::cvr_menu_lobby_loading_in_space());
        if !loading_in_space || notice.shown != LOADING_IS_NOTICE.load(Relaxed) {
            (c.loading_end)(c.ctx);
        }
        LOADING_IS_NOTICE.store(notice.shown, Relaxed);
        let mut eye_desc = *source;
        if engine_stereo {
            eye_desc.Width = source.Width / 2;
        }
        // AFW (Render/Afw.hpp): MAIN drew the eye its picture's label says. The label is read again at the submission; a
        // frame whose label changed in between is not paired.
        let mut afw_eye = -1i32;
        let mut afw_image: ImagePose = core::mem::zeroed();
        if crate::afw::cvr_afw_active() && !engine_stereo {
            let _lock = cvr_host::submission::lock();
            afw_image = read_image(c.back_buffer);
            let p = &afw_image.pose;
            if afw_image.valid()
                && p.present()
                && p.view == 1
                && (0..=1).contains(&p.head.afw_eye)
                && afw_image.queue == c.queue as usize
                && p.head.origin_serial == c.origin
            {
                afw_eye = i32::from(p.head.afw_eye);
            }
        }
        let afw_pose_id = if afw_eye >= 0 {
            afw_image.pose.pose_id
        } else {
            0
        };
        let main = if afw_eye >= 0 {
            afw_eye as usize
        } else {
            usize::from(c.main_is_right)
        };
        // The loading screen's picture: the window's 16:9 part fitted by the height.
        let mut loading_uv = [0.0f32, 0.0, 1.0, 1.0];
        let mut loading_aspect = 16.0f32 / 9.0;
        let loading_copy = if notice.shown {
            notice.live
        } else {
            loading_in_space && !crate::engine_stereo::cvr_engine_stereo_world_drawn()
        };
        if loading_in_space {
            let (window_w, window_h) = (source.Width as f32, source.Height as f32);
            let design_w = window_w.min(window_h * 16.0 / 9.0);
            loading_aspect = design_w / window_h;
            loading_uv[0] = 0.5 * (1.0 - design_w / window_w);
            loading_uv[2] = 1.0 - loading_uv[0];
        }
        if !(c.ensure_resources)(c.ctx, &eye_desc) {
            crate::menu_lobby::trace_boot_skip("no resources");
            if diagnostics() {
                CyberpunkVR_DebugCapSkipNoRes.fetch_add(1, Relaxed);
            }
            return false;
        }
        // Write into the NEXT buffer of the pool, never into the one just published.
        let mut previous_serial = 0u64;
        let snapshot_raw =
            crate::xr_capture_state::cvr_xr_capture_begin_snapshot(&mut previous_serial, c.writes);
        if snapshot_raw.is_null() {
            crate::menu_lobby::trace_boot_skip("no snapshot slot");
            curtain_only(c, source, &eye_desc, split, main, engine_stereo, menu_open);
            if diagnostics() {
                CyberpunkVR_DebugCapSkipNoSlot.fetch_add(1, Relaxed);
            }
            return false;
        }
        // The snapshot's reference of this capture's own, released when it ends.
        let snapshot = ID3D12Resource::from_raw(snapshot_raw);
        let ring = cvr_xr::capture_ring::cvr_xr_capture_ring();
        if !cvr_xr::swapchains::cvr_xr_next_capture_allocator(
            (*ring).fence,
            (*ring).fence_event,
            (&raw mut (*ring).allocator_fences).cast::<u64>(),
            &raw mut (*ring).allocator_index,
        ) {
            crate::menu_lobby::trace_boot_skip("no allocator");
            return false; // the ring stays where it is
        }
        let index = (*ring).allocator_index;
        let allocator_raw = (*ring).allocators[index as usize];
        let list_raw = (*ring).lists[index as usize];
        let (Some(allocator), Some(list)) = (
            ID3D12CommandAllocator::from_raw_borrowed(&allocator_raw),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list_raw),
        ) else {
            return false;
        };
        if allocator.Reset().is_err()
            || list.Reset(allocator, None::<&ID3D12PipelineState>).is_err()
        {
            cvr_host::log::write("OpenXRManager: Failed to reset mono capture command list\n");
            return false;
        }
        let profile = cvr_gpu::stage_profile::cvr_gpu_stage_begin(list_raw);
        let captured = frame_body(
            c,
            list,
            list_raw,
            profile.cast(),
            snapshot_raw,
            &snapshot,
            previous_serial,
            main,
            afw_eye,
            afw_pose_id,
            &afw_image,
            menu_open,
            loading_dark,
            engine_stereo,
            split,
            squeeze,
            notice,
            loading_in_space,
            loading_copy,
            &eye_desc,
            loading_uv,
            loading_aspect,
            source,
        );
        cvr_gpu::stage_profile::cvr_gpu_stage_free(profile);
        drop(snapshot);
        captured
    }
}

/// A FRAME NOT CAPTURED UNDER THE CURTAIN: no snapshot slot was free (the submission still reading every one -- its
/// thread held up making the frame generator on the first stereo pair, or draining for an overlay), so nothing of this
/// frame reaches the headset, which keeps showing the last capture. The presented image still goes to the window: the
/// curtain and the intro's picture are drawn over it here as the capture would have, or the window flashes the lobby
/// the curtain hides (the C++ build never skipped there).
///
/// # Safety
/// As cvr_xr_capture_frame; `source` and `eye_desc` are this present's.
#[allow(clippy::too_many_arguments)]
unsafe fn curtain_only(
    c: &CaptureFrameIn,
    source: &D3D12_RESOURCE_DESC,
    eye_desc: &D3D12_RESOURCE_DESC,
    split: bool,
    main: usize,
    engine_stereo: bool,
    menu_open: bool,
) {
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue, ID3D12Fence, ID3D12PipelineState,
    };
    let curtain = crate::menu_lobby::cvr_menu_lobby_curtain();
    if curtain <= 0.0 {
        return;
    }
    // SAFETY (whole body): as the caller promises; the ring is the capture's own, used on this thread only.
    unsafe {
        let ring = cvr_xr::capture_ring::cvr_xr_capture_ring();
        if !cvr_xr::swapchains::cvr_xr_next_capture_allocator(
            (*ring).fence,
            (*ring).fence_event,
            (&raw mut (*ring).allocator_fences).cast::<u64>(),
            &raw mut (*ring).allocator_index,
        ) {
            return;
        }
        let index = (*ring).allocator_index;
        let allocator_raw = (*ring).allocators[index as usize];
        let list_raw = (*ring).lists[index as usize];
        let (Some(allocator), Some(list), Some(queue), Some(fence), Some(bb)) = (
            ID3D12CommandAllocator::from_raw_borrowed(&allocator_raw),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list_raw),
            ID3D12CommandQueue::from_raw_borrowed(&c.queue),
            ID3D12Fence::from_raw_borrowed(&(*ring).fence),
            ID3D12Resource::from_raw_borrowed(&c.back_buffer),
        ) else {
            return;
        };
        if !crate::lobby_intro::cvr_intro_overlay_begin(curtain, &c.head_centers[main]) {
            return;
        }
        if allocator.Reset().is_err() || list.Reset(allocator, None::<&ID3D12PipelineState>).is_err() {
            return;
        }
        let seen = {
            let _lock = cvr_host::submission::lock();
            let image = read_image(c.back_buffer);
            [image_pose(c, 0, &image, 1), image_pose(c, 1, &image, 1)]
        };
        let intro_picture = split || (!engine_stereo && !menu_open);
        barrier(list, bb, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        let height = source.Height as i32;
        if split {
            for (eye, pose) in seen.iter().enumerate() {
                let x = if eye == 0 { 0 } else { (source.Width - eye_desc.Width) as i32 };
                let rect = RECT { left: x, top: 0, right: x + eye_desc.Width as i32, bottom: height };
                crate::lobby_intro::cvr_intro_overlay_record(
                    c.device,
                    list_raw,
                    c.back_buffer,
                    &rect,
                    pose,
                    &c.fovs[eye],
                    true,
                    index,
                );
            }
        } else {
            let rect = RECT { left: 0, top: 0, right: source.Width as i32, bottom: height };
            crate::lobby_intro::cvr_intro_overlay_record(
                c.device,
                list_raw,
                c.back_buffer,
                &rect,
                &seen[main],
                &c.fovs[main],
                intro_picture,
                index,
            );
        }
        barrier(list, bb, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
        if list.Close().is_err() {
            return;
        }
        let lists = [Some(list.cast::<ID3D12CommandList>().expect("a graphics list is a command list"))];
        queue.ExecuteCommandLists(&lists);
        (*ring).fence_value += 1;
        let value = (*ring).fence_value;
        (*ring).allocator_fences[index as usize] = value;
        let _ = queue.Signal(fence, value);
    }
}

/// cvr_xr_capture_frame from the list's reset on.
///
/// # Safety
/// As cvr_xr_capture_frame.
#[allow(clippy::too_many_arguments)]
unsafe fn frame_body(
    c: &CaptureFrameIn,
    list: &ID3D12GraphicsCommandList,
    list_raw: *mut c_void,
    profile: *const c_void,
    snapshot_raw: *mut c_void,
    snapshot: &ID3D12Resource,
    previous_serial: u64,
    main: usize,
    afw_eye: i32,
    afw_pose_id: u64,
    afw_image: &ImagePose,
    menu_open: bool,
    loading_dark: bool,
    engine_stereo: bool,
    split: bool,
    squeeze: bool,
    notice: crate::intro_boot::BootFrame,
    loading_in_space: bool,
    loading_copy: bool,
    eye_desc: &D3D12_RESOURCE_DESC,
    loading_uv: [f32; 4],
    loading_aspect: f32,
    source: &D3D12_RESOURCE_DESC,
) -> bool {
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let _ = snapshot;
        let ring = cvr_xr::capture_ring::cvr_xr_capture_ring();
        let index = (*ring).allocator_index;
        let eye_w = eye_desc.Width as u32;
        let eye_h = source.Height;
        let half_x = |eye: usize| {
            if eye == 0 {
                0
            } else {
                (source.Width - eye_desc.Width) as i32
            }
        };
        // THE LOBBY'S CURTAIN AND INTRO, INTO THE FRAME (Render/IntroOverlay.hpp): over the presented image itself, before
        // anything is copied out of it, so the window shows what the eyes do. Each eye's image gets the intro's picture
        // seen through that eye (the second eye's below, after its copy); a window both eyes share the curtain alone.
        let curtain_now = crate::menu_lobby::cvr_menu_lobby_curtain();
        let intro = crate::lobby_intro::cvr_intro_overlay_begin(curtain_now, &c.head_centers[main]);
        crate::menu_lobby::trace_boot_frame(
            curtain_now,
            intro,
            (notice.shown, notice.live, notice.opacity),
            menu_open,
            split,
        );
        let intro_picture = split || (!engine_stereo && !menu_open);
        if intro {
            let seen = {
                let _lock = cvr_host::submission::lock();
                let image = read_image(c.back_buffer);
                [image_pose(c, 0, &image, 1), image_pose(c, 1, &image, 1)]
            };
            let Some(bb) = ID3D12Resource::from_raw_borrowed(&c.back_buffer) else {
                return false;
            };
            barrier(
                list,
                bb,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            );
            let height = source.Height as i32;
            if split {
                for eye in 0..2 {
                    let x = half_x(eye);
                    let rect = RECT {
                        left: x,
                        top: 0,
                        right: x + eye_w as i32,
                        bottom: height,
                    };
                    crate::lobby_intro::cvr_intro_overlay_record(
                        c.device,
                        list_raw,
                        c.back_buffer,
                        &rect,
                        &seen[eye],
                        &c.fovs[eye],
                        true,
                        index,
                    );
                }
            } else {
                let rect = RECT {
                    left: 0,
                    top: 0,
                    right: source.Width as i32,
                    bottom: height,
                };
                crate::lobby_intro::cvr_intro_overlay_record(
                    c.device,
                    list_raw,
                    c.back_buffer,
                    &rect,
                    &seen[main],
                    &c.fovs[main],
                    intro_picture,
                    index,
                );
            }
            barrier(
                list,
                bb,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            );
        }

        // MAIN's picture, the window's and the second eye's.
        let context = FrameContext {
            c,
            list: list_raw,
            allocator_index: index,
            eye_w,
            eye_h,
            main,
            intro_picture,
            notice,
            loading_copy,
        };
        let ctx = &context as *const FrameContext as *mut c_void;
        let pictures = CapturePicturesIn {
            second: SecondEyeIn {
                device: c.device,
                list: list_raw,
                profile,
                eye_w,
                eye_h,
                eye1_width: c.eye1_width,
                ipd: c.ipd,
                main_is_right: c.main_is_right,
                asym_active: c.asym_active,
                intro,
                ctx,
                ensure_slots: frame_ensure_slots,
                take_slot: frame_take_slot,
                hud_params: frame_hud_params,
                eye1_fov: frame_eye1_fov,
                asym_windows: frame_asym_windows,
                intro_record: frame_intro_record,
            },
            back_buffer: c.back_buffer,
            snapshot: snapshot_raw,
            source_width: source.Width,
            source_height: source.Height,
            source_format: source.Format.0,
            main_eye: main as u32,
            afw_eye,
            previous_serial: previous_serial != 0,
            squeeze,
            loading_in_space,
            loading_copy,
            engine_stereo,
            split,
            menu_open,
            notice_shown: notice.shown,
            stereo_submit: c.stereo_submit,
            mono_menu: c.mono_menu,
            poses: c.poses,
            fovs: c.fovs,
            loading_uv,
            loading_aspect,
            loading_begin: frame_loading_begin,
            loading_record: frame_loading_record,
            queue: c.queue,
            afw_pose_id: afw_image.pose.pose_id,
            afw_origin: afw_image.pose.head.origin_serial,
            afw_render_frame_id: afw_image.pose.render_frame_id,
            half_ipd: c.half_ipd,
            afw_has_render_frame_id: afw_image.pose.has_render_frame_id,
            keep_inputs: c.keep_inputs,
        };
        let mut out = CapturePicturesOut {
            source: core::ptr::null_mut(),
            status: SECOND_EYE_NONE,
            loading_window: false,
            afw_warped: false,
        };
        cvr_xr_capture_pictures(&pictures, &mut out);
        if out.status < 0 {
            let _ = list.Close();
            return false;
        }

        let submit = CaptureSubmitIn {
            list: list_raw,
            queue: c.queue,
            fence: (*ring).fence,
            fence_value: &raw mut (*ring).fence_value,
            allocator_fence: (&raw mut (*ring).allocator_fences).cast::<u64>().add(index as usize),
            profile,
            back_buffer: c.back_buffer,
            second_source: out.source,
            snapshot: snapshot_raw,
            writes: c.writes,
            ctx: core::ptr::null_mut(),
            device: c.device,
            present_event: c.present_event,
            serial: c.serial,
            afw_pose_id,
            origin: c.origin,
            poses: c.poses,
            head_centers: c.head_centers,
            fovs: c.fovs,
            main_eye: main as u32,
            cas_amount: c.cas_amount,
            sharpness: c.sharpness,
            has_view: c.has_view,
            second_eye: out.status > 0,
            afw_warped: out.afw_warped,
            split,
            squeeze,
            engine_stereo,
            loading_window: out.loading_window,
            loading_dark,
            notice_shown: notice.shown,
            bind: c.bind,
            keep_inputs: c.keep_inputs,
            lock: submission_lock,
            unlock: submission_unlock,
            afw_fallback: afw_fallback,
        };
        cvr_xr_capture_submit(&submit);
        true
    }
}
extern "C" fn submission_lock(_: *mut c_void) {
    cvr_host::submission::cvr_image_submission_lock();
}
extern "C" fn submission_unlock(_: *mut c_void) {
    cvr_host::submission::cvr_image_submission_unlock();
}
extern "C" fn afw_fallback(_: *mut c_void, why: *const core::ffi::c_char) {
    crate::afw::cvr_afw_fallback(why);
}
