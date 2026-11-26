//! WHAT A VIEW MAY RUN, ASKED PER NODE (the feature and capability tests of formerly src/Stereo/NodeDispatch.cpp):
//! the per-bit feature test with its probe (the bits the shadow-mask pass asks, per view), the capability test --
//! the scope view refused the weapon plane, the second view's AutoGrass only while its terrain buffer exists, the
//! refusal census, the honest grant, the HUD node's override for the second view --, and the render-element pass
//! driver running a node whose view is refused the weapon plane on a copy of itself without those passes.
//! And the per-view render mask: the named categories granted, the world categories the eye has shared with the
//! other views, the scope's WeaponPlane-only bits cleared.
//!
//! NodeDispatch.cpp keeps the exports, the trampolines, the registry lines and the C++ name Detour_ViewFeatureCheck
//! (SyncStereo.cpp installs it), and hands this file the node dispatch's thread state and the render-mask table.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::moved_globals as m;
use crate::stereo_shared as s;
use core::ffi::c_char;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use cvr_engine::code::{read, read_bytes, write};
use std::sync::{LazyLock, Mutex};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_FeatBitProbe: AtomicI32;
    static CyberpunkVR_FeatBitNode: AtomicU32;
    static CyberpunkVR_AutoGrassReadiness: [AtomicU64; 4];
    static CyberpunkVR_OpticsHideWeaponPlane: AtomicI32;
    static CyberpunkVR_CapCensus: AtomicI32;
    static CyberpunkVR_DebugCapDenies: AtomicU64;
    static CyberpunkVR_CapGrant: AtomicU32;
    static CyberpunkVR_DebugCapGrants: AtomicU64;
    static CyberpunkVR_HudInVrcam: AtomicI32;
    static CyberpunkVR_DebugHudGateDenied: AtomicU64;
    static CyberpunkVR_DebugHudGateForced: AtomicU64;
    static CyberpunkVR_WeaponPassListsFiltered: AtomicU64;
}
pub(crate) fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn diag(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, SeqCst);
    }
}

/// The trampolines MinHook writes : the per-bit feature test and the render-element pass driver (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_nd_originals: [core::sync::atomic::AtomicUsize; 2] = [const { core::sync::atomic::AtomicUsize::new(0) }; 2];

/// The view capability test's trampoline (written by engine_hooks.rs's install).
#[no_mangle]
pub static cvr_view_feature_check_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// What NodeDispatch.cpp shares with these detours.
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    /// kRenderMasks[index].desc_rva.
    render_mask_desc: extern "C" fn(u32) -> u32,
    /// &DRAWHUD_WORK_RVA (Profiler.cpp).
    drawhud_work_rva: *const u32,
    stereo_log: extern "C" fn(*const c_char),
    /// kRenderMaskCount.
    /// g_vrcam_ctx_seen: the eye's view context, as the render mask last saw it.
    vrcam_ctx_seen: *const core::sync::atomic::AtomicUsize,
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    exe_base: (&raw const s::g_exe_base).cast(),
    render_mask_desc: s::cvr_cpp_render_mask_desc,
    drawhud_work_rva: &raw const s::DRAWHUD_WORK_RVA,
    stereo_log: s::stereo_log,
    vrcam_ctx_seen: &raw const m::g_vrcam_ctx_seen,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn base() -> usize {
    // SAFETY: attached.
    unsafe { *rt().exe_base }
}
/// The node a work function belongs to, as an RVA (0 outside a node or below the image).
fn node_rva(work: usize) -> u32 {
    let base = base();
    if base != 0 && work > base {
        (work - base) as u32
    } else {
        0
    }
}
static OPTICS_FEED: LazyLock<u64> =
    LazyLock::new(|| crate::vrcam_config::cname_hash(b"vr_optics_feed"));
/// kRenderMasks rows: AutoGrass and the weapon plane.
const MASK_AUTO_GRASS: u32 = 1;
const MASK_WEAPON_PLANE: u32 = 23;
/// The capability nodes the default grant covers.
const CLUSTERED_LIGHTS_CULL_RVA: u32 = 0x410F1C;
const RENDER_LIGHT_BUFFERS_RVA: u32 = 0x411B94;

// ---- the feature bits the shadow-mask pass asks about, per view (answered: they agree; default off)

