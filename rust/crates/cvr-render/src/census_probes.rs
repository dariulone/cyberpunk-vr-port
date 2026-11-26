//! THE PROBES ON THE BARRIER HOOK'S TRANSITIONS (part of formerly src/Stereo/CommandListCensus.cpp).
//!
//!   expo  the per-view FrameExposureData (28 bytes): MAIN's parked in a staging buffer of ours where MAIN's state is
//!         known and written over the second view's where the second view's is, field-masked (127, the whole buffer,
//!         ships: the two eyes must show one exposure -- without it the second view's pre-exposure multiplier sits
//!         2880x MAIN's and every target from Lighting on comes out white); and the opt-in readback that prints both
//!         views' seven floats ([expo], 8 s or 200 ms for a transient).
//!   tile  the light cull's per-tile grid read back per view and per shape, three buffers round-robin and only the
//!         oldest read (the copy is recorded in the engine's list and nothing waits for the GPU).
//!   cull  the small UAV counters, read off a UAV barrier from the state they never leave.
//!   lights, dispatch, light content, indirect: what each view uploads, dispatches and executes indirectly, by node,
//!         with what one view does and the other never does named in the reports.
//!
//! Every copy is appended to the engine's own list at the barrier that names the resource's state.
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use core::ffi::{c_char, c_void};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::hud::Barrier;
use crate::single_pass::Hook;
use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_ExpoProbe: AtomicI32;
    static CyberpunkVR_DebugExpoValMain: AtomicU32;
    static CyberpunkVR_DebugExpoValVrcam: AtomicU32;
    static CyberpunkVR_ExpoMirror: AtomicI32;
    static CyberpunkVR_ExpoFieldMask: AtomicU32;
    static CyberpunkVR_DebugExpoMirrors: AtomicU64;
    static CyberpunkVR_TileProbe: AtomicI32;
    static CyberpunkVR_DebugTileNonzeroMain: AtomicU64;
    static CyberpunkVR_DebugTileNonzeroVrcam: AtomicU64;
    static CyberpunkVR_CullCountProbe: AtomicI32;
    static g_game_device: *mut c_void;
    fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const Hook;
    /// `cvr::log("%s", text)`: the stereo module's switch and rate cap.
    fn cvr_cpp_stereo_log(text: *const c_char);
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
fn g6(v: f32) -> String {
    cvr_crt::format(c"%.6g", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
const fn st(s: D3D12_RESOURCE_STATES) -> u32 {
    s.0 as u32
}
const SHADER_READ: u32 = st(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE)
    | st(D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
const COPY_SOURCE: u32 = st(D3D12_RESOURCE_STATE_COPY_SOURCE);
const COPY_DEST: u32 = st(D3D12_RESOURCE_STATE_COPY_DEST);
const UNORDERED: u32 = st(D3D12_RESOURCE_STATE_UNORDERED_ACCESS);

/// The list's raw barrier, buffer copy and texture copy (past our own hooks), when hooked.
struct Calls {
    barrier: unsafe extern "system" fn(*mut c_void, u32, *const Barrier),
    copy_buffer:
        Option<unsafe extern "system" fn(*mut c_void, *mut c_void, u64, *mut c_void, u64, u64)>,
    copy_texture: Option<
        unsafe extern "system" fn(
            *mut c_void,
            *const D3D12_TEXTURE_COPY_LOCATION,
            u32,
            u32,
            u32,
            *const D3D12_TEXTURE_COPY_LOCATION,
            *const D3D12_BOX,
        ),
    >,
}
unsafe fn calls(list: *mut c_void) -> Option<Calls> {
    let hook = cvr_cpp_command_list_hook_entry(list).as_ref()?;
    if hook.barrier_call == 0 {
        return None;
    }
    Some(Calls {
        barrier: core::mem::transmute(hook.barrier_call),
        copy_buffer: (hook.cbr_original != 0).then(|| core::mem::transmute(hook.cbr_original)),
        copy_texture: (hook.copytex != 0).then(|| core::mem::transmute(hook.copytex)),
    })
}
impl Calls {
    unsafe fn transition(&self, list: *mut c_void, res: *mut c_void, before: u32, after: u32) {
        (self.barrier)(list, 1, &Barrier::transition(res, before, after));
    }
}
fn buffer_desc(width: u64) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
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
    }
}
fn heap(kind: D3D12_HEAP_TYPE) -> D3D12_HEAP_PROPERTIES {
    D3D12_HEAP_PROPERTIES {
        Type: kind,
        CreationNodeMask: 1,
        VisibleNodeMask: 1,
        ..Default::default()
    }
}
/// A buffer of `width` bytes in `kind`, resting in COPY_DEST.
unsafe fn buffer(kind: D3D12_HEAP_TYPE, width: u64) -> Option<ID3D12Resource> {
    let device = g_game_device;
    let dev = ID3D12Device::from_raw_borrowed(&device)?;
    let mut res: Option<ID3D12Resource> = None;
    dev.CreateCommittedResource(
        &heap(kind),
        D3D12_HEAP_FLAG_NONE,
        &buffer_desc(width),
        D3D12_RESOURCE_STATE_COPY_DEST,
        None,
        &mut res,
    )
    .ok()?;
    res
}
/// Mapped for reading, nothing declared read yet.
unsafe fn mapped(res: &ID3D12Resource) -> Option<*mut u8> {
    let mut m = null_mut();
    res.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut m))
        .ok()?;
    (!m.is_null()).then_some(m.cast())
}
fn set_name(res: &ID3D12Resource, name: &str) {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated wide string that outlives the call.
    let _ = unsafe { res.SetName(windows::core::PCWSTR(wide.as_ptr())) };
}

