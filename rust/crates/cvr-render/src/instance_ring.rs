//! The C ABI of the native instance ring ledger (native::instance_ring) for include/Render/NativeInstanceRing.hpp and
//! its C++ face src/Render/NativeInstanceRing.cpp: what the single-pass observer knows of the engine's per-instance
//! upload rings -- each ring's numeric observation, the writers that reserved space in it, the CPU bytes a genuine
//! native writer left behind (copied after it returned), and the native {stamp, head} publications. The ledger is an
//! owned handle here; a completed version also carries the C++ face's presence token (its shared_ptr, which the face's
//! weak_ptrs watch), dropped with the version; a query hands over one reference to the shared payload.
//! tools/single_pass_tests/native_instance_ring_test and instance_ring_concurrent_test run their cases against these
//! in their Rust variants.
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering::*};
use std::sync::Arc;

pub use crate::native::instance_ring::{
    instance_range, CompletedWriter, Ledger, Limits, Payload, PresenceToken, Publication, Range,
    ReadRequest, RingObservation, Scope, Status, WriterTicket,
};

// ---- the C++ half's presence tokens (std::shared_ptr<const WriterTicket>, held here) ----------------------------------

static MAKE_PRESENCE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static DROP_PRESENCE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// The C++ face's presence callbacks: a new shared_ptr<const WriterTicket> owned by the ledger, and its release.
#[no_mangle]
pub extern "C" fn cvr_instance_ring_attach(
    make: extern "C" fn(*const WriterTicket) -> *mut c_void,
    drop: extern "C" fn(*mut c_void),
) {
    MAKE_PRESENCE.store(make as *mut c_void, Release);
    DROP_PRESENCE.store(drop as *mut c_void, Release);
}
/// A completed writer's cache membership: expires (the C++ weak_ptr) when the ledger drops it.
struct Presence(*mut c_void);
// SAFETY: a std::shared_ptr control block, released through the thread-safe attached callback.
unsafe impl Send for Presence {}
impl Presence {
    fn new(ticket: &WriterTicket) -> Option<Self> {
        let make = MAKE_PRESENCE.load(Acquire);
        if make.is_null() {
            return None;
        }
        // SAFETY: the attached callback.
        let make: extern "C" fn(*const WriterTicket) -> *mut c_void =
            unsafe { core::mem::transmute(make) };
        let handle = make(ticket);
        (!handle.is_null()).then_some(Self(handle))
    }
}
impl Drop for Presence {
    fn drop(&mut self) {
        let drop = DROP_PRESENCE.load(Acquire);
        if !drop.is_null() {
            // SAFETY: the attached callback, releasing the handle it made.
            let drop: extern "C" fn(*mut c_void) = unsafe { core::mem::transmute(drop) };
            drop(self.0);
        }
    }
}

/// The ledger's presence hook: the C++ face's token for a newly completed version.
fn presence_token(ticket: &WriterTicket) -> Option<PresenceToken> {
    Presence::new(ticket).map(|presence| Box::new(presence) as PresenceToken)
}

const _: () = assert!(
    core::mem::size_of::<RingObservation>() == 120
        && core::mem::size_of::<Scope>() == 56
        && core::mem::size_of::<WriterTicket>() == 88
        && core::mem::size_of::<CompletedWriter>() == 104
        && core::mem::size_of::<Publication>() == 64
        && core::mem::size_of::<ReadRequest>() == 160
        && core::mem::size_of::<Limits>() == 48
);

/// What Query hands the C++ face: the payload (one reference, handed over), its producer, the subrange and the
/// publication.
#[repr(C)]
pub struct QueryOut {
    pub payload: *const Payload,
    pub producer: CompletedWriter,
    pub offset: usize,
    pub bytes: usize,
    pub publication: Publication,
}
impl Default for QueryOut {
    fn default() -> Self {
        Self {
            payload: core::ptr::null(),
            producer: CompletedWriter::default(),
            offset: 0,
            bytes: 0,
            publication: Publication::default(),
        }
    }
}
const _: () = assert!(core::mem::size_of::<QueryOut>() == 192);

// ---- the C ABI (the C++ face, include/Render/NativeInstanceRing.hpp) ----------------------------------------------

