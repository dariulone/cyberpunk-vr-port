//! The FBT calibration's file (src/Fbt/Game/Calibration.cpp, Save): fbt_calibration.ini in the config folder, written
//! whole through the C runtime as the .cpp's text-mode `fprintf`s wrote it -- the header comment; the version, scale and
//! eye heights; the origin, yaw inverse and head rotation inverse; the arm lengths; each captured device's offset and
//! rotation inverse (waist, feet, hands).
//!
//! In the Rust build the .cpp calls `cvr_fbt_save_calibration` with its path and the calibration (mirrored here); its
//! log stays there. The unit test below writes a file in the temp folder.

use core::ffi::{c_char, CStr};
use cvr_crt::{self as crt, Arg};

/// wrpuppets::fbt::Calibration (externals/wrpuppets-fbt-ik/include/Fbt/FbtCalibration.hpp).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Calibration {
    pub done: bool,
    pub scale: f32,
    pub player_eye_height: f32,
    pub avatar_eye_height: f32,
    pub origin: [f32; 3],
    pub yaw_inverse: [f32; 4],
    pub head_rot_inverse: [f32; 4],
    pub valid: [bool; 5],
    pub offset: [[f32; 3]; 5],
    pub rot_inverse: [[f32; 4]; 5],
    pub arm_length: [f32; 2],
}
const _: () = assert!(
    core::mem::size_of::<Calibration>() == 216
        && core::mem::offset_of!(Calibration, valid) == 60
        && core::mem::offset_of!(Calibration, offset) == 68
        && core::mem::offset_of!(Calibration, arm_length) == 208
);
const KEYS: [&CStr; 5] = [c"waist", c"left_foot", c"right_foot", c"left_hand", c"right_hand"];

fn f(v: f32) -> Arg {
    Arg::F(f64::from(v))
}
/// The file's text pieces, in the .cpp's fprintf order.
pub fn pieces(c: &Calibration) -> Vec<std::ffi::CString> {
    let mut out = vec![
        crt::format(c"; Full-body tracking calibration (src/Fbt/FbtCalibration). Room = STAGE in V's model axes, metres.\n", &[]),
        crt::format(c"version=2\nscale=%.7f\nplayer_eye_height=%.5f\nv_eye_height=%.5f\n", &[f(c.scale), f(c.player_eye_height), f(c.avatar_eye_height)]),
        crt::format(
            c"origin=%.6f,%.6f,%.6f\nyaw_inverse=%.7f,%.7f,%.7f,%.7f\nhead_rot_inverse=%.7f,%.7f,%.7f,%.7f\n",
            &[
                f(c.origin[0]),
                f(c.origin[1]),
                f(c.origin[2]),
                f(c.yaw_inverse[0]),
                f(c.yaw_inverse[1]),
                f(c.yaw_inverse[2]),
                f(c.yaw_inverse[3]),
                f(c.head_rot_inverse[0]),
                f(c.head_rot_inverse[1]),
                f(c.head_rot_inverse[2]),
                f(c.head_rot_inverse[3]),
            ],
        ),
        crt::format(c"arm_length=%.5f,%.5f\n", &[f(c.arm_length[0]), f(c.arm_length[1])]),
    ];
    for (i, key) in KEYS.iter().enumerate() {
        if !c.valid[i] {
            continue;
        }
        let (o, r) = (c.offset[i], c.rot_inverse[i]);
        out.push(crt::format(
            c"%s_offset=%.6f,%.6f,%.6f\n%s_rot_inverse=%.7f,%.7f,%.7f,%.7f\n",
            &[Arg::S(key.as_ptr()), f(o[0]), f(o[1]), f(o[2]), Arg::S(key.as_ptr()), f(r[0]), f(r[1]), f(r[2]), f(r[3])],
        ));
    }
    out
}
/// The calibration written to `path`; false when it cannot be opened.
pub fn save(path: &CStr, c: &Calibration) -> bool {
    // SAFETY: a C string path.
    let Some(mut file) = (unsafe { crt::File::open(path.as_ptr(), c"w") }) else { return false };
    for piece in pieces(c) {
        file.puts(&piece);
    }
    true
}

/// Save's file.
///
/// # Safety
/// `path` is a C string; `calibration` a live calibration.
#[no_mangle]
pub unsafe extern "C" fn cvr_fbt_save_calibration(path: *const c_char, calibration: *const Calibration) -> bool {
    // SAFETY: as documented.
    unsafe { save(CStr::from_ptr(path), &*calibration) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_text() {
        let mut c = Calibration { scale: 1.0123456, player_eye_height: 1.71, avatar_eye_height: 1.65, ..Default::default() };
        c.origin = [0.1, -0.25, 0.0];
        c.yaw_inverse = [0.0, 0.0, 0.5, 0.8660254];
        c.head_rot_inverse = [0.0, 0.0, 0.0, 1.0];
        c.arm_length = [0.61, 0.6];
        c.valid[0] = true;
        c.offset[0] = [0.0, 0.05, -0.1];
        c.rot_inverse[0] = [0.0, 0.0, 0.0, 1.0];
        c.valid[3] = true;
        let path = std::env::temp_dir().join("cvr_fbt_calibration_test.ini");
        let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert!(save(&cpath, &c));
        let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let _ = std::fs::remove_file(&path);
        assert!(text.starts_with("; Full-body tracking calibration (src/Fbt/FbtCalibration). Room = STAGE in V's model axes, metres.\nversion=2\nscale=1.0123456\n"));
        assert!(text.contains("player_eye_height=1.71000\nv_eye_height=1.65000\norigin=0.100000,-0.250000,0.000000\n"));
        assert!(text.contains("yaw_inverse=0.0000000,0.0000000,0.5000000,0.8660254\nhead_rot_inverse=0.0000000,0.0000000,0.0000000,1.0000000\n"));
        assert!(text.contains("arm_length=0.61000,0.60000\nwaist_offset=0.000000,0.050000,-0.100000\nwaist_rot_inverse=0.0000000,0.0000000,0.0000000,1.0000000\n"));
        assert!(text.contains("left_hand_offset=0.000000,0.000000,0.000000\n") && !text.contains("left_foot"), "only the valid devices");
        assert!(!save(c"Z:\\no\\such\\folder\\fbt.ini", &c));
    }
}
