//! THE PIPELINES, THE SIGHT AND THE BUFFER MAPS (part of formerly src/Stereo/CommandListCensus.cpp).
//!
//! Every pipeline's shaders named by the FNV-1a of their bytes at creation (a PSO pointer is not stable across runs,
//! the bytes are); the draw probe that lists the shaders of the draws with a given index count; the holographic
//! sight's pixel shader -- confirmed by removal -- with every vertex shader paired with it cached, the one the DRAW
//! uses written out once; the 6-index instanced quads tallied per node and view ([psoprobe]); the optics shader dump
//! armed by CYBERPUNKVR_OPTICS_SHADER_DUMP=1. And two address maps: the buffers by GPU VA as they are created (a
//! vertex-buffer view carries only a VA), and the last few upload ranges by the VA they were copied to.
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use cvr_host::fast::FastMap;
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{LazyLock, Mutex};
use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_BYTECODE;

use cvr_engine::code;

extern "C" {
    static CyberpunkVR_SightPsHash: AtomicU64;
    static CyberpunkVR_SightPsDump: AtomicI32;
    static CyberpunkVR_DebugSightVsUsed: AtomicU64;
    static CyberpunkVR_DrawProbePs: [AtomicU64; 4];
    static CyberpunkVR_DrawProbeVs: [AtomicU64; 4];
    static CyberpunkVR_DrawProbePsLen: [AtomicU32; 4];
    static CyberpunkVR_DrawProbeHits: AtomicU64;
    fn cvr_cpp_stereo_log(text: *const c_char);
}
fn log(text: &str) {
    if let Ok(text) = std::ffi::CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(1_469_598_103_934_665_603u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(1_099_511_628_211)
    })
}
/// # Safety
/// `p` points to `n` readable bytes (or `n` is 0).
#[no_mangle]
pub unsafe extern "C" fn cvr_census_fnv1a(p: *const u8, n: usize) -> u64 {
    if n == 0 {
        return fnv1a(&[]);
    }
    fnv1a(core::slice::from_raw_parts(p, n))
}
unsafe fn blob(b: &D3D12_SHADER_BYTECODE) -> Option<&[u8]> {
    (!b.pShaderBytecode.is_null() && b.BytecodeLength != 0)
        .then(|| core::slice::from_raw_parts(b.pShaderBytecode.cast::<u8>(), b.BytecodeLength))
}

/// A shader written to the log folder, with the DXBC container's chunk list in the line (SHEX/SHDR: fxc; DXIL:
/// dxc), read guarded.
pub unsafe fn sight_ps_dump(bytes: &[u8], name: &str) {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, name);
    let shown = path.display().to_string();
    if shown.len() >= 260 {
        return; // the C++ copied it into a MAX_PATH buffer
    }
    {
        use std::io::Write;
        let Ok(mut f) = std::fs::File::create(&path) else {
            return;
        };
        let _ = f.write_all(bytes);
    }
    let mut tags = String::new();
    let base = bytes.as_ptr() as usize;
    let len = bytes.len();
    if len > 32 && &bytes[..4] == b"DXBC" {
        if let Some(n) = code::read::<u32>(base + 28) {
            for i in 0..n.min(24) as usize {
                let Some(off) = code::read::<u32>(base + 32 + i * 4) else {
                    break;
                };
                if off as usize + 4 > len {
                    break;
                }
                if tags.len() < 240 {
                    let tag = &bytes[off as usize..off as usize + 4];
                    let end = tag.iter().position(|b| *b == 0).unwrap_or(4);
                    tags.push_str(&String::from_utf8_lossy(&tag[..end]));
                    tags.push(' ');
                }
            }
        }
    }
    log(&format!(
        "[pso] sight shader dumped {len} bytes -> {shown}   chunks: {tags}"
    ));
}
/// # Safety
/// `bytes` points to `len` readable bytes, `name` to a NUL-terminated file name.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_sight_ps_dump(
    bytes: *const u8,
    len: usize,
    name: *const c_char,
) {
    if bytes.is_null() || name.is_null() {
        return;
    }
    sight_ps_dump(
        core::slice::from_raw_parts(bytes, len),
        &CStr::from_ptr(name).to_string_lossy(),
    );
}

#[derive(Clone, Copy, Default)]
struct PsoIds {
    ps: u64,
    vs: u64,
    ps_len: u32,
}
static PSO_IDS: LazyLock<Mutex<FastMap<usize, PsoIds>>> =
    LazyLock::new(|| Mutex::new(cvr_host::fast::fast_map()));
fn ids_of(pso: *mut c_void) -> Option<PsoIds> {
    lock(&PSO_IDS).get(&(pso as usize)).copied()
}
/// Every vertex shader created alongside the sight's pixel shader (the first is not the one it draws with).
struct SightVs {
    hash: u64,
    bytes: Vec<u8>,
    written: bool,
}
static SIGHT_VS: Mutex<Vec<SightVs>> = Mutex::new(Vec::new());