fn ledger<'a>(p: *mut c_void) -> &'a mut Ledger {
    // SAFETY: a ledger from cvr_instance_ring_new, used by its one caller at a time.
    unsafe { &mut *(p as *mut Ledger) }
}
fn r<'a, T>(p: *const T) -> &'a T {
    // SAFETY: the C++ face's live argument.
    unsafe { &*p }
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_new(limits: *const Limits) -> *mut c_void {
    let mut ledger = Ledger::new(*r(limits));
    ledger.set_presence_hook(presence_token);
    Box::into_raw(Box::new(ledger)).cast()
}
/// # Safety
/// `p` from cvr_instance_ring_new, freed once.
#[no_mangle]
pub unsafe extern "C" fn cvr_instance_ring_free(p: *mut c_void) {
    if !p.is_null() {
        // SAFETY: as the caller promises.
        drop(unsafe { Box::from_raw(p as *mut Ledger) });
    }
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_range(
    a: *const RingObservation,
    absolute: u64,
    first: u32,
    instances: u32,
    out: *mut Range,
) -> bool {
    let range = instance_range(r(a), absolute, first, instances);
    // SAFETY: the caller's output.
    unsafe { *out = range.unwrap_or_default() };
    range.is_some()
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_observe(p: *mut c_void, a: *const RingObservation) -> Status {
    ledger(p).observe(r(a))
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_begin(
    p: *mut c_void,
    a: *const RingObservation,
    scope: *const Scope,
    out: *mut WriterTicket,
) -> Status {
    let (status, ticket) = ledger(p).begin(r(a), r(scope));
    // SAFETY: the caller's output.
    unsafe { *out = ticket };
    status
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_qualify_write(
    p: *mut c_void,
    a: *const RingObservation,
    ticket: *const WriterTicket,
    absolute: u64,
    first: u32,
    instances: u32,
) -> Status {
    ledger(p).qualify_write(r(a), r(ticket), absolute, first, instances)
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_validate_write(
    p: *mut c_void,
    ticket: *const WriterTicket,
) -> Status {
    ledger(p).validate_write(r(ticket))
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_discard_completed(
    p: *mut c_void,
    ticket: *const WriterTicket,
) -> Status {
    ledger(p).discard_completed(r(ticket))
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_invalidate_physical(
    p: *mut c_void,
    a: *const RingObservation,
    first: u32,
    instances: u32,
) -> Status {
    ledger(p).invalidate_physical(r(a), first, instances)
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_finish_unobserved(
    p: *mut c_void,
    a: *const RingObservation,
    ticket: *const WriterTicket,
    first: u32,
    instances: u32,
) -> Status {
    ledger(p).finish_unobserved(r(a), r(ticket), first, instances)
}
/// # Safety
/// `bytes` holds `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_instance_ring_complete(
    p: *mut c_void,
    a: *const RingObservation,
    completed: *const CompletedWriter,
    bytes: *const u8,
    len: usize,
) -> Status {
    // SAFETY: the caller's bytes.
    let bytes = if bytes.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(bytes, len) }
    };
    ledger(p).complete(r(a), r(completed), bytes)
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_publish(
    p: *mut c_void,
    a: *const RingObservation,
    publication: *const Publication,
) -> Status {
    ledger(p).publish(r(a), r(publication))
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_restart_observation(
    p: *mut c_void,
    a: *const RingObservation,
) -> Status {
    ledger(p).restart_observation(r(a))
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_reuse(
    p: *mut c_void,
    a: *const RingObservation,
    absolute: u64,
    instances: u32,
    active: *const WriterTicket,
) -> Status {
    // SAFETY: the caller's optional writer.
    ledger(p).reuse(r(a), absolute, instances, unsafe { active.as_ref() })
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_unknown(p: *mut c_void, ring: u64) {
    ledger(p).unknown(ring);
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_destroy(p: *mut c_void, ring: u64) {
    ledger(p).destroy(ring);
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_reset(p: *mut c_void) {
    ledger(p).reset();
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_query(
    p: *mut c_void,
    a: *const RingObservation,
    request: *const ReadRequest,
    out: *mut QueryOut,
) -> Status {
    let (status, candidate) = ledger(p).query(r(a), r(request));
    let publication = *candidate.published();
    let (payload, offset, bytes) = candidate.into_payload();
    // SAFETY: the caller's output.
    unsafe {
        *out = QueryOut {
            producer: payload
                .as_ref()
                .map_or_else(CompletedWriter::default, |data| data.producer()),
            payload: payload.map_or(core::ptr::null(), Arc::into_raw),
            offset,
            bytes,
            publication,
        }
    };
    status
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_presence(
    p: *mut c_void,
    ticket: *const WriterTicket,
) -> *mut c_void {
    ledger(p)
        .presence_token(r(ticket))
        .and_then(|token| token.downcast_ref::<Presence>())
        .map_or(core::ptr::null_mut(), |presence| presence.0)
}
#[no_mangle]
pub extern "C" fn cvr_instance_ring_live_cpu_bytes(p: *mut c_void) -> usize {
    ledger(p).live_cpu_bytes()
}
/// One more reference to a payload.
///
/// # Safety
/// `payload` came from cvr_instance_ring_query and is still referenced.
#[no_mangle]
pub unsafe extern "C" fn cvr_instance_ring_payload_retain(payload: *const Payload) {
    if !payload.is_null() {
        // SAFETY: as the caller promises.
        unsafe { Arc::increment_strong_count(payload) };
    }
}
/// # Safety
/// `payload` carries one reference of the caller's, released once.
#[no_mangle]
pub unsafe extern "C" fn cvr_instance_ring_payload_release(payload: *const Payload) {
    if !payload.is_null() {
        // SAFETY: as the caller promises.
        unsafe { Arc::decrement_strong_count(payload) };
    }
}
/// The payload's bytes (alive while it is referenced).
///
/// # Safety
/// `payload` is referenced by the caller.
#[no_mangle]
pub unsafe extern "C" fn cvr_instance_ring_payload_bytes(
    payload: *const Payload,
    len: *mut usize,
) -> *const u8 {
    // SAFETY: as the caller promises.
    let Some(p) = (unsafe { payload.as_ref() }) else {
        // SAFETY: the caller's output.
        unsafe { *len = 0 };
        return core::ptr::null();
    };
    // SAFETY: the caller's output.
    unsafe { *len = p.bytes().len() };
    p.bytes().as_ptr()
}
