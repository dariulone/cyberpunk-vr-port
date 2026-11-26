//! LodFov (formerly src/Hooks/LodFov.cpp and include/Hooks/LodFovPatch.hpp, which stays for tools/lod_tests): the
//! level-of-detail cone, and the one site whose value is NOT the render fov. The main camera descriptor builder reads
//! the fov in degrees and derives the screen-space-error threshold max(tan^2(fov/2) * 1.6984, 1); handed a VR field of
//! view that term grows several times and every cull and detail decision fires as if objects were further away. The
//! MAIN builder's read (2.5 exe+0x425A44, `movss xmm0,[rbx+20h]`) goes through a callback that clamps the value used for
//! the computation (a wide fov becomes 3.04639287, which lands on the floor); the other three builders (constructor,
//! plane override, SetFov) have their multiply replaced so the threshold is the floor while xr_fix_lod holds.
//!
//! The same callback is the one place that provably sees the braindance fov: while the braindance (or a takeover's
//! lens) owns the view, the descriptor the scene camera renders through -- recognised by the pose the scene publishes,
//! or by position for a lens -- gets the headset's fov written back. The exports, the shared camera state and the
//! registrations stay C++ (src/Hooks/LodFov.cpp, src/Core/VrCore.cpp).
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use cvr_crt::{format as cformat, Arg};
use cvr_engine::code::{image_base, read, read_bytes, write};
use cvr_engine::trampoline::{allocate, patch_jump, Code};

#[allow(non_upper_case_globals, dead_code)]
extern "C" {
    static CyberpunkVR_BdFovWrite: i32;
    static mut CyberpunkVR_DebugBdHeadWrites: u64;
    static mut CyberpunkVR_DebugBdViewSeen: u64;
    static mut CyberpunkVR_DebugBdViewByFov: u64;
    static mut CyberpunkVR_DebugBdFovWrites: u64;
    static CyberpunkVR_FixLodEnable: i32;
    static mut CyberpunkVR_DebugFixLodCoverage: u32;
    static CyberpunkVR_DebugVrcamWantFov: f32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_normalFovOverrideValue: f32;
    static g_bdActive: AtomicI32;
    static g_bdScenePoseValid: AtomicI32;
    static g_bdSceneQuat: [f32; 4];
    static g_bdScenePosFP: [AtomicI32; 3];
    static g_devCamBaseValid: AtomicI32;
    static g_devCamPosValid: AtomicI32;
    static g_devCamPosFP: [AtomicI32; 3];
    fn cvr_locate_owns_takeover() -> bool;
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_bd_scene_fov() -> f32;
}

/// The braindance view descriptor (read out of the running process): position (int32[3], 1/131072 m), orientation,
/// fov.
const BD_POS: usize = 0x00;
const BD_QUAT: usize = 0x10;
const BD_FOV: usize = 0x20;

static FIX_LOD_HITS: AtomicU64 = AtomicU64::new(0);
static BD_FOV_WRITES: AtomicU64 = AtomicU64::new(0);
static BD_FOV_SEEN: AtomicU64 = AtomicU64::new(0);
/// The scene camera's own fov, learned on a frame the pose test confirmed and before anything of ours was written.
static BD_LEARNED_FOV: AtomicU32 = AtomicU32::new(0);
/// Why the pose test did not match: the stage reached (0 pose invalid, 1 bad quat, 2 dot, 3 dist, 4 matched).
static BD_WHY_DOT: AtomicU32 = AtomicU32::new(0);
static BD_WHY_DIST: AtomicU32 = AtomicU32::new(0);
static BD_WHY_STAGE: AtomicI32 = AtomicI32::new(0);

