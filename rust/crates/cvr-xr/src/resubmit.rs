//! The last accepted frame sent once more (formerly OpenXRManager::ResubmitLastFrame): a display cycle with nothing new
//! ends with the frame the runtime last accepted -- its projection (or the menu quad) and its extra layers -- as long
//! as it still names the current swapchains and space and is under a second old. A depth swapchain recreated since is
//! dropped from the layer, not the frame. Refused once, it is not offered again.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_host::log;
use openxr_sys as xr;
use xr::Handle as _;

/// OpenXRManager::LastSubmission (include/Runtimes/OpenXRManager.hpp), field for field.
#[repr(C)]
pub struct LastSubmission {
    pub valid: bool,
    pub menu_quad: bool,
    pub view_count: u32,
    pub submitted_at: xr::Time,
    pub local_space: xr::Space,
    pub color: [xr::Swapchain; 2],
    pub depth: [xr::Swapchain; 2],
    pub views: [xr::CompositionLayerProjectionView; 2],
    pub depth_info: [xr::CompositionLayerDepthInfoKHR; 2],
    pub quad: xr::CompositionLayerQuad,
    pub extra: [*const xr::CompositionLayerBaseHeader; MAX_EXTRA],
    pub extra_count: u32,
}
pub const MAX_EXTRA: usize = 32;
const _: () = assert!(core::mem::size_of::<LastSubmission>() == 752);
const _: () = assert!(core::mem::offset_of!(LastSubmission, quad) == 376 && core::mem::offset_of!(LastSubmission, extra_count) == 744);

/// TEN SECONDS WITH NO NEW FRAME, NOT ONE (#243): under SteamVR with Steam Link an empty end is worse than a held
/// picture -- a run of them makes Steam Link reset its stream and SteamVR hide the session -- while a held frame is
/// reprojected with the head and gives way the moment a new one comes. Longer than that it is a frozen world.
const MAX_HOLD_NS: i64 = 10_000_000_000;

/// Whether the last frame can be sent at `display_time` against the current swapchains (`eyes`, `depths`) and space;
/// on success the views' depth links are refreshed for the depth swapchains that still exist.
pub fn usable(last: &mut LastSubmission, local: xr::Space, eyes: &[xr::Swapchain], depths: &[xr::Swapchain], display_time: i64) -> bool {
    if !last.valid || last.local_space != local || last.view_count as usize > eyes.len() {
        return false;
    }
    if display_time - last.submitted_at.as_nanos() > MAX_HOLD_NS {
        return false;
    }
    for eye in 0..last.view_count as usize {
        if last.color[eye] != eyes[eye] {
            return false;
        }
        let keep_depth = last.depth[eye] != xr::Swapchain::NULL && last.depth[eye] == depths[eye];
        last.views[eye].next =
            if keep_depth { (&last.depth_info[eye] as *const xr::CompositionLayerDepthInfoKHR).cast() } else { core::ptr::null() };
    }
    true
}

#[cfg(not(test))]
extern "system" {
    fn xrEndFrame(session: xr::Session, info: *const xr::FrameEndInfo) -> xr::Result;
}

