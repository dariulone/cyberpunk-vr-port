//! The peer eye's shader camera (`include/Render/StereoShaderCamera.hpp`): from the shared 848-byte shader camera
//! block and the peer eye's current and previous camera inputs (and its temporal weight), the same-projection,
//! perspective, camera-relative scene constants -- view-projection and its inverse (through the native inverse
//! callback), projection, the jitter-free previous view-projection, the previous view-projection relative to the
//! current position (the fixed-point 1/131072 position delta), view, camera-relative view and view-projection, the
//! clip transform, world positions, axes, the jitter in clip units -- with the native matrix helper's float rounding
//! (its first add order included). And the decode of a native shader camera history (pose, view, projection, weight)
//! into the previous camera input. Previous data and temporal weight belong to the PEER eye: never the first eye's.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `BuildPeerShaderCamera` and `DecodeShaderCameraHistory` forward to `cvr_scam_*`;
//! `tools/single_pass_tests/shader_camera_test` runs its cases against them in its Rust variant.

pub type ShaderCamera = [u8; 848];
pub type History = [u8; 0x184];
/// The native matrix inverse: (source, destination) -> destination.
pub type InvertShaderMatrix = unsafe extern "C-unwind" fn(
    *const core::ffi::c_void,
    *mut core::ffi::c_void,
) -> *mut core::ffi::c_void;

/// camera_math::Matrix.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Matrix(pub [[f32; 4]; 4]);

