//! The frame generation's statistics panel as an XR quad (formerly src/Framegen/StatsOverlay.cpp's Prepare and
//! Shutdown): a small persistent bitmap (cvr_gpu::framegen_overlay draws it) refreshed four times a second into a swapchain
//! of its own through an upload buffer, the quad placed at the configured corner of the view -- or, following the head,
//! in the local space at the configured cone. Submission only reuses the quad.
//!
//! The C++ class keeps its name and holds one of these (include/Framegen/StatsOverlay.hpp).
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::c_void;
use cvr_body::hud::{cvr_hud_head_yaw, DisplayClock, Follow};
use cvr_gpu::framegen_core::{
    cvr_framegen_active_backend, cvr_framegen_get_settings, cvr_framegen_metrics_enabled,
    cvr_framegen_now_ms, cvr_framegen_statistics, Settings, Statistics,
};
use cvr_gpu::framegen_overlay::{draw, panel_size, Canvas};
use openxr_sys as xr;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12Resource, D3D12_COMMAND_LIST_TYPE_DIRECT,
    D3D12_FENCE_FLAG_NONE, D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_UPLOAD,
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RANGE, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_GENERIC_READ, D3D12_RESOURCE_STATE_RENDER_TARGET,
    D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use xr::Handle as _;

use crate::hud_quad::transition;

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

const R8G8B8A8_UNORM_SRGB: i64 = 29;

/// The hardware telemetry's switch: framegen's metrics.
unsafe extern "C" fn metrics_enabled() -> bool {
    cvr_framegen_metrics_enabled()
}

/// The panel's state (the C++ StatsOverlay's members).
pub struct Overlay {
    swapchain: xr::Swapchain,
    images: Vec<xr::SwapchainImageD3D12KHR>,
    allocator: Option<ID3D12CommandAllocator>,
    list: Option<ID3D12GraphicsCommandList>,
    upload: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    layer: xr::CompositionLayerQuad,
    fence_value: u64,
    next_update: f64,
    acquired_index: u32,
    acquired: bool,
    published: bool,
    width: u32,
    height: u32,
    row_pitch: u32,
    pixels: Vec<u32>,
    follow: Follow,
    clock: DisplayClock,
    anchor: xr::Posef,
    origin_serial: u64,
    follow_mode: i32,
}

