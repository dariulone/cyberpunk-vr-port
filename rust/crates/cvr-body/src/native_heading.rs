//! The tracking reference the native turns move (formerly include/Runtimes/NativeBodyHeading.hpp). The locomotion
//! adapter owns the persistent body quaternion; this only consumes native turns, never a requested body turn.
use crate::cmath::remainder;

/// Same field order as the C++ class.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeBodyHeading {
    owner: usize,
    origin: u64,
    view: f32,
    body: f32,
    centre_yaw: f32,
    valid: bool,
}

pub fn wrap(yaw: f32) -> f32 {
    remainder(yaw, 6.28318530718)
}

impl NativeBodyHeading {
    pub fn reset(&mut self) {
        *self = NativeBodyHeading::default();
    }
    pub fn matches(&self, owner: usize, origin: u64) -> bool {
        self.valid && owner != 0 && owner == self.owner && origin == self.origin
    }
    pub fn owned_by(&self, owner: usize) -> bool {
        self.valid && owner != 0 && owner == self.owner
    }
    pub fn observe(&mut self, owner: usize, origin: u64, body_yaw: f32) -> bool {
        if owner == 0 || !body_yaw.is_finite() {
            return false;
        }
        if !self.matches(owner, origin) {
            if !self.valid || owner != self.owner {
                self.centre_yaw = wrap(body_yaw);
            }
            self.owner = owner;
            self.origin = origin;
            self.view = wrap(body_yaw);
            self.valid = true;
        }
        // A climb or another controller can turn the body: a body turn, never view input.
        self.body = wrap(body_yaw);
        true
    }
    pub fn commit(&mut self, body_yaw: f32, native_turn: f32) {
        if !self.valid || !body_yaw.is_finite() || !native_turn.is_finite() {
            return;
        }
        self.view = wrap(self.view + native_turn);
        self.body = wrap(body_yaw);
    }
    pub fn view(&self) -> f32 {
        self.view
    }
    pub fn body(&self) -> f32 {
        self.body
    }
    pub fn centre_yaw(&self) -> f32 {
        self.centre_yaw
    }
    pub fn offset(&self) -> f32 {
        wrap(self.body - self.view)
    }
}
