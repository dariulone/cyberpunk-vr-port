// SettingsRes -- one hook, one file. Installed from the registry it registers itself with at the
// bottom; see Hooks/Hook.hpp for why the order is declared here rather than in a boot function.
//
// The render-settings object, as the engine finalises it. Read-only: it reports the active and
// target resolutions and the size the XR runtime recommends, which is a REFERENCE value --
// printing it beside the others has already been misread once as evidence of a forced override.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/settings_res.rs: the patch, the callback and its log; the core's side stays here.
// The core's side: rust/crates/cvr-render/src/core_services.rs.
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("SettingsRes", ::cvr::hooks::Stage::Boot, 85, cvr_hook_settings_res_install);
#endif
#else
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

namespace {
uint64_t g_settingsResHits = 0;
}  // namespace

extern "C" void __fastcall OnSettingsResCallback(void* settingsPtr) {
    g_settingsResHits++;

    const uintptr_t settings = reinterpret_cast<uintptr_t>(settingsPtr);
    if (settings < 0x10000) {
        return;
    }

    const uintptr_t prev = g_settingsResPtr;
    g_settingsResPtr = settings;
    ApplySettingsResolutionOverride(settings);

    if (settings != prev || ((g_settingsResHits % 600) == 1)) {
        uint32_t activeWidth = 0;
        uint32_t activeHeight = 0;
        uint32_t targetWidth = 0;
        uint32_t targetHeight = 0;
        ReadU32Safe(settings + 0x18, &activeWidth);
        ReadU32Safe(settings + 0x1C, &activeHeight);
        ReadU32Safe(settings + 0xB0, &targetWidth);    // 2.31: +0x84/+0x88
        ReadU32Safe(settings + 0xB4, &targetHeight);
        // On CHANGE only: this line is a status, and a status that repeats is noise. It printed on
        // every hook call, 201 identical lines in one session.
        static uint32_t s_lastA = 0, s_lastB = 0, s_lastC = 0, s_lastD = 0;
        const bool changed = (activeWidth != s_lastA) || (activeHeight != s_lastB) ||
                             (targetWidth != s_lastC) || (targetHeight != s_lastD);
        s_lastA = activeWidth; s_lastB = activeHeight; s_lastC = targetWidth; s_lastD = targetHeight;
        if (changed)
        Log("SettingsRes hook: ptr=%p active=%ux%u target=%ux%u forced=%ux%u\n",
            reinterpret_cast<void*>(settings),
            activeWidth,
            activeHeight,
            targetWidth,
            targetHeight,
            GetForcedRenderWidthValue(),
            GetForcedRenderHeightValue());
    }
}

bool InstallSettingsResHook() {
    // 2.5 sub_1405D9590: cmp [rcx+18],eax(target w) / jne / mov eax,[rcx+B4] / cmp [rcx+1C],eax (2.31: +88)
    const char* pattern = "\x39\x41\x18\x75\x00\x8B\x81\xB4\x00\x00\x00\x39\x41\x1C";
    const char* mask = "xxxx?xxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 14;
    void* tramp = AllocateTrampoline(found, 512);
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

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnSettingsResCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;

    code[pos++] = 0x5D;
    code[pos++] = 0x41; code[pos++] = 0x5B;
    code[pos++] = 0x41; code[pos++] = 0x5A;
    code[pos++] = 0x41; code[pos++] = 0x59;
    code[pos++] = 0x41; code[pos++] = 0x58;
    code[pos++] = 0x5A;
    code[pos++] = 0x59;
    code[pos++] = 0x58;
    code[pos++] = 0x9D;

    // The relocated compare. Its jne leaves for the native "sizes differ" exit, as the original does.
    code[pos++] = 0x39; code[pos++] = 0x41; code[pos++] = 0x18;
    const uint8_t* differ = found + 5 + static_cast<int8_t>(found[4]);
    code[pos++] = 0x0F; code[pos++] = 0x85;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>(differ - (code + pos + 4));
    pos += 4;
    code[pos++] = 0x8B; code[pos++] = 0x81; code[pos++] = 0xB4; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;
    code[pos++] = 0x39; code[pos++] = 0x41; code[pos++] = 0x1C;

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

CVR_HOOK("SettingsRes", ::cvr::hooks::Stage::Boot, 85, InstallSettingsResHook);
#endif
