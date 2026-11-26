//! The native shader snapshots (formerly src/Render/NativeShaderSnapshot.cpp): in explicit observation runs the
//! creation hooks copy a root signature's serialized bytes and a graphics pipeline's VS/PS bytecode, input layout
//! (each semantic read through ReadProcessMemory) and creation contract into owners kept in the D3D12 object's private
//! data (one per object, never replaced), so a snapshot outlives the original PSO. Retention is bounded: at most 16384
//! roots and 16384 pipeline sources, 512 MiB together, each reserved before the copy and released with its owner (a
//! root) or with the last reference to the snapshot (a source).
//!
//! The snapshot itself stays the .cpp's value (`std::shared_ptr<const shader_snapshot::Snapshot>`, compared by identity
//! by its consumers): the source owner holds one such reference, which the .cpp's host builds from this module's view
//! once at creation; its deleter hands the reservation back through `cvr_shs_release`. The observation switch (the
//! probe's internal-command flag and native_probe::ObserveBindings) is the host's too, so the tests' stubs stay in force.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering::*};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use crate::pipeline_stream::PipelineStreamInfo;

pub const MAX_SHADER_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ROOT_BYTES: usize = 1024 * 1024;
pub const MAX_INPUTS: u32 = 32;
pub const MAX_SOURCES: u32 = 16384;
const MAX_RETAINED_BYTES: u64 = 512 * 1024 * 1024;
/// sizeof(shader_snapshot::Input) (a std::string and six UINTs), counted per input element with 256 semantic bytes.
const INPUT_BYTES: u64 = 56;
const ROOT_KEY: GUID = GUID::from_values(0x7cb1bec1, 0x9757, 0x434b, [0xb2, 0xcb, 0xb1, 0x25, 0xad, 0x4b, 0x41, 0x36]);
const SOURCE_KEY: GUID = GUID::from_values(0x7cb1bec1, 0x9757, 0x434b, [0xb2, 0xcb, 0xb1, 0x25, 0xad, 0x4b, 0x41, 0x37]);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static RETAINED_BYTES: AtomicU64 = AtomicU64::new(0);
static ROOTS: AtomicU32 = AtomicU32::new(0);
static SOURCES: AtomicU32 = AtomicU32::new(0);

// ---- the .cpp's host

