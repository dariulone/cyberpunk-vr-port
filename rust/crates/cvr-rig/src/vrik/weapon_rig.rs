//! The WEAPON's bones, as against the character's (formerly src/Anim/WeaponRig.cpp). The pose-apply detour sees both
//! skeletons through one function and tells them apart by whose track buffer it is; the three guards stay in the
//! detour and only the bodies live here: identify the rig by its bone names and write the reload module's parts and
//! float tracks onto it, the census of track buffers, and the weapon's own pass that captures its parts.
//!
//! NO VirtualQuery ON THIS PATH: the engine calls it ~10k times a second. The detour's SEH frame is the guard.
use super::*;
use cvr_body::cmath::{cos, sin};

const RIG_N: usize = 3;
const WRITES: usize = 32;
const SIG_MAX: usize = 128;
const SIG_NAMES: usize = 4;
const TRACKS: usize = 16;
const OFF_MAX: f32 = 0.80;
const CENSUS_MAX: usize = 96;

extern "C" {
    static mut g_RigBuf: [usize; RIG_N];
    static mut g_RigSeen: [u32; RIG_N];
    static mut g_RigBones: [u32; RIG_N];
    static mut g_RigTracks: [u32; RIG_N];
    static mut g_RigWriteN: i32;
    static mut g_RigWriteWhich: [i32; WRITES];
    static mut g_RigWriteBone: [i32; WRITES];
    static mut g_RigWriteOff: [[f32; 3]; WRITES];
    static mut g_RigWriteBase: [[f32; 3]; WRITES];
    static mut g_RigWriteHaveBase: [i32; WRITES];
    static mut g_RigWriteApplied: [u32; WRITES];
    static mut g_RigWriteSlot: [i32; WRITES];
    static mut g_RigWriteRotAxis: [[f32; 3]; WRITES];
    static mut g_RigWriteRotAngle: [f32; WRITES];
    static mut g_RigWriteBaseRot: [[f32; 4]; WRITES];
    static mut g_RigWriteHaveBaseRot: [i32; WRITES];
    static mut g_RigWriteScale: [f32; WRITES];
    static mut g_RigWriteAbs: [i32; WRITES];
    static mut g_RigWriteQuat: [[f32; 4]; WRITES];
    static mut g_RigWriteQuatOn: [i32; WRITES];
    static mut g_RigWriteEnabled: [i32; WRITES];
    static mut g_RigWritePin: [i32; WRITES];
    static mut g_RigPose: [[[f32; 7]; 20]; RIG_N];
    static mut g_RigPoseHave: [i32; RIG_N];
    static mut g_RigSigN: i32;
    static mut g_RigSigWhich: [i32; SIG_MAX];
    static mut g_RigSigBones: [u32; SIG_MAX];
    static mut g_RigSigIdx: [[i32; SIG_NAMES]; SIG_MAX];
    static mut g_RigSigHash: [[u64; SIG_NAMES]; SIG_MAX];
    static mut g_RigTrackVal: [[f32; TRACKS]; RIG_N];
    static mut g_RigTrackHave: [i32; RIG_N];
    static mut g_RigTrackSet: [[f32; TRACKS]; RIG_N];
    static mut g_RigTrackOn: [[i32; TRACKS]; RIG_N];
    static mut g_RigPassSeen: [u32; 4];
    static mut g_RigPassRn: [i32; 4];
    static mut g_RigPassDst: [i32; 4];
    static mut g_RigPassSlots: [i32; 4];
    static mut g_RigPassBufLo: [i32; 4];
    static mut g_RigPassMap0: [i32; 8];
    static mut g_PoseCensusN: i32;
    static mut g_PoseCensusBuf: [usize; CENSUS_MAX];
    static mut g_PoseCensusA4: [u32; CENSUS_MAX];
    static mut g_PoseCensusHits: [u32; CENSUS_MAX];
    static mut g_PoseCensusArmed: [u8; CENSUS_MAX];
    static mut g_PoseCensusHitsOut: [u32; CENSUS_MAX];
    static mut g_PoseCensusHitsIn: [u32; CENSUS_MAX];
    static mut g_PoseCensusWeaponOut: i32;
    static mut g_PoseCensusFull: i32;
    static mut g_WeaponPartOff: [[f32; 3]; 24];
    static mut g_WeaponPartWriteOn: i32;
    static mut g_WeaponPartCount: i32;
    static mut g_WeaponPartIdx: [i32; 24];
    static mut g_WeaponPartPos: [[f32; 3]; 24];
    static mut g_WeaponPartRot: [[f32; 4]; 24];
    static mut g_WeaponPartHave: i32;
    static mut g_WeaponMatchCalls: u64;
    /// The main image's range (src/Core/VrCore.cpp, C++ linkage), through src/Anim/CharacterRigRust.cpp.
    fn cvr_cpp_game_module_range(base: *mut usize, size: *mut usize);
}

