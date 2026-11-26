//! The C ABI of the graph definition owner's move proposal and its definitions' single retirement
//! (native::definition_owner) for include/Render/NativeDefinitionOwner.hpp: in the Rust build the header's MovePlan
//! keeps a capture's fields and hands them here to capture, revalidate and verify the detach; its RetireState owns a
//! Rust retirement state. tools/single_pass_tests/definition_owner_test and definition_source_test run their cases
//! against these in their Rust variants.
use crate::graph_snapshot_abi::Read;
use crate::native::definition_owner::{MovePlan, NativeVector, RetireState};

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}
fn reader(read: Read) -> impl FnMut(u64, &mut [u8]) -> bool {
    move |at, out| unsafe { read(at, out.as_mut_ptr().cast(), out.len()) }
}

/// A ready MovePlan's fields, borrowed (or what a capture yields, borrowed until cvr_do_plan_free).
#[repr(C)]
pub struct PlanFields {
    pub address: u64,
    pub trailer: u64,
    pub saved: NativeVector,
    pub empty: NativeVector,
    pub definitions: *const u64,
    pub count: usize,
    pub failure: u32,
    pub ready: bool,
}
const _: () =
    assert!(core::mem::size_of::<NativeVector>() == 16 && core::mem::size_of::<PlanFields>() == 72);

unsafe fn plan_of(fields: &PlanFields) -> Option<MovePlan> {
    unsafe {
        fields.ready.then(|| {
            MovePlan::restored(
                fields.address,
                fields.trailer,
                fields.saved,
                fields.empty,
                slice(fields.definitions, fields.count).to_vec(),
            )
        })
    }
}

/// MovePlan::Capture: the native plan (its fields through `out`; free it with cvr_do_plan_free) or null with the
/// failure in `out`.
///
/// # Safety
/// `out` is writable; `read` follows definition_owner::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_do_plan_capture(
    graph: u64,
    read: Option<Read>,
    out: *mut PlanFields,
) -> *mut MovePlan {
    unsafe {
        let mut adapter = read.map(reader);
        let mut plan = MovePlan::default();
        let captured = plan.capture(
            graph,
            adapter
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
        );
        *out = PlanFields {
            address: plan.owner_address(),
            trailer: plan.trailer_address(),
            saved: plan.saved_header(),
            empty: plan.empty_header(),
            definitions: core::ptr::null(),
            count: 0,
            failure: plan.rejection() as u32,
            ready: captured,
        };
        if !captured {
            return core::ptr::null_mut();
        }
        let plan = Box::into_raw(Box::new(plan));
        (*out).definitions = (*plan).definitions().as_ptr();
        (*out).count = (*plan).definitions().len();
        plan
    }
}

/// # Safety
/// `plan` comes from cvr_do_plan_capture (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_do_plan_free(plan: *mut MovePlan) {
    if !plan.is_null() {
        drop(unsafe { Box::from_raw(plan) });
    }
}

/// MovePlan::MatchesBefore (`detached` false) or VerifyDetached (true) of a C++ plan.
///
/// # Safety
/// The fields hold their counts; `read` follows definition_owner::ReadMemory.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_do_plan_check(
    fields: *const PlanFields,
    read: Option<Read>,
    detached: bool,
) -> bool {
    unsafe {
        let (Some(plan), Some(read)) = (plan_of(&*fields), read) else {
            return false;
        };
        let mut adapter = reader(read);
        if detached {
            plan.verify_detached(Some(&mut adapter))
        } else {
            plan.matches_before(&mut adapter)
        }
    }
}

/// A retirement state; free it with cvr_do_retire_free.
#[no_mangle]
pub extern "C-unwind" fn cvr_do_retire_new(
    epoch: u64,
    all_prior_consumers_accounted: bool,
) -> *mut RetireState {
    Box::into_raw(Box::new(RetireState::new(
        epoch,
        all_prior_consumers_accounted,
    )))
}

/// # Safety
/// `state` comes from cvr_do_retire_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_do_retire_free(state: *mut RetireState) {
    if !state.is_null() {
        drop(unsafe { Box::from_raw(state) });
    }
}

/// RetireState's steps: AddConsumer (0), CompleteConsumer (1), VerifyDetach (2, `value` = detached), CloseSourceUses
/// (3), UnclaimedNativeMutation (4), BeginRetire (5), FinishRetire (6), ReadyToRetire (7).
///
/// # Safety
/// `state` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_do_retire_step(
    state: *mut RetireState,
    step: u32,
    epoch: u64,
    value: u64,
) -> bool {
    unsafe {
        let state = &mut *state;
        match step {
            0 => state.add_consumer(epoch, value),
            1 => state.complete_consumer(epoch, value),
            2 => state.verify_detach(epoch, value != 0),
            3 => state.close_source_uses(epoch),
            4 => {
                state.unclaimed_native_mutation();
                false
            }
            5 => state.begin_retire(epoch),
            6 => state.finish_retire(epoch),
            _ => state.ready_to_retire(),
        }
    }
}

/// RetireState's accessors.
#[repr(C)]
pub struct RetireView {
    pub epoch: u64,
    pub consumers: usize,
    pub pending: usize,
    pub failure: u32,
    pub retiring: bool,
    pub retired: bool,
}
const _: () = assert!(core::mem::size_of::<RetireView>() == 32);

/// # Safety
/// `state` is live; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_do_retire_state(
    state: *const RetireState,
    out: *mut RetireView,
) {
    unsafe {
        let s = &*state;
        *out = RetireView {
            epoch: s.epoch(),
            consumers: s.consumers(),
            pending: s.pending_consumers(),
            failure: s.failure_reason() as u32,
            retiring: s.retiring(),
            retired: s.retired(),
        };
    }
}
