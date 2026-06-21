#pragma once
// ============================================================================
// Weapon-aim native hook (CyberpunkVRPort) -- "bullet from the weapon".
//
// Redirect the player's shot direction from the camera (HMD) to the weapon
// barrel for all weapons in VR. No script lever exists, so the logic hooks the
// native shot-direction path, gates it by callsite, rewrites the temporary aim
// buffer, calls the original, then restores the caller-owned data.
// ============================================================================
#include <windows.h>
#include <tlhelp32.h>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <intrin.h>
#include <MinHook.h>

// ============================================================================
// HARDWARE-BREAKPOINT ACCESS TRACER. To find WHICH function reads the camera for
// the shot (the bullet direction is computed upstream and baked -- bracketing the
// camera at the shot-caller did nothing). Sets a HW breakpoint (DR0) on read/write
// of a chosen camera field on ALL threads; a VEH captures the RIP (as RVA) of every
// accessor into a unique list. Fire a shot, then dump the list -> the RVAs that read
// the camera; the shot's reader is among them. Bounded (stops after N unique hits).
// ============================================================================
extern volatile uintptr_t g_exeBaseTrace;
extern volatile uint32_t  g_traceRvas[128];
extern volatile uint32_t  g_traceRvaCounts[128];
extern volatile int       g_traceCount;
extern volatile uint64_t  g_traceHits;
extern volatile uintptr_t g_traceAddr;   // watched address
extern volatile int       g_traceActive;
extern volatile int       g_traceGated;  // 1 = only record while g_shotInProgress
extern volatile int       g_shotInProgress;
static PVOID s_traceVeh = nullptr;

