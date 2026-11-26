//! THE HAPTIC SETTINGS THE INI CARRIES (main's include/Runtimes/HapticRouting.hpp and the gains of
//! src/Runtimes/Haptics.cpp): where the PSVR2's rumble goes (the external Toolkit bridge or the native OpenXR output),
//! the strength of the OpenXR haptics restored from the earlier fork, and the vehicle audio rumble's strength,
//! channels and grip rule. Owned here so the settings poll, the overlay and the haptics all read one copy.
use core::sync::atomic::Ordering::Relaxed;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64};

/// A saved preference, not hot actuator ownership. The external Toolkit process has independent EDS/audio writers; a
/// game restart and bridge start/stop are required when changing routes. Detection is a startup snapshot, not a
/// readiness handshake: the released bridge only reads our mapping and has no response channel.
pub struct Routing {
    requested: AtomicI32,
    active_request: AtomicI32,
    toolkit: AtomicBool,
    detected: AtomicBool,
}

impl Routing {
    pub const fn new() -> Self {
        Self { requested: AtomicI32::new(2), active_request: AtomicI32::new(2), toolkit: AtomicBool::new(false), detected: AtomicBool::new(false) }
    }
    /// 0 native, 1 Toolkit, 2 auto (native unless a compatible bridge is running).
    pub fn requested(&self) -> i32 {
        self.requested.load(Relaxed)
    }
    pub fn request(&self, mode: i32) {
        self.requested.store(if (0..=2).contains(&mode) { mode } else { 0 }, Relaxed);
    }
    pub fn begin_session(&self, psvr2: bool, bridge_detected: bool) {
        let mode = self.requested();
        self.active_request.store(mode, Relaxed);
        self.detected.store(bridge_detected, Relaxed);
        self.toolkit.store(psvr2 && mode != 0 && bridge_detected, Relaxed);
    }
    pub fn toolkit(&self) -> bool {
        self.toolkit.load(Relaxed)
    }
    pub fn bridge_detected(&self) -> bool {
        self.detected.load(Relaxed)
    }
    pub fn pending(&self, psvr2: bool) -> bool {
        psvr2 && self.requested() != self.active_request.load(Relaxed)
    }
}

impl Default for Routing {
    fn default() -> Self {
        Self::new()
    }
}

pub static HAPTIC_ROUTE: Routing = Routing::new();

/// cvr::haptics::SafeGain (main's include/Runtimes/LegacyHapticPolicy.hpp).
pub fn safe_gain(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        if 2.0 < value {
            2.0
        } else if value < 0.0 {
            0.0
        } else {
            value
        }
    } else {
        fallback
    }
}

static LEGACY_GAIN: AtomicU32 = AtomicU32::new(0x3FA0_0000); // 1.25
/// Off by default: the loopback hears all output audio, the car radio included.
static VEHICLE_GAIN: AtomicU32 = AtomicU32::new(0);
static VEHICLE_CHANNELS: AtomicI32 = AtomicI32::new(1);
static VEHICLE_REQUIRE_GRIP: AtomicBool = AtomicBool::new(false);

pub fn set_legacy_gain(gain: f32) {
    LEGACY_GAIN.store(safe_gain(gain, 1.25).to_bits(), Relaxed);
}
pub fn legacy_gain() -> f32 {
    f32::from_bits(LEGACY_GAIN.load(Relaxed))
}
pub fn set_vehicle_gain(gain: f32) {
    VEHICLE_GAIN.store(safe_gain(gain, 1.0).to_bits(), Relaxed);
}
pub fn vehicle_gain() -> f32 {
    f32::from_bits(VEHICLE_GAIN.load(Relaxed))
}
pub fn set_vehicle_audio_channels(mode: i32) {
    VEHICLE_CHANNELS.store(mode.clamp(0, 3), Relaxed);
}
pub fn vehicle_audio_channels() -> i32 {
    VEHICLE_CHANNELS.load(Relaxed)
}
pub fn set_vehicle_audio_require_grip(enabled: bool) {
    VEHICLE_REQUIRE_GRIP.store(enabled, Relaxed);
}
pub fn vehicle_audio_require_grip() -> bool {
    VEHICLE_REQUIRE_GRIP.load(Relaxed)
}

/// The vehicle audio's per-hand levels (0..1) and when they came (GetTickCount64, 0: none): written by the loopback
/// capture's thread, read by the haptics' tick.
static VEHICLE_LEVEL: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static VEHICLE_MS: AtomicU64 = AtomicU64::new(0);
fn tick_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn level(v: f32) -> f32 {
    if v.is_finite() {
        if 1.0 < v { 1.0 } else if v < 0.0 { 0.0 } else { v }
    } else {
        0.0
    }
}
/// cvr::haptics::SetVehicleHaptics: the newest levels, stamped now.
pub fn set_vehicle_haptics(left: f32, right: f32) {
    VEHICLE_LEVEL[0].store(level(left).to_bits(), Relaxed);
    VEHICLE_LEVEL[1].store(level(right).to_bits(), Relaxed);
    VEHICLE_MS.store(tick_ms(), Relaxed);
}
/// The levels and their stamp.
pub fn vehicle_haptics() -> ([f32; 2], u64) {
    (
        [f32::from_bits(VEHICLE_LEVEL[0].load(Relaxed)), f32::from_bits(VEHICLE_LEVEL[1].load(Relaxed))],
        VEHICLE_MS.load(Relaxed),
    )
}
/// cvr::haptics::Clear's share: no levels, no stamp.
pub fn clear_vehicle_haptics() {
    VEHICLE_LEVEL[0].store(0, Relaxed);
    VEHICLE_LEVEL[1].store(0, Relaxed);
    VEHICLE_MS.store(0, Relaxed);
}

