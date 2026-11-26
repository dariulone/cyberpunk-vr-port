//! THE NATIVE TEXTURES' READ LEASES (formerly the Lease path of src/Render/NativeTextureRead.cpp): a texture the engine
//! binds is resolved to its native record (its handle in the registry's texture table, cached as the resource's private
//! data), acquired for a read on the engine's current upload context -- by its native state when the engine tracks it,
//! by its creation receipt when it is a fixed asset (texture_receipts.rs) -- and copied into a buffer with the engine's
//! own read registration first, so the native state tracking stays coherent. The module's failure code lives here; the
//! rest of the C++ module (the immutable leases, the fixed pins, the writable identities) writes it through
//! cvr_tr_set_failure and reads these helpers through the C ABI below.
use core::cell::Cell;
use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering::*};
use std::sync::OnceLock;

use windows::core::{Interface as _, GUID};
use windows::Win32::Graphics::Direct3D12::*;

use crate::texture_read::{
    alpha_format, copy_read_mask, fixed_owner as record_owner, footprint_range, has_read_state,
    known_allocation, read_state, shape, source_read_state, subresource_layout, view_layouts,
    MipFootprint, NativeTexture, ReadMemory, State, ALLOWED_READS, SHADER_READS,
};
use crate::texture_receipts::{fixed_compatible, native_queue, placement_of};

const NATIVE_HANDLE_KEY: GUID = GUID::from_u128(0xe9165914_664a_4174_b11b_203aadd53487);
const TABLE: u64 = 0x2f1d8;
const RECORDS: usize = 32768;
const STRIDE: u64 = 0xb0;

/// texture_read::Lease as the C++ lays it out (its ComPtr is the resource pointer).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Lease {
    pub resource: *mut c_void,
    pub native: NativeTexture,
    pub receipt: u64,
    pub owner: u64,
}
/// camera_upload::Context.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct UploadContext {
    pub context: u64,
    pub state: u64,
    pub list: u64,
}
const _: () =
    assert!(core::mem::size_of::<Lease>() == 48 && core::mem::size_of::<UploadContext>() == 24);

thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
}
pub(crate) fn fail(code: u32) {
    FAILURE.set(code);
}

static CURRENT_CONTEXT: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `current` writes camera_upload::CurrentContext(game) into its second argument.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_lease_attach(
    current: unsafe extern "C" fn(u64, *mut UploadContext),
) {
    CURRENT_CONTEXT.store(current as *mut c_void, Release);
}
pub(crate) fn current_context(game: u64) -> UploadContext {
    let f = CURRENT_CONTEXT.load(Acquire);
    let mut out = UploadContext::default();
    // The plugin's own (formerly lent by src/Render/NativeTextureRead.cpp): camera_upload::CurrentContext.
    #[cfg(all(feature = "rust-build", not(test)))]
    if f.is_null() {
        let c = crate::camera_upload::current(game);
        return UploadContext { context: c.context, state: c.state, list: c.list };
    }
    if !f.is_null() {
        // SAFETY: the attached function.
        unsafe {
            core::mem::transmute::<*mut c_void, unsafe extern "C" fn(u64, *mut UploadContext)>(f)(
                game, &mut out,
            )
        };
    }
    out
}
pub(crate) fn game() -> u64 {
    cvr_engine::code::image_base() as u64
}
pub(crate) fn read(address: u64, out: &mut [u8]) -> bool {
    address != 0 && address <= u64::MAX - out.len() as u64 && cvr_engine::stack::read(address, out)
}
pub(crate) fn read_value<T: Copy + Default>(address: u64) -> Option<T> {
    if address == 0 || address > u64::MAX - core::mem::size_of::<T>() as u64 {
        return None;
    }
    cvr_engine::stack::read_value(address)
}
/// The module's ReadMemory for ReadState: this process's memory.
unsafe extern "C" fn read_memory(
    _: *mut c_void,
    address: u64,
    out: *mut c_void,
    bytes: usize,
) -> bool {
    // SAFETY: the caller's buffer of `bytes` bytes.
    let buffer = unsafe { core::slice::from_raw_parts_mut(out as *mut u8, bytes) };
    read(address, buffer)
}
pub(crate) const READ: Option<ReadMemory> = Some(read_memory);

