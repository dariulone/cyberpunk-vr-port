// The prompts' buttons (include/Hooks/PromptButtons.hpp): the part of the controller icon atlas the game wants to
// show, mapped onto the physical button that produces that pad button through the port's XInput merge, and the
// native that hands it to the game's UI scripts (mods/redscript/CyberpunkVRPort_Prompts).
#include "Hooks/PromptButtons.hpp"
#include "Hooks/Handedness.hpp"
#include "Hooks/VehicleButtons.hpp"
#include "Hooks/Hook.hpp"
#include "Camera/CameraState.hpp"
#include "Core/GameBuild.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Utils/DebugGate.hpp"
#include "MinHook.h"

#include <RED4ext/RED4ext.hpp>

#include <atomic>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <set>

extern "C" __declspec(dllexport) int32_t CyberpunkVR_VehicleDialogConfirmOnA;   // src/Hooks/XInput.cpp

namespace cvr::input {

PromptLayout CurrentPromptLayout() {
    PromptLayout layout;
    const auto seat = VehicleButtonPolicy(g_isInVehicle, g_vehicleState.load(std::memory_order_relaxed));
    layout.vehicle = seat.confirmOnA && seat.exitOnX && CyberpunkVR_VehicleDialogConfirmOnA != 0;
    layout.leftHanded = g_liveControls.xrLeftHanded != 0;
    return layout;
}

std::string PromptPart(std::string_view part, const PromptLayout& layout) {
    // A part is a button and a state: "x", "x_hold", "x_disabled".
    std::string_view base = part, state;
    for (std::string_view suffix : {"_hold", "_disabled"}) {
        if (part.size() > suffix.size() && part.substr(part.size() - suffix.size()) == suffix) {
            base = part.substr(0, part.size() - suffix.size());
            state = suffix;
            break;
        }
    }
    std::string_view physical = base;
    if (layout.vehicle) {
        if (base == "x") physical = "a";        // DialogConfirm / Choice1: A mirrors X while seated
        else if (base == "b") physical = "x";   // ExitVehicle: a held X becomes B
    }
    std::string out(physical);
    out += state;
    // The seat's mirrors are the merge's roles; left-handed, a role's button is the other hand's.
    return layout.leftHanded ? MirroredPart(out) : out;
}

namespace {

// THE ICON OF A KEY, for every inkInputDisplayController (the HUD's and the menus' button hints) and the hold
// indicators: 2.5 sub_1407885C4(mapping, {EInputKey, hold}, out, device) looks the key up in the device's table of
// engine\ink\input\input_key_icon_dictionary.json and writes the atlas part's name at out+24. The part is mapped
// after the lookup, the way a dialogue line's is (VRPromptPart), so the key the game binds stays the key it binds
// and only its picture is the physical button's. A device of 0 is the mapping's current one (+504); 1 is the
// keyboard, whose keys are no hand's.
using IconLookup = char(__fastcall*)(void* mapping, void* key, uint8_t* out, int device);
IconLookup originalIconLookup = nullptr;
constexpr uintptr_t kIconLookupRva = 0x7885C4;

char __fastcall IconLookup_(void* mapping, void* key, uint8_t* out, int device) {
    const char found = originalIconLookup(mapping, key, out, device);
    if (!found || !out || !mapping) return found;
    const int kind = device ? device : *reinterpret_cast<const int32_t*>(static_cast<const uint8_t*>(mapping) + 504);
    if (kind == 1) return found;
    const PromptLayout layout = CurrentPromptLayout();
    if (!layout.vehicle && !layout.leftHanded) return found;
    auto& part = *reinterpret_cast<RED4ext::CName*>(out + 24);
    const char* text = part.ToString();
    if (!text || !*text) return found;
    const std::string mapped = PromptPart(text, layout);
    if (mapped == text) return found;
    if (cvr::RuntimeDiagnosticsEnabled()) {
        // Each mapping once a session: what a prompt asked for and what it was shown.
        static std::mutex mutex;
        static std::set<std::string> logged;
        std::lock_guard lock(mutex);
        if (logged.size() < 64 && logged.insert(std::string(text) + ">" + mapped).second)
            Log("[prompt-icon] %s -> %s (vehicle=%d left-handed=%d)\n", text, mapped.c_str(), layout.vehicle,
                layout.leftHanded);
    }
    part = RED4ext::CName(mapped.c_str());
    return found;
}

bool InstallIconLookup() {
    if (!cvr::game_build::Supported()) return false;
    const auto target = reinterpret_cast<void*>(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) + kIconLookupRva);
    if (std::memcmp(target, "\x48\x89\x5c\x24\x08\x48\x89\x74\x24\x10\x44\x89\x4c\x24\x20\x55", 16)) return false;
    if (MH_CreateHook(target, reinterpret_cast<void*>(&IconLookup_), reinterpret_cast<void**>(&originalIconLookup)) !=
        MH_OK)
        return false;
    if (MH_EnableHook(target) != MH_OK) {
        MH_RemoveHook(target);
        return false;
    }
    return true;
}
CVR_HOOK("PromptIcons", ::cvr::hooks::Stage::Boot, 84, InstallIconLookup);

}  // namespace

// What gameuiMenuGameController.RefreshInputIcons does (2.5 sub_142DD2F98): the icon mapping's listeners told the
// device changed (sub_140798A10, the engine's +528 being the mapping), so every icon widget looks its key up again.
// It calls them on the spot, so the caller is the main thread.
bool RefreshPromptIcons() {
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    constexpr uintptr_t kNotifyRva = 0x798A10, kEngineRva = 0x3C2B390;
    if (!cvr::game_build::Supported() || !originalIconLookup) return false;
    if (std::memcmp(reinterpret_cast<void*>(base + kNotifyRva), "\x48\x89\x5c\x24\x10\x48\x89\x74\x24\x18\x55\x57", 12))
        return false;
    const auto engine = *reinterpret_cast<const uintptr_t*>(base + kEngineRva);
    const auto mapping = engine ? *reinterpret_cast<const uintptr_t*>(engine + 528) : 0;
    if (!mapping) return false;
    using Notify = int64_t(__fastcall*)(uintptr_t listeners, uint32_t, uint32_t);
    reinterpret_cast<Notify>(base + kNotifyRva)(mapping + 48, 0, *reinterpret_cast<const uint32_t*>(mapping + 504));
    return true;
}

bool UpdatePromptIcons(RED4ext::CGameApplication*) {
    // The layout the icons on screen were looked up with; a widget made later looks its key up itself.
    static PromptLayout shown;
    const PromptLayout now = CurrentPromptLayout();
    if (now.vehicle == shown.vehicle && now.leftHanded == shown.leftHanded) return false;
    if (RefreshPromptIcons()) {
        if (cvr::RuntimeDiagnosticsEnabled())
            Log("[prompt-icon] refreshed (vehicle=%d left-handed=%d)\n", now.vehicle, now.leftHanded);
        shown = now;
    }
    return false;
}

}  // namespace cvr::input

// VRPromptPart(part) -> CName: the icon part of the physical button that presses the pad button `part` names.
void VRPromptPart(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, RED4ext::CName* aOut, int64_t) {
    RED4ext::CName part;
    RED4ext::GetParameter(aFrame, &part);
    aFrame->code++;
    if (!aOut) return;
    *aOut = part;
    const char* text = part.ToString();
    if (!text || !*text) return;
    const std::string mapped = cvr::input::PromptPart(text, cvr::input::CurrentPromptLayout());
    if (mapped != text) *aOut = RED4ext::CName(mapped.c_str());
}
