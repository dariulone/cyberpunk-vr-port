//! THE PEER EYE'S CAMERA INPUT (include/Render/StereoCameraInput.hpp): the 904 camera descriptor bytes the port copies
//! and rebuilds into private storage -- at the peer's own published pose (position in 1/131072 fixed point, a unit
//! orientation quaternion) or offset from the source eye along its right axis by a signed half IPD (two symmetric
//! fixed-point truncations, as PatchCamera makes them) -- with the peer's own jitter sample, render size and phase,
//! and under the asymmetric projection the peer's own aspect and NDC centre shift; then the engine's rebuild over the
//! copy. No native camera, history or jitter counter is changed. The header forwards to these in the Rust build;
//! tools/single_pass_tests/peer_camera_test runs its cases against them.
use core::ffi::c_void;

pub const CAMERA_BYTES: usize = 904;

/// JitterSample.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct JitterSample {
    pub pixel_x: f32,
    pub pixel_y: f32,
    pub clip_x: f32,
    pub clip_y: f32,
    pub phase: u32,
}
/// PeerProjection.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PeerProjection {
    pub aspect: f32,
    pub shift_x: f32,
    pub shift_y: f32,
}
/// RebuildCameraInput: the engine's matrix rebuild over the camera bytes.
pub type RebuildCameraInput = unsafe extern "C-unwind" fn(*mut c_void);
const _: () = assert!(
    core::mem::size_of::<JitterSample>() == 20 && core::mem::size_of::<PeerProjection>() == 12
);

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn write<const N: usize>(bytes: &mut [u8], offset: usize, value: [u8; N]) {
    bytes[offset..offset + N].copy_from_slice(&value);
}

/// ApplyPeerProjection.
pub fn apply_peer_projection(result: &mut [u8], projection: Option<&PeerProjection>) {
    let Some(p) = projection else { return };
    if !p.aspect.is_finite()
        || !(p.aspect > 0.0)
        || !p.shift_x.is_finite()
        || !p.shift_y.is_finite()
    {
        return;
    }
    write(result, 0x28, p.aspect.to_le_bytes());
    write(result, 0x30, p.shift_x.to_le_bytes());
    write(result, 0x34, p.shift_y.to_le_bytes());
}

fn finish(
    result: &mut [u8],
    jitter: &JitterSample,
    width: u32,
    height: u32,
    projection: Option<&PeerProjection>,
) {
    write(result, 0x370, jitter.pixel_x.to_le_bytes());
    write(result, 0x374, jitter.pixel_y.to_le_bytes());
    write(result, 0x378, width.to_le_bytes());
    write(result, 0x37C, height.to_le_bytes());
    write(result, 0x380, jitter.phase.to_le_bytes());
    apply_peer_projection(result, projection);
}

/// PrepareCameraAtPose into `result` (the camera's first 904 bytes).
///
/// # Safety
/// `source` holds 904 bytes and `pose` 32; `rebuild` is the engine's rebuild over `result`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn prepare_camera_at_pose(
    source: *const u8,
    pose: *const u8,
    jitter: &JitterSample,
    width: u32,
    height: u32,
    rebuild: Option<RebuildCameraInput>,
    result: &mut [u8; CAMERA_BYTES],
    projection: Option<&PeerProjection>,
) -> bool {
    let Some(rebuild) = rebuild else { return false };
    if source.is_null()
        || pose.is_null()
        || width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || !jitter.pixel_x.is_finite()
        || !jitter.pixel_y.is_finite()
    {
        return false;
    }
    let pose = unsafe { core::slice::from_raw_parts(pose, 32) };
    let mut norm = 0.0f32;
    for i in 0..4 {
        let component = f32::from_le_bytes(pose[16 + i * 4..20 + i * 4].try_into().unwrap());
        if !component.is_finite() {
            return false;
        }
        norm += component * component;
    }
    if norm < 0.99 || norm > 1.01 {
        return false;
    }
    result.copy_from_slice(unsafe { core::slice::from_raw_parts(source, CAMERA_BYTES) });
    result[..12].copy_from_slice(&pose[..12]);
    result[16..32].copy_from_slice(&pose[16..32]);
    for i in 0..3 {
        let fixed = read_i32(result, i * 4);
        write(
            result,
            0x280 + i * 4,
            (fixed as f32 / 131072.0f32).to_le_bytes(),
        );
    }
    finish(result, jitter, width, height, projection);
    unsafe { rebuild(result.as_mut_ptr().cast()) };
    true
}

/// PreparePeerCamera into `result` (the camera's first 904 bytes).
///
/// # Safety
/// `source` holds 904 bytes; `rebuild` is the engine's rebuild over `result`.
#[allow(clippy::too_many_arguments)]
pub unsafe fn prepare_peer_camera(
    source: *const u8,
    right: Option<&[f32; 3]>,
    signed_half_ipd: f32,
    jitter: &JitterSample,
    width: u32,
    height: u32,
    rebuild: Option<RebuildCameraInput>,
    result: &mut [u8; CAMERA_BYTES],
    projection: Option<&PeerProjection>,
) -> bool {
    let (Some(rebuild), Some(right)) = (rebuild, right) else {
        return false;
    };
    if source.is_null()
        || width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || !signed_half_ipd.is_finite()
        || signed_half_ipd.abs() > 0.15
    {
        return false;
    }
    let mut norm = 0.0f32;
    for &component in right {
        if !component.is_finite() {
            return false;
        }
        norm += component * component;
    }
    if norm < 0.99 || norm > 1.01 || !jitter.pixel_x.is_finite() || !jitter.pixel_y.is_finite() {
        return false;
    }
    result.copy_from_slice(unsafe { core::slice::from_raw_parts(source, CAMERA_BYTES) });
    for i in 0..3 {
        // PatchCamera's two symmetric truncations in fixed-point units (each product rounded to f32).
        let half: f32 = core::hint::black_box(right[i] * signed_half_ipd);
        let fixed: f32 = core::hint::black_box(half * 131072.0f32);
        let delta = 2 * (fixed as i32) as i64;
        let position = read_i32(result, i * 4) as i64 + delta;
        if position < i32::MIN as i64 || position > i32::MAX as i64 {
            return false;
        }
        write(result, i * 4, (position as i32).to_le_bytes());
        write(
            result,
            0x280 + i * 4,
            (position as f32 / 131072.0f32).to_le_bytes(),
        );
    }
    finish(result, jitter, width, height, projection);
    unsafe { rebuild(result.as_mut_ptr().cast()) };
    true
}

/// PrepareCameraAtPose (`at_pose`) or PreparePeerCamera.
///
/// # Safety
/// As the two functions; `result` holds 904 bytes; `jitter` is valid; `projection` is null or valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_cam_prepare(
    at_pose: bool,
    source: *const u8,
    pose: *const u8,
    right: *const [f32; 3],
    signed_half_ipd: f32,
    jitter: *const JitterSample,
    width: u32,
    height: u32,
    rebuild: Option<RebuildCameraInput>,
    result: *mut [u8; CAMERA_BYTES],
    projection: *const PeerProjection,
) -> bool {
    unsafe {
        if at_pose {
            prepare_camera_at_pose(
                source,
                pose,
                &*jitter,
                width,
                height,
                rebuild,
                &mut *result,
                projection.as_ref(),
            )
        } else {
            prepare_peer_camera(
                source,
                right.as_ref(),
                signed_half_ipd,
                &*jitter,
                width,
                height,
                rebuild,
                &mut *result,
                projection.as_ref(),
            )
        }
    }
}
