//! The C++ half's arithmetic, reproduced exactly: every Rust module ported from a header is compared with it bit
//! for bit on the same inputs.
//!
//! - `min`/`max`/`clamp` are MSVC's definitions (`b < a ? b : a`, `a < b ? b : a`, `hi < v ? hi : v < lo ? lo : v`):
//!   what they do with a NaN is part of the behaviour, and Rust's own `f32::min`/`clamp` differ there.
//! - The transcendental functions are the Universal CRT's own exports, the ones MSVC's <cmath> float overloads call
//!   (`std::remainder` -> remainderf, `std::expm1` -> expm1f, `std::hypot` -> _hypotf, ...), declared here directly so
//!   no Rust-side reimplementation can round differently.
//! - `sqrt` and `abs`/`copysign` are exact operations (sqrtss, sign bit) and use Rust's.

#[inline(always)]
pub fn min(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}

#[inline(always)]
pub fn max(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}

#[inline(always)]
pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if hi < v {
        hi
    } else if v < lo {
        lo
    } else {
        v
    }
}

/// `std::min({a, b, c})`: the first smallest (`*next < *found`).
#[inline(always)]
pub fn min3(a: f32, b: f32, c: f32) -> f32 {
    let mut m = a;
    if b < m {
        m = b;
    }
    if c < m {
        m = c;
    }
    m
}

#[allow(non_snake_case)]
mod ucrt {
    extern "C" {
        pub fn remainderf(x: f32, y: f32) -> f32;
        pub fn expm1f(x: f32) -> f32;
        pub fn expf(x: f32) -> f32;
        pub fn logf(x: f32) -> f32;
        pub fn asinf(x: f32) -> f32;
        pub fn acosf(x: f32) -> f32;
        pub fn atanf(x: f32) -> f32;
        pub fn atan2f(y: f32, x: f32) -> f32;
        pub fn sinf(x: f32) -> f32;
        pub fn cosf(x: f32) -> f32;
        pub fn tanf(x: f32) -> f32;
        pub fn tanhf(x: f32) -> f32;
        pub fn powf(x: f32, y: f32) -> f32;
        pub fn fmodf(x: f32, y: f32) -> f32;
        pub fn _hypotf(x: f32, y: f32) -> f32;
    }
}

macro_rules! crt1 {
    ($($name:ident => $f:ident),*) => {$(
        #[inline(always)]
        pub fn $name(x: f32) -> f32 {
            // SAFETY: a pure C runtime function.
            unsafe { ucrt::$f(x) }
        }
    )*};
}
macro_rules! crt2 {
    ($($name:ident => $f:ident),*) => {$(
        #[inline(always)]
        pub fn $name(x: f32, y: f32) -> f32 {
            // SAFETY: a pure C runtime function.
            unsafe { ucrt::$f(x, y) }
        }
    )*};
}
crt1!(expm1 => expm1f, exp => expf, ln => logf, asin => asinf, acos => acosf, atan => atanf, sin => sinf,
    cos => cosf, tan => tanf, tanh => tanhf);
crt2!(remainder => remainderf, atan2 => atan2f, pow => powf, fmod => fmodf, hypot => _hypotf);

#[inline(always)]
pub fn sqrt(x: f32) -> f32 {
    x.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nan_semantics_are_msvc_s() {
        let nan = f32::NAN;
        assert_eq!(min(1.0, nan), 1.0, "std::min(1, NaN) keeps the first");
        assert!(min(nan, 1.0).is_nan());
        assert!(max(nan, 1.0).is_nan(), "std::max(NaN, 1): a < b is false, a returned");
        assert_eq!(max(1.0, nan), 1.0);
        assert!(clamp(nan, 0.0, 1.0).is_nan());
        assert_eq!(min3(2.0, 1.0, 1.0), 1.0);
    }

    #[test]
    fn crt_functions_link() {
        assert_eq!(remainder(7.0, 2.0), -1.0);
        assert!((expm1(1e-3) - 0.0010005).abs() < 1e-6);
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert!((atan2(1.0, 1.0) - core::f32::consts::FRAC_PI_4).abs() < 1e-6);
    }
}