// ---- expo ------------------------------------------------------------------------------------------------------

struct Expo {
    /// The readbacks (MAIN, the second view, the optics view) and their mappings.
    rb: [usize; 3],
    map: [usize; 3],
    stage: usize,
    /// False until MAIN's exposure has actually been parked: never hand the second view uninitialised bytes.
    stage_ready: bool,
}
static EXPO: Mutex<Expo> = Mutex::new(Expo {
    rb: [0; 3],
    map: [0; 3],
    stage: 0,
    stage_ready: false,
});

unsafe fn expo_rb_ensure(e: &mut Expo, v: usize) -> bool {
    if e.rb[v] != 0 {
        return e.map[v] != 0;
    }
    let Some(rb) = buffer(D3D12_HEAP_TYPE_READBACK, 256) else {
        return false;
    };
    let Some(m) = mapped(&rb) else {
        return false;
    };
    set_name(
        &rb,
        [
            "CyberpunkVR_ExpoRbMain",
            "CyberpunkVR_ExpoRbVrcam",
            "CyberpunkVR_ExpoRbOptics",
        ][v],
    );
    e.rb[v] = rb.into_raw() as usize;
    e.map[v] = m as usize;
    true
}
unsafe fn expo_stage_ensure(e: &mut Expo) -> bool {
    if e.stage != 0 {
        return true;
    }
    let Some(stage) = buffer(D3D12_HEAP_TYPE_DEFAULT, 256) else {
        return false;
    };
    set_name(&stage, "CyberpunkVR_ExpoStage");
    e.stage = stage.into_raw() as usize;
    true
}

