//! The retained capture's native target cluster owned on the Rust side (formerly the C++ holders of
//! NativeTextureRead.cpp's WritableTargetIdentity, NativeTargetLifetime.cpp's Lease / BorrowedScope / BorrowedPageable,
//! NativeTargetHandoff.cpp's SnapshotSources and NativeTargetAccess.cpp's Receipt): every runtime is already Rust
//! (texture_writable.rs, target_lifetime.rs, target_handoff_observer.rs, target_access_observer.rs, queue_residency.rs);
//! these are the proofs they hand out, held here.
//!
//! An identity keeps the C++ WritableTargetIdentity layout: the 368-byte identity, then its private authenticated copy
//! as a shared_ptr's two words (the copy's address, no control block), so target_lifetime.rs' attached C++ validator
//! reads a Rust-owned identity as it reads a C++ one.

use core::ffi::c_void;
use std::sync::Arc;

use windows::core::{IUnknown, Interface};

use crate::native_recording::Lease;
use crate::texture_writable::WritableIdentity;

fn add_ref(raw: *mut c_void) {
    if !raw.is_null() {
        // SAFETY: a live reference the caller names; one more taken (released by `release`).
        core::mem::forget(unsafe { IUnknown::from_raw_borrowed(&raw) }.cloned());
    }
}
fn release(raw: *mut c_void) {
    if !raw.is_null() {
        // SAFETY: a reference this side holds, released once.
        drop(unsafe { IUnknown::from_raw(raw) });
    }
}
/// A Lease copied (its token referenced again, as ComPtr<IUnknown>'s copy).
pub fn lease_copy(lease: &Lease) -> Lease {
    add_ref(lease.token);
    Lease { token: lease.token, generation: lease.generation, context: lease.context, list: lease.list, registry: lease.registry }
}
/// A lease owned: its token released with it.
pub struct OwnedLease(pub Lease);
impl Drop for OwnedLease {
    fn drop(&mut self) {
        release(self.0.token);
    }
}
impl Clone for OwnedLease {
    fn clone(&self) -> OwnedLease {
        OwnedLease(lease_copy(&self.0))
    }
}
fn identity_clone(value: &WritableIdentity) -> WritableIdentity {
    WritableIdentity {
        creation: value.creation.clone(),
        native: value.native,
        recording: lease_copy(&value.recording),
        original_context: value.original_context,
        resource_desc: value.resource_desc,
        heap_desc: value.heap_desc,
        resource_heap_properties: value.resource_heap_properties,
        resource_heap_flags: value.resource_heap_flags,
        expected_writable_state: value.expected_writable_state,
        native_record: value.native_record,
        allocator_owner_observation: value.allocator_owner_observation,
    }
}
/// An identity's own references released (its creation's by drop, its recording's token here).
struct Plain(WritableIdentity);
impl Drop for Plain {
    fn drop(&mut self) {
        release(self.0.recording.token);
    }
}

// ---- the writable target identity

