// StageReplay -- see include/Render/StageReplay.hpp for what and why.
//
// Three layers:
//   1. Chained command-list hooks, installed at runtime over whatever already occupies the slots
//      (the plugin's own hooks or the driver). They mirror each list's graphics state (Shadow) and,
//      inside a consumer scope, append the call to the scope's stream.
//   2. The consumer scope, entered from the 0x334CC0 hook (Stereo/Grading.cpp): census counting,
//      source capture, target skip, target replay.
//   3. Poll, once per Present: census window, hook installation.
// All of it is inert until CyberpunkVR_StageReplayMode or a census request is set.
#include "Render/StageReplay.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeCameraBindingState.hpp"
#include "Render/NativeCameraFlushState.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/VrcamConfig.hpp"
#include "Utils/StereoLog.hpp"
#include "Hooks/SwapChainInternal.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include <wrl/client.h>
#include <windows.h>
#include <intrin.h>
#include <algorithm>
#include <atomic>
#include <memory>
#include <mutex>

using namespace cvr::stereo::stage_replay;

extern "C" {
// 0 off, 1 census (capture only), 2 skip target stages, 3 replay source capture into target,
// 4 replay with the target camera substituted. See stage_replay::Mode.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageReplayMode{0};
// Bit s selects RenderElements stage s (consumer arguments+0x10).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StageReplayStages{0};
// Side that records natively and is captured (1 = VRCAM, 0 = MAIN).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageReplaySource{1};
// Census: frames to count (written by the test), state 0 idle / 1 running / 2 done.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageCensusRequest{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageCensusState{0};
// Census detail stream: this stage's first call per side in the window is recorded in full.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageStreamStage{0xFFFFFFFFu};
// Rows [side*64+stage][32]; columns documented at Column below. Ops [row][Op].
__declspec(dllexport) uint64_t CyberpunkVR_StageCensusRows[128][32];
__declspec(dllexport) uint32_t CyberpunkVR_StageCensusOps[128][48];
// frames, presents, chainedVtables, hookInstalls, shadowsClaimed, shadowFull, epoch, reserved...
__declspec(dllexport) uint64_t CyberpunkVR_StageCensusSummary[16];
// Per side: commandsAddress, commandCount, payloadAddress, payloadBytes, overflow, list, engineFrame,
// stage, camVs, camPs, heapCpu, heapGpu, stride, nativeHeap, sliceBaseCapacity, cursor.
__declspec(dllexport) uint64_t CyberpunkVR_StageStreamInfo[2][16];
// Census stream diagnostic: the native render context (0x800 bytes) and its binding state (0x200)
// of each side's streamed call, copied after the call recorded. Address in [side][0] of Addr.
__declspec(dllexport) uint64_t CyberpunkVR_StageStreamContextDump[2][256];
__declspec(dllexport) uint64_t CyberpunkVR_StageStreamStateDump[2][64];
__declspec(dllexport) uint64_t CyberpunkVR_StageStreamContextAddr[2][2];
// captures, captureOverflow, captureBusy, skipped, replayed, replayCommands, replayTicks, fallbacks,
// cameraTables, cameraDescriptors, restoreOps, reserved..., [16+reason] fallback by reason.
__declspec(dllexport) uint64_t CyberpunkVR_StageReplayStats[32];
__declspec(dllexport) void CyberpunkVR_StageReplayPoll();
// Census timeline: 6 qwords per event -- frame|side<<32|stage<<40|ordinal<<48|kind<<56, thread,
// startQpc, endQpc, spanBegin, spanEnd. Kind 0 forward, 1 capture, 2 skip, 3 replay, 4 fallback,
// 5 present. Written only while a census window runs.
__declspec(dllexport) uint64_t CyberpunkVR_StageEvents[16384 * 6];
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageEventCount{0};
// 1: give VRCAM's GBuffer_Solid the same worker partitions as MAIN. The scene graph builder
// sub_141D43040 clamps the partition count to 1 when view+0xF94 (the AA/upscaler mode) is 1, and
// the render-to-texture eye has that mode -- but so does every other render-to-texture view (the
// optics feed, ...). The clamp is redirected (InstallVrcamPartitions) to a check that keeps
// min(workers, dword_14347F790) only for the graph whose camera name equals
// CyberpunkVR_VrcamGbufferPartitionsKey; every other view keeps the native 1. State: 0 native,
// 1 VRCAM partitioned, 2 site not installed (bytes did not match).
// Its own setting (xr_vrcam_gbuffer_partitions), not part of stage replay: unpatched, VRCAM's
// G-buffer is ONE job -- 2.15 ms on one thread in the city -- and the second view's whole graph,
// which the engine runs before MAIN's, waits on it. Node timeline 2026-10-16.
// The camera scope's view (vr_optics_feed) is the same kind of graph -- render-to-texture, run before
// MAIN's bulk with MAIN idle -- and has its own setting (xr_scope_gbuffer_partitions, off by default:
// measured slower, see LiveControlsPoll.cpp).
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_VrcamGbufferPartitions{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OpticsGbufferPartitions{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_VrcamGbufferPartitionsState{0};
// Read by the trampoline at install-time address: VRCAM's and the scope's camera-name hashes while
// partitioned, 0 otherwise. Must stay plain globals (Hooks/Trampoline.hpp).
__declspec(dllexport) volatile uint64_t CyberpunkVR_VrcamGbufferPartitionsKey = 0;
__declspec(dllexport) volatile uint64_t CyberpunkVR_OpticsGbufferPartitionsKey = 0;
// Replay: 1 lets a captured stream that contains resource barriers replay without them. Every
// stage begins with one barrier on the source view's own resource (census 816); the target's own
// consumer -- with its barrier -- is skipped as a whole, so the engine's state tracking stays
// consistent. 0 (default) treats any barrier as a reason to fall back.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageReplayAllowBarriers{0};
// 1 = census streams carry every bound table's descriptors (flag 16 on RootTable), for
// tools/single_pass_tests/stage_table_diff.py. Diagnostic only; captures never do this.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageReplayTableDump{0};
// Last view descriptors read for a capture ([0]) and a replay target ([1]), with their stage in [x][7].
__declspec(dllexport) uint64_t CyberpunkVR_StageReplayViewDiag[2][8];
// Replay: microseconds a target call may wait for the source's partitions of the same frame.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StageReplayWaitUs{0};
// Camera-replay diagnostics: target camera unknown, no heap, captured table without the source
// camera, tables built, tables reused; the last target camera, source camera, table size; then
// ring code mismatch, ring heap mismatch, ring exhausted, descriptors reserved.
__declspec(dllexport) uint64_t CyberpunkVR_StageReplayCameraDiag[12]{};
}

namespace cvr::stereo::stage_replay {
namespace {
using cvr::detail::CommandListVtableHook;

enum Column : uint32_t {
    Calls, Ticks, Commands, Draws, BlockingOps, ForeignOps, LastEngineFrame, MaxOrdinal, PeerDone, PeerPending,
    CamVs, CamPs, CameraSlots, ContextOk, Copies, CopiedDescriptors, Tables, TablesMatched, CameraTables,
    Skipped, Replayed, Fallbacks, LastFallback, ReplayTicks, Args0, Args1, Args2, Args3, CameraParams, EntryList,
    ShadowValid, Packets
};
static_assert(Packets == 31);

int64_t Now() { LARGE_INTEGER t; QueryPerformanceCounter(&t); return t.QuadPart; }
void Add(uint64_t& field, uint64_t value) { std::atomic_ref<uint64_t>(field).fetch_add(value, std::memory_order_relaxed); }
void Add32(uint32_t& field, uint32_t value) { std::atomic_ref<uint32_t>(field).fetch_add(value, std::memory_order_relaxed); }
void Store(uint64_t& field, uint64_t value) { std::atomic_ref<uint64_t>(field).store(value, std::memory_order_relaxed); }
void Max(uint64_t& field, uint64_t value) {
    std::atomic_ref<uint64_t> ref(field); uint64_t cur = ref.load(std::memory_order_relaxed);
    while (cur < value && !ref.compare_exchange_weak(cur, value, std::memory_order_relaxed)) {}
}
uintptr_t Base() { return reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base); }
bool ReadSafe(uintptr_t address, void* out, size_t bytes) {
    if (!address) return false;
    __try { std::memcpy(out, reinterpret_cast<const void*>(address), bytes); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}
bool ReadFn(uintptr_t address, void* out, size_t bytes) { return ReadSafe(address, out, bytes); }
bool WriteCursor(uint64_t state, uint32_t cursor) {
    __try { *reinterpret_cast<volatile uint32_t*>(state + 0x14) = cursor; return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
}

// The native renderer frame clock, renderer+0x4CC8 (2.5; incremented at 0x3D7EE5; 2.31: +0x4CA4, read by temporal
// passes; Framegen/Inputs.cpp reads the same field). It ties a source capture to the target of the
// same frame. [[engine+0x60]], which the consumer hands to its uploads, looked like a frame index
// but is constant (census 816).
uint32_t EngineFrame() {
    const uintptr_t base = Base(); if (!base) return 0;
    uint64_t renderer = 0; uint32_t frame = 0;
    if (!ReadSafe(base + 0x3BBCAE8, &renderer, 8) || !renderer || !ReadSafe(renderer + 0x4CC8, &frame, 4)) return 0;
    return frame;
}

// ---------------------------------------------------------------------------------------------
// Chained hooks
// ---------------------------------------------------------------------------------------------
constexpr size_t SlotCount = 81;
struct Chain { void** vtable{}; std::array<void*, SlotCount> prev{}; size_t last{}; bool list1{}, list2{}, list4{}, list5{}, list7{}; bool installed{}; };
std::array<Chain, 16> g_chains;
std::atomic<uint32_t> g_chainCount{0};
std::mutex g_installMutex;
std::atomic<bool> g_active{false};          // any mode or census -> hooks record
std::atomic<bool> g_tracking{false};        // replay modes -> shadows tracked
// Bumped whenever tracking (re)starts: a mirror is only trusted when its Reset was observed in the
// current generation, so state set while the hooks were off can never be "restored".
std::atomic<uint32_t> g_generation{1};

void* Prev(void* self, size_t slot) {
    void** vt = *reinterpret_cast<void***>(self);
    const uint32_t n = g_chainCount.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < n; ++i) if (g_chains[i].vtable == vt) return g_chains[i].prev[slot];
    return nullptr;
}
template <typename Fn> Fn PrevFn(void* self, size_t slot) { return reinterpret_cast<Fn>(Prev(self, slot)); }

// ---------------------------------------------------------------------------------------------
// Shadows
// ---------------------------------------------------------------------------------------------
// One mirror per live command list, open addressing over the list pointer. The engine creates and
// destroys lists (loading, pool growth), so a slot whose list has not been Reset for
// ShadowStaleFrames presents is taken over by a new list. Without that the table filled up in a
// session where replay was on from startup (838: 491 of 512 slots, 4.5 million misses, each a full
// probe run, and 8% of replays refused as ShadowInvalid).
constexpr uint32_t ShadowCapacity = 1024, ShadowProbe = 64, ShadowStaleFrames = 240;
struct ShadowSlot { std::atomic<uint64_t> key{0}; std::atomic<uint32_t> lastReset{0}; Shadow shadow; };
std::unique_ptr<ShadowSlot[]> g_shadows;
std::atomic<uint32_t> g_presents{1};   // advanced once per Present by Poll
thread_local uint64_t t_shadowList = 0;
thread_local ShadowSlot* t_shadowSlot = nullptr;

ShadowSlot* SlotFor(void* list, bool claim) {
    const uint64_t key = reinterpret_cast<uint64_t>(list);
    // The cached slot is re-checked: it may have been handed to another list since.
    if (t_shadowList == key && t_shadowSlot && t_shadowSlot->key.load(std::memory_order_relaxed) == key) return t_shadowSlot;
    if (!g_shadows) return nullptr;
    const uint32_t now = g_presents.load(std::memory_order_relaxed);
    const uint32_t h = uint32_t((key >> 4) * 2654435761u);
    ShadowSlot* stale = nullptr; uint64_t staleKey = 0;
    auto take = [&](ShadowSlot& slot) {
        slot.shadow = Shadow{}; slot.shadow.list = key;
        slot.lastReset.store(now, std::memory_order_relaxed);
        Add(CyberpunkVR_StageCensusSummary[4], 1);
        t_shadowList = key; t_shadowSlot = &slot; return &slot;
    };
    for (uint32_t probe = 0; probe < ShadowProbe; ++probe) {
        auto& slot = g_shadows[(h + probe) & (ShadowCapacity - 1)];
        uint64_t cur = slot.key.load(std::memory_order_acquire);
        if (cur == key) { t_shadowList = key; t_shadowSlot = &slot; return &slot; }
        if (cur == 0) {
            if (!claim) break;   // keys are never removed, so the key is not further on
            uint64_t expected = 0;
            if (slot.key.compare_exchange_strong(expected, key, std::memory_order_acq_rel)) return take(slot);
            if (expected == key) { t_shadowList = key; t_shadowSlot = &slot; return &slot; }
            continue;
        }
        if (claim && !stale && now - slot.lastReset.load(std::memory_order_relaxed) > ShadowStaleFrames) { stale = &slot; staleKey = cur; }
    }
    if (claim && stale && stale->key.compare_exchange_strong(staleKey, key, std::memory_order_acq_rel)) {
        Add(CyberpunkVR_StageCensusSummary[7], 1);   // slots taken over
        return take(*stale);
    }
    Add(CyberpunkVR_StageCensusSummary[5], 1);
    return nullptr;
}
Shadow* ShadowFor(void* list, bool claim) { auto* slot = SlotFor(list, claim); return slot ? &slot->shadow : nullptr; }
void NoteReset(void* list) {
    if (auto* slot = SlotFor(list, false)) slot->lastReset.store(g_presents.load(std::memory_order_relaxed), std::memory_order_relaxed);
}
thread_local bool t_replaying = false;
template <typename F> void Track(void* list, F&& f) {
    // Replay is tracked too: whatever it leaves bound (heaps it set where the target had none) is
    // the list's real state for the next stage. Restore works from a copy taken at entry.
    if (!g_tracking.load(std::memory_order_relaxed)) return;
    if (auto* s = ShadowFor(list, true)) f(*s);
}

// ---------------------------------------------------------------------------------------------
// Captures
// ---------------------------------------------------------------------------------------------
struct CaptureSlot {
    Stream stream;
    std::atomic<uint32_t> state{0};   // 0 empty, 1 writing, 2 complete, 3 reading
    uint32_t frame{}, stage{}, ordinal{};
    uint64_t list{}, camVs{}, camPs{}, spanBegin{}, spanEnd{};
    stage_replay::ViewDescriptors view;
    bool foreign{};
};
enum : uint32_t { Empty = 0, Writing = 1, Complete = 2, Reading = 3 };
constexpr size_t MaxCaptureSlots = 96;
std::mutex g_captureMutex;
std::array<std::atomic<CaptureSlot*>, 64 * MaxOrdinals * 2> g_captures{};
std::vector<std::unique_ptr<CaptureSlot>> g_captureStorage;
uint32_t g_increment{};
ID3D12Device* g_device{};

using CopyDescriptorsFn = void(STDMETHODCALLTYPE*)(ID3D12Device*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, const UINT*, UINT,
                                                   const D3D12_CPU_DESCRIPTOR_HANDLE*, const UINT*, D3D12_DESCRIPTOR_HEAP_TYPE);
CopyDescriptorsFn g_copyOriginal{};

CaptureSlot* Slot(uint32_t stage, uint32_t ordinal, uint32_t frame) {
    if (stage >= 64 || ordinal >= MaxOrdinals) return nullptr;
    // One slot per frame parity, so a capture of frame N survives while N+1 is recorded. A single
    // reused (warm) slot measured no cheaper (run 834: VRCAM 7.9-8.1 ms either way).
    const size_t index = (size_t(stage) * MaxOrdinals + ordinal) * 2 + (frame & 1);
    if (auto* slot = g_captures[index].load(std::memory_order_acquire)) return slot;
    std::lock_guard lock(g_captureMutex);
    if (auto* slot = g_captures[index].load(std::memory_order_acquire)) return slot;
    // Every (stage, ordinal, frame parity) in use needs its own slot: 13 stages with stage 6 in four
    // partitions and stage 24 in three calls are 36 (run 831 lost stages at the old cap of 32).
    if (g_captureStorage.size() >= MaxCaptureSlots) return nullptr;
    auto slot = std::make_unique<CaptureSlot>();
    slot->stream.Reserve(32768, 2u << 20);
    auto* raw = slot.get(); g_captureStorage.push_back(std::move(slot));
    g_captures[index].store(raw, std::memory_order_release);
    return raw;
}

// ---------------------------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------------------------
struct Scope {
    Stream* stream{};
    uint64_t* row{};
    uint32_t* ops{};
    CaptureSlot* capture{};
    uint64_t list{};
    uint64_t heapCpu{}, heapGpu{}; uint32_t stride{}, heapCount{};
    uint64_t camVs{}, camPs{};
    stage_replay::ViewDescriptors view;   // the source's view descriptors (capture)
    bool preserveCamera{};
    // Source binding state: the layout's descriptor-table groups (root parameter, CPU-table range).
    struct CameraGroup { uint32_t param{}, start{}, count{}; };
    static constexpr uint32_t MaxGroups = 24;
    uint64_t state{}, layout{}, cpuTable{};
    std::array<CameraGroup, MaxGroups> groups{};
    uint32_t groupCount{};
};
thread_local Scope* t_scope = nullptr;
void RefreshScopeGroups(Scope& s);
void RecordCameraTable(Scope& s, Command& c, std::array<uint64_t, 257>& buffer, size_t& bytes);
size_t RecordAnyTable(const Scope& s, const Command& c, std::array<uint64_t, 257>& buffer);

void Record(void* list, Command c, const void* data = nullptr, size_t bytes = 0) {
    Scope* s = t_scope;
    if (!s || t_replaying) return;
    if (s->ops && uint32_t(c.op) < 48) Add32(s->ops[uint32_t(c.op)], 1);
    if (list && reinterpret_cast<uint64_t>(list) != s->list) { c.flags |= 1; if (s->row) Add(s->row[ForeignOps], 1); if (s->capture) s->capture->foreign = true; }
    if (s->row) {
        Add(s->row[Commands], 1);
        if (IsDraw(c.op)) Add(s->row[Draws], 1);
        if (Blocking(c.op)) Add(s->row[BlockingOps], 1);
    }
    if (c.op == Op::RootTable && s->row) Add(s->row[Tables], 1);
    if (s->stream) {
        if (c.op == Op::RootTable && !s->capture && s->stride && c.b >= s->heapGpu && c.b < s->heapGpu + uint64_t(s->heapCount) * s->stride) {
            const uint64_t index = (c.b - s->heapGpu) / s->stride; c.c = index;
            // The flush copies a table immediately before binding it; look back a little.
            const auto& cmds = s->stream->Commands();
            for (size_t i = cmds.size(), seen = 0; i-- > 0 && seen < 256; ++seen)
                if (cmds[i].op == Op::Copy && cmds[i].a == index) {
                    c.count = uint16_t(i + 1);
                    if (s->row) Add(s->row[TablesMatched], 1);
                    if ((cmds[i].flags & 2) && s->row) Add(s->row[CameraTables], 1);
                    if (cmds[i].flags & 2) c.flags |= 2;
                    break;
                }
        } else if (c.op == Op::RootTable) c.c = ~uint64_t(0);
        if (c.op == Op::RootTable && s->preserveCamera && s->state) {
            std::array<uint64_t, 257> buffer;
            size_t tableBytes = 0;
            RecordCameraTable(*s, c, buffer, tableBytes);
            if (tableBytes) { s->stream->Push(c, buffer.data(), tableBytes); return; }
        }
        // Diagnostic (census streams only): every table's contents from the CPU table.
        if (c.op == Op::RootTable && !s->capture && s->state && CyberpunkVR_StageReplayTableDump.load(std::memory_order_relaxed)) {
            std::array<uint64_t, 257> buffer;
            if (const size_t tableBytes = RecordAnyTable(*s, c, buffer)) { c.flags |= 16; s->stream->Push(c, buffer.data(), tableBytes); return; }
        }
        s->stream->Push(c, data, bytes);
    }
}

// ---------------------------------------------------------------------------------------------
// Hook bodies. Each forwards first, then records/tracks; replay calls skip both.
// ---------------------------------------------------------------------------------------------
#define CVR_SR_PREV(slot, type) auto prev = PrevFn<type>(self, slot); if (!prev) return
using L = ID3D12GraphicsCommandList;

HRESULT STDMETHODCALLTYPE H_Reset(L* self, ID3D12CommandAllocator* allocator, ID3D12PipelineState* pso) {
    auto prev = PrevFn<HRESULT(STDMETHODCALLTYPE*)(L*, ID3D12CommandAllocator*, ID3D12PipelineState*)>(self, 10);
    if (!prev) return E_FAIL;
    const HRESULT out = prev(self, allocator, pso);
    if (SUCCEEDED(out)) {
        Track(self, [&](Shadow& s) { s.Reset(reinterpret_cast<uint64_t>(self), pso, g_generation.load(std::memory_order_relaxed)); });
        if (g_tracking.load(std::memory_order_relaxed)) NoteReset(self);
    }
    return out;
}
void STDMETHODCALLTYPE H_ClearState(L* self, ID3D12PipelineState* pso) {
    CVR_SR_PREV(11, void(STDMETHODCALLTYPE*)(L*, ID3D12PipelineState*)); prev(self, pso);
    Track(self, [&](Shadow& s) { s.Reset(reinterpret_cast<uint64_t>(self), pso, g_generation.load(std::memory_order_relaxed)); });
    if (g_tracking.load(std::memory_order_relaxed)) NoteReset(self);
    Record(self, {Op::Reset});
}
void STDMETHODCALLTYPE H_Draw(L* self, UINT v, UINT i, UINT sv, UINT si) {
    CVR_SR_PREV(12, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, UINT, UINT)); prev(self, v, i, sv, si);
    Record(self, {Op::Draw, 0, 0, v, uint64_t(i) | (uint64_t(sv) << 32), si});
}
void STDMETHODCALLTYPE H_DrawIndexed(L* self, UINT n, UINT i, UINT si, INT bv, UINT sinst) {
    CVR_SR_PREV(13, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, UINT, INT, UINT)); prev(self, n, i, si, bv, sinst);
    Record(self, {Op::DrawIndexed, 0, 0, n, uint64_t(i) | (uint64_t(si) << 32), uint64_t(uint32_t(bv)) | (uint64_t(sinst) << 32)});
}
void STDMETHODCALLTYPE H_Dispatch(L* self, UINT x, UINT y, UINT z) {
    CVR_SR_PREV(14, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, UINT)); prev(self, x, y, z);
    Record(self, {Op::Dispatch, 0, 0, x, y, z});
}
void STDMETHODCALLTYPE H_CopyBuffer(L* self, ID3D12Resource* d, UINT64 doff, ID3D12Resource* s, UINT64 soff, UINT64 n) {
    CVR_SR_PREV(15, void(STDMETHODCALLTYPE*)(L*, ID3D12Resource*, UINT64, ID3D12Resource*, UINT64, UINT64)); prev(self, d, doff, s, soff, n);
    Record(self, {Op::CopyBuffer, 0, 0, uint32_t(n), reinterpret_cast<uint64_t>(d), doff, reinterpret_cast<uint64_t>(s)});
}
void STDMETHODCALLTYPE H_CopyTexture(L* self, const D3D12_TEXTURE_COPY_LOCATION* d, UINT x, UINT y, UINT z, const D3D12_TEXTURE_COPY_LOCATION* s, const D3D12_BOX* box) {
    CVR_SR_PREV(16, void(STDMETHODCALLTYPE*)(L*, const D3D12_TEXTURE_COPY_LOCATION*, UINT, UINT, UINT, const D3D12_TEXTURE_COPY_LOCATION*, const D3D12_BOX*));
    prev(self, d, x, y, z, s, box); Record(self, {Op::CopyTexture, 0, 0, 0, d ? reinterpret_cast<uint64_t>(d->pResource) : 0});
}
void STDMETHODCALLTYPE H_CopyResource(L* self, ID3D12Resource* d, ID3D12Resource* s) {
    CVR_SR_PREV(17, void(STDMETHODCALLTYPE*)(L*, ID3D12Resource*, ID3D12Resource*)); prev(self, d, s);
    Record(self, {Op::CopyTexture, 0, 0, 1, reinterpret_cast<uint64_t>(d)});
}
void STDMETHODCALLTYPE H_Topology(L* self, D3D12_PRIMITIVE_TOPOLOGY t) {
    CVR_SR_PREV(20, void(STDMETHODCALLTYPE*)(L*, D3D12_PRIMITIVE_TOPOLOGY)); prev(self, t);
    Track(self, [&](Shadow& s) { s.topology = t; s.topologySet = true; });
    Record(self, {Op::Topology, 0, 0, uint32_t(t)});
}
void STDMETHODCALLTYPE H_Viewports(L* self, UINT n, const D3D12_VIEWPORT* v) {
    CVR_SR_PREV(21, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_VIEWPORT*)); prev(self, n, v);
    Track(self, [&](Shadow& s) { if (v) s.SetViewports(n, v); });
    Record(self, {Op::Viewports, 0, uint16_t(n)}, v, v ? size_t(n) * sizeof(D3D12_VIEWPORT) : 0);
}
void STDMETHODCALLTYPE H_Scissors(L* self, UINT n, const D3D12_RECT* r) {
    CVR_SR_PREV(22, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_RECT*)); prev(self, n, r);
    Track(self, [&](Shadow& s) { if (r) s.SetScissors(n, r); });
    Record(self, {Op::Scissors, 0, uint16_t(n)}, r, r ? size_t(n) * sizeof(D3D12_RECT) : 0);
}
void STDMETHODCALLTYPE H_Blend(L* self, const FLOAT* f) {
    CVR_SR_PREV(23, void(STDMETHODCALLTYPE*)(L*, const FLOAT*)); prev(self, f);
    Track(self, [&](Shadow& s) { const float one[4]{1, 1, 1, 1}; std::memcpy(s.blend.data(), f ? f : one, 16); s.blendSet = true; });
    Record(self, {Op::BlendFactor, uint8_t(f ? 0 : 4)}, f, f ? 16 : 0);
}
void STDMETHODCALLTYPE H_Stencil(L* self, UINT ref) {
    CVR_SR_PREV(24, void(STDMETHODCALLTYPE*)(L*, UINT)); prev(self, ref);
    Track(self, [&](Shadow& s) { s.stencil = ref; s.stencilSet = true; });
    Record(self, {Op::StencilRef, 0, 0, ref});
}
void STDMETHODCALLTYPE H_Pipeline(L* self, ID3D12PipelineState* pso) {
    CVR_SR_PREV(25, void(STDMETHODCALLTYPE*)(L*, ID3D12PipelineState*)); prev(self, pso);
    Track(self, [&](Shadow& s) { s.pso = pso; s.psoSet = true; });
    Record(self, {Op::Pipeline, 0, 0, 0, reinterpret_cast<uint64_t>(pso)});
}
void STDMETHODCALLTYPE H_Barrier(L* self, UINT n, const D3D12_RESOURCE_BARRIER* b) {
    CVR_SR_PREV(26, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_RESOURCE_BARRIER*)); prev(self, n, b);
    Record(self, {Op::Barrier, 0, uint16_t(n), 0, (n && b) ? reinterpret_cast<uint64_t>(b[0].Transition.pResource) : 0},
           b, b ? size_t(std::min<UINT>(n, 64)) * sizeof(D3D12_RESOURCE_BARRIER) : 0);
}
void STDMETHODCALLTYPE H_Bundle(L* self, L* bundle) {
    CVR_SR_PREV(27, void(STDMETHODCALLTYPE*)(L*, L*)); prev(self, bundle);
    Track(self, [&](Shadow& s) { s.valid = false; });
    Record(self, {Op::Bundle});
}
void STDMETHODCALLTYPE H_Heaps(L* self, UINT n, ID3D12DescriptorHeap* const* h) {
    CVR_SR_PREV(28, void(STDMETHODCALLTYPE*)(L*, UINT, ID3D12DescriptorHeap* const*)); prev(self, n, h);
    Track(self, [&](Shadow& s) { if (h) s.SetHeaps(n, h); });
    Record(self, {Op::Heaps, 0, uint16_t(n)}, h, h ? size_t(n) * sizeof(void*) : 0);
}
void STDMETHODCALLTYPE H_ComputeRoot(L* self, ID3D12RootSignature* r) {
    CVR_SR_PREV(29, void(STDMETHODCALLTYPE*)(L*, ID3D12RootSignature*)); prev(self, r);
    Record(self, {Op::ComputeState, 0, 0, 29, reinterpret_cast<uint64_t>(r)});
}
void STDMETHODCALLTYPE H_GraphicsRoot(L* self, ID3D12RootSignature* r) {
    CVR_SR_PREV(30, void(STDMETHODCALLTYPE*)(L*, ID3D12RootSignature*)); prev(self, r);
    Track(self, [&](Shadow& s) { s.SetRoot(r); });
    Record(self, {Op::RootSignature, 0, 0, 0, reinterpret_cast<uint64_t>(r)});
}
void STDMETHODCALLTYPE H_ComputeTable(L* self, UINT i, D3D12_GPU_DESCRIPTOR_HANDLE h) {
    CVR_SR_PREV(31, void(STDMETHODCALLTYPE*)(L*, UINT, D3D12_GPU_DESCRIPTOR_HANDLE)); prev(self, i, h);
    Record(self, {Op::ComputeState, 0, 0, 31, h.ptr});
}
void STDMETHODCALLTYPE H_GraphicsTable(L* self, UINT i, D3D12_GPU_DESCRIPTOR_HANDLE h) {
    CVR_SR_PREV(32, void(STDMETHODCALLTYPE*)(L*, UINT, D3D12_GPU_DESCRIPTOR_HANDLE)); prev(self, i, h);
    Track(self, [&](Shadow& s) { s.SetView(i, Shadow::Table, h.ptr); });
    Record(self, {Op::RootTable, 0, 0, i, h.ptr});
}
void STDMETHODCALLTYPE H_ComputeConstant(L* self, UINT i, UINT v, UINT o) {
    CVR_SR_PREV(33, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, UINT)); prev(self, i, v, o);
    Record(self, {Op::ComputeState, 0, 0, 33});
}
void STDMETHODCALLTYPE H_GraphicsConstant(L* self, UINT i, UINT v, UINT o) {
    CVR_SR_PREV(34, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, UINT)); prev(self, i, v, o);
    Track(self, [&](Shadow& s) { s.SetConstants(i, 1, &v, o); });
    Record(self, {Op::RootConstant, 0, 0, i, v, o});
}
void STDMETHODCALLTYPE H_ComputeConstants(L* self, UINT i, UINT n, const void* v, UINT o) {
    CVR_SR_PREV(35, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, const void*, UINT)); prev(self, i, n, v, o);
    Record(self, {Op::ComputeState, 0, 0, 35});
}
void STDMETHODCALLTYPE H_GraphicsConstants(L* self, UINT i, UINT n, const void* v, UINT o) {
    CVR_SR_PREV(36, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, const void*, UINT)); prev(self, i, n, v, o);
    Track(self, [&](Shadow& s) { s.SetConstants(i, n, static_cast<const uint32_t*>(v), o); });
    Record(self, {Op::RootConstants, 0, uint16_t(n), i, 0, o}, v, v ? size_t(n) * 4 : 0);
}
template <size_t SlotIndex, Op Kind, uint8_t ShadowKind>
void STDMETHODCALLTYPE H_RootView(L* self, UINT i, D3D12_GPU_VIRTUAL_ADDRESS a) {
    CVR_SR_PREV(SlotIndex, void(STDMETHODCALLTYPE*)(L*, UINT, D3D12_GPU_VIRTUAL_ADDRESS)); prev(self, i, a);
    if constexpr (ShadowKind != 0) Track(self, [&](Shadow& s) { s.SetView(i, ShadowKind, a); });
    Record(self, {Kind, 0, 0, i, a});
}
void STDMETHODCALLTYPE H_IndexBuffer(L* self, const D3D12_INDEX_BUFFER_VIEW* v) {
    CVR_SR_PREV(43, void(STDMETHODCALLTYPE*)(L*, const D3D12_INDEX_BUFFER_VIEW*)); prev(self, v);
    Track(self, [&](Shadow& s) { s.indexSet = true; s.indexNull = !v; if (v) s.index = *v; });
    Record(self, {Op::IndexBuffer, uint8_t(v ? 0 : 4)}, v, v ? sizeof(*v) : 0);
}
void STDMETHODCALLTYPE H_VertexBuffers(L* self, UINT start, UINT n, const D3D12_VERTEX_BUFFER_VIEW* v) {
    CVR_SR_PREV(44, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, const D3D12_VERTEX_BUFFER_VIEW*)); prev(self, start, n, v);
    Track(self, [&](Shadow& s) { s.SetVertices(start, n, v); });
    Record(self, {Op::VertexBuffers, uint8_t(v ? 0 : 4), uint16_t(n), start}, v, v ? size_t(n) * sizeof(*v) : 0);
}
void STDMETHODCALLTYPE H_SoTargets(L* self, UINT start, UINT n, const D3D12_STREAM_OUTPUT_BUFFER_VIEW* v) {
    CVR_SR_PREV(45, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, const D3D12_STREAM_OUTPUT_BUFFER_VIEW*)); prev(self, start, n, v);
    Record(self, {Op::Other, 0, 0, 45});
}
void STDMETHODCALLTYPE H_Targets(L* self, UINT n, const D3D12_CPU_DESCRIPTOR_HANDLE* rt, BOOL single, const D3D12_CPU_DESCRIPTOR_HANDLE* ds) {
    CVR_SR_PREV(46, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_CPU_DESCRIPTOR_HANDLE*, BOOL, const D3D12_CPU_DESCRIPTOR_HANDLE*)); prev(self, n, rt, single, ds);
    Record(self, {Op::Targets, 0, uint16_t(n), 0, (n && rt) ? rt[0].ptr : 0, ds ? ds->ptr : 0});
}
void STDMETHODCALLTYPE H_ClearDepth(L* self, D3D12_CPU_DESCRIPTOR_HANDLE h, D3D12_CLEAR_FLAGS f, FLOAT d, UINT8 s, UINT n, const D3D12_RECT* r) {
    CVR_SR_PREV(47, void(STDMETHODCALLTYPE*)(L*, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_CLEAR_FLAGS, FLOAT, UINT8, UINT, const D3D12_RECT*)); prev(self, h, f, d, s, n, r);
    Record(self, {Op::Clear, 0, 0, 47, h.ptr});
}
void STDMETHODCALLTYPE H_ClearRtv(L* self, D3D12_CPU_DESCRIPTOR_HANDLE h, const FLOAT* c, UINT n, const D3D12_RECT* r) {
    CVR_SR_PREV(48, void(STDMETHODCALLTYPE*)(L*, D3D12_CPU_DESCRIPTOR_HANDLE, const FLOAT*, UINT, const D3D12_RECT*)); prev(self, h, c, n, r);
    Record(self, {Op::Clear, 0, 0, 48, h.ptr});
}
void STDMETHODCALLTYPE H_ClearUavUint(L* self, D3D12_GPU_DESCRIPTOR_HANDLE g, D3D12_CPU_DESCRIPTOR_HANDLE c, ID3D12Resource* r, const UINT* v, UINT n, const D3D12_RECT* rc) {
    CVR_SR_PREV(49, void(STDMETHODCALLTYPE*)(L*, D3D12_GPU_DESCRIPTOR_HANDLE, D3D12_CPU_DESCRIPTOR_HANDLE, ID3D12Resource*, const UINT*, UINT, const D3D12_RECT*)); prev(self, g, c, r, v, n, rc);
    Record(self, {Op::Clear, 0, 0, 49});
}
void STDMETHODCALLTYPE H_ClearUavFloat(L* self, D3D12_GPU_DESCRIPTOR_HANDLE g, D3D12_CPU_DESCRIPTOR_HANDLE c, ID3D12Resource* r, const FLOAT* v, UINT n, const D3D12_RECT* rc) {
    CVR_SR_PREV(50, void(STDMETHODCALLTYPE*)(L*, D3D12_GPU_DESCRIPTOR_HANDLE, D3D12_CPU_DESCRIPTOR_HANDLE, ID3D12Resource*, const FLOAT*, UINT, const D3D12_RECT*)); prev(self, g, c, r, v, n, rc);
    Record(self, {Op::Clear, 0, 0, 50});
}
void STDMETHODCALLTYPE H_Discard(L* self, ID3D12Resource* r, const D3D12_DISCARD_REGION* region) {
    CVR_SR_PREV(51, void(STDMETHODCALLTYPE*)(L*, ID3D12Resource*, const D3D12_DISCARD_REGION*)); prev(self, r, region);
    Record(self, {Op::Discard, 0, 0, 0, reinterpret_cast<uint64_t>(r)});
}
void STDMETHODCALLTYPE H_BeginQuery(L* self, ID3D12QueryHeap* h, D3D12_QUERY_TYPE t, UINT i) {
    CVR_SR_PREV(52, void(STDMETHODCALLTYPE*)(L*, ID3D12QueryHeap*, D3D12_QUERY_TYPE, UINT)); prev(self, h, t, i);
    Record(self, {Op::Query, 0, 0, uint32_t(t), reinterpret_cast<uint64_t>(h), i});
}
void STDMETHODCALLTYPE H_EndQuery(L* self, ID3D12QueryHeap* h, D3D12_QUERY_TYPE t, UINT i) {
    CVR_SR_PREV(53, void(STDMETHODCALLTYPE*)(L*, ID3D12QueryHeap*, D3D12_QUERY_TYPE, UINT)); prev(self, h, t, i);
    Record(self, {Op::Query, 1, 0, uint32_t(t), reinterpret_cast<uint64_t>(h), i});
}
void STDMETHODCALLTYPE H_ResolveQuery(L* self, ID3D12QueryHeap* h, D3D12_QUERY_TYPE t, UINT i, UINT n, ID3D12Resource* r, UINT64 o) {
    CVR_SR_PREV(54, void(STDMETHODCALLTYPE*)(L*, ID3D12QueryHeap*, D3D12_QUERY_TYPE, UINT, UINT, ID3D12Resource*, UINT64)); prev(self, h, t, i, n, r, o);
    Record(self, {Op::Resolve, 0, 0, uint32_t(t)});
}
void STDMETHODCALLTYPE H_Predication(L* self, ID3D12Resource* r, UINT64 o, D3D12_PREDICATION_OP op) {
    CVR_SR_PREV(55, void(STDMETHODCALLTYPE*)(L*, ID3D12Resource*, UINT64, D3D12_PREDICATION_OP)); prev(self, r, o, op);
    Record(self, {Op::Predication, 0, 0, uint32_t(op), reinterpret_cast<uint64_t>(r)});
}
void STDMETHODCALLTYPE H_SetMarker(L* self, UINT m, const void* d, UINT n) {
    CVR_SR_PREV(56, void(STDMETHODCALLTYPE*)(L*, UINT, const void*, UINT)); prev(self, m, d, n);
    Record(self, {Op::Marker, 0, 0, 56});
}
void STDMETHODCALLTYPE H_BeginEvent(L* self, UINT m, const void* d, UINT n) {
    CVR_SR_PREV(57, void(STDMETHODCALLTYPE*)(L*, UINT, const void*, UINT)); prev(self, m, d, n);
    Record(self, {Op::Marker, 0, 0, 57});
}
void STDMETHODCALLTYPE H_EndEvent(L* self) {
    CVR_SR_PREV(58, void(STDMETHODCALLTYPE*)(L*)); prev(self);
    Record(self, {Op::Marker, 0, 0, 58});
}
void STDMETHODCALLTYPE H_Indirect(L* self, ID3D12CommandSignature* sig, UINT max, ID3D12Resource* args, UINT64 argOffset, ID3D12Resource* count, UINT64 countOffset) {
    CVR_SR_PREV(59, void(STDMETHODCALLTYPE*)(L*, ID3D12CommandSignature*, UINT, ID3D12Resource*, UINT64, ID3D12Resource*, UINT64));
    prev(self, sig, max, args, argOffset, count, countOffset);
    const uint64_t extra[3]{argOffset, reinterpret_cast<uint64_t>(count), countOffset};
    Record(self, {Op::Indirect, 0, 0, max, reinterpret_cast<uint64_t>(sig), reinterpret_cast<uint64_t>(args)}, extra, sizeof(extra));
}
void STDMETHODCALLTYPE H_DepthBounds(L* self, FLOAT lo, FLOAT hi) {
    CVR_SR_PREV(62, void(STDMETHODCALLTYPE*)(L*, FLOAT, FLOAT)); prev(self, lo, hi);
    Track(self, [&](Shadow& s) { s.depthMin = lo; s.depthMax = hi; s.depthBoundsSet = true; });
    uint64_t bits = 0; std::memcpy(&bits, &lo, 4); uint32_t h32; std::memcpy(&h32, &hi, 4); bits |= uint64_t(h32) << 32;
    Record(self, {Op::DepthBounds, 0, 0, 0, bits});
}
void STDMETHODCALLTYPE H_SamplePositions(L* self, UINT a, UINT b, D3D12_SAMPLE_POSITION* p) {
    CVR_SR_PREV(63, void(STDMETHODCALLTYPE*)(L*, UINT, UINT, D3D12_SAMPLE_POSITION*)); prev(self, a, b, p);
    Record(self, {Op::Other, 0, 0, 63});
}
void STDMETHODCALLTYPE H_ViewMask(L* self, UINT mask) {
    CVR_SR_PREV(65, void(STDMETHODCALLTYPE*)(L*, UINT)); prev(self, mask);
    Track(self, [&](Shadow& s) { s.viewMask = mask; s.viewMaskSet = true; });
    Record(self, {Op::ViewInstanceMask, 0, 0, mask});
}
void STDMETHODCALLTYPE H_Immediate(L* self, UINT n, const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* p, const D3D12_WRITEBUFFERIMMEDIATE_MODE* m) {
    CVR_SR_PREV(66, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER*, const D3D12_WRITEBUFFERIMMEDIATE_MODE*)); prev(self, n, p, m);
    Record(self, {Op::Immediate, 0, uint16_t(n)});
}
void STDMETHODCALLTYPE H_BeginRenderPass(L* self, UINT n, const D3D12_RENDER_PASS_RENDER_TARGET_DESC* rt, const D3D12_RENDER_PASS_DEPTH_STENCIL_DESC* ds, D3D12_RENDER_PASS_FLAGS f) {
    CVR_SR_PREV(68, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_RENDER_PASS_RENDER_TARGET_DESC*, const D3D12_RENDER_PASS_DEPTH_STENCIL_DESC*, D3D12_RENDER_PASS_FLAGS));
    prev(self, n, rt, ds, f); Record(self, {Op::Targets, 0, uint16_t(n), 68});
}
void STDMETHODCALLTYPE H_EndRenderPass(L* self) {
    CVR_SR_PREV(69, void(STDMETHODCALLTYPE*)(L*)); prev(self);
    Record(self, {Op::Targets, 0, 0, 69});
}
void STDMETHODCALLTYPE H_Pipeline1(L* self, ID3D12StateObject* so) {
    CVR_SR_PREV(75, void(STDMETHODCALLTYPE*)(L*, ID3D12StateObject*)); prev(self, so);
    Record(self, {Op::Other, 0, 0, 75});
}
void STDMETHODCALLTYPE H_ShadingRate(L* self, D3D12_SHADING_RATE rate, const D3D12_SHADING_RATE_COMBINER* c) {
    CVR_SR_PREV(77, void(STDMETHODCALLTYPE*)(L*, D3D12_SHADING_RATE, const D3D12_SHADING_RATE_COMBINER*)); prev(self, rate, c);
    Track(self, [&](Shadow& s) { s.rate = rate; s.combiners = {}; if (c) { s.combiners[0] = c[0]; s.combiners[1] = c[1]; } s.rateSet = true; });
    uint64_t packed = c ? (uint64_t(c[0]) | (uint64_t(c[1]) << 32)) : ~uint64_t(0);
    Record(self, {Op::ShadingRate, 0, 0, uint32_t(rate), packed});
}
void STDMETHODCALLTYPE H_ShadingImage(L* self, ID3D12Resource* image) {
    CVR_SR_PREV(78, void(STDMETHODCALLTYPE*)(L*, ID3D12Resource*)); prev(self, image);
    Track(self, [&](Shadow& s) { s.rateImage = image; s.rateImageSet = true; });
    Record(self, {Op::ShadingImage, 0, 0, 0, reinterpret_cast<uint64_t>(image)});
}
void STDMETHODCALLTYPE H_Barrier7(L* self, UINT n, const D3D12_BARRIER_GROUP* g) {
    CVR_SR_PREV(80, void(STDMETHODCALLTYPE*)(L*, UINT, const D3D12_BARRIER_GROUP*)); prev(self, n, g);
    Record(self, {Op::Barrier, 7, uint16_t(n)});
}
template <size_t SlotIndex>
void STDMETHODCALLTYPE H_OtherVoid5(L* self, void* a, void* b, void* c, void* d, void* e) {
    // Signature-agnostic forwarder for rarely used commands (copies, resolves, meta/raytracing):
    // x64 passes the first four integer/pointer arguments in registers and the rest on the stack,
    // so forwarding five pointer-sized arguments is exact for every method with <= 5 such args and
    // no floating-point parameters. Only used for slots whose methods satisfy that.
    CVR_SR_PREV(SlotIndex, void(STDMETHODCALLTYPE*)(L*, void*, void*, void*, void*, void*)); prev(self, a, b, c, d, e);
    Record(self, {Op::Other, 0, 0, uint32_t(SlotIndex)});
}