/// MAIN: its exposure parked in our staging buffer. The second view: MAIN's written over its own (the masked
/// fields, in coalesced runs), only once MAIN's is genuinely there. Frame order is the second view first, so it
/// gets MAIN's from the previous frame -- exposure adapts over many frames.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_expo_mirror(list: *mut c_void, src: *mut c_void, vrcam: bool) {
    if CyberpunkVR_ExpoMirror.load(Relaxed) == 0 || list.is_null() || src.is_null() {
        return;
    }
    let mut e = EXPO.lock().unwrap_or_else(|p| p.into_inner());
    if !expo_stage_ensure(&mut e) {
        return;
    }
    let Some(c) = calls(list) else { return };
    let Some(copy_buffer) = c.copy_buffer else {
        return;
    };
    let stage = e.stage as *mut c_void;
    if !vrcam {
        c.transition(list, src, SHADER_READ, COPY_SOURCE);
        copy_buffer(list, stage, 0, src, 0, 28);
        e.stage_ready = true;
        c.transition(list, src, COPY_SOURCE, SHADER_READ);
        return;
    }
    if !e.stage_ready {
        return;
    }
    c.transition(list, stage, COPY_DEST, COPY_SOURCE);
    c.transition(list, src, SHADER_READ, COPY_DEST);
    let mask = CyberpunkVR_ExpoFieldMask.load(Relaxed) & 0x7F;
    if mask == 0x7F {
        copy_buffer(list, src, 0, stage, 0, 28);
    } else {
        let mut k = 0u32;
        while k < 7 {
            if mask & (1 << k) == 0 {
                k += 1;
                continue;
            }
            let mut run = k;
            while run < 7 && mask & (1 << run) != 0 {
                run += 1;
            }
            copy_buffer(
                list,
                src,
                k as u64 * 4,
                stage,
                k as u64 * 4,
                (run - k) as u64 * 4,
            );
            k = run;
        }
    }
    c.transition(list, src, COPY_DEST, SHADER_READ);
    c.transition(list, stage, COPY_SOURCE, COPY_DEST);
    diag(&CyberpunkVR_DebugExpoMirrors);
}
/// The view's 28 bytes into its readback, at the barrier that hands them to the shaders.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_expo_probe_copy(list: *mut c_void, src: *mut c_void, v: i32) {
    if CyberpunkVR_ExpoProbe.load(Relaxed) == 0
        || list.is_null()
        || src.is_null()
        || !(0..3).contains(&v)
    {
        return;
    }
    let mut e = EXPO.lock().unwrap_or_else(|p| p.into_inner());
    if !expo_rb_ensure(&mut e, v as usize) {
        return;
    }
    let Some(c) = calls(list) else { return };
    let Some(copy_buffer) = c.copy_buffer else {
        return;
    };
    c.transition(list, src, SHADER_READ, COPY_SOURCE);
    copy_buffer(list, e.rb[v as usize] as *mut c_void, 0, src, 0, 28);
    c.transition(list, src, COPY_SOURCE, SHADER_READ);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_census_expo_probe_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let probe = CyberpunkVR_ExpoProbe.load(Relaxed);
    let maps = EXPO.lock().unwrap_or_else(|p| p.into_inner()).map;
    if probe == 0 || maps[0] == 0 || maps[1] == 0 {
        return;
    }
    let now = tick();
    let period = if probe >= 2 { 200 } else { 8000 };
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < period {
        return;
    }
    LAST.store(now, Relaxed);
    let read7 = |p: usize| -> [f32; 7] {
        core::array::from_fn(|i| core::ptr::read_unaligned((p as *const f32).add(i)))
    };
    let (m, v) = (read7(maps[0]), read7(maps[1]));
    CyberpunkVR_DebugExpoValMain.store(m[6].to_bits(), Relaxed);
    CyberpunkVR_DebugExpoValVrcam.store(v[6].to_bits(), Relaxed);
    let row = |f: &[f32; 7]| f.iter().map(|x| g6(*x)).collect::<Vec<_>>().join(" ");
    log(&format!(
        "[expo] t={now} MAIN {} | VRCAM {}",
        row(&m),
        row(&v)
    ));
    if maps[2] != 0 {
        log(&format!("[expo-optics] t={now} {}", row(&read7(maps[2]))));
    }
}

// ---- tile ------------------------------------------------------------------------------------------------------

struct TileGrid {
    w: u32,
    h: u32,
    pitch: u32,
    fmt: DXGI_FORMAT,
    rb: [usize; 3],
    map: [usize; 3],
    writes: u32,
    nz: u64,
    mx: u64,
    sum: u64,
    seen: bool,
}
/// Every distinct grid per view, keyed by its shape: only grids of the SAME shape compare between the views.
static TILES: Mutex<[Vec<TileGrid>; 2]> = Mutex::new([Vec::new(), Vec::new()]);