struct FeatBit {
    bit: u32,
    yes: [u64; 2],
    no: [u64; 2],
}
static FEAT_BITS: Mutex<Vec<FeatBit>> = Mutex::new(Vec::new());
fn featbit_note(bit: u32, answer: bool, vrcam: bool) {
    let v = vrcam as usize;
    let mut bits = FEAT_BITS.lock().unwrap_or_else(|e| e.into_inner());
    let i = match bits.iter().position(|f| f.bit == bit) {
        Some(i) => i,
        None if bits.len() < 32 => {
            bits.push(FeatBit {
                bit,
                yes: [0; 2],
                no: [0; 2],
            });
            bits.len() - 1
        }
        None => return,
    };
    if answer {
        bits[i].yes[v] += 1;
    } else {
        bits[i].no[v] += 1;
    }
}
fn featbit_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    // SAFETY: a plain system query.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let last = LAST.load(Relaxed);
    if last != 0 && now - last < 5000 {
        return;
    }
    LAST.store(now, Relaxed);
    let mut line = String::new();
    {
        let bits = FEAT_BITS.lock().unwrap_or_else(|e| e.into_inner());
        for f in bits.iter() {
            // A bit is interesting when the two views do not agree.
            let mark = if (f.yes[0] > f.no[0]) != (f.yes[1] > f.no[1]) {
                " <-- DIFFERS"
            } else {
                ""
            };
            if line.len() < 1000 - 80 {
                line.push_str(&format!(
                    "bit{} M={}/{} V={}/{}{mark}  ",
                    f.bit, f.yes[0], f.no[0], f.yes[1], f.no[1]
                ));
            }
        }
    }
    // SAFETY: an export.
    let node = unsafe { CyberpunkVR_FeatBitNode.load(Relaxed) };
    stereo_log(&format!(
        "[featbit] node {node:X} feature answers (yes/no per view): {}",
        if line.is_empty() {
            "(none seen)"
        } else {
            line.as_str()
        }
    ));
}
/// The per-bit feature test (sub_14033E358(work, bit)), answered by the original; with the probe on, the answers
/// inside CyberpunkVR_FeatBitNode counted per view.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_feature_bit(work_context: usize, bit: usize) -> u8 {
    let original: unsafe extern "C" fn(usize, usize) -> u8 =
        core::mem::transmute(cvr_nd_originals[0].load(Acquire));
    let answer = original(work_context, bit);
    if CyberpunkVR_FeatBitProbe.load(Relaxed) != 0 && base() != 0 && bit < 64 {
        let work = crate::render_tls::current_node_work();
        if work > base() && node_rva(work) == CyberpunkVR_FeatBitNode.load(Relaxed) {
            featbit_note(bit as u32, answer != 0, crate::render_tls::vrcam_node_active());
            featbit_report();
        }
    }
    answer
}

// ---- the capability test (sub_14021BE28(work, descriptor))

