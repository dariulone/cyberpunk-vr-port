//! A HUD channel's quad layer (formerly src/Runtimes/HudQuad.cpp): the channel's captured canvas copied into an
//! OpenXR swapchain of its own (a ring of fenced command allocators, the source held until its copy completes), and
//! the panel placed for the frame -- in a free-look cone or head-locked, at the channel's distance (an interaction
//! prompt at its object's depth), kept in front of the world and out of the way of a hand, in both eyes at one
//! angular position unless the HUD has stereo depth.
//!
//! The C++ class keeps its name and holds one of these (include/Runtimes/HudQuad.hpp); the canvas comes from the HUD
//! panel's capture (Render/HudPanel.hpp), handed over as a held reference.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::c_void;
use cvr_body::cmath::{clamp, cos, sin, tan};
use cvr_body::xr::{rotate_vector, Quat, Vec3};
use cvr_host::log;
use openxr_sys as xr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12GraphicsCommandList,
    ID3D12PipelineState, ID3D12Resource, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_FENCE_FLAG_NONE, D3D12_RESOURCE_BARRIER,
    D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_STATES, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_RENDER_TARGET,
    D3D12_RESOURCE_TRANSITION_BARRIER,
};
use xr::Handle as _;

use crate::hands::Pose;
use crate::hud::{cvr_hud_head_yaw, DisplayClock, Follow};

/// cvr::hud::Channel.
pub const MAIN: u32 = 0;
pub const INTERACTION: u32 = 1;
pub const BASILISK: u32 = 2;
pub const SURVEILLANCE: u32 = 3;
pub const HIT_MARKER: u32 = 4;
/// The vehicle's minimap: in the HUD at its tile's place, or projected over a car's hood or a bike's bars (main's 0.2.4).
pub const VEHICLE_MINIMAP: u32 = 5;

/// Placement selected for the current minimap layer (1 projected); zero also covers the native fallback.
#[no_mangle]
pub static CyberpunkVR_VehicleMinimapProjected: AtomicU32 = AtomicU32::new(0);

/// A numeric guard, not full layer validation (handles, image rectangles and extensions stay the runtime's
/// responsibility): XrPosef requires unit length within 1 %.
pub fn submittable(quad: &xr::CompositionLayerQuad) -> bool {
    let (p, q) = (quad.pose.position, quad.pose.orientation);
    let norm2 = q.x as f64 * q.x as f64 + q.y as f64 * q.y as f64 + q.z as f64 * q.z as f64 + q.w as f64 * q.w as f64;
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && norm2.is_finite() && norm2 >= 0.99 * 0.99 && norm2 <= 1.01 * 1.01
        && quad.size.width.is_finite() && quad.size.height.is_finite() && quad.size.width > 0.0 && quad.size.height > 0.0
}
/// FilterSubmittableQuads: only the optional layers the caller passes are compacted, in order (the primary world or
/// menu layer is never inferred from a type). Returns how many are kept.
///
/// # Safety
/// `layers` holds `count` layer pointers (or nulls).
pub unsafe fn filter_submittable_quads(layers: *mut *const xr::CompositionLayerBaseHeader, count: u32) -> u32 {
    let mut kept = 0u32;
    for i in 0..count as usize {
        // SAFETY: as the caller vouches.
        let layer = unsafe { *layers.add(i) };
        let Some(header) = (unsafe { layer.as_ref() }) else { continue };
        if header.ty != xr::StructureType::COMPOSITION_LAYER_QUAD || submittable(unsafe { &*(layer as *const xr::CompositionLayerQuad) }) {
            unsafe { *layers.add(kept as usize) = layer };
            kept += 1;
        } else {
            // a bad optional quad must not reject the world submission: dropped, said once (main's 0.2.4)
            static LOGGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                // SAFETY: a quad layer, checked above.
                let q = unsafe { &*(layer as *const xr::CompositionLayerQuad) };
                let (o, p) = (q.pose.orientation, q.pose.position);
                cvr_host::log!(
                    "[hud-panel] invalid quad dropped: eye {} orientation ({},{},{},{}) position ({},{},{}) size {}x{}
",
                    q.eye_visibility.into_raw(),
                    o.x,
                    o.y,
                    o.z,
                    o.w,
                    p.x,
                    p.y,
                    p.z,
                    q.size.width,
                    q.size.height
                );
            }
        }
    }
    kept
}
/// The minimap's quad, held by cvr-render's frame objects beside the five channels'.
static MINIMAP_QUAD: core::sync::atomic::AtomicPtr<Quad> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
pub fn set_minimap_quad(quad: *mut Quad) {
    MINIMAP_QUAD.store(quad, Ordering::Release);
}
pub fn minimap_quad() -> *mut Quad {
    MINIMAP_QUAD.load(Ordering::Acquire)
}
#[cfg(not(test))]
extern "C" {
    /// cvr-rig wheel_grab.rs: a fresh cabin- or bars-locked anchor for this tracking space.
    fn cvr_anim_wheel_read_minimap_anchor(origin: u64, out: *mut cvr_body::vehicle_minimap::VehicleMinimapAnchor) -> bool;
}

/// include/Runtimes/HudClearance.hpp.
pub mod clearance {
    pub const MARGIN: f32 = 0.15;
    pub const MIN_DISTANCE: f32 = 0.35;
    pub const FREE: f32 = 99.0;
    pub const HAND_RADIUS: f32 = 0.08;
    pub const HAND_OUT_SCALE: f32 = 2.0;
    /// In front of the nearest surface, never nearer than the reading minimum nor farther than configured.
    pub fn limit(configured: f32, published: f32) -> f32 {
        if published <= 0.0 {
            return configured;
        }
        let mut l = published - MARGIN;
        if l > configured {
            l = configured;
        }
        if l < MIN_DISTANCE {
            l = MIN_DISTANCE;
        }
        l
    }
    /// The front of a tracked hand inside the panel's field of view (head-local), or FREE.
    pub fn hand_front(x: f32, y: f32, z: f32, tan_h: f32, tan_v: f32) -> f32 {
        let depth = -z;
        if depth <= HAND_RADIUS {
            return FREE;
        }
        let (ax, ay) = (if x < 0.0 { -x } else { x }, if y < 0.0 { -y } else { y });
        if ax - HAND_RADIUS > tan_h * depth || ay - HAND_RADIUS > tan_v * depth {
            return FREE;
        }
        depth - HAND_RADIUS
    }
    /// The out-of-view scale while a hand is in front: quickly out, slowly back.
    pub fn out_step(current: f32, hand_in: bool, dt: f32, out_scale: f32) -> f32 {
        let target = if hand_in { out_scale } else { 1.0 };
        let mut k = (if hand_in { 12.0 } else { 4.0 }) * dt;
        if k > 1.0 {
            k = 1.0;
        }
        if k < 0.0 {
            k = 0.0;
        }
        current + (target - current) * k
    }
    /// The distance towards its limit: a big step in snaps, small ones in are quick, out is slow.
    pub fn step(current: f32, target: f32, dt: f32) -> f32 {
        if current <= 0.0 {
            return target;
        }
        if target < current * 0.6 {
            return target;
        }
        let mut k = (if target < current { 25.0 } else { 2.5 }) * dt;
        if k > 1.0 {
            k = 1.0;
        }
        if k < 0.0 {
            k = 0.0;
        }
        current + (target - current) * k
    }
    /// src/Natives/HudClearance.cpp GrowFactor: a panel brought nearer grows back part of what it lost.
    pub fn grow_factor(configured: f32, distance: f32, grow: f32) -> f32 {
        if !(grow > 0.0) || !(distance > 0.0) || !(configured > distance) {
            return 1.0;
        }
        let g = if grow > 1.0 { 1.0 } else { grow };
        cvr_body::cmath::pow(configured / distance, g)
    }
}
/// include/Runtimes/InteractionDepth.hpp StepDistance: the prompt glides to its object's depth; a new target far
/// from the current one snaps.
pub fn step_distance(current: f32, target: f32, dt: f32) -> f32 {
    if current <= 0.0 || target <= 0.0 {
        return target;
    }
    let ratio = if target > current { target / current } else { current / target };
    if ratio > 1.5 {
        return target;
    }
    let mut k = dt * 10.0;
    if k > 1.0 {
        k = 1.0;
    }
    if k < 0.0 {
        k = 0.0;
    }
    current + (target - current) * k
}

