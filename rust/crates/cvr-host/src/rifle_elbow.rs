//! THE RIFLE ELBOW A/B FIXTURE (main's ce2a00cad, include/Anim/RifleElbowAudit.hpp and CharacterRig.cpp): an
//! attach-only diagnostic. With CyberpunkVR_RifleElbowAB 1 (the swivel before main's fix) or 2 (the corrected one) the
//! right arm's solve (cvr-rig vrik/arm.rs) samples its controller target and pole into CyberpunkVR_RifleElbowSample, and
//! the completed pose's shoulder, elbow and wrist are kept per hand packet; the frame generator's capture
//! (cvr-gpu frame_capture.rs) writes them beside each eye drawn from that packet, so an elbow A/B is matched to its
//! stereo frames. Here, below both crates: the solve is cvr-rig's, the capture cvr-gpu's. 0 (the default) is normal.
use core::cell::Cell;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering::*};
use std::sync::Mutex;

/// 0 normal, 1 the pre-fix swivel, 2 the corrected swivel.
#[no_mangle]
pub static CyberpunkVR_RifleElbowAB: AtomicI32 = AtomicI32::new(0);
/// Odd while the sample is written.
#[no_mangle]
pub static CyberpunkVR_RifleElbowSeq: AtomicU32 = AtomicU32::new(0);
/// mode, pole weight, shoulder-to-hand (body right, forward, up), the elbow's radial (the same axes), the hand's model
/// rotation (x y z w), the weapon class.
#[no_mangle]
pub static mut CyberpunkVR_RifleElbowSample: [f32; 13] = [0.0; 13];

/// The final model-space shoulder, elbow and wrist of one hand packet (metres), and the mode it was solved in (-1: two
/// solves of the packet disagreed).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Pose {
    pub pose_id: u64,
    pub mode: i32,
    pub joints: [f32; 9],
}

static HISTORY: Mutex<[Pose; 64]> = Mutex::new([Pose { pose_id: 0, mode: 0, joints: [0.0; 9] }; 64]);
thread_local! {
    static PENDING: Cell<Pose> = const { Cell::new(Pose { pose_id: 0, mode: 0, joints: [0.0; 9] }) };
}

pub fn mode() -> i32 {
    CyberpunkVR_RifleElbowAB.load(Relaxed)
}
/// The right arm's solve: the sample, published, and the packet the pose belongs to (0: not a stable packet).
pub fn sample(mode: i32, pose_id: u64, values: [f32; 13]) {
    PENDING.with(|p| p.set(Pose { pose_id, mode, joints: [0.0; 9] }));
    CyberpunkVR_RifleElbowSeq.fetch_add(1, AcqRel);
    // SAFETY: the export's own words; readers follow the sequence.
    unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_RifleElbowSample, values) };
    CyberpunkVR_RifleElbowSeq.fetch_add(1, Release);
}
/// The completed pose (VRIK_CaptureModelCache): the pending packet's joints, when the solve that set it is this packet's
/// (`current_packet`) in the mode still asked for. Two solves of one packet that disagree mark it -1.
pub fn complete(current_packet: u32, joints: Option<[f32; 9]>) {
    let mode = mode();
    if mode == 0 {
        return;
    }
    let pending = PENDING.with(|p| p.replace(Pose::default()));
    if pending.pose_id == 0 {
        return;
    }
    let Some(joints) = joints else { return };
    if pending.pose_id as u32 != current_packet || pending.mode != mode {
        return;
    }
    let pose = Pose { joints, ..pending };
    let mut history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    let entry = &mut history[((pose.pose_id / 2) % 64) as usize];
    if entry.pose_id != pose.pose_id {
        *entry = pose;
    } else if entry.mode != pose.mode || entry.joints.iter().zip(&pose.joints).any(|(a, b)| (a - b).abs() >= 0.001) {
        entry.mode = -1;
    }
}
/// FindRifleElbowPose: the pose kept for this packet's id, or none (the fixture off, another packet, a disagreement).
pub fn find(pose_id: u64) -> Option<Pose> {
    if pose_id == 0 || mode() == 0 {
        return None;
    }
    let history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    let pose = history[((pose_id / 2) % 64) as usize];
    (pose.pose_id == pose_id && pose.mode > 0).then_some(pose)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_packet_kept_and_a_disagreement_marked() {
        CyberpunkVR_RifleElbowAB.store(2, Relaxed);
        let id = (1u64 << 63) | 42;
        sample(2, id, [0.0; 13]);
        complete(42, Some([1.0; 9]));
        assert_eq!(find(id).map(|p| p.joints), Some([1.0; 9]));
        // the same packet solved again, elsewhere: not one pose
        sample(2, id, [0.0; 13]);
        complete(42, Some([2.0; 9]));
        assert_eq!(find(id), None);
        // another packet's solve does not complete this one
        sample(2, id + 2, [0.0; 13]);
        complete(43, Some([1.0; 9]));
        assert_eq!(find(id + 2), None);
        CyberpunkVR_RifleElbowAB.store(0, Relaxed);
        assert_eq!(find(id), None);
        assert_eq!(CyberpunkVR_RifleElbowSeq.load(Relaxed) % 2, 0);
    }
}
