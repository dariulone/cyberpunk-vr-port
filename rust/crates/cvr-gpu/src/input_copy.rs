//! Framegen's copy of one engine input -- the depth or the motion vectors a frame was rendered with -- into a texture
//! of its own (formerly src/Framegen/InputCopy.cpp; the header keeps `InputTag` and `CopyInput`, which forwards the
//! tag and the fields of `Inputs` it fills).
//!
//! Depth is taken from the 32-bit float depth formats and copied as R32_FLOAT; motion keeps its own format (RG16F,
//! RG32F, or the RGBA16F texture ray tracing writes, whose .xy is the 2D motion FidelityFX reads). The copy is the
//! tag's rectangle (the whole texture when it gives none), the source put back in its tagged state, the copy left in
//! NON_PIXEL_SHADER_RESOURCE; the copy is remade when its size or format changes, and only on the source's device.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// include/Framegen/InputCopy.hpp's InputTag.
#[repr(C)]
pub struct InputTag {
    pub resource: *mut c_void,
    pub state: u32,
    pub kind: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
/// The fields of framegen's `Inputs` a copy fills (each `ComPtr` is one owning pointer).
#[repr(C)]
pub struct InputSlots {
    pub depth: *mut Option<ID3D12Resource>,
    pub motion: *mut Option<ID3D12Resource>,
    pub ready_fence: *mut Option<ID3D12Fence>,
    pub width: *mut u32,
    pub height: *mut u32,
    pub motion_width: *mut u32,
    pub motion_height: *mut u32,
    pub has_depth: *mut bool,
    pub has_motion: *mut bool,
    pub bytes: *mut u64,
}

fn motion_pixel_bytes(format: DXGI_FORMAT) -> u32 {
    match format {
        DXGI_FORMAT_R16G16_FLOAT => 4,
        DXGI_FORMAT_R32G32_FLOAT | DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
        _ => 0,
    }
}
unsafe fn barrier(
    list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    a: D3D12_RESOURCE_STATES,
    b: D3D12_RESOURCE_STATES,
    subresource: u32,
) {
    if a == b {
        return;
    }
    let v = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: core::mem::transmute_copy(resource),
                Subresource: subresource,
                StateBefore: a,
                StateAfter: b,
            }),
        },
    };
    list.ResourceBarrier(&[v]);
}
fn device_of<T: Interface>(object: &T) -> Option<ID3D12Device> {
    let child: ID3D12DeviceChild = object.cast().ok()?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live device child writing its device.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}

/// One tagged input copied on `list` into `out`'s depth (kind 0) or motion (kind 1) texture.
///
/// # Safety
/// `list` is an open command list or null, `tag` and `out` valid, `out`'s pointers framegen's `Inputs` fields.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_framegen_copy_input(
    list: *mut c_void,
    tag: *const InputTag,
    out: *const InputSlots,
) -> bool {
    let (tag, out) = (&*tag, &*out);
    let (Some(list), Some(source)) = (
        ID3D12GraphicsCommandList::from_raw_borrowed(&list),
        ID3D12Resource::from_raw_borrowed(&tag.resource),
    ) else {
        return false;
    };
    if tag.kind > 1 {
        return false;
    }
    let desc = source.GetDesc();
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.SampleDesc.Count != 1
        || desc.DepthOrArraySize != 1
    {
        return false;
    }
    let width = if tag.width != 0 {
        tag.width
    } else {
        desc.Width as u32
    };
    let height = if tag.height != 0 {
        tag.height
    } else {
        desc.Height
    };
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || tag.x as u64 + width as u64 > desc.Width
        || tag.y as u64 + height as u64 > desc.Height as u64
    {
        return false;
    }
    let mut format = desc.Format;
    if tag.kind == 0 {
        match format {
            DXGI_FORMAT_R32_TYPELESS
            | DXGI_FORMAT_D32_FLOAT
            | DXGI_FORMAT_R32_FLOAT
            | DXGI_FORMAT_R32G8X24_TYPELESS
            | DXGI_FORMAT_D32_FLOAT_S8X24_UINT
            | DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS => format = DXGI_FORMAT_R32_FLOAT,
            _ => return false,
        }
    } else if motion_pixel_bytes(format) == 0 {
        return false;
    }
    let target = &mut *(if tag.kind == 0 { out.depth } else { out.motion });
    if let Some(existing) = target.as_ref() {
        match (device_of(list), device_of(existing)) {
            (Some(a), Some(b)) if a.as_raw() == b.as_raw() => {}
            _ => return false,
        }
    }
    let recreate = match target.as_ref() {
        None => true,
        Some(t) => {
            let d = t.GetDesc();
            d.Width != width as u64 || d.Height != height || d.Format != format
        }
    };
    if recreate {
        let Some(device) = device_of(list) else {
            return false;
        };
        let d = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut created: Option<ID3D12Resource> = None;
        if device
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &d,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut created,
            )
            .is_err()
        {
            return false;
        }
        let Some(created) = created else {
            return false;
        };
        let name: PCWSTR = if tag.kind == 0 {
            w!("CVR_Framegen_Depth")
        } else {
            w!("CVR_Framegen_Motion")
        };
        let _ = created.SetName(name);
        *target = Some(created);
        let fence = &mut *out.ready_fence;
        if fence.is_none() {
            match device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
                Ok(f) => *fence = Some(f),
                Err(_) => return false,
            }
        }
    } else if let Some(t) = target.as_ref() {
        barrier(
            list,
            t,
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
        );
    }
    let Some(copy) = target.as_ref() else {
        return false;
    };
    let state = D3D12_RESOURCE_STATES(tag.state as i32);
    barrier(list, source, state, D3D12_RESOURCE_STATE_COPY_SOURCE, 0);
    let src = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(source),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let dst = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(copy),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            SubresourceIndex: 0,
        },
    };
    let bounds = D3D12_BOX {
        left: tag.x,
        top: tag.y,
        front: 0,
        right: tag.x + width,
        bottom: tag.y + height,
        back: 1,
    };
    list.CopyTextureRegion(&dst, 0, 0, 0, &src, Some(&bounds));
    barrier(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, state, 0);
    barrier(
        list,
        copy,
        D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
    );
    if tag.kind == 0 {
        *out.width = width;
        *out.height = height;
        *out.has_depth = true;
    } else {
        *out.motion_width = width;
        *out.motion_height = height;
        *out.has_motion = true;
    }
    let motion_bytes = (*out.motion)
        .as_ref()
        .map_or(0, |m| motion_pixel_bytes(m.GetDesc().Format));
    *out.bytes = *out.width as u64 * *out.height as u64 * 4
        + *out.motion_width as u64 * *out.motion_height as u64 * motion_bytes as u64;
    true
}
