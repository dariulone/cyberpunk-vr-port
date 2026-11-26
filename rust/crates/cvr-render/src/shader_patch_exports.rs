//! The hidden-area shader patches' and the shader census' switches and counters (formerly defined in
//! src/Render/HiddenAreaShaders.cpp; shader_patches.rs' PLUGIN table takes their addresses). Generated from the definition lines; the scratch
//! check_moved compares every variable's bytes with the C++ definitions'.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, AtomicU64};

// ---- src/Render/HiddenAreaShaders.cpp

/// The census log's rows (the commands the census records) and their count.
pub const CENSUS_LOG_LENGTH: usize = 512;
#[repr(transparent)]
pub struct CensusLog(pub core::cell::UnsafeCell<[crate::shader_patches::CensusRecord; CENSUS_LOG_LENGTH]>);
// SAFETY: written by the census under its own sequence, read by the tools from outside.
unsafe impl Sync for CensusLog {}
#[no_mangle]
pub static CyberpunkVR_ShaderCensusLog: CensusLog =
    CensusLog(core::cell::UnsafeCell::new([crate::shader_patches::CensusRecord::ZERO; CENSUS_LOG_LENGTH]));

/// 1 = record commands into CyberpunkVR_ShaderCensusLog and dump each shader seen once.
#[no_mangle]
pub static CyberpunkVR_ShaderCensus: AtomicU32 = AtomicU32::new(0);
/// the marker file was found
#[no_mangle]
pub static CyberpunkVR_ShaderCensusRecording: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ShaderCensusPipelines: AtomicU64 = AtomicU64::new(0);
/// 1 = bind the patched twins while the mask is on.
#[no_mangle]
pub static CyberpunkVR_HiddenAreaPatch: AtomicU32 = AtomicU32::new(1);
/// patched shaders found
#[no_mangle]
pub static CyberpunkVR_HiddenAreaPatchLoaded: AtomicU32 = AtomicU32::new(0);
/// engine pipelines with a patch
#[no_mangle]
pub static CyberpunkVR_HiddenAreaPatchRegistered: AtomicU32 = AtomicU32::new(0);
/// twins created
#[no_mangle]
pub static CyberpunkVR_HiddenAreaPatchBuilt: AtomicU32 = AtomicU32::new(0);
/// twins the device refused
#[no_mangle]
pub static CyberpunkVR_HiddenAreaPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_HiddenAreaPatchBinds: AtomicU64 = AtomicU64::new(0);
/// THE VOLUMETRIC FOG IGNORES THE PROJECTION'S NDC SHIFT. Its three froxel shaders build each froxel's ray
/// as right*tanX*ndc.x + up*tanY*ndc.y + forward -- no off-centre term -- so under the asymmetric
/// projection (Stereo/AsymProjection.cpp) the fog was lit for rays ~14 degrees off the pixels it was drawn
/// on, and its history, reprojected with the true matrix, smeared into blocks and hanging shafts.
/// tools/asym_tests/asym_fog_shaders.py builds twins that take the shift (P2x minus the TAA jitter, from the
/// camera block they already bind) off the NDC first; they load from `asym\` beside the plugin. Equal to the
/// engine's own shader when there is no shift, so the twin is bound whenever this is 1.
#[no_mangle]
pub static CyberpunkVR_AsymFogPatch: AtomicU32 = AtomicU32::new(1);
/// patched shaders found
#[no_mangle]
pub static CyberpunkVR_AsymFogPatchLoaded: AtomicU32 = AtomicU32::new(0);
/// twins created
#[no_mangle]
pub static CyberpunkVR_AsymFogPatchBuilt: AtomicU32 = AtomicU32::new(0);
/// twins the device refused
#[no_mangle]
pub static CyberpunkVR_AsymFogPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymFogPatchBinds: AtomicU64 = AtomicU64::new(0);
/// THE SCREEN-SPACE REFLECTIONS IGNORE IT TOO. Their trace rebuilds each pixel's view-space point with the
/// symmetric tangents of its own block (b6 row 17.zw) and projects the reflected ray back with the full
/// projection (rows 0-3, which carries the shift), so under the shift the march starts ~116 px from its pixel
/// (Quest 3, 960 px eye) and runs the wrong way, mirrored in the two eyes. tools/asym_tests/asym_ssr_shaders.py
/// builds twins that take the projection's own centre off the NDC first; they load from `asym_ssr\`.
/// The planar reflections' trace (node 0x77D450) has the same block and the same bug and is among them.
#[no_mangle]
pub static CyberpunkVR_AsymSsrPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_AsymSsrPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymSsrPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymSsrPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymSsrPatchBinds: AtomicU64 = AtomicU64::new(0);
/// AND THE CONE AO WORKS IN A SHEARED VIEW SPACE. It rebuilds positions with its block's symmetric tangents
/// and projects them back with the same, so nothing moves, but under the shift the space it measures corners
/// in is sheared by shift * tan * z, mirrored between the eyes, and the occlusion in corners differs.
/// tools/asym_tests/asym_ao_shaders.py builds twins (from `asym_ao\`) that take the centre off and put it
/// back. They read it from the padding at the end of the node's block, where Stereo/Grading.cpp writes it
/// with a tag, and use it only when the tag is there -- without the write they are the engine's shaders.
/// The cone trace also has a hidden-area patch; its hidden_area\ file carries this fix too, and the twin
/// here is that entry's fallback for when the mask is off.
#[no_mangle]
pub static CyberpunkVR_AsymAoPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_AsymAoPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymAoPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymAoPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymAoPatchBinds: AtomicU64 = AtomicU64::new(0);
/// CAMERA-FACING PARTICLES FACE THE SCREEN. Billboard sprites (smoke, steam, sparks) are laid out along the
/// camera's right and up axes (b1 rows 40/42) and take minus its forward axis (row 41) as their normal and as
/// their push towards the viewer. On a monitor that is invisible; in a headset the camera is the head, so every
/// sprite rolls with a tilted head and swings with a turned one. tools/asym_tests/particle_facing_shaders.py
/// builds vertex-shader twins (from `asym_particles\`) that turn each sprite to the eye position instead. One
/// shader serves many particle materials, so these twins are made with the pipeline, off the render thread.
#[no_mangle]
pub static CyberpunkVR_ParticleFacing: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_ParticleFacingLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ParticleFacingBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ParticleFacingFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ParticleFacingBinds: AtomicU64 = AtomicU64::new(0);
/// THE SUN'S CONTACT SHADOWS THIN OUT AT EACH EYE'S INNER EDGE. The shadow mask's screen-space march towards
/// the sun finds nothing where it runs off the picture, while the other eye still sees the occluder there.
/// tools/asym_tests/asym_shadow_shaders.py builds the shadow mask twin (from `asym_shadow\`) that merges the
/// two eyes' sun factor as one wide display (Render/SsrWideMerge.cpp, channel kShadowMask); without the tag
/// the plugin writes into its b6 padding it is the engine's shader.
#[no_mangle]
pub static CyberpunkVR_AsymShadowPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_AsymShadowPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymShadowPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymShadowPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_AsymShadowPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 6 (view_dir\, tools/asym_tests/view_dir_shaders.py): material pixel shaders that took the camera's
/// forward axis for the direction to the viewer (parallax layers of screens and holograms, facing terms) take
/// the pixel's own view ray, so their picture stays on the surface when the head turns.
#[no_mangle]
pub static CyberpunkVR_ViewDirPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_ViewDirPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ViewDirPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ViewDirPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ViewDirPatchBinds: AtomicU64 = AtomicU64::new(0);
/// THE AUTO-EXPOSURE METERED THE MIDDLE OF EACH EYE'S SCREEN. Its luminance image samples the scene through a
/// magnifier centred on the screen's middle, which under the asymmetric projection sits ~15 degrees outwards of
/// the view: MAIN metered the right-hand side the left eye never sees, and the second eye takes MAIN's
/// exposure, so head turns to either side moved the brightness of both eyes by different amounts.
/// tools/asym_tests/asym_expo_shaders.py builds twins (from `asym_expo\`) that centre the magnifier on the
/// view, read from the node's block where Stereo/Grading.cpp writes it with a tag, and adapt more slowly.
#[no_mangle]
pub static CyberpunkVR_ExposureCentre: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_ExposureCentreLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ExposureCentreBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ExposureCentreFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ExposureCentreBinds: AtomicU64 = AtomicU64::new(0);
/// Group 7 (screen_fx\, tools/asym_tests/screen_fx_shaders.py): engine screen effects that moved the picture
/// around each eye's screen middle (the Sandevistan's radial smear and block tearing) keep their tint, grid and
/// vignette without moving it.
#[no_mangle]
pub static CyberpunkVR_ScreenFxPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_ScreenFxPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ScreenFxPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ScreenFxPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ScreenFxPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 8 (stereo_merge\, tools/asym_tests/stereo_ssr_merge_shaders.py): 2.5 engine stereo only. The ambient
/// pass takes the reflections a pixel's eye lost at its inner edge from the other eye's half of the frame, where
/// the same surface was traced further (the stereo branch; the mono branch is the engine's).
#[no_mangle]
pub static CyberpunkVR_StereoMergePatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoMergePatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoMergePatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoMergePatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoMergePatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 9 (stereo_eye\, tools/asym_tests/stereo_eye_shaders.py): 2.5 engine stereo only. Passes dispatched once
/// over both eyes that told a pixel's eye by the camera block's eye (one block for the whole dispatch) instead of
/// the pixel's half -- the volumetric clouds' fog lookup, which gave the left eye's clouds the right eye's haze --
/// or rebuilt the world position with the frame camera over both halves: the underwater volume, its fog and depth
/// under the surface different in each eye.
#[no_mangle]
pub static CyberpunkVR_StereoEyePatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoEyePatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoEyePatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoEyePatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoEyePatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 10 (stereo_bloom\): normalized HDR weights in the first bloom downsample.
/// The shader retains the native mono path, thresholds, eye boundaries and mip clamps.
/// Live A/B on 2.5 with Transformer: thin lamp highlights stop dominating the bloom pyramid.
#[no_mangle]
pub static CyberpunkVR_StereoBloomPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoBloomPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoBloomPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoBloomPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoBloomPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 11: a shared particle-lighting tile is needed if either eye sees the particle.
/// The native allocator rejected it on the first occluded eye, leaving the other eye
/// to read unrelated atlas tiles (ParticleTriangles.rdc).
#[no_mangle]
pub static CyberpunkVR_StereoParticleLightingPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoParticleLightingPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoParticleLightingPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoParticleLightingPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoParticleLightingPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 12: clustered-light sphere/AABB bounds use each eye's projection scale,
/// matching the native planes, instead of the wider union camera's scale.
#[no_mangle]
pub static CyberpunkVR_StereoLightCullPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoLightCullPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoLightCullPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoLightCullPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoLightCullPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 13: the native distant-light culler projects with the union camera into
/// an eye-packed HiZ image. Test each eye's bounds in its own rectangle instead;
/// moving geometry at unrelated coordinates must not make facade lamps flicker.
#[no_mangle]
pub static CyberpunkVR_StereoDistantLightPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoDistantLightPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoDistantLightPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoDistantLightPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoDistantLightPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 14: the clustered probe/decal culler, which also writes the ambient pass's per-tile probe
/// mask, scales its bounds by the union camera like the light culler did. Each eye's scale instead;
/// a probe dropped from a tile it covers left a 32-pixel border that slid with the head.
#[no_mangle]
pub static CyberpunkVR_StereoProbeCullPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoProbeCullPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoProbeCullPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoProbeCullPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoProbeCullPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 15 (stereo_rt\, tools/asym_tests/stereo_rt_shaders.py): the ray-traced reflection raygens (DXIL
/// libraries) take the view direction from the eye instead of the stereo centre their world inverse is relative
/// to. Swapped when the state object is created (Stereo/DeviceHooks.cpp), so the switch takes effect at the next
/// creation, not live; Binds counts the state objects created with a twin.
#[no_mangle]
pub static CyberpunkVR_StereoRtPatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoRtPatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoRtPatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoRtPatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoRtPatchBinds: AtomicU64 = AtomicU64::new(0);
/// Group 16 (stereo_light_merge\, tools/asym_tests/stereo_light_merge_shaders.py): 2.5 engine stereo only. The tiled
/// light pass is dispatched once per eye over the whole frame's tile list, each dispatch leaving the other eye's tiles
/// after a tile and a depth load. These twins light every tile with its own pixel's eye in the LEFT eye's dispatch and
/// leave the right eye's dispatch right after its group barrier. An ALTERNATE of the stereo_merge twin of the same
/// shader: bound instead of it while this switch is on.
/// Measured (vr_bench ab light-merge, ABBA x3, still): RenderLightBuffers 2.64 -> 2.33 ms, stereo agreement and both
/// eyes' pictures unchanged (the difference to the per-eye twins within the frame-to-frame noise).
#[no_mangle]
pub static CyberpunkVR_StereoLightMergePatch: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_StereoLightMergePatchLoaded: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoLightMergePatchBuilt: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoLightMergePatchFailed: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoLightMergePatchBinds: AtomicU64 = AtomicU64::new(0);
