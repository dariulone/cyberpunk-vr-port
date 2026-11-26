#include "Render/StereoBindingBank.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include <algorithm>
#include <atomic>
#include <cstring>
#include <map>
#include <new>
#include <set>
#include <tuple>
#include <intrin.h>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// The bank: rust/crates/cvr-render/src/binding_bank.rs (tools/single_pass_tests/binding_bank_test and
// source_binding_assembly_test run their cases against it in their Rust variants).
extern "C" {
HRESULT cvr_bank_create_pass(ID3D12Device*,const void*,size_t,void**);
HRESULT cvr_bank_create_source_pass(ID3D12Device*,const void*,size_t,cvr::stereo::SourcePassFailure*,void**);
HRESULT cvr_bank_create_arena(ID3D12Device*,ID3D12RootSignature*,const void*,UINT,UINT64,void**);
HRESULT cvr_bank_append_constants(void*,const void*,const void*,size_t,UINT*);
bool cvr_bank_restart_arena(void*);
bool cvr_bank_captured_for(void*,ID3D12GraphicsCommandList*,bool(*)(void*,void*));
bool cvr_bank_capture_eye(void*,ID3D12GraphicsCommandList*,UINT,bool(*)(void*,void*),bool(*)(void*,void*));
bool cvr_bank_bind(void*,ID3D12GraphicsCommandList*,UINT,UINT,bool(*)(void*,void*));
bool cvr_bank_bind_source(void*,ID3D12GraphicsCommandList*,UINT,bool(*)(void*,void*));
void cvr_bank_counts(void*,void*);
}
#endif
#endif
namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's banks are binding_bank.rs' (no StereoBindingBank is made)
// The storage is the Rust object (its IUnknown vtable is Rust's); the inputs are lent as mirrors, the command resources'
// Keep / Has are callbacks.
namespace {
struct RustPlan {const uint8_t* serialized;size_t serializedLength;const PairedBindingLocation* bindings;size_t bindingCount;
    std::array<UINT,2> parameters,descriptors;UINT originalParameters,rootDwords;};
struct RustValue {const uint8_t* constants;size_t constantsLength;ID3D12Resource* source;D3D12_SHADER_RESOURCE_VIEW_DESC srv;
    D3D12_RESOURCE_STATES state;D3D12_CONSTANT_BUFFER_VIEW_DESC gpuCbv;IUnknown* owner;bool borrowedSrv,implicitSrv;};
struct RustPair {PairedBinding binding;std::array<RustValue,2> eyes;};
struct RustTable {UINT parameter,offset,count;ID3D12DescriptorHeap* cpuHeap;IUnknown* const* leases;size_t leaseCount;
    uint64_t identity,version,storageScope;UINT storageExtent;};
struct RustDraw {ID3D12RootSignature* root;const RustPlan* plan;const RustPair* pairs;size_t pairCount;const RustTable* shared;
    size_t sharedCount;ID3D12RootSignature* sourceRoot;};
struct RustSource {ID3D12RootSignature* root;const uint8_t* serialized;size_t serializedLength;const RustTable* tables;size_t tableCount;};
struct RustCounts {UINT draws,descriptors,snapshots;UINT64 constantBytes;};
static_assert(sizeof(RustPlan)==56 && sizeof(RustValue)==104 && sizeof(RustPair)==224 && sizeof(RustTable)==72 &&
    sizeof(RustDraw)==56 && sizeof(RustSource)==40 && sizeof(RustCounts)==24,"mirrored by binding_bank.rs");
RustPlan Lend(const PairedRootPlan& plan){
    return {plan.serialized.data(),plan.serialized.size(),plan.bindings.data(),plan.bindings.size(),plan.parameters,plan.descriptors,
        plan.originalParameters,plan.rootDwords};
}
RustValue Lend(const BankValue& v){
    return {v.constants.data(),v.constants.size(),v.source.Get(),v.srv,v.state,v.gpuCbv,v.owner.Get(),v.borrowedSrv,v.implicitSrv};
}
RustPair Lend(const BankPair& p){return {p.binding,{Lend(p.eyes[0]),Lend(p.eyes[1])}};}
// The tables with their leases as raw pointers: the owners stay held by the caller's tables for the call.
struct LentTables {std::vector<std::vector<IUnknown*>> leases;std::vector<RustTable> tables;};
void Lend(std::span<const SharedBankTable> tables,LentTables& output){
    output.leases.reserve(tables.size());output.tables.reserve(tables.size());
    for(const auto& t:tables){auto& leases=output.leases.emplace_back();leases.reserve(t.leases.size());
        for(const auto& lease:t.leases)leases.push_back(lease.Get());
        output.tables.push_back({t.parameter,t.offset,t.count,t.cpuHeap.Get(),leases.data(),leases.size(),t.identity,t.version,
            t.storageScope,t.storageExtent});}
}
bool Keep(void* list,void* storage)noexcept{
    try{return gpu::KeepCommandResources(static_cast<ID3D12GraphicsCommandList*>(list),{static_cast<IUnknown*>(storage)});}catch(...){return false;}
}
bool Has(void* list,void* storage)noexcept{
    try{return gpu::HasCommandResource(static_cast<ID3D12GraphicsCommandList*>(list),static_cast<IUnknown*>(storage));}catch(...){return false;}
}
RustCounts Counts(IUnknown* storage){RustCounts counts{};if(storage)cvr_bank_counts(storage,&counts);return counts;}
}
struct StereoBindingBank::Storage:IUnknown {};
StereoBindingBank::StereoBindingBank()=default;
StereoBindingBank::~StereoBindingBank()=default;
HRESULT StereoBindingBank::Create(ID3D12Device* device,ID3D12RootSignature* root,const PairedRootPlan& plan,
    std::span<const BankPair> pairs,std::span<const SharedBankTable> shared) {
    const BankDraw draw{root,&plan,pairs,shared};return CreatePass(device,{&draw,1});
}
HRESULT StereoBindingBank::CreatePass(ID3D12Device* device,std::span<const BankDraw> draws)try{
    if(storage_ || !device || draws.empty() || draws.size()>32768)return E_INVALIDARG;
    std::vector<RustPlan> plans;std::vector<std::vector<RustPair>> pairs;std::vector<LentTables> tables;std::vector<RustDraw> lent;
    plans.reserve(draws.size());pairs.reserve(draws.size());tables.reserve(draws.size());lent.reserve(draws.size());
    for(const auto& draw:draws){
        plans.push_back(draw.plan?Lend(*draw.plan):RustPlan{});
        auto& lentPairs=pairs.emplace_back();lentPairs.reserve(draw.pairs.size());for(const auto& pair:draw.pairs)lentPairs.push_back(Lend(pair));
        auto& lentTables=tables.emplace_back();Lend(draw.shared,lentTables);
        lent.push_back({draw.root,draw.plan?&plans.back():nullptr,lentPairs.data(),lentPairs.size(),lentTables.tables.data(),
            lentTables.tables.size(),draw.sourceRoot});
    }
    void* created{};const auto hr=cvr_bank_create_pass(device,lent.data(),lent.size(),&created);if(FAILED(hr))return hr;
    storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
HRESULT StereoBindingBank::CreateSourcePass(ID3D12Device* device,std::span<const SourceBankDraw> inputs)try{
    sourcePassFailure_={};
    if(storage_){sourcePassFailure_.stage=UINT(SourcePassFailureStage::Arguments);
        sourcePassFailure_.bits=uint64_t(SourcePassFailureReason::AlreadyInitialized);return E_INVALIDARG;}
    std::vector<LentTables> tables;std::vector<RustSource> lent;tables.reserve(inputs.size());lent.reserve(inputs.size());
    for(const auto& input:inputs){auto& lentTables=tables.emplace_back();Lend(input.tables,lentTables);
        lent.push_back({input.root.Get(),input.serializedRoot.data(),input.serializedRoot.size(),lentTables.tables.data(),lentTables.tables.size()});}
    void* created{};const auto hr=cvr_bank_create_source_pass(device,lent.data(),lent.size(),&sourcePassFailure_,&created);if(FAILED(hr))return hr;
    storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));return S_OK;
}catch(const std::bad_alloc&){sourcePassFailure_.stage=UINT(SourcePassFailureStage::Exception);
    sourcePassFailure_.bits=uint64_t(SourcePassFailureReason::AllocationException);return E_OUTOFMEMORY;}
