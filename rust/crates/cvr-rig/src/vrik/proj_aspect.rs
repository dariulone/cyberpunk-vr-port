//! ProjAspectCopy and ProjAspectCall (formerly src/Hooks/ProjAspectCopy.cpp and ProjAspectCall.cpp): the projection
//! block copy (exe+0x2129F8) and the three call sites that pass fov and aspect by offset (0x59D66A, 0x59D97E, 0x59D8EC).
//! A 16:9 main-camera projection gets aspect 1 and, when its fov is the 16:9 horizontal, the matching square vertical.
//!
//! REGISTERED, BUT NOT INSTALLED: these write the engine's projection fov and aspect, and the last time that was got
//! wrong the world came out "too big" in both eyes. The gates (src/Hooks/ProjAspectCopy.cpp, ProjAspectCall.cpp keep
//! the registrations) answer no, preserving the disabled state; the registry reports them as skipped.
use core::sync::atomic::{AtomicU64, Ordering};
use cvr_body::cmath::{atan, tan};
use cvr_crt::{format as cformat, Arg};
use cvr_engine::code::{image_base, read, read_bytes, write};
use cvr_engine::trampoline::{allocate, patch_jump, Code};

extern "C" {
    fn cvr_cpp_verbose_log() -> i32;
}

static COPY_HITS: AtomicU64 = AtomicU64::new(0);
static CALL_HITS: AtomicU64 = AtomicU64::new(0);

fn f6(v: f32) -> String {
    cformat(c"%.6f", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}
/// The shadow-map guess both callbacks share: an orthographic or very wide fov, an aspect far from 16:9, or a 16:9 fov
/// outside the VR camera's ranges.
fn is_shadow_map(fov: f32, aspect: f32, aspect_16x9: bool, looks_like_view: bool) -> bool {
    let mut shadow = fov <= 1.0 || fov >= 179.0;
    if !shadow && !aspect_16x9 && (aspect < 1.5 || aspect > 2.0) {
        shadow = true;
    }
    if !shadow && looks_like_view {
        shadow = !((fov > 80.0 && fov < 110.0) || (fov > 120.0 && fov < 170.0));
    }
    shadow
}
/// The 16:9 horizontal (~132.5 degrees) made the square vertical; other values kept.
fn square_vertical(fov: f32) -> f32 {
    if fov > 120.0 && fov < 170.0 {
        let half = fov * 0.5 * 3.141_592_653_5 / 180.0;
        atan(tan(half) * (9.0 / 16.0)) * 2.0 * 180.0 / 3.141_592_653_5
    } else {
        fov
    }
}

/// rcx = the destination block, rdx = the source block, after the original copy of +0x80..+0x8F.
#[no_mangle]
pub unsafe extern "C" fn OnProjAspectCopyCallback(dst: usize, src: usize) {
    let hits = COPY_HITS.fetch_add(1, Ordering::Relaxed) + 1;
    if dst == 0 || src == 0 {
        return;
    }
    let (Some(src_fov), Some(src_aspect), Some(mut dst_fov), Some(mut dst_aspect)) = (
        read::<f32>(src + 0x80),
        read::<f32>(src + 0x84),
        read::<f32>(dst + 0x80),
        read::<f32>(dst + 0x84),
    ) else {
        return;
    };
    let aspect_16x9 = src_aspect > 1.70 && src_aspect < 1.85;
    let looks_like_view = aspect_16x9 && src_fov > 30.0 && src_fov < 180.0;
    let shadow = is_shadow_map(src_fov, src_aspect, aspect_16x9, looks_like_view);
    let mut patched = false;
    if looks_like_view && !shadow {
        let fov = square_vertical(src_fov);
        if !write::<f32>(dst + 0x80, fov) || !write::<f32>(dst + 0x84, 1.0) {
            return;
        }
        dst_fov = fov;
        dst_aspect = 1.0;
        patched = true;
    }
    if hits <= 20 || hits % 600 == 0 || patched || shadow {
        super::env::log(&format!(
            "ProjAspect: hits={hits} srcFov={} srcAspect={} -> dstFov={} dstAspect={} patched={} isShadowMap={}\n",
            f6(src_fov),
            f6(src_aspect),
            f6(dst_fov),
            f6(dst_aspect),
            patched as i32,
            shadow as i32
        ));
    }
}

/// rcx = the source (rdx at the site), edx/r8d = the fov's and the aspect's offsets, r9d = the site.
#[no_mangle]
pub unsafe extern "C" fn OnProjAspectCallCallback(src: usize, fov_off: u32, aspect_off: u32, _site: u32) {
    let hits = CALL_HITS.fetch_add(1, Ordering::Relaxed) + 1;
    if src == 0 {
        return;
    }
    let (fov_at, aspect_at) = (src + fov_off as usize, src + aspect_off as usize);
    let (Some(fov), Some(aspect)) = (read::<f32>(fov_at), read::<f32>(aspect_at)) else {
        return;
    };
    let (mut last_fov, mut last_aspect, mut patched) = (fov, aspect, false);
    let fov_valid = fov.is_finite() && fov > 30.0 && fov < 180.0;
    let aspect_16x9 = aspect.is_finite() && aspect > 1.70 && aspect < 1.85;
    let shadow = is_shadow_map(fov, aspect, aspect_16x9, fov_valid && aspect_16x9);
    if fov_valid && aspect_16x9 && !shadow {
        let patched_fov = square_vertical(fov);
        if fov > 120.0 && fov < 170.0 && !write::<f32>(fov_at, patched_fov) {
            return;
        }
        if !write::<f32>(aspect_at, 1.0) {
            return;
        }
        last_fov = patched_fov;
        last_aspect = 1.0;
        patched = true;
    }
    if hits <= 20 || hits % 600 == 0 || patched || shadow {
        super::env::log(&format!(
            "ProjAspectCall: hits={hits} fovOff=0x{fov_off:X} aspectOff=0x{aspect_off:X} fov={} aspect={} patched={} isShadowMap={}\n",
            f6(last_fov),
            f6(last_aspect),
            patched as i32,
            shadow as i32
        ));
    }
}

/// The gates: both stay off (see the module's note).
#[no_mangle]
pub extern "C" fn cvr_hook_proj_aspect_copy_wanted() -> bool {
    false
}
#[no_mangle]
pub extern "C" fn cvr_hook_proj_aspect_call_wanted() -> bool {
    false
}

fn verify(rva: usize, expected: &[u8], what: &str) -> Option<usize> {
    let found = image_base() + rva;
    let mut got = vec![0u8; expected.len()];
    if !read_bytes(found, &mut got) {
        return None;
    }
    if let Some(i) = got.iter().zip(expected).position(|(a, b)| a != b) {
        // SAFETY: a C++ service.
        if unsafe { cvr_cpp_verbose_log() } != 0 {
            super::env::log(&format!(
                "{what}: RVA 0x{rva:X} mismatch at +0x{i:X} (got {:02X} expected {:02X})\n",
                got[i], expected[i]
            ));
        }
        return None;
    }
    Some(found)
}

/// The block copy at 0x2129F8 (Boot, 50): the original copy, then the callback with its registers.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_proj_aspect_copy_install() -> bool {
    const EXPECTED: [u8; 14] = [0x0F, 0x10, 0x8A, 0x80, 0x00, 0x00, 0x00, 0x0F, 0x11, 0x89, 0x80, 0x00, 0x00, 0x00];
    let Some(found) = verify(0x2129F8, &EXPECTED, "ProjAspect hook") else {
        return false;
    };
    let Some(tramp) = allocate(found, 512) else { return false };
    let mut c = Code::new(tramp);
    c.bytes(&EXPECTED)
        .volatile_call(4, &[], OnProjAspectCopyCallback as *const () as u64)
        .jmp(found + EXPECTED.len());
    c.commit();
    patch_jump(found, EXPECTED.len(), tramp);
    true
}

