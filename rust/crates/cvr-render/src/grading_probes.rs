//! THE FIELD-BY-FIELD PROBES ON THE UPLOADS (part of formerly src/Stereo/Grading.cpp): each compares what the two
//! views upload, so the field that differs is named instead of guessed.
//!
//!   checker   the shadow mask's checkerboard block (56 bytes) diffed whole per view, its phase patch (off: the
//!             pattern is structural), and the small uploads whose two views are >=4x apart, by size and node
//!   sway      the 480-byte frame constants: every differing float with which side advances it, what SHIPPED per
//!             view, and the last uploads' interleaving (view / renderer frame / authored counter)
//!   blk       the generic diff, retargetable live by (node, size), and its inverse census: the sizes a node
//!             uploads, or the nodes that upload a size
//!   casc      the cascade's view-projection per cascade index and view at draw time, and the call stack once
//!   grid      the height mask's grid origin and the LOD-fade phase, each with its spread within a view
//!   wide      every upload >=128 bytes by (node, size) and view, and the temporal field detector over its first 128
//!             floats: fields that advance in both eyes and disagree are the kind worth equalising
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;

use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_CheckerPhaseFix: AtomicI32;
    static CyberpunkVR_CheckerProbe: AtomicI32;
    static CyberpunkVR_CheckerSize: AtomicU32;
    static CyberpunkVR_DebugCheckerFixes: AtomicU64;
    static CyberpunkVR_BlockDiffNode: AtomicU32;
    static CyberpunkVR_BlockDiffSize: AtomicU32;
    fn cvr_cpp_stereo_log(text: *const c_char);
    fn CyberpunkVR_ProfNodeName(rva: u32) -> *const c_char;
}
fn log(text: &str) {
    if let Ok(text) = std::ffi::CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
fn diag(counter: &AtomicU64) {
    // SAFETY: an export of the C++ half.
    if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0 {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
/// Every `period` ms at most (the first call passes).
fn due(last: &AtomicU64, period: u64) -> bool {
    let now = tick();
    let l = last.load(Relaxed);
    if l != 0 && now.wrapping_sub(l) < period {
        return false;
    }
    last.store(now, Relaxed);
    true
}
fn f(spec: &CStr, v: f32) -> String {
    cvr_crt::format(spec, &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
fn f4(v: f32) -> String {
    f(c"%.4f", v)
}
fn node_name(rva: u32, none: &str) -> String {
    // SAFETY: the profiler's name table hands back a static string or null.
    let nm = unsafe { CyberpunkVR_ProfNodeName(rva) };
    if !nm.is_null() {
        // SAFETY: a NUL-terminated static string.
        let s = unsafe { CStr::from_ptr(nm) }.to_string_lossy();
        if !s.is_empty() {
            return s.into_owned();
        }
    }
    if rva != 0 {
        "?".into()
    } else {
        none.into()
    }
}
fn read_floats<const N: usize>(src: *const c_void, bytes: usize) -> Option<[f32; N]> {
    let mut raw = vec![0u8; bytes.min(N * 4)];
    if !code::read_bytes(src as usize, &mut raw) {
        return None;
    }
    let mut out = [0f32; N];
    for (i, c) in raw.chunks_exact(4).enumerate() {
        out[i] = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    }
    Some(out)
}
/// "[i].x" plus whether each side advances it ("+") or holds it ("=").
fn field(i: usize, moved: [bool; 2]) -> String {
    format!(
        "[{}].{}{}{}",
        i / 4,
        ['x', 'y', 'z', 'w'][i % 4],
        if moved[0] { "+" } else { "=" },
        if moved[1] { "+" } else { "=" }
    )
}

// ---- checker ---------------------------------------------------------------------------------------------------

const CHECKER_MAX: usize = 64;
struct Checker {
    /// The second view's copy and MAIN's, so the two can be diffed whole.
    vrcam: [u8; CHECKER_MAX],
    main: [u8; CHECKER_MAX],
    have: bool,
    phase: [u32; 2],
    n: [u64; 2],
    /// Small uploads by (size, node), per view.
    small: Vec<(u32, u32, [u64; 2])>,
}
static CHECKER: Mutex<Checker> = Mutex::new(Checker {
    vrcam: [0; CHECKER_MAX],
    main: [0; CHECKER_MAX],
    have: false,
    phase: [0; 2],
    n: [0; 2],
    small: Vec::new(),
});

/// Every upload under 128 bytes, per view and node: a row where one view does four times the other's work is a pass
/// one eye runs and the other does not.
#[no_mangle]
pub extern "C" fn cvr_grading_checker_small_note(size: u32, node: u32, vrcam: bool) {
    if size == 0 || size >= 128 {
        return;
    }
    let mut c = lock(&CHECKER);
    let i = match c.small.iter().position(|s| s.0 == size && s.1 == node) {
        Some(i) => i,
        None if c.small.len() >= 96 => return,
        None => {
            c.small.push((size, node, [0; 2]));
            c.small.len() - 1
        }
    };
    c.small[i].2[vrcam as usize] += 1;
}
#[no_mangle]
pub extern "C" fn cvr_grading_checker_small_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    let mut line = String::new();
    let c = lock(&CHECKER);
    for (size, node, hits) in &c.small {
        let (lo, hi) = (hits[0].min(hits[1]), hits[0].max(hits[1]));
        if hi < 50 || lo * 4 > hi {
            continue;
        }
        if line.len() >= 700 - 60 {
            break;
        }
        line.push_str(&format!(
            "{size}B@{} M={} V={}  ",
            node_name(*node, "no-node"),
            hits[0],
            hits[1]
        ));
    }
    log(&format!(
        "[checksz] small uploads whose two views are >=4x apart, by size and node: {}",
        if line.is_empty() {
            "(none -- every small block is balanced)"
        } else {
            line.as_str()
        }
    ));
}
fn checker_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    // SAFETY: exports of the C++ half.
    let (size, fixes) = unsafe {
        (
            CyberpunkVR_CheckerSize.load(Relaxed),
            CyberpunkVR_DebugCheckerFixes.load(Relaxed),
        )
    };
    let sz = (size as usize).min(CHECKER_MAX);
    let c = lock(&CHECKER);
    let mut line = String::new();
    let mut n = 0;
    let mut o = 0;
    while o + 4 <= sz {
        if c.vrcam[o..o + 4] != c.main[o..o + 4] {
            n += 1;
            let a = u32::from_le_bytes(c.main[o..o + 4].try_into().unwrap_or_default());
            let b = u32::from_le_bytes(c.vrcam[o..o + 4].try_into().unwrap_or_default());
            if line.len() < 600 - 70 {
                line.push_str(&format!(
                    "+{o} M={a}/{} V={b}/{}  ",
                    f4(f32::from_bits(a)),
                    f4(f32::from_bits(b))
                ));
            }
        }
        o += 4;
    }
    log(&format!(
        "[checker] {size}-byte block: MAIN n={} VRCAM n={} | [3].y M={} V={} | differing dwords ({n}, as uint/float): {} | fixes={fixes}",
        c.n[0],
        c.n[1],
        c.phase[0],
        c.phase[1],
        if n != 0 { line.as_str() } else { "(none -- the two views' copies are identical)" }
    ));
}
/// The block noted per view; MAIN's given the second view's phase ([3].y) when the fix is on. True with `dst`
/// filled, false to ship the engine's own.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_checker_note(
    src: *const c_void,
    dst: *mut c_void,
    vrcam: bool,
    size: u32,
) -> bool {
    let n = (size as usize).min(CHECKER_MAX);
    let mut tmp = [0u8; CHECKER_MAX];
    if !code::read_bytes(src as usize, &mut tmp[..n]) {
        return false;
    }
    let phase = if n >= 56 {
        u32::from_le_bytes(tmp[52..56].try_into().unwrap_or_default())
    } else {
        0
    };
    let mut patched = false;
    {
        let mut c = lock(&CHECKER);
        if vrcam {
            c.phase[1] = phase;
            c.n[1] += 1;
            c.vrcam[..n].copy_from_slice(&tmp[..n]);
            c.have = true;
        } else {
            c.phase[0] = phase;
            c.n[0] += 1;
            c.main[..n].copy_from_slice(&tmp[..n]);
            if CyberpunkVR_CheckerPhaseFix.load(Relaxed) != 0 && c.have && n >= 56 {
                let out = core::slice::from_raw_parts_mut(dst.cast::<u8>(), n);
                out.copy_from_slice(&tmp[..n]);
                out[52..56].copy_from_slice(&c.vrcam[52..56]);
                patched = true;
            }
        }
    }
    if patched {
        diag(&CyberpunkVR_DebugCheckerFixes);
    }
    if CyberpunkVR_CheckerProbe.load(Relaxed) != 0 {
        checker_report();
    }
    patched
}

// ---- sway ------------------------------------------------------------------------------------------------------

const SWAY_FLOATS: usize = 120;
struct SwayDiff {
    last: [[f32; SWAY_FLOATS]; 2],
    moved: [[u32; SWAY_FLOATS]; 2],
    seen: [u64; 2],
}
static SWAY_DIFF: Mutex<SwayDiff> = Mutex::new(SwayDiff {
    last: [[0.0; SWAY_FLOATS]; 2],
    moved: [[0; SWAY_FLOATS]; 2],
    seen: [0; 2],
});
fn sway_diff_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    let s = lock(&SWAY_DIFF);
    if s.seen[0] < 5 || s.seen[1] < 5 {
        return;
    }
    let mut line = String::new();
    let mut n = 0;
    for i in 0..SWAY_FLOATS {
        if s.last[0][i].to_bits() == s.last[1][i].to_bits() {
            continue;
        }
        n += 1;
        let moved = [
            s.moved[0][i] as u64 * 4 >= s.seen[0],
            s.moved[1][i] as u64 * 4 >= s.seen[1],
        ];
        if line.len() < 1500 - 70 {
            line.push_str(&format!(
                "{} M={} V={}  ",
                field(i, moved),
                f4(s.last[0][i]),
                f4(s.last[1][i])
            ));
        }
    }
    let (dm, dv) = (s.last[0][113].to_bits(), s.last[1][113].to_bits());
    log(&format!(
        "[swaydiff] 480B block, EVERY differing float ({n} of 120; + = advances for that view, = = static): {} || dither index [28].y as uint: MAIN {dm} (&63 = {}) VRCAM {dv} (&63 = {})",
        if n != 0 { line.as_str() } else { "(none)" },
        dm & 63,
        dv & 63
    ));
}
#[no_mangle]
pub extern "C" fn cvr_grading_sway_diff_note(src: *const c_void, vrcam: bool) {
    let Some(cur) = read_floats::<SWAY_FLOATS>(src, SWAY_FLOATS * 4) else {
        return;
    };
    {
        let mut s = lock(&SWAY_DIFF);
        let v = vrcam as usize;
        if s.seen[v] != 0 {
            for i in 0..SWAY_FLOATS {
                if s.last[v][i].to_bits() != cur[i].to_bits() {
                    s.moved[v][i] += 1;
                }
            }
        }
        s.last[v] = cur;
        s.seen[v] += 1;
    }
    sway_diff_report();
}
/// What SHIPPED per view (the diff reads the engine's block before the patch), and the last ten uploads verbatim.
struct SwayShip {
    shipped: [u32; 2],
    n: [u64; 2],
    order: [(u32, u32, u8); 10],
    order_n: u32,
}
static SWAY_SHIP: Mutex<SwayShip> = Mutex::new(SwayShip {
    shipped: [0; 2],
    n: [0; 2],
    order: [(0, 0, 0); 10],
    order_n: 0,
});
#[no_mangle]
pub extern "C" fn cvr_grading_sway_shipped_note(blk: *const c_void, vrcam: bool) {
    let Some(v) = code::read::<u32>(blk as usize + 113 * 4) else {
        return;
    };
    let mut s = lock(&SWAY_SHIP);
    s.shipped[vrcam as usize] = v;
    s.n[vrcam as usize] += 1;
}
#[no_mangle]
pub extern "C" fn cvr_grading_sway_order_note(vrcam: bool, frame: u32, authored: u32) {
    let mut s = lock(&SWAY_SHIP);
    let i = (s.order_n % 10) as usize;
    s.order[i] = (frame, authored, vrcam as u8);
    s.order_n = s.order_n.wrapping_add(1);
}
#[no_mangle]
pub extern "C" fn cvr_grading_sway_shipped_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    let s = lock(&SWAY_SHIP);
    let mut order = String::new();
    let n = s.order_n;
    for i in n.saturating_sub(10)..n {
        let (frame, authored, view) = s.order[(i % 10) as usize];
        if order.len() < 420 - 40 {
            order.push_str(&format!(
                "{}/f{frame}/c{authored} ",
                if view != 0 { "V" } else { "M" }
            ));
        }
    }
    log(&format!(
        "[swayship] dither index [28].y as SHIPPED: MAIN {} (&63 = {}, n={}) | VRCAM {} (&63 = {}, n={}) | equal={} || last uploads (view/frame/authored): {order}",
        s.shipped[0],
        s.shipped[0] & 63,
        s.n[0],
        s.shipped[1],
        s.shipped[1] & 63,
        s.n[1],
        if s.shipped[0] == s.shipped[1] { "YES" } else { "no" }
    ));
}

