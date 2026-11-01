// The first-person neck cap's skin (tools/neck_cap/make_neck_cap.py builds it and adds it to the player entities).
//
// The cap closes the body's neck opening, which the game leaves open in the first person (its own cap belongs to the
// head item, and nothing in that slot is drawn in the first person). It is a plain skinned mesh component,
// vrp_neck_cap, so the character creation's skin tone never reaches it: the game sets the tone on the body
// components it knows. This copies the body's: the full body mesh carries the tone as its mesh appearance
// ("02_ca_limestone", or a variant of it -- "_muscularity", "_censored"), and the cap has the game cap's appearances,
// one per tone. The cap takes the longest of its own appearance names the body's begins with; with none (the lobby's
// hologram, a body of another kind) it is switched off until there is one.
//
// Called a few times a second from the game thread (mods/cet/CyberpunkVRPort_VRIK); a walk of the player's
// components and two name compares when nothing changed.

#include "Natives/NativeFunctions.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/Entity.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IComponent.hpp>
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/animAnimatedObject.hpp>
#include <RED4ext/Scripting/Natives/Generated/world/AnimationSystem.hpp>
#include "Natives/NativeHelpers.hpp"

#include <cstring>
#include <string>

extern void Log(const char* fmt, ...);

namespace {
constexpr const char* kCap = "vrp_neck_cap";
// The full body mesh component, one per body.
constexpr const char* kBodies[] = {"t0_000_pma_base__full", "t0_000_pwa_base__full"};

RED4ext::CName Appearance(RED4ext::ent::IComponent* component) {
    auto* type = component->GetType();
    auto* prop = type ? type->GetProperty("meshAppearance") : nullptr;
    return prop ? prop->GetValue<RED4ext::CName>(component) : RED4ext::CName{};
}

// The appearance names of the cap's mesh, once it is loaded.
bool CapAppearances(RED4ext::ent::IComponent* cap, RED4ext::DynArray<RED4ext::Handle<RED4ext::ISerializable>>** out) {
    auto* type = cap->GetType();
    auto* meshProp = type ? type->GetProperty("meshResource") : nullptr;
    auto* mesh = meshProp ? meshProp->GetValue<RED4ext::Handle<RED4ext::ISerializable>*>(cap) : nullptr;
    if (!mesh || !mesh->instance) return false;
    auto* meshType = mesh->instance->GetType();
    auto* apps = meshType ? meshType->GetProperty("appearances") : nullptr;
    if (!apps) return false;
    *out = apps->GetValue<RED4ext::DynArray<RED4ext::Handle<RED4ext::ISerializable>>*>(mesh->instance);
    return *out != nullptr;
}

// The longest of the cap's appearance names that the body's appearance begins with; empty when none does.
RED4ext::CName ToneFor(RED4ext::ent::IComponent* cap, const char* body) {
    RED4ext::DynArray<RED4ext::Handle<RED4ext::ISerializable>>* apps = nullptr;
    if (!body || !CapAppearances(cap, &apps)) return {};
    RED4ext::CName best{};
    size_t bestLength = 0;
    for (auto& app : *apps) {
        if (!app.instance) continue;
        auto* type = app.instance->GetType();
        auto* nameProp = type ? type->GetProperty("name") : nullptr;
        if (!nameProp) continue;
        const auto name = nameProp->GetValue<RED4ext::CName>(app.instance);
        const char* text = name.ToString();
        if (!text) continue;
        const size_t length = std::strlen(text);
        // A whole name, or a name followed by a variant ("_muscularity", "_censored", ...).
        if (length > bestLength && std::strncmp(body, text, length) == 0 && (body[length] == 0 || body[length] == '_')) {
            best = name;
            bestLength = length;
        }
    }
    return best;
}

bool Call(RED4ext::ent::IComponent* component, const char* function, bool* argument) {
    auto* type = component->GetType();
    auto* func = type ? type->GetFunction(function) : nullptr;
    if (!func) return false;
    RED4ext::StackArgs_t args;
    if (argument) args.emplace_back(nullptr, argument);
    return RED4ext::ExecuteFunction(component, func, nullptr, args);
}

bool Enabled(RED4ext::ent::IComponent* component) {
    auto* type = component->GetType();
    auto* prop = type ? type->GetProperty("isEnabled") : nullptr;
    return prop && prop->GetValue<bool>(component);
}
}

// VRNeckCapSync() -> Int32: 0 in step, 1 the cap took the body's tone, 2 the cap was switched off (no tone of its
// own for the body's appearance), 3 the mesh is not loaded yet; -1 no player, -2 no body component, -3 no cap.
void VRNeckCapSync(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, int32_t* out, int64_t) {
    stack->code++;
    int32_t result = 0;
    auto* player = FindPlayerEntity();
    RED4ext::ent::IComponent *cap = nullptr, *body = nullptr;
    if (player) {
        for (auto& handle : player->components) {
            auto* component = handle.instance;
            if (!component) continue;
            const char* name = component->name.ToString();
            if (!name) continue;
            if (!cap && EqualsInsensitive(name, kCap)) cap = component;
            for (const char* candidate : kBodies)
                if (!body && EqualsInsensitive(name, candidate)) body = component;
        }
    }
    if (!player) result = -1;
    else if (!body) result = -2;
    else if (!cap) result = -3;
    else {
        RED4ext::DynArray<RED4ext::Handle<RED4ext::ISerializable>>* apps = nullptr;
        if (!CapAppearances(cap, &apps)) result = 3;
        else {
            const auto bodyTone = Appearance(body);
            const auto tone = ToneFor(cap, bodyTone.ToString());
            if (tone.hash == 0) {
                if (Enabled(cap)) {
                    bool off = false;
                    Call(cap, "Toggle", &off);
                    Log("[neckcap] body appearance %s: no cap tone for it, cap off", bodyTone.ToString());
                    result = 2;
                }
            } else {
                const bool wasOff = !Enabled(cap);
                if (Appearance(cap) != tone) {
                    auto* prop = cap->GetType()->GetProperty("meshAppearance");
                    if (prop) prop->SetValue<RED4ext::CName>(cap, tone);
                    Call(cap, "RefreshAppearance", nullptr);
                    Log("[neckcap] body appearance %s -> cap %s", bodyTone.ToString(), tone.ToString());
                    result = 1;
                }
                if (wasOff) {
                    bool on = true;
                    Call(cap, "Toggle", &on);
                    result = 1;
                }
            }
        }
    }
    if (out) *out = result;
}
