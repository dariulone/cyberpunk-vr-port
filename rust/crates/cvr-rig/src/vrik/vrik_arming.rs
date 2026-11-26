//! Arming the player's solve and the mirror double (formerly src/Natives/VrikArming.cpp).
//!
//! ARMING (`VRIK_DoArmPlayer`): the player's root animated object, its meta-rig's bone names resolved to the indices
//! the solve uses (head, hands, palms, arm chains, spine, legs, eyes, the FPP camera chain, the smoking grip's finger
//! bones and slots), the parent table copied for the FK, the rig policy configured, the baked smoking grips loaded
//! once, and only then -- when the binding read again is the one the work started from -- the track buffers that
//! let the pose hook recognise the player's pass.
//!
//! THE MIRROR DOUBLE: a mirror (V's bathroom, game 2.5) shows an NPC copy of V that the scene animates. While one is
//! bound (VRMirrorDouble, from CET) every player pass is cached and every pass of the double's root rig takes the
//! cached local transforms of the bones both rigs name; VRIK is not suspended for the scene meanwhile. The pose copied
//! is the last pass of the player's batch (published when the next batch starts), so its legs do not shake between
//! passes.
use super::cyberware::VRIK_ConfigureCyberwareTracks;
use super::env::*;
use super::rig::VRIK_ConfigureRigPolicy;
use super::snapshot::VRIK_IsReadable;
use super::MAX_BONES;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, RttiSystem};
use red4ext_abi::stack::{CStack, StackFrame, StackType};
use std::io::Write;
use std::sync::{Mutex, RwLock};

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_PlayerTrackBufA: usize;
    static mut g_PlayerTrackBufB: usize;
    static mut g_VRPalmRIdx: i32;
    static mut g_VRPalmLIdx: i32;
    static mut g_VREyeLeftIdx: i32;
    static mut g_VREyeRightIdx: i32;
    static mut g_VRFppCamIdx: [i32; 5];
    static mut g_VRSmokeFingerIdx: [i32; 32];
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeFingerName: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerNameL: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerRot: [[f32; 4]; 32];
    static mut g_VRSmokeFingerRotL: [[f32; 4]; 32];
    static mut g_VRSmokeFingerRotLC: [[f32; 4]; 32];
    static mut g_VRSmokeThumbIsL: [i32; 32];
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerHave: i32;
    static mut g_VRSmokeFingerHaveL: i32;
    static mut g_VRSmokeCigIdx: i32;
    static mut g_VRSmokeMouthBoneIdx: i32;
    static mut g_VRSmokeLighterIdx: i32;
    static mut g_VRSmokeCigHave: i32;
    static mut g_VRSmokeLighterHave: i32;
    static mut g_VRSmokeCigLHave: i32;
    static mut g_VRSmokeCigPos: [f32; 3];
    static mut g_VRSmokeCigRot: [f32; 4];
    static mut g_VRSmokeLighterPos: [f32; 3];
    static mut g_VRSmokeLighterRot: [f32; 4];
    static mut g_VRSmokeCigLPos: [f32; 3];
    static mut g_VRSmokeCigLRot: [f32; 4];
    static mut g_VRSmokeThumbFlickL: [f32; 4];
    static CyberpunkVR_VrikBatchGapMs: f32;
    // The C++ half: the player's objects (src/Natives/AnimLookup.cpp), the pose state's lock and heading flag
    // (Anim/VrikState.hpp's inline variables, src/Anim/CharacterRigRust.cpp), the live controls.
    fn cvr_anim_player_animated_object(component: *const c_char) -> *mut c_void;
    fn cvr_anim_object_for_entity(entity: *mut c_void, component: *const c_char, any: bool) -> *mut c_void;
    fn cvr_anim_player_entity() -> *mut c_void;
    fn cvr_anim_qword_is_heap(value: u64) -> bool;
    fn cvr_cpp_pose_lock();
    fn cvr_cpp_pose_unlock();
    fn cvr_cpp_set_pose_heading_ready(ready: bool);
    fn cvr_cpp_cutscene_suspend_tier() -> i32;
}

// ---- the engine's rig, read under SEH -------------------------------------------------------------------------

fn read<T: Copy>(at: usize) -> Option<T> {
    cvr_engine::code::read::<T>(at)
}
/// A DynArray's (entries, size) at `at`.
fn array(at: usize) -> Option<(usize, u32)> {
    Some((read::<usize>(at)?, read::<u32>(at + 0xC)?))
}
/// anim::AnimatedObject: metaRigID (+0x00), metaRig (+0x08). anim::MetaRig: boneTransforms (+0x00), parentIndeces
/// (+0x10), boneNames (+0x20), referenceTracks (+0x40), trackNames (+0x50).
mod meta {
    pub const BONE_TRANSFORMS: usize = 0x00;
    pub const PARENT_INDECES: usize = 0x10;
    pub const BONE_NAMES: usize = 0x20;
    pub const REFERENCE_TRACKS: usize = 0x40;
    pub const TRACK_NAMES: usize = 0x50;
}

