// The hidden-area shader patches' exports for the Rust variant of the pipeline-lifetime test: the plugin defines them in
// Rust (rust/crates/cvr-render/src/shader_patch_exports.rs) and hands their table to the patches as it loads
// (shader_patches.rs' PLUGIN); this program stands in for both -- the definitions with the C++ build's starting values
// (copied from src/Render/HiddenAreaShaders.cpp's C++ branch), the table and the attachment as the C++ half made them
// before B394.
#include "Stereo/EngineStereo.hpp"
#include <atomic>
#include <cstdint>
#include <iterator>

extern "C" int GetMenuMode();
extern "C" std::atomic<uint32_t> CyberpunkVR_HiddenAreaEnabled;
namespace {
struct CensusRecord { uint32_t node; int32_t side; uint32_t kind, x, y, z; uint64_t hash; uint32_t count, bytes; uint64_t pso; };
}
extern "C" {
// 1 = record commands into CyberpunkVR_ShaderCensusLog and dump each shader seen once.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ShaderCensus{0};
__declspec(dllexport) CensusRecord CyberpunkVR_ShaderCensusLog[512]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ShaderCensusRecording{0};   // the marker file was found
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ShaderCensusPipelines{0};
// 1 = bind the patched twins while the mask is on.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaPatchLoaded{0};   // patched shaders found
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaPatchRegistered{0};   // engine pipelines with a patch
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaPatchBuilt{0};        // twins created
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_HiddenAreaPatchFailed{0};       // twins the device refused
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HiddenAreaPatchBinds{0};
// THE VOLUMETRIC FOG IGNORES THE PROJECTION'S NDC SHIFT. Its three froxel shaders build each froxel's ray
// as right*tanX*ndc.x + up*tanY*ndc.y + forward -- no off-centre term -- so under the asymmetric
// projection (Stereo/AsymProjection.cpp) the fog was lit for rays ~14 degrees off the pixels it was drawn
// on, and its history, reprojected with the true matrix, smeared into blocks and hanging shafts.
// tools/asym_tests/asym_fog_shaders.py builds twins that take the shift (P2x minus the TAA jitter, from the
// camera block they already bind) off the NDC first; they load from `asym\` beside the plugin. Equal to the
// engine's own shader when there is no shift, so the twin is bound whenever this is 1.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymFogPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymFogPatchLoaded{0};    // patched shaders found
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymFogPatchBuilt{0};     // twins created
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymFogPatchFailed{0};    // twins the device refused
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_AsymFogPatchBinds{0};
// THE SCREEN-SPACE REFLECTIONS IGNORE IT TOO. Their trace rebuilds each pixel's view-space point with the
// symmetric tangents of its own block (b6 row 17.zw) and projects the reflected ray back with the full
// projection (rows 0-3, which carries the shift), so under the shift the march starts ~116 px from its pixel
// (Quest 3, 960 px eye) and runs the wrong way, mirrored in the two eyes. tools/asym_tests/asym_ssr_shaders.py
// builds twins that take the projection's own centre off the NDC first; they load from `asym_ssr\`.
// The planar reflections' trace (node 0x77D450) has the same block and the same bug and is among them.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymSsrPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymSsrPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymSsrPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymSsrPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_AsymSsrPatchBinds{0};
// AND THE CONE AO WORKS IN A SHEARED VIEW SPACE. It rebuilds positions with its block's symmetric tangents
// and projects them back with the same, so nothing moves, but under the shift the space it measures corners
// in is sheared by shift * tan * z, mirrored between the eyes, and the occlusion in corners differs.
// tools/asym_tests/asym_ao_shaders.py builds twins (from `asym_ao\`) that take the centre off and put it
// back. They read it from the padding at the end of the node's block, where Stereo/Grading.cpp writes it
// with a tag, and use it only when the tag is there -- without the write they are the engine's shaders.
// The cone trace also has a hidden-area patch; its hidden_area\ file carries this fix too, and the twin
// here is that entry's fallback for when the mask is off.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymAoPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymAoPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymAoPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymAoPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_AsymAoPatchBinds{0};
// CAMERA-FACING PARTICLES FACE THE SCREEN. Billboard sprites (smoke, steam, sparks) are laid out along the
// camera's right and up axes (b1 rows 40/42) and take minus its forward axis (row 41) as their normal and as
// their push towards the viewer. On a monitor that is invisible; in a headset the camera is the head, so every
// sprite rolls with a tilted head and swings with a turned one. tools/asym_tests/particle_facing_shaders.py
// builds vertex-shader twins (from `asym_particles\`) that turn each sprite to the eye position instead. One
// shader serves many particle materials, so these twins are made with the pipeline, off the render thread.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ParticleFacing{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ParticleFacingLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ParticleFacingBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ParticleFacingFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ParticleFacingBinds{0};
// THE SUN'S CONTACT SHADOWS THIN OUT AT EACH EYE'S INNER EDGE. The shadow mask's screen-space march towards
// the sun finds nothing where it runs off the picture, while the other eye still sees the occluder there.
// tools/asym_tests/asym_shadow_shaders.py builds the shadow mask twin (from `asym_shadow\`) that merges the
// two eyes' sun factor as one wide display (Render/SsrWideMerge.cpp, channel kShadowMask); without the tag
// the plugin writes into its b6 padding it is the engine's shader.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymShadowPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymShadowPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymShadowPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AsymShadowPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_AsymShadowPatchBinds{0};
// Group 6 (view_dir\, tools/asym_tests/view_dir_shaders.py): material pixel shaders that took the camera's
// forward axis for the direction to the viewer (parallax layers of screens and holograms, facing terms) take
// the pixel's own view ray, so their picture stays on the surface when the head turns.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ViewDirPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ViewDirPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ViewDirPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ViewDirPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ViewDirPatchBinds{0};
// THE AUTO-EXPOSURE METERED THE MIDDLE OF EACH EYE'S SCREEN. Its luminance image samples the scene through a
// magnifier centred on the screen's middle, which under the asymmetric projection sits ~15 degrees outwards of
// the view: MAIN metered the right-hand side the left eye never sees, and the second eye takes MAIN's
// exposure, so head turns to either side moved the brightness of both eyes by different amounts.
// tools/asym_tests/asym_expo_shaders.py builds twins (from `asym_expo\`) that centre the magnifier on the
// view, read from the node's block where Stereo/Grading.cpp writes it with a tag, and adapt more slowly.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExposureCentre{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExposureCentreLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExposureCentreBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExposureCentreFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ExposureCentreBinds{0};
// Group 7 (screen_fx\, tools/asym_tests/screen_fx_shaders.py): engine screen effects that moved the picture
// around each eye's screen middle (the Sandevistan's radial smear and block tearing) keep their tint, grid and
// vignette without moving it.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ScreenFxPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ScreenFxPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ScreenFxPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ScreenFxPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ScreenFxPatchBinds{0};
// Group 8 (stereo_merge\, tools/asym_tests/stereo_ssr_merge_shaders.py): 2.5 engine stereo only. The ambient
// pass takes the reflections a pixel's eye lost at its inner edge from the other eye's half of the frame, where
// the same surface was traced further (the stereo branch; the mono branch is the engine's).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoMergePatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoMergePatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoMergePatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoMergePatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoMergePatchBinds{0};
// Group 9 (stereo_eye\, tools/asym_tests/stereo_eye_shaders.py): 2.5 engine stereo only. Passes dispatched once
// over both eyes that told a pixel's eye by the camera block's eye (one block for the whole dispatch) instead of
// the pixel's half -- the volumetric clouds' fog lookup, which gave the left eye's clouds the right eye's haze --
// or rebuilt the world position with the frame camera over both halves: the underwater volume, its fog and depth
// under the surface different in each eye.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoEyePatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoEyePatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoEyePatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoEyePatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoEyePatchBinds{0};
// Group 10 (stereo_bloom\): normalized HDR weights in the first bloom downsample.
// The shader retains the native mono path, thresholds, eye boundaries and mip clamps.
// Live A/B on 2.5 with Transformer: thin lamp highlights stop dominating the bloom pyramid.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoBloomPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoBloomPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoBloomPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoBloomPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoBloomPatchBinds{0};
// Group 11: a shared particle-lighting tile is needed if either eye sees the particle.
// The native allocator rejected it on the first occluded eye, leaving the other eye
// to read unrelated atlas tiles (ParticleTriangles.rdc).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoParticleLightingPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoParticleLightingPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoParticleLightingPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoParticleLightingPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoParticleLightingPatchBinds{0};
// Group 12: clustered-light sphere/AABB bounds use each eye's projection scale,
// matching the native planes, instead of the wider union camera's scale.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightCullPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightCullPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightCullPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightCullPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoLightCullPatchBinds{0};
// Group 13: the native distant-light culler projects with the union camera into
// an eye-packed HiZ image. Test each eye's bounds in its own rectangle instead;
// moving geometry at unrelated coordinates must not make facade lamps flicker.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoDistantLightPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoDistantLightPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoDistantLightPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoDistantLightPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoDistantLightPatchBinds{0};
// Group 14: the clustered probe/decal culler, which also writes the ambient pass's per-tile probe
// mask, scales its bounds by the union camera like the light culler did. Each eye's scale instead;
// a probe dropped from a tile it covers left a 32-pixel border that slid with the head.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoProbeCullPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoProbeCullPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoProbeCullPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoProbeCullPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoProbeCullPatchBinds{0};
// Group 15 (stereo_rt\, tools/asym_tests/stereo_rt_shaders.py): the ray-traced reflection raygens (DXIL
// libraries) take the view direction from the eye instead of the stereo centre their world inverse is relative
// to. Swapped when the state object is created (Stereo/DeviceHooks.cpp), so the switch takes effect at the next
// creation, not live; Binds counts the state objects created with a twin.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoRtPatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoRtPatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoRtPatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoRtPatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoRtPatchBinds{0};
// Group 16 (stereo_light_merge\, tools/asym_tests/stereo_light_merge_shaders.py): 2.5 engine stereo only. The tiled
// light pass is dispatched once per eye over the whole frame's tile list, each dispatch leaving the other eye's tiles
// after a tile and a depth load. These twins light every tile with its own pixel's eye in the LEFT eye's dispatch and
// leave the right eye's dispatch right after its group barrier. An ALTERNATE of the stereo_merge twin of the same
// shader: bound instead of it while this switch is on.
// Measured (vr_bench ab light-merge, ABBA x3, still): RenderLightBuffers 2.64 -> 2.33 ms, stereo agreement and both
// eyes' pictures unchanged (the difference to the per-eye twins within the frame-to-frame noise).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightMergePatch{1};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightMergePatchLoaded{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightMergePatchBuilt{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoLightMergePatchFailed{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoLightMergePatchBinds{0};
}

