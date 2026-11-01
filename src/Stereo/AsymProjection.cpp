#include "Utils/DebugGate.hpp"
// AsymProjection -- the engine's own off-centre projection, per eye.
//
// A headset lens is not centred on its panel: a Quest 3 eye sees -54/+40 deg horizontally and
// +44/-55 vertically. The port used to render one frustum SYMMETRIC about the eye axis, sized to the
// widest half-angle, and let the compositor throw the rest away -- 110x110 deg of tangent span for a
// 94x99 deg panel, a third of every rendered pixel never shown and the centre that much softer.
//
// REDengine 4 can draw the off-centre frustum itself; nothing has to be faked. Its camera matrix
// builder (sub_140296EA0) takes, besides the vertical fov, aspect and zoom, an NDC shift of the
// projection centre at camera+0x30/+0x34 and post-multiplies the projection by that translation
// (row 3 = (x, y, 0, 1), so P20/P21 carry it) -- the same multiply it uses for the TAA jitter, but a
// separate field: the history camera is rebuilt with the jitter zeroed and the shift kept
// (sub_140297904 -> sub_140787D84), so motion vectors and DLSS see an unjittered camera that is
// simply off-centre.
//
// Where the shift goes in. Every eye view is prepared by sub_1404240DC(ctx, spec, view): it builds
// the view's four camera descriptors (view+0xE0 + i*0x3E0, one per rendering plane; Render/ViewCameraLayout.hpp)
// from the view's camera INPUT block at view+0x2C50 -- the same layout as a descriptor: fov +0x2C70, aspect
// +0x2C78, zoom +0x2C7C, shift +0x2C80/+0x2C84, near/far +0x2C90/+0x2C94 -- then adds the jitter and rebuilds.
// (2.31: descriptors view+0x70 + i*0x3A0, input block view+0x1E20.)
// Writing the input block on the way in reaches the descriptors, their copies, the jittered rebuild,
// the history, the view-state matrices and the Streamline constants through the engine's own code, in
// one place, for both eyes. FinalCamera is too late: it runs in PrepareSceneRendering, after the
// matrices exist. Only the two eye views pass through here (measured: 0 other views).
//
// Measured on the live game with MAIN shifted by hand (mode 2), time frozen: the picture moves by
// exactly shift * size / 2 (144 px at 0.3 of a 960 px eye, 96 px at 0.2 vertically, 288 at 0.6), and
// the overlap is the same picture -- lighting, reflections and AO stay on their geometry; what differs
// is DLSS resampling and the auto exposure seeing a different frame. The screen-space HUD stays where
// it was drawn, as it must.
//
// The view is named by its key at view+0x28 (MAIN 0, VRCAM g_vrcam_ctx_key) -- the same test the cull
// and DLSS detours use.

#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Core/VrCoreShared.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Render/ViewCameraLayout.hpp"

#include <windows.h>
#include <atomic>

extern "C" int CyberpunkVR_MainIsRightEye;
extern void Log(const char* fmt, ...);
#include <cmath>
#include <cstdint>
#include <cstring>

