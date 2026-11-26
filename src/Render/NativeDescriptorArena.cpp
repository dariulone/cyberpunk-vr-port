#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/NativeDescriptorArena.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <atomic>
#include <mutex>
#include <optional>
#if CVR_RUST_VRIK
// The native descriptor arena is Rust (rust/crates/cvr-render/src/native_descriptor_arena.rs: the camera
// heap placement, the engine ring, the owner, Bind and ValidateSubmission); its only user, the retained
// geometry capture, calls it there.
#else

namespace cvr::stereo::native_descriptors {
namespace {
using Microsoft::WRL::ComPtr;
constexpr GUID ownerType{0x13c4cd91,0x2a6b,0x4591,{0xbb,0x1d,0x14,0x72,0x69,0x5c,0x81,0xc8}};
thread_local Failure failure{};
bool Fail(Failure value){failure=value;return false;}
bool Read(uintptr_t address,void* output,size_t bytes){SIZE_T copied{};return address && address<=UINTPTR_MAX-bytes &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;}
bool Compatible(uintptr_t base){
    static const bool valid=[&]{
        // Native table materialization and its cold path own cursor+0x14 and
        // use precisely [base+cursor, base+capacity) before allocating a new block.
        // 2.5: the allocator sub_140419DF4 inlines its 2.31 cold path (sub_141EF5D8A): one whole-function row.
        constexpr std::array<native_recording::detail::Code,12> code{{
            {0x335C3C,0x4DC,0x20371793713C127Full},{0x2457F84,0xD3,0xFDEFCB6798D5CC8Bull},
            {0x419DF4,0x138,0x6B65A1650C2F23E4ull},
            {0x3E9860,185,0x0990890D546C54BDull},{0x3E97E4,85,0x894B596726ACB33Aull},
            {0x3E96EC,82,0x792C0A460B932B2Eull},{0x3E9740,163,0x7DF6C8153488B6BCull},
            {0x246A84E,111,0xA2E05EB8DB0627E2ull},{0x3E7D34,106,0x0A0DB7DBCE20E1CDull},
            {0x3E6CD4,38,0xB56EF3B145CCE822ull},{0x3E7ED4,86,0x15A749BD01861343ull},
            {0x5B3154,72,0x5928B74865426BF6ull}}};
        std::array<uint8_t,0x500> bytes{};
        for(const auto& row:code)if(!base || base>UINTPTR_MAX-row.rva || !Read(base+row.rva,bytes.data(),row.bytes) ||
            native_recording::detail::Fingerprint({bytes.data(),row.bytes})!=row.hash)return false;
        return true;
    }();return valid;
}
bool Advance(uint64_t state,UINT before,UINT after)noexcept {
    __try{return state && !(state&3) && state<=UINT64_MAX-0x18 &&
        UINT(InterlockedCompareExchange(reinterpret_cast<volatile LONG*>(state+0x14),LONG(after),LONG(before)))==before;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
bool SameDevice(ID3D12Device* device,ID3D12DeviceChild* child){ComPtr<ID3D12Device> actual;
    return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&actual))) && actual.Get()==device;}
constexpr uint64_t RingOffset=0x14F1E40,OutstandingOffset=0x1410730;
struct RingLease {uint64_t registry{},ring{},heap{},capacity{},begin{},end{};};
bool ReadRing(uint64_t registry,detail::Ring& output){
    output={};if(!registry || registry>UINT64_MAX-RingOffset-72)return false;
    const auto ring=registry+RingOffset;UINT count{},outstanding{};uint64_t tail{};
    detail::Ring next;next.address=ring;
    if(!Read(ring+64,&count,4) || count!=1 || !Read(ring+56,&next.heap,8) || !Read(ring+16,&next.capacity,8) ||
        !Read(ring+8,&tail,8) || !Read(ring,&next.head,8) || !Read(ring+8,&next.tail,8) || tail!=next.tail ||
        !Read(registry+OutstandingOffset,&outstanding,4))return false;
    next.outstanding=outstanding;output=next;return true;
}
bool RingAlive(const RingLease& lease){detail::Ring now;
    return ReadRing(lease.registry,now) && detail::ValidRing(now,lease.heap,1048576) && now.address==lease.ring &&
        now.capacity==lease.capacity && now.tail<=lease.begin && now.head>=lease.end;
}
bool ReserveRing(uintptr_t base,const native_recording::Lease& recording,ID3D12DescriptorHeap* heap,UINT count,UINT stride,
    Range& range,RingLease& lease,std::array<uint64_t,16>& receipt){
    detail::Ring before,after;const auto identity=reinterpret_cast<uintptr_t>(heap);const auto desc=heap->GetDesc();
    if(!ReadRing(recording.registry,before) || !detail::ValidRing(before,identity,desc.NumDescriptors) ||
        !count || count>8192 || count>before.capacity || before.head>UINT64_MAX-before.capacity)return Fail(Failure::Heap);
    using Reserve=uint64_t(__fastcall*)(void*,uint64_t,uint64_t,void*);
    // nullptr is the audited exhaustion path that returns capacity immediately.
    // The ordinary caller passes a flag pointer and may wait for a fence instead.
    const auto first=reinterpret_cast<Reserve>(base+0x419DF4)(reinterpret_cast<void*>(before.address),count,1,nullptr);
    if(first==before.capacity)return Fail(Failure::Capacity);
    uint64_t absolute{};
    if(first>UINT_MAX || !ReadRing(recording.registry,after) || !detail::RingResult(before,after,UINT(first),count,absolute))return Fail(Failure::Reservation);
    const auto cpu=heap->GetCPUDescriptorHandleForHeapStart();const auto gpu=heap->GetGPUDescriptorHandleForHeapStart();
    const auto end=(first+count)*stride;
    if(end>UINT64_MAX-cpu.ptr || end>UINT64_MAX-gpu.ptr)return Fail(Failure::Heap);
    range={UINT(first),count,0,count,{cpu.ptr+SIZE_T(first)*stride},{gpu.ptr+first*stride}};
    lease={recording.registry,before.address,identity,before.capacity,absolute,absolute+count};
    receipt={0,recording.registry,before.address,identity,before.capacity,before.head,before.tail,after.head,after.tail,
        first,count,absolute,before.outstanding,after.outstanding,1,0};return true;
}
struct Owner final:IUnknown {
    std::atomic<ULONG> refs{1};native_recording::Lease recording;camera_upload::Context context;
    scene_state::Bindings original;ComPtr<ID3D12DescriptorHeap> heap;ComPtr<ID3D12RootSignature> root;
    std::array<ComPtr<ID3D12DescriptorHeap>,2> heaps;ComPtr<ID3D12Device> device;
    std::vector<descriptors::OwnedSourceTable> tables;
    std::vector<ComPtr<IUnknown>> prior;
    std::optional<RingLease> ring;std::vector<RingLease> borrowedRings;
    std::vector<std::pair<UINT,D3D12_GPU_DESCRIPTOR_HANDLE>> bindings;Range range;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown) && id!=ownerType)return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
