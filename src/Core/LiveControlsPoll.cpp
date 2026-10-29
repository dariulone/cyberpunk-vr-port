// LiveControlsPoll -- reading the forty-odd live controls, once per tick, off the game threads.
//
// The controls are volatile scalars that hooks read directly (see Core/LiveControls.hpp for why there
// are no accessors). This file is the only writer, and it runs on the worker thread, which is what makes
// direct reads safe enough: a hook sees either the old value or the new one, never a half-written
// struct, because nothing here writes anything wider than a scalar.
//
// READ THE FILE, NOT THE SOURCE DEFAULT. A value that looks wrong is checked against vrport.ini, not
// against the initialiser in the header -- xr_motion_predict_ms sat at 50.9 in the ini while the source
// said something else, and that was the VR judder for a week.
//
// MakeLiveControlsUiState / PersistLiveControlsUiState are the overlay's side: it edits a snapshot and
// hands it back, so a slider dragged in the headset survives a restart.

#include "Overlay/ImGuiOverlay.hpp"   // OverlayArmLoadGuard
#include "Hooks/AnalogStick.hpp"
#include "Anim/WheelRange.hpp"
#include "Hooks/Reflex.hpp"
#include "Hooks/ReflexOptions.hpp"
#include <windows.h>
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <algorithm>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Render/PhysicalOpticsShaders.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include "Stereo/EngineStereo.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <iostream>

#include <MinHook.h>
#include "Hooks/SwapChain.hpp"
#include "Utils/LogThrottle.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Runtimes/HybridBodyYaw.hpp"
#include "Anim/ScenePolicy.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"

// Poll the CET VRIK mod's recenter request (written with an incrementing counter on
// save load / OnGameAttached); recenter when the counter changes.
static void PollVrikRecenterRequest() {
    InitRuntimePaths();
    WIN32_FILE_ATTRIBUTE_DATA fd;
    if (!GetFileAttributesExA(g_vrikRecenterPath, GetFileExInfoStandard, &fd)) return;
    if (CompareFileTime(&fd.ftLastWriteTime, &g_lastVrikRecenterWrite) == 0) return;
    g_lastVrikRecenterWrite = fd.ftLastWriteTime;

    FILE* file = _fsopen(g_vrikRecenterPath, "r", _SH_DENYNO);
    if (!file) return;
    char line[64];
    int counter = -1;
    while (fgets(line, sizeof(line), file)) {
        int v = 0;
        if (sscanf_s(line, "recenter=%d", &v) == 1) { counter = v; break; }
    }
    fclose(file);
    if (counter == 0) return;
    // Baseline was captured at startup (InitRuntimePaths); any later change = a fresh
    // OnGameAttached this session.
    if (counter != g_lastVrikRecenterCounter) {
        g_lastVrikRecenterCounter = counter;
        OpenXRManager::Get().RequestRecenter();
        // THE SAME SIGNAL MARKS THE LOAD for the overlay's pacing guard: on the branch this pacing
        // came from, every recorded DXGI_ERROR_DEVICE_HUNG landed within a few frames of this point.
        OverlayArmLoadGuard("save load");
        Log("VRIK recenter request (save load) -> recentering. counter=%d\n", counter);
    }
}

// Tracking-smoothing accessors (atomics live in openxr_manager.cpp). The proxy
// owns their ini persistence: parse -> Set* on file change, Get* -> write on Save.
extern "C" float GetHmdTrackingSmooth(); extern "C" void SetHmdTrackingSmooth(float);
// How near the support point the off hand has to be before the two-handed hold is offered, in metres.
// The grading-mirror mask: which of the eight measured differences in the LUT build's 688-byte constant
// block are taken from MAIN for the second view. One bit per candidate, deliberately -- see
// src/Stereo/Grading.cpp for the offsets and what each one measured. Live, because the bisect belongs to
// whoever is looking at the picture.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_GradeMirrorMask;
// The extra environment-handle slots: nine 8-byte fields in the graph context laid out as three
// elements of stride 0x3A0 -- the shape of a blended area-params list, and hdrLut/ldrLut ride in one
// of them. One bit per slot, 0 by default: refcounted handles, so a wrong one can kill the process.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_EnvExtraMask;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_WaterMirror;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_WaterAssign;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_ViewDataDiff;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_LutSrcLend;
extern "C" __declspec(dllexport) extern float CyberpunkVR_SceneStickYawDps;
extern "C" __declspec(dllexport) extern int   CyberpunkVR_SceneStickYawTier;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_BlockDiff;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_BlockDiffNode;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_BlockDiffSize;
// Which render-mask categories the second view is granted, one bit per row of kRenderMasks in
// src/Stereo/NodeDispatch.cpp. A category the view lacks makes the engine REFUSE whole nodes to it:
// bit 10 ClearLighting is what CRenderNode_HistogramUpdate asks for, and without it the second
// view's auto-exposure never runs at all.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_RenderMaskGrant;
// xr_stage_replay: MAIN replays VRCAM's recorded RenderElements stages with its own camera instead of
// recording them (src/Render/StageReplay.cpp, docs/single-pass-stage-replay-20261008.md). Applied
// only when the setting itself changes, so an unrelated ini edit never resets live test controls.
extern "C" __declspec(dllexport) int CyberpunkVR_StageReplaySetting = 0;
extern "C" void CyberpunkVR_StageReplayConfigure(int enable);
static int g_appliedStageReplay = -1;
// xr_vrcam_gbuffer_partitions: the second eye records its G-buffer in worker partitions like MAIN
// does (src/Render/StageReplay.cpp, ApplyPartitions). Independent of xr_stage_replay; the engine
// clamps a render-to-texture view to one partition, a 2 ms single-thread job on the frame's
// critical path in the city.
extern "C" __declspec(dllexport) int CyberpunkVR_VrcamGbufferPartitionsSetting = 1;
extern "C" void CyberpunkVR_VrcamGbufferPartitionsConfigure(int enable);
static int g_appliedGbufferPartitions = -1;
// xr_submit_chain: each render-graph submission point gets its own job-stage chain, so a graph's
// Synchronize nodes no longer wait for the previous batch's ExecuteCommandLists (src/Stereo/SubmitChain.cpp,
// docs/gpu-not-full-20261016.md). 0 off, 1 view jobs and Present wait for the chain, 2 only Present
// (MAIN's graph starts while VRCAM's last batches are still being submitted, -0.4 ms
// against 1 in the city). Off by default. Applied only when the setting changes, so a live CyberpunkVR_SubmitChain test
// value survives ini edits.
extern "C" __declspec(dllexport) int CyberpunkVR_SubmitChainSetting = 0;
extern "C" void CyberpunkVR_SubmitChainConfigure(int mode);
static int g_appliedSubmitChain = -1;
// The deferred present (SubmitChain.cpp) and the game thread leaving render jobs to the workers
// (GameThreadQueues.cpp): both live, both off by default, as ini keys so a launch that looks wrong can be
// A/B-ed without a debugger. Applied only on a change, like the chain, so a live test value survives edits.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmitChainDeferPresent;
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GameThreadSkipRenderQueue;
extern "C" __declspec(dllexport) int CyberpunkVR_DeferPresentSetting = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_GtSkipRenderQueueSetting = 0;
static int g_appliedDeferPresent = -1, g_appliedGtSkipRenderQueue = -1;
// Which of the second view's viewData holes are filled from MAIN, one bit per entry in
// kViewDataHoles (src/Stereo/ViewReuse.cpp). Live so the untried entries can be bisected without a
// build, which is what had kept three of them untried.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_ViewDataFixMask;
// The viewData ranges mirrored from MAIN into the second view. Bit 12 is the colour-grade
// section at +0x640: without it the second eye renders every environment with the identity
// grade, which is why a braindance has no green cast there and no bloom.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_EnvMirrorMask;
// The second eye's camera in a braindance: bit 1 raises the engine's rebuild flag at comp+0xA00,
// bit 2 writes a fresh pose in first. 0 restores the behaviour every build up to now had, where the
// flag was never raised and the view was rebuilt only when the engine happened to mark it itself.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdCamDirty;
// Bit 1 = the second eye, bit 2 = MAIN: push the component's transform every frame and call its
// own vtable+0x240 notify, which is what makes the engine consume it. 1 ships.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdPushTransform;
// 1 = MAIN's viewpoint in a braindance comes from the same per-frame scene pose the second eye is
// placed with, so all four quantities that position the pair move at one cadence. 1 ships.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdMainPosFromScene;
// 1 = the second eye is placed relative to MAIN's own located position for this frame; 0 = relative
// to the scene pose the script publishes. 1 ships -- it takes the script out of the pair's geometry.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdPushBase;
// 1 = while the braindance push owns the second eye, PatchCamera stops composing its own rotation
// and stops adding the head delta and the eye separation for that eye. One composition, one writer.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdOneComposition;
// 1 = in the braindance EDITOR the second eye takes MAIN's own base, so the pair stops sitting at
// two different heights. Playback is untouched by this -- there the scene pose is the base.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdEditorAlign;
// 1 = the head is composed onto the scene rotation the ENGINE has for this frame, taken from the
// located buffer at entry; 0 = onto the pose the CET script publishes, which is not per frame.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdBaseFromLocate;
// 1 = in a braindance MAIN's orientation is taken from whatever PatchCamera last composed, which is
// only as often as a camera is patched -- 3.4 times a second there. 0 = LocateCamera composes its
// own every frame. It had no live key until now, so every test of it so far ran at the default.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdQuatFromWriteSite;
// Where the second eye takes its position from: 0 its own attachment, 1 MAIN's base in a
// braindance, 2 MAIN's base always. See the note beside the export in VrCore.cpp.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_VrcamPosFromMain;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_DevCamInLocate;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_LensHeadWrite;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_InputDefaultInUi;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_PopupMagBlockMs;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_DevCamAnyName;
// The node whose square dispatch draws the scanner outline, and the switch for replacing its shader.
// Live in the ini because the candidates come from a measurement ([viscs] names them) and trying one
// must not cost a rebuild: the node is read at every dispatch, so a change takes effect at once.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_VisionCsNode;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_VisionCsSwap;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_VisionCsDim;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_VisionCsOrd;
// Sends EVERY draw item down the scene branch of the engine's plane router, so the first-person
// plane is drawn with the scene's parameter block instead of its own. 0 by default -- see
// src/Stereo/WeaponPlane.cpp for what the weapon plane buys and what giving it up costs.
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_WeaponPlaneAsScene;
// Nops the per-object plane filter in the render gather, so an object on the first-person plane is
// not dropped before its highlight can be drawn. 0 by default.
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_HighlightAnyPlane;
// Sends EVERY draw item to the scene draw-list bucket, so the first-person plane stops being a
// separate pile at all. 0 by default.
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_PlaneBucketScene;
// Declared rather than included: this file has no other business with the stereo internals, and
// the definition lives in src/Stereo/WeaponPlane.cpp.
namespace cvr { namespace detail { void weapon_plane_sync(); void highlight_any_plane_sync();
                                  void plane_bucket_scene_sync(); } }
extern "C" __declspec(dllexport) extern float CyberpunkVR_DevCamTolM;
// 1 = in a braindance MAIN's half-IPD goes into the located buffer instead of its camera
// component, which renders nothing there. 0 restores the component write.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdIpdInLocate;
// 1 = the braindance head base is the located buffer's own quaternion (fresh), 0 = the pose the
// CET script publishes for the scene camera (a tick old).
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_BdQuatFromBuffer;
// The light cull's capability grant, and the read-only probe of its three gates.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_CapGrant;
// Copies MAIN's fov/zoom/near/far onto the second view's camera, many times per frame.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_ForceVrcamCam;
// Which of MAIN's camera fields the second view takes: 1 fov, 2 zoom, 4 near, 8 far.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_VrcamCamFields;
// 0 off, 1 the engine's whole pass (it also ERASES faded entries from a list MAIN shares),
// 2 the same apply walked here with nothing removed. 2 is the one to use.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_RunViewParams;
// The exposure readback: 0 off, 1 a line every 8 s, 2 a line every 200 ms for catching a flash.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_ExpoProbe;
// 1 = count ExecuteIndirect per frame-graph node and per view, and print [indirect] every 10 s.
// It names the node the second view replays with an argument buffer that is not built.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_IndirectCensus;
// 1 = never declare a StateBefore for a foreign resource unless it was observed. Costs the eye
// capture on every frame where no barrier for that resource was seen -- i.e. stereo. Default 0.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_NoStateLies;
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_StableCopy;
// THE REUSE MODES, AS LIVE KEYS. Each one makes an engine function RETURN EARLY for the second
// view -- distant render/prepare, local shadow maps, GI, reflection probes -- so the second view
// reuses MAIN's result instead of building its own. That is wanted, and it is also the only
// mechanism in the port that suppresses engine WORK for one view and not the other.
//
// Which matters because of what the 0x88 crash turned out to be: the driver faults computing a
// resource state of INDIRECT_ARGUMENT on a binding whose resource record is null -- an object
// that has an allocation and a GPU address and was never built. Measured 2026-08-29 with the
// [indarg] watch: the second view issues 67452 such transitions to MAIN's 80986, on the SAME
// nodes (no node is VRCAM-only), and the last six before the crash were all the second view's.
// A skipped builder followed by a transition of what it did not build is exactly that shape.
//
// None of these had a live key, which is why none had ever been bisected against the crash.
// They are keys now so the four can be walked in ONE session instead of four rebuilds.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DistantReuseMode;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_LocalShadowReuseMode;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_GiReuseMode;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_ProbeReuseMode;
// 1 = the second view takes MAIN's exposure adaptation. Which fields is the mask below, DECIMAL:
// one bit per float, 27 = the adaptation set, 127 = the whole buffer (which whitens the eye).
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_ExpoMirror;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_ExpoFieldMask;
// Which readers of viewData+0x168 get MAIN's composition state, for the duration of that call:
// 1 CompositionPostProcess, 2 the RT declarations, 4 DrawHUD, 8 the fifth reader. Never
// DrawComposition, which is the one that crashes on it. Start at 3.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_CompLendScoped;
// The UNSCOPED composition lend, restored from the 0.1.6 render: it puts the borrowed state in the
// block on every dispatch of our view, which is what lets the second view run its composition at
// all. The scoped variant beside it deliberately withholds DrawComposition; this one does not.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_CompLendSet;
// The other half of that lend: the second eye's copy is taken from the FINISHED frame, at the
// epilogue of the composition node that writes it, instead of from the mid-chain target. Only
// meaningful while the lend is armed, because without it that node does not run for this view.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_FinalGrab;
// Which member of the 8-bit family a TYPELESS eye snapshot is typed as. 1 = _UNORM_SRGB, so the
// sampler decodes and ColorBlit's sRGB target re-encodes -- a round trip. 0 = _UNORM, sampled raw.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_StableSrgbView;
// Byte-compares the light array and the particle-lighting uploads between the views.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_LightContent;
// 0 = no LOD override at all, both eyes on the engine's own value. LodFov.cpp records what the
// debugger found at that site: the gate keys on a stack-local field that is not the fov.
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_FixLodEnable;
// The port's OWN HUD composite into the second eye. Made live because the engine's own HUD now
// reaches that view through DrawComposition + CompositionPostProcess (xr_comp_lend_set), so this
// path is a candidate for retirement and wants an A/B without a rebuild.
extern "C" __declspec(dllexport) extern int      CyberpunkVR_HudToSecondEye;
// xr_grip_from_fingers: kept here so a save keeps it; Runtimes/FingerGrip.cpp reads it when the session starts.
static int g_gripFromFingersSetting = 0;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_FingerBones;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_ThumbRigOpen;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_FingerSpread;
// 1 = each eye renders its own off-centre window and is submitted with it (src/Stereo/AsymProjection.cpp);
// 0 = the old symmetric frustum sized to the widest half-angle. 2 = test values (CyberpunkVR_AsymTest).
extern "C" __declspec(dllexport) int32_t CyberpunkVR_AsymProjection;
// 0..1: how far each eye's window reaches towards the other's on its inner side (1 = both eyes the same
// horizontal range, which keeps screen-space effects identical between the eyes).
extern "C" __declspec(dllexport) float CyberpunkVR_AsymInnerGuard;
// The port's OWN copy of the vision/outline layer into the second eye. It exists from the time the
// second view had no composition of its own and nothing drew the layer there; since the second view
// builds its own frame the engine draws it, and this copy is a second pass over the same pixels.
// Live so the two can be compared without a rebuild.
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VisionToSecondEye;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_HudInBraindance;
// 1 = inject the composition group into the second view's RTT graph through the engine's own pass
// adders. Needed because that view is built by SCENE_INCR, which contains neither the build-bit-82
// block nor the final builder that MAIN reaches composition through -- so forcing the bit cannot work.
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_VrcamCompositionGroup;
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandRadius;
extern "C" __declspec(dllexport) extern int   CyberpunkVR_TwoHandAim;
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandBreakRadius;
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandAimReleaseMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_CarryGripRadius;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RifleRiseMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RifleBackMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_ShotgunRiseMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_ShotgunBackMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilDownPistol;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilDownRifle;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilDownShotgun;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandRecoilBackCm;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandRecoilBackMaxCm;
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandBackMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_SniperRiseMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_SniperBackMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilScale;
extern "C" __declspec(dllexport) extern int   CyberpunkVR_Haptics;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HapticScale;
extern "C" __declspec(dllexport) extern float CyberpunkVR_PistolBackMul;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilClimbFrac;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilClimbMaxDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_RecoilClimbMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandRecoilReturnMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandRecoilReturnPow;
// WHERE THE SCANNER'S HUD SITS -- four movable pieces of it, as (x, y, scale) each. Written by the
// in-game editor through VRScannerSlotSet, read back by the CyberpunkVRPort_ScannerHud redscript.
//
//   [0..2]    the scanner frame      scannerGameController
//   [3..5]    the details panel      scannerDetailsGameController
//   [6..8]    the quickhack panel    QuickhacksListGameController
//   [9..11]   the hint line          ScannerHintInkGameController
//   [12..14]  the cyberdeck memory   the panel's top_panel container
//   [15..17]  the script list        the panel's left_panel container
//   [18..20]  the script description the panel's right_panel container
//
// The last three are CHILDREN of the quickhack panel above them, which is the point: they shared
// one movable block and could not be separated. Their offsets compose with the parent's -- move
// the panel and they follow, then nudge one of them on top of that.
//
// They are CONTAINERS, named as the .inkwidget names them, and that is a correction: the first
// version moved the widgets the controller keeps private refs to -- cells_memory and the list --
// which are INSIDE those containers, so the background, the separator line and the headings stayed
// behind. top_panel holds the memory's title, line, fluff and cells; left_panel holds the list and
// its heading; right_panel holds the description block.
//
// It started as ONE triple for the whole scanner, which was wrong before it was written: these are
// four separate widgets sitting in four places, so a single offset can only be right for one of them.
//
// Offsets are in the HUD's own design pixels (1920x1080), positive x right and positive y down, which
// is the convention inkWidget.SetTranslation takes. All four ship at (0, 0, 1): a mod that moves
// somebody's HUD before they ask it to is a bug, however good the default.
//
// AND THIS FILE IS THE PERSISTENCE, not just the source. A redscript cannot write a file and the CET
// bridge is not a dependency this wants, so the editor writes into these globals and asks for one ini
// save when it closes. The poll below only re-reads the ini when its write time CHANGES, so a live
// drag is not clobbered by the next poll.
// NOT ZEROES ANY MORE. These are the numbers the layout was actually dragged to in the headset, read
// back out of vrport.ini, and they ship because that ini is per-install and never enters the
// repository: on a fresh install zeroes mean the vanilla layout and every tester hunting the same
// numbers again. An existing ini still wins -- the poll below reads the file over these -- so nobody's
// own layout is touched.
// NEUTRAL, AND THAT IS THE CONFIGURATION THAT WAS ACTUALLY PLAYED.
//
// These used to carry tuned offsets -- frame -10.4, details -178.2/-57.4 at 0.4, hacks -75.4/1.8 at
// 0.8, memory 17.6/517.8 at 0.7, scripts 260.8/-8.0 at 0.6. They were dead in practice: an existing
// vrport.ini is parsed at startup and overwrites this array, and the live ini had all seven at
// 0,0,1 -- so the game has been running neutral, while a fresh install (a tester, who gets no ini)
// would have got the offsets nobody had looked at in a long time. Taken from the live config so the
// two agree, which is the whole point of a default.
//
// The tuned numbers are kept in this comment rather than in the array: if the panels ever need moving
// again, the in-game editor is the way -- hold RIGHT SHIFT while scanning -- and it writes the ini.
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerSlots[21] = {
      0.0f,    0.0f, 1.000f,   // the scanner frame
      0.0f,    0.0f, 1.000f,   // the details panel
      0.0f,    0.0f, 1.000f,   // the quickhack panel
      0.0f,    0.0f, 1.000f,   // the hint line
      0.0f,    0.0f, 1.000f,   // top_panel: the cyberdeck memory, whole
      0.0f,    0.0f, 1.000f,   // left_panel: the script list and its heading
      0.0f,    0.0f, 1.000f,   // right_panel: the description block
};
// The hand filter is UEVR-form now and lives in FlushHandsToShared; its speed is this.
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpSpeed;
// THE FILTER'S SPEED FOLLOWS WHAT IS IN THE HAND. The filter trades tracking noise against lag,
// and which of the two hurts depends on what the hand is doing: a blade is swung and any lag reads
// as the weapon trailing the arm; a sniper is held still and aimed, where the noise is what shows
// and the lag costs nothing. 0 in any of these means "use the base" (xr_hand_lerp_speed), which is
// also what empty hands and an unnamed weapon get.
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpPistol;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpRifle;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpShotgun;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpSniper;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpMelee;
// What every hand rate above is multiplied by under frame generation; 0 = no filter there.
extern "C" __declspec(dllexport) extern float CyberpunkVR_HandLerpFramegenScale;
// 1 = hand offsets measured from the filtered head (the one they are re-anchored on).
extern "C" __declspec(dllexport) extern int CyberpunkVR_HandRelToFilteredHead;
// 1 = the published hand pose is located once per frame, for that frame's instant.
extern "C" __declspec(dllexport) extern int CyberpunkVR_HandLocatePerFrame;

