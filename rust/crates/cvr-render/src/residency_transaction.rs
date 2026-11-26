//! The GPU residency lease's transaction (`include/Render/GpuResidencyLease.hpp`, `gpu::residency::detail::Transaction`):
//! one atomic MakeResident of a nonzero object count (a failure acquires nothing, an exception quarantines), the
//! one-time submission mark, the fence association (a failed Signal, a zero value or the removed-device sentinel
//! quarantine), cancellation only before any possible submission, retirement only at or past the associated fence
//! (pending is S_FALSE, UINT64_MAX is device removal, never completion), and the release that evicts every acquired
//! object exactly once (any failure, or exception, quarantines after attempting all).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the transaction's methods forward to `cvr_rt_*`; its members stay in C++,
//! mirrored here, and the SDK callbacks run in C++, their exceptions caught there and reported as a flag.
//! `tools/single_pass_tests/gpu_residency_lease_test` runs its cases against these in its Rust variant.

use core::ffi::c_void;

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
const DXGI_ERROR_DEVICE_REMOVED: i32 = 0x887A_0005u32 as i32;

/// residency::State.
pub mod state {
    pub const EMPTY: u32 = 0;
    pub const ACQUIRED: u32 = 1;
    pub const SUBMITTED: u32 = 2;
    pub const FENCED: u32 = 3;
    pub const RELEASING: u32 = 4;
    pub const RETIRED: u32 = 5;
    pub const QUARANTINED: u32 = 6;
}

/// detail::Transaction's members.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Transaction {
    pub state: u32,
    pub count: usize,
    pub attempted: usize,
    pub signal: u64,
}
const _: () = assert!(core::mem::size_of::<Transaction>() == 32);

/// An SDK call's outcome: its HRESULT, or an exception.
pub enum Call {
    Returned(i32),
    Threw,
}

impl Transaction {
    pub fn acquire(&mut self, count: usize, make: impl FnOnce() -> Call) -> i32 {
        if self.state != state::EMPTY || count == 0 {
            return E_INVALIDARG;
        }
        self.count = count;
        match make() {
            Call::Threw => {
                self.state = state::QUARANTINED;
                E_FAIL
            }
            Call::Returned(hr) => {
                if hr >= 0 {
                    self.state = state::ACQUIRED;
                } else {
                    self.count = 0;
                }
                hr
            }
        }
    }
    pub fn mark_submitted(&mut self) -> bool {
        if self.state != state::ACQUIRED {
            return false;
        }
        self.state = state::SUBMITTED;
        true
    }
    pub fn associate_fence(&mut self, value: u64, signal_result: i32) -> i32 {
        if self.state != state::SUBMITTED {
            return E_UNEXPECTED;
        }
        if signal_result < 0 || value == 0 || value == u64::MAX {
            self.state = state::QUARANTINED;
            return if signal_result < 0 { signal_result } else { E_INVALIDARG };
        }
        self.signal = value;
        self.state = state::FENCED;
        S_OK
    }
    pub fn cancel(&mut self, evict: impl FnMut(usize) -> Call) -> i32 {
        if self.state != state::ACQUIRED {
            return E_ACCESSDENIED;
        }
        self.release(evict)
    }
    pub fn retire(&mut self, completed: u64, evict: impl FnMut(usize) -> Call) -> i32 {
        if self.state == state::SUBMITTED {
            self.state = state::QUARANTINED;
            return E_UNEXPECTED;
        }
        if self.state != state::FENCED {
            return E_ACCESSDENIED;
        }
        if completed == u64::MAX {
            self.state = state::QUARANTINED;
            return DXGI_ERROR_DEVICE_REMOVED;
        }
        if completed < self.signal {
            return S_FALSE;
        }
        self.release(evict)
    }
    pub fn quarantine(&mut self) {
        self.state = state::QUARANTINED;
    }
    fn release(&mut self, mut evict: impl FnMut(usize) -> Call) -> i32 {
        self.state = state::RELEASING;
        let mut first = S_OK;
        while self.attempted < self.count {
            let index = self.attempted;
            self.attempted += 1;
            let hr = match evict(index) {
                Call::Returned(hr) => hr,
                Call::Threw => E_FAIL,
            };
            if hr < 0 && first >= 0 {
                first = hr;
            }
        }
        self.state = if first < 0 { state::QUARANTINED } else { state::RETIRED };
        first
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// The C++ trampoline over an SDK callback: (context, object index, threw out) -> its HRESULT.
pub type Callback = unsafe extern "C-unwind" fn(*mut c_void, usize, *mut bool) -> i32;
fn call(callback: Callback, context: *mut c_void, index: usize) -> Call {
    let mut threw = false;
    let hr = unsafe { callback(context, index, &mut threw) };
    if threw { Call::Threw } else { Call::Returned(hr) }
}
/// Acquire.
///
/// # Safety
/// `transaction` is valid; `make` is callable with `context`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rt_acquire(transaction: *mut Transaction, count: usize, make: Callback, context: *mut c_void) -> i32 {
    unsafe { (*transaction).acquire(count, || call(make, context, 0)) }
}
/// MarkSubmitted (`which` 0: 1 or 0), AssociateFence (1: `value`, `signal_result`), Quarantine (2).
///
/// # Safety
/// `transaction` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rt_op(transaction: *mut Transaction, which: u32, value: u64, signal_result: i32) -> i32 {
    let transaction = unsafe { &mut *transaction };
    match which {
        0 => transaction.mark_submitted() as i32,
        1 => transaction.associate_fence(value, signal_result),
        _ => {
            transaction.quarantine();
            S_OK
        }
    }
}
/// Cancel (`which` 0) or Retire (1: `completed`).
///
/// # Safety
/// `transaction` is valid; `evict` is callable with `context`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rt_release(transaction: *mut Transaction, which: u32, completed: u64, evict: Callback, context: *mut c_void) -> i32 {
    let transaction = unsafe { &mut *transaction };
    let evict = |index: usize| call(evict, context, index);
    if which == 0 { transaction.cancel(evict) } else { transaction.retire(completed, evict) }
}