/// One input element of the view (shader_snapshot::Input).
#[repr(C)]
pub struct InputView {
    pub semantic: *const u8,
    pub semantic_len: usize,
    pub index: u32,
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub classification: u32,
    pub step: u32,
}
/// A new snapshot's contents, lent to the host for the call; `bytes` its reservation (released by the snapshot).
#[repr(C)]
pub struct SnapshotView {
    pub id: u64,
    pub vs: *const u8,
    pub vs_len: usize,
    pub ps: *const u8,
    pub ps_len: usize,
    pub root: *const u8,
    pub root_len: usize,
    pub inputs: *const InputView,
    pub input_count: usize,
    pub stages: u32,
    pub stream_output: u32,
    pub bytes: u64,
}
const _: () = assert!(core::mem::size_of::<InputView>() == 40 && core::mem::size_of::<SnapshotView>() == 88);
#[repr(C)]
pub struct Host {
    /// !probe::internalCommands && native_probe::ObserveBindings().
    pub observe: unsafe extern "C" fn() -> bool,
    /// A new `std::shared_ptr<const Snapshot>` on the heap; null on failure, `*released` set when the snapshot's own
    /// deleter already handed the reservation back (else it stays this module's).
    pub make: unsafe extern "C" fn(*const SnapshotView, *mut bool) -> *mut c_void,
    /// Deletes such a heap reference.
    pub free: unsafe extern "C" fn(*mut c_void),
    /// The heap reference's snapshot: its identity (the Snapshot itself) and its byte views.
    pub describe: unsafe extern "C" fn(*const c_void, *mut Mirror),
}
/// A snapshot's C++ mirror as Rust reads it: its identity and its VS, PS and root bytes (alive with the source owner).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Mirror {
    pub identity: *const c_void,
    pub vs: *const u8,
    pub vs_len: usize,
    pub ps: *const u8,
    pub ps_len: usize,
    pub root: *const u8,
    pub root_len: usize,
}
const _: () = assert!(core::mem::size_of::<Mirror>() == 56);
/// One input element (shader_snapshot::Input).
pub struct InputRec {
    pub semantic: Vec<u8>,
    pub index: u32,
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub classification: u32,
    pub step: u32,
}
/// A source snapshot's Rust side.
pub struct SourceInfo {
    pub id: u64,
    pub mirror: Mirror,
    pub inputs: Vec<InputRec>,
}
/// A pipeline's source snapshot, its owner held.
#[derive(Clone)]
pub struct Source(IUnknown);
// SAFETY: the owner is a free-threaded COM object; its info is immutable once made.
unsafe impl Send for Source {}
impl Source {
    pub fn info(&self) -> &SourceInfo {
        owner_of(&self.0).info.as_ref().expect("a source owner's info")
    }
    /// The owner's heap `shared_ptr<const Snapshot>` (the host's; alive while this is).
    pub fn shared(&self) -> *mut c_void {
        owner_of(&self.0).snapshot
    }
}
/// shader_snapshot::Capture for Rust: the pipeline's source snapshot while observing (None otherwise).
pub fn source(pipeline: *mut c_void) -> Option<Source> {
    if !observe() || pipeline.is_null() {
        return None;
    }
    get(pipeline, &SOURCE_KEY).filter(|u| owner_of(u).info.is_some()).map(Source)
}
/// The plugin's host (formerly NativeShaderSnapshot.cpp's): a snapshot is a Rust record shared by count, handed out as
/// a heap handle whose first word is its identity (the record) -- where the .cpp handed out a heap
/// `std::shared_ptr<const Snapshot>` whose first word is the Snapshot. The record keeps the VS, PS and root bytes the
/// mirror views and the inputs instance_snapshot_native.rs lays out. The Rust test variants attach the .cpp's host.
#[cfg(all(feature = "rust-build", not(test)))]
pub(crate) mod plugin {
    use super::{InputRec, Mirror, SnapshotView, SOURCES};
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicUsize, Ordering::*};
    /// A snapshot (shader_snapshot::Snapshot): its id, bytes, inputs, reservation and references.
    struct Record {
        refs: AtomicUsize,
        id: u64,
        vs: Vec<u8>,
        ps: Vec<u8>,
        root: Vec<u8>,
        inputs: Vec<InputRec>,
        bytes: u64,
    }
    /// A heap reference to a record (the .cpp's heap shared_ptr): the identity first.
    #[repr(C)]
    struct Handle {
        identity: *const Record,
        record: *const Record,
    }
    fn handle(record: *const Record) -> *mut c_void {
        Box::into_raw(Box::new(Handle { identity: record, record })).cast()
    }
    fn record_of(shared: *const c_void) -> Option<&'static Record> {
        // SAFETY: a live handle (or null): its record lives while any handle does.
        unsafe { shared.cast::<Handle>().as_ref().and_then(|h| h.record.as_ref()) }
    }
    unsafe extern "C" fn observe() -> bool {
        !cvr_host::probe::internal() && crate::stereo_probe::observe_bindings()
    }
    unsafe extern "C" fn make(view: *const SnapshotView, released: *mut bool) -> *mut c_void {
        // SAFETY: the caller's view (its byte views live through the call) and output.
        let (v, released) = unsafe { (&*view, &mut *released) };
        *released = false;
        let bytes = |p: *const u8, n: usize| if p.is_null() || n == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(p, n) }.to_vec() };
        let inputs = if v.inputs.is_null() || v.input_count == 0 {
            Vec::new()
        } else {
            // SAFETY: the view's inputs.
            unsafe { core::slice::from_raw_parts(v.inputs, v.input_count) }
                .iter()
                .map(|i| InputRec {
                    semantic: bytes(i.semantic, i.semantic_len),
                    index: i.index,
                    format: i.format,
                    slot: i.slot,
                    offset: i.offset,
                    classification: i.classification,
                    step: i.step,
                })
                .collect()
        };
        let record = Box::new(Record {
            refs: AtomicUsize::new(1),
            id: v.id,
            vs: bytes(v.vs, v.vs_len),
            ps: bytes(v.ps, v.ps_len),
            root: bytes(v.root, v.root_len),
            inputs,
            bytes: v.bytes,
        });
        handle(Box::into_raw(record))
    }
    /// A heap reference released: the record (and its reservation) with the last one.
    pub(crate) unsafe extern "C" fn free(shared: *mut c_void) {
        if shared.is_null() {
            return;
        }
        // SAFETY: a handle made here, released once.
        let h = unsafe { Box::from_raw(shared.cast::<Handle>()) };
        // SAFETY: the handle's record.
        if unsafe { (*h.record).refs.fetch_sub(1, AcqRel) } == 1 {
            // SAFETY: the last reference.
            let record = unsafe { Box::from_raw(h.record.cast_mut()) };
            super::unreserve(&SOURCES, record.bytes);
        }
    }
    unsafe extern "C" fn describe(shared: *const c_void, out: *mut Mirror) {
        let Some(r) = record_of(shared) else { return };
        // SAFETY: the caller's output; the views live with the record.
        unsafe {
            *out = Mirror {
                identity: (r as *const Record).cast(),
                vs: r.vs.as_ptr(),
                vs_len: r.vs.len(),
                ps: r.ps.as_ptr(),
                ps_len: r.ps.len(),
                root: r.root.as_ptr(),
                root_len: r.root.len(),
            }
        };
    }
    pub(super) static HOST: super::Host = super::Host { observe, make, free, describe };
    /// A new heap reference to the same record (null when empty).
    pub(crate) fn retain(shared: *const c_void) -> *mut c_void {
        let Some(r) = record_of(shared) else { return core::ptr::null_mut() };
        r.refs.fetch_add(1, Relaxed);
        handle(r)
    }
    /// The record (the identity), null when empty.
    pub(crate) fn get(shared: *const c_void) -> *const c_void {
        record_of(shared).map_or(core::ptr::null(), |r| (r as *const Record).cast())
    }
    pub(crate) fn id(shared: *const c_void) -> u64 {
        record_of(shared).map_or(0, |r| r.id)
    }
    /// detail::Layout(the record's inputs, stride).
    pub(crate) fn layout(shared: *const c_void, stride: u32) -> bool {
        let Some(r) = record_of(shared) else { return false };
        if r.inputs.len() > MAX_INPUTS {
            return false;
        }
        let lent: Vec<crate::instance_snapshot_view::InputElement> = r
            .inputs
            .iter()
            .map(|i| crate::instance_snapshot_view::InputElement {
                semantic_bytes: i.semantic.len(),
                semantic_nul: i.semantic.contains(&0),
                index: i.index,
                format: i.format,
                slot: i.slot,
                offset: i.offset,
                classification: i.classification,
                step: i.step,
            })
            .collect();
        crate::instance_snapshot_view::layout(&lent, stride, MAX_INPUTS)
    }
    /// shader_snapshot::MaxInputs.
    const MAX_INPUTS: usize = 32;
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_attach(host: *const Host) {
    HOST.store(host.cast_mut(), Release);
}
fn host() -> Option<&'static Host> {
    // SAFETY: the attached table.
    let attached = unsafe { HOST.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::HOST));
    attached
}
fn observe() -> bool {
    // SAFETY: the host's switch.
    host().is_some_and(|h| unsafe { (h.observe)() })
}