struct HookSlot { size_t slot; void* hook; };
// Slots present on every ID3D12GraphicsCommandList.
const HookSlot kBaseSlots[]{
    {10, reinterpret_cast<void*>(&H_Reset)}, {11, reinterpret_cast<void*>(&H_ClearState)},
    {12, reinterpret_cast<void*>(&H_Draw)}, {13, reinterpret_cast<void*>(&H_DrawIndexed)},
    {14, reinterpret_cast<void*>(&H_Dispatch)}, {15, reinterpret_cast<void*>(&H_CopyBuffer)},
    {16, reinterpret_cast<void*>(&H_CopyTexture)}, {17, reinterpret_cast<void*>(&H_CopyResource)},
    {20, reinterpret_cast<void*>(&H_Topology)}, {21, reinterpret_cast<void*>(&H_Viewports)},
    {22, reinterpret_cast<void*>(&H_Scissors)}, {23, reinterpret_cast<void*>(&H_Blend)},
    {24, reinterpret_cast<void*>(&H_Stencil)}, {25, reinterpret_cast<void*>(&H_Pipeline)},
    {26, reinterpret_cast<void*>(&H_Barrier)}, {27, reinterpret_cast<void*>(&H_Bundle)},
    {28, reinterpret_cast<void*>(&H_Heaps)}, {29, reinterpret_cast<void*>(&H_ComputeRoot)},
    {30, reinterpret_cast<void*>(&H_GraphicsRoot)}, {31, reinterpret_cast<void*>(&H_ComputeTable)},
    {32, reinterpret_cast<void*>(&H_GraphicsTable)}, {33, reinterpret_cast<void*>(&H_ComputeConstant)},
    {34, reinterpret_cast<void*>(&H_GraphicsConstant)}, {35, reinterpret_cast<void*>(&H_ComputeConstants)},
    {36, reinterpret_cast<void*>(&H_GraphicsConstants)},
    {37, reinterpret_cast<void*>(&H_RootView<37, Op::ComputeState, 0>)},
    {38, reinterpret_cast<void*>(&H_RootView<38, Op::RootCbv, Shadow::Cbv>)},
    {39, reinterpret_cast<void*>(&H_RootView<39, Op::ComputeState, 0>)},
    {40, reinterpret_cast<void*>(&H_RootView<40, Op::RootSrv, Shadow::Srv>)},
    {41, reinterpret_cast<void*>(&H_RootView<41, Op::ComputeState, 0>)},
    {42, reinterpret_cast<void*>(&H_RootView<42, Op::RootUav, Shadow::Uav>)},
    {43, reinterpret_cast<void*>(&H_IndexBuffer)}, {44, reinterpret_cast<void*>(&H_VertexBuffers)},
    {45, reinterpret_cast<void*>(&H_SoTargets)}, {46, reinterpret_cast<void*>(&H_Targets)},
    {47, reinterpret_cast<void*>(&H_ClearDepth)}, {48, reinterpret_cast<void*>(&H_ClearRtv)},
    {49, reinterpret_cast<void*>(&H_ClearUavUint)}, {50, reinterpret_cast<void*>(&H_ClearUavFloat)},
    {51, reinterpret_cast<void*>(&H_Discard)}, {52, reinterpret_cast<void*>(&H_BeginQuery)},
    {53, reinterpret_cast<void*>(&H_EndQuery)}, {54, reinterpret_cast<void*>(&H_ResolveQuery)},
    {55, reinterpret_cast<void*>(&H_Predication)}, {56, reinterpret_cast<void*>(&H_SetMarker)},
    {57, reinterpret_cast<void*>(&H_BeginEvent)}, {58, reinterpret_cast<void*>(&H_EndEvent)},
    {59, reinterpret_cast<void*>(&H_Indirect)},
};
// ResolveSubresource(19) has exactly five pointer-sized args and no floats. CopyTiles(18) has six
// and stays unchained: a stream containing one is not detected, but the engine never issues it.
const HookSlot kOtherBaseSlots[]{
    {19, reinterpret_cast<void*>(&H_OtherVoid5<19>)},
};
const HookSlot kList1Slots[]{
    {62, reinterpret_cast<void*>(&H_DepthBounds)}, {63, reinterpret_cast<void*>(&H_SamplePositions)},
    {65, reinterpret_cast<void*>(&H_ViewMask)},
};
const HookSlot kList2Slots[]{{66, reinterpret_cast<void*>(&H_Immediate)}};
const HookSlot kList4Slots[]{
    {68, reinterpret_cast<void*>(&H_BeginRenderPass)}, {69, reinterpret_cast<void*>(&H_EndRenderPass)},
    {75, reinterpret_cast<void*>(&H_Pipeline1)},
};
const HookSlot kList5Slots[]{{77, reinterpret_cast<void*>(&H_ShadingRate)}, {78, reinterpret_cast<void*>(&H_ShadingImage)}};
const HookSlot kList7Slots[]{{80, reinterpret_cast<void*>(&H_Barrier7)}};

