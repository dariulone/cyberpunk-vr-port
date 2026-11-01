// SubmitChain -- render-graph submission points that do not hold up the rest of the graph.
//
// A render graph hands its recorded command lists to the GPU at submission points. The Synchronize
// node (sub_140655FAC -> sub_14058DC98) and three engine jobs (Submit_DrawBuffers, Submit_FrameTick,
// Submit_RetirePendingResources) all call sub_1422EF830(name, range, builder, fences). It moves
// the range's command lists into a batch and schedules into the caller's job-stage builder the Close
// jobs, then the Submit job (ExecuteCommandLists and the queue fences, sub_14039DB40), then for a
// two-queue sync JoinFrameSync. The caller's builder links its last stage to the caller's job counter,
// so a Synchronize node completes only after its Submit, and the next Synchronize waits for that. By
// the time a view graph reaches its last submission points all recording is done, so each graph ends
// in a serial Synchronize -> Close -> Submit -> Synchronize ... run on one or two workers: 2.24 ms in
// VRCAM, 1.90 ms in MAIN (docs/gpu-not-full-20261016.md).
// 2.31's submission point (sub_1401FAFA4) took a mode before the builder (name, range, mode, builder, fences);
// 2.5 dropped it. The remap's prologue signature had put the detour on sub_140329AD0, an unrelated function
// whose second argument is a float: it then wrote through the old arguments and crashed on the first frame.
//
// With CyberpunkVR_SubmitChain on, every submission point gets a builder of its own (sub_140409858).
// Its first stage waits for the previous submission point's last stage, so the queues get the batches
// in the original order, and for whatever the caller's builder was waiting for. Nothing else waits for
// its last stage, so a Synchronize node completes when its own CPU part is done and the next one runs
// while the previous batch is closed and submitted. A job queued after each last stage counts the
// submitted points, and what needs everything submitted waits for that count:
// - the Present node (sub_140BF60F0) of the view that presents, before it presents; the port's
//   present-time copies run inside that call;
// - mode 1 only: every view job, at its builder setup (sub_140B603D8), so MAIN's graph still starts
//   after everything VRCAM submitted, as before. Mode 2 lets MAIN's graph start while VRCAM's last
//   batches are still being submitted.
// Switching it off drains the chain before the next submission point runs with the caller's builder.
//
// DEFERRED PRESENT (CyberpunkVR_SubmitChainDeferPresent, on with the chain). The presenting view's Present
// node is the last node of MAIN's graph, and the game thread waits for MAIN's whole graph before it submits
// the next frame (the render scene's handle, scene+8). Inside that node went ~0.55 ms of waiting for the
// last batches to be submitted and ~0.7 ms of the DXGI Present with the port's present-time work, 1.3-2.0
// ms in all, during which nothing of the next frame could start (GPU queue timeline,
// tools/hitch_tests/gpu_chain_overlay.py). With the deferral the node only takes what it needs from the
// view's render data (the output object at +3976 and the output handle at +416), clears the device's
// per-frame flag the node clears, and queues the rest of the node -- the device's output setters, the
// present (sub_1405D95B8: DXGI Present, the present fence, the wait for the next back buffer, which back
// buffer the next frame draws into) and the window refresh -- as a job after the last submission point.
// The job's counter becomes the chain's tail, so the next frame's first batch reaches the queue after
// the present (and after the port's present-time copies, which read this frame's images). Every view job
// of the next frame waits for the job at its builder setup, and so does every render-packet run
// (sub_140334CC0, CyberpunkVR_WaitDeferredPresentForPackets), whichever job it is in: the present picks
// the presenting view's back buffer, and the port's present-time work must not run beside render nodes.
// What overlaps is the frame submit, the frame tick and the proxy flush. A Present node waits for the previous deferred present, so
// at most one is in flight. Only in the steady state (see kSteadyFrames); otherwise the node presents as
// before.
//
// The pause crash it was switched off for (2026-10-17): a render node's packet clear (sub_14095B35C, from
// 0x229D4C1) read an external texture whose object had been freed and its memory reused, in three of four
// pause soaks with the deferral on, while no render packet ran beside a present
// (CyberpunkVR_DeferredPresentPacketWaits stayed 0). The node was the second view's DrawComposition,
// walking the composition object the port lent it without a count of its own; the deferral only made the
// game thread's next UI frame overlap that walk more often. With the lent object held
// (CyberpunkVR_CompLendFrameHold, Stereo/ViewReuse.cpp) 25 pauses passed with the deferral on and every
// async compute pass on the graphics queue, so it is on again. Gain: 0.2-0.5 ms.