// ---- blk -------------------------------------------------------------------------------------------------------

const BLK_MAX_FLOATS: usize = 256;
struct Blk {
    last: [[f32; BLK_MAX_FLOATS]; 2],
    moved: [[u32; BLK_MAX_FLOATS]; 2],
    seen: [u64; 2],
    node: u32,
    size: u32,
    keys: Vec<(u32, [u64; 2])>,
    keys_by_node: bool,
}
static BLK: Mutex<Blk> = Mutex::new(Blk {
    last: [[0.0; BLK_MAX_FLOATS]; 2],
    moved: [[0; BLK_MAX_FLOATS]; 2],
    seen: [0; 2],
    node: 0,
    size: 0,
    keys: Vec::new(),
    keys_by_node: false,
});
/// The inverse census: node set -> the sizes it uploads (key = size); node 0 -> the nodes uploading the size.
#[no_mangle]
pub extern "C" fn cvr_grading_blk_key_note(key: u32, vrcam: bool) {
    let mut b = lock(&BLK);
    // SAFETY: an export of the C++ half.
    let by_node = unsafe { CyberpunkVR_BlockDiffNode.load(Relaxed) } == 0;
    if by_node != b.keys_by_node {
        b.keys_by_node = by_node; // the direction changed live: start clean
        b.keys.clear();
    }
    let i = match b.keys.iter().position(|k| k.0 == key) {
        Some(i) => i,
        None if b.keys.len() >= 48 => return,
        None => {
            b.keys.push((key, [0; 2]));
            b.keys.len() - 1
        }
    };
    b.keys[i].1[vrcam as usize] += 1;
}
#[no_mangle]
pub extern "C" fn cvr_grading_blk_keys_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    let b = lock(&BLK);
    let by_node = b.keys_by_node;
    let mut line = String::new();
    for (key, hits) in &b.keys {
        if hits[0] + hits[1] < 20 {
            continue; // transients would bury the per-frame ones
        }
        if line.len() >= 1300 - 70 {
            break;
        }
        if by_node {
            line.push_str(&format!(
                "{} M={} V={}  ",
                node_name(*key, "no-node"),
                hits[0],
                hits[1]
            ));
        } else {
            line.push_str(&format!("{key}B M={} V={}  ", hits[0], hits[1]));
        }
    }
    // SAFETY: exports of the C++ half.
    let (node, size) = unsafe {
        (
            CyberpunkVR_BlockDiffNode.load(Relaxed),
            CyberpunkVR_BlockDiffSize.load(Relaxed),
        )
    };
    if by_node {
        log(&format!(
            "[blkwho] nodes uploading a {size}-byte block, per view: {}",
            if line.is_empty() {
                "(none)"
            } else {
                line.as_str()
            }
        ));
    } else {
        log(&format!(
            "[blkwho] sizes uploaded under node {node:X}, per view: {}",
            if line.is_empty() {
                "(none -- this node uploads no buffers)"
            } else {
                line.as_str()
            }
        ));
    }
}
fn blk_report(nfloat: usize) {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    // SAFETY: exports of the C++ half.
    let (node, size) = unsafe {
        (
            CyberpunkVR_BlockDiffNode.load(Relaxed),
            CyberpunkVR_BlockDiffSize.load(Relaxed),
        )
    };
    let b = lock(&BLK);
    if b.seen[0] < 5 || b.seen[1] < 5 {
        log(&format!(
            "[blkdiff] node={node:X} size={size}: MAIN n={} VRCAM n={} -- one side is not uploading this block, so there is nothing to compare",
            b.seen[0], b.seen[1]
        ));
        return;
    }
    let mut line = String::new();
    let mut n = 0;
    for i in 0..nfloat {
        if b.last[0][i].to_bits() == b.last[1][i].to_bits() {
            continue;
        }
        n += 1;
        let moved = [
            b.moved[0][i] as u64 * 4 >= b.seen[0],
            b.moved[1][i] as u64 * 4 >= b.seen[1],
        ];
        if line.len() < 1500 - 70 {
            line.push_str(&format!(
                "{} M={} V={}  ",
                field(i, moved),
                f4(b.last[0][i]),
                f4(b.last[1][i])
            ));
        }
    }
    let (dm, dv) = if nfloat > 113 {
        (b.last[0][113].to_bits(), b.last[1][113].to_bits())
    } else {
        (0, 0)
    };
    let mut grid = String::new();
    if nfloat > 146 {
        let snap = |v: f32| (v * 21.333_333).floor() as i32;
        let (m, v) = (&b.last[0], &b.last[1]);
        grid = format!(
            " || [36].xyz M=({}, {}, {}) V=({}, {}, {}) snapped M=({}, {}, {}) V=({}, {}, {})",
            f4(m[144]),
            f4(m[145]),
            f4(m[146]),
            f4(v[144]),
            f4(v[145]),
            f4(v[146]),
            snap(m[144]),
            snap(m[145]),
            snap(m[146]),
            snap(v[144]),
            snap(v[145]),
            snap(v[146])
        );
    }
    log(&format!(
        "[blkdiff] node={node:X} size={size}, differing floats ({n} of {nfloat}; + = advances): {} || dither [28].y M={dm} (&63={}) V={dv} (&63={}){grid}",
        if n != 0 { line.as_str() } else { "(none)" },
        dm & 63,
        dv & 63
    ));
}
/// One upload of the aimed (node, size) per view; retargeting live resets the tables (two blocks never share a
/// column).
#[no_mangle]
pub extern "C" fn cvr_grading_blk_note(src: *const c_void, size: u32, side: i32) {
    if side < 0 {
        return; // neither eye: a probe face, not a comparison
    }
    let bytes = (size as usize).min(BLK_MAX_FLOATS * 4);
    let Some(cur) = read_floats::<BLK_MAX_FLOATS>(src, bytes) else {
        return;
    };
    {
        let mut b = lock(&BLK);
        // SAFETY: exports of the C++ half.
        let (node, sz) = unsafe {
            (
                CyberpunkVR_BlockDiffNode.load(Relaxed),
                CyberpunkVR_BlockDiffSize.load(Relaxed),
            )
        };
        if b.node != node || b.size != sz {
            b.node = node;
            b.size = sz;
            b.last = [[0.0; BLK_MAX_FLOATS]; 2];
            b.moved = [[0; BLK_MAX_FLOATS]; 2];
            b.seen = [0; 2];
        }
        let v = (side != 0) as usize;
        if b.seen[v] != 0 {
            for i in 0..bytes / 4 {
                if b.last[v][i].to_bits() != cur[i].to_bits() {
                    b.moved[v][i] += 1;
                }
            }
        }
        b.last[v][..bytes / 4].copy_from_slice(&cur[..bytes / 4]);
        b.seen[v] += 1;
    }
    blk_report(bytes / 4);
}

