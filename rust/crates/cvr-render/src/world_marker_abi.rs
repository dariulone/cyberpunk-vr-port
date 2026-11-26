//! The C ABI of the world markers' arithmetic (`world_marker_math`, already Rust's) for the C++ headers it came from:
//! `include/Render/WorldMarkerProjection.hpp`, `WorldMarkerReprojection.hpp`, `WorldMarkerScale.hpp` and
//! `WorldMarkerOwnership.hpp`. In the Rust build (`CVR_RUST_VRIK`) their functions forward here, writing their outputs
//! exactly where and when the C++ ones did; `tools/world_marker_tests` runs its cases against them in its Rust variant.

use crate::world_marker_math::{
    self as wm, MarkerAnchor, MarkerOwner, MarkerScale, SourceProjection, WorldMarkerCamera,
};

/// EncodeDepth (`which` 0) / DecodeDepth (1).
#[no_mangle]
pub extern "C-unwind" fn cvr_wm_depth(which: u32, value: f32) -> f32 {
    if which == 0 {
        wm::encode_depth(value)
    } else {
        wm::decode_depth(value)
    }
}
/// TextMatrixDepth (`p` null or `bytes` floats).
///
/// # Safety
/// `p` is null or holds `bytes` bytes (112 are read only when `bytes` is 112).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_text_matrix_depth(p: *const f32, bytes: usize) -> f32 {
    if p.is_null() || bytes != 112 {
        return 0.0;
    }
    unsafe { wm::text_matrix_depth(core::slice::from_raw_parts(p, 28), bytes) }
}
/// ReprojectTextMatrix.
///
/// # Safety
/// `p` holds 28 floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_reproject_text_matrix(
    p: *mut f32,
    pixels_per_inverse_depth: f32,
) {
    unsafe {
        wm::reproject_text_matrix(
            core::slice::from_raw_parts_mut(p, 28),
            pixels_per_inverse_depth,
        )
    }
}
/// ForwardDepth.
///
/// # Safety
/// The arrays are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_forward_depth(
    point: *const [f32; 3],
    eye: *const [f32; 3],
    q: *const [f32; 4],
) -> f32 {
    unsafe { wm::forward_depth(&*point, &*eye, &*q) }
}
/// StereoScale.
#[no_mangle]
pub extern "C-unwind" fn cvr_wm_stereo_scale(
    ipd: f32,
    vertical_fov_deg: f32,
    aspect: f32,
    zoom: f32,
    main_right: bool,
) -> f32 {
    wm::stereo_scale(ipd, vertical_fov_deg, aspect, zoom, main_right)
}
/// IsInkMatrix.
///
/// # Safety
/// `m` holds 16 floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_is_ink_matrix(m: *const f32, w: f32, h: f32) -> bool {
    unsafe { wm::is_ink_matrix(core::slice::from_raw_parts(m, 16), w, h) }
}
/// IsInkCamera (`data` null or `bytes` bytes).
///
/// # Safety
/// `data` is null or holds `bytes` bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_is_ink_camera(data: *const u8, bytes: usize) -> bool {
    if data.is_null() || bytes != wm::INK_CAMERA_BYTES as usize {
        return false;
    }
    unsafe {
        wm::is_ink_camera(
            core::slice::from_raw_parts(data.cast::<f32>(), bytes / 4),
            bytes,
        )
    }
}
/// InkCameraEye.
///
/// # Safety
/// `data` holds at least 852 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_ink_camera_eye(data: *const u8) -> i32 {
    unsafe { wm::ink_camera_eye(core::slice::from_raw_parts(data, wm::INK_CAMERA_EYE + 4)) }
}
/// ShiftInkCamera.
///
/// # Safety
/// `out` and `source` hold `bytes` bytes (at least 190 floats); they may be the same block.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_shift_ink_camera(
    out: *mut f32,
    source: *const f32,
    bytes: usize,
    squeeze: f32,
    shift_x: f32,
    shift_y: f32,
    content: f32,
) -> i32 {
    unsafe {
        let floats = bytes / 4;
        // The source is read only where the output is not yet written (each matrix before it moves), as in C++; a
        // copy keeps the same reads when both name one block.
        let source: Vec<f32> = core::slice::from_raw_parts(source, floats).to_vec();
        wm::shift_ink_camera(
            core::slice::from_raw_parts_mut(out, floats),
            &source,
            bytes,
            squeeze,
            shift_x,
            shift_y,
            content,
        )
    }
}
/// ReadProjection (`out` written only on success).
///
/// # Safety
/// `camera` is null or holds 0x380 bytes; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_read_projection(
    camera: *const u8,
    width: f32,
    height: f32,
    out: *mut SourceProjection,
    unjitter: bool,
) -> bool {
    if camera.is_null() {
        return false;
    }
    unsafe {
        match wm::read_projection(
            core::slice::from_raw_parts(camera, 0x380),
            width,
            height,
            unjitter,
        ) {
            Some(value) => {
                *out = value;
                true
            }
            None => false,
        }
    }
}
/// SourceDepth.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_source_depth(
    source: *const SourceProjection,
    point: *const [f32; 3],
) -> f32 {
    unsafe { wm::source_depth(&*source, &*point) }
}
/// CanvasToWorld (`level` false) / CanvasToWorldLevel (true, with `anchor`): `world` written past the arguments.
///
/// # Safety
/// The pointers are valid (`anchor` where used); `world` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_canvas_to_world(
    level: bool,
    source: *const SourceProjection,
    anchor: *const MarkerAnchor,
    inverse_depth: f32,
    x: f32,
    y: f32,
    world: *mut [f32; 3],
) -> bool {
    unsafe {
        let written = if level {
            wm::canvas_to_world_level_written(&*source, &*anchor, inverse_depth, x, y)
        } else {
            wm::canvas_to_world_written(&*source, inverse_depth, x, y)
        };
        match written {
            Some(value) => {
                *world = value;
                value.iter().all(|v| v.is_finite())
            }
            None => false,
        }
    }
}
/// AnchorOf.
///
/// # Safety
/// The pointers are valid; `anchor` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_anchor_of(
    source: *const SourceProjection,
    point: *const [f32; 3],
    anchor: *mut MarkerAnchor,
) -> bool {
    unsafe {
        *anchor = wm::anchor_of(&*source, &*point);
        (*anchor).valid
    }
}
/// LevelAxes.
///
/// # Safety
/// The pointers are valid; `right` and `up` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_level_axes(
    source: *const SourceProjection,
    right: *mut [f32; 3],
    up: *mut [f32; 3],
) {
    unsafe {
        let (r, u) = wm::level_axes(&*source);
        *right = r;
        *up = u;
    }
}
/// TextTranslationExact (`matrix` null refused).
///
/// # Safety
/// `matrix` is null or holds 16 floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_text_translation_exact(
    matrix: *mut f32,
    x: f32,
    y: f32,
) -> bool {
    if matrix.is_null() {
        return false;
    }
    unsafe { wm::text_translation_exact(core::slice::from_raw_parts_mut(matrix, 16), x, y) }
}
/// TextToWorld (`matrix` null refused; `anchor` null for none).
///
/// # Safety
/// `matrix` is null or holds 16 floats; `source` is valid; `anchor` is null or valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_text_to_world(
    matrix: *mut [f32; 16],
    source: *const SourceProjection,
    inverse_depth: f32,
    anchor: *const MarkerAnchor,
) -> bool {
    if matrix.is_null() {
        return false;
    }
    unsafe { wm::text_to_world(&mut *matrix, &*source, inverse_depth, anchor.as_ref()) }
}
/// MarkerCamera.
///
/// # Safety
/// `eye` is valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_marker_camera(
    eye: *const SourceProjection,
    out: *mut WorldMarkerCamera,
) {
    unsafe { *out = wm::marker_camera(&*eye) }
}
/// CachedMarkerPivot (`x`, `y` written past the finiteness checks, as in C++).
///
/// # Safety
/// `x` and `y` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_cached_marker_pivot(
    width: f32,
    height: f32,
    offset_x: f32,
    offset_y: f32,
    x: *mut f32,
    y: *mut f32,
) -> bool {
    let (ok, px, py) = wm::cached_marker_pivot(width, height, offset_x, offset_y);
    // (false, 0, 0) only for a refused argument: a computed pivot of 0 is inside the bound.
    if ok || px != 0.0 || py != 0.0 {
        unsafe {
            *x = px;
            *y = py;
        }
    }
    ok
}
/// MarkerScale::Target (`which` 0: current `a`, `b`, factor `c`), Commit (1: `a`, `b` from `c`, `d`), Restore (2:
/// current `a`, `b`); `x`, `y` written on success.
///
/// # Safety
/// `scale` is valid; `x` and `y` are writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_wm_marker_scale(
    which: u32,
    scale: *mut MarkerScale,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    x: *mut f32,
    y: *mut f32,
) -> bool {
    unsafe {
        let scale = &mut *scale;
        let result = match which {
            0 => scale.target(a, b, c),
            1 => {
                scale.commit(a, b, c, d);
                return true;
            }
            _ => scale.restore(a, b),
        };
        match result {
            Some((px, py)) => {
                *x = px;
                *y = py;
                true
            }
            None => false,
        }
    }
}
/// MergeEffectOwner.
///
/// # Safety
/// `common` is valid and writable; `next` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_wm_merge_effect_owner(
    common: *mut MarkerOwner,
    next: *const MarkerOwner,
) -> bool {
    unsafe { wm::merge_effect_owner(&mut *common, *next) }
}
const _: () = assert!(
    core::mem::size_of::<MarkerAnchor>() == 24
        && core::mem::size_of::<MarkerScale>() == 20
        && core::mem::size_of::<MarkerOwner>() == 112
);
