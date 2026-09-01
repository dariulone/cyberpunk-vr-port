// LocateCamera -- one hook, one file.
//
// Where the engine locates the camera. This composes the heading and publishes it, and it
// reads its clean base from g_engineCamQuat -- the quaternion PatchCamera snapshotted
// BEFORE its own write, last frame. Reading the camera's own current quaternion instead is
// the documented way to make the camera spin up without bound from the smallest head turn:
// our write feeds our next base, and the loop has no fixed point.
//
// INSTALL ORDER IS EXACTLY WHAT IT WAS: Locate 10, Patch 12, Final 14, all in Stage::Boot. An
// adversarial pass over the plan for this split proposed reordering Patch before Locate for a
// "single-meaning" startup signal, and the payoff does not exist: g_engineCamQuatValid is set only
// when camKind == 1, which needs ClassifyPatchCameraOwner to have self-calibrated its name offset,
// which cannot happen until MAIN's placed component passes the site. The fallback stays an ordinary
// startup transient in BOTH orders. The two windows being weighed are the gap between consecutive
// FindPattern calls inside one function -- microseconds. An unobservable change is not a safe
// change; it is an unfalsifiable one, so the order is preserved.

#include "Camera/CameraLink.hpp"
#include "Camera/CameraState.hpp"
#include "Anim/CharacterRig.hpp"  // g_VrikFrameEpoch: exact camera/entity frame pairing
#include "Utils/LogThrottle.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

// The locate callback refreshes the player-state flags from RTTI, so it needs the script
// bindings as well as the Win32 headers.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Functions.hpp>

#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstddef>
#include <mutex>
#include "Overlay/LiveControlsUi.hpp"   // AdsCameraTelemetryUiState

// MAIN's live ADS magnification, recovered from its own projection by the stereo module.
extern "C" float CyberpunkVR_MainAdsZoomFactor;

// ---- ADS: THE WEAPON LAYER AND THE WORLD MAGNIFIED BY DIFFERENT AMOUNTS ------------------------
//
// Ported from dabinn's TofuExpress (4f676e33). CP2077 renders the world and the first-person
// weapon/body layer with INDEPENDENT ADS zooms. On a flat screen that is invisible, because the
// sight is held at the centre of the screen where both projections agree. In VR the sight is held
// off-axis in front of one eye, and two magnifications put it in two different screen positions --
// which is the reticle and scope glass drifting away from where the bullet actually goes.
//
// The fix drives the WEAPON override from MAIN's live world magnification while aiming, and restores
// the engine's own pair on the way out. MAIN/world projection is left completely alone: it is the
// reference, not the thing being corrected.
//
// NO __try/__except, which is where this departs from the original. This project has measured that
// __try does NOT protect a bad access in Cyberpunk -- REDEngine's vectored handler takes the
// exception before the frame-based handler is ever consulted -- so the guard is the one that works
// here: probed reads and writes, plus a plausibility gate. The gate matters twice over because these
// two offsets were found on someone else's build: a weight that is not in [0,1] or a zoom that is
// not a positive finite number means the field is not the field, and the honest response is to leave
// the engine alone and say so once, rather than to write a float into whatever is really there.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_AdsWeaponZoomSync = 1;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugAdsZoomWrites = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugAdsZoomRejects = 0;

// The telemetry snapshot lives here, next to the only place that can produce it: this hook is where
// the engine's own camera residual is visible before VR adds anything to it.
static std::mutex g_adsCameraTelemetryMutex;
static AdsCameraTelemetryUiState g_adsCameraTelemetry{};

extern "C" void GetAdsCameraTelemetryUiState(AdsCameraTelemetryUiState* outState) {
    if (!outState) return;
    std::lock_guard<std::mutex> lock(g_adsCameraTelemetryMutex);
    *outState = g_adsCameraTelemetry;
}

// BOTH EYES, and that is a correction to the original rather than a copy of it. The fields are on
// the CAMERA OBJECT, and this port has two of them -- MAIN's and VRCAM's, classified apart by the
// PatchCamera owner test. Forcing the weapon layer's magnification on MAIN alone would leave the
// second eye rendering the weapon at the vanilla zoom, i.e. the gun a different size in each eye
// while aiming. On a flat screen that failure mode does not exist; in a headset it is the worst kind,
// because the two images cannot be fused and the brain picks one.
static void SyncAdsWeaponZoomOneCamera(uintptr_t camObj, bool aiming, bool prevAiming,
                                       uintptr_t* savedCam, float* savedWeight, float* savedValue,
                                       bool* saved) {
    if (!camObj) return;
    const uintptr_t weightAddr = camObj + 0x280;
    const uintptr_t valueAddr  = camObj + 0x284;

    if (aiming) {
        // Capture the engine's own pair once per aim-in, so leaving ADS can put back exactly what
        // was there rather than a guessed neutral value.
        if (!prevAiming || !*saved || *savedCam != camObj) {
            float w = 0.0f, v = 0.0f;
            // ZERO IS ALLOWED for the value. The pair is (weight, zoom) and the weight is what arms
            // it, so an inactive override may well carry a zero, unset zoom -- refusing that would
            // have made this whole fix inert on exactly the weapons that need it, and the only sign
            // would have been one log line nobody was looking for.
            if (ReadFloatSafe(weightAddr, &w) && ReadFloatSafe(valueAddr, &v) &&
                std::isfinite(w) && std::isfinite(v) &&
                w >= -0.01f && w <= 1.01f && v >= 0.0f && v < 64.0f) {
                *savedCam = camObj;
                *savedWeight = w;
                *savedValue = v;
                *saved = true;
            } else {
                ++CyberpunkVR_DebugAdsZoomRejects;
                static bool s_toldZoomPair = false;
                if (!s_toldZoomPair) {
                    s_toldZoomPair = true;
                    if (g_verboseLog) Log("[ads] the weapon-zoom pair at camObj+0x280/+0x284 does not look like "
                    "(weight, zoom) -- read %.4f / %.4f. Not writing; set "
                    "CyberpunkVR_AdsWeaponZoomSync=0 to silence, or re-verify the offsets "
                    "against this build.\n", w, v);
                }
                return;
            }
        }
        const float worldZoom = CyberpunkVR_MainAdsZoomFactor;
        if (*saved && std::isfinite(worldZoom) && worldZoom > 0.5f && worldZoom < 12.0f) {
            if (WriteFloatSafe(weightAddr, 1.0f) && WriteFloatSafe(valueAddr, worldZoom)) {
                ++CyberpunkVR_DebugAdsZoomWrites;
            }
        }
    } else if (prevAiming && *saved && *savedCam == camObj) {
        WriteFloatSafe(weightAddr, *savedWeight);
        WriteFloatSafe(valueAddr, *savedValue);
        *saved = false;
    }
}

static void SyncAdsWeaponZoomToWorld() {
    static uintptr_t s_savedCam[2] = {0, 0};
    static float s_savedWeight[2] = {0.0f, 0.0f};
    static float s_savedValue[2] = {1.0f, 1.0f};
    static bool  s_saved[2] = {false, false};
    static bool  s_prevAiming = false;

    if (!CyberpunkVR_AdsWeaponZoomSync) { s_prevAiming = g_isAiming; return; }

    const bool aiming = g_isAiming;
    const uintptr_t cams[2] = { g_camObjMain.load(std::memory_order_acquire),
                                g_camObjVrcam.load(std::memory_order_acquire) };
    // Per-camera saved state, because the two objects can be latched at different times and can hold
    // different vanilla values -- restoring one eye's pair into the other would be a fresh bug.
    for (int i = 0; i < 2; ++i) {
        SyncAdsWeaponZoomOneCamera(cams[i], aiming, s_prevAiming,
                                   &s_savedCam[i], &s_savedWeight[i], &s_savedValue[i],
                                   &s_saved[i]);
    }
    s_prevAiming = aiming;
}

// The second eye's transform pushed through the engine's own notify -- see the note at the call site
// and CyberpunkVR_BdPushTransform in VrCore.cpp. Its own function because __try cannot live in a
// frame that needs C++ unwinding, and because the callback below has plenty of that.
// The rotation composed earlier in the call, held until MAIN's own position for the frame is known.
// A plain local would do if the push happened where it is composed; it does not, and this keeps the
// two halves of one frame's pose together rather than recomputing either.
float g_bdPushQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };

