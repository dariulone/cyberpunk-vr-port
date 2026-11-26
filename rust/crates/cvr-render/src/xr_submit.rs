//! THE SUBMIT'S RESOURCES AND THE CYCLE'S SUBMISSION (formerly OpenXRManager's eye swapchains, RTV heap, submit ring,
//! sRGB decode, last copy, last submission and last submitted serial; OpenXRCapture.cpp's EnsureMonoSubmitResources;
//! and src/Runtimes/OpenXRFrameLoop.cpp FrameThreadMain from the captured frame taken to the end without a picture).
//!
//! The resources are the XR submit thread's: created at the game's presented size and the runtime's best format for
//! it, and recreated -- once the GPU is done with the last submission, bounded to a second -- when that size changes.
//! The cycle takes the newest captured frame, the next free allocator, the frame generator's selection, copies the
//! eyes and ends the frame (xr_cycle_end.rs); a cycle that cannot submit its picture releases what it took and sends
//! the last accepted frame again, or an empty end (cvr_xr::resubmit).
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{LazyLock, Mutex, MutexGuard};

use cvr_gpu::framegen_core as core_fg;
use cvr_host::log;
use cvr_xr::resubmit::LastSubmission;
use openxr_sys as xr;
use windows::core::Interface as _;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandAllocator, ID3D12DescriptorHeap, ID3D12Fence, ID3D12GraphicsCommandList,
};
use windows::Win32::System::Threading::CreateEventA;
use xr::Handle as _;

use crate::depth_layer::{self, DepthIn};
use crate::framegen_inputs::Inputs;
use crate::framegen_presenter::{CapturedSelectIn, SelectedOut};
use crate::xr_cycle_end::{CycleCopiesIn, CycleEndIn, EyeSwap, LastCopy};

/// One eye's colour swapchain (EyeSwapchain): its images (the runtime's, not referenced) and their RTVs.
struct Eye {
    handle: u64,
    width: i32,
    height: i32,
    images: Vec<xr::SwapchainImageD3D12KHR>,
    rtvs: Vec<usize>,
}

struct Submit {
    eyes: Vec<Eye>,
    rtv_heap: Option<ID3D12DescriptorHeap>,
    allocators: [Option<ID3D12CommandAllocator>; 3],
    lists: [Option<ID3D12GraphicsCommandList>; 3],
    fence: Option<ID3D12Fence>,
    event: HANDLE,
    fence_value: u64,
    /// The fence value of the last submission recorded from each allocator: it may be reset once the fence passes it.
    allocator_fence: [u64; 3],
    allocator_index: u32,
    /// The swapchain does not declare the eye bytes as sRGB: the eyes go through the decode (cvr_gpu::srgb_pass).
    convert: bool,
    srgb_pass: *mut c_void,
    last_copy: LastCopy,
    last_submission: LastSubmission,
    last_submitted_serial: u64,
}
// SAFETY: the XR submit thread's, used under the lock.
unsafe impl Send for Submit {}
static SUBMIT: LazyLock<Mutex<Submit>> = LazyLock::new(|| {
    // SAFETY: plain records of the C++ half's layout, all-zero their initial state.
    let mut last_submission: LastSubmission = unsafe { core::mem::zeroed() };
    last_submission.quad.ty = xr::StructureType::COMPOSITION_LAYER_QUAD;
    Mutex::new(Submit {
        eyes: Vec::new(),
        rtv_heap: None,
        allocators: [None, None, None],
        lists: [None, None, None],
        fence: None,
        event: HANDLE::default(),
        fence_value: 0,
        allocator_fence: [0; 3],
        allocator_index: 0,
        convert: false,
        srgb_pass: core::ptr::null_mut(),
        // SAFETY: as above.
        last_copy: unsafe { core::mem::zeroed() },
        last_submission,
        last_submitted_serial: 0,
    })
});
fn submit() -> MutexGuard<'static, Submit> {
    SUBMIT.lock().unwrap_or_else(|e| e.into_inner())
}
/// The eyes' count and the second eye's width, for the capture on the game's thread.
/// Diagnostic counts (the runtime diagnostics on): the ready resources' fast path, the runtime's capabilities asked
/// again, a resize put off until the GPU is done with the old resources (main's 8255f379b).
#[no_mangle]
pub static CyberpunkVR_XrSubmitResourceCache: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static EYE_COUNT: AtomicU32 = AtomicU32::new(0);
static EYE1_WIDTH: AtomicI32 = AtomicI32::new(0);

