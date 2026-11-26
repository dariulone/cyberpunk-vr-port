#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// ProjStage -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order live here rather than in a boot function.
//
// The projection staging copy, one of the four sites that carry FOV and aspect into the
// engine's projection block.

#include "Hooks/Hook.hpp"
#if CVR_RUST_VRIK
// rust/crates/cvr-rig/src/vrik/proj_stage.rs: the same stub and callback.
extern "C" bool cvr_hook_proj_stage_install();
CVR_HOOK("ProjStage", ::cvr::hooks::Stage::Boot, 55, cvr_hook_proj_stage_install);
#else
#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>
#include <cmath>

extern "C" void __fastcall OnProjStageCallback(const void* src) {
    g_projStageHits++;
    g_projStagePatched = false;
    if (!src) return;

    __try {
        const uintptr_t base = reinterpret_cast<uintptr_t>(src);
        const float fov = *reinterpret_cast<const float*>(base + 0x80);
        const float aspect = *reinterpret_cast<const float*>(base + 0x84);
        const float extra = *reinterpret_cast<const float*>(base + 0x88);

        g_projStageFov = fov;
        g_projStageAspect = aspect;
        g_projStageExtra = extra;

        // === NUOVO: Rileva e salta le shadow map ===
        bool isShadowMap = false;

        // Shadow map tipiche: FOV molto piccolo o zero
        if (fov <= 1.0f || fov >= 179.0f) {
            isShadowMap = true;
        }

        // Oppure aspect non compatibile con la camera VR (es. != 1.0)
        // In VR forziamo aspect=1.0, ma le shadow map possono essere 1:1, 2:1, etc.
        // Tuttavia, se aspect è ~16:9 MA il FOV è anomalo, potrebbe essere una shadow map
        if (!isShadowMap && (aspect > 1.70f && aspect < 1.85f)) {
            // Se FOV è plausibile per la camera VR (~93°), allora è la camera principale
            if (fov > 80.0f && fov < 110.0f) {
                isShadowMap = false;
            } else {
                // Aspect 16:9 ma FOV non da camera VR → probabilmente shadow map
                isShadowMap = true;
            }
        }

        if (!isShadowMap && std::isfinite(fov) && std::isfinite(aspect) 
            && (fov > 30.0f) && (fov < 180.0f) 
            && (aspect > 1.70f) && (aspect < 1.85f)) {
            g_projStageAspect = 1.0f;
            g_projStagePatched = true;
        }

        if (g_projStageHits <= 20 || (g_projStageHits % 600) == 0 || g_projStagePatched) {
            Log("ProjStage: hits=%llu fov=%.6f aspect=%.6f extra=%.6f patched=%d isShadowMap=%d\n",
                static_cast<unsigned long long>(g_projStageHits),
                g_projStageFov,
                g_projStageAspect,
                g_projStageExtra,
                g_projStagePatched ? 1 : 0,
                isShadowMap ? 1 : 0);
        }
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
    }
}

bool InstallProjStageHook() {
    HMODULE exe = GetModuleHandleA("Cyberpunk2077.exe");
    if (!exe)
        return false;

    uint8_t* found = reinterpret_cast<uint8_t*>(exe) + 0x5FBD37; // 2.31: 0x127970
    const uint8_t expected[] = {
        0xF3, 0x0F, 0x10, 0xA2, 0x80, 0x00, 0x00, 0x00,
        0xF3, 0x0F, 0x10, 0xAA, 0x84, 0x00, 0x00, 0x00,
        0xF3, 0x0F, 0x10, 0xB2, 0x88, 0x00, 0x00, 0x00,
    };
    constexpr int replaceLen = sizeof(expected);
    for (int i = 0; i < replaceLen; ++i) {
        if (found[i] != expected[i]) {
            if (g_verboseLog) {
                Log("ProjStage hook: RVA 0x5FBD37 mismatch at +0x%X (got %02X expected %02X)\n", i, found[i], expected[i]);
            }
            return false;
        }
    }

    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp)
        return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Save volatile regs/flags and volatile xmm regs. xmm7 is nonvolatile and holds scale.
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

    // rcx = rdx (source struct)
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD1;
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnProjStageCallback));
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

    // Original loads
    for (int i = 0; i < replaceLen; ++i)
        code[pos++] = expected[i];

    // Override xmm4/xmm5 from globals (leave xmm6 as original src+88)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&g_projStageFov));
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x20; // movss xmm4,[rax]
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(&g_projStageAspect));
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x28; // movss xmm5,[rax]

    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    for (int i = 5; i < replaceLen; ++i)
        found[i] = 0x90;
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}
CVR_HOOK("ProjStage", ::cvr::hooks::Stage::Boot, 55, InstallProjStageHook);
#endif
#endif
