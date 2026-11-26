//! What each view's shaders are actually TOLD, as opposed to what we believe we set (formerly
//! src/Stereo/ViewConstants.cpp; the file keeps the probes' exported switches and counters, the upload-heap lookup the
//! capture shares, and its names, which the capture's descriptor hooks call, forward here).
//!
//! Two probes with one purpose. The cbv probe records how many constant-buffer views each node binds per view (the
//! 256-byte block bound at b6), so "the second view's lighting is wrong" can be narrowed to a node that binds a
//! different number of them, and dumps one node's block for both views side by side. The camera probe reads the camera
//! constant buffer itself -- position, basis and three rows of the projection -- and PAIRS two views' uploads made
//! within a few milliseconds and a plausible eye separation of each other: one view's constants read alone look
//! plausible almost always; two read next to each other show which field the second eye is missing.
//!
//! Every read of a block goes through an SEH guard: the pointer comes from an upload heap the engine may already have
//! recycled, and a probe that crashes the game teaches nothing.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use cvr_body::cmath;
use std::ffi::CString;
use std::sync::Mutex;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_CbvDumpNode: AtomicU32;
    static CyberpunkVR_CamPairMaxMs: AtomicU32;
    static CyberpunkVR_DebugCamPairs: AtomicU64;
    static CyberpunkVR_RuntimeDiagnostics: AtomicI32;
    /// Camera buffers big enough to be one (counted by the capture's descriptor hook).
    static g_cc_big: AtomicU64;

    /// `cvr::log("%s", text)`: the stereo module's switch and rate cap, "[stereo] " before and a newline after.
    fn cvr_cpp_stereo_log(text: *const c_char);
}
#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

