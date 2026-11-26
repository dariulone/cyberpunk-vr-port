//! The hands published to the VRIK solve (formerly the filters and the seqlock packet of
//! OpenXRManager::FlushHandsToShared): a hand coming back into tracking glides to where it is (cvr-body's Reacquire),
//! the one hand filter in UEVR's form on the pose the arms actually read (delta-time lerp with the distance term,
//! outside the head frame by default, off on the wheel, scaled under frame generation), and the packet written into the
//! shared block under its seqlock: the hands (the right one with the weapon offset), the view anchor, the sample's
//! stamp, the head's orientation and height, the neck pivot, the head's position.
//!
//! Choosing the reference head, locating the hands for the frame and keeping what was published stay with
//! OpenXRManager for now.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use cvr_body::cmath::{cos, sin};
use cvr_body::reacquire::{HandPose as ReacquirePose, Reacquire};
use cvr_body::tracking::exponential_follow;
use cvr_body::xr::{conjugate_quat, multiply_quat, nlerp_quat, rotate_vector, Quat, Vec3};
use std::sync::atomic::{fence, AtomicU32, Ordering};
use std::sync::Mutex;

use crate::hands::{HandPose, Pose};

#[allow(non_upper_case_globals)]
#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_HandLerpSpeed: f32;
    static CyberpunkVR_HandLerpPistol: f32;
    static CyberpunkVR_HandLerpRifle: f32;
    static CyberpunkVR_HandLerpShotgun: f32;
    static CyberpunkVR_HandLerpSniper: f32;
    static CyberpunkVR_HandLerpMelee: f32;
    static CyberpunkVR_HandLerpFramegenScale: f32;
    static CyberpunkVR_WheelHandLerpSpeed: f32;
    static CyberpunkVR_HandFilterInWorld: i32;
    static CyberpunkVR_WeaponClass: i32;
    static g_hasWeaponEquipped: bool;
    /// 0 = the arm IK drives the hand, 1 = the wheel's animation does (src/Anim/CharacterRigState.cpp).
    static g_wheelBlendLeft: AtomicU32;
    static g_wheelBlendRight: AtomicU32;
}

/// The tuning the filter reads, every call (all live).
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// EffectiveHandLerpSpeed: the weapon class's rate, else the base.
    pub speed: f32,
    pub framegen_scale: f32,
    pub wheel_speed: f32,
    pub filter_world: bool,
    /// The wheel's hold per hand (left, right).
    pub wheel_hold: [f32; 2],
}
#[cfg(not(test))]
fn tuning() -> Tuning {
    // SAFETY: the C++ half's live tuning globals, read as it reads them.
    unsafe {
        let r = |v: &f32| core::ptr::read_volatile(v);
        let base = r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpSpeed));
        let speed = if !core::ptr::read_volatile(core::ptr::addr_of!(g_hasWeaponEquipped)) {
            base
        } else {
            let v = match core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_WeaponClass)) {
                1 => r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpPistol)),
                2 => r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpRifle)),
                3 => r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpShotgun)),
                4 => r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpSniper)),
                5 => r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpMelee)),
                _ => 0.0,
            };
            if v > 0.001 {
                v
            } else {
                base
            }
        };
        Tuning {
            speed,
            framegen_scale: r(&*core::ptr::addr_of!(CyberpunkVR_HandLerpFramegenScale)),
            wheel_speed: r(&*core::ptr::addr_of!(CyberpunkVR_WheelHandLerpSpeed)),
            filter_world: core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_HandFilterInWorld)) != 0,
            wheel_hold: [f32::from_bits(g_wheelBlendLeft.load(Ordering::Relaxed)), f32::from_bits(g_wheelBlendRight.load(Ordering::Relaxed))],
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Lerp {
    pose: Pose,
    init: bool,
}
/// The filters' state across publications.
#[derive(Default)]
pub struct Filters {
    reacquire: [Reacquire; 2],
    reacquire_origin: u64,
    reacquire_last_us: u64,
    lerp: [Lerp; 2],
    lerp_origin: u64,
    lerp_last_us: u64,
    last_mode: [i32; 2],
}
impl Filters {
    pub fn new() -> Filters {
        Filters { last_mode: [-1, -1], ..Default::default() }
    }