/// What identifies the player's rig binding: the objects and arrays a rebind replaces.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct PlayerBinding {
    object: usize,
    rig: usize,
    names: usize,
    parents: usize,
    reference: usize,
    track_a: usize,
    track_b: usize,
    id: u64,
    bones: u32,
    parent_count: u32,
    reference_count: u32,
}
fn read_binding(object: usize) -> Option<PlayerBinding> {
    if object == 0 {
        return None;
    }
    let rig = read::<usize>(object + 0x08)?;
    if rig == 0 {
        return None;
    }
    let (names, bones) = array(rig + meta::BONE_NAMES)?;
    let (parents, parent_count) = array(rig + meta::PARENT_INDECES)?;
    let (reference, reference_count) = array(rig + meta::BONE_TRANSFORMS)?;
    if bones == 0 || parent_count == 0 || reference_count == 0 {
        return None;
    }
    let (tracks, track_count) = array(rig + meta::REFERENCE_TRACKS)?;
    let owner_b = read::<usize>(object + 0x18)?;
    let key = PlayerBinding {
        object,
        rig,
        names,
        parents,
        reference,
        track_a: if track_count != 0 { tracks } else { 0 },
        track_b: if owner_b != 0 { read::<usize>(owner_b + 0x18)? } else { 0 },
        id: read::<u64>(object)?,
        bones,
        parent_count,
        reference_count,
    };
    let ok = key.bones > 0
        && key.bones as usize <= MAX_BONES
        && key.parent_count == key.bones
        && key.reference_count >= key.bones
        && key.names != 0
        && key.parents != 0
        && key.reference != 0
        && (key.track_a != 0 || key.track_b != 0);
    ok.then_some(key)
}
static PLAYER_BINDING: Mutex<PlayerBinding> = Mutex::new(PlayerBinding {
    object: 0,
    rig: 0,
    names: 0,
    parents: 0,
    reference: 0,
    track_a: 0,
    track_b: 0,
    id: 0,
    bones: 0,
    parent_count: 0,
    reference_count: 0,
});
fn player_object() -> usize {
    // SAFETY: the C++ half's lookup of the player's root animated object.
    unsafe { cvr_anim_player_animated_object(c"root".as_ptr()) as usize }
}
/// Whether the player's rig binding is not the one the last arm published (or cannot be read).
#[no_mangle]
pub extern "C" fn VRIK_PlayerBindingChanged() -> bool {
    match read_binding(player_object()) {
        Some(current) => current != *PLAYER_BINDING.lock().unwrap_or_else(|p| p.into_inner()),
        None => true,
    }
}

// ---- the names ------------------------------------------------------------------------------------------------

fn eq(a: &[u8], b: &str) -> bool {
    a.eq_ignore_ascii_case(b.as_bytes())
}
/// Natives/Natives.cpp ContainsInsensitive.
fn contains(haystack: &[u8], needle: &str) -> bool {
    let h = haystack.to_ascii_lowercase();
    let n = needle.as_bytes().to_ascii_lowercase();
    n.is_empty() || h.windows(n.len()).any(|w| w == n.as_slice())
}
/// A name into a 48-byte slot as strncpy(dst, src, 47) leaves it (the slot zeroed past it here, as the arrays are).
fn copy_name(dst: &mut [c_char; 48], src: &[u8]) {
    let n = src.len().min(47);
    for (i, d) in dst.iter_mut().enumerate().take(47) {
        *d = if i < n { src[i] as c_char } else { 0 };
    }
}
fn slot_name(name: &[c_char; 48]) -> &[u8] {
    let n = name.iter().position(|&c| c == 0).unwrap_or(48);
    // SAFETY: c_char and u8 share a layout.
    unsafe { core::slice::from_raw_parts(name.as_ptr() as *const u8, n) }
}

/// The parsed "F"/"C"/"K" lines of a grip file: tag, bone name, then the floats.
fn grip_lines(name: &str) -> Vec<(String, String, Vec<f32>)> {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Grips, name);
    let Ok(text) = std::fs::read(&path) else { return Vec::new() };
    let text = String::from_utf8_lossy(&text);
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(tag), Some(bone)) = (words.next(), words.next()) else { continue };
        // istream >> float, value by value; a value that does not parse ends the line's numbers.
        let mut values = Vec::new();
        for w in words {
            match w.parse::<f32>() {
                Ok(v) => values.push(v),
                Err(_) => break,
            }
        }
        lines.push((tag.to_string(), bone.to_string(), values));
    }
    lines
}

