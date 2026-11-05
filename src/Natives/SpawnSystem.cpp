// ENTITIES THE PORT SPAWNS, WITHOUT CODEWARE (stage 4 of docs/native-frameworks-plan-20261030.md).
//
// The lobby's props, lights and figures (mods/cet/CyberpunkVRPort_Lobby) and the basketball were spawned through
// Codeware's DynamicEntitySystem / StaticEntitySystem. This is the same, the way the game spawns its own dynamic
// entities and the way Codeware's dynamic system does it: an entity stub is created for a record (a template is given
// a SpawnableObject record of its own first), and once the stub is in, the population system is asked to keep the
// entity spawned, in view, with its appearance. Nothing of it goes into a save.
//
// VRPortSpawnSystem is a game system of the plugin's own (a native class deriving gameIGameSystem, added to the game
// instance like the game's): the world's attach and detach reach it, so what it spawned is let go with the world that
// held it. Entities that are not game objects (props) are entered in the world's entity registry as they spawn, so
// FindEntityByID finds them like any other.
//
// Natives (all VRSpawn*): spawn from a template or a record with an appearance, a position, an orientation and tags;
// delete one or every one with a tag; the entity of an id once it is in; the entities and ids with a tag.

#include "Natives/NativeFunctions.hpp"

#include <RED4ext/GameEngine.hpp>
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Relocation.hpp>
#include <RED4ext/RTTITypes.hpp>
#include <RED4ext/ResourceDepot.hpp>
#include <RED4ext/TweakDB.hpp>
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/gameIDynamicEntityIDSystem.hpp>
#include <RED4ext/Scripting/Natives/gameIEntitySpawnerEventsBroadcaster.hpp>
#include <RED4ext/Scripting/Natives/gameIEntityStubSystem.hpp>
#include <RED4ext/Scripting/Natives/gameIGameSystem.hpp>
#include <RED4ext/Scripting/Natives/gameIPopulationSystem.hpp>

#include "Core/TweakRecords.hpp"
#include "Natives/RttiType.hpp"

#include <memory>
#include <mutex>
#include <unordered_map>
#include <unordered_set>
#include <vector>

extern void Log(const char* fmt, ...);

namespace {

using RED4ext::CName;
using RED4ext::EntityID;
using RED4ext::Handle;
using RED4ext::TweakDBID;

// worldRuntimeEntityRegistry is a table by entity id that keeps what it has: entering an id it knows changes nothing
// (sub_14074AD78), and it outlives the session. The game takes its game objects out as they detach; a prop entered
// here has to be taken out here, or its dead entry stays. The dynamic ids start over with every session, so a prop of a
// later session could get the id of an earlier one and FindEntityByID answered null for an entity standing in the
// world. Back from a save the lobby's menu panel was one (2026-11-02): it never built, and with it nothing could be
// done any more. An entity of the game's own with such an id would have been just as lost.
constexpr uint32_t kRegisterEntityHash = 2840271332;    // enters an entity
constexpr uint32_t kUnregisterEntityHash = 3878623943;  // takes it out (sub_14074A848)
constexpr size_t kEntityRegistrySlot = 13;              // the registry among the runtime scene's systems

struct Spawned {
    EntityID id;
    TweakDBID record;
    CName appearance;
    std::vector<CName> tags;
    RED4ext::UniquePtr<RED4ext::EntityStub> stub;
    bool deleted = false;
    // What was entered in the registry, and the registry: held until it is out again.
    Handle<RED4ext::ent::Entity> entered;
    Handle<RED4ext::IScriptable> registry;
};

struct SpawnSystem : RED4ext::game::IGameSystem {
    RED4ext::CClass* GetNativeType() override;

    void OnStreamingWorldLoaded(RED4ext::world::RuntimeScene*, uint64_t, const RED4ext::JobGroup&) override { Listen(); }
    void OnBeforeWorldDetach(RED4ext::world::RuntimeScene*) override { StopListening(); }
    void OnAfterWorldDetach() override;

    void Listen();
    void StopListening();
    void OnSpawnerEvent(RED4ext::game::EntitySpawnerEventType type, EntityID id);
    void LeaveRegistry(Spawned& spawned);