/// The live controls a quad reads (g_liveControls).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub hud_panel: i32,
    pub interaction_panel: i32,
    pub hud_follow_mode: i32,
    pub hud_stereo_depth: i32,
    pub hud_follow_deg: f32,
    pub interaction_follow_deg: f32,
    pub loot_follow_deg: f32,
    pub interaction_distance: f32,
    pub interaction_fov: f32,
    pub hud_fov: f32,
    pub hud_distance: f32,
}
/// The channel's newest captured frame, as the HUD panel's capture (cvr-gpu hud_panel) holds it.
pub use cvr_gpu::hud_panel::FrameView;
/// cvr::hud::PoseDebug (the exported read-out).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseDebug {
    pub frames: u64,
    pub display_time: i64,
    pub head_yaw: f32,
    pub panel_yaw: f32,
    pub dt: f32,
    pub eye_error: f32,
    pub mode: i32,
    pub tracked: i32,
    pub layers: i32,
    pub head: [f32; 3],
    pub left: [f32; 3],
    pub right: [f32; 3],
    pub source_serial: u64,
    pub source_tick: u64,
}
const _: () = assert!(core::mem::size_of::<PoseDebug>() == 96 && core::mem::offset_of!(PoseDebug, source_serial) == 80);

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    /// The GPU stage profile around the copy (Render/GpuStageProfile.hpp): begin returns a batch, scope ends it.
    fn cvr_cpp_gpu_profile_hud_begin(list: *mut c_void) -> *mut c_void;
    fn cvr_cpp_gpu_profile_hud_end(batch: *mut c_void);
    fn cvr_cpp_gpu_profile_submitted(batch: *mut c_void, queue: *mut c_void, fence: *mut c_void, value: u64);
    fn cvr_cpp_gpu_profile_release(batch: *mut c_void);
    static CyberpunkVR_InteractionAnchor: i32;
    static CyberpunkVR_HudClearance: i32;
    static CyberpunkVR_HudClearanceGrow: f32;
    static CyberpunkVR_HudClearanceHands: i32;
    static CyberpunkVR_HudClearanceHandsScale: f32;
    static CyberpunkVR_HudSubmitRing: AtomicU32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_HudPoseDebug: PoseDebug;
    static CyberpunkVR_HudPoseDebugSeq: AtomicU32;
}
#[cfg(not(test))]
extern "system" {
    fn xrEnumerateSwapchainFormats(session: xr::Session, capacity: u32, count: *mut u32, formats: *mut i64) -> xr::Result;
    fn xrCreateSwapchain(session: xr::Session, info: *const xr::SwapchainCreateInfo, swapchain: *mut xr::Swapchain) -> xr::Result;
    fn xrEnumerateSwapchainImages(swapchain: xr::Swapchain, capacity: u32, count: *mut u32, images: *mut xr::SwapchainImageBaseHeader) -> xr::Result;
    fn xrDestroySwapchain(swapchain: xr::Swapchain) -> xr::Result;
    fn xrAcquireSwapchainImage(swapchain: xr::Swapchain, info: *const xr::SwapchainImageAcquireInfo, index: *mut u32) -> xr::Result;
    fn xrWaitSwapchainImage(swapchain: xr::Swapchain, info: *const xr::SwapchainImageWaitInfo) -> xr::Result;
    fn xrReleaseSwapchainImage(swapchain: xr::Swapchain, info: *const xr::SwapchainImageReleaseInfo) -> xr::Result;
}

/// The panel distances and scales the channels share (HudQuad.cpp's file statics).
struct Shared {
    anchor_distance: f32,
    clearance_distance: [f32; 2],
    hand_out_scale: [f32; 2],
}
static SHARED: Mutex<Shared> = Mutex::new(Shared { anchor_distance: 0.0, clearance_distance: [0.0; 2], hand_out_scale: [1.0, 1.0] });

const R8G8B8A8_UNORM_SRGB: i64 = 29;

#[derive(Default)]
struct CopySlot {
    allocator: Option<ID3D12CommandAllocator>,
    commands: Option<ID3D12GraphicsCommandList>,
    /// The source canvas, held until this slot's copy completes.
    frame: *mut c_void,
    fence_value: u64,
}

/// One channel's quad (the C++ cvr::hud::Quad's state).
pub struct Quad {
    channel: u32,
    swapchain: xr::Swapchain,
    images: Vec<*mut c_void>,
    copies: [CopySlot; 3],
    copy_cursor: usize,
    fence: Option<ID3D12Fence>,
    fence_value: u64,
    origin_serial: u64,
    last_stamp: u64,
    /// The published image (the canvas last copied): what the layer shows as its sub-image.
    width: u32,
    height: u32,
    /// The swapchain's, which only ever grows: a canvas that changes size (the minimap's with and without the quest
    /// band under it) is copied into its corner, the swapchain is not remade.
    capacity_w: u32,
    capacity_h: u32,
    /// Per swapchain image: the size last copied into it (w | h << 32).
    image_content: Vec<u64>,
    /// Capacity-sized, zero (transparent): clears an image's old content.
    blank: Option<ID3D12Resource>,
    published_windshield_map: bool,
    published_windshield_split: f32,
    published_hud_rect: [f32; 4],
    acquired_index: u32,
    acquired: bool,
    submission_failed: bool,
    published_masked: bool,
    published_loot_visible: bool,
    published_tick: u64,
    event: windows_sys::Win32::Foundation::HANDLE,
    follow: Follow,
    clock: DisplayClock,
    follow_mode: i32,
    second_eye: bool,
    anchor: Pose,
    last_eye_offsets: [xr::Vector3f; 2],
    eye_offsets_valid: bool,
    layer: xr::CompositionLayerQuad,
    right_layer: xr::CompositionLayerQuad,
}

fn empty_layer() -> xr::CompositionLayerQuad {
    xr::CompositionLayerQuad {
        ty: xr::StructureType::COMPOSITION_LAYER_QUAD,
        next: core::ptr::null(),
        layer_flags: xr::CompositionLayerFlags::EMPTY,
        space: xr::Space::NULL,
        eye_visibility: xr::EyeVisibility::BOTH,
        sub_image: xr::SwapchainSubImage {
            swapchain: xr::Swapchain::NULL,
            image_rect: xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: 0, height: 0 } },
            image_array_index: 0,
        },
        pose: xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } },
        size: xr::Extent2Df { width: 0.0, height: 0.0 },
    }
}

pub(crate) fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: a borrowed reference for the call that records the barrier; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

