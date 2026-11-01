// GameThreadQueues -- the game thread does not run render jobs while it waits.
//
// The game thread waits for its frame's job counters through sub_1405D3CFC -> FlushCounter
// sub_14014AB30(dispatcher, counter, 1, -1, ...). While the counter is pending it takes jobs itself,
// through the dequeue sub_14026DE10 (FlushCounter's only use of it), which tries dispatcher queues 3, 2
// and 1 in that order. Render node jobs (RunRenderJob 0x276830) are queued on 1, the world tick's jobs on
// 2. Stack samples of the game thread in the city, with the deferred present on (Stereo/SubmitChain.cpp):
// 6.8 % of its time inside render nodes, ~0.9 ms a frame, taken while it waited for the world tick. A
// render node picked up just before the world tick's last job finishes holds the world tick open until
// the node is done, and the game thread is the frame's limit: the world tick got ~0.5 ms longer as soon
// as the next frame's render overlapped it more.
//
// With CyberpunkVR_GameThreadSkipRenderQueue on, the game thread's dequeue tries only queues 3 and 2. A
// job on queue 1 stays where it is for the fifteen workers, which are idle most of the frame; when
// nothing is found the job token is given back exactly as the original's empty path does. FlushCounter's
// returning of low-priority jobs (the path the earlier "return render jobs" test used, ~7200 requeues a
// frame) is never reached for them, because they are not taken in the first place.

#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <intrin.h>

extern "C" {
// 1 = the game thread leaves dispatcher queue 1 (render jobs) to the workers while it waits. Live for A/B.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GameThreadSkipRenderQueue{0};
// [0] dequeues, [1] jobs taken from queue 3, [2] from queue 2, [3] dequeues that found nothing on 3 and 2
// while queue 1 had jobs (left to the workers).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_GameThreadQueueStats[4]{};
}
// THE WAIT FOR THE WORLD TICK RUNS THE LUA HOST'S COLLECTOR. The world tick (sub_14069C788) hands its jobs to the
// dispatcher and waits for them in sub_1405D3CFC, the game thread taking jobs too; ~9-12 ms of a frame in the city,
// most of it with nothing to take. Inside that wait (and only there: the same dequeue serves every counter wait,
// some of them inside a mod's call into the game), an empty dequeue lends the time to the host's collector, one
// slice of ~60 us (src/Lua/HostGc.cpp: no finalizers, nothing while the VM runs, the state's lock taken or the slice
// skipped).
// Not done: PAUSE instructions in the game thread's empty path, to leave the core's other hardware thread
// to the worker there (FlushCounter's idle loop of QPC reads and PeekMessage is ~50 % of the game thread's
// samples). 0 / 64 / 256 PAUSEs per empty dequeue measured 13.66 / 13.63 / 13.65 ms (2026-10-17).

// The Lua host's collector slice (src/Lua/HostGc.cpp).
extern "C" void CvrLuaIdleWork();

