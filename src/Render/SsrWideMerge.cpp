#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// SsrWideMerge -- screen-space reflections of the two eyes computed as ONE wide display.
//
// The SSR trace (node ScreenSpaceReflections, 0x77A100) can only reflect what its own picture shows. Under
// the asymmetric projection each eye's lens window reaches 14 degrees further outwards than the other's, so
// the two eyes reflect different things near their edges, and a small head turn moves those edges through
// the reflections -- a slight head turn and the reflections are different. The shared-view fade before this
// (2026-10-15, v2) made the eyes agree by cutting every hit to the part both windows share, the narrower
// of the two. This does the opposite: a reflection exists if EITHER eye's picture holds it, the way it would
// on a wide monitor spanning both windows.
//
// The trace twins (tools/asym_tests/asym_ssr_shaders.py, wide_merge) keep every traced pixel's result --
// colour, confidence, view depth and a stamp -- in a store of their own (u16), with a header that describes
// the camera it was traced from. Before writing their own result they look the same surface point up in
// the OTHER eye's store (u17): through their own camera to the world and through the stored camera into the
// other eye's picture, accepted only if the other eye saw the same surface there (view depth within the
// tolerance). Where the other eye's ray found more than this eye's, its result is taken, so near one window's
// edge the eye with the wider view supplies the reflection for both. MAIN reads the second view's current
// frame (the second view renders first); the second view reads MAIN's previous frame, which the reprojection
// through both cameras keeps in place for everything that does not move.
//
// The engine does not trace every tile every frame: its classifier sends a tile whose coarse, noisy rays all
// miss to a list it only clears -- exactly the tiles at an eye's edge whose rays leave its picture, and a
// different set every frame. Measured 2026-10-15: with the latest pass alone the merge left tile-sized dark
// blocks jumping on the tower glass even in a still frame. So the clear is twinned too (it merges instead of
// clearing), and an entry is accepted up to 3 passes old, through the camera of the pass that wrote it.
//
// The planar reflections (node ScreenSpacePlanarReflections, 0x77D450) trace with the same block, built by
// the same sub_140789724 and uploaded the same way, and have the same edges: their trace twin
// (asym_ssr_shaders.py, planar_merge) merges the ray's own hit before the shader's inline history blend, on
// a store pair of its own (channel kPlanar).
//
// The cone AO (node DrawConeAO) counts a cone sample whose uv leaves the picture as open, so its occlusion
// thins out towards each eye's inner edge where the other eye still sees the occluders: its trace twins
// (asym_ao_shaders.py, ao_merge) move towards the other eye's result by how many more samples they lost,
// on a store pair of their own (channel kConeAo). Their header is built from the block itself (rows 4-7
// view->world, the centred tangents); PrepareConeAo only fills row 11 and binds the stores.
//
// This file is the CPU side: the two stores, the three rows the twins read (24-26, after the engine's 372
// bytes of CB_SSRRaymarch, uploaded as 384), and the binding of u16/u17 on the node's own command context
// through the engine's binder, so the engine copies them into the trace's descriptor table like any of its
// own. Without the tag in row 24 the twins never touch the stores and trace exactly as the engine does.
#include "Render/SsrWideMerge.hpp"
#include "Render/ViewCameraLayout.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Stereo/StereoInternal.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <vector>