/// The baked smoking grips, loaded once per session (name-keyed, so they survive index shifts and skeletons).
unsafe fn load_smoke_grips() {
    let (mut r_fing, mut l_fing) = (0, 0);
    let (mut cig_loaded, mut ltr_loaded) = (false, false);
    for file in ["CyberpunkVR_SmokeGrip_right.ini", "CyberpunkVR_LighterGrip_Left.ini"] {
        for (tag, bone, v) in grip_lines(file) {
            let b = bone.as_bytes();
            match tag.as_str() {
                "F" if v.len() >= 4 => {
                    let q = [v[0], v[1], v[2], v[3]];
                    let right = (0..g_VRSmokeFingerCount.clamp(0, 32) as usize).find(|&k| eq(slot_name(&g_VRSmokeFingerName[k]), &bone));
                    if let Some(k) = right {
                        g_VRSmokeFingerRot[k] = q;
                        r_fing += 1;
                    } else if let Some(k) = (0..g_VRSmokeFingerCountL.clamp(0, 32) as usize).find(|&k| eq(slot_name(&g_VRSmokeFingerNameL[k]), &bone)) {
                        g_VRSmokeFingerRotL[k] = q;
                        l_fing += 1;
                    }
                }
                "C" if v.len() >= 7 => {
                    if eq(b, "WeaponLeft") {
                        g_VRSmokeLighterPos = [v[0], v[1], v[2]];
                        g_VRSmokeLighterRot = [v[3], v[4], v[5], v[6]];
                        ltr_loaded = true;
                    } else {
                        g_VRSmokeCigPos = [v[0], v[1], v[2]];
                        g_VRSmokeCigRot = [v[3], v[4], v[5], v[6]];
                        cig_loaded = true;
                    }
                }
                "K" if v.len() >= 4 => g_VRSmokeThumbFlickL = [v[0], v[1], v[2], v[3]],
                _ => {}
            }
        }
    }
    if r_fing > 0 {
        g_VRSmokeFingerHave = 1;
    }
    if l_fing > 0 {
        g_VRSmokeFingerHaveL = 1;
    }
    if cig_loaded {
        g_VRSmokeCigHave = 1;
    }
    if ltr_loaded {
        g_VRSmokeLighterHave = 1;
    }
    // The left hand's cigarette grip: seeded from the lighter's, so a bone the file omits (or a missing file) falls
    // back to the lighter hold.
    g_VRSmokeFingerRotLC = g_VRSmokeFingerRotL;
    g_VRSmokeCigLPos = g_VRSmokeLighterPos;
    g_VRSmokeCigLRot = g_VRSmokeLighterRot;
    if ltr_loaded {
        g_VRSmokeCigLHave = 1;
    }
    let left_cig = cvr_host::paths::file(cvr_host::paths::Folder::Grips, "CyberpunkVR_SmokeGrip_Left.ini");
    if left_cig.exists() {
        let (mut lc_fing, mut lc_slot) = (0, false);
        for (tag, bone, v) in grip_lines("CyberpunkVR_SmokeGrip_Left.ini") {
            match tag.as_str() {
                "F" if v.len() >= 4 => {
                    if let Some(k) = (0..g_VRSmokeFingerCountL.clamp(0, 32) as usize).find(|&k| eq(slot_name(&g_VRSmokeFingerNameL[k]), &bone)) {
                        g_VRSmokeFingerRotLC[k] = [v[0], v[1], v[2], v[3]];
                        lc_fing += 1;
                    }
                }
                "C" if v.len() >= 7 => {
                    g_VRSmokeCigLPos = [v[0], v[1], v[2]];
                    g_VRSmokeCigLRot = [v[3], v[4], v[5], v[6]];
                    lc_slot = true;
                }
                _ => {}
            }
        }
        if lc_fing > 0 || lc_slot {
            g_VRSmokeCigLHave = 1;
        }
    }
    let info = format!(
        "resolvedR={} resolvedL={} loadedR={} loadedL={} cig={} lighter={}\n",
        g_VRSmokeFingerCount, g_VRSmokeFingerCountL, r_fing, l_fing, cig_loaded as i32, ltr_loaded as i32
    );
    if cvr_host::log::verbose() {
        let _ = std::fs::write(cvr_host::paths::file(cvr_host::paths::Folder::Log, "CyberpunkVR_SmokeGrip_loadinfo.txt"), super::hand_body::text_mode(&info));
    }
}

/// `bool f(this)` result of a script function with no arguments returning a CName, as RED4ext::ExecuteFunction calls
/// it (the engine's Execute, here under SEH).
unsafe fn call_cname(instance: *mut c_void, function: &red4ext_abi::function::BaseFunction) -> Option<CName> {
    let rtti = RttiSystem::get()?;
    let mut value = CName::NONE;
    let ret = function.return_type.as_ref()?;
    let mut result = StackType {
        ty: ret.ty,
        value: &mut value as *mut CName as *mut c_void,
    };
    let mut stack = CStack::new(rtti, instance, &mut [], &mut result)?;
    let address = function.execute_address()?;
    let ok = cvr_engine::code::guarded_call3(
        address,
        function as *const _ as *mut c_void,
        &mut stack as *mut CStack as *mut c_void,
        core::ptr::null_mut(),
    )?;
    ok.then_some(value)
}
/// The player's resolved body gender is Female.
unsafe fn player_female() -> bool {
    let player = cvr_anim_player_entity();
    if player.is_null() {
        return false;
    }
    let Some(class) = object_class(player) else { return false };
    let Some(f) = class.function(CName::hashed("GetResolvedGenderName")) else {
        return false;
    };
    call_cname(player, f) == Some(CName::hashed("Female"))
}

static SMOKE_GRIP_LOADED: AtomicI32 = AtomicI32::new(0);
static BONE_DIAG_WRITTEN: AtomicI32 = AtomicI32::new(0);

