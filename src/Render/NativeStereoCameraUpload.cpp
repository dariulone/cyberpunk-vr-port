#include "Render/NativeStereoCameraUpload.hpp"
#include "Render/NativeStereoProbe.hpp"
#include "Render/SinglePassTrace.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/ConstantUploadRegistry.hpp"
#include "Render/NativeCameraFlushState.hpp"
#include "Render/StereoSceneState.hpp"
#include "Render/CommandResources.hpp"
#include <windows.h>
#include <wrl/client.h>
#include <array>
#include <mutex>
#include <cstring>
#include <optional>
using Microsoft::WRL::ComPtr;
#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassTwinUpload{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassTwinAllocations{0},CyberpunkVR_SinglePassTwinBindings{0},CyberpunkVR_SinglePassTwinFailures{0};
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
// The upload checks: rust/crates/cvr-render/src/camera_upload_checks.rs (the native calls and the cached results stay
// here).
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The camera upload: rust/crates/cvr-render/src/camera_upload.rs. The descriptor audit's lookups, the constant upload
// registry and the native probe's state are lent to it here.
struct CvrCameraUploadRuntime {
    std::atomic<uint32_t>* twinUpload;std::atomic<uint32_t>* twinAllocations;std::atomic<uint32_t>* twinBindings;
    std::atomic<uint32_t>* twinFailures;std::atomic<uint32_t>* traceState;std::atomic<uint32_t>* pipelineCount;
    const cvr::stereo::native_probe::PipelineRecord* pipelines;
    bool(*probeEnabled)();
};
static_assert(sizeof(CvrCameraUploadRuntime)==64 && sizeof(cvr::stereo::descriptors::CbvBinding)==32 &&
    sizeof(cvr::stereo::native_probe::PipelineRecord)==80 && sizeof(cvr::stereo::camera_upload::Result)==24 &&
    sizeof(cvr::stereo::ShaderCamera)==848,"mirrored by camera_upload.rs");