/// THE HELD WEAPON'S OWN PASSES. A rig is identified by its bone names, so every other animated copy of a registered
/// gun (a holster's copy on the body, another instance of the gun) matched the same `which` as the gun in the hands:
/// its read-back overwrote the held gun's pose (VRRigBone: with the Malorian drawn, 4 of 5 snapshots were other copies'
/// poses), the writes landed on it too, and the first pass of a new write latched ITS bone as the base -- the
/// Malorian's slide, grabbed, stood shifted to the side with its parts turned; a turned cylinder took another copy's
/// rotation as its rest. The reload publishes the animated objects of the weapons in the player's hands; while any are
/// published only their passes are read and written (the pass's object is anim_pose_hook's scope), with none every
/// match is, as before.
const HELD_N: usize = 8;
static HELD: [core::sync::atomic::AtomicUsize; HELD_N] = [const { core::sync::atomic::AtomicUsize::new(0) }; HELD_N];
static HELD_ON: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Passes of a matched rig: [held object, another object (skipped), outside an object update].
pub static HELD_STATS: [core::sync::atomic::AtomicU32; 3] = [const { core::sync::atomic::AtomicU32::new(0) }; 3];

/// The animated objects of the weapons in the player's hands (none: every rig match is the held gun's, as before).
pub fn set_held_objects(objects: &[usize]) {
    use core::sync::atomic::Ordering::Relaxed;
    for (k, slot) in HELD.iter().enumerate() {
        slot.store(objects.get(k).copied().unwrap_or(0), Relaxed);
    }
    HELD_ON.store(objects.iter().any(|&o| o != 0), Relaxed);
}
/// This pass belongs to a published held object (or nothing is published). A pass outside an object update cannot be
/// told apart: it keeps the old behaviour and is counted.
fn held_pass() -> bool {
    use core::sync::atomic::Ordering::Relaxed;
    if !HELD_ON.load(Relaxed) {
        return true;
    }
    let (object, _) = super::anim_pose_hook::object_scope();
    if object == 0 {
        HELD_STATS[2].fetch_add(1, Relaxed);
        return true;
    }
    if HELD.iter().any(|h| h.load(Relaxed) == object) {
        HELD_STATS[0].fetch_add(1, Relaxed);
        true
    } else {
        HELD_STATS[1].fetch_add(1, Relaxed);
        false
    }
}

#[inline(always)]
unsafe fn rd<T: Copy>(p: *const u8, off: usize) -> T {
    (p.add(off) as *const T).read()
}

/// The game's shared empty track buffer: a rig with no float tracks is handed one static buffer in the main image's
/// read-only data while its rig object still reports a count at +0xA0. A buffer inside the image has no tracks.
unsafe fn rig_track_count(rig: *const u8, track_buf: usize) -> u32 {
    let (mut base, mut size) = (0usize, 0usize);
    cvr_cpp_game_module_range(&mut base, &mut size);
    if base != 0 && track_buf >= base && track_buf < base.wrapping_add(size) {
        return 0;
    }
    rd::<u32>(rig, 0xA0)
}