/// The resolve itself, under the pose state's lock; the track-buffer bitmask (1 A, 2 B) or -1.
unsafe fn arm_player_locked() -> i32 {
    g_PlayerTrackBufA = 0;
    g_PlayerTrackBufB = 0;
    g_VRBoneCount = 0;
    g_VRFKCount = 0;
    g_solveCacheN = 0;
    g_solveCacheModel = false;
    g_solveCacheTick = 0xFFFF_FFFF;
    let anim = player_object();
    if anim == 0 || !VRIK_IsReadable(anim as *const c_void, 0x40) {
        return -1;
    }
    let Some(binding) = read_binding(anim) else { return -1 };
    let mut track_a = 0usize;
    let mut track_b = 0usize;
    let owner_a = *((anim + 0x8) as *const usize);
    if VRIK_IsReadable(owner_a as *const c_void, 0x48) {
        track_a = *((owner_a + 0x40) as *const usize);
    }
    let owner_b = *((anim + 0x18) as *const usize);
    if VRIK_IsReadable(owner_b as *const c_void, 0x20) {
        track_b = *((owner_b + 0x18) as *const usize);
    }

    // The head, hand and arm bones by the meta-rig's bone names: an exact match, else the shortest name containing
    // the needles (the hand root, not a finger like "RightHandThumb1").
    let meta_rig = owner_a;
    if meta_rig != 0 && cvr_anim_qword_is_heap(meta_rig as u64) {
        let names_entries = *((meta_rig + meta::BONE_NAMES) as *const *const CName);
        let bone_count = *((meta_rig + meta::BONE_NAMES + 0xC) as *const u32);
        if bone_count > 0 && bone_count < 8192 {
            let names = core::slice::from_raw_parts(names_entries, bone_count as usize);
            let (mut head, mut right_hand, mut left_hand) = (-1i32, -1i32, -1i32);
            let (mut right_arm, mut right_fore, mut left_arm, mut left_fore) = (-1i32, -1i32, -1i32, -1i32);
            let (mut right_palm, mut left_palm) = (-1i32, -1i32);
            let mut spine: Vec<i32> = Vec::with_capacity(8);
            let mut fingers: Vec<(i32, Vec<u8>)> = Vec::new();
            let mut fingers_l: Vec<(i32, Vec<u8>)> = Vec::new();
            let (mut smoke_cig, mut smoke_mouth, mut smoke_lighter) = (-1i32, -1i32, -1i32);
            const NO_MATCH: usize = usize::MAX;
            let (mut head_len, mut right_len, mut left_len) = (NO_MATCH, NO_MATCH, NO_MATCH);
            for (i, name) in names.iter().enumerate() {
                let nm = name.text().as_bytes();
                if nm.is_empty() {
                    continue;
                }
                let i = i as i32;
                let len = nm.len();
                if eq(nm, "Head") {
                    head = i;
                    head_len = 0;
                } else if head_len != 0 && contains(nm, "head") && len < head_len {
                    head = i;
                    head_len = len;
                }
                // "_setup" copies of the same names are not part of the animated chain.
                if !contains(nm, "_") {
                    if eq(nm, "RightHandMiddle1") {
                        right_palm = i;
                    }
                    if eq(nm, "LeftHandMiddle1") {
                        left_palm = i;
                    }
                }
                let is_hand = contains(nm, "hand");
                if is_hand && contains(nm, "right") {
                    if eq(nm, "RightHand") {
                        right_hand = i;
                        right_len = 0;
                    } else if right_len != 0 && len < right_len {
                        right_hand = i;
                        right_len = len;
                    }
                }
                if is_hand && contains(nm, "left") {
                    if eq(nm, "LeftHand") {
                        left_hand = i;
                        left_len = 0;
                    } else if left_len != 0 && len < left_len {
                        left_hand = i;
                        left_len = len;
                    }
                }
                if eq(nm, "RightArm") {
                    right_arm = i;
                }
                if eq(nm, "RightForeArm") {
                    right_fore = i;
                }
                if eq(nm, "LeftArm") {
                    left_arm = i;
                }
                if eq(nm, "LeftForeArm") {
                    left_fore = i;
                }
                if crate::skeleton::is_primary_spine(nm) && spine.len() < 8 {
                    spine.push(i);
                }
                let set = |name: &str, target: *mut i32| {
                    if eq(nm, name) {
                        *target = i;
                    }
                };
                set("RightUpLeg", &raw mut g_VRRightUpLegIdx);
                set("LeftUpLeg", &raw mut g_VRLeftUpLegIdx);
                set("RightLeg", &raw mut g_VRRightLegIdx);
                set("LeftLeg", &raw mut g_VRLeftLegIdx);
                set("RightFoot", &raw mut g_VRRightFootIdx);
                set("LeftFoot", &raw mut g_VRLeftFootIdx);
                set("Hips", &raw mut g_VRHipsIdx);
                set("Neck", &raw mut g_VRNeckIdx);
                set("Neck1", &raw mut g_VRNeck1Idx);
                set("LeftEye", &raw mut g_VREyeLeftIdx);
                set("RightEye", &raw mut g_VREyeRightIdx);
                // The FPP camera control chain (frozen by the pose hook).
                let fpp = (&raw mut g_VRFppCamIdx) as *mut i32;
                set("Torso_fppCamera_Control_GRP", fpp);
                set("Torso_fppCamera_Aim_JNT", fpp.add(1));
                set("Torso_fppCamera_Target_JNT", fpp.add(2));
                set("Torso_fppCamera_UpOffset_GRP", fpp.add(3));
                set("Torso_fppCamera_Up_GRP", fpp.add(4));
                // The finger bones of the smoking grip: deform finger and metacarpal joints (no '_', not the wrist,
                // not the hand's end).
                if is_hand && contains(nm, "right") && !eq(nm, "RightHand") && !contains(nm, "_") && !contains(nm, "end") && fingers.len() < 32 {
                    fingers.push((i, nm.to_vec()));
                }
                if eq(nm, "WeaponRight") {
                    smoke_cig = i;
                }
                if eq(nm, "WeaponRight1") {
                    smoke_mouth = i;
                }
                if is_hand && contains(nm, "left") && !eq(nm, "LeftHand") && !contains(nm, "_") && !contains(nm, "end") && fingers_l.len() < 32 {
                    fingers_l.push((i, nm.to_vec()));
                }
                if eq(nm, "WeaponLeft") {
                    smoke_lighter = i;
                }
            }
            if head >= 0 {
                g_VRHeadBoneIdx = head;
            }
            if right_hand >= 0 {
                g_VRRightBoneIdx = right_hand;
            }
            if left_hand >= 0 {
                g_VRLeftBoneIdx = left_hand;
            }
            g_VRPalmRIdx = if right_palm >= 0 { right_palm } else { right_hand };
            g_VRPalmLIdx = if left_palm >= 0 { left_palm } else { left_hand };
            if right_arm >= 0 {
                g_VRRightUpperArmIdx = right_arm;
            }
            if right_fore >= 0 {
                g_VRRightForeArmIdx = right_fore;
            }
            if left_arm >= 0 {
                g_VRLeftUpperArmIdx = left_arm;
            }
            if left_fore >= 0 {
                g_VRLeftForeArmIdx = left_fore;
            }
            g_VRSpineCount = spine.len() as i32;
            for s in 0..8 {
                g_VRSpineIdx[s] = spine.get(s).copied().unwrap_or(-1);
            }
            // The finger sets. Rotations are set to identity only before a pose exists: a rebind on a desync must not
            // wipe the loaded or captured pose.
            let init_rot_r = g_VRSmokeFingerHave == 0;
            for s in 0..32 {
                g_VRSmokeFingerIdx[s] = fingers.get(s).map_or(-1, |f| f.0);
                match fingers.get(s) {
                    Some((_, n)) => copy_name(&mut g_VRSmokeFingerName[s], n),
                    None => g_VRSmokeFingerName[s][0] = 0,
                }
                if init_rot_r {
                    g_VRSmokeFingerRot[s] = [0.0, 0.0, 0.0, 1.0];
                }
            }
            g_VRSmokeFingerCount = fingers.len() as i32;
            g_VRSmokeCigIdx = smoke_cig;
            g_VRSmokeMouthBoneIdx = smoke_mouth;
            let init_rot_l = g_VRSmokeFingerHaveL == 0;
            for s in 0..32 {
                g_VRSmokeFingerIdxL[s] = fingers_l.get(s).map_or(-1, |f| f.0);
                match fingers_l.get(s) {
                    Some((_, n)) => copy_name(&mut g_VRSmokeFingerNameL[s], n),
                    None => g_VRSmokeFingerNameL[s][0] = 0,
                }
                if init_rot_l {
                    g_VRSmokeFingerRotL[s] = [0.0, 0.0, 0.0, 1.0];
                }
                // The left thumb's bones take the trigger's press flick.
                g_VRSmokeThumbIsL[s] = fingers_l.get(s).is_some_and(|(_, n)| contains(&n[..n.len().min(47)], "thumb")) as i32;
            }
            g_VRSmokeFingerCountL = fingers_l.len() as i32;
            g_VRSmokeLighterIdx = smoke_lighter;
            if (!fingers.is_empty() || !fingers_l.is_empty()) && SMOKE_GRIP_LOADED.swap(1, Ordering::Relaxed) == 0 {
                load_smoke_grips();
            }

            // The parent table, for the FK each frame.
            let parents = *((meta_rig + meta::PARENT_INDECES) as *const *const i16);
            let parent_count = *((meta_rig + meta::PARENT_INDECES + 0xC) as *const u32);
            let copy_n = parent_count.min(bone_count) as usize;
            let mut written = 0usize;
            for i in 0..copy_n.min(MAX_BONES) {
                g_VRBoneParent[i] = *parents.add(i);
                written += 1;
            }
            g_VRBoneCount = written as i32;

            // The rig policy, configured on every bind (save or body changes included).
            let policy_names: Vec<*const c_char> = (0..written)
                .map(|i| {
                    let t = names[i].text();
                    if t.is_empty() {
                        core::ptr::null()
                    } else {
                        t.as_ptr() as *const c_char
                    }
                })
                .collect();
            let reference_entries = *((meta_rig + meta::BONE_TRANSFORMS) as *const *const u8);
            let reference_count = *((meta_rig + meta::BONE_TRANSFORMS + 0xC) as *const u32) as usize;
            let reference = if reference_count > 0 { reference_entries } else { core::ptr::null() };
            let reference_limit = reference_count.min(written);
            let reference_readable = !reference.is_null() && reference_limit > 0 && VRIK_IsReadable(reference as *const c_void, reference_limit * 48);
            let female = player_female();
            VRIK_ConfigureRigPolicy(
                policy_names.as_ptr(),
                written as i32,
                if reference_readable { reference } else { core::ptr::null() },
                if reference_readable { reference_limit as i32 } else { 0 },
                female,
            );
            let track_names_entries = *((meta_rig + meta::TRACK_NAMES) as *const *const CName);
            let track_names_count = *((meta_rig + meta::TRACK_NAMES + 0xC) as *const u32) as usize;
            let mut track_names: Vec<*const c_char> = Vec::new();
            if track_names_count <= 8192 {
                for i in 0..track_names_count {
                    let t = (*track_names_entries.add(i)).text();
                    track_names.push(if t.is_empty() { core::ptr::null() } else { t.as_ptr() as *const c_char });
                }
            }
            let reference_tracks_count = *((meta_rig + meta::REFERENCE_TRACKS + 0xC) as *const u32);
            let reference_tracks = if reference_tracks_count != 0 {
                *((meta_rig + meta::REFERENCE_TRACKS) as *const *const f32)
            } else {
                core::ptr::null()
            };
            VRIK_ConfigureCyberwareTracks(track_names.as_ptr(), track_names.len() as i32, reference_tracks);

            // The FK prefix the solve reads (parents precede children in this rig).
            {
                let mut mx = 0i32;
                let mut acc = |v: i32| mx = mx.max(v);
                for v in [
                    g_VRHeadBoneIdx,
                    g_VRRightBoneIdx,
                    g_VRLeftBoneIdx,
                    g_VRRightUpperArmIdx,
                    g_VRRightForeArmIdx,
                    g_VRLeftUpperArmIdx,
                    g_VRLeftForeArmIdx,
                ] {
                    acc(v);
                }
                for s in 0..8 {
                    acc(g_VRSpineIdx[s]);
                }
                for v in [
                    g_VRRightUpLegIdx,
                    g_VRLeftUpLegIdx,
                    g_VRRightLegIdx,
                    g_VRLeftLegIdx,
                    g_VRRightFootIdx,
                    g_VRLeftFootIdx,
                    g_VRHipsIdx,
                    g_VRNeckIdx,
                    g_VRNeck1Idx,
                    g_VREyeLeftIdx,
                    g_VREyeRightIdx,
                ] {
                    acc(v);
                }
                for s in 0..3 {
                    acc(g_VRForeTwistR[s]);
                    acc(g_VRForeTwistL[s]);
                }
                for i in 0..written {
                    if g_VRUpperOwned[i] || g_VRShadowSource[i] >= 0 {
                        acc(i as i32);
                    }
                }
                let lim = mx + 1;
                g_VRFKCount = if lim < g_VRBoneCount { lim } else { g_VRBoneCount };
            }

            // The bone-resolve diagnostic, once (with the verbose diagnostics).
            if BONE_DIAG_WRITTEN.load(Ordering::Relaxed) == 0 && cvr_host::log::verbose() {
                let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "vrik_bone_resolve.txt");
                if let Ok(mut f) = std::fs::File::create(path) {
                    let _ = write!(
                        f,
                        "boneCount={} parentCount={} head={} rightHand={} leftHand={} rightArm={} rightForeArm={} leftArm={} leftForeArm={} spineCount={}\r\n",
                        bone_count,
                        parent_count,
                        g_VRHeadBoneIdx,
                        g_VRRightBoneIdx,
                        g_VRLeftBoneIdx,
                        g_VRRightUpperArmIdx,
                        g_VRRightForeArmIdx,
                        g_VRLeftUpperArmIdx,
                        g_VRLeftForeArmIdx,
                        g_VRSpineCount
                    );
                    BONE_DIAG_WRITTEN.store(1, Ordering::Relaxed);
                }
            }
        }
    }

    // Published only once indices, hierarchy, reference pose and replay state belong to this rig.
    if g_VRBoneCount > 0 && read_binding(anim) == Some(binding) {
        g_PlayerTrackBufA = track_a;
        g_PlayerTrackBufB = track_b;
        *PLAYER_BINDING.lock().unwrap_or_else(|p| p.into_inner()) = binding;
    }
    (g_PlayerTrackBufA != 0) as i32 | if g_PlayerTrackBufB != 0 { 2 } else { 0 }
}

