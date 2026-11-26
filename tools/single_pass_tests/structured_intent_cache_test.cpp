#include "Render/SourceResourceIntent.hpp"
#include <dxgi1_6.h>
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <new>
#include <stdexcept>
#include <vector>

namespace {
struct alignas(std::max_align_t) AllocationHeader {size_t bytes;};
std::atomic<bool> watchCapacityRelease{};
std::atomic<size_t> largestRelease{};
}
// Observe the metadata allocation actually being released, without timing or
// adding a production diagnostic field for vector capacity.
void* operator new(size_t bytes){
    if(bytes>SIZE_MAX-sizeof(AllocationHeader))throw std::bad_alloc();
    if(auto* memory=std::malloc(sizeof(AllocationHeader)+(bytes?bytes:1))){auto* header=static_cast<AllocationHeader*>(memory);header->bytes=bytes;return header+1;}
    throw std::bad_alloc();
}
void operator delete(void* memory)noexcept{if(!memory)return;auto* header=static_cast<AllocationHeader*>(memory)-1;
    if(watchCapacityRelease.load(std::memory_order_relaxed)){auto previous=largestRelease.load(std::memory_order_relaxed);
        while(previous<header->bytes && !largestRelease.compare_exchange_weak(previous,header->bytes,std::memory_order_relaxed)){} }
    std::free(header);
}
void operator delete(void* memory,size_t)noexcept{::operator delete(memory);}
void* operator new[](size_t bytes){return ::operator new(bytes);}
void operator delete[](void* memory)noexcept{::operator delete(memory);}
void operator delete[](void* memory,size_t)noexcept{::operator delete(memory);}

using Microsoft::WRL::ComPtr;
namespace intent=cvr::stereo::source_resource_intent;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
static void Check(HRESULT value){if(FAILED(value))throw std::runtime_error("WARP fixture API failed");}
static std::vector<uint8_t> Pattern(size_t bytes,uint8_t seed){std::vector<uint8_t> result(bytes);
    for(size_t index=0;index<bytes;++index)result[index]=uint8_t(seed+index*17);return result;}