pub(crate) fn resource<'a>(p: &'a *mut c_void) -> Option<&'a ID3D12Resource> {
    // SAFETY: a live resource of the caller's, or null.
    unsafe { ID3D12Resource::from_raw_borrowed(p) }
}
pub(crate) fn list_of<'a>(p: &'a *mut c_void) -> Option<&'a ID3D12GraphicsCommandList> {
    // SAFETY: a live command list of the caller's, or null.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(p) }
}
pub(crate) fn device_of(child: &ID3D12DeviceChild) -> Option<ID3D12Device> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}
pub(crate) fn default_heap(r: &ID3D12Resource) -> bool {
    let mut properties = D3D12_HEAP_PROPERTIES::default();
    let mut flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: a live resource.
    unsafe { r.GetHeapProperties(Some(&mut properties), Some(&mut flags)) }.is_ok()
        && properties.Type == D3D12_HEAP_TYPE_DEFAULT
}
fn subresources(d: &D3D12_RESOURCE_DESC) -> u32 {
    d.MipLevels as u32 * d.DepthOrArraySize as u32
}

/// Identity: the native record still names this resource in the current registry.
pub fn identity(n: &NativeTexture) -> bool {
    n.registry != 0
        && n.resource != 0
        && n.handle != 0
        && n.handle <= 32768
        && read_value::<u64>(game() + 0x3bcdca8) == Some(n.registry)
        && n.registry <= u64::MAX - 0x2f1e0 - (n.handle as u64 - 1) * STRIDE
        && read_value::<u64>(n.registry + TABLE + (n.handle as u64 - 1) * STRIDE)
            == Some(n.resource)
}
/// ResolveNative: the resource's native record -- from its private data when that still holds, else by a scan of the
/// registry's texture table (the one record naming it), then remembered.
pub(crate) fn resolve_native(r: &ID3D12Resource) -> Option<NativeTexture> {
    let registry = read_value::<u64>(game() + 0x3bcdca8).filter(|&v| v != 0)?;
    let mut native = NativeTexture::default();
    let mut bytes = 24u32;
    // SAFETY: at most 24 bytes written into the record.
    let cached = unsafe {
        r.GetPrivateData(
            &NATIVE_HANDLE_KEY,
            &mut bytes,
            Some(&mut native as *mut NativeTexture as *mut c_void),
        )
    };
    if cached.is_ok()
        && bytes == 24
        && native.registry == registry
        && native.resource == r.as_raw() as u64
        && identity(&native)
    {
        return Some(native);
    }
    let size = RECORDS * STRIDE as usize;
    fail(3);
    if registry > u64::MAX - TABLE - size as u64 {
        return None;
    }
    let mut records = vec![0u8; size];
    if !read(registry + TABLE, &mut records) {
        return None;
    }
    let mut native = NativeTexture {
        registry,
        resource: r.as_raw() as u64,
        handle: 0,
        reserved: 0,
    };
    for i in 0..RECORDS {
        let at = i * STRIDE as usize;
        if u64::from_le_bytes(records[at..at + 8].try_into().unwrap()) == native.resource {
            if native.handle != 0 {
                fail(4);
                return None;
            }
            native.handle = i as u32 + 1;
        }
    }
    fail(5);
    if !identity(&native) {
        return None;
    }
    // SAFETY: 24 bytes of the record.
    unsafe {
        r.SetPrivateData(
            &NATIVE_HANDLE_KEY,
            24,
            Some(&native as *const NativeTexture as *const c_void),
        )
    }
    .is_ok()
    .then_some(native)
}
/// FixedOwner: the fixed asset's owner from its native record (0 when the record is not one).
pub fn fixed_owner(n: &NativeTexture) -> u64 {
    if !identity(n) || n.registry > u64::MAX - TABLE - (n.handle as u64 - 1) * STRIDE - 0x58 {
        return 0;
    }
    let mut record = [0u8; 0x58];
    if !read(
        n.registry + TABLE + (n.handle as u64 - 1) * STRIDE,
        &mut record,
    ) {
        return 0;
    }
    if u64::from_le_bytes(record[0..8].try_into().unwrap()) == n.resource {
        record_owner(&record)
    } else {
        0
    }
}
/// FixedContext: the upload context is a direct one (kind 1 or 2) recording this list, with no pending batch.
pub fn fixed_context(c: &UploadContext) -> bool {
    c.context != 0
        && c.state != 0
        && c.list != 0
        && c.context <= u64::MAX - 0x538
        && read_value::<u32>(c.context + 0x68).is_some_and(|k| k == 1 || k == 2)
        && read_value::<u64>(c.context + 0x30) == Some(c.list)
        && read_value::<u32>(c.context + 0x534) == Some(0)
}
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}
/// Compatible: the engine's read registration and its flush are the audited ones.
pub fn compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| {
        const EXPECTED: [(u64, usize, u64); 4] = [
            (0x337230, 0x33, 0x96d1a06a9c2cba7f),
            (0x337264, 0x11a, 0xbbe79ba5988d6982),
            (0x337380, 0x506, 0x1ca82dd09817c1a3),
            (0x3e3bb4, 0x36, 0xf999b875f95ad550),
        ];
        let base = game();
        let mut bytes = [0u8; 1536];
        EXPECTED.iter().all(|&(rva, len, hash)| {
            read(base + rva, &mut bytes[..len]) && fnv(&bytes[..len]) == hash
        })
    })
}
/// Validate: the lease's record still names its resource; a receipt lease is still the fixed asset's, on the registry's
/// direct queue when one is given, and its receipt is still acquired, uploaded and untouched.
pub fn validate(source: &Lease, queue: *mut c_void) -> bool {
    if source.resource.is_null()
        || source.native.resource != source.resource as u64
        || !identity(&source.native)
    {
        return false;
    }
    if source.receipt == 0 {
        return true;
    }
    // SAFETY: a live queue of the caller's, or null.
    let q = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) };
    if !fixed_compatible()
        || source.owner == 0
        || fixed_owner(&source.native) != source.owner
        || (!queue.is_null() && !native_queue(source.native.registry, q, false))
    {
        return false;
    }
    crate::texture_receipts::receipt_valid(source.resource, source.receipt, source.native.registry)
}
/// Acquire: the bound texture as a read lease on this list.
fn acquire(list: *mut c_void, resource_ptr: *mut c_void, out: &mut Lease) -> bool {
    *out = Lease {
        resource: core::ptr::null_mut(),
        native: NativeTexture::default(),
        receipt: 0,
        owner: 0,
    };
    fail(1);
    let (Some(l), Some(r)) = (list_of(&list), resource(&resource_ptr)) else {
        return false;
    };
    // SAFETY (whole function): live objects.
    let desc = unsafe { r.GetDesc() };
    if !compatible() || unsafe { l.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT || !shape(&desc) {
        return false;
    }
    fail(69);
    if alpha_format(desc.Format)
        && (!default_heap(r) || !known_allocation(&placement_of(resource_ptr).allocation))
    {
        return false;
    }
    let base = game();
    let context = current_context(base);
    fail(2);
    if context.list != list as u64 {
        return false;
    }
    let Some(native) = resolve_native(r) else {
        return false;
    };
    let state = read_state(
        &native,
        context.context,
        context.list,
        READ,
        core::ptr::null_mut(),
    );
    if state.status == 8 {
        fail(50);
        if !fixed_compatible() {
            return false;
        }
        let owner = fixed_owner(&native);
        fail(51);
        if owner == 0 {
            return false;
        }
        fail(52);
        if !default_heap(r) {
            return false;
        }
        fail(56);
        if !fixed_context(&context) || current_context(base) != context {
            return false;
        }
        fail(53);
        let mut code = 0;
        let receipt =
            crate::texture_receipts::acquire_receipt(resource_ptr, native.registry, &mut code);
        fail(code);
        if receipt == 0 {
            return false;
        }
        let lease = Lease {
            resource: resource_ptr,
            native,
            receipt,
            owner,
        };
        fail(54);
        if !validate(&lease, core::ptr::null_mut()) {
            return false;
        }
        *out = lease;
        fail(0);
        return true;
    }
    fail(10 + state.status);
    if alpha_format(desc.Format) {
        if state.status != 1 || state.value & SHADER_READS == 0 {
            return false;
        }
    } else if !source_read_state(state, subresources(&desc)) {
        return false;
    }
    *out = Lease {
        resource: resource_ptr,
        native,
        receipt: 0,
        owner: 0,
    };
    fail(0);
    true
}
fn same_layout(
    a: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    b: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
) -> bool {
    a.Offset == b.Offset
        && a.Footprint.Format == b.Footprint.Format
        && a.Footprint.Width == b.Footprint.Width
        && a.Footprint.Height == b.Footprint.Height
        && a.Footprint.Depth == b.Footprint.Depth
        && a.Footprint.RowPitch == b.Footprint.RowPitch
}
/// ViewFootprints: the copy layouts of a view of the leased texture, starting at `offset`.
fn view_footprints(
    device: *mut c_void,
    source: &Lease,
    view: &D3D12_SHADER_RESOURCE_VIEW_DESC,
    offset: u64,
) -> Option<Vec<MipFootprint>> {
    fail(70);
    // SAFETY: a live device of the caller's, or null.
    let d = unsafe { ID3D12Device::from_raw_borrowed(&device) }?;
    let r = resource(&source.resource)?;
    if offset % D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT as u64 != 0
        || !validate(source, core::ptr::null_mut())
    {
        return None;
    }
    let actual = device_of(&r.cast::<ID3D12DeviceChild>().ok()?)?;
    if actual.as_raw() != device {
        return None;
    }
    fail(71);
    // SAFETY: a live resource.
    let result = view_layouts(d, &unsafe { r.GetDesc() }, view, offset)?;
    fail(0);
    Some(result)
}
type Register = unsafe extern "C" fn(*mut c_void, u32, u32, u32);
type Flush = unsafe extern "C" fn(*mut c_void);
/// CopySubresource: one subresource of the leased texture into a buffer at `layout`, the engine's read registration
/// first (with recovery of the shader-read state when the copy state could not be established).
fn copy_subresource(
    list: *mut c_void,
    source: &Lease,
    mip: u32,
    buffer: *mut c_void,
    layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
) -> bool {
    fail(20);
    let (Some(l), Some(r), Some(b)) = (
        list_of(&list),
        resource(&source.resource),
        resource(&buffer),
    ) else {
        return false;
    };
    // SAFETY (whole function): live objects; the engine functions are fingerprinted by compatible().
    unsafe {
        if !compatible()
            || l.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT
            || !identity(&source.native)
            || source.native.resource != source.resource as u64
        {
            return false;
        }
        let desc = r.GetDesc();
        let destination = b.GetDesc();
        if !shape(&desc)
            || mip >= subresources(&desc)
            || destination.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
        {
            return false;
        }
        let (Ok(lc), Ok(bc), Ok(rc)) = (
            l.cast::<ID3D12DeviceChild>(),
            b.cast::<ID3D12DeviceChild>(),
            r.cast::<ID3D12DeviceChild>(),
        ) else {
            return false;
        };
        let (Some(a), Some(bd), Some(cd)) = (device_of(&lc), device_of(&bc), device_of(&rc)) else {
            return false;
        };
        if a.as_raw() != bd.as_raw() || a.as_raw() != cd.as_raw() {
            return false;
        }
        fail(71);
        let Some(expected) = subresource_layout(&a, &desc, mip, layout.Offset) else {
            return false;
        };
        if !same_layout(layout, &expected.layout)
            || footprint_range(layout, expected.rows, expected.row_bytes, destination.Width)
                .is_none()
        {
            return false;
        }
        fail(72);
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        let mut flags = D3D12_HEAP_FLAGS::default();
        if b.GetHeapProperties(Some(&mut heap), Some(&mut flags))
            .is_err()
            || (heap.Type != D3D12_HEAP_TYPE_DEFAULT && heap.Type != D3D12_HEAP_TYPE_READBACK)
            || (destination.Flags & !D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0
        {
            return false;
        }
        let base = game();
        let context = current_context(base);
        fail(21);
        if context.list != list as u64 {
            return false;
        }
        let implicit = source.receipt != 0;
        let single_state = alpha_format(desc.Format) || subresources(&desc) > 1;
        let required;
        if implicit {
            fail(42);
            if !validate(source, core::ptr::null_mut())
                || !default_heap(r)
                || !fixed_context(&context)
            {
                return false;
            }
            required = ALLOWED_READS;
        } else {
            let before = read_state(
                &source.native,
                context.context,
                context.list,
                READ,
                core::ptr::null_mut(),
            );
            if single_state && before.status != 1 {
                fail(30 + before.status);
                return false;
            }
            fail(30 + before.status);
            match copy_read_mask(before) {
                Some(mask) => required = mask,
                None => return false,
            }
        }
        if !cvr_gpu::command_resources::keep(l, &[source.resource, buffer]) {
            return false;
        }
        let register: Register = core::mem::transmute(base as usize + 0x337264);
        let flush: Flush = core::mem::transmute(base as usize + 0x3e3bb4);
        let _internal = cvr_host::probe::Internal::new();
        let native_context = context.context as *mut c_void;
        let current = || current_context(base) == context && identity(&source.native);
        let state_now = || {
            read_state(
                &source.native,
                context.context,
                context.list,
                READ,
                core::ptr::null_mut(),
            )
        };
        let recover = || {
            fail(40);
            if !current() {
                return false;
            }
            let mut state: State = state_now();
            if state.status == 4 {
                flush(native_context);
                state = state_now();
            }
            if (state.status != 1 && state.status != 2 && state.status != 5) || !current() {
                return false;
            }
            register(native_context, source.native.handle, SHADER_READS, u32::MAX);
            flush(native_context);
            state = state_now();
            if current()
                && (!single_state || state.status == 1)
                && has_read_state(state, SHADER_READS)
            {
                fail(41);
            }
            false
        };
        fail(22);
        if !current() {
            return false;
        }
        if implicit {
            fail(42);
            if !validate(source, core::ptr::null_mut()) || !fixed_context(&context) {
                return false;
            }
            // Flag 0x40 still registers native allocation use before bypassing state tracking. COMMON / cumulative
            // read promotion needs no source barrier.
            fail(40);
            register(native_context, source.native.handle, required, u32::MAX);
            flush(native_context);
            fail(40);
            if !current() || !fixed_context(&context) || !validate(source, core::ptr::null_mut()) {
                return false;
            }
        } else {
            // 0x1F42D8 keeps an existing superset for a read request: shader permissions retained, the combined state
            // left in native tracking.
            fail(40);
            register(native_context, source.native.handle, required, u32::MAX);
            flush(native_context);
            let copied = state_now();
            if !current()
                || (single_state && copied.status != 1)
                || !has_read_state(copied, required)
            {
                return recover();
            }
        }
        let from = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(&source.resource),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: mip,
            },
        };
        let to = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(&buffer),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: *layout,
            },
        };
        l.CopyTextureRegion(&to, 0, 0, 0, &from, None);
        fail(0);
        true
    }
}