template <typename F> void ForSlots(const Chain& c, F&& f) {
    auto each = [&](const HookSlot* slots, size_t count) { for (size_t i = 0; i < count; ++i) if (slots[i].slot <= c.last) f(slots[i]); };
    each(kBaseSlots, std::size(kBaseSlots));
    each(kOtherBaseSlots, std::size(kOtherBaseSlots));
    if (c.list1) each(kList1Slots, std::size(kList1Slots));
    if (c.list2) each(kList2Slots, std::size(kList2Slots));
    if (c.list4) each(kList4Slots, std::size(kList4Slots));
    if (c.list5) each(kList5Slots, std::size(kList5Slots));
    if (c.list7) each(kList7Slots, std::size(kList7Slots));
}
// Installs over whatever occupies each slot now (the plugin's own hook or the driver), recording it
// as the predecessor BEFORE the slot points at us. Uninstall puts the predecessor back only where
// the slot still holds our hook, so a later patch by someone else is never undone.
void SetInstalled(Chain& c, bool install) {
    if (c.installed == install) return;
    void** vt = c.vtable;
    DWORD old{};
    if (!VirtualProtect(vt + 10, (c.last - 10 + 1) * sizeof(void*), PAGE_READWRITE, &old)) return;
    ForSlots(c, [&](const HookSlot& slot) {
        void* volatile* address = reinterpret_cast<void* volatile*>(vt + slot.slot);
        if (install) {
            void* current = *address;
            if (current == slot.hook) return;
            c.prev[slot.slot] = current;
            std::atomic_thread_fence(std::memory_order_release);
            InterlockedExchangePointer(address, slot.hook);
        } else {
            InterlockedCompareExchangePointer(address, c.prev[slot.slot], slot.hook);
        }
    });
    DWORD ignored{}; VirtualProtect(vt + 10, (c.last - 10 + 1) * sizeof(void*), old, &ignored);
    c.installed = install;
    Add(CyberpunkVR_StageCensusSummary[3], 1);
    cvr::log("[stage-replay] %s command-list vtable %p (last slot %zu)", install ? "chained" : "unchained", vt, c.last);
}

