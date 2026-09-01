// OnFootDeltaHead -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order live here rather than in a boot function.
//
// The per-frame heading delta on foot: where snap turn and stick turning are injected.

#include "Core/VrCoreShared.hpp"
#include "Core/LiveControls.hpp"
#include "Camera/CameraState.hpp"   // CyberpunkVR_BodyYawFollow: the mirror set below
#include "Core/Telemetry.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"

#include <windows.h>
#include <cstdint>
#include <cstddef>

// The follow loop and the realign accumulator (src/Hooks/BodyYawFollow.cpp).
extern "C" float BodyYawFollowStep();
extern "C" void  BodyYawFollowRelease();
extern "C" double RecoilLastShotMs();
// Peak |heading delta| seen within 150 ms of a shot, degrees. Non-zero means the game kicks the
// camera through THIS channel, which is also the channel the body follower writes -- and then the
// sideways jerk on a shot is the weapon's own recoil, not the animation and not the follower.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugShotHeadingKickDeg = 0.0f;

extern "C" void __fastcall OnOnFootDeltaHeadCallback(float* deltaHead) {
    // The log printed "DeltaHead: hits=0" for this hook while it was running 72 times a second:
    // the telemetry field existed and nobody incremented it, left behind by an older trampoline
    // that wrote it inline. A counter that reads zero while the code runs is worse than no
    // counter -- it was about to send me hunting an installation failure that never happened.
    if (g_telemetry) {
        ++g_telemetry->deltaHeadHits;
        g_telemetry->deltaHeadRcx = reinterpret_cast<uintptr_t>(deltaHead);
    }
    if (!deltaHead) return;
    if (g_isInVehicle) {
        // MOUNTED: the follower issues nothing, so the realign it accumulated on foot must not be
        // left standing -- PatchCamera subtracts it from the CAR's heading and the drive ends up
        // that many degrees off the road. See BodyYawFollowRelease.
        BodyYawFollowRelease();
        return;
    }

    // Physical body rotation (F10 -> VRIK). OFF (default): no continuous body-yaw
    // tracking from the HMD -- only the discrete snap-turn is applied (classic heading).
    // ONE GATE for the whole feature. The plugin-side mirror is what the camera write and the pose
    // path test on their hot paths, and it is set from here so the two can never disagree.
    const bool bodyRot = g_liveControls.xrPhysicalBodyRotation != 0;
    CyberpunkVR_BodyYawFollow = bodyRot ? 1 : 0;

    // MEASUREMENT ONLY. Cancelling the weapon's camera kick HERE was built and taken out again on the
    // user's call: hiding it in the view leaves the game still applying it to the character, and the
    // recovery it then plays back has to be hidden too. The kick is removed at its source instead --
    // the weapons' own recoil stats. This peak-hold stays because it is what found the kick: the value
    // is read BEFORE anything of ours is added, so it is the game's own contribution and nothing else.
    {
        static LARGE_INTEGER s_f = {};
        if (s_f.QuadPart == 0) QueryPerformanceFrequency(&s_f);
        LARGE_INTEGER qn{}; QueryPerformanceCounter(&qn);
        const double nowMs = s_f.QuadPart ? (double)qn.QuadPart * 1000.0 / (double)s_f.QuadPart : 0.0;
        const double shot = RecoilLastShotMs();
        if (shot > 0.0 && (nowMs - shot) >= 0.0 && (nowMs - shot) < 150.0) {
            for (int k = 0; k < 4; ++k) {
                const float a = deltaHead[k] < 0.0f ? -deltaHead[k] : deltaHead[k];
                if (a > CyberpunkVR_DebugShotHeadingKickDeg) CyberpunkVR_DebugShotHeadingKickDeg = a;
            }
        }
    }

    int idx = GetSnapTurnYawIndex();
    if (idx < 0) idx = 0;
    if (idx > 3) idx = 3;

    // 2. Aggiungi lo snap yaw se presente
    const LONG bits = InterlockedExchange(&g_pendingSnapYawDeltaBits, 0);
    float snap = 0.0f;
    if (bits != 0) {
        memcpy(&snap, &bits, sizeof(float));
        // SNAP EVENT PUBLISH (snap_trace-driven design). This callback runs at TICK
        // stage, BEFORE the same tick's animation pass -- i.e. BEFORE the VRIK solve
        // that will otherwise consume a one-locate-old (pre-snap) view packet. Publish
        // the yaw delta (radians, [146]) + a bump counter ([147]): the plugin's packet
        // latch rotates the packet by exactly this delta ONCE, so the snap-tick solve
        // matches the heading the NEXT locate provably renders (trace: inject at
        // hits=N, view turned at N+1). No entity comparison -- snap_trace showed the
        // puppet yaw deviates from the heading by up to ~10deg PERMANENTLY
        // (turn-in-place deadband), which made the old comparator fire every tick.
        //
        // (A "suppress while sprinting" variant lived here briefly — built on a sprint
        // turn-rate-limit hypothesis. The snapdiag log KILLED it: [141] jumps the full
        // snap delta in ONE frame during sprint too, so suppression only guaranteed a
        // stale-packet solve on every sprint snap. Events publish unconditionally again;
        // the true sprint-only ghost is a velocity-amplified base-staleness, hunted via
        // the mountYaw diag.)
        if (float* shSnap = GetShotShared()) {
            shSnap[146] = snap * 0.01745329252f;   // degrees -> radians
            shSnap[148] = shSnap[141];             // PRE-snap heading: the plugin only
                                                   // rotates a packet that still shows
                                                   // this heading (double-apply guard,
                                                   // robust to tick-internal ordering)
            shSnap[150] = shSnap[99];              // tick stamp: the plugin DEFERS the
                                                   // packet rotation past this tick (the
                                                   // view holds; puppet turns next tick)
            shSnap[147] = shSnap[147] + 1.0f;      // event counter (plugin consumes)
            Log("[snap-pub] ms=%llu ctr=%.0f delta=%.2fdeg preHeading=%.4f ack=%.0f tick=%.0f\n",
                (unsigned long long)GetTickCount64(), shSnap[147], snap, shSnap[148], shSnap[149], shSnap[150]);
        }
    }

    // bodyRot OFF (default) -> classic snap-turn only: the heading never tracks the
    // head; the camera composes heading * FULL HMD, so a head turn moves ONLY the view.
    if (!bodyRot) {
        // And release the accumulator: the guard inside BodyYawFollowStep cannot, because this
        // return is upstream of the only call to it.
        BodyYawFollowRelease();
        deltaHead[idx] += snap;
        return;
    }

    // BODY REALIGN (physical body rotation ON), on foot, armed and unarmed alike.
    //
    // THE GAME TURNS THE CHARACTER; WE ONLY ASK IT TO. This injects into the engine's own
    // per-frame heading delta -- the same channel the snap turn uses -- so the entity yaw,
    // and with it the drawn mesh, the collision, the aim and the movement direction, all
    // move together because the engine moved them.
    //
    // Writing the component transforms instead was tried in full. It reached the drawn
    // body (measured: all 102 of the player's records carried the angle), and the hand
    // symptom it was blamed for turned out to be an unrelated frame mismatch in
    // LocateCamera -- see src/Hooks/BodyYawFollow.cpp, which carries the corrected
    // account. What rules that route out is the gameplay half: aim, movement, cover and
    // the collision capsule come from this heading, not from those transforms.
    //
    // THE RECENTER BASE IS NOT TOUCHED. The version of this that shipped before called
    // RotateBaseYaw(step) to cancel the view, which is right in the algebra and wrong in
    // the ORDER: the heading changes here, inside the game tick, while the base only takes
    // effect on the next XR cycle, so for one frame the view swings by the whole step --
    // the camera drift this feature was always reported to have. The cancellation now
    // happens on our side of the same frame, in the camera write and in the head-offset
    // recipe, both of which compose from (engine yaw - CyberpunkVR_BodyYawRealignRad).
    // Recentring therefore keeps working exactly as it did with the feature off.
    //
    // The loop, the cone and the realign accumulator live in src/Hooks/BodyYawFollow.cpp;
    // this file is only the door into the engine's heading.
    if (g_menuModeValue == 0) {
        const float step = BodyYawFollowStep();         // radians, 0 when inside the cone
        if (step != 0.0f) deltaHead[idx] += step * 57.2957795f;
    }

    if (snap != 0.0f) deltaHead[idx] += snap;
}

