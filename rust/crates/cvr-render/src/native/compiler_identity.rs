//! The render graph compiler's identity, pinned through one dispatch (include/Render/NativeGraphCompilerIdentity.hpp).
//!
//! Captured before the dispatch (return 0x40A76C): the producer job's group (D1) and frame, the renderer passed in R8,
//! its wrapper (+0x98) and the wrapper's pool, all matching the frozen binding, read twice, then D1 retained and
//! everything read twice more. The compiler (return 0xBB68B5, mode 2, same pool) is matched through its local job's
//! parent handle (+0x20) to the pinned group, which must hold a callback token (pending > 0). The AF59C0 local
//! JobQueue's +0x20 is the actual worker handle, not the worker: no worker field or job+0x38 is read as a renderer, and
//! matching may happen after the root returned (no producer stack reads). Complete only after the match, the three
//! normal returns (compiler, dispatch, root) and the pinned D1's completion. This pins only D1; it neither owns
//! definitions nor proves resource freshness or GPU completion. The first rejection sticks (and a failed lease never
//! re-arms); ephemeral addresses are cleared on every return, even after a failure. The caller serializes methods.
#![allow(unsafe_op_in_unsafe_fn)]
use super::job_lease::JobLeaseApi;
use core::ffi::c_void;

pub const COMPILER_DISPATCH_RETURN: u64 = 0x40A76C;
pub const COMPILER_CALL_RETURN: u64 = 0xBB68B5;

