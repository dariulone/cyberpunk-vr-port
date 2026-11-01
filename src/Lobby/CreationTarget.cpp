// VRCreationTarget(entity) -> Bool: the game's character customization dresses `entity` instead of its own preview
// puppet, so that the lobby's figure (mods/cet/CyberpunkVRPort_Lobby/modules/figure.lua) -- an NPC of the player's
// TPP template standing in the lobby -- wears what the player picks and what an engram carries. A null entity gives
// the game its own puppet back.
//
// gameuiCharacterCustomizationSystem (game 2.5, IDA headless): its option calls -- ApplyChangeToOption (vtable
// +0x278 -> sub_142234190 -> sub_142D888D0), the presets, TriggerVoiceToneSample (+0x298) -- all apply to the one
// puppet the system holds as a weak handle at +0x108 (sub_142D89F94 locks it, then puts the option's appearance
// parts on that entity through the appearance system). The preview puppet the new game's screens spawn
// (gameuiCharacterCreationPuppetPreviewGameController) sits there, out of the world's sight. Checked live: the
// system's vtable at RVA 0x36B6280, the preview NPCPuppet at +0x108.
#include "Natives/NativeFunctions.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>

extern void Log(const char*, ...);

namespace {
constexpr uintptr_t kSystemVtable = 0x36B6280;  // gameuiCharacterCustomizationSystem
constexpr size_t kTarget = 0x108;               // WeakHandle: the puppet the options go onto

RED4ext::WeakHandle<RED4ext::ISerializable> g_gameTarget;
bool g_swapped = false;

uint8_t* System() {
    auto* engine = RED4ext::CGameEngine::Get();
    auto* game = engine && engine->framework ? engine->framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* type = rtti ? rtti->GetClass("gameuiICharacterCustomizationSystem") : nullptr;
    auto* system = game && type ? reinterpret_cast<uint8_t*>(game->GetSystem(type)) : nullptr;
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if (!system || *reinterpret_cast<uintptr_t*>(system) != base + kSystemVtable) return nullptr;
    return system;
}
}  // namespace

void VRCreationTarget(RED4ext::IScriptable*, RED4ext::CStackFrame* f, bool* out, int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> entity;
    RED4ext::GetParameter(f, &entity);
    ++f->code;
    auto* system = System();
    if (!system) {
        Log("[VRLobby] creation target: no character customization system of the known layout\n");
        if (out) *out = false;
        return;
    }
    auto& target = *reinterpret_cast<RED4ext::WeakHandle<RED4ext::ISerializable>*>(system + kTarget);
    if (entity) {
        if (!g_swapped) {
            g_gameTarget = target;
            g_swapped = true;
        }
        target = static_cast<const RED4ext::Handle<RED4ext::ISerializable>&>(entity);
    } else if (g_swapped) {
        target = g_gameTarget;
        g_gameTarget.Reset();
        g_swapped = false;
    }
    if (out) *out = true;
}