static inline LONG CALLBACK Wa_TraceVeh(EXCEPTION_POINTERS* ep) {
    if (ep->ExceptionRecord->ExceptionCode == EXCEPTION_SINGLE_STEP) {
        // DR6 low bits indicate which DRx fired; we only use DR0.
        const uintptr_t rip = ep->ContextRecord->Rip;
        const uintptr_t base = g_exeBaseTrace;
        // Shot window: g_shotInProgress is set TIGHTLY around the per-round TargetHelper
        // call (=2) and the PlayerShotCaller bracket (=1). Only record reads inside it.
        const bool record = (!g_traceGated) || (g_shotInProgress != 0);
        if (record && base && rip >= base && rip < base + 0x10000000) {
            const uint32_t rva = static_cast<uint32_t>(rip - base);
            ++g_traceHits;
            // insert unique + per-RVA count (low count = per-shot reader, high = per-frame render)
            int n = g_traceCount;
            bool seen = false;
            for (int i = 0; i < n && i < 128; ++i) if (g_traceRvas[i] == rva) { ++g_traceRvaCounts[i]; seen = true; break; }
            if (!seen && n < 128) { g_traceRvas[n] = rva; g_traceRvaCounts[n] = 1; g_traceCount = n + 1; }
        }
        ep->ContextRecord->Dr6 = 0;
        // Re-arm: keep RF set so we don't immediately re-trap the same instr.
        ep->ContextRecord->EFlags |= 0x10000; // RF
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

extern volatile int g_traceWriteOnly;  // 1 = DR0 watches WRITE only, 0 = read/write
static inline void Wa_SetDr0AllThreads(uintptr_t addr, bool enable) {
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (snap == INVALID_HANDLE_VALUE) return;
    THREADENTRY32 te; te.dwSize = sizeof(te);
    const DWORD pid = GetCurrentProcessId();
    const DWORD self = GetCurrentThreadId();
    if (Thread32First(snap, &te)) {
        do {
            if (te.th32OwnerProcessID != pid) continue;
            if (te.th32ThreadID == self) continue;
            HANDLE th = OpenThread(THREAD_GET_CONTEXT | THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME, FALSE, te.th32ThreadID);
            if (!th) continue;
            SuspendThread(th);
            CONTEXT ctx; ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
            if (GetThreadContext(th, &ctx)) {
                if (enable) {
                    ctx.Dr0 = addr;
                    // DR7: L0=1 (bit0); RW0 = 01 (write-only) or 11 (read/write); LEN0=11 (8 bytes)
                    const uint64_t rw = g_traceWriteOnly ? 0x1ull : 0x3ull;
                    ctx.Dr7 = (ctx.Dr7 & ~0xF0003ull) | 0x1ull | (rw << 16) | (0x3ull << 18);
                } else {
                    ctx.Dr0 = 0;
                    ctx.Dr7 &= ~0xF0003ull;
                }
                ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
                SetThreadContext(th, &ctx);
            }
            ResumeThread(th);
            CloseHandle(th);
        } while (Thread32Next(snap, &te));
    }
    CloseHandle(snap);
}

inline void Wa_StartTrace(uintptr_t addr, int gated, int writeOnly) {
    if (g_traceActive) return;
    g_exeBaseTrace = reinterpret_cast<uintptr_t>(GetModuleHandleA("Cyberpunk2077.exe"));
    g_traceCount = 0; g_traceHits = 0; g_traceAddr = addr; g_traceGated = gated; g_traceWriteOnly = writeOnly;
    if (!s_traceVeh) s_traceVeh = AddVectoredExceptionHandler(1, &Wa_TraceVeh);
    Wa_SetDr0AllThreads(addr, true);
    g_traceActive = 1;
}
inline void Wa_StopTrace() {
    if (!g_traceActive) return;
    if (g_traceAddr) Wa_SetDr0AllThreads(g_traceAddr, false);
    g_traceActive = 0;
}

// --- counters / instrumentation (defined in main.cpp) ----------------------
extern volatile uint64_t g_waProjCalls;        // projectile copy hook invocations
extern volatile uint64_t g_waProjMutated;      // projectile shoot events we redirected
extern volatile int      g_waProjCtrl;         // 1 = redirect startVelocity to controller fwd (g_fireInShot-gated)
extern volatile int      g_waProjNeg;          // flip sign
extern volatile int      g_waProjUnguide;      // 1 = clear smartGunIsProjectileGuided (defeat homing)
extern volatile float    g_waProjRange;        // targetPosition distance along controller (def 1000)
extern volatile int      g_waProjAlways;       // 1 = bypass the g_fireInShot gate
extern volatile int      g_waProjOriginRow;    // localToWorld row (0..3) used as the world muzzle origin
extern volatile uint32_t g_waProjLastRetRva;   // caller return RVA for the last copy call
extern volatile uint32_t g_waProjRejectReason; // bitmask: 1 ctrlOff, 2 gate, 4 nullRcx, 8 noShared, 16 targetLocal, 32 badCtrl
extern volatile uint32_t g_waProjGateRva;      // only mutate this copy caller; 0 = all callers
extern volatile uint64_t g_waProjRet36F9FF;    // queue/state copy spam path
extern volatile uint64_t g_waProjRet36FD7C;    // manager insert/update path
extern volatile uint64_t g_waProjRet4E5109;    // active projectile consumer path
extern volatile uint64_t g_waProjRet4E615F;    // alternate projectile consumer path
extern volatile float    g_shotOrigin[3];      // player/camera WORLD position (pumped from CET)
extern volatile float    g_projDump[64];       // diag: l2w(0-23) tgtPre(24-26) guided(27) origin(28-30) ctrlDir(31-33) svPost(34-36) tgtPost(37-39) reason/control(40+)
extern volatile uint64_t g_waTargetCalls;      // TargetHelper hook invocations (all callers)
extern volatile uint64_t g_waTargetFromShot;   // ...called from the shot wrapper (ret 0x46F017)
extern volatile uint64_t g_waClassifyCalls;    // classify hook invocations (all)
extern volatile uint64_t g_waClassifyFromShot; // ...called from the shot-vector processor
extern volatile uint64_t g_waRedirects;        // shot directions we actually rewrote
extern volatile int      g_waInstalled;
extern volatile float    g_waTargetOrigin[4];  // last shot origin (source+0x50)
extern volatile float    g_waTargetDir[4];     // last original aim delta (target-origin)
extern volatile float    g_xfTestYaw;          // shared sanity-yaw (also used by TargetHelper test)
extern volatile uint64_t g_shotTick;           // tick of last shot-frame flag set
extern volatile int      g_xfTestPlane;        // 0=around Z(yaw) 1=around Y 2=around X
extern volatile uint32_t g_waLastRetRva;       // last TargetHelper caller return rva (diag)
extern volatile int      g_shotInProgress;
extern volatile int      g_fireInShot;         // nest-safe shot-window counter (trace gate)
extern volatile uintptr_t g_waExeBase;         // Cyberpunk2077.exe base

// --- TargetHelper clean controller-redirect (target = origin + ctrlFwd*100) ---
extern volatile int      g_waTgtCtrl;  // 1 = redirect TargetHelper target to controller forward
extern volatile int      g_waTgtNeg;   // flip sign
extern volatile uint64_t g_waTgtOvr;   // override count (confirms it applied)

// --- redirect control (set from CET via SetVRWeaponAim) --------------------
extern volatile int      g_waEnable;   // 0 = instrument only; 1 = redirect to g_waFwd
extern volatile int      g_waMode;     // bit0: also move shot origin to g_waPos (muzzle)
extern volatile float    g_waFwd[3];   // unit barrel forward, WORLD space
extern volatile float    g_waPos[3];   // weapon/muzzle WORLD position
extern volatile float    g_waGateDist; // (unused by classify; kept for compat)
extern volatile uint32_t g_waFwdSeq;

// ============================================================================
// CROSSHAIR-AIM UPDATE HOOK @0x4B9D84.
// This function runs per frame: it reads the crosshair cache = [X+0x30CC8],
// pulls position@cache+0x350 and orientation/direction@cache+0x370, and caches copies
// into X+0x311F0 / X+0x31200. GetDefaultCrosshairData reads the same cache+0x350/0x370.
// The bullet aim IS this crosshair direction (every shot fn we patched was a consumer of
// it). We hook this updater, capture cache via this(rcx)+0x30CC8, and BEFORE the original
// rewrite cache+0x370 to the weapon forward (and optionally +0x350 to the muzzle), so the
// engine's own copy + every later reader (the bullet) sees the barrel direction.
// this=rcx is rock-solid (it's the method's this), so cache capture can't fault.
// ============================================================================
static constexpr uintptr_t kWaXhUpdOffset = 0x4B9D84;
extern volatile uint64_t g_waXhCalls;
extern volatile uint64_t g_waXhMutated;
extern volatile int      g_waXhSnapped;
extern volatile float    g_waXhPos[4];   // captured cache+0x350 (pre-write)
extern volatile float    g_waXhDir[4];   // captured cache+0x370 (pre-write)
typedef void* (*WaXhUpd_t)(void*, void*, void*, void*);
static WaXhUpd_t OrigWaXhUpd = nullptr;

extern "C" inline void* Hooked_WaXhUpd(void* rcx, void* rdx, void* r8, void* r9) {
    ++g_waXhCalls;
    if (rcx) {
        __try {
            void* cache = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(rcx) + 0x30CC8);
            if (cache) {
                float* pos = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(cache) + 0x350);
                float* dir = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(cache) + 0x370);
                // One-shot snapshot of the original cache fields (to learn the format).
                if (!g_waXhSnapped) {
                    for (int i = 0; i < 4; ++i) { g_waXhPos[i] = pos[i]; g_waXhDir[i] = dir[i]; }
                    g_waXhSnapped = 1;
                }
                if (false /* DISABLED 2026-06-15: crosshair-cache dir = UI/screen-space, dead lever */) {
                    const float fx = g_waFwd[0], fy = g_waFwd[1], fz = g_waFwd[2];
                    const float fl = fx*fx + fy*fy + fz*fz;
                    if (fl > 0.25f) {
                        const float inv = 1.0f / std::sqrt(fl);
                        dir[0] = fx*inv; dir[1] = fy*inv; dir[2] = fz*inv; // dir[3] left as-is
                        if (g_waMode & 1) { pos[0]=g_waPos[0]; pos[1]=g_waPos[1]; pos[2]=g_waPos[2]; }
                        ++g_waXhMutated;
                    }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return OrigWaXhUpd(rcx, rdx, r8, r9);
}

// ============================================================================
// HEADING-DECOUPLE HOOK @0x4E8A1C -- aim/view split experiment.
// This function applies a heading OFFSET (aim relative to view): it gates on
// [camObj+0x4B8]>0 && byte[camObj+0x474]!=0, then uses heading offsets [camObj+0x4E4]
// (yaw) / [camObj+0x4E8] (pitch) on top of the view to build the aim/heading point that
// the SHOT follows.
// We hook it, capture camObj, and (when g_waHeadForce) force the flags + write a heading
// offset -> the bullet should shift while the VR view (HMD via LocateCamera) stays put.
// VERIFY step: static g_waHeadYaw/Pitch (tuned from overlay). Then feed controller delta.
// ============================================================================
static constexpr uintptr_t kWaHeadOffset = 0x4E8A1C;
extern volatile uint64_t  g_waHeadCalls;
extern volatile uintptr_t g_waHeadObj;     // captured camObj (this)
extern volatile int       g_waHeadForce;   // 1 = force flags + write offset
extern volatile float     g_waHeadYaw;     // heading yaw offset to write @+0x4E4
extern volatile float     g_waHeadPitch;   // heading pitch offset to write @+0x4E8
extern volatile float     g_waHeadOrig4E4; // captured original values (diag)
extern volatile float     g_waHeadOrig4E8;
extern volatile float     g_waHeadVal4B8;
extern volatile int       g_waHeadFlag474;
typedef uintptr_t (*WaHeadFn)(void*, void*, void*, void*);
static WaHeadFn OrigWaHead = nullptr;

extern "C" inline uintptr_t Hooked_WaHead(void* rcx, void* rdx, void* r8, void* r9) {
    ++g_waHeadCalls;
    if (rcx) {
        g_waHeadObj = reinterpret_cast<uintptr_t>(rcx);
        __try {
            uint8_t* o = reinterpret_cast<uint8_t*>(rcx);
            g_waHeadOrig4E4 = *reinterpret_cast<float*>(o + 0x4E4);
            g_waHeadOrig4E8 = *reinterpret_cast<float*>(o + 0x4E8);
            g_waHeadVal4B8  = *reinterpret_cast<float*>(o + 0x4B8);
            g_waHeadFlag474 = *reinterpret_cast<uint8_t*>(o + 0x474);
            if (g_waHeadForce) {
                // Gate: if [+0x4B8] > 0 the function
                // EARLY-RETURNS (no heading). So force [+0x4B8] = 0 to KEEP the gate OPEN
                // (previous 1.0 closed it -> that's why yaw did nothing). Then [+0x474]!=0,
                // and abs(offset) must exceed the threshold to apply -> write the offsets.
                *reinterpret_cast<float*>(o + 0x4B8) = 0.0f;
                *reinterpret_cast<uint8_t*>(o + 0x474) = 1;
                *reinterpret_cast<float*>(o + 0x4E4) = g_waHeadYaw;
                *reinterpret_cast<float*>(o + 0x4E8) = g_waHeadPitch;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return OrigWaHead ? OrigWaHead(rcx, rdx, r8, r9) : 0;
}

// Hook RVAs for this game build.
static constexpr uintptr_t kWaProjOffset           = 0x28D4B8;
static constexpr uintptr_t kWaTargetHelperOffset   = 0x46F774;
static constexpr uintptr_t kWaTargetShotRet        = 0x46F017; // TargetHelper call from the player-shot wrapper
static constexpr uintptr_t kWaShotClassifyOffset   = 0x291FDE0;
static constexpr uintptr_t kWaClassifyRetProc      = 0x292279E; // return into ShotVectorProcessor
static constexpr uintptr_t kWaClassifyRetAltProc   = 0x2923292;

typedef void* (*WaProjFunc_t)(void*, void*, void*, void*);
typedef void* (*WaTargetFunc_t)(void*, void*, void*, void*, void*, void*);
typedef uint32_t (*WaClassifyFunc_t)(void*, void*);
static WaProjFunc_t     OrigWaProj      = nullptr;
static WaTargetFunc_t   OrigWaTarget    = nullptr;
static WaClassifyFunc_t OrigWaClassify  = nullptr;

// --- shot-pipeline instrumentation (counter-only, to find the real bullet path) ---
extern volatile uint64_t g_waCandA, g_waCandB, g_waSVP, g_waSFVW;
static constexpr uintptr_t kWaCandAOffset = 0x291D9C8;
static constexpr uintptr_t kWaCandBOffset = 0x291DD54;
static constexpr uintptr_t kWaSVPOffset   = 0x292263C; // ShotVectorProcessor
static constexpr uintptr_t kWaSFVWOffset  = 0x29216D0; // ShotFinalVectorWrite
typedef uintptr_t (*WaCand_t)(void*, void*, void*, void*);
typedef void (*WaSVP_t)(void*, void*, void*);
typedef void (*WaSFVW_t)(void*, int, uint32_t, void*, void*);
static WaCand_t OrigWaCandA = nullptr;
static WaCand_t OrigWaCandB = nullptr;
static WaSVP_t  OrigWaSVP   = nullptr;
static WaSFVW_t OrigWaSFVW  = nullptr;
extern "C" inline uintptr_t Hooked_WaCandA(void* a, void* b, void* c, void* d) { ++g_waCandA; return OrigWaCandA ? OrigWaCandA(a,b,c,d) : 0; }
extern "C" inline uintptr_t Hooked_WaCandB(void* a, void* b, void* c, void* d) { ++g_waCandB; return OrigWaCandB ? OrigWaCandB(a,b,c,d) : 0; }
extern "C" inline void Hooked_WaSVP(void* a, void* b, void* c) { ++g_waSVP; if (OrigWaSVP) OrigWaSVP(a,b,c); }
extern "C" inline void Hooked_WaSFVW(void* a, int b, uint32_t c, void* d, void* e) { ++g_waSFVW; if (OrigWaSFVW) OrigWaSFVW(a,b,c,d,e); }

// Projectile shoot path: copies a gameprojectileShootEvent (dest=rcx).
// Layout (RED4ext SDK, verified): startPoint@0xF0, startVelocity@0x100 (= direction*speed),
// weaponVelocity@0x110. Redirecting these (+0xF0/+0x100/
// +0x110) and projectiles "land where the mouse points". We rewrite startVelocity to the
// weapon forward (preserving speed) and optionally startPoint to the muzzle. Gated by
// distance(startPoint, published muzzle pos) so only the player's own shot is touched
// (NPC projectiles originate far away). HOT fn -> work only when enabled; NO VirtualQuery.
extern "C" inline void* Hooked_WaProj(void* rcx, void* rdx, void* r8, void* r9) {
    void* result = OrigWaProj(rcx, rdx, r8, r9); // performs the copy; rcx = dest event
    ++g_waProjCalls;
    const uintptr_t ret = reinterpret_cast<uintptr_t>(_ReturnAddress());
    const uint32_t retRva = (g_waExeBase && ret >= g_waExeBase) ? static_cast<uint32_t>(ret - g_waExeBase) : 0;
    g_waProjLastRetRva = retRva;
    if (retRva == 0x36F9FF) ++g_waProjRet36F9FF;
    else if (retRva == 0x36FD7C) ++g_waProjRet36FD7C;
    else if (retRva == 0x4E5109) ++g_waProjRet4E5109;
    else if (retRva == 0x4E615F) ++g_waProjRet4E615F;

    uint32_t reason = 0;
    if (!g_waProjCtrl) reason |= 1u;
    if (!(g_waProjAlways || g_fireInShot > 0)) reason |= 2u;
    if (!rcx) reason |= 4u;
    if (!g_pSharedHands) reason |= 8u;
    if (g_waProjGateRva && retRva != g_waProjGateRva) reason |= 128u;

    float cfx = 0.0f, cfy = 0.0f, cfz = 0.0f, cl = 0.0f;
    if (g_pSharedHands) {
        cfx = g_pSharedHands[60]; cfy = g_pSharedHands[61]; cfz = g_pSharedHands[62];
        cl = cfx*cfx + cfy*cfy + cfz*cfz;
        if (!(std::isfinite(cl) && cl > 0.1f)) reason |= 32u;
    }

    if (rcx) {
        __try {
            uint8_t* ev = reinterpret_cast<uint8_t*>(rcx);
            float* tp = reinterpret_cast<float*>(ev + 0x120);
            bool* guided = reinterpret_cast<bool*>(ev + 0x154);
            const float tlen2 = tp[0]*tp[0]+tp[1]*tp[1]+tp[2]*tp[2];
            if (!(std::isfinite(tlen2) && tlen2 > 2500.0f)) reason |= 16u;
            for (int i = 0; i < 24; ++i) g_projDump[i] = reinterpret_cast<float*>(ev + 0xB0)[i];
            g_projDump[24]=tp[0]; g_projDump[25]=tp[1]; g_projDump[26]=tp[2];
            g_projDump[27]=*guided ? 1.0f : 0.0f;
            float* sv0 = reinterpret_cast<float*>(ev + 0x100);
            g_projDump[45]=std::sqrt(sv0[0]*sv0[0]+sv0[1]*sv0[1]+sv0[2]*sv0[2]);
            g_projDump[46]=tlen2;
        } __except (EXCEPTION_EXECUTE_HANDLER) { reason |= 64u; }
    }
    g_waProjRejectReason = reason;
    g_projDump[40]=static_cast<float>(reason);
    g_projDump[41]=static_cast<float>(g_waProjCtrl);
    g_projDump[42]=static_cast<float>(g_waProjAlways);
    g_projDump[43]=static_cast<float>(g_fireInShot);
    g_projDump[44]=cl;
    g_projDump[47]=static_cast<float>(g_waProjGateRva);
    // Controller override: the player's bullet reaches this projectile event copy path.
    // Rewrite startVelocity(+0x100)
    // to the dxgi controller forward (shared[60..62]) preserving speed, gated to the player-shot
    // window (g_fireInShot, reliable -- replaces the old fragile distance gate). g_waProjNeg flips.
    // gate: normally the player-shot window (g_fireInShot); g_waProjAlways=1 bypasses it (with the
    // mod, the converted bullet may spawn outside our shot bracket). Diagnostics captured for UI.
    if ((reason & (1u|2u|4u|8u|32u|128u)) == 0) {
        __try {
            uint8_t* ev = reinterpret_cast<uint8_t*>(rcx);
            float* l2w = reinterpret_cast<float*>(ev + 0xB0);  // localToWorld Matrix (4x4)
            float* sv = reinterpret_cast<float*>(ev + 0x100);  // startVelocity (Vector4)
            float* wv = reinterpret_cast<float*>(ev + 0x110);  // weaponVelocity (Vector4)
            float* tp = reinterpret_cast<float*>(ev + 0x120);  // WeaponParams.targetPosition (WORLD)
            bool*  guided = reinterpret_cast<bool*>(ev + 0x154); // smartGunIsProjectileGuided
            const float spd0 = std::sqrt(sv[0]*sv[0]+sv[1]*sv[1]+sv[2]*sv[2]);
            const float sgn = (g_waProjNeg?-1.0f:1.0f);
            const float u = sgn/std::sqrt(cl);
            const float ux=cfx*u, uy=cfy*u, uz=cfz*u;       // unit controller forward (WORLD)
            const float spd = (std::isfinite(spd0) && spd0>0.01f) ? spd0 : 100.0f;
            // 1) startVelocity AND weaponVelocity = controller dir * speed (world ballistic vel)
            sv[0]=ux*spd; sv[1]=uy*spd; sv[2]=uz*spd;
            const float wvl = std::sqrt(wv[0]*wv[0]+wv[1]*wv[1]+wv[2]*wv[2]);
            if (std::isfinite(wvl) && wvl > 0.01f) { wv[0]=ux*wvl; wv[1]=uy*wvl; wv[2]=uz*wvl; }
            // 2) targetPosition = WORLD origin + controller dir * range. The old world-target
            // filter was diagnostic-only: converted projectile bullets can use local targets here.
            float ox, oy, oz;
            if (g_shotOrigin[0]!=0.0f || g_shotOrigin[1]!=0.0f || g_shotOrigin[2]!=0.0f) {
                ox=g_shotOrigin[0]; oy=g_shotOrigin[1]; oz=g_shotOrigin[2];
            } else {
                int row = (g_waProjOriginRow >= 0 && g_waProjOriginRow <= 3) ? g_waProjOriginRow : 1;
                ox=l2w[row*4+0]; oy=l2w[row*4+1]; oz=l2w[row*4+2];
            }
            tp[0]=ox+ux*g_waProjRange; tp[1]=oy+uy*g_waProjRange; tp[2]=oz+uz*g_waProjRange; tp[3]=1.0f;
            // 3) disable guidance: clear smartGunIsProjectileGuided AND the tracked-target handle
            // (WeaponParams.trackedTargetComponent @ +0x20 -> ev+0x140), so nothing re-homes it.
            if (g_waProjUnguide) {
                __try { *guided = false; } __except(EXCEPTION_EXECUTE_HANDLER) {}
                __try { uint64_t* trk = reinterpret_cast<uint64_t*>(ev + 0x140); trk[0]=0; trk[1]=0; } __except(EXCEPTION_EXECUTE_HANDLER) {}
            }
            g_projDump[28]=ox; g_projDump[29]=oy; g_projDump[30]=oz; // world origin we used
            g_projDump[31]=ux; g_projDump[32]=uy; g_projDump[33]=uz; // controller dir written
            // POST verification: read back what we wrote
            g_projDump[34]=sv[0]; g_projDump[35]=sv[1]; g_projDump[36]=sv[2]; // startVel POST
            g_projDump[37]=tp[0]; g_projDump[38]=tp[1]; g_projDump[39]=tp[2]; // targetPos POST
            ++g_waProjMutated;
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (false /* DISABLED 2026-06-15: projectile g_waFwd startVel path, dead (homes to targetPos) */ && rcx) {
        const float fx = g_waFwd[0], fy = g_waFwd[1], fz = g_waFwd[2];
        const float fl = fx * fx + fy * fy + fz * fz;
        if (fl > 0.25f) {
            __try {
                float* sp = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(rcx) + 0xF0); // startPoint
                float* sv = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(rcx) + 0x100); // startVelocity
                bool gated = true;
                if (g_waGateDist > 0.0f) {
                    const float gx = sp[0]-g_waPos[0], gy = sp[1]-g_waPos[1], gz = sp[2]-g_waPos[2];
                    gated = (gx*gx + gy*gy + gz*gz) <= (g_waGateDist * g_waGateDist);
                }
                if (gated) {
                    const float spd = std::sqrt(sv[0]*sv[0] + sv[1]*sv[1] + sv[2]*sv[2]);
                    if (std::isfinite(spd) && spd > 0.01f) {
                        const float inv = 1.0f / std::sqrt(fl);
                        sv[0] = fx*inv*spd; sv[1] = fy*inv*spd; sv[2] = fz*inv*spd;
                        if (g_waMode & 1) { sp[0]=g_waPos[0]; sp[1]=g_waPos[1]; sp[2]=g_waPos[2]; }
                        ++g_waProjMutated;
                    }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) {}
        }
    }
    return result;
}

// TargetHelper(arg1, outHit, shotContext, targetInfo, float* origin, float* target):
// Per-shot lever in this build (fires once per fired round for hitscan AND projectile
// weapons -- confirmed: pistol + shotgun both bump targetCalls; ShotInputClassify never fires).
// It is called from two sites; only the player-shot wrapper callsite drives the
// bullet, so we gate on that return address.
// Redirect: target = origin + weaponForward * range, call original (it traces toward our
// direction), then RESTORE the caller's buffer.
extern "C" inline void* Hooked_WaTarget(void* a1, void* a2, void* a3, void* a4, void* origin, void* target) {
    ++g_waTargetCalls;

    const uintptr_t ret = reinterpret_cast<uintptr_t>(_ReturnAddress());
    const uintptr_t base = g_waExeBase;
    const uintptr_t retRva = (base && ret >= base) ? (ret - base) : 0;
    // BOTH TargetHelper callers count now (0x46F017 shot-wrapper AND 0x1F1251B). Capturing
    // the caller helps see which path the wall-shot actually uses.
    const bool fromShot = (retRva == kWaTargetShotRet) || (retRva == 0x1F1251B);

    float* o = reinterpret_cast<float*>(origin);
    float* t = reinterpret_cast<float*>(target);

    ++g_waTargetCalls;
    if (fromShot) {
        ++g_waTargetFromShot;
        g_shotInProgress = 2;
        g_waLastRetRva = static_cast<uint32_t>(retRva);
        // Publish the shot-frame flag to shared mem so dxgi can skip the HMD camera
        // overwrite this frame (lets the engine's native snap-to-aim through -> bullet
        // follows AIM, not the head). Decayed by Hooked_Go (frequent).
        g_shotTick = GetTickCount64();
        if (g_pSharedHands) { reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[57] = 1u; }
        __try {
            if (o && t) {
                const float dx = t[0] - o[0], dy = t[1] - o[1], dz = t[2] - o[2];
                g_waTargetOrigin[0] = o[0]; g_waTargetOrigin[1] = o[1]; g_waTargetOrigin[2] = o[2];
                g_waTargetDir[0] = dx; g_waTargetDir[1] = dy; g_waTargetDir[2] = dz;

                // CLEAN REDIRECT (g_waTgtCtrl): target = origin + controllerFwd * 100. This makes
                // dir = normalize(target-origin) = the controller forward, at the very point
                // TargetHelper computes the shot direction. If the wall impact follows the
                // controller -> TargetHelper drives the bullet. If not -> it's aim-assist
                // and the impact trace is a different caller. g_waTgtNeg flips sign.
                if (g_waTgtCtrl && g_pSharedHands) {
                    float fx=g_pSharedHands[60], fy=g_pSharedHands[61], fz=g_pSharedHands[62];
                    const float l=fx*fx+fy*fy+fz*fz;
                    if (std::isfinite(l) && l>0.1f) {
                        const float inv=(g_waTgtNeg?-100.0f:100.0f)/std::sqrt(l);
                        t[0]=o[0]+fx*inv; t[1]=o[1]+fy*inv; t[2]=o[2]+fz*inv;
                        ++g_waTgtOvr;
                    }
                }
                // TEST: when g_xfTestYaw != 0, BEND the trace target hard in a chosen plane.
                else if (g_xfTestYaw != 0.0f) {
                    const float a = g_xfTestYaw;
                    const float ca = std::cos(a), sa = std::sin(a);
                    float ndx=dx, ndy=dy, ndz=dz;
                    if (g_xfTestPlane == 1) { ndx = dx*ca - dz*sa; ndz = dx*sa + dz*ca; }      // around Y
                    else if (g_xfTestPlane == 2) { ndy = dy*ca - dz*sa; ndz = dy*sa + dz*ca; } // around X
                    else { ndx = dx*ca - dy*sa; ndy = dx*sa + dy*ca; }                          // around Z
                    t[0] = o[0] + ndx; t[1] = o[1] + ndy; t[2] = o[2] + ndz;
                    ++g_waRedirects;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }

    void* result = OrigWaTarget(a1, a2, a3, a4, origin, target);
    if (fromShot && g_shotInProgress == 2) g_shotInProgress = 0;
    return result;
}

// ShotInputClassify(source, targetPoint) -- THE hitscan aim lever.
extern "C" inline uint32_t Hooked_WaClassify(void* source, void* targetPoint) {
    ++g_waClassifyCalls;

    // Return-address gate: only the player's shot flows in from the shot-vector processor.
    const uintptr_t ret = reinterpret_cast<uintptr_t>(_ReturnAddress());
    const uintptr_t base = g_waExeBase;
    bool fromShot = false;
    if (base && ret >= base) {
        const uintptr_t rva = ret - base;
        fromShot = (rva == kWaClassifyRetProc || rva == kWaClassifyRetAltProc);
    }

    bool mutated = false;
    float savedX = 0, savedY = 0, savedZ = 0, savedW = 0;
    float* tgt = reinterpret_cast<float*>(targetPoint);

    if (fromShot) {
        ++g_waClassifyFromShot;
        __try {
            if (source && tgt) {
                const float* o = reinterpret_cast<const float*>(reinterpret_cast<uint8_t*>(source) + 0x50);
                const float ox = o[0], oy = o[1], oz = o[2];
                const float dx = tgt[0] - ox, dy = tgt[1] - oy, dz = tgt[2] - oz;
                // diagnostics (original aim)
                g_waTargetOrigin[0] = ox; g_waTargetOrigin[1] = oy; g_waTargetOrigin[2] = oz;
                g_waTargetDir[0] = dx; g_waTargetDir[1] = dy; g_waTargetDir[2] = dz;

                if (false /* DISABLED 2026-06-15: TargetHelper target = aim-assist, dead lever */) {
                    const float fx = g_waFwd[0], fy = g_waFwd[1], fz = g_waFwd[2];
                    const float fl = fx * fx + fy * fy + fz * fz;
                    if (fl > 0.25f) {
                        float dist = std::sqrt(dx * dx + dy * dy + dz * dz);
                        if (std::isfinite(dist) && dist >= 0.05f && dist <= 10000.0f) {
                            const float inv = 1.0f / std::sqrt(fl);
                            savedX = tgt[0]; savedY = tgt[1]; savedZ = tgt[2]; savedW = tgt[3];
                            float bx = ox, by = oy, bz = oz;
                            if (g_waMode & 1) { bx = g_waPos[0]; by = g_waPos[1]; bz = g_waPos[2]; }
                            tgt[0] = bx + fx * inv * dist;
                            tgt[1] = by + fy * inv * dist;
                            tgt[2] = bz + fz * inv * dist;
                            mutated = true;
                            ++g_waRedirects;
                        }
                    }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { mutated = false; }
    }

    uint32_t result = OrigWaClassify ? OrigWaClassify(source, targetPoint) : 0;

    // Restore the caller's buffer; the redirect is baked in by now.
    if (mutated) {
        __try {
            tgt[0] = savedX; tgt[1] = savedY; tgt[2] = savedZ; tgt[3] = savedW;
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return result;
}

// ============================================================================
// PHYSICS-TRACE CALL-SITE PATCH.
// The hitscan ray for the player's shot is built by the physics trace invoked via
// `call [r10+0x150]` at +0x46F1EA, inside the player shot wrapper (the
// same fn that calls TargetHelper). TargetHelper is aim-assist (mutating it never moved
// the bullet); THIS call is the actual raycast. Because the call site lives only in the
// player shot path, patching it needs no time-window gate. We replace the 7-byte call
// with `call <relay>` + 2 NOPs; the relay tail-jumps to Hook_WaPhysTrace, which rotates
// the trace basis + ray list to the weapon forward, calls the original via the vtable,
// then restores.
// ============================================================================
static constexpr uintptr_t kWaPhysCallsite = 0x46F1EA;
static constexpr uint8_t   kWaExpectedCall[7] = { 0x41, 0xFF, 0x92, 0x50, 0x01, 0x00, 0x00 };

extern volatile uint64_t g_waPhysCalls;
extern volatile uint64_t g_waPhysMutated;
extern volatile int      g_waPhysPatched;
// One-shot snapshot of the physics-call args (to locate the real direction field).
extern volatile int      g_waDbgSnapped;
extern volatile float    g_waDbgArg3[72];  // arg3 (r8) first 0x120 bytes as floats
extern volatile float    g_waDbgRay[40];   // rayList (r9) first 0xA0 bytes as floats
extern volatile float    g_waDbgRayEntry[28]; // first ray-list entry (deref *rayList) 0x70 bytes

typedef uintptr_t (*WaPhysFn)(void* self, void* a2, void* a3, void* rayList);
static uint8_t* s_waCallsite = nullptr;
static uint8_t  s_waOrigBytes[7] = {0};
static uint8_t* s_waRelay = nullptr;

struct WaSaved { float* p; float x, y, z; };
static constexpr int kWaMaxSaved = 256;

static inline float WaLen3(float x, float y, float z) { return std::sqrt(x * x + y * y + z * z); }

// Shortest-arc quaternion (qx,qy,qz,qw) rotating unit vector a onto unit vector b.
static inline void WaQuatFromTo(const float a[3], const float b[3], float q[4]) {
    const float d = a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
    if (d >= 0.99999f) { q[0]=q[1]=q[2]=0.0f; q[3]=1.0f; return; }
    if (d <= -0.99999f) {
        float ax[3] = {1,0,0};
        if (std::fabs(a[0]) > 0.9f) { ax[0]=0; ax[1]=1; ax[2]=0; }
        float c[3] = { a[1]*ax[2]-a[2]*ax[1], a[2]*ax[0]-a[0]*ax[2], a[0]*ax[1]-a[1]*ax[0] };
        float l = WaLen3(c[0],c[1],c[2]); if (l<1e-6f) l=1.0f;
        q[0]=c[0]/l; q[1]=c[1]/l; q[2]=c[2]/l; q[3]=0.0f; return;
    }
    float c[3] = { a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0] };
    q[0]=c[0]; q[1]=c[1]; q[2]=c[2]; q[3]=1.0f + d;
    const float n = std::sqrt(q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3]);
    if (n > 1e-8f) { float inv=1.0f/n; q[0]*=inv; q[1]*=inv; q[2]*=inv; q[3]*=inv; }
}

// v' = q * v * q^-1  (q = x,y,z,w). Writes back in place, saving the original.
static inline void WaRotSave(float* v, const float q[4], WaSaved* saved, int& n) {
    if (!v || n >= kWaMaxSaved) return;
    const float x = v[0], y = v[1], z = v[2];
    if (!std::isfinite(x) || !std::isfinite(y) || !std::isfinite(z)) return;
    saved[n].p = v; saved[n].x = x; saved[n].y = y; saved[n].z = z; ++n;
    const float tx = 2.0f * (q[1]*z - q[2]*y);
    const float ty = 2.0f * (q[2]*x - q[0]*z);
    const float tz = 2.0f * (q[0]*y - q[1]*x);
    v[0] = x + q[3]*tx + (q[1]*tz - q[2]*ty);
    v[1] = y + q[3]*ty + (q[2]*tx - q[0]*tz);
    v[2] = z + q[3]*tz + (q[0]*ty - q[1]*tx);
}

// Rotate the trace basis (arg3) + ray list to the published weapon forward.
static inline int WaCompensateToWeapon(void* arg3, void* rayList, WaSaved* saved) {
    int n = 0;
    if (!arg3 || !rayList) return 0;
    __try {
        uint8_t* base = reinterpret_cast<uint8_t*>(arg3);
        uint8_t* list = reinterpret_cast<uint8_t*>(rayList);
        float* fwd = reinterpret_cast<float*>(base + 0x40);
        float cur[3] = { fwd[0], fwd[1], fwd[2] };
        float cl = WaLen3(cur[0], cur[1], cur[2]);
        if (!std::isfinite(cl) || cl < 1e-4f) return 0;
        cur[0]/=cl; cur[1]/=cl; cur[2]/=cl;

        float want[3] = { g_waFwd[0], g_waFwd[1], g_waFwd[2] };
        float wl = WaLen3(want[0], want[1], want[2]);
        if (wl < 0.5f) return 0;
        want[0]/=wl; want[1]/=wl; want[2]/=wl;

        float q[4];
        WaQuatFromTo(cur, want, q);

        // Basis vectors (forward/right/up + duplicates).
        WaRotSave(reinterpret_cast<float*>(base + 0x40), q, saved, n);
        WaRotSave(reinterpret_cast<float*>(base + 0x50), q, saved, n);
        WaRotSave(reinterpret_cast<float*>(base + 0x60), q, saved, n);
        WaRotSave(reinterpret_cast<float*>(base + 0x70), q, saved, n);
        WaRotSave(reinterpret_cast<float*>(base + 0xC0), q, saved, n);
        WaRotSave(reinterpret_cast<float*>(base + 0xD0), q, saved, n);
        WaRotSave(reinterpret_cast<float*>(base + 0xE0), q, saved, n);
        // Ray-list segment (forward * length) + per-entry right/up/forward.
        WaRotSave(reinterpret_cast<float*>(list + 0x30), q, saved, n);
        uint8_t* entries = *reinterpret_cast<uint8_t**>(list);
        const uint32_t count = *reinterpret_cast<uint32_t*>(list + 0x0C);
        if (entries && count > 0 && count <= 64) {
            for (uint32_t i = 0; i < count && n + 3 <= kWaMaxSaved; ++i) {
                uint8_t* e = entries + static_cast<size_t>(i) * 0x70;
                WaRotSave(reinterpret_cast<float*>(e + 0x00), q, saved, n);
                WaRotSave(reinterpret_cast<float*>(e + 0x10), q, saved, n);
                WaRotSave(reinterpret_cast<float*>(e + 0x20), q, saved, n);
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return n;
}

static inline WaPhysFn WaResolveOrig(void* self) {
    if (!self) return nullptr;
    __try {
        void** vt = *reinterpret_cast<void***>(self);
        if (!vt) return nullptr;
        return reinterpret_cast<WaPhysFn>(vt[0x150 / sizeof(void*)]);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return nullptr; }
}

extern "C" inline uintptr_t Hooked_WaPhysTrace(void* self, void* a2, void* a3, void* rayList) {
    ++g_waPhysCalls;

    // One-shot arg snapshot (only when redirect is enabled, i.e. the user fired a test
    // shot) so we can find which field actually holds the camera-forward direction.
    if (g_waEnable && !g_waDbgSnapped) {
        __try {
            if (a3) { const float* f = reinterpret_cast<const float*>(a3); for (int i=0;i<72;++i) g_waDbgArg3[i]=f[i]; }
            if (rayList) {
                const float* r = reinterpret_cast<const float*>(rayList);
                for (int i=0;i<40;++i) g_waDbgRay[i]=r[i];
                uint8_t* ep = *reinterpret_cast<uint8_t**>(rayList); // entries ptr @ +0
                if (ep) { const float* e = reinterpret_cast<const float*>(ep); for (int i=0;i<28;++i) g_waDbgRayEntry[i]=e[i]; }
            }
            g_waDbgSnapped = 1;
        } __except (EXCEPTION_EXECUTE_HANDLER) { g_waDbgSnapped = 1; }
    }

    WaSaved saved[kWaMaxSaved];
    int saveCount = 0;
    const float fl = g_waFwd[0]*g_waFwd[0] + g_waFwd[1]*g_waFwd[1] + g_waFwd[2]*g_waFwd[2];
    if (false /* DISABLED 2026-06-15: physics-trace basis = secondary hit/cover raycast, dead */ && fl > 0.25f) {
        saveCount = WaCompensateToWeapon(a3, rayList, saved);
        if (saveCount > 0) ++g_waPhysMutated;
    }
    WaPhysFn orig = WaResolveOrig(self);
    uintptr_t result = orig ? orig(self, a2, a3, rayList) : 0;
    if (saveCount > 0) {
        __try {
            for (int i = 0; i < saveCount; ++i) {
                if (saved[i].p) { saved[i].p[0]=saved[i].x; saved[i].p[1]=saved[i].y; saved[i].p[2]=saved[i].z; }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return result;
}

static inline void* WaAllocRelayNear(uint8_t* target) {
    SYSTEM_INFO si{}; GetSystemInfo(&si);
    const uintptr_t gran = si.dwAllocationGranularity ? si.dwAllocationGranularity : 0x10000;
    const uintptr_t ta = reinterpret_cast<uintptr_t>(target);
    for (uintptr_t dist = gran; dist < 0x70000000ull; dist += gran) {
        uintptr_t cand[2] = { ta + dist, ta > dist ? ta - dist : 0 };
        for (uintptr_t c : cand) {
            if (!c) continue;
            c &= ~(gran - 1);
            void* m = VirtualAlloc(reinterpret_cast<void*>(c), gran, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE);
            if (m) return m;
        }
    }
    return nullptr;
}

// Patches the physics-trace call site (verifies the expected bytes first -- if they
// don't match this game build, it refuses to patch and leaves the game untouched).
inline bool InstallWeaponPhysPatch() {
    if (!g_waExeBase) return false;
    s_waCallsite = reinterpret_cast<uint8_t*>(g_waExeBase + kWaPhysCallsite);
    if (std::memcmp(s_waCallsite, kWaExpectedCall, sizeof(kWaExpectedCall)) != 0) {
        g_waPhysPatched = -1; // signature mismatch -> not patched (safe)
        return false;
    }
    std::memcpy(s_waOrigBytes, s_waCallsite, sizeof(s_waOrigBytes));

    s_waRelay = reinterpret_cast<uint8_t*>(WaAllocRelayNear(s_waCallsite));
    if (!s_waRelay) { g_waPhysPatched = -2; return false; }
    // relay: mov rax, &Hooked_WaPhysTrace ; jmp rax
    uint8_t relay[12] = { 0x48, 0xB8, 0,0,0,0,0,0,0,0, 0xFF, 0xE0 };
    const uint64_t dst = reinterpret_cast<uint64_t>(&Hooked_WaPhysTrace);
    std::memcpy(relay + 2, &dst, 8);
    std::memcpy(s_waRelay, relay, sizeof(relay));
    FlushInstructionCache(GetCurrentProcess(), s_waRelay, sizeof(relay));

    const intptr_t delta = s_waRelay - (s_waCallsite + 5);
    if (delta < INT32_MIN || delta > INT32_MAX) { g_waPhysPatched = -3; return false; }
    uint8_t patch[7] = { 0xE8,
        static_cast<uint8_t>(delta & 0xFF), static_cast<uint8_t>((delta >> 8) & 0xFF),
        static_cast<uint8_t>((delta >> 16) & 0xFF), static_cast<uint8_t>((delta >> 24) & 0xFF),
        0x90, 0x90 };
    DWORD oldp = 0;
    if (!VirtualProtect(s_waCallsite, 7, PAGE_EXECUTE_READWRITE, &oldp)) { g_waPhysPatched = -4; return false; }
    std::memcpy(s_waCallsite, patch, sizeof(patch));
    FlushInstructionCache(GetCurrentProcess(), s_waCallsite, sizeof(patch));
    DWORD ign = 0; VirtualProtect(s_waCallsite, 7, oldp, &ign);
    g_waPhysPatched = 1;
    return true;
}

// ============================================================================
// NORMALIZE CALL-SITE PATCHES.
// 0x46F0E5 is the older TargetHelper-wrapper experiment. The real mapped weapon-fire
// routine at 0x84C968 computes dir = Normalize(target - muzzle),
// then feeds spread and the trace. Patch both direct callsites, but track the real
// weapon-fire one separately so the test is unambiguous.
// ============================================================================
static constexpr uintptr_t kWaNormCallsite = 0x46F0E5;
static constexpr uintptr_t kWaFireNormCallsite = 0x84C968;
static constexpr uintptr_t kWaNormalizeFn  = 0x13DE80;
extern volatile uint64_t g_waNormShot;
extern volatile uint64_t g_waNormMutated;
extern volatile int      g_waNormPatched;
extern volatile uint64_t g_waFireNormShot;
extern volatile uint64_t g_waFireNormMutated;
extern volatile int      g_waFireNormPatched;

typedef void* (*WaNormFn)(float*, float*);
static WaNormFn s_waNormOrig = nullptr;
static uint8_t* s_waNormCallsite = nullptr;
static uint8_t  s_waNormOrigBytes[5] = {0};
static uint8_t* s_waNormRelay = nullptr;
static uint8_t* s_waFireNormCallsite = nullptr;
static uint8_t* s_waFireNormRelay = nullptr;

extern "C" inline void* Hooked_WaNormShot(float* input, float* output) {
    void* result = s_waNormOrig ? s_waNormOrig(input, output) : reinterpret_cast<void*>(output);
    ++g_waNormShot;
    const uintptr_t ret = reinterpret_cast<uintptr_t>(_ReturnAddress());
    const uintptr_t base = g_waExeBase;
    const bool fromWeaponFire = (base && ret >= base && (ret - base) == (kWaFireNormCallsite + 5));
    if (fromWeaponFire) ++g_waFireNormShot;
    // ISOLATED LEVER (2026-06-15): override ONLY the weapon-fire dir (@0x84C968 =
    // dir = normalize(target - muzzle)), using the dxgi-published CONTROLLER forward (shared[60..62]);
    // fall back to g_waFwd only if the controller fwd isn't published. The 0x46F0E5 (smart-aim)
    // override is intentionally NOT mutated here. g_waFireNormShot tells us if that path even
    // fires on the player's shot; g_waFireNormMutated counts the redirects.
    if (g_waEnable && output && fromWeaponFire) {
        float fx = g_waFwd[0], fy = g_waFwd[1], fz = g_waFwd[2];
        if (g_pSharedHands) {
            const float cx = g_pSharedHands[60], cy = g_pSharedHands[61], cz = g_pSharedHands[62];
            if (cx*cx + cy*cy + cz*cz > 0.25f) { fx = cx; fy = cy; fz = cz; }
        }
        const float fl = fx*fx + fy*fy + fz*fz;
        if (fl > 0.25f) {
            __try {
                const float inv = 1.0f / std::sqrt(fl);
                output[0] = fx * inv; output[1] = fy * inv; output[2] = fz * inv;
                ++g_waNormMutated; ++g_waFireNormMutated;
            } __except (EXCEPTION_EXECUTE_HANDLER) {}
        }
    }
    return result;
}

inline bool WaInstallNormPatchAt(uintptr_t siteRva, uint8_t*& callsite, uint8_t*& relayOut, volatile int* status) {
    if (!g_waExeBase) return false;
    if (*status == 1) return true;
    s_waNormOrig = reinterpret_cast<WaNormFn>(g_waExeBase + kWaNormalizeFn);
    callsite = reinterpret_cast<uint8_t*>(g_waExeBase + siteRva);
    // Verify it is `E8 rel32` calling Normalize.
    if (callsite[0] != 0xE8) { *status = -1; return false; }
    int32_t rel = 0; std::memcpy(&rel, callsite + 1, 4);
    uint8_t* callTarget = callsite + 5 + rel;
    if (callTarget != reinterpret_cast<uint8_t*>(g_waExeBase + kWaNormalizeFn)) { *status = -2; return false; }
    if (siteRva == kWaNormCallsite) std::memcpy(s_waNormOrigBytes, callsite, 5);

    relayOut = reinterpret_cast<uint8_t*>(WaAllocRelayNear(callsite));
    if (!relayOut) { *status = -3; return false; }
    uint8_t stub[12] = { 0x48, 0xB8, 0,0,0,0,0,0,0,0, 0xFF, 0xE0 };
    const uint64_t dst = reinterpret_cast<uint64_t>(&Hooked_WaNormShot);
    std::memcpy(stub + 2, &dst, 8);
    std::memcpy(relayOut, stub, sizeof(stub));
    FlushInstructionCache(GetCurrentProcess(), relayOut, sizeof(stub));

    const intptr_t delta = relayOut - (callsite + 5);
    if (delta < INT32_MIN || delta > INT32_MAX) { *status = -4; return false; }
    uint8_t patch[5] = { 0xE8,
        static_cast<uint8_t>(delta & 0xFF), static_cast<uint8_t>((delta >> 8) & 0xFF),
        static_cast<uint8_t>((delta >> 16) & 0xFF), static_cast<uint8_t>((delta >> 24) & 0xFF) };
    DWORD oldp = 0;
    if (!VirtualProtect(callsite, 5, PAGE_EXECUTE_READWRITE, &oldp)) { *status = -5; return false; }
    std::memcpy(callsite, patch, sizeof(patch));
    FlushInstructionCache(GetCurrentProcess(), callsite, sizeof(patch));
    DWORD ign = 0; VirtualProtect(callsite, 5, oldp, &ign);
    *status = 1;
    return true;
}

inline bool InstallWeaponNormPatch() {
    bool ok = WaInstallNormPatchAt(kWaNormCallsite, s_waNormCallsite, s_waNormRelay, &g_waNormPatched);
    ok = WaInstallNormPatchAt(kWaFireNormCallsite, s_waFireNormCallsite, s_waFireNormRelay, &g_waFireNormPatched) && ok;
    return ok;
}

// ============================================================================
// SHOTSNAP DECOUPLE @0x79ACA0, retargeted to the controller. PlayerShotCaller runs the visual + projectile/hitscan dispatch
// synchronously on the game thread. We BRACKET the FPP camera localOrientation
// (cam+0xD0): onEnter write the controller/weapon-aim quaternion, run the original
// (the shot reads cam+0xD0 -> bullet flies along the controller), onLeave restore the
// HMD quaternion. The renderer reads cam+0xD0 on the render thread on its own schedule;
// the snap window is the microsecond duration of this call, so the VIEW keeps following
// the head.
//
// The "clean" orientation = saved (cam local = HMD-injected) composed with the delta
// that rotates HMD-aim -> controller-aim:  clean = saved (X) delta.
// delta = inv(hmdRel) (X) rightHandQuat  (both from shared mem; headset space), with a
// tunable convention/mode because the camera-local quat sign/axis order must match.
// g_ssTestYaw provides a static-yaw sanity test (does bracketing cam+0xD0 move the bullet
// at all?) before trusting the controller math.
// ============================================================================
static constexpr uintptr_t kSsOffset = 0x79ACA0;
extern volatile uint64_t  g_ssCalls;
extern volatile uint64_t  g_ssSnapped;
extern volatile uintptr_t g_ssCamPtr;     // FPP camera obj (set by CET SetVRShotCamera)
extern volatile int       g_ssEnable;
extern volatile int       g_ssMode;       // bracket offset selector: 0=+0xF0(world) 1=+0xD0(local) 2=+0x110
extern volatile float     g_ssTestYaw;    // static yaw (rad) sanity test; 0 = use controller
extern volatile float     g_ssCamQuat[4]; // last read bracket-offset quat (diag)
extern volatile float     g_ssDiagD0[4];  // diag: cam+0xD0 (local orient)
extern volatile float     g_ssDiagF0[4];  // diag: cam+0xF0 (world orient)
extern volatile float     g_ssDiag110[4]; // diag: cam+0x110
typedef void (*SsFn)(void*, void*, void*, void*);
static SsFn OrigSs = nullptr;

// ============================================================================
// GET-WORLD-ORIENTATION HOOK @0x802390 -- the shot's aim reader (found via memory access tracing
// on cam+0xF0: this is the ONLY accessor that fires on the SHOT, not
// every frame). This function copies the camera world orientation quat from
// [rcx-0x30] (= cam+0xF0) into [rdx]. The shot reads its aim through this getter. We hook
// it and overwrite the OUTPUT quat [rdx] -> the shot (and whatever else calls it) gets our
// orientation. g_goTestYaw: rotate the output by a static yaw (sanity). g_goMode: 0 off,
// 1 always (test: see if bullet+view move), 2 gated-by-shot (g_shotInProgress).
// ============================================================================
static constexpr uintptr_t kGoOffset = 0x802390;
extern volatile uint64_t g_goCalls;
extern volatile uint64_t g_goMutated;
extern volatile int      g_goMode;
extern volatile float    g_goTestYaw;
extern volatile int      g_goPlane;
extern volatile float    g_goLastQuat[4];
typedef void* (*GoFn)(void*, void*);
static GoFn OrigGo = nullptr;

extern "C" inline void* Hooked_Go(void* rcx, void* rdx) {
    void* result = OrigGo ? OrigGo(rcx, rdx) : nullptr;
    ++g_goCalls;
    // Decay the shot-frame flag ~120ms after the last shot (this getter fires per frame).
    if (g_pSharedHands && reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[57] != 0u) {
        if (GetTickCount64() - g_shotTick > 120) reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[57] = 0u;
    }
    const bool gate = (g_goMode == 1) || (g_goMode == 2 && g_shotInProgress != 0);
    if (gate && rdx) {
        __try {
            float* q = reinterpret_cast<float*>(rdx);
            g_goLastQuat[0]=q[0]; g_goLastQuat[1]=q[1]; g_goLastQuat[2]=q[2]; g_goLastQuat[3]=q[3];
            if (g_goTestYaw != 0.0f) {
                // rotate the output quat by testYaw about a chosen axis (sanity test).
                const float h = g_goTestYaw * 0.5f;
                const float s = std::sin(h), c = std::cos(h);
                float ax=0,ay=0,az=0;
                if (g_goPlane == 1) ay = 1.0f; else if (g_goPlane == 2) ax = 1.0f; else az = 1.0f;
                const float rx=ax*s, ry=ay*s, rz=az*s, rw=c;
                const float ox=q[0], oy=q[1], oz=q[2], ow=q[3];
                // q' = q (X) r
                q[0] = ow*rx + ox*rw + oy*rz - oz*ry;
                q[1] = ow*ry - ox*rz + oy*rw + oz*rx;
                q[2] = ow*rz + ox*ry - oy*rx + oz*rw;
                q[3] = ow*rw - ox*rx - oy*ry - oz*rz;
                ++g_goMutated;
            } else if (g_pSharedHands) {
                const float ax=g_pSharedHands[53], ay=g_pSharedHands[54], az=g_pSharedHands[55], aw=g_pSharedHands[56];
                const float al = ax*ax+ay*ay+az*az+aw*aw;
                if (al > 0.25f) {
                    const float inv = 1.0f/std::sqrt(al);
                    q[0]=ax*inv; q[1]=ay*inv; q[2]=az*inv; q[3]=aw*inv;
                    ++g_goMutated;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return result;
}

// ============================================================================
// CAMERA-TRANSFORM GETTER HOOK @0x1D92A0 -- another camera transform lever (found via HW-breakpoint trace
// of cam+0xF0 readers). `GetWorldTransform(rcx, rdx=cam, r8=out)` copies the
// camera worldTransform: pos[rdx+0xE0]->[r8+0], orient[rdx+0xF0]->[r8+0x10]. EVERYONE
// (render + shot) calls this to get the camera transform; the shot derives the bullet
// direction from the returned orientation. We hook it and, ONLY while a player shot is in
// progress (g_shotInProgress, set by the PlayerShotCaller hook), overwrite the output
// orientation [r8+0x10] with the controller-aim quat (dxgi-published, shared mem [53..56]).
// -> the shot gets the controller direction; the render (ungated) gets the real HMD orient.
// g_xfMode: 0=off, 1=ALWAYS (test: moves view+shot, confirms the lever), 2=gated-by-shot.
// ============================================================================
static constexpr uintptr_t kXfOffset = 0x1D92A0;
extern volatile uint64_t g_xfCalls;
extern volatile uint64_t g_xfMutated;
extern volatile int      g_xfMode;          // 0 off, 1 always, 2 gated-by-shot
extern volatile float    g_xfTestYaw;       // !=0 -> rotate output orient by this (sanity); 0 -> controller quat
extern volatile int      g_shotInProgress;
extern volatile float    g_xfLastOut[4];    // diag: output orient we saw
typedef uintptr_t (*XfFn)(void*, void*, void*);
static XfFn OrigXf = nullptr;

extern "C" inline uintptr_t Hooked_Xf(void* rcx, void* rdx, void* r8) {
    uintptr_t result = OrigXf ? OrigXf(rcx, rdx, r8) : 0;
    ++g_xfCalls;
    // Gate = xfMode itself (no g_waEnable dependency). mode 1 = always, 2 = shot only.
    const bool gate = (g_xfMode == 1) || (g_xfMode == 2 && g_shotInProgress != 0);
    if (gate && r8) {
        __try {
            float* o = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(r8) + 0x10);
            g_xfLastOut[0]=o[0]; g_xfLastOut[1]=o[1]; g_xfLastOut[2]=o[2]; g_xfLastOut[3]=o[3];
            if (g_xfTestYaw != 0.0f) {
                // Sanity: rotate the camera output orientation by a static yaw (game-up = Z).
                const float h = g_xfTestYaw * 0.5f;
                const float qz = std::sin(h), qw = std::cos(h);
                const float ox=o[0], oy=o[1], oz=o[2], ow=o[3];
                // o = o (X) yaw
                o[0] = ow*0  + ox*qw + oy*qz - oz*0;
                o[1] = ow*0  - ox*qz + oy*qw + oz*0;
                o[2] = ow*qz + ox*0  - oy*0  + oz*qw;
                o[3] = ow*qw - ox*0  - oy*0  - oz*qz;
                ++g_xfMutated;
            } else if (g_pSharedHands) {
                const float ax=g_pSharedHands[53], ay=g_pSharedHands[54], az=g_pSharedHands[55], aw=g_pSharedHands[56];
                const float al = ax*ax+ay*ay+az*az+aw*aw;
                if (al > 0.25f) {
                    const float inv = 1.0f/std::sqrt(al);
                    o[0]=ax*inv; o[1]=ay*inv; o[2]=az*inv; o[3]=aw*inv;
                    ++g_xfMutated;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return result;
}

static inline void SsQuatMul(float ax,float ay,float az,float aw, float bx,float by,float bz,float bw,
                             float& ox,float& oy,float& oz,float& ow) {
    ox = aw*bx + ax*bw + ay*bz - az*by;
    oy = aw*by - ax*bz + ay*bw + az*bx;
    oz = aw*bz + ax*by - ay*bx + az*bw;
    ow = aw*bw - ax*bx - ay*by - az*bz;
}

extern "C" inline void Hooked_Ss(void* rcx, void* rdx, void* r8, void* r9) {
    ++g_ssCalls;
    bool snapped = false;
    float saved[4] = {0,0,0,1};
    float* slot = nullptr;

    // Bracket the FPP CAMERA component (set by CET SetVRShotCamera -> g_ssCamPtr). Per the
    // entIPlacedComponent SDK layout: localTransform.Orientation @ +0xD0 (identity -- cam
    // doesn't rotate locally), worldTransform.Orientation @ +0xF0 (the LIVE world orientation
    // the shot likely reads). g_ssMode selects the bracket offset: 0=+0xF0, 1=+0xD0, 2=+0x110.
    // The controller-aim quat dxgi built (same game-world convention) is at shared mem [53..56].
    void* cam = reinterpret_cast<void*>(g_ssCamPtr);
    float aim[4] = {0,0,0,1};
    if (g_pSharedHands) {
        aim[0]=g_pSharedHands[53]; aim[1]=g_pSharedHands[54]; aim[2]=g_pSharedHands[55]; aim[3]=g_pSharedHands[56];
    }

    if (cam) {
        __try {
            uint8_t* c = reinterpret_cast<uint8_t*>(cam);
            for (int i=0;i<4;++i) {
                g_ssDiagD0[i]  = reinterpret_cast<float*>(c+0xD0)[i];
                g_ssDiagF0[i]  = reinterpret_cast<float*>(c+0xF0)[i];
                g_ssDiag110[i] = reinterpret_cast<float*>(c+0x110)[i];
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }

    if (g_ssEnable && cam) {
        const int off = (g_ssMode==1) ? 0xD0 : (g_ssMode==2 ? 0x110 : 0xF0);
        __try {
            slot = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(cam) + off);
            const float cx=slot[0], cy=slot[1], cz=slot[2], cw=slot[3];
            const float cl = cx*cx+cy*cy+cz*cz+cw*cw;
            g_ssCamQuat[0]=cx; g_ssCamQuat[1]=cy; g_ssCamQuat[2]=cz; g_ssCamQuat[3]=cw;
            if (std::isfinite(cl) && cl > 0.5f && cl < 1.5f) {
                saved[0]=cx; saved[1]=cy; saved[2]=cz; saved[3]=cw;
                float nx,ny,nz,nw;
                if (g_ssTestYaw != 0.0f) {
                    const float hh = g_ssTestYaw * 0.5f;
                    SsQuatMul(cx,cy,cz,cw, 0.0f,0.0f,std::sin(hh),std::cos(hh), nx,ny,nz,nw);
                } else {
                    nx=aim[0]; ny=aim[1]; nz=aim[2]; nw=aim[3];
                }
                const float nl = nx*nx+ny*ny+nz*nz+nw*nw;
                if (std::isfinite(nl) && nl > 0.01f) {
                    const float inv = 1.0f/std::sqrt(nl);
                    slot[0]=nx*inv; slot[1]=ny*inv; slot[2]=nz*inv; slot[3]=nw*inv;
                    snapped = true;
                    ++g_ssSnapped;
                }
            } else { slot = nullptr; }
        } __except (EXCEPTION_EXECUTE_HANDLER) { slot = nullptr; snapped = false; }
    }

    g_shotInProgress = 1;            // gate window for the camera-transform getter hook
    ++g_fireInShot;                  // also gate the trace-dispatcher funnel (nest-safe counter)
    if (OrigSs) OrigSs(rcx, rdx, r8, r9);
    --g_fireInShot;
    g_shotInProgress = 0;

    if (snapped && slot) {
        __try { slot[0]=saved[0]; slot[1]=saved[1]; slot[2]=saved[2]; slot[3]=saved[3]; }
        __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
}

// ============================================================================
// FIRE-SHOT HOOK @0x4E4AFC -- the shot dispatcher.
// This path handles hitscan AND projectile shots. The function takes (rcx, rdx, r8):
// r8 (3rd arg) = the shot CONTEXT (mov rbx,r8). [r8+0x80] = a Vec3 aim DIRECTION (floats
// @+0x80/+0x84/+0x88; the fn reads it, NEGATES via xorps, then atan2f's it to a heading ->
// proves it is the direction). This is the lowest-level bullet-direction lever: rewrite it
// here and the bullet flies that way for EVERY weapon, without touching the camera at all
// (no feedback loop). g_fireMode: 0 dump-only, 1 bend-test (rotate by g_fireTestAng about
// g_firePlane -> if the wall impact moves, +0x80 IS the lever), 2 controller-override
// (write the dxgi-published controller forward [shared 60..62], sign per g_fireNeg).
// ============================================================================
// SCANNER MODEL: the bend test on [r8+0x80] did NOT move the bullet -> that field is the
// recoil/spread auxiliary angle (read, negated, atan2'd to a heading index), NOT the ray.
// The real aim direction is another field in the shot state. So this hook is now a live
// SCANNER: dump a window of floats from a chosen source (r8 shot-ctx / rdx arg2 / deref of
// [rdx] / deref of [rdx+0x10] transform) at a UI-settable byte offset, so we can SCRUB the
// shot state in-game and find the triplet that reads ~camera-forward when firing at a wall.
// Then override THAT field (g_fireOvrSrc/g_fireOvrOff) with the controller forward.
static constexpr uintptr_t kFireOffset = 0x4E4AFC;
extern volatile uint64_t g_fireCalls;
extern volatile uint64_t g_fireMutated;
extern volatile int      g_fireMode;      // 0 scan-only, 1 bend-test, 2 controller override
extern volatile int      g_firePlane;     // bend axis: 0=Z(yaw) 1=Y 2=X
extern volatile float    g_fireTestAng;   // bend angle (radians)
extern volatile int      g_fireNeg;       // mode2: 0 = write +fwd, 1 = write -fwd
extern volatile float    g_fireDir[4];    // captured override-target (pre-write) for UI
extern volatile float    g_fireDirOut[4]; // what we wrote (post)
extern volatile int      g_fireScanSrc;   // 0=r8  1=rdx  2=*(rdx+0x10)  3=*(rdx)
extern volatile int      g_fireScanRange; // bytes to scan from the source base
extern volatile int      g_fireOvrSrc;    // override source (same enum)
extern volatile int      g_fireOvrOff;    // override byte offset
extern volatile int      g_fireXform;     // transform-quat override: 0 off,1 +F0,2 +D0,3 both
extern volatile int      g_fireXformOff;  // world-orient quat offset (default 0xF0)
extern volatile int      g_fireCamSnap;   // 1 = snap FPP cam orientation to controller during the shot
extern volatile int      g_fireCamSnapOff;// cam quat offset (default 0xF0 world; try 0xD0 local)
// AUTO-SCAN results: every unit-length float-triple found in the shot struct (= a direction
// basis vector). For each: byte offset, the xyz, and dot with the controller forward (hint:
// the AIM/forward vector dots ~1 when you point the controller where you fire).
extern volatile int      g_fireHitCount;
extern volatile int      g_fireHitOff[24];
extern volatile float    g_fireHitVec[24*3];
extern volatile float    g_fireHitDot[24];
typedef void (*FireFn)(void*, void*, void*, void*);
static FireFn OrigFire = nullptr;

// src: 0=r8, 1=rdx, 2=*(rdx+0x10), 3=*(rdx), 4=rcx, 5=*(rcx). rcx matters because this path
// loads xmm9=[rcx] right before the bullet raycast -> [rcx] likely holds origin/direction.
static inline uint8_t* WaFireResolve(int src, void* rcx, void* rdx, void* r8) {
    if (src == 0) return reinterpret_cast<uint8_t*>(r8);
    if (src == 1) return reinterpret_cast<uint8_t*>(rdx);
    if (src == 4) return reinterpret_cast<uint8_t*>(rcx);
    void* base = (src == 5) ? rcx : rdx;
    if (!base) return nullptr;
    __try {
        uintptr_t p = (src == 2) ? *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(base) + 0x10)
                                 : *reinterpret_cast<uintptr_t*>(base);
        if (p > 0x10000 && p < 0x7FFFFFFFFFFFull) return reinterpret_cast<uint8_t*>(p);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return nullptr;
}

extern "C" inline void Hooked_Fire(void* rcx, void* rdx, void* r8, void* r9) {
    ++g_fireCalls;
    // Controller forward (the scan hint + the override value).
    float cfx=0, cfy=0, cfz=1; bool haveCtrl=false;
    if (g_pSharedHands) {
        cfx=g_pSharedHands[60]; cfy=g_pSharedHands[61]; cfz=g_pSharedHands[62];
        const float cl=cfx*cfx+cfy*cfy+cfz*cfz;
        if (std::isfinite(cl) && cl>0.1f) { const float ci=1.0f/std::sqrt(cl); cfx*=ci; cfy*=ci; cfz*=ci; haveCtrl=true; }
    }
    // 1) AUTO-SCAN ALL sources (r8/rdx/*(rdx+0x10)/*(rdx)/rcx/*(rcx)) for unit-length float-triples
    // (= direction basis vectors). For each hit record src, offset, xyz, dot-with-controller. The
    // bullet aim vector dots ~1 with the controller. g_fireHitOff encodes src*0x10000 + offset.
    int hits = 0;
    const int range = (g_fireScanRange > 0 && g_fireScanRange < 0x8000) ? g_fireScanRange : 0x600;
    for (int src = 0; src < 6 && hits < 24; ++src) {
        uint8_t* base = WaFireResolve(src, rcx, rdx, r8);
        if (!base) continue;
        __try {
            for (int off = 0; off + 12 <= range && hits < 24; off += 4) {
                const float* d = reinterpret_cast<const float*>(base + off);
                const float x=d[0], y=d[1], z=d[2];
                if (!std::isfinite(x) || !std::isfinite(y) || !std::isfinite(z)) continue;
                const float l = x*x + y*y + z*z;
                if (l < 0.94f || l > 1.06f) continue;           // not unit length
                if (x==0.0f && y==0.0f && z==0.0f) continue;
                const float inv = 1.0f/std::sqrt(l);
                const float nx=x*inv, ny=y*inv, nz=z*inv;
                g_fireHitOff[hits] = src*0x10000 + off;          // encode source + offset
                g_fireHitVec[hits*3+0]=nx; g_fireHitVec[hits*3+1]=ny; g_fireHitVec[hits*3+2]=nz;
                g_fireHitDot[hits] = haveCtrl ? (nx*cfx + ny*cfy + nz*cfz) : 0.0f;
                ++hits;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    g_fireHitCount = hits;
    // 1b) ★ TRANSFORM-ORIENTATION override: the raycast takes the shooter transform
    // r9 = *(rdx+0x10); its world orientation is a QUATERNION @+0xF0 (and local @+0xD0) -- this is
    // the actual shot direction (not a flat Vec3, so auto-scan missed it). Overwrite it with the
    // controller aim quat (shared[53..56], game space). g_fireXform: 0 off, 1 write +0xF0, 2 write
    // +0xD0, 3 write both. g_fireXformOff lets us scrub the quat offset if 0xF0 isn't it.
    // SAFETY: writing into *(rdx+0x10) HUNG the game -> it is a live entity/component, not a private
    // shot transform; mutating it corrupts physics/render. So this is now READ-ONLY: capture the quat
    // at the chosen offset for inspection ONLY (no write). g_fireXform != 0 just selects capture.
    if (g_fireXform && rdx) {
        __try {
            uintptr_t xf = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(rdx) + 0x10);
            if (xf > 0x10000 && xf < 0x7FFFFFFFFFFFull) {
                const float* cap = reinterpret_cast<const float*>(xf + g_fireXformOff);
                g_fireDir[0]=cap[0]; g_fireDir[1]=cap[1]; g_fireDir[2]=cap[2]; g_fireDir[3]=cap[3];
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    // 2) Bend-test / controller-override at the chosen override offset.
    uint8_t* ovrBase = WaFireResolve(g_fireOvrSrc, rcx, rdx, r8);
    if (ovrBase && g_fireMode != 0) {
        __try {
            float* d = reinterpret_cast<float*>(ovrBase + g_fireOvrOff);
            g_fireDir[0]=d[0]; g_fireDir[1]=d[1]; g_fireDir[2]=d[2]; g_fireDir[3]=d[3];
            if (g_fireMode == 1 && g_fireTestAng != 0.0f) {
                const float h = g_fireTestAng;
                const float sn = std::sin(h), cs = std::cos(h);
                const float x=d[0], y=d[1], z=d[2];
                float nx=x, ny=y, nz=z;
                if (g_firePlane == 0) { nx = x*cs - y*sn; ny = x*sn + y*cs; }
                else if (g_firePlane == 1) { nx = x*cs + z*sn; nz = -x*sn + z*cs; }
                else { ny = y*cs - z*sn; nz = y*sn + z*cs; }
                d[0]=nx; d[1]=ny; d[2]=nz;
                ++g_fireMutated;
            } else if (g_fireMode == 2 && haveCtrl) {
                const float sgn = g_fireNeg ? -1.0f : 1.0f;
                d[0]=cfx*sgn; d[1]=cfy*sgn; d[2]=cfz*sgn;
                ++g_fireMutated;
            }
            g_fireDirOut[0]=d[0]; g_fireDirOut[1]=d[1]; g_fireDirOut[2]=d[2]; g_fireDirOut[3]=d[3];
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    // ★ CAM-SNAP: the projectile launch (synchronous inside the shot) reads its direction from a
    // crosshair ORIENTATION PROVIDER = the camera. So for the shot's duration, force the FPP camera
    // WORLD orientation (g_ssCamPtr+0xF0) to the controller aim quat (shared[53..56]); the provider
    // reads the controller -> the bullet launches down the controller. Restore right after so the
    // VIEW only blips for the synchronous shot call (micro). g_fireCamSnap on.
    bool snapped = false; float savedQ[4] = {0,0,0,1}; float* camQ = nullptr;
    if (g_fireCamSnap && g_ssCamPtr && g_pSharedHands) {
        __try {
            const float qx=g_pSharedHands[53], qy=g_pSharedHands[54], qz=g_pSharedHands[55], qw=g_pSharedHands[56];
            const float ql = qx*qx+qy*qy+qz*qz+qw*qw;
            if (std::isfinite(ql) && ql > 0.25f) {
                const float inv = 1.0f/std::sqrt(ql);
                camQ = reinterpret_cast<float*>(g_ssCamPtr + g_fireCamSnapOff);
                savedQ[0]=camQ[0]; savedQ[1]=camQ[1]; savedQ[2]=camQ[2]; savedQ[3]=camQ[3];
                camQ[0]=qx*inv; camQ[1]=qy*inv; camQ[2]=qz*inv; camQ[3]=qw*inv;
                snapped = true; ++g_fireMutated;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { snapped = false; camQ = nullptr; }
    }
    // Bracket the shot window (counter, nest-safe) so the trace-dispatcher hook can gate to the
    // player bullet trace that fires synchronously inside the shot.
    ++g_fireInShot;
    if (OrigFire) OrigFire(rcx, rdx, r8, r9);
    --g_fireInShot;
    if (snapped && camQ) {
        __try { camQ[0]=savedQ[0]; camQ[1]=savedQ[1]; camQ[2]=savedQ[2]; camQ[3]=savedQ[3]; } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
}

// ============================================================================
// TRACE-DISPATCHER HOOK @0x1303EC -- the GENERIC physics-trace funnel (40+ callers: AI vision,
// cover, physics, AND the player bullet). TargetHelper and related shot paths compute
// dir = normalize(end - origin) then call this with a ray struct (arg5/rbx): [rbx+0x08] = origin,
// [rbx+0x18] = end, [rbx+0x28..2A] = flags. Hooking it unconditionally would break the game, so we
// GATE to the player-shot window (g_fireInShot, set by Hooked_Fire, AND
// g_shotInProgress from Hooked_Ss). During that window we (a) capture the return RVA (-> which of
// the 40 callers IS the bullet trace) + dump the ray struct, and (b) optionally OVERRIDE the end
// point = origin + controller_forward * dist, so the bullet flies down the controller. The ray
// origin/end may be float Vec3 or fixed-point WorldPosition; we dump raw dwords to disambiguate.
// ============================================================================
static constexpr uintptr_t kTraceOffset = 0x1303EC;
extern volatile uint64_t g_trShotCalls;       // dispatcher calls during the shot window
extern volatile int      g_trRetCount;        // distinct return RVAs captured
extern volatile uint32_t g_trRetRing[16];     // their RVAs
extern volatile uint32_t g_trCallerRay[16*12];// per-caller ray-struct (arg5) dump (12 dwords each)
extern volatile float    g_trCallerDir[16*4]; // per-caller arg3 (r8) = the DIRECTION vector dump
extern volatile uint32_t g_trCallerHits[16];  // how many times each caller fired this shot
extern volatile int      g_trOverride;        // 0 off, 1 override (ONLY the gated caller)
extern volatile uint32_t g_trGateRet;         // REQUIRED non-zero ret-RVA to override (no override-all)
extern volatile int      g_trWriteOff;        // byte offset in the ray struct (arg5) to write the unit dir
extern volatile int      g_trForce;           // 1 = write even if current value isn't a unit vector
extern volatile int      g_trNeg;
extern volatile uint64_t g_trOvrCount;
extern volatile int      g_fireInShot;
typedef void* (*TraceFn)(void*, void*, void*, void*, void*, void*);
static TraceFn OrigTrace = nullptr;

// arg3 (r8) turned out to be a POINTER not the direction (dumped as ~9e20 garbage). The DIRECTION
// lives in the ray struct (arg5). So we
// (a) dump all 12 ray dwords as float to LOCATE the unit-vector direction triple, and (b) OVERRIDE
// by writing the controller unit-forward to *(float*)(rayStruct + g_trWriteOff) (default 0x18),
// gated to one caller ret-RVA, only when the current value there is a finite ~unit vector (or
// g_trForce). All under SEH.
extern "C" inline void* Hooked_Trace(void* rcx, void* rdx, void* r8, void* r9, void* rayStruct, void* a6) {
    const bool inShot = (g_fireInShot != 0) || (g_shotInProgress != 0);
    if (inShot) {
        ++g_trShotCalls;
        const uintptr_t ret = reinterpret_cast<uintptr_t>(_ReturnAddress());
        const uintptr_t base = g_waExeBase;
        const uint32_t retRva = (base && ret >= base) ? static_cast<uint32_t>(ret - base) : 0;
        __try {
            int slot = -1;
            for (int i = 0; i < g_trRetCount && i < 16; ++i) if (g_trRetRing[i] == retRva) { slot = i; break; }
            if (slot < 0 && g_trRetCount < 16) { slot = g_trRetCount++; g_trRetRing[slot] = retRva; }
            if (slot >= 0) {
                ++g_trCallerHits[slot];
                if (rayStruct) { const uint32_t* rw = reinterpret_cast<const uint32_t*>(rayStruct);
                    for (int i = 0; i < 12; ++i) g_trCallerRay[slot*12 + i] = rw[i]; }
                if (r8) { const float* dir = reinterpret_cast<const float*>(r8);
                    for (int i = 0; i < 4; ++i) g_trCallerDir[slot*4 + i] = dir[i]; }
            }
            // OVERRIDE: only the gated caller, write unit controller-fwd at rayStruct+g_trWriteOff.
            if (g_trOverride && g_trGateRet != 0 && retRva == g_trGateRet && g_pSharedHands && rayStruct) {
                float fx=g_pSharedHands[60], fy=g_pSharedHands[61], fz=g_pSharedHands[62];
                const float l=fx*fx+fy*fy+fz*fz;
                if (std::isfinite(l) && l>0.1f) {
                    const float inv=(g_trNeg?-1.0f:1.0f)/std::sqrt(l); fx*=inv; fy*=inv; fz*=inv;
                    const int off = (g_trWriteOff >= 0 && g_trWriteOff <= 0x20) ? g_trWriteOff : 0x18;
                    float* w = reinterpret_cast<float*>(reinterpret_cast<uint8_t*>(rayStruct) + off);
                    const float wl = w[0]*w[0]+w[1]*w[1]+w[2]*w[2];
                    const bool isUnit = std::isfinite(wl) && wl > 0.5f && wl < 2.0f;
                    if (g_trForce || isUnit) { w[0]=fx; w[1]=fy; w[2]=fz; ++g_trOvrCount; }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return OrigTrace(rcx, rdx, r8, r9, rayStruct, a6);
}

inline bool InstallWeaponAimHooks() {
    HMODULE h = GetModuleHandleA("Cyberpunk2077.exe");
    if (!h) return false;
    g_waExeBase = reinterpret_cast<uintptr_t>(h);
    MH_Initialize(); // no-op if already initialized by the VRIK hooks

    // FIRE-SHOT direction lever: dump/bend/override [r8+0x80].
    void* fire = reinterpret_cast<void*>(g_waExeBase + kFireOffset);
    if (MH_CreateHook(fire, &Hooked_Fire, reinterpret_cast<void**>(&OrigFire)) == MH_OK) MH_EnableHook(fire);

    // TRACE-DISPATCHER funnel (gated to the player shot): capture caller + override end point.
    void* trace = reinterpret_cast<void*>(g_waExeBase + kTraceOffset);
    if (MH_CreateHook(trace, &Hooked_Trace, reinterpret_cast<void**>(&OrigTrace)) == MH_OK) MH_EnableHook(trace);

    // ShotSnap decouple hook (PlayerShotCaller -> sets g_shotInProgress).
    void* ss = reinterpret_cast<void*>(g_waExeBase + kSsOffset);
    if (MH_CreateHook(ss, &Hooked_Ss, reinterpret_cast<void**>(&OrigSs)) == MH_OK) MH_EnableHook(ss);

    // Camera-transform getter hook (THE lever): rewrite the output orientation to the
    // controller during the shot window.
    void* xf = reinterpret_cast<void*>(g_waExeBase + kXfOffset);
    if (MH_CreateHook(xf, &Hooked_Xf, reinterpret_cast<void**>(&OrigXf)) == MH_OK) MH_EnableHook(xf);

    // GetWorldOrientation getter @0x802390 (confirmed shot aim reader).
    void* go = reinterpret_cast<void*>(g_waExeBase + kGoOffset);
    if (MH_CreateHook(go, &Hooked_Go, reinterpret_cast<void**>(&OrigGo)) == MH_OK) MH_EnableHook(go);

    void* proj     = reinterpret_cast<void*>(g_waExeBase + kWaProjOffset);
    void* target   = reinterpret_cast<void*>(g_waExeBase + kWaTargetHelperOffset);
    void* classify = reinterpret_cast<void*>(g_waExeBase + kWaShotClassifyOffset);

    bool ok = true;
    if (MH_CreateHook(proj, &Hooked_WaProj, reinterpret_cast<void**>(&OrigWaProj)) != MH_OK) ok = false;
    else if (MH_EnableHook(proj) != MH_OK) ok = false;
    if (MH_CreateHook(target, &Hooked_WaTarget, reinterpret_cast<void**>(&OrigWaTarget)) != MH_OK) ok = false;
    else if (MH_EnableHook(target) != MH_OK) ok = false;
    if (MH_CreateHook(classify, &Hooked_WaClassify, reinterpret_cast<void**>(&OrigWaClassify)) != MH_OK) ok = false;
    else if (MH_EnableHook(classify) != MH_OK) ok = false;

    // Shot-direction lever: call-site patch on the Normalize(target-origin) at 0x46F0E5
    // (verifies it calls Normalize first; refuses on mismatch). This is the one that
    // controls the bullet for this build. The physics-trace patch below is the secondary
    // (hit/cover) trace -- kept for completeness.
    InstallWeaponNormPatch();
    InstallWeaponPhysPatch();

    // ROOT aim lever: crosshair-update hook (rewrites cache+0x370 direction -> weapon fwd).
    void* xhupd = reinterpret_cast<void*>(g_waExeBase + kWaXhUpdOffset);
    if (MH_CreateHook(xhupd, &Hooked_WaXhUpd, reinterpret_cast<void**>(&OrigWaXhUpd)) == MH_OK) MH_EnableHook(xhupd);

    // Heading-decouple hook: capture camObj + force heading offset.
    void* headfn = reinterpret_cast<void*>(g_waExeBase + kWaHeadOffset);
    if (MH_CreateHook(headfn, &Hooked_WaHead, reinterpret_cast<void**>(&OrigWaHead)) == MH_OK) MH_EnableHook(headfn);

    // Counter-only instrumentation on the shot-candidate / shot-vector family, to find
    // which functions actually fire on the player's shot for this build.
    void* candA = reinterpret_cast<void*>(g_waExeBase + kWaCandAOffset);
    void* candB = reinterpret_cast<void*>(g_waExeBase + kWaCandBOffset);
    void* svp   = reinterpret_cast<void*>(g_waExeBase + kWaSVPOffset);
    void* sfvw  = reinterpret_cast<void*>(g_waExeBase + kWaSFVWOffset);
    if (MH_CreateHook(candA, &Hooked_WaCandA, reinterpret_cast<void**>(&OrigWaCandA)) == MH_OK) MH_EnableHook(candA);
    if (MH_CreateHook(candB, &Hooked_WaCandB, reinterpret_cast<void**>(&OrigWaCandB)) == MH_OK) MH_EnableHook(candB);
    if (MH_CreateHook(svp,   &Hooked_WaSVP,   reinterpret_cast<void**>(&OrigWaSVP))   == MH_OK) MH_EnableHook(svp);
    if (MH_CreateHook(sfvw,  &Hooked_WaSFVW,  reinterpret_cast<void**>(&OrigWaSFVW))  == MH_OK) MH_EnableHook(sfvw);

    g_waInstalled = ok ? 1 : 0;
    return ok;
}
