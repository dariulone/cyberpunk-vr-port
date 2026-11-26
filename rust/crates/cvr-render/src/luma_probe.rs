//! Reading pixels back to settle an argument about brightness (formerly src/Stereo/LumaProbe.cpp; the file keeps the
//! vision dump's switch and slot, and the mirror's calls forward here).
//!
//! The bright/dark alternation was invisible to reasoning: the two frames were structurally identical, the same nodes
//! ran, the same targets were bound. What separated them was a NUMBER, so these probes read the number -- an 8x8 block
//! of the finished image (luma), of the constant buffer that feeds it (cb), and of every large texture the tonemap node
//! reads (ti) -- each split by which of the two output states the frame landed in. A field with a large split between
//! the two states is the culprit; innocent fields sit near zero. The surfaces are R11G11B10 and half-float, hence the
//! decoders: read as bytes they yield numbers that look like data and are not.
//!
//! The vision dump rides along: the same readback machinery pointed at the outline layer, written once to the log
//! folder as a tight w*h RGBA8 raster.
//!
//! The readback slots and what the mirror recorded into them are the C++ half's (src/Stereo/SyncStereo.cpp,
//! src/Stereo/Mirror.cpp); the mirror's thread calls these one at a time.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering::*};
use std::ffi::CString;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

#[allow(non_upper_case_globals)]
extern "C" {
    static g_game_device: *mut c_void;
    static mut g_luma_rb: [*mut c_void; 4];
    static mut g_luma_map: [*mut u8; 4];
    static g_luma_parity: [u32; 4];
    static g_luma_frame: [u32; 4];
    static g_luma_finidx: [u32; 4];
    static mut g_cb_rb: [*mut c_void; 4];
    static mut g_cb_map: [*mut u8; 4];
    static g_cb_reset_pending: AtomicBool;
    static mut g_ti_rb: [*mut c_void; 4];
    static mut g_ti_map: [*mut u8; 4];
    static g_ti_count: [u32; 4];
    static g_ti_src: [[*mut c_void; 24]; 4];
    static g_ti_fmt: [[u32; 24]; 4];
    static g_ti_tag: [[u32; 24]; 4];
    static g_visdump_map: *mut c_void;
    static g_visdump_w: u32;
    static g_visdump_h: u32;
    static g_visdump_pitch: u32;
    static CyberpunkVR_LumaWave: AtomicI32;
    static CyberpunkVR_DebugLumaEvenMilli: AtomicU32;
    static CyberpunkVR_DebugLumaOddMilli: AtomicU32;
    static CyberpunkVR_DebugLumaDeltaMilli: AtomicU32;

    /// `cvr::log("%s", text)`: the stereo module's switch and rate cap, "[stereo] " before and a newline after.
    fn cvr_cpp_stereo_log(text: *const c_char);
}