HRESULT StereoBindingBank::CreateArena(ID3D12Device* device,ID3D12RootSignature* root,const PairedRootPlan& plan,UINT maxDraws,UINT64 bytes)try{
    if(storage_ || !device)return E_INVALIDARG;
    const auto lent=Lend(plan);void* created{};const auto hr=cvr_bank_create_arena(device,root,&lent,maxDraws,bytes,&created);if(FAILED(hr))return hr;
    storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(created)));return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
HRESULT StereoBindingBank::AppendConstants(const PairedRootPlan& plan,std::span<const BankPair> pairs,UINT& index)try{
    if(!storage_)return E_INVALIDARG;
    const auto lentPlan=Lend(plan);std::vector<RustPair> lent;lent.reserve(pairs.size());for(const auto& pair:pairs)lent.push_back(Lend(pair));
    return cvr_bank_append_constants(storage_.Get(),&lentPlan,lent.data(),lent.size(),&index);
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool StereoBindingBank::RestartArena(){return storage_ && cvr_bank_restart_arena(storage_.Get());}
void* StereoBindingBank::DetachStorage(){return static_cast<IUnknown*>(storage_.Detach());}
void StereoBindingBank::AttachStorage(void* storage){storage_.Attach(static_cast<Storage*>(static_cast<IUnknown*>(storage)));}
bool StereoBindingBank::CapturedFor(ID3D12GraphicsCommandList* list)const{return storage_ && list && cvr_bank_captured_for(storage_.Get(),list,&Has);}
bool StereoBindingBank::CaptureEye(ID3D12GraphicsCommandList* list,UINT eye){
    return storage_ && list && cvr_bank_capture_eye(storage_.Get(),list,eye,&Keep,&Has);
}
bool StereoBindingBank::Bind(ID3D12GraphicsCommandList* list,UINT index)const{
    return storage_ && list && cvr_bank_bind(storage_.Get(),list,UINT_MAX,index,&Has);
}
bool StereoBindingBank::BindSourceEye(ID3D12GraphicsCommandList* list,UINT eye,UINT index)const{
    return storage_ && list && eye<2 && cvr_bank_bind(storage_.Get(),list,eye,index,&Has);
}
bool StereoBindingBank::BindSource(ID3D12GraphicsCommandList* list,UINT index)const{
    return storage_ && list && cvr_bank_bind_source(storage_.Get(),list,index,&Keep);
}
UINT StereoBindingBank::DescriptorCount()const{return Counts(storage_.Get()).descriptors;}
UINT StereoBindingBank::DrawCount()const{return Counts(storage_.Get()).draws;}
UINT StereoBindingBank::SnapshotCount()const{return Counts(storage_.Get()).snapshots;}
UINT64 StereoBindingBank::ConstantBytes()const{return Counts(storage_.Get()).constantBytes;}
#endif
#else
struct StereoBindingBank::Storage final : IUnknown {
    struct Copy {ComPtr<ID3D12Resource> source,owned;D3D12_RESOURCE_STATES state{};UINT eye{};};
    struct Draw {ComPtr<ID3D12RootSignature> root,sourceRoot;std::vector<std::pair<UINT,UINT>> tables,samplerTables;
        std::array<std::vector<std::pair<UINT,UINT>>,2> sourceTables;};
    std::atomic<ULONG> refs{1};
    ComPtr<ID3D12Device> device;
    std::vector<Draw> draws;
    std::vector<std::vector<uint8_t>> sourceLayouts;
    bool sourcePass{};
    ComPtr<ID3D12DescriptorHeap> heap,samplers;
    ComPtr<ID3D12Resource> constants;
    std::vector<Copy> copies;
    std::vector<ComPtr<IUnknown>> leases;
    ID3D12GraphicsCommandList* recording{};
    UINT descriptors{},stride{},samplerStride{},capturedMask{};UINT64 constantBytes{};
    ComPtr<ID3D12RootSignature> arenaRoot;PairedRootPlan arenaPlan;
    void* mapped{};UINT maxDraws{},perDrawDescriptors{};UINT64 capacity{};
    ~Storage(){if(mapped){const D3D12_RANGE written{0,SIZE_T(capacity)};constants->Unmap(0,&written);}}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release() override{const auto n=--refs;if(!n)delete this;return n;}
};
StereoBindingBank::StereoBindingBank()=default;
StereoBindingBank::~StereoBindingBank()=default;
namespace {
constexpr UINT DescriptorLimit=1000000;
constexpr UINT64 ConstantLimit=16*1024*1024,SnapshotLimit=256*1024*1024;
constexpr auto ShaderRead=D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
bool SameDevice(ID3D12Device* expected,ID3D12DeviceChild* object) {
    if(!object)return false;ComPtr<ID3D12Device> device;
    return SUCCEEDED(object->GetDevice(IID_PPV_ARGS(&device))) && device.Get()==expected;
}
HRESULT Buffer(ID3D12Device* device,UINT64 bytes,ComPtr<ID3D12Resource>& output) {
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=bytes;d.Height=d.DepthOrArraySize=d.MipLevels=1;
    d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
    return device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&output));
}
struct PreparedValue {const BankValue* value{};UINT slot{};UINT64 constantOffset{};ID3D12Resource* snapshot{};bool srv{};};
bool ComponentMapping(UINT mapping) {
    if(!(mapping&0x1000) || (mapping&~0x1fffu))return false;
    for(UINT i=0;i<4;++i)if(((mapping>>(i*3))&7)>5)return false;return true;
}
bool TableFits(const D3D12_ROOT_SIGNATURE_DESC1& root,UINT parameter,UINT count,bool sampler) {
    if(parameter>=root.NumParameters)return false;const auto& p=root.pParameters[parameter];
    if(p.ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE || !p.DescriptorTable.NumDescriptorRanges)return false;
    uint64_t next{};
    for(UINT i=0;i<p.DescriptorTable.NumDescriptorRanges;++i) {
        const auto& r=p.DescriptorTable.pDescriptorRanges[i];
        if((r.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER)!=sampler)return false;
        const uint64_t offset=r.OffsetInDescriptorsFromTableStart==UINT_MAX?next:r.OffsetInDescriptorsFromTableStart;
        if(offset>=count || (r.NumDescriptors!=UINT_MAX && r.NumDescriptors>count-offset))return false;
        next=r.NumDescriptors==UINT_MAX?uint64_t(UINT_MAX)+1:offset+r.NumDescriptors;
    }
    return true;
}
bool CbvRegisters(const D3D12_ROOT_PARAMETER1& parameter,std::vector<UINT>& registers) {
    if(parameter.ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE || !parameter.DescriptorTable.NumDescriptorRanges)return false;
    UINT next{};
    for(UINT i=0;i<parameter.DescriptorTable.NumDescriptorRanges;++i) {
        const auto& range=parameter.DescriptorTable.pDescriptorRanges[i];
        const UINT offset=range.OffsetInDescriptorsFromTableStart==UINT_MAX?next:range.OffsetInDescriptorsFromTableStart;
        if(range.RangeType!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV || range.RegisterSpace || !range.NumDescriptors ||
            offset>65536 || range.NumDescriptors>65536-offset || range.BaseShaderRegister>UINT_MAX-range.NumDescriptors)return false;
        next=offset+range.NumDescriptors;registers.resize(std::max(registers.size(),size_t(next)),UINT_MAX);
        for(UINT n=0;n<range.NumDescriptors;++n){if(registers[offset+n]!=UINT_MAX)return false;registers[offset+n]=range.BaseShaderRegister+n;}
    }
    return !registers.empty();
}
struct SourceTableLayout {
    UINT count{};D3D12_DESCRIPTOR_HEAP_TYPE type{};
    bool unbounded{};UINT tailOffset{},tailRegister{};
};
struct SourceLayout {UINT parameters{};std::array<SourceTableLayout,64> tables{};};
constexpr uint64_t Bit(SourcePassFailureReason reason){return uint64_t(reason);}
HRESULT ReadSourceLayout(std::span<const uint8_t> bytes,SourceLayout& output,SourcePassFailure& diagnostic) {
    using Reason=SourcePassFailureReason;
    const auto reject=[&](Reason reason,HRESULT result=E_INVALIDARG){diagnostic.bits=Bit(reason);return result;};
    diagnostic.stage=UINT(SourcePassFailureStage::RootLayout);diagnostic.parameter=UINT_MAX;
    if(bytes.empty() || bytes.size()>1024*1024)return reject(Reason::BlobExtent);
    ComPtr<ID3D12VersionedRootSignatureDeserializer> decoder;
    auto hr=D3D12CreateVersionedRootSignatureDeserializer(bytes.data(),bytes.size(),IID_PPV_ARGS(&decoder));
    if(FAILED(hr))return reject(Reason::DecoderCreate,hr);
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* decoded{};
    hr=decoder->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&decoded);if(FAILED(hr))return reject(Reason::DecoderVersion,hr);
    const auto& root=decoded->Desc_1_1;
    if(root.NumParameters>64)return reject(Reason::RootParameters);
    if(root.Flags&(D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE|
        D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED|D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED))return reject(Reason::RootFlags);
    SourceLayout pending;pending.parameters=root.NumParameters;
    for(UINT parameter=0;parameter<root.NumParameters;++parameter){
        diagnostic.parameter=parameter;
        const auto& table=root.pParameters[parameter];
        if(table.ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)return reject(Reason::RootParameterKind);
        if(!table.DescriptorTable.NumDescriptorRanges)return reject(Reason::RootRanges);
        const auto* ranges=table.DescriptorTable.pDescriptorRanges;
        const bool sampler=ranges[0].RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;
        const UINT limit=sampler?2048:DescriptorLimit;uint64_t next{},extent{};
        SourceTableLayout entry;entry.type=sampler?D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER:D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        for(UINT range=0;range<table.DescriptorTable.NumDescriptorRanges;++range){
            const auto& r=ranges[range];
            if(UINT(r.RangeType)>UINT(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) ||
                (r.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER)!=sampler)return reject(Reason::RangeType);
            if(!r.NumDescriptors)return reject(Reason::RangeCount);
            const uint64_t offset=r.OffsetInDescriptorsFromTableStart==D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND?next:r.OffsetInDescriptorsFromTableStart;
            if(r.NumDescriptors==UINT_MAX){
                // No meaningful APPEND position follows an unbounded range.
                // A final tail also cannot alias its finite prefix.
                if(range+1!=table.DescriptorTable.NumDescriptorRanges)return reject(Reason::RangeOrder);
                if(extent>offset)return reject(Reason::RangeAlias);
                entry.unbounded=true;entry.tailOffset=UINT(offset);entry.tailRegister=r.BaseShaderRegister;
                extent=offset+1;break;
            }
            if(offset>=limit || r.NumDescriptors>limit-offset)return reject(Reason::RangeExtent);
            if(uint64_t(r.BaseShaderRegister)+r.NumDescriptors>uint64_t(UINT_MAX)+1)return reject(Reason::RangeRegisters);
            next=offset+r.NumDescriptors;extent=std::max(extent,next);
        }
        entry.count=UINT(extent);pending.tables[parameter]=entry;
    }
    output=pending;return S_OK;
}
}
HRESULT StereoBindingBank::Create(ID3D12Device* device,ID3D12RootSignature* root,const PairedRootPlan& plan,
    std::span<const BankPair> pairs,std::span<const SharedBankTable> shared) {
    const BankDraw draw{root,&plan,pairs,shared};return CreatePass(device,{&draw,1});
}
HRESULT StereoBindingBank::CreatePass(ID3D12Device* device,std::span<const BankDraw> draws) try {
    if(storage_ || !device || draws.empty() || draws.size()>32768)return E_INVALIDARG;
    probe::Scope internal;
    ComPtr<Storage> pending;pending.Attach(new Storage);pending->device=device;
    pending->stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    if(!pending->stride)return E_INVALIDARG;
    UINT cursor{},samplerCursor{};UINT64 snapshotBytes{};
    std::map<std::tuple<ID3D12DescriptorHeap*,UINT,UINT>,UINT> sharedOffsets;
    std::map<std::pair<UINT,ID3D12Resource*>,size_t> snapshots;
    std::map<std::vector<uint8_t>,UINT64> constantOffsets;
    std::set<IUnknown*> leased;
    std::vector<PreparedValue> values;
    std::vector<UINT> nullCbvs;
    for(const auto& input:draws) {
    if(!input.plan || !SameDevice(device,input.root))return E_INVALIDARG;
    const auto& plan=*input.plan;const auto pairs=input.pairs;const auto shared=input.shared;
    if(pairs.size()!=plan.bindings.size() || pairs.size()>512 || shared.size()>64)return E_INVALIDARG;
    Storage::Draw draw;draw.root=input.root;draw.sourceRoot=input.sourceRoot;
    if(input.sourceRoot && (!SameDevice(device,input.sourceRoot) || !shared.empty()))return E_INVALIDARG;
    ComPtr<ID3D12VersionedRootSignatureDeserializer> decoder;
    if(plan.serialized.empty() || plan.serialized.size()>1024*1024 ||
       FAILED(D3D12CreateVersionedRootSignatureDeserializer(plan.serialized.data(),plan.serialized.size(),IID_PPV_ARGS(&decoder))))return E_INVALIDARG;
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* decoded{};
    if(FAILED(decoder->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&decoded)) || decoded->Desc_1_1.NumParameters>64 ||
       plan.originalParameters>decoded->Desc_1_1.NumParameters)return E_INVALIDARG;
    const auto& rootLayout=decoded->Desc_1_1;
    // Only descriptor-table shared state is copied. Root CBVs/constants which
    // the transformed shaders still read require explicit materialization.
    std::set<UINT> parameters;
    for(const auto& table:shared) {
        if(!table.cpuHeap || !table.count || table.parameter>=plan.originalParameters || !parameters.insert(table.parameter).second ||
           !SameDevice(device,table.cpuHeap.Get()))return E_INVALIDARG;
        const auto d=table.cpuHeap->GetDesc();
        if((d.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && d.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER) || d.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_NONE ||
           table.offset>d.NumDescriptors || table.count>d.NumDescriptors-table.offset)return E_INVALIDARG;
        const bool sampler=d.Type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
        if(!TableFits(rootLayout,table.parameter,table.count,sampler))return E_INVALIDARG;
        if(!sampler && table.leases.empty())return E_INVALIDARG;
        auto& next=sampler?samplerCursor:cursor;const UINT limit=sampler?2048:DescriptorLimit;
        const auto key=std::make_tuple(table.cpuHeap.Get(),table.offset,table.count);
        const auto found=sharedOffsets.find(key);UINT offset{};
        if(found!=sharedOffsets.end())offset=found->second;
        else {if(table.count>limit-next)return E_INVALIDARG;offset=next;next+=table.count;sharedOffsets[key]=offset;}
        for(const auto& lease:table.leases){if(!lease)return E_INVALIDARG;if(leased.insert(lease.Get()).second)pending->leases.push_back(lease);}
        (sampler?draw.samplerTables:draw.tables).emplace_back(table.parameter,offset);
    }
    std::array<UINT,2> base{};
    for(UINT stage=0;stage<2;++stage) {
        base[stage]=cursor;
        if(plan.descriptors[stage]) {
            if(!TableFits(rootLayout,plan.parameters[stage],plan.descriptors[stage],false) ||
               !parameters.insert(plan.parameters[stage]).second || plan.descriptors[stage]>DescriptorLimit-cursor)return E_INVALIDARG;
            draw.tables.emplace_back(plan.parameters[stage],cursor);cursor+=plan.descriptors[stage];
        }
    }
    std::vector<bool> used(pairs.size());
    std::set<UINT> privateSlots;
    struct SourceTable {std::vector<UINT> registers;std::array<UINT,2> base{};std::set<UINT> used;};
    std::map<UINT,SourceTable> sourceTables;
    for(const auto& binding:plan.bindings) {
        size_t index{};for(;index<pairs.size();++index)if(pairs[index].binding==binding.binding)break;
        if(index==pairs.size() || used[index])return E_INVALIDARG;used[index]=true;
        const auto stage=binding.binding.visibility==D3D12_SHADER_VISIBILITY_VERTEX?0u:1u;
        if(binding.destinationParameter!=plan.parameters[stage] || binding.destinationOffset>plan.descriptors[stage] ||
           plan.descriptors[stage]-binding.destinationOffset<2)return E_INVALIDARG;
        if(input.sourceRoot) {
            if(binding.binding.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV || binding.sourceType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE ||
                binding.sourceParameter>=plan.originalParameters)return E_INVALIDARG;
            const auto& p=rootLayout.pParameters[binding.sourceParameter];
            if(p.ShaderVisibility!=binding.binding.visibility && p.ShaderVisibility!=D3D12_SHADER_VISIBILITY_ALL)return E_INVALIDARG;
            auto [it,inserted]=sourceTables.try_emplace(binding.sourceParameter);auto& table=it->second;
            if(inserted) {
                if(!CbvRegisters(p,table.registers))return E_INVALIDARG;
                for(UINT eye=0;eye<2;++eye){if(table.registers.size()>DescriptorLimit-cursor)return E_INVALIDARG;
                    table.base[eye]=cursor;draw.sourceTables[eye].emplace_back(binding.sourceParameter,cursor);
                    for(UINT n=0;n<table.registers.size();++n)nullCbvs.push_back(cursor+n);cursor+=UINT(table.registers.size());}
            }
            if(binding.sourceOffset>=table.registers.size() || table.registers[binding.sourceOffset]!=binding.binding.sourceRegister ||
                !table.used.insert(binding.sourceOffset).second)return E_INVALIDARG;
        }
        for(UINT eye=0;eye<2;++eye) {
            const auto& value=pairs[index].eyes[eye];PreparedValue entry{&value,base[stage]+binding.destinationOffset+eye};
            if(!privateSlots.insert(entry.slot).second)return E_INVALIDARG;
            if(binding.binding.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV) {
                if(value.borrowedSrv || value.implicitSrv)return E_INVALIDARG;
                if(value.source){
                    if(!value.constants.empty() || !value.owner || !SameDevice(device,value.source.Get()))return E_INVALIDARG;
                    const auto desc=value.source->GetDesc();const auto gpu=value.source->GetGPUVirtualAddress();const auto& cbv=value.gpuCbv;
                    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width || !cbv.BufferLocation || (cbv.BufferLocation&255) ||
                        !cbv.SizeInBytes || cbv.SizeInBytes>65536 || (cbv.SizeInBytes&255) || cbv.BufferLocation<gpu ||
                        cbv.BufferLocation-gpu>desc.Width || cbv.SizeInBytes>desc.Width-(cbv.BufferLocation-gpu))return E_INVALIDARG;
                    pending->leases.emplace_back(value.source);pending->leases.push_back(value.owner);
                }else{
                    if(value.gpuCbv.BufferLocation || value.gpuCbv.SizeInBytes || value.owner || value.constants.empty() || value.constants.size()>65536)return E_INVALIDARG;
                    const UINT64 size=(value.constants.size()+255)&~UINT64(255);
                    std::vector<uint8_t> key(value.constants.begin(),value.constants.end());
                    const auto found=constantOffsets.find(key);
                    if(found!=constantOffsets.end())entry.constantOffset=found->second;
                    else {if(size>ConstantLimit-pending->constantBytes)return E_INVALIDARG;
                        entry.constantOffset=pending->constantBytes;pending->constantBytes+=size;constantOffsets.emplace(std::move(key),entry.constantOffset);}
                }
            } else if(binding.binding.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV) {
                entry.srv=true;
                if(!value.constants.empty() || value.gpuCbv.BufferLocation || value.gpuCbv.SizeInBytes || (value.source && !SameDevice(device,value.source.Get())))return E_INVALIDARG;
                if(value.implicitSrv){if(!value.borrowedSrv || !value.source)return E_INVALIDARG;}
                else if(value.srv.ViewDimension<D3D12_SRV_DIMENSION_BUFFER || value.srv.ViewDimension>D3D12_SRV_DIMENSION_TEXTURECUBEARRAY ||
                    !ComponentMapping(value.srv.Shader4ComponentMapping))return E_INVALIDARG;
                if(value.borrowedSrv){
                    if(!value.source || !value.owner || value.source->GetDesc().Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)return E_INVALIDARG;
                    entry.snapshot=value.source.Get();pending->leases.emplace_back(value.source);pending->leases.push_back(value.owner);
                    values.push_back(entry);continue;
                }
                if(value.owner)return E_INVALIDARG;
                if(!value.source){values.push_back(entry);continue;} // Explicit typed null descriptor.
                const auto key=std::make_pair(eye,value.source.Get());auto found=snapshots.find(key);
                if(found!=snapshots.end()) {
                    const auto& existing=pending->copies[found->second];
                    if(existing.state!=value.state)return E_INVALIDARG;entry.snapshot=existing.owned.Get();
                } else {
                    const auto d=value.source->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
                    if(d.Dimension==D3D12_RESOURCE_DIMENSION_UNKNOWN || (d.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) ||
                       FAILED(value.source->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return E_INVALIDARG;
                    const auto allocation=device->GetResourceAllocationInfo(0,1,&d);
                    if(allocation.SizeInBytes==UINT64_MAX || allocation.SizeInBytes>SnapshotLimit-snapshotBytes)return E_OUTOFMEMORY;
                    snapshotBytes+=allocation.SizeInBytes;
                    Storage::Copy copy;copy.source=value.source;copy.state=value.state;copy.eye=eye;heap={};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
                    auto hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&copy.owned));
                    if(FAILED(hr))return hr;entry.snapshot=copy.owned.Get();snapshots[key]=pending->copies.size();pending->copies.push_back(std::move(copy));
                }
            } else return E_INVALIDARG;
            values.push_back(entry);
            if(input.sourceRoot){entry.slot=sourceTables.at(binding.sourceParameter).base[eye]+binding.sourceOffset;values.push_back(entry);}
        }
    }
    if(privateSlots.size()!=size_t(plan.descriptors[0])+plan.descriptors[1])return E_INVALIDARG;
    pending->draws.push_back(std::move(draw));
    }
    if(!cursor)return E_INVALIDARG;pending->descriptors=cursor;
    D3D12_DESCRIPTOR_HEAP_DESC desc{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,cursor,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
    auto hr=device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&pending->heap));if(FAILED(hr))return hr;
    const auto start=pending->heap->GetCPUDescriptorHandleForHeapStart();
    auto cpu=[&](UINT at){return D3D12_CPU_DESCRIPTOR_HANDLE{start.ptr+SIZE_T(at)*pending->stride};};
    if(samplerCursor) {
        desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;desc.NumDescriptors=samplerCursor;
        hr=device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&pending->samplers));if(FAILED(hr))return hr;
        pending->samplerStride=device->GetDescriptorHandleIncrementSize(desc.Type);
    }
    for(const auto& [key,at]:sharedOffsets) {
        const auto [heap,offset,count]=key;const auto type=heap->GetDesc().Type;
        const auto stride=type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER?pending->samplerStride:pending->stride;
        auto source=heap->GetCPUDescriptorHandleForHeapStart();source.ptr+=SIZE_T(offset)*stride;
        auto destination=type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER?pending->samplers->GetCPUDescriptorHandleForHeapStart():start;
        destination.ptr+=SIZE_T(at)*stride;device->CopyDescriptorsSimple(count,destination,source,type);
    }
    if(pending->constantBytes) {
        hr=Buffer(device,pending->constantBytes,pending->constants);if(FAILED(hr))return hr;
        void* mapped{};D3D12_RANGE none{};hr=pending->constants->Map(0,&none,&mapped);if(FAILED(hr))return hr;
        if(!mapped){pending->constants->Unmap(0,&none);return E_FAIL;}
        std::memset(mapped,0,size_t(pending->constantBytes));
        for(const auto& [bytes,offset]:constantOffsets)std::memcpy(static_cast<uint8_t*>(mapped)+offset,bytes.data(),bytes.size());
        const D3D12_RANGE written{0,SIZE_T(pending->constantBytes)};pending->constants->Unmap(0,&written);
    }
    for(auto slot:nullCbvs)device->CreateConstantBufferView(nullptr,cpu(slot));
    for(const auto& entry:values) {
        if(entry.srv)device->CreateShaderResourceView(entry.snapshot,entry.value->implicitSrv?nullptr:&entry.value->srv,cpu(entry.slot));
        else {D3D12_CONSTANT_BUFFER_VIEW_DESC cbv=entry.value->source?entry.value->gpuCbv:
                D3D12_CONSTANT_BUFFER_VIEW_DESC{pending->constants->GetGPUVirtualAddress()+entry.constantOffset,UINT((entry.value->constants.size()+255)&~size_t(255))};
            device->CreateConstantBufferView(&cbv,cpu(entry.slot));}
    }
    storage_=std::move(pending);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}

