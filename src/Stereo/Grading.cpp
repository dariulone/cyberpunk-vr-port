#include "Utils/DebugGate.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/SinglePassTrace.hpp"
#include "Render/NativePreparation.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Render/NativeGeometryPackets.hpp"
#include "Render/StereoGpuProbe.hpp"
#include "Render/StereoCameraInput.hpp"
#include "Render/ViewCameraLayout.hpp"
#include "Render/StereoShaderCamera.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeMaterialBinding.hpp"
#include "Render/NativeSourceCbvBinding.hpp"
#include "Render/NativeSourceSrvBinding.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/NativeEarlyDepth.hpp"
#include "Render/NativePairedGeometry.hpp"
#include "Render/StageReplay.hpp"
// Grading -- colour grading and tonemapping, for two views instead of one.
//
// The constant-buffer upload, the grading compose and the tonemapping LUT generation. The engine
// builds these once per frame for the view it is drawing; with a second eye the question is whether
// it may reuse the first view's answer or must be given its own, and the visible cost of getting it
// wrong is the two eyes not matching in colour -- which the brain notices long before it notices a
// geometry error.

#include "Stereo/SyncStereo.hpp"
#include "Stereo/CascadeSampling.hpp"
#include "Render/MonoCascades.hpp"
#include "Stereo/FogHistory.hpp"
#include "Render/SsrWideMerge.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Stereo/WorldMarkers.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <windows.h>
#include <d3d12.h>
#include <d3d11.h>
#include <d3d11on12.h>
#include <dxgi1_4.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>
#include "MinHook.h"
#include "Utils/LogThrottle.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Lobby/MenuLobby.hpp"

namespace cvr {
namespace detail {
extern "C" {
__declspec(dllexport) int32_t CyberpunkVR_OpticsDataProbe=0;
__declspec(dllexport) uint8_t CyberpunkVR_OpticsViewData[3][4096]{};
__declspec(dllexport) uint8_t CyberpunkVR_OpticsGradeData[3][688]{};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsDataSeq[3]{};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsGradeSeq[3]{};
__declspec(dllexport) int32_t CyberpunkVR_OpticsGradeMirror=1;
__declspec(dllexport) int32_t CyberpunkVR_OpticsMaterialProbe=0;
__declspec(dllexport) float CyberpunkVR_OpticsMaterialData[3][84]{};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsMaterialSize[3]{};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsMaterialSeq[3]{};
__declspec(dllexport) float CyberpunkVR_OpticsShadowData[3][232]{};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsShadowSeq[3]{};
}
static int optics_data_side();
static void optics_shadow_snapshot(uint32_t size,const void* source) {
    if(!CyberpunkVR_OpticsDataProbe || !source || size!=928 ||
       t_current_node_work!=reinterpret_cast<uintptr_t>(g_exe_base)+PREPARE_SCENE_NODE_WORK_RVA)return;
    const int side=optics_data_side();if(side<0)return;
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsShadowSeq[side]));
    cloud_cb_raw_copy(CyberpunkVR_OpticsShadowData[side],source,size);
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsShadowSeq[side]));
}
void optics_material_snapshot(uint32_t size,const void* source) {
    if(!CyberpunkVR_OpticsMaterialProbe || !source || size<100 || size>1024)return;
    float marker{};
    if(!cloud_cb_raw_copy(&marker,static_cast<const uint8_t*>(source)+96,4))return;
    const int kind=marker==-713.25f?0:marker==-714.25f?1:marker==-715.25f?2:-1;
    if(kind<0)return;
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsMaterialSeq[kind]));
    cloud_cb_raw_copy(CyberpunkVR_OpticsMaterialData[kind],source,std::min(size,336u));
    CyberpunkVR_OpticsMaterialSize[kind]=size;
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsMaterialSeq[kind]));
}
static int optics_data_side() {
    if(!t_active_view_known)return -1;
    return t_active_view_key==0?0:t_active_view_key==g_vrcam_ctx_key?1:
        t_active_view_key==cvr::cname_hash("vr_optics_feed")?2:-1;
}
static void optics_grade_snapshot(void* src) {
    if(!CyberpunkVR_OpticsDataProbe)return;
    const int side=optics_data_side();if(side<0)return;
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsGradeSeq[side]));
    cloud_cb_raw_copy(CyberpunkVR_OpticsGradeData[side],src,688);
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsGradeSeq[side]));
}

// ================================================================================================
// WHY CLOTH FLAGS STAND STILL IN THE SECOND EYE -- the second view's clock is zero.
//
// Reported 2026-08-17: the fabric flags on buildings wave in MAIN and hang dead in the second eye.
//
// THERE IS NO CLOTH SIMULATION IN THEM. The flags' pipeline state was identified from an Nsight
// capture (PipelineState_55927: one draw per view in GBuffer_Discard, 11847 indices, the SAME vertex
// and index buffers for both views), and its VERTEX shader does the waving itself:
//
//     phase  = frac(((b0[0].z * b4[2].x) / b4[1].z) + ... + sin(b0[0].z * ...))   // b0[0].z is TIME
//     wind   = sqrt(b0[9].x^2 + b0[9].y^2), normalised, dotted with INSTANCE_TRANSFORM
//     offset = sway texture sampled at (row, phase), scaled by b4[4].x * windResponse
//     pos    = POSITION * b5[4] + b5[5] + offset
//
// so the motion is a function of two things only: the TIME in the frame-constants block and the wind
// vector beside it. Nothing is skinned, nothing is simulated, and every search for a lost deformation
// was looking for data that does not exist -- which is why the instance-skinning buffer, the
// skinning/tangent node (frozen outright, its dispatch counter flat at 19940), the render mask (the
// second view has all 27 categories), LOD substitution (identical draw shapes and buffers) and the
// foliage-wind claim all came back clean.
//
// THE MEASUREMENT. Both views upload that 480-byte block once per frame from the same node, so the
// two can be compared field by field. Of its 120 floats, exactly three advance for MAIN and never
// change for the second view, whose copies are all zero -- and the control direction, fields that move
// only for the second view, was EMPTY:
//
//     [0].z  M=133150.22  V=0     the clock in seconds; THE field the cloth VS takes its phase from
//     [1].z  M=133150.19  V=0     the same clock as of the previous frame (~31 ms behind)
//     [1].w  M=0.1850     V=0     a slow clock, ~1e-4 per second: the day fraction
//
// The wind vector is identical in both views, so it was never the problem. Time in the second eye
// stands at zero for EVERYTHING, not just for cloth; the flags are simply the most visible thing that
// is a function of it.
//
// (Two more floats are zero on the same side, [9].z = 172.16 and [9].w = 728.93 for MAIN, but they
// move too slowly to have earned the name "clock" -- so they are in the mask and off by default.)
//
// THE FIX lends MAIN's clock to the second view's block, into a COPY and never into the engine's
// buffer: sub_140334328 allocates, memcpys from the caller's pointer and commits (read in the
// debugger before writing anything), so a temporary of ours is read once and engine state is not
// touched at all. Taking MAIN's latest value is at worst one frame of phase -- 31 ms on a flag.
// Verified live: the fill counter advances once per frame, and the flags were confirmed to wave in the headset.
//
// TWO INSTRUMENTS PAID FOR THIS and are gone, both too expensive to ship: a per-(node, size, view)
// census of 400/480-byte uploads, which is what named the producing node instead of guessing it, and a
// draw-shape census over every DrawIndexedInstanced. Each took a mutex on a hot path. Their two
// lessons are worth more than the code:
//
//   * a probe keyed only by SIZE is a mixture -- 480 bytes is a common block, and the first run put
//     three different render targets' dimensions in one column and called it a clock;
//   * "frozen" means nothing unless the view is uploading at all. The first run reported the second
//     view's fields as frozen when its bucket had simply stopped growing at 694 samples.
// MODE 2, added 2026-08-17 for the shadow mismatch, and it supersedes mode 1 rather than extending it.
//
// The complete diff (above) found the frame counter at [28].y differing by exactly ONE between the
// views -- MAIN 12009 against VRCAM 12010, then MAIN 12425 against VRCAM 12424 -- and the shaders take
// their dither slice from `asuint(b0[28].y) & 63`, so 41 against 42 and 9 against 8 are DIFFERENT
// slices of blue noise. Alpha-tested geometry (fences, foliage, tree crowns) therefore discards a
// different set of pixels in each eye, in the shadow cascade as much as in the GBuffer. That is the
// reported defect exactly: a piece of shadow present in one eye and missing in the other, in different
// places, flickering per pixel.
//
// NOTE WHICH WAY THE ONE GOES: sometimes the second view is ahead, sometimes MAIN is. The counter
// advances between the two views' uploads, and which of them uploads first varies frame to frame. That
// is why mode 1 -- lend MAIN's latest value to the second view -- CANNOT fix this: in the frames where
// the second view uploads first it would receive the previous frame's value and still differ by one.
// The same objection applies to the clock mode 1 was written for.
//
// So mode 2 holds ONE value per frame and gives it to BOTH views, keyed on the renderer's frame id:
// the first upload of a frame installs the values staged from MAIN's previous authored block, and every
// upload in that frame -- MAIN's own included -- ships those. Order-independent by construction, and
// the two eyes are then bit-identical in these fields. The cost is that everything driven by them is
// one frame late, uniformly: 31 ms of sway phase and one slice of noise, neither of which is visible.
//
// MODE 3, added after modes 1 and 2 both failed to change the shadows -- and added because BOTH of them
// depend on WHICH VIEW UPLOADS FIRST, so neither can prove anything about the dither hypothesis. Mode 2
// keys on the renderer frame id, and the shipped values showed that id advancing between the two views'
// uploads (MAIN shipped 11787 where it authored 11789; the second view shipped 11789) -- the engine
// counts each VIEW as a frame, so that key does not group a stereo pair at all. Mode 1 does not use the
// frame id but hands over "MAIN's latest", which is the same value only if the second view uploads
// after MAIN.
//
// So mode 3 removes the ordering question instead of arguing about it: the dither slice is forced to
// ONE CONSTANT in both views. Equality then cannot fail for a mechanical reason, which is the only way
// a negative result becomes evidence. If the eyes still differ under mode 3, the dithered alpha test in
// this block is NOT what makes them differ, and the hypothesis is dead rather than unproven.
//
// The cost while testing is a static noise pattern instead of a temporally varying one (the shaders
// index a 64-slice blue-noise array with counter & 63, and a fixed slice means TAA has nothing to
// average). That is a diagnostic setting, not a shipping one.
//
//   mode 0  off
//   mode 1  lend MAIN's latest to the second view only (the clock fix as first shipped)
//   mode 2  one value per frame for both views, keyed on the renderer frame id -- which does NOT
//           group a stereo pair, so this is mode 1 with extra steps
//   mode 3  mode 1, plus the dither slice forced to CyberpunkVR_SwayDitherConst in both views
//   mode 4  the second view gets MAIN's WHOLE block, verbatim
//   mode 5  mode 4, plus the dither slice forced to a constant in EVERY frame-constants upload,
//           whatever node made it  (default)
//
// Mode 5 exists because mode 3 could not settle the question it was built for. The cascade pixel shader
// dithers its alpha test from asuint(b0[28].y) & 63, and modes 1-3 all patch the 480-byte block uploaded
// under PrepareSceneRendering -- but WHICH INSTANCE of b0 the cascade pass binds is not known, and the
// cascade node itself uploads no 480-byte block at all (measured: zero rows). So those modes may simply
// never have reached the shader, which makes their negative results worthless.
//
// Mode 5 drops the node key for the dither pair alone: every 480-byte upload gets the same forced slice,
// in both views, so no instance can be missed. The node filter is replaced by a CONTENT filter -- the
// block must have a large clock at [0].z -- because 480 bytes is a common size and one of the other
// blocks of that size was measured carrying render-target dimensions, which must not be overwritten.
// If the eyes still differ under mode 5, the blue-noise dither is not the cause and the hypothesis is
// finally dead rather than untested.
//
// Mode 4 is the blunt form of the same doctrine, and it is blunt on purpose: no frame key, no field
// mask, nothing to pick wrong. It is bounded by the measurement rather than by hope -- of the 120
// floats only ten differ between the views at all, so copying the block wholesale changes exactly those
// ten. Two of them are the camera position and direction ([5].xyz and [6].xyz, apart by the IPD), so
// the second eye shades from MAIN's camera; at 1.3 cm that is not something an eye can find, and if it
// ever is, the masked modes above are still there.
// Back to 4: mode 5 forced the dither slice to a constant everywhere, which was a diagnostic and is
// measured innocent -- and a constant slice means static noise, which is worse than the defect it was
// testing for. Mode 4 keeps the clocks mirrored, which is what the cloth flags need.
// MODE 1, narrowed back to what was actually confirmed working on the cloth flags: MAIN's clock lent
// to the second view. Mode 4 copies the whole block, camera position included, and was never verified
// as necessary -- shipping the broader change on the strength of an unrelated hunt would be guessing.
// Mode 4 on request: the second view gets MAIN's whole frame-constants block. Worth
// knowing which lever is which -- the cable's shadow stopped differing in the same build where this
// went the other way, from 4 to 1 with the mask narrowed, so if it comes back this is the knob.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_SwayTimeFix = 4;
// Which fields to hold, as a bitmask over kSwayTimeFields. Default: the three clocks (bits 0-2), the
// dither pair (bits 5-6) and the second clock pair (bits 7-8). The two slow accumulators (3-4) and
// [0].w (9) are measured to differ as well but are left out until something asks for them.
// Live-settable, so widening or narrowing it costs no rebuild.
// 0x07: the three clocks, which is the set the flags were fixed with. The dither pair (0x60) is
// measured not to matter for the shadows, and the second clock pair (0x180) was added on the same
// unproven hunch -- both remain available.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_SwayTimeMask = 0x07;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSwayTimeFills = 0;
// Mode 3 only: the frame counter both views are given. Any value works -- only its low six bits are
// read, as a slice index into a static noise array -- and being a constant is the whole point.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_SwayDitherConst = 12345;

// ---- THE COMPLETE DIFF, because the first probe had a blind spot -------------------------------
//
// Shadows cast by fences and foliage differ between the eyes: a piece of shadow is in one and not the
// other, in different places, and it flickers per pixel. Two things are already measured: each view
// rasterises its OWN cascade (cutting the node for the second view left that eye with no sun shadows
// at all), and the two views draw the SAME casters into it -- live, at equal resolutions, 94002 draws
// against 94004 and 267828 instances against 267831 in one interval, the residue being one frame's
// worth on the interval boundary. So the same geometry, drawn with shared cascade records, ends up as
// different pixels; and alpha-tested geometry becomes pixels through a DITHERED discard, whose noise
// slice the pixel shader picks with asuint(b0[28].y) & 63 -- a frame counter.
//
// THE FIRST PROBE COULD NOT HAVE SEEN THAT. It printed fields that advance for MAIN and are frozen for
// the second view, and fields that are static but differ. A field that ADVANCES IN BOTH VIEWS WITH
// DIFFERENT VALUES falls in neither list -- and a frame counter one step apart is exactly that. So
// this reports every float whose current value differs at all, with what each side is doing to it,
// and the dither index as an integer beside it. No categories to fall between.
// DEFAULT 0: a mutex and a 120-float compare on every frame-constants upload. It found the frozen
// clock and the dither difference; neither needs watching now.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SwayDiff = 0;

// ---- THE GENERIC BLOCK DIFF, retargetable live --------------------------------------------------
//
// Built after the cascade shaders were read, because they showed that the block I had been holding was
// very likely the wrong INSTANCE of the right block. The cascade pixel shader dithers its alpha test
// from `asuint(b0[28].y) & 63` -- b0 being a 30-float4 block, so the 480 bytes I hold -- but the patch
// is keyed to uploads under PrepareSceneRendering. If the cascade pass uploads its own b0 under its own
// node, nothing I did ever reached it, which is exactly what "three modes, no visible change" looks
// like.
//
// And the same shader has a SECOND discard whose inputs are worse: a height-range mask addressed in a
// world grid whose origin is floor(b1[36].xyz * 21.3333) * 0.046875. The grid step is 0.046875 world
// units = 4.7 cm and the eyes are 6.5 cm apart, so 0.065 / 0.046875 = 1.39 steps: the two views land in
// DIFFERENT cells as a matter of arithmetic, not of chance, and the mask is then read at an offset in
// one eye. b1 is 53 float4 = 848 bytes.
//
// So the instrument is parameterised instead of hard-wired: point it at a (node, size) pair and it
// reports every float that differs between the views, with what each side is doing to it. Both knobs
// are live-settable, which is the difference between one build and five.
extern "C" __declspec(dllexport) const char* CyberpunkVR_ProfNodeName(uint32_t rva);

// ================================================================================================
// THE CHECKERBOARD PHASE -- why thin shadows differ between the eyes.
//
// The sun-shadow mask is evaluated at HALF resolution on a checkerboard, and the shader that does it
// (PipelineState_858, one draw per view at 1280 and 1536 -- half of 2560 and half of 3072) opens with:
//
//     x  = uint(gl_FragCoord.x);  y = uint(gl_FragCoord.y);
//     px = (x << 1) | ((asuint(cb_b6[3]).y ^ y) & 1);      // the full-res pixel this lane stands for
//
// so cb_b6[3].y decides WHICH half of the pattern this lane belongs to; the other half is reconstructed.
//
// THE PHASE IS NOT THE PROBLEM -- measured, whole-block, in gameplay: the 56-byte block is IDENTICAL for
// the two views, zero differing dwords, so both eyes use the same parity function. Forcing it patched 1008
// times and changed nothing on screen, which is what a no-op looks like. The first "the counter differs by
// one, so the parity is opposite" reading was inference, not measurement, and it was wrong.
//
// WHAT REMAINS IS THE PATTERN ITSELF, AND IT IS STRUCTURAL. `parity` is a function of the SCREEN x and y
// of the eye being drawn. One world point projects to different screen x in each eye, so it falls in the
// evaluated half in one eye and in the reconstructed half in the other; and because px = (x << 1) | parity,
// the sample it does get is a full-res pixel to the left in one eye and to the right in the other. That is
// a one-pixel horizontal disagreement, worst where a shadow is one or two pixels wide and worse with
// distance -- which is the reported symptom (shadows simply shifted... a little
// farther out the thin part of a shadow differs between the eyes), and why grass and cables differ while a building's
// shadow does not.
//
// NO CONSTANT CAN EQUALISE THAT. Half of the full-res pixels are never evaluated by either eye, and which
// world points land in the evaluated half is decided by each eye's own projection. Equalising the phase,
// the seed, the wind, the cascade record or the frame clock cannot change which pixels get sampled -- which
// is exactly why every one of those was measured identical and none of them helped. The only true fix is
// full-resolution evaluation, i.e. a full-width mask target and a pixel shader without the `x << 1`, and
// the engine offers no switch for it: the interleave is baked into this pass (its feature bits answer
// identically for both views, see CyberpunkVR_FeatBitProbe), "CheckerboardSize" turns out to be an editor
// selection-highlight setting, and the RTTI class that owns an "interleaved" property carries
// enabled/radius/offset/angle/events beside it and has nothing to do with shadows.
//
// So this stays a probe, at 0, and the write stays disarmed. Better a named structural limit than a knob
// that pretends to fix it.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CheckerPhaseFix = 0;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CheckerProbe = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_CheckerSize = 56;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCheckerFixes = 0;

namespace {
constexpr uint32_t kCheckerMax = 64;
uint8_t g_checker[kCheckerMax] = {};      // the first view's copy
uint8_t g_checker_m[kCheckerMax] = {};   // ...and the second's, so the two can be diffed whole
bool    g_checker_have = false;
uint32_t g_checker_main = 0, g_checker_vrcam = 0;
uint64_t g_checker_n[2] = {0, 0};
std::mutex g_checker_mtx;

bool checker_copy(const void* src, void* dst, uint32_t n) {
    __try { memcpy(dst, src, n); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

// The 56-byte block does not pass the BUFFER uploader -- the probe aimed there printed nothing at all.
// So the small sizes are enumerated instead of guessed: every upload under 128 bytes, per view, through
// whichever uploader it arrives on. b6 is 56 bytes by the shader's own layout (48 + 4 + 4), and if the
// engine pads or batches it the real number will be in this list.
// KEYED BY NODE AS WELL AS SIZE, because the sizes alone raised the real question and could not answer
// it. The plain size census found rows where the two views do different AMOUNTS of work rather than the
// same work with different numbers -- 112 bytes uploaded 731 times for MAIN and never for the second
// view, 48 bytes 21388 against 1743, 96 bytes 4270 against 580. An imbalance like that is a pass one eye
// runs and the other does not, which is a fixable defect, unlike the checkerboard. Naming the pass is the
// whole point; the size on its own names nothing.
struct SmallSize { uint32_t size; uint32_t node; uint64_t hits[2]; };
SmallSize g_small[96];
uint32_t g_small_n = 0;

void checker_small_note(uint32_t size, bool vrcam) {
    if (size == 0 || size >= 128) return;
    uint32_t node = 0;
    if (g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        if (t_current_node_work > base)
            node = static_cast<uint32_t>(t_current_node_work - base);
    }
    std::lock_guard<std::mutex> lk(g_checker_mtx);
    uint32_t i = 0;
    for (; i < g_small_n; ++i)
        if (g_small[i].size == size && g_small[i].node == node) break;
    if (i == g_small_n) {
        if (g_small_n >= 96) return;
        g_small[g_small_n++] = SmallSize{size, node, {0, 0}};
    }
    ++g_small[i].hits[vrcam ? 1 : 0];
}

void checker_small_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[700];
    int used = 0;
    line[0] = 0;
    std::lock_guard<std::mutex> lk(g_checker_mtx);
    for (uint32_t i = 0; i < g_small_n; ++i) {
        const SmallSize& s = g_small[i];
        // Only the rows where the imbalance is the finding: one side at least four times the other, or
        // one side at zero. A balanced row is the engine working as intended and would only crowd the log.
        const uint64_t lo = (s.hits[0] < s.hits[1]) ? s.hits[0] : s.hits[1];
        const uint64_t hi = (s.hits[0] < s.hits[1]) ? s.hits[1] : s.hits[0];
        if (hi < 50) continue;
        if (lo * 4 > hi) continue;
        if (used >= static_cast<int>(sizeof(line)) - 60) break;
        const char* nm = CyberpunkVR_ProfNodeName(s.node);
        used += snprintf(line + used, sizeof(line) - used, "%uB@%s M=%llu V=%llu  ", s.size,
                         (nm && *nm) ? nm : (s.node ? "?" : "no-node"),
                         (unsigned long long)s.hits[0], (unsigned long long)s.hits[1]);
    }
    log("[checksz] small uploads whose two views are >=4x apart, by size and node: %s",
        used ? line : "(none -- every small block is balanced)");
}

void checker_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    // EVERY differing dword of the block, not just the field I guessed at. Reading [3].y as the phase
    // came from SPIRV-Cross's struct layout, and the value there turned out to be a float around 0.018
    // whose parity never changes -- so either the phase is elsewhere in this block or this is not the
    // block. A full diff answers both without another guess.
    char line[600];
    int used = 0, n = 0;
    line[0] = 0;
    const uint32_t sz = (CyberpunkVR_CheckerSize < kCheckerMax) ? CyberpunkVR_CheckerSize
                                                                : kCheckerMax;
    for (uint32_t o = 0; o + 4 <= sz; o += 4) {
        if (memcmp(g_checker + o, g_checker_m + o, 4) == 0) continue;
        ++n;
        uint32_t a = 0, b = 0;
        float fa = 0.0f, fb = 0.0f;
        memcpy(&a, g_checker_m + o, 4);
        memcpy(&b, g_checker + o, 4);
        memcpy(&fa, g_checker_m + o, 4);
        memcpy(&fb, g_checker + o, 4);
        if (used < static_cast<int>(sizeof(line)) - 70)
            used += snprintf(line + used, sizeof(line) - used,
                             "+%u M=%u/%.4f V=%u/%.4f  ", o, a, fa, b, fb);
    }
    log("[checker] %u-byte block: MAIN n=%llu VRCAM n=%llu | [3].y M=%u V=%u | differing dwords "
        "(%d, as uint/float): %s | fixes=%llu",
        CyberpunkVR_CheckerSize, (unsigned long long)g_checker_n[0],
        (unsigned long long)g_checker_n[1], g_checker_main, g_checker_vrcam,
        n, n ? line : "(none -- the two views' copies are identical)",
        (unsigned long long)CyberpunkVR_DebugCheckerFixes);
}

// Returns a patched buffer for the second view, or false to ship the engine's own.
bool checker_note(const void* src, void* dst, bool vrcam, uint32_t size) {
    uint8_t tmp[kCheckerMax];
    const uint32_t n = (size < kCheckerMax) ? size : kCheckerMax;
    if (!checker_copy(src, tmp, n)) return false;
    uint32_t phase = 0;
    if (n >= 56) memcpy(&phase, tmp + 3 * 16 + 4, sizeof(phase));   // [3].y
    bool patched = false;
    {
        std::lock_guard<std::mutex> lk(g_checker_mtx);
        if (vrcam) {
            g_checker_vrcam = phase;
            ++g_checker_n[1];
            memcpy(g_checker, tmp, n);
            g_checker_have = true;
        } else {
            g_checker_main = phase;
            ++g_checker_n[0];
            memcpy(g_checker_m, tmp, n);
            if (CyberpunkVR_CheckerPhaseFix && g_checker_have && n >= 56) {
                memcpy(dst, tmp, n);
                memcpy(reinterpret_cast<uint8_t*>(dst) + 3 * 16 + 4, g_checker + 3 * 16 + 4,
                       sizeof(phase));
                patched = true;
            }
        }
    }
    if (patched)
        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCheckerFixes)));
    if (CyberpunkVR_CheckerProbe) checker_report();
    return patched;
}
}  // namespace
// ON, aimed at the SAMPLING side of the shadow cascades.
//
// Where this stands: the two views were measured to fit the sun cascade 9-12 cm (casc0) and 27 cm (casc1)
// apart, forcing MAIN onto the other view's fit brings that separation to exactly 0, and the sun shafts
// improved -- but they are still displaced. So the lend reaches what RASTERISES the atlas and not what
// SAMPLES it. The rasteriser's matrices come from the cascade record; the sampler's come from somewhere else,
// and only two functions in the whole exe address that record by its offset (the pass itself and
// sub_140E9BBC4, which is the bit-44 variant of the shadow-mask pass and is measured NOT to run -- bit 44
// answers no for both views). So the sampler reads it through a register, and the way to find that is to
// diff what the lighting node uploads rather than to keep reading disassembly.
//
// BindLightingGlobalConstants (0xC45C70) is the named node that carries the lighting globals, which is where
// cascade sampling matrices belong. Pointed there, the size census says what it uploads per view and the
// field diff says which of it differs.
// DEFAULT 0 AND LIVE, like every other measurement here: `xr_block_diff`, with
// `xr_block_diff_node` (HEX work-RVA) and `xr_block_diff_size` aiming it. It shipped at 1.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_BlockDiff = 0;
// Node 0 = the other direction: which NODES upload a block of BlockDiffSize, per view. Aimed at 416
// bytes, because that is both b8 (26 float4, the wind parameters the cascade vertex shader sways foliage
// with) and the exact size AdvanceSpeedTreeWind uploads -- and that pass is behind the once-per-frame
// latch, so one view per frame produces it.
// Aimed at the volumetric fog: the wide census reports 384B@VolumetricFog as a block BOTH views
// upload and whose contents differ, one per view per frame, so the (node, size) key is fine enough
// -- unlike 848B@RenderShadowCascade in the same list, which mixes cascade 0 and cascade 1 and is
// therefore guaranteed to read as "differs" whatever the truth is.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_BlockDiffNode = VOLUMETRIC_FOG_NODE_RVA;
// 848 = 53 float4 = b1, which the census confirms the cascade node uploads for both views (4156 against
// 4602 in one interval) and which the shader's height-mask grid origin [36].xyz lives in. The 448-byte
// rows in the same census are b7, 28 float4, where that shader's dither threshold comes from.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_BlockDiffSize = 384;

