// EntityAttach -- a thing carried by a slot of another entity, the way the game carries a weapon in a hand.
//
// HOW THE GAME CARRIES A THING, verified against game 2.5 with IDA headless and live (2026-10-28: x64dbg on
// entSlotComponent's attachment factory while AddItemToSlot put an item in WeaponLeft). Nothing places a held item
// each frame. Its entity's root component (entEntity::transformComponent, +0xB0) is made a CHILD of the owner's slot
// component in the components' transform graph:
//
//     sub_14044764C  (RVA 0x44764C)  entity, parent component handle, { slot name, flag }, keep world transform
//       sub_1404478D8                 parent IPlacedComponent vf70 (+0x230) makes the attachment -- entSlotComponent
//                                     an entSlotAttachment for a named slot (sub_140201114), any other component an
//                                     entHardAttachment -- and both components register it (sub_1403253F4)
//
// The transform update then moves the child in the same pass as the slot's bone, so it cannot trail the hand the way
// a world entity placed by script (or steered by its body) does -- a frame behind the hand, 21 cm at a run
// (mods/cet/CyberpunkVRPort_Reload/reload/reload.lua, THE CARRIER). The engine runs it from the
// RuntimeSystemEntityTransactor's attach transactions (sub_1404474F4) and directly (sub_1429939AC: an entity onto
// another's root; sub_1407E250C). Keeping the world transform makes the child's local transform the slot's inverse
// times where the child is, so a thing taken stays where the hand closed on it.
//
//     sub_1404476A8  (RVA 0x4476A8)  entity, keep world transform, component (null: the root)
//
// is the reverse: the root's incoming transform attachment taken out of both components; keeping the world transform
// writes the root's stored world pose (+0xE0 fixed-point position, +0xF0 orientation) as its local one. For a root a
// slot carried that pose is not where the slot carried it (measured 25 cm off, 2026-10-28), so the caller places the
// root itself where it saw it; keeping the world only bounds what is drawn if it does not.
//
// The slot carries the component, not its physics: a kinematic body stays where it was when the root was attached
// unless the caller moves it (VRBodySetPos), and made dynamic again it puts the entity back there.
//
// A physical body must not be Dynamic while its component has a connected parent transform (the engine's own
// validation: "of Dynamic type ... cannot have a connected parentTransform"): the caller makes it kinematic before
// the attach, and after the detach places it and makes it dynamic again (mods/cet/CyberpunkVRPort_Lobby/modules/
// items.lua).
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstring>

#include "Core/GameBuild.hpp"
#include "Natives/NativeFunctions.hpp"

extern "C" {
// [0] attaches made, [1] detaches made, [2] attaches refused (no root, a root already attached, not a placed
// component), [3] calls that faulted.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DebugEntityAttach[4]{};
}

namespace {
const uintptr_t base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
constexpr uintptr_t kAttachRva = 0x44764C;
constexpr uintptr_t kDetachRva = 0x4476A8;
// IPlacedComponent: incoming attachments are kept first in the attachment list (+0x70), their number at +0x80.
constexpr size_t kIncomingCount = 0x80;

// What sub_1404478D8 reads: the slot's name, and a flag it copies into the attachment (bit 3 of +0x50, which an
// entSlotAttachment carries by default).
struct AttachRequest {
    uint64_t slot;
    uint8_t flag;
    uint8_t pad[7];
};
using AttachFn = void(__fastcall*)(void* entity, void* parent, AttachRequest* request, bool keepWorld);
using DetachFn = void(__fastcall*)(void* entity, bool keepWorld, void* component);

bool Verified() {
    static const bool ok = [] {
        if (!cvr::game_build::Supported()) return false;
        const uint8_t attach[] = {0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x10,0x57,0x48,0x83,0xec,0x30,
                                  0x48,0x8b,0x89,0xb0,0x00,0x00,0x00};
        const uint8_t detach[] = {0x48,0x8b,0xc4,0x48,0x89,0x58,0x08,0x48,0x89,0x70,0x10,0x48,0x89,0x78,0x18,
                                  0x55,0x48,0x8d,0x68,0xe8};
        return !memcmp(reinterpret_cast<void*>(base + kAttachRva), attach, sizeof(attach)) &&
               !memcmp(reinterpret_cast<void*>(base + kDetachRva), detach, sizeof(detach));
    }();
    return ok;
}

bool IsA(RED4ext::IScriptable* object, const char* className) {
    auto* cls = RED4ext::CRTTISystem::Get()->GetClass(className);
    return object && cls && object->GetType()->IsA(cls);
}

// The number of attachments the component is the child of; -1 when it cannot be read.
int Incoming(void* component) {
    __try {
        return *reinterpret_cast<uint16_t*>(static_cast<uint8_t*>(component) + kIncomingCount);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -1;
    }
}

// No C++ objects in these two: SEH and unwinding do not mix.
bool CallAttach(void* entity, void* parent, AttachRequest* request) {
    __try {
        reinterpret_cast<AttachFn>(base + kAttachRva)(entity, parent, request, true);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

bool CallDetach(void* entity) {
    __try {
        reinterpret_cast<DetachFn>(base + kDetachRva)(entity, true, nullptr);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

RED4ext::ent::IPlacedComponent* Root(RED4ext::IScriptable* object) {
    if (!IsA(object, "entEntity")) return nullptr;
    return static_cast<RED4ext::ent::Entity*>(object)->transformComponent;
}
}  // namespace

// VRAttachToSlot(entity, parent, slot) -> Bool: the entity's root made a child of the parent component's slot, where
// it is now (parent: an entSlotComponent such as the player's ItemAttachmentSlots, slot: one of its slots, WeaponLeft
// or WeaponRight for the hands). True once the root is attached.
void VRAttachToSlot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> entity, parent;
    RED4ext::CName slot;
    RED4ext::GetParameter(aFrame, &entity);
    RED4ext::GetParameter(aFrame, &parent);
    RED4ext::GetParameter(aFrame, &slot);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (!Verified()) return;
    auto* root = Root(entity.instance);
    if (!root || !IsA(parent.instance, "entIPlacedComponent") || Incoming(root) != 0) {
        CyberpunkVR_DebugEntityAttach[2].fetch_add(1, std::memory_order_relaxed);
        return;
    }
    AttachRequest request{slot.hash, 1, {}};
    if (!CallAttach(entity.instance, &parent, &request)) {
        CyberpunkVR_DebugEntityAttach[3].fetch_add(1, std::memory_order_relaxed);
        return;
    }
    const bool attached = Incoming(root) > 0;
    CyberpunkVR_DebugEntityAttach[attached ? 0 : 2].fetch_add(1, std::memory_order_relaxed);
    if (aOut) *aOut = attached;
}

// VRDetachFromSlot(entity) -> Bool: the root back in the world where it is now. True once nothing carries it.
void VRDetachFromSlot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> entity;
    RED4ext::GetParameter(aFrame, &entity);
    aFrame->code++;
    if (aOut) *aOut = false;
    if (!Verified()) return;
    auto* root = Root(entity.instance);
    if (!root) return;
    if (Incoming(root) == 0) {
        if (aOut) *aOut = true;
        return;
    }
    if (!CallDetach(entity.instance)) {
        CyberpunkVR_DebugEntityAttach[3].fetch_add(1, std::memory_order_relaxed);
        return;
    }
    CyberpunkVR_DebugEntityAttach[1].fetch_add(1, std::memory_order_relaxed);
    if (aOut) *aOut = Incoming(root) == 0;
}
