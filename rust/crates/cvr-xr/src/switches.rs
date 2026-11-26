//! The manager's switches, owned here (formerly OpenXRManager's members): the hand-tracking mode (4), weapon aim (on),
//! the visual holsters (on), the hands-only body (off), the diagnostic request counter and the shared slots' block the
//! present keeps for GetSharedSlot. include/Runtimes/OpenXRManager.hpp's members are references to them; the present
//! (present.rs) reads them here.
use core::sync::atomic::{AtomicI32, AtomicPtr};

#[repr(C)]
pub struct SwitchesShared {
    pub hand_tracking_mode: AtomicI32,
    pub weapon_aim_enable: AtomicI32,
    pub immersive_holsters: AtomicI32,
    pub body_hands_only: AtomicI32,
    pub log_diag_request: AtomicI32,
    /// The shared slots' block (the C++ half reads it as a plain float*).
    pub shared_hands: AtomicPtr<f32>,
    /// 1 = the physical reload highlights the magazine and the bolt to rack (published inverted to slot 174).
    pub reload_glow: AtomicI32,
}
const _: () = assert!(core::mem::size_of::<SwitchesShared>() == 40, "mirrored by include/Runtimes/OpenXRManager.hpp");

pub static SWITCHES: SwitchesShared = SwitchesShared {
    hand_tracking_mode: AtomicI32::new(4),
    weapon_aim_enable: AtomicI32::new(1),
    immersive_holsters: AtomicI32::new(1),
    body_hands_only: AtomicI32::new(0),
    log_diag_request: AtomicI32::new(0),
    shared_hands: AtomicPtr::new(core::ptr::null_mut()),
    reload_glow: AtomicI32::new(1),
};
#[no_mangle]
pub extern "C" fn cvr_xr_switches() -> *const SwitchesShared {
    &SWITCHES
}

#[cfg(test)]
mod tests {
    //! The defaults the C++ members had.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn defaults() {
        let s = &SWITCHES;
        let values = [&s.hand_tracking_mode, &s.weapon_aim_enable, &s.immersive_holsters, &s.body_hands_only, &s.log_diag_request]
            .map(|v| v.load(Relaxed));
        assert_eq!(values, [4, 1, 1, 0, 0]);
        assert!(s.shared_hands.load(Relaxed).is_null());
        assert_eq!(s.reload_glow.load(Relaxed), 1);
    }
}
