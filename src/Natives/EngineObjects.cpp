// ENGINE OBJECTS THE SCRIPTS REACH, WITHOUT CODEWARE (stage 4 of docs/native-frameworks-plan-20261030.md).
//
// Codeware added script methods to engine classes the port's scripts and Lua mods call: an entity's components and
// world transform, a mesh component's resource and appearance, the ink system's layers, the weather. These are the
// same operations as natives of the plugin under the port's own names (VR*), so nothing collides with Codeware when
// it is installed for other mods. What 2.5 already has natively (inkLayerWrapper, SpawnFromExternal, CallbackSystem,
// ScriptableService) is used as the game has it: the layer natives hand out the game's own inkLayerWrapper.
//
// The lobby's exposure (mods/redscript/CyberpunkVRPort_EngineStereo/vrport_lobby_exposure.reds) is here too: the
// script registers for the environment resources as they load and hands them over; the settings themselves are
// native classes no script declares without Codeware.

#include "Natives/NativeFunctions.hpp"

#include <RED4ext/JobQueue.hpp>
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Relocation.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/inkIWidgetComponentWrapper.hpp>
#include <RED4ext/Scripting/Natives/inkLayer.hpp>
#include <RED4ext/Scripting/Natives/inkWorldLayer.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/IWidgetController.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/VirtualWindow.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MorphTargetSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/SkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/world/WeatherState.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Utils.hpp>

#include <chrono>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

extern void Log(const char* fmt, ...);

namespace {

using RED4ext::CName;
using RED4ext::Handle;

// Address-library hashes (red4ext's cyberpunk2077_addresses.json), the functions Codeware calls for the same.
constexpr uint32_t kInkSystemInstanceHash = 2635337807;     // the ink system singleton (a pointer variable)
constexpr uint32_t kPlacedSetTransformHash = 1828854026;    // entIPlacedComponent: sets its world transform
constexpr uint32_t kWeatherSetByNameHash = 2334794340;      // worldRuntimeSystemWeather: blends to a state by name
constexpr uint32_t kWeatherStateHash = 2679119876;          // worldRuntimeSystemWeather: the current state

// A mesh component's resource and appearance loads (2.5 vtable, as Codeware has it).
constexpr size_t kLoadResourceSlot = 0x260;     // uint64_t (IComponent::*)(JobQueue&)
constexpr size_t kRefreshAppearanceSlot = 0x280; // void (IComponent::*)()

template<typename T>
T Param(RED4ext::CStackFrame* frame) {
    T value{};
    RED4ext::GetParameter(frame, &value);
    return value;
}

RED4ext::CClass* Class(const char* name) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    return rtti ? rtti->GetClass(name) : nullptr;
}

bool IsA(RED4ext::IScriptable* object, const char* className) {
    auto* cls = Class(className);
    return object && cls && object->GetType()->IsA(cls);
}

// ---- entities ----------------------------------------------------------------------------------------------------

}  // namespace

void VREntityComponents(RED4ext::IScriptable*, RED4ext::CStackFrame* frame,
                        RED4ext::DynArray<Handle<RED4ext::ent::IComponent>>* out, int64_t) {
    auto entity = Param<Handle<RED4ext::ent::Entity>>(frame);
    frame->code++;
    if (!out) return;
    if (entity)
        for (const auto& component : entity->components) out->PushBack(component);
}

// An entity's components of one class (or derived from it): the list a script would otherwise walk with an IsA call
// on every component, every frame (world touch: ~20 components a device, one of them a screen).
void VREntityComponentsOf(RED4ext::IScriptable*, RED4ext::CStackFrame* frame,
                          RED4ext::DynArray<Handle<RED4ext::ent::IComponent>>* out, int64_t) {
    auto entity = Param<Handle<RED4ext::ent::Entity>>(frame);
    const auto kind = Param<CName>(frame);
    frame->code++;
    if (!out || !entity) return;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(kind) : nullptr;
    if (!cls) return;
    for (const auto& component : entity->components)
        if (component && component->GetType()->IsA(cls)) out->PushBack(component);
}

