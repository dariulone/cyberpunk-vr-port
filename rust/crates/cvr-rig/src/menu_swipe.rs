//! The VR menu's summon swipe (formerly include/Natives/MenuSwipe.hpp, which in the Rust build wraps this): the right
//! hand, grip squeezed, swept straight down. Without the game or the runtime, so tools/vr_menu_tests feeds it hand
//! paths (its Rust variant links this through cvr_rust.lib).
//!
//! The swipe: at least 0.20 m down, inside 0.40 s, at 0.75 m/s or faster on average, and mostly vertical (horizontal
//! travel at most 0.65 of the drop) -- measured against a deliberate flick (~0.3 m in ~0.25 s) and against a hand
//! dropped to the side after an action (slower, and it starts low). A grip that has held something (the steering wheel,
//! a ladder, an item in the fingers) stays out of the swipe until it is released: climbing is the same motion, and the
//! climb's last pull goes on after the ladder has let the player go.
use core::ffi::c_void;
use std::collections::VecDeque;

pub const SWIPE_DROP: f32 = 0.20;
pub const SWIPE_WINDOW: f64 = 0.40;
pub const SWIPE_SPEED: f32 = 0.75;
pub const SWIPE_SLANT: f32 = 0.65;
pub const SWIPE_COOLDOWN: f64 = 0.9;
/// The history kept, a little longer than the window.
pub const SWIPE_HISTORY: f64 = 0.6;

/// cvr::vrmenu::SwipeSample.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct SwipeSample {
    /// Seconds.
    pub t: f64,
    /// Tracking space, metres, Y up.
    pub p: [f32; 3],
    /// The grip squeezed, and on nothing during this press.
    pub held: bool,
}
const _: () = assert!(core::mem::size_of::<SwipeSample>() == 24);

#[derive(Debug)]
pub struct SwipeDetector {
    history: VecDeque<SwipeSample>,
    last_fire: f64,
    /// This press of the grip has held something.
    owned: bool,
}
impl Default for SwipeDetector {
    fn default() -> Self {
        SwipeDetector {
            history: VecDeque::new(),
            last_fire: -10.0,
            owned: false,
        }
    }
}
impl SwipeDetector {
    /// One sample of the hand: `grip_down` the grip squeezed, `holding` that grip on something. Some((start, end)) once
    /// per swipe.
    pub fn push(
        &mut self,
        t: f64,
        p: &[f32; 3],
        grip_down: bool,
        holding: bool,
    ) -> Option<(SwipeSample, SwipeSample)> {
        if !grip_down {
            self.owned = false;
        } else if holding {
            self.owned = true;
        }
        self.history.push_back(SwipeSample {
            t,
            p: *p,
            held: grip_down && !self.owned,
        });
        while self
            .history
            .front()
            .is_some_and(|front| t - front.t > SWIPE_HISTORY)
        {
            self.history.pop_front();
        }
        let last = *self.history.back()?;
        if t - self.last_fire < SWIPE_COOLDOWN || self.history.len() < 3 || !last.held {
            return None;
        }
        // The earliest squeezed sample of the window from which the hand fell far and straight enough.
        let mut first = None;
        for s in self.history.iter().rev() {
            if last.t - s.t > SWIPE_WINDOW || !s.held {
                break;
            }
            let drop = s.p[1] - last.p[1];
            let (dx, dz) = (s.p[0] - last.p[0], s.p[2] - last.p[2]);
            if drop >= SWIPE_DROP
                && (dx * dx + dz * dz).sqrt() <= SWIPE_SLANT * drop
                && drop / (last.t - s.t) as f32 >= SWIPE_SPEED
            {
                first = Some(*s);
            }
        }
        let first = first?;
        self.last_fire = t;
        self.history.clear();
        Some((first, last))
    }
    /// Tracking lost or the gesture turned off: the window starts again; what the grip held is kept.
    pub fn clear(&mut self) {
        self.history.clear();
    }
}

#[no_mangle]
pub extern "C" fn cvr_swipe_new() -> *mut c_void {
    Box::into_raw(Box::<SwipeDetector>::default()).cast()
}
/// # Safety
/// `swipe` from cvr_swipe_new, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_swipe_free(swipe: *mut c_void) {
    if !swipe.is_null() {
        drop(unsafe { Box::from_raw(swipe as *mut SwipeDetector) });
    }
}
/// # Safety
/// `swipe` from cvr_swipe_new; `p` three floats; `start` / `end` a sample each, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_swipe_push(
    swipe: *mut c_void,
    t: f64,
    p: *const f32,
    grip_down: bool,
    holding: bool,
    start: *mut SwipeSample,
    end: *mut SwipeSample,
) -> bool {
    // SAFETY: as the caller vouches.
    let (swipe, p) = unsafe {
        (
            &mut *(swipe as *mut SwipeDetector),
            &*(p as *const [f32; 3]),
        )
    };
    let Some((a, b)) = swipe.push(t, p, grip_down, holding) else {
        return false;
    };
    // SAFETY: as the caller vouches.
    unsafe {
        if !start.is_null() {
            *start = a;
        }
        if !end.is_null() {
            *end = b;
        }
    }
    true
}
/// # Safety
/// `swipe` from cvr_swipe_new.
#[no_mangle]
pub unsafe extern "C" fn cvr_swipe_clear(swipe: *mut c_void) {
    // SAFETY: as the caller vouches.
    unsafe { (*(swipe as *mut SwipeDetector)).clear() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deliberate_swipe_fires_once() {
        let mut s = SwipeDetector::default();
        let (mut t, mut p, mut fires) = (10.0, [0.25f32, 1.35, -0.30], 0);
        let dt = 1.0 / 90.0;
        let mut step = |s: &mut SwipeDetector, dy: f32, n: i32| {
            for _ in 0..n {
                p[1] += dy / n as f32;
                if s.push(t, &p, true, false).is_some() {
                    fires += 1;
                }
                t += dt;
            }
        };
        step(&mut s, 0.0, 9);
        step(&mut s, -0.30, 23);
        step(&mut s, 0.0, 9);
        assert_eq!(fires, 1);
    }
}