/// Resolve the player's rig for the solve (bootstrap and every real desync); the track-buffer bitmask, or -1.
#[no_mangle]
pub unsafe extern "C" fn VRIK_DoArmPlayer() -> i32 {
    cvr_cpp_pose_lock();
    let result = arm_player_locked();
    cvr_cpp_set_pose_heading_ready(result > 0 && g_VRHeadBoneIdx >= 0);
    cvr_cpp_pose_unlock();
    result
}

// ---- the mirror double ----------------------------------------------------------------------------------------

const BONE_BYTES: usize = 48;
struct Mirror {
    object: usize,
    player_rig: usize,
    bones: u32,
    map: Vec<i16>,
    pose: Vec<u8>,
    pose_bones: u32,
    pending: Vec<u8>,
    pending_bones: u32,
    last_pass_qpc: i64,
}
static MIRROR: RwLock<Mirror> = RwLock::new(Mirror {
    object: 0,
    player_rig: 0,
    bones: 0,
    map: Vec::new(),
    pose: Vec::new(),
    pose_bones: 0,
    pending: Vec::new(),
    pending_bones: 0,
    last_pass_qpc: 0,
});
static MIRROR_ACTIVE: AtomicI32 = AtomicI32::new(0);
static MIRROR_TRACK_A: AtomicUsize = AtomicUsize::new(0);
static MIRROR_TRACK_B: AtomicUsize = AtomicUsize::new(0);
/// [0] binds [1] bind failures [2] player passes cached [3] double passes written [4] double passes skipped
/// [5] bones mapped [6] double bones [7] double passes through buffer A.
/// [0] binds [1] bind failures [2] player passes cached [3] double passes written [4] double passes skipped [5] bones
/// mapped [6] double bones [7] double passes through buffer A -- exported under main's name.
#[export_name = "CyberpunkVR_DebugMirrorDouble"]
static MIRROR_STATS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

