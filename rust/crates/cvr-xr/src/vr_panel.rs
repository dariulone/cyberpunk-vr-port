//! THE VR MENU'S PANEL (formerly src/Overlay/VrPanel.cpp): the canvases the overlay draws the VR menu into (a pool of
//! three 1440x1080 render targets, each reused only once nobody holds it and its last copy has completed), and the quad
//! layer that shows the latest one in the headset -- copied into an OpenXR swapchain of its own on the producer's
//! queue -- with the pointer's ray as a thin second quad from the controller to the panel.
//!
//! The C++ class keeps its name and holds one of these (include/Overlay/VrPanel.hpp); the overlay (C++ for now)
//! holds a canvas through a lease, the shared_ptr it was.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use std::sync::{Arc, Mutex, MutexGuard};

use cvr_body::cmath::{atan2, cos, sin, sqrt};
use cvr_body::vr_overlay::{self as vrui, CANVAS_HEIGHT, CANVAS_WIDTH};
use cvr_body::xr::{multiply_quat, rotate_vector, Quat, Vec3};
use cvr_gpu::color_blit::Blit;
use openxr_sys as xr;
use std::sync::atomic::{AtomicPtr, Ordering::*};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use xr::Handle as _;

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

/// What the panel asks of the C++ half: the image submission mutex (cvr::camera::ImageSubmissionMutex), held around
/// the copy's ExecuteCommandLists as every XR image submission is.
#[repr(C)]
pub struct PanelRuntime {
    submission_lock: extern "C" fn(),
    submission_unlock: extern "C" fn(),
}
/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    unsafe extern "C" {
        /// The image submission mutex (cvr-host submission.rs).
        safe fn cvr_image_submission_lock();
        safe fn cvr_image_submission_unlock();
    }
    pub(super) static RT: super::PanelRuntime = super::PanelRuntime {
        submission_lock: cvr_image_submission_lock,
        submission_unlock: cvr_image_submission_unlock,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<PanelRuntime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<PanelRuntime> = AtomicPtr::new(core::ptr::null_mut());
#[no_mangle]
pub extern "C" fn cvr_vrui_panel_attach(runtime: *const PanelRuntime) {
    RUNTIME.store(runtime as *mut PanelRuntime, Release);
}

// ---- the canvases ------------------------------------------------------------------------------------------------

/// One canvas: the render target the VR menu is drawn into, its view, and the fence value of its last draw.
pub struct Canvas {
    pub texture: ID3D12Resource,
    pub rtv: ID3D12DescriptorHeap,
    format: DXGI_FORMAT,
    sync: Mutex<CanvasSync>,
}
#[derive(Default)]
struct CanvasSync {
    fence: Option<ID3D12Fence>,
    value: u64,
    serial: u64,
}
impl Canvas {
    fn sync(&self) -> MutexGuard<'_, CanvasSync> {
        self.sync.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Still being drawn or read on the GPU.
    fn busy(&self) -> bool {
        let s = self.sync();
        // SAFETY: a plain fence query.
        s.fence
            .as_ref()
            .is_some_and(|f| unsafe { f.GetCompletedValue() } < s.value)
    }
}
struct Pool {
    slots: [Option<Arc<Canvas>>; 3],
    latest: Option<Arc<Canvas>>,
    serial: u64,
}
static POOL: Mutex<Pool> = Mutex::new(Pool {
    slots: [None, None, None],
    latest: None,
    serial: 0,
});
fn pool() -> MutexGuard<'static, Pool> {
    POOL.lock().unwrap_or_else(|e| e.into_inner())
}

fn create_canvas(device: &ID3D12Device, format: DXGI_FORMAT) -> Option<Canvas> {
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: CANVAS_WIDTH as u64,
        Height: CANVAS_HEIGHT,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: if format == DXGI_FORMAT_R8G8B8A8_UNORM {
            DXGI_FORMAT_R8G8B8A8_TYPELESS
        } else {
            format
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        ..Default::default()
    };
    let hd = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
        NumDescriptors: 1,
        ..Default::default()
    };
    // SAFETY: plain creation calls on the caller's device.
    unsafe {
        let mut texture: Option<ID3D12Resource> = None;
        device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
                None,
                &mut texture,
            )
            .ok()?;
        let texture = texture?;
        let rtv: ID3D12DescriptorHeap = device.CreateDescriptorHeap(&hd).ok()?;
        let rd = D3D12_RENDER_TARGET_VIEW_DESC {
            Format: format,
            ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        device.CreateRenderTargetView(
            &texture,
            Some(&rd),
            rtv.GetCPUDescriptorHandleForHeapStart(),
        );
        Some(Canvas {
            texture,
            rtv,
            format,
            sync: Mutex::new(CanvasSync::default()),
        })
    }
}