/// texture_read::WritableTargetIdentity owned, in the C++ layout (the authenticated copy's address in the trailing
/// shared_ptr words).
#[repr(C)]
pub struct Identity {
    value: core::mem::ManuallyDrop<WritableIdentity>,
    data: [usize; 2],
    captured: Option<Arc<Plain>>,
}
const _: () = assert!(core::mem::offset_of!(Identity, data) == 368);
impl Drop for Identity {
    fn drop(&mut self) {
        // SAFETY: the identity's own value, dropped once.
        drop(Plain(unsafe { core::mem::ManuallyDrop::take(&mut self.value) }));
    }
}
impl Clone for Identity {
    fn clone(&self) -> Identity {
        let mut copy = Identity { value: core::mem::ManuallyDrop::new(identity_clone(&self.value)), data: [0; 2], captured: self.captured.clone() };
        copy.data[0] = copy.captured.as_ref().map_or(0, |c| &c.0 as *const WritableIdentity as usize);
        copy
    }
}
impl Identity {
    /// AuthenticateWritableIdentity: the identity with its authenticated copy (itself, as captured).
    pub fn authenticated(value: WritableIdentity) -> Identity {
        let captured = Arc::new(Plain(identity_clone(&value)));
        let data = [&captured.0 as *const WritableIdentity as usize, 0];
        Identity { value: core::mem::ManuallyDrop::new(value), data, captured: Some(captured) }
    }
    /// CaptureWritableIdentity.
    pub fn capture(list: *mut c_void, resource: *mut c_void, expected: u32) -> Option<Identity> {
        // SAFETY: plain data, all zeroes (None interfaces, a null token) first.
        let mut next: WritableIdentity = unsafe { core::mem::zeroed() };
        // SAFETY: the live list and resource (or null); a zeroed output.
        if !unsafe { crate::texture_writable::cvr_tr_capture_writable_identity(list, resource, expected, &mut next) } {
            drop(Plain(next));
            return None;
        }
        Some(Identity::authenticated(next))
    }
    pub fn value(&self) -> &WritableIdentity {
        &self.value
    }
    /// The identity as the C++ lays it out (for the Rust runtimes that take its address).
    pub fn raw(&self) -> *const WritableIdentity {
        &*self.value
    }
    /// ValidateWritableIdentity.
    pub fn validate(&self, queue: *mut c_void) -> bool {
        let captured = self.captured.as_ref().map_or(core::ptr::null(), |c| &c.0 as *const WritableIdentity);
        // SAFETY: the identity and its authenticated copy; a live queue or null.
        unsafe { crate::texture_writable::cvr_tr_validate_writable_identity(self.raw(), captured, queue) }
    }
}

// ---- the target lifetimes (target_lifetime.rs' proofs)

struct CaptureProof {
    identity: Identity,
    source: crate::target_lifetime::Snapshot,
}
/// target_lifetime::Lease.
#[derive(Clone, Default)]
pub struct LifetimeLease(Option<Arc<CaptureProof>>);
impl LifetimeLease {
    /// target_lifetime::Capture of the identity.
    pub fn capture(identity: &Identity) -> Option<LifetimeLease> {
        // SAFETY: plain data, all zeroes first.
        let mut source: crate::target_lifetime::Snapshot = unsafe { core::mem::zeroed() };
        // SAFETY: the identity (the C++ layout) and a local output.
        if !unsafe { crate::target_lifetime::cvr_tl_capture(identity.raw(), &mut source) } {
            return None;
        }
        Some(LifetimeLease(Some(Arc::new(CaptureProof { identity: identity.clone(), source }))))
    }
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
}
struct BorrowProof {
    captured: Arc<CaptureProof>,
    scope: Option<IUnknown>,
    bound: crate::target_lifetime::Snapshot,
}
/// target_lifetime::BorrowedScope.
#[derive(Clone, Default)]
pub struct BorrowedScope(Option<Arc<BorrowProof>>);
impl BorrowedScope {
    /// target_lifetime::Bind of the lease within the queue's residency scope (its Rust proof).
    pub fn bind(lease: &LifetimeLease, queue: *mut c_void, scope: Option<&IUnknown>) -> Option<BorrowedScope> {
        let captured = lease.0.as_ref();
        // SAFETY: plain data, all zeroes first.
        let mut bound: crate::target_lifetime::Snapshot = unsafe { core::mem::zeroed() };
        let (identity, source) = captured.map_or((core::ptr::null(), core::ptr::null()), |c| (c.identity.raw(), &c.source as *const _));
        let scope_raw = scope.map_or(core::ptr::null_mut(), |s| s.as_raw());
        // SAFETY: the captured proof's records (or null), a live queue and the scope's handle (or null).
        if !unsafe { crate::target_lifetime::cvr_tl_bind(identity, source, queue, scope_raw, &mut bound) } {
            return None;
        }
        Some(BorrowedScope(Some(Arc::new(BorrowProof { captured: captured?.clone(), scope: scope.cloned(), bound }))))
    }
    /// target_lifetime::Validate.
    pub fn validate(&self) -> bool {
        let p = self.0.as_ref();
        let (identity, source) = p.map_or((core::ptr::null(), core::ptr::null()), |p| (p.captured.identity.raw(), &p.captured.source as *const _));
        let scope = p.and_then(|p| p.scope.as_ref()).map_or(core::ptr::null_mut(), |s| s.as_raw());
        let bound = p.map_or(core::ptr::null(), |p| &p.bound as *const _);
        // SAFETY: the proof's records (or null).
        unsafe { crate::target_lifetime::cvr_tl_validate(identity, source, scope, bound) }
    }
    /// target_lifetime::GetBorrowedPageable of `expected` through this scope: the pageable, the scope's identity.
    pub fn pageable(&self, device: *mut c_void, expected: &Identity) -> Option<(IUnknown, Identity)> {
        let p = self.0.as_ref()?;
        let scope = p.scope.as_ref().map_or(core::ptr::null_mut(), |s| s.as_raw());
        let mut out = core::ptr::null_mut();
        // SAFETY: the live device, the identities' records, the proof's records and a local output.
        let found = unsafe {
            crate::target_lifetime::cvr_tl_borrowed_pageable(device, expected.raw(), p.captured.identity.raw(), &p.captured.source, scope, &p.bound, &mut out)
        };
        if !found || out.is_null() {
            return None;
        }
        // SAFETY: the pageable's one reference.
        Some((unsafe { IUnknown::from_raw(out) }, p.captured.identity.clone()))
    }
}
pub fn lifetime_failure() -> u32 {
    crate::target_lifetime::cvr_tl_last_failure()
}
pub fn lifetime_details() -> [u64; 12] {
    let mut out = [0u64; 12];
    // SAFETY: a local output.
    unsafe { crate::target_lifetime::cvr_tl_last_details(&mut out) };
    out
}

