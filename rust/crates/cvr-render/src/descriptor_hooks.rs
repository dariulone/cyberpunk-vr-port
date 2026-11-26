//! The device's descriptor hooks (formerly src/Hooks/DepthCapture.cpp's AuditSrv / AuditUav / AuditSampler,
//! HookedCopyDescriptors(Simple), HookedCreateCommandSignature, InstallDepthCaptureHooks and
//! DescriptorMutationHooksReady): each view creation and descriptor copy runs the original inside the descriptor audit's
//! mutation scope, then tells the audit, the registered intent (structured views), the mono cascades and the hidden
//! area (DSV copies) and the stage replay; a command signature is told to the cascades and gets the indirect policy's
//! receipt. The depth-stencil view hook itself is depth_capture.rs's cvr_depth_create_dsv.

use core::ffi::c_void;
use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::*;

type Handle = D3D12_CPU_DESCRIPTOR_HANDLE;
type CreateSrv = unsafe extern "system" fn(*mut c_void, *mut c_void, *const D3D12_SHADER_RESOURCE_VIEW_DESC, Handle);
type CreateUav = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const D3D12_UNORDERED_ACCESS_VIEW_DESC, Handle);
type CreateSampler = unsafe extern "system" fn(*mut c_void, *const D3D12_SAMPLER_DESC, Handle);
type CopyDescriptors = unsafe extern "system" fn(*mut c_void, u32, *const Handle, *const u32, u32, *const Handle, *const u32, D3D12_DESCRIPTOR_HEAP_TYPE);
type CopySimple = unsafe extern "system" fn(*mut c_void, u32, Handle, Handle, D3D12_DESCRIPTOR_HEAP_TYPE);
type CreateSignature = unsafe extern "system" fn(*mut c_void, *const D3D12_COMMAND_SIGNATURE_DESC, *mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;

const SRV: usize = 18;
const UAV: usize = 19;
const DSV: usize = 21;
const SAMPLER: usize = 22;
const COPY: usize = 23;
const COPY_SIMPLE: usize = 24;
const SIGNATURE: usize = 41;
/// The indirect policy's receipt in a command signature's private data (src/Render/NativeIndirectPolicy.cpp).
const RECEIPT_KEY: GUID = GUID::from_values(0x6e80df9a, 0x3c21, 0x4dc7, [0x9d, 0x81, 0x35, 0x4f, 0x90, 0xaa, 0x17, 0x61]);
/// indirect_policy::detail::Receipt.
#[repr(C)]
struct Receipt {
    version: u32,
    kind: u32,
    stride: u32,
    node_mask: u32,
}

extern "C" {
    fn cvr_sri_mark_structured_view(resource: *mut c_void, view: *const D3D12_SHADER_RESOURCE_VIEW_DESC, enabled: *const core::sync::atomic::AtomicU32) -> bool;
    fn cvr_mono_cascades_copy_descriptors(device: *mut c_void, destinations: u32, dest: *const Handle, sizes: *const u32, sources: u32, src: *const Handle, counts: *const u32, kind: i32);
    fn cvr_mono_cascades_signature_created(signature: *mut c_void, desc: *const D3D12_COMMAND_SIGNATURE_DESC);
    fn cvr_hidden_area_copy_descriptors(device: *mut c_void, destinations: u32, dest: *const Handle, sizes: *const u32, sources: u32, src: *const Handle, counts: *const u32, kind: i32);
    fn cvr_srt_copy(n: u32, dest: *const Handle, sizes: *const u32, m: u32, src: *const Handle, counts: *const u32, kind: i32);
    fn cvr_sda_audit_copy(device: *mut c_void, n: u32, dest: *const Handle, sizes: *const u32, m: u32, source: *const Handle, counts: *const u32, kind: i32);
    fn cvr_indirect_classify(desc: *const D3D12_COMMAND_SIGNATURE_DESC) -> u32;
    fn cvr_depth_create_dsv(device: *mut c_void, resource: *mut c_void, desc: *const D3D12_DEPTH_STENCIL_VIEW_DESC, dest: Handle);
    /// g_verboseLog (C++ linkage; defined by cvr-ffi vrcore_exports.rs under this symbol).
    #[link_name = "?g_verboseLog@@3HC"]
    static g_verboseLog: i32;
}

/// descriptors::MutationScope: the audit's borrowed ranges told a descriptor mutates (left when it was entered).
struct Mutation(bool);
impl Mutation {
    fn new() -> Self {
        Mutation(crate::descriptor_audit::tracking_call(4, 0))
    }
}
impl Drop for Mutation {
    fn drop(&mut self) {
        if self.0 {
            crate::descriptor_audit::tracking_call(5, 0);
        }
    }
}
fn table(device: *mut c_void) -> *mut *mut c_void {
    // SAFETY: a live COM object's vtable pointer.
    unsafe { *device.cast::<*mut *mut c_void>() }
}
fn original<F: Copy>(device: *mut c_void, slot: usize) -> Option<F> {
    let p = cvr_engine::vtable::original(table(device), slot);
    // SAFETY: the slot's recorded original, of the slot's signature.
    (!p.is_null()).then(|| unsafe { core::mem::transmute_copy::<*mut c_void, F>(&p) })
}
/// descriptors::Enabled: the native probe observes bindings, outside our own commands.
fn audit_enabled() -> bool {
    crate::stereo_probe::cvr_observe_bindings() && !cvr_host::probe::internal()
}

unsafe extern "system" fn audit_srv(device: *mut c_void, resource: *mut c_void, desc: *const D3D12_SHADER_RESOURCE_VIEW_DESC, dest: Handle) {
    let _mutation = Mutation::new();
    if let Some(o) = original::<CreateSrv>(device, SRV) {
        // SAFETY: the original over the caller's arguments.
        unsafe { o(device, resource, desc, dest) };
    }
    // SAFETY: the caller's view; the exported switch.
    unsafe {
        if !cvr_host::probe::internal() && cvr_cpp_registered_intent_tracking() {
            cvr_sri_mark_structured_view(resource, desc, &crate::native_state_exports::CyberpunkVR_RegisteredIntentEnable);
        }
        crate::descriptor_audit::cvr_sda_audit_write(4, core::ptr::null_mut(), desc as usize, dest.ptr as u64, 0, 0, resource, core::ptr::null_mut());
    }
}
unsafe extern "system" fn audit_uav(device: *mut c_void, resource: *mut c_void, counter: *mut c_void, desc: *const D3D12_UNORDERED_ACCESS_VIEW_DESC, dest: Handle) {
    let _mutation = Mutation::new();
    if let Some(o) = original::<CreateUav>(device, UAV) {
        // SAFETY: the original over the caller's arguments.
        unsafe { o(device, resource, counter, desc, dest) };
    }
    // SAFETY: the caller's view.
    unsafe { crate::descriptor_audit::cvr_sda_audit_write(5, core::ptr::null_mut(), desc as usize, dest.ptr as u64, 0, 0, resource, counter) };
}
unsafe extern "system" fn audit_sampler(device: *mut c_void, desc: *const D3D12_SAMPLER_DESC, dest: Handle) {
    let _mutation = Mutation::new();
    if let Some(o) = original::<CreateSampler>(device, SAMPLER) {
        // SAFETY: the original over the caller's arguments.
        unsafe { o(device, desc, dest) };
    }
    // SAFETY: the caller's sampler.
    unsafe { crate::descriptor_audit::cvr_sda_audit_write(6, core::ptr::null_mut(), desc as usize, dest.ptr as u64, 0, 0, core::ptr::null_mut(), core::ptr::null_mut()) };
}
unsafe extern "system" fn create_command_signature(
    device: *mut c_void,
    desc: *const D3D12_COMMAND_SIGNATURE_DESC,
    root: *mut c_void,
    id: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    let Some(o) = original::<CreateSignature>(device, SIGNATURE) else { return windows::Win32::Foundation::E_FAIL };
    // SAFETY: the original over the caller's arguments.
    let result = unsafe { o(device, desc, root, id, out) };
    // SAFETY: the created signature and the caller's description.
    unsafe {
        if result.is_ok() && !desc.is_null() && !out.is_null() && !(*out).is_null() {
            let signature = *out;
            cvr_mono_cascades_signature_created(signature, desc);
            // indirect_policy::Created: the policy's receipt in the signature's private data.
            if !cvr_host::probe::internal() && crate::stereo_probe::cvr_observe_bindings() {
                let d = &*desc;
                let receipt = Receipt { version: 1, kind: cvr_indirect_classify(desc), stride: d.ByteStride, node_mask: d.NodeMask };
                if let Some(s) = ID3D12CommandSignature::from_raw_borrowed(&signature) {
                    let _ = s.SetPrivateData(&RECEIPT_KEY, core::mem::size_of::<Receipt>() as u32, Some((&raw const receipt).cast()));
                }
            }
        }
    }
    result
}
/// The copies' consumers: the cascades and the hidden area for DSV copies, then the audit and the stage replay.
unsafe fn copies(device: *mut c_void, n: u32, dest: *const Handle, sizes: *const u32, m: u32, source: *const Handle, counts: *const u32, kind: D3D12_DESCRIPTOR_HEAP_TYPE) {
    // SAFETY: the caller's descriptor ranges.
    unsafe {
        if kind == D3D12_DESCRIPTOR_HEAP_TYPE_DSV {
            cvr_mono_cascades_copy_descriptors(device, n, dest, sizes, m, source, counts, kind.0);
            cvr_hidden_area_copy_descriptors(device, n, dest, sizes, m, source, counts, kind.0);
        }
        cvr_sda_audit_copy(device, n, dest, sizes, m, source, counts, kind.0);
        cvr_srt_copy(n, dest, sizes, m, source, counts, kind.0);
    }
}
unsafe extern "system" fn copy_descriptors(
    device: *mut c_void,
    n: u32,
    dest: *const Handle,
    sizes: *const u32,
    m: u32,
    source: *const Handle,
    counts: *const u32,
    kind: D3D12_DESCRIPTOR_HEAP_TYPE,
) {
    let _mutation = Mutation::new();
    if let Some(o) = original::<CopyDescriptors>(device, COPY) {
        // SAFETY: the original over the caller's arguments.
        unsafe { o(device, n, dest, sizes, m, source, counts, kind) };
    }
    // SAFETY: as above.
    unsafe { copies(device, n, dest, sizes, m, source, counts, kind) };
}
unsafe extern "system" fn copy_descriptors_simple(device: *mut c_void, n: u32, dest: Handle, source: Handle, kind: D3D12_DESCRIPTOR_HEAP_TYPE) {
    let _mutation = Mutation::new();
    if let Some(o) = original::<CopySimple>(device, COPY_SIMPLE) {
        // SAFETY: the original over the caller's arguments.
        unsafe { o(device, n, dest, source, kind) };
    }
    // SAFETY: one range of `n` descriptors each side.
    unsafe { copies(device, 1, &dest, &n, 1, &source, &n, kind) };
}

/// InstallDepthCaptureHooks: the audit's view hooks (when the audit is on, after its initialisation), the copies, the
/// command signatures and the depth-stencil view hook.
pub fn install(device: *mut c_void) {
    if device.is_null() {
        return;
    }
    let vtable = table(device);
    // SAFETY: the device's live vtable; our hooks of each slot's signature.
    unsafe {
        if audit_enabled() {
            crate::descriptor_audit::cvr_sda_audit_write(0, device, 0, 0, 0, 0, core::ptr::null_mut(), core::ptr::null_mut());
            cvr_engine::vtable::patch(vtable, SRV, audit_srv as *mut c_void);
            cvr_engine::vtable::patch(vtable, UAV, audit_uav as *mut c_void);
            cvr_engine::vtable::patch(vtable, SAMPLER, audit_sampler as *mut c_void);
        }
        cvr_engine::vtable::patch(vtable, COPY, copy_descriptors as *mut c_void);
        cvr_engine::vtable::patch(vtable, COPY_SIMPLE, copy_descriptors_simple as *mut c_void);
        cvr_engine::vtable::patch(vtable, SIGNATURE, create_command_signature as *mut c_void);
        if cvr_engine::vtable::patch(vtable, DSV, cvr_depth_create_dsv as *mut c_void) && core::ptr::read_volatile(&raw const g_verboseLog) != 0 {
            cvr_host::log::write("[DEPTH-DIAG] Hooked ID3D12Device::CreateDepthStencilView (slot 21)\n");
        }
    }
}
/// DescriptorMutationHooksReady: every view and copy slot holds our hook over a recorded original.
pub fn ready(device: *mut c_void) -> bool {
    if device.is_null() {
        return false;
    }
    let vtable = table(device);
    let ours = |slot: usize, hook: *mut c_void| {
        // SAFETY: the device's live vtable.
        (unsafe { *vtable.add(slot) == hook }) && !cvr_engine::vtable::original(vtable, slot).is_null()
    };
    ours(SRV, audit_srv as *mut c_void)
        && ours(UAV, audit_uav as *mut c_void)
        && ours(SAMPLER, audit_sampler as *mut c_void)
        && ours(COPY, copy_descriptors as *mut c_void)
        && ours(COPY_SIMPLE, copy_descriptors_simple as *mut c_void)
}

#[no_mangle]
pub extern "C" fn cvr_dh_install_descriptor_hooks(device: *mut c_void) {
    install(device)
}
#[no_mangle]
pub extern "C" fn cvr_dh_descriptor_hooks_ready(device: *mut c_void) -> bool {
    ready(device)
}
const _: () = assert!(core::mem::size_of::<Receipt>() == 16);

// constants::* (formerly src/Render/RegisteredConstantUpload.cpp's entry points): registered_copies.rs' entries.
unsafe fn cvr_cpp_registered_intent_tracking() -> bool {
    crate::registered_copies::entries::cvr_rc2_intent_configured()
}
