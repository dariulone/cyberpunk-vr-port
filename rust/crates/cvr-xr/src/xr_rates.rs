//! The XR loop's rate reports (formerly in src/Runtimes/OpenXRFrameLoop.cpp): once a second, from the Present thread,
//! [xrrate] (presents against XR cycles, waits, begins, ends with and without a picture, submits, the display rate),
//! and behind CyberpunkVR_XrDeepDiag [xrwarp] (the submitted image's age), [xrsrc]/[xrage] (the second eye's content),
//! [xrgap] (the game's frame interval), [xrcap] (the captures); [xreye] (the second eye's fallbacks), [xrloop] (whether
//! waits, begins and ends pair 1:1), [xrcadence] (display periods per submitted frame), [xrcycle] (where a cycle's
//! time went: wait against work, HELD and LATE) and [xrorder] (the last 64 events: P present, W wait, E submit with a
//! picture, e empty submit, s loop busy). The cadence and cycle accumulators and the event ring are owned here; the
//! counters are the C++ half's (C linkage). All of it is behind CyberpunkVR_XrRateLog.
#![cfg_attr(test, allow(dead_code, unused_imports))]
#![allow(non_upper_case_globals)]
use core::ptr::{addr_of, read_volatile};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use cvr_host::log;

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_XrRateLog: i32;
    static CyberpunkVR_XrDeepDiag: i32;
    static CyberpunkVR_DebugXrCycles: u64;
    static CyberpunkVR_DebugMonoSubmits: u64;
    static CyberpunkVR_DebugXrWaits: AtomicU64;
    static CyberpunkVR_DebugXrWaitsReturned: AtomicU64;
    static CyberpunkVR_DebugXrWaitFailed: AtomicU64;
    static CyberpunkVR_DebugXrBegins: AtomicU64;
    static CyberpunkVR_DebugXrBeginDiscarded: AtomicU64;
    static CyberpunkVR_DebugXrEnds: AtomicU64;
    static CyberpunkVR_DebugXrEndsWithLayer: AtomicU64;
    static CyberpunkVR_DebugXrEndsEmpty: AtomicU64;
    static CyberpunkVR_DebugXrEndFailed: AtomicU64;
    static CyberpunkVR_DebugSubmitAgeCount: AtomicU64;
    static CyberpunkVR_DebugSubmitAgeSumUs: AtomicU64;
    static CyberpunkVR_DebugSubmitAgeMinUs: AtomicU64;
    static CyberpunkVR_DebugSubmitAgeMaxUs: AtomicU64;
    static CyberpunkVR_DebugSubmitAgeBuckets: [AtomicU64; 4];
    static CyberpunkVR_DebugEyeAgeCount: AtomicU64;
    static CyberpunkVR_DebugEyeAgeSumMs: AtomicU64;
    static CyberpunkVR_DebugEyeAgeMaxMs: AtomicU64;
    static CyberpunkVR_DebugEyeAgeNever: AtomicU64;
    static CyberpunkVR_DebugEyeAgeBuckets: [AtomicU64; 4];
    static CyberpunkVR_DebugStableCopies: u64;
    static CyberpunkVR_DebugStableSkips: u64;
    static CyberpunkVR_StereoEyeMaxAgeMs: u32;
    static CyberpunkVR_DebugVrcamEyePaired: AtomicU64;
    static CyberpunkVR_DebugVrcamEyeUnpaired: AtomicU64;
    static CyberpunkVR_DebugVrcamEyeReused: AtomicU64;
    static CyberpunkVR_DebugPresentGapBuckets: [AtomicU64; 4];
    static CyberpunkVR_DebugPresentGapUsMax: AtomicU64;
    static CyberpunkVR_DebugCapOk: AtomicU64;
    static CyberpunkVR_DebugCapSkipNoView: AtomicU64;
    static CyberpunkVR_DebugCapSkipNoRes: AtomicU64;
    static CyberpunkVR_DebugCapSkipNoSlot: AtomicU64;
    static CyberpunkVR_DebugCapSkipFence: AtomicU64;
    static CyberpunkVR_DebugCapSkipReset: AtomicU64;
    static CyberpunkVR_DebugCapFenceWaits: AtomicU64;
    static CyberpunkVR_DebugCapFenceUsSum: AtomicU64;
    static CyberpunkVR_DebugCapFenceUsMax: AtomicU64;
}

