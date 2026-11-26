//! The frame graph resolver's post-colour watch (src/Stereo/SyncStereo.cpp, `Detour_Resolve3D20` once the id is a
//! post-colour one): the stage probe -- MAIN's post-colour physical index per stage salt, VRCAM's with the stage's
//! generation counter, the salt-70 resolves' callers this frame, dumped as one `[s70res]` line at the mirror blit's
//! resolve (the first 600 frames); then, inside VRCAM's tonemap node, the producer's output -- its resolve at the
//! frame's highest generation -- and inside the final blit, the consumer's natural index, swapped for the tonemap's
//! fresh output when CyberpunkVR_FixPostVersion asks (pinned), both published; and the first 60 `[pv]` lines.
//!
//! The C++ detour calls the original and its fog, frame generation and lobby observers, filters the post-colour ids,
//! and calls `cvr_resolve_post_color` with the node's state; the generation counter is read through ReadProcessMemory
//! where the .cpp read it under SEH.
/// The `[s70res]` line: the frame, the blit's index, and this frame's salt-70 resolves (caller RVA = index).
pub fn s70_line(frame: u32, fin: u32, resolves: &[(u32, u32)]) -> String {
    let mut line = format!("[s70res] fr={frame} finIdx={fin} :");
    for (rva, physical) in resolves.iter().take(20) {
        line += &format!(" {rva:X}={physical}");
    }
    line
}
/// Whether the consumer takes the tonemap's output: fresh (within 8 resolves), present and different.
pub fn pin(natural: u32, tm_idx: u32, tm_seq: u32, seq: u32) -> Option<u32> {
    (tm_idx != 0 && seq.wrapping_sub(tm_seq) < 8 && tm_idx != natural).then_some(tm_idx)
}
/// Whether a tonemap resolve is the output so far: its generation at least the latched one, the latch forgotten
/// when the generation dropped (a new frame).
pub fn tm_output(v7: u32, latched: u32, last: u32) -> bool {
    let latched = if v7.wrapping_add(4) < last { 0 } else { latched };
    v7 >= latched
}

// ---- the detour's body (the Rust build: the C++ half's globals and log)

