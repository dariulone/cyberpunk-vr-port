//! The target handoff's rules (`include/Render/NativeTargetHandoff.hpp`, `target_handoff::detail`): the observation
//! window; the legacy texture states this observer knows (reads combined, at most one write and only alone); a watched
//! target's per-subresource state ledger (seeded as render target or depth write, exact non-split transitions from the
//! known current state, aliasing refusal, the frozen states); and one source recording's ledger (capture, generation
//! match, exit, local and foreign touches, shader draws over watched UAV states, close, the single pending validation
//! on its queue, submission and reset before submission).
//!
//! In the Rust build (`CVR_RUST_VRIK`) `StateKnown` and the methods of `ObservationWindow`, `TargetState` and
//! `RecordingState` forward to `cvr_th_*`; their members stay in C++, mirrored here. `tools/single_pass_tests/
//! target_handoff_test` runs its cases against them in its Rust variant.

pub const STATES_PER_SOURCE: usize = 5;
const ALL_SUBRESOURCES: u32 = u32::MAX;
const E_PENDING: i32 = 0x8000_000Au32 as i32;

const DEPTH_READ: u32 = 0x20;
const NON_PIXEL_SHADER_RESOURCE: u32 = 0x40;
const PIXEL_SHADER_RESOURCE: u32 = 0x80;
const COPY_SOURCE: u32 = 0x800;
const RESOLVE_SOURCE: u32 = 0x2000;
const SHADING_RATE_SOURCE: u32 = 0x100_0000;
const RENDER_TARGET: u32 = 0x4;
const UNORDERED_ACCESS: u32 = 0x8;
const DEPTH_WRITE: u32 = 0x10;
const COPY_DEST: u32 = 0x400;
const RESOLVE_DEST: u32 = 0x1000;

/// target_handoff::Failure (the values these rules record).
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const SHAPE: u32 = 2;
    pub const STATE_BEFORE: u32 = 8;
    pub const SUBRESOURCE: u32 = 9;
    pub const SPLIT_BARRIER: u32 = 10;
    pub const UNKNOWN_BARRIER: u32 = 11;
    pub const ALIASING: u32 = 12;
    pub const POST_EXIT_WRITE: u32 = 13;
    pub const OTHER_LIST_TOUCH: u32 = 14;
    pub const AFTER_CLOSE: u32 = 15;
    pub const MISSING_EXIT: u32 = 16;
    pub const CLOSE_FAILED: u32 = 17;
    pub const RESET_BEFORE_SUBMIT: u32 = 18;
    pub const SUBMISSION: u32 = 19;
    pub const REPLAY: u32 = 20;
    pub const UNKNOWN_COMMAND: u32 = 21;
    pub const GENERATION: u32 = 22;
}
/// ExitKind::Other.
const EXIT_OTHER: u32 = 9;
/// SourceFlags.
pub mod source {
    pub const CAPTURED: u64 = 1;
    pub const EXITED: u64 = 2;
    pub const CLOSE_SUCCEEDED: u64 = 4;
    pub const PENDING_VALIDATED: u64 = 8;
    pub const SUBMITTED_OBSERVED: u64 = 16;
    pub const REFUSED: u64 = 32;
    pub const RESET_BEFORE_SUBMIT: u64 = 64;
    pub const PREFIX_UNKNOWN: u64 = 128;
}
/// StateFlags.
pub mod state {
    pub const CURRENT_KNOWN: u64 = 1;
    pub const FROZEN_KNOWN: u64 = 2;
    pub const STATE_REFUSED: u64 = 4;
}