#[cfg(not(test))]
fn tick_ms() -> u64 {
    // SAFETY: a plain Win32 query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

#[cfg(not(test))]
impl Quad {
    pub fn new(channel: u32) -> Quad {
        Quad {
            channel,
            swapchain: xr::Swapchain::NULL,
            images: Vec::new(),
            copies: Default::default(),
            copy_cursor: 0,
            fence: None,
            fence_value: 0,
            origin_serial: 0,
            last_stamp: 0,
            width: 0,
            height: 0,
            capacity_w: 0,
            capacity_h: 0,
            image_content: Vec::new(),
            blank: None,
            published_windshield_map: false,
            published_windshield_split: 1.0,
            published_hud_rect: [0.0; 4],
            acquired_index: 0,
            acquired: false,
            submission_failed: false,
            published_masked: false,
            published_loot_visible: false,
            published_tick: 0,
            event: core::ptr::null_mut(),
            follow: Follow::default(),
            clock: DisplayClock::default(),
            follow_mode: -1,
            second_eye: false,
            anchor: Pose { orientation: Quat::IDENTITY, position: Vec3::default() },
            last_eye_offsets: [xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 }; 2],
            eye_offsets_valid: false,
            layer: empty_layer(),
            right_layer: empty_layer(),
        }
    }

    fn completed(&self) -> u64 {
        // SAFETY: the quad's own live fence.
        self.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() })
    }
    fn retire_completed(&mut self, completed: u64) {
        for slot in &mut self.copies {
            if !slot.frame.is_null() && slot.fence_value <= completed {
                // SAFETY: the reference this slot held for its copy.
                unsafe { cvr_gpu::hud_panel::frame_release(slot.frame) };
                slot.frame = core::ptr::null_mut();
            }
        }
    }
    fn retire_if_live(&mut self) {
        if self.fence.is_some() && !self.submission_failed {
            let c = self.completed();
            self.retire_completed(c);
        }
    }

    /// Quad::Ensure: a swapchain of the canvas's size (in R8G8B8A8_UNORM_SRGB), its images, the fence, the event and
    /// the copy slots' allocators and lists. A size change waits for the last copy, then recreates the swapchain.
    unsafe fn ensure(&mut self, session: xr::Session, device: &ID3D12Device, w: u32, h: u32) -> bool {
        // SAFETY (whole body): the live session, device and the quad's own objects.
        unsafe {
            if self.submission_failed {
                return false;
            }
            // THE SWAPCHAIN IS NOT REMADE FOR A CANVAS THAT FITS: the minimap's canvas has two sizes (the map alone, with
            // the quest band under it), and remaking it at every switch cost 85-95 ms frames (2026-11-14). It grows to
            // the largest canvas seen, both ways at once.
            if self.swapchain != xr::Swapchain::NULL && w <= self.capacity_w && h <= self.capacity_h {
                return true;
            }
            let (w, h) = (w.max(self.capacity_w), h.max(self.capacity_h));
            if self.fence.is_some() && self.completed() < self.fence_value {
                return false;
            }
            if self.swapchain != xr::Swapchain::NULL {
                if self.acquired {
                    let wait = xr::SwapchainImageWaitInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO, next: core::ptr::null(), timeout: xr::Duration::from_nanos(0) };
                    if xrWaitSwapchainImage(self.swapchain, &wait) != xr::Result::SUCCESS {
                        return false;
                    }
                    let release = xr::SwapchainImageReleaseInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO, next: core::ptr::null() };
                    if xrReleaseSwapchainImage(self.swapchain, &release).into_raw() < 0 {
                        return false;
                    }
                    self.acquired = false;
                }
                xrDestroySwapchain(self.swapchain);
                self.swapchain = xr::Swapchain::NULL;
                self.images.clear();
            }
            let mut count = 0u32;
            if xrEnumerateSwapchainFormats(session, 0, &mut count, core::ptr::null_mut()).into_raw() < 0 {
                return false;
            }
            let mut formats = vec![0i64; count as usize];
            if xrEnumerateSwapchainFormats(session, count, &mut count, formats.as_mut_ptr()).into_raw() < 0 || !formats.contains(&R8G8B8A8_UNORM_SRGB) {
                return false;
            }
            if self.fence.is_none() {
                match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
                    Ok(f) => self.fence = Some(f),
                    Err(_) => return false,
                }
            }
            if self.event.is_null() {
                self.event = windows_sys::Win32::System::Threading::CreateEventW(core::ptr::null(), 0, 0, core::ptr::null());
            }
            if self.event.is_null() {
                return false;
            }
            for slot in &mut self.copies {
                if slot.allocator.is_none() {
                    match device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) {
                        Ok(a) => slot.allocator = Some(a),
                        Err(_) => return false,
                    }
                }
                if slot.commands.is_none() {
                    let Some(a) = slot.allocator.as_ref() else { return false };
                    match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_DIRECT, a, None::<&ID3D12PipelineState>) {
                        Ok(l) => {
                            let _ = l.Close();
                            slot.commands = Some(l);
                        }
                        Err(_) => return false,
                    }
                }
            }
            let info = xr::SwapchainCreateInfo {
                ty: xr::StructureType::SWAPCHAIN_CREATE_INFO,
                next: core::ptr::null(),
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::TRANSFER_DST | xr::SwapchainUsageFlags::SAMPLED | xr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                format: R8G8B8A8_UNORM_SRGB,
                sample_count: 1,
                width: w,
                height: h,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            };
            if xrCreateSwapchain(session, &info, &mut self.swapchain).into_raw() < 0 {
                return false;
            }
            if xrEnumerateSwapchainImages(self.swapchain, 0, &mut count, core::ptr::null_mut()).into_raw() < 0 || count == 0 {
                return false;
            }
            let mut images: Vec<xr::SwapchainImageD3D12KHR> = (0..count)
                .map(|_| xr::SwapchainImageD3D12KHR { ty: xr::StructureType::SWAPCHAIN_IMAGE_D3D12_KHR, next: core::ptr::null_mut(), texture: core::ptr::null_mut() })
                .collect();
            if xrEnumerateSwapchainImages(self.swapchain, count, &mut count, images.as_mut_ptr().cast()).into_raw() < 0 {
                return false;
            }
            self.images = images.iter().map(|i| i.texture).collect();
            // Transparent rows and columns round a smaller canvas, never an older, larger one's: a committed resource is
            // created zeroed.
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: w as u64,
                Height: h,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_TYPELESS,
                SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                ..Default::default()
            };
            self.blank = None;
            let mut blank: Option<ID3D12Resource> = None;
            if device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_SOURCE, None, &mut blank).is_err() {
                return false;
            }
            self.blank = blank;
            self.image_content = vec![0; self.images.len()];
            self.capacity_w = w;
            self.capacity_h = h;
            self.width = 0;
            self.height = 0;
            self.last_stamp = 0;
            self.published_tick = 0;
            self.published_masked = false;
            self.published_loot_visible = false;
            log!("[hud-panel] XR layer ready {}x{} ({} images)\n", w, h, count);
            true
        }
    }

    /// Quad::UpdateImage: the frame's canvas copied into a swapchain image through a free copy slot (pipelined: never
    /// wait for one; otherwise wait up to 100 ms for the last copy). The published image stays valid when no slot is.
    unsafe fn update_image(&mut self, frame: &FrameView, queue: &ID3D12CommandQueue, pipelined: bool, other_queue: bool) -> bool {
        // SAFETY (whole body): the quad's own objects; the frame's held canvas; the runtime's swapchain images.
        unsafe {
            let Some(fence) = self.fence.clone() else { return false };
            let mut completed = fence.GetCompletedValue();
            if completed == u64::MAX {
                self.submission_failed = true;
                return false;
            }
            if !pipelined && completed < self.fence_value {
                use windows::Win32::Foundation::HANDLE;
                if fence.SetEventOnCompletion(self.fence_value, HANDLE(self.event)).is_err()
                    || windows_sys::Win32::System::Threading::WaitForSingleObject(self.event, 100) != 0
                {
                    return false;
                }
                completed = fence.GetCompletedValue();
                if completed == u64::MAX {
                    self.submission_failed = true;
                    return false;
                }
            }
            self.retire_completed(completed);
            if self.last_stamp == frame.serial && !self.acquired {
                return true;
            }
            // A canvas the game's queue has not finished yet is not copied on another queue: the image keeps the last.
            // On the game's own queue (the Rust build has no runtime queue of its own: main's m_xrQueue) the copy is
            // ordered after the capture already, and refusing the newest canvas starved the layer.
            if let Some(producer) = ID3D12Fence::from_raw_borrowed(&frame.fence).filter(|_| other_queue) {
                if producer.GetCompletedValue() < frame.serial {
                    return self.last_stamp != 0;
                }
            }
            let n = self.copies.len();
            let mut free = None;
            for i in 0..n {
                let index = (self.copy_cursor + i) % n;
                if self.copies[index].fence_value <= completed {
                    free = Some(index);
                    self.copy_cursor = (index + 1) % n;
                    break;
                }
            }
            // The published swapchain image is still valid; the source canvas need not be kept past the copy.
            let Some(slot_index) = free else { return self.last_stamp != 0 };
            if !self.acquired {
                let acquire = xr::SwapchainImageAcquireInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_ACQUIRE_INFO, next: core::ptr::null() };
                if xrAcquireSwapchainImage(self.swapchain, &acquire, &mut self.acquired_index).into_raw() < 0 {
                    return false;
                }
                self.acquired = true;
            }
            let wait = xr::SwapchainImageWaitInfo {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO,
                next: core::ptr::null(),
                timeout: xr::Duration::from_nanos(if pipelined { 0 } else { 100_000_000 }),
            };
            let waited = xrWaitSwapchainImage(self.swapchain, &wait);
            if waited == xr::Result::TIMEOUT_EXPIRED {
                return pipelined && self.last_stamp != 0;
            }
            if waited != xr::Result::SUCCESS {
                return false;
            }
            let swapchain = self.swapchain;
            let mut acquired = self.acquired;
            let release = |acquired: &mut bool| {
                let info = xr::SwapchainImageReleaseInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO, next: core::ptr::null() };
                let r = xrReleaseSwapchainImage(swapchain, &info);
                *acquired = false;
                r
            };
            let slot = &mut self.copies[slot_index];
            let (Some(allocator), Some(commands)) = (slot.allocator.clone(), slot.commands.clone()) else {
                release(&mut acquired);
                self.acquired = acquired;
                return false;
            };
            if self.acquired_index as usize >= self.images.len() || allocator.Reset().is_err() || commands.Reset(&allocator, None::<&ID3D12PipelineState>).is_err() {
                release(&mut acquired);
                self.acquired = acquired;
                return false;
            }
            let (Some(target), Some(source)) = (
                ID3D12Resource::from_raw_borrowed(&self.images[self.acquired_index as usize]),
                ID3D12Resource::from_raw_borrowed(&frame.canvas),
            ) else {
                release(&mut acquired);
                self.acquired = acquired;
                return false;
            };
            let profile = cvr_cpp_gpu_profile_hud_begin(commands.as_raw());
            let to_copy = transition(target, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_DEST);
            commands.ResourceBarrier(&[to_copy]);
            let canvas = source.GetDesc();
            let size = canvas.Width | (canvas.Height as u64) << 32;
            let index = self.acquired_index as usize;
            if canvas.Width == self.capacity_w as u64 && canvas.Height == self.capacity_h {
                commands.CopyResource(target, source);
            } else {
                if self.image_content.get(index) != Some(&size) {
                    if let Some(blank) = self.blank.as_ref() {
                        commands.CopyResource(target, blank);
                    }
                }
                let location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
                    pResource: core::mem::transmute_copy(r),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
                };
                commands.CopyTextureRegion(&location(target), 0, 0, 0, &location(source), None);
            }
            if let Some(content) = self.image_content.get_mut(index) {
                *content = size;
            }
            let back = transition(target, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_RENDER_TARGET);
            commands.ResourceBarrier(&[back]);
            cvr_cpp_gpu_profile_hud_end(profile);
            if commands.Close().is_err() {
                cvr_cpp_gpu_profile_release(profile);
                release(&mut acquired);
                self.acquired = acquired;
                return false;
            }
            // The exact source stays alive until THIS slot's copy completes.
            if !slot.frame.is_null() {
                cvr_gpu::hud_panel::frame_release(slot.frame);
            }
            slot.frame = frame.handle;
            self.fence_value += 1;
            slot.fence_value = self.fence_value;
            let lists = [commands.cast::<ID3D12CommandList>().ok()];
            queue.ExecuteCommandLists(&lists);
            let signaled = queue.Signal(&fence, slot.fence_value).is_ok();
            if signaled {
                cvr_cpp_gpu_profile_submitted(profile, queue.as_raw(), fence.as_raw(), slot.fence_value);
            } else {
                cvr_cpp_gpu_profile_release(profile);
                self.submission_failed = true;
            }
            let released = release(&mut acquired);
            self.acquired = acquired;
            if !signaled || released.into_raw() < 0 {
                return false;
            }
            self.last_stamp = frame.serial;
            self.published_tick = frame.stamp;
            self.published_masked = frame.masked;
            self.published_loot_visible = frame.loot_visible;
            self.width = frame.width;
            self.height = frame.height;
            self.published_hud_rect = frame.hud_rect;
            self.published_windshield_map = frame.windshield_map;
            self.published_windshield_split = frame.windshield_split;
            true
        }
    }

    /// Quad::Prepare: the channel's layer for this frame, or null.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn prepare(
        &mut self,
        session: u64,
        space: u64,
        device: *mut c_void,
        queue: *mut c_void,
        enabled: bool,
        tracked: bool,
        head: &Pose,
        origin: u64,
        display_time: i64,
        eye_offsets: *const xr::Vector3f,
        body_yaw: f32,
        main_layer: *const xr::CompositionLayerBaseHeader,
        main_second_eye: *const xr::CompositionLayerBaseHeader,
        completed_source: bool,
    ) -> *const xr::CompositionLayerBaseHeader {
        // SAFETY (whole body): the live session, device and queue; the HUD capture's frame; the C++ half's exports.
        unsafe {
            self.second_eye = false;
            if self.channel == VEHICLE_MINIMAP {
                CyberpunkVR_VehicleMinimapProjected.store(0, Ordering::Relaxed);
            }
            let mut c = Controls::default();
            hud_controls(&mut c);
            let channel = self.channel;
            // The scanner stays at infinity with stereo depth on: its reticle and details are drawn in the eye.
            let stereo_depth = c.hud_stereo_depth != 0 && channel != SURVEILLANCE;
            if !enabled || c.hud_panel == 0 || (channel == INTERACTION && c.interaction_panel == 0) {
                self.retire_if_live();
                // Switched off: the game's slots come back. A menu over the world only pauses the channel.
                if c.hud_panel == 0 || (channel == INTERACTION && c.interaction_panel == 0) {
                    cvr_gpu::hud_panel::cvr_hud_panel_drop_consumer(channel);
                } else {
                    cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready(false, channel);
                }
                self.follow = Follow { valid: false, following: false, ..self.follow };
                self.follow.reset_delay();
                self.clock = DisplayClock::default();
                return core::ptr::null();
            }
            let mut view = FrameView {
                handle: core::ptr::null_mut(),
                canvas: core::ptr::null_mut(),
                width: 0,
                height: 0,
                stamp: 0,
                serial: 0,
                masked: false,
                loot_visible: false,
                empty: false,
                fence: core::ptr::null_mut(),
                windshield_map: false,
                windshield_split: 1.0,
                hud_rect: [0.0; 4],
            };
            let have = cvr_gpu::hud_panel::latest_view(channel, completed_source, &mut view);
            let release_view = |v: &FrameView| {
                if !v.handle.is_null() {
                    cvr_gpu::hud_panel::frame_release(v.handle);
                }
            };
            let eyes_ready = stereo_depth || !eye_offsets.is_null() || self.eye_offsets_valid;
            if have && view.empty {
                // Nothing visible: no layer, but the consumer stays ready so the native slots stay masked.
                release_view(&view);
                self.retire_if_live();
                cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready((tracked || self.follow.valid) && eyes_ready, channel);
                return core::ptr::null();
            }
            let (Some(device), Some(queue)) = (ID3D12Device::from_raw_borrowed(&device), ID3D12CommandQueue::from_raw_borrowed(&queue)) else {
                release_view(&view);
                self.retire_if_live();
                cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready(false, channel);
                return core::ptr::null();
            };
            if !have || view.canvas.is_null() {
                release_view(&view);
                self.retire_if_live();
                cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready(false, channel);
                return core::ptr::null();
            }
            if !self.ensure(xr::Session::from_raw(session), device, view.width, view.height) {
                release_view(&view);
                cvr_gpu::hud_panel::cvr_hud_panel_drop_consumer(channel);
                return core::ptr::null();
            }
            let pipelined = CyberpunkVR_HudSubmitRing.load(Ordering::Relaxed) != 0;
            let updated = self.update_image(&view, queue, pipelined, completed_source);
            // The slot took the frame's reference when it copied it; otherwise it is released here.
            let taken = self.copies.iter().any(|s| s.frame == view.handle);
            if !taken {
                release_view(&view);
            }
            if !updated || self.last_stamp == 0 || tick_ms().wrapping_sub(self.published_tick) >= 300 {
                if self.submission_failed {
                    cvr_gpu::hud_panel::cvr_hud_panel_drop_consumer(channel);
                } else {
                    cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready(false, channel);
                }
                return core::ptr::null();
            }
            if channel == VEHICLE_MINIMAP {
                return self.minimap_layer(space, tracked, origin, main_layer, main_second_eye);
            }
            cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready((tracked || self.follow.valid) && eyes_ready, channel);
            if !eyes_ready || !self.published_masked {
                return core::ptr::null();
            }
            // Loot and its tooltip share the interaction texture: their cone travels with the captured frame.
            let degrees = if channel == INTERACTION {
                if self.published_loot_visible {
                    c.loot_follow_deg
                } else {
                    c.interaction_follow_deg
                }
            } else {
                c.hud_follow_deg
            };
            // A zero interaction or loot cone is the head-locked panel, like the hit marker's.
            let head_locked = channel == BASILISK || channel == SURVEILLANCE || channel == HIT_MARKER || (channel == INTERACTION && degrees <= 0.0);
            let wanted_mode = if head_locked { 2 } else if channel == MAIN { c.hud_follow_mode } else { 0 };
            if self.origin_serial != origin || self.follow_mode != wanted_mode {
                self.follow = Follow { valid: false, following: false, ..self.follow };
                self.follow.reset_delay();
                self.clock = DisplayClock::default();
                self.origin_serial = origin;
                self.follow_mode = wanted_mode;
            }
            let dt = self.clock.step(display_time);
            if tracked {
                let q = head.orientation;
                let head_yaw = cvr_hud_head_yaw(q.x, q.y, q.z, q.w, if self.follow.valid { self.follow.yaw } else { body_yaw });
                let cone = clamp(degrees, 5.0, 90.0) * 0.01745329252;
                let yaw = if head_locked {
                    head_yaw
                } else if self.follow_mode == 1 && body_yaw.is_finite() {
                    body_yaw
                } else {
                    self.follow.update(head_yaw, cone, self.clock.elapsed, channel == MAIN)
                };
                if head_locked || self.follow_mode == 1 {
                    self.follow.yaw = yaw;
                    self.follow.valid = true;
                    self.follow.following = false;
                    self.follow.reset_delay();
                }
                let mut distance = clamp(if channel == INTERACTION { c.interaction_distance } else { c.hud_distance }, 0.5, 5.0);
                let mut shared = SHARED.lock().unwrap_or_else(|p| p.into_inner());
                // The prompt at its object's depth, 10 cm in front of it.
                if channel == INTERACTION && CyberpunkVR_InteractionAnchor != 0 {
                    let depth = crate::hud_natives::interaction_target_depth(tick_ms());
                    if depth > 0.0 {
                        shared.anchor_distance = step_distance(shared.anchor_distance, clamp(depth - 0.10, 0.35, 6.0), dt);
                        distance = shared.anchor_distance;
                    } else {
                        shared.anchor_distance = 0.0;
                    }
                }
                let mut configured = 0.0f32;
                let clear = CyberpunkVR_HudClearance != 0;
                let main_or_prompt = channel == MAIN || channel == INTERACTION;
                // The nearest tracked hand inside this panel's field of view.
                let mut hand_front = clearance::FREE;
                if clear && CyberpunkVR_HudClearanceHands != 0 && main_or_prompt {
                    let panel_fov = clamp(if channel == INTERACTION { c.interaction_fov } else { c.hud_fov }, 30.0, 120.0) * 0.01745329252;
                    let tan_v = tan(panel_fov * 0.5);
                    let tan_h = tan_v * self.width as f32 / self.height as f32;
                    for h in 0..2 {
                        let mut hand = crate::tracking::HeadPose::EMPTY;
                        if crate::tracking::cvr_xr_get_hand_pose(h, &mut hand) {
                            let f = clearance::hand_front(hand.pos[0], hand.pos[1], hand.pos[2], tan_h, tan_v);
                            if f < hand_front {
                                hand_front = f;
                            }
                        }
                    }
                }
                if channel == INTERACTION && shared.anchor_distance > 0.0 && clear {
                    distance = clearance::limit(distance, crate::hud_natives::clearance_published(tick_ms()));
                    shared.clearance_distance[1] = distance;
                } else if main_or_prompt && clear {
                    configured = distance;
                    let i = if channel == MAIN { 0 } else { 1 };
                    let limit = clearance::limit(distance, crate::hud_natives::clearance_published(tick_ms()));
                    shared.clearance_distance[i] = clearance::step(shared.clearance_distance[i], limit, dt);
                    distance = shared.clearance_distance[i];
                }
                self.anchor.orientation = Quat { x: 0.0, y: sin(yaw * 0.5), z: 0.0, w: cos(yaw * 0.5) };
                self.anchor.position = Vec3 { x: head.position.x - sin(yaw) * distance, y: head.position.y, z: head.position.z - cos(yaw) * distance };
                if head_locked {
                    // A zero cone follows the complete head immediately.
                    self.anchor.orientation = head.orientation;
                    let gaze_y = if channel == SURVEILLANCE { crate::gaze::read().0 } else { 0.0 };
                    let o = rotate_vector(head.orientation, Vec3 { x: 0.0, y: distance * gaze_y, z: -distance });
                    self.anchor.position = Vec3 { x: head.position.x + o.x, y: head.position.y + o.y, z: head.position.z + o.z };
                }
                let fov = clamp(if channel == INTERACTION { c.interaction_fov } else { c.hud_fov }, 30.0, 120.0) * 0.01745329252;
                let mut hand_out = 1.0f32;
                if main_or_prompt {
                    let i = if channel == MAIN { 0 } else { 1 };
                    shared.hand_out_scale[i] =
                        clearance::out_step(shared.hand_out_scale[i], hand_front < distance, dt, clamp(CyberpunkVR_HudClearanceHandsScale, 1.0, 3.0));
                    hand_out = shared.hand_out_scale[i];
                }
                let grow = if configured > 0.0 { clearance::grow_factor(configured, distance, CyberpunkVR_HudClearanceGrow) } else { 1.0 };
                let hgt = 2.0 * distance * tan(fov * 0.5) * grow * hand_out;
                self.layer.size = xr::Extent2Df { width: hgt * self.width as f32 / self.height as f32, height: hgt };
            } else {
                self.follow.reset_delay();
                self.clock = DisplayClock::default();
            }
            if !self.follow.valid {
                return core::ptr::null();
            }
            let a = self.anchor;
            self.layer.pose = xr::Posef {
                orientation: xr::Quaternionf { x: a.orientation.x, y: a.orientation.y, z: a.orientation.z, w: a.orientation.w },
                position: xr::Vector3f { x: a.position.x, y: a.position.y, z: a.position.z },
            };
            self.layer.layer_flags = xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA;
            self.layer.space = xr::Space::from_raw(space);
            self.layer.eye_visibility = xr::EyeVisibility::BOTH;
            self.layer.sub_image = xr::SwapchainSubImage {
                swapchain: self.swapchain,
                image_rect: xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: self.width as i32, height: self.height as i32 } },
                image_array_index: 0,
            };
            if !eye_offsets.is_null() && tracked {
                self.last_eye_offsets = [*eye_offsets, *eye_offsets.add(1)];
                self.eye_offsets_valid = true;
            }
            if !stereo_depth && self.eye_offsets_valid {
                // Each eye sees the same angular direction: only the eye translation is cancelled.
                self.right_layer = self.layer;
                self.layer.eye_visibility = xr::EyeVisibility::LEFT;
                self.right_layer.eye_visibility = xr::EyeVisibility::RIGHT;
                let (l, r) = (self.last_eye_offsets[0], self.last_eye_offsets[1]);
                self.layer.pose.position.x += l.x;
                self.layer.pose.position.y += l.y;
                self.layer.pose.position.z += l.z;
                self.right_layer.pose.position.x += r.x;
                self.right_layer.pose.position.y += r.y;
                self.right_layer.pose.position.z += r.z;
                self.second_eye = true;
            }
            if channel == MAIN && core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 {
                CyberpunkVR_HudPoseDebugSeq.fetch_add(1, Ordering::AcqRel);
                let d = &mut *core::ptr::addr_of_mut!(CyberpunkVR_HudPoseDebug);
                d.frames += 1;
                d.display_time = display_time;
                d.panel_yaw = self.follow.yaw;
                d.dt = dt;
                d.source_serial = self.last_stamp;
                d.source_tick = self.published_tick;
                let q = head.orientation;
                d.head_yaw = cvr_hud_head_yaw(q.x, q.y, q.z, q.w, self.follow.yaw);
                d.mode = self.follow_mode;
                d.tracked = tracked as i32;
                d.layers = if self.second_eye { 2 } else { 1 };
                d.head = [head.position.x, head.position.y, head.position.z];
                let lp = self.layer.pose.position;
                let rp = if self.second_eye { self.right_layer.pose.position } else { lp };
                d.left = [lp.x, lp.y, lp.z];
                d.right = [rp.x, rp.y, rp.z];
                d.eye_error = if self.second_eye {
                    let (l, r) = (self.last_eye_offsets[0], self.last_eye_offsets[1]);
                    let x = (lp.x - l.x) - (rp.x - r.x);
                    let y = (lp.y - l.y) - (rp.y - r.y);
                    let z = (lp.z - l.z) - (rp.z - r.z);
                    (x * x + y * y + z * z).sqrt()
                } else {
                    0.0
                };
                CyberpunkVR_HudPoseDebugSeq.fetch_add(1, Ordering::Release);
            }
            (&self.layer as *const xr::CompositionLayerQuad).cast()
        }
    }

    /// The minimap's layer: projected (the anchor the wheel publishes), else at its tile's place on the ordinary HUD
    /// layer (and its second eye); nowhere to show it drops the consumer, and the game's own map comes back.
    ///
    /// # Safety
    /// The layers are null or this frame's quads.
    unsafe fn minimap_layer(&mut self, space: u64, tracked: bool, origin: u64, main_layer: *const xr::CompositionLayerBaseHeader,
        main_second_eye: *const xr::CompositionLayerBaseHeader) -> *const xr::CompositionLayerBaseHeader {
        use cvr_body::vehicle_minimap::{minimap_placement_for, VehicleMinimapAnchor};
        let channel = self.channel;
        let mut cabin = VehicleMinimapAnchor::default();
        // SAFETY: a local anchor.
        let projected = tracked && unsafe { cvr_anim_wheel_read_minimap_anchor(origin, &mut cabin) };
        // SAFETY: the caller's layers.
        let quad_of = |l: *const xr::CompositionLayerBaseHeader| {
            unsafe { l.as_ref() }.filter(|h| h.ty == xr::StructureType::COMPOSITION_LAYER_QUAD).map(|_| unsafe { *(l as *const xr::CompositionLayerQuad) })
        };
        let ordinary = quad_of(main_layer);
        if !projected && ordinary.is_none() {
            cvr_gpu::hud_panel::cvr_hud_panel_drop_consumer(channel); // nowhere to show it: the game's own map
            return core::ptr::null();
        }
        // Not briefly the transparent windshield image in the ordinary HUD on exit (or the opaque one on the glass on
        // entry): the consumer kept ready while the new style reaches us.
        if projected != self.published_windshield_map {
            cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready(true, channel);
            return core::ptr::null();
        }
        let mut layer = empty_layer();
        layer.layer_flags = xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA;
        layer.space = xr::Space::from_raw(space);
        layer.eye_visibility = xr::EyeVisibility::BOTH;
        layer.sub_image = xr::SwapchainSubImage {
            swapchain: self.swapchain,
            image_rect: xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: self.width as i32, height: self.height as i32 } },
            image_array_index: 0,
        };
        let rotate = |q: xr::Quaternionf, v: Vec3| {
            let r = cvr_body::xr::rotate_vector(Quat { x: q.x, y: q.y, z: q.z, w: q.w }, v);
            xr::Vector3f { x: r.x, y: r.y, z: r.z }
        };
        if projected {
            // The map at its placement's size (a car: about 8 x 5 degrees at 1.4 m), the quest text under it: the image
            // grows downward and the map stays where it was.
            let place = minimap_placement_for(cabin.bike);
            let split = if self.published_windshield_split > 0.05 && self.published_windshield_split <= 1.0 { self.published_windshield_split } else { 1.0 };
            let height = place.height / split;
            let q = xr::Quaternionf { x: cabin.orientation.x, y: cabin.orientation.y, z: cabin.orientation.z, w: cabin.orientation.w };
            let down = rotate(q, Vec3 { x: 0.0, y: -(height - place.height) * 0.5, z: 0.0 });
            layer.pose = xr::Posef {
                orientation: q,
                position: xr::Vector3f { x: cabin.position.x + down.x, y: cabin.position.y + down.y, z: cabin.position.z + down.z },
            };
            layer.size = xr::Extent2Df { width: place.width, height };
        } else {
            let r = self.published_hud_rect;
            let place = |main: &xr::CompositionLayerQuad, target: &mut xr::CompositionLayerQuad| {
                target.pose = main.pose;
                target.eye_visibility = main.eye_visibility;
                let offset = rotate(main.pose.orientation, Vec3 {
                    x: (r[0] + r[2] * 0.5 - 0.5) * main.size.width,
                    y: (0.5 - r[1] - r[3] * 0.5) * main.size.height,
                    z: 0.0,
                });
                target.pose.position.x += offset.x;
                target.pose.position.y += offset.y;
                target.pose.position.z += offset.z;
                target.size = xr::Extent2Df { width: r[2] * main.size.width, height: r[3] * main.size.height };
            };
            if let Some(main) = ordinary {
                place(&main, &mut layer);
            }
            if let Some(second) = quad_of(main_second_eye) {
                self.right_layer = layer;
                place(&second, &mut self.right_layer);
                self.second_eye = true;
            }
        }
        self.layer = layer;
        let valid = submittable(&self.layer) && (!self.second_eye || submittable(&self.right_layer));
        cvr_gpu::hud_panel::cvr_hud_panel_set_consumer_ready(valid, channel);
        if !valid || !self.published_masked {
            self.second_eye = false;
            return core::ptr::null();
        }
        CyberpunkVR_VehicleMinimapProjected.store(projected as u32, Ordering::Relaxed);
        (&self.layer as *const xr::CompositionLayerQuad).cast()
    }

    pub fn second_eye(&self) -> *const xr::CompositionLayerBaseHeader {
        if self.second_eye {
            (&self.right_layer as *const xr::CompositionLayerQuad).cast()
        } else {
            core::ptr::null()
        }
    }

    /// Quad::Shutdown: the last copy waited for (a second at most), the image released, the swapchain and the GPU
    /// objects dropped, the held canvases released.
    pub unsafe fn shutdown(&mut self) {
        // SAFETY (whole body): the quad's own objects.
        unsafe {
            if self.channel == VEHICLE_MINIMAP {
                CyberpunkVR_VehicleMinimapProjected.store(0, Ordering::Relaxed);
            }
            cvr_gpu::hud_panel::cvr_hud_panel_drop_consumer(self.channel);
            if let Some(f) = self.fence.clone() {
                if f.GetCompletedValue() < self.fence_value && !self.event.is_null() {
                    use windows::Win32::Foundation::HANDLE;
                    let _ = f.SetEventOnCompletion(self.fence_value, HANDLE(self.event));
                    if windows_sys::Win32::System::Threading::WaitForSingleObject(self.event, 1000) != 0 {
                        return;
                    }
                }
            }
            if self.swapchain != xr::Swapchain::NULL && self.acquired {
                let wait = xr::SwapchainImageWaitInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO, next: core::ptr::null(), timeout: xr::Duration::from_nanos(100_000_000) };
                if xrWaitSwapchainImage(self.swapchain, &wait) != xr::Result::SUCCESS {
                    return;
                }
                let release = xr::SwapchainImageReleaseInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO, next: core::ptr::null() };
                if xrReleaseSwapchainImage(self.swapchain, &release).into_raw() < 0 {
                    return;
                }
                self.acquired = false;
            }
            if self.swapchain != xr::Swapchain::NULL {
                xrDestroySwapchain(self.swapchain);
            }
            self.swapchain = xr::Swapchain::NULL;
            self.acquired = false;
            self.images.clear();
            self.width = 0;
            self.height = 0;
            self.last_stamp = 0;
            self.follow = Follow::default();
            self.clock = DisplayClock::default();
            self.second_eye = false;
            for slot in &mut self.copies {
                if !slot.frame.is_null() {
                    cvr_gpu::hud_panel::frame_release(slot.frame);
                }
                *slot = CopySlot::default();
            }
            self.copy_cursor = 0;
            self.fence = None;
            self.fence_value = 0;
            self.published_tick = 0;
            self.published_masked = false;
            self.published_loot_visible = false;
            self.submission_failed = false;
            if !self.event.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.event);
            }
            self.event = core::ptr::null_mut();
        }
    }
}

