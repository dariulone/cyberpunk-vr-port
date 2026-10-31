// EngineStereoDlss -- under render feature 98 the engine's one DLSS evaluate over the side-by-side frame becomes one
// evaluate per eye (include/Stereo/EngineStereoDlss.hpp).
//
// What the engine hands Streamline for its viewport each frame -- the tags, the constants, the options -- is
// recorded as it goes by. At its evaluate each eye gets:
//
//   * a viewport of its own (kEyeViewport): Streamline keeps one DLSS feature, and so one history, per viewport;
//   * the engine's tags with their extents cut to the eye's half: a tag the size of the scaling input (colour,
//     depth, motion, masks) takes the input's half, one the size of the output takes the output's half, anything
//     else (the 1x1 exposure) stays as it is. A tag without an extent is the whole resource. The right eye
//     reads copies of its halves at (0, 0) and writes a target of its own, copied into its half (DLSS writes
//     nothing at an output offset and its right eye swam with its inputs read at one);
//   * the engine's options with half the output width (its own, recorded from slDLSSSetOptions; derived from the
//     scale when the hook went in after the engine set them);
//   * the engine's constants with the motion scale's x doubled. Streamline turns the motion into pixels with the
//     scale times the extent's width, and the engine's scale is right for the two-eyes-wide extent, so a half-wide
//     one needs twice the scale for the same pixels;
//   * the jitter in the eye's own pixels. The engine's jitter is in pixels of the size each eye camera turns it
//     into NDC with (2 j / size, sub_140C28EC0) -- the view's (EngineStereo EyeJitterSize) -- and an eye's NDC
//     spans its own half, so the eye's samples moved by j * eye width / that size.
//
// The engine's own viewport is never evaluated in this mode. Anything that does not fit (another feature, no
// tags or constants for the evaluate's viewport and frame, an odd width) runs the engine's evaluate unchanged,
// counted in CyberpunkVR_DebugEngineStereoDlss.
#include "Stereo/EngineStereoDlss.hpp"
#include "Render/CommandResources.hpp"
#include "Render/IdleDlssContext.hpp"
#include "Stereo/EngineStereo.hpp"
#include <windows.h>
#include <d3d12.h>
#include <MinHook.h>
#include <atomic>
#include <cmath>
#include <cstring>
#include <mutex>
#ifndef SL_FEATURE_FUN_IMPORT_STATIC
#define SL_FEATURE_FUN_IMPORT_STATIC(feature, name) static PFun_##name* s_##name = nullptr   // sl.h's; helpers unused
#endif
#include <sl_dlss.h>

extern void Log(const char*,...);
// 1 = DLSS per eye under engine stereo (default); 0 = the engine's one evaluate over both eyes. Live.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EngineStereoDlssPerEye{1};
// [0] evaluates split per eye, [1] passed through (another feature / mode off), [2] no tags for the viewport,
// [3] no constants for the viewport and frame, [4] tags without a colour input and output of even width,
// [5] eye options set, [6] an eye's evaluate failed, [7] the engine's options calls seen.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEngineStereoDlss[8] = {};
// Diagnostics, live: 1 = the right eye evaluated first, 2 = the right eye's output put into the left half,
// 4 = the left eye only, 8 = the eye's tags also passed with its evaluate (local tags), 16 = the right eye
// written straight into its half (no target of its own: DLSS leaves the half black), 32 = the right eye's
// inputs read in their halves at an offset (no copies).
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EngineStereoDlssMode{0};
// The last split: per eye [options result, tag result, constants result, evaluate result] (eye 0 at 0..3).
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DebugEngineStereoDlssResults[8] = {};
// 1 = each eye's DLSS gets the jitter in its own pixels (default); 0 = the engine's jitter as it is. Live.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EngineStereoDlssEyeJitter{1};
// The last split's constants as the engine gave them: jitter x, y (input pixels), mvecScale x, y; and the scale
// the eyes' jitter was given, x, y.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugEngineStereoDlssJitter[6] = {};
// Test only, live: non-zero x / y replace the eyes' jitter scale (sign included); [2] non-zero replaces the
// motion scale's x factor (2 by default).
extern "C" __declspec(dllexport) float CyberpunkVR_EngineStereoDlssJitterTest[3] = {};
// Test only, live: bit N set = the eyes' DLSS gets no resource for the engine's tag of type N (29 = the bias
// current colour mask).
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EngineStereoDlssDropTags{0};