HRESULT StereoBindingBank::CreateSourcePass(ID3D12Device* device,std::span<const SourceBankDraw> inputs)try{
    using Stage=SourcePassFailureStage;using Reason=SourcePassFailureReason;
    sourcePassFailure_={};
    const auto reject=[&](Stage stage,auto bits,HRESULT result=E_INVALIDARG){sourcePassFailure_.stage=UINT(stage);sourcePassFailure_.bits=uint64_t(bits);return result;};
    if(storage_)return reject(Stage::Arguments,Reason::AlreadyInitialized);
    if(!device)return reject(Stage::Arguments,Reason::MissingDevice);
    if(inputs.empty())return reject(Stage::Arguments,Reason::EmptyInputs);
    if(inputs.size()>32768)return reject(Stage::Arguments,Reason::InputLimit);
    probe::Scope internal;ComPtr<Storage> pending;pending.Attach(new Storage);
    pending->device=device;pending->sourcePass=true;
    pending->stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    pending->samplerStride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER);
    if(!pending->stride || !pending->samplerStride)return reject(Stage::Arguments,Reason::Stride);
    struct Copy {ComPtr<ID3D12DescriptorHeap> heap;UINT offset{},count{},destination{};D3D12_DESCRIPTOR_HEAP_TYPE type{};UINT input{},parameter{};};
    struct Stable {uint64_t version{};ID3D12DescriptorHeap* heap{};UINT offset{},count{},destination{};D3D12_DESCRIPTOR_HEAP_TYPE type{};};
    using StorageRange=std::tuple<ID3D12RootSignature*,UINT,ID3D12DescriptorHeap*,UINT,UINT,D3D12_DESCRIPTOR_HEAP_TYPE>;
    std::vector<Copy> copies;std::vector<SourceLayout> layouts;
    std::map<ID3D12RootSignature*,size_t> roots;
    std::map<uint64_t,Stable> stable;
    std::map<uint64_t,StorageRange> storageScopes;
    std::map<StorageRange,UINT> storageRanges;
    std::set<IUnknown*> leased;UINT cursor{},samplerCursor{};UINT64 layoutBytes{};
    UINT inputIndex{};
    for(const auto& input:inputs){
        sourcePassFailure_.input=inputIndex++;sourcePassFailure_.parameter=UINT_MAX;
        if(!SameDevice(device,input.root.Get()))return reject(Stage::Input,Reason::RootDevice);
        if(input.tables.size()>64)return reject(Stage::Input,Reason::TableLimit);
        size_t layoutIndex{};const auto foundRoot=roots.find(input.root.Get());
        if(foundRoot==roots.end()){
            if(input.serializedRoot.empty() || input.serializedRoot.size()>1024*1024)return reject(Stage::RootBytes,Reason::BlobExtent);
            if(input.serializedRoot.size()>ConstantLimit-layoutBytes)return reject(Stage::RootBytes,Reason::BlobBudget,E_OUTOFMEMORY);
            std::vector<uint8_t> captured(input.serializedRoot.begin(),input.serializedRoot.end());
            SourceLayout layout;const auto hr=ReadSourceLayout(captured,layout,sourcePassFailure_);if(FAILED(hr))return hr;
            sourcePassFailure_.parameter=UINT_MAX;
            layoutBytes+=captured.size();layoutIndex=layouts.size();layouts.push_back(layout);
            pending->sourceLayouts.push_back(std::move(captured));
            roots.emplace(input.root.Get(),layoutIndex);
        }else{
            layoutIndex=foundRoot->second;const auto& bytes=pending->sourceLayouts[layoutIndex];
            if(input.serializedRoot.size()!=bytes.size() || !std::equal(bytes.begin(),bytes.end(),input.serializedRoot.begin()))return reject(Stage::RootBytes,Reason::BlobMismatch);
        }
        const auto& layout=layouts[layoutIndex];Storage::Draw draw;draw.root=input.root;
        std::set<UINT> parameters;
        for(const auto& table:input.tables){
            sourcePassFailure_.parameter=table.parameter;
            if(table.parameter>=layout.parameters)return reject(Stage::Table,Reason::Parameter);
            if(!parameters.insert(table.parameter).second)return reject(Stage::Table,Reason::DuplicateParameter);
            if(!table.cpuHeap)return reject(Stage::Table,Reason::CpuHeap);
            if(!SameDevice(device,table.cpuHeap.Get()))return reject(Stage::Table,Reason::CpuHeapDevice);
            if(bool(table.identity)!=bool(table.version))return reject(Stage::Table,Reason::LegacyLabels);
            if(bool(table.storageScope)!=bool(table.storageExtent))return reject(Stage::Storage,Reason::StorageLabels);
            if(table.storageScope && table.identity)return reject(Stage::Storage,Reason::MixedLabels);
            const auto& expected=layout.tables[table.parameter];const auto heap=table.cpuHeap->GetDesc();
            const bool extent=expected.unbounded?table.count>=expected.count &&
                uint64_t(expected.tailRegister)+table.count-expected.tailOffset<=uint64_t(UINT_MAX)+1:table.count==expected.count;
            if(heap.Type!=expected.type)return reject(Stage::Table,Reason::HeapType);
            if(heap.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_NONE)return reject(Stage::Table,Reason::HeapFlags);
            if(!extent)return reject(Stage::Table,Reason::SourceExtent);
            if(table.offset>heap.NumDescriptors || table.count>heap.NumDescriptors-table.offset)return reject(Stage::Table,Reason::SourceOffset);
            const UINT copiedExtent=table.storageScope?table.storageExtent:table.count;
            if(table.storageScope){const bool storageExtent=expected.unbounded?copiedExtent>=expected.count &&
                    uint64_t(expected.tailRegister)+copiedExtent-expected.tailOffset<=uint64_t(UINT_MAX)+1:copiedExtent==expected.count;
                if(table.count>copiedExtent || !storageExtent || copiedExtent>heap.NumDescriptors-table.offset)return reject(Stage::Storage,Reason::StorageExtent);}
            const bool sampler=heap.Type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
            if(!sampler && table.leases.empty())return reject(Stage::Lease,Reason::MissingLeases);
            for(const auto& owner:table.leases){if(!owner)return reject(Stage::Lease,Reason::NullLease);if(leased.insert(owner.Get()).second)pending->leases.push_back(owner);}
            auto& next=sampler?samplerCursor:cursor;const UINT limit=sampler?2048:DescriptorLimit;
            UINT destination{};bool copy=true;
            const StorageRange range{input.root.Get(),table.parameter,table.cpuHeap.Get(),table.offset,copiedExtent,heap.Type};
            if(table.storageScope){const auto [scope,inserted]=storageScopes.emplace(table.storageScope,range);
                if(!inserted && scope->second!=range)return reject(Stage::Storage,Reason::StorageConflict);
                const auto found=storageRanges.find(range);
                if(found!=storageRanges.end()){destination=found->second;copy=false;}
            }
            const auto found=table.identity?stable.find(table.identity):stable.end();
            if(found!=stable.end()){
                const auto& prior=found->second;
                if(prior.version!=table.version || prior.heap!=table.cpuHeap.Get() || prior.offset!=table.offset ||
                    prior.count!=table.count || prior.type!=heap.Type)return reject(Stage::Storage,Reason::LegacyConflict);
                destination=prior.destination;copy=false;
            }
            if(copy){
                if(copiedExtent>limit-next)return reject(Stage::Capacity,Bit(Reason::DescriptorCapacity)|Bit(sampler?Reason::SamplerCapacity:Reason::ResourceCapacity));
                destination=next;next+=copiedExtent;
                copies.push_back({table.cpuHeap,table.offset,copiedExtent,destination,heap.Type,sourcePassFailure_.input,table.parameter});
                if(table.identity)stable.emplace(table.identity,Stable{table.version,table.cpuHeap.Get(),table.offset,table.count,destination,heap.Type});
                if(table.storageScope)storageRanges.emplace(range,destination);
            }
            (sampler?draw.samplerTables:draw.tables).emplace_back(table.parameter,destination);
        }
        pending->draws.push_back(std::move(draw));
    }
    sourcePassFailure_.input=sourcePassFailure_.parameter=UINT_MAX;
    pending->descriptors=cursor;
    if(cursor){const D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,cursor,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
        const auto hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&pending->heap));if(FAILED(hr))return reject(Stage::ResourceHeap,Reason::HeapCreate,hr);}
    if(samplerCursor){const D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,samplerCursor,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
        const auto hr=device->CreateDescriptorHeap(&heap,IID_PPV_ARGS(&pending->samplers));if(FAILED(hr))return reject(Stage::SamplerHeap,Reason::HeapCreate,hr);}
    for(const auto& copy:copies){
        sourcePassFailure_.input=copy.input;sourcePassFailure_.parameter=copy.parameter;
        const bool sampler=copy.type==D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER;
        const UINT stride=sampler?pending->samplerStride:pending->stride;
        const auto source=copy.heap->GetCPUDescriptorHandleForHeapStart();
        const auto destination=(sampler?pending->samplers:pending->heap)->GetCPUDescriptorHandleForHeapStart();
        if(!source.ptr || !destination.ptr)return reject(Stage::Copy,Reason::Handle);
        if(uint64_t(copy.offset)+copy.count-1>(SIZE_MAX-source.ptr)/stride ||
            uint64_t(copy.destination)+copy.count-1>(SIZE_MAX-destination.ptr)/stride)return reject(Stage::Copy,Reason::CopyAddress);
        device->CopyDescriptorsSimple(copy.count,{destination.ptr+SIZE_T(copy.destination)*stride},{source.ptr+SIZE_T(copy.offset)*stride},copy.type);
    }
    storage_=std::move(pending);sourcePassFailure_={};return S_OK;
}catch(const std::bad_alloc&){sourcePassFailure_.stage=UINT(SourcePassFailureStage::Exception);sourcePassFailure_.bits=Bit(SourcePassFailureReason::AllocationException);return E_OUTOFMEMORY;}