fn read_f32(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn read_i32(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
/// camera_math::Load.
pub fn load(bytes: &[u8], at: usize) -> Matrix {
    let mut m = Matrix::default();
    for r in 0..4 {
        for c in 0..4 {
            m.0[r][c] = read_f32(bytes, at + (r * 4 + c) * 4);
        }
    }
    m
}
fn store(bytes: &mut [u8], at: usize, m: &Matrix) {
    for r in 0..4 {
        for c in 0..4 {
            bytes[at + (r * 4 + c) * 4..at + (r * 4 + c) * 4 + 4]
                .copy_from_slice(&m.0[r][c].to_le_bytes());
        }
    }
}
/// camera_math::Transpose.
pub fn transpose(a: &Matrix) -> Matrix {
    let mut m = Matrix::default();
    for r in 0..4 {
        for c in 0..4 {
            m.0[r][c] = a.0[c][r];
        }
    }
    m
}
/// camera_math::Multiply: the native helper's float rounding, its first add order included (row 1 before row 0).
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = Matrix::default();
    for row in 0..4 {
        for c in 0..4 {
            let mut value = a.0[row][1] * b.0[1][c] + a.0[row][0] * b.0[0][c];
            value += a.0[row][2] * b.0[2][c];
            value += a.0[row][3] * b.0[3][c];
            m.0[row][c] = value;
        }
    }
    m
}
/// camera_math::Relative.
pub fn relative(value: &mut Matrix) {
    value.0[3] = [0.0, 0.0, 0.0, 1.0];
}
/// camera_math::Finite.
pub fn finite(value: &Matrix) -> bool {
    value.0.iter().flatten().all(|f| f.is_finite())
}

/// BuildPeerShaderCamera.
pub fn build_peer(
    shared: &ShaderCamera,
    current: &[u8],
    previous: &[u8],
    temporal_weight: f32,
    invert: Option<&mut dyn FnMut(&Matrix) -> Matrix>,
) -> Option<ShaderCamera> {
    let width = read_u32(current, 0x378);
    let height = read_u32(current, 0x37C);
    let invert = invert?;
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || !temporal_weight.is_finite()
        || temporal_weight < 0.0
        || !(read_f32(current, 0x20) > 0.0)
        || !(read_f32(previous, 0x20) > 0.0)
        || !(read_f32(current, 0x40) > 0.0)
        || !(read_f32(current, 0x44) > read_f32(current, 0x40))
        || current[0x384] & 4 == 0
        || previous[0x384] & 4 == 0
        || read_u32(previous, 0x378) != width
        || read_u32(previous, 0x37C) != height
    {
        return None;
    }
    let mut view = load(current, 0x50);
    let projection = load(current, 0x2F0);
    let mut previous_view = load(previous, 0x50);
    let mut previous_projection = load(previous, 0x2F0);
    let vp = transpose(&load(current, 0x2B0));
    if !finite(&view)
        || !finite(&projection)
        || !finite(&previous_view)
        || !finite(&previous_projection)
        || !finite(&vp)
    {
        return None;
    }
    let inverse = invert(&vp);
    if !finite(&inverse) {
        return None;
    }
    let mut result = *shared;
    store(&mut result, 0, &vp);
    store(&mut result, 64, &inverse);
    store(&mut result, 128, &transpose(&projection));
    // The native history's projection excludes the previous temporal jitter.
    previous_projection.0[2][0] = 0.0;
    previous_projection.0[2][1] = 0.0;
    store(
        &mut result,
        192,
        &transpose(&multiply(&previous_view, &previous_projection)),
    );
    relative(&mut previous_view);
    let mut translation = Matrix::default();
    for i in 0..4 {
        translation.0[i][i] = 1.0;
    }
    let mut world = [0.0f32, 0.0, 0.0, 1.0];
    let mut old_world = [0.0f32, 0.0, 0.0, 1.0];
    let mut fixed = [0i32; 4];
    for i in 0..3 {
        let now = read_i32(current, i * 4);
        let old = read_i32(previous, i * 4);
        let delta = now as i64 - old as i64;
        if delta < i32::MIN as i64 || delta > i32::MAX as i64 {
            return None;
        }
        translation.0[3][i] = delta as f32 / 131072.0;
        world[i] = now as f32 / 131072.0;
        old_world[i] = old as f32 / 131072.0;
        fixed[i] = now;
    }
    store(
        &mut result,
        256,
        &transpose(&multiply(
            &multiply(&translation, &previous_view),
            &previous_projection,
        )),
    );
    store(&mut result, 320, &transpose(&view));
    relative(&mut view);
    store(&mut result, 384, &transpose(&view));
    store(&mut result, 448, &transpose(&multiply(&view, &projection)));
    store(&mut result, 512, &transpose(&load(current, 0x110)));
    let put = |result: &mut ShaderCamera, at: usize, bytes: &[u8]| {
        result[at..at + bytes.len()].copy_from_slice(bytes)
    };
    let floats = |values: &[f32]| {
        values
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect::<Vec<u8>>()
    };
    put(&mut result, 576, &floats(&world[..3]));
    put(&mut result, 588, &temporal_weight.to_le_bytes());
    put(&mut result, 592, &floats(&world));
    put(
        &mut result,
        608,
        &fixed
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>(),
    );
    put(&mut result, 624, &floats(&world));
    put(&mut result, 640, &current[0x250..0x250 + 48]);
    put(&mut result, 688, &floats(&old_world));
    put(&mut result, 704, &previous[0x250..0x250 + 48]);
    put(&mut result, 800, &current[0x290..0x290 + 16]);
    let jitter = [
        2.0f32 * read_f32(current, 0x370) / width as f32,
        2.0f32 * read_f32(current, 0x374) / height as f32,
    ];
    if !jitter[0].is_finite() || !jitter[1].is_finite() {
        return None;
    }
    put(&mut result, 816, &floats(&jitter));
    put(&mut result, 824, &current[0x380..0x384]);
    Some(result)
}

/// DecodeShaderCameraHistory: the previous camera input (the current one with the history's pose, view, projection
/// and axes) and the temporal weight. Native 0x297904 reads the pose at +8/+0x20, view/projection at +0x30/+0xB0 and
/// the weight at +0x180.
pub fn decode_history(current: &[u8], history: &History) -> Option<(Vec<u8>, f32)> {
    if history[0] == 0 {
        return None;
    }
    let weight = read_f32(history, 0x180);
    let view = load(history, 0x30);
    let projection = load(history, 0xB0);
    if !weight.is_finite()
        || weight < 0.0
        || !finite(&view)
        || !finite(&projection)
        || projection.0[2][0] != 0.0
        || projection.0[2][1] != 0.0
    {
        return None;
    }
    let mut result = current.to_vec();
    result[0..12].copy_from_slice(&history[8..20]);
    result[16..32].copy_from_slice(&history[0x20..0x30]);
    store(&mut result, 0x50, &view);
    store(&mut result, 0x2F0, &projection);
    const AXES: [usize; 3] = [0, 2, 1];
    for (axis, &column) in AXES.iter().enumerate() {
        let direction = [
            view.0[0][column],
            view.0[1][column],
            view.0[2][column],
            0.0f32,
        ];
        for (k, value) in direction.iter().enumerate() {
            let at = 0x250 + axis * 16 + k * 4;
            result[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Some((result, weight))
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// BuildPeerShaderCamera (`output` untouched on a refusal).
///
/// # Safety
/// `shared` and `output` are 848 bytes; `current` and `previous` are `input_bytes` each; `invert` follows the native
/// inverse's contract.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scam_build(
    shared: *const ShaderCamera,
    current: *const u8,
    previous: *const u8,
    input_bytes: usize,
    temporal_weight: f32,
    invert: Option<InvertShaderMatrix>,
    output: *mut ShaderCamera,
) -> bool {
    unsafe {
        let current = core::slice::from_raw_parts(current, input_bytes);
        let previous = core::slice::from_raw_parts(previous, input_bytes);
        let mut call = invert.map(|invert| {
            move |source: &Matrix| {
                let mut out = Matrix::default();
                invert(
                    (source as *const Matrix).cast(),
                    (&mut out as *mut Matrix).cast(),
                );
                out
            }
        });
        match build_peer(
            &*shared,
            current,
            previous,
            temporal_weight,
            call.as_mut()
                .map(|f| f as &mut dyn FnMut(&Matrix) -> Matrix),
        ) {
            Some(result) => {
                *output = result;
                true
            }
            None => false,
        }
    }
}
/// DecodeShaderCameraHistory (`previous` and `weight` untouched on a refusal).
///
/// # Safety
/// `current` and `previous` are `input_bytes` each; `history` is valid; `weight` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scam_decode(
    current: *const u8,
    input_bytes: usize,
    history: *const History,
    previous: *mut u8,
    weight: *mut f32,
) -> bool {
    unsafe {
        match decode_history(core::slice::from_raw_parts(current, input_bytes), &*history) {
            Some((result, value)) => {
                core::ptr::copy_nonoverlapping(result.as_ptr(), previous, input_bytes);
                *weight = value;
                true
            }
            None => false,
        }
    }
}