namespace cvr::engine_stereo::dlss {
namespace {
constexpr uint32_t kEyeViewport[2]{0x43565201u, 0x43565202u};
constexpr uint32_t kMaxTags = 24;

struct TagSet {
    sl::ResourceTag tag[kMaxTags];
    sl::Resource resource[kMaxTags];
    uint32_t count = 0, viewport = 0;
    bool valid = false;
};
struct ConstantSet { sl::Constants values; uint32_t frame = 0, viewport = 0; bool valid = false; };
struct OptionSet { sl::DLSSOptions values; uint32_t viewport = 0; bool valid = false; };

std::recursive_mutex mutex;
cvr::gpu::IdleDlssContext originalContext;
std::atomic<bool> lifecycleHooksReady{false};
std::atomic<bool> optionsIntercepted{false};
bool originalReclaimed=false;
thread_local int evaluatingEye = -1;   // EvaluatingEye()
TagSet tags;
ConstantSet constants;
OptionSet engineOptions;
OptionSet eyeOptions[2];
SetTagFn setTag = nullptr;

using SetOptionsFn = sl::Result (*)(const sl::ViewportHandle&, const sl::DLSSOptions&);
using GetFeatureFunctionFn = sl::Result (*)(sl::Feature, const char*, void*&);
SetOptionsFn setOptions = nullptr;        // unhooked slDLSSSetOptions (the trampoline once hooked)
std::atomic<bool> optionsHooked{false};

bool OwnViewport(uint32_t v) { return v == kEyeViewport[0] || v == kEyeViewport[1]; }

// THE RIGHT EYE'S OWN RESOURCES. DLSS takes the right eye's colour from its half at an offset, but writes nothing
// at an output offset (aimed at x = 2648 of the engine's 5296-wide target the right half stayed black; the same
// evaluate aimed at the left half drew the right eye) and, by the right eye shimmering and its picture swimming
// while the left one was steady, does not take its motion and depth there either. So the right eye gets copies:
// its half of every input the size of the scaling input (colour, motion, depth, masks) into a texture of the
// eye's size, read at (0, 0), and its output into a target of its own, copied into its half afterwards -- all on
// the evaluate's command list. A depth-stencil resource cannot be copied in part and keeps the offset. A
// Each recording retains its copies through submission and GPU fence completion.
// Replacing a cache entry releases only the cache's reference, never an in-flight copy.
struct EyeCopy {
    ID3D12Resource* resource = nullptr;
    sl::Resource tag{};
    uint64_t width = 0;
    uint32_t height = 0;
    DXGI_FORMAT format = DXGI_FORMAT_UNKNOWN;
    D3D12_RESOURCE_STATES state = D3D12_RESOURCE_STATE_COMMON;   // where it rests between evaluates
    sl::BufferType type = 0;
    bool output = false;
    bool logged = false;
    ID3D12Resource* staging = nullptr;   // a depth-stencil source copied whole first (it cannot be copied in part)
};
EyeCopy copies[kMaxTags];
constexpr D3D12_RESOURCE_STATES kReadable = D3D12_RESOURCE_STATES(D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
                                                                  D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);

DXGI_FORMAT Typed(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_R16G16B16A16_TYPELESS: return DXGI_FORMAT_R16G16B16A16_FLOAT;
    case DXGI_FORMAT_R32G32B32A32_TYPELESS: return DXGI_FORMAT_R32G32B32A32_FLOAT;
    case DXGI_FORMAT_R10G10B10A2_TYPELESS: return DXGI_FORMAT_R10G10B10A2_UNORM;
    case DXGI_FORMAT_R8G8B8A8_TYPELESS: return DXGI_FORMAT_R8G8B8A8_UNORM;
    case DXGI_FORMAT_B8G8R8A8_TYPELESS: return DXGI_FORMAT_B8G8R8A8_UNORM;
    case DXGI_FORMAT_R16G16_TYPELESS: return DXGI_FORMAT_R16G16_FLOAT;
    case DXGI_FORMAT_R32G32_TYPELESS: return DXGI_FORMAT_R32G32_FLOAT;
    case DXGI_FORMAT_R32_TYPELESS: return DXGI_FORMAT_R32_FLOAT;
    case DXGI_FORMAT_R16_TYPELESS: return DXGI_FORMAT_R16_FLOAT;
    case DXGI_FORMAT_R8_TYPELESS: return DXGI_FORMAT_R8_UNORM;
    case DXGI_FORMAT_R8G8_TYPELESS: return DXGI_FORMAT_R8G8_UNORM;
    default: return f;
    }
}

bool KeepCopy(ID3D12GraphicsCommandList* list, const EyeCopy& copy) {
    if (cvr::gpu::HasCommandResource(list, copy.resource) &&
        (!copy.staging || cvr::gpu::HasCommandResource(list, copy.staging))) return true;
    return copy.staging
        ? cvr::gpu::KeepCommandResources(list, {copy.resource, copy.staging})
        : cvr::gpu::KeepCommandResources(list, {copy.resource});
}

D3D12_RESOURCE_DESC DescOf(ID3D12Resource* r, bool* ok) {
    D3D12_RESOURCE_DESC d{};
    *ok = false;
    __try { d = r->GetDesc(); *ok = true; } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return d;
}

// The right eye's copy for a tag of this type: the source's format at the eye's size, made or remade as needed.
EyeCopy* OwnCopy(sl::BufferType type, ID3D12Resource* source, uint64_t width, uint32_t height, bool output) {
    bool ok = false;
    D3D12_RESOURCE_DESC d = DescOf(source, &ok);
    if (!ok) return nullptr;
    EyeCopy* slot = nullptr;
    for (auto& c : copies) if ((c.resource || c.logged) && c.type == type) { slot = &c; break; }
    if (!slot) for (auto& c : copies) if (!c.resource && !c.logged) { slot = &c; break; }
    if (!slot) return nullptr;
    slot->type = type;
    const bool depth = (d.Flags & D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL) != 0;
    if (d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || (depth && (output || d.MipLevels != 1 || d.DepthOrArraySize != 1))) {
        if (!slot->logged) Log("[engine-stereo-dlss] right eye tag %u: format %d flags 0x%X not copied; offset kept\n",
                               uint32_t(type), int(d.Format), unsigned(d.Flags));
        slot->logged = true;
        return nullptr;
    }
    const DXGI_FORMAT format = Typed(d.Format);
    if (slot->resource && slot->width == width && slot->height == height && slot->format == format && slot->output == output &&
            (slot->staging != nullptr) == depth)
        return slot;
    if (slot->resource) slot->resource->Release();
    if (slot->staging) slot->staging->Release();
    slot->resource = slot->staging = nullptr;
    ID3D12Device* device = nullptr;
    if (FAILED(source->GetDevice(IID_PPV_ARGS(&device))) || !device) return nullptr;
    const DXGI_FORMAT sourceFormat = d.Format;
    D3D12_HEAP_PROPERTIES stagingHeap{};
    stagingHeap.Type = D3D12_HEAP_TYPE_DEFAULT;
    ID3D12Resource* staging = nullptr;
    if (depth) {
        D3D12_RESOURCE_DESC whole = d;
        whole.Flags = D3D12_RESOURCE_FLAG_NONE;
        whole.Alignment = 0;
        const HRESULT shr = device->CreateCommittedResource(&stagingHeap, D3D12_HEAP_FLAG_NONE, &whole, D3D12_RESOURCE_STATE_COPY_DEST,
                                                            nullptr, IID_PPV_ARGS(&staging));
        Log("[engine-stereo-dlss] right eye tag %u: whole-depth staging %llux%u format %d: 0x%08X\n", uint32_t(type),
            (unsigned long long)whole.Width, whole.Height, int(whole.Format), unsigned(shr));
        if (FAILED(shr) || !staging) { device->Release(); return nullptr; }
        staging->SetName(L"CyberpunkVR right eye DLSS depth staging");
    }
    d.Width = width;
    d.Height = height;
    d.DepthOrArraySize = 1;
    d.MipLevels = 1;
    d.Alignment = 0;
    d.Format = format;
    d.SampleDesc = {1, 0};
    d.Layout = D3D12_TEXTURE_LAYOUT_UNKNOWN;
    d.Flags = output ? D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS : D3D12_RESOURCE_FLAG_NONE;
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    const D3D12_RESOURCE_STATES state = output ? D3D12_RESOURCE_STATE_UNORDERED_ACCESS : kReadable;
    ID3D12Resource* resource = nullptr;
    const HRESULT hr = device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, state, nullptr, IID_PPV_ARGS(&resource));
    device->Release();
    Log("[engine-stereo-dlss] right eye %s tag %u: %llux%u format %d (source format %d): 0x%08X\n", output ? "output" : "input",
        uint32_t(type), (unsigned long long)width, height, int(format), int(sourceFormat), unsigned(hr));
    if (FAILED(hr) || !resource) { if (staging) staging->Release(); return nullptr; }
    resource->SetName(output ? L"CyberpunkVR right eye DLSS output" : L"CyberpunkVR right eye DLSS input");
    slot->resource = resource;
    slot->staging = staging;
    slot->tag = sl::Resource(sl::ResourceType::eTex2d, resource, uint32_t(state));
    slot->width = width;
    slot->height = height;
    slot->format = format;
    slot->state = state;
    slot->output = output;
    return slot;
}