/// A C int knob, read once.
#[cfg(not(test))]
fn on(knob: &'static i32) -> bool {
    // SAFETY: a plain C int the launcher's gate sets.
    unsafe { read_volatile(knob as *const i32) != 0 }
}

// ---- the event ring

const PATTERN_LEN: usize = 64;
static PATTERN: [AtomicU8; PATTERN_LEN] = [const { AtomicU8::new(0) }; PATTERN_LEN];
static PATTERN_AT: AtomicU32 = AtomicU32::new(0);

/// One event into the ring (XrMark): P present, W wait, E submit with a picture, e empty submit, s loop busy.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_mark(c: u8) {
    // SAFETY: the launcher's flag.
    if !on(unsafe { &*addr_of!(CyberpunkVR_XrRateLog) }) {
        return;
    }
    let i = PATTERN_AT.fetch_add(1, Ordering::Relaxed) as usize;
    PATTERN[i % PATTERN_LEN].store(c, Ordering::Relaxed);
}

/// The ring as a string, oldest first ('.' where nothing was written yet).
pub fn pattern() -> String {
    let at = PATTERN_AT.load(Ordering::Relaxed) as usize;
    (0..PATTERN_LEN)
        .map(|i| match PATTERN[(at + i) % PATTERN_LEN].load(Ordering::Relaxed) {
            0 => '.',
            c => c as char,
        })
        .collect()
}

// ---- the cadence and the cycles

/// Display periods per submitted frame, and where the cycles' time went; cleared by the reader so each line is its
/// own second.
#[derive(Default)]
pub struct Cycles {
    pub cadence: [u64; 8],
    pub last_submit_ms: f64,
    pub wait_sum: f64,
    pub wait_peak: f64,
    pub work_sum: f64,
    pub work_peak: f64,
    pub period_peak: f64,
    pub n: u64,
    pub held: u64,
    pub late: u64,
}
impl Cycles {
    pub const NEW: Cycles =
        Cycles { cadence: [0; 8], last_submit_ms: 0.0, wait_sum: 0.0, wait_peak: 0.0, work_sum: 0.0, work_peak: 0.0, period_peak: 0.0, n: 0, held: 0, late: 0 };

    /// A submit with a picture at `now_ms`: the periods since the last one, rounded, into its bucket (0..7).
    pub fn bucket(&mut self, now_ms: f64, period_ms: f64) {
        if self.last_submit_ms > 0.0 && period_ms > 0.0 {
            let b = ((((now_ms - self.last_submit_ms) / period_ms) + 0.5) as i32).clamp(0, 7);
            self.cadence[b as usize] += 1;
        }
        self.last_submit_ms = now_ms;
    }
    /// One cycle: its wait and its work. HELD: the wait stood more than a period and a half (the runtime paced us
    /// down); LATE: the work filled more than half a period (the loop lost the frame).
    pub fn accumulate(&mut self, wait_ms: f64, work_ms: f64, period_ms: f64) {
        self.n += 1;
        self.wait_sum += wait_ms;
        self.work_sum += work_ms;
        if wait_ms > self.wait_peak {
            self.wait_peak = wait_ms;
        }
        if work_ms > self.work_peak {
            self.work_peak = work_ms;
        }
        if period_ms > self.period_peak {
            self.period_peak = period_ms;
        }
        if wait_ms > period_ms * 1.5 {
            self.held += 1;
        }
        if work_ms > period_ms * 0.5 {
            self.late += 1;
        }
    }
}
static CYCLES: Mutex<Cycles> = Mutex::new(Cycles::NEW);

fn period_ms_of(period_ns: i64) -> f64 {
    if period_ns > 0 {
        period_ns as f64 / 1.0e6
    } else {
        13.89
    }
}

