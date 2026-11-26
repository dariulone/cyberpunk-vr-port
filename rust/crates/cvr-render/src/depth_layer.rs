//! THE STEREO DEPTH LAYER (formerly include/Runtimes/StereoDepthPolicy.hpp and DepthSwapchainImage.hpp, OpenXRCapture.cpp's
//! EnsureStereoDepthSwapchains, the frame loop's depth callback and stereo depth report, and the stereo depth renderer's
//! owner, src/Render/StereoDepth.cpp): each eye's depth for XR_KHR_composition_layer_depth, recorded from the frame
//! generator's inputs of the selected frame into the eye's own depth swapchain.
//!
//! A frame's depth is submitted only when it is the depth of the very pixels shown: each eye's input labelled with the
//! eye's own pose, the tracking origin and the engine frame, its camera projecting exactly like the eye's view. A
//! generated midpoint has a depth of its own (main's 0.2.4, CyberpunkVR_SyntheticDepth): B's depth moved half of B's
//! motion vectors back toward A, the vectors the interpolation follows (cvr_gpu::stereo_depth). A reused frame
//! resubmits the depth swapchains' last released images with it.
//!
//! The eyes' depth swapchains (D32_FLOAT) are created lazily at the plan's size and recreated on a size change once no
//! record of the renderer reads them; a timed-out wait keeps its acquired image and waits again next cycle
//! (xrReleaseSwapchainImage is legal only after a successful wait).
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::*};
use std::sync::{LazyLock, Mutex, MutexGuard};

use cvr_gpu::frame_capture::CameraData;
use cvr_gpu::stereo_depth::{
    cvr_gpu_stereo_depth_free, cvr_gpu_stereo_depth_idle, cvr_gpu_stereo_depth_new,
    cvr_gpu_stereo_depth_poll, cvr_gpu_stereo_depth_record, DepthEye,
};
use cvr_host::log;
use openxr_sys as xr;
use windows::core::{Interface as _, HSTRING};
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use xr::Handle as _;

use crate::framegen_inputs::{cvr_framegen_release_inputs, cvr_framegen_retain_inputs, Inputs};
use crate::framegen_presenter::{DepthSource, Identity};

/// cvr::depth::Reject: why a frame went out without its depth layer.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Reject {
    None = 0,
    Synthetic,
    #[default]
    Missing,
    Identity,
    Projection,
    Camera,
    Queue,
    Resource,
    Busy,
    Runtime,
}

/// cvr::depth::View: one eye's depth image and its planes (metres) and jitter.
#[derive(Clone, Copy, Default, Debug)]
pub struct View {
    pub width: u32,
    pub height: u32,
    pub near_z: f32,
    pub far_z: f32,
    pub jitter: [f32; 2],
    /// A midpoint's depth: B's depth moved this share of B's motion vectors (toward A), and the vectors' scale to a
    /// fraction of the image (CameraData::motionScale). 0 for a real frame (main's 0.2.4).
    pub motion: f32,
    pub motion_scale: [f32; 2],
}

/// Depth layers for generated (midpoint) frames too (main's 0.2.4, StereoDepthPolicy.hpp Describe); 0 = real frames
/// only.
#[no_mangle]
pub static mut CyberpunkVR_SyntheticDepth: i32 = 1;
/// cvr::depth::Plan.
#[derive(Clone, Copy, Default, Debug)]
pub struct Plan {
    pub eyes: [View; 2],
}

/// The same pose to 1e-5: unit-length orientations equal up to sign, positions equal.
fn same_pose(a: &xr::Posef, b: &xr::Posef) -> bool {
    let (x, y) = (&a.orientation, &b.orientation);
    let dot = x.x * y.x + x.y * y.y + x.z * y.z + x.w * y.w;
    let (p, q) = (&a.position, &b.position);
    dot.is_finite()
        && (dot.abs() - 1.0).abs() < 1e-5
        && (p.x - q.x).abs() < 1e-5
        && (p.y - q.y).abs() < 1e-5
        && (p.z - q.z).abs() < 1e-5
}
/// The engine camera's off-centre projection is the view's: the same coefficients the engine receives.
fn same_projection(c: &CameraData, f: &xr::Fovf) -> bool {
    let (l, r, u, d) = (
        f.angle_left.tan(),
        f.angle_right.tan(),
        f.angle_up.tan(),
        f.angle_down.tan(),
    );
    let half_y = (c.vertical_fov * 0.5).tan();
    let half_x = half_y * c.aspect;
    let close = |x: f32, y: f32| {
        x.is_finite() && y.is_finite() && (x - y).abs() < 0.002 * 1f32.max(y.abs())
    };
    r > l
        && u > d
        && close(half_x, (r - l) * 0.5)
        && close(half_y, (u - d) * 0.5)
        && close(c.shift[0], -(r + l) / (r - l))
        && close(c.shift[1], -(u + d) / (u - d))
}