// ---- the owners kept in private data

#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    key: GUID,
    /// A root's serialized bytes (reserved against ROOTS).
    root: Vec<u8>,
    /// A source's heap `shared_ptr<const Snapshot>` (the host's).
    snapshot: *mut c_void,
    /// A source's Rust side.
    info: Option<SourceInfo>,
}
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live owner.
    unsafe {
        *out = core::ptr::null_mut();
        let owner = &*(this as *const Owner);
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != owner.key) {
            return E_NOINTERFACE;
        }
        *out = this;
        owner.refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live owner.
    unsafe { (*(this as *const Owner)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: a live owner, deleted at zero.
    unsafe {
        let remaining = (*(this as *const Owner)).refs.fetch_sub(1, SeqCst) - 1;
        if remaining == 0 {
            drop(Box::from_raw(this as *mut Owner));
        }
        remaining
    }
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };
impl Drop for Owner {
    fn drop(&mut self) {
        if self.key == ROOT_KEY {
            unreserve(&ROOTS, self.root.len() as u64);
        } else if let (false, Some(h)) = (self.snapshot.is_null(), host()) {
            // SAFETY: the host's reference, deleted once.
            unsafe { (h.free)(self.snapshot) };
        }
    }
}
/// A new owner as an IUnknown holding its creation reference.
fn owner(key: GUID, root: Vec<u8>, snapshot: *mut c_void, info: Option<SourceInfo>) -> IUnknown {
    let raw = Box::into_raw(Box::new(Owner { vtbl: &OWNER_VTBL, refs: AtomicU32::new(1), key, root, snapshot, info }));
    // SAFETY: the new object's one reference.
    unsafe { IUnknown::from_raw(raw.cast()) }
}
/// The object's owner under `key`, held for the caller (its reference released when the IUnknown drops).
fn get(object: *mut c_void, key: &GUID) -> Option<IUnknown> {
    // SAFETY: a live D3D12 object of the caller's or null.
    let object = unsafe { ID3D12Object::from_raw_borrowed(&object) }?;
    let mut unknown: *mut c_void = core::ptr::null_mut();
    let mut bytes = core::mem::size_of::<*mut c_void>() as u32;
    // SAFETY: the private data's interface pointer into `unknown` (a reference on success).
    let got = unsafe { object.GetPrivateData(key, &mut bytes, Some((&mut unknown as *mut *mut c_void).cast())) };
    if got.is_err() || bytes as usize != core::mem::size_of::<*mut c_void>() || unknown.is_null() {
        return None;
    }
    // SAFETY: the reference GetPrivateData handed out.
    let unknown = unsafe { IUnknown::from_raw(unknown) };
    let mut result: *mut c_void = core::ptr::null_mut();
    // SAFETY: a QueryInterface for this module's key (only its owners answer it).
    if unsafe { unknown.query(key, &mut result) }.is_err() || result.is_null() {
        return None;
    }
    // SAFETY: the reference the query handed out.
    Some(unsafe { IUnknown::from_raw(result) })
}
fn owner_of(unknown: &IUnknown) -> &Owner {
    // SAFETY: an IUnknown answering this module's key is one of its owners.
    unsafe { &*(unknown.as_raw() as *const Owner) }
}

// ---- the reads and the reservations

fn read(source: *const c_void, destination: &mut [u8]) -> bool {
    let address = source as u64;
    !source.is_null() && !destination.is_empty() && address <= u64::MAX - destination.len() as u64 && crate::instance_layout::read(address, destination)
}
fn copy(source: *const c_void, bytes: usize) -> Option<Vec<u8>> {
    let mut result = vec![0u8; bytes];
    read(source, &mut result).then_some(result)
}
/// A semantic name: up to 255 bytes and a terminator, read at once or byte by byte.
fn semantic(source: *const u8) -> Option<Vec<u8>> {
    if source.is_null() {
        return None;
    }
    let mut bytes = [0u8; 256];
    if read(source.cast(), &mut bytes) {
        let end = bytes.iter().position(|&b| b == 0)?;
        return (end != 0).then(|| bytes[..end].to_vec());
    }
    let mut result = Vec::new();
    for i in 0..256u64 {
        let address = source as u64;
        let mut c = [0u8; 1];
        if address > u64::MAX - i || !read((address + i) as *const c_void, &mut c) {
            return None;
        }
        if c[0] == 0 {
            return (!result.is_empty()).then_some(result);
        }
        result.push(c[0]);
    }
    None
}
fn reserve(count: &AtomicU32, bytes: u64) -> bool {
    if count.fetch_update(Relaxed, Relaxed, |current| (current < MAX_SOURCES).then_some(current + 1)).is_err() {
        return false;
    }
    if RETAINED_BYTES
        .fetch_update(Relaxed, Relaxed, |used| (bytes <= MAX_RETAINED_BYTES && used <= MAX_RETAINED_BYTES - bytes).then_some(used + bytes))
        .is_err()
    {
        count.fetch_sub(1, Relaxed);
        return false;
    }
    true
}
fn unreserve(count: &AtomicU32, bytes: u64) {
    count.fetch_sub(1, Relaxed);
    RETAINED_BYTES.fetch_sub(bytes, Relaxed);
}
fn next_id() -> u64 {
    NEXT_ID.fetch_update(Relaxed, Relaxed, |id| (id != u64::MAX).then_some(id + 1)).unwrap_or(0)
}

// ---- creation

fn root_created(root: *mut c_void, bytes: *const c_void, size: usize) {
    if !observe() || root.is_null() || get(root, &ROOT_KEY).is_some() {
        return;
    }
    if bytes.is_null() || size == 0 || size > MAX_ROOT_BYTES || !reserve(&ROOTS, size as u64) {
        return;
    }
    let Some(data) = copy(bytes, size) else {
        unreserve(&ROOTS, size as u64);
        return;
    };
    let owner = owner(ROOT_KEY, data, core::ptr::null_mut(), None);
    // SAFETY: a live root (checked above); the object takes its own reference.
    let root = unsafe { ID3D12Object::from_raw_borrowed(&root) }.expect("root");
    // SAFETY: as above.
    let _ = unsafe { root.SetPrivateDataInterface(&ROOT_KEY, &owner) };
}
/// The creation contract: (StageCompleteness, StreamOutputPresence).
type Contract = (u32, u32);
#[allow(clippy::too_many_arguments)]
fn capture_graphics(pipeline: *mut c_void, root: *mut c_void, vs: D3D12_SHADER_BYTECODE, ps: D3D12_SHADER_BYTECODE, input: &D3D12_INPUT_LAYOUT_DESC, creation: Contract) {
    if !observe() || pipeline.is_null() || get(pipeline, &SOURCE_KEY).is_some() {
        return;
    }
    if root.is_null()
        || vs.pShaderBytecode.is_null()
        || vs.BytecodeLength == 0
        || vs.BytecodeLength > MAX_SHADER_BYTES
        || ps.BytecodeLength > MAX_SHADER_BYTES
        || (ps.BytecodeLength != 0 && ps.pShaderBytecode.is_null())
        || input.NumElements > MAX_INPUTS
        || (input.NumElements != 0 && input.pInputElementDescs.is_null())
    {
        return;
    }
    let Some(serialized) = get(root, &ROOT_KEY) else { return };
    let serialized = &owner_of(&serialized).root;
    if serialized.is_empty() {
        return;
    }
    let count = input.NumElements as usize;
    let mut elements = [D3D12_INPUT_ELEMENT_DESC::default(); MAX_INPUTS as usize];
    if count != 0 {
        // SAFETY: plain descriptions' bytes.
        let raw = unsafe { core::slice::from_raw_parts_mut(elements.as_mut_ptr().cast::<u8>(), count * core::mem::size_of::<D3D12_INPUT_ELEMENT_DESC>()) };
        if !read(input.pInputElementDescs.cast(), raw) {
            return;
        }
    }
    let bytes = (vs.BytecodeLength + ps.BytecodeLength + serialized.len()) as u64 + count as u64 * (INPUT_BYTES + 256);
    if !reserve(&SOURCES, bytes) {
        return;
    }
    let made = (|| {
        let vs_bytes = copy(vs.pShaderBytecode, vs.BytecodeLength)?;
        let ps_bytes = if ps.BytecodeLength != 0 { copy(ps.pShaderBytecode, ps.BytecodeLength)? } else { Vec::new() };
        let mut names = Vec::with_capacity(count);
        for element in &elements[..count] {
            names.push(semantic(element.SemanticName.0)?);
        }
        let views: Vec<InputView> = elements[..count]
            .iter()
            .zip(&names)
            .map(|(e, name)| InputView {
                semantic: name.as_ptr(),
                semantic_len: name.len(),
                index: e.SemanticIndex,
                format: e.Format.0 as u32,
                slot: e.InputSlot,
                offset: e.AlignedByteOffset,
                classification: e.InputSlotClass.0 as u32,
                step: e.InstanceDataStepRate,
            })
            .collect();
        let id = next_id();
        if id == 0 {
            return None;
        }
        let view = SnapshotView {
            id,
            vs: vs_bytes.as_ptr(),
            vs_len: vs_bytes.len(),
            ps: ps_bytes.as_ptr(),
            ps_len: ps_bytes.len(),
            root: serialized.as_ptr(),
            root_len: serialized.len(),
            inputs: views.as_ptr(),
            input_count: views.len(),
            stages: creation.0,
            stream_output: creation.1,
            bytes,
        };
        let mut released = false;
        // SAFETY: the view lives through the call.
        let snapshot = host().map_or(core::ptr::null_mut(), |h| unsafe { (h.make)(&view, &mut released) });
        let inputs: Vec<InputRec> = elements[..count]
            .iter()
            .zip(names)
            .map(|(e, semantic)| InputRec {
                semantic,
                index: e.SemanticIndex,
                format: e.Format.0 as u32,
                slot: e.InputSlot,
                offset: e.AlignedByteOffset,
                classification: e.InputSlotClass.0 as u32,
                step: e.InstanceDataStepRate,
            })
            .collect();
        Some((snapshot, released, id, inputs))
    })();
    let (snapshot, id, inputs) = match made {
        Some((snapshot, _, id, inputs)) if !snapshot.is_null() => (snapshot, id, inputs),
        Some((_, true, _, _)) => return,
        _ => {
            unreserve(&SOURCES, bytes);
            return;
        }
    };
    let mut mirror = Mirror { identity: core::ptr::null(), vs: core::ptr::null(), vs_len: 0, ps: core::ptr::null(), ps_len: 0, root: core::ptr::null(), root_len: 0 };
    if let Some(h) = host() {
        // SAFETY: the heap reference just made; a writable mirror.
        unsafe { (h.describe)(snapshot, &mut mirror) };
    }
    let owner = owner(SOURCE_KEY, Vec::new(), snapshot, Some(SourceInfo { id, mirror, inputs }));
    // SAFETY: a live pipeline (checked above); the object takes its own reference.
    let pipeline = unsafe { ID3D12Object::from_raw_borrowed(&pipeline) }.expect("pipeline");
    // SAFETY: as above.
    let _ = unsafe { pipeline.SetPrivateDataInterface(&SOURCE_KEY, &owner) };
}
/// DescribeCreation: a complete description's stages and stream output.
fn describe_creation(source: &D3D12_GRAPHICS_PIPELINE_STATE_DESC) -> Contract {
    if source.pRootSignature.is_none() || source.VS.pShaderBytecode.is_null() || source.VS.BytecodeLength == 0 || (source.PS.BytecodeLength != 0 && source.PS.pShaderBytecode.is_null()) {
        return (0, 0);
    }
    (
        if source.HS.BytecodeLength != 0 || source.DS.BytecodeLength != 0 || source.GS.BytecodeLength != 0 { 2 } else { 1 },
        if source.StreamOutput.NumEntries != 0 || source.StreamOutput.NumStrides != 0 { 2 } else { 1 },
    )
}
/// DescribeStreamCreation: a parsed complete stream's.
fn describe_stream_creation(source: &PipelineStreamInfo) -> Contract {
    if source.root.is_null() || source.vs.pShaderBytecode.is_null() || source.vs.BytecodeLength == 0 || (source.ps.BytecodeLength != 0 && source.ps.pShaderBytecode.is_null()) {
        return (0, 0);
    }
    (if source.other_stages { 2 } else { 1 }, if source.stream_output { 2 } else { 1 })
}

// ---- the C ABI (the .cpp's API forwards)

/// RootCreated.
///
/// # Safety
/// `bytes` holds `size` bytes of this process (read through ReadProcessMemory).
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_root_created(root: *mut c_void, bytes: *const c_void, size: usize) {
    root_created(root, bytes, size);
}
/// GraphicsCreated (legacy partial bytecode: the contract stays unknown).
///
/// # Safety
/// `input` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_graphics_created(
    pipeline: *mut c_void,
    root: *mut c_void,
    vs: D3D12_SHADER_BYTECODE,
    ps: D3D12_SHADER_BYTECODE,
    input: *const D3D12_INPUT_LAYOUT_DESC,
) {
    // SAFETY: as documented.
    capture_graphics(pipeline, root, vs, ps, unsafe { &*input }, (0, 0));
}
/// GraphicsCreated from a complete description.
///
/// # Safety
/// `source` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_graphics_desc_created(pipeline: *mut c_void, source: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC) {
    // SAFETY: as documented.
    let source = unsafe { &*source };
    let root = (*source.pRootSignature).as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw());
    capture_graphics(pipeline, root, source.VS, source.PS, &source.InputLayout, describe_creation(source));
}
/// GraphicsStreamCreated (after ReadPipelineStream succeeded on the actual creation descriptor).
///
/// # Safety
/// `source` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_graphics_stream_created(pipeline: *mut c_void, source: *const PipelineStreamInfo) {
    // SAFETY: as documented.
    let source = unsafe { &*source };
    capture_graphics(pipeline, source.root, source.vs, source.ps, &source.input, describe_stream_creation(source));
}
/// DescribeCreation and DescribeStreamCreation (`stream` picks the PipelineStreamInfo): [stages, stream output].
///
/// # Safety
/// `source` is the described value.
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_describe(source: *const c_void, stream: bool, out: *mut [u32; 2]) {
    // SAFETY: as documented.
    let (stages, output) = unsafe {
        if stream {
            describe_stream_creation(&*source.cast::<PipelineStreamInfo>())
        } else {
            describe_creation(&*source.cast::<D3D12_GRAPHICS_PIPELINE_STATE_DESC>())
        }
    };
    // SAFETY: as documented.
    unsafe { *out = [stages, output] };
}
/// Capture: the pipeline's heap `shared_ptr<const Snapshot>` (null without one), its owner held in `*holder` (an
/// IUnknown reference the caller releases after copying the snapshot's reference).
///
/// # Safety
/// `holder` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_shs_capture(pipeline: *mut c_void, holder: *mut *mut c_void) -> *mut c_void {
    // SAFETY: as documented.
    unsafe { *holder = core::ptr::null_mut() };
    if !observe() || pipeline.is_null() {
        return core::ptr::null_mut();
    }
    let Some(found) = get(pipeline, &SOURCE_KEY) else { return core::ptr::null_mut() };
    let snapshot = owner_of(&found).snapshot;
    // SAFETY: the holder takes the owner's reference.
    unsafe { *holder = found.into_raw() };
    snapshot
}
/// A snapshot's reservation, handed back by its deleter.
#[no_mangle]
pub extern "C" fn cvr_shs_release(bytes: u64) {
    unreserve(&SOURCES, bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_are_bounded() {
        let count = AtomicU32::new(MAX_SOURCES - 1);
        assert!(reserve(&count, 16));
        assert!(!reserve(&count, 16));
        unreserve(&count, 16);
        assert!(!reserve(&AtomicU32::new(0), MAX_RETAINED_BYTES + 1));
    }
    #[test]
    fn creation_contracts() {
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC::default();
        assert_eq!(describe_creation(&desc), (0, 0));
        let byte = [0u8; 4];
        desc.VS = D3D12_SHADER_BYTECODE { pShaderBytecode: byte.as_ptr().cast(), BytecodeLength: 4 };
        assert_eq!(describe_creation(&desc), (0, 0)); // no root
    }
}
