//! The present hook's XR side (src/Runtimes/OpenXRPresent.cpp, OpenXRManager::OnPresent): each present publishes the
//! manager's switches into the shared slots (the block kept for GetSharedSlot), counts the present, advances VRIK's
//! frame epoch, buckets the game's frame interval against the display period, flushes the hands and logs the skeleton's
//! rate; then, from the swapchain's description and its current back buffer, it captures the mono frame with this
//! present's eye poses while mono submission is on (a failure logged every 300 presents), ticks the foveated rendering,
//! the VRS and the five HUD channels' captures, and under the present lock records the presented frame -- its size
//! (half the width under engine stereo), format, buffer index and serial -- releasing the frame it last held, the
//! present logged every 300 with the runtime diagnostics on.
//!
//! The .cpp's OnPresent forwards to `cvr_xr_on_present` with a host of the manager's members and calls; the switches
//! are switches.rs', the presented frame presented.rs' and the submission's state submit_state.rs'. The unit tests below check the sizes and the log lines.

/// The mono eye poses' render size: the swapchain's buffer, or the back buffer's when the swapchain's is unset.
pub fn mono_size(buffer: (u32, u32), resource: (u64, u32)) -> (f32, f32) {
    let (mut width, mut height) = (buffer.0 as f32, buffer.1 as f32);
    if (width <= 1.0 || height <= 1.0) && resource.0 != 0 && resource.1 != 0 {
        width = resource.0 as f32;
        height = resource.1 as f32;
    }
    (width, height)
}
/// The presented frame's width, height and format: the back buffer's where known, the swapchain's otherwise; the
/// width halved under engine stereo (one eye of the side-by-side frame).
pub fn presented(buffer: (u32, u32, u32), resource: (u64, u32, u32), engine_stereo: bool) -> (u32, u32, u32) {
    let mut width = if resource.0 != 0 { resource.0 as u32 } else { buffer.0 };
    if engine_stereo {
        width /= 2;
    }
    let height = if resource.1 != 0 { resource.1 } else { buffer.1 };
    let format = if resource.2 != 0 { resource.2 } else { buffer.2 };
    (width, height, format)
}
pub fn mono_failure_line(serial: u64, views: [bool; 2]) -> String {
    format!("OpenXRManager: Mono capture failed. serial={serial} views=({},{})\n", i32::from(views[0]), i32::from(views[1]))
}
#[allow(clippy::too_many_arguments)]
pub fn present_line(hwnd: usize, size: (u32, u32), format: u32, index: u32, resource: (u64, u32), running: bool) -> String {
    format!(
        "OpenXRManager: Present observed. hwnd={hwnd:016X} size={}x{} format={format} backbufferIndex={index} resourceWidth={} resourceHeight={} sessionRunning={}\n",
        size.0,
        size.1,
        resource.0,
        resource.1,
        i32::from(running)
    )
}

#[cfg(not(test))]
mod abi {
    use super::*;
    use crate::mono_eyes::MonoInputs;
    use crate::published::PresentSlots;
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
    use openxr_sys as xr;
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::{ID3D12Resource, D3D12_RESOURCE_DESC};
    use windows::Win32::Graphics::Dxgi::{IDXGISwapChain, IDXGISwapChain3};