/// StereoDepthPolicy.hpp Describe: whether the selected frame's depth is the depth of its pixels in these views, and
/// the plan -- each eye's size, planes in metres (the game multiplies head translation by its world scale) and jitter.
pub(crate) fn describe(
    selected: &DepthSource,
    views: &[xr::CompositionLayerProjectionView],
    game_units_per_meter: f32,
    out: &mut Plan,
) -> Reject {
    *out = Plan::default();
    // A MIDPOINT HAS A DEPTH OF ITS OWN: B's depth, the real frame it leads to, moved half of B's motion vectors back
    // toward A -- the same vectors the interpolation follows, camera motion included. With the depth layer on real
    // frames only, every other image would be reprojected another way.
    let synthetic = selected.synthetic;
    // SAFETY: the exported switch.
    if synthetic && unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_SyntheticDepth) } == 0 {
        return Reject::Synthetic;
    }
    let f: &Identity = &selected.identity;
    let (Some(i0), Some(i1)) = (selected.input(0), selected.input(1)) else {
        return Reject::Missing;
    };
    if views.len() != 2 || !f.stereo || f.serial == 0 {
        return Reject::Missing;
    }
    if !game_units_per_meter.is_finite() || game_units_per_meter <= 0.0 {
        return Reject::Camera;
    }
    if i0.view == i1.view {
        return Reject::Identity;
    }
    for (eye, input) in [i0, i1].into_iter().enumerate() {
        let c = &input.camera;
        if !input.submitted
            || !input.has_depth
            || input.depth.is_none()
            || input.width == 0
            || input.height == 0
        {
            return Reject::Missing;
        }
        if synthetic
            && (!input.has_motion
                || input.motion.is_none()
                || input.motion_width == 0
                || input.motion_height == 0
                || !c.camera_motion
                || c.motion3d
                || !c.motion_scale[0].is_finite()
                || !c.motion_scale[1].is_finite())
        {
            return Reject::Missing;
        }
        // A midpoint's pose is a blend (the presenter's identity pose 0); its B inputs still carry B's own identity.
        if f.origin == 0
            || (!synthetic && (f.pose[eye] == 0 || input.pose_id != f.pose[eye]))
            || input.origin != f.origin
            || input.frame_id != f.native_frame
            || input.view < 1
            || input.view > 2
            || !same_pose(&selected.pose[eye], &views[eye].pose)
        {
            return Reject::Identity;
        }
        if !same_projection(c, &views[eye].fov) {
            return Reject::Projection;
        }
        if !c.near_plane.is_finite()
            || c.near_plane <= 0.0
            || !(c.far_plane > c.near_plane)
            || (!c.far_plane.is_finite() && !c.infinite)
            || !c.jitter[0].is_finite()
            || !c.jitter[1].is_finite()
            || c.jitter[0].abs() > 2.0
            || c.jitter[1].abs() > 2.0
        {
            return Reject::Camera;
        }
        let v = &mut out.eyes[eye];
        v.width = input.width;
        v.height = input.height;
        let n = c.near_plane / game_units_per_meter;
        let far_clip = c.far_plane / game_units_per_meter;
        v.near_z = if c.inverted { far_clip } else { n };
        v.far_z = if c.inverted { n } else { far_clip };
        v.jitter = c.jitter;
        v.motion = if synthetic { 0.5 } else { 0.0 };
        v.motion_scale = c.motion_scale;
    }
    Reject::None
}