/// The second view's refusals per (node, descriptor), the table printed whenever a new pair appears.
static CAP_DENIES: Mutex<Vec<(u32, u32, u64)>> = Mutex::new(Vec::new());
fn cap_census_note(work: usize, required: usize) {
    let base = base();
    if base == 0 {
        return;
    }
    let (node, desc) = (
        node_rva(work),
        if required > base {
            (required - base) as u32
        } else {
            0
        },
    );
    // SAFETY: an export.
    diag(unsafe { &CyberpunkVR_DebugCapDenies });
    let mut denies = CAP_DENIES.lock().unwrap_or_else(|e| e.into_inner());
    let i = match denies.iter().position(|d| d.0 == node && d.1 == desc) {
        Some(i) => i,
        None if denies.len() < 32 => {
            denies.push((node, desc, 0));
            let mut line = String::new();
            for d in denies.iter() {
                if line.len() < 900 - 32 {
                    line.push_str(&format!("node {:X}/desc {:X} ", d.0, d.1));
                }
            }
            stereo_log(&format!(
                "[cap] VRCAM capability refusals ({} distinct): {line}",
                denies.len()
            ));
            denies.len() - 1
        }
        None => return,
    };
    denies[i].2 += 1;
}
/// The test made to pass honestly: the descriptor's words (+1 qword: the test compares mask[i] with
/// descriptor[i + 1]) OR-ed into the view's own mask at ctx + 0x2688. True when anything changed.
fn cap_grant_required(work_context: usize, required: usize) -> bool {
    if work_context == 0 || required == 0 {
        return false;
    }
    (|| -> Option<bool> {
        let ctx: usize = read(work_context + 0x18)?;
        if ctx == 0 {
            return Some(false);
        }
        let mut changed = false;
        for i in 0..32 {
            let need: u64 = read(required + 8 + 8 * i)?;
            let have: u64 = read(ctx + 0x2688 + 8 * i)?;
            let missing = need & !have;
            if missing != 0 {
                write(ctx + 0x2688 + 8 * i, have | missing).then_some(())?;
                changed = true;
            }
        }
        Some(changed)
    })()
    .unwrap_or(false)
}
/// The scope view refused RenderMask/WeaponPlane (asked here, from the view the node renders, because the view
/// context's mask is rewritten from another thread).
unsafe fn optics_refuses(work_context: usize, required: usize) -> bool {
    let base = base();
    if base == 0
        || CyberpunkVR_OpticsHideWeaponPlane.load(Relaxed) == 0
        || required != base + (rt().render_mask_desc)(MASK_WEAPON_PLANE) as usize
    {
        return false;
    }
    (|| -> Option<bool> {
        let ctx: usize = read(work_context + 0x18)?;
        Some(ctx != 0 && read::<u64>(ctx + 0x28)? == *OPTICS_FEED)
    })()
    .unwrap_or(false)
}
/// CP2077 PrepareAutoSpawnOnTerrain reads the shared terrain state through work + 0x20 -> + 0x98 and passes
/// state + 0x7B0 on without a null-handle check: 0 ready, 1 no context, 2 no state, 3 no buffer.
fn auto_grass_readiness(work_context: usize) -> usize {
    let owner: usize = if work_context != 0 {
        read(work_context + 0x20).unwrap_or(0)
    } else {
        0
    };
    if owner == 0 {
        return 1;
    }
    let state: usize = read(owner + 0x98).unwrap_or(0);
    if state == 0 {
        return 2;
    }
    match read::<u32>(state + 0x7B0) {
        Some(buffer) if buffer != 0 && buffer != u32::MAX => 0,
        _ => 3,
    }
}
/// The capability test: the scope's weapon plane refused; the second view's AutoGrass grant only while the native
/// buffer exists (a zero handle decremented to 0xFFFFFFFF read past the resource table); the second view's refusals
/// counted and, by CyberpunkVR_CapGrant, granted; inside DrawHUD on the second view every refusal overridden.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_view_feature_check(work_context: usize, required: usize) -> u8 {
    if optics_refuses(work_context, required) {
        return 0;
    }
    let r = rt();
    let original: unsafe extern "C" fn(usize, usize) -> u8 =
        core::mem::transmute(cvr_view_feature_check_original.load(core::sync::atomic::Ordering::Acquire));
    let answer = original(work_context, required);
    let vrcam = crate::render_tls::vrcam_node_active();
    let base = base();
    if vrcam && base != 0 && required == base + (r.render_mask_desc)(MASK_AUTO_GRASS) as usize {
        let status = auto_grass_readiness(work_context);
        diag(&CyberpunkVR_AutoGrassReadiness[status]);
        if status != 0 {
            return 0;
        }
    }
    let work = crate::render_tls::current_node_work();
    if answer == 0 && vrcam && CyberpunkVR_CapCensus.load(Relaxed) != 0 {
        cap_census_note(work, required);
    }
    let grant = CyberpunkVR_CapGrant.load(Relaxed);
    if answer == 0 && vrcam && grant != 0 && base != 0 {
        let node = node_rva(work);
        let wanted =
            grant >= 2 || node == CLUSTERED_LIGHTS_CULL_RVA || node == RENDER_LIGHT_BUFFERS_RVA;
        if wanted && cap_grant_required(work_context, required) {
            diag(&CyberpunkVR_DebugCapGrants);
            // Now it passes on merit.
            return original(work_context, required);
        }
    }
    if CyberpunkVR_HudInVrcam.load(Relaxed) == 0 || !vrcam || answer != 0 {
        return answer;
    }
    if work == 0 || base == 0 || work <= base || node_rva(work) != *r.drawhud_work_rva {
        return answer;
    }
    // DrawHUD asks twice; inside the HUD node on the second eye every refusal is the same refusal.
    diag(&CyberpunkVR_DebugHudGateDenied);
    diag(&CyberpunkVR_DebugHudGateForced);
    static SAID: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, Relaxed) {
        stereo_log("[hud] second-eye capability refusal overridden inside DrawHUD -- HUD should now render for VRCAM (set CyberpunkVR_HudInVrcam=0 to revert)");
    }
    1
}

