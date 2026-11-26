//! Two pieces of the stage replay's frame bookkeeping (src/Render/StageReplay.cpp): the ordinal of a call among the
//! same (side, stage) calls of one engine frame (`NextOrdinal`: a row's word holds frame << 32 | count; the first call
//! of a new frame restarts the count at 1 and keeps the finished frame's count as the row's last count -- how many
//! source partitions a target waits for); and the validated configuration (`CyberpunkVR_StageReplayConfigure`:
//! eleven RenderElements stages -- 3-7, 9, 20-23, 30 -- with VRCAM as the source, barrier-only streams replayable,
//! the camera replay mode; 0 turns it all off); and the G-buffer partitions' publication (`ApplyPartitions`, each
//! StageReplayPoll: nothing once the trampoline failed -- state 2 --; else the scope's key -- the CName hash of
//! vr_optics_feed while xr_scope_gbuffer_partitions is on, 0 otherwise --, then, when it changed, VRCAM's key -- its
//! current context key while xr_vrcam_gbuffer_partitions is on, 0 otherwise -- and the state, 1 with a key and 0
//! without, a change of state logged; the two exported switches store the settings it reads).
//!
//! The .cpp's NextOrdinal passes its row's atomics to `cvr_srg_next_ordinal`; its exported configure, ApplyPartitions
//! and the two switches forward to `cvr_srg_configure`, `cvr_srg_apply_partitions`,
//! `cvr_srg_vrcam_partitions_configure` and `cvr_srg_scope_partitions_configure`. The unit tests below check the
//! ordinals, the configuration and the partitions' steps.
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

/// The ordinal of this call in `frame` (0 for the frame's first), counted in `state`; a new frame stores the
/// finished frame's count (when there was one) in `last`.
pub fn next_ordinal(state: &AtomicU64, last: &AtomicU32, frame: u32) -> u32 {
    let mut current = state.load(Relaxed);
    loop {
        let (seen, count) = ((current >> 32) as u32, current as u32);
        let same = seen == frame;
        let next = u64::from(frame) << 32 | u64::from(if same { count.wrapping_add(1) } else { 1 });
        match state.compare_exchange_weak(current, next, Relaxed, Relaxed) {
            Ok(_) => {
                if !same && count != 0 {
                    last.store(count, Relaxed);
                }
                return if same { count } else { 0 };
            }
            Err(actual) => current = actual,
        }
    }
}

/// The validated stages: RenderElements stages 3-7, 9, 20-23 and 30.
pub const VALIDATED_STAGES: u64 = 0x40F0_02F8;
/// The configuration's exports: (source, stages, barrier-only streams allowed, mode -- 4, the camera replay).
pub fn configuration(enable: bool) -> (u32, u64, u32, u32) {
    if enable {
        (1, VALIDATED_STAGES, 1, 4)
    } else {
        (1, 0, 0, 0)
    }
}

/// One ApplyPartitions step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Partitions {
    /// The trampoline failed (state 2): nothing is published.
    Locked,
    /// The scope's key published; VRCAM's is unchanged.
    Same { optics: u64 },
    /// The scope's key, VRCAM's new key and the state published; `log` when the state differs from the last applied.
    Changed { optics: u64, key: u64, state: u32, log: bool },
}
/// The step for the current `state`, the last `applied` state, the VRCAM switch and its context key, VRCAM's
/// `published` key, the scope's switch and its CName hash.
pub fn partitions(state: u32, applied: u32, want: bool, vrcam_key: u64, published: u64, optics_on: bool, optics_hash: u64) -> Partitions {
    if state == 2 {
        return Partitions::Locked;
    }
    let optics = if optics_on { optics_hash } else { 0 };
    let key = if want { vrcam_key } else { 0 };
    if key == published {
        return Partitions::Same { optics };
    }
    let state = u32::from(key != 0);
    Partitions::Changed { optics, key, state, log: state != applied }
}

#[cfg(feature = "rust-build")]
#[allow(non_upper_case_globals)]
mod abi {
    use super::*;
    use core::ffi::c_char;
    use core::sync::atomic::Ordering::SeqCst;

    extern "C" {
        static CyberpunkVR_StageReplaySource: AtomicU32;
        static CyberpunkVR_StageReplayStages: AtomicU64;
        static CyberpunkVR_StageReplayAllowBarriers: AtomicU32;
        static CyberpunkVR_StageReplayMode: AtomicU32;
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }

    /// # Safety
    /// `state` and `last` are the .cpp's row atomics.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_srg_next_ordinal(state: *const AtomicU64, last: *const AtomicU32, frame: u32) -> u32 {
        // SAFETY: as the caller vouches.
        unsafe { next_ordinal(&*state, &*last, frame) }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_srg_configure(enable: i32) {
        let (source, stages, barriers, mode) = configuration(enable != 0);
        // SAFETY: the C++ half's exports, stored as its std::atomic stores do.
        unsafe {
            CyberpunkVR_StageReplaySource.store(source, SeqCst);
            CyberpunkVR_StageReplayStages.store(stages, SeqCst);
            CyberpunkVR_StageReplayAllowBarriers.store(barriers, SeqCst);
            CyberpunkVR_StageReplayMode.store(mode, SeqCst);
        }
        let line = if enable != 0 { c"[stage-replay] configured on (11 stages)" } else { c"[stage-replay] configured off" };
        // SAFETY: a C string for the C++ half's log.
        unsafe { cvr_cpp_stereo_log(line.as_ptr()) };
    }
    extern "C" {
        static CyberpunkVR_VrcamGbufferPartitions: AtomicU32;
        static CyberpunkVR_OpticsGbufferPartitions: AtomicU32;
        static CyberpunkVR_VrcamGbufferPartitionsState: AtomicU32;
        static mut CyberpunkVR_VrcamGbufferPartitionsKey: u64;
        static mut CyberpunkVR_OpticsGbufferPartitionsKey: u64;
        /// src/Stereo/SyncStereo.cpp: VRCAM's current frame-graph context key.
        static g_vrcam_ctx_key: AtomicU64;
    }
    static APPLIED: AtomicU32 = AtomicU32::new(0);
    #[no_mangle]
    pub extern "C-unwind" fn cvr_srg_apply_partitions(want: u32) {
        static OPTICS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        // SAFETY: the C++ half's exports and atomics; its volatile keys read and written as volatile.
        unsafe {
            let step = partitions(
                CyberpunkVR_VrcamGbufferPartitionsState.load(Relaxed),
                APPLIED.load(Relaxed),
                want != 0,
                if want != 0 { g_vrcam_ctx_key.load(core::sync::atomic::Ordering::Acquire) } else { 0 },
                core::ptr::read_volatile(&raw const CyberpunkVR_VrcamGbufferPartitionsKey),
                CyberpunkVR_OpticsGbufferPartitions.load(Relaxed) != 0,
                *OPTICS.get_or_init(|| crate::vrcam_config::cname_hash(b"vr_optics_feed")),
            );
            let (optics, changed) = match step {
                Partitions::Locked => return,
                Partitions::Same { optics } => (optics, None),
                Partitions::Changed { optics, key, state, log } => (optics, Some((key, state, log))),
            };
            core::ptr::write_volatile(&raw mut CyberpunkVR_OpticsGbufferPartitionsKey, optics);
            let Some((key, state, log)) = changed else { return };
            core::ptr::write_volatile(&raw mut CyberpunkVR_VrcamGbufferPartitionsKey, key);
            CyberpunkVR_VrcamGbufferPartitionsState.store(state, SeqCst);
            if log {
                let line = if state != 0 {
                    c"[stage-replay] VRCAM GBuffer_Solid partitions follow the worker count (VRCAM only)"
                } else {
                    c"[stage-replay] VRCAM GBuffer_Solid partitions native"
                };
                cvr_cpp_stereo_log(line.as_ptr());
            }
            APPLIED.store(state, Relaxed);
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_srg_vrcam_partitions_configure(enable: i32) {
        // SAFETY: the C++ half's switch.
        unsafe { CyberpunkVR_VrcamGbufferPartitions.store(u32::from(enable != 0), SeqCst) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_srg_scope_partitions_configure(enable: i32) {
        // SAFETY: the C++ half's switch.
        unsafe { CyberpunkVR_OpticsGbufferPartitions.store(u32::from(enable != 0), SeqCst) };
    }
}

#[cfg(test)]
mod tests {
    //! The ordinals, the configuration and the partitions' steps.
    use super::*;

    #[test]
    fn ordinals() {
        let (state, last) = (AtomicU64::new(0), AtomicU32::new(0));
        // Frame 7: 0, 1, 2; nothing finished yet.
        assert_eq!([7, 7, 7].map(|f| next_ordinal(&state, &last, f)), [0, 1, 2]);
        assert_eq!((state.load(Relaxed), last.load(Relaxed)), (7 << 32 | 3, 0));
        // Frame 8 restarts at 0 and keeps frame 7's three calls.
        assert_eq!(next_ordinal(&state, &last, 8), 0);
        assert_eq!((state.load(Relaxed), last.load(Relaxed)), (8 << 32 | 1, 3));
        assert_eq!(next_ordinal(&state, &last, 8), 1);
        // A frame number going back is a new frame too.
        assert_eq!(next_ordinal(&state, &last, 5), 0);
        assert_eq!(last.load(Relaxed), 2);
        // The first call ever (frame 0 on a fresh row, count 0) leaves the last count alone.
        let (fresh, kept) = (AtomicU64::new(0), AtomicU32::new(9));
        assert_eq!(next_ordinal(&fresh, &kept, 0), 0);
        assert_eq!((fresh.load(Relaxed), kept.load(Relaxed)), (1, 9));
    }
    #[test]
    fn configured() {
        assert_eq!(VALIDATED_STAGES.count_ones(), 11);
        let stages: Vec<u32> = (0..64).filter(|s| VALIDATED_STAGES >> s & 1 != 0).collect();
        assert_eq!(stages, [3, 4, 5, 6, 7, 9, 20, 21, 22, 23, 30]);
        assert_eq!(configuration(true), (1, VALIDATED_STAGES, 1, 4));
        assert_eq!(configuration(false), (1, 0, 0, 0));
    }
    #[test]
    fn partition_steps() {
        let hash = 0xABCD;
        assert_eq!(partitions(2, 0, true, 7, 0, true, hash), Partitions::Locked);
        // The scope's key follows its switch whether or not VRCAM's changes.
        assert_eq!(partitions(0, 0, false, 7, 0, true, hash), Partitions::Same { optics: hash });
        assert_eq!(partitions(1, 1, true, 7, 7, false, hash), Partitions::Same { optics: 0 });
        // VRCAM's key published with its state; only a change of state logs.
        assert_eq!(partitions(0, 0, true, 7, 0, false, hash), Partitions::Changed { optics: 0, key: 7, state: 1, log: true });
        assert_eq!(partitions(1, 1, true, 9, 7, false, hash), Partitions::Changed { optics: 0, key: 9, state: 1, log: false });
        assert_eq!(partitions(1, 1, false, 9, 9, true, hash), Partitions::Changed { optics: hash, key: 0, state: 0, log: true });
        // With the switch on but no VRCAM context yet the key is 0: nothing changes from a 0 published key.
        assert_eq!(partitions(0, 0, true, 0, 0, false, hash), Partitions::Same { optics: 0 });
    }
}