/// The light cull's grid: a small single-mip UAV integer texture at render-res/32.
#[no_mangle]
pub extern "C" fn cvr_census_tile_is_grid(d: &D3D12_RESOURCE_DESC) -> bool {
    d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && d.MipLevels == 1
        && d.DepthOrArraySize == 1
        && (d.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0
        && (40..=220).contains(&d.Width)
        && (40..=220).contains(&d.Height)
        && [
            DXGI_FORMAT_R16_UINT,
            DXGI_FORMAT_R32_UINT,
            DXGI_FORMAT_R8_UINT,
            DXGI_FORMAT_R32G32_UINT,
        ]
        .contains(&d.Format)
}
unsafe fn tile_slot(grids: &mut Vec<TileGrid>, d: &D3D12_RESOURCE_DESC) -> Option<usize> {
    let w = d.Width as u32;
    if let Some(i) = grids
        .iter()
        .position(|t| t.w == w && t.h == d.Height && t.fmt == d.Format)
    {
        return Some(i);
    }
    let device = g_game_device;
    if grids.len() >= 8 {
        return None;
    }
    let dev = ID3D12Device::from_raw_borrowed(&device)?;
    let mut fp = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut total = 0u64;
    dev.GetCopyableFootprints(d, 0, 1, 0, Some(&mut fp), None, None, Some(&mut total));
    let mut rb = Vec::with_capacity(3);
    let mut map = [0usize; 3];
    for slot in &mut map {
        let res = buffer(D3D12_HEAP_TYPE_READBACK, total)?; // a failure drops (releases) what was made
        *slot = mapped(&res)? as usize;
        rb.push(res);
    }
    let rb: [usize; 3] = core::array::from_fn(|k| rb[k].clone().into_raw() as usize);
    grids.push(TileGrid {
        w,
        h: d.Height,
        pitch: fp.Footprint.RowPitch,
        fmt: d.Format,
        rb,
        map,
        writes: 0,
        nz: 0,
        mx: 0,
        sum: 0,
        seen: false,
    });
    Some(grids.len() - 1)
}
/// Appended at the barrier that publishes the grid, so the source state is exact.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_tile_probe_copy(
    list: *mut c_void,
    src: *mut c_void,
    d: &D3D12_RESOURCE_DESC,
    after: u32,
    vrcam: bool,
) {
    if CyberpunkVR_TileProbe.load(Relaxed) == 0
        || list.is_null()
        || src.is_null()
        || g_game_device.is_null()
    {
        return;
    }
    let mut tiles = TILES.lock().unwrap_or_else(|p| p.into_inner());
    let grids = &mut tiles[vrcam as usize];
    let Some(i) = tile_slot(grids, d) else { return };
    let Some(c) = calls(list) else { return };
    let Some(copy_texture) = c.copy_texture else {
        return;
    };
    let t = &mut grids[i];
    c.transition(list, src, after, COPY_SOURCE);
    let dst = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute::<usize, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(
            t.rb[(t.writes % 3) as usize],
        ),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                Offset: 0,
                Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                    Format: d.Format,
                    Width: t.w,
                    Height: t.h,
                    Depth: 1,
                    RowPitch: t.pitch,
                },
            },
        },
    };
    let source = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute::<
            *mut c_void,
            core::mem::ManuallyDrop<Option<ID3D12Resource>>,
        >(src),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    copy_texture(list, &dst, 0, 0, 0, &source, core::ptr::null());
    c.transition(list, src, COPY_SOURCE, after);
    t.writes += 1;
    t.seen = true;
}
/// The oldest of the three buffers: two further copies were recorded after it, so the GPU has retired it.
unsafe fn tile_scan(t: &mut TileGrid) {
    let base = t.map[(t.writes % 3) as usize] as *const u8;
    let stride = if t.fmt == DXGI_FORMAT_R8_UINT {
        1
    } else if t.fmt == DXGI_FORMAT_R16_UINT {
        2
    } else if t.fmt == DXGI_FORMAT_R32G32_UINT {
        8
    } else {
        4
    };
    (t.nz, t.mx, t.sum) = (0, 0, 0);
    for y in 0..t.h as usize {
        let row = base.add(y * t.pitch as usize);
        for x in 0..t.w as usize {
            let mut bytes = [0u8; 8];
            core::ptr::copy_nonoverlapping(row.add(x * stride), bytes.as_mut_ptr(), stride);
            let val = u64::from_le_bytes(bytes);
            if val != 0 {
                t.nz += 1;
                t.sum += val;
                t.mx = t.mx.max(val);
            }
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_census_tile_probe_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 8000 {
        return;
    }
    let mut tiles = TILES.lock().unwrap_or_else(|p| p.into_inner());
    if tiles[0].is_empty() || tiles[1].is_empty() {
        return;
    }
    LAST.store(now, Relaxed);
    for (v, grids) in tiles.iter_mut().enumerate() {
        let mut line = String::new();
        for t in grids.iter_mut() {
            if !t.seen || t.writes < 3 {
                continue; // the ring not filled yet: nothing retired
            }
            tile_scan(t);
            if line.len() < 900 - 80 {
                line.push_str(&format!(
                    "{}x{} fmt{} nz{} max{} sum{} | ",
                    t.w, t.h, t.fmt.0 as u32, t.nz, t.mx, t.sum
                ));
            }
            if v == 0 {
                CyberpunkVR_DebugTileNonzeroMain.store(t.nz, Relaxed);
            } else {
                CyberpunkVR_DebugTileNonzeroVrcam.store(t.nz, Relaxed);
            }
        }
        log(&format!(
            "[tile] {:<5} grids: {}",
            if v == 1 { "VRCAM" } else { "MAIN" },
            if line.is_empty() {
                "(none)"
            } else {
                line.as_str()
            }
        ));
    }
}

// ---- cull ------------------------------------------------------------------------------------------------------

struct CullCnt {
    res: usize,
    rb: [usize; 3],
    map: [usize; 3],
    writes: u32,
    bytes: u32,
    last: [[u32; 5]; 2],
    seen: [bool; 2],
}
static CULL: Mutex<Vec<CullCnt>> = Mutex::new(Vec::new());

unsafe fn cull_slot(counters: &mut Vec<CullCnt>, res: *mut c_void, bytes: u32) -> Option<usize> {
    if let Some(i) = counters.iter().position(|c| c.res == res as usize) {
        return Some(i);
    }
    if counters.len() >= 6 || g_game_device.is_null() {
        return None;
    }
    let mut made = Vec::with_capacity(3);
    let mut map = [0usize; 3];
    for slot in &mut map {
        let rb = buffer(D3D12_HEAP_TYPE_READBACK, 256)?;
        *slot = mapped(&rb)? as usize;
        made.push(rb);
    }
    let rb: [usize; 3] = core::array::from_fn(|k| made[k].clone().into_raw() as usize);
    counters.push(CullCnt {
        res: res as usize,
        rb,
        map,
        writes: 0,
        bytes: bytes.min(20),
        last: [[0; 5]; 2],
        seen: [false; 2],
    });
    Some(counters.len() - 1)
}
/// A small UAV counter copied out from the state it never leaves, and the view's row taken from the oldest ring
/// entry, which the GPU has long retired.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_cull_count_note(
    list: *mut c_void,
    res: *mut c_void,
    bytes: u32,
    vrcam: bool,
) {
    if CyberpunkVR_CullCountProbe.load(Relaxed) == 0 || list.is_null() || res.is_null() {
        return;
    }
    let mut counters = CULL.lock().unwrap_or_else(|p| p.into_inner());
    let Some(i) = cull_slot(&mut counters, res, bytes) else {
        return;
    };
    let Some(c) = calls(list) else { return };
    let Some(copy_buffer) = c.copy_buffer else {
        return;
    };
    let cnt = &mut counters[i];
    c.transition(list, res, UNORDERED, COPY_SOURCE);
    copy_buffer(
        list,
        cnt.rb[(cnt.writes % 3) as usize] as *mut c_void,
        0,
        res,
        0,
        cnt.bytes as u64,
    );
    c.transition(list, res, COPY_SOURCE, UNORDERED);
    cnt.writes += 1;
    cnt.seen[vrcam as usize] = true;
    if cnt.writes >= 3 {
        let oldest = cnt.map[(cnt.writes % 3) as usize] as *const u8;
        let row = cnt.last[vrcam as usize].as_mut_ptr().cast::<u8>();
        core::ptr::copy_nonoverlapping(oldest, row, cnt.bytes as usize);
    }
}
#[no_mangle]
pub extern "C" fn cvr_census_cull_count_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 8000 {
        return;
    }
    let counters = CULL.lock().unwrap_or_else(|p| p.into_inner());
    let both = counters.iter().any(|c| c.seen[0] && c.seen[1]);
    if counters.is_empty() {
        return;
    }
    LAST.store(now, Relaxed);
    let mut line = String::new();
    for c in counters.iter() {
        if line.len() >= 1000 - 90 {
            break;
        }
        let (m, v) = (c.last[0], c.last[1]);
        line.push_str(&format!(
            "{:016X}({}B,{}{}) M[{} {} {} {} {}] V[{} {} {} {} {}] | ",
            c.res,
            c.bytes,
            if c.seen[0] { "m" } else { "-" },
            if c.seen[1] { "v" } else { "-" },
            m[0],
            m[1],
            m[2],
            m[3],
            m[4],
            v[0],
            v[1],
            v[2],
            v[3],
            v[4]
        ));
    }
    log(&format!(
        "[cull] small UAV counters (shared ones carry both m and v): {line}{}",
        if both {
            ""
        } else {
            "  [no counter seen by both views yet]"
        }
    ));
}

