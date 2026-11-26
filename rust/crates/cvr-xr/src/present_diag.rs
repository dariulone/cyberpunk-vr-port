//! Present's diagnostics (formerly inline in OpenXRManager::OnPresent): the game's own frame interval bucketed against
//! the display period ([xrgap] prints the buckets), and every two seconds the skeleton's update rate read against the
//! present rate -- [vrik], [rebind], [heading], [bodyyaw], [yawphase] and, behind CyberpunkVR_XrDeepDiag, the shake,
//! the engine's overwrites and the batch clock. Every counter is the C++ half's (C linkage); the window's peaks are
//! reset here once printed.
#![cfg_attr(test, allow(dead_code, unused_imports))]
#![allow(non_upper_case_globals)]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_XrDeepDiag: i32;
    static CyberpunkVR_VrikRateLog: i32;
    static CyberpunkVR_DebugPresentGapBuckets: [AtomicU64; 4];
    static CyberpunkVR_DebugPresentGapUsMax: AtomicU64;

    static g_VrikFrameEpoch: AtomicU32;
    static g_AnimPoseMatchCalls: u64;
    static g_VRIKFreshTotal: i32;
    static g_VRIKReplayTotal: i32;
    static CyberpunkVR_DebugPoseExact: u64;
    static CyberpunkVR_DebugPoseEstimated: u64;
    static CyberpunkVR_DebugSlotMiss: u64;
    static CyberpunkVR_DebugSlotReused: u64;
    static CyberpunkVR_DebugRebindLoopCalls: u64;

    static mut CyberpunkVR_DebugHeadingStepDeg: f32;
    static mut CyberpunkVR_DebugBodyYawLagDeg: f32;
    static mut CyberpunkVR_DebugBodyYawStepDeg: f32;
    static mut CyberpunkVR_DebugHipsYawStepDeg: f32;
    static mut CyberpunkVR_DebugYawToSolveMaxMs: f32;
    static mut CyberpunkVR_DebugYawToSolveMinMs: f32;
    static mut CyberpunkVR_DebugYawLagWriteDeg: f32;
    static CyberpunkVR_DebugYawWritesAll: u64;
    static CyberpunkVR_DebugYawWritesPlayer: u64;

    static CyberpunkVR_HandLerpSpeed: f32;
    static CyberpunkVR_VrikBatchGapMs: f32;
    static CyberpunkVR_DebugHandPerFrameLocates: u64;
    static mut g_VrikShakePeakMm: [[f32; 4]; 2];
    static mut g_VrikShakeSlowMm: [[f32; 4]; 2];
    static mut g_VrikShakeSlowN: [[i32; 4]; 2];
    static mut g_VrikOverwritePassHist: [i32; 8];
    static mut g_VrikBatchGapNew: i32;
    static mut g_VrikBatchGapSame: i32;
    static mut g_VrikBatchGapNewMin: f32;
    static mut g_VrikBatchGapSameMax: f32;
    static mut g_VrikEngineOverwriteMm: f32;
    static mut g_VrikEngineOverwriteBone: i32;
    static mut g_VrikEngineOverwriteHits: i32;
    static mut g_VrikEngineOverwritePasses: i32;

    static g_VRHipsIdx: i32;
    static g_VRNeckIdx: i32;
    static g_VRRightUpperArmIdx: i32;
    static g_VRRightForeArmIdx: i32;
    static g_VRRightBoneIdx: i32;
    static g_VRLeftUpperArmIdx: i32;
    static g_VRLeftForeArmIdx: i32;
    static g_VRLeftBoneIdx: i32;
    static g_VRSpineIdx: [i32; 8];
}

/// Which interval bucket a gap falls in: whole display periods, 3 meaning three or more (the image repeats).
pub fn gap_bucket(dt_ms: f64, period_ms: f64) -> usize {
    let p = if period_ms > 1.0 { period_ms } else { 13.89 };
    ((dt_ms / p) as i32).clamp(0, 3) as usize
}

