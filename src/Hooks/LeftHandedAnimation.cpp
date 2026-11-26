#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
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
#include "Natives/NativeFunctions.hpp"  // the natives defined here, with C linkage in the Rust build
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_EquipUnequipItem.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimGraphResourceContainer.hpp>
#include <atomic>
#include <cstdio>
#include <iterator>

#if !CVR_RUST_VRIK
extern "C" {
#endif
// Single-wield graphs attached for the left hand (diagnostics).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LeftHandedGraphsAttached{};
#endif
// The controller whose names are mirrored (src/Hooks/LeftHandedMirror.cpp), and the original ApplyFeature.
#if !CVR_RUST_VRIK
extern std::atomic<uintptr_t> CyberpunkVR_LeftHandedController;
#endif
#if !CVR_RUST_VRIK
uint64_t cvr_left_handed_apply_feature(uintptr_t controller, uint64_t name, void* feature);
bool cvr_left_handed_installed();
#endif
#if !CVR_RUST_VRIK
}
#endif
// The doors ask whether a mirrored feature is an item handling, and its state and type.
#if !CVR_RUST_VRIK   // the Rust build's: cvr-rig's vrik/left_handed_animation.rs, the C name cvr-render's cpp_services.rs
extern "C" uint32_t cvr_cpp_item_handling_bits(const void* feature) {
    if (!feature) return 0xFFFF;
    // an item handling's state and type: AnimFeature_EquipUnequipItem itemState +44, itemType +48
    const auto* instance = *reinterpret_cast<RED4ext::ISerializable* const*>(feature);
    static RED4ext::CClass* equip = RED4ext::CRTTISystem::Get()->GetClass(
        RED4ext::anim::AnimFeature_EquipUnequipItem::NAME);
    if (instance && equip && const_cast<RED4ext::ISerializable*>(instance)->GetType() == equip) {
        const auto* item = reinterpret_cast<const RED4ext::anim::AnimFeature_EquipUnequipItem*>(instance);
        return (static_cast<uint32_t>(item->itemState) & 0xFF) | (static_cast<uint32_t>(item->itemType) & 0xFF) << 8;
    }
    return 0xFFFF;
}
namespace {
uint64_t NameHash(const char* s) {
    uint64_t h = 0xCBF29CE484222325ull;  // CName: FNV-1a, 64 bits
    for (; *s; ++s) h = (h ^ static_cast<uint8_t>(*s)) * 0x100000001B3ull;
    return h;
}
// An animation controller's graph slots: a graph attached to / detached from a slot on behalf of a source.
using SlotGraphFn = uint64_t(__fastcall*)(uintptr_t controller, RED4ext::CName slot,
                                          RED4ext::Handle<RED4ext::anim::AnimGraph>* graph, RED4ext::CName source);
// Graph slots: attach 0xD61900, detach 0xD617E8 -- what a player state machine's start calls for the graph its
// definition names (0x9069D8, 0x906B88). Resolved at the first draw, against the bytes the port was written for.
constexpr uintptr_t kAttachGraph = 0xD61900;
constexpr uintptr_t kDetachGraph = 0xD617E8;
SlotGraphFn s_attachGraph{};
SlotGraphFn s_detachGraph{};
bool Matches(uintptr_t base, uintptr_t rva, const char* hex) {
    for (size_t i = 0; hex[2 * i]; ++i) {
        unsigned byte{};
        if (std::sscanf(hex + 2 * i, "%2x", &byte) != 1 || *reinterpret_cast<const uint8_t*>(base + rva + i) != byte)
            return false;
    }
    return true;
}
void ResolveGraphSlots() {
    static const bool resolved = [] {
        const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        if (base && Matches(base, kAttachGraph, "488bc448895808488968104889701848") &&
            Matches(base, kDetachGraph, "488bc448895808488968104889701848")) {
            s_attachGraph = reinterpret_cast<SlotGraphFn>(base + kAttachGraph);
            s_detachGraph = reinterpret_cast<SlotGraphFn>(base + kDetachGraph);
            return true;
        }
        Log("LeftHandedAnimation: graph slot natives mismatch; the left hand's single wield cannot attach its graph.\n");
        return false;
    }();
    (void)resolved;
}
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
    if (!controller || !cls || !cvr_left_handed_installed()) return;
    for (const char* name : {"rightHandItemHandling", "leftHandItemHandling"}) {
        auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature*>(cls->CreateInstance(true));
        if (!feature) return;
        RED4ext::Handle<RED4ext::anim::AnimFeature> handle(feature);
        cvr_left_handed_apply_feature(controller, NameHash(name), &handle);
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
    ResolveGraphSlots();
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
#endif
#endif
