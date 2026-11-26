//! The retained geometry's private IA views (src/Render/NativeRetainedGeometry.cpp, CaptureIaSnapshots): each recorded
//! private view consumed by this draw (the index buffer of an indexed draw, or a vertex slot of the draw's inputs; a
//! resource and an owner), each role once, its range inside its own buffer (a nonzero GPU address; the reserved span
//! inside the buffer; a copy of at most the reserved bytes), the original view's coordinates kept (size and format or
//! stride; the copy the whole view; the view at the private copy), every consumed role present (the IB of an indexed
//! draw; exactly the copied vertex slots); the draw rewritten onto the private views, the owned leases' address, width
//! and flags, and the view and reuse rows.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp lends the views and keeps the leases of the rewritten draw; the unit
//! test below covers the admission, each refusal and the rows.

use crate::geometry_commands::DrawRecord;
use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// A recorded private IA view's facts.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IaView {
    pub role: u32,
    pub slot: u32,
    pub resource: *mut c_void,
    pub owner: bool,
    pub source_offset: u64,
    pub private_offset: u64,
    pub copied: u64,
    pub reserved: u64,
    pub origin: u64,
    pub reused: bool,
    pub index: D3D12_INDEX_BUFFER_VIEW,
    pub vertex: D3D12_VERTEX_BUFFER_VIEW,
}
/// The owned lease of a view's buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lease {
    pub gpu: u64,
    pub width: u64,
    pub flags: u32,
    pub _pad: u32,
}
const _: () = assert!(core::mem::size_of::<IaView>() == 104 && core::mem::size_of::<Lease>() == 24);

/// The refusals (the .cpp's messages): 1 a role not consumed, 2 a role twice, 3 a range off its buffer, 4 an IB off the
/// original coordinates, 5 a VB off them, 6 the consumed IB omitted, 7 a consumed VB omitted.
pub mod refusal {
    pub const ROLE: u32 = 1;
    pub const TWICE: u32 = 2;
    pub const RANGE: u32 = 3;
    pub const INDEX: u32 = 4;
    pub const VERTEX: u32 = 5;
    pub const NO_INDEX: u32 = 6;
    pub const NO_VERTEX: u32 = 7;
}
/// A view's buffer: its description's dimension, width and flags, and its GPU address.
pub type Buffer = (D3D12_RESOURCE_DIMENSION, u64, u32, u64);
fn buffer(resource: *mut c_void) -> Option<Buffer> {
    // SAFETY: the view's resource lives (the source holds it).
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }?;
    let desc = unsafe { resource.GetDesc() };
    Some((desc.Dimension, desc.Width, desc.Flags.0 as u32, unsafe { resource.GetGPUVirtualAddress() }))
}
/// The views checked, `draw` rewritten onto them, the leases and rows filled (0) or the refusal.
pub fn admit(views: &[IaView], indexed: bool, copied_mask: u32, draw: &mut DrawRecord, describe: impl Fn(*mut c_void) -> Option<Buffer>, leases: &mut [Lease], source: u64, side: u64, view_rows: &mut [[u64; 16]], reuse_rows: &mut [[u64; 8]]) -> u32 {
    let mut seen = [false; 17];
    for (n, v) in views.iter().enumerate() {
        let index = v.role == 0;
        if v.resource.is_null() || !v.owner || (if index { !indexed || v.slot != u32::MAX } else { v.role != 1 || v.slot >= 16 || draw.input_slot_mask & (1u32 << v.slot) == 0 }) {
            return refusal::ROLE;
        }
        let key = if index { 0 } else { v.slot as usize + 1 };
        if seen[key] {
            return refusal::TWICE;
        }
        seen[key] = true;
        let Some((dimension, width, flags, gpu)) = describe(v.resource) else { return refusal::RANGE };
        if dimension != D3D12_RESOURCE_DIMENSION_BUFFER || gpu == 0 || v.private_offset > width || v.reserved > width - v.private_offset || v.copied == 0 || v.copied > v.reserved || gpu > u64::MAX - v.private_offset {
            return refusal::RANGE;
        }
        leases[n] = Lease { gpu, width, flags, _pad: 0 };
        if index {
            let original = draw.index;
            if v.index.SizeInBytes != original.SizeInBytes || v.index.Format != original.Format || v.copied != u64::from(original.SizeInBytes) || v.index.BufferLocation != gpu + v.private_offset {
                return refusal::INDEX;
            }
            draw.index = v.index;
        } else {
            let original = draw.vertices[v.slot as usize];
            if v.vertex.SizeInBytes != original.SizeInBytes || v.vertex.StrideInBytes != original.StrideInBytes || v.copied != u64::from(original.SizeInBytes) || v.vertex.BufferLocation != gpu + v.private_offset {
                return refusal::VERTEX;
            }
            draw.vertices[v.slot as usize] = v.vertex;
        }
    }
    if seen[0] != indexed {
        return refusal::NO_INDEX;
    }
    if (0..16).any(|slot| seen[slot + 1] != (copied_mask & (1u32 << slot) != 0)) {
        return refusal::NO_VERTEX;
    }
    for (n, v) in views.iter().enumerate() {
        let index = v.role == 0;
        let (location, size, shape) = if index { (v.index.BufferLocation, v.index.SizeInBytes, v.index.Format.0 as u32) } else { (v.vertex.BufferLocation, v.vertex.SizeInBytes, v.vertex.StrideInBytes) };
        view_rows[n] = [source, side, u64::from(v.role), u64::from(v.slot), v.resource as u64, u64::from(v.owner), v.source_offset, v.private_offset, v.copied, v.reserved, location, u64::from(size), u64::from(shape), 0, 0, 0];
        reuse_rows[n] = [source, side, u64::from(v.role), u64::from(v.slot), v.origin, u64::from(v.reused), v.private_offset, v.copied];
    }
    0
}