void InstallOn(const CommandListVtableHook& entry) {
    void** vt = entry.vtable;
    const uint32_t n = g_chainCount.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < n; ++i) if (g_chains[i].vtable == vt) { SetInstalled(g_chains[i], true); return; }
    if (n >= g_chains.size()) return;
    Chain& chain = g_chains[n];
    chain.vtable = vt;
    chain.list1 = entry.atomic_copy32_original != nullptr;
    chain.list2 = entry.write_immediate_original != nullptr;
    chain.list4 = entry.begin_render_pass_original != nullptr;
    chain.list5 = entry.shading_rate_original != nullptr;
    chain.list7 = entry.enhanced_barrier_original != nullptr;
    chain.last = chain.list7 ? 80 : chain.list5 ? 78 : chain.list4 ? 76 : chain.list2 ? 66 : chain.list1 ? 65 : 59;
    for (size_t s = 0; s <= chain.last && s < SlotCount; ++s) chain.prev[s] = vt[s];
    // Publish every predecessor before the first detour becomes reachable.
    g_chainCount.store(n + 1, std::memory_order_release);
    SetInstalled(chain, true);
}

void InstallHooks() {
    std::lock_guard lock(g_installMutex);
    if (!g_shadows) g_shadows = std::make_unique<ShadowSlot[]>(ShadowCapacity);
    const CommandListVtableHook* entries[16]{};
    const uint32_t n = cvr::detail::command_list_hook_entries(entries, 16);
    for (uint32_t i = 0; i < n; ++i) if (entries[i] && entries[i]->vtable) InstallOn(*entries[i]);
    CyberpunkVR_StageCensusSummary[2] = g_chainCount.load(std::memory_order_acquire);
}
bool AnyInstalled() {
    const uint32_t n = g_chainCount.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < n; ++i) if (g_chains[i].installed) return true;
    return false;
}
void UninstallHooks() {
    std::lock_guard lock(g_installMutex);
    const uint32_t n = g_chainCount.load(std::memory_order_acquire);
    for (uint32_t i = 0; i < n; ++i) SetInstalled(g_chains[i], false);
}

// ---------------------------------------------------------------------------------------------
// Census bookkeeping
// ---------------------------------------------------------------------------------------------
std::atomic<bool> g_census{false};
std::atomic<uint32_t> g_censusFrames{0};
std::array<std::atomic<uint32_t>, 2> g_streamTaken{};
std::array<Stream, 2> g_censusStreams;
std::once_flag g_censusStreamInit;

// Ordinal of this call among same (side, stage) calls of the same engine frame.
struct OrdinalState { std::atomic<uint64_t> frameCount{0}; };   // frame << 32 | count
std::array<OrdinalState, 128> g_ordinals;
// Completion of (side, stage, ordinal) for the current frame, used only to report CPU order.
std::array<std::atomic<uint64_t>, 128 * MaxOrdinals> g_done{};

// Calls of (side, stage) in the last completed frame: how many source partitions a target waits for.
std::array<std::atomic<uint32_t>, 128> g_lastCount{};
// Per target stage: frame << 2 | state (1 replay in progress, 2 replayed, 3 failed). The first
// partition of a frame claims the replay; the others wait for its outcome, then skip or record.
std::array<std::atomic<uint64_t>, 64> g_claim{};
uint32_t NextOrdinal(uint32_t row, uint32_t frame) {
    auto& st = g_ordinals[row].frameCount;
    uint64_t cur = st.load(std::memory_order_relaxed);
    for (;;) {
        const uint32_t f = uint32_t(cur >> 32), c = uint32_t(cur);
        const uint64_t next = (f == frame) ? ((uint64_t(frame) << 32) | (c + 1)) : ((uint64_t(frame) << 32) | 1);
        if (st.compare_exchange_weak(cur, next, std::memory_order_relaxed)) {
            if (f != frame && c) g_lastCount[row].store(c, std::memory_order_relaxed);
            return f == frame ? c : 0;
        }
    }
}

void LogEvent(uint32_t frame, uint32_t side, uint32_t stage, uint32_t ordinal, uint32_t kind, int64_t start, int64_t end,
              uint64_t spanBegin, uint64_t spanEnd) {
    if (!g_census.load(std::memory_order_relaxed)) return;
    const uint32_t i = CyberpunkVR_StageEventCount.fetch_add(1, std::memory_order_relaxed);
    if (i >= 16384) return;
    uint64_t* e = CyberpunkVR_StageEvents + size_t(i) * 6;
    e[0] = uint64_t(frame) | (uint64_t(side & 0xFF) << 32) | (uint64_t(stage & 0xFF) << 40) | (uint64_t(ordinal & 0xFF) << 48) | (uint64_t(kind & 0xFF) << 56);
    e[1] = GetCurrentThreadId(); e[2] = uint64_t(start); e[3] = uint64_t(end); e[4] = spanBegin; e[5] = spanEnd;
}

struct CameraInfo { uint64_t vs{}, ps{}; uint64_t packed{}; bool ok{}; };
CameraInfo ReadCamera(const camera_upload::Context& ctx) {
    CameraInfo info;
    if (!ctx) return info;
    camera_upload::CpuSlot vs, ps;
    const bool v = camera_upload::ResolveCameraSlot(&ReadFn, ctx.state, 0, vs);
    const bool p = camera_upload::ResolveCameraSlot(&ReadFn, ctx.state, 1, ps);
    info.vs = v ? vs.descriptor : 0; info.ps = p ? ps.descriptor : 0;
    info.packed = (v ? (uint64_t(vs.parameter) | (uint64_t(vs.index) << 16)) : 0xFFFF) |
                  ((p ? (uint64_t(ps.parameter) | (uint64_t(ps.index) << 16)) : 0xFFFF) << 32);
    info.ok = v || p;
    return info;
}

// The descriptor bound to (shader stage, kind, slot) in a context's CPU table, through the current
// layout's slot maps (sub_140336D3C: kind 0 SRV at layout+0x4C3, 1 UAV +0x8C3, 2 CBV +0x453,
// 3 sampler +0x95B; two bytes per slot, the first is the group). 0 when the layout has no such slot.
uint64_t ReadBound(uint64_t state, uint32_t stage, uint32_t kind, uint32_t slot) {
    static constexpr uint32_t base[4] = {1219, 2243, 1107, 2395}, stride[4] = {256, 38, 28, 32}, slots[4] = {128, 19, 14, 16};
    if (!state || stage > 2 || kind > 3 || slot >= slots[kind]) return 0;
    uint64_t layout = 0, cpu = 0, value = 0;
    uint8_t group = 0xFF; uint32_t start = 0; uint16_t first = 0, count = 0;
    if (!ReadSafe(state + 0x68, &layout, 8) || !ReadSafe(state + 0x28, &cpu, 8) || !layout || !cpu || layout > UINT64_MAX - 0xA00 ||
        !ReadSafe(layout + base[kind] + stage * stride[kind] + slot * 2, &group, 1) || group >= 64) return 0;
    const uint64_t entry = layout + 0x38 + uint64_t(group) * 16;
    if (!ReadSafe(entry + 4, &start, 4) || !ReadSafe(entry + 8, &first, 2) || !ReadSafe(entry + 10, &count, 2) ||
        slot < first || slot - first >= count || start > 8192) return 0;
    return ReadSafe(cpu + (uint64_t(start) + slot - first) * 8, &value, 8) ? value : 0;
}