/// A read of the engine's memory: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompilerBinding {
    pub consumer_id: u64,
    pub epoch: u64,
    pub generation: u64,
    pub renderer: u64,
    pub wrapper: u64,
    pub pool: u64,
    pub frame_owner: u64,
    pub execution_graph: u64,
    pub producer_context: u64,
}
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Failure {
    #[default]
    None,
    Arguments,
    Caller,
    AlreadyAttempted,
    MissingApi,
    Read,
    ProducerIdentity,
    Renderer,
    Wrapper,
    Pool,
    FrameOwner,
    Group,
    References,
    Pending,
    Changed,
    NotCaptured,
    AlreadyMatched,
    CompilerJob,
    CompilerParent,
    NoCallbackToken,
    NotMatched,
    AlreadyReturned,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DispatchIdentity {
    group: u64,
    frame: u64,
    renderer: u64,
    wrapper: u64,
    pool: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Parent {
    handle: u64,
    group: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CompileIdentity {
    parent: Parent,
    wrapper: u64,
    pool: u64,
}

fn pointer(address: u64, bytes: u64) -> bool {
    address != 0 && address & 7 == 0 && address <= u64::MAX - bytes
}
fn r64(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, at: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(at, &mut b).then(|| u64::from_le_bytes(b))
}
fn r32(read: &mut dyn FnMut(u64, &mut [u8]) -> bool, at: u64) -> Option<i32> {
    let mut b = [0u8; 4];
    read(at, &mut b).then(|| i32::from_le_bytes(b))
}

/// The parent chain of a compiler job: job+0x20 -> handle -> group, with a live reference and a callback token.
fn read_parent(job: u64, read: &mut dyn FnMut(u64, &mut [u8]) -> bool) -> Result<Parent, Failure> {
    if !pointer(job, 0x28) {
        return Err(Failure::CompilerJob);
    }
    let handle = r64(read, job + 0x20).ok_or(Failure::Read)?;
    if !pointer(handle, 8) {
        return Err(Failure::CompilerParent);
    }
    let group = r64(read, handle).ok_or(Failure::Read)?;
    if !pointer(group, 0x20) {
        return Err(Failure::Group);
    }
    let refs = r32(read, group + 0x1c).ok_or(Failure::Read)?;
    let pending = r32(read, group + 0x18).ok_or(Failure::Read)?;
    if refs <= 0 {
        return Err(Failure::References);
    }
    if pending <= 0 {
        return Err(Failure::NoCallbackToken);
    }
    Ok(Parent { handle, group })
}

pub struct CompilerIdentityLease {
    binding: CompilerBinding,
    api: Option<JobLeaseApi>,
    group_pin: *mut c_void,
    image_base: u64,
    producer_context: u64,
    renderer_argument_address: u64,
    compiler_job: u64,
    compiler_parent_handle: u64,
    attempted: bool,
    captured: bool,
    matched: bool,
    dispatch_returned: bool,
    compiler_returned: bool,
    root_returned: bool,
    failed: bool,
    failure: Failure,
}
impl Default for CompilerIdentityLease {
    fn default() -> Self {
        CompilerIdentityLease {
            binding: CompilerBinding::default(),
            api: None,
            group_pin: core::ptr::null_mut(),
            image_base: 0,
            producer_context: 0,
            renderer_argument_address: 0,
            compiler_job: 0,
            compiler_parent_handle: 0,
            attempted: false,
            captured: false,
            matched: false,
            dispatch_returned: false,
            compiler_returned: false,
            root_returned: false,
            failed: false,
            failure: Failure::None,
        }
    }
}
impl Drop for CompilerIdentityLease {
    fn drop(&mut self) {
        if let (false, Some(release)) = (self.group_pin.is_null(), self.api.and_then(|a| a.release))
        {
            // SAFETY: the group this lease retained.
            unsafe { release(&mut self.group_pin) };
        }
    }
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

impl CompilerIdentityLease {
    /// The first refusal is kept; every later call is refused (the C++ face also refuses a throwing reader with Read).
    pub(crate) fn reject(&mut self, reason: Failure) -> bool {
        if !self.failed {
            self.failure = reason;
        }
        self.failed = true;
        false
    }
    fn read_dispatch(
        &mut self,
        binding: &CompilerBinding,
        job: u64,
        argument: u64,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
        before_retain: bool,
    ) -> Option<DispatchIdentity> {
        let mut out = DispatchIdentity::default();
        self.failure = Failure::Read;
        out.renderer = r64(read, argument)?;
        out.group = r64(read, job + 0x18)?;
        out.frame = r64(read, job + 0x38)?;
        self.failure = Failure::Renderer;
        if out.renderer != binding.renderer {
            return None;
        }
        self.failure = Failure::FrameOwner;
        if out.frame != binding.frame_owner {
            return None;
        }
        self.failure = Failure::Group;
        if !pointer(out.group, 0x20) || out.group == out.frame {
            return None;
        }
        self.failure = Failure::Read;
        out.wrapper = r64(read, out.renderer + 0x98)?;
        self.failure = Failure::Wrapper;
        if out.wrapper != binding.wrapper {
            return None;
        }
        self.failure = Failure::Read;
        out.pool = r64(read, out.wrapper)?;
        self.failure = Failure::Pool;
        if out.pool != binding.pool {
            return None;
        }
        self.failure = Failure::Read;
        let refs = r32(read, out.group + 0x1c)?;
        let pending = r32(read, out.group + 0x18)?;
        self.failure = Failure::References;
        if refs <= 0 || (before_retain && refs == i32::MAX) {
            return None;
        }
        self.failure = Failure::Pending;
        if pending < 0 {
            return None;
        }
        Some(out)
    }
    fn read_compile(
        &mut self,
        job: u64,
        pool: u64,
        read: &mut dyn FnMut(u64, &mut [u8]) -> bool,
    ) -> Option<CompileIdentity> {
        let parent = match read_parent(job, read) {
            Ok(p) => p,
            Err(reason) => {
                self.failure = reason;
                return None;
            }
        };
        self.failure = Failure::Group;
        if parent.group != self.group() {
            return None;
        }
        self.failure = Failure::Read;
        let wrapper = r64(read, self.binding.renderer + 0x98)?;
        self.failure = Failure::Wrapper;
        if wrapper != self.binding.wrapper {
            return None;
        }
        self.failure = Failure::Read;
        let p = r64(read, wrapper)?;
        self.failure = Failure::Pool;
        if p != pool || p != self.binding.pool {
            return None;
        }
        Some(CompileIdentity {
            parent,
            wrapper,
            pool: p,
        })
    }

    /// The dispatch's identity pinned before the original dispatch runs.
    ///
    /// # Safety
    /// The api's callbacks are the engine's group calls (they do not unwind); the group they get is the producer's.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn capture_before_dispatch(
        &mut self,
        binding: &CompilerBinding,
        producer_job: u64,
        renderer_argument_address: u64,
        caller: u64,
        image_base: u64,
        read: Option<Read<'_>>,
        api: JobLeaseApi,
    ) -> bool {
        if self.failed {
            return false;
        }
        if self.attempted {
            return self.reject(Failure::AlreadyAttempted);
        }
        self.attempted = true;
        let Some(read) = read else {
            return self.reject(Failure::Arguments);
        };
        if image_base == 0
            || image_base > u64::MAX - COMPILER_CALL_RETURN
            || binding.consumer_id == 0
            || binding.epoch == 0
            || binding.generation == 0
            || !pointer(producer_job, 0x40)
            || !pointer(renderer_argument_address, 8)
            || !pointer(binding.renderer, 0xA0)
            || !pointer(binding.wrapper, 8)
            || !pointer(binding.pool, 3)
            || !pointer(binding.frame_owner, 12)
            || !pointer(binding.execution_graph, 0x70)
        {
            return self.reject(Failure::Arguments);
        }
        if producer_job != binding.producer_context {
            return self.reject(Failure::ProducerIdentity);
        }
        if caller != image_base + COMPILER_DISPATCH_RETURN {
            return self.reject(Failure::Caller);
        }
        let (Some(retain), Some(release), Some(_)) = (api.retain, api.release, api.complete) else {
            return self.reject(Failure::MissingApi);
        };
        let Some(first) =
            self.read_dispatch(binding, producer_job, renderer_argument_address, read, true)
        else {
            return self.reject(self.failure);
        };
        let Some(second) =
            self.read_dispatch(binding, producer_job, renderer_argument_address, read, true)
        else {
            return self.reject(self.failure);
        };
        if first != second {
            return self.reject(Failure::Changed);
        }
        // Released on every way out but success -- a refusal or a reader unwinding through here (the C++ face's
        // readers may throw).
        let mut pin = Pin {
            release,
            group: first.group as usize as *mut c_void,
        };
        retain(pin.group);
        let second = self.read_dispatch(
            binding,
            producer_job,
            renderer_argument_address,
            read,
            false,
        );
        let third = second.and_then(|_| {
            self.read_dispatch(
                binding,
                producer_job,
                renderer_argument_address,
                read,
                false,
            )
        });
        let (Some(second), Some(third)) = (second, third) else {
            return self.reject(self.failure);
        };
        if first != second || first != third {
            return self.reject(Failure::Changed);
        }
        self.binding = *binding;
        self.image_base = image_base;
        self.producer_context = producer_job;
        self.renderer_argument_address = renderer_argument_address;
        self.api = Some(api);
        self.group_pin = core::mem::replace(&mut pin.group, core::ptr::null_mut());
        self.captured = true;
        self.failure = Failure::None;
        true
    }
    /// Non-mutating domain filter: unrelated frame compilations may use the same pool. A failed filter is not a
    /// completion or an authority to discard pins.
    pub fn matches_compiler_parent(&self, compiler_job: u64, read: Option<Read<'_>>) -> bool {
        let Some(read) = read else { return false };
        if !self.captured || self.failed {
            return false;
        }
        match (
            read_parent(compiler_job, read),
            read_parent(compiler_job, read),
        ) {
            (Ok(a), Ok(b)) => a == b && a.group == self.group(),
            _ => false,
        }
    }
    pub fn match_before_compiler(
        &mut self,
        pool: u64,
        mode: u8,
        compiler_job: u64,
        caller: u64,
        read: Option<Read<'_>>,
    ) -> bool {
        if self.failed {
            return false;
        }
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if self.matched {
            return self.reject(Failure::AlreadyMatched);
        }
        let Some(read) = read else {
            return self.reject(Failure::Arguments);
        };
        if !pointer(pool, 3) {
            return self.reject(Failure::Arguments);
        }
        if caller != self.image_base + COMPILER_CALL_RETURN || mode != 2 {
            return self.reject(Failure::Caller);
        }
        if pool != self.binding.pool {
            return self.reject(Failure::Pool);
        }
        let Some(first) = self.read_compile(compiler_job, pool, read) else {
            return self.reject(self.failure);
        };
        let Some(second) = self.read_compile(compiler_job, pool, read) else {
            return self.reject(self.failure);
        };
        if first != second {
            return self.reject(Failure::Changed);
        }
        self.compiler_job = compiler_job;
        self.compiler_parent_handle = first.parent.handle;
        self.matched = true;
        self.failure = Failure::None;
        true
    }
    pub fn mark_original_dispatch_returned(&mut self) -> bool {
        self.renderer_argument_address = 0;
        if self.failed {
            return false;
        }
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if self.dispatch_returned {
            return self.reject(Failure::AlreadyReturned);
        }
        self.dispatch_returned = true;
        true
    }
    pub fn mark_original_compiler_returned(&mut self) -> bool {
        self.compiler_job = 0;
        if self.failed {
            return false;
        }
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if !self.matched {
            return self.reject(Failure::NotMatched);
        }
        if self.compiler_returned {
            return self.reject(Failure::AlreadyReturned);
        }
        self.compiler_returned = true;
        true
    }
    /// The original 219730's normal return (not cancellation, timeout or the AA scheduler's return).
    pub fn mark_original_root_returned(&mut self) -> bool {
        self.producer_context = 0;
        self.renderer_argument_address = 0;
        if !self.captured {
            return self.reject(Failure::NotCaptured);
        }
        if self.root_returned {
            return self.reject(Failure::AlreadyReturned);
        }
        self.root_returned = true;
        !self.failed
    }
    pub fn complete(&self) -> bool {
        let Some(complete) = self.api.and_then(|a| a.complete) else {
            return false;
        };
        self.captured
            && !self.failed
            && self.matched
            && self.dispatch_returned
            && self.compiler_returned
            && self.root_returned
            && !self.group_pin.is_null()
            // SAFETY: the group this lease holds.
            && unsafe { complete(self.group_pin) }
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    pub fn matched(&self) -> bool {
        self.matched
    }
    pub fn original_dispatch_returned(&self) -> bool {
        self.dispatch_returned
    }
    pub fn original_compiler_returned(&self) -> bool {
        self.compiler_returned
    }
    pub fn original_root_returned(&self) -> bool {
        self.root_returned
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
    pub fn group(&self) -> u64 {
        self.group_pin as u64
    }
    pub fn binding(&self) -> &CompilerBinding {
        &self.binding
    }
    pub fn matches_binding(&self, expected: &CompilerBinding) -> bool {
        self.captured && !self.failed && self.binding == *expected
    }
    pub fn producer_context_address(&self) -> u64 {
        self.producer_context
    }
    pub fn renderer_argument_address(&self) -> u64 {
        self.renderer_argument_address
    }
    pub fn compiler_job_address(&self) -> u64 {
        self.compiler_job
    }
    /// A frozen identity label only; no late dereference is permitted.
    pub fn compiler_parent_handle(&self) -> u64 {
        self.compiler_parent_handle
    }
    pub fn rejection(&self) -> Failure {
        self.failure
    }
}

#[cfg(test)]
mod tests {
    //! tools/single_pass_tests/compiler_identity_test.cpp's cases (the C++ reader's thrown exception is a failed read
    //! here: Rust readers return false).
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const BASE: u64 = 0x1_4000_0000;
    const CONTEXT: u64 = 0x10000;
    const ARGUMENT: u64 = 0x11000;
    const FRAME: u64 = 0x20000;
    const GRAPH: u64 = 0x30000;
    const GROUP: u64 = 0x40000;
    const FOREIGN_GROUP: u64 = 0x41000;
    const RENDERER: u64 = 0x50000;
    const WRAPPER: u64 = 0x60000;
    const POOL: u64 = 0x70000;
    const COMPILER_JOB: u64 = 0x80000;
    const PARENT_HANDLE: u64 = 0x90000;
    const FOREIGN_HANDLE: u64 = 0x91000;

    #[derive(Default)]
    struct World {
        memory: BTreeMap<u64, u8>,
        reads: u64,
        fail_at: u64,
        change_at: u64,
        change_address: u64,
        change_value: u64,
        refs: i32,
        baseline_refs: i32,
        pending: i32,
        retains: u32,
        releases: u32,
        complete_calls: u32,
        change_on_retain: bool,
        expired_producer: bool,
        forbid_reads: bool,
    }
    thread_local! {
        static W: RefCell<World> = RefCell::new(World::default());
    }
    fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
        W.with(|w| f(&mut w.borrow_mut()))
    }
    fn put(w: &mut World, address: u64, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            w.memory.insert(address + i as u64, *v);
        }
    }
    fn put64(address: u64, v: u64) {
        with(|w| put(w, address, &v.to_le_bytes()));
    }
    fn put32(address: u64, v: i32) {
        with(|w| put(w, address, &v.to_le_bytes()));
    }
    fn read(address: u64, out: &mut [u8]) -> bool {
        with(|w| {
            assert!(!w.forbid_reads, "the lease reread expired native storage");
            assert!(
                !w.expired_producer
                    || !((CONTEXT..CONTEXT + 0x40).contains(&address) || address == ARGUMENT),
                "the lease reread the expired producer context or R8 argument"
            );
            w.reads += 1;
            if w.reads == w.fail_at {
                return false;
            }
            if w.reads == w.change_at {
                let (a, v) = (w.change_address, w.change_value);
                put(w, a, &v.to_le_bytes());
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
    unsafe extern "C" fn retain(group: *mut c_void) {
        with(|w| {
            assert!(
                group as u64 == GROUP && w.refs > 0 && w.refs < i32::MAX,
                "invalid D1 retain"
            );
            w.refs += 1;
            w.retains += 1;
            let r = w.refs;
            put(w, GROUP + 0x1c, &r.to_le_bytes());
            if w.change_on_retain {
                put(w, CONTEXT + 0x18, &FOREIGN_GROUP.to_le_bytes());
            }
        });
    }
    unsafe extern "C" fn release(group: *mut *mut c_void) {
        with(|w| {
            assert!(
                *group as u64 == GROUP && w.refs > 0 && w.releases < w.retains,
                "invalid D1 release"
            );
            w.refs -= 1;
            w.releases += 1;
            let r = w.refs;
            put(w, GROUP + 0x1c, &r.to_le_bytes());
        });
        *group = core::ptr::null_mut();
    }
    unsafe extern "C" fn complete(group: *mut c_void) -> bool {
        with(|w| {
            assert!(group as u64 == GROUP, "polled a foreign group");
            w.complete_calls += 1;
            w.pending == 0
        })
    }
    const API: JobLeaseApi = JobLeaseApi {
        retain: Some(retain),
        release: Some(release),
        complete: Some(complete),
    };
    fn binding() -> CompilerBinding {
        CompilerBinding {
            consumer_id: 13,
            epoch: 11,
            generation: 12,
            renderer: RENDERER,
            wrapper: WRAPPER,
            pool: POOL,
            frame_owner: FRAME,
            execution_graph: GRAPH,
            producer_context: CONTEXT,
        }
    }
    fn set_pending(v: i32) {
        with(|w| w.pending = v);
        put32(GROUP + 0x18, v);
    }
    fn reset() {
        with(|w| {
            *w = World {
                refs: 1,
                baseline_refs: 1,
                ..World::default()
            };
        });
        put64(CONTEXT + 0x18, GROUP);
        put64(CONTEXT + 0x38, FRAME);
        put64(ARGUMENT, RENDERER);
        put64(RENDERER + 0x98, WRAPPER);
        put64(WRAPPER, POOL);
        put64(COMPILER_JOB + 0x20, PARENT_HANDLE);
        put64(PARENT_HANDLE, GROUP);
        put64(FOREIGN_HANDLE, FOREIGN_GROUP);
        put32(FOREIGN_GROUP + 0x1c, 1);
        put32(FOREIGN_GROUP + 0x18, 1);
        put32(GROUP + 0x1c, 1);
        put32(GROUP + 0x18, 0);
    }
    fn capture(l: &mut CompilerIdentityLease) -> bool {
        unsafe {
            l.capture_before_dispatch(
                &binding(),
                CONTEXT,
                ARGUMENT,
                BASE + COMPILER_DISPATCH_RETURN,
                BASE,
                Some(&mut read),
                API,
            )
        }
    }
    fn matched(l: &mut CompilerIdentityLease) -> bool {
        l.match_before_compiler(
            POOL,
            2,
            COMPILER_JOB,
            BASE + COMPILER_CALL_RETURN,
            Some(&mut read),
        )
    }
    fn balanced() {
        with(|w| {
            assert!(
                w.refs == w.baseline_refs && w.retains == w.releases,
                "D1 pin imbalance"
            )
        });
    }
    fn finish(l: &mut CompilerIdentityLease) {
        assert!(
            l.mark_original_compiler_returned()
                && l.mark_original_dispatch_returned()
                && l.mark_original_root_returned()
        );
    }

    #[test]
    fn lifecycle() {
        for _ in 0..64 {
            reset();
            {
                let mut l = CompilerIdentityLease::default();
                assert!(!l.captured() && !l.complete() && !l.matches_binding(&binding()));
                assert!(
                    capture(&mut l)
                        && with(|w| w.refs) == 2
                        && l.group() == GROUP
                        && *l.binding() == binding()
                );
                assert!(
                    l.producer_context_address() == CONTEXT
                        && l.renderer_argument_address() == ARGUMENT
                );
                assert!(!l.complete() && with(|w| w.complete_calls) == 0);
                set_pending(1);
                assert!(
                    l.matches_compiler_parent(COMPILER_JOB, Some(&mut read)) && matched(&mut l)
                );
                assert!(
                    l.compiler_parent_handle() == PARENT_HANDLE
                        && l.compiler_job_address() == COMPILER_JOB
                        && l.matched()
                );
                assert!(!l.complete() && with(|w| w.complete_calls) == 0);
                assert!(
                    l.mark_original_compiler_returned()
                        && l.compiler_job_address() == 0
                        && !l.complete()
                );
                assert!(
                    l.mark_original_dispatch_returned()
                        && l.renderer_argument_address() == 0
                        && !l.complete()
                );
                put64(CONTEXT + 0x18, FOREIGN_GROUP);
                assert!(l.mark_original_root_returned() && l.producer_context_address() == 0);
                with(|w| {
                    w.refs -= 1;
                    w.baseline_refs = 0;
                    w.expired_producer = true;
                });
                set_pending(2);
                assert!(!l.complete());
                set_pending(0);
                with(|w| {
                    w.memory.clear();
                    w.forbid_reads = true;
                });
                assert!(l.complete() && l.matches_binding(&binding()));
            }
            balanced();
        }
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            assert!(l.mark_original_dispatch_returned() && l.mark_original_root_returned());
            with(|w| w.expired_producer = true);
            set_pending(1);
            assert!(l.matches_compiler_parent(COMPILER_JOB, Some(&mut read)) && matched(&mut l));
            assert!(l.mark_original_compiler_returned() && !l.complete());
            set_pending(0);
            with(|w| w.forbid_reads = true);
            assert!(l.complete());
        }
        balanced();
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l) && l.group() == GROUP);
        }
        balanced();
    }
    #[test]
    fn invalid_capture() {
        for invalid in 0..23 {
            reset();
            {
                let mut l = CompilerIdentityLease::default();
                let mut b = binding();
                let (mut job, mut argument, mut caller, mut base) =
                    (CONTEXT, ARGUMENT, BASE + COMPILER_DISPATCH_RETURN, BASE);
                let mut api = API;
                let mut no_reader = false;
                match invalid {
                    0 => b.consumer_id = 0,
                    1 => b.epoch = 0,
                    2 => b.generation = 0,
                    3 => b.renderer = 0,
                    4 => b.wrapper = 0,
                    5 => b.pool = 0,
                    6 => b.frame_owner = 0,
                    7 => b.execution_graph = 0,
                    8 => b.producer_context += 8,
                    9 => job = 0,
                    10 => job += 1,
                    11 => argument = 0,
                    12 => argument += 1,
                    13 => caller += 1,
                    14 => base = u64::MAX,
                    15 => no_reader = true,
                    16 => api.retain = None,
                    17 => api.release = None,
                    18 => api.complete = None,
                    19 => b.renderer = u64::MAX - 7,
                    20 => b.execution_graph += 1,
                    21 => argument = u64::MAX - 7,
                    _ => job = u64::MAX - 7,
                }
                let reader: Option<Read<'_>> = if no_reader { None } else { Some(&mut read) };
                let ok = unsafe {
                    l.capture_before_dispatch(&b, job, argument, caller, base, reader, api)
                };
                assert!(
                    !ok && l.failed() && !l.captured() && !l.complete(),
                    "case {invalid}"
                );
            }
            balanced();
        }
        for group in [0, GROUP + 1, FRAME, u64::MAX - 7] {
            reset();
            put64(CONTEXT + 0x18, group);
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.failed());
            drop(l);
            balanced();
        }
        for address in [ARGUMENT, CONTEXT + 0x38, RENDERER + 0x98, WRAPPER] {
            reset();
            let v = match address {
                ARGUMENT => RENDERER + 0x100,
                a if a == CONTEXT + 0x38 => FRAME + 0x100,
                WRAPPER => POOL + 0x100,
                _ => WRAPPER + 0x100,
            };
            put64(address, v);
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.failed());
            drop(l);
            balanced();
        }
        for count in [0, -1, i32::MAX] {
            reset();
            put32(GROUP + 0x1c, count);
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::References);
            drop(l);
            balanced();
        }
        reset();
        set_pending(-1);
        {
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.rejection() == Failure::Pending);
        }
        balanced();
        reset();
        with(|w| {
            w.refs = i32::MAX - 1;
            w.baseline_refs = i32::MAX - 1;
        });
        put32(GROUP + 0x1c, i32::MAX - 1);
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l) && with(|w| w.refs) == i32::MAX);
        }
        balanced();
    }
    #[test]
    fn partial_reads_and_torn_fields() {
        reset();
        let capture_reads = {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            with(|w| w.reads)
        };
        balanced();
        for at in 1..=capture_reads {
            reset();
            with(|w| w.fail_at = at);
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.failed() && !l.captured() && !l.complete());
            drop(l);
            balanced();
        }
        reset();
        with(|w| w.change_on_retain = true);
        {
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.failed());
        }
        balanced();
        assert!(with(|w| w.retains == 1 && w.releases == 1));
        for address in [
            ARGUMENT,
            CONTEXT + 0x18,
            CONTEXT + 0x38,
            RENDERER + 0x98,
            WRAPPER,
        ] {
            reset();
            let v = match address {
                ARGUMENT => RENDERER + 0x100,
                a if a == CONTEXT + 0x18 => FOREIGN_GROUP,
                a if a == CONTEXT + 0x38 => FRAME + 0x100,
                WRAPPER => POOL + 0x100,
                _ => WRAPPER + 0x100,
            };
            with(|w| {
                w.change_at = 8;
                w.change_address = address;
                w.change_value = v;
            });
            let mut l = CompilerIdentityLease::default();
            assert!(!capture(&mut l) && l.failed());
            drop(l);
            balanced();
        }
        reset();
        let match_reads = {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            set_pending(1);
            with(|w| w.reads = 0);
            assert!(matched(&mut l));
            with(|w| w.reads)
        };
        balanced();
        for at in 1..=match_reads {
            reset();
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            set_pending(1);
            with(|w| {
                w.reads = 0;
                w.fail_at = at;
            });
            assert!(!matched(&mut l) && l.failed() && !l.complete());
            assert!(
                !l.mark_original_root_returned()
                    && l.producer_context_address() == 0
                    && l.renderer_argument_address() == 0
            );
            drop(l);
            balanced();
        }
        for address in [COMPILER_JOB + 0x20, PARENT_HANDLE, RENDERER + 0x98, WRAPPER] {
            reset();
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            set_pending(1);
            let v = match address {
                a if a == COMPILER_JOB + 0x20 => FOREIGN_HANDLE,
                PARENT_HANDLE => FOREIGN_GROUP,
                WRAPPER => POOL + 0x100,
                _ => WRAPPER + 0x100,
            };
            with(|w| {
                w.change_at = w.reads + 7;
                w.change_address = address;
                w.change_value = v;
            });
            assert!(!matched(&mut l) && l.failed() && !l.complete());
            drop(l);
            balanced();
        }
    }
    #[test]
    fn foreign_and_invalid_compiler() {
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            set_pending(1);
            put64(COMPILER_JOB + 0x20, FOREIGN_HANDLE);
            assert!(!l.matches_compiler_parent(COMPILER_JOB, Some(&mut read)) && !l.failed());
            put64(COMPILER_JOB + 0x20, PARENT_HANDLE);
            assert!(l.matches_compiler_parent(COMPILER_JOB, Some(&mut read)) && matched(&mut l));
            finish(&mut l);
            set_pending(0);
            assert!(l.complete());
        }
        balanced();
        for invalid in 0..13 {
            reset();
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            set_pending(1);
            let (mut pool, mut job, mut caller, mut mode) =
                (POOL, COMPILER_JOB, BASE + COMPILER_CALL_RETURN, 2u8);
            let mut no_reader = false;
            match invalid {
                0 => put64(COMPILER_JOB + 0x20, FOREIGN_HANDLE),
                1 => put64(PARENT_HANDLE, FOREIGN_GROUP),
                2 => job = 0,
                3 => job += 1,
                4 => pool += 8,
                5 => mode = 1,
                6 => caller += 1,
                7 => no_reader = true,
                8 => put64(RENDERER + 0x98, WRAPPER + 8),
                9 => put64(WRAPPER, POOL + 8),
                10 => put32(GROUP + 0x1c, 0),
                11 => set_pending(0),
                _ => put64(COMPILER_JOB + 0x20, PARENT_HANDLE + 1),
            }
            let reader: Option<Read<'_>> = if no_reader { None } else { Some(&mut read) };
            assert!(
                !l.match_before_compiler(pool, mode, job, caller, reader)
                    && l.failed()
                    && !l.complete(),
                "case {invalid}"
            );
            drop(l);
            balanced();
        }
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            assert!(!l.matches_compiler_parent(COMPILER_JOB, Some(&mut read)) && !matched(&mut l));
            assert!(l.rejection() == Failure::NoCallbackToken && with(|w| w.complete_calls) == 0);
        }
        balanced();
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            for field in 0..9 {
                let mut o = binding();
                match field {
                    0 => o.consumer_id += 1,
                    1 => o.epoch += 1,
                    2 => o.generation += 1,
                    3 => o.renderer += 8,
                    4 => o.wrapper += 8,
                    5 => o.pool += 8,
                    6 => o.frame_owner += 8,
                    7 => o.execution_graph += 8,
                    _ => o.producer_context += 8,
                }
                assert!(!l.matches_binding(&o) && !l.failed());
            }
            assert_eq!(*l.binding(), binding());
        }
        balanced();
    }
    #[test]
    fn returns_and_rearm() {
        for missing in 0..4 {
            reset();
            {
                let mut l = CompilerIdentityLease::default();
                assert!(capture(&mut l));
                set_pending(1);
                if missing != 0 {
                    assert!(matched(&mut l));
                }
                if missing != 0 && missing != 1 {
                    assert!(l.mark_original_compiler_returned());
                }
                if missing != 2 {
                    assert!(l.mark_original_dispatch_returned());
                }
                if missing != 3 {
                    assert!(l.mark_original_root_returned());
                }
                set_pending(0);
                assert!(!l.complete() && with(|w| w.complete_calls) == 0);
            }
            balanced();
        }
        for duplicate in 0..4 {
            reset();
            {
                let mut l = CompilerIdentityLease::default();
                assert!(capture(&mut l));
                set_pending(1);
                assert!(matched(&mut l));
                finish(&mut l);
                set_pending(0);
                assert!(l.complete());
                let accepted = match duplicate {
                    0 => matched(&mut l),
                    1 => l.mark_original_compiler_returned(),
                    2 => l.mark_original_dispatch_returned(),
                    _ => l.mark_original_root_returned(),
                };
                assert!(!accepted && l.failed() && !l.complete());
            }
            balanced();
        }
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            assert!(!l.mark_original_compiler_returned() && l.failed() && !l.complete());
        }
        balanced();
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(!l.mark_original_root_returned());
            with(|w| w.forbid_reads = true);
            assert!(!capture(&mut l) && !l.captured() && with(|w| w.retains) == 0);
        }
        balanced();
        reset();
        {
            let mut l = CompilerIdentityLease::default();
            assert!(capture(&mut l));
            let mut o = binding();
            o.execution_graph += 0x100;
            let ok = unsafe {
                l.capture_before_dispatch(
                    &o,
                    CONTEXT,
                    ARGUMENT,
                    BASE + COMPILER_DISPATCH_RETURN,
                    BASE,
                    Some(&mut read),
                    API,
                )
            };
            assert!(!ok && l.failed() && *l.binding() == binding() && l.group() == GROUP);
            l.mark_original_root_returned();
            with(|w| w.forbid_reads = true);
            assert!(
                !l.complete()
                    && l.producer_context_address() == 0
                    && l.renderer_argument_address() == 0
            );
        }
        balanced();
    }
}