// ---- casc ------------------------------------------------------------------------------------------------------

/// b1[0..3], the matrix the cascade's vertex shader transforms with, per view and cascade index.
static CASC: Mutex<[[([f32; 16], u64); 8]; 2]> = Mutex::new([[([0.0; 16], 0); 8]; 2]);
fn casc_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    let c = lock(&CASC);
    let mut line = String::new();
    for idx in 0..8 {
        let ((a, sa), (b, sb)) = (c[0][idx], c[1][idx]);
        if sa == 0 || sb == 0 {
            continue;
        }
        let (mut ndiff, mut worst) = (0, 0f32);
        for i in 0..16 {
            let d = (a[i] - b[i]).abs();
            if d != 0.0 {
                ndiff += 1;
            }
            if d > worst {
                worst = d;
            }
        }
        if line.len() < 1400 - 130 {
            line.push_str(&format!(
                "casc{idx}: {ndiff}/16 differ, worst {} | translation M=({}, {}, {}) V=({}, {}, {})  ",
                f(c"%.6g", worst),
                f4(a[3]),
                f4(a[7]),
                f4(a[11]),
                f4(b[3]),
                f4(b[7]),
                f4(b[11])
            ));
        }
    }
    log(&format!(
        "[cascmat] cascade b1[0..3] at draw time, keyed by cascade index: {}",
        if line.is_empty() {
            "(no cascade index seen on both sides yet)"
        } else {
            line.as_str()
        }
    ));
}
#[no_mangle]
pub extern "C" fn cvr_grading_casc_note(src: *const c_void, vrcam: bool, idx: i32) {
    if !(0..8).contains(&idx) {
        return;
    }
    let Some(m) = read_floats::<16>(src, 64) else {
        return;
    };
    {
        let mut c = lock(&CASC);
        let slot = &mut c[vrcam as usize][idx as usize];
        slot.0 = m;
        slot.1 += 1;
    }
    casc_report();
}
/// Once: the game-module frames (as RVAs) at the cascade's 848-byte upload -- they name the block's builder.
#[no_mangle]
pub extern "C" fn cvr_grading_casc_stack_report(base: usize) {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, SeqCst) {
        return;
    }
    let mut frames = [core::ptr::null_mut::<c_void>(); 24];
    // SAFETY: a plain stack capture into our buffer.
    let n = unsafe {
        windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(0, &mut frames, None)
    };
    let mut line = String::new();
    for a in frames[..n as usize].iter().map(|p| *p as usize) {
        if a < base || a - base > 0x500_0000 {
            continue; // our frames and the CRT's say nothing about the engine
        }
        if line.len() < 900 - 16 {
            line.push_str(&format!("{:X} ", (a - base) as u32));
        }
    }
    log(&format!(
        "[cascstack] engine frames (RVA) at the cascade's 848-byte upload: {}",
        if line.is_empty() {
            "(none in the game module)"
        } else {
            line.as_str()
        }
    ));
}