extern "C" {
// 0 = the engine's symmetric projection. 1 = each eye's own runtime frustum
// (Runtimes/AsymmetricProjection.hpp). 2 = the test values below, for measuring the engine.
__declspec(dllexport) int32_t CyberpunkVR_AsymProjection = 1;
__declspec(dllexport) int32_t CyberpunkVR_AsymSquarePixels = 1;
__declspec(dllexport) float CyberpunkVR_AsymInnerGuard = 0.0f;
// [0..3] MAIN: NDC shift x, y; aspect (0 keeps the engine's); unused. [4..7] the same for VRCAM.
__declspec(dllexport) float CyberpunkVR_AsymTest[8] = {};
// Prepare calls seen: MAIN, VRCAM, other views.
__declspec(dllexport) uint64_t CyberpunkVR_DebugAsymPrepare[3] = {};
// The input block as the engine left it, per eye (MAIN [0..7], VRCAM [8..15]):
// fov, +0x1E44, aspect, zoom, shift x, shift y, near, far.
__declspec(dllexport) float CyberpunkVR_DebugAsymView[16] = {};
// The plane overrides as the engine left them, per eye and plane (4 x 8 floats each):
// fov, zoom, shift x, shift y, [4], [5], near, far; zeros where the plane has none.
__declspec(dllexport) float CyberpunkVR_DebugAsymPlanes[64] = {};
// What the engine built, per eye (MAIN [0..15], VRCAM [16..31]): scene descriptor fov, aspect, zoom,
// shift x, shift y, P00, P11, P20, P21; first-person descriptor fov, shift x, shift y, P00, P11, P20,
// P21. Read right after the producer returns -- the numbers the frame is drawn with.
__declspec(dllexport) float CyberpunkVR_DebugAsymBuilt[32] = {};
// The terms the input fov goes through on the way to the descriptor, per eye:
// descriptor fov = [view+0x17F4] * input fov + [view+0x1D40].
__declspec(dllexport) float CyberpunkVR_DebugAsymFovTerms[4] = {};
// The window each eye was given in mode 1, tangents l, r, d, u (MAIN [0..3], VRCAM [4..7]).
__declspec(dllexport) float CyberpunkVR_DebugAsymWindow[8] = {};
// All four descriptors per eye after the producer (eye*4+desc)*8: fov, aspect, zoom, shift x, shift y,
// jitter x, jitter y, P20. And whether each plane carries an override right now (eye*4+plane).
__declspec(dllexport) float CyberpunkVR_DebugAsymDescs[64] = {};
__declspec(dllexport) uint32_t CyberpunkVR_DebugAsymPlanePtr[8] = {};
// The sun cascades fitted to both eyes' frusta (Detour_CascadeFit). 0 = each view fits its own window.
__declspec(dllexport) int32_t CyberpunkVR_AsymCascadeUnion = 1;
// Fits given the union: MAIN, VRCAM, and fits left alone (no window of ours on the view, or off).
__declspec(dllexport) uint64_t CyberpunkVR_DebugAsymCascadeFits[3] = {};
// What the last fit of each view was given: vertical fov, aspect, shift x, shift y (MAIN [0..3], VRCAM [4..7]).
__declspec(dllexport) float CyberpunkVR_DebugAsymCascadeWindow[8] = {};
}

