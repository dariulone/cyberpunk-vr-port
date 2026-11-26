#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Utils/DebugGate.hpp"
// BodyYawCensus -- WHEN the body's yaw is computed, relative to the animation pass that draws it.
//
// WHAT THIS SITE IS. `sub_140336390` is the function that computes the player's body yaw and stores
// it as a pure-yaw quaternion into its state at `+0x1D0` (position at `+0x1C0`, same fixed-point
// scale `dword_1431EEE78` the camera path uses). Found live, 2026-08-13: a hardware write watchpoint
// walked from the drawn body's world quaternion up through the propagation --
//   sub_1401D74FC SetWorldTransform  <- sub_140329460 (binding loop)  <- sub_141CA3720
//   sub_1401D8558 UpdateWorldTransforms <- sub_14068E1F8 <- sub_1401C9430 <- sub_140B53E3C
// -- and everything below this site only COPIES the value. See docs/vrik-map.md §6.
//
// WHY IT IS MEASURED HERE AND NOT ARGUED ABOUT. The body and the camera turn by the SAME number
// (measured: entity yaw == view heading to five digits), so the double on a fast mouse flick cannot
// be a second rotation -- it can only be a difference of INSTANTS: the pose was baked with the yaw
// of one frame while the view was drawn with the yaw of the next. That question has exactly two
// answers and they lead to opposite work:
//   * the yaw is computed BEFORE the animation batch -> the solve can read the fresh value and the
//     double is ours to remove;
//   * the yaw is computed AFTER it -> the pose of that frame is already consumed, and no bone write
//     can reach the frame being drawn (which is what the old root-pre-rotation experiment found).
// The instrument therefore reports the gap in milliseconds AND the disagreement in degrees against
// the yaw this site actually wrote -- the degrees do not depend on the CET push at all, so they
// close the one caveat the [bodyyaw] census still had.
//
// PATCH SAFETY. The site is patched the same way PatchCamera patches the camera write: the original
// instruction runs first, then a stub saves the volatile registers AND xmm0-xmm3 before calling into
// C++ -- this function passes floats in xmm registers, so a plain MinHook detour on its entry could
// clobber arguments. The 17-byte pattern was checked against the exe on disk: ONE match (the 8-byte
// write instruction alone matches twice, so the longer form is required).

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/AobScanner.hpp"
#include "Camera/CameraState.hpp"  // g_lastLocatePosFP: the rendered camera, this port's own

#include <windows.h>
#include <cstdint>
#include <cmath>

// ---- what the census publishes -----------------------------------------------------------------
//
// Written on a game job thread, read on the animation thread and in Present. Plain 8-byte aligned
// scalars: on x86-64 those loads and stores do not tear, and a census that is one sample stale is
// still a census. Nothing here is load-bearing for the pose.
// The exports (CyberpunkVR_DebugYawWriteMs, CyberpunkVR_EngineBodyYaw*, CyberpunkVR_DebugYawWrites*) are
// src/Hooks/BodyHooksState.cpp's (both builds). The engine yaw is LOAD-BEARING, not a census: the solve converts world to
// model with it (model space IS the entity frame; the camera heading lagged it a frame on every turn).
extern "C" {
#if !CVR_RUST_VRIK
extern double CyberpunkVR_DebugYawWriteMs;
#endif
extern float CyberpunkVR_EngineBodyYawZ,CyberpunkVR_EngineBodyYawW;
#if !CVR_RUST_VRIK
extern int CyberpunkVR_EngineBodyYawValid;
#endif
extern uint64_t CyberpunkVR_DebugYawWritesAll,CyberpunkVR_DebugYawWritesPlayer;
}

#if CVR_RUST_VRIK
// THE SITE IS RUST'S (rust/crates/cvr-rig/src/vrik/body_yaw_census.rs: the stub, the callback, the census); the
// registration stays here, so the install order holds.
#if !CVR_RUST_VRIK   // registered from rust/crates/cvr-render/src/hook_table.rs
CVR_HOOK("BodyYawCensus", ::cvr::hooks::Stage::Boot, 61, cvr_hook_body_yaw_census_install);
#endif
#else
// Defined in src/Hooks/BodyYawFollow.cpp.
extern "C" void BodyYawFollowTick(float engineZ, float engineW, const float* enginePos);

namespace {

double YawNowMs() {
    static LARGE_INTEGER s_freq = {};
    if (s_freq.QuadPart == 0) QueryPerformanceFrequency(&s_freq);
    if (s_freq.QuadPart == 0) return 0.0;
    LARGE_INTEGER t{};
    QueryPerformanceCounter(&t);
    return static_cast<double>(t.QuadPart) * 1000.0 / static_cast<double>(s_freq.QuadPart);
}

}  // namespace

