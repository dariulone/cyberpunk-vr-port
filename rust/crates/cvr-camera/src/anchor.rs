//! Where the view goes (formerly include/Camera/AnchorTranslation.hpp): the tracked residual and the calibration
//! composed in their own axes, and THE BODY'S NECK GOES WHERE THE PLAYER'S NECK IS -- a physical turn turns the head
//! about the neck, a stick turn turns the world about the eyes, so the body's neck is put at the player's neck (the
//! calibrated eye less the lever along the tracking forward) and the body turns about its own neck. The live and
//! simulator measurements behind it are at the C++ original's lines.
use cvr_body::cmath::{cos, sin, sqrt};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchorVector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn av(x: f32, y: f32, z: f32) -> AnchorVector {
    AnchorVector { x, y, z }
}

/// How far forward of the game's seat camera the view sits in a car (metres), under the user's car offsets.
pub const CAR_CAMERA_AHEAD: f32 = 0.10;

/// The seated view's offset: the user's (the car's or the bike's trio) and, in a car, CAR_CAMERA_AHEAD; none in a
/// passenger's window combat, where the game's own lean-out camera rules.
pub fn active_vehicle_camera_offset(configured: AnchorVector, passenger_combat: bool, bike: bool) -> AnchorVector {
    if passenger_combat {
        AnchorVector::default()
    } else if bike {
        configured
    } else {
        av(configured.x, configured.y + CAR_CAMERA_AHEAD, configured.z)
    }
}

/// How far ahead of the neutral neck the on-foot camera sits (metres): a man's body, a woman's.
pub const NECK_CAMERA_AHEAD_MALE: f32 = 0.12;
pub const NECK_CAMERA_AHEAD_FEMALE: f32 = 0.10;

/// The camera's mount on the neck (formerly include/Camera/NeckCameraMount.hpp): neutral rig geometry only -- no
/// animated feet, current head rotation, previous bake or rendered-camera feedback enters it.
pub fn neck_camera_offset(neutral_neck: AnchorVector, eye_height: f32, reference_camera: AnchorVector, female: bool) -> AnchorVector {
    av(
        neutral_neck.x - reference_camera.x,
        neutral_neck.y + (if female { NECK_CAMERA_AHEAD_FEMALE } else { NECK_CAMERA_AHEAD_MALE }) - reference_camera.y,
        eye_height + 0.10 - reference_camera.z,
    )
}
pub fn neck_camera_eye_offset(neutral_neck: AnchorVector, neutral_eyes: AnchorVector, female: bool) -> AnchorVector {
    neck_camera_offset(neutral_neck, neutral_eyes.z, neutral_eyes, female)
}

pub fn rotate_anchor_yaw(v: AnchorVector, yaw: f32) -> AnchorVector {
    let (c, s) = (cos(yaw), sin(yaw));
    av(c * v.x - s * v.y, s * v.x + c * v.y, v.z)
}