namespace {
constexpr uint32_t kSwayDiffFloats = 120;        // 480 bytes = 30 float4
struct SwayDiffBlk {
    float    last[kSwayDiffFloats];
    uint32_t moved[kSwayDiffFloats];
    uint64_t seen;
};
SwayDiffBlk g_swaydiff[2] = {};
std::mutex g_swaydiff_mtx;

bool sway_diff_copy(const void* src, float* out) {
    __try { memcpy(out, src, kSwayDiffFloats * sizeof(float)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

void sway_diff_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[1500];
    int used = 0, n = 0;
    std::lock_guard<std::mutex> lk(g_swaydiff_mtx);
    if (g_swaydiff[0].seen < 5 || g_swaydiff[1].seen < 5) return;
    line[0] = 0;
    for (uint32_t i = 0; i < kSwayDiffFloats; ++i) {
        if (memcmp(&g_swaydiff[0].last[i], &g_swaydiff[1].last[i], sizeof(float)) == 0) continue;
        ++n;
        const bool mm = g_swaydiff[0].moved[i] * 4 >= g_swaydiff[0].seen;
        const bool mv = g_swaydiff[1].moved[i] * 4 >= g_swaydiff[1].seen;
        if (used < static_cast<int>(sizeof(line)) - 70)
            used += snprintf(line + used, sizeof(line) - used, "[%u].%c%s%s M=%.4f V=%.4f  ",
                             i / 4, "xyzw"[i % 4], mm ? "+" : "=", mv ? "+" : "=",
                             g_swaydiff[0].last[i], g_swaydiff[1].last[i]);
    }
    uint32_t dm = 0, dv = 0;
    memcpy(&dm, &g_swaydiff[0].last[113], sizeof(dm));   // [28].y, the dither slice index
    memcpy(&dv, &g_swaydiff[1].last[113], sizeof(dv));
    log("[swaydiff] 480B block, EVERY differing float (%d of 120; + = advances for that view, "
        "= = static): %s || dither index [28].y as uint: MAIN %u (&63 = %u) VRCAM %u (&63 = %u)",
        n, n ? line : "(none)", dm, dm & 63u, dv, dv & 63u);
}

// What actually SHIPPED, per view -- the diff above reads the engine's own block, before the patch, so
// on its own it can never say whether the patch took. Without this the next question ("still different:
// did the fix miss, or is the cause elsewhere?") is unanswerable except by guessing, and guessing is
// what this whole hunt has been paying for.
uint32_t g_sway_shipped[2] = {0, 0};
uint64_t g_sway_shipped_n[2] = {0, 0};

void sway_shipped_note(const void* blk, bool vrcam) {
    uint32_t v = 0;
    __try { memcpy(&v, reinterpret_cast<const uint8_t*>(blk) + 113 * 4, sizeof(v)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    const int i = vrcam ? 1 : 0;
    g_sway_shipped[i] = v;
    ++g_sway_shipped_n[i];
}

// THE INTERLEAVING, recorded rather than inferred. The shipped values came out unequal with a gap of
// two, from which I inferred that the two views cross different frame boundaries -- but that was an
// inference from an effect, and the whole hunt has been paying for those. This keeps the last few
// uploads verbatim: which view, the renderer frame id it saw, and the counter it authored. From that
// the grouping is readable directly: whether the views alternate, whether they share a frame id, and
// which of them opens a frame.
struct SwayOrder { uint32_t frame; uint32_t authored; uint8_t view; };
SwayOrder g_sway_order[10] = {};
uint32_t g_sway_order_n = 0;
std::mutex g_sway_order_mtx;

void sway_order_note(bool vrcam, uint32_t frame, uint32_t authored) {
    std::lock_guard<std::mutex> lk(g_sway_order_mtx);
    g_sway_order[g_sway_order_n % 10] = SwayOrder{frame, authored, vrcam ? uint8_t(1) : uint8_t(0)};
    ++g_sway_order_n;
}

void sway_shipped_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char order[420];
    int used = 0;
    order[0] = 0;
    {
        std::lock_guard<std::mutex> lk(g_sway_order_mtx);
        const uint32_t n = g_sway_order_n;
        const uint32_t first = (n > 10) ? n - 10 : 0;
        for (uint32_t i = first; i < n; ++i) {
            const SwayOrder& s = g_sway_order[i % 10];
            if (used < static_cast<int>(sizeof(order)) - 40)
                used += snprintf(order + used, sizeof(order) - used, "%s/f%u/c%u ",
                                 s.view ? "V" : "M", s.frame, s.authored);
        }
    }
    log("[swayship] dither index [28].y as SHIPPED: MAIN %u (&63 = %u, n=%llu) | "
        "VRCAM %u (&63 = %u, n=%llu) | equal=%s || last uploads (view/frame/authored): %s",
        g_sway_shipped[0], g_sway_shipped[0] & 63u, (unsigned long long)g_sway_shipped_n[0],
        g_sway_shipped[1], g_sway_shipped[1] & 63u, (unsigned long long)g_sway_shipped_n[1],
        (g_sway_shipped[0] == g_sway_shipped[1]) ? "YES" : "no", order);
}

// The generic one: any (node, size), full diff, no categories to fall between.
constexpr uint32_t kBlkMaxFloats = 256;          // 1 KB covers b0 (480 B) and b1 (848 B)
struct BlkDiff {
    float    last[kBlkMaxFloats];
    uint32_t moved[kBlkMaxFloats];
    uint64_t seen;
};
BlkDiff g_blk[2] = {};
std::mutex g_blk_mtx;
uint32_t g_blk_node = 0;
uint32_t g_blk_size = 0;

bool blk_copy(const void* src, float* out, uint32_t bytes) {
    __try { memcpy(out, src, bytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

// WHICH BLOCKS THE TARGET NODE UPLOADS AT ALL. The first run of the diff, aimed at (cascade node, 480),
// printed nothing whatever -- which is itself a result: the cascade pass does not upload its own copy of
// b0, it reads the frame-global one. But "nothing" is a poor way to learn that, and guessing the next
// size to try is how the last hour went. So the node's uploads are enumerated by size, and the sizes are
// read off the log instead of proposed.
// It runs BOTH WAYS, because each direction answered a question the other could not. Aimed at (cascade
// node, 480) the diff printed nothing at all -- the cascade pass does not upload its own b0, it reads the
// frame-global one -- and retargeted to 848 it found one upload against zero, so the pass does not
// upload its b1 either. Its constants are therefore produced by some OTHER node, and "which node
// uploads an 848-byte block" is a question about a size, not about a node.
//
//   node set, size 0   -> list the sizes that node uploads
//   node 0, size set   -> list the nodes that upload that size
//
// Both keyed per view, so a block only one eye ever uploads shows up as such -- which is itself a
// finding: an earlier census caught PrepareBlankRendering uploading 8517 blocks for MAIN and none for
// the second view.
struct BlkKey { uint32_t key; uint64_t hits[2]; };
BlkKey g_blk_keys[48];
uint32_t g_blk_keys_n = 0;
bool g_blk_keys_by_node = false;

void blk_key_note(uint32_t key, bool vrcam) {
    std::lock_guard<std::mutex> lk(g_blk_mtx);
    const bool by_node = (CyberpunkVR_BlockDiffNode == 0);
    if (by_node != g_blk_keys_by_node) {         // direction changed live: start clean
        g_blk_keys_by_node = by_node;
        g_blk_keys_n = 0;
        memset(g_blk_keys, 0, sizeof(g_blk_keys));
    }
    uint32_t i = 0;
    for (; i < g_blk_keys_n; ++i) if (g_blk_keys[i].key == key) break;
    if (i == g_blk_keys_n) {
        if (g_blk_keys_n >= 48) return;
        g_blk_keys[g_blk_keys_n++] = BlkKey{key, {0, 0}};
    }
    ++g_blk_keys[i].hits[vrcam ? 1 : 0];
}

void blk_keys_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[1300];
    int used = 0;
    line[0] = 0;
    const bool by_node = g_blk_keys_by_node;
    std::lock_guard<std::mutex> lk(g_blk_mtx);
    for (uint32_t i = 0; i < g_blk_keys_n; ++i) {
        const BlkKey& s = g_blk_keys[i];
        if (s.hits[0] + s.hits[1] < 20) continue;      // transients would bury the per-frame ones
        if (used >= static_cast<int>(sizeof(line)) - 70) break;
        if (by_node) {
            const char* nm = CyberpunkVR_ProfNodeName(s.key);
            used += snprintf(line + used, sizeof(line) - used, "%s M=%llu V=%llu  ",
                             (nm && *nm) ? nm : (s.key ? "?" : "no-node"),
                             (unsigned long long)s.hits[0], (unsigned long long)s.hits[1]);
        } else {
            used += snprintf(line + used, sizeof(line) - used, "%uB M=%llu V=%llu  ", s.key,
                             (unsigned long long)s.hits[0], (unsigned long long)s.hits[1]);
        }
    }
    if (by_node)
        log("[blkwho] nodes uploading a %u-byte block, per view: %s",
            CyberpunkVR_BlockDiffSize, used ? line : "(none)");
    else
        log("[blkwho] sizes uploaded under node %X, per view: %s",
            CyberpunkVR_BlockDiffNode, used ? line : "(none -- this node uploads no buffers)");
}

void blk_report(uint32_t nfloat) {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[1500];
    int used = 0, n = 0;
    line[0] = 0;
    std::lock_guard<std::mutex> lk(g_blk_mtx);
    if (g_blk[0].seen < 5 || g_blk[1].seen < 5) {
        log("[blkdiff] node=%X size=%u: MAIN n=%llu VRCAM n=%llu -- one side is not uploading this "
            "block, so there is nothing to compare",
            CyberpunkVR_BlockDiffNode, CyberpunkVR_BlockDiffSize,
            (unsigned long long)g_blk[0].seen, (unsigned long long)g_blk[1].seen);
        return;
    }
    for (uint32_t i = 0; i < nfloat; ++i) {
        if (memcmp(&g_blk[0].last[i], &g_blk[1].last[i], sizeof(float)) == 0) continue;
        ++n;
        const bool mm = g_blk[0].moved[i] * 4 >= g_blk[0].seen;
        const bool mv = g_blk[1].moved[i] * 4 >= g_blk[1].seen;
        if (used < static_cast<int>(sizeof(line)) - 70)
            used += snprintf(line + used, sizeof(line) - used, "[%u].%c%s%s M=%.4f V=%.4f  ",
                             i / 4, "xyzw"[i % 4], mm ? "+" : "=", mv ? "+" : "=",
                             g_blk[0].last[i], g_blk[1].last[i]);
    }
    // The two fields the cascade shader actually decides with, printed whether or not they differ:
    // b0[28].y is the dither slice, b1[36].xyz the grid origin the height mask is addressed by.
    uint32_t dm = 0, dv = 0;
    if (nfloat > 113) {
        memcpy(&dm, &g_blk[0].last[113], sizeof(dm));
        memcpy(&dv, &g_blk[1].last[113], sizeof(dv));
    }
    char grid[220];
    grid[0] = 0;
    if (nfloat > 146)
        snprintf(grid, sizeof(grid),
                 " || [36].xyz M=(%.4f, %.4f, %.4f) V=(%.4f, %.4f, %.4f) snapped M=(%d, %d, %d) "
                 "V=(%d, %d, %d)",
                 g_blk[0].last[144], g_blk[0].last[145], g_blk[0].last[146],
                 g_blk[1].last[144], g_blk[1].last[145], g_blk[1].last[146],
                 (int)floorf(g_blk[0].last[144] * 21.3333333f),
                 (int)floorf(g_blk[0].last[145] * 21.3333333f),
                 (int)floorf(g_blk[0].last[146] * 21.3333333f),
                 (int)floorf(g_blk[1].last[144] * 21.3333333f),
                 (int)floorf(g_blk[1].last[145] * 21.3333333f),
                 (int)floorf(g_blk[1].last[146] * 21.3333333f));
    log("[blkdiff] node=%X size=%u, differing floats (%d of %u; + = advances): %s || dither [28].y "
        "M=%u (&63=%u) V=%u (&63=%u)%s",
        CyberpunkVR_BlockDiffNode, CyberpunkVR_BlockDiffSize, n, nfloat, n ? line : "(none)",
        dm, dm & 63u, dv, dv & 63u, grid);
}

void blk_note(const void* src, uint32_t size, int32_t side) {
    if (side < 0) return;                 // neither eye: a probe face, not a comparison
    const bool vrcam = side != 0;
    const uint32_t bytes = (size < kBlkMaxFloats * 4) ? size : kBlkMaxFloats * 4;
    float cur[kBlkMaxFloats] = {};
    if (!blk_copy(src, cur, bytes)) return;
    {
        std::lock_guard<std::mutex> lk(g_blk_mtx);
        // Retargeting live must not mix two blocks in one column -- the mistake that cost a whole
        // measurement earlier today -- so a change of target resets the tables.
        if (g_blk_node != CyberpunkVR_BlockDiffNode || g_blk_size != CyberpunkVR_BlockDiffSize) {
            g_blk_node = CyberpunkVR_BlockDiffNode;
            g_blk_size = CyberpunkVR_BlockDiffSize;
            memset(g_blk, 0, sizeof(g_blk));
        }
        BlkDiff& s = g_blk[vrcam ? 1 : 0];
        if (s.seen)
            for (uint32_t i = 0; i < bytes / 4; ++i)
                if (memcmp(&s.last[i], &cur[i], sizeof(float)) != 0) ++s.moved[i];
        memcpy(s.last, cur, bytes);
        ++s.seen;
    }
    blk_report(bytes / 4);
}

// ---- THE GRID ORIGIN, the one field the height mask is addressed by ------------------------------
//
// The block diff cannot be used here and the reason is worth writing down, because it is the third time
// today the same trap has caught me: the cascade node uploads an 848-byte block roughly 24 TIMES PER
// FRAME PER VIEW (4156 against 4602 in a five-second interval at ~35 fps), and a probe that keeps only
// the last one compares two arbitrary different blocks. That is what produced "132 of 212 floats differ"
// with matrix rows reading M=546.40 against V=-0.0221 -- not a finding, an artefact.
//
// So this watches ONE field instead of the whole block: b1[36].xyz, which the cascade pixel shader turns
// into the height-mask grid origin as floor(x * 21.3333) * 0.046875. And it CHECKS ITS OWN ASSUMPTION --
// that the field is one value per view per frame rather than varying across those 24 blocks -- because
// mirroring it is only meaningful if it is.
// DEFAULT 0: this one locks on an 848-byte upload that happens two dozen times a frame per view.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_GridProbe = 0;

// ---- KILL THE HEIGHT-MASK DISCARD, in both eyes --------------------------------------------------
//
// The last of the three discards in the cascade pixel shader, and the last candidate standing: the other
// two are measured dead (the LOD-fade phase is 0 in both views with one distinct value seen, and forcing
// the blue-noise slice to a constant in every frame-constants block changed nothing). The elimination is
// sound in the other direction too -- solid objects' shadows match and only alpha-tested geometry
// differs, and alpha-tested geometry is what these discards decide.
//
// Equalising this one's input would need b1[36].xy, which I could not isolate: the cascade node uploads
// an 848-byte block two dozen times a frame and the field's spread WITHIN one view is 7-10 world units,
// so those blocks are not all the same block. Rather than guess, neutralise: the mask is a 256-texel map
// covering 12 world units around floor(b1[36].xy * 21.3333) * 0.046875, and the shader itself skips the
// test when the lookup lands outside [0,1] --
//
//     if (_146 < 0 || _148 < 0 || _146 > 1 || _148 > 1)  _157 = 0;   // and 0.999 - 0 < 0 is false
//
// so an origin far from the world puts every pixel out of range and the discard never fires, in either
// eye. If the shadows agree with the test dead, this discard is the cause and the real fix is to give
// both views one origin. If they still differ, nothing in this shader's discards explains it and the
// hunt moves to the cascade matrices.
// DEFAULT 0 since it was measured innocent: neutralising this discard changed nothing on screen, and
// leaving it off costs frame rate for nothing (the engine wanted that geometry skipped). Kept because the
// measurement is worth being able to repeat.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_MaskKill = 0;
extern "C" __declspec(dllexport) float   CyberpunkVR_MaskKillOrigin = 1.0e9f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMaskKills = 0;

namespace {
// THREE candidate fields, all read by the same cascade pixel shader, all watched with their spread
// beside them so the probe says whether its own key is fine enough before its numbers are believed:
//
//   [36].xyz  the height-mask grid origin        floor(x * 21.3333) * 0.046875, step 4.7 cm
//   [51].z    the LOD-fade dither phase          (asuint(z) & 3) * 0.7, a two-bit counter
//
// [36] already measured a spread of 2.85 and 10.02 units WITHIN one view, so it is per block rather
// than per frame and comparing "the last one" says nothing -- that is recorded here rather than
// forgotten. [51].z should be per frame if it is a counter, and its spread will say so.
struct GridOrigin {
    float    v[3];
    float    lo[3];
    float    hi[3];
    uint32_t dither;          // [51].z as raw bits
    uint32_t dither_lo;
    uint32_t dither_hi;
    uint64_t seen;
};
GridOrigin g_grid[2] = {};
std::mutex g_grid_mtx;

// ---- the cascade's view-projection matrix, per cascade index and view ----------------------------
// b1[0..3] is the matrix the vertex shader transforms with (`gl_Position.x = mad(_762, _30_m0[0].z, ...)`),
// so sixteen floats from the start of the block are the placement. Keyed by cascade index, which is the
// key that was missing every previous time.
struct CascMat {
    float    m[16];
    uint64_t seen;
};
CascMat g_casc[2][8] = {};
std::mutex g_casc_mtx;


void casc_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[1400];
    int used = 0;
    line[0] = 0;
    std::lock_guard<std::mutex> lk(g_casc_mtx);
    for (int idx = 0; idx < 8; ++idx) {
        const CascMat& a = g_casc[0][idx];
        const CascMat& b = g_casc[1][idx];
        if (!a.seen || !b.seen) continue;
        int ndiff = 0;
        float worst = 0.0f;
        for (int i = 0; i < 16; ++i) {
            const float d = fabsf(a.m[i] - b.m[i]);
            if (d != 0.0f) ++ndiff;
            if (d > worst) worst = d;
        }
        if (used < static_cast<int>(sizeof(line)) - 130)
            used += snprintf(line + used, sizeof(line) - used,
                             "casc%d: %d/16 differ, worst %.6g | translation M=(%.4f, %.4f, %.4f) "
                             "V=(%.4f, %.4f, %.4f)  ", idx, ndiff, worst,
                             a.m[3], a.m[7], a.m[11], b.m[3], b.m[7], b.m[11]);
    }
    log("[cascmat] cascade b1[0..3] at draw time, keyed by cascade index: %s",
        used ? line : "(no cascade index seen on both sides yet)");
}

// ---- WHO BUILDS THE BLOCK, taken from the call stack ---------------------------------------------
//
// The fix has to go where the cascade placement is COMPUTED, and that is upstream of the upload: at the
// upload r8 points into the thread stack, so the 848 bytes are assembled in a caller's local and there is
// no persistent record to overwrite. Two attempts to find that caller by hand failed -- the return
// addresses immediately above the upload land in a per-thread pool allocator, and the cascade config
// globals are registry internals with hundreds of readers.
//
// So the plugin takes the stack itself, which is the reliable version of the same idea: capture the
// backtrace at an 848-byte upload made inside the cascade pass, keep only the frames inside the game
// module, and log them as RVAs once. Those RVAs name the builder, and then the placement can be made
// one-per-frame instead of one-per-eye.
void casc_stack_report() {
    static std::atomic<int> s_done{0};
    int expected = 0;
    if (!s_done.compare_exchange_strong(expected, 1)) return;   // once is enough
    void* frames[24] = {};
    const USHORT n = RtlCaptureStackBackTrace(0, 24, frames, nullptr);
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    char line[900];
    int used = 0;
    line[0] = 0;
    for (USHORT i = 0; i < n; ++i) {
        const uintptr_t a = reinterpret_cast<uintptr_t>(frames[i]);
        // The game module only: our own frames and the CRT's say nothing about the engine.
        if (a < base || a - base > 0x5000000) continue;
        if (used < static_cast<int>(sizeof(line)) - 16)
            used += snprintf(line + used, sizeof(line) - used, "%X ", (unsigned)(a - base));
    }
    log("[cascstack] engine frames (RVA) at the cascade's 848-byte upload: %s",
        used ? line : "(none in the game module)");
}

// The guarded read lives apart from the lock: MSVC refuses __try in a function that needs unwinding.
bool casc_copy(const void* src, float* out) {
    __try { memcpy(out, src, 16 * sizeof(float)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

void casc_note(const void* src, bool vrcam, int32_t idx) {
    if (idx < 0 || idx >= 8) return;
    float m[16];
    if (!casc_copy(src, m)) return;
    {
        std::lock_guard<std::mutex> lk(g_casc_mtx);
        CascMat& c = g_casc[vrcam ? 1 : 0][idx];
        memcpy(c.m, m, sizeof(m));
        ++c.seen;
    }
    casc_report();
}


bool grid_read(const void* src, float* out, uint32_t* dither) {
    __try {
        memcpy(out, reinterpret_cast<const uint8_t*>(src) + 144 * 4, 3 * sizeof(float));
        memcpy(dither, reinterpret_cast<const uint8_t*>(src) + 206 * 4, sizeof(*dither));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

void grid_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    std::lock_guard<std::mutex> lk(g_grid_mtx);
    if (!g_grid[0].seen || !g_grid[1].seen) return;
    const int sm[3] = { (int)floorf(g_grid[0].v[0] * 21.3333333f),
                        (int)floorf(g_grid[0].v[1] * 21.3333333f),
                        (int)floorf(g_grid[0].v[2] * 21.3333333f) };
    const int sv[3] = { (int)floorf(g_grid[1].v[0] * 21.3333333f),
                        (int)floorf(g_grid[1].v[1] * 21.3333333f),
                        (int)floorf(g_grid[1].v[2] * 21.3333333f) };
    log("[grid] cascade b1: [36].xyz MAIN snapped (%d, %d, %d) spread (%.3f, %.3f, %.3f) | VRCAM "
        "snapped (%d, %d, %d) spread (%.3f, %.3f, %.3f) | same cell=%s "
        "|| LOD dither [51].z MAIN %u (&3=%u, %u distinct values seen) | VRCAM %u (&3=%u, %u "
        "distinct) | equal=%s || n M=%llu V=%llu",
        sm[0], sm[1], sm[2],
        g_grid[0].hi[0] - g_grid[0].lo[0], g_grid[0].hi[1] - g_grid[0].lo[1],
        g_grid[0].hi[2] - g_grid[0].lo[2],
        sv[0], sv[1], sv[2],
        g_grid[1].hi[0] - g_grid[1].lo[0], g_grid[1].hi[1] - g_grid[1].lo[1],
        g_grid[1].hi[2] - g_grid[1].lo[2],
        (sm[0] == sv[0] && sm[1] == sv[1] && sm[2] == sv[2]) ? "YES" : "no",
        g_grid[0].dither, g_grid[0].dither & 3u,
        g_grid[0].dither_hi - g_grid[0].dither_lo + 1u,
        g_grid[1].dither, g_grid[1].dither & 3u,
        g_grid[1].dither_hi - g_grid[1].dither_lo + 1u,
        ((g_grid[0].dither & 3u) == (g_grid[1].dither & 3u)) ? "YES" : "no",
        (unsigned long long)g_grid[0].seen, (unsigned long long)g_grid[1].seen);
    // Per interval, so "spread" answers the assumption for the seconds being looked at rather than for
    // all time -- a lifetime spread would just accumulate the player walking around.
    memset(g_grid, 0, sizeof(g_grid));
}

void grid_note(const void* src, bool vrcam) {
    float v[3];
    uint32_t d = 0;
    if (!grid_read(src, v, &d)) return;
    {
        std::lock_guard<std::mutex> lk(g_grid_mtx);
        GridOrigin& g = g_grid[vrcam ? 1 : 0];
        if (!g.seen) {
            for (int i = 0; i < 3; ++i) { g.lo[i] = v[i]; g.hi[i] = v[i]; }
            g.dither_lo = d;
            g.dither_hi = d;
        } else {
            for (int i = 0; i < 3; ++i) {
                if (v[i] < g.lo[i]) g.lo[i] = v[i];
                if (v[i] > g.hi[i]) g.hi[i] = v[i];
            }
            if (d < g.dither_lo) g.dither_lo = d;
            if (d > g.dither_hi) g.dither_hi = d;
        }
        memcpy(g.v, v, sizeof(v));
        g.dither = d;
        ++g.seen;
    }
    grid_report();
}
}  // namespace

void sway_diff_note(const void* src, bool vrcam) {
    float cur[kSwayDiffFloats];
    if (!sway_diff_copy(src, cur)) return;
    {
        std::lock_guard<std::mutex> lk(g_swaydiff_mtx);
        SwayDiffBlk& s = g_swaydiff[vrcam ? 1 : 0];
        if (s.seen)
            for (uint32_t i = 0; i < kSwayDiffFloats; ++i)
                if (memcmp(&s.last[i], &cur[i], sizeof(float)) != 0) ++s.moved[i];
        memcpy(s.last, cur, sizeof(cur));
        ++s.seen;
    }
    sway_diff_report();
}
}  // namespace

namespace {
// Float indices into the block. Bits of CyberpunkVR_SwayTimeMask address this table in order.
//   0: [0].z  clock, seconds          3: [9].z  accumulator      6: [28].y dither slice index
//   1: [1].z  the same, prev frame    4: [9].w  accumulator      7: [0].x  second clock
//   2: [1].w  day fraction            5: [28].x counter partner  8: [1].x  ...prev frame
//   9: [0].w  differs in low bits only
constexpr uint32_t kSwayTimeFields[] = { 2, 6, 7, 38, 39, 112, 113, 0, 4, 3 };
constexpr uint32_t kSwayTimeCount = sizeof(kSwayTimeFields) / sizeof(kSwayTimeFields[0]);

// Mode 1 state: MAIN's most recent authored values, lent to the second view.
float g_sway_time[kSwayTimeCount] = {};
std::atomic<int> g_sway_time_have{0};

// Mode 2 state: one set of values per frame, given to both views.
//   g_sway_next  -- staged from MAIN's authored block, for the NEXT frame
//   g_sway_frame -- in force for the frame identified by g_sway_frame_id
// A mutex rather than atomics: this is a small struct read and written by two render threads, once per
// view per frame, and a torn set here would be a visible seam rather than a lost sample.
float g_sway_next[kSwayTimeCount] = {};
float g_sway_frame[kSwayTimeCount] = {};
uint32_t g_sway_frame_id = 0;
bool g_sway_next_have = false;
bool g_sway_frame_have = false;
std::mutex g_sway_frame_mtx;

bool sway_time_read(const void* src, float* out) {
    __try {
        const float* f = reinterpret_cast<const float*>(src);
        for (uint32_t i = 0; i < kSwayTimeCount; ++i) out[i] = f[kSwayTimeFields[i]];
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

bool sway_time_copy(const void* src, void* dst) {
    __try { memcpy(dst, src, 480); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

void sway_time_write(void* dst, const float* v) {
    float* f = reinterpret_cast<float*>(dst);
    for (uint32_t i = 0; i < kSwayTimeCount; ++i)
        if (CyberpunkVR_SwayTimeMask & (1u << i)) f[kSwayTimeFields[i]] = v[i];
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugSwayTimeFills)));
}

// ---- mode 1 -------------------------------------------------------------------------------------
void sway_time_capture(const void* src) {
    float v[kSwayTimeCount];
    if (!sway_time_read(src, v)) return;
    if (v[0] == 0.0f) return;                 // nothing to lend yet
    memcpy(g_sway_time, v, sizeof(v));
    g_sway_time_have.store(1, std::memory_order_release);
}

bool sway_time_apply(const void* src, void* dst) {
    if (!g_sway_time_have.load(std::memory_order_acquire)) return false;
    if (!sway_time_copy(src, dst)) return false;
    sway_time_write(dst, g_sway_time);
    return true;
}

// ---- mode 4: MAIN's whole block, verbatim --------------------------------------------------------
uint8_t g_sway_block[480] = {};
std::atomic<int> g_sway_block_have{0};

void sway_block_capture(const void* src) {
    uint8_t tmp[480];
    __try { memcpy(tmp, src, sizeof(tmp)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    // Guard on the clock being authored: a block of zeros would otherwise be lent out at load time and
    // freeze the second eye exactly as the original defect did.
    float clock = 0.0f;
    memcpy(&clock, tmp + 2 * 4, sizeof(clock));
    if (clock == 0.0f) return;
    memcpy(g_sway_block, tmp, sizeof(g_sway_block));
    g_sway_block_have.store(1, std::memory_order_release);
}

bool sway_block_apply(void* dst) {
    if (!g_sway_block_have.load(std::memory_order_acquire)) return false;
    memcpy(dst, g_sway_block, 480);
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugSwayTimeFills)));
    return true;
}

// ---- mode 3: the dither slice as a constant, in BOTH views ---------------------------------------
// Applied on top of whatever else the mode did, and applied to MAIN as well, which is what makes the
// two eyes agree without any assumption about upload order.
void sway_dither_force(void* dst) {
    const uint32_t v = CyberpunkVR_SwayDitherConst;
    // [28].x and [28].y, the counter pair -- written as raw bits, because the shader reads them with
    // asuint() and a float store would change the integer.
    memcpy(reinterpret_cast<uint8_t*>(dst) + 112 * 4, &v, sizeof(v));
    memcpy(reinterpret_cast<uint8_t*>(dst) + 113 * 4, &v, sizeof(v));
}

// ---- mode 2: one set of values per frame, for whichever view asks first --------------------------
bool sway_frame_apply(const void* src, void* dst, bool vrcam, uint32_t frame_id) {
    float authored[kSwayTimeCount];
    const bool read_ok = sway_time_read(src, authored);
    float use[kSwayTimeCount];
    bool have = false;
    {
        std::lock_guard<std::mutex> lk(g_sway_frame_mtx);
        if (frame_id != g_sway_frame_id) {
            g_sway_frame_id = frame_id;
            if (g_sway_next_have) {
                memcpy(g_sway_frame, g_sway_next, sizeof(g_sway_frame));
                g_sway_frame_have = true;
            }
        }
        // Only MAIN's block is authored with real values -- the second view's clocks are zero -- so
        // MAIN is the only source, staged one frame ahead so that the value is already in hand
        // whichever view opens the next frame.
        if (!vrcam && read_ok && authored[0] != 0.0f) {
            memcpy(g_sway_next, authored, sizeof(g_sway_next));
            g_sway_next_have = true;
        }
        if (g_sway_frame_have) {
            memcpy(use, g_sway_frame, sizeof(use));
            have = true;
        }
    }
    if (!have) return false;
    if (!sway_time_copy(src, dst)) return false;
    sway_time_write(dst, use);
    return true;
}
}  // namespace

// The buffer uploader, which is where these blocks actually pass. Every node calls it, so the filter
// is one integer compare before anything else happens.
using BufUploadFn = int64_t(__fastcall*)(uint32_t, uint32_t, void*);
static BufUploadFn g_orig_buf_upload = nullptr;
// Mode 5's content filter: is this 480-byte block the frame constants? The clock at [0].z runs into the
// hundreds of thousands of seconds, so "greater than a thousand" separates it from every other block of
// that size seen in the census -- one of which carried 1.0f there and render-target dimensions further
// on, and must not be written.
// ---- THE WIDE CENSUS: every large upload, both uploaders, and whether the views disagree ----------
//
// Built because the narrow version has now cost five rounds. Each time the shape was the same: a probe on ONE
// uploader, or keyed to ONE node picked by reasoning, and a silence that read like a negative result. The
// latest was aiming at BindLightingGlobalConstants -- which turns out to be 84 bytes with a single callee,
// i.e. a BIND that points the root signature at a buffer somebody else filled, so it uploads nothing and the
// probe was always going to print nothing.
//
// This asks the question the other way round and without a candidate: for every upload of 128 bytes or more,
// on BOTH uploaders, keyed by (node, size), how often each view does it and whether their CONTENTS ever
// differ. The first 64 bytes are enough to detect a difference and cheap enough to compare on a hot path.
// Rows where the two views disagree are where a per-view value lives, and the shadow sampling matrices --
// whatever node owns them -- have to appear among them.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_WideCensus = 1;
// The per-field temporal detector rides on the same census; see the long note at kTFields.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_TemporalScan = 1;
extern "C" __declspec(dllexport) const char* CyberpunkVR_ProfNodeName(uint32_t rva);
#define CyberpunkVR_ProfNodeName2 CyberpunkVR_ProfNodeName
namespace {
constexpr uint32_t kWideHead = 64;
// ---- and the TEMPORAL FIELD DETECTOR, which is the generalisation of everything above -------------
//
// The question that prompted it: the census lists a dozen blocks whose contents differ between the views, so
// why not just copy them all? Because most of those differences are correct -- the 848-byte rows are
// CameraShaderConsts, and two eyes ARE two cameras; forcing them equal renders both eyes from one camera and
// the picture goes flat. Copying everything is not a fix, it is deleting the second eye.
//
// But the blocks hold two different kinds of field, and only one kind is a bug:
//
//     position, orientation, view matrices        MUST differ   -- that is what stereo is
//     jitter, sequence index, clocks, counters    MUST match    -- or the eyes accumulate different results
//
// Both of today's real findings were the second kind: the frame clock that left the flags frozen, and the
// cascade fit. So instead of hunting block by block, separate the two kinds ONCE, across every block.
//
// THE DISCRIMINATOR IS A STATIONARY CAMERA. Hold still and pose-derived fields stop changing while temporal
// ones keep advancing. So this counts, per field of every block: how often it changes in each view, and how
// often the two views disagree about it. Read while standing still, a field that still advances in both views
// AND differs between them is temporal, and temporal fields are the ones worth equalising.
//
// The window is the first 128 floats of each block, which covers the fog block whole and the head of the
// larger ones; the log says so rather than pretending to cover everything.
constexpr uint32_t kTFields = 128;
struct WideRow {
    uint32_t node, size;
    uint64_t hits[2];
    uint8_t  head[2][kWideHead];
    bool     seen[2];
    uint64_t diffs;
    float    last[2][kTFields];
    uint32_t moved[2][kTFields];
    uint32_t vdiff[kTFields];
    uint32_t samples[2];
    // HOW MANY TIMES A VIEW UPLOADS THIS BLOCK IN ONE FRAME, which is what finally makes the comparison
    // honest. Keeping only the latest sample per side is fine for a block uploaded once per view per frame,
    // and meaningless for one uploaded many times with different sub-view state: RenderSkyScattering reported
    // 43 fields swapping +-1 between the "views" because it was comparing MAIN's last CUBEMAP FACE against the
    // second eye's last face, and ReflectionProbes rows showed up with both columns filled because each eye
    // renders its own probes. Same for RenderShadowCascade, which uploads once per cascade. So a row is only
    // reported when both views upload it exactly once a frame.
    uint32_t last_frame[2];
    uint32_t in_frame[2];
    uint32_t max_per_frame[2];
};
WideRow g_wide[192];
uint32_t g_wide_n = 0;
std::mutex g_wide_mtx;

bool wide_copy(const void* src, void* dst) {
    __try { memcpy(dst, src, kWideHead); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

bool wide_copy_floats(const void* src, float* dst, uint32_t n) {
    __try { memcpy(dst, src, n * sizeof(float)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

void wide_note(const void* src, uint32_t size, int32_t side) {
    if(!cvr::RuntimeDiagnosticsEnabled())return;
    // side < 0 is a view that is neither eye -- a reflection-probe face. Counting it as MAIN is
    // exactly what made this census report mixtures.
    if (!CyberpunkVR_WideCensus || !src || size < 128 || side < 0) return;
    uint8_t head[kWideHead];
    if (!wide_copy(src, head)) return;
    uint32_t node = 0;
    if (g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        if (t_current_node_work > base) node = static_cast<uint32_t>(t_current_node_work - base);
    }
    const int v = side;
    uint32_t frame_id = 0;
    if (g_exe_base) {
        const uintptr_t renderer =
            *reinterpret_cast<uintptr_t*>(g_exe_base + RENDERER_GLOBAL_RVA);
        if (renderer) frame_id = *reinterpret_cast<uint32_t*>(renderer + 0x4CC8);
    }
    const uint32_t nf = (size / 4 < kTFields) ? size / 4 : kTFields;
    float cur[kTFields];
    const bool have_fields = CyberpunkVR_TemporalScan && wide_copy_floats(src, cur, nf);
    std::lock_guard<std::mutex> lk(g_wide_mtx);
    uint32_t i = 0;
    for (; i < g_wide_n; ++i)
        if (g_wide[i].node == node && g_wide[i].size == size) break;
    if (i == g_wide_n) {
        if (g_wide_n >= 192) return;
        g_wide[g_wide_n] = WideRow{node, size, {0, 0}, {}, {false, false}, 0};
        i = g_wide_n++;
    }
    WideRow& r = g_wide[i];
    ++r.hits[v];
    memcpy(r.head[v], head, kWideHead);
    r.seen[v] = true;
    // A difference only means something once BOTH views have been seen for this (node, size).
    if (r.seen[0] && r.seen[1] && memcmp(r.head[0], r.head[1], kWideHead) != 0) ++r.diffs;

    // ---- per-field, for the temporal detector ----
    if (!CyberpunkVR_TemporalScan || !have_fields) return;
    if (r.samples[v]) {
        for (uint32_t k = 0; k < nf; ++k)
            if (memcmp(&r.last[v][k], &cur[k], sizeof(float)) != 0) ++r.moved[v][k];
    }
    memcpy(r.last[v], cur, nf * sizeof(float));
    ++r.samples[v];
    if (frame_id != r.last_frame[v]) { r.last_frame[v] = frame_id; r.in_frame[v] = 1; }
    else ++r.in_frame[v];
    if (r.in_frame[v] > r.max_per_frame[v]) r.max_per_frame[v] = r.in_frame[v];
    // The between-view comparison uses each view's LATEST sample, which is why it is only trusted for fields
    // that also advance: for a field that sits still, "latest" is the same thing in both views anyway.
    if (r.samples[0] && r.samples[1]) {
        for (uint32_t k = 0; k < nf; ++k)
            if (memcmp(&r.last[0][k], &r.last[1][k], sizeof(float)) != 0) ++r.vdiff[k];
    }
}

void temporal_report() {
    if(!cvr::RuntimeDiagnosticsEnabled())return;
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 6000) return;
    s_last = now;
    // ONE LINE PER BLOCK, and a count of anything dropped. The first version packed every block into a single
    // 1500-character buffer with a 280-character slot each and a 14-block cap, and simply stopped when full --
    // silently, which is how a report starts lying about what it found. That failure has its own entry in this
    // project's notes and it still caught me here.
    uint32_t rows = 0, dropped = 0, multi = 0;
    std::lock_guard<std::mutex> lk(g_wide_mtx);
    for (uint32_t i = 0; i < g_wide_n; ++i) {
        WideRow& r = g_wide[i];
        if (r.samples[0] < 16 || r.samples[1] < 16) continue;
        // Once per view per frame, or the "latest sample" of each side is not the same thing on both sides.
        if (r.max_per_frame[0] != 1 || r.max_per_frame[1] != 1) { ++multi; continue; }
        const uint32_t nf = (r.size / 4 < kTFields) ? r.size / 4 : kTFields;
        char fields[1400];
        int uf = 0, nfound = 0, shown = 0;
        fields[0] = 0;
        for (uint32_t k = 0; k < nf; ++k) {
            // Advances in BOTH views -- a field that moved once is not a counter -- and the views disagree
            // about it on most samples.
            if (r.moved[0][k] * 4 < r.samples[0] || r.moved[1][k] * 4 < r.samples[1]) continue;
            if (r.vdiff[k] * 2 < r.samples[0]) continue;
            // AND THE RELATIVE SIZE OF THE DISAGREEMENT, which is what replaced "read it while standing
            // still". That test assumed pose-derived fields stop moving when the camera does -- and in VR the
            // camera never does: head tracking always jitters, so every matrix row kept advancing and the
            // filter passed the whole of CameraShaderConsts. This works instead because the two kinds
            // disagree by very different AMOUNTS. Two eyes are 6.5 cm apart, so anything derived from the
            // pose differs by a whisker of its own magnitude -- the fog block's camera X read 520.3243
            // against 520.2593, a relative 1e-4. A counter or a jitter offset differs arbitrarily: that same
            // block's jitter read -0.0499 against -0.1045 and its sequence index 592 against 78. So: keep
            // fields whose views disagree by at least a fifth of the larger magnitude, with a floor so that
            // two near-zero values are not called a disagreement.
            const float a = fabsf(r.last[0][k]), b = fabsf(r.last[1][k]);
            const float big = (a > b) ? a : b;
            if (big < 1e-3f) continue;
            if (fabsf(r.last[0][k] - r.last[1][k]) < 0.2f * big) continue;
            ++nfound;
            if (uf < static_cast<int>(sizeof(fields)) - 44) {
                uf += snprintf(fields + uf, sizeof(fields) - uf, "[%u].%c=%.3f/%.3f ",
                               k / 4, "xyzw"[k % 4], r.last[0][k], r.last[1][k]);
                ++shown;
            }
        }
        if (!nfound) continue;
        ++rows;
        dropped += static_cast<uint32_t>(nfound - shown);
        const char* nm = CyberpunkVR_ProfNodeName2(r.node);
        log("[temporal] %uB@%s (%d field%s, M/V): %s%s", r.size,
            (nm && *nm) ? nm : (r.node ? "?" : "no-node"), nfound, nfound == 1 ? "" : "s", fields,
            (nfound > shown) ? "... TRUNCATED" : "");
        // Per interval, so the answer describes the seconds being looked at -- which matters here, because
        // the whole test is "what still advances while the camera is NOT moving".
        memset(r.moved, 0, sizeof(r.moved));
        memset(r.vdiff, 0, sizeof(r.vdiff));
        r.samples[0] = r.samples[1] = 0;
        r.max_per_frame[0] = r.max_per_frame[1] = 0;
    }
    log("[temporal] --- %u block%s with fields that advance in both eyes and disagree by >=20%% (pose-derived "
        "fields disagree by ~1e-4), window = first %u floats%s",
        rows, rows == 1 ? "" : "s", kTFields,
        dropped ? " -- SOME FIELDS TRUNCATED, see the rows above" : "");
    if (multi)
        log("[temporal] %u block(s) skipped: uploaded more than once per view per frame (cubemap faces, "
            "cascades), so comparing each side's latest sample would compare different sub-views", multi);
}

void wide_report() {
    if(!cvr::RuntimeDiagnosticsEnabled())return;
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    s_last = now;
    char line[1500];
    int used = 0;
    line[0] = 0;
    std::lock_guard<std::mutex> lk(g_wide_mtx);
    for (uint32_t i = 0; i < g_wide_n; ++i) {
        const WideRow& r = g_wide[i];
        // Only rows BOTH views produce and whose contents disagree: a one-sided row is a pass one eye owns,
        // and a matching row is the engine agreeing with itself. Neither is what is being looked for.
        if (!r.seen[0] || !r.seen[1] || !r.diffs) continue;
        if (used >= static_cast<int>(sizeof(line)) - 80) break;
        const char* nm = CyberpunkVR_ProfNodeName2(r.node);
        used += snprintf(line + used, sizeof(line) - used, "%uB@%s M=%llu V=%llu d=%llu  ", r.size,
                         (nm && *nm) ? nm : (r.node ? "?" : "no-node"),
                         (unsigned long long)r.hits[0], (unsigned long long)r.hits[1],
                         (unsigned long long)r.diffs);
    }
    log("[wide] uploads >=128 B that BOTH views make and whose contents differ, by (node, size): %s",
        used ? line : "(none yet)");
}
}  // namespace

// ---- THE VOLUMETRIC FOG'S JITTER, put in step between the eyes -----------------------------------
//
// Found by the wide census plus a field diff, after the cascade fit was equalised and the blocky sun shafts
// were still displaced. The fog pass uploads a 384-byte block once per view per frame, and ten of its 96
// floats differ. Most of that is legitimate:
//
//     [7].x  M=520.3243  V=520.2593   -- apart by 0.0650, i.e. exactly the IPD: the camera position
//     [7].y, [2].x, [2].y             -- differences of 1e-4, derived from it
//
// but four are not:
//
//     [10].x  M=-0.0499  V=-0.1045     the froxel jitter offset, advancing every frame
//     [10].y  M= 0.2578  V= 0.3414
//     [10].z  M= 0.4227  V=-0.1234
//     [11].z  M= 592     V= 78         the index into that jitter sequence
//
// Volumetric fog is a coarse 3D grid whose blockiness is hidden by jittering it per frame and accumulating
// over time. The two eyes jitter with different offsets and sit at different points of the sequence -- 592
// against 78 -- so they accumulate different fog. That is what shows as sun shafts displaced in
// opposite directions and a whole block of light present in one eye and absent in the other, and why it
// reads as the same class of artefact as the grass without being the shadow map at all.
//
// The second view runs first, so it is captured and MAIN is given its values, in-frame: taking MAIN's
// latest would hand the earlier view a sequence position from the previous frame, which is the same trap
// the cascade lend had to avoid. Copied as raw dwords, because [11].z is an integer in a float slot.
//
// AND ALL OF THAT IS RETRACTED. A later reading of the same diff printed 49 of 96 floats differing, with
// MAIN's column holding an exactly axis-aligned basis ([4] = (-1,0,0), [5] = (0,0,1), [6] = (0,-1,0)), a
// near-origin position ([7] = (0.04, 3.62, 1.59)) and round defaults ([12] = 250/250/125, [13] = 0.9/0.9/0.9)
// while the other column held a real world camera at (-7026, -4512, 80). Those are not two views of a scene.
// MAIN's column was catching a CUBEMAP FACE -- a reflection probe's fog block -- because more than one
// consumer uploads 384 bytes under this node, and the (node, size) key cannot tell them apart.
//
// So the "10 of 96 differ" reading that this fix was built on is void as well: it was comparing whichever
// instance each view happened to upload last, which is the same key-too-coarse mistake that has now cost
// this investigation five separate rounds. The fix DID fire (its counter advanced once per frame, verified)
// and it changed nothing visible -- which is the only honest thing left to say about it, because a patch
// keyed on a mixture may well have been writing scene jitter into a probe face.
//
// DEFAULT 0. Re-arming it needs a CONTENT filter, not a tighter node: the player's block is the one whose
// camera position matches the world position the player view carries, and a basis that is not axis-aligned.
// Until the probe can separate the instances, nothing measured here about per-view fog jitter is established.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_FogJitterFix = 0;
// Gated: the player view's 384-byte volumetric-fog block, per side (MAIN, VRCAM), as last uploaded.
extern "C" __declspec(dllexport) float CyberpunkVR_FogBlockDump[2][96] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFogJitterFixes = 0;

namespace {
constexpr uint32_t kFogBlockBytes = 384;
// Float indices: [10].x/[10].y/[10].z = 40/41/42, [11].z = 46.
constexpr uint32_t kFogJitterFields[] = { 40, 41, 42, 46 };
constexpr uint32_t kFogJitterCount = sizeof(kFogJitterFields) / sizeof(kFogJitterFields[0]);
uint32_t g_fogj[kFogJitterCount] = {};
std::atomic<int> g_fogj_have{0};

bool fog_block_copy(const void* src, void* dst) {
    __try { memcpy(dst, src, kFogBlockBytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

void fog_jitter_capture(const void* src) {
    uint32_t v[kFogJitterCount];
    __try {
        for (uint32_t i = 0; i < kFogJitterCount; ++i)
            memcpy(&v[i], reinterpret_cast<const uint8_t*>(src) + kFogJitterFields[i] * 4, 4);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    memcpy(g_fogj, v, sizeof(v));
    g_fogj_have.store(1, std::memory_order_release);
}

bool fog_jitter_apply(const void* src, void* dst) {
    if (!g_fogj_have.load(std::memory_order_acquire)) return false;
    if (!fog_block_copy(src, dst)) return false;
    for (uint32_t i = 0; i < kFogJitterCount; ++i)
        memcpy(reinterpret_cast<uint8_t*>(dst) + kFogJitterFields[i] * 4, &g_fogj[i], 4);
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugFogJitterFixes)));
    return true;
}

// True when this upload is the PLAYER VIEW's fog block. The node and the size are not enough -- reflection
// probes upload 384 bytes under this node too, and a probe face is what MAIN's column of the diff turned out
// to be catching. A cubemap face is recognisable from its own contents: its basis is exactly axis-aligned, so
// every component of the three rows is 0 or +-1. The player's view is never that.
bool fog_block_is_player_view(const void* src) {
    float basis[9];
    __try { memcpy(basis, reinterpret_cast<const uint8_t*>(src) + 16 * 4, sizeof(basis)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    for (int i = 0; i < 9; ++i) {
        const float a = fabsf(basis[i]);
        if (a > 1e-4f && a < 0.9999f) return true;      // an off-axis component: a real view
    }
    return false;
}

void fog_block_dump(uint32_t size, const void* src) {
    if (size != kFogBlockBytes || !src || !g_exe_base || !cvr::RuntimeDiagnosticsEnabled() ||
        (t_view_side != 0 && t_view_side != 1)) return;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    if (t_current_node_work <= base || static_cast<uint32_t>(t_current_node_work - base) != VOLUMETRIC_FOG_NODE_RVA) return;
    if (!fog_block_is_player_view(src)) return;
    __try { memcpy(CyberpunkVR_FogBlockDump[t_view_side], src, kFogBlockBytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) {}
}

bool fog_block_here(uint32_t size, const void* src) {
    fog_block_dump(size, src);
    if (!CyberpunkVR_FogJitterFix || size != kFogBlockBytes || !src || !g_exe_base) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    if (t_current_node_work <= base ||
            static_cast<uint32_t>(t_current_node_work - base) != VOLUMETRIC_FOG_NODE_RVA)
        return false;
    return fog_block_is_player_view(src);
}
}  // namespace

// ---- THE PROJECTION JITTER, put in step between the eyes -----------------------------------------
//
// The once-per-frame census, after it was finally made to compare two eyes and nothing else, left four scalars
// of the 848-byte CameraShaderConsts differing while every matrix entry around them agreed to 1e-4:
//
//     [0].w   M= 0.062  V=-0.031        float 3
//     [4].z   M= 2.134  V=-1.066        float 18
//     [16].w  M= 0.062  V=-0.031        float 67
//     [28].w  M= 0.062  V=-0.031        float 115
//
// The same values appear under RenderShadowmask, VolumetricFog, ApplyTXAA, RenderBackground and
// RenderScreenSpaceWaterDepth -- i.e. it is one shared field of that block, not a per-pass quantity -- and
// they advance every frame. That is the shape of a projection JITTER: the sub-pixel offset an engine adds per
// frame so a temporal filter can accumulate. Two eyes on different jitter sequences accumulate different
// results, and the passes carrying it are exactly the ones that draw the blocky sun shafts seen
// displaced in opposite directions.
//
// WHAT IS NOT CLAIMED. The field is not identified. The 1:-2 ratio between the views is one instant's sample,
// not a law -- a jitter sequence gives an arbitrary ratio each frame -- and an earlier reading of these same
// numbers as an eye offset was WRONG: the port's own camera measurement says the eyes sit 0.0640 m apart
// against an expected 0.0640 with no vertical disparity, so the stereo placement is correct and this is
// something else. What is established is only that these four advance and disagree while their neighbours do
// not. Equalising them is therefore a test, and its failure mode is obvious: if they are geometric rather than
// temporal, MAIN's projection visibly breaks.
//
// Captured from the second view, which runs first, and applied to MAIN in the same frame -- the same direction
// the cascade lend needed for the same reason.
//
// ANSWERED, AND THE ANSWER IS GEOMETRIC. With MAIN forced onto the other eye's values the shadows slid and
// snapped back about once a second and the MENU came up shifted by half a screen. A menu is drawn straight
// through the same projection, so a half-screen displacement says these four scalars are part of the transform
// and not a temporal offset riding on it. The failure mode was written down before the test and it is exactly
// what happened, which is the one good thing about it.
//
// So: whatever advances in them every frame, they carry geometry, and forcing them across the eyes is not
// available. DEFAULT 0, and it stays 0 -- kept only because the four field indices and their measured values
// are the record of what was ruled out.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_JitterFix = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugJitterFixes = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugJitterUnstable = 0;

namespace {
constexpr uint32_t kCamBlockBytes = 848;
constexpr uint32_t kJitterFields[] = { 3, 18, 67, 115 };
constexpr uint32_t kJitterCount = sizeof(kJitterFields) / sizeof(kJitterFields[0]);
uint32_t g_jit[kJitterCount] = {};
std::atomic<int> g_jit_have{0};

bool jitter_read(const void* src, uint32_t* out) {
    __try {
        for (uint32_t i = 0; i < kJitterCount; ++i)
            memcpy(&out[i], reinterpret_cast<const uint8_t*>(src) + kJitterFields[i] * 4, 4);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

// Capture, and check the assumption while doing it: if the value is not the same across every 848-byte upload
// a view makes in a frame, then it is per-pass after all and taking "any of them" would be meaningless. The
// counter says so instead of the code assuming it.
void jitter_capture(const void* src) {
    uint32_t v[kJitterCount];
    if (!jitter_read(src, v)) return;
    if (g_jit_have.load(std::memory_order_acquire) &&
            memcmp(v, g_jit, sizeof(v)) != 0)
        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugJitterUnstable)));
    memcpy(g_jit, v, sizeof(v));
    g_jit_have.store(1, std::memory_order_release);
}

bool jitter_apply(const void* src, void* dst) {
    if (!g_jit_have.load(std::memory_order_acquire)) return false;
    __try { memcpy(dst, src, kCamBlockBytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    for (uint32_t i = 0; i < kJitterCount; ++i)
        memcpy(reinterpret_cast<uint8_t*>(dst) + kJitterFields[i] * 4, &g_jit[i], 4);
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugJitterFixes)));
    return true;
}
}  // namespace

// ---- THE CASCADE SAMPLING MATRIX: the half of the shadow setup the fit lend never reached --------
//
// Measured in RenderDoc on two independent captures, and it resolves a contradiction this file already
// records a few hundred lines down: the sun-cascade atlas is SHARED by the eyes -- one texture, written
// exactly once per frame by a single copy before either eye's pass -- and the cascade fit record is identical
// after the lend in ViewReuse.cpp ([cascrec] "differing dwords (0)", [cascfit] after=0.0000 m). By that
// reasoning the two eyes read one shadow map identically. They do not, and the dither hunt that followed from
// the same reasoning came back innocent.
//
// The missing half is the transform each eye uses to READ that shared atlas. The mask pixel shader declares
// its own constant layout, so these offsets are not guesses:
//
//     b13 CSConstants 928 B = float4 x6 | cascadeMatrix[4] | cascadeVec[4] | ... | SDistantShadowParams | ...
//
// cascadeMatrix maps an ABSOLUTE world position into the atlas -- verified by transforming the camera position
// through it and landing inside the unit cube -- so it cannot legitimately depend on which eye is looking. It
// does. Twelve of the block's 58 float4 rows differ between the eyes, the same twelve in both captures, and
// cascadeMatrix[0] and [1] are among them:
//
//     cascade 0 (12 m across, 1 texel = 5.9 mm)    lateral -5.71 / -4.83 texel    depth +58.1 mm
//     cascade 1 (40 m across, 1 texel = 19.5 mm)   lateral -8.34 / +3.39 texel    depth +58.2 mm
//
// The lateral part changes between captures because the fit is snapped to the texel grid, so what survives
// there is rounding. The depth part does not change: 58.1 / 58.2 mm in one capture, 58.1 / 58.4 in the other,
// against a geometric prediction of 58.1 mm -- the measured 0.0640 m eye separation projected onto the sun
// direction. So the cascade frame follows each eye's own camera, and each eye tests the shared atlas against
// depths biased by that projection.
//
// cascadeMatrix[2] and [3] are all zero in both captures, which is the same answer the count probe in
// ViewReuse.cpp reached from the other side: there are two cascades, and the atlas has two array slices.
//
// DIRECTION. Captured from the second view, which runs first, and applied to MAIN -- the same direction the fit
// lend needs and for the same reason. The atlas is rasterised with the second view's fit, because that is what
// the fit lend hands MAIN, so the second view's sampling matrix is the one that matches the pixels in it.
//
// Mode 1 replaces the two cascade matrices and nothing else. Mode 2 replaces the whole 928-byte block, kept as
// a separate measurement rather than a wider default because the other ten differing rows are not identified.
//
// FAILURE MODE, written down before the test. If these matrices are not what samples the atlas, MAIN's shadows
// will move wholesale rather than settle -- a visible slide, or a constant offset in one eye. That is the shape
// CyberpunkVR_JitterFix produced above, and the remedy is the same: set this to 0.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascSampleLend = 1;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascSampleProbe = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascSampleLends = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascSampleSkips = 0;

namespace {
constexpr uint32_t kCascSampleBytes = 928;      // CSConstants, as the mask pixel shader declares it
constexpr uint32_t kCascMatFloat = 24;          // cascadeMatrix[0] begins at byte 96
constexpr uint32_t kCascMatFloats = cvr::stereo::CascadeMatrixFloatCount;
static_assert(kCascMatFloat*4+kCascMatFloats*sizeof(float)<=kCascSampleBytes);

// PAIRED BY ORDINAL WITHIN EACH VIEW'S TURN, not by "the last snapshot I saw".
//
// This node uploads the block TWICE per view per frame, and the first version kept one snapshot and handed it
// to both of MAIN's uploads. Standing still that is harmless -- every pose in the frame is the same one -- and
// standing still is exactly the case that got fixed. Moving, it is not harmless: if the second view's two
// uploads are two different things, MAIN's first upload receives the second one's matrix, and the error is the
// motion between them rather than the 58 mm eye offset. The live report agrees that something of that size is
// in play: the two views' matrices now disagree by 120-190 mm along the light while moving, against 58 mm
// measured standing.
//
// So each view's turn is numbered from zero -- a turn begins when t_view_side changes at this node -- and the
// k-th upload of MAIN is paired with the k-th of the second view. If MAIN reaches a k the second view never
// filled, the lend is refused and counted rather than served a matrix from a different instant.
//
// The slot spread in the report is the measurement that decides the remaining question: if the second view's
// own slot 0 and slot 1 hold the same matrix, then pairing was never the problem and the rasterisation half is,
// and the next block to lend is 848B@RenderShadowCascade -- which the census says differs between the views.
constexpr int kCascSlots = 4;

float   g_cascs_slot[kCascSlots][kCascMatFloats] = {};
uint8_t g_cascs_slot_all[kCascSlots][kCascSampleBytes] = {};
int     g_cascs_have_n = 0;                     // slots the second view filled in its current turn
int     g_cascs_idx = 0;                        // next ordinal within the current turn
int     g_cascs_side = -2;                      // the side whose turn it is, as last seen HERE
std::atomic<int> g_cascs_said{0};

float    g_cascs_dbasis[cvr::stereo::CascadeMatrixCount] = {};
float    g_cascs_dtrans[cvr::stereo::CascadeMatrixCount] = {};
float    g_cascs_dmm[cvr::stereo::CascadeMatrixCount] = {};
float    g_cascs_wskip = 0.0f;
float    g_cascs_spread = 0.0f;                  // mm between the second view's own slot 0 and slot k
uint64_t g_cascs_pairs = 0;
uint64_t g_cascs_miss = 0;                       // MAIN reached an ordinal the second view never filled

bool cascs_read_mat(const void* src, float* out) {
    __try {
        memcpy(out, reinterpret_cast<const uint8_t*>(src) + kCascMatFloat * 4,
               kCascMatFloats * sizeof(float));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return true;
}

// Is this 928-byte upload the sun-cascade setup at all? Content, not just node and size: the fog fix above
// exists because a node plus a size caught a reflection-probe face, and this node uploads this size TWICE per
// view per frame (census: 928B@PrepareSceneRendering M=8844 against 1584B@PrepareSceneRendering M=4422). A
// cascade matrix has three rows ending in 0 and a translation row ending in 1.
// Low uses two matrices, but Medium enables a third. Requiring matrices 2/3
// to be zero silently disabled this entire correction after a settings change.
// Accept the active prefix and preserve the native zero tail at every quality.
bool cascs_is_setup(const void* src) {
    float matrices[kCascMatFloats];
    return cascs_read_mat(src,matrices) && cvr::stereo::ActiveCascadeMatrices(matrices)>0;
}

// Tell one INSTANCE of this block from another -- and do not mistake a pose for an instance.
//
// The first version compared the basis components and the translation against bounds taken from a capture of a
// STANDING player, and it rejected 365 of 473 blocks: lends=108 skipped=365 in the log. Worse, it rejected
// exactly the frames that matter. The cascade's depth axis is the light -- normalise the matrix's third column
// and it comes out as -sun to six digits -- while its u/v axes are built from the view direction, so a turning
// head rotates the whole basis, and the two views' matrices are built from poses sampled milliseconds apart.
// Any bound tight enough to identify an instance from rotation or translation therefore throws away every
// moving frame, which is the only kind that shows the artefact. That is the fifth time in this file a filter
// has been narrower than the thing it was measuring.
//
// SCALE is what identifies the instance and does not move between two views of one frame: cascade 0 spans
// 12.00 m and cascade 1 spans 40.00 m, measured identical in both captures. Rotation and translation are left
// entirely alone, which is the whole point -- they are what the lend exists to transfer.
bool cascs_same_setup(const float* a, const float* b, float* worst) {
    return cvr::stereo::SameCascadeSamplingLayout(a,b,worst);
}

// Two matrices' disagreement along the LIGHT, in millimetres. The depth axis is the third column, and its
// length is depth units per metre, so the translation difference along it converts straight into millimetres
// and is directly comparable with the 58 mm the captures measured. The TRANSLATION part alone: the basis terms
// contribute through the world position too, and this side of the fence cannot see a world position.
float cascs_mm(const float* pa, const float* pb) {
    float len = 0.0f;
    for (int r = 0; r < 3; ++r) len += pa[r * 4 + 2] * pa[r * 4 + 2];
    len = sqrtf(len);
    return (len > 1e-9f) ? fabsf(pa[14] - pb[14]) / len * 1000.0f : 0.0f;
}

void cascs_note_pair(const float* mine, const float* theirs) {
    ++g_cascs_pairs;
    for (size_t m = 0; m < cvr::stereo::CascadeMatrixCount; ++m) {
        const float* pa = mine + m * 16;
        const float* pb = theirs + m * 16;
        float db = 0.0f, dt = 0.0f;
        for (int r = 0; r < 3; ++r) {
            for (int c = 0; c < 3; ++c) {
                const float d = fabsf(pa[r * 4 + c] - pb[r * 4 + c]);
                if (d > db) db = d;
            }
        }
        for (int c = 0; c < 3; ++c) {
            const float d = fabsf(pa[12 + c] - pb[12 + c]);
            if (d > dt) dt = d;
        }
        const float mm = cascs_mm(pa, pb);
        if (db > g_cascs_dbasis[m]) g_cascs_dbasis[m] = db;
        if (dt > g_cascs_dtrans[m]) g_cascs_dtrans[m] = dt;
        if (mm > g_cascs_dmm[m]) g_cascs_dmm[m] = mm;
    }
}

// One line the first time the lend lands, whatever the launcher's DEBUG box says -- it is the difference
// between a fix that shipped and a fix that silently never matched a block, and one line per session is free.
// The repeating report stays behind the probe flag.
void cascs_report(bool first) {
    if (!first) {
        if (!CyberpunkVR_CascSampleProbe) return;
        static uint64_t s_last = 0;
        const uint64_t now = GetTickCount64();
        if (s_last && now - s_last < 5000) return;
        s_last = now;
    }
    log("[cascsample] cascadeMatrix, MAIN against the second view BEFORE the lend, worst in this interval: "
        "casc0 basis=%.2e trans=%.2e (%.1f mm along the light) | casc1 basis=%.2e trans=%.2e (%.1f mm) | "
        "casc2 basis=%.2e trans=%.2e (%.1f mm) | casc3 basis=%.2e trans=%.2e (%.1f mm) | "
        "pairs=%llu lends=%llu skipped=%llu (worst scale mismatch %.3f) unpaired=%llu | the second view's "
        "OWN slots differ by %.1f mm | mode=%d",
        g_cascs_dbasis[0], g_cascs_dtrans[0], g_cascs_dmm[0],
        g_cascs_dbasis[1], g_cascs_dtrans[1], g_cascs_dmm[1],
        g_cascs_dbasis[2], g_cascs_dtrans[2], g_cascs_dmm[2],
        g_cascs_dbasis[3], g_cascs_dtrans[3], g_cascs_dmm[3],
        (unsigned long long)g_cascs_pairs,
        (unsigned long long)CyberpunkVR_DebugCascSampleLends,
        (unsigned long long)CyberpunkVR_DebugCascSampleSkips,
        g_cascs_wskip,
        (unsigned long long)g_cascs_miss,
        g_cascs_spread,
        (int)CyberpunkVR_CascSampleLend);
    if (first) return;                          // keep the first line's numbers in the interval it reports
    for (size_t m = 0; m < cvr::stereo::CascadeMatrixCount; ++m) {
        g_cascs_dbasis[m] = 0.0f;
        g_cascs_dtrans[m] = 0.0f;
        g_cascs_dmm[m] = 0.0f;
    }
    g_cascs_wskip = 0.0f;
    g_cascs_spread = 0.0f;
    g_cascs_pairs = 0;
    g_cascs_miss = 0;
}

bool cascs_block_here(uint32_t size, const void* src) {
    if (!CyberpunkVR_CascSampleLend || size != kCascSampleBytes || !src || !g_exe_base) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    if (t_current_node_work <= base ||
            static_cast<uint32_t>(t_current_node_work - base) != PREPARE_SCENE_NODE_WORK_RVA)
        return false;
    return cascs_is_setup(src);
}

// A turn begins when the side changes at this node; within a turn the uploads are numbered from zero.
int cascs_ordinal(int side) {
    if (side != g_cascs_side) {
        g_cascs_side = side;
        g_cascs_idx = 0;
        if (side == 1) g_cascs_have_n = 0;       // the second view starts filling its slots afresh
    }
    return g_cascs_idx++;
}

void cascs_capture(const void* src, int slot) {
    if (slot < 0 || slot >= kCascSlots) return;
    float mat[kCascMatFloats];
    if (!cascs_read_mat(src, mat)) return;
    uint8_t all[kCascSampleBytes];
    __try { memcpy(all, src, kCascSampleBytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    memcpy(g_cascs_slot[slot], mat, sizeof(mat));
    memcpy(g_cascs_slot_all[slot], all, kCascSampleBytes);
    if(slot==0 && t_view_side==1)cvr::cascades::SourceSampling(mat,unsigned(cvr::stereo::ActiveCascadeMatrices(mat)));
    if (slot + 1 > g_cascs_have_n) g_cascs_have_n = slot + 1;
    // How far this view's own slots sit from each other -- the number that says whether pairing mattered.
    if (slot > 0) {
        for (size_t m = 0; m < cvr::stereo::CascadeMatrixCount; ++m) {
            const float mm = cascs_mm(g_cascs_slot[0] + m * 16, g_cascs_slot[slot] + m * 16);
            if (mm > g_cascs_spread) g_cascs_spread = mm;
        }
    }
}

bool cascs_apply(const void* src, void* dst, int slot) {
    if (g_cascs_have_n <= 0) return false;
    // COVERAGE BEATS PAIRING, and it is measured rather than argued: the report says the second view's own
    // slots hold the same matrix to 0.0 mm, so which of them MAIN receives cannot matter -- while MISSING a
    // slot does matter enormously. The strict version refused 389 uploads in one interval, and a correction
    // that lands on some of MAIN's uploads and not others makes MAIN alternate between two shadow transforms,
    // which is the twitch this whole exercise is chasing. So take the matching slot when it exists and the
    // last filled one otherwise, and count the fallback instead of hiding it.
    if (slot < 0) return false;
    if (slot >= kCascSlots || slot >= g_cascs_have_n) {
        ++g_cascs_miss;
        slot = g_cascs_have_n - 1;
    }
    float mine[kCascMatFloats];
    if (!cascs_read_mat(src, mine)) return false;
    float worst = 0.0f;
    if (!cascs_same_setup(mine, g_cascs_slot[slot], &worst)) {
        if (worst > g_cascs_wskip) g_cascs_wskip = worst;   // so a skip says HOW far off, not just that it was
        CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascSampleSkips)));
        return false;
    }
    cascs_note_pair(mine, g_cascs_slot[slot]);
    if (CyberpunkVR_CascSampleLend >= 2) {
        memcpy(dst, g_cascs_slot_all[slot], kCascSampleBytes);
    } else {
        __try { memcpy(dst, src, kCascSampleBytes); }
        __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
        memcpy(reinterpret_cast<uint8_t*>(dst) + kCascMatFloat * 4, g_cascs_slot[slot],
               sizeof(g_cascs_slot[slot]));
    }
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascSampleLends)));
    if(t_view_side==0)cvr::cascades::MainSampling(reinterpret_cast<const float*>(static_cast<const uint8_t*>(dst)+kCascMatFloat*4),unsigned(cvr::stereo::ActiveCascadeMatrices(g_cascs_slot[slot])));
    if (g_cascs_said.exchange(1) == 0) cascs_report(true);
    return true;
}
}  // namespace

// ---- ONE CASCADE CAMERA FOR BOTH VIEWS, which is where the evidence ended up pointing -------------
//
// The reading in the headset is what settles the direction, and it is worth more than the three lends before it:
// lending only the SAMPLING matrix reproduces the bit-50 artefact exactly. Bit 50 makes a view emit no cascade
// passes at all, so it samples an atlas somebody else rasterised. Handing MAIN a foreign sampling matrix looks
// identical to that, so whatever draws the atlas is aligned with MAIN and not with the second view -- and the
// cascade record this port already equalises ([cascfit] after=0.0000 m) is therefore not what aligns it.
//
// PRECISELY WHAT DRAWS IT, from a capture taken after the fix (cpvr_frame13113): ONE pass per frame, not one
// per view. Forty depth-only DrawIndexedInstanced into the cascade target, events 23735..23879, sitting in the
// gap between the two views' graphs and immediately before MAIN's; the atlas the masks sample is not written in
// that frame at all. So the pass is single and it runs with the LAST cascade camera uploaded before it, which
// is MAIN's -- which is why MAIN's own sampling matrix used to match and the second view's did not. Lending the
// camera moves that single rasterisation onto the second view's frame, and lending the matrix puts both eyes on
// it. "A view samples the atlas it rasterised itself" was the wrong way to say this: there is only one
// rasterisation, and what matters is that the camera and the sampling matrix belong to the SAME view.
//
// What does determine it is the 848-byte CameraShaderConsts the cascade node uploads per cascade, and the
// census has been saying so all along: 848B@RenderShadowCascade M=8844 V=9782 d=18316 -- uploaded twice per
// view per frame, once per cascade, contents differing between the views. Give both views one camera there and
// the two rasterisations become the same picture, which is the only state in which either view's sampling
// matrix can be correct.
//
// Keyed by CASCADE INDEX, not by (node, size). Two cascades upload this block under one node, and pairing them
// by node and size alone would copy cascade 1's camera over cascade 0 -- guaranteed to read as "differs"
// whatever the truth is, which is the note already written against this exact census row.
//
// Whole block, per the request: cameras and matrices alike, nothing left per-view to disagree about.
//
// FAILURE MODE. This block is CameraShaderConsts, and CyberpunkVR_JitterFix above proved that forcing its
// fields across the eyes at the PLAYER passes breaks geometry -- a menu displaced by half a screen. Here it is
// forced only at the cascade node, whose consumers are the shadow rasterisation, so the blast radius should be
// the shadow map alone. If it is not, the recognisable failure is shadows drawn from visibly the wrong place
// or missing outright, not a shifted image; set this to 0.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_CascRenderLend = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugCascRenderLends = 0;

namespace {
constexpr int kCascRenderSlots = 8;

uint8_t  g_cascr[kCascRenderSlots][kCamBlockBytes] = {};
bool     g_cascr_have[kCascRenderSlots] = {};
float    g_cascr_dmax[kCascRenderSlots] = {};
uint64_t g_cascr_pairs = 0;
std::atomic<int> g_cascr_said{0};

bool cascr_block_here(uint32_t size, const void* src) {
    if (!CyberpunkVR_CascRenderLend || size != kCamBlockBytes || !src || !g_exe_base) return false;
    if (t_cascade_idx < 0 || t_cascade_idx >= kCascRenderSlots) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    return t_current_node_work > base &&
           static_cast<uint32_t>(t_current_node_work - base) == CASCADE_NODE_RVA;
}

void cascr_capture(const void* src, int idx) {
    uint8_t all[kCamBlockBytes];
    __try { memcpy(all, src, kCamBlockBytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    memcpy(g_cascr[idx], all, kCamBlockBytes);
    cvr::cascades::Camera(unsigned(idx),all,kCamBlockBytes,true);
    g_cascr_have[idx] = true;
}

void cascr_report(bool first);

bool cascr_apply(const void* src, void* dst, int idx) {
    if (!g_cascr_have[idx]) return false;
    float mine[kCamBlockBytes / 4];
    __try { memcpy(mine, src, kCamBlockBytes); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    // How far apart the two views' cascade cameras were BEFORE the copy -- so a lend that lands on a block
    // that never differed is distinguishable from one that closed a real gap.
    const float* theirs = reinterpret_cast<const float*>(g_cascr[idx]);
    float dmax = 0.0f;
    for (uint32_t i = 0; i < kCamBlockBytes / 4; ++i) {
        const float d = fabsf(mine[i] - theirs[i]);
        if (d > dmax) dmax = d;
    }
    if (dmax > g_cascr_dmax[idx]) g_cascr_dmax[idx] = dmax;
    ++g_cascr_pairs;
    memcpy(dst, g_cascr[idx], kCamBlockBytes);
    cvr::cascades::Camera(unsigned(idx),dst,kCamBlockBytes,false);
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugCascRenderLends)));
    if (g_cascr_said.exchange(1) == 0) cascr_report(true);
    return true;
}

// Same shape as cascs_report: the FIRST landing says so whatever the launcher's DEBUG box says, because a lend
// that never matched a block is otherwise indistinguishable from one that worked. The repeating line stays
// behind the probe.
void cascr_report(bool first) {
    if (!first) {
        if (!CyberpunkVR_CascSampleProbe) return;
        static uint64_t s_last = 0;
        const uint64_t now = GetTickCount64();
        if (s_last && now - s_last < 5000) return;
        s_last = now;
    }
    log("[cascrender] 848B cascade camera, worst per-float gap between the views before the lend: "
        "casc0=%.4f casc1=%.4f casc2=%.4f casc3=%.4f | pairs=%llu lends=%llu",
        g_cascr_dmax[0], g_cascr_dmax[1], g_cascr_dmax[2], g_cascr_dmax[3],
        (unsigned long long)g_cascr_pairs,
        (unsigned long long)CyberpunkVR_DebugCascRenderLends);
    if (first) return;                          // keep the first line's numbers in the interval it reports
    for (int i = 0; i < kCascRenderSlots; ++i) g_cascr_dmax[i] = 0.0f;
    g_cascr_pairs = 0;
}
}  // namespace

static bool looks_like_frame_consts(const void* src) {
    float clock = 0.0f;
    __try { memcpy(&clock, reinterpret_cast<const uint8_t*>(src) + 2 * 4, sizeof(clock)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return clock > 1000.0f;
}

// ---- THE DITHER LOCK, on BOTH uploaders ----------------------------------------------------------
//
// Reopening a question I closed too early. The alpha-test dither of the cascade pass was declared innocent
// on the strength of mode 5, which forces the slice to a constant in every frame-constants block -- but
// mode 5 lives in the BUFFER uploader only, and the cascade node uploads no 480-byte block of its own, so
// which instance of b0 its pixel shader reads was never established. A test that may never have reached its
// target is not a negative result. That is the fourth time today a probe has been narrower than the thing
// it was measuring, and this time the fix is to cover both paths rather than to reason about which one.
//
// AND THE NEW MEASUREMENT MAKES IT MATTER. The depth-target probe says both eyes rasterise the sun cascades
// into the SAME atlas (casc0/casc1: identical DSV descriptors for both views), from a cascade record the
// in-frame diff shows identical. So the eyes already share one shadow map -- there is nothing to hand over,
// which answers "pass the shadows from MAIN to the second view" -- and the only way that shared map can
// still hold different content at the two lighting passes is if the two rasterisations DISAGREE about which
// alpha-tested pixels to keep. Order in a frame is: second view clears, draws, lights; then MAIN clears,
// draws, lights. Each eye samples its own rasterisation. Give them different dither slices and grass -- the
// most finely alpha-tested thing in the scene -- lands differently in the two eyes.
//
// The slices ARE different today, and by construction: [28].x/[28].y were both measured to advance in the
// two views with different values, and mode 4 hands the second view MAIN's block from the PREVIOUS frame
// (the second view runs first), so the mirroring cannot equalise them within a frame.
//
// Locking both to one constant sidesteps the ordering entirely and is the strongest form of the test. Its
// visible cost while on: the noise pattern stops animating, so alpha-tested foliage may show a fixed dither
// texture instead of a shimmer. That is a diagnostic cost, not a shipping one.
//
// RESULT, AND IT ALSO RETRACTS THE REASON FOR BUILDING IT. The counters say the constant uploader carries
// NONE of these blocks -- DitherLockBuf = 465242, DitherLockCb = 0 -- so the 480-byte frame constants travel
// the buffer path only, mode 5 had already covered every instance of them, and my "the test may never have
// reached its target" was wrong. With the slice locked on both paths the eyes still disagree on grass, so
// the cascade pass's alpha-test dither is genuinely innocent, this time on a test that provably reached
// everything.
//
// DEFAULT 0: a locked slice means static noise, which is a real visual cost for no benefit.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_DitherLock = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugDitherLockBuf = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugDitherLockCb = 0;

// Fills `out` (480 bytes) when this upload is a frame-constants block that should ship with the slice
// locked. It carries mode 4/1's mirroring itself: returning through its own buffer bypasses the path
// further down that would otherwise apply it, and the flags fix must not be a casualty of this test.
static bool dither_lock_build(const void* src, void* out) {
    if (!CyberpunkVR_DitherLock || !src || !g_exe_base) return false;
    if (!looks_like_frame_consts(src)) return false;
    if (!sway_time_copy(src, out)) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uint32_t rva = (t_current_node_work > base)
        ? static_cast<uint32_t>(t_current_node_work - base) : 0;
    if (rva == PREPARE_SCENE_NODE_WORK_RVA) {
        if (view_is_main_now()) sway_block_capture(src);
        else if (view_is_vrcam_now()) sway_block_apply(out);
    }
    sway_dither_force(out);
    return true;
}

static bool ReadSinglePassProbe(uintptr_t address,void* out,size_t size) {
    SIZE_T bytes{};
    return address && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),out,size,&bytes) && bytes==size;
}
static void PredictPeerCameraInput(cvr::stereo::trace::Record& row) {
    if(row.node!=0x33DDA4 || !row.inputValid)return;
    const bool paired=(CyberpunkVR_PreparationCameraTrace.load() || cvr::stereo::early_depth::NeedsCamera()) && cvr::stereo::preparation::Pairing() &&
        CyberpunkVR_PreparationPairState.load()==2;
    if(paired ? row.side!=0 : row.side!=1)return;
    row.predictionPeerSide=paired?1u:0u;
    const auto peer=paired?CyberpunkVR_PreparationPairData[3]:g_main_view_ctx.load(std::memory_order_acquire);
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    if(!peer || !base || peer==row.view || (paired && row.view!=CyberpunkVR_PreparationPairData[4])){row.reserved=1;return;}
    if(paired){
        cvr::stereo::CameraInput current,previous,check;
        cvr::stereo::ShaderCameraHistory history,historyCheck;
        float weight{};
        if(!ReadSinglePassProbe(peer+cvr::view_layout::kCamera,current.bytes.data(),current.bytes.size()) ||
            !ReadSinglePassProbe(peer+cvr::view_layout::kHistory,history.bytes.data(),history.bytes.size()) ||
            !ReadSinglePassProbe(peer+cvr::view_layout::kCamera,check.bytes.data(),check.bytes.size()) || check.bytes!=current.bytes ||
            !ReadSinglePassProbe(peer+cvr::view_layout::kHistory,historyCheck.bytes.data(),historyCheck.bytes.size()) || historyCheck.bytes!=history.bytes){row.reserved=13;return;}
        if(current.Read<uint32_t>(0x378)!=row.width || current.Read<uint32_t>(0x37C)!=row.height ||
            std::memcmp(current.bytes.data()+0x20,row.input.data()+0x20,0x28)){row.reserved=14;return;}
        if(!cvr::stereo::DecodeShaderCameraHistory(current,history,previous,weight)){row.reserved=15;return;}
        const auto inverse=reinterpret_cast<cvr::stereo::InvertShaderMatrix>(base+0x297A1C);
        if(!cvr::stereo::BuildPeerShaderCamera(row.camera,current,previous,weight,inverse,row.shaderPrediction)){row.reserved=16;return;}
        row.prediction=current.bytes;row.previousInput=previous.bytes;row.predictionCounter=current.Read<uint32_t>(0x380);
        row.predictionValid=row.previousInputValid=row.shaderPredictionValid=1;
        row.validShaderWords=cvr::stereo::CameraWordMask::All();row.reserved=12;return;
    }
    uint64_t flags{};uint32_t size[2]{},counter{};uint8_t halton[3]{};
    // The first view is prepared. The peer owns its pose, temporal counter and
    // history; none of these may be substituted with the first view's data.
    // 2.5 view context: flags +0x25B8, counter +0x2450, source pose +0x2C50 (2.31: +0x17D8/+0x1660/+0x1E20).
    if(!ReadSinglePassProbe(row.view+0x25B8,&flags,sizeof(flags)) ||
       !ReadSinglePassProbe(peer+0x34,size,sizeof(size)) ||
       !ReadSinglePassProbe(peer+0x2450,&counter,sizeof(counter)) ||
       !ReadSinglePassProbe(peer+0x2C50,row.peerSourcePose.data(),row.peerSourcePose.size()) ||
       !ReadSinglePassProbe(base+0x392C5E8,&halton[0],1) ||
       !ReadSinglePassProbe(base+0x392C620,&halton[1],1) ||
       !ReadSinglePassProbe(base+0x392C658,&halton[2],1)){row.reserved=2;return;}
    // The first validation path is same-resolution DLSS with the native R2
    // sequence. Unsupported AA modes must not borrow a different sequence.
    // Group 70 = DLSS; 72/73/74 and the 2.5 upscalers 76/78 are the others (2.31: 69 and 71-73).
    if(!(flags&0x40) || (flags&0x5700) || halton[0] || halton[1] || halton[2]){row.reserved=3;return;}
    if(size[0]!=row.width || size[1]!=row.height){row.reserved=4;return;}
    const auto jitter=cvr::stereo::PeekR2Jitter(counter,size[0],size[1]);
    // This native routine only rebuilds the supplied camera's numeric matrices.
    // Its input is an owned copy, never the live game's view/camera object.
    auto rebuild=reinterpret_cast<cvr::stereo::RebuildCameraInput>(base+0x296EA0);
    struct Guarded { std::array<uint8_t,32> before; cvr::stereo::CameraInput camera; std::array<uint8_t,32> after; } local{};
    local.before.fill(0xA5);local.after.fill(0xA5);
    // The peer's own window under the asymmetric projection (its input block, as the view producer left it).
    cvr::stereo::PeerProjection peerProjection{};
    const bool ownProjection=cvr::projection::Active() &&
        ReadSinglePassProbe(peer+0x2C78,&peerProjection.aspect,4) && ReadSinglePassProbe(peer+0x2C80,&peerProjection.shiftX,8);
    if(!cvr::stereo::PrepareCameraAtPose(row.input.data(),row.peerSourcePose.data(),jitter,size[0],size[1],rebuild,local.camera,
        ownProjection?&peerProjection:nullptr)){row.reserved=5;return;}
    for(auto b:local.before)if(b!=0xA5){row.reserved=6;return;}
    for(auto b:local.after)if(b!=0xA5){row.reserved=6;return;}
    row.prediction=local.camera.bytes;row.predictionCounter=counter;row.predictionValid=1;
    cvr::stereo::CameraInput previous;
    uint8_t historyValid{};
    if(!ReadSinglePassProbe(peer+cvr::view_layout::kCamera,previous.bytes.data(),previous.bytes.size()) ||
       !ReadSinglePassProbe(peer+cvr::view_layout::kHistory,&historyValid,sizeof(historyValid))){row.reserved=7;return;}
    row.previousInput=previous.bytes;row.previousInputValid=1;
    if(!historyValid){row.reserved=8;return;}
    const auto inverse=reinterpret_cast<cvr::stereo::InvertShaderMatrix>(base+0x297A1C);
    if(cvr::stereo::BuildPeerShaderCamera(row.camera,local.camera,previous,0.0f,inverse,row.shaderPrediction)) {
        // The peer's history weight uses a later per-view frame delta. It is unknown
        // here. Only shaders whose proven read mask excludes it are eligible.
        row.validShaderWords=cvr::stereo::CameraWordMask::All();row.validShaderWords.Exclude(147);
        const uint32_t unavailable=0x7FC00000;std::memcpy(row.shaderPrediction.data()+588,&unavailable,4);
        row.shaderPredictionValid=1;row.reserved=11;
        if(!paired && row.index!=UINT32_MAX && CyberpunkVR_SinglePassTwinUpload.load(std::memory_order_relaxed)) {
            const auto uploaded=cvr::stereo::camera_upload::Prepare(base,row.index,row.camera,row.shaderPrediction);
            row.twinCpuDescriptor=uploaded.cpuDescriptor;row.twinGpuAddress=uploaded.gpuAddress;row.twinBytes=uploaded.bytes;
        }
    } else row.reserved=10;
}

// Observe the bytes actually forwarded, after any existing stereo corrections.
static void TraceCameraUpload(uint32_t index,uint32_t size,const void* src) {
    if(size!=848)return;
    const bool early=cvr::stereo::early_depth::NeedsCamera();
    const bool pairedGeometry=cvr::stereo::paired_geometry::NeedsCamera();
    if(!early && !pairedGeometry && CyberpunkVR_SinglePassTraceState.load(std::memory_order_relaxed)!=1)return;
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    const auto node=base && t_current_node_work>base?static_cast<uint32_t>(t_current_node_work-base):0;
    cvr::stereo::trace::Camera(node,t_view_side,index,src,&PredictPeerCameraInput);
    if(early){cvr::stereo::trace::Record row;row.node=node;row.side=t_view_side;row.index=index;
        std::memcpy(row.camera.data(),src,row.camera.size());cvr::stereo::trace::CaptureNativeInputs(row);
        if(row.inputValid)PredictPeerCameraInput(row);cvr::stereo::early_depth::Camera(row);}
    if(pairedGeometry){cvr::stereo::trace::Record row;row.node=node;row.side=t_view_side;row.index=index;
        std::memcpy(row.camera.data(),src,row.camera.size());cvr::stereo::trace::CaptureNativeInputs(row);
        cvr::stereo::paired_geometry::Camera(row);}
}
// THE ENGINE'S HUD WARP, OFF UNDER THE ASYMMETRIC PROJECTION. The HUD composite bends the HUD about
// each image's own centre (b6 r3.xy: x moves by cy^2*cx*k.x, y by 2*cx^2*cy*k.y, cx/cy from the centre).
// The eyes' windows are mirror images, so one world point sits at another column in each eye and the
// bend moves it by a different amount -- vertically too, which the eyes cannot fuse. Measured on the
// NPC quest marker (1920x946 capture, k = 0.084242): MAIN at cx -0.21, the second view at cx +0.05,
// MAIN 6 px higher near the top and 2 px lower at 62 % height, the warp's own numbers to 0.4 px.
// Zero in both views keeps each in-eye marker on its point. The HUD panel keeps the engine's bend:
// its composite takes the values kept here (HudPanel.cpp).
//
// 2.5, UNDER THE ENGINE'S STEREO, the same block is 432 bytes and the composite (the indirect dispatch after the
// HUD's blur pyramid; markeroutline.rdc event 95215, its b6) runs each eye's half about that eye's own centre:
// the bend as above, and two more terms on each eye's image rather than on the world:
//  - the halo's taps (r7, r8.xy: LOD 1, 2 and 4 of the HUD at uv + k * (2 uv - 1), k -0.005 .. -0.0126) step out
//    from the eye's centre, so one marker's halo sat on another side of it in each eye -- the blurred double of
//    the breach and ripperdoc markers, left in the left eye and right in the right (2026-10-24). Taken at
//    the HUD's own pixel the halo stays round its marker in both.
//  - the shadow (r3.zw, 0.0008 of the target both ways: 4.2 x 2.3 eye pixels down and right) is the black copy of
//    every icon beside it (2026-10-24: the outline sits right of the icon). CyberpunkVR_HudShadowOffset
//    scales it; 0 puts it under the HUD.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_HudFlatInEye = 1;
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_HudFlatInEyeBlocks{0};
extern "C" __declspec(dllexport) float CyberpunkVR_HudShadowOffset = 0.0f;
extern "C" float CyberpunkVR_HudEngineCurvature[2];
static bool FlattenHudComposite(uint32_t size,const void* src,uint8_t* out) {
    const bool engineStereo=size==432 && cvr::engine_stereo::Enabled();
    if(!src || !(engineStereo || (size==512 && cvr::projection::Active()))) return false;
    const float* r=static_cast<const float*>(src);
    const float w=r[16*4+2],h=r[16*4+3];
    if(!(w>=64.0f && w<=16384.0f && h>=64.0f && h<=16384.0f)) return false;
    // The main menu shown in its world (Lobby/MenuLobby.hpp). Its block has no glow, so the HUD's fingerprint below
    // does not take it.
    if(engineStereo && cvr::lobby::Active()) {
        std::memcpy(out,src,size);
        if(cvr::lobby::ComposeWorld(reinterpret_cast<float*>(out))) return true;
    }
    if(!CyberpunkVR_HudFlatInEye) return false;
    if(!hud_cb_block_plausible(static_cast<const uint8_t*>(src),w,h)) return false;
    std::memcpy(out,src,size);
    float* o=reinterpret_cast<float*>(out);
    CyberpunkVR_HudEngineCurvature[0]=o[3*4+0];CyberpunkVR_HudEngineCurvature[1]=o[3*4+1];
    o[3*4+0]=0.0f;o[3*4+1]=0.0f;
    if(engineStereo) {
        for(int i=0;i<4;++i)o[7*4+i]=0.0f;
        o[8*4+0]=0.0f;o[8*4+1]=0.0f;
        o[3*4+2]*=CyberpunkVR_HudShadowOffset;o[3*4+3]*=CyberpunkVR_HudShadowOffset;
    }
    CVR_DIAGNOSTIC(CyberpunkVR_HudFlatInEyeBlocks.fetch_add(1,std::memory_order_relaxed));
    return true;
}
static int64_t ForwardBufUpload(uint32_t idx,uint32_t size,void* src) {
    optics_shadow_snapshot(size,src);
    optics_material_snapshot(size,src);
    cvr::stereo::constants::RegisteredUploadScope constantScope(idx,size,src);
    if(size==848 || size==1584)cvr::stereo::ObserveFogConstants(size,src);
    TraceCameraUpload(idx,size,src);
    cvr::stereo::source_cbv::UploadWriterScope writer(idx,size);
    const auto result=g_orig_buf_upload(idx,size,src);
    writer.CompleteNormalReturn();
    return result;
}
static __int64 ForwardCbUpload(unsigned int size,void* src) {
    optics_shadow_snapshot(size,src);
    optics_material_snapshot(size,src);
    if(size==848 || size==1584)cvr::stereo::ObserveFogConstants(size,src);
    TraceCameraUpload(UINT32_MAX,size,src);
    return g_orig_cb_upload(size,src);
}

// THE CONE AO'S PROJECTION CENTRE, written into its own block. The cone AO (node DrawConeAO) rebuilds view
// positions with its block's symmetric tangents, and nothing else it binds knows the asymmetric projection's
// NDC shift, so its twins (Render/HiddenAreaShaders.cpp, tools/asym_tests/asym_ao_shaders.py) read this view's
// centre from the block itself. The block is 240 bytes on game 2.5 (15 rows; 2.31: 208, 13 rows; measured as the
// node's only upload, on the buffer path); row 12 is a float2 and padding -- 12.z held stale memory such as
// 1e25 in 2.31, different per view, and 2.5's shaders read only 12.xy -- so the
// centre goes there: 12.z = x | y << 16 in signed 1/16384 units (the pixel's NDC, y from the top, so
// shift.x and -shift.y), 12.w = kAoCentreTag. The twins use it only when the tag is there and otherwise
// compute what the engine's shaders do; it is written only while those twins are loaded (the same game
// build) and CyberpunkVR_AsymAoPatch is on, which also switches the correction in the hidden-area twin.
// CyberpunkVR_DebugAsymAoUploads says what the node uploads: [i] = size (| 0x80000000 on the constant
// uploader), [8 + i] = count.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_AsymAoBlockSize = cvr::ssr_merge::kAoBlockBytes;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugAsymAoUploads[16] = {};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugAsymAoBlock[2][64] = {};   // last engine block per view
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugAsymAoPatched[3] = {};  // MAIN, second view, neither
// ENGINE STEREO (render feature 98): the engine runs the cone AO over the side-by-side frame with the node's
// two-halves mode (row 14.y) off, as one picture with the centre's tangents, and the AO thinned out along both
// eyes' inner edges. 1 = turn the mode on with each eye's window and the separation in the block, for the twins'
// stereo recipe (tools/asym_tests/asym_ao_shaders.py STEREO_MAGIC); 0 = the engine's block. Live.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EngineStereoAoHalves{1};
// [0] blocks turned to two halves, [1] left alone (the eyes' tangents in rows 12/13 unusable).
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoAo[2] = {};
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymAoPatch;         // Render/HiddenAreaShaders.cpp
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymAoPatchLoaded;
namespace {
constexpr uint32_t kAoCentreRow = 12;
constexpr uint32_t kAoCentreTag = 0x41734331u;   // asym_ao_shaders.py MAGIC
constexpr uint32_t kAoStereoTag = 0x41735331u;   // asym_ao_shaders.py STEREO_MAGIC
constexpr uint32_t kAoBlockMax = 1024;
bool ao_node_now() {
    if (!g_exe_base) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    return t_current_node_work > base && t_current_node_work - base == CONE_AO_NODE_RVA;
}
void ao_upload_note(uint32_t size, bool cbPath, const void* src) {
    const uint32_t key = size | (cbPath ? 0x80000000u : 0u);
    for (uint32_t i = 0; i < 8; ++i) {
        if (CyberpunkVR_DebugAsymAoUploads[i] == key) { ++CyberpunkVR_DebugAsymAoUploads[8 + i]; break; }
        if (!CyberpunkVR_DebugAsymAoUploads[i]) {
            CyberpunkVR_DebugAsymAoUploads[i] = key; CyberpunkVR_DebugAsymAoUploads[8 + i] = 1; break;
        }
    }
    if (src && (t_view_side == 0 || t_view_side == 1) && size == CyberpunkVR_AsymAoBlockSize) {
        __try { memcpy(CyberpunkVR_DebugAsymAoBlock[t_view_side], src, size < 256 ? size : 256); }
        __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
}
uint32_t ao_fixed(float v) {
    float q = std::round(v * 16384.0f);
    q = q < -32768.0f ? -32768.0f : (q > 32767.0f ? 32767.0f : q);
    return static_cast<uint16_t>(static_cast<int16_t>(q));
}
// True with the block to ship in `out`.
bool ao_centre_write(uint32_t size, const void* src, uint8_t* out) {
    const uint32_t want = CyberpunkVR_AsymAoBlockSize;
    if (!want || size != want || size > kAoBlockMax || size < (kAoCentreRow + 1) * 16 || !src) return false;
    if (!CyberpunkVR_AsymAoPatch.load(std::memory_order_relaxed) ||
            !CyberpunkVR_AsymAoPatchLoaded.load(std::memory_order_relaxed)) return false;
    float c[2] = {0.0f, 0.0f};
    cvr::projection::ViewWindow w{};
    if ((t_view_side == 0 || t_view_side == 1) && cvr::projection::ViewWindowOf(t_view_side, &w) &&
            std::isfinite(w.shiftX) && std::isfinite(w.shiftY)) {
        c[0] = w.shiftX; c[1] = -w.shiftY;
    }
    __try { memcpy(out, src, size); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    const uint32_t packed = ao_fixed(c[0]) | (ao_fixed(c[1]) << 16);
    memcpy(out + kAoCentreRow * 16 + 8, &packed, sizeof(packed));
    memcpy(out + kAoCentreRow * 16 + 12, &kAoCentreTag, sizeof(kAoCentreTag));
    ++CyberpunkVR_DebugAsymAoPatched[(t_view_side == 0 || t_view_side == 1) ? t_view_side : 2];
    // Engine stereo: rows 12 / 13 .xy are the left / right eye's tangents the mode projects with; row 0, which it
    // builds positions with, becomes theirs (mirrored windows: the same). 12.z the left eye's centre, 13.w the
    // right eye's, 12.w kAoStereoTag, 14.y the mode, 14.w the separation. With one shared window both centres
    // are MAIN's.
    if (cvr::engine_stereo::Enabled() && cvr::engine_stereo::Rendering() &&
            CyberpunkVR_EngineStereoAoHalves.load(std::memory_order_relaxed) && size >= 15 * 16) {
        float tl[2], tr[2];
        memcpy(tl, out + 12 * 16, 8);
        memcpy(tr, out + 13 * 16, 8);
        float cl[2] = {c[0], c[1]}, cr[2] = {c[0], c[1]};
        cvr::engine_stereo::EyeProjection eye[2];
        if (cvr::engine_stereo::EyeWindow(0, &eye[0]) && cvr::engine_stereo::EyeWindow(1, &eye[1]) &&
                std::isfinite(eye[0].shiftX + eye[0].shiftY + eye[1].shiftX + eye[1].shiftY)) {
            cl[0] = eye[0].shiftX; cl[1] = -eye[0].shiftY;
            cr[0] = eye[1].shiftX; cr[1] = -eye[1].shiftY;
        }
        const float sep = cvr::engine_stereo::EyeSeparation();
        if (std::isfinite(tl[0] + tl[1] + tr[0] + tr[1]) && tl[0] > 0.0f && tl[1] > 0.0f &&
                std::fabs(tl[0] - tr[0]) < 1e-3f && std::fabs(tl[1] - tr[1]) < 1e-3f) {
            memcpy(out, tl, 8);
            const uint32_t on = 1;
            const uint32_t left = ao_fixed(cl[0]) | (ao_fixed(cl[1]) << 16);
            const uint32_t right = ao_fixed(cr[0]) | (ao_fixed(cr[1]) << 16);
            memcpy(out + 12 * 16 + 8, &left, 4);
            memcpy(out + 12 * 16 + 12, &kAoStereoTag, 4);
            memcpy(out + 13 * 16 + 12, &right, 4);
            memcpy(out + 14 * 16 + 4, &on, 4);
            memcpy(out + 14 * 16 + 12, &sep, 4);
            ++CyberpunkVR_DebugEngineStereoAo[0];
        } else {
            ++CyberpunkVR_DebugEngineStereoAo[1];
        }
    }
    return true;
}
}  // namespace

// THE AUTO-EXPOSURE'S VIEW CENTRE, written into its luminance image's block (node HistogramUpdate). The
// meter samples the scene through a magnifier centred on the middle of the screen; the twins in asym_expo\
// (tools/asym_tests/asym_expo_shaders.py) centre it on the view instead, which is what makes both eyes meter
// the same region around the head's view. They read the centre from row 1.zw of the 24-byte block: z = x | y
// << 16 in signed 1/16384 units (the view's NDC, y down, as for the cone AO), w = kExpoCentreTag. The block
// ends at row 1.y, so on the constant uploader (256-byte slots) the upload grows to 32 bytes; a block that
// arrives on the buffer uploader is left alone. CyberpunkVR_DebugExpoUploads says what the node uploads:
// [i] = size (| 0x80000000 on the constant uploader), [8 + i] = count; CyberpunkVR_DebugExpoCentred counts
// the blocks written for MAIN, the second view and neither.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ExpoCentreBlockSize = 24;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugExpoUploads[16] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoCentred[3] = {};
// The node's last blocks as uploaded by the engine, per view: [side][0] the luminance image's 24 bytes,
// [1] the 28-byte blocks (last of them), [2] the adaptation's 56 bytes. 16 floats each.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugExpoBlocks[2][3][16] = {};
extern "C" std::atomic<uint32_t> CyberpunkVR_ExposureCentre;          // Render/HiddenAreaShaders.cpp
extern "C" std::atomic<uint32_t> CyberpunkVR_ExposureCentreLoaded;
namespace {
constexpr uint32_t kExpoCentreTag = 0x41734531u;   // asym_expo_shaders.py MAGIC
constexpr uint32_t kExpoBlockMax = 256;
bool expo_node_now() {
    if (!g_exe_base) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    return t_current_node_work > base && t_current_node_work - base == HISTOGRAM_UPDATE_NODE_RVA;
}
void expo_upload_note(uint32_t size, bool cbPath, const void* src) {
    const int slot = size == 24 ? 0 : (size == 28 ? 1 : (size == 56 ? 2 : -1));
    if (src && slot >= 0 && (t_view_side == 0 || t_view_side == 1)) {
        __try { memcpy(CyberpunkVR_DebugExpoBlocks[t_view_side][slot], src, size); }
        __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    const uint32_t key = size | (cbPath ? 0x80000000u : 0u);
    for (uint32_t i = 0; i < 8; ++i) {
        if (CyberpunkVR_DebugExpoUploads[i] == key) { ++CyberpunkVR_DebugExpoUploads[8 + i]; break; }
        if (!CyberpunkVR_DebugExpoUploads[i]) {
            CyberpunkVR_DebugExpoUploads[i] = key; CyberpunkVR_DebugExpoUploads[8 + i] = 1; break;
        }
    }
}
// True with the block to ship in `out` and its size in `ship`.
bool expo_centre_write(uint32_t size, const void* src, bool cbPath, uint8_t* out, uint32_t* ship) {
    if (!src || size != CyberpunkVR_ExpoCentreBlockSize || size < 24 || size > kExpoBlockMax) return false;
    if (size < 32 && !cbPath) return false;
    if (!CyberpunkVR_ExposureCentre.load(std::memory_order_relaxed) ||
            !CyberpunkVR_ExposureCentreLoaded.load(std::memory_order_relaxed)) return false;
    float c[2] = {0.0f, 0.0f};
    cvr::projection::ViewWindow w{};
    if ((t_view_side == 0 || t_view_side == 1) && cvr::projection::ViewWindowOf(t_view_side, &w) &&
            std::isfinite(w.shiftX) && std::isfinite(w.shiftY)) {
        c[0] = w.shiftX; c[1] = -w.shiftY;
    }
    *ship = size < 32 ? 32 : size;
    memset(out, 0, *ship);
    __try { memcpy(out, src, size); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    const uint32_t packed = ao_fixed(c[0]) | (ao_fixed(c[1]) << 16);
    memcpy(out + 24, &packed, sizeof(packed));
    memcpy(out + 28, &kExpoCentreTag, sizeof(kExpoCentreTag));
    ++CyberpunkVR_DebugExpoCentred[(t_view_side == 0 || t_view_side == 1) ? t_view_side : 2];
    return true;
}
}  // namespace

// THE AUTO-EXPOSURE'S ENVIRONMENT METER (tools/asym_tests/asym_expo_shaders.py has the reasoning): the light of
// every direction the meter has seen, kept in cells around the player (u16), and the exposure set for the
// light reaching the eye from the hemisphere in front of it, not for the picture. Two blocks of the node:
//  - the luminance image's (df89, after the centre write) grown to 96: row 2 MAIN's lens window (l, r, d, u
//    tangents), rows 3-5 the camera's world right / up / forward, row 3.w kExpoAccTag -- its samples add their
//    light to the cells of their directions;
//  - the adaptation's 56-byte block (b059) grown to 112 (row 6.x the level offset's rate, see
//    CyberpunkVR_ExposureLevelTime): row 3.z kExpoEnvTag, row 3.w dt over the surprise
//    fade's time, row 4 the view's world forward and the cells' write rate, row 5 the confidence fade (time and
//    the player's movement), the view's own part (gain, limit in natural log) and the mode (low byte; bits
//    8-15 the lean towards the view's hemisphere, CyberpunkVR_ExposureHemisphere * 255).
// MAIN alone meters (the second view takes MAIN's exposure, xr_expo_mirror) and has the cells bound; every other
// view's grown blocks carry zeros -- written either way, so a constant slot never keeps a tag from an earlier
// upload.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExposureMemory{1};
// 0 = the environment; diagnostics: 1 = the meter's own reading (the cells still learn), 2 = without the view's own part.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ExposureMemoryMode = 0;
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureMemoryWrite = 0.25f;     // s: a cell in view takes the reading
// A still world does not change for not being looked at: what is remembered fades with the player's movement and
// with a light the view finds changed, time alone only as a safety (20 s made the place collapse to wherever the
// head rested: +24 % over 28 s looking under the overpass, measured 2026-10-16).
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureMemoryAge = 600.0f;      // s: a cell out of view fades...
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureMemoryDistance = 4.0f;   // m: ...and per this many metres moved
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureMemorySurprise = 0.5f;   // s: ...and while the view reads a new light
// s: the engine's meter against the cells' plain mean on the same samples, smoothed this slowly (the level offset).
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureLevelTime = 8.0f;
// 0 = the sphere's mean light (the place); 1 = the view's cosine-weighted hemisphere (the light reaching the eye,
// which under an overpass moves more with a head turn than the engine's meter does).
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureHemisphere = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureGazeGain = 0.15f;        // the view's own difference taken...
extern "C" __declspec(dllexport) float CyberpunkVR_ExposureGazeLimit = 0.3f;        // ...up to this many EV
// [0] MAIN adaptation blocks tagged, [1] other adaptation blocks grown untagged, [2] adaptation blocks on the
// buffer uploader (left alone), [3] the binder refused the cells, [4] cells not allocated, [5] camera
// unreadable, [6] resets (start, a pause, a jump), [7] MAIN luminance-image blocks tagged.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugExpoEnv[8] = {};
// The last MAIN block: forward xyz, write rate, fade, metres moved, dt (s).
extern "C" __declspec(dllexport) float CyberpunkVR_DebugExpoEnvParams[8] = {};
namespace {
constexpr uint32_t kExpoEnvTag = 0x41734532u;   // asym_expo_shaders.py ENV_MAGIC
constexpr uint32_t kExpoAccTag = 0x41734533u;   // asym_expo_shaders.py ENV_ACC_MAGIC
constexpr uint32_t kExpoAdaptBytes = 56, kExpoAdaptEnvBytes = 112, kExpoEnvBytes = 96;
struct ExpoEnvClock { LARGE_INTEGER last{}; float position[3]{}; bool have = false; } g_expo_env;
// The view's scene camera descriptor (Render/ViewCameraLayout.hpp): position in 1/131072 units, orientation
// quaternion (x, y, z, w) at +16; the
// camera's right, forward and up are its +X, +Y and +Z.
bool expo_env_camera(uintptr_t view, float position[3], float forward[3], float right[3] = nullptr, float up[3] = nullptr) {
    if (!view) return false;
    int32_t fixed[3]{};
    float q[4]{};
    __try {
        memcpy(fixed, reinterpret_cast<const void*>(view + cvr::view_layout::kCamera), sizeof(fixed));
        memcpy(q, reinterpret_cast<const void*>(view + cvr::view_layout::kCamera + 16), sizeof(q));
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    float norm = 0.0f;
    for (float v : q) { if (!std::isfinite(v)) return false; norm += v * v; }
    if (norm < 0.98f || norm > 1.02f) return false;
    for (int i = 0; i < 3; ++i) position[i] = float(double(fixed[i]) / 131072.0);
    // q v q^-1 for the camera's axes.
    const float x = q[0], y = q[1], z = q[2], w = q[3];
    forward[0] = 2.0f * (x * y - w * z);
    forward[1] = 1.0f - 2.0f * (x * x + z * z);
    forward[2] = 2.0f * (y * z + w * x);
    if (right) {
        right[0] = 1.0f - 2.0f * (y * y + z * z);
        right[1] = 2.0f * (x * y + w * z);
        right[2] = 2.0f * (x * z - w * y);
    }
    if (up) {
        up[0] = 2.0f * (x * z + w * y);
        up[1] = 2.0f * (y * z - w * x);
        up[2] = 1.0f - 2.0f * (x * x + y * y);
    }
    return true;
}
// The luminance image's block after expo_centre_write (32 bytes in `out`) grown to 96 with its samples' frame:
// MAIN's lens window and camera basis and kExpoAccTag on MAIN with the cells bound, zeros elsewhere.
void expo_acc_extend(uint8_t* out, uint32_t* ship) {
    if (*ship > 32) return;
    memset(out + *ship, 0, kExpoEnvBytes - *ship);
    *ship = kExpoEnvBytes;
    if (t_view_side != 0 || !CyberpunkVR_ExposureMemory.load(std::memory_order_relaxed)) return;
    cvr::projection::ViewWindow w{};
    float position[3]{}, axes[3][3]{};
    if (!cvr::projection::ViewWindowOf(0, &w) ||
            !expo_env_camera(t_current_node_view, position, axes[2], axes[0], axes[1])) { ++CyberpunkVR_DebugExpoEnv[5]; return; }
    if (!cvr::ssr_merge::BindExposureCells(0, CyberpunkVR_DebugExpoEnv)) return;
    // shift = -centre / half span, in tangents
    const float cx = -w.shiftX * w.halfX, cy = -w.shiftY * w.halfY;
    const float window[4] = {cx - w.halfX, cx + w.halfX, cy - w.halfY, cy + w.halfY};
    memcpy(out + 32, window, sizeof(window));
    for (int k = 0; k < 3; ++k) memcpy(out + 48 + 16 * k, axes[k], sizeof(axes[k]));
    memcpy(out + 60, &kExpoAccTag, sizeof(kExpoAccTag));
    ++CyberpunkVR_DebugExpoEnv[7];
}
bool expo_env_write(uint32_t size, const void* src, bool cbPath, uint8_t* out, uint32_t* ship) {
    if (!src || size != kExpoAdaptBytes) return false;
    if (!CyberpunkVR_ExposureCentre.load(std::memory_order_relaxed) ||
            !CyberpunkVR_ExposureCentreLoaded.load(std::memory_order_relaxed)) return false;
    if (!cbPath) { ++CyberpunkVR_DebugExpoEnv[2]; return false; }
    memset(out, 0, kExpoAdaptEnvBytes);
    __try { memcpy(out, src, size); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    *ship = kExpoAdaptEnvBytes;
    if (t_view_side != 0 || !CyberpunkVR_ExposureMemory.load(std::memory_order_relaxed)) { ++CyberpunkVR_DebugExpoEnv[1]; return true; }
    float position[3]{}, forward[3]{};
    if (!expo_env_camera(t_current_node_view, position, forward)) { ++CyberpunkVR_DebugExpoEnv[5]; return true; }
    if (!cvr::ssr_merge::BindExposureCells(0, CyberpunkVR_DebugExpoEnv, true)) return true;
    LARGE_INTEGER now{}, frequency{};
    QueryPerformanceCounter(&now);
    QueryPerformanceFrequency(&frequency);
    auto& clock = g_expo_env;
    const double dt = clock.have ? double(now.QuadPart - clock.last.QuadPart) / double(frequency.QuadPart) : 0.0;
    float moved = 0.0f;
    for (int i = 0; i < 3; ++i) moved += (position[i] - clock.position[i]) * (position[i] - clock.position[i]);
    moved = std::sqrt(moved);
    // A pause (loading, a menu) or a jump (teleport, fast travel) starts the cells afresh.
    const bool reset = !clock.have || !(dt >= 0.0 && dt <= 2.0) || !(moved <= 30.0f);
    clock.last = now;
    memcpy(clock.position, position, sizeof(position));
    clock.have = true;
    const auto positive = [](float v, float fallback) { return std::isfinite(v) && v > 0.0f ? v : fallback; };
    float rate = 1.0f, fade = 0.0f, surprise = 0.0f, levelRate = 1.0f;
    if (reset) {
        moved = 0.0f;
        ++CyberpunkVR_DebugExpoEnv[6];
    } else {
        rate = float(1.0 - std::exp(-dt / positive(CyberpunkVR_ExposureMemoryWrite, 0.25f)));
        fade = float(std::exp(-dt / positive(CyberpunkVR_ExposureMemoryAge, 600.0f) -
                              moved / positive(CyberpunkVR_ExposureMemoryDistance, 4.0f)));
        surprise = float(dt / positive(CyberpunkVR_ExposureMemorySurprise, 0.5f));
        levelRate = float(1.0 - std::exp(-dt / positive(CyberpunkVR_ExposureLevelTime, 8.0f)));
    }
    float gain = CyberpunkVR_ExposureGazeGain, limit = CyberpunkVR_ExposureGazeLimit;
    if (!std::isfinite(gain) || gain < 0.0f) gain = 0.0f;
    if (!std::isfinite(limit) || limit < 0.0f) limit = 0.0f;
    float lean = CyberpunkVR_ExposureHemisphere;
    if (!std::isfinite(lean)) lean = 0.0f;
    const uint32_t mode = (CyberpunkVR_ExposureMemoryMode & 255u) | (uint32_t(std::lround(std::clamp(lean, 0.0f, 1.0f) * 255.0f)) << 8);
    const float row4[4] = {forward[0], forward[1], forward[2], rate};
    float row5[4] = {fade, gain, limit * 0.69314718f, 0.0f};
    memcpy(&row5[3], &mode, sizeof(mode));
    memcpy(out + 56, &kExpoEnvTag, sizeof(kExpoEnvTag));
    memcpy(out + 60, &surprise, sizeof(surprise));
    memcpy(out + 64, row4, sizeof(row4));
    memcpy(out + 80, row5, sizeof(row5));
    memcpy(out + 96, &levelRate, sizeof(levelRate));
    const float debug[8] = {forward[0], forward[1], forward[2], rate, fade, moved, float(dt), levelRate};
    memcpy(CyberpunkVR_DebugExpoEnvParams, debug, sizeof(debug));
    ++CyberpunkVR_DebugExpoEnv[0];
    return true;
}
}  // namespace

// What the reflection nodes upload, as CyberpunkVR_DebugAsymAoUploads for the cone AO: [i] = size
// (| 0x80000000 on the constant uploader, | 0x40000000 in the planar node), [8 + i] = count; the last block of
// the size the merge takes is in CyberpunkVR_DebugSsrBlocks / DebugSsprBlocks once Prepare accepts it.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugSsrUploads[16] = {};
static void ssr_upload_note(uint32_t size, bool cbPath) {
    if (!g_exe_base || t_current_node_work <= reinterpret_cast<uintptr_t>(g_exe_base)) return;
    const auto node = t_current_node_work - reinterpret_cast<uintptr_t>(g_exe_base);
    if (node != SSR_NODE_RVA && node != SSPR_NODE_RVA) return;
    const uint32_t key = size | (cbPath ? 0x80000000u : 0u) | (node == SSPR_NODE_RVA ? 0x40000000u : 0u);
    for (uint32_t i = 0; i < 8; ++i) {
        if (CyberpunkVR_DebugSsrUploads[i] == key) { ++CyberpunkVR_DebugSsrUploads[8 + i]; break; }
        if (!CyberpunkVR_DebugSsrUploads[i]) {
            CyberpunkVR_DebugSsrUploads[i] = key; CyberpunkVR_DebugSsrUploads[8 + i] = 1; break;
        }
    }
}

// Diagnostics: the camera blocks (CameraShaderConsts: 852 bytes in 2.5, uploaded as 864) the uploaders were handed, each with
// the render node that uploaded it and the view side, in a ring (CyberpunkVR_DebugCamBlockNext counts every one).
struct CamBlockNote { uint32_t node; int32_t side; uint32_t seq; uint32_t pad; uint8_t block[864]; };
extern "C" __declspec(dllexport) CamBlockNote CyberpunkVR_DebugCamBlocks[64] = {};
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DebugCamBlockNext{0};
static void cam_block_note(uint32_t size, const void* src) {
    if (size != sizeof(CamBlockNote::block) || !src || !g_exe_base || !cvr::RuntimeDiagnosticsEnabled()) return;
    const uint32_t seq = CyberpunkVR_DebugCamBlockNext.fetch_add(1, std::memory_order_relaxed);
    CamBlockNote& note = CyberpunkVR_DebugCamBlocks[seq % 64];
    note.node = t_current_node_work > reinterpret_cast<uintptr_t>(g_exe_base)
        ? static_cast<uint32_t>(t_current_node_work - reinterpret_cast<uintptr_t>(g_exe_base)) : 0;
    note.side = t_view_side;
    __try { std::memcpy(note.block, src, sizeof(note.block)); } __except (EXCEPTION_EXECUTE_HANDLER) {}
    note.seq = seq + 1;
}

static int64_t __fastcall Detour_BufUpload(uint32_t idx, uint32_t size, void* src) {
    cam_block_note(size, src);
    ssr_upload_note(size, false);
    if (ao_node_now()) {
        ao_upload_note(size, false, src);
        alignas(16) uint8_t centred[kAoBlockMax];
        if (ao_centre_write(size, src, centred)) {
            cvr::ssr_merge::PrepareConeAo(t_view_side, centred, size);   // the eyes' AO as one wide display
            return ForwardBufUpload(idx, size, centred);
        }
    }
    if (expo_node_now()) {
        expo_upload_note(size, false, src);
        alignas(16) uint8_t centred[kExpoBlockMax];
        uint32_t ship = 0;
        if (expo_centre_write(size, src, false, centred, &ship)) return ForwardBufUpload(idx, ship, centred);
        expo_env_write(size, src, false, centred, &ship);   // counts a block it cannot grow here
    }
    if(size==cvr::markers::kRewriteCameraBytes || size==cvr::markers::kRewriteTextBytes) {
        alignas(16) uint8_t markerConstants[cvr::markers::kRewriteScratchBytes];
        uint32_t ship=size;
        if(cvr::markers::RewriteConstants(size,src,markerConstants,&ship))
            return ForwardBufUpload(idx,ship,markerConstants);
    }
    if(size==512 || size==432) {
        alignas(16) uint8_t flat[512];
        if(FlattenHudComposite(size,src,flat)) return ForwardBufUpload(idx,size,flat);
    }
    // The projection jitter, put in step. Keyed on the SIDE, so a reflection-probe face -- which is neither
    // eye -- is left entirely alone.
    if (CyberpunkVR_JitterFix && src && size == kCamBlockBytes) {
        if (t_view_side == 1) {
            jitter_capture(src);
        } else if (t_view_side == 0) {
            uint8_t jittered[kCamBlockBytes];
            if (jitter_apply(src, jittered)) return ForwardBufUpload(idx, size, jittered);
        }
    }
    if (CyberpunkVR_WideCensus && cvr::RuntimeDiagnosticsEnabled()) { wide_note(src, size, t_view_side); wide_report(); temporal_report(); }
    if (fog_block_here(size, src)) {
        if (view_is_vrcam_now()) {
            fog_jitter_capture(src);
        } else if (view_is_main_now()) {
            uint8_t fogged[384];
            if (fog_jitter_apply(src, fogged)) return ForwardBufUpload(idx, size, fogged);
        }
    }
    // The cascade sampling matrix, on THIS uploader and on the other one: which of the two carries a given
    // block is not something to assume, and assuming it is what made several probes above look like negative
    // results. Keyed on the SIDE, so a reflection probe -- which is neither eye -- is left alone entirely.
    if (cascs_block_here(size, src) && (t_view_side == 0 || t_view_side == 1)) {
        const int slot = cascs_ordinal(t_view_side);
        if (t_view_side == 1) {
            cascs_capture(src, slot);
        } else {
            uint8_t lent[kCascSampleBytes];
            if (cascs_apply(src, lent, slot)) {
                cascs_report(false);
                return ForwardBufUpload(idx, size, lent);
            }
        }
        cascs_report(false);
    }
    // One cascade camera for both views, keyed by cascade index -- see the note above cascr_block_here.
    if (cascr_block_here(size, src) && (t_view_side == 0 || t_view_side == 1)) {
        if (t_view_side == 1) {
            cascr_capture(src, t_cascade_idx);
        } else {
            uint8_t camlent[kCamBlockBytes];
            if (cascr_apply(src, camlent, t_cascade_idx)) {
                cascr_report(false);
                return ForwardBufUpload(idx, size, camlent);
            }
        }
        cascr_report(false);
    }
    // The checkerboard phase, first and cheapest: one integer compare, and the block is small.
    if (CyberpunkVR_CheckerProbe && src) {
        checker_small_note(size, t_vrcam_node_active);
        checker_small_report();
    }
    if ((CyberpunkVR_CheckerPhaseFix || CyberpunkVR_CheckerProbe) && src &&
            size == CyberpunkVR_CheckerSize) {
        uint8_t phased[64];
        if (checker_note(src, phased, t_vrcam_node_active, size))
            return ForwardBufUpload(idx, size, phased);
    }
    // Mode 5: the dither slice, forced in EVERY frame-constants block regardless of which node uploaded
    // it, so the instance the cascade pass reads cannot be missed. Returns through its own buffer.
    if (CyberpunkVR_SwayTimeFix == 5 && size == 480 && src && g_exe_base &&
            looks_like_frame_consts(src)) {
        uint8_t forced[480];
        if (sway_time_copy(src, forced)) {
            // Mode 4's mirroring still happens for the block the flags depend on -- forcing the dither
            // must not cost the fix that is already known to work.
            const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
            const uint32_t rva = (t_current_node_work > base)
                ? static_cast<uint32_t>(t_current_node_work - base) : 0;
            if (rva == PREPARE_SCENE_NODE_WORK_RVA) {
                if (view_is_main_now()) sway_block_capture(src);
                else if (view_is_vrcam_now()) sway_block_apply(forced);
            }
            sway_dither_force(forced);
            CVR_DIAGNOSTIC(InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugSwayTimeFills)));
            return ForwardBufUpload(idx, size, forced);
        }
    }
    if (size == 480) {
        uint8_t locked[480];
        if (dither_lock_build(src, locked)) {
            CVR_DIAGNOSTIC(InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugDitherLockBuf)));
            return ForwardBufUpload(idx, size, locked);
        }
    }
    // The retargetable diff, first: it must see the engine's own data, and it must be able to look at
    // any (node, size) without a rebuild.
    if (CyberpunkVR_BlockDiff && src && g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uint32_t rva = (t_current_node_work > base)
            ? static_cast<uint32_t>(t_current_node_work - base) : 0;
        if (CyberpunkVR_BlockDiffNode == 0) {
            // Which nodes upload this size.
            if (size == CyberpunkVR_BlockDiffSize) {
                blk_key_note(rva, t_vrcam_node_active);
                blk_keys_report();
            }
        } else if (rva == CyberpunkVR_BlockDiffNode) {
            // Which sizes this node uploads, and the full diff for the one named.
            blk_key_note(size, t_vrcam_node_active);
            blk_keys_report();
            if (size == CyberpunkVR_BlockDiffSize &&
                    (size != 384 || fog_block_is_player_view(src)))
                blk_note(src, size, t_view_side);
        }
    }
    // The grid origin: watched on its own because the block it lives in is uploaded two dozen times a
    // frame and cannot be compared wholesale, and optionally moved out of the world so the discard it
    // addresses can never fire.
    if ((CyberpunkVR_GridProbe || CyberpunkVR_MaskKill) && src && g_exe_base && size == 848) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uint32_t rva = (t_current_node_work > base)
            ? static_cast<uint32_t>(t_current_node_work - base) : 0;
        if (rva == CASCADE_NODE_RVA) {
            if (CyberpunkVR_GridProbe) {
                grid_note(src, t_vrcam_node_active);
                casc_note(src, t_vrcam_node_active, t_cascade_idx);
                if (t_cascade_idx >= 0) casc_stack_report();
            }
            if (CyberpunkVR_MaskKill) {
                uint8_t killed[848];
                __try {
                    memcpy(killed, src, sizeof(killed));
                    const float far_away = CyberpunkVR_MaskKillOrigin;
                    memcpy(killed + 144 * 4, &far_away, sizeof(far_away));
                    memcpy(killed + 145 * 4, &far_away, sizeof(far_away));
                } __except (EXCEPTION_EXECUTE_HANDLER) { return ForwardBufUpload(idx, size, src); }
                CVR_DIAGNOSTIC(InterlockedIncrement64(
                    reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugMaskKills)));
                return ForwardBufUpload(idx, size, killed);
            }
        }
    }
    // The clock, lent from MAIN's block to the second view's. Gated on the node as well as the size,
    // because "480 bytes in the second eye" is a dozen unrelated blocks while "480 bytes from the node
    // that owns the frame constants" is the one.
    if ((CyberpunkVR_SwayTimeFix || CyberpunkVR_SwayDiff) && size == 480 && src && g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uint32_t rva = (t_current_node_work > base)
            ? static_cast<uint32_t>(t_current_node_work - base) : 0;
        if (rva == PREPARE_SCENE_NODE_WORK_RVA) {
            // Measured BEFORE the patch below, so this reports what the engine itself authored.
            if (CyberpunkVR_SwayDiff) sway_diff_note(src, t_vrcam_node_active);
            uint8_t patched[480];
            if (CyberpunkVR_SwayTimeFix == 2) {
                // One set of values per frame, for BOTH views -- MAIN's block is patched too, which is
                // the whole point: equality cannot depend on which view happens to upload first.
                uint32_t frame_id = 0;
                __try {
                    const uintptr_t renderer =
                        *reinterpret_cast<uintptr_t*>(g_exe_base + RENDERER_GLOBAL_RVA);
                    if (renderer) frame_id = *reinterpret_cast<uint32_t*>(renderer + 0x4CC8);
                } __except (EXCEPTION_EXECUTE_HANDLER) { frame_id = 0; }
                if (CyberpunkVR_SwayDiff) {
                    uint32_t authored = 0;
                    __try {
                        memcpy(&authored, reinterpret_cast<const uint8_t*>(src) + 113 * 4,
                               sizeof(authored));
                    } __except (EXCEPTION_EXECUTE_HANDLER) { authored = 0; }
                    sway_order_note(t_vrcam_node_active, frame_id, authored);
                }
                if (frame_id &&
                        sway_frame_apply(src, patched, t_vrcam_node_active, frame_id)) {
                    if (CyberpunkVR_SwayDiff) {
                        sway_shipped_note(patched, t_vrcam_node_active);
                        sway_shipped_report();
                    }
                    return ForwardBufUpload(idx, size, patched);
                }
                // Fell through: nothing held yet, so the engine's own block ships. Recorded as such,
                // or a stalled counter in the log would read as "the patch is working".
                if (CyberpunkVR_SwayDiff) {
                    sway_shipped_note(src, t_vrcam_node_active);
                    sway_shipped_report();
                }
            } else if (CyberpunkVR_SwayTimeFix == 4) {
                if (view_is_main_now()) {
                    sway_block_capture(src);
                } else if (view_is_vrcam_now() && sway_block_apply(patched)) {
                    if (CyberpunkVR_SwayDiff) {
                        sway_shipped_note(patched, t_vrcam_node_active);
                        sway_shipped_report();
                    }
                    return ForwardBufUpload(idx, size, patched);
                }
            } else if (CyberpunkVR_SwayTimeFix == 1 || CyberpunkVR_SwayTimeFix == 3) {
                const bool force = (CyberpunkVR_SwayTimeFix == 3);
                bool shipped = false;
                if (view_is_main_now()) {
                    sway_time_capture(src);
                    // MAIN is patched only in mode 3, and then only in the dither pair: the point is
                    // that both eyes get the SAME slice without depending on who uploads first.
                    if (force && sway_time_copy(src, patched)) {
                        sway_dither_force(patched);
                        shipped = true;
                    }
                } else if (view_is_vrcam_now() && sway_time_apply(src, patched)) {
                    if (force) sway_dither_force(patched);
                    shipped = true;
                }
                if (shipped) {
                    if (CyberpunkVR_SwayDiff) {
                        sway_shipped_note(patched, t_vrcam_node_active);
                        sway_shipped_report();
                    }
                    return ForwardBufUpload(idx, size, patched);
                }
            }
        }
    }
    return ForwardBufUpload(idx, size, src);
}

// Declared here because the upload detour below counts into them; they are DEFINED beside the
// mirror further down, next to the offsets they explain.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugGradeMainUploads;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugGradeVrcamUploads;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugGradeOtherView;
CVR_DETOUR("[sway] buffer uploader sub_140334328", BUF_UPLOAD_RVA, Detour_BufUpload, g_orig_buf_upload)

// The pixel stage's b6 uploader (sub_14041A56C: allocate, copy, bind as PS b6). In the shadow mask node its
// block is the contact-shadow settings, whose padding carries the wide merge's stamp and tag
// (Render/SsrWideMerge.cpp, kShadowMask).
using PsB6UploadFn = void(__fastcall*)(size_t, void*);
static PsB6UploadFn g_orig_ps_b6_upload = nullptr;
static void __fastcall Detour_PsB6Upload(size_t size, void* src) {
    if (size == cvr::ssr_merge::kShadowBlockBytes && src && g_exe_base &&
            t_current_node_work == reinterpret_cast<uintptr_t>(g_exe_base) + RENDER_SHADOWMASK_RVA) {
        alignas(16) uint8_t block[cvr::ssr_merge::kShadowBlockBytes];
        bool copied = true;
        __try { memcpy(block, src, sizeof(block)); }
        __except (EXCEPTION_EXECUTE_HANDLER) { copied = false; }
        if (copied && cvr::ssr_merge::PrepareShadowMask(t_view_side, t_current_node_view, block, sizeof(block)))
            return g_orig_ps_b6_upload(size, block);
    }
    g_orig_ps_b6_upload(size, src);
}
CVR_DETOUR("[wide-merge] pixel-stage b6 uploader sub_14041A56C", PS_B6_UPLOAD_RVA, Detour_PsB6Upload, g_orig_ps_b6_upload)

using BindCameraFn=__int64(__fastcall*)(uint32_t,uint32_t,uint32_t);
static BindCameraFn g_original_bind_camera{};
static __int64 __fastcall BindCameraProbe(uint32_t binding,uint32_t handle,uint32_t stages) {
    const auto result=g_original_bind_camera(binding,handle,stages);
    for(const auto& annotation:cvr::stereo::source_cbv::ObserveBindMask(binding,handle,stages))
        cvr::stereo::descriptors::Cbv(&annotation.view,{annotation.cpu},&annotation.origin);
    cvr::stereo::early_depth::BoundCamera(binding,handle,stages);
    cvr::stereo::paired_geometry::BoundCamera(t_view_side,binding,handle,stages);
    if(CyberpunkVR_EarlyDepthNativeCamera.load(std::memory_order_relaxed) || cvr::stereo::paired_geometry::NeedsCamera())
        cvr::stereo::camera_upload::ObserveNativeBind(reinterpret_cast<uintptr_t>(g_exe_base),binding,handle,stages);
    if(CyberpunkVR_SinglePassTwinUpload.load(std::memory_order_relaxed)) {
        const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
        cvr::stereo::camera_upload::AfterNativeBind(base,binding,handle,stages,t_view_side==1 && t_current_node_work==base+0x33DDA4);
    }
    return result;
}
static bool WantNativeCameraProbe(){return cvr::stereo::native_probe::Enabled() || cvr::stereo::families::Enabled();}
CVR_DETOUR_IF("[single-pass] bounded camera binding probe",0x337968,BindCameraProbe,g_original_bind_camera,WantNativeCameraProbe)

using BindMaterialFn=__int64(__fastcall*)(uint32_t,uint32_t,uint8_t);
static BindMaterialFn g_original_bind_material{};
static __int64 __fastcall BindMaterialProbe(uint32_t binding,uint32_t handle,uint8_t stage){
    const auto result=g_original_bind_material(binding,handle,stage);
    cvr::stereo::material_binding::ObserveBind(reinterpret_cast<uintptr_t>(g_exe_base),binding,handle,stage);
    for(const auto& annotation:cvr::stereo::source_cbv::ObserveBindStage(binding,handle,stage))
        cvr::stereo::descriptors::Cbv(&annotation.view,{annotation.cpu},&annotation.origin);
    return result;
}
static bool WantNativeMaterialProbe(){
    return WantNativeCameraProbe() && cvr::stereo::material_binding::Compatible(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));
}
CVR_DETOUR_IF("[single-pass] native material binding",0x336B54,BindMaterialProbe,g_original_bind_material,WantNativeMaterialProbe)
static bool WantNativeSourceSrv(){
    const bool wanted=GetEnvironmentVariableW(L"CVR_STEREO_SOURCE_SELECTORS",nullptr,0) &&
        cvr::stereo::native_probe::ObserveBindings() && cvr::stereo::families::Enabled() &&
        cvr::stereo::source_srv::Compatible() && cvr::stereo::source_srv::ConfigureXstate();
    return wanted;
}
CVR_DETOUR_IF("[single-pass] current buffer SRV",0x338FC0,CvrSourceSrvIndividualDetour,CvrSourceSrvIndividualOriginal,WantNativeSourceSrv)
CVR_DETOUR_IF("[single-pass] current cached SRV group",0x338ED4,CvrSourceSrvCachedDetour,CvrSourceSrvCachedOriginal,WantNativeSourceSrv)

// Fresh native callers 0x23AEA2 and 0x229D4C1 both pass renderer,
// render arguments and a [begin,end) span of 16-byte sorted packets.
using RenderPacketsFn=__int64(__fastcall*)(void*,const void*,const void*);
static RenderPacketsFn g_original_render_packets{};
static __int64 __fastcall RenderPacketsInner(void* renderer,const void* arguments,const void* span);
extern "C" void CyberpunkVR_WaitDeferredPresentForPackets();   // Stereo/SubmitChain.cpp
// Single-pass stage replay wraps the whole consumer, including the probes below, so a target
// stage it replays skips them together with the native consumer (Render/StageReplay.cpp).
static __int64 __fastcall RenderPacketsProbe(void* renderer,const void* arguments,const void* span) {
    CyberpunkVR_WaitDeferredPresentForPackets();
    if(cvr::stereo::stage_replay::Active()) {
        const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
        const auto node=base && t_current_node_work>base?uint32_t(t_current_node_work-base):0;
        return cvr::stereo::stage_replay::Consume(t_view_side,node,renderer,arguments,span,&RenderPacketsInner);
    }
    return RenderPacketsInner(renderer,arguments,span);
}
static __int64 __fastcall RenderPacketsInner(void* renderer,const void* arguments,const void* span) {
    const bool group=CyberpunkVR_StereoGpuProbeGroup.load(std::memory_order_relaxed)!=0;
    if(CyberpunkVR_GeometryPacketState.load(std::memory_order_relaxed)!=1 && !group && !cvr::stereo::preparation::Pairing())
        return g_original_render_packets(renderer,arguments,span);
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    const auto node=base && t_current_node_work>base?uint32_t(t_current_node_work-base):0;
    const auto ticket=cvr::stereo::packets::Begin(node,t_view_side,renderer,arguments,span);
    cvr::stereo::preparation::DrawScope drawScope(node,t_view_side,arguments);
    cvr::stereo::gpu_probe::GroupContext previous;
    if(group) {
        uint32_t plane=UINT_MAX;SIZE_T read{};
        if(arguments && node==0x33DDA4)ReadProcessMemory(GetCurrentProcess(),static_cast<const uint8_t*>(arguments)+0x10,&plane,4,&read);
        previous=cvr::stereo::gpu_probe::BeginGroup(read==4?t_view_side:-1,plane);
    }
    const auto early=cvr::stereo::early_depth::Begin(node,t_view_side,renderer,arguments,span);
    const auto result=g_original_render_packets(renderer,arguments,span);
    cvr::stereo::early_depth::End(early);
    if(group)cvr::stereo::gpu_probe::EndGroup(previous);
    cvr::stereo::packets::End(ticket);
    return result;
}
static bool WantNativePacketsProbe() {
    // CPU packet observation must not depend on the three hand-picked GPU
    // shader variants. Recording still takes the disabled fast path by default.
    // 2.5 prologue (sub_140334CC0): mov rax,rsp / mov [rax+10h],rbx / mov [rax+18h],r8 / mov [rax+8],rcx.
    const uint8_t expected[]{0x48,0x8B,0xC4,0x48,0x89,0x58,0x10,0x4C,0x89,0x40,0x18,0x48,0x89,0x48,0x08};
    uint8_t actual[sizeof(expected)]{};SIZE_T read{};
    const auto address=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr))+0x334CC0;
    const bool compatible=ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),actual,sizeof(actual),&read) &&
        read==sizeof(actual) && std::memcmp(actual,expected,sizeof(actual))==0;
    CyberpunkVR_GeometryPacketHookCompatible.store(compatible?1u:2u,std::memory_order_release);
    return compatible;
}
CVR_DETOUR_IF("[single-pass] bounded native geometry packets",0x334CC0,RenderPacketsProbe,g_original_render_packets,WantNativePacketsProbe)

static __int64 __fastcall Detour_CbUpload(unsigned int size, void* src) {
    if (size == 512 || size == 432) {
        alignas(16) uint8_t flat[512];
        if (FlattenHudComposite(size, src, flat)) return ForwardCbUpload(size, flat);
    }
    // The SSR and planar-reflection traces' block, grown by the rows their wide-merge twins read
    // (Render/SsrWideMerge.cpp).
    ssr_upload_note(size, true);
    cam_block_note(size, src);
    if (size == cvr::ssr_merge::kEngineBlockBytes && g_exe_base) {
        const auto node = t_current_node_work - reinterpret_cast<uintptr_t>(g_exe_base);
        if (node == SSR_NODE_RVA || node == SSPR_NODE_RVA) {
            alignas(16) uint8_t grown[cvr::ssr_merge::kBlockBytes];
            uint32_t ship = 0;
            const auto channel = node == SSR_NODE_RVA ? cvr::ssr_merge::kReflections : cvr::ssr_merge::kPlanar;
            if (cvr::ssr_merge::Prepare(channel, t_view_side, t_current_node_view, src, size, grown, &ship))
                return ForwardCbUpload(ship, grown);
        }
    }
    if (ao_node_now()) {
        ao_upload_note(size, true, src);
        alignas(16) uint8_t centred[kAoBlockMax];
        if (ao_centre_write(size, src, centred)) {
            cvr::ssr_merge::PrepareConeAo(t_view_side, centred, size);
            return ForwardCbUpload(size, centred);
        }
    }
    if (expo_node_now()) {
        expo_upload_note(size, true, src);
        alignas(16) uint8_t centred[kExpoBlockMax];
        uint32_t ship = 0;
        if (expo_centre_write(size, src, true, centred, &ship)) {
            expo_acc_extend(centred, &ship);
            return ForwardCbUpload(ship, centred);
        }
        if (expo_env_write(size, src, true, centred, &ship)) return ForwardCbUpload(ship, centred);
    }
    if(size==cvr::markers::kRewriteCameraBytes || size==cvr::markers::kRewriteTextBytes) {
        alignas(16) uint8_t markerConstants[cvr::markers::kRewriteScratchBytes];
        uint32_t ship=size;
        if(cvr::markers::RewriteConstants(size,src,markerConstants,&ship))
            return ForwardCbUpload(ship,markerConstants);
    }
    // The projection jitter, put in step. Keyed on the SIDE, so a reflection-probe face -- which is neither
    // eye -- is left entirely alone.
    if (CyberpunkVR_JitterFix && src && size == kCamBlockBytes) {
        if (t_view_side == 1) {
            jitter_capture(src);
        } else if (t_view_side == 0) {
            uint8_t jittered[kCamBlockBytes];
            if (jitter_apply(src, jittered)) return ForwardCbUpload(size, jittered);
        }
    }
    if (CyberpunkVR_WideCensus && cvr::RuntimeDiagnosticsEnabled()) { wide_note(src, size, t_view_side); wide_report(); temporal_report(); }
    if (fog_block_here(size, src)) {
        if (view_is_vrcam_now()) {
            fog_jitter_capture(src);
        } else if (view_is_main_now()) {
            uint8_t fogged[384];
            if (fog_jitter_apply(src, fogged)) return ForwardCbUpload(size, fogged);
        }
    }
    // The cascade sampling matrix, on this uploader too -- see the note on the buffer path.
    if (cascs_block_here(size, src) && (t_view_side == 0 || t_view_side == 1)) {
        const int slot = cascs_ordinal(t_view_side);
        if (t_view_side == 1) {
            cascs_capture(src, slot);
        } else {
            uint8_t lent[kCascSampleBytes];
            if (cascs_apply(src, lent, slot)) {
                cascs_report(false);
                return ForwardCbUpload(size, lent);
            }
        }
        cascs_report(false);
    }
    // One cascade camera for both views, on this uploader too.
    if (cascr_block_here(size, src) && (t_view_side == 0 || t_view_side == 1)) {
        if (t_view_side == 1) {
            cascr_capture(src, t_cascade_idx);
        } else {
            uint8_t camlent[kCamBlockBytes];
            if (cascr_apply(src, camlent, t_cascade_idx)) {
                cascr_report(false);
                return ForwardCbUpload(size, camlent);
            }
        }
        cascr_report(false);
    }
    // The field-level diff, on THIS uploader too. Which of the two carries a given block is not something to
    // assume: the wide census sees a row on both paths, the diff used to see only one, and that asymmetry is
    // what made several silences look like negative results today.
    if (CyberpunkVR_BlockDiff && src && g_exe_base) {
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        const uint32_t rva = (t_current_node_work > base)
            ? static_cast<uint32_t>(t_current_node_work - base) : 0;
        // Same content filter as the fix: a 384-byte block under the fog node is either the player's view
        // or a reflection-probe face, and comparing one against the other is what produced "49 of 96 differ".
        if (rva == CyberpunkVR_BlockDiffNode && size == CyberpunkVR_BlockDiffSize &&
                (size != 384 || fog_block_is_player_view(src)))
            blk_note(src, size, t_view_side);
    }
    // The same two things as on the buffer uploader: enumerate the small sizes, and hold the
    // checkerboard phase if this is the block that carries it. Which uploader b6 arrives on is not
    // something to assume -- the 56-byte probe on the other one printed nothing.
    if (CyberpunkVR_CheckerProbe && src) {
        checker_small_note(size, t_vrcam_node_active);
        checker_small_report();
    }
    if ((CyberpunkVR_CheckerPhaseFix || CyberpunkVR_CheckerProbe) && src &&
            size == CyberpunkVR_CheckerSize) {
        uint8_t phased[64];
        if (checker_note(src, phased, t_vrcam_node_active, size))
            return ForwardCbUpload(size, phased);
    }
    // The dither lock, on THIS uploader too. It has to be ahead of the grading early-out, which returns for
    // everything that is not the tonemap block -- and it is the whole point of the exercise: the buffer-only
    // version of this test may never have reached the b0 instance the cascade pass reads.
    if (size == 480) {
        uint8_t locked[480];
        if (dither_lock_build(src, locked)) {
            CVR_DIAGNOSTIC(InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugDitherLockCb)));
            return ForwardCbUpload(size, locked);
        }
    }
    if (!grade_up_is_target(size, src)) return ForwardCbUpload(size, src);
    optics_grade_snapshot(src);
    // WHOSE UPLOAD THIS IS, BY THE VIRTUAL-CAMERA NAME HASH. The test used to be
    // `t_vrcam_node_active ? 1 : 0`, whose else-branch called EVERYTHING that is not the second
    // view "MAIN" -- including the flat 960x540 desktop window, which uploads a grading block of
    // its own. So g_gcu[0] was not MAIN's block but whichever of those uploaded last.
    //
    // Measured 2026-08-30 with the debugger, standing in the braindance with the green cast plainly
    // on MAIN's screen, reading g_gcu[0] across a single run/pause cycle and nothing else changing:
    //
    //     sample 1   +010 1.0       +024 1.0   +028 1.0   +04C 0.0     +074 1.0
    //     sample 2   +010 1.04167   +024 1.3   +028 1.3   +04C 0.435   +074 1.3
    //
    // The second is the braindance grading, the first is the identity. One buffer, one scene, two
    // different sources -- which is why the mirror sometimes had nothing to copy and the eye never
    // settled. It is the same defect that was found in the exposure buffer earlier the same day.
    //
    // A view we cannot name, and any third view, is passed through untouched: vanilla grading for
    // that upload, which is a degradation and not a refusal.
    if (!t_active_view_known) return ForwardCbUpload(size, src);
    const uint64_t vrcam_key = g_vrcam_ctx_key.load(std::memory_order_acquire);
    const bool grade_is_vrcam = (vrcam_key != 0 && t_active_view_key == vrcam_key);
    const bool grade_is_main  = (t_active_view_key == 0);
    if(t_active_view_key==cvr::cname_hash("vr_optics_feed")) {
        // Same LUT inputs as VRCAM. Never publish a scope upload as MAIN.
        if(CyberpunkVR_OpticsGradeMirror && CyberpunkVR_GradeMirrorMask && g_gcu_seen[0])
            return grade_up_mirror_call(size,src);
        return ForwardCbUpload(size,src);
    }
    if (!grade_is_vrcam && !grade_is_main) {
        CVR_DIAGNOSTIC(InterlockedIncrement64(
            reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugGradeOtherView)));
        return ForwardCbUpload(size, src);
    }
    const int v = grade_is_vrcam ? 1 : 0;
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
        v ? &CyberpunkVR_DebugGradeVrcamUploads : &CyberpunkVR_DebugGradeMainUploads)));
    if (v == 0) {                                   // MAIN: this is the reference block
        const bool ok = grade_up_capture(src, 0);
        const __int64 r = ForwardCbUpload(size, src);
        if (ok && CyberpunkVR_GradeUpProbe) grade_up_report();
        return r;
    }
    // VRCAM: record what it WOULD have uploaded, then substitute the chosen dwords.
    if (CyberpunkVR_GradeUpProbe) grade_up_capture(src, 1);
    if (!CyberpunkVR_GradeMirrorMask || !g_gcu_seen[0]) return ForwardCbUpload(size, src);
    return grade_up_mirror_call(size, src);
}

