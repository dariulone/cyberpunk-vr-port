// AnimPose -- THE HOOK. The engine's animation pose-apply, detoured.
//
// This is the file the pose path exists for, and it belongs in Hooks/ with the other twenty-three:
// it was in Anim/ only because it started life as a header next to the maths it calls.
//
// IT IS NOT REGISTERED WITH THE HOOK REGISTRY, AND THAT IS DELIBERATE. Every hook in Hooks/ is
// installed by a stage at boot; this one is installed when SCRIPT asks, through the
// InstallVRAnimPoseHook native. Registering it with CVR_HOOK would install it at boot instead --
// a behaviour change wearing the clothes of a file move.
//
// THE WORK IS ELSEWHERE: cvr_anim_pose_apply, in src/Hooks/AnimPoseBody.cpp or, with CVR_RUST_VRIK, in its Rust port
// (rust/crates/cvr-rig/src/vrik/pose_hook.rs). This file is the hook itself -- the trampoline, the SEH frame the
// work runs inside, the lock it may leave held, the mirror double after it -- and is the same in both builds.

#include "Anim/VrikHook.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include <windows.h>

#if !CVR_RUST_VRIK   // the Rust build's hook: rust/crates/cvr-rig/src/vrik/anim_pose_hook.rs
// The player's own work: identifies the pass, writes the weapon rig, the finger layers and the camera bones, and
// solves or replays VRIK. Reports through playerStateLocked whether it took g_PlayerPoseStateLock, so a fault inside
// it still releases the lock below.
extern "C" void cvr_anim_pose_apply(void* a1, void* a2, unsigned int a4, bool* playerStateLocked);

// The trampoline this detour calls through. The hook's own state.
typedef void* (*AnimPoseFunc_t)(void* a1, void* a2, void* a3, unsigned int a4);
namespace { AnimPoseFunc_t OriginalAnimPose = nullptr; }

// The engine's own apply first; then ours, guarded. SEH is dynamic: a fault anywhere in the work -- C++ or Rust
// frames alike -- lands in this __except, and the __finally releases the player-state lock it held.
static void* Hooked_AnimPoseApply_Inner(void* a1, void* a2, void* a3, unsigned int a4) {
    void* result = OriginalAnimPose(a1, a2, a3, a4);
    bool playerStateLocked=false;
    __try {
    __try {
        cvr_anim_pose_apply(a1, a2, a4, &playerStateLocked);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    } __finally {
        if(playerStateLocked)ReleaseSRWLockExclusive(&g_PlayerPoseStateLock);
    }
    return result;
}

// THE DETOUR. The player's own work above, then the mirror double: a pass of the scene's NPC copy of V takes the
// pose the player's last pass left (src/Natives/VrikArming.cpp). Kept outside the body above, which returns from
// a dozen places; with no double bound the cost is one relaxed load.
extern "C" void* Hooked_AnimPoseApply(void* a1, void* a2, void* a3, unsigned int a4) {
    void* result = Hooked_AnimPoseApply_Inner(a1, a2, a3, a4);
    if (!VRIK_MirrorDoubleActive()) return result;
    __try {
        void* poseDesc = reinterpret_cast<void**>(a2)[7];
        if (poseDesc) {
            uint8_t*  boneBuf  = reinterpret_cast<uint8_t**>(poseDesc)[0];
            uintptr_t trackBuf = reinterpret_cast<uintptr_t*>(poseDesc)[3];
            const uint32_t slots = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(poseDesc) + 8);
            if (boneBuf && trackBuf) VRIK_MirrorDoublePass(boneBuf, trackBuf, slots);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return result;
}

bool InstallAnimPoseHook() {
    HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
    if (!hMod) return false;
    void* target = reinterpret_cast<void*>(reinterpret_cast<uintptr_t>(hMod) + 0x1B3C08);
    MH_Initialize(); // no-op if already initialized
    if (MH_CreateHook(target, &Hooked_AnimPoseApply, reinterpret_cast<void**>(&OriginalAnimPose)) != MH_OK)
        return false;
    if (MH_EnableHook(target) != MH_OK)
        return false;
    return true;
}
#endif