fn identity() -> xr::Posef {
    xr::Posef {
        orientation: xr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        position: xr::Vector3f {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
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
        pose: identity(),
        size: xr::Extent2Df {
            width: 0.0,
            height: 0.0,
        },
    }
}
fn reset_follow(f: &mut Follow) {
    f.valid = false;
    f.following = false;
    f.reset_delay();
}

impl Overlay {
    fn new() -> Overlay {
        Overlay {
            swapchain: xr::Swapchain::NULL,
            images: Vec::new(),
            allocator: None,
            list: None,
            upload: None,
            fence: None,
            layer: empty_layer(),
            fence_value: 0,
            next_update: 0.0,
            acquired_index: 0,
            acquired: false,
            published: false,
            width: 0,
            height: 0,
            row_pitch: 0,
            pixels: Vec::new(),
            follow: Follow::default(),
            clock: DisplayClock::default(),
            anchor: identity(),
            origin_serial: 0,
            follow_mode: -1,
        }
    }
    fn completed(&self) -> u64 {
        // SAFETY: a fence of ours.
        self.fence
            .as_ref()
            .map_or(u64::MAX, |f| unsafe { f.GetCompletedValue() })
    }

    /// The swapchain, the copy objects and the upload buffer for a panel of `width` x `height`.
    #[cfg(not(test))]
    unsafe fn create(
        &mut self,
        session: xr::Session,
        device: &ID3D12Device,
        width: u32,
        height: u32,
    ) -> bool {
        self.width = width;
        self.height = height;
        self.row_pitch = (width * 4 + 255) & !255;
        self.pixels.resize((width * height) as usize, 0);
        unsafe {
            let mut count = 0u32;
            if xrEnumerateSwapchainFormats(session, 0, &mut count, core::ptr::null_mut()).into_raw()
                < 0
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
            if !formats.contains(&R8G8B8A8_UNORM_SRGB) {
                return false;
            }
            let info = xr::SwapchainCreateInfo {
                ty: xr::StructureType::SWAPCHAIN_CREATE_INFO,
                next: core::ptr::null(),
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::TRANSFER_DST
                    | xr::SwapchainUsageFlags::SAMPLED
                    | xr::SwapchainUsageFlags::COLOR_ATTACHMENT,
                format: R8G8B8A8_UNORM_SRGB,
                sample_count: 1,
                width,
                height,
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
            {
                self.shutdown(false);
                return false;
            }
            self.images = vec![
                xr::SwapchainImageD3D12KHR {
                    ty: xr::StructureType::SWAPCHAIN_IMAGE_D3D12_KHR,
                    next: core::ptr::null_mut(),
                    texture: core::ptr::null_mut(),
                };
                count as usize
            ];
            let created = xrEnumerateSwapchainImages(
                self.swapchain,
                count,
                &mut count,
                self.images.as_mut_ptr().cast(),
            )
            .into_raw()
                >= 0
                && (|| -> windows::core::Result<()> {
                    let allocator: ID3D12CommandAllocator =
                        device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
                    let list: ID3D12GraphicsCommandList = device.CreateCommandList(
                        0,
                        D3D12_COMMAND_LIST_TYPE_DIRECT,
                        &allocator,
                        None,
                    )?;
                    let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
                    list.Close()?;
                    self.allocator = Some(allocator);
                    self.list = Some(list);
                    self.fence = Some(fence);
                    Ok(())
                })()
                .is_ok();
            if !created {
                self.shutdown(false);
                return false;
            }
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: (self.row_pitch * height) as u64,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Format: DXGI_FORMAT_UNKNOWN,
                ..Default::default()
            };
            let heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            };
            let mut upload: Option<ID3D12Resource> = None;
            if device
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    None,
                    &mut upload,
                )
                .is_err()
            {
                self.shutdown(false);
                return false;
            }
            self.upload = upload;
        }
        true
    }

    /// The panel drawn into the acquired image and copied there, every 250 ms.
    #[cfg(not(test))]
    unsafe fn refresh(&mut self, queue: &ID3D12CommandQueue, settings: &Settings, now: f64) {
        if !((now >= self.next_update || self.acquired) && self.completed() >= self.fence_value) {
            return;
        }
        unsafe {
            if !self.acquired {
                let info = xr::SwapchainImageAcquireInfo {
                    ty: xr::StructureType::SWAPCHAIN_IMAGE_ACQUIRE_INFO,
                    next: core::ptr::null(),
                };
                if xrAcquireSwapchainImage(self.swapchain, &info, &mut self.acquired_index)
                    .into_raw()
                    >= 0
                {
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
            let mut stats = Statistics::default();
            cvr_framegen_statistics(&mut stats);
            let (Some(upload), Some(allocator), Some(list), Some(fence)) =
                (&self.upload, &self.allocator, &self.list, &self.fence)
            else {
                return;
            };
            let mut data: *mut c_void = core::ptr::null_mut();
            let none = D3D12_RANGE { Begin: 0, End: 0 };
            if upload.Map(0, Some(&none), Some(&mut data)).is_ok() {
                let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
                windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut time);
                let (w, h) = (self.width as i32, self.height as i32);
                let mut canvas = Canvas {
                    pixels: &mut self.pixels,
                    width: w,
                    height: h,
                };
                draw(
                    &mut canvas,
                    settings,
                    &stats,
                    (time.wHour as u32, time.wMinute as u32),
                    cvr_framegen_active_backend(),
                );
                for row in 0..self.height as usize {
                    core::ptr::copy_nonoverlapping(
                        self.pixels.as_ptr().add(row * self.width as usize),
                        (data as *mut u8).add(row * self.row_pitch as usize) as *mut u32,
                        self.width as usize,
                    );
                }
                upload.Unmap(0, None);
                let target_raw = self.images[self.acquired_index as usize].texture;
                if let (Ok(()), Ok(()), Some(target)) = (
                    allocator.Reset(),
                    list.Reset(allocator, None),
                    ID3D12Resource::from_raw_borrowed(&(target_raw as *mut c_void)),
                ) {
                    list.ResourceBarrier(&[transition(
                        target,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                    )]);
                    let src = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: core::mem::transmute_copy(upload),
                        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                                Offset: 0,
                                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                    Format: target.GetDesc().Format,
                                    Width: self.width,
                                    Height: self.height,
                                    Depth: 1,
                                    RowPitch: self.row_pitch,
                                },
                            },
                        },
                    };
                    let dst = D3D12_TEXTURE_COPY_LOCATION {
                        pResource: core::mem::transmute_copy(target),
                        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                            SubresourceIndex: 0,
                        },
                    };
                    list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
                    list.ResourceBarrier(&[transition(
                        target,
                        D3D12_RESOURCE_STATE_COPY_DEST,
                        D3D12_RESOURCE_STATE_RENDER_TARGET,
                    )]);
                    if list.Close().is_ok() {
                        let lists = [Some(
                            list.cast::<ID3D12CommandList>().expect("a command list"),
                        )];
                        queue.ExecuteCommandLists(&lists);
                        self.fence_value += 1;
                        let _ = queue.Signal(fence, self.fence_value);
                        self.published = true;
                        self.next_update = now + 250.0;
                    }
                }
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
    }

    /// Prepare: the quad for this frame, or none.
    #[cfg(not(test))]
    #[allow(clippy::too_many_arguments)]
    unsafe fn prepare(
        &mut self,
        session: xr::Session,
        view: xr::Space,
        device: Option<&ID3D12Device>,
        queue: Option<&ID3D12CommandQueue>,
        local: xr::Space,
        head: Option<&xr::Posef>,
        origin: u64,
        display_time: i64,
    ) -> *const xr::CompositionLayerBaseHeader {
        let mut settings = Settings::default();
        cvr_framegen_get_settings(&mut settings);
        if !settings.overlay {
            if self.fence.is_none() || self.completed() >= self.fence_value {
                unsafe { self.shutdown(false) };
            }
            return core::ptr::null();
        }
        let (Some(device), Some(queue)) = (device, queue) else {
            return core::ptr::null();
        };
        if session == xr::Session::NULL || view == xr::Space::NULL {
            return core::ptr::null();
        }
        // SAFETY: the adapter of a live device.
        unsafe {
            cvr_gpu::hardware_telemetry::cvr_gpu_hw_telemetry_configure(
                device.as_raw(),
                metrics_enabled,
            )
        };
        let (wanted_width, wanted_height) = panel_size(settings.overlay_layout);
        if self.swapchain != xr::Swapchain::NULL
            && (self.width != wanted_width || self.height != wanted_height)
            && self.completed() >= self.fence_value
        {
            unsafe { self.shutdown(false) };
        }
        if self.swapchain == xr::Swapchain::NULL
            && !unsafe { self.create(session, device, wanted_width, wanted_height) }
        {
            return core::ptr::null();
        }
        unsafe { self.refresh(queue, &settings, cvr_framegen_now_ms()) };
        if !self.published {
            return core::ptr::null();
        }
        self.layer.space = view;
        self.layer.eye_visibility = xr::EyeVisibility::BOTH;
        self.layer.layer_flags = xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA;
        self.layer.sub_image = xr::SwapchainSubImage {
            swapchain: self.swapchain,
            image_rect: xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: self.width as i32,
                    height: self.height as i32,
                },
            },
            image_array_index: 0,
        };
        const POSITIONS: [[f32; 2]; 9] = [
            [-18.0, 18.0],
            [18.0, 18.0],
            [-18.0, -18.0],
            [18.0, -18.0],
            [0.0, 18.0],
            [0.0, -18.0],
            [-18.0, 0.0],
            [18.0, 0.0],
            [0.0, 0.0],
        ];
        const RADIANS: f32 = 0.01745329252;
        let distance = settings.overlay_distance;
        let position = POSITIONS[settings.overlay_corner.clamp(0, 8) as usize];
        let x = ((position[0] + settings.overlay_x).clamp(-70.0, 70.0) * RADIANS).tan() * distance;
        let y = ((position[1] + settings.overlay_y).clamp(-60.0, 60.0) * RADIANS).tan() * distance;
        self.layer.pose = xr::Posef {
            orientation: xr::Quaternionf {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            position: xr::Vector3f { x, y, z: -distance },
        };
        if self.origin_serial != origin || self.follow_mode != settings.overlay_follow {
            reset_follow(&mut self.follow);
            self.clock = DisplayClock::default();
            self.origin_serial = origin;
        }
        let local_ok = local != xr::Space::NULL;
        if settings.overlay_follow != 0 && local_ok && head.is_some() {
            let h = head.expect("checked");
            let q = h.orientation;
            let head_yaw = cvr_hud_head_yaw(q.x, q.y, q.z, q.w, self.follow.yaw);
            let dt = self.clock.step(display_time);
            let yaw = self
                .follow
                .update(head_yaw, settings.overlay_cone * RADIANS, dt, false);
            self.anchor.orientation = xr::Quaternionf {
                x: 0.0,
                y: (yaw * 0.5).sin(),
                z: 0.0,
                w: (yaw * 0.5).cos(),
            };
            self.anchor.position = xr::Vector3f {
                x: h.position.x + yaw.cos() * x - yaw.sin() * distance,
                y: h.position.y + y,
                z: h.position.z - yaw.sin() * x - yaw.cos() * distance,
            };
            self.layer.pose = self.anchor;
            self.layer.space = local;
        } else if settings.overlay_follow != 0 && local_ok {
            if !self.follow.valid {
                return core::ptr::null();
            }
            self.layer.pose = self.anchor;
            self.layer.space = local;
        }
        self.follow_mode = settings.overlay_follow;
        let angle = match settings.overlay_layout {
            0 => 36.0f32,
            1 => 25.0,
            _ => 22.0,
        };
        let w = 2.0 * distance * (angle * RADIANS * 0.5).tan() * settings.overlay_scale;
        self.layer.size = xr::Extent2Df {
            width: w,
            height: w * self.height as f32 / self.width as f32,
        };
        &self.layer as *const xr::CompositionLayerQuad as *const xr::CompositionLayerBaseHeader
    }

    /// Shutdown: the swapchain and the copy objects let go once the GPU is done with them (`wait`: up to a second).
    #[cfg(not(test))]
    unsafe fn shutdown(&mut self, wait: bool) {
        if self.swapchain == xr::Swapchain::NULL && self.fence.is_none() && self.pixels.is_empty() {
            return;
        }
        if let (true, Some(fence)) = (wait, &self.fence) {
            if self.completed() < self.fence_value {
                // SAFETY: an event of ours for the fence's completion.
                unsafe {
                    let event = windows_sys::Win32::System::Threading::CreateEventW(
                        core::ptr::null(),
                        0,
                        0,
                        core::ptr::null(),
                    );
                    if !event.is_null() {
                        if fence
                            .SetEventOnCompletion(
                                self.fence_value,
                                windows::Win32::Foundation::HANDLE(event),
                            )
                            .is_ok()
                        {
                            windows_sys::Win32::System::Threading::WaitForSingleObject(event, 1000);
                        }
                        windows_sys::Win32::Foundation::CloseHandle(event);
                    }
                }
            }
        }
        if self.fence.is_some() && self.completed() < self.fence_value {
            return;
        }
        if self.acquired && self.swapchain != xr::Swapchain::NULL {
            unsafe {
                let ready = xr::SwapchainImageWaitInfo {
                    ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO,
                    next: core::ptr::null(),
                    timeout: xr::Duration::from_nanos(0),
                };
                if xrWaitSwapchainImage(self.swapchain, &ready) != xr::Result::SUCCESS {
                    return;
                }
                let release = xr::SwapchainImageReleaseInfo {
                    ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO,
                    next: core::ptr::null(),
                };
                if xrReleaseSwapchainImage(self.swapchain, &release).into_raw() < 0 {
                    return;
                }
            }
            self.acquired = false;
        }
        if self.swapchain != xr::Swapchain::NULL {
            unsafe { xrDestroySwapchain(self.swapchain) };
            self.swapchain = xr::Swapchain::NULL;
        }
        self.images = Vec::new();
        self.upload = None;
        self.allocator = None;
        self.list = None;
        self.fence = None;
        self.fence_value = 0;
        self.next_update = 0.0;
        self.acquired = false;
        self.published = false;
        self.pixels = Vec::new();
        self.width = 0;
        self.height = 0;
        self.row_pitch = 0;
        reset_follow(&mut self.follow);
        self.clock = DisplayClock::default();
        self.follow_mode = -1;
    }
}

