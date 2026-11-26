// LeftHandedMirror (with src/Hooks/LeftHandedAnimation.cpp) -- the weapon alone in the LEFT hand animates as the game's single wield, on the other side
// (xr_left_handed; which hand holds what is mods/redscript/CyberpunkVRPort_LeftHanded).
//
// The player's ranged graph animates the weapon in hand through two mirrored branches, each driven by names of its
// own: the single-wield branch by the features RightWeaponData, rightHandItemHandling, RightReloadData,
// WeaponEquipType and the events RightShoot, RightReload ..., the branch of a pair's second gun by their Left*
// twins. The engine names them after the slot the weapon hangs on (Cyberpunk2077.exe 2.5, RVA 0x218AD00:
// LeftWeaponData / RightWeaponData, 0x218AC60: LeftReload / RightReload ...), so a weapon held alone on WeaponLeft
// drove the pair's branch: a hold made for the second of two guns, no recoil on its rounds.
//
// While the weapon is alone in the left hand, the names reaching the player's animation controller are mirrored at
// its two doors -- ApplyFeature (0x53DC14) and PushEvent (0x824614), which the scripts' features and events and
// the engine's own pass through alike: the left weapon drives the single-wield branch, the empty right hand the
// other. The single-wield animations that branch plays are then the right hand's own mirrored onto the left
// (tools/lefthanded_anims, active under the anim wrapper vr_left_handed): the hold, each round's squeeze, the draw,
// the reload -- the game's single wield, on the left hand. The redscript turns both on and off together, at each
// draw (VRLeftHandedAnimations).
// This file: the two doors and their install, and the exports they keep. In the Rust build the doors are
// rust/crates/cvr-rig/src/vrik/left_handed_animation.rs. The script native that turns the mirror on stays in
// LeftHandedAnimation.cpp.
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <iterator>
// The last names mirrored, as received (diagnostics): name hash, then kind (1 feature, 2 event) | mirrored << 8 |
// an item handling's itemState << 16 | its itemType << 24 (0xFFFF << 16 when not one).
struct LeftHandedNameRecord {
    uint64_t name;
    uint32_t info;
    uint32_t pad;
};
extern "C" {
// Names mirrored so far (diagnostics).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_LeftHandedNamesMirrored{};
#endif
// The controller whose names are mirrored, 0 while none is.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
__declspec(dllexport) std::atomic<uintptr_t> CyberpunkVR_LeftHandedController{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's log is rust/crates/cvr-render/src/render_moved_exports.rs'
__declspec(dllexport) LeftHandedNameRecord CyberpunkVR_LeftHandedNameLog[64]{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LeftHandedNameLogCount{};
#endif
}

// An AnimFeature_EquipUnequipItem's state and type, 0xFFFF for any other feature (src/Hooks/LeftHandedAnimation.cpp).
#if !CVR_RUST_VRIK
extern "C" uint32_t cvr_cpp_item_handling_bits(const void* feature);
#endif

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("LeftHandedAnimation", ::cvr::hooks::Stage::Boot, 91, cvr_hook_left_handed_animation_install);
#endif
#else
namespace {
using ApplyFeatureFn = uint64_t(__fastcall*)(uintptr_t controller, uint64_t name, void* feature);
using PushEventFn = uint64_t(__fastcall*)(uintptr_t controller, uint64_t name);
// An animation controller's graph slots: a graph attached to / detached from a slot on behalf of a source.
ApplyFeatureFn s_applyFeature{};
PushEventFn s_pushEvent{};

// Every name of the ranged graph that has a twin on the other branch.
constexpr const char* kTwins[][2] = {
    {"RightWeaponData", "LeftWeaponData"},       {"rightHandItemHandling", "leftHandItemHandling"},
    {"RightReloadData", "LeftReloadData"},       {"WeaponEquipType", "LeftWeaponEquipType"},
    {"RightReload", "LeftReload"},               {"RightShoot", "LeftShoot"},
    {"RightReloadLoop", "LeftReloadLoop"},       {"EndRightReload", "EndLeftReload"},
    {"RightCycleRound", "LeftCycleRound"},
};
uint64_t s_twins[std::size(kTwins)][2]{};

uint64_t NameHash(const char* s) {
    uint64_t h = 0xCBF29CE484222325ull;  // CName: FNV-1a, 64 bits
    for (; *s; ++s) h = (h ^ static_cast<uint8_t>(*s)) * 0x100000001B3ull;
    return h;
}

void Note(uint64_t name, uint32_t kind, const void* feature) {
    // an item handling's state and type, through the engine's RTTI (src/Hooks/LeftHandedAnimation.cpp)
    const uint32_t handling = feature ? cvr_cpp_item_handling_bits(feature) : 0xFFFF;
    const uint32_t at = CyberpunkVR_LeftHandedNameLogCount.fetch_add(1, std::memory_order_relaxed);
    CyberpunkVR_LeftHandedNameLog[at % std::size(CyberpunkVR_LeftHandedNameLog)] = {name, kind | 1u << 8 | handling << 16, 0};
}
void FillTwins() {
    for (size_t i = 0; i < std::size(kTwins); ++i) {
        s_twins[i][0] = NameHash(kTwins[i][0]);
        s_twins[i][1] = NameHash(kTwins[i][1]);
    }
}
uint64_t Mirrored(uintptr_t controller, uint64_t name, uint32_t kind, const void* feature) {
    if (!controller || controller != CyberpunkVR_LeftHandedController.load(std::memory_order_relaxed)) return name;
    for (const auto& twin : s_twins) {
        if (name == twin[0] || name == twin[1]) {
            CyberpunkVR_LeftHandedNamesMirrored.fetch_add(1, std::memory_order_relaxed);
            Note(name, kind, feature);
            return name == twin[0] ? twin[1] : twin[0];
        }
    }
    return name;
}

}
extern "C" uint64_t CvrLeftHandedApplyFeature(uintptr_t controller, uint64_t name, void* feature) {
    return s_applyFeature(controller, Mirrored(controller, name, 1, feature), feature);
}
extern "C" uint64_t CvrLeftHandedPushEvent(uintptr_t controller, uint64_t name) {
    return s_pushEvent(controller, Mirrored(controller, name, 2, nullptr));
}
// The original ApplyFeature for the script native, and the bench's originals.
extern "C" uint64_t cvr_left_handed_apply_feature(uintptr_t controller, uint64_t name, void* feature) {
    return s_applyFeature(controller, name, feature);
}
extern "C" bool cvr_left_handed_installed() { return s_applyFeature != nullptr; }
extern "C" void cvr_left_handed_bind(uintptr_t apply, uintptr_t push) {
    s_applyFeature = reinterpret_cast<ApplyFeatureFn>(apply);s_pushEvent = reinterpret_cast<PushEventFn>(push);
    FillTwins();
}
namespace {

bool Matches(uintptr_t base, uintptr_t rva, const char* hex) {
    for (size_t i = 0; hex[2 * i]; ++i) {
        unsigned byte{};
        if (std::sscanf(hex + 2 * i, "%2x", &byte) != 1 || *reinterpret_cast<const uint8_t*>(base + rva + i) != byte)
            return false;
    }
    return true;
}

constexpr uintptr_t kApplyFeature = 0x53DC14;
constexpr uintptr_t kPushEvent = 0x824614;

bool Install() {
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if (!base || !Matches(base, kApplyFeature, "488bc4488958084889681048897018") ||
        !Matches(base, kPushEvent, "48895c24084889742410574883ec70")) {
        Log("LeftHandedAnimation: native ABI mismatch; hooks not installed.\n");
        return false;
    }
    FillTwins();
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) return false;
    auto* apply = reinterpret_cast<void*>(base + kApplyFeature);
    auto* push = reinterpret_cast<void*>(base + kPushEvent);
    if (MH_CreateHook(apply, reinterpret_cast<void*>(&CvrLeftHandedApplyFeature), reinterpret_cast<void**>(&s_applyFeature)) != MH_OK)
        return false;
    if (MH_CreateHook(push, reinterpret_cast<void*>(&CvrLeftHandedPushEvent), reinterpret_cast<void**>(&s_pushEvent)) != MH_OK) {
        MH_RemoveHook(apply);
        return false;
    }
    if (MH_EnableHook(apply) != MH_OK || MH_EnableHook(push) != MH_OK) {
        MH_DisableHook(apply);
        MH_DisableHook(push);
        MH_RemoveHook(apply);
        MH_RemoveHook(push);
        return false;
    }
    Log("LeftHandedAnimation: animation controller ApplyFeature/PushEvent installed (2.5).\n");
    return true;
}
CVR_HOOK("LeftHandedAnimation", ::cvr::hooks::Stage::Boot, 91, Install);
}  // namespace
#endif
