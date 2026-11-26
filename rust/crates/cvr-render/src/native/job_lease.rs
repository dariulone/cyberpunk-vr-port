//! A hold on an engine job group pair (include/Render/NativeJobLease.hpp): retained while the source native job scope
//! is live, then sealed after all work was queued. Both the begin and the end group are held -- Advance moves the
//! pending end group to begin and makes a new empty end, which alone would falsely appear complete. Released on drop.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;

/// The engine's group calls (retain the group, release and null the pointer, completed).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JobLeaseApi {
    pub retain: Option<unsafe extern "C" fn(*mut c_void)>,
    pub release: Option<unsafe extern "C" fn(*mut *mut c_void)>,
    pub complete: Option<unsafe extern "C" fn(*mut c_void) -> bool>,
}

pub struct NativeJobLease {
    api: JobLeaseApi,
    groups: [*mut c_void; 2],
    sealed: bool,
}
impl NativeJobLease {
    pub fn new(api: JobLeaseApi) -> Self {
        NativeJobLease {
            api,
            groups: [core::ptr::null_mut(); 2],
            sealed: false,
        }
    }
    /// The pair held (the end only when it is another group).
    ///
    /// # Safety
    /// `begin` and `end` are live engine job groups.
    pub unsafe fn capture(&mut self, begin: *mut c_void, end: *mut c_void) -> bool {
        let (Some(retain), Some(_), Some(_)) =
            (self.api.retain, self.api.release, self.api.complete)
        else {
            return false;
        };
        if self.sealed || begin.is_null() || end.is_null() {
            return false;
        }
        self.groups = [
            begin,
            if begin == end {
                core::ptr::null_mut()
            } else {
                end
            },
        ];
        for group in self.groups {
            if !group.is_null() {
                retain(group);
            }
        }
        self.sealed = true;
        true
    }
    pub fn complete(&self) -> bool {
        if !self.sealed {
            return false;
        }
        let Some(complete) = self.api.complete else {
            return false;
        };
        // SAFETY: groups this lease holds.
        self.groups
            .iter()
            .all(|&g| g.is_null() || unsafe { complete(g) })
    }
}
impl Drop for NativeJobLease {
    fn drop(&mut self) {
        if let Some(release) = self.api.release {
            for group in self.groups.iter_mut() {
                if !group.is_null() {
                    // SAFETY: a group this lease retained.
                    unsafe { release(group) };
                }
            }
        }
    }
}
