#if !CVR_RUST_VRIK   // the Rust build registers this file's hook from rust/crates/cvr-render/src/hook_table.rs
// OnFootMoveXY -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order are declared here rather than in a boot function.
//
// On-foot planar movement input, so locomotion follows the chosen source rather than the
// game heading.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Camera/CameraState.hpp"   // CyberpunkVR_BodyYawRealignRad
#include "Runtimes/OpenXRManager.hpp"
#include "Hooks/LadderInput.hpp"
#include "Utils/DebugGate.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

#if CVR_RUST_VRIK
// THE HOOK IS RUST'S (rust/crates/cvr-rig/src/vrik/on_foot_move_xy.rs: the callback and the stub); the registration stays
// here, so the install order holds.
extern "C" bool cvr_hook_on_foot_move_xy_install();
CVR_HOOK("OnFootMoveXY", ::cvr::hooks::Stage::Boot, 78, cvr_hook_on_foot_move_xy_install);
#else
extern "C" void OnOnFootMoveXYCallback(void* moveStruct) {
    if (g_telemetry) {
        CVR_DIAGNOSTIC(++g_telemetry->moveXYHits);
        CVR_DIAGNOSTIC(g_telemetry->moveXYRsi=reinterpret_cast<uintptr_t>(moveStruct));
    }
    cvr::swimming::BindNativeInput(moveStruct);
    if(cvr::ladder::Active())return; // ladder MoveY is up/down, independent of gaze yaw
    const int src = g_liveControls.xrMovementSource;

    // Game means forward in the character's own frame. The native solver uses
    // that same frame by default; HMD/hand are explicit choices in the panel.
    if (src <= 0) return;
    if (g_menuModeValue != 0) return;
    if (!moveStruct) return;
    float* p = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(moveStruct) + 0x90);
    float x = p[0];
    float y = p[1];
    if (x == 0.0f && y == 0.0f) return;
    float yaw = 0.0f;
    switch (src) {
        case 1: yaw = OpenXRManager::Get().GetHmdYawRelToBody(); break;
        case 2: yaw = OpenXRManager::Get().GetHandYawRelToBody(0); break;
        case 3: yaw = OpenXRManager::Get().GetHandYawRelToBody(1); break;
        default: return;
    }
    // Convert a tracking-relative direction to the current native body frame.
    yaw -= BodyYawFollowOffset();
    float c = cosf(yaw);
    float s = sinf(yaw);
    p[0] = x * c - y * s;
    p[1] = x * s + y * c;
}

bool InstallOnFootMoveXYHook() {
    const char* pattern = "\xF3\x0F\x11\x86\x94\x00\x00\x00\xF3\x0F\x58\xCA";
    const char* mask = "xxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8; // movss [rsi+94h],xmm0
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // Original instruction first so the struct already has the new Y scalar.
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x86;
    code[pos++] = 0x94; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;

    // Save volatile state (flags, GPRs, xmm0-5) before calling the C++ callback,
    // so the game's following 'addss xmm1,xmm2' and registers survive.
    code[pos++] = 0x9C;                                   // pushfq
    code[pos++] = 0x50;                                   // push rax
    code[pos++] = 0x51;                                   // push rcx
    code[pos++] = 0x52;                                   // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50;              // push r8
    code[pos++] = 0x41; code[pos++] = 0x51;              // push r9
    code[pos++] = 0x41; code[pos++] = 0x52;              // push r10
    code[pos++] = 0x41; code[pos++] = 0x53;              // push r11
    code[pos++] = 0x55;                                   // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x60; // sub rsp,0x60
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;             // movups [rsp],xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // [rsp+10h],xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // xmm3
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x64; code[pos++] = 0x24; code[pos++] = 0x40; // xmm4
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x6C; code[pos++] = 0x24; code[pos++] = 0x50; // xmm5

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;             // mov rbp,rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp,-16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp,0x20 (shadow)

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xF1;             // mov rcx,rsi (arg0)
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnOnFootMoveXYCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;                                 // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;             // mov rsp,rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;             // movups xmm0,[rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x64; code[pos++] = 0x24; code[pos++] = 0x40;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x6C; code[pos++] = 0x24; code[pos++] = 0x50;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x60; // add rsp,0x60

    code[pos++] = 0x5D;                                   // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B;              // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A;              // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59;              // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58;              // pop r8
    code[pos++] = 0x5A;                                   // pop rdx
    code[pos++] = 0x59;                                   // pop rcx
    code[pos++] = 0x58;                                   // pop rax
    code[pos++] = 0x9D;                                   // popfq

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
CVR_HOOK("OnFootMoveXY", ::cvr::hooks::Stage::Boot, 78, InstallOnFootMoveXYHook);
#endif
#endif