/// head_residual and tracking_offset in the recenter axes; model_offset in model axes.
pub fn compose_anchor_translation(head_residual: AnchorVector, tracking_offset: AnchorVector, model_offset: AnchorVector,
    tracking_yaw: f32, model_yaw: f32) -> AnchorVector {
    let room = rotate_anchor_yaw(
        av(head_residual.x + tracking_offset.x, head_residual.y + tracking_offset.y, head_residual.z + tracking_offset.z),
        tracking_yaw,
    );
    let body = rotate_anchor_yaw(model_offset, model_yaw);
    av(room.x + body.x, room.y + body.y, room.z + body.z)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorRecipe {
    pub tracking_offset: AnchorVector,
    pub model_offset: AnchorVector,
    pub scale: f32,
    pub tracking_yaw: f32,
    pub model_yaw: f32,
    pub centre_yaw: f32,
    /// On foot the HMD owns the optical centre; body yaw places the avatar about that point.
    pub body_orbit: bool,
}

impl Default for AnchorRecipe {
    fn default() -> Self {
        AnchorRecipe {
            tracking_offset: AnchorVector::default(),
            model_offset: AnchorVector::default(),
            scale: 1.0,
            tracking_yaw: 0.0,
            model_yaw: 0.0,
            centre_yaw: 0.0,
            body_orbit: false,
        }
    }
}

pub fn compose_view_translation(residual: AnchorVector, recipe: &AnchorRecipe, tracking_yaw: f32, body_yaw: f32) -> AnchorVector {
    compose_anchor_translation(
        residual,
        recipe.tracking_offset,
        recipe.model_offset,
        tracking_yaw,
        if recipe.body_orbit { recipe.centre_yaw } else { body_yaw },
    )
}

pub const ORBIT_EYE_AHEAD_OF_NECK: f32 = 0.09;
pub const ORBIT_REST_NECK_REACH: f32 = 0.15;

pub fn body_orbit_scale(recipe: &AnchorRecipe) -> f32 {
    if recipe.scale.is_finite() && recipe.scale > 0.0 {
        recipe.scale
    } else {
        1.0
    }
}

/// The eye's lever over the neck in model axes (straight ahead), scaled.
pub fn body_orbit_lever(recipe: &AnchorRecipe) -> AnchorVector {
    av(0.0, ORBIT_EYE_AHEAD_OF_NECK * body_orbit_scale(recipe), 0.0)
}

/// THE PLAYER'S NECK, NOT AN ASSUMED ONE: the rest neck from the roomscale ledger, clamped to ORBIT_REST_NECK_REACH.
pub fn body_orbit_neck_lever(recipe: &AnchorRecipe, rest_neck: Option<&AnchorVector>) -> AnchorVector {
    let lever = body_orbit_lever(recipe);
    let Some(rest) = rest_neck else { return lever };
    if !rest.x.is_finite() || !rest.y.is_finite() {
        return lever;
    }
    let mut dx = rest.x;
    let mut dy = rest.y + ORBIT_EYE_AHEAD_OF_NECK;
    let off = sqrt(dx * dx + dy * dy);
    if off > ORBIT_REST_NECK_REACH {
        dx *= ORBIT_REST_NECK_REACH / off;
        dy *= ORBIT_REST_NECK_REACH / off;
    }
    let scale = body_orbit_scale(recipe);
    av(-dx * scale, (ORBIT_EYE_AHEAD_OF_NECK - dy) * scale, 0.0)
}

/// The body's neck in the model, as calibrated: the point a physical turn keeps in place.
pub fn body_orbit_pivot(recipe: &AnchorRecipe) -> AnchorVector {
    let lever = body_orbit_lever(recipe);
    av(recipe.model_offset.x - lever.x, recipe.model_offset.y - lever.y, 0.0)
}

pub fn body_orbit_model(recipe: &AnchorRecipe, body_yaw: f32, tracking_yaw: f32, rest_neck: Option<&AnchorVector>) -> AnchorVector {
    if !recipe.body_orbit {
        return AnchorVector::default();
    }
    let tracking_yaw = if tracking_yaw.is_finite() { tracking_yaw } else { recipe.centre_yaw };
    let lever = body_orbit_neck_lever(recipe, rest_neck);
    let eye = rotate_anchor_yaw(recipe.model_offset, recipe.centre_yaw - body_yaw);
    let behind = rotate_anchor_yaw(lever, tracking_yaw - body_yaw);
    let pivot = body_orbit_pivot(recipe);
    av(eye.x - behind.x - pivot.x, eye.y - behind.y - pivot.y, 0.0)
}

pub fn make_anchor_recipe(manual: AnchorVector, vehicle: AnchorVector, baked: AnchorVector, in_vehicle: bool, scale: f32,
    tracking_yaw: f32, model_yaw: f32) -> AnchorRecipe {
    let mut r = AnchorRecipe { scale, tracking_yaw, model_yaw, centre_yaw: model_yaw, model_offset: baked, ..AnchorRecipe::default() };
    if in_vehicle {
        // Seat adjustments keep the vehicle view basis.
        r.tracking_offset = av(manual.x + vehicle.x, manual.y + vehicle.y, manual.z + vehicle.z);
    } else {
        r.model_offset = av(baked.x + manual.x, baked.y + manual.y, baked.z + manual.z);
    }
    r
}

/// The observation already contains the active bake: keep it, add the measured difference.
pub fn absolute_horizontal_camera_bake(active: AnchorVector, body_target: AnchorVector, rendered: AnchorVector) -> AnchorVector {
    av(active.x + body_target.x - rendered.x, active.y + body_target.y - rendered.y, 0.0)
}