impl Submit {
    fn publish_eyes(&self) {
        EYE_COUNT.store(self.eyes.len() as u32, Relaxed);
        EYE1_WIDTH.store(self.eyes.get(1).map_or(0, |e| e.width), Relaxed);
    }
    fn destroy_swapchains(&mut self) {
        extern "system" {
            fn xrDestroySwapchain(swapchain: xr::Swapchain) -> xr::Result;
        }
        for e in &self.eyes {
            if e.handle != 0 {
                // SAFETY: the eye's swapchain, destroyed once.
                unsafe { xrDestroySwapchain(xr::Swapchain::from_raw(e.handle)) };
            }
        }
        depth_layer::cvr_xr_depth_destroy_swapchains();
        self.eyes.clear();
        self.publish_eyes();
    }
    /// The ring's lists and allocators (a resize): the fence and its event are the session's, not the size's -- their
    /// timeline goes on, as capture readers can still hold receipts from it (main's 8255f379b).
    fn release_lists(&mut self) {
        self.lists = [None, None, None];
        self.allocators = [None, None, None];
    }
    fn release_ring(&mut self) {
        if !self.event.is_invalid() {
            // SAFETY: the ring's event, closed once.
            let _ = unsafe { CloseHandle(self.event) };
            self.event = HANDLE::default();
        }
        self.fence = None;
        *crate::xr_capture_state::SUBMIT_FENCE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.lists = [None, None, None];
        self.allocators = [None, None, None];
    }
    fn fence_raw(&self) -> *mut c_void {
        self.fence
            .as_ref()
            .map_or(core::ptr::null_mut(), |f| f.as_raw())
    }
}

extern "C" {
    fn GetDepthSubmit() -> i32;
    fn GetMonoDepthCapture() -> i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_StereoSubmit: i32;
    static CyberpunkVR_VrcamEyeReuseMax: u32;
    static CyberpunkVR_MainIsRightEye: i32;
    static mut CyberpunkVR_StereoEyeCapture: i32;
    static CyberpunkVR_XrDeepDiag: i32;
    static CyberpunkVR_DebugVrcamEyePaired: AtomicU64;
    static CyberpunkVR_DebugVrcamEyeUnpaired: AtomicU64;
    static CyberpunkVR_DebugVrcamEyeReused: AtomicU64;
    static CyberpunkVR_XrReuseImages: AtomicU32;
}
fn read<T: Copy>(v: &T) -> T {
    // SAFETY: an export of the C++ half, read as it changes.
    unsafe { core::ptr::read_volatile(v) }
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    read(unsafe { &*(&raw const CyberpunkVR_RuntimeDiagnostics) }) != 0
}

// ---- the resources ---------------------------------------------------------------------------------------------------

/// What the resources are ensured for: the session and device, the instance and its system, the game's presented size
/// and format, the test switch passing over the sRGB companion format.
struct EnsureIn {
    session: u64,
    instance: u64,
    system: u64,
    device: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    test_convert: bool,
}

/// OpenXRManager::EnsureMonoSubmitResources: false while the mono submission is off, without the game's device and
/// queue or the session, or before a present gave the frame's size and format; else the resources ensured
/// (submit_ensure). The test marker passing over the sRGB companion format is read once per process.
pub fn ensure_mono_submit() -> bool {
    let submission = &cvr_xr::submit_state::SUBMIT;
    if !submission.mono_submit_enabled.load(Relaxed) {
        return false;
    }
    // SAFETY: set at InitGraphics, cleared at shutdown, as the members were.
    let (device, queue) = unsafe { (*submission.device.get(), *submission.queue.get()) };
    let (session, _, _) = cvr_xr::frame::session_handles();
    if device.is_null() || queue.is_null() || session == 0 {
        return false;
    }
    let (width, height, format) = {
        let presented = &cvr_xr::presented::PRESENTED;
        let _held = presented.lock.lock();
        // SAFETY: under the present lock.
        unsafe { (*presented.width.get(), *presented.height.get(), *presented.format.get()) }
    };
    if width == 0 || height == 0 || format == 0 {
        return false;
    }
    static TEST_CONVERT: LazyLock<bool> = LazyLock::new(|| {
        let on = cvr_host::paths::test_marker("cvr_swapchain_convert.txt");
        if on {
            log!("OpenXRManager: TEST: sRGB companion swapchain format passed over (cvr_swapchain_convert.txt)\n");
        }
        on
    });
    let (instance, system) = cvr_xr::view_configs::instance_and_system();
    let input = EnsureIn { session, instance, system, device, width, height, format, test_convert: *TEST_CONVERT };
    // SAFETY: the live session, instance and device.
    unsafe { submit_ensure(&input) }
}
/// The C++ half's EnsureMonoSubmitResources.
#[no_mangle]
pub extern "C" fn cvr_xr_ensure_mono_submit() -> bool {
    ensure_mono_submit()
}

