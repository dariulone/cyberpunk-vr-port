//! The HUD composite's parameters (src/Stereo/Hud.cpp, `hud_composite_params` and the exported constants only it
//! reads): the engine's HUD composite constants as read out of the game's own b6 buffer at the composite dispatch
//! (PipelineState_576) -- the curvature, the aberration, the HUD and glow gains, the shadow, the wide halo's gain and
//! lod, the bisection mode and the flicker -- exported so they can be tuned without a rebuild, laid over
//! ColorBlitHudParams' captured defaults, with the time the scanlines run on (the tick count's last 100 s, in seconds).
//!
//! The C++ half's `hud_composite_params` forwards to `cvr_hud_composite_params`; the variables are defined here and
//! exported from rust/exported-data.txt. The unit tests below check the layout, the defaults and the overlay.
#![allow(non_upper_case_globals)]

/// ColorBlitHudParams (include/Render/ColorBlit.hpp): 31 root constants.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudParams {
    pub curvature: [f32; 2],
    pub shadow_uv: [f32; 2],
    pub mip_offset1: [f32; 2],
    pub mip_offset2: [f32; 2],
    pub mip_offset4: [f32; 2],
    pub glow_weight1: f32,
    pub glow_weight2: f32,
    pub glow_weight4: f32,
    pub aberration: f32,
    pub hud_gain: f32,
    pub glow_gain: f32,
    pub shadow_strength: f32,
    pub bloom_gain: f32,
    pub bloom_lod: f32,
    pub shadow_mip: f32,
    pub time: f32,
    pub flicker: f32,
    pub target_w: f32,
    pub target_h: f32,
    pub debug_mode: f32,
    pub hud_shift_u: f32,
    pub layer_only: f32,
    pub panel_brightness: f32,
    pub panel_shadow: f32,
    pub panel_glow: f32,
    pub panel_engine_curve: f32,
    /// The vehicle map's projection (the HUD panel's only; 0 here) and its share of the canvas.
    pub windshield_map: f32,
    pub windshield_split: f32,
}
const _: () = assert!(core::mem::size_of::<HudParams>() == 33 * 4);
const _: () = assert!(core::mem::offset_of!(HudParams, target_w) == 22 * 4);
impl Default for HudParams {
    /// The captured values (ColorBlitHudParams' initialisers).
    fn default() -> Self {
        HudParams {
            curvature: [0.009017, 0.084242],
            shadow_uv: [0.0008, 0.0008],
            mip_offset1: [-0.0050, -0.0025],
            mip_offset2: [-0.0108, -0.0068],
            mip_offset4: [-0.0126, -0.0073],
            glow_weight1: 0.08,
            glow_weight2: 0.07,
            glow_weight4: 0.06,
            aberration: 0.0001,
            hud_gain: 2.0,
            glow_gain: 1.0,
            shadow_strength: 1.0,
            bloom_gain: 0.65,
            bloom_lod: 1.8,
            shadow_mip: 0.036,
            time: 0.0,
            flicker: 1.0,
            target_w: 0.0,
            target_h: 0.0,
            debug_mode: 0.0,
            hud_shift_u: 0.0,
            layer_only: 0.0,
            panel_brightness: 1.0,
            panel_shadow: 1.0,
            panel_glow: 1.0,
            panel_engine_curve: 0.0,
            windshield_map: 0.0,
            windshield_split: 1.0,
        }
    }
}

// ---- the engine's HUD composite constants (formerly Hud.cpp's exports)

#[no_mangle]
pub static mut CyberpunkVR_HudCurvatureX: f32 = 0.009017;
#[no_mangle]
pub static mut CyberpunkVR_HudCurvatureY: f32 = 0.084242;
#[no_mangle]
pub static mut CyberpunkVR_HudAberration: f32 = 0.0001;
/// The engine's x2.
#[no_mangle]
pub static mut CyberpunkVR_HudGain: f32 = 2.0;
#[no_mangle]
pub static mut CyberpunkVR_HudGlowGain: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_HudShadow: f32 = 1.0;
/// The wide halo (map, weapon icons, tracked quest). Gain is the engine's _43_m0[5].x; the lod is its 1.8 rebased
/// from the half-res pyramid it samples onto our full-res mip chain.
#[no_mangle]
pub static mut CyberpunkVR_HudBloomGain: f32 = 0.65;
#[no_mangle]
pub static mut CyberpunkVR_HudBloomLod: f32 = 1.8;
/// Sharpness bisection: 0 = full composite, 1 = HUD term only, 2 = no curvature warp, 3 = no halo, 4 = no glow,
/// 5 = no aberration. Live-switchable, no rebuild.
#[no_mangle]
pub static mut CyberpunkVR_HudDebugMode: f32 = 0.0;
/// On, and on the engine's own frame time (bound as b1 from its own constant buffer). This is not cosmetic: the
/// flicker term gates the glow, and its mean is about 0.66 -- forcing it to 1 made the halo half again too strong
/// and visibly softened the text.
#[no_mangle]
pub static mut CyberpunkVR_HudFlicker: f32 = 1.0;

/// hud_composite_params at a tick count (milliseconds).
pub fn composite(tick_ms: u64) -> HudParams {
    let mut p = HudParams::default();
    // SAFETY: the exported constants, read by value.
    unsafe {
        p.curvature = [CyberpunkVR_HudCurvatureX, CyberpunkVR_HudCurvatureY];
        p.aberration = CyberpunkVR_HudAberration;
        p.hud_gain = CyberpunkVR_HudGain;
        p.glow_gain = CyberpunkVR_HudGlowGain;
        p.shadow_strength = CyberpunkVR_HudShadow;
        p.bloom_gain = CyberpunkVR_HudBloomGain;
        p.bloom_lod = CyberpunkVR_HudBloomLod;
        p.flicker = CyberpunkVR_HudFlicker;
        p.debug_mode = CyberpunkVR_HudDebugMode;
    }
    p.time = (tick_ms % 100_000) as f32 * 0.001;
    p
}

/// # Safety
/// `out` is a writable ColorBlitHudParams.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_hud_composite_params(out: *mut HudParams) {
    // SAFETY: a plain Win32 call.
    let tick = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    // SAFETY: as the caller vouches.
    unsafe { *out = composite(tick) };
}

#[cfg(test)]
mod tests {
    //! The layout, the defaults and the overlay.
    use super::*;

    #[test]
    fn composites() {
        let p = composite(123_456);
        assert_eq!((p.curvature, p.hud_gain, p.bloom_lod, p.shadow_mip, p.panel_brightness), ([0.009017, 0.084242], 2.0, 1.8, 0.036, 1.0));
        assert!((p.time - 23.456).abs() < 1e-4, "{}", p.time);
        // SAFETY: only this test writes it.
        unsafe { CyberpunkVR_HudDebugMode = 3.0 };
        assert_eq!(composite(0).debug_mode, 3.0);
        // SAFETY: as above.
        unsafe { CyberpunkVR_HudDebugMode = 0.0 };
        assert_eq!(composite(0).target_w, 0.0);
    }
}
