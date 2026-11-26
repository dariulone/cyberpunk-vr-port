//! A scheduler callback's terminal job group, held to the callback's completion (include/Render/
//! NativeGraphTaskCompletion.hpp).
//!
//! Only the audited AC4A04 scheduler callback may supply this worker context; the caller proves the payload/graph and
//! the native body fingerprints separately. Both guarded scheduler bodies (returns 0x26E110 and 0x2735E3) keep a local
//! handle in the worker (+0x68 / +0x150, published at worker+0x20) and decrement the original queued completion token
//! only after the callback returns: the handle's group is read twice (a live reference, a pending callback token,
//! never the root prerequisite), retained, and read twice more. Completion only after the original callback's normal
//! return -- cancellation, a timeout or a zero count observed earlier is not completion. The native reference
//! callbacks must not unwind; the caller serializes all methods.
#![allow(unsafe_op_in_unsafe_fn)]
pub use super::consumer_identity::{
    AGGREGATE_EXECUTION_CALLBACK_RETURN, EXECUTION_CALLBACK_RETURN,
};
use super::graph_jobs::Read;
use super::job_lease::JobLeaseApi;
use core::ffi::c_void;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskCompletionFailure {
    #[default]
    None,
    Arguments,
    Caller,
    AlreadyCaptured,
    MissingApi,
    Read,
    HandleAddress,
    Group,
    PrerequisiteAlias,
    References,
    NoCallbackToken,
    Changed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Identity {
    handle: u64,
    group: u64,
}

fn pointer(value: u64, bytes: u64) -> bool {
    value != 0 && value & 7 == 0 && value <= u64::MAX - bytes
}

/// A group pin taken during a capture, released unless handed over.
struct Pin {
    release: unsafe extern "C" fn(*mut *mut c_void),
    group: *mut c_void,
}
impl Drop for Pin {
    fn drop(&mut self) {
        if !self.group.is_null() {
            // SAFETY: the group this capture retained.
            unsafe { (self.release)(&mut self.group) };
        }
    }
}

#[derive(Default)]
pub struct TaskCompletionLease {
    api: Option<JobLeaseApi>,
    group: usize,
    handle_address: u64,
    captured: bool,
    original_returned: bool,
    failure: TaskCompletionFailure,
}
impl Drop for TaskCompletionLease {
    fn drop(&mut self) {
        if let (true, Some(release)) = (self.group != 0, self.api.and_then(|a| a.release)) {
            let mut group = self.group as *mut c_void;
            // SAFETY: the group this lease retained.
            unsafe { release(&mut group) };
            self.group = 0;
        }
    }
}
impl TaskCompletionLease {
    fn read_identity(
        &mut self,
        worker: u64,
        handle_offset: u64,
        prerequisite: u64,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        before_retain: bool,
    ) -> Option<Identity> {
        let (mut b8, mut refs, mut pending) = ([0u8; 8], [0u8; 4], [0u8; 4]);
        self.failure = TaskCompletionFailure::Read;
        if !read(worker + 0x20, &mut b8) {
            return None;
        }
        let handle = u64::from_le_bytes(b8);
        self.failure = TaskCompletionFailure::HandleAddress;
        if handle != worker + handle_offset {
            return None;
        }
        self.failure = TaskCompletionFailure::Read;
        if !read(handle, &mut b8) {
            return None;
        }
        let group = u64::from_le_bytes(b8);
        self.failure = TaskCompletionFailure::Group;
        if !pointer(group, 0x20) {
            return None;
        }
        self.failure = TaskCompletionFailure::PrerequisiteAlias;
        if group == prerequisite {
            return None;
        }
        self.failure = TaskCompletionFailure::Read;
        if !read(group + 0x1c, &mut refs) || !read(group + 0x18, &mut pending) {
            return None;
        }
        let (refs, pending) = (i32::from_le_bytes(refs), i32::from_le_bytes(pending));
        self.failure = TaskCompletionFailure::References;
        if refs <= 0 || (before_retain && refs == i32::MAX) {
            return None;
        }
        self.failure = TaskCompletionFailure::NoCallbackToken;
        if pending <= 0 {
            return None;
        }
        Some(Identity { handle, group })
    }

    /// The worker's terminal group held before the scheduler callback runs.
    ///
    /// # Safety
    /// The worker's group is a live engine job group; the callbacks do not unwind.
    pub unsafe fn capture_before_callback(
        &mut self,
        worker: u64,
        caller: u64,
        image_base: u64,
        root_prerequisite: u64,
        read: Option<Read<'_>>,
        api: JobLeaseApi,
    ) -> bool {
        if self.captured {
            self.failure = TaskCompletionFailure::AlreadyCaptured;
            return false;
        }
        self.failure = TaskCompletionFailure::Arguments;
        let Some(read) = read else { return false };
        if !pointer(root_prerequisite, 0x20)
            || image_base == 0
            || image_base > u64::MAX - EXECUTION_CALLBACK_RETURN
        {
            return false;
        }
        self.failure = TaskCompletionFailure::Caller;
        let handle_offset = if caller == image_base + EXECUTION_CALLBACK_RETURN {
            0x68
        } else if caller == image_base + AGGREGATE_EXECUTION_CALLBACK_RETURN {
            0x150
        } else {
            return false;
        };
        self.failure = TaskCompletionFailure::Arguments;
        if !pointer(worker, handle_offset + 8) {
            return false;
        }
        self.failure = TaskCompletionFailure::MissingApi;
        let (Some(retain), Some(release), Some(_)) = (api.retain, api.release, api.complete) else {
            return false;
        };
        let Some(first) = self.read_identity(worker, handle_offset, root_prerequisite, read, true)
        else {
            return false;
        };
        let Some(second) = self.read_identity(worker, handle_offset, root_prerequisite, read, true)
        else {
            return false;
        };
        self.failure = TaskCompletionFailure::Changed;
        if first != second {
            return false;
        }
        let mut pin = Pin {
            release,
            group: first.group as usize as *mut c_void,
        };
        retain(pin.group);
        let Some(second) =
            self.read_identity(worker, handle_offset, root_prerequisite, read, false)
        else {
            return false;
        };
        let Some(third) = self.read_identity(worker, handle_offset, root_prerequisite, read, false)
        else {
            return false;
        };
        self.failure = TaskCompletionFailure::Changed;
        if first != second || first != third {
            return false;
        }
        self.api = Some(api);
        self.group = pin.group as usize;
        pin.group = core::ptr::null_mut();
        self.handle_address = first.handle;
        self.captured = true;
        self.failure = TaskCompletionFailure::None;
        true
    }
    /// Only after the original callback returned normally.
    pub fn mark_original_returned(&mut self) -> bool {
        if !self.captured || self.original_returned {
            return false;
        }
        self.original_returned = true;
        true
    }
    pub fn complete(&self) -> bool {
        let Some(complete) = self.api.and_then(|a| a.complete) else {
            return false;
        };
        // SAFETY: the group this lease holds.
        self.captured
            && self.original_returned
            && self.group != 0
            && unsafe { complete(self.group as *mut c_void) }
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    pub fn original_returned(&self) -> bool {
        self.original_returned
    }
    pub fn group(&self) -> u64 {
        self.group as u64
    }
    pub fn handle_address(&self) -> u64 {
        self.handle_address
    }
    pub fn rejection(&self) -> TaskCompletionFailure {
        self.failure
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/graph_task_completion_test.cpp's cases, for both scheduler layouts (a thrown read is a
    //! failed read here).
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const BASE: u64 = 0x1_4000_0000;
    const WORKER: u64 = 0x10000;
    const GROUP: u64 = 0x20000;
    const PREREQUISITE: u64 = 0x30000;
    const LAYOUTS: [(u64, u64); 2] = [
        (EXECUTION_CALLBACK_RETURN, 0x68),
        (AGGREGATE_EXECUTION_CALLBACK_RETURN, 0x150),
    ];

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        handle: u64,
        callback_return: u64,
        reads: u64,
        fail_at: u64,
        change_at: u64,
        change_address: u64,
        change_value: u64,
        retained: u32,
        released: u32,
        completion_reads: u32,
        references: i32,
        initial_references: i32,
        pending: i32,
        change_on_retain: bool,
        forbid_reads: bool,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        W.with(|w| f(&mut w.borrow_mut()))
    }
    fn put_in(w: &mut World, address: u64, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            w.memory.insert(address + i as u64, *v);
        }
    }
    fn put64(address: u64, v: u64) {
        with(|w| put_in(w, address, &v.to_le_bytes()));
    }
    fn put32(address: u64, v: i32) {
        with(|w| put_in(w, address, &v.to_le_bytes()));
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        with(|w| {
            assert!(
                !w.forbid_reads,
                "the completion reread an expired worker context"
            );
            w.reads += 1;
            if w.reads == w.fail_at {
                return false;
            }
            if w.reads == w.change_at {
                let (a, v) = (w.change_address, w.change_value);
                put_in(w, a, &v.to_le_bytes());
            }
            for (i, o) in out.iter_mut().enumerate() {
                match w.memory.get(&(address + i as u64)) {
                    Some(v) => *o = *v,
                    None => return false,
                }
            }
            true
        })
    }
    unsafe extern "C" fn retain(value: *mut c_void) {
        with(|w| {
            assert!(
                value as u64 == GROUP && w.references > 0 && w.references < i32::MAX,
                "invalid terminal retain"
            );
            w.references += 1;
            w.retained += 1;
            let r = w.references;
            put_in(w, GROUP + 0x1c, &r.to_le_bytes());
            if w.change_on_retain {
                let h = w.handle;
                put_in(w, h, &(GROUP + 0x100).to_le_bytes());
            }
        });
    }
    unsafe extern "C" fn release(value: *mut *mut c_void) {
        assert!(!value.is_null());
        with(|w| {
            assert!(
                *value as u64 == GROUP && w.references > w.initial_references,
                "invalid terminal release"
            );
            w.references -= 1;
            w.released += 1;
            let r = w.references;
            put_in(w, GROUP + 0x1c, &r.to_le_bytes());
        });
        *value = core::ptr::null_mut();
    }
    unsafe extern "C" fn complete(value: *mut c_void) -> bool {
        with(|w| {
            assert_eq!(value as u64, GROUP, "the wrong terminal group polled");
            w.completion_reads += 1;
            w.pending == 0
        })
    }
    const API: JobLeaseApi = JobLeaseApi {
        retain: Some(retain),
        release: Some(release),
        complete: Some(complete),
    };

    fn reset(layout: (u64, u64)) {
        with(|w| {
            *w = World {
                handle: WORKER + layout.1,
                callback_return: layout.0,
                references: 1,
                initial_references: 1,
                pending: 1,
                ..World::default()
            }
        });
        let handle = with(|w| w.handle);
        put64(WORKER + 0x20, handle);
        put64(handle, GROUP);
        put32(GROUP + 0x1c, 1);
        put32(GROUP + 0x18, 1);
        put32(GROUP + 0x100 + 0x1c, 1);
        put32(GROUP + 0x100 + 0x18, 1);
    }
    fn callback_return() -> u64 {
        with(|w| w.callback_return)
    }
    fn handle() -> u64 {
        with(|w| w.handle)
    }
    fn capture_full(
        lease: &mut TaskCompletionLease,
        worker: u64,
        caller: u64,
        base: u64,
        prerequisite: u64,
        reader: bool,
        api: JobLeaseApi,
    ) -> bool {
        let reader: Option<Read<'_>> = if reader { Some(&mut read) } else { None };
        unsafe { lease.capture_before_callback(worker, caller, base, prerequisite, reader, api) }
    }
    fn capture(lease: &mut TaskCompletionLease) -> bool {
        capture_full(
            lease,
            WORKER,
            BASE + callback_return(),
            BASE,
            PREREQUISITE,
            true,
            API,
        )
    }
    fn set(f: impl FnOnce(&mut World)) {
        with(f);
    }
    fn balanced() {
        with(|w| {
            assert!(
                w.retained == w.released && w.references == w.initial_references,
                "terminal retain/release imbalance"
            )
        });
    }

    #[test]
    fn lifecycle() {
        for layout in LAYOUTS {
            reset(layout);
            {
                let mut lease = TaskCompletionLease::default();
                assert!(!lease.captured() && !lease.original_returned() && !lease.complete());
                assert!(!lease.mark_original_returned());
                assert!(
                    capture(&mut lease)
                        && lease.group() == GROUP
                        && lease.handle_address() == handle()
                );
                assert_eq!(with(|w| w.references), 2);
                assert!(!lease.complete() && with(|w| w.completion_reads) == 0);
                set(|w| w.pending = 0);
                assert!(
                    !lease.complete() && with(|w| w.completion_reads) == 0,
                    "an early zero certified completion"
                );
                set(|w| w.pending = 3);
                assert!(lease.mark_original_returned() && !lease.mark_original_returned());
                set(|w| {
                    w.memory.clear();
                    w.forbid_reads = true;
                });
                assert!(!lease.complete() && with(|w| w.completion_reads) == 1);
                set(|w| w.pending = 0);
                assert!(lease.complete() && with(|w| w.completion_reads) == 2);
                assert!(
                    !capture(&mut lease)
                        && lease.rejection() == TaskCompletionFailure::AlreadyCaptured
                );
                assert!(lease.complete());
            }
            balanced();
            reset(layout);
            {
                let mut cancelled = TaskCompletionLease::default();
                assert!(capture(&mut cancelled));
                set(|w| w.pending = 0);
                assert!(!cancelled.complete() && !cancelled.original_returned());
            }
            balanced();
            // A terminal group can conservatively aggregate more than one native consumer.
            reset(layout);
            {
                let (mut first, mut second) = (
                    TaskCompletionLease::default(),
                    TaskCompletionLease::default(),
                );
                assert!(capture(&mut first) && capture(&mut second) && with(|w| w.references) == 3);
                assert!(first.mark_original_returned() && second.mark_original_returned());
                set(|w| w.pending = 2);
                assert!(!first.complete() && !second.complete());
                set(|w| w.pending = 0);
                assert!(first.complete() && second.complete());
            }
            balanced();
        }
    }
    #[test]
    fn invalid_inputs() {
        for layout in LAYOUTS {
            reset(layout);
            let ok = BASE + layout.0;
            let cases: [(u64, u64, u64, u64, bool); 10] = [
                (0, ok, BASE, PREREQUISITE, true),
                (WORKER + 1, ok, BASE, PREREQUISITE, true),
                (u64::MAX - 7, ok, BASE, PREREQUISITE, true),
                (WORKER, ok + 1, BASE, PREREQUISITE, true),
                (WORKER, ok - 1, BASE, PREREQUISITE, true),
                (WORKER, ok, 0, PREREQUISITE, true),
                (WORKER, ok, u64::MAX, PREREQUISITE, true),
                (WORKER, ok, BASE, 0, true),
                (WORKER, ok, BASE, PREREQUISITE + 1, true),
                (WORKER, ok, BASE, PREREQUISITE, false),
            ];
            for (i, (worker, caller, base, prerequisite, reader)) in cases.into_iter().enumerate() {
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture_full(&mut lease, worker, caller, base, prerequisite, reader, API)
                        && !lease.captured(),
                    "case {i}"
                );
                drop(lease);
                balanced();
            }
            for missing in 0..3 {
                let mut api = API;
                match missing {
                    0 => api.retain = None,
                    1 => api.release = None,
                    _ => api.complete = None,
                }
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture_full(&mut lease, WORKER, ok, BASE, PREREQUISITE, true, api)
                        && !lease.captured()
                );
                drop(lease);
                balanced();
            }
            reset(layout);
            {
                let other = if layout.0 == EXECUTION_CALLBACK_RETURN {
                    AGGREGATE_EXECUTION_CALLBACK_RETURN
                } else {
                    EXECUTION_CALLBACK_RETURN
                };
                let mut lease = TaskCompletionLease::default();
                assert!(!capture_full(
                    &mut lease,
                    WORKER,
                    BASE + other,
                    BASE,
                    PREREQUISITE,
                    true,
                    API
                ));
                assert_eq!(
                    lease.rejection(),
                    TaskCompletionFailure::HandleAddress,
                    "a known caller accepted the other layout"
                );
            }
            balanced();
            reset(layout);
            {
                let mut lease = TaskCompletionLease::default();
                assert!(!capture_full(
                    &mut lease,
                    u64::MAX - layout.1 - 7,
                    ok,
                    BASE,
                    PREREQUISITE,
                    true,
                    API
                ));
                assert!(
                    lease.rejection() == TaskCompletionFailure::Arguments && with(|w| w.reads) == 0
                );
            }
            balanced();
            for value in [0, GROUP, handle() + 8, u64::MAX - 7] {
                reset(layout);
                put64(WORKER + 0x20, value);
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease)
                        && lease.rejection() == TaskCompletionFailure::HandleAddress
                );
                drop(lease);
                balanced();
            }
            for group in [0, GROUP + 1, PREREQUISITE, u64::MAX - 7] {
                reset(layout);
                put64(handle(), group);
                let mut lease = TaskCompletionLease::default();
                assert!(!capture(&mut lease) && !lease.captured());
                drop(lease);
                balanced();
            }
            for count in [0, -1, i32::MAX] {
                reset(layout);
                put32(GROUP + 0x1c, count);
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease) && lease.rejection() == TaskCompletionFailure::References
                );
                drop(lease);
                balanced();
            }
            for count in [0, -1] {
                reset(layout);
                put32(GROUP + 0x18, count);
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease)
                        && lease.rejection() == TaskCompletionFailure::NoCallbackToken
                );
                drop(lease);
                balanced();
            }
            reset(layout);
            set(|w| {
                w.references = i32::MAX - 1;
                w.initial_references = i32::MAX - 1;
            });
            put32(GROUP + 0x1c, i32::MAX - 1);
            {
                let mut lease = TaskCompletionLease::default();
                assert!(capture(&mut lease) && with(|w| w.references) == i32::MAX);
            }
            balanced();
        }
    }
    #[test]
    fn failed_reads_and_changes() {
        for layout in LAYOUTS {
            reset(layout);
            let total = {
                let mut lease = TaskCompletionLease::default();
                assert!(capture(&mut lease));
                with(|w| w.reads)
            };
            balanced();
            for at in 1..=total {
                reset(layout);
                set(|w| w.fail_at = at);
                let mut lease = TaskCompletionLease::default();
                assert!(!capture(&mut lease) && !lease.captured() && !lease.complete());
                drop(lease);
                balanced();
            }
            reset(layout);
            set(|w| w.change_on_retain = true);
            {
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease) && lease.rejection() == TaskCompletionFailure::Changed
                );
            }
            balanced();
            assert!(with(|w| w.retained == 1 && w.released == 1));
            for at in [5, 13] {
                reset(layout);
                let h = handle();
                set(|w| {
                    w.change_at = at;
                    w.change_address = h;
                    w.change_value = GROUP + 0x100;
                });
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease) && lease.rejection() == TaskCompletionFailure::Changed
                );
                drop(lease);
                balanced();
            }
            reset(layout);
            let h = handle();
            set(|w| {
                w.change_at = 13;
                w.change_address = WORKER + 0x20;
                w.change_value = h + 8;
            });
            {
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease)
                        && lease.rejection() == TaskCompletionFailure::HandleAddress
                );
            }
            balanced();
            // The callback token must remain present throughout the entry capture, even after the extra retain.
            reset(layout);
            set(|w| {
                w.change_at = 13;
                w.change_address = GROUP + 0x18;
                w.change_value = 2 << 32;
            });
            {
                let mut lease = TaskCompletionLease::default();
                assert!(
                    !capture(&mut lease)
                        && lease.rejection() == TaskCompletionFailure::NoCallbackToken
                );
            }
            balanced();
        }
    }
}