/// OpenXRManager::ResubmitLastFrame. False when there is none to send (the caller ends the cycle empty).
///
/// # Safety
/// `last` is OpenXRManager's; `eyes`/`depths` hold `eye_count` swapchain handles each; the handles are live.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_resubmit_last_frame(
    last: *mut LastSubmission,
    session: u64,
    local_space: u64,
    eyes: *const u64,
    depths: *const u64,
    eye_count: u32,
    display_time: i64,
) -> bool {
    // SAFETY (whole body): the caller's record and handle arrays; the layers it names are its own, alive until the
    // next submission replaces them.
    unsafe {
        let Some(last) = last.as_mut() else { return false };
        let n = eye_count as usize;
        let eyes: Vec<xr::Swapchain> = (0..n).map(|i| xr::Swapchain::from_raw(*eyes.add(i))).collect();
        let depths: Vec<xr::Swapchain> = (0..n).map(|i| xr::Swapchain::from_raw(*depths.add(i))).collect();
        let local = xr::Space::from_raw(local_space);
        if !usable(last, local, &eyes, &depths, display_time) {
            return false;
        }
        let mut projection = xr::CompositionLayerProjection {
            ty: xr::StructureType::COMPOSITION_LAYER_PROJECTION,
            next: core::ptr::null(),
            layer_flags: xr::CompositionLayerFlags::EMPTY,
            space: local,
            view_count: 0,
            views: core::ptr::null(),
        };
        let mut layers: [*const xr::CompositionLayerBaseHeader; 1 + MAX_EXTRA] = [core::ptr::null(); 1 + MAX_EXTRA];
        if last.menu_quad {
            last.quad.space = local;
            layers[0] = (&last.quad as *const xr::CompositionLayerQuad).cast();
        } else {
            projection.view_count = last.view_count;
            projection.views = last.views.as_ptr();
            layers[0] = (&projection as *const xr::CompositionLayerProjection).cast();
        }
        // The extras point at the persistent, mutable overlay objects, not copies: validated again on held-frame
        // cycles, not only when each HUD was prepared.
        last.extra_count = crate::hud_quad::filter_submittable_quads(last.extra.as_mut_ptr(), (last.extra_count as usize).min(MAX_EXTRA) as u32);
        let mut count = 1usize;
        for i in 0..(last.extra_count as usize).min(MAX_EXTRA) {
            layers[count] = last.extra[i];
            count += 1;
        }
        let info = xr::FrameEndInfo {
            ty: xr::StructureType::FRAME_END_INFO,
            next: core::ptr::null(),
            display_time: xr::Time::from_nanos(display_time),
            environment_blend_mode: xr::EnvironmentBlendMode::OPAQUE,
            layer_count: count as u32,
            layers: layers.as_ptr(),
        };
        let res = xrEndFrame(xr::Session::from_raw(session), &info);
        if res.into_raw() >= 0 {
            return true;
        }
        last.valid = false;
        log!("OpenXRManager: resubmitting the last frame failed (res={}); ending this cycle empty\n", res.into_raw());
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> LastSubmission {
        // SAFETY: plain C structures, all-zero is their value-initialised state.
        let mut last: LastSubmission = unsafe { core::mem::zeroed() };
        last.valid = true;
        last.view_count = 2;
        last.submitted_at = xr::Time::from_nanos(1_000);
        last.local_space = xr::Space::from_raw(7);
        last.color = [xr::Swapchain::from_raw(11), xr::Swapchain::from_raw(12)];
        last.depth = [xr::Swapchain::from_raw(21), xr::Swapchain::from_raw(22)];
        last
    }
    #[test]
    fn the_last_frame_is_sent_while_it_names_what_exists() {
        let eyes = [xr::Swapchain::from_raw(11), xr::Swapchain::from_raw(12)];
        let mut last = record();
        assert!(usable(&mut last, xr::Space::from_raw(7), &eyes, &[xr::Swapchain::from_raw(21), xr::Swapchain::from_raw(99)], 2_000));
        assert!(!last.views[0].next.is_null(), "its depth still exists");
        assert!(last.views[1].next.is_null(), "a recreated depth swapchain is dropped from the layer");
        assert!(!usable(&mut record(), xr::Space::from_raw(8), &eyes, &eyes, 2_000), "another space");
        assert!(!usable(&mut record(), xr::Space::from_raw(7), &[eyes[0], xr::Swapchain::from_raw(13)], &eyes, 2_000), "a new swapchain");
        assert!(usable(&mut record(), xr::Space::from_raw(7), &eyes, &eyes, 1_000 + 3_000_000_000), "a 3 s stall is held");
        assert!(!usable(&mut record(), xr::Space::from_raw(7), &eyes, &eyes, 1_000 + MAX_HOLD_NS + 1), "held over ten seconds");
    }
}

#[cfg(not(test))]
#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_XrResubmits: core::sync::atomic::AtomicU64;
    static CyberpunkVR_XrEmptyEnds: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrEnds: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugXrEndsEmpty: core::sync::atomic::AtomicU64;
}