    std::mutex lock;
    std::unordered_map<uint64_t, std::shared_ptr<Spawned>> byId;
    uint32_t listener = 0;
};

RED4ext::TTypedClass<SpawnSystem> g_type("VRPortSpawnSystem");
RED4ext::CClass* SpawnSystem::GetNativeType() { return &g_type; }

Handle<RED4ext::IScriptable> g_instance;

RED4ext::GameInstance* Game() {
    auto* engine = RED4ext::CGameEngine::Get();
    return engine && engine->framework ? engine->framework->gameInstance : nullptr;
}

template<typename T>
T* System() {
    auto* game = Game();
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* type = rtti ? rtti->GetClass(T::NAME) : nullptr;
    return game && type ? reinterpret_cast<T*>(game->GetSystem(type)) : nullptr;
}

SpawnSystem* Instance() { return static_cast<SpawnSystem*>(g_instance.instance); }

void SpawnSystem::Listen() {
    if (listener) return;
    if (auto* events = System<RED4ext::game::IEntitySpawnerEventsBroadcaster>())
        listener = events->RegisterListener(
            [this](RED4ext::game::EntitySpawnerEventType type, EntityID id, EntityID, RED4ext::EntityStub*) {
                OnSpawnerEvent(type, id);
            },
            "");
}

void SpawnSystem::StopListening() {
    if (!listener) return;
    if (auto* events = System<RED4ext::game::IEntitySpawnerEventsBroadcaster>()) events->UnregisterListener(listener);
    listener = 0;
}

void SpawnSystem::LeaveRegistry(Spawned& spawned) {
    Handle<RED4ext::ent::Entity> entity;
    Handle<RED4ext::IScriptable> registry;
    {
        std::lock_guard<std::mutex> guard(lock);
        entity.Swap(spawned.entered);
        registry.Swap(spawned.registry);
    }
    if (!entity || !registry) return;
    using UnregisterFn = void (*)(void*, RED4ext::ent::Entity*);
    static const auto unregisterEntity =
        reinterpret_cast<UnregisterFn>(RED4ext::UniversalRelocBase::Resolve(kUnregisterEntityHash));
    if (unregisterEntity) unregisterEntity(registry.instance, entity.instance);
}

// The world went: its stubs and entities with it, and what the registry was told of them.
void SpawnSystem::OnAfterWorldDetach() {
    std::unordered_map<uint64_t, std::shared_ptr<Spawned>> gone;
    {
        std::lock_guard<std::mutex> guard(lock);
        gone.swap(byId);
    }
    for (auto& [id, spawned] : gone) LeaveRegistry(*spawned);
}

void SpawnSystem::OnSpawnerEvent(RED4ext::game::EntitySpawnerEventType type, EntityID id) {
    std::shared_ptr<Spawned> spawned;
    {
        std::lock_guard<std::mutex> guard(lock);
        const auto it = byId.find(id.hash);
        if (it == byId.end()) return;
        spawned = it->second;
    }
    if (type == RED4ext::game::EntitySpawnerEventType::Spawn) {
        auto* population = System<RED4ext::game::IPopulationSystem>();
        Handle<RED4ext::ent::Entity> entity = population ? population->FindEntity(static_cast<uint32_t>(id.hash))
                                                          : Handle<RED4ext::ent::Entity>();
        if (!entity) {
            Log("[spawn] %016llX spawned, but the population system has no entity for it\n", id.hash);
            return;
        }
        Log("[spawn] %016llX in the world\n", id.hash);
        static RED4ext::CClass* const gameObject = RED4ext::CRTTISystem::Get()->GetClass("gameObject");
        // A prop is no game object and the registry would not know it: entered here, FindEntityByID finds it.
        if (!entity->GetType()->IsA(gameObject)) {
            using RegisterFn = void (*)(void*, RED4ext::ent::Entity*);
            static const auto registerEntity =
                reinterpret_cast<RegisterFn>(RED4ext::UniversalRelocBase::Resolve(kRegisterEntityHash));
            auto* engine = RED4ext::CGameEngine::Get();
            const uintptr_t scene = engine && engine->framework ? engine->framework->unk18 : 0;
            Handle<RED4ext::IScriptable> registry;
            if (scene) registry = reinterpret_cast<Handle<RED4ext::IScriptable>*>(scene)[kEntityRegistrySlot];
            if (registry && registerEntity) {
                registerEntity(registry.instance, entity.instance);
                std::lock_guard<std::mutex> guard(lock);
                spawned->entered = entity;
                spawned->registry = registry;
            } else {
                Log("[spawn] %016llX: no runtime entity registry to enter it in\n", id.hash);
            }
        }
    } else if (type == RED4ext::game::EntitySpawnerEventType::Despawn) {
        LeaveRegistry(*spawned);
        if (spawned->deleted) {
            std::lock_guard<std::mutex> guard(lock);
            byId.erase(id.hash);
        }
    }
}

// A template's own SpawnableObject record, named by its path (the name Codeware gives it too, so either makes the
// same record).
TweakDBID TemplateRecord(const RED4ext::CString& path) {
    const RED4ext::ResourcePath resource(path.c_str());
    const uint32_t hash = RED4ext::FNV1a32(reinterpret_cast<const uint8_t*>(&resource), sizeof(resource));
    const TweakDBID record(hash, 1);
    auto* db = RED4ext::TweakDB::Get();
    if (!db) return {};
    {
        std::shared_lock<RED4ext::SharedSpinLock> guard(db->mutex01);
        if (db->recordsByID.Get(record)) return record;
    }
    auto* refType = RttiType("raRef:CResource");
    auto* recordType = RED4ext::CRTTISystem::Get()->GetClass("gamedataSpawnableObject_Record");
    if (!refType || !recordType) {
        Log("[spawn] no %s type: %s gets no record\n", refType ? "SpawnableObject record" : "raRef:CResource",
            path.c_str());
        return {};
    }
    {
        cvr::tweak_records::FlatWrites writes(db);
        RED4ext::ResourcePath value = resource;
        writes.Set(TweakDBID(record, ".entityTemplatePath"), refType, &value);
    }
    if (db->CreateRecord(record, recordType)) {
        Log("[spawn] record %08X:%02X made for %s\n", record.name.hash, record.name.length, path.c_str());
        return record;
    }
    Log("[spawn] the TweakDB made no record for %s\n", path.c_str());
    return {};
}

bool RecordExists(TweakDBID record) {
    auto* db = RED4ext::TweakDB::Get();
    if (!db) return false;
    std::shared_lock<RED4ext::SharedSpinLock> guard(db->mutex01);
    return db->recordsByID.Get(record) != nullptr;
}

void Delete(SpawnSystem& system, const std::shared_ptr<Spawned>& spawned) {
    RED4ext::UniquePtr<RED4ext::EntityStub> stub;
    {
        std::lock_guard<std::mutex> guard(system.lock);
        if (spawned->deleted) return;
        spawned->deleted = true;
        stub = std::move(spawned->stub);
        // Never spawned (or not yet): nothing will report its despawn.
        if (!stub) system.byId.erase(spawned->id.hash);
    }
    if (stub) {
        if (auto* population = System<RED4ext::game::IPopulationSystem>())
            population->RemoveEntity(static_cast<uint32_t>(spawned->id.hash), 0);
        if (auto* stubs = System<RED4ext::game::IEntityStubSystem>()) stubs->DeleteStub(std::move(stub));
    }
}

std::vector<std::shared_ptr<Spawned>> Tagged(SpawnSystem& system, CName tag) {
    std::vector<std::shared_ptr<Spawned>> out;
    std::lock_guard<std::mutex> guard(system.lock);
    for (const auto& [_, spawned] : system.byId)
        if (!spawned->deleted)
            for (CName t : spawned->tags)
                if (t == tag) {
                    out.push_back(spawned);
                    break;
                }
    return out;
}

Handle<RED4ext::ent::Entity> EntityOf(EntityID id) {
    auto* population = System<RED4ext::game::IPopulationSystem>();
    return population ? population->FindEntity(static_cast<uint32_t>(id.hash)) : Handle<RED4ext::ent::Entity>();
}

template<typename T>
T Param(RED4ext::CStackFrame* frame) {
    T value{};
    RED4ext::GetParameter(frame, &value);
    return value;
}

}  // namespace

