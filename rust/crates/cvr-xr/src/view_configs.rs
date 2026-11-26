//! The runtime's stereo view configuration, owned here (formerly OpenXRManager's m_viewConfigViews): the views the
//! instance's system reported at Init, refreshed each time the mono submission's resources are ensured, cleared at
//! shutdown; GetRecommendedRenderTargetSize reads the first view's recommended size here. With it the instance and the
//! system it was made for, as the Rust half keeps them (OpenXRManager's m_instance and m_systemId hold the same).
use core::sync::atomic::{AtomicU64, Ordering};
use openxr_sys as xr;
use std::sync::Mutex;

/// The instance and its head-mounted system (set when the instance is made, cleared when it is destroyed).
pub static INSTANCE: AtomicU64 = AtomicU64::new(0);
pub static SYSTEM: AtomicU64 = AtomicU64::new(0);

/// The recommended and largest image sizes of each view, in the runtime's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewConfig {
    pub recommended: (u32, u32),
    pub max: (u32, u32),
}
static RECORDS: Mutex<Vec<ViewConfig>> = Mutex::new(Vec::new());

fn records() -> std::sync::MutexGuard<'static, Vec<ViewConfig>> {
    RECORDS.lock().unwrap_or_else(|p| p.into_inner())
}
pub fn set_system(instance: u64, system: u64) {
    INSTANCE.store(instance, Ordering::Release);
    SYSTEM.store(system, Ordering::Release);
}
pub fn instance_and_system() -> (u64, u64) {
    (INSTANCE.load(Ordering::Acquire), SYSTEM.load(Ordering::Acquire))
}
/// The runtime's views as it reported them (the record replaced whole, as the vector's assign did).
pub fn record(views: &[xr::ViewConfigurationView]) {
    *records() = views
        .iter()
        .map(|v| ViewConfig {
            recommended: (v.recommended_image_rect_width, v.recommended_image_rect_height),
            max: (v.max_image_rect_width, v.max_image_rect_height),
        })
        .collect();
}
/// The first view's recommended image size, while any view is recorded.
pub fn recommended() -> Option<(u32, u32)> {
    records().first().map(|v| v.recommended)
}

/// OpenXRManager::GetRecommendedRenderTargetSize: false while no view is recorded; either output may be null.
///
/// # Safety
/// `width` and `height` are null or writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_recommended_render_target_size(width: *mut u32, height: *mut u32) -> bool {
    let Some((w, h)) = recommended() else { return false };
    // SAFETY: the caller's outputs, each null or writable.
    unsafe {
        if let Some(width) = width.as_mut() {
            *width = w;
        }
        if let Some(height) = height.as_mut() {
            *height = h;
        }
    }
    true
}
/// Shutdown: the record cleared.
#[no_mangle]
pub extern "C" fn cvr_xr_forget_view_configs() {
    records().clear();
}

#[cfg(test)]
mod tests {
    //! The record and the recommended size the getter reads.
    use super::*;

    fn view(rw: u32, rh: u32, mw: u32, mh: u32) -> xr::ViewConfigurationView {
        xr::ViewConfigurationView {
            ty: xr::StructureType::VIEW_CONFIGURATION_VIEW,
            next: core::ptr::null_mut(),
            recommended_image_rect_width: rw,
            max_image_rect_width: mw,
            recommended_image_rect_height: rh,
            max_image_rect_height: mh,
            recommended_swapchain_sample_count: 1,
            max_swapchain_sample_count: 1,
        }
    }

    #[test]
    fn records_and_the_recommended_size() {
        // SAFETY: null outputs are allowed; then two locals.
        assert!(!unsafe { cvr_xr_recommended_render_target_size(core::ptr::null_mut(), core::ptr::null_mut()) });
        record(&[view(1600, 1500, 4096, 4000), view(1700, 1400, 4096, 4000)]);
        let (mut w, mut h) = (0u32, 0u32);
        assert!(unsafe { cvr_xr_recommended_render_target_size(&mut w, &mut h) });
        assert_eq!((w, h), (1600, 1500));
        assert_eq!(records()[1].max, (4096, 4000));
        // Only the height asked for.
        let mut only = 0u32;
        assert!(unsafe { cvr_xr_recommended_render_target_size(core::ptr::null_mut(), &mut only) });
        assert_eq!(only, 1500);
        cvr_xr_forget_view_configs();
        assert_eq!(recommended(), None);
        set_system(7, 9);
        assert_eq!(instance_and_system(), (7, 9));
        set_system(0, 0);
    }
}
