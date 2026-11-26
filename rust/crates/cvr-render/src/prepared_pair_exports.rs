//! The native prepared pair's exported rows (src/Render/NativePreparedPair.cpp, `PrepareGeometry`, `StartInternal` and
//! `CompareNativeOutput`): each mesh plan step's row (its eye mask, the left and right sources' ordinals -- all ones when
//! absent -- and, for a paired step, the pair's command; single-eye steps counted per eye in the batch stats 4 and 5,
//! at most 1024 rows); the pass's depth state row (depth enable, function, write mask, stencil enable, read and write
//! masks, front and back stencil functions, the first source's stencil reference, the front face's fail, depth-fail
//! and pass operations and the back face's pass operation); and the native output comparison's verdict (complete: all
//! ten planes compared; exact: complete against native exits with no differences in either eye).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npp_plan_rows`, `cvr_npp_depth_row` and
//! `cvr_npp_output_verdict`; the unit tests below cover each field.

use windows::Win32::Graphics::Direct3D12::D3D12_DEPTH_STENCIL_DESC;

/// stereo MeshPlanDraw.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Step {
    pub mask: u32,
    pub source: [u32; 2],
}
const _: () = assert!(core::mem::size_of::<Step>() == 12);

/// The plan rows; `ordinals` each eye's source ordinals by eye index, `commands` the paired command by left index.
pub fn plan_rows(steps: &[Step], ordinals: [&[u64]; 2], commands: &[u64], rows: &mut [u64; 4096], stats: &mut [u64; 16]) {
    for (i, step) in steps.iter().enumerate() {
        let mut row = [u64::from(step.mask), u64::MAX, u64::MAX, u64::MAX];
        for eye in 0..2 {
            if step.mask & (1 << eye) != 0 {
                row[1 + eye] = ordinals[eye].get(step.source[eye] as usize).copied().unwrap_or(u64::MAX);
            }
        }
        if step.mask == 3 {
            row[3] = commands.get(step.source[0] as usize).copied().unwrap_or(u64::MAX);
        } else {
            stats[if step.mask == 1 { 4 } else { 5 }] += 1;
        }
        if let Some(out) = rows.get_mut(i * 4..i * 4 + 4) {
            out.copy_from_slice(&row);
        }
    }
}
/// The depth state row.
pub fn depth_row(ds: &D3D12_DEPTH_STENCIL_DESC, stencil: u32) -> [u64; 13] {
    let u = |v: i32| u64::from(v as u32);
    [
        u(ds.DepthEnable.0),
        u(ds.DepthFunc.0),
        u(ds.DepthWriteMask.0),
        u(ds.StencilEnable.0),
        u64::from(ds.StencilReadMask),
        u64::from(ds.StencilWriteMask),
        u(ds.FrontFace.StencilFunc.0),
        u(ds.BackFace.StencilFunc.0),
        u64::from(stencil),
        u(ds.FrontFace.StencilFailOp.0),
        u(ds.FrontFace.StencilDepthFailOp.0),
        u(ds.FrontFace.StencilPassOp.0),
        u(ds.BackFace.StencilPassOp.0),
    ]
}
/// CompareNativeOutput's verdict: writes the complete (8) and exact (9) flags, returns the pass's acceptance.
pub fn output_verdict(valid: bool, compared: u32, stats: &mut [u64; 24]) -> bool {
    stats[8] = u64::from(valid && compared == 10);
    stats[9] = u64::from(stats[8] != 0 && stats[10] != 0 && stats[6] == 0 && stats[7] == 0);
    stats[8] != 0 && stats[6] == 0 && stats[7] == 0
}

// ---- the C ABI

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// # Safety
/// The arrays hold their counts (null when empty); `rows` holds 4096 values, `stats` 16.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_npp_plan_rows(
    steps: *const Step,
    step_count: usize,
    left: *const u64,
    left_count: usize,
    right: *const u64,
    right_count: usize,
    commands: *const u64,
    command_count: usize,
    rows: *mut [u64; 4096],
    stats: *mut [u64; 16],
) {
    unsafe {
        plan_rows(slice(steps, step_count), [slice(left, left_count), slice(right, right_count)], slice(commands, command_count), &mut *rows, &mut *stats)
    };
}
/// # Safety
/// `ds` is valid; `out` holds 13 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_depth_row(ds: *const D3D12_DEPTH_STENCIL_DESC, stencil: u32, out: *mut [u64; 13]) {
    unsafe { *out = depth_row(&*ds, stencil) };
}
/// # Safety
/// `stats` holds 24 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_output_verdict(valid: bool, compared: u32, stats: *mut [u64; 24]) -> bool {
    output_verdict(valid, compared, unsafe { &mut *stats })
}

#[cfg(test)]
mod tests {
    //! Each field.
    use super::*;
    use windows::Win32::Foundation::{FALSE, TRUE};
    use windows::Win32::Graphics::Direct3D12::*;

    #[test]
    fn plans() {
        let steps = [Step { mask: 3, source: [0, 1] }, Step { mask: 1, source: [1, u32::MAX] }, Step { mask: 2, source: [u32::MAX, 0] }];
        let (mut rows, mut stats) = ([0u64; 4096], [0u64; 16]);
        plan_rows(&steps, [&[10, 11], &[20, 21]], &[7, 8], &mut rows, &mut stats);
        assert_eq!(rows[..12], [3, 10, 21, 7, 1, 11, u64::MAX, u64::MAX, 2, u64::MAX, 20, u64::MAX]);
        assert_eq!((stats[4], stats[5]), (1, 1));
        // Past 1024 rows the steps still count but write nothing.
        let many = vec![Step { mask: 1, source: [0, 0] }; 1025];
        let (mut rows, mut stats) = ([0u64; 4096], [0u64; 16]);
        plan_rows(&many, [&[5], &[]], &[], &mut rows, &mut stats);
        assert_eq!((rows[4092], stats[4]), (1, 1025));
    }
    #[test]
    fn depths() {
        let ds = D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: TRUE,
            DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ALL,
            DepthFunc: D3D12_COMPARISON_FUNC_GREATER_EQUAL,
            StencilEnable: FALSE,
            StencilReadMask: 0xF0,
            StencilWriteMask: 0x0F,
            FrontFace: D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_ZERO,
                StencilPassOp: D3D12_STENCIL_OP_REPLACE,
                StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
            },
            BackFace: D3D12_DEPTH_STENCILOP_DESC {
                StencilFailOp: D3D12_STENCIL_OP_KEEP,
                StencilDepthFailOp: D3D12_STENCIL_OP_KEEP,
                StencilPassOp: D3D12_STENCIL_OP_INCR,
                StencilFunc: D3D12_COMPARISON_FUNC_EQUAL,
            },
        };
        assert_eq!(depth_row(&ds, 0x80), [1, 7, 1, 0, 0xF0, 0x0F, 8, 3, 0x80, 1, 2, 3, 7]);
    }
    #[test]
    fn verdicts() {
        let mut stats = [0u64; 24];
        stats[10] = 1;
        assert!(output_verdict(true, 10, &mut stats));
        assert_eq!((stats[8], stats[9]), (1, 1));
        assert!(!output_verdict(true, 9, &mut stats));
        assert_eq!((stats[8], stats[9]), (0, 0));
        assert!(!output_verdict(false, 10, &mut stats));
        stats[10] = 0;
        assert!(output_verdict(true, 10, &mut stats));
        assert_eq!(stats[9], 0);
        stats[10] = 1;
        stats[7] = 2;
        assert!(!output_verdict(true, 10, &mut stats));
        assert_eq!((stats[8], stats[9]), (1, 0));
    }
}