/// A new quad for `channel` (the C++ cvr::hud::Quad's constructor).
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_hud_quad_new(channel: u32) -> *mut Quad {
    Box::into_raw(Box::new(Quad::new(channel)))
}
/// The quad freed (its destructor; Shutdown first).
///
/// # Safety
/// `quad` came from cvr_xr_hud_quad_new.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_hud_quad_free(quad: *mut Quad) {
    if !quad.is_null() {
        // SAFETY: the box made by cvr_xr_hud_quad_new.
        drop(unsafe { Box::from_raw(quad) });
    }
}
/// Quad::Prepare.
///
/// # Safety
/// `quad` came from cvr_xr_hud_quad_new; the handles are live; `head` is an XrPosef; `eye_offsets` null or two.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_hud_quad_prepare(
    quad: *mut Quad,
    session: u64,
    space: u64,
    device: *mut c_void,
    queue: *mut c_void,
    enabled: bool,
    tracked: bool,
    head: *const Pose,
    origin: u64,
    display_time: i64,
    eye_offsets: *const xr::Vector3f,
    body_yaw: f32,
    main_layer: *const xr::CompositionLayerBaseHeader,
    main_second_eye: *const xr::CompositionLayerBaseHeader,
    completed_source: bool,
) -> *const xr::CompositionLayerBaseHeader {
    // SAFETY: the caller's quad and head.
    let (Some(q), Some(head)) = (unsafe { quad.as_mut() }, unsafe { head.as_ref() }) else { return core::ptr::null() };
    // SAFETY: as the caller vouches.
    unsafe {
        q.prepare(session, space, device, queue, enabled, tracked, head, origin, display_time, eye_offsets, body_yaw, main_layer, main_second_eye,
            completed_source)
    }
}
/// Quad::SecondEye.
///
/// # Safety
/// `quad` came from cvr_xr_hud_quad_new.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_hud_quad_second_eye(quad: *const Quad) -> *const xr::CompositionLayerBaseHeader {
    // SAFETY: the caller's quad.
    unsafe { quad.as_ref() }.map_or(core::ptr::null(), |q| q.second_eye())
}
/// Quad::Shutdown.
///
/// # Safety
/// `quad` came from cvr_xr_hud_quad_new.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_hud_quad_shutdown(quad: *mut Quad) {
    // SAFETY: the caller's quad.
    if let Some(q) = unsafe { quad.as_mut() } {
        // SAFETY: its own objects.
        unsafe { q.shutdown() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }
    #[test]
    fn the_clearance_asserts_of_the_header() {
        use clearance::*;
        assert!(near(limit(1.5, FREE), 1.5), "open space: as configured");
        let f = hand_front(0.0, 0.0, -0.40, 0.6, 0.6);
        assert!(f < 0.33 && f > 0.31, "a hand 40 cm ahead: its front at 32 cm");
        assert!(hand_front(0.0, -0.60, -0.40, 0.6, 0.6) >= FREE);
        assert!(hand_front(0.0, 0.0, 0.10, 0.6, 0.6) >= FREE);
        assert!(out_step(1.0, true, 1.0, HAND_OUT_SCALE) == HAND_OUT_SCALE && out_step(HAND_OUT_SCALE, false, 1.0, HAND_OUT_SCALE) == 1.0);
        assert!(near(limit(1.5, 0.0), 1.5));
        assert!(near(limit(1.5, 1.0), 0.85));
        assert!(near(limit(1.5, 0.3), MIN_DISTANCE));
        assert!(near(limit(2.02, 5.0), 2.02));
        assert!(near(step(1.5, 0.5, 0.011), 0.5));
        assert!(step(1.5, 1.2, 0.011) < 1.5 && step(1.5, 1.2, 0.011) > 1.2);
        assert!(step(1.0, 1.5, 0.011) - 1.0 < 0.05);
        assert!(near(grow_factor(2.0, 1.0, 0.5), 2f32.sqrt()));
        assert_eq!(grow_factor(1.0, 2.0, 0.5), 1.0, "never shrinks");
    }
    #[test]
    fn the_prompt_glides_to_its_object() {
        assert!(near(step_distance(0.0, 1.2, 0.011), 1.2));
        assert!(near(step_distance(2.0, 0.8, 0.011), 0.8));
        assert!(near(step_distance(0.8, 3.0, 0.011), 3.0));
        let g = step_distance(1.0, 1.2, 0.011);
        assert!(g > 1.0 && g < 1.2);
        assert!(near(step_distance(1.0, 1.2, 1.0), 1.2));
    }
}

