//! SettingsRes (formerly src/Hooks/SettingsRes.cpp): the render-settings object as the engine finalises it. Read-only
//! apart from the forced-resolution override the C++ core applies to it: the active and target resolutions are logged
//! with the size the XR runtime recommends, which is a REFERENCE value (printing it beside the others was once misread
//! as evidence of a forced override). Logged on change only.
//!
//! THE PATCH (2.5 sub_1405D9590): `cmp [rcx+18],eax / jne differ / mov eax,[rcx+B4] / cmp [rcx+1C],eax` goes to a stub
//! that saves the flags, the volatile registers and xmm0-xmm3, calls OnSettingsResCallback(rcx) on an aligned stack,
//! restores everything and runs the relocated compare, whose jne still leaves for the native "sizes differ" exit.
use core::ffi::c_void;
use cvr_engine::code::{find_pattern, read};
use cvr_engine::trampoline::{allocate, patch_jump, Code};
use std::sync::Mutex;

extern "C" {
    /// The settings object to the core (g_settingsResPtr, ApplySettingsResolutionOverride); the previous one back.
    fn cvr_cpp_settings_res_apply(settings: usize) -> usize;
    /// GetForcedRenderWidthValue / GetForcedRenderHeightValue.
    fn cvr_cpp_forced_render_size(width: *mut u32, height: *mut u32);
}

const PATTERN: &[u8] = b"\x39\x41\x18\x75\x00\x8B\x81\xB4\x00\x00\x00\x39\x41\x1C";
const MASK: &[u8] = b"xxxx?xxxxxxxxx";
const REPLACE: usize = 14;

struct State {
    hits: u64,
    last: [u32; 4],
}
static STATE: Mutex<State> = Mutex::new(State { hits: 0, last: [0; 4] });

#[no_mangle]
pub unsafe extern "C" fn OnSettingsResCallback(settings_ptr: *mut c_void) {
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    state.hits += 1;
    let settings = settings_ptr as usize;
    if settings < 0x10000 {
        return;
    }
    let previous = cvr_cpp_settings_res_apply(settings);
    if settings == previous && state.hits % 600 != 1 {
        return;
    }
    let field = |at: usize| read::<u32>(settings + at).unwrap_or(0);
    // active at +0x18/+0x1C, target at +0xB0/+0xB4 (2.31: +0x84/+0x88)
    let now = [field(0x18), field(0x1C), field(0xB0), field(0xB4)];
    let changed = now != state.last;
    state.last = now;
    if changed {
        let (mut forced_w, mut forced_h) = (0u32, 0u32);
        cvr_cpp_forced_render_size(&mut forced_w, &mut forced_h);
        super::env::log(&format!(
            "SettingsRes hook: ptr={settings:016X} active={}x{} target={}x{} forced={forced_w}x{forced_h}\n",
            now[0], now[1], now[2], now[3]
        ));
    }
}

/// The stub at `at`, returning to `back` after the relocated compare (its jne to `differ`).
fn stub(at: usize, back: usize, differ: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    // pushfq / push rax, rcx, rdx, r8, r9, r10, r11, rbp
    c.bytes(&[0x9C, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x55]);
    // sub rsp,40h / movups [rsp],xmm0 / [rsp+10h],xmm1 / [rsp+20h],xmm2 / [rsp+30h],xmm3
    c.bytes(&[
        0x48, 0x83, 0xEC, 0x40, 0x0F, 0x11, 0x04, 0x24, 0x0F, 0x11, 0x4C, 0x24, 0x10, 0x0F, 0x11, 0x54, 0x24, 0x20, 0x0F, 0x11, 0x5C, 0x24, 0x30,
    ]);
    // mov rbp,rsp / and rsp,-16 / sub rsp,20h
    c.bytes(&[0x48, 0x89, 0xE5, 0x48, 0x83, 0xE4, 0xF0, 0x48, 0x83, 0xEC, 0x20]);
    c.mov_rax(callback).bytes(&[0xFF, 0xD0]); // call rax (rcx: the settings object)
    c.bytes(&[0x48, 0x89, 0xEC]); // mov rsp,rbp
    c.bytes(&[
        0x0F, 0x10, 0x04, 0x24, 0x0F, 0x10, 0x4C, 0x24, 0x10, 0x0F, 0x10, 0x54, 0x24, 0x20, 0x0F, 0x10, 0x5C, 0x24, 0x30, 0x48, 0x83, 0xC4, 0x40,
    ]);
    // pop rbp, r11, r10, r9, r8, rdx, rcx, rax / popfq
    c.bytes(&[0x5D, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x58, 0x9D]);
    // The relocated compare; its jne leaves for the native "sizes differ" exit, as the original does.
    c.bytes(&[0x39, 0x41, 0x18]);
    let rel = (differ as i64 - (c.here() as i64 + 6)) as i32;
    c.bytes(&[0x0F, 0x85]).bytes(&rel.to_le_bytes());
    c.bytes(&[0x8B, 0x81, 0xB4, 0x00, 0x00, 0x00, 0x39, 0x41, 0x1C]);
    c.jmp(back);
    c
}

/// Installed from src/Hooks/SettingsRes.cpp's registration (Boot, 85).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_settings_res_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else {
        return false;
    };
    let Some(jne) = read::<i8>(found + 4) else { return false };
    let differ = (found as i64 + 5 + jne as i64) as usize;
    let Some(tramp) = allocate(found, 512) else { return false };
    let code = stub(tramp, found + REPLACE, differ, OnSettingsResCallback as *const () as u64);
    // SAFETY: fresh arena memory, then a site whose 14 bytes the stub reproduces.
    code.commit();
    patch_jump(found, REPLACE, tramp);
    true
}