HRESULT StereoBindingBank::CreateArena(ID3D12Device* device,ID3D12RootSignature* root,const PairedRootPlan& plan,UINT maxDraws,UINT64 bytes)try{
    if(storage_ || !device || !SameDevice(device,root) || !maxDraws || maxDraws>MaxArenaDraws || !bytes || bytes>ConstantLimit ||
        (bytes&255) || plan.bindings.empty() || plan.bindings.size()>32 || plan.serialized.empty() || plan.serialized.size()>1024*1024)return E_INVALIDARG;
    const uint64_t descriptors=uint64_t(plan.descriptors[0])+plan.descriptors[1];
    if(descriptors!=plan.bindings.size()*2 || descriptors*maxDraws>DescriptorLimit)return E_INVALIDARG;
    ComPtr<ID3D12VersionedRootSignatureDeserializer> decoder;
    auto hr=D3D12CreateVersionedRootSignatureDeserializer(plan.serialized.data(),plan.serialized.size(),IID_PPV_ARGS(&decoder));if(FAILED(hr))return hr;
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* decoded{};
    hr=decoder->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&decoded);if(FAILED(hr))return hr;
    if(decoded->Desc_1_1.NumParameters>64 || plan.originalParameters>decoded->Desc_1_1.NumParameters)return E_INVALIDARG;
    for(UINT stage=0;stage<2;++stage)if(plan.descriptors[stage]){
        if(plan.parameters[stage]<plan.originalParameters || !TableFits(decoded->Desc_1_1,plan.parameters[stage],plan.descriptors[stage],false))return E_INVALIDARG;
        const auto& table=decoded->Desc_1_1.pParameters[plan.parameters[stage]];
        if(table.ShaderVisibility!=(stage?D3D12_SHADER_VISIBILITY_PIXEL:D3D12_SHADER_VISIBILITY_VERTEX))return E_INVALIDARG;
        for(UINT i=0;i<table.DescriptorTable.NumDescriptorRanges;++i)if(table.DescriptorTable.pDescriptorRanges[i].RangeType!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV)return E_INVALIDARG;
    }
    if(plan.descriptors[0] && plan.descriptors[1] && plan.parameters[0]==plan.parameters[1])return E_INVALIDARG;
    std::vector<bool> used(size_t(descriptors),false);
    for(const auto& binding:plan.bindings){
        if(binding.binding.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV ||
            (binding.binding.visibility!=D3D12_SHADER_VISIBILITY_VERTEX && binding.binding.visibility!=D3D12_SHADER_VISIBILITY_PIXEL))return E_INVALIDARG;
        const auto stage=binding.binding.visibility==D3D12_SHADER_VISIBILITY_VERTEX?0u:1u;
        if(binding.destinationParameter!=plan.parameters[stage] || binding.destinationOffset>plan.descriptors[stage] || plan.descriptors[stage]-binding.destinationOffset<2)return E_INVALIDARG;
        const auto at=(stage?plan.descriptors[0]:0)+binding.destinationOffset;
        if(used[at] || used[at+1])return E_INVALIDARG;used[at]=used[at+1]=true;
    }
    probe::Scope internal;ComPtr<Storage> s;s.Attach(new Storage);s->device=device;s->arenaRoot=root;s->arenaPlan=plan;
    s->maxDraws=maxDraws;s->perDrawDescriptors=UINT(descriptors);s->capacity=bytes;s->draws.reserve(maxDraws);
    s->stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    const D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,UINT(descriptors)*maxDraws,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
    hr=device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&s->heap));if(FAILED(hr))return hr;
    hr=Buffer(device,bytes,s->constants);if(FAILED(hr))return hr;
    const D3D12_RANGE none{};hr=s->constants->Map(0,&none,&s->mapped);if(FAILED(hr))return hr;
    if(!s->mapped){s->constants->Unmap(0,&none);return E_FAIL;}
    storage_=std::move(s);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}

