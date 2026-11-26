#include "Stereo/RenderParity.hpp"
#include "Stereo/VrcamConfig.hpp"
#include "Render/StageReplay.hpp"
#include "Stereo/FogHistory.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Render/NativeDefinitionTracking.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Camera/CameraState.hpp"
#include "Framegen/Inputs.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Core/VrCoreShared.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/DebugGate.hpp"
#include <atomic>
#include <cstring>
#include <intrin.h>
#include <mutex>
#include <wrl/client.h>

extern "C" {
#if !CVR_RUST_VRIK   // the Rust build's original is engine_detours.rs's (the thunk reads it by symbol)
void CvrRenderGraphCacheDetour();
void* CvrRenderGraphCacheOriginal{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
__declspec(dllexport) int CyberpunkVR_RenderCachePerEye=1;
__declspec(dllexport) int CyberpunkVR_EarlyAaMode=1;
__declspec(dllexport) int CyberpunkVR_FogHistorySync=1;
__declspec(dllexport) int CyberpunkVR_SkyRadianceSync=1;
__declspec(dllexport) int CyberpunkVR_OpticsSkyRadianceSync=1;
__declspec(dllexport) float CyberpunkVR_OpticsSkyRadianceDebug[8]{};
__declspec(dllexport) uint32_t CyberpunkVR_OpticsSkyRadianceSeq{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OpticsSkyRadianceWrites{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
__declspec(dllexport) extern int CyberpunkVR_RenderParityDebugCapture;
#else
__declspec(dllexport) int CyberpunkVR_RenderParityDebugCapture=0;
#endif
// gated: cache salts / AA MAIN / AA swaps / AA misses / fog MAIN / fog applied /
// fog different frame-or-owner / fog invalid pose / fog recycled resource / fog same handle
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_RenderParityCounters[10]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RenderParityDebugSeq[3]{};
__declspec(dllexport) uint64_t CyberpunkVR_RenderParityDebug[3][10]{};
// Gated MAIN observations / VRCAM writes / unavailable-or-invalid samples.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SkyRadianceCounters[3]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SkyRadianceDebugSeq{};
__declspec(dllexport) float CyberpunkVR_SkyRadianceDebug[8]{}; // native VRCAM / MAIN source
// Publications / borrows / unavailable / incompatible / upload failure /
// camera observations / grid observations / invalid grids.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FogReprojectionCounters[8]{};
// Gated, per view (MAIN, VRCAM): the fog camera's P20, P21 and camera[51] (the NDC jitter). P2x minus
// the jitter is the projection's off-centre shift, which the borrowed fog history must keep.
__declspec(dllexport) float CyberpunkVR_FogCameraDebug[2][4]{};
// History borrows refused because the two views' froxel grids span different frusta (FogHistory.hpp).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FogFrustumMismatch{};
// Gated, per view: the volumetric-fog node's whole camera constants (212 floats) and the prepare-scene
// fog constants (396 floats) as last seen -- to tell which fields carry the frustum.
__declspec(dllexport) float CyberpunkVR_FogCameraDump[2][212]{};
__declspec(dllexport) float CyberpunkVR_FogConstantsDump[2][396]{};
#endif
}
#define CVR_PARITY_DIAGNOSTIC(...) do { if(CyberpunkVR_RenderParityDebugCapture || ::cvr::RuntimeDiagnosticsEnabled()) { __VA_ARGS__; } } while(false)

namespace cvr::stereo {
namespace {
using namespace cvr::detail;
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/render_parity.rs
struct Prepared {uint64_t name{};uint32_t frame{};bool valid{};};
thread_local Prepared prepared;
thread_local uintptr_t fogWorkContext{};
std::mutex aaMutex,fogMutex;
AaSample mainAa;
SkyRadianceSample mainSkyRadiance;
struct FogGridSample {RenderOwner owner{};uint32_t frame{};FogGrid grid{};bool valid{};};
struct FogSource {FogSample sample{};FogProjection projection{};FogGrid grid{};bool valid{};FogProjection frustum{};};
FogSource mainFog;
FogGridSample fogGrids[2];
struct FogWork {FogCamera camera{};RenderOwner owner{};uint32_t frame{},scatteringOutput{};uint64_t pose{};bool cameraValid{};};
thread_local FogWork fogWork;

bool ReadBytes(uintptr_t address,void* output,size_t size) {
    SIZE_T copied{};
    return address && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,size,&copied) && copied==size;
}

bool ReadSkyRadiance(uintptr_t view,SkyRadiance& color) {
    // Native sky storage +617E80 passes exactly viewData+440 to +618030.
    // The latter uses RGB * W to fill the sky-light LUT gains at +DAC..DB4.
    for(size_t i=0;i<color.size();++i)
        if(!ReadFloatSafe(view+0x440+i*sizeof(float),&color[i]))return false;
    return ValidSkyRadiance(color);
}
bool WriteSkyRadiance(uintptr_t view,const SkyRadiance& color) {
    __try {std::memcpy(reinterpret_cast<void*>(view+0x440),color.data(),sizeof(color));return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}

bool Owner(RenderOwner& out,uint32_t& frame) {
    auto& xr=OpenXRManager::Get();
    if(g_menuModeValue!=0 || !xr.IsSessionRunning() || xr.ExternalPoseResetPending())return false;
    RenderOwner next{};
    if(!ReadPtrSafe(reinterpret_cast<uintptr_t>(g_exe_base)+RENDERER_GLOBAL_RVA,&next.renderer) || !next.renderer ||
       !ReadU32Safe(next.renderer+0x4CC8,&frame))return false;
    next.player=cvr::roomscale::PlayerIdentity();
    next.mainCamera=g_camObjMain.load(std::memory_order_acquire);
    next.eyeCamera=g_vrcam_comp.load(std::memory_order_acquire);
    next.origin=xr.GetTrackingOriginSerial();next.eyeName=g_vrcam_ctx_key.load(std::memory_order_acquire);
    next.width=g_vrcam_view_w.load(std::memory_order_acquire);next.height=g_vrcam_view_h.load(std::memory_order_acquire);
    if(!next)return false;out=next;return true;
}
bool Allocation(uint32_t handle,FogAllocation& out) {
    // Exact resource table used by native SRV binder +1FABD0 (2.31):
    // pool + 0x2F200 + (handle-1)*0xB0; wrapper at +18, SRV at +08.
    // Native barrier +1F40DC also checks the actual D3D resource at -28.
    // Compare identities only. Never AddRef/dereference a remembered game pointer.
    if(!handle || handle>0x7fffffff)return false;
    FogAllocation value{};
    if(!ReadPtrSafe(reinterpret_cast<uintptr_t>(g_exe_base)+0x3BCDCA8,&value.pool) || !value.pool)return false;
    const auto record=value.pool+0x2F200+uintptr_t(handle-1)*0xB0;
    if(!ReadPtrSafe(record-0x28,&value.resource) || !ReadPtrSafe(record+0x18,&value.object) ||
       !ReadPtrSafe(record+8,&value.srv) || !value)return false;
    out=value;return true;
}
uint32_t FogOutput() {
    uintptr_t context{},state{};uint32_t output{};
    if(fogWorkContext && ReadPtrSafe(fogWorkContext+0x18,&context) && context &&
       ReadPtrSafe(context+0x2B78,&state) && state)ReadU32Safe(state+0x18,&output);
    return output;
}
uint64_t GraphKey(uint32_t frame,uint64_t hash,uintptr_t caller) {
    // The prepared snapshot is consumed only by its verified native call, in
    // the same render frame. Nested/other lookups keep their original key.
    if(CyberpunkVR_RenderCachePerEye && prepared.valid && frame==prepared.frame &&
       caller==reinterpret_cast<uintptr_t>(g_exe_base)+0x40A1D1 &&
       prepared.name && (prepared.name==g_vrcam_ctx_key.load(std::memory_order_acquire) ||
                         prepared.name==cvr::cname_hash("vr_optics_feed"))) {
        // Do not split the startup/menu graph before a live stereo player and
        // MAIN's AA configuration are known. Otherwise a cache entry can be
        // built from provisional feature bits and reused after initialization.
        RenderOwner owner{};uint32_t current{},mode{};bool ready=false;
        if(Owner(owner,current) && current==frame) {
            std::lock_guard lock(aaMutex);ready=mainAa.Read(owner,frame,mode);
        }
        if(ready) {
            hash=VrcamGraphHash(hash,prepared.name);
            CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[0]);
        }
    }
    // Graphs built while a graph-builder patch is active must not share cache entries with the
    // native ones, or a cached graph would hide (or keep) the patch (Render/StageReplay.cpp).
    if(caller==reinterpret_cast<uintptr_t>(g_exe_base)+0x40A1D1)hash^=cvr::stereo::stage_replay::GraphSalt();
    return hash;
}
#endif
#if !CVR_RUST_VRIK   // the Rust build registers the cache lookup, with its site check, from engine_detours.rs
bool CacheSiteMatches() {
    const uint8_t bytes[]={0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x6c,0x24,0x10,
                           0x48,0x89,0x74,0x24,0x18,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57};
    return g_exe_base && !std::memcmp(g_exe_base+GRAPH_CACHE_LOOKUP_RVA,bytes,sizeof(bytes));
}
CVR_DETOUR_IF("[render-parity] GraphCacheLookup",GRAPH_CACHE_LOOKUP_RVA,CvrRenderGraphCacheDetour,CvrRenderGraphCacheOriginal,CacheSiteMatches);
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/render_parity.rs
struct RestoreAa {
    uintptr_t address{};uint32_t saved{};
    ~RestoreAa(){if(address)WriteU32Safe(address,saved);}
};

bool BindFogCamera(const FogCamera& camera) {
    // Native upload-ring allocation owns GPU lifetime. Each recording thread
    // gets a private CPU descriptor; the engine copies it at dispatch just as
    // it does its own scratch constant descriptor. Do not overwrite b6's slot.
    static std::mutex descriptorMutex;
    static Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> heap;
    static std::atomic<ID3D12Device*> descriptorDevice{};
    static UINT stride{},next{};
    thread_local D3D12_CPU_DESCRIPTOR_HANDLE descriptor{};
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    if(!base || !g_game_device)return false;
    if(auto* device=descriptorDevice.load(std::memory_order_acquire);device && device!=g_game_device)return false;
    if(!descriptor.ptr) {
        std::lock_guard lock(descriptorMutex);
        if(next>=128)return false;
        if(!heap) {
            D3D12_DESCRIPTOR_HEAP_DESC desc{};
            desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;desc.NumDescriptors=128;
            if(FAILED(g_game_device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap))))return false;
            stride=g_game_device->GetDescriptorHandleIncrementSize(desc.Type);
            descriptorDevice.store(g_game_device,std::memory_order_release);
        }
        descriptor.ptr=heap->GetCPUDescriptorHandleForHeapStart().ptr+next++*stride;
    }
    using Current=uintptr_t(*)();
    using Upload=void(*)(uint32_t,const void*,D3D12_CPU_DESCRIPTOR_HANDLE);
    using Bind=void(*)(uintptr_t,uint8_t,uint32_t,uint32_t,const D3D12_CPU_DESCRIPTOR_HANDLE*,bool);
    const auto current=reinterpret_cast<Current>(base+0x337230)();
    uintptr_t state{};
    if(!current || !ReadPtrSafe(current+0x60,&state) || !state)return false;
    reinterpret_cast<Upload>(base+0x333B88)(sizeof(camera),camera.data(),descriptor);
    reinterpret_cast<Bind>(base+0x336D3C)(state,2,2,1,&descriptor,true);
    return true;
}

#endif
#if !CVR_RUST_VRIK
using FogSrvFn=int64_t(__fastcall*)(uint32_t,uint32_t*,uintptr_t,uint32_t,uint8_t);
#endif
#if CVR_RUST_VRIK
// The fog history bind, its original and its registration are Rust's (render_parity.rs, engine_detours.rs).
#else
FogSrvFn originalFogSrv{};
int64_t __fastcall BindFogHistory(uint32_t slot,uint32_t* handle,uintptr_t unused,uint32_t mip,uint8_t stage) {
    const auto caller=reinterpret_cast<uintptr_t>(_ReturnAddress());
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    // +61CB63 binds t1 (history) in the scattering shader. +61D0BB instead
    // resolves t0 for integration, which directly indexes current froxels.
    if(!CyberpunkVR_FogHistorySync || caller!=base+0x77C6A6 || slot!=1 || mip || stage!=2 ||
       t_current_node_work!=base+VOLUMETRIC_FOG_NODE_RVA || t_view_side!=1 || !handle || !*handle ||
       !fogWorkContext || !fogWork.cameraValid)
        return originalFogSrv(slot,handle,unused,mip,stage);
    RenderOwner owner{};uint32_t frame{};uintptr_t registryPointer{},registry{};
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!Owner(owner,frame) || owner!=fogWork.owner || frame!=fogWork.frame || !pose ||
       pose.poseId!=fogWork.pose || !ReadPtrSafe(fogWorkContext+8,&registryPointer) || !registryPointer ||
       !ReadPtrSafe(registryPointer,&registry))return originalFogSrv(slot,handle,unused,mip,stage);
    FogSource source;FogGridSample grid;
    {std::lock_guard lock(fogMutex);source=mainFog;grid=fogGrids[1];}
    if(!source.valid || !grid.valid || grid.owner!=owner || grid.frame!=frame) {
        CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_FogReprojectionCounters[2]);
        return originalFogSrv(slot,handle,unused,mip,stage);
    }
    {
        FogProjection own{};
        if(!FogFrustum(fogWork.camera,own) || !SameFogFrustum(own,source.frustum)) {
            CyberpunkVR_FogFrustumMismatch.fetch_add(1,std::memory_order_relaxed);
            return originalFogSrv(slot,handle,unused,mip,stage);
        }
    }
    FogAllocation live{},scattering{},integrated{};
    FogCamera patched{};
    if(!Allocation(source.sample.handle,live) || !Allocation(fogWork.scatteringOutput,scattering) ||
       !Allocation(FogOutput(),integrated) ||
       !CanBorrowFog(source.sample,owner,registry,frame,pose.poseId,XrDiagNowUs(),live,scattering,integrated,source.grid,grid.grid) ||
       !ApplyFogHistoryProjection(fogWork.camera,source.projection,patched)) {
        CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_FogReprojectionCounters[3]);
        return originalFogSrv(slot,handle,unused,mip,stage);
    }
    if(!BindFogCamera(patched)) {
        CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_FogReprojectionCounters[4]);
        return originalFogSrv(slot,handle,unused,mip,stage);
    }
    auto borrowed=source.sample.handle;
    CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_FogReprojectionCounters[1];++CyberpunkVR_RenderParityCounters[5]);
    return originalFogSrv(slot,&borrowed,unused,mip,stage);
}
#endif
// VRCAM's fog history only (t_view_side 1), on a bind every node makes: not installed in engine stereo.
#if !CVR_RUST_VRIK
CVR_DETOUR_IF("[render-parity] reproject MAIN fog history",0x3E05F8,BindFogHistory,originalFogSrv,cvr::engine_stereo::SecondViewWanted);
#endif
}
#if CVR_RUST_VRIK
#else
bool RenderGraphCacheObservationInstalled(){return g_cvrDetourReg_CvrRenderGraphCacheDetour.installed;}
#endif
#if CVR_RUST_VRIK
// RENDER PARITY IS RUST'S (rust/crates/cvr-render/src/render_parity.rs; CvrRenderGraphKey, which the graph-cache
// thunk calls, is defined there); the C++ names forward, and Rust is handed what it shares with the stereo module.
// Its table, the XR owner and the node's pose are built in Rust (render_parity.rs).
#else
extern "C" uint64_t CvrRenderGraphKey(uint32_t frame,uint64_t hash,uintptr_t caller,uintptr_t cache,uint32_t views,uintptr_t missed) {
    const auto key=GraphKey(frame,hash,caller);
    definition_tracking::BeforeCacheLookup(cache,frame,key,views,missed,caller);
    return key;
}
void ClearPreparedGraph(){prepared={};}
void ObservePreparedGraph(uintptr_t manager) {
    prepared={};uintptr_t views{},view{};uint32_t count{},frame{};uint64_t name{};
    if(!manager || !ReadU32Safe(manager+0x54,&count) || count!=1 ||
       !ReadPtrSafe(manager+0x48,&views) || !views || !ReadPtrSafe(views,&view) || !view ||
       !ReadU64Safe(view+0x28,&name) || !cvr::framegen::ReadRenderFrameIndex(&frame))return;
    prepared={name,frame,true};
}
int64_t ComputeFlagsWithMainAa(FlagComputeFn original,void* a1,int64_t a2,int64_t context,int64_t view,bool* ready) {
    RestoreAa restore;
    if(ready)*ready=false;
    if(context && view) {
        RenderOwner owner{};uint32_t frame{},mode{};uint64_t name{};
        const bool valid=Owner(owner,frame) && ReadU64Safe(context+0x28,&name) && ReadU32Safe(view+0xFA4,&mode) && mode<=8;
        uint32_t desired{};bool change=false;
        SkyRadiance nativeSky{},desiredSky{};bool changeSky=false;
        const bool skyValid=valid && ReadSkyRadiance(uintptr_t(view),nativeSky);
        {
            std::lock_guard lock(aaMutex);
            if(!valid){mainAa={};mainSkyRadiance={};}
            else if(IsMainAaContext(uintptr_t(context),name,g_main_view_ctx.load(std::memory_order_acquire))) {
                mainAa={owner,frame,mode,true};CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[1]);
                mainSkyRadiance={owner,nativeSky,frame,skyValid};
                CVR_PARITY_DIAGNOSTIC(if(skyValid)++CyberpunkVR_SkyRadianceCounters[0]);
            } else if(name==owner.eyeName || name==cvr::cname_hash("vr_optics_feed")) {
                const bool optics=name==cvr::cname_hash("vr_optics_feed");
                const bool available=mainAa.Read(owner,frame,desired);
                const bool apply=available && CyberpunkVR_EarlyAaMode && CyberpunkVR_StreamlineHistoryFix;
                if(ready)*ready=apply;
                change=apply && mode!=desired;
                if(!available)CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[3]);
                const bool skyAvailable=skyValid && mainSkyRadiance.Read(owner,frame,desiredSky);
                changeSky=CyberpunkVR_SkyRadianceSync && (!optics || CyberpunkVR_OpticsSkyRadianceSync) &&
                    skyAvailable && nativeSky!=desiredSky;
                if(optics) {
                    ++CyberpunkVR_OpticsSkyRadianceSeq;
                    std::memcpy(CyberpunkVR_OpticsSkyRadianceDebug,nativeSky.data(),sizeof(nativeSky));
                    std::memcpy(CyberpunkVR_OpticsSkyRadianceDebug+4,desiredSky.data(),sizeof(desiredSky));
                    ++CyberpunkVR_OpticsSkyRadianceSeq;
                    if(changeSky)++CyberpunkVR_OpticsSkyRadianceWrites;
                }
                CVR_PARITY_DIAGNOSTIC(
                    if(!skyAvailable)++CyberpunkVR_SkyRadianceCounters[2];
                    ++CyberpunkVR_SkyRadianceDebugSeq;
                    std::memcpy(CyberpunkVR_SkyRadianceDebug,nativeSky.data(),sizeof(nativeSky));
                    std::memcpy(CyberpunkVR_SkyRadianceDebug+4,desiredSky.data(),sizeof(desiredSky));
                    ++CyberpunkVR_SkyRadianceDebugSeq;
                );
            }
            CVR_PARITY_DIAGNOSTIC(
                ++CyberpunkVR_RenderParityDebugSeq[0];
                const uint64_t debug[]{uint64_t(context),uint64_t(view),g_main_view_ctx.load(),frame,mode,desired,
                    name,uint64_t(valid),owner.mainCamera,owner.eyeCamera};
                std::memcpy(CyberpunkVR_RenderParityDebug[0],debug,sizeof(debug));
                ++CyberpunkVR_RenderParityDebugSeq[0];
            );
        }
        // The later ViewReuse environment mirror runs at GPU-node dispatch.
        // That is too late: +219730 has already updated CPU sky storage, which
        // sky-modulated local lights then sample in +23DA14. Copy only this
        // radiance float4 before native preparation, without borrowing a LUT,
        // light list, camera matrix, or imposing a fixed brightness multiplier.
        if(changeSky && WriteSkyRadiance(uintptr_t(view),desiredSky))
            CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_SkyRadianceCounters[1]);
        if(change) {
            if(WriteU32Safe(view+0xFA4,desired)) {
                restore.address=view+0xFA4;restore.saved=mode;
                CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[2]);
            } else if(ready)*ready=false;
        }
    } else {std::lock_guard lock(aaMutex);mainAa={};mainSkyRadiance={};}
    return original(a1,a2,context,view);
}
uint64_t MergeViewFlags(uint64_t desired,uint64_t native,bool ready) {
    // Native bit33 is useful only when computed under the agreed AA mode.
    // A missing MAIN observation must preserve the prior complete behavior.
    return ready ? PreserveViewBit33(desired,native):desired;
}
uintptr_t SetFogWorkContext(uintptr_t context) {
    const auto previous=fogWorkContext;fogWorkContext=context;fogWork={};return previous;
}
void ObserveFogConstants(uint32_t size,const void* source) {
    if((!CyberpunkVR_FogHistorySync && !CyberpunkVR_RenderParityDebugCapture && !::cvr::RuntimeDiagnosticsEnabled()) ||
       !source || t_view_side<0 || t_view_side>1)return;
    const auto base=reinterpret_cast<uintptr_t>(g_exe_base);
    if(size!=1584 && !(size==sizeof(FogCamera) && fogWorkContext && t_current_node_work==base+VOLUMETRIC_FOG_NODE_RVA))return;
    RenderOwner owner{};uint32_t frame{};
    if(!Owner(owner,frame))return;
    if(size==1584 && t_current_node_work==base+PREPARE_SCENE_NODE_WORK_RVA) {
        std::array<uint8_t,1584> constants{};FogGrid grid{};
        const bool valid=ReadBytes(reinterpret_cast<uintptr_t>(source),constants.data(),constants.size()) && ReadFogGrid(constants.data(),grid);
        CVR_PARITY_DIAGNOSTIC(std::memcpy(CyberpunkVR_FogConstantsDump[t_view_side],constants.data(),constants.size()));
        std::lock_guard lock(fogMutex);fogGrids[t_view_side]={owner,frame,grid,valid};
        CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_FogReprojectionCounters[valid?6:7]);
    } else if(size==sizeof(FogCamera) && fogWorkContext) {
        const auto pose=cvr::camera::CurrentNodePoseIdentity();FogProjection projection{};
        fogWork.cameraValid=pose && pose.view==uint32_t(t_view_side+1) &&
            ReadBytes(reinterpret_cast<uintptr_t>(source),fogWork.camera.data(),sizeof(fogWork.camera)) &&
            FogCurrentProjection(fogWork.camera,projection);
        fogWork.owner=owner;fogWork.frame=frame;fogWork.pose=pose.poseId;
        CVR_PARITY_DIAGNOSTIC(if(fogWork.cameraValid) {
            ++CyberpunkVR_FogReprojectionCounters[5];
            const float* c=fogWork.camera.data();float* d=CyberpunkVR_FogCameraDebug[t_view_side];
            d[0]=c[34];d[1]=c[38];d[2]=c[kFogCameraJitter];d[3]=c[kFogCameraJitter+1];
            std::memcpy(CyberpunkVR_FogCameraDump[t_view_side],c,sizeof(CyberpunkVR_FogCameraDump[0]));
        });
    }
}
void SynchronizeFogHistory(uintptr_t registry,uint32_t* handle,uint32_t logicalId,uintptr_t caller) {
    if((!CyberpunkVR_FogHistorySync && !CyberpunkVR_RenderParityDebugCapture && !::cvr::RuntimeDiagnosticsEnabled()) || !handle ||
       t_current_node_work!=reinterpret_cast<uintptr_t>(g_exe_base)+VOLUMETRIC_FOG_NODE_RVA ||
       t_view_side<0 || t_view_side>1)return;
    if(caller==reinterpret_cast<uintptr_t>(g_exe_base)+0x77C609) {
        fogWork.scatteringOutput=*handle;return;
    }
    if(caller!=reinterpret_cast<uintptr_t>(g_exe_base)+0x77D01F)return;
    // Native +1F02E4 remaps the declaration's 0x49471DDA through +7674AC.
    // The resolved logical key is not constrained to that low-24-bit value.
    // Identify the consumer by its exact node AND native return address.
    RenderOwner owner{};uint32_t frame{};const auto pose=cvr::camera::CurrentNodePoseIdentity();
    const auto now=XrDiagNowUs();
    const bool valid=Owner(owner,frame) && pose && pose.view==uint32_t(t_view_side+1) &&
        pose.head.originSerial==owner.origin && fogWork.cameraValid && fogWork.owner==owner &&
        fogWork.frame==frame && fogWork.pose==pose.poseId;
    std::lock_guard lock(fogMutex);
    CVR_PARITY_DIAGNOSTIC(
        const int index=1+t_view_side;
        ++CyberpunkVR_RenderParityDebugSeq[index];
        const auto output=FogOutput();
        const uint64_t debug[]{frame,pose.poseId,registry,*handle,mainFog.sample.frame,mainFog.sample.pose,mainFog.sample.registry,
            mainFog.sample.handle,logicalId,(uint64_t(valid)<<32)|output};
        std::memcpy(CyberpunkVR_RenderParityDebug[index],debug,sizeof(debug));
        ++CyberpunkVR_RenderParityDebugSeq[index];
    );
    if(!valid) {mainFog={};CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[7]);return;}
    FogAllocation allocation{};
    if(t_view_side==0) {
        const auto& grid=fogGrids[0];FogProjection projection{},frustum{};
        if(grid.valid && grid.owner==owner && grid.frame==frame && Allocation(*handle,allocation) &&
           FogCurrentProjection(fogWork.camera,projection) && FogFrustum(fogWork.camera,frustum)) {
            mainFog={{owner,allocation,registry,pose.poseId,now,frame,*handle},projection,grid.grid,true,frustum};
            CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[4];++CyberpunkVR_FogReprojectionCounters[0]);
        } else {mainFog={};CVR_PARITY_DIAGNOSTIC(++CyberpunkVR_RenderParityCounters[8]);}
        return;
    }
    // Never replace the integration input. It belongs to this view's current
    // grid; MAIN sharing is applied only to t1 of the reprojecting shader above.
}
#endif
}
