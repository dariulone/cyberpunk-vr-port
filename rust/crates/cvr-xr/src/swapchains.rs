//! The eye swapchains (formerly the creation half of OpenXRManager::EnsureMonoSubmitResources,
//! src/Runtimes/OpenXRCapture.cpp): the format the eyes are submitted in (and whether the submit has to convert into
//! it), one swapchain per view at the game's size, its images named for the debug layer, a render-target view for
//! every image, and the submit's own command allocators, command lists and fence.
//!
//! The objects are handed to OpenXRManager, which keeps and releases them as before (the COM references are
//! transferred, not shared). The first D3D12 in Rust: through the `windows` crate, called on the game's device.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use core::ffi::c_void;
use cvr_host::log;
use openxr_sys as xr;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandAllocator, ID3D12DescriptorHeap, ID3D12Device, ID3D12Fence, ID3D12GraphicsCommandList, ID3D12PipelineState, ID3D12Resource,
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, D3D12_FENCE_FLAG_NONE,
    D3D12_RENDER_TARGET_VIEW_DESC, D3D12_RTV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;
use xr::Handle as _;

// DXGI_FORMAT values the pick names.
const R8G8B8A8_TYPELESS: i64 = 27;
const R8G8B8A8_UNORM: i64 = 28;
const R8G8B8A8_UNORM_SRGB: i64 = 29;
const B8G8R8A8_UNORM: i64 = 87;
const B8G8R8A8_TYPELESS: i64 = 90;
const B8G8R8A8_UNORM_SRGB: i64 = 91;
const R16G16B16A16_FLOAT: i64 = 10;

/// include/Runtimes/OpenXRInternal.hpp PickMonoSwapchainFormat: an 8-bit game format takes its bit-compatible
/// _UNORM_SRGB companion (the bytes are display-encoded; a UNORM swapchain would encode them twice), else a format the
/// submit converts into; anything else the game's own format, else the preferred list, else the runtime's first.
/// `test_convert` passes over the companion so the conversion is exercised. Returns (format, convert).
pub fn pick_mono_format(runtime: &[i64], game: i64, test_convert: bool) -> (i64, bool) {
    let has = |f: i64| runtime.contains(&f);
    let companion = match game {
        R8G8B8A8_TYPELESS | R8G8B8A8_UNORM | R8G8B8A8_UNORM_SRGB => R8G8B8A8_UNORM_SRGB,
        B8G8R8A8_TYPELESS | B8G8R8A8_UNORM | B8G8R8A8_UNORM_SRGB => B8G8R8A8_UNORM_SRGB,
        _ => 0,
    };
    if companion != 0 {
        if !test_convert && has(companion) {
            return (companion, false);
        }
        for candidate in [R8G8B8A8_UNORM_SRGB, B8G8R8A8_UNORM_SRGB, R16G16B16A16_FLOAT, R8G8B8A8_UNORM, B8G8R8A8_UNORM] {
            if (!test_convert || candidate != companion) && has(candidate) {
                return (candidate, true);
            }
        }
    }
    if has(game) {
        return (game, false);
    }
    for preferred in [R8G8B8A8_UNORM, R8G8B8A8_UNORM_SRGB, B8G8R8A8_UNORM, B8G8R8A8_UNORM_SRGB, R16G16B16A16_FLOAT] {
        if has(preferred) {
            return (preferred, false);
        }
    }
    (runtime.first().copied().unwrap_or(game), false)
}

/// The pick for OpenXRManager (its runtime list as an array).
///
/// # Safety
/// `formats` holds `count` values; `convert` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_pick_mono_swapchain_format(formats: *const i64, count: u32, game: i64, test_convert: bool, convert: *mut bool) -> i64 {
    let runtime = if formats.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(formats, count as usize) } };
    let (format, c) = pick_mono_format(runtime, game, test_convert);
    // SAFETY: the caller's flag.
    if let Some(out) = unsafe { convert.as_mut() } {
        *out = c;
    }
    format
}

