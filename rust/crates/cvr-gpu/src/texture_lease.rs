//! THE ENGINE TEXTURE LEASE (formerly src/Natives/HudPanel.cpp TextureOf and cvr::hud::TextureLease's release): an engine
//! colour target held for one of the port's own reads. Its pool handle is referenced, so the resource -- and the
//! placed-resource heap behind it -- outlives the engine's release while the read is in flight. Only a single-sample 2D
//! RGBA target resting in the engine's shader-read state is leased; with `idle`, one in COMMON too: an ink target the
//! engine leaves to decay after each write (Natives/MenuSurface.cpp), which a read promotes implicitly.
//!
//! The engine's layout, read as the C++ half read it (the game build the port supports): a DynamicTexture's backing at
//! +0x70 (its vtable checked), the backing's handle at +0x18, the texture pool's entries of 0xB0 bytes behind the pool
//! pointer, an entry's reference count at +0x2F1D0, its resource at +0x2F1D8 and its state at +0x2F220; the handle's
//! release is the engine's own function.
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

use cvr_engine::code::{image_base, read};
use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D12::{ID3D12Resource, D3D12_RESOURCE_DIMENSION_TEXTURE2D};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R16G16B16A16_TYPELESS,
    DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};

const TEXTURE_BACKING: usize = 0x70;
const TEXTURE_WIDTH: usize = 0x40;
const TEXTURE_HEIGHT: usize = 0x44;
const BACKING_VTABLE: usize = 0x30646a8;
const BACKING_HANDLE: usize = 0x18;
const TEXTURE_POOL: usize = 0x3bcdca8;
const ENTRY_SIZE: usize = 0xb0;
const ENTRY_REFS: usize = 0x2f1d0;
const ENTRY_RESOURCE: usize = 0x2f1d8;
const ENTRY_STATE: usize = 0x2f220;
const RELEASE: usize = 0x3e3248;
const MAX_HANDLE: u32 = 131072;
/// D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | NON_PIXEL_SHADER_RESOURCE.
const SHADER_READ: u32 = 0xc0;

/// The lease's end: one reference of the engine's handle released through the engine's own function.
#[no_mangle]
pub extern "C" fn cvr_hud_texture_release(handle: u32) {
    if handle == 0 {
        return;
    }
    let mut h = handle;
    // SAFETY: the engine's handle release, `void(uint32_t*)`, at its address in the supported build.
    unsafe {
        let release: extern "C" fn(*mut u32) = core::mem::transmute(image_base() + RELEASE);
        release(&mut h);
    }
}

/// A lease on `texture`'s (a RED4ext::DynamicTexture's) engine target: its handle referenced and the resource with one
/// reference of the caller's own (`handle`, `resource`); false, with nothing held, when the texture is not such a
/// target or not in a readable state.
///
/// # Safety
/// `texture` is a DynamicTexture or null; `handle` and `resource` are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_hud_texture_lease(
    texture: *mut c_void,
    idle: bool,
    handle: *mut u32,
    resource: *mut *mut c_void,
) -> bool {
    // SAFETY: the caller's out-parameters.
    unsafe {
        *handle = 0;
        *resource = core::ptr::null_mut();
    }
    if texture.is_null() {
        return false;
    }
    let texture = texture as usize;
    let base = image_base();
    let Some(backing) = read::<usize>(texture + TEXTURE_BACKING).filter(|&b| b != 0) else {
        return false;
    };
    if read::<usize>(backing) != Some(base + BACKING_VTABLE) {
        return false;
    }
    let Some(id) = read::<u32>(backing + BACKING_HANDLE).filter(|&id| id != 0 && id <= MAX_HANDLE)
    else {
        return false;
    };
    let Some(pool) = read::<usize>(base + TEXTURE_POOL).filter(|&p| p != 0) else {
        return false;
    };
    let entry = pool + ENTRY_SIZE * (id as usize - 1);
    // The handle referenced while it is alive (a count of 0 is a handle being released).
    // SAFETY: the engine's live pool entry; its count is a 32-bit interlocked value.
    let refs = unsafe { &*((entry + ENTRY_REFS) as *const AtomicI32) };
    let mut count = refs.load(Ordering::SeqCst);
    while count > 0 {
        match refs.compare_exchange(count, count + 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(observed) => count = observed,
        }
    }
    if count <= 0 {
        return false;
    }
    // Held from here: released on every way out but the lease.
    let refuse = || {
        cvr_hud_texture_release(id);
        false
    };
    let raw = read::<*mut c_void>(entry + ENTRY_RESOURCE).unwrap_or(core::ptr::null_mut());
    // SAFETY: the engine's resource, borrowed.
    let Some(target) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return refuse();
    };
    // SAFETY: a live resource.
    let desc = unsafe { target.GetDesc() };
    let format_ok = [
        DXGI_FORMAT_R8G8B8A8_TYPELESS,
        DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R16G16B16A16_TYPELESS,
    ]
    .contains(&desc.Format);
    let width = read::<u32>(texture + TEXTURE_WIDTH);
    let height = read::<u32>(texture + TEXTURE_HEIGHT);
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.SampleDesc.Count != 1
        || !format_ok
        || width.map(u64::from) != Some(desc.Width)
        || height != Some(desc.Height)
    {
        return refuse();
    }
    let state = read::<u32>(entry + ENTRY_STATE);
    if state != Some(SHADER_READ) && !(idle && state == Some(0)) {
        return refuse();
    }
    // SAFETY: the caller's out-parameters; the resource with a reference of its own.
    unsafe {
        *handle = id;
        *resource = target.clone().into_raw();
    }
    true
}