extern "C" __declspec(dllexport) bool GetWeaponAimEnabled() {
    return OpenXRManager::Get().GetWeaponAimEnable();
}

void PollLiveControls() {
    InitRuntimePaths();
    PollVrikRecenterRequest();

    WIN32_FILE_ATTRIBUTE_DATA fileData;
    if (!GetFileAttributesExA(g_liveControlPath, GetFileExInfoStandard, &fileData)) {
        return;
    }

    if (CompareFileTime(&fileData.ftLastWriteTime, &g_lastLiveControlWrite) == 0) {
        return;
    }

    g_lastLiveControlWrite = fileData.ftLastWriteTime;

    // Neutral-neck mounting supplies the anatomical offset. These are optional trims.
    float xrHeadOffsetX = 0.0f;
    float xrHeadOffsetY = 0.0f;
    float xrHeadOffsetZ = 0.0f;
    int xrRecenter = 0;
    int xrMonoSubmit = 1;
    // Seeded from the live value, not from a constant: an ini without the key must leave the
    // current mode alone rather than reset it.
    int xrThreadedSubmit = CyberpunkVR_ThreadedMonoSubmit;
    int xrWindowWidth = 0;
    int xrWindowHeight = 0;
    float xrForceFov = 0.0f;
    int xrMenuRect = 0;
    float xrMenuFov = 65.4f;
    float xrMenuFollowDeg = 60.0f;
    int xrHudPanel=1;
    float xrHudFollowDeg=60.0f, xrHudFov=65.4f, xrHudDistance=1.5f;
    int xrInteractionPanel=1;float xrInteractionFollowDeg=0.0f,xrInteractionDistance=2.0f,xrInteractionFov=75.0f;
    float xrLootFollowDeg=0.0f;
    int xrHudFollowMode=0, xrHudStereoDepth=1;
    float xrHudBrightness=1.0f, xrHudShadow=1.0f, xrHudGlow=1.0f;
    cvr::hud::LayoutSettings hudElements{};
    auto framegen=cvr::framegen::GetSettings();
    cvr::foveated::Settings foveated{};
    cvr::vrs::Settings vrs{};
    cvr::hidden_area::Settings hiddenArea{};
    uint32_t monoCascades=CyberpunkVR_MonoCascades.load(std::memory_order_relaxed);
    int monoMode=CyberpunkVR_MonoMode;
    int scopeStabilize=cvr::optics::source::GetStabilize();
    int nvidiaReflex=cvr::reflex::DefaultMode;
    auto overlay=cvr::vrui::GetSettings();
    float xrPitchSign = 1.0f;
    float xrPitchScale = 1.35f;
    int xrSyncSequential = 1;
    int xr3DofMovement = 0;
    int xrFirstLaunch = 1;
    float xrMotionPredictMs = 0.0f;
    float xrStereoScale = 1.0f;
    float xrWorldScale = 1.0f;
    float xrIpdScale = 1.0f;
    float xrSharpness = 0.0f;
    float xrSharpmix = cvr::sharpen::DefaultAmount;
    int xrReuseLastFrame = 0;
    int xrPairLock = 0;
    int xrRenderPoseSubmit = 1;
    int xrPoseLag = 1;
    int xrRuntime = 0;
    // Per-eye, frame-labelled depth snapshots. Missing/invalid/synthetic depth
    // is omitted from that projection layer rather than borrowed from another frame.
    int xrDepthSubmit = 1;
    int xrMovementControl = g_liveControls.xrMovementControl;
    int xrDisableMouseY = g_liveControls.xrDisableMouseY;
    int xrXInputHook = g_liveControls.xrXInputHook != 0 ? g_liveControls.xrXInputHook : 1;
    int xrSnapTurn = g_liveControls.xrSnapTurn;
    float xrHmdSmooth = GetHmdTrackingSmooth();
    float xrHandLerp = CyberpunkVR_HandLerpSpeed;
    float xrLerpPistol  = CyberpunkVR_HandLerpPistol;
    float xrLerpRifle   = CyberpunkVR_HandLerpRifle;
    float xrLerpShotgun = CyberpunkVR_HandLerpShotgun;
    float xrLerpSniper  = CyberpunkVR_HandLerpSniper;
    float xrLerpMelee   = CyberpunkVR_HandLerpMelee;
    float xrLerpFramegen = CyberpunkVR_HandLerpFramegenScale;
    uint32_t xrGradeMirrorMask = CyberpunkVR_GradeMirrorMask;
    uint32_t xrEnvExtraMask = CyberpunkVR_EnvExtraMask;
    uint32_t xrWaterMirror = CyberpunkVR_WaterMirror;
    uint32_t xrWaterAssign = CyberpunkVR_WaterAssign;
    int32_t  xrViewDataDiff = CyberpunkVR_ViewDataDiff;
    uint32_t xrLutSrcLend    = CyberpunkVR_LutSrcLend;
    float    xrSceneYawDps   = CyberpunkVR_SceneStickYawDps;
    int      xrSceneYawTier  = CyberpunkVR_SceneStickYawTier;
    int32_t  xrBlockDiff     = CyberpunkVR_BlockDiff;
    uint32_t xrBlockDiffNode = CyberpunkVR_BlockDiffNode;
    uint32_t xrBlockDiffSize = CyberpunkVR_BlockDiffSize;
    uint32_t xrRenderMaskGrant = CyberpunkVR_RenderMaskGrant;
    int      xrStageReplay   = CyberpunkVR_StageReplaySetting;
    int      xrGbufferPartitions = CyberpunkVR_VrcamGbufferPartitionsSetting;
    int      xrSubmitChain = CyberpunkVR_SubmitChainSetting;
    int      xrDeferPresent = CyberpunkVR_DeferPresentSetting;
    int      xrGtSkipRenderQueue = CyberpunkVR_GtSkipRenderQueueSetting;
    uint32_t xrViewDataFixMask = CyberpunkVR_ViewDataFixMask;
    uint32_t xrEnvMirrorMask = CyberpunkVR_EnvMirrorMask;
    int32_t  xrBdCamDirty = CyberpunkVR_BdCamDirty;
    int32_t  xrBdPushTransform = CyberpunkVR_BdPushTransform;
    int32_t  xrBdMainPosScene = CyberpunkVR_BdMainPosFromScene;
    int32_t  xrBdPushBase = CyberpunkVR_BdPushBase;
    int32_t  xrBdOneComp = CyberpunkVR_BdOneComposition;
    int32_t  xrBdEditorAlign = CyberpunkVR_BdEditorAlign;
    int32_t  xrBdBaseFromLocate = CyberpunkVR_BdBaseFromLocate;
    uint32_t xrCompLendSet = CyberpunkVR_CompLendSet;
    int32_t  xrFinalGrab = CyberpunkVR_FinalGrab;
    int32_t  xrStableSrgbView = CyberpunkVR_StableSrgbView;
    int32_t  xrBdQuatWriteSite = CyberpunkVR_BdQuatFromWriteSite;
    int32_t  xrVrcamPosFromMain = CyberpunkVR_VrcamPosFromMain;
    int32_t  xrDevCamInLocate = CyberpunkVR_DevCamInLocate;
    int32_t  xrLensHeadWrite = CyberpunkVR_LensHeadWrite;
    int32_t  xrInputDefaultInUi = CyberpunkVR_InputDefaultInUi;
    int32_t  xrPopupMagBlockMs = CyberpunkVR_PopupMagBlockMs;
    int32_t  xrDevCamAnyName = CyberpunkVR_DevCamAnyName;
    float    xrDevCamTolM = CyberpunkVR_DevCamTolM;
    int32_t  xrBdIpdInLocate = CyberpunkVR_BdIpdInLocate;
    int32_t  xrBdQuatFromBuffer = CyberpunkVR_BdQuatFromBuffer;
    uint32_t xrCapGrant = CyberpunkVR_CapGrant;
    uint32_t xrForceVrcamCam = CyberpunkVR_ForceVrcamCam;
    uint32_t xrVrcamCamFields = CyberpunkVR_VrcamCamFields;
    uint32_t xrRunViewParams = CyberpunkVR_RunViewParams;
    int32_t  xrExpoProbe = CyberpunkVR_ExpoProbe;
    int32_t  xrIndirectCensus = CyberpunkVR_IndirectCensus;
    int32_t  xrNoStateLies = CyberpunkVR_NoStateLies;
    int32_t  xrStableCopy = CyberpunkVR_StableCopy;
    uint32_t xrDistantReuse = CyberpunkVR_DistantReuseMode;
    uint32_t xrLocalShadowReuse = CyberpunkVR_LocalShadowReuseMode;
    uint32_t xrGiReuse = CyberpunkVR_GiReuseMode;
    uint32_t xrProbeReuse = CyberpunkVR_ProbeReuseMode;
    int32_t  xrExpoMirror = CyberpunkVR_ExpoMirror;
    uint32_t xrExpoFieldMask = CyberpunkVR_ExpoFieldMask;
    uint32_t xrCompLendScoped = CyberpunkVR_CompLendScoped;
    int xrLightContent = CyberpunkVR_LightContent;
    int32_t  xrFixLod = CyberpunkVR_FixLodEnable;
    int      xrHudTo2 = CyberpunkVR_HudToSecondEye;
    int      xrFingerBones = CyberpunkVR_FingerBones;
    int      xrThumbRigOpen = CyberpunkVR_ThumbRigOpen;
    float    xrFingerSpread = CyberpunkVR_FingerSpread;
    int32_t  xrAsym = CyberpunkVR_AsymProjection;
    float    xrAsymGuard = CyberpunkVR_AsymInnerGuard;
    int      xrVisionTo2 = CyberpunkVR_VisionToSecondEye;
    int      xrHudBd  = CyberpunkVR_HudInBraindance;
    uint32_t xrCompGroup = CyberpunkVR_VrcamCompositionGroup;
    float xrTwoHandRadius = CyberpunkVR_TwoHandRadius;
    int   xrTwoHandAim = CyberpunkVR_TwoHandAim;
    float xrTwoHandBreak = CyberpunkVR_TwoHandBreakRadius;
    float xrTwoHandAimRelease = CyberpunkVR_TwoHandAimReleaseMs;
    float xrCarryRadius = CyberpunkVR_CarryGripRadius;
    float xrRifleRise = CyberpunkVR_RifleRiseMul;
    float xrRifleBack = CyberpunkVR_RifleBackMul;
    float xrShotgunRise = CyberpunkVR_ShotgunRiseMul;
    float xrShotgunBack = CyberpunkVR_ShotgunBackMul;
    float xrDownPistol = CyberpunkVR_RecoilDownPistol;
    float xrDownRifle = CyberpunkVR_RecoilDownRifle;
    float xrDownShotgun = CyberpunkVR_RecoilDownShotgun;
    float xrBackCm = CyberpunkVR_HandRecoilBackCm;
    float xrBackMaxCm = CyberpunkVR_HandRecoilBackMaxCm;
    float xrTwoHandBack = CyberpunkVR_TwoHandBackMul;
    float xrSniperRise = CyberpunkVR_SniperRiseMul;
    float xrSniperBack = CyberpunkVR_SniperBackMul;
    float xrRecoilScale = CyberpunkVR_RecoilScale;
    int   xrHaptics = CyberpunkVR_Haptics;
    float xrHapticScale = CyberpunkVR_HapticScale;
    float xrPistolBack = CyberpunkVR_PistolBackMul;
    float xrClimbFrac = CyberpunkVR_RecoilClimbFrac;
    float xrClimbMax = CyberpunkVR_RecoilClimbMaxDeg;
    float xrClimbMs = CyberpunkVR_RecoilClimbMs;
    float xrReturnMs = CyberpunkVR_HandRecoilReturnMs;
    float xrReturnPow = CyberpunkVR_HandRecoilReturnPow;
    float xrScannerSlots[21];
    for (int i = 0; i < 21; ++i) xrScannerSlots[i] = CyberpunkVR_ScannerSlots[i];
    int   xrHandRelFiltered = CyberpunkVR_HandRelToFilteredHead;
    int   xrHandPerFrame    = CyberpunkVR_HandLocatePerFrame;
    auto tryParseIntKey = [](const char* text, const char* key, int* outValue) {
        if (!text || !key || !outValue) return false;
        const size_t keyLen = strlen(key);
        if (_strnicmp(text, key, keyLen) != 0) return false;
        const char* cursor = text + keyLen;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        if (*cursor != '=') return false;
        ++cursor;
        while (*cursor == ' ' || *cursor == '\t') ++cursor;
        *outValue = atoi(cursor);
        return true;
    };
    float xrSnapTurnAngleDeg = g_liveControls.xrSnapTurnAngleDeg > 0.0f ? g_liveControls.xrSnapTurnAngleDeg : 30.0f;
    int xrMovementSource = g_liveControls.xrMovementSource;
    int xrMovementSpeedMode=0;
    float xrLeftStickDeadzone=.15f,xrRightStickDeadzone=.15f,xrMaxInputThreshold=.90f;
    int xrXInputInstall = g_liveControls.xrXInputInstall;
    int xrInputActions = g_liveControls.xrInputActions;
    int xrMonoXQueueWait = g_liveControls.xrMonoXQueueWait;
    int xrSnapTurnPulseMs = g_liveControls.xrSnapTurnPulseMs > 0 ? g_liveControls.xrSnapTurnPulseMs : 30;
    int xrMonoDepthCapture = g_liveControls.xrMonoDepthCapture;
    int xrSnapTurnYawIndex = g_liveControls.xrSnapTurnYawIndex >= 0 && g_liveControls.xrSnapTurnYawIndex <= 3 ? g_liveControls.xrSnapTurnYawIndex : 1;
    int xrImmersiveHolsters = g_liveControls.xrImmersiveHolsters;
    int xrPhysicalBodyRotation = g_liveControls.xrPhysicalBodyRotation;
    int xrTrackedBodyRotation = g_liveControls.xrTrackedBodyRotation;
    int xrHybridBodyRotation = g_liveControls.xrHybridBodyRotation;
    int xrRoomscaleMovement = g_liveControls.xrRoomscaleMovement;
    int xrPhysicalCrouch = g_liveControls.xrPhysicalCrouch;
    float xrPhysicalCrouchDepth = g_liveControls.xrPhysicalCrouchDepth;
    int xrBreaststrokeSwim = g_liveControls.xrBreaststrokeSwim;
    int xrLadderGripClimb = g_liveControls.xrLadderGripClimb;
    int xrLadderAutoFinish = g_liveControls.xrLadderAutoFinish;
    float xrLadderFinishDistance = g_liveControls.xrLadderFinishDistance;
    float xrBodyFreeLookDeg = g_liveControls.xrBodyFreeLookDeg;
    float xrBodyFreeLookDownDeg = g_liveControls.xrBodyFreeLookDownDeg;
    float xrBodyFreeLookSwimDeg = g_liveControls.xrBodyFreeLookSwimDeg;
    float xrBodyMoveRadius = g_liveControls.xrBodyMoveRadius;
    int xrCutsceneSuspendTier = g_liveControls.xrCutsceneSuspendTier;
    int xrVehicleCutsceneSuspendTier = g_liveControls.xrVehicleCutsceneSuspendTier;
    bool vehicleCutsceneTierConfigured = false;
    float xrVehHeadOffsetX = g_liveControls.xrVehHeadOffsetX;
    float xrVehHeadOffsetY = g_liveControls.xrVehHeadOffsetY;
    float xrVehHeadOffsetZ = g_liveControls.xrVehHeadOffsetZ;
    int xrWheelGrab = g_liveControls.xrWheelGrab;
    float xrWheelRadius = g_liveControls.xrWheelRadius > 0.0f ? g_liveControls.xrWheelRadius : 0.28f;
    float xrWheelSteerMaxDeg = cvr::anim::WheelFullLockDegrees(g_liveControls.xrWheelSteerMaxDeg);
    float xrWheelSteerDeadDeg = g_liveControls.xrWheelSteerDeadDeg >= 0.0f ? g_liveControls.xrWheelSteerDeadDeg : 1.5f;
    int xrWheelPrediction = g_liveControls.xrWheelPrediction;
    float xrWheelPredictionMs = g_liveControls.xrWheelPredictionMs;
    int xrWheelHorn = g_liveControls.xrWheelHorn;
    float xrWheelHornRadius = g_liveControls.xrWheelHornRadius > 0.0f ? g_liveControls.xrWheelHornRadius : 0.12f;
    int xrVehicleGunTrigger = g_liveControls.xrVehicleGunTrigger;
    float xrVehicleThrottleTrim = g_liveControls.xrVehicleThrottleTrim > 0.0f ? g_liveControls.xrVehicleThrottleTrim : 0.5f;

    FILE* file = _fsopen(g_liveControlPath, "r", _SH_DENYNO);
    if (!file) return;

    char line[128];
    while (fgets(line, sizeof(line), file)) {
        float value = 0.0f;

        if (sscanf_s(line, "xr_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_x = %f", &value) == 1) {
            xrHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_y = %f", &value) == 1) {
            xrHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_head_offset_z = %f", &value) == 1) {
            xrHeadOffsetZ = value;
            continue;
        }
        int intValue = 0;
        if (sscanf_s(line, "xr_recenter=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_recenter = %d", &intValue) == 1) {
            xrRecenter = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_submit = %d", &intValue) == 1) {
            xrMonoSubmit = intValue;
            continue;
        }
        // -1 auto (thread on SteamVR, inline on Virtual Desktop), 0 inline, 1 thread.
        if (sscanf_s(line, "xr_threaded_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_threaded_submit = %d", &intValue) == 1) {
            xrThreadedSubmit = (intValue < 0) ? -1 : (intValue > 0 ? 1 : 0);
            continue;
        }

        if (sscanf_s(line, "xr_window_width=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_width = %d", &intValue) == 1) {
            xrWindowWidth = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_window_height=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_window_height = %d", &intValue) == 1) {
            xrWindowHeight = intValue;
            continue;
        }

        if (sscanf_s(line, "xr_force_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_force_fov = %f", &value) == 1) {
            xrForceFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_rect=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_menu_rect = %d", &intValue) == 1) {
            xrMenuRect = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_menu_fov=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_fov = %f", &value) == 1) {
            xrMenuFov = value;
            continue;
        }
        if (sscanf_s(line, "xr_menu_follow_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_menu_follow_deg = %f", &value) == 1) {
            xrMenuFollowDeg = value;
            continue;
        }
        if (cvr::hud::ParseLayoutSetting(line,hudElements)) continue;
        if (cvr::framegen::ParseSetting(line,framegen)) continue;
        if (cvr::foveated::ParseSetting(line,foveated)) continue;
        if (cvr::vrs::ParseSetting(line,vrs)) continue;
        if (cvr::hidden_area::ParseSetting(line,hiddenArea)) continue;
        if(sscanf_s(line,"xr_mono_cascades = %d",&intValue)==1){monoCascades=intValue!=0;continue;}
        if(sscanf_s(line,"xr_mono_mode = %d",&intValue)==1){monoMode=intValue!=0;continue;}
        if(sscanf_s(line,"xr_scope_stabilize = %d",&intValue)==1){scopeStabilize=intValue;continue;}
        if (tryParseIntKey(line,"xr_nvidia_reflex",&intValue)) {nvidiaReflex=cvr::reflex::NormalizeMode(intValue);continue;}
        if (cvr::vrui::ParseSetting(line,overlay)) continue;
        if (sscanf_s(line,"xr_hud_follow_mode = %d",&intValue)==1) { xrHudFollowMode=intValue;continue; }
        if (sscanf_s(line,"xr_hud_stereo_depth = %d",&intValue)==1) { xrHudStereoDepth=intValue;continue; }
        if (sscanf_s(line,"xr_hud_brightness = %f",&value)==1) { xrHudBrightness=value;continue; }
        if (sscanf_s(line,"xr_hud_shadow = %f",&value)==1) { xrHudShadow=value;continue; }
        if (sscanf_s(line,"xr_hud_glow = %f",&value)==1) { xrHudGlow=value;continue; }
        if (sscanf_s(line, "xr_hud_panel = %d", &intValue) == 1) { xrHudPanel=intValue; continue; }
        if (sscanf_s(line, "xr_hud_follow_deg = %f", &value) == 1) { xrHudFollowDeg=value; continue; }
        if (sscanf_s(line, "xr_hud_fov = %f", &value) == 1) { xrHudFov=value; continue; }
        if (sscanf_s(line, "xr_hud_distance = %f", &value) == 1) { xrHudDistance=value; continue; }
        if (sscanf_s(line,"xr_interaction_panel = %d",&intValue)==1){xrInteractionPanel=intValue;continue;}
        if (sscanf_s(line,"xr_interaction_follow_deg = %f",&value)==1){xrInteractionFollowDeg=value;continue;}
        if (sscanf_s(line,"xr_loot_follow_deg = %f",&value)==1){xrLootFollowDeg=value;continue;}
        if (sscanf_s(line,"xr_interaction_distance = %f",&value)==1){xrInteractionDistance=value;continue;}
        if (sscanf_s(line,"xr_interaction_fov = %f",&value)==1){xrInteractionFov=value;continue;}
        if (sscanf_s(line, "xr_pitch_sign=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_sign = %f", &value) == 1) {
            xrPitchSign = value < 0.0f ? -1.0f : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_pitch_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_pitch_scale = %f", &value) == 1) {
            xrPitchScale = value > 0.01f ? value : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_sync_sequential=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_sync_sequential = %d", &intValue) == 1) {
            xrSyncSequential = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_3dof_movement=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_3dof_movement = %d", &intValue) == 1) {
            xr3DofMovement = intValue;
            continue;
        }
        if (sscanf_s(line, "first_launch=%d", &intValue) == 1 ||
            sscanf_s(line, "first_launch = %d", &intValue) == 1) {
            xrFirstLaunch = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_motion_predict_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_motion_predict_ms = %f", &value) == 1) {
            xrMotionPredictMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_stereo_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_stereo_scale = %f", &value) == 1) {
            xrStereoScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_world_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_world_scale = %f", &value) == 1) {
            xrWorldScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_ipd_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_ipd_scale = %f", &value) == 1) {
            xrIpdScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpness=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpness = %f", &value) == 1) {
            xrSharpness = value;
            continue;
        }
        if (sscanf_s(line, "xr_sharpmix=%f", &value) == 1 ||
            sscanf_s(line, "xr_sharpmix = %f", &value) == 1) {
            xrSharpmix = value;
            continue;
        }
        if (sscanf_s(line, "xr_hmd_smooth=%f", &value) == 1 ||
            sscanf_s(line, "xr_hmd_smooth = %f", &value) == 1) {
            xrHmdSmooth = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_per_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hand_per_frame = %d", &intValue) == 1) {
            xrHandPerFrame = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hand_rel_filtered=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hand_rel_filtered = %d", &intValue) == 1) {
            xrHandRelFiltered = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_pistol=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_pistol = %f", &value) == 1) {
            xrLerpPistol = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_rifle=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_rifle = %f", &value) == 1) {
            xrLerpRifle = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_shotgun=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_shotgun = %f", &value) == 1) {
            xrLerpShotgun = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_sniper=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_sniper = %f", &value) == 1) {
            xrLerpSniper = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_melee=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_melee = %f", &value) == 1) {
            xrLerpMelee = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_speed=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_speed = %f", &value) == 1) {
            xrHandLerp = value;
            continue;
        }
        if (sscanf_s(line, "xr_hand_lerp_framegen_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_hand_lerp_framegen_scale = %f", &value) == 1) {
            xrLerpFramegen = value;
            continue;
        }
        // Three numbers on one line per piece, because twelve keys for one feature is a wall of ini
        // nobody can read. The editor writes them back in exactly this shape.
        {
            float sx = 0.0f, sy = 0.0f, ss = 0.0f;
            int slot = -1;
            if      (sscanf_s(line, "xr_scanner_frame=%f,%f,%f",   &sx, &sy, &ss) == 3) slot = 0;
            else if (sscanf_s(line, "xr_scanner_details=%f,%f,%f", &sx, &sy, &ss) == 3) slot = 1;
            else if (sscanf_s(line, "xr_scanner_hacks=%f,%f,%f",   &sx, &sy, &ss) == 3) slot = 2;
            else if (sscanf_s(line, "xr_scanner_hint=%f,%f,%f",    &sx, &sy, &ss) == 3) slot = 3;
            else if (sscanf_s(line, "xr_scanner_memory=%f,%f,%f",  &sx, &sy, &ss) == 3) slot = 4;
            else if (sscanf_s(line, "xr_scanner_scripts=%f,%f,%f", &sx, &sy, &ss) == 3) slot = 5;
            else if (sscanf_s(line, "xr_scanner_desc=%f,%f,%f",    &sx, &sy, &ss) == 3) slot = 6;
            if (slot >= 0) {
                xrScannerSlots[slot * 3 + 0] = sx;
                xrScannerSlots[slot * 3 + 1] = sy;
                xrScannerSlots[slot * 3 + 2] = ss;
                continue;
            }
        }
        if (sscanf_s(line, "xr_vrcam_composition=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_composition = %d", &intValue) == 1) {
            xrCompGroup = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_grip_from_fingers=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_grip_from_fingers = %d", &intValue) == 1) {
            g_gripFromFingersSetting = intValue == 1 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_finger_spread=%f", &value) == 1 ||
            sscanf_s(line, "xr_finger_spread = %f", &value) == 1) {
            xrFingerSpread = std::isfinite(value) ? std::clamp(value, 0.0f, 2.0f) : 1.0f;
            continue;
        }
        if (sscanf_s(line, "xr_thumb_rig_open=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_thumb_rig_open = %d", &intValue) == 1) {
            xrThumbRigOpen = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_finger_bones=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_finger_bones = %d", &intValue) == 1) {
            xrFingerBones = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_hud_to_second_eye=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hud_to_second_eye = %d", &intValue) == 1) {
            xrHudTo2 = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_asym_projection=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_asym_projection = %d", &intValue) == 1) {
            xrAsym = intValue == 1 ? 1 : 0;
            continue;
        }
        {
            float g = 0.0f;
            if (sscanf_s(line, "xr_asym_inner_guard=%f", &g) == 1 || sscanf_s(line, "xr_asym_inner_guard = %f", &g) == 1) {
                xrAsymGuard = (g < 0.0f) ? 0.0f : ((g > 1.0f) ? 1.0f : g);
                continue;
            }
        }
        if (sscanf_s(line, "xr_hud_in_braindance=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_hud_in_braindance = %d", &intValue) == 1) {
            xrHudBd = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_fix_lod=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_fix_lod = %d", &intValue) == 1) {
            xrFixLod = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_comp_lend_scoped=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_comp_lend_scoped = %d", &intValue) == 1) {
            xrCompLendScoped = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue & 0xF));
            continue;
        }
        if (sscanf_s(line, "xr_expo_mirror=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_expo_mirror = %d", &intValue) == 1) {
            xrExpoMirror = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_expo_field_mask=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_expo_field_mask = %d", &intValue) == 1) {
            xrExpoFieldMask = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue & 0x7F));
            continue;
        }
        if (sscanf_s(line, "xr_distant_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_distant_reuse = %d", &intValue) == 1) {
            xrDistantReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_local_shadow_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_local_shadow_reuse = %d", &intValue) == 1) {
            xrLocalShadowReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_gi_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_gi_reuse = %d", &intValue) == 1) {
            xrGiReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_probe_reuse=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_probe_reuse = %d", &intValue) == 1) {
            xrProbeReuse = (intValue < 0) ? 0u : static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_stable_copy=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_stable_copy = %d", &intValue) == 1) {
            xrStableCopy = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_no_state_lies=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_no_state_lies = %d", &intValue) == 1) {
            xrNoStateLies = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_indirect_census=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_indirect_census = %d", &intValue) == 1) {
            xrIndirectCensus = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_expo_probe=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_expo_probe = %d", &intValue) == 1) {
            xrExpoProbe = (intValue < 0) ? 0 : ((intValue > 2) ? 2 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_run_view_params=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_run_view_params = %d", &intValue) == 1) {
            xrRunViewParams = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue > 2 ? 2 : intValue));
            continue;
        }
        if (sscanf_s(line, "xr_vrcam_cam_fields=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_cam_fields = %x", &intValue) == 1) {
            xrVrcamCamFields = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue & 0xF));
            continue;
        }
        if (sscanf_s(line, "xr_force_vrcam_cam=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_force_vrcam_cam = %d", &intValue) == 1) {
            xrForceVrcamCam = intValue ? 1u : 0u;
            continue;
        }
        if (sscanf_s(line, "xr_cap_grant=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_cap_grant = %d", &intValue) == 1) {
            xrCapGrant = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_light_content=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_light_content = %d", &intValue) == 1) {
            xrLightContent = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_quat_from_buffer=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_quat_from_buffer = %d", &intValue) == 1) {
            xrBdQuatFromBuffer = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_bd_ipd_in_locate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_ipd_in_locate = %d", &intValue) == 1) {
            xrBdIpdInLocate = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_dev_cam_tol_m=%f", &value) == 1 ||
            sscanf_s(line, "xr_dev_cam_tol_m = %f", &value) == 1) {
            xrDevCamTolM = value;
            continue;
        }
        if (sscanf_s(line, "xr_dev_cam_any_name=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dev_cam_any_name = %d", &intValue) == 1) {
            xrDevCamAnyName = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_input_default_in_ui=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_input_default_in_ui = %d", &intValue) == 1) {
            xrInputDefaultInUi = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_popup_mag_block_ms=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_popup_mag_block_ms = %d", &intValue) == 1) {
            xrPopupMagBlockMs = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_lens_head_write=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_lens_head_write = %d", &intValue) == 1) {
            xrLensHeadWrite = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_dev_cam_in_locate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_dev_cam_in_locate = %d", &intValue) == 1) {
            xrDevCamInLocate = intValue;
            continue;
        }
        {
            unsigned int visCsNode = 0;
            if (sscanf_s(line, "xr_vision_cs_node=%x", &visCsNode) == 1 ||
                sscanf_s(line, "xr_vision_cs_node = %x", &visCsNode) == 1) {
                CyberpunkVR_VisionCsNode = visCsNode;
                continue;
            }
        }
        if (sscanf_s(line, "xr_vision_cs_ord=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_cs_ord = %d", &intValue) == 1) {
            CyberpunkVR_VisionCsOrd = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vision_cs_dim=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_cs_dim = %d", &intValue) == 1) {
            CyberpunkVR_VisionCsDim = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vision_cs_swap=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_cs_swap = %d", &intValue) == 1) {
            CyberpunkVR_VisionCsSwap = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vision_to_second_eye=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vision_to_second_eye = %d", &intValue) == 1) {
            xrVisionTo2 = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_weapon_plane_as_scene=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_weapon_plane_as_scene = %d", &intValue) == 1) {
            CyberpunkVR_WeaponPlaneAsScene = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_highlight_any_plane=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_highlight_any_plane = %d", &intValue) == 1) {
            CyberpunkVR_HighlightAnyPlane = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_plane_bucket_scene=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_plane_bucket_scene = %d", &intValue) == 1) {
            CyberpunkVR_PlaneBucketScene = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vrcam_pos_from_main=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_pos_from_main = %d", &intValue) == 1) {
            xrVrcamPosFromMain = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_bd_quat_from_write_site=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_quat_from_write_site = %d", &intValue) == 1) {
            xrBdQuatWriteSite = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_stable_srgb_view=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_stable_srgb_view = %d", &intValue) == 1) {
            xrStableSrgbView = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_final_grab=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_final_grab = %d", &intValue) == 1) {
            xrFinalGrab = intValue ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_comp_lend_set=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_comp_lend_set = %d", &intValue) == 1) {
            xrCompLendSet = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_bd_base_from_locate=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_base_from_locate = %d", &intValue) == 1) {
            xrBdBaseFromLocate = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_editor_align=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_editor_align = %d", &intValue) == 1) {
            xrBdEditorAlign = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_one_composition=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_one_composition = %d", &intValue) == 1) {
            xrBdOneComp = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_push_base=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_push_base = %d", &intValue) == 1) {
            xrBdPushBase = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_main_pos_from_scene=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_main_pos_from_scene = %d", &intValue) == 1) {
            xrBdMainPosScene = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_bd_push_transform=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_push_transform = %d", &intValue) == 1) {
            if (intValue >= 0 && intValue <= 3) xrBdPushTransform = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_bd_cam_dirty=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_bd_cam_dirty = %d", &intValue) == 1) {
            if (intValue >= 0 && intValue <= 3) xrBdCamDirty = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_env_mirror_mask=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_env_mirror_mask = %x", &intValue) == 1) {
            xrEnvMirrorMask = static_cast<uint32_t>(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_viewdata_fix_mask=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_viewdata_fix_mask = %x", &intValue) == 1) {
            xrViewDataFixMask = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_render_mask_grant=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_render_mask_grant = %d", &intValue) == 1) {
            xrRenderMaskGrant = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_stage_replay=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_stage_replay = %d", &intValue) == 1) {
            xrStageReplay = intValue != 0;
            continue;
        }
        if (sscanf_s(line, "xr_vrcam_gbuffer_partitions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vrcam_gbuffer_partitions = %d", &intValue) == 1) {
            xrGbufferPartitions = intValue != 0;
            continue;
        }
        if (sscanf_s(line, "xr_submit_chain=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_submit_chain = %d", &intValue) == 1) {
            xrSubmitChain = intValue < 0 ? 0 : intValue > 2 ? 2 : intValue;
            continue;
        }
        if (sscanf_s(line, "xr_defer_present=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_defer_present = %d", &intValue) == 1) {
            xrDeferPresent = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_gt_skip_render_queue=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_gt_skip_render_queue = %d", &intValue) == 1) {
            xrGtSkipRenderQueue = intValue != 0 ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_water_mirror=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_water_mirror = %d", &intValue) == 1) {
            xrWaterMirror = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
        }
        if (sscanf_s(line, "xr_water_assign=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_water_assign = %d", &intValue) == 1) {
            xrWaterAssign = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue > 1 ? 1 : intValue));
        }
        if (sscanf_s(line, "xr_viewdata_diff=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_viewdata_diff = %d", &intValue) == 1) {
            xrViewDataDiff = intValue < 0 ? 0 : (intValue > 2 ? 2 : intValue);
        }
        // THE NODE IS HEX, because every RVA in this project is written and read as hex and a node
        // key that silently took decimal would aim the diff at an address nobody meant.
        if (sscanf_s(line, "xr_scene_stick_yaw_dps=%f", &value) == 1 ||
            sscanf_s(line, "xr_scene_stick_yaw_dps = %f", &value) == 1) {
            xrSceneYawDps = value < 0.0f ? 0.0f : (value > 360.0f ? 360.0f : value);
            continue;
        }
        if (sscanf_s(line, "xr_scene_stick_yaw_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_scene_stick_yaw_tier = %d", &intValue) == 1) {
            xrSceneYawTier = intValue < 0 ? 0 : (intValue > 4 ? 4 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_lut_src_lend=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_lut_src_lend = %d", &intValue) == 1) {
            xrLutSrcLend = static_cast<uint32_t>(intValue < 0 ? 0 : (intValue > 1 ? 1 : intValue));
        }
        if (sscanf_s(line, "xr_block_diff=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_block_diff = %d", &intValue) == 1) {
            xrBlockDiff = intValue < 0 ? 0 : (intValue > 2 ? 2 : intValue);
        }
        if (sscanf_s(line, "xr_block_diff_node=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_block_diff_node = %x", &intValue) == 1) {
            xrBlockDiffNode = static_cast<uint32_t>(intValue);
        }
        if (sscanf_s(line, "xr_block_diff_size=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_block_diff_size = %d", &intValue) == 1) {
            xrBlockDiffSize = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
        }
        if (sscanf_s(line, "xr_env_extra_mask=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_env_extra_mask = %d", &intValue) == 1) {
            xrEnvExtraMask = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        // HEX, LIKE ITS SIBLING xr_env_mirror_mask. It was %d while the value is a bit mask that
        // everyone -- the table, the comments, this session -- writes in hex, so "7FE0003" parsed as
        // 7 and stopped at the F. A measurement was reported on the strength of that, and the number
        // that produced it was never the number in the file.
        if (sscanf_s(line, "xr_grade_mirror_mask=%x", &intValue) == 1 ||
            sscanf_s(line, "xr_grade_mirror_mask = %x", &intValue) == 1) {
            xrGradeMirrorMask = static_cast<uint32_t>(intValue < 0 ? 0 : intValue);
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_radius = %f", &value) == 1) {
            xrTwoHandRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_aim=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_two_hand_aim = %d", &intValue) == 1) {
            xrTwoHandAim = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_break=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_break = %f", &value) == 1) {
            xrTwoHandBreak = value;
            continue;
        }
        if (sscanf_s(line, "xr_two_hand_aim_release_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_two_hand_aim_release_ms = %f", &value) == 1) {
            xrTwoHandAimRelease = value;
            continue;
        }
        if (sscanf_s(line, "xr_carry_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_carry_radius = %f", &value) == 1) {
            xrCarryRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_rifle_rise=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_rifle_rise = %f", &value) == 1) {
            xrRifleRise = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_rifle_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_rifle_back = %f", &value) == 1) {
            xrRifleBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_shotgun_rise=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_shotgun_rise = %f", &value) == 1) {
            xrShotgunRise = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_shotgun_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_shotgun_back = %f", &value) == 1) {
            xrShotgunBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_down_pistol=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_down_pistol = %f", &value) == 1) {
            xrDownPistol = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_down_rifle=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_down_rifle = %f", &value) == 1) {
            xrDownRifle = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_down_shotgun=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_down_shotgun = %f", &value) == 1) {
            xrDownShotgun = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_back_cm=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_back_cm = %f", &value) == 1) {
            xrBackCm = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_back_max_cm=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_back_max_cm = %f", &value) == 1) {
            xrBackMaxCm = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_twohand_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_twohand_back = %f", &value) == 1) {
            xrTwoHandBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_sniper_rise=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_sniper_rise = %f", &value) == 1) {
            xrSniperRise = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_sniper_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_sniper_back = %f", &value) == 1) {
            xrSniperBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_scale = %f", &value) == 1) {
            xrRecoilScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_haptics=%f", &value) == 1 ||
            sscanf_s(line, "xr_haptics = %f", &value) == 1) {
            xrHaptics = value != 0.0f ? 1 : 0;
            continue;
        }
        if (sscanf_s(line, "xr_haptic_scale=%f", &value) == 1 ||
            sscanf_s(line, "xr_haptic_scale = %f", &value) == 1) {
            xrHapticScale = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_pistol_back=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_pistol_back = %f", &value) == 1) {
            xrPistolBack = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_climb=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_climb = %f", &value) == 1) {
            xrClimbFrac = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_climb_max_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_climb_max_deg = %f", &value) == 1) {
            xrClimbMax = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_climb_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_climb_ms = %f", &value) == 1) {
            xrClimbMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_return_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_return_ms = %f", &value) == 1) {
            xrReturnMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_recoil_return_pow=%f", &value) == 1 ||
            sscanf_s(line, "xr_recoil_return_pow = %f", &value) == 1) {
            xrReturnPow = value;
            continue;
        }
        if (sscanf_s(line, "xr_render_pose_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_render_pose_submit = %d", &intValue) == 1) {
            xrRenderPoseSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_reuse_last_frame=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_reuse_last_frame = %d", &intValue) == 1) {
            xrReuseLastFrame = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pair_lock=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pair_lock = %d", &intValue) == 1) {
            xrPairLock = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_pose_lag=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_pose_lag = %d", &intValue) == 1) {
            xrPoseLag = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_runtime=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_runtime = %d", &intValue) == 1) {
            xrRuntime = ClampRuntimeMode(intValue);
            continue;
        }
        if (sscanf_s(line, "xr_depth_submit=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_depth_submit = %d", &intValue) == 1) {
            xrDepthSubmit = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_movement_control=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_control = %d", &intValue) == 1) {
            xrMovementControl = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_disable_mouse_y=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_disable_mouse_y = %d", &intValue) == 1) {
            xrDisableMouseY = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_hook=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_hook = %d", &intValue) == 1) {
            xrXInputHook = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn = %d", &intValue) == 1) {
            xrSnapTurn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_angle_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_snap_turn_angle_deg = %f", &value) == 1) {
            xrSnapTurnAngleDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_movement_source=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_movement_source = %d", &intValue) == 1) {
            xrMovementSource = intValue;
            continue;
        }
        if(sscanf_s(line,"xr_movement_speed_mode = %d",&intValue)==1){xrMovementSpeedMode=intValue;continue;}
        if(sscanf_s(line,"xr_left_stick_deadzone = %f",&value)==1){xrLeftStickDeadzone=value;continue;}
        if(sscanf_s(line,"xr_right_stick_deadzone = %f",&value)==1){xrRightStickDeadzone=value;continue;}
        if(sscanf_s(line,"xr_max_input_threshold = %f",&value)==1){xrMaxInputThreshold=value;continue;}
        if (sscanf_s(line, "xr_cutscene_suspend_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_cutscene_suspend_tier = %d", &intValue) == 1) {
            xrCutsceneSuspendTier = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_cutscene_suspend_tier=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vehicle_cutscene_suspend_tier = %d", &intValue) == 1) {
            xrVehicleCutsceneSuspendTier = intValue;
            vehicleCutsceneTierConfigured = true;
            continue;
        }
        if (sscanf_s(line, "xr_body_free_look_deg=%f", &value) == 1) xrBodyFreeLookDeg=value;
        if (sscanf_s(line, "xr_body_free_look_down_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_body_free_look_down_deg = %f", &value) == 1) xrBodyFreeLookDownDeg=value;
        if (sscanf_s(line, "xr_body_free_look_swim_deg=%f", &value) == 1) xrBodyFreeLookSwimDeg=value;
        if (sscanf_s(line, "xr_body_move_radius=%f", &value) == 1) xrBodyMoveRadius=value;
        if (sscanf_s(line, "xr_physical_crouch=%d", &intValue) == 1) xrPhysicalCrouch=intValue;
        if (sscanf_s(line, "xr_physical_crouch_depth=%f", &value) == 1) xrPhysicalCrouchDepth=value;
        if (sscanf_s(line, "xr_ladder_grip_climb=%d", &intValue) == 1) xrLadderGripClimb=intValue;
        if (sscanf_s(line, "xr_ladder_auto_finish=%d", &intValue) == 1) xrLadderAutoFinish=intValue;
        if (sscanf_s(line, "xr_ladder_finish_distance=%f", &value) == 1) xrLadderFinishDistance=value;
        if (sscanf_s(line, "xr_breaststroke_swim=%d", &intValue) == 1) {
            xrBreaststrokeSwim=intValue;
            continue;
        }
        if (sscanf_s(line, "xr_roomscale_movement=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_roomscale_movement = %d", &intValue) == 1) {
            xrRoomscaleMovement = intValue;
        }
        if (sscanf_s(line, "xr_tracked_body_rotation = %d", &intValue) == 1) xrTrackedBodyRotation=intValue;
        if (sscanf_s(line, "xr_hybrid_body_rotation = %d", &intValue) == 1) xrHybridBodyRotation=intValue;
        if (sscanf_s(line, "xr_physical_body_rotation=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_physical_body_rotation = %d", &intValue) == 1) {
            xrPhysicalBodyRotation = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_xinput_install=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_xinput_install = %d", &intValue) == 1) {
            xrXInputInstall = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_input_actions=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_input_actions = %d", &intValue) == 1) {
            xrInputActions = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_xqueue_wait=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_xqueue_wait = %d", &intValue) == 1) {
            xrMonoXQueueWait = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_pulse_ms=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_pulse_ms = %d", &intValue) == 1) {
            xrSnapTurnPulseMs = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_mono_depth_capture=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_mono_depth_capture = %d", &intValue) == 1) {
            xrMonoDepthCapture = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_snap_turn_yaw_index=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_snap_turn_yaw_index = %d", &intValue) == 1) {
            xrSnapTurnYawIndex = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_immersive_holsters=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_immersive_holsters = %d", &intValue) == 1) {
            xrImmersiveHolsters = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_x=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_x = %f", &value) == 1) {
            xrVehHeadOffsetX = value;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_y=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_y = %f", &value) == 1) {
            xrVehHeadOffsetY = value;
            continue;
        }
        if (sscanf_s(line, "xr_veh_head_offset_z=%f", &value) == 1 ||
            sscanf_s(line, "xr_veh_head_offset_z = %f", &value) == 1) {
            xrVehHeadOffsetZ = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_grab=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_grab = %d", &intValue) == 1) {
            xrWheelGrab = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_radius = %f", &value) == 1) {
            xrWheelRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_steer_max_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_steer_max_deg = %f", &value) == 1) {
            xrWheelSteerMaxDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_steer_dead_deg=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_steer_dead_deg = %f", &value) == 1) {
            xrWheelSteerDeadDeg = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_prediction=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_prediction = %d", &intValue) == 1) {
            xrWheelPrediction = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_prediction_ms=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_prediction_ms = %f", &value) == 1) {
            xrWheelPredictionMs = value;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_horn=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_wheel_horn = %d", &intValue) == 1) {
            xrWheelHorn = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_wheel_horn_radius=%f", &value) == 1 ||
            sscanf_s(line, "xr_wheel_horn_radius = %f", &value) == 1) {
            xrWheelHornRadius = value;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_gun_trigger=%d", &intValue) == 1 ||
            sscanf_s(line, "xr_vehicle_gun_trigger = %d", &intValue) == 1) {
            xrVehicleGunTrigger = intValue;
            continue;
        }
        if (sscanf_s(line, "xr_vehicle_throttle_trim=%f", &value) == 1 ||
            sscanf_s(line, "xr_vehicle_throttle_trim = %f", &value) == 1) {
            xrVehicleThrottleTrim = value;
            continue;
        }

    }
    fclose(file);

    const int prevXrRecenter = g_liveControls.xrRecenter;
    const int prevXrMonoSubmit = g_liveControls.xrMonoSubmit;
    const bool changed = g_liveControls.xrHeadOffsetX != xrHeadOffsetX ||
        g_liveControls.xrHeadOffsetY != xrHeadOffsetY ||
        g_liveControls.xrHeadOffsetZ != xrHeadOffsetZ ||
        g_liveControls.xrRecenter != xrRecenter ||
        g_liveControls.xrMonoSubmit != xrMonoSubmit ||
        g_liveControls.xrForceFov != xrForceFov ||
        g_liveControls.xrMenuRect != xrMenuRect ||
        g_liveControls.xrMenuFov != xrMenuFov ||
        g_liveControls.xrMenuFollowDeg != xrMenuFollowDeg ||
        g_liveControls.xrHudPanel != xrHudPanel ||
        g_liveControls.xrHudFollowDeg != xrHudFollowDeg ||
        g_liveControls.xrHudFov != xrHudFov ||
        g_liveControls.xrHudDistance != xrHudDistance ||
        g_liveControls.xr3DofMovement != xr3DofMovement ||
        g_liveControls.xrFirstLaunch != xrFirstLaunch ||
        g_liveControls.xrMotionPredictMs != xrMotionPredictMs ||
        g_liveControls.xrStereoScale != xrStereoScale ||
        g_liveControls.xrWorldScale != xrWorldScale ||
        g_liveControls.xrIpdScale != xrIpdScale ||
        g_liveControls.xrSharpness != xrSharpness ||
        g_liveControls.xrSharpmix != xrSharpmix ||
        g_liveControls.xrReuseLastFrame != xrReuseLastFrame ||
        g_liveControls.xrPairLock != xrPairLock ||
        g_liveControls.xrRenderPoseSubmit != xrRenderPoseSubmit ||
        g_liveControls.xrRuntime != xrRuntime ||
        g_liveControls.xrDepthSubmit != xrDepthSubmit;

    g_liveControls.xrHeadOffsetX = xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = xrHeadOffsetZ;
    g_liveControls.xrRecenter = xrRecenter;
    g_liveControls.xrMonoSubmit = xrMonoSubmit;
    g_liveControls.xrForceFov = xrForceFov;
    g_liveControls.xrMenuRect = xrMenuRect;
    g_liveControls.xrMenuFov = xrMenuFov;
    g_liveControls.xrMenuFollowDeg = xrMenuFollowDeg;
    cvr::framegen::SetSettings(framegen);
    cvr::foveated::SetSettings(foveated);
    cvr::vrs::SetSettings(vrs);
    cvr::hidden_area::SetSettings(hiddenArea);
    CyberpunkVR_MonoCascades.store(monoCascades,std::memory_order_relaxed);
    cvr::engine_stereo::ApplyMonoMode(monoMode!=0);
    cvr::optics::source::SetStabilize(scopeStabilize);
    cvr::reflex::SetMode(nvidiaReflex);
    cvr::vrui::SetSettings(overlay);
    g_liveControls.xrHudPanel=xrHudPanel!=0;
    g_liveControls.xrHudFollowMode=xrHudFollowMode==1;
    g_liveControls.xrHudStereoDepth=xrHudStereoDepth!=0;
    g_liveControls.xrHudBrightness=(xrHudBrightness>=0.25f && xrHudBrightness<=3) ? xrHudBrightness : 1;
    g_liveControls.xrHudShadow=(xrHudShadow>=0 && xrHudShadow<=2) ? xrHudShadow : 1;
    g_liveControls.xrHudGlow=(xrHudGlow>=0 && xrHudGlow<=2) ? xrHudGlow : 1;
    cvr::hud::SetLayoutSettings(hudElements);
    g_liveControls.xrHudFollowDeg=(xrHudFollowDeg>=5 && xrHudFollowDeg<=90) ? xrHudFollowDeg : 60.0f;
    g_liveControls.xrInteractionPanel=xrInteractionPanel!=0;
    g_liveControls.xrInteractionFollowDeg=(xrInteractionFollowDeg>=0 && xrInteractionFollowDeg<=90)?xrInteractionFollowDeg:0.0f;
    g_liveControls.xrLootFollowDeg=(xrLootFollowDeg>=0 && xrLootFollowDeg<=90)?xrLootFollowDeg:0.0f;
    g_liveControls.xrInteractionDistance=(xrInteractionDistance>=.5f && xrInteractionDistance<=5)?xrInteractionDistance:2.0f;
    g_liveControls.xrInteractionFov=(xrInteractionFov>=30 && xrInteractionFov<=120)?xrInteractionFov:75.0f;
    g_liveControls.xrHudFov=(xrHudFov>=30 && xrHudFov<=120) ? xrHudFov : 65.4f;
    g_liveControls.xrHudDistance=(xrHudDistance>=0.5f && xrHudDistance<=5) ? xrHudDistance : 1.5f;
    g_liveControls.xr3DofMovement = xr3DofMovement;
    g_liveControls.xrFirstLaunch = xrFirstLaunch != 0 ? 1 : 0;
    g_liveControls.xrMotionPredictMs = xrMotionPredictMs >= 0.0f ? xrMotionPredictMs : 0.0f;
    g_liveControls.xrStereoScale = xrStereoScale < 0.0f ? 0.0f : (xrStereoScale > 10.0f ? 10.0f : xrStereoScale);
    g_liveControls.xrWorldScale = xrWorldScale < 0.05f ? 0.05f : (xrWorldScale > 20.0f ? 20.0f : xrWorldScale);
    g_liveControls.xrIpdScale = xrIpdScale < 0.0f ? 0.0f : (xrIpdScale > 5.0f ? 5.0f : xrIpdScale);
    g_liveControls.xrSharpness = xrSharpness < 0.0f ? 0.0f : (xrSharpness > 1.0f ? 1.0f : xrSharpness);
    g_liveControls.xrSharpmix = xrSharpmix < 0.0f ? 0.0f : (xrSharpmix > 1.0f ? 1.0f : xrSharpmix);
    g_liveControls.xrReuseLastFrame = xrReuseLastFrame != 0 ? 1 : 0;
    g_liveControls.xrPairLock = xrPairLock != 0 ? 1 : 0;
    g_liveControls.xrRenderPoseSubmit = xrRenderPoseSubmit != 0 ? 1 : 0;
    g_liveControls.xrPoseLag = xrPoseLag;
    g_liveControls.xrRuntime = ClampRuntimeMode(xrRuntime);
    g_liveControls.xrDepthSubmit = xrDepthSubmit != 0 ? 1 : 0;
    // xrMovementSource is the authoritative locomotion mode (0..3); legacy
    // xrMovementControl mirrors it for old configs (0 = Game, anything else
    // means VR-driven so map to legacy 1).
    if (xrMovementSource < 0 || xrMovementSource > 3) xrMovementSource = xrMovementControl != 0 ? 1 : 0;
    g_liveControls.xrMovementSource = xrMovementSource;
    g_liveControls.xrMovementSpeedMode=xrMovementSpeedMode==1;
    g_liveControls.xrLeftStickDeadzone=cvr::input::StickDeadzone(xrLeftStickDeadzone);
    g_liveControls.xrRightStickDeadzone=cvr::input::StickDeadzone(xrRightStickDeadzone);
    g_liveControls.xrMaxInputThreshold=cvr::input::StickFullInput(xrMaxInputThreshold);
    g_liveControls.xrMovementControl = xrMovementSource != 0 ? 1 : 0;
    g_liveControls.xrPhysicalBodyRotation = xrPhysicalBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrTrackedBodyRotation = xrTrackedBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrHybridBodyRotation = xrHybridBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrBodyRotationMode=static_cast<int>(cvr::body::RotationModeFromFlags(
        xrPhysicalBodyRotation,xrTrackedBodyRotation,xrHybridBodyRotation));
    g_liveControls.xrRoomscaleMovement = xrRoomscaleMovement != 0 ? 1 : 0;
    g_liveControls.xrPhysicalCrouch = xrPhysicalCrouch != 0 ? 1 : 0;
    g_liveControls.xrPhysicalCrouchDepth = cvr::body::CrouchThreshold(xrPhysicalCrouchDepth);
    g_liveControls.xrBreaststrokeSwim = xrBreaststrokeSwim != 0 ? 1 : 0;
    g_liveControls.xrLadderGripClimb = xrLadderGripClimb != 0 ? 1 : 0;
    g_liveControls.xrLadderAutoFinish = xrLadderAutoFinish != 0 ? 1 : 0;
    g_liveControls.xrLadderFinishDistance=std::clamp(std::isfinite(xrLadderFinishDistance) ? xrLadderFinishDistance:1.0f,.2f,1.2f);
    g_liveControls.xrBodyFreeLookDeg=std::clamp(xrBodyFreeLookDeg,0.0f,60.0f);
    g_liveControls.xrBodyFreeLookDownDeg=std::clamp(std::isfinite(xrBodyFreeLookDownDeg) ? xrBodyFreeLookDownDeg:30.0f,0.0f,90.0f);
    g_liveControls.xrBodyFreeLookSwimDeg=std::clamp(std::isfinite(xrBodyFreeLookSwimDeg) ? xrBodyFreeLookSwimDeg:5.0f,0.0f,90.0f);
    g_liveControls.xrBodyMoveRadius=std::clamp(xrBodyMoveRadius,0.0f,.30f);
    g_liveControls.xrCutsceneSuspendTier =
        (xrCutsceneSuspendTier < -1) ? -1 : (xrCutsceneSuspendTier > 4 ? 4 : xrCutsceneSuspendTier);
    g_liveControls.xrVehicleCutsceneSuspendTier = cvr::anim::ResolveVehicleVrikSuspendTier(
        xrCutsceneSuspendTier,xrVehicleCutsceneSuspendTier,vehicleCutsceneTierConfigured);
    g_liveControls.xrDisableMouseY = xrDisableMouseY != 0 ? 1 : 0;
    g_liveControls.xrXInputHook = xrXInputHook != 0 ? 1 : 0;
    g_liveControls.xrSnapTurn = xrSnapTurn != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnAngleDeg = xrSnapTurnAngleDeg > 0.0f ? xrSnapTurnAngleDeg : 30.0f;
    g_liveControls.xrXInputInstall = xrXInputInstall != 0 ? 1 : 0;
    g_liveControls.xrInputActions = xrInputActions != 0 ? 1 : 0;
    g_liveControls.xrMonoXQueueWait = xrMonoXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnPulseMs = xrSnapTurnPulseMs > 0 ? xrSnapTurnPulseMs : 30;
    g_liveControls.xrMonoDepthCapture = xrMonoDepthCapture != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnYawIndex = (xrSnapTurnYawIndex >= 0 && xrSnapTurnYawIndex <= 3) ? xrSnapTurnYawIndex : 1;
    g_liveControls.xrImmersiveHolsters = xrImmersiveHolsters != 0 ? 1 : 0;
    OpenXRManager::Get().SetImmersiveHolsters(g_liveControls.xrImmersiveHolsters);
    // Clamped wider than the slider (+/-0.50) but still bounded: the value can arrive from a
    // hand-edited ini, and a metre of head offset is not a setting, it is a typo.
    auto clampVehOff = [](float v) { return (v < -1.0f) ? -1.0f : (v > 1.0f ? 1.0f : v); };
    g_liveControls.xrVehHeadOffsetX = clampVehOff(xrVehHeadOffsetX);
    g_liveControls.xrVehHeadOffsetY = clampVehOff(xrVehHeadOffsetY);
    g_liveControls.xrVehHeadOffsetZ = clampVehOff(xrVehHeadOffsetZ);
    g_liveControls.xrWheelGrab = xrWheelGrab != 0 ? 1 : 0;
    // Clamped, not trusted: these come from a text file. Below ~8 cm the grab is unreachable for a
    // hand you cannot see; above 60 cm every grip in a car is a grab.
    g_liveControls.xrWheelRadius = (xrWheelRadius < 0.08f) ? 0.08f
                                 : (xrWheelRadius > 0.60f ? 0.60f : xrWheelRadius);
    g_liveControls.xrWheelSteerMaxDeg = cvr::anim::WheelFullLockDegrees(xrWheelSteerMaxDeg);
    // The deadzone is capped well under the smallest full-lock angle (30 deg): one that met or passed
    // it would leave no range at all between "dead" and "full lock".
    g_liveControls.xrWheelSteerDeadDeg = (xrWheelSteerDeadDeg < 0.0f) ? 0.0f
                                       : (xrWheelSteerDeadDeg > 20.0f ? 20.0f : xrWheelSteerDeadDeg);
    g_liveControls.xrWheelPrediction = xrWheelPrediction != 0 ? 1 : 0;
    g_liveControls.xrWheelPredictionMs = std::isfinite(xrWheelPredictionMs) ? std::clamp(xrWheelPredictionMs,0.0f,8.0f):0.0f;
    g_liveControls.xrWheelHorn = xrWheelHorn != 0 ? 1 : 0;
    // Below 4 cm the hub is unhittable with a hand you cannot see; above 30 cm it swallows the rim,
    // and every reach for the wheel would honk.
    g_liveControls.xrWheelHornRadius = (xrWheelHornRadius < 0.04f) ? 0.04f
                                     : (xrWheelHornRadius > 0.30f ? 0.30f : xrWheelHornRadius);
    g_liveControls.xrVehicleGunTrigger = xrVehicleGunTrigger != 0 ? 1 : 0;
    g_liveControls.xrVehicleThrottleTrim = (xrVehicleThrottleTrim < 0.05f) ? 0.05f
                                         : (xrVehicleThrottleTrim > 3.0f ? 3.0f : xrVehicleThrottleTrim);
    SetHmdTrackingSmooth(xrHmdSmooth);
    // 100, not 30: melee wants 40 and the old ceiling silently cut the number that was asked for.
    CyberpunkVR_HandLerpSpeed = (xrHandLerp < 0.0f) ? 0.0f : ((xrHandLerp > 100.0f) ? 100.0f : xrHandLerp);
    // Same clamp as the base above; 0 stays 0, which is how a class asks for the base.
    auto clampLerp = [](float v) { return (v < 0.0f) ? 0.0f : ((v > 100.0f) ? 100.0f : v); };
    CyberpunkVR_HandLerpPistol  = clampLerp(xrLerpPistol);
    CyberpunkVR_HandLerpRifle   = clampLerp(xrLerpRifle);
    CyberpunkVR_HandLerpShotgun = clampLerp(xrLerpShotgun);
    CyberpunkVR_HandLerpSniper  = clampLerp(xrLerpSniper);
    CyberpunkVR_HandLerpMelee   = clampLerp(xrLerpMelee);
    CyberpunkVR_HandLerpFramegenScale = std::isfinite(xrLerpFramegen) ? std::clamp(xrLerpFramegen, 0.0f, 4.0f) : 1.5f;
    // Clamped rather than trusted: at 0 the hold can never be offered and at a third of a metre it is
    // offered for a hand nowhere near the weapon, and both read as "the feature is broken".
    // Seventeen candidate bits: 0-7 the original unidentified words, 8-14 the grading values that
    // carry the look, 15-16 the two small integers that are the LUT-selection candidates.
    // Anything above them is dropped rather than reinterpreted.
    // 26 BITS, NOT 17, AND THE NUMBER COMES FROM THE TABLE. kGradeMirrorOff grew from seventeen
    // entries to twenty-six and this clamp did not, so every new bit was cut off before it reached
    // the mirror -- the mask read 3 while the file said 7FE0003, and the test that rested on it was
    // void. Whenever that table grows again, this constant grows with it.
    CyberpunkVR_GradeMirrorMask = xrGradeMirrorMask & 0x3FFFFFFu;
    // Nine slots exist; anything above them is dropped rather than reinterpreted.
    CyberpunkVR_EnvExtraMask = xrEnvExtraMask & 0x1FFFFFu;   // 21 measured object slots
    CyberpunkVR_WaterMirror  = xrWaterMirror & 0xFu;         // 4 groups in kWaterMirror
    CyberpunkVR_WaterAssign  = xrWaterAssign ? 1u : 0u;
    CyberpunkVR_ViewDataDiff = xrViewDataDiff;
    CyberpunkVR_LutSrcLend    = xrLutSrcLend;
    CyberpunkVR_SceneStickYawDps  = xrSceneYawDps;
    CyberpunkVR_SceneStickYawTier = xrSceneYawTier;
    CyberpunkVR_BlockDiff     = xrBlockDiff;
    CyberpunkVR_BlockDiffNode = xrBlockDiffNode;
    CyberpunkVR_BlockDiffSize = xrBlockDiffSize;
    // 27 categories exist; anything above them is dropped rather than reinterpreted.
    CyberpunkVR_RenderMaskGrant = xrRenderMaskGrant & 0x07FFFFFFu;
    CyberpunkVR_StageReplaySetting = xrStageReplay ? 1 : 0;
    if (g_appliedStageReplay != CyberpunkVR_StageReplaySetting) {
        CyberpunkVR_StageReplayConfigure(CyberpunkVR_StageReplaySetting);
        g_appliedStageReplay = CyberpunkVR_StageReplaySetting;
    }
    CyberpunkVR_VrcamGbufferPartitionsSetting = xrGbufferPartitions ? 1 : 0;
    if (g_appliedGbufferPartitions != CyberpunkVR_VrcamGbufferPartitionsSetting) {
        CyberpunkVR_VrcamGbufferPartitionsConfigure(CyberpunkVR_VrcamGbufferPartitionsSetting);
        g_appliedGbufferPartitions = CyberpunkVR_VrcamGbufferPartitionsSetting;
    }
    CyberpunkVR_DeferPresentSetting = xrDeferPresent;
    if (g_appliedDeferPresent != CyberpunkVR_DeferPresentSetting) {
        CyberpunkVR_SubmitChainDeferPresent.store(uint32_t(CyberpunkVR_DeferPresentSetting), std::memory_order_relaxed);
        g_appliedDeferPresent = CyberpunkVR_DeferPresentSetting;
    }
    CyberpunkVR_GtSkipRenderQueueSetting = xrGtSkipRenderQueue;
    if (g_appliedGtSkipRenderQueue != CyberpunkVR_GtSkipRenderQueueSetting) {
        CyberpunkVR_GameThreadSkipRenderQueue.store(uint32_t(CyberpunkVR_GtSkipRenderQueueSetting), std::memory_order_relaxed);
        g_appliedGtSkipRenderQueue = CyberpunkVR_GtSkipRenderQueueSetting;
    }
    CyberpunkVR_SubmitChainSetting = xrSubmitChain;
    if (g_appliedSubmitChain != CyberpunkVR_SubmitChainSetting) {
        CyberpunkVR_SubmitChainConfigure(CyberpunkVR_SubmitChainSetting);
        g_appliedSubmitChain = CyberpunkVR_SubmitChainSetting;
    }
    CyberpunkVR_ViewDataFixMask = xrViewDataFixMask;
    CyberpunkVR_EnvMirrorMask = xrEnvMirrorMask;
    CyberpunkVR_BdCamDirty = xrBdCamDirty;
    CyberpunkVR_BdPushTransform = xrBdPushTransform;
    CyberpunkVR_BdMainPosFromScene = xrBdMainPosScene;
    CyberpunkVR_BdPushBase = xrBdPushBase;
    CyberpunkVR_BdOneComposition = xrBdOneComp;
    CyberpunkVR_BdEditorAlign = xrBdEditorAlign;
    CyberpunkVR_BdBaseFromLocate = xrBdBaseFromLocate;
    CyberpunkVR_CompLendSet = xrCompLendSet;
    CyberpunkVR_FinalGrab = xrFinalGrab;
    CyberpunkVR_StableSrgbView = xrStableSrgbView;
    CyberpunkVR_BdQuatFromWriteSite = xrBdQuatWriteSite;
    // Only the three modes exist; anything else is dropped rather than reinterpreted, which is the
    // clamp this project added after a mode key parsed as a boolean and silently became 1.
    // 0 = off, 1 = the turret's route for an UNCLAIMED camera only, 2 = that route for every takeover.
    CyberpunkVR_DevCamInLocate = xrDevCamInLocate < 0 ? 0 : (xrDevCamInLocate > 2 ? 2 : xrDevCamInLocate);
    CyberpunkVR_LensHeadWrite = (xrLensHeadWrite != 0) ? 1 : 0;
    CyberpunkVR_InputDefaultInUi = (xrInputDefaultInUi != 0) ? 1 : 0;
    CyberpunkVR_PopupMagBlockMs =
        (xrPopupMagBlockMs >= 0 && xrPopupMagBlockMs <= 10000) ? xrPopupMagBlockMs : 1000;
    CyberpunkVR_DevCamAnyName = (xrDevCamAnyName != 0) ? 1 : 0;
    CyberpunkVR_DevCamTolM = (xrDevCamTolM > 0.05f && xrDevCamTolM < 100.0f) ? xrDevCamTolM : 1.5f;
    CyberpunkVR_VrcamPosFromMain =
        (xrVrcamPosFromMain >= 0 && xrVrcamPosFromMain <= 3) ? xrVrcamPosFromMain : 0;
    CyberpunkVR_BdIpdInLocate = xrBdIpdInLocate != 0 ? 1 : 0;
    CyberpunkVR_BdQuatFromBuffer = xrBdQuatFromBuffer != 0 ? 1 : 0;
    CyberpunkVR_CapGrant = xrCapGrant;
    CyberpunkVR_ForceVrcamCam = xrForceVrcamCam;
    CyberpunkVR_VrcamCamFields = xrVrcamCamFields;
    CyberpunkVR_RunViewParams = xrRunViewParams;
    CyberpunkVR_ExpoProbe = xrExpoProbe;
    CyberpunkVR_IndirectCensus = xrIndirectCensus;
    CyberpunkVR_NoStateLies = xrNoStateLies;
    CyberpunkVR_StableCopy = xrStableCopy;
    CyberpunkVR_DistantReuseMode = xrDistantReuse;
    CyberpunkVR_LocalShadowReuseMode = xrLocalShadowReuse;
    CyberpunkVR_GiReuseMode = xrGiReuse;
    CyberpunkVR_ProbeReuseMode = xrProbeReuse;
    CyberpunkVR_ExpoMirror = xrExpoMirror;
    CyberpunkVR_ExpoFieldMask = xrExpoFieldMask;

    CyberpunkVR_CompLendScoped = xrCompLendScoped;
    CyberpunkVR_LightContent = xrLightContent;
    CyberpunkVR_FixLodEnable = xrFixLod;
    CyberpunkVR_HudToSecondEye = xrHudTo2;
    CyberpunkVR_FingerBones = xrFingerBones;
    CyberpunkVR_ThumbRigOpen = xrThumbRigOpen;
    CyberpunkVR_FingerSpread = xrFingerSpread;
    CyberpunkVR_AsymProjection = xrAsym;
    CyberpunkVR_AsymInnerGuard = xrAsymGuard;
    CyberpunkVR_VisionToSecondEye = xrVisionTo2;
    CyberpunkVR_HudInBraindance = xrHudBd;
    CyberpunkVR_VrcamCompositionGroup = xrCompGroup;
    CyberpunkVR_TwoHandRadius = (xrTwoHandRadius < 0.02f) ? 0.02f
                              : ((xrTwoHandRadius > 0.30f) ? 0.30f : xrTwoHandRadius);
    CyberpunkVR_TwoHandAim = (xrTwoHandAim < 0) ? 0 : ((xrTwoHandAim > 2) ? 2 : xrTwoHandAim);
    CyberpunkVR_TwoHandBreakRadius = (xrTwoHandBreak <= 0.0f) ? 0.0f
                                   : ((xrTwoHandBreak > 0.60f) ? 0.60f : xrTwoHandBreak);
    CyberpunkVR_TwoHandAimReleaseMs = (xrTwoHandAimRelease < 20.0f) ? 20.0f
                                    : ((xrTwoHandAimRelease > 2000.0f) ? 2000.0f : xrTwoHandAimRelease);
    CyberpunkVR_CarryGripRadius = (xrCarryRadius < 0.03f) ? 0.03f
                                : ((xrCarryRadius > 0.30f) ? 0.30f : xrCarryRadius);
    CyberpunkVR_RifleRiseMul    = (xrRifleRise   < 0.0f) ? 0.0f : ((xrRifleRise   > 2.0f) ? 2.0f : xrRifleRise);
    CyberpunkVR_RifleBackMul    = (xrRifleBack   < 0.0f) ? 0.0f : ((xrRifleBack   > 6.0f) ? 6.0f : xrRifleBack);
    CyberpunkVR_ShotgunRiseMul  = (xrShotgunRise < 0.0f) ? 0.0f : ((xrShotgunRise > 2.0f) ? 2.0f : xrShotgunRise);
    CyberpunkVR_ShotgunBackMul  = (xrShotgunBack < 0.0f) ? 0.0f : ((xrShotgunBack > 6.0f) ? 6.0f : xrShotgunBack);
    CyberpunkVR_RecoilDownPistol  = (xrDownPistol  < 0.0f) ? 0.0f : ((xrDownPistol  > 1.0f) ? 1.0f : xrDownPistol);
    CyberpunkVR_RecoilDownRifle   = (xrDownRifle   < 0.0f) ? 0.0f : ((xrDownRifle   > 1.0f) ? 1.0f : xrDownRifle);
    CyberpunkVR_RecoilDownShotgun = (xrDownShotgun < 0.0f) ? 0.0f : ((xrDownShotgun > 1.0f) ? 1.0f : xrDownShotgun);
    CyberpunkVR_HandRecoilBackCm    = (xrBackCm    < 0.0f) ? 0.0f : ((xrBackCm    > 20.0f) ? 20.0f : xrBackCm);
    CyberpunkVR_HandRecoilBackMaxCm = (xrBackMaxCm < 0.0f) ? 0.0f : ((xrBackMaxCm > 30.0f) ? 30.0f : xrBackMaxCm);
    CyberpunkVR_TwoHandBackMul = (xrTwoHandBack < 0.0f) ? 0.0f : ((xrTwoHandBack > 1.0f) ? 1.0f : xrTwoHandBack);
    CyberpunkVR_SniperRiseMul  = (xrSniperRise < 0.0f) ? 0.0f : ((xrSniperRise > 4.0f) ? 4.0f : xrSniperRise);
    CyberpunkVR_SniperBackMul  = (xrSniperBack < 0.0f) ? 0.0f : ((xrSniperBack > 8.0f) ? 8.0f : xrSniperBack);
    CyberpunkVR_RecoilScale    = (xrRecoilScale < 0.0f) ? 0.0f : ((xrRecoilScale > 2.0f) ? 2.0f : xrRecoilScale);
    CyberpunkVR_Haptics        = xrHaptics;
    CyberpunkVR_HapticScale    = (xrHapticScale < 0.0f) ? 0.0f : ((xrHapticScale > 2.0f) ? 2.0f : xrHapticScale);
    CyberpunkVR_PistolBackMul  = (xrPistolBack < 0.0f) ? 0.0f : ((xrPistolBack > 4.0f) ? 4.0f : xrPistolBack);
    CyberpunkVR_RecoilClimbFrac   = (xrClimbFrac < 0.0f) ? 0.0f : ((xrClimbFrac > 2.0f) ? 2.0f : xrClimbFrac);
    CyberpunkVR_RecoilClimbMaxDeg = (xrClimbMax  < 0.0f) ? 0.0f : ((xrClimbMax  > 45.0f) ? 45.0f : xrClimbMax);
    CyberpunkVR_RecoilClimbMs     = (xrClimbMs   < 50.0f) ? 50.0f : ((xrClimbMs > 5000.0f) ? 5000.0f : xrClimbMs);
    CyberpunkVR_HandRecoilReturnMs  = (xrReturnMs  < 40.0f) ? 40.0f : ((xrReturnMs > 800.0f) ? 800.0f : xrReturnMs);
    CyberpunkVR_HandRecoilReturnPow = (xrReturnPow < 0.0f) ? 0.0f : ((xrReturnPow > 1.5f) ? 1.5f : xrReturnPow);
    // Clamped to one screen either way, and to a scale that leaves something on screen. A piece dragged
    // ten thousand pixels off or scaled to nothing looks exactly like a broken mod, and an ini edited by
    // hand is the likeliest way to get there. The scale range matches what the editor's wheel allows,
    // so a saved layout and a live one cannot disagree.
    for (int i = 0; i < 7; ++i) {
        float sx = xrScannerSlots[i * 3 + 0];
        float sy = xrScannerSlots[i * 3 + 1];
        float ss = xrScannerSlots[i * 3 + 2];
        CyberpunkVR_ScannerSlots[i * 3 + 0] = (sx < -1920.0f) ? -1920.0f : ((sx > 1920.0f) ? 1920.0f : sx);
        CyberpunkVR_ScannerSlots[i * 3 + 1] = (sy < -1080.0f) ? -1080.0f : ((sy > 1080.0f) ? 1080.0f : sy);
        CyberpunkVR_ScannerSlots[i * 3 + 2] = (ss < 0.10f) ? 0.10f : ((ss > 5.0f) ? 5.0f : ss);
    }
    CyberpunkVR_HandRelToFilteredHead = xrHandRelFiltered;
    CyberpunkVR_HandLocatePerFrame = xrHandPerFrame;
    // NEGATIVE IS ALLOWED, down to one period BEHIND the frame's target, and that is not a mistake.
    // We locate the hands at a FUTURE instant, so the runtime extrapolates -- and extrapolation noise
    // grows with the distance predicted. The spec guarantees at least 50 ms of retained history, so
    // aiming closer to now, or slightly behind it, is an accurate measurement rather than a guess. It
    // trades latency for steadiness, which is the trade this shake is about.
    WriteVrikSettingsFile(); // keep the CET-facing bridge file in sync with vrport.ini
    if (prevXrRecenter == 0 && xrRecenter != 0) {
        OpenXRManager::Get().RequestRecenter();
        Log("OpenXR recenter requested.\n");
    }

    if (prevXrMonoSubmit != xrMonoSubmit) {
        OpenXRManager::Get().SetMonoSubmitEnabled(xrMonoSubmit != 0);
        Log("OpenXR mono submit %s.\n", xrMonoSubmit != 0 ? "enabled" : "disabled");
    }

    // A plain global rather than a live-controls field: producer and consumer are both inside
    // this plugin and there is no CET boundary to cross, so OpenXRManager reads it directly.
    if (CyberpunkVR_ThreadedMonoSubmit != xrThreadedSubmit) {
        CyberpunkVR_ThreadedMonoSubmit = xrThreadedSubmit;
        const char* how = (xrThreadedSubmit == 0) ? "INLINE pump (forced)"
                        : (xrThreadedSubmit > 0)  ? "submit THREAD (forced)"
                                                  : "auto";
        Log("OpenXR submit owner: xr_threaded_submit=%d -> %s; resolved now = %s\n",
            xrThreadedSubmit, how,
            OpenXRManager::Get().UseThreadedSubmit() ? "submit thread" : "inline pump");
    }

    // Applies or reverts the plane-routing byte to match the key just parsed. A no-op unless it
    // changed, so this costs one comparison per tick.
    cvr::detail::weapon_plane_sync();
    cvr::detail::highlight_any_plane_sync();
    cvr::detail::plane_bucket_scene_sync();



    if (changed && g_verboseLog) {
        Log("Live controls updated: xr_head_offset=(%.4f,%.4f,%.4f) xr_recenter=%d xr_mono_submit=%d xr_force_fov=%.3f xr_menu_rect=%d xr_menu_fov=%.3f xr_3dof_movement=%d xr_motion_predict_ms=%.2f xr_stereo_scale=%.3f xr_render_pose_submit=%d xr_runtime=%d\n",
            g_liveControls.xrHeadOffsetX, g_liveControls.xrHeadOffsetY, g_liveControls.xrHeadOffsetZ, g_liveControls.xrRecenter, g_liveControls.xrMonoSubmit, g_liveControls.xrForceFov, g_liveControls.xrMenuRect, g_liveControls.xrMenuFov, g_liveControls.xr3DofMovement, g_liveControls.xrMotionPredictMs, g_liveControls.xrStereoScale, g_liveControls.xrRenderPoseSubmit, g_liveControls.xrRuntime);
        if (g_liveControls.xrRuntime != 0) {
            Log("Live controls: xr_runtime=%d will apply on next startup before OpenXR init.\n", g_liveControls.xrRuntime);
        }
    }
}