#[cfg(not(test))]
extern "system" {
    fn xrEnumerateSwapchainFormats(
        session: xr::Session,
        capacity: u32,
        count: *mut u32,
        formats: *mut i64,
    ) -> xr::Result;
    fn xrCreateSwapchain(
        session: xr::Session,
        info: *const xr::SwapchainCreateInfo,
        swapchain: *mut xr::Swapchain,
    ) -> xr::Result;
    fn xrEnumerateSwapchainImages(
        swapchain: xr::Swapchain,
        capacity: u32,
        count: *mut u32,
        images: *mut xr::SwapchainImageBaseHeader,
    ) -> xr::Result;
    fn xrDestroySwapchain(swapchain: xr::Swapchain) -> xr::Result;
    fn xrAcquireSwapchainImage(
        swapchain: xr::Swapchain,
        info: *const xr::SwapchainImageAcquireInfo,
        index: *mut u32,
    ) -> xr::Result;
    fn xrWaitSwapchainImage(
        swapchain: xr::Swapchain,
        info: *const xr::SwapchainImageWaitInfo,
    ) -> xr::Result;
    fn xrReleaseSwapchainImage(
        swapchain: xr::Swapchain,
        info: *const xr::SwapchainImageReleaseInfo,
    ) -> xr::Result;
}
extern "C" {
    fn GetDepthSubmit() -> i32;
    fn GetMonoDepthCapture() -> i32;
    static CyberpunkVR_StereoDepthReportSeq: AtomicU32;
    static mut CyberpunkVR_StereoDepthReport: [u8; 768];
}

/// DXGI_FORMAT_D32_FLOAT.
const D32_FLOAT: i64 = 40;
/// cvr::gpu::profile::Stage::SubmitDepth.
const STAGE_SUBMIT_DEPTH: u32 = 4;

/// One eye's depth swapchain (EyeSwapchain's depth fields).
#[derive(Default)]
struct Eye {
    handle: u64,
    width: u32,
    height: u32,
    /// Acquired, its wait not yet succeeded.
    pending: bool,
    pending_index: u32,
    images: Vec<usize>,
}
impl Eye {
    fn handle(&self) -> xr::Swapchain {
        xr::Swapchain::from_raw(self.handle)
    }
    /// DepthSwapchainImage.hpp WaitImage: a timeout does not cancel the acquisition -- the index kept, its wait
    /// retried; the image may be released only after XR_SUCCESS.
    fn wait_image(&mut self) -> xr::Result {
        // SAFETY (whole body): the eye's live swapchain.
        unsafe {
            if !self.pending {
                let acquire = xr::SwapchainImageAcquireInfo {
                    ty: xr::StructureType::SWAPCHAIN_IMAGE_ACQUIRE_INFO,
                    next: core::ptr::null(),
                };
                let result =
                    xrAcquireSwapchainImage(self.handle(), &acquire, &mut self.pending_index);
                if result != xr::Result::SUCCESS {
                    return result;
                }
                self.pending = true;
            }
            let wait = xr::SwapchainImageWaitInfo {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO,
                next: core::ptr::null(),
                timeout: xr::Duration::from_nanos(0),
            };
            let result = xrWaitSwapchainImage(self.handle(), &wait);
            if result == xr::Result::SUCCESS {
                self.pending = false;
            }
            result
        }
    }
    fn destroy(&mut self) {
        if self.handle != 0 {
            // SAFETY: the eye's swapchain, destroyed once.
            unsafe { xrDestroySwapchain(self.handle()) };
        }
        *self = Eye::default();
    }
}

/// The renderer's release of an eye's input: one inputs reference.
unsafe extern "C" fn release_input(keep: *mut c_void) {
    // SAFETY: a reference retained for the record.
    unsafe { cvr_framegen_release_inputs(keep as *const Inputs) };
}

struct Layer {
    /// The runtime's depth format (0 until enumerated).
    format: i64,
    eyes: [Eye; 2],
    /// cvr_gpu::stereo_depth's renderer (created with the first record).
    renderer: *mut c_void,
}
// SAFETY: the XR thread's, used under the lock.
unsafe impl Send for Layer {}
static LAYER: LazyLock<Mutex<Layer>> = LazyLock::new(|| {
    Mutex::new(Layer {
        format: 0,
        eyes: [Eye::default(), Eye::default()],
        renderer: core::ptr::null_mut(),
    })
});
fn layer() -> MutexGuard<'static, Layer> {
    LAYER.lock().unwrap_or_else(|e| e.into_inner())
}
/// The runtime supports XR_KHR_composition_layer_depth and a depth swapchain format (m_depthLayerSupported).
static SUPPORTED: AtomicBool = AtomicBool::new(false);

