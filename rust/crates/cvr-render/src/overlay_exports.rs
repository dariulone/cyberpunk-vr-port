//! The barrel dots' exported variables (formerly definitions in src/Overlay/OverlayDebugDraw.cpp), defined on the Rust
//! side and exported from rust/exported-data.txt; the C++ readers keep declaring them, the tools read them by name.
//! Written by the crosshair (overlay_crosshair.rs) and read by the VRCAM eye composite and the desktop mirror.
#![allow(non_upper_case_globals)]

/// The barrel dot, in NDC, for the eye the overlay cannot reach. Read by the VRCAM eye composite (OpenXRCapture) and by
/// the desktop mirror (SyncStereo). The tick is what makes a stale value harmless: the consumers ignore it once it
/// stops being refreshed, so holstering the weapon removes the dot instead of freezing it.
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotNdcX: f32 = 0.0;
/// The SECOND eye needs its own value. The dot marks where the bullet goes, and the bullet line passes through the
/// FIRST eye (the game aligns the weapon to its camera, which is MAIN). From the second eye, one IPD off that line, the
/// same world point lies at a different angle -- exactly the parallax the reticle is now zeroed for. Without this the
/// dot and the reticle disagree by that angle in the second eye, and the instrument would contradict the thing it is
/// there to measure.
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotNdcX2: f32 = 0.0;
/// The dot parallax distance. Must match the distance sight_reflex_ps.hlsl was compiled with (build_sight.ps1 arg 2).
/// 0 = the dot marks the BORE DIRECTION in both eyes, with no per-eye parallax: back to that until the instance
/// measurement says whether the two views place the weapon differently -- a correction derived from the wrong model is
/// worse than none, because it moves the very reference the reticle is being judged against.
#[no_mangle]
pub static mut CyberpunkVR_SightZeroMeters: f32 = 20.0;
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotNdcY: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotRadiusPx: f32 = 3.0;
/// Range at which the dot marks the bullet's line. A mark can only be exact at one distance -- that is true of every
/// sight -- but at any finite value both eyes agree on the same world point, which is the part that was broken.
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotDistM: f32 = 20.0;
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotEyeSign: i32 = 1;
/// Straight nudge of the SECOND eye's dot, in NDC (screen half-widths). Negative = left. 0.0025 is about 3 mrad at this
/// field of view, i.e. 65 mm at 20 m -- the size of the error being chased. Whatever value this lands on IS that error,
/// measured, and the same number then applies to the reticle.
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotOffX2: f32 = 0.0;
/// 0 = one projection for both eyes plus a constant parallax on the second (simple, steady); 1 = a real world point
/// projected per eye (exact, but only as steady as the muzzle transform).
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotWorld: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotTick: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BarrelDotSecondEye: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugBarrelDotDraws: u64 = 0;
