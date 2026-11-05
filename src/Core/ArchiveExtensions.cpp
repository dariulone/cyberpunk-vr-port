#include "Core/ArchiveExtensions.hpp"
#include "Core/Paths.hpp"

#include <RED4ext/JobQueue.hpp>
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Relocation.hpp>
#include <RED4ext/ResourceDepot.hpp>
#include <RED4ext/ResourceLoader.hpp>
#include <RED4ext/ResourceReference.hpp>
#include <RED4ext/RTTISystem.hpp>
#include <RED4ext/TweakDB.hpp>
#include <RED4ext/Scripting/Natives/CMesh.hpp>
#include <RED4ext/Scripting/Natives/CMeshMaterialEntry.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimSetupEntry.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/mesh/MeshAppearance.hpp>
#include <RED4ext/Scripting/Natives/Generated/world/StreamingBlock.hpp>
#include <RED4ext/Scripting/Natives/Generated/world/StreamingWorld.hpp>

#include <windows.h>

#include <yaml-cpp/yaml.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <memory>
#include <mutex>
#include <shared_mutex>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

extern void Log(const char* fmt, ...);

namespace cvr::archive {
namespace {

namespace fs = std::filesystem;
using RED4ext::ResourcePath;

// Address-library hashes (red4ext's cyberpunk2077_addresses.json), the ones ArchiveXL resolves for the same purposes.
constexpr uint32_t kInitializeArchivesHash = 2892763485;    // ResourceDepot: mounts its archive groups
constexpr uint32_t kLoadArchivesHash = 3573367139;          // ResourceDepot: mounts archives into a group (2.5 form)
constexpr uint32_t kLoadFactoryAsyncHash = 1931943018;      // FactoryIndex: loads one factory CSV
constexpr uint32_t kInitializeAnimationsHash = 2855474741;  // entAnimatedComponent: builds its animation setup
constexpr uint32_t kMeshPostLoadHash = 2272530537;          // CMesh::PostLoad
constexpr uint32_t kDependenciesReadyHash = 3892979844;     // ResourceSerializer: a resource's dependencies are in
constexpr uint32_t kSerializerLoadHash = 2577814646;        // ResourceSerializer: a resource is requested
constexpr uint32_t kDeserializeHash = 2901686778;           // ResourceSerializer: a resource's deserialization job

// The game's own last factory: the port's are loaded right after it, into the same index.
constexpr ResourcePath kLastGameFactory(R"(base\gameplay\factories\vehicles\vehicles.csv)");

// ResourceSerializer's context for one resource, as far as it is read here (layout of 2.5, as ArchiveXL has it).
struct SerializerContext {
    uint8_t unk00[0x230];
    RED4ext::DynArray<RED4ext::Handle<RED4ext::ISerializable>> serializables;  // 230
};

using InitializeArchivesFn = void (*)(RED4ext::ResourceDepot*);
using LoadArchivesFn = bool (*)(RED4ext::ResourceDepot*, RED4ext::ArchiveGroup&, const RED4ext::DynArray<RED4ext::CString>&,
                                RED4ext::DynArray<ResourcePath>&, int32_t, uint8_t);
using LoadFactoryAsyncFn = void (*)(uintptr_t index, ResourcePath path, uintptr_t context);
using InitializeAnimationsFn = void (*)(RED4ext::ent::AnimatedComponent*);
using MeshPostLoadFn = void (*)(RED4ext::CMesh*, void*);
using DependenciesReadyFn = void (*)(SerializerContext*);
// The request carries the resource's path at +0x28 (2.5, as ArchiveXL has it).
using SerializerLoadFn = void (*)(void* serializer, uint64_t a2, uint8_t* request, uint64_t a4, uint64_t a5);
using DeserializeFn = void (*)(void* serializer, uint64_t a2, uint64_t a3, RED4ext::JobHandle& job, uint8_t* request,
                               uint64_t a6, void* results, uint64_t a8);

InitializeArchivesFn g_initializeArchives = nullptr;
LoadArchivesFn g_loadArchives = nullptr;
LoadFactoryAsyncFn g_loadFactoryAsync = nullptr;
InitializeAnimationsFn g_initializeAnimations = nullptr;
MeshPostLoadFn g_meshPostLoad = nullptr;
DependenciesReadyFn g_dependenciesReady = nullptr;
SerializerLoadFn g_serializerLoad = nullptr;
DeserializeFn g_deserialize = nullptr;

// ---- the manifest ------------------------------------------------------------------------------------------------

struct AnimationEntry {
    std::vector<std::string> entities;
    std::string component, set;
    std::vector<std::string> vars;
    int priority = 0;
};

struct PatchEntry {
    std::string source;
    std::vector<std::string> targets;
    std::vector<std::string> props;
};

struct Manifest {
    std::vector<std::string> factories;
    std::vector<AnimationEntry> animations;
    std::vector<PatchEntry> patches;
};
Manifest g_manifest;

std::vector<std::string> List(const YAML::Node& node) {
    std::vector<std::string> out;
    if (node && node.IsScalar()) out.push_back(node.as<std::string>());
    else if (node && node.IsSequence())
        for (const auto& item : node) out.push_back(item.as<std::string>());
    return out;
}

// Not beside the archive: ArchiveXL applies every *.xl under a mod group's folder, the plugin's archive folder too.
fs::path ManifestPath() { return fs::path(paths::Dir(paths::Folder::Config)) / L"archive.yaml"; }

bool ReadManifest() {
    try {
        std::ifstream stream(ManifestPath(), std::ios::binary);
        if (!stream) return false;
        const YAML::Node root = YAML::Load(stream);
        g_manifest.factories = List(root["factories"]);
        for (const auto& item : root["animations"]) {
            AnimationEntry entry;
            entry.entities = List(item["entity"]);
            entry.component = item["component"] ? item["component"].as<std::string>() : std::string();
            entry.set = item["set"].as<std::string>();
            entry.vars = List(item["vars"]);
            entry.priority = item["priority"] ? item["priority"].as<int>() : 128;
            g_manifest.animations.push_back(std::move(entry));
        }
        for (const auto& item : root["resource"]["patch"]) {
            PatchEntry entry;
            entry.source = item.first.as<std::string>();
            if (item.second.IsMap()) {
                entry.targets = List(item.second["targets"]);
                entry.props = List(item.second["props"]);
            } else {
                entry.targets = List(item.second);
            }
            g_manifest.patches.push_back(std::move(entry));
        }
    } catch (const std::exception& e) {
        Log("[archive] %ls: %s\n", ManifestPath().c_str(), e.what());
        return false;
    }
    return true;
}

// ---- what the manifest becomes once the archives are mounted -----------------------------------------------------

std::vector<ResourcePath> g_factories;
// Animation setups by the entity template path, or by FNV1a64(component name, template path) for one component.
std::unordered_map<uint64_t, std::vector<RED4ext::anim::AnimSetupEntry>> g_animations;

// A patch resource, loaded when one of its targets is first requested and held for the session: a few small
// resources, needed again by every load of their targets.
struct Patch {
    ResourcePath source;
    std::string name;
    bool appearances = false;  // a mesh patch: its appearances are added to the target
    std::mutex lock;
    RED4ext::SharedPtr<RED4ext::ResourceToken<RED4ext::CResource>> token;
};
std::vector<std::shared_ptr<Patch>> g_patches;
std::unordered_map<uint64_t, std::vector<std::shared_ptr<Patch>>> g_patchesByTarget;

void StartLoading(Patch& patch) {
    std::lock_guard<std::mutex> guard(patch.lock);
    if (!patch.token) patch.token = RED4ext::ResourceLoader::Get()->LoadAsync<RED4ext::CResource>(patch.source);
}

// A patch resource ready to merge: its load is part of its target's deserialization job (Detour_Deserialize), so the
// target is never done before it.
RED4ext::CResource* Ready(Patch& patch) {
    StartLoading(patch);
    if (!patch.token->IsLoaded()) {
        Log("[archive] patch %s is %s\n", patch.name.c_str(), patch.token->IsFailed() ? "missing" : "STILL LOADING");
        return nullptr;
    }
    return patch.token->resource.GetPtr();
}

const std::vector<std::shared_ptr<Patch>>* PatchesOf(const uint8_t* request) {
    if (!request || g_patchesByTarget.empty()) return nullptr;
    const auto it = g_patchesByTarget.find(*reinterpret_cast<const uint64_t*>(request + 0x28));
    return it == g_patchesByTarget.end() ? nullptr : &it->second;
}

void Prepare(RED4ext::ResourceDepot* depot) {
    int missing = 0;
    for (const std::string& factory : g_manifest.factories) {
        const ResourcePath path(factory.c_str());
        if (depot->ResourceExists(path)) g_factories.push_back(path);
        else Log("[archive] factory %s is in no archive\n", factory.c_str()), ++missing;
    }
    size_t setups = 0;
    for (const AnimationEntry& entry : g_manifest.animations) {
        const ResourcePath set(entry.set.c_str());
        if (!depot->ResourceExists(set)) {
            Log("[archive] animation set %s is in no archive\n", entry.set.c_str());
            ++missing;
            continue;
        }
        for (const std::string& entity : entry.entities) {
            const ResourcePath target(entity.c_str());
            if (!depot->ResourceExists(target)) continue;
            RED4ext::anim::AnimSetupEntry setup;
            setup.animSet.path = set;
            setup.priority = static_cast<uint8_t>(entry.priority);
            for (const std::string& var : entry.vars) setup.variableNames.PushBack(RED4ext::CNamePool::Add(var.c_str()));
            const uint64_t key = entry.component.empty() ? target.hash : RED4ext::FNV1a64(entry.component.c_str(), target.hash);
            g_animations[key].push_back(std::move(setup));
            ++setups;
        }
    }
    for (const PatchEntry& entry : g_manifest.patches) {
        const ResourcePath source(entry.source.c_str());
        if (!depot->ResourceExists(source)) {
            Log("[archive] patch %s is in no archive\n", entry.source.c_str());
            ++missing;
            continue;
        }
        auto patch = std::make_shared<Patch>();
        patch->source = source;
        patch->name = entry.source;
        patch->appearances = std::find(entry.props.begin(), entry.props.end(), "appearances") != entry.props.end();
        for (const std::string& target : entry.targets) g_patchesByTarget[ResourcePath(target.c_str()).hash].push_back(patch);
        g_patches.push_back(std::move(patch));
    }
    Log("[archive] manifest: %zu factories, %zu animation setups on %zu targets, %zu patches, %d missing\n",
        g_factories.size(), setups, g_animations.size(), g_patches.size(), missing);
}

// ---- mounting ----------------------------------------------------------------------------------------------------

// The port's archives in a mod group of their own, placed with the game's mod groups (before the first other one).
void Mount(RED4ext::ResourceDepot* depot) {
    const fs::path dir = paths::Dir(paths::Folder::Archive);
    RED4ext::DynArray<RED4ext::CString> archives;
    std::error_code error;
    for (const auto& entry : fs::directory_iterator(dir, error))
        if (entry.is_regular_file() && entry.path().extension() == L".archive") archives.PushBack(entry.path().string().c_str());
    if (archives.size() == 0) {
        Log("[archive] no archive in %ls\n", dir.c_str());
        return;
    }
    const std::string base = dir.string();
    auto& groups = depot->groups;
    auto it = std::find_if(groups.begin(), groups.end(), [&](const RED4ext::ArchiveGroup& g) {
        return std::strcmp(g.basePath.c_str(), base.c_str()) == 0;
    });
    if (it == groups.end()) {
        const auto firstOther = std::find_if(groups.begin(), groups.end(), [](const RED4ext::ArchiveGroup& g) {
            return g.scope != RED4ext::ArchiveScope::Mod;
        });
        const auto index = static_cast<uint32_t>(firstOther - groups.begin());
        groups.Emplace(firstOther);
        groups[index].basePath = base.c_str();
        groups[index].scope = RED4ext::ArchiveScope::Mod;
        it = groups.begin() + index;
    }
    RED4ext::DynArray<ResourcePath> loaded;
    const bool failed = g_loadArchives(nullptr, *it, archives, loaded, 0, 0);
    Log("[archive] mounted %u archive(s) from %ls%s\n", archives.size(), dir.c_str(), failed ? " -- ONE FAILED" : "");
}

void Detour_InitializeArchives(RED4ext::ResourceDepot* depot) {
    g_initializeArchives(depot);
    LARGE_INTEGER started{}, done{}, frequency{};
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&started);
    Mount(depot);
    Prepare(depot);
    QueryPerformanceCounter(&done);
    Log("[archive] mount and manifest: %.1f ms\n", double(done.QuadPart - started.QuadPart) * 1000.0 / double(frequency.QuadPart));
}

// ---- factories, animations, patches ------------------------------------------------------------------------------

// A patched resource is requested: its patches start loading alongside it.
void Detour_SerializerLoad(void* serializer, uint64_t a2, uint8_t* request, uint64_t a4, uint64_t a5) {
    if (const auto* patches = PatchesOf(request))
        for (const auto& patch : *patches) StartLoading(*patch);
    g_serializerLoad(serializer, a2, request, a4, a5);
}

// ... and its deserialization waits for them: their load jobs are joined to its job.
void Detour_Deserialize(void* serializer, uint64_t a2, uint64_t a3, RED4ext::JobHandle& job, uint8_t* request, uint64_t a6,
                        void* results, uint64_t a8) {
    if (const auto* patches = PatchesOf(request))
        for (const auto& patch : *patches) {
            StartLoading(*patch);
            job.Join(patch->token->job);
        }
    g_deserialize(serializer, a2, a3, job, request, a6, results, a8);
}

void Detour_LoadFactoryAsync(uintptr_t index, ResourcePath path, uintptr_t context) {
    g_loadFactoryAsync(index, path, context);
    if (path != kLastGameFactory) return;
    for (const ResourcePath& factory : g_factories) g_loadFactoryAsync(index, factory, context);
    Log("[archive] %zu item factories loaded after the game's\n", g_factories.size());
}

void Detour_InitializeAnimations(RED4ext::ent::AnimatedComponent* component) {
    auto* owner = reinterpret_cast<RED4ext::ent::IComponent*>(component)->owner;
    if (owner && !g_animations.empty()) {
        const uint64_t templatePath = owner->templatePath.hash;
        auto it = g_animations.find(templatePath);
        if (it == g_animations.end())
            it = g_animations.find(RED4ext::FNV1a64(reinterpret_cast<RED4ext::ent::IComponent*>(component)->name.ToString(),
                                                    templatePath));
        if (it != g_animations.end())
            for (const auto& setup : it->second) component->animations.gameplay.PushBack(setup);
    }
    g_initializeAnimations(component);
}

// The patch mesh's appearances added to the target, with the external materials they use. A same-named appearance of
// the target is replaced. Local material instances are not carried over (none of the port's patches has one).
void MergeAppearances(RED4ext::CMesh* target, RED4ext::CMesh* patch, const Patch& source) {
    auto* appearanceType = RED4ext::CRTTISystem::Get()->GetClass("meshMeshAppearance");
    for (const RED4ext::CMeshMaterialEntry& entry : patch->materialEntries) {
        const bool known = std::any_of(target->materialEntries.begin(), target->materialEntries.end(),
                                       [&](const RED4ext::CMeshMaterialEntry& e) { return e.name == entry.name; });
        if (known) continue;
        if (entry.isLocalInstance || entry.index >= patch->externalMaterials.size()) {
            Log("[archive] %s: material %s is local to the patch -- not carried over\n", source.name.c_str(),
                entry.name.ToString());
            continue;
        }
        RED4ext::CMeshMaterialEntry added{};
        added.name = entry.name;
        added.index = static_cast<uint16_t>(target->externalMaterials.size());
        added.isLocalInstance = false;
        target->externalMaterials.PushBack(patch->externalMaterials[entry.index]);
        target->materialEntries.PushBack(added);
    }
    for (const auto& appearance : patch->appearances) {
        if (!appearance) continue;
        auto* clone = static_cast<RED4ext::mesh::MeshAppearance*>(appearanceType->CreateInstance(true));
        clone->name = appearance->name;
        clone->chunkMaterials = appearance->chunkMaterials;
        clone->tags = appearance->tags;
        RED4ext::Handle<RED4ext::mesh::MeshAppearance> handle(clone);
        auto same = std::find_if(target->appearances.begin(), target->appearances.end(),
                                 [&](const auto& a) { return a && a->name == appearance->name; });
        if (same != target->appearances.end()) *same = handle;
        else target->appearances.PushBack(handle);
    }
    target->forceLoadAllAppearances = false;
}

void Detour_MeshPostLoad(RED4ext::CMesh* mesh, void* params) {
    if (mesh && !g_patchesByTarget.empty()) {
        const auto it = g_patchesByTarget.find(mesh->path.hash);
        if (it != g_patchesByTarget.end())
            for (const auto& patch : it->second) {
                if (!patch->appearances) continue;
                RED4ext::CResource* resource = Ready(*patch);
                if (resource && std::strcmp(resource->GetNativeType()->GetName().ToString(), "CMesh") == 0) {
                    MergeAppearances(mesh, static_cast<RED4ext::CMesh*>(resource), *patch);
                    Log("[archive] appearances of %s added to a mesh it patches (%u appearances now)\n",
                        patch->name.c_str(), mesh->appearances.size());
                }
            }
    }
    g_meshPostLoad(mesh, params);
}

// The features' fixes (AddFixup), by type; set at load, only read after.
std::vector<std::pair<RED4ext::CName, Fixup>>& Fixups() {
    static std::vector<std::pair<RED4ext::CName, Fixup>> fixups;
    return fixups;
}

// The features' fixes, each on the resources of its type; and a streaming block patched into a streaming world: one
// more block reference, already holding its loaded block.
void Detour_DependenciesReady(SerializerContext* context) {
    if (context && !Fixups().empty()) {
        for (const auto& serializable : context->serializables) {
            if (!serializable) continue;
            const auto type = serializable->GetNativeType()->GetName();
            for (const auto& [name, fix] : Fixups())
                if (name == type) fix(serializable.GetPtr());
        }
    }
    if (context && !g_patchesByTarget.empty()) {
        for (const auto& serializable : context->serializables) {
            if (!serializable ||
                std::strcmp(serializable->GetNativeType()->GetName().ToString(), "worldStreamingWorld") != 0)
                continue;
            auto* world = static_cast<RED4ext::world::StreamingWorld*>(serializable.GetPtr());
            const auto it = g_patchesByTarget.find(world->path.hash);
            if (it == g_patchesByTarget.end()) continue;
            for (const auto& patch : it->second) {
                if (!Ready(*patch)) continue;
                world->blockRefs.EmplaceBack();
                auto& ref = world->blockRefs[world->blockRefs.size() - 1];
                ref.path = patch->source;
                ref.token = *reinterpret_cast<RED4ext::SharedPtr<RED4ext::ResourceToken<RED4ext::world::StreamingBlock>>*>(
                    &patch->token);
                Log("[archive] streaming block %s merged into its world\n", patch->name.c_str());
            }
        }
    }
    g_dependenciesReady(context);
}

// ---- what ArchiveXL changes for every mod -----------------------------------------------------------------------
// Two engine behaviours ArchiveXL changes whatever the manifests say; the port's assets were made with both in place.
// Either is the same change made twice when ArchiveXL is installed too.

constexpr uint32_t kUpdateWorldLayerHash = 2254060789;  // inkWorldLayer: updates its world widget components
constexpr uint32_t kSpawnFromTemplateHash = 2509382878; // EntitySpawner: spawns an entity from a template

// An ink world layer updates its first ten world widget components only (cmp ebx, 10; jae out). The port's menu, its
// glass, beam and icons, the world markers and the weapons' own screens are more than ten with the game's. The
// branch goes.
void LiftWorldWidgetLimit() {
    auto* code = reinterpret_cast<uint8_t*>(RED4ext::UniversalRelocBase::Resolve(kUpdateWorldLayerHash));
    if (!code) {
        Log("[archive] world widget limit: the layer update is not in the address library\n");
        return;
    }
    constexpr uint8_t kLimit[] = {0x83, 0xFB, 0x0A, 0x73};  // cmp ebx, 10; jae rel8
    constexpr uint8_t kLifted[] = {0x83, 0xFB, 0x0A, 0x90, 0x90};
    uint8_t* const end = code + 0x100;
    if (std::search(code, end, std::begin(kLifted), std::end(kLifted)) != end) {
        Log("[archive] world widget limit: already lifted (ArchiveXL)\n");
        return;
    }
    uint8_t* const at = std::search(code, end, std::begin(kLimit), std::end(kLimit));
    if (at == end) {
        Log("[archive] world widget limit: the compare is not where 2.5 has it -- NOT lifted\n");
        return;
    }
    DWORD protection = 0;
    VirtualProtect(at + 3, 2, PAGE_EXECUTE_READWRITE, &protection);
    at[3] = 0x90;
    at[4] = 0x90;
    VirtualProtect(at + 3, 2, protection, &protection);
    FlushInstructionCache(GetCurrentProcess(), at + 3, 2);
    Log("[archive] world widget limit lifted\n");
}

// An entity spawned from a record (DynamicEntitySpec.recordID: the lobby's figures) with no appearance, or "default",
// takes the record's appearanceName -- not the template's first appearance.
using SpawnFromTemplateFn = void* (*)(void* spawner, void* out, uint8_t* request, ResourcePath templatePath);
SpawnFromTemplateFn g_spawnFromTemplate = nullptr;

void* Detour_SpawnFromTemplate(void* spawner, void* out, uint8_t* request, ResourcePath templatePath) {
    static const RED4ext::CName kDefault("default");
    auto& appearance = *reinterpret_cast<RED4ext::CName*>(request + 0xC0);     // 2.5, as ArchiveXL has it
    const auto record = *reinterpret_cast<const RED4ext::TweakDBID*>(request + 0xE0);
    if (record.value && (!appearance || appearance == kDefault)) {
        static RED4ext::CBaseRTTIType* const nameType = RED4ext::CRTTISystem::Get()->GetType("CName");
        if (auto* db = RED4ext::TweakDB::Get()) {
            std::shared_lock<RED4ext::SharedSpinLock> guard(db->mutex00);
            if (auto* flat = db->GetFlatValue(RED4ext::TweakDBID(record, ".appearanceName"))) {
                const RED4ext::CStackType value = flat->GetValue();
                if (value.type == nameType && *static_cast<RED4ext::CName*>(value.value))
                    appearance = *static_cast<RED4ext::CName*>(value.value);
            }
        }
    }
    return g_spawnFromTemplate(spawner, out, request, templatePath);
}

// ---- install -----------------------------------------------------------------------------------------------------

// An earlier release installed the archive and its manifest in archive\pc\mod, where the game would mount them over
// the plugin's and ArchiveXL apply the manifest a second time: the copies that are the port's are taken out (same
// bytes: deleted; different: renamed .legacy, which neither reads).
void RetireLegacyCopies() {
    std::vector<wchar_t> exe(32768);
    const DWORD length = GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size()));
    if (!length || length >= exe.size()) return;
    const fs::path legacy =
        fs::path(std::wstring(exe.data(), length)).parent_path().parent_path().parent_path() / L"archive" / L"pc" / L"mod";
    const std::pair<const wchar_t*, fs::path> copies[] = {
        {L"cyberpunkvrport.archive", fs::path(paths::Dir(paths::Folder::Archive)) / L"cyberpunkvrport.archive"},
        {L"cyberpunkvrport.archive.xl", ManifestPath()},
    };
    std::error_code error;
    for (const auto& [name, now] : copies) {
        const fs::path old = legacy / name;
        if (!fs::exists(old, error) || !fs::exists(now, error)) continue;
        bool same = fs::file_size(old, error) == fs::file_size(now, error);
        if (same) {
            std::ifstream a(old, std::ios::binary), b(now, std::ios::binary);
            std::vector<char> x(1 << 20), y(1 << 20);
            while (same && a && b) {
                a.read(x.data(), x.size());
                b.read(y.data(), y.size());
                same = a.gcount() == b.gcount() && std::equal(x.begin(), x.begin() + a.gcount(), y.begin());
            }
        }
        if (same) {
            fs::remove(old, error);
            Log("[archive] removed the earlier release's copy %ls%s\n", old.c_str(), error ? " (FAILED)" : "");
        } else {
            fs::path aside = old;
            aside += L".legacy";
            fs::rename(old, aside, error);
            Log("[archive] set the earlier release's copy aside: %ls%s\n", aside.c_str(), error ? " (FAILED)" : "");
        }
    }
}

