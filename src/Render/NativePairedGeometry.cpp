#include "Render/NativePairedGeometry.hpp"
#include "Render/GeometryTargetCheckpoint.hpp"
#include "Render/GBufferMaterialContract.hpp"
#include "Render/StereoGeometryChain.hpp"
#include "Render/NativeStereoFamilies.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/NativeTextureRead.hpp"
#include "Render/NativeRetainedGeometry.hpp"
#include "Render/NativeEarlyDepth.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <algorithm>
#include <cstring>
#include <memory>
#include <mutex>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The probe -- its work, its lock, the cameras, the native and paired draws, the proofs and the comparison -- is
// rust/crates/cvr-render/src/paired_geometry_native.rs (the queue detours and the camera trace call it directly). Here:
// the API the native probe's host (NativeStereoProbe.cpp) calls.
extern "C" {
bool cvr_pg2_active();
bool cvr_pg2_draw(ID3D12GraphicsCommandList*,const cvr::stereo::native_probe::DrawRecord*);
void cvr_pg2_frame_boundary();
}
namespace cvr::stereo::paired_geometry {
static_assert(sizeof(native_probe::DrawRecord)==1096,"mirrored by geometry_commands.rs");
bool Active(){return cvr_pg2_active();}
bool Draw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){return cvr_pg2_draw(list,&draw);}
void FrameBoundary(){cvr_pg2_frame_boundary();}
}
#endif
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairedGeometryRequest{},CyberpunkVR_PairedGeometryState{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairedGeometryOrdinal{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairedGeometryFamily{};
__declspec(dllexport) uint64_t CyberpunkVR_PairedGeometryStats[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PairedGeometryDifferent[12]{},CyberpunkVR_PairedGeometryBytes[2]{},CyberpunkVR_PairedGeometryCameraDiagnostics[16]{};
__declspec(dllexport) uint64_t CyberpunkVR_PairedGeometryChanged[12]{};
}
namespace cvr::stereo::paired_geometry {
using Microsoft::WRL::ComPtr;
namespace {
std::mutex mutex;
std::atomic<bool> watching{};
std::atomic<uint64_t> epoch{};
struct CameraData {
    ShaderCamera bytes{};camera_upload::Context context{};
    uint64_t epoch{},view{};UINT frame{},handle{},stages{};int side=-1;
};
thread_local CameraData pendingCamera,boundCamera;
struct Plane {
    std::array<D3D12_PLACED_SUBRESOURCE_FOOTPRINT,3> layout;
    UINT slot{},plane{},rows{},width{},stride{},component{},bytes{};
};
struct Eye {
    GeometryTargetCheckpoint before,after;
    native_probe::DrawRecord draw;
    native_recording::Lease recording;
    CameraData camera;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12Resource> readback;
    std::vector<Plane> planes;
    bool nativeExecuted{},captured{},pending{},submitted{};
};
struct Work {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> mainQueue;
    ComPtr<ID3D12PipelineState> originalPipeline;
    std::shared_ptr<const families::Family> family;
    std::array<Eye,2> eyes;
    StereoGeometryChain chain;
    ComPtr<ID3D12DescriptorHeap> cameraHeap;
    UINT frame{},ordinal{},selector{};bool failed{},complete{};
};
std::unique_ptr<Work> work;
uintptr_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Read(uint64_t address,void* out,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),out,bytes,&copied) && copied==bytes;
}
void Fail(UINT reason){if(work)work->failed=true;if(!CyberpunkVR_PairedGeometryStats[14])CyberpunkVR_PairedGeometryStats[14]=reason;CyberpunkVR_PairedGeometryState=4;}
bool NativeQueue(const native_recording::Lease& lease,ID3D12CommandQueue* queue){
    uint64_t registry{},nativeQueue{};const auto base=Base();
    return base && queue && lease.registry && base<=UINT64_MAX-0x3BCDCB0 &&
        Read(base+0x3BCDCA8,&registry,8) && registry==lease.registry && registry<=UINT64_MAX-0x14104D8 &&
        Read(registry+0x14104D0,&nativeQueue,8) && nativeQueue==reinterpret_cast<uintptr_t>(queue) && queue->GetDesc().Type==D3D12_COMMAND_LIST_TYPE_DIRECT;
}
void NativeDraw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){
    probe::Scope internal;list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);
}
bool SameGeometry(const native_probe::DrawRecord& a,const native_probe::DrawRecord& b){
    if(a.pipeline!=b.pipeline || a.root!=b.root || a.indices!=b.indices || a.instances!=b.instances ||
        a.firstIndex!=b.firstIndex || a.baseVertex!=b.baseVertex || a.inputSlotMask!=b.inputSlotMask ||
        std::memcmp(&a.index,&b.index,sizeof(a.index)) || a.instanceDataBytes!=48 || b.instanceDataBytes!=48 ||
        std::memcmp(a.instanceData.data(),b.instanceData.data(),48))return false;
    for(UINT slot=0;slot<16;++slot)if(slot!=7 && (a.inputSlotMask&(1u<<slot)) &&
        std::memcmp(&a.vertices[slot],&b.vertices[slot],sizeof(a.vertices[slot])))return false;
    return !std::memcmp(&a.viewport,&b.viewport,sizeof(a.viewport)) && !std::memcmp(&a.scissor,&b.scissor,sizeof(a.scissor));
}
bool WritableTargets(ID3D12GraphicsCommandList* list,const scene_state::GeometryTargets& targets){
    if(targets.colors!=3)return false;
    for(UINT slot=0;slot<5;++slot)if(slot==4 || slot<3)if(!texture_read::ValidateWritableTarget(list,targets.resources[slot].Get(),
        slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET))return false;
    return true;
}
struct Restore {
    ID3D12GraphicsCommandList* list;const native_probe::DrawRecord& draw;scene_state::Bindings bindings;
    ComPtr<ID3D12GraphicsCommandList1> views;
    camera_upload::BindingScope cameraScope;
    bool valid{},cameraTouched{};
    Restore(ID3D12GraphicsCommandList* command,const native_probe::DrawRecord& value):list(command),draw(value){
        valid=scene_state::Snapshot(list,bindings) && bindings.resetSeen && bindings.heapsSeen &&
            bindings.root==reinterpret_cast<ID3D12RootSignature*>(draw.root) && SUCCEEDED(list->QueryInterface(IID_PPV_ARGS(&views)));
    }
    ~Restore(){
        if(!valid)return;
        if(cameraTouched && !camera_upload::EndNativePairStages(Base(),cameraScope)){
            const auto diagnostics=camera_upload::BindingDiagnostics();std::copy(diagnostics.begin(),diagnostics.end(),CyberpunkVR_PairedGeometryCameraDiagnostics);Fail(31);
        }
        // EndNativePairStages already restored the actual GPU camera table and
        // the engine's matching cursor/cache. Do not rebind an older table.
        probe::Scope internal;views->SetViewInstanceMask(UINT_MAX);
        list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));
        std::array<D3D12_CPU_DESCRIPTOR_HANDLE,8> colors{};for(UINT i=0;i<8;++i)colors[i]={draw.targets[i]};
        const D3D12_CPU_DESCRIPTOR_HANDLE depth{draw.depth};
        list->OMSetRenderTargets(draw.rtCount,draw.rtCount?colors.data():nullptr,BOOL(draw.rtContiguous),draw.depth?&depth:nullptr);
        list->RSSetViewports(1,&draw.viewport);list->RSSetScissorRects(1,&draw.scissor);
    }
};
bool RecordProof(ID3D12GraphicsCommandList* list,UINT side){
    auto& eye=work->eyes[side];const auto& expected=eye.after.Targets();const auto& initial=eye.before.Targets();UINT64 size{};
    std::vector<Plane> planes;
    for(UINT slot=0;slot<5;++slot)if(slot==4 || slot<3){
        const auto nativeDesc=expected.resources[slot]->GetDesc(),arrayDesc=work->chain.Resource(slot)->GetDesc();
        for(UINT plane=0;plane<(slot==4?2u:1u);++plane){Plane item;item.slot=slot;item.plane=plane;item.width=UINT(nativeDesc.Width);
            UINT rows[2]{};UINT64 rowBytes[2]{},total{};
            work->device->GetCopyableFootprints(&nativeDesc,plane,1,size,&item.layout[0],&rows[0],&rowBytes[0],&total);
            if(total==UINT64_MAX || !rows[0])return false;
            size=(item.layout[0].Offset+UINT64(item.layout[0].Footprint.RowPitch)*rows[0]+511)&~UINT64(511);
            work->device->GetCopyableFootprints(&arrayDesc,1-side+plane*2,1,size,&item.layout[1],&rows[1],&rowBytes[1],&total);
            if(total==UINT64_MAX || rows[0]!=rows[1] || rows[0]!=nativeDesc.Height || rowBytes[0]!=rowBytes[1] || rowBytes[0]%nativeDesc.Width)return false;
            size=(item.layout[1].Offset+UINT64(item.layout[1].Footprint.RowPitch)*rows[1]+511)&~UINT64(511);
            UINT beforeRows{};UINT64 beforeBytes{};const auto beforeDesc=initial.resources[slot]->GetDesc();
            work->device->GetCopyableFootprints(&beforeDesc,plane,1,size,&item.layout[2],&beforeRows,&beforeBytes,&total);
            if(total==UINT64_MAX || beforeRows!=rows[0] || beforeBytes!=rowBytes[0])return false;
            size=(item.layout[2].Offset+UINT64(item.layout[2].Footprint.RowPitch)*beforeRows+511)&~UINT64(511);
            item.rows=rows[0];item.stride=UINT(rowBytes[0]/nativeDesc.Width);item.bytes=slot==4 && plane?1u:4u;
            item.component=slot==4 && plane && item.stride==8?4u:0u;
            if(item.component+item.bytes>item.stride || (slot<4 && item.stride!=4))return false;planes.push_back(item);
        }
    }
    if(!size || size>512*1024*1024)return false;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=size;desc.Height=1;
    desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    probe::Scope internal;
    if(FAILED(work->device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&eye.readback))) ||
        !gpu::KeepCommandResources(list,{eye.readback.Get()}))return false;
    for(UINT slot=0;slot<5;++slot)if(slot==4 || slot<3){
        auto* native=expected.resources[slot].Get();auto* array=work->chain.Resource(slot);auto* before=initial.resources[slot].Get();
        if(!gpu::KeepCommandResources(list,{native,array,before}))return false;
        const auto state=slot==4?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET;
        std::array<D3D12_RESOURCE_BARRIER,4> barriers{};UINT count{};
        auto add=[&](ID3D12Resource* resource,UINT sub){auto& b=barriers[count++];b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={resource,sub,state,D3D12_RESOURCE_STATE_COPY_SOURCE};};
        add(native,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);add(before,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES);
        for(UINT plane=0;plane<(slot==4?2u:1u);++plane)add(array,1-side+plane*2);
        list->ResourceBarrier(count,barriers.data());
        for(const auto& plane:planes)if(plane.slot==slot)for(UINT version=0;version<3;++version){D3D12_TEXTURE_COPY_LOCATION from{},to{};
            from.pResource=version==1?array:version==2?before:native;from.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            from.SubresourceIndex=version==1?1-side+plane.plane*2:plane.plane;
            to.pResource=eye.readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=plane.layout[version];
            list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
        }
        for(UINT i=0;i<count;++i)std::swap(barriers[i].Transition.StateBefore,barriers[i].Transition.StateAfter);
        list->ResourceBarrier(count,barriers.data());
    }
    eye.planes=std::move(planes);return true;
}
void Poll(){
    if(!work || work->complete)return;
    if(!work->failed && !CyberpunkVR_PairedGeometryStats[4])return;
    for(const auto& eye:work->eyes)if(eye.nativeExecuted){
        if(!eye.submitted || !eye.fence)return;const auto done=eye.fence->GetCompletedValue();
        if(done==UINT64_MAX){Fail(41);work->complete=true;watching=false;return;}if(done<1)return;
    }
    if(!work->failed)for(UINT side=0;side<2;++side){auto& eye=work->eyes[side];
        if(!eye.readback || eye.planes.empty()){Fail(42);break;}
        void* mapped{};const D3D12_RANGE read{0,SIZE_T(eye.readback->GetDesc().Width)};
        if(FAILED(eye.readback->Map(0,&read,&mapped)) || !mapped){Fail(43);break;}
        for(const auto& plane:eye.planes){const auto output=side*6+(plane.slot==4?4+plane.plane:plane.slot);
            for(UINT y=0;y<plane.rows;++y)for(UINT x=0;x<plane.width;++x){
                const auto* a=static_cast<const uint8_t*>(mapped)+plane.layout[0].Offset+UINT64(y)*plane.layout[0].Footprint.RowPitch+x*plane.stride+plane.component;
                const auto* b=static_cast<const uint8_t*>(mapped)+plane.layout[1].Offset+UINT64(y)*plane.layout[1].Footprint.RowPitch+x*plane.stride+plane.component;
                const auto* before=static_cast<const uint8_t*>(mapped)+plane.layout[2].Offset+UINT64(y)*plane.layout[2].Footprint.RowPitch+x*plane.stride+plane.component;
                CyberpunkVR_PairedGeometryDifferent[output]+=std::memcmp(a,b,plane.bytes)!=0;CyberpunkVR_PairedGeometryBytes[side]+=plane.bytes;
                CyberpunkVR_PairedGeometryChanged[output]+=std::memcmp(a,before,plane.bytes)!=0;
            }
        }
        const D3D12_RANGE none{};eye.readback->Unmap(0,&none);
    }
    if(!work->failed && std::any_of(std::begin(CyberpunkVR_PairedGeometryDifferent),std::end(CyberpunkVR_PairedGeometryDifferent),[](auto value){return value!=0;}))Fail(44);
    if(!work->failed)CyberpunkVR_PairedGeometryState=3;work->complete=true;watching=false;
}
}
bool Active(){return watching.load(std::memory_order_relaxed) || CyberpunkVR_PairedGeometryRequest.load(std::memory_order_relaxed)!=0;}
bool NeedsCamera(){return Active();}
void FrameBoundary(){
    if(!Active() && CyberpunkVR_PairedGeometryState.load()!=2)return;
    std::lock_guard lock(mutex);Poll();
    if(work && !work->complete && !CyberpunkVR_PairedGeometryState.load()){Fail(45);Poll();}
    const auto request=CyberpunkVR_PairedGeometryRequest.exchange(0);if(!request)return;
    if(work && !work->complete){Fail(1);return;}
    work=std::make_unique<Work>();work->ordinal=CyberpunkVR_PairedGeometryOrdinal.load();work->selector=CyberpunkVR_PairedGeometryFamily.load();epoch.fetch_add(1,std::memory_order_release);
    std::fill_n(CyberpunkVR_PairedGeometryStats,16,0ull);std::fill_n(CyberpunkVR_PairedGeometryDifferent,12,0ull);
    std::fill_n(CyberpunkVR_PairedGeometryChanged,12,0ull);
    std::fill_n(CyberpunkVR_PairedGeometryBytes,2,0ull);std::fill_n(CyberpunkVR_PairedGeometryCameraDiagnostics,16,0ull);CyberpunkVR_PairedGeometryStats[15]=request;
    if(request!=1 || work->ordinal>=64 || (work->selector && work->selector!=70) || !native_probe::ObserveBindings() || !native_recording::Compatible() || retained_geometry::Active() ||
        early_depth::Drawing() || early_depth::RunTracking()){Fail(2);work->complete=true;return;}
    watching=true;CyberpunkVR_PairedGeometryState=1;
}
void Camera(const trace::Record& row){
    if(probe::internalCommands || !NeedsCamera())return;pendingCamera={};boundCamera={};
    if(row.side<0 || row.side>1 || row.node!=0x33DDA4 || !row.inputValid || row.index==UINT_MAX || !row.index ||
        row.view!=CyberpunkVR_PreparationPairData[row.side?3:4])return;
    pendingCamera.bytes=row.camera;pendingCamera.epoch=epoch.load(std::memory_order_acquire);pendingCamera.view=row.view;
    pendingCamera.frame=CyberpunkVR_PreparationPairMerged.load();pendingCamera.side=row.side;pendingCamera.handle=row.index;
}
void BoundCamera(int side,uint32_t binding,uint32_t handle,uint32_t stages){
    if(probe::internalCommands || !NeedsCamera() || binding!=1 || !(stages&3))return;
    const auto context=camera_upload::CurrentContext(Base());
    if(!context || pendingCamera.epoch!=epoch.load(std::memory_order_acquire) || pendingCamera.side!=side || pendingCamera.handle!=handle){boundCamera={};return;}
    if(boundCamera.handle!=handle || boundCamera.context!=context || boundCamera.epoch!=pendingCamera.epoch){boundCamera=pendingCamera;boundCamera.context=context;boundCamera.stages=0;}
    boundCamera.stages|=stages&3;
}
bool Draw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw){
    if(probe::internalCommands || !Active())return false;
    const auto tag=preparation::CurrentDrawTag();std::lock_guard lock(mutex);bool handled=false;
    try{
        if(!work || work->complete || work->failed || !list || draw.side>1 || tag.side!=int(draw.side) || tag.stage!=7 || !tag.scope || !tag.frame || retained_geometry::InRenderPass(list))return false;
        const auto side=draw.side;auto& eye=work->eyes[side];if(eye.nativeExecuted)return false;
        const auto family=families::Bound(list);
        const auto cameraStages=work->selector?3u:1u;
        const bool shader=family && (work->selector?
            family->vertexSha256=="84b48dd0ebdd29fbaf7895d84acf3e3030eb265d9423fc92dbb6cb56a41df63f" &&
            family->pixelSha256=="0cf4b32c82315ba36b2ae78b957274a4cf65577d4d76ff126238d75c833df5b2":
            gbuffer_material::MatchesShaders(family->vertexSha256,family->pixelSha256));
        if(!shader || !family->nativeCameraPipeline || family->nativeCameraStages!=cameraStages || families::NativeGeometryStatus(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline))!=1 || family->sourceRoot.Get()!=reinterpret_cast<ID3D12RootSignature*>(draw.root) ||
            (!work->selector && draw.indices!=6) || draw.indices<3 || draw.indices%3 || draw.instances!=1 || draw.vertices[7].StrideInBytes!=48 || draw.instanceDataBytes!=48 || draw.rtCount!=3 ||
            (draw.flags&255)!=191 || !draw.depth || !draw.viewport.Width || !draw.viewport.Height || draw.viewport.TopLeftX || draw.viewport.TopLeftY)return false;
        ++CyberpunkVR_PairedGeometryStats[side];
        if(!side && CyberpunkVR_PairedGeometryStats[0]<=work->ordinal)return false;
        if(side && (!work->eyes[0].captured || work->frame!=tag.frame || family!=work->family || !SameGeometry(work->eyes[0].draw,draw)))return false;
        if(!side && work->frame)return false;
        if(boundCamera.epoch!=epoch.load(std::memory_order_acquire) || boundCamera.side!=int(side) ||
            (boundCamera.stages&cameraStages)!=cameraStages || boundCamera.context!=camera_upload::CurrentContext(Base()) || boundCamera.context.list!=reinterpret_cast<uintptr_t>(list)){Fail(3);return false;}
        scene_state::GeometryTargets targets;if(!scene_state::CaptureGeometryTargets(draw,targets) || !WritableTargets(list,targets)){Fail(4);return false;}
        if(!native_recording::Capture(list,camera_upload::CurrentContext(Base()),eye.recording)){Fail(100+native_recording::LastFailure());return false;}
        if(side && (!work->eyes[0].submitted || !work->mainQueue || !NativeQueue(eye.recording,work->mainQueue.Get()) || eye.recording.registry!=work->eyes[0].recording.registry)){Fail(5);return false;}
        if(!side){if(FAILED(list->GetDevice(IID_PPV_ARGS(&work->device)))){Fail(6);return false;}
            work->frame=tag.frame;work->family=family;work->originalPipeline=reinterpret_cast<ID3D12PipelineState*>(draw.pipeline);
            CyberpunkVR_PairedGeometryStats[8]=UINT(draw.viewport.Width);CyberpunkVR_PairedGeometryStats[9]=UINT(draw.viewport.Height);CyberpunkVR_PairedGeometryStats[12]=family->id;}
        eye.draw=draw;eye.camera=boundCamera;
        if(FAILED(work->device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&eye.fence))) ||
            FAILED(eye.before.Create(work->device.Get(),targets)) || FAILED(eye.after.Create(work->device.Get(),targets)) || !eye.before.Capture(list,targets)){Fail(7);return false;}
        NativeDraw(list,draw);handled=true;eye.nativeExecuted=true;CyberpunkVR_PairedGeometryStats[10+side]=draw.sequence;
        if(!eye.after.Capture(list,targets)){Fail(8);return handled;}
        eye.captured=true;CyberpunkVR_PairedGeometryStats[2+side]=1;if(!side)return handled;
        Restore restore(list,draw);if(!restore.valid){Fail(9);return handled;}
        if(FAILED(work->chain.Create(work->device.Get(),UINT(draw.viewport.Width),UINT(draw.viewport.Height))) ||
            FAILED(work->chain.PrepareEye(list,1,work->eyes[0].before.Targets())) || FAILED(work->chain.PrepareEye(list,0,eye.before.Targets()))){Fail(10);return handled;}
        camera_upload::Result camera;
        restore.cameraTouched=true;
        if(!camera_upload::BeginNativePairStages(Base(),eye.camera.handle,eye.camera.bytes,work->eyes[0].camera.bytes,eye.camera.bytes,
            work->cameraHeap,camera,cameraStages,restore.cameraScope)){
            const auto diagnostics=camera_upload::BindingDiagnostics();std::copy(diagnostics.begin(),diagnostics.end(),CyberpunkVR_PairedGeometryCameraDiagnostics);Fail(11);return handled;
        }
        const auto diagnostics=camera_upload::BindingDiagnostics();std::copy(diagnostics.begin(),diagnostics.end(),CyberpunkVR_PairedGeometryCameraDiagnostics);
        if(camera.bytes!=2048 || !camera.gpuAddress || !work->cameraHeap ||
            !gpu::KeepCommandResources(list,{family->nativeCameraPipeline.Get(),family->sourceRoot.Get(),work->cameraHeap.Get()})){Fail(12);return handled;}
        {probe::Scope internal;
            list->SetPipelineState(family->nativeCameraPipeline.Get());restore.views->SetViewInstanceMask(3);
            if(!work->chain.Bind(list,3,3)){Fail(13);return handled;}
            list->DrawIndexedInstanced(draw.indices,draw.instances,draw.firstIndex,draw.baseVertex,draw.firstInstance);
        }
        CyberpunkVR_PairedGeometryStats[4]=1;
        if(!RecordProof(list,0) || !RecordProof(list,1)){Fail(14);return handled;}
        CyberpunkVR_PairedGeometryState=2;return handled;
    }catch(...){Fail(15);return handled;}
}
bool BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(probe::internalCommands || !Active() || !queue || !lists)return false;
    std::lock_guard lock(mutex);if(!work || work->complete)return false;bool serialize=false;
    for(UINT side=0;side<2;++side){auto& eye=work->eyes[side];if(!eye.nativeExecuted || eye.submitted)continue;
        UINT occurrences{};for(UINT i=0;i<count;++i)occurrences+=reinterpret_cast<uintptr_t>(lists[i])==eye.recording.list;
        if(!occurrences)continue;serialize=true;eye.pending=true;
        if(occurrences!=1 || !native_recording::ValidateSubmission(eye.recording,queue) || !NativeQueue(eye.recording,queue) ||
            (side && (!work->eyes[0].submitted || queue!=work->mainQueue.Get()))){Fail(200+native_recording::LastFailure());continue;}
        if(!side)work->mainQueue=queue;CyberpunkVR_PairedGeometryStats[7]|=UINT64(1)<<side;
    }
    if(serialize)++CyberpunkVR_PairedGeometryStats[13];return serialize;
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(probe::internalCommands || !Active() || !queue || !lists)return;
    std::lock_guard lock(mutex);if(!work || work->complete)return;
    for(UINT side=0;side<2;++side){auto& eye=work->eyes[side];if(!eye.pending || eye.submitted)continue;
        if(std::none_of(lists,lists+count,[&](auto* list){return reinterpret_cast<uintptr_t>(list)==eye.recording.list;}))continue;
        if(!eye.fence || FAILED(queue->Signal(eye.fence.Get(),1)))Fail(16);
        eye.submitted=true;eye.pending=false;CyberpunkVR_PairedGeometryStats[5+side]=1;
    }
}
}
#endif
