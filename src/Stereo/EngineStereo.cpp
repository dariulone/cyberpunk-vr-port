// EngineStereo -- the engine's own two-eye frame as the headset's stereo. What the mode changes and where is
// in Stereo/EngineStereo.hpp; why, and the measurements, in docs/engine-stereo-plan.md. This file holds the
// switch and the three numbers the engine asks the Odyssey 3D SDK wrapper for.
//
// WHERE THE EYES COME FROM (IDA, 2.5). The view producer sub_1404240DC, when render feature 98 is on, asks the
// platform for eye separation (vfunc +1088 -> sub_142284990, wrapper float +0x570), the first-person plane's
// separation (+1096 -> sub_1422849D0, +0x57C) and convergence (+1104 -> sub_142284950, +0x56C); the first-person
// plane's convergence is the cvar float dword_143D31790. The camera build sub_14206EC30 clones each plane's
// descriptor into a left (kind 1, +0x3D0) and a right (kind 2, +0x3D8) camera at centre -/+ 0.5 * sep * right
// (camera+0x250), in metres, and the matrix builder sub_140296EA0 takes their projection as the centre's
// symmetric one with the off-axis terms P20/P21 of a frustum converged at `conv`: P20 = zoom * (+-sep/2) /
// (2 * tan(fov/2) * conv). The camera's own NDC shift (+0x30/+0x34) is applied after that, so the port's
// asymmetric window still reaches both eyes through the clones.
//
// The comfort values the SDK would hand out (sep a fraction of the IPD, near convergence, a weaker separation
// on the weapon) are made for a monitor. A headset needs the wearer's own: sep = the IPD the rest of the port
// uses (GetDesiredHalfIpd, world scale and taste multipliers included), convergence parallel, the weapon the
// same as the world.

#include "Stereo/EngineStereo.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Core/CoreInternal.hpp"

#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstddef>
#include <cstdlib>
#include <cstring>
#include <iterator>
#include <mutex>

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/inkLayer.hpp>

extern void Log(const char* fmt, ...);
float GetDesiredHalfIpd();
extern int g_launcherWidth;    // one eye's render size (Core/VrCore.cpp)
extern int g_launcherHeight;
extern "C" __declspec(dllexport) extern int CyberpunkVR_IpdInWorldPos;
extern "C" void CyberpunkVR_SetVrcamEnabled(uint32_t on);   // src/Stereo/SyncStereo.cpp

extern "C" {
__declspec(dllexport) int32_t CyberpunkVR_EngineStereo = 0;
__declspec(dllexport) int32_t CyberpunkVR_MonoMode = 0;
__declspec(dllexport) int32_t CyberpunkVR_MonoView = 0;
__declspec(dllexport) int32_t CyberpunkVR_EngineStereoEyeWindows = 1;
// 1 = the eye cameras turn the TAA jitter into NDC with their own width (half the view's): each eye's samples
// cover a whole pixel, as the engine's pixel jitter means. 0 = the view's width, as the engine hands it (an eye
// jitters over half a pixel horizontally). Live: the size is written at every jittered build.
__declspec(dllexport) int32_t CyberpunkVR_EngineStereoEyeJitterWidth = 0;
// MAIN frames prepared with the engine's stereo off / on.
__declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoFrames[2] = {};
// What the engine was last handed: separation, first-person separation, convergence.
__declspec(dllexport) float CyberpunkVR_DebugEngineStereoValues[3] = {};
// Eye camera builds given their own window: left, right.
__declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoEyeBuilds[2] = {};
// The last eye camera built, per eye: aspect, shift x, shift y as given; P00, P11, P20, P21 as built.
__declspec(dllexport) float CyberpunkVR_DebugEngineStereoEyes[2][7] = {};
// The last eye camera built, per eye: its jitter x, y in pixels and the width, height that turned them into NDC.
__declspec(dllexport) float CyberpunkVR_DebugEngineStereoEyeJitter[2][4] = {};
// First-person plane eyes put back at their own place after the plane's override rebuild: left, right.
__declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoPlaneEyes[2] = {};
// 1 (default) = ink lays the UI out in one eye's window (UiOneEye); 0 = in the whole window, as the engine does.
// Read when a layer is resized: at start-up, and when the window changes size.
__declspec(dllexport) int32_t CyberpunkVR_EngineStereoUiOneEye = 1;
// Layer resizes handed one eye's size / passed as they came; the last size asked for and the size given.
__declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoUiResizes[2] = {};
__declspec(dllexport) int32_t CyberpunkVR_DebugEngineStereoUiSize[4] = {};
// Layer ticks handed the layer's one-eye size as their screen (Detour_LayerTick).
__declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoUiTicks = 0;
// 1 (default) = the loading screen is shown as one picture on the menu panel while its layer is up (LoadingShown).
__declspec(dllexport) int32_t CyberpunkVR_EngineStereoLoadingPanel = 1;
__declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoLoadingTicks = 0;
}

