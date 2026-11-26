//! THE NATIVE CONSTANT CAPTURE'S STATE (formerly src/Render/NativeConstantCapture.cpp's): the capture's lock, its readers
//! (SetReader), its switch while the probe records (Begin, End), a draw's snapshot (the read position marked and traced,
//! the family's CBV uses through the .cpp's host, constant_ledger.rs' step, the family held), the submissions'
//! validation (BeforeSubmit), a reset list's draws (Reset), the poll's part -- the registered trace's collection while
//! the capture is requested and the deferred bindings' resolution -- and the published rows.
//!
//! The .cpp lends the exported rows, the descriptors' current CBV and the families (a family's CBV uses at a draw, a
//! held family and its release), and keeps the pair's copy and the bank's pass under this lock (`cvr_ncc2_lock`).
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::*};

use windows::Win32::System::Threading::{AcquireSRWLockExclusive, ReleaseSRWLockExclusive, SRWLOCK};

use crate::constant_ledger::{self as ledger, CbvUse, Ledger, Read, SnapshotDraw, Validate, INTERN_REFUSED, THREW};
use crate::constant_timeline::Resolved;
use crate::registered_copies::{self as registered, op};

/// native_probe::DrawCapacity.
const DRAW_CAPACITY: usize = 4096;

/// MarkPosition: the read's position at the list (0: none).
pub type Mark = unsafe extern "C" fn(*mut c_void) -> u64;
/// ResolvePosition: the timeline's result at the read's position.
pub type ResolvePosition = unsafe extern "C" fn(u64, *mut c_void, u64, *mut c_void, usize) -> Resolved;
/// The family's CBV uses' sink.
pub type PushUse = unsafe extern "C" fn(*mut c_void, *const CbvUse);

/// The .cpp's host (static storage, attached at its static initialisation).
#[repr(C)]
pub struct Host {
    pub enable: *const AtomicU32,
    pub seq: *const AtomicU32,
    pub failure: *const AtomicU32,
    pub draw_count: *const AtomicU32,
    pub binding_count: *const AtomicU32,
    pub payload_count: *const AtomicU32,
    pub blob_bytes: *const AtomicU32,
    pub pending_lists: *const AtomicU32,
    pub draws: *mut u64,
    pub bindings: *mut u64,
    pub payloads: *mut u64,
    pub blob: *mut u64,
    /// descriptors::ResolveCbvAddress: the descriptor's CBV now (heap, address, bytes).
    pub resolve_cbv: unsafe extern "C" fn(u64, *mut u64, *mut u64, *mut u32) -> bool,
    /// The family's CBV uses at the draw (list, root) in its order, located in its paired plan and resolved
    /// (descriptors::ResolveCbv); false: it threw.
    pub uses: unsafe extern "C" fn(*const c_void, u64, u64, *mut c_void, PushUse) -> bool,
    /// A held copy of the family (null: it threw) and its release.
    pub hold: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub release: unsafe extern "C" fn(*mut c_void),
}
const _: () = assert!(core::mem::size_of::<Host>() == 16 * 8);
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_attach(host: *const Host) {
    HOST.store(host.cast_mut(), Release);
}
fn host() -> Option<&'static Host> {
    // SAFETY: attached once with static storage.
    let attached = unsafe { HOST.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::HOST));
    attached
}
fn word(value: *const AtomicU32) -> &'static AtomicU32 {
    // SAFETY: the lent exported words live for the process.
    unsafe { &*value }
}

struct Lock(UnsafeCell<SRWLOCK>);
// SAFETY: an SRW lock, used only through its acquire and release.
unsafe impl Sync for Lock {}
static LOCK: Lock = Lock(UnsafeCell::new(SRWLOCK { Ptr: null_mut() }));
fn raw_lock(on: bool) {
    // SAFETY: the static lock; acquire and release pair up on the calling thread.
    unsafe {
        if on {
            AcquireSRWLockExclusive(LOCK.0.get());
        } else {
            ReleaseSRWLockExclusive(LOCK.0.get());
        }
    }
}
/// The capture's lock (the .cpp's pair copy holds it): `true` acquires, `false` releases.
#[no_mangle]
pub extern "C-unwind" fn cvr_ncc2_lock(on: bool) {
    raw_lock(on);
}
struct Locked;
impl Drop for Locked {
    fn drop(&mut self) {
        raw_lock(false);
    }
}
fn lock() -> Locked {
    raw_lock(true);
    Locked
}

