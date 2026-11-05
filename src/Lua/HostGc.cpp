#include <stdafx.h>

#include "IdleGc.h"

#include <atomic>
#include <cstdint>
#include <algorithm>
#include <mutex>

// The plugin's log, one line (src/Core/Log.cpp).
extern "C" void CvrLogText(const char* text);

// THE HOST PACES THE MODS' COLLECTOR IN THE GAME. The Stereo mod used to step it before every onUpdate (a fixed
// budget, the collector stopped in between), so all of it ran on the game thread ahead of the world tick, the
// frame's limit when the game thread is. The game thread then waits ~9-12 ms of every frame for the world tick's
// jobs, most of it with nothing to run. Now, while the mod asks for it (VRLuaHostGc, only in the game; the menus
// keep the mod's pacing):
// - the collector's steps run in that wait, a slice at a time, whenever the game thread's dequeue comes back
//   empty (Stereo/GameThreadQueues.cpp), on the game thread, holding the state's lock and only when the VM runs
//   nothing (no call in progress, so nothing on the Lua stack is half made);
// - a cycle starts only once the heap has grown by CyberpunkVR_LuaIdleGcCycleKB (or an eighth of what the last
//   cycle left), and slices keep a gap between them: the mods' Observe callbacks run on the workers during the
//   world tick and take the same lock. Back-to-back slices with a cycle every frame (the first try) held it for
//   most of the wait and made the frame 17 -> 24 ms;
// - those slices stop before the finalizer phase: finalizers are the __gc of the userdata, and for the game
//   values the mods hold that is a release into the game, which only happens at the host's safe point, before
//   onUpdate, as before;
// - should the waits not give the collector enough time (a cycle running for more than ~2 s, or the heap grown
//   well past the last cycle's live size), the safe point steps it itself for up to 1 ms a frame.
// CyberpunkVR_LuaIdleGc = 0 gives the pacing back to the mod at its next onUpdate (live, for A/B).
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LuaIdleGc{1};
// [0] wait slices run, [1] their steps, [2] their QPC ticks, [3] slices given up (state locked by another thread,
// or the VM busy), [4] safe-point finalizer steps, [5] their ticks, [6] catch-up steps at the safe point, [7] their
// ticks, [8] cycles completed in the waits, [9] safe points with the host pacing.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_LuaIdleGcStats[10]{};
// Microseconds of collector work per slice in a wait (one atomic phase can take longer: it is a single step), the
// least time between two slices, and the growth that starts a cycle.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LuaIdleGcSliceUs{30};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LuaIdleGcGapUs{150};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LuaIdleGcCycleKB{2048};
}

namespace {

using StateData = TiltedPhoques::Lockable<sol::state, std::recursive_mutex>::Data;

std::recursive_mutex* g_mutex = nullptr;
lua_State* g_L = nullptr;
// Safe points counted, and the one at which the mod last asked for the host's pacing.
std::atomic<uint64_t> g_safePoints{0};
std::atomic<uint64_t> g_wanted{UINT64_MAX};
// Where the running cycle started (safe point) and the heap after the last completed one.
uint64_t g_cycleStart = 0;
uint64_t g_liveAfterCycle = 0;
uint64_t g_nextSlice = 0;
LARGE_INTEGER g_freq{};

constexpr int kPause = 0, kFinalize = 5;

uint64_t Now()
{
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    return static_cast<uint64_t>(t.QuadPart);
}

int UntilDeadline(void* ud)
{
    return Now() >= *static_cast<const uint64_t*>(ud) ? 1 : 0;
}

uint64_t Ticks(double us)
{
    return static_cast<uint64_t>(us * static_cast<double>(g_freq.QuadPart) / 1e6);
}

bool Active()
{
    if (!g_L || !CyberpunkVR_LuaIdleGc.load(std::memory_order_relaxed))
        return false;
    const uint64_t wanted = g_wanted.load(std::memory_order_relaxed);
    // Asked at the latest or the one before (the wait comes after the safe point of its frame).
    return wanted != UINT64_MAX && wanted + 1 >= g_safePoints.load(std::memory_order_relaxed);
}

void NoteCycleEnd()
{
    g_liveAfterCycle = cvr_lj_gc_total(g_L);
    g_cycleStart = g_safePoints.load(std::memory_order_relaxed);
}

}  // namespace

// From LuaVM::PostInitializeMods, holding the state's lock: the state the mods run in, and its lock. The view of the
// collector's fields that IdleGc.c has is checked against lua_gc's before anything uses it.
extern "C" void CvrLuaGcRegister(sol::state* apState)
{
    QueryPerformanceFrequency(&g_freq);
    lua_State* L = apState->lua_state();
    const uint64_t total = cvr_lj_gc_total(L);
    const uint64_t counted = static_cast<uint64_t>(lua_gc(L, LUA_GCCOUNT, 0)) * 1024u + static_cast<uint64_t>(lua_gc(L, LUA_GCCOUNTB, 0));
    const int stepmul = lua_gc(L, LUA_GCSETSTEPMUL, 200);
    lua_gc(L, LUA_GCSETSTEPMUL, stepmul);
    const bool layout = total == counted && cvr_lj_gc_stepmul(L) == static_cast<uint32_t>(stepmul);
    const bool idle = !cvr_lj_vm_busy(L);
    if (!layout || !idle)
    {
        char line[200];
        snprintf(line, sizeof(line), "[lua] host GC pacing off: layout %s (total %llu vs %llu), VM %s\n", layout ? "ok" : "MISMATCH",
                 static_cast<unsigned long long>(total), static_cast<unsigned long long>(counted), idle ? "idle" : "busy at load");
        CvrLogText(line);
        return;
    }
    // Lockable<T>::Data is { T Value; TLock Lock; }: the state is its first member.
    g_mutex = &reinterpret_cast<StateData*>(apState)->Lock;
    g_L = L;
    NoteCycleEnd();
    CvrLogText("[lua] host GC pacing ready\n");
}