fn mirror() -> std::sync::RwLockWriteGuard<'static, Mirror> {
    MIRROR.write().unwrap_or_else(|p| p.into_inner())
}
fn qpc_ms(ticks: i64) -> f64 {
    static PER_MS: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let per_ms = *PER_MS.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: an out-parameter.
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut f) };
        f as f64 / 1000.0
    });
    ticks as f64 / per_ms
}
fn mirror_clear() {
    MIRROR_ACTIVE.store(0, Ordering::Release);
    let mut m = mirror();
    MIRROR_TRACK_A.store(0, Ordering::Relaxed);
    MIRROR_TRACK_B.store(0, Ordering::Relaxed);
    m.object = 0;
    m.player_rig = 0;
    m.bones = 0;
    m.pose_bones = 0;
    m.pending_bones = 0;
    m.last_pass_qpc = 0;
}

#[no_mangle]
pub extern "C" fn VRIK_MirrorDoubleActive() -> bool {
    MIRROR_ACTIVE.load(Ordering::Relaxed) != 0
}
/// The scene tier that suspends VRIK on foot: -1 (never) while a mirror double is bound.
#[no_mangle]
pub extern "C" fn VRIK_EffectiveCutsceneSuspendTier() -> i32 {
    if VRIK_MirrorDoubleActive() {
        -1
    } else {
        // SAFETY: a read of the live controls.
        unsafe { cvr_cpp_cutscene_suspend_tier() }
    }
}
/// After the original apply and the player's own work: a player pass is cached; a double's pass takes the cached
/// pose of the bones both rigs name.
#[no_mangle]
pub unsafe extern "C" fn VRIK_MirrorDoublePass(bone_buf: *mut u8, track_buf: usize, slots: u32) {
    if bone_buf.is_null() {
        return;
    }
    if track_buf == g_PlayerTrackBufA || track_buf == g_PlayerTrackBufB {
        let n = g_VRBoneCount;
        if n <= 0 || n as usize > MAX_BONES || slots < n as u32 {
            return;
        }
        let mut now = 0i64;
        windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now);
        {
            let mut m = mirror();
            let new_batch = m.last_pass_qpc == 0 || qpc_ms(now - m.last_pass_qpc) > CyberpunkVR_VrikBatchGapMs as f64;
            m.last_pass_qpc = now;
            if new_batch && m.pending_bones > 0 {
                let bytes = m.pending_bones as usize * BONE_BYTES;
                let Mirror { pose, pending, .. } = &mut *m;
                let len = bytes.max(pose.len());
                pose.resize(len, 0);
                pose[..bytes].copy_from_slice(&pending[..bytes]);
                m.pose_bones = m.pending_bones;
            }
            let bytes = n as usize * BONE_BYTES;
            let len = bytes.max(m.pending.len());
            m.pending.resize(len, 0);
            m.pending[..bytes].copy_from_slice(core::slice::from_raw_parts(bone_buf, bytes));
            m.pending_bones = n as u32;
        }
        MIRROR_STATS[2].fetch_add(1, Ordering::Relaxed);
        return;
    }
    let a = MIRROR_TRACK_A.load(Ordering::Relaxed);
    let via_a = a != 0 && track_buf == a;
    if !via_a && track_buf != MIRROR_TRACK_B.load(Ordering::Relaxed) {
        return;
    }
    let mut written = false;
    {
        let m = MIRROR.read().unwrap_or_else(|p| p.into_inner());
        let (bones, cached) = (m.bones, m.pose_bones);
        if bones > 0 && cached > 0 && slots >= bones {
            for i in 0..bones as usize {
                let j = m.map.get(i).copied().unwrap_or(-1);
                if j < 0 || j as u32 >= cached {
                    continue;
                }
                let j = j as usize;
                core::ptr::copy_nonoverlapping(m.pose.as_ptr().add(j * BONE_BYTES), bone_buf.add(i * BONE_BYTES), BONE_BYTES);
            }
            written = true;
        }
    }
    MIRROR_STATS[if written { 3 } else { 4 }].fetch_add(1, Ordering::Relaxed);
    if via_a {
        MIRROR_STATS[7].fetch_add(1, Ordering::Relaxed);
    }
}