impl Layer {
    fn renderer(&mut self) -> *mut c_void {
        if self.renderer.is_null() {
            self.renderer = cvr_gpu_stereo_depth_new(release_input);
        }
        self.renderer
    }
    fn idle(&self) -> bool {
        // SAFETY: the layer's renderer.
        self.renderer.is_null() || unsafe { cvr_gpu_stereo_depth_idle(self.renderer) }
    }
    /// EnsureStereoDepthSwapchains: both eyes' depth swapchains at the plan's size, the format enumerated once (a
    /// runtime without D32_FLOAT has no depth layer).
    fn ensure(&mut self, session: u64, plan: &Plan) -> bool {
        if !SUPPORTED.load(Relaxed) || session == 0 {
            return false;
        }
        let session = xr::Session::from_raw(session);
        // SAFETY (whole body): the live session and the eyes' swapchains.
        unsafe {
            if self.format == 0 {
                let mut count = 0u32;
                if xrEnumerateSwapchainFormats(session, 0, &mut count, core::ptr::null_mut())
                    .into_raw()
                    < 0
                    || count == 0
                {
                    return false;
                }
                let mut formats = vec![0i64; count as usize];
                if xrEnumerateSwapchainFormats(session, count, &mut count, formats.as_mut_ptr())
                    .into_raw()
                    < 0
                {
                    return false;
                }
                if !formats.contains(&D32_FLOAT) {
                    SUPPORTED.store(false, Relaxed);
                    cvr_gpu::framegen_core::cvr_framegen_set_depth_capture_enabled(false);
                    return false;
                }
                self.format = D32_FLOAT;
            }
            for eye in 0..2 {
                let v = plan.eyes[eye];
                if v.width == 0 || v.height == 0 || v.width > 16384 || v.height > 16384 {
                    return false;
                }
                let e = &self.eyes[eye];
                if e.handle != 0 && e.width == v.width && e.height == v.height {
                    continue;
                }
                // DRS/resolution changes never destroy a resource still read by our GPU.
                if !self.idle() {
                    return false;
                }
                let format = self.format;
                let e = &mut self.eyes[eye];
                if e.pending {
                    let wait = xr::SwapchainImageWaitInfo {
                        ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO,
                        next: core::ptr::null(),
                        timeout: xr::Duration::from_nanos(0),
                    };
                    if xrWaitSwapchainImage(e.handle(), &wait) != xr::Result::SUCCESS {
                        return false;
                    }
                    let release = xr::SwapchainImageReleaseInfo {
                        ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO,
                        next: core::ptr::null(),
                    };
                    if xrReleaseSwapchainImage(e.handle(), &release) != xr::Result::SUCCESS {
                        return false;
                    }
                    e.pending = false;
                }
                if e.handle != 0 {
                    if xrDestroySwapchain(e.handle()).into_raw() < 0 {
                        return false;
                    }
                    e.handle = 0;
                    e.images.clear();
                }
                let info = xr::SwapchainCreateInfo {
                    ty: xr::StructureType::SWAPCHAIN_CREATE_INFO,
                    next: core::ptr::null(),
                    create_flags: xr::SwapchainCreateFlags::EMPTY,
                    usage_flags: xr::SwapchainUsageFlags::DEPTH_STENCIL_ATTACHMENT
                        | xr::SwapchainUsageFlags::TRANSFER_DST,
                    format,
                    sample_count: 1,
                    width: v.width,
                    height: v.height,
                    face_count: 1,
                    array_size: 1,
                    mip_count: 1,
                };
                let mut handle = xr::Swapchain::NULL;
                if xrCreateSwapchain(session, &info, &mut handle).into_raw() < 0 {
                    return false;
                }
                e.handle = handle.into_raw();
                let mut count = 0u32;
                if xrEnumerateSwapchainImages(handle, 0, &mut count, core::ptr::null_mut())
                    .into_raw()
                    < 0
                    || count == 0
                {
                    xrDestroySwapchain(handle);
                    e.handle = 0;
                    return false;
                }
                let mut images: Vec<xr::SwapchainImageD3D12KHR> = (0..count)
                    .map(|_| xr::SwapchainImageD3D12KHR {
                        ty: xr::StructureType::SWAPCHAIN_IMAGE_D3D12_KHR,
                        next: core::ptr::null_mut(),
                        texture: core::ptr::null_mut(),
                    })
                    .collect();
                if xrEnumerateSwapchainImages(handle, count, &mut count, images.as_mut_ptr().cast())
                    .into_raw()
                    < 0
                {
                    xrDestroySwapchain(handle);
                    e.handle = 0;
                    e.images.clear();
                    return false;
                }
                e.images = images.iter().map(|i| i.texture as usize).collect();
                e.width = v.width;
                e.height = v.height;
                for (i, image) in images.iter().enumerate() {
                    let p = image.texture as *mut c_void;
                    if let Some(r) = ID3D12Resource::from_raw_borrowed(&p) {
                        let _ = r.SetName(&HSTRING::from(
                            format!("OpenXR_eye{eye}_depth_image{i}").as_str(),
                        ));
                    }
                }
                log!(
                    "OpenXRManager: per-eye depth swapchain eye={} {}x{} D32_FLOAT\n",
                    eye,
                    v.width,
                    v.height
                );
            }
        }
        true
    }
}