/// The most images a runtime's swapchain may have here (runtimes give two to four).
pub const MAX_IMAGES: usize = 8;
/// One eye's swapchain as OpenXRManager::EyeSwapchain keeps it.
#[repr(C)]
pub struct EyeImages {
    pub handle: u64,
    pub width: i32,
    pub height: i32,
    pub image_count: u32,
    /// ID3D12Resource*, the runtime's (not referenced here).
    pub images: [*mut c_void; MAX_IMAGES],
    /// D3D12_CPU_DESCRIPTOR_HANDLE.ptr of each image's RTV.
    pub rtvs: [usize; MAX_IMAGES],
}
/// What the creation gives OpenXRManager.
#[repr(C)]
pub struct EyeSwapchains {
    pub view_count: u32,
    /// How many eyes got a swapchain (the rest failed: the call returned false).
    pub created: u32,
    pub eyes: [EyeImages; 4],
    /// ID3D12DescriptorHeap*, one reference transferred.
    pub rtv_heap: *mut c_void,
    pub rtv_size: u32,
    /// The first eye image's real geometry (the resource is typeless; the views are typed by the swapchain format).
    pub eye0_width: u32,
    pub eye0_height: u32,
    pub eye0_format: u32,
}

#[cfg(not(test))]
extern "system" {
    fn xrCreateSwapchain(session: xr::Session, info: *const xr::SwapchainCreateInfo, swapchain: *mut xr::Swapchain) -> xr::Result;
    fn xrEnumerateSwapchainImages(swapchain: xr::Swapchain, capacity: u32, count: *mut u32, images: *mut xr::SwapchainImageBaseHeader) -> xr::Result;
}

fn set_name(object: &impl Interface, name: &str) {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a live D3D12 object (every one here derives ID3D12Object) and a NUL-terminated name it copies.
    unsafe {
        if let Ok(o) = object.cast::<windows::Win32::Graphics::Direct3D12::ID3D12Object>() {
            let _ = o.SetName(PCWSTR(wide.as_ptr()));
        }
    }
}

