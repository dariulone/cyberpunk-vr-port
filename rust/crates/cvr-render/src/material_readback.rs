//! THE MATERIAL READBACK PERMIT'S STATE (include/Render/MaterialReadbackState.hpp): one recorded material readback is
//! permitted for exactly one submission of its own recording -- the permit enabled only for a single valid occurrence
//! of a still-recorded state, frozen at the first submission, any replay or a foreign recording's reset rejecting it;
//! the submissions counted down; sealed only once every submission landed, the fences completed and nothing replayed
//! (the caller then writes the permanent zero before mapping the readback); a rejected one disarmed under the same
//! completion. The class's members stay in C++ (mirrored here) and its methods forward in the Rust build;
//! tools/single_pass_tests/material_readback_state_test runs its cases against them.

/// ReadbackState::Stage.
pub const RECORDED: u32 = 0;
pub const SUBMITTING: u32 = 1;
pub const SUBMITTED: u32 = 2;
pub const SEALED: u32 = 3;
pub const REJECTED: u32 = 4;
/// ReadbackState::PermitUpdate.
pub const PERMIT_NONE: u32 = 0;
pub const PERMIT_ENABLE: u32 = 1;
pub const PERMIT_DISABLE: u32 = 2;

/// ReadbackState's members.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ReadbackState {
    pub stage: u32,
    pub pending: u32,
    pub frozen: bool,
    pub submitted: bool,
    pub enabled: bool,
    pub sealed: bool,
    pub replayed: bool,
}
const _: () = assert!(core::mem::size_of::<ReadbackState>() == 16);

impl ReadbackState {
    /// BeforeSubmit.
    pub fn before_submit(
        &mut self,
        occurrences: u32,
        valid: bool,
        current_recording_owned: bool,
    ) -> u32 {
        if occurrences == 0 {
            return PERMIT_NONE;
        }
        if !current_recording_owned {
            // A reset after submission started belongs to a different recording.
            if self.frozen {
                return PERMIT_NONE;
            }
            self.frozen = true;
            self.stage = REJECTED;
            return PERMIT_DISABLE;
        }
        if self.pending != u32::MAX {
            self.pending += 1;
        } else {
            self.replayed = true;
        }
        if self.frozen {
            self.stage = REJECTED;
            self.replayed = true;
            return PERMIT_NONE;
        }
        self.frozen = true;
        if occurrences != 1 {
            self.replayed = true;
        }
        if self.stage != RECORDED || occurrences != 1 || !valid {
            self.stage = REJECTED;
            return PERMIT_DISABLE;
        }
        self.enabled = true;
        self.stage = SUBMITTING;
        PERMIT_ENABLE
    }
    pub fn submitted(&mut self) {
        if self.pending == 0 {
            return;
        }
        self.pending -= 1;
        self.submitted = true;
        if self.stage == SUBMITTING && self.pending == 0 {
            self.stage = SUBMITTED;
        }
    }
    /// Reset may follow Execute before its Submitted callback arrives.
    pub fn reset_before_submit(&mut self) {
        if !self.frozen {
            self.stage = REJECTED;
        }
    }
    pub fn invalidated(&mut self) {
        self.stage = REJECTED;
    }
    pub fn can_seal(&self, fences_complete: bool) -> bool {
        fences_complete
            && self.stage == SUBMITTED
            && self.submitted
            && self.pending == 0
            && self.enabled
            && !self.sealed
            && !self.replayed
    }
    /// True authorizes the caller's permanent zero write before mapping the readback.
    pub fn seal(&mut self, fences_complete: bool) -> bool {
        if !self.can_seal(fences_complete) {
            return false;
        }
        self.enabled = false;
        self.sealed = true;
        self.stage = SEALED;
        true
    }
    pub fn disarm_rejected(&mut self, fences_complete: bool) -> bool {
        if self.stage != REJECTED
            || !fences_complete
            || !self.submitted
            || self.pending != 0
            || self.replayed
            || !self.enabled
        {
            return false;
        }
        self.enabled = false;
        self.sealed = true;
        true
    }
    /// The original completion fences cannot certify a replay's completion.
    pub fn permit_may_be_in_flight(&self, fences_complete: bool) -> bool {
        self.enabled && (self.pending != 0 || self.replayed || (self.submitted && !fences_complete))
    }
}

/// ReadbackState's steps: BeforeSubmit (0: `a` occurrences, `b` valid, `c` owned; the permit update), Submitted (1),
/// ResetBeforeSubmit (2), Invalidated (3), CanSeal (4, `b` fences), Seal (5, `b`), DisarmRejected (6, `b`),
/// PermitMayBeInFlight (7, `b`).
///
/// # Safety
/// `state` points at a ReadbackState's members.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_mr_step(
    state: *mut ReadbackState,
    step: u32,
    a: u32,
    b: bool,
    c: bool,
) -> u32 {
    let s = unsafe { &mut *state };
    match step {
        0 => s.before_submit(a, b, c),
        1 => {
            s.submitted();
            0
        }
        2 => {
            s.reset_before_submit();
            0
        }
        3 => {
            s.invalidated();
            0
        }
        4 => s.can_seal(b) as u32,
        5 => s.seal(b) as u32,
        6 => s.disarm_rejected(b) as u32,
        _ => s.permit_may_be_in_flight(b) as u32,
    }
}
