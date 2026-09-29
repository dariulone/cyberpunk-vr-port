// OverlayPanels -- every control the headset overlay draws, and the small widgets they are made of.
//
// The widgets exist because ImGui has no clamped-int slider that reports change the way this file needs:
// CheckboxInt, SliderIntClamped and InputIntClamped all return whether the value moved, so a panel can
// persist only on change instead of writing the ini every frame.
//
// THE PANELS EDIT A SNAPSHOT, NOT THE LIVE CONTROLS. DrawLiveControls takes a LiveControlsUiState by
// reference, and the caller hands it back to be persisted. The live values are volatile scalars read by
// hooks on other threads; editing them directly from the UI thread would mean a slider drag is a series
// of half-applied states, which is visible in the headset.

#include "Anim/WheelGrab.hpp"
#include "Runtimes/HybridBodyYaw.hpp"
#include "Anim/VehiclePosePolicy.hpp"
#include "Utils/SharedSlots.hpp"
#include "Core/VrCoreShared.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Runtimes/OpenXRManager.hpp"
extern "C" float GetGameRenderVerticalFovDeg();
#include <algorithm>
#include <cfloat>
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>
#include <imgui.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>
#include "im3d.h"
#include "Overlay/OverlayInternal.hpp"
#include "Overlay/VrWidgets.hpp"
#include "Utils/DebugGate.hpp"
#include "Camera/CameraState.hpp"
#include "Hooks/Reflex.hpp"

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

bool CheckboxInt(const char* label, int* value) {
    bool checked = *value != 0;
    const bool changed = ImGui::Checkbox(label, &checked);
    if (changed) {
        *value = checked ? 1 : 0;
    }
    return changed;
}