// An entity's skinned meshes (the three classes the lobby's construct handles, modules/construct.lua): the lobby looks
// through them every frame while an engram's look goes on, and the puppet gathers hundreds of components. Whether
// each is on is left to the script's IsEnabled: filtered here on the component's own flag, a mesh the system had just
// switched on showed for frames before the lobby saw it.
void VREntitySkinnedMeshes(RED4ext::IScriptable*, RED4ext::CStackFrame* frame,
                           RED4ext::DynArray<Handle<RED4ext::ent::IComponent>>* out, int64_t) {
    auto entity = Param<Handle<RED4ext::ent::Entity>>(frame);
    frame->code++;
    if (!out || !entity) return;
    static const RED4ext::CName kinds[] = {"entSkinnedMeshComponent", "entGarmentSkinnedMeshComponent",
                                           "entMorphTargetSkinnedMeshComponent"};
    for (const auto& component : entity->components) {
        if (!component) continue;
        const auto type = component->GetType()->GetName();
        for (const auto& kind : kinds) {
            if (type == kind) {
                out->PushBack(component);
                break;
            }
        }
    }
}

void VREntitySetWorldTransform(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto entity = Param<Handle<RED4ext::ent::Entity>>(frame);
    auto transform = Param<RED4ext::WorldTransform>(frame);
    frame->code++;
    using SetTransformFn = void (*)(RED4ext::ent::IPlacedComponent*, const RED4ext::WorldTransform&);
    static const auto setTransform =
        reinterpret_cast<SetTransformFn>(RED4ext::UniversalRelocBase::Resolve(kPlacedSetTransformHash));
    const bool done = entity && entity->transformComponent && setTransform;
    if (done) setTransform(entity->transformComponent, transform);
    if (out) *out = done;
}

namespace {

// ---- mesh components ---------------------------------------------------------------------------------------------

// Where a mesh component keeps its resource and appearance: the mesh components, the skinned ones (garments
// included) and the morph target ones, each at its own offsets.
struct MeshFields {
    RED4ext::ResourcePath* path = nullptr;
    CName* appearance = nullptr;
};

MeshFields FieldsOf(RED4ext::ent::IComponent* component) {
    if (!component) return {};
    if (IsA(component, "entMeshComponent")) {
        auto* c = reinterpret_cast<RED4ext::ent::MeshComponent*>(component);
        return {&c->mesh.path, &c->meshAppearance};
    }
    if (IsA(component, "entMorphTargetSkinnedMeshComponent")) {
        auto* c = reinterpret_cast<RED4ext::ent::MorphTargetSkinnedMeshComponent*>(component);
        return {&c->morphResource.path, &c->meshAppearance};
    }
    if (IsA(component, "entSkinnedMeshComponent")) {
        auto* c = reinterpret_cast<RED4ext::ent::SkinnedMeshComponent*>(component);
        return {&c->mesh.path, &c->meshAppearance};
    }
    return {};
}

void RefreshAppearance(RED4ext::ent::IComponent* component) {
    auto** table = *reinterpret_cast<void***>(component);
    reinterpret_cast<void (*)(RED4ext::ent::IComponent*)>(table[kRefreshAppearanceSlot / sizeof(void*)])(component);
}

// The component's resource loaded anew and its appearance rebuilt once it is in; with `wait`, the call returns once
// that is done (5 s at most), so the caller's next frame already shows it.
bool Reload(const Handle<RED4ext::ent::IComponent>& component, bool wait) {
    if (!component || !component->owner) return false;
    auto** table = *reinterpret_cast<void***>(component.instance);
    using LoadResourceFn = uint64_t (*)(RED4ext::ent::IComponent*, RED4ext::JobQueue&);
    RED4ext::JobQueue queue;
    reinterpret_cast<LoadResourceFn>(table[kLoadResourceSlot / sizeof(void*)])(component.instance, queue);
    RED4ext::WeakHandle<RED4ext::ent::IComponent> weak(component);
    queue.Dispatch([weak] {
        if (auto held = weak.Lock(); held && held->owner) RefreshAppearance(held.instance);
    });
    if (wait) {
        struct Done {
            std::mutex lock;
            std::condition_variable signal;
            bool done = false;
        };
        auto done = std::make_shared<Done>();
        queue.Dispatch([done] {
            std::lock_guard<std::mutex> guard(done->lock);
            done->done = true;
            done->signal.notify_all();
        });
        std::unique_lock<std::mutex> guard(done->lock);
        done->signal.wait_for(guard, std::chrono::seconds(5), [&] { return done->done; });
    }
    return true;
}

}  // namespace

void VRComponentChangeResource(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    auto path = Param<RED4ext::CString>(frame);
    auto wait = Param<bool>(frame);
    frame->code++;
    const MeshFields fields = FieldsOf(component.instance);
    bool done = false;
    if (fields.path) {
        *fields.path = RED4ext::ResourcePath(path.c_str());
        done = Reload(component, wait);
    }
    if (out) *out = done;
}

