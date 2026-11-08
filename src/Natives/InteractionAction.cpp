// VRInteractionAction(action: CName) -> Bool: one interaction input action (ChoiceApply, ChoiceScrollUp,
// ChoiceScrollDown) handed straight to the game's choice visualizers, the listener the input system feeds when the
// player scrolls or confirms a choice hub. Nothing else sees it: no pad state, no other input context (a weapon does
// not reload, a door does not open). The hub that is shown applies it to its highlighted choice exactly as a confirm
// by the player would -- the scene behind the hub then takes that choice's branch. True when a visualizer took it.
//
// Used by the walkable mirror (mods/cet/CyberpunkVRPort_VRIK): a mirror's own "leave" choice is the only path that
// turns the reflection off (device ForceOFF, the mirrors area, the double despawned, the mirror prefab variant), so
// leaving it from a distance takes that choice.
//
// Game 2.5 (IDA headless + one live breakpoint on the dialog hub's input handler):
//   gameinteractionsCManager + 0x80 is a DynArray<Handle> of its interaction contexts (0xA60-byte objects made and
//   appended in sub_140C0414C under the lock at +0x78); each context + 0x40 holds the visualizers' input listener
//   (made in sub_140C04388; vtable RVA 0x309F198: +0 its destructor sub_140C055A4, +8 OnInput
//   sub_140FF8A98(listener, event, result)). OnInput picks
//   the active visualizer (its list at +0x68), calls its handler (vtable +0xF0) -- for a dialog hub
//   gameinteractionsvisDialogVisualizerLogic sub_140FF8E28, which reads only the event's action name at +0x40
//   (ChoiceScrollUp / ChoiceScrollDown / ChoiceApply) -- and sub_140FF8B98 sends the chosen choice to the hub's
//   owner. `result` gets +0x40 = consumed.
#include "Core/GameBuild.hpp"
#include "Natives/NativeFunctions.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>
#include <atomic>
#include <cstdint>

extern void Log(const char*, ...);

namespace {
constexpr uintptr_t kListenerVtable = 0x309F198;   // the visualizers' input listener

constexpr size_t kListener = 0x40;                  // context + 0x40: the listener
constexpr size_t kEventAction = 0x40;               // the input event's action name (CName)
constexpr size_t kResultConsumed = 0x40;
using OnInputFn = int64_t (*)(void* listener, void* event, void* result);

std::atomic<int> g_logged{0};

uint8_t* Manager() {
    auto* engine = RED4ext::CGameEngine::Get();
    auto* game = engine && engine->framework ? engine->framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    if (!game || !rtti) return nullptr;
    for (const char* name : {"gameinteractionsCManager", "gameinteractionsIManager"}) {
        auto* type = rtti->GetClass(name);
        if (auto* system = type ? game->GetSystem(type) : nullptr) return reinterpret_cast<uint8_t*>(system);
    }
    return nullptr;
}

bool Readable(const void* p, size_t n) {
    MEMORY_BASIC_INFORMATION mbi{};
    if (!p || !VirtualQuery(p, &mbi, sizeof(mbi)) || mbi.State != MEM_COMMIT) return false;
    if (!(mbi.Protect & (PAGE_READWRITE | PAGE_READONLY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_READ))) return false;
    return reinterpret_cast<const uint8_t*>(p) + n <= reinterpret_cast<const uint8_t*>(mbi.BaseAddress) + mbi.RegionSize;
}

// A context's listener when `context` is one of the known layout.
uint8_t* ListenerOf(uint8_t* context, uintptr_t want) {
    if (!Readable(context, kListener + 8)) return nullptr;
    auto* listener = *reinterpret_cast<uint8_t**>(context + kListener);
    return Readable(listener, 16) && *reinterpret_cast<uintptr_t*>(listener) == want ? listener : nullptr;
}

// The listener in the contexts array at manager + `off` (a DynArray<Handle>: pointer, capacity, size), or null.
uint8_t* ListenerInArray(uint8_t* manager, size_t off, uintptr_t want) {
    auto* entries = *reinterpret_cast<uint8_t**>(manager + off);
    const uint32_t cap = *reinterpret_cast<uint32_t*>(manager + off + 8);
    const uint32_t count = *reinterpret_cast<uint32_t*>(manager + off + 0xC);
    if (!entries || count == 0 || count > 16 || cap < count || !Readable(entries, 16 * size_t(count))) return nullptr;
    for (uint32_t i = 0; i < count; ++i)
        if (auto* listener = ListenerOf(*reinterpret_cast<uint8_t**>(entries + 16 * size_t(i)), want)) return listener;
    return nullptr;
}

// The first context's listener, or null (logged once). The contexts array is at +0x80 of the manager by the code
// that fills it (sub_140C0414C); when the system object the game instance hands out is not that one, its fields are
// searched once for the array (or a context) leading to a listener of the known vtable, and the offset is kept.
std::atomic<int> g_contextsAt{-1};
uint8_t* Listener() {
    if (!cvr::game_build::Supported()) return nullptr;
    auto* manager = Manager();
    if (!manager || !Readable(manager, 0x800 + 16)) return nullptr;
    const auto want = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) + kListenerVtable;
    const int known = g_contextsAt.load();
    if (known >= 0) {
        if (auto* listener = ListenerInArray(manager, size_t(known), want)) return listener;
        if (auto* listener = ListenerOf(*reinterpret_cast<uint8_t**>(manager + known), want)) return listener;
    }
    for (size_t off = 0; off < 0x800; off += 8) {
        uint8_t* listener = ListenerInArray(manager, off, want);
        if (!listener) listener = ListenerOf(*reinterpret_cast<uint8_t**>(manager + off), want);
        if (listener) {
            if (g_contextsAt.exchange(int(off)) != int(off))
                Log("[Interaction] the visualizers' listener: contexts at +0x%zX of the interaction manager %p\n", off,
                    manager);
            return listener;
        }
    }
    if (g_logged.exchange(1) == 0)
        Log("[Interaction] no visualizers' listener of the known layout in the interaction manager %p: "
            "VRInteractionAction is off\n", manager);
    return nullptr;
}
}  // namespace

void VRInteractionAction(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t) {
    RED4ext::CName action;
    RED4ext::GetParameter(aFrame, &action);
    aFrame->code++;
    if (aOut) *aOut = false;
    auto* listener = Listener();
    if (!listener || action.hash == 0) return;
    alignas(16) uint8_t event[0x100] = {};
    alignas(16) uint8_t result[0x100] = {};
    *reinterpret_cast<uint64_t*>(event + kEventAction) = action.hash;
    auto onInput = *reinterpret_cast<OnInputFn*>(*reinterpret_cast<uintptr_t*>(listener) + 8);
    onInput(listener, event, result);
    if (aOut) *aOut = result[kResultConsumed] != 0;
}