namespace cvr::spawn {

void RegisterTypes() {
    RED4ext::CNamePool::Add("VRPortSpawnSystem");
    g_type.flags = {.isNative = true};
    RED4ext::CRTTISystem::Get()->RegisterType(&g_type);
}

void PostRegisterTypes() { g_type.parent = RED4ext::CRTTISystem::Get()->GetClass("gameIGameSystem"); }

// Into the game instance's systems, like the game's own (the world's attach and detach then reach it).
bool AddToGame(RED4ext::CGameApplication*) {
    auto* game = Game();
    if (!game || g_instance) return true;
    if (game->systemMap.Get(&g_type)) return true;
    g_instance = Handle<RED4ext::IScriptable>(static_cast<RED4ext::IScriptable*>(g_type.CreateInstance(true)));
    Instance()->gameInstance = game;
    game->systemMap.Insert(&g_type, g_instance);
    game->systemImplementations.Insert(&g_type, &g_type);
    game->systemInstances.EmplaceBack(g_instance);
    Instance()->Listen();
    Log("[spawn] VRPortSpawnSystem added to the game's systems\n");
    return true;
}

}  // namespace cvr::spawn

namespace {

// One entity of a SpawnableObject (or Character ...) record; its id, or none.
EntityID Spawn(TweakDBID record, CName appearance, const RED4ext::Vector4& position,
               const RED4ext::Quaternion& orientation, const RED4ext::DynArray<CName>& tags) {
    SpawnSystem* system = Instance();
    auto* ids = System<RED4ext::game::IDynamicEntityIDSystem>();
    auto* stubs = System<RED4ext::game::IEntityStubSystem>();
    if (!system || !ids || !stubs) {
        Log("[spawn] no spawn: the game's %s is not there\n",
            !system ? "VRPortSpawnSystem" : !ids ? "IDynamicEntityIDSystem" : "IEntityStubSystem");
        return {};
    }
    if (!record.value || !RecordExists(record)) {
        Log("[spawn] no spawn: record %08X:%02X is not in the TweakDB\n", record.name.hash, record.name.length);
        return {};
    }
    system->Listen();

    auto spawned = std::make_shared<Spawned>();
    spawned->id = ids->GetNextTransientID();
    if (!spawned->id.hash) return {};
    spawned->record = record;
    spawned->appearance = appearance;
    for (CName tag : tags) spawned->tags.push_back(tag);
    {
        std::lock_guard<std::mutex> guard(system->lock);
        system->byId[spawned->id.hash] = spawned;
    }
    RED4ext::EntityStubCreateRequest request(spawned->id, position, orientation, record);
    stubs->CreateStub(request, [system, spawned](RED4ext::EntityStubTokenPtr& token) {
        bool keep = token && token->status == RED4ext::EntityStubStatus::Created;
        if (keep) {
            std::lock_guard<std::mutex> guard(system->lock);
            // Deleted while its stub was made: the stub goes back at once.
            keep = !spawned->deleted;
            if (keep) spawned->stub = token->ExtractStub();
        }
        if (!keep) {
            if (!token || token->status != RED4ext::EntityStubStatus::Created)
                Log("[spawn] the stub system made no stub (status %d)\n", token ? static_cast<int>(token->status) : -1);
            if (token && token->status == RED4ext::EntityStubStatus::Created)
                if (auto* s = System<RED4ext::game::IEntityStubSystem>()) s->DeleteStub(token->ExtractStub());
            std::lock_guard<std::mutex> guard(system->lock);
            system->byId.erase(spawned->id.hash);
            return;
        }
        RED4ext::PopulationEntityRegisterRequest population{};
        population.entityID = spawned->id;
        population.alwaysSpawned = true;   // a place a kilometre wide: nothing of it streams out with distance
        population.spawnInView = true;
        if (spawned->appearance) {
            population.appearanceName = spawned->appearance;
            population.prefetchAppearance = true;
        }
        auto* populationSystem = System<RED4ext::game::IPopulationSystem>();
        if (populationSystem) populationSystem->RegisterEntity(population);
        Log("[spawn] %016llX: stub made (record %08X:%02X), %s\n", spawned->id.hash, spawned->record.name.hash,
            spawned->record.name.length, populationSystem ? "handed to the population system" : "NO population system");
    });
    return spawned->id;
}

}  // namespace