/// detail::ObservationWindow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ObservationWindow {
    pub events: u64,
    pub open: bool,
    pub finished: bool,
}
impl ObservationWindow {
    pub fn begin(&mut self) -> bool {
        if self.finished {
            return false;
        }
        self.open = true;
        true
    }
    pub fn event(&mut self) -> bool {
        if !self.open {
            return false;
        }
        if self.events != u64::MAX {
            self.events += 1;
        }
        true
    }
    pub fn finish(&mut self) {
        self.open = false;
        self.finished = true;
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// detail::StateKnown: only legacy texture states; reads combine, a write stands alone.
pub fn state_known(value: u32) -> bool {
    const READS: u32 = DEPTH_READ | NON_PIXEL_SHADER_RESOURCE | PIXEL_SHADER_RESOURCE | COPY_SOURCE | RESOLVE_SOURCE | SHADING_RATE_SOURCE;
    const WRITES: u32 = RENDER_TARGET | UNORDERED_ACCESS | DEPTH_WRITE | COPY_DEST | RESOLVE_DEST;
    if value & !(READS | WRITES) != 0 {
        return false;
    }
    let write = value & WRITES;
    write == 0 || (write & (write - 1) == 0 && value == write)
}

/// detail::SubresourceState.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SubresourceState {
    pub seed: u32,
    pub current: u32,
    pub frozen: u32,
    pub transitions: u32,
    pub flags: u64,
}
/// detail::TargetState.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TargetState {
    pub states: [SubresourceState; STATES_PER_SOURCE],
    pub count: u32,
    pub failure: u32,
}
impl TargetState {
    fn live(&mut self) -> &mut [SubresourceState] {
        let count = (self.count as usize).min(STATES_PER_SOURCE);
        &mut self.states[..count]
    }
    pub fn reject(&mut self, why: u32) {
        if self.failure == failure::NONE {
            self.failure = why;
        }
        for state in self.live() {
            state.flags |= state::STATE_REFUSED;
        }
    }
    pub fn seed(&mut self, subresources: u32, value: u32) -> bool {
        *self = Self::default();
        if subresources == 0 || subresources as usize > STATES_PER_SOURCE || (value != RENDER_TARGET && value != DEPTH_WRITE) {
            self.reject(failure::SHAPE);
            return false;
        }
        self.count = subresources;
        for state in self.live() {
            *state = SubresourceState { seed: value, current: value, frozen: 0, transitions: 0, flags: state::CURRENT_KNOWN };
        }
        true
    }
    pub fn transition(&mut self, subresource: u32, before: u32, after: u32, flags: u32) -> bool {
        if self.failure != failure::NONE {
            return false;
        }
        if self.count == 0 {
            self.reject(failure::SUBRESOURCE);
            return false;
        }
        if flags != 0 {
            self.reject(failure::SPLIT_BARRIER);
            return false;
        }
        if !state_known(before) || !state_known(after) {
            self.reject(failure::UNKNOWN_BARRIER);
            return false;
        }
        if subresource != ALL_SUBRESOURCES && subresource >= self.count {
            self.reject(failure::SUBRESOURCE);
            return false;
        }
        let (first, end) = if subresource == ALL_SUBRESOURCES { (0, self.count) } else { (subresource, subresource + 1) };
        let range = first as usize..(end as usize).min(STATES_PER_SOURCE);
        if self.states[range.clone()].iter().any(|state| state.flags & state::CURRENT_KNOWN == 0 || state.current != before) {
            self.reject(failure::STATE_BEFORE);
            return false;
        }
        for state in &mut self.states[range] {
            state.current = after;
            if state.transitions != u32::MAX {
                state.transitions += 1;
            }
        }
        true
    }
    pub fn alias(&mut self, affects: bool) {
        if affects {
            self.reject(failure::ALIASING);
        }
    }
    pub fn freeze(&mut self) {
        if self.failure == failure::NONE {
            for state in self.live() {
                state.frozen = state.current;
                state.flags |= state::FROZEN_KNOWN;
            }
        }
    }
}