void Transition(D3D12_RESOURCE_BARRIER& b, ID3D12Resource* r, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to) {
    b = D3D12_RESOURCE_BARRIER{};
    b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition.pResource = r;
    b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b.Transition.StateBefore = from;
    b.Transition.StateAfter = to;
}

// Copies between regions of engine resources and eye copies, one way, everything returned to its state.
struct CopyJob { ID3D12Resource* engine; D3D12_RESOURCE_STATES engineState; EyeCopy* own; sl::Extent half; };
void RunCopies(void* commandList, const CopyJob* jobs, uint32_t n, bool intoEngine) {
    if (!n) return;
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commandList);
    D3D12_RESOURCE_BARRIER b[3 * kMaxTags];
    uint32_t k = 0;
    const auto engineTo = intoEngine ? D3D12_RESOURCE_STATE_COPY_DEST : D3D12_RESOURCE_STATE_COPY_SOURCE;
    const auto ownTo = intoEngine ? D3D12_RESOURCE_STATE_COPY_SOURCE : D3D12_RESOURCE_STATE_COPY_DEST;
    for (uint32_t i = 0; i < n; ++i) {
        if (jobs[i].engineState != engineTo) Transition(b[k++], jobs[i].engine, jobs[i].engineState, engineTo);
        Transition(b[k++], jobs[i].own->resource, jobs[i].own->state, ownTo);
    }
    list->ResourceBarrier(k, b);
    // depth: the whole resource into its staging copy first, which the half is then taken from
    k = 0;
    for (uint32_t i = 0; i < n; ++i) {
        if (intoEngine || !jobs[i].own->staging) continue;
        D3D12_TEXTURE_COPY_LOCATION whole{}, from{};
        whole.pResource = jobs[i].own->staging;
        whole.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        from.pResource = jobs[i].engine;
        from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&whole, 0, 0, 0, &from, nullptr);
        Transition(b[k++], jobs[i].own->staging, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
    }
    if (k) list->ResourceBarrier(k, b);
    for (uint32_t i = 0; i < n; ++i) {
        D3D12_TEXTURE_COPY_LOCATION engine{}, own{};
        engine.pResource = !intoEngine && jobs[i].own->staging ? jobs[i].own->staging : jobs[i].engine;
        engine.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        own.pResource = jobs[i].own->resource;
        own.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        const sl::Extent& h = jobs[i].half;
        if (intoEngine) {
            const D3D12_BOX box{0, 0, 0, h.width, h.height, 1};
            list->CopyTextureRegion(&engine, h.left, h.top, 0, &own, &box);
        } else {
            const D3D12_BOX box{h.left, h.top, 0, h.left + h.width, h.top + h.height, 1};
            list->CopyTextureRegion(&own, 0, 0, 0, &engine, &box);
        }
    }
    k = 0;
    for (uint32_t i = 0; i < n; ++i) {
        if (jobs[i].engineState != engineTo) Transition(b[k++], jobs[i].engine, engineTo, jobs[i].engineState);
        Transition(b[k++], jobs[i].own->resource, ownTo, jobs[i].own->state);
        if (!intoEngine && jobs[i].own->staging)
            Transition(b[k++], jobs[i].own->staging, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
    }
    list->ResourceBarrier(k, b);
}

