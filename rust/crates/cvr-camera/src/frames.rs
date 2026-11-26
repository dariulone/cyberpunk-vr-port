//! Frames the hands, the native camera and the scenes are read in (formerly include/Camera/{HandViewFrame,
//! NativeCameraPair,SceneCameraHeading}.hpp).
use crate::anchor::{av, compose_view_translation, rotate_anchor_yaw, AnchorRecipe, AnchorVector};
use cvr_body::cmath::{atan2, cos, sin};
use cvr_body::xr::{conjugate_quat, multiply_quat, nlerp_quat, Quat};

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandViewFrame {
    pub position: AnchorVector,
    pub rotation: Quat,
}

#[allow(clippy::too_many_arguments)]
pub fn build_hand_view_frame(body_base: AnchorVector, recipe: &AnchorRecipe, head_residual_xr: AnchorVector, head_xr: Quat,
    body_yaw: f32, tracking_yaw: f32, pitch: Quat) -> HandViewFrame {
    let delta = compose_view_translation(
        av(head_residual_xr.x * recipe.scale, -head_residual_xr.z * recipe.scale, head_residual_xr.y * recipe.scale),
        recipe,
        tracking_yaw,
        body_yaw,
    );
    let model_delta = rotate_anchor_yaw(delta, -body_yaw);
    let inv_body = Quat { x: 0.0, y: 0.0, z: -sin(body_yaw * 0.5), w: cos(body_yaw * 0.5) };
    let heading = Quat { x: 0.0, y: 0.0, z: sin(tracking_yaw * 0.5), w: cos(tracking_yaw * 0.5) };
    let mapped_head = Quat { x: head_xr.x, y: -head_xr.z, z: head_xr.y, w: head_xr.w };
    let mut rotation = multiply_quat(inv_body, multiply_quat(multiply_quat(heading, pitch), mapped_head));
    rotation = nlerp_quat(rotation, rotation, 0.0);
    HandViewFrame {
        position: av(body_base.x + model_delta.x, body_base.y + model_delta.y, body_base.z + model_delta.z),
        rotation,
    }
}

pub type FixedPosition = [i32; 3];

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraEntitySample {
    pub camera_quat: [f32; 4],
    pub entity_quat: [f32; 4],
    pub camera_minus_entity: [f32; 3],
    pub body_minus_entity: [f32; 3],
    pub valid: bool,
}

pub fn make_camera_entity_sample(centre: &FixedPosition, body_base: &FixedPosition, entity: &FixedPosition,
    camera_quat: &[f32; 4], entity_quat: &[f32; 4]) -> CameraEntitySample {
    let mut out = CameraEntitySample::default();
    let mut span = 0.0f32;
    for k in 0..3 {
        // Subtract before conversion: world floats lose sub-millimetre bits in Night City.
        out.camera_minus_entity[k] = (centre[k] as i64 - entity[k] as i64) as f32 / 131072.0;
        out.body_minus_entity[k] = (body_base[k] as i64 - entity[k] as i64) as f32 / 131072.0;
        span += out.camera_minus_entity[k] * out.camera_minus_entity[k];
    }
    for side in 0..2 {
        let src = if side == 1 { entity_quat } else { camera_quat };
        let mut norm = 0.0f64;
        for k in 0..4 {
            norm += src[k] as f64 * src[k] as f64;
        }
        if !norm.is_finite() || norm < 0.5 || norm > 1.5 {
            return out;
        }
        let inv = (1.0 / norm.sqrt()) as f32;
        let dst = if side == 1 { &mut out.entity_quat } else { &mut out.camera_quat };
        for k in 0..4 {
            dst[k] = src[k] * inv;
        }
    }
    out.valid = span.is_finite() && span >= 0.0001 && span < 9.0;
    out
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraPacketState {
    Valid = 0,
    Unavailable = 1,
    Invalidated = 2,
}

#[allow(clippy::too_many_arguments)]
pub fn camera_packet_status(owner: usize, origin: u64, stamp: u64, current_owner: usize, current_origin: u64, now: u64,
    valid: bool, unavailable: bool) -> CameraPacketState {
    if owner == 0 || owner != current_owner || origin != current_origin {
        return CameraPacketState::Invalidated;
    }
    if !valid {
        return if unavailable { CameraPacketState::Unavailable } else { CameraPacketState::Invalidated };
    }
    if stamp == 0 || now < stamp || now - stamp > 250 {
        return CameraPacketState::Unavailable;
    }
    CameraPacketState::Valid
}

pub fn scripted_camera_tier(tier: i32) -> bool {
    (2..=5).contains(&tier)
}

pub fn use_native_main_heading(mounted: bool, tier: i32, device: bool, external_scene: bool) -> bool {
    (mounted || scripted_camera_tier(tier)) && !device && !external_scene
}

/// The base from one completed composition and its head sample; no second read of a newer body yaw enters it.
pub fn composition_base_yaw(composed: &[f32; 4], head_xr: &[f32; 4]) -> Option<f32> {
    let (mut a, mut b) = (0.0f32, 0.0f32);
    for i in 0..4 {
        a += composed[i] * composed[i];
        b += head_xr[i] * head_xr[i];
    }
    if !(a + b).is_finite() || a < 0.5 || a > 1.5 || b < 0.5 || b > 1.5 {
        return None;
    }
    let base = multiply_quat(
        Quat { x: composed[0], y: composed[1], z: composed[2], w: composed[3] },
        conjugate_quat(Quat { x: head_xr[0], y: -head_xr[2], z: head_xr[1], w: head_xr[3] }),
    );
    let q = nlerp_quat(base, base, 0.0);
    let x = 2.0 * (q.x * q.y - q.z * q.w);
    let y = 1.0 - 2.0 * (q.x * q.x + q.z * q.z);
    if x * x + y * y < 1e-6 {
        return None;
    }
    Some(atan2(-x, y))
}
