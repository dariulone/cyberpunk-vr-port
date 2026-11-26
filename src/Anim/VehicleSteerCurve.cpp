// THE ROAD WHEELS AGAINST THE CABIN WHEEL, for the car the player drives with a hand on the wheel.
//
// The hands turn the cabin wheel one to one (WheelGrab: the input is read off the cabin wheel's curve), and the car
// turns its road wheels from that same input. Most drive models take the "alternative" turn update, which squares
// the input -- a thumbstick's curve: measured on the 911, the hands at 20/40/80/120 degrees put the road wheels at
// 1.5/5.7/18.6/33.7 degrees. Too little of a real wheel's middle. The TweakDB flag off (a copied drive model, tried
// on 2026-11-17) answered linearly, 8.8 degrees at 20, which was too much -- and it also dropped the drive model's
// speed taper: the alternative update scales the turn by turnUpdateMid/MaxSpeedTurnMul above its speed thresholds
// (the 911: 45 % at 25 m/s), the linear one only by the car's own curve set, if it has one.
//
// So this writes the car's live physics instead, only while a hand owns the wheel of the car the player drives:
//   road = maxWheelTurnDeg * speed taper * c1(|n|) * (share + (1 - share) * |n|)
// where the vanilla alternative update gives maxWheelTurnDeg * speed taper * c1(|n|) * |n| (c1: the car's input
// curve, |n| without one). `share` = CyberpunkVR_WheelRoadLinearShare, 0.48 by default: 20 degrees of wheel ->
// 5 degrees of road on the 911, then 11 / 24 / 37, and the full lock still at the car's full 50.
//
// HOW. The physics copies its drive model at spawn (TweakDB edits never reach a live car), and its turn update
// (sub_14061F538 on 2.5) reads the copy every step. In the linear branch the target road angle is
//   sign(n) * maxWheelTurnDeg * c0(speed) * c1(|n|)
// slewed at wheelTurnMaxAdd/SubPerSecond. So the flag is cleared and maxWheelTurnDeg is rewritten each poll to
//   M = maxWheelTurnDeg * taper(speed) * (share + (1 - share) * |n|) / c0(speed)
// with taper() the alternative branch's own remap of the speed thresholds and c0 the car's speed curve evaluated
// by the game's own curve function. Nothing else is touched; the moment the hand lets go, the player leaves the
// seat, a menu opens or the car changes, the copy gets its own values back -- and the alternative update's smoothed
// input is set where the road wheels are, so they do not jump. NPC drivers and traffic never see the change.
//
// Every offset is the 2.5 build's (3.0.88) and checked against the code that uses it before anything is written;
// the live block is identified by its two owner pointers back to the vehicle. A mismatch leaves the car vanilla.

#include "Anim/VehicleSteerCurve.hpp"
#include "Camera/PlayerCameraState.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/StereoLog.hpp"
#include <algorithm>
#include <bit>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <windows.h>

extern "C" {
// Share of the turn that reaches the road wheels in proportion to the hands, 0..1. 1 = linear (20 degrees of wheel
// gave 8.8 on the 911 -- too much), 0 = the game's own squared curve (1.5). 0.48: 5 degrees at 20.
__declspec(dllexport) float CyberpunkVR_WheelRoadLinearShare = 0.48f;
// 0 = leave every car's steering exactly as the game has it.
__declspec(dllexport) int32_t CyberpunkVR_WheelRoadCurve = 1;
// Test read-out: applied, maxWheelTurnDeg (own), written M, |n|, c0, taper, road angle, own alternative flag.
__declspec(dllexport) volatile float CyberpunkVR_WheelRoadDebug[8] = {};
// Test read-out: the mounted vehicle object, as the plugin last saw it (0 when on foot).
__declspec(dllexport) volatile uint64_t CyberpunkVR_DebugMountedVehicle = 0;
}

