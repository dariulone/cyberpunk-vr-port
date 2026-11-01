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

struct MenuTransition {uint64_t tick{},state{};int32_t previous{},requested{},nativeBefore{};uint32_t thread{};};
static_assert(sizeof(MenuTransition)==32);
struct MenuSource {uint64_t tick{},object{};uint32_t type{},getter{};std::array<uint8_t,128> prefix{};uint64_t owner{};uint32_t ownerType{};int32_t mode=-1;};
static_assert(sizeof(MenuSource)==168);
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuTransitionSeq{},CyberpunkVR_MenuTransitionCount{};
__declspec(dllexport) MenuTransition CyberpunkVR_MenuTransitions[64]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuSourceSeq{};
__declspec(dllexport) MenuSource CyberpunkVR_MenuSource{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuMethodsLength{};
__declspec(dllexport) char CyberpunkVR_MenuMethods[32768]{};
}
static std::mutex menuTransitionMutex;

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

extern "C" void __fastcall OnMenuModeHookCallback(void* menuState, int newMode) {
    // The written flag is the world's pause behind a menu. The main menu as a place keeps that world rendering, so the
    // game writes 0 under an open main menu: it is still a menu (Lobby/MenuLobby.hpp HoldsRender).
    if (newMode == 0 && cvr::lobby::HoldsRender(menuState)) newMode = 1;
    const int prevMode = g_menuModeValue;
    g_menuModeValue = newMode;
    static const bool diagnostics=GetEnvironmentVariableW(L"CVR_STEREO_FAMILY_MANIFEST",nullptr,0)>0 ||
        GetEnvironmentVariableW(L"CVR_STARTUP_DIAGNOSTICS",nullptr,0)>0;
    if(diagnostics && prevMode!=newMode) {
        std::lock_guard lock(menuTransitionMutex);CyberpunkVR_MenuTransitionSeq.fetch_add(1);
        const auto index=CyberpunkVR_MenuTransitionCount.load();
        if(index<64) {
            auto& row=CyberpunkVR_MenuTransitions[index];row.tick=GetTickCount64();row.state=reinterpret_cast<uintptr_t>(menuState);
            row.previous=prevMode;row.requested=newMode;row.nativeBefore=-1;row.thread=GetCurrentThreadId();
            SIZE_T bytes{};ReadProcessMemory(GetCurrentProcess(),static_cast<const uint8_t*>(menuState)+0x128,&row.nativeBefore,4,&bytes);
            CyberpunkVR_MenuTransitionCount=index+1;
        }
        CyberpunkVR_MenuTransitionSeq.fetch_add(1);
    }

    if (prevMode != newMode) {
        if (g_verboseLog) Log("MenuMode hook: state=%p prev=%d new=%d\n", menuState, prevMode, newMode);
    }
}

bool InstallMenuModeHook() {
    const char* pattern = "\x33\xC9\x41\x8B\xD0\x41\x8B\xC0\x87\x83\x28\x01\x00\x00";
    const char* mask = "xxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
    void* tramp = AllocateTrampoline(found, 384);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD9; // mov rcx,rbx
    code[pos++] = 0x44; code[pos++] = 0x89; code[pos++] = 0xC2; // mov edx,r8d
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnMenuModeHookCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;
    
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    code[pos++] = 0x33; code[pos++] = 0xC9;
    code[pos++] = 0x41; code[pos++] = 0x8B; code[pos++] = 0xD0;
    code[pos++] = 0x41; code[pos++] = 0x8B; code[pos++] = 0xC0;
    code[pos++] = 0x87; code[pos++] = 0x83; code[pos++] = 0x28; code[pos++] = 0x01; code[pos++] = 0x00; code[pos++] = 0x00;

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i) found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}

CVR_HOOK("MenuMode", ::cvr::hooks::Stage::Boot, 70, InstallMenuModeHook);