/// The cycle's depth inputs the C++ half evaluates: GetWorldScale(), and whether the live controls scale the eyes
/// apart (xr_ipd_scale / xr_stereo_scale not 1: the engine camera is not the eye's).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DepthIn {
    pub world_scale: f32,
    pub camera_scaled: bool,
}
const _: () = assert!(core::mem::size_of::<DepthIn>() == 8);
impl DepthIn {
    /// GetWorldScale() and whether the live controls' IPD or stereo scale moves the eyes away from 1.
    pub fn live() -> DepthIn {
        use cvr_settings::lc;
        DepthIn {
            world_scale: cvr_camera::world_scale::world_scale(lc!(xrWorldScale)),
            camera_scaled: cvr_camera::world_scale::eyes_scaled(lc!(xrIpdScale), lc!(xrStereoScale)),
        }
    }
}

/// What the cycle chose, for its stereo depth report.
#[derive(Default)]
pub struct Cycle {
    pub plan: Plan,
    pub reason: Reject,
    pub native_frame: u32,
    pub synthetic: bool,
}

/// The cycle's submit objects the depth record goes into.
pub struct Target {
    pub device: *mut c_void,
    pub list: *mut c_void,
    pub queue: *mut c_void,
    pub session: u64,
    pub slot: u32,
    pub fence: *mut c_void,
    pub value: u64,
    pub profile: *const cvr_gpu::stage_profile::Batch,
}