// ---- the light uploads -----------------------------------------------------------------------------------------

fn g4(v: f64) -> String {
    cvr_crt::format(c"%.4g", &[cvr_crt::Arg::F(v)])
        .to_string_lossy()
        .into_owned()
}
fn exclusive(main: bool, vrcam: bool, spaced: bool) -> &'static str {
    match (main, vrcam, spaced) {
        (true, false, false) => "!MAIN-only",
        (false, true, false) => "!VRCAM-only",
        (true, false, true) => " !MAIN-only",
        (false, true, true) => " !VRCAM-only",
        _ => "",
    }
}

#[derive(Clone, Copy)]
struct LightSizeBin {
    bytes: u32,
    hits: [u32; 2],
    dst_size: [u64; 2],
}
/// A constant-buffer destination of the light uploads (<= 4 KB; the big arrays are noise).
#[derive(Clone, Copy)]
struct LightDst {
    res: usize,
    size: u64,
    hits: [u32; 2],
    last_bytes: u32,
}
struct Lights {
    bins: Vec<LightSizeBin>,
    dsts: Vec<LightDst>,
}
static LIGHTS: Mutex<Lights> = Mutex::new(Lights {
    bins: Vec::new(),
    dsts: Vec::new(),
});

/// One upload inside ClusteredLightsCull + RenderLightBuffers: its size per view and its destination.
#[no_mangle]
pub extern "C" fn cvr_census_light_note(
    num_bytes: u64,
    dst_size: u64,
    dst: *mut c_void,
    vrcam: bool,
) {
    let sz = num_bytes as u32;
    let v = vrcam as usize;
    let mut l = LIGHTS.lock().unwrap_or_else(|p| p.into_inner());
    let i = match l.bins.iter().position(|b| b.bytes == sz) {
        Some(i) => i,
        None if l.bins.len() >= 48 => return,
        None => {
            l.bins.push(LightSizeBin {
                bytes: sz,
                hits: [0; 2],
                dst_size: [0; 2],
            });
            l.bins.len() - 1
        }
    };
    l.bins[i].hits[v] += 1;
    l.bins[i].dst_size[v] = dst_size;
    if dst.is_null() || dst_size > 4096 {
        return;
    }
    let i = match l.dsts.iter().position(|d| d.res == dst as usize) {
        Some(i) => i,
        None if l.dsts.len() >= 24 => return,
        None => {
            l.dsts.push(LightDst {
                res: dst as usize,
                size: dst_size,
                hits: [0; 2],
                last_bytes: sz,
            });
            l.dsts.len() - 1
        }
    };
    l.dsts[i].hits[v] += 1;
    l.dsts[i].last_bytes = sz;
}
/// Upload size, how often each view makes it and the destination's capacity, biggest first: a size one view
/// uploads and the other never does is what is hunted. Every 12 s once the second view has uploaded at all.
#[no_mangle]
pub extern "C" fn cvr_census_light_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 12000 {
        return;
    }
    let mut bins = LIGHTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .bins
        .clone();
    if bins.is_empty() || !bins.iter().any(|b| b.hits[1] != 0) {
        return;
    }
    LAST.store(now, Relaxed);
    bins.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    let mut line = String::new();
    for b in &bins {
        if line.len() >= 1500 - 48 {
            break;
        }
        line.push_str(&format!(
            "{}B m{}/v{} dst{}{} | ",
            b.bytes,
            b.hits[0],
            b.hits[1],
            if b.dst_size[0] != 0 {
                b.dst_size[0]
            } else {
                b.dst_size[1]
            },
            exclusive(b.hits[0] != 0, b.hits[1] != 0, false)
        ));
    }
    log(&format!(
        "[lights] upload sizes inside ClusteredLightsCull+RenderLightBuffers: {line}"
    ));
    let mut dl = String::new();
    for d in LIGHTS.lock().unwrap_or_else(|p| p.into_inner()).dsts.iter() {
        if dl.len() >= 900 - 64 {
            break;
        }
        dl.push_str(&format!(
            "{:016X} sz{} m{}/v{} last{}B{} | ",
            d.res,
            d.size,
            d.hits[0],
            d.hits[1],
            d.last_bytes,
            exclusive(d.hits[0] != 0, d.hits[1] != 0, true)
        ));
    }
    log(&format!(
        "[lights] constant-buffer destinations (<=4KB): {dl}"
    ));
}

