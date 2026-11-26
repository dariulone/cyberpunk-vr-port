//! THE CAMERA SCOPE'S RAY RECONSTRUCTION AT FULL RESOLUTION (main's include/Stereo/ScopeResolution.hpp). With DLSS RR
//! on, the scope's render-to-texture view is drawn at the upscaler's ratio like MAIN; this switch has the engine size
//! it natively instead (xr_scope_rr_full_resolution, on by default), by masking the resolution-scale bits of the view's
//! flags only around the native resolution calculation -- never into the render graph, where RR still needs its native
//! denoiser inputs and passes.
use core::sync::atomic::AtomicBool;

pub static FULL_RESOLUTION: AtomicBool = AtomicBool::new(true);
pub const RR_FEATURE: u64 = 1u64 << (71 - 64);
/// CP2.5 RenderRes fallback at 4255BD tests exactly these bits before calling the global upscaler ratio getter. Its
/// dynamic-resolution branch tests a subset.
pub const RESOLUTION_SCALE_BITS: u64 =
    (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10) | (1 << 12) | (1 << 13) | (1 << 14) | (1 << 27);

pub fn full_resolution_eligible(enabled: bool, scope: bool, flags: u64, width: i64, height: i64) -> bool {
    enabled && scope && (flags & RR_FEATURE) != 0 && width >= 16 && height >= 16 && width <= 4096 && height <= 4096
}

/// Only around the native resolution calculation, BEFORE graph construction: the flags masked for its duration and
/// restored when it ends. Native code writes every size/previous-size field consistently.
pub struct NativeResolutionFlags {
    flags: *mut u64,
    saved: u64,
}
impl NativeResolutionFlags {
    /// # Safety
    /// `flags` is the view's live flags word for the guard's lifetime.
    pub unsafe fn new(flags: *mut u64) -> Self {
        // SAFETY: as the caller vouches.
        let saved = unsafe { *flags };
        unsafe { *flags = saved & !RESOLUTION_SCALE_BITS };
        Self { flags, saved }
    }
}
impl Drop for NativeResolutionFlags {
    fn drop(&mut self) {
        // SAFETY: the word `new` was given, still live.
        unsafe { *self.flags = self.saved };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_only_for_the_guard() {
        // The masked word is the view's first flags word; RR_FEATURE is bit 71, bit 7 of the second (it shares a bit
        // number with a resolution-scale bit, never the word).
        let mut flags = RESOLUTION_SCALE_BITS | (1 << 5) | (1 << 40);
        {
            // SAFETY: a local word, alive for the guard.
            let _g = unsafe { NativeResolutionFlags::new(&mut flags) };
            assert_eq!(flags, (1 << 5) | (1 << 40));
        }
        assert_eq!(flags, RESOLUTION_SCALE_BITS | (1 << 5) | (1 << 40));
        assert!(full_resolution_eligible(true, true, RR_FEATURE, 1024, 1024));
        assert!(!full_resolution_eligible(true, true, 0, 1024, 1024));
        assert!(!full_resolution_eligible(true, true, RR_FEATURE, 8, 1024));
    }
}