/// NOTHING NEW THIS CYCLE (FrameThreadMain): the last accepted frame sent again with its own poses, so the runtime
/// reprojects it to this display time; only when that is impossible (no frame yet, swapchains recreated, the runtime
/// not rendering, or a second without a new frame) the end goes out empty -- a display frame with nothing in it. Both
/// counted apart from the real submit.
///
/// # Safety
/// As cvr_xr_resubmit_last_frame.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_end_without_picture(
    should_render: bool,
    display_time: i64,
    last: *mut LastSubmission,
    session: u64,
    local_space: u64,
    eyes: *const u64,
    depths: *const u64,
    eye_count: u32,
) {
    use core::sync::atomic::Ordering;
    let diag = crate::present_diag_enabled;
    // SAFETY: the caller's record and handles; the C++ half's counters.
    unsafe {
        if should_render && cvr_xr_resubmit_last_frame(last, session, local_space, eyes, depths, eye_count, display_time) {
            CyberpunkVR_XrResubmits.fetch_add(1, Ordering::Relaxed);
            if diag() {
                CyberpunkVR_DebugXrEnds.fetch_add(1, Ordering::Relaxed);
            }
            crate::xr_rates::cvr_xr_mark(b'E');
            return;
        }
        let info = xr::FrameEndInfo {
            ty: xr::StructureType::FRAME_END_INFO,
            next: core::ptr::null(),
            display_time: xr::Time::from_nanos(display_time),
            environment_blend_mode: xr::EnvironmentBlendMode::OPAQUE,
            layer_count: 0,
            layers: core::ptr::null(),
        };
        CyberpunkVR_XrEmptyEnds.fetch_add(1, Ordering::Relaxed);
        if diag() {
            CyberpunkVR_DebugXrEnds.fetch_add(1, Ordering::Relaxed);
        }
        if diag() {
            CyberpunkVR_DebugXrEndsEmpty.fetch_add(1, Ordering::Relaxed);
        }
        crate::xr_rates::cvr_xr_mark(b'e');
        xrEndFrame(xr::Session::from_raw(session), &info);
    }
}

/// What the cycle's end submitted (FrameThreadMain), recorded for cvr_xr_resubmit_last_frame: the projection views
/// (at most two, their depth links copied out), the menu's quad, every layer after the first. Valid when every view
/// located was recorded.
///
/// # Safety
/// `last` is OpenXRManager's record; `views` holds `view_count` views (their `next` null or a depth info); `color`
/// holds min(view_count, 2) handles; `quad` is valid; `layers` holds `layer_count` pointers.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_record_submission(
    last: *mut LastSubmission,
    views: *const xr::CompositionLayerProjectionView,
    view_count: u32,
    menu_quad: bool,
    local_space: u64,
    display_time: i64,
    color: *const u64,
    quad: *const xr::CompositionLayerQuad,
    layers: *const *const xr::CompositionLayerBaseHeader,
    layer_count: u32,
) {
    // SAFETY (whole body): the caller's record, views, handles and layers as documented.
    unsafe {
        let Some(last) = last.as_mut() else { return };
        let n = view_count.min(2);
        last.view_count = n;
        last.menu_quad = menu_quad;
        last.local_space = xr::Space::from_raw(local_space);
        last.submitted_at = xr::Time::from_nanos(display_time);
        for eye in 0..n as usize {
            let v = &*views.add(eye);
            last.views[eye] = *v;
            last.color[eye] = xr::Swapchain::from_raw(*color.add(eye));
            last.depth[eye] = xr::Swapchain::NULL;
            last.views[eye].next = core::ptr::null();
            let depth = v.next as *const xr::CompositionLayerDepthInfoKHR;
            if !depth.is_null() && (*depth).ty == xr::StructureType::COMPOSITION_LAYER_DEPTH_INFO_KHR {
                last.depth_info[eye] = *depth;
                last.depth_info[eye].next = core::ptr::null();
                last.depth[eye] = (*depth).sub_image.swapchain;
            }
        }
        if let Some(q) = quad.as_ref() {
            last.quad = *q;
        }
        last.extra_count = 0;
        for i in 1..layer_count as usize {
            if last.extra_count as usize >= MAX_EXTRA {
                break;
            }
            last.extra[last.extra_count as usize] = *layers.add(i);
            last.extra_count += 1;
        }
        last.valid = n == view_count && n > 0;
    }
}