LiveControlsUiState MakeLiveControlsUiState() {
    LiveControlsUiState state{};
    state.xrHeadOffsetX = g_liveControls.xrHeadOffsetX;
    state.xrHeadOffsetY = g_liveControls.xrHeadOffsetY;
    state.xrHeadOffsetZ = g_liveControls.xrHeadOffsetZ;
    state.xrRecenter = g_liveControls.xrRecenter;
    state.xrMonoSubmit = g_liveControls.xrMonoSubmit;
    state.xrForceFov = g_liveControls.xrForceFov;
    state.xrMenuRect = g_liveControls.xrMenuRect;
    state.xrMenuFov = g_liveControls.xrMenuFov;
    state.xrMenuFollowDeg = g_liveControls.xrMenuFollowDeg;
    state.xrHudPanel=g_liveControls.xrHudPanel;
    state.xrHudFollowDeg=g_liveControls.xrHudFollowDeg;
    state.xrInteractionPanel=g_liveControls.xrInteractionPanel;
    state.xrInteractionFollowDeg=g_liveControls.xrInteractionFollowDeg;
    state.xrLootFollowDeg=g_liveControls.xrLootFollowDeg;
    state.xrInteractionDistance=g_liveControls.xrInteractionDistance;
    state.xrInteractionFov=g_liveControls.xrInteractionFov;
    state.xrHudFov=g_liveControls.xrHudFov;
    state.xrHudDistance=g_liveControls.xrHudDistance;
    state.xrHudFollowMode=g_liveControls.xrHudFollowMode;
    state.xrHudStereoDepth=g_liveControls.xrHudStereoDepth;
    state.xrGripFromFingers=g_gripFromFingersSetting;
    state.xrFingerBones=CyberpunkVR_FingerBones;
    state.xrThumbRigOpen=CyberpunkVR_ThumbRigOpen;
    state.xrFingerSpread=CyberpunkVR_FingerSpread;
    state.xrHudBrightness=g_liveControls.xrHudBrightness;
    state.xrHudShadow=g_liveControls.xrHudShadow;
    state.xrHudGlow=g_liveControls.xrHudGlow;
    state.hudElements=cvr::hud::GetLayoutSettings();
    state.framegen=cvr::framegen::GetSettings();
    state.foveated=cvr::foveated::GetSettings();
    state.vrs=cvr::vrs::GetSettings();
    state.hiddenArea=cvr::hidden_area::GetSettings();
    state.monoCascades=int(CyberpunkVR_MonoCascades.load(std::memory_order_relaxed));
    state.monoMode=CyberpunkVR_MonoMode!=0;
    state.scopeStabilize=cvr::optics::source::GetStabilize();
    state.nvidiaReflex=cvr::reflex::GetMode();
    state.overlay=cvr::vrui::GetSettings();
    state.xr3DofMovement = g_liveControls.xr3DofMovement;
    state.xrFirstLaunch = g_liveControls.xrFirstLaunch;
    state.xrMotionPredictMs = g_liveControls.xrMotionPredictMs;
    state.xrStereoScale = g_liveControls.xrStereoScale;
    state.xrWorldScale = g_liveControls.xrWorldScale;
    state.xrIpdScale = g_liveControls.xrIpdScale;
    state.xrSharpness = g_liveControls.xrSharpness;
    state.xrSharpmix = g_liveControls.xrSharpmix;
    state.xrReuseLastFrame = g_liveControls.xrReuseLastFrame;
    state.xrPairLock = g_liveControls.xrPairLock;
    state.xrRenderPoseSubmit = g_liveControls.xrRenderPoseSubmit;
    state.xrPoseLag = g_liveControls.xrPoseLag;
    state.xrRuntime = g_liveControls.xrRuntime;
    state.xrMovementControl = g_liveControls.xrMovementControl;
    state.xrDisableMouseY = g_liveControls.xrDisableMouseY;
    state.xrXInputHook = g_liveControls.xrXInputHook;
    state.xrSnapTurn = g_liveControls.xrSnapTurn;
    state.xrSnapTurnAngleDeg = g_liveControls.xrSnapTurnAngleDeg;
    state.xrMovementSource = g_liveControls.xrMovementSource;
    state.xrMovementSpeedMode=g_liveControls.xrMovementSpeedMode;
    state.xrLeftStickDeadzone=g_liveControls.xrLeftStickDeadzone;
    state.xrRightStickDeadzone=g_liveControls.xrRightStickDeadzone;
    state.xrMaxInputThreshold=g_liveControls.xrMaxInputThreshold;
    const auto bodyMode=static_cast<cvr::body::RotationMode>(g_liveControls.xrBodyRotationMode);
    state.xrPhysicalBodyRotation = bodyMode==cvr::body::RotationMode::HeadCone;
    state.xrTrackedBodyRotation = bodyMode==cvr::body::RotationMode::Tracked;
    state.xrHybridBodyRotation = bodyMode==cvr::body::RotationMode::Hybrid;
    state.xrRoomscaleMovement = g_liveControls.xrRoomscaleMovement;
    state.xrPhysicalCrouch = g_liveControls.xrPhysicalCrouch;
    state.xrPhysicalCrouchDepth = g_liveControls.xrPhysicalCrouchDepth;
    state.xrAsymProjection = CyberpunkVR_AsymProjection == 1 ? 1 : 0;
    state.xrHaptics = CyberpunkVR_Haptics != 0 ? 1 : 0;
    state.xrHapticScale = CyberpunkVR_HapticScale;
    state.xrBreaststrokeSwim = g_liveControls.xrBreaststrokeSwim;
    state.xrLadderGripClimb = g_liveControls.xrLadderGripClimb;
    state.xrLadderAutoFinish=g_liveControls.xrLadderAutoFinish;
    state.xrLadderFinishDistance=g_liveControls.xrLadderFinishDistance;
    state.xrBodyFreeLookDeg=g_liveControls.xrBodyFreeLookDeg;
    state.xrBodyFreeLookDownDeg=g_liveControls.xrBodyFreeLookDownDeg;
    state.xrBodyFreeLookSwimDeg=g_liveControls.xrBodyFreeLookSwimDeg;
    state.xrBodyMoveRadius=g_liveControls.xrBodyMoveRadius;
    state.xrCutsceneSuspendTier = g_liveControls.xrCutsceneSuspendTier;
    state.xrVehicleCutsceneSuspendTier = g_liveControls.xrVehicleCutsceneSuspendTier;
    state.xrXInputInstall = g_liveControls.xrXInputInstall;
    state.xrInputActions = g_liveControls.xrInputActions;
    state.xrMonoXQueueWait = g_liveControls.xrMonoXQueueWait;
    state.xrMonoDepthCapture = g_liveControls.xrMonoDepthCapture;
    state.xrSnapTurnPulseMs = g_liveControls.xrSnapTurnPulseMs;
    state.xrImmersiveHolsters = g_liveControls.xrImmersiveHolsters;
    state.xrVehHeadOffsetX = g_liveControls.xrVehHeadOffsetX;
    state.xrVehHeadOffsetY = g_liveControls.xrVehHeadOffsetY;
    state.xrVehHeadOffsetZ = g_liveControls.xrVehHeadOffsetZ;
    state.xrWheelGrab = g_liveControls.xrWheelGrab;
    state.xrWheelRadius = g_liveControls.xrWheelRadius;
    state.xrWheelSteerMaxDeg = g_liveControls.xrWheelSteerMaxDeg;
    state.xrWheelSteerDeadDeg = g_liveControls.xrWheelSteerDeadDeg;
    state.xrWheelPrediction = g_liveControls.xrWheelPrediction;
    state.xrWheelPredictionMs = g_liveControls.xrWheelPredictionMs;
    state.xrWheelHorn = g_liveControls.xrWheelHorn;
    state.xrWheelHornRadius = g_liveControls.xrWheelHornRadius;
    state.xrVehicleGunTrigger = g_liveControls.xrVehicleGunTrigger;
    state.xrVehicleThrottleTrim = g_liveControls.xrVehicleThrottleTrim;
    return state;
}

