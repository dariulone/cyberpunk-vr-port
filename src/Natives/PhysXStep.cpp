// PhysXStep -- see include/Natives/PhysXStep.hpp.
#include "Natives/PhysXStep.hpp"
#include "Natives/FrameOrderProbe.hpp"
#include "Natives/HeldBody.hpp"
#include "Natives/PhysHands.hpp"
#include "Hooks/Hook.hpp"
#include "MinHook.h"
#include <windows.h>
#include <atomic>
#include <cstdint>

extern void Log(const char* fmt, ...);

namespace cvr::physxstep {
namespace {
// PxPhysics::getNbScenes / getScenes, PxScene::simulate / fetchResults (PhysX 3.4, counted from its headers and
// checked against the slots Natives/DebugView.cpp already calls).
constexpr int kPhysicsGetNbScenes = 19, kPhysicsGetScenes = 20, kSceneSimulate = 56, kSceneFetchResults = 61;

using SimulateFn = void (*)(void*, float, void*, void*, uint32_t, bool);
using FetchFn = bool (*)(void*, bool, uint32_t*);
SimulateFn originalSimulate = nullptr;
FetchFn originalFetch = nullptr;
std::atomic<bool> hooked{false};
SRWLOCK hookLock = SRWLOCK_INIT;

void Simulate(void* scene, float dt, void* completion, void* scratch, uint32_t scratchSize, bool control) {
    cvr::frameorder::Note('S');
    cvr::heldbody::BeforeSimulate(scene, dt);
    cvr::physhands::BeforeSimulate(scene, dt);
    originalSimulate(scene, dt, completion, scratch, scratchSize, control);
    cvr::frameorder::Note('s');
}
bool Fetch(void* scene, bool block, uint32_t* error) {
    cvr::frameorder::Note('F');
    const bool done = originalFetch(scene, block, error);
    if (done) cvr::physhands::AfterFetch(scene);
    cvr::frameorder::Note('f');
    return done;
}

template <class T> T Slot(void* object, int slot) { return reinterpret_cast<T>((*reinterpret_cast<void***>(object))[slot]); }

// Both functions hooked, enabled in one pass. MH_EnableHook stops every thread of the process while it patches, and
// the game has a few hundred: done for one hook and then the other, on the frame a thing was first taken, that was the
// 56 to 92 ms frame every session showed ("[cost] a long frame ... carrying"). So it is done once, at boot.
bool HookAt(void* simulate, void* fetch) {
    if (MH_CreateHook(simulate, reinterpret_cast<void*>(&Simulate), reinterpret_cast<void**>(&originalSimulate)) != MH_OK) return false;
    if (MH_CreateHook(fetch, reinterpret_cast<void*>(&Fetch), reinterpret_cast<void**>(&originalFetch)) != MH_OK) {
        MH_RemoveHook(simulate);
        return false;
    }
    if (MH_QueueEnableHook(simulate) != MH_OK || MH_QueueEnableHook(fetch) != MH_OK || MH_ApplyQueued() != MH_OK) {
        MH_RemoveHook(simulate);
        MH_RemoveHook(fetch);
        return false;
    }
    return true;
}

// AT BOOT, FROM THE DLL ITSELF. No scene exists yet, but every scene's table is NpScene's, at RVA 0x20CAF0 of the
// PhysX3_x64.dll the game ships (simulate 56 = RVA 0x53760, fetchResults 61 = RVA 0x53980, the same two a live scene's
// table gave). Another DLL fails the checks below and is left to the lazy path, which reads a live scene's table.
constexpr uintptr_t kSceneTableRva = 0x20CAF0, kSimulateRva = 0x53760, kFetchResultsRva = 0x53980;
bool InstallAtBoot() {
    HMODULE dll = GetModuleHandleA("PhysX3_x64.dll");
    if (!dll) return false;
    const uintptr_t base = reinterpret_cast<uintptr_t>(dll);
    void** table = reinterpret_cast<void**>(base + kSceneTableRva);
    __try {
        if (reinterpret_cast<uintptr_t>(table[kSceneSimulate]) != base + kSimulateRva ||
            reinterpret_cast<uintptr_t>(table[kSceneFetchResults]) != base + kFetchResultsRva) {
            Log("[physxstep] PhysX3_x64.dll is not the one read: the step is hooked from the first scene instead\n");
            return false;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
    AcquireSRWLockExclusive(&hookLock);
    const bool ok = !hooked.load() && HookAt(table[kSceneSimulate], table[kSceneFetchResults]);
    if (ok) hooked.store(true, std::memory_order_release);
    ReleaseSRWLockExclusive(&hookLock);
    return ok || hooked.load();
}
CVR_HOOK("PhysXStep", ::cvr::hooks::Stage::Boot, 85, InstallAtBoot);

bool Hook() {
    HMODULE dll = GetModuleHandleA("PhysX3_x64.dll");
    auto getPhysics = dll ? reinterpret_cast<void* (*)()>(GetProcAddress(dll, "PxGetPhysics")) : nullptr;
    void* physics = getPhysics ? getPhysics() : nullptr;
    if (!physics) return false;
    const uint32_t count = Slot<uint32_t (*)(void*)>(physics, kPhysicsGetNbScenes)(physics);
    void* scene = nullptr;
    if (count == 0 || Slot<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(physics, kPhysicsGetScenes)(physics, &scene, 1, 0) != 1 || !scene)
        return false;
    void* simulate = Slot<void*>(scene, kSceneSimulate);
    void* fetch = Slot<void*>(scene, kSceneFetchResults);
    const auto inDll = [&](void* p) {
        MEMORY_BASIC_INFORMATION m{};
        return VirtualQuery(p, &m, sizeof(m)) && m.AllocationBase == dll;
    };
    if (!inDll(simulate) || !inDll(fetch)) {
        Log("[physxstep] the scene's table is not PhysX's own: not hooked\n");
        return false;
    }
    if (!HookAt(simulate, fetch)) return false;
    Log("[physxstep] PxScene::simulate %p and fetchResults %p hooked (%u scenes)\n", simulate, fetch, count);
    return true;
}
}  // namespace

bool Ensure() {
    if (hooked.load(std::memory_order_acquire)) return true;
    AcquireSRWLockExclusive(&hookLock);
    bool ok = hooked.load();
    if (!ok) {
        __try {
            ok = Hook();
        } __except (EXCEPTION_EXECUTE_HANDLER) {
            ok = false;
        }
        hooked.store(ok, std::memory_order_release);
    }
    ReleaseSRWLockExclusive(&hookLock);
    return ok;
}
}  // namespace cvr::physxstep
