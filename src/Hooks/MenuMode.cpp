// MenuMode -- one hook, one file. Installed from the registry it registers itself with at the
// bottom; see Hooks/Hook.hpp for why the order is declared here rather than in a boot function.
//
// Menu mode. The engine tells us its menu state changed; the whole VR path branches on it --
// the submitted layer becomes a quad, the camera stops following the head, input is rerouted.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Lobby/MenuLobby.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>
#include <atomic>
#include <mutex>
#include <array>
#include <intrin.h>
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/CProperty.hpp>
#include <string>

#if !CVR_RUST_VRIK   // unused by the Rust build
struct MenuSource {uint64_t tick{},object{};uint32_t type{},getter{};std::array<uint8_t,128> prefix{};uint64_t owner{};uint32_t ownerType{};int32_t mode=-1;};
#endif
#if !CVR_RUST_VRIK   // unused by the Rust build
static_assert(sizeof(MenuSource)==168);
#endif
extern "C" {
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuSourceSeq{};
#endif
#if !CVR_RUST_VRIK   // defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs
__declspec(dllexport) MenuSource CyberpunkVR_MenuSource{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuMethodsLength{};
__declspec(dllexport) char CyberpunkVR_MenuMethods[32768]{};
#endif
}

#if CVR_RUST_VRIK
// The observation is rust/crates/cvr-rig/src/vrik/menu_source.rs; the registrations stay here.
// The Rust build registers these detours from Rust (rust/crates/cvr-render/src/engine_detours.rs).
#else
namespace {
using MenuSourceGet=void*(*)(void*,void*);
MenuSourceGet originalMenuSource{};
using MenuSourceMode=int32_t(*)(void*);
MenuSourceMode originalMenuModeGet{};
std::mutex sourceMutex;
void ObserveMenuMethods(){
    if(CyberpunkVR_MenuMethodsLength.load())return;
    auto* rtti=RED4ext::CRTTISystem::Get();if(!rtti || !rtti->GetClass("SingleplayerMenuGameController"))return;
    std::string text;
    for(const auto* name:{"SingleplayerMenuGameController","PauseMenuListItemData","inkLoadingLayer","inkInitialLoadingControllerSupervisor"}){
        auto* type=rtti->GetClass(name);
        for(unsigned depth=0;type && depth<3;++depth,type=type->parent){
            text+="CLASS ";text+=type->name.ToString();text+='\n';
            for(auto* function:type->funcs)if(function){text+=function->fullName.ToString();text+=" params=";text+=std::to_string(function->params.Size());text+='\n';}
            if(depth==0){RED4ext::DynArray<RED4ext::CProperty*> properties;type->GetProperties(properties);
                for(auto* property:properties)if(property && property->type){text+="PROPERTY ";text+=property->name.ToString();text+=' ';text+=property->type->GetName().ToString();text+='\n';}}
        }
    }
    if(auto* type=rtti->GetEnum("PauseMenuAction"))for(uint32_t i=0;i<type->hashList.Size() && i<type->valueList.Size();++i){
        text+="ACTION ";text+=type->hashList[i].ToString();text+=' ';text+=std::to_string(type->valueList[i]);text+='\n';}
    if(text.size()>=sizeof(CyberpunkVR_MenuMethods))return;
    std::memcpy(CyberpunkVR_MenuMethods,text.data(),text.size());CyberpunkVR_MenuMethodsLength.store(static_cast<uint32_t>(text.size()),std::memory_order_release);
}
bool ReadMenuSource(const void* p,void* out,size_t size){SIZE_T copied{};return p && ReadProcessMemory(GetCurrentProcess(),p,out,size,&copied) && copied==size;}
bool MenuSourceEnabled(){
    if(!GetEnvironmentVariableW(L"CVR_STARTUP_DIAGNOSTICS",nullptr,0))return false;
    // 2.5 sub_1404C259C, all 0xBF bytes (2.31 sub_14029D10C: 0xA6 bytes, 0x05536A812E9DFB49). Same handle getter
    // ([object+0x1E8/0x1F0]), reordered; the caller's return site 0x4C32E8 is unchanged code.
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,0xBF> code{};
    if(!ReadMenuSource(reinterpret_cast<void*>(base+0x4C259C),code.data(),code.size()))return false;
    uint64_t hash=14695981039346656037ull;for(auto b:code)hash=(hash^b)*1099511628211ull;return hash==0x2F88BC5D4F5787EBull;
}
void* ObserveMenuSource(void* system,void* output){
    void* result=originalMenuSource(system,output);static const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(reinterpret_cast<uintptr_t>(_ReturnAddress())!=base+0x4C32E8)return result;
    MenuSource row;row.tick=GetTickCount64();uintptr_t type{},getter{};
    if(ReadMenuSource(output,&row.object,8) && row.object && ReadMenuSource(reinterpret_cast<void*>(row.object),&type,8) &&
        ReadMenuSource(reinterpret_cast<void*>(type+0x18),&getter,8)){
        row.type=type>=base && type-base<=UINT32_MAX?uint32_t(type-base):0;
        row.getter=getter>=base && getter-base<=UINT32_MAX?uint32_t(getter-base):0;
        ReadMenuSource(reinterpret_cast<void*>(row.object),row.prefix.data(),row.prefix.size());
    }
    std::lock_guard lock(sourceMutex);ObserveMenuMethods();CyberpunkVR_MenuSourceSeq.fetch_add(1);
    CyberpunkVR_MenuSource=row;CyberpunkVR_MenuSourceSeq.fetch_add(1);return result;
}
CVR_DETOUR_IF("[startup] native menu source observation",0x4C259C,ObserveMenuSource,originalMenuSource,MenuSourceEnabled)
bool MenuModeGetterEnabled(){
    if(!GetEnvironmentVariableW(L"CVR_STARTUP_DIAGNOSTICS",nullptr,0))return false;
    // 2.5 sub_1404C278C (2.31 sub_14029D098 0xFFB684F78627FEAC): the same code, only its rip-relative operands moved.
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));std::array<uint8_t,0x71> code{};
    if(!ReadMenuSource(reinterpret_cast<void*>(base+0x4C278C),code.data(),code.size()))return false;
    uint64_t hash=14695981039346656037ull;for(auto b:code)hash=(hash^b)*1099511628211ull;return hash==0x9B2A220D3C65DC17ull;
}
int32_t ObserveMenuModeGet(void* source){
    const auto result=originalMenuModeGet(source);static const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(reinterpret_cast<uintptr_t>(_ReturnAddress())==base+0x4C32FC){
        uintptr_t owner{},type{};ReadMenuSource(static_cast<uint8_t*>(source)+8,&owner,8);
        if(owner)ReadMenuSource(reinterpret_cast<void*>(owner),&type,8);
        std::lock_guard lock(sourceMutex);CyberpunkVR_MenuSourceSeq.fetch_add(1);
        if(CyberpunkVR_MenuSource.object==reinterpret_cast<uintptr_t>(source)){
            CyberpunkVR_MenuSource.owner=owner;CyberpunkVR_MenuSource.ownerType=type>=base && type-base<=UINT32_MAX?uint32_t(type-base):0;
            CyberpunkVR_MenuSource.mode=result;
        }
        CyberpunkVR_MenuSourceSeq.fetch_add(1);
    }
    return result;
}
CVR_DETOUR_IF("[startup] native menu source mode",0x4C278C,ObserveMenuModeGet,originalMenuModeGet,MenuModeGetterEnabled)
}
#endif

// The menu mode callback, its transition table and the hook: src/Hooks/MenuModeCallback.cpp.
