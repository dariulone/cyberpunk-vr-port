//! The camera scope's render graph built with MAIN's dependencies, not as one chain (formerly
//! src/Stereo/ScopeGraphDag.cpp; the file keeps the exported switch and state and the Boot hook's registration).
//!
//! The scene graph builder sub_142276E60(owner, graph, builder, features = view+0x25B0, view data) adds a view's passes
//! and then wires the jobs that run them. With feature bit 64 (the bit that also adds the RTT colour extraction pass) it
//! calls sub_1408EA568, which makes every node wait for the one before it, and skips its dependency rules (0x14227B6FF:
//! sub_1403C1C00(features, 64)). The scope's view is an RTT view with that bit, so its 63 nodes ran as one chain while
//! MAIN's 87 form a graph 23 deep and 23 wide. With xr_scope_graph_dag (CyberpunkVR_ScopeGraphDag, default on) that one
//! call answers no when the view being built is the scope's (its camera name, [features-2588h]), and the builder takes
//! its dependency rules. The call goes to a stub rather than a detour of sub_1403C1C00: callers keep values in volatile
//! registers across that leaf (0x1405C4B67 keeps r9), so only a stub that touches nothing but rax and the flags, as the
//! function itself, can stand in for it. A graph is built when its view's features change, so the switch applies to
//! the next build.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering::*};
use cvr_engine::trampoline;

#[allow(non_upper_case_globals)]
extern "C" {
    /// 1 = the scope's graph gets MAIN's dependency rules; read by the stub, live.
    static CyberpunkVR_ScopeGraphDag: u32;
    /// 0 not installed, 1 installed, 2 the site did not match.
    static CyberpunkVR_ScopeGraphDagState: AtomicU32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

/// call sub_1403C1C00, in sub_142276E60.
const SERIAL_TEST_CALL_RVA: usize = 0x227B6FF;
const FEATURE_TEST_RVA: usize = 0x3C1C00;
/// mov edx, 40h ; mov rcx, r12 (the call follows).
const EXPECTED: [u8; 8] = [0xBA, 0x40, 0x00, 0x00, 0x00, 0x49, 0x8B, 0xCC];

/// The engine's CName hash (FNV-1a 64), as src/Stereo/VrcamConfig.hpp's cname_hash.
const fn cname_hash(s: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}
const SCOPE_CAMERA: u64 = cname_hash(b"vr_optics_feed");

fn mov_rax(code: &mut Vec<u8>, value: u64) {
    code.extend_from_slice(&[0x48, 0xB8]);
    code.extend_from_slice(&value.to_le_bytes());
}

/// The Boot hook (src/Stereo/ScopeGraphDag.cpp's CVR_HOOK line names it): the builder's call of the feature test
/// re-targeted to the stub.
///
/// # Safety
/// Called once by the install pass, in the game's process, before the builder runs.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_scope_graph_dag_install() -> bool {
    let state = &*(&raw const CyberpunkVR_ScopeGraphDagState);
    let exe = GetModuleHandleW(core::ptr::null()) as usize;
    let site = exe + SERIAL_TEST_CALL_RVA;
    let before = core::slice::from_raw_parts((site - 8) as *const u8, 8);
    let opcode = *(site as *const u8);
    let rel = core::ptr::read_unaligned((site + 1) as *const i32);
    if before != EXPECTED
        || opcode != 0xE8
        || (site as i64 + 5 + rel as i64) as usize != exe + FEATURE_TEST_RVA
    {
        state.store(2, SeqCst);
        return false;
    }
    let Some(at) = trampoline::allocate(site, 64) else {
        state.store(2, SeqCst);
        return false;
    };
    let mut code = Vec::with_capacity(64);
    mov_rax(&mut code, &raw const CyberpunkVR_ScopeGraphDag as u64);
    code.extend_from_slice(&[0x83, 0x38, 0x00]); // cmp dword [rax], 0
    code.extend_from_slice(&[0x74, 0x00]); // je original
    let je = code.len() - 1;
    mov_rax(&mut code, SCOPE_CAMERA);
    code.extend_from_slice(&[0x48, 0x3B, 0x81, 0x78, 0xDA, 0xFF, 0xFF]); // cmp rax, [rcx-2588h]
    code.extend_from_slice(&[0x75, 0x00]); // jne original
    let jne = code.len() - 1;
    code.extend_from_slice(&[0x31, 0xC0, 0xC3]); // xor eax, eax ; ret
    for k in [je, jne] {
        code[k] = (code.len() - (k + 1)) as u8;
    }
    mov_rax(&mut code, (exe + FEATURE_TEST_RVA) as u64); // original:
    code.extend_from_slice(&[0xFF, 0xE0]); // jmp rax
    trampoline::Code::new(at).bytes(&code).commit();
    if !trampoline::patch_call(site, at) {
        state.store(2, SeqCst);
        return false;
    }
    state.store(1, SeqCst);
    true
}
