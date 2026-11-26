//! The render threads' state (formerly C++ thread-locals of SyncStereo.cpp, NodeDispatch.cpp, ViewReuse.cpp,
//! DeviceHooks.cpp and VisionLayer.cpp, lent to Rust by index through Capture.cpp's CaptureTls): one block per thread.
//! include/Stereo/RenderTls.hpp gives the C++ half of the Rust build each name as this block's field (a macro over an
//! inline reference function), so its uses read and write the same storage; capture_hooks.rs's Tls indexes the fields.

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::{addr_of_mut, null_mut};

/// D3D12_RESOURCE_STATE_RENDER_TARGET.
const RENDER_TARGET: u32 = 0x4;

/// The block (include/Stereo/RenderTls.hpp's RenderTls), in capture_hooks.rs's Tls order.
#[repr(C)]
pub struct RenderTls {
    pub current_node_work: usize,
    pub cascade_idx: i32,
    pub vrcam_node_active: bool,
    pub cur_rt_w: u32,
    pub cur_rt_h: u32,
    pub mirror_copy_node_active: bool,
    pub in_vrcam_2rt: bool,
    pub cb_armed_2rt: bool,
    pub tm_rt0: *mut c_void,
    pub tm_from_final: bool,
    pub tm_rt0_list: *mut c_void,
    pub tm_rt0_state: u32,
    pub tm_rt0_state_seen: bool,
    pub tm_consumed: bool,
    pub mirror_copy_rtv: *mut c_void,
    pub mirror_copy_rtv_format: u32,
    pub mirror_copy_list: *mut c_void,
    pub mirror_src_state: u32,
    pub mirror_src_state_seen: bool,
    pub final_res: *mut c_void,
    pub final_state: u32,
    pub final_list: *mut c_void,
    pub active_view_known: bool,
    pub active_view_key: u64,
    pub vision_node: usize,
    pub vision_ord: i32,
    pub last_disp: [u32; 3],
    pub current_node_view: usize,
    pub view_side: i32,
    pub prof_disp_depth: i32,
    pub prof_child_ticks: i64,
}
const _: () = assert!(core::mem::size_of::<RenderTls>() == 184 && core::mem::offset_of!(RenderTls, view_side) == 168);
const INIT: RenderTls = RenderTls {
    current_node_work: 0,
    cascade_idx: -1,
    vrcam_node_active: false,
    cur_rt_w: 0,
    cur_rt_h: 0,
    mirror_copy_node_active: false,
    in_vrcam_2rt: false,
    cb_armed_2rt: false,
    tm_rt0: null_mut(),
    tm_from_final: false,
    tm_rt0_list: null_mut(),
    tm_rt0_state: RENDER_TARGET,
    tm_rt0_state_seen: false,
    tm_consumed: true,
    mirror_copy_rtv: null_mut(),
    mirror_copy_rtv_format: 0,
    mirror_copy_list: null_mut(),
    mirror_src_state: RENDER_TARGET,
    mirror_src_state_seen: false,
    final_res: null_mut(),
    final_state: 0,
    final_list: null_mut(),
    active_view_known: false,
    active_view_key: 0,
    vision_node: 0,
    vision_ord: 0,
    last_disp: [0; 3],
    current_node_view: 0,
    view_side: -1,
    prof_disp_depth: 0,
    prof_child_ticks: 0,
};
thread_local! {
    static BLOCK: UnsafeCell<RenderTls> = const { UnsafeCell::new(INIT) };
}
/// This thread's block.
#[no_mangle]
pub extern "C" fn cvr_render_tls() -> *mut RenderTls {
    BLOCK.with(UnsafeCell::get)
}
// SAFETY (all): this thread's live block, a plain field read or write.
/// t_view_side: the view this thread's node dispatch renders (-1 none, 0 main, 1 the second).
pub fn view_side() -> i32 {
    unsafe { (*cvr_render_tls()).view_side }
}
/// t_current_node_work: the render node this thread works.
pub fn current_node_work() -> usize {
    unsafe { (*cvr_render_tls()).current_node_work }
}
/// t_current_node_view: the view of that node.
pub fn current_node_view() -> usize {
    unsafe { (*cvr_render_tls()).current_node_view }
}
/// t_vrcam_node_active: inside a node dispatch of the second view on this thread.
pub fn vrcam_node_active() -> bool {
    unsafe { (*cvr_render_tls()).vrcam_node_active }
}
/// t_cascade_idx and its write.
pub fn cascade_idx() -> i32 {
    unsafe { (*cvr_render_tls()).cascade_idx }
}
pub fn set_cascade_idx(index: i32) {
    unsafe { (*cvr_render_tls()).cascade_idx = index };
}
/// This thread's field `which` (capture_hooks.rs's Tls), null past them.
pub fn field(which: u32) -> *mut c_void {
    let p = cvr_render_tls();
    // SAFETY: this thread's live block; only field addresses are formed.
    unsafe {
        match which {
        0 => addr_of_mut!((*p).current_node_work).cast(),
        1 => addr_of_mut!((*p).cascade_idx).cast(),
        2 => addr_of_mut!((*p).vrcam_node_active).cast(),
        3 => addr_of_mut!((*p).cur_rt_w).cast(),
        4 => addr_of_mut!((*p).cur_rt_h).cast(),
        5 => addr_of_mut!((*p).mirror_copy_node_active).cast(),
        6 => addr_of_mut!((*p).in_vrcam_2rt).cast(),
        7 => addr_of_mut!((*p).cb_armed_2rt).cast(),
        8 => addr_of_mut!((*p).tm_rt0).cast(),
        9 => addr_of_mut!((*p).tm_from_final).cast(),
        10 => addr_of_mut!((*p).tm_rt0_list).cast(),
        11 => addr_of_mut!((*p).tm_rt0_state).cast(),
        12 => addr_of_mut!((*p).tm_rt0_state_seen).cast(),
        13 => addr_of_mut!((*p).tm_consumed).cast(),
        14 => addr_of_mut!((*p).mirror_copy_rtv).cast(),
        15 => addr_of_mut!((*p).mirror_copy_rtv_format).cast(),
        16 => addr_of_mut!((*p).mirror_copy_list).cast(),
        17 => addr_of_mut!((*p).mirror_src_state).cast(),
        18 => addr_of_mut!((*p).mirror_src_state_seen).cast(),
        19 => addr_of_mut!((*p).final_res).cast(),
        20 => addr_of_mut!((*p).final_state).cast(),
        21 => addr_of_mut!((*p).final_list).cast(),
        22 => addr_of_mut!((*p).active_view_known).cast(),
        23 => addr_of_mut!((*p).active_view_key).cast(),
        24 => addr_of_mut!((*p).vision_node).cast(),
        25 => addr_of_mut!((*p).vision_ord).cast(),
        26 => addr_of_mut!((*p).last_disp).cast(),
        27 => addr_of_mut!((*p).current_node_view).cast(),
        28 => addr_of_mut!((*p).view_side).cast(),
        29 => addr_of_mut!((*p).prof_disp_depth).cast(),
        30 => addr_of_mut!((*p).prof_child_ticks).cast(),
            _ => null_mut(),
        }
    }
}