namespace cvr::anim {
namespace {
// vehicle (vehicleBaseObject)
constexpr uintptr_t kVehPhysics = 0x2D0;     // the wheeled physics; found live, checked by its owner pointers
constexpr uintptr_t kVehInput = 0x278;       // the steering input the physics reads, -1..1
constexpr uintptr_t kVehState = 0x5F0;       // -> +0xC8 speed m/s, +0xCC speed / 10 (the speed curves' input)
constexpr uintptr_t kVehCurves = 0x2E0;      // -> +8 the car's curve set (none on most cars)
constexpr uintptr_t kVehCurvesOn = 0xA41;    // byte: the input curve c1 is used (else |n|)
// physics
constexpr uintptr_t kPhysOwnerA = 0x60, kPhysOwnerB = 0x108;
constexpr uintptr_t kPhysRoad = 0xF4;        // road wheel angle, degrees (the linear branch's slewed state)
constexpr uintptr_t kPhysAltState = 0xF8;    // the alternative branch's slewed input
constexpr uintptr_t kMaxDeg = 0xC50, kAlt = 0xC5C;
constexpr uintptr_t kBase = 0xC60, kMid = 0xC64, kTop = 0xC68, kMidMul = 0xC70, kTopMul = 0xC78;
constexpr uintptr_t kStateSpeed = 0xC8, kStateSpeedTenth = 0xCC;
// curve set: entries at +0x40, count at +0x4C, 0x40 bytes each, CName key first
constexpr uintptr_t kSetEntries = 0x40, kSetCount = 0x4C, kEntryStride = 0x40;
constexpr uint32_t kCurveEvalRva = 0x61FB14, kCurveValueRva = 0x62008C;

struct Site { uint32_t rva; const char* bytes; };
// The instructions that read each offset above, in the turn update and its helpers (2.5).
constexpr Site kSites[] = {
    {0x61F5A1, "488b9308010000"},       // mov rdx,[rbx+108h]          physics -> vehicle
    {0x61F5AF, "488bbaf0050000"},       // mov rdi,[rdx+5F0h]          vehicle -> state
    {0x61F5E5, "f3440f59a278020000"},   // mulss xmm12,[rdx+278h]      steering input
    {0x61F5F6, "f3440f109fcc000000"},   // movss xmm11,[rdi+0CCh]      speed / 10
    {0x61F709, "488b4360"},             // mov rax,[rbx+60h]           physics -> vehicle
    {0x61F70D, "80b8410a000000"},       // cmp byte [rax+0A41h],0      input curve on
    {0x61F71E, "80bb5c0c000000"},       // cmp byte [rbx+0C5Ch],0      alternative turn update
    {0x61F72B, "f3440f109fc8000000"},   // movss xmm11,[rdi+0C8h]      speed
    {0x61F965, "f30f10a3500c0000"},     // movss xmm4,[rbx+0C50h]      maxWheelTurnDeg
    {0x61FAAB, "f30f1193f4000000"},     // movss [rbx+0F4h],xmm2       road angle
    {kCurveEvalRva, "4883ec28418bd0488d05"},          // the curve lookup (key table follows)
    {kCurveValueRva, "4055488bec4883ec500fb65138"},  // the curve value
    {0x6E3964, "488b91e0020000"},       // mov rdx,[rcx+2E0h]          vehicle -> curve holder
};

using CurveValueFn = float(__fastcall*)(uintptr_t entry, float x);
CurveValueFn g_curveValue{};
uintptr_t g_curveKeys{};

bool SitesMatch() {
    const auto exe = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if (!exe) return false;
    for (const auto& site : kSites) {
        const auto* code = reinterpret_cast<const uint8_t*>(exe + site.rva);
        for (size_t i = 0; site.bytes[i * 2]; ++i) {
            unsigned value{};
            if (std::sscanf(site.bytes + i * 2, "%2x", &value) != 1 || code[i] != value) {
                Log("[steer-curve] the game's turn update is not the one this was written for (rva %X): cars keep "
                    "their own steering\n", site.rva);
                return false;
            }
        }
    }
    // lea rax,[rip+rel32] at +7: the table of the four curve names the turn update looks up
    int32_t rel{};
    std::memcpy(&rel, reinterpret_cast<const void*>(exe + kCurveEvalRva + 10), 4);
    g_curveKeys = exe + kCurveEvalRva + 14 + static_cast<intptr_t>(rel);
    g_curveValue = reinterpret_cast<CurveValueFn>(exe + kCurveValueRva);
    return true;
}
bool Ready() {
    static const bool ok = SitesMatch();
    return ok;
}

bool WriteU8Safe(uintptr_t addr, uint8_t value) {
    __try { *reinterpret_cast<uint8_t*>(addr) = value; return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
bool CurveValueSafe(uintptr_t entry, float x, float* out) {
    __try { *out = g_curveValue(entry, x); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
float ReadF(uintptr_t addr, float fallback = 0.0f) {
    float v{};
    return ReadFloatSafe(addr, &v) && std::isfinite(v) ? v : fallback;
}

// The car's curve `index` (0 speed taper, 1 input) at x, as the turn update reads it; false without that curve.
bool CarCurve(uintptr_t set, uint32_t index, float x, float* out) {
    uint64_t key{};
    uintptr_t entries{};
    uint32_t count{};
    if (!ReadU64Safe(g_curveKeys + 8 * index, &key) || !ReadPtrSafe(set + kSetEntries, &entries) ||
        !ReadU32Safe(set + kSetCount, &count) || !entries || count > 64)
        return false;
    for (uint32_t i = 0; i < count; ++i) {
        uint64_t k{};
        if (ReadU64Safe(entries + kEntryStride * i, &k) && k == key) {
            float v{};
            if (!CurveValueSafe(entries + kEntryStride * i, x, &v) || !std::isfinite(v)) return false;
            *out = std::clamp(v, 0.0f, 1.0f);
            return true;
        }
    }
    return false;
}

// sub_14061CAE4: a to b across [lo, hi], clamped; the bounds are put in order first.
float Remap(float x, float lo, float hi, float a, float b) {
    if (hi < lo) { std::swap(lo, hi); std::swap(a, b); }
    if (x <= lo) return a;
    if (x < hi) return (x - lo) / (hi - lo) * (b - a) + a;
    return b;
}
// The alternative turn update's speed taper, from the copied thresholds.
float SpeedTaper(uintptr_t phys, float speed) {
    const float base = ReadF(phys + kBase), mid = ReadF(phys + kMid), top = ReadF(phys + kTop);
    const float midMul = ReadF(phys + kMidMul, 1.0f), topMul = ReadF(phys + kTopMul, 1.0f);
    if (mid == 0.0f) return Remap(speed, base, top, 1.0f, topMul);
    if (speed > mid) return Remap(speed, mid, top, midMul, topMul);
    return Remap(speed, base, mid, 1.0f, midMul);
}

struct Patch {
    uintptr_t vehicle{}, phys{};
    float ownMax{}, written{}, taper{1.0f};
    uint8_t ownAlt{};
    bool applied{}, refused{}, logged{};
};
Patch g_patch;
std::mutex g_mutex;

bool Owned(const Patch& p) {
    uintptr_t a{}, b{};
    return p.phys && ReadPtrSafe(p.phys + kPhysOwnerA, &a) && ReadPtrSafe(p.phys + kPhysOwnerB, &b) &&
           a == p.vehicle && b == p.vehicle;
}
// Our values still in the block: nothing has replaced or freed it since the last write.
bool StillOurs(const Patch& p) {
    float m{};
    uint8_t alt{};
    return Owned(p) && ReadFloatSafe(p.phys + kMaxDeg, &m) && ReadU8Safe(p.phys + kAlt, &alt) && alt == 0 &&
           std::bit_cast<uint32_t>(m) == std::bit_cast<uint32_t>(p.written);
}

void Restore(Patch& p) {
    if (!p.applied) return;
    p.applied = false;
    CyberpunkVR_WheelRoadDebug[0] = 0.0f;
    if (!StillOurs(p)) {
        Log("[steer-curve] %p: the car's physics changed under the curve; nothing put back\n",
            reinterpret_cast<void*>(p.vehicle));
        return;
    }
    if (p.ownAlt) {
        // The alternative branch steers from its own slewed input: start it where the road wheels are now.
        const float road = ReadF(p.phys + kPhysRoad);
        WriteFloatSafe(p.phys + kPhysAltState, std::clamp(road / (p.ownMax * std::max(p.taper, 0.05f)), -1.0f, 1.0f));
    }
    WriteFloatSafe(p.phys + kMaxDeg, p.ownMax);
    WriteU8Safe(p.phys + kAlt, p.ownAlt);
}

bool Take(Patch& p) {
    uintptr_t phys{};
    if (!ReadPtrSafe(p.vehicle + kVehPhysics, &phys) || !phys) return false;
    p.phys = phys;
    float own{};
    uint8_t alt{};
    if (!Owned(p) || !ReadFloatSafe(phys + kMaxDeg, &own) || !std::isfinite(own) || own < 5.0f || own > 120.0f ||
        !ReadU8Safe(phys + kAlt, &alt) || alt > 1) {
        Log("[steer-curve] %p: no drive model where expected (physics %p); this car keeps its own steering\n",
            reinterpret_cast<void*>(p.vehicle), reinterpret_cast<void*>(phys));
        p.phys = 0;
        return false;
    }
    p.ownMax = own;
    p.ownAlt = alt;
    if (!p.logged) {
        p.logged = true;
        Log("[steer-curve] %p: wheel curve on (maxWheelTurnDeg %.1f, alternative turn update %d)\n",
            reinterpret_cast<void*>(p.vehicle), own, int(alt));
    }
    return true;
}
}  // namespace

void VehicleSteerCurveTick(bool held) {
    std::unique_lock lock(g_mutex, std::try_to_lock);
    if (!lock.owns_lock()) return;
    const uintptr_t mounted = CurrentMountedVehicleIdentity();
    CyberpunkVR_DebugMountedVehicle = mounted;
    Patch& p = g_patch;
    const uintptr_t vehicle = held && CyberpunkVR_WheelRoadCurve && Ready() ? mounted : 0;
    if (p.vehicle && p.vehicle != vehicle) Restore(p);
    if (!vehicle) {
        if (p.vehicle != mounted) p = {};   // keep the refusal while seated in the same car
        return;
    }
    if (p.vehicle != vehicle) { p = {}; p.vehicle = vehicle; }
    if (p.refused) return;
    if (!p.applied) {
        if (!Take(p)) { p.refused = true; return; }
        p.applied = true;
    } else if (!StillOurs(p)) {
        float m{};
        if (!Owned(p)) {
            Log("[steer-curve] %p: the car's physics changed under the curve; letting go of it\n",
                reinterpret_cast<void*>(p.vehicle));
            p.applied = false;
            p.refused = true;
            return;
        }
        // The game set its drive model again (our maxWheelTurnDeg gone): those are the car's own values now.
        if (ReadFloatSafe(p.phys + kMaxDeg, &m) && std::bit_cast<uint32_t>(m) != std::bit_cast<uint32_t>(p.written) &&
            !Take(p)) {
            p.applied = false;
            p.refused = true;
            return;
        }
    }
    const float n = std::clamp(std::fabs(ReadF(vehicle + kVehInput)), 0.0f, 1.0f);
    uintptr_t state{};
    float speed = 0.0f, tenth = 0.0f;
    if (ReadPtrSafe(vehicle + kVehState, &state) && state) {
        speed = std::fabs(ReadF(state + kStateSpeed));
        tenth = std::clamp(ReadF(state + kStateSpeedTenth), -1.0f, 1.0f);
    }
    float c0 = 1.0f;
    uintptr_t holder{}, set{};
    if (ReadPtrSafe(vehicle + kVehCurves, &holder) && holder && ReadPtrSafe(holder + 8, &set) && set) {
        float v{};
        if (CarCurve(set, 0, tenth, &v)) c0 = v;
    }
    const float share = std::clamp(CyberpunkVR_WheelRoadLinearShare, 0.0f, 1.0f);
    p.taper = SpeedTaper(p.phys, speed);
    float m = p.ownMax * p.taper * (share + (1.0f - share) * n) / std::max(c0, 0.05f);
    m = std::clamp(m, 0.5f, p.ownMax * 8.0f);
    WriteU8Safe(p.phys + kAlt, 0);
    WriteFloatSafe(p.phys + kMaxDeg, m);
    p.written = m;
    CyberpunkVR_WheelRoadDebug[0] = 1.0f;
    CyberpunkVR_WheelRoadDebug[1] = p.ownMax;
    CyberpunkVR_WheelRoadDebug[2] = m;
    CyberpunkVR_WheelRoadDebug[3] = n;
    CyberpunkVR_WheelRoadDebug[4] = c0;
    CyberpunkVR_WheelRoadDebug[5] = p.taper;
    CyberpunkVR_WheelRoadDebug[6] = ReadF(p.phys + kPhysRoad);
    CyberpunkVR_WheelRoadDebug[7] = p.ownAlt;
}

}  // namespace cvr::anim