/// Double bone -> player bone by name hash (the same index first: the same rig family); the number mapped, or -1.
fn build_map(double_names: usize, double_bones: u32, player_names: usize, player_bones: u32) -> Option<(Vec<i16>, i32)> {
    let mut d = vec![0u8; double_bones as usize * 8];
    let mut p = vec![0u8; player_bones as usize * 8];
    if !cvr_engine::code::read_bytes(double_names, &mut d) || !cvr_engine::code::read_bytes(player_names, &mut p) {
        return None;
    }
    let hash = |b: &[u8], i: usize| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
    let mut map = vec![-1i16; double_bones as usize];
    let mut mapped = 0;
    for i in 0..double_bones as usize {
        let h = hash(&d, i);
        if h == 0 {
            continue;
        }
        if i < player_bones as usize && hash(&p, i) == h {
            map[i] = i as i16;
            mapped += 1;
            continue;
        }
        if let Some(j) = (0..player_bones as usize).find(|&j| hash(&p, j) == h) {
            map[i] = j as i16;
            mapped += 1;
        }
    }
    Some((map, mapped))
}
fn track_a_of(object: usize) -> usize {
    match read::<usize>(object + 0x8) {
        Some(owner) if owner != 0 => read::<usize>(owner + 0x40).unwrap_or(0),
        _ => 0,
    }
}