// ---- grid ------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct GridOrigin {
    v: [f32; 3],
    lo: [f32; 3],
    hi: [f32; 3],
    dither: u32,
    dither_lo: u32,
    dither_hi: u32,
    seen: u64,
}
const GRID_ZERO: GridOrigin = GridOrigin {
    v: [0.0; 3],
    lo: [0.0; 3],
    hi: [0.0; 3],
    dither: 0,
    dither_lo: 0,
    dither_hi: 0,
    seen: 0,
};
static GRID: Mutex<[GridOrigin; 2]> = Mutex::new([GRID_ZERO; 2]);
fn grid_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !due(&LAST, 5000) {
        return;
    }
    let mut g = lock(&GRID);
    if g[0].seen == 0 || g[1].seen == 0 {
        return;
    }
    let snap = |v: &[f32; 3]| v.map(|x| (x * 21.333_333).floor() as i32);
    let (sm, sv) = (snap(&g[0].v), snap(&g[1].v));
    let spread = |o: &GridOrigin| {
        (0..3)
            .map(|i| f(c"%.3f", o.hi[i] - o.lo[i]))
            .collect::<Vec<_>>()
            .join(", ")
    };
    log(&format!(
        "[grid] cascade b1: [36].xyz MAIN snapped ({}, {}, {}) spread ({}) | VRCAM snapped ({}, {}, {}) spread ({}) | same cell={} || LOD dither [51].z MAIN {} (&3={}, {} distinct values seen) | VRCAM {} (&3={}, {} distinct) | equal={} || n M={} V={}",
        sm[0],
        sm[1],
        sm[2],
        spread(&g[0]),
        sv[0],
        sv[1],
        sv[2],
        spread(&g[1]),
        if sm == sv { "YES" } else { "no" },
        g[0].dither,
        g[0].dither & 3,
        g[0].dither_hi.wrapping_sub(g[0].dither_lo).wrapping_add(1),
        g[1].dither,
        g[1].dither & 3,
        g[1].dither_hi.wrapping_sub(g[1].dither_lo).wrapping_add(1),
        if g[0].dither & 3 == g[1].dither & 3 { "YES" } else { "no" },
        g[0].seen,
        g[1].seen
    ));
    // Per interval: a lifetime spread would just accumulate the player walking around.
    *g = [GRID_ZERO; 2];
}
#[no_mangle]
pub extern "C" fn cvr_grading_grid_note(src: *const c_void, vrcam: bool) {
    let Some(v) = read_floats::<3>((src as usize + 144 * 4) as *const c_void, 12) else {
        return;
    };
    let Some(d) = code::read::<u32>(src as usize + 206 * 4) else {
        return;
    };
    {
        let mut all = lock(&GRID);
        let g = &mut all[vrcam as usize];
        if g.seen == 0 {
            g.lo = v;
            g.hi = v;
            g.dither_lo = d;
            g.dither_hi = d;
        } else {
            for i in 0..3 {
                if v[i] < g.lo[i] {
                    g.lo[i] = v[i];
                }
                if v[i] > g.hi[i] {
                    g.hi[i] = v[i];
                }
            }
            g.dither_lo = g.dither_lo.min(d);
            g.dither_hi = g.dither_hi.max(d);
        }
        g.v = v;
        g.dither = d;
        g.seen += 1;
    }
    grid_report();
}