using GradingComposeFn = void (__fastcall*)(void*, void*, uint8_t, float, void*, uint8_t*);
static GradingComposeFn g_orig_grading_compose = nullptr;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_GradingProbe = 1;   // OFF: composed grading block dump (scanner tint, parked)
static uint8_t  g_grade_out[2][40];
static uint8_t  g_grade_a3[2]  = {0, 0};
static float    g_grade_a4[2]  = {0.f, 0.f};
static uint8_t  g_grade_a6[2]  = {0, 0};
static bool     g_grade_seen[2] = {false, false};
static std::mutex g_grade_mtx;

static void grading_probe_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 4000) return;
    uint8_t out[2][40], a3[2], a6[2];
    float a4[2];
    {
        std::lock_guard<std::mutex> lk(g_grade_mtx);
        if (!g_grade_seen[0] || !g_grade_seen[1]) return;
        memcpy(out, g_grade_out, sizeof(out));
        memcpy(a3, g_grade_a3, sizeof(a3));
        memcpy(a4, g_grade_a4, sizeof(a4));
        memcpy(a6, g_grade_a6, sizeof(a6));
    }
    s_last = now;
    char line[900];
    int u = 0;
    line[0] = 0;
    for (int o = 0; o < 40; o += 4) {
        uint32_t m, v;
        memcpy(&m, out[0] + o, 4);
        memcpy(&v, out[1] + o, 4);
        if (m == v) continue;
        float fm, fv;
        memcpy(&fm, &m, 4);
        memcpy(&fv, &v, 4);
        if (u < static_cast<int>(sizeof(line)) - 90)
            u += snprintf(line + u, sizeof(line) - u,
                          "+%02X M=%08X(%.5g) V=%08X(%.5g)  ", o, m, fm, v, fv);
    }
    log("[grade] a3 M=%u V=%u | a4 M=%.5g V=%.5g | a6 M=%u V=%u | differing dwords: %s",
        a3[0], a3[1], a4[0], a4[1], a6[0], a6[1], u ? line : "(none)");
}

