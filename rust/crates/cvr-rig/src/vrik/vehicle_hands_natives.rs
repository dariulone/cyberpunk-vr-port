//! The hands in a car (main's src/Natives/VehicleHands.cpp): the drawn wrist a vehicle's door or wheel may take, and the
//! script's short lease on a hand's grip (physhands::vehicle_hands: it consumes grip input, never the hand's
//! collisions).
use super::physhands::{self, vehicle_hands};
use core::ffi::c_void;
use red4ext_abi::stack::StackFrame;

macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
/// VRVehicleHandWorld(side) -> Vector4: the fresh constrained wrist, world space (w 1); zero while that hand holds a
/// body, a thing or a person.
#[no_mangle]
pub unsafe extern "C" fn VRVehicleHandWorld(_this: *mut c_void, frame: *mut StackFrame, out: *mut [f32; 4], _: i64) {
    let mut side = -1i32;
    params!(frame, side);
    let Some(out) = out.as_mut() else { return };
    *out = [0.0; 4];
    if !(0..=1).contains(&side) {
        return;
    }
    let s = side as usize;
    if physhands::heldbody::holding_mask() & (1 << side) != 0 || physhands::adjust::holds_body(s) || physhands::grab::grabbed_person(s).0 > 0 {
        return;
    }
    if let Some(p) = physhands::pose::drawn_world(s) {
        *out = [p[0], p[1], p[2], 1.0];
    }
}
/// VRVehicleHandClaim(side, on): the grip of that hand the vehicle's for 250 ms (on), or given back.
#[no_mangle]
pub unsafe extern "C" fn VRVehicleHandClaim(_this: *mut c_void, frame: *mut StackFrame, _out: *mut c_void, _: i64) {
    let (mut side, mut on) = (-1i32, false);
    params!(frame, side, on);
    if side >= 0 {
        vehicle_hands::claim(side as usize, on);
    }
}
/// VRVehicleHandBusy(side) -> Bool: the vehicle holds that hand's grip.
#[no_mangle]
pub unsafe extern "C" fn VRVehicleHandBusy(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _: i64) {
    let mut side = -1i32;
    params!(frame, side);
    if !out.is_null() {
        *out = side >= 0 && vehicle_hands::busy(side as usize);
    }
}