// The view descriptors of a native context (stage_replay::ViewDescriptors), read at consumer entry.
// A texture slot still holding the null descriptor its neighbours hold counts as unbound: mapping
// it would turn every null descriptor of a replayed table into the target's texture.
stage_replay::ViewDescriptors ReadView(const camera_upload::Context& ctx, const CameraInfo& cam) {
    stage_replay::ViewDescriptors v;
    if (!ctx) return v;
    using V = stage_replay::ViewDescriptors;
    v.d[V::CameraVs] = cam.vs; v.d[V::CameraPs] = cam.ps;
    for (uint32_t stage = 0; stage < 2; ++stage) {
        v.d[stage ? V::ViewPs : V::ViewVs] = ReadBound(ctx.state, stage, 2, 0);
        const uint64_t texture = ReadBound(ctx.state, stage, 0, 37);
        if (texture && texture != ReadBound(ctx.state, stage, 0, 36) && texture != ReadBound(ctx.state, stage, 0, 38))
            v.d[stage ? V::TexturePs : V::TextureVs] = texture;
    }
    return v;
}

struct Slice { uint64_t heap{}; uint32_t base{}, capacity{}, stride{}, cursor{}; bool ok{}; };
Slice ReadSlice(const camera_upload::Context& ctx) {
    Slice s;
    if (!ctx) return s;
    s.ok = ReadSafe(ctx.state, &s.heap, 8) && ReadSafe(ctx.state + 8, &s.base, 4) && ReadSafe(ctx.state + 0xC, &s.capacity, 4) &&
           ReadSafe(ctx.state + 0x10, &s.stride, 4) && ReadSafe(ctx.state + 0x14, &s.cursor, 4);
    return s;
}

struct HeapInfo { ID3D12DescriptorHeap* heap{}; uint64_t cpu{}, gpu{}; uint32_t count{}; };
HeapInfo HeapInfoOf(ID3D12DescriptorHeap* h) {
    HeapInfo info;
    if (!h) return info;
    const auto desc = h->GetDesc();
    if (desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || !(desc.Flags & D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE)) return info;
    info.heap = h; info.cpu = h->GetCPUDescriptorHandleForHeapStart().ptr; info.gpu = h->GetGPUDescriptorHandleForHeapStart().ptr;
    info.count = desc.NumDescriptors;
    return info;
}
HeapInfo ShaderHeap(const Shadow* shadow) {
    if (!shadow || !shadow->heapsSet) return {};
    for (uint32_t i = 0; i < shadow->heapCount; ++i) if (auto info = HeapInfoOf(shadow->heaps[i]); info.heap) return info;
    return {};
}
// The heap the replayed stream binds. Partition lists start without heaps and set them inside the
// consumer, so both the target's mirror and its native slice can be empty at entry; then the heap
// is the shader-visible one every capture binds, and it must agree with the mirror when there is one.
HeapInfo ReplayHeap(const Shadow& shadow, const std::vector<CaptureSlot*>& caps) {
    HeapInfo info = ShaderHeap(&shadow);
    for (const auto* cap : caps) for (const auto& c : cap->stream.Commands()) {
        if (c.op != Op::Heaps) continue;
        const auto* heaps = reinterpret_cast<const uint64_t*>(cap->stream.Payload(c.d, size_t(c.count) * 8));
        HeapInfo bound{};
        for (uint32_t i = 0; heaps && i < c.count && !bound.heap; ++i) bound = HeapInfoOf(reinterpret_cast<ID3D12DescriptorHeap*>(heaps[i]));
        if (!bound.heap || (info.heap && info.heap != bound.heap)) return {};
        info = bound;
    }
    return info;
}

// Descriptors for rebuilt tables come straight from the engine's shader-visible ring -- the same
// frame-retired ring the native context slices are carved from -- so neither the target's slice
// nor its cursor is touched. nullptr selects the audited non-waiting path, which returns the ring
// capacity when the ring is exhausted (src/Render/NativeDescriptorArena.cpp).
bool RingReserve(const HeapInfo& heap, uint32_t count, uint32_t& first) {
    auto* diag = CyberpunkVR_StageReplayCameraDiag;
    const uintptr_t base = Base();
    static const bool compatible = [base] {
        std::array<uint8_t, 177> bytes{};
        if (!base || !ReadSafe(base + 0x419DF4, bytes.data(), bytes.size())) return false;
        uint64_t h = 14695981039346656037ull;
        for (const auto b : bytes) h = (h ^ b) * 1099511628211ull;
        return h == 0xBF311A1C73348164ull;
    }();
    if (!compatible) { Add(diag[8], 1); return false; }
    uint64_t registry = 0, ringHeap = 0, capacity = 0; uint32_t heaps = 0;
    if (!ReadSafe(base + 0x3BCDCA8, &registry, 8) || !registry || registry > UINT64_MAX - 0x14F1E40 - 72) { Add(diag[9], 1); return false; }
    const uint64_t ring = registry + 0x14F1E40;
    if (!ReadSafe(ring + 64, &heaps, 4) || heaps != 1 || !ReadSafe(ring + 56, &ringHeap, 8) || !ReadSafe(ring + 16, &capacity, 8) ||
        ringHeap != reinterpret_cast<uint64_t>(heap.heap) || !capacity || capacity > heap.count) { Add(diag[9], 1); return false; }
    if (!count || count > capacity) { Add(diag[10], 1); return false; }
    using Reserve = uint64_t(__fastcall*)(void*, uint64_t, uint64_t, void*);
    const uint64_t r = reinterpret_cast<Reserve>(base + 0x419DF4)(reinterpret_cast<void*>(ring), count, 1, nullptr);
    if (r >= capacity || count > capacity - r) { Add(diag[10], 1); return false; }
    first = uint32_t(r);
    Add(diag[11], count);
    return true;
}

void EnsureDevice(ID3D12GraphicsCommandList* list) {
    if (g_device) return;
    std::lock_guard lock(g_captureMutex);
    if (g_device) return;
    ID3D12Device* device{};
    if (FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || !device) return;
    g_increment = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    void** vt = *reinterpret_cast<void***>(device);
    g_copyOriginal = GetOriginalMethod<CopyDescriptorsFn>(vt, 23);
    if (!g_copyOriginal) g_copyOriginal = reinterpret_cast<CopyDescriptorsFn>(vt[23]);
    g_device = device;   // the game's device outlives the plugin's use of it; reference kept
}

// ---------------------------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------------------------
void NoteFallback(uint64_t* row, stage_replay::Fallback why) {
    Add(CyberpunkVR_StageReplayStats[7], 1);
    Add(CyberpunkVR_StageReplayStats[16 + std::min<uint32_t>(uint32_t(why), 15)], 1);
    if (row) { Add(row[Fallbacks], 1); Store(row[LastFallback], uint32_t(why)); }
}

void Restore(ID3D12GraphicsCommandList* list, const Shadow& s, const Touched& t) {
    uint32_t ops = 0;
    if (t.heaps && s.heapsSet) { list->SetDescriptorHeaps(s.heapCount, s.heaps.data()); ++ops; }
    if ((t.root || t.params || t.heaps) && s.rootSet) {
        list->SetGraphicsRootSignature(s.root); ++ops;
        for (uint32_t i = 0; i < s.params.size(); ++i) {
            const auto& p = s.params[i];
            switch (p.kind) {
            case Shadow::Table: list->SetGraphicsRootDescriptorTable(i, {p.value}); ++ops; break;
            case Shadow::Cbv: list->SetGraphicsRootConstantBufferView(i, p.value); ++ops; break;
            case Shadow::Srv: list->SetGraphicsRootShaderResourceView(i, p.value); ++ops; break;
            case Shadow::Uav: list->SetGraphicsRootUnorderedAccessView(i, p.value); ++ops; break;
            case Shadow::Constants:
                for (uint32_t w = 0; w < 16; ++w) if (p.wordMask & (1u << w)) { list->SetGraphicsRoot32BitConstant(i, p.words[w], w); ++ops; }
                break;
            default: break;
            }
        }
    }
    if (t.pso && s.psoSet) { list->SetPipelineState(s.pso); ++ops; }
    if (t.topology && s.topologySet) { list->IASetPrimitiveTopology(s.topology); ++ops; }
    if (t.index && s.indexSet) { list->IASetIndexBuffer(s.indexNull ? nullptr : &s.index); ++ops; }
    if (t.vertices) for (uint32_t slot = 0; slot < 16; ++slot) if (s.vertexMask & (1u << slot)) { list->IASetVertexBuffers(slot, 1, &s.vertices[slot]); ++ops; }
    if (t.viewports && s.viewportsSet) { list->RSSetViewports(s.viewportCount, s.viewports.data()); ++ops; }
    if (t.scissors && s.scissorsSet) { list->RSSetScissorRects(s.scissorCount, s.scissors.data()); ++ops; }
    if (t.blend && s.blendSet) { list->OMSetBlendFactor(s.blend.data()); ++ops; }
    if (t.stencil && s.stencilSet) { list->OMSetStencilRef(s.stencil); ++ops; }
    if (t.depthBounds && s.depthBoundsSet) {
        Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList1> l1;
        if (SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&l1)))) { l1->OMSetDepthBounds(s.depthMin, s.depthMax); ++ops; }
    }
    if ((t.rate && s.rateSet) || (t.rateImage && s.rateImageSet)) {
        Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList5> l5;
        if (SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&l5)))) {
            if (t.rate && s.rateSet) { l5->RSSetShadingRate(s.rate, s.combiners.data()); ++ops; }
            if (t.rateImage && s.rateImageSet) { l5->RSSetShadingRateImage(s.rateImage); ++ops; }
        }
    }
    if (t.viewMask && s.viewMaskSet) {
        Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList1> l1;
        if (SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&l1)))) { l1->SetViewInstanceMask(s.viewMask); ++ops; }
    }
    Add(CyberpunkVR_StageReplayStats[10], ops);
}

// The camera groups (VS and PS) of the engine's current binding state: which root parameters carry
// the camera table, and the CPU descriptors that table must contain for THIS view.
// `shape` hashes (group, root parameter, count, first) of each camera group; `start` indexes the
// context's own CPU table and differs between contexts. Each native
// context owns its own layout object, so two views are compared by shape, never by pointer.
struct CameraGroups { uint64_t layout{}, params{}, shape{}; std::vector<camera_upload::flush_state::Table> tables; bool ok{}; };
// Read straight from the binding layout: at consumer entry no program is bound yet and the engine
// may have a full rebind pending, which the strict flush snapshot (NativeCameraFlushState.hpp)
// rejects; the layout's camera groups and the CPU table entries are valid regardless.
CameraGroups ReadCameraGroups(uint64_t state) {
    CameraGroups out;
    if (!state) return out;
    uint64_t layout = 0, cpuTable = 0;
    if (!ReadSafe(state + 0x68, &layout, 8) || !ReadSafe(state + 0x28, &cpuTable, 8) || !layout || !cpuTable ||
        layout > UINT64_MAX - 0x473) return out;
    uint64_t groupsSeen = 0;
    for (uint32_t stage = 0; stage < 2; ++stage) {
        uint8_t group = 0xFF;
        if (!ReadSafe(layout + 0x455 + uint64_t(stage) * 0x1C, &group, 1) || group >= 64) return out;
        if (groupsSeen & (uint64_t(1) << group)) continue;
        groupsSeen |= uint64_t(1) << group;
        const uint64_t entry = layout + 0x38 + uint64_t(group) * 16;
        uint32_t start = 0; uint16_t first = 0, count = 0; uint8_t kind = 0, parameter = 0xFF;
        if (!ReadSafe(entry + 4, &start, 4) || !ReadSafe(entry + 8, &first, 2) || !ReadSafe(entry + 10, &count, 2) ||
            !ReadSafe(entry + 13, &kind, 1) || !ReadSafe(entry + 14, &parameter, 1)) return out;
        if (!count || count > 256 || first > 1 || kind == 2 || parameter >= 64 || start > 8192 || count > 8192 - start) return out;
        camera_upload::flush_state::Table table;
        table.group = group; table.rootParameter = parameter; table.start = start; table.count = count; table.first = first;
        table.descriptors.resize(count);
        if (!ReadSafe(cpuTable + uint64_t(start) * 8, table.descriptors.data(), size_t(count) * 8)) return out;
        for (const auto d : table.descriptors) if (!d) return out;
        out.params |= uint64_t(1) << parameter;
        out.tables.push_back(std::move(table));
    }
    out.layout = layout;
    uint64_t h = 0xCBF29CE484222325ull;
    auto mix = [&](uint64_t v) { h = (h ^ v) * 0x100000001B3ull; };
    for (const auto& t : out.tables) { mix(t.group); mix(t.rootParameter); mix(t.count); mix(t.first); }
    out.shape = h;
    out.ok = out.params != 0;
    return out;
}
CameraGroups ReadCameraGroups(const camera_upload::Context& ctx) { return ctx ? ReadCameraGroups(ctx.state) : CameraGroups{}; }

