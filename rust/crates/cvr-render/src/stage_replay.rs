//! The stage replay's pure rules (`include/Render/StageReplay.hpp`, `stage_replay`): the recorded operations' classes
//! (replayable graphics state and work, ignored markers / descriptor copies / compute state, blocking work), the
//! consumer hook's action for one (side, stage) call of RenderElements 0x33DDA4 (census captures; skip skips the target;
//! replay replays into the target what the source captured), the same-frame pairing, the camera and view descriptor
//! substitutions in a replayed table's sources (-1 when the target lacks a counterpart), the operation categories a
//! stream touched, and a payload's bounds inside its stream.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_sr_*`; the stream and the list shadow stay C++ containers
//! of src/Render/StageReplay.cpp. `tools/single_pass_tests/stage_replay_test` runs its cases against them in its Rust
//! variant.

/// stage_replay::Op values used by the rules.
pub mod op {
    pub const NONE: u8 = 0;
    pub const PIPELINE: u8 = 1;
    pub const ROOT_SIGNATURE: u8 = 2;
    pub const ROOT_TABLE: u8 = 3;
    pub const ROOT_CONSTANT: u8 = 4;
    pub const ROOT_CONSTANTS: u8 = 5;
    pub const ROOT_CBV: u8 = 6;
    pub const ROOT_SRV: u8 = 7;
    pub const ROOT_UAV: u8 = 8;
    pub const TOPOLOGY: u8 = 9;
    pub const INDEX_BUFFER: u8 = 10;
    pub const VERTEX_BUFFERS: u8 = 11;
    pub const VIEWPORTS: u8 = 12;
    pub const SCISSORS: u8 = 13;
    pub const BLEND_FACTOR: u8 = 14;
    pub const STENCIL_REF: u8 = 15;
    pub const DEPTH_BOUNDS: u8 = 16;
    pub const HEAPS: u8 = 17;
    pub const SHADING_RATE: u8 = 18;
    pub const SHADING_IMAGE: u8 = 19;
    pub const VIEW_INSTANCE_MASK: u8 = 20;
    pub const DRAW: u8 = 21;
    pub const DRAW_INDEXED: u8 = 22;
    pub const INDIRECT: u8 = 23;
    pub const MARKER: u8 = 24;
    pub const COPY: u8 = 25;
    pub const COMPUTE_STATE: u8 = 26;
    pub const DISPATCH: u8 = 27;
    pub const COUNT: u8 = 41;
}
/// stage_replay::Action.
pub mod action {
    pub const FORWARD: u8 = 0;
    pub const CAPTURE: u8 = 1;
    pub const SKIP: u8 = 2;
    pub const REPLAY: u8 = 3;
}
pub const NO_PAYLOAD: u64 = u64::MAX;
/// The view descriptor slots (CameraVs, CameraPs, ViewVs, ViewPs, TextureVs, TexturePs).
pub const VIEW_DESCRIPTORS: usize = 6;

/// stage_replay::Command.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Command {
    pub op: u8,
    pub flags: u8,
    pub count: u16,
    pub a: u32,
    pub b: u64,
    pub c: u64,
    pub d: u64,
}
/// stage_replay::Settings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Settings {
    pub mode: u32,
    pub stages: u64,
    pub source: u32,
}
/// stage_replay::Touched.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Touched {
    pub pso: bool,
    pub root: bool,
    pub params: bool,
    pub topology: bool,
    pub index: bool,
    pub vertices: bool,
    pub viewports: bool,
    pub scissors: bool,
    pub blend: bool,
    pub stencil: bool,
    pub depth_bounds: bool,
    pub heaps: bool,
    pub rate: bool,
    pub rate_image: bool,
    pub view_mask: bool,
}
const _: () = assert!(
    core::mem::size_of::<Command>() == 32 && core::mem::size_of::<Settings>() == 24 && core::mem::size_of::<Touched>() == 15
);

