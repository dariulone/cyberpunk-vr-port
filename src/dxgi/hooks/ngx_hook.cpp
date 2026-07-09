// NGX DLSS EvaluateFeature read-only hook.
//
// Patches nvngx_dlss.dll!NVSDK_NGX_D3D12_EvaluateFeature with a JMP that
// captures the engine's per-frame motion-vector / depth resources and the
// scale / reset NGX parameters. The original function is then re-entered via
// a small trampoline (saved displaced bytes + JMP back). No NGX state is
// modified — DLSS sees the same call it always saw.
//
// The NGX parameter struct exposes a stable vtable on its v-table-0 slot, but
// nvngx_dlss.dll's INTERNAL parameter type is opaque. Fortunately the DLSS
// SDK is open: NVSDK_NGX_Parameter is a thin wrapper around setters/getters
// that the game side already populates. We do NOT call NGX SDK API here —
// instead we walk a small slot of well-known string→pointer entries that the
// in-memory parameter object carries. We confirm fields by their KNOWN value
// patterns (resource pointer with COM vtable, float scale, int reset).
//
// This file deliberately avoids depending on the NGX SDK headers — the only
// constant we need is the function signature shape:
//
//   NVSDK_NGX_Result NVSDK_NGX_D3D12_EvaluateFeature(
//       ID3D12GraphicsCommandList* cmdList,
//       const NVSDK_NGX_Handle* featureHandle,
//       const NVSDK_NGX_Parameter* params,
//       PFN_NVSDK_NGX_ProgressCallback progress)
//
// rcx=cmdList, rdx=featureHandle, r8=params, r9=progress.

#include "ngx_hook.h"

#include <cstdint>
#include <cstring>
#include <mutex>
#include <atomic>

extern void Log(const char* fmt, ...);
extern volatile int g_verboseLog;

namespace {

std::atomic<uint8_t*> g_trampolineEntry{nullptr};

// Captured engine resources, refcounted (AddRef'd into the slots, Release'd
// when overwritten). Mutex protects the lifetime swap; the reads use AddRef
// under lock so the consumer can Release outside our scope safely.
std::mutex g_captureMutex;
ID3D12Resource* g_mvRes = nullptr;
ID3D12Resource* g_depthRes = nullptr;
std::atomic<float> g_mvScaleX{1.0f};
std::atomic<float> g_mvScaleY{1.0f};
std::atomic<int> g_resetFlag{0};
std::atomic<unsigned int> g_mvWidth{0};
std::atomic<unsigned int> g_mvHeight{0};
std::atomic<unsigned int> g_mvFormat{0};
std::atomic<unsigned int> g_evalCount{0};

void SetCapturedResource(ID3D12Resource*& slot, ID3D12Resource* newRes) {
    if (slot == newRes) return;
    if (newRes) newRes->AddRef();
    ID3D12Resource* old = slot;
    slot = newRes;
    if (old) old->Release();
}

bool IsLikelyComObject(const void* p) {
    if (!p) return false;
    if ((reinterpret_cast<uintptr_t>(p) & 0x7) != 0) return false;
    MEMORY_BASIC_INFORMATION mbi{};
    if (VirtualQuery(p, &mbi, sizeof(mbi)) == 0) return false;
    if (mbi.State != MEM_COMMIT) return false;
    if ((mbi.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY |
                       PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE)) == 0) return false;
    // vtable pointer at offset 0 should point into a module's code/rdata
    void* vtable = *reinterpret_cast<void* const*>(p);
    if (!vtable) return false;
    MEMORY_BASIC_INFORMATION vmbi{};
    if (VirtualQuery(vtable, &vmbi, sizeof(vmbi)) == 0) return false;
    if (vmbi.Type != MEM_IMAGE) return false;
    return true;
}

