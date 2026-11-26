//! FitRenderSizeInsideBounds (formerly src/Hooks/SwapChain.cpp): the window hooks of src/Hooks/OsInput.cpp fit a
//! launcher-set window bigger than the monitor into it at the render's aspect.

/// FitRenderSizeInsideBounds: the largest size with the render's aspect inside the bounds (a window set in the launcher
/// bigger than the monitor), rounded to nearest.
pub fn fit_render_size_inside_bounds(
    render_w: u32,
    render_h: u32,
    max_w: u32,
    max_h: u32,
) -> (u32, u32) {
    if render_w == 0
        || render_h == 0
        || max_w == 0
        || max_h == 0
        || (render_w <= max_w && render_h <= max_h)
    {
        return (render_w, render_h);
    }
    let (rw, rh, mw, mh) = (
        u64::from(render_w),
        u64::from(render_h),
        u64::from(max_w),
        u64::from(max_h),
    );
    if rw * mh > rh * mw {
        (max_w, ((rh * mw + rw / 2) / rw) as u32)
    } else {
        (((rw * mh + rh / 2) / rh) as u32, max_h)
    }
}

#[cfg(test)]
mod tests {
    use super::fit_render_size_inside_bounds as fit;

    #[test]
    fn fits_the_render_aspect() {
        assert_eq!(fit(2064, 2232, 4000, 4000), (2064, 2232));
        assert_eq!(fit(2064, 2232, 1920, 1080), (999, 1080));
        assert_eq!(fit(4000, 1000, 1920, 1080), (1920, 480));
        assert_eq!(fit(0, 1000, 1920, 1080), (0, 1000));
    }
}
