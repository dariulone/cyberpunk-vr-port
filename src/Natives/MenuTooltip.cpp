// The VR menu's native item tooltips, without Codeware's undeclared-library loader.
// Stock controller SpawnFromExternal only searches its library's declared dependencies. Load the four
// tooltip libraries asynchronously and declare that ready dependency before the stock spawn. The dependency
// must remain on the owner library: nested tooltip controllers resolve their lazy local modules through it.
// The engine still owns widget creation, initialization, and attachment to the caller's parent.
#include "Natives/NativeFunctions.hpp"
#include "Natives/MenuTooltipDependencies.hpp"
#include "Core/GameBuild.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include <RED4ext/ResourceLoader.hpp>
#include <RED4ext/Relocation.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/IWidgetController.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/WidgetLibraryResource.hpp>
#include <RED4ext/Scripting/Natives/Generated/ink/WidgetLibraryItemInstance.hpp>
#include <RED4ext/Scripting/Natives/Generated/red/ResourceReferenceScriptToken.hpp>
#include <array>
#include <mutex>
#include <shared_mutex>

extern void Log(const char*, ...);
namespace {
using namespace RED4ext;
using Library = ink::WidgetLibraryResource;
using Instance = ink::WidgetLibraryItemInstance;
using ExternalFn = void* (*)(Library*, Handle<Instance>&, ResourcePath, CName);
using FindFn = void* (*)(Library*, Handle<Library>&, ResourcePath);
constexpr uint32_t kSpawnExternal = 506278179, kFindExternal = 2756653218;
ExternalFn originalExternal{};
FindFn originalFind{};
bool installed{};

struct Entry {
    const char* path;
    SharedPtr<ResourceToken<Library>> token;
};
// Bounded cache: no controller/widget handles and no arbitrary resources retained by a script argument.
std::array<Entry, 4> libraries{{
    {"base\\gameplay\\gui\\common\\tooltip\\itemtooltip.inkwidget", {}},
    {"base\\gameplay\\gui\\common\\tooltip\\new_itemtooltip.inkwidget", {}},
    {"base\\gameplay\\gui\\common\\tooltip\\cyberdecktooltip.inkwidget", {}},
    {"base\\gameplay\\gui\\common\\tooltip\\programtooltip.inkwidget", {}}
}};
std::mutex libraryMutex;
std::shared_mutex dependencyMutex;

// -1 failed/unsupported, 0 loading, 1 ready. No waits or resource fetching on the game thread.
int Prepare(ResourcePath path, SharedPtr<ResourceToken<Library>>* ready = nullptr) {
    if (!installed) return -1;
    std::lock_guard lock(libraryMutex);
    for (auto& entry : libraries) {
        if (ResourcePath(entry.path) != path) continue;
        if (!entry.token) {
            auto* loader = ResourceLoader::Get();
            if (!loader) return -1;
            entry.token = loader->LoadAsync<Library>(path);
            if (!entry.token) return -1;
        }
        if (entry.token->IsFailed()) return -1;
        if (!entry.token->IsLoaded()) return 0;
        if (!entry.token->resource) return -1;
        if (ready) *ready = entry.token;
        return 1;
    }
    return -1;
}

struct SpawnScope {
    SharedPtr<ResourceToken<Library>> token;
    ResourcePath path;
    CName item;
};
thread_local const SpawnScope* activeScope{};
struct ScopedSpawn {
    const SpawnScope* previous;
    explicit ScopedSpawn(const SpawnScope& scope) : previous(activeScope) { activeScope = &scope; }
    ~ScopedSpawn() { activeScope = previous; }
};

void* FindExternal(Library* owner, Handle<Library>& out, ResourcePath path) {
    std::shared_lock lock(dependencyMutex);
    return originalFind(owner, out, path);
}

void* SpawnExternal(Library* owner, Handle<Instance>& out, ResourcePath path, CName item) {
    const auto* scope = activeScope;
    if (scope && owner && scope->path == path && scope->item == item && owner != scope->token->resource.GetPtr()) {
        // A bare SpawnFromLocal on the external resource creates the shell, but its child controllers
        // cannot find itemHeader/itemDetails later. The owner's dependency must exist during stock setup.
        std::unique_lock lock(dependencyMutex);
        cvr::menu_tooltip::DeclareReadyLibrary<Ref<Library>>(owner->externalLibraries, path, scope->token);
    }
    // Preserve the stock external-spawn path, including originating-library association for lazy modules.
    return originalExternal(owner, out, path, item);
}

bool InstallTooltipSpawn() {
    if (!cvr::game_build::Supported()) return false;
    auto* external = reinterpret_cast<void*>(UniversalRelocBase::Resolve(kSpawnExternal));
    auto* find = reinterpret_cast<void*>(UniversalRelocBase::Resolve(kFindExternal));
    if (!external || !find) return false;
    if (MH_CreateHook(find, reinterpret_cast<void*>(&FindExternal), reinterpret_cast<void**>(&originalFind)) != MH_OK)
        return false;
    if (MH_CreateHook(external, reinterpret_cast<void*>(&SpawnExternal), reinterpret_cast<void**>(&originalExternal)) != MH_OK) {
        MH_RemoveHook(find);
        return false;
    }
    if (MH_EnableHook(find) != MH_OK || MH_EnableHook(external) != MH_OK) {
        MH_DisableHook(find); MH_DisableHook(external);
        MH_RemoveHook(find); MH_RemoveHook(external);
        return false;
    }
    installed = true;
    return true;
}
CVR_HOOK("MenuTooltip", cvr::hooks::Stage::Boot, 84, InstallTooltipSpawn);
}

void VRMenuTooltipPrepare(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    RED4ext::CString path;
    RED4ext::GetParameter(frame, &path);
    ++frame->code;
    const auto result = Prepare(RED4ext::ResourcePath(path.c_str()));
    if (out) *out = result;
}

void VRMenuTooltipSpawn(RED4ext::IScriptable*, RED4ext::CStackFrame* frame,
                       RED4ext::Handle<RED4ext::IScriptable>* out, int64_t) {
    using namespace RED4ext;
    Handle<ink::IWidgetController> controller;
    Handle<ink::Widget> parent;
    CString path;
    CName item;
    GetParameter(frame, &controller);
    GetParameter(frame, &parent);
    GetParameter(frame, &path);
    GetParameter(frame, &item);
    ++frame->code;
    if (out) *out = {};
    if (!controller || !parent) return;
    ResourcePath resourcePath(path.c_str());
    SharedPtr<ResourceToken<Library>> token;
    if (Prepare(resourcePath, &token) != 1) return;
    auto* function = controller->GetType()->GetFunction("SpawnFromExternal");
    if (!function) return;
    WeakHandle<ink::Widget> weakParent(parent), result;
    ResRef resource;
    resource.resource.path = resourcePath;
    StackArgs_t args{{nullptr, &weakParent}, {nullptr, &resource}, {nullptr, &item}};
    const SpawnScope scope{token, resourcePath, item};
    const ScopedSpawn guard(scope);
    if (!ExecuteFunction(controller.GetPtr(), function, &result, args)) {
        Log("[menu-tooltip] native SpawnFromExternal invocation failed\n");
        return;
    }
    if (out) *out = result.Lock();
}
