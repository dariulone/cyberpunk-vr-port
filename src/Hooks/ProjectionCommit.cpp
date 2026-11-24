// ProjectionCommit -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order live here rather than in a boot function.
//
// Where the projection block is committed. Historically called the "unifix" site.

#include "Core/VrCoreShared.hpp"
#include "Utils/DebugGate.hpp"
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

// State private to this hook.
namespace {
volatile bool g_unifixEnableOverride = false;
}  // namespace

extern "C" void __fastcall OnUnifixHookCallback(void* projectionData, void* renderObj) {
    g_unifixHits++;
    if (renderObj) g_unifixRenderObj = reinterpret_cast<uintptr_t>(renderObj);
    if (!projectionData) return;

    __try {
        float* proj = reinterpret_cast<float*>(projectionData);
        bool nonZero = false;
        for (int i = 0; i < 9; ++i) {
            g_unifixProjDump[i] = proj[i];
            if (proj[i] != 0.0f) nonZero = true;
        }

        // Also read directly from render object (rbx+0x3030; 2.31: +0x21C0) — this is where
        // the game stores the ACTUAL projection during gameplay. The r13 source
        // may be a zeroed template; the real data is in the destination.
        float rbxProj[9] = {};
        bool rbxNonZero = false;
        if (renderObj) {
            float* dst = reinterpret_cast<float*>(g_unifixRenderObj + 0x3030);
            for (int i = 0; i < 9; ++i) {
                rbxProj[i] = dst[i];
                if (rbxProj[i] != 0.0f) rbxNonZero = true;
            }
        }

        static bool s_seenNonZeroRbx = false;
        bool shouldLog = false;
        if (rbxNonZero && !s_seenNonZeroRbx) { s_seenNonZeroRbx = true; shouldLog = true; }
        if (g_unifixHits <= 5) shouldLog = true;
        if ((g_unifixHits % 600) == 0 && cvr::RuntimeDiagnosticsEnabled()) shouldLog = true;

        if (shouldLog) {
            Log("Unifix: hits=%llu r13zero=%d rbxZero=%d\n",
                static_cast<unsigned long long>(g_unifixHits),
                nonZero ? 0 : 1,
                rbxNonZero ? 0 : 1);
            if (rbxNonZero) {
                Log("UnifixRender: rbx=%p p0-3=[%.6f %.6f %.6f %.6f] p4-7=[%.6f %.6f %.6f %.6f] fov=%.6f\n",
                    renderObj,
                    rbxProj[0], rbxProj[1], rbxProj[2], rbxProj[3],
                    rbxProj[4], rbxProj[5], rbxProj[6], rbxProj[7],
                    rbxProj[8]);
            }
        }

        if (g_unifixEnableOverride && rbxNonZero) {
            const float desiredFov = g_normalFovOverrideValue;
            float* dst = reinterpret_cast<float*>(g_unifixRenderObj + 0x3050);
            if (desiredFov > 1.0f && desiredFov < 179.0f && *dst != desiredFov) {
                *dst = desiredFov;
            }
        }
    }
    __except (EXCEPTION_EXECUTE_HANDLER) {
    }
}

bool InstallUnifixHook() {
    // AOB: movups xmm0,[r13]; movups [rbx+0x3030],xmm0
    // = 41 0F 10 45 00 | 0F 11 83 30 30 00 00  (12 bytes), in sub_14067B6AC.
    // 2.31 had mov rcx,rdi; mov rdx,rdi between the two (48 8B CF 48 8B D7, store to +0x21C0);
    // 2.5 dropped them, so the load itself (5 bytes) is the instruction replaced.
    const char* pattern = "\x41\x0F\x10\x45\x00\x0F\x11\x83\x30\x30\x00\x00";
    const char* mask = "xxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    // Replace 1 complete instruction (5 bytes): movups xmm0,[r13].
    // The store (movups, 7 bytes) stays untouched.
    constexpr int replaceLen = 5;
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- Trampoline: save, call callback, restore, execute originals, jump back ---

    // Push volatile GPRs + flags (9 * 8 = 72 bytes)
    code[pos++] = 0x9C;                          // pushfq
    code[pos++] = 0x50;                          // push rax
    code[pos++] = 0x51;                          // push rcx
    code[pos++] = 0x52;                          // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50;      // push r8
    code[pos++] = 0x41; code[pos++] = 0x51;      // push r9
    code[pos++] = 0x41; code[pos++] = 0x52;      // push r10
    code[pos++] = 0x41; code[pos++] = 0x53;      // push r11
    code[pos++] = 0x55;                          // push rbp

    // Save volatile xmm registers (4 * 16 = 64 bytes)
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp,40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;       // movups [rsp],xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h],xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h],xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h],xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;   // mov rbp,rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp,-10h (align)
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp,20h (shadow space)

    // rcx = r13 (projection data), rdx = rbx (render object)
    code[pos++] = 0x4C; code[pos++] = 0x89; code[pos++] = 0xE9;   // mov rcx, r13
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xDA;   // mov rdx, rbx

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnUnifixHookCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;                       // call rax

    // Restore xmm
    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;   // mov rsp,rbp
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;       // movups xmm0,[rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp,40h

    // Pop volatile GPRs (reverse order)
    code[pos++] = 0x5D;                          // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B;      // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A;      // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59;      // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58;      // pop r8
    code[pos++] = 0x5A;                          // pop rdx
    code[pos++] = 0x59;                          // pop rcx
    code[pos++] = 0x58;                          // pop rax
    code[pos++] = 0x9D;                          // popfq

    // Execute original 5 bytes:
    code[pos++] = 0x41; code[pos++] = 0x0F; code[pos++] = 0x10;   // movups xmm0, [r13]
    code[pos++] = 0x45; code[pos++] = 0x00;
    // (movups [rbx+0x3030],xmm0 at found+5 is NOT replaced — executes in-place)

    // Jump back to found + replaceLen
    code[pos++] = 0xE9;
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
    pos += 4;

    // Patch: JMP to trampoline + NOPs
    DWORD oldProtect;
    VirtualProtect(found, replaceLen, PAGE_EXECUTE_READWRITE, &oldProtect);
    found[0] = 0xE9;
    *reinterpret_cast<int32_t*>(found + 1) = static_cast<int32_t>(code - (found + 5));
    VirtualProtect(found, replaceLen, oldProtect, &oldProtect);
    FlushInstructionCache(GetCurrentProcess(), found, replaceLen);
    return true;
}
CVR_HOOK("ProjectionCommit", ::cvr::hooks::Stage::Boot, 45, InstallUnifixHook);
