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
#include "Core/GameBuild.hpp"
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

// Load the same CCO resources from a disposable mirror puppet rather than temporarily replacing the player's head.
// SetCustomizationOptionsResource's script signature says PlayerPuppet, but its native implementation only reads
// entity components and TppHead/RightArm slots (2.5: 22345D0, 2D8BFB4, 2D8BE88, 2D8BDB0).
void VRMirrorAppearanceOptions(RED4ext::IScriptable*, RED4ext::CStackFrame* f, bool* out, int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> entity;bool male=false;
    RED4ext::GetParameter(f,&entity);RED4ext::GetParameter(f,&male);++f->code;
    if(out)*out=false;
    auto* system=System();
    if(!system || !entity || !cvr::game_build::Supported() || cvr::game_build::Overridden() ||
       !entity->GetType()->IsA(RED4ext::CRTTISystem::Get()->GetClass("NPCPuppet")))return;
    // Empty callback object, exactly as the game's public script wrapper constructs it.
    alignas(16) unsigned char callback[80]{};
    const auto vtable=*reinterpret_cast<uintptr_t**>(system);
    using Fn=void(*)(void*,const RED4ext::Handle<RED4ext::IScriptable>&,bool,void*);
    reinterpret_cast<Fn>(vtable[0x238/8])(system,entity,male,callback);
    if(out)*out=*reinterpret_cast<void**>(system+0xA0)!=nullptr;
}

// ResetCustomizationOptionsResource (vtable +0x240 -> 2D8F7FC -> 3744A0) first takes every part the system put on
// its target back off that target (by resource: every copy, the target's own ones too). The mirror makes its double
// that target, so a plain reset strips the reflection of hair, beard and piercings. Reset with no target, then give
// the game back the target it had.
void VRCustomizationReset(RED4ext::IScriptable*, RED4ext::CStackFrame* f, bool* out, int64_t) {
    ++f->code;
    if(out)*out=false;
    auto* system=System();
    if(!system || !cvr::game_build::Supported() || cvr::game_build::Overridden())return;
    auto& target=*reinterpret_cast<RED4ext::WeakHandle<RED4ext::ISerializable>*>(system+kTarget);
    const RED4ext::WeakHandle<RED4ext::ISerializable> kept=target;
    target.Reset();
    const auto vtable=*reinterpret_cast<uintptr_t**>(system);
    using Fn=void(*)(void*);
    reinterpret_cast<Fn>(vtable[0x240/8])(system);
    target=kept;
    if(out)*out=true;
}

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