// An entity of a template (an .ent path), through a SpawnableObject record of its own.
void VRSpawnTemplate(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, EntityID* out, int64_t) {
    auto path = Param<RED4ext::CString>(frame);
    auto appearance = Param<CName>(frame);
    auto position = Param<RED4ext::Vector4>(frame);
    auto orientation = Param<RED4ext::Quaternion>(frame);
    auto tags = Param<RED4ext::DynArray<CName>>(frame);
    frame->code++;
    EntityID id{};
    if (!RED4ext::ResourceDepot::Get()->ResourceExists(RED4ext::ResourcePath(path.c_str())))
        Log("[spawn] %s is in no archive\n", path.c_str());
    else
        id = Spawn(TemplateRecord(path), appearance, position, orientation, tags);
    if (out) *out = id;
}

// An entity of a record (a Character's NPC puppet ...).
void VRSpawnRecord(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, EntityID* out, int64_t) {
    auto record = Param<TweakDBID>(frame);
    auto appearance = Param<CName>(frame);
    auto position = Param<RED4ext::Vector4>(frame);
    auto orientation = Param<RED4ext::Quaternion>(frame);
    auto tags = Param<RED4ext::DynArray<CName>>(frame);
    frame->code++;
    const EntityID id = Spawn(record, appearance, position, orientation, tags);
    if (out) *out = id;
}

