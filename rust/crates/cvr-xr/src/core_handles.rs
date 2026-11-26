//! The manager's core handles (formerly OpenXRManager's own members, include/Runtimes/OpenXRManager.hpp): whether it is
//! initialised, the instance and its system, the headset's name and the runtime's eye image limit, the session with its
//! tracking, view and hand spaces, the action set with the hand pose, aim and haptic actions and the hands' paths, the
//! D3D12 graphics binding and the submit thread. The C++ members are references to this block (CVR_RUST_VRIK), so both
//! halves read and write the same handles; like the members they were, they are plain fields (the manager's own
//! sequencing -- Init, InitGraphics, Shutdown under its init lock -- orders them).
use core::cell::UnsafeCell;
use core::ffi::c_void;

/// CvrXrCoreShared.
#[repr(C)]
pub struct XrCore {
    pub initialized: bool,
    pub instance: u64,
    pub system_id: u64,
    pub system_name: [u8; 128],
    pub eye_image_limit: [u32; 2],
    pub session: u64,
    pub local_space: u64,
    pub view_space: u64,
    pub action_set: u64,
    pub hand_pose_action: u64,
    pub hand_aim_pose_action: u64,
    pub hand_aim_spaces: [u64; 2],
    pub haptic_action: u64,
    pub hand_paths: [u64; 2],
    pub hand_spaces: [u64; 2],
    /// XrGraphicsBindingD3D12KHR: type (and padding), next, device, queue.
    pub graphics_binding: [u64; 4],
    pub frame_thread: *mut c_void,
}
const _: () = assert!(
    core::mem::size_of::<XrCore>() == 304
        && core::mem::offset_of!(XrCore, system_name) == 24
        && core::mem::offset_of!(XrCore, session) == 160
        && core::mem::offset_of!(XrCore, hand_spaces) == 248
        && core::mem::offset_of!(XrCore, graphics_binding) == 264
        && core::mem::offset_of!(XrCore, frame_thread) == 296
);

struct Shared(UnsafeCell<XrCore>);
// SAFETY: as the C++ members were: plain fields the manager's own sequencing orders.
unsafe impl Sync for Shared {}
static SHARED: Shared = Shared(UnsafeCell::new(XrCore {
    initialized: false,
    instance: 0,
    system_id: 0,
    system_name: [0; 128],
    eye_image_limit: [0; 2],
    session: 0,
    local_space: 0,
    view_space: 0,
    action_set: 0,
    hand_pose_action: 0,
    hand_aim_pose_action: 0,
    hand_aim_spaces: [0; 2],
    haptic_action: 0,
    hand_paths: [0; 2],
    hand_spaces: [0; 2],
    graphics_binding: [0; 4],
    frame_thread: core::ptr::null_mut(),
}));

/// The block (for the C++ members' references).
#[no_mangle]
pub extern "C" fn cvr_xr_core() -> *mut XrCore {
    SHARED.0.get()
}
/// The block, for this crate.
///
/// # Safety
/// The caller follows the manager's sequencing (no two writers of one field at a time), as the C++ members did.
#[allow(clippy::mut_from_ref)]
pub unsafe fn core() -> &'static mut XrCore {
    // SAFETY: as documented.
    unsafe { &mut *SHARED.0.get() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_starts_empty_and_is_one_object() {
        let a = cvr_xr_core();
        assert_eq!(a, cvr_xr_core());
        // SAFETY: the test thread alone.
        let c = unsafe { &*a };
        assert!(!c.initialized && c.instance == 0 && c.session == 0 && c.frame_thread.is_null());
    }
}