/// EnsureMonoSubmitResources after its early outs: the runtime's formats and the one picked for the game's, the frame
/// generator's depth capture switched with the depth layer, the view records (cvr_xr::view_configs and the located
/// views' list), and -- unless the eye swapchains already
/// have the game's size and the ring is whole -- everything recreated: the GPU's last submission waited for (a second at
/// most), the swapchains, RTV heap, ring and event dropped, the eye swapchains made, the sRGB decode when the format
/// does not declare the bytes, the ring's allocators, lists, fence and event.
///
/// # Safety
/// The session and device live.
unsafe fn submit_ensure(i: &EnsureIn) -> bool {
    extern "system" {
        fn xrEnumerateSwapchainFormats(
            session: xr::Session,
            capacity: u32,
            count: *mut u32,
            formats: *mut i64,
        ) -> xr::Result;
        fn xrEnumerateViewConfigurationViews(
            instance: xr::Instance,
            system: xr::SystemId,
            ty: xr::ViewConfigurationType,
            capacity: u32,
            count: *mut u32,
            views: *mut xr::ViewConfigurationView,
        ) -> xr::Result;
    }
    let session = xr::Session::from_raw(i.session);
    // SAFETY (whole body): the live session, instance and device.
    unsafe {
        // Input collection is shared with framegen, but can run depth-only. Depth swapchains are created lazily from
        // the selected per-eye snapshots (depth_layer.rs). A live switch counts even with the colour resources as they
        // are; view poses and FOVs come from xrLocateViews every cycle.
        core_fg::cvr_framegen_set_depth_capture_enabled(
            depth_layer::cvr_xr_depth_supported()
                && GetDepthSubmit() != 0
                && GetMonoDepthCapture() != 0,
        );
        {
            let mut s = submit();
            let ready = s
                .eyes
                .first()
                .is_some_and(|e| e.width == i.width as i32 && e.height == i.height as i32)
                && s.allocators.iter().all(Option::is_some)
                && s.lists.iter().all(Option::is_some)
                && s.fence.is_some()
                && !s.event.is_invalid()
                && s.rtv_heap.is_some();
            if ready {
                if diagnostics() {
                    CyberpunkVR_XrSubmitResourceCache[0].fetch_add(1, Relaxed);
                }
                return true;
            }
            // A busy GPU is not leave to destroy its targets and allocators after a timeout: the old swapchains and
            // the last submission are kept and the next cycle tries again (the frame loop can resubmit that image
            // while the resize waits).
            if let (Some(fence), true) = (&s.fence, s.fence_value != 0) {
                let completed = fence.GetCompletedValue();
                if completed == u64::MAX {
                    s.last_submission.valid = false;
                    static NEXT: AtomicU64 = AtomicU64::new(0);
                    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
                    if now >= NEXT.load(Relaxed) {
                        NEXT.store(now + 5000, Relaxed);
                        log!("OpenXRManager: submit resize stopped: graphics device removed\n");
                    }
                    return false;
                }
                if completed < s.fence_value {
                    if diagnostics() {
                        CyberpunkVR_XrSubmitResourceCache[2].fetch_add(1, Relaxed);
                    }
                    return false;
                }
            }
        }
        // The session's capabilities are wanted when the images are made, not every display cycle.
        if diagnostics() {
            CyberpunkVR_XrSubmitResourceCache[1].fetch_add(1, Relaxed);
        }
        let mut count = 0u32;
        let res = xrEnumerateSwapchainFormats(session, 0, &mut count, core::ptr::null_mut());
        if res.into_raw() < 0 || count == 0 {
            log!(
                "OpenXRManager: xrEnumerateSwapchainFormats count failed (res={} count={})\n",
                res.into_raw(),
                count
            );
            return false;
        }
        let mut formats = vec![0i64; count as usize];
        let res = xrEnumerateSwapchainFormats(session, count, &mut count, formats.as_mut_ptr());
        if res.into_raw() < 0 || count == 0 {
            log!(
                "OpenXRManager: xrEnumerateSwapchainFormats list failed (res={} count={})\n",
                res.into_raw(),
                count
            );
            return false;
        }
        formats.truncate(count as usize);
        let mut convert = false;
        let selected = cvr_xr::swapchains::cvr_xr_pick_mono_swapchain_format(
            formats.as_ptr(),
            count,
            i.format as i64,
            i.test_convert,
            &mut convert,
        );


        let instance = xr::Instance::from_raw(i.instance);
        let system = xr::SystemId::from_raw(i.system);
        let mut view_count = 0u32;
        xrEnumerateViewConfigurationViews(
            instance,
            system,
            xr::ViewConfigurationType::PRIMARY_STEREO,
            0,
            &mut view_count,
            core::ptr::null_mut(),
        );
        let mut views: Vec<xr::ViewConfigurationView> = (0..view_count)
            .map(|_| xr::ViewConfigurationView {
                ty: xr::StructureType::VIEW_CONFIGURATION_VIEW,
                next: core::ptr::null_mut(),
                recommended_image_rect_width: 0,
                max_image_rect_width: 0,
                recommended_image_rect_height: 0,
                max_image_rect_height: 0,
                recommended_swapchain_sample_count: 0,
                max_swapchain_sample_count: 0,
            })
            .collect();
        let capacity = view_count;
        xrEnumerateViewConfigurationViews(
            instance,
            system,
            xr::ViewConfigurationType::PRIMARY_STEREO,
            capacity,
            &mut view_count,
            views.as_mut_ptr(),
        );
        cvr_xr::view_configs::record(&views);
        cvr_xr::views::resize(view_count);

        let mut s = submit();
        // The retirement gate above proved the old submit resources out of use.
        s.destroy_swapchains();
        s.rtv_heap = None;
        // The last submission's and the last copy's swapchains are gone (a runtime may give the new ones the same
        // handle values, with no image released yet).
        s.last_submission.valid = false;
        s.last_copy.valid = false;
        s.allocator_fence = [0; 3];
        s.release_lists();

        let mut made: cvr_xr::swapchains::EyeSwapchains = core::mem::zeroed();
        let ok = cvr_xr::swapchains::cvr_xr_create_eye_swapchains(
            i.session, i.device, i.width, i.height, selected, view_count, &mut made,
        );
        let n = if ok { made.view_count } else { made.created } as usize;
        s.eyes = made.eyes[..n]
            .iter()
            .map(|m| Eye {
                handle: m.handle,
                width: m.width,
                height: m.height,
                images: m.images[..m.image_count as usize]
                    .iter()
                    .map(|&texture| xr::SwapchainImageD3D12KHR {
                        ty: xr::StructureType::SWAPCHAIN_IMAGE_D3D12_KHR,
                        next: core::ptr::null_mut(),
                        texture: texture.cast(),
                    })
                    .collect(),
                rtvs: if made.rtv_heap.is_null() {
                    Vec::new()
                } else {
                    m.rtvs[..m.image_count as usize].to_vec()
                },
            })
            .collect();
        s.publish_eyes();
        s.rtv_heap =
            (!made.rtv_heap.is_null()).then(|| ID3D12DescriptorHeap::from_raw(made.rtv_heap));
        if !ok {
            return false;
        }
        depth_layer::cvr_xr_depth_forget_format();

        let mut summary = format!(
            "OpenXRManager: Mono swapchain formats. game={} selected={} convert={} runtime:",
            i.format, selected, convert as i32
        );
        for f in &formats {
            if summary.len() >= 512 - 32 {
                break;
            }
            summary.push_str(&format!(" {f}"));
        }
        summary.truncate(511);
        cvr_host::log::write(&format!("{summary}\n"));

        // No swapchain format declares the eye bytes as sRGB: each eye is drawn through the decode instead.
        s.convert = false;
        if convert {
            if s.srgb_pass.is_null() {
                s.srgb_pass = cvr_gpu::srgb_pass::cvr_gpu_srgb_pass_new().cast();
            }
            if !cvr_gpu::srgb_pass::cvr_gpu_srgb_pass_ensure(
                s.srgb_pass.cast(),
                i.device,
                selected as i32,
                i.width,
                i.height,
            ) {
                log!(
                    "OpenXRManager: the sRGB decode for swapchain format {} failed to initialise\n",
                    selected
                );
                return false;
            }
            s.convert = true;
        }

        let mut allocators = [core::ptr::null_mut::<c_void>(); 3];
        let mut lists = [core::ptr::null_mut::<c_void>(); 3];
        let mut fence = core::ptr::null_mut::<c_void>();
        let ok = cvr_xr::swapchains::cvr_xr_create_submit_objects(
            i.device,
            allocators.as_mut_ptr(),
            lists.as_mut_ptr(),
            &mut fence,
        );
        for k in 0..3 {
            s.allocators[k] =
                (!allocators[k].is_null()).then(|| ID3D12CommandAllocator::from_raw(allocators[k]));
            s.lists[k] =
                (!lists[k].is_null()).then(|| ID3D12GraphicsCommandList::from_raw(lists[k]));
        }
        // the session's fence kept (its timeline goes on); a new one only for the first resources
        let made_fence = (!fence.is_null()).then(|| ID3D12Fence::from_raw(fence));
        if s.fence.is_none() {
            s.fence = made_fence;
            // the capture's view of it: a write waits for the runtime queue's retired reads (main's 0.2.4)
            *crate::xr_capture_state::SUBMIT_FENCE.lock().unwrap_or_else(|e| e.into_inner()) = s.fence.clone();
        }
        if !ok || s.fence.is_none() {
            return false;
        }
        if s.event.is_invalid() {
            match CreateEventA(None, false, false, None) {
                Ok(event) if !event.is_invalid() => s.event = event,
                _ => {
                    log!("OpenXRManager: Failed to create mono fence event\n");
                    return false;
                }
            }
        }
        let (eye_w, eye_h) = s.eyes.first().map_or((0, 0), |e| (e.width, e.height));
        let (rec_w, rec_h) = views.first().map_or((0, 0), |v| {
            (
                v.recommended_image_rect_width,
                v.recommended_image_rect_height,
            )
        });
        log!(
            "OpenXRManager: Mono submit resources ready. game={}x{} eye0={}x{} rec0={}x{} format={}\n",
            i.width,
            i.height,
            if view_count != 0 { eye_w } else { 0 },
            if view_count != 0 { eye_h } else { 0 },
            if view_count != 0 { rec_w } else { 0 },
            if view_count != 0 { rec_h } else { 0 },
            i.format
        );
        true
    }
}

