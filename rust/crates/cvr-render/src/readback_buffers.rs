//! The native passes' private buffers (src/Render/NativePreparedPair.cpp's Readback, NativeRetainedGeometry.cpp's full T5
//! readback, Seal's CBV, T5 and instance readbacks and the instance writer's readback and predicate, NativeEarlyDepth.cpp's
//! material readback and permit): a committed buffer of a heap type and initial state -- a plain row-major buffer, or
//! shaped as another resource's description with its width changed --; an UPLOAD word mapped, zeroed and kept mapped; an
//! UPLOAD buffer zeroed through its mapping.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpps create these through `cvr_rb_*` (each object handed over with its
//! creation's reference); the unit tests below create each on WARP.

use core::ffi::c_void;
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live D3D12 object the caller holds for the call.
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
/// A plain row-major buffer's description.
pub fn buffer_desc(bytes: u64) -> D3D12_RESOURCE_DESC {
    D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    }
}
/// A committed buffer: plain, or `like`'s description with `bytes` as its width.
pub fn buffer(device: &ID3D12Device, like: Option<&ID3D12Resource>, bytes: u64, heap: D3D12_HEAP_TYPE, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource, HRESULT> {
    let desc = match like {
        // SAFETY: a live resource.
        Some(resource) => D3D12_RESOURCE_DESC { Width: bytes, ..unsafe { resource.GetDesc() } },
        None => buffer_desc(bytes),
    };
    let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; the descriptions are locals.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.map_err(|e| e.code())?;
    out.ok_or(E_FAIL)
}
/// An 8-byte UPLOAD word, mapped, zeroed and published; the mapping kept.
pub fn mapped_word(device: &ID3D12Device) -> Result<(ID3D12Resource, *mut c_void), HRESULT> {
    let word = buffer(device, None, 8, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ)?;
    let mut mapped = core::ptr::null_mut();
    // SAFETY: the new UPLOAD word.
    unsafe { word.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)) }.map_err(|e| e.code())?;
    if mapped.is_null() {
        return Err(E_FAIL);
    }
    // SAFETY: the mapped word; published before the GPU reads it.
    unsafe { core::ptr::write_volatile(mapped.cast::<u64>(), 0) };
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    Ok((word, mapped))
}
/// An UPLOAD buffer of `bytes`, mapped, zeroed and published; the mapping kept.
pub fn mapped_upload(device: &ID3D12Device, bytes: u64) -> Result<(ID3D12Resource, *mut c_void), HRESULT> {
    let upload = buffer(device, None, bytes, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ)?;
    let mut mapped = core::ptr::null_mut();
    // SAFETY: the new UPLOAD buffer.
    unsafe { upload.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)) }.map_err(|e| e.code())?;
    if mapped.is_null() {
        return Err(E_FAIL);
    }
    // SAFETY: the mapped buffer's bytes; published before the GPU reads them.
    unsafe { core::ptr::write_bytes(mapped.cast::<u8>(), 0, bytes as usize) };
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    Ok((upload, mapped))
}
/// Zeroes an UPLOAD buffer's first `bytes` through its mapping (false when it does not map).
pub fn zero(resource: &ID3D12Resource, bytes: usize) -> bool {
    let mut mapped = core::ptr::null_mut();
    // SAFETY: a live UPLOAD buffer of at least `bytes`.
    if unsafe { resource.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)) }.is_err() || mapped.is_null() {
        return false;
    }
    // SAFETY: as above, mapped.
    unsafe {
        core::ptr::write_bytes(mapped.cast::<u8>(), 0, bytes);
        resource.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: bytes }));
    }
    true
}

