//! THE NODE DISPATCH'S PROBES (formerly src/Stereo/NodeDispatch.cpp): the per-view node census (the work functions
//! each view runs, and the asymmetry logged), the cloud state at the clouds node's entry diffed between the views, the
//! cloud lighting shader selector (and its optional mirror), the sky record index the views fill, and the scope's
//! render probe. Reading only, but for the selector mirror (default off).
//!
//! NodeDispatch.cpp keeps the exports and calls these from its dispatcher; Detour_CloudsNode stays its C++ name for
//! SyncStereo.cpp's hook.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::stereo_shared as s;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_engine::code::{read, read_bytes, write};
use std::sync::{LazyLock, Mutex};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_vrcam_ctx_key: AtomicU64;
    static CyberpunkVR_DebugMainNodeWorks: [AtomicUsize; 256];
    static CyberpunkVR_DebugSecondaryNodeWorks: [AtomicUsize; 256];
    static CyberpunkVR_DebugMainNodeUnique: AtomicU32;
    static CyberpunkVR_DebugSecondaryNodeUnique: AtomicU32;
    static CyberpunkVR_SkyProbe: AtomicI32;
    static CyberpunkVR_DebugSkyIdxMain: AtomicU32;
    static CyberpunkVR_DebugSkyIdxVrcam: AtomicU32;
    static CyberpunkVR_DebugMainAaMode: AtomicU32;
    static CyberpunkVR_DebugVrcamAaMode: AtomicU32;
    static CyberpunkVR_CloudLightProbe: AtomicI32;
    static CyberpunkVR_CloudLightMirror: AtomicI32;
    static CyberpunkVR_DebugCloudSelMain: AtomicU32;
    static CyberpunkVR_DebugCloudSelVrcam: AtomicU32;
    static CyberpunkVR_DebugCloudSelWrites: AtomicU64;
    static CyberpunkVR_OpticsRenderProbe: AtomicI32;
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn vrcam_key() -> u64 {
    // SAFETY: an export.
    unsafe { g_vrcam_ctx_key.load(SeqCst) }
}
/// The clouds node's trampoline (written by engine_hooks.rs's install).
#[no_mangle]
pub static cvr_clouds_node_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[repr(C)]
pub struct Runtime {
    stereo_log: extern "C" fn(*const c_char),
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here from the objects stereo_shared.rs and moved_globals.rs own (formerly the C++ half's,
/// attached at its static initialisation).
static RT: Runtime = Runtime {
    stereo_log: s::stereo_log,
};
fn rt() -> &'static Runtime {
    &RT
}
fn stereo_log(text: &str) {
    let line = std::ffi::CString::new(text).unwrap_or_default();
    (rt().stereo_log)(line.as_ptr());
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
/// A report due every `ms` across threads (the compare-exchange on the next due time).
fn due(next: &AtomicU64, ms: u64) -> bool {
    let now = tick();
    let at = next.load(Relaxed);
    now >= at
        && next
            .compare_exchange(at, now + ms, Relaxed, Relaxed)
            .is_ok()
}

// ---- the per-view node census: which work functions each view dispatches

static CENSUS: Mutex<()> = Mutex::new(());
/// A work function's RVA added to its view's set (256 at most).
#[no_mangle]
pub extern "C" fn cvr_nd_node_census_add(rva: u32, vrcam: bool) {
    // SAFETY: exports of the C++ half.
    let (list, n) = unsafe {
        if vrcam {
            (
                &CyberpunkVR_DebugSecondaryNodeWorks,
                &CyberpunkVR_DebugSecondaryNodeUnique,
            )
        } else {
            (
                &CyberpunkVR_DebugMainNodeWorks,
                &CyberpunkVR_DebugMainNodeUnique,
            )
        }
    };
    let _lock = CENSUS.lock().unwrap_or_else(|e| e.into_inner());
    let count = n.load(Relaxed) as usize;
    if list[..count.min(256)]
        .iter()
        .any(|w| w.load(Relaxed) as u32 == rva)
        || count >= 256
    {
        return;
    }
    list[count].store(rva as usize, Relaxed);
    n.store(count as u32 + 1, Relaxed);
}
/// The asymmetry, every 20 s once both views were seen: the RVAs one view dispatches and the other does not.
#[no_mangle]
pub extern "C" fn cvr_nd_node_census_dump() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now - last < 20000 {
        return;
    }
    let (main, vrcam): (Vec<u32>, Vec<u32>) = {
        let _lock = CENSUS.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: exports.
        unsafe {
            let m = CyberpunkVR_DebugMainNodeUnique.load(Relaxed).min(256) as usize;
            let v = CyberpunkVR_DebugSecondaryNodeUnique.load(Relaxed).min(256) as usize;
            (
                CyberpunkVR_DebugMainNodeWorks[..m]
                    .iter()
                    .map(|w| w.load(Relaxed) as u32)
                    .collect(),
                CyberpunkVR_DebugSecondaryNodeWorks[..v]
                    .iter()
                    .map(|w| w.load(Relaxed) as u32)
                    .collect(),
            )
        }
    };
    // Wait until both views have been seen at all.
    if main.is_empty() || vrcam.is_empty() {
        return;
    }
    LAST.store(now, Relaxed);
    for (name, a, b) in [("MAIN", &main, &vrcam), ("VRCAM", &vrcam, &main)] {
        let mut line = String::new();
        let mut count = 0;
        for rva in a.iter().filter(|r| !b.contains(r)) {
            count += 1;
            if line.len() < 1024 - 16 {
                line.push_str(&format!("{rva:X} "));
            }
        }
        stereo_log(&format!(
            "[census] {name}-only nodes: {count} of {} | {}",
            a.len(),
            if count != 0 { line.as_str() } else { "(none)" }
        ));
    }
}

// ---- the cloud state, snapped at the clouds node's own entry for both views

/// The per-view cloud state (ctx + 0x2BD8; the selector at +32).
const CLOUD_STATE: usize = 0x2BD8;
/// As much of `n` bytes as is readable, in 32-byte steps.
fn read_upto(dst: &mut [u8], src: usize) -> usize {
    let mut done = 0;
    while done < dst.len() {
        let step = (dst.len() - done).min(32);
        if !read_bytes(src + done, &mut dst[done..done + step]) {
            break;
        }
        done += step;
    }
    done
}
static CLOUD_SNAPS: LazyLock<Mutex<[(Vec<u8>, usize); 2]>> =
    LazyLock::new(|| Mutex::new([(vec![0; 256], 0), (vec![0; 256], 0)]));
fn clouds_node_note(ctx: usize, vrcam: bool) {
    let st: usize = read(ctx + CLOUD_STATE).unwrap_or(0);
    if st == 0 {
        return;
    }
    let mut cur = [0u8; 256];
    let got = read_upto(&mut cur, st);
    if got < 64 {
        return;
    }
    let (reference, n) = {
        let mut snaps = CLOUD_SNAPS.lock().unwrap_or_else(|e| e.into_inner());
        let side = vrcam as usize;
        snaps[side].0[..got].copy_from_slice(&cur[..got]);
        snaps[side].1 = got;
        if !vrcam || snaps[0].1 == 0 {
            return;
        }
        let n = snaps[0].1.min(got) & !3;
        (snaps[0].0[..n].to_vec(), n)
    };
    if n < 64 {
        return;
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if !due(&NEXT, 6000) {
        return;
    }
    let mut line = String::new();
    for k in 0..n / 4 {
        let m = u32::from_ne_bytes(reference[4 * k..4 * k + 4].try_into().unwrap());
        let v = u32::from_ne_bytes(cur[4 * k..4 * k + 4].try_into().unwrap());
        if m != v && line.len() < 900 - 32 {
            line.push_str(&format!("{:X}{{{m:08X}|{v:08X}}} ", k * 4));
        }
    }
    stereo_log(&format!(
        "[cloudnode] state at the clouds-node entry, {n} bytes, M|V: {}",
        if line.is_empty() {
            "(identical)"
        } else {
            line.as_str()
        }
    ));
}
/// The clouds node (CLOUDS_NODE_RVA): both views' cloud state snapped at one place, the node's own entry.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_clouds_node(a1: *mut c_void, a2: *mut c_void) -> u8 {
    if CyberpunkVR_CloudLightProbe.load(Relaxed) != 0 {
        let ctx: usize = if a2.is_null() {
            0
        } else {
            read(a2 as usize + 0x18).unwrap_or(0)
        };
        if ctx != 0 {
            let vrcam = read::<u64>(ctx + 0x28).is_some_and(|k| k == vrcam_key());
            clouds_node_note(ctx, vrcam);
        }
    }
    let original: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u8 =
        core::mem::transmute(cvr_clouds_node_original.load(core::sync::atomic::Ordering::Acquire));
    original(a1, a2)
}
/// The cloud lighting shader selector (cloudState + 32: 1 one permutation, else the other) per view, MAIN's mirrored
/// onto the second view when CyberpunkVR_CloudLightMirror is on.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_cloud_sel_note(ctx: usize, vrcam: bool) {
    if CyberpunkVR_CloudLightProbe.load(Relaxed) == 0 || ctx == 0 {
        return;
    }
    let ok = (|| -> Option<()> {
        let st: usize = read(ctx + CLOUD_STATE)?;
        if st == 0 {
            return None;
        }
        let sel: i32 = read(st + 32)?;
        if vrcam {
            CyberpunkVR_DebugCloudSelVrcam.store(sel as u32, Relaxed);
            let main = CyberpunkVR_DebugCloudSelMain.load(Relaxed);
            if CyberpunkVR_CloudLightMirror.load(Relaxed) != 0
                && main != 0xFFFF_FFFF
                && sel != main as i32
            {
                write(st + 32, main as i32).then_some(())?;
                if diagnostics() {
                    CyberpunkVR_DebugCloudSelWrites.fetch_add(1, SeqCst);
                }
            }
        } else {
            CyberpunkVR_DebugCloudSelMain.store(sel as u32, Relaxed);
        }
        Some(())
    })();
    if ok.is_none() {
        return;
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if !due(&NEXT, 5000) {
        return;
    }
    let (m, v) = (
        CyberpunkVR_DebugCloudSelMain.load(Relaxed),
        CyberpunkVR_DebugCloudSelVrcam.load(Relaxed),
    );
    stereo_log(&format!(
        "[cloudsel] cloudState+32 (picks the lighting shader) -- MAIN {}  VRCAM {}  {}  mirror={} writes={}",
        m as i32,
        v as i32,
        if m == v { "same shader" } else { "<- DIFFERENT SHADERS" },
        CyberpunkVR_CloudLightMirror.load(Relaxed),
        CyberpunkVR_DebugCloudSelWrites.load(Relaxed)
    ));
}
/// The sky record index (view + 0x24C8): the same record means the two views share one amortised sky.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_sky_probe_note(ctx: usize, vrcam: bool) {
    if CyberpunkVR_SkyProbe.load(Relaxed) == 0 || ctx == 0 {
        return;
    }
    let Some(idx) = read::<u8>(ctx + 0x24C8) else {
        return;
    };
    if vrcam {
        CyberpunkVR_DebugSkyIdxVrcam.store(idx as u32, Relaxed);
    } else {
        CyberpunkVR_DebugSkyIdxMain.store(idx as u32, Relaxed);
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if !due(&NEXT, 5000) {
        return;
    }
    let (m, v) = (
        CyberpunkVR_DebugSkyIdxMain.load(Relaxed),
        CyberpunkVR_DebugSkyIdxVrcam.load(Relaxed),
    );
    stereo_log(&format!(
        "[sky] sky-record index view+0x24C8 -- MAIN {m}  VRCAM {v}   {}     AA mode viewData+0xFA4 -- MAIN {}  VRCAM {}",
        if m == v { "<- SAME RECORD: the two views share one amortised sky" } else { "<- separate records" },
        CyberpunkVR_DebugMainAaMode.load(Relaxed),
        CyberpunkVR_DebugVrcamAaMode.load(Relaxed)
    ));
}
static OPTICS_FEED: LazyLock<u64> =
    LazyLock::new(|| crate::vrcam_config::cname_hash(b"vr_optics_feed"));
/// The scope's render probe: for seven nodes, once per view, the view's feature words, the node's execution byte and
/// the manager's selector 102.
#[no_mangle]
pub unsafe extern "C" fn cvr_nd_optics_render_probe(ctx: usize, work: usize, key: u64, rva: u32) {
    if CyberpunkVR_OpticsRenderProbe.load(Relaxed) == 0 || ctx == 0 || work == 0 {
        return;
    }
    let side = if key == 0 {
        0
    } else if key == vrcam_key() {
        1
    } else if key == *OPTICS_FEED {
        2
    } else {
        return;
    };
    const NODES: [u32; 7] = [
        0x557204, 0xf60474, 0x3fe078, 0xdac960, 0x411b94, 0x3e1b20, 0x7f3c20,
    ];
    let Some(i) = NODES.iter().position(|&n| n == rva) else {
        return;
    };
    let bit = 1u32 << i;
    static SEEN: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
    if SEEN[side].fetch_or(bit, SeqCst) & bit != 0 {
        return;
    }
    let _ = (|| -> Option<()> {
        let manager: usize = read(ctx + 0x2B90)?;
        let f0: u64 = read(ctx + 0x25B0)?;
        let f1: u64 = read(ctx + 0x25B8)?;
        let execution: u8 = read(work + 48)?;
        let selector: u32 = if manager != 0 {
            read(manager + 408)?
        } else {
            0
        };
        stereo_log(&format!(
            "[optics-render] side={side} node={rva:X} flags={f0:016X}/{f1:016X} execution={execution} selector102={selector}"
        ));
        Some(())
    })();
}