/// How many eye swapchains there are (m_eyeSwapchains.size()).
#[no_mangle]
pub extern "C" fn cvr_xr_submit_eye_count() -> u32 {
    EYE_COUNT.load(Relaxed)
}
/// The second eye's swapchain width, or 0.
#[no_mangle]
pub extern "C" fn cvr_xr_submit_eye1_width() -> i32 {
    EYE1_WIDTH.load(Relaxed)
}
/// The serial of the frame the runtime last accepted (m_lastSubmittedSerial).
#[no_mangle]
pub extern "C" fn cvr_xr_submit_last_serial() -> u64 {
    submit().last_submitted_serial
}
/// The eye swapchains destroyed with the session (Shutdown).
#[no_mangle]
pub extern "C" fn cvr_xr_submit_destroy_swapchains() {
    submit().destroy_swapchains();
}
/// The ring's event, fence, lists and allocators and the RTV heap released (Shutdown).
#[no_mangle]
pub extern "C" fn cvr_xr_submit_release() {
    let mut s = submit();
    s.release_ring();
    s.rtv_heap = None;
}

// ---- the cycle -------------------------------------------------------------------------------------------------------

/// The cycle as the C++ frame loop has it after its views and head are located.
#[repr(C)]
pub struct CycleSubmitIn {
    pub session: u64,
    pub local_space: u64,
    pub view_space: u64,
    pub device: *mut c_void,
    pub queue: *mut c_void,
    pub display_time: i64,
    pub display_period: i64,
    /// m_predictedDisplayPeriodNs.
    pub predicted_period_ns: i64,
    pub wait_enter_ms: f64,
    pub wait_ms: f64,
    /// The head located this cycle (location.pose).
    pub head_pose: xr::Posef,
    pub tracking_origin: u64,
    pub quads: [*mut c_void; 5],
    pub framegen_overlay: *mut c_void,
    pub settings_panel: *mut c_void,
    pub frame_sync_event: *mut c_void,
    /// The located views' count, and m_views.size() for the waiting line.
    pub view_count: u32,
    pub views_len: u32,
    /// The runtime's IPD in metres.
    pub runtime_ipd: f32,
    pub depth: DepthIn,
    pub mono_ready: bool,
    pub mono_enabled: bool,
    pub should_render: bool,
    pub menu_rect_active: bool,
    pub head_located: bool,
    /// The session runs and no menu rect is up (the frame generator's gameplay).
    pub gameplay: bool,
    /// cvr::projection::Active().
    pub asym_active: bool,
}
const _: () = assert!(
    core::mem::size_of::<CycleSubmitIn>() == 216
        && core::mem::offset_of!(CycleSubmitIn, head_pose) == 80
        && core::mem::offset_of!(CycleSubmitIn, quads) == 120
        && core::mem::offset_of!(CycleSubmitIn, view_count) == 184
        && core::mem::offset_of!(CycleSubmitIn, depth) == 196
);