// The copies below read structures the engine owns: SEH only, no objects to unwind (callers hold the lock).
void CopyOptions(const sl::DLSSOptions& options, uint32_t v) {
    __try {
        engineOptions.values = options;
        engineOptions.viewport = v;
        engineOptions.valid = options.structVersion >= 1;
    } __except (EXCEPTION_EXECUTE_HANDLER) { engineOptions.valid = false; }
}

void CopyTags(uint32_t v, const sl::ResourceTag* in, uint32_t count) {
    __try {
        // A new viewport starts a new set; the same one adds to it (the engine may tag in several calls),
        // a type tagged again replaces its entry.
        if (!tags.valid || tags.viewport != v) { tags.count = 0; tags.viewport = v; }
        for (uint32_t i = 0; i < count; ++i) {
            const auto& src = in[i];
            if (src.structType != sl::ResourceTag::s_structType || src.structVersion != 1 || !src.resource ||
                src.resource->structType != sl::Resource::s_structType || src.resource->structVersion != 1) continue;
            uint32_t slot = 0;
            while (slot < tags.count && tags.tag[slot].type != src.type) ++slot;
            if (slot == kMaxTags) continue;
            tags.tag[slot] = src;
            tags.resource[slot] = *src.resource;
            tags.resource[slot].next = nullptr;
            tags.tag[slot].resource = &tags.resource[slot];
            tags.tag[slot].next = nullptr;
            if (slot == tags.count) ++tags.count;
        }
        tags.valid = tags.count != 0;
    } __except (EXCEPTION_EXECUTE_HANDLER) { tags.valid = false; }
}

void CopyConstants(const sl::Constants& values, uint32_t frame, uint32_t v) {
    __try {
        if (values.structType != sl::Constants::s_structType || values.structVersion < 1 || values.structVersion > 2) return;
        constants.values = values;
        constants.values.next = nullptr;
        constants.frame = frame;
        constants.viewport = v;
        constants.valid = true;
    } __except (EXCEPTION_EXECUTE_HANDLER) { constants.valid = false; }
}

