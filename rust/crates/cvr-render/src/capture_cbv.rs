//! The constant-buffer view hook's probes (src/Stereo/Capture.cpp, `hk_CreateCBV` after the camera upload): for a view of
//! 768 bytes or more, the per-view camera constants (the bytes from the mapped upload ring, else from the copy record;
//! noted with the view the node draws for, or the stage counted when neither has them); then the grading LUT's capture
//! of every non-empty view; then, for a 256-byte view made inside a node, the view constants at its head, noted by the
//! node's work RVA (and dumped whole for the node CyberpunkVR_CbvDumpNode names).
//!
//! The C++ hook keeps its descriptor, source and upload observers and calls `cvr_capture_cbv_views` with the view's
//! GPU address and size, the node's view and work, and the image base. The arithmetic and the readers it uses are
//! Rust's (upload_maps, census_pso, view_constants, grading_lut).
use crate::census_pso::cvr_census_filled_cpu_for_va;
use crate::grading_lut::cvr_grading_gcb_cbv_note;
use crate::upload_maps::mapped_cpu_for_va;
use crate::view_constants::{cvr_camcb_note, cvr_camcb_stages, cvr_cbv_dump_note, cvr_cbv_probe_note, cvr_cbv_read_head};

#[allow(non_upper_case_globals)]
extern "C" {
    /// src/Stereo/ViewConstants.cpp: the probes' switches and the dumped node.
    static CyberpunkVR_CamCbProbe: i32;
    static CyberpunkVR_CbvProbe: i32;
    static CyberpunkVR_CbvDumpNode: u32;
    /// The camera probe's count of views over 768 bytes (ViewConstants.cpp; cvr_camcb_stages logs it).
    static g_cc_big: core::sync::atomic::AtomicU64;
}
/// A C++ switch, read as the .cpp reads it.
///
/// # Safety
/// `value` is a live global.
unsafe fn get<T: Copy>(value: *const T) -> T {
    unsafe { core::ptr::read_volatile(value) }
}

/// The RVA of a node's work inside the image, when the node is one (its work past the base).
pub fn node_rva(work: usize, base: usize) -> Option<u32> {
    (base != 0 && work > base).then(|| (work - base) as u32)
}

/// hk_CreateCBV's probes for a view at `va` of `size` bytes.
#[no_mangle]
pub extern "C" fn cvr_capture_cbv_views(va: u64, size: u32, vrcam: bool, work: usize, base: usize) {
    // SAFETY: the C++ half's switches.
    if unsafe { get(&raw const CyberpunkVR_CamCbProbe) } != 0 && size >= 768 {
        // SAFETY: the C++ half's counter.
        unsafe { g_cc_big.fetch_add(1, core::sync::atomic::Ordering::Relaxed) };
        let mut cp = mapped_cpu_for_va(va, 848) as *const u8;
        if cp.is_null() {
            cp = cvr_census_filled_cpu_for_va(va, 848);
        }
        // SAFETY: the camera block's bytes, or none.
        unsafe {
            if cp.is_null() {
                cvr_camcb_stages();
            } else {
                cvr_camcb_note(cp, vrcam);
            }
        }
    }
    if size != 0 {
        cvr_grading_gcb_cbv_note(va, size);
    }
    // SAFETY: the C++ half's switch.
    if unsafe { get(&raw const CyberpunkVR_CbvProbe) } == 0 || size != 256 {
        return;
    }
    let Some(rva) = node_rva(work, base) else { return };
    let p = mapped_cpu_for_va(va, 256) as *const u8;
    if p.is_null() {
        return;
    }
    let (mut xy, mut count) = ([0.0f32; 2], 0u32);
    // SAFETY: 256 mapped bytes; out-parameters.
    if !unsafe { cvr_cbv_read_head(p, xy.as_mut_ptr(), &mut count) } {
        return;
    }
    // SAFETY: the view constants' recorder; the block's bytes.
    unsafe {
        cvr_cbv_probe_note(rva, count, vrcam);
        if rva == get(&raw const CyberpunkVR_CbvDumpNode) {
            cvr_cbv_dump_note(p, vrcam);
        }
    }
}