fn learned() -> f32 {
    f32::from_bits(BD_LEARNED_FOV.load(Ordering::Relaxed))
}
fn diagnostics() -> bool {
    // SAFETY: a plain read of the C++ half's flag.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn verbose() -> bool {
    // SAFETY: a C++ service.
    unsafe { cvr_cpp_verbose_log() != 0 }
}
fn f(v: f32, precision: &core::ffi::CStr) -> String {
    cformat(precision, &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}
fn read_quat(p: usize) -> Option<[f32; 4]> {
    let mut b = [0u8; 16];
    if !read_bytes(p + BD_QUAT, &mut b) {
        return None;
    }
    let q: [f32; 4] = core::array::from_fn(|i| f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]));
    Some(q)
}
fn len2(q: &[f32; 4]) -> f32 {
    q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]
}
/// The struct's own orientation, when it is a unit quaternion.
fn bd_read_quat(p: usize) -> Option<[f32; 4]> {
    read_quat(p).filter(|q| {
        let l = len2(q);
        l > 0.9 && l < 1.1
    })
}
fn position(p: usize) -> Option<[i32; 3]> {
    let mut out = [0i32; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = read::<u32>(p + BD_POS + i * 4)? as i32;
    }
    Some(out)
}
fn distance2(pos: &[i32; 3], reference: &[AtomicI32; 3]) -> f32 {
    let k = 1.0f32 / 131072.0;
    let mut d2 = 0.0f32;
    for i in 0..3 {
        let d = (pos[i] - reference[i].load(Ordering::Relaxed)) as f32 * k;
        d2 += d * d;
    }
    d2
}
/// The same test against the takeover's lens (no scene pose): position only, within half a metre.
unsafe fn dev_view_matches(p: usize) -> Option<[f32; 4]> {
    if g_devCamBaseValid.load(Ordering::Acquire) == 0 || g_devCamPosValid.load(Ordering::Acquire) == 0 {
        return None;
    }
    let q = read_quat(p)?;
    let l = len2(&q);
    if !(l > 0.9 && l < 1.1) {
        return None;
    }
    let pos = position(p)?;
    (distance2(&pos, &g_devCamPosFP) <= 0.5 * 0.5).then_some(q)
}
/// Is this struct the camera the braindance renders through? By the pose the scene reports (orientation within
/// 0.999 and position within 0.75 m), the one identity our own writes cannot feed back into.
unsafe fn bd_view_matches(p: usize) -> Option<[f32; 4]> {
    BD_WHY_STAGE.store(0, Ordering::Relaxed);
    BD_WHY_DOT.store((-1.0f32).to_bits(), Ordering::Relaxed);
    BD_WHY_DIST.store((-1.0f32).to_bits(), Ordering::Relaxed);
    if g_bdScenePoseValid.load(Ordering::Acquire) == 0 {
        return None;
    }
    let q = read_quat(p)?;
    BD_WHY_STAGE.store(1, Ordering::Relaxed);
    let l = len2(&q);
    if !(l > 0.9 && l < 1.1) {
        return None;
    }
    let s = core::ptr::read_volatile(&raw const g_bdSceneQuat);
    let mut dot = q[0] * s[0] + q[1] * s[1] + q[2] * s[2] + q[3] * s[3];
    if dot < 0.0 {
        dot = -dot;
    }
    BD_WHY_STAGE.store(2, Ordering::Relaxed);
    BD_WHY_DOT.store(dot.to_bits(), Ordering::Relaxed);
    if dot < 0.999 {
        return None;
    }
    let pos = position(p)?;
    let d2 = distance2(&pos, &g_bdScenePosFP);
    BD_WHY_STAGE.store(3, Ordering::Relaxed);
    BD_WHY_DIST.store(d2.sqrt().to_bits(), Ordering::Relaxed);
    if d2 > 0.75 * 0.75 {
        return None;
    }
    BD_WHY_STAGE.store(4, Ordering::Relaxed);
    Some(q)
}