namespace cvr::detail {
namespace {

constexpr uintptr_t VIEW_PREPARE_RVA = 0x4240DC;
constexpr size_t kInputFov = 0x2C70;   // fov, ?, aspect, zoom, shift x, shift y, ?, ?, near, far
constexpr size_t kFovScale = 0x25D4;
constexpr size_t kFovAdd   = 0x2B70;
// THE FIRST-PERSON PLANE HAS ITS OWN PROJECTION, AND ITS OWN SHIFT. After building the descriptors the
// producer hands each one whose plane carries an override (the pointer at view+0x1688 + i*0x10) to
// sub_140422D2C, which rebuilds that descriptor from the override: fov [0], zoom [1], NDC shift
// [2]/[3], near/far [6]/[7]. The weapon pass draws with descriptor 2 (sub_14033DDA4) and plane 2 carries
// the override (fov = the scene's, far 4000). Measured with the override left alone: the world moved
// and the arms and the gun stayed put. The shift is a ratio of the frustum's own span, so the same
// number is right at the plane's own fov.
//
// WRITTEN WHERE IT IS APPLIED, NOT ON THE WAY IN. MAIN's override exists when the producer is entered;
// VRCAM's is attached DURING the producer (measured: null at entry, set at exit), so a write at entry
// left the second eye's gun unshifted -- 116 px off its hand at a Quest 3 shift. sub_140422D2C has one
// caller, this producer, so a detour on it told which eye is being prepared covers both.
constexpr size_t kPlaneOverrides = 0x2470;
constexpr int kPlanes = 4;
constexpr size_t kDesc = cvr::view_layout::kCamera, kDescStride = cvr::view_layout::kCameraStride;

using ViewPrepareFn = __int64(__fastcall*)(void*, void*, uint8_t*);
ViewPrepareFn g_orig_view_prepare = nullptr;
constexpr uintptr_t PLANE_OVERRIDE_RVA = 0x422D2C;
using PlaneOverrideFn = __int64(__fastcall*)(float*, void*);
PlaneOverrideFn g_orig_plane_override = nullptr;

// The eye this thread is preparing, for the plane detour: side (-1 none), the shift to give the
// planes, and whether to give it (else: take back a shift of ours still standing).
struct Preparing { int side = -1; bool want = false; float x = 0.0f, y = 0.0f, lastX = 0.0f, lastY = 0.0f; };
thread_local Preparing t_preparing;

// What we put in, so a later pass can tell the engine's own refresh from our value still standing,
// and put the engine's back when the mode goes off.
struct Side {
    bool  active = false;
    float wroteFov = 0.0f, wroteX = 0.0f, wroteY = 0.0f, wroteAspect = 0.0f;
    float engineFov = 0.0f, engineAspect = 0.0f;
    float halfX = 0.0f, halfY = 0.0f;   // the window's half spans (tangent), for ViewWindowOf
};
Side g_side[2];

// THE RENDER TARGET KEEPS THE GAME RESOLUTION'S ASPECT, the lens window has the headset's. The window is
// grown to the target's aspect so the pixels stay square (AsymmetricProjection.hpp); what that costs is
// the grown sliver, and the resolution that would make it zero is said here, once per change.
void NoteAspect(const uint8_t* view, const cvr::projection::EyeWindow& w, const cvr::projection::EyeWindow& content) {
    static uint32_t lastW = 0, lastH = 0;
    static float lastAspect = 0.0f, lastLens = 0.0f;
    uint32_t size[2]{};
    std::memcpy(size, view + 0x34, sizeof(size));
    if (cvr::engine_stereo::Enabled()) size[0] /= 2;   // both eyes side by side in one target
    const float aspect = (w.r - w.l) / (w.u - w.d);
    const float lensAspect = (content.r - content.l) / (content.u - content.d);
    if (!size[0] || !size[1] || size[0] > 16384 || size[1] > 16384 || !(aspect > 0.05f) || !(lensAspect > 0.05f)) return;
    if (size[0] == lastW && size[1] == lastH && std::fabs(aspect - lastAspect) < 1e-4f &&
        std::fabs(lensAspect - lastLens) < 1e-4f) return;
    lastW = size[0]; lastH = size[1]; lastAspect = aspect; lastLens = lensAspect;
    const float render = float(size[0]) / float(size[1]);
    const float grown = std::fabs(aspect / lensAspect - 1.0f) * 100.0f;
    Log("[asym] MAIN window tan l=%.4f r=%.4f d=%.4f u=%.4f (%.1f x %.1f deg), aspect %.4f; render %ux%u, aspect %.4f "
        "-> %s; content aspect %.4f (inner guard %.2f), window grown %.2f%% %s\n",
        w.l, w.r, w.d, w.u, (std::atan(w.r) - std::atan(w.l)) * 57.2957795f, (std::atan(w.u) - std::atan(w.d)) * 57.2957795f,
        aspect, size[0], size[1], render, std::fabs(render / aspect - 1.0f) < 0.002f ? "square pixels" : "NON-SQUARE pixels",
        lensAspect, CyberpunkVR_AsymInnerGuard, grown, aspect < lensAspect ? "vertically" : "horizontally");
    if (grown >= 0.3f)
        Log("[asym] a target matching the content exactly: %ux%u or %ux%u\n",
            size[0], unsigned(std::lround(size[0] / lensAspect)), unsigned(std::lround(size[1] * lensAspect)), size[1]);
}

// ENGINE STEREO: MAIN is the head centre the engine clones into the two eyes (Stereo/EngineStereo.cpp). MAIN gets
// the horizontal union of the eyes' windows -- what culling and the sun cascades see from the centre -- at the
// eyes' one vertical span (Windows() gives both eyes the same), centred between their centres; each eye's own
// aspect and shift go to the eye cameras' build, so each renders exactly its lens window.
cvr::projection::EyeWindow EngineStereoCentre(const AsymEyeWindows& w) {
    const cvr::projection::EyeWindow& a = w.eye[0];
    const cvr::projection::EyeWindow& b = w.eye[1];
    const float span = a.u - a.d, c = 0.25f * (a.u + a.d + b.u + b.d);
    cvr::engine_stereo::EyeProjection eyes[2];
    bool valid = span > 0.05f && std::fabs((b.u - b.d) - span) < 1e-3f * span;
    for (int e = 0; e < 2; ++e) {
        const cvr::projection::EngineProjection p = cvr::projection::ToEngine(w.eye[e]);
        eyes[e] = {p.aspect, p.shiftX, p.shiftY};
        valid = valid && p.aspect > 0.05f && p.aspect < 20.0f && std::isfinite(p.shiftX) && std::isfinite(p.shiftY) &&
                std::fabs(p.shiftX) < 1.0f && std::fabs(p.shiftY) < 1.0f;
    }
    cvr::engine_stereo::PublishEyes(eyes, valid);
    return {std::fmin(a.l, b.l), std::fmax(a.r, b.r), c - 0.5f * span, c + 0.5f * span};
}

void Restore(Side& s, float* in) {
    if (in[0] == s.wroteFov) in[0] = s.engineFov;
    if (in[4] == s.wroteX) in[4] = 0.0f;
    if (in[5] == s.wroteY) in[5] = 0.0f;
    if (in[2] == s.wroteAspect) in[2] = s.engineAspect;
    s.active = false;
}

void Apply(uint8_t* view, int side) {
    float* in = reinterpret_cast<float*>(view + kInputFov);
    Side& s = g_side[side];
    // The engine's own values: refreshed whenever the field no longer holds what we wrote.
    if (!s.active || in[0] != s.wroteFov) s.engineFov = in[0];
    if (!s.active || in[2] != s.wroteAspect) s.engineAspect = in[2];
    float* plane[kPlanes] = {};
    for (int i = 0; i < kPlanes; ++i) {
        plane[i] = *reinterpret_cast<float**>(view + kPlaneOverrides + i * 0x10);
    }
    if (!s.active) {
        float* dbg = CyberpunkVR_DebugAsymView + side * 8;
        dbg[0] = in[0]; dbg[1] = in[1]; dbg[2] = in[2]; dbg[3] = in[3];
        dbg[4] = in[4]; dbg[5] = in[5]; dbg[6] = in[8]; dbg[7] = in[9];
        for (int i = 0; i < kPlanes; ++i) {
            float* p = CyberpunkVR_DebugAsymPlanes + side * 32 + i * 8;
            for (int k = 0; k < 8; ++k) p[k] = plane[i] ? plane[i][k] : 0.0f;
        }
    }
    const float fovScale = *reinterpret_cast<const float*>(view + kFovScale);
    const float fovAdd = *reinterpret_cast<const float*>(view + kFovAdd);
    CyberpunkVR_DebugAsymFovTerms[side * 2 + 0] = fovScale;
    CyberpunkVR_DebugAsymFovTerms[side * 2 + 1] = fovAdd;

    float fov = 0.0f, x = 0.0f, y = 0.0f, aspect = 0.0f;
    bool want = false;
    if (CyberpunkVR_AsymProjection == 1) {
        AsymEyeWindows w{};
        const int mainEye = CyberpunkVR_MainIsRightEye ? 1 : 0;
        const int eye = side == 0 ? mainEye : 1 - mainEye;
        if (OpenXRManager::Get().GetAsymmetricEyeWindows(&w) && std::fabs(fovScale) > 0.05f) {
            const cvr::projection::EyeWindow own =
                side == 0 && cvr::engine_stereo::Enabled() ? EngineStereoCentre(w) : w.eye[eye];
            const cvr::projection::EngineProjection p = cvr::projection::ToEngine(own);
            float* dw = CyberpunkVR_DebugAsymWindow + side * 4;
            dw[0] = own.l; dw[1] = own.r; dw[2] = own.d; dw[3] = own.u;
            // The descriptor gets scale * input + add; solve for the input that lands on the window's
            // own vertical fov.
            fov = (p.vfovDeg - fovAdd) / fovScale;
            x = p.shiftX; y = p.shiftY; aspect = p.aspect;
            want = fov > 1.0f && fov < 175.0f && aspect > 0.05f && aspect < 20.0f &&
                   std::isfinite(x) && std::isfinite(y) && std::fabs(x) < 1.0f && std::fabs(y) < 1.0f;
            AsymEyeWindows content{};
            if (want && side == 0 && OpenXRManager::Get().GetAsymmetricEyeWindows(&content, false))
                NoteAspect(view, w.eye[eye], content.eye[eye]);
        }
    } else if (CyberpunkVR_AsymProjection == 2) {
        const float* t = CyberpunkVR_AsymTest + side * 4;
        fov = s.engineFov;
        x = std::isfinite(t[0]) ? t[0] : 0.0f;
        y = std::isfinite(t[1]) ? t[1] : 0.0f;
        aspect = (std::isfinite(t[2]) && t[2] > 0.05f && t[2] < 20.0f) ? t[2] : s.engineAspect;
        want = true;
    }

    // The planes get the shift from the plane detour (see PLANE_OVERRIDE_RVA). Their own fov is left
    // to the engine: it follows the camera's, which the CameraFov hook already sets to the window --
    // scaling it here as well applied the ratio twice (the first-person plane at 84 deg against the
    // scene's 99).
    t_preparing.side = side;
    t_preparing.want = want;
    t_preparing.x = x; t_preparing.y = y;
    t_preparing.lastX = s.wroteX; t_preparing.lastY = s.wroteY;
    if (want) {
        in[0] = fov; in[2] = aspect; in[4] = x; in[5] = y;
        s.wroteFov = fov; s.wroteX = x; s.wroteY = y; s.wroteAspect = aspect;
        s.halfY = std::tan((fov * fovScale + fovAdd) * 0.5f * 0.01745329252f);
        s.halfX = s.halfY * aspect;
        s.active = true;
    } else if (s.active) {
        Restore(s, in);
    }
}

__int64 __fastcall Detour_PlaneOverride(float* override, void* descriptor) {
    if (override && t_preparing.side >= 0) {
        __try {
            if (t_preparing.want) {
                override[2] = t_preparing.x; override[3] = t_preparing.y;
            } else if (override[2] == t_preparing.lastX && override[3] == t_preparing.lastY) {
                override[2] = 0.0f; override[3] = 0.0f;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    // Engine stereo: the rebuild moves the plane's eyes to its centre; they go back to their places.
    cvr::engine_stereo::EyePlaces eyes;
    __try { eyes = cvr::engine_stereo::KeepEyePlaces(static_cast<uint8_t*>(descriptor)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { eyes.valid = false; }
    const __int64 r = g_orig_plane_override(override, descriptor);
    __try { cvr::engine_stereo::RestoreEyePlaces(eyes); } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return r;
}

void ReadBuilt(const uint8_t* view, int side) {
    for (int i = 0; i < kPlanes; ++i) {
        const float* d = reinterpret_cast<const float*>(view + kDesc + i * kDescStride);
        float* o = CyberpunkVR_DebugAsymDescs + (side * 4 + i) * 8;
        o[0] = d[0x20 / 4]; o[1] = d[0x28 / 4]; o[2] = d[0x2C / 4]; o[3] = d[0x30 / 4]; o[4] = d[0x34 / 4];
        o[5] = d[0x370 / 4]; o[6] = d[0x374 / 4]; o[7] = d[0x1B0 / 4];
        CyberpunkVR_DebugAsymPlanePtr[side * 4 + i] =
            *reinterpret_cast<const uintptr_t*>(view + kPlaneOverrides + i * 0x10) ? 1u : 0u;
    }
    float* o = CyberpunkVR_DebugAsymBuilt + side * 16;
    const float* d0 = reinterpret_cast<const float*>(view + kDesc);
    const float* d2 = reinterpret_cast<const float*>(view + kDesc + 2 * kDescStride);
    o[0] = d0[0x20 / 4]; o[1] = d0[0x28 / 4]; o[2] = d0[0x2C / 4]; o[3] = d0[0x30 / 4]; o[4] = d0[0x34 / 4];
    o[5] = d0[0x190 / 4]; o[6] = d0[0x1A4 / 4]; o[7] = d0[0x1B0 / 4]; o[8] = d0[0x1B4 / 4];
    o[9] = d2[0x20 / 4]; o[10] = d2[0x30 / 4]; o[11] = d2[0x34 / 4];
    o[12] = d2[0x190 / 4]; o[13] = d2[0x1A4 / 4]; o[14] = d2[0x1B0 / 4]; o[15] = d2[0x1B4 / 4];
    if (side == 0) cvr::engine_stereo::NoteViewEyes(view + kDesc, view + kDesc + 2 * kDescStride);
}

__int64 __fastcall Detour_ViewPrepare(void* ctx, void* spec, uint8_t* view) {
    int side = -1;
    t_preparing.side = -1;
    if (view) {
        __try {
            const uint64_t key = *reinterpret_cast<const uint64_t*>(view + 0x28);
            if (key != 0 && key == g_vrcam_ctx_key.load(std::memory_order_relaxed)) side = 1;
            else if (is_main_view(view)) side = 0;
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugAsymPrepare[side < 0 ? 2 : side]);
            if (side >= 0) Apply(view, side);
        } __except (EXCEPTION_EXECUTE_HANDLER) { side = -1; }
    }
    const __int64 r = g_orig_view_prepare(ctx, spec, view);
    t_preparing.side = -1;
    if (side == 0) {
        __try { cvr::engine_stereo::NoteMainPrepared(view); } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (side >= 0 && cvr::RuntimeDiagnosticsEnabled()) {
        __try { ReadBuilt(view, side); } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return r;
}

// ONE SET OF SUN CASCADES HAS TO COVER BOTH EYES.
//
// The cascade fit (sub_1402A23AC, called by the cascade-setup node sub_140C0F204 with the view's scene
// descriptor at ctx+0xE0, 2.31: +0x70) takes the eight corners of each depth slice by unprojecting NDC (+-1, +-1, z)
// through the descriptor's own projection (sub_1404E6384 -> sub_1404E6474), and that projection carries the
// NDC shift (camera+0x30/+0x34, the same field the view producer is given). So every view fits its
// cascades to exactly its own window -- and the port then hands the second view's fit to MAIN (the record,
// the cascade camera and the sampling matrices: Stereo/ViewReuse.cpp CascRecLend, Grading.cpp
// CascRenderLend / CascSampleLend), because both eyes rasterise and sample one shared atlas.
//
// With identical windows that was exact. With each eye its own lens window the windows are mirror images
// (Quest 3: -54/+40 against -40/+54), and MAIN's outer 14 degrees lie outside the second view's cascade
// boxes: shadows there come from the wrong cascade or none, in one eye only.
//
// So each view's fit is given the UNION of the two eyes' windows -- what every eye renders is then inside
// every cascade, the eyes' fits differ by the eye separation alone (as they did with a symmetric
// projection), and the lends above are exact again. Only the fit sees the union: the descriptor itself,
// and everything else that reads it, keeps the eye's own window. The union is scaled to the descriptor's
// own vertical span, so a zoom or a fov the engine changed on top stays applied.
constexpr uintptr_t CASCADE_FIT_RVA = 0x2A23AC;
using CascadeFitFn = __int64(__fastcall*)(void*, void*, void*, uint8_t*, void*);
CascadeFitFn g_orig_cascade_fit = nullptr;

// The union window in the descriptor's terms, or false to leave the fit alone.
bool CascadeUnion(const uint8_t* camera, int side, float out[4]) {
    if (!CyberpunkVR_AsymCascadeUnion || CyberpunkVR_AsymProjection != 1 || !g_side[side].active) return false;
    AsymEyeWindows w{};
    if (!OpenXRManager::Get().GetAsymmetricEyeWindows(&w)) return false;
    const int mainEye = CyberpunkVR_MainIsRightEye ? 1 : 0;
    const cvr::projection::EyeWindow& own = w.eye[side == 0 ? mainEye : 1 - mainEye];
    const float* d = reinterpret_cast<const float*>(camera);
    const float fov = d[0x20 / 4], aspect = d[0x28 / 4], sx = d[0x30 / 4], sy = d[0x34 / 4];
    if (!(fov > 1.0f && fov < 179.0f) || !(aspect > 0.05f && aspect < 20.0f)) return false;
    // The descriptor must still hold the window this eye was given, or the union would be built for some
    // other frustum.
    const cvr::projection::EngineProjection given = cvr::projection::ToEngine(own);
    if (std::fabs(sx - given.shiftX) > 1e-3f || std::fabs(sy - given.shiftY) > 1e-3f ||
        std::fabs(aspect - given.aspect) > 1e-3f * given.aspect) return false;
    const float k = std::tan(fov * 0.5f * 0.01745329252f) / (0.5f * (own.u - own.d));
    const cvr::projection::EyeWindow u{k * std::fmin(w.eye[0].l, w.eye[1].l), k * std::fmax(w.eye[0].r, w.eye[1].r),
                                      k * std::fmin(w.eye[0].d, w.eye[1].d), k * std::fmax(w.eye[0].u, w.eye[1].u)};
    const cvr::projection::EngineProjection p = cvr::projection::ToEngine(u);
    if (!(p.vfovDeg > 1.0f && p.vfovDeg < 179.0f) || !(p.aspect > 0.05f && p.aspect < 20.0f) ||
        !std::isfinite(p.shiftX) || !std::isfinite(p.shiftY)) return false;
    out[0] = p.vfovDeg; out[1] = p.aspect; out[2] = p.shiftX; out[3] = p.shiftY;
    return true;
}

// Game 2.5's caller (sub_140C0F204) hands the fit a copy of the scene descriptor on its own stack (copied by
// sub_1402130D0, converted by sub_14206EAD0 when the descriptor's extra projection mode is on) and the view's
// +0x2390 as the fifth argument, so the view is found from that one. 2.31 passed view+0x70 itself.
constexpr size_t kCascadeFitViewArg = 0x2390;

__int64 __fastcall Detour_CascadeFit(void* records, void* a2, void* a3, uint8_t* camera, void* a5) {
    // The fit copies the descriptor before using it and reads nothing else of it but the cascade count,
    // so a patched copy is all it needs; the view's own descriptor is never written.
    alignas(16) uint8_t copy[kDescStride];
    uint8_t* use = camera;
    int side = -1;
    if (camera && a5) {
        __try {
            const uint8_t* view = static_cast<const uint8_t*>(a5) - kCascadeFitViewArg;
            const uint64_t key = *reinterpret_cast<const uint64_t*>(view + 0x28);
            if (key != 0 && key == g_vrcam_ctx_key.load(std::memory_order_relaxed)) side = 1;
            else if (is_main_view(view)) side = 0;
            float u[4];
            if (side >= 0 && CascadeUnion(camera, side, u)) {
                std::memcpy(copy, camera, kDescStride);
                float* f = reinterpret_cast<float*>(copy);
                f[0x20 / 4] = u[0]; f[0x28 / 4] = u[1]; f[0x30 / 4] = u[2]; f[0x34 / 4] = u[3];
                std::memcpy(CyberpunkVR_DebugAsymCascadeWindow + side * 4, u, sizeof(u));
                use = copy;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { use = camera; }
    }
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugAsymCascadeFits[use != camera ? side : 2]);
    return g_orig_cascade_fit(records, a2, a3, use, a5);
}

}  // namespace

CVR_DETOUR("[asym] view prepare sub_1404240DC", VIEW_PREPARE_RVA, Detour_ViewPrepare, g_orig_view_prepare)
CVR_DETOUR("[asym] plane override sub_140422D2C", PLANE_OVERRIDE_RVA, Detour_PlaneOverride, g_orig_plane_override)
CVR_DETOUR("[asym] cascade fit sub_1402A23AC", CASCADE_FIT_RVA, Detour_CascadeFit, g_orig_cascade_fit)

}  // namespace cvr::detail

namespace cvr::projection {
bool ViewWindowOf(int side, ViewWindow* out) {
    if (side < 0 || side > 1 || !out) return false;
    const auto& s = cvr::detail::g_side[side];
    if (!s.active) return false;
    *out = {s.wroteX, s.wroteY, s.halfX, s.halfY};
    return std::isfinite(out->shiftX) && std::isfinite(out->shiftY) && out->halfX > 0.01f && out->halfY > 0.01f &&
           std::isfinite(out->halfX) && std::isfinite(out->halfY);
}
}  // namespace cvr::projection
