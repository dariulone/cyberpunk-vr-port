//! The descriptor audit's owned source descriptors (src/Render/StereoDescriptorAudit.cpp,
//! `descriptors::EmitSourceDescriptor`): an owned source table's slot written into its private CPU heap -- a used slot
//! from its canonical value (a CBV's view, an SRV / UAV of its retained resource and counter with its view, or none for
//! an implicit one, a sampler's description; an undecodable value gives the zero view as before), an unused one as the
//! kind's typed null (a null CBV, an R32_UINT buffer UAV, a point-clamp sampler, an R32_FLOAT buffer SRV).
//!
//! In the Rust build (`CVR_RUST_VRIK`) EmitSourceDescriptor forwards to `cvr_sda_emit` (the caller's internal scope keeps
//! the device's hooks from observing it). `tools/single_pass_tests/source_table_capture_test` and
//! `source_sparse_capture_test` run their cases against it in their Rust variants.

use core::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32_UINT};

use crate::descriptor_store::Value;
use crate::descriptor_views::{decode_cbv, decode_sampler, decode_srv, decode_uav};

/// EmitSourceDescriptor.
///
/// # Safety
/// `device` is live; `target` is a CPU descriptor of a heap of the slot's type.
pub unsafe fn emit(device: &ID3D12Device, kind: u32, used: bool, value: &Value, resource: Option<&ID3D12Resource>, counter: Option<&ID3D12Resource>, target: D3D12_CPU_DESCRIPTOR_HANDLE) {
    unsafe {
        if !used {
            match kind {
                1 => device.CreateConstantBufferView(None, target),
                3 => {
                    let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
                        Format: DXGI_FORMAT_R32_UINT,
                        ViewDimension: D3D12_UAV_DIMENSION_BUFFER,
                        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Buffer: D3D12_BUFFER_UAV { NumElements: 1, ..Default::default() } },
                    };
                    device.CreateUnorderedAccessView(None::<&ID3D12Resource>, None::<&ID3D12Resource>, Some(&view), target);
                }
                4 => {
                    let view = D3D12_SAMPLER_DESC {
                        Filter: D3D12_FILTER_MIN_MAG_MIP_POINT,
                        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
                        MaxAnisotropy: 1,
                        ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
                        MaxLOD: f32::MAX,
                        ..Default::default()
                    };
                    device.CreateSampler(&view, target);
                }
                _ => {
                    let view = D3D12_SHADER_RESOURCE_VIEW_DESC {
                        Format: DXGI_FORMAT_R32_FLOAT,
                        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
                        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
                        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Buffer: D3D12_BUFFER_SRV { NumElements: 1, ..Default::default() } },
                    };
                    device.CreateShaderResourceView(None::<&ID3D12Resource>, Some(&view), target);
                }
            }
            return;
        }
        match kind {
            1 => device.CreateConstantBufferView(Some(&decode_cbv(value).unwrap_or_default().view), target),
            2 => {
                let view = decode_srv(value, None).unwrap_or_default();
                device.CreateShaderResourceView(resource, (!view.implicit).then_some(&view.view as *const _), target);
            }
            3 => {
                let view = decode_uav(value).unwrap_or_default();
                device.CreateUnorderedAccessView(resource, counter, (!view.implicit).then_some(&view.view as *const _), target);
            }
            _ => device.CreateSampler(&decode_sampler(value).unwrap_or_default().view, target),
        }
    }
}

// ---- the C ABI

/// # Safety
/// `device` is live; `value` is valid; `resource` and `counter` are live or null; `target` is a CPU descriptor of a heap
/// of the slot's type.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_emit(device: *mut c_void, kind: u32, used: bool, value: *const Value, resource: *mut c_void, counter: *mut c_void, target: usize) {
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return;
    };
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource) };
    let counter = unsafe { ID3D12Resource::from_raw_borrowed(&counter) };
    unsafe { emit(device, kind, used, &*value, resource, counter, D3D12_CPU_DESCRIPTOR_HANDLE { ptr: target }) };
}