void VRSpawnDelete(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto id = Param<EntityID>(frame);
    frame->code++;
    bool done = false;
    if (SpawnSystem* system = Instance()) {
        std::shared_ptr<Spawned> spawned;
        {
            std::lock_guard<std::mutex> guard(system->lock);
            const auto it = system->byId.find(id.hash);
            if (it != system->byId.end()) spawned = it->second;
        }
        if (spawned && !spawned->deleted) {
            Delete(*system, spawned);
            done = true;
        }
    }
    if (out) *out = done;
}

void VRSpawnDeleteTagged(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    auto tag = Param<CName>(frame);
    frame->code++;
    int32_t deleted = 0;
    if (SpawnSystem* system = Instance())
        for (const auto& spawned : Tagged(*system, tag)) Delete(*system, spawned), ++deleted;
    if (out) *out = deleted;
}

void VRSpawnManaged(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    auto id = Param<EntityID>(frame);
    frame->code++;
    bool known = false;
    if (SpawnSystem* system = Instance()) {
        std::lock_guard<std::mutex> guard(system->lock);
        const auto it = system->byId.find(id.hash);
        known = it != system->byId.end() && !it->second->deleted;
    }
    if (out) *out = known;
}

void VRSpawnedEntity(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, Handle<RED4ext::ent::Entity>* out, int64_t) {
    auto id = Param<EntityID>(frame);
    frame->code++;
    if (out) *out = Instance() ? EntityOf(id) : Handle<RED4ext::ent::Entity>();
}

void VRSpawnTagged(RED4ext::IScriptable*, RED4ext::CStackFrame* frame,
                   RED4ext::DynArray<Handle<RED4ext::ent::Entity>>* out, int64_t) {
    auto tag = Param<CName>(frame);
    frame->code++;
    if (!out) return;
    if (SpawnSystem* system = Instance())
        for (const auto& spawned : Tagged(*system, tag))
            if (auto entity = EntityOf(spawned->id)) out->PushBack(entity);
}

void VRSpawnTaggedIDs(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::DynArray<EntityID>* out, int64_t) {
    auto tag = Param<CName>(frame);
    frame->code++;
    if (!out) return;
    if (SpawnSystem* system = Instance())
        for (const auto& spawned : Tagged(*system, tag)) out->PushBack(spawned->id);
}
