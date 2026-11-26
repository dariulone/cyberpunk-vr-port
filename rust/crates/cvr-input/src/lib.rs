//! Stick, keypad and trigger input shared by Pico 4 and Quest 3 (formerly `include/Hooks/AnalogStick.hpp`,
//! `KeypadInput.hpp` and `WeaponTrigger.hpp`).
//!
//! Every function here must give the C++ answer bit for bit: `tools/analog_input_tests` runs both on the same
//! inputs. The C++ standard library's `min`/`clamp` keep a NaN where Rust's `f32::min`/`f32::clamp` would not (or
//! would panic), so the two helpers below spell out MSVC's definitions instead of calling the Rust ones.

#![no_std]
extern crate alloc;

/// MSVC `std::min(a, b)`: `b < a ? b : a`.
#[inline(always)]
pub fn cpp_min(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}

/// MSVC `std::clamp(v, lo, hi)`: `hi < v ? hi : v < lo ? lo : v`. A NaN `v` passes through, as in C++.
#[inline(always)]
pub fn cpp_clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if hi < v {
        hi
    } else if v < lo {
        lo
    } else {
        v
    }
}

/// THE MODIFIER of the D-pad and of the VR menu's swipe (2026-11-24): on a controller with a thumbrest (Quest 3 and
/// the other Touch controllers) the LEFT thumb resting on its thumbrest for more than 0.2 s; on every other one the
/// left stick held in. The input loop publishes the raw states; the D-pad chord and the menu's swipe read them.
pub mod modifier {
    use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::*};
    /// The thumbrests touched now (left, right), and whether this controller has a thumbrest at all (it has reported
    /// a touch).
    pub static THUMBREST: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
    pub static THUMBREST_SEEN: AtomicBool = AtomicBool::new(false);
    /// When the modifier's thumbrest touch began (ms, 0 untouched).
    static THUMBREST_SINCE: AtomicU64 = AtomicU64::new(0);
    /// The left stick held in now.
    pub static LEFT_CLICK: AtomicBool = AtomicBool::new(false);
    /// The menu's swipe was made with the left stick held: its release is no sprint.
    pub static SWIPE_USED_CLICK: AtomicBool = AtomicBool::new(false);
    pub const THUMBREST_HOLD_MS: u64 = 200;
    /// The hand whose thumbrest is the modifier (0 left, 1 right): the left one, the right one when a left-hander's
    /// sticks are traded (xr_left_handed_bindings 2) -- the chord is mirrored with the rest of the controls, its
    /// direction then on the left stick.
    pub static SIDE: AtomicU8 = AtomicU8::new(0);
    /// How the D-pad is reached, for the HUD's hints: 0 the left stick held in, 1 a thumbrest, 2 PSVR2's Triangle touch.
    pub static SCHEME: AtomicU8 = AtomicU8::new(0);
    /// The controllers have the pad's own D-pad and bumpers (Steam Frame): no chord stands in for them.
    pub static NATIVE_PAD: AtomicBool = AtomicBool::new(false);
    pub const SCHEME_CLICK: u8 = 0;
    pub const SCHEME_THUMBREST: u8 = 1;
    pub const SCHEME_TRIANGLE: u8 = 2;
    /// The input loop's frame: the raw thumbrests and the left click, at `now` ms; the modifier's thumbrest is the
    /// left one (`side` 0) or the right one.
    pub fn publish(thumbrest: [bool; 2], left_click: bool, now: u64) {
        publish_side(thumbrest, 0, left_click, now);
    }
    pub fn publish_side(thumbrest: [bool; 2], side: u8, left_click: bool, now: u64) {
        if thumbrest[0] || thumbrest[1] {
            THUMBREST_SEEN.store(true, Relaxed);
        }
        THUMBREST[0].store(thumbrest[0], Relaxed);
        THUMBREST[1].store(thumbrest[1], Relaxed);
        let side = side.min(1);
        if SIDE.swap(side, Relaxed) != side {
            // the other hand's touch never counts as this one's hold
            THUMBREST_SINCE.store(0, Relaxed);
        }
        if thumbrest[side as usize] {
            let _ = THUMBREST_SINCE.compare_exchange(0, now.max(1), Relaxed, Relaxed);
        } else {
            THUMBREST_SINCE.store(0, Relaxed);
        }
        LEFT_CLICK.store(left_click, Relaxed);
        if !left_click {
            SWIPE_USED_CLICK.store(false, Relaxed);
        }
    }
    /// This controller uses the thumbrest scheme.
    pub fn thumbrest_controller() -> bool {
        THUMBREST_SEEN.load(Relaxed)
    }
    /// The modifier's thumbrest held past 0.2 s at `now` ms.
    pub fn thumbrest_held(now: u64) -> bool {
        let since = THUMBREST_SINCE.load(Relaxed);
        since != 0 && now.saturating_sub(since) >= THUMBREST_HOLD_MS
    }
    /// The modifier held: the thumbrest's on a thumbrest controller, else the left stick's.
    pub fn held(now: u64) -> bool {
        if thumbrest_controller() {
            thumbrest_held(now)
        } else {
            LEFT_CLICK.load(Relaxed)
        }
    }
}

pub mod stick {
    use super::cpp_min;

    pub const DEFAULT_DEADZONE: f32 = 0.15;
    pub const DEFAULT_FULL_INPUT: f32 = 0.90;

