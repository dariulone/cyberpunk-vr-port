//! Body yaw prediction (formerly include/Runtimes/BodyYawPrediction.hpp). It works only on an already accepted
//! three-device yaw: a wrist gesture, a tracking reset or filter catch-up is never taken for angular speed.
use crate::cmath::{clamp, expm1, max, min, min3, remainder, sqrt};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub time: f32,
    pub yaw: f32,
}

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyYawPrediction {
    previous: f32,
    smooth: f32,
    value: f32,
    rate: f32,
    previous_rate: f32,
    interval: f32,
    since_change: f32,
    initialized: bool,
    moving: bool,
    history: [Point; 6],
    count: u32,
}

fn wrap(a: f32) -> f32 {
    remainder(a, 6.28318530718)
}

impl BodyYawPrediction {
    pub fn reset(&mut self, seed: f32) {
        *self = BodyYawPrediction::default();
        self.previous = seed;
        self.smooth = seed;
        self.value = seed;
    }

    pub fn suspend(&mut self) {
        self.initialized = false;
        self.rate = 0.0;
        self.previous_rate = 0.0;
        self.interval = 0.0;
        self.since_change = 0.0;
        self.count = 0;
        self.moving = false;
        self.smooth = self.value;
    }

    fn drop_first(&mut self, n: usize) {
        // std::move(begin+1, begin+n, begin)
        for i in 1..n {
            self.history[i - 1] = self.history[i];
        }
    }

    pub fn update(&mut self, measured: f32, dt: f32, accepted: bool) -> f32 {
        if !measured.is_finite() || !dt.is_finite() || dt <= 0.0 {
            return self.value;
        }
        if !self.initialized {
            self.previous = measured;
            self.initialized = true;
        }
        let change = wrap(measured - self.previous);
        let changed = change.abs() > 0.00001;
        self.since_change += dt;
        let instant = if changed { change / self.since_change } else { 0.0 };
        self.previous = measured;
        if changed {
            self.interval = self.since_change;
            self.since_change = 0.0;
        }
        let usable = accepted && dt <= 0.05 && instant.abs() <= 12.566371;
        for i in 0..self.count as usize {
            self.history[i].time -= dt;
            self.history[i].yaw -= change;
        }
        if !usable || self.since_change > 0.05 {
            self.count = 0;
        }
        if usable {
            if self.count as usize == self.history.len() {
                self.drop_first(self.history.len());
                self.count -= 1;
            }
            self.history[self.count as usize] = Point { time: 0.0, yaw: 0.0 };
            self.count += 1;
            while self.count > 3 && self.history[0].time < -0.1 {
                self.drop_first(self.count as usize);
                self.count -= 1;
            }
        }
        let count = self.count as usize;
        let mut fitted = 0.0f32;
        let mut error = 0.0f32;
        let span = if count > 1 { self.history[count - 1].time - self.history[0].time } else { 0.0 };
        let mut stable = false;
        if count >= 3 && span >= 0.018 && span <= 0.15 {
            let (mut mt, mut my) = (0.0f32, 0.0f32);
            let h0 = self.history[0];
            for p in &self.history[..count] {
                mt += p.time - h0.time;
                my += p.yaw - h0.yaw;
            }
            mt /= count as f32;
            my /= count as f32;
            let (mut tt, mut ty) = (0.0f32, 0.0f32);
            for p in &self.history[..count] {
                let t = p.time - h0.time - mt;
                let y = p.yaw - h0.yaw - my;
                tt += t * t;
                ty += t * y;
            }
            if tt > 1e-8 {
                fitted = ty / tt;
            }
            for p in &self.history[..count] {
                let r = p.yaw - h0.yaw - my - fitted * (p.time - h0.time - mt);
                error += r * r;
            }
            error = sqrt(error / count as f32);
            stable = fitted.abs() > 0.174532925
                && fitted.abs() <= 12.566371
                && error <= max(0.006, fitted.abs() * min(self.interval, 0.035) * 0.65)
                && !(change * fitted < 0.0 && change.abs() > 0.004363323);
        }
        self.previous_rate = if stable { fitted } else { 0.0 };
        let stopped = usable && self.moving && !changed;
        self.rate = if stable && changed { fitted } else { 0.0 };
        let tau = if stable { 0.010 } else { 0.035 };
        let alpha = -expm1(-min(dt, 0.1) / tau);
        self.smooth = wrap(self.smooth + wrap(measured - self.smooth) * alpha);
        let mut target = self.smooth;
        if stable {
            let lag = dt / expm1(dt / tau);
            let residual = wrap(measured - self.smooth);
            let lead = self.previous_rate * lag;
            if lead * residual > 0.0 {
                target = wrap(self.smooth + min(lead.abs(), residual.abs()).copysign(lead));
            }
        }
        if stopped {
            target = measured;
            self.smooth = measured;
        }
        let measured_error = wrap(measured - self.value);
        let proposed = wrap(target - self.value);
        if measured_error * proposed < 0.0 {
            target = self.value;
        }
        let limit = 12.566371 * dt;
        self.value = wrap(self.value + clamp(wrap(target - self.value), -limit, limit));
        self.moving = stable;
        self.value
    }

    pub fn sample(&self, age_seconds: f32) -> f32 {
        if !age_seconds.is_finite() || age_seconds < 0.0 || self.rate == 0.0 {
            return self.value;
        }
        let horizon = min3(age_seconds, self.interval, 0.015);
        wrap(self.value + clamp(self.rate * horizon, -0.052359878, 0.052359878))
    }

    pub fn value(&self) -> f32 {
        self.value
    }
    pub fn rate(&self) -> f32 {
        self.rate
    }
}
