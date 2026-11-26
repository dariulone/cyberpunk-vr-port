//! FreeDeltaHead (formerly src/Hooks/FreeDeltaHead.cpp): the free camera's heading delta, a byte patch with no
//! callback of ours. The original store runs first, then the stub counts the hit and records rsi and the stored
//! scalar into the telemetry block (Core/Telemetry.hpp) -- TELEMETRY ONLY: this path crashed when the scalar was
//! rewritten. The telemetry address is baked into the stub at install time.
use cvr_engine::code::find_pattern;
use cvr_engine::trampoline::{allocate, patch_jump, Code};

extern "C" {
    // src/Hooks/HookServices.cpp: g_telemetry + kFreeDeltaTelemetryOffset
    fn cvr_cpp_telemetry_free_delta() -> usize;
}

/// movss [rsi+0CCCh],xmm3 / je
const PATTERN: &[u8] = b"\xF3\x0F\x11\x9E\xCC\x0C\x00\x00\x74\x5C";
const MASK: &[u8] = b"xxxxxxxxxx";
const REPLACE: usize = 8;

pub fn stub(at: usize, site: usize, telemetry: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0xF3, 0x0F, 0x11, 0x9E, 0xCC, 0x0C, 0x00, 0x00]) // the original movss
        .bytes(&[0x50]) // push rax
        .mov_rax(telemetry)
        .bytes(&[0xFF, 0x00]) // inc dword [rax]
        .bytes(&[0x48, 0x89, 0x70, 0x08]) // mov [rax+8],rsi
        .bytes(&[0xF3, 0x0F, 0x11, 0x58, 0x10]) // movss [rax+10h],xmm3
        .bytes(&[0x58]) // pop rax
        .jmp(site + REPLACE);
    c
}

/// Installed from src/Hooks/FreeDeltaHead.cpp's registration (Boot, 77).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_free_delta_head_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 256) else { return false };
    stub(tramp, found, cvr_cpp_telemetry_free_delta() as u64).commit();
    patch_jump(found, REPLACE, tramp);
    true
}