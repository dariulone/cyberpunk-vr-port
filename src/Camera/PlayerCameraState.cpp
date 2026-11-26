// RefreshPlayerCameraState: the player's flags from RTTI (mounted vehicle, driving, aiming, weapons, scene tier),
// refreshed once per entity tick from the camera locate. Formerly in src/Hooks/LocateCamera.cpp; it stays C++ in both
// builds until the engine's scripting is reached from Rust (rust/crates/cvr-rig/src/vrik/locate_camera.rs calls it
// through cvr_cpp_refresh_player_camera_state).
#include "Utils/DebugGate.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Camera/PlayerCameraState.hpp"
#include "Camera/SceneCameraHeading.hpp"
#include "Camera/SceneCameraComfort.hpp"
#include "Camera/CameraState.hpp"
#include "Anim/CharacterRig.hpp"  // g_VrikFrameEpoch: exact camera/entity frame pairing
#include "Anim/WheelGrab.hpp"
#include "Anim/LeftHanded.hpp"
#include "Utils/LogThrottle.hpp"
#include "Core/LiveControls.hpp"
#include "Anim/VehiclePosePolicy.hpp"
#include "Core/Telemetry.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/BodyHeading.hpp"
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
#include "Lobby/MenuLobby.hpp"

#if CVR_RUST_VRIK
// THE REFRESH IS RUST'S (rust/crates/cvr-ffi/src/player_camera_state.rs, under the C name cvr-rig's locate hook calls),
// with the RTTI cache VrCore.cpp made for it. What it asks of the C++ half is here.
// GetPlayer, the vehicle state and IsDriver: rust/crates/cvr-ffi/src/player_camera_state.rs.
#else

namespace {std::atomic<uintptr_t> s_mountedVehicleIdentity{};}
uintptr_t CurrentMountedVehicleIdentity() {return s_mountedVehicleIdentity.load(std::memory_order_acquire);}

void RefreshPlayerCameraState() {
    static std::mutex refreshMutex;
    std::unique_lock guard(refreshMutex,std::try_to_lock);
    if(!guard.owns_lock())return;
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
        static uint32_t refreshCalls=0;
        bool refreshPlayer = ((refreshCalls++ & 31) == 0);
        if (float* shSeq = GetShotShared()) {
            const float seq = shSeq[99];
            if (seq != s_lastEntSeqForPlayer) { s_lastEntSeqForPlayer = seq; refreshPlayer = true; }
        }
        if (refreshPlayer) {
            RED4ext::ScriptGameInstance gameInstance;
            RED4ext::Handle<RED4ext::IScriptable> playerHandle;
            const bool playerQueried = RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
            if (playerQueried) cvr::roomscale::SetPlayer(reinterpret_cast<uintptr_t>(playerHandle.instance));

            g_isInVehicle=false;
            bool mountedCar=false;
            uintptr_t mountedIdentity=0;
            if (playerHandle && g_mountedVehicleProp) {
                auto mountedVehicle = g_mountedVehicleProp->GetValue<RED4ext::WeakHandle<RED4ext::IScriptable>>(playerHandle.instance);
                const auto mounted=mountedVehicle.Lock();
                mountedIdentity=reinterpret_cast<uintptr_t>(mounted.GetPtr());
                g_isInVehicle = mountedIdentity!=0;
                if(mounted) {
                    static const auto* carClass=RED4ext::CRTTISystem::Get()->GetClass("vehicleCarBaseObject");
                    mountedCar=carClass && mounted->GetType()->IsA(carClass);
                }
            }
            cvr::anim::WheelSetMountedCar(mountedCar);
            s_mountedVehicleIdentity.store(mountedIdentity,std::memory_order_release);

            // PlayerPuppet::vehicleState mirrors PSM.Vehicle. Combat is passenger
            // window combat; DriverCombat is a separate state. A missing IsDriver
            // function must never classify every mounted passenger as the driver.
            int vehicleState=-1;
            if(playerHandle && g_vehicleStateProp)
                vehicleState=static_cast<int>(g_vehicleStateProp->GetValue<RED4ext::game::PSMVehicle>(playerHandle.instance));
            g_vehicleState.store(vehicleState,std::memory_order_relaxed);
            bool driving = false;
            if (g_isInVehicle) {
                if(vehicleState>=0) {
                    driving=cvr::anim::IsDrivingVehicleState(vehicleState);
                } else if (g_isDriverFunc && playerHandle) {
                    RED4ext::ScriptGameInstance gi;
                    bool isDriver = false;
                    RED4ext::StackArgs_t args;
                    args.emplace_back(nullptr, &gi);
                    args.emplace_back(nullptr, &playerHandle);
                    // The cast picks the (void* instance) overload: a bare nullptr is ambiguous
                    // against the (CClass* context) one, and a static function wants no instance.
                    if (RED4ext::ExecuteFunction(static_cast<void*>(nullptr), g_isDriverFunc, &isDriver, args))
                        driving = isDriver;
                }
            }
            g_isDriving.store(driving, std::memory_order_relaxed);

            if (playerHandle && g_isAimingProp) {
                g_isAiming = g_isAimingProp->GetValue<bool>(playerHandle.instance);
            }

            if (playerHandle && g_equippedWeaponProp) {
                auto equippedWeapon = g_equippedWeaponProp->GetValue<RED4ext::WeakHandle<RED4ext::IScriptable>>(playerHandle.instance);
                g_hasRightWeaponEquipped = (equippedWeapon.instance != nullptr);
            }
            if (playerHandle && g_equippedLeftWeaponProp) {
                auto equippedWeapon = g_equippedLeftWeaponProp->GetValue<RED4ext::WeakHandle<RED4ext::IScriptable>>(playerHandle.instance);
                g_hasLeftWeaponEquipped = (equippedWeapon.instance != nullptr);
            }
            // A WEAPON IN HAND is the right hand's -- or, left-handed, the left hand's alone: everything that asks
            // "is a weapon out" (the trigger, the buttons it owns, the hands' poses and filters) asks it of both.
            g_hasWeaponEquipped = g_hasRightWeaponEquipped || cvr::anim::WeaponInLeftHand();

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
            // `sceneTier : GameplayTier` (0 = Undefined, 1 = Tier1_FullGameplay,
            // 4 = Tier4_FPPCinematic, 5 = Tier5_Cinematic). The upstream PR had
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
}
#endif