struct State {
    reader: Option<Read>,
    validator: Option<Validate>,
    mark: Option<Mark>,
    resolve: Option<ResolvePosition>,
    active: bool,
    ledger: Option<Ledger>,
    /// The snapshotted draws' families (held copies), in the ledger's draw order.
    owners: Vec<Owner>,
}
/// A snapshotted draw's family: the .cpp's held copy (the Rust test variants' Snapshot) or the registry's (the plugin's).
enum Owner {
    Cpp(*mut c_void),
    #[cfg(any(feature = "rust-build", feature = "probe-exports"))]
    Rust(std::sync::Arc<crate::families_native::Family>),
}
impl Owner {
    /// The family as the .cpp reads it (a std::shared_ptr<const Family>*).
    fn shared(&self) -> *const c_void {
        match self {
            Owner::Cpp(held) => held.cast_const(),
            #[cfg(any(feature = "rust-build", feature = "probe-exports"))]
            Owner::Rust(family) => family.mirror(),
        }
    }
}
#[allow(irrefutable_let_patterns)] // the C++ build's Rust (no Rust families) has only the .cpp's
fn release_owner(h: &Host, owner: Owner) {
    if let Owner::Cpp(held) = owner {
        // SAFETY: a held family.
        unsafe { (h.release)(held) };
    }
}
/// The plugin's own host (formerly lent by src/Render/NativeConstantCapture.cpp): the exports Rust defines and the
/// descriptors' current CBV; its families are the registry's (cvr_ncc2_snapshot_draw). The Rust test variants attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::{CbvUse, Host, PushUse};
    use crate::native_state_exports::*;
    use core::ffi::c_void;
    unsafe extern "C" fn resolve_cbv(descriptor: u64, heap: *mut u64, address: *mut u64, bytes: *mut u32) -> bool {
        // descriptors::ResolveCbvAddress.
        let Some(now) = crate::descriptor_audit::resolve_cbv_address(descriptor) else { return false };
        // SAFETY: the caller's outputs.
        unsafe {
            *heap = now.heap;
            *address = now.address;
            *bytes = now.bytes;
        }
        true
    }
    unsafe extern "C" fn no_uses(_: *const c_void, _: u64, _: u64, _: *mut c_void, _: PushUse) -> bool {
        false
    }
    unsafe extern "C" fn no_hold(_: *const c_void) -> *mut c_void {
        core::ptr::null_mut()
    }
    unsafe extern "C" fn no_release(_: *mut c_void) {}
    // SAFETY: immutable; its pointers name process-lifetime exports.
    unsafe impl Sync for Host {}
    pub static HOST: Host = Host {
        enable: &raw const CyberpunkVR_ConstantCaptureEnable,
        seq: &raw const CyberpunkVR_ConstantCaptureSeq,
        failure: &raw const CyberpunkVR_ConstantCaptureFailure,
        draw_count: &raw const CyberpunkVR_ConstantDrawCount,
        binding_count: &raw const CyberpunkVR_ConstantBindingCount,
        payload_count: &raw const CyberpunkVR_ConstantPayloadCount,
        blob_bytes: &raw const CyberpunkVR_ConstantBlobBytes,
        pending_lists: &raw const CyberpunkVR_ConstantPendingLists,
        draws: &raw mut CyberpunkVR_ConstantDraws,
        bindings: &raw mut CyberpunkVR_ConstantBindings,
        payloads: &raw mut CyberpunkVR_ConstantPayloads,
        blob: &raw mut CyberpunkVR_ConstantBlob,
        resolve_cbv,
        uses: no_uses,
        hold: no_hold,
        release: no_release,
    };
    const _: () = assert!(core::mem::size_of::<CbvUse>() == 56);
}
struct StateCell(UnsafeCell<State>);
// SAFETY: touched only under LOCK.
unsafe impl Sync for StateCell {}
static STATE: StateCell =
    StateCell(UnsafeCell::new(State { reader: None, validator: None, mark: None, resolve: None, active: false, ledger: None, owners: Vec::new() }));