// ---- the wide census and the temporal field detector -----------------------------------------------------------

const WIDE_HEAD: usize = 64;
/// The temporal detector's window: the first 128 floats of each block.
const T_FIELDS: usize = 128;
const RENDERER_GLOBAL_RVA: usize = 0x3BB_CAE8;
struct WideRow {
    node: u32,
    size: u32,
    hits: [u64; 2],
    head: [[u8; WIDE_HEAD]; 2],
    seen: [bool; 2],
    diffs: u64,
    last: [[f32; T_FIELDS]; 2],
    moved: [[u32; T_FIELDS]; 2],
    vdiff: [u32; T_FIELDS],
    samples: [u32; 2],
    /// How many times a view uploads this block in one frame: a row is compared only when both upload it once.
    last_frame: [u32; 2],
    in_frame: [u32; 2],
    max_per_frame: [u32; 2],
}
static WIDE: Mutex<Vec<Box<WideRow>>> = Mutex::new(Vec::new());
extern "C" {
    static CyberpunkVR_WideCensus: AtomicI32;
    static CyberpunkVR_TemporalScan: AtomicI32;
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
/// Every upload of 128 bytes or more, by (node, size) and view: how often, whether the heads ever differ, and per
/// field of the first 128 floats how often it moves in each view and how often the views disagree.
#[no_mangle]
pub extern "C" fn cvr_grading_wide_note(
    src: *const c_void,
    size: u32,
    side: i32,
    node: u32,
    base: usize,
) {
    // SAFETY: exports of the C++ half.
    let (census, temporal) = unsafe {
        (
            CyberpunkVR_WideCensus.load(Relaxed),
            CyberpunkVR_TemporalScan.load(Relaxed),
        )
    };
    if !diagnostics() || census == 0 || src.is_null() || size < 128 || side < 0 {
        return;
    }
    let mut head = [0u8; WIDE_HEAD];
    if !code::read_bytes(src as usize, &mut head) {
        return;
    }
    let v = side as usize;
    let frame_id = if base != 0 {
        code::read::<usize>(base + RENDERER_GLOBAL_RVA)
            .filter(|r| *r != 0)
            .and_then(|r| code::read::<u32>(r + 0x4CC8))
            .unwrap_or(0)
    } else {
        0
    };
    let nf = (size as usize / 4).min(T_FIELDS);
    let cur = if temporal != 0 {
        read_floats::<T_FIELDS>(src, nf * 4)
    } else {
        None
    };
    let mut rows = lock(&WIDE);
    let i = match rows.iter().position(|r| r.node == node && r.size == size) {
        Some(i) => i,
        None if rows.len() >= 192 => return,
        None => {
            rows.push(Box::new(WideRow {
                node,
                size,
                hits: [0; 2],
                head: [[0; WIDE_HEAD]; 2],
                seen: [false; 2],
                diffs: 0,
                last: [[0.0; T_FIELDS]; 2],
                moved: [[0; T_FIELDS]; 2],
                vdiff: [0; T_FIELDS],
                samples: [0; 2],
                last_frame: [0; 2],
                in_frame: [0; 2],
                max_per_frame: [0; 2],
            }));
            rows.len() - 1
        }
    };
    let r = &mut rows[i];
    r.hits[v] += 1;
    r.head[v] = head;
    r.seen[v] = true;
    if r.seen[0] && r.seen[1] && r.head[0] != r.head[1] {
        r.diffs += 1;
    }
    let Some(cur) = cur else { return };
    if r.samples[v] != 0 {
        for k in 0..nf {
            if r.last[v][k].to_bits() != cur[k].to_bits() {
                r.moved[v][k] += 1;
            }
        }
    }
    r.last[v][..nf].copy_from_slice(&cur[..nf]);
    r.samples[v] += 1;
    if frame_id != r.last_frame[v] {
        r.last_frame[v] = frame_id;
        r.in_frame[v] = 1;
    } else {
        r.in_frame[v] += 1;
    }
    if r.in_frame[v] > r.max_per_frame[v] {
        r.max_per_frame[v] = r.in_frame[v];
    }
    if r.samples[0] != 0 && r.samples[1] != 0 {
        for k in 0..nf {
            if r.last[0][k].to_bits() != r.last[1][k].to_bits() {
                r.vdiff[k] += 1;
            }
        }
    }
}
/// The fields that advance in BOTH eyes and disagree by >=20% (pose-derived fields disagree by ~1e-4): temporal
/// fields, the kind worth equalising. Rows uploaded more than once per view per frame are skipped and counted.
#[no_mangle]
pub extern "C" fn cvr_grading_temporal_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !diagnostics() || !due(&LAST, 6000) {
        return;
    }
    let (mut reported, mut dropped, mut multi) = (0u32, 0u32, 0u32);
    let mut rows = lock(&WIDE);
    for r in rows.iter_mut() {
        if r.samples[0] < 16 || r.samples[1] < 16 {
            continue;
        }
        if r.max_per_frame[0] != 1 || r.max_per_frame[1] != 1 {
            multi += 1;
            continue;
        }
        let nf = (r.size as usize / 4).min(T_FIELDS);
        let mut fields = String::new();
        let (mut nfound, mut shown) = (0u32, 0u32);
        for k in 0..nf {
            if (r.moved[0][k] as u64) * 4 < r.samples[0] as u64
                || (r.moved[1][k] as u64) * 4 < r.samples[1] as u64
            {
                continue;
            }
            if (r.vdiff[k] as u64) * 2 < r.samples[0] as u64 {
                continue;
            }
            let (a, b) = (r.last[0][k], r.last[1][k]);
            let (aa, bb) = (a.abs(), b.abs());
            let big = if aa > bb { aa } else { bb };
            if big < 1e-3 || (a - b).abs() < 0.2 * big {
                continue;
            }
            nfound += 1;
            if fields.len() < 1400 - 44 {
                fields.push_str(&format!(
                    "[{}].{}={}/{} ",
                    k / 4,
                    ['x', 'y', 'z', 'w'][k % 4],
                    f(c"%.3f", a),
                    f(c"%.3f", b)
                ));
                shown += 1;
            }
        }
        if nfound == 0 {
            continue;
        }
        reported += 1;
        dropped += nfound - shown;
        log(&format!(
            "[temporal] {}B@{} ({nfound} field{}, M/V): {fields}{}",
            r.size,
            node_name(r.node, "no-node"),
            if nfound == 1 { "" } else { "s" },
            if nfound > shown { "... TRUNCATED" } else { "" }
        ));
        r.moved = [[0; T_FIELDS]; 2];
        r.vdiff = [0; T_FIELDS];
        r.samples = [0; 2];
        r.max_per_frame = [0; 2];
    }
    log(&format!(
        "[temporal] --- {reported} block{} with fields that advance in both eyes and disagree by >=20% (pose-derived fields disagree by ~1e-4), window = first {T_FIELDS} floats{}",
        if reported == 1 { "" } else { "s" },
        if dropped != 0 { " -- SOME FIELDS TRUNCATED, see the rows above" } else { "" }
    ));
    if multi != 0 {
        log(&format!(
            "[temporal] {multi} block(s) skipped: uploaded more than once per view per frame (cubemap faces, cascades), so comparing each side's latest sample would compare different sub-views"
        ));
    }
}
/// The (node, size) rows both views upload and whose contents differ.
#[no_mangle]
pub extern "C" fn cvr_grading_wide_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    if !diagnostics() || !due(&LAST, 5000) {
        return;
    }
    let rows = lock(&WIDE);
    let mut line = String::new();
    for r in rows.iter() {
        if !r.seen[0] || !r.seen[1] || r.diffs == 0 {
            continue;
        }
        if line.len() >= 1500 - 80 {
            break;
        }
        line.push_str(&format!(
            "{}B@{} M={} V={} d={}  ",
            r.size,
            node_name(r.node, "no-node"),
            r.hits[0],
            r.hits[1],
            r.diffs
        ));
    }
    log(&format!(
        "[wide] uploads >=128 B that BOTH views make and whose contents differ, by (node, size): {}",
        if line.is_empty() { "(none yet)" } else { line.as_str() }
    ));
}