static bool Equal(std::span<const uint8_t> a,std::span<const uint8_t> b){return a.size()==b.size() && std::equal(a.begin(),a.end(),b.begin());}
static D3D12_RESOURCE_DESC Description(uint64_t bytes){D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width=bytes;desc.Height=desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;return desc;}
static D3D12_SHADER_RESOURCE_VIEW_DESC Structured(uint64_t bytes,uint64_t first=0){D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    view.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;view.Format=DXGI_FORMAT_UNKNOWN;view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    view.Buffer.FirstElement=first;view.Buffer.NumElements=UINT(bytes/128);view.Buffer.StructureByteStride=128;return view;}
struct Fixture {
    ComPtr<ID3D12Device> device;ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;
    Fixture(){ComPtr<IDXGIFactory4> factory;ComPtr<IDXGIAdapter> warp;Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
        Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp)));Check(D3D12CreateDevice(warp.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)));
        Check(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_COPY,IID_PPV_ARGS(&allocator)));
        Check(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_COPY,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));}
    ComPtr<ID3D12Resource> Buffer(uint64_t bytes,D3D12_HEAP_TYPE type=D3D12_HEAP_TYPE_DEFAULT){D3D12_HEAP_PROPERTIES heap{};heap.Type=type;
        const auto desc=Description(bytes);ComPtr<ID3D12Resource> result;Check(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,
            type==D3D12_HEAP_TYPE_UPLOAD?D3D12_RESOURCE_STATE_GENERIC_READ:D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&result)));return result;}
    void Write(ID3D12Resource* resource,uint64_t offset,std::span<const uint8_t> bytes){void* mapped{};const D3D12_RANGE none{};
        Check(resource->Map(0,&none,&mapped));Require(mapped!=nullptr,"UPLOAD fixture map failed");
        std::memcpy(static_cast<uint8_t*>(mapped)+offset,bytes.data(),bytes.size());const D3D12_RANGE written{SIZE_T(offset),SIZE_T(offset+bytes.size())};resource->Unmap(0,&written);}
    bool Observe(ID3D12Resource* destination,uint64_t offset,ID3D12Resource* source,uint64_t sourceOffset,size_t bytes){
        return intent::ObserveCopy(list.Get(),destination,offset,source,sourceOffset,bytes);}
    bool Capture(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,intent::Candidate& output){
        return intent::CaptureStructuredAtDraw(resource,resource->GetGPUVirtualAddress()+offset,bytes,output);}
    bool Generic(const ComPtr<ID3D12Resource>& resource,uint64_t offset,size_t bytes,intent::Candidate& output){
        return intent::CaptureAtDraw(resource,resource->GetGPUVirtualAddress()+offset,bytes,output);}
};
static void FullWriteAndUnrelatedTail(Fixture& fixture){
    intent::Clear();constexpr size_t size=1024*1024;auto destination=fixture.Buffer(size),source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);
    auto data=Pattern(size,41);fixture.Write(source.Get(),0,data);const auto view=Structured(size);
    Require(intent::MarkStructuredView(destination.Get(),&view) && fixture.Observe(destination.Get(),0,source.Get(),0,size),"Full structured write failed");
    intent::Candidate original,current,generic;Require(fixture.Capture(destination,436736,2048,original),"Original selected span missing");
    const auto sequence=original.CopySequence();Require(!original.IsComposed() && original.Pieces().size()==1 && original.CopiedBytes()==size,
        "One original copy was mislabeled as composition");
    const auto tail=Pattern(2560,99);fixture.Write(source.Get(),0,tail);
    Require(fixture.Observe(destination.Get(),1044224,source.Get(),0,tail.size()),"Unrelated tail update failed");
    Require(fixture.Capture(destination,436736,2048,current) && Equal(current.Bytes(),std::span<const uint8_t>(data).subspan(436736,2048)) &&
        current.CopySequence()==sequence,"Unrelated tail erased the required current structured span");
    Require(!fixture.Generic(destination,436736,2048,generic),"Generic capture reconstructed a discarded complete copy");
    Require(Equal(original.Bytes(),current.Bytes()),"Tail update changed an immutable snapshot");
    original={};current={};
    const auto fragmentData=Pattern(16,7);fixture.Write(source.Get(),0,fragmentData);
    for(size_t index=0;index<intent::MaxRangesPerResource+32;++index)
        Require(fixture.Observe(destination.Get(),800000+index*16,source.Get(),0,fragmentData.size()),"Structured update hit the old generic range cap");
    Require(fixture.Capture(destination,436736,2048,current) && current.CopySequence()==sequence &&
        Equal(current.Bytes(),std::span<const uint8_t>(data).subspan(436736,2048)),"Generic range eviction erased unrelated structured coverage");
    const auto stats=intent::GetStatistics();Require(stats.rangeCount<=intent::MaxGenericRangesPerResource && stats.structuredBytes==size &&
        stats.structuredRanges>intent::MaxRangesPerResource && stats.livePayloadBytes<=intent::MaxResourceBytes,"Structured coverage bounds/accounting failed");
}
static void AdjacentUpdatesAndReceipts(Fixture& fixture){
    intent::Clear();auto destination=fixture.Buffer(512),source=fixture.Buffer(512,D3D12_HEAP_TYPE_UPLOAD);const auto view=Structured(512);
    const auto first=Pattern(128,3),second=Pattern(128,81);fixture.Write(source.Get(),0,first);fixture.Write(source.Get(),256,second);
    Require(intent::MarkStructuredView(destination.Get(),&view) && fixture.Observe(destination.Get(),0,source.Get(),0,first.size()) &&
        fixture.Observe(destination.Get(),128,source.Get(),256,second.size()),"Adjacent observed writes failed");
    intent::Candidate single,composed,generic;Require(fixture.Capture(destination,0,8,single),"Single-copy receipt missing");
    Require(fixture.Capture(destination,120,16,composed) && composed.IsComposed() && composed.Pieces().size()==2,
        "Cross-copy view was not explicitly composed");
    std::vector<uint8_t> expected(first.end()-8,first.end());expected.insert(expected.end(),second.begin(),second.begin()+8);
    Require(Equal(composed.Bytes(),expected) && !composed.CopySequence() && !composed.ProducerList() && !composed.CopiedBytes() &&
        !composed.DestinationOffset() && !composed.SourceOffset() && !composed.ViewOffset(),"Composed snapshot invented a single-copy receipt");
    const auto pieces=composed.Pieces();Require(pieces[0].copySequence==single.CopySequence() && pieces[1].copySequence>pieces[0].copySequence &&
        pieces[0].producerList==reinterpret_cast<uintptr_t>(fixture.list.Get()) && pieces[1].sourceResource==reinterpret_cast<uintptr_t>(source.Get()) &&
        pieces[0].destinationOffset==0 && pieces[0].sourceOffset==0 && pieces[0].copiedBytes==128 && pieces[0].pieceDestinationOffset==120 &&
        pieces[0].pieceSourceOffset==120 && pieces[0].pieceBytes==8 && pieces[1].destinationOffset==128 && pieces[1].sourceOffset==256 &&
        pieces[1].pieceDestinationOffset==128 && pieces[1].pieceSourceOffset==256 && pieces[1].pieceBytes==8,"Actual piece provenance changed");
    Require(!fixture.Generic(destination,120,16,generic),"Generic capture composed adjacent copies");single={};
    intent::Clear();auto stats=intent::GetStatistics();Require(!stats.currentBytes && !stats.rangeCount && !stats.structuredBytes && !stats.structuredRanges &&
        stats.livePayloadBytes==16 && stats.livePayloads==1 && Equal(composed.Bytes(),expected),"Clear lost retained composed-byte accounting");
    composed={};Require(!intent::GetStatistics().livePayloadBytes,"Composed snapshot release leaked bytes");
}
static void HolesUnknownWritesAndPartialOverwrite(Fixture& fixture){
    intent::Clear();auto destination=fixture.Buffer(4096),source=fixture.Buffer(4096,D3D12_HEAP_TYPE_UPLOAD),unknown=fixture.Buffer(4096);
    const auto view=Structured(4096);const auto full=Pattern(4096,21);fixture.Write(source.Get(),0,full);
    Require(intent::MarkStructuredView(destination.Get(),&view) && fixture.Observe(destination.Get(),0,source.Get(),0,full.size()),"Hole fixture full write failed");
    intent::Candidate retained,current;Require(fixture.Capture(destination,500,160,retained),"Hole fixture retained candidate missing");
    Require(!fixture.Observe(destination.Get(),512,unknown.Get(),0,128),"Unknown DEFAULT writer was sampled");
    Require(fixture.Capture(destination,500,12,current) && fixture.Capture(destination,640,20,current) &&
        !fixture.Capture(destination,500,160,current) && !fixture.Capture(destination,512,1,current),"Unknown writer left stale holes or erased untouched bytes");
    const auto patch=Pattern(64,95);fixture.Write(source.Get(),0,patch);
    Require(fixture.Observe(destination.Get(),512,source.Get(),0,64) && !fixture.Capture(destination,500,160,current),"Partial fill concealed an unobserved hole");
    Require(fixture.Observe(destination.Get(),576,source.Get(),0,64) && fixture.Capture(destination,500,160,current) && current.IsComposed(),
        "Fully observed adjacent patches could not fill the exact hole");
    std::vector<uint8_t> expected(full.begin()+500,full.begin()+512);expected.insert(expected.end(),patch.begin(),patch.end());
    expected.insert(expected.end(),patch.begin(),patch.end());expected.insert(expected.end(),full.begin()+640,full.begin()+660);
    Require(Equal(current.Bytes(),expected) && Equal(retained.Bytes(),std::span<const uint8_t>(full).subspan(500,160)),"Partial overwrite or retained snapshot bytes changed");
    intent::Invalidate(destination.Get(),600,8);Require(!fixture.Capture(destination,596,16,current) && fixture.Capture(destination,608,16,current),
        "Explicit partial invalidation did not preserve exact nonoverwritten coverage");
    Require(!fixture.Observe(destination.Get(),700,source.Get(),4090,16) && !fixture.Capture(destination,700,1,current) &&
        fixture.Capture(destination,716,16,current),"Rejected source bounds retained overwritten stale bytes");
    intent::Invalidate(destination.Get(),UINT64_MAX,8);Require(!fixture.Capture(destination,0,1,current),"Overflow writer preserved coverage");
}
static void GenericAndDirectRecordStaySeparate(Fixture& fixture){
    intent::Clear();auto destination=fixture.Buffer(4096),source=fixture.Buffer(4096,D3D12_HEAP_TYPE_UPLOAD);const auto view=Structured(4096);
    const auto data=Pattern(128,31),patch=Pattern(8,88);intent::Candidate candidate;
    Require(intent::MarkStructuredView(destination.Get(),&view) && intent::RecordIntendedCopy(fixture.list.Get(),destination.Get(),0,0,data),"Direct copy precondition failed");
    Require(fixture.Generic(destination,0,128,candidate) && !fixture.Capture(destination,0,1,candidate),"Direct CPU label became structured observed coverage");
    fixture.Write(source.Get(),0,data);Require(fixture.Observe(destination.Get(),0,source.Get(),0,128),"Observed full copy failed");
    Require(intent::RecordIntendedCopy(fixture.list.Get(),destination.Get(),32,0,patch),"Direct partial copy failed");
    Require(!fixture.Generic(destination,0,8,candidate) && fixture.Generic(destination,32,8,candidate) &&
        fixture.Capture(destination,0,32,candidate) && fixture.Capture(destination,40,16,candidate) && !fixture.Capture(destination,32,1,candidate),
        "Direct record changed generic semantics or fabricated structured observations");
    Require(!fixture.Capture(destination,0,intent::MaxCopyBytes+1,candidate),"Structured view exceeded 64KiB capture limit");
}
static void FreshMarkerIdentityAndGate(Fixture& fixture){
    intent::Clear();constexpr size_t size=65536;D3D12_HEAP_DESC heapDesc{};heapDesc.SizeInBytes=size;heapDesc.Properties.Type=D3D12_HEAP_TYPE_DEFAULT;
    heapDesc.Flags=D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS;ComPtr<ID3D12Heap> heap;Check(fixture.device->CreateHeap(&heapDesc,IID_PPV_ARGS(&heap)));
    const auto desc=Description(size);ComPtr<ID3D12Resource> a,b;Check(fixture.device->CreatePlacedResource(heap.Get(),0,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&a)));
    Check(fixture.device->CreatePlacedResource(heap.Get(),0,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&b)));
    Require(a.Get()!=b.Get() && a->GetGPUVirtualAddress()==b->GetGPUVirtualAddress(),"Same-GPUVA alias fixture failed");
    auto source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);const auto data=Pattern(128,46);fixture.Write(source.Get(),0,data);const auto view=Structured(size);
    std::atomic<uint32_t> enabled{};intent::Candidate candidate;
    Require(!intent::MarkStructuredView(a.Get(),&view,&enabled) && !intent::ObserveCopy(fixture.list.Get(),a.Get(),0,source.Get(),0,128,&enabled),
        "Disabled callbacks populated structured coverage");enabled=1;
    Require(intent::MarkStructuredView(a.Get(),&view,&enabled) && intent::MarkStructuredView(b.Get(),&view,&enabled) &&
        intent::ObserveCopy(fixture.list.Get(),a.Get(),0,source.Get(),0,128,&enabled) && fixture.Capture(a,0,128,candidate),"Exact marked alias failed");
    Require(!intent::CaptureStructuredAtDraw(b,a->GetGPUVirtualAddress(),128,candidate),"Same-GPUVA alias inherited foreign coverage");
    auto bad=view;bad.Buffer.StructureByteStride=64;Require(!intent::MarkStructuredView(a.Get(),&bad) && !fixture.Capture(a,0,1,candidate),"Invalid marker retained usable coverage");
    Require(intent::MarkStructuredView(a.Get(),&view) && !fixture.Capture(a,0,1,candidate),"Renewed marker revived discarded observations");
    Require(fixture.Observe(a.Get(),0,source.Get(),0,128) && fixture.Capture(a,0,128,candidate),"Renewed marker could not observe new bytes");
    candidate={};intent::Clear();Require(!fixture.Capture(a,0,1,candidate),"Prior-generation marker revived coverage");
    Require(fixture.Observe(a.Get(),0,source.Get(),0,128) && !fixture.Capture(a,0,1,candidate),"Unmarked small copy became structured coverage");
    enabled=0;Require(!intent::MarkStructuredView(a.Get(),&view,&enabled),"Disabled marker renewed a generation");
}
static void BudgetCompactionAndImmutableAccounting(Fixture& fixture){
    intent::Clear();const size_t size=intent::MaxResourceBytes;auto destination=fixture.Buffer(size),source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);
    const auto data=Pattern(size,71);fixture.Write(source.Get(),0,data);const auto view=Structured(size);
    Require(intent::MarkStructuredView(destination.Get(),&view) && fixture.Observe(destination.Get(),0,source.Get(),0,size),"Maximum structured copy failed");
    const auto tail=Pattern(256,18);fixture.Write(source.Get(),0,tail);
    Require(fixture.Observe(destination.Get(),size-256,source.Get(),0,256),"Unused backing bytes were not compacted to fit the budget");
    auto stats=intent::GetStatistics();Require(stats.livePayloadBytes==size && stats.structuredBytes==size,"Compacted backing violated exact per-resource budget");
    intent::Candidate candidate;Require(fixture.Capture(destination,512,32,candidate) && candidate.CopiedBytes()==size && candidate.Pieces()[0].copiedBytes==size &&
        candidate.ViewOffset()==512 && Equal(candidate.Bytes(),std::span<const uint8_t>(data).subspan(512,32)),"Compaction changed original copy labels or bytes");
    candidate={};Require(!fixture.Capture(destination,size-264,16,candidate),"Composed snapshot bypassed the full retained-byte budget");
    intent::Clear();fixture.Write(source.Get(),0,data);Require(intent::MarkStructuredView(destination.Get(),&view) &&
        fixture.Observe(destination.Get(),0,source.Get(),0,size) && fixture.Capture(destination,512,32,candidate),"Maximum immutable budget fixture failed");
    intent::Clear();stats=intent::GetStatistics();Require(stats.livePayloadBytes==size && !stats.structuredBytes && !stats.ownerCount,"Clear lost immutable backing accounting");
    Require(intent::MarkStructuredView(destination.Get(),&view) && !fixture.Observe(destination.Get(),0,source.Get(),0,size),"Retained immutable bytes exceeded per-resource budget");
    Require(Equal(candidate.Bytes(),std::span<const uint8_t>(data).subspan(512,32)),"Budget rejection mutated a captured snapshot");candidate={};
    Require(fixture.Observe(destination.Get(),0,source.Get(),0,size),"Released captured bytes kept the budget saturated");intent::Clear();
    Require(!intent::GetStatistics().livePayloadBytes,"Budget fixture cleanup leaked backing bytes");
}
static void GlobalBoundAndResourceLifetime(Fixture& fixture){
    intent::Clear();const size_t size=intent::MaxResourceBytes;auto source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);const auto data=Pattern(size,11);
    fixture.Write(source.Get(),0,data);const auto view=Structured(size);std::vector<ComPtr<ID3D12Resource>> resources;
    for(size_t index=0;index<intent::MaxGlobalBytes/size+1;++index){resources.push_back(fixture.Buffer(size));
        Require(intent::MarkStructuredView(resources.back().Get(),&view) && fixture.Observe(resources.back().Get(),0,source.Get(),0,size),"Global structured cache saturated permanently");}
    auto stats=intent::GetStatistics();Require(stats.livePayloadBytes==intent::MaxGlobalBytes && stats.structuredBytes==intent::MaxGlobalBytes &&
        stats.structuredRanges<=intent::MaxPayloads && stats.evictions,"Structured global bound failed");intent::Candidate candidate;
    Require(!fixture.Capture(resources.front(),0,32,candidate) && fixture.Capture(resources.back(),0,32,candidate),"Global eviction lost the latest structured resource");
    auto* held=candidate.Destination();resources.clear();intent::Clear();Require(candidate.Destination()==held &&
        Equal(candidate.Bytes(),std::span<const uint8_t>(data).first(32)),"Candidate/resource lifetime was not external to the private owner");
    candidate={};stats=intent::GetStatistics();Require(!stats.livePayloadBytes && !stats.livePayloads && !stats.ownerCount &&
        !stats.structuredBytes && !stats.structuredRanges,"Private structured owner release leaked cache data");
}
static void UntypedTrafficPreservesStructuredCoverage(Fixture& fixture){
    intent::Clear();constexpr size_t size=1024*1024;
    auto destination=fixture.Buffer(size),source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);const auto data=Pattern(size,74);const auto view=Structured(size);
    fixture.Write(source.Get(),0,data);Require(intent::MarkStructuredView(destination.Get(),&view) &&
        fixture.Observe(destination.Get(),0,source.Get(),0,size),"Typed global eviction precondition failed");
    intent::Candidate selected,generic;Require(fixture.Capture(destination,436736,2048,selected),"Typed original selected span missing");
    const auto sequence=selected.CopySequence();selected={};
    const auto before=intent::GetStatistics().evictions;
    constexpr size_t count=intent::MaxGlobalBytes/intent::MaxResourceBytes+1;
    static_assert(count*intent::MaxResourceBytes>intent::MaxGlobalBytes);
    std::vector<ComPtr<ID3D12Resource>> resources;
    for(size_t index=0;index<count;++index){resources.push_back(fixture.Buffer(intent::MaxResourceBytes));
        for(size_t offset=0;offset<intent::MaxResourceBytes;offset+=intent::MaxCopyBytes)
            Require(fixture.Observe(resources.back().Get(),offset,source.Get(),0,intent::MaxCopyBytes),"Untyped traffic could not admit a bounded observed copy");
        const auto stats=intent::GetStatistics();Require(stats.currentBytes<=intent::MaxGlobalBytes && stats.livePayloadBytes<=intent::MaxGlobalBytes &&
            stats.rangeCount<=intent::MaxPayloads && stats.structuredBytes==size && stats.structuredRanges==1,"Untyped pressure violated typed coverage or global bounds");}
    const auto stats=intent::GetStatistics();Require(stats.evictions>before && stats.currentBytes==intent::MaxGlobalBytes &&
        stats.livePayloadBytes==intent::MaxGlobalBytes,"Global priority did not reclaim bounded untyped payloads");
    intent::StructuredCoverageDiagnostic coverage;Require(intent::QueryStructuredCoverage(destination,436736,2048,coverage) &&
        coverage.markerFresh && coverage.ownerHintMatchesMarker && coverage.structuredBytes==size && coverage.structuredRanges==1 &&
        coverage.coveredBytes==2048 && coverage.firstHoleOffset==UINT64_MAX,"Untyped traffic erased the current exact typed span");
    Require(fixture.Capture(destination,436736,2048,selected) && selected.CopySequence()==sequence &&
        Equal(selected.Bytes(),std::span<const uint8_t>(data).subspan(436736,2048)),"Typed coverage was replaced or revived from historical bytes");
    Require(!fixture.Generic(resources.front(),0,intent::MaxCopyBytes,generic) &&
        fixture.Generic(resources.back(),intent::MaxResourceBytes-intent::MaxCopyBytes,intent::MaxCopyBytes,generic) &&
        Equal(generic.Bytes(),std::span<const uint8_t>(data).first(intent::MaxCopyBytes)),"Global priority did not evict old generic copies and retain new ones");
    intent::Clear();const auto cleared=intent::GetStatistics();Require(!cleared.currentBytes && !cleared.rangeCount && !cleared.structuredBytes &&
        !cleared.structuredRanges && cleared.livePayloadBytes==size+intent::MaxCopyBytes,"Priority eviction lost immutable capture accounting on Clear");
    selected={};generic={};resources.clear();Require(!intent::GetStatistics().livePayloadBytes,"Priority eviction fixture leaked immutable payload bytes");
}
static void ClearReleasesMetadataCapacity(Fixture& fixture){
    intent::Clear();constexpr size_t size=1024*1024,holes=1024;
    auto destination=fixture.Buffer(size),source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);const auto data=Pattern(size,53);const auto view=Structured(size);
    fixture.Write(source.Get(),0,data);Require(intent::MarkStructuredView(destination.Get(),&view) &&
        fixture.Observe(destination.Get(),0,source.Get(),0,size),"Capacity cleanup full write failed");
    intent::Candidate retained;Require(fixture.Capture(destination,0,1,retained),"Capacity cleanup retained payload missing");
    for(size_t index=0;index<holes;++index)intent::Invalidate(destination.Get(),index*256+1,1);
    auto stats=intent::GetStatistics();Require(stats.structuredRanges==holes+1 && stats.structuredBytes==size-holes,
        "Capacity cleanup fragmentation precondition failed");