// ---- the node behind each compute dispatch ---------------------------------------------------------------------

struct DispatchBin {
    node_rva: u32,
    shape: [u32; 3],
    hits: [u32; 2],
}
static DISPATCH: Mutex<Vec<DispatchBin>> = Mutex::new(Vec::new());

#[no_mangle]
pub extern "C" fn cvr_census_dispatch_note(rva: u32, x: u32, y: u32, z: u32, vrcam: bool) {
    let mut bins = DISPATCH.lock().unwrap_or_else(|p| p.into_inner());
    let i = match bins
        .iter()
        .position(|b| b.node_rva == rva && b.shape == [x, y, z])
    {
        Some(i) => i,
        None if bins.len() >= 96 => return,
        None => {
            bins.push(DispatchBin {
                node_rva: rva,
                shape: [x, y, z],
                hits: [0; 2],
            });
            bins.len() - 1
        }
    };
    bins[i].hits[vrcam as usize] += 1;
}
/// Every node that dispatches for one view and NEVER for the other, aggregated per node rather than per shape
/// (group counts are content-derived, so exact shapes almost never match between the views).
#[no_mangle]
pub extern "C" fn cvr_census_dispatch_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 12000 {
        return;
    }
    let mut agg: Vec<(u32, [u64; 2])> = Vec::new();
    {
        let bins = DISPATCH.lock().unwrap_or_else(|p| p.into_inner());
        if !bins.iter().any(|b| b.hits[0] != 0) || !bins.iter().any(|b| b.hits[1] != 0) {
            return;
        }
        LAST.store(now, Relaxed);
        for b in bins.iter() {
            let k = match agg.iter().position(|a| a.0 == b.node_rva) {
                Some(k) => k,
                None if agg.len() >= 96 => continue,
                None => {
                    agg.push((b.node_rva, [0; 2]));
                    agg.len() - 1
                }
            };
            for s in 0..2 {
                agg[k].1[s] += b.hits[s] as u64;
            }
        }
    }
    let (mut mo, mut vo) = (String::new(), String::new());
    let (mut mn, mut vn) = (0, 0);
    for (rva, hits) in &agg {
        if hits[0] != 0 && hits[1] == 0 {
            mn += 1;
            if mo.len() < 1200 - 32 {
                mo.push_str(&format!("{rva:X}({}) ", hits[0]));
            }
        } else if hits[0] == 0 && hits[1] != 0 {
            vn += 1;
            if vo.len() < 600 - 32 {
                vo.push_str(&format!("{rva:X}({}) ", hits[1]));
            }
        }
    }
    log(&format!(
        "[disp] nodes that dispatch for MAIN and NEVER for VRCAM ({mn} of {}): {mo}",
        agg.len()
    ));
    log(&format!(
        "[disp] nodes that dispatch for VRCAM and NEVER for MAIN ({vn}): {vo}"
    ));
}