// The layout pointer changes with every program switch, so this runs often inside a capture: it
// reads the layout's table groups (every group visible to a graphics stage, layout+0x18) without
// allocating and remembers the last few layouts per thread. Census run 826 put the old
// vector-building version at ~100 ns per command.
bool ReadLayoutGroups(uint64_t layout, std::array<Scope::CameraGroup, Scope::MaxGroups>& groups, uint32_t& count) {
    count = 0;
    __try {
        const uint64_t* visibility = reinterpret_cast<const uint64_t*>(layout + 0x18);
        const uint64_t mask = visibility[0] | visibility[1] | visibility[2];
        for (uint32_t group = 0; group < 64 && count < groups.size(); ++group) {
            if (!(mask & (uint64_t(1) << group))) continue;
            const uint64_t entry = layout + 0x38 + uint64_t(group) * 16;
            const uint32_t start = *reinterpret_cast<const uint32_t*>(entry + 4);
            const uint16_t n = *reinterpret_cast<const uint16_t*>(entry + 10);
            const uint8_t kind = *reinterpret_cast<const uint8_t*>(entry + 13);
            const uint8_t parameter = *reinterpret_cast<const uint8_t*>(entry + 14);
            if (!n || n > 256 || kind == 2 || parameter >= 64 || start > 8192 || n > 8192 - start) continue;
            groups[count++] = {parameter, start, n};
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { count = 0; return false; }
    return count != 0;
}
void RefreshScopeGroups(Scope& s) {
    struct Entry { uint64_t layout; std::array<Scope::CameraGroup, Scope::MaxGroups> groups; uint32_t count; };
    thread_local std::array<Entry, 8> cache{};
    thread_local uint32_t next = 0;
    s.groupCount = 0;
    s.layout = *reinterpret_cast<const volatile uint64_t*>(s.state + 0x68);
    s.cpuTable = *reinterpret_cast<const volatile uint64_t*>(s.state + 0x28);
    if (!s.layout || s.layout > UINT64_MAX - 0x473) return;
    for (const auto& e : cache)
        if (e.layout == s.layout) { s.groups = e.groups; s.groupCount = e.count; return; }
    Entry& e = cache[next++ % cache.size()];
    e.layout = s.layout;
    ReadLayoutGroups(s.layout, e.groups, e.count);
    s.groups = e.groups; s.groupCount = e.count;
}

// At the source's bind of a table, the context's CPU table holds exactly the descriptors the bound
// GPU table was flushed from (a cached table is reused only while they are unchanged). A table that
// holds any of the source's view descriptors is kept as payload [count, descriptors...] so the
// replay can rebuild it with the target's (SubstituteView).
void RecordCameraTable(Scope& s, Command& c, std::array<uint64_t, 257>& buffer, size_t& bytes) {
    bytes = 0;
    if (*reinterpret_cast<const volatile uint64_t*>(s.state + 0x68) != s.layout) RefreshScopeGroups(s);
    for (uint32_t k = 0; k < s.groupCount; ++k) {
        const auto& g = s.groups[k];
        if (g.param != c.a || !s.cpuTable || !g.count || g.count > 256) continue;
        const uint64_t* entries = reinterpret_cast<const uint64_t*>(s.cpuTable) + g.start;
        bool view = false;
        for (uint32_t i = 0; i < g.count; ++i) {
            buffer[1 + i] = entries[i];
            view |= s.view.Contains(entries[i]);
        }
        if (!view) return;
        buffer[0] = g.count;
        bytes = size_t(g.count + 1) * 8;
        c.flags |= 8;
        if (s.row) Add(s.row[CameraTables], 1);
        return;
    }
}

// Census diagnostic: the CPU-table contents behind any root table bind, found through the layout
// group whose root parameter is the bound one (groups visible to VS/PS/GS, layout+0x18).
size_t RecordAnyTable(const Scope& s, const Command& c, std::array<uint64_t, 257>& buffer) {
    __try {
        const uint64_t layout = *reinterpret_cast<const volatile uint64_t*>(s.state + 0x68);
        const uint64_t cpuTable = *reinterpret_cast<const volatile uint64_t*>(s.state + 0x28);
        if (!layout || !cpuTable || layout > UINT64_MAX - 0x473) return 0;
        const uint64_t* visibility = reinterpret_cast<const uint64_t*>(layout + 0x18);
        const uint64_t groups = visibility[0] | visibility[1] | visibility[2];
        for (uint32_t group = 0; group < 64; ++group) {
            if (!(groups & (uint64_t(1) << group))) continue;
            const uint64_t entry = layout + 0x38 + uint64_t(group) * 16;
            const uint32_t start = *reinterpret_cast<const uint32_t*>(entry + 4);
            const uint16_t n = *reinterpret_cast<const uint16_t*>(entry + 10);
            const uint8_t parameter = *reinterpret_cast<const uint8_t*>(entry + 14);
            if (parameter != c.a || !n || n > 256 || start > 8192) continue;
            buffer[0] = n | (uint64_t(group) << 32);
            std::memcpy(buffer.data() + 1, reinterpret_cast<const uint64_t*>(cpuTable) + start, size_t(n) * 8);
            return size_t(n + 1) * 8;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return 0;
}

// Blocking ops other than the barriers AllowBarriers lets through.
bool Replayable(const Stream& s, bool allowBarriers) {
    if (!s.BlockingOps()) return true;
    if (!allowBarriers) return false;
    for (const auto& c : s.Commands()) if (Blocking(c.op) && c.op != Op::Barrier) return false;
    return true;
}

// `cameraTables`: (command index, target GPU table) for every camera-table bind, ascending.
void IssueStream(ID3D12GraphicsCommandList* list, const Stream& s, const std::vector<std::pair<size_t, uint64_t>>& cameraTables,
                 uint32_t& issued) {
    const auto& cmds = s.Commands();
    size_t nextTable = 0;
    for (size_t index = 0; index < cmds.size(); ++index) {
        const Command& c = cmds[index];
        switch (c.op) {
        case Op::Pipeline: list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(c.b)); break;
        case Op::RootSignature: list->SetGraphicsRootSignature(reinterpret_cast<ID3D12RootSignature*>(c.b)); break;
        case Op::RootTable: {
            uint64_t gpu = c.b;
            if (nextTable < cameraTables.size() && cameraTables[nextTable].first == index) gpu = cameraTables[nextTable++].second;   // the target's camera
            list->SetGraphicsRootDescriptorTable(c.a, {gpu}); break;
        }
        case Op::RootConstant: list->SetGraphicsRoot32BitConstant(c.a, uint32_t(c.b), uint32_t(c.c)); break;
        case Op::RootConstants: if (auto* p = s.Payload(c.d, size_t(c.count) * 4)) list->SetGraphicsRoot32BitConstants(c.a, c.count, p, uint32_t(c.c)); break;
        case Op::RootCbv: list->SetGraphicsRootConstantBufferView(c.a, c.b); break;
        case Op::RootSrv: list->SetGraphicsRootShaderResourceView(c.a, c.b); break;
        case Op::RootUav: list->SetGraphicsRootUnorderedAccessView(c.a, c.b); break;
        case Op::Topology: list->IASetPrimitiveTopology(D3D12_PRIMITIVE_TOPOLOGY(c.a)); break;
        case Op::IndexBuffer: list->IASetIndexBuffer((c.flags & 4) ? nullptr : reinterpret_cast<const D3D12_INDEX_BUFFER_VIEW*>(s.Payload(c.d, sizeof(D3D12_INDEX_BUFFER_VIEW)))); break;
        case Op::VertexBuffers: list->IASetVertexBuffers(c.a, c.count, (c.flags & 4) ? nullptr : reinterpret_cast<const D3D12_VERTEX_BUFFER_VIEW*>(s.Payload(c.d, size_t(c.count) * sizeof(D3D12_VERTEX_BUFFER_VIEW)))); break;
        case Op::Viewports: if (auto* p = s.Payload(c.d, size_t(c.count) * sizeof(D3D12_VIEWPORT))) list->RSSetViewports(c.count, reinterpret_cast<const D3D12_VIEWPORT*>(p)); break;
        case Op::Scissors: if (auto* p = s.Payload(c.d, size_t(c.count) * sizeof(D3D12_RECT))) list->RSSetScissorRects(c.count, reinterpret_cast<const D3D12_RECT*>(p)); break;
        case Op::BlendFactor: list->OMSetBlendFactor((c.flags & 4) ? nullptr : reinterpret_cast<const FLOAT*>(s.Payload(c.d, 16))); break;
        case Op::StencilRef: list->OMSetStencilRef(c.a); break;
        case Op::DepthBounds: {
            Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList1> l1;
            if (SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&l1)))) { float lo, hi; uint32_t a = uint32_t(c.b), b = uint32_t(c.b >> 32); std::memcpy(&lo, &a, 4); std::memcpy(&hi, &b, 4); l1->OMSetDepthBounds(lo, hi); }
            break;
        }
        case Op::Heaps: if (auto* p = s.Payload(c.d, size_t(c.count) * sizeof(void*))) list->SetDescriptorHeaps(c.count, reinterpret_cast<ID3D12DescriptorHeap* const*>(p)); break;
        case Op::ShadingRate: {
            Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList5> l5;
            if (SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&l5)))) {
                if (c.b == ~uint64_t(0)) l5->RSSetShadingRate(D3D12_SHADING_RATE(c.a), nullptr);
                else { const D3D12_SHADING_RATE_COMBINER comb[2]{D3D12_SHADING_RATE_COMBINER(uint32_t(c.b)), D3D12_SHADING_RATE_COMBINER(uint32_t(c.b >> 32))}; l5->RSSetShadingRate(D3D12_SHADING_RATE(c.a), comb); }
            }
            break;
        }
        case Op::ShadingImage: break;   // per eye: the target keeps its own image
        case Op::ViewInstanceMask: {
            Microsoft::WRL::ComPtr<ID3D12GraphicsCommandList1> l1;
            if (SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&l1)))) l1->SetViewInstanceMask(c.a);
            break;
        }
        case Op::Draw: list->DrawInstanced(c.a, uint32_t(c.b), uint32_t(c.b >> 32), uint32_t(c.c)); ++issued; break;
        case Op::DrawIndexed: list->DrawIndexedInstanced(c.a, uint32_t(c.b), uint32_t(c.b >> 32), int32_t(uint32_t(c.c)), uint32_t(c.c >> 32)); ++issued; break;
        case Op::Indirect: {
            const uint64_t* extra = reinterpret_cast<const uint64_t*>(s.Payload(c.d, 24));
            if (extra) { list->ExecuteIndirect(reinterpret_cast<ID3D12CommandSignature*>(c.b), c.a, reinterpret_cast<ID3D12Resource*>(c.c), extra[0],
                                               reinterpret_cast<ID3D12Resource*>(extra[1]), extra[2]); ++issued; }
            break;
        }
        default: break;   // markers, copies, compute state, and (when allowed) barriers are not replayed
        }
    }
}

// Re-issues the source partitions, in source slice order, into the target's list, then restores the
// target's mirrored state. With `camera`, every binding of a camera-group root parameter is replaced
// by the target's own camera table; the layouts must agree so the parameters mean the same thing.
bool Replay(ID3D12GraphicsCommandList* list, const std::vector<CaptureSlot*>& caps, const Shadow& shadow, const camera_upload::Context& ctx,
            bool camera, uint64_t camVs, uint64_t camPs, const stage_replay::ViewDescriptors& target, uint64_t* row) {
    (void)ctx;
    std::vector<std::vector<std::pair<size_t, uint64_t>>> cameraTables(caps.size());
    if (camera) {
        auto* diag = CyberpunkVR_StageReplayCameraDiag;
        if (!camVs || !camPs) { Add(diag[0], 1); NoteFallback(row, stage_replay::Fallback::Camera); return false; }
        const HeapInfo heap = ReplayHeap(shadow, caps);
        if (!heap.heap || !g_copyOriginal || !g_device || !g_increment) { Add(diag[1], 1); NoteFallback(row, stage_replay::Fallback::NoHeap); return false; }
        // Unique substituted lists, laid out back to back in one ring reservation.
        struct Unique { uint64_t hash; uint32_t offset, count; };
        std::vector<Unique> unique;
        std::vector<uint64_t> flat;
        struct Use { size_t cap, command; uint32_t unique; };
        std::vector<Use> uses;
        std::array<uint64_t, 256> sources;
        for (size_t k = 0; k < caps.size(); ++k) {
            const auto& stream = caps[k]->stream;
            const auto& cmds = stream.Commands();
            for (size_t i = 0; i < cmds.size(); ++i) {
                const Command& c = cmds[i];
                if (c.op != Op::RootTable || !(c.flags & 8)) continue;
                const uint64_t* head = reinterpret_cast<const uint64_t*>(stream.Payload(c.d, 8));
                const uint32_t n = head ? uint32_t(head[0]) : 0;
                const uint64_t* payload = (n && n <= 256) ? reinterpret_cast<const uint64_t*>(stream.Payload(c.d, size_t(n + 1) * 8)) : nullptr;
                if (!payload) { NoteFallback(row, stage_replay::Fallback::Camera); return false; }
                std::memcpy(sources.data(), payload + 1, size_t(n) * 8);
                if (SubstituteView(sources.data(), n, caps[k]->view, target) <= 0) {
                    Add(diag[2], 1); NoteFallback(row, stage_replay::Fallback::Camera); return false;
                }
                uint64_t h = 0xCBF29CE484222325ull;
                for (uint32_t j = 0; j < n; ++j) h = (h ^ sources[j]) * 0x100000001B3ull;
                h = (h ^ n) * 0x100000001B3ull;
                uint32_t found = UINT32_MAX;
                for (uint32_t u = 0; u < unique.size(); ++u)
                    if (unique[u].hash == h && unique[u].count == n && !std::memcmp(flat.data() + unique[u].offset, sources.data(), size_t(n) * 8)) { found = u; break; }
                if (found == UINT32_MAX) {
                    found = uint32_t(unique.size());
                    unique.push_back({h, uint32_t(flat.size()), n});
                    flat.insert(flat.end(), sources.begin(), sources.begin() + n);
                    Add(diag[3], 1);
                } else Add(diag[4], 1);
                uses.push_back({k, i, found});
                Store(diag[5], camVs); Store(diag[6], caps[k]->camVs); Store(diag[7], n);
            }
        }
        if (!flat.empty()) {
            if (flat.size() > 8192) { NoteFallback(row, stage_replay::Fallback::Capacity); return false; }
            uint32_t first = 0;
            if (!RingReserve(heap, uint32_t(flat.size()), first)) { NoteFallback(row, stage_replay::Fallback::Capacity); return false; }
            std::vector<D3D12_CPU_DESCRIPTOR_HANDLE> src(flat.size());
            std::vector<UINT> ones(flat.size(), 1);
            for (size_t j = 0; j < flat.size(); ++j) src[j].ptr = SIZE_T(flat[j]);
            const D3D12_CPU_DESCRIPTOR_HANDLE dst{SIZE_T(heap.cpu + uint64_t(first) * g_increment)};
            const UINT dstCount = UINT(flat.size());
            g_copyOriginal(g_device, 1, &dst, &dstCount, UINT(flat.size()), src.data(), ones.data(), D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            for (const auto& use : uses)
                cameraTables[use.cap].emplace_back(use.command, heap.gpu + uint64_t(first + unique[use.unique].offset) * g_increment);
            Add(CyberpunkVR_StageReplayStats[8], unique.size()); Add(CyberpunkVR_StageReplayStats[9], flat.size());
        }
    }
    // The state to restore TO, taken before the replayed commands move the mirror.
    thread_local Shadow entry;
    entry = shadow;
    t_replaying = true;
    uint32_t issued = 0;
    Touched touched{};
    for (size_t k = 0; k < caps.size(); ++k) {
        IssueStream(list, caps[k]->stream, cameraTables[k], issued);
        const Touched t = Touched::Of(caps[k]->stream);
        touched.pso |= t.pso; touched.root |= t.root; touched.params |= t.params; touched.topology |= t.topology;
        touched.index |= t.index; touched.vertices |= t.vertices; touched.viewports |= t.viewports; touched.scissors |= t.scissors;
        touched.blend |= t.blend; touched.stencil |= t.stencil; touched.depthBounds |= t.depthBounds; touched.heaps |= t.heaps;
        touched.rate |= t.rate; touched.rateImage |= t.rateImage; touched.viewMask |= t.viewMask;
    }
    Restore(list, entry, touched);
    t_replaying = false;
    Add(CyberpunkVR_StageReplayStats[5], issued);
    return true;
}

}  // namespace

// ---------------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------------
bool Active() { return g_active.load(std::memory_order_relaxed); }
uint64_t GraphSalt() {
    return CyberpunkVR_VrcamGbufferPartitionsState.load(std::memory_order_relaxed) == 1 ? 0x9E3779B97F4A7C15ull : 0;
}

