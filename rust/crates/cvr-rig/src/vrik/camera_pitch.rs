//! CameraPitch (formerly src/Hooks/CameraPitch.cpp): the camera pitch the engine is about to apply.
//!
//! In VR the head owns pitch, so the mouse must not also write it: mouse-Y pitch in CP2077 also drives a constrained
//! camera pivot offset, which moves the head toward or away from the body. BUT ONLY WHEN THE OPTION SAYS SO
//! ("Disable Mouse Y", dabinn's TofuExpress 11974ee5): with it off the game's pitch is kept and composed with the HMD
//! orientation at both write sites, through g_gamePitchRadians -- the pitch the game will actually settle at, after
//! its clamp, whose unit is inferred from the clamp itself (a limit past 3.2 cannot be radians).
use super::xinput::InputControls;
use cvr_engine::code::{find_pattern, read};
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

extern "C" {
    static mut g_gamePitchRadians: f32;
    fn cvr_cpp_verbose_log() -> i32;
}

static HITS: AtomicU64 = AtomicU64::new(0);
/// The pitch the stub loads into xmm0 before the game's clamp. Its address is baked into the stub.
static OVERRIDE: AtomicU32 = AtomicU32::new(0);

/// rcx = the pitch state (rdi at the site), xmm1 = the game's pitch.
#[no_mangle]
pub unsafe extern "C" fn OnPitchHookCallback(pitch_state: usize, original_pitch: f32) {
    let hits = HITS.fetch_add(1, Ordering::Relaxed) + 1;
    let mut controls = InputControls::default();
    // SAFETY: a C++ service filling a local.
    unsafe { super::live_reads::input_controls(&mut controls) };
    let desired = if controls.disable_mouse_y != 0 { 0.0 } else { original_pitch };
    let min_pitch = read::<f32>(pitch_state.wrapping_add(0x14)).unwrap_or(0.0);
    let max_pitch = read::<f32>(pitch_state.wrapping_add(0x18)).unwrap_or(0.0);
    // (std::max)(min, (std::min)(max, desired))
    let upper = if desired < max_pitch { desired } else { max_pitch };
    let clamped = if min_pitch < upper { upper } else { min_pitch };
    let degrees = min_pitch.abs() > 3.2 || max_pitch.abs() > 3.2;
    // SAFETY: the plugin's published pitch (a volatile float).
    unsafe { core::ptr::write_volatile(&raw mut g_gamePitchRadians, clamped * if degrees { 0.017_453_292_52 } else { 1.0 }) };
    OVERRIDE.store(desired.to_bits(), Ordering::Relaxed);
    // SAFETY: a C++ service.
    if unsafe { cvr_cpp_verbose_log() } != 0 && hits % 600 == 1 {
        super::env::log(&format!(
            "Pitch hook: state={pitch_state:#x} original={original_pitch:.6} desired={desired:.6} clamp=[{min_pitch:.6}, {max_pitch:.6}]\n"
        ));
    }
}

/// movss xmm1,[rdi+14h] / maxss xmm1,xmm0 / minss xmm1,[rdi+18h]: the game's clamp of the pitch in xmm0.
const PATTERN: &[u8] = b"\xF3\x0F\x10\x4F\x14\xF3\x0F\x5F\xC8\xF3\x0F\x5D\x4F\x18";
const MASK: &[u8] = b"xxxxxxxxxxxxxx";
const REPLACE: usize = 14;

/// The stub: the callback (rcx = rdi, xmm1 = xmm0), then xmm0 = the override and the original clamp.
pub fn stub(at: usize, site: usize, callback: u64, override_value: u64) -> Code {
    let mut c = Code::new(at);
    c.volatile_call(4, &[0x48, 0x89, 0xF9, 0x0F, 0x28, 0xC8], callback) // mov rcx,rdi / movaps xmm1,xmm0
        .mov_rax(override_value)
        .bytes(&[0xF3, 0x0F, 0x10, 0x00]) // movss xmm0,[rax]
        .bytes(PATTERN)
        .jmp(site + REPLACE);
    c
}

/// Installed from src/Hooks/CameraPitch.cpp's registration (Boot, 30).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_camera_pitch_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 512) else { return false };
    let code = stub(tramp, found, OnPitchHookCallback as *const () as u64, &OVERRIDE as *const AtomicU32 as u64);
    // SAFETY: fresh arena memory, then a site whose 14 bytes the stub reproduces.
    unsafe {
        code.commit();
        patch_jump(found, REPLACE, tramp);
    }
    true
}