unsafe fn install_call_site(rva: usize, expected: &[u8], fov_off: u32, aspect_off: u32, site: u32) -> bool {
    let Some(found) = verify(rva, expected, "ProjAspectCall hook") else {
        return false;
    };
    let Some(tramp) = allocate(found, 512) else { return false };
    let mut arg = vec![0x48, 0x89, 0xD1]; // mov rcx, rdx
    arg.push(0xBA); // mov edx, imm32
    arg.extend_from_slice(&fov_off.to_le_bytes());
    arg.extend_from_slice(&[0x41, 0xB8]); // mov r8d, imm32
    arg.extend_from_slice(&aspect_off.to_le_bytes());
    arg.extend_from_slice(&[0x41, 0xB9]); // mov r9d, imm32
    arg.extend_from_slice(&site.to_le_bytes());
    let mut c = Code::new(tramp);
    c.volatile_call(4, &arg, OnProjAspectCallCallback as *const () as u64)
        .bytes(expected)
        .jmp(found + expected.len());
    c.commit();
    patch_jump(found, expected.len(), tramp);
    true
}
/// The three call sites (Boot, 51).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_proj_aspect_call_install() -> bool {
    const A: [u8; 16] = [0xF3, 0x0F, 0x10, 0x92, 0x84, 0x00, 0x00, 0x00, 0xF3, 0x0F, 0x10, 0x8A, 0x80, 0x00, 0x00, 0x00];
    const B: [u8; 10] = [0xF3, 0x0F, 0x10, 0x52, 0x7C, 0xF3, 0x0F, 0x10, 0x4A, 0x78];
    let mut ok = install_call_site(0x59D66A, &A, 0x80, 0x84, 1);
    ok |= install_call_site(0x59D97E, &A, 0x80, 0x84, 2);
    ok |= install_call_site(0x59D8EC, &B, 0x78, 0x7C, 3);
    ok
}