// ---- the render-element pass driver: a refused weapon plane taken out of the pass list, not skipped

const PASS_ENTRY: usize = 48;
const PASS_FLAGS: usize = 8;
const WEAPON_PASS: u32 = 0x20;
/// The node's pass list and count when the list holds a weapon-plane pass.
fn weapon_passes(node: usize) -> Option<(usize, u32)> {
    let list: usize = read(node + 48)?;
    let count: u32 = read(node + 60)?;
    if list == 0 || count > 4096 {
        return None;
    }
    for i in 0..count as usize {
        if read::<u32>(list + i * PASS_ENTRY + PASS_FLAGS)? & WEAPON_PASS != 0 {
            return Some((list, count));
        }
    }
    None
}
/// The render-element pass driver (sub_14033DDA4(node, work, args)): it remembers each pass's flags BEFORE asking
/// whether a weapon-plane pass may draw, so a refused pass followed by one with the same flags draws through a
/// functor the skipped pass never built (the call through null at +0x2093FC2). A node whose view is refused the
/// weapon plane runs on a copy of its first 64 bytes whose list (+48, count +60) leaves those passes out.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_pass_driver(node: usize, work: usize, args: usize) {
    let r = rt();
    let original: unsafe extern "C" fn(usize, usize, usize) =
        core::mem::transmute(cvr_nd_originals[1].load(Acquire));
    // the late scene velocity passes with the first-person geometry's own motion added (main's 269e9ee6d)
    if crate::scene_velocity::draw(node, work, args, base(), original) {
        return;
    }
    let found = if node != 0 { weapon_passes(node) } else { None };
    let base = base();
    let Some((list, count)) = found else {
        original(node, work, args);
        return;
    };
    if base == 0
        || work == 0
        || cvr_nd_view_feature_check(
            work,
            base + (r.render_mask_desc)(MASK_WEAPON_PLANE) as usize,
        ) != 0
    {
        original(node, work, args);
        return;
    }
    let mut kept: Vec<u8> = Vec::with_capacity(count as usize * PASS_ENTRY);
    let mut entry = [0u8; PASS_ENTRY];
    let mut readable = true;
    for i in 0..count as usize {
        if !read_bytes(list + i * PASS_ENTRY, &mut entry) {
            readable = false;
            break;
        }
        let flags = u32::from_ne_bytes([entry[8], entry[9], entry[10], entry[11]]);
        if flags & WEAPON_PASS == 0 {
            kept.extend_from_slice(&entry);
        }
    }
    #[repr(C, align(16))]
    struct NodeCopy([u8; 64]);
    let mut copy = NodeCopy([0; 64]);
    if !readable || !read_bytes(node, &mut copy.0) {
        original(node, work, args);
        return;
    }
    let n = (kept.len() / PASS_ENTRY) as u32;
    copy.0[48..56].copy_from_slice(&(kept.as_ptr() as usize).to_ne_bytes());
    copy.0[60..64].copy_from_slice(&n.to_ne_bytes());
    diag(&CyberpunkVR_WeaponPassListsFiltered);
    // Which views this happens to, once per view key.
    static KEYS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
    let key = (|| -> Option<u64> {
        let ctx: usize = read(work + 0x18)?;
        if ctx == 0 {
            return Some(!0);
        }
        read(ctx + 0x28)
    })()
    .unwrap_or(!0);
    for slot in &KEYS {
        let seen = slot.load(Relaxed);
        if seen == key.wrapping_add(1) {
            break;
        }
        if seen == 0
            && slot
                .compare_exchange(0, key.wrapping_add(1), SeqCst, SeqCst)
                .is_ok()
        {
            stereo_log(&format!(
                "[optics] weapon-plane passes left out of a node's list: view key {key:016X}{}, {n} of {count} kept",
                if key == *OPTICS_FEED { " (scope view)" } else { "" }
            ));
            break;
        }
    }
    original(copy.0.as_mut_ptr() as usize, work, args);
}

// ---- the per-view render mask: the named categories granted, the world categories shared, the scope's weapon plane