void VRComponentChangeAppearance(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    auto appearance = Param<CName>(frame);
    auto wait = Param<bool>(frame);
    frame->code++;
    const MeshFields fields = FieldsOf(component.instance);
    bool done = false;
    if (fields.appearance) {
        *fields.appearance = appearance;
        done = Reload(component, wait);
    }
    if (out) *out = done;
}

// A VISUAL COMPONENT DISSOLVED OUT OR IN: the engine's dithered fade (Rendering/DissolveTime, 0.5 s), the one an
// entity streaming in or an appearance change shows. IVisualComponent keeps a byte of dissolve reasons (+300): drawn
// while none is set, each change between none and some fades the render proxy (2.5: 0x1403D1CD8, the handler of
// enteventsSetDissolveVisibility calls it with the event's reason and mode). The port sets its own reason bit, so
// the game's own dissolves (scanner, streaming) are left as they are. `mode` is the engine's last argument (0..2, as
// its callers pass it); measured 2026-11-05 on the player's body: 0 fades out and in over ~0.45 s, 1 and 2 fade out
// the same but 2 comes back in at once. On a component that is switched off only the bit changes, and the proxy it
// gets when it is switched on again keeps it (dissolved out before Toggle(true): no frame of it is seen).
constexpr uint32_t kVisualDissolveHash = 3960219843;
constexpr uint8_t kPortDissolveReason = 6;

void VRComponentDissolve(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    auto hidden = Param<bool>(frame);
    auto mode = Param<int32_t>(frame);
    frame->code++;
    using DissolveFn = char (*)(RED4ext::ent::IComponent*, bool, uint8_t, uint8_t);
    static const auto dissolve = reinterpret_cast<DissolveFn>(RED4ext::UniversalRelocBase::Resolve(kVisualDissolveHash));
    const bool done = dissolve && component && component->owner && IsA(component.instance, "entIVisualComponent") &&
                      mode >= 0 && mode <= 2;
    if (done) dissolve(component.instance, hidden, kPortDissolveReason, static_cast<uint8_t>(mode));
    if (out) *out = done;
}

// Whether the port's reason holds the component dissolved out (the byte the call above writes).
void VRComponentDissolved(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    frame->code++;
    const bool visual = component && IsA(component.instance, "entIVisualComponent");
    if (out)
        *out = visual && (reinterpret_cast<const uint8_t*>(component.instance)[300] & (1u << kPortDissolveReason)) != 0;
}

// Whether a dissolve reaches the component's render proxy now: the engine passes it on only while the component is
// attached (+136 bit 1, the same test 0x1403D1CD8 makes). A component just switched on needs a few frames for that; a
// fade asked before then only clears the bit, and the proxy then comes in at once (measured 2026-11-05: on, then
// dissolved in 0.6 s later, it faded; the order matters).
void VRComponentDissolveReady(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    frame->code++;
    const bool visual = component && component->owner && IsA(component.instance, "entIVisualComponent");
    if (out) *out = visual && (reinterpret_cast<const uint8_t*>(component.instance)[136] & 2) != 0;
}

void VRComponentRefreshAppearance(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    frame->code++;
    const bool done = component && component->owner && FieldsOf(component.instance).appearance;
    if (done) RefreshAppearance(component.instance);
    if (out) *out = done;
}

// A component told apart from every other: its address, mixed with its name (a component gone, the next one may be
// given its address). Its id is its template's, so the copies of one component the character creation system adds to
// a puppet all carry the id of the puppet's own.
void VRComponentKey(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, uint64_t* out, int64_t) {
    auto component = Param<Handle<RED4ext::ent::IComponent>>(frame);
    frame->code++;
    if (!out) return;
    *out = component ? reinterpret_cast<uint64_t>(component.instance) ^ component->name.hash : 0;
}