// ---- the target handoff's snapshots

/// target_handoff::TargetSnapshot owned.
#[derive(Clone)]
pub struct TargetSnapshot {
    pub identity: Identity,
    pub view: cvr_gpu::scene_state::TargetInfo,
    pub slot: u32,
    pub planes: u32,
    pub final_states: [u32; 2],
}
/// target_handoff::SourceSnapshot owned.
#[derive(Clone)]
pub struct SourceSnapshot {
    pub recording: OwnedLease,
    pub frame: u32,
    pub side: u32,
    pub targets: [TargetSnapshot; 4],
}
/// The observer's snapshot record (target_handoff_observer.rs' layout: references handed over).
#[repr(C)]
struct RawTarget {
    identity: WritableIdentity,
    identity_data: [usize; 2],
    view: cvr_gpu::scene_state::TargetInfo,
    slot: u32,
    planes: u32,
    final_states: [u32; 2],
}
#[repr(C)]
struct RawSource {
    recording: Lease,
    frame: u32,
    side: u32,
    targets: [RawTarget; 4],
}
const _: () = assert!(core::mem::size_of::<RawTarget>() == 448 && core::mem::size_of::<RawSource>() == 1840);
/// target_handoff::SnapshotSources: both sources' frozen targets, each identity authenticated.
pub fn snapshot_sources(queue: *mut c_void, frame: u32) -> Option<[SourceSnapshot; 2]> {
    // SAFETY: plain data, all zeroes first.
    let mut raw: [RawSource; 2] = unsafe { core::mem::zeroed() };
    // SAFETY: a live queue (or null) and the zeroed records the observer fills.
    if !unsafe { crate::target_handoff_observer::cvr_th2_snapshot(queue, frame, (&raw mut raw).cast()) } {
        return None;
    }
    let [a, b] = raw;
    let own = |s: RawSource| -> SourceSnapshot {
        let RawSource { recording, frame, side, targets } = s;
        SourceSnapshot {
            recording: OwnedLease(recording),
            frame,
            side,
            targets: targets.map(|t| TargetSnapshot { identity: Identity::authenticated(t.identity), view: t.view, slot: t.slot, planes: t.planes, final_states: t.final_states }),
        }
    };
    Some([own(a), own(b)])
}
/// target_handoff::Begin.
pub fn handoff_begin(list: *mut c_void, recording: &Lease, frame: u32, side: u32, targets: *const c_void) -> bool {
    // SAFETY: the live list, the recording and the draw's scene_state::GeometryTargets.
    unsafe { crate::target_handoff_observer::cvr_th2_begin(list, recording, frame, side, targets.cast()) }
}