bool SliderIntClamped(const char* label, int* value, int minValue, int maxValue) {
    int temp = *value;
    const bool changed = widgets::SliderInt(label, &temp, minValue, maxValue);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool InputIntClamped(const char* label, int* value, int minValue, int maxValue) {
    int temp = *value;
    const bool changed = ImGui::InputInt(label, &temp, 1, 64);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool DrawFovControl(LiveControlsUiState& state) {
    bool changed = false;
    const bool psvr2 = OpenXRManager::Get().IsRuntimePsvr2();
    bool overrideEnabled = state.xrForceFov > 0.0f;
    if (ImGui::Checkbox("Enable FOV override", &overrideEnabled)) {
        state.xrForceFov = overrideEnabled ? (psvr2 ? 104.5f : 112.0f) : 0.0f;
        changed = true;
    }
    if (!overrideEnabled) ImGui::BeginDisabled();
    int preset = 0;
    if (std::fabs(state.xrForceFov - 104.5f) < .01f) preset = 1;
    else if (std::fabs(state.xrForceFov - 94.0f) < .01f) preset = 2;
    else if (std::fabs(state.xrForceFov - 104.0f) < .01f) preset = 3;
    if (widgets::Combo("FOV preset", &preset,
        "Custom\0PSVR2 - 104.5 deg\0Quest 3 legacy - 94 deg (test)\0Pico 4 - 104 deg\0")) {
        if (preset > 0) {
            const float values[] = {0.0f, 104.5f, 94.0f, 104.0f};
            state.xrForceFov = values[preset];
            changed = true;
        }
    }
    float fov = overrideEnabled ? state.xrForceFov : (psvr2 ? 104.5f : 112.0f);
    if (widgets::SliderFloat("Horizontal render / projection FOV", &fov, 80.0f, 140.0f, "%.1f deg")) {
        state.xrForceFov = fov;
        changed = true;
    }
    if (!overrideEnabled) ImGui::EndDisabled();
    ImGui::TextWrapped("Override changes BOTH the game VR camera and OpenXR projection. "
        "Off uses automatic panel-edge coverage, which can be wider than the per-eye FOV span.");
    if (psvr2) ImGui::TextWrapped("PSVR2: 104.5 deg restores the previous override. Automatic coverage measured 123 deg on this setup.");
    ImGui::TextWrapped("Narrower coverage can improve central detail at the same resolution, but may reveal black borders. "
        "Quest 3 legacy is an optional comparison, not a universal correction; keep the matching headset resolution preset.");
    const float actual = GetGameRenderFovDeg();
    const float vertical = GetGameRenderVerticalFovDeg();
    if (actual > 1.0f && vertical > 1.0f)
        ImGui::Text("Camera render FOV: %.1f H / %.1f V deg", actual, vertical);
    return changed;
}

// In-headset VR floating-hands controls: tracking on/off, IK calibration, wrist
// alignment, and the diagnostic dump -- everything that used to live only in the
// desktop CET window. Values are published to the RED4ext arm-IK plugin through
// shared memory (OpenXRManager::SetVRHandCalib). Defaults mirror the plugin's
// baked calibration so the rig behaves identically before anything is touched.
void DrawVRHandsControls() {
    // Tracking toggle (writes shared-mem slot [32]; plugin installs hooks + arms
    // and sets g_VRBind = this value). Must be 4 = full-arm IK (the mode the CET
    // "Start VR Tracking" button uses). Mode 2 is the legacy direct bone-write
    // fallback -> stretched forearm / wrong placement, which is what this was.
    static bool s_vrHandTracking = true;   // default ON — backend's m_vrHandTrackingMode also defaults to 4
    if (ImGui::Checkbox("Start VR hand tracking", &s_vrHandTracking)) {
        OpenXRManager::Get().SetVRHandTrackingMode(s_vrHandTracking ? 4 : 0);
    }
    ImGui::SameLine();
    if (ImGui::Button("Log VR Diag")) {
        OpenXRManager::Get().RequestVRDiag();
    }
    if (ImGui::IsItemHovered()) {
        ImGui::SetTooltip("Dumps gizmo vs. bone poses to vrik_diag.txt (next to dxgi.dll)\n"
                          "for tuning the arm IK. Same as the CET 'Log VR Diag' button.");
    }

    ImGui::Separator();
    // Select one body-yaw policy; persist the compatible INI flags together.
    {
        LiveControlsUiState st{};
        GetLiveControlsUiState(&st);
        bool roomscale = st.xrRoomscaleMovement != 0;
        if (ImGui::Checkbox("Roomscale movement", &roomscale)) {
            st.xrRoomscaleMovement = roomscale ? 1 : 0;
            SetLiveControlsUiState(&st, 1);
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Move the player capsule with physical horizontal head movement.\n"
                              "Uses native collision/steps and preserves stick or WASD movement.\n"
                              "Suspended in menus, vehicles and scripted movement.");
        }
        using BodyMode=cvr::body::RotationMode;
        int mode=static_cast<int>(cvr::body::RotationModeFromFlags(st.xrPhysicalBodyRotation,
            st.xrTrackedBodyRotation,st.xrHybridBodyRotation));
        static const char* bodyModes[]{"Stick / snap only","Head + free-look cone",
            "HMD + controllers","Hybrid (default)"};
        ImGui::SetNextItemWidth(410.0f);
        if(widgets::Combo("Body rotation",&mode,bodyModes,4)){
            st.xrPhysicalBodyRotation=mode==static_cast<int>(BodyMode::HeadCone);
            st.xrTrackedBodyRotation=mode==static_cast<int>(BodyMode::Tracked);
            st.xrHybridBodyRotation=mode==static_cast<int>(BodyMode::Hybrid);
            SetLiveControlsUiState(&st,1);
        }
        if(ImGui::IsItemHovered())ImGui::SetTooltip("Hybrid: use the head cone while upright.\n"
            "Looking down by 10 degrees or bending by 5 degrees uses HMD + controllers.\n"
            "Return below 8 / 3 degrees; the current body heading is preserved at handover.\n"
            "HMD + controllers always disables the body cones. Missing tracking holds heading.");
        const bool trackedBody=mode==static_cast<int>(BodyMode::Tracked);
        const bool hybridBody=mode==static_cast<int>(BodyMode::Hybrid);
        const bool bodyRot=mode!=static_cast<int>(BodyMode::Off);
        // Cutscene VRIK suspend (PR #40). Picks the minimum scene tier at which the plugin fully
        // suspends the body+arm solve so the engine authored cinematic pose plays clean. Persisted
        // through the LiveControls bridge (vrport.ini xr_cutscene_suspend_tier), read directly by
        // AnimPose, so a change here takes effect without a restart.
        //
        // Combo index -> stored min-tier: Never(-1), Tier2+(1), Tier3+(2), Tier4+(3), Tier5(4).
        static const int kTierValues[] = { -1, 1, 2, 3, 4 };
        static const char* kTierLabels[] = {
            "Never (VRIK always on)",
            "Staged scenes and up (Tier 2+)",
            "Tier 3 and up",
            "Cinematics (Tier 4+)  [default]",
            "Full cinematics only (Tier 5)",
        };
        auto sceneSuspendControl=[&](const char* label,const char* id,int& tier,const char* help) {
            int idx=tier==0 ? 0 : 3;
            for(int i=0;i<5;++i)if(kTierValues[i]==tier) { idx=i;break; }
            const std::string controlLabel=std::string(label)+id;
            if(widgets::Combo(controlLabel.c_str(),&idx,kTierLabels,5)) {
                tier=kTierValues[idx];SetLiveControlsUiState(&st,1);
            }
            if(ImGui::IsItemHovered())ImGui::SetTooltip("%s",help);
        };
        sceneSuspendControl("Suspend VRIK in cutscenes (on foot)","##cutsceneSuspend",
            st.xrCutsceneSuspendTier,
            "Applies only on foot. VRIK yields to the scene animation at or above this tier.\n"
            "The vehicle threshold is configured separately below.");
        sceneSuspendControl("Suspend VRIK in cutscenes (vehicle)","##cutsceneSuspendVehicle",
            st.xrVehicleCutsceneSuspendTier,
            "Applies in vehicles, including passenger window combat.\n"
            "VRIK yields to the scene animation at or above this tier. Never keeps VRIK enabled.");
        ImGui::BeginDisabled(!bodyRot || trackedBody);
        float cone = st.xrBodyFreeLookDeg;
        if (widgets::SliderFloat("Free-look cone", &cone, 0.0f, 60.0f, "%.0f deg")) {
            st.xrBodyFreeLookDeg=cone;SetLiveControlsUiState(&st,1);
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("How far your head may turn before the body starts coming around.\n"
                              "The body settles toward the centre, then restores the full free-look zone.\n"
                              "Default: 10 degrees. Hybrid uses this value while looking ahead.");
        }
        ImGui::EndDisabled();
        ImGui::BeginDisabled(mode!=static_cast<int>(BodyMode::HeadCone));
        float downCone=st.xrBodyFreeLookDownDeg;
        if(widgets::SliderFloat("Free-look cone when looking down",&downCone,0.0f,90.0f,"%.0f deg")) {
            st.xrBodyFreeLookDownDeg=downCone;SetLiveControlsUiState(&st,1);
        }
        if(ImGui::IsItemHovered())ImGui::SetTooltip("Extra room to reach chest-mounted items.\n"
            "Blends in from 10 to 30 degrees of downward head tilt.\n"
            "Never narrows the normal cone; set the same value to disable widening.");
        float swimCone=st.xrBodyFreeLookSwimDeg;
        if(widgets::SliderFloat("Free-look cone in water",&swimCone,0.0f,90.0f,"%.0f deg")) {
            st.xrBodyFreeLookSwimDeg=swimCone;SetLiveControlsUiState(&st,1);
        }
        if(ImGui::IsItemHovered())ImGui::SetTooltip("One fixed cone on the surface and underwater.\n"
            "Looking down and physical bending do not widen it. Default: 5 degrees.");
        ImGui::EndDisabled();
        if(trackedBody)ImGui::TextDisabled("Body free-look cones are disabled in HMD + controllers mode.");
        if(hybridBody)ImGui::TextDisabled("Hybrid: head cone upright; HMD + controllers when looking down or bending.");
        float radiusCm=st.xrBodyMoveRadius*100;
        if(widgets::SliderFloat("Body movement free zone",&radiusCm,0.0f,15.0f,"%.1f cm")) {
            st.xrBodyMoveRadius=radiusCm*.01f;SetLiveControlsUiState(&st,1);
        }
        if(ImGui::IsItemHovered())ImGui::SetTooltip("Small physical head movements leave the body in place.\n"
            "Outside this zone the body follows smoothly; camera tracking remains immediate.");
        if (bodyRot && !trackedBody && cvr::RuntimeDiagnosticsEnabled()) {
            ImGui::Text("active cone %.1f deg | realign %+.1f deg | head-vs-body %+.1f deg",
                        CyberpunkVR_BodyYawFollowDeadDeg,CyberpunkVR_DebugBodyFollowOffsetDeg,CyberpunkVR_DebugBodyFollowErrDeg);
        }
    }

    ImGui::Separator();
    ImGui::TextUnformatted("Hand IK Calibration (per hand: R = right, L = left)");

    // Defaults mirror the plugin's baked calibration (main.cpp globals).
    static float scaleR = 1.05f, scaleL = 1.06f;   // reach scale (arm straightening)
    static float heightR = 0.0f, heightL = 0.0f; // vertical fine-tune offset (m)
    static float swingR = 1.0f,  swingL = 1.0f;    // elbow-swing gain
    static float poleR = 0.0f,   poleL = 0.0f;     // elbow pole spin (deg)
    static float wRp = 0.0f, wRy = -90.0f, wRr = 0.0f;     // right wrist euler (deg)
    static float wLp = -180.0f, wLy = -90.0f, wLr = 0.0f;  // left wrist euler (deg)

    // TWO-WAY SYNC (fixes "калибровка не сохраняется"). These statics used to be
    // one-way UI -> manager: they kept their hardcoded defaults after the manager
    // loaded vrik_calibration.ini / ran auto-calibration, so the FIRST slider touch
    // (or Apply) pushed 14 default values over the real calibration -- which the next
    // Save then wrote to disk. Pull the manager's live values into the sliders
    // whenever the user isn't actively dragging.
    if (!ImGui::IsAnyItemActive()) {
        float c[14]; OpenXRManager::Get().GetVRHandCalib(c);
        scaleR=c[0]; scaleL=c[1]; heightR=c[2]; heightL=c[3];
        swingR=c[4]; swingL=c[5]; poleR=c[6]; poleL=c[7];
        wRp=c[8]; wRy=c[9]; wRr=c[10]; wLp=c[11]; wLy=c[12]; wLr=c[13];
    }

    bool calChanged = false;
    calChanged |= widgets::SliderFloat("Reach scale R", &scaleR, 0.80f, 1.30f, "%.3f");
    calChanged |= widgets::SliderFloat("Reach scale L", &scaleL, 0.80f, 1.30f, "%.3f");
    calChanged |= widgets::SliderFloat("Height R", &heightR, -0.20f, 0.50f, "%.3f m");
    calChanged |= widgets::SliderFloat("Height L", &heightL, -0.20f, 0.50f, "%.3f m");
    calChanged |= widgets::SliderFloat("Elbow swing R", &swingR, -3.0f, 3.0f, "%.2f");
    calChanged |= widgets::SliderFloat("Elbow swing L", &swingL, -3.0f, 3.0f, "%.2f");
    calChanged |= widgets::SliderFloat("Elbow pole R", &poleR, -180.0f, 180.0f, "%.1f deg");
    calChanged |= widgets::SliderFloat("Elbow pole L", &poleL, -180.0f, 180.0f, "%.1f deg");

    ImGui::Separator();
    ImGui::TextUnformatted("Wrist rotation offset (palm/finger alignment, deg)");
    calChanged |= widgets::SliderFloat("Wrist R pitch", &wRp, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist R yaw",   &wRy, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist R roll",  &wRr, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist L pitch", &wLp, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist L yaw",   &wLy, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist L roll",  &wLr, -180.0f, 180.0f, "%.1f");

    ImGui::Separator();
    // Auto-calibration: T-pose sample from the same controller poses that draw the gizmo hands.
    // Press "Start", stretch arms out to the sides, stand straight. We derive shoulder offsets
    // and arm scale, then save to vrik_calibration.ini next to dxgi.dll.
    int  cState = OpenXRManager::Get().GetCalibrationState();
    if (cState == 1) {
        float prog = OpenXRManager::Get().GetCalibrationProgress();
        ImGui::TextColored(ImVec4(1.0f, 0.9f, 0.3f, 1.0f),
                           "AUTO-CALIBRATING: stretch arms STRAIGHT OUT to the sides,");
        ImGui::TextColored(ImVec4(1.0f, 0.9f, 0.3f, 1.0f),
                           "stand straight facing forward. Hold for %.1fs (%.0f%%).",
                           (1.0f - prog) * 4.0f, prog * 100.0f);
        ImGui::ProgressBar(prog, ImVec2(280, 0));
    } else {
        if (ImGui::Button("Start Auto-Calibration (T-pose, 4s)", ImVec2(280, 0))) {
            OpenXRManager::Get().StartAutoCalibration(4.0f);
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Press once, then stretch BOTH gizmo hands OUT to the sides at shoulder height\n"
                              "and stand straight facing forward. The mod measures the visible controller\n"
                              "positions, computes shoulder pivots + arm scale, and saves the result\n"
                              "to vrik_calibration.ini.");
        }
        if (cState == 2) {
            ImGui::SameLine();
            ImGui::TextColored(ImVec4(0.4f, 1.0f, 0.4f, 1.0f), "Saved.");
        }
    }

    ImGui::Separator();
    // Neutral rig alignment is automatic; this button only clears optional trims.
    {
        float cb[3]; OpenXRManager::Get().GetCameraOffset(cb);
        ImGui::TextUnformatted("Camera: neck + 15 cm forward, eyes + 10 cm up");
        if (ImGui::Button("Reset camera to neck", ImVec2(180, 0))) {
            OpenXRManager::Get().BakeCameraOffset();
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Uses the neutral neck, 15 cm forward and 10 cm above reference eyes.\n"
                              "Resets camera trims; no stance, head direction or Bake measurement is needed.");
        }
        ImGui::SameLine();
        if (ImGui::Button("Clear trim##cambake", ImVec2(90, 0))) {
            OpenXRManager::Get().ClearCameraOffset();
            OpenXRManager::Get().SaveCalibrationToFile();
        }
        ImGui::Text("camera mount: R %.3f  Fwd %.3f  Up %.3f", cb[0], cb[1], cb[2]);
    }

    ImGui::Separator();
    bool apply  = ImGui::Button("Apply Calibration");
    ImGui::SameLine();
    bool save   = ImGui::Button("Save");
    ImGui::SameLine();
    bool load   = ImGui::Button("Load");
    ImGui::SameLine();
    if (ImGui::Button("Reset Defaults")) {
        scaleR = 1.05f; scaleL = 1.06f; heightR = 0.0f; heightL = 0.0f;
        swingR = 1.0f; swingL = 1.0f; poleR = 0.0f; poleL = 0.0f;
        wRp = 0.0f; wRy = -90.0f; wRr = 0.0f; wLp = -180.0f; wLy = -90.0f; wLr = 0.0f;
        OpenXRManager::Get().SetShoulderAnatomical(0.14f, -0.17f, 0.05f, -0.14f, -0.17f, 0.05f);
        calChanged = true;
    }

    if (calChanged || apply) {
        OpenXRManager::Get().SetVRHandCalib(scaleR, scaleL, heightR, heightL,
                                            swingR, swingL, poleR, poleL,
                                            wRp, wRy, wRr, wLp, wLy, wLr);
    }
    // AUTOSAVE: persist edits once the drag/press is released, so tweaks survive a
    // restart without requiring the user to remember the Save button.
    {
        static bool s_calDirty = false;
        if (calChanged) s_calDirty = true;
        if (s_calDirty && !ImGui::IsAnyItemActive()) {
            OpenXRManager::Get().SaveCalibrationToFile();
            s_calDirty = false;
        }
    }
    if (save) OpenXRManager::Get().SaveCalibrationToFile();
    if (load) {
        // The two-way sync above pulls the loaded values into the sliders next frame.
        OpenXRManager::Get().LoadCalibrationFromFile();
    }
}