// The index of the viewport among an evaluate's inputs, -1 without one.
int FindViewport(const sl::BaseStructure* const* inputs, uint32_t count, uint32_t* viewport) {
    __try {
        for (uint32_t i = 0; i < count; ++i)
            if (inputs[i] && inputs[i]->structType == sl::ViewportHandle::s_structType) {
                *viewport = uint32_t(*static_cast<const sl::ViewportHandle*>(inputs[i]));
                return int(i);
            }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return -1;
}

uint32_t ViewportValue(const void* handle) {
    __try { return uint32_t(*static_cast<const sl::ViewportHandle*>(handle)); }
    __except (EXCEPTION_EXECUTE_HANDLER) { return kEyeViewport[0]; }   // treated as ours: ignored
}

using AllocateResourcesFn = sl::Result (*)(void*,sl::Feature,const sl::ViewportHandle&);
using FreeResourcesFn = sl::Result (*)(sl::Feature,const sl::ViewportHandle&);
AllocateResourcesFn allocateResources = nullptr;
FreeResourcesFn freeResources = nullptr;
sl::Result TrackedAllocate(void* list,sl::Feature feature,const sl::ViewportHandle& viewport) {
    std::lock_guard lock(mutex);
    if(feature==sl::kFeatureDLSS && ViewportValue(&viewport)==0)
        originalContext.Used(static_cast<ID3D12GraphicsCommandList*>(list),GetTickCount64());
    const auto result=allocateResources(list,feature,viewport);
    return result;
}
sl::Result TrackedFree(sl::Feature feature,const sl::ViewportHandle& viewport) {
    std::lock_guard lock(mutex);
    const auto result=freeResources(feature,viewport);
    if(result==sl::Result::eOk && feature==sl::kFeatureDLSS && ViewportValue(&viewport)==0)originalContext.Released();
    return result;
}

sl::Result HookedSetOptions(const sl::ViewportHandle& viewport, const sl::DLSSOptions& options) {
    std::lock_guard lock(mutex);
    const uint32_t v = ViewportValue(&viewport);
    if (!OwnViewport(v)) {
        CopyOptions(options, v);
        ++CyberpunkVR_DebugEngineStereoDlss[7];
    }
    return setOptions(viewport, options);
}

bool Same(const sl::DLSSOptions& a, const sl::DLSSOptions& b) {
    return a.mode == b.mode && a.outputWidth == b.outputWidth && a.outputHeight == b.outputHeight &&
           a.preExposure == b.preExposure && a.exposureScale == b.exposureScale && a.colorBuffersHDR == b.colorBuffersHDR &&
           a.dlaaPreset == b.dlaaPreset && a.qualityPreset == b.qualityPreset && a.balancedPreset == b.balancedPreset &&
           a.performancePreset == b.performancePreset && a.ultraPerformancePreset == b.ultraPerformancePreset &&
           a.ultraQualityPreset == b.ultraQualityPreset && a.useAutoExposure == b.useAutoExposure &&
           a.alphaUpscalingEnabled == b.alphaUpscalingEnabled && a.structVersion == b.structVersion;
}

// The extent a tag covers: its own, or the whole resource.
bool Area(const sl::ResourceTag& tag, sl::Extent* out) {
    if (tag.extent.width && tag.extent.height) { *out = tag.extent; return true; }
    auto* native = tag.resource ? static_cast<ID3D12Resource*>(tag.resource->native) : nullptr;
    if (!native) return false;
    D3D12_RESOURCE_DESC desc{};
    __try { desc = native->GetDesc(); } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    *out = sl::Extent{0, 0, uint32_t(desc.Width), desc.Height};
    return out->width && out->height;
}

sl::Extent Half(const sl::Extent& e, int eye) {
    const uint32_t w = e.width / 2;
    return sl::Extent{e.top, e.left + (eye ? w : 0), w, e.height};
}

// Options when the engine set its own before the hook went in: the mode from the scale, the rest the defaults
// of an HDR colour pipeline that tags its exposure.
sl::DLSSOptions Derived(const sl::Extent& in, const sl::Extent& out, bool exposureTagged) {
    sl::DLSSOptions o{};
    const double scale = double(in.width) / double(out.width);
    o.mode = scale > 0.95 ? sl::DLSSMode::eDLAA : scale > 0.62 ? sl::DLSSMode::eMaxQuality :
             scale > 0.54 ? sl::DLSSMode::eBalanced : scale > 0.42 ? sl::DLSSMode::eMaxPerformance :
             sl::DLSSMode::eUltraPerformance;
    o.colorBuffersHDR = sl::Boolean::eTrue;
    o.useAutoExposure = exposureTagged ? sl::Boolean::eFalse : sl::Boolean::eTrue;
    return o;
}

void LogOnce(const TagSet& t, const ConstantSet& c, const sl::DLSSOptions& o, bool recorded, const sl::Extent& in,
             const sl::Extent& out) {
    static std::atomic<bool> logged{false};
    if (logged.exchange(true)) return;
    Log("[engine-stereo-dlss] per-eye DLSS: viewport %u -> %u/%u, input %ux%u at (%u,%u), output %ux%u at (%u,%u)\n",
        t.viewport, kEyeViewport[0], kEyeViewport[1], in.width, in.height, in.left, in.top, out.width, out.height, out.left, out.top);
    for (uint32_t i = 0; i < t.count; ++i) {
        sl::Extent a{};
        Area(t.tag[i], &a);
        Log("[engine-stereo-dlss]   tag type %u lifecycle %u extent %ux%u at (%u,%u) resource %ux%u state 0x%X\n",
            uint32_t(t.tag[i].type), uint32_t(t.tag[i].lifecycle), t.tag[i].extent.width, t.tag[i].extent.height,
            t.tag[i].extent.left, t.tag[i].extent.top, a.width, a.height, t.tag[i].resource ? t.tag[i].resource->state : 0);
    }
    Log("[engine-stereo-dlss]   constants jitter (%.4f,%.4f) mvecScale (%.6f,%.6f) depthInverted %d mvJittered %d cameraMotion %d "
        "mv3D %d reset %d\n", c.values.jitterOffset.x, c.values.jitterOffset.y, c.values.mvecScale.x, c.values.mvecScale.y,
        int(c.values.depthInverted), int(c.values.motionVectorsJittered), int(c.values.cameraMotionIncluded),
        int(c.values.motionVectors3D), int(c.values.reset));
    Log("[engine-stereo-dlss]   options (%s) v%llu mode %u output %ux%u preExposure %.3f exposureScale %.3f hdr %d autoExposure %d "
        "presets %u/%u/%u/%u/%u/%u\n", recorded ? "the engine's" : "derived", (unsigned long long)o.structVersion, uint32_t(o.mode),
        o.outputWidth, o.outputHeight, o.preExposure, o.exposureScale, int(o.colorBuffersHDR), int(o.useAutoExposure),
        uint32_t(o.dlaaPreset), uint32_t(o.qualityPreset), uint32_t(o.balancedPreset), uint32_t(o.performancePreset),
        uint32_t(o.ultraPerformancePreset), uint32_t(o.ultraQualityPreset));
}
}  // namespace