extern "C" {
void cvr_ncu_attach(const CvrCameraUploadRuntime*);
void cvr_ncu_current(uint64_t,cvr::stereo::camera_upload::Context*);
void cvr_ncu_current_binding(cvr::stereo::camera_upload::Result*);
bool cvr_ncu_begin_stages(bool,uint64_t,uint32_t,const void*,const void*,const void*,void**,cvr::stereo::camera_upload::Result*,uint32_t,
    cvr::stereo::camera_upload::BindingScope*);
bool cvr_ncu_end_stages(bool,uint64_t,cvr::stereo::camera_upload::BindingScope*);
void cvr_ncu_diagnostics(std::array<uint64_t,16>*);
}
namespace cvr::stereo::camera_upload {
namespace {
bool ProbeEnabled(){return native_probe::Enabled();}
const CvrCameraUploadRuntime kRuntime{&CyberpunkVR_SinglePassTwinUpload,&CyberpunkVR_SinglePassTwinAllocations,&CyberpunkVR_SinglePassTwinBindings,
    &CyberpunkVR_SinglePassTwinFailures,&CyberpunkVR_SinglePassTraceState,&CyberpunkVR_NativeStereoPipelineCount,CyberpunkVR_NativeStereoPipelines,
    &ProbeEnabled};
const bool kAttached=(cvr_ncu_attach(&kRuntime),true);
void** Heap(ComPtr<ID3D12DescriptorHeap>& heap){return reinterpret_cast<void**>(heap.GetAddressOf());}
}
Context CurrentContext(uintptr_t base){Context context;cvr_ncu_current(base,&context);return context;}
std::array<uint64_t,16> BindingDiagnostics(){std::array<uint64_t,16> value;cvr_ncu_diagnostics(&value);return value;}
Result CurrentBinding(){Result result;cvr_ncu_current_binding(&result);return result;}
}
#endif
#else
namespace cvr::stereo::camera_upload {
namespace {
std::mutex mutex;
ComPtr<ID3D12DescriptorHeap> descriptors;
UINT stride{},next{};
constexpr UINT Capacity=128;
thread_local Result* expectedResult{};
struct ResultCapture {
    Result* previous{};
    explicit ResultCapture(Result& result):previous(expectedResult){expectedResult=&result;}
    ~ResultCapture(){expectedResult=previous;}
    ResultCapture(const ResultCapture&)=delete;
    ResultCapture& operator=(const ResultCapture&)=delete;
};
struct Pending {uint32_t nativeHandle{};Result result{};};
thread_local Pending pending;
thread_local Result currentBinding;
thread_local BindingTracker bindingTracker;
thread_local std::array<uint64_t,16> bindingDiagnostics;
bool Read(uintptr_t address,void* output,size_t bytes) {
    SIZE_T copied{};return address && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(address),output,bytes,&copied) && copied==bytes;
}
bool NativeCompatible(uintptr_t base){
    if(!base)return false;
#if CVR_RUST_VRIK
    static const bool compatible=cvr_ncu_native_code(base);return compatible;
#else
    static const bool compatible=[&]{
        struct Code {uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{{0x333B88,0xAD,0x76EEBB3582ED71F9ull},{0x336D3C,0xF1,0x6B6B9044CD5F18EAull},{0x337230,0x33,0x96D1A06A9C2CBA7Full}};
        std::array<uint8_t,256> bytes{};
        for(const auto& c:expected){if(!Read(base+c.rva,bytes.data(),c.bytes))return false;
            uint64_t hash=14695981039346656037ull;for(UINT i=0;i<c.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=c.hash)return false;}
        return true;
    }();return compatible;
#endif
}
Context Current(uintptr_t base){
    if(!NativeCompatible(base))return {};
    using Get=void*(*)();Context result;
    result.context=reinterpret_cast<uintptr_t>(reinterpret_cast<Get>(base+0x337230)());
#if CVR_RUST_VRIK
    if(!cvr_ncu_context(result.context,&result.list,&result.state))return {};
#else
    if(!result.context || result.context>UINT64_MAX-0x68 ||
        !Read(result.context+0x30,&result.list,8) || !Read(result.context+0x60,&result.state,8))return {};
#endif
    return result;
}
bool AllocateNative(uintptr_t base,Context context,const ShaderCamera& main,const ShaderCamera& peer,ComPtr<ID3D12DescriptorHeap>& heap,Result& output){
    output={};output.reserved=1;if(!NativeCompatible(base) || expectedResult)return false;
    output.reserved=2;if(!context)return false;
    auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(context.list);
    output.reserved=3;ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    if(!heap){const D3D12_DESCRIPTOR_HEAP_DESC desc{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,1,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
        if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&heap))))return false;}
    ComPtr<ID3D12Device> actual;if(FAILED(heap->GetDevice(IID_PPV_ARGS(&actual))) || actual!=device)return false;
    const auto heapDesc=heap->GetDesc();
    if(heapDesc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || heapDesc.NumDescriptors!=1 || heapDesc.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_NONE)return false;
    output.reserved=4;alignas(16) std::array<uint8_t,2048> bytes{};
    std::memcpy(bytes.data(),main.data(),main.size());std::memcpy(bytes.data()+1024,peer.data(),peer.size());
    Result result;result.cpuDescriptor=heap->GetCPUDescriptorHandleForHeapStart().ptr;
    using Upload=void(*)(uint32_t,const void*,D3D12_CPU_DESCRIPTOR_HANDLE);
    {ResultCapture capture(result);reinterpret_cast<Upload>(base+0x333B88)(UINT(bytes.size()),bytes.data(),{result.cpuDescriptor});}
#if CVR_RUST_VRIK
    static_assert(sizeof(bytes)==2048,"the pair cvr_ncu_uploaded checks");
    if(!cvr_ncu_uploaded(result.bytes,result.gpuAddress))return false;
#else
    if(result.bytes!=bytes.size() || !result.gpuAddress || (result.gpuAddress&255))return false;