    /// The centre deadzone a player may choose, 0..0.30; anything else is the default 0.15.
    #[inline]
    pub fn deadzone(value: f32) -> f32 {
        if value.is_finite() && value >= 0.0 && value <= 0.30 {
            value
        } else {
            DEFAULT_DEADZONE
        }
    }

    /// The raw travel that already gives full input, 0.80..1; anything else is the default 0.90.
    #[inline]
    pub fn full_input(value: f32) -> f32 {
        if value.is_finite() && value >= 0.80 && value <= 1.0 {
            value
        } else {
            DEFAULT_FULL_INPUT
        }
    }

    /// TE6-testing-highlight's ApplyStickRange: each axis remapped on its own from the centre deadzone to the chosen
    /// full-travel point, its sign kept.
    #[inline]
    pub fn analog_axis(value: f32, deadzone_setting: f32, full_input_setting: f32) -> f32 {
        if !value.is_finite() {
            return 0.0;
        }
        let deadzone = deadzone(deadzone_setting);
        let full = full_input(full_input_setting);
        let magnitude = value.abs();
        if magnitude <= deadzone {
            return 0.0;
        }
        cpp_min(1.0, (magnitude - deadzone) / (full - deadzone)).copysign(value)
    }

    /// Analog movement is a mode of walking only: mode 1 on foot. Driving keeps the game's own mapping.
    #[inline]
    pub fn use_analog_movement(mode: i32, vehicle: bool) -> bool {
        mode == 1 && !vehicle
    }

    /// The caller supplies raw travel in analog mode and the legacy deadzoned axis in fixed mode. Scanner ownership
    /// can zero that value before it is tested.
    #[inline]
    pub fn at_full_travel(value: f32, analog: bool, full_input_setting: f32, negative: bool) -> bool {
        if !value.is_finite() {
            return false;
        }
        let value = if negative { -value } else { value };
        if analog {
            value >= full_input(full_input_setting)
        } else {
            value > 0.90
        }
    }

    /// ONE PRESS PER PUSH (main's AnalogStick.hpp StickPulse): a stick held at its stop becomes a single short button
    /// press, re-armed only once the stick comes back under half travel in the same direction. The game's ToggleCrouch
    /// reacts to a tap alone (inputContexts.xml gives it no hold timeout), so a level-held R3 did nothing past a quick
    /// flick. A stick still held when a blocking context (the scanner's zoom, a device screen) ends must come back
    /// first, or leaving that context would press by itself.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StickPulse {
        armed: bool,
        until: u64,
    }
    impl Default for StickPulse {
        fn default() -> Self {
            StickPulse { armed: true, until: 0 }
        }
    }
    impl StickPulse {
        #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN travel re-arms, as in C++
        pub fn update(&mut self, at_stop: bool, travel: f32, now_ms: u64, pulse_ms: u32, allowed: bool) -> bool {
            if at_stop {
                if self.armed && allowed {
                    self.until = now_ms + u64::from(pulse_ms);
                }
                self.armed = false;
            } else if !(travel >= 0.5) {
                self.armed = true;
            }
            if !allowed {
                self.until = 0;
            }
            now_ms < self.until
        }
    }
}

pub mod keypad {
    use super::{cpp_clamp, cpp_min, stick};

    /// How long a stick push keeps the numeric keypad claimed after its last report.
    pub const INPUT_TIMEOUT_MS: u64 = 350;

    #[inline]
    pub fn input_fresh(stamp: u64, now: u64) -> bool {
        stamp != 0 && now >= stamp && now - stamp <= INPUT_TIMEOUT_MS
    }

    /// UI_MoveCursor* reads the raw left axes, not the paired locomotion axes.
    #[inline]
    pub fn cursor_axis(value: f32, deadzone: f32) -> f32 {
        0.5 * stick::analog_axis(value, deadzone, 1.0)
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Point {
        pub x: f32,
        pub y: f32,
    }

    /// The keypad cursor in panel pixels. Same layout as the C++ `cvr::input::KeypadCursor`.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Cursor {
        pub position: Point,
        pub previous_head: Point,
        pub initialized: bool,
        pub had_head: bool,
    }

    impl Cursor {
        /// Head movement stays relative to the native panel hit. The stick works in panel pixels, so a tiny or
        /// distant screen cannot amplify its speed.
        pub fn update(&mut self, head_hit: bool, head: Point, stick: Point, size: Point, dt: f32) -> bool {
            if !size.x.is_finite() || !size.y.is_finite() || size.x < 2.0 || size.y < 2.0 {
                *self = Cursor::default();
                return false;
            }
            let head_hit = head_hit && head.x.is_finite() && head.y.is_finite();
            if !self.initialized {
                if !head_hit {
                    return false;
                }
                self.position = head;
                self.initialized = true;
            } else if head_hit && self.had_head {
                self.position.x += head.x - self.previous_head.x;
                self.position.y += head.y - self.previous_head.y;
            }
            self.had_head = head_hit;
            if head_hit {
                self.previous_head = head;
            }
            let step = if dt.is_finite() { cpp_clamp(dt, 0.0, 0.05) } else { 0.0 };
            let speed = 1.5 * cpp_min(size.x, size.y);
            if stick.x.is_finite() {
                self.position.x += cpp_clamp(stick.x, -0.5, 0.5) * speed * step;
            }
            if stick.y.is_finite() {
                self.position.y -= cpp_clamp(stick.y, -0.5, 0.5) * speed * step;
            }
            self.position.x = cpp_clamp(self.position.x, 1.0, size.x - 1.0);
            self.position.y = cpp_clamp(self.position.y, 1.0, size.y - 1.0);
            true
        }
    }
}

