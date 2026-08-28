// OverlayDebugDraw -- the two things drawn ON the world rather than in a panel.
//
// The hand locator answers "where does the plugin think the hand is", which is the first question in
// every hand or weapon problem and cannot be answered from a log: it is a position, and a position is
// read by looking at it.
//
// The barrel crosshair draws from g_lastLocateQuat -- the quaternion the LOCATE site published, not the
// composed one. That is deliberate: it shows where the engine's camera is pointing, so a disagreement
// between the crosshair and where a shot goes is exactly the diagnosis wanted.

#include "Overlay/ImGuiOverlay.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Core/LiveControls.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <algorithm>
#include <atomic>
#include <cfloat>
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>
#include <imgui.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>
#include "im3d.h"
#include "Camera/CameraLink.hpp"   // cvr::camera::BarrelFrameRead
#include "Utils/SharedSlots.hpp"
#include "Overlay/OverlayInternal.hpp"

extern volatile int g_verboseLog; // per-frame log spam toggle (default off)
extern void Log(const char* fmt, ...);
extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);
extern volatile float g_lastLocateQuat[4];
extern "C" int   CyberpunkVR_StereoModuleEnable;   // vr_core.cpp: did we install at all
extern "C" int   CyberpunkVR_StereoModuleLoaded;
extern "C" int32_t CyberpunkVR_StereoLog;
extern "C" int      CyberpunkVR_StereoSubmit;              // openxr_frameloop.cpp
extern "C" int32_t  CyberpunkVR_StereoEyeCapture;
extern "C" uint32_t CyberpunkVR_StereoEyeMaxAgeMs;
extern "C" uint32_t CyberpunkVR_DebugVrcamEyeAgeMs;        // 0xFFFFFFFF = never produced
extern "C" unsigned long long CyberpunkVR_DebugStereoEyeSubmits;
extern "C" int32_t CyberpunkVR_StableCopy;
extern "C" int32_t CyberpunkVR_StableFromTonemap;
extern "C" uint64_t CyberpunkVR_DebugStableCopies;
extern "C" uint64_t CyberpunkVR_DebugStableSkips;
extern "C" int32_t CyberpunkVR_VrcamDlss;
extern "C" int32_t CyberpunkVR_ForceVrcamCam;
extern "C" uint32_t CyberpunkVR_VrcamEnabled;
extern "C" void        CyberpunkVR_SetVrcamEnabled(uint32_t on);
extern "C" const char* CyberpunkVR_VrcamComponentName();
extern "C" const char* CyberpunkVR_VrcamCameraName();
extern "C" uint32_t CyberpunkVR_MirrorOutput;
extern "C" uint64_t CyberpunkVR_DebugVrcamNodeHits;   // 0 => the second view never dispatched
extern "C" uint64_t CyberpunkVR_DebugMirrorRtvHits;
extern "C" int CyberpunkVR_IsVrcamViewActive();
extern "C" float CyberpunkVR_DebugMainProjYY;
extern "C" float CyberpunkVR_DebugMainCamFov;
extern "C" float CyberpunkVR_MainAdsZoomFactor;
extern "C" float CyberpunkVR_DebugVrcamWantFov;
extern "C" float CyberpunkVR_DebugVrcamBaseFov;
extern "C" int32_t  CyberpunkVR_ProfEnable;
extern "C" double   CyberpunkVR_ProfFrameMs;
extern "C" double   CyberpunkVR_ProfDispMainMs;
extern "C" double   CyberpunkVR_ProfDispVrcamMs;
extern "C" uint32_t CyberpunkVR_ProfDispMainNodes;
extern "C" uint32_t CyberpunkVR_ProfDispVrcamNodes;
extern "C" void     CyberpunkVR_ProfDumpNodes();
extern "C" int      CyberpunkVR_ProfSnapshotNodes(uint32_t* rva, double* msv, double* msm,
                                                  uint32_t* cv, uint32_t* cm, int maxn);
extern "C" const char* CyberpunkVR_ProfNodeName(uint32_t rva);
extern "C" uint64_t CyberpunkVR_DebugViewKeyMainNodes;
extern "C" uint64_t CyberpunkVR_DebugViewKeyOtherNodes;
extern volatile int32_t g_lastLocatePosFP[3];
extern "C" float CyberpunkVRPort_HalfIpd();
extern "C" float GetGameRenderFovDeg();
extern "C" int CyberpunkVR_MainIsRightEye;
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();