static bool grading_snapshot(void* a5, uint8_t* a6, uint8_t a3, float a4, int v) {
    __try {
        std::lock_guard<std::mutex>* dummy = nullptr; (void)dummy;
        memcpy(g_grade_out[v], a5, 40);
        g_grade_a6[v] = a6 ? *a6 : 0;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    g_grade_a3[v] = a3;
    g_grade_a4[v] = a4;
    g_grade_seen[v] = true;
    return true;
}

static void __fastcall Detour_GradingCompose(void* a1, void* a2, uint8_t a3, float a4,
                                             void* a5, uint8_t* a6) {
    g_orig_grading_compose(a1, a2, a3, a4, a5, a6);
    if (!CyberpunkVR_GradingProbe || !a5) return;
    const int v = t_vrcam_node_active ? 1 : 0;
    {
        std::lock_guard<std::mutex> lk(g_grade_mtx);
        if (!grading_snapshot(a5, a6, a3, a4, v)) return;
    }
    grading_probe_report();
}

// Live-settable so the field can be A/B-ed without a rebuild. 0x640 = viewData+1600, the
// grading-settings pointer sub_140C944A4 reads EVERYTHING out of (+224..+248). 0xF88 was
// the first guess and is measurably not it: lending it fired 6776 times and changed
// nothing, because sub_140C944A4 does `if (!v9) a4 = -1.0f` and throws that float away.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GradingSrcOff = 0x640;
using TonemapLutFn = __int64(__fastcall*)(void*, void*);
static TonemapLutFn g_orig_tonemap_lut = nullptr;
// Parked at 0 while the composer probe says what actually differs: 0x640 is the same
// object for both views and 0xF88 is thrown away by the composer.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GradingSrcLend = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradingLends = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradingSrcMain = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradingSrcVrcam = 0;
// 0 = only fill a hole (never displace), 1 = also replace a pointer the view already has.
// 0x640 IS populated for the second view -- with its own ungraded settings -- so filling alone
// would do nothing here; the whole point is to substitute, scoped to the one call.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GradingSrcDisplace = 1;
static std::atomic<uintptr_t> g_main_grading_src{0};

// SEH only -- no C++ objects in here (C2712), and the restore has to survive an exception.
static int tonemap_view_kind(void* a2) {          // 0 = main, 1 = vrcam, -1 = other/unknown
    if (!a2) return -1;
    __try {
        uintptr_t ctx = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(a2) + 0x18);
        if (!ctx) return -1;
        const uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
        if (key == 0) return 0;
        if (key == g_vrcam_ctx_key) return 1;
        return -1;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return -1; }
}