pub mod trigger {
    /// Hysteresis shared by semi-auto, full-auto and charge weapons. A held value stays held; it is never turned
    /// into a repeating pulse train. Same layout as the C++ `cvr::input::WeaponTrigger`.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WeaponTrigger {
        pub active: bool,
        pub down: bool,
        pub ready: bool,
    }

    impl WeaponTrigger {
        pub const BREAK: f32 = 0.85;
        pub const RESET: f32 = 0.20;

        pub fn update(&mut self, enabled: bool, value: f32) -> bool {
            if !enabled || !value.is_finite() {
                *self = WeaponTrigger::default();
                return false;
            }
            if !self.active {
                self.active = true;
                self.down = false;
                self.ready = value <= Self::RESET;
            }
            if value <= Self::RESET {
                self.down = false;
                self.ready = true;
            } else if self.ready && value >= Self::BREAK {
                self.down = true;
                self.ready = false;
            }
            self.down
        }
    }

    /// The trigger's hysteresis sampled at the XR sync (main's FingerPull): 0 released; successive held pulls have
    /// different identities even if the game polled neither the reset nor the break.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct FingerPull {
        pub trigger: WeaponTrigger,
        pub serial: u64,
    }
    impl FingerPull {
        pub fn sample(&mut self, enabled: bool, value: f32) -> u64 {
            let held = self.trigger.down;
            if !self.trigger.update(enabled, value) {
                return 0;
            }
            if !held {
                self.serial += 1;
            }
            self.serial
        }
    }

    /// One press per physical pull as the game polls it (main's FingerButton): a pull already held when the route opens
    /// (or its owner -- the weapon, the hand -- changes) never presses; a reset and re-press between two polls shows
    /// the release first.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct FingerButton {
        pub owner: u64,
        pub previous: u64,
        pub blocked: u64,
        pub active: bool,
        pub down: bool,
    }
    impl FingerButton {
        pub fn poll(&mut self, enabled: bool, source: u64, pull: u64) -> bool {
            if !enabled || !self.active || self.owner != source {
                self.active = enabled;
                self.owner = source;
                self.blocked = pull;
                self.previous = pull;
                self.down = false;
            } else {
                let missed_reset = self.down && pull != self.previous;
                self.down = pull != 0 && pull != self.blocked && !missed_reset;
                self.previous = pull;
            }
            self.down
        }
    }

    /// A menu or reload block may consume a pull without publishing it (main's FingerAction): reopening the route while
    /// that same pull is held must never manufacture a new press. A pull is published for 100 ms from its start.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct FingerAction {
        pub seen: u64,
        pub began: u64,
        pub accepted: bool,
    }
    impl FingerAction {
        pub fn sample(&mut self, pull: u64, allowed: bool, now: u64) -> u64 {
            if pull != self.seen {
                self.seen = pull;
                self.began = now;
                self.accepted = allowed;
            }
            if !allowed {
                self.accepted = false;
            }
            if self.accepted && now.wrapping_sub(self.began) <= 100 {
                pull
            } else {
                0
            }
        }
    }
}

