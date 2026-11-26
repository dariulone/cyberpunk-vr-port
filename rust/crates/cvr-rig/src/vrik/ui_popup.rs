//! Which UI overlay owns the B button, and the popups' visual state (formerly src/Natives/UiPopup.cpp), natives
//! registered by rust/crates/cvr-ffi/src/natives.rs.
//!
//! The phone, the radio port and the vehicle list are overlays, not menus: the game's menu-mode value stays 0 while
//! they are up, so B (Exit_Button for all three) must reach the game and must not drop the physical reload's magazine.
//! Only script can see them, so script publishes it (VRUiPopup); the moment one closes is stamped, for the drop's block
//! window.
//!
//! The popups' visual state (UISystem.RequestNewVisualState(n"inkModalPopupState")) lays one mono picture over both
//! eyes, so in VR it is not entered: requests are held back and counted, and only that many restores of the same state
//! are swallowed, which keeps the engine's state stack balanced if VR starts or stops while a popup is up.
use super::env::GetTickCount64;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};
use red4ext_abi::cname::CName;
use red4ext_abi::stack::StackFrame;

#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_PopupVisualStateFilter: i32;
    static mut CyberpunkVR_PopupVisualStateStats: [u64; 3];
    // The flags' home while their C++ readers remain (src/Natives/UiPopup.cpp in the Rust build).
    fn cvr_cpp_ui_popup_exchange(open: i32) -> i32;
    fn cvr_cpp_ui_popup_closed_store(ms: u64);
}

static HELD_POPUP_STATES: AtomicI32 = AtomicI32::new(0);
const MODAL_POPUP_STATE: CName = CName::hashed("inkModalPopupState");

/// VRUiPopup(active: Int32) -> Int32: a B-owning overlay is up (1) or not (0).
#[no_mangle]
pub unsafe extern "C" fn VRUiPopup(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut active = 0i32;
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut active);
        frame.finish_params();
    }
    let now = (active != 0) as i32;
    let prev = cvr_cpp_ui_popup_exchange(now);
    if prev != 0 && now == 0 {
        cvr_cpp_ui_popup_closed_store(GetTickCount64());
    }
    if !out.is_null() {
        *out = now;
    }
}
/// VRPopupVisualState(state: CName, request: Bool) -> Bool: whether the wrapped UISystem call goes ahead.
#[no_mangle]
pub unsafe extern "C" fn VRPopupVisualState(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let (mut state, mut request) = (CName::NONE, false);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut state);
        frame.get_parameter(&mut request);
        frame.finish_params();
    }
    let stats = &raw mut CyberpunkVR_PopupVisualStateStats;
    let mut pass = true;
    if state == MODAL_POPUP_STATE {
        if request {
            if CyberpunkVR_PopupVisualStateFilter != 0 && super::xr_calls::session_running() {
                HELD_POPUP_STATES.fetch_add(1, Ordering::Relaxed);
                (*stats)[0] += 1;
                pass = false;
            }
        } else {
            let swallowed = HELD_POPUP_STATES
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |held| (held > 0).then(|| held - 1))
                .is_ok();
            if swallowed {
                (*stats)[1] += 1;
                pass = false;
            }
        }
    }
    if pass {
        (*stats)[2] += 1;
    }
    if !out.is_null() {
        *out = pass;
    }
}
