#include "Stereo/DetourRegistry.hpp"
#include "Core/GameBuild.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Utils/StereoLog.hpp"
#include "Render/NativeDefinitionTracking.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/NativeSceneUpdate.hpp"
#include "Render/NativeSourceCbvBinding.hpp"
#include "Render/NativeSourceSrvBinding.hpp"
#include "Render/NativeSourceCbvSnapshot.hpp"
#include "Render/ConstantUploadRegistry.hpp"
#include "Render/NativeInstanceObserver.hpp"

#include <MinHook.h>
#include <windows.h>
#include <intrin.h>

#include <algorithm>
#include <vector>

#if CVR_RUST_VRIK
extern "C" void cvr_detours_register(const char* name, uintptr_t rva, void* detour, void** original,
                                     cvr::detail::DetourWantedFn wanted, bool* installed);
#if CVR_RUST_VRIK
// The engine detours whose hooks and originals are Rust's register from Rust (rust/crates/cvr-render/src/engine_detours.rs).
extern "C" void cvr_engine_detours_attach();
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin registers them in rust/crates/cvr-render/src/engine_detours.rs cvr_static_registrations
namespace {
[[maybe_unused]] const bool kRustEngineDetours = (cvr_engine_detours_attach(), true);
}
#endif
#endif
#endif
namespace cvr {
namespace detail {

namespace {

using namespace cvr::stereo::instance_observer;
#if !CVR_RUST_VRIK   // the Rust build's detours: rust/crates/cvr-render/src/instance_observer_native.rs
NativeWriter originalInstanceWriter{};
NativePublish originalInstancePublish{};
uint32_t __fastcall ObserveInstanceWriter(void* ring,const void* bytes,uint32_t count,uint32_t* cursor){
    if(Dormant())return originalInstanceWriter(ring,bytes,count,cursor);
    auto call=Enter(ring,bytes,count,cursor,reinterpret_cast<uintptr_t>(_ReturnAddress()));
    const auto result=originalInstanceWriter(ring,bytes,count,cursor);Returned(call,result);return result;
}
void __fastcall ObserveInstancePublish(void* ring){
    if(Dormant()){originalInstancePublish(ring);return;}
    auto call=Publication(ring,reinterpret_cast<uintptr_t>(_ReturnAddress()));
    originalInstancePublish(ring);Returned(call);
}
CVR_DETOUR_IF("[instance] completed writer",WriterRva,ObserveInstanceWriter,originalInstanceWriter,Compatible);
CVR_DETOUR_IF("[instance] stamp publication",PublishRva,ObserveInstancePublish,originalInstancePublish,Compatible);
#endif

#if !CVR_RUST_VRIK
// A FUNCTION-LOCAL STATIC, for the same reason as the hook registry's: detours register from static
// constructors in translation units whose initialisation order is unspecified, and a namespace-scope
// head could be zeroed after the first registration and silently drop it.
EngineDetour*& Head() {
    static EngineDetour* head = nullptr;
    return head;
}

#endif

}  // namespace

#if CVR_RUST_VRIK
// The registry and the install pass are rust/crates/cvr-engine/src/detours.rs: every EngineDetour registers there, the
// pass checks the base and the build, runs PrepareBeforeDetours and installs by RVA with the same lines. The instance
// witness and the source selectors that follow read its results.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // no translation unit of the plugin declares an EngineDetour
EngineDetour::EngineDetour(const char* n, uintptr_t r, void* d, void** o, DetourWantedFn w)
    : name(n), rva(r), detour(d), original(o), wanted(w), next(nullptr), installed(false) {
    cvr_detours_register(n, r, d, o, w, &installed);
}
#endif

// InstallEngineDetours and what runs before its pass: rust/crates/cvr-render/src/stereo_start.rs.
#else
EngineDetour::EngineDetour(const char* n, uintptr_t r, void* d, void** o, DetourWantedFn w)
    : name(n), rva(r), detour(d), original(o), wanted(w), next(Head()), installed(false) {
    Head() = this;
}

int InstallEngineDetours() {
    // THE MODULE BASE IS CHECKED, NOT ASSUMED. Reaching here before it is resolved would put every
    // detour at offset zero -- MinHook would happily hook whatever is there, and the symptom would be
    // a crash with our module on the stack and nothing pointing at the cause.
    if (!g_exe_base) {
        log("[detour] REFUSING to install: module base not resolved yet.");
        return -1;
    }
    // Every RVA below belongs to one game build (Core/GameBuild.hpp); on another, each of them is
    // the middle of some unrelated function.
    if (!cvr::game_build::Supported()) {
        log("[detour] REFUSING to install: game %s, these addresses are %s.",
            cvr::game_build::Running(), cvr::game_build::Expected());
        return -1;
    }
    const MH_STATUS st = MH_Initialize();
    if (st != MH_OK && st != MH_ERROR_ALREADY_INITIALIZED) {
        log("[detour] MH_Initialize failed: %d -- no detour installed.", (int)st);
        return -1;
    }

    std::vector<EngineDetour*> due;
    cvr::stereo::preparation::PrepareGraphParentCodeGuards(reinterpret_cast<uintptr_t>(g_exe_base));
    cvr::stereo::source_cbv::PrepareCodeGuards(reinterpret_cast<uintptr_t>(g_exe_base));
    cvr::stereo::source_srv::PrepareCodeGuards(reinterpret_cast<uintptr_t>(g_exe_base));
    cvr::stereo::source_cbv_snapshot::PrepareUploadCodeGuards(reinterpret_cast<uintptr_t>(g_exe_base));
    cvr::stereo::instance_observer::PrepareCodeGuards(reinterpret_cast<uintptr_t>(g_exe_base));
    const bool instanceWitnessPrepared=cvr::stereo::instance_observer::PrepareWitness(reinterpret_cast<uintptr_t>(g_exe_base));
    CyberpunkVR_SourceSrvBindingEnable=0;
    if(GetEnvironmentVariableW(L"CVR_STEREO_SOURCE_SELECTORS",nullptr,0))cvr::stereo::source_srv::ConfigureXstate();
    cvr::stereo::constants::SetCreationAddressObserver(cvr::stereo::source_cbv::ObserveCreationAddress);
    cvr::stereo::preparation::PrepareGraphJobCodeGuards();
    cvr::stereo::scene_update::PrepareCodeGuards();
    cvr::stereo::preparation::SetSceneUpdatePoll(cvr::stereo::scene_update::Poll);
    cvr::stereo::definition_tracking::PrepareCodeGuards();
    for (EngineDetour* d = Head(); d; d = d->next) {
        if (!d->installed) due.push_back(d);
    }
    // Ascending RVA rather than link order: the log then reads in the same order as a disassembly of
    // the image, which is how anybody checking these against a game build reads them.
    std::stable_sort(due.begin(), due.end(),
                     [](const EngineDetour* a, const EngineDetour* b) { return a->rva < b->rva; });

    int failed = 0;
    for (EngineDetour* d : due) {
        if (d->wanted && !d->wanted()) {
            log("[detour] %-46s skipped (disabled)", d->name);
            continue;
        }
        // Left at its old value by tools/patchday/remap.py: hooking it would hook a stranger.
        if (cvr::game_build::Unmapped(d->rva)) {
            log("[detour] %-46s skipped (RVA %#zx not carried to this game build)", d->name, d->rva);
            ++failed;
            continue;
        }
        void* target = reinterpret_cast<void*>(g_exe_base + d->rva);
        const bool ok = MH_CreateHook(target, d->detour, d->original) == MH_OK &&
                        MH_EnableHook(target) == MH_OK;
        d->installed = ok;
        if (!ok) ++failed;
        // EVERY result, with the address. An RVA that no longer holds the expected function is a
        // game-patch problem and this line is the whole diagnosis; the old pass logged the same
        // information forty-six times, written out by hand each time.
        log("[detour] %-46s %s @%p", d->name, ok ? "ok" : "FAILED", target);
    }
    if (failed) {
        log("[detour] %d of %zu FAILED -- the behaviour behind them is absent, not degraded.",
            failed, due.size());
    }
    bool individual{},cached{};
    uint32_t instanceMask{};
    const bool instanceWitnessInstalled=instanceWitnessPrepared &&
        cvr::stereo::instance_observer::InstallWitness(reinterpret_cast<uintptr_t>(g_exe_base));
    if(instanceWitnessInstalled && cvr::stereo::instance_observer::WitnessInstalled())instanceMask|=2;
    log("[instance] successful CAS witness %s",instanceWitnessInstalled?"ok":"FAILED");
    for(auto* d=Head();d;d=d->next){if(d->rva==0x338FC0)individual=d->installed;if(d->rva==0x338ED4)cached=d->installed;
        if(d->installed && d->rva==WriterRva)instanceMask|=1;
        if(d->installed && d->rva==PublishRva)instanceMask|=4;}
    cvr::stereo::instance_observer::SetInstalled(instanceMask);
    if(individual && cached && GetEnvironmentVariableW(L"CVR_STEREO_SOURCE_SELECTORS",nullptr,0) &&
        cvr::stereo::source_srv::Compatible() && cvr::stereo::source_srv::ConfigureXstate())CyberpunkVR_SourceSrvBindingEnable=1;
    return failed;
}

#endif

}  // namespace detail
}  // namespace cvr
