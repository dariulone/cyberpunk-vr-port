//! The world markers' vertex shaders (formerly src/Render/WorldMarkerShader.cpp): the engine's flat ink vertex shaders
//! of 2.5, recognised by container length and DXBC digest, each with its replacement from Render/WorldMarkers.hlsl
//! (compiled by DXC at build time -- the native pixel shaders are SM6 and D3D12 rejects a pipeline mixing them with
//! FXC/SM5 vertex shaders). Every pipeline of the family is replaced when it is created, so the first one built proves
//! the path; any rejection turns tagging off for good.
//!
//! The plugin's table is `plugin::table()`: the replacements made from Rust (rust/shaders/passes/src/world_markers.rs,
//! embedded by cvr-gpu), or with CVR_SHADER_HLSL the DXIL build.rs compiles from the HLSL as the reference build's CMake
//! step does; a test program attaches its own (formerly src/Render/WorldMarkerShader.cpp's, which the C++ build keeps).
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use cvr_host::log;

/// One variant: the native shader's length and digest, and its replacement.
#[repr(C)]
pub struct Variant {
    pub length: u32,
    pub digest: [u8; 16],
    pub code: *const u8,
    pub size: usize,
}
static VARIANTS: AtomicPtr<Variant> = AtomicPtr::new(core::ptr::null_mut());
static COUNT: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicU32 = AtomicU32::new(0);
static FAILED: AtomicBool = AtomicBool::new(false);

fn variants() -> &'static [Variant] {
    let p = VARIANTS.load(Ordering::Acquire);
    if p.is_null() {
        #[cfg(all(feature = "rust-build", not(test)))]
        return &plugin::table().0;
        #[cfg(not(all(feature = "rust-build", not(test))))]
        return &[];
    } else {
        // SAFETY: the C++ half's table of COUNT rows, alive for the process.
        unsafe { core::slice::from_raw_parts(p, COUNT.load(Ordering::Acquire)) }
    }
}
/// # Safety
/// `table` is `count` rows alive for the process (the first two the screen sprite and the procedural shape).
#[no_mangle]
pub unsafe extern "C" fn cvr_world_marker_shaders_attach(table: *const Variant, count: usize) {
    COUNT.store(count, Ordering::Release);
    VARIANTS.store(table as *mut Variant, Ordering::Release);
}

/// The plugin's six variants: the native shader's container length and DXBC digest, and its replacement -- made from
/// Rust, or with CVR_SHADER_HLSL compiled from src/Render/WorldMarkers.hlsl by build.rs (the screen sprite and the
/// procedural shape first).
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::Variant;
    mod dxil {
        include!(concat!(env!("OUT_DIR"), "/world_marker_dxil.rs"));
    }
    pub(super) struct Table(pub(super) [Variant; 6]);
    // SAFETY: the rows point at immutable statics.
    unsafe impl Sync for Table {}
    // SAFETY: as Sync: pointers to immutable statics, valid on any thread.
    unsafe impl Send for Table {}
    fn row(length: u32, digest: [u8; 16], rust: &'static [u8], hlsl: &'static [u8]) -> Variant {
        let code = if cvr_gpu::dxil::hlsl_for(rust) { hlsl } else { rust };
        Variant { length, digest, code: code.as_ptr(), size: code.len() }
    }
    pub(super) fn table() -> &'static Table {
        use cvr_gpu::dxil as rust;
        static TABLE: std::sync::OnceLock<Table> = std::sync::OnceLock::new();
        TABLE.get_or_init(|| {
            Table([
                row(3042, [0x01, 0x6a, 0xd1, 0x94, 0x18, 0x9b, 0x83, 0x51, 0xb5, 0xaf, 0x0b, 0xdf, 0xfe, 0x34, 0x7d, 0x12], rust::WORLD_MARKER_SPRITE_VS, &dxil::SPRITE.0),
                row(3442, [0x22, 0xb0, 0x65, 0x57, 0x3e, 0xc4, 0xc7, 0x63, 0x8e, 0xe3, 0xf3, 0xa7, 0xc0, 0x32, 0x9b, 0x9b], rust::WORLD_MARKER_PROCEDURAL_VS, &dxil::PROCEDURAL.0),
                row(3350, [0x4f, 0xe4, 0x56, 0x79, 0x98, 0x5b, 0x38, 0x49, 0x4a, 0xd6, 0xcf, 0x1e, 0xc8, 0x2e, 0xc2, 0xc8], rust::WORLD_MARKER_SPRITE_STEREO_VS, &dxil::SPRITESTEREO.0),
                row(3350, [0x3f, 0x0e, 0x72, 0xc5, 0x31, 0x8f, 0xe2, 0xf6, 0x41, 0x86, 0x69, 0xe1, 0xe9, 0xc2, 0x5d, 0x9b], rust::WORLD_MARKER_TEXT_STEREO_VS, &dxil::TEXTSTEREO.0),
                row(3350, [0x5b, 0x31, 0xe8, 0x62, 0x62, 0xdc, 0x37, 0x12, 0x03, 0xbd, 0x2d, 0x20, 0x19, 0xaf, 0xd6, 0x34], rust::WORLD_MARKER_SPRITE_STEREO_Z_VS, &dxil::SPRITESTEREOZ.0),
                row(3302, [0xad, 0x30, 0x45, 0x79, 0x5b, 0x73, 0xd0, 0x3c, 0x25, 0xc7, 0x85, 0xe6, 0x1f, 0x68, 0x04, 0x4e], rust::WORLD_MARKER_SPRITE_STEREO_ONE_VS, &dxil::SPRITESTEREOONE.0),
            ])
        })
    }
}

