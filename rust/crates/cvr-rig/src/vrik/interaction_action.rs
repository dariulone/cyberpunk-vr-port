//! VRInteractionAction(action: CName) -> Bool (formerly src/Natives/InteractionAction.cpp, where the game's path is
//! written up): one interaction input action (ChoiceApply, ChoiceScrollUp, ChoiceScrollDown) handed straight to the
//! choice visualizers' input listener -- the one the input system feeds when the player scrolls or confirms a choice
//! hub -- and nothing else. True when a visualizer took it. The interaction manager's contexts (a DynArray<Handle> at
//! +0x80 by the code that fills it) are searched once for a listener of the known vtable and the offset kept; every
//! read is checked against the memory map first, as the C++ did.
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};
use cvr_engine::code::{image_base, readable_range};
use red4ext_abi::cname::CName;
use red4ext_abi::engine::system_named;
use red4ext_abi::stack::StackFrame;

/// The visualizers' input listener's vtable.
const LISTENER_VTABLE: usize = 0x309F198;
/// context + 0x40: the listener; the input event's action name; the result's consumed flag.
const LISTENER: usize = 0x40;
const EVENT_ACTION: usize = 0x40;
const RESULT_CONSUMED: usize = 0x40;
type OnInputFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64;

static LOGGED: AtomicI32 = AtomicI32::new(0);
static CONTEXTS_AT: AtomicI32 = AtomicI32::new(-1);

fn log(text: &str) {
    super::env::log(text);
}
unsafe fn at<T: Copy>(address: usize) -> T {
    core::ptr::read_unaligned(address as *const T)
}
fn manager() -> usize {
    for name in ["gameinteractionsCManager", "gameinteractionsIManager"] {
        let system = system_named(name);
        if !system.is_null() {
            return system as usize;
        }
    }
    0
}
/// A context's listener when `context` is one of the known layout.
unsafe fn listener_of(context: usize, want: usize) -> usize {
    if !readable_range(context, LISTENER + 8) {
        return 0;
    }
    let listener = at::<usize>(context + LISTENER);
    if readable_range(listener, 16) && at::<usize>(listener) == want {
        listener
    } else {
        0
    }
}
/// The listener in the contexts array at manager + `off` (pointer, capacity, size), or 0.
unsafe fn listener_in_array(manager: usize, off: usize, want: usize) -> usize {
    let entries = at::<usize>(manager + off);
    let cap = at::<u32>(manager + off + 8);
    let count = at::<u32>(manager + off + 0xC);
    if entries == 0 || count == 0 || count > 16 || cap < count || !readable_range(entries, 16 * count as usize) {
        return 0;
    }
    for i in 0..count as usize {
        let listener = listener_of(at::<usize>(entries + 16 * i), want);
        if listener != 0 {
            return listener;
        }
    }
    0
}
/// The first context's listener, or 0 (logged once).
unsafe fn listener() -> usize {
    if !cvr_engine::game_build::supported() {
        return 0;
    }
    let manager = manager();
    if manager == 0 || !readable_range(manager, 0x800 + 16) {
        return 0;
    }
    let want = image_base() + LISTENER_VTABLE;
    let known = CONTEXTS_AT.load(Ordering::SeqCst);
    if known >= 0 {
        let off = known as usize;
        let listener = listener_in_array(manager, off, want);
        if listener != 0 {
            return listener;
        }
        let listener = listener_of(at::<usize>(manager + off), want);
        if listener != 0 {
            return listener;
        }
    }
    for off in (0..0x800usize).step_by(8) {
        let mut listener = listener_in_array(manager, off, want);
        if listener == 0 {
            listener = listener_of(at::<usize>(manager + off), want);
        }
        if listener != 0 {
            if CONTEXTS_AT.swap(off as i32, Ordering::SeqCst) != off as i32 {
                log(&format!(
                    "[Interaction] the visualizers' listener: contexts at +0x{off:X} of the interaction manager {manager:016X}\n"
                ));
            }
            return listener;
        }
    }
    if LOGGED.swap(1, Ordering::SeqCst) == 0 {
        log(&format!(
            "[Interaction] no visualizers' listener of the known layout in the interaction manager {manager:016X}: VRInteractionAction is off\n"
        ));
    }
    0
}

#[repr(C, align(16))]
struct Block([u8; 0x100]);

#[no_mangle]
pub unsafe extern "C" fn VRInteractionAction(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut action = CName::default();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut action);
        frame.finish_params();
    }
    if let Some(out) = out.as_mut() {
        *out = false;
    }
    let listener = listener();
    if listener == 0 || action.0 == 0 {
        return;
    }
    let mut event = Block([0; 0x100]);
    let mut result = Block([0; 0x100]);
    event.0[EVENT_ACTION..EVENT_ACTION + 8].copy_from_slice(&action.0.to_le_bytes());
    let on_input = at::<OnInputFn>(at::<usize>(listener) + 8);
    on_input(
        listener as *mut c_void,
        event.0.as_mut_ptr() as *mut c_void,
        result.0.as_mut_ptr() as *mut c_void,
    );
    if let Some(out) = out.as_mut() {
        *out = result.0[RESULT_CONSUMED] != 0;
    }
}