ComPtr<Owner> Get(IUnknown* value){ComPtr<Owner> owner;if(value)value->QueryInterface(ownerType,reinterpret_cast<void**>(owner.GetAddressOf()));return owner;}
bool SameBinding(const scene_state::Bindings& a,const scene_state::Bindings& b){
    if(a.root!=b.root || a.heapCount!=b.heapCount || a.heaps!=b.heaps || a.tableMask!=b.tableMask)return false;
    for(UINT i=0;i<64;++i)if(a.tables[i].ptr!=b.tables[i].ptr)return false;return true;
}
bool SameTables(const Tables& tables,const Owner& owner){return tables.bindings.size()==owner.bindings.size() &&
    std::equal(tables.bindings.begin(),tables.bindings.end(),owner.bindings.begin(),[](const auto& a,const auto& b){return a.first==b.first && a.second.ptr==b.second.ptr;});}
bool SameRecording(const native_recording::Lease& a,const native_recording::Lease& b){return a.token && a.token==b.token &&
    a.list==b.list && a.generation==b.generation && a.context==b.context && a.registry==b.registry;}
}
struct Cache::Storage {
    struct Entry {ID3D12DescriptorHeap* heap{};descriptors::OwnedSourceTable source;D3D12_GPU_DESCRIPTOR_HANDLE gpu{};
        uint64_t ordinal{};ComPtr<IUnknown> owner;};
    native_recording::Lease recording;std::vector<Entry> entries;
};
Cache::Cache()=default;Cache::~Cache()=default;
bool CopyTables(ID3D12GraphicsCommandList* list,const scene_state::Bindings& bindings,
    std::span<const descriptors::OwnedSourceTable> tables,uint64_t ordinal,Cache& cache,Tables& output)try{
    output={};failure=Failure::Arguments;
    if(probe::internalCommands || !list || tables.empty() || tables.size()>64 || !bindings.root || !bindings.heapCount || bindings.heapCount>2)return false;
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));if(!Compatible(base))return Fail(Failure::Code);
    const auto context=camera_upload::CurrentContext(base);native_recording::Lease recording;camera_upload::flush_state::Snapshot before;
    if(!context || context.list!=reinterpret_cast<uintptr_t>(list) || !native_recording::Capture(list,context,recording) ||
        !native_recording::ValidateRecording(recording,list,context))return Fail(Failure::Recording);
    if(cache.storage_ && !SameRecording(cache.storage_->recording,recording))cache.storage_.reset();
    if(!cache.storage_){cache.storage_=std::make_unique<Cache::Storage>();cache.storage_->recording=recording;}
    scene_state::Bindings live;
    if(!scene_state::CaptureSourceBindings(list,live) || !SameBinding(bindings,live) ||
        !camera_upload::flush_state::Read(Read,context,3,before))return Fail(Failure::State);
    ComPtr<Owner> owner;owner.Attach(new Owner);owner->recording=recording;owner->context=context;owner->original=bindings;owner->root=bindings.root;
    for(UINT i=0;i<bindings.heapCount;++i){owner->heaps[i]=bindings.heaps[i];
        if(reinterpret_cast<uintptr_t>(bindings.heaps[i])==before.heap)owner->heap=bindings.heaps[i];}
    if(!owner->heap || FAILED(list->GetDevice(IID_PPV_ARGS(&owner->device))) || !SameDevice(owner->device.Get(),owner->heap.Get()))return Fail(Failure::Heap);
    const auto desc=owner->heap->GetDesc();const auto stride=owner->device->GetDescriptorHandleIncrementSize(desc.Type);
    const auto cpu=owner->heap->GetCPUDescriptorHandleForHeapStart();const auto gpuStart=owner->heap->GetGPUDescriptorHandleForHeapStart();
    if(desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || desc.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE ||
        !cpu.ptr || !gpuStart.ptr || before.stride!=stride || !before.capacity || before.cursor>before.capacity ||
        UINT64(before.base)+before.capacity>desc.NumDescriptors ||
        (before.cpuValid && before.cachedCpu!=cpu.ptr) || (before.gpuValid && before.cachedGpu!=gpuStart.ptr))return Fail(Failure::Heap);
    UINT total{},fresh{};uint64_t mask{};std::vector<const Cache::Storage::Entry*> reused;
    for(const auto& table:tables){
        if(!table.cpuHeap || !table.owner || !table.provenance || table.parameter>=64 || !(bindings.tableMask&(1ull<<table.parameter)) ||
            table.sourceGpu!=bindings.tables[table.parameter].ptr || !SameDevice(owner->device.Get(),table.cpuHeap.Get()))return Fail(Failure::Table);
        const auto source=table.cpuHeap->GetDesc();
        if(source.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_NONE || !table.count || table.count>source.NumDescriptors)return Fail(Failure::Table);
        if(source.Type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER)continue;
        if(source.Type!=desc.Type || table.count>8192-total || mask&(1ull<<table.parameter))return Fail(Failure::Table);
        mask|=1ull<<table.parameter;total+=table.count;owner->tables.push_back(table);
        const Cache::Storage::Entry* existing{};
        for(const auto& entry:cache.storage_->entries)if(entry.heap==owner->heap.Get() && entry.ordinal<ordinal && entry.source.cpuHeap==table.cpuHeap &&
            entry.source.owner==table.owner && entry.source.count==table.count && entry.source.identity==table.identity && entry.source.version==table.version){existing=&entry;break;}
        reused.push_back(existing);if(existing){owner->prior.push_back(existing->owner);
            const auto prior=Get(existing->owner.Get());if(!prior)return Fail(Failure::Changed);
            if(prior->ring)owner->borrowedRings.push_back(*prior->ring);
        }else fresh+=table.count;
    }
    if(!total)return Fail(Failure::Table);
    if((before.dirty&before.graphicsMask) || before.rebind)return Fail(Failure::State);
    bool extraRing{};
    if(fresh){extraRing=!detail::Plan(before,fresh,desc.NumDescriptors,stride,cpu.ptr,gpuStart.ptr,owner->range);}
    else{if(UINT64(before.base)+before.capacity>desc.NumDescriptors || before.cursor>before.capacity)return Fail(Failure::Heap);
        owner->range={before.base+before.cursor,0,before.cursor,before.cursor,{},{}};}
    owner->bindings.reserve(owner->tables.size());UINT offset{};
    Tables next;next.entries.reserve(owner->tables.size());
    for(size_t index=0;index<owner->tables.size();++index){const auto& table=owner->tables[index];const auto* old=reused[index];
        const auto handle=old?old->gpu:D3D12_GPU_DESCRIPTOR_HANDLE{owner->range.gpu.ptr+UINT64(offset)*stride};
        owner->bindings.push_back({table.parameter,handle});
        next.entries.push_back({ordinal,table.parameter,table.count,reinterpret_cast<uintptr_t>(table.cpuHeap.Get()),handle.ptr,
            old?old->ordinal:ordinal,old?old->source.parameter:table.parameter,old?1ull:0ull});
        if(!old)offset+=table.count;
    }
    next.owner=owner;next.recording=recording;next.range=owner->range;next.bindings=owner->bindings;next.descriptors=total;
    next.heap=before.heap;next.base=before.base;next.capacity=before.capacity;next.borrowedMask=bindings.tableMask&~mask;
    cache.storage_->entries.reserve(cache.storage_->entries.size()+owner->tables.size());
    camera_upload::flush_state::Snapshot check;
    if(!camera_upload::flush_state::Read(Read,context,3,check) || check!=before || camera_upload::CurrentContext(base)!=context ||
        !native_recording::ValidateRecording(recording,list,context))return Fail(Failure::Changed);
    if(!gpu::KeepCommandResources(list,{owner.Get()}))return Fail(Failure::Retention);
    if(extraRing){RingLease ring;
        if(!ReserveRing(base,recording,owner->heap.Get(),fresh,stride,owner->range,ring,next.ring))return false;
        owner->ring=ring;next.ring[0]=ordinal;
    }else if(fresh && !Advance(context.state,before.cursor,owner->range.after))return Fail(Failure::Reservation);
    // Never roll this reservation back, even if a later validation fails. Native
    // allocation retirement releases its block with the original recording.
    if(!camera_upload::flush_state::Read(Read,context,3,check) || (fresh && !extraRing?!detail::Advanced(before,check,owner->range):check!=before))return Fail(Failure::Changed);
    if(extraRing){next.ring[15]=1;next.range=owner->range;next.base=owner->range.first;next.capacity=owner->range.count;offset=0;
        for(size_t index=0;index<owner->tables.size();++index)if(!next.entries[index][7]){
            owner->bindings[index].second.ptr=owner->range.gpu.ptr+UINT64(offset)*stride;next.entries[index][4]=owner->bindings[index].second.ptr;
            next.bindings[index]=owner->bindings[index];offset+=owner->tables[index].count;}}
    {probe::Scope internal;offset=0;
        for(size_t index=0;index<owner->tables.size();++index){if(next.entries[index][7])continue;const auto& table=owner->tables[index];
            owner->device->CopyDescriptorsSimple(table.count,{owner->range.cpu.ptr+SIZE_T(offset)*stride},
                table.cpuHeap->GetCPUDescriptorHandleForHeapStart(),desc.Type);offset+=table.count;}}
    // All cached entries pin immutable CPU storage and the ORIGINAL range owner.
    // A different recording or shader-visible heap can never borrow its slots.
    for(size_t index=0;index<owner->tables.size();++index)if(!next.entries[index][7])cache.storage_->entries.push_back(
        {owner->heap.Get(),owner->tables[index],owner->bindings[index].second,ordinal,owner});
    output=std::move(next);failure=Failure::None;return true;
}catch(...){return Fail(Failure::Exception);}
bool Bind(ID3D12GraphicsCommandList* list,const Tables& tables){
    const auto owner=Get(tables.owner.Get());if(!owner || !list || !SameTables(tables,*owner.Get()) || !SameRecording(tables.recording,owner->recording) ||
        reinterpret_cast<uintptr_t>(list)!=owner->recording.list || !gpu::HasCommandResource(list,owner.Get()))return false;
    const auto context=camera_upload::CurrentContext(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)));scene_state::Bindings current;
    if(context!=owner->context || !native_recording::ValidateRecording(owner->recording,list,context) ||
        !scene_state::CaptureSourceBindings(list,current) || !SameBinding(owner->original,current))return false;
    probe::Scope internal;for(const auto& [parameter,handle]:owner->bindings)list->SetGraphicsRootDescriptorTable(parameter,handle);return true;
}
bool ValidateSubmission(const Tables& tables,ID3D12CommandQueue* queue){const auto owner=Get(tables.owner.Get());
    return owner && SameTables(tables,*owner.Get()) && SameRecording(tables.recording,owner->recording) &&
        native_recording::ValidateSubmission(owner->recording,queue) && (!owner->ring || RingAlive(*owner->ring)) &&
        std::all_of(owner->borrowedRings.begin(),owner->borrowedRings.end(),RingAlive);}
Failure LastFailure(){return failure;}
}
#endif
#endif
