// The menu mode hook (src/Hooks/MenuMode.cpp keeps the startup observers of the menu source): the engine says its menu
// state changed, and the whole VR path branches on it -- the submitted layer becomes a quad, the camera stops following
// the head, input is rerouted. In the Rust build the callback and its stub are rust/crates/cvr-rig/src/vrik/menu_mode.rs.
#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <mutex>

struct MenuTransition {uint64_t tick{},state{};int32_t previous{},requested{},nativeBefore{};uint32_t thread{};};
static_assert(sizeof(MenuTransition)==32);
extern "C" {
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuTransitionSeq{},CyberpunkVR_MenuTransitionCount{};
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/render_moved_exports.rs'
__declspec(dllexport) MenuTransition CyberpunkVR_MenuTransitions[64]{};
#endif
}

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("MenuMode", ::cvr::hooks::Stage::Boot, 70, cvr_hook_menu_mode_install);
#endif
#else
// The lobby holding the menu up, through src/Hooks/HookServices.cpp, as the Rust port asks it.
extern "C" bool cvr_cpp_lobby_holds_render(uintptr_t menuState);
static std::mutex menuTransitionMutex;

extern "C" void __fastcall OnMenuModeHookCallback(void* menuState, int newMode) {
    // The written flag is the world's pause behind a menu. The main menu as a place keeps that world rendering, so the
    // game writes 0 under an open main menu: it is still a menu (Lobby/MenuLobby.hpp HoldsRender).
    if (newMode == 0 && cvr_cpp_lobby_holds_render(reinterpret_cast<uintptr_t>(menuState))) newMode = 1;
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
#endif
