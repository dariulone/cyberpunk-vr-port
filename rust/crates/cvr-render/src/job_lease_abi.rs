//! The C ABI of the engine job group lease (native::job_lease) for include/Render/NativeJobLease.hpp: in the Rust
//! build the header's NativeJobLease owns a lease here (the engine's retain / release / complete calls stay its API).
//! tools/single_pass_tests/cpu_collector_test runs its lease cases against it in its Rust variant.
use core::ffi::c_void;

use crate::native::job_lease::{JobLeaseApi, NativeJobLease};

/// A lease over the engine's group calls; free it with cvr_job_lease_free (which releases the held groups).
///
/// # Safety
/// `api` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_job_lease_new(api: *const JobLeaseApi) -> *mut NativeJobLease {
    Box::into_raw(Box::new(NativeJobLease::new(unsafe { *api })))
}

/// # Safety
/// `lease` comes from cvr_job_lease_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_job_lease_free(lease: *mut NativeJobLease) {
    if !lease.is_null() {
        drop(unsafe { Box::from_raw(lease) });
    }
}

/// NativeJobLease::Capture.
///
/// # Safety
/// `lease` is live; `begin` / `end` are live engine job groups (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_job_lease_capture(
    lease: *mut NativeJobLease,
    begin: *mut c_void,
    end: *mut c_void,
) -> bool {
    unsafe { (*lease).capture(begin, end) }
}

/// NativeJobLease::Complete.
///
/// # Safety
/// `lease` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_job_lease_complete(lease: *const NativeJobLease) -> bool {
    unsafe { (*lease).complete() }
}