/// rcx = rbx (the descriptor), xmm1 = its fov; the fov the computation is to use.
#[no_mangle]
pub unsafe extern "C" fn OnFixLoDCallback(rbx: usize, original: f32) -> f32 {
    let hits = FIX_LOD_HITS.fetch_add(1, Ordering::Relaxed) + 1;
    let mut result = original;
    let takeover = cvr_locate_owns_takeover();
    let bd_active = g_bdActive.load(Ordering::Relaxed) != 0;
    if rbx != 0 && CyberpunkVR_BdFovWrite != 0 && (bd_active || takeover) {
        let p = rbx;
        let want = core::ptr::read_volatile(&raw const g_normalFovOverrideValue);
        let not_ours_yet = (want > 1.0 && want < 179.0) && (original - want).abs() > 0.01;
        // Seeded from the script, unless it is our own output coming back.
        let scene_fov = cvr_cpp_bd_scene_fov();
        if scene_fov > 1.0 && scene_fov < 179.0 && (want <= 1.0 || (scene_fov - want).abs() > 0.01) && (scene_fov - learned()).abs() > 0.01 {
            BD_LEARNED_FOV.store(scene_fov.to_bits(), Ordering::Relaxed);
        }
        let mut is_bd_view = if bd_active {
            bd_view_matches(p).is_some()
        } else {
            dev_view_matches(p).is_some()
        };
        if is_bd_view {
            // Learn the scene's own fov, only from a frame not written yet.
            if not_ours_yet && original > 1.0 && original < 179.0 {
                BD_LEARNED_FOV.store(original.to_bits(), Ordering::Relaxed);
            }
        } else if learned() > 1.0 && (original - learned()).abs() < 0.5 {
            // The published pose is a tick old and this pan outran it: the fov says it is the same view.
            is_bd_view = bd_read_quat(p).is_some();
            if is_bd_view && diagnostics() {
                CyberpunkVR_DebugBdViewByFov += 1;
            }
        }
        if is_bd_view {
            if diagnostics() {
                CyberpunkVR_DebugBdViewSeen += 1;
            }
            // Not on the second eye's view: it carries the value the port computed for it this frame.
            let vrcam_want = core::ptr::read_volatile(&raw const CyberpunkVR_DebugVrcamWantFov);
            let second_eye = vrcam_want > 1.0 && (original - vrcam_want).abs() < 0.05;
            if CyberpunkVR_BdFovWrite != 0 && not_ours_yet && !second_eye && original > 1.0 && original < 179.0 {
                write::<f32>(p + BD_FOV, want);
                if diagnostics() {
                    CyberpunkVR_DebugBdFovWrites += 1;
                }
                result = want;
            }
            if BD_FOV_WRITES.fetch_add(1, Ordering::Relaxed) % 300 == 0 && verbose() {
                super::env::log(&format!(
                    "FixLoD: braindance view at {rbx:016X} fov {} -> {} | head={} views={} byFov={} learned={}\n",
                    f(original, c"%.3f"),
                    f(want, c"%.3f"),
                    CyberpunkVR_DebugBdHeadWrites,
                    CyberpunkVR_DebugBdViewSeen,
                    CyberpunkVR_DebugBdViewByFov,
                    f(learned(), c"%.3f")
                ));
            }
        } else if BD_FOV_SEEN.fetch_add(1, Ordering::Relaxed) % 3000 == 0 && verbose() {
            super::env::log(&format!(
                "FixLoD: braindance, view is not the scene camera: fov {} stage={} dot={} dist={} | want={} learned={} poseValid={}\n",
                f(original, c"%.3f"),
                BD_WHY_STAGE.load(Ordering::Relaxed),
                f(f32::from_bits(BD_WHY_DOT.load(Ordering::Relaxed)), c"%.4f"),
                f(f32::from_bits(BD_WHY_DIST.load(Ordering::Relaxed)), c"%.3f"),
                f(want, c"%.3f"),
                f(learned(), c"%.3f"),
                (g_bdScenePoseValid.load(Ordering::Acquire) != 0) as i32
            ));
        }
    }
    // THE LOD OVERRIDE, on what reaches the engine (after every block that can write it): a fov above the floor's
    // break-even (75 degrees) becomes the historical substitute, below the MAXSS floor.
    if CyberpunkVR_FixLodEnable != 0 && result > 75.0 {
        result = 3.046_392_87;
    }
    if hits % 600 == 1 && verbose() {
        super::env::log(&format!(
            "FixLoD: hits={hits} rbx={rbx:016X} originalVal={} result={} isVRCamera={}\n",
            f(original, c"%.6f"),
            f(result, c"%.6f"),
            (result != original) as i32
        ));
    }
    result
}