/// THE CYCLE'S SUBMISSION (the frame loop from the captured frame taken to its end): with the submit's resources ready
/// for the located views, the newest captured frame and the second eye's picture taken, the next free allocator, the
/// frame generator's selection, the copies and the end -- true when the frame was accepted. Otherwise what was taken is
/// released (the frame generator told the submission failed) and the last accepted frame goes out again, or an empty
/// end -- false (the frame loop then ends the cycle).
///
/// # Safety
/// `input` is valid; its objects live.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_cycle_submit(input: *const CycleSubmitIn) -> bool {
    // SAFETY: the caller's record.
    let Some(c) = (unsafe { input.as_ref() }) else {
        return false;
    };
    let mut guard = submit();
    let s = &mut *guard;
    // SAFETY (whole body): the submit's live resources and the caller's objects.
    unsafe {
        if c.mono_ready && c.view_count as usize == s.eyes.len() {
            if cycle_picture(c, s) {
                return true;
            }
        } else if diagnostics() && c.mono_enabled {
            // The frame loop's counter starts again every cycle: every waiting cycle says so.
            log!(
                "OpenXRManager: {} submit waiting. ready={} views={} shouldRender={}\n",
                "Mono",
                c.mono_ready as i32,
                c.views_len,
                c.should_render as i32
            );
        }

        // NOTHING NEW THIS CYCLE: the last accepted frame again, with its own poses, so the runtime reprojects it to
        // this display time; only when that is impossible does the end go out empty (cvr_xr::resubmit).
        let mut eyes = [0u64; 2];
        let mut depths = [0u64; 2];
        let count = s.eyes.len().min(2);
        for eye in 0..count {
            eyes[eye] = s.eyes[eye].handle;
            depths[eye] = depth_layer::cvr_xr_depth_swapchain(eye as u32);
        }
        let resubmits = crate::used_moved_exports::CyberpunkVR_XrResubmits.load(Relaxed);
        cvr_xr::resubmit::cvr_xr_end_without_picture(
            c.should_render,
            c.display_time,
            &mut s.last_submission,
            c.session,
            c.local_space,
            eyes.as_ptr(),
            depths.as_ptr(),
            count as u32,
        );
        if crate::used_moved_exports::CyberpunkVR_XrResubmits.load(Relaxed) != resubmits {
            // the resent cycle in the delivery trace too: synthetic -1, kind -2
            cvr_xr::xr_trace::cycle([
                cvr_xr::tracking::now_ms(),
                c.display_time as f64,
                s.last_submitted_serial as f64,
                -1.0,
                -2.0,
                0.0,
                1.0,
                c.display_period as f64,
                0.0,
                c.wait_ms,
                0.0,
                0.0,
            ]);
        }
    }
    false
}