// ---- main's include/Runtimes/LegacyHapticPolicy.hpp: the earlier fork's pulses, shots and the vehicle rumble's freshness

/// The earlier fork's QueueOpenXRHapticPulse (main's LegacyHapticPolicy.hpp): one pending pulse per hand, merged (the
/// stronger, the longer) while fresh (100 ms).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct PulseRequest {
    pub amplitude: f32,
    pub duration_ms: i32,
    pub queued_ms: u64,
}
pub const PULSE0: PulseRequest = PulseRequest { amplitude: 0.0, duration_ms: 0, queued_ms: 0 };
impl PulseRequest {
    pub fn merge(&mut self, amp: f32, ms: i32, now: u64) {
        if !amp.is_finite() || amp <= 0.0 || amp > 1.0 || !(1..=1000).contains(&ms) {
            return;
        }
        if !self.fresh(now) {
            *self = PULSE0;
        }
        self.amplitude = self.amplitude.max(amp);
        self.duration_ms = self.duration_ms.max(ms);
        self.queued_ms = now;
    }
    pub fn fresh(&self, now: u64) -> bool {
        self.duration_ms > 0 && now >= self.queued_ms && now - self.queued_ms <= 100
    }
}
/// RecoilOnShot's strength and length from the weapon's kick (degrees).
pub fn legacy_shot(kick: f32) -> PulseRequest {
    let kick = if !kick.is_finite() || kick <= 0.0 || kick > 16.0 { 1.0 } else { kick };
    let root = kick.sqrt();
    PulseRequest { amplitude: (0.45 + 0.25 * root).min(1.0), duration_ms: 65.min((35.0 + 8.0 * root) as i32), queued_ms: 0 }
}
/// The established dual Toolkit impact, without coupling it to adaptive-trigger selection: a heavy gun keeps the longer
/// impact envelope.
pub fn dual_toolkit_shot(kick: f32) -> PulseRequest {
    let heavy = kick.is_finite() && kick >= 3.5;
    PulseRequest { amplitude: if heavy { 1.0 } else { 0.85 }, duration_ms: if heavy { 125 } else { 75 }, queued_ms: 0 }
}
pub fn vehicle_fresh(driving: bool, now: u64, received: u64) -> bool {
    driving && received != 0 && now >= received && now - received <= 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_clamps() {
        assert_eq!(legacy_gain(), 1.25);
        assert_eq!(safe_gain(f32::NAN, 1.25), 1.25);
        assert_eq!(safe_gain(3.0, 1.0), 2.0);
        assert_eq!(safe_gain(-1.0, 1.0), 0.0);
        let r = Routing::new();
        assert_eq!(r.requested(), 2);
        r.request(7);
        assert_eq!(r.requested(), 0);
        r.request(1);
        r.begin_session(true, true);
        assert!(r.toolkit() && !r.pending(true));
        r.request(0);
        assert!(r.pending(true) && !r.pending(false));
    }


    #[test]
    fn the_earlier_forks_shot_model() {
        let p = legacy_shot(4.0);
        assert!((p.amplitude - 0.95).abs() < 1e-6 && p.duration_ms == 51, "{p:?}");
        assert_eq!(legacy_shot(f32::NAN), legacy_shot(1.0));
        assert_eq!(legacy_shot(16.0).duration_ms, 65);
        assert_eq!(legacy_shot(16.0).amplitude, 1.0);
        assert_eq!(dual_toolkit_shot(3.5).duration_ms, 125);
        assert_eq!(dual_toolkit_shot(1.0).amplitude, 0.85);
    }

    #[test]
    fn pulses_merge_while_fresh() {
        let mut p = PULSE0;
        p.merge(0.5, 30, 1000);
        p.merge(0.4, 60, 1050);
        assert_eq!((p.amplitude, p.duration_ms, p.queued_ms), (0.5, 60, 1050));
        p.merge(0.2, 10, 1300); // stale: starts again
        assert_eq!((p.amplitude, p.duration_ms), (0.2, 10));
        p.merge(1.5, 10, 1300); // refused
        assert_eq!(p.amplitude, 0.2);
        assert!(p.fresh(1400) && !p.fresh(1401));
        assert!(vehicle_fresh(true, 150, 100) && !vehicle_fresh(true, 201, 100) && !vehicle_fresh(false, 150, 100));
    }
}
