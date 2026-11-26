#include "Render/NativeBreadcrumbWrite.hpp"
#include "Render/NativeTextureRead.hpp"
#include <windows.h>
#if CVR_RUST_VRIK
// The capture and the description: rust/crates/cvr-render/src/breadcrumb_capture.rs; the lease holds the Rust state.
namespace cvr::stereo::breadcrumb_write {
static_assert(sizeof(constants::ResourceCreationInfo)==48 && sizeof(Snapshot)==176 && offsetof(Snapshot,heapDesc)==104 &&
    offsetof(Snapshot,destination)==152,"mirrored by breadcrumb_capture.rs");
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's immediate writes call breadcrumb_capture.rs themselves
extern "C" {
void* cvr_bcw_capture(ID3D12GraphicsCommandList2*,uint32_t,uint64_t,uint64_t,uint64_t);
void cvr_bcw_release(void*);
bool cvr_bcw_describe(const void*,Snapshot*);
uint32_t cvr_bcw_last_failure();
}
struct Lease::Data {
    void* rust{};
    explicit Data(void* value):rust(value){}
    ~Data(){cvr_bcw_release(rust);}
    Data(const Data&)=delete;Data& operator=(const Data&)=delete;
};
bool Capture(ID3D12GraphicsCommandList2* list,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* parameters,
             const D3D12_WRITEBUFFERIMMEDIATE_MODE* modes,uintptr_t caller,Lease& output){
    output={};
    void* made=cvr_bcw_capture(list,count,reinterpret_cast<uintptr_t>(parameters),reinterpret_cast<uintptr_t>(modes),caller);
    if(!made)return false;
    try{output.data_=std::make_shared<const Lease::Data>(made);}catch(...){cvr_bcw_release(made);return false;}
    return true;
}
bool Describe(const Lease& lease,Snapshot& output){output={};return cvr_bcw_describe(lease.data_?lease.data_->rust:nullptr,&output);}
uint32_t LastFailure(){return cvr_bcw_last_failure();}
#endif
}  // namespace cvr::stereo::breadcrumb_write
#else