/// VRMirrorDouble(entity, flags) -> Bool: the entity (the scene's NPC copy of V) takes the player's pose until
/// VRMirrorDoubleClear; flags bit 0 identifies its passes by buffer A as well.
#[no_mangle]
pub unsafe extern "C" fn VRMirrorDouble(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let (mut entity, mut flags) = (Handle::null(), 0i32);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut entity);
        frame.get_parameter(&mut flags);
        frame.finish_params();
    }
    if !out.is_null() {
        *out = false;
    }
    let fail = || {
        MIRROR_STATS[1].fetch_add(1, Ordering::Relaxed);
    };
    let instance = entity.instance as *mut c_void;
    let is_entity = !instance.is_null() && object_class(instance).is_some_and(|c| c.is_a(CName::hashed("entEntity")));
    if !is_entity {
        fail();
        return;
    }
    let object = cvr_anim_object_for_entity(instance, c"root".as_ptr(), false) as usize;
    let player_object = player_object();
    let (Some(dbl), Some(player)) = (read_binding(object), read_binding(player_object)) else {
        fail();
        return;
    };
    if object == 0 || object == player_object {
        fail();
        return;
    }
    let track_a = if flags & 1 != 0 { track_a_of(object) } else { 0 };
    let track_b = dbl.track_b;
    let shared = (track_a != 0 && (track_a == g_PlayerTrackBufA || track_a == g_PlayerTrackBufB))
        || (track_b != 0 && (track_b == g_PlayerTrackBufA || track_b == g_PlayerTrackBufB));
    if track_b == 0 || shared {
        fail();
        return;
    }
    {
        let m = MIRROR.read().unwrap_or_else(|p| p.into_inner());
        if MIRROR_ACTIVE.load(Ordering::Acquire) != 0
            && m.object == dbl.object
            && m.player_rig == player.rig
            && MIRROR_TRACK_B.load(Ordering::Relaxed) == track_b
            && MIRROR_TRACK_A.load(Ordering::Relaxed) == track_a
        {
            *out = true;
            return;
        }
    }
    let mapped = {
        let mut m = mirror();
        match build_map(dbl.names, dbl.bones, player.names, player.bones) {
            Some((map, mapped)) if mapped > 0 => {
                m.map = map;
                m.bones = dbl.bones;
                m.object = dbl.object;
                m.player_rig = player.rig;
                m.pose_bones = 0;
                MIRROR_TRACK_A.store(track_a, Ordering::Relaxed);
                MIRROR_TRACK_B.store(track_b, Ordering::Relaxed);
                mapped
            }
            Some((_, mapped)) => mapped,
            None => -1,
        }
    };
    if mapped <= 0 {
        mirror_clear();
        fail();
        return;
    }
    MIRROR_STATS[5].store(mapped as u64, Ordering::Relaxed);
    MIRROR_STATS[6].store(dbl.bones as u64, Ordering::Relaxed);
    MIRROR_STATS[0].fetch_add(1, Ordering::Relaxed);
    MIRROR_ACTIVE.store(1, Ordering::Release);
    if !out.is_null() {
        *out = true;
    }
}
/// VRMirrorDoubleClear(): no double; the scene suspends VRIK again as configured.
#[no_mangle]
pub unsafe extern "C" fn VRMirrorDoubleClear(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    mirror_clear();
}
/// VRMirrorDoubleStats() -> String: the counters, for the harness.
#[no_mangle]
pub unsafe extern "C" fn VRMirrorDoubleStats(_this: *mut c_void, frame: *mut StackFrame, out: *mut CString, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    let s: Vec<u64> = MIRROR_STATS.iter().map(|v| v.load(Ordering::Relaxed)).collect();
    let text = format!(
        "active={} binds={} fails={} cached={} written={} skipped={} mapped={} bones={} viaA={}",
        MIRROR_ACTIVE.load(Ordering::Relaxed),
        s[0],
        s[1],
        s[2],
        s[3],
        s[4],
        s[5],
        s[6],
        s[7]
    );
    if !out.is_null() {
        CString::assign(out, &text);
    }
}
