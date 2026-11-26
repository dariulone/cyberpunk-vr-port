//! The binocular gaze centre, owned here (formerly include/Camera/GazeCenter.hpp's inline atomics): one gaze ray through
//! the vertical centre of the rendered views -- its tangent and its NDC height -- published by the frame loop from the
//! asymmetric windows each cycle the views carry stats, and whether the head looks along it (the HUD natives switch it
//! off in menus, for the device camera and the head-aimed weapon). The header's atomics are references to these; its
//! Read, CanvasOffset and the C++ shims read them there.
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[repr(C)]
pub struct GazeShared {
    /// cvr::gaze::Center {tangentY, ndcY} as its bits (the tangent in the low half).
    pub center_bits: AtomicU64,
    pub head_look: AtomicBool,
}
const _: () = assert!(core::mem::size_of::<GazeShared>() == 16, "mirrored by include/Camera/GazeCenter.hpp");

pub static GAZE: GazeShared = GazeShared { center_bits: AtomicU64::new(0), head_look: AtomicBool::new(true) };
#[no_mangle]
pub extern "C" fn cvr_xr_gaze_shared() -> *const GazeShared {
    &GAZE
}

/// The centre's bits as std::bit_cast of {tangentY, ndcY} gives them.
pub fn center_bits(tangent_y: f32, ndc_y: f32) -> u64 {
    u64::from(tangent_y.to_bits()) | (u64::from(ndc_y.to_bits()) << 32)
}
/// cvr::gaze::Publish.
pub fn publish(tangent_y: f32, ndc_y: f32) {
    GAZE.center_bits.store(center_bits(tangent_y, ndc_y), Ordering::Release);
}
/// cvr::gaze::Read: the centre while the head looks along it, else none.
pub fn read() -> (f32, f32) {
    if !GAZE.head_look.load(Ordering::Relaxed) {
        return (0.0, 0.0);
    }
    let bits = GAZE.center_bits.load(Ordering::Acquire);
    (f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32))
}

#[cfg(test)]
mod tests {
    //! The bits' order, the published centre read back, and none while the head does not look.
    use super::*;

    #[test]
    fn published_and_read() {
        assert_eq!(center_bits(1.0, 0.0), 0x3F80_0000);
        assert_eq!(center_bits(0.0, -2.0), 0xC000_0000_0000_0000);
        assert!(GAZE.head_look.load(Ordering::Relaxed));
        publish(-0.2, -0.25);
        assert_eq!(read(), (-0.2, -0.25));
        GAZE.head_look.store(false, Ordering::Relaxed);
        assert_eq!(read(), (0.0, 0.0));
        GAZE.head_look.store(true, Ordering::Relaxed);
        publish(0.0, 0.0);
    }
}
