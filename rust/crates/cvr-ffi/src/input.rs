//! Stick, keypad and trigger input (stage 1a): the C++ hooks call these through include/Hooks/AnalogStick.hpp,
//! KeypadInput.hpp and WeaponTrigger.hpp. None of them can panic (no indexing, no allocation, no unwrap); the pointer
//! arguments are `Option<&mut T>`, a null pointer being `None`.

use cvr_input::keypad::{self, Cursor, Point};
use cvr_input::stick;
use cvr_input::trigger::WeaponTrigger;

#[no_mangle]
pub extern "C" fn cvr_stick_deadzone(value: f32) -> f32 {
    stick::deadzone(value)
}

#[no_mangle]
pub extern "C" fn cvr_stick_full_input(value: f32) -> f32 {
    stick::full_input(value)
}

#[no_mangle]
pub extern "C" fn cvr_analog_axis(value: f32, deadzone: f32, full_input: f32) -> f32 {
    stick::analog_axis(value, deadzone, full_input)
}

#[no_mangle]
pub extern "C" fn cvr_use_analog_movement(mode: i32, vehicle: bool) -> bool {
    stick::use_analog_movement(mode, vehicle)
}

#[no_mangle]
pub extern "C" fn cvr_at_full_travel(value: f32, analog: bool, full_input: f32, negative: bool) -> bool {
    stick::at_full_travel(value, analog, full_input, negative)
}

#[no_mangle]
pub extern "C" fn cvr_keypad_input_fresh(stamp: u64, now: u64) -> bool {
    keypad::input_fresh(stamp, now)
}

#[no_mangle]
pub extern "C" fn cvr_keypad_cursor_axis(value: f32, deadzone: f32) -> f32 {
    keypad::cursor_axis(value, deadzone)
}

#[no_mangle]
pub extern "C" fn cvr_keypad_cursor_update(
    cursor: Option<&mut Cursor>,
    head_hit: bool,
    head: Point,
    stick: Point,
    size: Point,
    dt: f32,
) -> bool {
    match cursor {
        Some(cursor) => cursor.update(head_hit, head, stick, size, dt),
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn cvr_weapon_trigger_update(trigger: Option<&mut WeaponTrigger>, enabled: bool, value: f32) -> bool {
    match trigger {
        Some(trigger) => trigger.update(enabled, value),
        None => false,
    }
}

#[cfg(test)]
mod layout {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    // The same numbers are static_asserted in include/Rust/CvrRust.h.
    #[test]
    fn matches_the_header() {
        assert_eq!((size_of::<Point>(), align_of::<Point>()), (8, 4));
        assert_eq!((size_of::<Cursor>(), align_of::<Cursor>()), (20, 4));
        assert_eq!(offset_of!(Cursor, previous_head), 8);
        assert_eq!(offset_of!(Cursor, initialized), 16);
        assert_eq!(offset_of!(Cursor, had_head), 17);
        assert_eq!((size_of::<WeaponTrigger>(), align_of::<WeaponTrigger>()), (3, 1));
    }
}