void RecordTags(const void* viewport, const void* list, uint32_t count, SetTagFn original) {
    if (!Enabled() || !viewport || !list || !count) return;
    const uint32_t v = ViewportValue(viewport);
    if (OwnViewport(v)) return;
    std::lock_guard lock(mutex);
    setTag = original;
    CopyTags(v, static_cast<const sl::ResourceTag*>(list), count);
}

void RecordConstants(const sl::Constants& values, const sl::FrameToken& frame, const sl::ViewportHandle& viewport) {
    if (!Enabled()) return;
    const uint32_t v = ViewportValue(&viewport);
    if (OwnViewport(v)) return;
    std::lock_guard lock(mutex);
    CopyConstants(values, uint32_t(frame), v);
}

void InstallOptionsHook() {
    if (optionsHooked.load(std::memory_order_acquire)) return;
    static std::mutex installMutex;
    std::lock_guard lock(installMutex);
    if (optionsHooked.load(std::memory_order_relaxed)) return;
    const auto module = GetModuleHandleW(L"sl.interposer.dll");
    if(module) {
        static bool allocateHooked=false,freeHooked=false;
        if(!allocateResources) {
            auto target=GetProcAddress(module,"slAllocateResources");
            if(target && MH_CreateHook(target,reinterpret_cast<void*>(&TrackedAllocate),reinterpret_cast<void**>(&allocateResources))==MH_OK)
                allocateHooked=MH_EnableHook(target)==MH_OK;
        }
        if(!freeResources) {
            auto target=GetProcAddress(module,"slFreeResources");
            if(target && MH_CreateHook(target,reinterpret_cast<void*>(&TrackedFree),reinterpret_cast<void**>(&freeResources))==MH_OK)
                freeHooked=MH_EnableHook(target)==MH_OK;
        }
        lifecycleHooksReady.store(allocateHooked && freeHooked,std::memory_order_release);
    }
    auto get = module ? reinterpret_cast<GetFeatureFunctionFn>(GetProcAddress(module, "slGetFeatureFunction")) : nullptr;
    void* target = nullptr;
    if (!get || get(sl::kFeatureDLSS, "slDLSSSetOptions", target) != sl::Result::eOk || !target) return;
    if (MH_CreateHook(target, reinterpret_cast<void*>(&HookedSetOptions), reinterpret_cast<void**>(&setOptions)) != MH_OK ||
        MH_EnableHook(target) != MH_OK) {
        // Not hookable: still callable for the eyes, and the options derived.
        MH_RemoveHook(target);
        setOptions = reinterpret_cast<SetOptionsFn>(target);
        Log("[engine-stereo-dlss] slDLSSSetOptions could not be hooked; eye options are derived\n");
    } else {
        optionsIntercepted.store(true,std::memory_order_release);
        Log("[engine-stereo-dlss] slDLSSSetOptions hooked\n");
    }
    optionsHooked.store(true, std::memory_order_release);
}