/// A pipeline's shaders named at creation (65536 at most).
///
/// # Safety
/// `ps` and `vs` point to the pipeline description's shader bytecode.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_pso_ids_record(
    pso: *mut c_void,
    ps: *const D3D12_SHADER_BYTECODE,
    vs: *const D3D12_SHADER_BYTECODE,
) {
    if pso.is_null() {
        return;
    }
    let (ps, vs) = (&*ps, &*vs);
    let mut ids = PsoIds::default();
    if let Some(b) = blob(ps) {
        ids.ps = fnv1a(b);
        ids.ps_len = b.len() as u32;
    }
    let vs_bytes = blob(vs);
    if let Some(b) = vs_bytes {
        ids.vs = fnv1a(b);
    }
    if ids.ps == 0 && ids.vs == 0 {
        return;
    }
    // The process-only investigation switch: native shader bytes recorded, no pipeline changed.
    static OPTICS_DUMP: LazyLock<bool> = LazyLock::new(|| {
        std::env::var_os("CYBERPUNKVR_OPTICS_SHADER_DUMP").is_some_and(|v| v == "1")
    });
    if *OPTICS_DUMP {
        static DUMPED: Mutex<Vec<u64>> = Mutex::new(Vec::new());
        let mut dumped = lock(&DUMPED);
        for b in [blob(ps), vs_bytes].into_iter().flatten() {
            if b.len() > 262_144 {
                continue;
            }
            let hash = fnv1a(b);
            if dumped.len() >= 2048 || dumped.contains(&hash) {
                continue;
            }
            dumped.push(hash);
            sight_ps_dump(b, &format!("cyberpunkvr_optics_{hash:016X}.bin"));
        }
    }
    let sight = CyberpunkVR_SightPsHash.load(Relaxed);
    if ids.ps == sight {
        if let Some(b) = vs_bytes {
            let mut cache = lock(&SIGHT_VS);
            if !cache.iter().any(|e| e.hash == ids.vs) && cache.len() < 8 {
                cache.push(SightVs {
                    hash: ids.vs,
                    bytes: b.to_vec(),
                    written: false,
                });
                log(&format!(
                    "[pso] sight-PS pipeline #{} uses VS {:016X} ({} bytes)",
                    cache.len(),
                    ids.vs,
                    b.len()
                ));
            }
        }
    }
    if CyberpunkVR_SightPsDump.load(Relaxed) != 0 && ids.ps == sight {
        static DONE: AtomicBool = AtomicBool::new(false);
        if DONE.compare_exchange(false, true, SeqCst, SeqCst).is_ok() {
            sight_ps_dump(blob(ps).unwrap_or(&[]), "cyberpunkvr_sight_ps.bin");
        }
    }
    let mut map = lock(&PSO_IDS);
    if map.len() < 65536 {
        map.insert(pso as usize, ids);
    }
}
/// The pixel shader's hash of a known pipeline, or 0.
#[no_mangle]
pub extern "C" fn cvr_census_pso_ps(pso: *mut c_void) -> u64 {
    ids_of(pso).map_or(0, |ids| ids.ps)
}
/// The draw probe: the first four distinct pixel shaders drawing with the probed index count.
#[no_mangle]
pub extern "C" fn cvr_census_draw_probe(pso: *mut c_void) {
    static PROBE: Mutex<()> = Mutex::new(());
    let Some(ids) = ids_of(pso) else { return };
    // SAFETY: exports of the C++ half.
    unsafe {
        CyberpunkVR_DrawProbeHits.fetch_add(1, Relaxed);
        let _lock = lock(&PROBE);
        for i in 0..4 {
            let ps = CyberpunkVR_DrawProbePs[i].load(Relaxed);
            if ps == ids.ps {
                return;
            }
            if ps == 0 {
                CyberpunkVR_DrawProbePs[i].store(ids.ps, Relaxed);
                CyberpunkVR_DrawProbeVs[i].store(ids.vs, Relaxed);
                CyberpunkVR_DrawProbePsLen[i].store(ids.ps_len, Relaxed);
                return;
            }
        }
    }
}