pub mod scanner {
    /// The scanner request held only while the game confirms it (include/Hooks/ScannerGesture.hpp): an input request
    /// is not proof the game opened the scanner -- a rejected or stale one is released, a game-driven exit respected.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ScannerRequest {
        started: u64,
        pending: bool,
        accepted: bool,
    }
    impl ScannerRequest {
        pub fn update(&mut self, requested: &mut bool, vision: i32, now: u64) {
            if !*requested {
                *self = ScannerRequest::default();
                return;
            }
            if !self.pending {
                self.pending = true;
                self.started = now;
                self.accepted = false;
            }
            if vision == 1 {
                self.accepted = true;
            }
            if (self.accepted && vision == 0) || (vision != 1 && now >= self.started && now - self.started >= 750) {
                *requested = false;
                *self = ScannerRequest::default();
            }
        }
    }

    /// World slow motion belongs to the world, not to tracked hands (main's ScannerHandPhysics): the scanner and the
    /// global time scale are independent reasons, and leaving one must not resume the hands' physics while the other
    /// still holds it. Each transition advances a generation, which invalidates the work begun before it.
    #[derive(Debug, Default)]
    pub struct ScannerHandPhysics {
        /// Low bits: scanner, time scale; the rest: the transition generation.
        state: core::sync::atomic::AtomicU64,
    }
    impl ScannerHandPhysics {
        pub const fn new() -> Self {
            ScannerHandPhysics { state: core::sync::atomic::AtomicU64::new(0) }
        }
        fn reason(&self, mask: u64, active: bool) {
            use core::sync::atomic::Ordering::{AcqRel, Acquire};
            let mut before = self.state.load(Acquire);
            loop {
                let flags = if active { (before & 3) | mask } else { (before & 3) & !mask };
                if flags == before & 3 {
                    return;
                }
                let after = ((before & !3u64).wrapping_add(4)) | flags;
                match self.state.compare_exchange_weak(before, after, AcqRel, Acquire) {
                    Ok(_) => return,
                    Err(now) => before = now,
                }
            }
        }
        pub fn publish(&self, vision: i32) {
            if vision >= 0 {
                self.reason(1, vision == 1);
            }
        }
        pub fn time_scale(&self, scale: f32) {
            if scale.is_finite() && scale >= 0.0 {
                self.reason(2, scale < 0.999);
            }
        }
        pub fn epoch(&self) -> u64 {
            let value = self.state.load(core::sync::atomic::Ordering::Acquire);
            ((value >> 2) << 1) | u64::from(value & 3 != 0)
        }
        pub fn scanner_active(&self) -> bool {
            self.state.load(core::sync::atomic::Ordering::Acquire) & 1 != 0
        }
        pub fn active(&self) -> bool {
            self.epoch() & 1 != 0
        }
        pub fn accepts(&self, completed: u64) -> bool {
            completed & 1 == 0 && self.epoch() == completed
        }
    }
    /// The one the XInput merge publishes the scanner into (main's cvr::input::scannerHandPhysics).
    pub static SCANNER_HAND_PHYSICS: ScannerHandPhysics = ScannerHandPhysics::new();

    /// The left ear's zone around the controller's grip centre (below and outside the ear), the margin added while
    /// armed.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ScannerEarZone {
        pub x: f32,
        pub y: f32,
        pub z: f32,
        pub radius: f32,
        pub margin: f32,
    }
    impl ScannerEarZone {
        pub fn contains(&self, px: f32, py: f32, pz: f32, armed: bool) -> bool {
            if !px.is_finite() || !py.is_finite() || !pz.is_finite() || px > -0.04 {
                return false;
            }
            let r0 = if self.radius > 0.02 && self.radius <= 0.60 { self.radius } else { 0.22 };
            let extra = if self.margin >= 0.0 && self.margin <= 0.30 { self.margin } else { 0.04 };
            let r = r0 + if armed { extra } else { 0.0 };
            let (dx, dy, dz) = (px - self.x, py - self.y, pz - self.z);
            dx * dx + dy * dy + dz * dz <= r * r
        }
    }
    impl Default for ScannerEarZone {
        fn default() -> Self {
            ScannerEarZone { x: -0.18, y: -0.08, z: 0.08, radius: 0.22, margin: 0.04 }
        }
    }

    /// The scanner's click (main's ScannerClick): the physical LEFT stick sampled before its D-pad and deferred-sprint
    /// conversion. Only a new press inside the settled LEFT ear zone belongs to the scanner; ownership lasts through
    /// the release, even after leaving the zone or opening a menu, so it cannot become a late sprint.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct ScannerClick {
        held: bool,
        primed: bool,
        claimed: bool,
        armed: bool,
        in_ms: f64,
        out_ms: f64,
        presses: u64,
    }
    impl ScannerClick {
        /// True while the click is the scanner's (and on its release).
        #[allow(clippy::too_many_arguments)]
        pub fn update(&mut self, enabled: bool, down: bool, valid: bool, x: f32, y: f32, z: f32, dt_ms: f64, zone: &ScannerEarZone,
            enter_ms: i32, exit_ms: i32) -> bool {
            let was = self.held;
            self.held = down;
            let releasing = self.claimed && !down;
            if !down {
                self.claimed = false;
            }
            if !enabled || dt_ms > 250.0 {
                // A tracking or input pause cannot turn an unobserved press into a fresh one.
                self.armed = false;
                self.primed = false;
                self.in_ms = 0.0;
                self.out_ms = 0.0;
            } else {
                let dt = if !dt_ms.is_finite() || dt_ms < 0.0 { 0.0 } else { dt_ms.min(100.0) };
                let inside = valid && zone.contains(x, y, z, self.armed);
                if inside {
                    self.in_ms += dt;
                    self.out_ms = 0.0;
                } else {
                    self.out_ms += dt;
                    self.in_ms = 0.0;
                }
                if !self.armed && self.in_ms >= f64::from(if enter_ms > 0 { enter_ms } else { 80 }) {
                    self.armed = true;
                }
                if self.armed && self.out_ms >= f64::from(if exit_ms > 0 { exit_ms } else { 250 }) {
                    self.armed = false;
                }
                if !down {
                    self.primed = true;
                }
                if down && !was && self.primed && self.armed && inside {
                    self.claimed = true;
                    self.presses += 1;
                }
            }
            self.claimed || releasing
        }
        pub fn presses(&self) -> u64 {
            self.presses
        }
        pub fn held(&self) -> bool {
            self.held && self.claimed && self.armed
        }
    }

    /// The raw click's other owner (main's DeferredLeftStickClick): the left stick's click doubles as the D-pad's
    /// modifier, so its L3 is emitted for one frame on the release -- never for a hold another owner (the scanner, a
    /// grenade, the cyberware chord, the overlay's chord, a D-pad direction) used.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct DeferredLeftStickClick {
        held: bool,
        used: bool,
        since: u64,
        passed: bool,
    }
    impl DeferredLeftStickClick {
        /// The click held this long in a vehicle with nothing else made of it is the game's own held L3 (its autodrive is
        /// a half-second hold of it).
        pub const HOLD_MS: u64 = 400;
        pub fn update(&mut self, down: bool, other_owner: bool) -> bool {
            if other_owner {
                self.used = true;
            }
            if down {
                self.held = true;
                return false;
            }
            let emit = self.held && !self.used && !self.passed;
            *self = Self::default();
            emit
        }
        /// After `update`, each frame: in a vehicle the click held HOLD_MS with no other owner is the game's L3, held until
        /// it is let go (its release is then no click of its own).
        pub fn hold(&mut self, vehicle: bool, now: u64) -> bool {
            if !self.held || self.used || !vehicle {
                self.since = 0;
                return false;
            }
            if self.since == 0 {
                self.since = now.max(1);
            }
            self.passed |= now.wrapping_sub(self.since) >= Self::HOLD_MS;
            self.passed
        }
    }
}