#if !CVR_RUST_VRIK
extern void Log(const char*,...);
#endif
#if !CVR_RUST_VRIK
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymSsrPatch;          // Render/HiddenAreaShaders.cpp
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymSsrPatchLoaded;
#endif
// 1 = merge the eyes' reflections (needs the asym_ssr twins); 0 = each eye traces alone.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SsrMerge{1};
// The same for the planar reflections.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SsprMerge{1};
// 0 = merge; diagnostics: 1 = paint the pixels that took the other eye's result red, 2 = show the other eye's
// result wherever it is the same surface (black elsewhere), 3 = show this eye's own result.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_SsrMergeMode = 0;
// The other eye's result is the same surface if its view depth is within this fraction (+1 cm).
extern "C" __declspec(dllexport) float CyberpunkVR_SsrMergeTolerance = 0.03f;
// [0] MAIN blocks, [1] second-view blocks, [2] other views (left alone), [3] binder refused a slot,
// [4] store allocation failed, [5] block/camera unreadable, [6] store capacity (entries), [7] width << 16 | height.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSsrMerge[8] = {};
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugSsprMerge[8] = {};   // the same for the planar reflections
#endif
// The last block shipped per view (grown, 432 bytes).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
#else
extern "C" __declspec(dllexport) float CyberpunkVR_DebugSsrBlocks[2][cvr::ssr_merge::kBlockBytes/4] = {};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugSsprBlocks[2][cvr::ssr_merge::kBlockBytes/4] = {};
#endif
#if !CVR_RUST_VRIK
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymAoPatch;           // Render/HiddenAreaShaders.cpp
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymAoPatchLoaded;
#endif
// The cone AO's merge (channel kConeAo, tools/asym_tests/asym_ao_shaders.py ao_merge): 1 = on (needs the
// asym_ao twins and the centre Stereo/Grading.cpp writes into the block).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_AoMerge{1};
// 0 = merge; diagnostics: 1 = the pixels that moved towards the other eye black, 2 = the other eye's AO
// wherever it is the same surface (open elsewhere), 3 = this eye's own AO.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_AoMergeMode = 0;
extern "C" __declspec(dllexport) float CyberpunkVR_AoMergeTolerance = 0.03f;
// How fast a pixel moves to the other eye's AO: by this times the difference of the fractions of cone
// samples the two eyes lost off their pictures (4: all the way at a quarter more lost).
extern "C" __declspec(dllexport) float CyberpunkVR_AoMergeStrength = 4.0f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugAoMerge[8] = {};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
#else
extern "C" __declspec(dllexport) float CyberpunkVR_DebugAoBlocks[2][cvr::ssr_merge::kAoBlockBytes/4] = {};
#endif
#if !CVR_RUST_VRIK
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymShadowPatch;          // Render/HiddenAreaShaders.cpp
extern "C" std::atomic<uint32_t> CyberpunkVR_AsymShadowPatchLoaded;
#endif
// The shadow mask's sun factor merged as one wide display (channel kShadowMask, asym_shadow_shaders.py).
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ShadowMerge{1};
// 0 = merge; diagnostics: 1 = the pixels that moved towards the other eye fully shadowed, 2 = the other eye's
// factor wherever it is the same surface (lit elsewhere), 3 = this eye's own factor.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ShadowMergeMode = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugShadowMerge[8] = {};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugShadowBlocks[2][cvr::ssr_merge::kShadowBlockBytes/4] = {};
#endif
// Diagnostic: nonzero snapshots the exposure cells (cvr::ssr_merge::BindExposureCells) into
// CyberpunkVR_DebugExpoCells (raw entries: the cells' float3, then the frame's sums' uint3); Seq counts snapshots.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugExpoCellsRequest = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugExpoCellsSeq = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugExpoCells[2*cvr::ssr_merge::kExposureCells*3] = {};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ProbeLayoutNode = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugLayoutCount = 0;
extern "C" __declspec(dllexport) uint8_t CyberpunkVR_DebugLayoutBytes[4*14+4*128+4*19] = {};
#endif