/// The views' admission (0 or the refusal), `draw` rewritten, the leases and rows filled.
///
/// # Safety
/// `views`, `leases`, `view_rows` and `reuse_rows` hold `count` entries; `draw` writable; the views' resources live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_ia_views(views: *const IaView, count: usize, indexed: bool, copied_mask: u32, draw: *mut DrawRecord, leases: *mut Lease, source: u64, side: u64, view_rows: *mut [u64; 16], reuse_rows: *mut [u64; 8]) -> u32 {
    let n = if views.is_null() { 0 } else { count };
    // SAFETY: as documented.
    unsafe {
        let views = if n == 0 { &[][..] } else { core::slice::from_raw_parts(views, n) };
        let leases = if n == 0 { &mut [][..] } else { core::slice::from_raw_parts_mut(leases, n) };
        let view_rows = if n == 0 { &mut [][..] } else { core::slice::from_raw_parts_mut(view_rows, n) };
        let reuse_rows = if n == 0 { &mut [][..] } else { core::slice::from_raw_parts_mut(reuse_rows, n) };
        admit(views, indexed, copied_mask, &mut *draw, buffer, leases, source, side, view_rows, reuse_rows)
    }
}

#[cfg(test)]
mod tests {
    //! An indexed draw of two vertex slots: admitted and rewritten, the rows; each refusal.
    use super::*;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT};

    fn view(role: u32, slot: u32, private_offset: u64, copied: u64) -> IaView {
        IaView {
            role,
            slot,
            resource: 0x1000 as *mut c_void,
            owner: true,
            source_offset: 8,
            private_offset,
            copied,
            reserved: copied + 16,
            origin: 3,
            reused: false,
            index: D3D12_INDEX_BUFFER_VIEW { BufferLocation: 0x10_0000 + private_offset, SizeInBytes: copied as u32, Format: DXGI_FORMAT_R16_UINT },
            vertex: D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x10_0000 + private_offset, SizeInBytes: copied as u32, StrideInBytes: 16 },
        }
    }
    fn describe(_: *mut c_void) -> Option<Buffer> {
        Some((D3D12_RESOURCE_DIMENSION_BUFFER, 4096, 0, 0x10_0000))
    }
    #[test]
    fn views() {
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.input_slot_mask = 0b1000_0001;
        d.index = D3D12_INDEX_BUFFER_VIEW { BufferLocation: 0x9000, SizeInBytes: 96, Format: DXGI_FORMAT_R16_UINT };
        d.vertices[0] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x9100, SizeInBytes: 256, StrideInBytes: 16 };
        d.vertices[7] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x9200, SizeInBytes: 512, StrideInBytes: 16 };
        let views = [view(0, u32::MAX, 0, 96), view(1, 0, 256, 256), view(1, 7, 1024, 512)];
        let (mut leases, mut rows, mut reuse) = ([Lease::default(); 3], [[0u64; 16]; 3], [[0u64; 8]; 3]);
        let mut x = d;
        assert_eq!(admit(&views, true, 0b1000_0001, &mut x, describe, &mut leases, 5, 1, &mut rows, &mut reuse), 0);
        assert_eq!((x.index.BufferLocation, x.vertices[0].BufferLocation, x.vertices[7].BufferLocation), (0x10_0000, 0x10_0100, 0x10_0400), "the draw on the private copies");
        assert_eq!(leases[1], Lease { gpu: 0x10_0000, width: 4096, flags: 0, _pad: 0 });
        assert_eq!((rows[0][2], rows[0][3], rows[0][12], rows[2][11], rows[2][12]), (0, u64::from(u32::MAX), DXGI_FORMAT_R16_UINT.0 as u64, 512, 16));
        assert_eq!(&reuse[1], &[5, 1, 1, 0, 3, 0, 256, 256]);
        let run = |views: &[IaView], indexed: bool, mask: u32| {
            let (mut l, mut r, mut u) = ([Lease::default(); 3], [[0u64; 16]; 3], [[0u64; 8]; 3]);
            let mut y = d;
            admit(views, indexed, mask, &mut y, describe, &mut l[..views.len()], 5, 1, &mut r[..views.len()], &mut u[..views.len()])
        };
        assert_eq!(run(&[view(1, 3, 0, 64)], true, 0b1000), refusal::ROLE, "a slot the draw does not consume");
        assert_eq!(run(&views, false, 0b1000_0001), refusal::ROLE, "an IB of a non-indexed draw");
        assert_eq!(run(&[views[1], views[1]], false, 1), refusal::TWICE);
        assert_eq!(run(&[IaView { copied: 300, reserved: 280, ..views[1] }], false, 1), refusal::RANGE, "a copy beyond its reservation");
        assert_eq!(run(&[IaView { index: D3D12_INDEX_BUFFER_VIEW { Format: DXGI_FORMAT_R32_UINT, ..views[0].index }, ..views[0] }], true, 0), refusal::INDEX);
        assert_eq!(run(&[IaView { vertex: D3D12_VERTEX_BUFFER_VIEW { StrideInBytes: 32, ..views[1].vertex }, ..views[1] }], false, 1), refusal::VERTEX);
        assert_eq!(run(&views[1..], true, 0b1000_0001), refusal::NO_INDEX);
        assert_eq!(run(&views[..2], true, 0b1000_0001), refusal::NO_VERTEX, "slot 7 copied but not recorded");
    }
}