// Its own resolver: the shared one is declared further down, with the DrawComposition code.
using GradingViewDataFn = __int64(__fastcall*)(void*);
static GradingViewDataFn g_grading_viewdata_get = nullptr;

static uintptr_t* tonemap_grading_slot(void* a2) {
    if (!g_grading_viewdata_get && g_exe_base)
        g_grading_viewdata_get = reinterpret_cast<GradingViewDataFn>(g_exe_base + 0x33C248);
    if (!g_grading_viewdata_get) return nullptr;
    __try {
        uint8_t* viewData = reinterpret_cast<uint8_t*>(g_grading_viewdata_get(a2));
        return viewData ? reinterpret_cast<uintptr_t*>(viewData + CyberpunkVR_GradingSrcOff) : nullptr;
    } __except (EXCEPTION_EXECUTE_HANDLER) { return nullptr; }
}

static __int64 tonemap_call_lent(void* a1, void* a2, uintptr_t* slot, uintptr_t lend) {
    const uintptr_t saved = *slot;
    __try {
        *slot = lend;
        return g_orig_tonemap_lut(a1, a2);
    } __finally {
        *slot = saved;
    }
}

void lut_src_build_begin();   // CommandListCensus.cpp: is MAIN's LUT source table fresh enough to lend

static __int64 __fastcall Detour_TonemapLut(void* a1, void* a2) {
    lut_src_build_begin();
    if(CyberpunkVR_OpticsDataProbe) {
        const int side=optics_data_side();
        if(side>=0) {
            const auto vd=reinterpret_cast<GradingViewDataFn>(g_exe_base+0x33c248)(a2);
            if(vd) {
                InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsDataSeq[side]));
                cloud_cb_raw_copy(CyberpunkVR_OpticsViewData[side],reinterpret_cast<const void*>(vd),4096);
                InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_OpticsDataSeq[side]));
            }
        }
    }
    if (CyberpunkVR_GradeCbProbe) {
        const int kind = tonemap_view_kind(a2);
        if (kind >= 0) {
            t_grade_cb_view = kind;
            t_grade_cb_idx  = 0;
            const __int64 r = g_orig_tonemap_lut(a1, a2);
            grade_cb_commit(kind, t_grade_cb_idx);
            t_grade_cb_view = -1;
            grade_cb_report();
            return r;
        }
    }
    if (!CyberpunkVR_GradingSrcLend) return g_orig_tonemap_lut(a1, a2);
    const int kind = tonemap_view_kind(a2);
    uintptr_t* slot = (kind >= 0) ? tonemap_grading_slot(a2) : nullptr;
    if (!slot) return g_orig_tonemap_lut(a1, a2);
    if (kind == 0) {                                   // MAIN: remember where it grades from
        uintptr_t cur = 0;
        __try { cur = *slot; } __except (EXCEPTION_EXECUTE_HANDLER) { cur = 0; }
        if (cur) {
            g_main_grading_src.store(cur, std::memory_order_release);
            CyberpunkVR_DebugGradingSrcMain = cur;
        }
        return g_orig_tonemap_lut(a1, a2);
    }
    const uintptr_t lend = g_main_grading_src.load(std::memory_order_acquire);
    uintptr_t cur = 0;
    __try { cur = *slot; } __except (EXCEPTION_EXECUTE_HANDLER) { return g_orig_tonemap_lut(a1, a2); }
    CyberpunkVR_DebugGradingSrcVrcam = cur;      // so the two can be compared from outside
    if (!lend || lend == cur) return g_orig_tonemap_lut(a1, a2);
    if (cur && !CyberpunkVR_GradingSrcDisplace) return g_orig_tonemap_lut(a1, a2);
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugGradingLends)));
    return tonemap_call_lent(a1, a2, slot, lend);
}