/// The poll's collection of the registered trace while the capture is requested.
static COLLECTING: AtomicBool = AtomicBool::new(false);
fn state(_: &Locked) -> &'static mut State {
    // SAFETY: under the lock.
    unsafe { &mut *STATE.0.get() }
}
fn ledger_of(ledger: &mut Option<Ledger>) -> &mut Ledger {
    ledger.get_or_insert_with(Ledger::new)
}

/// Publish: the ledger's rows and counts, the lists still pending (and one for the deferred bindings).
fn publish(h: &Host, l: &Ledger) {
    // SAFETY: the lent exported rows.
    unsafe {
        *h.draws = l.draws.as_ptr() as u64;
        word(h.draw_count).store(l.draws.len() as u32, SeqCst);
        *h.bindings = l.bindings.as_ptr() as u64;
        word(h.binding_count).store(l.bindings.len() as u32, SeqCst);
        *h.payloads = l.payloads.as_ptr() as u64;
        word(h.payload_count).store(l.payloads.len() as u32, SeqCst);
        *h.blob = l.blob.as_ptr() as u64;
        word(h.blob_bytes).store(l.blob.len() as u32, SeqCst);
    }
    word(h.pending_lists).store(l.pending_lists() as u32 + u32::from(l.deferred != 0), SeqCst);
}

unsafe extern "C-unwind" fn resolve_cbv(_: *mut c_void, descriptor: u64, heap: *mut u64, address: *mut u64, bytes: *mut u32) -> bool {
    // SAFETY: the ledger's outputs.
    host().is_some_and(|h| unsafe { (h.resolve_cbv)(descriptor, heap, address, bytes) })
}
/// RegisteredReadBinding.
unsafe extern "C-unwind" fn register_binding(_: *mut c_void, position: u64, visibility: u32, shader_register: u32, address: u64, bytes: u32) {
    registered::trace(op::BINDING, &[position, visibility.into(), shader_register.into(), address, bytes.into()]);
}
/// The resolver (`user`: the State's resolver copied for the step).
unsafe extern "C-unwind" fn resolve_position(user: *mut c_void, position: u64, list: u64, address: u64, out: *mut c_void, bytes: usize, detail: *mut u32) -> u32 {
    // SAFETY: the step's resolver.
    let Some(resolve) = (unsafe { *user.cast::<Option<ResolvePosition>>() }) else { return 1 };
    // SAFETY: the ledger's output and the caller's resolver.
    let result = unsafe { resolve(position, list as *mut c_void, address, out, bytes) };
    // SAFETY: as documented.
    unsafe { *detail = result.detail as u32 };
    result.status as u32
}
/// The ledger's host over the readers; `resolver` the state's resolver kept for the step.
fn ledger_host(reader: Option<Read>, validator: Option<Validate>, resolver: &Option<ResolvePosition>) -> ledger::Host {
    ledger::Host {
        user: (resolver as *const Option<ResolvePosition>).cast_mut().cast(),
        read: reader,
        validate: validator,
        resolve: resolve_cbv,
        register: register_binding,
        resolve_position: resolver.map(|_| resolve_position as ledger::ResolvePosition),
    }
}