namespace {
static_assert(sizeof(CensusRecord)==48,"CensusRecord is shared with Rust");
struct GroupCounters { std::atomic<uint32_t>* enabled; std::atomic<uint32_t>* loaded; std::atomic<uint32_t>* built;
                       std::atomic<uint32_t>* failed; std::atomic<uint64_t>* binds; };
struct Counters {
    std::atomic<uint32_t>* hiddenAreaEnabled;
    std::atomic<uint32_t>* census; std::atomic<uint32_t>* censusRecording; std::atomic<uint64_t>* censusPipelines;
    CensusRecord* censusLog; uint32_t censusLogLength;
    std::atomic<uint32_t>* hamPatch; std::atomic<uint32_t>* hamLoaded; std::atomic<uint32_t>* hamRegistered;
    std::atomic<uint32_t>* hamBuilt; std::atomic<uint32_t>* hamFailed; std::atomic<uint64_t>* hamBinds;
    GroupCounters groups[17];   // the order of shader_patches.rs FOLDERS
};
#define CVR_GROUP(name) GroupCounters{&CyberpunkVR_##name,&CyberpunkVR_##name##Loaded,&CyberpunkVR_##name##Built,\
                                      &CyberpunkVR_##name##Failed,&CyberpunkVR_##name##Binds}
const Counters kCounters{
    &CyberpunkVR_HiddenAreaEnabled,
    &CyberpunkVR_ShaderCensus,&CyberpunkVR_ShaderCensusRecording,&CyberpunkVR_ShaderCensusPipelines,
    CyberpunkVR_ShaderCensusLog,uint32_t(std::size(CyberpunkVR_ShaderCensusLog)),
    &CyberpunkVR_HiddenAreaPatch,&CyberpunkVR_HiddenAreaPatchLoaded,&CyberpunkVR_HiddenAreaPatchRegistered,
    &CyberpunkVR_HiddenAreaPatchBuilt,&CyberpunkVR_HiddenAreaPatchFailed,&CyberpunkVR_HiddenAreaPatchBinds,
    {
    CVR_GROUP(AsymFogPatch),
    CVR_GROUP(AsymSsrPatch),
    CVR_GROUP(AsymAoPatch),
    CVR_GROUP(ParticleFacing),
    CVR_GROUP(ExposureCentre),
    CVR_GROUP(AsymShadowPatch),
    CVR_GROUP(ViewDirPatch),
    CVR_GROUP(ScreenFxPatch),
    CVR_GROUP(StereoMergePatch),
    CVR_GROUP(StereoEyePatch),
    CVR_GROUP(StereoBloomPatch),
    CVR_GROUP(StereoParticleLightingPatch),
    CVR_GROUP(StereoLightCullPatch),
    CVR_GROUP(StereoDistantLightPatch),
    CVR_GROUP(StereoProbeCullPatch),
    CVR_GROUP(StereoRtPatch),
    CVR_GROUP(StereoLightMergePatch)
    }};
#undef CVR_GROUP
bool EngineStereoEnabled(){return cvr::engine_stereo::Enabled();}
int MenuMode(){return GetMenuMode();}
}
extern "C" void cvr_shader_patches_attach(const Counters* counters,bool (*engineStereo)(),int (*menuMode)());
namespace {
// While the DLL initialises, before any pipeline is created: Rust reads every switch as off until then.
const bool attached=(cvr_shader_patches_attach(&kCounters,&EngineStereoEnabled,&MenuMode),true);
}
