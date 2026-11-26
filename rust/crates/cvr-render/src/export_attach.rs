//! THE EXPORTS' ATTACHMENT (formerly the static initialisers of src/Render/GpuStageProfile.cpp, LensMask.cpp,
//! CameraSourceResolve.cpp, LensSourceResolve.cpp, DesktopMirror.cpp, FixedVrs.cpp, SceneGpuProfile.cpp,
//! MonoCascades.cpp, SsrWideMerge.cpp and Stereo/Grading.cpp's block rewrites): the tables of the exported switches and counters (the moved exports' modules define them) handed
//! to the cvr-gpu and cvr-render halves that read and write them -- the GPU stage profile's request and report, the lens
//! mask's switch and counters, the camera and lens source resolves' controls, the desktop mirror's forced sizes, the
//! fixed VRS report, the scene GPU profile's switches and report, the mono cascades' switch, report and queries, the
//! SSR/AO/shadow merges' switches and debug rows with the game's device, the block rewrites' switches and debug rows. Run once as the plugin loads
//! (cvr-ffi's load), before anything renders.
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::typed_moved_exports as typed;
use crate::used_moved_exports_rest as used;

extern "C" {
    fn GetForcedDisplayModeWidth() -> u32;
    fn GetForcedDisplayModeHeight() -> u32;
    fn GetForcedWindowWidth() -> u32;
}
extern "C" fn display_width() -> u32 {
    // SAFETY: VrCore.cpp's query.
    unsafe { GetForcedDisplayModeWidth() }
}
extern "C" fn display_height() -> u32 {
    // SAFETY: as above.
    unsafe { GetForcedDisplayModeHeight() }
}
extern "C" fn window_width() -> u32 {
    // SAFETY: as above.
    unsafe { GetForcedWindowWidth() }
}
fn array<T>(values: &'static [T]) -> *const T {
    values.as_ptr()
}
/// A table whose fields are all pointer-sized (pointers, lengths, function pointers), in its C order.
fn flat<T, const N: usize>(words: [usize; N]) -> *const T {
    assert_eq!(core::mem::size_of::<T>(), N * core::mem::size_of::<usize>());
    Box::leak(Box::new(words)).as_ptr().cast()
}
/// engine_stereo::SecondViewWanted.
extern "C" fn second_view_wanted() -> bool {
    !crate::engine_stereo::cvr_engine_stereo_enabled() && !crate::engine_stereo::cvr_engine_stereo_mono_launch()
}
extern "C" {
    fn CyberpunkVR_GetGameDevice() -> *mut core::ffi::c_void;
}
/// cvr::detail::g_game_device (DeviceHooks.cpp's export).
extern "C" fn game_device() -> *mut core::ffi::c_void {
    // SAFETY: a plain read of the C++ half's device.
    unsafe { CyberpunkVR_GetGameDevice() }
}
extern "C" fn internal_commands() -> bool {
    cvr_host::probe::internal()
}
extern "C" fn set_internal_commands(on: bool) -> bool {
    cvr_host::probe::set_internal(on)
}

