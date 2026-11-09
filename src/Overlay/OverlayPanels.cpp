#include "Framegen/FlightRecorder.hpp"
#include "Framegen/FrameCapture.hpp"
#include "Framegen/RefreshCadence.hpp"
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
#include "Anim/WheelRange.hpp"
#include "Runtimes/HybridBodyYaw.hpp"
#include "Anim/VehiclePosePolicy.hpp"
#include "Utils/SharedSlots.hpp"
#include "Runtimes/WristBase.hpp"
#include "Core/VrCoreShared.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Fbt/Game/Menu.hpp"
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
#include "Overlay/UiRec.hpp"
#include "Utils/DebugGate.hpp"
#include "Camera/CameraState.hpp"
#include "Hooks/Reflex.hpp"
#include "Framegen/DlssFrameGen.hpp"
#include "Stereo/EngineStereo.hpp"

extern volatile int g_verboseLog; // per-frame log spam toggle (default off)
extern void Log(const char* fmt, ...);
extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);
extern volatile float g_lastLocateQuat[4];
extern "C" int32_t CyberpunkVR_OverlaySkipEmpty;
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
    const bool changed = UI::Checkbox(label, &checked);
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
    const bool changed = UI::InputInt(label, &temp, 1, 64);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool DrawFovControl(LiveControlsUiState& state) {
    bool changed = false;
    const bool psvr2 = OpenXRManager::Get().IsRuntimePsvr2();
    bool overrideEnabled = state.xrForceFov > 0.0f;
    if (UI::Checkbox("Enable FOV override", &overrideEnabled)) {
        state.xrForceFov = overrideEnabled ? (psvr2 ? 104.5f : 112.0f) : 0.0f;
        changed = true;
    }
    if (!overrideEnabled) UI::BeginDisabled();
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
    if (!overrideEnabled) UI::EndDisabled();
    UI::TextWrapped("Override changes BOTH the game VR camera and OpenXR projection. "
        "Off uses automatic panel-edge coverage, which can be wider than the per-eye FOV span.");
    if (psvr2) UI::TextWrapped("PSVR2: 104.5 deg restores the previous override. Automatic coverage measured 123 deg on this setup.");
    UI::TextWrapped("Narrower coverage can improve central detail at the same resolution, but may reveal black borders. "
        "Quest 3 legacy is an optional comparison, not a universal correction; keep the matching headset resolution preset.");
    const float actual = GetGameRenderFovDeg();
    const float vertical = GetGameRenderVerticalFovDeg();
    if (actual > 1.0f && vertical > 1.0f)
        UI::Text("Camera render FOV: %.1f H / %.1f V deg", actual, vertical);
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
    if (UI::Checkbox("Start VR hand tracking", &s_vrHandTracking)) {
        OpenXRManager::Get().SetVRHandTrackingMode(s_vrHandTracking ? 4 : 0);
    }
    UI::SameLine();
    if (UI::Button("Log VR Diag")) {
        OpenXRManager::Get().RequestVRDiag();
    }
    if (UI::IsItemHovered()) {
        UI::SetTooltip("Dumps gizmo vs. bone poses to vrik_diag.txt (next to dxgi.dll)\n"
                          "for tuning the arm IK. Same as the CET 'Log VR Diag' button.");
    }

    UI::Separator();
    // Select one body-yaw policy; persist the compatible INI flags together.
    {
        LiveControlsUiState st{};
        GetLiveControlsUiState(&st);
        bool handsOnly = st.xrBodyHandsOnly != 0;
        if (UI::Checkbox("Hands only (hide the body)", &handsOnly)) {
            st.xrBodyHandsOnly = handsOnly ? 1 : 0;
            SetLiveControlsUiState(&st, 1);
        }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Draw only your hands: the rest of the body and all clothing are hidden,\n"
                              "the arms are cut at the wrists. The tactical belt and the weapons\n"
                              "hanging in their holsters stay visible.");
        }
        bool roomscale = st.xrRoomscaleMovement != 0;
        if (UI::Checkbox("Roomscale movement", &roomscale)) {
            st.xrRoomscaleMovement = roomscale ? 1 : 0;
            SetLiveControlsUiState(&st, 1);
        }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Move the player capsule with physical horizontal head movement.\n"
                              "Uses native collision/steps and preserves stick or WASD movement.\n"
                              "Suspended in menus, vehicles and scripted movement.");
        }
        using BodyMode=cvr::body::RotationMode;
        int mode=static_cast<int>(cvr::body::RotationModeFromFlags(st.xrPhysicalBodyRotation,
            st.xrTrackedBodyRotation,st.xrHybridBodyRotation,st.xrHeadLedBodyRotation));
        static const char* bodyModes[]{"Stick / snap only","Head + free-look cone",
            "HMD + controllers","Hybrid","Head-led (default)"};
        UI::SetNextItemWidth(410.0f);
        if(widgets::Combo("Body rotation",&mode,bodyModes,5)){
            st.xrPhysicalBodyRotation=mode==static_cast<int>(BodyMode::HeadCone);
            st.xrTrackedBodyRotation=mode==static_cast<int>(BodyMode::Tracked);
            st.xrHybridBodyRotation=mode==static_cast<int>(BodyMode::Hybrid);
            st.xrHeadLedBodyRotation=mode==static_cast<int>(BodyMode::HeadLed);
            SetLiveControlsUiState(&st,1);
        }
        if(UI::IsItemHovered())UI::SetTooltip("Head-led: the body stays while the head looks within 10 degrees, then turns\n"
            "to the head at 300 deg/s; looking down (past 30 degrees) it stays until 70 degrees. No controllers.\n"
            "Hybrid: the head cone upright, HMD + controllers looking down or bent.\n"
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
        // Each combo marks its own default (LauncherConfig.cpp): on foot Tier 3+, in a vehicle Tier 4+.
        static const int kTierValues[] = { -1, 1, 2, 3, 4 };
        static const char* kOnFootTierLabels[] = {
            "Never (VRIK always on)",
            "Staged scenes and up (Tier 2+)",
            "Tier 3 and up  [default]",
            "Cinematics (Tier 4+)",
            "Full cinematics only (Tier 5)",
        };
        static const char* kVehicleTierLabels[] = {
            "Never (VRIK always on)",
            "Staged scenes and up (Tier 2+)",
            "Tier 3 and up",
            "Cinematics (Tier 4+)  [default]",
            "Full cinematics only (Tier 5)",
        };
        auto sceneSuspendControl=[&](const char* label,const char* id,int& tier,const char* const* labels,int fallback,
                                     const char* help) {
            int idx=fallback;
            for(int i=0;i<5;++i)if(kTierValues[i]==tier) { idx=i;break; }
            const std::string controlLabel=std::string(label)+id;
            if(widgets::Combo(controlLabel.c_str(),&idx,labels,5)) {
                tier=kTierValues[idx];SetLiveControlsUiState(&st,1);
            }
            if(UI::IsItemHovered())UI::SetTooltip("%s",help);
        };
        sceneSuspendControl("Suspend VRIK in cutscenes (on foot)","##cutsceneSuspend",
            st.xrCutsceneSuspendTier,kOnFootTierLabels,2,
            "Applies only on foot. VRIK yields to the scene animation at or above this tier.\n"
            "The vehicle threshold is configured separately below.");
        sceneSuspendControl("Suspend VRIK in cutscenes (vehicle)","##cutsceneSuspendVehicle",
            st.xrVehicleCutsceneSuspendTier,kVehicleTierLabels,3,
            "Applies in vehicles, including passenger window combat.\n"
            "VRIK yields to the scene animation at or above this tier. Never keeps VRIK enabled.");
        {
            float comfort=st.xrSceneCamComfortDeg;
            if(widgets::SliderFloat("Scene camera smoothing",&comfort,0.0f,45.0f,"%.0f deg")) {
                st.xrSceneCamComfortDeg=comfort;SetLiveControlsUiState(&st,1);
            }
            if(UI::IsItemHovered())UI::SetTooltip("While a scene drives the camera, its turning within this angle does not\n"
                "turn the view; past it the view eases after the scene camera. Cuts still switch at once.\n"
                "0 = the scene camera exactly as the game moves it. Default: 30 degrees.");
        }
        UI::BeginDisabled(!bodyRot || trackedBody);
        float cone = st.xrBodyFreeLookDeg;
        if (widgets::SliderFloat("Free-look cone", &cone, 0.0f, 60.0f, "%.0f deg")) {
            st.xrBodyFreeLookDeg=cone;SetLiveControlsUiState(&st,1);
        }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("How far your head may turn before the body starts coming around.\n"
                              "The body settles toward the centre, then restores the full free-look zone.\n"
                              "Default: 10 degrees. Hybrid uses this value while looking ahead.");
        }
        UI::EndDisabled();
        UI::BeginDisabled(mode!=static_cast<int>(BodyMode::HeadCone));
        float downCone=st.xrBodyFreeLookDownDeg;
        if(widgets::SliderFloat("Free-look cone when looking down",&downCone,0.0f,90.0f,"%.0f deg")) {
            st.xrBodyFreeLookDownDeg=downCone;SetLiveControlsUiState(&st,1);
        }
        if(UI::IsItemHovered())UI::SetTooltip("Extra room to reach chest-mounted items.\n"
            "Blends in from 10 to 30 degrees of downward head tilt.\n"
            "Never narrows the normal cone; set the same value to disable widening.");
        float swimCone=st.xrBodyFreeLookSwimDeg;
        if(widgets::SliderFloat("Free-look cone in water",&swimCone,0.0f,90.0f,"%.0f deg")) {
            st.xrBodyFreeLookSwimDeg=swimCone;SetLiveControlsUiState(&st,1);
        }
        if(UI::IsItemHovered())UI::SetTooltip("One fixed cone on the surface and underwater.\n"
            "Looking down and physical bending do not widen it. Default: 5 degrees.");
        UI::EndDisabled();
        if(trackedBody)UI::TextDisabled("Body free-look cones are disabled in HMD + controllers mode.");
        if(hybridBody)UI::TextDisabled("Hybrid: head cone upright; HMD + controllers when looking down or bending.");
        float radiusCm=st.xrBodyMoveRadius*100;
        if(widgets::SliderFloat("Body movement free zone",&radiusCm,0.0f,15.0f,"%.1f cm")) {
            st.xrBodyMoveRadius=radiusCm*.01f;SetLiveControlsUiState(&st,1);
        }
        if(UI::IsItemHovered())UI::SetTooltip("Small physical head movements leave the body in place.\n"
            "Outside this zone the body follows smoothly; camera tracking remains immediate.");
        if (bodyRot && !trackedBody && cvr::RuntimeDiagnosticsEnabled()) {
            UI::Text("active cone %.1f deg | realign %+.1f deg | head-vs-body %+.1f deg",
                        CyberpunkVR_BodyYawFollowDeadDeg,CyberpunkVR_DebugBodyFollowOffsetDeg,CyberpunkVR_DebugBodyFollowErrDeg);
        }
    }

    UI::Separator();
    UI::TextUnformatted("Hand IK Calibration (per hand: R = right, L = left)");

    // Defaults mirror the plugin's baked calibration (main.cpp globals).
    static float scaleR = 1.05f, scaleL = 1.06f;   // reach scale (arm straightening)
    static float heightR = 0.0f, heightL = 0.0f; // vertical fine-tune offset (m)
    static float swingR = 1.0f,  swingL = 1.0f;    // elbow-swing gain
    static float poleR = 0.0f,   poleL = 0.0f;     // elbow pole spin (deg)
    // Wrist euler OFFSETS (deg) from the baked base (Runtimes/WristBase.hpp): 0 is the tuned hand.
    static float wRp = 0.0f, wRy = 0.0f, wRr = 0.0f;
    static float wLp = 0.0f, wLy = 0.0f, wLr = 0.0f;
    const float* wb = cvr::calib::kWristBase;

    // TWO-WAY SYNC (fixes calibration not being saved). These statics used to be
    // one-way UI -> manager: they kept their hardcoded defaults after the manager
    // loaded vrik_calibration.ini / ran auto-calibration, so the FIRST slider touch
    // (or Apply) pushed 14 default values over the real calibration -- which the next
    // Save then wrote to disk. Pull the manager's live values into the sliders
    // whenever the player isn't actively dragging.
    if (!UI::IsAnyItemActive()) {
        float c[14]; OpenXRManager::Get().GetVRHandCalib(c);
        scaleR=c[0]; scaleL=c[1]; heightR=c[2]; heightL=c[3];
        swingR=c[4]; swingL=c[5]; poleR=c[6]; poleL=c[7];
        wRp=c[8]-wb[0]; wRy=c[9]-wb[1]; wRr=c[10]-wb[2]; wLp=c[11]-wb[3]; wLy=c[12]-wb[4]; wLr=c[13]-wb[5];
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

    UI::Separator();
    UI::TextUnformatted("Wrist rotation offset (palm/finger alignment, deg)");
    calChanged |= widgets::SliderFloat("Wrist R pitch", &wRp, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist R yaw",   &wRy, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist R roll",  &wRr, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist L pitch", &wLp, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist L yaw",   &wLy, -180.0f, 180.0f, "%.1f");
    calChanged |= widgets::SliderFloat("Wrist L roll",  &wLr, -180.0f, 180.0f, "%.1f");

    UI::Separator();
    // Auto-calibration: T-pose sample from the same controller poses that draw the gizmo hands.
    // Press "Start", stretch arms out to the sides, stand straight. We derive shoulder offsets
    // and arm scale, then save to vrik_calibration.ini next to dxgi.dll.
    int  cState = OpenXRManager::Get().GetCalibrationState();
    if (cState == 1) {
        float prog = OpenXRManager::Get().GetCalibrationProgress();
        UI::TextColored(ImVec4(1.0f, 0.9f, 0.3f, 1.0f),
                           "AUTO-CALIBRATING: stretch arms STRAIGHT OUT to the sides,");
        UI::TextColored(ImVec4(1.0f, 0.9f, 0.3f, 1.0f),
                           "stand straight facing forward. Hold for %.1fs (%.0f%%).",
                           (1.0f - prog) * 4.0f, prog * 100.0f);
        UI::ProgressBar(prog, ImVec2(280, 0));
    } else {
        if (UI::Button("Start Auto-Calibration (T-pose, 4s)", ImVec2(280, 0))) {
            OpenXRManager::Get().StartAutoCalibration(4.0f);
        }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Press once, then stretch BOTH gizmo hands OUT to the sides at shoulder height\n"
                              "and stand straight facing forward. The mod measures the visible controller\n"
                              "positions, computes shoulder pivots + arm scale, and saves the result\n"
                              "to vrik_calibration.ini.");
        }
        if (cState == 2) {
            UI::SameLine();
            UI::TextColored(ImVec4(0.4f, 1.0f, 0.4f, 1.0f), "Saved.");
        }
    }

    UI::Separator();
    // Neutral rig alignment is automatic; this button only clears optional trims.
    {
        float cb[3]; OpenXRManager::Get().GetCameraOffset(cb);
        UI::TextUnformatted("Camera: neck + 15 cm forward, eyes + 10 cm up");
        if (UI::Button("Reset camera to neck", ImVec2(180, 0))) {
            OpenXRManager::Get().BakeCameraOffset();
        }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Uses the neutral neck, 15 cm forward and 10 cm above reference eyes.\n"
                              "Resets camera trims; no stance, head direction or Bake measurement is needed.");
        }
        UI::SameLine();
        if (UI::Button("Clear trim##cambake", ImVec2(90, 0))) {
            OpenXRManager::Get().ClearCameraOffset();
            OpenXRManager::Get().SaveCalibrationToFile();
        }
        UI::Text("camera mount: R %.3f  Fwd %.3f  Up %.3f", cb[0], cb[1], cb[2]);
    }

    UI::Separator();
    bool apply  = UI::Button("Apply Calibration");
    UI::SameLine();
    bool save   = UI::Button("Save");
    UI::SameLine();
    bool load   = UI::Button("Load");
    UI::SameLine();
    if (UI::Button("Reset Defaults")) {
        scaleR = 1.05f; scaleL = 1.06f; heightR = 0.0f; heightL = 0.0f;
        swingR = 1.0f; swingL = 1.0f; poleR = 0.0f; poleL = 0.0f;
        wRp = wRy = wRr = wLp = wLy = wLr = 0.0f;
        OpenXRManager::Get().SetShoulderAnatomical(0.14f, -0.17f, 0.05f, -0.14f, -0.17f, 0.05f);
        calChanged = true;
    }

    if (calChanged || apply) {
        OpenXRManager::Get().SetVRHandCalib(scaleR, scaleL, heightR, heightL,
                                            swingR, swingL, poleR, poleL,
                                            wb[0] + wRp, wb[1] + wRy, wb[2] + wRr,
                                            wb[3] + wLp, wb[4] + wLy, wb[5] + wLr);
    }
    // AUTOSAVE: persist edits once the drag/press is released, so tweaks survive a
    // restart without requiring the player to remember the Save button.
    {
        static bool s_calDirty = false;
        if (calChanged) s_calDirty = true;
        if (s_calDirty && !UI::IsAnyItemActive()) {
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
    ImGuiIO& io = UI::GetIO();

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
        UI::TextColored(ImVec4(1.0f, 0.55f, 0.35f, 1.0f), "Stereo module not installed.");
        UI::TextWrapped("The engine hooks did not load, so everything below is inert and the "
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
        UI::TextColored(ImVec4(0.45f, 0.9f, 0.5f, 1.0f),
                           "Stereo active  (%s = VRCAM, %s = MAIN)", kVrcamEye, kMainEye);
    } else if (submitOn) {
        UI::TextColored(ImVec4(1.0f, 0.8f, 0.35f, 1.0f),
                           "No second eye right now -> both eyes get MAIN (mono)");
        UI::TextDisabled("Expected in menus and while loading. If it stays here in gameplay: "
                            "the VRCAM component is off, or vrcam.json names a camera the player "
                            "entity does not carry.");
    } else {
        UI::TextDisabled("Stereo submit off -> both eyes get MAIN (mono)");
    }
    if (cvr::RuntimeDiagnosticsEnabled() && eyeEverProduced)
        UI::TextDisabled("vrcam eye age %u ms  (stale over %u)   eye submits %llu",
                            eyeAge, CyberpunkVR_StereoEyeMaxAgeMs,
                            static_cast<unsigned long long>(CyberpunkVR_DebugStereoEyeSubmits));
    else if(cvr::RuntimeDiagnosticsEnabled())
        UI::TextDisabled("vrcam eye: never produced a frame");
    UI::Separator();

    bool submit = submitOn;
    char submitLabel[64];
    std::snprintf(submitLabel, sizeof(submitLabel), "Send VRCAM to the %s EYE", kVrcamEye);
    if (UI::Checkbox(submitLabel, &submit))
        CyberpunkVR_StereoSubmit = submit ? 1 : 0;
    if (UI::IsItemHovered())
        UI::SetTooltip("Off = the old behaviour, MAIN duplicated into both eyes.\n"
                          "On = the %s eye gets the VRCAM view's own final colour --\n"
                          "the same image the desktop mirror shows -- re-encoded to the\n"
                          "swapchain's sRGB format and scaled to the eye size.", kVrcamEye);

    // No separate capture switch: the submit path drives CyberpunkVR_StereoEyeCapture from the
    // checkbox above, so the per-frame snapshot cost appears and disappears with the feature
    // instead of being a second thing to remember to turn off.
    if(cvr::RuntimeDiagnosticsEnabled())UI::TextDisabled("snapshot copies %llu   skips %llu   vrcam RTV hits %llu",
                        static_cast<unsigned long long>(CyberpunkVR_DebugStableCopies),
                        static_cast<unsigned long long>(CyberpunkVR_DebugStableSkips),
                        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorRtvHits));

    int maxAge = static_cast<int>(CyberpunkVR_StereoEyeMaxAgeMs);
    if (widgets::SliderInt("Eye staleness limit (ms)", &maxAge, 33, 1000))
        CyberpunkVR_StereoEyeMaxAgeMs = static_cast<uint32_t>(maxAge);
    if (UI::IsItemHovered())
        UI::SetTooltip("How long the last VRCAM frame stays usable. Past this the %s\n"
                          "eye falls back to MAIN -- one eye frozen while the other moves\n"
                          "is far worse to look at than mono.", kVrcamEye);

    UI::Separator();
    // Read-only on purpose. VRCAM's upscaler mirrors MAIN's, decided at graph build from MAIN's
    // own flags -- a switch here could only ever disagree with the engine.
    UI::TextDisabled("VRCAM DLSS: %s  (follows MAIN's upscaler, no switch)",
                        CyberpunkVR_VrcamDlss ? "ON" : "off");
    if (UI::IsItemHovered()) {
        UI::SetTooltip("On whenever MAIN is upscaling with DLSS: vrcam gets its own\n"
                          "Streamline viewport and renders below its target.\n"
                          "Off for DLAA / TAA / no upscaler. Change it in the game's\n"
                          "graphics settings, not here.");
    }
    bool forceCam = CyberpunkVR_ForceVrcamCam != 0;
    if (UI::Checkbox("Match VRCAM projection to MAIN  (fov / zoom / near / far)", &forceCam))
        CyberpunkVR_ForceVrcamCam = forceCam ? 1 : 0;

    UI::Separator();
    // Forces the RTT component's isEnabled through the game's RTTI (the CET side re-asserts it,
    // so it survives a reload/respawn). Off means the engine stops rendering the second view
    // entirely, not just our stereo shift -- which is the cheapest way back to plain mono.
    bool vrcamOn = CyberpunkVR_VrcamEnabled != 0;
    if (UI::Checkbox("VRCAM component  (RTTI Toggle: force ON/OFF)", &vrcamOn))
        CyberpunkVR_SetVrcamEnabled(vrcamOn ? 1u : 0u);
    UI::TextDisabled("component %s   camera %s",
                        CyberpunkVR_VrcamComponentName(), CyberpunkVR_VrcamCameraName());
    if(cvr::RuntimeDiagnosticsEnabled())UI::TextDisabled("view nodes: main %llu   other %llu   vrcam %llu",
                        static_cast<unsigned long long>(CyberpunkVR_DebugViewKeyMainNodes),
                        static_cast<unsigned long long>(CyberpunkVR_DebugViewKeyOtherNodes),
                        static_cast<unsigned long long>(CyberpunkVR_DebugVrcamNodeHits));

    // Separate second swapchain + window mirroring the VRCAM eye (for OBS / desktop preview).
    // Costs a per-frame copy, so it is off unless asked for.
    bool mirrorOn = CyberpunkVR_MirrorOutput != 0;
    if (UI::Checkbox("VRCAM Mirror  (separate window, for capture)", &mirrorOn))
        CyberpunkVR_MirrorOutput = mirrorOn ? 1u : 0u;

    // Weapon ADS is not a toggle: the vrcam eye always follows MAIN's vertical FOV, narrowed by
    // the aim zoom. Read-only here because the numbers are the quickest way to tell a wrong FOV
    // from a stale one.
    UI::TextDisabled("vrcam fov %.2f  (asset %.2f)   main yy %.5f  fov %.2f   ADS x%.3f",
                        CyberpunkVR_DebugVrcamWantFov, CyberpunkVR_DebugVrcamBaseFov,
                        CyberpunkVR_DebugMainProjYY, CyberpunkVR_DebugMainCamFov,
                        CyberpunkVR_MainAdsZoomFactor);

    if (UI::CollapsingHeader("Diagnostics")) {
        UI::Checkbox("Compact ADS camera telemetry (in-headset)", &g_showCompactAdsTelemetry);
        if (UI::IsItemHovered()) {
            UI::SetTooltip("A small panel that stays up after F10 is closed, reporting what the "
                              "ENGINE did to the camera when the sights came up:\n"
                              "delta and peak in centimetres, right/forward/up in the heading's own "
                              "basis, against a baseline captured while hip firing.\n"
                              "Stereo submission is unaffected.");
        }
        if (g_showCompactAdsTelemetry) {
            UI::Indent();
            widgets::SliderFloat("Telemetry X", &g_compactAdsTelemetryX, 0.10f, 0.90f, "%.2f");
            widgets::SliderFloat("Telemetry Y", &g_compactAdsTelemetryY, 0.10f, 0.90f, "%.2f");
            UI::TextDisabled("Normalised position in the eye image");
            UI::Unindent();
        }
        UI::Separator();

        bool slog = CyberpunkVR_StereoLog != 0;
        if (UI::Checkbox("Stereo logging -> cyberpunkvrport.log", &slog))
            CyberpunkVR_StereoLog = slog ? 1 : 0;

        bool stable = CyberpunkVR_StableCopy != 0;
        if (UI::Checkbox("Committed snapshot of the VRCAM final", &stable))
            CyberpunkVR_StableCopy = stable ? 1 : 0;
        if (UI::IsItemHovered())
            UI::SetTooltip("Off = read the engine's transient directly. That target is a\n"
                              "frame-graph allocation which a later pass aliases, which is what\n"
                              "made the image alternate bright/dark. Leave on.");
        bool fromTonemap = CyberpunkVR_StableFromTonemap != 0;
        if (UI::Checkbox("Snapshot at the tonemap node instead of RenderFinal2D", &fromTonemap))
            CyberpunkVR_StableFromTonemap = fromTonemap ? 1 : 0;

        bool prof = CyberpunkVR_ProfEnable != 0;
        if (UI::Checkbox("Node CPU profiler  (per-node self+incl ms)", &prof))
            CyberpunkVR_ProfEnable = prof ? 1 : 0;
        UI::TextDisabled("frame %.2f ms   dispatch: main %.2f (%u)  vrcam %.2f (%u)",
                            CyberpunkVR_ProfFrameMs,
                            CyberpunkVR_ProfDispMainMs, CyberpunkVR_ProfDispMainNodes,
                            CyberpunkVR_ProfDispVrcamMs, CyberpunkVR_ProfDispVrcamNodes);
        if (UI::Button("Dump node audit -> log  (resets window)"))
            CyberpunkVR_ProfDumpNodes();
        if (prof) {
            // Top-15 by SELF time. Self is the only rankable column: inclusive double-counts,
            // because SceneDrv contains every scene pass it dispatches.
            static uint32_t rva[15];
            static double msv[15], msm[15];
            static uint32_t cv[15], cm[15];
            const int n = CyberpunkVR_ProfSnapshotNodes(rva, msv, msm, cv, cm, 15);
            UI::TextDisabled("top %d by self ms/frame   (main | vrcam)", n);
            for (int i = 0; i < n; ++i) {
                const char* nm = CyberpunkVR_ProfNodeName(rva[i]);
                UI::Text("%-30s %6.3f | %6.3f", (nm && nm[0]) ? nm : "?", msm[i], msv[i]);
            }
        }
    }
}

