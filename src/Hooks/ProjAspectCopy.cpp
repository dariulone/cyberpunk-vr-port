#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// ProjAspectCopy -- one hook, one file.
//
// The projection block copy: one of the four sites that carry FOV and aspect into the
// engine's projection data.
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
// The callback and the stub are rust/crates/cvr-rig/src/vrik/proj_aspect.rs; the registration (gated off) stays.
extern "C" bool cvr_hook_proj_aspect_copy_install();
extern "C" bool cvr_hook_proj_aspect_copy_wanted();
CVR_HOOK_IF("ProjAspectCopy", ::cvr::hooks::Stage::Boot, 50, cvr_hook_proj_aspect_copy_install, cvr_hook_proj_aspect_copy_wanted);
#else
// State private to this hook.
namespace {
float g_projAspectLastSrcFov = 0.0f;
float g_projAspectLastSrcAspect = 0.0f;
float g_projAspectLastDstFov = 0.0f;
float g_projAspectLastDstAspect = 0.0f;
bool g_projAspectLastPatched = false;
uint64_t g_projAspectCopyHits = 0;
}  // namespace

namespace { bool ProjAspectCopyWanted() { return false; } }

extern "C" void __fastcall OnProjAspectCopyCallback(void* dst, const void* src) {
    g_projAspectCopyHits++;
    if (!dst || !src)
        return;
    __try {
        const uintptr_t srcAddr = reinterpret_cast<uintptr_t>(src);
        const uintptr_t dstAddr = reinterpret_cast<uintptr_t>(dst);
        const float srcFov = *reinterpret_cast<const float*>(srcAddr + 0x80);
        const float srcAspect = *reinterpret_cast<const float*>(srcAddr + 0x84);
        float dstFov = *reinterpret_cast<float*>(dstAddr + 0x80);
        float dstAspect = *reinterpret_cast<float*>(dstAddr + 0x84);
        g_projAspectLastSrcFov = srcFov;
        g_projAspectLastSrcAspect = srcAspect;
        g_projAspectLastDstFov = dstFov;
        g_projAspectLastDstAspect = dstAspect;
        g_projAspectLastPatched = false;
        
        const bool aspectLooks16x9 = (srcAspect > 1.70f && srcAspect < 1.85f);
        const bool fovLooksValid = (srcFov > 30.0f && srcFov < 180.0f);
        const bool looksLikeView = aspectLooks16x9 && fovLooksValid;
        
        // === NUOVO: Rileva shadow map e saltale ===
        bool isShadowMap = false;
        
        // Shadow map tipiche: FOV molto piccolo (ortografiche) o molto ampio
        if (srcFov <= 1.0f || srcFov >= 179.0f) {
            isShadowMap = true;
        }
        
        // Se aspect non è 16:9, probabilmente non è la camera principale
        if (!isShadowMap && !aspectLooks16x9) {
            // Shadow map spesso usano aspect 1:1, 2:1, o altri valori non standard
            if (srcAspect < 1.5f || srcAspect > 2.0f) {
                isShadowMap = true;
            }
        }
        
        // Se FOV è nel range "camera VR" (~80-110°) e aspect è 16:9, è la camera principale
        if (!isShadowMap && looksLikeView) {
            if (srcFov > 80.0f && srcFov < 110.0f) {
                isShadowMap = false; // Confermato: camera principale
            } else if (srcFov > 120.0f && srcFov < 170.0f) {
                // FOV 16:9-horizontal (~132.5°) → probabilmente camera principale
                isShadowMap = false;
            } else {
                // FOV fuori range camera VR ma aspect 16:9 → sospetto shadow map
                isShadowMap = true;
            }
        }
        
        if (looksLikeView && !isShadowMap) {
            float patchedFov = srcFov;
            // Se il FOV copiato è già il VFOV quadrato (~104), mantienilo
            // Se è l'orizzontale 16:9 (~132.5), converti nel VFOV quadrato
            if (srcFov > 120.0f && srcFov < 170.0f) {
                const float halfH = srcFov * 0.5f * 3.1415926535f / 180.0f;
                patchedFov = std::atan(std::tan(halfH) * (9.0f / 16.0f)) * 2.0f * 180.0f / 3.1415926535f;
            }
            *reinterpret_cast<float*>(dstAddr + 0x80) = patchedFov;
            *reinterpret_cast<float*>(dstAddr + 0x84) = 1.0f;
            dstFov = patchedFov;
            dstAspect = 1.0f;
            g_projAspectLastPatched = true;
            g_projAspectLastDstFov = dstFov;
            g_projAspectLastDstAspect = dstAspect;
        }
        
        if (g_projAspectCopyHits <= 20 || (g_projAspectCopyHits % 600) == 0 || g_projAspectLastPatched || isShadowMap) {
            Log("ProjAspect: hits=%llu srcFov=%.6f srcAspect=%.6f -> dstFov=%.6f dstAspect=%.6f patched=%d isShadowMap=%d\n",
                static_cast<unsigned long long>(g_projAspectCopyHits),
                srcFov,
                srcAspect,
                dstFov,
                dstAspect,
                g_projAspectLastPatched ? 1 : 0,
                isShadowMap ? 1 : 0);
        }
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
    }
}

bool InstallProjAspectCopyHook() {
    HMODULE exe = GetModuleHandleA("Cyberpunk2077.exe");
    if (!exe)
        return false;

    uint8_t* found = reinterpret_cast<uint8_t*>(exe) + 0x2129F8;
    const uint8_t expected[] = {
        0x0F, 0x10, 0x8A, 0x80, 0x00, 0x00, 0x00,
        0x0F, 0x11, 0x89, 0x80, 0x00, 0x00, 0x00,
    };
    for (size_t i = 0; i < sizeof(expected); ++i) {
        if (found[i] != expected[i]) {
            if (g_verboseLog) {
                Log("ProjAspect hook: RVA 0x2129F8 byte mismatch at +0x%zX (got %02X expected %02X)\n",
                    i, found[i], expected[i]);
            }
            return false;
        }
    }

    constexpr int replaceLen = 14; // full 2-instruction copy
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp)
        return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Execute the original copy first:
    //   movups xmm1, [rdx+80h]
    //   movups [rcx+80h], xmm1
    for (int i = 0; i < replaceLen; ++i)
        code[pos++] = found[i];

    // Save volatile regs + flags
    code[pos++] = 0x9C;                          // pushfq
    code[pos++] = 0x50;                          // push rax
    code[pos++] = 0x51;                          // push rcx
    code[pos++] = 0x52;                          // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50;     // push r8
    code[pos++] = 0x41; code[pos++] = 0x51;     // push r9
    code[pos++] = 0x41; code[pos++] = 0x52;     // push r10
    code[pos++] = 0x41; code[pos++] = 0x53;     // push r11
    code[pos++] = 0x55;                         // push rbp

    // Save xmm0-xmm3 + xmm1 is included
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;

    // callback(dst=rcx, src=rdx)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnProjAspectCopyCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;

    // restore
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

CVR_HOOK_IF("ProjAspectCopy", ::cvr::hooks::Stage::Boot, 50, InstallProjAspectCopyHook, ProjAspectCopyWanted);
#endif
#endif