/// cvr::vrui::AcquireCanvas: a free canvas of this format -- one nobody else holds and whose last draw has completed --
/// made when its slot is empty; none when all three are taken.
pub fn acquire_canvas(device: &ID3D12Device, format: DXGI_FORMAT) -> Option<Arc<Canvas>> {
    let mut p = pool();
    for slot in p.slots.iter_mut() {
        if slot
            .as_ref()
            .is_some_and(|c| Arc::strong_count(c) != 1 || c.busy())
        {
            continue;
        }
        if slot.as_ref().is_some_and(|c| c.format != format) {
            *slot = None;
        }
        if slot.is_none() {
            *slot = Some(Arc::new(create_canvas(device, format)?));
        }
        return slot.clone();
    }
    None
}
/// cvr::vrui::PublishCanvas: the canvas drawn, done at `value` on `fence`; the panel shows it next.
pub fn publish_canvas(canvas: &Arc<Canvas>, fence: &ID3D12Fence, value: u64) {
    let mut p = pool();
    p.serial += 1;
    let serial = p.serial;
    {
        let mut s = canvas.sync();
        s.fence = Some(fence.clone());
        s.value = value;
        s.serial = serial;
    }
    p.latest = Some(canvas.clone());
}
/// cvr::vrui::ClearCanvas: nothing to show, and the canvases nobody holds are let go.
pub fn clear_canvas() {
    let mut p = pool();
    p.latest = None;
    for slot in p.slots.iter_mut() {
        if slot
            .as_ref()
            .is_some_and(|c| Arc::strong_count(c) == 1 && !c.busy())
        {
            *slot = None;
        }
    }
}

/// The C++ overlay's lease on a canvas (the shared_ptr it held): its texture and view, borrowed for the lease.
///
/// # Safety
/// `device` is an ID3D12Device; the outputs are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_acquire_canvas(
    device: *mut c_void,
    format: i32,
    texture: *mut *mut c_void,
    rtv: *mut *mut c_void,
) -> *const c_void {
    // SAFETY: the caller's device.
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return core::ptr::null();
    };
    let Some(canvas) = acquire_canvas(device, DXGI_FORMAT(format)) else {
        return core::ptr::null();
    };
    // SAFETY: the caller's slots.
    unsafe {
        *texture = canvas.texture.as_raw();
        *rtv = canvas.rtv.as_raw();
    }
    Arc::into_raw(canvas) as *const c_void
}
/// # Safety
/// `lease` is from cvr_vrui_acquire_canvas, `fence` an ID3D12Fence.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_publish_canvas(
    lease: *const c_void,
    fence: *mut c_void,
    value: u64,
) {
    // SAFETY: as the caller promises; the lease stays the caller's.
    unsafe {
        if lease.is_null() {
            return;
        }
        Arc::increment_strong_count(lease as *const Canvas);
        let canvas = Arc::from_raw(lease as *const Canvas);
        if let Some(fence) = ID3D12Fence::from_raw_borrowed(&fence) {
            publish_canvas(&canvas, fence, value);
        }
    }
}
/// # Safety
/// `lease` is from cvr_vrui_acquire_canvas, released once.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_canvas_release(lease: *const c_void) {
    if !lease.is_null() {
        // SAFETY: the lease's own reference.
        drop(unsafe { Arc::from_raw(lease as *const Canvas) });
    }
}
#[no_mangle]
pub extern "C" fn cvr_vrui_clear_canvas() {
    clear_canvas()
}

// ---- the panel ---------------------------------------------------------------------------------------------------

const R8G8B8A8_UNORM_SRGB: i64 = 29;

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}
fn mul(a: Vec3, s: f32) -> Vec3 {
    Vec3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
fn xr_quat(q: Quat) -> xr::Quaternionf {
    xr::Quaternionf {
        x: q.x,
        y: q.y,
        z: q.z,
        w: q.w,
    }
}
fn xr_vec(v: Vec3) -> xr::Vector3f {
    xr::Vector3f {
        x: v.x,
        y: v.y,
        z: v.z,
    }
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
            image_rect: xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: 0,
                    height: 0,
                },
            },
            image_array_index: 0,
        },
        pose: xr::Posef {
            orientation: xr::Quaternionf {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            position: xr::Vector3f {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
        },
        size: xr::Extent2Df {
            width: 0.0,
            height: 0.0,
        },
    }
}