#endif
    output=result;return true;
}
}
Context CurrentContext(uintptr_t base){return Current(base);}
void ObserveNativeBind(uintptr_t base,uint32_t binding,uint32_t handle,uint32_t stages){
    if(binding==1 && (stages&3))bindingTracker.Observe(Read,Current(base),handle,stages);
}
std::array<uint64_t,16> BindingDiagnostics(){return bindingDiagnostics;}
static bool BeginStages(uintptr_t base,uint32_t nativeCameraHandle,const ShaderCamera& current,const ShaderCamera& main,const ShaderCamera& peer,ComPtr<ID3D12DescriptorHeap>& heap,
                       Result& output,uint32_t stages,BindingScope& scope){
    output={};output.reserved=5;scope={};bindingDiagnostics={};
    const auto context=Current(base);
    bindingDiagnostics[0]=stages;bindingDiagnostics[1]=context.context;bindingDiagnostics[2]=context.state;bindingDiagnostics[3]=context.list;
    if(!bindingTracker.Capture(Read,context,stages,scope))return false;
    // Original shaders may still execute in this scope, so every requested
    // stage must already read the exact MAIN camera kept in the new prefix.
    uint64_t trustedAddress{};uint32_t trustedBytes{};
    for(UINT stage=0;stage<2;++stage)if(stages&(1u<<stage)){
        descriptors::CbvBinding binding;ShaderCamera before{},again{};
        output.reserved=6+stage;
        bindingDiagnostics[4]=stage;bindingDiagnostics[5]=scope.original[stage].handle;bindingDiagnostics[6]=scope.original[stage].descriptor;
        const bool resolved=descriptors::ResolveCpuCbvAddress(scope.original[stage].descriptor,binding);
        bindingDiagnostics[7]=resolved;bindingDiagnostics[8]=binding.address;bindingDiagnostics[9]=binding.bytes;
        // BufUpload camera handles can name DEFAULT buffers, which are not
        // readable through the CPU upload registry. Both stages using the
        // caller's freshly uploaded/bound handle share its exact camera input.
#if CVR_RUST_VRIK
        if(cvr_ncu_trusted(resolved,binding.bytes,nativeCameraHandle,scope.original[stage].handle,binding.address,trustedAddress,trustedBytes)){
#else
        if(resolved && binding.bytes>=current.size() && nativeCameraHandle && scope.original[stage].handle==nativeCameraHandle &&
            (!trustedAddress || (binding.address==trustedAddress && binding.bytes==trustedBytes))){
#endif
            trustedAddress=binding.address;trustedBytes=binding.bytes;
            bindingDiagnostics[10]=2;bindingDiagnostics[11]=nativeCameraHandle;bindingDiagnostics[15]=2;continue;
        }
        const bool first=resolved && binding.bytes>=current.size() && constants::ReadUpload(binding.address,before.data(),before.size());
        const bool second=first && constants::ReadUpload(binding.address,again.data(),again.size());
        bindingDiagnostics[10]=first;bindingDiagnostics[11]=second;bindingDiagnostics[12]=UINT64_MAX;
#if CVR_RUST_VRIK
        static_assert(sizeof(ShaderCamera)==848,"mirrored by camera_upload_checks.rs");
        if(first)cvr_ncu_first_difference(current.data(),before.data(),bindingDiagnostics.data()+12);
#else
        if(first)for(size_t i=0;i<current.size();i+=4)if(std::memcmp(current.data()+i,before.data()+i,4)){
            uint32_t actual{},expected{};std::memcpy(&actual,before.data()+i,4);std::memcpy(&expected,current.data()+i,4);
            bindingDiagnostics[12]=i/4;bindingDiagnostics[13]=actual;bindingDiagnostics[14]=expected;break;
        }
#endif
        bindingDiagnostics[15]=first && second && before==again;
        if(!first || !second || before!=current || again!=current){scope={};return false;}
    }
    if(!AllocateNative(base,context,main,peer,heap,output)){scope={};return false;}
    for(UINT stage=0;stage<2;++stage)if(stages&(1u<<stage)){
        CpuSlot actualSlot;
        if(Current(base)!=context || bindingTracker.Changed(scope,stage) || !ResolveCameraSlot(Read,context.state,stage,actualSlot) ||
            actualSlot.descriptor!=scope.original[stage].descriptor){output.reserved=8;scope={};return false;}
    }
    scope.replacement=output.cpuDescriptor;
    using Bind=void(*)(void*,uint8_t,uint32_t,uint32_t,const uint64_t*,bool);
    for(UINT stage=0;stage<2;++stage)if(stages&(1u<<stage)){
        scope.applied|=1u<<stage;
        reinterpret_cast<Bind>(base+0x336D3C)(reinterpret_cast<void*>(context.state),uint8_t(stage),2,1,&scope.replacement,true);
    }
    for(UINT stage=0;stage<2;++stage)if(stages&(1u<<stage)){
        CpuSlot actual;
        if(Current(base)!=context || !ResolveCameraSlot(Read,context.state,stage,actual) || actual.descriptor!=scope.replacement){
            output.reserved=9;EndNativeStages(base,scope);return false;
        }
    }
    output.reserved=0;return true;
}
bool BeginNativeStages(uintptr_t base,uint32_t nativeCameraHandle,const ShaderCamera& main,const ShaderCamera& peer,
                       ComPtr<ID3D12DescriptorHeap>& heap,Result& output,uint32_t stages,BindingScope& scope){
    return BeginStages(base,nativeCameraHandle,main,main,peer,heap,output,stages,scope);
}
bool EndNativeStages(uintptr_t base,BindingScope& scope){
    if(!scope.applied)return true;
    const auto restore=[](void* user,Context expected,uint32_t handle,uint32_t stages){
        const auto base=reinterpret_cast<uintptr_t>(user);if(Current(base)!=expected || !handle)return false;
        using Bind=__int64(__fastcall*)(uint32_t,uint32_t,uint32_t);
        reinterpret_cast<Bind>(base+0x337968)(1,handle,stages);return Current(base)==expected;
    };
    return RestoreScope(Read,Current(base),scope,restore,reinterpret_cast<void*>(base));
}
namespace {
struct FlushReceipt {
    flush_state::Snapshot original,flushed;
    scene_state::Bindings bindings;
    ComPtr<ID3D12DescriptorHeap> resourceHeap;
    ComPtr<ID3D12RootSignature> root;
    uint32_t stages{};
};
thread_local std::optional<FlushReceipt> pairFlush;
bool FlushCompatible(uintptr_t base){
    if(!base)return false;
#if CVR_RUST_VRIK
    static const bool compatible=cvr_ncu_flush_code(base);return compatible;
#else
    static const bool compatible=[&]{
        struct Code {uint32_t rva,bytes;uint64_t hash;};
        constexpr Code expected[]{{0x335C3C,0x4DC,0x20371793713C127Full},{0x2457F84,0xD3,0xFDEFCB6798D5CC8Bull}};
        std::array<uint8_t,0x500> bytes{};
        for(const auto& code:expected){if(base>UINT64_MAX-code.rva || !Read(base+code.rva,bytes.data(),code.bytes))return false;
            uint64_t hash=14695981039346656037ull;
            for(UINT i=0;i<code.bytes;++i)hash=(hash^bytes[i])*1099511628211ull;
            if(hash!=code.hash)return false;}
        return true;
    }();return compatible;
#endif
}
bool SameBindings(const scene_state::Bindings& before,ID3D12GraphicsCommandList* list){
    scene_state::Bindings after;
    return scene_state::Snapshot(list,after) && before.root==after.root && before.heapCount==after.heapCount && before.heaps==after.heaps;
}
bool HeapCache(const flush_state::Snapshot& state,ID3D12DescriptorHeap* heap){
    return heap && (!state.cpuValid || state.cachedCpu==heap->GetCPUDescriptorHandleForHeapStart().ptr) &&
        (!state.gpuValid || state.cachedGpu==heap->GetGPUDescriptorHandleForHeapStart().ptr);
}
bool GpuCamera(ID3D12GraphicsCommandList* list,uint64_t root,uint32_t stages,const flush_state::Snapshot& state,
               const Result* replacement=nullptr){
    for(UINT stage=0;stage<2;++stage)if(stages&(1u<<stage)){
        CpuSlot slot;UINT parameter{},offset{};descriptors::CbvBinding cpu,gpu;
        if(!ResolveCameraSlot(Read,state.context.state,stage,slot) ||
           !descriptors::FindBinding(root,D3D12_DESCRIPTOR_RANGE_TYPE_CBV,0,1,
               stage?D3D12_SHADER_VISIBILITY_PIXEL:D3D12_SHADER_VISIBILITY_VERTEX,parameter,offset))return false;
        const auto found=std::find_if(state.tables.begin(),state.tables.end(),[&](const auto& table){return table.group==slot.parameter;});
        if(found==state.tables.end() || found->rootParameter!=parameter || offset!=1-found->first ||
           !descriptors::ResolveCpuCbvAddress(slot.descriptor,cpu) || !descriptors::ResolveCbv(list,root,parameter,offset,gpu) ||
           cpu.address!=gpu.address || cpu.bytes!=gpu.bytes || cpu.bytes<848 ||
           (replacement && (gpu.address!=replacement->gpuAddress || gpu.bytes!=2048)))return false;
        bindingDiagnostics[7]=1;bindingDiagnostics[8]=gpu.address;bindingDiagnostics[9]=gpu.bytes;
    }
    return true;
}
bool Flush(uintptr_t base,const flush_state::Snapshot& update,FlushReceipt& receipt,flush_state::Snapshot& after){
    if(!FlushCompatible(base) || Current(base)!=update.context || !HeapCache(update,receipt.resourceHeap.Get()) ||
       !SameBindings(receipt.bindings,reinterpret_cast<ID3D12GraphicsCommandList*>(update.context.list)))return false;
    using Materialize=void(__fastcall*)(void*,void*,uint8_t);
    // The original indexed draw uses this exact call before its D3D draw. The
    // no-rollover receipt reserves both new tables and forbids heap replacement.
    reinterpret_cast<Materialize>(base+0x335C3C)(reinterpret_cast<void*>(update.context.state),reinterpret_cast<void*>(update.context.context),0);
    return Current(base)==update.context && flush_state::Read(Read,update.context,receipt.stages,after) &&
        flush_state::Flushed(update,after) && after.cpuValid && after.gpuValid && HeapCache(after,receipt.resourceHeap.Get()) &&
        SameBindings(receipt.bindings,reinterpret_cast<ID3D12GraphicsCommandList*>(update.context.list));
}
}
bool BeginNativePairStages(uintptr_t base,uint32_t currentHandle,const ShaderCamera& expectedCurrent,const ShaderCamera& main,const ShaderCamera& peer,
                          ComPtr<ID3D12DescriptorHeap>& heap,Result& output,uint32_t stages,BindingScope& scope){
    output={};scope={};bindingDiagnostics={};bindingDiagnostics[12]=20;
    const auto context=Current(base);FlushReceipt receipt;receipt.stages=stages;
    bindingDiagnostics[0]=stages;bindingDiagnostics[1]=context.context;bindingDiagnostics[2]=context.state;bindingDiagnostics[3]=context.list;
    const auto compatible=FlushCompatible(base);bindingDiagnostics[15]=compatible;
    if(pairFlush){bindingDiagnostics[12]=25;return false;}
    if(!compatible || !context)return false;
    bindingDiagnostics[12]=26;
    Read(context.state,&bindingDiagnostics[4],8);Read(context.state+0x68,&bindingDiagnostics[5],8);
    Read(context.state+0x70,&bindingDiagnostics[6],8);Read(context.state+0x78,&bindingDiagnostics[7],8);
    for(UINT i=0;i<4;++i){uint32_t value{};Read(context.state+8+i*4,&value,4);bindingDiagnostics[8+i]=value;}
    uint8_t rebind{};Read(context.state+0x180,&rebind,1);bindingDiagnostics[13]=rebind;
    uint32_t program{};Read(context.context+0x3c0,&program,4);bindingDiagnostics[14]=program;
    if(!flush_state::Read(Read,context,stages,receipt.original))return false;
    bindingDiagnostics[12]=27;bindingDiagnostics[13]=receipt.original.groups;bindingDiagnostics[14]=receipt.original.count;
    if(!flush_state::Room(receipt.original))return false;
    bindingDiagnostics[12]=21;auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(context.list);
    if(!scene_state::Snapshot(list,receipt.bindings) || !receipt.bindings.resetSeen || !receipt.bindings.heapsSeen || !receipt.bindings.root)return false;
    for(UINT i=0;i<receipt.bindings.heapCount;++i)if(reinterpret_cast<uintptr_t>(receipt.bindings.heaps[i])==receipt.original.heap)receipt.resourceHeap=receipt.bindings.heaps[i];
    if(!receipt.resourceHeap)return false;receipt.root=receipt.bindings.root;
    const auto desc=receipt.resourceHeap->GetDesc();ComPtr<ID3D12Device> device,heapDevice;
    if(list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV ||
       desc.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE || uint64_t(receipt.original.base)+receipt.original.capacity>desc.NumDescriptors ||
       FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || FAILED(receipt.resourceHeap->GetDevice(IID_PPV_ARGS(&heapDevice))) || device!=heapDevice ||
       receipt.original.stride!=device->GetDescriptorHandleIncrementSize(desc.Type) || !HeapCache(receipt.original,receipt.resourceHeap.Get()) ||
       !GpuCamera(list,reinterpret_cast<uintptr_t>(receipt.root.Get()),stages,receipt.original) ||
       !gpu::KeepCommandResources(list,{receipt.resourceHeap.Get(),receipt.root.Get()}))return false;
    pairFlush=std::move(receipt);
    if(!BeginStages(base,currentHandle,expectedCurrent,main,peer,heap,output,stages,scope)){
        bindingDiagnostics[12]=100+output.reserved;const auto failed=bindingDiagnostics;
        if(scope.applied)EndNativePairStages(base,scope);else pairFlush.reset();bindingDiagnostics=failed;return false;
    }
    bindingDiagnostics[12]=22;flush_state::Snapshot update;
    if(!flush_state::Read(Read,context,stages,update) || !flush_state::Pending(pairFlush->original,update,output.cpuDescriptor) ||
       !Flush(base,update,*pairFlush,pairFlush->flushed) ||
       !GpuCamera(list,reinterpret_cast<uintptr_t>(pairFlush->root.Get()),stages,pairFlush->flushed,&output)){
        const auto failed=bindingDiagnostics;const bool restored=EndNativePairStages(base,scope);
        bindingDiagnostics=failed;bindingDiagnostics[14]=restored;bindingDiagnostics[15]=0;return false;
    }
    bindingDiagnostics[12]=0;bindingDiagnostics[15]=1;return true;
}
bool EndNativePairStages(uintptr_t base,BindingScope& scope)try{
    if(!pairFlush)return EndNativeStages(base,scope);
    auto receipt=std::move(*pairFlush);pairFlush.reset();bindingDiagnostics[12]=23;
    // Restoring one shared VS/PS table can satisfy both stages. The exact CPU
    // descriptor comparison below distinguishes that from an unrelated rebind.
    if(!EndNativeStages(base,scope) || Current(base)!=receipt.original.context)return false;
    flush_state::Snapshot update,after;
    if(!flush_state::Read(Read,receipt.original.context,receipt.stages,update) || !flush_state::SameAllocation(receipt.original,update) ||
       update.dirty!=(receipt.original.dirty|update.groups) || update.rebind || update.cursor>update.capacity || update.count>update.capacity-update.cursor)return false;
    for(size_t i=0;i<update.tables.size();++i)
        if(update.tables[i].descriptors!=receipt.original.tables[i].descriptors || update.tables[i].cached!=-1)return false;
    if(!Flush(base,update,receipt,after) || !GpuCamera(reinterpret_cast<ID3D12GraphicsCommandList*>(update.context.list),
       reinterpret_cast<uintptr_t>(receipt.root.Get()),receipt.stages,after))return false;
    bindingDiagnostics[12]=0;bindingDiagnostics[15]=1;return true;
}catch(...){EndNativeStages(base,scope);pairFlush.reset();bindingDiagnostics[12]=24;return false;}
bool BeginNative(uintptr_t base,const ShaderCamera& main,const ShaderCamera& peer,ComPtr<ID3D12DescriptorHeap>& heap,Result& output){
    const auto context=Current(base);
    if(!AllocateNative(base,context,main,peer,heap,output))return false;
    using Bind=void(*)(void*,uint8_t,uint32_t,uint32_t,const uint64_t*,bool);
    reinterpret_cast<Bind>(base+0x336D3C)(reinterpret_cast<void*>(context.state),0,2,1,&output.cpuDescriptor,true);
    return true;
}
void EndNative(uintptr_t base,uint32_t handle){
    if(base && handle){using Bind=__int64(__fastcall*)(uint32_t,uint32_t,uint32_t);
        reinterpret_cast<Bind>(base+0x337968)(1,handle,1);}
}
Result Prepare(uintptr_t base,uint32_t nativeHandle,const ShaderCamera& first,const ShaderCamera& second) {
    pending={};
    if(!base || !nativeHandle || !CyberpunkVR_SinglePassTwinUpload.load(std::memory_order_relaxed) ||
       CyberpunkVR_SinglePassTraceState.load(std::memory_order_relaxed)!=1 || !native_probe::Enabled())return {};
    // A validated native prototype must exist before attempting its camera path.
    const auto count=CyberpunkVR_NativeStereoPipelineCount.load(std::memory_order_acquire);ID3D12PipelineState* pipeline{};
    for(unsigned i=0;i<count;++i)if(CyberpunkVR_NativeStereoPipelines[i].instanced) {
        pipeline=reinterpret_cast<ID3D12PipelineState*>(CyberpunkVR_NativeStereoPipelines[i].original);break;
    }
    if(!pipeline)return {};
    std::lock_guard lock(mutex);
    if(next>=Capacity)return {};
    if(!descriptors) {
        // The probe retains the original PSO, which supplies its actual device.
        ComPtr<ID3D12Device> device;
        if(FAILED(pipeline->GetDevice(IID_PPV_ARGS(&device)))){++CyberpunkVR_SinglePassTwinFailures;return {};}
        D3D12_DESCRIPTOR_HEAP_DESC desc{};desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;desc.NumDescriptors=Capacity;
        if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&descriptors)))){++CyberpunkVR_SinglePassTwinFailures;return {};}
        stride=device->GetDescriptorHandleIncrementSize(desc.Type);
    }
    alignas(16) std::array<uint8_t,2048> data{};
    std::memcpy(data.data(),first.data(),first.size());std::memcpy(data.data()+1024,second.data(),second.size());
    Result result;result.cpuDescriptor=descriptors->GetCPUDescriptorHandleForHeapStart().ptr+next++*stride;
    using Upload=void(*)(uint32_t,const void*,D3D12_CPU_DESCRIPTOR_HANDLE);
    const auto upload=reinterpret_cast<Upload>(base+0x333B88);
    {ResultCapture capture(result);upload(UINT(data.size()),data.data(),{result.cpuDescriptor});}