namespace overlay {

void DrawHandLocatorOverlay() {
    if (!g_drawHandLocator) return;

    OpenXRHeadPose head{};
    OpenXRHeadPose left{};
    OpenXRHeadPose right{};
    if (!OpenXRManager::Get().GetHeadPose(&head) || !head.valid) return;

    const bool hasLeft = OpenXRManager::Get().GetHandPose(0, &left) && left.valid;
    const bool hasRight = OpenXRManager::Get().GetHandPose(1, &right) && right.valid;
    if (!hasLeft && !hasRight) return;

    const ImVec2 displaySize = ImGui::GetIO().DisplaySize;
    if (displaySize.x <= 1.0f || displaySize.y <= 1.0f) return;

    // BACKGROUND, not foreground: this geometry is projected for one eye, and the second-eye
    // overlay pass (OverlayRecordIntoTarget) skips the background list for exactly that reason.
    // Being under the menu window rather than over it is the incidental other effect, and the
    // better of the two orders anyway.
    ImDrawList* drawList = ImGui::GetBackgroundDrawList();
    if (!drawList) return;

    Im3d::AppData& appData = Im3d::GetAppData();
    appData.m_viewOrigin = Im3d::Vec3(0.0f, 0.0f, 0.0f);
    appData.m_viewDirection = Im3d::Vec3(0.0f, 0.0f, -1.0f);
    appData.m_worldUp = Im3d::Vec3(0.0f, 1.0f, 0.0f);
    appData.m_viewportSize = Im3d::Vec2(displaySize.x, displaySize.y);
    appData.m_deltaTime = 1.0f / 90.0f;
    appData.m_projOrtho = false;
    // Same GAME-RENDER vertical tan as the point projectors (not the lens V-FOV),
    // so Im3d size attenuation agrees with where the points actually land.
    {
        float tx = 0.0f, ty = tanf(50.0f * (3.1415926535f / 180.0f));
        GetOverlayProjTans(displaySize, &tx, &ty);
        appData.m_projScaleY = ty;
    }
    Im3d::NewFrame();

    const auto drawEndpointLabel = [&](const OpenXRHeadPose& handPose, float x, float y, float z,
        ImU32 color, const char* text, const ImVec2& offset) {
        ImVec2 screen{};
        if (!ProjectHandLocalPoint(head, handPose, x, y, z, displaySize, &screen)) return;
        drawList->AddText(ImVec2(screen.x + offset.x, screen.y + offset.y), color, text);
    };

    const auto drawHandWire = [&](const OpenXRHeadPose& handPose, ImU32 bodyColor, const char* name, bool isLeftHand) {
        const ImU32 rightColor = IM_COL32(255, 80, 80, 255);
        const ImU32 upColor = IM_COL32(80, 255, 80, 255);
        const ImU32 fwdColor = IM_COL32(80, 160, 255, 255);
        const ImU32 textColor = IM_COL32(255, 255, 255, 255);

        const float s = g_handLocatorScale;
        const float wristHalfW = 0.026f * s;
        const float palmHalfW = 0.034f * s;
        const float wristY = -0.055f * s;
        const float palmMidY = -0.018f * s;
        const float knuckleY = 0.045f * s;
        const float fwdLen = 0.180f * s;

        // Hand abstract frame:
        //   +X = thumb side
        //   +Y = fingers direction
        //   +Z = palm normal
        // Empirical grip-pose basis from the current runtime/controller:
        //   +X = left, +Y = forward, -Z = up
        // Desired hand mapping:
        //   thumb   -> up      => -Z
        //   fingers -> forward => +Y
        //   palm    -> left/right => +X for right hand, -X for left hand
        const auto mapHandPoint = [&](float hx, float hy, float hz, float* cx, float* cy, float* cz) {
            if (isLeftHand) {
                *cx = -hz;
            } else {
                *cx = hz;
            }
            *cy = -hy;
            *cz = -hx;
        };

        const auto bone = [&](float ax, float ay, float az, float bx, float by, float bz, ImU32 color, float thickness) {
            float cax = 0.0f, cay = 0.0f, caz = 0.0f;
            float cbx = 0.0f, cby = 0.0f, cbz = 0.0f;
            mapHandPoint(ax, ay, az, &cax, &cay, &caz);
            mapHandPoint(bx, by, bz, &cbx, &cby, &cbz);
            DrawProjectedBone(drawList, head, handPose, cax, cay, caz, cbx, cby, cbz, displaySize, color, thickness);
        };

        const auto finger = [&](float baseX, float baseY, float midX, float midY, float tipX, float tipY) {
            bone(baseX, baseY, 0.0f, midX, midY, -0.010f * s, bodyColor, 2.0f);
            bone(midX, midY, -0.010f * s, tipX, tipY, -0.020f * s, bodyColor, 2.0f);
        };

        // Wrist and palm outline.
        bone(-wristHalfW, wristY, 0.0f, wristHalfW, wristY, 0.0f, bodyColor, 2.0f);
        bone(-wristHalfW, wristY, 0.0f, -palmHalfW, palmMidY, 0.0f, bodyColor, 2.0f);
        bone(wristHalfW, wristY, 0.0f, palmHalfW, palmMidY, 0.0f, bodyColor, 2.0f);
        bone(-palmHalfW, palmMidY, 0.0f, -palmHalfW, knuckleY, 0.0f, bodyColor, 2.0f);
        bone(palmHalfW, palmMidY, 0.0f, palmHalfW, knuckleY, 0.0f, bodyColor, 2.0f);
        bone(-palmHalfW, knuckleY, 0.0f, palmHalfW, knuckleY, 0.0f, bodyColor, 2.0f);

        // Fingers.
        finger(-0.024f * s, knuckleY, -0.026f * s, 0.074f * s, -0.026f * s, 0.096f * s); // pinky
        finger(-0.010f * s, knuckleY, -0.011f * s, 0.086f * s, -0.011f * s, 0.116f * s); // ring
        finger(0.006f * s, knuckleY, 0.006f * s, 0.094f * s, 0.006f * s, 0.128f * s);     // middle
        finger(0.022f * s, knuckleY, 0.023f * s, 0.086f * s, 0.023f * s, 0.116f * s);     // index

        // Thumb: always +X in abstract hand space. Handedness is handled by mapHandPoint.
        bone(0.020f * s, -0.004f * s, 0.0f,
             0.046f * s, 0.014f * s, -0.006f * s,
             bodyColor, 2.0f);
        bone(0.046f * s, 0.014f * s, -0.006f * s,
             0.065f * s, 0.035f * s, -0.016f * s,
             bodyColor, 2.0f);

        // Axes: right/left, up/down, forward.
        bone(-0.060f * s, 0.0f, 0.0f, 0.060f * s, 0.0f, 0.0f, rightColor, 2.0f);
        bone(0.0f, -0.060f * s, 0.0f, 0.0f, 0.060f * s, 0.0f, upColor, 2.0f);
        bone(0.0f, 0.0f, 0.0f, 0.0f, 0.0f, -fwdLen, fwdColor, 3.0f);

        // Forward arrow head.
        bone(0.0f, 0.0f, -fwdLen, 0.020f * s, 0.0f, -(fwdLen - 0.028f * s), fwdColor, 2.0f);
        bone(0.0f, 0.0f, -fwdLen, -0.020f * s, 0.0f, -(fwdLen - 0.028f * s), fwdColor, 2.0f);
        bone(0.0f, 0.0f, -fwdLen, 0.0f, 0.020f * s, -(fwdLen - 0.028f * s), fwdColor, 2.0f);
        bone(0.0f, 0.0f, -fwdLen, 0.0f, -0.020f * s, -(fwdLen - 0.028f * s), fwdColor, 2.0f);

        // Center dot.
        ImVec2 center{};
        if (ProjectHandLocalPoint(head, handPose, 0.0f, 0.0f, 0.0f, displaySize, &center)) {
            drawList->AddCircleFilled(center, 4.0f, bodyColor);
            drawList->AddText(ImVec2(center.x + 8.0f, center.y - 20.0f), bodyColor, name);
        }

        drawEndpointLabel(handPose, 0.070f * s, 0.0f, 0.0f, textColor, "R", ImVec2(4.0f, -6.0f));
        drawEndpointLabel(handPose, -0.070f * s, 0.0f, 0.0f, textColor, "L", ImVec2(-10.0f, -6.0f));
        drawEndpointLabel(handPose, 0.0f, 0.070f * s, 0.0f, textColor, "U", ImVec2(-4.0f, -14.0f));
        drawEndpointLabel(handPose, 0.0f, -0.070f * s, 0.0f, textColor, "D", ImVec2(-4.0f, 2.0f));
        drawEndpointLabel(handPose, 0.0f, 0.0f, -(fwdLen + 0.020f * s), textColor, "F", ImVec2(4.0f, -6.0f));
    };

    if (hasLeft) {
        if (g_drawHandProxy3D) {
            EmitHandProxyIm3d(left, true, g_handLocatorScale, g_drawHandDebugAxes);
        }
        if (g_drawHandDebugAxes || !g_drawHandProxy3D) {
            drawHandWire(left, IM_COL32(0, 220, 255, 255), "LEFT", true);
        }
    }
    if (hasRight) {
        if (g_drawHandProxy3D) {
            EmitHandProxyIm3d(right, false, g_handLocatorScale, g_drawHandDebugAxes);
        }
        if (g_drawHandDebugAxes || !g_drawHandProxy3D) {
            drawHandWire(right, IM_COL32(255, 180, 0, 255), "RIGHT", false);
        }
    }

    Im3d::EndFrame();
    if (g_drawHandProxy3D) {
        RenderIm3dToDrawList(drawList, displaySize);
    }

    // Aim ray: a long bright line down each hand's forward (abstract -Z), i.e. the same
    // direction as the small "F" axis but extended several meters, so the user can see
    // in-headset where the controller / held weapon is pointing. Sampled in many short
    // segments so it still draws correctly when part of the ray falls behind the eye.
    if (g_drawAimRay) {
        // Laser sight: a long forward ray down each hand's pointing axis. The abstract hand
        // frame's FORWARD is +Y (user-confirmed in-headset: the bright +Y axis ran from the
        // fingers down the gun barrel; -Z had pointed sideways from the palm). Sampled in many
        // short segments so it still draws when part of the ray falls behind the eye.
        const float maxLen = (g_aimRayLenM > 0.5f) ? g_aimRayLenM : 8.0f;
        const auto laser = [&](const OpenXRHeadPose& hp, bool isLeft, ImU32 color, float thick) {
            const int N = 40;
            ImVec2 prev{};
            bool havePrev = false;
            for (int i = 0; i <= N; ++i) {
                const float t = (static_cast<float>(i) / static_cast<float>(N)) * maxLen;
                const Im3d::Vec3 p = AbstractHandPointToHeadSpace(hp, isLeft, 0.0f, t, 0.0f); // +Y = forward
                ImVec2 sc{};
                if (ProjectIm3dPointToScreen(p, displaySize, &sc)) {
                    if (havePrev) drawList->AddLine(prev, sc, color, thick);
                    prev = sc;
                    havePrev = true;
                } else {
                    havePrev = false;
                }
            }
            // Aim dot at ~4 m so "where it points" reads at a glance.
            const Im3d::Vec3 dot = AbstractHandPointToHeadSpace(hp, isLeft, 0.0f, 4.0f, 0.0f);
            ImVec2 dc{};
            if (ProjectIm3dPointToScreen(dot, displaySize, &dc)) {
                drawList->AddCircleFilled(dc, 6.0f, color);
                drawList->AddCircle(dc, 10.0f, IM_COL32(255, 255, 255, 255), 0, 2.0f);
            }
        };
        if (hasRight) laser(right, false, IM_COL32(80, 255, 80, 255), 3.0f); // weapon hand: green
        if (hasLeft)  laser(left, true, IM_COL32(0, 200, 255, 160), 2.0f);   // off hand: cyan, dimmer
    }
}

// The barrel dot, in NDC, for the eye the overlay cannot reach. Written by DrawBarrelCrosshair
// below; read by the VRCAM eye composite (openxr_capture.cpp) and by the desktop mirror
// (sync_stereo.cpp). The tick is what makes a stale value harmless: the consumers ignore it once
// it stops being refreshed, so holstering the weapon removes the dot instead of freezing it.
extern "C" __declspec(dllexport) float    CyberpunkVR_BarrelDotNdcX = 0.0f;
// The SECOND eye needs its own value. The dot marks where the bullet goes, and the bullet
// line passes through the FIRST eye (the game aligns the weapon to its camera, which is
// MAIN). From the second eye, one IPD off that line, the same world point lies at a
// different angle -- exactly the parallax the reticle is now zeroed for. Without this the
// dot and the reticle disagree by that angle in the second eye, and the instrument would
// contradict the thing it is there to measure.
extern "C" __declspec(dllexport) float    CyberpunkVR_BarrelDotNdcX2 = 0.0f;
// Must match the distance sight_reflex_ps.hlsl was compiled with (build_sight.ps1 arg 2).
// 0 = the dot marks the BORE DIRECTION in both eyes, with no per-eye parallax. Back to that
// until the instance measurement says whether the two views place the weapon differently:
// a correction derived from the wrong model is worse than none, because it moves the very
// reference the reticle is being judged against.
extern "C" __declspec(dllexport) float    CyberpunkVR_SightZeroMeters = 20.0f;   // dot parallax distance
extern "C" __declspec(dllexport) float    CyberpunkVR_BarrelDotNdcY = 0.0f;
extern "C" __declspec(dllexport) float    CyberpunkVR_BarrelDotRadiusPx = 3.0f;
// Range at which the dot marks the bullet's line. A mark can only be exact at one distance --
// that is true of every sight -- but at any finite value both eyes agree on the same world point,
// which is the part that was broken.
extern "C" __declspec(dllexport) float    CyberpunkVR_BarrelDotDistM = 20.0f;
extern "C" __declspec(dllexport) int      CyberpunkVR_BarrelDotEyeSign = 1;
// Straight nudge of the SECOND eye's dot, in NDC (screen half-widths). Negative = left.
// 0.0025 is about 3 mrad at this field of view, i.e. 65 mm at 20 m -- the size of the error we
// have been chasing. Whatever value this lands on IS that error, measured, and the same number
// then applies to the reticle.
extern "C" __declspec(dllexport) float    CyberpunkVR_BarrelDotOffX2 = 0.0f;
// 0 = one projection for both eyes plus a constant parallax on the second (simple, steady).
// 1 = a real world point projected per eye (exact, but only as steady as the muzzle transform).
// 2 = the closest raycast surface projected per eye; a miss uses mode 0.
extern "C" __declspec(dllexport) int      CyberpunkVR_BarrelDotWorld = 1;
extern "C" int CyberpunkVR_MainIsRightEye;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_BarrelDotTick = 0;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_BarrelDotSecondEye = 1;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_BarrelDotSecondVisible = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBarrelDotDraws = 0;

// EXACT barrel crosshair. The plugin publishes the weapon muzzle WORLD forward (shared[24..26]); we
// rotate it into the located game camera's local frame (inv(camQuat) * fwd) and project that
// direction with the SAME view/FOV the eye renders through -> the dot lands exactly where the bullet
// goes (both derive from the same muzzle + camera). No controller-space guessing.
// The compact ADS-camera panel, drawn with the game running so the numbers can be read while
// aiming rather than reconstructed from a log afterwards. A window, not the background list, so it
// reaches both eyes through the second-eye overlay pass. Off by default (dabinn, TofuExpress
// ec1aa65c): it is an instrument, not a HUD.
void DrawCompactAdsCameraTelemetry() {
    if (!g_showCompactAdsTelemetry) return;

    AdsCameraTelemetryUiState t{};
    GetAdsCameraTelemetryUiState(&t);
    const ImVec2 display = ImGui::GetIO().DisplaySize;
    ImGui::SetNextWindowPos(
        ImVec2(display.x * g_compactAdsTelemetryX, display.y * g_compactAdsTelemetryY),
        ImGuiCond_Always,
        ImVec2(0.5f, 0.5f));
    ImGui::SetNextWindowBgAlpha(0.72f);
    constexpr ImGuiWindowFlags flags = ImGuiWindowFlags_NoDecoration |
        ImGuiWindowFlags_AlwaysAutoResize | ImGuiWindowFlags_NoSavedSettings |
        ImGuiWindowFlags_NoFocusOnAppearing | ImGuiWindowFlags_NoNav |
        ImGuiWindowFlags_NoInputs;
    if (ImGui::Begin("ADS camera telemetry##compact", nullptr, flags)) {
        if (!t.available) {
            ImGui::TextUnformatted("ADS CAM  waiting for gameplay camera...");
        } else {
            const ImVec4 stateColor = t.aiming
                ? ImVec4(1.0f, 0.78f, 0.24f, 1.0f)
                : ImVec4(0.45f, 0.9f, 0.55f, 1.0f);
            const float mainAdsZoom = CyberpunkVR_MainAdsZoomFactor;
            const float sharedZoomRaw = OpenXRManager::Get().GetSharedSlot(28);
            const float finalZoom = (std::isfinite(mainAdsZoom) && mainAdsZoom > 0.0f)
                ? mainAdsZoom : 1.0f;
            ImGui::TextColored(stateColor, "ADS CAM  %s", t.aiming ? "ON" : "HIP");
            ImGui::Text("zoom MAIN %.3fx   final %.3fx", mainAdsZoom, finalZoom);
            ImGui::TextDisabled("shared[28] %.3fx   diagnostic only", sharedZoomRaw);
            if (!t.baselineValid) {
                ImGui::TextUnformatted("Hold hip-fire briefly to capture baseline");
            } else {
                ImGui::Text("delta cm  R %+6.2f  F %+6.2f  U %+6.2f",
                            t.deltaRight * 100.0f, t.deltaForward * 100.0f, t.deltaUp * 100.0f);
                ImGui::Text("peak  cm  R %6.2f  F %6.2f  U %6.2f   n=%u",
                            t.peakRight * 100.0f, t.peakForward * 100.0f,
                            t.peakUp * 100.0f, t.samples);
                ImGui::TextDisabled("raw   cm  R %+6.2f  F %+6.2f  U %+6.2f",
                                    t.residualRight * 100.0f, t.residualForward * 100.0f,
                                    t.residualUp * 100.0f);
            }
        }
    }
    ImGui::End();
}

void DrawBarrelCrosshair() {
    int laserDotMode = g_liveControls.xrLaserDotMode;
    if (laserDotMode < 0 || laserDotMode > 2) laserDotMode = 1;
    CyberpunkVR_BarrelDotWorld = laserDotMode;

    const float enableLaser = OpenXRManager::Get().GetSharedSlot(144);   // weapon flag (was [126]: HMD-Z collision)
    const bool surfaceMode = laserDotMode == 2;
    const bool raycastActive = g_drawBarrelCross && surfaceMode && enableLaser >= 0.9f;
    // CET owns every physics query. Publish the UI/mode gate before returning so disabling a dot
    // stops muzzle and visibility work rather than merely stopping the final draw.
    OpenXRManager::Get().SetSharedSlot(vrshared::kBarrelRayActive, raycastActive ? 1.0f : 0.0f);
    
    float rad = 3.0f;
    
    if (!g_drawBarrelCross || enableLaser < 0.9f){
        // The second eye is stamped by the capture path rather than ImGui. Clear its publication
        // immediately when the common UI/weapon gate closes instead of waiting for the 250 ms
        // consumer safety timeout.
        CyberpunkVR_BarrelDotTick = 0;
        CyberpunkVR_BarrelDotSecondVisible = 0;
        /*rad = 0.0f;
        // Background list: world-projected, see DrawHandLocatorOverlay.
        ImDrawList* dl = ImGui::GetBackgroundDrawList();
        if (dl) {
            ImVec2 sc = {0.0f, 0.0f};
            dl->AddCircleFilled(sc, rad, IM_COL32(255, 60, 60, 0));
            //dl->AddCircle(sc, 11.0f, IM_COL32(255, 255, 255, 235), 0, 2.0f);
        }*/
        return;
    } 

    // ONE INSTANT, BOTH QUANTITIES (dabinn, TofuExpress 821e8a4e). The camera quaternion and the
    // muzzle direction come out of a single seqlocked packet published at MAIN's final-camera
    // callback. What stood here read the latest LOCATED quaternion at Present and sampled the muzzle
    // slots independently, so a fast head turn combined two different moments and the dot smeared
    // into a velocity-dependent trail; and the located quaternion is not even the right camera,
    // since it goes through the mixer and is scaled by the camera weight on the way to the screen.
    //
    // The latch for the identity-default muzzle moved to the publisher, where the direction is
    // captured, so it cannot disagree with the quaternion it ships with.
    cvr::camera::BarrelFrame bf{};
    if (!cvr::camera::BarrelFrameRead(&bf)) return;
    const float mfx = bf.muzzleFwd[0], mfy = bf.muzzleFwd[1], mfz = bf.muzzleFwd[2];
    if (mfx*mfx + mfy*mfy + mfz*mfz < 0.25f) return;

    const float cqx = bf.camQuat[0], cqy = bf.camQuat[1], cqz = bf.camQuat[2], cqw = bf.camQuat[3];

    // A DIRECTION CANNOT MARK AN IMPACT POINT except for an eye that lies on the bullet's line.
    //
    // The old code rotated the muzzle forward into camera axes and projected that vector, i.e. it
    // drew a mark at infinity along the barrel. The bullet, though, leaves the MUZZLE, so an
    // impact at range t appears from eye E offset by (muzzle - E)perp / t. The weapon is held in
    // front of the left eye, so that offset is ~0 there and the mark looked exact; the right eye
    // is an IPD or more off the barrel and the same mark misses, by a constant distance in metres
    // -- which is why it never changed with the sight's zero distance and never showed up in any
    // of the per-eye rendering measurements. Nothing about the picture was ever wrong.
    //
    // So build a real point on the bullet's line and project it from each eye's own position.
    // Both eyes then mark the same place in the world, and both are right.
    // LATCHED ON THIS SIDE. The slot reads back as (0,0,0) on a good share of frames -- the
    // publisher filters local-space samples, so something else is clearing it, and chasing who
    // is not worth another round-trip. A muzzle position does not stop existing between frames,
    // so the last real one is kept and a zero read simply changes nothing. Immune to whoever
    // writes there and to the order they do it in.
    static float s_mp[3] = {0.0f, 0.0f, 0.0f};
    {
        const float rx = OpenXRManager::Get().GetSharedSlot(200);
        const float ry = OpenXRManager::Get().GetSharedSlot(201);
        const float rz = OpenXRManager::Get().GetSharedSlot(202);
        if (rx*rx + ry*ry + rz*rz > 1.0f) { s_mp[0] = rx; s_mp[1] = ry; s_mp[2] = rz; }
    }
    const float mpx = s_mp[0], mpy = s_mp[1], mpz = s_mp[2];
    // Straight from the camera hook in this same DLL. Routing these two through shared memory
    // added two more ways to end up with nothing and no message -- which is exactly what happened.
    const float hcx = static_cast<float>(g_lastLocatePosFP[0]) / 131072.0f;
    const float hcy = static_cast<float>(g_lastLocatePosFP[1]) / 131072.0f;
    const float hcz = static_cast<float>(g_lastLocatePosFP[2]) / 131072.0f;
    const bool haveWorld = (mpx * mpx + mpy * mpy + mpz * mpz) > 1.0f &&
                           (hcx * hcx + hcy * hcy + hcz * hcz) > 1.0f;

    // CET owns the physics query and publishes one world-space hit packet. A sequence that stops
    // changing means the publisher was interrupted; never leave the last dot glued to a wall.
    static uint32_t s_lastRaySeq = 0;
    static uint64_t s_lastRayTick = 0;
    float hitx = 0.0f, hity = 0.0f, hitz = 0.0f, hitValid = 0.0f;
    float mainVisible = 1.0f, secondVisible = 1.0f;
    uint32_t raySeq = 0;
    bool rayPacket = false;
    for (int attempt = 0; surfaceMode && attempt < 3 && !rayPacket; ++attempt) {
        const uint32_t s0 = static_cast<uint32_t>(
            OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelRaySeq));
        if (s0 == 0u || (s0 & 1u)) continue;
        std::atomic_thread_fence(std::memory_order_acquire);
        const float x = OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelRayHitX + 0);
        const float y = OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelRayHitX + 1);
        const float z = OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelRayHitX + 2);
        const float v = OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelRayHitValid);
        const float vm = OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelMainVisible);
        const float vs = OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelSecondVisible);
        std::atomic_thread_fence(std::memory_order_acquire);
        const uint32_t s1 = static_cast<uint32_t>(
            OpenXRManager::Get().GetSharedSlot(vrshared::kBarrelRaySeq));
        if (s0 == s1 && !(s1 & 1u)) {
            hitx = x; hity = y; hitz = z; hitValid = v;
            mainVisible = vm; secondVisible = vs;
            raySeq = s1; rayPacket = true;
        }
    }
    if (rayPacket && raySeq != s_lastRaySeq) {
        s_lastRaySeq = raySeq;
        s_lastRayTick = GetTickCount64();
    }
    const uint64_t nowTick = GetTickCount64();
    const bool rayFresh = rayPacket && s_lastRayTick != 0 && (nowTick - s_lastRayTick) <= 20u;
    const bool haveRayHit = surfaceMode && rayFresh && hitValid > 0.5f &&
                            (hcx * hcx + hcy * hcy + hcz * hcz) > 1.0f &&
                            (hitx * hitx + hity * hity + hitz * hitz) > 1.0f;

    const ImVec2 displaySize = ImGui::GetIO().DisplaySize;
    if (displaySize.x <= 1.0f || displaySize.y <= 1.0f) return;

    // The camera's right axis, from the same quaternion the view is built with. Column 0 of the
    // rotation -- the engine's camera frame is X right, Y forward, Z up (verified live by
    // right x forward = up on the render camera's own basis).
    const float rgt[3] = {
        1.0f - 2.0f * (cqy * cqy + cqz * cqz),
        2.0f * (cqx * cqy + cqz * cqw),
        2.0f * (cqx * cqz - cqy * cqw)
    };
    float halfIpd = CyberpunkVRPort_HalfIpd();
    if (!(halfIpd > 0.0001f)) halfIpd = OpenXRManager::Get().GetSharedSlot(95);
    if (!(halfIpd > 0.0001f)) halfIpd = 0.0325f;

    // Publish exactly the two synthetic eye origins used by the dot projection. Naming these
    // MAIN/second avoids assuming which physical eye MAIN owns when the live eye sign is flipped.
    // CET reads the pair through this seqlock before issuing its game-thread visibility rays.
    const float sMain = (CyberpunkVR_BarrelDotEyeSign >= 0) ? +1.0f : -1.0f;
    const float mainEye[3] = {
        hcx + rgt[0] * halfIpd * sMain,
        hcy + rgt[1] * halfIpd * sMain,
        hcz + rgt[2] * halfIpd * sMain
    };
    const float secondEye[3] = {
        hcx - rgt[0] * halfIpd * sMain,
        hcy - rgt[1] * halfIpd * sMain,
        hcz - rgt[2] * halfIpd * sMain
    };
    {
        static uint32_t s_eyeSeq = 0;
        uint32_t nextEven = s_eyeSeq + 2u;
        if (nextEven >= 1000000u) nextEven = 2u;
        auto& xr = OpenXRManager::Get();
        xr.SetSharedSlot(vrshared::kBarrelEyeSeq, static_cast<float>(nextEven - 1u));
        std::atomic_thread_fence(std::memory_order_release);
        for (int i = 0; i < 3; ++i) {
            xr.SetSharedSlot(vrshared::kBarrelMainEyeX + i, mainEye[i]);
            xr.SetSharedSlot(vrshared::kBarrelSecondEyeX + i, secondEye[i]);
        }
        std::atomic_thread_fence(std::memory_order_release);
        xr.SetSharedSlot(vrshared::kBarrelEyeSeq, static_cast<float>(nextEven));
        s_eyeSeq = nextEven;
    }

    // Screen position of the impact point as seen from one eye. sign: -1 = MAIN/left, +1 = VRCAM.
    static float s_dbgLy[2] = {0.0f, 0.0f};
    static float s_dbgWhy = 0.0f;
    auto dotForEye = [&](float sign, ImVec2* out) -> bool {
        if (!haveWorld) { s_dbgWhy = 1.0f; return false; }
        const float D = CyberpunkVR_BarrelDotDistM;
        if (!(D > 0.5f)) { s_dbgWhy = 2.0f; return false; }
        const float ex = hcx + rgt[0] * halfIpd * sign;
        const float ey = hcy + rgt[1] * halfIpd * sign;
        const float ez = hcz + rgt[2] * halfIpd * sign;
        // The point the bullet reaches at D, and the ray from THIS eye to it.
        const float dx = (mpx + mfx * D) - ex;
        const float dy = (mpy + mfy * D) - ey;
        const float dz = (mpz + mfz * D) - ez;
        float lx = 0.0f, ly = 0.0f, lz = 0.0f;
        RotateVectorByQuaternion(dx, dy, dz, -cqx, -cqy, -cqz, cqw, &lx, &ly, &lz);
        s_dbgLy[(sign > 0.0f) ? 1 : 0] = ly;
        const bool okp = ProjectHeadSpacePointToScreen(lx, lz, -ly, displaySize, out);
        if (!okp) s_dbgWhy = 3.0f;
        return okp;
    };
    auto surfaceDotForEye = [&](float sign, ImVec2* out) -> bool {
        if (!haveRayHit) return false;
        const float ex = hcx + rgt[0] * halfIpd * sign;
        const float ey = hcy + rgt[1] * halfIpd * sign;
        const float ez = hcz + rgt[2] * halfIpd * sign;
        float lx = 0.0f, ly = 0.0f, lz = 0.0f;
        RotateVectorByQuaternion(hitx - ex, hity - ey, hitz - ez,
                                 -cqx, -cqy, -cqz, cqw, &lx, &ly, &lz);
        return ProjectHeadSpacePointToScreen(lx, lz, -ly, displaySize, out);
    };

    ImVec2 sc{};
    // game-cam-local (Yfwd/Xright/Zup) -> OpenXR view convention (forward -Z, right +X, up +Y): (x, z, -y)
    // The control number: the muzzle forward in camera axes. The old path projects exactly this
    // and works, so its forward component tells whether the axis mapping below is the same one.
    float vx = 0.0f, vy = 0.0f, vz = 0.0f;
    RotateVectorByQuaternion(mfx, mfy, mfz, -cqx, -cqy, -cqz, cqw, &vx, &vy, &vz);
    ImVec2 scRight{};
    // Sign fixed by observation: a fixed world point must slide LEFT on screen when the eye
    // moves RIGHT, and it was doing the opposite -- the second eye sat right of the first. So
    // MAIN is the +right eye here, not the -right one. Live switch rather than a silent constant.
    // Do NOT require both, and do NOT fall back. Requiring both meant one failed projection
    // dropped the pair onto the direction path, which is wrong for the second eye BY
    // CONSTRUCTION -- a mark at infinity cannot show a finite impact from an eye off the line.
    // That is what made the right eye's dot sit right of the shot on half the frames.
    const bool mainOk = surfaceMode ? surfaceDotForEye(sMain, &sc) : dotForEye(sMain, &sc);
    if (!(surfaceMode ? surfaceDotForEye(-sMain, &scRight) : dotForEye(-sMain, &scRight))) scRight = sc;
    // OFF by default. The world point is the exact answer, but it depends on a muzzle transform
    // that goes to identity between frames (fists, and recoil right after a shot) -- latched, it
    // then lags and the two dots visibly part company. The simple form below has none of that:
    // one projection, plus a constant parallax for the second eye.
    const bool worldOk = mainOk && (surfaceMode ? haveRayHit : CyberpunkVR_BarrelDotWorld != 0);
    {
        static int s_said = -1;
        const int now = worldOk ? 1 : 0;
        if (now != s_said) {
            s_said = now;
            Log("BarrelDot: worldPath=%d why=%.0f ly=(%.3f %.3f) v=(%.3f %.3f %.3f) fwd=(%.3f %.3f %.3f) D=%.1f "
                "ndc=(%.5f %.5f) muzzle=(%.3f %.3f %.3f) head=(%.3f %.3f %.3f) halfIpd=%.4f\n",
                now, s_dbgWhy, s_dbgLy[0], s_dbgLy[1], vx, vy, vz, mfx, mfy, mfz,
                CyberpunkVR_BarrelDotDistM,
                CyberpunkVR_BarrelDotNdcX, CyberpunkVR_BarrelDotNdcX2,
                mpx, mpy, mpz, hcx, hcy, hcz, halfIpd);
        }
    }
    // With world data present the direction path is not a fallback, it is a wrong answer.
    if (CyberpunkVR_BarrelDotWorld == 1 && haveWorld && !worldOk) return;
    if (worldOk || ProjectHeadSpacePointToScreen(vx, vz, -vy, displaySize, &sc)) {
        // NO ZOOM COMPENSATION HERE ANY MORE (dabinn, TofuExpress 2cb7b031). ADS magnification is
        // already in the projection this point was built with -- GetOverlayProjTans divides the
        // tangents by MAIN's own live factor -- so scaling the screen offset by shared[28] on top
        // applied the zoom TWICE. shared[28] is a CET GetZoom sample taken on its own schedule: not
        // only redundant, it can also be a frame out of step with the projection.
        // PUBLISH IT FOR THE SECOND EYE. The overlay draws into the backbuffer, which is eye 0
        // only -- eye 1 is the VRCAM view and no ImGui list ever reaches it. Rather than repeat
        // this projection there (and risk the two disagreeing for a reason of my own making),
        // hand the finished screen position over in NDC and let the eye pass stamp the same
        // point. Both eyes are projected through the same effective ADS frustum, so there is
        // nothing left to compensate for here either.
        CyberpunkVR_BarrelDotNdcX = (sc.x / displaySize.x) * 2.0f - 1.0f;
        CyberpunkVR_BarrelDotNdcY = 1.0f - (sc.y / displaySize.y) * 2.0f;
        {
            // Parallax of the zero distance, in NDC. Small-angle: the second eye sees the
            // zero point IPD/D radians to the side, and a magnified (scoped) image scales
            // that angle along with everything else.
            float thx = 0.0f, thy = 0.0f;
            float dx = 0.0f;
            const float zeroM = CyberpunkVR_SightZeroMeters;
            if (zeroM > 0.1f && GetOverlayProjTans(displaySize, &thx, &thy)) {
                const float ipd = OpenXRManager::Get().GetRuntimeIpd();
                if (ipd > 0.001f) {
                    // The offset eye is on the other side once MAIN moves eyes.
                    // thx already comes back magnified by the live ADS factor, so the
                    // parallax expressed in NDC scales with it automatically. Multiplying by
                    // shared[28] here was the same double application a third time.
                    dx = -(ipd / zeroM) / thx;
                    if (CyberpunkVR_MainIsRightEye) dx = -dx;
                }
            }
            CyberpunkVR_BarrelDotNdcX2 = (worldOk
                ? ((scRight.x / displaySize.x) * 2.0f - 1.0f)   // its own eye, its own ray
                : (CyberpunkVR_BarrelDotNdcX + (surfaceMode ? 0.0f : dx)))
                + CyberpunkVR_BarrelDotOffX2;
        }
        CyberpunkVR_BarrelDotRadiusPx = rad;
        CyberpunkVR_BarrelDotTick = GetTickCount64();
        const bool mainDotVisible = !surfaceMode || !rayFresh || mainVisible > 0.5f;
        CyberpunkVR_BarrelDotSecondVisible =
            (!surfaceMode || !rayFresh || secondVisible > 0.5f) ? 1 : 0;

        // Background list: world-projected, see DrawHandLocatorOverlay.
        ImDrawList* dl = ImGui::GetBackgroundDrawList();
        if (dl && mainDotVisible) {
            dl->AddCircleFilled(sc, rad, IM_COL32(255, 60, 60, 255));
            //dl->AddCircle(sc, 11.0f, IM_COL32(255, 255, 255, 235), 0, 2.0f);
        }
    }
}

}  // namespace overlay
using namespace overlay;