extern "C" {
    static CyberpunkVR_RenderMaskGrant: AtomicU32;
    static CyberpunkVR_DebugRenderMaskGrants: AtomicU64;
    static CyberpunkVR_MaskShareFast: AtomicI32;
    static CyberpunkVR_MaskShareTiming: AtomicI32;
    static CyberpunkVR_MaskShareTicks: AtomicU64;
    static CyberpunkVR_MaskShareCalls: AtomicU64;
}
type MaskWords = [u64; 32];
/// The view's 32-qword render mask.
const VIEW_MASK: usize = 0x2688;
/// HUD, DebugDraw and Discarded: the eye's own ownership, never shared.
const UNSHARED_MASKS: [u32; 3] = [12, 25, 26];
/// A category's required words: descriptor + 8 (the test compares mask[i] with descriptor[i + 1]).
fn mask_need(index: u32) -> Option<MaskWords> {
    read(base() + (rt().render_mask_desc)(index) as usize + 8)
}
struct WorldMask {
    mask: MaskWords,
    source: MaskWords,
}
static WORLD_MASK: Mutex<WorldMask> = Mutex::new(WorldMask {
    mask: [0; 32],
    source: [0; 32],
});
/// Bumped under the lock whenever the world mask is rebuilt.
static WORLD_MASK_GEN: AtomicU64 = AtomicU64::new(0);
fn world_mask() -> std::sync::MutexGuard<'static, WorldMask> {
    WORLD_MASK.lock().unwrap_or_else(|e| e.into_inner())
}
/// The eye's enabled world categories rebuilt when its mask changes; any other view given them.
fn share_world_render_mask_locked(ctx: usize, eye: bool) {
    let Some(mut have) = read::<MaskWords>(ctx + VIEW_MASK) else {
        return;
    };
    let mut w = world_mask();
    if eye {
        if have == w.source {
            return;
        }
        w.source = have;
        w.mask = [0; 32];
        for k in 0..s::cvr_cpp_render_mask_count() {
            if UNSHARED_MASKS.contains(&k) {
                continue;
            }
            let Some(need) = mask_need(k) else { continue };
            if (0..32).all(|i| have[i] & need[i] == need[i]) {
                for i in 0..32 {
                    w.mask[i] |= need[i];
                }
            }
        }
        WORLD_MASK_GEN.fetch_add(1, Release);
    } else {
        for i in 0..32 {
            have[i] |= w.mask[i];
        }
        write(ctx + VIEW_MASK, have);
    }
}
thread_local! {
    static T_SOURCE: core::cell::Cell<MaskWords> = const { core::cell::Cell::new([0; 32]) };
    static T_MASK: core::cell::Cell<MaskWords> = const { core::cell::Cell::new([0; 32]) };
    static T_GEN: core::cell::Cell<u64> = const { core::cell::Cell::new(!0) };
    static T_WANT: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    static T_UNION: core::cell::Cell<MaskWords> = const { core::cell::Cell::new([0; 32]) };
}
/// The same result without the lock: the eye compares with the last snapshot this thread saw and locks only when its
/// mask changed; any other view keeps a per-thread copy of the shared mask (by generation) and ORs in only the words
/// that are missing.
fn share_world_render_mask_fast(ctx: usize, eye: bool) {
    let Some(have) = read::<MaskWords>(ctx + VIEW_MASK) else {
        return;
    };
    if eye {
        if have == T_SOURCE.get() {
            return;
        }
        share_world_render_mask_locked(ctx, true);
        T_SOURCE.set(have);
        return;
    }
    let generation = WORLD_MASK_GEN.load(Acquire);
    if generation != T_GEN.get() {
        let w = world_mask();
        T_MASK.set(w.mask);
        T_GEN.set(WORLD_MASK_GEN.load(Relaxed));
    }
    let mask = T_MASK.get();
    for i in 0..32 {
        let missing = mask[i] & !have[i];
        if missing != 0 {
            let at = ctx + VIEW_MASK + 8 * i;
            match read::<u64>(at) {
                Some(current) => {
                    write(at, current | missing);
                }
                None => return,
            }
        }
    }
}
fn share_world_render_mask(ctx: usize, eye: bool) {
    // SAFETY: exports.
    let (fast, timing) = unsafe {
        (
            CyberpunkVR_MaskShareFast.load(Relaxed) != 0,
            CyberpunkVR_MaskShareTiming.load(Relaxed) != 0,
        )
    };
    let run = || {
        if fast {
            share_world_render_mask_fast(ctx, eye)
        } else {
            share_world_render_mask_locked(ctx, eye)
        }
    };
    if !timing {
        run();
        return;
    }
    use windows::Win32::System::Performance::QueryPerformanceCounter;
    let (mut t0, mut t1) = (0i64, 0i64);
    // SAFETY: out parameters.
    let _ = unsafe { QueryPerformanceCounter(&mut t0) };
    run();
    // SAFETY: out parameters; exports.
    unsafe {
        let _ = QueryPerformanceCounter(&mut t1);
        CyberpunkVR_MaskShareTicks.fetch_add((t1 - t0) as u64, SeqCst);
        CyberpunkVR_MaskShareCalls.fetch_add(1, SeqCst);
    }
}
/// The categories in CyberpunkVR_RenderMaskGrant (bit per kRenderMasks row: DistantLights, AutoGrass,
/// GameplayPostProcess by default) OR-ed into the view's mask -- their union built once per value on each thread --,
/// then the world categories shared; the eye's context noted for the render-mask report.
#[no_mangle]
pub extern "C" fn cvr_nd_render_mask_grant(ctx: usize, eye: bool) {
    // SAFETY: an export.
    let want = unsafe { CyberpunkVR_RenderMaskGrant.load(Relaxed) };
    if ctx == 0 || want == 0 || base() == 0 {
        return;
    }
    let granted = (|| -> Option<bool> {
        if T_WANT.get() != want {
            let mut union: MaskWords = [0; 32];
            for k in 0..s::cvr_cpp_render_mask_count() {
                if want & (1 << k) == 0 {
                    continue;
                }
                let need = mask_need(k)?;
                for i in 0..32 {
                    union[i] |= need[i];
                }
            }
            T_UNION.set(union);
            T_WANT.set(want);
        }
        let union = T_UNION.get();
        let mut changed = false;
        for i in 0..32 {
            let at = ctx + VIEW_MASK + 8 * i;
            let have: u64 = read(at)?;
            let missing = union[i] & !have;
            if missing != 0 {
                write(at, have | missing).then_some(())?;
                changed = true;
            }
        }
        Some(changed)
    })();
    match granted {
        None => {
            T_WANT.set(0);
            return;
        }
        // SAFETY: an export.
        Some(true) => diag(unsafe { &CyberpunkVR_DebugRenderMaskGrants }),
        Some(false) => {}
    }
    share_world_render_mask(ctx, eye);
    if eye {
        // SAFETY: a C++ global, attached.
        unsafe { (*rt().vrcam_ctx_seen).store(ctx, Release) };
    }
}
/// The bits WeaponPlane requires that no other category does (the categories share their view-type bits).
static WEAPON_PLANE_ONLY: LazyLock<MaskWords> = LazyLock::new(|| {
    let mut result: MaskWords = [0; 32];
    let Some(weapon) = mask_need(MASK_WEAPON_PLANE) else {
        return result;
    };
    let mut others: MaskWords = [0; 32];
    for k in 0..s::cvr_cpp_render_mask_count() {
        if k == MASK_WEAPON_PLANE {
            continue;
        }
        if let Some(need) = mask_need(k) {
            for i in 0..32 {
                others[i] |= need[i];
            }
        }
    }
    let mut bits = 0;
    for i in 0..32 {
        result[i] = weapon[i] & !others[i];
        bits += result[i].count_ones();
    }
    stereo_log(&format!(
        "[optics] scope view hides the weapon plane: {bits} mask bit(s)"
    ));
    result
});
/// THE SCOPE CAMERA MUST NOT SEE THE PLAYER'S OWN FIRST-PERSON PLANE: its WeaponPlane-only bits cleared at every
/// scope node (the engine rewrites the mask once per frame).
#[no_mangle]
pub extern "C" fn cvr_nd_optics_hide_weapon_plane(ctx: usize) {
    // SAFETY: an export.
    if ctx == 0 || unsafe { CyberpunkVR_OpticsHideWeaponPlane.load(Relaxed) } == 0 || base() == 0 {
        return;
    }
    let only = *WEAPON_PLANE_ONLY;
    for i in 0..32 {
        let at = ctx + VIEW_MASK + 8 * i;
        match read::<u64>(at) {
            Some(have) if have & only[i] != 0 => {
                write(at, have & !only[i]);
            }
            Some(_) => {}
            None => return,
        }
    }
}