// ---- registered where they are defined ---------------------------------------------------------
CVR_DETOUR("[gradeup] constant uploader sub_14041A780", CB_UPLOAD_RVA, Detour_CbUpload, g_orig_cb_upload)
CVR_DETOUR("[grade] grading composer sub_140C944A4", GRADING_COMPOSE_RVA, Detour_GradingCompose, g_orig_grading_compose)
CVR_DETOUR("[grading] GenerateTonemappingLUT sub_140F60474 (vrcam borrows main grading source)", TONEMAP_LUT_RVA, Detour_TonemapLut, g_orig_tonemap_lut)

// ================================================================================================
// THE GRADING SOURCE, moved out of the monolith.
//
// Giving the second eye the first eye's grading source is what keeps the two from differing in colour.
// The probes here answer the question that made it possible: WHAT actually differs in the composed
// grading parameters, and in the 688-byte constant block the LUT build uploads.
//
// grade_up_store is written with an SEH guard because it reads a caller-supplied pointer whose lifetime
// is the engine's business, not ours.
// ================================================================================================

// ---- colour grading: give the second eye the first eye's grading source ---------------------
//
// Symptom: the scanner's green screen tint never appeared in VRCAM, though its HUD markers did.
// The capture settles where it is lost. The tonemap pass PipelineState_777 runs for BOTH views
// with an identical set of 31 root tables and both bind the SAME `3 x Texture3D<float3>` --
// three 48^3 R11G11B10 grading tables. What differs is their CONTENT at sample time: neutral for
// VRCAM, green for MAIN. They are rebuilt once per view by CRenderNode_GenerateTonemappingLUT
// (three 6x6x6 dispatches = 48^3/8^3, recorded into the ASYNC COMPUTE list -- which is why no
// census here ever saw them until compute lists started being hooked).
//
// The node's body sub_14077A36C opens with
//     viewData = sub_14033C248(wc);  rdi = *(viewData + 0xF88);
//     if (!rdi) goto cold;           xmm6 = *(float*)(rdi + 0x44);
//     cold:  xmm6 = flt_1431EFC58;   // = -1.0f, the "unset" sentinel
// and passes xmm6 into sub_140C944A4. So MAIN feeds a real grading parameter and VRCAM feeds
// "no value", which is exactly a neutral table.
//
// `viewData+0xF88` is hole bit 8 in kViewDataHoles -- it has been in that table from the start,
// labelled "composition/debug" and left off because its consumer was unknown, and the live diff
// has reported it MAIN-set/VRCAM-zero in every sample it has ever taken.
//
// Lending is scoped to this one call and restored in a __finally, so nothing is left pointing at
// another view's object. That matters: this is the same shape of edit that crashed three times
// at viewData+0x168 -- but 0x168 is a resource SET the RTT view genuinely does not own, whereas
// this is a settings object the grading code only reads one float out of.
//
// NOT tried again: skipping the node so VRCAM reuses MAIN's tables. That turns the second eye
// black. The tables are TRANSIENT -- the capture has aliasing barriers and DiscardResource
// around them, so despite sharing a resource id there is nothing of MAIN's left to sample.
// ---- what actually differs in the composed grading parameters -------------------------------
// Measured, so no more guessing at inputs: viewData+0x640 is the SAME object for both views
// (DebugGradingSrcMain == DebugGradingSrcVrcam), and lending viewData+0xF88 fired 6776 times
// and changed nothing. So watch the OUTPUT instead. sub_140C944A4 fills a 40-byte parameter
// block from that shared object plus two per-view scalars:
//     void sub_140C944A4(a1, viewData, a3 /*byte*/, a4 /*float*/, a5 /*out 40B*/, a6 /*out 1B*/)
//     out[0]=src[224] out[4]=src[228] out[8]=src[232] out[16]=src[236] out[20]=src[240]
//     out[28]=a3+4    out[12]=a3 ? a4 : -1.0f        out[36..39]=src[244..247]
// Snapshot it per view and print the two side by side; whatever differs is the grade.
// ---- the constant buffers the LUT build actually reads --------------------------------------
// Everything upstream measured EQUAL between the views: same shader, same three 6x6x6 dispatches,
// same grading-settings object at viewData+0x640, and a byte-identical 40-byte parameter block
// out of sub_140C944A4. Yet the tables come out neutral for VRCAM and graded for MAIN. So the
// difference has to be in what PipelineState_865 itself is handed -- its constant buffers.
//
// They live in the upload ring and are bound in place, so CopyBufferRegion never sees them; the
// place they are visible is CreateConstantBufferView, which hands over the GPU address, and the
// ring is already mapped. Capture every CBV created while GenerateTonemappingLUT is on the
// stack, in creation order, per view, and diff.
thread_local int      t_grade_cb_view = -1;   // 0 = main, 1 = vrcam, -1 = not in the node
thread_local uint32_t t_grade_cb_idx  = 0;
// GRADE_CB_SLOTS moved to Stereo/StereoInternal.hpp.
// GRADE_CB_MAX moved to Stereo/StereoInternal.hpp.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_GradeCbProbe = 1;   // OFF: grading-LUT CB capture (scanner tint, parked)
uint8_t  g_gcb[2][GRADE_CB_SLOTS][GRADE_CB_MAX];
uint32_t g_gcb_len[2][GRADE_CB_SLOTS];
static uint32_t g_gcb_n[2] = {0, 0};
std::mutex g_gcb_mtx;