void ReleaseGameMouseCapture() {
    ClipCursor(nullptr);
    ReleaseCapture();

    CURSORINFO cursorInfo{sizeof(CURSORINFO)};
    if (GetCursorInfo(&cursorInfo) && (cursorInfo.flags & CURSOR_SHOWING) == 0) {
        while (ShowCursor(TRUE) < 0) {
        }
    }
}

void UpdateImGuiMouseFromCursor(HWND hwnd, float backbufferWidth, float backbufferHeight) {
    ImGuiIO& io = ImGui::GetIO();

    RECT client{};
    POINT cursor{};
    if (hwnd && GetClientRect(hwnd, &client) && GetCursorPos(&cursor) && ScreenToClient(hwnd, &cursor)) {
        io.AddMousePosEvent(
            static_cast<float>(cursor.x),
            static_cast<float>(cursor.y));
    }

    io.AddMouseButtonEvent(0, (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0);
    io.AddMouseButtonEvent(1, (GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0);
    io.AddMouseButtonEvent(2, (GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0);
}

// Stereo panel, carried over from the testbed overlay (testbed/src/overlay_imgui.cpp).
//
// Nothing here goes through LiveControlsUiState: the engine hooks read these globals on every
// frame, so edits apply immediately and the Save button has nothing to do with them. That is
// the same contract the testbed panel had, and it is what makes this usable for tuning IPD
// with the headset on.
void DrawStereoControls() {
    if (!CyberpunkVR_StereoModuleLoaded) {
        ImGui::TextColored(ImVec4(1.0f, 0.55f, 0.35f, 1.0f), "Stereo module not installed.");
        ImGui::TextWrapped("The engine hooks did not load, so everything below is inert and the "
                           "headset is on the plain mono path. Either "
                           "CyberpunkVR_StereoModuleEnable is 0, or bin\\x64\\vrport_nostereo.txt "
                           "exists. See the \"Stereo:\" line in cyberpunkvrport.log.");
        return;
    }

    // Three separate questions, reported separately, because each fails on its own:
    // is the second view rendering, did we get a colour frame from it, and did it reach
    // the headset.
    const uint32_t eyeAge = CyberpunkVR_DebugVrcamEyeAgeMs;
    const bool eyeEverProduced = eyeAge != 0xFFFFFFFFu;
    const bool eyeFresh = eyeEverProduced && eyeAge <= CyberpunkVR_StereoEyeMaxAgeMs;
    const bool submitOn = CyberpunkVR_StereoSubmit != 0;

    // Which eye VRCAM lands in is CyberpunkVR_MainIsRightEye's to decide -- the submit picks
    // eye (MainIsRightEye ? 0 : 1) for it -- so the wording is derived rather than written down.
    // It used to say "right" in four places while the default sent VRCAM to the LEFT eye, which
    // is the sort of label that costs an hour of looking in the wrong place.
    const char* kVrcamEye = CyberpunkVR_MainIsRightEye ? "LEFT" : "RIGHT";
    const char* kMainEye  = CyberpunkVR_MainIsRightEye ? "RIGHT" : "LEFT";

    if (submitOn && eyeFresh) {
        ImGui::TextColored(ImVec4(0.45f, 0.9f, 0.5f, 1.0f),
                           "Stereo active  (%s = VRCAM, %s = MAIN)", kVrcamEye, kMainEye);
    } else if (submitOn) {
        ImGui::TextColored(ImVec4(1.0f, 0.8f, 0.35f, 1.0f),
                           "No second eye right now -> both eyes get MAIN (mono)");
        ImGui::TextDisabled("Expected in menus and while loading. If it stays here in gameplay: "
                            "the VRCAM component is off, or vrcam.json names a camera the player "
                            "entity does not carry.");
    } else {
        ImGui::TextDisabled("Stereo submit off -> both eyes get MAIN (mono)");
    }
    if (cvr::RuntimeDiagnosticsEnabled() && eyeEverProduced)
        ImGui::TextDisabled("vrcam eye age %u ms  (stale over %u)   eye submits %llu",
                            eyeAge, CyberpunkVR_StereoEyeMaxAgeMs,
                            static_cast<unsigned long long>(CyberpunkVR_DebugStereoEyeSubmits));
    else if(cvr::RuntimeDiagnosticsEnabled())
        ImGui::TextDisabled("vrcam eye: never produced a frame");
    ImGui::Separator();

    bool submit = submitOn;
    char submitLabel[64];
    std::snprintf(submitLabel, sizeof(submitLabel), "Send VRCAM to the %s EYE", kVrcamEye);
    if (ImGui::Checkbox(submitLabel, &submit))
        CyberpunkVR_StereoSubmit = submit ? 1 : 0;
    if (ImGui::IsItemHovered())
        ImGui::SetTooltip("Off = the old behaviour, MAIN duplicated into both eyes.\n"
                          "On = the %s eye gets the VRCAM view's own final colour --\n"
                          "the same image the desktop mirror shows -- re-encoded to the\n"
                          "swapchain's sRGB format and scaled to the eye size.", kVrcamEye);

    // No separate capture switch: the submit path drives CyberpunkVR_StereoEyeCapture from the
    // checkbox above, so the per-frame snapshot cost appears and disappears with the feature
    // instead of being a second thing to remember to turn off.
    if(cvr::RuntimeDiagnosticsEnabled())ImGui::TextDisabled("snapshot copies %llu   skips %llu   vrcam RTV hits %llu",
                        static_cast<unsigned long long>(CyberpunkVR_DebugStableCopies),
                        static_cast<unsigned long long>(CyberpunkVR_DebugStableSkips),
                        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorRtvHits));

    int maxAge = static_cast<int>(CyberpunkVR_StereoEyeMaxAgeMs);
    if (widgets::SliderInt("Eye staleness limit (ms)", &maxAge, 33, 1000))
        CyberpunkVR_StereoEyeMaxAgeMs = static_cast<uint32_t>(maxAge);
    if (ImGui::IsItemHovered())
        ImGui::SetTooltip("How long the last VRCAM frame stays usable. Past this the %s\n"
                          "eye falls back to MAIN -- one eye frozen while the other moves\n"
                          "is far worse to look at than mono.", kVrcamEye);

    ImGui::Separator();
    // Read-only on purpose. VRCAM's upscaler mirrors MAIN's, decided at graph build from MAIN's
    // own flags -- a switch here could only ever disagree with the engine.
    ImGui::TextDisabled("VRCAM DLSS: %s  (follows MAIN's upscaler, no switch)",
                        CyberpunkVR_VrcamDlss ? "ON" : "off");
    if (ImGui::IsItemHovered()) {
        ImGui::SetTooltip("On whenever MAIN is upscaling with DLSS: vrcam gets its own\n"
                          "Streamline viewport and renders below its target.\n"
                          "Off for DLAA / TAA / no upscaler. Change it in the game's\n"
                          "graphics settings, not here.");
    }
    bool forceCam = CyberpunkVR_ForceVrcamCam != 0;
    if (ImGui::Checkbox("Match VRCAM projection to MAIN  (fov / zoom / near / far)", &forceCam))
        CyberpunkVR_ForceVrcamCam = forceCam ? 1 : 0;

    ImGui::Separator();
    // Forces the RTT component's isEnabled through the game's RTTI (the CET side re-asserts it,
    // so it survives a reload/respawn). Off means the engine stops rendering the second view
    // entirely, not just our stereo shift -- which is the cheapest way back to plain mono.
    bool vrcamOn = CyberpunkVR_VrcamEnabled != 0;
    if (ImGui::Checkbox("VRCAM component  (RTTI Toggle: force ON/OFF)", &vrcamOn))
        CyberpunkVR_SetVrcamEnabled(vrcamOn ? 1u : 0u);
    ImGui::TextDisabled("component %s   camera %s",
                        CyberpunkVR_VrcamComponentName(), CyberpunkVR_VrcamCameraName());
    if(cvr::RuntimeDiagnosticsEnabled())ImGui::TextDisabled("view nodes: main %llu   other %llu   vrcam %llu",
                        static_cast<unsigned long long>(CyberpunkVR_DebugViewKeyMainNodes),
                        static_cast<unsigned long long>(CyberpunkVR_DebugViewKeyOtherNodes),
                        static_cast<unsigned long long>(CyberpunkVR_DebugVrcamNodeHits));

    // Separate second swapchain + window mirroring the VRCAM eye (for OBS / desktop preview).
    // Costs a per-frame copy, so it is off unless asked for.
    bool mirrorOn = CyberpunkVR_MirrorOutput != 0;
    if (ImGui::Checkbox("VRCAM Mirror  (separate window, for capture)", &mirrorOn))
        CyberpunkVR_MirrorOutput = mirrorOn ? 1u : 0u;

    // Weapon ADS is not a toggle: the vrcam eye always follows MAIN's vertical FOV, narrowed by
    // the aim zoom. Read-only here because the numbers are the quickest way to tell a wrong FOV
    // from a stale one.
    ImGui::TextDisabled("vrcam fov %.2f  (asset %.2f)   main yy %.5f  fov %.2f   ADS x%.3f",
                        CyberpunkVR_DebugVrcamWantFov, CyberpunkVR_DebugVrcamBaseFov,
                        CyberpunkVR_DebugMainProjYY, CyberpunkVR_DebugMainCamFov,
                        CyberpunkVR_MainAdsZoomFactor);

    if (ImGui::CollapsingHeader("Diagnostics")) {
        ImGui::Checkbox("Compact ADS camera telemetry (in-headset)", &g_showCompactAdsTelemetry);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("A small panel that stays up after F10 is closed, reporting what the "
                              "ENGINE did to the camera when the sights came up:\n"
                              "delta and peak in centimetres, right/forward/up in the heading's own "
                              "basis, against a baseline captured while hip firing.\n"
                              "Stereo submission is unaffected.");
        }
        if (g_showCompactAdsTelemetry) {
            ImGui::Indent();
            widgets::SliderFloat("Telemetry X", &g_compactAdsTelemetryX, 0.10f, 0.90f, "%.2f");
            widgets::SliderFloat("Telemetry Y", &g_compactAdsTelemetryY, 0.10f, 0.90f, "%.2f");
            ImGui::TextDisabled("Normalised position in the eye image");
            ImGui::Unindent();
        }
        ImGui::Separator();

        bool slog = CyberpunkVR_StereoLog != 0;
        if (ImGui::Checkbox("Stereo logging -> cyberpunkvrport.log", &slog))
            CyberpunkVR_StereoLog = slog ? 1 : 0;

        bool stable = CyberpunkVR_StableCopy != 0;
        if (ImGui::Checkbox("Committed snapshot of the VRCAM final", &stable))
            CyberpunkVR_StableCopy = stable ? 1 : 0;
        if (ImGui::IsItemHovered())
            ImGui::SetTooltip("Off = read the engine's transient directly. That target is a\n"
                              "frame-graph allocation which a later pass aliases, which is what\n"
                              "made the image alternate bright/dark. Leave on.");
        bool fromTonemap = CyberpunkVR_StableFromTonemap != 0;
        if (ImGui::Checkbox("Snapshot at the tonemap node instead of RenderFinal2D", &fromTonemap))
            CyberpunkVR_StableFromTonemap = fromTonemap ? 1 : 0;

        bool prof = CyberpunkVR_ProfEnable != 0;
        if (ImGui::Checkbox("Node CPU profiler  (per-node self+incl ms)", &prof))
            CyberpunkVR_ProfEnable = prof ? 1 : 0;
        ImGui::TextDisabled("frame %.2f ms   dispatch: main %.2f (%u)  vrcam %.2f (%u)",
                            CyberpunkVR_ProfFrameMs,
                            CyberpunkVR_ProfDispMainMs, CyberpunkVR_ProfDispMainNodes,
                            CyberpunkVR_ProfDispVrcamMs, CyberpunkVR_ProfDispVrcamNodes);
        if (ImGui::Button("Dump node audit -> log  (resets window)"))
            CyberpunkVR_ProfDumpNodes();
        if (prof) {
            // Top-15 by SELF time. Self is the only rankable column: inclusive double-counts,
            // because SceneDrv contains every scene pass it dispatches.
            static uint32_t rva[15];
            static double msv[15], msm[15];
            static uint32_t cv[15], cm[15];
            const int n = CyberpunkVR_ProfSnapshotNodes(rva, msv, msm, cv, cm, 15);
            ImGui::TextDisabled("top %d by self ms/frame   (main | vrcam)", n);
            for (int i = 0; i < n; ++i) {
                const char* nm = CyberpunkVR_ProfNodeName(rva[i]);
                ImGui::Text("%-30s %6.3f | %6.3f", (nm && nm[0]) ? nm : "?", msm[i], msv[i]);
            }
        }
    }
}

static bool DrawDrivingControls(LiveControlsUiState& state) {
    bool changed=false;
            ImGui::Separator();
            ImGui::TextUnformatted("Driving -- hands on the wheel");
            changed |= CheckboxInt("Grab the wheel with the grips", &state.xrWheelGrab);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip(
                    "While DRIVING, bring a hand to where the driving animation holds the\n"
                    "wheel (or the handlebars) and squeeze that grip: the arm is handed back\n"
                    "to the game's own animation -- hand on the wheel, fingers wrapped around\n"
                    "it -- instead of following the controller. Release the grip and it goes\n"
                    "back to your hand.\n\n"
                    "Each hand is independent: hold the wheel with one and keep the other on\n"
                    "a gun. While a hand is at the wheel that grip does nothing else (no\n"
                    "holster equip, no magazine grab).");
            }
            {
                float r = state.xrWheelRadius > 0.0f ? state.xrWheelRadius : 0.28f;
                if (widgets::SliderFloat("Grab radius (m)", &r, 0.08f, 0.60f, "%.2f")) {
                    state.xrWheelRadius = r;
                    changed = true;
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("How close your hand has to be to the animated hand before the\n"
                                      "grip counts as grabbing the wheel. Bigger = easier to catch a\n"
                                      "wheel you cannot see; too big and every grip in a car grabs.");
                }
                float m = state.xrWheelSteerMaxDeg > 0.0f ? state.xrWheelSteerMaxDeg : 90.0f;
                if (widgets::SliderFloat("Full lock at (deg)", &m, 30.0f, 120.0f, "%.0f")) {
                    state.xrWheelSteerMaxDeg = m;
                    changed = true;
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("Wheel sensitivity: hand rotation from the grab position to full lock.\n"
                                      "One hand uses a hub calibrated where you grab; two use their connecting line.\n"
                                      "Adding or releasing a hand keeps the current turn.\n"
                                      "90 = a quarter turn reaches full lock. Lower = more sensitive.\n"
                                      "Travel beyond full lock keeps the same neutral position.");
                }
                float d = state.xrWheelSteerDeadDeg >= 0.0f ? state.xrWheelSteerDeadDeg : 1.5f;
                if (widgets::SliderFloat("Steering deadzone (deg)", &d, 0.0f, 20.0f, "%.1f")) {
                    state.xrWheelSteerDeadDeg = d;
                    changed = true;
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("Tilt around centre that steers nothing at all.\n"
                                      "Raise it if the car drifts while you hold the wheel straight;\n"
                                      "every degree here is a degree of dead wheel off centre.\n"
                                      "The full range still ends at 'Full lock at', so widening the\n"
                                      "deadzone does not make the steering jump.");
                }

                changed |= CheckboxInt("Steering prediction", &state.xrWheelPrediction);
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("Gently anticipate a continuing turn from recent hand movement.\n"
                                      "Limited to 8 ms and 0.5 degrees; neutral stays unchanged.\n"
                                      "Prediction stops on a pause, reversal or grip change.\n"
                                      "Off by default. It cannot anticipate the start of a turn.");
                }
                if (state.xrWheelPrediction) {
                    changed |= widgets::SliderFloat("Prediction horizon (ms)", &state.xrWheelPredictionMs, 0.0f, 8.0f, "%.1f");
                }
                changed |= CheckboxInt("Horn -- hand on the wheel hub", &state.xrWheelHorn);
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip(
                        "While DRIVING, put a hand on the MIDDLE of the wheel -- where you would\n"
                        "slap a real horn -- and the car honks for as long as it stays there.\n"
                        "No grip needed; a hand that is GRABBING the wheel never honks.");
                }
                float hr = state.xrWheelHornRadius > 0.0f ? state.xrWheelHornRadius : 0.12f;
                if (widgets::SliderFloat("Horn hub radius (m)", &hr, 0.04f, 0.30f, "%.2f")) {
                    state.xrWheelHornRadius = hr;
                    changed = true;
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("How near the wheel centre the hand counts as on the hub.\n"
                                      "Bigger = easier to find the horn without seeing it; too big\n"
                                      "and it reaches the rim, so every grab honks.");
                }

                ImGui::Spacing();
                changed |= CheckboxInt("Trigger fires the gun while driving", &state.xrVehicleGunTrigger);
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip(
                        "Draw a weapon in the driver seat and the RIGHT TRIGGER stops being the\n"
                        "throttle and becomes the gun: drive with the left hand on the wheel and\n"
                        "shoot with the right.\n\n"
                        "The throttle LATCHES at whatever it was when the weapon came out, so the\n"
                        "car keeps rolling, and the LEFT STICK forward/back trims that speed while\n"
                        "you shoot. Holster the weapon and the trigger is the throttle again.\n\n"
                        "While the weapon is out the left stick's forward/back is taken by the\n"
                        "trim: no lean / rock and no autodrive gesture until you holster.");
                }
                float tt = state.xrVehicleThrottleTrim > 0.0f ? state.xrVehicleThrottleTrim : 0.5f;
                if (widgets::SliderFloat("Throttle trim rate (/s)", &tt, 0.05f, 3.0f, "%.2f")) {
                    state.xrVehicleThrottleTrim = tt;
                    changed = true;
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("How much of the throttle's full travel the left stick adds or\n"
                                      "removes per second while a weapon is out.\n"
                                      "0.5 = two seconds held to go from idle to floored.");
                }

                // LIVE STATE, so "it did not grab" and "it steers the wrong way" are both answerable
                // without a log: the armed mask is raised by proximity, the blends by the grab itself,
                // and the angle is what the stick is being driven from.
                if(cvr::RuntimeDiagnosticsEnabled()) {
                    const int mask = static_cast<int>(
                        OpenXRManager::Get().GetSharedSlot(vrshared::kWheelArmedMask));
                    const int horn = cvr::anim::g_wheelHornMask.load(std::memory_order_relaxed);
                    ImGui::Text("driving %d   at wheel  L %d R %d   hold  L %.2f R %.2f",
                                g_isDriving.load(std::memory_order_relaxed) ? 1 : 0,
                                (mask & vrshared::kWheelArmedLeftBit) ? 1 : 0,
                                (mask & vrshared::kWheelArmedRightBit) ? 1 : 0,
                                cvr::anim::g_wheelBlendLeft.load(std::memory_order_relaxed),
                                cvr::anim::g_wheelBlendRight.load(std::memory_order_relaxed));
                    ImGui::Text("steer  %+.1f deg  ->  stick %+.2f     horn  L %d R %d",
                                cvr::anim::g_wheelSteerDeg.load(std::memory_order_relaxed),
                                cvr::anim::g_wheelSteer.load(std::memory_order_relaxed),
                                (horn & vrshared::kWheelArmedLeftBit) ? 1 : 0,
                                (horn & vrshared::kWheelArmedRightBit) ? 1 : 0);
                }
            }

        ImGui::Separator();
        ImGui::TextUnformatted("SEATED CAMERA OFFSET");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip(
                "Head offsets in GENERAL are a standing calibration. Seated, the game's own\n"
                "vehicle camera is already where it should be -- which is why the port drops\n"
                "its two automatic bakes in a vehicle -- so a standing offset carries the view\n"
                "off the seat instead of correcting it.\n\n"
                "These three are added to the Head sliders while you are in a vehicle and\n"
                "ignored the moment you step out, so the car and the street can be tuned\n"
                "separately. Zero = the car keeps exactly the offset it has today.");
        }
        const bool passengerCombat=g_isInVehicle && cvr::anim::IsPassengerWindowCombat(
            g_vehicleState.load(std::memory_order_relaxed));
        ImGui::BeginDisabled(passengerCombat);
        changed |= widgets::SliderFloat("Car Head X right", &state.xrVehHeadOffsetX, -0.50f, 0.50f, "%.3f m");
        changed |= widgets::SliderFloat("Car Head Y forward", &state.xrVehHeadOffsetY, -0.50f, 0.50f, "%.3f m");
        changed |= widgets::SliderFloat("Car Head Z up", &state.xrVehHeadOffsetZ, -0.50f, 0.50f, "%.3f m");
        ImGui::EndDisabled();
        {   // Live, so a slider that is doing nothing says so instead of being blamed.
            ImGui::TextDisabled(passengerCombat ? "   (window combat: use GENERAL > Head offsets)"
                               : g_isInVehicle ? "   (in a vehicle: these are live)"
                                              : "   (on foot: these are ignored)");
        }
    return changed;
}