/// SetReader (the readers' addresses, null: none).
///
/// # Safety
/// Each pointer is null or the function its type names.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_set_reader(read: *const c_void, validate: *const c_void, mark: *const c_void, resolve: *const c_void) {
    let l = lock();
    let s = state(&l);
    // SAFETY: as documented (a null pointer is None).
    unsafe {
        s.reader = core::mem::transmute::<*const c_void, Option<Read>>(read);
        s.validator = core::mem::transmute::<*const c_void, Option<Validate>>(validate);
        s.mark = core::mem::transmute::<*const c_void, Option<Mark>>(mark);
        s.resolve = core::mem::transmute::<*const c_void, Option<ResolvePosition>>(resolve);
    }
}
/// Begin: the ledger cleared and the families released; active while the capture is enabled.
#[no_mangle]
pub extern "C-unwind" fn cvr_ncc2_begin() {
    let Some(h) = host() else { return };
    let l = lock();
    let s = state(&l);
    word(h.seq).fetch_add(1, SeqCst);
    ledger_of(&mut s.ledger).begin();
    for owner in s.owners.drain(..) {
        release_owner(h, owner);
    }
    word(h.failure).store(0, SeqCst);
    s.active = word(h.enable).load(SeqCst) != 0;
    publish(h, ledger_of(&mut s.ledger));
    word(h.seq).fetch_add(1, SeqCst);
}
/// A snapshotted draw's source.
#[repr(C)]
pub struct Source {
    pub family: u64,
    pub list: u64,
    pub root: u64,
    pub sequence: u64,
    pub side: u32,
    pub frame: u32,
}
const _: () = assert!(core::mem::size_of::<Source>() == 40);
unsafe extern "C" fn push_use(sink: *mut c_void, value: *const CbvUse) {
    // SAFETY: snapshot's vector and the .cpp's use.
    unsafe { (*sink.cast::<Vec<CbvUse>>()).push(*value) };
}
/// Snapshot: one draw's CBVs (while active and not failed); a failed step fails the capture (2) and deactivates it.
///
/// # Safety
/// `source` is valid; `family` the .cpp's family (its shared pointer) or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_snapshot(source: *const Source, family: *const c_void) {
    if family.is_null() {
        return;
    }
    // SAFETY: as documented.
    snapshot_with(unsafe { &*source }, Family::Cpp(family));
}
/// The family a snapshot reads: the .cpp's (its uses and hold through the host) or the registry's.
enum Family {
    Cpp(*const c_void),
    #[cfg(any(feature = "rust-build", feature = "probe-exports"))]
    Rust(std::sync::Arc<crate::families_native::Family>),
}
/// The registry family's CBV uses at the draw (list, root) in its order: located in its paired root's plan (a table
/// source, space 0, one descriptor) and resolved (descriptors::ResolveCbv).
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
fn rust_uses(family: &crate::families_native::Family, list: u64, root: u64) -> Vec<CbvUse> {
    let plan = &family.paired_root.plan;
    let mut out = Vec::new();
    for u in family.uses.iter().filter(|u| u.kind == 2) {
        // SAFETY: a plain record; all zeroes its default.
        let mut v: CbvUse = unsafe { core::mem::zeroed() };
        v.visibility = u.visibility;
        v.shader_register = u.shader_register;
        v.expected = u.bytes;
        let location = plan.bindings.iter().find(|p| {
            p.binding.kind.0 as u32 == u.kind && p.binding.visibility.0 as u32 == u.visibility && u.space == 0 && u.count == 1 && p.binding.source_register == u.shader_register
        });
        if let Some(l) = location.filter(|l| l.source_type == windows::Win32::Graphics::Direct3D12::D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE) {
            v.state = 1;
            v.parameter = l.source_parameter;
            v.offset = l.source_offset;
            if let Some(cbv) = crate::descriptor_audit::resolve_cbv(list, root, v.parameter, v.offset) {
                v.state = 2;
                v.descriptor = cbv.descriptor;
                v.heap = cbv.heap;
                v.address = cbv.address;
                v.bytes = cbv.bytes;
            }
        }
        out.push(v);
    }
    out
}
/// CopyConstantPair for the plugin's constant bank (constant_bank.rs): the pair's registry family (single CBVs only) and
/// each paired binding's two eyes' payload copies, in the plan's order.
#[cfg(all(feature = "rust-build", not(test)))]
pub(crate) fn copy_pair(left: u32, right: u32) -> Option<(std::sync::Arc<crate::families_native::Family>, Vec<[Vec<u8>; 2]>)> {
    let h = host()?;
    let l = lock();
    if word(h.failure).load(SeqCst) != 0 || left == right {
        return None;
    }
    let s = state(&l);
    let at = ledger_of(&mut s.ledger).pair(left, right)?;
    let family = match s.owners.get(at[0])? {
        Owner::Rust(family) => family.clone(),
        Owner::Cpp(_) => return None,
    };
    // SAFETY: the family's uses.
    if !unsafe { crate::constant_capture_checks::cvr_ncc_single_cbvs(family.uses.as_ptr().cast(), family.uses.len()) } {
        return None;
    }
    let ledger = ledger_of(&mut s.ledger);
    let mut out = Vec::with_capacity(family.paired_root.plan.bindings.len());
    for b in &family.paired_root.plan.bindings {
        let mut eyes: [Vec<u8>; 2] = Default::default();
        for (eye, bytes) in eyes.iter_mut().enumerate() {
            *bytes = ledger.payload(at[eye], b.source_parameter, b.source_offset, b.binding.visibility.0 as u32, b.binding.source_register)?.to_vec();
        }
        out.push(eyes);
    }
    Some((family, out))
}
/// The native probe's snapshot (formerly constants::Snapshot with families::Bound): the draw's bound family from the
/// registry, its CBVs snapshotted.
///
/// # Safety
/// `draw` is the probe's record.
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_snapshot_draw(draw: *const crate::geometry_commands::DrawRecord) {
    // SAFETY: as documented.
    let d = unsafe { &*draw };
    let Some(family) = crate::families_native::bound(d.list as *mut c_void) else { return };
    let src = Source { family: family.id, list: d.list, root: d.root, sequence: d.sequence, side: d.side as u32, frame: d.frame };
    snapshot_with(&src, Family::Rust(family));
}
fn snapshot_with(src: &Source, family: Family) {
    let Some(h) = host() else { return };
    if word(h.enable).load(Relaxed) == 0 {
        return;
    }
    let l = lock();
    let s = state(&l);
    if !s.active || word(h.failure).load(SeqCst) != 0 {
        return;
    }
    word(h.seq).fetch_add(1, SeqCst);
    let State { reader, validator, mark, resolve, active, ledger, owners } = s;
    let ledger = ledger_of(ledger);
    let completed = (|| {
        if ledger.draws.len() >= DRAW_CAPACITY {
            return false;
        }
        let sequence = src.sequence as u32;
        // SAFETY: the reader's mark of a live list.
        let position = mark.map_or(0, |mark| unsafe { mark(src.list as *mut c_void) });
        if mark.is_some() && position == 0 {
            return false;
        }
        if position != 0 {
            registered::trace(op::DRAW, &[position, sequence.into(), src.family, src.side.into(), src.frame.into()]);
        }
        let uses: Vec<CbvUse> = match &family {
            Family::Cpp(cpp) => {
                let mut uses: Vec<CbvUse> = Vec::new();
                // SAFETY: the .cpp's family and this vector's sink.
                if !unsafe { (h.uses)(*cpp, src.list, src.root, (&raw mut uses).cast(), push_use) } {
                    return false;
                }
                uses
            }
            #[cfg(any(feature = "rust-build", feature = "probe-exports"))]
            Family::Rust(f) => rust_uses(f, src.list, src.root),
        };
        let draw = SnapshotDraw { family: src.family, list: src.list, position, sequence, side: src.side, frame: src.frame, _pad: 0 };
        let resolver = *resolve;
        let lh = ledger_host(*reader, *validator, &resolver);
        // SAFETY: the readers and the host's callbacks.
        let asks = unsafe { ledger.snapshot(&draw, &uses, &lh, DRAW_CAPACITY) };
        if asks & INTERN_REFUSED != 0 {
            word(h.failure).store(1, SeqCst);
        }
        if asks & THREW != 0 {
            word(h.failure).store(2, SeqCst);
            *active = false;
            return true;
        }
        match &family {
            Family::Cpp(cpp) => {
                // SAFETY: the .cpp's family.
                let held = unsafe { (h.hold)(*cpp) };
                if held.is_null() {
                    return false;
                }
                owners.push(Owner::Cpp(held));
            }
            #[cfg(any(feature = "rust-build", feature = "probe-exports"))]
            Family::Rust(f) => owners.push(Owner::Rust(f.clone())),
        }
        true
    })();
    if !completed {
        word(h.failure).store(2, SeqCst);
        *active = false;
    }
    publish(h, ledger);
    word(h.seq).fetch_add(1, SeqCst);
}
/// End: inactive; with a read position reader and nothing pending, the registered reads freeze.
#[no_mangle]
pub extern "C-unwind" fn cvr_ncc2_end() {
    let Some(h) = host() else { return };
    let l = lock();
    let s = state(&l);
    s.active = false;
    if s.mark.is_some() && word(h.enable).load(Relaxed) != 0 && ledger_of(&mut s.ledger).pending_lists() == 0 {
        registered::freeze();
    }
}
/// BeforeSubmit: the submitted lists' draws validated (constant_ledger.rs).
///
/// # Safety
/// `lists` holds `count` lists (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_before_submit(count: u32, lists: *const *mut c_void) {
    let Some(h) = host() else { return };
    if lists.is_null() || word(h.pending_lists).load(SeqCst) == 0 {
        return;
    }
    let l = lock();
    let s = state(&l);
    word(h.seq).fetch_add(1, SeqCst);
    let resolver = s.resolve;
    let lh = ledger_host(s.reader, s.validator, &resolver);
    let ledger = ledger_of(&mut s.ledger);
    // SAFETY: as documented; the readers and the host's callbacks.
    unsafe { ledger.before_submit(core::slice::from_raw_parts(lists, count as usize), &lh) };
    if s.mark.is_some() && word(h.enable).load(Relaxed) != 0 && !s.active && ledger.pending_lists() == 0 {
        registered::freeze();
    }
    publish(h, ledger);
    word(h.seq).fetch_add(1, SeqCst);
}
/// Reset: a reset list's pending draws abandoned.
#[no_mangle]
pub extern "C-unwind" fn cvr_ncc2_reset(list: *mut c_void) {
    let Some(h) = host() else { return };
    if word(h.pending_lists).load(SeqCst) == 0 {
        return;
    }
    let l = lock();
    let ledger = ledger_of(&mut state(&l).ledger);
    if !ledger.has_pending(list as u64) {
        return;
    }
    word(h.seq).fetch_add(1, SeqCst);
    ledger.reset(list as u64);
    publish(h, ledger);
    word(h.seq).fetch_add(1, SeqCst);
}
/// Poll's part: the registered trace cleared when the capture is requested and published when it stops (nothing
/// pending), and, while inactive with nothing pending, the deferred bindings resolved after the fences' refresh.
#[no_mangle]
pub extern "C-unwind" fn cvr_ncc2_poll() {
    let Some(h) = host() else { return };
    let requested = word(h.enable).load(Relaxed) != 0;
    if requested && !COLLECTING.load(Relaxed) {
        registered::entries::clear_registered(false);
        COLLECTING.store(true, Relaxed);
    }
    {
        let l = lock();
        let s = state(&l);
        let resolver = s.resolve;
        let lh = ledger_host(s.reader, s.validator, &resolver);
        let ledger = ledger_of(&mut s.ledger);
        if !s.active && ledger.pending_lists() == 0 && ledger.deferred != 0 && resolver.is_some() {
            word(h.seq).fetch_add(1, SeqCst);
            registered::refresh_fences();
            // SAFETY: the resolver and the host's callbacks.
            if unsafe { ledger.resolve_deferred(&lh) } & INTERN_REFUSED != 0 {
                word(h.failure).store(1, SeqCst);
            }
            publish(h, ledger);
            word(h.seq).fetch_add(1, SeqCst);
        }
    }
    if !requested && COLLECTING.load(Relaxed) && word(h.pending_lists).load(SeqCst) == 0 {
        registered::entries::clear_registered(true);
        COLLECTING.store(false, Relaxed);
    }
}

