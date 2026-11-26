//! CyberwareChord (formerly src/Hooks/CyberwareChord.cpp): the arm cyberware's chord, L3 with the grip held half a
//! second, claimed from the raw XR buttons before the deferred L3/D-pad and grip consumers see them.
//!
//! DELIVERY. The game's input poll dispatches a fired chord as one F18 press -- Input Loader appends F18 to
//! IconicCyberware_Button with no UI override, so the game's own context and cooldown still decide whether the
//! ability activates; unlike E it survives a keyboard rebinding, unlike LB+RB it cannot leak the shoulders'
//! scheme-specific actions. With the game window behind (the headset has the focus, BackgroundPad keeps the sticks
//! working) a key would land in the other app: the weapon mod's frame takes the chord instead, through
//! VRCyberwareChordTake, and runs PlayerPuppet.ActivateIconicCyberware itself.
use super::xinput::{InputControls, KeyInput};
use core::ffi::c_void;
use cvr_input::cyberware_hold::CyberwareHold;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_CyberwareChordSent: AtomicU64;
    fn cvr_cpp_menu_mode() -> i32;
    fn cvr_cpp_vrui_captures_input() -> bool;
    fn cvr_cpp_keypad_input_active() -> bool;
    fn cvr_cpp_device_cam_active() -> bool;
    fn cvr_cpp_braindance_active() -> bool;
    fn cvr_cpp_pose_scene_tier() -> i32;
    fn cvr_cpp_foreground_in_process() -> bool;
    fn cvr_cpp_send_input(count: u32, inputs: *const c_void, size: i32) -> u32;
    fn cvr_cpp_is_driving() -> bool;
}

static CHORD: Mutex<CyberwareHold> = Mutex::new(CyberwareHold::new());
/// Until when a fired chord may still be dispatched (VRIK_ClockMs).
static PENDING_UNTIL: AtomicU64 = AtomicU64::new(0);
/// A chord that fired with the game window behind: until when the script side may still take it.
static SCRIPT_UNTIL: AtomicU64 = AtomicU64::new(0);
/// A chord that fired in the driver seat: until when the driving mechanic may still take it (the autopilot).
static AUTOPILOT_UNTIL: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    // SAFETY: a function pointer the tests set, or GetTickCount64.
    unsafe { super::gestures::now_ms() }
}
fn allowed() -> bool {
    let mut c = InputControls::default();
    // SAFETY: C++ services; the struct is filled in place.
    unsafe {
        super::live_reads::input_controls(&mut c);
        c.xinput_hook != 0
            && cvr_cpp_menu_mode() == 0
            && !cvr_cpp_vrui_captures_input()
            && !cvr_cpp_keypad_input_active()
            && !cvr_cpp_device_cam_active()
            && !cvr_cpp_braindance_active()
            && cvr_cpp_pose_scene_tier() <= 1
    }
}

/// Called with the raw XR buttons; true while the chord claims L3 and the grip from their other consumers.
#[no_mangle]
pub extern "C" fn cvr_cyberware_chord_update(l3: bool, grip: f32, r3: bool, input_active: bool) -> bool {
    let mut chord = CHORD.lock().unwrap_or_else(|p| p.into_inner());
    let allowed = input_active && allowed();
    let now = now_ms();
    let result = chord.step(l3, grip, r3, allowed, now);
    if !allowed || r3 {
        PENDING_UNTIL.store(0, Ordering::Release);
    } else if result.fire {
        PENDING_UNTIL.store(now + 200, Ordering::Release);
    }
    result.claimed
}

/// Called by the game's input poll: delivers a fired chord as one direct key while this process owns the
/// foreground; never types into another app.
#[no_mangle]
pub extern "C" fn cvr_cyberware_chord_dispatch() {
    let until = PENDING_UNTIL.swap(0, Ordering::AcqRel);
    if until == 0 || now_ms() >= until || !allowed() {
        return;
    }
    // SAFETY: C++ services; the inputs outlive the call.
    unsafe {
        // IN THE DRIVER SEAT THE CHORD IS THE AUTOPILOT. The game's own autodrive is L3 held for half a second, which the
        // port never produces (its L3 is a one-frame press on release); the driving mechanic queues the autopilot
        // request instead, so the game's availability checks (own car, a lane, no race) still decide.
        if cvr_cpp_is_driving() {
            AUTOPILOT_UNTIL.store(now_ms() + 500, Ordering::Release);
            return;
        }
        if !cvr_cpp_foreground_in_process() {
            SCRIPT_UNTIL.store(now_ms() + 500, Ordering::Release);
            return;
        }
        const INPUT_KEYBOARD: u32 = 1;
        const KEYEVENTF_KEYUP: u32 = 0x2;
        const VK_F18: u16 = 0x81;
        let down = KeyInput { ty: INPUT_KEYBOARD, _pad: 0, vk: VK_F18, scan: 0, flags: 0, time: 0, extra: 0, _union_rest: [0; 8] };
        let up = KeyInput { flags: KEYEVENTF_KEYUP, ..down };
        let keys = [down, up];
        let size = core::mem::size_of::<KeyInput>() as i32;
        let sent = cvr_cpp_send_input(2, keys.as_ptr() as *const c_void, size);
        if sent == 1 {
            // release after a partial OS insertion
            cvr_cpp_send_input(1, keys.as_ptr().add(1) as *const c_void, size);
        }
        if sent == 2 && CyberpunkVR_RuntimeDiagnostics != 0 {
            CyberpunkVR_CyberwareChordSent.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// True once for a chord that fired while the game window was not in front (VRCyberwareChordTake). Half a second:
/// a chord the script side did not take by then (a load, a pause) is not run later.
#[no_mangle]
pub extern "C" fn cvr_cyberware_chord_take() -> bool {
    let until = SCRIPT_UNTIL.swap(0, Ordering::AcqRel);
    until != 0 && now_ms() < until && allowed()
}
/// True once for a chord that fired in the driver seat, where it switches the autopilot (VRAutopilotChordTake).
#[no_mangle]
pub extern "C" fn cvr_autopilot_chord_take() -> bool {
    let until = AUTOPILOT_UNTIL.swap(0, Ordering::AcqRel);
    // SAFETY: the player state's flag.
    until != 0 && now_ms() < until && unsafe { cvr_cpp_is_driving() }
}

