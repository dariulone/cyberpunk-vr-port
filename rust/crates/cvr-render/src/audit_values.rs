//! The descriptor audit's canonical descriptor values and object cookies (src/Render/StereoDescriptorAudit.cpp,
//! `descriptors::`): a CBV (kind 1: address, size), an SRV (kind 2: the resource's cookie, the view's format, dimension,
//! mapping and dimension fields, the resource's shape, heap and buffer address), a UAV (kind 3: the resource's and the
//! counter's cookies, the view's format, dimension and fields) and a sampler (kind 4: every field, floats as bits) --
//! an unknown dimension, a resource without a cookie or a null view of no resource is kind 0 -- and the per-object
//! cookie held as COM private data (a reused address never inherits a destroyed object's identity).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp's Cookie and the value encodings in Cbv, Srv, Uav and Sampler forward to
//! `cvr_sda_*`; `tools/single_pass_tests/descriptor_audit_test` runs its cases against them in its Rust variant.

use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;

use crate::descriptor_store::Value;

const COOKIE_GUID: GUID = GUID::from_values(0xc7ec4301, 0x790e, 0x4b32, [0x93, 0x87, 0x7c, 0xc1, 0x19, 0x52, 0xd1, 0x44]);
static NEXT_COOKIE: AtomicU64 = AtomicU64::new(1);

/// Cookie: the object's identity for its lifetime (0 when it cannot hold one).
pub fn cookie(raw: *mut c_void) -> u64 {
    // SAFETY: a live D3D12 object of the caller's, or null.
    let Some(object) = (unsafe { ID3D12Object::from_raw_borrowed(&raw) }) else {
        return 0;
    };
    let mut value = 0u64;
    let mut bytes = 8u32;
    // SAFETY: room for the cookie.
    if unsafe { object.GetPrivateData(&COOKIE_GUID, &mut bytes, Some(&mut value as *mut u64 as *mut c_void)) }.is_ok() && bytes == 8 && value != 0 {
        return value;
    }
    value = NEXT_COOKIE.fetch_add(1, Relaxed);
    // SAFETY: the cookie's 8 bytes.
    if unsafe { object.SetPrivateData(&COOKIE_GUID, 8, Some(&value as *const u64 as *const c_void)) }.is_ok() {
        value
    } else {
        0
    }
}

/// The object's cookie when it holds one (never assigns one).
pub fn held_cookie(object: &ID3D12Object) -> Option<u64> {
    let mut value = 0u64;
    let mut bytes = 8u32;
    // SAFETY: room for the cookie.
    (unsafe { object.GetPrivateData(&COOKIE_GUID, &mut bytes, Some(&mut value as *mut u64 as *mut c_void)) }.is_ok() && bytes == 8).then_some(value)
}

fn bits(f: f32) -> u64 {
    f.to_bits() as u64
}

/// The CBV's value (kind 1), or the empty value for a null view.
pub fn cbv_value(desc: Option<&D3D12_CONSTANT_BUFFER_VIEW_DESC>) -> Value {
    let mut v: Value = [0; 20];
    if let Some(d) = desc {
        v[0] = 1;
        v[4] = d.BufferLocation;
        v[5] = d.SizeInBytes as u64;
    }
    v
}

/// DescribeSrvResource: the resource's shape (flag 4), heap (flag 8) and buffer address.
fn describe_srv_resource(raw: *mut c_void, v: &mut Value) {
    // SAFETY: a live resource of the caller's, or null.
    let Some(r) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return;
    };
    // SAFETY (whole block): a live resource.
    unsafe {
        let d = r.GetDesc();
        v[1] |= 4;
        v[13] = d.Dimension.0 as u32 as u64 | ((d.Format.0 as u32 as u64) << 32);
        v[14] = d.Width;
        v[15] = d.Height as u64 | ((d.DepthOrArraySize as u64) << 32) | ((d.MipLevels as u64) << 48);
        v[16] = d.Flags.0 as u32 as u64 | ((d.Layout.0 as u32 as u64) << 32);
        v[17] = d.SampleDesc.Count as u64 | ((d.SampleDesc.Quality as u64) << 32);
        let mut heap = D3D12_HEAP_PROPERTIES::default();
        let mut flags = D3D12_HEAP_FLAGS::default();
        if r.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_ok() {
            v[1] |= 8;
            v[18] = heap.Type.0 as u32 as u64 | ((heap.CPUPageProperty.0 as u32 as u64) << 16) | ((heap.MemoryPoolPreference.0 as u32 as u64) << 32);
        }
        if d.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER {
            v[19] = r.GetGPUVirtualAddress();
        }
    }
}

