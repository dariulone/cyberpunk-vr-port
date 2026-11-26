//! The rendered hand's recoil displacement and its rate (main's include/Anim/HandRecoilSpring.hpp): the damped pose
//! spring advanced over the actual solve interval in closed form, a fractional frame included -- the same elapsed time
//! gives the same pose whatever the frame rate (the 2 ms substeps it replaces reached 0.93-0.97 of the analytic peak).
use crate::cmath::{cos, exp, sin, sqrt};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandRecoilSpring {
    pub x: f32,
    pub v: f32,
}
impl HandRecoilSpring {
    pub fn advance(&mut self, omega: f32, dt: f32) {
        if dt <= 0.0 || (self.x == 0.0 && self.v == 0.0) {
            return;
        }
        const DAMPING: f32 = 0.55;
        let decay = DAMPING * omega;
        let frequency = omega * sqrt(1.0 - DAMPING * DAMPING);
        let envelope = exp(-decay * dt);
        let c = cos(frequency * dt);
        let s = sin(frequency * dt) / frequency;
        let next_x = envelope * (self.x * c + (self.v + decay * self.x) * s);
        self.v = envelope * (self.v * c - (decay * self.v + omega * omega * self.x) * s);
        self.x = next_x;
        if self.x.abs() < 1e-5 && self.v.abs() < 1e-4 {
            self.x = 0.0;
            self.v = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// main's weapon_controls_test: the same elapsed time, the same pose feedback, in any number of steps.
    #[test]
    fn the_same_elapsed_time_gives_the_same_pose() {
        const OMEGA: f32 = 32.0;
        let sample = |steps: i32| {
            let mut spring = HandRecoilSpring { x: 0.0, v: OMEGA };
            for _ in 0..steps {
                spring.advance(OMEGA, 0.08 / steps as f32);
            }
            spring
        };
        let reference = sample(1);
        for steps in [4, 8, 12] {
            let result = sample(steps);
            assert!((reference.x - result.x).abs() < 0.00001 && (reference.v - result.v).abs() < 0.0001, "{steps} steps");
        }
        let mut spring = reference;
        spring.advance(OMEGA, 1.0);
        assert_eq!(spring, HandRecoilSpring::default(), "a second later it is at rest");
        let mut idle = HandRecoilSpring::default();
        idle.advance(OMEGA, 0.016);
        assert_eq!(idle, HandRecoilSpring::default());
    }
}