// ---- the HUD channels' layers of one frame (FrameThreadMain)

/// What the frame's HUD layers need: the head located this cycle, the menu, the frame generation overlay's switch, the
/// display time, the device, and the five channels' quads (Main, Interaction, Basilisk, Surveillance, HitMarker).
#[repr(C)]
pub struct HudLayersIn {
    pub head_located: bool,
    pub menu_rect: bool,
    /// cvr::framegen::GetSettings().overlay: the panels stay over a menu.
    pub framegen_overlay: bool,
    pub display_time: i64,
    pub device: *mut c_void,
    pub queue: *mut c_void,
    pub quads: [*mut Quad; 5],
    /// The projection layer's left view, for the scanner's diagnostic line.
    pub render_pose: xr::Posef,
    /// The HUD canvases are drawn on the game's queue; copied on another (SteamVR's, or a runtime queue of its own) they
    /// are taken only once finished: the newest finished one (main's 6f33af62c).
    pub completed_source: bool,
}

/// What the HUD layers report back: the Main channel's layer, and the head they were placed against.
#[repr(C)]
pub struct HudLayersOut {
    pub main: *const xr::CompositionLayerBaseHeader,
    pub tracked: bool,
    pub head: xr::Posef,
}

#[cfg(not(test))]
extern "C" {
    /// The body's follow offset (rust/crates/cvr-rig, in the same library).
    fn cvr_body_yaw_follow_offset() -> f32;
}
#[cfg(not(test))]
extern "system" {
    #[link_name = "xrLocateSpace"]
    fn xr_locate_space(space: xr::Space, base: xr::Space, time: xr::Time, location: *mut xr::SpaceLocation) -> xr::Result;
    #[link_name = "xrLocateViews"]
    fn xr_locate_views(session: xr::Session, info: *const xr::ViewLocateInfo, state: *mut xr::ViewState, capacity: u32, count: *mut u32, views: *mut xr::View) -> xr::Result;
}

