#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// ProjAspectCall -- one hook, one file.
//
// The three call sites that pass FOV and aspect by offset. One installer, three patches --
// they share a callback that is told which site it is speaking for, so they are one hook.
//
// REGISTERED, BUT NOT INSTALLED. The boot had this call commented out:
//
//     //bool copyH = InstallProjAspectCopyHook();
//     //bool aspecCall = InstallProjAspectCallHooks();
//
// so the hook has not run for a long time. Moving it to the registry would have QUIETLY TURNED IT
// BACK ON, which is the one thing a restructure must not do -- these write the engine's projection
// FOV and aspect, and the last time that was got wrong the world came out "too big" in both eyes.
// The gate below preserves the disabled state exactly. To try it again, return true from it; the
// registry will then report the install like any other hook instead of the silence a commented-out
// call gives.

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

#if CVR_RUST_VRIK
// The callback and the stubs are rust/crates/cvr-rig/src/vrik/proj_aspect.rs; the registration (gated off) stays.
extern "C" bool cvr_hook_proj_aspect_call_install();
extern "C" bool cvr_hook_proj_aspect_call_wanted();
CVR_HOOK_IF("ProjAspectCall", ::cvr::hooks::Stage::Boot, 51, cvr_hook_proj_aspect_call_install, cvr_hook_proj_aspect_call_wanted);
#else
// State private to this hook.
namespace {
uint64_t g_projAspectCallHits = 0;
float g_projAspectCallLastFov = 0.0f;
float g_projAspectCallLastAspect = 0.0f;
uint32_t g_projAspectCallLastFovOff = 0;
uint32_t g_projAspectCallLastAspectOff = 0;
bool g_projAspectCallLastPatched = false;
}  // namespace

namespace { bool ProjAspectCallWanted() { return false; } }

extern "C" void __fastcall OnProjAspectCallCallback(void* src, uint32_t fovOff, uint32_t aspectOff, uint32_t siteId) {
    (void)siteId;
    g_projAspectCallHits++;
    g_projAspectCallLastPatched = false;
    g_projAspectCallLastFovOff = fovOff;
    g_projAspectCallLastAspectOff = aspectOff;
    if (!src)
        return;
    __try {
        const uintptr_t base = reinterpret_cast<uintptr_t>(src);
        float* fovPtr = reinterpret_cast<float*>(base + fovOff);
        float* aspectPtr = reinterpret_cast<float*>(base + aspectOff);
        const float fov = *fovPtr;
        const float aspect = *aspectPtr;
        g_projAspectCallLastFov = fov;
        g_projAspectCallLastAspect = aspect;
        
        const bool fovLooksValid = std::isfinite(fov) && (fov > 30.0f) && (fov < 180.0f);
        const bool aspectLooks16x9 = std::isfinite(aspect) && (aspect > 1.70f) && (aspect < 1.85f);
        
        // === NUOVO: Rileva shadow map e saltale ===
        bool isShadowMap = false;
        
        // Shadow map tipiche: FOV molto piccolo (ortografiche) o molto ampio
        if (fov <= 1.0f || fov >= 179.0f) {
            isShadowMap = true;
        }
        
        // Se aspect non è 16:9, probabilmente non è la camera principale
        if (!isShadowMap && !aspectLooks16x9) {
            if (aspect < 1.5f || aspect > 2.0f) {
                isShadowMap = true;
            }
        }
        
        // Se FOV è nel range "camera VR" (~80-110°) e aspect è 16:9, è la camera principale
        if (!isShadowMap && fovLooksValid && aspectLooks16x9) {
            if (fov > 80.0f && fov < 110.0f) {
                isShadowMap = false; // Confermato: camera principale
            } else if (fov > 120.0f && fov < 170.0f) {
                // FOV 16:9-horizontal (~132.5°) → probabilmente camera principale
                isShadowMap = false;
            } else {
                // FOV fuori range camera VR ma aspect 16:9 → sospetto shadow map
                isShadowMap = true;
            }
        }
        
        if (fovLooksValid && aspectLooks16x9 && !isShadowMap) {
            float patchedFov = fov;
            if (patchedFov > 120.0f && patchedFov < 170.0f) {
                const float halfH = patchedFov * 0.5f * 3.1415926535f / 180.0f;
                patchedFov = std::atan(std::tan(halfH) * (9.0f / 16.0f)) * 2.0f * 180.0f / 3.1415926535f;
                *fovPtr = patchedFov;
            }
            *aspectPtr = 1.0f;
            g_projAspectCallLastFov = patchedFov;
            g_projAspectCallLastAspect = 1.0f;
            g_projAspectCallLastPatched = true;
        }
        
        if (g_projAspectCallHits <= 20 || (g_projAspectCallHits % 600) == 0 || g_projAspectCallLastPatched || isShadowMap) {
            Log("ProjAspectCall: hits=%llu fovOff=0x%X aspectOff=0x%X fov=%.6f aspect=%.6f patched=%d isShadowMap=%d\n",
                static_cast<unsigned long long>(g_projAspectCallHits),
                fovOff,
                aspectOff,
                g_projAspectCallLastFov,
                g_projAspectCallLastAspect,
                g_projAspectCallLastPatched ? 1 : 0,
                isShadowMap ? 1 : 0);
        }
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
    }
}

