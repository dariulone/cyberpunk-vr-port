#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// ForceHeadingUpdate -- one hook, one file. Installed from the registry it registers itself with at the
// bottom; see Hooks/Hook.hpp for why the order is declared here rather than in a boot function.
//
// A pure byte patch, no callback: it forces the engine to recompute the heading every frame
// rather than only when its own input says so. Nothing of ours runs at the site.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

#if CVR_RUST_VRIK
// THE PATCH IS RUST'S (rust/crates/cvr-rig/src/vrik/force_heading_update.rs); the registration stays here, so the install
// order holds.
extern "C" bool cvr_hook_force_heading_update_install();
CVR_HOOK("ForceHeadingUpdate", ::cvr::hooks::Stage::Boot, 75, cvr_hook_force_heading_update_install);
#else
bool InstallForceHeadingUpdateHook() {
    const char* pattern = "\x48\x8B\xCB\xF3\x0F\x10\x4F\x1C\xE8\x00\x00\x00\x00\x84\xC0";
    const char* mask = "xxxxxxxxx????xx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 15;
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    const int32_t relCall = *reinterpret_cast<int32_t*>(found + 9);
    const uintptr_t callTarget = reinterpret_cast<uintptr_t>(found + 13) + relCall;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    code[pos++] = 0x48; code[pos++] = 0x8B; code[pos++] = 0xCB; // mov rcx,rbx
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4F; code[pos++] = 0x1C; // movss xmm1,[rdi+1C]
    WriteMovRaxImm64(code, pos, callTarget);
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax
    code[pos++] = 0x30; code[pos++] = 0xC0; // xor al,al

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
    Log("ForceHeadingUpdate hook installed at %p target=%p\n", found, reinterpret_cast<void*>(callTarget));
    return true;
}

CVR_HOOK("ForceHeadingUpdate", ::cvr::hooks::Stage::Boot, 75, InstallForceHeadingUpdateHook);
#endif
#endif