/// Attach every table (once).
pub fn attach() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the exports live for the process; the tables are leaked for it.
    unsafe {
        cvr_gpu::stage_profile::cvr_gpu_stage_attach(
            &used::CyberpunkVR_GpuStageRequest,
            &used::CyberpunkVR_GpuStageReportSeq,
            core::ptr::addr_of_mut!(typed::CyberpunkVR_GpuStageReport).cast(),
            2048,
        );
        cvr_gpu::optics::cvr_optics_mask_attach(
            core::ptr::from_ref(&used::CyberpunkVR_OpticsLensClean).cast_mut(),
            array(&used::CyberpunkVR_OpticsLensMaskDraws).cast_mut(),
            array(&used::CyberpunkVR_OpticsLensMaskTags).cast_mut(),
        );
        let camera = Box::leak(Box::new(cvr_gpu::optics_camera::Controls {
            enabled: &used::CyberpunkVR_OpticsCameraResolve,
            alpha: &typed::CyberpunkVR_OpticsCameraAlpha,
            motion_alpha: &typed::CyberpunkVR_OpticsCameraMotionAlpha,
            gamma: &typed::CyberpunkVR_OpticsCameraGamma,
            count: &used::CyberpunkVR_OpticsCameraResolveCount,
            misses: array(&used::CyberpunkVR_OpticsCameraResolveMisses),
            microseconds: &typed::CyberpunkVR_OpticsCameraResolveMicroseconds,
        }));
        cvr_gpu::optics_camera::cvr_optics_camera_attach(camera);
        let source = Box::leak(Box::new(cvr_gpu::optics_source::Controls {
            enabled: &used::CyberpunkVR_OpticsSourceResolve,
            alpha: &typed::CyberpunkVR_OpticsResolveAlpha,
            gamma: &typed::CyberpunkVR_OpticsResolveGamma,
            count: &used::CyberpunkVR_OpticsResolveCount,
            sides: array(&used::CyberpunkVR_OpticsResolveSides),
            microseconds: &typed::CyberpunkVR_OpticsResolveMicroseconds,
            jitter_mode: &used::CyberpunkVR_OpticsResolveJitterMode,
            sharpen: &typed::CyberpunkVR_OpticsLensSharpen,
        }));
        cvr_gpu::optics_source::cvr_optics_source_attach(source);
        cvr_gpu::desktop_mirror::cvr_desktop_mirror_attach(display_width, display_height, window_width);
        let vrs = Box::leak(Box::new(crate::fixed_vrs::Attach {
            engine_stereo_enabled: crate::engine_stereo::cvr_engine_stereo_enabled,
            report_seq: core::ptr::addr_of!(used::CyberpunkVR_FixedVrsReportSeq).cast::<AtomicU32>(),
            report: core::ptr::addr_of_mut!(typed::CyberpunkVR_FixedVrsReport).cast(),
            report_bytes: 1024,
        }));
        crate::fixed_vrs::cvr_vrs_attach(vrs);
        use crate::declared_moved_exports as declared;
        cvr_gpu::scene_profile::cvr_scene_profile_attach(flat([
            core::ptr::addr_of!(declared::CyberpunkVR_SceneGpuRequest) as usize,
            core::ptr::addr_of!(declared::CyberpunkVR_SceneGpuReportSeq) as usize,
            core::ptr::addr_of!(declared::CyberpunkVR_SceneGpuGroupMode) as usize,
            core::ptr::addr_of!(declared::CyberpunkVR_SceneGpuNodeFilter) as usize,
            core::ptr::addr_of!(declared::CyberpunkVR_SceneGpuViewportDetail) as usize,
            core::ptr::addr_of!(used::CyberpunkVR_SceneGpuFrameRequest) as usize,
            core::ptr::addr_of!(used::CyberpunkVR_SceneGpuPipelineStats) as usize,
            core::ptr::addr_of!(used::CyberpunkVR_SceneGpuDrawOcclusion) as usize,
            core::ptr::addr_of_mut!(typed::CyberpunkVR_SceneGpuReport) as usize,
            262144,
            array(&used::CyberpunkVR_PresentTiming) as usize,
        ]));
        cvr_gpu::mono_cascades::cvr_mono_cascades_attach(flat([
            core::ptr::addr_of!(declared::CyberpunkVR_MonoCascades) as usize,
            core::ptr::addr_of!(declared::CyberpunkVR_MonoCascadeReportSeq) as usize,
            core::ptr::addr_of_mut!(typed::CyberpunkVR_MonoCascadeReport) as usize,
            4096,
            second_view_wanted as extern "C" fn() -> bool as usize,
            internal_commands as extern "C" fn() -> bool as usize,
            set_internal_commands as extern "C" fn(bool) -> bool as usize,
        ]));
        crate::ssr_merge::cvr_ssr_merge_attach(flat::<crate::ssr_merge::Runtime, 31>([
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_SsrMerge) as usize,
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_SsprMerge) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_SsrMergeMode) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_SsrMergeTolerance) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugSsrMerge) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugSsprMerge) as usize,
            core::ptr::addr_of_mut!(crate::typed_moved_exports::CyberpunkVR_DebugSsrBlocks) as usize,
            core::ptr::addr_of_mut!(crate::typed_moved_exports::CyberpunkVR_DebugSsprBlocks) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymSsrPatch) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymSsrPatchLoaded) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymAoPatch) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymAoPatchLoaded) as usize,
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_AoMerge) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_AoMergeMode) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_AoMergeTolerance) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_AoMergeStrength) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugAoMerge) as usize,
            core::ptr::addr_of_mut!(crate::typed_moved_exports::CyberpunkVR_DebugAoBlocks) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymShadowPatch) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymShadowPatchLoaded) as usize,
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_ShadowMerge) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ShadowMergeMode) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugShadowMerge) as usize,
            core::ptr::addr_of_mut!(crate::typed_moved_exports::CyberpunkVR_DebugShadowBlocks) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugExpoCellsRequest) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugExpoCellsSeq) as usize,
            core::ptr::addr_of_mut!(crate::typed_moved_exports::CyberpunkVR_DebugExpoCells) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ProbeLayoutNode) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugLayoutCount) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugLayoutBytes) as usize,
            game_device as extern "C" fn() -> *mut core::ffi::c_void as usize,
        ]).cast_mut());
        crate::block_rewrites::cvr_block_rewrites_attach(flat::<crate::block_rewrites::Runtime, 27>([
            core::ptr::addr_of_mut!(crate::typed_moved_exports::CyberpunkVR_AsymAoBlockSize) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymAoPatch) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_AsymAoPatchLoaded) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugAsymAoPatched) as usize,
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_EngineStereoAoHalves) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugEngineStereoAo) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExpoCentreBlockSize) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_ExposureCentre) as usize,
            core::ptr::addr_of!(crate::shader_patch_exports::CyberpunkVR_ExposureCentreLoaded) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugExpoCentred) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_HudFlatInEye) as usize,
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_HudFlatInEyeBlocks) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_HudShadowOffset) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports_rest::CyberpunkVR_HudEngineCurvature) as usize,
            core::ptr::addr_of_mut!(crate::declared_moved_exports::CyberpunkVR_RuntimeDiagnostics) as usize,
            core::ptr::addr_of!(crate::used_moved_exports::CyberpunkVR_ExposureMemory) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureMemoryMode) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureMemoryWrite) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureMemoryAge) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureMemoryDistance) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureMemorySurprise) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureLevelTime) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureHemisphere) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureGazeGain) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_ExposureGazeLimit) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugExpoEnv) as usize,
            core::ptr::addr_of_mut!(crate::used_moved_exports::CyberpunkVR_DebugExpoEnvParams) as usize,
        ]).cast_mut());
    }
}