// R15 is the movement component. RE proved S+0x90 is its owner. Compare that
// pointer with GetPlayer's actual instance: proximity to a leaning VR camera
// cannot reliably identify a player, and a retained pointer can survive a reload.
extern "C" void OnBodyYawWriteCallback(void* state) {
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugYawWritesAll);
    if (!cvr::roomscale::IsPlayerMovement(state)) return;
    cvr::roomscale::ObserveMovement(state);
    __try {
        const uint8_t* s = static_cast<const uint8_t*>(state);
        const int32_t* p = reinterpret_cast<const int32_t*>(s + 0x1C0);
        const float k = 1.0f / 131072.0f;
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugYawWritesPlayer);
        const float* q = reinterpret_cast<const float*>(s + 0x1D0);
        // A pure-yaw quaternion by construction here (x and y are zero -- verified live), so the
        // pair is all the solve needs. Published BEFORE the timestamp so a reader that sees a
        // fresh time always sees the matching angle.
        CyberpunkVR_EngineBodyYawZ = q[2];
        CyberpunkVR_EngineBodyYawW = q[3];
        CyberpunkVR_EngineBodyYawValid = 1;
        CyberpunkVR_DebugYawWriteMs = YawNowMs();
        // The follower writes the body from here, because the engine's own transform write fires only
        // when something changes (measured: 8 times a session) while this site is once per frame for
        // the player. It is handed the engine's yaw so it can set the root ABSOLUTELY and never
        // compound. See src/Hooks/BodyYawFollow.cpp.
        const float enginePos[3] = { static_cast<float>(p[0]) * k,
                                    static_cast<float>(p[1]) * k,
                                    static_cast<float>(p[2]) * k };
        BodyYawFollowTick(q[2], q[3], enginePos);
        // NOTHING IS APPLIED HERE, and that is a measured correction. state+0x1D0 looked exactly like
        // the body's rotation -- it is the only write to that field in this whole function -- and the
        // follower modified it for two builds. Result: the offset sat at -45 deg while the game's own
        // body heading ([141], read from the pre-write camera component) stayed at the engine yaw to
        // five digits. It is a copy nothing propagates. The real write is earlier in the same
        // function, 0x1403365FA -> sub_1401DC0E0, and the follower lives there now
        // (src/Hooks/BodyYawFollow.cpp). This site keeps its census role and nothing else.
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

namespace {

bool InstallBodyYawCensusHook() {
    // movups [r15+1D0h], xmm0   /   movss [r15+1E0h], xmm6
    const char* pattern = "\x41\x0F\x11\x87\xD0\x01\x00\x00\xF3\x41\x0F\x11\xB7\xE0\x01\x00\x00";
    const char* mask = "xxxxxxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8;   // just the movups; the jmp is 5 and the rest is padded with nops
    void* tramp = AllocateTrampoline(found, 256);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // The original instruction first, so the callback reads the value that was actually stored.
    code[pos++] = 0x41; code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x87;
    code[pos++] = 0xD0; code[pos++] = 0x01; code[pos++] = 0x00; code[pos++] = 0x00;

    // Save the volatile registers and xmm0-xmm3 (this function is float-heavy; see the note above).
    code[pos++] = 0x9C;                                     // pushfq
    code[pos++] = 0x50;                                     // push rax
    code[pos++] = 0x51;                                     // push rcx
    code[pos++] = 0x52;                                     // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50;                 // push r8
    code[pos++] = 0x41; code[pos++] = 0x51;                 // push r9
    code[pos++] = 0x41; code[pos++] = 0x52;                 // push r10
    code[pos++] = 0x41; code[pos++] = 0x53;                 // push r11
    code[pos++] = 0x55;                                     // push rbp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40;   // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24;   // movups [rsp], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5;                       // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0;   // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20;   // sub rsp, 20h

    code[pos++] = 0x4C; code[pos++] = 0x89; code[pos++] = 0xF9;                       // mov rcx, r15
    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnBodyYawWriteCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0;                                           // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC;                       // mov rsp, rbp
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24;   // movups xmm0, [rsp]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20;
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30;
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40;   // add rsp, 40h
    code[pos++] = 0x5D;                                     // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B;                 // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A;                 // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59;                 // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58;                 // pop r8
    code[pos++] = 0x5A;                                     // pop rdx
    code[pos++] = 0x59;                                     // pop rcx
    code[pos++] = 0x58;                                     // pop rax
    code[pos++] = 0x9D;                                     // popfq

    code[pos++] = 0xE9;                                     // jmp back past the replaced bytes
    *reinterpret_cast<int32_t*>(code + pos) =
        static_cast<int32_t>((found + replaceLen) - (code + pos + 4));
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

}  // namespace

CVR_HOOK("BodyYawCensus", ::cvr::hooks::Stage::Boot, 61, InstallBodyYawCensusHook);
#endif
#endif