// ---- the pair's copy (the .cpp's CopyConstantPair, under cvr_ncc2_lock) ------------------------------------------------

fn locked_state() -> &'static mut State {
    // SAFETY: the caller holds the lock (cvr_ncc2_lock).
    unsafe { &mut *STATE.0.get() }
}
/// The pair's draws (the left and right eyes' indices).
///
/// # Safety
/// Under cvr_ncc2_lock; `at` two writable indices.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_pair(left: u32, right: u32, at: *mut [usize; 2]) -> bool {
    match ledger_of(&mut locked_state().ledger).pair(left, right) {
        Some(found) => {
            // SAFETY: as documented.
            unsafe { at.write(found) };
            true
        }
        None => false,
    }
}
/// A draw's held family (null: none).
///
/// # Safety
/// Under cvr_ncc2_lock.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_owner(index: usize) -> *const c_void {
    locked_state().owners.get(index).map_or(core::ptr::null(), Owner::shared)
}
/// A draw's payload at the binding (valid while the lock is held).
///
/// # Safety
/// Under cvr_ncc2_lock; the outputs writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncc2_payload(
    draw: usize,
    parameter: u32,
    offset: u32,
    visibility: u32,
    shader_register: u32,
    data: *mut *const u8,
    bytes: *mut usize,
) -> bool {
    match ledger_of(&mut locked_state().ledger).payload(draw, parameter, offset, visibility, shader_register) {
        Some(payload) => {
            // SAFETY: as documented.
            unsafe {
                data.write(payload.as_ptr());
                bytes.write(payload.len());
            }
            true
        }
        None => false,
    }
}