/// XrBucketCadence: a submit with a picture, bucketed by the runtime's own period.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_bucket_cadence(period_ns: i64) {
    // SAFETY: the launcher's flag.
    if !on(unsafe { &*addr_of!(CyberpunkVR_XrRateLog) }) {
        return;
    }
    let now = crate::tracking::now_ms();
    CYCLES.lock().unwrap_or_else(|p| p.into_inner()).bucket(now, period_ms_of(period_ns));
}

/// XrAccumulateCycle: one cycle's wait (from `wait_enter_ms`, `wait_ms` long) and its work since.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_accumulate_cycle(wait_enter_ms: f64, wait_ms: f64, period_ns: i64) {
    // SAFETY: the launcher's flag.
    if !on(unsafe { &*addr_of!(CyberpunkVR_XrRateLog) }) {
        return;
    }
    let work = crate::tracking::now_ms() - wait_enter_ms - wait_ms;
    CYCLES.lock().unwrap_or_else(|p| p.into_inner()).accumulate(wait_ms, work, period_ms_of(period_ns));
}

// ---- the report

#[derive(Default)]
struct Window {
    last_ms: u64,
    cycles: u64,
    waits: u64,
    begins: u64,
    ends: u64,
    layer: u64,
    empty: u64,
    submits: u64,
    presents: u64,
    age_count: u64,
    age_sum: u64,
    age_buckets: [u64; 4],
    eye_count: u64,
    eye_sum: u64,
    eye_never: u64,
    eye_buckets: [u64; 4],
    stable_copies: u64,
    stable_skips: u64,
    paired: u64,
    unpaired: u64,
    reused: u64,
    gap: [u64; 4],
    cap: [u64; 9],
}
static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

fn load(a: &AtomicU64) -> u64 {
    a.load(Ordering::Relaxed)
}
fn pct(part: u64, total: u64) -> f64 {
    if total != 0 {
        100.0 * part as f64 / total as f64
    } else {
        0.0
    }
}