/// cvr::vrui::Panel.
pub struct Panel {
    swapchain: xr::Swapchain,
    images: Vec<*mut c_void>,
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    fence: Option<ID3D12Fence>,
    /// The canvas the copy in flight reads, held until it completes.
    flight: Option<Arc<Canvas>>,
    blit: Blit,
    fence_value: u64,
    last_serial: u64,
    index: u32,
    acquired: bool,
    published: bool,
    ray_visible: bool,
    layer: xr::CompositionLayerQuad,
    ray: xr::CompositionLayerQuad,
}
impl Default for Panel {
    fn default() -> Self {
        Panel {
            swapchain: xr::Swapchain::NULL,
            images: Vec::new(),
            allocator: None,
            list: None,
            fence: None,
            flight: None,
            blit: Blit::default(),
            fence_value: 0,
            last_serial: 0,
            index: 0,
            acquired: false,
            published: false,
            ray_visible: false,
            layer: empty_layer(),
            ray: empty_layer(),
        }
    }
}
impl Panel {
    fn completed(&self) -> u64 {
        // SAFETY: a plain fence query.
        self.fence
            .as_ref()
            .map(|f| unsafe { f.GetCompletedValue() })
            .unwrap_or(0)
    }
    /// The swapchain, its images, the copy's allocator, list and fence; false (torn down again) when any fails.
    unsafe fn create(&mut self, session: xr::Session, device: &ID3D12Device) -> bool {
        let mut count = 0u32;
        if xrEnumerateSwapchainFormats(session, 0, &mut count, core::ptr::null_mut()).into_raw() < 0
        {
            return false;
        }
        let mut formats = vec![0i64; count as usize];
        if xrEnumerateSwapchainFormats(session, count, &mut count, formats.as_mut_ptr()).into_raw()
            < 0
        {
            return false;
        }
        if !formats.contains(&R8G8B8A8_UNORM_SRGB) {
            return false;
        }
        let info = xr::SwapchainCreateInfo {
            ty: xr::StructureType::SWAPCHAIN_CREATE_INFO,
            next: core::ptr::null(),
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED,
            format: R8G8B8A8_UNORM_SRGB,
            sample_count: 1,
            width: CANVAS_WIDTH,
            height: CANVAS_HEIGHT,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        };
        if xrCreateSwapchain(session, &info, &mut self.swapchain).into_raw() < 0 {
            return false;
        }
        if xrEnumerateSwapchainImages(self.swapchain, 0, &mut count, core::ptr::null_mut())
            .into_raw()
            < 0
            || count == 0
        {
            self.shutdown();
            return false;
        }
        let mut images: Vec<xr::SwapchainImageD3D12KHR> = (0..count)
            .map(|_| xr::SwapchainImageD3D12KHR {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_D3D12_KHR,
                next: core::ptr::null_mut(),
                texture: core::ptr::null_mut(),
            })
            .collect();
        let ok = xrEnumerateSwapchainImages(
            self.swapchain,
            count,
            &mut count,
            images.as_mut_ptr().cast(),
        )
        .into_raw()
            >= 0
            && {
                self.allocator = device
                    .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                    .ok();
                self.allocator.is_some()
            }
            && {
                let allocator = self.allocator.as_ref().expect("made above");
                self.list = device
                    .CreateCommandList(
                        0,
                        D3D12_COMMAND_LIST_TYPE_DIRECT,
                        allocator,
                        None::<&ID3D12PipelineState>,
                    )
                    .ok();
                self.list.is_some()
            }
            && {
                self.fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).ok();
                self.fence.is_some()
            };
        self.images = images.iter().map(|i| i.texture).collect();
        if !ok {
            self.shutdown();
            return false;
        }
        let _ = self.list.as_ref().expect("made above").Close();
        true
    }

    /// The latest canvas copied into the next swapchain image (when it changed, and the last copy completed), on the
    /// producer's queue under the image submission mutex.
    unsafe fn copy(
        &mut self,
        source: &Arc<Canvas>,
        device: &ID3D12Device,
        queue: &ID3D12CommandQueue,
    ) {
        if !self.acquired {
            let info = xr::SwapchainImageAcquireInfo {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_ACQUIRE_INFO,
                next: core::ptr::null(),
            };
            if xrAcquireSwapchainImage(self.swapchain, &info, &mut self.index).into_raw() >= 0 {
                self.acquired = true;
            }
        }
        if !self.acquired {
            return;
        }
        let wait = xr::SwapchainImageWaitInfo {
            ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO,
            next: core::ptr::null(),
            timeout: xr::Duration::from_nanos(0),
        };
        if xrWaitSwapchainImage(self.swapchain, &wait) != xr::Result::SUCCESS {
            return;
        }
        let (Some(allocator), Some(list), Some(fence)) = (
            self.allocator.clone(),
            self.list.clone(),
            self.fence.clone(),
        ) else {
            return;
        };
        let target = self
            .images
            .get(self.index as usize)
            .copied()
            .unwrap_or(core::ptr::null_mut());
        let recorded = allocator.Reset().is_ok()
            && list.Reset(&allocator, None::<&ID3D12PipelineState>).is_ok()
            && self.blit.ensure(
                device,
                DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                CANVAS_WIDTH,
                CANVAS_HEIGHT,
            )
            && match ID3D12Resource::from_raw_borrowed(&target) {
                Some(dst) => self.blit.record_overlay(
                    &list,
                    &source.texture,
                    dst,
                    4,
                    false,
                    [0.0; 2],
                    [0.0; 2],
                    None,
                    0,
                    0,
                    0,
                    0.0,
                ),
                None => false,
            }
            && list.Close().is_ok();
        if recorded {
            let rt = &*RUNTIME.load(Acquire);
            (rt.submission_lock)();
            let lists = [Some(
                list.cast::<ID3D12CommandList>().expect("a command list"),
            )];
            queue.ExecuteCommandLists(&lists);
            self.flight = Some(source.clone());
            self.fence_value += 1;
            if queue.Signal(&fence, self.fence_value).is_ok() {
                self.last_serial = source.sync().serial;
                self.published = true;
            }
            (rt.submission_unlock)();
        }
        let release = xr::SwapchainImageReleaseInfo {
            ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO,
            next: core::ptr::null(),
        };
        if xrReleaseSwapchainImage(self.swapchain, &release).into_raw() < 0 {
            self.published = false;
        }
        self.acquired = false;
    }

    /// Panel::Prepare: the panel's quad layer for this frame, or none (the menu closed, nothing drawn yet).
    pub unsafe fn prepare(
        &mut self,
        session: xr::Session,
        space: xr::Space,
        device: Option<&ID3D12Device>,
        queue: Option<&ID3D12CommandQueue>,
    ) -> *const xr::CompositionLayerBaseHeader {
        self.ray_visible = false;
        if !vrui::cvr_vrui_visible() {
            self.shutdown();
            return core::ptr::null();
        }
        let mut view: vrui::View = core::mem::zeroed();
        vrui::cvr_vrui_get_view(&mut view);
        let (Some(device), Some(queue)) = (device, queue) else {
            return core::ptr::null();
        };
        if !view.valid {
            return core::ptr::null();
        }
        let Some(source) = pool().latest.clone() else {
            return core::ptr::null();
        };
        // Same queue as the producer. Hold its lease until this copy has completed.
        if self.swapchain == xr::Swapchain::NULL && !self.create(session, device) {
            return core::ptr::null();
        }
        if self.completed() >= self.fence_value {
            self.flight = None;
            if self.last_serial != source.sync().serial || self.acquired {
                self.copy(&source, device, queue);
            }
        }
        if !self.published {
            return core::ptr::null();
        }
        let layer = &mut self.layer;
        layer.space = space;
        layer.eye_visibility = xr::EyeVisibility::BOTH;
        layer.layer_flags = xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA;
        layer.sub_image = xr::SwapchainSubImage {
            swapchain: self.swapchain,
            image_rect: xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: CANVAS_WIDTH as i32,
                    height: CANVAS_HEIGHT as i32,
                },
            },
            image_array_index: 0,
        };
        layer.pose = xr::Posef {
            orientation: xr_quat(view.pose.orientation),
            position: xr_vec(view.pose.position),
        };
        layer.size = xr::Extent2Df {
            width: view.width,
            height: view.height,
        };
        if view.ray {
            let delta = sub(view.ray_end, view.ray_start);
            let length = sqrt(dot(delta, delta));
            if length > 0.02 {
                // Rotate local +Y onto the controller-to-panel segment.
                let d = mul(delta, 1.0 / length);
                let mut q = Quat {
                    x: d.z,
                    y: 0.0,
                    z: -d.x,
                    w: 1.0 + d.y,
                };
                let n = sqrt(q.x * q.x + q.z * q.z + q.w * q.w);
                if n > 0.001 {
                    q.x /= n;
                    q.z /= n;
                    q.w /= n;
                    let normal = rotate_vector(
                        q,
                        Vec3 {
                            x: 0.0,
                            y: 0.0,
                            z: 1.0,
                        },
                    );
                    let mut facing = sub(
                        view.head_position,
                        mul(add(view.ray_start, view.ray_end), 0.5),
                    );
                    facing = sub(facing, mul(d, dot(facing, d)));
                    let f = sqrt(dot(facing, facing));
                    if f > 0.001 {
                        facing = mul(facing, 1.0 / f);
                        let cross = Vec3 {
                            x: normal.y * facing.z - normal.z * facing.y,
                            y: normal.z * facing.x - normal.x * facing.z,
                            z: normal.x * facing.y - normal.y * facing.x,
                        };
                        let angle = atan2(dot(d, cross), dot(normal, facing));
                        let s = sin(angle * 0.5);
                        q = multiply_quat(
                            Quat {
                                x: d.x * s,
                                y: d.y * s,
                                z: d.z * s,
                                w: cos(angle * 0.5),
                            },
                            q,
                        );
                    }
                    self.ray = self.layer;
                    self.ray.sub_image.image_rect = xr::Rect2Di {
                        offset: xr::Offset2Di { x: 0, y: 0 },
                        extent: xr::Extent2Di {
                            width: 3,
                            height: 3,
                        },
                    };
                    self.ray.pose = xr::Posef {
                        orientation: xr_quat(q),
                        position: xr_vec(mul(add(view.ray_start, view.ray_end), 0.5)),
                    };
                    self.ray.size = xr::Extent2Df {
                        width: 0.003,
                        height: length,
                    };
                    self.ray_visible = true;
                }
            }
        }
        (&self.layer as *const xr::CompositionLayerQuad).cast()
    }

    /// Panel::Ray: the pointer's quad this frame, or none.
    pub fn ray(&self) -> *const xr::CompositionLayerBaseHeader {
        if self.ray_visible {
            (&self.ray as *const xr::CompositionLayerQuad).cast()
        } else {
            core::ptr::null()
        }
    }

    /// Panel::Shutdown: the swapchain and the copy's objects let go once the last copy completed (an acquired image
    /// released first); the canvases cleared.
    pub unsafe fn shutdown(&mut self) {
        if self.fence.is_some() && self.completed() < self.fence_value {
            return;
        }
        if self.acquired && self.swapchain != xr::Swapchain::NULL {
            let wait = xr::SwapchainImageWaitInfo {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO,
                next: core::ptr::null(),
                timeout: xr::Duration::from_nanos(0),
            };
            if xrWaitSwapchainImage(self.swapchain, &wait) != xr::Result::SUCCESS {
                return;
            }
            let release = xr::SwapchainImageReleaseInfo {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO,
                next: core::ptr::null(),
            };
            if xrReleaseSwapchainImage(self.swapchain, &release).into_raw() < 0 {
                return;
            }
            self.acquired = false;
        }
        if self.swapchain != xr::Swapchain::NULL {
            xrDestroySwapchain(self.swapchain);
        }
        self.swapchain = xr::Swapchain::NULL;
        self.images.clear();
        self.flight = None;
        self.allocator = None;
        self.list = None;
        self.fence = None;
        self.fence_value = 0;
        self.last_serial = 0;
        self.published = false;
        self.ray_visible = false;
        clear_canvas();
    }
}