/// One swapchain per view at the game's size in `format`, the images named, their RTVs in one heap.
///
/// # Safety
/// `session` and `device` are live (the device an ID3D12Device*); `out` is valid.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_create_eye_swapchains(session: u64, device: *mut c_void, width: u32, height: u32, format: i64, view_count: u32, out: *mut EyeSwapchains) -> bool {
    // SAFETY (whole body): the live session and device; the runtime's image arrays sized as enumerated.
    unsafe {
        let Some(out) = out.as_mut() else { return false };
        *out = core::mem::zeroed();
        let Some(device) = ID3D12Device::from_raw_borrowed(&device) else { return false };
        let views = (view_count as usize).min(out.eyes.len());
        out.view_count = views as u32;
        for eye in 0..views {
            let info = xr::SwapchainCreateInfo {
                ty: xr::StructureType::SWAPCHAIN_CREATE_INFO,
                next: core::ptr::null(),
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                // SteamVR samples the projection images during composition, as it does the HUD quads (main's 1cba63ec0).
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::TRANSFER_DST | xr::SwapchainUsageFlags::SAMPLED,
                format,
                sample_count: 1,
                width,
                height,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            };
            let mut handle = xr::Swapchain::NULL;
            let res = xrCreateSwapchain(xr::Session::from_raw(session), &info, &mut handle);
            if res.into_raw() < 0 {
                log!("OpenXRManager: Failed to create mono swapchain for eye {} (res={})\n", eye, res.into_raw());
                return false;
            }
            let e = &mut out.eyes[eye];
            e.handle = handle.into_raw();
            e.width = width as i32;
            e.height = height as i32;
            out.created = eye as u32 + 1;
            let mut count = 0u32;
            xrEnumerateSwapchainImages(handle, 0, &mut count, core::ptr::null_mut());
            if count as usize > MAX_IMAGES {
                log!("OpenXRManager: eye {} swapchain has {} images, more than {}\n", eye, count, MAX_IMAGES);
                return false;
            }
            let mut images: [xr::SwapchainImageD3D12KHR; MAX_IMAGES] = core::array::from_fn(|_| xr::SwapchainImageD3D12KHR {
                ty: xr::StructureType::SWAPCHAIN_IMAGE_D3D12_KHR,
                next: core::ptr::null_mut(),
                texture: core::ptr::null_mut(),
            });
            xrEnumerateSwapchainImages(handle, count, &mut count, images.as_mut_ptr().cast());
            e.image_count = count;
            for (im, image) in images.iter().take(count as usize).enumerate() {
                e.images[im] = image.texture;
                // The runtime made these, so nothing has named them: the debug layer's "Unnamed ID3D12Resource".
                if let Some(texture) = ID3D12Resource::from_raw_borrowed(&image.texture) {
                    set_name(texture, &format!("OpenXR_eye{eye}_color_image{im}"));
                }
            }
            // The eye image's real geometry, for the capture thread's right-eye target.
            if eye == 0 && count > 0 {
                if let Some(texture) = ID3D12Resource::from_raw_borrowed(&images[0].texture) {
                    let desc = texture.GetDesc();
                    out.eye0_width = desc.Width as u32;
                    out.eye0_height = desc.Height;
                    out.eye0_format = desc.Format.0 as u32;
                    log!(
                        "OpenXRManager: eye image {}x{} resourceFmt={} viewFmt={}\n",
                        desc.Width as u32,
                        desc.Height,
                        desc.Format.0 as u32,
                        format as u32
                    );
                }
            }
        }
        // Stable descriptors for every runtime-owned image.
        let total: u32 = out.eyes[..views].iter().map(|e| e.image_count).sum();
        let heap_desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: total, ..Default::default() };
        let heap: Option<ID3D12DescriptorHeap> = if total == 0 { None } else { device.CreateDescriptorHeap(&heap_desc).ok() };
        let Some(heap) = heap else {
            log!("OpenXRManager: failed to create eye RTV descriptors\n");
            return false;
        };
        out.rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
        let mut rtv = heap.GetCPUDescriptorHandleForHeapStart();
        let mut view = D3D12_RENDER_TARGET_VIEW_DESC { Format: DXGI_FORMAT(format as i32), ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D, ..Default::default() };
        view.Anonymous.Texture2D = Default::default();
        for e in out.eyes[..views].iter_mut() {
            for im in 0..e.image_count as usize {
                let texture = ID3D12Resource::from_raw_borrowed(&e.images[im]);
                device.CreateRenderTargetView(texture, Some(&view as *const _), rtv);
                e.rtvs[im] = rtv.ptr;
                rtv.ptr += out.rtv_size as usize;
            }
        }
        out.rtv_heap = heap.into_raw();
        true
    }
}