pub fn replayable(op: u8) -> bool {
    (op::PIPELINE..=op::INDIRECT).contains(&op)
}
pub fn ignored(op: u8) -> bool {
    op == op::MARKER || op == op::COPY || op == op::COMPUTE_STATE
}
pub fn blocking(op: u8) -> bool {
    (op::DISPATCH..op::COUNT).contains(&op)
}
pub fn is_draw(op: u8) -> bool {
    op == op::DRAW || op == op::DRAW_INDEXED || op == op::INDIRECT
}
pub fn stage_selected(mask: u64, stage: u32) -> bool {
    stage < 64 && (mask >> stage) & 1 != 0
}
/// Decide: the consumer hook's action for one (side, stage) call.
pub fn decide(settings: &Settings, side: i32, node: u32, stage: u32) -> u8 {
    if node != 0x33DDA4 || !(0..=1).contains(&side) || !stage_selected(settings.stages, stage) {
        return action::FORWARD;
    }
    let target = side as u32 != settings.source;
    match settings.mode {
        1 => action::CAPTURE,
        2 => {
            if target { action::SKIP } else { action::FORWARD }
        }
        3 | 4 => {
            if target { action::REPLAY } else { action::CAPTURE }
        }
        _ => action::FORWARD,
    }
}
pub fn same_frame(capture_frame: u32, target_frame: u32, capture_complete: bool) -> bool {
    capture_complete && capture_frame == target_frame
}
/// SubstituteCamera: the source camera descriptors replaced in a table's sources.
pub fn substitute_camera(sources: &mut [u64], from_vs: u64, to_vs: u64, from_ps: u64, to_ps: u64) -> u32 {
    let mut replaced = 0;
    for source in sources {
        if from_vs != 0 && *source == from_vs && to_vs != 0 {
            *source = to_vs;
            replaced += 1;
        } else if from_ps != 0 && *source == from_ps && to_ps != 0 {
            *source = to_ps;
            replaced += 1;
        }
    }
    replaced
}
/// ViewDescriptors::Contains.
pub fn contains(descriptors: &[u64; VIEW_DESCRIPTORS], value: u64) -> bool {
    value != 0 && descriptors.contains(&value)
}
/// SubstituteView: each source view descriptor replaced with the target's; -1 when the target lacks one (the sources
/// replaced before that stay replaced, as in C++).
pub fn substitute_view(sources: &mut [u64], from: &[u64; VIEW_DESCRIPTORS], to: &[u64; VIEW_DESCRIPTORS]) -> i32 {
    let mut replaced = 0;
    for source in sources {
        if *source == 0 {
            continue;
        }
        if let Some(k) = from.iter().position(|&d| d == *source) {
            if to[k] == 0 {
                return -1;
            }
            *source = to[k];
            replaced += 1;
        }
    }
    replaced
}
/// Touched::Of.
pub fn touched(commands: &[Command]) -> Touched {
    let mut t = Touched::default();
    for command in commands {
        match command.op {
            op::PIPELINE => t.pso = true,
            op::ROOT_SIGNATURE => {
                t.root = true;
                t.params = true;
            }
            op::ROOT_TABLE | op::ROOT_CONSTANT | op::ROOT_CONSTANTS | op::ROOT_CBV | op::ROOT_SRV | op::ROOT_UAV => t.params = true,
            op::TOPOLOGY => t.topology = true,
            op::INDEX_BUFFER => t.index = true,
            op::VERTEX_BUFFERS => t.vertices = true,
            op::VIEWPORTS => t.viewports = true,
            op::SCISSORS => t.scissors = true,
            op::BLEND_FACTOR => t.blend = true,
            op::STENCIL_REF => t.stencil = true,
            op::DEPTH_BOUNDS => t.depth_bounds = true,
            op::HEAPS => t.heaps = true,
            op::SHADING_RATE => t.rate = true,
            op::SHADING_IMAGE => t.rate_image = true,
            op::VIEW_INSTANCE_MASK => t.view_mask = true,
            _ => {}
        }
    }
    t
}
/// Stream::Payload's bounds.
pub fn payload_in_bounds(offset: u64, bytes: usize, size: usize) -> bool {
    offset != NO_PAYLOAD && offset <= size as u64 && bytes as u64 <= size as u64 - offset
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// The classes by `which`: Replayable (0), Ignored (1), Blocking (2), IsDraw (3).
#[no_mangle]
pub extern "C-unwind" fn cvr_sr_class(which: u32, value: u8) -> bool {
    match which {
        0 => replayable(value),
        1 => ignored(value),
        2 => blocking(value),
        _ => is_draw(value),
    }
}
/// StageSelected (`which` 0: `a` mask, `b` stage), SameFrame (1: `a` capture frame, `b` target frame, `flag`), Stream
/// payload bounds (2: `a` offset, `b` bytes, `c` size).
#[no_mangle]
pub extern "C-unwind" fn cvr_sr_check(which: u32, a: u64, b: u64, c: u64, flag: bool) -> bool {
    match which {
        0 => stage_selected(a, b as u32),
        1 => same_frame(a as u32, b as u32, flag),
        _ => payload_in_bounds(a, b as usize, c as usize),
    }
}
/// Decide.
///
/// # Safety
/// `settings` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sr_decide(settings: *const Settings, side: i32, node: u32, stage: u32) -> u8 {
    decide(unsafe { &*settings }, side, node, stage)
}
/// SubstituteCamera.
///
/// # Safety
/// `sources` holds `count` writable values (or is null with none).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sr_substitute_camera(sources: *mut u64, count: u32, from_vs: u64, to_vs: u64, from_ps: u64, to_ps: u64) -> u32 {
    if sources.is_null() || count == 0 {
        return 0;
    }
    substitute_camera(unsafe { core::slice::from_raw_parts_mut(sources, count as usize) }, from_vs, to_vs, from_ps, to_ps)
}
/// ViewDescriptors::Contains.
///
/// # Safety
/// `descriptors` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sr_contains(descriptors: *const [u64; VIEW_DESCRIPTORS], value: u64) -> bool {
    contains(unsafe { &*descriptors }, value)
}
/// SubstituteView.
///
/// # Safety
/// `sources` holds `count` writable values (or is null with none); `from` and `to` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sr_substitute_view(
    sources: *mut u64,
    count: u32,
    from: *const [u64; VIEW_DESCRIPTORS],
    to: *const [u64; VIEW_DESCRIPTORS],
) -> i32 {
    if sources.is_null() || count == 0 {
        return 0;
    }
    unsafe { substitute_view(core::slice::from_raw_parts_mut(sources, count as usize), &*from, &*to) }
}
/// Touched::Of over a stream's commands.
///
/// # Safety
/// `commands` holds `count` commands (or is null with none); `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sr_touched(commands: *const Command, count: usize, out: *mut Touched) {
    let commands = if commands.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(commands, count) } };
    unsafe { *out = touched(commands) };
}