/// The previous present's stamp in ms (f64 bits); 0 = none.
static PREV_PRESENT_MS: AtomicU64 = AtomicU64::new(0);

/// The game's frame interval, bucketed by display periods, and its peak in µs -- behind CyberpunkVR_XrDeepDiag.
/// Disarmed it forgets the last stamp, so arming it live does not book one enormous gap measured from before.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_present_gap(display_period_ns: i64) {
    // SAFETY: plain C ints the panel sets.
    let (deep, diag) = unsafe { (read_volatile(addr_of!(CyberpunkVR_XrDeepDiag)), read_volatile(addr_of!(CyberpunkVR_RuntimeDiagnostics))) };
    if deep == 0 {
        PREV_PRESENT_MS.store(0, Ordering::Relaxed);
        return;
    }
    let now = crate::tracking::now_ms();
    let prev = f64::from_bits(PREV_PRESENT_MS.load(Ordering::Relaxed));
    if prev > 0.0 {
        let dt = now - prev;
        let b = gap_bucket(dt, display_period_ns as f64 / 1.0e6);
        if diag != 0 {
            // SAFETY: the C++ half's atomics.
            unsafe { CyberpunkVR_DebugPresentGapBuckets[b].fetch_add(1, Ordering::Relaxed) };
        }
        // SAFETY: as above.
        unsafe { CyberpunkVR_DebugPresentGapUsMax.fetch_max((dt * 1000.0) as u64, Ordering::Relaxed) };
    }
    PREV_PRESENT_MS.store(now.to_bits(), Ordering::Relaxed);
}

/// What the previous window ended on.
struct RateWindow {
    last: u64,
    presents: u64,
    apply: u64,
    fresh: i32,
    replay: i32,
    epoch: u32,
    exact: u64,
    estimated: u64,
    miss: u64,
    reused: u64,
    rebind: u64,
    yaw_all: u64,
    yaw_player: u64,
    locates: u64,
}
static RATE: std::sync::Mutex<RateWindow> = std::sync::Mutex::new(RateWindow {
    last: 0,
    presents: 0,
    apply: 0,
    fresh: 0,
    replay: 0,
    epoch: 0,
    exact: 0,
    estimated: 0,
    miss: 0,
    reused: 0,
    rebind: 0,
    yaw_all: 0,
    yaw_player: 0,
    locates: 0,
});

/// A count's change per second.
fn per_s(now: u64, before: u64, dt: f64) -> f64 {
    now.wrapping_sub(before) as f64 / dt
}

/// The rig bone an index names, for the overwrite line.
#[cfg(not(test))]
unsafe fn bone_name(b: i32) -> &'static str {
    // SAFETY: the rig's indices as the natives resolved them.
    unsafe {
        if b < 0 {
            return "none";
        }
        let named = [
            (g_VRHipsIdx, "hips"),
            (g_VRNeckIdx, "neck"),
            (g_VRRightUpperArmIdx, "R upperarm"),
            (g_VRRightForeArmIdx, "R forearm"),
            (g_VRRightBoneIdx, "R hand"),
            (g_VRLeftUpperArmIdx, "L upperarm"),
            (g_VRLeftForeArmIdx, "L forearm"),
            (g_VRLeftBoneIdx, "L hand"),
        ];
        if let Some((_, who)) = named.iter().find(|(i, _)| *i == b) {
            return who;
        }
        if (*addr_of!(g_VRSpineIdx)).contains(&b) {
            return "spine";
        }
        "?"
    }
}

