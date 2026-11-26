//! The descriptor audit's borrowed range inventory (src/Render/StereoDescriptorAudit.cpp,
//! `descriptors::InventoryBorrowedSourceRange`): under the metadata lock, each slot of a borrowed range classified from
//! its canonical value in the store (unknown, known null, resource SRV with its shape, other kinds; an SRV counts as a
//! resource SRV only when it decodes), the resource SRVs' cookies kept sorted and unique, and later each cookie counted
//! as tracked (a known resource) or untracked.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the inventory's slot walk forwards to `cvr_sda_inventory` on the .cpp's Rust store
//! and the cookie count to `cvr_sda_inventory_tracked`. No C++ test reaches the inventory (it needs a captured borrowed
//! range); the unit test below inventories a WARP heap's slots.

use crate::audit_lifetimes::resource_known;
use crate::descriptor_store::{inventory_borrowed_descriptor, BorrowedDescriptorInventory, RangeIdentity, Store};
use crate::descriptor_views::decode_srv;

/// The range's slots classified into `counts`; the resource SRVs' cookies, sorted and unique.
pub fn inventory(store: &Store, identity: &RangeIdentity, counts: &mut BorrowedDescriptorInventory) -> Vec<u64> {
    let mut cookies = Vec::new();
    for i in 0..identity.count {
        let descriptor = store.get(store.at(identity.cpu.wrapping_add(i as u64 * identity.stride as u64)));
        let srv_valid = descriptor.is_some_and(|d| d[0] == 2 && decode_srv(d, None).is_some());
        let cookie = inventory_borrowed_descriptor(descriptor, srv_valid, counts);
        if cookie != 0 {
            cookies.push(cookie);
        }
    }
    cookies.sort_unstable();
    cookies.dedup();
    counts.unique_cookies = cookies.len() as u64;
    cookies
}
/// The cookies counted as tracked and untracked.
pub fn tracked(cookies: &[u64]) -> (u64, u64) {
    let known = cookies.iter().filter(|&&c| resource_known(c)).count() as u64;
    (known, cookies.len() as u64 - known)
}

// ---- the C ABI

/// The inventory: `counts` filled and the cookies (at most `capacity`) into `cookies`; their count.
///
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `identity` and `counts` are valid; `cookies` has room for
/// `capacity`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_inventory(store: *const Store, identity: *const RangeIdentity, counts: *mut BorrowedDescriptorInventory, cookies: *mut u64, capacity: usize) -> usize {
    let found = inventory(unsafe { &*store }, unsafe { &*identity }, unsafe { &mut *counts });
    let n = found.len().min(capacity);
    if n != 0 {
        unsafe { core::ptr::copy_nonoverlapping(found.as_ptr(), cookies, n) };
    }
    n
}
/// The cookies' tracked and untracked counts added to `tracked_count` and `untracked_count`.
///
/// # Safety
/// `cookies` holds `count` cookies; the counts are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_inventory_tracked(cookies: *const u64, count: usize, tracked_count: *mut u64, untracked_count: *mut u64) {
    let cookies = if cookies.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(cookies, count) } };
    let (known, unknown) = tracked(cookies);
    unsafe {
        *tracked_count += known;
        *untracked_count += unknown;
    }
}

#[cfg(test)]
mod tests {
    //! A WARP texture's SRV (twice), a null SRV, a CBV and an empty slot inventoried; the cookie tracked once known.
    use super::*;
    use crate::audit_lifetimes::track_resource;
    use crate::audit_values::{cbv_value, cookie, srv_value};
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    unsafe extern "C-unwind" fn removed(_: u64) {}

    #[test]
    fn inventories_slots_and_tracks_cookies() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let heap_props = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 4,
            Height: 4,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let mut texture: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap_props, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut texture) }.unwrap();
        let texture = texture.unwrap();
        let raw = texture.as_raw();
        let texture_cookie = cookie(raw);
        let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
        };
        let (cpu, gpu, stride) = (0x30_0000u64, 0x40_0000u64, 32u32);
        let mut store = Store::new(false);
        assert!(store.heap_created(cpu, gpu, 8, stride, 0xB108));
        let srv = srv_value(raw, texture_cookie, Some(&view));
        let null = srv_value(core::ptr::null_mut(), 0, Some(&view));
        let cbv = cbv_value(Some(&D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: 0x1_0000, SizeInBytes: 256 }));
        for (slot, value) in [(0u64, &srv), (1, &null), (2, &cbv), (4, &srv)] {
            store.write_value(cpu + slot * stride as u64, stride, value);
        }
        let identity = store.range(gpu, 5);
        let mut counts = BorrowedDescriptorInventory::default();
        let cookies = inventory(&store, &identity, &mut counts);
        assert_eq!(cookies, [texture_cookie]);
        assert_eq!((counts.slots, counts.resource_srv, counts.known_null, counts.other_kinds, counts.unknown, counts.unique_cookies), (5, 2, 1, 1, 1, 1));
        assert_eq!(tracked(&cookies), (0, 1));
        assert!(track_resource(raw, texture_cookie, removed));
        assert_eq!(tracked(&cookies), (1, 0));
    }
}