HRESULT StereoBindingBank::AppendConstants(const PairedRootPlan& plan,std::span<const BankPair> pairs,UINT& index)try{
    if(!storage_ || !storage_->arenaRoot || plan!=storage_->arenaPlan || pairs.size()!=plan.bindings.size() ||
        storage_->draws.size()>=storage_->maxDraws)return E_INVALIDARG;
    auto& s=*storage_.Get();std::vector<PreparedValue> values;values.reserve(pairs.size()*2);
    std::vector<bool> seen(pairs.size());UINT64 cursor=s.constantBytes;
    for(const auto& binding:plan.bindings){
        size_t at{};while(at<pairs.size() && pairs[at].binding!=binding.binding)++at;
        if(at==pairs.size() || seen[at])return E_INVALIDARG;seen[at]=true;
        const auto stage=binding.binding.visibility==D3D12_SHADER_VISIBILITY_VERTEX?0u:1u;
        for(UINT eye=0;eye<2;++eye){const auto& value=pairs[at].eyes[eye];
            if(value.source || value.owner || value.gpuCbv.BufferLocation || value.gpuCbv.SizeInBytes || value.borrowedSrv || value.implicitSrv ||
                value.constants.empty() || value.constants.size()>65536)return E_INVALIDARG;
            const UINT64 size=(value.constants.size()+255)&~UINT64(255);if(size>s.capacity-cursor)return E_OUTOFMEMORY;
            values.push_back({&value,s.descriptors+(stage?plan.descriptors[0]:0)+binding.destinationOffset+eye,cursor});cursor+=size;
        }
    }
    Storage::Draw draw;draw.root=s.arenaRoot;
    for(UINT stage=0;stage<2;++stage)if(plan.descriptors[stage])draw.tables.emplace_back(plan.parameters[stage],s.descriptors+(stage?plan.descriptors[0]:0));
    probe::Scope internal;const auto cpu=s.heap->GetCPUDescriptorHandleForHeapStart();const auto gpu=s.constants->GetGPUVirtualAddress();
    for(const auto& value:values){const auto size=UINT((value.value->constants.size()+255)&~size_t(255));
        auto* target=static_cast<uint8_t*>(s.mapped)+value.constantOffset;std::memset(target,0,size);
        std::memcpy(target,value.value->constants.data(),value.value->constants.size());
        const D3D12_CONSTANT_BUFFER_VIEW_DESC cbv{gpu+value.constantOffset,size};
        s.device->CreateConstantBufferView(&cbv,{cpu.ptr+SIZE_T(value.slot)*s.stride});
    }
    _mm_sfence();index=UINT(s.draws.size());s.draws.push_back(std::move(draw));s.constantBytes=cursor;s.descriptors+=s.perDrawDescriptors;return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool StereoBindingBank::RestartArena(){
    if(!storage_ || !storage_->arenaRoot)return false;
    auto& s=*storage_.Get();s.draws.clear();s.constantBytes=s.descriptors=s.capturedMask=0;s.recording=nullptr;return true;
}
bool StereoBindingBank::CapturedFor(ID3D12GraphicsCommandList* list)const{
    return storage_ && list && storage_->recording==list && storage_->capturedMask==3 && gpu::HasCommandResource(list,storage_.Get());
}

bool StereoBindingBank::CaptureEye(ID3D12GraphicsCommandList* list,UINT eye) {
    if(!storage_ || storage_->sourcePass || !list || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || eye>1 || (storage_->capturedMask&(1u<<eye)))return false;
    if(!SameDevice(storage_->device.Get(),list))return false;
    if(storage_->recording) {
        if(storage_->recording!=list || !cvr::gpu::HasCommandResource(list,storage_.Get()))return false;
    } else if(!cvr::gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    probe::Scope internal;
    for(const auto& copy:storage_->copies)if(copy.eye==eye) {
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        const bool transition=(copy.state&D3D12_RESOURCE_STATE_COPY_SOURCE)==0;
        if(transition){barrier.Transition={copy.source.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,copy.state,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&barrier);}
        list->CopyResource(copy.owned.Get(),copy.source.Get());
        if(transition){std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);list->ResourceBarrier(1,&barrier);}
        barrier.Transition={copy.owned.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,ShaderRead};list->ResourceBarrier(1,&barrier);
    }
    storage_->recording=list;storage_->capturedMask|=1u<<eye;return true;
}
bool StereoBindingBank::Bind(ID3D12GraphicsCommandList* list,UINT index) const {
    if(!storage_ || storage_->sourcePass || !list || index>=storage_->draws.size() || storage_->recording!=list || storage_->capturedMask!=3 || !cvr::gpu::HasCommandResource(list,storage_.Get()))return false;
    probe::Scope internal;
    const auto& draw=storage_->draws[index];
    ID3D12DescriptorHeap* heaps[]{storage_->heap.Get(),storage_->samplers.Get()};list->SetDescriptorHeaps(storage_->samplers?2:1,heaps);list->SetGraphicsRootSignature(draw.root.Get());
    const auto start=storage_->heap->GetGPUDescriptorHandleForHeapStart();
    for(const auto& [parameter,offset]:draw.tables)list->SetGraphicsRootDescriptorTable(parameter,{start.ptr+UINT64(offset)*storage_->stride});
    if(storage_->samplers){const auto samplerStart=storage_->samplers->GetGPUDescriptorHandleForHeapStart();
        for(const auto& [parameter,offset]:draw.samplerTables)list->SetGraphicsRootDescriptorTable(parameter,{samplerStart.ptr+UINT64(offset)*storage_->samplerStride});}
    return true;
}
bool StereoBindingBank::BindSourceEye(ID3D12GraphicsCommandList* list,UINT eye,UINT index)const {
    if(!storage_ || storage_->sourcePass || !list || eye>1 || index>=storage_->draws.size() || storage_->recording!=list || storage_->capturedMask!=3 ||
        !cvr::gpu::HasCommandResource(list,storage_.Get()))return false;
    const auto& draw=storage_->draws[index];if(!draw.sourceRoot || draw.sourceTables[eye].empty())return false;
    probe::Scope internal;ID3D12DescriptorHeap* heap=storage_->heap.Get();list->SetDescriptorHeaps(1,&heap);list->SetGraphicsRootSignature(draw.sourceRoot.Get());
    const auto start=storage_->heap->GetGPUDescriptorHandleForHeapStart();
    for(const auto& [parameter,offset]:draw.sourceTables[eye])list->SetGraphicsRootDescriptorTable(parameter,{start.ptr+UINT64(offset)*storage_->stride});
    return true;
}
bool StereoBindingBank::BindSource(ID3D12GraphicsCommandList* list,UINT index)const{
    if(!storage_ || !storage_->sourcePass || !list || index>=storage_->draws.size() ||
        list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT || !SameDevice(storage_->device.Get(),list))return false;
    if(!gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    probe::Scope internal;const auto& draw=storage_->draws[index];
    ID3D12DescriptorHeap* heaps[2]{};UINT count{};
    if(storage_->heap)heaps[count++]=storage_->heap.Get();if(storage_->samplers)heaps[count++]=storage_->samplers.Get();
    list->SetDescriptorHeaps(count,heaps);list->SetGraphicsRootSignature(draw.root.Get());
    if(storage_->heap){const auto start=storage_->heap->GetGPUDescriptorHandleForHeapStart();
        for(const auto& [parameter,offset]:draw.tables)list->SetGraphicsRootDescriptorTable(parameter,{start.ptr+UINT64(offset)*storage_->stride});}
    if(storage_->samplers){const auto start=storage_->samplers->GetGPUDescriptorHandleForHeapStart();
        for(const auto& [parameter,offset]:draw.samplerTables)list->SetGraphicsRootDescriptorTable(parameter,{start.ptr+UINT64(offset)*storage_->samplerStride});}
    return true;
}
UINT StereoBindingBank::DescriptorCount()const{return storage_?storage_->descriptors:0;}
UINT StereoBindingBank::DrawCount()const{return storage_?UINT(storage_->draws.size()):0;}
UINT StereoBindingBank::SnapshotCount()const{return storage_?UINT(storage_->copies.size()):0;}
UINT64 StereoBindingBank::ConstantBytes()const{return storage_?storage_->constantBytes:0;}
#endif
}