// The NGX D3D12 parameter object stores ID3D12Resource* values; once
// captured, the resources will fail an IsLikelyComObject check if the game
// frees them between frames. So we always AddRef under lock — the consumer
// only ever sees a still-valid AddRef'd pointer or null.
//
// Heuristic field discovery: we sweep a bounded window of the params struct
// looking for COM vtable pointers that are exactly TWO distinct
// ID3D12Resource pointers (MV is typically R16G16_FLOAT 16bpp ~ 1280×720;
// depth is R32_TYPELESS 32bpp at full render res). For a first iteration we
// log every COM-pointer field at runtime — the user's log will then tell us
// exact offsets. We never deref past the validated pointer.
// The original NGX-Parameter heuristic sweep is retained ONLY for future
// fallback work; CP2077 uses Streamline so the slSetTag hook above is the active
// capture path. If a future game hits NGX directly we'd resurrect this.
#if 0
void CaptureParametersFromOpaqueStruct(const void* params) {
    if (!params) return;

    constexpr size_t kSweepBytes = 0x800;
    constexpr size_t kFieldStride = sizeof(void*);
    const uint8_t* base = reinterpret_cast<const uint8_t*>(params);

    MEMORY_BASIC_INFORMATION mbi{};
    if (VirtualQuery(base, &mbi, sizeof(mbi)) == 0) return;
    const uint8_t* regionEnd = static_cast<const uint8_t*>(mbi.BaseAddress) +
                                static_cast<size_t>(mbi.RegionSize);
    const size_t safeBytes = (base < regionEnd)
        ? static_cast<size_t>(regionEnd - base)
        : 0;
    const size_t sweep = (safeBytes < kSweepBytes) ? safeBytes : kSweepBytes;

    ID3D12Resource* candidateMv = nullptr;
    ID3D12Resource* candidateDepth = nullptr;
    unsigned int mvW = 0, mvH = 0, mvFmt = 0;
    unsigned int dW = 0, dH = 0;

    // Scan candidates: pointer-aligned slots whose value passes
    // IsLikelyComObject. Then probe with GetDesc — if it returns a TEXTURE2D
    // it's a real D3D12 resource. Smallest texture = motion vectors,
    // largest = scene color/depth (depending on flags).
    struct Candidate { ID3D12Resource* res; D3D12_RESOURCE_DESC desc; size_t offset; };
    Candidate cands[16] = {};
    int nCands = 0;
    for (size_t off = 0; off + kFieldStride <= sweep && nCands < 16; off += kFieldStride) {
        void* slot = *reinterpret_cast<void* const*>(base + off);
        if (!IsLikelyComObject(slot)) continue;
        ID3D12Resource* res = reinterpret_cast<ID3D12Resource*>(slot);
        // Probe via QueryInterface — if it's not really ID3D12Resource we
        // get HR != S_OK and we move on.
        ID3D12Resource* probed = nullptr;
        if (res->QueryInterface(IID_PPV_ARGS(&probed)) != S_OK || !probed) continue;
        D3D12_RESOURCE_DESC desc = probed->GetDesc();
        probed->Release();
        if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D) continue;
        cands[nCands++] = {res, desc, off};
    }

    if (nCands == 0) return;

    // Identify MV vs Depth among candidates.
    // - Depth: D-family format OR R32_TYPELESS/R32_FLOAT/R32G8X24_TYPELESS at
    //   full render resolution.
    // - Motion vectors: 2-channel float (R16G16_FLOAT=34, R32G32_FLOAT=16) at
    //   pre-DLSS resolution (smaller than depth).
    for (int i = 0; i < nCands; ++i) {
        const Candidate& c = cands[i];
        const DXGI_FORMAT f = c.desc.Format;
        const bool isDepthFmt =
            f == DXGI_FORMAT_R32_TYPELESS || f == DXGI_FORMAT_D32_FLOAT ||
            f == DXGI_FORMAT_R32_FLOAT || f == DXGI_FORMAT_R32G8X24_TYPELESS ||
            f == DXGI_FORMAT_D32_FLOAT_S8X24_UINT ||
            f == DXGI_FORMAT_R24G8_TYPELESS || f == DXGI_FORMAT_D24_UNORM_S8_UINT;
        const bool isMvFmt =
            f == DXGI_FORMAT_R16G16_FLOAT || f == DXGI_FORMAT_R32G32_FLOAT ||
            f == DXGI_FORMAT_R16G16_SINT || f == DXGI_FORMAT_R32G32_SINT;
        if (isDepthFmt && !candidateDepth) {
            candidateDepth = c.res;
            dW = static_cast<unsigned int>(c.desc.Width);
            dH = c.desc.Height;
        } else if (isMvFmt && !candidateMv) {
            candidateMv = c.res;
            mvW = static_cast<unsigned int>(c.desc.Width);
            mvH = c.desc.Height;
            mvFmt = static_cast<unsigned int>(c.desc.Format);
        }
    }

    // Float / int sweep for MV scale + reset flag. Heuristic: NGX MV scale
    // is typically a non-1 finite float in [-32, 32] (engines pass
    // renderRes/displayRes ratio multiplied or sign for clip-space
    // direction). Reset is a 0/1 int. We log a fingerprint so we can
    // pinpoint exact offsets once we see it in the log.
    float mvSx = 1.0f, mvSy = 1.0f;
    int rst = 0;
    bool gotMvSx = false, gotMvSy = false;

    for (size_t off = 0; off + 4 <= sweep; off += 4) {
        const float fv = *reinterpret_cast<const float*>(base + off);
        const uint32_t iv = *reinterpret_cast<const uint32_t*>(base + off);
        if (!gotMvSx) {
            if (fv > -32.0f && fv < 32.0f && fv != 0.0f && fv != 1.0f &&
                _finite(fv)) {
                mvSx = fv;
                gotMvSx = true;
                continue;
            }
        } else if (!gotMvSy) {
            if (fv > -32.0f && fv < 32.0f && fv != 0.0f && fv != 1.0f &&
                _finite(fv)) {
                mvSy = fv;
                gotMvSy = true;
                continue;
            }
        }
        if (iv == 0u || iv == 1u) {
            rst |= static_cast<int>(iv);
        }
    }

    {
        std::lock_guard<std::mutex> lock(g_captureMutex);
        if (candidateMv) SetCapturedResource(g_mvRes, candidateMv);
        if (candidateDepth) SetCapturedResource(g_depthRes, candidateDepth);
    }
    if (candidateMv) {
        g_mvWidth.store(mvW, std::memory_order_relaxed);
        g_mvHeight.store(mvH, std::memory_order_relaxed);
        g_mvFormat.store(mvFmt, std::memory_order_relaxed);
    }
    g_mvScaleX.store(mvSx, std::memory_order_relaxed);
    g_mvScaleY.store(mvSy, std::memory_order_relaxed);
    g_resetFlag.store(rst, std::memory_order_relaxed);

    const unsigned int n = g_evalCount.fetch_add(1, std::memory_order_relaxed) + 1;
    if (g_verboseLog && (n == 1 || (n % 600) == 0)) {
        Log("[NGX] eval#%u mv=%p %ux%u fmt=%u depth=%p %ux%u sx=%f sy=%f reset=%d nCands=%d\n",
            n, candidateMv, mvW, mvH, mvFmt, candidateDepth, dW, dH, mvSx, mvSy, rst, nCands);
    }
}
#endif // 0 — legacy NGX direct heuristic (CP2077 uses Streamline; see slSetTag hook above)

