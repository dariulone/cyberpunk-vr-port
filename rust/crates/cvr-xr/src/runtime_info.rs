//! What the runtime reported, owned here (formerly OpenXRManager's members): the located views' mean horizontal and
//! vertical field of view in degrees and the IPD (the frame loop stores them each cycle the views carry stats), and
//! which runtime the session runs on -- SteamVR, the PSVR2 system, Virtual Desktop, the OpenXR Simulator (the test
//! runtime, whose controllers report the aim direction as their grip pose). include/Runtimes/OpenXRManager.hpp's
//! members are references to them; the frame loop (cvr-render xr_frame_loop.rs) and the hands' publication
//! (published.rs) read them here.
use crate::calibration::AtomicF32;
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};

#[repr(C)]
pub struct RuntimeShared {
    pub horizontal_fov_deg: AtomicF32,
    pub vertical_fov_deg: AtomicF32,
    pub ipd: AtomicF32,
    pub steamvr: AtomicBool,
    pub psvr2: AtomicBool,
    pub virtual_desktop: AtomicBool,
    pub simulator: AtomicBool,
}
const _: () = {
    assert!(core::mem::size_of::<RuntimeShared>() == 16, "mirrored by include/Runtimes/OpenXRManager.hpp");
    assert!(core::mem::offset_of!(RuntimeShared, steamvr) == 12 && core::mem::offset_of!(RuntimeShared, simulator) == 15);
};

pub static RUNTIME: RuntimeShared = RuntimeShared {
    horizontal_fov_deg: AtomicF32::zero(),
    vertical_fov_deg: AtomicF32::zero(),
    ipd: AtomicF32::zero(),
    steamvr: AtomicBool::new(false),
    psvr2: AtomicBool::new(false),
    virtual_desktop: AtomicBool::new(false),
    simulator: AtomicBool::new(false),
};
#[no_mangle]
pub extern "C" fn cvr_xr_runtime_shared() -> *const RuntimeShared {
    &RUNTIME
}

impl RuntimeShared {
    /// The located views' stats (the frame loop, each cycle they carry them).
    pub fn store_view_stats(&self, hfov_deg: f32, vfov_deg: f32, ipd: f32) {
        self.horizontal_fov_deg.store(hfov_deg);
        self.vertical_fov_deg.store(vfov_deg);
        self.ipd.store(ipd);
    }
    pub fn psvr2(&self) -> bool {
        self.psvr2.load(Relaxed)
    }
    pub fn simulator(&self) -> bool {
        self.simulator.load(Relaxed)
    }
}

#[cfg(test)]
mod tests {
    //! The defaults the C++ members had and the stats' order.
    use super::*;

    #[test]
    fn defaults_and_stats() {
        assert!(!RUNTIME.steamvr.load(Relaxed) && !RUNTIME.psvr2() && !RUNTIME.virtual_desktop.load(Relaxed));
        assert!(!RUNTIME.simulator());
        assert_eq!((RUNTIME.horizontal_fov_deg.load(), RUNTIME.ipd.load()), (0.0, 0.0));
        let r = RuntimeShared {
            horizontal_fov_deg: AtomicF32::zero(),
            vertical_fov_deg: AtomicF32::zero(),
            ipd: AtomicF32::zero(),
            steamvr: AtomicBool::new(false),
            psvr2: AtomicBool::new(true),
            virtual_desktop: AtomicBool::new(false),
            simulator: AtomicBool::new(true),
        };
        r.store_view_stats(104.0, 98.5, 0.064);
        assert_eq!((r.horizontal_fov_deg.load(), r.vertical_fov_deg.load(), r.ipd.load()), (104.0, 98.5, 0.064));
        assert!(r.psvr2() && r.simulator());
    }
}
