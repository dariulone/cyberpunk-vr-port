//! Single pass's policy for the engine's indirect command signatures (formerly src/Render/NativeIndirectPolicy.cpp
//! and the inline detail of include/Render/NativeIndirectPolicy.hpp). A signature the engine creates is classified
//! once, after its successful creation, and the receipt is kept in the signature's own private data: no registry of
//! raw pointers exists. Only the two pure draw kinds, which leave root and IA state alone, are ever accepted:
//! kind 1 is DRAW_INDEXED, kind 2 is DRAW, and every other signature, an unobserved one or a malformed receipt is 0.
use core::ffi::c_void;

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;

const RECEIPT_KEY: GUID = GUID::from_values(0x6e80df9a, 0x3c21, 0x4dc7, [0x9d, 0x81, 0x35, 0x4f, 0x90, 0xaa, 0x17, 0x61]);

/// detail::Receipt: what the signature's private data holds (16 bytes, version 1).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub version: u32,
    pub kind: u32,
    pub stride: u32,
    pub node_mask: u32,
}
const _: () = assert!(core::mem::size_of::<Receipt>() == 16);

/// detail::ValidStride: at least the kind's arguments (20 bytes indexed, 16 plain), at most 1 KiB, whole DWORDs.
pub fn valid_stride(stride: u32, kind: u32) -> bool {
    let minimum = match kind {
        1 => core::mem::size_of::<D3D12_DRAW_INDEXED_ARGUMENTS>() as u32,
        2 => core::mem::size_of::<D3D12_DRAW_ARGUMENTS>() as u32,
        _ => 0,
    };
    minimum != 0 && stride >= minimum && stride <= 1024 && stride.is_multiple_of(4)
}

/// detail::Classify: one draw argument on node 0 or 1 with a valid stride, else 0.
///
/// # Safety
/// `desc.pArgumentDescs`, when not null, points to the descriptor's first argument description.
pub unsafe fn classify(desc: &D3D12_COMMAND_SIGNATURE_DESC) -> u32 {
    if desc.NumArgumentDescs != 1 || desc.pArgumentDescs.is_null() || desc.NodeMask > 1 {
        return 0;
    }
    // SAFETY: the one argument description the caller vouches for.
    let ty = unsafe { (*desc.pArgumentDescs).Type };
    let kind = if ty == D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED {
        1
    } else if ty == D3D12_INDIRECT_ARGUMENT_TYPE_DRAW {
        2
    } else {
        0
    };
    if valid_stride(desc.ByteStride, kind) {
        kind
    } else {
        0
    }
}

/// detail::DrawKind: the receipt's kind when it is a well-formed version 1 receipt, else 0.
pub fn draw_kind(receipt: &Receipt) -> u32 {
    if receipt.version == 1 && receipt.node_mask <= 1 && valid_stride(receipt.stride, receipt.kind) {
        receipt.kind
    } else {
        0
    }
}
/// detail::PureDrawStride: the actual stride (it may pad past the arguments) of either pure draw kind, else 0.
pub fn pure_draw_stride(receipt: &Receipt) -> u32 {
    if draw_kind(receipt) != 0 {
        receipt.stride
    } else {
        0
    }
}
/// detail::DrawIndexedStride: the stride of an indexed pure draw only.
pub fn draw_indexed_stride(receipt: &Receipt) -> u32 {
    if receipt.kind == 1 {
        pure_draw_stride(receipt)
    } else {
        0
    }
}

/// Created: classify a signature the engine has just created successfully (the port's own and unobserved ones
/// are left alone). The receipt keeps the original stride even for a rejected signature, for diagnostics.
pub fn created(signature: &ID3D12CommandSignature, desc: &D3D12_COMMAND_SIGNATURE_DESC) {
    if cvr_host::probe::internal() || !crate::stereo_probe::observe_bindings() {
        return;
    }
    // SAFETY: the creation descriptor the engine passed with this signature.
    let receipt = Receipt {
        version: 1,
        kind: unsafe { classify(desc) },
        stride: desc.ByteStride,
        node_mask: desc.NodeMask,
    };
    // SAFETY: 16 bytes copied by the runtime.
    let _ = unsafe { signature.SetPrivateData(&RECEIPT_KEY, 16, Some(&receipt as *const Receipt as *const c_void)) };
}

/// Describe: the raw receipt (zero when unobserved or of another size). Not an authorisation: use the accessors.
pub fn describe(signature: &ID3D12CommandSignature) -> Receipt {
    if !crate::stereo_probe::observe_bindings() {
        return Receipt::default();
    }
    let mut receipt = Receipt::default();
    let mut bytes = 16u32;
    // SAFETY: at most `bytes` bytes written into the receipt.
    let read = unsafe { signature.GetPrivateData(&RECEIPT_KEY, &mut bytes, Some(&mut receipt as *mut Receipt as *mut c_void)) };
    if read.is_err() || bytes != 16 {
        return Receipt::default();
    }
    receipt
}

// The header's forwarders (NativeIndirectPolicy.hpp in the Rust build).

#[unsafe(no_mangle)]
pub extern "C" fn cvr_indirect_valid_stride(stride: u32, kind: u32) -> bool {
    valid_stride(stride, kind)
}
/// # Safety
/// `desc` points to a command signature description (its argument pointer as D3D12 defines it).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_indirect_classify(desc: *const D3D12_COMMAND_SIGNATURE_DESC) -> u32 {
    // SAFETY: the caller's description.
    match unsafe { desc.as_ref() } {
        Some(d) => unsafe { classify(d) },
        None => 0,
    }
}
/// # Safety
/// `receipt` points to a receipt.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_indirect_receipt_kind(receipt: *const Receipt) -> u32 {
    // SAFETY: the caller's receipt.
    unsafe { receipt.as_ref() }.map_or(0, draw_kind)
}
/// # Safety
/// `receipt` points to a receipt.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_indirect_receipt_pure_stride(receipt: *const Receipt) -> u32 {
    // SAFETY: the caller's receipt.
    unsafe { receipt.as_ref() }.map_or(0, pure_draw_stride)
}
/// # Safety
/// `receipt` points to a receipt.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_indirect_receipt_indexed_stride(receipt: *const Receipt) -> u32 {
    // SAFETY: the caller's receipt.
    unsafe { receipt.as_ref() }.map_or(0, draw_indexed_stride)
}
/// # Safety
/// `signature` is null or a live command signature; `desc` points to its creation description.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_indirect_created(signature: *mut c_void, desc: *const D3D12_COMMAND_SIGNATURE_DESC) {
    // SAFETY: borrowed for this call.
    if let (Some(s), Some(d)) = (unsafe { ID3D12CommandSignature::from_raw_borrowed(&signature) }, unsafe { desc.as_ref() }) {
        created(s, d);
    }
}
/// # Safety
/// `signature` is null or a live command signature; `out` points to a receipt.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_indirect_describe(signature: *mut c_void, out: *mut Receipt) {
    // SAFETY: borrowed for this call.
    let receipt = unsafe { ID3D12CommandSignature::from_raw_borrowed(&signature) }
        .map(describe)
        .unwrap_or_default();
    if !out.is_null() {
        // SAFETY: the caller's receipt.
        unsafe { *out = receipt };
    }
}