static bool InstallProjAspectCallHookAtRva(uintptr_t rva, const uint8_t* expected, int replaceLen, uint32_t fovOff, uint32_t aspectOff, uint32_t siteId) {
    HMODULE exe = GetModuleHandleA("Cyberpunk2077.exe");
    if (!exe)
        return false;

    uint8_t* found = reinterpret_cast<uint8_t*>(exe) + rva;
    for (int i = 0; i < replaceLen; ++i) {
        if (found[i] != expected[i]) {
            if (g_verboseLog) {
                Log("ProjAspectCall hook: RVA 0x%llX mismatch at +0x%X (got %02X expected %02X)\n",
                    static_cast<unsigned long long>(rva), i, found[i], expected[i]);
            }
            return false;
        }
    }

    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp)
        return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Save volatile regs + flags
    code[pos++] = 0x9C;
    code[pos++] = 0x50;
    code[pos++] = 0x51;
    code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x50;
    code[pos++] = 0x41; code[pos++] = 0x51;
    code[pos++] = 0x41; code[pos++] = 0x52;
    code[pos++] = 0x41; code[pos++] = 0x53;
    code[pos++] = 0x55;

    // Save xmm0-xmm3 (xmm0 already carries another input to sub_140109814)
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    // rcx=src(rdx), edx=fovOff, r8d=aspectOff, r9d=siteId
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xD1; // mov rcx, rdx
    code[pos++] = 0xBA; *reinterpret_cast<uint32_t*>(code + pos) = fovOff; pos += 4; // mov edx, imm32
    code[pos++] = 0x41; code[pos++] = 0xB8; *reinterpret_cast<uint32_t*>(code + pos) = aspectOff; pos += 4; // mov r8d, imm32
    code[pos++] = 0x41; code[pos++] = 0xB9; *reinterpret_cast<uint32_t*>(code + pos) = siteId; pos += 4; // mov r9d, imm32

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnProjAspectCallCallback));
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

    // Execute original bytes (loads into xmm2/xmm1)
    for (int i = 0; i < replaceLen; ++i)
        code[pos++] = expected[i];

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

bool InstallProjAspectCallHooks() {
    bool ok = false;

    {
        const uint8_t patternA[] = {
            0xF3, 0x0F, 0x10, 0x92, 0x84, 0x00, 0x00, 0x00,
            0xF3, 0x0F, 0x10, 0x8A, 0x80, 0x00, 0x00, 0x00,
        };
        ok |= InstallProjAspectCallHookAtRva(0x59D66A, patternA, sizeof(patternA), 0x80, 0x84, 1);
        ok |= InstallProjAspectCallHookAtRva(0x59D97E, patternA, sizeof(patternA), 0x80, 0x84, 2);
    }

    {
        const uint8_t patternB[] = {
            0xF3, 0x0F, 0x10, 0x52, 0x7C,
            0xF3, 0x0F, 0x10, 0x4A, 0x78,
        };
        ok |= InstallProjAspectCallHookAtRva(0x59D8EC, patternB, sizeof(patternB), 0x78, 0x7C, 3);
    }

    return ok;
}

CVR_HOOK_IF("ProjAspectCall", ::cvr::hooks::Stage::Boot, 51, InstallProjAspectCallHooks, ProjAspectCallWanted);
#endif
#endif