    /// A hand coming back into tracking glides (head-local, as published); a new tracking origin starts over.
    pub fn reacquire(&mut self, hands: &mut [HandPose; 2], origin: u64, now_us: u64) {
        let dt = if self.reacquire_last_us != 0 && now_us > self.reacquire_last_us {
            ((now_us - self.reacquire_last_us) as f64 * 1e-6) as f32
        } else {
            0.0
        };
        self.reacquire_last_us = now_us;
        if self.reacquire_origin != origin {
            for r in &mut self.reacquire {
                r.reset();
            }
            self.reacquire_origin = origin;
        }
        for (h, src) in hands.iter_mut().enumerate() {
            let pose = ReacquirePose { p: src.pos, q: src.ori };
            let pose = self.reacquire[h].step(&pose, src.valid, dt);
            if !src.valid {
                continue;
            }
            src.pos = pose.p;
            src.ori = pose.q;
        }
    }

    /// The hand filter in UEVR's form, after the source is chosen: lerp_speed = rate * dt; position by
    /// min(1, lerp_speed * max(1, |error|)), rotation by lerp_speed * |dot| -- or, on the native frame path, the
    /// same rates as an exponential time response. Outside the head frame unless CyberpunkVR_HandFilterInWorld is 0.
    pub fn smooth(&mut self, hands: &mut [HandPose; 2], head: &Pose, origin: u64, frame_head: bool, framegen: bool, now_us: u64, t: &Tuning) {
        if self.lerp_origin != origin {
            for l in &mut self.lerp {
                l.init = false;
            }
            self.lerp_origin = origin;
        }
        let dt = if self.lerp_last_us != 0 && now_us > self.lerp_last_us {
            ((now_us - self.lerp_last_us) as f64 * 1e-6) as f32
        } else {
            0.0
        };
        self.lerp_last_us = now_us;
        let bypass = framegen && !(t.framegen_scale > 0.0);
        let speed = if bypass { 0.0 } else { t.speed * if framegen { t.framegen_scale } else { 1.0 } };
        for h in 0..2 {
            // Bypassed under framegen (scale 0) its history is dropped, so switching back seeds from the pose.
            if bypass || !hands[h].valid {
                self.lerp[h].init = false;
                continue;
            }
            // A hand on the wheel is not smoothed.
            if t.wheel_hold[h] > 0.01 && t.wheel_speed > 0.0 {
                self.lerp[h].init = false;
                continue;
            }
            let lerp_speed = speed * dt;
            // The state is a WORLD pose in one mode and a HEAD-LOCAL one in the other: a flip restarts it.
            let mode = if t.filter_world { 1 } else { 0 };
            if self.last_mode[h] != mode {
                self.last_mode[h] = mode;
                self.lerp[h].init = false;
            }
            let src = &mut hands[h];
            let local = Pose {
                orientation: Quat { x: src.ori[0], y: src.ori[1], z: src.ori[2], w: src.ori[3] },
                position: Vec3 { x: src.pos[0], y: src.pos[1], z: src.pos[2] },
            };
            let cur = if t.filter_world {
                let w = rotate_vector(head.orientation, local.position);
                Pose {
                    orientation: multiply_quat(head.orientation, local.orientation),
                    position: Vec3 { x: head.position.x + w.x, y: head.position.y + w.y, z: head.position.z + w.z },
                }
            } else {
                local
            };
            let st = &mut self.lerp[h];
            if !st.init || lerp_speed <= 0.0 || speed <= 0.0 {
                st.pose = cur;
                st.init = true;
            } else {
                let dx = cur.position.x - st.pose.position.x;
                let dy = cur.position.y - st.pose.position.y;
                let dz = cur.position.z - st.pose.position.z;
                let mut len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len < 1.0 {
                    len = 1.0; // UEVR: max(1, length)
                }
                let mut tp = if frame_head { exponential_follow(speed * len, dt) } else { lerp_speed * len };
                if tp > 1.0 {
                    tp = 1.0;
                }
                st.pose.position.x += dx * tp;
                st.pose.position.y += dy * tp;
                st.pose.position.z += dz * tp;
                let (a, b) = (st.pose.orientation, cur.orientation);
                let mut d = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
                if d < 0.0 {
                    d = -d;
                }
                if d > 1.0 {
                    d = 1.0;
                }
                let mut tr = if frame_head { exponential_follow(speed * d, dt) } else { lerp_speed * d };
                if tr > 1.0 {
                    tr = 1.0;
                }
                st.pose.orientation = nlerp_quat(st.pose.orientation, cur.orientation, tr);
            }
            let out = if t.filter_world {
                let inv = conjugate_quat(head.orientation);
                let d = Vec3 {
                    x: st.pose.position.x - head.position.x,
                    y: st.pose.position.y - head.position.y,
                    z: st.pose.position.z - head.position.z,
                };
                Pose { orientation: multiply_quat(inv, st.pose.orientation), position: rotate_vector(inv, d) }
            } else {
                st.pose
            };
            src.pos = [out.position.x, out.position.y, out.position.z];
            src.ori = [out.orientation.x, out.orientation.y, out.orientation.z, out.orientation.w];
        }
    }
}
static FILTERS: Mutex<Option<Filters>> = Mutex::new(None);