/// The submit's command allocators and lists (three, closed) and its fence, named; the references are transferred.
///
/// # Safety
/// `device` is a live ID3D12Device*; the out arrays hold three pointers each.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_create_submit_objects(device: *mut c_void, allocators: *mut *mut c_void, lists: *mut *mut c_void, fence: *mut *mut c_void) -> bool {
    // SAFETY (whole body): the live device; the caller's three-slot arrays and fence slot.
    unsafe {
        let Some(device) = ID3D12Device::from_raw_borrowed(&device) else { return false };
        let mut made: Vec<ID3D12CommandAllocator> = Vec::with_capacity(3);
        for i in 0..3 {
            match device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) {
                Ok(a) => {
                    set_name(&a, "OpenXR_submit_allocator");
                    made.push(a);
                }
                Err(_) => {
                    log!("OpenXRManager: Failed to create submit command allocator {}\n", i);
                    for (k, a) in made.into_iter().enumerate() {
                        *allocators.add(k) = a.into_raw();
                    }
                    return false;
                }
            }
        }
        for (i, a) in made.iter().enumerate() {
            *allocators.add(i) = a.clone().into_raw();
        }
        for (i, a) in made.iter().enumerate() {
            match device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_DIRECT, a, None::<&ID3D12PipelineState>) {
                Ok(list) => {
                    set_name(&list, "OpenXR_submit_command_list");
                    let _ = list.Close();
                    *lists.add(i) = list.into_raw();
                }
                Err(_) => {
                    log!("OpenXRManager: Failed to create submit command list {}\n", i);
                    return false;
                }
            }
        }
        match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
            Ok(f) => {
                set_name(&f, "OpenXR_submit_fence");
                *fence = f.into_raw();
                true
            }
            Err(_) => {
                log!("OpenXRManager: Failed to create mono fence\n");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_8_bit_backbuffer_takes_its_srgb_companion() {
        let runtime = [R8G8B8A8_UNORM, R8G8B8A8_UNORM_SRGB, B8G8R8A8_UNORM_SRGB];
        assert_eq!(pick_mono_format(&runtime, R8G8B8A8_UNORM, false), (R8G8B8A8_UNORM_SRGB, false));
        assert_eq!(pick_mono_format(&runtime, B8G8R8A8_UNORM, false), (B8G8R8A8_UNORM_SRGB, false));
        // the test marker passes over the companion: the conversion is exercised
        assert_eq!(pick_mono_format(&runtime, R8G8B8A8_UNORM, true), (B8G8R8A8_UNORM_SRGB, true));
        // no companion listed: convert into the first usable
        assert_eq!(pick_mono_format(&[R16G16B16A16_FLOAT, R8G8B8A8_UNORM], R8G8B8A8_UNORM, false), (R16G16B16A16_FLOAT, true));
    }
    #[test]
    fn other_formats_keep_the_old_preference() {
        let hdr = 24; // R10G10B10A2_UNORM
        assert_eq!(pick_mono_format(&[hdr, R8G8B8A8_UNORM], hdr, false), (hdr, false));
        assert_eq!(pick_mono_format(&[B8G8R8A8_UNORM, R8G8B8A8_UNORM_SRGB], hdr, false), (R8G8B8A8_UNORM_SRGB, false));
        assert_eq!(pick_mono_format(&[77], hdr, false), (77, false));
        assert_eq!(pick_mono_format(&[], hdr, false), (hdr, false));
    }
}

// ---- one eye's picture into its swapchain (FrameThreadMain)

/// One eye's copy: its swapchain (the handle, the runtime's images, their render-target views, the size), the submit's
/// command list, the picture, and how it goes in.
#[repr(C)]
pub struct EyeCopy {
    pub swapchain: u64,
    /// XrSwapchainImageD3D12KHR[image_count].
    pub images: *const xr::SwapchainImageD3D12KHR,
    pub image_count: u32,
    /// D3D12_CPU_DESCRIPTOR_HANDLE[rtv_count].
    pub rtvs: *const usize,
    pub rtv_count: u32,
    pub width: i32,
    pub height: i32,
    /// ID3D12GraphicsCommandList*, recording.
    pub list: *mut c_void,
    /// ID3D12Resource*, resting in COPY_SOURCE.
    pub source: *mut c_void,
    /// The swapchain does not declare the eye bytes as sRGB: they are drawn through the decode instead of copied.
    pub convert: bool,
    /// The decode's per-recording slot (allocator * 2 + eye).
    pub decode_slot: u32,
    /// The eyelids, 0 open .. 1 shut (a loading screen or the VR menu's blink).
    pub blink: f32,
    /// The picture is the second eye's own (VRCAM's).
    pub second_eye: bool,
    /// SrgbToLinearPass::Record through the C++ half.
    pub decode: Option<unsafe extern "C" fn(ctx: *mut c_void, list: *mut c_void, source: *mut c_void, target: *mut c_void, slot: u32) -> bool>,
    pub decode_ctx: *mut c_void,
    /// cvr::gpu::profile's SubmitColor scope through the C++ half (open returns the scope, close ends it).
    pub scope_open: Option<unsafe extern "C" fn(profile: *mut c_void) -> *mut c_void>,
    pub scope_close: Option<unsafe extern "C" fn(scope: *mut c_void)>,
    pub profile: *mut c_void,
}

/// How an eye's copy went: the image was not acquired, was acquired but not filled, or was filled.
pub const EYE_NOT_ACQUIRED: i32 = 0;
pub const EYE_ACQUIRED_FAILED: i32 = 1;
pub const EYE_COPIED: i32 = 2;