// ---- the light array's content ---------------------------------------------------------------------------------

const LIGHT_SNAP_MAX: usize = 64 * 1024;
static LIGHT_CONTENT: Mutex<[Vec<u8>; 2]> = Mutex::new([Vec::new(), Vec::new()]);
extern "C" {
    /// upload_maps.rs: the upload heap's CPU mapping (mapped once, kept), or null.
    fn cvr_upload_map_read(res: *mut c_void) -> *const u8;
}
/// The bytes the engine is about to copy, read straight from the mapped upload ring; the largest seen per view is
/// kept (that is the array).
#[no_mangle]
pub unsafe extern "C" fn cvr_census_light_content_note(
    src: *mut c_void,
    off: u64,
    n: u64,
    vrcam: bool,
) {
    let p = cvr_upload_map_read(src);
    if p.is_null() {
        return;
    }
    let n = n as usize;
    if n > LIGHT_SNAP_MAX {
        return;
    }
    let mut tmp = vec![0u8; n];
    if !code::read_bytes(p as usize + off as usize, &mut tmp) {
        return;
    }
    let mut snaps = LIGHT_CONTENT.lock().unwrap_or_else(|p| p.into_inner());
    let snap = &mut snaps[vrcam as usize];
    if n <= snap.len() {
        return;
    }
    *snap = tmp;
}
/// The two arrays compared as 16-byte records (a world position and a scalar): their lengths, how many records
/// differ and how many look world-space, which lane differs and by how much, and the first five side by side.
#[no_mangle]
pub extern "C" fn cvr_census_light_content_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 12000 {
        return;
    }
    let (a, b) = {
        let snaps = LIGHT_CONTENT.lock().unwrap_or_else(|p| p.into_inner());
        if snaps[0].is_empty() || snaps[1].is_empty() {
            return;
        }
        (snaps[0].clone(), snaps[1].clone())
    };
    LAST.store(now, Relaxed);
    let f32_at = |p: &[u8], o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
    let worldish = |p: &[u8]| {
        let mut c = 0u32;
        let mut o = 0;
        while o + 12 <= p.len() {
            let (x, y, z) = (f32_at(p, o), f32_at(p, o + 4), f32_at(p, o + 8));
            if x.abs() > 100.0 && y.abs() > 100.0 && z.abs() < 500.0 {
                c += 1;
            }
            o += 16;
        }
        c
    };
    let common = a.len().min(b.len());
    let (mut diff, mut first) = (0u32, u32::MAX);
    let mut o = 0;
    while o + 16 <= common {
        if a[o..o + 16] != b[o..o + 16] {
            diff += 1;
            if first == u32::MAX {
                first = o as u32;
            }
        }
        o += 16;
    }
    log(&format!(
        "[lightbuf] MAIN {} B ({} world-ish recs) | VRCAM {} B ({}) | of {} common recs {} differ, first at +{:X}",
        a.len(),
        worldish(&a),
        b.len(),
        worldish(&b),
        common / 16,
        diff,
        if first == u32::MAX { 0 } else { first }
    ));
    // WHICH lane differs decides everything: ~0.064 is a world position shifted by the IPD, wild is view-derived
    // data computed wrong for the second view.
    let mut maxd = [0f64; 4];
    let mut dcnt = [0u32; 4];
    let mut o = 0;
    while o + 16 <= common {
        for l in 0..4 {
            let at = o + l * 4;
            if a[at..at + 4] != b[at..at + 4] {
                dcnt[l] += 1;
                let d = (f32_at(&a, at) as f64 - f32_at(&b, at) as f64).abs();
                if d > maxd[l] && d < 1e30 {
                    maxd[l] = d;
                }
            }
        }
        o += 16;
    }
    log(&format!(
        "[lightbuf] per-lane: L0 {} diff max {} | L1 {} max {} | L2 {} max {} | L3 {} max {}",
        dcnt[0],
        g4(maxd[0]),
        dcnt[1],
        g4(maxd[1]),
        dcnt[2],
        g4(maxd[2]),
        dcnt[3],
        g4(maxd[3])
    ));
    let mut r0 = String::new();
    for k in 0..5usize {
        if (k + 1) * 16 > common {
            break;
        }
        if r0.len() < 700 - 160 {
            let row = |p: &[u8]| {
                (0..4)
                    .map(|i| g4(f32_at(p, k * 16 + i * 4) as f64))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            r0.push_str(&format!("[{k}] M({}) V({})  ", row(&a), row(&b)));
        }
    }
    log(&format!("[lightbuf] first records {r0}"));
}