// ---- the C ABI (src/Render/NativeTextureRead.cpp in the Rust build)

#[no_mangle]
pub extern "C" fn cvr_tr_failure() -> u32 {
    FAILURE.get()
}
#[no_mangle]
pub extern "C" fn cvr_tr_set_failure(code: u32) {
    fail(code);
}
/// # Safety
/// `native` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_identity(native: *const NativeTexture) -> bool {
    // SAFETY: as the caller promises.
    identity(unsafe { &*native })
}
/// # Safety
/// `resource` is live or null; `out` receives the record.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_resolve_native(
    resource_ptr: *mut c_void,
    out: *mut NativeTexture,
) -> bool {
    let Some(r) = resource(&resource_ptr) else {
        return false;
    };
    // SAFETY: as the caller promises.
    match resolve_native(r) {
        Some(native) => {
            unsafe { *out = native };
            true
        }
        None => false,
    }
}
/// # Safety
/// `native` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_fixed_owner_of(native: *const NativeTexture) -> u64 {
    // SAFETY: as the caller promises.
    fixed_owner(unsafe { &*native })
}
/// # Safety
/// `context` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_fixed_context(context: *const UploadContext) -> bool {
    // SAFETY: as the caller promises.
    fixed_context(unsafe { &*context })
}
#[no_mangle]
pub extern "C" fn cvr_tr_compatible() -> bool {
    compatible()
}
/// # Safety
/// `source` is valid; `queue` live or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_validate(source: *const Lease, queue: *mut c_void) -> bool {
    // SAFETY: as the caller promises.
    validate(unsafe { &*source }, queue)
}
/// # Safety
/// `list` and `resource` are live or null; `out` receives the lease, its resource borrowed (the face holds it).
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_acquire(
    list: *mut c_void,
    resource_ptr: *mut c_void,
    out: *mut Lease,
) -> bool {
    // SAFETY: as the caller promises.
    acquire(list, resource_ptr, unsafe { &mut *out })
}
/// # Safety
/// `device` live or null; `source` and `view` valid; `out` holds `capacity` layouts; `count` receives how many.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_view_footprints(
    device: *mut c_void,
    source: *const Lease,
    view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    offset: u64,
    out: *mut MipFootprint,
    capacity: usize,
    count: *mut usize,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { *count = 0 };
    let Some(layouts) = view_footprints(device, unsafe { &*source }, unsafe { &*view }, offset)
    else {
        return false;
    };
    if layouts.len() > capacity {
        fail(70);
        return false;
    }
    // SAFETY: `layouts.len()` values fit in the caller's buffer.
    unsafe {
        core::ptr::copy_nonoverlapping(layouts.as_ptr(), out, layouts.len());
        *count = layouts.len();
    }
    true
}
/// # Safety
/// `list` and `buffer` live or null; `source` and `layout` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_copy_subresource(
    list: *mut c_void,
    source: *const Lease,
    mip: u32,
    buffer: *mut c_void,
    layout: *const D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
) -> bool {
    // SAFETY: as the caller promises.
    copy_subresource(list, unsafe { &*source }, mip, buffer, unsafe { &*layout })
}