namespace cvr {
namespace detail {
namespace {

using DequeueFn = char (*)(uint8_t*, void*, uint8_t*);
DequeueFn g_orig_dequeue = nullptr;
using WorldTickWaitFn = char (*)(int64_t*);
WorldTickWaitFn g_orig_world_tick_wait = nullptr;

constexpr uintptr_t GT_DEQUEUE_RVA = 0x26DE10;
// sub_1405D3CFC(counter): the game thread's wait for a counter, and the return address of its call in the world
// tick sub_14069C788 (0x14069C8C7, 5 bytes).
constexpr uintptr_t WORLD_TICK_WAIT_RVA = 0x5D3CFC;
constexpr uintptr_t WORLD_TICK_WAIT_RETURN_RVA = 0x69C8CC;
thread_local bool t_world_tick_wait = false;

char Detour_WorldTickWait(int64_t* counter) {
    const bool world_tick = g_exe_base &&
        reinterpret_cast<uintptr_t>(_ReturnAddress()) == reinterpret_cast<uintptr_t>(g_exe_base) + WORLD_TICK_WAIT_RETURN_RVA;
    if (!world_tick) return g_orig_world_tick_wait(counter);
    t_world_tick_wait = true;
    const char result = g_orig_world_tick_wait(counter);
    t_world_tick_wait = false;
    return result;
}

// The dispatcher's job queues as sub_14026DE10 reads them (relative to dispatcher+64): +1152 the count
// of queued jobs, +1160 the workers' semaphore; queue n's ring at 128 + 256*n: +32 entries (40 bytes),
// +40 capacity (a power of two), +64 read index, +128 write index.
constexpr size_t AVAILABLE = 1152, SEMAPHORE = 1160, RING_ENTRIES = 32, RING_CAPACITY = 40, RING_READ = 64, RING_WRITE = 128;
size_t Ring(int queue) { return 128 + 256 * static_cast<size_t>(queue); }

bool Pop(uint8_t* q, int queue, void* out) {
    uint8_t* ring = q + Ring(queue);
    auto* read = reinterpret_cast<volatile LONG64*>(ring + RING_READ);
    const LONG64 at = *read;
    if (static_cast<uint64_t>(at) >= *reinterpret_cast<volatile uint64_t*>(ring + RING_WRITE)) return false;
    const uint64_t mask = *reinterpret_cast<uint64_t*>(ring + RING_CAPACITY) - 1;
    const uint8_t* entry = *reinterpret_cast<uint8_t**>(ring + RING_ENTRIES) + 40 * (static_cast<uint64_t>(at) & mask);
    uint8_t copy[40];
    memcpy(copy, entry, sizeof(copy));
    if (InterlockedCompareExchange64(read, at + 1, at) != at) return false;
    memcpy(out, copy, sizeof(copy));
    return true;
}

bool Queued(uint8_t* q, int queue) {
    uint8_t* ring = q + Ring(queue);
    return *reinterpret_cast<volatile uint64_t*>(ring + RING_READ) < *reinterpret_cast<volatile uint64_t*>(ring + RING_WRITE);
}

char Dequeue(uint8_t* q, void* out, uint8_t* queue);

char Detour_GameThreadDequeue(uint8_t* q, void* out, uint8_t* queue) {
    const char taken = Dequeue(q, out, queue);
    if (!taken && t_world_tick_wait) CvrLuaIdleWork();
    return taken;
}

char Dequeue(uint8_t* q, void* out, uint8_t* queue) {
    if (!CyberpunkVR_GameThreadSkipRenderQueue.load(std::memory_order_relaxed)) return g_orig_dequeue(q, out, queue);
    CyberpunkVR_GameThreadQueueStats[0].fetch_add(1, std::memory_order_relaxed);
    auto* available = reinterpret_cast<volatile LONG*>(q + AVAILABLE);
    const LONG count = *available;
    if (count <= 0 || InterlockedCompareExchange(available, count - 1, count) != count) return 0;
    if (Pop(q, 3, out)) {
        *queue = 3;
        CyberpunkVR_GameThreadQueueStats[1].fetch_add(1, std::memory_order_relaxed);
        return 1;
    }
    if (Pop(q, 2, out)) {
        *queue = 2;
        CyberpunkVR_GameThreadQueueStats[2].fetch_add(1, std::memory_order_relaxed);
        return 1;
    }
    if (Queued(q, 1)) CyberpunkVR_GameThreadQueueStats[3].fetch_add(1, std::memory_order_relaxed);
    // Nothing taken: the token goes back, waking a worker if one is waiting for it.
    if (InterlockedExchangeAdd(available, 1) < 0) ReleaseSemaphore(*reinterpret_cast<HANDLE*>(q + SEMAPHORE), 1, nullptr);
    return 0;
}

}  // namespace

CVR_DETOUR("[gt-queues] game thread dequeue sub_14026DE10", GT_DEQUEUE_RVA, Detour_GameThreadDequeue, g_orig_dequeue)
CVR_DETOUR("[gt-queues] game thread counter wait sub_1405D3CFC", WORLD_TICK_WAIT_RVA, Detour_WorldTickWait, g_orig_world_tick_wait)

}  // namespace detail
}  // namespace cvr