namespace {
// The census half of Copy, apart so that the 4 KB source list is not in the frame of every
// CopyDescriptors call of the game: its stack probe ran on all of them.
__declspec(noinline) void CensusCopy(Scope* s, UINT n, const D3D12_CPU_DESCRIPTOR_HANDLE* dest, const UINT* sizes, UINT m,
                                     const D3D12_CPU_DESCRIPTOR_HANDLE* src, const UINT* counts) {
    // Flatten sources.
    std::array<uint64_t, 512> flat{}; uint32_t total = 0;
    for (UINT r = 0; r < m && total < flat.size(); ++r) {
        const UINT c = counts ? counts[r] : 1;
        for (UINT i = 0; i < c && total < flat.size(); ++i) flat[total++] = src[r].ptr + uint64_t(i) * s->stride;
    }
    uint32_t consumed = 0;
    for (UINT r = 0; r < n; ++r) {
        const UINT size = sizes ? sizes[r] : 1;
        const uint64_t start = dest[r].ptr;
        if (consumed + size > total) break;
        if (start >= s->heapCpu && start < s->heapCpu + uint64_t(s->heapCount) * s->stride) {
            Command c{Op::Copy, 0, uint16_t(size), uint32_t((start - s->heapCpu) / s->stride)};
            bool camera = false;
            for (UINT i = 0; i < size; ++i) if ((s->camVs && flat[consumed + i] == s->camVs) || (s->camPs && flat[consumed + i] == s->camPs)) camera = true;
            if (camera) c.flags |= 2;
            if (s->row) Add(s->row[CopiedDescriptors], size);
            Record(nullptr, c, flat.data() + consumed, size_t(size) * 8);
        }
        consumed += size;
    }
}
}  // namespace

// Called from the device CopyDescriptors hooks. Only CBV/SRV/UAV copies into the scope's
// shader-visible heap are kept: they are the contents of the tables the scope's draws bind.
void Copy(ID3D12Device* device, UINT n, const D3D12_CPU_DESCRIPTOR_HANDLE* dest, const UINT* sizes, UINT m,
          const D3D12_CPU_DESCRIPTOR_HANDLE* src, const UINT* counts, D3D12_DESCRIPTOR_HEAP_TYPE type) {
    Scope* s = t_scope;
    if (!s || t_replaying || !s->stream || type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || !s->stride || !dest || !src) return;
    if (s->row) Add(s->row[Copies], 1);
    // A capture needs no copies: replay ignores them and takes camera-table contents from the
    // context's CPU table at each bind (RecordCameraTable). Only census streams keep them.
    if (s->capture) return;
    CensusCopy(s, n, dest, sizes, m, src, counts);
}

// Consumer hook entry. Returns true when the native consumer must NOT run (skip or replay done).
enum EventKind : uint32_t { KindForward = 0, KindCapture = 1, KindSkip = 2, KindReplay = 3, KindFallback = 4, KindPresent = 5 };
struct Ticket {
    Scope scope; Scope* previous{}; CaptureSlot* capture{}; uint64_t* row{}; int64_t start{};
    uint64_t spanBegin{}, spanEnd{};
    uint32_t rowIndex{}, ordinal{}, frame{}, stage{}, kind{}; bool active{}, censusStream{}; int32_t side{-1};
    uint64_t context{}, state{};
};

// Acquires every source partition of (stage, frame) for reading, waiting up to waitUs. On failure
// nothing stays acquired and the reason is returned.
stage_replay::Fallback AcquireSources(uint32_t stage, uint32_t frame, uint32_t first, uint32_t expected, uint32_t waitUs, bool allowBarriers,
                                      std::vector<CaptureSlot*>& out) {
    out.clear();
    if (!expected || first >= MaxOrdinals || expected > MaxOrdinals - first) return stage_replay::Fallback::NoCapture;
    const int64_t deadline = Now() + int64_t(waitUs) * 10;   // QPC is 10 MHz on supported Windows
    for (;;) {
        bool pending = false;
        stage_replay::Fallback why = stage_replay::Fallback::None;
        for (uint32_t o = first; o < first + expected; ++o) {
            CaptureSlot* cap = Slot(stage, o, frame);
            if (!cap) { why = stage_replay::Fallback::Ordinal; break; }
            uint32_t state = Complete;
            if (!cap->state.compare_exchange_strong(state, Reading, std::memory_order_acq_rel)) {
                pending = true; why = state == Writing ? stage_replay::Fallback::Busy : stage_replay::Fallback::NoCapture; break;
            }
            out.push_back(cap);
            if (!SameFrame(cap->frame, frame, true)) { pending = true; why = stage_replay::Fallback::StaleFrame; break; }
            if (cap->stream.Overflowed()) { why = stage_replay::Fallback::CaptureOverflow; break; }
            if (!Replayable(cap->stream, allowBarriers)) { why = stage_replay::Fallback::CaptureBlocking; break; }
            if (cap->foreign) { why = stage_replay::Fallback::CaptureList; break; }
        }
        if (why == stage_replay::Fallback::None) {
            std::sort(out.begin(), out.end(), [](const CaptureSlot* a, const CaptureSlot* b) { return a->spanBegin < b->spanBegin; });
            return why;
        }
        for (auto* cap : out) cap->state.store(Complete, std::memory_order_release);
        out.clear();
        if (!pending || !waitUs || Now() >= deadline) return why;
        YieldProcessor();
    }
}

// Several source calls of one stage are either partitions of ONE pass -- slices of a single packet
// array, so their spans tile it end to start -- or separate passes (stage 24: three calls per view,
// spans in different arrays). Partitions are replayed whole by the first target call of the frame;
// separate passes pair by ordinal. Read without acquiring: the source finished the stage before
// the target reached it, otherwise the replay falls back anyway.
bool SourcePartitioned(uint32_t stage, uint32_t frame, uint32_t expected) {
    if (expected <= 1) return true;
    if (expected > MaxOrdinals) return false;
    std::array<std::pair<uint64_t, uint64_t>, MaxOrdinals> spans{};
    for (uint32_t o = 0; o < expected; ++o) {
        CaptureSlot* cap = Slot(stage, o, frame);
        if (!cap || cap->state.load(std::memory_order_acquire) != Complete || !SameFrame(cap->frame, frame, true)) return true;
        spans[o] = {cap->spanBegin, cap->spanEnd};
    }
    std::sort(spans.begin(), spans.begin() + expected);
    for (uint32_t o = 0; o + 1 < expected; ++o) if (spans[o].second != spans[o + 1].first) return false;
    return true;
}

bool Enter(Ticket& t, int32_t side, uint32_t node, const void* arguments, const void* span) {
    t = {};
    if (!g_active.load(std::memory_order_relaxed)) return false;
    uint32_t stage = 0xFF;
    uint64_t args[4]{};
    if (arguments && ReadSafe(reinterpret_cast<uintptr_t>(arguments), args, sizeof(args))) stage = uint32_t(args[2]);
    if (node != 0x33DDA4u || side < 0 || side > 1) return false;
    const uint32_t stage64 = stage < 63 ? stage : 63;
    t.rowIndex = uint32_t(side) * 64 + stage64; t.side = side; t.stage = stage;
    const bool census = g_census.load(std::memory_order_relaxed);
    t.row = census ? CyberpunkVR_StageCensusRows[t.rowIndex] : nullptr;
    t.frame = EngineFrame();
    t.ordinal = NextOrdinal(t.rowIndex, t.frame);
    uint64_t spanWords[2]{};
    if (span && ReadSafe(reinterpret_cast<uintptr_t>(span), spanWords, 16)) { t.spanBegin = spanWords[0]; t.spanEnd = spanWords[1]; }
    const Settings settings{CyberpunkVR_StageReplayMode.load(std::memory_order_relaxed), CyberpunkVR_StageReplayStages.load(std::memory_order_relaxed),
                            CyberpunkVR_StageReplaySource.load(std::memory_order_relaxed)};
    const Action action = Decide(settings, side, node, stage);
    const bool wantStream = census && stage == CyberpunkVR_StageStreamStage.load(std::memory_order_relaxed) && t.ordinal == 0 &&
        CyberpunkVR_StageCensusSummary[0] >= 3;   // list mirrors re-established after the hooks went in
    if (!census && action == Action::Forward) return false;

    auto ctx = camera_upload::CurrentContext(Base());
    auto* list = ctx ? reinterpret_cast<ID3D12GraphicsCommandList*>(ctx.list) : nullptr;
    if (list) EnsureDevice(list);
    Shadow* shadow = list ? ShadowFor(list, false) : nullptr;
    const bool shadowValid = shadow && shadow->valid && shadow->generation == g_generation.load(std::memory_order_relaxed);
    const CameraInfo cam = ReadCamera(ctx);
    if (t.row) {
        Add(t.row[Calls], 1);
        Store(t.row[LastEngineFrame], t.frame); Max(t.row[MaxOrdinal], t.ordinal + 1);
        const uint32_t peer = uint32_t(1 - side) * 64 + stage64;
        if (g_done[peer * MaxOrdinals + std::min(t.ordinal, MaxOrdinals - 1)].load(std::memory_order_acquire) == (uint64_t(t.frame) << 1 | 1)) Add(t.row[PeerDone], 1);
        else Add(t.row[PeerPending], 1);
        if (cam.ok) { Store(t.row[CamVs], cam.vs); Store(t.row[CamPs], cam.ps); Store(t.row[CameraSlots], cam.packed); }
        if (ctx) Add(t.row[ContextOk], 1);
        for (int i = 0; i < 4; ++i) Store(t.row[Args0 + i], args[i]);
        { const CameraGroups groups = ReadCameraGroups(ctx); Store(t.row[CameraParams], groups.ok ? groups.params : 0); }
        Store(t.row[EntryList], ctx.list);
        if (shadowValid) Add(t.row[ShadowValid], 1);
        if (t.spanEnd >= t.spanBegin) Add(t.row[Packets], (t.spanEnd - t.spanBegin) / 16);
    }
    const HeapInfo heap = ShaderHeap(shadow);
    auto fill = [&](Scope& sc) {
        sc.row = t.row; sc.ops = census ? CyberpunkVR_StageCensusOps[t.rowIndex] : nullptr;
        sc.list = ctx.list; sc.heapCpu = heap.cpu; sc.heapGpu = heap.gpu; sc.heapCount = heap.count;
        sc.stride = g_increment; sc.camVs = cam.vs; sc.camPs = cam.ps; sc.state = ctx ? ctx.state : 0;
    };
    auto begin = [&](uint32_t kind) { t.kind = kind; t.previous = t_scope; t_scope = &t.scope; t.active = true; t.start = Now(); };

    if (action == Action::Skip) {
        const int64_t now = Now();
        Add(CyberpunkVR_StageReplayStats[3], 1); if (t.row) Add(t.row[Skipped], 1);
        LogEvent(t.frame, side, stage, t.ordinal, KindSkip, now, now, t.spanBegin, t.spanEnd);
        return true;
    }
    if (action == Action::Replay) {
        const int64_t start = Now();
        const uint32_t sourceCount = g_lastCount[settings.source * 64 + stage64].load(std::memory_order_relaxed);
        const bool paired = !SourcePartitioned(stage, t.frame, sourceCount);
        if (paired) Add(CyberpunkVR_StageReplayStats[11], 1);
        // Partitions: one target call per frame replays the whole source stage; the others follow.
        if (stage < 64 && !paired) {
            auto& claim = g_claim[stage];
            uint64_t cur = claim.load(std::memory_order_acquire);
            const uint64_t mine = (uint64_t(t.frame) << 2) | 1;
            bool claimed = false;
            while (!claimed && (cur >> 2) != t.frame) claimed = claim.compare_exchange_weak(cur, mine, std::memory_order_acq_rel);
            if (!claimed) {
                const int64_t deadline = start + 50000;   // 5 ms at 10 MHz
                while ((cur >> 2) == t.frame && (cur & 3) == 1 && Now() < deadline) { YieldProcessor(); cur = claim.load(std::memory_order_acquire); }
                if ((cur >> 2) == t.frame && (cur & 3) == 2) {
                    Add(CyberpunkVR_StageReplayStats[3], 1); if (t.row) Add(t.row[Skipped], 1);
                    LogEvent(t.frame, side, stage, t.ordinal, KindSkip, start, Now(), t.spanBegin, t.spanEnd);
                    return true;
                }
                // The leading partition failed (its reason is recorded) or did not finish in time.
                NoteFallback(t.row, (cur >> 2) == t.frame && (cur & 3) == 3 ? stage_replay::Fallback::Leader : stage_replay::Fallback::Busy);
                fill(t.scope); begin(KindFallback);
                return false;
            }
        }
        bool replayed = false;
        stage_replay::Fallback why = stage_replay::Fallback::None;
        if (!list) why = stage_replay::Fallback::NoContext;
        else if (!shadowValid) why = stage_replay::Fallback::ShadowInvalid;
        else if (shadow->overflow) why = stage_replay::Fallback::ShadowOverflow;
        else {
            std::vector<CaptureSlot*> caps;
            const uint32_t targetCount = g_lastCount[t.rowIndex].load(std::memory_order_relaxed);
            if (paired && (t.ordinal >= sourceCount || targetCount != sourceCount)) why = stage_replay::Fallback::Ordinal;
            else why = AcquireSources(stage, t.frame, paired ? t.ordinal : 0, paired ? 1 : sourceCount,
                                      CyberpunkVR_StageReplayWaitUs.load(std::memory_order_relaxed),
                                      CyberpunkVR_StageReplayAllowBarriers.load(std::memory_order_relaxed) != 0, caps);
            if (why == stage_replay::Fallback::None) {
                const bool camera = settings.mode == uint32_t(Mode::ReplayCamera);
                stage_replay::ViewDescriptors target;
                if (camera) {
                    target = ReadView(ctx, cam);
                    for (uint32_t k = 0; k < target.d.size(); ++k) CyberpunkVR_StageReplayViewDiag[1][k] = target.d[k];
                    CyberpunkVR_StageReplayViewDiag[1][7] = stage;
                }
                replayed = Replay(list, caps, *shadow, ctx, camera, cam.vs, cam.ps, target, t.row);
                for (auto* cap : caps) cap->state.store(Complete, std::memory_order_release);
            }
        }
        if (stage < 64 && !paired) g_claim[stage].store((uint64_t(t.frame) << 2) | (replayed ? 2 : 3), std::memory_order_release);
        if (!replayed) {
            if (why != stage_replay::Fallback::None) NoteFallback(t.row, why);
            fill(t.scope); begin(KindFallback);   // native consumer, still counted by a running census
            return false;
        }
        const int64_t end = Now();
        Add(CyberpunkVR_StageReplayStats[4], 1); Add(CyberpunkVR_StageReplayStats[6], uint64_t(end - start));
        if (t.row) { Add(t.row[Replayed], 1); Add(t.row[ReplayTicks], uint64_t(end - start)); }
        LogEvent(t.frame, side, stage, t.ordinal, KindReplay, start, end, t.spanBegin, t.spanEnd);
        return true;
    }
    // Forward with a scope: census counting, census stream, or source capture.
    fill(t.scope);
    if (action == Action::Capture && settings.mode >= uint32_t(Mode::Replay)) {
        CaptureSlot* cap = (t.ordinal < MaxOrdinals) ? Slot(stage, t.ordinal, t.frame) : nullptr;
        if (cap) {
            uint32_t st = cap->state.load(std::memory_order_acquire);
            if ((st == Empty || st == Complete) && cap->state.compare_exchange_strong(st, Writing, std::memory_order_acq_rel)) {
                cap->stream.Clear(); cap->frame = t.frame; cap->stage = stage; cap->ordinal = t.ordinal;
                cap->list = ctx.list; cap->camVs = cam.vs; cap->camPs = cam.ps; cap->foreign = false;
                cap->view = ReadView(ctx, cam); t.scope.view = cap->view;
                for (uint32_t k = 0; k < cap->view.d.size(); ++k) CyberpunkVR_StageReplayViewDiag[0][k] = cap->view.d[k];
                CyberpunkVR_StageReplayViewDiag[0][7] = stage;
                cap->spanBegin = t.spanBegin; cap->spanEnd = t.spanEnd;
                t.capture = cap; t.scope.stream = &cap->stream; t.scope.capture = cap;
                t.scope.preserveCamera = settings.mode == uint32_t(Mode::ReplayCamera);
                Add(CyberpunkVR_StageReplayStats[0], 1);
            } else Add(CyberpunkVR_StageReplayStats[2], 1);
        }
    } else if (wantStream && g_streamTaken[side].exchange(1) == 0) {
        std::call_once(g_censusStreamInit, [] { for (auto& s : g_censusStreams) s.Reserve(32768, 2u << 20); });
        g_censusStreams[side].Clear(); t.scope.stream = &g_censusStreams[side]; t.censusStream = true;
        t.context = ctx.context; t.state = ctx.state;
        auto* info = CyberpunkVR_StageStreamInfo[side];
        info[5] = ctx.list; info[6] = t.frame; info[7] = stage; info[8] = cam.vs; info[9] = cam.ps;
        info[10] = heap.cpu; info[11] = heap.gpu; info[12] = g_increment;
        const Slice slice = ReadSlice(ctx);
        info[13] = slice.heap; info[14] = uint64_t(slice.base) | (uint64_t(slice.capacity) << 32); info[15] = slice.cursor;
    }
    begin(t.capture ? KindCapture : KindForward);
    return false;
}