template<typename Fn>
bool Hook(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk, uint32_t hash, Fn detour, Fn* original,
          const char* what) {
    void* target = reinterpret_cast<void*>(RED4ext::UniversalRelocBase::Resolve(hash));
    const bool ok = target && sdk->hooking->Attach(handle, target, reinterpret_cast<void*>(detour),
                                                   reinterpret_cast<void**>(original));
    if (!ok) Log("[archive] %s NOT hooked\n", what);
    return ok;
}

}  // namespace

void AddFixup(const char* type, Fixup fix) {
    if (type && fix) Fixups().emplace_back(RED4ext::CName(type), fix);
}

void Install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk) {
    RetireLegacyCopies();
    LiftWorldWidgetLimit();
    LARGE_INTEGER started{}, done{}, frequency{};
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&started);
    if (ReadManifest()) {
        QueryPerformanceCounter(&done);
        Log("[archive] manifest read: %.1f ms\n", double(done.QuadPart - started.QuadPart) * 1000.0 / double(frequency.QuadPart));
    } else {
        Log("[archive] no manifest at %ls: only the archive is mounted\n", ManifestPath().c_str());
    }
    g_loadArchives = reinterpret_cast<LoadArchivesFn>(RED4ext::UniversalRelocBase::Resolve(kLoadArchivesHash));
    int hooked = 0;
    if (g_loadArchives)
        hooked += Hook(handle, sdk, kInitializeArchivesHash, &Detour_InitializeArchives, &g_initializeArchives,
                       "archive mounting");
    hooked += Hook(handle, sdk, kLoadFactoryAsyncHash, &Detour_LoadFactoryAsync, &g_loadFactoryAsync, "item factories");
    hooked += Hook(handle, sdk, kInitializeAnimationsHash, &Detour_InitializeAnimations, &g_initializeAnimations,
                   "animation sets");
    hooked += Hook(handle, sdk, kSerializerLoadHash, &Detour_SerializerLoad, &g_serializerLoad, "patch preloading");
    hooked += Hook(handle, sdk, kDeserializeHash, &Detour_Deserialize, &g_deserialize, "patch load order");
    hooked += Hook(handle, sdk, kMeshPostLoadHash, &Detour_MeshPostLoad, &g_meshPostLoad, "mesh patches");
    hooked += Hook(handle, sdk, kDependenciesReadyHash, &Detour_DependenciesReady, &g_dependenciesReady,
                   "streaming block patches and fixes");
    hooked += Hook(handle, sdk, kSpawnFromTemplateHash, &Detour_SpawnFromTemplate, &g_spawnFromTemplate,
                   "record appearances");
    Log("[archive] %d of 8 engine paths hooked\n", hooked);
}

}  // namespace cvr::archive
