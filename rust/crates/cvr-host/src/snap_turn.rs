//! A snap turn handed to the game (main's cvr::fsr3::NoteSnapTurn, called from OnFootDeltaHead): its count and its angle,
//! for FSR3's cut detection (cvr-render fsr3.rs, camera_jumped) -- the engine does not flag a snap as a cut, and its
//! motion vectors carry it as motion. Here, below both crates: the snap is seen in cvr-rig, FSR3 is cvr-render's.
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

static COUNT: AtomicU32 = AtomicU32::new(0);
static DEGREES: AtomicU32 = AtomicU32::new(0);

/// A snap turn of `degrees` was handed to the game.
pub fn note(degrees: f32) {
    if degrees == 0.0 || !degrees.is_finite() {
        return;
    }
    DEGREES.store(degrees.abs().to_bits(), Relaxed);
    COUNT.fetch_add(1, Relaxed);
}
/// The snaps so far and the last one's angle (degrees, positive).
pub fn latest() -> (u32, f32) {
    let count = COUNT.load(Relaxed);
    (count, f32::from_bits(DEGREES.load(Relaxed)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_and_keeps_the_angle() {
        let (before, _) = super::latest();
        super::note(0.0);
        assert_eq!(super::latest().0, before);
        super::note(-45.0);
        let (after, degrees) = super::latest();
        assert_eq!(after, before + 1);
        assert_eq!(degrees, 45.0);
    }
}