#[cfg(not(test))]
extern "system" {
    fn xrAcquireSwapchainImage(swapchain: xr::Swapchain, info: *const xr::SwapchainImageAcquireInfo, index: *mut u32) -> xr::Result;
    fn xrWaitSwapchainImage(swapchain: xr::Swapchain, info: *const xr::SwapchainImageWaitInfo) -> xr::Result;
}
#[cfg(not(test))]
#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugStereoEyeSubmits: u64;
}

/// One eye's picture into its swapchain (FrameThreadMain): the next image acquired and waited for (200 ms at most --
/// never hang the frame loop), the picture copied in (or drawn through the sRGB decode when the swapchain is not
/// sRGB; the picture rests in COPY_SOURCE, the image in RENDER_TARGET), and with the eyelids closing their black bands
/// cleared over it. Returns EYE_NOT_ACQUIRED, EYE_ACQUIRED_FAILED or EYE_COPIED.
///
/// # Safety
/// `copy` is valid: live handles, the runtime's image array, a recording command list.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_copy_eye(copy: *const EyeCopy, eye: u32) -> i32 {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12GraphicsCommandList, ID3D12Resource, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_RENDER_TARGET,
    };
    use crate::hud_quad::transition;
    // SAFETY (whole body): the caller's live handles and recording list; the runtime's images as enumerated.
    unsafe {
        let Some(c) = copy.as_ref() else { return EYE_NOT_ACQUIRED };
        let swapchain = xr::Swapchain::from_raw(c.swapchain);
        let mut index = 0u32;
        let acquire = xr::SwapchainImageAcquireInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_ACQUIRE_INFO, next: core::ptr::null() };
        let res = xrAcquireSwapchainImage(swapchain, &acquire, &mut index);
        if res.into_raw() < 0 {
            log!("OpenXRManager: xrAcquireSwapchainImage failed for eye {} (res={})\n", eye, res.into_raw());
            return EYE_NOT_ACQUIRED;
        }
        let wait = xr::SwapchainImageWaitInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_WAIT_INFO, next: core::ptr::null(), timeout: xr::Duration::from_nanos(200_000_000) };
        let res = xrWaitSwapchainImage(swapchain, &wait);
        crate::xr_trace::stage(crate::xr_trace::STAGE_IMAGE);
        if res.into_raw() < 0 || res == xr::Result::TIMEOUT_EXPIRED {
            log!("OpenXRManager: xrWaitSwapchainImage failed/timeout for eye {} (res={})\n", eye, res.into_raw());
            return EYE_ACQUIRED_FAILED;
        }
        let texture = if index < c.image_count { (*c.images.add(index as usize)).texture } else { core::ptr::null_mut() };
        if texture.is_null() {
            log!("OpenXRManager: XR swapchain texture missing for eye {} image {}\n", eye, index);
            return EYE_ACQUIRED_FAILED;
        }
        let texture_raw = texture as *mut c_void;
        let (Some(list), Some(source), Some(target)) = (
            ID3D12GraphicsCommandList::from_raw_borrowed(&c.list),
            ID3D12Resource::from_raw_borrowed(&c.source),
            ID3D12Resource::from_raw_borrowed(&texture_raw),
        ) else {
            return EYE_ACQUIRED_FAILED;
        };
        let scope = match (c.scope_open, c.profile.is_null()) {
            (Some(open), false) => open(c.profile),
            _ => core::ptr::null_mut(),
        };
        let close = |scope: *mut c_void| {
            if let (Some(f), false) = (c.scope_close, scope.is_null()) {
                f(scope);
            }
        };
        if c.convert && c.decode.is_some() {
            list.ResourceBarrier(&[transition(source, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)]);
            let decoded = (c.decode.unwrap())(c.decode_ctx, c.list, c.source, texture_raw, c.decode_slot);
            list.ResourceBarrier(&[transition(source, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            if !decoded {
                log!("OpenXRManager: sRGB decode into the swapchain failed for eye {}\n", eye);
                close(scope);
                return EYE_ACQUIRED_FAILED;
            }
        } else {
            list.ResourceBarrier(&[transition(target, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_DEST)]);
            list.CopyResource(target, source);
            list.ResourceBarrier(&[transition(target, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_RENDER_TARGET)]);
        }
        if c.second_eye && core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0 {
            let p = core::ptr::addr_of_mut!(CyberpunkVR_DebugStereoEyeSubmits);
            core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
        }
        if c.blink > 0.0 && index < c.rtv_count {
            let (w, h) = (c.width, c.height);
            let lid = (h as f32 * 0.5 * c.blink).ceil() as i32;
            let rects = [RECT { left: 0, top: 0, right: w, bottom: lid }, RECT { left: 0, top: h - lid, right: w, bottom: h }];
            let rtv = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: *c.rtvs.add(index as usize) };
            list.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0], Some(&rects));
        }
        close(scope);
        EYE_COPIED
    }
}

