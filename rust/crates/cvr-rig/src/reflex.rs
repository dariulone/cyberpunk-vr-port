//! NVIDIA Reflex's options policy (formerly the inline half of include/Hooks/ReflexOptions.hpp, with the timing ABI of
//! include/Hooks/ReflexTiming.hpp): the latency mode the port selects is written into the game's own Streamline
//! options block, and nothing else of it changes -- the frame cap, the marker policy and the extension chain stay the
//! game's. The detour itself is `vrik::reflex_hook`.

/// -1 follows the game; 0 off (the default); 1 low latency; 2 low latency + boost.
pub const DEFAULT_MODE: i32 = 0;

const OPTIONS_TYPE: [u64; 2] = [0x49026d0bf03af81a, 0x3454215e96c451a6];

/// Streamline ReflexOptions v1, as the game supplies it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub next: *mut core::ffi::c_void,
    pub ty: [u64; 2],
    pub version: u32,
    pub padding: u32,
    pub mode: i32,
    pub frame_limit_us: u32,
    pub use_markers_to_optimize: u8,
    pub padding2: u8,
    pub virtual_key: u16,
    pub id_thread: u32,
}
const _: () = assert!(
    core::mem::size_of::<Options>() == 48
        && core::mem::offset_of!(Options, mode) == 32
        && core::mem::offset_of!(Options, frame_limit_us) == 36
        && core::mem::offset_of!(Options, id_thread) == 44
);

/// KnownOptions: version 1 of the options struct this policy was written against.
pub fn known_options(value: &Options) -> bool {
    value.version == 1 && value.ty == OPTIONS_TYPE
}
/// NormalizeMode: -1..2, anything else follows the game.
pub fn normalize_mode(mode: i32) -> i32 {
    if (-1..=2).contains(&mode) {
        mode
    } else {
        -1
    }
}
/// OverrideOptions: the game's block with the port's mode, while the XR renderer is active and the mode differs.
pub fn override_options(source: &Options, mode: i32, active: bool) -> Option<Options> {
    if !active || !(0..=2).contains(&mode) || !known_options(source) || source.mode == mode {
        return None;
    }
    Some(Options { mode, ..*source })
}

/// Streamline ReflexReport v1.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TimingFrame {
    pub next: *mut core::ffi::c_void,
    pub ty: [u64; 2],
    pub version: u64,
    pub frame_id: u64,
    pub input: u64,
    pub sim_start: u64,
    pub sim_end: u64,
    pub render_start: u64,
    pub render_end: u64,
    pub present_start: u64,
    pub present_end: u64,
    pub driver_start: u64,
    pub driver_end: u64,
    pub queue_start: u64,
    pub queue_end: u64,
    pub gpu_start: u64,
    pub gpu_end: u64,
    pub gpu_active_us: u32,
    pub gpu_frame_us: u32,
}
impl Default for TimingFrame {
    fn default() -> TimingFrame {
        TimingFrame {
            next: core::ptr::null_mut(),
            ty: [0x4453a1c80d569b37, 0x2b9557def4404dbe],
            version: 1,
            frame_id: 0,
            input: 0,
            sim_start: 0,
            sim_end: 0,
            render_start: 0,
            render_end: 0,
            present_start: 0,
            present_end: 0,
            driver_start: 0,
            driver_end: 0,
            queue_start: 0,
            queue_end: 0,
            gpu_start: 0,
            gpu_end: 0,
            gpu_active_us: 0,
            gpu_frame_us: 0,
        }
    }
}
/// Streamline ReflexState v1 (the backward-compatible prefix; no owned pointers or resources).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TimingState {
    pub next: *mut core::ffi::c_void,
    pub ty: [u64; 2],
    pub version: u64,
    pub low_latency_available: bool,
    pub latency_report_available: bool,
    pub padding: u16,
    pub stats_window_message: u32,
    pub frames: [TimingFrame; 64],
    pub flash_indicator_driver_controlled: bool,
}
impl Default for TimingState {
    fn default() -> TimingState {
        TimingState {
            next: core::ptr::null_mut(),
            ty: [0x4728daf9f0bb5985, 0x8979bda280aefdb2],
            version: 1,
            low_latency_available: false,
            latency_report_available: false,
            padding: 0,
            stats_window_message: 0,
            frames: [TimingFrame::default(); 64],
            flash_indicator_driver_controlled: false,
        }
    }
}
const _: () = assert!(
    core::mem::size_of::<TimingFrame>() == 152
        && core::mem::offset_of!(TimingFrame, gpu_start) == 128
        && core::mem::offset_of!(TimingFrame, gpu_active_us) == 144
        && core::mem::size_of::<TimingState>() == 9776
        && core::mem::offset_of!(TimingState, frames) == 40
        && core::mem::offset_of!(TimingState, flash_indicator_driver_controlled) == 9768
);

// The header's forwarders (Hooks/ReflexOptions.hpp in the Rust build).

/// # Safety
/// `value` points to an options block.
#[no_mangle]
pub unsafe extern "C" fn cvr_reflex_known_options(value: *const Options) -> bool {
    // SAFETY: the caller's block.
    unsafe { value.as_ref() }.is_some_and(known_options)
}
#[no_mangle]
pub extern "C" fn cvr_reflex_normalize_mode(mode: i32) -> i32 {
    normalize_mode(mode)
}
/// # Safety
/// `source` points to an options block; `result` to one to write (untouched when nothing is overridden).
#[no_mangle]
pub unsafe extern "C" fn cvr_reflex_override_options(source: *const Options, mode: i32, active: bool, result: *mut Options) -> bool {
    // SAFETY: the caller's blocks.
    let Some(adjusted) = unsafe { source.as_ref() }.and_then(|s| override_options(s, mode, active)) else {
        return false;
    };
    // SAFETY: the caller's output.
    unsafe { *result = adjusted };
    true
}