#if CVR_RUST_VRIK
// The Rust build's stores and bindings are rust/crates/cvr-render/src/ssr_merge.rs; the exports and the game's device
// are attached to it by export_attach.rs as the plugin loads.
#else
namespace cvr::ssr_merge {
namespace {
using Microsoft::WRL::ComPtr;
struct StoreSet {
    ComPtr<ID3D12Resource> buffer[2];
    D3D12_CPU_DESCRIPTOR_HANDLE uav[2]{};
    uint32_t capacity{};
};
constexpr UINT kDescriptorPairs=8;
std::mutex mutex;
ComPtr<ID3D12DescriptorHeap> heap;
UINT stride{},nextPair{};
std::vector<std::pair<uint64_t,std::shared_ptr<StoreSet>>> retired;
// One store pair per channel: the two traces run in different nodes at different sizes.
struct ChannelState {
    std::shared_ptr<StoreSet> current;
    uint32_t refusedCapacity{};
    std::atomic<uint32_t> stamps[2]{};
};
ChannelState channels[kChannels];
constexpr const wchar_t* kStoreNames[kChannels][2]{
    {L"CyberpunkVR SSR store MAIN",L"CyberpunkVR SSR store second view"},
    {L"CyberpunkVR SSPR store MAIN",L"CyberpunkVR SSPR store second view"},
    {L"CyberpunkVR cone AO store MAIN",L"CyberpunkVR cone AO store second view"},
    {L"CyberpunkVR shadow mask store MAIN",L"CyberpunkVR shadow mask store second view"},
    {L"CyberpunkVR exposure cells MAIN",L"CyberpunkVR exposure cells second view"}};

// Two RWStructuredBuffer<uint3>, zeroed at creation (no tag in the header, so nothing reads as valid), in
// COMMON: a buffer is promoted to UNORDERED_ACCESS on use and decays back between submissions, so neither
// the engine's state tracker nor ours has anything to do with them.
std::shared_ptr<StoreSet> Ensure(Channel channel,uint32_t capacity) {
    std::lock_guard lock(mutex);
    auto& current=channels[channel].current;
    auto& refusedCapacity=channels[channel].refusedCapacity;
    const uint64_t now=GetTickCount64();
    std::erase_if(retired,[&](const auto& r){return now-r.first>10000;});
    if(current && current->capacity>=capacity)return current;
    if(refusedCapacity==capacity)return nullptr;
    auto* device=cvr::detail::g_game_device;
    if(!device)return nullptr;
    if(!heap) {
        D3D12_DESCRIPTOR_HEAP_DESC desc{};
        desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;desc.NumDescriptors=kDescriptorPairs*2;
        if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap)))){refusedCapacity=capacity;return nullptr;}
        stride=device->GetDescriptorHandleIncrementSize(desc.Type);
    }
    auto set=std::make_shared<StoreSet>();
    D3D12_HEAP_PROPERTIES properties{};properties.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=uint64_t(capacity)*kEntryBytes;desc.Height=1;
    desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    const UINT pair=nextPair++%kDescriptorPairs;
    for(int side=0;side<2;++side) {
        if(FAILED(device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COMMON,nullptr,
                                                  IID_PPV_ARGS(&set->buffer[side])))) {
            refusedCapacity=capacity;Log("[ssr-merge] store of %u entries could not be created (channel %d)\n",capacity,int(channel));
            return nullptr;
        }
        set->buffer[side]->SetName(kStoreNames[channel][side]);
        D3D12_UNORDERED_ACCESS_VIEW_DESC view{};
        view.ViewDimension=D3D12_UAV_DIMENSION_BUFFER;view.Buffer.NumElements=capacity;view.Buffer.StructureByteStride=kEntryBytes;
        set->uav[side].ptr=heap->GetCPUDescriptorHandleForHeapStart().ptr+SIZE_T(pair*2+side)*stride;
        device->CreateUnorderedAccessView(set->buffer[side].Get(),nullptr,&view,set->uav[side]);
    }
    set->capacity=capacity;
    if(current)retired.emplace_back(now,current);
    current=set;
    Log("[ssr-merge] %s stores of %u entries (%.1f MB each)\n",channel==kPlanar?"planar":channel==kConeAo?"cone AO":channel==kShadowMask?"shadow mask":
        channel==kExposure?"exposure":"SSR",capacity,
        double(capacity)*kEntryBytes/1048576.0);
    return set;
}

