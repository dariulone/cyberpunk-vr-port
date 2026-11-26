#include "Render/NativeMaterialRead.hpp"
#include <cstdio>
#include <stdexcept>

using namespace cvr::stereo;
namespace {
void Require(bool good){if(!good)throw std::runtime_error("material read assertion failed");}
constexpr uint64_t game=0x1000000,registry=0x5000000;
constexpr uint32_t handle=2;
constexpr uint64_t recordAddress=registry+0x5C0AF8+uint64_t(handle-1)*0xB0;
struct Memory {
    uint64_t registryValue=registry;
    std::array<uint8_t,0x50> record{};
    uint32_t reads{},failAt{},changeAt{};
    bool changeRegistry{};
};
void PutRecord(Memory& memory,const material_binding::Record& record,uint8_t type=6){
    memory.record={};std::memcpy(memory.record.data(),&record.bytes,4);
    memory.record[6]=uint8_t((record.kind<<4)|type);
    std::memcpy(memory.record.data()+8,&record.gpu,8);
    std::memcpy(memory.record.data()+16,&record.resource,8);
    std::memcpy(memory.record.data()+0x48,&record.descriptor,8);
}
bool Read(void* user,uint64_t address,void* out,size_t bytes){
    auto& memory=*static_cast<Memory*>(user);
    if(++memory.reads==memory.failAt)return false;
    if(memory.reads==memory.changeAt){
        if(memory.changeRegistry)memory.registryValue+=8;
        else memory.record[16]^=8;
    }
    if(address==game+0x3BCDCA8 && bytes==8){std::memcpy(out,&memory.registryValue,8);return true;}
    if(address==recordAddress && bytes==memory.record.size()){
        std::memcpy(out,memory.record.data(),bytes);return true;
    }
    return false;
}
void SubmissionChecks(){
    namespace detail=material_read::detail;
    material_binding::Snapshot saved;
    saved.context={0x1000,0x2000,0x3000};saved.registry=registry;saved.handle=handle;
    saved.recordAddress=recordAddress;saved.record={0x900100,0x80000,0x70000,292,3};
    saved.slot.descriptor=saved.record.descriptor;
    auto fixture=[&]{Memory memory;PutRecord(memory,saved.record);return memory;};
    auto valid=[&](Memory& memory){return detail::SubmissionRecord(Read,&memory,game,saved);};
    auto memory=fixture();
    // Only the global pointer and record exist; no context, TLS or CPU shadow is readable.
    Require(valid(memory) && memory.reads==4);
    Require(detail::SubmissionHeap(D3D12_HEAP_TYPE_DEFAULT,3));
    Require(detail::NeedsBufferReceipt(D3D12_HEAP_TYPE_DEFAULT,1));
    Require(!detail::SubmissionHeap(D3D12_HEAP_TYPE_DEFAULT,1));
    Require(!detail::NeedsBufferReceipt(D3D12_HEAP_TYPE_UPLOAD,1));
    Require(!detail::NeedsBufferReceipt(D3D12_HEAP_TYPE_DEFAULT,3));
    for(const uint32_t kind:{0u,1u,2u,3u,5u,6u,15u})
        Require(detail::SubmissionHeap(D3D12_HEAP_TYPE_UPLOAD,kind));
    for(const uint32_t kind:{1u,2u,4u,5u,16u,UINT32_MAX})
        Require(!detail::SubmissionHeap(D3D12_HEAP_TYPE_DEFAULT,kind));
    for(const uint32_t kind:{4u,16u,UINT32_MAX})
        Require(!detail::SubmissionHeap(D3D12_HEAP_TYPE_UPLOAD,kind));
    for(const auto heap:{D3D12_HEAP_TYPE_READBACK,D3D12_HEAP_TYPE_CUSTOM,D3D12_HEAP_TYPE_GPU_UPLOAD})
        Require(!detail::SubmissionHeap(heap,3));
    for(uint32_t field=0;field<5;++field){
        auto changed=saved.record;
        switch(field){
            case 0:changed.resource+=8;break;
            case 1:changed.gpu+=256;break;
            case 2:++changed.bytes;break;
            case 3:changed.kind=6;break;
            case 4:changed.descriptor+=8;break;
        }
        memory=fixture();PutRecord(memory,changed);Require(!valid(memory));
    }
    memory=fixture();memory.registryValue+=8;Require(!valid(memory));
    for(const uint8_t type:{uint8_t(0),uint8_t(5),uint8_t(13)}){
        memory=fixture();PutRecord(memory,saved.record,type);Require(!valid(memory));
    }
    for(const uint32_t failure:{1u,2u,3u,4u}){
        memory=fixture();memory.failAt=failure;Require(!valid(memory));
    }
    for(const uint32_t at:{3u,4u}){
        memory=fixture();memory.changeAt=at;Require(!valid(memory));
    }
    memory=fixture();memory.changeAt=3;memory.changeRegistry=true;Require(!valid(memory));
    auto invalid=saved;invalid.recordAddress+=8;
    memory=fixture();Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    invalid=saved;invalid.slot.descriptor+=8;
    memory=fixture();Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    for(const uint32_t value:{0u,handle+1,0x100001u,UINT32_MAX}){
        invalid=saved;invalid.handle=value;memory=fixture();
        Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    }
    invalid=saved;invalid.registry=0;memory=fixture();
    Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    invalid=saved;invalid.registry=UINT64_MAX;memory=fixture();memory.registryValue=UINT64_MAX;
    Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    for(const uint32_t kind:{4u,16u,UINT32_MAX}){
        invalid=saved;invalid.record.kind=kind;memory=fixture();PutRecord(memory,invalid.record);
        Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    }
    for(const uint32_t bytes:{0u,291u}){
        invalid=saved;invalid.record.bytes=bytes;memory=fixture();PutRecord(memory,invalid.record);
        Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    }
    for(const uint64_t address:{uint64_t(0),saved.record.gpu+1,UINT64_MAX-255}){
        invalid=saved;invalid.record.gpu=address;memory=fixture();PutRecord(memory,invalid.record);
        Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    }
    invalid=saved;invalid.record.resource=0;memory=fixture();PutRecord(memory,invalid.record);
    Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    invalid=saved;invalid.record.descriptor=0;invalid.slot.descriptor=0;memory=fixture();PutRecord(memory,invalid.record);
    Require(!detail::SubmissionRecord(Read,&memory,game,invalid));
    memory=fixture();Require(!detail::SubmissionRecord(nullptr,&memory,game,saved));
    Require(!detail::SubmissionRecord(Read,&memory,0,saved));
    Require(!detail::SubmissionRecord(Read,&memory,UINT64_MAX,saved));
}
}
int main()try{
    namespace detail=material_read::detail;
    SubmissionChecks();
    uint32_t required{};
    for(const uint32_t mask:{1u,3u,0x41u,0x81u,0xc1u,0x201u,0x801u,0xac3u}){
        Require(detail::CopyReadMask({mask,2,3},D3D12_HEAP_TYPE_DEFAULT,3,required));
        Require(required==(mask|0x801u) && (required&mask)==mask);
        Require(detail::HasReadState({required,2,3},D3D12_HEAP_TYPE_DEFAULT,3,required));
        Require(detail::HasReadState({required,2,3},D3D12_HEAP_TYPE_DEFAULT,3,mask));
    }
    Require(!detail::HasReadState({1,2,3},D3D12_HEAP_TYPE_DEFAULT,3,0x801));
    Require(!detail::HasReadState({0x801,2,3},D3D12_HEAP_TYPE_DEFAULT,3,0));
    Require(!detail::HasReadState({0x801,2,3},D3D12_HEAP_TYPE_DEFAULT,3,0x800));
    for(const uint32_t mask:{0u,2u,0xc0u,0x800u,0x8c0u,0x9u,0x401u,0x801u|0x1000u})
        Require(!detail::CopyReadMask({mask,2,3},D3D12_HEAP_TYPE_DEFAULT,3,required) && !required);
    for(const uint32_t status:{0u,1u,3u,4u,5u,6u,7u,8u,9u}){
        Require(!detail::CopyReadMask({0xac3,status,3},D3D12_HEAP_TYPE_DEFAULT,3,required) && !required);
        Require(!detail::HasReadState({0xac3,status,3},D3D12_HEAP_TYPE_DEFAULT,3,0x801));
    }
    for(const uint32_t kind:{1u,2u,4u,5u,16u,UINT32_MAX})
        Require(!detail::CopyReadMask({0xac3,2,kind},D3D12_HEAP_TYPE_DEFAULT,kind,required));
    for(const uint32_t status:{8u,9u})
        Require(!detail::CopyReadMask({D3D12_RESOURCE_STATE_GENERIC_READ,status,1},D3D12_HEAP_TYPE_DEFAULT,1,required));
    Require(!detail::CopyReadMask({0x801,2,3},D3D12_HEAP_TYPE_DEFAULT,6,required));
    for(const uint32_t kind:{0u,3u,6u,15u})
        Require(detail::CopyReadMask({1,2,kind},D3D12_HEAP_TYPE_DEFAULT,kind,required) && required==0x801);
    for(const uint32_t kind:{1u,2u,3u,5u}){
        Require(detail::CopyReadMask({D3D12_RESOURCE_STATE_GENERIC_READ,1,kind},D3D12_HEAP_TYPE_UPLOAD,kind,required));
        Require(required==D3D12_RESOURCE_STATE_GENERIC_READ);
        Require(detail::HasReadState({required,1,kind},D3D12_HEAP_TYPE_UPLOAD,kind,0x801));
    }
    Require(!detail::CopyReadMask({0xac3,1,4},D3D12_HEAP_TYPE_UPLOAD,4,required));
    Require(!detail::CopyReadMask({0x801,1,3},D3D12_HEAP_TYPE_UPLOAD,3,required));
    Require(!detail::CopyReadMask({0xac3,2,3},D3D12_HEAP_TYPE_UPLOAD,3,required));
    for(const auto heap:{D3D12_HEAP_TYPE_READBACK,D3D12_HEAP_TYPE_CUSTOM,D3D12_HEAP_TYPE_GPU_UPLOAD})
        Require(!detail::CopyReadMask({0xac3,1,3},heap,3,required));
    Require(detail::RecoveryReadMask({0x800,2,3},3,0xc1,required) && required==0x8c1);
    Require(detail::RecoveryReadMask({0x41,2,3},3,0x81,required) && required==0xc1);
    Require(detail::RecoveryReadMask({0,2,3},3,1,required) && required==1);
    Require(!detail::RecoveryReadMask({0x400,2,3},3,1,required) && !required);
    Require(!detail::RecoveryReadMask({0x800,4,3},3,1,required));
    Require(!detail::RecoveryReadMask({0x800,5,3},3,1,required));
    Require(!detail::RecoveryReadMask({0x800,2,3},3,0,required));
    Require(!detail::RecoveryReadMask({0x800,2,3},3,0x401,required));
    Require(!detail::RecoveryReadMask({0x800,2,1},1,1,required));
    material_binding::Snapshot snapshot;
    snapshot.context={0x1000,0x2000,0x3000};snapshot.registry=0x4000;snapshot.handle=2;
    snapshot.record={0x900100,0x5000,0x6000,292,3};
    auto native=material_binding::detail::Native(snapshot);
    Require(detail::NativeIdentity(snapshot,native));
    native.resource+=8;Require(!detail::NativeIdentity(snapshot,native));native=material_binding::detail::Native(snapshot);
    native.gpu+=256;Require(!detail::NativeIdentity(snapshot,native));native=material_binding::detail::Native(snapshot);
    native.bytes=512;Require(!detail::NativeIdentity(snapshot,native));native=material_binding::detail::Native(snapshot);
    native.context+=8;Require(!detail::NativeIdentity(snapshot,native));native=material_binding::detail::Native(snapshot);
    native.registry+=8;Require(!detail::NativeIdentity(snapshot,native));native=material_binding::detail::Native(snapshot);
    native.handle+=1;Require(!detail::NativeIdentity(snapshot,native));native=material_binding::detail::Native(snapshot);
    native.kind=4;Require(!detail::NativeIdentity(snapshot,native));
    snapshot.record.kind=4;Require(!detail::NativeIdentity(snapshot,material_binding::detail::Native(snapshot)));
    Require(detail::SourceRange(0x900100,0x900000,548,256));
    Require(!detail::SourceRange(0x900100,0x900000,547,256));
    Require(!detail::SourceRange(0x900100,0x900000,1024,0));
    Require(!detail::SourceRange(0x8fff00,0x900000,1024,0));
    Require(!detail::SourceRange(UINT64_MAX-255,UINT64_MAX-511,1024,256));
    Require(detail::DestinationRange(292,0) && detail::DestinationRange(548,256));
    Require(!detail::DestinationRange(291,0) && !detail::DestinationRange(548,257));
    Require(!detail::DestinationRange(1024,UINT64_MAX) && !detail::DestinationRange(UINT64_MAX,UINT64_MAX-291));
    std::puts("PASS material submission registry identity, read masks, transient rejection, recovery guards and exact logical ranges");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