bool WriteRel32Jmp(uint8_t* dst, void* target) {
    intptr_t rel = reinterpret_cast<intptr_t>(target) -
                   (reinterpret_cast<intptr_t>(dst) + 5);
    if (rel < INT32_MIN || rel > INT32_MAX) return false;
    DWORD oldProtect = 0;
    if (!VirtualProtect(dst, 5, PAGE_EXECUTE_READWRITE, &oldProtect)) return false;
    dst[0] = 0xE9;
    *reinterpret_cast<int32_t*>(dst + 1) = static_cast<int32_t>(rel);
    DWORD discard = 0;
    VirtualProtect(dst, 5, oldProtect, &discard);
    FlushInstructionCache(GetCurrentProcess(), dst, 5);
    return true;
}

// Allocate an executable trampoline within +/-2GB of `anchor` so an E9 from it
// can reach our hook target and the original code can JMP back to it.
// (`near` would have been clearer but it's a Windows historic macro.)
uint8_t* AllocateExecNearby(void* anchor, size_t bytes) {
    SYSTEM_INFO si{};
    GetSystemInfo(&si);
    const uintptr_t step = si.dwAllocationGranularity ? si.dwAllocationGranularity : 0x10000;
    const uintptr_t target = reinterpret_cast<uintptr_t>(anchor);
    for (intptr_t delta = -static_cast<intptr_t>(0x40000000);
         delta <= static_cast<intptr_t>(0x40000000); delta += step) {
        uintptr_t addr = target + delta;
        addr = addr & ~(step - 1);
        void* mem = VirtualAlloc(reinterpret_cast<void*>(addr), bytes,
            MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE);
        if (mem) return reinterpret_cast<uint8_t*>(mem);
    }
    return nullptr;
}

}  // namespace