void PersistLiveControlsUiState(const LiveControlsUiState& state) {
    InitRuntimePaths();
    FILE* file = _fsopen(g_liveControlPath, "w", _SH_DENYNO);
    if (!file) return;

    fprintf(file, "xr_head_offset_x=%.4f\n", state.xrHeadOffsetX);
    fprintf(file, "xr_head_offset_y=%.4f\n", state.xrHeadOffsetY);
    fprintf(file, "xr_head_offset_z=%.4f\n", state.xrHeadOffsetZ);
    fprintf(file, "xr_recenter=0\n");
    fprintf(file, "xr_mono_submit=%d\n", state.xrMonoSubmit != 0 ? 1 : 0);
    // Written from the global because it is not a UI control. It MUST be here all the same:
    // this function rewrites the whole file, and a key left out is a key deleted -- the
    // mistake that once ate xr_hand_predict.
    fprintf(file, "xr_threaded_submit=%d\n", CyberpunkVR_ThreadedMonoSubmit);
    fprintf(file, "xr_force_fov=%.3f\n", state.xrForceFov);
    fprintf(file, "xr_menu_rect=%d\n", state.xrMenuRect != 0 ? 1 : 0);
    fprintf(file, "xr_menu_fov=%.3f\n", state.xrMenuFov);
    fprintf(file, "xr_menu_follow_deg=%.3f\n", state.xrMenuFollowDeg >= 5.0f ? state.xrMenuFollowDeg : 60.0f);
    cvr::framegen::WriteSettings(file,state.framegen);
    cvr::foveated::WriteSettings(file,state.foveated);
    cvr::vrs::WriteSettings(file,state.vrs);
    cvr::hidden_area::WriteSettings(file,state.hiddenArea);
    std::fprintf(file,"xr_mono_cascades=%d\n",state.monoCascades?1:0);
    std::fprintf(file,"xr_mono_mode=%d\n",state.monoMode?1:0);
    std::fprintf(file,"xr_scope_stabilize=%d\n",state.scopeStabilize);
    fprintf(file,"xr_nvidia_reflex=%d\n",cvr::reflex::NormalizeMode(state.nvidiaReflex));
    cvr::vrui::WriteSettings(file,state.overlay);
    fprintf(file,"xr_hud_panel=%d\nxr_hud_follow_deg=%.3f\nxr_hud_fov=%.3f\nxr_hud_distance=%.3f\n",state.xrHudPanel,state.xrHudFollowDeg,state.xrHudFov,state.xrHudDistance);
    fprintf(file,"xr_interaction_panel=%d\nxr_interaction_follow_deg=%.3f\nxr_interaction_distance=%.3f\nxr_interaction_fov=%.3f\n",state.xrInteractionPanel,state.xrInteractionFollowDeg,state.xrInteractionDistance,state.xrInteractionFov);
    fprintf(file,"xr_loot_follow_deg=%.3f\n",state.xrLootFollowDeg);
    fprintf(file,"xr_hud_follow_mode=%d\nxr_hud_stereo_depth=%d\nxr_hud_brightness=%.3f\nxr_hud_shadow=%.3f\nxr_hud_glow=%.3f\n",state.xrHudFollowMode,state.xrHudStereoDepth,state.xrHudBrightness,state.xrHudShadow,state.xrHudGlow);
    cvr::hud::SaveLayoutSettings(file,state.hudElements);
    fprintf(file, "xr_3dof_movement=%d\n", state.xr3DofMovement != 0 ? 1 : 0);
    // Not a control, but it MUST be written back: this function rewrites the whole file, so
    // leaving the key out would drop it, and the next launch would read the default 1 and
    // re-install the shipped settings over whatever the player had just changed.
    fprintf(file, "first_launch=%d\n", state.xrFirstLaunch != 0 ? 1 : 0);
    fprintf(file, "xr_motion_predict_ms=%.2f\n", state.xrMotionPredictMs);
    fprintf(file, "xr_stereo_scale=%.3f\n", state.xrStereoScale);
    fprintf(file, "xr_world_scale=%.3f\n", state.xrWorldScale);
    fprintf(file, "xr_ipd_scale=%.3f\n", state.xrIpdScale);
    fprintf(file, "xr_sharpness=%.3f\n", state.xrSharpness);
    fprintf(file, "xr_sharpmix=%.3f\n", state.xrSharpmix);
    fprintf(file, "xr_reuse_last_frame=%d\n", state.xrReuseLastFrame != 0 ? 1 : 0);
    fprintf(file, "xr_pair_lock=%d\n", state.xrPairLock != 0 ? 1 : 0);
    fprintf(file, "xr_render_pose_submit=%d\n", state.xrRenderPoseSubmit != 0 ? 1 : 0);
    fprintf(file, "xr_depth_submit=%d\n", g_liveControls.xrDepthSubmit != 0 ? 1 : 0);
    fprintf(file, "xr_pose_lag=%d\n", state.xrPoseLag);
    fprintf(file, "xr_runtime=%d\n", ClampRuntimeMode(state.xrRuntime));
    fprintf(file, "xr_hmd_smooth=%.3f\n", GetHmdTrackingSmooth());
    // The hand filter's speed, in UEVR's units (follow per second, multiplied by dt at the point of
    // use). Replaces xr_hand_smooth, which was a fraction per FRAME and therefore frame-rate dependent.
    fprintf(file, "xr_hand_lerp_speed=%.3f\n", CyberpunkVR_HandLerpSpeed);
    // Per weapon class, 0 = use the base above. 1 handgun, 2 rifle, 3 shotgun, 4 sniper, 5 melee.
    fprintf(file, "xr_hand_lerp_pistol=%.3f\n", CyberpunkVR_HandLerpPistol);
    fprintf(file, "xr_hand_lerp_rifle=%.3f\n", CyberpunkVR_HandLerpRifle);
    fprintf(file, "xr_hand_lerp_shotgun=%.3f\n", CyberpunkVR_HandLerpShotgun);
    fprintf(file, "xr_hand_lerp_sniper=%.3f\n", CyberpunkVR_HandLerpSniper);
    fprintf(file, "xr_hand_lerp_melee=%.3f\n", CyberpunkVR_HandLerpMelee);
    // Every rate above times this under frame generation (1.5 = the rate and half again); 0 = off there.
    fprintf(file, "xr_hand_lerp_framegen_scale=%.3f\n", CyberpunkVR_HandLerpFramegenScale);
    // How near the support point the off hand has to come before the two-handed hold is offered, in
    // Which measured differences in the grading-LUT constant block the second view takes from MAIN.
    // One bit each, and they are tried ONE AT A TIME: all eight at once crashes the game, because some
    // of these fields are descriptor indices and lending one points this view at something it does not
    // own. 3 = the shipped pair (+0x230/+0x238). 16 = +0x258, the shader-permutation byte (0x12 vs
    // 0x16) and the best remaining candidate. 4 = +0x220. 8 = +0x248. 32/64/128 = the unidentified
    // ones, last and separately.
    fprintf(file, "xr_grade_mirror_mask=%X\n", CyberpunkVR_GradeMirrorMask);
    // Which extra environment handles the second view takes from MAIN. Bits 0-2 are element 0
    // (0x1F0/0x220/0x380) and froze the mirror when tried; bits 3-5 are element 1 and bits 6-8
    // element 2, never tried. One bit at a time -- refcounted handles.
    fprintf(file, "xr_env_extra_mask=%u\n", CyberpunkVR_EnvExtraMask);
    fprintf(file, "xr_water_mirror=%u\n", CyberpunkVR_WaterMirror);
    fprintf(file, "xr_water_assign=%u\n", CyberpunkVR_WaterAssign);
    fprintf(file, "xr_viewdata_diff=%d\n", CyberpunkVR_ViewDataDiff);
    fprintf(file, "xr_lut_src_lend=%u\n", CyberpunkVR_LutSrcLend);
    fprintf(file, "xr_scene_stick_yaw_dps=%.2f\n", CyberpunkVR_SceneStickYawDps);
    fprintf(file, "xr_scene_stick_yaw_tier=%d\n", CyberpunkVR_SceneStickYawTier);
    fprintf(file, "xr_block_diff=%d\n", CyberpunkVR_BlockDiff);
    fprintf(file, "xr_block_diff_node=%X\n", CyberpunkVR_BlockDiffNode);
    fprintf(file, "xr_block_diff_size=%u\n", CyberpunkVR_BlockDiffSize);
    // Render-mask categories granted to the second view, one bit per row of kRenderMasks. The
    // [rmask] log line prints the map and marks which view has what; [cap] lists the nodes the
    // engine still refuses. 1 = DistantLights, 2 = AutoGrass, 1024 = ClearLighting (what
    // HistogramUpdate, i.e. the auto-exposure, asks for), 2048 = GameplayPostProcess.
    fprintf(file, "xr_render_mask_grant=%u\n", CyberpunkVR_RenderMaskGrant);
    // 1 = single-pass stage replay: MAIN re-issues VRCAM's recorded geometry stages with its own
    // camera (about 2 ms less render-job CPU per frame in the city). 0 = both eyes record.
    fprintf(file, "xr_stage_replay=%d\n", CyberpunkVR_StageReplaySetting);
    // 1 = the second eye records its G-buffer in worker partitions like MAIN (same image, the
    // engine otherwise records it as one job). 0 = the engine's single partition.
    fprintf(file, "xr_vrcam_gbuffer_partitions=%d\n", CyberpunkVR_VrcamGbufferPartitionsSetting);
    // 2 = render-graph submission points chain their Close/Submit jobs among themselves instead of
    // holding up the graph (the next Synchronize starts while the previous batch is submitted); only
    // Present waits for the chain. 1 = view jobs wait for it too. 0 = the engine's order.
    fprintf(file, "xr_submit_chain=%d\n", CyberpunkVR_SubmitChainSetting);
    // 1 = the presenting node's DXGI Present runs as a job after the chain (steady state only).
    fprintf(file, "xr_defer_present=%d\n", CyberpunkVR_DeferPresentSetting);
    // 1 = while it waits, the game thread takes jobs from queues 3 and 2 only and leaves render jobs to the workers.
    fprintf(file, "xr_gt_skip_render_queue=%d\n", CyberpunkVR_GtSkipRenderQueueSetting);
    // Which viewData holes the second view gets filled from MAIN, HEX, one bit per entry of
    // kViewDataHoles: 1 rain block, 2 composition-out resource set (DANGEROUS), 4/8/10 composition
    // out, 20 rain block, 40 cloud wind (deliberately off -- handled via the cloud CB), 80 rain
    // wetness, 100/200/400 composition-debug, 800 night pair, 1000 far distance, 2000 scene bounds.
    fprintf(file, "xr_viewdata_fix_mask=%X\n", CyberpunkVR_ViewDataFixMask);
    // Which viewData ranges the second view takes from MAIN. Bit 12 (0x1000) is the colour
    // grade at +0x640 -- the whole environment look rides on it.
    fprintf(file, "xr_env_mirror_mask=%X\n", CyberpunkVR_EnvMirrorMask);
    fprintf(file, "xr_bd_cam_dirty=%d\n", CyberpunkVR_BdCamDirty);
    fprintf(file, "xr_bd_push_transform=%d\n", CyberpunkVR_BdPushTransform);
    fprintf(file, "xr_bd_main_pos_from_scene=%d\n",
            CyberpunkVR_BdMainPosFromScene);
    fprintf(file, "xr_bd_push_base=%d\n", CyberpunkVR_BdPushBase);
    fprintf(file, "xr_bd_one_composition=%d\n",
            CyberpunkVR_BdOneComposition);
    fprintf(file, "xr_bd_editor_align=%d\n", CyberpunkVR_BdEditorAlign);
    fprintf(file, "xr_bd_base_from_locate=%d\n",
            CyberpunkVR_BdBaseFromLocate);
    fprintf(file, "xr_comp_lend_set=%u\n", CyberpunkVR_CompLendSet);
    fprintf(file, "xr_final_grab=%d\n", CyberpunkVR_FinalGrab);
    fprintf(file, "xr_stable_srgb_view=%d\n", CyberpunkVR_StableSrgbView);
    fprintf(file, "xr_bd_quat_from_write_site=%d\n",
            CyberpunkVR_BdQuatFromWriteSite);
    // 0 = the second eye keeps its own attachment, 1 = it takes MAIN's base in a braindance,
    // 2 = always. 1 is the one that closes the 1.48 m gap measured in a braindance edit.
    fprintf(file, "xr_dev_cam_tol_m=%.2f\n", CyberpunkVR_DevCamTolM);
    fprintf(file, "xr_dev_cam_any_name=%d\n", CyberpunkVR_DevCamAnyName);
    fprintf(file, "xr_dev_cam_in_locate=%d\n", CyberpunkVR_DevCamInLocate);
    fprintf(file, "xr_lens_head_write=%d\n", CyberpunkVR_LensHeadWrite);
    fprintf(file, "xr_input_default_in_ui=%d\n", CyberpunkVR_InputDefaultInUi);
    fprintf(file, "xr_popup_mag_block_ms=%d\n", CyberpunkVR_PopupMagBlockMs);
    fprintf(file, "xr_vrcam_pos_from_main=%d\n", CyberpunkVR_VrcamPosFromMain);
    // Hex, because the value is an RVA and it is read straight off a log line printed in hex.
    fprintf(file, "xr_vision_cs_node=%X\n", CyberpunkVR_VisionCsNode);
    fprintf(file, "xr_vision_cs_swap=%d\n", CyberpunkVR_VisionCsSwap);
    fprintf(file, "xr_vision_cs_dim=%d\n", CyberpunkVR_VisionCsDim);
    fprintf(file, "xr_vision_cs_ord=%d\n", CyberpunkVR_VisionCsOrd);
    fprintf(file, "xr_weapon_plane_as_scene=%d\n", CyberpunkVR_WeaponPlaneAsScene);
    fprintf(file, "xr_highlight_any_plane=%d\n", CyberpunkVR_HighlightAnyPlane);
    fprintf(file, "xr_plane_bucket_scene=%d\n", CyberpunkVR_PlaneBucketScene);
    // 1 = MAIN's half of the eye separation goes into the located buffer in a braindance,
    // which is the descriptor the engine renders it through. 0 = back to the component.
    fprintf(file, "xr_bd_ipd_in_locate=%d\n", CyberpunkVR_BdIpdInLocate);
    // 1 = the braindance head base comes from the located buffer (fresh), 0 = from the pose the
    // script publishes for the scene camera (a tick old, and it judders on head turns).
    fprintf(file, "xr_bd_quat_from_buffer=%d\n", CyberpunkVR_BdQuatFromBuffer);
    // 0 = off. 1 = grant the refused capability at ClusteredLightsCull / RenderLightBuffers
    // to the second view; 2 = at every node. See Detour_ViewFeatureCheck.
    fprintf(file, "xr_cap_grant=%u\n", CyberpunkVR_CapGrant);
    // 1 = second view's fov/zoom/near/far follow MAIN's (shipped default). 0 leaves the
    // second view its own -- expect its field of view to look wrong, which is the point:
    // it says whether that copy is what breaks the light cluster grid.
    fprintf(file, "xr_force_vrcam_cam=%u\n", CyberpunkVR_ForceVrcamCam);
    // HEX. Which of MAIN's camera fields the second view takes: 1 fov, 2 zoom, 4 near, 8 far.
    // F = the old behaviour; 3 = fov and zoom only, leaving the depth slicing this view's own.
    fprintf(file, "xr_vrcam_cam_fields=%X\n", CyberpunkVR_VrcamCamFields);
    // The environment-override stack, applied to this view: 0 off, 1 the engine's whole pass
    // (which also erases faded entries from a list MAIN shares), 2 the same apply with nothing
    // removed. It is what puts the scanner's green tint into the second eye.
    fprintf(file, "xr_run_view_params=%u\n", CyberpunkVR_RunViewParams);
    // Reads both views' 28-byte FrameExposureData back into the log. 0 off, 1 every 8 s,
    // 2 every 200 ms -- 2 is for catching a transient such as the braindance glasses flash.
    // Not free: it copies out of an engine resource on every bind of that buffer.
    fprintf(file, "xr_expo_probe=%d\n", CyberpunkVR_ExpoProbe);
    // 1 = [indirect] every 10 s: ExecuteIndirect per frame-graph node, per view. Names the node
    // the second view replays with an argument buffer that has not been rebuilt.
    fprintf(file, "xr_indirect_census=%d\n", CyberpunkVR_IndirectCensus);
    // 1 = refuse a barrier on a foreign resource whose state was not observed. It refuses the eye
    // capture too, and stereo goes with it -- see the note at CyberpunkVR_NoStateLies. Default 0.
    fprintf(file, "xr_no_state_lies=%d\n", CyberpunkVR_NoStateLies);
    fprintf(file, "xr_stable_copy=%d\n", CyberpunkVR_StableCopy);
    // 1 = the second view reuses MAIN's result and the engine's builder is skipped for it;
    // 0 = the second view builds its own. See the note beside the externs: a skipped builder is
    // the leading candidate for the INDIRECT_ARGUMENT binding the driver dies on.
    fprintf(file, "xr_distant_reuse=%u\n", CyberpunkVR_DistantReuseMode);
    fprintf(file, "xr_local_shadow_reuse=%u\n", CyberpunkVR_LocalShadowReuseMode);
    fprintf(file, "xr_gi_reuse=%u\n", CyberpunkVR_GiReuseMode);
    fprintf(file, "xr_probe_reuse=%u\n", CyberpunkVR_ProbeReuseMode);
    // 1 = the second view takes MAIN's exposure adaptation, so the braindance flash reaches it.
    fprintf(file, "xr_expo_mirror=%d\n", CyberpunkVR_ExpoMirror);
    // DECIMAL, one bit per float of the 28-byte FrameExposureData. 27 = f0|f1|f3|f4, the four
    // slots the adaptation moves and the only ones the two views agree on at rest. 127 copies
    // the whole buffer, which is what whitened the second eye.
    fprintf(file, "xr_expo_field_mask=%u\n", CyberpunkVR_ExpoFieldMask);
    // Which readers of viewData+0x168 get MAIN's composition state, for that call only:
    // 1 CompositionPostProcess, 2 the RT declarations, 4 DrawHUD, 8 the fifth reader. Try 3.
    // DrawComposition is deliberately not offered a bit: it is the node that crashes on it.
    fprintf(file, "xr_comp_lend_scoped=%u\n", CyberpunkVR_CompLendScoped);
    // 1 = byte-compare the light array ([lightbuf]) and the particle uploads ([partbuf])
    // between the views. Diagnostic; costs a 64 KB copy per upload while it is on.
    fprintf(file, "xr_light_content=%d\n", CyberpunkVR_LightContent);
    fprintf(file, "xr_fix_lod=%d\n", CyberpunkVR_FixLodEnable);
    // The port's own HUD composite into the second eye, and whether it applies in a braindance.
    fprintf(file, "xr_hud_to_second_eye=%d\n", CyberpunkVR_HudToSecondEye);
    // 1 = finger-tracking gloves: grip from a fist or thumbs-up (hand skeleton), not the squeeze value. Next session.
    fprintf(file, "xr_grip_from_fingers=%d\n", state.xrGripFromFingers ? 1 : 0);
    // 1 = with xr_grip_from_fingers, each of V's fingers follows its own measured bend; 0 = poses from the buttons.
    fprintf(file, "xr_finger_bones=%d\n", state.xrFingerBones ? 1 : 0);
    // 1 = with finger tracking, the open thumb is the rig's reference pose; 0 = laid flat along the index.
    fprintf(file, "xr_thumb_rig_open=%d\n", state.xrThumbRigOpen ? 1 : 0);
    // How far V's fingers spread sideways as the player's do: 0 = off (only curl), 1 = default, up to 2.
    fprintf(file, "xr_finger_spread=%.2f\n", state.xrFingerSpread);
    // 1 = each eye renders exactly its own lens window (off-centre, the runtime's per-eye FOV, so a FOV
    // cut set in the runtime is followed) and is submitted with it; 0 = the old symmetric frustum.
    fprintf(file, "xr_asym_projection=%d\n", CyberpunkVR_AsymProjection == 1 ? 1 : 0);
    // 1 = both eyes render the same horizontal range (screen-space reflections, shadows, bloom identical in
    // the two eyes); 0 = each eye only its own lens window (sharper per pixel, the eyes then disagree).
    fprintf(file, "xr_asym_inner_guard=%.2f\n", CyberpunkVR_AsymInnerGuard);
    // 1 = the engine's own two-eye frame instead of the second view (Stereo/EngineStereo.hpp). Read once at
    // start. Written as the ini asked for it: the mono mode turns it off for a launch, not for good.
    fprintf(file, "xr_engine_stereo=%d\n", cvr::engine_stereo::Requested() ? 1 : 0);
    fprintf(file, "xr_vision_to_second_eye=%d\n", CyberpunkVR_VisionToSecondEye);
    fprintf(file, "xr_hud_in_braindance=%d\n", CyberpunkVR_HudInBraindance);
    // 1 = inject the composition group into the second view's graph via the engine's pass adders.
    fprintf(file, "xr_vrcam_composition=%u\n", CyberpunkVR_VrcamCompositionGroup);
    // metres. Clamped to [0.02, 0.30] on read.
    fprintf(file, "xr_two_hand_radius=%.3f\n", CyberpunkVR_TwoHandRadius);
    // How the support hand steers the barrel. 0 = not at all (the weapon is the right controller's);
    // 1 = by how far the left hand moves against the right once the grip has closed (default); 2 = laid
    // on the line between the hands from the first frame (the old behaviour).
    fprintf(file, "xr_two_hand_aim=%d\n", CyberpunkVR_TwoHandAim);
    // metres from the support point at which a held two-hand grip lets go of the weapon. 0 = never.
    fprintf(file, "xr_two_hand_break=%.3f\n", CyberpunkVR_TwoHandBreakRadius);
    // How long the weapon takes to give the left hand's steer back after the grip opens, milliseconds.
    // Clamped to [20, 2000] on read.
    fprintf(file, "xr_two_hand_aim_release_ms=%.1f\n", CyberpunkVR_TwoHandAimReleaseMs);
    // How near the carried weapon the right hand has to come before its fingers close on the grip and
    // the grip button takes the weapon back. One number for both. Clamped to [0.03, 0.30] on read.
    fprintf(file, "xr_carry_radius=%.3f\n", CyberpunkVR_CarryGripRadius);
    // HOW THE RECOIL IMPULSE IS SPLIT PER CLASS: muzzle rise and shoulder travel, as multipliers on the
    // per-weapon numbers. A rifle barely lifts and shoves back; a shotgun does both. Pistols are the
    // reference and have no multiplier. Rise clamped to [0, 2], travel to [0, 6].
    fprintf(file, "xr_recoil_rifle_rise=%.3f\n", CyberpunkVR_RifleRiseMul);
    fprintf(file, "xr_recoil_rifle_back=%.3f\n", CyberpunkVR_RifleBackMul);
    fprintf(file, "xr_recoil_shotgun_rise=%.3f\n", CyberpunkVR_ShotgunRiseMul);
    fprintf(file, "xr_recoil_shotgun_back=%.3f\n", CyberpunkVR_ShotgunBackMul);
    // Snipers and precision rifles: the one shouldered class that flips MORE than the pistol reference
    // and throws the hand furthest. Rise clamped to [0, 4], travel to [0, 8].
    fprintf(file, "xr_recoil_sniper_rise=%.3f\n", CyberpunkVR_SniperRiseMul);
    fprintf(file, "xr_recoil_sniper_back=%.3f\n", CyberpunkVR_SniperBackMul);
    // THE WHOLE HAND RECOIL, every weapon and both channels (angle and throw), applied last -- after the
    // ceilings, so a weapon at the cap loses the same share as a light one. Clamped to [0, 2].
    fprintf(file, "xr_recoil_scale=%.3f\n", CyberpunkVR_RecoilScale);
    // CONTROLLER VIBRATION (Runtimes/Haptics.cpp): rounds by weapon class, melee block and parry,
    // bullet block and deflect, a tick on reaching a belt element. 0 switches it all off; the scale
    // multiplies every profile, clamped to [0, 2].
    fprintf(file, "xr_haptics=%d\n", CyberpunkVR_Haptics);
    fprintf(file, "xr_haptic_scale=%.3f\n", CyberpunkVR_HapticScale);
    // A pistol's throw: the smallest of the four, because its energy goes into the flip. Its RISE has no
    // multiplier -- the reference angle is the pistol's. Clamped to [0, 4].
    fprintf(file, "xr_recoil_pistol_back=%.3f\n", CyberpunkVR_PistolBackMul);
    // MUZZLE CLIMB ACROSS A BURST: what fraction of one shot's peak angle accumulates, the ceiling for
    // that accumulation in degrees, and the time constant it walks back down with.
    fprintf(file, "xr_recoil_climb=%.3f\n", CyberpunkVR_RecoilClimbFrac);
    fprintf(file, "xr_recoil_climb_max_deg=%.3f\n", CyberpunkVR_RecoilClimbMaxDeg);
    fprintf(file, "xr_recoil_climb_ms=%.1f\n", CyberpunkVR_RecoilClimbMs);
    // HOW FAST THE HAND COMES BACK: the reference settle in milliseconds, and how much the weapon's own
    // kick stretches it (0 = every weapon settles alike). The settle belongs to the wrist; the cartridge
    // decides the ANGLE, not the duration.
    fprintf(file, "xr_recoil_return_ms=%.1f\n", CyberpunkVR_HandRecoilReturnMs);
    fprintf(file, "xr_recoil_return_pow=%.3f\n", CyberpunkVR_HandRecoilReturnPow);
    // WHICH WAY the hand is thrown, per class: 0 = straight back along the barrel, 1 = straight down
    // (the grip dipping in the palm as the muzzle flips). Clamped to [0, 1] on read.
    fprintf(file, "xr_recoil_down_pistol=%.3f\n", CyberpunkVR_RecoilDownPistol);
    fprintf(file, "xr_recoil_down_rifle=%.3f\n", CyberpunkVR_RecoilDownRifle);
    fprintf(file, "xr_recoil_down_shotgun=%.3f\n", CyberpunkVR_RecoilDownShotgun);
    // HOW FAR the hand is thrown backwards on one shot, and the ceiling for it once the per-weapon
    // ratio and the class multiplier have had their say. Centimetres.
    fprintf(file, "xr_recoil_back_cm=%.3f\n", CyberpunkVR_HandRecoilBackCm);
    fprintf(file, "xr_recoil_back_max_cm=%.3f\n", CyberpunkVR_HandRecoilBackMaxCm);
    // WHAT A SECOND HAND LEAVES OF THAT THROW, 0..1 -- separate from what it leaves of the muzzle flip,
    // which is a stiffer reduction (leverage against a moment arm, not mass against an impulse).
    fprintf(file, "xr_recoil_twohand_back=%.3f\n", CyberpunkVR_TwoHandBackMul);
    // The scanner's four movable pieces, x,y,scale each, in 1920x1080 design pixels. Normally written by
    // the in-game editor rather than by hand -- hold RIGHT SHIFT while scanning.
    {
        static const char* kScannerKeys[7] = { "xr_scanner_frame", "xr_scanner_details",
                                              "xr_scanner_hacks", "xr_scanner_hint",
                                              "xr_scanner_memory", "xr_scanner_scripts",
                                              "xr_scanner_desc" };
        for (int i = 0; i < 7; ++i) {
            fprintf(file, "%s=%.1f,%.1f,%.3f\n", kScannerKeys[i],
                    CyberpunkVR_ScannerSlots[i * 3 + 0],
                    CyberpunkVR_ScannerSlots[i * 3 + 1],
                    CyberpunkVR_ScannerSlots[i * 3 + 2]);
        }
    }
    fprintf(file, "xr_movement_control=%d\n", state.xrMovementControl != 0 ? 1 : 0);
    fprintf(file, "xr_disable_mouse_y=%d\n", state.xrDisableMouseY != 0 ? 1 : 0);
    fprintf(file, "xr_xinput_hook=%d\n", state.xrXInputHook != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn=%d\n", state.xrSnapTurn != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn_angle_deg=%.2f\n", state.xrSnapTurnAngleDeg > 0.0f ? state.xrSnapTurnAngleDeg : 30.0f);
    fprintf(file, "xr_movement_source=%d\n", state.xrMovementSource < 0 ? 0 : (state.xrMovementSource > 3 ? 3 : state.xrMovementSource));
    fprintf(file,"xr_movement_speed_mode=%d\nxr_left_stick_deadzone=%.3f\nxr_right_stick_deadzone=%.3f\nxr_max_input_threshold=%.3f\n",
        state.xrMovementSpeedMode==1,cvr::input::StickDeadzone(state.xrLeftStickDeadzone),
        cvr::input::StickDeadzone(state.xrRightStickDeadzone),cvr::input::StickFullInput(state.xrMaxInputThreshold));
    fprintf(file, "xr_physical_body_rotation=%d\n", state.xrPhysicalBodyRotation != 0 ? 1 : 0);
    fprintf(file, "xr_tracked_body_rotation=%d\n", state.xrTrackedBodyRotation != 0 ? 1 : 0);
    fprintf(file, "xr_hybrid_body_rotation=%d\n", state.xrHybridBodyRotation != 0 ? 1 : 0);
    fprintf(file, "xr_roomscale_movement=%d\n", state.xrRoomscaleMovement != 0 ? 1 : 0);
    fprintf(file, "xr_physical_crouch=%d\n", state.xrPhysicalCrouch != 0 ? 1 : 0);
    fprintf(file, "xr_physical_crouch_depth=%.3f\n", cvr::body::CrouchThreshold(state.xrPhysicalCrouchDepth));
    fprintf(file, "xr_breaststroke_swim=%d\n", state.xrBreaststrokeSwim != 0 ? 1 : 0);
    fprintf(file, "xr_ladder_grip_climb=%d\n", state.xrLadderGripClimb != 0 ? 1 : 0);
    fprintf(file, "xr_ladder_auto_finish=%d\nxr_ladder_finish_distance=%.3f\n",state.xrLadderAutoFinish!=0 ? 1:0,state.xrLadderFinishDistance);
    fprintf(file, "xr_body_free_look_deg=%.3f\n", state.xrBodyFreeLookDeg);
    fprintf(file, "xr_body_free_look_down_deg=%.3f\n", state.xrBodyFreeLookDownDeg);
    fprintf(file, "xr_body_free_look_swim_deg=%.3f\n", state.xrBodyFreeLookSwimDeg);
    fprintf(file, "xr_body_move_radius=%.4f\n", state.xrBodyMoveRadius);
    fprintf(file, "xr_cutscene_suspend_tier=%d\n",
            state.xrCutsceneSuspendTier < -1 ? -1 : (state.xrCutsceneSuspendTier > 4 ? 4 : state.xrCutsceneSuspendTier));
    fprintf(file, "xr_vehicle_cutscene_suspend_tier=%d\n",
            cvr::anim::ResolveVehicleVrikSuspendTier(state.xrCutsceneSuspendTier,state.xrVehicleCutsceneSuspendTier,true));
    fprintf(file, "xr_xinput_install=%d\n", state.xrXInputInstall != 0 ? 1 : 0);
    fprintf(file, "xr_input_actions=%d\n", state.xrInputActions != 0 ? 1 : 0);
    fprintf(file, "xr_mono_xqueue_wait=%d\n", state.xrMonoXQueueWait != 0 ? 1 : 0);
    fprintf(file, "xr_mono_depth_capture=%d\n", state.xrMonoDepthCapture != 0 ? 1 : 0);
    fprintf(file, "xr_snap_turn_pulse_ms=%d\n", state.xrSnapTurnPulseMs > 0 ? state.xrSnapTurnPulseMs : 30);
    fprintf(file, "xr_immersive_holsters=%d\n", state.xrImmersiveHolsters != 0 ? 1 : 0);
    fprintf(file, "xr_veh_head_offset_x=%.4f\n", state.xrVehHeadOffsetX);
    fprintf(file, "xr_veh_head_offset_y=%.4f\n", state.xrVehHeadOffsetY);
    fprintf(file, "xr_veh_head_offset_z=%.4f\n", state.xrVehHeadOffsetZ);
    fprintf(file, "xr_wheel_grab=%d\n", state.xrWheelGrab != 0 ? 1 : 0);
    fprintf(file, "xr_wheel_radius=%.3f\n", state.xrWheelRadius > 0.0f ? state.xrWheelRadius : 0.28f);
    fprintf(file, "xr_wheel_steer_max_deg=%.1f\n", cvr::anim::WheelFullLockDegrees(state.xrWheelSteerMaxDeg));
    fprintf(file, "xr_wheel_steer_dead_deg=%.1f\n", state.xrWheelSteerDeadDeg >= 0.0f ? state.xrWheelSteerDeadDeg : 1.5f);
    fprintf(file, "xr_wheel_prediction=%d\n", state.xrWheelPrediction != 0 ? 1 : 0);
    fprintf(file, "xr_wheel_prediction_ms=%.1f\n", std::isfinite(state.xrWheelPredictionMs) ? std::clamp(state.xrWheelPredictionMs,0.0f,8.0f):0.0f);
    fprintf(file, "xr_wheel_horn=%d\n", state.xrWheelHorn != 0 ? 1 : 0);
    fprintf(file, "xr_wheel_horn_radius=%.3f\n", state.xrWheelHornRadius > 0.0f ? state.xrWheelHornRadius : 0.12f);
    fprintf(file, "xr_vehicle_gun_trigger=%d\n", state.xrVehicleGunTrigger != 0 ? 1 : 0);
    fprintf(file, "xr_vehicle_throttle_trim=%.2f\n", state.xrVehicleThrottleTrim > 0.0f ? state.xrVehicleThrottleTrim : 0.5f);
    fclose(file);

    WIN32_FILE_ATTRIBUTE_DATA fileData;
    if (GetFileAttributesExA(g_liveControlPath, GetFileExInfoStandard, &fileData)) {
        g_lastLiveControlWrite = fileData.ftLastWriteTime;
    }
}

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState) {
    if (!outState) return;
    *outState = MakeLiveControlsUiState();
}

