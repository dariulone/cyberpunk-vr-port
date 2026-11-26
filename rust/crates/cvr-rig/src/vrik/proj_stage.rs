//! ProjStage (formerly src/Hooks/ProjStage.cpp): the projection staging copy, one of the sites that carry FOV and
//! aspect into the engine's projection block (2.5 RVA 0x5FBD37: `movss xmm4,[rdx+80h] / movss xmm5,[rdx+84h] /
//! movss xmm6,[rdx+88h]`).
//!
//! The three loads go to a stub that calls OnProjStageCallback(rdx) inside the usual call frame, reruns them, then
//! loads xmm4 and xmm5 from g_projStageFov / g_projStageAspect: the callback leaves the FOV and puts aspect 1.0 into the
//! VR camera's 16:9 staging (shadow maps -- an implausible FOV, or 16:9 with a FOV outside the VR camera's -- keep
//! theirs). xmm6 stays the source's. Reads go through cvr_engine::code (the C++ one was guarded by SEH).
use core::ffi::c_void;
use cvr_engine::code::{read, read_bytes, rva};
use cvr_engine::trampoline::{allocate, patch_jump, Code};

extern "C" {
    static mut g_projStageHits: u64;
    static mut g_projStageFov: f32;
    static mut g_projStageAspect: f32;
    static mut g_projStageExtra: f32;
    static mut g_projStagePatched: bool;
}

const SITE_RVA: usize = 0x5FBD37; // 2.31: 0x127970
const EXPECTED: [u8; 24] = [
    0xF3, 0x0F, 0x10, 0xA2, 0x80, 0x00, 0x00, 0x00, // movss xmm4,[rdx+80h]
    0xF3, 0x0F, 0x10, 0xAA, 0x84, 0x00, 0x00, 0x00, // movss xmm5,[rdx+84h]
    0xF3, 0x0F, 0x10, 0xB2, 0x88, 0x00, 0x00, 0x00, // movss xmm6,[rdx+88h]
];

#[no_mangle]
pub unsafe extern "C" fn OnProjStageCallback(src: *const c_void) {
    g_projStageHits += 1;
    g_projStagePatched = false;
    if src.is_null() {
        return;
    }
    let Some([fov, aspect, extra]) = read::<[f32; 3]>(src as usize + 0x80) else {
        return;
    };
    g_projStageFov = fov;
    g_projStageAspect = aspect;
    g_projStageExtra = extra;
    // Shadow maps: a tiny or zero FOV, or 16:9 with a FOV that is not the VR camera's (~93 degrees).
    let mut shadow_map = fov <= 1.0 || fov >= 179.0;
    if !shadow_map && aspect > 1.70 && aspect < 1.85 {
        shadow_map = !(fov > 80.0 && fov < 110.0);
    }
    if !shadow_map && fov.is_finite() && aspect.is_finite() && fov > 30.0 && fov < 180.0 && aspect > 1.70 && aspect < 1.85 {
        g_projStageAspect = 1.0;
        g_projStagePatched = true;
    }
    let hits = g_projStageHits;
    if hits <= 20 || hits % 600 == 0 || g_projStagePatched {
        super::env::log(&format!(
            "ProjStage: hits={hits} fov={:.6} aspect={:.6} extra={:.6} patched={} isShadowMap={}\n",
            g_projStageFov as f64, g_projStageAspect as f64, g_projStageExtra as f64, g_projStagePatched as i32, shadow_map as i32
        ));
    }
}

fn stub(at: usize, back: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0x9C, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x55]);
    c.bytes(&[
        0x48, 0x83, 0xEC, 0x40, 0x0F, 0x11, 0x04, 0x24, 0x0F, 0x11, 0x4C, 0x24, 0x10, 0x0F, 0x11, 0x54, 0x24, 0x20, 0x0F, 0x11, 0x5C, 0x24, 0x30,
    ]);
    c.bytes(&[0x48, 0x89, 0xE5, 0x48, 0x83, 0xE4, 0xF0, 0x48, 0x83, 0xEC, 0x20]);
    c.bytes(&[0x48, 0x89, 0xD1]); // mov rcx,rdx (the source struct)
    c.mov_rax(callback).bytes(&[0xFF, 0xD0]);
    c.bytes(&[0x48, 0x89, 0xEC]);
    c.bytes(&[
        0x0F, 0x10, 0x04, 0x24, 0x0F, 0x10, 0x4C, 0x24, 0x10, 0x0F, 0x10, 0x54, 0x24, 0x20, 0x0F, 0x10, 0x5C, 0x24, 0x30, 0x48, 0x83, 0xC4, 0x40,
    ]);
    c.bytes(&[0x5D, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x58, 0x9D]);
    c.bytes(&EXPECTED); // the original loads
                        // xmm4 and xmm5 from the globals (xmm6 stays the source's +88h)
                        // The addresses of the C++ half's globals, for the stub to read.
    let (fov, aspect) = (core::ptr::addr_of!(g_projStageFov) as u64, core::ptr::addr_of!(g_projStageAspect) as u64);
    c.mov_rax(fov).bytes(&[0xF3, 0x0F, 0x10, 0x20]); // movss xmm4,[rax]
    c.mov_rax(aspect).bytes(&[0xF3, 0x0F, 0x10, 0x28]); // movss xmm5,[rax]
    c.jmp(back);
    c
}

/// Installed from src/Hooks/ProjStage.cpp's registration (Boot, 55); refused unless the three loads are there.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_proj_stage_install() -> bool {
    let found = rva(SITE_RVA);
    let mut bytes = [0u8; 24];
    if !read_bytes(found, &mut bytes) || bytes != EXPECTED {
        return false;
    }
    let Some(tramp) = allocate(found, 512) else { return false };
    let code = stub(tramp, found + EXPECTED.len(), OnProjStageCallback as *const () as u64);
    // SAFETY: fresh arena memory, then a site whose 24 bytes the stub reproduces.
    code.commit();
    patch_jump(found, EXPECTED.len(), tramp);
    true
}