// Hook on sl.interposer.dll!slSetTag — Streamline tag-registration API. CP2077
// uses Streamline (sl.interposer.dll) which proxies DLSS, so the game NEVER
// calls NVSDK_NGX_D3D12_EvaluateFeature directly (confirmed by 0 evaluations
// observed during a full play session despite DLSS being active). Streamline
// tagging is where the game explicitly labels each resource by purpose
// (BufferType: MotionVectors / Depth / ScalingInputColor / etc.) — exactly
// what we need, with NO heuristic guessing.
//
// Signature (from Streamline SDK):
//   sl::Result slSetTag(const ViewportHandle& vp,
//                       const ResourceTag* tags,
//                       uint32_t numTags,
//                       CommandBuffer* cmdBuf);
//
// ResourceTag layout (sl::ResourceTag, ~64 bytes):
//   +0x00: Resource* resource          (ptr to sl::Resource struct)
//   +0x08: BufferType type             (uint32 — see kBufferType* enum below)
//   +0x0C: ResourceLifecycle lifecycle (uint32)
//   +0x10..+0x18: Extent extent        (top/left/width/height u32)
//
// sl::Resource layout:
//   +0x00: ResourceType type           (uint32, 1 = Tex2d)
//   +0x08: void* native                (ID3D12Resource* for D3D12)
//   +0x10..: state, view, alloc bits   (we ignore)
//
// Stable across Streamline 2.x; if upstream rearranges we'll see garbage in
// the dump log and adjust.
enum SlBufferType : uint32_t {
    kBufferTypeDepth = 0,
    kBufferTypeMotionVectors = 1,
    kBufferTypeHudLessColor = 2,
    kBufferTypeUiColor = 3,
    kBufferTypeRawColor = 4,
    kBufferTypeScalingInputColor = 5,
    kBufferTypeScalingOutputColor = 6,
};

using SlSetTagFn = uint32_t (*)(const void* /*vp*/, const void* /*tags*/, uint32_t /*numTags*/, void* /*cmdBuf*/);
SlSetTagFn g_origSlSetTag = nullptr;
std::atomic<bool> g_setTagHookInstalled{false};
std::atomic<uint64_t> g_setTagCalls{0};
std::atomic<uint64_t> g_setTagInvalid{0};

// slEvaluateFeature: every DLSS evaluation goes through here. Used as a
// confirmation hook (does Streamline get any calls at all?) and as a probe
// for the cmdBuffer argument which carries the live D3D12 command list.
// Signature (Streamline 2.x):
//   sl::Result slEvaluateFeature(sl::Feature feature,
//                                const sl::FrameToken& frame,
//                                const sl::ViewportHandle& viewport,
//                                sl::CommandBuffer* cmdBuffer);
// Where Feature is a u32 (DLSS=0, DLSSG=1, REFLEX=2, NIS=3, etc).
using SlEvaluateFeatureFn = uint32_t (*)(uint32_t /*feature*/, const void* /*frameToken*/, const void* /*viewportHandle*/, void* /*cmdBuffer*/);
SlEvaluateFeatureFn g_origSlEvaluateFeature = nullptr;
std::atomic<bool> g_evaluateFeatureHookInstalled{false};
std::atomic<uint64_t> g_evaluateFeatureCalls{0};

