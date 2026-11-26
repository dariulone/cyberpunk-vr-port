//! The planar vector of roomscale (cvr::roomscale::Vec2): x right, y forward (-OpenXR Z). Its operators are the C++
//! ones, component by component.
use crate::cmath::{cos, sin};
use core::ops::{Add, Mul, Sub};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

pub const fn v2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, b: Vec2) -> Vec2 {
        v2(self.x + b.x, self.y + b.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, b: Vec2) -> Vec2 {
        v2(self.x - b.x, self.y - b.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        v2(self.x * s, self.y * s)
    }
}

pub fn dot(a: Vec2, b: Vec2) -> f32 {
    a.x * b.x + a.y * b.y
}

pub fn finite(v: Vec2) -> bool {
    v.x.is_finite() && v.y.is_finite()
}

pub fn rotate(v: Vec2, yaw: f32) -> Vec2 {
    let (c, s) = (cos(yaw), sin(yaw));
    v2(c * v.x - s * v.y, s * v.x + c * v.y)
}
