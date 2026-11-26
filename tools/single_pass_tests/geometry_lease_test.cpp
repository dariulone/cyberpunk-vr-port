#include "Render/NativeGeometryLeases.hpp"
#include "Render/CommandResources.hpp"
#include <dxgi1_6.h>
#include <map>
#include <vector>
#include <cstdio>
#include <stdexcept>
namespace cvr::stereo::native_probe {bool ObserveBindings(){return true;}}
using namespace cvr::stereo::geometry;
using Microsoft::WRL::ComPtr;
static void Require(bool value,const char* why){if(!value)throw std::runtime_error(why);}
static void Check(HRESULT value){Require(SUCCEEDED(value),"D3D12 setup failed");}
using Memory=std::map<uint64_t,std::vector<uint8_t>>;
static bool Read(void* context,uint64_t address,void* data,size_t bytes) {
    for(const auto& [start,source]:*static_cast<Memory*>(context))if(address>=start && address-start<=source.size() && bytes<=source.size()-(address-start)) {
        std::memcpy(data,source.data()+address-start,bytes);return true;
    }
    return false;
}
template<class T>void Put(Memory& memory,uint64_t address,T value){auto& data=memory[address];data.resize(sizeof(value));std::memcpy(data.data(),&value,sizeof(value));}
int main() try {
    Memory memory;const uint64_t registry=0x100000;
    auto record=[&](uint32_t handle,uint64_t resource) {
        std::array<uint8_t,24> data{};const uint32_t bytes=4096;const uint64_t gpu=0x10000ull*handle;
        std::memcpy(data.data(),&bytes,4);std::memcpy(data.data()+8,&gpu,8);std::memcpy(data.data()+16,&resource,8);
        memory[registry+0x5C0AF8+uint64_t(handle-1)*0xB0]={data.begin(),data.end()};
    };
    record(2,0x2200);record(3,0x3300);record(4,0x4400);std::array<NativeBuffer,16> decoded;
    IaFrame frame{};frame.rva=0x34291C;frame.r12=registry;frame.rbx=0xB0;
    Require(DecodeIa(frame,true,0,1,Read,&memory,decoded) && decoded[0].resource==0x2200,"Packed index source wrong");
    frame.rva=0x342980;frame.r14=3;
    Require(DecodeIa(frame,false,0,3,Read,&memory,decoded) && decoded[2].resource==0x2200,"Packed vertex source wrong");
    frame={};frame.rva=0x3392C7;frame.rdi=registry;frame.rsp=0x8000;Put(memory,0x8118,uint64_t(0x9000));Put(memory,0x9000,uint32_t(3));
    Require(DecodeIa(frame,true,0,1,Read,&memory,decoded) && decoded[0].resource==0x3300,"Split index source wrong");
    frame.rva=0x339382;Put(memory,0x80F8,uint32_t(3));frame.rsi=0xA02C;frame.r12=uint64_t(0)-0x20;
    Put(memory,0xA000,uint32_t(2));Put(memory,0xA004,uint32_t(3));Put(memory,0xA008,uint32_t(4));
    Require(DecodeIa(frame,false,0,3,Read,&memory,decoded) && decoded[0].resource==0x2200 && decoded[2].resource==0x4400,"Split vertex array origin wrong");
    Require(!DecodeIa(frame,false,0,2,Read,&memory,decoded),"Native/API slot count mismatch accepted");
    frame={};frame.rva=0x339E6F;frame.rbp=registry;frame.rbx=0xB0;frame.rsi=7;
    Require(DecodeIa(frame,false,7,1,Read,&memory,decoded) && decoded[0].resource==0x2200,"Instance binding source wrong");
    Require(!DecodeIa(frame,false,6,1,Read,&memory,decoded),"Wrong instance slot accepted");
    frame.rbx++;Require(!DecodeIa(frame,false,7,1,Read,&memory,decoded),"Misaligned record accepted");
    frame.rva=0;Require(!DecodeIa(frame,false,0,1,Read,&memory,decoded),"Unknown call site accepted");

    NativeBuffer stateSource{0x2200,0x20000,4096,registry,0x2000000,2,0};
    const uint64_t nativeList=0xABC000,buckets=0x2100000,entries=0x2200000;
    Put(memory,stateSource.context+0x30,nativeList);Put(memory,stateSource.context+0x534,uint32_t(0));
    struct StateMap {uint64_t buckets;uint32_t count,bucketCount;uint64_t data;uint32_t capacity,stride;};
    Put(memory,stateSource.context+0x568,StateMap{buckets,2,4,entries,4,16});
    Put(memory,buckets+8,uint32_t(0));Put(memory,entries,std::array<uint32_t,4>{1,6,6,0x400});Put(memory,entries+16,std::array<uint32_t,4>{UINT_MAX,2,2,3});
    auto state=ReadNativeState(stateSource,nativeList,Read,&memory);Require(state.status==2 && state.value==3,"Native state collision lookup failed");
    Require(ReadNativeState(stateSource,nativeList+1,Read,&memory).status==3,"Foreign command context accepted");
    Put(memory,stateSource.context+0x534,uint32_t(1));Require(ReadNativeState(stateSource,nativeList,Read,&memory).status==4,"Unflushed barriers ignored");Put(memory,stateSource.context+0x534,uint32_t(0));
    Put(memory,entries+16,std::array<uint32_t,4>{UINT_MAX,2,2,0x80000000});Require(ReadNativeState(stateSource,nativeList,Read,&memory).status==5,"Uninitialized map value accepted");
    Put(memory,entries,std::array<uint32_t,4>{0,6,6,0x400});Require(ReadNativeState(stateSource,nativeList,Read,&memory).status==5,"Map cycle did not terminate");
    stateSource.resource++;Require(ReadNativeState(stateSource,nativeList,Read,&memory).status==3,"Reused native handle accepted");

    ComPtr<IDXGIFactory4> factory;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> adapter;Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Check(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
    D3D12_HEAP_PROPERTIES properties{};properties.Type=D3D12_HEAP_TYPE_UPLOAD;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=4096;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> resource;Check(device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&resource)));
    const auto address=resource->GetGPUVirtualAddress();NativeBuffer native{reinterpret_cast<uintptr_t>(resource.Get()),address+256,1024};
    auto lease=AcquireBuffer(native,address+512,256);Require(lease.resource && lease.gpu==address && lease.bytes==4096,"Suballocated view rejected");
    Require(!AcquireBuffer(native,address+255,256).resource && !AcquireBuffer(native,address+1024,512).resource,"Native range expanded");
    native.gpu=address+4096;Require(!AcquireBuffer(native,address+4096,256).resource,"Resource bounds ignored");
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> commands;
    Check(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
    Check(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&commands)));
    Begin(true);D3D12_INDEX_BUFFER_VIEW index{address+512,256,DXGI_FORMAT_R16_UINT};
    D3D12_VERTEX_BUFFER_VIEW vertex{address+512,256,16};IndexLease(commands.Get(),&index,lease);
    std::array<BufferLease,16> leases{};leases[0]=lease;VertexLeases(commands.Get(),0,1,&vertex,leases);
    cvr::stereo::native_probe::DrawRecord draw{};draw.sequence=1;draw.frame=1;draw.side=1;draw.list=reinterpret_cast<uintptr_t>(commands.Get());
    draw.index=index;draw.indices=36;draw.flags=128;draw.inputSlotMask=1;draw.vertices[0]=vertex;Snapshot(draw);
    DrawResources owned;Require(CopyDraw(1,owned) && owned.index.resource && owned.vertices[0].resource,"Complete geometry lease not owned");
    Require(CyberpunkVR_GeometryLeaseResourceCount==2,"Shared IB/VB resource was retained twice");
    Vertices(commands.Get(),16,16,nullptr);Require(!CyberpunkVR_GeometryLeaseFailure,"Valid high IA slots poisoned supported geometry");
    draw.sequence=2;draw.inputSlotMask=3;Snapshot(draw);Require(!CopyDraw(2,owned),"Missing used vertex stream accepted");
    draw.sequence=3;draw.inputSlotMask=1;draw.firstIndex=128;Snapshot(draw);Require(!CopyDraw(3,owned),"Index range overflow accepted");
    VertexLeases(commands.Get(),0,32,nullptr,{});draw.sequence=4;draw.firstIndex=0;Snapshot(draw);Require(!CopyDraw(4,owned),"32-slot clear left stale vertex leases");
    Reset(commands.Get());draw.sequence=5;Snapshot(draw);Require(!CopyDraw(5,owned),"Reset left stale bound leases");
    const auto publishedDraws=CyberpunkVR_GeometryLeaseDrawCount.load(),publishedSeq=CyberpunkVR_GeometryLeaseSeq.load();
    BeginBindings();draw.index=index;draw.vertices[0]=vertex;draw.inputSlotMask=1;draw.flags=128;
    IndexLease(commands.Get(),&index,lease);leases[0]=lease;VertexLeases(commands.Get(),0,1,&vertex,leases);
    DrawResources current;
    Require(CopyCurrentBindings(commands.Get(),draw,true,current) && current.index.resource==resource && current.vertices[0].resource==resource,
        "Live indirect IA bindings required a diagnostic draw sequence");
    auto wrong=draw;wrong.list++;Require(!CopyCurrentBindings(commands.Get(),wrong,true,current),"Foreign list IA receipt accepted");
    wrong=draw;wrong.flags=0;Require(!CopyCurrentBindings(commands.Get(),wrong,true,current),"Unknown current PSO input mask accepted");
    wrong=draw;wrong.inputSlotMask|=1u<<16;Require(!CopyCurrentBindings(commands.Get(),wrong,true,current),"Unsupported consumed IA slot accepted");
    wrong=draw;wrong.inputSlotMask|=2;Require(!CopyCurrentBindings(commands.Get(),wrong,true,current),"Missing consumed current VB accepted");
    wrong=draw;wrong.index.BufferLocation+=2;Require(!CopyCurrentBindings(commands.Get(),wrong,true,current),"Stale current IB view accepted");
    wrong=draw;wrong.vertices[0].BufferLocation+=16;Require(!CopyCurrentBindings(commands.Get(),wrong,true,current),"Stale current VB view accepted");
    Require(current.index.resource==resource && current.vertices[0].resource==resource,"Rejected live capture replaced prior output");
    IndexLease(commands.Get(),nullptr,{});
    Require(!CopyCurrentBindings(commands.Get(),draw,true,current) && CopyCurrentBindings(commands.Get(),draw,false,current) && !current.index.resource,
        "Non-indexed indirect DRAW required an IB or indexed draw accepted a missing IB");
    wrong=draw;wrong.inputSlotMask=0;
    Require(CopyCurrentBindings(commands.Get(),wrong,false,current) && !current.vertices[0].resource,
        "Procedural non-indexed DRAW required an unused vertex stream");
    Require(CopyCurrentBindings(commands.Get(),draw,false,current),"Live bound VB capture did not recover after procedural DRAW");
    Snapshot(draw);Require(CyberpunkVR_GeometryLeaseDrawCount==publishedDraws && CyberpunkVR_GeometryLeaseSeq==publishedSeq,
        "Live IA capture published diagnostic draws or copied instances");
    Reset(commands.Get());Require(!CopyCurrentBindings(commands.Get(),draw,false,current),"Live IA recording Reset left stale leases");
    EndBindings();Require(!CopyCurrentBindings(commands.Get(),draw,false,current) && current.vertices[0].resource==resource,
        "Live capture remained active or its handed-off owners expired after EndBindings");
    End();resource.Reset();lease={};leases={};Require(CopyDraw(1,owned) && owned.index.resource->GetDesc().Width==4096,"Published lease expired after native references were released");
    Require(cvr::gpu::HasCommandResource(commands.Get(),owned.index.resource.Get()),"Recording did not retain its bound geometry");
    Check(commands->Close());cvr::gpu::ResetCommandResources(commands.Get());Begin(false);
    Require(!CyberpunkVR_GeometryLeaseDrawCount && !CyberpunkVR_GeometryLeaseFailure,"Geometry capture did not reset");
    std::puts("PASS native IA decoding, exact bounds, owned geometry, shared-resource deduplication and Reset");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