// sub_140336D3C's own lookup (table +13 layout, +5 slot handles): the handle now in UAV slot `slot`.
uint64_t BoundUav(uintptr_t table,uint32_t slot,uint32_t stage) {
    __try {
        const auto layout=*reinterpret_cast<const uintptr_t*>(table+13*8);
        const auto range=*reinterpret_cast<const uint8_t*>(layout+2243+38*stage+2*slot);
        if(range==0xFF)return 0;
        const auto entry=layout+56+16*uintptr_t(range);
        const auto index=slot+*reinterpret_cast<const uint32_t*>(entry+4)-*reinterpret_cast<const uint16_t*>(entry+8);
        return *reinterpret_cast<const uint64_t*>(*reinterpret_cast<const uintptr_t*>(table+5*8)+8*uintptr_t(index));
    } __except(EXCEPTION_EXECUTE_HANDLER) {return 0;}
}

bool ReadCamera(uintptr_t view,float out[3]) {
    // The view's scene camera descriptor: the position as 1/131072 fixed point (Render/ViewCameraLayout.hpp).
    __try {
        for(int i=0;i<3;++i)out[i]=float(double(*reinterpret_cast<const int32_t*>(view+cvr::view_layout::kCamera+4*i))/131072.0);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
    return std::isfinite(out[0]) && std::isfinite(out[1]) && std::isfinite(out[2]);
}

bool CopyBlock(uint8_t* out,const void* src,uint32_t size) {
    __try {std::memcpy(out,src,size);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
    return true;
}

// The command context the node records on (sub_140337230): its binding table at +0x60, its list at +0x30.
bool CurrentContext(uintptr_t base,uintptr_t& table,ID3D12GraphicsCommandList*& list) {
    using Current=uintptr_t(*)();
    __try {
        const auto context=reinterpret_cast<Current>(base+0x337230)();
        if(!context)return false;
        table=*reinterpret_cast<const uintptr_t*>(context+0x60);
        list=*reinterpret_cast<ID3D12GraphicsCommandList* const*>(context+0x30);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
    return table && list;
}
// The channel's stores, bound as u16 (this eye's) and u17 (the other eye's) on the command context the node
// records on, and ordered against the dispatches that touched them before. Null (counted) when anything refused.
// stage: the binder's shader stage, 2 compute (the default), 1 pixel.
std::shared_ptr<StoreSet> BindStores(Channel channel,int side,uint32_t capacity,uint64_t* debug,uint8_t stage=2) {
    const auto stores=Ensure(channel,capacity);
    if(!stores){++debug[4];return nullptr;}
    const auto base=reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base);
    using Bind=void(*)(uintptr_t,uint8_t,uint32_t,uint32_t,const D3D12_CPU_DESCRIPTOR_HANDLE*,bool);
    uintptr_t table{};ID3D12GraphicsCommandList* list{};
    if(!CurrentContext(base,table,list)){++debug[3];return nullptr;}
    const auto bind=reinterpret_cast<Bind>(base+0x336D3C);
    const auto& own=stores->uav[side];
    const auto& other=stores->uav[side^1];
    bind(table,stage,1,kOwnSlot,&own,true);
    bind(table,stage,1,kOtherSlot,&other,true);
    // A slot the binder did not take would leave the twins' slots pointing at whatever the table held
    // before; they must then not see the tag.
    if(BoundUav(table,kOwnSlot,stage)!=own.ptr || BoundUav(table,kOtherSlot,stage)!=other.ptr) {
        ++debug[3];return nullptr;
    }
    // This eye's trace writes its store after the other eye's trace read it (and reads the other's after
    // it was written): order both against the previous dispatches that touched them.
    cvr::stereo::probe::Scope internal;
    D3D12_RESOURCE_BARRIER barriers[2]{};
    for(int i=0;i<2;++i){barriers[i].Type=D3D12_RESOURCE_BARRIER_TYPE_UAV;barriers[i].UAV.pResource=stores->buffer[i].Get();}
    list->ResourceBarrier(2,barriers);
    return stores;
}
}  // namespace

bool Prepare(Channel channel,int side,uintptr_t view,const void* src,uint32_t size,uint8_t* out,uint32_t* ship) {
    if(size!=kEngineBlockBytes || !src || !out || !ship || (channel!=kReflections && channel!=kPlanar))return false;
    uint64_t* debug=channel==kPlanar?CyberpunkVR_DebugSsprMerge:CyberpunkVR_DebugSsrMerge;
    if(side!=0 && side!=1){++debug[2];return false;}
    const auto& enabled=channel==kPlanar?CyberpunkVR_SsprMerge:CyberpunkVR_SsrMerge;
    // Engine stereo has no second view to merge with: both eyes are one view, side by side.
    if(!enabled.load(std::memory_order_relaxed) || !CyberpunkVR_AsymSsrPatch.load(std::memory_order_relaxed) ||
       !CyberpunkVR_AsymSsrPatchLoaded.load(std::memory_order_relaxed) || !cvr::detail::g_exe_base ||
       cvr::engine_stereo::Enabled())return false;
    std::memset(out,0,kBlockBytes);
    if(!CopyBlock(out,src,size)){++debug[5];return false;}
    // Row 32.zw is the pixel size in NDC, 2 / width and 2 / height: ndc = (pixel + 0.5 - row36.xy) * row32.zw - 1
    // (2.31: rows 16 and 19.zw).
    float pixel[2]{};std::memcpy(pixel,out+kPixelSizeRow*16+8,8);
    float camera[3]{};
    const double w=pixel[0]>0?2.0/pixel[0]:0,h=pixel[1]>0?2.0/pixel[1]:0;
    const auto width=uint32_t(std::lround(w)),height=uint32_t(std::lround(h));
    if(!(width>=16 && width<=16384 && height>=16 && height<=16384) || std::abs(w-width)>0.01 || std::abs(h-height)>0.01 ||
       !view || !ReadCamera(view,camera)){++debug[5];return false;}
    const uint32_t capacity=kHeaderEntries+width*height;
    const uint32_t stamp=channels[channel].stamps[side].fetch_add(1,std::memory_order_relaxed)+1;
    if(!BindStores(channel,side,capacity,debug))return false;
    uint8_t* row=out+kCameraRow*16;
    std::memcpy(row,camera,12);std::memcpy(row+12,&kTag,4);
    row=out+kStoreRow*16;
    const uint32_t info[4]{stamp,width,height,capacity};std::memcpy(row,info,16);
    row=out+kModeRow*16;
    const uint32_t mode=CyberpunkVR_SsrMergeMode;float tolerance=CyberpunkVR_SsrMergeTolerance;
    if(!std::isfinite(tolerance) || tolerance<0)tolerance=0;
    std::memcpy(row,&mode,4);std::memcpy(row+4,&tolerance,4);
    *ship=kBlockBytes;
    ++debug[side];
    debug[6]=capacity;debug[7]=(uint64_t(width)<<16)|height;
    std::memcpy(channel==kPlanar?CyberpunkVR_DebugSsprBlocks[side]:CyberpunkVR_DebugSsrBlocks[side],out,kBlockBytes);
    return true;
}

bool PrepareConeAo(int side,uint8_t* block,uint32_t size) {
    uint64_t* debug=CyberpunkVR_DebugAoMerge;
    if(size!=kAoBlockBytes || !block)return false;
    if(side!=0 && side!=1){++debug[2];return false;}
    if(!CyberpunkVR_AoMerge.load(std::memory_order_relaxed) || !CyberpunkVR_AsymAoPatch.load(std::memory_order_relaxed) ||
       !CyberpunkVR_AsymAoPatchLoaded.load(std::memory_order_relaxed) || !cvr::detail::g_exe_base ||
       cvr::engine_stereo::Enabled())return false;
    // The header is the block's own view->world rows and centred tangents: without the centre the twins'
    // reconstruction is not the view space those rows describe.
    uint32_t centreTag{};std::memcpy(&centreTag,block+kAoCentreRow*16+12,4);
    float dims[2]{};std::memcpy(dims,block+16+8,8);          // row 1: 1/width, 1/height, width, height
    const auto width=uint32_t(std::lround(dims[0])),height=uint32_t(std::lround(dims[1]));
    if(centreTag!=kAoCentreTag || !(width>=16 && width<=16384 && height>=16 && height<=16384) ||
       std::abs(dims[0]-float(width))>0.01f || std::abs(dims[1]-float(height))>0.01f){++debug[5];return false;}
    const uint32_t capacity=kHeaderEntries+width*height;
    if(!BindStores(kConeAo,side,capacity,debug))return false;
    const uint32_t stamp=channels[kConeAo].stamps[side].fetch_add(1,std::memory_order_relaxed)+1;
    float tolerance=CyberpunkVR_AoMergeTolerance,strength=CyberpunkVR_AoMergeStrength;
    if(!std::isfinite(tolerance) || tolerance<0)tolerance=0;
    if(!std::isfinite(strength) || strength<0)strength=0;
    uint8_t* row=block+kAoMergeRow*16;
    std::memcpy(row,&stamp,4);std::memcpy(row+4,&kAoTag,4);std::memcpy(row+8,&tolerance,4);std::memcpy(row+12,&strength,4);
    const uint32_t mode=CyberpunkVR_AoMergeMode;std::memcpy(block+kAoModeRow*16+8,&mode,4);
    ++debug[side];
    debug[6]=capacity;debug[7]=(uint64_t(width)<<16)|height;
    std::memcpy(CyberpunkVR_DebugAoBlocks[side],block,kAoBlockBytes);
    return true;
}

bool PrepareShadowMask(int side,uintptr_t view,uint8_t* block,uint32_t size) {
    uint64_t* debug=CyberpunkVR_DebugShadowMerge;
    if(size!=kShadowBlockBytes || !block)return false;
    if(side!=0 && side!=1){++debug[2];return false;}
    // Engine stereo has no second view to merge with, as for the reflections and the cone AO.
    if(!CyberpunkVR_ShadowMerge.load(std::memory_order_relaxed) || !CyberpunkVR_AsymShadowPatch.load(std::memory_order_relaxed) ||
       !CyberpunkVR_AsymShadowPatchLoaded.load(std::memory_order_relaxed) || !cvr::detail::g_exe_base ||
       cvr::engine_stereo::Enabled())return false;
    // The mask is drawn at half width (checkerboard); its store holds one entry per drawn pixel. The view's
    // render size is in its scene camera descriptor (+0x378 width, +0x37C height; Render/ViewCameraLayout.hpp).
    uint32_t dims[2]{};
    if(!view || !CopyBlock(reinterpret_cast<uint8_t*>(dims),reinterpret_cast<const void*>(view+cvr::view_layout::kCamera+0x378),8) ||
       !(dims[0]>=16 && dims[0]<=16384 && dims[1]>=16 && dims[1]<=16384)){++debug[5];return false;}
    const uint32_t capacity=kHeaderEntries+(dims[0]/2+1)*dims[1];
    if(!BindStores(kShadowMask,side,capacity,debug,1))return false;
    const uint32_t stamp=channels[kShadowMask].stamps[side].fetch_add(1,std::memory_order_relaxed)+1;
    const uint32_t tag=kShadowTag|(CyberpunkVR_ShadowMergeMode&15u);
    std::memcpy(block+3*16+8,&stamp,4);std::memcpy(block+3*16+12,&tag,4);
    ++debug[side];
    debug[6]=capacity;debug[7]=(uint64_t(dims[0])<<16)|dims[1];
    std::memcpy(CyberpunkVR_DebugShadowBlocks[side],block,kShadowBlockBytes);
    return true;
}

namespace {
// After the luminance image's samples went in (the adaptation's bind): a request copies the cells and the
// frame's sums to a readback buffer; a few binds later they are in CyberpunkVR_DebugExpoCells.
void SnapshotCells(ID3D12Resource* cells) {
    static ComPtr<ID3D12Resource> readback;
    static int pending=0;
    constexpr UINT64 bytes=uint64_t(2*kExposureCells)*kEntryBytes;
    if(pending>0 && --pending==0 && readback) {
        void* p{};D3D12_RANGE r{0,SIZE_T(bytes)};
        if(SUCCEEDED(readback->Map(0,&r,&p))) {
            std::memcpy(CyberpunkVR_DebugExpoCells,p,SIZE_T(bytes));
            D3D12_RANGE w{0,0};readback->Unmap(0,&w);
            ++CyberpunkVR_DebugExpoCellsSeq;
        }
    }
    if(!CyberpunkVR_DebugExpoCellsRequest || pending)return;
    CyberpunkVR_DebugExpoCellsRequest=0;
    auto* device=cvr::detail::g_game_device;
    if(!device)return;
    if(!readback) {
        D3D12_HEAP_PROPERTIES properties{};properties.Type=D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC desc{};
        desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;
        desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if(FAILED(device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback))))return;
    }
    uintptr_t table{};ID3D12GraphicsCommandList* list{};
    if(!CurrentContext(reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base),table,list))return;
    // The luminance image's dispatch used the cells as a UAV in this list, which promoted them.
    cvr::stereo::probe::Scope internal;
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition.pResource=cells;
    b.Transition.Subresource=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    b.Transition.StateBefore=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;b.Transition.StateAfter=D3D12_RESOURCE_STATE_COPY_SOURCE;
    list->ResourceBarrier(1,&b);
    list->CopyBufferRegion(readback.Get(),0,cells,0,bytes);
    std::swap(b.Transition.StateBefore,b.Transition.StateAfter);
    list->ResourceBarrier(1,&b);
    pending=8;
}
}  // namespace

