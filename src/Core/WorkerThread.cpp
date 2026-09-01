// WorkerThread -- the one background thread, and the reason there is only one.
//
// It polls: the live-control file, the hotkeys, the recenter request, the calibration bridge. All of
// that is file and registry work that must not happen on a render thread, and all of it is slow enough
// that doing it per frame would be visible.
//
// ONE THREAD, NOT ONE PER JOB. Each poll is idempotent and none of them depend on each other's timing,
// so a single loop with a sleep is both sufficient and the only shape in which the ordering between
// them is obvious. Adding a second thread here would mean the live controls could change under a poll
// that had already read half of them.

#include "Anim/ReloadPose.hpp"   // RestFingerTick
#include "Anim/TwoHandGrip.hpp"  // TwoHandTick
#include <windows.h>
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include "Camera/CameraState.hpp"   // PatchFastPath, PatchFastDisarm
// The per-camera recognition counters, read by the fast-path recovery below.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPatchCamMain;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPatchCamVrcam;
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
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
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"

DWORD WINAPI WorkerThread(LPVOID) {
    if (g_verboseLog) Log("Worker thread started, waiting 8 seconds...\n");
    if (g_backendModulePath[0] != '\0') {
        if (g_verboseLog) Log("Backend module loaded from: %s\n", g_backendModulePath);
    }
    Sleep(8000);

    EnsureLiveControlFileExists();
    PollLiveControls();
    InitGameModuleInfo();

    // Allocate telemetry structure
    g_telemetry = static_cast<TelemetryData*>(VirtualAlloc(nullptr, sizeof(TelemetryData), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    ZeroMemory(g_telemetry, sizeof(TelemetryData));

    g_setterTrace = static_cast<SetterTraceData*>(VirtualAlloc(nullptr, sizeof(SetterTraceData), MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE));
    ZeroMemory(g_setterTrace, sizeof(SetterTraceData));

    // Give the stereo module the node dispatcher FIRST. Both want RVA 0x1EC404 and MinHook
    // allows one hook per address, so this was a race between two entry points: sync_stereo
    // boots from CreateDXGIFactory, this thread wakes 8 s after DllMain, and on a slow load
    // the factory call lands later than that. Whoever lost printed "failed to hook node
    // dispatcher" -- and when the loser was sync_stereo the whole second view went dark: no
    // t_vrcam_node_active, so no RTV capture, no snapshot, no right eye and no mirror window.
    // InitStereoOnce is idempotent, so calling it here just settles the order: whatever the
    // stereo module means to own, it owns before the PostStereo stage is asked to install.
    InitStereoOnce();
    cvr::hooks::InstallStage(cvr::hooks::Stage::PostStereo);






    // ProjAspectCopy / ProjAspectCall were already commented out here; their six sibling
    // "did it install" flags went with them. All six had ONE write and NO readers -- the boot
    // being long enough to hide that is the only reason they survived.



    // LodFov moved to Hooks/LodFov.cpp and installs itself from the registry below.


    // g_forceHeadingUpdateHookInstalled is gone with this line. Moving the hook out showed the
    // flag had exactly one write and NO readers -- it was already dead, and only the boot function
    // being long enough to hide that kept it. The registry reports the install result instead.



    // XInput carries its own gate now; see Hooks/XInput.cpp.


    // PatchBuffer's gate moved with the hook to Hooks/PatchBuffer.cpp, where the precondition sits
    // beside the code it governs instead of in a boot function that has to remember it.

    // The three NativeSetter tracers moved to their own files and carry their own gate;
    // the registry reports them as skipped when the gate is off.

    // EVERY HOOK THAT NOW OWNS ITS OWN FILE, installed from the registry it registered itself
    // with. This is where the hand-written list above is being retired to: as each hook moves out
    // it declares its own stage and order beside its own code, and its line disappears from here.
    // The registry logs one line per hook, pass or fail -- the old list discarded the return value,
    // so a hook that stopped matching after a game patch looked exactly like a feature nobody had
    // written.
    cvr::hooks::InstallStage(cvr::hooks::Stage::Boot);
    cvr::hooks::ReportInstalled();

    uint32_t prevLocateHits = 0;
    uint32_t prevPatchHits = 0;
    uint32_t prevFinalHits = 0;
    uint32_t prevDeltaHeadHits = 0;
    uint32_t prevMoveXYHits = 0;
    uint32_t prevFreeDeltaHits = 0;
    uintptr_t prevPatchRdx = 0;
    uintptr_t prevPatchRsi = 0;
    uintptr_t prevFinalRsi = 0;
    uintptr_t prevDeltaHeadRcx = 0;
    uintptr_t prevMoveXYRsi = 0;
    uintptr_t prevFreeDeltaRsi = 0;
    uint32_t prevMetaWriteHits = 0;
    uint32_t prevMetaConsumeHits = 0;
    uint32_t prevClearHits = 0;
    uintptr_t prevMetaWriteTemp = 0;
    uintptr_t prevMetaWriteMeta = 0;
    uintptr_t prevMetaConsumeTemp = 0;
    uintptr_t prevMetaConsumeMeta = 0;
    uintptr_t prevMetaWriteRsp = 0;
    uintptr_t prevMetaConsumeRsp = 0;
    uintptr_t prevClearTemp = 0;
    uintptr_t prevClearReturn = 0;
    uint32_t loopCounter = 0;

    for (;;) {
        PollLiveControls();
        // THE FAST PATH'S RECOVERY, and it costs nothing while nothing is wrong.
        //
        // The trampoline filter can only compare pointers, so the one failure it cannot notice is a
        // camera object that was DESTROYED and recreated: the latched address is never seen again,
        // every component is rejected, and the camera would stay frozen for the session. The
        // classifier's own latch-drop covers a camera that is still passing the site and no longer
        // qualifies; it cannot cover one that stops passing at all.
        //
        // A blind disarm every tick would fix that and cost a full classifier pass each time --
        // ~180 slow calls per 200 ms, about 900/s, which is most of what the fast path just saved.
        // So disarm on the SIGNAL instead: each camera's own recognition counter advances once per
        // rendered frame, and a tick in which one of them did not advance at all is exactly the
        // state that needs a re-identification. In the healthy case both advance and nothing is
        // written; when the site is idle (paused, loading, no camera yet) the disarm is free
        // because there are no calls to be slow.
        {
            static uint64_t prevCamMain = 0, prevCamVrcam = 0;
            static bool     prevCamValid = false;
            const uint64_t m = CyberpunkVR_DebugPatchCamMain;
            const uint64_t v = CyberpunkVR_DebugPatchCamVrcam;
            if (prevCamValid && (m == prevCamMain || v == prevCamVrcam)) PatchFastDisarm();
            prevCamMain = m;
            prevCamVrcam = v;
            prevCamValid = true;
        }
        PollHotkeys();
        ApplyKnownResolutionOverrides();
        // The resting left-hand pose's DISK half: writes the frame the pose path captured, and loads it
        // back on the first pass after the finger resolver has names to match against. Here rather than in
        // the pose path because that runs inside the game's own pose apply, several times a tick, and a
        // file open there is an unbounded wait in the middle of the animation.
        cvr::anim::RestFingerTick();
        cvr::anim::RestFingerRightTick();
        cvr::anim::TwoHandTick();

        if ((loopCounter++ % 10) != 0) {
            Sleep(200);
            continue;
        }

        // The periodic telemetry dump below is pure diagnostics; skip it entirely
        // (and its bookkeeping) unless verbose logging is on. PollLiveControls /
        // PollHotkeys / resolution overrides above still run every iteration.
        if (!g_verboseLog) {
            Sleep(200);
            continue;
        }

        uint32_t lHits = g_telemetry->locateHits;
        uint32_t pHits = g_telemetry->patchHits;
        uint32_t fHits = g_telemetry->finalHits;
        
        Log("--- TELEMETRY SAMPLE ---\n");
        Log("LocateCamera: hits=%u, rbx=%p, xmm0(f32)=%.6f\n", 
            lHits, reinterpret_cast<void*>(g_telemetry->locateRbx), g_telemetry->locateXmm0);
        
        Log("PatchCamera:  hits=%u, rdx=%p, xmm0=(%.6f, %.6f, %.6f, %.6f)\n", 
            pHits, reinterpret_cast<void*>(g_telemetry->patchRdx), 
            g_telemetry->patchXmm0[0], g_telemetry->patchXmm0[1], 
            g_telemetry->patchXmm0[2], g_telemetry->patchXmm0[3]);
        Log("PatchCamera:  rsi=%p\n", reinterpret_cast<void*>(g_telemetry->patchRsi));

        Log("FinalCamera:  hits=%u, rsi=%p\n", 
            fHits, reinterpret_cast<void*>(g_telemetry->finalRsi));

        Log("DeltaHead:    hits=%u, rcx=%p, xmm0=%.6f\n",
            g_telemetry->deltaHeadHits,
            reinterpret_cast<void*>(g_telemetry->deltaHeadRcx),
            g_telemetry->deltaHeadXmm0);

        Log("MoveXY:       hits=%u, rsi=%p, xmm0=%.6f\n",
            g_telemetry->moveXYHits,
            reinterpret_cast<void*>(g_telemetry->moveXYRsi),
            g_telemetry->moveXYXmm0);

        Log("FreeDelta:    hits=%u, rsi=%p, xmm3=%.6f\n",
            g_telemetry->freeDeltaHits,
            reinterpret_cast<void*>(g_telemetry->freeDeltaRsi),
            g_telemetry->freeDeltaXmm3);

        Log("Unifix:       hits=%llu, proj=[%.4f %.4f %.4f %.4f %.4f %.4f %.4f %.4f] fov=%.4f\n",
            static_cast<unsigned long long>(g_unifixHits),
            g_unifixProjDump[0], g_unifixProjDump[1], g_unifixProjDump[2], g_unifixProjDump[3],
            g_unifixProjDump[4], g_unifixProjDump[5], g_unifixProjDump[6], g_unifixProjDump[7],
            g_unifixProjDump[8]);

        Log("ProjStage:    hits=%llu fov=%.4f aspect=%.4f extra=%.4f patched=%d\n",
            static_cast<unsigned long long>(g_projStageHits),
            g_projStageFov,
            g_projStageAspect,
            g_projStageExtra,
            g_projStagePatched ? 1 : 0);

        // Read render object projection data directly (filled by game during gameplay)
        if (g_unifixRenderObj) {
            uintptr_t rbx = g_unifixRenderObj;
            float renderProj[9] = {};
            bool ok = true;
            __try {
                for (int i = 0; i < 9; ++i)
                    renderProj[i] = *reinterpret_cast<float*>(rbx + 0x21C0 + i * 4);
            }
            __except (EXCEPTION_EXECUTE_HANDLER) { ok = false; }
            if (ok) {
                Log("UnifixRender: rbx=%p p0-3=[%.6f %.6f %.6f %.6f] p4-7=[%.6f %.6f %.6f %.6f] fov=%.6f\n",
                    reinterpret_cast<void*>(rbx),
                    renderProj[0], renderProj[1], renderProj[2], renderProj[3],
                    renderProj[4], renderProj[5], renderProj[6], renderProj[7],
                    renderProj[8]);
            }
        }

        if (g_telemetry->patchRdx != prevPatchRdx || g_telemetry->patchRsi != prevPatchRsi) {
            uintptr_t patchRdx = g_telemetry->patchRdx;
            uintptr_t patchRsi = g_telemetry->patchRsi;
            LogVec4At("patch rdx-0x10", patchRdx ? patchRdx - 0x10 : 0);
            LogVec4At("patch rdx+0x00", patchRdx);
            LogVec4At("patch rdx+0x10", patchRdx ? patchRdx + 0x10 : 0);
            LogVec4At("patch rdx+0x20", patchRdx ? patchRdx + 0x20 : 0);
            LogU8At("patch rsi+0xB0", patchRsi ? patchRsi + 0xB0 : 0);
            LogU8At("patch rsi+0xB1", patchRsi ? patchRsi + 0xB1 : 0);
            LogVec4At("patch rsi+0x90", patchRsi ? patchRsi + 0x90 : 0);
            LogVec4At("patch rsi+0xA0", patchRsi ? patchRsi + 0xA0 : 0);
        }

        if (g_telemetry->finalRsi != prevFinalRsi) {
            uintptr_t finalRsi = g_telemetry->finalRsi;
            LogFloatAt("final rsi+0x40", finalRsi ? finalRsi + 0x40 : 0);
            LogFloatAt("final rsi+0x44", finalRsi ? finalRsi + 0x44 : 0);
            LogVec4At("final rsi+0x30", finalRsi ? finalRsi + 0x30 : 0);
            LogVec4At("final rsi+0x40", finalRsi ? finalRsi + 0x40 : 0);
        }

        if (g_telemetry->deltaHeadRcx != prevDeltaHeadRcx || g_telemetry->deltaHeadHits != prevDeltaHeadHits) {
            uintptr_t deltaHeadRcx = g_telemetry->deltaHeadRcx;
            LogFloatAt("delta rcx+0x98", deltaHeadRcx ? deltaHeadRcx + 0x98 : 0);
            LogFloatAt("delta rcx+0x9C", deltaHeadRcx ? deltaHeadRcx + 0x9C : 0);
            LogFloatAt("delta rcx+0xA0", deltaHeadRcx ? deltaHeadRcx + 0xA0 : 0);
            LogFloatAt("delta rcx+0xA4", deltaHeadRcx ? deltaHeadRcx + 0xA4 : 0);
            LogFloatAt("delta rcx+0xA8", deltaHeadRcx ? deltaHeadRcx + 0xA8 : 0);
        }

        if (g_telemetry->moveXYRsi != prevMoveXYRsi || g_telemetry->moveXYHits != prevMoveXYHits) {
            uintptr_t moveXYRsi = g_telemetry->moveXYRsi;
            LogFloatAt("moveXY rsi+0x90", moveXYRsi ? moveXYRsi + 0x90 : 0);
            LogFloatAt("moveXY rsi+0x94", moveXYRsi ? moveXYRsi + 0x94 : 0);
            LogFloatAt("moveXY rsi+0x98", moveXYRsi ? moveXYRsi + 0x98 : 0);
            LogFloatAt("moveXY rsi+0x9C", moveXYRsi ? moveXYRsi + 0x9C : 0);
        }

        if (g_telemetry->freeDeltaRsi != prevFreeDeltaRsi || g_telemetry->freeDeltaHits != prevFreeDeltaHits) {
            uintptr_t freeDeltaRsi = g_telemetry->freeDeltaRsi;
            LogFloatAt("freeDelta rsi+0xCC8", freeDeltaRsi ? freeDeltaRsi + 0xCC8 : 0);
            LogFloatAt("freeDelta rsi+0xCCC", freeDeltaRsi ? freeDeltaRsi + 0xCCC : 0);
            LogFloatAt("freeDelta rsi+0x208", freeDeltaRsi ? freeDeltaRsi + 0x208 : 0);
            LogFloatAt("freeDelta rsi+0x20C", freeDeltaRsi ? freeDeltaRsi + 0x20C : 0);
        }

        if ((g_setterTrace->metaWriteHits != 0 && prevMetaWriteHits == 0) ||
            g_setterTrace->metaWriteTemp != prevMetaWriteTemp ||
            g_setterTrace->metaWriteMeta != prevMetaWriteMeta ||
            g_setterTrace->metaWriteRsp != prevMetaWriteRsp) {
            Log("NativeSetMetaWrite: hits=%u, temp=%p, meta=%p, rsp=%p\n",
                g_setterTrace->metaWriteHits,
                reinterpret_cast<void*>(g_setterTrace->metaWriteTemp),
                reinterpret_cast<void*>(g_setterTrace->metaWriteMeta),
                reinterpret_cast<void*>(g_setterTrace->metaWriteRsp));
            LogVec4At("metaWrite temp+0x00", g_setterTrace->metaWriteTemp);
            LogPtrAt("metaWrite temp+0x08", g_setterTrace->metaWriteTemp ? g_setterTrace->metaWriteTemp + 0x08 : 0);
            LogPtrAt("metaWrite meta+0x00", g_setterTrace->metaWriteMeta);
            LogPtrAt("metaWrite meta+0x08", g_setterTrace->metaWriteMeta ? g_setterTrace->metaWriteMeta + 0x08 : 0);
            LogPtrPayloadVec4At("metaWrite meta+0x10", g_setterTrace->metaWriteMeta ? g_setterTrace->metaWriteMeta + 0x10 : 0);
            LogStackWindowAt("metaWrite stack", g_setterTrace->metaWriteRsp, 12);
        }

        if ((g_setterTrace->metaConsumeHits != 0 && prevMetaConsumeHits == 0) ||
            g_setterTrace->metaConsumeTemp != prevMetaConsumeTemp ||
            g_setterTrace->metaConsumeMeta != prevMetaConsumeMeta ||
            g_setterTrace->metaConsumeRsp != prevMetaConsumeRsp) {
            Log("NativeSetMetaConsume: hits=%u, temp=%p, meta=%p, rsp=%p\n",
                g_setterTrace->metaConsumeHits,
                reinterpret_cast<void*>(g_setterTrace->metaConsumeTemp),
                reinterpret_cast<void*>(g_setterTrace->metaConsumeMeta),
                reinterpret_cast<void*>(g_setterTrace->metaConsumeRsp));
            LogVec4At("metaConsume temp+0x00", g_setterTrace->metaConsumeTemp);
            LogPtrAt("metaConsume temp+0x08", g_setterTrace->metaConsumeTemp ? g_setterTrace->metaConsumeTemp + 0x08 : 0);
            LogPtrAt("metaConsume meta+0x00", g_setterTrace->metaConsumeMeta);
            LogPtrAt("metaConsume meta+0x08", g_setterTrace->metaConsumeMeta ? g_setterTrace->metaConsumeMeta + 0x08 : 0);
            LogPtrPayloadVec4At("metaConsume meta+0x10", g_setterTrace->metaConsumeMeta ? g_setterTrace->metaConsumeMeta + 0x10 : 0);
            LogStackWindowAt("metaConsume stack", g_setterTrace->metaConsumeRsp, 12);
        }

        if ((g_setterTrace->clearHits != 0 && prevClearHits == 0) ||
            g_setterTrace->clearTemp != prevClearTemp ||
            g_setterTrace->clearReturn != prevClearReturn) {
            Log("NativeSetClear: hits=%u, temp=%p, return=%p\n",
                g_setterTrace->clearHits,
                reinterpret_cast<void*>(g_setterTrace->clearTemp),
                reinterpret_cast<void*>(g_setterTrace->clearReturn));
            LogVec4At("clear temp+0x00", g_setterTrace->clearTemp);
            LogPtrAt("clear temp+0x08", g_setterTrace->clearTemp ? g_setterTrace->clearTemp + 0x08 : 0);
        }

        prevLocateHits = lHits;
        prevPatchHits = pHits;
        prevFinalHits = fHits;
        prevDeltaHeadHits = g_telemetry->deltaHeadHits;
        prevMoveXYHits = g_telemetry->moveXYHits;
        prevFreeDeltaHits = g_telemetry->freeDeltaHits;
        prevPatchRdx = g_telemetry->patchRdx;
        prevPatchRsi = g_telemetry->patchRsi;
        prevFinalRsi = g_telemetry->finalRsi;
        prevDeltaHeadRcx = g_telemetry->deltaHeadRcx;
        prevMoveXYRsi = g_telemetry->moveXYRsi;
        prevFreeDeltaRsi = g_telemetry->freeDeltaRsi;
        prevMetaWriteHits = g_setterTrace->metaWriteHits;
        prevMetaConsumeHits = g_setterTrace->metaConsumeHits;
        prevClearHits = g_setterTrace->clearHits;
        prevMetaWriteTemp = g_setterTrace->metaWriteTemp;
        prevMetaWriteMeta = g_setterTrace->metaWriteMeta;
        prevMetaConsumeTemp = g_setterTrace->metaConsumeTemp;
        prevMetaConsumeMeta = g_setterTrace->metaConsumeMeta;
        prevMetaWriteRsp = g_setterTrace->metaWriteRsp;
        prevMetaConsumeRsp = g_setterTrace->metaConsumeRsp;
        prevClearTemp = g_setterTrace->clearTemp;
        prevClearReturn = g_setterTrace->clearReturn;

        Sleep(200);
    }
    return 0;
}