/// Two guns in hand (main's Hooks/DualWieldRapidFire.hpp and DualWieldWeaponIdentity.hpp).
pub mod dual_wield {
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Two deliberate trigger breaks per hand within 600 ms request the game's Heywood Hurricane. Holding an automatic
    /// gun, one fast hand, and a pair of simultaneous first shots cannot activate it. The inputs already went through
    /// the trigger's hysteresis.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct DualWieldRapidFire {
        down: [bool; 2],
        first: [u64; 2],
        last: [u64; 2],
        request_until: u64,
        count: [u32; 2],
    }
    impl DualWieldRapidFire {
        pub const WINDOW_MS: u64 = 600;
        pub const REQUEST_MS: u64 = 200;
        pub fn update(&mut self, active: bool, left: bool, right: bool, now: u64) -> bool {
            if !active {
                *self = DualWieldRapidFire::default();
                // No request or partial cadence survives a gameplay-context change.
                self.down = [left, right];
                return false;
            }
            let held = [left, right];
            for side in 0..2 {
                if self.count[side] != 0 && (now < self.first[side] || now - self.first[side] > Self::WINDOW_MS) {
                    if self.count[side] == 2 && now >= self.last[side] && now - self.last[side] <= Self::WINDOW_MS {
                        self.count[side] = 1;
                        self.first[side] = self.last[side];
                    } else {
                        self.count[side] = 0;
                    }
                }
                if held[side] && !self.down[side] {
                    if self.count[side] == 0 {
                        self.first[side] = now;
                    } else if self.count[side] >= 2 {
                        // Only the two most recent breaks from this hand.
                        self.first[side] = self.last[side];
                    }
                    self.last[side] = now;
                    if self.count[side] < 2 {
                        self.count[side] += 1;
                    }
                }
                self.down[side] = held[side];
            }
            if self.count[0] >= 2 && self.count[1] >= 2 {
                self.request_until = now + Self::REQUEST_MS;
                self.count = [0, 0];
            }
            now < self.request_until
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum WeaponSide {
        Unknown = -1,
        Left = 0,
        Right = 1,
    }
    pub static DUAL_WEAPON_LEFT: AtomicUsize = AtomicUsize::new(0);
    pub static DUAL_WEAPON_RIGHT: AtomicUsize = AtomicUsize::new(0);
    pub fn shot_weapon_side_of(shot: usize, left: usize, right: usize) -> WeaponSide {
        if shot == 0 || left == 0 || right == 0 || left == right {
            WeaponSide::Unknown
        } else if shot == left {
            WeaponSide::Left
        } else if shot == right {
            WeaponSide::Right
        } else {
            WeaponSide::Unknown
        }
    }
    /// The published attachment instances are compared only, never dereferenced or held alive: the shot's AttackData
    /// owns its weapon's identity, even once its triggers are released (the Hurricane) or both records name one model.
    pub fn publish_dual_weapons(left: usize, right: usize) {
        DUAL_WEAPON_LEFT.store(left, Ordering::Release);
        DUAL_WEAPON_RIGHT.store(right, Ordering::Release);
    }
    pub fn shot_weapon_side(shot: usize) -> WeaponSide {
        shot_weapon_side_of(shot, DUAL_WEAPON_LEFT.load(Ordering::Acquire), DUAL_WEAPON_RIGHT.load(Ordering::Acquire))
    }
}

/// The pad's B/Exit binding is routed separately (main's Hooks/GameplaySecondary.hpp): a gameplay hold is published
/// only if the controller using logical B has been up since a change of owner.
pub mod secondary {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct GameplaySecondary {
        armed_side: i32,
    }
    impl Default for GameplaySecondary {
        fn default() -> Self {
            GameplaySecondary { armed_side: -1 }
        }
    }
    impl GameplaySecondary {
        pub fn reset(&mut self) {
            self.armed_side = -1;
        }
        pub fn update(&mut self, eligible: bool, down: bool, physical_side: i32) -> bool {
            if !eligible || !down {
                self.armed_side = if eligible { physical_side } else { -1 };
                return false;
            }
            if self.armed_side == physical_side {
                return true;
            }
            self.reset();
            false
        }
    }
}

/// A pin-type belt prop's activation on a stick click (main's Input/GrenadeStick.hpp): the script side selects the
/// physical controller while one is held; that controller's fresh click is the grenade's, not the game's.
pub mod grenade_stick {
    use core::sync::atomic::{AtomicI32, AtomicU32};
    pub static GRENADE_STICK_HAND: AtomicI32 = AtomicI32::new(-1);
    pub static GRENADE_STICK_CLICKS: AtomicU32 = AtomicU32::new(0);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct GrenadeStick {
        down: [bool; 2],
        claimed: i32,
    }
    impl Default for GrenadeStick {
        fn default() -> Self {
            GrenadeStick { down: [false; 2], claimed: -1 }
        }
    }
    impl GrenadeStick {
        /// The hand whose click is claimed (-1 none).
        pub fn update(&mut self, owner: i32, eligible: bool, left: bool, right: bool) -> i32 {
            let pressed = [left, right];
            if self.claimed >= 0 && !pressed[self.claimed as usize] {
                self.claimed = -1;
            }
            if eligible && (0..2).contains(&owner) && pressed[owner as usize] && !self.down[owner as usize] && !pressed[1 - owner as usize] {
                self.claimed = owner;
                GRENADE_STICK_CLICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
            self.down = [left, right];
            self.claimed
        }
    }
}

pub mod handedness {
    use alloc::format;
    use alloc::string::{String, ToString};
    /// What each xr_left_handed_bindings choice trades (include/Hooks/Handedness.hpp): the triggers and grips always,
    /// the face buttons from 1, the sticks at 2.
    pub const fn trades_buttons(bindings: i32) -> bool {
        bindings >= 1
    }
    pub const fn trades_sticks(bindings: i32) -> bool {
        bindings >= 2
    }
    /// The controller icon atlas part of the other hand's button (Handedness.hpp MirroredPart): the triggers always,
    /// the face buttons from 1, the sticks (axes and clicks) at 2; the shoulders stay, and a part the atlas has no
    /// mirror of ("y_wide") is unchanged.
    pub fn mirrored_part(part: &str, bindings: i32) -> String {
        let swap = |from: &str, to: &str| format!("{to}{}", &part[from.len()..]);
        let trigger = part.contains("trigger");
        if part.starts_with("left_") && !part.starts_with("left_shoulder") {
            return if trigger || trades_sticks(bindings) { swap("left_", "right_") } else { part.to_string() };
        }
        if part.starts_with("right_") && !part.starts_with("right_shoulder") {
            return if trigger || trades_sticks(bindings) { swap("right_", "left_") } else { part.to_string() };
        }
        if part.starts_with("ls_") {
            return if trades_sticks(bindings) { swap("ls_", "rs_") } else { part.to_string() };
        }
        if part.starts_with("rs_") {
            return if trades_sticks(bindings) { swap("rs_", "ls_") } else { part.to_string() };
        }
        if !trades_buttons(bindings) {
            return part.to_string();
        }
        let (base, rest) = match part.find('_') {
            Some(cut) => (&part[..cut], &part[cut..]),
            None => (part, ""),
        };
        if !rest.is_empty() && rest != "_hold" && rest != "_disabled" {
            return part.to_string();
        }
        let traded = match base {
            "a" => Some("x"),
            "x" => Some("a"),
            "b" => Some("y"),
            "y" => Some("b"),
            _ => None,
        };
        traded.map_or_else(|| part.to_string(), |t| format!("{t}{rest}"))
    }
    /// The pad buttons with A/X and B/Y traded (`faces`) and the stick clicks traded (`sticks`).
    pub const fn mirrored_buttons(buttons: u16, faces: bool, sticks: bool) -> u16 {
        const A: u16 = 0x1000;
        const B: u16 = 0x2000;
        const X: u16 = 0x4000;
        const Y: u16 = 0x8000;
        const LS: u16 = 0x0040;
        const RS: u16 = 0x0080;
        let mut out = buttons;
        if faces {
            out = (out & !(A | B | X | Y))
                | (if buttons & A != 0 { X } else { 0 })
                | (if buttons & X != 0 { A } else { 0 })
                | (if buttons & B != 0 { Y } else { 0 })
                | (if buttons & Y != 0 { B } else { 0 });
        }
        if sticks {
            out = (out & !(LS | RS)) | (if buttons & LS != 0 { RS } else { 0 }) | (if buttons & RS != 0 { LS } else { 0 });
        }
        out
    }
}

pub mod wheel_hold {
    /// THE PORT'S WEAPON WHEEL TAKES THE HOLD OF Y (include/Hooks/WeaponWheelHold.hpp): a press is withheld until
    /// decided -- let go before HOLD_MS and the game gets a tap of TAP_MS; held HOLD_MS it is the wheel's to its end.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WheelHoldResult {
        pub game_y: bool,
        pub pending: bool,
        pub held: bool,
    }
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct WheelHold {
        since: u64,
        tap_until: u64,
        down: bool,
        held: bool,
        owned: bool,
    }
    impl WheelHold {
        pub const HOLD_MS: u64 = 300;
        pub const TAP_MS: u64 = 90;
        pub fn step(&mut self, y: bool, armed: bool, now: u64) -> WheelHoldResult {
            if !y {
                if self.down && self.owned && !self.held && armed {
                    self.tap_until = now + Self::TAP_MS;
                }
                self.down = false;
                self.held = false;
            } else if !self.down {
                self.down = true;
                self.since = now;
                self.owned = armed;
            } else if self.owned && !self.held && armed && now.wrapping_sub(self.since) >= Self::HOLD_MS {
                self.held = true;
            }
            if self.down && !self.held && !armed {
                self.owned = false;
            }
            WheelHoldResult {
                held: self.held,
                pending: self.down && self.owned && !self.held,
                game_y: now < self.tap_until || (self.down && !self.owned),
            }
        }
    }
}

pub mod psvr2 {
    /// Triangle held: the left stick is the D-pad, the look zeroed for the whole shift (include/Hooks/Psvr2Input.hpp).
    pub fn triangle_dpad(enabled: bool, x: &mut f32, y: &mut f32, buttons: &mut u16) -> bool {
        if !enabled {
            return false;
        }
        let mut dpad = 0u16;
        if *y > 0.5 {
            dpad |= 0x0001;
        }
        if *y < -0.5 {
            dpad |= 0x0002;
        }
        if *x < -0.5 {
            dpad |= 0x0004;
        }
        if *x > 0.5 {
            dpad |= 0x0008;
        }
        *buttons |= dpad;
        *x = 0.0;
        *y = 0.0;
        dpad != 0
    }
    /// The Sense controller's Create button at the game's poll: a tap is Start (0x10), a hold of 500 ms Back (0x20).
    /// Losing focus or the provider cancels the gesture rather than turning it into a tap.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct SenseSystemButton {
        held: bool,
        fired: bool,
        began: u64,
    }
    impl SenseSystemButton {
        pub fn update(&mut self, pressed: bool, now: u64, available: bool) -> u16 {
            if !available {
                self.held = false;
                self.fired = false;
                return 0;
            }
            let mut event = 0u16;
            if pressed {
                if !self.held {
                    self.began = now;
                    self.fired = false;
                } else if !self.fired && now.wrapping_sub(self.began) >= 500 {
                    event = 0x0020;
                    self.fired = true;
                }
            } else if self.held && !self.fired {
                event = if now.wrapping_sub(self.began) >= 500 { 0x0020 } else { 0x0010 };
            }
            self.held = pressed;
            event
        }
    }
}