/// THE CYCLE'S DEPTH LAYER (the frame loop's former depth callback): a reused frame with the swapchains' last released
/// depth images reuses their infos; otherwise the selected frame's depth is described, the eyes' depth images waited for
/// and recorded into (under the profile's SubmitDepth scope). Sets the views' `next` to `infos` and the depth images
/// acquired; returns whether the depth layer goes out.
///
/// # Safety
/// The target's objects live; `last_copy` is the manager's.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn cycle(
    d: &DepthIn,
    t: &Target,
    reuse_images: bool,
    copy_ready: bool,
    menu_rect_active: bool,
    fg_selected: bool,
    last_copy: &crate::xr_cycle_end::LastCopy,
    views: &mut [xr::CompositionLayerProjectionView],
    infos: &mut [xr::CompositionLayerDepthInfoKHR; 2],
    acquired: &mut [bool],
    out: &mut Cycle,
) -> bool {
    let source = if fg_selected {
        crate::framegen_presenter::depth_source()
    } else {
        None
    };
    if let Some(s) = &source {
        out.native_frame = s.identity.native_frame;
        out.synthetic = s.synthetic;
    }
    // SAFETY: C++ getters.
    let enabled = unsafe { GetDepthSubmit() != 0 && GetMonoDepthCapture() != 0 };
    let supported = SUPPORTED.load(Relaxed);
    let mut l = layer();
    let mut use_depth = false;
    if reuse_images
        && copy_ready
        && !menu_rect_active
        && last_copy.depth_used
        && enabled
        && supported
        && views.len() >= 2
        && last_copy.depth_info[0].sub_image.swapchain.into_raw() == l.eyes[0].handle
        && last_copy.depth_info[1].sub_image.swapchain.into_raw() == l.eyes[1].handle
    {
        for eye in 0..2 {
            infos[eye] = last_copy.depth_info[eye];
            infos[eye].next = core::ptr::null();
            views[eye].next = (&raw const infos[eye]).cast();
        }
        use_depth = true;
        out.reason = Reject::None;
    } else if copy_ready && !menu_rect_active && fg_selected && enabled && supported {
        let reason = &mut out.reason;
        let plan = &mut out.plan;
        *reason = match &source {
            Some(s) => describe(s, views, d.world_scale, plan),
            None => Reject::Missing,
        };
        if *reason == Reject::None && d.camera_scaled {
            *reason = Reject::Camera;
        }
        if *reason == Reject::None && !l.ensure(t.session, plan) {
            *reason = Reject::Runtime;
        }
        if *reason == Reject::None {
            let mut targets = [core::ptr::null_mut::<c_void>(); 2];
            for eye in 0..2 {
                // On timeout the acquired image is kept and waited for again next cycle.
                if l.eyes[eye].wait_image() != xr::Result::SUCCESS {
                    *reason = Reject::Runtime;
                    break;
                }
                acquired[eye] = true;
                let e = &l.eyes[eye];
                let Some(&image) = e.images.get(e.pending_index as usize) else {
                    *reason = Reject::Resource;
                    break;
                };
                targets[eye] = image as *mut c_void;
            }
            if *reason == Reject::None {
                let s = source.as_ref().unwrap();
                let mut eyes: [DepthEye; 2] = core::array::from_fn(|eye| {
                    let v = &plan.eyes[eye];
                    let mut e = DepthEye {
                        keep: core::ptr::null_mut(),
                        depth: core::ptr::null_mut(),
                        input_queue: core::ptr::null_mut(),
                        ready_fence: core::ptr::null_mut(),
                        fence_value: 0,
                        submitted: false,
                        inverted: false,
                        near_plane: 0.0,
                        far_plane: 0.0,
                        destination: targets[eye],
                        width: v.width,
                        height: v.height,
                        jitter: v.jitter,
                        motion: core::ptr::null_mut(),
                        motion_width: 0,
                        motion_height: 0,
                        motion_share: v.motion,
                        motion_scale: v.motion_scale,
                    };
                    if let Some(input) = s.input(eye) {
                        // SAFETY: a held reference, one more for the record.
                        e.keep = unsafe { cvr_framegen_retain_inputs(input) } as *mut c_void;
                        e.depth = input
                            .depth
                            .as_ref()
                            .map_or(core::ptr::null_mut(), |r| r.as_raw());
                        e.input_queue = input.queue;
                        e.ready_fence = input
                            .ready_fence
                            .as_ref()
                            .map_or(core::ptr::null_mut(), |f| f.as_raw());
                        e.fence_value = input.fence_value;
                        e.submitted = input.submitted;
                        e.inverted = input.camera.inverted;
                        e.near_plane = input.camera.near_plane;
                        e.far_plane = input.camera.far_plane;
                        e.motion = input.motion.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw());
                        e.motion_width = input.motion_width;
                        e.motion_height = input.motion_height;
                    }
                    e
                });
                let scope = if t.profile.is_null() {
                    u32::MAX
                } else {
                    // SAFETY: the cycle's live batch.
                    unsafe {
                        cvr_gpu::stage_profile::cvr_gpu_stage_scope_begin(
                            t.profile,
                            STAGE_SUBMIT_DEPTH,
                        )
                    }
                };
                let renderer = l.renderer();
                // SAFETY: the layer's renderer, the cycle's objects; the eyes' references handed over.
                use_depth = unsafe {
                    cvr_gpu_stereo_depth_record(
                        renderer,
                        t.device,
                        t.list,
                        t.queue,
                        eyes.as_mut_ptr(),
                        t.slot,
                        t.fence,
                        t.value,
                    )
                };
                if scope != u32::MAX {
                    // SAFETY: the scope opened above on the live batch.
                    unsafe { cvr_gpu::stage_profile::cvr_gpu_stage_scope_end(t.profile, scope) };
                }
                if !use_depth {
                    *reason = Reject::Busy;
                }
            }
        }
        if use_depth {
            for eye in 0..2 {
                let v = &plan.eyes[eye];
                infos[eye] = xr::CompositionLayerDepthInfoKHR {
                    ty: xr::StructureType::COMPOSITION_LAYER_DEPTH_INFO_KHR,
                    next: core::ptr::null(),
                    sub_image: xr::SwapchainSubImage {
                        swapchain: l.eyes[eye].handle(),
                        image_rect: xr::Rect2Di {
                            offset: xr::Offset2Di { x: 0, y: 0 },
                            extent: xr::Extent2Di {
                                width: v.width as i32,
                                height: v.height as i32,
                            },
                        },
                        image_array_index: 0,
                    },
                    min_depth: 0.0,
                    max_depth: 1.0,
                    near_z: v.near_z,
                    far_z: v.far_z,
                };
                views[eye].next = (&raw const infos[eye]).cast();
            }
        }
    }
    use_depth
}