namespace {

// ---- the ink system's layers ---------------------------------------------------------------------------------------

// The ink system: its first layer manager holds the layers (2.5, as Codeware has it).
RED4ext::DynArray<Handle<RED4ext::ink::Layer>>* Layers() {
    static auto* const instance = reinterpret_cast<uint8_t**>(RED4ext::UniversalRelocBase::Resolve(kInkSystemInstanceHash));
    uint8_t* system = instance ? *instance : nullptr;
    if (!system) return nullptr;
    auto& managers = *reinterpret_cast<RED4ext::DynArray<RED4ext::SharedPtr<uint8_t>>*>(system + 0x380);
    if (managers.size() == 0 || !managers[0].instance) return nullptr;
    return reinterpret_cast<RED4ext::DynArray<Handle<RED4ext::ink::Layer>>*>(managers[0].instance + 0x38);
}

// The game's own wrapper of a layer (2.5 has it as a native class, with GetVirtualWindow, GetGameControllers ...):
// its one field, the layer, right after IScriptable.
Handle<RED4ext::IScriptable> Wrap(const Handle<RED4ext::ink::Layer>& layer) {
    static RED4ext::CClass* const type = Class("inkLayerWrapper");
    static const bool fits = type && type->GetSize() >= 0x50;
    if (!fits) {
        static bool told = false;
        if (!told) Log("[engine] inkLayerWrapper is not the 2.5 class (size %u): no layers handed out\n",
                       type ? type->GetSize() : 0), told = true;
        return {};
    }
    auto* wrapper = static_cast<RED4ext::IScriptable*>(type->CreateInstance(true));
    *reinterpret_cast<Handle<RED4ext::ink::Layer>*>(reinterpret_cast<uint8_t*>(wrapper) + 0x40) = layer;
    return Handle<RED4ext::IScriptable>(wrapper);
}

}  // namespace

void VRInkLayers(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::DynArray<Handle<RED4ext::IScriptable>>* out,
                 int64_t) {
    frame->code++;
    if (!out) return;
    if (auto* layers = Layers())
        for (const auto& layer : *layers)
            if (layer)
                if (auto wrapped = Wrap(layer)) out->PushBack(wrapped);
}

// The ink system's layer of this native type (inkMenuLayer, inkHUDLayer ...), for the lobby (Lobby/MenuLobby.cpp).
RED4ext::ink::Layer* FindInkLayer(CName type) {
    if (auto* layers = Layers())
        for (const auto& layer : *layers)
            if (layer && layer->GetNativeType()->GetName() == type) return layer.instance;
    return nullptr;
}

void VRInkLayer(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, Handle<RED4ext::IScriptable>* out, int64_t) {
    auto name = Param<CName>(frame);
    frame->code++;
    if (!out) return;
    if (auto* layers = Layers())
        for (const auto& layer : *layers)
            if (layer && layer->GetNativeType()->GetName() == name) {
                *out = Wrap(layer);
                return;
            }
}

// The game controllers of the world's widget components that have a window (the screens of terminals, doors ...).
void VRInkWorldControllers(RED4ext::IScriptable*, RED4ext::CStackFrame* frame,
                           RED4ext::DynArray<RED4ext::WeakHandle<RED4ext::IScriptable>>* out, int64_t) {
    frame->code++;
    if (!out) return;
    static RED4ext::CClass* const worldLayerType = Class("inkWorldLayer");
    auto* layers = Layers();
    if (!layers || !worldLayerType) return;
    for (const auto& layer : *layers) {
        if (!layer || !layer->GetNativeType()->IsA(worldLayerType)) continue;
        auto* world = reinterpret_cast<RED4ext::ink::WorldLayer*>(layer.instance);
        for (const auto& component : world->components) {
            if (!component.instance || !component->GetWindow()) continue;
            auto controller = component->GetGameController();
            if (controller)
                out->PushBack(RED4ext::WeakHandle<RED4ext::IScriptable>(*reinterpret_cast<Handle<RED4ext::IScriptable>*>(&controller)));
        }
        return;
    }
}

namespace {

// ---- weather ------------------------------------------------------------------------------------------------------

// The world's weather system behind the script interface GameInstance.GetWeatherSystem hands out (+0x40: 2.5, as
// Codeware has it). The interface is no game system of the instance's map: the script getter is the way to it. The
// getter (sub_1413A9A10) reads the weather off the runtime scene (instance +0x78) without asking whether there is one,
// and between two sessions there is none: the lobby's once-a-second look at the weather while a new game took over
// from the main menu read address 0x280 (crash 20261102-094655).
uint8_t* WeatherSystem() {
    static RED4ext::CClass* const game = Class("ScriptGameInstance");
    Handle<RED4ext::IScriptable> iface;
    RED4ext::ScriptGameInstance instance;
    if (!game || !instance.instance || !instance.instance->unk78[0]) return nullptr;
    if (!RED4ext::ExecuteFunction(game, RED4ext::CName("GetWeatherSystem"), &iface, instance) || !iface) return nullptr;
    return *reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(iface.instance) + 0x40);
}

}  // namespace

