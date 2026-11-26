//! ForceHeadingUpdate (formerly src/Hooks/ForceHeadingUpdate.cpp): a pure byte patch, nothing of ours runs at the site.
//! It forces the engine to recompute the heading every frame rather than only when its own input says so: the call's
//! result is discarded and `al` cleared, so the caller always takes the recompute branch.
use super::env::log;
use cvr_engine::code::find_pattern;
use cvr_engine::trampoline::{allocate, patch_jump, Code};

/// mov rcx,rbx / movss xmm1,[rdi+1C] / call <rel32> / test al,al
const PATTERN: &[u8] = b"\x48\x8B\xCB\xF3\x0F\x10\x4F\x1C\xE8\x00\x00\x00\x00\x84\xC0";
const MASK: &[u8] = b"xxxxxxxxx????xx";
const REPLACE: usize = 15;

/// The stub at `at`: the two instructions, the call through rax to `call_target`, xor al,al, and back past the site.
pub fn stub(at: usize, site: usize, call_target: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0x48, 0x8B, 0xCB]) // mov rcx,rbx
        .bytes(&[0xF3, 0x0F, 0x10, 0x4F, 0x1C]) // movss xmm1,[rdi+1C]
        .mov_rax(call_target)
        .bytes(&[0xFF, 0xD0]) // call rax
        .bytes(&[0x30, 0xC0]) // xor al,al
        .jmp(site + REPLACE);
    c
}

/// Installed from src/Hooks/ForceHeadingUpdate.cpp's registration (Boot, 75).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_force_heading_update_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 256) else { return false };
    let rel_call = core::ptr::read_unaligned((found + 9) as *const i32);
    let call_target = (found as i64 + 13 + rel_call as i64) as u64;
    let code = stub(tramp, found, call_target);
    code.commit();
    patch_jump(found, REPLACE, tramp);
    log(&format!("ForceHeadingUpdate hook installed at {:016X} target={:016X}\n", found, call_target));
    true
}