/// Both filters on the hands about to be published: the reacquire glide, then the hand filter.
#[cfg(not(test))]
pub fn filter_hands(hands: &mut [HandPose; 2], head: &Pose, origin: u64, frame_head: bool, framegen: bool, now_us: u64) {
    let mut guard = FILTERS.lock().unwrap_or_else(|p| p.into_inner());
    let f = guard.get_or_insert_with(Filters::new);
    f.reacquire(hands, origin, now_us);
    // Body inference saw the raw hands; this is what the arms read.
    f.smooth(hands, head, origin, frame_head, framegen, now_us, &tuning());
}

/// Both filters on the hands about to be published (`hands` are the head-local srcHands; `head` the anchor).
///
/// # Safety
/// `left`/`right` point at OpenXRHeadPose (whose head HandPose mirrors), `head` at an XrPosef.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_filter_published_hands(
    left: *mut HandPose,
    right: *mut HandPose,
    head: *const Pose,
    origin: u64,
    frame_head: bool,
    framegen: bool,
    now_us: u64,
) {
    // SAFETY: the caller's two hands and head.
    let (Some(l), Some(r), Some(head)) = (unsafe { left.as_mut() }, unsafe { right.as_mut() }, unsafe { head.as_ref() }) else { return };
    let mut hands = [HandPose { pos: l.pos, ori: l.ori, valid: l.valid }, HandPose { pos: r.pos, ori: r.ori, valid: r.valid }];
    filter_hands(&mut hands, head, origin, frame_head, framegen, now_us);
    l.pos = hands[0].pos;
    l.ori = hands[0].ori;
    r.pos = hands[1].pos;
    r.ori = hands[1].ori;
}

/// The packet's inputs besides the hands.
#[repr(C)]
pub struct Packet {
    /// The weapon offset: pitch, yaw, roll in degrees, and the translation in the hand's frame.
    pub weapon_euler: [f32; 3],
    pub weapon_offset: [f32; 3],
    /// [112..115]: 2 the native v2 anchor (consumed tracking XY, the bend angle), 1 the XYZ view anchor, 0 none.
    pub anchor_mode: i32,
    pub anchor: [f32; 3],
    /// [67] the sample's stamp.
    pub stamp: f32,
    /// The head relative to the recenter base: its orientation and position.
    pub head_ori: [f32; 4],
    pub head_pos: [f32; 3],
}