// ---- the work the Dispatch census could never see: ExecuteIndirect --------------------------------------------

struct IndirectBin {
    node_rva: u32,
    sig: usize,
    hits: [u64; 2],
}
static INDIRECT: Mutex<Vec<IndirectBin>> = Mutex::new(Vec::new());

/// One ExecuteIndirect by node and command signature, per view; reported every 10 s once both views issued any.
#[no_mangle]
pub extern "C" fn cvr_census_indirect_note(rva: u32, sig: *mut c_void, vrcam: bool) {
    {
        let mut bins = INDIRECT.lock().unwrap_or_else(|p| p.into_inner());
        let i = match bins
            .iter()
            .position(|b| b.node_rva == rva && b.sig == sig as usize)
        {
            Some(i) => i,
            None if bins.len() >= 64 => return,
            None => {
                bins.push(IndirectBin {
                    node_rva: rva,
                    sig: sig as usize,
                    hits: [0; 2],
                });
                bins.len() - 1
            }
        };
        bins[i].hits[vrcam as usize] += 1;
    }
    indirect_report();
}
fn indirect_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 10000 {
        return;
    }
    let bins = INDIRECT.lock().unwrap_or_else(|p| p.into_inner());
    if !bins.iter().any(|b| b.hits[0] != 0) || !bins.iter().any(|b| b.hits[1] != 0) {
        return;
    }
    LAST.store(now, Relaxed);
    let (mut mo, mut vo) = (String::new(), String::new());
    for b in bins.iter() {
        if b.hits[0] != 0 && b.hits[1] == 0 && mo.len() < 900 - 48 {
            mo.push_str(&format!(
                "{:X}/sig{:016X}({}) ",
                b.node_rva, b.sig, b.hits[0]
            ));
        }
        if b.hits[0] == 0 && b.hits[1] != 0 && vo.len() < 900 - 48 {
            vo.push_str(&format!(
                "{:X}/sig{:016X}({}) ",
                b.node_rva, b.sig, b.hits[1]
            ));
        }
    }
    drop(bins);
    log(&format!(
        "[indirect] node/signature pairs MAIN issues and VRCAM never: {}",
        if mo.is_empty() { "(none)" } else { mo.as_str() }
    ));
    log(&format!(
        "[indirect] node/signature pairs VRCAM issues and MAIN never: {}",
        if vo.is_empty() { "(none)" } else { vo.as_str() }
    ));
}