bool BindExposureCells(int side,uint64_t* debug,bool snapshot) {
    if((side!=0 && side!=1) || !debug || !cvr::detail::g_exe_base)return false;
    const auto stores=BindStores(kExposure,side,2*kExposureCells,debug);   // the cells, then this frame's sums
    if(!stores)return false;
    if(snapshot)SnapshotCells(stores->buffer[side].Get());
    return true;
}

// Diagnostic: the binding layout (sub_140336D3C's table: CBV, SRV, UAV range bytes per stage) the first
// draw or dispatch of node CyberpunkVR_ProbeLayoutNode records with, to see which slots a pass's root
// signature offers. [0] count, then 4 x 14 CBV, 4 x 128 SRV, 4 x 19 UAV bytes.
void ProbeLayout(uint32_t node) {
    if(!node || node!=CyberpunkVR_ProbeLayoutNode || CyberpunkVR_DebugLayoutCount || !cvr::detail::g_exe_base)return;
    uintptr_t table{};ID3D12GraphicsCommandList* list{};
    if(!CurrentContext(reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base),table,list))return;
    __try {
        const auto layout=*reinterpret_cast<const uintptr_t*>(table+13*8);
        std::memcpy(CyberpunkVR_DebugLayoutBytes,reinterpret_cast<const void*>(layout+1107),4*14);
        std::memcpy(CyberpunkVR_DebugLayoutBytes+4*14,reinterpret_cast<const void*>(layout+1219),4*128);
        std::memcpy(CyberpunkVR_DebugLayoutBytes+4*14+4*128,reinterpret_cast<const void*>(layout+2243),4*19);
        CyberpunkVR_DebugLayoutCount=1;
    } __except(EXCEPTION_EXECUTE_HANDLER) {CyberpunkVR_DebugLayoutCount=2;}
}
}  // namespace cvr::ssr_merge
#endif  // CVR_RUST_VRIK
#endif