#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Utils/StereoLog.hpp"
#include <windows.h>
#include <atomic>
#include <cstdint>
#include <intrin.h>

extern "C" {
// 0 off, 1 chained (every view job also waits for the chain), 2 chained (only Present waits). Set from
// xr_submit_chain (src/Core/LiveControlsPoll.cpp) when the setting changes; writable live for A/B tests.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmitChain{0};
// [0] submission points chained, [1] of them submitted, [2] Present waits that had to wait, [3] their
// QPC ticks, [4] view-job waits that had to wait, [5] their ticks, [6] waits that gave up after 500 ms,
// [7] drains after switching off.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SubmitChainStats[8]{};
// The dispatcher queue (0..3, 3 served first) the chained Close/Submit/JoinFrameSync jobs go to; -1 keeps
// the caller's (render nodes run from queue 1, the world tick's jobs from queue 2). Live for A/B tests.
__declspec(dllexport) std::atomic<int32_t> CyberpunkVR_SubmitChainPriority{-1};
// 1 = the presenting view's Present node queues the present after the chain (see DEFERRED PRESENT); only
// with the chain on. Set from xr_defer_present (default 1); the pause crash it was once switched off for is
// closed (see DEFERRED PRESENT), and on 2.5 it passed 25 pause-menu round trips with a camera scope drawn.
// Live for A/B tests.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SubmitChainDeferPresent{0};
// [0] presents deferred, [1] of them done, [2] Present nodes that waited for the previous one, [3] their
// QPC ticks, [4] view jobs that waited for the deferred present, [5] their ticks, [6] presents run in the
// node (not yet steady: start-up, menus, loading, a changed output), [7] waits that gave up after 500 ms.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DeferredPresentStats[8]{};
// The steady-state gate, last presenting node: [0] presents with the same output, [1] presents since a
// graph last ran the node without presenting, [2] output changes, [3] output handle changes.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DeferredPresentGate[4]{};
// Render-packet runs (sub_140334CC0) that waited for a deferred present: [0] count, [1] QPC ticks.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DeferredPresentPacketWaits[2]{};
}