// ---- the copies submitted and the eye images released (FrameThreadMain)

/// The cycle's copies: whether they all went in, the acquired color and depth images per eye, the submit's list, queue
/// and fence, and the C++ half's fence counters.
#[repr(C)]
pub struct SubmitCopies {
    pub copy_ready: bool,
    pub view_count: u32,
    pub acquired: [bool; 4],
    pub acquired_depth: [bool; 4],
    pub color: [u64; 4],
    pub depth: [u64; 4],
    /// ID3D12GraphicsCommandList*, ID3D12CommandQueue*, ID3D12Fence*.
    pub list: *mut c_void,
    pub queue: *mut c_void,
    /// The queue the submit fence is signalled on (RuntimeQueue(), main's 0.2.4): the runtime's queue is ordered after
    /// `queue` first (OrderRuntimeAfter). Null: `queue`.
    pub fence_queue: *mut c_void,
    pub fence: *mut c_void,
    /// m_fenceValue and m_cmdAllocatorFence[m_cmdAllocatorIndex].
    pub fence_value: *mut u64,
    pub allocator_fence: *mut u64,
    /// cvr::gpu::profile::Submitted through the C++ half.
    pub submitted: Option<unsafe extern "C" fn(profile: *mut c_void, queue: *mut c_void, fence: *mut c_void, value: u64)>,
    pub profile: *mut c_void,
}
/// What the submission left: the fence value the reads of the captured pictures end at, whether every color image and
/// every depth image was released.
#[repr(C)]
pub struct SubmittedCopies {
    pub read_fence: u64,
    pub release_ok: bool,
    pub depth_ok: bool,
}

#[cfg(not(test))]
extern "system" {
    fn xrReleaseSwapchainImage(swapchain: xr::Swapchain, info: *const xr::SwapchainImageReleaseInfo) -> xr::Result;
}

/// Releases each acquired image of one kind; false when any release failed (logged as `what`).
#[cfg(not(test))]
unsafe fn release_images(s: &SubmitCopies, depth: bool, cleanup: bool) -> bool {
    let mut ok = true;
    let info = xr::SwapchainImageReleaseInfo { ty: xr::StructureType::SWAPCHAIN_IMAGE_RELEASE_INFO, next: core::ptr::null() };
    for eye in 0..(s.view_count as usize).min(4) {
        let (acquired, handle) = if depth { (s.acquired_depth[eye], s.depth[eye]) } else { (s.acquired[eye], s.color[eye]) };
        if !acquired {
            continue;
        }
        // SAFETY: the caller's live swapchain, its image acquired this cycle.
        let res = unsafe { xrReleaseSwapchainImage(xr::Swapchain::from_raw(handle), &info) };
        if res.into_raw() < 0 {
            log!(
                "OpenXRManager: {}xrReleaseSwapchainImage {}failed for eye {} (res={})\n",
                if depth { "[DEPTH] " } else { "" },
                if cleanup { "cleanup " } else { "" },
                eye,
                res.into_raw()
            );
            ok = false;
        }
    }
    ok
}