    /// OpenXRManager's members and calls OnPresent uses.
    #[repr(C)]
    pub struct PresentHost {
        pub ctx: *mut c_void,
        /// GetCameraOffset: the neck mount plus the stored camera trim.
        pub camera_offset: unsafe extern "C" fn(*mut c_void, *mut f32),
        #[allow(clippy::type_complexity)]
        pub capture_mono: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *const D3D12_RESOURCE_DESC,
            u64,
            *const xr::Posef,
            *const xr::Fovf,
            *const bool,
            *const xr::Posef,
        ) -> bool,
        /// The foveated rendering's, the VRS's and the HUD channels' ticks.
        pub ticks: unsafe extern "C" fn(*mut c_void),
    }
    extern "C" {
        static CyberpunkVR_RuntimeDiagnostics: i32;
        /// cvr::projection::Active() is this export at 1.
        static CyberpunkVR_AsymProjection: i32;
        /// cvr::projection::WindowParams (rust/crates/cvr-render/src/asym_projection.rs).
        fn cvr_projection_window_params(fit_target: bool, guard: *mut f32, target: *mut f32);
        /// cvr::engine_stereo::Enabled() (rust/crates/cvr-render/src/engine_stereo.rs).
        fn cvr_engine_stereo_enabled() -> bool;
        static mut CyberpunkVR_DebugTidPresent: u32;
        /// src/Anim/CharacterRigState.cpp: the frame counter VRIK keys on.
        static g_VrikFrameEpoch: AtomicU32;
    }
    fn log(text: &str) {
        cvr_host::log::write(text);
    }

    /// # Safety
    /// `host` is the manager's live host; `swap_chain` is null or the game's swapchain.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_xr_on_present(host: *const PresentHost, swap_chain: *mut c_void) {
        // SAFETY: as the caller vouches, for the host's members and calls throughout.
        let Some(h) = (unsafe { host.as_ref() }) else { return };
        let switches = &crate::switches::SWITCHES;
        let mut slots = PresentSlots {
            hand_tracking_mode: switches.hand_tracking_mode.load(Relaxed),
            weapon_aim_enable: switches.weapon_aim_enable.load(Relaxed),
            immersive_holsters: switches.immersive_holsters.load(Relaxed),
            body_hands_only: switches.body_hands_only.load(Relaxed),
            camera_offset: [0.0; 3],
            log_diag_request: switches.log_diag_request.load(Relaxed),
            reload_glow: switches.reload_glow.load(Relaxed),
        };
        unsafe {
            (h.camera_offset)(h.ctx, slots.camera_offset.as_mut_ptr());
            let block = crate::published::cvr_xr_publish_present_slots(&slots);
            if !block.is_null() {
                switches.shared_hands.store(block, Relaxed);
            }
        }
        let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&swap_chain) }) else { return };
        // SAFETY: the C++ half's switch and counters.
        unsafe {
            if core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 {
                CyberpunkVR_DebugTidPresent = windows_sys::Win32::System::Threading::GetCurrentThreadId();
            }
        }
        let presented_frame = &crate::presented::PRESENTED;
        let count = presented_frame.present_count.fetch_add(1, Relaxed) + 1;
        unsafe { g_VrikFrameEpoch.fetch_add(1, Relaxed) };
        let submit = &crate::submit_state::SUBMIT;
        crate::present_diag::cvr_xr_present_gap(submit.predicted_display_period_ns.load(Relaxed));
        let mono = submit.mono_submit_enabled.load(Relaxed);
        let phase = crate::xr_trace::phase_start();
        unsafe { crate::published::cvr_xr_flush_hands(core::ptr::null(), 0, core::ptr::null()) };
        crate::xr_trace::phase_end(crate::xr_trace::PHASE_FLUSH_HANDS, phase);
        crate::present_diag::cvr_xr_vrik_rate_log(count);

        // SAFETY: the game's swapchain, borrowed for this call.
        let Ok(desc) = (unsafe { chain.GetDesc() }) else {
            log("OpenXRManager: Present hook could not read swapchain desc.\n");
            return;
        };
        let index = chain.cast::<IDXGISwapChain3>().map(|c| unsafe { c.GetCurrentBackBufferIndex() }).unwrap_or(0);
        let back: Option<ID3D12Resource> = unsafe { chain.GetBuffer(index) }.ok();
        let resource = back.as_ref().map(|b| unsafe { b.GetDesc() }).unwrap_or_default();
        let buffer = &desc.BufferDesc;

        // Zeroed, as the .cpp's `{}` arrays were (an unused eye keeps an all-zero pose).
        // SAFETY: plain floats.
        let (mut poses, mut centres, mut fovs): ([xr::Posef; 2], [xr::Posef; 2], [xr::Fovf; 2]) =
            unsafe { (core::mem::zeroed(), core::mem::zeroed(), core::mem::zeroed()) };
        let mut views = [false; 2];
        if mono {
            let (fov_width, fov_height) = mono_size((buffer.Width, buffer.Height), (resource.Width, resource.Height));
            let mut inputs =
                MonoInputs { present: count, fov_width, fov_height, asym_guard: 0.0, asym_target: 0.0, asym_active: false };
            // SAFETY: the host's call; the outputs are arrays of two.
            unsafe {
                cvr_projection_window_params(true, &mut inputs.asym_guard, &mut inputs.asym_target);
                inputs.asym_active = core::ptr::read_volatile(&raw const CyberpunkVR_AsymProjection) == 1;
                crate::mono_eyes::cvr_xr_mono_eye_poses(
                    &inputs,
                    poses.as_mut_ptr(),
                    centres.as_mut_ptr(),
                    fovs.as_mut_ptr(),
                    views.as_mut_ptr(),
                );
            }
        }
        if mono {
            if let Some(back) = back.as_ref() {
                // SAFETY: the host's call with this present's back buffer and poses.
                let phase = crate::xr_trace::phase_start();
                let ok = unsafe {
                    (h.capture_mono)(
                        h.ctx,
                        back.as_raw(),
                        &resource,
                        count,
                        poses.as_ptr(),
                        fovs.as_ptr(),
                        views.as_ptr(),
                        centres.as_ptr(),
                    )
                };
                crate::xr_trace::phase_end(crate::xr_trace::PHASE_CAPTURE, phase);
                if !ok && count % 300 == 1 {
                    log(&mono_failure_line(count, views));
                }
            }
        }
        // SAFETY: the host's calls.
        unsafe { (h.ticks)(h.ctx) };
        let engine_stereo = unsafe { cvr_engine_stereo_enabled() };
        let (width, height, format) = presented(
            (buffer.Width, buffer.Height, buffer.Format.0 as u32),
            (resource.Width, resource.Height, resource.Format.0 as u32),
            engine_stereo,
        );
        // Under the present lock (held to the end): the frame last held released, this one recorded.
        let held = presented_frame.lock.lock();
        // SAFETY: the cells under the lock; the frame last held is an ID3D12Resource this side releases once.
        unsafe {
            let last = *presented_frame.back_buffer.get();
            if !last.is_null() {
                drop(ID3D12Resource::from_raw(last));
                *presented_frame.back_buffer.get() = core::ptr::null_mut();
            }
            *presented_frame.width.get() = width;
            *presented_frame.height.get() = height;
            *presented_frame.format.get() = format;
            *presented_frame.buffer_index.get() = index;
            *presented_frame.serial.get() = count;
        }
        drop(back);
        // SAFETY: the C++ half's switch.
        let diagnostics = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0;
        if diagnostics && count % 300 == 1 {
            let running = crate::events::session_running();
            log(&present_line(
                desc.OutputWindow.0 as usize,
                (buffer.Width, buffer.Height),
                buffer.Format.0 as u32,
                index,
                (resource.Width, resource.Height),
                running,
            ));
        }
        drop(held);
    }
}

