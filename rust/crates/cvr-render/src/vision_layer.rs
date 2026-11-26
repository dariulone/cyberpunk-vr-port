//! The scanner outline, in the second eye: identifying its surface (formerly src/Stereo/VisionLayer.cpp; the file
//! keeps the exported knobs and the recording thread's dispatch and ordinal thread-locals, which the capture and the
//! census write, and its names forward here with them).
//!
//! The outline is not part of the view's colour image: RenderVisionElements writes it by COMPUTE into a surface of its
//! own. Several surfaces in a frame share its dimensions and format, so the match is a conjunction -- a 2D UAV texture
//! at least the second view's size (a pre-filter; under DLSS the layer is at the OUTPUT resolution and the view's
//! recorded size is the RENDER resolution, hence 'at least', never 'equal', and no upper bound: DLSS Ultra Performance
//! sits near 3.0), the dispatch that just ran covering it at 8x8, and the node and its ordinal. The [rtmap] and
//! [vismap] tables are kept because the failure mode here is SILENCE: an outline that never appears looks exactly like
//! a feature that is switched off. Both are reported as tables at most every 20 s, never a line per hit, and the
//! node table says when it runs out of room.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use std::ffi::CString;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

#[allow(non_upper_case_globals)]
extern "C" {
    /// The second view's recorded render size.
    static g_vrcam_view_w: AtomicU32;
    static g_vrcam_view_h: AtomicU32;
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

unsafe extern "C" fn read_desc(resource: *mut c_void, out: *mut c_void, _: *mut c_void) -> bool {
    let Some(resource) = ID3D12Resource::from_raw_borrowed(&resource) else {
        return false;
    };
    *(out as *mut D3D12_RESOURCE_DESC) = resource.GetDesc();
    true
}
/// The resource's description, read under SEH (the C++'s mirror_get_resource_desc).
unsafe fn resource_desc(resource: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
    if resource.is_null() {
        return None;
    }
    let mut desc = D3D12_RESOURCE_DESC::default();
    let ok = cvr_engine::code::guarded_call3(
        read_desc as *const () as usize,
        resource,
        (&mut desc as *mut D3D12_RESOURCE_DESC).cast(),
        core::ptr::null_mut(),
    );
    (ok == Some(true)).then_some(desc)
}

/// Is this surface at least as big as the second view?
unsafe fn about_view(d: &D3D12_RESOURCE_DESC) -> bool {
    let vw = g_vrcam_view_w.load(Acquire);
    let vh = g_vrcam_view_h.load(Acquire);
    if vw == 0 || vh == 0 {
        return false;
    }
    d.Width >= vw as u64 && d.Height >= vh
}

/// The pre-filter: a 2D single-sample UAV texture the size of a view (format, mips and RENDER_TARGET described one
/// measured allocation, not a requirement).
///
/// # Safety
/// `d` points to a resource description.
#[no_mangle]
pub unsafe extern "C" fn cvr_vision_layer_signature(d: *const D3D12_RESOURCE_DESC) -> bool {
    let d = &*d;
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && d.DepthOrArraySize == 1
        && d.SampleDesc.Count == 1
        && (d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0) != 0
        && about_view(d)
}
/// The last dispatch on this thread (`dispatch`: x, y, z) covered the surface at 8x8, one deep.
///
/// # Safety
/// `d` points to a resource description, `dispatch` to three group counts.
#[no_mangle]
pub unsafe extern "C" fn cvr_vision_matches_dispatch(
    d: *const D3D12_RESOURCE_DESC,
    dispatch: *const u32,
) -> bool {
    let d = &*d;
    let (x, y, z) = (*dispatch, *dispatch.add(1), *dispatch.add(2));
    if z != 1 || x == 0 || y == 0 {
        return false;
    }
    let gx = ((d.Width + 7) / 8) as u32;
    let gy = (d.Height + 7) / 8;
    x == gx && y == gy
}
/// The second view's OWN full-size surface rather than one of its half-res intermediates (the pre-filter's range).
///
/// # Safety
/// `d` points to a resource description.
#[no_mangle]
pub unsafe extern "C" fn cvr_vision_is_vrcam_full_size(d: *const D3D12_RESOURCE_DESC) -> bool {
    about_view(&*d)
}

// ---- node -> render-target map: what every node actually binds, per view ------------------------------------

#[derive(Clone, Copy, Default)]
struct RtMapEntry {
    node_rva: u32,
    w: u32,
    h: u32,
    fmt: u32,
    mips: u32,
    hits: [u64; 2],
}
struct RtMap {
    rows: Vec<RtMapEntry>,
}
const RTMAP_CAPACITY: usize = 128;
static RTMAP: Mutex<RtMap> = Mutex::new(RtMap { rows: Vec::new() });
static RTMAP_LAST: AtomicU64 = AtomicU64::new(0);

/// `snprintf` into a `size`-byte buffer at `u`: the text cut to what fits, `u` advanced by its full length.
fn append(line: &mut String, u: &mut usize, size: usize, text: &str) {
    if *u < size {
        let room = size - *u - 1;
        line.push_str(&text[..text.len().min(room)]);
    }
    *u += text.len();
}

fn rtmap_report() {
    // SAFETY: plain Win32 call.
    let now = unsafe { GetTickCount64() };
    let last = RTMAP_LAST.load(Relaxed);
    if last != 0 && now - last < 20000 {
        return;
    }
    let rows = RTMAP.lock().unwrap_or_else(|p| p.into_inner()).rows.clone();
    if rows.is_empty() {
        return;
    }
    RTMAP_LAST.store(now, Relaxed);
    // Only 1-mip RGBA8 colour targets (TYPELESS 27 / UNORM 28 / UNORM_SRGB 29) at least 1000 wide: the shape of an
    // overlay layer.
    for pass in 0..2 {
        let (mut line, mut u, mut c) = (String::new(), 0usize, 0);
        for e in rows.iter().filter(|e| e.hits[pass] != 0) {
            if e.mips != 1 || e.w < 1000 || !(27..=29).contains(&e.fmt) {
                continue;
            }
            c += 1;
            if u < 1400 - 56 {
                append(
                    &mut line,
                    &mut u,
                    1400,
                    &format!(
                        "{:X}:{}x{}/f{}({}) ",
                        e.node_rva, e.w, e.h, e.fmt, e.hits[pass]
                    ),
                );
            }
        }
        log(&format!(
            "[rtmap] {:<5} node:RGBA8 1-mip full-size targets ({c}): {}",
            if pass != 0 { "VRCAM" } else { "MAIN" },
            if c != 0 { line.as_str() } else { "(none)" }
        ));
    }
}

/// The target `res` bound while node `work` records, per view.
///
/// # Safety
/// `res` is a live resource or null; `exe_base` the C++'s g_exe_base (nothing is recorded before it is resolved).
#[no_mangle]
pub unsafe extern "C" fn cvr_rtmap_note(
    res: *mut c_void,
    vrcam: bool,
    work: usize,
    exe_base: usize,
) {
    if exe_base == 0 || res.is_null() {
        return;
    }
    let rva = if work > exe_base {
        (work - exe_base) as u32
    } else {
        0
    };
    let Some(d) = resource_desc(res) else {
        return;
    };
    {
        let mut map = RTMAP.lock().unwrap_or_else(|p| p.into_inner());
        let key = RtMapEntry {
            node_rva: rva,
            w: d.Width as u32,
            h: d.Height,
            fmt: d.Format.0 as u32,
            mips: d.MipLevels as u32,
            hits: [0; 2],
        };
        let i = match map.rows.iter().position(|e| {
            (e.node_rva, e.w, e.h, e.fmt, e.mips) == (key.node_rva, key.w, key.h, key.fmt, key.mips)
        }) {
            Some(i) => i,
            None => {
                if map.rows.len() >= RTMAP_CAPACITY {
                    return;
                }
                map.rows.push(key);
                map.rows.len() - 1
            }
        };
        map.rows[i].hits[vrcam as usize] += 1;
    }
    rtmap_report();
}

// ---- the vision map: node#ordinal:size per view -----------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct VisMapEntry {
    node_rva: u32,
    w: u32,
    h: u32,
    ord: i32,
    hits: [u64; 2],
}
struct VisMap {
    rows: Vec<VisMapEntry>,
    full_said: bool,
}
/// 256, NOT 64, AND IT SAYS SO WHEN IT FILLS: at 64 the table saturated in an ordinary session and the node being
/// hunted was simply missing from the map.
const VISMAP_CAPACITY: usize = 256;
static VISMAP: Mutex<VisMap> = Mutex::new(VisMap {
    rows: Vec::new(),
    full_said: false,
});
static VISMAP_LAST: AtomicU64 = AtomicU64::new(0);