// The mod's side (VRLuaHostGc): true = the host paces the collector this frame, and the mod leaves it alone.
extern "C" bool CvrLuaHostGcWanted(bool aWant)
{
    if (!aWant)
    {
        g_wanted.store(UINT64_MAX, std::memory_order_relaxed);
        return false;
    }
    g_wanted.store(g_safePoints.load(std::memory_order_relaxed), std::memory_order_relaxed);
    return Active();
}

// LuaVM::Update, on the game thread before the mods' onUpdate: the finalizers, and a catch-up when the waits gave the
// collector too little time.
extern "C" void CvrLuaGcSafePoint()
{
    const uint64_t point = g_safePoints.fetch_add(1, std::memory_order_relaxed) + 1;
    if (!g_L || !Active())
        return;
    std::scoped_lock lock(*g_mutex);
    if (cvr_lj_vm_busy(g_L))
        return;
    CyberpunkVR_LuaIdleGcStats[9].fetch_add(1, std::memory_order_relaxed);
    // The mod stopped the collector's own trigger; keep it so (a reload makes the state run it again).
    lua_gc(g_L, LUA_GCSTOP, 0);
    if (cvr_lj_gc_state(g_L) == kFinalize)
    {
        const uint64_t t0 = Now();
        uint64_t deadline = t0 + Ticks(500.0);
        const int steps = cvr_lj_gc_slice(g_L, 1, 0, &UntilDeadline, &deadline);
        CyberpunkVR_LuaIdleGcStats[4].fetch_add(steps, std::memory_order_relaxed);
        CyberpunkVR_LuaIdleGcStats[5].fetch_add(Now() - t0, std::memory_order_relaxed);
        if (cvr_lj_gc_state(g_L) == kPause)
            NoteCycleEnd();
    }
    const uint64_t total = cvr_lj_gc_total(g_L);
    const bool slow = cvr_lj_gc_state(g_L) != kPause && point - g_cycleStart > 240;
    const bool grown = total > g_liveAfterCycle + g_liveAfterCycle / 2 + (16u << 20);
    if (slow || grown)
    {
        const uint64_t t0 = Now();
        uint64_t deadline = t0 + Ticks(1000.0);
        const int steps = cvr_lj_gc_slice(g_L, 1, 1, &UntilDeadline, &deadline);
        CyberpunkVR_LuaIdleGcStats[6].fetch_add(steps, std::memory_order_relaxed);
        CyberpunkVR_LuaIdleGcStats[7].fetch_add(Now() - t0, std::memory_order_relaxed);
        if (cvr_lj_gc_state(g_L) == kPause)
            NoteCycleEnd();
    }
}

// The game thread's dequeue came back empty while it waits for the world tick (Stereo/GameThreadQueues.cpp): one
// slice of the collector's work, finalizers excepted.
extern "C" void CvrLuaIdleWork()
{
    if (!Active())
        return;
    const uint64_t now = Now();
    if (now < g_nextSlice)
        return;
    g_nextSlice = now + Ticks(static_cast<double>(CyberpunkVR_LuaIdleGcGapUs.load(std::memory_order_relaxed)));
    if (!g_mutex->try_lock())
    {
        CyberpunkVR_LuaIdleGcStats[3].fetch_add(1, std::memory_order_relaxed);
        return;
    }
    if (cvr_lj_vm_busy(g_L))
    {
        g_mutex->unlock();
        CyberpunkVR_LuaIdleGcStats[3].fetch_add(1, std::memory_order_relaxed);
        return;
    }
    const int before = cvr_lj_gc_state(g_L);
    if (before == kFinalize)
    {
        g_mutex->unlock();
        return;
    }
    if (before == kPause)
    {
        const uint64_t grow = (std::max)(static_cast<uint64_t>(CyberpunkVR_LuaIdleGcCycleKB.load(std::memory_order_relaxed)) << 10,
                                         g_liveAfterCycle / 8);
        if (cvr_lj_gc_total(g_L) < g_liveAfterCycle + grow)
        {
            g_mutex->unlock();
            return;
        }
    }
    const uint64_t t0 = Now();
    uint64_t deadline = t0 + Ticks(static_cast<double>(CyberpunkVR_LuaIdleGcSliceUs.load(std::memory_order_relaxed)));
    const int steps = cvr_lj_gc_slice(g_L, 0, 1, &UntilDeadline, &deadline);
    if (before != kPause && cvr_lj_gc_state(g_L) == kPause)
    {
        // A cycle with nothing to finalize ends in the sweep.
        NoteCycleEnd();
        CyberpunkVR_LuaIdleGcStats[8].fetch_add(1, std::memory_order_relaxed);
    }
    else if (before == kPause)
    {
        g_cycleStart = g_safePoints.load(std::memory_order_relaxed);
    }
    g_mutex->unlock();
    g_nextSlice = Now() + Ticks(static_cast<double>(CyberpunkVR_LuaIdleGcGapUs.load(std::memory_order_relaxed)));
    CyberpunkVR_LuaIdleGcStats[0].fetch_add(1, std::memory_order_relaxed);
    CyberpunkVR_LuaIdleGcStats[1].fetch_add(steps, std::memory_order_relaxed);
    CyberpunkVR_LuaIdleGcStats[2].fetch_add(Now() - t0, std::memory_order_relaxed);
}