#if CVR_RUST_VRIK
    // The Rust cache's allocations do not pass through this program's operator new: its unit test
    // (rust/crates/cvr-render/src/source_intent.rs, empty_releases_structured_capacity) holds Clear's release instead.
    intent::Clear();
#else
    largestRelease=0;watchCapacityRelease=true;intent::Clear();watchCapacityRelease=false;
    Require(largestRelease.load()>=stats.structuredRanges*sizeof(uint64_t),"Clear retained the structured metadata allocation");
#endif
    stats=intent::GetStatistics();Require(!stats.currentBytes && !stats.rangeCount && !stats.structuredBytes && !stats.structuredRanges &&
        stats.livePayloadBytes==size && Equal(retained.Bytes(),std::span<const uint8_t>(data).first(1)),"Capacity cleanup changed retained bytes or counts");
    Require(intent::MarkStructuredView(destination.Get(),&view) && fixture.Observe(destination.Get(),0,source.Get(),0,128),"Released metadata storage could not resume");
    intent::Candidate current;Require(fixture.Capture(destination,0,128,current) && !fixture.Capture(destination,4096,1,current),
        "Released metadata storage revived previous coverage");retained={};intent::Clear();
    Require(!intent::GetStatistics().livePayloadBytes,"Capacity cleanup leaked immutable data");
}
static void InterleavedPayloadEviction(Fixture& fixture){
    intent::Clear();constexpr size_t size=1024*1024,updates=1024,patchBytes=128;
    auto destination=fixture.Buffer(intent::MaxResourceBytes),source=fixture.Buffer(size,D3D12_HEAP_TYPE_UPLOAD);
    const auto data=Pattern(size,67),patch=Pattern(patchBytes,92),extra=Pattern(intent::MaxCopyBytes,24);const auto view=Structured(size);
    fixture.Write(source.Get(),0,data);Require(intent::MarkStructuredView(destination.Get(),&view) &&
        fixture.Observe(destination.Get(),0,source.Get(),0,size),"Interleaved eviction full write failed");
    intent::Candidate retained,current;Require(fixture.Capture(destination,128,16,retained),"Interleaved eviction immutable payload missing");
    fixture.Write(source.Get(),0,patch);
    for(size_t index=0;index<updates;++index)Require(fixture.Observe(destination.Get(),index*256,source.Get(),0,patch.size()),"Interleaved update failed");
    Require(intent::GetStatistics().structuredRanges==2*updates,"Interleaved slices were not established");
    const auto before=intent::GetStatistics().evictions;
    for(size_t index=0;index<15;++index)Require(intent::RecordIntendedCopy(fixture.list.Get(),destination.Get(),size+index*extra.size(),0,extra),
        "Interleaved payload pressure failed to admit a bounded copy");
    auto stats=intent::GetStatistics();Require(stats.evictions>=before+updates/2+1 && stats.structuredRanges==updates/2 &&
        stats.structuredBytes==(updates/2)*patchBytes && stats.livePayloadBytes==intent::MaxResourceBytes,"Linear payload eviction miscounted interleaved ranges");
    Require(!fixture.Capture(destination,128,1,current) && !fixture.Capture(destination,0,1,current) &&
        fixture.Capture(destination,(updates-1)*256,patchBytes,current) && Equal(current.Bytes(),patch),"Payload eviction erased newer copies or retained old fragments");
    Require(Equal(retained.Bytes(),std::span<const uint8_t>(data).subspan(128,16)),"Payload eviction mutated the captured old copy");
    current={};retained={};intent::Clear();Require(!intent::GetStatistics().livePayloadBytes && !intent::GetStatistics().structuredBytes,
        "Interleaved eviction cleanup leaked coverage or bytes");
}
static void CurrentCoverageDiagnostics(Fixture& fixture){
    intent::Clear();auto destination=fixture.Buffer(4096),other=fixture.Buffer(4096),source=fixture.Buffer(4096,D3D12_HEAP_TYPE_UPLOAD);
    intent::StructuredCoverageDiagnostic diagnostic;const auto view=Structured(4096);const auto data=Pattern(4096,36);fixture.Write(source.Get(),0,data);
    auto stats=intent::GetStatistics();Require(intent::QueryStructuredCoverage(other,32,64,diagnostic) && diagnostic.version &&
        diagnostic.resource==reinterpret_cast<uintptr_t>(other.Get()) && !diagnostic.ownerPresent && !diagnostic.markerFresh &&
        !diagnostic.structuredBytes && !diagnostic.structuredRanges && !diagnostic.coveredBytes && diagnostic.firstHoleOffset==32,
        "Nonowner coverage query created or inherited current coverage");
    Require(intent::GetStatistics().ownerCount==stats.ownerCount && intent::GetStatistics().livePayloadBytes==stats.livePayloadBytes,
        "Read-only nonowner query allocated a payload or private owner");
    Require(fixture.Observe(destination.Get(),0,source.Get(),0,128) && intent::MarkStructuredView(destination.Get(),&view) &&
        intent::QueryStructuredCoverage(destination,0,128,diagnostic) && diagnostic.ownerPresent && diagnostic.markerFresh &&
        !diagnostic.ownerHintGeneration && !diagnostic.ownerHintMatchesMarker && !diagnostic.coveredBytes,
        "Coverage query revived a copy observed before its marker");
    Require(fixture.Observe(destination.Get(),0,source.Get(),0,data.size()) && intent::QueryStructuredCoverage(destination,512,128,diagnostic) &&
        diagnostic.markerGeneration==diagnostic.generation && diagnostic.ownerHintGeneration==diagnostic.generation && diagnostic.ownerHintMatchesMarker &&
        diagnostic.viewOffset==0 && diagnostic.viewBytes==4096 && diagnostic.structuredBytes==4096 && diagnostic.structuredRanges==1 &&
        diagnostic.queryOffset==512 && diagnostic.queryBytes==128 && diagnostic.coveredBytes==128 && diagnostic.firstHoleOffset==UINT64_MAX,
        "Full observed coverage query lost current marker or exact span");
    intent::Invalidate(destination.Get(),544,16);
    Require(intent::QueryStructuredCoverage(destination,512,128,diagnostic) && diagnostic.structuredBytes==4080 && diagnostic.structuredRanges==2 &&
        diagnostic.coveredBytes==112 && diagnostic.firstHoleOffset==544 && diagnostic.ownerHintMatchesMarker,
        "Coverage query concealed a current exact hole");
    Require(intent::QueryStructuredCoverage(destination,544,16,diagnostic) && !diagnostic.coveredBytes && diagnostic.firstHoleOffset==544,
        "Fully unknown selected span inherited prior bytes");
    Require(intent::QueryStructuredCoverage(destination,560,64,diagnostic) && diagnostic.coveredBytes==64 && diagnostic.firstHoleOffset==UINT64_MAX,
        "Coverage query erased an untouched adjacent span");
    intent::Clear();Require(intent::QueryStructuredCoverage(destination,512,128,diagnostic) && diagnostic.ownerPresent && !diagnostic.markerFresh &&
        diagnostic.markerGeneration<diagnostic.generation && !diagnostic.ownerHintGeneration && !diagnostic.ownerHintMatchesMarker &&
        !diagnostic.structuredBytes && !diagnostic.structuredRanges && !diagnostic.coveredBytes && diagnostic.firstHoleOffset==512,
        "Cleared coverage query revived old marker or data");
    Require(!intent::QueryStructuredCoverage(destination,4090,16,diagnostic) && !intent::QueryStructuredCoverage(destination,0,intent::MaxCopyBytes+1,diagnostic),
        "Coverage query admitted an unbounded or invalid span");
}
int main(){try{Fixture fixture;FullWriteAndUnrelatedTail(fixture);AdjacentUpdatesAndReceipts(fixture);HolesUnknownWritesAndPartialOverwrite(fixture);
    GenericAndDirectRecordStaySeparate(fixture);FreshMarkerIdentityAndGate(fixture);BudgetCompactionAndImmutableAccounting(fixture);GlobalBoundAndResourceLifetime(fixture);
    UntypedTrafficPreservesStructuredCoverage(fixture);ClearReleasesMetadataCapacity(fixture);InterleavedPayloadEviction(fixture);CurrentCoverageDiagnostics(fixture);
    intent::Clear();const auto stats=intent::GetStatistics();Require(!stats.currentBytes && !stats.rangeCount && !stats.livePayloadBytes &&
        !stats.structuredBytes && !stats.structuredRanges,"Final structured cache cleanup failed");
    // WARP resources and CPU UPLOAD sampling only; no GPU copy or execution.
    std::puts("structured_intent_cache_test: OK");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"structured_intent_cache_test: %s\n",error.what());return 1;}}