bool DrawFpsOverlayControls(LiveControlsUiState& state) {
    bool changed=false;auto& fg=state.framegen;
            changed|=ImGui::Checkbox("FPS overlay",&fg.overlay);
            changed|=widgets::Combo("Overlay layout",&fg.overlayLayout,"Detailed\0Compact\0Minimal\0");
            changed|=widgets::Combo("Overlay position",&fg.overlayCorner,"Top left\0Top right\0Bottom left\0Bottom right\0Top center\0Bottom center\0Left center\0Right center\0Center\0");
            changed|=widgets::SliderFloat("Overlay size",&fg.overlayScale,.5f,2,"%.2fx");
            changed|=widgets::SliderFloat("Overlay distance",&fg.overlayDistance,.5f,5,"%.2f m");
            changed|=widgets::SliderFloat("Overlay horizontal offset",&fg.overlayX,-60,60,"%.1f deg");
            changed|=widgets::SliderFloat("Overlay vertical offset",&fg.overlayY,-45,45,"%.1f deg");
            changed|=CheckboxInt("Overlay free look",&fg.overlayFollow);
            ImGui::BeginDisabled(!fg.overlayFollow);
            changed|=widgets::SliderFloat("Overlay free-look cone",&fg.overlayCone,5,90,"%.1f deg");
            if(ImGui::IsItemHovered())ImGui::SetTooltip("Hold the panel heading inside this yaw range; catch up beyond it.");
            ImGui::EndDisabled();
            int history=fg.historySeconds==10?0:(fg.historySeconds==60?2:(fg.historySeconds==120?3:1));
            if(widgets::Combo("History window",&history,"10 seconds\0 30 seconds\0 60 seconds\0 120 seconds\0")) {const int seconds[]={10,30,60,120};fg.historySeconds=seconds[history];changed=true;}
            if(fg.overlay) {
            if(ImGui::Button("Reset statistics"))cvr::framegen::ResetStatistics();
            const auto stats=cvr::framegen::GetStatistics();
            ImGui::Text("Real %.1f FPS   Framegen %.1f FPS   Output %.1f FPS",stats.realFps,stats.generatedFps,stats.outputFps);
            ImGui::Text("Average %.1f FPS   Min %.1f   Max %.1f",stats.averageFps,stats.minimumFps,stats.maximumFps);
            if(stats.low1Fps>0)ImGui::Text("1%% low %.1f FPS",stats.low1Fps);else ImGui::TextDisabled("1%% low: collecting 100 real frame intervals");
            if(stats.low01Fps>0)ImGui::Text("0.1%% low %.1f FPS",stats.low01Fps);else ImGui::TextDisabled("0.1%% low: collecting 1000 real frame intervals");
            const float graphMax=float(std::max(33.34,stats.budgetMs*2));
            ImGui::PlotLines("CPU history (ms)",stats.cpuGraph.data(),int(stats.cpuGraph.size()),0,nullptr,0,graphMax,ImVec2(0,55));
            ImGui::PlotLines("GPU history (ms)",stats.gpuGraph.data(),int(stats.gpuGraph.size()),0,nullptr,0,graphMax,ImVec2(0,55));
            ImGui::Text("CPU frame %.2f ms   Peak %.2f ms",stats.cpuMs,stats.cpuPeakMs);
            if(stats.gpuValid)ImGui::Text("GPU frame %.2f ms   Peak %.2f ms",stats.gpuMs,stats.gpuPeakMs);
            else ImGui::TextDisabled("GPU frame timing: waiting for completed queries");
            ImGui::Text("Generation %.2f ms   Tracked VRAM %.1f MiB",stats.generationMs,double(stats.vramBytes)/(1024*1024));
            const auto& hw=stats.hardware;constexpr double gib=1024.0*1024*1024;
            ImGui::Text("CPU: %s",hw.cpuName[0]?hw.cpuName:"--");
            ImGui::Text("GPU: %s",hw.gpuName[0]?hw.gpuName:"--");
            if(hw.cpuUsage>=0)ImGui::Text("CPU load %.1f%%   Busiest logical core %.1f%%",hw.cpuUsage,hw.coreMaximum);
            if(hw.gpuUsage>=0)ImGui::Text("GPU load %.1f%%   Temperature %.0f C",hw.gpuUsage,hw.gpuTemperature);
            if(hw.gpuMemoryValid)ImGui::Text("GPU memory %.2f / %.2f GiB",hw.gpuMemoryUsed/gib,hw.gpuMemoryTotal/gib);
            if(hw.gpuProcessMemoryValid)ImGui::Text("Game GPU memory %.2f GiB   Budget %.2f GiB",hw.gpuProcessUsed/gib,hw.gpuProcessBudget/gib);
            ImGui::Text("RAM %.2f / %.2f GiB   Game working set %.2f GiB",hw.ramUsed/gib,hw.ramTotal/gib,hw.appWorkingSet/gib);
            ImGui::Text("Depth + motion: %s   Skipped %llu",stats.inputsReady?"both eyes observed":"waiting",stats.skipped);
            ImGui::TextDisabled("CPU: captured frame cadence. GPU: graphics-queue frame interval. Peaks: last 2 seconds.");
            ImGui::TextDisabled("Output FPS excludes repeated frames; it is not a headset scanout measurement.");
            ImGui::TextDisabled("Lows use the mean of the slowest 1%% / 0.1%% of real-frame intervals. Hardware polling: 1 Hz.");
            } else ImGui::TextDisabled("FPS overlay and statistics are off.");
    return changed;
}