#[cfg(feature = "rust-build")]
mod abi {
    use super::*;
    use crate::instance_layout::read;
    use core::ffi::c_char;
    use core::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering::*};
    use std::cell::Cell;
    use std::sync::Mutex;

    #[allow(non_upper_case_globals)]
    extern "C" {
        static CyberpunkVR_StageProbe: i32;
        static CyberpunkVR_FixPostVersion: i32;
        static CyberpunkVR_RuntimeDiagnostics: i32;
        static mut CyberpunkVR_DebugResolveHits: u64;
        static mut CyberpunkVR_DebugPinApplied: u64;
        static mut CyberpunkVR_DebugTmPostIdx: u32;
        static mut CyberpunkVR_DebugFinNatural: u32;
        static mut CyberpunkVR_DebugFinPinned: u32;
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    fn log(text: String) {
        let line = std::ffi::CString::new(text).unwrap_or_default();
        // SAFETY: a C string for the C++ half's log.
        unsafe { cvr_cpp_stereo_log(line.as_ptr()) };
    }
    /// A C++ global, read as the .cpp reads it.
    fn get<T: Copy>(value: *const T) -> T {
        // SAFETY: the C++ half's live globals.
        unsafe { core::ptr::read_volatile(value) }
    }
    fn set<T: Copy>(value: *mut T, v: T) {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile(value, v) }
    }
    /// InterlockedIncrement64 on a C++ counter.
    fn bump(value: *mut u64) {
        // SAFETY: an aligned 64-bit C++ global, shared with the C++ half's interlocked increments.
        unsafe { (*(value as *const AtomicU64)).fetch_add(1, SeqCst) };
    }
    fn diagnostics() -> bool {
        get(&raw const CyberpunkVR_RuntimeDiagnostics) != 0
    }

    /// The stage probe's tables (written only: read by a debugger).
    struct Stages {
        vrcam: [u32; 128],
        main: [u32; 128],
        generation: [u32; 128],
        s70: [(u32, u32); 20],
    }
    static STAGES: Mutex<Stages> = Mutex::new(Stages { vrcam: [0; 128], main: [0; 128], generation: [0; 128], s70: [(0, 0); 20] });
    static S70_COUNT: AtomicI64 = AtomicI64::new(0);
    static STAGE_FRAME: AtomicU32 = AtomicU32::new(0);
    static PV_SEQ: AtomicU32 = AtomicU32::new(1);
    static TM_POST_IDX: AtomicU32 = AtomicU32::new(0);
    static TM_POST_GEN: AtomicU32 = AtomicU32::new(0);
    static TM_POST_SEQ: AtomicU32 = AtomicU32::new(0);
    static PV_LINES: AtomicU32 = AtomicU32::new(0);
    thread_local! {
        static LAST_TM_GEN: Cell<u32> = const { Cell::new(0) };
    }

    /// The view's generation counter for a stage (`reg + 24064 * stage + 64`), 0 when unreadable.
    fn generation(reg: i64, stage: u8) -> u32 {
        if reg == 0 {
            return 0;
        }
        let mut b = [0u8; 4];
        let at = (reg as u64).wrapping_add(24064 * u64::from(stage)).wrapping_add(64);
        if read(at, &mut b) { u32::from_le_bytes(b) } else { 0 }
    }
    /// Detour_Resolve3D20 after the original, for a post-colour id.
    ///
    /// # Safety
    /// `out` is the resolver's live output.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_resolve_post_color(reg: i64, out: *mut u32, id: u32, r9: i64, caller_rva: u32, in_fin: bool, in_tm: bool) {
        // SAFETY: as the caller vouches.
        let out = unsafe { &mut *out };
        let probe = get(&raw const CyberpunkVR_StageProbe) != 0;
        let stage = r9 as u8;
        if probe && id == 0x3D7E_6258 && stage < 128 {
            STAGES.lock().unwrap_or_else(|e| e.into_inner()).main[stage as usize] = *out;
        }
        if probe && id == 0x3C7E_6258 {
            let v7 = generation(reg, stage);
            {
                let mut s = STAGES.lock().unwrap_or_else(|e| e.into_inner());
                if stage < 128 {
                    s.vrcam[stage as usize] = *out;
                    s.generation[stage as usize] = v7;
                }
                if stage == 70 {
                    let i = S70_COUNT.fetch_add(1, SeqCst);
                    if (0..20).contains(&i) {
                        s.s70[i as usize] = (caller_rva, *out);
                    }
                }
            }
            if in_fin {
                let frame = STAGE_FRAME.fetch_add(1, Relaxed);
                if frame < 600 {
                    let count = S70_COUNT.load(SeqCst).clamp(0, 20) as usize;
                    let resolves = STAGES.lock().unwrap_or_else(|e| e.into_inner()).s70;
                    log(s70_line(frame, *out, &resolves[..count]));
                }
                S70_COUNT.store(0, SeqCst);
            }
        }
        if !in_fin && !in_tm {
            return;
        }
        if diagnostics() {
            bump(&raw mut CyberpunkVR_DebugResolveHits);
        }
        let v7 = generation(reg, stage);
        let seq = PV_SEQ.fetch_add(1, Relaxed);
        if in_tm {
            let last = LAST_TM_GEN.with(Cell::get);
            if tm_output(v7, TM_POST_GEN.load(Acquire), last) {
                TM_POST_IDX.store(*out, Release);
                TM_POST_GEN.store(v7, Release);
                TM_POST_SEQ.store(seq, Release);
                set(&raw mut CyberpunkVR_DebugTmPostIdx, *out);
            }
            LAST_TM_GEN.with(|c| c.set(v7));
        } else {
            set(&raw mut CyberpunkVR_DebugFinNatural, *out);
            if get(&raw const CyberpunkVR_FixPostVersion) != 0 {
                if let Some(pinned) = pin(*out, TM_POST_IDX.load(Acquire), TM_POST_SEQ.load(Acquire), seq) {
                    *out = pinned;
                    if diagnostics() {
                        bump(&raw mut CyberpunkVR_DebugPinApplied);
                    }
                }
            }
            set(&raw mut CyberpunkVR_DebugFinPinned, *out);
        }
        if PV_LINES.fetch_add(1, SeqCst) < 60 {
            log(format!("[pv] {} id={id:08X} idx={} v7={v7}", if in_fin { "fin" } else { "tm " }, *out));
        }
    }

    /// The resolver's original (sub_140334810), written by the install pass (engine_detours.rs registers the hook).
    #[no_mangle]
    pub static cvr_resolve3d20_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    /// Post-colour ids: (salt << 24) ^ 0x3D7E6258, the salt only in the top byte.
    const POSTCOLOR_LOW: u32 = 0x007E_6258;
    /// The frame graph resolver's detour (formerly SyncStereo.cpp's Detour_Resolve3D20), entered through
    /// `cvr_resolve3d20_hook` with the engine caller's return address: the original, then the fog history, the frame
    /// generation's native inputs and the lobby's resolve, then for a post-colour id the watch above with the node's
    /// state (the mirror blit's node; VRCAM's tonemap work).
    ///
    /// # Safety
    /// The resolver's arguments; `caller` its return address.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_resolve3d20_body(reg: i64, out: *mut u32, idp: *mut u32, r9: i64, caller: usize) -> i64 {
        type Resolve = unsafe extern "C" fn(i64, *mut u32, *mut u32, i64) -> i64;
        // SAFETY: the install pass wrote the original before the hook went live.
        let original: Resolve = unsafe { core::mem::transmute::<usize, Resolve>(cvr_resolve3d20_original.load(Relaxed)) };
        // SAFETY: the engine's arguments.
        let r = unsafe { original(reg, out, idp, r9) };
        if out.is_null() || idp.is_null() {
            return r;
        }
        // SAFETY: the resolver's live output and id; this thread's node state.
        unsafe {
            crate::render_parity::cvr_rp_synchronize_fog_history(reg as usize, out, *idp, caller);
            crate::framegen_inputs::cvr_framegen_observe_native_input(*idp, *out);
            crate::menu_lobby::cvr_menu_lobby_observe_resolve(*idp, *out);
            let id = *idp;
            if id & 0x00FF_FFFF != POSTCOLOR_LOW {
                return r;
            }
            use crate::capture_hooks::{exe_base, tls_ptr, Tls, TONEMAP_WORK};
            let base = exe_base();
            let in_fin = *tls_ptr::<bool>(Tls::MirrorCopyNodeActive);
            let in_tm = *tls_ptr::<bool>(Tls::VrcamNodeActive) && *tls_ptr::<usize>(Tls::CurrentNodeWork) == base.wrapping_add(TONEMAP_WORK);
            cvr_resolve_post_color(reg, out, id, r9, caller.wrapping_sub(base) as u32, in_fin, in_tm);
        }
        r
    }
    // The hook the engine calls: the body with the caller's return address (_ReturnAddress) as its fifth argument.
    core::arch::global_asm!(
        ".globl cvr_resolve3d20_hook",
        ".def cvr_resolve3d20_hook",
        ".scl 2",
        ".type 32",
        ".endef",
        "cvr_resolve3d20_hook:",
        ".seh_proc cvr_resolve3d20_hook",
        "    sub rsp, 0x38",
        ".seh_stackalloc 0x38",
        ".seh_endprologue",
        "    mov rax, [rsp+0x38]",
        "    mov [rsp+0x20], rax",
        "    call cvr_resolve3d20_body",
        "    add rsp, 0x38",
        "    ret",
        ".seh_endproc",
    );
    extern "C" {
        /// The thunk above.
        pub fn cvr_resolve3d20_hook();
    }
}
#[cfg(feature = "rust-build")]
pub(crate) use abi::{cvr_resolve3d20_hook, cvr_resolve3d20_original};

#[cfg(test)]
mod tests {
    //! The pure parts.
    use super::*;

    #[test]
    fn lines() {
        assert_eq!(s70_line(3, 41, &[(0x209AB0, 41), (0x378224, 40)]), "[s70res] fr=3 finIdx=41 : 209AB0=41 378224=40");
        assert_eq!(s70_line(0, 0, &[]), "[s70res] fr=0 finIdx=0 :");
    }
    #[test]
    fn pins() {
        assert_eq!(pin(5, 9, 100, 104), Some(9));
        assert_eq!(pin(5, 9, 100, 108), None);
        assert_eq!(pin(9, 9, 100, 101), None);
        assert_eq!(pin(5, 0, 100, 101), None);
        // The sequence wraps.
        assert_eq!(pin(5, 9, u32::MAX, 2), Some(9));
    }
    #[test]
    fn outputs() {
        assert!(tm_output(29, 29, 29));
        assert!(!tm_output(28, 29, 29));
        // A generation four or more below the last one: a new frame, the latch forgotten.
        assert!(tm_output(3, 29, 29));
        assert!(!tm_output(25, 29, 29));
        assert!(tm_output(25, 0, 29));
    }
}