/// Track indices belong to the source rig; the pose buffer has its own namespace and capacity
/// (formerly include/Anim/RigTrackMapping.hpp).
#[allow(clippy::too_many_arguments)]
unsafe fn apply_rig_tracks(pairs: *const u32, count: u32, source_count: u32, destination: *mut f32, capacity: u32,
                           observed: &mut [f32; TRACKS], requested: &[f32; TRACKS], enabled: &[i32; TRACKS]) -> bool {
    let limit = TRACKS as u32;
    for o in observed.iter_mut() {
        *o = -1.0;
    }
    if pairs.is_null() || destination.is_null() || count == 0 || capacity == 0 || count > source_count {
        return false;
    }
    let mut any = false;
    for i in 0..count as usize {
        let src = *pairs.add(i * 2);
        let dst = *pairs.add(i * 2 + 1);
        if src >= source_count || src >= limit || dst >= capacity {
            continue;
        }
        let before = *destination.add(dst as usize);
        if !before.is_finite() {
            continue;
        }
        observed[src as usize] = before;
        any = true;
        let value = requested[src as usize];
        if enabled[src as usize] != 0 && value.is_finite() {
            *destination.add(dst as usize) = value;
        }
    }
    any
}

/// The remap's destination for a rig bone among its first `count` pairs, or -1.
#[inline(always)]
unsafe fn remap_dst(remap: *const u32, count: u32, bone: i32) -> i32 {
    for e in 0..count as usize {
        if *remap.add(e * 2) == bone as u32 {
            return *remap.add(e * 2 + 1) as i32;
        }
    }
    -1
}

