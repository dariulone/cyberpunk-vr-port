// LeftHandedAnimation -- the weapon alone in the LEFT hand animates as the game's single wield, on the other side
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
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_EquipUnequipItem.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimGraphResourceContainer.hpp>
#include <MinHook.h>
#include <atomic>
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
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_LeftHandedNamesMirrored{};
// The controller whose names are mirrored, 0 while none is.
__declspec(dllexport) std::atomic<uintptr_t> CyberpunkVR_LeftHandedController{};
__declspec(dllexport) LeftHandedNameRecord CyberpunkVR_LeftHandedNameLog[64]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LeftHandedNameLogCount{};
// Single-wield graphs attached for the left hand (diagnostics).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LeftHandedGraphsAttached{};
}

namespace {
using ApplyFeatureFn = uint64_t(__fastcall*)(uintptr_t controller, uint64_t name, void* feature);
using PushEventFn = uint64_t(__fastcall*)(uintptr_t controller, uint64_t name);
// An animation controller's graph slots: a graph attached to / detached from a slot on behalf of a source.
using SlotGraphFn = uint64_t(__fastcall*)(uintptr_t controller, RED4ext::CName slot,
                                          RED4ext::Handle<RED4ext::anim::AnimGraph>* graph, RED4ext::CName source);
ApplyFeatureFn s_applyFeature{};
PushEventFn s_pushEvent{};
SlotGraphFn s_attachGraph{};
SlotGraphFn s_detachGraph{};

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
    uint32_t handling = 0xFFFF;
    if (feature) {
        // an item handling's state and type: AnimFeature_EquipUnequipItem itemState +44, itemType +48
        const auto* instance = *reinterpret_cast<RED4ext::ISerializable* const*>(feature);
        static RED4ext::CClass* equip = RED4ext::CRTTISystem::Get()->GetClass(
            RED4ext::anim::AnimFeature_EquipUnequipItem::NAME);
        if (instance && equip && const_cast<RED4ext::ISerializable*>(instance)->GetType() == equip) {
            const auto* item = reinterpret_cast<const RED4ext::anim::AnimFeature_EquipUnequipItem*>(instance);
            handling = (static_cast<uint32_t>(item->itemState) & 0xFF) | (static_cast<uint32_t>(item->itemType) & 0xFF) << 8;
        }
    }
    const uint32_t at = CyberpunkVR_LeftHandedNameLogCount.fetch_add(1, std::memory_order_relaxed);
    CyberpunkVR_LeftHandedNameLog[at % std::size(CyberpunkVR_LeftHandedNameLog)] = {name, kind | 1u << 8 | handling << 16, 0};
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

uint64_t __fastcall ApplyFeature(uintptr_t controller, uint64_t name, void* feature) {
    return s_applyFeature(controller, Mirrored(controller, name, 1, feature), feature);
}

uint64_t __fastcall PushEvent(uintptr_t controller, uint64_t name) {
    return s_pushEvent(controller, Mirrored(controller, name, 2, nullptr));
}

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
// Graph slots: attach 0xD61900, detach 0xD617E8 -- what a player state machine's start calls for the graph its
// definition names (0x9069D8, 0x906B88).
constexpr uintptr_t kAttachGraph = 0xD61900;
constexpr uintptr_t kDetachGraph = 0xD617E8;

bool Install() {
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if (!base || !Matches(base, kApplyFeature, "488bc4488958084889681048897018") ||
        !Matches(base, kPushEvent, "48895c24084889742410574883ec70")) {
        Log("LeftHandedAnimation: native ABI mismatch; hooks not installed.\n");
        return false;
    }
    if (Matches(base, kAttachGraph, "488bc448895808488968104889701848") &&
        Matches(base, kDetachGraph, "488bc448895808488968104889701848")) {
        s_attachGraph = reinterpret_cast<SlotGraphFn>(base + kAttachGraph);
        s_detachGraph = reinterpret_cast<SlotGraphFn>(base + kDetachGraph);
    } else {
        Log("LeftHandedAnimation: graph slot natives mismatch; the left hand's single wield cannot attach its graph.\n");
    }
    for (size_t i = 0; i < std::size(kTwins); ++i) {
        s_twins[i][0] = NameHash(kTwins[i][0]);
        s_twins[i][1] = NameHash(kTwins[i][1]);
    }
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) return false;
    auto* apply = reinterpret_cast<void*>(base + kApplyFeature);
    auto* push = reinterpret_cast<void*>(base + kPushEvent);
    if (MH_CreateHook(apply, reinterpret_cast<void*>(&ApplyFeature), reinterpret_cast<void**>(&s_applyFeature)) != MH_OK)
        return false;
    if (MH_CreateHook(push, reinterpret_cast<void*>(&PushEvent), reinterpret_cast<void**>(&s_pushEvent)) != MH_OK) {
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

namespace {
// WHEN THE HANDS TRADE BRANCHES, BOTH START EMPTY-HANDED. A feature stays on the controller until it is written
// again, and each hand's state now reaches the other branch's name: the item handling the left hand left under
// leftHandItemHandling before the trade (a weapon) stayed there after it, where nothing writes it any more -- and
// with both handlings reading "ranged weapon" the graph lays the pair's branch, driven by nothing, over the whole
// left arm (BlendByMaskDynamic _base_left_arm_full). The trade happens as a draw begins; the hand that draws sends
// its handling right after.
void EmptyHandlings(uintptr_t controller) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(RED4ext::anim::AnimFeature_EquipUnequipItem::NAME) : nullptr;
    if (!controller || !cls || !s_applyFeature) return;
    for (const char* name : {"rightHandItemHandling", "leftHandItemHandling"}) {
        auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature*>(cls->CreateInstance(true));
        if (!feature) return;
        RED4ext::Handle<RED4ext::anim::AnimFeature> handle(feature);
        s_applyFeature(controller, NameHash(name), &handle);
    }
}

// THE GRAPH THE SINGLE WIELD LIVES IN COMES WITH THE RIGHT HAND'S WEAPON. The player's base graph plays a weapon in
// hand from a graph slot (player_base.animgraph: GraphSlot "ranged"), and the graph in it -- player_dualwield, both
// branches -- is attached by the right hand's weapon state machine as it starts: its definition names it
// (playerStateMachineDefinitions.WeaponRight.animGraphName / .animGraphSlot, "ranged"), the left hand's names none,
// since the game never draws a weapon in the left hand without one in the right. So a weapon drawn into the left
// hand before any into the right found the slot empty -- the whole upper body at the rig's reference pose,
// whatever its features or animations, until a draw into the right hand attached the graph for the rest of the
// session (measured 2026-10-29: the slot's attachments logged; none for the left draws of a fresh load). The left
// hand's draw attaches it here the way the right hand's state machine does at each start: the graph the container
// component looks up by that name, detached and attached again on behalf of the same source ("State machine"),
// so either hand's start replaces the other's.
void AttachSingleWieldGraph(uintptr_t controller, RED4ext::CName graphName, RED4ext::CName slot) {
    if (!s_attachGraph || !s_detachGraph || !graphName || !slot) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    static RED4ext::CClass* containerType =
        rtti ? rtti->GetClass(RED4ext::ent::AnimGraphResourceContainer::NAME) : nullptr;
    auto* entity = reinterpret_cast<RED4ext::ent::IComponent*>(controller)->owner;
    if (!entity || !containerType) return;
    for (auto& component : entity->components) {
        if (!component || component->GetType() != containerType) continue;
        auto* container = reinterpret_cast<RED4ext::ent::AnimGraphResourceContainer*>(component.GetPtr());
        for (auto& entry : container->animGraphLookupTable) {
            if (entry.graphName != graphName) continue;
            if (!entry.animGraphResource.IsLoaded()) {
                Log("LeftHandedAnimation: the single wield's graph is not loaded; not attached.\n");
                return;
            }
            RED4ext::Handle<RED4ext::anim::AnimGraph> graph = entry.animGraphResource.Get();
            const RED4ext::CName source("State machine");
            s_detachGraph(controller, slot, &graph, source);
            s_attachGraph(controller, slot, &graph, source);
            CyberpunkVR_LeftHandedGraphsAttached.fetch_add(1, std::memory_order_relaxed);
            return;
        }
    }
}
}  // namespace

// VRLeftHandedAnimations(controller, on, graph, slot): the player's animation controller, and whether the weapon is
// alone in the left hand -- its names are mirrored while it is, and the single wield's graph (the right hand's weapon
// state machine's: its name in the container, the slot it goes in) is attached as the draw begins.
void VRLeftHandedAnimations(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> controller;
    bool on{};
    RED4ext::CName graph;
    RED4ext::CName slot;
    RED4ext::GetParameter(frame, &controller);
    RED4ext::GetParameter(frame, &on);
    RED4ext::GetParameter(frame, &graph);
    RED4ext::GetParameter(frame, &slot);
    ++frame->code;
    const auto component = controller ? reinterpret_cast<uintptr_t>(controller.GetPtr()) : 0;
    const auto next = on ? component : 0;
    const auto previous = CyberpunkVR_LeftHandedController.exchange(next, std::memory_order_relaxed);
    // a trade either way, or the weapon alone in the left hand of a player the mirror has not served yet (a load)
    if (component && (on ? previous != component : previous != 0)) EmptyHandlings(component);
    if (component && on) AttachSingleWieldGraph(component, graph, slot);
}