/// The weapon's Euler angles as a quaternion (the C++ EulerToQuat, its operation order).
pub fn euler_to_quat(pitch_deg: f32, yaw_deg: f32, roll_deg: f32) -> [f32; 4] {
    let p = pitch_deg * (3.1415926535f32 / 180.0) * 0.5;
    let y = yaw_deg * (3.1415926535f32 / 180.0) * 0.5;
    let r = roll_deg * (3.1415926535f32 / 180.0) * 0.5;
    let (cp, sp) = (cos(p), sin(p));
    let (cy, sy) = (cos(y), sin(y));
    let (cr, sr) = (cos(r), sin(r));
    [
        sr * cp * cy - cr * sp * sy,
        cr * sp * cy + sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
        cr * cp * cy + sr * sp * sy,
    ]
}
/// MulQuatLoc: a * b.
fn mul_quat_loc(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let ([ax, ay, az, aw], [bx, by, bz, bw]) = (a, b);
    [
        ax * bw + aw * bx + ay * bz - az * by,
        ay * bw + aw * by + az * bx - ax * bz,
        az * bw + aw * bz + ax * by - ay * bx,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The packet into the shared block (256 floats) under its seqlock: [127] goes odd, the payload, then even. `seq` is
/// OpenXRManager's counter, advanced by two so both markers are right by construction. Returns the even sequence and
/// fills `right_out` with the right hand as published (with the weapon offset).
///
/// # Safety
/// `shared` is the 1 KiB mapping; the hands point at OpenXRHeadPose; `packet`, `seq` and `right_out` are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_write_hand_packet(
    shared: *mut f32,
    left: *const HandPose,
    right: *const HandPose,
    packet: *const Packet,
    seq: *mut u32,
    right_out: *mut [f32; 7],
) -> u32 {
    // SAFETY (whole body): the caller's mapping and structures; slots written with volatile stores, as the C++ wrote
    // through its float pointer, the sequence slot as a volatile u32.
    unsafe {
        let (Some(l), Some(r), Some(p)) = (left.as_ref(), right.as_ref(), packet.as_ref()) else { return 0 };
        let slot = |i: usize, v: f32| core::ptr::write_volatile(shared.add(i), v);
        let seq_slot = shared.add(127).cast::<u32>();
        *seq += 2;
        let start = *seq | 1; // always odd, always advancing
        core::ptr::write_volatile(seq_slot, start);
        fence(Ordering::Release);

        // Left hand [0..7].
        slot(0, if l.valid { 1.0 } else { 0.0 });
        for k in 0..3 {
            slot(1 + k, l.pos[k]);
        }
        for k in 0..4 {
            slot(4 + k, l.ori[k]);
        }
        // Right hand [8..15], with the weapon offset.
        let [rx, ry, rz] = r.pos;
        let rq = r.ori;
        let [rqx, rqy, rqz, rqw] = rq;
        let off = euler_to_quat(p.weapon_euler[0], p.weapon_euler[1], p.weapon_euler[2]);
        let fq = mul_quat_loc(rq, off);
        let [dx, dy, dz] = p.weapon_offset;
        let tx = 2.0 * (rqy * dz - rqz * dy);
        let ty = 2.0 * (rqz * dx - rqx * dz);
        let tz = 2.0 * (rqx * dy - rqy * dx);
        let vx = dx + rqw * tx + (rqy * tz - rqz * ty);
        let vy = dy + rqw * ty + (rqz * tx - rqx * tz);
        let vz = dz + rqw * tz + (rqx * ty - rqy * tx);
        let right_pose = [rx + vx, ry + vy, rz + vz, fq[0], fq[1], fq[2], fq[3]];
        slot(8, if r.valid { 1.0 } else { 0.0 });
        for (k, v) in right_pose.iter().enumerate() {
            slot(9 + k, *v);
        }
        // [112..115] the view anchor from this sample's head.
        match p.anchor_mode {
            2 | 1 => {
                slot(112, p.anchor[0]);
                slot(113, p.anchor[1]);
                slot(114, p.anchor[2]);
                slot(115, p.anchor_mode as f32);
            }
            _ => slot(115, 0.0),
        }
        // [67] the stamp, inside the seqlock so it travels with its pose.
        slot(67, p.stamp);
        // [16..19] the head's orientation relative to the base.
        for k in 0..4 {
            slot(16 + k, p.head_ori[k]);
        }
        // [89] the head's height, [90] the neck pivot under the optical centre (0.15 m forward, 0.08 m up of it).
        let base_y = p.head_pos[1];
        slot(89, base_y);
        let ori = Quat { x: p.head_ori[0], y: p.head_ori[1], z: p.head_ori[2], w: p.head_ori[3] };
        let opt = rotate_vector(ori, Vec3 { x: 0.0, y: 0.08, z: -0.15 });
        slot(90, base_y - opt.y);
        // [124..126] the head's position relative to the base.
        slot(124, p.head_pos[0]);
        slot(125, base_y);
        slot(126, p.head_pos[2]);

        fence(Ordering::Release);
        let end = start + 1; // even = complete, never repeated
        core::ptr::write_volatile(seq_slot, end);
        if let Some(out) = right_out.as_mut() {
            *out = right_pose;
        }
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hand(x: f32) -> HandPose {
        HandPose { pos: [x, 0.0, -0.3], ori: [0.0, 0.0, 0.0, 1.0], valid: true }
    }
    fn tuning(speed: f32) -> Tuning {
        Tuning { speed, framegen_scale: 1.25, wheel_speed: 150.0, filter_world: true, wheel_hold: [0.0, 0.0] }
    }
    #[test]
    fn the_filter_seeds_then_follows_by_the_rate() {
        let mut f = Filters::new();
        let head = Pose { orientation: Quat::IDENTITY, position: Vec3 { x: 0.0, y: 1.7, z: 0.0 } };
        let mut hands = [hand(0.2), hand(-0.2)];
        f.smooth(&mut hands, &head, 1, false, false, 1_000_000, &tuning(15.0));
        assert_eq!(hands[0].pos[0], 0.2, "the first publication seeds");
        let mut hands = [hand(0.3), hand(-0.2)];
        f.smooth(&mut hands, &head, 1, false, false, 1_010_000, &tuning(15.0));
        // 15/s * 0.01 s * max(1, 0.1 m) = 0.15 of the way
        assert!((hands[0].pos[0] - 0.215).abs() < 1e-5, "{:?}", hands[0].pos);
        // a new origin starts over
        let mut hands = [hand(0.5), hand(-0.2)];
        f.smooth(&mut hands, &head, 2, false, false, 1_020_000, &tuning(15.0));
        assert!((hands[0].pos[0] - 0.5).abs() < 1e-6);
        // on the wheel: not smoothed
        let mut t = tuning(15.0);
        t.wheel_hold = [1.0, 0.0];
        let mut hands = [hand(0.9), hand(-0.2)];
        f.smooth(&mut hands, &head, 2, false, false, 1_030_000, &t);
        assert_eq!(hands[0].pos[0], 0.9);
    }
    #[test]
    fn the_packet_is_framed_by_an_odd_then_even_sequence() {
        let mut shared = [0.0f32; 256];
        let mut seq = 0u32;
        let l = hand(0.2);
        let r = HandPose { pos: [0.1, 0.0, 0.0], ori: [0.0, 0.0, 0.0, 1.0], valid: true };
        let p = Packet {
            weapon_euler: [0.0, 90.0, 0.0],
            weapon_offset: [0.0, 0.0, -0.1],
            anchor_mode: 0,
            anchor: [0.0; 3],
            stamp: 12.5,
            head_ori: [0.0, 0.0, 0.0, 1.0],
            head_pos: [0.0, 1.7, 0.0],
        };
        let mut right = [0.0f32; 7];
        let end = unsafe { cvr_xr_write_hand_packet(shared.as_mut_ptr(), &l, &r, &p, &mut seq, &mut right) };
        assert_eq!((seq, end), (2, 4));
        assert_eq!(shared[127].to_bits(), 4);
        assert_eq!((shared[0], shared[1], shared[8]), (1.0, 0.2, 1.0));
        assert!((right[2] + 0.1).abs() < 1e-6, "the offset in the hand's frame: {right:?}");
        assert!((shared[90] - (1.7 - 0.08)).abs() < 1e-6);
        assert_eq!((shared[67], shared[115], shared[125]), (12.5, 0.0, 1.7));
        let end = unsafe { cvr_xr_write_hand_packet(shared.as_mut_ptr(), &l, &r, &p, &mut seq, &mut right) };
        assert_eq!(end, 6, "every even value strictly new");
    }
}