/// The cycle's command list closed, executed and fenced (the fence value stamped on the allocator and handed to the
/// GPU profile), and the eye images released: before the submission when a copy failed (an image released then may
/// hold nothing of this frame), after it otherwise.
///
/// # Safety
/// `s` and `out` are valid; the list is the recording one, the handles live.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_submit_copies(s: *const SubmitCopies, out: *mut SubmittedCopies) {
    use windows::Win32::Graphics::Direct3D12::{ID3D12CommandList, ID3D12CommandQueue, ID3D12Fence, ID3D12GraphicsCommandList};
    // SAFETY (whole body): the caller's live objects and counters.
    unsafe {
        let (Some(s), Some(out)) = (s.as_ref(), out.as_mut()) else { return };
        out.release_ok = true;
        out.depth_ok = true;
        // Even a partial pair is queued before either image goes back to the compositor (main's 1cba63ec0: SteamVR on
        // Radeon reads the projection images on the bound queue).
        if let (Some(list), Some(queue), Some(fence)) = (
            ID3D12GraphicsCommandList::from_raw_borrowed(&s.list),
            ID3D12CommandQueue::from_raw_borrowed(&s.queue),
            ID3D12Fence::from_raw_borrowed(&s.fence),
        ) {
            let _ = list.Close();
            let lists = [Some(list.cast::<ID3D12CommandList>().expect("a graphics list is a command list"))];
            queue.ExecuteCommandLists(&lists);
            crate::xr_trace::stage(crate::xr_trace::STAGE_COPIES);
            // the runtime's own queue after the copies' queue (main's 0.2.4), the fence on the runtime's
            crate::runtime_queue::order_runtime_after(s.queue);
            let fence_queue = if s.fence_queue.is_null() { s.queue } else { s.fence_queue };
            let signal = ID3D12CommandQueue::from_raw_borrowed(&fence_queue).unwrap_or(queue);
            *s.fence_value += 1;
            let value = *s.fence_value;
            *s.allocator_fence = value;
            if signal.Signal(fence, value).is_ok() {
                if let Some(f) = s.submitted {
                    f(s.profile, fence_queue, s.fence, value);
                }
            }
            out.read_fence = value;
        }
        if s.copy_ready {
            out.release_ok = release_images(s, false, false);
            out.depth_ok = release_images(s, true, false);
        } else {
            release_images(s, false, true);
            release_images(s, true, true);
        }
    }
}

// ---- the submit and capture rings' next allocator (FrameThreadMain, CaptureMonoPresentedFrame)

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_XrAllocatorBusy: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugCapFenceWaits: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugCapFenceUsSum: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugCapFenceUsMax: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugCapSkipFence: core::sync::atomic::AtomicU64;
}

/// Waits until `fence` reaches `busy`, `budget_ms` at most; whether it did. A DEADLINE ON THE FENCE VALUE on QPC (the
/// tick clock moves in ~15.6 ms steps), never one wait's own result: a registration left over from an earlier timed-out
/// wait fires later and leaves the auto-reset event signalled, so a single wait can return at once for the OLD value
/// while the allocator is still executing -- on the submit side the cause of skips in one display cycle of seven on the
/// simulator, on the capture side exactly the fault the per-allocator values exist to prevent.
#[cfg(not(test))]
unsafe fn wait_fence(f: &windows::Win32::Graphics::Direct3D12::ID3D12Fence, event: *mut c_void, busy: u64, budget_ms: f64) -> bool {
    use windows::Win32::Foundation::HANDLE;
    let deadline = crate::tracking::now_ms() + budget_ms;
    loop {
        let left = deadline - crate::tracking::now_ms();
        if left <= 0.0 {
            break;
        }
        // SAFETY: the caller's fence and event.
        unsafe {
            let _ = f.SetEventOnCompletion(busy, HANDLE(event));
            windows_sys::Win32::System::Threading::WaitForSingleObject(event, left as u32 + 1);
            if f.GetCompletedValue() >= busy {
                return true;
            }
        }
    }
    // SAFETY: the caller's fence.
    unsafe { f.GetCompletedValue() >= busy }
}