// The weather blends to a state by name and stays there (the weather cycle stops, +0x8C) until the game sets one.
void VRWeatherSet(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto weather = Param<CName>(frame);
    auto blend = Param<float>(frame);
    auto priority = Param<uint32_t>(frame);
    frame->code++;
    using SetFn = bool (*)(uint8_t*, CName, CName, float, uint32_t, uint32_t);
    static const auto set = reinterpret_cast<SetFn>(RED4ext::UniversalRelocBase::Resolve(kWeatherSetByNameHash));
    uint8_t* system = WeatherSystem();
    bool done = system && set && set(system, weather, CName(), blend, 0, priority);
    if (done) *reinterpret_cast<bool*>(system + 0x8C) = false;
    if (out) *out = done;
}

void VRWeatherState(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, CName* out, int64_t) {
    frame->code++;
    using StateFn = void* (*)(uint8_t*, Handle<RED4ext::world::WeatherState>&);
    static const auto state = reinterpret_cast<StateFn>(RED4ext::UniversalRelocBase::Resolve(kWeatherStateHash));
    Handle<RED4ext::world::WeatherState> current;
    uint8_t* system = WeatherSystem();
    if (system && state) state(system, current);
    static RED4ext::CProperty* const name = Class("worldWeatherState") ? Class("worldWeatherState")->GetProperty("name")
                                                                       : nullptr;
    if (out) *out = current && name ? *name->GetValuePtr<CName>(current.instance) : CName();
}

namespace {

// ---- the lobby's exposure -----------------------------------------------------------------------------------------

// Every exposure setting of the environments loaded so far, with the compensation it came with.
struct ExposureSetting {
    Handle<RED4ext::ISerializable> setting;
    float before = 0.0f;
};
std::mutex g_exposureLock;
std::vector<ExposureSetting> g_exposure;
float g_exposureOffset = 0.0f;

RED4ext::CProperty* Property(RED4ext::CClass* type, const char* name) {
    return type ? type->GetProperty(name) : nullptr;
}

// The area parameters (array<handle:IAreaSettings>) of a struct property of `object` (renderAreaSettings of the
// area parameters, worldRenderSettings of the environment definition).
RED4ext::DynArray<Handle<RED4ext::ISerializable>>* AreaParameters(RED4ext::ISerializable* object, const char* holder) {
    auto* type = object->GetNativeType();
    auto* outer = Property(type, holder);
    if (!outer || outer->type->GetType() != RED4ext::ERTTIType::Class) return nullptr;
    auto* inner = Property(static_cast<RED4ext::CClass*>(outer->type), "areaParameters");
    if (!inner) return nullptr;
    return inner->GetValuePtr<RED4ext::DynArray<Handle<RED4ext::ISerializable>>>(outer->GetValuePtr<void>(object));
}

}  // namespace

void VRLobbyExposureCollect(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    auto resource = Param<Handle<RED4ext::ISerializable>>(frame);
    frame->code++;
    static RED4ext::CClass* const exposureType = Class("ExposureAreaSettings");
    static RED4ext::CProperty* const compensation = Property(exposureType, "exposureCompensationOffset");
    int32_t added = 0;
    if (resource && compensation) {
        auto* params = AreaParameters(resource.instance, "renderAreaSettings");
        if (!params) params = AreaParameters(resource.instance, "worldRenderSettings");
        if (params) {
            std::lock_guard<std::mutex> guard(g_exposureLock);
            for (const auto& param : *params) {
                if (!param || !param->GetNativeType()->IsA(exposureType)) continue;
                bool known = false;
                for (const auto& s : g_exposure) known |= s.setting.instance == param.instance;
                if (known) continue;
                float* value = compensation->GetValuePtr<float>(param.instance);
                g_exposure.push_back({param, *value});
                // One loaded while the lobby is up takes its offset at once.
                *value += g_exposureOffset;
                ++added;
            }
        }
    }
    if (out) *out = added;
}

// The lobby's offset, in stops, on every collected setting over what it had (0: back to what they had).
void VRLobbyExposureSet(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    auto offset = Param<float>(frame);
    frame->code++;
    static RED4ext::CProperty* const compensation = Property(Class("ExposureAreaSettings"), "exposureCompensationOffset");
    if (!compensation) return;
    std::lock_guard<std::mutex> guard(g_exposureLock);
    g_exposureOffset = offset;
    for (const auto& s : g_exposure) *compensation->GetValuePtr<float>(s.setting.instance) = s.before + offset;
}

void VRLobbyExposureGet(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, float* out, int64_t) {
    frame->code++;
    std::lock_guard<std::mutex> guard(g_exposureLock);
    if (out) *out = g_exposureOffset;
}

void VRLobbyExposureCount(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    frame->code++;
    std::lock_guard<std::mutex> guard(g_exposureLock);
    if (out) *out = static_cast<int32_t>(g_exposure.size());
}