/// The cycle with its views located: the captured frame, the allocator, the selection, the copies and the end. True
/// when the frame was accepted; false with what was taken released.
///
/// # Safety
/// As cvr_xr_cycle_submit.
unsafe fn cycle_picture(c: &CycleSubmitIn, s: &mut Submit) -> bool {
    // SAFETY (whole body): as the caller promises.
    unsafe {
        let view_count = c.view_count;
        let stereo_submit = read(&*(&raw const CyberpunkVR_StereoSubmit)) != 0;
        let mut mono_source: *mut c_void = core::ptr::null_mut();
        let mut vrcam_eye: *mut c_void = core::ptr::null_mut();
        let mut read_fence = 0u64;
        let mut serial = 0u64;
        let mut poses: [xr::Posef; 2] = core::mem::zeroed();
        let mut pose_ids = [0u64; 2];
        let mut capture_ms = 0.0f64;
        let mut dark = false;
        let mut fovs: [xr::Fovf; 2] = core::mem::zeroed();
        let mut has_view = [false; 2];
        let mut inputs: [*const Inputs; 2] = [core::ptr::null(); 2];
        let mut picked = SelectedOut {
            color: [core::ptr::null_mut(); 2],
            synthetic: false,
        };
        // The eye MAIN's picture is in the frame taken below.
        let mut main_eye = if read(&*(&raw const CyberpunkVR_MainIsRightEye)) != 0 {
            1u32
        } else {
            0
        };
        depth_layer::cvr_xr_depth_poll();

        // THE QUEUES (main's 0.2.4): with the runtime on a queue of its own, a cycle reads only FINISHED captures there
        // (SteamVR: CyberpunkVR_SteamVrFinishedCapture's bit for generation on or off; the other runtimes without
        // generation); the others copy on the game's queue, the runtime's ordered after it. Under SteamVR the generation
        // of a finished capture runs on the runtime's queue too.
        let steamvr = cvr_xr::runtime_info::RUNTIME.steamvr.load(Relaxed);
        let own_queue = cvr_xr::runtime_queue::has_own();
        let runtime_queue = if own_queue { cvr_xr::runtime_queue::runtime_queue() } else { c.queue };
        let current_fg = core_fg::cvr_framegen_enabled();
        let finished_capture = own_queue
            && if steamvr {
                read(&*(&raw const crate::xr_cycle_end::CyberpunkVR_SteamVrFinishedCapture)) & if current_fg { 1 } else { 2 } != 0
            } else {
                !current_fg
            };
        let synthesis_queue = if steamvr && finished_capture { runtime_queue } else { c.queue };

        // THE NEWEST CAPTURED FRAME (xr_capture_state.rs): the image pinned and referenced with its poses, FOVs and
        // labels, the frame generator's inputs, and with the stereo submit the second eye's picture of the same frame.
        // On the runtime's queue a capture still running on the game's is not taken: the one before it when finished
        // and not sent, else the last frame goes again.
        let mut taken: crate::xr_capture_state::Taken = core::mem::zeroed();
        let finished = crate::xr_capture_state::Finished {
            only: finished_capture,
            buffer: finished_capture && current_fg && read(&*(&raw const crate::xr_cycle_end::CyberpunkVR_SteamVrFrameBuffer)) != 0,
        };
        if crate::xr_capture_state::take(
            stereo_submit && view_count >= 2,
            read(&*(&raw const CyberpunkVR_VrcamEyeReuseMax)),
            core_fg::cvr_framegen_capture_inputs_enabled(),
            finished,
            &mut taken,
        ) {
            mono_source = taken.texture;
            serial = taken.serial;
            capture_ms = taken.capture_ms;
            dark = taken.dark;
            main_eye = taken.main_eye;
            poses = taken.poses;
            pose_ids = taken.pose_ids;
            fovs = taken.fovs;
            has_view = taken.has_view;
            inputs = taken.inputs;
            vrcam_eye = taken.second;
            if taken.second_reused && diagnostics() {
                CyberpunkVR_DebugVrcamEyeReused.fetch_add(1, Relaxed);
            }
        }

        // A midpoint already accepted by the runtime owns its endpoint independently of the newest capture: a new
        // capture not ready or without its pair must not drop that pinned endpoint (SteamVR, main's 0.2.4).
        if steamvr && (mono_source.is_null() || vrcam_eye.is_null()) && !c.menu_rect_active && core_fg::cvr_framegen_enabled() {
            if let Some(owed) = crate::framegen_presenter::pending_endpoint(c.tracking_origin) {
                crate::xr_cycle_end::release_captured_read(s.fence_raw(), mono_source, 0);
                crate::xr_cycle_end::release_captured_read(s.fence_raw(), vrcam_eye, 0);
                for eye in 0..2 {
                    crate::framegen_inputs::cvr_framegen_release_inputs(inputs[eye]);
                }
                mono_source = owed.color[0];
                vrcam_eye = owed.color[1];
                main_eye = 0;
                serial = owed.serial;
                capture_ms = owed.captured_ms;
                poses = owed.poses;
                fovs = owed.fovs;
                pose_ids = owed.pose_ids;
                has_view = [true, true];
                inputs = owed.inputs;
            }
        }
        // Colour is capture-fence complete; the depth and motion snapshots carry their own producer fences: ordered on
        // the runtime's queue without a CPU wait or a wait behind the next game frame.
        if steamvr && finished_capture {
            for input in inputs {
                if let Some(input) = input.as_ref() {
                    if let (Some(fence), true) = (&input.ready_fence, input.fence_value != 0) {
                        if let Some(queue) = windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue::from_raw_borrowed(&runtime_queue) {
                            let _ = queue.Wait(fence, input.fence_value);
                        }
                    }
                }
            }
        }

        // The next allocator, once the GPU is done with the last submission recorded from it (cvr_xr_next_allocator:
        // a short deadline, then the last frame again).
        let fence = s.fence_raw();
        let allocator_free = cvr_xr::swapchains::cvr_xr_next_allocator(
            fence,
            s.event.0,
            s.allocator_fence.as_ptr(),
            &mut s.allocator_index,
        );
        let index = s.allocator_index as usize;
        let allocator = if allocator_free {
            s.allocators[index]
                .as_ref()
                .map_or(core::ptr::null_mut(), |a| a.as_raw())
        } else {
            core::ptr::null_mut()
        };
        let list = s.lists[index]
            .as_ref()
            .map_or(core::ptr::null_mut(), |l| l.as_raw());

        // The engine-side second eye capture follows the switch (sync_stereo ORs its own mirror request).
        *(&raw mut CyberpunkVR_StereoEyeCapture) = stereo_submit as i32;

        // THE CYCLE'S SELECTION (framegen_presenter.rs): the pair's frame, its read lease and the presenter's choice;
        // the poses, FOVs, labels, serial and capture time follow the selected pixels.
        let captured = CapturedSelectIn {
            device: c.device,
            queue: synthesis_queue,
            submit_fence: fence,
            color: mono_source,
            second: vrcam_eye,
            poses: &mut poses,
            fovs: &mut fovs,
            pose_ids: &mut pose_ids,
            serial: &mut serial,
            captured_ms: &mut capture_ms,
            inputs,
            origin: c.tracking_origin,
            view_count,
            main_eye,
            gameplay: c.gameplay,
        };
        cvr_xr::xr_trace::stage(cvr_xr::xr_trace::STAGE_SELECTION);
        let select_start = cvr_xr::tracking::now_ms();
        let fg_selected = crate::framegen_presenter::cvr_xr_framegen_select(&captured, &mut picked);
        let select_ms = cvr_xr::tracking::now_ms() - select_start;
        // The image, its own pose, and its read reservation were taken together with MAIN above.
        if stereo_submit && view_count >= 2 {
            if !vrcam_eye.is_null() {
                if read(&*(&raw const CyberpunkVR_XrDeepDiag)) != 0 && diagnostics() {
                    CyberpunkVR_DebugVrcamEyePaired.fetch_add(1, Relaxed);
                }
            } else if diagnostics() {
                CyberpunkVR_DebugVrcamEyeUnpaired.fetch_add(1, Relaxed);
            }
        }

        if !allocator.is_null() && !mono_source.is_null() && has_view[0] && has_view[1] {
            // THE CYCLE'S COPIES AND ITS END (xr_cycle_end.rs); the views live here until the end has gone out.
            let n = view_count as usize;
            let mut views: Vec<xr::CompositionLayerProjectionView> = vec![core::mem::zeroed(); n];
            let swaps: Vec<EyeSwap> = s.eyes[..n]
                .iter()
                .map(|e| EyeSwap {
                    handle: e.handle,
                    images: e.images.as_ptr(),
                    image_count: e.images.len() as u32,
                    rtvs: e.rtvs.as_ptr(),
                    rtv_count: e.rtvs.len() as u32,
                    width: e.width,
                    height: e.height,
                })
                .collect();
            let mut color = [0u64; 2];
            for eye in 0..2 {
                if eye < n {
                    color[eye] = s.eyes[eye].handle;
                }
            }
            let end = CycleEndIn {
                release_ok: false,
                depth_ok: false,
                use_depth_layer: false,
                menu_rect_active: c.menu_rect_active,
                head_located: c.head_located,
                should_render: c.should_render,
                reuse_images: false,
                fg_selected,
                fg_synthetic: fg_selected && picked.synthetic,
                curtain_shut: false,
                menu_blink: 0.0,
                view_count,
                projection_views: views.as_mut_ptr(),
                head_pose: c.head_pose,
                mono_pose0: poses[0].orientation,
                display_time: c.display_time,
                display_period: c.display_period,
                wait_enter_ms: c.wait_enter_ms,
                wait_ms: c.wait_ms,
                mono_capture_ms: capture_ms,
                predicted_period_ns: c.predicted_period_ns,
                session: c.session,
                local_space: c.local_space,
                view_space: c.view_space,
                device: c.device,
                queue: runtime_queue,
                eye_width: s.eyes[0].width,
                eye_height: s.eyes[0].height,
                color,
                acquired_eyes: [false; 2],
                eye_sources: [core::ptr::null_mut(); 2],
                present_serial: serial,
                mono_pose_ids: pose_ids,
                quads: c.quads.map(|q| q.cast()),
                framegen_overlay: c.framegen_overlay.cast(),
                settings_panel: c.settings_panel.cast(),
                tracking_origin: c.tracking_origin,
                last_copy: &mut s.last_copy,
                last_submission: &mut s.last_submission,
                last_submitted_serial: &mut s.last_submitted_serial,
                frame_sync_event: c.frame_sync_event,
                mono_source,
                vrcam_eye,
                captured_read_fence: read_fence,
                // cvr::capture::SubmissionQueue: a generated image behind its synthesis producer, a finished capture on
                // the runtime's queue, otherwise the game's
                copy_queue: if fg_selected && picked.synthetic {
                    synthesis_queue
                } else if finished_capture {
                    runtime_queue
                } else {
                    c.queue
                },
                select_ms,
            };
            let mut copies = CycleCopiesIn {
                end,
                allocator,
                list,
                fence,
                fence_value: &mut s.fence_value,
                allocator_fence: &mut s.allocator_fence[index],
                allocator_index: index as u32,
                frame_main_eye: main_eye,
                mono_dark: dark,
                convert: s.convert && !s.srgb_pass.is_null(),
                framegen_enabled: core_fg::cvr_framegen_enabled(),
                reuse_switch: CyberpunkVR_XrReuseImages.load(Relaxed) != 0,
                fg_colors: picked.color,
                swaps: swaps.as_ptr(),
                srgb_pass: s.srgb_pass,
                projection: cvr_xr::views::ProjectionInputs {
                    poses,
                    fovs,
                    has_view,
                    main_eye,
                    view_count,
                    asym_active: c.asym_active,
                    framegen_selected: fg_selected,
                    second_picture: !vrcam_eye.is_null(),
                    menu_rect: c.menu_rect_active,
                    ipd: c.runtime_ipd,
                },
                depth: c.depth,
            };
            let accepted = crate::xr_cycle_end::cycle_copies(&mut copies);
            read_fence = copies.end.captured_read_fence;
            if accepted {
                return true;
            }
        }

        if fg_selected {
            // why the generated cycle went out without its copies (rate-limited: the first eight, then every 500th)
            static REFUSED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
            let n = REFUSED.fetch_add(1, Relaxed);
            if n < 8 || n % 500 == 0 {
                cvr_host::log::write(&format!(
                    "[framegen] a selected cycle refused ({}): allocator free {} source {} views {}/{} synthetic {}\n",
                    n + 1,
                    !allocator.is_null(),
                    !mono_source.is_null(),
                    has_view[0],
                    has_view[1],
                    picked.synthetic
                ));
            }
            crate::framegen_presenter::submitted_retaining(fence, read_fence, false, steamvr);
        }
        crate::xr_cycle_end::release_captured_read(fence, mono_source, read_fence);
        crate::xr_cycle_end::release_captured_read(fence, vrcam_eye, read_fence);
        false
    }
}