/// The frame's HUD layers after the projection (or the menu's quad): the head located at the display time (when it was
/// located this cycle, and over a menu only with the frame generation overlay), the eyes' offsets from it, the body's
/// heading (the recenter base's yaw plus the follow offset), then each channel's quad and its second eye, in channel
/// order. Writes at most `capacity` layers to `out` and returns how many; `result` gets the Main channel's layer and
/// the head (with whether it was tracked).
///
/// # Safety
/// `input` and `result` are valid; `out` holds `capacity` pointers.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_hud_layers(input: *const HudLayersIn, out: *mut *const xr::CompositionLayerBaseHeader, capacity: u32, result: *mut HudLayersOut) -> u32 {
    use xr::Handle;
    // SAFETY: the caller's input and result.
    let (Some(inp), Some(res)) = (unsafe { input.as_ref() }, unsafe { result.as_mut() }) else { return 0 };
    res.main = core::ptr::null();
    let session = crate::frame::SESSION.load(Ordering::Acquire);
    let local = crate::frame::LOCAL_SPACE.load(Ordering::Acquire);
    let view = crate::frame::VIEW_SPACE.load(Ordering::Acquire);
    // SAFETY (whole body): the session's live handles; zeroed OpenXR structs with their types; the caller's arrays.
    unsafe {
        let mut head: xr::SpaceLocation = core::mem::zeroed();
        head.ty = xr::StructureType::SPACE_LOCATION;
        let mut views: [xr::View; 2] = core::mem::zeroed();
        for v in views.iter_mut() {
            v.ty = xr::StructureType::VIEW;
        }
        let mut offsets = [xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 }; 2];
        let (mut tracked, mut eyes) = (false, false);
        if inp.head_located && (!inp.menu_rect || inp.framegen_overlay) {
            let valid = xr::SpaceLocationFlags::ORIENTATION_VALID | xr::SpaceLocationFlags::POSITION_VALID;
            tracked = xr_locate_space(xr::Space::from_raw(view), xr::Space::from_raw(local), xr::Time::from_nanos(inp.display_time), &mut head).into_raw() >= 0
                && head.location_flags.contains(valid);
            let info = xr::ViewLocateInfo {
                ty: xr::StructureType::VIEW_LOCATE_INFO,
                next: core::ptr::null(),
                view_configuration_type: xr::ViewConfigurationType::PRIMARY_STEREO,
                display_time: xr::Time::from_nanos(inp.display_time),
                space: xr::Space::from_raw(local),
            };
            let mut state: xr::ViewState = core::mem::zeroed();
            state.ty = xr::StructureType::VIEW_STATE;
            let mut count = 0u32;
            eyes = tracked
                && xr_locate_views(xr::Session::from_raw(session), &info, &mut state, 2, &mut count, views.as_mut_ptr()).into_raw() >= 0
                && count == 2
                && state.view_state_flags.contains(xr::ViewStateFlags::POSITION_VALID);
            if eyes {
                let h = head.pose.position;
                for (o, v) in offsets.iter_mut().zip(views.iter()) {
                    *o = xr::Vector3f { x: v.pose.position.x - h.x, y: v.pose.position.y - h.y, z: v.pose.position.z - h.z };
                }
            }
        }
        let body_yaw = crate::tracking::base_and_origin().map_or(0.0, |(b, _)| {
            let q = b.orientation;
            cvr_body::cmath::atan2(2.0 * (q.w * q.y + q.x * q.z), 1.0 - 2.0 * (q.x * q.x + q.y * q.y))
        }) + cvr_body_yaw_follow_offset();
        let head_pose = Pose {
            orientation: Quat { x: head.pose.orientation.x, y: head.pose.orientation.y, z: head.pose.orientation.z, w: head.pose.orientation.w },
            position: Vec3 { x: head.pose.position.x, y: head.pose.position.y, z: head.pose.position.z },
        };
        res.tracked = tracked;
        res.head = head.pose;
        let origin = crate::tracking::origin_serial();
        let eye_offsets = if eyes { offsets.as_ptr() } else { core::ptr::null() };
        let mut n = 0u32;
        let mut push = |l: *const xr::CompositionLayerBaseHeader| {
            if !l.is_null() && n < capacity {
                *out.add(n as usize) = l;
                n += 1;
            }
        };
        for (i, &q) in inp.quads.iter().enumerate() {
            let layer = cvr_xr_hud_quad_prepare(q, session, local, inp.device, inp.queue, !inp.menu_rect, tracked, &head_pose, origin, inp.display_time, eye_offsets, body_yaw,
                core::ptr::null(), core::ptr::null(), inp.completed_source);
            if layer.is_null() {
                continue;
            }
            if i == 0 {
                res.main = layer;
            }
            push(layer);
            push(cvr_xr_hud_quad_second_eye(q));
            if i == 3 && tracked && eyes && crate::present_diag_enabled() {
                // The scanner's panel against the head, the eye and the rendered view, once a second.
                static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let tick = windows_sys::Win32::System::SystemInformation::GetTickCount64();
                if tick - LAST.load(Ordering::Relaxed) >= 1000 {
                    LAST.store(tick, Ordering::Relaxed);
                    let quad = &*(layer as *const xr::CompositionLayerQuad);
                    let (h, qp, e, p) = (&head.pose, &quad.pose, &views[0].pose, &inp.render_pose);
                    log!(
                        "[scanner-pose] headQ=({:.6},{:.6},{:.6},{:.6}) eyeQ=({:.6},{:.6},{:.6},{:.6}) renderQ=({:.6},{:.6},{:.6},{:.6}) panelQ=({:.6},{:.6},{:.6},{:.6}) delta=({:.6},{:.6},{:.6}) size=({:.4},{:.4}) image={}x{}\n",
                        h.orientation.x, h.orientation.y, h.orientation.z, h.orientation.w,
                        e.orientation.x, e.orientation.y, e.orientation.z, e.orientation.w,
                        p.orientation.x, p.orientation.y, p.orientation.z, p.orientation.w,
                        qp.orientation.x, qp.orientation.y, qp.orientation.z, qp.orientation.w,
                        qp.position.x - e.position.x, qp.position.y - e.position.y, qp.position.z - e.position.z,
                        quad.size.width, quad.size.height, quad.sub_image.image_rect.extent.width, quad.sub_image.image_rect.extent.height
                    );
                }
            }
        }
        // The vehicle's minimap: projected over the hood or the bars, else at its tile's place on the main HUD layer.
        let minimap = minimap_quad();
        if !minimap.is_null() {
            let main_second = if res.main.is_null() { core::ptr::null() } else { cvr_xr_hud_quad_second_eye(inp.quads[0]) };
            let layer = cvr_xr_hud_quad_prepare(minimap, session, local, inp.device, inp.queue, !inp.menu_rect, tracked, &head_pose, origin, inp.display_time,
                eye_offsets, body_yaw, res.main, main_second, inp.completed_source);
            if !layer.is_null() {
                push(layer);
                push(cvr_xr_hud_quad_second_eye(minimap));
            }
        }
        n
    }
}
/// The HUD's live controls, from the C++ half's block (formerly src/Runtimes/HudQuad.cpp's cvr_cpp_hud_controls).
fn hud_controls(o: &mut Controls) {
    use cvr_settings::lc;
    o.hud_panel = lc!(xrHudPanel);
    o.interaction_panel = lc!(xrInteractionPanel);
    o.hud_follow_mode = lc!(xrHudFollowMode);
    o.hud_stereo_depth = lc!(xrHudStereoDepth);
    o.hud_follow_deg = lc!(xrHudFollowDeg);
    o.interaction_follow_deg = lc!(xrInteractionFollowDeg);
    o.loot_follow_deg = lc!(xrLootFollowDeg);
    o.interaction_distance = lc!(xrInteractionDistance);
    o.interaction_fov = lc!(xrInteractionFov);
    o.hud_fov = lc!(xrHudFov);
    o.hud_distance = lc!(xrHudDistance);
}