fn variant_of(code: &[u8]) -> Option<usize> {
    if code.len() < 20 || &code[..4] != b"DXBC" {
        return None;
    }
    variants().iter().position(|v| code.len() == v.length as usize && code[4..20] == v.digest)
}
fn bytes(v: &Variant) -> (*const u8, usize) {
    (v.code, v.size)
}

/// VertexShaderCode: the replacement of the screen sprite (or the procedural shape).
pub fn vertex_shader_code(procedural: bool) -> Option<(*const u8, usize)> {
    variants().get(if procedural { 1 } else { 0 }).map(bytes)
}
/// VertexShaderReplacement: the replacement of a recognised native shader.
pub fn vertex_shader_replacement(original: &[u8]) -> Option<(*const u8, usize)> {
    variant_of(original).map(|i| bytes(&variants()[i]))
}
/// VertexShaderResult: whether the pipeline with the replacement was created.
pub fn vertex_shader_result(original: &[u8], success: bool) {
    let Some(variant) = variant_of(original) else { return };
    if !success {
        FAILED.store(true, Ordering::Release);
        log!("[world-markers] VS pipeline rejected; tagging disabled\n");
    } else {
        let bit = 1u32 << variant;
        if INSTALLED.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            log!("[world-markers] native ink VS variant {} installed\n", variant);
        }
    }
}
/// VertexShadersReady: a replacement went in and none was rejected.
pub fn vertex_shaders_ready() -> bool {
    INSTALLED.load(Ordering::Acquire) != 0 && !FAILED.load(Ordering::Acquire)
}

// ---- the C ABI (include/Render/WorldMarkerShader.hpp)

/// # Safety
/// `code` is `size` readable bytes or null.
unsafe fn slice<'a>(code: *const c_void, size: usize) -> &'a [u8] {
    if code.is_null() {
        &[]
    } else {
        // SAFETY: the caller's bytecode.
        unsafe { core::slice::from_raw_parts(code as *const u8, size) }
    }
}
/// # Safety
/// `out`/`out_size` are writable.
unsafe fn answer(found: Option<(*const u8, usize)>, out: *mut *const c_void, out_size: *mut usize) -> bool {
    let (p, n) = found.unwrap_or((core::ptr::null(), 0));
    // SAFETY: the caller's out-pointers.
    unsafe {
        if let Some(o) = out.as_mut() {
            *o = p as *const c_void;
        }
        if let Some(o) = out_size.as_mut() {
            *o = n;
        }
    }
    found.is_some()
}
/// # Safety
/// `out`/`out_size` are writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_world_marker_vertex_shader_code(procedural: bool, out: *mut *const c_void, out_size: *mut usize) {
    // SAFETY: the caller's out-pointers.
    unsafe { answer(vertex_shader_code(procedural), out, out_size) };
}
/// # Safety
/// `code` is `size` readable bytes or null; `out`/`out_size` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_world_marker_vertex_shader_replacement(code: *const c_void, size: usize, out: *mut *const c_void, out_size: *mut usize) -> bool {
    // SAFETY: the caller's buffers.
    unsafe { answer(vertex_shader_replacement(slice(code, size)), out, out_size) }
}
/// # Safety
/// `code` is `size` readable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_world_marker_vertex_shader_result(code: *const c_void, size: usize, success: bool) {
    // SAFETY: the caller's bytecode.
    vertex_shader_result(unsafe { slice(code, size) }, success)
}
#[no_mangle]
pub extern "C" fn cvr_world_marker_vertex_shaders_ready() -> bool {
    vertex_shaders_ready()
}