#[no_mangle]
pub extern "C" fn cvr_xr_framegen_overlay_new() -> *mut Overlay {
    Box::into_raw(Box::new(Overlay::new()))
}

/// # Safety
/// `overlay` came from cvr_xr_framegen_overlay_new (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_framegen_overlay_free(overlay: *mut Overlay) {
    if !overlay.is_null() {
        // SAFETY: the caller's.
        drop(unsafe { Box::from_raw(overlay) });
    }
}

/// StatsOverlay::Prepare.
///
/// # Safety
/// The XR handles and D3D12 objects are live or null; `head` is null or a pose.
#[cfg(not(test))]
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_xr_framegen_overlay_prepare(
    overlay: *mut Overlay,
    session: u64,
    view: u64,
    device: *mut c_void,
    queue: *mut c_void,
    local: u64,
    head: *const xr::Posef,
    origin: u64,
    display_time: i64,
) -> *const xr::CompositionLayerBaseHeader {
    // SAFETY: the caller's.
    let Some(overlay) = (unsafe { overlay.as_mut() }) else {
        return core::ptr::null();
    };
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device) };
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) };
    unsafe {
        overlay.prepare(
            xr::Session::from_raw(session),
            xr::Space::from_raw(view),
            device,
            queue,
            xr::Space::from_raw(local),
            head.as_ref(),
            origin,
            display_time,
        )
    }
}

/// StatsOverlay::Shutdown.
///
/// # Safety
/// `overlay` came from cvr_xr_framegen_overlay_new (or is null).
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_framegen_overlay_shutdown(overlay: *mut Overlay, wait: bool) {
    // SAFETY: the caller's.
    if let Some(overlay) = unsafe { overlay.as_mut() } {
        unsafe { overlay.shutdown(wait) };
    }
}