void BdPushTransformOnce(uintptr_t comp, float qx, float qy, float qz, float qw,
                                bool secondEye, const int32_t* base) {
    if (comp <= 0x10000) return;
    __try {
        uint8_t* c = reinterpret_cast<uint8_t*>(comp);
        const float hq[4] = { qx, qy, qz, qw };
        if (!IsPlausibleUnitQuaternion(hq)) return;
        float r[3] = {};
        ComputeRightVectorFromQuaternion(hq, r);
        const bool haveRight = IsPlausibleUnitVector3(r);
        const float half = CyberpunkVR_IpdInWorldPos ? GetDesiredHalfIpd() : 0.0f;
        // The two eyes take opposite halves about the scene camera, and the swap that makes MAIN the
        // right eye turns both round -- the same expression PatchCamera uses, sign for sign.
        const float sgn0 = secondEye ? +1.0f : -1.0f;
        const float sign = CyberpunkVR_MainIsRightEye ? -sgn0 : sgn0;
        int32_t* p = reinterpret_cast<int32_t*>(c + 0xE0);
        for (int i = 0; i < 3; ++i) {
            int32_t v = (CyberpunkVR_BdPushBase && base)
                            ? base[i]
                            : g_bdScenePosFP[i].load(std::memory_order_relaxed);
            if (haveRight && half != 0.0f)
                v += static_cast<int32_t>(r[i] * half * sign * 131072.0f);
            p[i] = v;
        }
        float* q = reinterpret_cast<float*>(c + 0xF0);
        q[0] = hq[0]; q[1] = hq[1]; q[2] = hq[2]; q[3] = hq[3];
        // The engine's own "the transform changed" call: rcx = component, rdx = component+0x100,
        // exactly as exe+0x1D8AE4..AF1 issues it.
        const uintptr_t vt = *reinterpret_cast<uintptr_t*>(c);
        if (vt <= 0x10000) return;
        using NotifyFn = void (__fastcall*)(void*, void*);
        NotifyFn notify = *reinterpret_cast<NotifyFn*>(vt + 0x240);
        if (notify) {
            notify(c, c + 0x100);
            if (secondEye) ++CyberpunkVR_DebugBdPushTransform;
            else           ++CyberpunkVR_DebugBdPushMain;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

extern "C" void __fastcall OnLocateCameraCallback(float* rbxPtr, float xmm0_val) {
    (void)xmm0_val;
    g_locateCameraHits++;
    if (g_telemetry) {
        g_telemetry->locateHits = static_cast<uint32_t>(g_locateCameraHits);
        g_telemetry->locateRbx = reinterpret_cast<uint64_t>(rbxPtr);
        g_telemetry->locateXmm0 = xmm0_val;
    }
    if (!rbxPtr || reinterpret_cast<uintptr_t>(rbxPtr) < 0x10000) return;

    int32_t* posFP = reinterpret_cast<int32_t*>(rbxPtr);
    float* quat = reinterpret_cast<float*>(rbxPtr + 4); // +16 bytes = +4 floats

    float dummy;
    if (!ReadFloatSafe(reinterpret_cast<uintptr_t>(quat), &dummy)) return;
    // THE ENGINE'S OWN ROTATION FOR THIS CAMERA, taken before a single field is written. In a
    // braindance this is the scene camera's rotation for the frame, and it is the base the head is
    // composed onto -- see CyberpunkVR_BdBaseFromLocate. Captured here because this hook writes that
    // same quaternion further down; read afterwards it would be our own output.
    const float bdEntryQuat[4] = { quat[0], quat[1], quat[2], quat[3] };

    // Raw ENGINE view at entry (yaw + pos), for the [dx-win] snap-window diag.
    g_dbgEntryYaw = atan2f(2.0f * (quat[3] * quat[2] + quat[0] * quat[1]),
                           1.0f - 2.0f * (quat[1] * quat[1] + quat[2] * quat[2]));
    g_dbgEntryPosX = static_cast<float>(posFP[0]) / 131072.0f;
    g_dbgEntryPosY = static_cast<float>(posFP[1]) / 131072.0f;
    g_dbgEntryPosZ = static_cast<float>(posFP[2]) / 131072.0f;

    // The real gameplay camera is heap-backed. The juddery second bake came from
    // transient camera transforms built on the current thread stack, so reject those.
    {
        const NT_TIB* tib = reinterpret_cast<const NT_TIB*>(NtCurrentTeb());
        const uintptr_t cp  = reinterpret_cast<uintptr_t>(rbxPtr);
        const uintptr_t sLo = reinterpret_cast<uintptr_t>(tib->StackLimit);
        const uintptr_t sHi = reinterpret_cast<uintptr_t>(tib->StackBase);
        if (cp >= sLo && cp < sHi) {
            static uint32_t s_scRej = 0;
            static uint64_t s_scMs = 0;
            ++s_scRej;
            const uint64_t scNow = GetTickCount64();
            if (s_scMs == 0) s_scMs = scNow;
            if (scNow - s_scMs >= 1000) {
                if (g_verboseLog) Log("[STACKCAM] rejected %u/s stack-temp camera locates (the foreign second bake)\n", s_scRej);
                s_scRej = 0;
                s_scMs = scNow;
            }
            return;
        }
    }

    SyncAdsWeaponZoomToWorld();

    // 1. Inizializza la cache RTTI solo al primissimo frame
    if (!g_isRTTIInitialized) {
        InitializeMountedVehicleCache();
    }

  
    // 2. Player-state refresh (in-vehicle / aiming / weapon flags). PERF (audit,
    // session 3): GetPlayer + 3 RTTI property reads used to run on EVERY locate
    // call (2-3+ per frame). These are gameplay-rate flags, so refresh them once
    // per entity tick (Lua push seq [99] bump), with an every-32nd-call fallback
    // for sessions where the VRIK Lua entity push is not running.
    {
        static float s_lastEntSeqForPlayer = -1.0f;
        bool refreshPlayer = ((g_locateCameraHits & 31) == 0);
        if (float* shSeq = GetShotShared()) {
            const float seq = shSeq[99];
            if (seq != s_lastEntSeqForPlayer) { s_lastEntSeqForPlayer = seq; refreshPlayer = true; }
        }
        if (refreshPlayer) {
            RED4ext::ScriptGameInstance gameInstance;
            RED4ext::Handle<RED4ext::IScriptable> playerHandle;
            RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);

            if (playerHandle && g_mountedVehicleProp) {
                auto mountedVehicle = g_mountedVehicleProp->GetValue<RED4ext::WeakHandle<RED4ext::IScriptable>>(playerHandle.instance);
                g_isInVehicle = (mountedVehicle.instance != nullptr);
            }

            // DRIVER SEAT, not just mounted. Only the driver has a wheel (or handlebars) in front
            // of them, and the wheel grab hands the arms back to the driving animation -- which is
            // the wrong pose for every other seat. Static call, null instance.
            bool driving = false;
            if (g_isInVehicle) {
                if (g_isDriverFunc && playerHandle) {
                    RED4ext::ScriptGameInstance gi;
                    bool isDriver = false;
                    RED4ext::StackArgs_t args;
                    args.emplace_back(nullptr, &gi);
                    args.emplace_back(nullptr, &playerHandle);
                    // The cast picks the (void* instance) overload: a bare nullptr is ambiguous
                    // against the (CClass* context) one, and a static function wants no instance.
                    if (RED4ext::ExecuteFunction(static_cast<void*>(nullptr), g_isDriverFunc, &isDriver, args))
                        driving = isDriver;
                } else {
                    driving = true;   // no IsDriver -> any seat, see InitializeMountedVehicleCache
                }
            }
            g_isDriving.store(driving, std::memory_order_relaxed);

            if (playerHandle && g_isAimingProp) {
                g_isAiming = g_isAimingProp->GetValue<bool>(playerHandle.instance);
            }

            if (playerHandle && g_equippedWeaponProp) {
                auto equippedWeapon = g_equippedWeaponProp->GetValue<RED4ext::WeakHandle<RED4ext::IScriptable>>(playerHandle.instance);
                g_hasWeaponEquipped = (equippedWeapon.instance != nullptr);
            }

            // Weapon flag lives in [144]. It used to be written to [126], COLLIDING with
            // the OpenXR HMD position publish ([124..126] -- [126] is the HMD Z!) that
            // VRIK reads as its head base and the overlay laser gate read as a weapon
            // flag (audit find).
            OpenXRManager::Get().SetSharedSlot(144, g_hasWeaponEquipped ? 1.0f : 0.0f);
            // In-vehicle flag [31]: the VRIK hook disables the whole BODY chain
            // (PlaceBodyUnderHMD / torso dampen / girdle pins / legs) while seated --
            // the vehicle drives the puppet, body IK fights it and breaks the
            // character/camera position. Arms-only in vehicles.
            OpenXRManager::Get().SetSharedSlot(31, g_isInVehicle ? 1.0f : 0.0f);
            // CUTSCENE SUSPEND, producer half (PR #40 in substance, RTTI instead of CET).
            //
            // The player's own scene tier, read as a property: PlayerPuppet carries
            // `sceneTier : GameplayTier` (0 = Tier1_FullGameplay .. 3 = Tier4_FPPCinematic,
            // 4 = Tier5_Cinematic), verified by RTTI dump rather than assumed. The upstream PR had
            // the VRIK CET mod walk the PlayerStateMachine blackboard and push the number through a
            // native; this reads it beside the three player flags above and needs no script tick.
            //
            // IT GOES IN A GLOBAL, NOT A SHARED SLOT, and that is the bug fix. Published first into
            // slots [157]/[158] it measurably never arrived: XInput.cpp writes those same two slots
            // every input tick with the right-B and left-Y pressed flags, so both the tier and the
            // threshold were overwritten with 0 before the pose hook could read them. The slot map
            // exists to cross to the CET mods; nothing that stays inside this DLL belongs in it.
            // (An earlier note here blamed SetSharedSlot for writing a different pointer. That was
            // wrong: the write landed, the button publish erased it.)
            if (playerHandle && g_sceneTierProp) {
                // GameplayTier is int32_t in the SDK, but read by the property own size so a future
                // widening cannot silently read three bytes of something else.
                const void* tp = g_sceneTierProp->GetValuePtr<void>(playerHandle.instance);
                const uint32_t sz = g_sceneTierProp->type ? g_sceneTierProp->type->GetSize() : 4u;
                int tier = 0;
                if (tp) {
                    if (sz == 1)      tier = *static_cast<const int8_t*>(tp);
                    else if (sz == 2) tier = *static_cast<const int16_t*>(tp);
                    else              tier = *static_cast<const int32_t*>(tp);
                }
                g_sceneTier.store(tier, std::memory_order_relaxed);
            }
        }
    }
    

    // SNAP HOLDBACK REMOVED. It held the view yaw one snap-delta back for the locates of a snap
    // tick, arming off a counter in shared[147] and reading [146]/[149]/[99] beside it. Two things
    // retire it: the hold itself was already disabled (s_hbLeft = 0, so snapHoldYaw never left
    // zero and the premultiply below was dead code), and the snap-turn model it compensated for is
    // gone. What remains of it here would be a shared-memory read per locate feeding an
    // always-zero correction, plus a burst log. Deleted whole, along with g_snapHold141.

    // SPRINT-START JERK DETECTOR (temporary diag, option C). The Aim_JNT FULL freeze
    // did NOT kill the sprint-start head jerk -> it is NOT rig camera-bone animation.
    // Remaining suspect: the engine camera SYSTEM itself (procedural sprint offset /
    // camera following the leaning spine), which passes 1:1 into the rendered view
    // because the view translation base is RAW LOCATED. Measure it: dev = located -
    // (tickEntity + cleanPair). cleanPair is EXACTLY (0,0,1.6) through sprint (proven),
    // so dev isolates whatever the engine adds on top of the clean head anchor.
    // Sampled once per entity tick (first locate after the [99] bump -> the v*dt
    // render-vs-tick skew stays roughly constant sample-to-sample). Windows arm on:
    // speed crossing UP through 4 m/s (sprint engage), DOWN through 3.5 m/s (sprint
    // stop), or a vertical dev jump > 8 mm/tick (velocity-free kick channel).
    // Profile answers: magnitude, direction, duration, and whether dev RETURNS to
    // baseline (transient kick) or SETTLES at an offset (sprint lean) -- each implies
    // a different fix.
    {
        float* shJ = GetShotShared();
        static float s_jkLastTick = -1.0f;
        static float s_jkPrevDev[3] = { 0.0f, 0.0f, 0.0f };
        static bool  s_jkPrevValid = false;
        static float s_jkPrevSp2 = 0.0f;
        static int   s_jkWin = 0;
        static uint64_t s_jkLastArmMs = 0;
        if (shJ && !g_isInVehicle && shJ[131] != 0.0f) {
            const float tickNow = shJ[99];
            if (tickNow != s_jkLastTick) {
                s_jkLastTick = tickNow;
                const float devX = g_dbgEntryPosX - (shJ[96] + shJ[128]);
                const float devY = g_dbgEntryPosY - (shJ[97] + shJ[129]);
                const float devZ = g_dbgEntryPosZ - (shJ[98] + shJ[130]);
                const float sp2 = shJ[132] * shJ[132] + shJ[133] * shJ[133];
                const uint64_t nowMs = GetTickCount64();
                const char* why = nullptr;
                if (s_jkPrevSp2 < 16.0f && sp2 >= 16.0f)        why = "SPRINT-ENGAGE";
                else if (s_jkPrevSp2 > 12.25f && sp2 <= 12.25f) why = "SPRINT-STOP";
                else if (s_jkPrevValid) {
                    const float dz = devZ - s_jkPrevDev[2];
                    if ((dz > 0.008f || dz < -0.008f) && nowMs - s_jkLastArmMs > 1000)
                        why = "Z-KICK";
                }
                s_jkPrevSp2 = sp2;
                if (why && s_jkWin == 0) {
                    s_jkWin = 45;
                    s_jkLastArmMs = nowMs;
                    if (g_verboseLog) Log("[jerk] ARM(%s) speed=%.2f dev=(%.4f,%.4f,%.4f)\n",
                        why, sqrtf(sp2), devX, devY, devZ);
                }
                s_jkPrevDev[0] = devX; s_jkPrevDev[1] = devY; s_jkPrevDev[2] = devZ;
                s_jkPrevValid = true;
                if (s_jkWin > 0) {
                    --s_jkWin;
                    // FOV branch: origFov = what the game last TRIED to set (a sprint
                    // FOV boost shows here even though the hook flattens it); storedH =
                    // the actual +0x410 the render uses (must stay pinned to the lens).
                    float storedH = 0.0f;
                    if (void* cs = g_dbgFovCamState)
                        ReadFloatSafe(reinterpret_cast<uintptr_t>(cs) + 0x410, &storedH);
                    // Same shape as [dx-win], and the biggest single source in vr_core: ~590
                    // lines a session across its value variants.
                    LOG_THROTTLED(3000, "[jerk] ms=%llu tick=%.0f dev=(%.4f,%.4f,%.4f) v=(%.2f,%.2f) origFov=%.3f storedH=%.3f\n",
                        (unsigned long long)nowMs, tickNow,
                        devX, devY, devZ,
                        shJ[132], shJ[133],
                        g_dbgLastOriginalFov, storedH);
                }
            }
        }
    }

    float camera_qx = quat[0];
    float camera_qy = quat[1];
    float camera_qz = quat[2];
    float camera_qw = quat[3];

    // SKIP-HMD test (decoupled-aim experiment): the plugin publishes a shot-frame flag
    // [57] and a master mode [58] to shared mem. mode 1 = always skip the HMD orientation
    // overwrite (view follows the game's stick/mouse aim, no head); mode 2 = skip only on
    // the shot frame (let the engine's native snap-to-aim through -> bullet should follow
    // AIM not the head). When skipping, we leave the game's camera quat untouched.
    bool skipHmdOrientation = false;
    if (float* sh = GetShotShared()) {
        const uint32_t mode = reinterpret_cast<volatile uint32_t*>(sh)[58];
        const uint32_t shotFrame = reinterpret_cast<volatile uint32_t*>(sh)[57];
        if (mode == 1u) skipHmdOrientation = true;
        else if (mode == 2u && shotFrame != 0u) skipHmdOrientation = true;
    }
    // Menu stability: in a full-screen menu (e.g. the world map),
    // do NOT drive the game camera with the HMD orientation, otherwise the menu/
    // map SWIMS as you turn your head. Leave the game camera quat untouched so the
    // menu view stays put. Detection: the native menu-mode hook OR the redscript
    // world-map bridge flag (shared slot [81]) for menus the native hook misses.
    bool menuOpen = (g_menuModeValue != 0);
    if (!menuOpen) {
        if (float* sh = GetShotShared()) {
            if (reinterpret_cast<volatile uint32_t*>(sh)[81] != 0u) menuOpen = true;
        }
    }
    if (menuOpen) skipHmdOrientation = true;

    // The heading base must be a value WE NEVER WROTE.
    //
    // camera_q* is the camera's current orientation, and once PatchCamera writes the camera
    // object that orientation is ours, HMD rotation included. Deriving the heading from it
    // feeds our own yaw back in every frame and the camera spins up without bound. So when the
    // Patch writer owns the camera, take the base from the snapshot PatchCamera captured
    // before its overwrite; it lags by at most one frame, which a heading cannot notice.
    float baseQx = camera_qx;
    float baseQy = camera_qy;
    float baseQz = camera_qz;
    float baseQw = camera_qw;
    if (CyberpunkVR_CamWriteInPatch && g_engineCamQuatValid) {
        baseQx = g_engineCamQuat[0];
        baseQy = g_engineCamQuat[1];
        baseQz = g_engineCamQuat[2];
        baseQw = g_engineCamQuat[3];
    }

    // In this camera path the game-local basis is effectively:
    // X = right, Y = forward, Z = up.
    // The standard quaternion basis formulas assume X = right, Y = up, Z = forward,
    // so the produced "up" vector is the game's forward, and the produced "forward"
    // vector is the game's up.
    float bodyGameForwardX = 2.0f * (baseQx * baseQy - baseQz * baseQw);
    float bodyGameForwardY = 1.0f - 2.0f * (baseQx * baseQx + baseQz * baseQz);

    // FROM HERE ON THIS IS THE VIEW'S FORWARD, NOT THE BODY'S -- and that distinction is the whole
    // bug the physical body rotation had.
    //
    // The vector above comes from the camera component, which inherits the ENTITY's yaw, so with the
    // body follower on it carries the realign we injected into the engine's heading. Everything this
    // hook derives from it describes the VIEW, not the body: the composed camera quaternion (which is
    // also published as the render-view pose the solve builds its hand targets from), the heading in
    // [141], the level matrix that takes the player's room position into the world, and the controller
    // aim quaternion for the shot. The rendered camera itself is composed in PatchCamera from
    // (engine yaw - realign), so leaving the realign in here made the PUBLISHED view orientation
    // disagree with the DRAWN one by exactly that angle.
    //
    // What that looked like: the hand offset from the head came out rotated about the vertical by the
    // realign, so at 92.7 deg of body turn the avatar's hand sat beside and behind the headset while
    // the controller was in front of the chest. Measured to the degree -- the body's transforms all
    // carried +50.69 (the same value the solve converted with, so the frames agreed) while the
    // published view carried +50.69 too, when the drawn view was at -42.00.
    //
    // Taking it out once, here, fixes every consumer at the source instead of one at a time.
    if (CyberpunkVR_BodyYawRealignRad != 0.0f) {
        const float ra = -CyberpunkVR_BodyYawRealignRad;
        const float cr = cosf(ra), sr = sinf(ra);
        const float fx = bodyGameForwardX * cr - bodyGameForwardY * sr;
        const float fy = bodyGameForwardX * sr + bodyGameForwardY * cr;
        bodyGameForwardX = fx;
        bodyGameForwardY = fy;
    }



    // POSE PAIR LOCKING: fetch the render eye FIRST, then take a pair-locked head
    // pose — eye0 samples live + freezes, eye1 replays eye0's pose. Both eyes of
    // the stereo pair therefore drive the camera (and below, VRIK) from ONE head
    // pose, so the engine's IK/skeleton stops rebuilding between the ~11 ms-apart
    // left/right renders (the body/hands jitter seen even on the flat mirror).
    OpenXRHeadPose xrPose{};
    const int renderEye = OpenXRManager::Get().GetCurrentRenderEyeIndex();
    // POSE PAIR LOCKING: in AER, READ the frozen snapshot the engine ALREADY built
    // this pair's skeleton from (published in OnPresent at the pair boundary, before
    // the animation pass). LocateCamera runs DURING render, AFTER animation, so it
    // must NOT re-sample — the camera view must match the body the plugin already
    // posed. In mono there is no pairing, so sample live (no added latency).
    // xr_pair_lock (vrport.ini): 0 disables the pose-pair-lock and samples the LIVE
    // head pose every camera-locate instead of the per-pair frozen snapshot, trading
    // pair-consistent body alignment for a small per-eye skeleton tear.
    // REVERTED (user order): live sampling exactly as the long-tested build. The
    // one-sample-per-frame boundary freeze (20:08) did not remove the hand trail
    // and made the snap double WORSE (frozen heading delayed the view a frame
    // behind the game world). AER keeps the pair-locked snapshot; mono samples live.
    // THE FRAME'S ONE HEAD SAMPLE -- the same struct PatchCamera composes the orientation from.
    //
    // This used to be GetHeadPose(), the cache the frame loop refreshes once per cycle and runs
    // through the adaptive smoother. The POSITION below is built from it, while the orientation
    // was already coming from a fresh unfiltered locate at the write site, so the rendered eye
    // sat at a lagging, motion-dependent place while looking in the current direction -- and the
    // layer was labelled with a third sample again. One sample removes all three disagreements
    // at once. See AcquireFrameHeadSample.
    const bool hasXR = CyberpunkVR_OneSamplePerFrame
        ? OpenXRManager::Get().AcquireFrameHeadSample(&xrPose)
        : OpenXRManager::Get().GetHeadPose(&xrPose);
    const bool composeAtWrite = (CyberpunkVR_CamWriteInPatch && CyberpunkVR_CamComposeAtWrite);
    if (hasXR) {
        OpenXRManager::Get().ApplyViewBoxPitch(&xrPose);
        // Hand the EXACT sample this frame's camera is built from to the submit path, so
        // the image is labelled with the pose it was rendered from instead of whatever the
        // pose cache holds by the time it reaches Present. See SetPendingRenderHeadPose.
        //
        // ONLY when this site is the one that composes what gets written. Under compose-at-
        // write the write site publishes instead, and publishing from both would let whichever
        // ran last label the image with a pose that was never written into the camera -- which
        // is precisely the mismatch the compositor turns into judder.
        // ...AND WHEN THE BRAINDANCE PUSH OWNS THE COMPOSITION, THIS SITE OWNS THE LABEL TOO.
        // The pixels are then drawn from the rotation composed here, so the frame has to be
        // labelled with the sample it came from. Leaving the label to the write site meant the
        // compositor reprojected against a pose the image was never rendered from -- invisible
        // on the flat monitor, which has no compositor, and a rare jerk in the headset.
        if (!composeAtWrite || BdPushOwnsComposition()) {
            OpenXRManager::Get().PushRenderHeadPose(xrPose);
        }

        uint32_t currentSeq = g_lastLocateSeq + 1;
        if (float* sh = GetShotShared()) {
            // [94] current render eye for CET/Lua, [95] desired half IPD.
            sh[94] = static_cast<float>(renderEye);
            sh[95] = GetDesiredHalfIpd();
        }
        OpenXRManager::Get().StoreRenderEyePose(0, xrPose, currentSeq);
        OpenXRManager::Get().StoreRenderEyePose(1, xrPose, currentSeq);

        // NOTE: shared-memory hands/head ([0..19],[89],[90]) are NO LONGER flushed
        // here. The VRIK plugin reads them during the engine's ANIMATION pass, which
        // runs BEFORE this render hook — flushing here landed one stage too late and
        // tore the skeleton across the eye pair. They are now published in OnPresent
        // at the pair boundary (UpdatePairLock + FlushHandsToShared), before the next
        // pair's animation.

        // Mouse/controller yaw is always the body heading. The game's PITCH is added only when
        // the user has asked for it: by default the headset supplies vertical look, and mouse-Y
        // pitch also drags a constrained pivot offset that moves the head relative to the body.
        const float gameYaw = atan2f(-bodyGameForwardX, bodyGameForwardY);
        const float gamePitch = g_liveControls.xrDisableMouseY != 0 ? 0.0f : g_gamePitchRadians;
        const float cy = cosf(gameYaw * 0.5f);
        const float sy = sinf(gameYaw * 0.5f);
        const float pc = cosf(gamePitch * 0.5f);
        const float ps = sinf(gamePitch * 0.5f);

        // Publish the heading for the write site. This -- not the finished product -- is what
        // this hook is uniquely able to produce: it is the only place that has the body
        // forward, the recenter base and the physical-rotation realign. The HMD half is
        // multiplied in at the write, where it can be current.
        // ---- ADS CAMERA-PIVOT TELEMETRY (dabinn, TofuExpress f8a827eb) ------------------------
        //
        // What did the ENGINE do to the camera when the player raised the sights? Subtract the same
        // entity + clean-pair anchor the sprint-jerk detector above uses, so VR's own head
        // translation is not in the number, and rotate the residual into the heading's local
        // right/forward/up. Hip fire keeps refreshing the baseline; ADS freezes it and reports the
        // delta and the running peak against it. Tick-gated off [99], because this is a gameplay
        // quantity and sampling it per locate would just count locates.
        if (float* shAds = GetShotShared(); shAds && shAds[131] != 0.0f) {
            static float s_adsLastTick = -1.0f;
            static float s_adsBaseline[3] = {};
            static float s_adsPeak[3] = {};
            static bool  s_adsBaselineValid = false;
            static bool  s_adsPrevAiming = false;
            static unsigned int s_adsSamples = 0;
            const float tick = shAds[99];
            if (tick != s_adsLastTick) {
                s_adsLastTick = tick;
                const float dx = g_dbgEntryPosX - (shAds[96] + shAds[128]);
                const float dy = g_dbgEntryPosY - (shAds[97] + shAds[129]);
                const float dz = g_dbgEntryPosZ - (shAds[98] + shAds[130]);
                const float cyaw = cosf(gameYaw), syaw = sinf(gameYaw);
                const float local[3] = { cyaw * dx + syaw * dy, -syaw * dx + cyaw * dy, dz };

                if (!g_isAiming) {
                    s_adsBaseline[0] = local[0];
                    s_adsBaseline[1] = local[1];
                    s_adsBaseline[2] = local[2];
                    s_adsBaselineValid = true;
                    s_adsPeak[0] = s_adsPeak[1] = s_adsPeak[2] = 0.0f;
                    s_adsSamples = 0;
                } else {
                    if (!s_adsPrevAiming) {
                        s_adsPeak[0] = s_adsPeak[1] = s_adsPeak[2] = 0.0f;
                        s_adsSamples = 0;
                    }
                    ++s_adsSamples;
                }

                const float delta[3] = {
                    s_adsBaselineValid ? local[0] - s_adsBaseline[0] : 0.0f,
                    s_adsBaselineValid ? local[1] - s_adsBaseline[1] : 0.0f,
                    s_adsBaselineValid ? local[2] - s_adsBaseline[2] : 0.0f
                };
                if (g_isAiming) {
                    for (int i = 0; i < 3; ++i) {
                        const float a = fabsf(delta[i]);
                        if (a > s_adsPeak[i]) s_adsPeak[i] = a;
                    }
                }
                {
                    std::lock_guard<std::mutex> lock(g_adsCameraTelemetryMutex);
                    g_adsCameraTelemetry.available = 1;
                    g_adsCameraTelemetry.aiming = g_isAiming ? 1 : 0;
                    g_adsCameraTelemetry.baselineValid = s_adsBaselineValid ? 1 : 0;
                    g_adsCameraTelemetry.samples = s_adsSamples;
                    g_adsCameraTelemetry.residualRight = local[0];
                    g_adsCameraTelemetry.residualForward = local[1];
                    g_adsCameraTelemetry.residualUp = local[2];
                    g_adsCameraTelemetry.deltaRight = delta[0];
                    g_adsCameraTelemetry.deltaForward = delta[1];
                    g_adsCameraTelemetry.deltaUp = delta[2];
                    g_adsCameraTelemetry.peakRight = s_adsPeak[0];
                    g_adsCameraTelemetry.peakForward = s_adsPeak[1];
                    g_adsCameraTelemetry.peakUp = s_adsPeak[2];
                }
                s_adsPrevAiming = g_isAiming;
            }
        }

        g_headingSy = sy;
        g_headingCy = cy;
        g_headingPitchS = ps;
        g_headingPitchC = pc;
        g_headingValid = skipHmdOrientation ? 0u : 1u;
        CyberpunkVR_DebugTidLocateCam = GetCurrentThreadId();

        const float xrGameX = xrPose.oriX;
        const float xrGameY = -xrPose.oriZ;
        const float xrGameZ = xrPose.oriY;
        const float xrGameW = xrPose.oriW;

        // Camera = heading * FULL HMD orientation in EVERY on-foot mode. With physical
        // body rotation ON, body-realign (OnOnFootDeltaHead) turns the game HEADING only
        // on a PHYSICAL body turn and rotates the recenter base by the same angle, so a
        // head-only turn moves the VIEW but leaves the body/heading put. (The old unarmed
        // branch stripped the HMD yaw and glued the heading to it, which rotated the body
        // on every head turn -- replaced by the realign model.)
        // yaw * pitch * HMD. Pitch after yaw, so it tilts about the camera's own right axis
        // rather than a world axis; identity whenever the option leaves gamePitch at zero, which
        // makes the default path bit-identical to what it was.
        float headX, headY, headZ, headW;
        MulQuat(0.0f, 0.0f, sy, cy, ps, 0.0f, 0.0f, pc, headX, headY, headZ, headW);

        // IN A BRAINDANCE THE BASE IS THE SCENE'S OWN CAMERA, NOT THE HEADING.
        //
        // Writing the composed orientation from here made MAIN stop following the scene while VRCAM
        // kept following it -- reported exactly that way. The write was fine; the BASE was not. The
        // gameplay base is a heading (yaw*pitch) fed from g_engineCamQuat, and during a braindance
        // nothing turns that heading: the replay drives a virtual camera the port cannot reach as a
        // component (the only object within a metre of its published pose is a `Senses` component).
        // So composing onto the heading pinned MAIN to wherever the heading last was. VRCAM went on
        // following because its RTT component hangs off a parent the scene does move, and our delta
        // just rides on top of that.
        //
        // g_bdSceneQuat is that scene camera's orientation, published by the scene system itself
        // (RemoteCamera.cpp) in the same game axes as the camera descriptor -- it is the value the
        // braindance identification dots against, and it measured 0.9995 against the descriptor the
        // engine renders through. Using it as the base makes the view follow the replay again and the
        // head compose on top, which is what the old descriptor write did before it desynced the
        // matrices.
        if (g_bdActive.load(std::memory_order_relaxed) &&
                g_bdScenePoseValid.load(std::memory_order_acquire)) {
            // THE BUFFER'S OWN QUATERNION FIRST, the script's published pose as the fallback.
            //
            // Both are the same camera; they differ only in age. `quat` is what the engine put in
            // this buffer for the camera being located, this frame, and we have not written to it
            // yet -- the write is further down. g_bdSceneQuat comes from a Lua update and is a tick
            // behind, and composing a fresh head onto a stale base is a lag that catches up: judder
            // on head turns, in both eyes at once, since both are composed from this one value.
            // MODE 2 IS THE ONE THAT IS SAFE. Mode 1 took the buffer unconditionally, and the buffer
            // is the array THIS HOOK WRITES further down: on any call where the engine had not
            // refilled it, the base was our own previous output and the head was composed onto
            // itself. That is a feedback loop, it made the judder worse, and this codebase already
            // carried the warning -- reading our own output back as a base is what once made the
            // camera spin up without bound.
            //
            // Mode 2 asks the only question that separates the two cases: is what is in the buffer
            // the value WE last published? If it is, the engine has not written since and there is
            // nothing fresher here than the script's pose, so the script's pose is used. If it is
            // not, it is the engine's own value for this camera, this frame, and it is strictly
            // fresher than a pose published from a Lua tick.
            bool fromBuffer = false;
            if (CyberpunkVR_BdQuatFromBuffer) {
                const float l2 = quat[0]*quat[0] + quat[1]*quat[1] +
                                 quat[2]*quat[2] + quat[3]*quat[3];
                bool ours = false;
                if (CyberpunkVR_BdQuatFromBuffer >= 2 && g_headQuatValid) {
                    float d = quat[0]*g_headQuatComposed[0] + quat[1]*g_headQuatComposed[1] +
                              quat[2]*g_headQuatComposed[2] + quat[3]*g_headQuatComposed[3];
                    if (d < 0.0f) d = -d;
                    ours = (d > 0.99999f);
                }
                if (l2 > 0.9f && l2 < 1.1f && !ours) {
                    headX = quat[0]; headY = quat[1]; headZ = quat[2]; headW = quat[3];
                    fromBuffer = true;
                    ++CyberpunkVR_DebugBdQuatFromBuffer;
                }
            }
            if (!fromBuffer) {
                // The engine's value for this frame first, the script's published pose as fallback.
                const float l2 = bdEntryQuat[0] * bdEntryQuat[0] + bdEntryQuat[1] * bdEntryQuat[1] +
                                 bdEntryQuat[2] * bdEntryQuat[2] + bdEntryQuat[3] * bdEntryQuat[3];
                if (CyberpunkVR_BdBaseFromLocate && l2 > 0.9f && l2 < 1.1f) {
                    headX = bdEntryQuat[0];
                    headY = bdEntryQuat[1];
                    headZ = bdEntryQuat[2];
                    headW = bdEntryQuat[3];
                    ++CyberpunkVR_DebugBdBaseFromLocate;
                } else {
                    headX = g_bdSceneQuat[0];
                    headY = g_bdSceneQuat[1];
                    headZ = g_bdSceneQuat[2];
                    headW = g_bdSceneQuat[3];
                    ++CyberpunkVR_DebugBdBaseFromScript;
                }
            }
        }
        float tmpX, tmpY, tmpZ, tmpW;
        MulQuat(headX, headY, headZ, headW, xrGameX, xrGameY, xrGameZ, xrGameW,
                tmpX, tmpY, tmpZ, tmpW);
        NormalizeQuat(tmpX, tmpY, tmpZ, tmpW);

        camera_qx = tmpX;
        camera_qy = tmpY;
        camera_qz = tmpZ;
        camera_qw = tmpW;

        // Publish the composed orientation for the PatchCamera writer.
        //
        // This is a DEDICATED global, deliberately not g_lastLocateQuat: that one mirrors the
        // serialiser buffer AFTER the write below, so the moment the write moves elsewhere it
        // starts reporting the engine's own value instead of ours -- which is how the camera
        // once stopped following the mouse. Published whenever we composed it, independent of
        // who ends up writing it.
        g_headQuatComposed[0] = camera_qx;
        g_headQuatComposed[1] = camera_qy;
        g_headQuatComposed[2] = camera_qz;
        g_headQuatComposed[3] = camera_qw;
        g_headQuatValid = skipHmdOrientation ? 0u : 1u;

        // (An attempt to write both cameras directly from here, through the cached pointers,
        // is deliberately NOT present. It was tried to lift the orientation off the engine's
        // update cadence, and it stopped VRCAM tracking altogether -- the second view's
        // transform is derived from its parent and the engine recomputes it, so a write placed
        // outside its own update does not survive. PatchCamera remains the only writer: it
        // runs immediately after the engine's own store, which is what makes it stick.)
        //
        // ...AND NOW THE REASON THAT ATTEMPT DID NOT SURVIVE IS KNOWN, so a narrow version of it is
        // here. The write did not fail; the NOTIFICATION was missing. The engine's setter compares the
        // new transform against the stored one and calls the component's vtable+0x240 only when they
        // differ (exe+0x1D8AF1) -- and that virtual is what tells the render side to rebuild the view.
        // A write placed outside the setter never reached it, so the fields changed and nothing read
        // them again.
        //
        // So: write the fields, then make the same call the setter makes, with the same arguments.
        // Only in a braindance, where the setter is measured at 3.4 calls a second because the body
        // the component hangs off does not move -- everywhere else it runs every frame and this would
        // be a second writer for no gain. See CyberpunkVR_BdPushTransform, which also records what
        // forcing the resulting flag directly cost.
        // (the push itself happens further down, once MAIN's own position for this frame is known --
        //  see CyberpunkVR_BdPushBase. The composed rotation is stashed for it here.)
        g_bdPushQuat[0] = camera_qx; g_bdPushQuat[1] = camera_qy;
        g_bdPushQuat[2] = camera_qz; g_bdPushQuat[3] = camera_qw;

        // AND THE RING ENTRY GOES WITH IT.
        //
        // With CyberpunkVR_PoseReadBack on, FinalCamera takes the render camera's quaternion and looks
        // it up in this ring to find which XR sample produced it; that sample becomes the pose the
        // frame is SUBMITTED with, per eye. So the ring has to be keyed on the value actually written
        // into the cameras. Once the composition moved here, PatchCamera was still filing its own --
        // the lookup then matched nothing or matched something stale, and both eyes were submitted
        // with a pose their pixels were never drawn from. Invisible on the flat monitor, which has no
        // compositor in the way, and a jerk in the headset whenever the scene was moving.
        //
        // Same rule as the frame label two hundred lines up: whoever composes files the entry.
        if (BdPushOwnsComposition()) {
            const float qr[4] = { camera_qx, camera_qy, camera_qz, camera_qw };
            cvr::camera::CamWriteQuatPublish(camera_qx, camera_qy, camera_qz, camera_qw);
            cvr::camera::CamWriteRecordPush(qr, xrPose);
        }
        // Skip the HMD orientation write on the shot frame (or always, mode 1) so the game's
        // native aim/snap drives the camera -> the bullet follows the controller/stick aim.
        //
        // CamWriteInPatch: LocateCamera COMPOSES, PatchCamera WRITES. This buffer is a
        // serialised copy of the camera description and the engine refills part of it after we
        // return, so a write here is only half-applied -- consumers that read the other
        // representation see an unrotated camera. PatchCamera writes the component's own
        // store, and it is the only site that can tell MAIN from VRCAM, which is what the
        // second view needs to track at all.
        // AND IN A BRAINDANCE, ALWAYS FROM HERE. Braindance has no camera component to write:
        // measured live, the only object within a metre of the scene camera's published pose is a
        // component named `Senses` (CName CAC5E62C55DDB77E, resolved through the game's own name
        // pool), 0.163 m away with fov 0 -- it shares the head's orientation because it sits on the
        // head, and it is not a camera. So PatchCamera's braindance search reporting found=0 is the
        // right answer, not a threshold that needs widening, and the scene drives a virtual camera
        // that no component write can reach.
        //
        // The previous attempt wrote the orientation far downstream, into the render descriptor built
        // by exe+0x4E4030 -- and one instruction too late, after sub_1401E412C had already derived
        // both of that descriptor's basis matrices and both translation rows from the OLD quaternion.
        // That is what shook and shimmered. This buffer is upstream of all of it.
        //
        // VRCAM is unaffected and stays on PatchCamera: the second view is not in the mixer table
        // ([director+0x48], see docs/cp2077-camera-write-chain.md section 6.4), so it never reaches
        // this serialiser, and PatchCamera remains the only site that can tell the two views apart.
        // ...AND ONLY WHILE THE SCENE OWNS THE CAMERA. The justification above -- "braindance has no
        // camera component to write" -- is true of PLAYBACK and false of the EDITOR, where the player
        // flies the player's own camera and that component is live again. Keyed on g_bdActive alone,
        // the editor also took this route, and this buffer is written half-applied by design (the
        // engine refills part of it after we return), so MAIN juddered on head turns there while VRCAM,
        // which is written at its component in PatchCamera, did not. Reported exactly that way.
        const bool bdNow = g_bdActive.load(std::memory_order_relaxed) &&
                           g_bdScenePoseValid.load(std::memory_order_acquire);

        // A TAKEOVER TAKES THE SAME ROUTE, with no braindance state in it.
        //
        // The engine renders the main view through the camera being located, and during a takeover that
        // camera is the turret's: measured, with this route on, the second eye's push fires from this very
        // call and its jitter disappears -- which it could only do if this buffer's position is the lens.
        // What was still missing is permission to put the orientation in: the write below is gated on
        // "compose-at-write is off, or a braindance is running", and a takeover is neither, so MAIN kept
        // the engine's own aim and only the second eye followed the head. Reported exactly that way.
        const bool takeoverNow = LocateOwnsTakeover();

        // AND WHAT GOES IN IS WHAT THE WRITE SITE COMPOSED, not what this site composed.
        //
        // Under compose-at-write PatchCamera composes and publishes the pose the image is LABELLED
        // with. In a braindance the picture for MAIN is written from here instead, out of a head
        // sample taken at this site -- so the pixels came from one sample and the label from
        // another, which is exactly the mismatch the comment above this block warns about and
        // exactly what the compositor turns into judder. It shows only in the headset (the flat
        // monitor has no reprojection to get wrong) and it is there with the replay PAUSED, because
        // it is driven by head motion alone. Both were reported.
        //
        // The second eye never had it: its orientation is written at PatchCamera, the same site that
        // publishes the label. Taking that same composed value here puts MAIN on it too -- one
        // sample, one label, both eyes. This is only correct now that PatchCamera composes onto the
        // SCENE's base in a braindance; before that its value carried the body heading, which a
        // braindance never turns.
        float bdWritten[4] = {};
        // ...and a takeover takes it on the same terms and for the same reason: PatchCamera composed the
        // lens yaw with the head for this frame and published that value, the second eye is written with
        // it, and MAIN must carry the identical quaternion or the two eyes diverge and reconverge on every
        // head turn. One composition, one head sample, one label.
        const bool bdUseWriteSite =
            (bdNow ? (CyberpunkVR_BdQuatFromWriteSite != 0) : takeoverNow) &&
            cvr::camera::CamWriteQuatRead(bdWritten) &&
            (bdWritten[0]*bdWritten[0] + bdWritten[1]*bdWritten[1] +
             bdWritten[2]*bdWritten[2] + bdWritten[3]*bdWritten[3]) > 0.9f;
        if (bdUseWriteSite) {
            camera_qx = bdWritten[0]; camera_qy = bdWritten[1];
            camera_qz = bdWritten[2]; camera_qw = bdWritten[3];
            g_headQuatComposed[0] = camera_qx; g_headQuatComposed[1] = camera_qy;
            g_headQuatComposed[2] = camera_qz; g_headQuatComposed[3] = camera_qw;
            ++CyberpunkVR_DebugBdQuatFromWriteSite;
        }
        if (!skipHmdOrientation && (!CyberpunkVR_CamWriteInPatch || bdNow || takeoverNow)) {
            quat[0] = camera_qx;
            quat[1] = camera_qy;
            quat[2] = camera_qz;
            quat[3] = camera_qw;
        }
    }

    // In a menu, also skip the HMD POSITION injection (not just orientation): the
    // map/menu must be a flat, static 2D panel. Moving the camera position with the
    // head shifts the rendered map background while its pins are projected for a
    // fixed position -> pins drift off the map.
    if (hasXR && !menuOpen) {
        // "Fix Head" (xr3DofMovement) is gone -- removed on the user's instruction, and it was
        // wrong on its own terms: it dropped the head translation AND every Tracking/Camera
        // offset with it, so the sliders it hid were the ones people needed. Positional
        // tracking is not an option in a 6DoF port; the honest knob is world scale, which
        // stays. The field is left in the settings struct so old ini files still parse, but
        // nothing reads it any more.
        const bool allowGameCameraTranslation = true;
        const float posScale = 1.0f * GetWorldScale();

        // Map OpenXR local position into game-local camera space first:
        // XR: X=right, Y=up, -Z=forward; game local: X=right, Y=forward, Z=up.
        // BAKED camera->head offset + Head sliders on top (sliders stay 0 after baking).
        // IN VEHICLE both bakes are DROPPED: they were measured on the standing body
        // (camera-mount vs foot centre / head bone); seated, the vehicle camera is
        // already correct and the baked shift just pushes the view off the seat.
        // The plugin mirrors this by not adding [91..93] to camModelPos in vehicle,
        // and [120..123] below carries the same (bake-less) total, so the hands stay
        // consistent with the view. Manual Tracking-Camera sliders stay live.
        float camBake[3] = { 0.0f, 0.0f, 0.0f };
        if (allowGameCameraTranslation && !g_isInVehicle) OpenXRManager::Get().GetCameraOffset(camBake);
        // THE IN-VEHICLE OFFSET, the other half of the same argument: the bakes above are dropped
        // when seated because they are standing measurements, and the manual Head sliders are the
        // same measurement by hand -- so the seat gets its own trio, added on top and live only
        // while mounted. Folded in HERE, with the bakes, so every consumer below (the three local
        // components AND the total published in [120..122] that the hand targets are kept
        // consistent with) sees one number and they cannot drift apart.
        float vehOff[3] = { 0.0f, 0.0f, 0.0f };
        if (allowGameCameraTranslation && g_isInVehicle) {
            vehOff[0] = g_liveControls.xrVehHeadOffsetX;
            vehOff[1] = g_liveControls.xrVehHeadOffsetY;
            vehOff[2] = g_liveControls.xrVehHeadOffsetZ;
        }
        // EYE-VIEW offset ("bake to eyes"): view-only, no feedback into the body solve.
        float eyeBake[3] = { 0.0f, 0.0f, 0.0f };
        if (float* shEye = GetShotShared()) {
            if (allowGameCameraTranslation) {
                if (!g_isInVehicle && shEye[119] == 1.0f) { eyeBake[0] = shEye[116]; eyeBake[1] = shEye[117]; eyeBake[2] = shEye[118]; }
                // Publish the TOTAL view offset actually applied ([120..123]) so hand
                // targets stay consistent with whatever the user tunes the view to.
                shEye[120] = g_liveControls.xrHeadOffsetX + camBake[0] + eyeBake[0] + vehOff[0];
                shEye[121] = g_liveControls.xrHeadOffsetY + camBake[1] + eyeBake[1] + vehOff[1];
                shEye[122] = g_liveControls.xrHeadOffsetZ + camBake[2] + eyeBake[2] + vehOff[2];
                shEye[123] = 1.0f;
            } else {
                shEye[123] = 0.0f;
            }
        }
        const float localRight = xrPose.posX * posScale +
            (allowGameCameraTranslation
                 ? (g_liveControls.xrHeadOffsetX + camBake[0] + eyeBake[0] + vehOff[0])
                 : 0.0f);
        const float localForward = -xrPose.posZ * posScale +
            (allowGameCameraTranslation
                 ? (g_liveControls.xrHeadOffsetY + camBake[1] + eyeBake[1] + vehOff[1])
                 : 0.0f);
        const float localUp = xrPose.posY * posScale +
            (allowGameCameraTranslation
                 ? (g_liveControls.xrHeadOffsetZ + camBake[2] + eyeBake[2] + vehOff[2])
                 : 0.0f);

        // Perfectly level heading matrix for translation (no sliding into the floor when pitched).
        //
        // THE HEAD IS ROTATED BY THE VIEW'S HEADING, NOT THE BODY'S, and the difference only appeared
        // once the body started turning on its own. This matrix takes the HMD's offset -- the player's
        // physical position in the room, relative to the recenter origin -- into world space. Built from
        // `bodyGameForward`, it is the ENTITY's yaw, which now carries the body-follow offset; so the
        // whole play space span with the body, the head landed somewhere else, and the hands (whose
        // world position is built from this very delta) rode along. Measured: entity/[141] at 29.35 deg
        // against an engine yaw of -37.86, i.e. 67 deg of play-space rotation nobody asked for.
        //
        // The view is composed from the ENGINE's yaw (CyberpunkVR_ViewYawFromEngine), so that is the
        // frame the head offset belongs in. The body's own yaw is still what the SOLVE converts
        // world->model with -- two consumers, two headings, and they are no longer the same number.
        float flatYaw = atan2f(-bodyGameForwardX, bodyGameForwardY);
        // ON FOOT ONLY -- see the twin gate in PatchCamera.cpp for the measurement. Mounted, the
        // body-yaw census does not describe the view heading and pins the driving view ~90 deg off.
        if (CyberpunkVR_ViewYawFromEngine && CyberpunkVR_EngineBodyYawValid && !g_isInVehicle) {
            float wz = CyberpunkVR_EngineBodyYawZ, ww = CyberpunkVR_EngineBodyYawW;
            if (ww < 0.0f) { wz = -wz; ww = -ww; }
            if (wz != 0.0f || ww != 0.0f) flatYaw = 2.0f * atan2f(wz, ww);
        }
        // MOUNTED, TAKE THE YAW THE VIEW WAS ACTUALLY COMPOSED WITH. The note above is explicit that this
        // matrix belongs in the VIEW's frame -- "two consumers, two headings, and they are no longer the
        // same number" -- and on foot the engine-yaw substitution above is what makes them the same. In a
        // vehicle nothing did: the view is composed from the camera's pre-write quaternion, assembled per
        // rendered frame, while bodyGameForward advances on the entity tick. While the car TURNS that
        // difference grows every frame and the eye is rotated by it -- jitter proportional to the turn
        // rate, absent parked and absent in a straight line, which is what was observed.
        //
        // PatchCamera publishes this at the instant it uses it and runs before this function in the frame,
        // so it is a same-frame number rather than a cached one.
        if (g_isInVehicle && CyberpunkVR_VehicleAnchorFromViewYaw && g_viewYawUsedValid) {
            flatYaw = g_viewYawUsedRad;
        }
        // (The realign is already out of bodyGameForward at the top of this function, so this matrix
        // is the VIEW's heading -- which is what takes the room position into the world without
        // swinging the play space every time the body comes around.)
        const float flatCy = cosf(flatYaw);
        const float flatSy = sinf(flatYaw);
        // Hand it to the hand publish so it can rebuild this same delta from ITS head sample.
        g_anchorOff[0] = localRight   - xrPose.posX * posScale;
        g_anchorOff[1] = localForward + xrPose.posZ * posScale;
        g_anchorOff[2] = localUp      - xrPose.posY * posScale;
        g_anchorCy = flatCy;
        g_anchorSy = flatSy;
        g_anchorScale = posScale;
        g_anchorRecipeValid = 1;
        const float worldDeltaX = flatCy * localRight - flatSy * localForward;
        const float worldDeltaY = flatSy * localRight + flatCy * localForward;
        const float worldDeltaZ = localUp;

        // WorldPosition fixed-point is int32 * (2<<16) = 131072 (17 fractional bits) --
        // CONFIRMED against RED4ext SDK WorldPosition.hpp after the published absolute
        // position measured EXACTLY 2x the real camera. The old 65536 multiplier injected
        // only HALF of every offset here (head translation 0.5:1, half-applied bakes and
        // sliders). Now 1:1: real meters in, real meters rendered.
        //
        // NATIVE-VR VIEW BASE (on foot). The user's directive, verbatim: "HMD = Camera,
        // перезаписывается каждый раз, игра не должна её трогать; всё идёт от HMD и
        // контроллеров как в нативных играх". So the main FPP camera's translation is
        // REPLACED outright: view = entity + clean pair + worldDelta -- the EXACT
        // expression the hand/body anchors use (ResolveViewPos), same push, same tick.
        // The engine's located translation (procedural lean/bob/kick/neck-pivot) does
        // not participate at all; the real head translation arrives via worldDelta.
        // No filters, no easing, no chase -- the previous per-tick 0.35 easing chase is
        // what produced the left-hand head-turn ghost. The located value is used ONLY
        // to IDENTIFY the main FPP camera (generous ball around entity+pair: kicks are
        // cm-scale; the armed AIM camera sits 0.3-0.4m BELOW and fails the qz band).
        // Vehicles / cinematics (pair stale or camera far): raw located + worldDelta,
        // nothing else.
        // VIEW TRANSLATION = raw located + worldDelta. THE FINAL BASE, reasoned:
        // the renderer places the SKELETON with a per-render-frame INTERPOLATED
        // entity transform; located is built from that same render-rate entity.
        // Any view term anchored to the TICK entity instead (the old stabilizer's
        // per-tick latched correction, then the synthetic entity+pair base) drifts
        // from the body by v*dt during locomotion -- THE strafe/sprint/shot body
        // shift, and the tick-vs-render beat was the walking body tremble. Sharing
        // located's render-rate base welds body and view by timeline. Residual:
        // the engine's input-driven camera lean (cm, plays even at v=0) -- to be
        // killed GAME-SIDE at the source (animgraph input, like the bobbing kill),
        // NOT compensated here. No filters, no synth, no tick anchors in the view.
        const int32_t deltaFPx = static_cast<int32_t>(worldDeltaX * 131072.0f);
        const int32_t deltaFPy = static_cast<int32_t>(worldDeltaY * 131072.0f);
        const int32_t deltaFPz = static_cast<int32_t>(worldDeltaZ * 131072.0f);
        // THE DELTA IS NO LONGER ADDED HERE (CyberpunkVR_HeadTranslationInPatch).
        //
        // Adding it to this buffer put MAIN's head translation into the blender's CameraSetup
        // ENTRY, where every field is multiplied by the camera's blend weight -- while MAIN's
        // orientation went through the component and was not. Both channels now travel the same
        // route, in PatchCamera, so the weighting applies to them equally. Setting the flag to 0
        // restores the old split without a rebuild.
        if (!CyberpunkVR_HeadTranslationInPatch) {
            posFP[0] += deltaFPx;
            posFP[1] += deltaFPy;
            posFP[2] += deltaFPz;
        }

        // Publish it for the write site: this is where the delta is COMPUTED (it needs the body
        // forward, the recenter base and the bakes), and PatchCamera is where it is applied to
        // both cameras. See g_headDeltaFP.
        g_headDeltaFP[0].store(deltaFPx, std::memory_order_relaxed);
        g_headDeltaFP[1].store(deltaFPy, std::memory_order_relaxed);
        g_headDeltaFP[2].store(deltaFPz, std::memory_order_relaxed);
        g_headDeltaValid.store(1, std::memory_order_release);
        // [104..111] RENDER-VIEW POSE v2 (game world axes) + [141..142] heading,
        // seqlocked by [143]. REVERTED to the render-stage writer (user order): the
        // boundary publisher experiment did not remove the trail and worsened snap.
        if (float* shView = GetShotShared()) {
            static uint32_t s_vpSeqCtr = 0;
            volatile uint32_t* vpSeq = reinterpret_cast<volatile uint32_t*>(&shView[143]);
            *vpSeq = ++s_vpSeqCtr;               // odd: write in progress
            shView[104] = camera_qx; shView[105] = camera_qy;
            shView[106] = camera_qz; shView[107] = camera_qw;
            // [108..110] = the head displacement the SOLVE still has to add on top of the camera
            // it is given, and that is not the same number in both arrangements.
            //
            // THIS IS THE DOUBLE COUNT THAT MADE THE BODY STICK TO THE HEAD. VRIK builds its
            // anchor as (camera it was handed) + (this delta). The camera it is handed is the
            // game's FPP camera -- and with the translation now applied in the COMPONENT, that
            // camera ALREADY contains the displacement. Publishing it again told the solve to move
            // the body by the head displacement twice, so the body tracked the head instead of
            // staying in the world: exactly the reported symptom, and it appeared the moment the
            // flag went to 1.
            //
            // So the delta published here is the part NOT yet in the camera: zero when PatchCamera
            // applies it, the full vector in the old split where the camera stayed clean.
            const bool deltaAlreadyInCamera = (CyberpunkVR_HeadTranslationInPatch != 0);
            shView[108] = deltaAlreadyInCamera ? 0.0f : worldDeltaX;
            shView[109] = deltaAlreadyInCamera ? 0.0f : worldDeltaY;
            shView[110] = deltaAlreadyInCamera ? 0.0f : worldDeltaZ;
            shView[111] = 2.0f;
            // ([112..115] retired: old stabilizer slots, no writers/readers left.)
            // [141] = RENDER-FRESH game heading (rad) + [142] validity. During the snap
            // one-tick view hold the RENDERED heading is (game - snapDelta); publish THAT,
            // so the hands mapping and the [148] pre-snap guard track what is on screen.
            shView[141] = atan2f(-bodyGameForwardX, bodyGameForwardY);
            shView[142] = 1.0f;
            // [227..230] the HEAD orientation this view was composed from, XR axes, same space
            // as the [16..19] the hand publish carries. The arms rotate their head-local
            // controller offset by the view quaternion [104..107], which is built here -- at a
            // different instant from the offsets. Publishing the head part lets that gap be
            // divided out exactly, without assuming anything about how the view is composed.
            shView[227] = xrPose.oriX; shView[228] = xrPose.oriY;
            shView[229] = xrPose.oriZ; shView[230] = xrPose.oriW;
            // [68] age stamp, inside this seqlock. The arms hang off THIS pose while the image
            // is rendered from the camera written later in the same call -- so how old this is
            // when the solve consumes it IS the distance the hands trail the view.
            {
                LARGE_INTEGER c{}, f{};
                QueryPerformanceCounter(&c);
                QueryPerformanceFrequency(&f);
                const double ms = (f.QuadPart > 0)
                    ? (double)c.QuadPart * 1000.0 / (double)f.QuadPart : 0.0;
                shView[68] = (float)fmod(ms, 100000.0);
            }
            *vpSeq = ++s_vpSeqCtr;               // even: packet complete

            // THE SAME FRAME, HANDED TO THE SOLVE WITHOUT LEAVING THE PROCESS.
            //
            // Identical values, taken from the identical locals, published in one struct under its
            // own seqlock -- so the solve gets one instant instead of re-reading a shared block that
            // it is on the wrong side of. The shared slots above stay for CET/redscript, which are a
            // real boundary; this is not.
            {
                cvr::camera::ViewFrame vf{};
                vf.viewQuat[0] = camera_qx; vf.viewQuat[1] = camera_qy;
                vf.viewQuat[2] = camera_qz; vf.viewQuat[3] = camera_qw;
                vf.worldDelta[0] = shView[108];
                vf.worldDelta[1] = shView[109];
                vf.worldDelta[2] = shView[110];
                vf.deltaSemantics = shView[111];
                vf.headingRad = shView[141];
                vf.headOri[0] = xrPose.oriX; vf.headOri[1] = xrPose.oriY;
                vf.headOri[2] = xrPose.oriZ; vf.headOri[3] = xrPose.oriW;
                vf.stampMs = shView[68];
                cvr::camera::ViewFramePublish(vf);
            }
        }

        if (g_verboseLog && (g_locateCameraHits % 600) == 1) {
            Log("LocateCamera translation: allow=%d posScale=%.4f local=(%.4f, %.4f, %.4f)\n",
                allowGameCameraTranslation ? 1 : 0,
                posScale,
                localRight,
                localForward,
                localUp);
        }
    }

    // Per-eye stereo separation for AER uses the runtime's
    // ACTUAL per-eye eye-pose translations, not a synthetic +/-halfIpd scalar.
    // We therefore prefer the current runtime eye-center offset from
    // OpenXRManager (eye pose minus center-eye), scaled by WorldScale/IPDScale/
    // StereoScale, then rotate that full local offset into world using the
    // located camera basis. This preserves asymmetric runtime frusta / tiny
    // non-X offsets with the runtime's own IPD. Fallback to the
    // old right*halfIpd path only if the runtime eye offsets are unavailable.
    // NOTE: IPD shift is applied REGARDLESS of menuOpen. HISTORY: menuOpen once read
    // shared[63], which collided with the weapon-aim delta-quaternion float bits
    // ([63..66]) and came out "true" on most frames -- that prevented eye alternation
    // entirely (game rendered only one eye). The menu/map flag has since moved to the
    // DEDICATED uint32 slot [81] (SetVRMenuOpen bridge; [70..76] are the anatomical
    // shoulder offsets, NOT a menu path). Applying IPD unconditionally is kept anyway:
    // it is correct in menus too (static 2D panel + stereo eyes) and avoids re-linking
    // eye alternation to any flag.
    // LATE IPD SHIFT: compute the per-eye stereo offset here but DO NOT move the
    // located camera. posFP feeds the engine's IK/physics/gameplay head; shifting
    // it ±halfIPD every frame is what makes VRIK thrash. We store the (eye-signed)
    // shift and let OnFinalCameraCallback add it to the FINAL render camera only,
    // post-IK, just before projection. Render output is
    // unchanged (the final camera ends up at the same place); only the IK/physics
    // head now stays at the stable center.
    int32_t ipdShiftFP[3] = {0, 0, 0};
    int32_t bdIpdShiftFP[3] = {0, 0, 0};   // MAIN's half, braindance only; see below
    if (hasXR) {
        const int renderEye = (g_locateCameraHits % 2);

        float right[3] = {};
        //float hmdQuat[4] = { xrPose.oriX, -xrPose.oriZ, xrPose.oriY, xrPose.oriW };
        //ComputeRightVectorFromQuaternion(hmdQuat, right);

        float cameraQuat[4] = { camera_qx, camera_qy, camera_qz, camera_qw };
        ComputeRightVectorFromQuaternion(cameraQuat, right);

        if (IsPlausibleUnitVector3(right)) {
            const float halfIpd = GetDesiredHalfIpd();
            const float eyeSign = (renderEye == 0) ? -1.0f : 1.0f;
            //const float eyeSign = (renderEye == 0) ? 1.0f : -1.0f;

            const float ipdShift = halfIpd * eyeSign;
            // 131072 = WorldPosition fixed-point scale (17 fractional bits, see the
            // worldDelta injection above). The old 65536 halved the stereo eye
            // separation -- the rendered IPD was HALF the configured one.
            ipdShiftFP[0] = static_cast<int32_t>(right[0] * ipdShift * 131072.0f);
            ipdShiftFP[1] = static_cast<int32_t>(right[1] * ipdShift * 131072.0f);
            ipdShiftFP[2] = static_cast<int32_t>(right[2] * ipdShift * 131072.0f);

            // BRAINDANCE ONLY, AND ONLY WHILE THE SCENE OWNS THE CAMERA. This buffer is filled from
            // the camera component in ordinary play, so MAIN's half is already in it by the time we
            // get here and adding it again would double the separation. In a braindance there is no
            // component to fill it from -- the replay drives a camera the port cannot reach -- and
            // this buffer IS what the engine renders through, which is also why MAIN's orientation
            // is composed from here in that mode. See CyberpunkVR_BdIpdInLocate.
            //
            // MAIN is the left eye by default, so the sign matches PatchCamera's `sgn0` for kind 1,
            // and it changes hands with the swap for the same reason it does there: each eye would
            // otherwise get the other's viewpoint.
            //
            // COMPUTED HERE, APPLIED BELOW THE HEAD-CENTRE SNAPSHOT. The first cut added it to posFP
            // right here, which is above the point where `renderedPosFP` is taken -- so the value
            // published as the HEAD CENTRE became the left eye instead, and the overlay that draws a
            // world point from it would have been half an IPD out.
            if (CyberpunkVR_BdIpdInLocate && CyberpunkVR_IpdInWorldPos &&
                g_bdActive.load(std::memory_order_relaxed) &&
                g_bdScenePoseValid.load(std::memory_order_acquire)) {
                const float sign = CyberpunkVR_MainIsRightEye ? +1.0f : -1.0f;
                for (int i = 0; i < 3; ++i)
                    bdIpdShiftFP[i] = static_cast<int32_t>(right[i] * halfIpd * sign * 131072.0f);
            }
            if (g_verboseLog && (g_locateCameraHits % 600) == 1) {
                Log("LocateCamera IPD: eye=%d halfIpd=%.4f right=(%.3f, %.3f, %.3f) shift=%.4f\n",
                    renderEye,
                    halfIpd, right[0], right[1], right[2], ipdShift);
            }
        }
    }

    // Located camera = head CENTER (no IPD). IK/physics/VRIK read this.
    // The head centre in world metres, for the overlay. The barrel dot used to be drawn from a
    // DIRECTION alone, which can only be right for an eye that lies on the bullet's line -- the
    // left one, because the weapon is held in front of it. To put a real world point on screen
    // the overlay needs the eye's world position, and this is the only place that has it.
    // THE HEAD CENTRE AS RENDERED -- and with the translation in the component it is `posFP`
    // ITSELF, with nothing added.
    //
    // MEASURED, because I got this wrong first: this buffer is filled by SerializeSetup from
    // component+0xE0, so once PatchCamera adds the delta there, the value arrives here ALREADY
    // containing it. Live check at the serialise breakpoint, same stationary scene:
    // flag=1 -> pos 44468875, flag=0 -> pos 44528526, difference -59651 against a published delta
    // of -64178. The add is in. Adding it again here counted the head displacement twice.
    // MAIN'S VIEWPOINT, FROM THE SAME PER-FRAME SCENE POSE THE SECOND EYE IS PLACED WITH.
    // Before the head centre is taken and before the half-IPD goes in, so both build on it. See
    // CyberpunkVR_BdMainPosFromScene for why one of four quantities at a different cadence is the
    // pair coming apart rather than one eye lagging.
    if (CyberpunkVR_BdMainPosFromScene &&
        g_bdActive.load(std::memory_order_relaxed) &&
        g_bdScenePoseValid.load(std::memory_order_acquire)) {
        for (int i = 0; i < 3; ++i)
            posFP[i] = g_bdScenePosFP[i].load(std::memory_order_relaxed);
        ++CyberpunkVR_DebugBdMainPos;
    }

    // THE VIEWPOINT SLIDER FOR A TAKEN-OVER CAMERA, applied to the buffer itself.
    //
    // "нужно добавлять слайдер специально для этой миссии... для камеры": the turret puts the eye
    // somewhere the mission's own framing chose, and it is not where a seated player's head is. This is
    // the only place the offset can go once the takeover is driven from here -- the buffer is what MAIN
    // renders from AND the base the second eye's push takes two blocks below, so adding it here moves the
    // PAIR. Adding it at the component instead moved one eye and fought the push.
    //
    // Yaw only, about the lens's own aim: forward means where the turret looks, and pitching the offset
    // with a nose-down mount would drop the viewpoint through the floor. Same expression PatchCamera uses
    // for the surveillance-camera case, sign for sign.
    if (LocateOwnsTakeover()) {
        const float ox = CyberpunkVR_DevCamOffsetX;
        const float oy = CyberpunkVR_DevCamOffsetY;
        const float oz = CyberpunkVR_DevCamOffsetZ;
        if (ox != 0.0f || oy != 0.0f || oz != 0.0f) {
            const float yaw = g_devCamAimValid.load(std::memory_order_acquire) ? g_devCamAimYaw : 0.0f;
            const float sy = sinf(yaw), cy = cosf(yaw);
            posFP[0] += static_cast<int32_t>((ox * cy - oy * sy) * 131072.0f);
            posFP[1] += static_cast<int32_t>((ox * sy + oy * cy) * 131072.0f);
            posFP[2] += static_cast<int32_t>(oz * 131072.0f);
        }
    }

    int32_t renderedPosFP[3] = { posFP[0], posFP[1], posFP[2] };

    // NOW the braindance half goes in -- after the head centre has been taken, so that stays the
    // centre. This buffer is what the engine renders the braindance through; MAIN has no camera
    // component there, which is why its half cannot go where the second eye's does.
    if (bdIpdShiftFP[0] || bdIpdShiftFP[1] || bdIpdShiftFP[2]) {
        for (int i = 0; i < 3; ++i) posFP[i] += bdIpdShiftFP[i];
        ++CyberpunkVR_DebugBdIpdLocate;
    }
    // NOW the push, with MAIN's own head centre for this frame as the base. Not gated on the scene
    // pose any more: the editor has no scene pose, and that is exactly the case where the two eyes
    // were falling back to different bases and coming apart vertically.
    // Playback only, for the reason spelled out at BdPushOwnsComposition: in the editor the setter
    // runs normally and this would be a second writer.
    // A DEVICE TAKEOVER, ON ITS OWN GATE AND WITH ITS OWN STATE.
    //
    // The first attempt at this OR-ed LocateOwnsTakeover() into the braindance conditions above, and that
    // was wrong for a reason worth keeping: those branches consume braindance state -- the scene pose, the
    // scene quaternion -- which nothing publishes during a takeover, so MAIN's orientation froze and the
    // fov stopped being forced. "надо писать под takeover гейтом все". So this block shares only the
    // composition every located camera already gets (camera_qx..w = base * head) and the buffer's own
    // position; everything else is its own.
    //
    // WHAT IT WRITES: the second eye's transform, from here, in the same call that composed it -- one
    // writer, no stamp to go stale, which is the difference between this and mode 3 of
    // CyberpunkVR_VrcamPosFromMain (that one reads the centre a frame later and trails MAIN). MAIN itself
    // needs nothing written here: the engine renders it through the camera being located, and this buffer
    // IS that camera.
    // ...AND ONLY FOR THE CAMERA THE TAKEOVER RENDERS THROUGH. This site runs for EVERY located camera,
    // the player's own included, and the first cut pushed on all of them -- so the second eye took
    // whichever was located last, which is the player's head rather than the turret. That is the
    // "vrcam != main по позиции" that survived the previous build even though the push itself was
    // running (measured: CyberpunkVR_DebugTakeoverPush = 2375).
    //
    // The lens position identifies it, the same reference the fov write now uses. Within a metre: the
    // stamp is a frame old on a moving mount, and this buffer already carries the eye separation.
    bool locatedIsLens = false;
    if (g_devCamPosValid.load(std::memory_order_acquire)) {
        const float k = 1.0f / 131072.0f;
        float d2 = 0.0f;
        for (int i = 0; i < 3; ++i) {
            const float d = (renderedPosFP[i] - g_devCamPosFP[i].load(std::memory_order_relaxed)) * k;
            d2 += d * d;
        }
        locatedIsLens = d2 <= 1.0f;
    }
    if (LocateOwnsTakeover() && g_headQuatValid && locatedIsLens) {
        const float tq[4] = { g_headQuatComposed[0], g_headQuatComposed[1],
                              g_headQuatComposed[2], g_headQuatComposed[3] };
        if (IsPlausibleUnitQuaternion(tq)) {
            BdPushTransformOnce(g_camObjVrcam.load(std::memory_order_relaxed),
                                tq[0], tq[1], tq[2], tq[3], true, renderedPosFP);
            ++CyberpunkVR_DebugTakeoverPush;
        }
    }

    if (CyberpunkVR_BdPushTransform != 0 && g_headQuatValid &&
        g_bdActive.load(std::memory_order_relaxed) &&
        g_bdScenePoseValid.load(std::memory_order_acquire)) {
        if (CyberpunkVR_BdPushTransform & 1)
            BdPushTransformOnce(g_camObjVrcam.load(std::memory_order_relaxed),
                                g_bdPushQuat[0], g_bdPushQuat[1], g_bdPushQuat[2], g_bdPushQuat[3],
                                true, renderedPosFP);
        if (CyberpunkVR_BdPushTransform & 2)
            BdPushTransformOnce(g_camObjMain.load(std::memory_order_relaxed),
                                g_bdPushQuat[0], g_bdPushQuat[1], g_bdPushQuat[2], g_bdPushQuat[3],
                                false, renderedPosFP);
    }

    // The head CENTRE, before the separation -- this is what the second view sits on in mode 3.
    for (int i = 0; i < 3; ++i)
        g_locateCenterFP[i].store(renderedPosFP[i], std::memory_order_relaxed);
    // Published for the camera census, which otherwise has no way to see where MAIN actually renders
    // from in a braindance.
    for (int i = 0; i < 3; ++i) g_locatePosFP[i].store(posFP[i], std::memory_order_relaxed);
    g_locatePosValid.store(1, std::memory_order_release);
    if (!CyberpunkVR_HeadTranslationInPatch && g_headDeltaValid.load(std::memory_order_acquire)) {
        // Old split only: there the delta went into this buffer above, so posFP already has it and
        // this branch is a no-op -- kept as the single place that states the invariant.
    }
    if (float* shp = GetShotShared()) {
        shp[204] = static_cast<float>(renderedPosFP[0]) / 131072.0f;
        shp[205] = static_cast<float>(renderedPosFP[1]) / 131072.0f;
        shp[206] = static_cast<float>(renderedPosFP[2]) / 131072.0f;
        shp[207] = 1.0f;
    }
    g_lastLocatePosFP[0] = renderedPosFP[0];
    g_lastLocatePosFP[1] = renderedPosFP[1];
    g_lastLocatePosFP[2] = renderedPosFP[2];
    // Per-eye shift carried to OnFinalCameraCallback for late application.
    g_lastIpdShiftFP[0] = ipdShiftFP[0];
    g_lastIpdShiftFP[1] = ipdShiftFP[1];
    g_lastIpdShiftFP[2] = ipdShiftFP[2];
    g_lastLocateQuat[0] = quat[0];
    g_lastLocateQuat[1] = quat[1];
    g_lastLocateQuat[2] = quat[2];
    g_lastLocateQuat[3] = quat[3];
    ++g_lastLocateSeq;
    {
        cvr::camera::LocatedCameraFrame frame{};
        frame.worldPos[0] = static_cast<float>(renderedPosFP[0]) / 131072.0f;
        frame.worldPos[1] = static_cast<float>(renderedPosFP[1]) / 131072.0f;
        frame.worldPos[2] = static_cast<float>(renderedPosFP[2]) / 131072.0f;
        // Publish the quaternion this locate COMPOSED, not `quat`. In the active
        // CamWriteInPatch path this serialized buffer is intentionally left untouched and PatchCamera
        // writes the component later; `quat` is therefore only the pre-HMD engine base.
        frame.worldQuat[0] = camera_qx; frame.worldQuat[1] = camera_qy;
        frame.worldQuat[2] = camera_qz; frame.worldQuat[3] = camera_qw;
        frame.sequence = g_lastLocateSeq;
        frame.frameEpoch = g_VrikFrameEpoch.load(std::memory_order_relaxed);
        cvr::camera::LocatedCameraFramePublish(frame);
    }

    // Publish the located camera + a controller-aim quaternion for the plugin's ShotSnap.
    // controllerAim = bodyYaw (X) controllerGame, built EXACTLY like the camera quat above
    // (same x,-z,y axis map + the same gameYaw), so the bullet, when this quat is bracketed
    // into the located camera during a shot, flies down the controller while the view (which
    // reads the HMD quat we just wrote) stays on the head.
    if (hasXR) {
        if (float* sh = GetShotShared()) {
            OpenXRHeadPose handPose{};
            const bool hasHand = OpenXRManager::Get().GetHandPose(1, &handPose) && handPose.valid;
            if (hasHand) {
                const float gameYaw2 = atan2f(-bodyGameForwardX, bodyGameForwardY);
                const float cy2 = cosf(gameYaw2 * 0.5f);
                const float sy2 = sinf(gameYaw2 * 0.5f);
                const float cgX = handPose.oriX;
                const float cgY = -handPose.oriZ;
                const float cgZ = handPose.oriY;
                const float cgW = handPose.oriW;
                float aX, aY, aZ, aW;
                MulQuat(0.0f, 0.0f, sy2, cy2, cgX, cgY, cgZ, cgW, aX, aY, aZ, aW);
                NormalizeQuat(aX, aY, aZ, aW);
                const uintptr_t camAddr = reinterpret_cast<uintptr_t>(rbxPtr);
                uint32_t lo = static_cast<uint32_t>(camAddr & 0xFFFFFFFFu);
                uint32_t hi = static_cast<uint32_t>(camAddr >> 32);
                memcpy(&sh[51], &lo, 4);
                memcpy(&sh[52], &hi, 4);
                sh[53] = aX; sh[54] = aY; sh[55] = aZ; sh[56] = aW;
                // Controller FORWARD as a WORLD direction vector for the fire-shot hook:
                // rotate game-forward (0,1,0) by the aim quat.
                // v = q * (0,1,0) * q^-1, expanded:
                const float fwX = 2.0f * (aX * aY - aZ * aW);
                const float fwY = 1.0f - 2.0f * (aX * aX + aZ * aZ);
                const float fwZ = 2.0f * (aY * aZ + aX * aW);
                sh[60] = fwX; sh[61] = fwY; sh[62] = fwZ;
                // DELTA quat = inv(hmd_game) * controller_game  (both remapped x,-z,y,w to game axes).
                // The plugin multiplies the provider's ORIGINAL camera quat by this:
                //   qNew = camera * delta = (bodyYaw*hmd) * (inv(hmd)*controller) = bodyYaw*controller
                // -> bullet flies down the controller, in correct game-world space (pivots off the
                //    known-correct camera orientation instead of rebuilding world from scratch).
                {
                    // PROPER OpenXR->game for a RELATIVE rotation. The component swap (x,-z,y,w) used
                    // elsewhere is only valid for absolute look quats, NOT for a rotation delta (that
                    // needs a similarity transform P*q*P^-1). So: compute the delta in RAW XR space,
                    // then conjugate it into game space by P = rotX(+90deg) (xr->game axis map).
                    // delta_xr = inv(head_xr) * hand_xr   (controller relative to head, headset space)
                    float dxrX, dxrY, dxrZ, dxrW;
                    MulQuat(-xrPose.oriX, -xrPose.oriY, -xrPose.oriZ, xrPose.oriW,   // inv(head_xr)
                            handPose.oriX, handPose.oriY, handPose.oriZ, handPose.oriW,
                            dxrX, dxrY, dxrZ, dxrW);
                    // delta_game = P * delta_xr * P^-1 ; P=(0.70710678,0,0,0.70710678)
                    const float pX = 0.70710678f, pW = 0.70710678f;
                    float t1X, t1Y, t1Z, t1W;
                    MulQuat(pX, 0.0f, 0.0f, pW, dxrX, dxrY, dxrZ, dxrW, t1X, t1Y, t1Z, t1W);   // P * delta
                    float dX, dY, dZ, dW;
                    MulQuat(t1X, t1Y, t1Z, t1W, -pX, 0.0f, 0.0f, pW, dX, dY, dZ, dW);            // * P^-1
                    NormalizeQuat(dX, dY, dZ, dW);
                    sh[63] = dX; sh[64] = dY; sh[65] = dZ; sh[66] = dW;
                }
                sh[50] = static_cast<float>(g_lastLocateSeq & 0xFFFFFF); // valid/heartbeat
            }
        }
    }
}

// ---- WHICH VIEW IS RECORDING RIGHT NOW ---------------------------------------------------
// The exact answer, and the only one that survives MAIN and VRCAM being the same size.
//
// The render graph's node dispatcher carries the view context in work_context+0x18, and the
// view's identity is the CName hash at ctx+0x28: MAIN is 0, VRCAM is the hash of its feed
// name, and the engine's own helper views (distant geometry, shadows, reflections) each have
// their own. Nodes record their command lists on the dispatching thread, so a thread-local
// set here is readable from the D3D12 hooks that run inside the node -- which is exactly how
// the depth pick below can know whose depth-stencil it is looking at, instead of guessing
// from resolution.
//
// One hook, not the whole stereo module: this is the single fact needed.

bool InstallLocateCameraHook() {
    const char* pattern = "\xF3\x0F\x11\x43\x20\x48\x8D\x54\x24\x20\x48\x8B\x06";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 10; 
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rbx
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9; // mov rcx, rbx
    // Set arg2 (xmm1) = xmm0 (since float args go in xmm registers, xmm1 is 2nd arg)
    code[pos++] = 0x0F; code[pos++] = 0x28; code[pos++] = 0xC8; // movaps xmm1, xmm0

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnLocateCameraCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instructions:
    // movss [rbx+20h], xmm0
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x43; code[pos++] = 0x20;
    // lea rdx, [rsp+20h]
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;

    // jmp back
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

CVR_HOOK("LocateCamera", ::cvr::hooks::Stage::Boot, 10, InstallLocateCameraHook);