#[cfg(test)]
mod tests {
    //! The sizes and the log lines.
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(mono_size((2064, 2208), (4128, 2208)), (2064.0, 2208.0));
        // An unset swapchain size falls back to the back buffer's, only when the back buffer's is known.
        assert_eq!(mono_size((0, 0), (1920, 1080)), (1920.0, 1080.0));
        assert_eq!(mono_size((1, 900), (1920, 1080)), (1920.0, 1080.0));
        assert_eq!(mono_size((0, 0), (0, 1080)), (0.0, 0.0));
        // The back buffer's size and format win; engine stereo halves the width.
        assert_eq!(presented((800, 600, 28), (1920, 1080, 87), false), (1920, 1080, 87));
        assert_eq!(presented((800, 600, 28), (0, 0, 0), false), (800, 600, 28));
        assert_eq!(presented((800, 600, 28), (4128, 2208, 87), true), (2064, 2208, 87));
        assert_eq!(presented((801, 600, 28), (0, 0, 0), true), (400, 600, 28));
    }
    #[test]
    fn lines() {
        assert_eq!(mono_failure_line(301, [true, false]), "OpenXRManager: Mono capture failed. serial=301 views=(1,0)\n");
        assert_eq!(
            present_line(0xA0B1C, (1920, 1080), 87, 2, (1920, 1080), true),
            "OpenXRManager: Present observed. hwnd=00000000000A0B1C size=1920x1080 format=87 backbufferIndex=2 resourceWidth=1920 resourceHeight=1080 sessionRunning=1\n"
        );
    }
}