pub mod prompts {
    //! What the HUD's prompts show for the pad buttons a VR controller reaches another way (cvr-rig's input_prompts).
    use alloc::format;
    use alloc::string::{String, ToString};

    /// A glyph shown for the D-pad: the modifier held (cvr_input::modifier) and a stick's direction.
    pub const CHORD_DPAD: u8 = 1;
    /// A glyph shown for RB, the combat gadget: the right grip held and the right stick pressed in (cvr_input::gadget_hold).
    pub const CHORD_GADGET: u8 = 2;
    /// LB / RB in a menu or a braindance: the grips stand in for the shoulders, the icon itself replaced by the port's.
    pub const GRIP_LEFT: u8 = 3;
    pub const GRIP_RIGHT: u8 = 4;
    /// The scanner's zoom: the left trigger held and the right stick pushed.
    pub const CHORD_ZOOM: u8 = 5;
    /// A button that does nothing in VR: its icon is hidden.
    pub const UNREACHABLE: u8 = 6;

    /// Where the player is, as far as the pad's buttons mean something else there.
    #[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
    pub struct Place {
        /// A menu, a braindance or the map: the grips are the shoulders.
        pub menu: bool,
        /// Seated in a vehicle.
        pub mounted: bool,
        /// The sprint is the stick's click (else the left stick pushed to its stop).
        pub click_sprint: bool,
    }