bool Evaluate(sl::Feature feature, const sl::FrameToken& frame, const sl::BaseStructure* const* inputs, uint32_t count,
              void* commandList, EvaluateFn evaluate, ConstantsFn setConstants, sl::Result* result) {
    auto* debug = CyberpunkVR_DebugEngineStereoDlss;
    // A frame the engine rendered mono (before the main menu) is one picture: one evaluate over it, as the engine's.
    if (feature != sl::kFeatureDLSS || !Enabled() || !Rendering() ||
        !CyberpunkVR_EngineStereoDlssPerEye.load(std::memory_order_relaxed) ||
        !inputs || !count || count > 15 || !commandList || !evaluate || !setConstants || !result) {
        ++debug[1];
        return false;
    }
    InstallOptionsHook();
    uint32_t viewport = 0;
    const int at = FindViewport(inputs, count, &viewport);
    std::lock_guard lock(mutex);
    if (at < 0 || OwnViewport(viewport) || !setTag || !setOptions) { ++debug[1]; return false; }
    if (!tags.valid || tags.viewport != viewport) { ++debug[2]; return false; }
    if (!constants.valid || constants.viewport != viewport || constants.frame != uint32_t(frame)) { ++debug[3]; return false; }
    sl::Extent in{}, out{};
    bool haveIn = false, haveOut = false, exposureTagged = false;
    for (uint32_t i = 0; i < tags.count; ++i) {
        if (tags.tag[i].type == sl::kBufferTypeScalingInputColor) haveIn = Area(tags.tag[i], &in);
        else if (tags.tag[i].type == sl::kBufferTypeScalingOutputColor) haveOut = Area(tags.tag[i], &out);
        else if (tags.tag[i].type == sl::kBufferTypeExposure) exposureTagged = true;
    }
    if (!haveIn || !haveOut || in.width < 2 || out.width < 2 || in.width % 2 || out.width % 2) { ++debug[4]; return false; }
    const bool recorded = engineOptions.valid && engineOptions.viewport == viewport;
    const sl::DLSSOptions base = recorded ? engineOptions.values : Derived(in, out, exposureTagged);
    LogOnce(tags, constants, base, recorded, in, out);

    // Only the observed engine viewport (0). A live recording or unfinished GPU
    // submission owns a marker, so elapsed frames alone can never allow release.
    if(viewport==0 && recorded && optionsIntercepted.load(std::memory_order_acquire) &&
       lifecycleHooksReady.load(std::memory_order_acquire) && originalContext.Ready(GetTickCount64())) {
        const sl::ViewportHandle original(0);
        const auto released=freeResources(sl::kFeatureDLSS,original);
        Log("[engine-stereo-dlss-reclaim] viewport=0 result=%d pending=%u tick_ms=%llu\n",int(released),originalContext.Pending(),GetTickCount64());
        if(released==sl::Result::eOk){originalContext.Released();originalReclaimed=true;}
        else originalContext.Disable(); // no repeated attempts or render-loop stalls
    }

    const uint32_t mode = CyberpunkVR_EngineStereoDlssMode.load(std::memory_order_relaxed);
    static std::atomic<uint32_t> logged{0};
    const bool log = logged.fetch_add(1) < 3;
    if (log) {
        Log("[engine-stereo-dlss] evaluate frame %u viewport %u inputs %u (mode %u):", uint32_t(frame), viewport, count, mode);
        for (uint32_t i = 0; i < count; ++i) Log(" %08X", inputs[i] ? inputs[i]->structType.data1 : 0u);
        Log("\n");
    }
    float jitterX = 1.0f, jitterY = 1.0f;
    uint32_t jitterWidth = 0, jitterHeight = 0;
    if (CyberpunkVR_EngineStereoDlssEyeJitter.load(std::memory_order_relaxed) &&
        cvr::engine_stereo::EyeJitterSize(&jitterWidth, &jitterHeight)) {
        jitterX = float(in.width / 2) / float(jitterWidth);
        jitterY = float(in.height) / float(jitterHeight);
    }
    if (CyberpunkVR_EngineStereoDlssJitterTest[0] != 0.0f) jitterX = CyberpunkVR_EngineStereoDlssJitterTest[0];
    if (CyberpunkVR_EngineStereoDlssJitterTest[1] != 0.0f) jitterY = CyberpunkVR_EngineStereoDlssJitterTest[1];
    const float motionX = CyberpunkVR_EngineStereoDlssJitterTest[2] != 0.0f ? CyberpunkVR_EngineStereoDlssJitterTest[2] : 2.0f;
    CyberpunkVR_DebugEngineStereoDlssJitter[0] = constants.values.jitterOffset.x;
    CyberpunkVR_DebugEngineStereoDlssJitter[1] = constants.values.jitterOffset.y;
    CyberpunkVR_DebugEngineStereoDlssJitter[2] = constants.values.mvecScale.x;
    CyberpunkVR_DebugEngineStereoDlssJitter[3] = constants.values.mvecScale.y;
    CyberpunkVR_DebugEngineStereoDlssJitter[4] = jitterX;
    CyberpunkVR_DebugEngineStereoDlssJitter[5] = jitterY;
    sl::Result worst = sl::Result::eOk;
    for (int step = 0; step < 2; ++step) {
        const int eye = (mode & 1) ? 1 - step : step;
        if (eye == 1 && (mode & 4)) continue;
        const sl::ViewportHandle handle(kEyeViewport[eye]);
        sl::DLSSOptions options = base;
        options.outputWidth = out.width / 2;
        options.outputHeight = out.height;
        int32_t* res = CyberpunkVR_DebugEngineStereoDlssResults + eye * 4;
        if (!eyeOptions[eye].valid || !Same(eyeOptions[eye].values, options)) {
            const auto r = setOptions(handle, options);
            eyeOptions[eye].values = options;
            eyeOptions[eye].valid = r == sl::Result::eOk;
            res[0] = int32_t(r);
            ++debug[5];
            if (r != sl::Result::eOk) Log("[engine-stereo-dlss] eye %d options refused (%d)\n", eye, int(r));
        }
        sl::Constants values = constants.values;
        values.mvecScale.x *= motionX;
        values.jitterOffset.x *= jitterX;
        values.jitterOffset.y *= jitterY;
        res[2] = int32_t(setConstants(values, frame, handle));
        sl::ResourceTag eyeTags[kMaxTags];
        // Native DLSS-G adds HUD-less color (tag 2) alongside scaling output
        // (tag 4). Both may be output-sized. The old one-element after array
        // overwrote the stack when the second copy job was appended.
        CopyJob before[kMaxTags], after[kMaxTags];
        static_assert(sizeof(after)/sizeof(after[0]) == sizeof(eyeTags)/sizeof(eyeTags[0]));
        uint32_t nBefore = 0, nAfter = 0;
        for (uint32_t i = 0; i < tags.count; ++i) {
            eyeTags[i] = tags.tag[i];
            sl::Extent a{};
            if (!Area(tags.tag[i], &a)) continue;
            const bool input = a.width == in.width && a.height == in.height;
            const bool isOutput = !input && a.width == out.width && a.height == out.height;
            if (input) eyeTags[i].extent = Half(a, eye);
            else if (isOutput) eyeTags[i].extent = Half(a, (mode & 2) ? 0 : eye);
            if (eye != 1 || (!input && !isOutput) || (isOutput && (mode & 18)) || (input && (mode & 32))) continue;
            auto* native = static_cast<ID3D12Resource*>(tags.tag[i].resource->native);
            const sl::Extent half = eyeTags[i].extent;
            bool shared = false;                           // one resource under two tags: copied once, offset kept
            for (uint32_t j = 0; j < tags.count; ++j)
                if (j != i && tags.tag[j].resource && tags.tag[j].resource->native == native) shared = true;
            EyeCopy* own = native && !shared ? OwnCopy(tags.tag[i].type, native, half.width, half.height, isOutput) : nullptr;
            if (!own) continue;
            if (!KeepCopy(static_cast<ID3D12GraphicsCommandList*>(commandList), *own)) {
                // Do not record references whose lifetime we cannot protect.
                // Preserve the existing offset fallback used for allocation failure.
                static bool warned = false;
                if (!warned) Log("[engine-stereo-dlss-memory] lifetime registration failed; right-eye copy skipped\n");
                warned = true;
                continue;
            }
            const CopyJob job{native, D3D12_RESOURCE_STATES(tags.tag[i].resource->state), own, half};
            if (isOutput) after[nAfter++] = job;
            else before[nBefore++] = job;
            eyeTags[i].resource = &own->tag;
            eyeTags[i].extent = sl::Extent{0, 0, half.width, half.height};
        }
        if(nBefore) {
            RunCopies(commandList, before, nBefore, false);
        }
        const uint32_t drop = CyberpunkVR_EngineStereoDlssDropTags.load(std::memory_order_relaxed);
        for (uint32_t i = 0; i < tags.count; ++i)
            if (tags.tag[i].type < 32 && (drop >> tags.tag[i].type) & 1u) eyeTags[i].resource = nullptr;
        res[1] = int32_t(setTag(&handle, eyeTags, tags.count, commandList));
        const sl::BaseStructure* eyeInputs[16 + kMaxTags]{};
        uint32_t n = 0;
        for (uint32_t i = 0; i < count; ++i) eyeInputs[n++] = int(i) == at ? &handle : inputs[i];
        if (mode & 8)
            for (uint32_t i = 0; i < tags.count; ++i) eyeInputs[n++] = &eyeTags[i];
        evaluatingEye = eye;
        const auto r = evaluate(feature, frame, eyeInputs, n, commandList);
        evaluatingEye = -1;
        res[3] = int32_t(r);
        if (r == sl::Result::eOk && nAfter) {
            RunCopies(commandList, after, nAfter, true);
        }
        if (log)
            Log("[engine-stereo-dlss]   eye %d: tags %d constants %d evaluate %d; in (%u,%u %ux%u) out (%u,%u %ux%u)\n", eye,
                res[1], res[2], res[3], Half(in, eye).left, Half(in, eye).top, Half(in, eye).width, Half(in, eye).height,
                Half(out, (mode & 2) ? 0 : eye).left, Half(out, (mode & 2) ? 0 : eye).top, Half(out, eye).width, Half(out, eye).height);
        if (r != sl::Result::eOk) {
            worst = r;
            if (debug[6]++ < 4) Log("[engine-stereo-dlss] eye %d evaluate failed (%d)\n", eye, int(r));
        }
    }
    ++debug[0];
    if(worst==sl::Result::eOk && viewport==0)originalContext.SplitSucceeded();
    *result = worst;
    return true;
}

sl::Result EvaluateOriginal(sl::Feature feature,const sl::FrameToken& frame,const sl::BaseStructure* const* inputs,
                            uint32_t count,void* list,EvaluateFn evaluate) {
    std::lock_guard lock(mutex);
    if(feature==sl::kFeatureDLSS) {
        uint32_t viewport=UINT32_MAX;
        if(FindViewport(inputs,count,&viewport)<0)originalContext.Disable();
        else if(viewport==0) {
            originalContext.Used(static_cast<ID3D12GraphicsCommandList*>(list),GetTickCount64());
            if(originalReclaimed) {
                // Streamline recreates the feature lazily on evaluate. Reassert
                // current engine options, never substitute an eye-sized preset.
                if(engineOptions.valid && engineOptions.viewport==0 && setOptions) {
                    const sl::ViewportHandle original(0);
                    setOptions(original,engineOptions.values);
                }
                Log("[engine-stereo-dlss-reclaim] viewport=0 resumed; lazy recreation tick_ms=%llu\n",GetTickCount64());
                originalReclaimed=false;
            }
        }
    }
    return evaluate(feature,frame,inputs,count,list);
}

int EvaluatingEye() { return evaluatingEye; }

}  // namespace cvr::engine_stereo::dlss