/// OpenXRManager::ReportXrFrameRates: called from every Present (`present_count` its serial, `period_ns` the
/// runtime's display period); marks the ring and prints the reports once a second.
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_report_frame_rates(present_count: u64, period_ns: i64) {
    cvr_xr_mark(b'P');
    // SAFETY (whole body): the C++ half's counters and knobs, read as the C++ did.
    unsafe {
        if !on(&*addr_of!(CyberpunkVR_XrRateLog)) {
            return;
        }
        let deep = on(&*addr_of!(CyberpunkVR_XrDeepDiag));
        let diag = on(&*addr_of!(CyberpunkVR_RuntimeDiagnostics));
        let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
        let cycles = read_volatile(addr_of!(CyberpunkVR_DebugXrCycles));
        let waits = load(&CyberpunkVR_DebugXrWaits);
        let begins = load(&CyberpunkVR_DebugXrBegins);
        let ends = load(&CyberpunkVR_DebugXrEnds);
        let layer = load(&CyberpunkVR_DebugXrEndsWithLayer);
        let empty = load(&CyberpunkVR_DebugXrEndsEmpty);
        let subs = read_volatile(addr_of!(CyberpunkVR_DebugMonoSubmits));
        let mut guard = WINDOW.lock().unwrap_or_else(|p| p.into_inner());
        let w = match guard.as_mut() {
            Some(w) if w.last_ms != 0 => w,
            _ => {
                let w = guard.get_or_insert_with(Window::default);
                (w.last_ms, w.cycles, w.waits, w.begins, w.ends) = (now, cycles, waits, begins, ends);
                (w.layer, w.empty, w.submits, w.presents) = (layer, empty, subs, present_count);
                return;
            }
        };
        if now - w.last_ms < 1000 {
            return;
        }
        let dt = (now - w.last_ms) as f64 / 1000.0;
        let rate = |cur: u64, prev: u64| cur.wrapping_sub(prev) as f64 / dt;
        let present_rate = rate(present_count, w.presents);
        let layer_rate = rate(layer, w.layer);
        let period_ms = period_ns as f64 / 1.0e6;
        let display_hz = if period_ms > 0.0 { 1000.0 / period_ms } else { 0.0 };
        log!(
            "[xrrate] presents {:.1}/s | xr cycles {:.1}/s | wait {:.1}/s begin {:.1}/s end {:.1}/s (WITH A PICTURE {:.1}/s, EMPTY {:.1}/s) | mono submits {:.1}/s | display {:.2} ms = {:.1} Hz | perDisplay {:.2} perPresent {:.2}\n",
            present_rate,
            rate(cycles, w.cycles),
            rate(waits, w.waits),
            rate(begins, w.begins),
            rate(ends, w.ends),
            layer_rate,
            rate(empty, w.empty),
            rate(subs, w.submits),
            period_ms,
            display_hz,
            if display_hz > 0.0 { layer_rate / display_hz } else { 0.0 },
            if present_rate > 0.0 { layer_rate / present_rate } else { 0.0 }
        );

        if deep {
            let n = load(&CyberpunkVR_DebugSubmitAgeCount);
            let sum = load(&CyberpunkVR_DebugSubmitAgeSumUs);
            let lo = load(&CyberpunkVR_DebugSubmitAgeMinUs);
            let hi = load(&CyberpunkVR_DebugSubmitAgeMaxUs);
            let b: [u64; 4] = core::array::from_fn(|i| load(&CyberpunkVR_DebugSubmitAgeBuckets[i]));
            let dn = n.wrapping_sub(w.age_count);
            let ds = sum.wrapping_sub(w.age_sum);
            let db: [u64; 4] = core::array::from_fn(|i| b[i].wrapping_sub(w.age_buckets[i]));
            let dbt: u64 = db.iter().copied().fold(0u64, u64::wrapping_add);
            w.age_count = n;
            w.age_sum = sum;
            w.age_buckets = b;
            if dn != 0 {
                log!(
                    "[xrwarp] submitted image age this window: mean {:.2} ms, min {:.2}, max {:.2}, spread {:.2} | by display period: 0 {:.0}% | 1 {:.0}% | 2 {:.0}% | 3+ {:.0}%  (n={})\n",
                    ds as f64 / dn as f64 / 1000.0,
                    lo as f64 / 1000.0,
                    hi as f64 / 1000.0,
                    hi.wrapping_sub(lo) as f64 / 1000.0,
                    pct(db[0], dbt),
                    pct(db[1], dbt),
                    pct(db[2], dbt),
                    pct(db[3], dbt),
                    dn
                );
            }
            if diag {
                CyberpunkVR_DebugSubmitAgeMinUs.store(0, Ordering::Relaxed);
                CyberpunkVR_DebugSubmitAgeMaxUs.store(0, Ordering::Relaxed);
            }
        }

        if deep {
            let c = load(&CyberpunkVR_DebugEyeAgeCount);
            let s = load(&CyberpunkVR_DebugEyeAgeSumMs);
            let mx = load(&CyberpunkVR_DebugEyeAgeMaxMs);
            let nv = load(&CyberpunkVR_DebugEyeAgeNever);
            let b: [u64; 4] = core::array::from_fn(|i| load(&CyberpunkVR_DebugEyeAgeBuckets[i]));
            let db: [u64; 4] = core::array::from_fn(|i| b[i].wrapping_sub(w.eye_buckets[i]));
            w.eye_buckets = b;
            let (dc, ds, dn) = (c.wrapping_sub(w.eye_count), s.wrapping_sub(w.eye_sum), nv.wrapping_sub(w.eye_never));
            w.eye_count = c;
            w.eye_sum = s;
            w.eye_never = nv;
            let cop = read_volatile(addr_of!(CyberpunkVR_DebugStableCopies));
            let skp = read_volatile(addr_of!(CyberpunkVR_DebugStableSkips));
            let (dcop, dskp) = (cop.wrapping_sub(w.stable_copies), skp.wrapping_sub(w.stable_skips));
            w.stable_copies = cop;
            w.stable_skips = skp;
            if dc != 0 || dn != 0 {
                log!("[xrsrc] second view produced {} new images this window, {} copies refused\n", dcop, dskp);
                log!(
                    "[xrage] second eye content age: mean {:.1} ms, max {:.1} ms | by game frame: 0 {} | 1 {} | 2 {} | 3+ {} | no image {}  (n={}, gate {} ms)\n",
                    if dc != 0 { ds as f64 / dc as f64 / 1000.0 } else { 0.0 },
                    mx as f64 / 1000.0,
                    db[0],
                    db[1],
                    db[2],
                    db[3],
                    dn,
                    dc,
                    read_volatile(addr_of!(CyberpunkVR_StereoEyeMaxAgeMs))
                );
            }
            if diag {
                CyberpunkVR_DebugEyeAgeMaxMs.store(0, Ordering::Relaxed);
            }
        }

        {
            let (cp, cu, cr) = (load(&CyberpunkVR_DebugVrcamEyePaired), load(&CyberpunkVR_DebugVrcamEyeUnpaired), load(&CyberpunkVR_DebugVrcamEyeReused));
            let (dp, du, dr) = (cp.wrapping_sub(w.paired), cu.wrapping_sub(w.unpaired), cr.wrapping_sub(w.reused));
            w.paired = cp;
            w.unpaired = cu;
            w.reused = cr;
            if du != 0 || dr != 0 {
                log!(
                    "[xreye] second eye: no slot for this frame {} times, of which {} took its own most recent image and {} fell back to MAIN (own image {}, counted only with deep diag)\n",
                    du,
                    dr,
                    if du >= dr { du - dr } else { 0 },
                    dp
                );
            }
        }

        if deep {
            let g: [u64; 4] = core::array::from_fn(|i| load(&CyberpunkVR_DebugPresentGapBuckets[i]));
            let dg: [u64; 4] = core::array::from_fn(|i| g[i].wrapping_sub(w.gap[i]));
            w.gap = g;
            let tot: u64 = dg.iter().copied().fold(0u64, u64::wrapping_add);
            let gmax = load(&CyberpunkVR_DebugPresentGapUsMax);
            if tot != 0 {
                log!(
                    "[xrgap] game frame interval by display period: <1 {} | 1 {} | 2 {} | 3+ {} | worst {:.2} ms  (n={})\n",
                    dg[0],
                    dg[1],
                    dg[2],
                    dg[3],
                    gmax as f64 / 1000.0,
                    tot
                );
            }
            if diag {
                CyberpunkVR_DebugPresentGapUsMax.store(0, Ordering::Relaxed);
            }
        }

        if deep {
            let c = [
                load(&CyberpunkVR_DebugCapOk),
                load(&CyberpunkVR_DebugCapSkipNoView),
                load(&CyberpunkVR_DebugCapSkipNoRes),
                load(&CyberpunkVR_DebugCapSkipNoSlot),
                load(&CyberpunkVR_DebugCapSkipFence),
                load(&CyberpunkVR_DebugCapSkipReset),
                load(&CyberpunkVR_DebugCapFenceWaits),
                load(&CyberpunkVR_DebugCapFenceUsSum),
                load(&CyberpunkVR_DebugCapFenceUsMax),
            ];
            let d: [u64; 8] = core::array::from_fn(|i| c[i].wrapping_sub(w.cap[i]));
            w.cap[..8].copy_from_slice(&c[..8]);
            let (waits_n, wait_us) = (d[6], d[7]);
            log!(
                "[xrcap] captures {} ok | skipped: noview {}, nores {}, noslot {}, fence-timeout {}, reset {} | fence waits {}, mean {:.2} ms, max {:.2} ms\n",
                d[0],
                d[1],
                d[2],
                d[3],
                d[4],
                d[5],
                waits_n,
                if waits_n != 0 { wait_us as f64 / waits_n as f64 / 1000.0 } else { 0.0 },
                c[8] as f64 / 1000.0
            );
            if diag {
                CyberpunkVR_DebugCapFenceUsMax.store(0, Ordering::Relaxed);
            }
        }

        let waits_back = load(&CyberpunkVR_DebugXrWaitsReturned);
        let wait_fail = load(&CyberpunkVR_DebugXrWaitFailed);
        let end_fail = load(&CyberpunkVR_DebugXrEndFailed);
        let paired = waits_back.wrapping_sub(wait_fail) == begins
            && begins.wrapping_add(end_fail) >= ends
            && begins.wrapping_add(end_fail).wrapping_sub(ends) <= 1;
        log!(
            "[xrloop] xrWaitFrame {} started / {} returned (failed {}) | xrBeginFrame {} (discarded {}) | xrEndFrame {} = with a layer {} + EMPTY {} (failed {}) | {}\n",
            waits,
            waits_back,
            wait_fail,
            begins,
            load(&CyberpunkVR_DebugXrBeginDiscarded),
            ends,
            layer,
            empty,
            end_fail,
            if paired { "PAIRED 1:1" } else { "NOT PAIRED -- this is the defect" }
        );

        {
            let c = {
                let mut g = CYCLES.lock().unwrap_or_else(|p| p.into_inner());
                // The last submit's stamp is not a window quantity: it carries over.
                let last = g.last_submit_ms;
                let c = core::mem::replace(&mut *g, Cycles::NEW);
                g.last_submit_ms = last;
                c
            };
            let tot: u64 = c.cadence.iter().sum();
            if tot > 0 {
                let t = tot as f64;
                log!(
                    "[xrcadence] display periods per submitted frame: <1 {:.0}% | 1 {:.0}% | 2 {:.0}% | 3 {:.0}% | 4 {:.0}% | 5+ {:.0}%  (n={})\n",
                    100.0 * c.cadence[0] as f64 / t,
                    100.0 * c.cadence[1] as f64 / t,
                    100.0 * c.cadence[2] as f64 / t,
                    100.0 * c.cadence[3] as f64 / t,
                    100.0 * c.cadence[4] as f64 / t,
                    100.0 * (c.cadence[5] + c.cadence[6] + c.cadence[7]) as f64 / t,
                    tot
                );
            }
            if c.n > 0 {
                let d = c.n as f64;
                log!(
                    "[xrcycle] {} cycles | wait {:.2} ms mean, {:.1} peak | work {:.2} mean, {:.1} peak | HELD {}, LATE {} | period peak {:.2} ms\n",
                    c.n,
                    c.wait_sum / d,
                    c.wait_peak,
                    c.work_sum / d,
                    c.work_peak,
                    c.held,
                    c.late,
                    c.period_peak
                );
            }
        }

        log!("[xrorder] {}   (P present, W wait, E submit with a picture, e submit EMPTY, s loop busy)\n", pattern());

        (w.last_ms, w.cycles, w.waits, w.begins, w.ends) = (now, cycles, waits, begins, ends);
        (w.layer, w.empty, w.submits, w.presents) = (layer, empty, subs, present_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_rounds_to_whole_periods() {
        let mut c = Cycles::NEW;
        c.bucket(100.0, 13.89);
        assert_eq!(c.cadence, [0; 8]);
        c.bucket(113.0, 13.89);
        c.bucket(141.0, 13.89);
        c.bucket(400.0, 13.89);
        assert_eq!(c.cadence[1], 1);
        assert_eq!(c.cadence[2], 1);
        assert_eq!(c.cadence[7], 1);
    }

    #[test]
    fn held_and_late_point_at_opposite_culprits() {
        let mut c = Cycles::NEW;
        c.accumulate(12.0, 1.0, 13.89);
        c.accumulate(22.0, 1.0, 13.89);
        c.accumulate(5.0, 8.0, 13.89);
        assert_eq!((c.n, c.held, c.late), (3, 1, 1));
        assert_eq!(c.wait_peak, 22.0);
        assert_eq!(c.work_peak, 8.0);
    }

    #[test]
    fn the_ring_reads_oldest_first() {
        let p = pattern();
        assert_eq!(p.len(), PATTERN_LEN);
    }
}