// ---- the verified 2.5 sites (include/Hooks/LodFovPatch.hpp) ----
struct Site {
    name: &'static str,
    rva: usize,
    bytes: &'static [u8],
}
const MAIN: Site = Site {
    name: "MAIN",
    rva: 0x425A44,
    bytes: b"\xF3\x0F\x10\x43\x20\xF3\x0F\x59\x05\xBF\x3E\x39\x03\xE8\xDA\xE5\x67\x01\xF6\x83\x99\x03\x00\x00\x01\xF3\x0F\x59\xC0\x40\x88\xB3\x98\x03\x00\x00\xF3\x0F\x59\x05\xD8\x43\x39\x03\xF3\x0F\x5F\x05\xB8\x42\x39\x03\xF3\x0F\x11\x43\x24",
};
const TAILS: [Site; 3] = [
    Site {
        name: "constructor",
        rva: 0x213911,
        bytes: b"\xF3\x0F\x59\x05\x2F\x65\x5A\x03\xF3\x0F\x5F\x05\x0F\x64\x5A\x03\xF3\x0F\x11\x43\x24",
    },
    Site {
        name: "plane override",
        rva: 0x422C90,
        bytes: b"\xF3\x0F\x59\x05\xB0\x71\x39\x03\xF3\x0F\x5F\x05\x90\x70\x39\x03\xF3\x0F\x11\x43\x24",
    },
    Site {
        name: "SetFov",
        rva: 0x206F789,
        bytes: b"\xF3\x0F\x59\x05\xB7\xA6\x74\x01\xF3\x0F\x5F\x05\x97\xA5\x74\x01\xF3\x0F\x11\x43\x24",
    },
];
const SCALE_RVA: usize = 0x37B9E48;
const FLOOR_RVA: usize = 0x37B9D30;
const SCALE: f32 = 1.698_400_7;

fn matches(site: &Site) -> bool {
    let mut got = vec![0u8; site.bytes.len()];
    read_bytes(image_base() + site.rva, &mut got) && got == site.bytes
}
fn constants_match() -> bool {
    let base = image_base();
    read::<f32>(base + SCALE_RVA) == Some(SCALE) && read::<f32>(base + FLOOR_RVA) == Some(1.0)
}
fn validate_tails() -> bool {
    if !constants_match() {
        super::env::log("FixLoD: LOD constants do not match the verified game build\n");
        return false;
    }
    for site in &TAILS {
        if !matches(site) {
            super::env::log(&format!("FixLoD: {} guard mismatch at exe+0x{:X}\n", site.name, site.rva));
            return false;
        }
    }
    true
}
/// The threshold tail's replacement (LodFovPatch.hpp EmitThreshold): with xr_fix_lod the floor (xmm0 = 1.0), else
/// the native multiply by the coefficient; both continue after the original MULSS.
fn emit_threshold(c: &mut Code, enable: u64, scale: u64, continuation: usize) {
    c.bytes(&[0x9C, 0x50]) // pushfq; push rax
        .mov_rax(enable)
        .bytes(&[0x83, 0x38, 0x00]) // cmp dword [rax], 0
        .bytes(&[0x74, 0x10]) // je native multiply
        .bytes(&[0xB8, 0x00, 0x00, 0x80, 0x3F]) // mov eax, 1.0f
        .bytes(&[0x66, 0x0F, 0x6E, 0xC0]) // movd xmm0, eax
        .bytes(&[0x58, 0x9D]) // restore and continue
        .jmp(continuation)
        .mov_rax(scale)
        .bytes(&[0xF3, 0x0F, 0x59, 0x00]) // mulss xmm0, [rax]
        .bytes(&[0x58, 0x9D])
        .jmp(continuation);
}