uint32_t HookedSlEvaluateFeature(uint32_t feature, const void* frameToken, const void* viewportHandle, void* cmdBuffer) {
    const uint64_t n = g_evaluateFeatureCalls.fetch_add(1, std::memory_order_relaxed);
    if (g_verboseLog && (n < 3 || (n % 600) == 0)) {
        Log("[NGX] slEvaluateFeature invoked #%llu feature=%u frameToken=%p viewport=%p cmdBuf=%p\n",
            static_cast<unsigned long long>(n), feature, frameToken, viewportHandle, cmdBuffer);
    }
    if (g_origSlEvaluateFeature) {
        return g_origSlEvaluateFeature(feature, frameToken, viewportHandle, cmdBuffer);
    }
    return 1;
}

// Direct hook on _nvngx.dll!NVSDK_NGX_D3D12_EvaluateFeature — the
// internal NGX dispatcher. CP2077 may bypass Streamline entirely for DLSS
// and call _nvngx directly. If THIS hook also never fires, DLSS is not
// being evaluated at all in the VR config.
// Signature:
//   NVSDK_NGX_Result NVSDK_NGX_D3D12_EvaluateFeature(
//       ID3D12GraphicsCommandList* InCmdList,
//       const NVSDK_NGX_Handle* InFeatureHandle,
//       const NVSDK_NGX_Parameter* InParameters,
//       PFN_NVSDK_NGX_ProgressCallback InCallback);
using NgxD3D12EvalFn = uint32_t (*)(void*, const void*, const void*, void*);
NgxD3D12EvalFn g_origNgxEvalDirect = nullptr;
std::atomic<bool> g_ngxEvalDirectInstalled{false};
std::atomic<uint64_t> g_ngxEvalDirectCalls{0};

uint32_t HookedNgxD3D12Eval(void* cmdList, const void* handle, const void* params, void* callback) {
    const uint64_t n = g_ngxEvalDirectCalls.fetch_add(1, std::memory_order_relaxed);
    if (g_verboseLog && (n < 3 || (n % 600) == 0)) {
        Log("[NGX] _nvngx NVSDK_NGX_D3D12_EvaluateFeature invoked #%llu cmdList=%p handle=%p params=%p\n",
            static_cast<unsigned long long>(n), cmdList, handle, params);
    }
    if (g_origNgxEvalDirect) {
        return g_origNgxEvalDirect(cmdList, handle, params, callback);
    }
    return 1;
}

