//! ProjectionCommit (formerly src/Hooks/ProjectionCommit.cpp; historically the "unifix" site): where the projection
//! block is committed into the render object (2.5 sub_14067B6AC: `movups xmm0,[r13] / movups [rbx+3030h],xmm0`).
//!
//! The load (5 bytes) goes to a stub that saves the flags, the volatile registers and xmm0-xmm3, calls
//! OnUnifixHookCallback(r13, rbx) on an aligned stack, restores everything and runs the load; the store stays in place.
//! The callback keeps the last block, the render object and the hit count for the worker's diagnostics and logs the
//! first sight of a non-zero committed projection. The render object's own copy (rbx+0x3030) is what gameplay uses:
//! the r13 source may be a zeroed template. Reads go through cvr_engine::code (the C++ one was guarded by SEH).
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering};
use cvr_engine::code::{find_pattern, read, write};
use cvr_engine::trampoline::{allocate, patch_jump, Code};

extern "C" {
    static mut g_unifixHits: u64;
    static mut g_unifixRenderObj: usize;
    static mut g_unifixProjDump: [f32; 9];
    static g_normalFovOverrideValue: f32;
}

/// movups xmm0,[r13] / movups [rbx+0x3030],xmm0 (2.31 had two moves between them and stored to +0x21C0).
const PATTERN: &[u8] = b"\x41\x0F\x10\x45\x00\x0F\x11\x83\x30\x30\x00\x00";
const MASK: &[u8] = b"xxxxxxxxxxxx";
/// The load alone; the store executes in place.
const REPLACE: usize = 5;
/// The FOV override at the commit (render object +0x3050) was never switched on; kept for the record.
const ENABLE_OVERRIDE: bool = false;

static SEEN_NONZERO_RBX: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub unsafe extern "C" fn OnUnifixHookCallback(projection: *mut c_void, render: *mut c_void) {
    g_unifixHits += 1;
    let hits = g_unifixHits;
    if !render.is_null() {
        g_unifixRenderObj = render as usize;
    }
    if projection.is_null() {
        return;
    }
    let Some(proj) = read::<[f32; 9]>(projection as usize) else { return };
    g_unifixProjDump = proj;
    let non_zero = proj.iter().any(|&p| p != 0.0);
    let mut rbx = [0.0f32; 9];
    if !render.is_null() {
        let Some(committed) = read::<[f32; 9]>(g_unifixRenderObj + 0x3030) else {
            return;
        };
        rbx = committed;
    }
    let rbx_non_zero = rbx.iter().any(|&p| p != 0.0);
    // The first non-zero committed projection, the first five calls, then every 600th.
    let mut should_log = rbx_non_zero && !SEEN_NONZERO_RBX.swap(true, Ordering::Relaxed);
    if hits <= 5 || (cvr_host::log::verbose() && hits % 600 == 0) {
        should_log = true;
    }
    if should_log {
        let log = super::env::log;
        log(&format!(
            "Unifix: hits={hits} r13zero={} rbxZero={}\n",
            (!non_zero) as i32,
            (!rbx_non_zero) as i32
        ));
        if rbx_non_zero {
            let p = rbx.map(|v| v as f64);
            log(&format!(
                "UnifixRender: rbx={:016X} p0-3=[{:.6} {:.6} {:.6} {:.6}] p4-7=[{:.6} {:.6} {:.6} {:.6}] fov={:.6}\n",
                render as usize, p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8]
            ));
        }
    }
    if ENABLE_OVERRIDE && rbx_non_zero {
        let desired = g_normalFovOverrideValue;
        let at = g_unifixRenderObj + 0x3050;
        if desired > 1.0 && desired < 179.0 && read::<f32>(at).is_some_and(|f| f != desired) {
            write::<f32>(at, desired);
        }
    }
}

/// The stub at `at`: the call frame around the callback (rcx = r13, rdx = rbx), then the load, back to `back`.
fn stub(at: usize, back: usize, callback: u64) -> Code {
    let mut c = Code::new(at);
    c.bytes(&[0x9C, 0x50, 0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x41, 0x52, 0x41, 0x53, 0x55]);
    c.bytes(&[
        0x48, 0x83, 0xEC, 0x40, 0x0F, 0x11, 0x04, 0x24, 0x0F, 0x11, 0x4C, 0x24, 0x10, 0x0F, 0x11, 0x54, 0x24, 0x20, 0x0F, 0x11, 0x5C, 0x24, 0x30,
    ]);
    c.bytes(&[0x48, 0x89, 0xE5, 0x48, 0x83, 0xE4, 0xF0, 0x48, 0x83, 0xEC, 0x20]);
    c.bytes(&[0x4C, 0x89, 0xE9, 0x48, 0x89, 0xDA]); // mov rcx,r13 / mov rdx,rbx
    c.mov_rax(callback).bytes(&[0xFF, 0xD0]);
    c.bytes(&[0x48, 0x89, 0xEC]);
    c.bytes(&[
        0x0F, 0x10, 0x04, 0x24, 0x0F, 0x10, 0x4C, 0x24, 0x10, 0x0F, 0x10, 0x54, 0x24, 0x20, 0x0F, 0x10, 0x5C, 0x24, 0x30, 0x48, 0x83, 0xC4, 0x40,
    ]);
    c.bytes(&[0x5D, 0x41, 0x5B, 0x41, 0x5A, 0x41, 0x59, 0x41, 0x58, 0x5A, 0x59, 0x58, 0x9D]);
    c.bytes(&[0x41, 0x0F, 0x10, 0x45, 0x00]); // movups xmm0,[r13]
    c.jmp(back);
    c
}

/// Installed from src/Hooks/ProjectionCommit.cpp's registration (Boot, 45).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_projection_commit_install() -> bool {
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else {
        return false;
    };
    let Some(tramp) = allocate(found, 512) else { return false };
    let code = stub(tramp, found + REPLACE, OnUnifixHookCallback as *const () as u64);
    // SAFETY: fresh arena memory, then a site whose 5 bytes the stub reproduces.
    code.commit();
    patch_jump(found, REPLACE, tramp);
    true
}