void Leave(Ticket& t);
__int64 Consume(int32_t side, uint32_t node, void* renderer, const void* arguments, const void* span, ConsumerFn forward) {
    Ticket t;
    if (Enter(t, side, node, arguments, span)) return 0;
    const __int64 result = forward(renderer, arguments, span);
    Leave(t);
    return result;
}

void Leave(Ticket& t) {
    if (!t.active) return;
    t_scope = t.previous;
    const int64_t end = Now();
    if (t.row) Add(t.row[Ticks], uint64_t(end - t.start));
    if (t.capture) {
        if (t.capture->stream.Overflowed()) Add(CyberpunkVR_StageReplayStats[1], 1);
        t.capture->state.store(Complete, std::memory_order_release);
    }
    if (t.censusStream && t.side >= 0) {
        auto& s = g_censusStreams[t.side]; auto* info = CyberpunkVR_StageStreamInfo[t.side];
        info[0] = reinterpret_cast<uint64_t>(s.Commands().data()); info[1] = s.Commands().size();
        info[2] = reinterpret_cast<uint64_t>(s.Payload(0, 0)); info[3] = s.PayloadBytes(); info[4] = s.Overflowed();
        CyberpunkVR_StageStreamContextAddr[t.side][0] = t.context; CyberpunkVR_StageStreamContextAddr[t.side][1] = t.state;
        if (!t.context || !ReadSafe(t.context, CyberpunkVR_StageStreamContextDump[t.side], sizeof(CyberpunkVR_StageStreamContextDump[0])))
            std::memset(CyberpunkVR_StageStreamContextDump[t.side], 0, sizeof(CyberpunkVR_StageStreamContextDump[0]));
        if (!t.state || !ReadSafe(t.state, CyberpunkVR_StageStreamStateDump[t.side], sizeof(CyberpunkVR_StageStreamStateDump[0])))
            std::memset(CyberpunkVR_StageStreamStateDump[t.side], 0, sizeof(CyberpunkVR_StageStreamStateDump[0]));
    }
    if (t.side >= 0 && t.ordinal < MaxOrdinals)
        g_done[t.rowIndex * MaxOrdinals + t.ordinal].store(uint64_t(t.frame) << 1 | 1, std::memory_order_release);
    LogEvent(t.frame, uint32_t(t.side), t.stage, t.ordinal, t.kind, t.start, end, t.spanBegin, t.spanEnd);
}

// VRCAM GBuffer_Solid partitions. The 15 bytes at 0x2277A46 (2.5 scene builder sub_142276E60; 2.31 0x1D43B2B)
//     cmp [rsi+0FA4h], ebx ; mov edi, cs:dword_143C14B50 ; cmovz edi, ebx
// jump to a check that keeps the worker count when the view's AA mode is 1 only if the graph is
// VRCAM's or the scope's. r12 is the builder's fourth argument for the whole function (set once at 0x2276E8D):
// its only caller (0x40A3E8) passes subview+0x25B0, and the subview's camera name is at +0x28
// (the hash FlagCompute and RenderParity key on), so the name is [r12-2588h]. ebx is 1 there.
// rax is free: the next instruction is `xor ecx, ecx; call`, which clobbers it, and resets flags.
// 2.31: viewData +F94, subview+0x17D0, [r12-17A8h].
bool InstallVrcamPartitions() {
    auto* site = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr)) + 0x2277A46;
    static const uint8_t expected[15] = {0x39, 0x9E, 0xA4, 0x0F, 0x00, 0x00, 0x8B, 0x3D,
                                         0xFE, 0xD0, 0x99, 0x01, 0x0F, 0x44, 0xFB};
    if (std::memcmp(site, expected, sizeof(expected)) != 0) {
        CyberpunkVR_VrcamGbufferPartitionsState.store(2);
        return false;
    }
    auto* code = static_cast<uint8_t*>(AllocateTrampoline(site, 128));
    if (!code) { CyberpunkVR_VrcamGbufferPartitionsState.store(2); return false; }
    const uintptr_t workers = uintptr_t(site + 12) + *reinterpret_cast<const int32_t*>(site + 8);
    int pos = 0;
    WriteMovRaxImm64(code, pos, workers);
    for (uint8_t b : {0x8B, 0x38}) code[pos++] = b;                                 // mov edi, [rax]
    for (uint8_t b : {0x39, 0x9E, 0xA4, 0x0F, 0x00, 0x00}) code[pos++] = b;         // cmp [rsi+0FA4h], ebx
    code[pos++] = 0x75; const int toDone1 = pos++;                                  // jnz done
    int toNext[2], toDone[2];
    const volatile uint64_t* keys[2] = {&CyberpunkVR_VrcamGbufferPartitionsKey, &CyberpunkVR_OpticsGbufferPartitionsKey};
    for (int k = 0; k < 2; ++k) {                                                   // per key:
        WriteMovRaxImm64(code, pos, reinterpret_cast<uintptr_t>(keys[k]));
        for (uint8_t b : {0x48, 0x8B, 0x00}) code[pos++] = b;                       //   mov rax, [rax]
        for (uint8_t b : {0x48, 0x85, 0xC0}) code[pos++] = b;                       //   test rax, rax
        code[pos++] = 0x74; toNext[k] = pos++;                                      //   jz next key
        for (uint8_t b : {0x49, 0x3B, 0x84, 0x24, 0x78, 0xDA, 0xFF, 0xFF}) code[pos++] = b;  // cmp rax, [r12-2588h]
        code[pos++] = 0x74; toDone[k] = pos++;                                      //   jz done
        code[toNext[k]] = uint8_t(pos - (toNext[k] + 1));
    }
    for (uint8_t b : {0x89, 0xDF}) code[pos++] = b;                                 // one: mov edi, ebx
    code[toDone1] = uint8_t(pos - (toDone1 + 1));
    for (int k = 0; k < 2; ++k) code[toDone[k]] = uint8_t(pos - (toDone[k] + 1));
    code[pos++] = 0xE9;                                                             // done: jmp past the site
    *reinterpret_cast<int32_t*>(code + pos) = static_cast<int32_t>((site + sizeof(expected)) - (code + pos + 4));
    pos += 4;
    DWORD old;
    VirtualProtect(site, sizeof(expected), PAGE_EXECUTE_READWRITE, &old);
    site[0] = 0xE9;
    *reinterpret_cast<int32_t*>(site + 1) = static_cast<int32_t>(code - (site + 5));
    for (size_t i = 5; i < sizeof(expected); ++i) site[i] = 0x90;
    VirtualProtect(site, sizeof(expected), old, &old);
    FlushInstructionCache(GetCurrentProcess(), site, sizeof(expected));
    return true;
}
CVR_HOOK("VrcamGbufferPartitions", ::cvr::hooks::Stage::Boot, 50, InstallVrcamPartitions);

// Publishes VRCAM's and the scope's camera names to the trampoline. VRCAM's can be re-resolved at
// runtime (vrcam pick), so it is re-read on every poll rather than latched.
void ApplyPartitions(uint32_t want) {
    static uint32_t applied = 0;
    if (CyberpunkVR_VrcamGbufferPartitionsState.load(std::memory_order_relaxed) == 2) return;
    static const uint64_t optics = cvr::cname_hash("vr_optics_feed");
    CyberpunkVR_OpticsGbufferPartitionsKey =
        CyberpunkVR_OpticsGbufferPartitions.load(std::memory_order_relaxed) ? optics : 0;
    const uint64_t key = want ? cvr::detail::g_vrcam_ctx_key.load(std::memory_order_acquire) : 0;
    if (key == CyberpunkVR_VrcamGbufferPartitionsKey) return;
    CyberpunkVR_VrcamGbufferPartitionsKey = key;
    const uint32_t state = key ? 1 : 0;
    CyberpunkVR_VrcamGbufferPartitionsState.store(state);
    if (state != applied)
        cvr::log("[stage-replay] VRCAM GBuffer_Solid partitions %s", state ? "follow the worker count (VRCAM only)" : "native");
    applied = state;
}

}  // namespace cvr::stereo::stage_replay

// The validated configuration (docs/single-pass-stage-replay-20261008.md): eleven RenderElements
// stages (3-7, 9, 20-23, 30), VRCAM as the source, VRCAM's G-buffer in worker-count partitions
// (now xr_vrcam_gbuffer_partitions, on by itself), barrier-only streams replayable; each passed a frozen-time check (time dilation ~0, run 845) in
// the city and in an interior. Left out, both visibly wrong in MAIN with every view descriptor
// substituted: stage 10 (deferred decals; their packets carry per-view instance data --
// startInstance differs in 55% of the draws, census 841) and stage 24 (distant city hazed over,
// hologram colours changed on the roof). 0 turns all of it off.
extern "C" __declspec(dllexport) void CyberpunkVR_StageReplayConfigure(int enable) {
    constexpr uint64_t kStages = 0x40F002F8ull;
    CyberpunkVR_StageReplaySource.store(1);
    CyberpunkVR_StageReplayStages.store(enable ? kStages : 0);
    CyberpunkVR_StageReplayAllowBarriers.store(enable ? 1u : 0u);
    CyberpunkVR_StageReplayMode.store(enable ? uint32_t(cvr::stereo::stage_replay::Mode::ReplayCamera) : 0u);
    cvr::log("[stage-replay] configured %s", enable ? "on (11 stages)" : "off");
}

// xr_vrcam_gbuffer_partitions: applied by the next StageReplayPoll (ApplyPartitions), with or
// without stage replay.
extern "C" __declspec(dllexport) void CyberpunkVR_VrcamGbufferPartitionsConfigure(int enable) {
    CyberpunkVR_VrcamGbufferPartitions.store(enable ? 1u : 0u);
}

// xr_scope_gbuffer_partitions: likewise, applied by the next StageReplayPoll.
extern "C" __declspec(dllexport) void CyberpunkVR_ScopeGbufferPartitionsConfigure(int enable) {
    CyberpunkVR_OpticsGbufferPartitions.store(enable ? 1u : 0u);
}

extern "C" __declspec(dllexport) void CyberpunkVR_StageReplayPoll() {
    using namespace cvr::stereo::stage_replay;
    Add(CyberpunkVR_StageCensusSummary[1], 1);
    ApplyPartitions(CyberpunkVR_VrcamGbufferPartitions.load(std::memory_order_relaxed));
    const uint32_t mode = CyberpunkVR_StageReplayMode.load(std::memory_order_relaxed);
    const uint32_t request = CyberpunkVR_StageCensusRequest.load(std::memory_order_relaxed);
    if (request && CyberpunkVR_StageCensusState.load(std::memory_order_relaxed) != 1) {
        // Start a window: clear rows, streams, events and take flags.
        g_census.store(false, std::memory_order_release);
        std::memset(CyberpunkVR_StageCensusRows, 0, sizeof(CyberpunkVR_StageCensusRows));
        std::memset(CyberpunkVR_StageCensusOps, 0, sizeof(CyberpunkVR_StageCensusOps));
        std::memset(CyberpunkVR_StageStreamInfo, 0, sizeof(CyberpunkVR_StageStreamInfo));
        CyberpunkVR_StageEventCount.store(0);
        for (auto& f : g_streamTaken) f.store(0);
        g_censusFrames.store(request);
        CyberpunkVR_StageCensusRequest.store(0);
        CyberpunkVR_StageCensusState.store(1);
        CyberpunkVR_StageCensusSummary[0] = 0;
        g_census.store(true, std::memory_order_release);
    } else if (g_census.load(std::memory_order_relaxed)) {
        CyberpunkVR_StageCensusSummary[0] += 1;
        const int64_t now = Now();
        LogEvent(EngineFrame(), 0xFF, 0xFF, 0, KindPresent, now, now, 0, 0);
        if (g_censusFrames.fetch_sub(1) <= 1) { g_census.store(false, std::memory_order_release); CyberpunkVR_StageCensusState.store(2); }
    }
    const bool wanted = mode != 0 || g_census.load(std::memory_order_relaxed);
    const bool tracking = g_tracking.load(std::memory_order_relaxed);
    if (wanted) {
        if (!tracking) g_generation.fetch_add(1, std::memory_order_acq_rel);   // mirrors from before are not trusted
        InstallHooks();   // idempotent; picks up vtables created later
    }
    // Census needs the bound shader-visible heap (from the mirror) to place descriptor copies.
    g_tracking.store(wanted, std::memory_order_release);
    g_active.store(wanted, std::memory_order_release);
    if (!wanted && AnyInstalled()) UninstallHooks();
    CyberpunkVR_StageCensusSummary[6] = EngineFrame();
    g_presents.fetch_add(1, std::memory_order_relaxed);
}