// LIVE-DISCOVERED layout (CP2077 + Streamline 2.x):
//   sl::ResourceTag (64 bytes, stack-allocated):
//     +0x00: NULL
//     +0x08..+0x18: GUID 4C6A5AAD-B445-496C-87FF-1AF3845BE653 (= sl::Resource type id)
//     +0x18: u32 = 1 (ResourceType::Tex2d marker)
//     +0x20: sl::Resource* (pointer to STACK-allocated Resource struct)
//     +0x28: u32 BufferType (the value we care about for MV/Depth/Color discrimination)
//     +0x2C: u32 ResourceLifecycle
//     +0x30..0x38: padding/reserved
//     +0x38: u32 extent.top
//     +0x3C: u32 extent.left
//     (extent.width/height live further but we don't need them — D3D12 has GetDesc)
//
//   sl::Resource (referenced via tag+0x20):
//     +0x00: u32 type (1=Tex2d)
//     +0x08: void* native (ID3D12Resource*)
//     +0x10+: state, view, allocator bits (we ignore)
bool ReadTagFieldsSeh(const uint8_t* tagBytes, const void*& resourceStructPtr, uint32_t& bufType) {
    __try {
        resourceStructPtr = *reinterpret_cast<const void* const*>(tagBytes + 0x20);
        bufType           = *reinterpret_cast<const uint32_t*>(tagBytes + 0x28);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}
bool ReadResourceNativeSeh(const void* resStructPtr, const void*& outNative) {
    // LIVE-DISCOVERED from CP2077 dump:
    //   sl::Resource layout (128 bytes, stack-allocated by game caller):
    //     +0x00: NULL/padding
    //     +0x08, +0x10: GUID 4B7224183A9D70CF-61721C72F8139183 (= sl::Resource type id)
    //     +0x18: u32 = 1 (sl::ResourceType::eTex2d)
    //     +0x20: chain pointer (void* next, often NULL/stack-like)
    //     +0x28: ★ void* native (ID3D12Resource* for D3D12)
    //     +0x30, +0x38: state/view (usually NULL on tagging)
    //     +0x60..+0x78: dimensions (width/height/mip/array)
    // Three distinct native pointers observed for three distinct buffer types
    // confirms this is the per-resource discriminator.
    __try {
        outNative = *reinterpret_cast<const void* const*>(
            reinterpret_cast<const uint8_t*>(resStructPtr) + 0x28);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// Raw byte dump was used to derive the exact ResourceTag + Resource layout
// from live game data. Now that the offsets are locked in (tag+0x20 → resource
// struct, +0x28 inside = native ID3D12Resource*, tag+0x28 = bufType u32),
// disable the dump entirely. Bump back to a small N to re-investigate if the
// Streamline version updates.
std::atomic<int> g_tagDumpsRemaining{0};
void DumpRawTagBytes(const uint8_t* tagBytes) {
    if (!g_verboseLog) return;
    int remaining = g_tagDumpsRemaining.fetch_sub(1, std::memory_order_relaxed);
    if (remaining <= 0) return;
    char hex[300] = {};
    int pos = 0;
    for (int i = 0; i < 64 && pos < static_cast<int>(sizeof(hex)) - 4; ++i) {
        __try {
            uint8_t b = tagBytes[i];
            pos += sprintf_s(hex + pos, sizeof(hex) - pos, "%02X ", static_cast<unsigned>(b));
        } __except (EXCEPTION_EXECUTE_HANDLER) {
            break;
        }
    }
    Log("[NGX-DUMP] tag raw 64B: %s\n", hex);
    // Also dump as qwords for easier pointer inspection.
    for (int q = 0; q < 8; ++q) {
        uint64_t val = 0;
        __try { val = *reinterpret_cast<const uint64_t*>(tagBytes + q * 8); }
        __except (EXCEPTION_EXECUTE_HANDLER) { break; }
        Log("[NGX-DUMP]   +0x%02x = 0x%016llx\n", q * 8,
            static_cast<unsigned long long>(val));
    }
    // ALSO dump the sl::Resource struct referenced by tag+0x20 — the native
    // ID3D12Resource* lives inside it but the offset is unclear (our +0x08
    // guess returned garbage 0x4B7224183A9D70CF on the live game). Walk first
    // 16 qwords (128 bytes) so we can see the layout.
    uint64_t resStructAddr = 0;
    __try { resStructAddr = *reinterpret_cast<const uint64_t*>(tagBytes + 0x20); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    if (!resStructAddr) return;
    const uint8_t* res = reinterpret_cast<const uint8_t*>(resStructAddr);
    Log("[NGX-DUMP] sl::Resource @0x%016llx:\n",
        static_cast<unsigned long long>(resStructAddr));
    for (int q = 0; q < 16; ++q) {
        uint64_t val = 0;
        __try { val = *reinterpret_cast<const uint64_t*>(res + q * 8); }
        __except (EXCEPTION_EXECUTE_HANDLER) { return; }
        const char* tag = "";
        if (val != 0 && IsLikelyComObject(reinterpret_cast<void*>(val))) {
            tag = " <COM-candidate>";
        }
        Log("[NGX-DUMP]   res+0x%02x = 0x%016llx%s\n", q * 8,
            static_cast<unsigned long long>(val), tag);
    }
}

void ProcessTag(const uint8_t* tagBytes) {
    DumpRawTagBytes(tagBytes);
    const void* resourceStructPtr = nullptr;
    uint32_t bufType = 0xFFFFFFFFu;
    if (!ReadTagFieldsSeh(tagBytes, resourceStructPtr, bufType)) {
        g_setTagInvalid.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    if (!resourceStructPtr) return;

    // Dereference sl::Resource.native (+0x08) to get the actual ID3D12Resource*.
    const void* nativePtr = nullptr;
    if (!ReadResourceNativeSeh(resourceStructPtr, nativePtr)) {
        g_setTagInvalid.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    if (!IsLikelyComObject(nativePtr)) {
        // First few invocations the resource may not yet be valid; log once.
        const uint64_t n = g_setTagCalls.load(std::memory_order_relaxed);
        if (g_verboseLog && n < 8) {
            Log("[NGX] setTag #%llu type=%u resourceStruct=%p native=%p NOT_COM\n",
                static_cast<unsigned long long>(n), bufType, resourceStructPtr, nativePtr);
        }
        g_setTagInvalid.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    ID3D12Resource* d3dRes = reinterpret_cast<ID3D12Resource*>(const_cast<void*>(nativePtr));
    // Verify via QueryInterface.
    ID3D12Resource* probed = nullptr;
    if (d3dRes->QueryInterface(IID_PPV_ARGS(&probed)) != S_OK || !probed) {
        g_setTagInvalid.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    D3D12_RESOURCE_DESC desc = probed->GetDesc();
    probed->Release();

    if (bufType == kBufferTypeMotionVectors) {
        std::lock_guard<std::mutex> lock(g_captureMutex);
        SetCapturedResource(g_mvRes, d3dRes);
        g_mvWidth.store(static_cast<unsigned int>(desc.Width), std::memory_order_relaxed);
        g_mvHeight.store(desc.Height, std::memory_order_relaxed);
        g_mvFormat.store(static_cast<unsigned int>(desc.Format), std::memory_order_relaxed);
    } else if (bufType == kBufferTypeDepth) {
        std::lock_guard<std::mutex> lock(g_captureMutex);
        SetCapturedResource(g_depthRes, d3dRes);
    }

    // Log first ~12 tagged resources so we map every bufType value the engine
    // uses to a real D3D12 resource (width/height/format). After that, only
    // milestone logs to keep the file lean.
    const uint64_t n = g_setTagCalls.load(std::memory_order_relaxed);
    if (g_verboseLog && (n < 12 || (n % 1800) == 0)) {
        Log("[NGX] setTag #%llu bufType=%u native=%p (D3D12 %llux%u fmt=%u dim=%u)\n",
            static_cast<unsigned long long>(n),
            bufType,
            d3dRes,
            static_cast<unsigned long long>(desc.Width),
            desc.Height,
            static_cast<unsigned>(desc.Format),
            static_cast<unsigned>(desc.Dimension));
    }
}

uint32_t HookedSlSetTag(const void* vp, const void* tags, uint32_t numTags, void* cmdBuf) {
    const uint64_t n = g_setTagCalls.fetch_add(1, std::memory_order_relaxed);
    // Always log first 3 calls so we can confirm the hook IS being invoked
    // even with weird arguments. After that log every 1800 calls if any.
    if (g_verboseLog && (n < 3 || (n % 1800) == 0)) {
        Log("[NGX] slSetTag invoked #%llu vp=%p tags=%p numTags=%u cmdBuf=%p\n",
            static_cast<unsigned long long>(n), vp, tags, numTags, cmdBuf);
    }
    if (tags && numTags > 0 && numTags < 64) {
        constexpr size_t kTagStride = 64; // sl::ResourceTag size
        const uint8_t* base = reinterpret_cast<const uint8_t*>(tags);
        for (uint32_t i = 0; i < numTags; ++i) {
            ProcessTag(base + i * kTagStride);
        }
    }
    if (g_origSlSetTag) {
        return g_origSlSetTag(vp, tags, numTags, cmdBuf);
    }
    return 1; // SL_RESULT_FAIL
}

// Install a 5-byte E9 inline hook on `targetFn` that JMPs into `hookFn`.
// On success, *outOriginal becomes a function pointer that can be called to
// re-enter the original function (16-byte preserved prologue + JMP back). The
// preserved-prologue scheme assumes the first 16 bytes contain only safe-to-
// relocate insns (typical for MSVC-built export entry points). Returns the
// patched bytes pointer for diagnostics, or nullptr on failure.
template <typename TFnPtr>
uint8_t* InstallE9HookAt(uint8_t* target, void* hookFn, TFnPtr* outOriginal) {
    constexpr size_t kProloguePreserve = 16;
    uint8_t* tramp = AllocateExecNearby(target, kProloguePreserve + 5);
    if (!tramp) return nullptr;

    DWORD oldProtect = 0;
    if (!VirtualProtect(target, kProloguePreserve + 5, PAGE_EXECUTE_READWRITE, &oldProtect)) {
        VirtualFree(tramp, 0, MEM_RELEASE);
        return nullptr;
    }
    memcpy(tramp, target, kProloguePreserve);
    intptr_t backRel = reinterpret_cast<intptr_t>(target + kProloguePreserve) -
                       reinterpret_cast<intptr_t>(tramp + kProloguePreserve + 5);
    if (backRel < INT32_MIN || backRel > INT32_MAX) {
        VirtualProtect(target, kProloguePreserve + 5, oldProtect, &oldProtect);
        VirtualFree(tramp, 0, MEM_RELEASE);
        return nullptr;
    }
    tramp[kProloguePreserve + 0] = 0xE9;
    *reinterpret_cast<int32_t*>(tramp + kProloguePreserve + 1) = static_cast<int32_t>(backRel);
    *outOriginal = reinterpret_cast<TFnPtr>(tramp);
    const bool ok = WriteRel32Jmp(target, hookFn);
    VirtualProtect(target, kProloguePreserve + 5, oldProtect, &oldProtect);
    if (!ok) {
        *outOriginal = nullptr;
        VirtualFree(tramp, 0, MEM_RELEASE);
        return nullptr;
    }
    return tramp;
}

bool NgxInstallEvaluateFeatureHook() {
    if (g_setTagHookInstalled.load(std::memory_order_acquire) &&
        g_evaluateFeatureHookInstalled.load(std::memory_order_acquire)) {
        return true;
    }
    HMODULE dll = GetModuleHandleA("sl.interposer.dll");
    if (!dll) return false;

    // Hook slSetTag if not already installed.
    if (!g_setTagHookInstalled.load(std::memory_order_acquire)) {
        auto target = reinterpret_cast<uint8_t*>(GetProcAddress(dll, "slSetTag"));
        if (target) {
            uint8_t* tramp = InstallE9HookAt(target,
                reinterpret_cast<void*>(&HookedSlSetTag), &g_origSlSetTag);
            if (tramp) {
                g_setTagHookInstalled.store(true, std::memory_order_release);
                g_trampolineEntry.store(tramp, std::memory_order_release);
                if (g_verboseLog) {
                    Log("[NGX] sl.interposer.dll!slSetTag hook installed. target=%p tramp=%p\n",
                        target, tramp);
                }
            }
        }
    }

    // ⚠ NOTE: slEvaluateFeature + _nvngx.dll!NVSDK_NGX_D3D12_EvaluateFeature
    // hooks DISABLED. The 16-byte prologue-preserve trampoline crashed both
    // (DEP violation at 0x0, captured on the first hot call). Their prologues
    // contain a RIP-relative or other non-relocatable insn within the first
    // 16 bytes, so the displaced copy executes garbage when our HookedSl...
    // returns through `g_orig...(...)`. slSetTag works fine because its
    // prologue is plain `mov [rsp+...], rcx` etc.
    // To enable these later we need either a length-decoder (Zydis / udis86)
    // or a known prologue pattern matcher. Capture-only path through slSetTag
    // is sufficient — that's where the game tags MV / Depth / Color resources
    // by purpose, exactly what we need for forward extrapolation.

    return g_setTagHookInstalled.load(std::memory_order_acquire);
}

ID3D12Resource* NgxAcquireMotionVectors() {
    std::lock_guard<std::mutex> lock(g_captureMutex);
    if (g_mvRes) g_mvRes->AddRef();
    return g_mvRes;
}

ID3D12Resource* NgxAcquireDepth() {
    std::lock_guard<std::mutex> lock(g_captureMutex);
    if (g_depthRes) g_depthRes->AddRef();
    return g_depthRes;
}

float NgxGetMvScaleX() { return g_mvScaleX.load(std::memory_order_relaxed); }
float NgxGetMvScaleY() { return g_mvScaleY.load(std::memory_order_relaxed); }
int   NgxGetResetFlag() { return g_resetFlag.load(std::memory_order_relaxed); }
unsigned int NgxGetMvWidth() { return g_mvWidth.load(std::memory_order_relaxed); }
unsigned int NgxGetMvHeight() { return g_mvHeight.load(std::memory_order_relaxed); }
unsigned int NgxGetMvFormat() { return g_mvFormat.load(std::memory_order_relaxed); }
unsigned int NgxGetEvalCount() { return g_evalCount.load(std::memory_order_relaxed); }