bool InstallOnFootDeltaHeadHook() {
    const char* pattern = "\xF3\x0F\x10\x81\x9C\x00\x00\x00\x48\x8D\x54\x24\x30";
    const char* mask = "xxxxxxxxxxxxx";
    uint8_t* found = static_cast<uint8_t*>(FindPattern("Cyberpunk2077.exe", pattern, mask));
    if (!found) return false;

    constexpr int replaceLen = 8; // movss xmm0,[rcx+9Ch]
    void* tramp = AllocateTrampoline(found, 512);
    if (!tramp) return false;

    uint8_t* code = static_cast<uint8_t*>(tramp);
    int pos = 0;

    // --- CALL C++ CALLBACK ---
    code[pos++] = 0x9C; // pushfq
    code[pos++] = 0x50; // push rax
    code[pos++] = 0x51; // push rcx
    code[pos++] = 0x52; // push rdx
    code[pos++] = 0x41; code[pos++] = 0x50; // push r8
    code[pos++] = 0x41; code[pos++] = 0x51; // push r9
    code[pos++] = 0x41; code[pos++] = 0x52; // push r10
    code[pos++] = 0x41; code[pos++] = 0x53; // push r11
    code[pos++] = 0x55; // push rbp

    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x40; // sub rsp, 40h
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x04; code[pos++] = 0x24; // movups [rsp+00h], xmm0
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups [rsp+10h], xmm1
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups [rsp+20h], xmm2
    code[pos++] = 0x0F; code[pos++] = 0x11; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups [rsp+30h], xmm3

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xE5; // mov rbp, rsp
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xE4; code[pos++] = 0xF0; // and rsp, -16
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xEC; code[pos++] = 0x20; // sub rsp, 20h

    // Set arg1 (rcx) = rcx + 9Ch
    code[pos++] = 0x48; code[pos++] = 0x8D; code[pos++] = 0x89; code[pos++] = 0x9C; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00; // lea rcx, [rcx+9Ch]

    WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(OnOnFootDeltaHeadCallback));
    code[pos++] = 0xFF; code[pos++] = 0xD0; // call rax

    code[pos++] = 0x48; code[pos++] = 0x89; code[pos++] = 0xEC; // mov rsp, rbp

    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x04; code[pos++] = 0x24; // movups xmm0, [rsp+00h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x4C; code[pos++] = 0x24; code[pos++] = 0x10; // movups xmm1, [rsp+10h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x54; code[pos++] = 0x24; code[pos++] = 0x20; // movups xmm2, [rsp+20h]
    code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x5C; code[pos++] = 0x24; code[pos++] = 0x30; // movups xmm3, [rsp+30h]
    code[pos++] = 0x48; code[pos++] = 0x83; code[pos++] = 0xC4; code[pos++] = 0x40; // add rsp, 40h

    code[pos++] = 0x5D; // pop rbp
    code[pos++] = 0x41; code[pos++] = 0x5B; // pop r11
    code[pos++] = 0x41; code[pos++] = 0x5A; // pop r10
    code[pos++] = 0x41; code[pos++] = 0x59; // pop r9
    code[pos++] = 0x41; code[pos++] = 0x58; // pop r8
    code[pos++] = 0x5A; // pop rdx
    code[pos++] = 0x59; // pop rcx
    code[pos++] = 0x58; // pop rax
    code[pos++] = 0x9D; // popfq

    // Original instruction: movss xmm0, [rcx+9Ch]
    code[pos++] = 0xF3; code[pos++] = 0x0F; code[pos++] = 0x10; code[pos++] = 0x81;
    code[pos++] = 0x9C; code[pos++] = 0x00; code[pos++] = 0x00; code[pos++] = 0x00;

    // jmp back
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
CVR_HOOK("OnFootDeltaHead", ::cvr::hooks::Stage::Boot, 76, InstallOnFootDeltaHeadHook);
