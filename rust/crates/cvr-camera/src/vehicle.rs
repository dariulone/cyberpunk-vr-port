//! The heading of the vehicle MAIN camera (formerly include/Camera/VehicleStereoHeading.hpp), called under the
//! camera composition mutex. Only the selected camera contributes a native heading; the camera's aim is kept relative
//! to its vehicle-driven owner, and a frame (epoch plus the owner's rotation) keeps one base for both eyes.
use cvr_body::cmath::{atan2, sqrt};

type Quaternion = [f32; 4];

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Selection {
    pub yaw: f32,
    pub changed: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VehicleStereoHeading {
    player: usize,
    main_camera: usize,
    epoch: u64,
    local: Quaternion,
    frame_entity: Quaternion,
    native_yaw: f32,
    frame_yaw: f32,
    native_valid: bool,
    frame_valid: bool,
    frame_entity_valid: bool,
}

fn normalize(q: Option<&[f32; 4]>) -> Option<Quaternion> {
    let q = q?;
    let n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !n.is_finite() || n <= 0.25 || n >= 4.0 {
        return None;
    }
    let inv = 1.0 / sqrt(n);
    Some([q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv])
}

fn multiply(a: &Quaternion, b: &Quaternion) -> Quaternion {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn yaw(q: &Quaternion) -> Option<f32> {
    let fx = 2.0 * (q[0] * q[1] - q[2] * q[3]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    if fx * fx + fy * fy <= 1.0e-6 {
        return None;
    }
    Some(atan2(-fx, fy))
}

fn same_rotation(a: &Quaternion, b: &Quaternion) -> bool {
    a == b || *a == [-b[0], -b[1], -b[2], -b[3]]
}

impl VehicleStereoHeading {
    pub fn set_source(&mut self, player: usize, main_camera: usize) {
        if self.player == player && self.main_camera == main_camera {
            return;
        }
        self.player = player;
        self.main_camera = main_camera;
        self.native_valid = false;
        self.frame_valid = false;
    }

    pub fn observe(&mut self, camera: usize, q: Option<&[f32; 4]>, entity_q: Option<&[f32; 4]>) -> bool {
        if self.player == 0 || self.main_camera == 0 || camera != self.main_camera {
            return false;
        }
        let (Some(native), Some(entity)) = (normalize(q), normalize(entity_q)) else { return false };
        let Some(y) = yaw(&native) else { return false };
        self.native_yaw = y;
        self.local = multiply(&[-entity[0], -entity[1], -entity[2], entity[3]], &native);
        self.native_valid = true;
        true
    }

    pub fn for_frame(&mut self, epoch: u64, entity_q: Option<&[f32; 4]>, published_yaw: f32, native_offset: f32) -> Selection {
        let entity = normalize(entity_q);
        let have_entity = entity.is_some();
        let entity = entity.unwrap_or_default();
        if epoch != 0
            && self.frame_valid
            && epoch == self.epoch
            && (!have_entity || !self.frame_entity_valid || same_rotation(&entity, &self.frame_entity))
        {
            return Selection { yaw: self.frame_yaw, changed: false };
        }
        let mut y = self.native_yaw;
        if self.native_valid && have_entity {
            let world = multiply(&entity, &self.local);
            if let Some(w) = yaw(&world) {
                y = w;
                self.native_yaw = w;
            }
        }
        self.frame_yaw = if self.native_valid { y + native_offset } else { published_yaw };
        self.frame_entity = entity;
        self.frame_entity_valid = have_entity;
        self.epoch = epoch;
        self.frame_valid = true;
        Selection { yaw: self.frame_yaw, changed: true }
    }
}