/// IDENTIFY THE RIG BY BONE NAME (a1[8] is the rig object, its names at +0x50, count at +0x58), then read the game's
/// own pose back, apply the float tracks and write the registered parts.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_weapon_rig_identify_and_write(a1: *mut *mut u8, a2: *mut *mut u8, a4: u32,
                                                                pose_desc: *mut u8, bone_buf: *mut u8,
                                                                track_buf: usize) {
    // THE RIG TABLES' LOCK round the pass (main's __try / __finally): a stale engine pointer faults inside the guarded
    // call, which ends the pass, and the lock is released all the same.
    struct Args {
        a1: *mut *mut u8,
        a2: *mut *mut u8,
        a4: u32,
        pose_desc: *mut u8,
        bone_buf: *mut u8,
        track_buf: usize,
    }
    unsafe extern "C" fn locked(args: *mut core::ffi::c_void, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void) -> usize {
        // SAFETY: the caller's arguments, for this call.
        let a = unsafe { &*(args as *const Args) };
        // SAFETY: the pose pass's own buffers, as the hook vouches.
        unsafe { identify_and_write_locked(a.a1, a.a2, a.a4, a.pose_desc, a.bone_buf, a.track_buf) };
        0
    }
    let mut args = Args { a1, a2, a4, pose_desc, bone_buf, track_buf };
    super::pose_rig::lock_rig_pose_state();
    let _ = cvr_engine::code::guarded_call4(
        locked as *const () as usize,
        (&raw mut args).cast(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    );
    super::pose_rig::unlock_rig_pose_state();
}
/// The pass itself, under the rig tables' lock.
unsafe fn identify_and_write_locked(a1: *mut *mut u8, a2: *mut *mut u8, a4: u32, pose_desc: *mut u8, bone_buf: *mut u8,
                                    track_buf: usize) {
    let mut which: i32 = -1;
    let mut nb: u32 = 0;

    let rig = *a1.add(8);
    if !rig.is_null() {
        let n = rd::<u32>(rig, 0x58);
        let names = rd::<*const u64>(rig, 0x50);
        if !names.is_null() && n > 0 {
            let sn = g_RigSigN;
            let mut s = 0i32;
            while s < sn && (s as usize) < SIG_MAX && which < 0 {
                let su = s as usize;
                s += 1;
                if g_RigSigBones[su] != n {
                    continue;
                }
                let mut ok = true;
                for k in 0..SIG_NAMES {
                    let bi = g_RigSigIdx[su][k];
                    if bi < 0 {
                        continue;
                    }
                    if bi as u32 >= n || *names.add(bi as usize) != g_RigSigHash[su][k] {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    which = g_RigSigWhich[su];
                }
            }
            if which >= 0 && !held_pass() {
                which = -1;
            }
            if which >= 0 {
                nb = n;
                let w = which as usize;
                g_RigBuf[w] = track_buf;
                g_RigBones[w] = n;
                g_RigTracks[w] = rig_track_count(rig, track_buf);
            }
        }
    }

    if which < 0 || nb == 0 {
        return;
    }
    let w = which as usize;
    g_RigSeen[w] = g_RigSeen[w].wrapping_add(1);
    // Each bone's slot through the live remap: a2[6]+0x40 the (src, dst) pairs, +0x50 the u16 counts by LOD (a4),
    // poseDesc+0x08 this buffer's slot count.
    let six = *a2.add(6);
    let remap: *const u32 = if six.is_null() { core::ptr::null() } else { rd::<*const u32>(six, 0x40) };
    let rcnt: *const u16 = if six.is_null() { core::ptr::null() } else { rd::<*const u16>(six, 0x50) };
    let rn: u32 = if rcnt.is_null() { 0 } else { *rcnt.add(a4 as usize) as u32 };
    let slots = rd::<u32>(pose_desc, 0x08);
    // Per-pass telemetry for the frame rig.
    if which == 1 {
        let lod = if a4 < 4 { a4 as usize } else { 3 };
        g_RigPassSeen[lod] = g_RigPassSeen[lod].wrapping_add(1);
        g_RigPassRn[lod] = rn as i32;
        g_RigPassSlots[lod] = slots as i32;
        g_RigPassBufLo[lod] = ((bone_buf as usize) & 0x7FFF_FFFF) as i32;
        if lod == 0 && !remap.is_null() {
            let m = if rn < 4 { rn } else { 4 } as usize;
            for e in 0..8 {
                g_RigPassMap0[e] = -1;
            }
            for e in 0..m {
                g_RigPassMap0[e * 2] = *remap.add(e * 2) as i32;
                g_RigPassMap0[e * 2 + 1] = *remap.add(e * 2 + 1) as i32;
            }
        }
        if g_RigWriteN > 0 {
            let b0 = g_RigWriteBone[0];
            let mut d0 = -1;
            if !remap.is_null() && rn != 0 && b0 >= 0 {
                d0 = remap_dst(remap, rn, b0);
            }
            g_RigPassDst[lod] = d0;
        }
    }
    // READ-BACK first, before any write of ours: the GAME's own parent-local transform per rig bone.
    if !remap.is_null() && rn != 0 {
        for e in 0..rn as usize {
            let src = *remap.add(e * 2);
            let d2 = *remap.add(e * 2 + 1);
            if src >= 20 || d2 >= slots {
                continue;
            }
            let t = bone_buf.add(d2 as usize * 48 + VRIK_TRANS_OFF) as *const f32;
            let q = bone_buf.add(d2 as usize * 48 + VRIK_ROT_OFF) as *const f32;
            let o = &mut g_RigPose[w][src as usize];
            o[0] = *t;
            o[1] = *t.add(1);
            o[2] = *t.add(2);
            o[3] = *q;
            o[4] = *q.add(1);
            o[5] = *q.add(2);
            o[6] = *q.add(3);
        }
        g_RigPoseHave[w] = 1;
    }
    // 2.50 RVA 0x1B3D13..0x1B3D39: float tracks use their OWN (source, destination) remap at six+0x60, LOD counts
    // +0x70. The rig's source count is NOT the destination capacity.
    let track_map: *const u32 = if six.is_null() { core::ptr::null() } else { rd::<*const u32>(six, 0x60) };
    let track_counts: *const u16 = if six.is_null() { core::ptr::null() } else { rd::<*const u16>(six, 0x70) };
    let track_pairs: u32 = if track_counts.is_null() { 0 } else { *track_counts.add(a4 as usize) as u32 };
    let track_capacity = rd::<u32>(pose_desc, 0x20);
    g_RigTrackHave[w] = apply_rig_tracks(track_map, track_pairs, g_RigTracks[w], track_buf as *mut f32,
        track_capacity, &mut g_RigTrackVal[w], &g_RigTrackSet[w], &g_RigTrackOn[w]) as i32;
    let wn = g_RigWriteN;
    let mut k = 0usize;
    while (k as i32) < wn && k < WRITES {
        let kk = k;
        k += 1;
        if g_RigWriteWhich[kk] != which {
            continue;
        }
        let bi = g_RigWriteBone[kk];
        if bi < 0 {
            continue;
        }
        if g_RigWriteEnabled[kk] == 0 {
            continue; // released -- the animation's pose stands untouched
        }
        let mut dst = -1;
        if !remap.is_null() && rn != 0 {
            dst = remap_dst(remap, rn, bi);
        }
        g_RigWriteSlot[kk] = dst;
        // A later LOD pass can omit a bone: resolve through THIS component's LOD-0 map.
        if dst < 0 && !remap.is_null() && !rcnt.is_null() && g_RigWriteAbs[kk] != 0 {
            let full = *rcnt as u32;
            if full <= nb {
                dst = remap_dst(remap, full, bi);
            }
        }
        g_RigWriteSlot[kk] = dst;
        if a4 == 0 && dst >= 0 {
            g_RigWritePin[kk] = dst; // diagnostics only
        }
        if dst < 0 || dst as u32 >= slots {
            continue;
        }
        let slot = bone_buf.add(dst as usize * 48);
        let t = slot.add(VRIK_TRANS_OFF) as *mut f32;
        let translation = super::pose_rig::RIG_WRITE_TRANSLATION[kk];
        if translation && g_RigWriteHaveBase[kk] == 0 && g_RigWriteAbs[kk] == 0 {
            // BASE SANITY: a part's parent-local translation is centimetres; beyond 0.6 m the slot held a world-stage
            // value -- wait for a pass where it is freshly local.
            let bm2 = *t * *t + *t.add(1) * *t.add(1) + *t.add(2) * *t.add(2);
            if !bm2.is_finite() || bm2 > 0.36 {
                continue;
            }
            g_RigWriteBase[kk] = [*t, *t.add(1), *t.add(2)];
            g_RigWriteHaveBase[kk] = 1;
        }
        let lim = |v: f32| {
            let mut v = v;
            if v > OFF_MAX {
                v = OFF_MAX;
            }
            if v < -OFF_MAX {
                v = -OFF_MAX;
            }
            v
        };
        let ox = lim(g_RigWriteOff[kk][0]);
        let oy = lim(g_RigWriteOff[kk][1]);
        let oz = lim(g_RigWriteOff[kk][2]);
        // A ROTATION OR SCALE WRITE LEAVES THE ANIMATED TRANSLATION STANDING: only a slot that owns the translation
        // writes it.
        if !translation {
        } else if g_RigWriteAbs[kk] != 0 {
            // replay: the offset fields carry the game's own local values
            *t = g_RigWriteOff[kk][0];
            *t.add(1) = g_RigWriteOff[kk][1];
            *t.add(2) = g_RigWriteOff[kk][2];
        } else {
            *t = g_RigWriteBase[kk][0] + ox;
            *t.add(1) = g_RigWriteBase[kk][1] + oy;
            *t.add(2) = g_RigWriteBase[kk][2] + oz;
        }
        g_RigWriteApplied[kk] = g_RigWriteApplied[kk].wrapping_add(1);

        let q = slot.add(16) as *mut f32;
        // absolute ROTATION
        if g_RigWriteQuatOn[kk] != 0 {
            for c in 0..4 {
                *q.add(c) = g_RigWriteQuat[kk][c];
            }
        }
        // ROTATION offset for parts that SPIN, composed onto the rest rotation in its LOCAL frame.
        if g_RigWriteRotAngle[kk] != 0.0 {
            if g_RigWriteHaveBaseRot[kk] == 0 {
                g_RigWriteBaseRot[kk] = [*q, *q.add(1), *q.add(2), *q.add(3)];
                g_RigWriteHaveBaseRot[kk] = 1;
            }
            let hh = g_RigWriteRotAngle[kk] * 0.008_726_646_26; // deg -> rad, halved
            let s = sin(hh);
            let c = cos(hh);
            let qx = g_RigWriteRotAxis[kk][0] * s;
            let qy = g_RigWriteRotAxis[kk][1] * s;
            let qz = g_RigWriteRotAxis[kk][2] * s;
            let qw = c;
            let [bx, by, bz, bw] = g_RigWriteBaseRot[kk];
            *q = bw * qx + bx * qw + by * qz - bz * qy; // new = base * offset (local spin)
            *q.add(1) = bw * qy - bx * qz + by * qw + bz * qx;
            *q.add(2) = bw * qz + bx * qy - by * qx + bz * qw;
            *q.add(3) = bw * qw - bx * qx - by * qy - bz * qz;
        }
        // SCALE write, the no-tracks visibility switch.
        if g_RigWriteScale[kk] > 0.0 {
            let sv = slot.add(32) as *mut f32;
            *sv = g_RigWriteScale[kk];
            *sv.add(1) = g_RigWriteScale[kk];
            *sv.add(2) = g_RigWriteScale[kk];
        }
    }
}

/// THE CENSUS -- the abandoned statistical route to rig identification, left gated off: (track buffer, a4) pairs
/// with their hits, armed and holstered.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_weapon_rig_census_note(a4: u32, track_buf: usize) {
    let mut slot: i32 = -1;
    let n = g_PoseCensusN;
    for k in 0..n.max(0) as usize {
        if g_PoseCensusBuf[k] == track_buf && g_PoseCensusA4[k] == a4 {
            slot = k as i32;
            break;
        }
    }
    if slot < 0 {
        if (n as usize) < CENSUS_MAX && n >= 0 {
            slot = n;
            let s = slot as usize;
            g_PoseCensusBuf[s] = track_buf;
            g_PoseCensusA4[s] = a4;
            g_PoseCensusHits[s] = 0;
            g_PoseCensusHitsOut[s] = 0;
            g_PoseCensusHitsIn[s] = 0;
            g_PoseCensusArmed[s] = if g_PoseCensusWeaponOut != 0 { 1 } else { 0 };
            g_PoseCensusN = n + 1;
        } else {
            g_PoseCensusFull = 1;
        }
    }
    if slot >= 0 {
        let s = slot as usize;
        g_PoseCensusHits[s] = g_PoseCensusHits[s].wrapping_add(1);
        if g_PoseCensusWeaponOut != 0 {
            g_PoseCensusHitsOut[s] = g_PoseCensusHitsOut[s].wrapping_add(1);
        } else {
            g_PoseCensusHitsIn[s] = g_PoseCensusHitsIn[s].wrapping_add(1);
        }
    }
}

/// THE WEAPON'S OWN PASS: capture every part's local transform, and -- THE PROOF mechanism -- optionally offset it.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_weapon_rig_capture_parts(bone_buf: *mut u8) {
    g_WeaponMatchCalls = g_WeaponMatchCalls.wrapping_add(1);
    let n = g_WeaponPartCount;
    let mut k = 0usize;
    while (k as i32) < n && k < 24 {
        let bi = g_WeaponPartIdx[k];
        if (0..MAXB).contains(&bi) {
            let t = bone_buf.add(bi as usize * 48 + VRIK_TRANS_OFF) as *const f32;
            let q = bone_buf.add(bi as usize * 48 + VRIK_ROT_OFF) as *const f32;
            g_WeaponPartPos[k] = [*t, *t.add(1), *t.add(2)];
            g_WeaponPartRot[k] = [*q, *q.add(1), *q.add(2), *q.add(3)];
        }
        k += 1;
    }
    g_WeaponPartHave = 1;

    if g_WeaponPartWriteOn != 0 {
        let mut k = 0usize;
        while (k as i32) < n && k < 24 {
            let bi = g_WeaponPartIdx[k];
            let [ox, oy, oz] = g_WeaponPartOff[k];
            k += 1;
            if !(0..MAXB).contains(&bi) {
                continue;
            }
            if ox == 0.0 && oy == 0.0 && oz == 0.0 {
                continue;
            }
            let t = bone_buf.add(bi as usize * 48 + VRIK_TRANS_OFF) as *mut f32;
            *t += ox;
            *t.add(1) += oy;
            *t.add(2) += oz;
        }
    }
}

const VRIK_TRANS_OFF: usize = 0;
const VRIK_ROT_OFF: usize = 16;