/// The two-second census (CyberpunkVR_VrikRateLog): the present rate, VRIK's frame epoch, the engine's pose applies,
/// fresh solves and replays, the exact-pose share and the slot fallbacks; then the rebind loop per present, the turn
/// rate, the body against the view, the yaw's phase and, behind CyberpunkVR_XrDeepDiag, where the shake enters.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_vrik_rate_log(present_count: u64) {
    // SAFETY: the panel's flag.
    if unsafe { read_volatile(addr_of!(CyberpunkVR_VrikRateLog)) } == 0 {
        return;
    }
    let mut w = RATE.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: a plain clock read.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    // SAFETY: the C++ half's counters, written from the game's threads; read once each, as the C++ did.
    let (apply, fresh, replay, epoch) = unsafe {
        (
            read_volatile(addr_of!(g_AnimPoseMatchCalls)),
            read_volatile(addr_of!(g_VRIKFreshTotal)),
            read_volatile(addr_of!(g_VRIKReplayTotal)),
            g_VrikFrameEpoch.load(Ordering::Relaxed),
        )
    };
    if w.last == 0 {
        w.last = now;
        w.presents = present_count;
        w.apply = apply;
        w.fresh = fresh;
        w.replay = replay;
        w.epoch = epoch;
        return;
    }
    if now - w.last < 2000 {
        return;
    }
    let dt = (now - w.last) as f64 / 1000.0;
    let d_apply = apply.wrapping_sub(w.apply) as f64;
    let d_fresh = fresh.wrapping_sub(w.fresh) as f64;
    // SAFETY: as above.
    let (ex, es, mi, ru) = unsafe {
        (
            read_volatile(addr_of!(CyberpunkVR_DebugPoseExact)),
            read_volatile(addr_of!(CyberpunkVR_DebugPoseEstimated)),
            read_volatile(addr_of!(CyberpunkVR_DebugSlotMiss)),
            read_volatile(addr_of!(CyberpunkVR_DebugSlotReused)),
        )
    };
    let d_ex = ex.wrapping_sub(w.exact) as f64;
    let d_es = es.wrapping_sub(w.estimated) as f64;
    // freshSolve should track present; passesPerSolve is the engine's own pose-apply count per frame (measured 4-5).
    cvr_host::log!(
        "[vrik] present={:.1}/s  frameEpoch={:.1}/s  poseApply={:.1}/s  freshSolve={:.1}/s  replay={:.1}/s  passesPerSolve={:.1}  poseExact={:.0}%  slotMiss={:.1}/s  reused={:.1}/s\n",
        per_s(present_count, w.presents, dt),
        epoch.wrapping_sub(w.epoch) as f64 / dt,
        d_apply / dt,
        d_fresh / dt,
        replay.wrapping_sub(w.replay) as f64 / dt,
        if d_fresh > 0.5 { d_apply / d_fresh } else { 0.0 },
        if d_ex + d_es > 0.5 { 100.0 * d_ex / (d_ex + d_es) } else { 0.0 },
        per_s(mi, w.miss, dt),
        per_s(ru, w.reused, dt)
    );
    // The world-transform rebind loop per present: 2 means the skeleton is placed twice inside one image.
    {
        // SAFETY: as above.
        let rb = unsafe { read_volatile(addr_of!(CyberpunkVR_DebugRebindLoopCalls)) };
        let d_pres = present_count.wrapping_sub(w.presents) as f64;
        let d_rb = rb.wrapping_sub(w.rebind) as f64;
        w.rebind = rb;
        cvr_host::log!(
            "[rebind] world-transform rebind loop {:.1}/s | perPresent {:.2}\n",
            d_rb / dt,
            if d_pres > 0.5 { d_rb / d_pres } else { 0.0 }
        );
    }
    // SAFETY: the C++ half's peaks, reset per window as the C++ did.
    unsafe {
        cvr_host::log!("[heading] turn rate peak {:.2} deg/frame\n", read_volatile(addr_of!(CyberpunkVR_DebugHeadingStepDeg)) as f64);
        cvr_host::log!(
            "[bodyyaw] view-vs-entity peak {:.2} deg | entity step {:.2} deg/solve | hips model step {:.2} deg\n",
            read_volatile(addr_of!(CyberpunkVR_DebugBodyYawLagDeg)) as f64,
            read_volatile(addr_of!(CyberpunkVR_DebugBodyYawStepDeg)) as f64,
            read_volatile(addr_of!(CyberpunkVR_DebugHipsYawStepDeg)) as f64
        );
        write_volatile(addr_of_mut!(CyberpunkVR_DebugBodyYawLagDeg), 0.0);
        write_volatile(addr_of_mut!(CyberpunkVR_DebugBodyYawStepDeg), 0.0);
        write_volatile(addr_of_mut!(CyberpunkVR_DebugHipsYawStepDeg), 0.0);
        let ya = read_volatile(addr_of!(CyberpunkVR_DebugYawWritesAll));
        let yp = read_volatile(addr_of!(CyberpunkVR_DebugYawWritesPlayer));
        cvr_host::log!(
            "[yawphase] yaw->solve {:.2}..{:.2} ms | lag vs written yaw {:.2} deg | writes all {:.0}/s player {:.0}/s\n",
            read_volatile(addr_of!(CyberpunkVR_DebugYawToSolveMinMs)) as f64,
            read_volatile(addr_of!(CyberpunkVR_DebugYawToSolveMaxMs)) as f64,
            read_volatile(addr_of!(CyberpunkVR_DebugYawLagWriteDeg)) as f64,
            per_s(ya, w.yaw_all, dt),
            per_s(yp, w.yaw_player, dt)
        );
        w.yaw_all = ya;
        w.yaw_player = yp;
        write_volatile(addr_of_mut!(CyberpunkVR_DebugYawToSolveMaxMs), 0.0);
        write_volatile(addr_of_mut!(CyberpunkVR_DebugYawToSolveMinMs), 0.0);
        write_volatile(addr_of_mut!(CyberpunkVR_DebugYawLagWriteDeg), 0.0);
        write_volatile(addr_of_mut!(CyberpunkVR_DebugHeadingStepDeg), 0.0);

        // Where the shake enters: the peak second difference in mm per stage and hand (xr is the control).
        let deep = read_volatile(addr_of!(CyberpunkVR_XrDeepDiag)) != 0;
        let slow = read_volatile(addr_of!(g_VrikShakeSlowMm));
        let peak = read_volatile(addr_of!(g_VrikShakePeakMm));
        let slow_n = read_volatile(addr_of!(g_VrikShakeSlowN));
        if deep {
            cvr_host::log!(
                "[vrikshake] slow(<3mm/frame): L xr={:.2} ctrl={:.2} anchor={:.2} target={:.2} | R xr={:.2} ctrl={:.2} anchor={:.2} target={:.2}  mm  (nXR={}/{})\n",
                slow[0][3] as f64,
                slow[0][0] as f64,
                slow[0][1] as f64,
                slow[0][2] as f64,
                slow[1][3] as f64,
                slow[1][0] as f64,
                slow[1][1] as f64,
                slow[1][2] as f64,
                slow_n[0][3],
                slow_n[1][3]
            );
            cvr_host::log!(
                "[vrikshake] any motion:      L xr={:.2} ctrl={:.2} anchor={:.2} target={:.2} | R xr={:.2} ctrl={:.2} anchor={:.2} target={:.2}  mm  (predict={:.2})\n",
                peak[0][3] as f64,
                peak[0][0] as f64,
                peak[0][1] as f64,
                peak[0][2] as f64,
                peak[1][3] as f64,
                peak[1][0] as f64,
                peak[1][1] as f64,
                peak[1][2] as f64,
                read_volatile(addr_of!(CyberpunkVR_HandLerpSpeed)) as f64
            );
        }
        // The window in which the skeleton is not ours: how far the engine moved a bone we own, which, how often.
        let b = read_volatile(addr_of!(g_VrikEngineOverwriteBone));
        if deep {
            cvr_host::log!(
                "[vrikshake] engine moved our bones by up to {:.1} mm (bone {} = {}) in {} of {} replay passes\n",
                read_volatile(addr_of!(g_VrikEngineOverwriteMm)) as f64,
                b,
                bone_name(b),
                read_volatile(addr_of!(g_VrikEngineOverwriteHits)),
                read_volatile(addr_of!(g_VrikEngineOverwritePasses))
            );
            let h = read_volatile(addr_of!(g_VrikOverwritePassHist));
            cvr_host::log!(
                "[vrikhips] overwrite by pass in batch: 1={} 2={} 3={} 4={} 5={} 6={} 7+={}\n",
                h[1],
                h[2],
                h[3],
                h[4],
                h[5],
                h[6],
                h[7]
            );
        }
        write_volatile(addr_of_mut!(g_VrikOverwritePassHist), [0; 8]);
        write_volatile(addr_of_mut!(g_VrikEngineOverwriteMm), 0.0);
        write_volatile(addr_of_mut!(g_VrikEngineOverwriteBone), -1);
        write_volatile(addr_of_mut!(g_VrikEngineOverwriteHits), 0);
        write_volatile(addr_of_mut!(g_VrikEngineOverwritePasses), 0);
        // The two populations the batch clock rests on: gaps inside one batch and gaps between batches.
        if deep {
            cvr_host::log!(
                "[vrikbatch] batches {} (min gap {:.2} ms) | same-batch passes {} (max gap {:.2} ms) | threshold {:.2}\n",
                read_volatile(addr_of!(g_VrikBatchGapNew)),
                read_volatile(addr_of!(g_VrikBatchGapNewMin)) as f64,
                read_volatile(addr_of!(g_VrikBatchGapSame)),
                read_volatile(addr_of!(g_VrikBatchGapSameMax)) as f64,
                read_volatile(addr_of!(CyberpunkVR_VrikBatchGapMs)) as f64
            );
        }
        write_volatile(addr_of_mut!(g_VrikBatchGapNew), 0);
        write_volatile(addr_of_mut!(g_VrikBatchGapSame), 0);
        write_volatile(addr_of_mut!(g_VrikBatchGapNewMin), 0.0);
        write_volatile(addr_of_mut!(g_VrikBatchGapSameMax), 0.0);
        let loc = read_volatile(addr_of!(CyberpunkVR_DebugHandPerFrameLocates));
        if deep {
            cvr_host::log!("[vrikshake] per-frame hand locates: {:.1}/s (expect ~2x present)\n", per_s(loc, w.locates, dt));
        }
        w.locates = loc;
        write_volatile(addr_of_mut!(g_VrikShakePeakMm), [[0.0; 4]; 2]);
        write_volatile(addr_of_mut!(g_VrikShakeSlowMm), [[0.0; 4]; 2]);
        write_volatile(addr_of_mut!(g_VrikShakeSlowN), [[0; 4]; 2]);
    }
    w.apply = apply;
    w.fresh = fresh;
    w.replay = replay;
    w.epoch = epoch;
    w.exact = ex;
    w.estimated = es;
    w.miss = mi;
    w.reused = ru;
    w.last = now;
    w.presents = present_count;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_count_whole_display_periods() {
        assert_eq!(gap_bucket(10.0, 13.89), 0);
        assert_eq!(gap_bucket(14.0, 13.89), 1);
        assert_eq!(gap_bucket(30.0, 13.89), 2);
        assert_eq!(gap_bucket(41.7, 13.89), 3);
        assert_eq!(gap_bucket(400.0, 13.89), 3);
        assert_eq!(gap_bucket(-5.0, 13.89), 0);
        // No period yet: 72 Hz is assumed.
        assert_eq!(gap_bucket(30.0, 0.0), 2);
        assert_eq!(gap_bucket(30.0, 11.1), 2);
    }

    #[test]
    fn rates_survive_a_wrapped_counter() {
        assert_eq!(per_s(5, u64::MAX - 4, 2.0), 5.0);
        assert_eq!(per_s(200, 100, 2.0), 50.0);
    }
}