/// The SRV's value (kind 2) for the resource with its `cookie`.
pub fn srv_value(resource: *mut c_void, cookie: u64, desc: Option<&D3D12_SHADER_RESOURCE_VIEW_DESC>) -> Value {
    let mut v: Value = [0; 20];
    v[0] = 2;
    v[2] = cookie;
    v[1] = !resource.is_null() as u64;
    if !resource.is_null() && cookie == 0 {
        v[0] = 0;
    }
    match desc {
        None => {
            v[1] |= 2;
            if resource.is_null() {
                v[0] = 0;
            }
        }
        Some(d) => {
            v[4] = d.Format.0 as u32 as u64;
            v[5] = d.ViewDimension.0 as u32 as u64;
            v[6] = d.Shader4ComponentMapping as u64;
            // SAFETY (whole block): the union member the dimension names.
            unsafe {
                let a = &d.Anonymous;
                match d.ViewDimension {
                    D3D12_SRV_DIMENSION_BUFFER => {
                        let b = a.Buffer;
                        v[7] = b.FirstElement;
                        v[8] = b.NumElements as u64;
                        v[9] = b.StructureByteStride as u64;
                        v[10] = b.Flags.0 as u32 as u64;
                    }
                    D3D12_SRV_DIMENSION_TEXTURE1D => {
                        let t = a.Texture1D;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_TEXTURE1DARRAY => {
                        let t = a.Texture1DArray;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = t.FirstArraySlice as u64;
                        v[10] = t.ArraySize as u64;
                        v[11] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_TEXTURE2D => {
                        let t = a.Texture2D;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = t.PlaneSlice as u64;
                        v[10] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_TEXTURE2DARRAY => {
                        let t = a.Texture2DArray;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = t.FirstArraySlice as u64;
                        v[10] = t.ArraySize as u64;
                        v[11] = t.PlaneSlice as u64;
                        v[12] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_TEXTURE2DMS => {}
                    D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY => {
                        let t = a.Texture2DMSArray;
                        v[7] = t.FirstArraySlice as u64;
                        v[8] = t.ArraySize as u64;
                    }
                    D3D12_SRV_DIMENSION_TEXTURE3D => {
                        let t = a.Texture3D;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_TEXTURECUBE => {
                        let t = a.TextureCube;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_TEXTURECUBEARRAY => {
                        let t = a.TextureCubeArray;
                        v[7] = t.MostDetailedMip as u64;
                        v[8] = t.MipLevels as u64;
                        v[9] = t.First2DArrayFace as u64;
                        v[10] = t.NumCubes as u64;
                        v[11] = bits(t.ResourceMinLODClamp);
                    }
                    D3D12_SRV_DIMENSION_RAYTRACING_ACCELERATION_STRUCTURE => {
                        v[7] = a.RaytracingAccelerationStructure.Location;
                    }
                    _ => v[0] = 0,
                }
            }
        }
    }
    if v[0] != 0 {
        describe_srv_resource(resource, &mut v);
    }
    v
}

/// The UAV's value (kind 3) for the resource and counter with their cookies.
pub fn uav_value(resource: *mut c_void, counter: *mut c_void, cookie: u64, counter_cookie: u64, desc: Option<&D3D12_UNORDERED_ACCESS_VIEW_DESC>) -> Value {
    let mut v: Value = [0; 20];
    v[0] = 3;
    v[2] = cookie;
    v[3] = counter_cookie;
    v[1] = !resource.is_null() as u64;
    if (!resource.is_null() && cookie == 0) || (!counter.is_null() && counter_cookie == 0) {
        v[0] = 0;
    }
    match desc {
        None => {
            v[1] |= 2;
            if resource.is_null() {
                v[0] = 0;
            }
        }
        Some(d) => {
            v[4] = d.Format.0 as u32 as u64;
            v[5] = d.ViewDimension.0 as u32 as u64;
            // SAFETY (whole block): the union member the dimension names.
            unsafe {
                let a = &d.Anonymous;
                match d.ViewDimension {
                    D3D12_UAV_DIMENSION_BUFFER => {
                        let b = a.Buffer;
                        v[7] = b.FirstElement;
                        v[8] = b.NumElements as u64;
                        v[9] = b.StructureByteStride as u64;
                        v[10] = b.CounterOffsetInBytes;
                        v[11] = b.Flags.0 as u32 as u64;
                    }
                    D3D12_UAV_DIMENSION_TEXTURE1D => v[7] = a.Texture1D.MipSlice as u64,
                    D3D12_UAV_DIMENSION_TEXTURE1DARRAY => {
                        let t = a.Texture1DArray;
                        v[7] = t.MipSlice as u64;
                        v[8] = t.FirstArraySlice as u64;
                        v[9] = t.ArraySize as u64;
                    }
                    D3D12_UAV_DIMENSION_TEXTURE2D => {
                        let t = a.Texture2D;
                        v[7] = t.MipSlice as u64;
                        v[8] = t.PlaneSlice as u64;
                    }
                    D3D12_UAV_DIMENSION_TEXTURE2DARRAY => {
                        let t = a.Texture2DArray;
                        v[7] = t.MipSlice as u64;
                        v[8] = t.FirstArraySlice as u64;
                        v[9] = t.ArraySize as u64;
                        v[10] = t.PlaneSlice as u64;
                    }
                    D3D12_UAV_DIMENSION_TEXTURE3D => {
                        let t = a.Texture3D;
                        v[7] = t.MipSlice as u64;
                        v[8] = t.FirstWSlice as u64;
                        v[9] = t.WSize as u64;
                    }
                    _ => v[0] = 0,
                }
            }
        }
    }
    v
}

/// The sampler's value (kind 4), or the empty value for a null description.
pub fn sampler_value(desc: Option<&D3D12_SAMPLER_DESC>) -> Value {
    let mut v: Value = [0; 20];
    if let Some(d) = desc {
        v[0] = 4;
        v[4] = d.Filter.0 as u32 as u64;
        v[5] = d.AddressU.0 as u32 as u64;
        v[6] = d.AddressV.0 as u32 as u64;
        v[7] = d.AddressW.0 as u32 as u64;
        v[8] = bits(d.MipLODBias);
        v[9] = d.MaxAnisotropy as u64;
        v[10] = d.ComparisonFunc.0 as u32 as u64;
        for i in 0..4 {
            v[11 + i] = bits(d.BorderColor[i]);
        }
        v[15] = bits(d.MinLOD);
        v[16] = bits(d.MaxLOD);
    }
    v
}

// ---- the C ABI

/// # Safety
/// `object` is a live D3D12 object or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_cookie(object: *mut c_void) -> u64 {
    cookie(object)
}
/// The value of one descriptor creation: `kind` 1 CBV, 2 SRV, 3 UAV, 4 sampler; `desc` that kind's description or
/// null; the resource and counter with their cookies where the kind has them.
///
/// # Safety
/// `desc` is valid for the kind or null; the objects are live or null; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_value(
    kind: u32,
    desc: *const c_void,
    resource: *mut c_void,
    cookie: u64,
    counter: *mut c_void,
    counter_cookie: u64,
    out: *mut Value,
) {
    let value = unsafe {
        match kind {
            1 => cbv_value(desc.cast::<D3D12_CONSTANT_BUFFER_VIEW_DESC>().as_ref()),
            2 => srv_value(resource, cookie, desc.cast::<D3D12_SHADER_RESOURCE_VIEW_DESC>().as_ref()),
            3 => uav_value(resource, counter, cookie, counter_cookie, desc.cast::<D3D12_UNORDERED_ACCESS_VIEW_DESC>().as_ref()),
            _ => sampler_value(desc.cast::<D3D12_SAMPLER_DESC>().as_ref()),
        }
    };
    unsafe { *out = value };
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

    #[test]
    fn null_views_and_unknown_dimensions() {
        assert_eq!(srv_value(core::ptr::null_mut(), 0, None)[..3], [0, 2, 0], "a null view of no resource is unknown");
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC { Format: DXGI_FORMAT_R8G8B8A8_UNORM, ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D, ..Default::default() };
        view.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        view.Anonymous.Texture2D = D3D12_TEX2D_SRV { MostDetailedMip: 1, MipLevels: 2, PlaneSlice: 0, ResourceMinLODClamp: 0.5 };
        let v = srv_value(core::ptr::null_mut(), 0, Some(&view));
        assert_eq!(v[..11], [2, 0, 0, 0, 28, 4, 0x1688, 1, 2, 0, 0.5f32.to_bits() as u64], "an explicit typed null SRV");
        view.ViewDimension = D3D12_SRV_DIMENSION(99);
        assert_eq!(srv_value(core::ptr::null_mut(), 0, Some(&view))[0], 0);
        let sampler = D3D12_SAMPLER_DESC { MaxAnisotropy: 8, BorderColor: [1.0, 0.0, 0.0, 1.0], MaxLOD: 3.0, ..Default::default() };
        let v = sampler_value(Some(&sampler));
        assert!(v[0] == 4 && v[9] == 8 && v[11] == 1.0f32.to_bits() as u64 && v[16] == 3.0f32.to_bits() as u64);
        assert_eq!(cbv_value(None), [0; 20]);
    }
}