fn vision_report() {
    // SAFETY: plain Win32 call.
    let now = unsafe { GetTickCount64() };
    let last = VISMAP_LAST.load(Relaxed);
    if last != 0 && now - last < 20000 {
        return;
    }
    let rows = VISMAP
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .rows
        .clone();
    if rows.is_empty() {
        return;
    }
    VISMAP_LAST.store(now, Relaxed);
    for pass in 0..2 {
        // 4096, and what does not fit is counted and printed.
        let (mut line, mut u, mut c, mut dropped) = (String::new(), 0usize, 0, 0);
        for e in rows.iter().filter(|e| e.hits[pass] != 0) {
            c += 1;
            if u < 4096 - 48 {
                append(
                    &mut line,
                    &mut u,
                    4096,
                    &format!(
                        "{:X}#{}:{}x{}({}) ",
                        e.node_rva, e.ord, e.w, e.h, e.hits[pass]
                    ),
                );
            } else {
                dropped += 1;
            }
        }
        log(&format!(
            "[vismap] {:<5} node#ordinal:size(hits) ({c}, {dropped} not shown): {}",
            if pass != 0 { "VRCAM" } else { "MAIN" },
            if c != 0 { line.as_str() } else { "(none)" }
        ));
    }
}

/// A full-size match `res` in node `node_rva` at ordinal `ord`, per view.
///
/// # Safety
/// `res` is a live resource or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_vision_note_surface(
    res: *mut c_void,
    vrcam: bool,
    node_rva: u32,
    ord: i32,
) {
    let Some(d) = resource_desc(res) else {
        return;
    };
    {
        let mut map = VISMAP.lock().unwrap_or_else(|p| p.into_inner());
        let (w, h) = (d.Width as u32, d.Height);
        let i = match map
            .rows
            .iter()
            .position(|e| e.node_rva == node_rva && e.ord == ord && e.w == w && e.h == h)
        {
            Some(i) => i,
            None => {
                if map.rows.len() >= VISMAP_CAPACITY {
                    if !map.full_said {
                        map.full_said = true;
                        log(&format!(
                            "[vismap] TABLE FULL at {} entries -- rows are being dropped, so a node's absence from the map \
                             below proves nothing",
                            map.rows.len()
                        ));
                    }
                    return;
                }
                map.rows.push(VisMapEntry {
                    node_rva,
                    w,
                    h,
                    ord,
                    hits: [0; 2],
                });
                map.rows.len() - 1
            }
        };
        map.rows[i].hits[vrcam as usize] += 1;
    }
    vision_report();
}
