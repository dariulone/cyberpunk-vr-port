#include "Render/NativeEarlyDepth.hpp"
#include "Render/EarlyDepthPass.hpp"
#include "Render/NativePreparation.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/ConstantUploadRegistry.hpp"
#include "Render/NativeConstantCapture.hpp"
#include "Render/InstanceCoverage.hpp"
#include "Render/NativeEyeMask.hpp"
#include "Render/NativeDepthPreparation.hpp"
#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeMaterialBinding.hpp"
#include "Render/NativeMaterialRead.hpp"
#include "Render/NativeCommandRecording.hpp"
#include "Render/AlphaDepthContract.hpp"
#include "Render/MaterialReadbackState.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Framegen/Framegen.hpp"
#include <windows.h>
#include <mutex>
#include <optional>
#include <intrin.h>
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The early depth is Rust's (rust/crates/cvr-render/src/early_depth_native.rs: the groups, the works and
// their pool, the draws' inputs and membership, the submissions' checks, the exports; the stereo families from
// families_native.rs, the binding banks from binding_bank.rs).
namespace cvr::stereo::early_depth {
namespace {
struct CvrNedPair {PairedBinding binding;const uint8_t* eyes[2];size_t bytes[2];};
static_assert(sizeof(CvrNedPair)==48 && sizeof(geometry::BufferLease)==80 &&
    sizeof(Group)==120 && offsetof(Group,cameraScope)==24 && sizeof(DepthComparisonResult)==88,"mirrored by early_depth_native.rs");
}
}
extern "C" {
bool cvr_ned_run_tracking();
bool cvr_ned_drawing();
void cvr_ned_reset(ID3D12CommandList*);
}
namespace cvr::stereo::early_depth {
void Reset(ID3D12CommandList* list){cvr_ned_reset(list);}
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the native draw origin's question (NativeDrawOrigin.cpp's test variants)
extern "C" bool cvr_ned_drawing();
namespace cvr::stereo::early_depth {
bool Drawing(){return cvr_ned_drawing();}
}
#endif
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthRequest{},CyberpunkVR_EarlyDepthState{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthStats[16]{};
__declspec(dllexport) int32_t CyberpunkVR_EarlyDepthResult{};
__declspec(dllexport) cvr::stereo::DepthComparisonResult CyberpunkVR_EarlyDepthComparison;
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthSourceCapacity=cvr::stereo::early_depth::MaxDraws;
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthInstanceCapacity=cvr::stereo::early_depth::MaxInstanceRows;
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthSources[cvr::stereo::early_depth::MaxDraws][2]{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthCoverage[cvr::stereo::early_depth::MaxDraws][2]{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthMasks[cvr::stereo::early_depth::MaxDraws][2]{},CyberpunkVR_EarlyDepthMaskStats[14]{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthExtraCoverage[cvr::stereo::early_depth::MaxDraws][2]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthFailureData[6]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthWholeTarget{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthRejectCommit{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthOptimized[8]{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthNativeCounts[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthNativeDifferences[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthGpuTimes[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthGpuInstances[4]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthInstanceReadScope{1};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthInstanceRows{},CyberpunkVR_EarlyDepthInstanceBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthInstanceCount{},CyberpunkVR_EarlyDepthInstanceBytes{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthRunRequest{},CyberpunkVR_EarlyDepthRunState{},CyberpunkVR_EarlyDepthRunStop{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthRunSeq{},CyberpunkVR_EarlyDepthRunCount{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunStats[16]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunArenas[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunDeferred[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunInstanceProofs[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunCpu[10]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthRunCpuFrequency{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthRunGeometryChecks[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthRunRows[1200][16]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthRunFrameStarts[1200]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthNativeCamera{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthNativeCameraStats[3]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunNativeCamera[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthCameraStages{1};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthCameraStageMasks[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthCameraBindingInfo[16]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunCameraStageMasks[4]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthSkinning{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthSkinningStats[6]{},CyberpunkVR_EarlyDepthShaderProofs[4]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunSkinningStats[6]{},CyberpunkVR_EarlyDepthRunShaderProofs[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthBindingChecks[4]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthRecording[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthRecordingDetails[8]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthMaterialBindings[12]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthMaterialCapture{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthMaterialRows{},CyberpunkVR_EarlyDepthMaterialBlob{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthMaterialCount{},CyberpunkVR_EarlyDepthMaterialBytes{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthMaterialRowBytes=29*sizeof(uint64_t);
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthMaterialHintCount{};
__declspec(dllexport) uint32_t CyberpunkVR_EarlyDepthMaterialHints[128][2]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthMaterialDescriptors{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthAlphaReference{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthAlphaOptimized{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_EarlyDepthSkinnedAlpha{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthSkinnedAlphaStats[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthAlphaStats[6]{};
__declspec(dllexport) uint64_t CyberpunkVR_EarlyDepthAlphaProofs[6]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_EarlyDepthRunBindingChecks[4]{};
}
namespace cvr::stereo::early_depth {
namespace {
struct CameraData {uint32_t frame{},handle{};int side=-1;uint64_t view{};std::array<uint8_t,848> own{},peer{};bool prepared{};};
thread_local CameraData pendingCamera,boundCamera;
thread_local Group group;
std::mutex mutex;uint64_t nextGroup{};
class CpuScope {
    UINT phase;int64_t begin{};
public:
    explicit CpuScope(UINT value):phase(value){if(RunTracking()){LARGE_INTEGER now{};QueryPerformanceCounter(&now);begin=now.QuadPart;}}
    void End(){if(begin){LARGE_INTEGER now{};QueryPerformanceCounter(&now);
        ++CyberpunkVR_EarlyDepthRunCpu[phase*2];CyberpunkVR_EarlyDepthRunCpu[phase*2+1]+=uint64_t(now.QuadPart-begin);begin=0;}}
    ~CpuScope(){End();}
};
struct SkinInputs {
    bool alpha{};
    std::array<uint8_t,448> modifiers{};
    descriptors::SrvBinding bones,deformation;
    texture_read::Lease texture;
    std::array<descriptors::DescriptorStamp,4> descriptors;
};
struct AlphaInputs {
    bool skinned{};
    material_binding::Lease material;
    std::array<uint32_t,4> modifiers{};
    descriptors::DescriptorStamp vertexMaterial,modifierBinding;
    descriptors::SrvBinding dither;
    descriptors::SamplerBinding sampler;
    descriptors::SrvBinding image;
    texture_read::Lease texture;
    UINT expectedIndex=UINT_MAX;
    std::vector<UINT> mips;
};
struct Item {
    native_probe::DrawRecord draw;scene_state::Bindings bindings;
    std::vector<uint8_t> instances;
    std::array<uint8_t,112> material{};
    std::array<uint8_t,848> peer{};
    std::vector<bool> coverage;
    std::vector<uint8_t> masks;
    bool matched{},gpuIdentity{};
    std::optional<SkinInputs> skin;
    std::optional<AlphaInputs> alpha;
};
struct Work {
    uint64_t beginQpc{};
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> cameraHeap;
    camera_upload::Result nativeCamera;
    bool cameraChecked{};uint64_t nativeCameraDraws{};
    bool alphaReference{},alphaOptimized{},skinnedAlpha{},alphaValid=true;
    std::array<native_recording::Lease,2> nativeRecordings;
    std::array<bool,2> nativeRecordingSubmitted{};
    std::array<uint64_t,4> cameraStageMasks{};
    struct InstanceRow {uint64_t sequence{};uint32_t side{},offset{},bytes{},stride{};std::array<uint8_t,112> material{};};
    static_assert(sizeof(InstanceRow)==136);
    EarlyDepthPass pass;std::vector<Item> items;
    std::vector<InstanceRow> instanceRows;std::vector<uint8_t> instanceBlob;
    packets::EyeMembership membership,gpuMembership;uint64_t renderer{},gpuInstanceResource{};
    uint32_t frame{},limit{};uint64_t mainGroup{},peerGroup{};bool mainDone{};
    bool whole{},created{},combinedInstanceRead{};uint32_t mode{};ID3D12GraphicsCommandList* mainList{};
    ID3D12GraphicsCommandList* peerList{};
    uint32_t status{};std::array<uint32_t,16> finalStats{};
    std::array<uint64_t,6> skinStats{};
    struct BindingCheck {
        ID3D12GraphicsCommandList* list{};UINT side{};SkinInputs skin;std::array<uint8_t,112> material{};
        geometry::BufferLease instanceSource;uint64_t instanceOffset{};std::vector<uint8_t> instances;bool submitted{};
    };
    std::vector<BindingCheck> bindingChecks;
    std::array<uint64_t,4> bindingStats{};bool bindingsValid=true;
    struct AlphaCheck {ID3D12GraphicsCommandList* list{};AlphaInputs inputs;std::array<uint8_t,112> vertexMaterial{};bool submitted{};};
    std::vector<AlphaCheck> alphaChecks;
    struct MaterialRow {
        uint64_t sequence{},frame{},side{},family{},resource{},handle{},kind{},descriptor{},address{},sourceOffset{},heapType{};
        uint64_t nativeStatus{},nativeState{},stage{},failure{},payloadOffset{},payloadBytes{};
        std::array<uint64_t,4> failureDetails{};
        uint64_t alphaGateBits{},ditherScaleBits{},ditherBiasBits{},ditherSelector{},controlMask{};
        uint64_t hintedIndex=UINT_MAX,descriptorMask{},textureConfirmed{};
    };
    static_assert(sizeof(MaterialRow)==29*sizeof(uint64_t));
    struct MaterialCopy {
        ID3D12GraphicsCommandList* list{};material_binding::Lease lease;
        Microsoft::WRL::ComPtr<ID3D12Resource> permit;void* mapped{};
        material_read::ReadbackState state;
        struct Control {descriptors::DescriptorStamp stamp;uint64_t address{};UINT bytes{};std::array<uint32_t,4> value{};};
        std::vector<Control> controls;
        std::array<descriptors::SrvBinding,2> textures;
        descriptors::SamplerBinding sampler;
        ~MaterialCopy(){if(mapped){const D3D12_RANGE written{0,8};permit->Unmap(0,&written);}}
    };
    Microsoft::WRL::ComPtr<ID3D12Resource> materialReadback;
    bool materialCapture{};
    std::vector<MaterialRow> materialRows;
    std::vector<std::unique_ptr<MaterialCopy>> materialCopies;
    std::vector<uint8_t> materialBlob;
    std::vector<std::array<UINT,2>> materialHints;
    std::vector<std::array<descriptors::Value,3>> materialDescriptors;
};
std::atomic<bool> materialWatching{};
std::unique_ptr<Work> work;
std::vector<std::unique_ptr<Work>> pool;
uint32_t runBudget{},lastRunFrame=UINT32_MAX;
void RunFail(uint32_t reason){
    if(CyberpunkVR_EarlyDepthRunState.load()!=1)return;
    if(!CyberpunkVR_EarlyDepthRunStats[15])CyberpunkVR_EarlyDepthRunStats[15]=reason;
    CyberpunkVR_EarlyDepthRunStop=1;CyberpunkVR_PreparationRunStop=1;
}
bool Read(uint64_t p,void* out,size_t bytes){SIZE_T copied{};return p && p<=UINT64_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(p),out,bytes,&copied) && copied==bytes;}
bool CaptureMaterial(Work& value,ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,
                     const families::Family& family,material_binding::Lease lease){
    constexpr UINT capacity=128,stride=512;
    if(value.materialRows.size()>=capacity || (list!=value.mainList && list!=value.peerList))return false;
    value.materialRows.reserve(capacity);value.materialCopies.reserve(capacity);value.materialDescriptors.reserve(capacity);
    auto copy=std::make_unique<Work::MaterialCopy>();copy->list=list;copy->lease=std::move(lease);
    UINT hint=UINT_MAX;uint64_t descriptorMask{};std::array<descriptors::Value,3> descriptorValues{};
    for(const auto& item:value.materialHints)if(item[0]==copy->lease.native.handle){hint=item[1];break;}
    auto texture=[&](UINT slot,UINT space,UINT reg){UINT parameter{},offset{};
        if(descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_SRV,space,reg,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) &&
            descriptors::ResolveSrvAtDraw(list,draw.root,parameter,offset,copy->textures[slot])){
            descriptorMask|=uint64_t(1)<<slot;descriptorValues[slot]=copy->textures[slot].description;
        }
    };
    if(hint!=UINT_MAX)texture(0,1,hint);
    texture(1,0,67);
    UINT samplerParameter{},samplerOffset{};
    if(descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,0,0,D3D12_SHADER_VISIBILITY_PIXEL,samplerParameter,samplerOffset) &&
        descriptors::ResolveSamplerAtDraw(list,draw.root,samplerParameter,samplerOffset,copy->sampler)){
        descriptorMask|=4;descriptorValues[2]=copy->sampler.stamp.value;
    }
    uint64_t controlMask{};std::array<uint32_t,4> modifiers{},selector{};
    for(const auto& location:family.pairedRoot->plan.bindings){const auto& binding=location.binding;
        if(binding.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV || binding.visibility!=D3D12_SHADER_VISIBILITY_PIXEL ||
            (binding.sourceRegister!=0 && binding.sourceRegister!=7))continue;
        const UINT offset=binding.sourceRegister==0?452:0,bytes=binding.sourceRegister==0?4:16;
        descriptors::CbvBinding cbv;Work::MaterialCopy::Control control;std::array<uint32_t,4> again{};
        if(!descriptors::ResolveCbv(list,draw.root,location.sourceParameter,location.sourceOffset,cbv) ||
            cbv.bytes<offset+bytes || cbv.address>UINT64_MAX-offset ||
            !descriptors::CaptureDescriptor(list,draw.root,location.sourceParameter,location.sourceOffset,control.stamp) ||
            control.stamp.value[4]!=cbv.address || control.stamp.value[5]!=cbv.bytes)continue;
        control.address=cbv.address+offset;control.bytes=bytes;
        if(!constants::ReadFixedUpload(control.address,control.value.data(),bytes) ||
            !constants::ReadFixedUpload(control.address,again.data(),bytes) || control.value!=again)continue;
        if(binding.sourceRegister==7){modifiers=control.value;controlMask|=1;}else{selector=control.value;controlMask|=2;}
        copy->controls.push_back(control);
    }
    Microsoft::WRL::ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return false;
    probe::Scope internal;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=capacity*stride;
    desc.Height=1;desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if(!value.materialReadback && FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
        D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value.materialReadback))))return false;
    heap.Type=D3D12_HEAP_TYPE_UPLOAD;desc.Width=8;
    if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&copy->permit))))return false;
    const D3D12_RANGE none{};
    if(FAILED(copy->permit->Map(0,&none,&copy->mapped)) || !copy->mapped)return false;
    *static_cast<volatile uint64_t*>(copy->mapped)=0;_mm_sfence();
    if(!gpu::KeepCommandResources(list,{copy->lease.source.resource.Get(),value.materialReadback.Get(),copy->permit.Get()}))return false;
    const auto state=geometry::ReadStateAtDraw(list,copy->lease.source);
    const auto slot=UINT(value.materialRows.size());auto& source=copy->lease;
    Work::MaterialRow row{draw.sequence,draw.frame,draw.side,family.asset,reinterpret_cast<uintptr_t>(source.source.resource.Get()),
        source.native.handle,source.native.record.kind,source.binding.descriptor,source.binding.address,source.sourceOffset,
        source.source.heapType,state.status,state.value};
    row.alphaGateBits=modifiers[0];row.ditherScaleBits=modifiers[2];row.ditherBiasBits=modifiers[3];row.ditherSelector=selector[0];row.controlMask=controlMask;
    row.hintedIndex=hint;row.descriptorMask=descriptorMask;
    list->SetPredication(copy->permit.Get(),0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    const bool copied=material_read::Copy(list,source,value.materialReadback.Get(),uint64_t(slot)*stride);
    list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    if(!copied){row.failure=material_read::LastFailure();row.failureDetails=buffer_read::LastFailureDetails();copy->state.Invalidated();}
    row.stage=UINT(copy->state.State());value.materialRows.push_back(row);value.materialCopies.push_back(std::move(copy));value.materialDescriptors.push_back(descriptorValues);
    materialWatching=true;
    return row.failure!=40;
}
void SubmitMaterials(Work& value,ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists,bool after){
    if(!lists)return;
    for(size_t i=0;i<value.materialCopies.size();++i){auto& copy=*value.materialCopies[i];auto& row=value.materialRows[i];
        if(copy.state.State()==material_read::ReadbackState::Stage::Sealed)continue;
        const auto occurrences=UINT(std::count(lists,lists+count,copy.list));if(!occurrences)continue;
        if(after){copy.state.Submitted();}
        else{
            const bool owned=gpu::HasCommandResource(copy.list,copy.permit.Get());
            bool valid=material_read::ValidateSubmission(copy.lease,queue);
            for(UINT slot=0;slot<2;++slot)if((row.descriptorMask&(uint64_t(1)<<slot)) && !descriptors::MatchesDescriptor(copy.textures[slot].stamp))row.descriptorMask&=~(uint64_t(1)<<slot);
            if((row.descriptorMask&4) && !descriptors::MatchesDescriptor(copy.sampler.stamp))row.descriptorMask&=~4ull;
            for(const auto& control:copy.controls){std::array<uint32_t,4> current{},again{};
                if(!descriptors::MatchesDescriptor(control.stamp) || !constants::ReadFixedUpload(control.address,current.data(),control.bytes) ||
                    !constants::ReadFixedUpload(control.address,again.data(),control.bytes) || current!=control.value || again!=control.value){
                    row.controlMask=0;
                }
            }
            const auto update=copy.state.BeforeSubmit(occurrences,valid,owned);
            if(update!=material_read::ReadbackState::PermitUpdate::None){
                *static_cast<volatile uint64_t*>(copy.mapped)=update==material_read::ReadbackState::PermitUpdate::Enable?1:0;_mm_sfence();
            }
            if(copy.state.State()==material_read::ReadbackState::Stage::Rejected && !row.failure)row.failure=valid?103:100;
        }
        row.stage=UINT(copy.state.State());
    }
}
HRESULT SealMaterials(Work& value){
    if(value.materialCopies.empty())return S_OK;
    for(const auto& copy:value.materialCopies)if(copy->state.PendingSubmissions())return S_FALSE;
    for(const auto& copy:value.materialCopies)if(copy->state.PermitMayBeInFlight(true))return E_FAIL;
    for(auto& copy:value.materialCopies)if(copy->state.Seal(true) || copy->state.DisarmRejected(true)){
        *static_cast<volatile uint64_t*>(copy->mapped)=0;
    }
    _mm_sfence();
    bool sealed=false;
    for(size_t i=0;i<value.materialCopies.size();++i){const auto& state=value.materialCopies[i]->state;
        if(state.PermitEnabled())return E_FAIL;
        value.materialRows[i].stage=UINT(state.State());sealed|=state.State()==material_read::ReadbackState::Stage::Sealed;
    }
    if(!sealed){value.materialBlob.clear();materialWatching=false;return S_OK;}
    std::vector<uint8_t> blob;
    try{blob.resize(value.materialCopies.size()*material_binding::LogicalBytes);}catch(...){return E_OUTOFMEMORY;}
    const D3D12_RANGE range{0,value.materialCopies.size()*512};void* data{};
    const auto result=value.materialReadback->Map(0,&range,&data);if(FAILED(result))return result;
    if(!data){const D3D12_RANGE none{};value.materialReadback->Unmap(0,&none);return E_FAIL;}
    size_t bytes{};
    for(size_t i=0;i<value.materialCopies.size();++i){auto& row=value.materialRows[i];row.stage=UINT(value.materialCopies[i]->state.State());
        if(row.stage!=UINT(material_read::ReadbackState::Stage::Sealed))continue;
        row.payloadOffset=bytes;row.payloadBytes=material_binding::LogicalBytes;
        std::memcpy(blob.data()+bytes,static_cast<const uint8_t*>(data)+i*512,material_binding::LogicalBytes);
        uint32_t actualIndex{};std::memcpy(&actualIndex,blob.data()+bytes,4);
        row.textureConfirmed=(row.descriptorMask&1) && row.hintedIndex==actualIndex;
        bytes+=material_binding::LogicalBytes;
    }
    const D3D12_RANGE none{};value.materialReadback->Unmap(0,&none);blob.resize(bytes);value.materialBlob=std::move(blob);materialWatching=false;return S_OK;
}
bool PrepareMembership(uint64_t renderer,const void* span){
    CpuScope cpu(0);
    uint64_t range[2]{},peerStorage{};const auto view=CyberpunkVR_PreparationPairData[3];
    if(!Read(reinterpret_cast<uintptr_t>(span),range,sizeof(range)) || range[1]<range[0] || (range[1]-range[0])%16 ||
        (range[0]&7) || (range[1]-range[0])/16>packets::MaxSpanPackets || !view || view>UINT64_MAX-0x2C48 ||
        !Read(view+0x2C40,&peerStorage,8) || !peerStorage || peerStorage>UINT64_MAX-0x35A0)return false;
    std::array<std::vector<packets::Packet>,2> inputs;
    inputs[0].resize((range[1]-range[0])/16);
    if(!inputs[0].empty() && !Read(range[0],inputs[0].data(),inputs[0].size()*16))return false;
    eager_depth::Snapshot peer;if(!peer.Capture(peerStorage+0x34C0,Read))return false;
    for(const auto packet:peer.packets)inputs[1].push_back({packet.first,packet.second});
    for(UINT side=0;side<2;++side){
        std::vector<packets::Inputs> details,again;std::vector<uint8_t> blob,againBlob;
        const bool geometryOnly=RunTracking();
        if(!packets::ResolveLive(renderer,inputs[side],details,blob,geometryOnly) || !packets::ResolveLive(renderer,inputs[side],again,againBlob,geometryOnly) ||
            details.size()!=again.size() || blob!=againBlob || (!details.empty() && std::memcmp(details.data(),again.data(),details.size()*sizeof(packets::Inputs))))return false;
        if(geometryOnly && CyberpunkVR_EarlyDepthRunStats[0]==1){
            std::vector<packets::Inputs> full;std::vector<uint8_t> fullBlob;
            if(!packets::ResolveLive(renderer,inputs[side],full,fullBlob) || full.size()!=details.size()){++CyberpunkVR_EarlyDepthRunGeometryChecks[1];return false;}
            for(size_t i=0;i<full.size();++i)if(!packets::EqualGeometry(details[i],blob,full[i],fullBlob)){
                ++CyberpunkVR_EarlyDepthRunGeometryChecks[1];CyberpunkVR_EarlyDepthFailureData[0]=5;
                CyberpunkVR_EarlyDepthFailureData[1]=side;CyberpunkVR_EarlyDepthFailureData[2]=i;return false;
            }
            ++CyberpunkVR_EarlyDepthRunGeometryChecks[0];
        }
        CyberpunkVR_EarlyDepthMaskStats[side]=UINT(inputs[side].size());
        for(size_t i=0;i<details.size();++i){
            auto host=details[i];
            if(host.source==2 && host.instanceStride==48 && host.instanceResource && host.instanceGpuAddress){
                if(work->gpuInstanceResource && work->gpuInstanceResource!=host.instanceResource)return false;
                work->gpuInstanceResource=host.instanceResource;
                std::vector<uint8_t> identities;
                if(!packets::GpuInstanceIdentities(host.instanceResource,host.instanceGpuAddress,host.instances,48,identities))return false;
                auto gpu=host;gpu.valid|=16;gpu.instanceOffset=0;gpu.instanceBytes=UINT(identities.size());
                if(!work->gpuMembership.Add(side,inputs[side][i],gpu,identities))return false;
                continue;
            }
            if(!work->membership.Add(side,inputs[side][i],host,blob)){
            const uint64_t failure[]{1,side,i,details[i].valid,details[i].source,details[i].instances};
            std::copy_n(failure,6,CyberpunkVR_EarlyDepthFailureData);return false;
        }}
        CyberpunkVR_EarlyDepthMaskStats[2+side]=work->membership.totals[side]+work->gpuMembership.totals[side];
        CyberpunkVR_EarlyDepthMaskStats[8+side]=work->membership.unresolved[side];
    }
    CyberpunkVR_EarlyDepthMaskStats[12]=CyberpunkVR_EarlyDepthMaskStats[13]=work->membership.KnownExtras();
    work->renderer=renderer;return true;
}
void Publish(){
    CyberpunkVR_EarlyDepthMaterialRows=work?reinterpret_cast<uintptr_t>(work->materialRows.data()):0;
    CyberpunkVR_EarlyDepthMaterialBlob=work?reinterpret_cast<uintptr_t>(work->materialBlob.data()):0;
    CyberpunkVR_EarlyDepthMaterialCount=work?UINT(work->materialRows.size()):0;
    CyberpunkVR_EarlyDepthMaterialBytes=work?UINT(work->materialBlob.size()):0;
    CyberpunkVR_EarlyDepthMaterialDescriptors=work?reinterpret_cast<uintptr_t>(work->materialDescriptors.data()):0;
    CyberpunkVR_EarlyDepthInstanceRows=work?reinterpret_cast<uintptr_t>(work->instanceRows.data()):0;
    CyberpunkVR_EarlyDepthInstanceBlob=work?reinterpret_cast<uintptr_t>(work->instanceBlob.data()):0;
    CyberpunkVR_EarlyDepthInstanceCount=work?uint32_t(work->instanceRows.size()):0;
    CyberpunkVR_EarlyDepthInstanceBytes=work?uint32_t(work->instanceBlob.size()):0;
    if(work){const auto counts=work->pass.NativeCounts();const auto differences=work->pass.NativeDifferences();
        std::copy(counts.begin(),counts.end(),CyberpunkVR_EarlyDepthNativeCounts);
        std::copy(differences.begin(),differences.end(),CyberpunkVR_EarlyDepthNativeDifferences);}
    if(work){const auto optimized=work->pass.OptimizedCounts();std::copy(optimized.begin(),optimized.end(),CyberpunkVR_EarlyDepthOptimized);}
    if(work){const auto times=work->pass.GpuTimes();std::copy(times.begin(),times.end(),CyberpunkVR_EarlyDepthGpuTimes);}
    if(work){const auto proof=work->pass.InstanceProofCounts();std::copy(proof.begin(),proof.end(),CyberpunkVR_EarlyDepthGpuInstances+2);}
    if(work){const auto proof=work->pass.ShaderProofCounts();std::copy(proof.begin(),proof.end(),CyberpunkVR_EarlyDepthShaderProofs);}
    if(work){const auto proof=work->pass.AlphaProofCounts();std::copy(proof.begin(),proof.end(),CyberpunkVR_EarlyDepthAlphaProofs);}
    if(work)std::copy(work->bindingStats.begin(),work->bindingStats.end(),CyberpunkVR_EarlyDepthBindingChecks);
}
void Fail(uint32_t reason,HRESULT result=E_FAIL){
    Publish();CyberpunkVR_EarlyDepthStats[15]=reason;CyberpunkVR_EarlyDepthResult=result;CyberpunkVR_EarlyDepthState=4;
    if(work)work->status=4;RunFail(reason);
}
void ClearOutput(){
    materialWatching=false;
    CyberpunkVR_EarlyDepthMaterialRows=CyberpunkVR_EarlyDepthMaterialBlob=0;
    CyberpunkVR_EarlyDepthMaterialCount=CyberpunkVR_EarlyDepthMaterialBytes=0;
    CyberpunkVR_EarlyDepthMaterialDescriptors=0;
    for(auto& value:CyberpunkVR_EarlyDepthStats)value=0;std::memset(CyberpunkVR_EarlyDepthSources,0,sizeof(CyberpunkVR_EarlyDepthSources));
    std::memset(CyberpunkVR_EarlyDepthCoverage,0,sizeof(CyberpunkVR_EarlyDepthCoverage));
    std::memset(CyberpunkVR_EarlyDepthMasks,0,sizeof(CyberpunkVR_EarlyDepthMasks));std::fill_n(CyberpunkVR_EarlyDepthMaskStats,14,0u);
    std::memset(CyberpunkVR_EarlyDepthExtraCoverage,0,sizeof(CyberpunkVR_EarlyDepthExtraCoverage));std::fill_n(CyberpunkVR_EarlyDepthFailureData,6,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthNativeCounts,8,0u);std::fill_n(CyberpunkVR_EarlyDepthNativeDifferences,4,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthOptimized,8,0ull);std::fill_n(CyberpunkVR_EarlyDepthGpuTimes,6,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthGpuInstances,4,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthNativeCameraStats,3,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthCameraStageMasks,4,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthCameraBindingInfo,16,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthSkinningStats,6,0ull);std::fill_n(CyberpunkVR_EarlyDepthShaderProofs,4,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthBindingChecks,4,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthMaterialBindings,12,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthAlphaStats,6,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthSkinnedAlphaStats,6,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthAlphaProofs,6,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthRecording,6,0ull);
    std::fill_n(CyberpunkVR_EarlyDepthRecordingDetails,8,0ull);
    CyberpunkVR_EarlyDepthComparison={};CyberpunkVR_EarlyDepthResult=S_FALSE;CyberpunkVR_EarlyDepthState=1;
}
void CompleteRunWork(Work& value){
    if(value.status!=2)return;
    DepthComparisonResult result;const auto hr=value.pass.Read(result);if(hr==S_FALSE)return;
    if(FAILED(hr)){value.status=4;RunFail(32);return;}
    const auto index=CyberpunkVR_EarlyDepthRunCount.load();if(index>=1200){value.status=4;RunFail(33);return;}
    const auto native=value.pass.NativeCounts();const auto optimized=value.pass.OptimizedCounts();const auto times=value.pass.GpuTimes();
    const auto instanceProofs=value.pass.InstanceProofCounts();
    for(UINT i=0;i<2;++i)CyberpunkVR_EarlyDepthRunInstanceProofs[i]+=instanceProofs[i];
    const auto shaderProofs=value.pass.ShaderProofCounts();
    for(UINT i=0;i<4;++i)CyberpunkVR_EarlyDepthRunShaderProofs[i]+=shaderProofs[i];
    for(UINT i=0;i<5;++i)CyberpunkVR_EarlyDepthRunSkinningStats[i]+=value.skinStats[i];
    if(value.skinStats[5])CyberpunkVR_EarlyDepthRunSkinningStats[5]=value.skinStats[5];
    for(UINT i=0;i<3;++i)CyberpunkVR_EarlyDepthRunBindingChecks[i]+=value.bindingStats[i];
    if(value.bindingStats[3])CyberpunkVR_EarlyDepthRunBindingChecks[3]=value.bindingStats[3];
    CyberpunkVR_EarlyDepthRunNativeCamera[0]+=value.nativeCamera.gpuAddress!=0;
    CyberpunkVR_EarlyDepthRunNativeCamera[1]+=value.nativeCameraDraws;
    for(UINT i=0;i<4;++i)CyberpunkVR_EarlyDepthRunCameraStageMasks[i]+=value.cameraStageMasks[i];
    const uint64_t row[]{value.frame,value.finalStats[2],value.finalStats[3],native[1],native[2],optimized[2],optimized[3],
        optimized[4],optimized[5],optimized[1],optimized[7],optimized[6],times[1]-times[0],times[2],times[4]-times[3],times[5]};
    CyberpunkVR_EarlyDepthRunSeq.fetch_add(1);
    std::copy_n(row,16,CyberpunkVR_EarlyDepthRunRows[index]);CyberpunkVR_EarlyDepthRunFrameStarts[index]=value.beginQpc;
    CyberpunkVR_EarlyDepthRunCount=index+1;
    ++CyberpunkVR_EarlyDepthRunStats[2];++CyberpunkVR_EarlyDepthRunStats[optimized[1]?3:4];
    CyberpunkVR_EarlyDepthRunStats[9]+=native[1];CyberpunkVR_EarlyDepthRunStats[10]+=native[2];
    CyberpunkVR_EarlyDepthRunStats[11]+=optimized[2]+optimized[3]+(optimized[1]?optimized[4]:optimized[5]);
    CyberpunkVR_EarlyDepthRunStats[12]+=optimized[6];value.status=3;
    CyberpunkVR_EarlyDepthRunSeq.fetch_add(1,std::memory_order_release);
}
void PollRun(){if(work)CompleteRunWork(*work);for(auto& value:pool)CompleteRunWork(*value);}
bool AcquireRunWork(){
    PollRun();
    if(CyberpunkVR_EarlyDepthRunStats[15])return false;
    if(work && work->status==1){RunFail(42);return false;}
    if(work && work->status!=3)pool.push_back(std::move(work));
    if(!work)for(auto it=pool.begin();it!=pool.end();++it)if((*it)->status==3){work=std::move(*it);pool.erase(it);break;}
    if(!work){
        if(pool.size()>=3){++CyberpunkVR_EarlyDepthRunStats[7];CyberpunkVR_EarlyDepthState=0;return false;}
        work=std::make_unique<Work>();
        CyberpunkVR_EarlyDepthRunStats[13]=std::max(CyberpunkVR_EarlyDepthRunStats[13].load(),uint64_t(pool.size()+1));
    }else{
        if(work->pass.Restart()!=S_OK){RunFail(34);return false;}
        ++CyberpunkVR_EarlyDepthRunStats[6];
        CyberpunkVR_EarlyDepthRunArenas[1]+=work->pass.ArenaCount();
        work->items.clear();work->instanceRows.clear();work->instanceBlob.clear();work->membership={};work->gpuMembership={};
        work->renderer=work->gpuInstanceResource=work->mainGroup=work->peerGroup=0;work->mainDone=false;work->mainList=work->peerList=nullptr;work->finalStats={};
        work->nativeCamera={};work->cameraChecked=false;work->nativeCameraDraws=0;work->cameraStageMasks={};work->skinStats={};
        work->bindingChecks.clear();work->bindingStats={};work->bindingsValid=true;
    }
    ++CyberpunkVR_EarlyDepthRunStats[1];return true;
}
bool Material(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,const families::Family& family,std::array<uint8_t,112>& bytes){
    if(family.pass.pixelShader || family.uses.size()!=2 || !family.pairedRoot || family.pairedRoot->plan.bindings.size()!=2)return false;
    bool camera=false,material=false;
    for(const auto& use:family.uses){if(use.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV || use.visibility!=D3D12_SHADER_VISIBILITY_VERTEX || use.space || use.count!=1)return false;
        if(use.shaderRegister==1 && use.bytes==848)camera=true;else if(use.shaderRegister==5 && use.bytes==112)material=true;else return false;}
    if(!camera || !material)return false;
    for(const auto& binding:family.pairedRoot->plan.bindings)if(binding.binding.sourceRegister==5){
        if(binding.sourceType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)return false;
        descriptors::CbvBinding value;std::array<uint8_t,112> again{};
        return descriptors::ResolveCbv(list,d.root,binding.sourceParameter,binding.sourceOffset,value) && value.bytes>=bytes.size() &&
            constants::ReadUpload(value.address,bytes.data(),bytes.size()) && constants::ReadUpload(value.address,again.data(),again.size()) && bytes==again;
    }
    return false;
}
const PairedBindingLocation* VertexBinding(const families::Family& family,D3D12_DESCRIPTOR_RANGE_TYPE type,UINT reg){
    if(!family.pairedRoot)return nullptr;
    const PairedBindingLocation* result{};
    for(const auto& entry:family.pairedRoot->plan.bindings)if(entry.binding.type==type &&
        entry.binding.visibility==D3D12_SHADER_VISIBILITY_VERTEX && entry.binding.sourceRegister==reg){
        if(result || entry.sourceType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)return nullptr;result=&entry;
    }return result;
}
bool SkinMaterial(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,const families::Family& family,
                  std::array<uint8_t,112>& material,SkinInputs& output,bool copyProof){
    const auto previousFailure=CyberpunkVR_EarlyDepthSkinningStats[5];
    CyberpunkVR_EarlyDepthSkinningStats[5]=1;
    const bool alpha=family.skinningReadContract==2 && family.alphaDepthContract==2 && family.pass.pixelShader && family.nativeCameraStages==1;
    if((!alpha && (family.skinningReadContract!=1 || family.pass.pixelShader)) || d.vertices[7].StrideInBytes!=64)return false;
    output.alpha=alpha;
    UINT mask{};
    for(const auto& use:family.uses){
        if(alpha && use.visibility==D3D12_SHADER_VISIBILITY_PIXEL)continue;
        if(use.visibility!=D3D12_SHADER_VISIBILITY_VERTEX || use.space || use.count!=1)return false;
        UINT bit{};
        if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){
            if(use.shaderRegister==1 && use.bytes==848)bit=1;
            if(use.shaderRegister==5 && use.bytes==112)bit=2;
            if(use.shaderRegister==7 && use.bytes==448)bit=4;
            if(use.shaderRegister==8 && use.bytes==512)bit=8;
        }else if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV && !use.bytes){
            if(use.shaderRegister==10)bit=16;if(use.shaderRegister==77)bit=32;
        }
        if(!bit || (mask&bit))return false;mask|=bit;
    }
    if(mask!=63)return false;
    auto constant=[&](UINT reg,std::span<uint8_t> bytes){
        const auto* binding=VertexBinding(family,D3D12_DESCRIPTOR_RANGE_TYPE_CBV,reg);descriptors::CbvBinding value;
        std::array<uint8_t,448> again{};
        auto& stamp=output.descriptors[reg==5?0:1];
        return binding && descriptors::ResolveCbv(list,d.root,binding->sourceParameter,binding->sourceOffset,value) && value.bytes>=bytes.size() &&
            descriptors::CaptureDescriptor(list,d.root,binding->sourceParameter,binding->sourceOffset,stamp) && stamp.value[0]==1 &&
            stamp.value[4]==value.address && stamp.value[5]==value.bytes &&
            constants::ReadFixedUpload(value.address,bytes.data(),bytes.size()) && constants::ReadFixedUpload(value.address,again.data(),bytes.size()) &&
            std::equal(bytes.begin(),bytes.end(),again.begin());
    };
    CyberpunkVR_EarlyDepthSkinningStats[5]=2;if(!constant(5,material) || !constant(7,output.modifiers))return false;
    CyberpunkVR_EarlyDepthSkinningStats[5]=3;if(!skinning::ZeroCull(output.modifiers))return false;
    auto resource=[&](UINT reg,descriptors::SrvBinding& out){
        const auto* binding=VertexBinding(family,D3D12_DESCRIPTOR_RANGE_TYPE_SRV,reg);
        auto& stamp=output.descriptors[reg==10?2:3];
        return binding && descriptors::ResolveSrvAtDraw(list,d.root,binding->sourceParameter,binding->sourceOffset,out) &&
            descriptors::CaptureDescriptor(list,d.root,binding->sourceParameter,binding->sourceOffset,stamp) && stamp.value==out.description;
    };
    CyberpunkVR_EarlyDepthSkinningStats[5]=4;if(!resource(10,output.bones) || !resource(77,output.deformation))return false;
    const auto& view=output.deformation.view;
    if(view.ViewDimension!=D3D12_SRV_DIMENSION_TEXTURE2D || view.Format!=DXGI_FORMAT_R8_UNORM ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || view.Texture2D.MostDetailedMip ||
        !view.Texture2D.MipLevels || view.Texture2D.PlaneSlice || view.Texture2D.ResourceMinLODClamp!=0)return false;
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    CyberpunkVR_EarlyDepthSkinningStats[5]=5;
    if(FAILED(output.bones.resource->GetHeapProperties(&heap,&flags)) ||
        (heap.Type!=D3D12_HEAP_TYPE_UPLOAD && heap.Type!=D3D12_HEAP_TYPE_GPU_UPLOAD))return false;
    // Reference mode uses the already-bound native SRV and retains native eye
    // output for exact comparison. Only replacement needs an early copy lease.
    if(copyProof && !d.side && !texture_read::Acquire(list,output.deformation.resource.Get(),output.texture)){
        CyberpunkVR_EarlyDepthSkinningStats[5]=100+texture_read::LastFailure();return false;
    }
    CyberpunkVR_EarlyDepthSkinningStats[5]=previousFailure;return true;
}
bool SameSkin(const std::optional<SkinInputs>& a,const std::optional<SkinInputs>& b){
    if(bool(a)!=bool(b))return false;if(!a)return true;
    return a->alpha==b->alpha && skinning::SameModifiers(a->modifiers,b->modifiers) && a->bones.description==b->bones.description && a->deformation.description==b->deformation.description &&
        a->bones.resource==b->bones.resource && a->deformation.resource==b->deformation.resource;
}
bool AlphaMaterial(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const families::Family& family,
                   std::array<uint8_t,112>& material,AlphaInputs& output,bool optimized){
    auto& reason=CyberpunkVR_EarlyDepthAlphaStats[5];reason=1;
    const auto stride=alpha_depth::InstanceStride(family.alphaDepthContract);
    if(!stride || !family.pass.pixelShader || family.nativeCameraStages!=1 || draw.vertices[7].StrideInBytes!=stride ||
        (family.alphaDepthContract==2 && family.skinningReadContract!=2))return false;
    output.skinned=family.alphaDepthContract==2;
    const auto* vertex=VertexBinding(family,D3D12_DESCRIPTOR_RANGE_TYPE_CBV,5);descriptors::CbvBinding cbv;std::array<uint8_t,112> again{};
    reason=2;
    if(!vertex || !descriptors::ResolveCbv(list,draw.root,vertex->sourceParameter,vertex->sourceOffset,cbv) || cbv.bytes<material.size() ||
        !descriptors::CaptureDescriptor(list,draw.root,vertex->sourceParameter,vertex->sourceOffset,output.vertexMaterial) ||
        !constants::ReadFixedUpload(cbv.address,material.data(),material.size()) || !constants::ReadFixedUpload(cbv.address,again.data(),again.size()) ||
        !(output.skinned?skinning::SameMaterial(material,again):material==again))return false;
    UINT parameter{},offset{};reason=3;
    if(!descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_CBV,0,4,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) ||
        material_binding::ResolveAtDraw(list,draw.root,parameter,offset,output.material)!=material_binding::Status::Registered)return false;
    reason=4;std::array<uint32_t,4> modifiersAgain{};
    if(!descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_CBV,0,7,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) ||
        !descriptors::ResolveCbv(list,draw.root,parameter,offset,cbv) || cbv.bytes<16 ||
        !descriptors::CaptureDescriptor(list,draw.root,parameter,offset,output.modifierBinding) ||
        !constants::ReadFixedUpload(cbv.address,output.modifiers.data(),16) || !constants::ReadFixedUpload(cbv.address,modifiersAgain.data(),16) || output.modifiers!=modifiersAgain)return false;
    reason=5;
    if(output.modifiers[2]!=0x3f800000u || (output.modifiers[3]&0x7fffffffu) ||
        !descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,67,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) ||
        !descriptors::ResolveSrvAtDraw(list,draw.root,parameter,offset,output.dither))return false;
    const auto& view=output.dither.view;const auto desc=output.dither.resource->GetDesc();
    if(view.ViewDimension!=D3D12_SRV_DIMENSION_TEXTURE2DARRAY || view.Format!=DXGI_FORMAT_R16G16B16A16_UNORM ||
        view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING || view.Texture2DArray.MostDetailedMip ||
        !view.Texture2DArray.MipLevels || view.Texture2DArray.ArraySize<64 || view.Texture2DArray.PlaneSlice ||
        desc.Width<64 || desc.Height<64 || view.Texture2DArray.FirstArraySlice>desc.DepthOrArraySize ||
        64u>UINT(desc.DepthOrArraySize)-view.Texture2DArray.FirstArraySlice)return false;
    reason=6;
    if(!descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,0,0,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) ||
        !descriptors::ResolveSamplerAtDraw(list,draw.root,parameter,offset,output.sampler))return false;
    if(optimized){
        reason=7;
        for(const auto& hint:work->materialHints)if(hint[0]==output.material.native.handle){output.expectedIndex=hint[1];break;}
        if(output.expectedIndex>=32768 || !descriptors::FindBinding(draw.root,D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,output.expectedIndex,D3D12_SHADER_VISIBILITY_PIXEL,parameter,offset) ||
            !descriptors::ResolveSrvAtDraw(list,draw.root,parameter,offset,output.image))return false;
        const auto& alpha=output.image.view;
        if(alpha.ViewDimension!=D3D12_SRV_DIMENSION_TEXTURE2D ||
            (alpha.Format!=DXGI_FORMAT_BC1_UNORM_SRGB && alpha.Format!=DXGI_FORMAT_BC7_UNORM_SRGB) ||
            alpha.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING)return false;
        if(!draw.side){
            if(!texture_read::Acquire(list,output.image.resource.Get(),output.texture)){reason=100+texture_read::LastFailure();return false;}
            Microsoft::WRL::ComPtr<ID3D12Device> device;std::vector<texture_read::MipFootprint> footprints;
            if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || !texture_read::ViewFootprints(device.Get(),output.texture,alpha,0,footprints)){
                reason=200+texture_read::LastFailure();return false;
            }
            for(const auto& footprint:footprints)output.mips.push_back(footprint.mip);
        }
    }
    reason=0;return true;
}
bool SameAlpha(const std::optional<AlphaInputs>& a,const std::optional<AlphaInputs>& b){
    if(bool(a)!=bool(b))return false;if(!a)return true;
    return a->skinned==b->skinned && a->material.native.registry==b->material.native.registry && a->material.native.handle==b->material.native.handle &&
        a->material.native.record==b->material.native.record && a->material.source.resource==b->material.source.resource &&
        a->modifiers==b->modifiers && a->dither.description==b->dither.description && a->sampler.stamp.value==b->sampler.stamp.value &&
        a->expectedIndex==b->expectedIndex && a->image.description==b->image.description && a->image.resource==b->image.resource;
}
bool SameSubmittedInstances(const Work::BindingCheck& check)try{
    if(!check.instanceSource.resource || check.instances.empty() || check.instanceOffset>check.instanceSource.bytes ||
        check.instances.size()>check.instanceSource.bytes-check.instanceOffset)return false;
    std::vector<uint8_t> current(check.instances.size());void* mapped{};
    const D3D12_RANGE range{SIZE_T(check.instanceOffset),SIZE_T(check.instanceOffset+check.instances.size())};
    if(FAILED(check.instanceSource.resource->Map(0,&range,&mapped)))return false;
    struct Unmap {ID3D12Resource* resource;~Unmap(){const D3D12_RANGE none{};resource->Unmap(0,&none);}} unmap{check.instanceSource.resource.Get()};
    const auto base=reinterpret_cast<uintptr_t>(mapped);
    if(!base || base>UINT64_MAX-check.instanceOffset)return false;
    for(UINT i=0;i<2;++i)if(!Read(base+check.instanceOffset,current.data(),current.size()) || current!=check.instances)return false;
    return true;
}catch(...){return false;}
void ValidateSubmission(Work& value,ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(value.alphaOptimized)for(UINT side=0;side<2;++side){
        const auto& recording=value.nativeRecordings[side];
        auto* list=reinterpret_cast<ID3D12CommandList*>(recording.list);
        if(!list || value.nativeRecordingSubmitted[side] || !lists || std::find(lists,lists+count,list)==lists+count)continue;
        value.nativeRecordingSubmitted[side]=true;
        if(native_recording::ValidateSubmission(recording,queue))++CyberpunkVR_EarlyDepthRecording[2+side];
        else {value.alphaValid=false;CyberpunkVR_EarlyDepthRecording[4]=native_recording::LastFailure();++CyberpunkVR_EarlyDepthAlphaStats[4];
            const auto details=native_recording::LastDetails();std::copy(details.begin(),details.end(),CyberpunkVR_EarlyDepthRecordingDetails);}
    }
    for(auto& check:value.alphaChecks){
        if(check.submitted || !lists || std::find(lists,lists+count,check.list)==lists+count)continue;
        check.submitted=true;const auto& inputs=check.inputs;bool valid=true;
        for(const auto* stamp:{&inputs.vertexMaterial,&inputs.modifierBinding,&inputs.material.stamp,&inputs.dither.stamp,&inputs.sampler.stamp})valid&=descriptors::MatchesDescriptor(*stamp);
        if(inputs.image.resource)valid&=descriptors::MatchesDescriptor(inputs.image.stamp);
        for(UINT repeat=0;repeat<2;++repeat){std::array<uint8_t,112> material{};std::array<uint32_t,4> modifiers{};
            valid&=constants::ReadFixedUpload(inputs.vertexMaterial.value[4],material.data(),material.size()) &&
                (inputs.skinned?skinning::SameMaterial(material,check.vertexMaterial):material==check.vertexMaterial);
            valid&=constants::ReadFixedUpload(inputs.modifierBinding.value[4],modifiers.data(),16) && modifiers==inputs.modifiers;
        }
        if(!valid){value.alphaValid=false;++CyberpunkVR_EarlyDepthAlphaStats[4];
            if(inputs.skinned){++CyberpunkVR_EarlyDepthSkinnedAlphaStats[4];CyberpunkVR_EarlyDepthSkinnedAlphaStats[5]=301;}}
    }
    for(auto& check:value.bindingChecks){
        if(check.submitted || !lists || std::find(lists,lists+count,check.list)==lists+count)continue;
        check.submitted=true;++value.bindingStats[check.side];uint64_t reason{};
        for(const auto& stamp:check.skin.descriptors)if(!descriptors::MatchesDescriptor(stamp)){reason=1;break;}
        if(!reason && value.mode==2 && check.skin.texture.resource && !texture_read::Validate(check.skin.texture))reason=5;
        std::array<uint8_t,112> material{},materialAgain{};std::array<uint8_t,448> modifiers{},modifiersAgain{};
        if(!reason && (!constants::ReadFixedUpload(check.skin.descriptors[0].value[4],material.data(),material.size()) ||
            !constants::ReadFixedUpload(check.skin.descriptors[0].value[4],materialAgain.data(),materialAgain.size()) ||
            !skinning::SameMaterial(material,check.material) || !skinning::SameMaterial(materialAgain,check.material)))reason=2;
        if(!reason && (!constants::ReadFixedUpload(check.skin.descriptors[1].value[4],modifiers.data(),modifiers.size()) ||
            !constants::ReadFixedUpload(check.skin.descriptors[1].value[4],modifiersAgain.data(),modifiersAgain.size()) ||
            !skinning::SameModifiers(modifiers,check.skin.modifiers) || !skinning::SameModifiers(modifiersAgain,check.skin.modifiers)))reason=3;
        if(!reason && !SameSubmittedInstances(check))reason=4;
        if(reason){value.bindingsValid=false;++value.bindingStats[2];value.bindingStats[3]=reason;
            if(check.skin.alpha){++CyberpunkVR_EarlyDepthAlphaStats[4];++CyberpunkVR_EarlyDepthSkinnedAlphaStats[4];CyberpunkVR_EarlyDepthSkinnedAlphaStats[5]=310+reason;}}
    }
    if((!value.bindingsValid || !value.alphaValid) && value.mode==2)value.pass.RejectShaderSnapshots();
}
bool SameGeometry(const Item& a,const native_probe::DrawRecord& b,const scene_state::Bindings& bindings){
    const auto& d=a.draw;
    if(d.pipeline!=b.pipeline || d.root!=b.root || d.indices!=b.indices || d.firstIndex!=b.firstIndex ||
        d.baseVertex!=b.baseVertex || d.inputSlotMask!=b.inputSlotMask || d.vertices[7].StrideInBytes!=b.vertices[7].StrideInBytes || std::memcmp(&d.index,&b.index,sizeof(d.index)) ||
        std::memcmp(&d.viewport,&b.viewport,sizeof(d.viewport)) || std::memcmp(&d.scissor,&b.scissor,sizeof(d.scissor)) ||
        a.bindings.stencil!=bindings.stencil || a.bindings.topology!=bindings.topology || a.bindings.blendFactor!=bindings.blendFactor)return false;
    for(UINT i=0;i<16;++i)if(i!=7 && (d.inputSlotMask&(1u<<i)) && std::memcmp(&d.vertices[i],&b.vertices[i],sizeof(d.vertices[i])))return false;
    return true;
}
}
bool RunTracking(){return CyberpunkVR_EarlyDepthRunState.load(std::memory_order_acquire)==1;}
bool Drawing(){return group.id && CyberpunkVR_EarlyDepthState.load(std::memory_order_relaxed)==1;}
bool MaterialPending(){return materialWatching.load(std::memory_order_relaxed);}
bool NeedsCamera(){return (RunTracking() || CyberpunkVR_EarlyDepthRequest.load(std::memory_order_relaxed) || CyberpunkVR_EarlyDepthState.load(std::memory_order_relaxed)==1) &&
    CyberpunkVR_NativeStereoProbeState.load()==1 && preparation::Pairing() && CyberpunkVR_PreparationPairState.load()==2;}
void BeforeFrameBoundary(){
    if(RunTracking() && CyberpunkVR_EarlyDepthRunStop.load())CyberpunkVR_PreparationRunStop=1;
}
void FrameBoundary(){
    if(!RunTracking() && !CyberpunkVR_EarlyDepthRunRequest.load())return;
    std::lock_guard lock(mutex);
    if(RunTracking()){
        PollRun();
        if(CyberpunkVR_NativeStereoLiveFailure.load())RunFail(35);
        if(CyberpunkVR_EarlyDepthRunStop){CyberpunkVR_PreparationRunStop=1;CyberpunkVR_EarlyDepthRunStats[14]=1;}
        if(!preparation::RunActive()){
            if(CyberpunkVR_PreparationRunFailure.load())RunFail(36);
            if((work && work->status==1) || std::any_of(pool.begin(),pool.end(),[](const auto& p){return p->status==1;}))RunFail(37);
            const bool pending=(work && work->status==2) || std::any_of(pool.begin(),pool.end(),[](const auto& p){return p->status==2;});
            if(!pending || CyberpunkVR_EarlyDepthRunStats[15]){
                if(!CyberpunkVR_EarlyDepthRunStop && (CyberpunkVR_EarlyDepthRunStats[0]!=runBudget ||
                    CyberpunkVR_EarlyDepthRunStats[1]!=CyberpunkVR_EarlyDepthRunStats[2]))RunFail(38);
                work.reset();pool.clear();CyberpunkVR_EarlyDepthState=0;Publish();
                CyberpunkVR_EarlyDepthRunState.store(CyberpunkVR_EarlyDepthRunStats[15]?3:2,std::memory_order_release);
            }
        }
        return;
    }
    if(!preparation::RunActive() && CyberpunkVR_PreparationRunState.load()!=3){
        if(!CyberpunkVR_PreparationRunRequest.load() && CyberpunkVR_PreparationRunState.load()==0)
            CyberpunkVR_PreparationRunRequest=CyberpunkVR_EarlyDepthRunRequest.load();
        return;
    }
    const auto count=CyberpunkVR_EarlyDepthRunRequest.exchange(0);
    if(!preparation::RunActive() || count<30 || count>1200 || CyberpunkVR_EarlyDepthState==1 || CyberpunkVR_EarlyDepthState==2 ||
        CyberpunkVR_EarlyDepthRequest || CyberpunkVR_EarlyDepthWholeTarget || CyberpunkVR_EarlyDepthRejectCommit ||
        CyberpunkVR_NativeStereoProbeRequest || CyberpunkVR_NativeStereoProbeState==1 ||
        CyberpunkVR_ConstantCaptureEnable || cvr::framegen::Enabled() || !descriptors::Healthy()){
        CyberpunkVR_EarlyDepthRunStats[15]=39;CyberpunkVR_EarlyDepthRunState=3;CyberpunkVR_PreparationRunStop=1;return;
    }
    work.reset();pool.clear();runBudget=count;lastRunFrame=UINT32_MAX;CyberpunkVR_EarlyDepthRunCount=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunStats)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunArenas)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunDeferred)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunInstanceProofs)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunNativeCamera)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunCameraStageMasks)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunSkinningStats)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunShaderProofs)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunBindingChecks)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunCpu)stat=0;
    for(auto& stat:CyberpunkVR_EarlyDepthRunGeometryChecks)stat=0;
    LARGE_INTEGER frequency{};QueryPerformanceFrequency(&frequency);CyberpunkVR_EarlyDepthRunCpuFrequency=frequency.QuadPart;
    CyberpunkVR_EarlyDepthRunStop=0;CyberpunkVR_NativeStereoLiveFailure=0;CyberpunkVR_EarlyDepthState=0;Publish();
    CyberpunkVR_EarlyDepthRunState.store(1,std::memory_order_release);
}
void Camera(const trace::Record& row){
    pendingCamera={};
    if(!NeedsCamera() || row.node!=0x33DDA4 || !row.inputValid || row.index==UINT32_MAX || row.side<0 || row.side>1)return;
    const auto view=CyberpunkVR_PreparationPairData[row.side?3:4];
    if(row.view!=view || (row.side==0 && (row.reserved!=12 || !row.shaderPredictionValid)))return;
    boundCamera={}; // A fresh upload must be followed by its matching native VS bind.
    pendingCamera={CyberpunkVR_PreparationPairMerged.load(),row.index,row.side,row.view,row.camera,row.shaderPrediction,row.side==0};
}
void BoundCamera(uint32_t binding,uint32_t handle,uint32_t stages){
    if(binding!=1 || !(stages&1))return;boundCamera={};
    if((stages&1) && pendingCamera.handle==handle && NeedsCamera()){boundCamera=pendingCamera;pendingCamera={};}
}
Group Begin(uint32_t node,int side,const void* renderer,const void* arguments,const void* span){
    const auto previous=group;group={};
    if(node!=0x33DDA4 || side<0 || side>1 || !NeedsCamera())return previous;
    uint32_t stage{};if(!Read(reinterpret_cast<uintptr_t>(arguments)+16,&stage,4) || stage!=3)return previous;
    std::lock_guard lock(mutex);group={++nextGroup,CyberpunkVR_PreparationPairMerged.load(),side};
    const bool running=RunTracking();
    if(side==0 && (running || CyberpunkVR_EarlyDepthRequest.load())){
        try{
            const auto request=running?AllDraws:CyberpunkVR_EarlyDepthRequest.exchange(0);
            const auto count=request==AllDraws?MaxDraws:request;
            if(running){
                if(!preparation::RunActive()){group={};return previous;}
                // A stop may arrive after the root already admitted both eyes.
                // Finish that pair; an unpaired native tail keeps the old frame ID.
                if(lastRunFrame==group.frame && (CyberpunkVR_EarlyDepthRunStop || CyberpunkVR_PreparationRunStop)){group={};return previous;}
                if(lastRunFrame==group.frame || CyberpunkVR_EarlyDepthRunStats[0]>=runBudget){RunFail(40);group={};return previous;}
                lastRunFrame=group.frame;++CyberpunkVR_EarlyDepthRunStats[0];
                if(!descriptors::Healthy()){RunFail(41);group={};return previous;}
                if(!AcquireRunWork()){group={};return previous;}
            }else{
                if(!count || count>MaxDraws || (work && CyberpunkVR_EarlyDepthState.load()==2)){Fail(1);return previous;}
                work=std::make_unique<Work>();
            }
            ClearOutput();work->status=1;work->frame=group.frame;work->limit=count;work->mainGroup=group.id;work->items.reserve(count);Publish();
            LARGE_INTEGER begin{};QueryPerformanceCounter(&begin);work->beginQpc=begin.QuadPart;
            work->combinedInstanceRead=CyberpunkVR_EarlyDepthInstanceReadScope.load()!=0;
            work->mode=running?2u:CyberpunkVR_EarlyDepthWholeTarget.load();if(work->mode>3){Fail(23);return previous;}work->whole=work->mode!=0;
            work->materialCapture=CyberpunkVR_EarlyDepthMaterialCapture.load()!=0;
            work->alphaReference=CyberpunkVR_EarlyDepthAlphaReference.load()!=0;
            work->alphaOptimized=CyberpunkVR_EarlyDepthAlphaOptimized.load()!=0;
            work->skinnedAlpha=CyberpunkVR_EarlyDepthSkinnedAlpha.load()!=0;
            if(work->alphaReference && (running || work->mode!=1 || !CyberpunkVR_EarlyDepthNativeCamera.load())){Fail(55);return previous;}
            if(work->alphaOptimized && (running || work->mode!=2 || !CyberpunkVR_EarlyDepthNativeCamera.load() || work->alphaReference)){Fail(55);return previous;}
            if(work->skinnedAlpha && (running || !CyberpunkVR_EarlyDepthSkinning.load() || (!work->alphaReference && !work->alphaOptimized))){Fail(59);return previous;}
            if(work->materialCapture && work->mode!=1){Fail(53);return previous;}
            work->materialHints.clear();const auto hintCount=CyberpunkVR_EarlyDepthMaterialHintCount.load(std::memory_order_acquire);
            if(hintCount>128){Fail(54);return previous;}
            for(UINT i=0;i<hintCount;++i){const auto* hint=CyberpunkVR_EarlyDepthMaterialHints[i];
                if(!hint[0] || hint[0]>0x100000 || hint[1]>=32768 || std::any_of(work->materialHints.begin(),work->materialHints.end(),[&](const auto& value){return value[0]==hint[0];})){
                    Fail(54);return previous;
                }
                work->materialHints.push_back({hint[0],hint[1]});
            }
            if(work->mode>=2 && cvr::framegen::Enabled()){Fail(31);return previous;}
            if(!PrepareMembership(reinterpret_cast<uintptr_t>(renderer),span)){Fail(17);return previous;}
            if(work->mode!=3 && CyberpunkVR_EarlyDepthNativeCamera.load()){
                const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));const auto stages=CyberpunkVR_EarlyDepthCameraStages.load();
                const bool ready=CyberpunkVR_StereoFamilyNativeCameras && boundCamera.frame==work->frame && boundCamera.side==0 && boundCamera.prepared;
                const bool bound=ready && (stages==1?camera_upload::BeginNative(base,boundCamera.own,boundCamera.peer,work->cameraHeap,work->nativeCamera):
                    stages==3 && camera_upload::BeginNativeStages(base,boundCamera.handle,boundCamera.own,boundCamera.peer,work->cameraHeap,work->nativeCamera,stages,group.cameraScope));
                if(!bound){
                    const auto info=camera_upload::BindingDiagnostics();std::copy(info.begin(),info.end(),CyberpunkVR_EarlyDepthCameraBindingInfo);
                    CyberpunkVR_EarlyDepthFailureData[0]=6;CyberpunkVR_EarlyDepthFailureData[1]=work->nativeCamera.reserved;
                    Fail(45);return previous;}
                if(stages==1)group.restoreCamera=boundCamera.handle;
                work->cameraStageMasks={stages,stages,0,0};std::copy_n(work->cameraStageMasks.data(),4,CyberpunkVR_EarlyDepthCameraStageMasks);
                CyberpunkVR_EarlyDepthNativeCameraStats[0]=1;
            }
        }catch(...){Fail(12,E_OUTOFMEMORY);}
    }
    if(side==1 && work && work->frame==group.frame && work->mainDone && !work->peerGroup)work->peerGroup=group.id;
    return previous;
}
void End(Group previous){
    auto ended=group;group=previous;
    if(ended.restoreCamera)camera_upload::EndNative(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)),ended.restoreCamera);
    const bool cameraRestored=camera_upload::EndNativeStages(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)),ended.cameraScope);
    if(ended.restoreCamera || ended.cameraScope.requested){
        std::lock_guard lock(mutex);
        if(work && work->mainGroup==ended.id){
            work->cameraStageMasks[2]=ended.restoreCamera?1:ended.cameraScope.restored;
            work->cameraStageMasks[3]=ended.cameraScope.preserved;
            std::copy_n(work->cameraStageMasks.data(),4,CyberpunkVR_EarlyDepthCameraStageMasks);
        }
        if(!cameraRestored){Fail(46);return;}
    }
    if(CyberpunkVR_EarlyDepthState.load()!=1)return;
    std::lock_guard lock(mutex);if(!work || !ended.id)return;
    if(ended.id==work->mainGroup){
        if(work->whole && !work->pass.FinishNative(work->mainList,0)){Fail(26);return;}
        work->mainDone=true;CyberpunkVR_EarlyDepthStats[12]=1;if(work->items.empty() && work->mode!=3 && !RunTracking())Fail(2);
    }
    if(ended.id==work->peerGroup){
        CyberpunkVR_EarlyDepthStats[13]=1;
        for(auto& item:work->items){
            item.matched=true;
            for(size_t i=0;i<item.masks.size();++i)if(item.coverage[i]!=(item.masks[i]!=2)){item.matched=false;break;}
            if(item.matched)++CyberpunkVR_EarlyDepthStats[3];
        }
        const bool complete=work->peerList && std::all_of(work->items.begin(),work->items.end(),[](const auto& i){return i.matched;});
        if(work->mode==2 && !work->pass.CommitNative(complete && work->bindingsValid && work->alphaValid && !work->items.empty() && !CyberpunkVR_EarlyDepthRejectCommit.load())){work->pass.AbortNative(work->peerList,1);Fail(30);return;}
        if(work->whole && !work->pass.FinishNative(work->peerList,1)){Fail(26);return;}
        if(!complete && !RunTracking()){Fail(3);return;}
        if(!work->pass.Readback(work->peerList)){Fail(4);return;}
        Publish();
        for(UINT i=0;i<16;++i)work->finalStats[i]=CyberpunkVR_EarlyDepthStats[i].load();work->status=2;
        std::copy_n(CyberpunkVR_EarlyDepthSkinningStats,6,work->skinStats.begin());
        CyberpunkVR_EarlyDepthState.store(2,std::memory_order_release);
    }
}
bool Draw(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& d,std::span<const uint8_t> instances){
    if(CyberpunkVR_EarlyDepthState.load()!=1 || !group.id)return false;
    CpuScope cpu(4);
    std::lock_guard lock(mutex);if(CyberpunkVR_EarlyDepthState.load()!=1 || !work || group.frame!=work->frame || d.side!=uint32_t(group.side))return false;
    if((d.side==0 && group.id!=work->mainGroup) || (d.side==1 && group.id!=work->peerGroup))return false;
    bool replaced=false;
    auto fail=[&](uint32_t reason,HRESULT result=E_FAIL){
        if(work->whole)work->pass.AbortNative(list,d.side);
        Fail(reason,result);return replaced;
    };
    auto fallback=[&]{
        if(!work->whole)return false;
        if(!work->pass.Original(list,d))return fail(25);
        return true;
    };
    try{
    const auto instanceLease=native_probe::InstanceLease(list);
    const auto instanceState=geometry::ReadStateAtDraw(list,instanceLease);
    const bool readable=instanceState.status==1 || instanceState.status==2 || instanceState.status==8 || instanceState.status==9;
    ++CyberpunkVR_EarlyDepthStats[d.side==0?0:1];
    if(work->whole){
        if(work->mode!=3 && !families::NativeDepthCompatible(reinterpret_cast<ID3D12PipelineState*>(d.pipeline))){
            CyberpunkVR_EarlyDepthFailureData[0]=3;CyberpunkVR_EarlyDepthFailureData[1]=d.sequence;CyberpunkVR_EarlyDepthFailureData[2]=d.pipeline;
            CyberpunkVR_EarlyDepthFailureData[3]=families::NativeDepthStatus(reinterpret_cast<ID3D12PipelineState*>(d.pipeline));return fail(24);
        }
        scene_state::Targets targets;
        if(!scene_state::CaptureTargets(d,targets))return fail(27);
        if(!work->created){
            Microsoft::WRL::ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return fail(5);
            const auto hr=work->pass.Create(device.Get(),UINT(d.viewport.Width),UINT(d.viewport.Height),work->mode);if(FAILED(hr))return fail(6,hr);work->created=true;
            if(RunTracking())++CyberpunkVR_EarlyDepthRunStats[5];
        }
        if(work->alphaOptimized && !work->nativeRecordings[d.side].token){
            const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
            CyberpunkVR_EarlyDepthRecording[5]=native_recording::Compatible();
            if(!native_recording::Capture(list,camera_upload::CurrentContext(base),work->nativeRecordings[d.side])){
                CyberpunkVR_EarlyDepthRecording[4]=native_recording::LastFailure();
                const auto details=native_recording::LastDetails();std::copy(details.begin(),details.end(),CyberpunkVR_EarlyDepthRecordingDetails);return fail(58);
            }
            ++CyberpunkVR_EarlyDepthRecording[d.side];
        }
        if(!work->pass.StartNative(list,d,targets.resources[3].Get(),readable?instanceLease.resource.Get():nullptr,
            D3D12_RESOURCE_STATES(instanceState.value))){
            if(texture_read::LastFailure()==40){CyberpunkVR_EarlyDepthSkinningStats[5]=240;CyberpunkVR_EarlyDepthFailureData[5]=40;}
            return fail(28);
        }
        (d.side?work->peerList:work->mainList)=list;
    }
    if(work->mode==3){if(!work->pass.ObserveNative(list,d))return fail(32);return false;}
    if(d.side==0 && work->items.size()>=work->limit)return fallback();
    if(d.rtCount || (d.flags&191)!=191 || (d.flags&64) || !(d.inputSlotMask&128) || d.inputSlotMask&~65535u){++CyberpunkVR_EarlyDepthStats[4];return fallback();}
    std::vector<uint8_t> gpuIdentities;
    const bool gpuIdentity=work->gpuInstanceResource && d.instanceResource==work->gpuInstanceResource && d.vertices[7].StrideInBytes==48;
    if(gpuIdentity){
        const auto offset=uint64_t(d.firstInstance)*48;
        if(offset>d.vertices[7].SizeInBytes || uint64_t(d.instances)*48>d.vertices[7].SizeInBytes-offset ||
            d.vertices[7].BufferLocation>UINT64_MAX-offset || !packets::GpuInstanceIdentities(d.instanceResource,
                d.vertices[7].BufferLocation+offset,d.instances,48,gpuIdentities)){++CyberpunkVR_EarlyDepthStats[4];return fallback();}
        instances=gpuIdentities;
    }
    if(instances.empty() || instances.size()!=uint64_t(d.instances)*d.vertices[7].StrideInBytes){++CyberpunkVR_EarlyDepthStats[4];return fallback();}
    const auto family=families::Bound(list);scene_state::Bindings bindings;std::array<uint8_t,112> material{};
    if((CyberpunkVR_EarlyDepthSkinning.load() || CyberpunkVR_EarlyDepthMaterialCapture.load()) && family && family->pass.pixelShader && family->pairedRoot){
        for(const auto& location:family->pairedRoot->plan.bindings){
            const auto& binding=location.binding;
            if(binding.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV || binding.visibility!=D3D12_SHADER_VISIBILITY_PIXEL || binding.sourceRegister!=4)continue;
            material_binding::Lease lease;
            const auto status=material_binding::ResolveAtDraw(list,d.root,location.sourceParameter,location.sourceOffset,lease);
            ++CyberpunkVR_EarlyDepthMaterialBindings[UINT(status)];
            if(CyberpunkVR_EarlyDepthMaterialCapture.load() && work->mode==1 && (family->asset==39 || family->asset==43) &&
                status==material_binding::Status::Registered && !CaptureMaterial(*work,list,d,*family,std::move(lease)))return fail(51);
            break;
        }
    }
    const bool alphaFamily=family && (family->alphaDepthContract==1 ||
        (work->skinnedAlpha && family->alphaDepthContract==2 && family->skinningReadContract==2 && CyberpunkVR_EarlyDepthSkinning.load()));
    const bool alphaReference=work->alphaReference && work->mode==1 && alphaFamily &&
        family->nativeCameraPipeline && family->nativeCameraStages==1 && work->nativeCamera.gpuAddress;
    const bool alphaOptimized=work->alphaOptimized && work->mode==2 && alphaFamily &&
        family->nativeCameraPipeline && family->nativeCameraStages==1 && work->nativeCamera.gpuAddress;
    if(!family || (family->pass.pixelShader && !alphaReference && !alphaOptimized) || !family->instanceOrdinalIndependent || !scene_state::Snapshot(list,bindings)){++CyberpunkVR_EarlyDepthStats[4];return fallback();}
    if(boundCamera.frame!=work->frame || boundCamera.side!=int(d.side) || !boundCamera.handle){++CyberpunkVR_EarlyDepthStats[5];return fallback();}
    std::optional<SkinInputs> skin;
    std::optional<AlphaInputs> alpha;
    const bool skinnedAlpha=(alphaReference || alphaOptimized) && family->alphaDepthContract==2;
    const auto rejectSkinned=[&](uint64_t reason){if(skinnedAlpha){++CyberpunkVR_EarlyDepthSkinnedAlphaStats[3];CyberpunkVR_EarlyDepthSkinnedAlphaStats[5]=reason;}};
    if((alphaReference || alphaOptimized) && !d.side){++CyberpunkVR_EarlyDepthAlphaStats[0];if(skinnedAlpha)++CyberpunkVR_EarlyDepthSkinnedAlphaStats[0];}
    uint64_t skinInstanceOffset{};
    if(CyberpunkVR_EarlyDepthSkinning.load() && work->nativeCamera.gpuAddress && family->nativeCameraPipeline && family->skinningReadContract){
        if(!d.side)++CyberpunkVR_EarlyDepthSkinningStats[0];skin.emplace();
        if(!SkinMaterial(list,d,*family,material,*skin,work->mode==2)){++CyberpunkVR_EarlyDepthSkinningStats[3];++CyberpunkVR_EarlyDepthStats[6];rejectSkinned(100+CyberpunkVR_EarlyDepthSkinningStats[5]);return fallback();}
        const auto relative=uint64_t(d.firstInstance)*64;
        if(!instanceLease.resource || (instanceLease.heapType!=D3D12_HEAP_TYPE_UPLOAD && instanceLease.heapType!=D3D12_HEAP_TYPE_GPU_UPLOAD) ||
            d.instanceResource!=reinterpret_cast<uintptr_t>(instanceLease.resource.Get()) || d.vertices[7].BufferLocation<instanceLease.gpu ||
            d.vertices[7].BufferLocation>UINT64_MAX-relative || relative>d.vertices[7].SizeInBytes || instances.size()>d.vertices[7].SizeInBytes-relative){
            ++CyberpunkVR_EarlyDepthSkinningStats[3];CyberpunkVR_EarlyDepthSkinningStats[5]=7;rejectSkinned(107);return fallback();
        }
        skinInstanceOffset=d.vertices[7].BufferLocation+relative-instanceLease.gpu;
        if(skinInstanceOffset>instanceLease.bytes || instances.size()>instanceLease.bytes-skinInstanceOffset){
            ++CyberpunkVR_EarlyDepthSkinningStats[3];CyberpunkVR_EarlyDepthSkinningStats[5]=7;rejectSkinned(107);return fallback();
        }
    }
    if(alphaReference || alphaOptimized){
        const auto skinMaterial=material;alpha.emplace();
        if(!AlphaMaterial(list,d,*family,material,*alpha,alphaOptimized)){
            ++CyberpunkVR_EarlyDepthAlphaStats[3];rejectSkinned(200+CyberpunkVR_EarlyDepthAlphaStats[5]);return fallback();}
        if(skinnedAlpha && (!skin || !skinning::SameMaterial(skinMaterial,material))){++CyberpunkVR_EarlyDepthAlphaStats[3];rejectSkinned(210);return fallback();}
    }else if(!skin && !Material(list,d,*family,material)){++CyberpunkVR_EarlyDepthStats[6];return fallback();}
    if(work->instanceRows.size()>=MaxInstanceRows || instances.size()>MaxInstanceBytes-work->instanceBlob.size())return fail(16);
    // Domains 3/4 publish GPU address identities, not CPU instance contents.
    work->instanceRows.push_back({d.sequence,gpuIdentity?d.side+3:d.side,uint32_t(work->instanceBlob.size()),uint32_t(instances.size()),d.vertices[7].StrideInBytes,material});
    work->instanceBlob.insert(work->instanceBlob.end(),instances.begin(),instances.end());
    if(d.side==0){
        if(!boundCamera.prepared){++CyberpunkVR_EarlyDepthStats[5];return fallback();}
        const bool nativeCamera=work->nativeCamera.gpuAddress && family->nativeCameraPipeline;
        if(nativeCamera){
            bool bound=false;
            for(const auto& binding:family->pairedRoot->plan.bindings)if(binding.binding.sourceRegister==1){
                descriptors::CbvBinding value;
                bound=descriptors::ResolveCbv(list,d.root,binding.sourceParameter,binding.sourceOffset,value) &&
                    value.address==work->nativeCamera.gpuAddress && value.bytes==2048;
            }
            if(!bound)return fail(46);
            if(!work->cameraChecked){
                std::array<uint8_t,2048> expected{},actual{},again{};
                std::memcpy(expected.data(),boundCamera.own.data(),848);std::memcpy(expected.data()+1024,boundCamera.peer.data(),848);
                if(!constants::ReadUpload(work->nativeCamera.gpuAddress,actual.data(),actual.size()) ||
                    !constants::ReadUpload(work->nativeCamera.gpuAddress,again.data(),again.size()) || actual!=expected || again!=expected)return fail(47);
                work->cameraChecked=true;CyberpunkVR_EarlyDepthNativeCameraStats[2]=1;
            }
        }
        CpuScope planning(1);
        std::array<uint8_t,64> geometry{};std::vector<uint8_t> masks;
        if(!packets::CurrentDrawGeometry(work->renderer,d.instances,d.vertices[7].StrideInBytes,geometry)){
            if(RunTracking()){++CyberpunkVR_EarlyDepthRunDeferred[0];return fallback();}
            ++CyberpunkVR_EarlyDepthMaskStats[7];return fail(18);
        }
        uint32_t membershipReason{};
        auto& membership=gpuIdentity?work->gpuMembership:work->membership;
        if(!membership.Plan(geometry,instances,d.vertices[7].StrideInBytes,masks,&membershipReason)){
            if(RunTracking()){++CyberpunkVR_EarlyDepthRunDeferred[0];return fallback();}
            CyberpunkVR_EarlyDepthFailureData[0]=2;CyberpunkVR_EarlyDepthFailureData[1]=d.sequence;
            CyberpunkVR_EarlyDepthFailureData[2]=membershipReason;return fail(19);
        }
        std::vector<uint8_t> extras;
        if(!gpuIdentity && !work->membership.Extras(geometry,d.vertices[7].StrideInBytes,extras)){
            if(!RunTracking())return fail(22);
            ++CyberpunkVR_EarlyDepthRunDeferred[1];extras.clear();
        }
        planning.End();CpuScope emit(2);
        Microsoft::WRL::ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return fail(5);
        if(!work->created){const auto hr=work->pass.Create(device.Get(),UINT(d.viewport.Width),UINT(d.viewport.Height));if(FAILED(hr))return fail(6,hr);work->created=true;}
        std::vector<BankPair> pairs;
        if(!nativeCamera)for(const auto& entry:family->pairedRoot->plan.bindings){BankPair pair;pair.binding=entry.binding;
            if(pair.binding.sourceRegister==1){pair.eyes[0].constants=boundCamera.peer;pair.eyes[1].constants=boundCamera.own;}
            else {pair.eyes[0].constants=material;pair.eyes[1].constants=material;}pairs.push_back(pair);}
        StereoBindingBank bank;
        if(!RunTracking() && !nativeCamera){
            const auto hr=bank.Create(device.Get(),family->pairedRoot->root.Get(),family->pairedRoot->plan,pairs);
            if(FAILED(hr)){++CyberpunkVR_EarlyDepthStats[7];return fail(7,hr);}
        }
        Item item{d,bindings,{instances.begin(),instances.end()},material,boundCamera.peer,std::vector<bool>(d.instances),masks,false,gpuIdentity};
        item.skin=std::move(skin);
        item.alpha=std::move(alpha);
        const auto extraCount=UINT(extras.size()/d.vertices[7].StrideInBytes);
        if(extraCount){
            item.instances.insert(item.instances.end(),extras.begin(),extras.end());item.coverage.resize(d.instances+extraCount);
            item.masks.insert(item.masks.end(),extraCount,uint8_t(1));
            if(work->instanceRows.size()>=MaxInstanceRows || extras.size()>MaxInstanceBytes-work->instanceBlob.size())return fail(16);
            work->instanceRows.push_back({d.sequence,2,uint32_t(work->instanceBlob.size()),uint32_t(extras.size()),d.vertices[7].StrideInBytes,material});
            work->instanceBlob.insert(work->instanceBlob.end(),extras.begin(),extras.end());
        }
        if(gpuIdentity && work->mode==2){
            const uint64_t address=d.vertices[7].BufferLocation+uint64_t(d.firstInstance)*48;
            if(!readable || instanceLease.resource.Get()!=reinterpret_cast<ID3D12Resource*>(d.instanceResource) || address<instanceLease.gpu ||
                !work->pass.CaptureInstances(list,instanceLease.resource.Get(),address-instanceLease.gpu,UINT(instances.size()),
                    D3D12_RESOURCE_STATES(instanceState.value),work->combinedInstanceRead))return fail(43);
        }
        if(item.skin){
            std::vector<skinning::BoneRange> ranges;
            if(!skinning::BoneRanges(family->skinningReadContract,item.instances,item.skin->bones.view,item.skin->bones.resource->GetDesc().Width,ranges)){
                ++CyberpunkVR_EarlyDepthSkinningStats[4];CyberpunkVR_EarlyDepthSkinningStats[5]=6;rejectSkinned(106);return fallback();
            }
            if(skinnedAlpha && d.sequence>UINT_MAX){rejectSkinned(211);return fallback();}
            if(work->mode==2 && !work->pass.CaptureSkinning(list,item.skin->bones.resource.Get(),ranges,item.skin->texture,skinnedAlpha?UINT(d.sequence):0)){
                ++CyberpunkVR_EarlyDepthSkinningStats[4];CyberpunkVR_EarlyDepthSkinningStats[5]=200+texture_read::LastFailure();
                rejectSkinned(100+CyberpunkVR_EarlyDepthSkinningStats[5]);
                if(texture_read::LastFailure()==40)return fail(50);
                return fallback();
            }
        }
        if(nativeCamera){
            if(alphaOptimized && (d.sequence>UINT_MAX || !work->pass.CaptureAlpha(list,item.alpha->material,item.alpha->texture,item.alpha->mips,item.alpha->expectedIndex,UINT(d.sequence)))){
                ++CyberpunkVR_EarlyDepthAlphaStats[3];CyberpunkVR_EarlyDepthAlphaStats[5]=d.sequence>UINT_MAX?10:work->pass.AlphaProofCounts()[5];
                rejectSkinned(200+CyberpunkVR_EarlyDepthAlphaStats[5]);
                if(CyberpunkVR_EarlyDepthAlphaStats[5]==1040 || CyberpunkVR_EarlyDepthAlphaStats[5]==2040)return fail(57);
                return fallback();
            }
            if(!work->pass.Main(list,d,*family,bank,masks,extras,0,true,alphaReference,alphaOptimized))return fail(8);
            ++work->nativeCameraDraws;++CyberpunkVR_EarlyDepthNativeCameraStats[1];
            if(RunTracking())++CyberpunkVR_EarlyDepthRunStats[8];
        }else if(RunTracking()){
            const auto before=work->pass.ArenaCount();const auto rendered=work->pass.MainConstants(list,d,*family,pairs,masks,extras);
            CyberpunkVR_EarlyDepthRunArenas[0]+=work->pass.ArenaCount()-before;
            if(!rendered)return fail(8);++CyberpunkVR_EarlyDepthRunStats[8];
        }else if(!work->pass.Main(list,d,*family,bank,masks,extras))return fail(8);
        replaced=work->whole;
        CyberpunkVR_EarlyDepthSources[work->items.size()][0]=d.sequence;
        CyberpunkVR_EarlyDepthCoverage[work->items.size()][1]=d.instances;
        CyberpunkVR_EarlyDepthExtraCoverage[work->items.size()][1]=extraCount;
        CyberpunkVR_EarlyDepthMaskStats[10]+=extraCount;CyberpunkVR_EarlyDepthMaskStats[11]+=extraCount!=0;
        CyberpunkVR_EarlyDepthMaskStats[13]=CyberpunkVR_EarlyDepthMaskStats[12]-CyberpunkVR_EarlyDepthMaskStats[10];
        for(size_t i=0;i<masks.size();++i){const auto at=masks[i]==3?0:1;
            ++CyberpunkVR_EarlyDepthMasks[work->items.size()][at];++CyberpunkVR_EarlyDepthMaskStats[4+at];
            if(!i || masks[i]!=masks[i-1])++CyberpunkVR_EarlyDepthMaskStats[6];
        }
        if(item.skin)work->bindingChecks.push_back({list,0,*item.skin,material,instanceLease,skinInstanceOffset,{instances.begin(),instances.end()}});
        if(item.alpha){work->alphaChecks.push_back({list,*item.alpha,material});++CyberpunkVR_EarlyDepthAlphaStats[1];
            if(item.alpha->skinned)++CyberpunkVR_EarlyDepthSkinnedAlphaStats[1];}
        work->items.push_back(std::move(item));++CyberpunkVR_EarlyDepthStats[2];
        if(work->items.back().skin)++CyberpunkVR_EarlyDepthSkinningStats[1];
        CyberpunkVR_EarlyDepthGpuInstances[0]+=gpuIdentity;
    }else{
        CpuScope peerCpu(3);
        if(!work->pass.MainSubmitted())return fail(9);
        if(work->peerList && work->peerList!=list)return fail(21);work->peerList=list;
        std::vector<InstanceSource> sources;std::vector<size_t> ids;
        for(size_t i=0;i<work->items.size();++i){auto& item=work->items[i];
            if(item.gpuIdentity==gpuIdentity && SameGeometry(item,d,bindings) &&
                (skin?skinning::SameMaterial(item.material,material):item.material==material) && item.peer==boundCamera.own && SameSkin(item.skin,skin) && SameAlpha(item.alpha,alpha)){
                sources.push_back({item.instances,&item.coverage,item.masks});ids.push_back(i);}}
        InstanceCoveragePlan plan;
        if(!PlanInstanceCoverage(sources,instances,d.vertices[7].StrideInBytes,plan))return fail(14);
        if(plan.matches.empty()){++CyberpunkVR_EarlyDepthStats[8];return fallback();}
        if(alpha){work->alphaChecks.push_back({list,*alpha,material});++CyberpunkVR_EarlyDepthAlphaStats[2];
            if(alpha->skinned)++CyberpunkVR_EarlyDepthSkinnedAlphaStats[2];}
        CyberpunkVR_EarlyDepthGpuInstances[1]+=gpuIdentity;
        if(skin){
            const auto& matched=work->items[ids[plan.matches.front().source]];
            skin->texture=matched.skin->texture;
            ++CyberpunkVR_EarlyDepthSkinningStats[2];work->bindingChecks.push_back({list,1,*skin,material,instanceLease,skinInstanceOffset,{instances.begin(),instances.end()}});
        }
        for(const auto& match:plan.matches)if(work->items[ids[match.source]].masks[match.instance]==2)return fail(20);
        if(work->whole){
            std::vector<InstanceRun> remaining;uint32_t at{};
            for(const auto run:plan.runs){if(run.first>at)remaining.push_back({at,run.first-at});at=run.first+run.count;}
            if(at<d.instances)remaining.push_back({at,d.instances-at});
            if(!work->pass.PeerRemainder(list,d,remaining))return fail(11);replaced=true;if(work->mode!=2)++CyberpunkVR_EarlyDepthStats[10];
        }else for(const auto& run:plan.runs){auto reference=d;reference.firstInstance+=run.first;reference.instances=run.count;
            if(!work->pass.Peer(list,reference))return fail(11);++CyberpunkVR_EarlyDepthStats[10];}
        if(!CommitInstanceCoverage(sources,plan))return fail(15);
        for(const auto& match:plan.matches){const auto at=ids[match.source];
            if(!CyberpunkVR_EarlyDepthSources[at][1])CyberpunkVR_EarlyDepthSources[at][1]=d.sequence;
            if(match.instance<work->items[at].draw.instances)++CyberpunkVR_EarlyDepthCoverage[at][0];
            else ++CyberpunkVR_EarlyDepthExtraCoverage[at][0];}
    }
    return replaced;
    }catch(...){return fail(12,E_OUTOFMEMORY);}
}
void Unsupported(ID3D12GraphicsCommandList* list,uint32_t reason){
    if(probe::internalCommands || CyberpunkVR_EarlyDepthState.load()!=1 || !group.id)return;
    std::lock_guard lock(mutex);if(!work || !work->whole || group.frame!=work->frame ||
        (group.id!=work->mainGroup && group.id!=work->peerGroup))return;
    work->pass.AbortNative(list,UINT(group.side));CyberpunkVR_EarlyDepthFailureData[0]=4;CyberpunkVR_EarlyDepthFailureData[1]=reason;Fail(29);
}
void InstanceWrite(ID3D12GraphicsCommandList* list,ID3D12Resource* resource){
    if(probe::internalCommands)return;
    const bool legacy=CyberpunkVR_EarlyDepthState.load()==1 && group.side==1 && group.id;
    if(!MaterialPending() && !legacy)return;
    std::lock_guard lock(mutex);
    if(work && MaterialPending())for(size_t i=0;i<work->materialCopies.size();++i){auto& copy=*work->materialCopies[i];
        if(copy.state.State()!=material_read::ReadbackState::Stage::Sealed && (!resource || resource==copy.lease.source.resource.Get())){
            copy.state.Invalidated();if(!work->materialRows[i].failure)work->materialRows[i].failure=101;
        }
    }
    if(legacy && work && work->mode==2 && work->peerList==list &&
        ((work->pass.InstanceSource() && (!resource || resource==work->pass.InstanceSource())) || work->pass.ShaderSource(resource))){
        work->pass.AbortNative(list,1);Fail(44);
    }
}
void InstanceBarrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource){
    if(probe::internalCommands || CyberpunkVR_EarlyDepthState.load()!=1 || group.side!=0 || !group.id)return;
    std::lock_guard lock(mutex);
    if(work && work->mode==2 && (!resource || resource==work->pass.InstanceSource()))work->pass.ReleaseInstanceRead(list);
}
void Closing(ID3D12GraphicsCommandList* list){
    if(probe::internalCommands || CyberpunkVR_EarlyDepthState.load()!=1)return;
    std::lock_guard lock(mutex);if(work)work->pass.ReleaseInstanceRead(list);
}
void InstanceAlias(ID3D12GraphicsCommandList* list,ID3D12Resource* before,ID3D12Resource* after){
    if(probe::internalCommands)return;
    const bool legacy=CyberpunkVR_EarlyDepthState.load()==1 && group.side==1 && group.id;
    if(!MaterialPending() && !legacy)return;
    std::lock_guard lock(mutex);
    if(work && MaterialPending())for(size_t i=0;i<work->materialCopies.size();++i){auto& copy=*work->materialCopies[i];
        if(copy.state.State()!=material_read::ReadbackState::Stage::Sealed && texture_read::AliasAffects(copy.lease.source.resource.Get(),before,after)){
            copy.state.Invalidated();if(!work->materialRows[i].failure)work->materialRows[i].failure=105;
        }
    }
    if(legacy && work && work->mode==2 && work->peerList==list && (work->pass.InstanceSource() || work->pass.ShaderSource(nullptr))){
        work->pass.AbortNative(list,1);Fail(44);
    }
}
void Reset(ID3D12CommandList* list){
    if(!RunTracking() && CyberpunkVR_EarlyDepthState.load()!=1 && CyberpunkVR_EarlyDepthState.load()!=2)return;
    std::lock_guard lock(mutex);if(work){
        for(auto& copy:work->materialCopies)if(copy->list==list)copy->state.ResetBeforeSubmit();
        work->pass.Reset(list);if(work->pass.Failed())Fail(13);
    }
    for(auto& value:pool){value->pass.Reset(list);if(value->pass.Failed())RunFail(13);}
}
void BeforeSubmit(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!CyberpunkVR_EarlyDepthState.load() && !RunTracking())return;
    std::lock_guard lock(mutex);if(work){SubmitMaterials(*work,queue,count,lists,false);ValidateSubmission(*work,queue,count,lists);work->pass.BeforeSubmit(queue,count,lists);Publish();}
    for(auto& value:pool){ValidateSubmission(*value,queue,count,lists);value->pass.BeforeSubmit(queue,count,lists);}
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists){
    if(!CyberpunkVR_EarlyDepthState.load() && !RunTracking())return;
    std::lock_guard lock(mutex);if(work){work->pass.Submitted(queue,count,lists);SubmitMaterials(*work,queue,count,lists,true);}
    for(auto& value:pool)value->pass.Submitted(queue,count,lists);
}
void Poll(){
    if(RunTracking())return;
    if(CyberpunkVR_EarlyDepthState.load()!=2)return;
    std::lock_guard lock(mutex);if(!work)return;
    if(!work->alphaValid){Fail(56);return;}
    if(work->materialCapture){
        std::array<bool,2> covered{};for(const auto& row:work->materialRows)if(row.side<2)covered[row.side]=true;
        if(!covered[0] || !covered[1]){Fail(53);return;}
    }
    for(const auto& copy:work->materialCopies)if(copy->state.PendingSubmissions())return;
    for(const auto& copy:work->materialCopies)if(copy->state.ReplayObserved()){Fail(52);return;}
    DepthComparisonResult result;auto hr=work->pass.Read(result);if(hr==S_FALSE)return;
    if(SUCCEEDED(hr)){hr=SealMaterials(*work);if(hr==S_FALSE)return;}
    CyberpunkVR_EarlyDepthResult=hr;CyberpunkVR_EarlyDepthComparison=result;
    Publish();
    work->status=SUCCEEDED(hr)?3:4;
    CyberpunkVR_EarlyDepthState.store(SUCCEEDED(hr)?3:4,std::memory_order_release);
}
}
#endif