fn log(text: &str) {
    if let Ok(text) = CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
/// One `%<spec>` of a float as the C runtime prints it (floats reach printf as doubles).
fn f(spec: &CStr, v: f32) -> String {
    cvr_crt::format(spec, &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}

unsafe extern "C" fn copy_bytes(dst: *mut c_void, src: *mut c_void, n: *mut c_void) -> bool {
    core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, n as usize);
    true
}
/// `n` bytes from engine memory, false when the read faults.
unsafe fn guarded_copy(dst: *mut u8, src: *const u8, n: usize) -> bool {
    cvr_engine::code::guarded_call3(
        copy_bytes as *const () as usize,
        dst.cast(),
        src as *mut c_void,
        n as *mut c_void,
    ) == Some(true)
}

/// A 256-byte block's head: two small positive floats and a whole count below 1024 at +12, the shape of a view-constant
/// block. Read under SEH.
///
/// # Safety
/// `xy` points to two floats and `w` to one u32.
#[no_mangle]
pub unsafe extern "C" fn cvr_cbv_read_head(p: *const u8, xy: *mut f32, w: *mut u32) -> bool {
    let mut head = [0u8; 16];
    if !guarded_copy(head.as_mut_ptr(), p, 16) {
        return false;
    }
    let x = f32::from_le_bytes(head[0..4].try_into().unwrap());
    let y = f32::from_le_bytes(head[4..8].try_into().unwrap());
    *xy = x;
    *xy.add(1) = y;
    let fw = f32::from_le_bytes(head[12..16].try_into().unwrap());
    if !(fw >= 0.0 && fw < 1024.0) {
        return false;
    }
    *w = fw as u32;
    x > 0.0 && x < 0.01 && y > 0.0 && y < 0.01
}

#[derive(Clone, Copy)]
struct CbvSeen {
    node_rva: u32,
    count: [u32; 2],
    hits: [u32; 2],
}
struct Cbv {
    seen: [CbvSeen; 48],
    n: usize,
}
static CBV: Mutex<Cbv> = Mutex::new(Cbv {
    seen: [CbvSeen {
        node_rva: 0,
        count: [0; 2],
        hits: [0; 2],
    }; 48],
    n: 0,
});
static CBV_LAST: AtomicU64 = AtomicU64::new(0);

/// The peak count field per node and view, reported (at most every 10 s) when a peak rises.
///
/// # Safety
/// Called by the capture's descriptor hook.
#[no_mangle]
pub unsafe extern "C" fn cvr_cbv_probe_note(node_rva: u32, count: u32, vrcam: bool) {
    let dump = {
        let mut c = CBV.lock().unwrap_or_else(|p| p.into_inner());
        let i = match c.seen[..c.n].iter().position(|s| s.node_rva == node_rva) {
            Some(i) => i,
            None => {
                if c.n >= c.seen.len() {
                    return;
                }
                let i = c.n;
                c.seen[i] = CbvSeen {
                    node_rva,
                    count: [0; 2],
                    hits: [0; 2],
                };
                c.n += 1;
                i
            }
        };
        let v = vrcam as usize;
        c.seen[i].hits[v] = c.seen[i].hits[v].wrapping_add(1);
        if count > c.seen[i].count[v] {
            c.seen[i].count[v] = count;
            true
        } else {
            false
        }
    };
    if !dump {
        return;
    }
    let now = GetTickCount64();
    let last = CBV_LAST.load(Relaxed);
    if last != 0 && now - last < 10000 {
        return;
    }
    CBV_LAST.store(now, Relaxed);
    // The C++ wrote into 1100 bytes while fewer than 1060 were used.
    let c = CBV.lock().unwrap_or_else(|p| p.into_inner());
    let mut line = String::new();
    for s in c.seen[..c.n].iter() {
        if line.len() >= 1100 - 40 {
            break;
        }
        line.push_str(&format!(
            "{:X}:max m{}/v{} (n {}/{}) ",
            s.node_rva, s.count[0], s.count[1], s.hits[0], s.hits[1]
        ));
        line.truncate(1099);
    }
    log(&format!(
        "[cbv] 256B view-constant blocks, peak count field per node: {line}"
    ));
}

struct CbvDump {
    block: [[u8; 256]; 2],
    have: [bool; 2],
}
static CBV_DUMP: Mutex<CbvDump> = Mutex::new(CbvDump {
    block: [[0; 256]; 2],
    have: [false; 2],
});

/// One node's block (CyberpunkVR_CbvDumpNode), both views, in full: the first block per view, printed once both are
/// in.
///
/// # Safety
/// Called by the capture's descriptor hook with a block's CPU address.
#[no_mangle]
pub unsafe extern "C" fn cvr_cbv_dump_note(p: *const u8, vrcam: bool) {
    let mut tmp = [0u8; 256];
    if !guarded_copy(tmp.as_mut_ptr(), p, 256) {
        return;
    }
    let blocks = {
        let mut d = CBV_DUMP.lock().unwrap_or_else(|p| p.into_inner());
        let v = vrcam as usize;
        if d.have[v] {
            return; // the first block per view is enough
        }
        d.block[v] = tmp;
        d.have[v] = true;
        if !(d.have[0] && d.have[1]) {
            return;
        }
        d.block
    };
    let g = |v: f32| f(c"%.5g", v);
    let row = |b: &[u8; 256], r: usize| -> [f32; 4] {
        core::array::from_fn(|k| {
            f32::from_le_bytes(b[r * 16 + k * 4..r * 16 + k * 4 + 4].try_into().unwrap())
        })
    };
    for half in 0..2 {
        let mut line = String::new();
        for r in half * 8..half * 8 + 8 {
            if line.len() >= 1400 - 170 {
                continue;
            }
            let (m, v) = (row(&blocks[0], r), row(&blocks[1], r));
            line.push_str(&format!(
                "[{r}] M({} {} {} {}) V({} {} {} {})  ",
                g(m[0]),
                g(m[1]),
                g(m[2]),
                g(m[3]),
                g(v[0]),
                g(v[1]),
                g(v[2]),
                g(v[3])
            ));
            line.truncate(1399);
        }
        log(&format!(
            "[cbvdump] node {:X} regs {}-{}: {line}",
            CyberpunkVR_CbvDumpNode.load(Relaxed),
            half * 8,
            half * 8 + 7
        ));
    }
}

// Counters per stage: a probe that finds nothing must be able to say WHERE it stopped.
static CC_CPU: AtomicU64 = AtomicU64::new(0);
static CC_BASIS: AtomicU64 = AtomicU64::new(0);
static CC_POS: AtomicU64 = AtomicU64::new(0);
static CC_RES: AtomicU64 = AtomicU64::new(0);
static STAGES_LAST: AtomicU64 = AtomicU64::new(0);

/// The camera probe's stage counters, at most every 10 s.
///
/// # Safety
/// Called by the capture's descriptor hook.
#[no_mangle]
pub unsafe extern "C" fn cvr_camcb_stages() {
    let now = GetTickCount64();
    let last = STAGES_LAST.load(Relaxed);
    if last != 0 && now - last < 10000 {
        return;
    }
    STAGES_LAST.store(now, Relaxed);
    log(&format!(
        "[camcb] stages: big={} cpu={} basis={} pos={} accepted={} pairs={}",
        g_cc_big.load(SeqCst),
        CC_CPU.load(SeqCst),
        CC_BASIS.load(SeqCst),
        CC_POS.load(SeqCst),
        CC_RES.load(SeqCst),
        CyberpunkVR_DebugCamPairs.load(Relaxed)
    ));
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// One view's projection, reduced to the four numbers that can differ and be seen. The view-projection acts on
/// camera-relative coordinates, so clip.x = dot(row28, ip): for a plain symmetric perspective row28 has NO component
/// along forward, and one IS the frustum's horizontal off-centre.
fn camcb_proj(tag: &str, proj: &[f32; 9], basis: &[f32; 9]) {
    let (r28, r29, fwd) = (&proj[0..3], &proj[3..6], &basis[3..6]);
    let sx = (r28[0] * r28[0] + r28[1] * r28[1] + r28[2] * r28[2]).sqrt();
    let sy = (r29[0] * r29[0] + r29[1] * r29[1] + r29[2] * r29[2]).sqrt();
    let cx = dot(r28, fwd);
    let cy = dot(r29, fwd);
    let fov = |s: f32| {
        if s > 1e-6 {
            2.0 * cmath::atan(1.0 / s) * 57.29578
        } else {
            0.0
        }
    };
    log(&format!(
        "[campair] {tag} proj: sx={} sy={}  hfov={} vfov={} deg  offCentre x={} y={}",
        f(c"%.6f", sx),
        f(c"%.6f", sy),
        f(c"%.4f", fov(sx)),
        f(c"%.4f", fov(sy)),
        f(c"%+.6f", cx),
        f(c"%+.6f", cy)
    ));
}

static PAIR_LAST: AtomicU64 = AtomicU64::new(0);
/// `a` the earlier camera, `b` the later one, both from the same frame.
fn camcb_pair(
    ap: &[f32; 3],
    ab: &[f32; 9],
    bp: &[f32; 3],
    bb: &[f32; 9],
    aproj: &[f32; 9],
    bproj: &[f32; 9],
) {
    // SAFETY: plain Win32 call.
    let now = unsafe { GetTickCount64() };
    let last = PAIR_LAST.load(Relaxed);
    if last != 0 && now - last < 3000 {
        return;
    }
    PAIR_LAST.store(now, Relaxed);
    let d = [bp[0] - ap[0], bp[1] - ap[1], bp[2] - ap[2]];
    // In the FIRST camera's own basis (rows 40/41/42 are right / forward / up): a correct eye separation is (+-IPD,
    // 0, 0), anything in the forward or up column a stereo bug independent of the sight.
    let dr = d[0] * ab[0] + d[1] * ab[1] + d[2] * ab[2];
    let df = d[0] * ab[3] + d[1] * ab[4] + d[2] * ab[5];
    let du = d[0] * ab[6] + d[1] * ab[7] + d[2] * ab[8];
    let p5 = |v: f32| f(c"%.5f", v);
    let s6 = |v: f32| f(c"%+.6f", v);
    log(&format!(
        "[campair] A=({} {} {})  B=({} {} {})",
        p5(ap[0]),
        p5(ap[1]),
        p5(ap[2]),
        p5(bp[0]),
        p5(bp[1]),
        p5(bp[2])
    ));
    log(&format!(
        "[campair] delta=({} {} {}) |d|={} m  ->  right={} forward={} up={}",
        p5(d[0]),
        p5(d[1]),
        p5(d[2]),
        f(c"%.4f", (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()),
        f(c"%+.4f", dr),
        f(c"%+.4f", df),
        f(c"%+.4f", du)
    ));
    log(&format!(
        "[campair] A right=({} {} {}) fwd=({} {} {})",
        s6(ab[0]),
        s6(ab[1]),
        s6(ab[2]),
        s6(ab[3]),
        s6(ab[4]),
        s6(ab[5])
    ));
    let ang = |x: &[f32], y: &[f32]| {
        let t = dot(x, y).clamp(-1.0, 1.0);
        cmath::acos(t) * 1000.0 // milliradians
    };
    log(&format!(
        "[campair] axis disagreement: right={} forward={} up={} mrad",
        f(c"%.3f", ang(&ab[0..3], &bb[0..3])),
        f(c"%.3f", ang(&ab[3..6], &bb[3..6])),
        f(c"%.3f", ang(&ab[6..9], &bb[6..9]))
    ));
    camcb_proj("A", aproj, ab);
    camcb_proj("B", bproj, bb);
}

struct Previous {
    pos: [f32; 3],
    basis: [f32; 9],
    proj: [f32; 9],
    tick: u64,
    valid: bool,
}
static PREVIOUS: Mutex<Previous> = Mutex::new(Previous {
    pos: [0.0; 3],
    basis: [0.0; 9],
    proj: [0.0; 9],
    tick: 0,
    valid: false,
});

/// A camera constant buffer (53 float4 rows at `cp`), fingerprinted by four independent structural facts and paired
/// with the previous one when both came within CyberpunkVR_CamPairMaxMs and an eye separation (2..15 cm) of each
/// other. `vrcam` is not consulted: descriptor creation does not happen inside node dispatch, so it cannot say which
/// view a buffer belongs to.
///
/// # Safety
/// Called by the capture's descriptor hook with the buffer's CPU address.
#[no_mangle]
pub unsafe extern "C" fn cvr_camcb_note(cp: *const u8, _vrcam: bool) {
    CC_CPU.fetch_add(1, Relaxed);
    let mut r = [0f32; 53 * 4];
    if !guarded_copy(r.as_mut_ptr().cast(), cp, core::mem::size_of_val(&r)) {
        cvr_camcb_stages();
        return;
    }
    let row = |i: usize| &r[i * 4..i * 4 + 4];
    let (p36, p37, p47) = (row(36), row(37), row(47));
    let (b0, b1, b2) = (row(40), row(41), row(42));
    // An orthonormal 3x3 alone matched 8303 unrelated buffers (object transforms, skinning palettes).
    let unit = |v: &[f32]| {
        let n = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        n > 0.98 && n < 1.02
    };
    let (d01, d02, d12) = (dot(b0, b1), dot(b0, b2), dot(b1, b2));
    if !unit(b0)
        || !unit(b1)
        || !unit(b2)
        || d01.abs() > 0.02
        || d02.abs() > 0.02
        || d12.abs() > 0.02
    {
        cvr_camcb_stages();
        return;
    }
    CC_BASIS.fetch_add(1, Relaxed);
    // Position AND rebase origin, and that they coincide: the engine rebases the world on the camera.
    if p37[3] != 1.0 || p36[3] != 0.0 {
        cvr_camcb_stages();
        return;
    }
    if (p36[0] - p37[0]).abs() > 1e-3
        || (p36[1] - p37[1]).abs() > 1e-3
        || (p36[2] - p37[2]).abs() > 1e-3
    {
        cvr_camcb_stages();
        return;
    }
    if p36[0].abs() + p36[1].abs() + p36[2].abs() < 1.0 {
        cvr_camcb_stages();
        return;
    }
    CC_POS.fetch_add(1, Relaxed);
    // The view's pixel size: whole numbers in range, with .zw exactly (0, 1).
    if p47[2] != 0.0 || p47[3] != 1.0 {
        cvr_camcb_stages();
        return;
    }
    if !(p47[0] >= 256.0 && p47[0] <= 16384.0 && p47[1] >= 256.0 && p47[1] <= 16384.0)
        || p47[0] != p47[0].floor()
        || p47[1] != p47[1].floor()
    {
        cvr_camcb_stages();
        return;
    }
    CC_RES.fetch_add(1, Relaxed);
    // Small auxiliary views (256x256 probes) are not eyes.
    if p47[0] < 1024.0 || p47[1] < 1024.0 {
        cvr_camcb_stages();
        return;
    }
    let mut basis = [0f32; 9];
    basis[0..3].copy_from_slice(&b0[0..3]);
    basis[3..6].copy_from_slice(&b1[0..3]);
    basis[6..9].copy_from_slice(&b2[0..3]);
    let mut proj = [0f32; 9];
    proj[0..3].copy_from_slice(&row(28)[0..3]);
    proj[3..6].copy_from_slice(&row(29)[0..3]);
    proj[6..9].copy_from_slice(&row(31)[0..3]);
    let pos: [f32; 3] = [p36[0], p36[1], p36[2]];
    let mut pair = None;
    {
        let mut prev = PREVIOUS.lock().unwrap_or_else(|p| p.into_inner());
        let now = GetTickCount64();
        if prev.valid && now - prev.tick <= CyberpunkVR_CamPairMaxMs.load(Relaxed) as u64 {
            let (dx, dy, dz) = (
                pos[0] - prev.pos[0],
                pos[1] - prev.pos[1],
                pos[2] - prev.pos[2],
            );
            let dd = (dx * dx + dy * dy + dz * dz).sqrt();
            if dd > 0.02 && dd < 0.15 {
                pair = Some((prev.pos, prev.basis, prev.proj));
                if CyberpunkVR_RuntimeDiagnostics.load(Relaxed) != 0 {
                    CyberpunkVR_DebugCamPairs.fetch_add(1, Relaxed);
                }
            }
        }
        *prev = Previous {
            pos,
            basis,
            proj,
            tick: now,
            valid: true,
        };
    }
    if let Some((ppos, pbasis, pproj)) = pair {
        camcb_pair(&ppos, &pbasis, &pos, &basis, &pproj, &proj);
    }
    cvr_camcb_stages();
}