/// The stereo depth report (CyberpunkVR_StereoDepthReport, a seqlock'd JSON line): the cycle's serial, engine frame,
/// synthetic or not, whether its depth went out with an accepted end, why not, the eyes' labels, sizes and planes.
pub(crate) fn report(
    c: &Cycle,
    use_depth: bool,
    end_result: i32,
    serial: u64,
    pose_ids: &[u64; 2],
) {
    use cvr_crt::Arg::{F, I, U};
    let reason = if end_result < 0 {
        Reject::Runtime
    } else {
        c.reason
    };
    let p = &c.plan;
    let text = cvr_crt::format(
        c"{\"serial\":%llu,\"frame\":%u,\"synthetic\":%d,\"depth\":%d,\"reason\":%u,\"pose\":[%llu,%llu],\"size\":[[%u,%u],[%u,%u]],\"planes\":[[%.9g,%.9g],[%.9g,%.9g]]}",
        &[
            U(serial),
            U(c.native_frame as u64),
            I(c.synthetic as i32),
            I((use_depth && end_result >= 0) as i32),
            U(reason as u64),
            U(pose_ids[0]),
            U(pose_ids[1]),
            U(p.eyes[0].width as u64),
            U(p.eyes[0].height as u64),
            U(p.eyes[1].width as u64),
            U(p.eyes[1].height as u64),
            F(p.eyes[0].near_z as f64),
            F(p.eyes[0].far_z as f64),
            F(p.eyes[1].near_z as f64),
            F(p.eyes[1].far_z as f64),
        ],
    );
    let bytes = text.as_bytes_with_nul();
    // SAFETY: the C++ half's exported report and its sequence counter.
    unsafe {
        CyberpunkVR_StereoDepthReportSeq.fetch_add(1, AcqRel);
        let report = &mut *(&raw mut CyberpunkVR_StereoDepthReport);
        let n = bytes.len().min(report.len());
        report[..n].copy_from_slice(&bytes[..n]);
        report[report.len() - 1] = 0;
        CyberpunkVR_StereoDepthReportSeq.fetch_add(1, Release);
    }
}

/// The runtime supports the depth layer (m_depthLayerSupported).
#[no_mangle]
pub extern "C" fn cvr_xr_depth_supported() -> bool {
    SUPPORTED.load(Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_xr_depth_set_supported(value: bool) {
    SUPPORTED.store(value, Relaxed);
}
/// An eye's depth swapchain (XrSwapchain), or 0.
#[no_mangle]
pub extern "C" fn cvr_xr_depth_swapchain(eye: u32) -> u64 {
    layer().eyes.get(eye as usize).map_or(0, |e| e.handle)
}
/// The depth format enumerated again with the next swapchain (m_depthSwapchainFormat = 0).
#[no_mangle]
pub extern "C" fn cvr_xr_depth_forget_format() {
    layer().format = 0;
}
/// The eyes' depth swapchains destroyed (with the colour swapchains).
#[no_mangle]
pub extern "C" fn cvr_xr_depth_destroy_swapchains() {
    let mut l = layer();
    for e in l.eyes.iter_mut() {
        e.destroy();
    }
}
/// The renderer's finished slots let go (Renderer::Poll).
#[no_mangle]
pub extern "C" fn cvr_xr_depth_poll() {
    let l = layer();
    if !l.renderer.is_null() {
        // SAFETY: the layer's renderer.
        unsafe { cvr_gpu_stereo_depth_poll(l.renderer) };
    }
}
/// Every record's submit fence passed (Renderer::Idle).
#[no_mangle]
pub extern "C" fn cvr_xr_depth_idle() -> bool {
    layer().idle()
}
/// The renderer freed (with the manager).
#[no_mangle]
pub extern "C" fn cvr_xr_depth_shutdown() {
    let mut l = layer();
    let r = core::mem::replace(&mut l.renderer, core::ptr::null_mut());
    // SAFETY: the layer's renderer, freed once.
    unsafe { cvr_gpu_stereo_depth_free(r) };
}