/// A committed buffer (`like` null: plain); `out` the new buffer with its reference, written only on success.
///
/// # Safety
/// The device (and `like`) are live; `out` a pointer slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rb_buffer(device: *mut c_void, like: *mut c_void, bytes: u64, heap: i32, state: i32, out: *mut *mut c_void) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    let like = borrowed::<ID3D12Resource>(like);
    match buffer(&device, like.as_ref(), bytes, D3D12_HEAP_TYPE(heap), D3D12_RESOURCE_STATES(state)) {
        Ok(resource) => {
            // SAFETY: as documented.
            unsafe { *out = resource.into_raw() };
            S_OK
        }
        Err(error) => error,
    }
}
/// An UPLOAD word mapped and zeroed: the word (with its reference) and its mapping.
///
/// # Safety
/// The device is live; the outputs are pointer slots.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rb_mapped_word(device: *mut c_void, out: *mut *mut c_void, mapped: *mut *mut c_void) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    match mapped_word(&device) {
        Ok((word, pointer)) => {
            // SAFETY: as documented.
            unsafe {
                *out = word.into_raw();
                *mapped = pointer;
            }
            S_OK
        }
        Err(error) => error,
    }
}
/// An UPLOAD buffer mapped and zeroed: the buffer (with its reference) and its mapping.
///
/// # Safety
/// The device is live; the outputs are pointer slots.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rb_mapped_upload(device: *mut c_void, bytes: u64, out: *mut *mut c_void, mapped: *mut *mut c_void) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    match mapped_upload(&device, bytes) {
        Ok((upload, pointer)) => {
            // SAFETY: as documented.
            unsafe {
                *out = upload.into_raw();
                *mapped = pointer;
            }
            S_OK
        }
        Err(error) => error,
    }
}
/// Zeroes an UPLOAD buffer's first bytes.
///
/// # Safety
/// `resource` is a live UPLOAD buffer of at least `bytes`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rb_zero(resource: *mut c_void, bytes: usize) -> bool {
    borrowed::<ID3D12Resource>(resource).is_some_and(|r| zero(&r, bytes))
}

#[cfg(test)]
mod tests {
    //! Each buffer on WARP.
    use super::*;

    fn warp() -> ID3D12Device {
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    #[test]
    fn buffers() {
        let device = warp();
        let readback = buffer(&device, None, 4096, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).unwrap();
        let desc = unsafe { readback.GetDesc() };
        assert_eq!((desc.Dimension, desc.Width, desc.Layout), (D3D12_RESOURCE_DIMENSION_BUFFER, 4096, D3D12_TEXTURE_LAYOUT_ROW_MAJOR));
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        unsafe { readback.GetHeapProperties(Some(&mut heap), None) }.unwrap();
        assert_eq!(heap.Type, D3D12_HEAP_TYPE_READBACK);
        let like = buffer(&device, Some(&readback), 512, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).unwrap();
        assert_eq!(unsafe { like.GetDesc() }.Width, 512, "the like's description at its own width");
        let mut out = core::ptr::null_mut();
        assert_eq!(unsafe { cvr_rb_buffer(device.as_raw(), core::ptr::null_mut(), 0, D3D12_HEAP_TYPE_READBACK.0, D3D12_RESOURCE_STATE_COPY_DEST.0, &mut out) }.is_err(), true, "no bytes");
        assert!(out.is_null(), "written only on success");
        let (word, mapped) = mapped_word(&device).unwrap();
        assert_eq!(unsafe { core::ptr::read_volatile(mapped.cast::<u64>()) }, 0);
        unsafe { core::ptr::write_volatile(mapped.cast::<u64>(), 5) };
        unsafe { word.Unmap(0, None) };
        let upload = buffer(&device, None, 64, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let mut pointer = core::ptr::null_mut();
        unsafe { upload.Map(0, None, Some(&mut pointer)) }.unwrap();
        unsafe { core::ptr::write_bytes(pointer.cast::<u8>(), 0xAB, 64) };
        unsafe { upload.Unmap(0, None) };
        assert!(zero(&upload, 64));
        let (wide, wide_pointer) = mapped_upload(&device, 1280).unwrap();
        assert!(unsafe { core::slice::from_raw_parts(wide_pointer as *const u8, 1280) }.iter().all(|&b| b == 0), "a mapped upload zeroed");
        unsafe { wide.Unmap(0, None) };
        unsafe { upload.Map(0, None, Some(&mut pointer)) }.unwrap();
        assert!(unsafe { core::slice::from_raw_parts(pointer as *const u8, 64) }.iter().all(|&b| b == 0), "zeroed through the mapping");
        unsafe { upload.Unmap(0, None) };
    }
}
