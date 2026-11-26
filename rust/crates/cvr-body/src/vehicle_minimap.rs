//! THE MAP OVER THE HOOD (main's include/Runtimes/VehicleMinimap.hpp): where the projected minimap hangs while driving
//! -- from the middle of the hands on the wheel or the bars (game units ~ metres, model forward/up) -- and its size.
//! A car: like an automotive HUD's virtual image beyond the glass over the hood, about 1.4 m out on the measured car, a
//! little below eye level. A bike: over the middle of the bars, a hand above them, where a bike's dash display sits,
//! seen from half a metre. Sizes halved 2026-11-14 at the user's word (the car's was 0.40 x 0.24 m, the bike's
//! 0.17 x 0.10): the map with the quest text under it, which scales with it. The bike's lowered 4 cm the same day.
use crate::cmath::{atan2, cos, max, sin};
use crate::xr::{conjugate_quat, multiply_quat, rotate_vector, Quat, Vec3};

/// The anchor the wheel publishes and the HUD reads (cvr::hud::VehicleMinimapAnchor).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleMinimapAnchor {
    pub orientation: Quat,
    pub position: Vec3,
    pub stamp: u64,
    pub origin: u64,
    pub vehicle: usize,
    pub bike: bool,
}
impl Default for VehicleMinimapAnchor {
    fn default() -> Self {
        Self { orientation: Quat::IDENTITY, position: Vec3::default(), stamp: 0, origin: 0, vehicle: 0, bike: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimapPlacement {
    pub forward: f32,
    pub up: f32,
    pub width: f32,
    pub height: f32,
}
pub fn minimap_placement_for(bike: bool) -> MinimapPlacement {
    if bike {
        MinimapPlacement { forward: 0.06, up: 0.08, width: 0.085, height: 0.05 }
    } else {
        MinimapPlacement { forward: 0.92, up: 0.08, width: 0.20, height: 0.12 }
    }
}
/// The anchor is this vehicle's, in this tracking space, and at most 100 ms old.
pub fn fresh_vehicle_minimap(anchor: &VehicleMinimapAnchor, now: u64, origin: u64, vehicle: usize) -> bool {
    vehicle != 0 && anchor.vehicle == vehicle && anchor.origin == origin && anchor.stamp != 0 && now >= anchor.stamp && now - anchor.stamp <= 100
}

/// The anchor's pose in the tracking space: the existing hand/view mapping inverted (model X right / Y forward / Z up
/// to XR X right / Y up / Z back). The paired camera and XR sample cancel the HMD's motion; neither the current HMD pose
/// nor a world-space vehicle position is a pivot. `wheel` and `camera` in model space, `camera_rotation` the camera's in
/// it; `head` the XR pose that produced that camera, `recenter` the tracking base. None when it is not finite, not a
/// rotation, or more than 3 m from the eye.
pub fn build_vehicle_minimap_pose(wheel: [f32; 3], camera: [f32; 3], camera_rotation: [f32; 4], head_orientation: Quat,
    head_position: Vec3, recenter_orientation: Quat, recenter_position: Vec3, world_scale: f32, bike: bool)
    -> Option<(Quat, Vec3)> {
    if !world_scale.is_finite() || world_scale <= 0.0 {
        return None;
    }
    let mapped_camera = Quat { x: camera_rotation[0], y: camera_rotation[2], z: -camera_rotation[1], w: camera_rotation[3] };
    let model_to_local = multiply_quat(recenter_orientation, multiply_quat(head_orientation, conjugate_quat(mapped_camera)));
    // On the measured car the wheel hub is 0.48 m ahead of and 0.22 m below the eye, which puts the image 1.4 m away,
    // centred about 6 degrees below eye level.
    let place = minimap_placement_for(bike);
    let delta = Vec3 {
        x: (wheel[0] - camera[0]) / world_scale,
        y: (wheel[2] + place.up - camera[2]) / world_scale,
        z: -(wheel[1] + place.forward - camera[1]) / world_scale,
    };
    let offset = rotate_vector(model_to_local, delta);
    // Leaned back about the vehicle's right axis until it faces the eye: from half a metre over the bars a vertical
    // panel is read at a slant; the car's virtual image leans only a few degrees.
    let pitch = atan2(-delta.y, max(-delta.z, 1e-3));
    let lean = Quat { x: sin(-pitch * 0.5), y: 0.0, z: 0.0, w: cos(-pitch * 0.5) };
    let local_head = rotate_vector(recenter_orientation, head_position);
    let q = multiply_quat(model_to_local, lean);
    let p = Vec3 {
        x: recenter_position.x + local_head.x + offset.x,
        y: recenter_position.y + local_head.y + offset.y,
        z: recenter_position.z + local_head.z + offset.z,
    };
    let norm = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    let ok = (p.x + p.y + p.z + norm).is_finite() && norm > 0.98 && norm < 1.02
        && delta.x * delta.x + delta.y * delta.y + delta.z * delta.z < 9.0;
    ok.then_some((q, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_car_image_ahead_and_below() {
        // A camera at the model origin looking forward, the hub 0.48 m ahead and 0.22 m below; no head turn, no base.
        let (q, p) = build_vehicle_minimap_pose([0.0, 0.48, -0.22], [0.0; 3], [0.0, 0.0, 0.0, 1.0], Quat::IDENTITY,
            Vec3::default(), Quat::IDENTITY, Vec3::default(), 1.0, false)
        .unwrap();
        assert!((p.z + 1.40).abs() < 1e-4 && (p.y + 0.14).abs() < 1e-4 && p.x.abs() < 1e-6, "{p:?}");
        assert!((q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w - 1.0).abs() < 1e-4);
        assert!(build_vehicle_minimap_pose([0.0, 5.0, 0.0], [0.0; 3], [0.0, 0.0, 0.0, 1.0], Quat::IDENTITY,
            Vec3::default(), Quat::IDENTITY, Vec3::default(), 1.0, false).is_none());
        let a = VehicleMinimapAnchor { stamp: 1000, origin: 7, vehicle: 9, ..Default::default() };
        assert!(fresh_vehicle_minimap(&a, 1100, 7, 9) && !fresh_vehicle_minimap(&a, 1101, 7, 9) && !fresh_vehicle_minimap(&a, 1050, 8, 9));
    }
}