    /// The glyph of the physical control that gives the pad button `part` names for `action` (the display's input action,
    /// "" when it is not known), and the chord it is half of (0: the control alone). None: the button as it is.
    ///
    ///   the D-pad   a stick's push (the right stick's; the left one's when the thumbrest chord is mirrored for a
    ///               left-hander, `mirrored`); the scanner's zoom is LT + the right stick;
    ///   RB          the right stick's click with the right grip (the combat gadget's chord); alone in the scanner (its
    ///               tabs); the D-pad's left in a car (the radio); the right trigger with a gun out at the wheel (the shot);
    ///   LB / RB     in a menu the grips (the icon replaced);
    ///   R3          in the scanner the right trigger (the tag);
    ///   B, R3       the dash is the right stick pushed up, the crouch pushed down;
    ///   L3          the sprint is the left stick pushed up (unless it is set to the click).
    ///
    /// The chords are physical controls, never traded again for a left-hander; the controls alone are the merge's roles and
    /// traded with them. The hold variants keep the same glyph (the game draws the hold's own ring).
    pub fn chord_part(part: &str, action: &str, mirrored: bool, at: Place) -> Option<(String, u8)> {
        let base = part.strip_suffix("_hold").unwrap_or(part);
        let dir = match base {
            "dpad_up" => "up",
            "dpad_down" => "down",
            "dpad_left" => "left",
            "dpad_right" => "right",
            "dpad_up_down" => "updown",
            "dpad_left_right" => "leftright",
            _ => "",
        };
        let stick = if mirrored { "left" } else { "right" };
        if !dir.is_empty() {
            if matches!(action, "ZoomIn" | "ZoomOut") && !at.mounted {
                return Some((format!("right_axis_{dir}"), CHORD_ZOOM));
            }
            // a car's D-pad left is the radio: the paint, the lights and the siren on it are out of reach
            if dir == "left" && at.mounted && matches!(action, "VehicleVisualCustomization" | "CycleLights" | "VehicleSiren") {
                return Some((part.to_string(), UNREACHABLE));
            }
            return Some((format!("{stick}_axis_{dir}"), CHORD_DPAD));
        }
        // the phone's tabs (its contacts and messages) out of a menu: the D-pad's left and right switch them too
        if !at.menu && matches!((base, action), ("left_shoulder", "popup_prior") | ("right_shoulder", "popup_next")) {
            let dir = if action == "popup_prior" { "left" } else { "right" };
            return Some((format!("{stick}_axis_{dir}"), CHORD_DPAD));
        }
        match base {
            "right_shoulder" => match action {
                "VehicleInsideWheel" => Some((format!("{stick}_axis_left"), CHORD_DPAD)),
                "ShootPrimary" | "ShootTertiary" | "RangedAttack" | "MeleeAttack" | "UI_FakeRangedAttack" => Some(("right_trigger".into(), 0)),
                "DescriptionChange" | "OpenQuickHackPanel" => Some(("right_thumb".into(), 0)),
                "UseCombatGadget" => Some(("right_thumb".into(), CHORD_GADGET)),
                _ if at.menu => Some((part.to_string(), GRIP_RIGHT)),
                _ if at.mounted => None,
                _ => Some(("right_thumb".into(), CHORD_GADGET)),
            },
            "left_shoulder" if at.menu => Some((part.to_string(), GRIP_LEFT)),
            "right_thumb" if action == "TagButton" => Some(("right_trigger".into(), 0)),
            "b" if matches!(action, "Dodge" | "Dodge_Tutorial" | "UI_FakeDodge") && !at.mounted => Some(("right_axis_up".into(), 0)),
            "b" | "right_thumb" if matches!(action, "ToggleCrouch" | "Crouch") && !at.mounted => Some(("right_axis_down".into(), 0)),
            "left_thumb" if action == "ToggleSprint" && !at.click_sprint => Some(("left_axis_up".into(), 0)),
            _ => None,
        }
    }
}

