//! The engine hooks the stereo install pass creates directly (formerly the blocks of src/Stereo/SyncStereo.cpp's
//! install_desc_ring_probe): the volumetric clouds node, the sky work, the node dispatcher (once) and the per-view
//! capability test -- MinHook straight at their RVAs, after the registered detours, each with its own log line. The
//! originals are Rust statics of the clouds probe, the view reuse, the node dispatch and the node features.

use core::ffi::c_void;
use core::ptr::addr_of;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};

const CLOUDS_NODE_RVA: usize = 0x7F_23B4;
const SKY_WORK_RVA: usize = 0x7F_8714;
const NODE_DISPATCH_RVA: usize = 0x33_2F90;
const VIEW_FEATURE_CHECK_RVA: usize = 0x5D_8D48;
const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_CREATED: i32 = 3;

use crate::node_dispatch::cvr_node_dispatch_original;
use crate::node_features::cvr_view_feature_check_original;
use crate::node_probes::cvr_clouds_node_original;
use crate::view_reuse::cvr_sky_work_original;
/// g_node_dispatch_hooked: the dispatcher is hooked once.
static NODE_DISPATCH_HOOKED: AtomicBool = AtomicBool::new(false);

extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
}
extern "C" {
    /// The stereo log (src/Stereo/ListDetourHost.cpp).
    fn cvr_cpp_stereo_log(text: *const core::ffi::c_char);
}
fn log(text: String) {
    let mut bytes = text.into_bytes();
    bytes.push(0);
    // SAFETY: a terminated string.
    unsafe { cvr_cpp_stereo_log(bytes.as_ptr().cast()) };
}
/// MH_CreateHook with the original into its slot.
fn create(target: usize, detour: *const (), slot: &AtomicUsize) -> i32 {
    // SAFETY: an engine function and a detour of its signature; the slot is an AtomicUsize (a pointer's layout).
    unsafe { MH_CreateHook(target as *mut c_void, detour.cast_mut().cast(), slot.as_ptr().cast()) }
}
fn enable(target: usize) -> i32 {
    // SAFETY: a hook created above.
    unsafe { MH_EnableHook(target as *mut c_void) }
}

/// install_desc_ring_probe's direct hooks (MinHook initialised, the registered detours installed).
#[no_mangle]
pub extern "C" fn cvr_install_engine_hooks(base: usize) {
    let clouds = base + CLOUDS_NODE_RVA;
    if create(clouds, crate::node_probes::cvr_nd_clouds_node as *const (), &cvr_clouds_node_original) == MH_OK && enable(clouds) == MH_OK {
        log(format!("[cloudnode] clouds node sub_1407F23B4 hooked @{clouds:016X}"));
    } else {
        log(format!("[cloudnode] failed to hook the clouds node @{clouds:016X}"));
    }
    let sky = base + SKY_WORK_RVA;
    if create(sky, crate::view_reuse::cvr_reuse_sky_work as *const (), &cvr_sky_work_original) == MH_OK && enable(sky) == MH_OK {
        // SAFETY: an exported control.
        let mode = unsafe { core::ptr::read_volatile(addr_of!(crate::declared_moved_exports::CyberpunkVR_SkyReuseMode)) };
        log(format!("[sky] sky work sub_1407F8714 hooked @{sky:016X} (SkyReuseMode={mode})"));
    } else {
        log(format!("[sky] failed to hook sky work @{sky:016X} -- both views keep filling one sky"));
    }
    if !NODE_DISPATCH_HOOKED.load(Acquire) {
        let node = base + NODE_DISPATCH_RVA;
        let status = create(node, crate::node_dispatch::cvr_nd_dispatch as *const (), &cvr_node_dispatch_original);
        if status == MH_OK && enable(node) == MH_OK {
            NODE_DISPATCH_HOOKED.store(true, Release);
            log(format!("[node] node dispatcher sub_140332F90 hooked @{node:016X} (view key published)"));
        } else {
            let owned = if status == MH_ERROR_ALREADY_CREATED { " ALREADY_CREATED -- another hook owns it" } else { "" };
            log(format!("[node] failed to hook node dispatcher @{node:016X} (MH_STATUS={status}{owned})"));
        }
    }
    let feature = base + VIEW_FEATURE_CHECK_RVA;
    let status = create(feature, crate::node_features::cvr_nd_view_feature_check as *const (), &cvr_view_feature_check_original);
    if status == MH_OK && enable(feature) == MH_OK {
        // SAFETY: an exported control.
        let hud = unsafe { core::ptr::read_volatile(addr_of!(crate::declared_moved_exports::CyberpunkVR_HudInVrcam)) };
        log(format!("[hud] view capability test sub_1405D8D48 hooked @{feature:016X} (HudInVrcam={hud})"));
    } else {
        log(format!("[hud] failed to hook view capability test @{feature:016X} (MH_STATUS={status}) -- the second eye keeps the engine's HUD refusal"));
    }
}