bool DrawLiveControls(LiveControlsUiState& state,int section) {
    bool changed = false;

    if (ImGui::Button("Recenter HMD (F7)")) {
        RequestLiveControlsRecenter();
    }
    ImGui::SameLine();
    if (ImGui::Button("Save")) {
        SetLiveControlsUiState(&state, 1);
    }

    if (section>=0 || ImGui::BeginTabBar("CyberpunkVRPortTabs")) {
        if (section>=0 ? section==0 : ImGui::BeginTabItem("GENERAL")) {
            if (ImGui::CollapsingHeader("GAME MENUS", ImGuiTreeNodeFlags_DefaultOpen)) {
                changed |= CheckboxInt("VR menu quad", &state.xrMenuRect);
                changed |= widgets::SliderFloat("VR menu FOV", &state.xrMenuFov, 30.0f, 120.0f, "%.1f deg");
            }

            if (ImGui::CollapsingHeader("Tracking / Camera")) {
        ImGui::TextUnformatted("Locomotion direction is set in the Controls tab.");
        changed |= CheckboxInt("Disable Mouse Y (Pitch)", &state.xrDisableMouseY);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Suppress mouse/right-stick pitch so only the HMD controls\n"
                              "vertical look. Applied by the CET VRIK mod and the\n"
                              "XInput merge. On by default.");
        }
        // "Fix Head" removed. It switched the view to 3DoF, and it did not stop at dropping the
        // head translation -- it dropped these three offsets and the calibration bakes with it,
        // then hid the very sliders that were needed to put the view right. The offsets are
        // always live now, and they reach BOTH eyes.
        changed |= widgets::SliderFloat("Head X right", &state.xrHeadOffsetX, -0.50f, 0.50f, "%.3f m");
        changed |= widgets::SliderFloat("Head Y forward", &state.xrHeadOffsetY, -0.50f, 0.50f, "%.3f m");
        changed |= widgets::SliderFloat("Head Z up", &state.xrHeadOffsetZ, -0.50f, 0.50f, "%.3f m");

            }

            if (ImGui::CollapsingHeader("Debug Gizmos")) {
                ImGui::TextUnformatted("Raw hand overlay / debug gizmos:");
                ImGui::Checkbox("Enable hand overlay", &g_drawHandLocator);
                ImGui::Checkbox("Draw 3D hand proxy", &g_drawHandProxy3D);
                ImGui::Checkbox("Draw debug wire/axes", &g_drawHandDebugAxes);
                widgets::SliderFloat("Locator scale", &g_handLocatorScale, 0.50f, 2.00f, "%.2f");
            }

            if (ImGui::CollapsingHeader("Diagnostics")) {
        { int vl = g_verboseLog; if (CheckboxInt("Verbose log (spammy diag)", &vl)) g_verboseLog = vl; }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Off by default for a clean cyberpunkvrport.log. Enable only\n"
                              "when capturing ClipCursor / depth / hook diagnostics.");
        }
            }
            if(section<0)ImGui::EndTabItem();
        }

        if (section>=0 ? section==1 : ImGui::BeginTabItem("FRAMEGEN")) {
            auto& fg=state.framegen;
            changed|=ImGui::Checkbox("Enable frame generation",&fg.enabled);
            if(ImGui::IsItemHovered())ImGui::SetTooltip("Hand smoothing is automatically disabled while frame generation is enabled.");
            int backend=int(fg.backend);
            if(widgets::Combo("Frame generator",&backend,"FidelityFX\0NVIDIA OFA + FidelityFX\0")) {fg.backend=cvr::framegen::Backend(backend);changed=true;}
            if(fg.backend==cvr::framegen::Backend::Nvidia)
                changed|=widgets::Combo("NVIDIA quality",&fg.quality,"Fast (performance)\0Medium (balanced)\0Slow (best)\0");
            int scale=fg.flowScale==100?2:(fg.flowScale==75?1:0);
            if(widgets::Combo("Flow resolution",&scale,"50%\0 75%\0 100%\0")) {fg.flowScale=scale==2?100:(scale==1?75:50);changed=true;}
            changed|=ImGui::Checkbox("Pace real frames at half headset refresh",&fg.autoPace);
            ImGui::TextWrapped("Uses game motion vectors and depth in both eyes. Adds one intermediate frame between real frames.");
            ImGui::Separator();
            const auto runtimeStatus=cvr::framegen::GetRuntimeStatus();
            ImGui::TextWrapped("%s",runtimeStatus.text);
            if(section<0)ImGui::EndTabItem();
        }

        if (section>=0 ? section==2 : ImGui::BeginTabItem("HUD")) {
            changed |= CheckboxInt("Enable HUD panel", &state.xrHudPanel);
            const char* followModes[] = {"Head / free-look cone", "Body rotation"};
            changed |= widgets::Combo("Follow", &state.xrHudFollowMode, followModes, 2);
            ImGui::BeginDisabled(state.xrHudFollowMode != 0);
            changed |= widgets::SliderFloat("Free-look cone", &state.xrHudFollowDeg, 5.0f, 90.0f, "%.1f deg");
            if(ImGui::IsItemHovered())ImGui::SetTooltip("Catch up beyond the cone, or after 3 seconds at rest when the yaw offset is over 10 degrees inside it.");
            ImGui::EndDisabled();
            changed |= widgets::SliderFloat("HUD FOV", &state.xrHudFov, 30.0f, 120.0f, "%.1f deg");
            changed |= CheckboxInt("Stereo depth", &state.xrHudStereoDepth);
            if (ImGui::IsItemHovered()) ImGui::SetTooltip("Off: the HUD has the same direction in both eyes.\nOn: place it at the selected physical depth.");
            ImGui::BeginDisabled(!state.xrHudStereoDepth);
            changed |= widgets::SliderFloat("Depth", &state.xrHudDistance, 0.5f, 5.0f, "%.2f m");
            ImGui::EndDisabled();
            changed |= widgets::SliderFloat("Brightness", &state.xrHudBrightness, 0.25f, 3.0f, "%.2fx");
            changed |= widgets::SliderFloat("Shadow / outline", &state.xrHudShadow, 0.0f, 2.0f, "%.2fx");
            changed |= widgets::SliderFloat("Glow", &state.xrHudGlow, 0.0f, 2.0f, "%.2fx");
            ImGui::Separator();
            if(ImGui::CollapsingHeader("INTERACTIONS / DIALOGS",ImGuiTreeNodeFlags_DefaultOpen)){
                changed |= CheckboxInt("World-space interactions / dialogs",&state.xrInteractionPanel);
                changed |= widgets::SliderFloat("Interaction free-look cone",&state.xrInteractionFollowDeg,5.0f,90.0f,"%.1f deg");
                changed |= widgets::SliderFloat("Loot free-look cone",&state.xrLootFollowDeg,5.0f,90.0f,"%.1f deg");
                if(ImGui::IsItemHovered())ImGui::SetTooltip("Used while the loot panel and item description are visible. Default: 10 degrees.");
                changed |= widgets::SliderFloat("Interaction distance",&state.xrInteractionDistance,.5f,5.0f,"%.2f m");
                changed |= widgets::SliderFloat("Interaction FOV",&state.xrInteractionFov,30.0f,120.0f,"%.1f deg");
                ImGui::TextWrapped("Interaction prompts and dialog choices share this panel. It follows only beyond the cone; no delayed catch-up at rest.");
            }
            ImGui::Separator();
            static int element=10;
            const char* elementLabels[cvr::hud::ElementCount];
            for(size_t i=0;i<cvr::hud::ElementCount;++i)elementLabels[i]=cvr::hud::Elements[i].label;
            widgets::Combo("Element",&element,elementLabels,int(cvr::hud::ElementCount));
            const auto status=cvr::hud::GetLayoutStatus();
            if(!status[element].available) ImGui::TextDisabled("Not currently visible in gameplay; settings are still saved.");
            auto& transform=state.hudElements[element];
            changed |= CheckboxInt("Show element", &transform.visible);
            changed |= widgets::SliderFloat("Horizontal offset", &transform.x, -100.0f,100.0f,"%.1f %%");
            changed |= widgets::SliderFloat("Vertical offset", &transform.y, -100.0f,100.0f,"%.1f %%");
            changed |= widgets::SliderFloat("Size", &transform.scale,0.1f,3.0f,"%.2fx");
            changed |= widgets::SliderFloat("Opacity", &transform.opacity,0.0f,1.0f,"%.2f");
            if(ImGui::Button("Reset element")) {transform={};changed=true;}
            ImGui::SameLine();
            if(ImGui::Button("Reset all elements")) {state.hudElements={};changed=true;}
            if(section<0)ImGui::EndTabItem();
        }

        if (section>=0 ? section==3 : ImGui::BeginTabItem("CONTROLS")) {
            static int page=0;
            static const char* pages[]={"GENERAL","DRIVING","BINDINGS"};
            widgets::Tabs("controls-pages",page,pages,3);
            if(page==1)changed|=DrawDrivingControls(state);
            else if(page==2)widgets::DrawBindings();
            else {
            ImGui::TextWrapped("Movement, weapon aiming and physical interaction shortcuts.");

            // Weapon aim: bullets/projectiles fly down the WEAPON BARREL (controller-pointed) instead
            // of the camera crosshair. Hooks the projectile launch orientation provider and feeds it
            // the game's own muzzle world transform. Writes shared[58]; the RED4ext plugin applies it.
            {
                static bool s_weaponAim = true;   // default ON — backend's m_weaponAimEnable also defaults to 1
                if (ImGui::Checkbox("Hand aim  (off = Decoupled VR Head Aim)", &s_weaponAim)) {
                    OpenXRManager::Get().SetWeaponAimEnable(s_weaponAim ? 1 : 0);
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("ON (Hand Aim): the controller points the weapon and VRIK drives the arms.\n"
                                      "OFF (Decoupled VR Head Aim): the WEAPON follows your head instead, the game\n"
                                      "keeps owning its position and its ADS animations, and VRIK stands down for\n"
                                      "the weapon arm. Either way the shot leaves the real muzzle, for guns and\n"
                                      "projectiles alike, and free-look while aiming is preserved.");
                }
                ImGui::Checkbox("Weapon Aim laser dot (where the bullet hits)", &g_drawBarrelCross);
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("Red dot projected from the actual weapon muzzle direction through the\n"
                                      "game camera -- marks exactly where the bullet will fly.");
                }
            }
            ImGui::Separator();

            changed |= CheckboxInt("Enable VR -> XInput merge", &state.xrXInputHook);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("OR the VR controller state into XInput gamepad 0 every poll.\n"
                                  "Off = the game only sees a physical pad / nothing.");
            }

            ImGui::Separator();
            ImGui::TextUnformatted("Weapon holsters (reach + right grip)");
            changed |= CheckboxInt("Immersive holsters", &state.xrImmersiveHolsters);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip(
                    "ON  - equip is chosen by the VISUAL holster you reach for:\n"
                    "      over the right shoulder = primary weapon (rifle / sniper),\n"
                    "      hip with a katana = melee, hip with a pistol = sidearm.\n"
                    "OFF - ignore visual holsters, fixed slot per zone:\n"
                    "      over-shoulder = EquipmentSlot1, right hip = Slot2, left hip = Slot3.\n"
                    "Reach to the zone and squeeze the RIGHT grip to equip / unequip.");
            }


            ImGui::Separator();
            ImGui::TextUnformatted("Locomotion direction");
            bool analogMovement=state.xrMovementSpeedMode==1;
            if(ImGui::Checkbox("Analog movement",&analogMovement)) {state.xrMovementSpeedMode=analogMovement?1:0;changed=true;}
            if(ImGui::IsItemHovered())ImGui::SetTooltip("On: speed follows left-stick travel. Off: the current fixed-speed movement (default).\nVehicle controls keep their existing response.");
            ImGui::BeginDisabled(!analogMovement);
            int leftDeadzone=int(state.xrLeftStickDeadzone*100+.5f);
            int rightDeadzone=int(state.xrRightStickDeadzone*100+.5f);
            int fullInput=int(state.xrMaxInputThreshold*100+.5f);
            if(widgets::SliderInt("Left stick deadzone",&leftDeadzone,0,30,"%d%%")){state.xrLeftStickDeadzone=leftDeadzone*.01f;changed=true;}
            if(widgets::SliderInt("Right stick deadzone",&rightDeadzone,0,30,"%d%%")){state.xrRightStickDeadzone=rightDeadzone*.01f;changed=true;}
            if(widgets::SliderInt("Full input threshold",&fullInput,80,100,"%d%%")){state.xrMaxInputThreshold=fullInput*.01f;changed=true;}
            if(ImGui::IsItemHovered())ImGui::SetTooltip("Raw travel that produces full analog input and activates sprint, dash and crouch.\nThe usable travel between the deadzone and this threshold maps to 0-100%%.");
            ImGui::EndDisabled();
            const char* moveSrcNames[] = { "Game (camera)", "HMD (head)", "Left hand", "Right hand" };
            int moveSrc = state.xrMovementSource;
            if (moveSrc < 0 || moveSrc > 3) moveSrc = state.xrMovementControl != 0 ? 1 : 0;
            if (widgets::Combo("Move source", &moveSrc, moveSrcNames, IM_ARRAYSIZE(moveSrcNames))) {
                state.xrMovementSource = moveSrc;
                state.xrMovementControl = moveSrc != 0 ? 1 : 0;
                changed = true;
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Game     - left stick walks the way the camera faces (vanilla).\n"
                                  "HMD      - left stick walks the way the headset faces.\n"
                                  "Left/Right hand - walks the way the chosen controller points.\n"
                                  "Vehicles always keep game heading.");
            }

            ImGui::Separator();
            changed |= CheckboxInt("Breaststroke swimming", &state.xrBreaststrokeSwim);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("In water: reach forward with both hands, sweep out and pull back.\n"
                                  "Movement begins during the pull. Gentle strokes swim normally; fast pulls boost.\n"
                                  "Swim where the headset points, including up/down; no stick needed.\n"
                                  "To rise, raise separated hands to chest level and push both down.\n"
                                  "Repeat strokes to keep moving. Pull the left stick back to stop.");
            }
            ImGui::Separator();
            changed |= CheckboxInt("Ladder grip climbing", &state.xrLadderGripClimb);
            if(ImGui::IsItemHovered())ImGui::SetTooltip("Grip a side rail or rung. Pull the hand down to climb up,\n"
                "or move it up to climb down. Release to stop. Native pelvis and legs stay on the ladder.");
            changed |= CheckboxInt("Auto finish near ladder top", &state.xrLadderAutoFinish);
            if(ImGui::IsItemHovered())ImGui::SetTooltip("After an upward pull near the top, finish climbing even when grips are released.\n"
                "Move the stick or push a held hand upward to cancel.");
            if(state.xrLadderAutoFinish)changed |= widgets::SliderFloat("Ladder finish distance", &state.xrLadderFinishDistance,.2f,1.2f,"%.2f m");
            ImGui::Separator();
            ImGui::TextUnformatted("Turning (right stick)");
            changed |= CheckboxInt("Snap turn", &state.xrSnapTurn);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Convert the right-stick X axis into discrete snap pulses\n"
                                  "instead of smooth rotation. Helps with motion sickness.");
            }
            if (state.xrSnapTurn != 0) {
                changed |= widgets::SliderFloat("Snap angle", &state.xrSnapTurnAngleDeg, 10.0f, 90.0f, "%.0f deg");
            }

            }
            if(section<0)ImGui::EndTabItem();
        }

        if (section>=0 ? section==4 : ImGui::BeginTabItem("STEREO")) {
            if(ImGui::CollapsingHeader("PERFORMANCE",ImGuiTreeNodeFlags_DefaultOpen)) {
                int reflex=state.nvidiaReflex+1;
                if(widgets::Combo("NVIDIA Reflex",&reflex,"Game setting\0Off (higher FPS)\0On (lower latency)\0On + Boost\0")) {
                    state.nvidiaReflex=reflex-1;changed=true;
                }
                ImGui::TextWrapped("Off can improve GPU utilization and FPS, but may increase input latency.");
                const int applied=cvr::reflex::GetAppliedMode();
                const char* appliedName=applied==0?"Off":applied==1?"On":applied==2?"On + Boost":"Waiting for renderer";
                ImGui::TextDisabled("Applied: %s",appliedName);
            }
            if (ImGui::CollapsingHeader("STEREO VIEW", ImGuiTreeNodeFlags_DefaultOpen)) {
                changed |= DrawFovControl(state);
        changed |= widgets::SliderFloat("Motion prediction (ms)", &state.xrMotionPredictMs, 0.0f, 60.0f, "%.1f ms");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Forward-predicts the head pose by this many ms using head\n"
                              "velocity, hiding render-to-photon latency. 0 = off.\n"
                              "Tune up until motion feels responsive without overshoot.");
        }
        changed |= widgets::SliderFloat("Stereo separation x", &state.xrStereoScale, 0.25f, 5.0f, "%.2fx");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Personal fine-tune on the auto IPD. 1.0 = calibrated natural\n"
                              "separation, auto-scaled to the headset's runtime IPD. Nudge\n"
                              "0.8-1.2 for taste; crank to 3-5x to exaggerate depth and make\n"
                              "the eye alternation obvious on the flat monitor for testing.");
        }
        // ── World scale + honest IPD ──
        changed |= widgets::SliderFloat("World scale", &state.xrWorldScale, 0.20f, 3.0f, "%.2f");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Scales eye separation AND head translation together.\n"
                              "Lower it (e.g. 0.8) to make the world look\n"
                              "BIGGER / yourself smaller; raise to shrink the world. Use this if V\n"
                              "and NPCs feel too large.");
        }
        changed |= widgets::SliderFloat("IPD scale", &state.xrIpdScale, 0.50f, 2.0f, "%.2fx");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Eye-separation multiplier on the runtime IPD. 1.0 = the neutral\n"
                              "baseline (+-0.033 m on a typical headset).\n"
                              "Affects stereo depth (diorama vs giant), NOT the monocular size.");
        }
        bool reuseLastFrame = state.xrReuseLastFrame != 0;
        if (ImGui::Checkbox("Reuse last clean frame", &reuseLastFrame)) {
            state.xrReuseLastFrame = reuseLastFrame ? 1 : 0;
            changed = true;
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("On stale ticks, re-submit the last clean captured eye and let\n"
                              "the compositor reproject it, instead of warping stale content\n"
                              "again. May lower submit rate\n"
                              "toward the capture rate. Off = always warp the stale eye.");
        }
        // The "Pose pair-lock" checkbox is gone: it toggled a freeze that only meant something
        // under AER's one-camera eye alternation. The ini key still round-trips so old files load.
            }

            DrawStereoControls();
            if(section<0)ImGui::EndTabItem();
        }

        if (section>=0 ? section==5 : ImGui::BeginTabItem("AVATAR")) {
            DrawVRHandsControls();
            if(section<0)ImGui::EndTabItem();
        }

        if(section<0)ImGui::EndTabBar();
    }

    return changed;
}

}  // namespace overlay
using namespace overlay;
