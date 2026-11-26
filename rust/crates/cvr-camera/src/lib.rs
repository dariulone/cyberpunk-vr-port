//! The view's placement (stage 3 of docs/rust-migration-plan.md), ported from include/Camera: where the view goes
//! (`anchor`), the ledgers and histories that tie a rendered frame to the pose it was rendered with (`eye_centre`,
//! `history`), the frames hands and scenes are read in (`frames`) and the vehicle camera's heading (`vehicle`).
pub mod anchor;
pub mod eye_centre;
pub mod external_view;
pub mod frames;
pub mod history;
pub mod image_ledger;
pub mod pose_ledger;
pub mod projection;
pub mod scene_comfort;
pub mod vehicle;
pub mod overlay_projection;
pub mod headset_fov;
pub mod fbt_helpers;
pub mod barrel_dot;
pub mod hand_wire;
pub mod sight_line;
pub mod world_scale;