fn log(text: &str) {
    if let Ok(text) = CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
/// One `%<spec>` of a double as the C runtime prints it.
fn f(spec: &CStr, v: f64) -> String {
    cvr_crt::format(spec, &[cvr_crt::Arg::F(v)])
        .to_string_lossy()
        .into_owned()
}
fn f4(v: f64) -> String {
    f(c"%.4f", v)
}

fn dec_f11(v: u32) -> f32 {
    let (e, m) = ((v >> 6) & 0x1F, v & 0x3F);
    if e == 0 {
        return m as f32 * (1.0 / 64.0) * 0.000_061_035_156_25; // 2^-14
    }
    if e == 31 {
        return 0.0; // inf/nan -> ignore
    }
    (1.0 + m as f32 / 64.0) * 2f32.powi(e as i32 - 15)
}
fn dec_f10(v: u32) -> f32 {
    let (e, m) = ((v >> 5) & 0x1F, v & 0x1F);
    if e == 0 {
        return m as f32 * (1.0 / 32.0) * 0.000_061_035_156_25;
    }
    if e == 31 {
        return 0.0;
    }
    (1.0 + m as f32 / 32.0) * 2f32.powi(e as i32 - 15)
}
fn half(h: u16) -> f32 {
    let (e, m) = (((h >> 10) & 0x1F) as u32, (h & 0x3FF) as u32);
    let f = if e == 0 {
        m as f32 * (1.0 / 1024.0) * 0.000_061_035_156_25
    } else if e == 31 {
        0.0
    } else {
        (1.0 + m as f32 / 1024.0) * 2f32.powi(e as i32 - 15)
    };
    if h & 0x8000 != 0 {
        -f
    } else {
        f
    }
}

/// A persistently mapped READBACK buffer of `width` bytes on the game's device into `rb` / `map` (left as they were on
/// failure).
unsafe fn ensure(rb: *mut *mut c_void, map: *mut *mut u8, width: u64) -> bool {
    if !(*rb).is_null() {
        return !(*map).is_null();
    }
    let Some(device) = ID3D12Device::from_raw_borrowed(&g_game_device) else {
        return false;
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: width,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_UNKNOWN,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut resource: Option<ID3D12Resource> = None;
    if device
        .CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut resource,
        )
        .is_err()
    {
        return false;
    }
    let Some(resource) = resource else {
        return false;
    };
    let mut p: *mut c_void = core::ptr::null_mut();
    if resource.Map(0, None, Some(&mut p)).is_err() || p.is_null() {
        return false; // the resource released as it drops
    }
    *rb = resource.into_raw();
    *map = p as *mut u8;
    true
}

/// The image block's readback for slot `idx` (8 rows at a 256-byte pitch).
///
/// # Safety
/// The mirror's thread, with the game's device live; `idx` < 4.
#[no_mangle]
pub unsafe extern "C" fn cvr_luma_probe_ensure(idx: u32) -> bool {
    let i = idx as usize;
    ensure(&raw mut g_luma_rb[i], &raw mut g_luma_map[i], 2048)
}
/// The constant buffer's readback for slot `idx` (232 dwords and slack).
///
/// # Safety
/// As [`cvr_luma_probe_ensure`].
#[no_mangle]
pub unsafe extern "C" fn cvr_cb_probe_ensure(idx: u32) -> bool {
    let i = idx as usize;
    ensure(&raw mut g_cb_rb[i], &raw mut g_cb_map[i], 1024)
}
/// The tonemap inputs' readback for slot `idx` (24 sections of 2 KB and slack).
///
/// # Safety
/// As [`cvr_luma_probe_ensure`].
#[no_mangle]
pub unsafe extern "C" fn cvr_ti_probe_ensure(idx: u32) -> bool {
    let i = idx as usize;
    ensure(&raw mut g_ti_rb[i], &raw mut g_ti_map[i], 65536)
}

#[derive(Clone, Copy, Default)]
struct FinBucket {
    idx: u32,
    sum: f64,
    n: u32,
}
struct Luma {
    sum: [f64; 2],
    n: [u32; 2],
    dsum: f64,
    dn: u32,
    last: f64,
    last_frame: u32,
    buckets: [FinBucket; 12],
    bucket_frames: u32,
}
static LUMA: Mutex<Luma> = Mutex::new(Luma {
    sum: [0.0; 2],
    n: [0; 2],
    dsum: 0.0,
    dn: 0,
    last: -1.0,
    last_frame: 0xFFFF_FFFF,
    buckets: [FinBucket {
        idx: 0,
        sum: 0.0,
        n: 0,
    }; 12],
    bucket_frames: 0,
});

/// The block's mean luma for slot `idx`, bucketed by the frame's output state and by the fin index it used, with the
/// window's summary logged every 120 frames.
///
/// # Safety
/// The mirror's thread; slot `idx` (< 4) mapped and filled.
#[no_mangle]
pub unsafe extern "C" fn cvr_luma_probe_collect(idx: u32) -> f64 {
    let i = idx as usize;
    let map = (*(&raw const g_luma_map))[i];
    let mut l = 0.0f64;
    for y in 0..8 {
        let row = map.add(y * 256) as *const u32;
        for x in 0..8 {
            let v = *row.add(x);
            l += (0.2126f32 * dec_f11(v & 0x7FF)
                + 0.7152f32 * dec_f11((v >> 11) & 0x7FF)
                + 0.0722f32 * dec_f10((v >> 22) & 0x3FF)) as f64;
        }
    }
    l /= 64.0;
    let frame = g_luma_frame[i];
    if CyberpunkVR_LumaWave.load(Relaxed) > 0 {
        CyberpunkVR_LumaWave.fetch_sub(1, Relaxed); // benign race; diagnostic only
        log(&format!("[lumaw] fr={frame} L={}", f4(l)));
    }
    let mut s = LUMA.lock().unwrap_or_else(|p| p.into_inner());
    let p = (g_luma_parity[i] & 1) as usize;
    s.sum[p] += l;
    s.n[p] += 1;
    // Bucketed by the fin natural index used THAT frame (carried through the readback ring): which physical index is
    // dark (correct) and which bright (wrong), and whether the correct one recurs.
    let fin = g_luma_finidx[i];
    for b in s.buckets.iter_mut() {
        if b.n == 0 || b.idx == fin {
            b.idx = fin;
            b.sum += l;
            b.n += 1;
            break;
        }
    }
    s.bucket_frames += 1;
    if s.bucket_frames >= 240 {
        for b in s.buckets.iter().filter(|b| b.n != 0) {
            log(&format!(
                "[fincorr] finidx={} meanL={} n={}",
                b.idx,
                f4(b.sum / b.n as f64),
                b.n
            ));
        }
        s.buckets = [FinBucket::default(); 12];
        s.bucket_frames = 0;
    }
    if s.last >= 0.0 && frame == s.last_frame.wrapping_add(1) {
        s.dsum += if l > s.last { l - s.last } else { s.last - l };
        s.dn += 1;
    }
    s.last = l;
    s.last_frame = frame;
    if s.n[0] + s.n[1] >= 120 {
        let e = if s.n[0] != 0 {
            s.sum[0] / s.n[0] as f64
        } else {
            0.0
        };
        let o = if s.n[1] != 0 {
            s.sum[1] / s.n[1] as f64
        } else {
            0.0
        };
        let dm = if s.dn != 0 { s.dsum / s.dn as f64 } else { 0.0 };
        CyberpunkVR_DebugLumaEvenMilli.store((e * 1000.0 + 0.5) as u32, Relaxed);
        CyberpunkVR_DebugLumaOddMilli.store((o * 1000.0 + 0.5) as u32, Relaxed);
        CyberpunkVR_DebugLumaDeltaMilli.store((dm * 1000.0 + 0.5) as u32, Relaxed);
        log(&format!(
            "[luma] n={}/{} even={} odd={} dmean={}(n={})",
            s.n[0],
            s.n[1],
            f4(e),
            f4(o),
            f4(dm),
            s.dn
        ));
        s.sum = [0.0; 2];
        s.n = [0; 2];
        s.dsum = 0.0;
        s.dn = 0;
    }
    l
}

#[derive(Clone, Copy)]
struct CbDword {
    v0: u32,
    v1: u32,
    nv: u8,
    last: u8,
    flips: u32,
    agree: u32,
    corr_n: u32,
}
const CB_ZERO: CbDword = CbDword {
    v0: 0,
    v1: 0,
    nv: 0,
    last: 0,
    flips: 0,
    agree: 0,
    corr_n: 0,
};
struct Cb {
    dw: [CbDword; 232],
    frames: u32,
    lmin: f64,
    lmax: f64,
}
static CB: Mutex<Cb> = Mutex::new(Cb {
    dw: [CB_ZERO; 232],
    frames: 0,
    lmin: 1e9,
    lmax: -1e9,
});

/// The raced constant-buffer dword, found by itself: a candidate holds EXACTLY two distinct values over the window and
/// flips between them in sync with the bright/normal luma state; scene-driven fields (matrices, time) take more than
/// two values and disqualify themselves. 232 dwords: 0..211 the tonemap's buffer, 214..220 VRCAM's exposure
/// accumulator (28 bytes at 856), 224..230 MAIN's (at 896).
///
/// # Safety
/// The mirror's thread; slot `idx` (< 4) mapped and filled.
#[no_mangle]
pub unsafe extern "C" fn cvr_cb_probe_collect(idx: u32, l: f64) {
    let mut s = CB.lock().unwrap_or_else(|p| p.into_inner());
    if g_cb_reset_pending.swap(false, AcqRel) {
        // The buffer changed: the window restarts.
        s.dw = [CB_ZERO; 232];
        s.frames = 0;
        s.lmin = 1e9;
        s.lmax = -1e9;
    }
    if l < s.lmin {
        s.lmin = l;
    }
    if l > s.lmax {
        s.lmax = l;
    }
    let flap_active = (s.lmax - s.lmin) > 0.02;
    let bright = flap_active && l > (s.lmin + s.lmax) * 0.5;
    let map = (*(&raw const g_cb_map))[idx as usize];
    let dw = map as *const u32;
    for i in 0..232 {
        let c = &mut s.dw[i];
        let v = *dw.add(i);
        let state: u8;
        if c.nv == 0 {
            c.v0 = v;
            c.nv = 1;
            state = 0;
        } else if v == c.v0 {
            state = 0;
        } else if c.nv == 1 {
            c.v1 = v;
            c.nv = 2;
            state = 1;
        } else if v == c.v1 {
            state = 1;
        } else {
            c.nv = 3; // more than two values: disqualified
            continue;
        }
        if c.nv == 2 {
            if state != c.last {
                c.flips += 1;
            }
            if flap_active {
                if (state != 0) == bright {
                    c.agree += 1;
                }
                c.corr_n += 1;
            }
        }
        c.last = state;
    }
    s.frames += 1;
    if s.frames >= 300 {
        let xv = map.add(856) as *const f32;
        let xm = map.add(896) as *const f32;
        let g = |p: *const f32, k: usize| f(c"%.5g", *p.add(k) as f64);
        log(&format!(
            "[expo] v={} {} {} {} {} {} {} | m={} {} {} {} {} {} {}",
            g(xv, 0),
            g(xv, 1),
            g(xv, 2),
            g(xv, 3),
            g(xv, 4),
            g(xv, 5),
            g(xv, 6),
            g(xm, 0),
            g(xm, 1),
            g(xm, 2),
            g(xm, 3),
            g(xm, 4),
            g(xm, 5),
            g(xm, 6)
        ));
        let (mut b1, mut b2) = (-1i32, -1i32);
        for i in 0..232 {
            let c = &s.dw[i];
            if c.nv != 2 || c.flips < 8 {
                continue;
            }
            if b1 < 0 || c.flips > s.dw[b1 as usize].flips {
                b2 = b1;
                b1 = i as i32;
            } else if b2 < 0 || c.flips > s.dw[b2 as usize].flips {
                b2 = i as i32;
            }
        }
        for i in [b1, b2] {
            if i < 0 {
                continue;
            }
            let c = &s.dw[i as usize];
            let hi = if c.corr_n != 0 && c.agree > c.corr_n / 2 {
                c.agree
            } else {
                c.corr_n - c.agree
            };
            let hit = c.corr_n >= 30 && hi * 10 >= c.corr_n * 9;
            log(&format!(
                "[cbflap] {} off=0x{:03X} v0={:08X}({}) v1={:08X}({}) flips={} sync={}/{}",
                if hit { "HIT " } else { "cand" },
                i * 4,
                c.v0,
                f4(f32::from_bits(c.v0) as f64),
                c.v1,
                f4(f32::from_bits(c.v1) as f64),
                c.flips,
                hi,
                c.corr_n
            ));
        }
        if b1 < 0 {
            log(&format!(
                "[cbflap] window done: no 2-valued dword (flap {})",
                if flap_active { "ACTIVE" } else { "inactive" }
            ));
        }
        s.dw = [CB_ZERO; 232];
        s.frames = 0;
        s.lmin = 1e9;
        s.lmax = -1e9;
    }
}

/// The green channel's mean over an 8x8 block (flap detection needs any monotonic channel).
unsafe fn block_green(base: *const u8, format: u32) -> f64 {
    let mut s = 0.0f64;
    for y in 0..8 {
        let row = base.add(y * 256);
        for x in 0..8 {
            let format = DXGI_FORMAT(format as i32);
            if format == DXGI_FORMAT_R11G11B10_FLOAT {
                s += dec_f11((*(row as *const u32).add(x) >> 11) & 0x7FF) as f64;
            } else if format == DXGI_FORMAT_R10G10B10A2_UNORM {
                s += ((*(row as *const u32).add(x) >> 10) & 0x3FF) as f64 * (1.0 / 1023.0);
            } else if format == DXGI_FORMAT_R16G16B16A16_FLOAT {
                s += half(*(row as *const u16).add(x * 4 + 1)) as f64;
            } else if format == DXGI_FORMAT_R32_FLOAT {
                s += *(row as *const f32).add(x) as f64;
            } else {
                // 8-bit RGBA variants: byte 1 is G.
                s += *row.add(x * 4 + 1) as f64 * (1.0 / 255.0);
            }
        }
    }
    s / 64.0
}

#[derive(Clone, Copy)]
struct Stat {
    res: usize,
    format: u32,
    tag: u32,
    sum: [f64; 2],
    n: [u32; 2],
}
const STAT_ZERO: Stat = Stat {
    res: 0,
    format: 0,
    tag: 0,
    sum: [0.0; 2],
    n: [0; 2],
};
struct Ti {
    stats: [Stat; 32],
    frames: u32,
    lmin: f64,
    lmax: f64,
}
static TI: Mutex<Ti> = Mutex::new(Ti {
    stats: [STAT_ZERO; 32],
    frames: 0,
    lmin: 1e9,
    lmax: -1e9,
});

/// Each tonemap input's block, split by the frame's output state, logged every 300 frames.
///
/// # Safety
/// The mirror's thread; slot `idx` (< 4) mapped and filled.
#[no_mangle]
pub unsafe extern "C" fn cvr_ti_probe_collect(idx: u32, l: f64) {
    let i = idx as usize;
    let mut s = TI.lock().unwrap_or_else(|p| p.into_inner());
    if l < s.lmin {
        s.lmin = l;
    }
    if l > s.lmax {
        s.lmax = l;
    }
    let flap_active = (s.lmax - s.lmin) > 0.02;
    let bright = (flap_active && l > (s.lmin + s.lmax) * 0.5) as usize;
    if flap_active {
        let map = (*(&raw const g_ti_map))[i];
        let count = g_ti_count[i].min(24) as usize;
        for k in 0..count {
            let res = g_ti_src[i][k] as usize;
            if res == 0 {
                continue;
            }
            let g = block_green(map.add(k * 2048), g_ti_fmt[i][k]);
            for t in s.stats.iter_mut() {
                if t.res == res || t.res == 0 {
                    t.res = res;
                    t.format = g_ti_fmt[i][k];
                    t.tag = g_ti_tag[i][k];
                    t.sum[bright] += g;
                    t.n[bright] += 1;
                    break;
                }
            }
        }
    }
    s.frames += 1;
    if s.frames >= 300 {
        for t in s.stats.iter() {
            if t.res == 0 || t.n[0] + t.n[1] < 30 {
                continue;
            }
            let mn = if t.n[0] != 0 {
                t.sum[0] / t.n[0] as f64
            } else {
                0.0
            };
            let mb = if t.n[1] != 0 {
                t.sum[1] / t.n[1] as f64
            } else {
                0.0
            };
            log(&format!(
                "[chain] res={:016X} fmt={} node=0x{:X} normal={}(n={}) bright={}(n={}) split={}",
                t.res,
                t.format,
                t.tag,
                f4(mn),
                t.n[0],
                f4(mb),
                t.n[1],
                f(c"%+.4f", mb - mn)
            ));
        }
        s.stats = [STAT_ZERO; 32];
        s.frames = 0;
        s.lmin = 1e9;
        s.lmax = -1e9;
    }
}

/// The outline layer's one-shot dump: rows written TIGHT (no 256-byte padding), so the file is a plain w*h RGBA8
/// raster in the log folder.
///
/// # Safety
/// The mirror's thread, with the dump's readback mapped.
#[no_mangle]
pub unsafe extern "C" fn cvr_vision_dump_write() {
    let (map, w, h, pitch) = (
        g_visdump_map as *const u8,
        g_visdump_w,
        g_visdump_h,
        g_visdump_pitch,
    );
    if map.is_null() || w == 0 || h == 0 {
        return;
    }
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "cyberpunkvr_vision.raw");
    let shown = path.display().to_string();
    if shown.len() >= 260 {
        return; // the C++ copied it into a MAX_PATH buffer
    }
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(1)
        .open(&path)
    else {
        return;
    };
    let row = (w * 4) as usize;
    for y in 0..h as usize {
        let _ = file.write_all(core::slice::from_raw_parts(
            map.add(y * pitch as usize),
            row,
        ));
    }
    drop(file);
    log(&format!("[vision] dumped {w}x{h} RGBA8 -> {shown}"));
}