namespace cvr {
namespace detail {
namespace {

using SubmitPointFn = void (*)(uintptr_t, void*, uint8_t*, void*);
using BuilderInitFn = void* (*)(void*, uint8_t, uint8_t, uintptr_t);
using BuilderFinishFn = void** (*)(void*, void**);
using LinkFn = void (*)(void*, void*);
using ReleaseFn = void (*)(void**);
using ScheduleFn = int (*)(uintptr_t, const void*, uint8_t, void*, void*);
using PresentNodeFn = void (*)(void*, uint8_t*, void*);
using ViewBuilderFn = void* (*)(void*, void*, int64_t*);
using CounterNewFn = void* (*)(void*, uintptr_t);
using DeviceOutputFn = void (*)(uintptr_t, uint64_t);
using XefgPresentIdFn = void (*)(uintptr_t);
using SwapchainPresentFn = uintptr_t (*)(const uint32_t*, uintptr_t, uint32_t);

constexpr uintptr_t BUILDER_INIT_RVA = 0x409858;     // (builder, prio, prio2, tag): fresh after/done counters
constexpr uintptr_t BUILDER_FINISH_RVA = 0x40993C;   // (builder, &last): hands over the last stage's counter
constexpr uintptr_t COUNTER_LINK_RVA = 0x272E1C;     // (src, dst): dst stays pending until src completes
constexpr uintptr_t COUNTER_RELEASE_RVA = 0x2744CC;  // (&counter): drops one reference
constexpr uintptr_t JOB_SCHEDULE_RVA = 0x272B40;     // (dispatcher, job, prio, after, done)
constexpr uintptr_t DISPATCHER_RVA = 0x3BCDBF8;
constexpr uintptr_t COUNTER_NEW_RVA = 0x273008;       // (unused, tag): a counter with one reference
constexpr uintptr_t RENDER_DEVICE_RVA = 0x3BBCAE8;    // the render device the Present node calls
constexpr uintptr_t XEFG_PRESENT_ID_RVA = 0x229D9C0;  // (device+18048): XeSS frame generation present id
constexpr uintptr_t SWAPCHAIN_PRESENT_RVA = 0x5D95B8; // (&swapchain id, -, sync interval)

// Builder (sub_140409858 / sub_1402746D8): +8 tag, +16 the counter the next stage waits for, +24 the
// current stage's counter, +40 priority, +41 second priority byte. Counter: +24 pending.
constexpr size_t BUILDER_TAG = 8, BUILDER_AFTER = 16, BUILDER_PRIO = 40, BUILDER_PRIO2 = 41;
constexpr size_t COUNTER_PENDING = 24;

struct EngineJob {
    void (*fn)(void*, void*);
    void* data;
    const void* name;
    uint64_t misc;
};

SubmitPointFn g_orig_submit_point = nullptr;
PresentNodeFn g_orig_present_node = nullptr;
ViewBuilderFn g_orig_view_builder = nullptr;

SRWLOCK g_lock = SRWLOCK_INIT;
void* g_tail = nullptr;  // one reference to the last stage of the last chained submission point
std::atomic<uint64_t> g_issued{0}, g_submitted{0};
std::atomic<bool> g_timeout_logged{false};

// The deferred present (see DEFERRED PRESENT). One slot per sequence number modulo 4; at most one in flight.
struct DeferredPresent {
    uint8_t* output;  // render data +3976
    uint64_t handle;  // render data +416
    uint64_t seq;
};
DeferredPresent g_deferred[4]{};
std::atomic<uint64_t> g_present_issued{0}, g_present_done{0};
std::atomic<void*> g_present_view{nullptr};  // the view object of the presenting view's graph
thread_local bool t_in_deferred_present = false;

uintptr_t Base() { return reinterpret_cast<uintptr_t>(g_exe_base); }
template <typename Fn> Fn Engine(uintptr_t rva) { return reinterpret_cast<Fn>(Base() + rva); }

bool Pending(void* counter) {
    return counter && *reinterpret_cast<volatile int32_t*>(static_cast<uint8_t*>(counter) + COUNTER_PENDING) != 0;
}

bool ChainPending() { return g_submitted.load(std::memory_order_acquire) < g_issued.load(std::memory_order_acquire); }

void MarkSubmitted(void* data, void*) {
    const uint64_t seq = reinterpret_cast<uint64_t>(data);
    uint64_t seen = g_submitted.load(std::memory_order_relaxed);
    while (seen < seq && !g_submitted.compare_exchange_weak(seen, seq, std::memory_order_release, std::memory_order_relaxed)) {}
    CyberpunkVR_SubmitChainStats[1].fetch_add(1, std::memory_order_relaxed);
}

// Spins until every submission point chained so far has been submitted. Returns the QPC ticks spent.
int64_t WaitSubmitted() {
    const uint64_t target = g_issued.load(std::memory_order_acquire);
    if (g_submitted.load(std::memory_order_acquire) >= target) return 0;
    static const int64_t give_up = [] { LARGE_INTEGER f; QueryPerformanceFrequency(&f); return f.QuadPart / 2; }();
    const int64_t t0 = prof_now();
    for (uint32_t i = 0; g_submitted.load(std::memory_order_acquire) < target; ++i) {
        if (i < 64) _mm_pause();
        else SwitchToThread();
        if ((i & 255) == 255 && prof_now() - t0 > give_up) {
            CyberpunkVR_SubmitChainStats[6].fetch_add(1, std::memory_order_relaxed);
            if (!g_timeout_logged.exchange(true))
                cvr::log("[submit-chain] chain stalled: %llu of %llu submission points submitted after 500 ms",
                    static_cast<unsigned long long>(g_submitted.load()), static_cast<unsigned long long>(target));
            break;
        }
    }
    return prof_now() - t0;
}

bool PresentPending() { return g_present_done.load(std::memory_order_acquire) < g_present_issued.load(std::memory_order_acquire); }

// Spins until the last deferred present has run. Returns the QPC ticks spent.
int64_t WaitPresented() {
    if (!PresentPending() || t_in_deferred_present) return 0;
    static const int64_t give_up = [] { LARGE_INTEGER f; QueryPerformanceFrequency(&f); return f.QuadPart / 2; }();
    const int64_t t0 = prof_now();
    for (uint32_t i = 0; PresentPending(); ++i) {
        if (i < 64) _mm_pause();
        else SwitchToThread();
        if ((i & 255) == 255 && prof_now() - t0 > give_up) {
            CyberpunkVR_DeferredPresentStats[7].fetch_add(1, std::memory_order_relaxed);
            cvr::log("[submit-chain] deferred present %llu not done after 500 ms",
                static_cast<unsigned long long>(g_present_issued.load()));
            break;
        }
    }
    return prof_now() - t0;
}

void Detour_SubmitPoint(uintptr_t name, void* range, uint8_t* builder, void* fences) {
    if (!CyberpunkVR_SubmitChain.load(std::memory_order_relaxed) || !builder) {
        if (ChainPending()) {
            CyberpunkVR_SubmitChainStats[7].fetch_add(1, std::memory_order_relaxed);
            WaitSubmitted();
        }
        WaitPresented();
        g_orig_submit_point(name, range, builder, fences);
        return;
    }
    alignas(16) uint8_t own[64] = {};
    const int32_t forced = CyberpunkVR_SubmitChainPriority.load(std::memory_order_relaxed);
    const uint8_t priority = forced >= 0 && forced <= 3 ? static_cast<uint8_t>(forced) : builder[BUILDER_PRIO];
    AcquireSRWLockExclusive(&g_lock);
    Engine<BuilderInitFn>(BUILDER_INIT_RVA)(own, priority, builder[BUILDER_PRIO2],
                                            *reinterpret_cast<uintptr_t*>(builder + BUILDER_TAG));
    void* first = *reinterpret_cast<void**>(own + BUILDER_AFTER);
    void* caller_after = *reinterpret_cast<void**>(builder + BUILDER_AFTER);
    const auto link = Engine<LinkFn>(COUNTER_LINK_RVA);
    if (Pending(g_tail)) link(g_tail, first);
    if (Pending(caller_after)) link(caller_after, first);
    g_orig_submit_point(name, range, own, fences);
    void* last = nullptr;
    Engine<BuilderFinishFn>(BUILDER_FINISH_RVA)(own, &last);
    const uint64_t seq = g_issued.load(std::memory_order_relaxed) + 1;
    if (last) {
        const EngineJob job{&MarkSubmitted, reinterpret_cast<void*>(seq), nullptr, 0};
        Engine<ScheduleFn>(JOB_SCHEDULE_RVA)(*reinterpret_cast<uintptr_t*>(Base() + DISPATCHER_RVA), &job, own[BUILDER_PRIO], last, nullptr);
    }
    g_issued.store(seq, std::memory_order_release);
    if (!last) MarkSubmitted(reinterpret_cast<void*>(seq), nullptr);
    if (g_tail) Engine<ReleaseFn>(COUNTER_RELEASE_RVA)(&g_tail);
    g_tail = last;
    ReleaseSRWLockExclusive(&g_lock);
    CyberpunkVR_SubmitChainStats[0].fetch_add(1, std::memory_order_relaxed);
}

// The same test the Present node makes before it presents: the view's render data (view vfunc +32)
// has an output at +3976 and its present flag at +336. VRCAM's graph runs this node too, with neither.
bool Presents(uint8_t* context) {
    __try {
        auto* view = *reinterpret_cast<uint8_t**>(context);
        const auto table = *reinterpret_cast<uintptr_t**>(view);
        auto* data = reinterpret_cast<uint8_t* (*)(void*)>(table[4])(view);
        return data && *reinterpret_cast<void**>(data + 0xF98) && data[336];
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return true;
    }
}

// The rest of the Present node sub_140BF60F0 after its checks, with what it read from the render data.
void RunPresent(uint8_t* output, uint64_t handle) {
    const uintptr_t device = *reinterpret_cast<uintptr_t*>(Base() + RENDER_DEVICE_RVA);
    const auto table = *reinterpret_cast<uintptr_t**>(device);
    reinterpret_cast<DeviceOutputFn>(table[56 / 8])(device, handle);
    reinterpret_cast<DeviceOutputFn>(table[0x80 / 8])(device, handle);
    reinterpret_cast<DeviceOutputFn>(table[0xA8 / 8])(device, handle);
    Engine<XefgPresentIdFn>(XEFG_PRESENT_ID_RVA)(*reinterpret_cast<uintptr_t*>(device + 0x4678));
    if (!output[0x17C]) {
        Engine<SwapchainPresentFn>(SWAPCHAIN_PRESENT_RVA)(reinterpret_cast<const uint32_t*>(output + 56), 0,
                                                          *reinterpret_cast<uint32_t*>(output + 0x78));
        InvalidateRect(*reinterpret_cast<HWND*>(*reinterpret_cast<uintptr_t*>(output + 48) + 8), nullptr, TRUE);
    }
}

void DeferredPresentJob(void* data, void*) {
    auto* d = static_cast<DeferredPresent*>(data);
    t_in_deferred_present = true;
    RunPresent(d->output, d->handle);
    t_in_deferred_present = false;
    g_present_done.store(d->seq, std::memory_order_release);
    CyberpunkVR_DeferredPresentStats[1].fetch_add(1, std::memory_order_relaxed);
}

// What the Present node reads before it presents; false when the view does not present.
bool PresentTarget(uint8_t* context, uint8_t** output, uint64_t* handle) {
    __try {
        auto* view = *reinterpret_cast<uint8_t**>(context);
        const auto table = *reinterpret_cast<uintptr_t**>(view);
        auto* data = reinterpret_cast<uint8_t* (*)(void*)>(table[4])(view);
        if (!data || !*reinterpret_cast<uint8_t**>(data + 0xF98) || !data[336]) return false;
        *output = *reinterpret_cast<uint8_t**>(data + 0xF98);
        *handle = *reinterpret_cast<uint64_t*>(data + 416);
        g_present_view.store(view, std::memory_order_relaxed);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

void DeferPresent(uint8_t* output, uint64_t handle) {
    if (PresentPending()) {
        const int64_t ticks = WaitPresented();
        CyberpunkVR_DeferredPresentStats[2].fetch_add(1, std::memory_order_relaxed);
        CyberpunkVR_DeferredPresentStats[3].fetch_add(static_cast<uint64_t>(ticks), std::memory_order_relaxed);
    }
    // The node clears this per-frame device flag after it presents. It belongs to this frame, which has
    // finished recording, so it is cleared now rather than after the next frame may have set it again.
    const uintptr_t device = *reinterpret_cast<uintptr_t*>(Base() + RENDER_DEVICE_RVA);
    *reinterpret_cast<uint8_t*>(*reinterpret_cast<uintptr_t*>(device + 18008) + 498) = 0;
    const uint64_t seq = g_present_issued.load(std::memory_order_relaxed) + 1;
    DeferredPresent& slot = g_deferred[seq & 3];
    slot = {output, handle, seq};
    AcquireSRWLockExclusive(&g_lock);
    void* counter = Engine<CounterNewFn>(COUNTER_NEW_RVA)(nullptr, 0);
    const EngineJob job{&DeferredPresentJob, &slot, nullptr, 0};
    g_present_issued.store(seq, std::memory_order_release);
    Engine<ScheduleFn>(JOB_SCHEDULE_RVA)(*reinterpret_cast<uintptr_t*>(Base() + DISPATCHER_RVA), &job, 3, g_tail, counter);
    if (g_tail) Engine<ReleaseFn>(COUNTER_RELEASE_RVA)(&g_tail);
    g_tail = counter;
    ReleaseSRWLockExclusive(&g_lock);
    CyberpunkVR_DeferredPresentStats[0].fetch_add(1, std::memory_order_relaxed);
}

// The present is deferred only in the steady state: another graph ran the node without presenting (VRCAM,
// so the game is in the world) in this or the previous frame, and the output object has been the same for
// kSteadyFrames presents (its handle at +416 is not: it changes from frame to frame). At start-up the first presents also bring up overlays hooked on the
// DXGI Present (CET builds its D3D12 overlay there); one of those, deferred into the next frame's start,
// crashed inside the driver. Menus, loading screens and output changes present in the node as before.
constexpr uint64_t kSteadyFrames = 600;
uint8_t* g_last_output = nullptr;  // these three: the presenting view's node only, one frame at a time
uint64_t g_last_handle = 0;
uint64_t g_steady = 0;
std::atomic<uint64_t> g_presenting{0}, g_secondary{0};  // presenting nodes run; g_presenting when a graph last ran it without presenting

void Detour_PresentNode(void* node, uint8_t* context, void* job) {
    if (context && (context[48] & 2) && CyberpunkVR_SubmitChain.load(std::memory_order_relaxed) &&
        CyberpunkVR_SubmitChainDeferPresent.load(std::memory_order_relaxed)) {
        uint8_t* output = nullptr;
        uint64_t handle = 0;
        if (PresentTarget(context, &output, &handle)) {
            if (output != g_last_output) CyberpunkVR_DeferredPresentGate[2].fetch_add(1, std::memory_order_relaxed);
            if (handle != g_last_handle) CyberpunkVR_DeferredPresentGate[3].fetch_add(1, std::memory_order_relaxed);
            g_steady = output == g_last_output ? g_steady + 1 : 0;
            g_last_output = output;
            g_last_handle = handle;
            const uint64_t frame = g_presenting.fetch_add(1, std::memory_order_relaxed) + 1;
            CyberpunkVR_DeferredPresentGate[0].store(g_steady, std::memory_order_relaxed);
            CyberpunkVR_DeferredPresentGate[1].store(frame - g_secondary.load(std::memory_order_relaxed), std::memory_order_relaxed);
            if (g_steady >= kSteadyFrames && frame - g_secondary.load(std::memory_order_relaxed) <= 2) {
                DeferPresent(output, handle);
                return;
            }
            CyberpunkVR_DeferredPresentStats[6].fetch_add(1, std::memory_order_relaxed);
        } else {
            g_secondary.store(g_presenting.load(std::memory_order_relaxed), std::memory_order_relaxed);
        }
    }
    WaitPresented();
    if (context && (context[48] & 2) && ChainPending() && Presents(context)) {
        const int64_t ticks = WaitSubmitted();
        CyberpunkVR_SubmitChainStats[2].fetch_add(1, std::memory_order_relaxed);
        CyberpunkVR_SubmitChainStats[3].fetch_add(static_cast<uint64_t>(ticks), std::memory_order_relaxed);
    }
    g_orig_present_node(node, context, job);
}

void* Detour_ViewBuilder(void* builder, void* job, int64_t* view) {
    // Every view, not only the one that presents: the port's present-time work (HUD capture, render-target
    // bookkeeping) must not run beside the next frame's render nodes. With only the presenting view gated,
    // VRCAM's nodes ran beside it, and pausing after long head turns crashed twice in the HUD adoption path
    // (a stale render-data pointer, then a full descriptor heap) where the ungated build never did.
    if (PresentPending()) {
        const int64_t ticks = WaitPresented();
        CyberpunkVR_DeferredPresentStats[4].fetch_add(1, std::memory_order_relaxed);
        CyberpunkVR_DeferredPresentStats[5].fetch_add(static_cast<uint64_t>(ticks), std::memory_order_relaxed);
    }
    if (CyberpunkVR_SubmitChain.load(std::memory_order_relaxed) == 1 && ChainPending()) {
        const int64_t ticks = WaitSubmitted();
        CyberpunkVR_SubmitChainStats[4].fetch_add(1, std::memory_order_relaxed);
        CyberpunkVR_SubmitChainStats[5].fetch_add(static_cast<uint64_t>(ticks), std::memory_order_relaxed);
    }
    return g_orig_view_builder(builder, job, view);
}

}  // namespace

}  // namespace detail
}  // namespace cvr

// Swapchain changes (ResizeBuffers) must not overlap a deferred present.
extern "C" void CyberpunkVR_WaitDeferredPresent() { cvr::detail::WaitPresented(); }

// No render packets beside a deferred present. The present's back-buffer step (sub_1405D9850) rewrites the
// swapchain's current back buffer in the global texture table that render packets resolve their targets
// through, and the port's present-time work shares state with the HUD adoption in OMSetRenderTargets. The
// view-job gate already keeps the next frame's nodes after the present; this also covers packets run from
// jobs outside a view job, such as the draw-buffer flush. With five async compute passes on the graphics
// queue, one soak with the deferral on crashed in a render packet's clear (sub_14095B35C, an external
// texture whose object had been freed and its memory reused); the same soak with the deferral off passed.
extern "C" void CyberpunkVR_WaitDeferredPresentForPackets() {
    if (!cvr::detail::PresentPending()) return;
    const int64_t ticks = cvr::detail::WaitPresented();
    if (!ticks) return;
    CyberpunkVR_DeferredPresentPacketWaits[0].fetch_add(1, std::memory_order_relaxed);
    CyberpunkVR_DeferredPresentPacketWaits[1].fetch_add(static_cast<uint64_t>(ticks), std::memory_order_relaxed);
}

extern "C" void CyberpunkVR_SubmitChainConfigure(int mode) {
    CyberpunkVR_SubmitChain.store(static_cast<uint32_t>(mode < 0 ? 0 : mode > 2 ? 2 : mode), std::memory_order_relaxed);
}

namespace cvr {
namespace detail {

CVR_DETOUR("[submit-chain] submission point sub_1422EF830", 0x22EF830, Detour_SubmitPoint, g_orig_submit_point)
CVR_DETOUR("[submit-chain] Present node sub_140BF60F0", 0xBF60F0, Detour_PresentNode, g_orig_present_node)
CVR_DETOUR("[submit-chain] view job builder sub_140B603D8", 0xB603D8, Detour_ViewBuilder, g_orig_view_builder)

}  // namespace detail
}  // namespace cvr
