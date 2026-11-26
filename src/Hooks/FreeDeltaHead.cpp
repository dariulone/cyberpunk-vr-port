#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// FreeDeltaHead -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order are declared here rather than in a boot function.
//
// Free-camera heading delta. A byte patch with no callback of ours at the site.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

#if CVR_RUST_VRIK
// THE PATCH IS RUST'S (rust/crates/cvr-rig/src/vrik/free_delta_head.rs); the registration stays here, so the install
// order holds.
extern "C" bool cvr_hook_free_delta_head_install();
CVR_HOOK("FreeDeltaHead", ::cvr::hooks::Stage::Boot, 77, cvr_hook_free_delta_head_install);
#else
bool InstallFreeDeltaHeadHook() {
    const char* pattern = "\xF3\x0F\x11\x9E\xCC\x0C\x00\x00\x74\x5C";
    const char* mask = "xxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8; // movss [rsi+0CCCh],xmm3
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Original instruction first. No live modification here: this path crashed when we
    // rewrote the scalar, so keep it telemetry-only.
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x9E;
    code[pos++] = 0xCC; code[pos++] = 0x0C; code[pos++] = 0x00; code[pos++] = 0x00;

    // Telemetry after the original store.
    code[pos++] = 0x50;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(g_telemetry) + kFreeDeltaTelemetryOffset);
    code[pos++] = 0xFF; code[pos++] = 0x00;
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0x70; code[pos++] = 0x08;
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x58; code[pos++] = 0x10; // movss [rax+10h],xmm3
    code[pos++] = 0x58;

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
CVR_HOOK("FreeDeltaHead", ::cvr::hooks::Stage::Boot, 77, InstallFreeDeltaHeadHook);
#endif
#endif