#if CVR_RUST_VRIK
    static_assert(sizeof(data)==2048,"the pair cvr_ncu_uploaded checks");
    if(!cvr_ncu_uploaded(result.bytes,result.gpuAddress)) {++CyberpunkVR_SinglePassTwinFailures;return {};}
#else
    if(result.bytes!=data.size() || !result.gpuAddress || (result.gpuAddress&255)) {++CyberpunkVR_SinglePassTwinFailures;return {};}
#endif
    ++CyberpunkVR_SinglePassTwinAllocations;pending={nativeHandle,result};return result;
}
void CreatedCbv(const D3D12_CONSTANT_BUFFER_VIEW_DESC* desc,D3D12_CPU_DESCRIPTOR_HANDLE destination) {
    if(!expectedResult || !desc || destination.ptr!=expectedResult->cpuDescriptor)return;
    expectedResult->gpuAddress=desc->BufferLocation;expectedResult->bytes=desc->SizeInBytes;
}
void AfterNativeBind(uintptr_t base,uint32_t binding,uint32_t nativeHandle,uint32_t stages,bool sceneVrcam) {
    if(binding==1)currentBinding={};
    const auto upload=pending.result;const auto descriptor=upload.cpuDescriptor;
    if(!descriptor || binding!=1 || nativeHandle!=pending.nativeHandle)return;
    pending={};
    if(!base || !sceneVrcam || !stages || stages>3 || CyberpunkVR_SinglePassTraceState.load(std::memory_order_relaxed)!=1 ||
       !CyberpunkVR_SinglePassTwinUpload.load(std::memory_order_relaxed))return;
    using Current=void*(*)();using Bind=void(*)(void*,uint8_t,uint32_t,uint32_t,const uint64_t*,bool);
    const auto current=reinterpret_cast<Current>(base+0x337230)();uintptr_t state{};
    if(!Read(reinterpret_cast<uintptr_t>(current)+0x60,&state,sizeof(state)) || !state){++CyberpunkVR_SinglePassTwinFailures;return;}
    const auto bind=reinterpret_cast<Bind>(base+0x336D3C);
    for(unsigned stage=0;stage<2;++stage)if(stages&(1u<<stage))bind(reinterpret_cast<void*>(state),uint8_t(stage),2,binding,&descriptor,true);
    currentBinding=upload;++CyberpunkVR_SinglePassTwinBindings;
}
Result CurrentBinding(){return CyberpunkVR_SinglePassTwinUpload.load(std::memory_order_relaxed) && CyberpunkVR_SinglePassTraceState.load(std::memory_order_relaxed)==1?currentBinding:Result{};}
}
#endif
