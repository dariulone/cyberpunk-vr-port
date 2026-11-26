//! The depth a stereo depth comparison's bank clears to (src/Render/StereoDepthComparison.cpp, `ClearFor`): only a
//! pass whose depth state is known, enabled and writing every bit has one -- 1 for the LESS, LESS_EQUAL and ALWAYS
//! tests, 0 for GREATER and GREATER_EQUAL; any other test has none.
//!
//! The .cpp's ClearFor passes its pass contract's depth state to `cvr_sdc_clear_for`; the unit test below checks
//! every comparison function.
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMPARISON_FUNC_ALWAYS, D3D12_COMPARISON_FUNC_GREATER, D3D12_COMPARISON_FUNC_GREATER_EQUAL,
    D3D12_COMPARISON_FUNC_LESS, D3D12_COMPARISON_FUNC_LESS_EQUAL, D3D12_DEPTH_WRITE_MASK_ALL,
};

/// The clear depth for a pass's depth state, or None.
pub fn clear_depth(known: bool, enable: bool, write_mask: i32, func: i32) -> Option<f32> {
    if !known || !enable || write_mask != D3D12_DEPTH_WRITE_MASK_ALL.0 {
        return None;
    }
    match func {
        f if f == D3D12_COMPARISON_FUNC_LESS.0 || f == D3D12_COMPARISON_FUNC_LESS_EQUAL.0 || f == D3D12_COMPARISON_FUNC_ALWAYS.0 => {
            Some(1.0)
        }
        f if f == D3D12_COMPARISON_FUNC_GREATER.0 || f == D3D12_COMPARISON_FUNC_GREATER_EQUAL.0 => Some(0.0),
        _ => None,
    }
}
/// # Safety
/// `value` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_clear_for(known: bool, enable: i32, write_mask: i32, func: i32, value: *mut f32) -> bool {
    let Some(depth) = clear_depth(known, enable != 0, write_mask, func) else { return false };
    // SAFETY: as the caller vouches.
    unsafe { *value = depth };
    true
}

#[cfg(test)]
mod tests {
    //! Every comparison function, and the state's conditions.
    use super::*;

    #[test]
    fn clears() {
        // d3d12.h: NEVER 1, LESS 2, EQUAL 3, LESS_EQUAL 4, GREATER 5, NOT_EQUAL 6, GREATER_EQUAL 7, ALWAYS 8.
        let all: Vec<Option<f32>> = (0..=9).map(|f| clear_depth(true, true, 1, f)).collect();
        assert_eq!(all, [None, None, Some(1.0), None, Some(1.0), Some(0.0), None, Some(0.0), Some(1.0), None]);
        // Unknown, disabled, or not writing (write mask ZERO 0) has none.
        assert_eq!(clear_depth(false, true, 1, 2), None);
        assert_eq!(clear_depth(true, false, 1, 2), None);
        assert_eq!(clear_depth(true, true, 0, 2), None);
    }
}