void grade_cb_commit(int kind, uint32_t n) {
    std::lock_guard<std::mutex> lk(g_gcb_mtx);
    g_gcb_n[kind] = n;
}

void grade_cb_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    uint8_t a[GRADE_CB_SLOTS][GRADE_CB_MAX], b[GRADE_CB_SLOTS][GRADE_CB_MAX];
    uint32_t la[GRADE_CB_SLOTS], lb[GRADE_CB_SLOTS], na, nb;
    {
        std::lock_guard<std::mutex> lk(g_gcb_mtx);
        na = g_gcb_n[0]; nb = g_gcb_n[1];
        if (!na || !nb) return;
        memcpy(a, g_gcb[0], sizeof(a));  memcpy(b, g_gcb[1], sizeof(b));
        memcpy(la, g_gcb_len[0], sizeof(la)); memcpy(lb, g_gcb_len[1], sizeof(lb));
    }
    s_last = now;
    char line[1200];
    int u = 0;
    line[0] = 0;
    const uint32_t n = na < nb ? na : nb;
    for (uint32_t k = 0; k < n && u < static_cast<int>(sizeof(line)) - 120; ++k) {
        const uint32_t len = la[k] < lb[k] ? la[k] : lb[k];
        uint32_t diffs = 0, first = 0xFFFFFFFF;
        for (uint32_t o = 0; o + 4 <= len; o += 4)
            if (memcmp(a[k] + o, b[k] + o, 4)) { ++diffs; if (first == 0xFFFFFFFF) first = o; }
        u += snprintf(line + u, sizeof(line) - u, "cb%u(%uB/%uB d%u", k, la[k], lb[k], diffs);
        if (diffs) {
            float fm, fv;
            memcpy(&fm, a[k] + first, 4);
            memcpy(&fv, b[k] + first, 4);
            uint32_t im, iv;
            memcpy(&im, a[k] + first, 4);
            memcpy(&iv, b[k] + first, 4);
            u += snprintf(line + u, sizeof(line) - u, " @+%X M=%08X(%.5g) V=%08X(%.5g)",
                          first, im, fm, iv, fv);
        }
        u += snprintf(line + u, sizeof(line) - u, ") ");
    }
    log("[gradecb] MAIN %u cbv / VRCAM %u cbv: %s", na, nb, line);
}

