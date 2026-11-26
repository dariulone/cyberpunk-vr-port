//! The three plane knobs (formerly src/Stereo/WeaponPlane.cpp; the file keeps the exported switches and their debug
//! states, and its `weapon_plane_sync` / `highlight_any_plane_sync` / `plane_bucket_scene_sync`, which the live-controls
//! poll calls, forward here).
//!
//! * xr_weapon_plane_as_scene (CyberpunkVR_WeaponPlaneAsScene, the launcher's scene-plane choice; on by default since
//!   main's d7239a782 / b3736fdaf): the first-person plane's routing branch in sub_14033DDA4 (PLANE_ROUTE_BRANCH_RVA,
//!   `jz short +2Bh`) becomes `jmp short`, so the first-person draw passes take the scene's view parameters; on 2.5 that
//!   gives the equipped model world occlusion. The separate draw-list bucket does not establish that depth.
//! * xr_highlight_any_plane (CyberpunkVR_HighlightAnyPlane): the highlight gather's per-object plane filter
//!   (PLANE_FILTER_JNZ_RVA, `jnz rel32` on the proxy's plane byte +0x9D) becomes a six-byte nop.
//! * the draw-list bucket (PLANE_BUCKET_LOAD_RVA, `mov sil, [rsi+9Dh]`, the proxy's plane loaded as a draw-list index):
//!   with the scene routing on, the weapon's draws go to the scene list and the background keeps its own (a jump to a
//!   small selector that zeroes SIL only for plane 2, flags kept); xr_plane_bucket_scene (CyberpunkVR_PlaneBucketScene)
//!   is the diagnostic that moves every plane there (`mov sil, 0`). Applied mode: 0 native, 1 all planes, 2 weapon only.
//!
//! The highlight filter defaults to 0 and touches nothing while off; flipping a key applies or reverts its bytes in place. A site is
//! validated against the exact bytes before every write and before every revert, and a mismatch (a game update, another
//! mod ahead of us) leaves it untouched with the reason logged (debug state -1; -2 when VirtualProtect refuses).
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering::*};

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_WeaponPlaneAsScene: AtomicI32;
    static CyberpunkVR_DebugWeaponPlaneState: AtomicI32;
    static CyberpunkVR_HighlightAnyPlane: AtomicI32;
    static CyberpunkVR_DebugHighlightAnyPlane: AtomicI32;
    static CyberpunkVR_PlaneBucketScene: AtomicI32;
    static CyberpunkVR_DebugPlaneBucketScene: AtomicI32;
}
extern "C" {
    /// cvr-ffi's AllocateTrampoline: executable memory within a rel32 jump of the target.
    fn cvr_engine_allocate_trampoline(target: *mut c_void, size: usize) -> *mut c_void;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn VirtualProtect(address: *const c_void, size: usize, protect: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, address: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
}
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

/// `jz short` in sub_14033DDA4: the scene branch above the one multiply by the parameter block's stride (0x3A0).
const PLANE_ROUTE_BRANCH_RVA: usize = 0x33E0B8;
/// `jnz rel32` in the highlight gather: objects whose plane byte does not match the plane gathered are dropped.
const PLANE_FILTER_JNZ_RVA: usize = 0x94EC57;
/// `mov sil, [rsi+9Dh]`: the proxy's plane loaded as the draw list's index.
const PLANE_BUCKET_LOAD_RVA: usize = 0x4A0983;

const ORIG_JZ: u8 = 0x74;
/// jmp short: same operand, same length.
const FORCED: u8 = 0xEB;
/// The displacement to the scene branch, checked as a guard (2.31: 0x1E).
const REL8: u8 = 0x2B;
/// `0F 85 rel32` as shipped; `66 0F 1F 44 00 00` the canonical six-byte nop.
const FILTER_ORIG: [u8; 6] = [0x0F, 0x85, 0xE5, 0x00, 0x00, 0x00];
const FILTER_NOP: [u8; 6] = [0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00];
/// `mov sil, [rsi+9Dh]`; `mov sil, 0` (flags untouched, like the load) padded with nops to the same seven bytes.
const BUCKET_ORIG: [u8; 7] = [0x40, 0x8A, 0xB6, 0x9D, 0x00, 0x00, 0x00];
const BUCKET_ZERO: [u8; 7] = [0x40, 0xB6, 0x00, 0x90, 0x90, 0x90, 0x90];

/// The selector the weapon-only bucket jumps to: the original load, then `pushfq; cmp sil, 2; jne +3; mov sil, 0; popfq;
/// jmp back`.
static SELECT: AtomicUsize = AtomicUsize::new(0);
const CHOOSE: [u8; 12] = [0x9c, 0x40, 0x80, 0xfe, 2, 0x75, 3, 0x40, 0xb6, 0, 0x9d, 0xe9];

/// What this module has actually written, not what was asked for (published to the render workers after the write).
static APPLIED: AtomicI32 = AtomicI32::new(0);
static FILTER_APPLIED: AtomicI32 = AtomicI32::new(0);
static BUCKET_APPLIED: AtomicI32 = AtomicI32::new(0);

fn exe_base() -> usize {
    static BASE: AtomicUsize = AtomicUsize::new(0);
    let mut base = BASE.load(Relaxed);
    if base == 0 {
        // SAFETY: the process image's own handle.
        base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
        BASE.store(base, Relaxed);
    }
    base
}

/// `Log(...)`: the project's log, no stereo switch or rate cap (the C++ file wrote through Log, not cvr::log).
fn log(text: &str) {
    cvr_host::log::write(text);
}

/// `%p` as the C runtime prints it.
fn p(at: usize) -> String {
    format!("{at:016X}")
}

/// Writes `want` into the branch's opcode byte, having first confirmed the site still holds `expect` and the rel8.
unsafe fn write_opcode(site: *mut u8, expect: u8, want: u8) -> bool {
    let (b0, b1) = (*site, *site.add(1));
    if b0 != expect || b1 != REL8 {
        CyberpunkVR_DebugWeaponPlaneState.store(-1, Relaxed);
        log(&format!(
            "[stereo] [plane] branch at {} reads {b0:02X} {b1:02X}, expected {expect:02X} {REL8:02X} -- NOT patched\n",
            p(site as usize)
        ));
        return false;
    }
    let mut old = 0u32;
    if VirtualProtect(site as *const c_void, 1, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        CyberpunkVR_DebugWeaponPlaneState.store(-2, Relaxed);
        log(&format!(
            "[stereo] [plane] VirtualProtect refused at {}\n",
            p(site as usize)
        ));
        return false;
    }
    *site = want;
    let mut junk = 0u32;
    VirtualProtect(site as *const c_void, 1, old, &mut junk);
    FlushInstructionCache(GetCurrentProcess(), site as *const c_void, 2);
    true
}

/// Writes `want` over `expect` at `site` (the filter's and the bucket's sites, which share the filter's debug state for
/// their failures, as the C++ did).
unsafe fn write_block(site: *mut u8, expect: &[u8], want: &[u8]) -> bool {
    let n = expect.len();
    if core::slice::from_raw_parts(site, n) != expect {
        CyberpunkVR_DebugHighlightAnyPlane.store(-1, Relaxed);
        log(&format!(
            "[stereo] [hlplane] site {} does not hold the expected bytes -- NOT patched\n",
            p(site as usize)
        ));
        return false;
    }
    let mut old = 0u32;
    if VirtualProtect(site as *const c_void, n, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        CyberpunkVR_DebugHighlightAnyPlane.store(-2, Relaxed);
        log(&format!(
            "[stereo] [hlplane] VirtualProtect refused at {}\n",
            p(site as usize)
        ));
        return false;
    }
    core::ptr::copy_nonoverlapping(want.as_ptr(), site, n);
    let mut junk = 0u32;
    VirtualProtect(site as *const c_void, n, old, &mut junk);
    FlushInstructionCache(GetCurrentProcess(), site as *const c_void, n);
    true
}

/// The weapon-only bucket's seven bytes at `site`: a rel32 jump to the selector (made once, near the site) and two nops.
unsafe fn weapon_bucket_instruction(site: *mut u8) -> Option<[u8; 7]> {
    let mut select = SELECT.load(Relaxed);
    if select == 0 {
        let code = cvr_engine_allocate_trampoline(site.cast(), 32) as *mut u8;
        if code.is_null() {
            return None;
        }
        let relative = code as isize - (site as isize + 5);
        if relative < i32::MIN as isize || relative > i32::MAX as isize {
            return None;
        }
        core::ptr::copy_nonoverlapping(BUCKET_ORIG.as_ptr(), code, BUCKET_ORIG.len());
        core::ptr::copy_nonoverlapping(CHOOSE.as_ptr(), code.add(BUCKET_ORIG.len()), CHOOSE.len());
        let displacement = code.add(BUCKET_ORIG.len() + CHOOSE.len());
        let back = site.add(BUCKET_ORIG.len()) as isize - (displacement as isize + 4);
        if back < i32::MIN as isize || back > i32::MAX as isize {
            return None;
        }
        core::ptr::copy_nonoverlapping((back as i32).to_le_bytes().as_ptr(), displacement, 4);
        FlushInstructionCache(GetCurrentProcess(), code as *const c_void, displacement.add(4) as usize - code as usize);
        select = code as usize;
        SELECT.store(select, Relaxed);
    }
    let offset = (select as isize - (site as isize + 5)) as i32;
    let mut instruction = [0xe9, 0, 0, 0, 0, 0x90, 0x90];
    instruction[1..5].copy_from_slice(&offset.to_le_bytes());
    Some(instruction)
}

/// The draw-list bucket: the weapon's draws into the scene list with the scene routing on (or every plane, the
/// diagnostic), or the game's own bucketing.
///
/// # Safety
/// Called from the live-controls poll (one thread), in the game's process.
#[no_mangle]
pub unsafe extern "C" fn cvr_plane_bucket_scene_sync() {
    let want = if CyberpunkVR_WeaponPlaneAsScene.load(Relaxed) != 0 {
        if CyberpunkVR_PlaneBucketScene.load(Relaxed) != 0 {
            1
        } else {
            2
        }
    } else {
        0
    };
    let applied = BUCKET_APPLIED.load(Relaxed);
    if want == applied {
        return;
    }
    let base = exe_base();
    if base == 0 {
        return;
    }
    let site = (base + PLANE_BUCKET_LOAD_RVA) as *mut u8;
    let mut selective = [0u8; 7];
    if want == 2 || applied == 2 {
        match weapon_bucket_instruction(site) {
            Some(i) => selective = i,
            None => return,
        }
    }
    let bytes = |mode: i32| -> [u8; 7] {
        match mode {
            2 => selective,
            1 => BUCKET_ZERO,
            _ => BUCKET_ORIG,
        }
    };
    if !write_block(site, &bytes(applied), &bytes(want)) {
        return;
    }
    BUCKET_APPLIED.store(want, Relaxed);
    CyberpunkVR_DebugPlaneBucketScene.store(want, Relaxed);
    log(&format!(
        "[stereo] [bucket] {}\n",
        match want {
            2 => "weapon draws use the scene plane",
            1 => "all draws use the scene plane",
            _ => "game plane selection restored",
        }
    ));
}
/// The scene routing is applied (main's scene_plane_applied: the render workers read it after the branch write).
#[no_mangle]
pub extern "C" fn cvr_scene_plane_applied() -> bool {
    APPLIED.load(Acquire) != 0
}

/// Applies or reverts the highlight gather's per-object plane filter.
///
/// # Safety
/// Called from the live-controls poll (one thread), in the game's process.
#[no_mangle]
pub unsafe extern "C" fn cvr_highlight_any_plane_sync() {
    let want = (CyberpunkVR_HighlightAnyPlane.load(Relaxed) != 0) as i32;
    if want == FILTER_APPLIED.load(Relaxed) {
        return;
    }
    let base = exe_base();
    if base == 0 {
        return;
    }
    let site = (base + PLANE_FILTER_JNZ_RVA) as *mut u8;
    if want != 0 {
        if !write_block(site, &FILTER_ORIG, &FILTER_NOP) {
            return;
        }
        FILTER_APPLIED.store(1, Relaxed);
        CyberpunkVR_DebugHighlightAnyPlane.store(1, Relaxed);
        log(&format!(
            "[stereo] [hlplane] xr_highlight_any_plane=1: the per-object plane filter at {} is a nop, so objects on \
             the first-person plane are no longer dropped from the gather.\n",
            p(site as usize)
        ));
    } else {
        if !write_block(site, &FILTER_NOP, &FILTER_ORIG) {
            return;
        }
        FILTER_APPLIED.store(0, Relaxed);
        CyberpunkVR_DebugHighlightAnyPlane.store(0, Relaxed);
        log(&format!(
            "[stereo] [hlplane] xr_highlight_any_plane=0: the game's own plane filter restored at {}.\n",
            p(site as usize)
        ));
    }
}

/// Applies or reverts the plane routing branch. Acts only when the requested state differs from what was last written.
///
/// # Safety
/// Called from the live-controls poll (one thread), in the game's process.
#[no_mangle]
pub unsafe extern "C" fn cvr_weapon_plane_sync() {
    let want = (CyberpunkVR_WeaponPlaneAsScene.load(Relaxed) != 0) as i32;
    if want == APPLIED.load(Relaxed) {
        return;
    }
    let base = exe_base();
    if base == 0 {
        return;
    }
    let site = (base + PLANE_ROUTE_BRANCH_RVA) as *mut u8;
    if want != 0 {
        if !write_opcode(site, ORIG_JZ, FORCED) {
            return;
        }
        APPLIED.store(1, Release);
        CyberpunkVR_DebugWeaponPlaneState.store(1, Relaxed);
        log(&format!("[stereo] [plane] xr_weapon_plane_as_scene=1: using scene view parameters at {}\n", p(site as usize)));
    } else {
        if !write_opcode(site, FORCED, ORIG_JZ) {
            return;
        }
        APPLIED.store(0, Release);
        CyberpunkVR_DebugWeaponPlaneState.store(0, Relaxed);
        log(&format!(
            "[stereo] [plane] xr_weapon_plane_as_scene=0: the game's own plane routing restored at {}.\n",
            p(site as usize)
        ));
    }
}