namespace cvr::engine_stereo {
namespace {

constexpr uintptr_t SEPARATION_RVA = 0x2284990;
constexpr uintptr_t WEAPON_SEPARATION_RVA = 0x22849D0;
constexpr uintptr_t CONVERGENCE_RVA = 0x2284950;
constexpr uintptr_t WEAPON_CONVERGENCE_RVA = 0x3D31790;   // .data float, read only by sub_1404240DC
// The view's render feature bits (sub_1404240DC stores them at view+0x25B0); 98 is the stereo frame.
constexpr size_t kViewFeatures = 0x25B0;
constexpr int kStereoFeature = 98;
// Far enough that the off-axis term is noise (P20 ~ 1e-8): both eyes look straight ahead, as a headset's do.
constexpr float kParallel = 1.0e6f;

using FloatGetterFn = float(__fastcall*)(void*);
FloatGetterFn g_orig_separation = nullptr;
FloatGetterFn g_orig_weapon_separation = nullptr;
FloatGetterFn g_orig_convergence = nullptr;

std::atomic<int> g_rendering{-1};
// xr_engine_stereo as the ini asks for it, before the mono mode has its say.
bool g_requested = false;
// The launch started in the mono mode.
bool g_monoLaunch = false;

bool ReadSwitch() {
    InitRuntimePaths();
    // On unless the ini says 0: a fresh install's ini has no such key, and a 2.5 install belongs in engine stereo.
    int value = 1, mono = 0;
    if (FILE* file = _fsopen(g_liveControlPath, "r", _SH_DENYNO)) {
        char line[256];
        while (fgets(line, sizeof(line), file)) {
            int v = 0;
            if (sscanf_s(line, "xr_engine_stereo=%d", &v) == 1 || sscanf_s(line, "xr_engine_stereo = %d", &v) == 1)
                value = v != 0 ? 1 : 0;
            if (sscanf_s(line, "xr_mono_mode=%d", &v) == 1 || sscanf_s(line, "xr_mono_mode = %d", &v) == 1)
                mono = v != 0 ? 1 : 0;
        }
        fclose(file);
    }
    g_requested = value != 0;
    g_monoLaunch = mono != 0;
    CyberpunkVR_MonoMode = mono;
    CyberpunkVR_MonoView = mono;
    // THE MONO MODE STARTS WITHOUT ENGINE STEREO: one picture for both eyes, the window one eye wide.
    if (mono && value) {
        value = 0;
        Log("[engine-stereo] OFF for this launch: xr_mono_mode=1 (one picture for both eyes)\n");
    }
    CyberpunkVR_EngineStereo = value;
    if (value) {
        // MAIN is the head centre: the engine adds the eye offsets itself.
        CyberpunkVR_IpdInWorldPos = 0;
        Log("[engine-stereo] ON (xr_engine_stereo=1): engine side-by-side frame, VRCAM off, MAIN at the head centre\n");
    }
    return value != 0;
}

float Separation() {
    const float sep = 2.0f * GetDesiredHalfIpd();
    return sep > 0.0f && sep < 1.0f ? sep : 0.064f;
}

float __fastcall Detour_Separation(void*) {
    const float sep = Separation();
    CyberpunkVR_DebugEngineStereoValues[0] = sep;
    return sep;
}

float __fastcall Detour_WeaponSeparation(void*) {
    const float sep = Separation();
    CyberpunkVR_DebugEngineStereoValues[1] = sep;
    return sep;
}

float __fastcall Detour_Convergence(void*) {
    // The first-person plane's convergence is a cvar the producer reads right after this call.
    if (detail::g_exe_base) *reinterpret_cast<volatile float*>(detail::g_exe_base + WEAPON_CONVERGENCE_RVA) = kParallel;
    CyberpunkVR_DebugEngineStereoValues[2] = kParallel;
    return kParallel;
}

// EACH EYE ITS OWN LENS WINDOW. A headset's two windows are mirror images (Quest 3: -54/+40 against -40/+54 deg);
// one shared window has to be their union, 108 deg wide, and square pixels then grow it 21 % taller than the lens.
// The eyes are clones of MAIN's descriptors (sub_14206EC30), and the matrix builder applies a camera's own aspect
// and NDC shift whatever its kind, the shift after the stereo branch -- so each eye camera is handed its own
// aspect and shift right before it is built. Both are ratios of the frustum's own span: the same numbers are
// right for every plane, the first-person one with its own fov included, and the same on every rebuild (jitter,
// history), so writing them each time is idempotent. The vertical fov stays the clone's, MAIN's: the eyes share
// one vertical span (Runtimes/AsymmetricProjection.hpp Windows()), which is what MAIN was given.
constexpr uintptr_t CAMERA_BUILD_RVA = 0x296EA0;
constexpr size_t kAspect = 0x28, kShiftX = 0x30, kShiftY = 0x34;
constexpr size_t kEyeKind = 0x398;    // 1 left, 2 right (sub_14206EC10)
constexpr size_t kEyeFlags = 0x399;   // bit 1: an eye camera, built off-axis
constexpr size_t kProjection = 0x190; // P, row-major: P00 +0x190, P11 +0x1A4, P20 +0x1B0, P21 +0x1B4
// The TAA jitter, in pixels, and the size the builder turns it into NDC with (sub_140C28EC0: 2 j / size). The
// view prepare hands the view's size (view+52/+56) to the centre and both clones alike (sub_140424D08).
constexpr size_t kJitter = 0x370, kJitterSize = 0x378;

using CameraBuildFn = __int64(__fastcall*)(uint8_t*);
CameraBuildFn g_orig_camera_build = nullptr;

struct EyeSlot { std::atomic<float> aspect{0.0f}, shiftX{0.0f}, shiftY{0.0f}; };
EyeSlot g_eyes[2];
std::atomic<bool> g_eyesValid{false};
std::atomic<uint32_t> g_jitterWidth{0}, g_jitterHeight{0};
std::atomic<uint32_t> g_viewJitterWidth{0}, g_viewJitterHeight{0};   // the centre's (the view's) size, built right before its eyes

__int64 __fastcall Detour_CameraBuild(uint8_t* camera) {
    int eye = -1;
    if (camera) {
        uint32_t* size = reinterpret_cast<uint32_t*>(camera + kJitterSize);
        if ((camera[kEyeFlags] & 1) && size[0] > 1 && size[1] > 1) {
            g_viewJitterWidth.store(size[0], std::memory_order_relaxed);
            g_viewJitterHeight.store(size[1], std::memory_order_relaxed);
        }
        // Only the view's own width is halved: the plane and eye rebuilds keep the size they were last given.
        const uint32_t view = g_viewJitterWidth.load(std::memory_order_relaxed);
        if ((camera[kEyeFlags] & 2) && CyberpunkVR_EngineStereoEyeJitterWidth && view > 1 && size[0] == view)
            size[0] = view / 2;
    }
    if (camera && (camera[kEyeFlags] & 2) && (camera[kEyeKind] == 1 || camera[kEyeKind] == 2) &&
        CyberpunkVR_EngineStereoEyeWindows && g_eyesValid.load(std::memory_order_acquire)) {
        eye = camera[kEyeKind] - 1;
        float* f = reinterpret_cast<float*>(camera);
        f[kAspect / 4] = g_eyes[eye].aspect.load(std::memory_order_relaxed);
        f[kShiftX / 4] = g_eyes[eye].shiftX.load(std::memory_order_relaxed);
        f[kShiftY / 4] = g_eyes[eye].shiftY.load(std::memory_order_relaxed);
    }
    const __int64 r = g_orig_camera_build(camera);
    if (eye >= 0) {
        ++CyberpunkVR_DebugEngineStereoEyeBuilds[eye];
        const float* f = reinterpret_cast<const float*>(camera);
        float* d = CyberpunkVR_DebugEngineStereoEyes[eye];
        d[0] = f[kAspect / 4]; d[1] = f[kShiftX / 4]; d[2] = f[kShiftY / 4];
        d[3] = f[kProjection / 4]; d[4] = f[kProjection / 4 + 5]; d[5] = f[kProjection / 4 + 8]; d[6] = f[kProjection / 4 + 9];
        // Builds without jitter (the history camera, rebuilds) carry a size of 0 or 1: only a real one is kept.
        const uint32_t* size = reinterpret_cast<const uint32_t*>(camera + kJitterSize);
        if (size[0] > 1 && size[1] > 1) {
            g_jitterWidth.store(size[0], std::memory_order_relaxed);
            g_jitterHeight.store(size[1], std::memory_order_relaxed);
        }
        float* j = CyberpunkVR_DebugEngineStereoEyeJitter[eye];
        j[0] = f[kJitter / 4]; j[1] = f[kJitter / 4 + 1]; j[2] = float(size[0]); j[3] = float(size[1]);
    }
    return r;
}

// THE FIRST-PERSON PLANE'S EYES WERE MOVED BACK TO ITS CENTRE. A plane with its own projection (plane 2, the
// override the producer hands to sub_140422D2C; Stereo/AsymProjection.cpp) is rebuilt from it after the eyes were
// cloned (sub_14042579C -> sub_14206EC30, centre -/+ sep/2 along the right vector), and the rebuild
// (sub_140422AA4) writes the CENTRE descriptor's position into both clones before building them. The arms and the
// gun were drawn from one point for both eyes, with only the convergence's off-axis term between them: a flat
// picture at the convergence distance -- the monitor's design (0.5 m), at infinity with parallel convergence
// (2026-10-23: the hands go mono with a weapon, are stereo without). The clones' own positions are kept
// across the rebuild and the eyes built again there; what the rebuild changed besides (fov, near/far, the
// window) stays. The orientation the rebuild writes is the centre's, which the clones had already.
constexpr uint8_t kHasEyes = 1;                 // a centre descriptor's flags (+0x399): it has the clones below
constexpr size_t kLeftEye = 0x3D0, kRightEye = 0x3D8;
constexpr size_t kPosition = 0x0;               // int x, y, z in 1/131072 m
constexpr size_t kWorldPosition = 0x280;        // the same as float x, y, z, 1

// THE UI IS LAID OUT FOR ONE EYE. Ink sizes every layer to the window (inkLayer::Resize, sub_14132654C, the one
// slot every layer class shares: the layer's size +0x30, its virtual window's, and the HUD entries' scale
// min(w / 3840, h / 2160)). Two eyes wide, the HUD's design came out 1.326 pixels per unit, bound by the height,
// where one eye's window fits it by the width at 0.690: every element, marker, subtitle and damage number 1.92x
// the size it has in the port's other modes, the design area 5091 pixels across of which each eye shows the middle
// 2648. The game positions projected widgets in that same layout, so the layout is the one place to change: handed
// one eye's window, ink lays out everything as in the one-eye modes, and each eye's UI pass draws that one-eye
// canvas into its own half (Hooks/WorldMarkers.cpp InkShift).
// Only the layers each eye's UI pass draws: the HUD and the game's notifications. A menu, a loading screen, the
// intro -- drawn once by the frame's centre over the whole window and shown to both eyes as one picture on a panel
// (Runtimes/OpenXRCapture.cpp) -- keep the window; laid out in one eye's they filled its left half, a tall picture
// with the menu across its middle and nothing above or below (2026-10-24).
// A layer drawn into a target of its own (the HUD, menus, notifications) is sized with it instead, by the slot
// after (sub_140399094: the size, the target, the virtual window's size) -- the same rule there.
constexpr uintptr_t LAYER_RESIZE_RVA = 0x132654C;
constexpr uintptr_t LAYER_TARGET_RVA = 0x399094;
using LayerResizeFn = __int64(__fastcall*)(void*, int, int);
using LayerTargetFn = __int64(__fastcall*)(void*, int, int, void*);
LayerResizeFn g_orig_layer_resize = nullptr;
LayerTargetFn g_orig_layer_target = nullptr;
std::atomic<bool> g_uiOneEye{false};

bool TwoEyesWide(int width, int height) {
    if (!Enabled() || g_launcherWidth <= 0 || g_launcherHeight <= 0 || width <= 0 || height <= 0) return false;
    const int64_t eyeWidth = int64_t(height) * g_launcherWidth / g_launcherHeight;
    return std::llabs(int64_t(width) - 2 * eyeWidth) <= 2;
}

bool DrawnPerEye(void* layer) {
    if (!layer) return false;
    const RED4ext::CName name = static_cast<RED4ext::ink::Layer*>(layer)->GetType()->GetName();
    return name == RED4ext::CName("inkHUDLayer") || name == RED4ext::CName("inkGameNotificationsLayer");
}

// The width a layer is given: one eye's of a window two eyes wide, for a layer drawn per eye while the port lays
// the UI out for one eye.
int LayerWidth(void* layer, int width, int height) {
    if (CyberpunkVR_EngineStereoUiOneEye && TwoEyesWide(width, height) && DrawnPerEye(layer)) {
        ++CyberpunkVR_DebugEngineStereoUiResizes[0];
        CyberpunkVR_DebugEngineStereoUiSize[0] = width; CyberpunkVR_DebugEngineStereoUiSize[1] = height;
        CyberpunkVR_DebugEngineStereoUiSize[2] = width / 2; CyberpunkVR_DebugEngineStereoUiSize[3] = height;
        if (!g_uiOneEye.exchange(true, std::memory_order_relaxed))
            Log("[engine-stereo] UI laid out for one eye: per-eye ink layers %dx%d instead of %dx%d\n", width / 2,
                height, width, height);
        return width / 2;
    }
    ++CyberpunkVR_DebugEngineStereoUiResizes[1];
    if (TwoEyesWide(width, height) && DrawnPerEye(layer)) g_uiOneEye.store(false, std::memory_order_relaxed);
    return width;
}

__int64 __fastcall Detour_LayerResize(void* layer, int width, int height) {
    return g_orig_layer_resize(layer, LayerWidth(layer, width, height), height);
}

__int64 __fastcall Detour_LayerTarget(void* layer, int width, int height, void* target) {
    return g_orig_layer_target(layer, LayerWidth(layer, width, height), height, target);
}

// AND ITS CONTROLLERS ARE TICKED WITH IT. A layer's tick (sub_14060787C, inkLayer::ActiveTick) hands every
// controller of the layer the screen it places things in -- projections (sub_1402A4774), mappins -- as the tick's
// size (+16): the render window's, two eyes wide, not the layer's. A projected widget then landed at its point
// in a screen twice the canvas's width. The layer's own size (+0x30) is the screen its canvas is.
//
// AND THE CAMERA THEY PLACE WORLD POINTS WITH IS ONE EYE'S. The tick's view (+8; sub_1409D63FC builds it, 16 bytes:
// the vtable 0x3620AA8 and the gameCameraSystem) is that camera for every controller: projections (sub_1402A4B18)
// and mappins (sub_14054A890) go through its world-to-screen (slot 13, sub_14054AD18 -> the camera system's +720),
// its other projection (slot 16, sub_142144370 -> +712) and back (slot 14, sub_1429F403C -> +728). The camera is
// the game's, its frustum the window's: 100.3 degrees high, aspect 1.849, tan 2.2155 to each side -- two eye
// windows across one eye's canvas. Everything placed off the canvas's centre came out at half its angle in the eye
// and a marker clamped to the screen's edge sat towards the middle of the eye (2026-10-24: the car's marker,
// turned away from it). The layer gets a view of its own on the same camera system, narrowed to the layer: NDC
// across times the window's width over the layer's (2), the canvas's pixels as square in the eye as on it.
constexpr uintptr_t LAYER_TICK_RVA = 0x60787C;
constexpr uintptr_t UI_VIEW_VTABLE_RVA = 0x3620AA8;
constexpr uintptr_t VIEW_PROJECT_RVA = 0x54AD18, VIEW_PROJECT_OTHER_RVA = 0x2144370, VIEW_UNPROJECT_RVA = 0x29F403C;
constexpr size_t kLayerSize = 0x30;
struct LayerTick { void* time; void* view; float size[2]; uint8_t flag; uint8_t pad[7]; };
static_assert(offsetof(LayerTick, view) == 8 && offsetof(LayerTick, size) == 16 && offsetof(LayerTick, flag) == 24);
using LayerTickFn = __int64(__fastcall*)(void*, const LayerTick*, void*);
LayerTickFn g_orig_layer_tick = nullptr;

void NoteLayerTick(void* layer);

// The view's own two fields, then the port's. The tick's jobs keep the pointer past the tick, so these live on;
// one per camera system.
struct OneEyeView { void* vtable; void* cameraSystem; float narrow; };
OneEyeView g_oneEyeViews[4]{};
size_t g_nextOneEyeView = 0;
std::mutex g_oneEyeViewsLock;

const OneEyeView* AsOneEyeView(const void* view) {
    const auto* own = static_cast<const OneEyeView*>(view);
    return own >= g_oneEyeViews && own < g_oneEyeViews + std::size(g_oneEyeViews) ? own : nullptr;
}

void* OneEyeViewOf(void* view, float narrow) {
    if (!view || !detail::g_exe_base || AsOneEyeView(view)) return view;
    void* const vtable = *static_cast<void**>(view);
    void* const cameraSystem = static_cast<void**>(view)[1];
    if (static_cast<uint8_t*>(vtable) != detail::g_exe_base + UI_VIEW_VTABLE_RVA || !cameraSystem) return view;
    std::lock_guard lock(g_oneEyeViewsLock);
    OneEyeView* own = nullptr;
    for (auto& candidate : g_oneEyeViews)
        if (candidate.cameraSystem == cameraSystem) own = &candidate;
    if (!own) own = &g_oneEyeViews[g_nextOneEyeView++ % std::size(g_oneEyeViews)];
    if (own->cameraSystem != cameraSystem || own->narrow != narrow) *own = {vtable, cameraSystem, narrow};
    return own;
}

__int64 __fastcall Detour_LayerTick(void* layer, const LayerTick* tick, void* jobs) {
    NoteLayerTick(layer);
    if (layer && tick && g_uiOneEye.load(std::memory_order_relaxed)) {
        const float* own = reinterpret_cast<const float*>(static_cast<uint8_t*>(layer) + kLayerSize);
        if (own[0] > 0.0f && own[1] == tick->size[1] && TwoEyesWide(int(tick->size[0]), int(tick->size[1])) &&
            !TwoEyesWide(int(own[0]), int(own[1]))) {
            LayerTick one = *tick;
            one.size[0] = own[0];
            one.view = OneEyeViewOf(tick->view, tick->size[0] / own[0]);
            ++CyberpunkVR_DebugEngineStereoUiTicks;
            return g_orig_layer_tick(layer, &one, jobs);
        }
    }
    return g_orig_layer_tick(layer, tick, jobs);
}

using ViewProjectFn = float*(__fastcall*)(void*, float*, const void*);
using ViewUnprojectFn = void*(__fastcall*)(void*, void*, const float*);
ViewProjectFn g_orig_view_project = nullptr;
ViewProjectFn g_orig_view_project_other = nullptr;
ViewUnprojectFn g_orig_view_unproject = nullptr;

float* __fastcall Detour_ViewProject(void* view, float* ndc, const void* world) {
    float* out = g_orig_view_project(view, ndc, world);
    if (const OneEyeView* own = AsOneEyeView(view); own && out) out[0] *= own->narrow;
    return out;
}

float* __fastcall Detour_ViewProjectOther(void* view, float* ndc, const void* world) {
    float* out = g_orig_view_project_other(view, ndc, world);
    if (const OneEyeView* own = AsOneEyeView(view); own && out) out[0] *= own->narrow;
    return out;
}

void* __fastcall Detour_ViewUnproject(void* view, void* world, const float* ndc) {
    if (const OneEyeView* own = AsOneEyeView(view); own && ndc) {
        const float camera[2] = {ndc[0] / own->narrow, ndc[1]};
        return g_orig_view_unproject(view, world, camera);
    }
    return g_orig_view_unproject(view, world, ndc);
}

// THE LOADING SCREEN IS ONE PICTURE, AS A MENU IS. The loading layer keeps the window, but the menu state the port
// follows (Hooks/MenuMode.cpp) is already the game's while its last screen waits for the button: the frame was split
// into the eyes and each showed half of the loading screen (2026-10-24). The layer's own state says when it is
// up (inkLayer::active, +0xB0); it is read only while the layer is being ticked.
// How it is drawn follows the frame: over a mono frame (the world not there yet) once across the whole window; once
// the world under it renders both eyes (feature 98, the screen waiting for the button), by each eye's UI pass into
// its own half, the whole screen squeezed to half the width -- squeezed again, the panel showed two narrow copies
// side by side (LoadingSideBySide).
constexpr size_t kLayerActive = 0xB0;
std::atomic<void*> g_loadingLayer{nullptr};
std::atomic<uint64_t> g_loadingTick{0};

void NoteLayerTick(void* layer) {
    if (!layer || static_cast<RED4ext::ink::Layer*>(layer)->GetType()->GetName() != RED4ext::CName("inkLoadingLayer"))
        return;
    g_loadingLayer.store(layer, std::memory_order_relaxed);
    g_loadingTick.store(GetTickCount64(), std::memory_order_relaxed);
    ++CyberpunkVR_DebugEngineStereoLoadingTicks;
}

bool LayerActive(const uint8_t* layer) {
    __try {
        return layer[kLayerActive] != 0;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

}  // namespace

bool UiOneEye() { return Enabled() && g_uiOneEye.load(std::memory_order_relaxed); }

bool LoadingShown() {
    if (!Enabled() || !CyberpunkVR_EngineStereoLoadingPanel ||
        GetTickCount64() - g_loadingTick.load(std::memory_order_relaxed) > 250)
        return false;
    const auto* layer = static_cast<const uint8_t*>(g_loadingLayer.load(std::memory_order_relaxed));
    return layer && LayerActive(layer);
}

bool OnePicture() { return Enabled() && (!Rendering() || MainMenuUp() || LoadingShown()); }

bool LoadingSideBySide() { return Enabled() && Rendering() && !MainMenuUp() && LoadingShown(); }

float UiViewNarrowing(const void* view) {
    const OneEyeView* own = AsOneEyeView(view);
    return own ? own->narrow : 1.0f;
}

EyePlaces KeepEyePlaces(uint8_t* descriptor) {
    EyePlaces kept;
    if (!Enabled() || !descriptor || !(descriptor[kEyeFlags] & kHasEyes)) return kept;
    for (int e = 0; e < 2; ++e) {
        uint8_t* eye = *reinterpret_cast<uint8_t* const*>(descriptor + (e ? kRightEye : kLeftEye));
        if (!eye) return kept;
        kept.eye[e] = eye;
        std::memcpy(kept.position[e], eye + kPosition, sizeof(kept.position[e]));
        std::memcpy(kept.world[e], eye + kWorldPosition, sizeof(kept.world[e]));
    }
    kept.valid = true;
    return kept;
}

void RestoreEyePlaces(const EyePlaces& kept) {
    if (!kept.valid || !detail::g_exe_base) return;
    // The engine's entry, so the eye-window detour gives the eye its window again.
    const auto build = reinterpret_cast<CameraBuildFn>(detail::g_exe_base + CAMERA_BUILD_RVA);
    for (int e = 0; e < 2; ++e) {
        uint8_t* eye = kept.eye[e];
        // Unmoved when the plane already matched its override (sub_140422A3C) and was not rebuilt.
        if (std::memcmp(eye + kPosition, kept.position[e], sizeof(kept.position[e])) == 0) continue;
        std::memcpy(eye + kPosition, kept.position[e], sizeof(kept.position[e]));
        std::memcpy(eye + kWorldPosition, kept.world[e], sizeof(kept.world[e]));
        build(eye);
        ++CyberpunkVR_DebugEngineStereoPlaneEyes[e];
    }
}

void PublishEyes(const EyeProjection eyes[2], bool valid) {
    if (valid) {
        for (int e = 0; e < 2; ++e) {
            g_eyes[e].aspect.store(eyes[e].aspect, std::memory_order_relaxed);
            g_eyes[e].shiftX.store(eyes[e].shiftX, std::memory_order_relaxed);
            g_eyes[e].shiftY.store(eyes[e].shiftY, std::memory_order_relaxed);
        }
    }
    g_eyesValid.store(valid, std::memory_order_release);
}

bool EyeWindow(int eye, EyeProjection* out) {
    if (eye < 0 || eye > 1 || !out || !CyberpunkVR_EngineStereoEyeWindows || !g_eyesValid.load(std::memory_order_acquire))
        return false;
    out->aspect = g_eyes[eye].aspect.load(std::memory_order_relaxed);
    out->shiftX = g_eyes[eye].shiftX.load(std::memory_order_relaxed);
    out->shiftY = g_eyes[eye].shiftY.load(std::memory_order_relaxed);
    return out->aspect > 0.05f;
}

float HudDesignRatio(float width, float height) {
    if (!Enabled() || g_launcherWidth <= 0 || g_launcherHeight <= 0 || !(width > 0) || !(height > 0)) return 1.0f;
    // A root two eyes wide (the window); anything else is not the HUD this is about.
    const float eyeWidth = height * float(g_launcherWidth) / float(g_launcherHeight);
    if (std::fabs(width - 2.0f * eyeWidth) > 0.01f * width) return 1.0f;
    const float own = std::fmin(width / 3840.0f, height / 2160.0f), eye = std::fmin(eyeWidth / 3840.0f, height / 2160.0f);
    return own > 0.0f && eye > 0.0f ? eye / own : 1.0f;
}

float EyeSeparation() { return Separation(); }

bool Enabled() {
    static const bool enabled = ReadSwitch();
    return enabled;
}

bool Requested() {
    (void)Enabled();
    return g_requested;
}

bool MonoLaunch() {
    (void)Enabled();
    return g_monoLaunch;
}

void ApplyMonoMode(bool mono) {
    CyberpunkVR_MonoMode = mono ? 1 : 0;
    // Engine stereo is running: this launch keeps it, and the switch takes effect at the next start
    // (RestartPending). The port's second view does not exist here to be switched.
    if (Enabled()) return;
    // A launch that started in mono stays mono: the second view's machinery is installed only for a launch that
    // may render one (SecondViewWanted), so there is nothing to go back to until the next start.
    const bool view = mono || g_monoLaunch;
    static int s_applied = -1;
    if (s_applied == (view ? 1 : 0)) return;
    s_applied = view ? 1 : 0;
    CyberpunkVR_MonoView = view ? 1 : 0;
    // MAIN at the head centre (the one picture is drawn from between the eyes), and the second view off. Written on
    // the first apply as well: the VRCAM request outlives a launch in the CET bridge, and a launch in stereo after
    // one in mono must not inherit "off".
    CyberpunkVR_IpdInWorldPos = view ? 0 : 1;
    CyberpunkVR_SetVrcamEnabled(view ? 0u : 1u);
    Log("[mono] %s\n", view ? "ON: one picture from the head centre for both eyes, VRCAM off"
                            : "OFF: MAIN is one eye, VRCAM the other");
}

bool Rendering() {
    return g_rendering.load(std::memory_order_relaxed) == 1;
}

bool EyeJitterSize(uint32_t* width, uint32_t* height) {
    *width = g_jitterWidth.load(std::memory_order_relaxed);
    *height = g_jitterHeight.load(std::memory_order_relaxed);
    return *width > 1 && *height > 1;
}

bool EyeSize(uint32_t* width, uint32_t* height) {
    *width = g_viewJitterWidth.load(std::memory_order_relaxed) / 2;
    *height = g_viewJitterHeight.load(std::memory_order_relaxed);
    return *width > 1 && *height > 1;
}

void NoteMainPrepared(const uint8_t* view) {
    if (!view || !Enabled()) return;
    const uint64_t bits = *reinterpret_cast<const uint64_t*>(view + kViewFeatures + (kStereoFeature / 64) * 8);
    const int on = ((bits >> (kStereoFeature % 64)) & 1) ? 1 : 0;
    ++CyberpunkVR_DebugEngineStereoFrames[on];
    if (g_rendering.exchange(on, std::memory_order_relaxed) != on) {
        if (on) Log("[engine-stereo] the engine renders both eyes (feature 98 on)\n");
        else Log("[engine-stereo] the engine renders ONE eye (feature 98 off)%s -- the headset shows it as one picture\n",
                 Armed() ? "" : ": before the main menu");
    }
}

CVR_DETOUR_IF("[engine-stereo] eye windows sub_140296EA0", CAMERA_BUILD_RVA, Detour_CameraBuild, g_orig_camera_build,
              &Enabled)
CVR_DETOUR_IF("[engine-stereo] one-eye UI inkLayer::Resize sub_14132654C", LAYER_RESIZE_RVA, Detour_LayerResize,
              g_orig_layer_resize, &Enabled)
CVR_DETOUR_IF("[engine-stereo] one-eye UI layer target sub_140399094", LAYER_TARGET_RVA, Detour_LayerTarget,
              g_orig_layer_target, &Enabled)
CVR_DETOUR_IF("[engine-stereo] one-eye UI layer tick sub_14060787C", LAYER_TICK_RVA, Detour_LayerTick,
              g_orig_layer_tick, &Enabled)
CVR_DETOUR_IF("[engine-stereo] one-eye UI view projection sub_14054AD18", VIEW_PROJECT_RVA, Detour_ViewProject,
              g_orig_view_project, &Enabled)
CVR_DETOUR_IF("[engine-stereo] one-eye UI view projection sub_142144370", VIEW_PROJECT_OTHER_RVA,
              Detour_ViewProjectOther, g_orig_view_project_other, &Enabled)
CVR_DETOUR_IF("[engine-stereo] one-eye UI view unprojection sub_1429F403C", VIEW_UNPROJECT_RVA, Detour_ViewUnproject,
              g_orig_view_unproject, &Enabled)
CVR_DETOUR_IF("[engine-stereo] convergence sub_142284950", CONVERGENCE_RVA, Detour_Convergence, g_orig_convergence,
              &Enabled)
CVR_DETOUR_IF("[engine-stereo] separation sub_142284990", SEPARATION_RVA, Detour_Separation, g_orig_separation,
              &Enabled)
CVR_DETOUR_IF("[engine-stereo] weapon separation sub_1422849D0", WEAPON_SEPARATION_RVA, Detour_WeaponSeparation,
              g_orig_weapon_separation, &Enabled)

}  // namespace cvr::engine_stereo