// ---- the 688-byte constant block the LUT build uploads --------------------------------------
// Located in sub_14077A36C's main flow:
//     cmp   [arg_0], 12h            ; a per-view byte, from sub_1401ED918(wc)
//     mov   edi, 5256A2C8h / eax, 2D3E6FF5h ; two shader permutations
//     cmovz edi, eax                ; chosen by that byte
//     mov   ecx, 2B0h               ; 688 bytes
//     call  sub_14041A780           ; <- the same uploader the cloud constants go through
// and the clouds taught us the thing that matters here: this engine puts DESCRIPTOR INDICES in
// its constant buffers (that is why mirroring the whole cloud buffer killed VRCAM's clouds --
// fields +140/+160/+164 were transient-target indices). So the choice between grading LUT
// Resource_345 (green, MAIN) and Resource_2123 (ordinary, VRCAM) is very plausibly a field in
// this block. Capture it per view and diff -- exactly the workflow that solved the clouds.
constexpr uint32_t  GRADE_CB_BYTES  = 0x2B0;      // 688
using CbUploadFn = __int64(__fastcall*)(unsigned int, void*);
CbUploadFn g_orig_cb_upload = nullptr;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_GradeUpProbe = 1;   // OFF: grading upload-ring capture (scanner tint, parked)
static uint8_t  g_gcu[2][GRADE_CB_BYTES];
bool     g_gcu_seen[2] = {false, false};
static std::mutex g_gcu_mtx;

static bool grade_up_store(const void* src, int v) {       // SEH only, no C++ objects
    uint8_t tmp[GRADE_CB_BYTES];
    if (!cloud_cb_raw_copy(tmp, src, GRADE_CB_BYTES)) return false;
    std::memcpy(g_gcu[v], tmp, GRADE_CB_BYTES);
    g_gcu_seen[v] = true;
    return true;
}

void grade_up_report() {
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 5000) return;
    uint8_t a[GRADE_CB_BYTES], b[GRADE_CB_BYTES];
    {
        std::lock_guard<std::mutex> lk(g_gcu_mtx);
        if (!g_gcu_seen[0] || !g_gcu_seen[1]) return;
        std::memcpy(a, g_gcu[0], GRADE_CB_BYTES);
        std::memcpy(b, g_gcu[1], GRADE_CB_BYTES);
    }
    s_last = now;
    char line[1500];
    int u = 0, diffs = 0;
    line[0] = 0;
    for (uint32_t o = 0; o + 4 <= GRADE_CB_BYTES; o += 4) {
        uint32_t m, v;
        std::memcpy(&m, a + o, 4);
        std::memcpy(&v, b + o, 4);
        if (m == v) continue;
        ++diffs;
        float fm, fv;
        std::memcpy(&fm, &m, 4);
        std::memcpy(&fv, &v, 4);
        if (u < static_cast<int>(sizeof(line)) - 70)
            u += snprintf(line + u, sizeof(line) - u, "+%03X M=%08X(%.4g) V=%08X(%.4g)  ",
                          o, m, fm, v, fv);
    }
    log("[gradeup] 688B grading block, %d differing dwords: %s", diffs, u ? line : "(none)");
}

// Mirror selected dwords of MAIN's block into VRCAM's, scoped to the one upload.
//
// Measured, stable across every sample (the frame-to-frame noise at +0A0/+170/+178/+1D0/+198/
// +278 is transient descriptor churn and is deliberately NOT in this list, for the same reason
// mirroring the whole cloud buffer killed the clouds):
//     +0C8 / +1B0   320 vs 306   -- 2560/8 vs 2444/8, pure resolution. Never mirror.
//     +230 / +238   32330 vs 16794  -- in range for the 163840-entry descriptor heap; the
//                   prime suspect for "which grading LUT array", i.e. Resource_345 vs _2123
//     +220          exactly 176x the above -- a byte offset with the same stride
//     +248          -0.1631 vs -0.1235
//     +258          0x12 vs 0x16 -- and 0x12 is literally what `cmp [arg_0], 12h` tests before
//                   `cmovz` picks the other shader permutation
//     +1B8 / +1E0 / +270  stable, unidentified
// One bit per candidate so they can be bisected live; default is the descriptor-index pair.
// BITS 8..14 ARE THE GRADING ITSELF, and they are the ones that carry the look. Measured
// 2026-08-30 by reading both captured blocks out of g_gcu in the debugger while the braindance
// played, and diffing them dword by dword:
//
//     off    MAIN      VRCAM      what it is in the .envparam
//     +010   1.04167   1.0        gammaValue Luminance 0.96, inverted
//     +014   1.04167   1.0
//     +018   1.04167   1.0
//     +024   1.3       1.0        gain Green
//     +028   1.3       1.0        gain Blue
//     +04C   0.435     0.0        contrastPivot
//     +074   1.3       1.0        saturation
//
// Those are literally the numbers in base\weather\24h_basic\24h_braindance_fpp.envparam, whose
// ColorGradingAreaSettings is the braindance's green cast (warmsquare LUT, gain 1.3 on green and
// blue, saturation 1.3). MAIN's block carries them; the second view's block is the identity.
//
// EVERY differing dword below 0x080 is one of those seven, and the first pointer-shaped
// difference only starts at +080 -- so this head region is grading and nothing else, and copying
// it cannot carry a per-view pointer across.
//
// This is why bits 0-7 never delivered the tint no matter which of them were set: they are all
// descriptor indices and unidentified words, and not one of them is a grading value.
static const uint32_t kGradeMirrorOff[] = {
    0x230, 0x238, 0x220, 0x248, 0x258, 0x1B8, 0x1E0, 0x270,
    0x010, 0x014, 0x018, 0x024, 0x028, 0x04C, 0x074,
    // BITS 15 and 16: THE TWO SMALL INTEGERS, and they are the only remaining candidates for
    // "which grading LUT". Measured 2026-08-30 from both captured blocks in the debugger:
    //
    //     +168   MAIN 2948   VRCAM 1638
    //     +18C   MAIN  564   VRCAM  559
    //
    // Everything else that still differs below +0x2B0 is a 0x1102_xxxxxxxx half-pointer, and
    // MAIN's pointer in the second view's slot is a dereference waiting to happen -- so those are
    // deliberately NOT candidates, whatever bits 2/4/6/7 above may once have suggested. Of those
    // eight, +0x230, +0x248 and +0x1B8 are measured EQUAL between the views now, so their bits
    // cannot do anything at all; that is why setting 3 and then 7 changed nothing.
    //
    // That the block reaches the second eye's picture is not assumed: forcing +074 to 5.0 turned
    // that eye acid-bright, which is what licenses looking for the LUT here rather than elsewhere.
    0x168, 0x18C,
    // BITS 17..26: THE VIEWS SAMPLE DIFFERENT GRADING LUTS, and the block says so in the clear.
    // Measured 2026-09-05 at a yellow street lamp, both 688-byte blocks taken on the engine's own
    // uploader in adjacent frames and the view named by which upload counter moved:
    //
    //     +0x210  cb6[33].x   MAIN 0.97917   second view 0.96875
    //     +0x214  cb6[33].y   MAIN 0.01042   second view 0.01562
    //
    // Those are a 3D LUT's sampling scale and half-texel bias, and they give the table's size
    // exactly: (N-1)/N and 1/(2N). MAIN reads N = 48 (47/48 and 1/96); the second view reads
    // N = 32 (31/32 and 1/64). 48 is the size of the tables the engine BUILDS per frame in
    // GenerateTonemappingLUT; 32 is an authored asset. So the second view is not grading through
    // this frame's environment at all -- it samples a stock table -- which is exactly what "MAIN
    // is warmer at the lamp, the second view is colder" looks like.
    //
    // Alongside them, seven descriptor indices read 0x1B0 for MAIN and 0x1AF for the second view --
    // adjacent slots, i.e. a different texture bound. They travel WITH the scale/bias or not at all:
    // mirroring the size of a table the view is not bound to would sample the wrong one on purpose.
    //
    // What this measurement also KILLED, so it is not retried: cb6[12] and cb6[13], the bit masks
    // that pick a LUT class from the stencil, are byte-identical between the views (+0x0C0..+0x0DF),
    // and so is cb6[6] (+0x060/+0x064), the log-space scale and offset. The stencil-driven-selection
    // theory recorded from the sticker hunt does not apply here.
    0x210, 0x214,                                             // bits 17, 18: LUT scale and bias
    0x19C, 0x1EC, 0x1FC, 0x224, 0x23C, 0x244, 0x264,          // bits 19..25: the descriptor indices
    // (+0x258 is NOT repeated here: it has been bit 4 since the table was written, and adding it a
    //  second time would give one dword two bits and make a mask impossible to read back.)
};
static const uint32_t kGradeMirrorCount =
    static_cast<uint32_t>(sizeof(kGradeMirrorOff) / sizeof(kGradeMirrorOff[0]));
// DEFAULT IS BACK TO THE DESCRIPTOR-INDEX PAIR, and bits 8-14 are retired rather than merely off.
//
// Those seven bits substituted MAIN's grading values into the finished 688-byte constant block. It
// worked, and it was the wrong end of the chain: the values are read out of viewData +0x650..+0x69F
// by the block's own producer, so mirroring them THERE (ViewReuse.cpp, env-mirror bit 12) delivers
// the same tint one step earlier and keeps everything else derived from those fields consistent.
//
// Measured, not assumed, 2026-08-30: with the viewData mirror on and this mask dropped from 0x7F03
// to 3, the braindance's green cast stayed on the second eye. Two mechanisms doing one job is how
// this project has repeatedly lost a day, so the survivor is the one nearer the cause.
//
// The bits are left in the table, not deleted: they are measured-correct offsets and cost nothing
// while clear, and they are the only route left if the viewData mirror ever has to be turned off.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_GradeMirrorMask = (1u << 0) | (1u << 1);
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradeMirrors = 0;
// So "it did not help" and "it never saw that view" can never be confused again.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradeMainUploads  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradeVrcamUploads = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugGradeOtherView    = 0;


__int64 grade_up_mirror_call(unsigned int size, void* src) {   // SEH only
    // ONE SLOT PER TABLE ENTRY, CHECKED. This was `saved[16]` beside a table of seventeen, so bit 16
    // -- one of the two descriptor indices that ship in the default mask -- wrote its saved dword
    // one past the end of the array, onto the stack frame. It never showed as a crash because the
    // neighbouring slot is written back immediately afterwards, but it was corruption all the same,
    // and the table only grew from here.
    uint32_t saved[64];
    static_assert(kGradeMirrorCount <= 64, "saved[] must have a slot per kGradeMirrorOff entry");
    const uint32_t mask = CyberpunkVR_GradeMirrorMask;
    uint8_t* p = static_cast<uint8_t*>(src);
    __try {
        for (uint32_t k = 0; k < kGradeMirrorCount; ++k) {
            if (!(mask & (1u << k))) continue;
            const uint32_t o = kGradeMirrorOff[k];
            std::memcpy(&saved[k], p + o, 4);
            std::memcpy(p + o, g_gcu[0] + o, 4);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { return ForwardCbUpload(size, src); }
    __int64 r = 0;
    __try {
        r = ForwardCbUpload(size, src);
    } __finally {
        for (uint32_t k = 0; k < kGradeMirrorCount; ++k)
            if (mask & (1u << k)) std::memcpy(p + kGradeMirrorOff[k], &saved[k], 4);
    }
    CVR_DIAGNOSTIC(InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugGradeMirrors)));
    return r;
}

bool grade_up_is_target(unsigned int size, void* src) {
    if (size != GRADE_CB_BYTES || !src || !g_exe_base) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    const uintptr_t work = t_current_node_work;
    return work > base && static_cast<uint32_t>(work - base) == 0xF60474;
}

bool grade_up_capture(void* src, int v) {
    std::lock_guard<std::mutex> lk(g_gcu_mtx);
    return grade_up_store(src, v);
}

}  // namespace detail
}  // namespace cvr