/// The MAIN builder's read (Boot, 60).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_lod_fov_install() -> bool {
    if !matches(&MAIN) || !validate_tails() {
        super::env::log("FixLoD: verified MAIN/descriptor sites unavailable; no pattern fallback\n");
        return false;
    }
    let found = image_base() + MAIN.rva;
    const REPLACE: usize = 5; // movss
    let Some(tramp) = allocate(found, 256) else { return false };
    let mut c = Code::new(tramp);
    // The C++ stub's frame, with the callback's xmm0 stored over the saved xmm0 before the restore.
    c.bytes(&[0x9C, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x55])
        .bytes(&[0x48, 0x83, 0xEC, 0x40])
        .bytes(&[
            0x0F, 0x11, 0x04, 0x24, 0x0F, 0x11, 0x4C, 0x24, 0x10, 0x0F, 0x11, 0x54, 0x24, 0x20, 0x0F, 0x11, 0x5C, 0x24, 0x30,
        ])
        .bytes(&[0x48, 0x89, 0xE5, 0x48, 0x83, 0xE4, 0xF0, 0x48, 0x83, 0xEC, 0x20])
        .bytes(&[0x48, 0x89, 0xD9]) // mov rcx, rbx
        .bytes(&[0xF3, 0x0F, 0x10, 0x4B, 0x20]) // movss xmm1, [rbx+20h]
        .mov_rax(OnFixLoDCallback as *const () as u64)
        .bytes(&[0xFF, 0xD0]) // call rax
        .bytes(&[0x0F, 0x11, 0x45, 0x00]) // movups [rbp], xmm0
        .bytes(&[0x48, 0x89, 0xEC])
        .bytes(&[
            0x0F, 0x10, 0x04, 0x24, 0x0F, 0x10, 0x4C, 0x24, 0x10, 0x0F, 0x10, 0x54, 0x24, 0x20, 0x0F, 0x10, 0x5C, 0x24, 0x30,
        ])
        .bytes(&[0x48, 0x83, 0xC4, 0x40])
        .bytes(&[0x5D, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x58, 0x9D])
        .jmp(found + REPLACE);
    c.commit();
    patch_jump(found, REPLACE, tramp);
    CyberpunkVR_DebugFixLodCoverage |= 1;
    super::env::log(&format!(
        "FixLoD hook: guarded MAIN at exe+0x{:X} -> {found:016X} (trampoline {tramp:016X})\n",
        MAIN.rva
    ));
    true
}

/// The three other builders' multiply (Boot, 61): only the MULSS by the coefficient is replaced.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_lod_fov_threshold_install() -> bool {
    if CyberpunkVR_DebugFixLodCoverage & 1 == 0 {
        super::env::log("FixLoD threshold: MAIN hook unavailable; descriptor tails not installed\n");
        return false;
    }
    if !validate_tails() {
        return false;
    }
    let base = image_base();
    let mut patched = 0;
    for (i, site) in TAILS.iter().enumerate() {
        let mul = base + site.rva;
        let Some(code) = allocate(mul, 64) else {
            super::env::log(&format!("FixLoD threshold: no trampoline space for {}\n", site.name));
            continue;
        };
        let displacement = code as i64 - (mul as i64 + 5);
        if displacement < i32::MIN as i64 || displacement > i32::MAX as i64 {
            super::env::log(&format!("FixLoD threshold: trampoline out of range for {}\n", site.name));
            continue;
        }
        let mut c = Code::new(code);
        emit_threshold(&mut c, &raw const CyberpunkVR_FixLodEnable as u64, (base + SCALE_RVA) as u64, mul + 8);
        c.commit();
        patch_jump(mul, 8, code);
        CyberpunkVR_DebugFixLodCoverage |= 1 << (i + 1);
        patched += 1;
        super::env::log(&format!(
            "FixLoD threshold: guarded {} at exe+0x{:X} -> trampoline {code:016X}\n",
            site.name, site.rva
        ));
    }
    super::env::log(&format!(
        "FixLoD threshold: {patched}/3 installed; coverage=0x{:X} (complete=0xF)\n",
        CyberpunkVR_DebugFixLodCoverage
    ));
    patched == 3
}