namespace cvr::stereo::breadcrumb_write {
struct Lease::Data {Snapshot captured;detail::Observation native;};
namespace {
using Microsoft::WRL::ComPtr;
thread_local uint32_t failure{};thread_local std::array<uint64_t,12> details{};
void Stage(uint32_t code){failure=code;details[0]=code;}
uint64_t Base(){return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Memory(void*,uint64_t address,void* output,size_t bytes){
    SIZE_T copied{};return address && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
bool Compatible(){
    static const uint64_t rejected=[]()->uint64_t{
        if(!native_recording::Compatible())return 1;
        const auto base=Base();std::array<uint8_t,1024> bytes{};
        for(const auto code:detail::Contract)if(!base || base>UINT64_MAX-code.rva || code.bytes>bytes.size() ||
            !Memory(nullptr,base+code.rva,bytes.data(),code.bytes) ||
            native_recording::detail::Fingerprint({bytes.data(),code.bytes})!=code.hash)return code.rva;
        return 0;
    }();details[1]=rejected;return !rejected;
}
uint64_t ImageBytes(uint64_t base){
    IMAGE_DOS_HEADER dos{};IMAGE_NT_HEADERS64 nt{};
    if(!detail::Read(Memory,nullptr,base,&dos,sizeof(dos)) || dos.e_magic!=IMAGE_DOS_SIGNATURE ||
        dos.e_lfanew<=0 || dos.e_lfanew>0x100000 || !detail::Field(Memory,nullptr,base,uint32_t(dos.e_lfanew),nt) ||
        nt.Signature!=IMAGE_NT_SIGNATURE || nt.FileHeader.Machine!=IMAGE_FILE_MACHINE_AMD64 ||
        nt.OptionalHeader.Magic!=IMAGE_NT_OPTIONAL_HDR64_MAGIC || nt.OptionalHeader.SizeOfImage<=detail::MarkerOutReturn ||
        base>UINT64_MAX-nt.OptionalHeader.SizeOfImage)return 0;
    return nt.OptionalHeader.SizeOfImage;
}
bool NativeFrame(uint64_t base,uintptr_t caller,detail::Frame& output){
    output={};const auto reject=[](uint32_t code){details[1]=code;return false;};
    const auto size=ImageBytes(base);if(!size)return reject(101);
    const bool direct=caller>=base && caller-base<size;
    if(direct && !detail::CallerMode(base,caller)){details[2]=caller;return reject(102);}
    const auto* tib=reinterpret_cast<const NT_TIB*>(NtCurrentTeb());
    const auto low=reinterpret_cast<uintptr_t>(tib->StackLimit),high=reinterpret_cast<uintptr_t>(tib->StackBase);
    CONTEXT current{};RtlCaptureContext(&current);
    __try{
        for(size_t i=0;i<native_recording::detail::MaxUnwoundFrames;++i){
            if(!current.Rip || current.Rsp<low || current.Rsp>=high)return reject(103);
            if(current.Rip>=base && current.Rip-base<size){
                details[2]=current.Rip;const auto mode=detail::CallerMode(base,current.Rip);
                if(!mode || (direct && current.Rip!=caller))return reject(104);
                DWORD64 nativeBase{};const auto nativeFunction=RtlLookupFunctionEntry(current.Rip,&nativeBase,nullptr);
                if(!nativeFunction || nativeBase!=base || nativeFunction->BeginAddress!=(mode==1?0x3E5174u:0x3E59DCu) ||
                    nativeFunction->EndAddress<=current.Rip-base)return reject(105);
                details[3]=mode==1?current.Rdi:current.Rbx;
                if(!detail::FrameFromRegisters(base,current.Rip,current.Rsp,current.Rbp,current.Rdi,current.Rbx,low,high,output))return reject(106);
                details[1]=0;return true;
            }
            DWORD64 image{};const auto function=RtlLookupFunctionEntry(current.Rip,&image,nullptr);
            if(!function)return reject(107);
            const auto previous=current.Rsp;PVOID handler{};DWORD64 establisher{};
            RtlVirtualUnwind(UNW_FLAG_NHANDLER,image,current.Rip,function,&current,&handler,&establisher,nullptr);
            if(current.Rsp<=previous)return reject(108);
        }
    }__except(EXCEPTION_EXECUTE_HANDLER){return reject(109);}
    return reject(110);
}
void Observed(const detail::Observation& value){
    details[3]=value.holder;details[4]=value.marker;details[5]=value.entry;details[6]=value.resource;
    details[7]=value.heap;details[8]=value.gpu;details[9]=value.slotGpu;details[10]=value.slot;details[11]=value.kind;
}
bool ReadObservation(uint64_t base,const detail::Frame& frame,uint64_t list,
                     const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER& parameter,uint32_t mode,detail::Observation& output){
    Stage(6);uint32_t rejected{};const bool success=detail::Observe(Memory,nullptr,base,frame,list,parameter,mode,output,&rejected);
    details[1]=rejected;Observed(output);return success;
}
bool SameDescription(const Snapshot& value){
    if(!value.creation.resource || !value.creation.heap ||
        !texture_read::detail::SameResourceDescription(value.resourceDesc,value.creation.resource->GetDesc()) ||
        !texture_read::detail::SameHeapDescription(value.heapDesc,value.creation.heap->GetDesc()))return false;
    D3D12_HEAP_PROPERTIES properties{};D3D12_HEAP_FLAGS flags{};
    return SUCCEEDED(value.creation.resource->GetHeapProperties(&properties,&flags)) &&
        texture_read::detail::SameHeapProperties(properties,value.heapDesc.Properties) && flags==value.heapDesc.Flags;
}
}
bool Capture(ID3D12GraphicsCommandList2* list,UINT count,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER* parameters,
             const D3D12_WRITEBUFFERIMMEDIATE_MODE* modes,uintptr_t caller,Lease& output)try{
    output={};details={};Stage(1);
    if(!list || count!=1 || !parameters || !modes || !caller)return false;
    Stage(2);if(!Compatible())return false;
    const auto base=Base();Stage(3);detail::Frame frame;
    if(!NativeFrame(base,caller,frame))return false;details[2]=frame.rip;
    D3D12_WRITEBUFFERIMMEDIATE_PARAMETER parameter{};D3D12_WRITEBUFFERIMMEDIATE_MODE mode{};
    Stage(4);if(!detail::Read(Memory,nullptr,reinterpret_cast<uintptr_t>(parameters),&parameter,sizeof(parameter)) ||
        !detail::Read(Memory,nullptr,reinterpret_cast<uintptr_t>(modes),&mode,sizeof(mode)) ||
        !detail::Parameters(detail::CallerMode(base,frame.rip),count,parameter,uint32_t(mode)))return false;
    // The current-context TLS is legitimately absent during native Close.
    // Its still-active breadcrumb frame retains the actual marker holder.
    auto value=std::make_shared<Lease::Data>();
    if(!ReadObservation(base,frame,reinterpret_cast<uintptr_t>(list),parameter,uint32_t(mode),value->native))return false;
    auto& saved=value->captured;auto& creation=saved.creation;
    creation.resource=reinterpret_cast<ID3D12Resource*>(value->native.resource);
    creation.heap=reinterpret_cast<ID3D12Heap*>(value->native.heap);
    saved.destination=parameter.Dest;saved.value=parameter.Value;saved.mode=uint32_t(mode);
    saved.resourceDesc=creation.resource->GetDesc();saved.heapDesc=creation.heap->GetDesc();
    ComPtr<ID3D12Device> resourceDevice,heapDevice,listDevice;Stage(7);
    if(FAILED(creation.resource->GetDevice(IID_PPV_ARGS(&resourceDevice))) || !resourceDevice ||
        FAILED(creation.heap->GetDevice(IID_PPV_ARGS(&heapDevice))) || resourceDevice!=heapDevice ||
        FAILED(list->GetDevice(IID_PPV_ARGS(&listDevice))) || resourceDevice!=listDevice || resourceDevice->GetNodeCount()!=1)return false;
    const auto allocation=resourceDevice->GetResourceAllocationInfo(0,1,&saved.resourceDesc);
    if(!detail::Shape(value->native,saved.resourceDesc,saved.heapDesc,allocation,creation.resource->GetGPUVirtualAddress()) ||
        !SameDescription(saved))return false;
    Stage(8);constants::ResourceCreationInfo observed;
    if(constants::QueryResourceCreation(creation.resource,observed)){
        if(!detail::ObservedCreationMatches(observed,value->native,allocation.SizeInBytes))return false;
        creation=std::move(observed);saved.provenance=Provenance::ObservedCreation;
    }else{
        creation.kind=2;creation.heapOffset=0;creation.allocationBytes=allocation.SizeInBytes;
        creation.initialStateKnown=true;creation.initialState=D3D12_RESOURCE_STATE_COPY_DEST;saved.provenance=Provenance::NativeFactory;
    }
    detail::Frame frameAgain;Stage(9);if(!NativeFrame(base,caller,frameAgain) || frameAgain!=frame)return false;
    detail::Observation again;if(!ReadObservation(base,frameAgain,reinterpret_cast<uintptr_t>(list),parameter,uint32_t(mode),again))return false;
    Stage(9);if(again!=value->native || !SameDescription(saved) ||
        creation.resource->GetGPUVirtualAddress()!=value->native.gpu)return false;
    output.data_=std::move(value);Stage(0);return true;
}catch(...){output={};Stage(10);return false;}
bool Describe(const Lease& lease,Snapshot& output)try{
    output={};Stage(1);if(!lease.data_)return false;
    const auto& value=*lease.data_;Stage(9);
    if(!SameDescription(value.captured) || value.captured.creation.resource->GetGPUVirtualAddress()!=value.native.gpu)return false;
    if(value.captured.provenance==Provenance::ObservedCreation){constants::ResourceCreationInfo current;
        if(!constants::QueryResourceCreation(value.captured.creation.resource,current) ||
            !texture_read::detail::SameCreationMetadata(current,value.captured.creation))return false;
    }
    output=value.captured;Stage(0);return true;
}catch(...){output={};Stage(10);return false;}
uint32_t LastFailure(){return failure;}
std::array<uint64_t,12> LastDetails(){return details;}
}
#endif