extern "C" void RequestLiveControlsRecenter() {
    g_liveControls.xrRecenter = 0;
    OpenXRManager::Get().RequestRecenter();
    Log("ImGui: OpenXR recenter requested.\n");
}

extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile) {
    if (!state) return;
    cvr::framegen::SetSettings(state->framegen);
    cvr::foveated::SetSettings(state->foveated);
    cvr::vrs::SetSettings(state->vrs);
    cvr::hidden_area::SetSettings(state->hiddenArea);
    CyberpunkVR_MonoCascades.store(state->monoCascades?1:0,std::memory_order_relaxed);
    cvr::engine_stereo::ApplyMonoMode(state->monoMode!=0);
    cvr::optics::source::SetStabilize(state->scopeStabilize);
    cvr::reflex::SetMode(state->nvidiaReflex);
    cvr::vrui::SetSettings(state->overlay);

    const int prevMono = g_liveControls.xrMonoSubmit;

    g_liveControls.xrHeadOffsetX = state->xrHeadOffsetX;
    g_liveControls.xrHeadOffsetY = state->xrHeadOffsetY;
    g_liveControls.xrHeadOffsetZ = state->xrHeadOffsetZ;
    g_liveControls.xrRecenter = 0;
    g_liveControls.xrMonoSubmit = state->xrMonoSubmit != 0 ? 1 : 0;
    g_liveControls.xrForceFov = state->xrForceFov > 0.0f ? state->xrForceFov : 0.0f;
    g_liveControls.xrHudPanel=state->xrHudPanel!=0;
    g_liveControls.xrHudFollowMode=state->xrHudFollowMode==1;
    g_liveControls.xrHudStereoDepth=state->xrHudStereoDepth!=0;
    g_gripFromFingersSetting=state->xrGripFromFingers!=0;   // read when the next session starts
    CyberpunkVR_FingerBones=state->xrFingerBones!=0;
    CyberpunkVR_ThumbRigOpen=state->xrThumbRigOpen!=0;
    CyberpunkVR_FingerSpread=std::clamp(state->xrFingerSpread,0.0f,2.0f);
    g_liveControls.xrHudBrightness=(state->xrHudBrightness>=0.25f && state->xrHudBrightness<=3) ? state->xrHudBrightness : 1;
    g_liveControls.xrHudShadow=(state->xrHudShadow>=0 && state->xrHudShadow<=2) ? state->xrHudShadow : 1;
    g_liveControls.xrHudGlow=(state->xrHudGlow>=0 && state->xrHudGlow<=2) ? state->xrHudGlow : 1;
    cvr::hud::SetLayoutSettings(state->hudElements);
    g_liveControls.xrHudFollowDeg=(state->xrHudFollowDeg>=5 && state->xrHudFollowDeg<=90) ? state->xrHudFollowDeg : 60.0f;
    g_liveControls.xrInteractionPanel=state->xrInteractionPanel!=0;
    g_liveControls.xrInteractionFollowDeg=(state->xrInteractionFollowDeg>=0 && state->xrInteractionFollowDeg<=90)?state->xrInteractionFollowDeg:0.0f;
    g_liveControls.xrLootFollowDeg=(state->xrLootFollowDeg>=0 && state->xrLootFollowDeg<=90)?state->xrLootFollowDeg:0.0f;
    g_liveControls.xrInteractionDistance=(state->xrInteractionDistance>=.5f && state->xrInteractionDistance<=5)?state->xrInteractionDistance:2.0f;
    g_liveControls.xrInteractionFov=(state->xrInteractionFov>=30 && state->xrInteractionFov<=120)?state->xrInteractionFov:75.0f;
    g_liveControls.xrHudFov=(state->xrHudFov>=30 && state->xrHudFov<=120) ? state->xrHudFov : 65.4f;
    g_liveControls.xrHudDistance=(state->xrHudDistance>=0.5f && state->xrHudDistance<=5) ? state->xrHudDistance : 1.5f;
    g_liveControls.xrMenuRect = state->xrMenuRect != 0 ? 1 : 0;
    g_liveControls.xrMenuFov = state->xrMenuFov > 1.0f ? state->xrMenuFov : 65.0f;
    g_liveControls.xrMenuFollowDeg = (state->xrMenuFollowDeg >= 5.0f && state->xrMenuFollowDeg <= 90.0f) ? state->xrMenuFollowDeg : 60.0f;
    g_liveControls.xr3DofMovement = state->xr3DofMovement != 0 ? 1 : 0;
    g_liveControls.xrFirstLaunch = state->xrFirstLaunch != 0 ? 1 : 0;
    g_liveControls.xrMotionPredictMs = state->xrMotionPredictMs >= 0.0f ? state->xrMotionPredictMs : 0.0f;
    g_liveControls.xrStereoScale = state->xrStereoScale < 0.0f ? 0.0f : (state->xrStereoScale > 10.0f ? 10.0f : state->xrStereoScale);
    g_liveControls.xrWorldScale = state->xrWorldScale < 0.05f ? 0.05f : (state->xrWorldScale > 20.0f ? 20.0f : state->xrWorldScale);
    g_liveControls.xrIpdScale = state->xrIpdScale < 0.0f ? 0.0f : (state->xrIpdScale > 5.0f ? 5.0f : state->xrIpdScale);
    g_liveControls.xrSharpness = state->xrSharpness < 0.0f ? 0.0f : (state->xrSharpness > 1.0f ? 1.0f : state->xrSharpness);
    g_liveControls.xrSharpmix = state->xrSharpmix < 0.0f ? 0.0f : (state->xrSharpmix > 1.0f ? 1.0f : state->xrSharpmix);
    g_liveControls.xrReuseLastFrame = state->xrReuseLastFrame != 0 ? 1 : 0;
    g_liveControls.xrPairLock = state->xrPairLock != 0 ? 1 : 0;
    g_liveControls.xrRenderPoseSubmit = state->xrRenderPoseSubmit != 0 ? 1 : 0;
    g_liveControls.xrPoseLag = state->xrPoseLag;
    g_liveControls.xrRuntime = ClampRuntimeMode(state->xrRuntime);
    {
        int src = state->xrMovementSource;
        if (src < 0 || src > 3) src = state->xrMovementControl != 0 ? 1 : 0;
        g_liveControls.xrMovementSource = src;
        g_liveControls.xrMovementControl = src != 0 ? 1 : 0;
    }
    g_liveControls.xrPhysicalBodyRotation = state->xrPhysicalBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrMovementSpeedMode=state->xrMovementSpeedMode==1;
    g_liveControls.xrLeftStickDeadzone=cvr::input::StickDeadzone(state->xrLeftStickDeadzone);
    g_liveControls.xrRightStickDeadzone=cvr::input::StickDeadzone(state->xrRightStickDeadzone);
    g_liveControls.xrMaxInputThreshold=cvr::input::StickFullInput(state->xrMaxInputThreshold);
    g_liveControls.xrTrackedBodyRotation = state->xrTrackedBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrHybridBodyRotation = state->xrHybridBodyRotation != 0 ? 1 : 0;
    g_liveControls.xrBodyRotationMode=static_cast<int>(cvr::body::RotationModeFromFlags(
        state->xrPhysicalBodyRotation,state->xrTrackedBodyRotation,state->xrHybridBodyRotation));
    g_liveControls.xrRoomscaleMovement = state->xrRoomscaleMovement != 0 ? 1 : 0;
    g_liveControls.xrPhysicalCrouch = state->xrPhysicalCrouch != 0 ? 1 : 0;
    g_liveControls.xrPhysicalCrouchDepth = cvr::body::CrouchThreshold(state->xrPhysicalCrouchDepth);
    // Only on a real change: mode 2 is the probes' test projection and must survive an unrelated edit.
    if ((state->xrAsymProjection != 0) != (CyberpunkVR_AsymProjection == 1))
        CyberpunkVR_AsymProjection = state->xrAsymProjection != 0 ? 1 : 0;
    CyberpunkVR_Haptics = state->xrHaptics != 0 ? 1 : 0;
    CyberpunkVR_HapticScale = std::isfinite(state->xrHapticScale) ? std::clamp(state->xrHapticScale, 0.0f, 2.0f) : 1.0f;
    g_liveControls.xrBreaststrokeSwim = state->xrBreaststrokeSwim != 0 ? 1 : 0;
    g_liveControls.xrLadderGripClimb = state->xrLadderGripClimb != 0 ? 1 : 0;
    g_liveControls.xrLadderAutoFinish=state->xrLadderAutoFinish!=0 ? 1:0;
    g_liveControls.xrLadderFinishDistance=std::clamp(std::isfinite(state->xrLadderFinishDistance) ? state->xrLadderFinishDistance:1.0f,.2f,1.2f);
    g_liveControls.xrBodyFreeLookDeg=std::clamp(state->xrBodyFreeLookDeg,0.0f,60.0f);
    g_liveControls.xrBodyFreeLookDownDeg=std::clamp(std::isfinite(state->xrBodyFreeLookDownDeg) ? state->xrBodyFreeLookDownDeg:30.0f,0.0f,90.0f);
    g_liveControls.xrBodyFreeLookSwimDeg=std::clamp(std::isfinite(state->xrBodyFreeLookSwimDeg) ? state->xrBodyFreeLookSwimDeg:5.0f,0.0f,90.0f);
    g_liveControls.xrBodyMoveRadius=std::clamp(state->xrBodyMoveRadius,0.0f,.30f);
    g_liveControls.xrCutsceneSuspendTier =
        (state->xrCutsceneSuspendTier < -1) ? -1
                                           : (state->xrCutsceneSuspendTier > 4 ? 4 : state->xrCutsceneSuspendTier);
    g_liveControls.xrVehicleCutsceneSuspendTier = cvr::anim::ResolveVehicleVrikSuspendTier(
        state->xrCutsceneSuspendTier,state->xrVehicleCutsceneSuspendTier,true);
    g_liveControls.xrDisableMouseY = state->xrDisableMouseY != 0 ? 1 : 0;
    g_liveControls.xrXInputHook = state->xrXInputHook != 0 ? 1 : 0;
    g_liveControls.xrSnapTurn = state->xrSnapTurn != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnAngleDeg = state->xrSnapTurnAngleDeg > 0.0f ? state->xrSnapTurnAngleDeg : 30.0f;
    g_liveControls.xrXInputInstall = state->xrXInputInstall != 0 ? 1 : 0;
    g_liveControls.xrInputActions = state->xrInputActions != 0 ? 1 : 0;
    g_liveControls.xrMonoXQueueWait = state->xrMonoXQueueWait != 0 ? 1 : 0;
    g_liveControls.xrMonoDepthCapture = state->xrMonoDepthCapture != 0 ? 1 : 0;
    g_liveControls.xrSnapTurnPulseMs = state->xrSnapTurnPulseMs > 0 ? state->xrSnapTurnPulseMs : 30;
    g_liveControls.xrImmersiveHolsters = state->xrImmersiveHolsters != 0 ? 1 : 0;
    OpenXRManager::Get().SetImmersiveHolsters(g_liveControls.xrImmersiveHolsters);
    {   // In-vehicle head offset: same bound as the ini path, for the same reason.
        auto cl = [](float v) { return (v < -1.0f) ? -1.0f : (v > 1.0f ? 1.0f : v); };
        g_liveControls.xrVehHeadOffsetX = cl(state->xrVehHeadOffsetX);
        g_liveControls.xrVehHeadOffsetY = cl(state->xrVehHeadOffsetY);
        g_liveControls.xrVehHeadOffsetZ = cl(state->xrVehHeadOffsetZ);
    }
    // DRIVING. Same clamps as the ini path -- the overlay sliders already bound these, but the two
    // entry points must not be able to disagree about what a valid value is.
    g_liveControls.xrWheelGrab = state->xrWheelGrab != 0 ? 1 : 0;
    g_liveControls.xrWheelHorn = state->xrWheelHorn != 0 ? 1 : 0;
    g_liveControls.xrVehicleGunTrigger = state->xrVehicleGunTrigger != 0 ? 1 : 0;
    {
        const float r = state->xrWheelRadius;
        g_liveControls.xrWheelRadius = (r < 0.08f) ? 0.08f : (r > 0.60f ? 0.60f : r);
        const float m = state->xrWheelSteerMaxDeg;
        g_liveControls.xrWheelSteerMaxDeg = cvr::anim::WheelFullLockDegrees(m);
        const float d = state->xrWheelSteerDeadDeg;
        g_liveControls.xrWheelSteerDeadDeg = (d < 0.0f) ? 0.0f : (d > 20.0f ? 20.0f : d);
        g_liveControls.xrWheelPrediction = state->xrWheelPrediction != 0 ? 1 : 0;
        g_liveControls.xrWheelPredictionMs = std::isfinite(state->xrWheelPredictionMs) ? std::clamp(state->xrWheelPredictionMs,0.0f,8.0f):0.0f;
        const float hr = state->xrWheelHornRadius;
        g_liveControls.xrWheelHornRadius = (hr < 0.04f) ? 0.04f : (hr > 0.30f ? 0.30f : hr);
        const float tt = state->xrVehicleThrottleTrim;
        g_liveControls.xrVehicleThrottleTrim = (tt < 0.05f) ? 0.05f : (tt > 3.0f ? 3.0f : tt);
    }
    WriteVrikSettingsFile(); // publish mouse-Y flag for the CET VRIK mod

    if (prevMono != g_liveControls.xrMonoSubmit) {
        OpenXRManager::Get().SetMonoSubmitEnabled(g_liveControls.xrMonoSubmit != 0);
        Log("ImGui: OpenXR mono submit %s.\n", g_liveControls.xrMonoSubmit != 0 ? "enabled" : "disabled");
    }
    if (state->xrRecenter != 0) {
        RequestLiveControlsRecenter();
    }

    if (persistToFile != 0) {
        PersistLiveControlsUiState(MakeLiveControlsUiState());
    }
}

void PollHotkeys() {
    static bool f7WasDown = false;
    static bool f8WasDown = false;

    const bool f7Down = (GetAsyncKeyState(VK_F7) & 0x8000) != 0;
    const bool f8Down = (GetAsyncKeyState(VK_F8) & 0x8000) != 0;

    if (f7Down && !f7WasDown) {
        OpenXRManager::Get().RequestRecenter();
        Log("Hotkey F7: OpenXR recenter requested.\n");
    }

    if (f8Down && !f8WasDown) {
        g_liveControls.xrMenuRect = g_liveControls.xrMenuRect != 0 ? 0 : 1;
        Log("Hotkey F8: xr_menu_rect=%d (%s).\n",
            g_liveControls.xrMenuRect,
            g_liveControls.xrMenuRect != 0 ? "small HMD rectangle" : "full HMD rectangle");
    }

    f7WasDown = f7Down;
    f8WasDown = f8Down;
}