/// THE SUBMIT RING'S NEXT ALLOCATOR, once the GPU is done with the last submission recorded from it. Its fence value is
/// kept per allocator, as on the capture side: "the submission two before the last" is not that bound once a cycle
/// rotates and submits nothing. An allocator is never reset while busy -- Reset does not fail on memory the GPU is
/// still executing, it recycles it.
///
/// Busy is common, not a fault: the copies queue behind the game's own GPU work on its queue, and under path tracing a
/// game frame outlasts three display cycles. So the wait is SHORT (3 ms) and the cycle then sends the last frame again
/// -- a stall here would only move this cycle's xrEndFrame past its display time, and an end with no layer is a black
/// frame.
///
/// True with `index` advanced when the next allocator is free; false, counted in CyberpunkVR_XrAllocatorBusy, when not.
///
/// # Safety
/// `fence` is the ring's ID3D12Fence, `event` its auto-reset event, `allocator_fence` its three fence values, `index`
/// the ring's current allocator.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_next_allocator(fence: *mut c_void, event: *mut c_void, allocator_fence: *const u64, index: *mut u32) -> bool {
    use windows::Win32::Graphics::Direct3D12::ID3D12Fence;
    // SAFETY (whole body): the caller's ring.
    unsafe {
        let Some(f) = ID3D12Fence::from_raw_borrowed(&fence) else { return false };
        let next = (*index + 1) % 3;
        let busy = *allocator_fence.add(next as usize);
        let mut free = busy == 0 || f.GetCompletedValue() >= busy;
        if !free {
            free = wait_fence(f, event, busy, 3.0);
            if !free {
                CyberpunkVR_XrAllocatorBusy.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
        }
        if free {
            *index = next;
        }
        free
    }
}

/// THE CAPTURE RING'S NEXT ALLOCATOR, once the GPU is done with it. Each allocator keeps the fence value of the last
/// submission recorded from it, and the ring only moves on to one that is free. It used to move first and then wait for
/// "the submission two before the last": every capture that stopped between the two (the wait timing out, a missing eye
/// texture) shifted the ring against the submissions, after which that bound guarded the wrong allocator and Reset
/// handed the GPU's command memory to the next recording while it was still being executed -- the device then faulted
/// at random addresses on the next frames (DRED 2026-11-03: removed inside OpenXR_capture_command_list under path
/// tracing at load).
///
/// BOUNDED (100 ms): this runs on the game's Present thread, so an infinite wait turns any GPU hiccup into a hung game,
/// and skipping one capture costs nothing (the submit side sends the frame it holds again, with its own pose). And
/// TIMED with the diagnostics on: the duration blocks the game's Present, so it is the cost -- a wait that succeeds
/// after 30 ms does more damage than one that times out, because it delays the publish without being a skip.
///
/// True with `index` advanced when the next allocator is free; false (the ring stays where it is, counted in
/// CyberpunkVR_DebugCapSkipFence) when not.
///
/// # Safety
/// As cvr_xr_next_allocator, for the capture ring.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_next_capture_allocator(
    fence: *mut c_void,
    event: *mut c_void,
    allocator_fence: *const u64,
    index: *mut u32,
) -> bool {
    use core::sync::atomic::Ordering::Relaxed;
    use windows::Win32::Graphics::Direct3D12::ID3D12Fence;
    // SAFETY (whole body): the caller's ring and the C++ half's counters.
    unsafe {
        let Some(f) = ID3D12Fence::from_raw_borrowed(&fence) else { return false };
        let next = (*index + 1) % 3;
        let busy = *allocator_fence.add(next as usize);
        if busy != 0 && f.GetCompletedValue() < busy {
            let diagnostics = core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_RuntimeDiagnostics)) != 0;
            let enter = if diagnostics { crate::tracking::now_ms() } else { 0.0 };
            let free = wait_fence(f, event, busy, 100.0);
            if diagnostics {
                let us = ((crate::tracking::now_ms() - enter) * 1000.0) as u64;
                CyberpunkVR_DebugCapFenceWaits.fetch_add(1, Relaxed);
                CyberpunkVR_DebugCapFenceUsSum.fetch_add(us, Relaxed);
                CyberpunkVR_DebugCapFenceUsMax.fetch_max(us, Relaxed);
            }
            if !free {
                if diagnostics {
                    CyberpunkVR_DebugCapSkipFence.fetch_add(1, Relaxed);
                }
                return false;
            }
        }
        *index = next;
        true
    }
}
