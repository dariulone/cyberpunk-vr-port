// Hidden-area shader patches and the shader census (Render/HiddenAreaShaders.hpp).
#include "Render/HiddenAreaShaders.hpp"
#include "Core/Paths.hpp"
#include "Render/DxilPatch.hpp"
#include "Stereo/EngineStereo.hpp"
#include <windows.h>
#include <wrl/client.h>
#include <atomic>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

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
// the pixel's half -- the volumetric clouds' fog lookup, which gave the left eye's clouds the right eye's haze.
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
}

namespace cvr::hidden_area::shaders {
namespace {
// The census switch file and its dumps: the plugin's log folder.
std::string Folder() { return cvr::paths::DirA(cvr::paths::Folder::Log); }
bool Recording() {
    static const bool on=[]{
        const auto folder=Folder();
        const bool found=!folder.empty() && GetFileAttributesA((folder+"cvr_shader_census.txt").c_str())!=INVALID_FILE_ATTRIBUTES;
        CyberpunkVR_ShaderCensusRecording.store(found?1:0,std::memory_order_relaxed);
        return found;
    }();
    return on;
}
uint64_t Hash(const void* data,size_t size) {   // FNV-1a, 64 bit
    uint64_t h=1469598103934665603ull;
    for(size_t i=0;i<size;++i){h^=static_cast<const uint8_t*>(data)[i];h*=1099511628211ull;}
    return h;
}
struct Shader { uint64_t hash; std::shared_ptr<const std::vector<uint8_t>> bytes; };
std::mutex mutex,logMutex;
std::unordered_map<uint64_t,std::shared_ptr<const std::vector<uint8_t>>> blobs;
std::unordered_map<const ID3D12PipelineState*,Shader> pipelines,vertexShaders;
std::unordered_set<uint64_t> written;

void Write(uint64_t hash,const std::vector<uint8_t>& bytes) {
    const auto folder=Folder()+"cvr_shaders\\";
    CreateDirectoryA(folder.c_str(),nullptr);
    char name[64];sprintf_s(name,"%016llx.dxil",static_cast<unsigned long long>(hash));
    const HANDLE file=CreateFileA((folder+name).c_str(),GENERIC_WRITE,FILE_SHARE_READ,nullptr,CREATE_ALWAYS,FILE_ATTRIBUTE_NORMAL,nullptr);
    if(file==INVALID_HANDLE_VALUE)return;
    DWORD wrote{};WriteFile(file,bytes.data(),DWORD(bytes.size()),&wrote,nullptr);CloseHandle(file);
}
}

namespace {
// The twins' folders (hidden_area\, asym\, ...) are in the plugin's shaders folder.
std::wstring PluginFolder() { return cvr::paths::Dir(cvr::paths::Folder::Shaders); }
// A signed container followed by 'HAM1', u32 count and the bit offset of each placeholder; or, for a
// FIXED patch (asym\), the signed container alone, used as it is.
// group: 0 = volumetric fog (asym\), 1 = screen-space reflections (asym_ssr\), 2 = cone AO (asym_ao\),
// 3 = particle vertex shaders (asym_particles\), 4 = auto-exposure (asym_expo\), 5 = the shadow mask
// (asym_shadow\), 6 = material view direction (view_dir\), 7 = screen effects (screen_fx\), 8 = engine stereo
// merges (stereo_merge\), 9 = engine stereo eye-of-pixel fixes (stereo_eye\),
// 10 = stereo bloom prefilter (stereo_bloom\), 11 = shared particle-lighting visibility
// (stereo_particle_lighting\), 12 = per-eye light-culling bounds (stereo_light_cull\),
// 13 = per-eye distant-light HiZ occlusion (stereo_distant_light\), 14 = per-eye probe/decal culling bounds
// (stereo_probe_cull\), 15 = ray-traced reflections' view direction per eye (stereo_rt\, DXIL libraries).
// Each fixed group has its switch. A hidden-area patch whose
// shader also has a fixed twin keeps that twin as `fallback`, bound while the mask is not.
struct Patch { std::vector<uint8_t> container; uint32_t offsets[4]{}; bool fixed=false; int group=0;
               std::vector<uint8_t> fallback; int fallbackGroup=-1; };
// eager: the twin is created with the original pipeline instead of at its first bind.
struct FixedGroup { std::atomic<uint32_t>* enabled; std::atomic<uint32_t>* built; std::atomic<uint32_t>* failed; std::atomic<uint64_t>* binds; bool eager; };
constexpr int kStereoRtGroup=15;
const FixedGroup kFixedGroups[16]={
    {&CyberpunkVR_AsymFogPatch,&CyberpunkVR_AsymFogPatchBuilt,&CyberpunkVR_AsymFogPatchFailed,&CyberpunkVR_AsymFogPatchBinds,false},
    {&CyberpunkVR_AsymSsrPatch,&CyberpunkVR_AsymSsrPatchBuilt,&CyberpunkVR_AsymSsrPatchFailed,&CyberpunkVR_AsymSsrPatchBinds,false},
    {&CyberpunkVR_AsymAoPatch,&CyberpunkVR_AsymAoPatchBuilt,&CyberpunkVR_AsymAoPatchFailed,&CyberpunkVR_AsymAoPatchBinds,false},
    {&CyberpunkVR_ParticleFacing,&CyberpunkVR_ParticleFacingBuilt,&CyberpunkVR_ParticleFacingFailed,&CyberpunkVR_ParticleFacingBinds,true},
    {&CyberpunkVR_ExposureCentre,&CyberpunkVR_ExposureCentreBuilt,&CyberpunkVR_ExposureCentreFailed,&CyberpunkVR_ExposureCentreBinds,false},
    {&CyberpunkVR_AsymShadowPatch,&CyberpunkVR_AsymShadowPatchBuilt,&CyberpunkVR_AsymShadowPatchFailed,&CyberpunkVR_AsymShadowPatchBinds,false},
    {&CyberpunkVR_ViewDirPatch,&CyberpunkVR_ViewDirPatchBuilt,&CyberpunkVR_ViewDirPatchFailed,&CyberpunkVR_ViewDirPatchBinds,true},
    {&CyberpunkVR_ScreenFxPatch,&CyberpunkVR_ScreenFxPatchBuilt,&CyberpunkVR_ScreenFxPatchFailed,&CyberpunkVR_ScreenFxPatchBinds,false},
    {&CyberpunkVR_StereoMergePatch,&CyberpunkVR_StereoMergePatchBuilt,&CyberpunkVR_StereoMergePatchFailed,&CyberpunkVR_StereoMergePatchBinds,false},
    {&CyberpunkVR_StereoEyePatch,&CyberpunkVR_StereoEyePatchBuilt,&CyberpunkVR_StereoEyePatchFailed,&CyberpunkVR_StereoEyePatchBinds,false},
    {&CyberpunkVR_StereoBloomPatch,&CyberpunkVR_StereoBloomPatchBuilt,&CyberpunkVR_StereoBloomPatchFailed,&CyberpunkVR_StereoBloomPatchBinds,false},
    {&CyberpunkVR_StereoParticleLightingPatch,&CyberpunkVR_StereoParticleLightingPatchBuilt,&CyberpunkVR_StereoParticleLightingPatchFailed,&CyberpunkVR_StereoParticleLightingPatchBinds,false},
    {&CyberpunkVR_StereoLightCullPatch,&CyberpunkVR_StereoLightCullPatchBuilt,&CyberpunkVR_StereoLightCullPatchFailed,&CyberpunkVR_StereoLightCullPatchBinds,false},
    {&CyberpunkVR_StereoDistantLightPatch,&CyberpunkVR_StereoDistantLightPatchBuilt,&CyberpunkVR_StereoDistantLightPatchFailed,&CyberpunkVR_StereoDistantLightPatchBinds,false},
    {&CyberpunkVR_StereoProbeCullPatch,&CyberpunkVR_StereoProbeCullPatchBuilt,&CyberpunkVR_StereoProbeCullPatchFailed,&CyberpunkVR_StereoProbeCullPatchBinds,false},
    {&CyberpunkVR_StereoRtPatch,&CyberpunkVR_StereoRtPatchBuilt,&CyberpunkVR_StereoRtPatchFailed,&CyberpunkVR_StereoRtPatchBinds,false},
};
const std::unordered_map<uint64_t,Patch>& Patches() {
    static const std::unordered_map<uint64_t,Patch> library=[]{
        std::unordered_map<uint64_t,Patch> l;
        const auto folder=PluginFolder()+L"hidden_area\\";
        WIN32_FIND_DATAW found{};
        const HANDLE find=FindFirstFileW((folder+L"*.dxil").c_str(),&found);
        if(find!=INVALID_HANDLE_VALUE) do {
            const HANDLE file=CreateFileW((folder+found.cFileName).c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,0,nullptr);
            if(file==INVALID_HANDLE_VALUE)continue;
            std::vector<uint8_t> bytes(found.nFileSizeLow);DWORD read{};
            const bool ok=ReadFile(file,bytes.data(),DWORD(bytes.size()),&read,nullptr) && read==bytes.size();
            CloseHandle(file);
            uint32_t size{},count{};
            if(!ok || bytes.size()<32 || memcmp(bytes.data(),"DXBC",4))continue;
            memcpy(&size,bytes.data()+24,4);
            if(size_t(size)+8+16!=bytes.size() || memcmp(bytes.data()+size,"HAM1",4))continue;
            memcpy(&count,bytes.data()+size+4,4);
            if(count!=4)continue;
            Patch patch;memcpy(patch.offsets,bytes.data()+size+8,16);
            bytes.resize(size);patch.container=std::move(bytes);
            const auto hash=dxil_patch::ContainerHash(patch.container.data(),size);
            if(memcmp(hash.data(),patch.container.data()+4,16))continue;
            l.emplace(wcstoull(found.cFileName,nullptr,16),std::move(patch));
        } while(FindNextFileW(find,&found));
        if(find!=INVALID_HANDLE_VALUE)FindClose(find);
        CyberpunkVR_HiddenAreaPatchLoaded.store(uint32_t(l.size()),std::memory_order_relaxed);
        // Fixed patches: a signed container, nothing appended.
        const auto loadFixed=[&](const wchar_t* sub,int group)->uint32_t {
            const auto fixedFolder=PluginFolder()+sub;
            const HANDLE findFixed=FindFirstFileW((fixedFolder+L"*.dxil").c_str(),&found);
            uint32_t fixedCount=0;
            if(findFixed==INVALID_HANDLE_VALUE)return 0;
            do {
                const HANDLE file=CreateFileW((fixedFolder+found.cFileName).c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,0,nullptr);
                if(file==INVALID_HANDLE_VALUE)continue;
                std::vector<uint8_t> bytes(found.nFileSizeLow);DWORD read{};
                const bool ok=ReadFile(file,bytes.data(),DWORD(bytes.size()),&read,nullptr) && read==bytes.size();
                CloseHandle(file);
                uint32_t size{};
                if(!ok || bytes.size()<32 || memcmp(bytes.data(),"DXBC",4))continue;
                memcpy(&size,bytes.data()+24,4);
                if(size_t(size)!=bytes.size())continue;
                const auto hash=dxil_patch::ContainerHash(bytes.data(),size);
                if(memcmp(hash.data(),bytes.data()+4,16))continue;
                const uint64_t key=wcstoull(found.cFileName,nullptr,16);
                if(const auto it=l.find(key);it!=l.end()) {
                    if(!it->second.fixed && it->second.fallback.empty()) {
                        it->second.fallback=std::move(bytes);it->second.fallbackGroup=group;++fixedCount;
                    }
                    continue;
                }
                Patch patch;patch.container=std::move(bytes);patch.fixed=true;patch.group=group;
                l.emplace(key,std::move(patch));++fixedCount;
            } while(FindNextFileW(findFixed,&found));
            FindClose(findFixed);
            return fixedCount;
        };
        CyberpunkVR_AsymFogPatchLoaded.store(loadFixed(L"asym\\",0),std::memory_order_relaxed);
        CyberpunkVR_AsymSsrPatchLoaded.store(loadFixed(L"asym_ssr\\",1),std::memory_order_relaxed);
        CyberpunkVR_AsymAoPatchLoaded.store(loadFixed(L"asym_ao\\",2),std::memory_order_relaxed);
        CyberpunkVR_ParticleFacingLoaded.store(loadFixed(L"asym_particles\\",3),std::memory_order_relaxed);
        CyberpunkVR_ExposureCentreLoaded.store(loadFixed(L"asym_expo\\",4),std::memory_order_relaxed);
        CyberpunkVR_AsymShadowPatchLoaded.store(loadFixed(L"asym_shadow\\",5),std::memory_order_relaxed);
        CyberpunkVR_ViewDirPatchLoaded.store(loadFixed(L"view_dir\\",6),std::memory_order_relaxed);
        CyberpunkVR_ScreenFxPatchLoaded.store(loadFixed(L"screen_fx\\",7),std::memory_order_relaxed);
        CyberpunkVR_StereoMergePatchLoaded.store(loadFixed(L"stereo_merge\\",8),std::memory_order_relaxed);
        CyberpunkVR_StereoEyePatchLoaded.store(loadFixed(L"stereo_eye\\",9),std::memory_order_relaxed);
        CyberpunkVR_StereoBloomPatchLoaded.store(loadFixed(L"stereo_bloom\\",10),std::memory_order_relaxed);
        CyberpunkVR_StereoParticleLightingPatchLoaded.store(loadFixed(L"stereo_particle_lighting\\",11),std::memory_order_relaxed);
        CyberpunkVR_StereoLightCullPatchLoaded.store(loadFixed(L"stereo_light_cull\\",12),std::memory_order_relaxed);
        CyberpunkVR_StereoDistantLightPatchLoaded.store(loadFixed(L"stereo_distant_light\\",13),std::memory_order_relaxed);
        CyberpunkVR_StereoProbeCullPatchLoaded.store(loadFixed(L"stereo_probe_cull\\",14),std::memory_order_relaxed);
        CyberpunkVR_StereoRtPatchLoaded.store(loadFixed(L"stereo_rt\\",kStereoRtGroup),std::memory_order_relaxed);
        return l;
    }();
    return library;
}
const Patch* PatchFor(const D3D12_SHADER_BYTECODE& code) {
    if(!code.pShaderBytecode || !code.BytecodeLength)return nullptr;
    const auto& library=Patches();
    if(library.empty())return nullptr;
    const auto it=library.find(Hash(code.pShaderBytecode,code.BytecodeLength));
    return it==library.end()?nullptr:&it->second;
}

// Placeholder values for one view: 1 + scale/8, so every value stays in [1, 2).
struct Params {
    float p[4]{};
    bool operator==(const Params& o)const{return !memcmp(p,o.p,sizeof(p));}
};
// A view's ellipse is published only after it held for this many frames, so dragging the slider
// does not build a twin per intermediate value.
constexpr uint32_t StableFrames=30;
std::mutex publishMutex;
Params pending[2],published[2];
uint32_t stable[2]{};
std::atomic<uint64_t> publishedGeneration[2]{},generations{0};

constexpr size_t MaxTwinsPerPipeline=16;
struct Entry final : IUnknown {
    std::atomic<ULONG> references{1};
    const Patch* patch{};
    Creator create;
    std::mutex mutex;
    std::vector<std::pair<Params,ID3D12PipelineState*>> built;   // nullptr: creation failed
    std::atomic<uint64_t> generation[2]{};
    std::atomic<ID3D12PipelineState*> twin[2]{};
    std::atomic<uint32_t> fixedDone{0};                  // the fixed twin (or fallback) was attempted
    std::atomic<ID3D12PipelineState*> fixedTwin{};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;
        *out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override {return ++references;}
    ULONG STDMETHODCALLTYPE Release() override {
        const ULONG remaining=--references;if(!remaining)delete this;return remaining;
    }
    ~Entry() {
        for(const auto& item:built)if(item.second)item.second->Release();
        if(auto* pipeline=fixedTwin.load(std::memory_order_relaxed))pipeline->Release();
    }
};
// A PSO owns its replacements. An address can be reused for an unrelated, unpatched PSO, including
// one created outside our device hooks; only private data identifies the actual object lifetime.
// GetPrivateData adds a reference, so a lookup remains alive while Bind uses the entry.
constexpr GUID entryKey{0x9b9c51a9,0x78bd,0x4dc7,{0x9f,0xf5,0x58,0x54,0x62,0x32,0x31,0x7c}};
std::mutex registrationMutex;
std::atomic<uint32_t> registered{0};
Microsoft::WRL::ComPtr<Entry> Find(ID3D12PipelineState* pso) {
    Microsoft::WRL::ComPtr<Entry> result;
    UINT bytes=sizeof(Entry*);Entry* entry{};
    if(SUCCEEDED(pso->GetPrivateData(entryKey,&bytes,&entry)) && bytes==sizeof(entry))result.Attach(entry);
    return result;
}

ID3D12PipelineState* Build(Entry& e,const Params& params) {
    for(const auto& [p,twin]:e.built)if(p==params)return twin;
    if(e.built.size()>=MaxTwinsPerPipeline)return nullptr;
    auto bytes=e.patch->container;
    bool ok=true;
    for(int i=0;i<4;++i)ok=ok && dxil_patch::WriteFloat(bytes,e.patch->offsets[i],params.p[i]);
    ID3D12PipelineState* twin=ok && dxil_patch::Sign(bytes)?e.create({bytes.data(),bytes.size()}):nullptr;
    e.built.emplace_back(params,twin);
    (twin?CyberpunkVR_HiddenAreaPatchBuilt:CyberpunkVR_HiddenAreaPatchFailed).fetch_add(1,std::memory_order_relaxed);
    return twin;
}
// The one twin of a fixed patch, created once.
ID3D12PipelineState* FixedTwin(Entry& e,const std::vector<uint8_t>& container,const FixedGroup& g) {
    ID3D12PipelineState* twin=e.fixedTwin.load(std::memory_order_acquire);
    if(twin || e.fixedDone.load(std::memory_order_acquire))return twin;
    std::lock_guard lock(e.mutex);
    if(e.fixedDone.load(std::memory_order_relaxed))return e.fixedTwin.load(std::memory_order_acquire);
    twin=e.create({container.data(),container.size()});
    (twin?*g.built:*g.failed).fetch_add(1,std::memory_order_relaxed);
    e.fixedTwin.store(twin,std::memory_order_release);
    e.fixedDone.store(1,std::memory_order_release);
    return twin;
}
}

bool HasPatch(const D3D12_SHADER_BYTECODE& code){return PatchFor(code)!=nullptr;}
D3D12_SHADER_BYTECODE LibraryTwin(const D3D12_SHADER_BYTECODE& code) {
    const Patch* patch=PatchFor(code);
    if(!patch || !patch->fixed || patch->group!=kStereoRtGroup || !CyberpunkVR_StereoRtPatch.load(std::memory_order_relaxed))return {};
    return {patch->container.data(),patch->container.size()};
}
void LibraryTwinCreated(bool ok,uint32_t libraries) {
    if(ok)CyberpunkVR_StereoRtPatchBinds.fetch_add(1,std::memory_order_relaxed);
    (ok?CyberpunkVR_StereoRtPatchBuilt:CyberpunkVR_StereoRtPatchFailed).fetch_add(libraries,std::memory_order_relaxed);
}
D3D12_SHADER_BYTECODE FixedVertexTwin(const D3D12_SHADER_BYTECODE& code) {
    const Patch* patch=PatchFor(code);
    if(!patch || !patch->fixed || !kFixedGroups[patch->group].enabled->load(std::memory_order_relaxed))return {};
    return {patch->container.data(),patch->container.size()};
}
void Register(ID3D12PipelineState* original,const D3D12_SHADER_BYTECODE& code,Creator create) {
    const Patch* patch=PatchFor(code);
    if(!original || !patch || !create)return;
    Microsoft::WRL::ComPtr<Entry> entry;entry.Attach(new Entry);
    entry->patch=patch;entry->create=std::move(create);
    {
        std::lock_guard lock(registrationMutex);
        // A driver may return an existing immutable PSO again. Keep its current twins alive instead
        // of replacing their owner while previously recorded commands can still use them.
        if(Find(original))return;
        if(FAILED(original->SetPrivateDataInterface(entryKey,entry.Get())))return;
        CyberpunkVR_HiddenAreaPatchRegistered.store(registered.fetch_add(1,std::memory_order_relaxed)+1,std::memory_order_relaxed);
    }
    if(patch->fixed && kFixedGroups[patch->group].eager && kFixedGroups[patch->group].enabled->load(std::memory_order_relaxed)) {
        FixedTwin(*entry.Get(),patch->container,kFixedGroups[patch->group]);
        entry->create=nullptr;   // done for good: drop the owned description
    }
}
void Publish(int side,const float scales[4]) {
    if(side<0 || side>1 || !scales)return;
    Params params;
    for(int i=0;i<4;++i) {
        const float s=scales[i]>=0.0f && scales[i]<7.99f?scales[i]:7.99f;   // a larger ellipse skips less
        params.p[i]=1.0f+s/8.0f;
    }
    std::lock_guard lock(publishMutex);
    if(!(params==pending[side])){pending[side]=params;stable[side]=0;return;}
    if(++stable[side]<StableFrames || (publishedGeneration[side].load(std::memory_order_relaxed) && published[side]==params))return;
    published[side]=params;
    publishedGeneration[side].store(generations.fetch_add(1,std::memory_order_relaxed)+1,std::memory_order_release);
}
namespace {
// One twin for every view: the shader reads the view's own shift.
ID3D12PipelineState* BindFixed(Entry& e,const std::vector<uint8_t>& container,int group,ID3D12PipelineState* pso) {
    const FixedGroup& g=kFixedGroups[group];
    if(!g.enabled->load(std::memory_order_relaxed))return pso;
    if((group>=10 && group<=14) && !cvr::engine_stereo::Enabled())return pso;
    ID3D12PipelineState* twin=FixedTwin(e,container,g);
    if(!twin)return pso;
    g.binds->fetch_add(1,std::memory_order_relaxed);
    return twin;
}
ID3D12PipelineState* Fallback(Entry* e,ID3D12PipelineState* pso) {
    return e && !e->patch->fallback.empty()?BindFixed(*e,e->patch->fallback,e->patch->fallbackGroup,pso):pso;
}
}

ID3D12PipelineState* Bind(ID3D12PipelineState* pso,int side) {
    if(!pso || !registered.load(std::memory_order_relaxed))return pso;
    const auto owner=Find(pso);
    Entry* e=owner.Get();
    if(e && e->patch->fixed)return BindFixed(*e,e->patch->container,e->patch->group,pso);
    if(!CyberpunkVR_HiddenAreaPatch.load(std::memory_order_relaxed))return Fallback(e,pso);
    if(!e || side<0 || side>1 || !CyberpunkVR_HiddenAreaEnabled.load(std::memory_order_relaxed) || GetMenuMode() ||
       cvr::engine_stereo::Enabled())return Fallback(e,pso);
    const uint64_t generation=publishedGeneration[side].load(std::memory_order_acquire);
    if(!generation)return Fallback(e,pso);
    ID3D12PipelineState* twin;
    if(e->generation[side].load(std::memory_order_acquire)==generation)twin=e->twin[side].load(std::memory_order_acquire);
    else {
        Params params;uint64_t current;
        {std::lock_guard lock(publishMutex);params=published[side];current=publishedGeneration[side].load(std::memory_order_relaxed);}
        std::lock_guard lock(e->mutex);
        twin=Build(*e,params);
        e->twin[side].store(twin,std::memory_order_release);
        e->generation[side].store(current,std::memory_order_release);
    }
    if(!twin)return Fallback(e,pso);
    CyberpunkVR_HiddenAreaPatchBinds.fetch_add(1,std::memory_order_relaxed);
    return twin;
}

void PipelineCreated(ID3D12PipelineState* pso,const D3D12_SHADER_BYTECODE& cs,const D3D12_SHADER_BYTECODE& ps,
                     const D3D12_SHADER_BYTECODE& vs) {
    if(!pso || !Recording())return;
    const auto keep=[pso](const D3D12_SHADER_BYTECODE& code,std::unordered_map<const ID3D12PipelineState*,Shader>& into) {
        if(!code.pShaderBytecode || !code.BytecodeLength)return;
        const uint64_t hash=Hash(code.pShaderBytecode,code.BytecodeLength);
        std::lock_guard lock(mutex);
        auto& blob=blobs[hash];
        if(!blob){
            const auto* bytes=static_cast<const uint8_t*>(code.pShaderBytecode);
            blob=std::make_shared<const std::vector<uint8_t>>(bytes,bytes+code.BytecodeLength);
        }
        into[pso]={hash,blob};
    };
    keep(cs.pShaderBytecode && cs.BytecodeLength?cs:ps,pipelines);
    keep(vs,vertexShaders);
    CyberpunkVR_ShaderCensusPipelines.fetch_add(1,std::memory_order_relaxed);
}

void Command(uint32_t node,int side,ID3D12PipelineState* pso,Kind kind,uint32_t x,uint32_t y,uint32_t z) {
    const uint32_t mode=CyberpunkVR_ShaderCensus.load(std::memory_order_relaxed);
    if(!mode || !Recording())return;
    const bool draw=kind==Kind::Draw || kind==Kind::DrawIndexed;
    // Mode 1: full-screen draws and dispatches -- material draws are not screen-space passes.
    // Mode 2: instanced draws only, by vertex shader (particles and the like).
    // Mode 3: every draw by vertex shader, one row per node and shader (counts dropped so it fits).
    // Mode 4: every draw by pixel shader, one row per node and shader (material shaders).
    if(mode==1 && (kind==Kind::DrawIndexed || (kind==Kind::Draw && !(x<=4 && y==1))))return;
    if(mode==2 && !(draw && y>1))return;
    if(mode>=3 && !draw)return;
    if(mode>=3){x=y=z=0;side=-9;}
    Shader shader{};
    {
        std::lock_guard lock(mutex);
        const auto& from=mode==2 || mode==3?vertexShaders:pipelines;
        const auto it=from.find(pso);
        if(it!=from.end())shader=it->second;
        if(shader.bytes && written.insert(shader.hash).second)Write(shader.hash,*shader.bytes);
    }
    const uint64_t key=mode>=3?0:reinterpret_cast<uint64_t>(pso);
    std::lock_guard lock(logMutex);
    for(auto& r:CyberpunkVR_ShaderCensusLog) {
        if(r.count && r.node==node && r.side==side && r.kind==uint32_t(kind) && r.hash==shader.hash && r.x==x && r.y==y && r.z==z &&
           r.pso==key){++r.count;return;}
        if(!r.count){r={node,side,uint32_t(kind),x,y,z,shader.hash,1,shader.bytes?uint32_t(shader.bytes->size()):0,key};return;}
    }
}
}