/// The 6-index instanced quads tallied by pixel shader and node: the sight draws exactly ONCE per view per frame,
/// so the row whose count tracks the frame count is it.
#[derive(Clone, Copy)]
struct SightCand {
    ps: u64,
    node_rva: u32,
    ps_len: u32,
    hits: [u64; 2],
}
static SIGHT: Mutex<Vec<SightCand>> = Mutex::new(Vec::new());
fn sight_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 20000 {
        return;
    }
    let cands = lock(&SIGHT).clone();
    if cands.is_empty() {
        return;
    }
    LAST.store(now, Relaxed);
    let mut line = String::new();
    for c in &cands {
        if line.len() < 1200 - 70 {
            line.push_str(&format!(
                "ps={:016X}({})@{:X} M{}/V{}  ",
                c.ps, c.ps_len, c.node_rva, c.hits[0], c.hits[1]
            ));
        }
    }
    log(&format!(
        "[psoprobe] 6-index instanced quads ({}): {line}",
        cands.len()
    ));
}
/// One 6-index instanced quad drawn with `pso` in node `rva`: tallied, the sight's vertex shader in use written out
/// once; the pixel shader's hash returned for the caller to act on (0 when unknown).
#[no_mangle]
pub unsafe extern "C" fn cvr_census_sight_note(pso: *mut c_void, rva: u32, vrcam: bool) -> u64 {
    if pso.is_null() {
        return 0;
    }
    let Some(ids) = ids_of(pso) else { return 0 };
    {
        let mut cands = lock(&SIGHT);
        let i = match cands
            .iter()
            .position(|c| c.ps == ids.ps && c.node_rva == rva)
        {
            Some(i) => i,
            None if cands.len() >= 32 => return ids.ps,
            None => {
                cands.push(SightCand {
                    ps: ids.ps,
                    node_rva: rva,
                    ps_len: ids.ps_len,
                    hits: [0; 2],
                });
                cands.len() - 1
            }
        };
        cands[i].hits[vrcam as usize] += 1;
    }
    if ids.ps == CyberpunkVR_SightPsHash.load(Relaxed) && ids.vs != 0 {
        CyberpunkVR_DebugSightVsUsed.store(ids.vs, Relaxed);
        let mut cache = lock(&SIGHT_VS);
        for e in cache.iter_mut().filter(|e| e.hash == ids.vs && !e.written) {
            e.written = true;
            sight_ps_dump(&e.bytes, "cyberpunkvr_sight_vs.bin");
            log(&format!(
                "[pso] sight VS IN USE {:016X} ({} bytes) -- that is the one to replace",
                e.hash,
                e.bytes.len()
            ));
        }
    }
    sight_report();
    ids.ps
}

// ---- the buffer maps -------------------------------------------------------------------------------------------

/// Buffers by GPU VA as they are created (512 at most): a vertex-buffer view carries only a VA, and buffers are
/// exempt from the state rules, so a copy out of one needs no barrier on the engine's resource.
static BUFS: Mutex<Vec<(u64, u64, usize)>> = Mutex::new(Vec::new());
#[no_mangle]
pub extern "C" fn cvr_census_buf_note(res: *mut c_void, va: u64, size: u64) {
    if res.is_null() || va == 0 || size == 0 {
        return;
    }
    let mut bufs = lock(&BUFS);
    if bufs.len() < 512 {
        bufs.push((va, size, res as usize));
    }
}
/// The buffer holding `need` bytes at `va`, and the offset into it.
///
/// # Safety
/// `off_out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_buf_for_va(
    va: u64,
    need: u64,
    off_out: *mut u64,
) -> *mut c_void {
    for &(base, size, res) in lock(&BUFS).iter() {
        if va >= base && va + need <= base + size {
            *off_out = va - base;
            return res as *mut c_void;
        }
    }
    core::ptr::null_mut()
}
/// The last 32 upload ranges by the VA they were copied to, with the mapped upload bytes behind them.
static FILLED: Mutex<([(u64, u64, usize); 32], u32)> = Mutex::new(([(0, 0, 0); 32], 0));
/// A CopyBufferRegion's destination VA (read guarded: the resource is the engine's) noted with the bytes copied in.
///
/// # Safety
/// `dst` is a resource or a stale pointer (the VA query is guarded).
#[no_mangle]
pub unsafe extern "C" fn cvr_census_filled_note(
    dst: *mut c_void,
    dst_off: u64,
    bytes: u64,
    cpu: *const u8,
) {
    if dst.is_null() {
        return;
    }
    // ID3D12Resource::GetGPUVirtualAddress is vtable slot 11.
    let Some(vtable) = code::read::<usize>(dst as usize) else {
        return;
    };
    let Some(get_va) = code::read::<usize>(vtable + 11 * 8) else {
        return;
    };
    let Some(va) = code::guarded_call4(
        get_va,
        dst,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    ) else {
        return;
    };
    let va = va as u64;
    if va == 0 || cpu.is_null() || bytes == 0 {
        return;
    }
    let mut filled = lock(&FILLED);
    let i = (filled.1 % 32) as usize;
    filled.1 = filled.1.wrapping_add(1);
    filled.0[i] = (va + dst_off, bytes, cpu as usize);
}
/// The CPU bytes behind `need` bytes at `va`, when an upload copied them there.
#[no_mangle]
pub extern "C" fn cvr_census_filled_cpu_for_va(va: u64, need: u64) -> *const u8 {
    for &(base, bytes, cpu) in lock(&FILLED).0.iter() {
        if cpu != 0 && va >= base && va + need <= base + bytes {
            return (cpu + (va - base) as usize) as *const u8;
        }
    }
    core::ptr::null()
}