#[no_mangle]
pub extern "C" fn cvr_vrui_panel_new() -> *mut Panel {
    Box::into_raw(Box::default())
}
/// # Safety
/// `panel` is from cvr_vrui_panel_new, freed once.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_panel_free(panel: *mut Panel) {
    if !panel.is_null() {
        // SAFETY: the caller's panel.
        drop(unsafe { Box::from_raw(panel) });
    }
}
/// # Safety
/// `panel` is live; `device` and `queue` are an ID3D12Device and an ID3D12CommandQueue, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_panel_prepare(
    panel: *mut Panel,
    session: u64,
    space: u64,
    device: *mut c_void,
    queue: *mut c_void,
) -> *const xr::CompositionLayerBaseHeader {
    // SAFETY: as the caller promises.
    unsafe {
        let Some(panel) = panel.as_mut() else {
            return core::ptr::null();
        };
        panel.prepare(
            xr::Session::from_raw(session),
            xr::Space::from_raw(space),
            ID3D12Device::from_raw_borrowed(&device),
            ID3D12CommandQueue::from_raw_borrowed(&queue),
        )
    }
}
/// # Safety
/// `panel` is live.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_panel_ray(
    panel: *const Panel,
) -> *const xr::CompositionLayerBaseHeader {
    // SAFETY: as the caller promises.
    unsafe { panel.as_ref() }
        .map(|p| p.ray())
        .unwrap_or(core::ptr::null())
}
/// # Safety
/// `panel` is live.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrui_panel_shutdown(panel: *mut Panel) {
    // SAFETY: as the caller promises.
    if let Some(p) = unsafe { panel.as_mut() } {
        unsafe { p.shutdown() }
    }
}