static bool DrawDrivingControls(LiveControlsUiState& state) {
    bool changed=false;
            UI::Separator();
            UI::TextUnformatted("Driving -- hands on the wheel");
            changed |= CheckboxInt("Grab the wheel with the grips", &state.xrWheelGrab);
            if (UI::IsItemHovered()) {
                UI::SetTooltip(
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
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("How close your hand has to be to the animated hand before the\n"
                                      "grip counts as grabbing the wheel. Bigger = easier to catch a\n"
                                      "wheel you cannot see; too big and every grip in a car grabs.");
                }
                float m = std::isfinite(state.xrWheelSteerMaxDeg) && state.xrWheelSteerMaxDeg >= 30.0f &&
                          state.xrWheelSteerMaxDeg <= 120.0f ? state.xrWheelSteerMaxDeg : 90.0f;
                if (widgets::SliderFloat("Bike full lock at (deg)", &m, 30.0f, 120.0f, "%.0f")) {
                    state.xrWheelSteerMaxDeg = m;
                    changed = true;
                }
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("Handlebar sensitivity on bikes: hand rotation from the grab position to full lock.\n"
                                      "A car's wheel turns with your hands one to one: full lock is where the cabin\n"
                                      "wheel's is (130 degrees, twice the game's own travel); road-wheel lock is unchanged.\n"
                                      "One hand uses a hub calibrated where you grab; two use their connecting line.\n"
                                      "Adding or releasing a hand keeps the current turn.");
                }
                float d = state.xrWheelSteerDeadDeg >= 0.0f ? state.xrWheelSteerDeadDeg : 0.0f;
                if (widgets::SliderFloat("Steering deadzone (deg)", &d, 0.0f, 20.0f, "%.1f")) {
                    state.xrWheelSteerDeadDeg = d;
                    changed = true;
                }
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("Tilt around centre that steers nothing at all.\n"
                                      "Raise it if the car drifts while you hold the wheel straight;\n"
                                      "every degree here is a degree of dead wheel off centre.\n"
                                      "The full range still ends at 'Full lock at', so widening the\n"
                                      "deadzone does not make the steering jump.");
                }

                changed |= CheckboxInt("Steering prediction", &state.xrWheelPrediction);
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("Gently anticipate a continuing turn from recent hand movement.\n"
                                      "Limited to 8 ms and 0.5 degrees; neutral stays unchanged.\n"
                                      "Prediction stops on a pause, reversal or grip change.\n"
                                      "Off by default. It cannot anticipate the start of a turn.");
                }
                if (state.xrWheelPrediction) {
                    changed |= widgets::SliderFloat("Prediction horizon (ms)", &state.xrWheelPredictionMs, 0.0f, 8.0f, "%.1f");
                }
                changed |= CheckboxInt("Horn -- hand on the wheel hub", &state.xrWheelHorn);
                if (UI::IsItemHovered()) {
                    UI::SetTooltip(
                        "While DRIVING, put a hand on the MIDDLE of the wheel -- where you would\n"
                        "slap a real horn -- and the car honks for as long as it stays there.\n"
                        "No grip needed; a hand that is GRABBING the wheel never honks.");
                }
                float hr = state.xrWheelHornRadius > 0.0f ? state.xrWheelHornRadius : 0.12f;
                if (widgets::SliderFloat("Horn hub radius (m)", &hr, 0.04f, 0.30f, "%.2f")) {
                    state.xrWheelHornRadius = hr;
                    changed = true;
                }
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("How near the wheel centre the hand counts as on the hub.\n"
                                      "Bigger = easier to find the horn without seeing it; too big\n"
                                      "and it reaches the rim, so every grab honks.");
                }

                UI::Spacing();
                changed |= CheckboxInt("Trigger fires the gun while driving", &state.xrVehicleGunTrigger);
                if (UI::IsItemHovered()) {
                    UI::SetTooltip(
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
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("How much of the throttle's full travel the left stick adds or\n"
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
                    UI::Text("driving %d   at wheel  L %d R %d   hold  L %.2f R %.2f",
                                g_isDriving.load(std::memory_order_relaxed) ? 1 : 0,
                                (mask & vrshared::kWheelArmedLeftBit) ? 1 : 0,
                                (mask & vrshared::kWheelArmedRightBit) ? 1 : 0,
                                cvr::anim::g_wheelBlendLeft.load(std::memory_order_relaxed),
                                cvr::anim::g_wheelBlendRight.load(std::memory_order_relaxed));
                    UI::Text("steer  %+.1f deg  ->  stick %+.2f     horn  L %d R %d",
                                cvr::anim::g_wheelSteerDeg.load(std::memory_order_relaxed),
                                cvr::anim::g_wheelSteer.load(std::memory_order_relaxed),
                                (horn & vrshared::kWheelArmedLeftBit) ? 1 : 0,
                                (horn & vrshared::kWheelArmedRightBit) ? 1 : 0);
                }
            }

        UI::Separator();
        UI::TextUnformatted("SEATED CAMERA OFFSET");
        if (UI::IsItemHovered()) {
            UI::SetTooltip(
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
        UI::BeginDisabled(passengerCombat);
        changed |= widgets::SliderFloat("Car Head X right", &state.xrVehHeadOffsetX, -0.50f, 0.50f, "%.3f m");
        changed |= widgets::SliderFloat("Car Head Y forward", &state.xrVehHeadOffsetY, -0.50f, 0.50f, "%.3f m");
        changed |= widgets::SliderFloat("Car Head Z up", &state.xrVehHeadOffsetZ, -0.50f, 0.50f, "%.3f m");
        UI::EndDisabled();
        {   // Live, so a slider that is doing nothing says so instead of being blamed.
            UI::TextDisabled(passengerCombat ? "   (window combat: use GENERAL > Head offsets)"
                               : g_isInVehicle ? "   (in a vehicle: these are live)"
                                              : "   (on foot: these are ignored)");
        }
    return changed;
}

bool DrawFpsOverlayControls(LiveControlsUiState& state) {
    bool changed=false;auto& fg=state.framegen;
            changed|=UI::Checkbox("FPS overlay",&fg.overlay);
            changed|=widgets::Combo("Overlay layout",&fg.overlayLayout,"Detailed\0Compact\0Minimal\0");
            changed|=widgets::Combo("Overlay position",&fg.overlayCorner,"Top left\0Top right\0Bottom left\0Bottom right\0Top center\0Bottom center\0Left center\0Right center\0Center\0");
            changed|=widgets::SliderFloat("Overlay size",&fg.overlayScale,.5f,2,"%.2fx");
            changed|=widgets::SliderFloat("Overlay distance",&fg.overlayDistance,.5f,5,"%.2f m");
            changed|=widgets::SliderFloat("Overlay horizontal offset",&fg.overlayX,-60,60,"%.1f deg");
            changed|=widgets::SliderFloat("Overlay vertical offset",&fg.overlayY,-45,45,"%.1f deg");
            changed|=CheckboxInt("Overlay free look",&fg.overlayFollow);
            UI::BeginDisabled(!fg.overlayFollow);
            changed|=widgets::SliderFloat("Overlay free-look cone",&fg.overlayCone,5,90,"%.1f deg");
            if(UI::IsItemHovered())UI::SetTooltip("Hold the panel heading inside this yaw range; catch up beyond it.");
            UI::EndDisabled();
            int history=fg.historySeconds==10?0:(fg.historySeconds==60?2:(fg.historySeconds==120?3:1));
            if(widgets::Combo("History window",&history,"10 seconds\0 30 seconds\0 60 seconds\0 120 seconds\0")) {const int seconds[]={10,30,60,120};fg.historySeconds=seconds[history];changed=true;}
            if(fg.overlay) {
            if(UI::Button("Reset statistics"))cvr::framegen::ResetStatistics();
            const auto stats=cvr::framegen::GetStatistics();
            UI::Text("Real %.1f FPS   Framegen %.1f FPS   Output %.1f FPS",stats.realFps,stats.generatedFps,stats.outputFps);
            UI::Text("Average %.1f FPS   Min %.1f   Max %.1f",stats.averageFps,stats.minimumFps,stats.maximumFps);
            if(stats.low1Fps>0)UI::Text("1%% low %.1f FPS",stats.low1Fps);else UI::TextDisabled("1%% low: collecting 100 real frame intervals");
            if(stats.low01Fps>0)UI::Text("0.1%% low %.1f FPS",stats.low01Fps);else UI::TextDisabled("0.1%% low: collecting 1000 real frame intervals");
            const float graphMax=float(std::max(33.34,stats.budgetMs*2));
            UI::PlotLines("CPU history (ms)",stats.cpuGraph.data(),int(stats.cpuGraph.size()),0,nullptr,0,graphMax,ImVec2(0,55));
            UI::PlotLines("GPU history (ms)",stats.gpuGraph.data(),int(stats.gpuGraph.size()),0,nullptr,0,graphMax,ImVec2(0,55));
            UI::Text("CPU frame %.2f ms   Peak %.2f ms",stats.cpuMs,stats.cpuPeakMs);
            if(stats.gpuValid)UI::Text("GPU frame %.2f ms   Peak %.2f ms",stats.gpuMs,stats.gpuPeakMs);
            else UI::TextDisabled("GPU frame timing: waiting for completed queries");
            UI::Text("Generation %.2f ms   Tracked VRAM %.1f MiB",stats.generationMs,double(stats.vramBytes)/(1024*1024));
            const auto& hw=stats.hardware;constexpr double gib=1024.0*1024*1024;
            UI::Text("CPU: %s",hw.cpuName[0]?hw.cpuName:"--");
            UI::Text("GPU: %s",hw.gpuName[0]?hw.gpuName:"--");
            if(hw.cpuUsage>=0)UI::Text("CPU load %.1f%%   Busiest logical core %.1f%%",hw.cpuUsage,hw.coreMaximum);
            if(hw.gpuUsage>=0)UI::Text("GPU load %.1f%%   Temperature %.0f C",hw.gpuUsage,hw.gpuTemperature);
            if(hw.gpuMemoryValid)UI::Text("GPU memory %.2f / %.2f GiB",hw.gpuMemoryUsed/gib,hw.gpuMemoryTotal/gib);
            if(hw.gpuProcessMemoryValid)UI::Text("Game GPU memory %.2f GiB   Budget %.2f GiB",hw.gpuProcessUsed/gib,hw.gpuProcessBudget/gib);
            UI::Text("RAM %.2f / %.2f GiB   Game working set %.2f GiB",hw.ramUsed/gib,hw.ramTotal/gib,hw.appWorkingSet/gib);
            UI::Text("Depth + motion: %s   Skipped %llu",stats.inputsReady?"both eyes observed":"waiting",stats.skipped);
            UI::TextDisabled("CPU: captured frame cadence. GPU: graphics-queue frame interval. Peaks: last 2 seconds.");
            UI::TextDisabled("Output FPS excludes repeated frames; it is not a headset scanout measurement.");
            UI::TextDisabled("Lows use the mean of the slowest 1%% / 0.1%% of real-frame intervals. Hardware polling: 1 Hz.");
            } else UI::TextDisabled("FPS overlay and statistics are off.");
    return changed;
}

bool DrawLiveControls(LiveControlsUiState& state,int section,bool* foveatedEditing) {
    bool changed = false;
    if(foveatedEditing)*foveatedEditing=false;

    if (UI::Button("Recenter HMD (F7)")) {
        RequestLiveControlsRecenter();
    }
    UI::SameLine();
    if (UI::Button("Save")) {
        SetLiveControlsUiState(&state, 1);
    }

    if (section>=0 || UI::BeginTabBar("CyberpunkVRPortTabs")) {
        if (section>=0 ? section==0 : UI::BeginTabItem("GENERAL")) {
            const bool casBlocked=cvr::sharpen::gameUpscaler.BlocksCas();
            float casAmount=casBlocked?0.0f:state.xrSharpmix;
            UI::BeginDisabled(casBlocked);
            if(widgets::SliderFloat("CAS Sharpening", &casAmount, 0.0f, 1.0f, "%.3f") && !casBlocked) {
                state.xrSharpmix=casAmount;changed=true;
            }
            if (UI::IsItemHovered(ImGuiHoveredFlags_AllowWhenDisabled))
                UI::SetTooltip("Contrast-adaptive sharpening for both eyes. 0 = off, 1 = full strength. Disabled automatically with game DLAA.");
            UI::EndDisabled();
            if (UI::CollapsingHeader("FIXED FOVEATED RENDERING", ImGuiTreeNodeFlags_DefaultOpen)) {
                auto& foveated = state.foveated;
                changed |= widgets::Combo("Peripheral quality", &foveated.mode, "Off\0Quality\0Balanced\0Performance\0");
                if (UI::IsItemHovered()) UI::SetTooltip("The center follows game DLSS/DLAA. Peripheral DLAA uses 100%% / 75%% / 50%% of the game's render resolution.");
                UI::BeginDisabled(foveated.mode == 0);
                changed |= widgets::SliderFloat("Center width", &foveated.widthPercent, 20.0f, 100.0f, "%.1f %%");
                bool editing=UI::IsItemActive();
                if (UI::IsItemHovered()) UI::SetTooltip("Full-quality region width as a percentage of each eye image.");
                changed |= widgets::SliderFloat("Center height", &foveated.heightPercent, 20.0f, 100.0f, "%.1f %%");
                editing|=UI::IsItemActive();
                if (UI::IsItemHovered()) UI::SetTooltip("Full-quality region height as a percentage of each eye image.");
                foveated=cvr::foveated::Normalize(foveated);
                const float maxX=(100.0f-foveated.widthPercent)*.5f, maxY=(100.0f-foveated.heightPercent)*.5f;
                UI::BeginDisabled(maxX==0);
                changed |= widgets::SliderFloat("Center offset X", &foveated.offsetXPercent, -maxX, maxX, "%.1f %%");
                editing|=UI::IsItemActive();
                if (UI::IsItemHovered()) UI::SetTooltip("Percentage of eye-image width. Negative = left, positive = right.");
                UI::EndDisabled();
                UI::BeginDisabled(maxY==0);
                changed |= widgets::SliderFloat("Center offset Y", &foveated.offsetYPercent, -maxY, maxY, "%.1f %%");
                editing|=UI::IsItemActive();
                if (UI::IsItemHovered()) UI::SetTooltip("Percentage of eye-image height. Negative = up, positive = down toward the hands.");
                UI::EndDisabled();
                if(foveatedEditing)*foveatedEditing=editing;
                changed |= UI::Checkbox("Show region boundary", &foveated.showBoundary);
                UI::EndDisabled();
                const auto status = cvr::foveated::GetStatus();
                UI::TextDisabled("%s", status.text);
                if (foveated.mode && status.evaluations) {
                    UI::Text("DLSS GPU: native %.2f ms / foveated %.2f ms",status.nativeMs,status.totalMs);
                }
            }
            changed|=CheckboxInt("Shared sun shadows",&state.monoCascades);
            if(UI::IsItemHovered())UI::SetTooltip("Render sun cascades once for both eyes. Shadow quality and distance remain controlled by the game.");
            changed|=SliderIntClamped("Scope lens stabilization",&state.scopeStabilize,0,100);
            if(UI::IsItemHovered())UI::SetTooltip("Magnifying scope lenses while the view moves: share of the stabilised image (percent). Higher: no blinking of lights and grates, softer. 0 = off. Standing still the lens is untouched.");
            if (UI::CollapsingHeader("VARIABLE RATE SHADING", ImGuiTreeNodeFlags_DefaultOpen)) {
                UI::PushID("fixedVrs");
                auto& v=state.vrs;
                changed|=widgets::Combo("Peripheral shading",&v.mode,"Off\0 2 x 2\0 4 x 4\0");
                if(UI::IsItemHovered())UI::SetTooltip("Opaque scene materials in both eyes. The center stays 1 x 1. Unsupported 4 x 4 falls back to 2 x 2.");
                UI::BeginDisabled(v.mode==0);
                changed|=widgets::SliderFloat("Center width",&v.width,20,100,"%.1f %%");
                changed|=widgets::SliderFloat("Center height",&v.height,20,100,"%.1f %%");
                v=cvr::vrs::Normalize(v);
                const float x=(100-v.width)*.5f,y=(100-v.height)*.5f;
                UI::BeginDisabled(x==0);changed|=widgets::SliderFloat("Center offset X",&v.offsetX,-x,x,"%.1f %%");UI::EndDisabled();
                UI::BeginDisabled(y==0);changed|=widgets::SliderFloat("Center offset Y",&v.offsetY,-y,y,"%.1f %%");UI::EndDisabled();
                UI::EndDisabled();
                const auto s=cvr::vrs::GetStatus();
                if(v.mode) {
                    if(s.failed)UI::TextDisabled("Unavailable: GPU resource error");
                    else if(s.checked && !s.supported)UI::TextDisabled("VRS Tier 2 unavailable");
                    else if(!s.readyMaps)UI::TextDisabled("Preparing");
                    else UI::TextDisabled("%s | %u px tiles",v.mode==2 && s.additionalRates?"4 x 4":"2 x 2",s.tileSize);
                }
                UI::PopID();
            }
            if (UI::CollapsingHeader("HIDDEN AREA MASK", ImGuiTreeNodeFlags_DefaultOpen)) {
                UI::PushID("hiddenArea");
                auto& h=state.hiddenArea;
                changed|=CheckboxInt("Mask lens edges",&h.enabled);
                if(UI::IsItemHovered())UI::SetTooltip("Skips shading the parts of each eye image the lens never shows. The visible area is unchanged.");
                UI::BeginDisabled(!h.enabled);
                changed|=widgets::SliderFloat("Visible radius",&h.radius,cvr::hidden_area::MinRadius,cvr::hidden_area::MaxRadius,"%.0f %%");
                if(UI::IsItemHovered())UI::SetTooltip("Size of the shaded area as a percentage of the eye's field of view: 100%% touches its edges, 141%% masks nothing inside it.\nLower it until a dark border appears at the edge of the lens, then raise it until the border is gone.");
                UI::EndDisabled();
                h=cvr::hidden_area::Normalize(h);
                if(h.enabled) {
                    const auto s=cvr::hidden_area::GetStatus();
                    if(!s.masked[0] && !s.masked[1])UI::TextDisabled("Waiting for the scene depth");
                    else UI::TextDisabled("Masked %.1f %% / %.1f %% of pixels (MAIN / VRCAM)",s.hiddenPercent[0],s.hiddenPercent[1]);
                }
                UI::PopID();
            }
            if (UI::CollapsingHeader("GAME MENUS", ImGuiTreeNodeFlags_DefaultOpen)) {
                changed |= CheckboxInt("VR menu quad", &state.xrMenuRect);
                changed |= widgets::SliderFloat("VR menu FOV", &state.xrMenuFov, 30.0f, 120.0f, "%.1f deg");
            }

            if (UI::CollapsingHeader("Tracking / Camera")) {
        UI::TextUnformatted("Locomotion direction is set in the Controls tab.");
        changed |= CheckboxInt("Disable Mouse Y (Pitch)", &state.xrDisableMouseY);
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Suppress mouse/right-stick pitch so only the HMD controls\n"
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

            if (UI::CollapsingHeader("Debug Gizmos")) {
                UI::TextUnformatted("Raw hand overlay / debug gizmos:");
                UI::Checkbox("Enable hand overlay", &g_drawHandLocator);
                UI::Checkbox("Draw 3D hand proxy", &g_drawHandProxy3D);
                UI::Checkbox("Draw debug wire/axes", &g_drawHandDebugAxes);
                widgets::SliderFloat("Locator scale", &g_handLocatorScale, 0.50f, 2.00f, "%.2f");
            }

            if (UI::CollapsingHeader("Diagnostics")) {
        { int vl = g_verboseLog; if (CheckboxInt("Verbose log (spammy diag)", &vl)) g_verboseLog = vl; }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Off by default for a clean cyberpunkvrport.log. Enable only\n"
                              "when capturing ClipCursor / depth / hook diagnostics.");
        }
            }
            if(section<0)UI::EndTabItem();
        }

        if (section>=0 ? section==1 : UI::BeginTabItem("FRAMEGEN")) {
            auto& fg=state.framegen;
            changed|=UI::Checkbox("Flight recorder",&fg.flightRecorder);
            if(UI::IsItemHovered())UI::SetTooltip("Persist pacing, missed inputs, GPU generation and XR submission counters, plus SteamVR compositor estimates. Works with FPS overlay off. No image capture. Toggle off to stop. Files capped at 16 MiB.");
            {char status[160]{};cvr::framegen::flight::Status(status,sizeof(status));UI::TextWrapped("Recorder: %s",status);}
            if(fg.flightRecorder){
                UI::TextWrapped("Logs: cyberpunkvrport/log/framegen-flight-*.jsonl");
                if(UI::Button("Mark performance problem"))cvr::framegen::flight::Count(cvr::framegen::flight::Event::Marker);
                UI::TextWrapped("Submitted frames are not displayed FPS. SteamVR delivery is an estimate; unavailable on VDXR.");
            }
            {
                char captureStatus[192]{};bool capturing=false;
                cvr::framegen::FrameCaptureStatus(captureStatus,sizeof(captureStatus),&capturing);
                UI::BeginDisabled(capturing || !fg.enabled);
                if(UI::Button("Capture ghosting images (10s delay)"))cvr::framegen::ScheduleFrameCapture(10);
                UI::EndDisabled();
                UI::TextWrapped("Image capture: %s",captureStatus);
                if(capturing && UI::Button("Cancel image capture"))cvr::framegen::CancelFrameCapture();
                if(UI::IsItemHovered())UI::SetTooltip("A capture already saving will finish safely.");
                UI::TextWrapped("Saves 3 stereo images with depth/motion for analysis. Close F10 and ride; capture may briefly hitch. It does not fix ghosting.");
            }
            changed|=UI::Checkbox("Enable frame generation",&fg.enabled);
            if(UI::IsItemHovered())UI::SetTooltip("While frame generation is enabled, hand smoothing runs at its rate times xr_hand_lerp_framegen_scale (1.25; 0 turns it off).");
            int backend=int(fg.backend);
            if(widgets::Combo("Frame generator",&backend,"FidelityFX\0NVIDIA OFA + FidelityFX\0NVIDIA DLSS Frame Generation\0NVIDIA OFA + V557 (experimental)\0")) {fg.backend=cvr::framegen::Backend(backend);changed=true;}
            using cvr::framegen::Backend;
            const Backend active=cvr::framegen::ActiveBackend();
            if(fg.backend==Backend::DlssG && fg.enabled && active!=Backend::DlssG)
                UI::TextWrapped("DLSS Frame Generation is not available here (%s); FidelityFX is used.",cvr::framegen::dlssg::Reason());
            else if(cvr::framegen::UsesNvidiaOFA(fg.backend) && fg.enabled && active==Backend::FidelityFX)
                UI::TextWrapped("NVIDIA optical flow is not available on this GPU; FidelityFX optical flow is used.");
            else if(cvr::framegen::UsesNvidiaOFA(fg.backend) && fg.flowScale)
                changed|=widgets::Combo("NVIDIA quality",&fg.quality,"Fast (performance)\0Medium (balanced)\0Slow (best)\0");
            if(fg.backend==Backend::FidelityFX || cvr::framegen::UsesNvidiaOFA(fg.backend)) {
                int scale=fg.flowScale==100?3:(fg.flowScale==75?2:(fg.flowScale==0?0:1));
                if(widgets::Combo("Optical flow",&scale,"Off (game vectors only)\0 50%\0 75%\0 100%\0")) {fg.flowScale=scale==3?100:(scale==2?75:(scale==0?0:50));changed=true;}
                if(UI::IsItemHovered())UI::SetTooltip("The game's motion vectors already describe almost all motion; optical flow adds GPU time\nand memory for what they miss (some particles and effects).");
            }
            if(fg.backend==Backend::NvidiaV557){
                if(!fg.flowScale){fg.flowScale=50;changed=true;}
                UI::TextWrapped("Experimental V557 optical reconstruction: current-camera pre-warp and flow confidence. Uses more color history; compare ghosting and performance. Existing pacing is retained. Optical flow is required.");
            }
            if(fg.backend==Backend::DlssG)
                UI::TextWrapped("NVIDIA's frame generation model on the game's motion vectors and depth. Highest quality, RTX 40/50.");
            changed|=UI::Checkbox("SteamVR framegen optimizations (restart game)",&fg.steamVrOptimizations);
            if(UI::IsItemHovered())UI::SetTooltip("Our SteamVR timing and queue package. Applies to Steam Link too; VDXR is unaffected.\nIncludes physical refresh correction, private queues and pacing improvements.\nSave your choice here, then restart the game. Adaptive real-frame pacing remains a separate control.");
            if(OpenXRManager::Get().IsRuntimeSteamVR()) {
                const bool packageActive=cvr::framegen::refresh::Targeted();
                UI::TextWrapped("SteamVR package active this session: %s",packageActive?"ON":"OFF");
                if(fg.steamVrOptimizations!=packageActive)
                    UI::TextWrapped("Restart required to turn the SteamVR package %s.",fg.steamVrOptimizations?"ON":"OFF");
            } else UI::TextWrapped("SteamVR package inactive on this runtime. Preference saved for SteamVR / Steam Link.");
            changed|=UI::Checkbox("Adaptive real-frame pacing",&fg.autoPace);
            if(UI::IsItemHovered())UI::SetTooltip("Up to half headset refresh. Leaves more GPU time for generated frames when adjacent real frames are being missed.");
            UI::TextWrapped(fg.backend==Backend::NvidiaV557?"V557 uses optical flow in both eyes; no engine-vector/depth interpolation. Adds one intermediate frame.":"Uses game motion vectors and depth in both eyes. Adds one intermediate frame between real frames.");
            UI::Separator();
            const auto runtimeStatus=cvr::framegen::GetRuntimeStatus();
            UI::TextWrapped("%s",runtimeStatus.text);
            if(section<0)UI::EndTabItem();
        }

        if (section>=0 ? section==2 : UI::BeginTabItem("HUD")) {
            changed |= CheckboxInt("Enable HUD panel", &state.xrHudPanel);
            const char* followModes[] = {"Head / free-look cone", "Body rotation"};
            changed |= widgets::Combo("Follow", &state.xrHudFollowMode, followModes, 2);
            UI::BeginDisabled(state.xrHudFollowMode != 0);
            changed |= widgets::SliderFloat("Free-look cone", &state.xrHudFollowDeg, 5.0f, 90.0f, "%.1f deg");
            if(UI::IsItemHovered())UI::SetTooltip("Catch up beyond the cone, or after 3 seconds at rest when the yaw offset is over 10 degrees inside it.");
            UI::EndDisabled();
            changed |= widgets::SliderFloat("HUD FOV", &state.xrHudFov, 30.0f, 120.0f, "%.1f deg");
            changed |= CheckboxInt("Stereo depth", &state.xrHudStereoDepth);
            if (UI::IsItemHovered()) UI::SetTooltip("Off: the HUD has the same direction in both eyes.\nOn: place it at the selected physical depth.");
            UI::BeginDisabled(!state.xrHudStereoDepth);
            changed |= widgets::SliderFloat("Depth", &state.xrHudDistance, 0.5f, 5.0f, "%.2f m");
            // In front of the world, never inside it (Runtimes/HudClearance.hpp).
            changed |= CheckboxInt("Keep HUD in front of the world", &state.xrHudClearance);
            if (UI::IsItemHovered()) UI::SetTooltip("The HUD comes closer when a wall or a person is nearer than Depth.\nOff: always at Depth.");
            UI::BeginDisabled(!state.xrHudClearance);
            changed |= widgets::SliderFloat("Grow when brought closer", &state.xrHudClearanceGrow, 0.0f, 1.0f, "%.2f");
            if (UI::IsItemHovered()) UI::SetTooltip("0: same size on screen. 1: same size in the world.");
            changed |= CheckboxInt("Hands push the HUD out of view", &state.xrHudClearanceHands);
            if (UI::IsItemHovered()) UI::SetTooltip("While a hand is in front of the HUD, the HUD grows until its elements leave the view,\nso your hands are not covered by it.");
            UI::BeginDisabled(!state.xrHudClearanceHands);
            changed |= widgets::SliderFloat("Out-of-view amount", &state.xrHudClearanceHandsScale, 1.0f, 3.0f, "%.2fx");
            if (UI::IsItemHovered()) UI::SetTooltip("How far the HUD moves out while a hand is in front of it.\n1: not at all. 2: to the edge of the view, a small head turn away. 3: fully out of view.");
            UI::EndDisabled();
            UI::EndDisabled();
            UI::EndDisabled();
            changed |= widgets::SliderFloat("Brightness", &state.xrHudBrightness, 0.25f, 3.0f, "%.2fx");
            changed |= widgets::SliderFloat("Shadow / outline", &state.xrHudShadow, 0.0f, 2.0f, "%.2fx");
            changed |= widgets::SliderFloat("Glow", &state.xrHudGlow, 0.0f, 2.0f, "%.2fx");
            UI::Separator();
            if(UI::CollapsingHeader("INTERACTIONS / DIALOGS",ImGuiTreeNodeFlags_DefaultOpen)){
                changed |= CheckboxInt("World-space interactions / dialogs",&state.xrInteractionPanel);
                changed |= widgets::SliderFloat("Interaction free-look cone",&state.xrInteractionFollowDeg,0.0f,90.0f,"%.1f deg");
                changed |= widgets::SliderFloat("Loot free-look cone",&state.xrLootFollowDeg,0.0f,90.0f,"%.1f deg");
                if(UI::IsItemHovered())UI::SetTooltip("Used while the loot panel and item description are visible. Default: 10 degrees.");
                changed |= widgets::SliderFloat("Interaction distance",&state.xrInteractionDistance,.5f,5.0f,"%.2f m");
                changed |= CheckboxInt("Prompt at the object's depth",&state.xrInteractionAnchor);
                if(UI::IsItemHovered())UI::SetTooltip("The prompt sits 10 cm in front of the object it names.\nInteraction distance is then used only when nothing is targeted.");
                changed |= widgets::SliderFloat("Interaction FOV",&state.xrInteractionFov,30.0f,120.0f,"%.1f deg");
                UI::TextWrapped("Interaction prompts and dialog choices share this panel. It follows only beyond the cone; no delayed catch-up at rest.");
            }
            UI::Separator();
            static int element=10;
            const char* elementLabels[cvr::hud::ElementCount];
            for(size_t i=0;i<cvr::hud::ElementCount;++i)elementLabels[i]=cvr::hud::Elements[i].label;
            widgets::Combo("Element",&element,elementLabels,int(cvr::hud::ElementCount));
            const auto status=cvr::hud::GetLayoutStatus();
            if(!status[element].available) UI::TextDisabled("Not currently visible in gameplay; settings are still saved.");
            auto& transform=state.hudElements[element];
            changed |= CheckboxInt("Show element", &transform.visible);
            changed |= widgets::SliderFloat("Horizontal offset", &transform.x, -100.0f,100.0f,"%.1f %%");
            changed |= widgets::SliderFloat("Vertical offset", &transform.y, -100.0f,100.0f,"%.1f %%");
            changed |= widgets::SliderFloat("Size", &transform.scale,0.1f,3.0f,"%.2fx");
            changed |= widgets::SliderFloat("Opacity", &transform.opacity,0.0f,1.0f,"%.2f");
            if(UI::Button("Reset element")) {transform={};changed=true;}
            UI::SameLine();
            if(UI::Button("Reset all elements")) {state.hudElements={};changed=true;}
            if(section<0)UI::EndTabItem();
        }

        if (section>=0 ? section==3 : UI::BeginTabItem("CONTROLS")) {
            static int page=0;
            static const char* pages[]={"GENERAL","DRIVING","BINDINGS"};
            widgets::Tabs("controls-pages",page,pages,3);
            if(page==1)changed|=DrawDrivingControls(state);
            else if(page==2)widgets::DrawBindings();
            else {
            UI::TextWrapped("Movement, weapon aiming and physical interaction shortcuts.");

            changed |= CheckboxInt("Left-handed controls", &state.xrLeftHanded);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("The two controllers trade roles: the left trigger fires and the right one aims,\n"
                                  "the right stick walks and the left one turns, dashes and crouches.\n"
                                  "X jumps, Y drops the magazine, A reloads and interacts, B switches weapons,\n"
                                  "and the right stick click is the D-pad. Button prompts show these buttons.");
            }
            // How far the controllers trade (Hooks/Handedness.hpp): the triggers and grips always go with the weapon hand.
            if (state.xrLeftHanded) {
                changed |= widgets::Combo("Left-handed bindings", &state.xrLeftHandedBindings,
                                          "Same as right-handed\0Left stick walks, buttons mirrored\0"
                                          "Left stick turns, buttons mirrored\0");
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("Same as right-handed: the buttons and sticks as right-handed; only the triggers\n"
                                      "and grips go with the weapon hand.\n"
                                      "Left stick walks: the buttons mirrored (X jumps, A reloads...), the sticks as they were.\n"
                                      "Left stick turns: the buttons and the sticks mirrored.");
                }
            }
            UI::Separator();
            // Finger tracking from OpenXR hand tracking (Runtimes/FingerGrip.hpp, Anim/HandGestures.hpp).
            if (UI::CollapsingHeader("FINGER TRACKING", ImGuiTreeNodeFlags_DefaultOpen)) {
                changed |= CheckboxInt("Enable finger tracking", &state.xrFingerBones);
                if (UI::IsItemHovered()) UI::SetTooltip("Each of V's fingers follows your own, from hand tracking (finger-tracking gloves, Index controllers).\nTurning it on applies at the next game start. Off: finger poses from the buttons.");
                UI::BeginDisabled(!state.xrFingerBones);
                changed |= CheckboxInt("Grip input from finger position instead of the sensor", &state.xrGripFromFingers);
                if (UI::IsItemHovered()) UI::SetTooltip("The grip is a fist or a thumbs-up, from the fingers' position,\nnot the controller's grip sensor: a relaxed hand never grips.");
                changed |= CheckboxInt("Enable finger spread", &state.xrFingerSpreadOn);
                if (UI::IsItemHovered()) UI::SetTooltip("V's fingers also spread sideways as yours do.\nOff: the fingers only bend, from the curl.");
                UI::BeginDisabled(!state.xrFingerSpreadOn);
                changed |= widgets::SliderFloat("Finger spread", &state.xrFingerSpread, 0.0f, 2.0f, "%.2fx");
                if (UI::IsItemHovered()) UI::SetTooltip("V's fingers spread sideways as yours do.\n0: off, the fingers only curl. 1: default.");
                UI::EndDisabled();
                UI::EndDisabled();
            }

            cvr::fbt::game::DrawSettingsSection();   // Fbt/Game/Menu.cpp

            // Weapon aim: bullets/projectiles fly down the WEAPON BARREL (controller-pointed) instead
            // of the camera crosshair. Hooks the projectile launch orientation provider and feeds it
            // the game's own muzzle world transform. Writes shared[58]; the RED4ext plugin applies it.
            {
                static bool s_weaponAim = true;   // default ON — backend's m_weaponAimEnable also defaults to 1
                if (UI::Checkbox("Hand aim  (off = Decoupled VR Head Aim)", &s_weaponAim)) {
                    OpenXRManager::Get().SetWeaponAimEnable(s_weaponAim ? 1 : 0);
                }
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("ON (Hand Aim): the controller points the weapon and VRIK drives the arms.\n"
                                      "OFF (Decoupled VR Head Aim): the WEAPON follows your head instead, the game\n"
                                      "keeps owning its position and its ADS animations, and VRIK stands down for\n"
                                      "the weapon arm. Either way the shot leaves the real muzzle, for guns and\n"
                                      "projectiles alike, and free-look while aiming is preserved.");
                }
                changed |= UI::Checkbox("Weapon Aim laser dot (where the bullet hits)", &state.xrWeaponLaserDot);
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("Red dot projected from the actual weapon muzzle direction through the\n"
                                      "game camera -- marks exactly where the bullet will fly.");
                }
            }
            UI::Separator();

            changed |= CheckboxInt("Controller vibration", &state.xrHaptics);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("Haptics: every round by weapon class (handgun, rifle, shotgun, sniper),\n"
                                  "melee block and parry, bullet block and deflect, reaching a belt element,\n"
                                  "pulling a grenade pin. Two-handed grip: both hands; one hand: one.");
            }
            UI::BeginDisabled(state.xrHaptics == 0);
            changed |= widgets::SliderFloat("Vibration strength", &state.xrHapticScale, 0.0f, 2.0f, "%.2fx");
            UI::EndDisabled();
            UI::Separator();

            changed |= CheckboxInt("Open the VR menu with a grip swipe", &state.xrMenuGesture);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("Hold the right grip and sweep the hand straight down to open or close the VR menu.\n"
                                  "Off: only L3 + R3 (both sticks pressed) opens it.\n"
                                  "A hand holding something (the steering wheel, a ladder, an item) never opens it.");
            }
            changed |= CheckboxInt("VR weapon wheel on a held Y", &state.xrVrWeaponWheel);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("Hold Y: a small wheel opens in front of the body and the game keeps running.\n"
                                  "Point at a weapon, the arm cyberware, fists, a grenade or a consumable, then let go of Y.\n"
                                  "A tap of Y is still the next weapon. Off: the game's own wheel, which slows time.");
            }
            UI::Separator();

            changed |= CheckboxInt("Enable VR -> XInput merge", &state.xrXInputHook);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("OR the VR controller state into XInput gamepad 0 every poll.\n"
                                  "Off = the game only sees a physical pad / nothing.");
            }

            UI::Separator();
            UI::TextUnformatted("Weapon holsters (reach + right grip)");
            changed |= CheckboxInt("Immersive holsters", &state.xrImmersiveHolsters);
            if (UI::IsItemHovered()) {
                UI::SetTooltip(
                    "ON  - every equipped weapon hangs on the body where it belongs:\n"
                    "      pistols on the right hip, blades and clubs on the left hip,\n"
                    "      rifles and shotguns over the shoulders, knives on the chest.\n"
                    "      Reach for one and squeeze the grip to draw it (a blade at the\n"
                    "      hip is pulled out along its handle); grip at its place to put it away.\n"
                    "OFF - no weapons on the body, fixed slot per zone:\n"
                    "      over-shoulder = EquipmentSlot1, right hip = Slot2, left hip = Slot3.");
            }

            UI::Separator();
            UI::TextUnformatted("Physical reload");
            changed |= CheckboxInt("Reload highlight", &state.xrReloadGlow);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("On: the gun shows the free hand what it needs - an empty magazine pulses,\n"
                                  "the place of a removed one stays lit, the bolt or the slide to rack glows.\n"
                                  "Off: no highlight; the reload itself works the same.");
            }


            UI::Separator();
            UI::TextUnformatted("Locomotion direction");
            bool analogMovement=state.xrMovementSpeedMode==1;
            if(UI::Checkbox("Analog movement",&analogMovement)) {state.xrMovementSpeedMode=analogMovement?1:0;changed=true;}
            if(UI::IsItemHovered())UI::SetTooltip("On: speed follows left-stick travel. Off: the current fixed-speed movement (default).\nVehicle controls keep their existing response.");
            UI::BeginDisabled(!analogMovement);
            int leftDeadzone=int(state.xrLeftStickDeadzone*100+.5f);
            int rightDeadzone=int(state.xrRightStickDeadzone*100+.5f);
            int fullInput=int(state.xrMaxInputThreshold*100+.5f);
            if(widgets::SliderInt("Left stick deadzone",&leftDeadzone,0,30,"%d%%")){state.xrLeftStickDeadzone=leftDeadzone*.01f;changed=true;}
            if(widgets::SliderInt("Right stick deadzone",&rightDeadzone,0,30,"%d%%")){state.xrRightStickDeadzone=rightDeadzone*.01f;changed=true;}
            if(widgets::SliderInt("Full input threshold",&fullInput,80,100,"%d%%")){state.xrMaxInputThreshold=fullInput*.01f;changed=true;}
            if(UI::IsItemHovered())UI::SetTooltip("Raw travel that produces full analog input and activates sprint, dash and crouch.\nThe usable travel between the deadzone and this threshold maps to 0-100%%.");
            UI::EndDisabled();
            // How the sprint starts (Hooks/XInput.cpp, the sprint gesture).
            changed |= widgets::Combo("Sprint", &state.xrSprintMode, "Hold the left stick forward\0Click the left stick\0");
            if (UI::IsItemHovered()) {
                UI::SetTooltip("Hold the left stick forward: the stick held at the stop for a moment sprints (default).\n"
                                  "Click the left stick: a click sprints until the stick is let go.");
            }
            const char* moveSrcNames[] = { "Game (camera)", "HMD (head)", "Left hand", "Right hand" };
            int moveSrc = state.xrMovementSource;
            if (moveSrc < 0 || moveSrc > 3) moveSrc = state.xrMovementControl != 0 ? 1 : 0;
            if (widgets::Combo("Move source", &moveSrc, moveSrcNames, IM_ARRAYSIZE(moveSrcNames))) {
                state.xrMovementSource = moveSrc;
                state.xrMovementControl = moveSrc != 0 ? 1 : 0;
                changed = true;
            }
            if (UI::IsItemHovered()) {
                UI::SetTooltip("Game     - left stick walks the way the camera faces (vanilla).\n"
                                  "HMD      - left stick walks the way the headset faces.\n"
                                  "Left/Right hand - walks the way the chosen controller points.\n"
                                  "Vehicles always keep game heading.");
            }

            UI::Separator();
            changed |= CheckboxInt("Breaststroke swimming", &state.xrBreaststrokeSwim);
            changed |= CheckboxInt("Physical crouch", &state.xrPhysicalCrouch);
            float crouchCm=state.xrPhysicalCrouchDepth*100.f;
            if(UI::SliderFloat("Physical crouch depth",&crouchCm,15.f,55.f,"%.0f cm")) {
                state.xrPhysicalCrouchDepth=crouchCm*.01f;changed=true;
            }
            if(UI::IsItemHovered())UI::SetTooltip("Height loss after the measured lean. Standing back up releases physical crouch.");
            if (UI::IsItemHovered()) {
                UI::SetTooltip("In water: reach forward with both hands, sweep out and pull back.\n"
                                  "Movement begins during the pull. Gentle strokes swim normally; fast pulls boost.\n"
                                  "Swim where the headset points, including up/down; no stick needed.\n"
                                  "To rise, raise separated hands to chest level and push both down.\n"
                                  "Repeat strokes to keep moving. Pull the left stick back to stop.");
            }
            UI::Separator();
            changed |= CheckboxInt("Ladder grip climbing", &state.xrLadderGripClimb);
            if(UI::IsItemHovered())UI::SetTooltip("Grip a side rail or rung. Pull the hand down to climb up,\n"
                "or move it up to climb down. Release to stop. Native pelvis and legs stay on the ladder.");
            changed |= CheckboxInt("Auto finish near ladder top", &state.xrLadderAutoFinish);
            if(UI::IsItemHovered())UI::SetTooltip("After an upward pull near the top, finish climbing even when grips are released.\n"
                "Move the stick or push a held hand upward to cancel.");
            if(state.xrLadderAutoFinish)changed |= widgets::SliderFloat("Ladder finish distance", &state.xrLadderFinishDistance,.2f,1.2f,"%.2f m");
            UI::Separator();
            UI::TextUnformatted("Turning (right stick)");
            changed |= CheckboxInt("Snap turn", &state.xrSnapTurn);
            if (UI::IsItemHovered()) {
                UI::SetTooltip("Convert the right-stick X axis into discrete snap pulses\n"
                                  "instead of smooth rotation. Helps with motion sickness.");
            }
            if (state.xrSnapTurn != 0) {
                changed |= widgets::SliderFloat("Snap angle", &state.xrSnapTurnAngleDeg, 10.0f, 90.0f, "%.0f deg");
            }

            }
            if(section<0)UI::EndTabItem();
        }

        if (section>=0 ? section==4 : UI::BeginTabItem("STEREO")) {
            if(UI::CollapsingHeader("PERFORMANCE",ImGuiTreeNodeFlags_DefaultOpen)) {
                bool stableEmptyPacing = CyberpunkVR_OverlaySkipEmpty == 0;
                if (UI::Checkbox("Stable pacing with no overlay graphics", &stableEmptyPacing)) {
                    CyberpunkVR_OverlaySkipEmpty = stableEmptyPacing ? 0 : 1;
                    Log("[pacing] stable empty-overlay pacing=%d (skipEmpty=%d)\n",
                        stableEmptyPacing, CyberpunkVR_OverlaySkipEmpty);
                }
                if (UI::IsItemHovered()) UI::SetTooltip(
                    "Keep the previous-overlay fence pacing even when no overlay is drawn.\n"
                    "May improve headset delivery when mounting or holstering. Adds a small\n"
                    "empty command submission. Uncheck for upstream behavior; resets ON at launch.");

                int reflex=state.nvidiaReflex+1;
                if(widgets::Combo("NVIDIA Reflex",&reflex,"Game setting\0Off (higher FPS)\0On (lower latency)\0On + Boost\0")) {
                    state.nvidiaReflex=reflex-1;changed=true;
                }
                UI::TextWrapped("Off can improve GPU utilization and FPS, but may increase input latency.");
                const int applied=cvr::reflex::GetAppliedMode();
                const char* appliedName=applied==0?"Off":applied==1?"On":applied==2?"On + Boost":"Waiting for renderer";
                UI::TextDisabled("Applied: %s",appliedName);
                changed |= CheckboxInt("Limit FPS to the headset refresh rate", &state.fpsCap);
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("Frames above the headset's refresh rate are never shown: they only load the GPU\n"
                                      "and make the picture stutter now and then. Off lets the game run free.");
                }
            }
            if (UI::CollapsingHeader("STEREO VIEW", ImGuiTreeNodeFlags_DefaultOpen)) {
                changed |= CheckboxInt("Mono mode", &state.monoMode);
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("One picture for both eyes, drawn once from the centre of the head:\n"
                                      "no depth, the most frames. Engine stereo and the second view are off.");
                }
                // OSR and AFW: two ways to the same launch (the eyes in turn), one at a time
                if (CheckboxInt("OSR (the port's other-eye reconstruction)", &state.osr)) {
                    if (state.osr) state.afw = 0;
                    changed = true;
                }
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("The eyes take turns: each frame draws one eye, and the port makes the other\n"
                                      "from its colour and depth. About mono's frame cost, with depth. Engine stereo\n"
                                      "and frame generation are off while it is on. Restart to apply.");
                }
                if (CheckboxInt("AFW (PureDark frame warp)", &state.afw)) {
                    if (state.afw) state.osr = 0;
                    changed = true;
                }
                if (UI::IsItemHovered()) {
                    UI::SetTooltip("The eyes take turns: each frame draws one eye, and PureDark's plugin\n"
                                      "(PDAFWPlugin.dll in the port's plugin folder, not shipped) makes the other.\n"
                                      "Without the plugin OSR makes it. Engine stereo and frame generation are off\n"
                                      "while it is on. Restart to apply (from OSR: at once).");
                }
                if (cvr::afw::Launch()) {
                    const auto afw = cvr::afw::Statistics();
                    UI::TextDisabled("%s: %llu made, %llu fell back%s%s",
                                     cvr::afw::UsesOsr() ? "OSR" : "AFW",
                                     static_cast<unsigned long long>(afw.warped),
                                     static_cast<unsigned long long>(afw.fallbacks), *afw.last ? " -- " : "", afw.last);
                }
                // Engine stereo is sized into the window at start-up and a launch in mono has no second view, so
                // those directions wait for the next launch (Stereo/EngineStereo.hpp ApplyMonoMode, RestartPending).
                if (cvr::engine_stereo::RestartPending())
                    UI::TextColored(ImVec4(1.0f, 0.8f, 0.35f, 1.0f), "Restart the game to apply");
                changed |= DrawFovControl(state);
        changed |= widgets::SliderFloat("Motion prediction (ms)", &state.xrMotionPredictMs, 0.0f, 60.0f, "%.1f ms");
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Forward-predicts the head pose by this many ms using head\n"
                              "velocity, hiding render-to-photon latency. 0 = off.\n"
                              "Tune up until motion feels responsive without overshoot.");
        }
        changed |= widgets::SliderFloat("Stereo separation x", &state.xrStereoScale, 0.25f, 5.0f, "%.2fx");
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Personal fine-tune on the auto IPD. 1.0 = calibrated natural\n"
                              "separation, auto-scaled to the headset's runtime IPD. Nudge\n"
                              "0.8-1.2 for taste; crank to 3-5x to exaggerate depth and make\n"
                              "the eye alternation obvious on the flat monitor for testing.");
        }
        // ── World scale + honest IPD ──
        changed |= widgets::SliderFloat("World scale", &state.xrWorldScale, 0.20f, 3.0f, "%.2f");
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Scales eye separation AND head translation together.\n"
                              "Lower it (e.g. 0.8) to make the world look\n"
                              "BIGGER / yourself smaller; raise to shrink the world. Use this if V\n"
                              "and NPCs feel too large.");
        }
        changed |= widgets::SliderFloat("IPD scale", &state.xrIpdScale, 0.50f, 2.0f, "%.2fx");
        if (UI::IsItemHovered()) {
            UI::SetTooltip("Eye-separation multiplier on the runtime IPD. 1.0 = the neutral\n"
                              "baseline (+-0.033 m on a typical headset).\n"
                              "Affects stereo depth (diorama vs giant), NOT the monocular size.");
        }
        bool reuseLastFrame = state.xrReuseLastFrame != 0;
        if (UI::Checkbox("Reuse last clean frame", &reuseLastFrame)) {
            state.xrReuseLastFrame = reuseLastFrame ? 1 : 0;
            changed = true;
        }
        if (UI::IsItemHovered()) {
            UI::SetTooltip("On stale ticks, re-submit the last clean captured eye and let\n"
                              "the compositor reproject it, instead of warping stale content\n"
                              "again. May lower submit rate\n"
                              "toward the capture rate. Off = always warp the stale eye.");
        }
        // The "Pose pair-lock" checkbox is gone: it toggled a freeze that only meant something
        // under AER's one-camera eye alternation. The ini key still round-trips so old files load.
        changed |= CheckboxInt("Asymmetric projection", &state.xrAsymProjection);
        if (UI::IsItemHovered()) {
            UI::SetTooltip("ON: each eye renders exactly its own off-centre lens window\n"
                              "(more of the panel used, square pixels).\n"
                              "OFF: the symmetric frustum sized to the widest half-angle.");
        }
            }

            DrawStereoControls();
            if(section<0)UI::EndTabItem();
        }

        if (section>=0 ? section==5 : UI::BeginTabItem("AVATAR")) {
            DrawVRHandsControls();
            if(section<0)UI::EndTabItem();
        }

        if(section<0)UI::EndTabBar();
    }

    return changed;
}

}  // namespace overlay
using namespace overlay;