/// detail::RecordingState.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RecordingState {
    pub list: u64,
    pub generation: u64,
    pub queue: u64,
    pub flags: u64,
    pub failure: u32,
    pub exit_reason: u32,
    pub exit_kind: u32,
    pub close_result: i32,
    pub foreign_failure: u32,
    pub foreign_list: u64,
    pub foreign_kind: u64,
    pub foreign_events: u64,
    pub pending_observed: bool,
}
impl Default for RecordingState {
    /// The C++ default member initializers: PrefixUnknown, and E_PENDING as the close result.
    fn default() -> Self {
        RecordingState {
            list: 0,
            generation: 0,
            queue: 0,
            flags: source::PREFIX_UNKNOWN,
            failure: 0,
            exit_reason: 0,
            exit_kind: 0,
            close_result: E_PENDING,
            foreign_failure: 0,
            foreign_list: 0,
            foreign_kind: 0,
            foreign_events: 0,
            pending_observed: false,
        }
    }
}
const _: () = assert!(
    core::mem::size_of::<ObservationWindow>() == 16
        && core::mem::size_of::<SubresourceState>() == 24
        && core::mem::size_of::<TargetState>() == 128
        && core::mem::size_of::<RecordingState>() == 88
        && core::mem::offset_of!(RecordingState, foreign_list) == 56
);
impl RecordingState {
    pub fn reject(&mut self, why: u32) {
        self.flags |= source::REFUSED;
        if self.failure == failure::NONE {
            self.failure = why;
        }
    }
    pub fn foreign(&mut self, list: u64, kind: u32) {
        if self.flags & source::CAPTURED == 0 {
            return;
        }
        if self.foreign_events == 0 {
            self.foreign_failure = failure::OTHER_LIST_TOUCH;
            self.foreign_list = list;
            self.foreign_kind = kind as u64;
        }
        if self.foreign_events != u64::MAX {
            self.foreign_events += 1;
        }
    }
    pub fn aggregate_flags(&self) -> u64 {
        if self.foreign_events != 0 { (self.flags | source::REFUSED) & !source::PENDING_VALIDATED } else { self.flags }
    }
    pub fn aggregate_failure(&self) -> u32 {
        if self.failure != failure::NONE { self.failure } else { self.foreign_failure }
    }
    pub fn begin(&mut self, list: u64, generation: u64) -> bool {
        *self = Self::default();
        self.list = list;
        self.generation = generation;
        if list == 0 || generation == 0 {
            self.reject(failure::ARGUMENTS);
            return false;
        }
        self.flags |= source::CAPTURED;
        true
    }
    pub fn matches(&mut self, list: u64, generation: u64) -> bool {
        if list != self.list || generation != self.generation {
            self.reject(failure::GENERATION);
            return false;
        }
        true
    }
    pub fn tracking(&self) -> bool {
        self.flags & source::CAPTURED != 0 && self.flags & (source::SUBMITTED_OBSERVED | source::RESET_BEFORE_SUBMIT) == 0
    }
    pub fn exit(&mut self, reason: u32, kind: u32) {
        if !self.tracking() || self.flags & source::EXITED != 0 {
            return;
        }
        if self.flags & source::CLOSE_SUCCEEDED != 0 {
            self.reject(failure::AFTER_CLOSE);
            return;
        }
        self.flags |= source::EXITED;
        self.exit_reason = reason;
        self.exit_kind = kind;
    }
    pub fn touch(&mut self, list: u64, write: bool, alias: bool, kind: u32) {
        if self.flags & source::CAPTURED == 0 {
            return;
        }
        if list != self.list {
            self.foreign(list, kind);
            return;
        }
        if !self.tracking() {
            return;
        }
        if self.flags & source::CLOSE_SUCCEEDED != 0 {
            self.reject(failure::AFTER_CLOSE);
            return;
        }
        if alias {
            self.reject(failure::ALIASING);
            return;
        }
        if write && self.flags & source::EXITED != 0 {
            self.reject(failure::POST_EXIT_WRITE);
        }
    }
    pub fn shader_draw(&mut self, list: u64, target: &TargetState, retired: bool) {
        if self.flags & source::CAPTURED == 0 {
            return;
        }
        let foreign = retired || list != self.list;
        if !foreign && !self.tracking() {
            return;
        }
        // OM bindings do not describe UAV writes by graphics shaders.
        let count = (target.count as usize).min(STATES_PER_SOURCE);
        let uav = target.states[..count].iter().any(|state| {
            (state.flags & state::CURRENT_KNOWN != 0 && state.current == UNORDERED_ACCESS)
                || (state.flags & state::FROZEN_KNOWN != 0 && state.frozen == UNORDERED_ACCESS)
        });
        if uav {
            if foreign {
                self.foreign(list, EXIT_OTHER);
            } else {
                self.reject(failure::UNKNOWN_COMMAND);
            }
        }
    }
    pub fn close(&mut self, result: i32) -> bool {
        if !self.tracking() {
            return false;
        }
        self.close_result = result;
        if result < 0 {
            self.reject(failure::CLOSE_FAILED);
            return false;
        }
        if self.flags & source::CLOSE_SUCCEEDED != 0 {
            self.reject(failure::REPLAY);
            return false;
        }
        self.flags |= source::CLOSE_SUCCEEDED;
        if self.flags & source::EXITED == 0 {
            self.reject(failure::MISSING_EXIT);
        }
        self.flags & source::REFUSED == 0
    }
    pub fn pending(&mut self, queue: u64, occurrences: u32, exact_lease: bool) -> bool {
        if self.pending_observed || self.flags & source::SUBMITTED_OBSERVED != 0 {
            self.reject(failure::REPLAY);
            return false;
        }
        self.pending_observed = true;
        self.queue = queue;
        if queue == 0 || occurrences != 1 || !exact_lease || self.flags & source::CLOSE_SUCCEEDED == 0 || self.flags & source::EXITED == 0 {
            self.reject(failure::SUBMISSION);
        }
        if self.flags & source::REFUSED != 0 {
            return false;
        }
        self.flags |= source::PENDING_VALIDATED;
        true
    }
    pub fn submitted(&mut self, queue: u64) {
        if queue != 0 && queue == self.queue && self.pending_observed && self.flags & source::RESET_BEFORE_SUBMIT == 0 {
            self.flags |= source::SUBMITTED_OBSERVED;
        }
    }
    pub fn reset(&mut self) {
        if self.tracking() {
            self.flags |= source::RESET_BEFORE_SUBMIT;
            self.reject(failure::RESET_BEFORE_SUBMIT);
        }
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// StateKnown.
#[no_mangle]
pub extern "C-unwind" fn cvr_th_state_known(value: u32) -> bool {
    state_known(value)
}
/// ObservationWindow: Begin (`which` 0), Event (1), Finish (2), Clear (3).
///
/// # Safety
/// `window` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_window(window: *mut ObservationWindow, which: u32) -> bool {
    let window = unsafe { &mut *window };
    match which {
        0 => window.begin(),
        1 => window.event(),
        2 => {
            window.finish();
            true
        }
        _ => {
            window.clear();
            true
        }
    }
}
/// TargetState: Reject (`which` 0: `a` why), Seed (1: `a` subresources, `b` state), Transition (2: `a` subresource,
/// `b` before, `c` after, `flags`), Alias (3: `a` affects), Freeze (4).
///
/// # Safety
/// `target` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_th_target(target: *mut TargetState, which: u32, a: u32, b: u32, c: u32, flags: u32) -> bool {
    let target = unsafe { &mut *target };
    match which {
        0 => {
            target.reject(a);
            true
        }
        1 => target.seed(a, b),
        2 => target.transition(a, b, c, flags),
        3 => {
            target.alias(a != 0);
            true
        }
        _ => {
            target.freeze();
            true
        }
    }
}
/// RecordingState: Reject (`which` 0: `c` why), Foreign (1: `a` list, `c` kind), AggregateFlags (2, the value),
/// AggregateFailure (3, the value), Begin (4: `a` list, `b` generation), Match (5: `a`, `b`), Tracking (6), Exit (7:
/// `c` reason, `d` kind), Touch (8: `a` list, `c` write, `d` alias, `e` kind), ShaderDraw (9: `a` list, `target`, `c`
/// retired), Close (10: `result`), Pending (11: `a` queue, `c` occurrences, `d` exact lease), Submitted (12: `a` queue),
/// Reset (13). A boolean result is 0 or 1.
///
/// # Safety
/// `recording` is valid; `target` is valid for ShaderDraw.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_th_recording(
    recording: *mut RecordingState,
    which: u32,
    a: u64,
    b: u64,
    c: u32,
    d: u32,
    e: u32,
    target: *const TargetState,
    result: i32,
) -> u64 {
    match which {
        2 => return unsafe { (*recording).aggregate_flags() },
        3 => return unsafe { (*recording).aggregate_failure() } as u64,
        6 => return unsafe { (*recording).tracking() } as u64,
        _ => {}
    }
    let recording = unsafe { &mut *recording };
    match which {
        0 => recording.reject(c),
        1 => recording.foreign(a, c),
        4 => return recording.begin(a, b) as u64,
        5 => return recording.matches(a, b) as u64,
        7 => recording.exit(c, d),
        8 => recording.touch(a, c != 0, d != 0, e),
        9 => recording.shader_draw(a, unsafe { &*target }, c != 0),
        10 => return recording.close(result) as u64,
        11 => return recording.pending(a, c, d != 0) as u64,
        12 => recording.submitted(a),
        _ => recording.reset(),
    }
    1
}