// ---- the target access (formerly NativeTargetAccess.cpp's Receipt, Query, MarkSource)

fn counters() -> &'static crate::target_access_observer::Counters {
    // SAFETY: the observer's process-wide counters.
    unsafe { &*crate::target_access_observer::cvr_ta_counters() }
}
pub fn observing() -> bool {
    counters().observing.load(core::sync::atomic::Ordering::Acquire)
}
fn unavailable() -> bool {
    counters().unavailable.load(core::sync::atomic::Ordering::Acquire)
}
/// The observer's lock (target_access_observer.rs' journal).
struct Locked;
impl Locked {
    fn new() -> Locked {
        crate::target_access_observer::events::cvr_ta_lock(true);
        Locked
    }
}
impl Drop for Locked {
    fn drop(&mut self) {
        crate::target_access_observer::events::cvr_ta_lock(false);
    }
}
/// A publication of the observer's stats (its sequence odd while it lasts).
struct Publish;
impl Publish {
    fn new() -> Publish {
        crate::declared_moved_exports::CyberpunkVR_TargetAccessSeq.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
        Publish
    }
}
impl Drop for Publish {
    fn drop(&mut self) {
        crate::target_access_observer::abi::cvr_ta_publish_stats();
        crate::declared_moved_exports::CyberpunkVR_TargetAccessSeq.fetch_add(1, core::sync::atomic::Ordering::Release);
    }
}
fn same_lease(a: &Lease, b: &Lease) -> bool {
    !a.token.is_null() && a.token == b.token && a.generation == b.generation && a.context == b.context && a.list == b.list && a.registry == b.registry
}
fn view_bytes(v: &cvr_gpu::scene_state::TargetInfo) -> &[u8] {
    // SAFETY: a plain record's bytes.
    unsafe { core::slice::from_raw_parts((v as *const cvr_gpu::scene_state::TargetInfo).cast::<u8>(), core::mem::size_of::<cvr_gpu::scene_state::TargetInfo>()) }
}
fn same_sources(a: &[SourceSnapshot; 2], b: &[SourceSnapshot; 2]) -> bool {
    a.iter().zip(b).all(|(l, r)| {
        same_lease(&l.recording.0, &r.recording.0)
            && l.frame == r.frame
            && l.side == r.side
            && l.targets.iter().zip(&r.targets).all(|(x, y)| {
                crate::texture_writable::same_identity_metadata(x.identity.value(), y.identity.value())
                    && x.slot == y.slot
                    && x.planes == y.planes
                    && x.final_states == y.final_states
                    && view_bytes(&x.view) == view_bytes(&y.view)
            })
    })
}
fn source_lists(sources: &[SourceSnapshot; 2]) -> [u64; 2] {
    [sources[0].recording.0.list, sources[1].recording.0.list]
}
/// queue_residency::Invocation.
#[derive(Clone, PartialEq, Eq)]
pub struct Invocation {
    pub thread: u64,
    pub queue: u64,
    pub lists_address: u64,
    pub lists: Vec<u64>,
}
/// queue_residency::DescribeInvocation of the scope's proof.
pub fn describe_invocation(scope: &IUnknown) -> Option<Invocation> {
    let mut view = crate::queue_residency::InvocationView { thread: 0, queue: 0, lists_address: 0, lists: core::ptr::null(), count: 0 };
    // SAFETY: the scope's proof and a local output (its lists the proof's, copied below).
    if !unsafe { crate::queue_residency::cvr_qr_describe_invocation(scope.as_raw(), &mut view) } {
        return None;
    }
    // SAFETY: the proof's lists.
    let lists = if view.lists.is_null() || view.count == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(view.lists, view.count) }.to_vec() };
    Some(Invocation { thread: view.thread, queue: view.queue, lists_address: view.lists_address, lists })
}
fn contains_recording(scope: &IUnknown, recording: &Lease) -> bool {
    // SAFETY: the scope's proof and the lease.
    unsafe { crate::queue_residency::cvr_qr_contains_recording_runtime(scope.as_raw(), recording) }
}
fn thread_id() -> u64 {
    // SAFETY: a plain query.
    u64::from(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() })
}
fn receipt_boundary(boundary: &crate::target_access::Boundary, invocation: &Invocation, lists: &[u64; 2]) -> bool {
    crate::target_access::receipt_boundary(boundary, invocation.thread, invocation.queue, invocation.lists_address, &invocation.lists, lists)
}
/// The receipt status of these sources (the journal locked by the caller).
fn status_locked(queue: *mut c_void, frame: u32, scope: &IUnknown, sources: &[SourceSnapshot; 2]) -> crate::target_access::ReceiptStatus {
    let lists = source_lists(sources);
    // SAFETY: plain data, all zeroes first.
    let mut status: crate::target_access::ReceiptStatus = unsafe { core::mem::zeroed() };
    // SAFETY: a local output.
    unsafe { crate::target_access_observer::abi::cvr_ta_receipt_status(queue as u64, thread_id(), scope.as_raw() as u64, frame, lists[0], lists[1], &mut status) };
    status
}
fn observed(identity: &Identity) -> crate::target_access::Resource {
    let c = &identity.value().creation;
    crate::target_access::Resource {
        pointer: c.resource.as_ref().map_or(0, |r| r.as_raw() as u64),
        allocation: crate::texture_read::Allocation { heap: c.heap.as_ref().map_or(0, |h| h.as_raw() as u64), offset: c.offset, bytes: c.bytes, kind: c.kind },
        flags: identity.value().resource_desc.Flags.0 as u32,
    }
}
/// target_access::MarkSource.
pub fn mark_source(list: *mut c_void, lease: &Lease, frame: u32, side: u32) -> bool {
    if !observing() {
        return false;
    }
    let _lock = Locked::new();
    if !observing() {
        return false;
    }
    let _publish = Publish::new();
    crate::target_access_observer::abi::cvr_ta_mark_source(list as u64, lease.token as u64, lease.generation, lease.context, lease.list, lease.registry, frame, side)
}
/// The access check's context: the queue and the sources whose identities it validates.
struct QueryContext {
    queue: *mut c_void,
    sources: *const [SourceSnapshot; 2],
}
unsafe extern "C-unwind" fn validate_target(ctx: *mut c_void, side: u32, target: u32) -> bool {
    // SAFETY: the context of the call below, alive for it.
    let c = unsafe { &*ctx.cast::<QueryContext>() };
    // SAFETY: as above.
    let sources = unsafe { &*c.sources };
    sources[side as usize].targets[target as usize].identity.validate(c.queue)
}
/// target_access::Query (the access window's check of both sources' targets at the boundary).
pub fn query(queue: *mut c_void, sources: &[SourceSnapshot; 2], boundary: &crate::target_access::Boundary) -> bool {
    if !observing() && !unavailable() {
        return false;
    }
    let _lock = Locked::new();
    if !observing() && !unavailable() {
        return false;
    }
    let _publish = Publish::new();
    let flat: [crate::target_access_observer::QuerySource; 2] = core::array::from_fn(|side| {
        let s = &sources[side];
        let l = &s.recording.0;
        crate::target_access_observer::QuerySource {
            token: l.token as u64,
            generation: l.generation,
            context: l.context,
            list: l.list,
            registry: l.registry,
            frame: s.frame,
            side: s.side,
            targets: core::array::from_fn(|t| {
                let target = &s.targets[t];
                let own = &target.identity.value().recording;
                crate::target_access_observer::QueryTarget {
                    observed: observed(&target.identity),
                    planes: target.planes,
                    expected: target.identity.value().expected_writable_state.0 as u32,
                    finals: target.final_states,
                    token: own.token as u64,
                    generation: own.generation,
                    context: own.context,
                    list: own.list,
                    registry: own.registry,
                }
            }),
        }
    });
    let mut context = QueryContext { queue, sources };
    // SAFETY: the queue, the flat sources, the boundary and the context (alive for the call).
    unsafe { crate::target_access_observer::abi::cvr_ta_query(queue, &flat, boundary, validate_target, (&raw mut context).cast()) }
}
/// target_access::Receipt.
pub struct Receipt {
    pub sources: [SourceSnapshot; 2],
    queue: windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue,
    boundary: crate::target_access::Boundary,
    scope: IUnknown,
    invocation: Invocation,
    key: crate::target_access::ReceiptKey,
}
impl Receipt {
    /// Receipt::Data::Live.
    pub fn live(&self, queue: *mut c_void, frame: u32) -> bool {
        if queue.is_null() || queue != self.queue.as_raw() || frame == 0 || frame != self.key.frame || thread_id() != self.key.thread {
            return false;
        }
        {
            let _lock = Locked::new();
            if !crate::target_access::receipt_matches(&self.key, &status_locked(queue, frame, &self.scope, &self.sources)) {
                return false;
            }
        }
        let Some(current) = describe_invocation(&self.scope) else { return false };
        if current != self.invocation || !receipt_boundary(&self.boundary, &current, &source_lists(&self.sources)) {
            return false;
        }
        if !self.sources.iter().all(|s| s.targets.iter().all(|t| t.identity.validate(core::ptr::null_mut()))) {
            return false;
        }
        // SAFETY: the scope's proof.
        if !unsafe { crate::queue_residency::cvr_qr_validate(self.scope.as_raw()) } {
            return false;
        }
        let _lock = Locked::new();
        crate::target_access::receipt_matches(&self.key, &status_locked(queue, frame, &self.scope, &self.sources))
    }
    /// target_access::Describe: the sources while the receipt stays live.
    pub fn describe(&self, queue: *mut c_void, frame: u32) -> Option<[SourceSnapshot; 2]> {
        if !self.live(queue, frame) {
            return None;
        }
        let next = self.sources.clone();
        self.live(queue, frame).then_some(next)
    }
}
/// target_access::Query with a receipt: the access window checked between two authenticated snapshots of the same
/// invocation within the queue's residency scope.
pub fn query_receipt(queue: *mut c_void, supplied: &[SourceSnapshot; 2], boundary: &crate::target_access::Boundary, scope: &IUnknown) -> Option<Receipt> {
    let frame = supplied[0].frame;
    if queue.is_null() || frame == 0 || supplied[1].frame != frame {
        return None;
    }
    let invocation = describe_invocation(scope)?;
    if invocation.queue != queue as u64 || invocation.thread != thread_id() || !receipt_boundary(boundary, &invocation, &source_lists(supplied)) {
        return None;
    }
    let authenticated = snapshot_sources(queue, frame)?;
    if !same_sources(supplied, &authenticated) || !authenticated.iter().all(|s| contains_recording(scope, &s.recording.0)) {
        return None;
    }
    let before = {
        let _lock = Locked::new();
        let state = status_locked(queue, frame, scope, &authenticated);
        if !crate::target_access::receipt_matches(&state.key, &state) {
            return None;
        }
        state.key
    };
    if !query(queue, &authenticated, boundary) {
        return None;
    }
    if describe_invocation(scope).as_ref() != Some(&invocation) {
        return None;
    }
    let confirmed = snapshot_sources(queue, frame)?;
    if !same_sources(&authenticated, &confirmed) || !confirmed.iter().all(|s| contains_recording(scope, &s.recording.0)) {
        return None;
    }
    let raw = queue;
    // SAFETY: the caller's live queue.
    let queue_ref = unsafe { windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue::from_raw_borrowed(&raw) }.cloned()?;
    let receipt = Receipt { sources: confirmed, queue: queue_ref, boundary: *boundary, scope: scope.clone(), invocation, key: before };
    {
        let _lock = Locked::new();
        if !crate::target_access::receipt_matches(&before, &status_locked(queue, frame, scope, &receipt.sources)) {
            return None;
        }
    }
    receipt.live(queue, frame).then_some(receipt)
}