pub mod gadget_hold {
    /// THE COMBAT GADGET'S CHORD (the pad's RB, which Touch controllers do not have): the grip held and the same
    /// hand's stick pressed in, held half a second -- RB from then until either lets go (the game aims the gadget while
    /// RB is held and throws it on the release). A click pressed with the grip held is the chord's from its start; let
    /// go before the half second it is the click it was (the slide's release in a physical reload): R3 for the game
    /// then, for TAP_MS. A click pressed without the grip stays the game's until it is released.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct GadgetResult {
        /// The stick's click is the chord's: withheld from the game.
        pub claimed: bool,
        /// RB held for the game.
        pub rb: bool,
        /// The click given back after all (a short press): R3 for the game.
        pub tap: bool,
    }
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct GadgetHold {
        pressed: u64,
        since: u64,
        down: bool,
        claimed: bool,
        firing: bool,
        fired: bool,
        tap_until: u64,
    }
    impl GadgetHold {
        pub const HOLD_MS: u64 = 500;
        /// How long a click given back is R3 (an XInput poll sees it).
        pub const TAP_MS: u64 = 90;
        pub const fn new() -> GadgetHold {
            GadgetHold { pressed: 0, since: 0, down: false, claimed: false, firing: false, fired: false, tap_until: 0 }
        }
        pub fn step(&mut self, click: bool, grip: f32, allowed: bool, now: u64) -> GadgetResult {
            let gripped = grip.is_finite() && if self.claimed { grip > 0.4 } else { grip >= 0.7 };
            if !click {
                let short = now.wrapping_sub(self.pressed) < Self::HOLD_MS;
                let tapped = self.claimed && !self.fired && allowed && short;
                let tap_until = if tapped { now.saturating_add(Self::TAP_MS) } else { self.tap_until };
                *self = GadgetHold { tap_until, ..GadgetHold::new() };
                return GadgetResult { tap: now < tap_until, ..GadgetResult::default() };
            }
            let tap = now < self.tap_until;
            if !self.down {
                self.down = true;
                self.pressed = now;
                if gripped && allowed {
                    self.claimed = true;
                    self.since = now.max(1);
                }
            }
            if !self.claimed {
                return GadgetResult { tap, ..GadgetResult::default() };
            }
            if !gripped || !allowed {
                // the grip let go (or a menu came up): RB ends; the click stays the chord's until it is released
                self.firing = false;
                self.since = 0;
                return GadgetResult { claimed: true, rb: false, tap };
            }
            if self.since != 0 && now.wrapping_sub(self.since) >= Self::HOLD_MS {
                self.firing = true;
                self.fired = true;
            }
            GadgetResult { claimed: true, rb: self.firing, tap }
        }
    }
}

pub mod cyberware_hold {
    /// The arm cyberware's chord, L3 with the grip held 500 ms (include/Hooks/CyberwareChord.hpp CyberwareHold).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ChordResult {
        pub claimed: bool,
        pub fire: bool,
    }
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct CyberwareHold {
        last: u64,
        since: u64,
        grip: bool,
        blocked: bool,
        claimed: bool,
        fired: bool,
    }
    impl CyberwareHold {
        pub const fn new() -> CyberwareHold {
            CyberwareHold { last: 0, since: 0, grip: false, blocked: false, claimed: false, fired: false }
        }
        pub fn step(&mut self, l3: bool, value: f32, r3: bool, allowed: bool, now: u64) -> ChordResult {
            self.grip = value.is_finite() && if self.grip { value > 0.4 } else { value >= 0.7 };
            let interrupted = self.last == 0 || now < self.last || now - self.last > 250;
            self.last = now;
            if !l3 && !self.grip {
                self.since = 0;
                self.blocked = false;
                self.claimed = false;
                self.fired = false;
                return ChordResult::default();
            }
            if !allowed || r3 || interrupted {
                self.since = 0;
                self.blocked = true;
                return ChordResult { claimed: self.claimed, fire: false };
            }
            if !l3 || !self.grip {
                self.since = 0;
                if self.claimed {
                    self.blocked = true;
                }
                return ChordResult { claimed: self.claimed, fire: false };
            }
            if self.blocked {
                return ChordResult { claimed: self.claimed, fire: false };
            }
            self.claimed = true;
            if self.since == 0 {
                self.since = now;
            }
            if !self.fired && now.wrapping_sub(self.since) >= 500 {
                self.fired = true;
                return ChordResult { claimed: true, fire: true };
            }
            ChordResult { claimed: true, fire: false }
        }
    }
}
#[cfg(test)]
mod tests;
