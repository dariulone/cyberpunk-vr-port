#include "Render/SourceBindingAssembly.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/SourceDescriptorViews.hpp"
#include <algorithm>
#include <cfloat>
#include <functional>
#include <map>
#include <new>
#include <set>

#if CVR_RUST_VRIK
// The root and view rules: rust/crates/cvr-render/src/assembly_layout.rs (tools/single_pass_tests/source_binding_assembly_test
// runs its cases against them in its Rust variant). The ranges cross as the .cpp's Range, mirrored there.
extern "C" {
bool cvr_sba_srv(const D3D12_SHADER_RESOURCE_VIEW_DESC*,const D3D12_SHADER_RESOURCE_VIEW_DESC*);
}
#endif
namespace cvr::stereo::source_binding_assembly {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's assembly is assembly_build.rs' and shared_table.rs'
// The definition and construction stay private to this translation unit.
// Public tables can retain a receipt, but cannot nominate a new table for it.
struct OwnedSrvCoverage {
    Scope source;
    SharedBankTable table;
    std::shared_ptr<const std::vector<OwnedSrvSlot>> slots;
};
namespace {
#if !CVR_RUST_VRIK
constexpr UINT ResourceLimit=1000000,SamplerLimit=2048;
#endif
#if CVR_RUST_VRIK
// The root layouts are Rust's (assembly_layout.rs, used by assembly_build.rs).
#else
struct Range {UINT offset{},count{};D3D12_DESCRIPTOR_RANGE1 desc{};};
struct Layout {UINT minimum{};bool tail{},sampler{};std::vector<Range> ranges;};
#if CVR_RUST_VRIK
struct LayoutHead {UINT minimum{};bool tail{},sampler{};};
static_assert(sizeof(Range)==32 && sizeof(LayoutHead)==8,"mirrored by assembly_layout.rs");
LayoutHead Head(const Layout& layout){return {layout.minimum,layout.tail,layout.sampler};}
#endif
#endif
bool ValidScope(const Scope& s){return s.root && s.list && s.generation && s.occurrence && s.source && s.eye<2;}
#if CVR_RUST_VRIK
// Layouts, Extent, SlotRange, CpuHeap, CbvRange and Canonical are BuildCore's, in Rust (assembly_build.rs).
#else
bool Layouts(const D3D12_ROOT_SIGNATURE_DESC1& root,std::array<Layout,64>& layouts){
#if CVR_RUST_VRIK
    std::array<LayoutHead,64> heads{};
    const auto push=[](void* all,UINT parameter,const void* ranges,size_t count){
        const auto* first=static_cast<const Range*>(ranges);(*static_cast<std::array<Layout,64>*>(all))[parameter].ranges.assign(first,first+count);};
    if(!cvr_sba_layouts(&root,heads.data(),&layouts,push))return false;
    for(UINT p=0;p<root.NumParameters;++p){layouts[p].minimum=heads[p].minimum;layouts[p].tail=heads[p].tail;layouts[p].sampler=heads[p].sampler;}
    return true;
}
#else
    if(!root.NumParameters || root.NumParameters>64 || !root.pParameters || root.Flags&(
        D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE|D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED|
        D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED))return false;
    UINT totalRanges{};
    for(UINT p=0;p<root.NumParameters;++p){const auto& parameter=root.pParameters[p];
        if(parameter.ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE ||
            !parameter.DescriptorTable.NumDescriptorRanges || !parameter.DescriptorTable.pDescriptorRanges ||
            parameter.DescriptorTable.NumDescriptorRanges>4096-totalRanges)return false;
        totalRanges+=parameter.DescriptorTable.NumDescriptorRanges;
        auto& layout=layouts[p];uint64_t next{};
        layout.sampler=parameter.DescriptorTable.pDescriptorRanges[0].RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;
        const UINT limit=layout.sampler?SamplerLimit:ResourceLimit;
        for(UINT n=0;n<parameter.DescriptorTable.NumDescriptorRanges;++n){const auto& range=parameter.DescriptorTable.pDescriptorRanges[n];
            if(UINT(range.RangeType)>UINT(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) || !range.NumDescriptors || layout.tail ||
                (range.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER)!=layout.sampler)return false;
            const uint64_t offset=range.OffsetInDescriptorsFromTableStart==UINT_MAX?next:range.OffsetInDescriptorsFromTableStart;
            const bool tail=range.NumDescriptors==UINT_MAX;
            const uint64_t end=tail?uint64_t(UINT_MAX)+1:offset+range.NumDescriptors;
            if(offset>=limit || (!tail && end>limit) || (!tail && uint64_t(range.BaseShaderRegister)+range.NumDescriptors>uint64_t(UINT_MAX)+1) ||
                (tail && (n+1!=parameter.DescriptorTable.NumDescriptorRanges || offset<layout.minimum)))return false;
            for(const auto& prior:layout.ranges)if(offset<uint64_t(prior.offset)+prior.count && prior.offset<end)return false;
            layout.ranges.push_back({UINT(offset),range.NumDescriptors,range});layout.tail=tail;
            layout.minimum=std::max(layout.minimum,UINT(tail?offset+1:end));next=end;
        }
    }return true;
}
#endif
bool Extent(const Layout& layout,UINT count){
#if CVR_RUST_VRIK
    const auto head=Head(layout);return cvr_sba_extent(&head,layout.ranges.empty()?nullptr:&layout.ranges.back(),count);
#else
    const UINT limit=layout.sampler?SamplerLimit:ResourceLimit;
    if(!count || count>limit || (layout.tail?count<layout.minimum:count!=layout.minimum))return false;
    return !layout.tail || uint64_t(layout.ranges.back().desc.BaseShaderRegister)+count-layout.ranges.back().offset<=uint64_t(UINT_MAX)+1;
#endif
}
const Range* SlotRange(const Layout& layout,UINT offset){
#if CVR_RUST_VRIK
    const auto index=cvr_sba_slot_range(layout.ranges.data(),layout.ranges.size(),offset);return index<0?nullptr:&layout.ranges[size_t(index)];
#else
    for(const auto& range:layout.ranges)if(offset>=range.offset && uint64_t(offset)-range.offset<range.count)return &range;return nullptr;
#endif
}
bool CpuHeap(ID3D12DescriptorHeap* heap,D3D12_DESCRIPTOR_HEAP_TYPE type,UINT count){
    if(!heap)return false;const auto desc=heap->GetDesc();
    return desc.Type==type && desc.Flags==D3D12_DESCRIPTOR_HEAP_FLAG_NONE && desc.NumDescriptors==count;
}
bool CbvRange(ID3D12Resource* resource,const D3D12_CONSTANT_BUFFER_VIEW_DESC& view,UINT reflected,UINT64 offset,bool privateBuffer){
#if CVR_RUST_VRIK
    return cvr_sba_cbv_range(resource,&view,reflected,offset,privateBuffer);
}
#else
    if(!resource || !reflected || reflected>65536 || !view.SizeInBytes || view.SizeInBytes>65536 ||
        (view.SizeInBytes&255) || reflected>view.SizeInBytes || !view.BufferLocation || (view.BufferLocation&255))return false;
    const auto desc=resource->GetDesc();const auto gpu=resource->GetGPUVirtualAddress();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || !gpu || gpu>UINT64_MAX-desc.Width ||
        offset>desc.Width || view.SizeInBytes>desc.Width-offset || view.BufferLocation!=gpu+offset)return false;
    if(privateBuffer){D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        if(FAILED(resource->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return false;}
    return true;
}
#endif
bool Canonical(const descriptors::OwnedSourceSlot& slot){
    if(slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){descriptor_views::CbvResult result;return descriptor_views::DecodeCbv(slot.stamp.value,result);}
    if(slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV){descriptor_views::SrvResult result;return descriptor_views::DecodeSrv(slot.stamp.value,result);}
    if(slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV){descriptor_views::UavResult result;return descriptor_views::DecodeUav(slot.stamp.value,result);}
    if(slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER){descriptor_views::SamplerResult result;return descriptor_views::DecodeSampler(slot.stamp.value,result);}
    return false;
}
#endif
bool SameDevice(ID3D12Device* expected,ID3D12DeviceChild* child){
    Microsoft::WRL::ComPtr<ID3D12Device> device;return child && SUCCEEDED(child->GetDevice(IID_PPV_ARGS(&device))) && device.Get()==expected;
}
bool SameStamp(const descriptors::DescriptorStamp& a,const descriptors::DescriptorStamp& b){
    return a.gpu==b.gpu && a.heap==b.heap && a.value==b.value;
}
bool CoverageShape(const SharedBankTable& table,const SharedBankTable& expected){
    if(!table.cpuHeap || table.cpuHeap!=expected.cpuHeap || table.parameter!=expected.parameter || table.offset!=expected.offset ||
        table.count!=expected.count || table.identity!=expected.identity || table.version!=expected.version ||
        table.storageScope!=expected.storageScope || table.storageExtent!=expected.storageExtent ||
        !table.count || !table.storageScope || table.count>table.storageExtent)return false;
    const auto desc=table.cpuHeap->GetDesc();
    if(desc.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || desc.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_NONE ||
        table.offset>desc.NumDescriptors || table.storageExtent>desc.NumDescriptors-table.offset)return false;
    std::set<IUnknown*> owners;for(const auto& lease:table.leases)if(lease)owners.insert(lease.Get());
    for(const auto& lease:expected.leases)if(!lease || !owners.contains(lease.Get()))return false;
    return true;
}
bool IssueCoverage(SharedBankTable& table,const Scope& source,std::span<const OwnedSrvSlot> slots,
                   std::span<const descriptors::OwnedSourceSlot> emitted){
    table.srvCoverage.reset();if(slots.empty())return true;
    if(!ValidScope(source) || !CoverageShape(table,table) || emitted.empty())return false;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    if(FAILED(table.cpuHeap->GetDevice(IID_PPV_ARGS(&device))) || !device)return false;
    const UINT stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);if(!stride)return false;
    std::set<IUnknown*> owners;for(const auto& lease:table.leases)if(lease)owners.insert(lease.Get());
    std::map<UINT,const descriptors::OwnedSourceSlot*> copied;
    for(const auto& slot:emitted)if(slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV){
        const auto [found,inserted]=copied.emplace(slot.offset,&slot);
        if(!inserted && !SameStamp(found->second->stamp,slot.stamp))return false;
    }
    for(const auto& slot:slots){descriptor_views::SrvResult decoded;
        if(!ValidScope(slot.contributor) || slot.contributor.root!=source.root || !slot.originalOwner || !slot.originalCpuHeap ||
            !slot.originalIdentity || !slot.originalVersion || !slot.originalGpu || !slot.originalHeap ||
            slot.offset>=slot.originalCount || slot.offset>=table.storageExtent ||
            !owners.contains(slot.originalOwner.Get()) || !owners.contains(slot.originalCpuHeap.Get()) ||
            !SameDevice(device.Get(),slot.originalCpuHeap.Get()) ||
            uint64_t(slot.offset)>(UINT64_MAX-slot.originalGpu)/stride || slot.stamp.gpu!=slot.originalGpu+uint64_t(slot.offset)*stride ||
            slot.stamp.heap!=slot.originalHeap || !descriptor_views::DecodeSrv(slot.stamp.value,decoded) || decoded.resourceCookie!=slot.resourceCookie)return false;
        const auto origin=slot.originalCpuHeap->GetDesc();
        if(origin.Type!=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || origin.Flags!=D3D12_DESCRIPTOR_HEAP_FLAG_NONE ||
            slot.compactIndex>=origin.NumDescriptors)return false;
        const auto match=copied.find(slot.offset);
        if(match==copied.end() || !SameStamp(match->second->stamp,slot.stamp))return false;
    }
    auto receipt=std::make_shared<OwnedSrvCoverage>();receipt->source=source;receipt->table=table;
    receipt->slots=std::make_shared<const std::vector<OwnedSrvSlot>>(slots.begin(),slots.end());table.srvCoverage=std::move(receipt);return true;
}
bool ReissueCoverage(SharedBankTable& table,const Scope& source,const SharedBankTable& physical){
    table.srvCoverage.reset();const auto& prior=physical.srvCoverage;if(!prior)return true;
    if(!ValidScope(source) || source.root!=prior->source.root || !CoverageShape(physical,prior->table))return false;
    auto expected=prior->table;expected.count=table.count;
    if(!CoverageShape(table,expected))return false;
    auto receipt=std::make_shared<OwnedSrvCoverage>();receipt->source=source;receipt->table=table;
    receipt->slots=prior->slots;table.srvCoverage=std::move(receipt);return true;
}
bool ExactStructured(const D3D12_SHADER_RESOURCE_VIEW_DESC& view){
#if CVR_RUST_VRIK
    return cvr_sba_srv(&view,nullptr);
#else
    return view.Format==DXGI_FORMAT_UNKNOWN && view.ViewDimension==D3D12_SRV_DIMENSION_BUFFER &&
        view.Shader4ComponentMapping==D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING && !view.Buffer.FirstElement &&
        view.Buffer.NumElements==8192 && view.Buffer.StructureByteStride==128 && !view.Buffer.Flags;
#endif
}
bool SameSrv(const D3D12_SHADER_RESOURCE_VIEW_DESC& a,const D3D12_SHADER_RESOURCE_VIEW_DESC& b){
#if CVR_RUST_VRIK
    return cvr_sba_srv(&a,&b);
#else
    return a.Format==b.Format && a.ViewDimension==b.ViewDimension && a.Shader4ComponentMapping==b.Shader4ComponentMapping &&
        a.Buffer.FirstElement==b.Buffer.FirstElement && a.Buffer.NumElements==b.Buffer.NumElements &&
        a.Buffer.StructureByteStride==b.Buffer.StructureByteStride && a.Buffer.Flags==b.Buffer.Flags;
#endif
}
#if CVR_RUST_VRIK
// NullSlot is Assemble's, in Rust (assembly_pass.rs).
#else
void NullSlot(ID3D12Device* device,D3D12_DESCRIPTOR_RANGE_TYPE type,D3D12_CPU_DESCRIPTOR_HANDLE target){
    if(type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){device->CreateConstantBufferView(nullptr,target);return;}
    if(type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV){D3D12_UNORDERED_ACCESS_VIEW_DESC view{};view.Format=DXGI_FORMAT_R32_UINT;
        view.ViewDimension=D3D12_UAV_DIMENSION_BUFFER;view.Buffer.NumElements=1;device->CreateUnorderedAccessView(nullptr,nullptr,&view,target);return;}
    if(type==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER){D3D12_SAMPLER_DESC view{};view.Filter=D3D12_FILTER_MIN_MAG_MIP_POINT;
        view.AddressU=view.AddressV=view.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;view.MaxAnisotropy=1;
        view.ComparisonFunc=D3D12_COMPARISON_FUNC_NEVER;view.MaxLOD=FLT_MAX;device->CreateSampler(&view,target);return;}
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};view.Format=DXGI_FORMAT_R32_FLOAT;view.ViewDimension=D3D12_SRV_DIMENSION_BUFFER;
    view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;view.Buffer.NumElements=1;device->CreateShaderResourceView(nullptr,&view,target);
}
#endif
}
bool DescribeOwnedSrvCoverage(const SharedBankTable& table,const Scope& source,OwnedSrvCoverageView& output)noexcept{
    output={};try{
        const auto& receipt=table.srvCoverage;
        if(!receipt || !ValidScope(source) || source!=receipt->source || !receipt->slots || receipt->slots->empty() || !CoverageShape(table,receipt->table))return false;
        output.source=source;output.owner=receipt;output.slots=*receipt->slots;return true;
    }catch(...){output={};return false;}
}
bool MatchesOwnedSrvCoverage(const SharedBankTable& table,const Scope& source,UINT offset,
                            const descriptors::DescriptorStamp& stamp,OwnedSrvCoverageView& output)noexcept{
    output={};try{
        OwnedSrvCoverageView view;if(!DescribeOwnedSrvCoverage(table,source,view))return false;
        descriptor_views::SrvResult decoded;if(!descriptor_views::DecodeSrv(stamp.value,decoded))return false;
        if(!std::any_of(view.slots.begin(),view.slots.end(),[&](const auto& slot){return slot.offset==offset &&
            slot.resourceCookie==decoded.resourceCookie && SameStamp(slot.stamp,stamp);}))return false;
        output=std::move(view);return true;
    }catch(...){output={};return false;}
}
struct GuardedValidation {const structured_snapshot_view::PrivateView* t5{};Microsoft::WRL::ComPtr<IUnknown> receipt;};
#if CVR_RUST_VRIK
// The plan is Rust's (rust/crates/cvr-render/src/assembly_build.rs): the input crosses as flat views (the private view's own
// validation and its record plan computed here), and the plan's tables are materialized from Rust's, their owners
// referenced again from the lent objects.
struct RustStr {const char* data;size_t size;};
struct RustOwned {Scope scope;UINT present,parameter,count;uint64_t cpuHeap,owner,identity,version,sourceGpu,sourceHeap;
    const descriptors::OwnedSourceSlot* slots;size_t slotCount;};
struct RustPrivateCbv {Scope scope;uint64_t value,copies;UINT parameter,offset,reflected,padded;uint64_t privateOffset,copied;
    D3D12_CONSTANT_BUFFER_VIEW_DESC view;uint64_t resource,owner,copiesOwner,copiesList,copiesGeneration,copiesOccurrence,copiesCopied;UINT copiesCount;
    bool finalized;};
struct RustOriginalCbv {Scope scope;UINT parameter,offset,reflected;D3D12_CONSTANT_BUFFER_VIEW_DESC view;uint64_t resource,owner;bool certified;};
struct RustPrivateSrv {Scope scope;uint64_t value;UINT parameter,offset;structured_snapshot_view::Scope viewScope;uint64_t viewResource,viewOwner;
    D3D12_SHADER_RESOURCE_VIEW_DESC viewSrv;uint64_t viewSourceOffset,viewCopied;UINT viewFirstRecord,viewRecordSpan,viewRecordCount;
    std::array<uint32_t,multilayer_selectors::MaxRecords> viewRecords;std::array<uint8_t,multilayer_selectors::B4Bytes> viewB4;bool viewValid;
    descriptors::Value originalDescription;uint64_t originalResource;D3D12_SHADER_RESOURCE_VIEW_DESC originalView;descriptors::DescriptorStamp originalStamp;
    uint64_t originalOwner,originalIdentity,originalVersion,originalGpu,originalHeap,resourceCookie;UINT originalCount;bool slotCertified;
    uint64_t b4,b4Copies,b4Resource,b4Owner;UINT b4Reflected;uint64_t b4Copied,b4CopiesOwner;const uint8_t* b4Bytes;size_t b4BytesLength;
    bool b4BytesCertified,planOk,planCoverageComplete,planSuperset;const uint32_t* planRecords;size_t planRecordCount;UINT planFirstRecord,planRecordSpan;
    uint64_t planResourceOffset;UINT planByteExtent;};
struct RustSelectorTable {RustStr root;uint32_t visibility,parameter,space,shaderRegisterBase,originalTableOffset,declaredExtent;const uint32_t* offsets;
    size_t offsetCount;uint32_t count;const source_selectors::SlotInterval* uses;size_t useCount;};
struct RustSelectors {RustStr family[3];uint32_t evaluatedSites,suppressed;bool complete,permission,permitted;const source_selectors::SiteSelection* selections;
    size_t selectionCount;const RustSelectorTable* tables;size_t tableCount;};
struct RustInput {Scope scope;const D3D12_ROOT_SIGNATURE_DESC1* root;const families::ResourceUse* uses;size_t useCount;const RustOwned* finite;
    size_t finiteCount;const RustOwned* sparse;size_t sparseCount;const RustPrivateCbv* privateCbvs;size_t privateCbvCount;const RustOriginalCbv* originalCbvs;
    size_t originalCbvCount;RustStr family[3];Scope selectorScope;const RustSelectors* selectors;bool dynamicCoverageCertified,guarded;
    const RustPrivateSrv* privateSrvs;size_t privateSrvCount;uint64_t guardedT5,guardedReceipt;RustStr multilayer[3];};
struct RustCopy {UINT offset,sourceOffset,count;uint64_t heap;};
struct RustCbvRow {UINT offset;D3D12_CONSTANT_BUFFER_VIEW_DESC view;};
struct RustSrvRow {UINT offset;D3D12_SHADER_RESOURCE_VIEW_DESC view;uint64_t resource;};
struct RustCoverage {Scope contributor;UINT offset,compactIndex;descriptors::DescriptorStamp stamp;uint64_t resourceCookie,originalIdentity,
    originalVersion,originalGpu,originalHeap;UINT originalCount;uint64_t originalOwner,originalCpuHeap;};
struct RustPlanHead {Scope scope;size_t tables;const std::array<UINT,3>* missing;size_t missingCount;UINT descriptors;bool ready;};
struct RustTableView {UINT parameter,count;Scope sourceScope;int32_t kind;const int32_t* slotTypes;size_t slotTypeCount;const RustCopy* copies;
    size_t copyCount;const RustCbvRow* cbvs;size_t cbvCount;const RustSrvRow* srvs;size_t srvCount;uint64_t sourceRoot,sourceGpu,sourceHeap;
    const descriptors::OwnedSourceSlot* sparseSlots;size_t sparseSlotCount;const RustCoverage* coverage;size_t coverageCount;bool sparseUnionCertified,unionStorage;
    const uint64_t* descriptorOwners;size_t descriptorOwnerCount;const uint64_t* leases;size_t leaseCount;};
static_assert(sizeof(Scope)==48 && sizeof(descriptors::DescriptorStamp)==176 && sizeof(descriptors::OwnedSourceSlot)==192 && sizeof(RustOwned)==128 &&
    sizeof(RustPrivateCbv)==176 && sizeof(RustOriginalCbv)==104 && sizeof(structured_snapshot_view::Scope)==40 && sizeof(RustPrivateSrv)==1112 &&
    sizeof(RustSelectorTable)==80 && sizeof(RustSelectors)==96 && sizeof(RustInput)==328 && sizeof(RustCopy)==24 && sizeof(RustCbvRow)==24 &&
    sizeof(RustSrvRow)==56 && sizeof(RustCoverage)==296 && sizeof(RustPlanHead)==80 && sizeof(RustTableView)==224 &&
    sizeof(source_selectors::SlotInterval)==12 && sizeof(source_selectors::SiteSelection)==16,"mirrored by assembly_build.rs");
extern "C" bool cvr_sba_build_core(const void*,void**,UINT*);
extern "C" void cvr_sba_plan_head(const void*,void*);
extern "C" void cvr_sba_plan_table(const void*,size_t,void*);
extern "C" void cvr_sba_plan_free(void*);
bool BuildCore(const Input& input,const GuardedValidation* guarded,Plan& output,Failure& failure)try{
    output={};failure=Failure::Arguments;
    const auto at=[](const void* value){return uint64_t(reinterpret_cast<uintptr_t>(value));};
    const auto str=[](std::string_view text){return RustStr{text.data(),text.size()};};
    const auto owned=[&](const Scope& scope,const auto* table,const descriptors::OwnedSourceSlot* slots,size_t count){
        return table?RustOwned{scope,1,table->parameter,table->count,at(table->cpuHeap.Get()),at(table->owner.Get()),table->identity,table->version,
            table->sourceGpu,table->sourceHeap,slots,count}:RustOwned{scope};};
    std::vector<RustOwned> finite,sparse;std::vector<RustPrivateCbv> cbvs;std::vector<RustOriginalCbv> originals;
    for(const auto& q:input.finite)finite.push_back(owned(q.scope,q.table,nullptr,0));
    for(const auto& q:input.sparse)sparse.push_back(owned(q.scope,q.table,q.table?q.table->slots.data():nullptr,q.table?q.table->slots.size():0));
    for(const auto& q:input.privateCbvs){RustPrivateCbv c{q.scope,at(q.value),at(q.copies)};
        if(q.value){const auto& v=*q.value;c.parameter=v.parameter;c.offset=v.offset;c.reflected=v.reflectedBytes;c.padded=v.paddedBytes;
            c.privateOffset=v.privateOffset;c.copied=v.copiedBytes;c.view=v.view;c.resource=at(v.resource.Get());c.owner=at(v.owner.Get());}
        if(q.copies){const auto& k=*q.copies;c.copiesOwner=at(k.owner.Get());c.copiesList=k.list;c.copiesGeneration=k.generation;
            c.copiesOccurrence=k.occurrence;c.copiesCopied=k.copiedBytes;c.copiesCount=k.count;}
        c.finalized=q.finalized;cbvs.push_back(c);}
    for(const auto& v:input.originalCbvs)originals.push_back({v.scope,v.parameter,v.offset,v.reflectedBytes,v.view,at(v.resource.Get()),at(v.owner.Get()),
        v.readEpochCertified});
    std::vector<RustPrivateSrv> srvs(input.privateSrvs.size());std::vector<multilayer_selectors::ReadPlan> plans(input.privateSrvs.size());
    for(size_t n=0;n<input.privateSrvs.size();++n){const auto& q=input.privateSrvs[n];auto& s=srvs[n];
        s.scope=q.scope;s.value=at(q.value);s.parameter=q.parameter;s.offset=q.offset;
        if(q.value){const auto& v=*q.value;s.viewScope=v.scope;s.viewResource=at(v.resource.Get());s.viewOwner=at(v.owner.Get());s.viewSrv=v.srv;
            s.viewSourceOffset=v.sourceOffset;s.viewCopied=v.copiedBytes;s.viewFirstRecord=v.firstRecord;s.viewRecordSpan=v.recordSpan;
            s.viewRecordCount=v.recordCount;s.viewRecords=v.records;s.viewB4=v.privateB4Bytes;s.viewValid=structured_snapshot_view::ValidatePrivateView(v);
            // The record plan of the full T5 domain (the only resource width the plan admits).
            multilayer_selectors::StructuredView view;view.key={std::string(multilayer_selectors::RootSha256),5,5,0,23,5};
            view.resourceWidth=structured_snapshot_view::DomainBytes;view.numElements=8192;view.structureByteStride=128;view.dimension=1;
            view.componentMapping=5768;view.explicitDescription=true;auto& checked=plans[n];std::string error;
            s.planOk=multilayer_selectors::PlanRead(input.family,{{std::string(multilayer_selectors::RootSha256),5,4,0,19,0},v.privateB4Bytes},view,
                checked,error);
            s.planCoverageComplete=checked.originalSiteCoverageComplete;s.planSuperset=checked.conservativeRecordSuperset;
            s.planRecords=checked.records.data();s.planRecordCount=checked.records.size();s.planFirstRecord=checked.firstRecord;
            s.planRecordSpan=checked.recordSpan;s.planResourceOffset=checked.resourceOffset;s.planByteExtent=checked.byteExtent;}
        s.originalDescription=q.original.description;s.originalResource=at(q.original.resource.Get());s.originalView=q.original.view;
        s.originalStamp=q.original.stamp;s.originalOwner=at(q.originalOwner.Get());s.originalIdentity=q.originalIdentity;s.originalVersion=q.originalVersion;
        s.originalGpu=q.originalGpu;s.originalHeap=q.originalHeap;s.resourceCookie=q.resourceCookie;s.originalCount=q.originalCount;
        s.slotCertified=q.originalSlotCertified;s.b4=at(q.privateB4);s.b4Copies=at(q.privateB4Copies);
        if(q.privateB4){s.b4Resource=at(q.privateB4->resource.Get());s.b4Owner=at(q.privateB4->owner.Get());s.b4Reflected=q.privateB4->reflectedBytes;
            s.b4Copied=q.privateB4->copiedBytes;}
        if(q.privateB4Copies)s.b4CopiesOwner=at(q.privateB4Copies->owner.Get());
        s.b4Bytes=q.privateB4Bytes.data();s.b4BytesLength=q.privateB4Bytes.size();s.b4BytesCertified=q.privateB4BytesCertified;}
    std::vector<RustSelectorTable> tables;RustSelectors selectors{};
    if(input.selectors){const auto& r=*input.selectors;tables.reserve(r.tables.size());
        for(const auto& table:r.tables){const auto& key=table.key;
            tables.push_back({str(key.rootSha256),key.visibility,key.parameter,key.space,key.shaderRegisterBase,key.originalTableOffset,key.declaredExtent,
                table.offsets.data(),table.offsets.size(),table.count,table.uses.data(),table.uses.size()});}
        selectors={{str(r.family.rootSha256),str(r.family.vertexSha256),str(r.family.pixelSha256)},r.evaluatedSites,r.nativeCommandsSuppressed,
            r.selectorEvaluationComplete,r.nativeExecutionPermission,r.nativeExecutionPermitted,r.selections.data(),r.selections.size(),tables.data(),
            tables.size()};}
    const RustInput lent{input.scope,input.root,input.shaderUses.data(),input.shaderUses.size(),finite.data(),finite.size(),sparse.data(),sparse.size(),
        cbvs.data(),cbvs.size(),originals.data(),originals.size(),{str(input.family.rootSha256),str(input.family.vertexSha256),str(input.family.pixelSha256)},
        input.selectorScope,input.selectors?&selectors:nullptr,input.dynamicCoverageCertified,guarded!=nullptr,srvs.data(),srvs.size(),
        guarded?at(guarded->t5):0,guarded?at(guarded->receipt.Get()):0,
        {str(multilayer_selectors::RootSha256),str(multilayer_selectors::VertexSha256),str(multilayer_selectors::PixelSha256)}};
    void* built{};UINT reason{};
    if(!cvr_sba_build_core(&lent,&built,&reason)){failure=Failure(reason);return false;}
    const std::unique_ptr<void,void(*)(void*)> held(built,&cvr_sba_plan_free);
    RustPlanHead head{};cvr_sba_plan_head(built,&head);
    Plan pending;pending.scope_=head.scope;pending.descriptors_=head.descriptors;pending.ready_=head.ready;
    for(size_t n=0;n<head.missingCount;++n)pending.missing_.push_back({head.missing[n][0],head.missing[n][1],head.missing[n][2]});
    const auto unknown=[](uint64_t value){return Microsoft::WRL::ComPtr<IUnknown>(reinterpret_cast<IUnknown*>(value));};
    for(size_t index=0;index<head.tables;++index){RustTableView v{};cvr_sba_plan_table(built,index,&v);Plan::Table table;
        table.parameter=v.parameter;table.count=v.count;table.sourceScope=v.sourceScope;table.type=D3D12_DESCRIPTOR_HEAP_TYPE(v.kind);
        for(size_t n=0;n<v.slotTypeCount;++n)table.slotTypes.push_back(D3D12_DESCRIPTOR_RANGE_TYPE(v.slotTypes[n]));
        for(size_t n=0;n<v.copyCount;++n){const auto& c=v.copies[n];
            table.copies.push_back({c.offset,c.sourceOffset,c.count,reinterpret_cast<ID3D12DescriptorHeap*>(c.heap)});}
        for(size_t n=0;n<v.cbvCount;++n)table.cbvs.push_back({v.cbvs[n].offset,v.cbvs[n].view});
        for(size_t n=0;n<v.srvCount;++n)table.srvs.push_back({v.srvs[n].offset,v.srvs[n].view,reinterpret_cast<ID3D12Resource*>(v.srvs[n].resource)});
        table.sourceRoot=v.sourceRoot;table.sourceGpu=v.sourceGpu;table.sourceHeap=v.sourceHeap;
        if(v.sparseSlotCount)table.sparseSlots.assign(v.sparseSlots,v.sparseSlots+v.sparseSlotCount);
        for(size_t n=0;n<v.coverageCount;++n){const auto& c=v.coverage[n];
            table.srvCoverage.push_back({c.contributor,c.offset,c.compactIndex,c.stamp,c.resourceCookie,c.originalIdentity,c.originalVersion,c.originalGpu,
                c.originalHeap,c.originalCount,unknown(c.originalOwner),reinterpret_cast<ID3D12DescriptorHeap*>(c.originalCpuHeap)});}
        table.sparseUnionCertified=v.sparseUnionCertified;
        for(size_t n=0;n<v.descriptorOwnerCount;++n)table.descriptorOwners.push_back(unknown(v.descriptorOwners[n]));
        for(size_t n=0;n<v.leaseCount;++n)table.leases.push_back(unknown(v.leases[n]));
        pending.tables_.push_back(std::move(table));}
    output=std::move(pending);failure=Failure::None;return true;
}catch(const std::bad_alloc&){output={};failure=Failure::Capacity;return false;}
catch(...){output={};failure=Failure::Exception;return false;}
#else
bool BuildCore(const Input& input,const GuardedValidation* guarded,Plan& output,Failure& failure)try{
    output={};failure=Failure::Arguments;
    if(!ValidScope(input.scope) || !input.root || input.shaderUses.empty() || input.shaderUses.size()>4096 ||
        input.finite.size()>64 || input.sparse.size()>64 || input.privateCbvs.size()>128 || input.originalCbvs.size()>128 || input.privateSrvs.size()>1)return false;
    failure=Failure::Scope;if(input.selectors && input.selectorScope!=input.scope)return false;
    std::array<Layout,64> layouts;failure=Failure::Root;if(!Layouts(*input.root,layouts))return false;
    source_tables::Plan domains;if(!source_tables::Build(*input.root,input.shaderUses,domains))return false;
    using Key=std::pair<UINT,UINT>;
    std::map<Key,D3D12_DESCRIPTOR_RANGE_TYPE> required;
    std::map<Key,UINT> cbvBytes;
    std::set<UINT> dynamicParameters;
    std::set<const source_selectors::TableSelection*> consumedSelections;
    auto add=[&](UINT parameter,UINT offset,D3D12_DESCRIPTOR_RANGE_TYPE type,UINT bytes){
        const auto [at,inserted]=required.emplace(Key{parameter,offset},type);
        if(!inserted && at->second!=type){failure=Failure::Collision;return false;}
        if(required.size()>ResourceLimit+SamplerLimit){failure=Failure::Capacity;return false;}
        if(type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV){if(!bytes || bytes>65536){failure=Failure::CbvRange;return false;}
            auto& extent=cbvBytes[{parameter,offset}];extent=std::max(extent,bytes);if(cbvBytes.size()>128){failure=Failure::Capacity;return false;}}
        return true;
    };
    for(const auto& use:input.shaderUses){UINT parameter=UINT_MAX,offset{};const Range* matched{};
        for(UINT p=0;p<input.root->NumParameters;++p)for(const auto& range:layouts[p].ranges){
            const auto visibility=input.root->pParameters[p].ShaderVisibility;
            if(UINT(range.desc.RangeType)!=use.type || range.desc.RegisterSpace!=use.space ||
                (visibility!=D3D12_SHADER_VISIBILITY_ALL && UINT(visibility)!=use.visibility) || use.shaderRegister<range.desc.BaseShaderRegister)continue;
            const uint64_t relative=uint64_t(use.shaderRegister)-range.desc.BaseShaderRegister;
            if(relative>=range.count || use.count>uint64_t(range.count)-relative)continue;
            if(matched || uint64_t(range.offset)+relative>UINT_MAX){failure=Failure::Root;return false;}
            matched=&range;parameter=p;offset=range.offset+UINT(relative);
        }
        if(!matched){failure=Failure::MissingUse;return false;}
        if(matched->count==UINT_MAX && use.count>1){
            failure=Failure::DynamicCoverage;
            if((!input.dynamicCoverageCertified && !guarded) || !input.selectors || !input.selectors->selectorEvaluationComplete ||
                input.selectors->family!=input.family || input.family.rootSha256.empty())return false;
            bool covered{};
            for(const auto& table:input.selectors->tables){const auto& key=table.key;
                if(key.parameter!=parameter || key.visibility!=use.visibility || key.space!=use.space ||
                    key.shaderRegisterBase!=use.shaderRegister || key.originalTableOffset!=offset || key.declaredExtent!=use.count ||
                    key.rootSha256!=input.family.rootSha256)continue;
                if(covered || table.offsets.empty() || table.offsets.size()>descriptors::SourceSlotCaptureLimit ||
                    !std::is_sorted(table.offsets.begin(),table.offsets.end()) ||
                    std::adjacent_find(table.offsets.begin(),table.offsets.end())!=table.offsets.end() ||
                    table.count!=uint64_t(table.offsets.back())+1){failure=Failure::DynamicCoverage;return false;}
                std::vector<UINT> declared;
                for(const auto& interval:table.uses){if(interval.type!=use.type || !interval.count ||
                    uint64_t(interval.offset)+interval.count>uint64_t(offset)+use.count || interval.offset<offset ||
                    interval.count>descriptors::SourceSlotCaptureLimit-declared.size())return false;
                    for(UINT n=0;n<interval.count;++n)declared.push_back(interval.offset+n);}
                if(declared!=table.offsets)return false;
                for(const auto selected:table.offsets){if(selected<offset || uint64_t(selected)-offset>=use.count ||
                    !add(parameter,selected,D3D12_DESCRIPTOR_RANGE_TYPE(use.type),use.bytes))return false;}
                consumedSelections.insert(&table);
                covered=true;
            }
            if(!covered)return false;dynamicParameters.insert(parameter);
        }else{
            if(use.count>ResourceLimit || uint64_t(offset)+use.count>ResourceLimit){failure=Failure::Capacity;return false;}
            for(UINT n=0;n<use.count;++n)if(!add(parameter,offset+n,D3D12_DESCRIPTOR_RANGE_TYPE(use.type),use.bytes))return false;
        }
    }
    if(input.selectors){failure=Failure::DynamicCoverage;
        if(input.selectors->family!=input.family || input.selectors->tables.size()>64 ||
            input.selectors->evaluatedSites!=input.selectors->selections.size() ||
            input.selectors->selections.size()>descriptors::SourceSlotCaptureLimit)return false;
        for(const auto& table:input.selectors->tables)if(!dynamicParameters.contains(table.key.parameter) || !consumedSelections.contains(&table))return false;
        for(const auto& selection:input.selectors->selections)if(!std::any_of(input.selectors->tables.begin(),input.selectors->tables.end(),[&](const auto& table){
            return selection.shaderIndex<table.key.declaredExtent &&
                uint64_t(table.key.originalTableOffset)+selection.shaderIndex==selection.descriptorOffset &&
                std::binary_search(table.offsets.begin(),table.offsets.end(),selection.descriptorOffset);}))return false;
    }
    Plan pending;pending.scope_=input.scope;std::map<UINT,size_t> tableIndices;
    std::set<Key> supplied;UINT resourceCount{},samplerCount{},selectedCount{};
    auto newTable=[&](UINT parameter,UINT count)->Plan::Table*{
        if(parameter>=input.root->NumParameters){failure=Failure::Root;return nullptr;}
        if(tableIndices.contains(parameter)){failure=Failure::Collision;return nullptr;}
        const auto& layout=layouts[parameter];if(!Extent(layout,count)){failure=Failure::Extent;return nullptr;}
        auto& cursor=layout.sampler?samplerCount:resourceCount;const UINT limit=layout.sampler?SamplerLimit:ResourceLimit;
        if(count>limit-cursor){failure=Failure::Capacity;return nullptr;}cursor+=count;
        Plan::Table table;table.parameter=parameter;table.count=count;table.sourceRoot=input.scope.root;table.sourceScope=input.scope;
        table.type=layout.sampler?D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER:D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        table.slotTypes.assign(count,layout.sampler?D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER:D3D12_DESCRIPTOR_RANGE_TYPE_SRV);
        for(UINT n=0;n<count;++n)if(const auto* range=SlotRange(layout,n))table.slotTypes[n]=range->desc.RangeType;
        tableIndices.emplace(parameter,pending.tables_.size());pending.tables_.push_back(std::move(table));return &pending.tables_.back();
    };
    for(const auto& qualified:input.finite){failure=Failure::Scope;if(qualified.scope!=input.scope || !qualified.table)return false;
        const auto& owned=*qualified.table;failure=Failure::MissingTable;
        if(!std::any_of(domains.finite.begin(),domains.finite.end(),[&](const auto& table){return table.parameter==owned.parameter;}) ||
            !owned.owner || !owned.identity || !owned.version || !owned.sourceGpu || !owned.sourceHeap)return false;
        auto* table=newTable(owned.parameter,owned.count);if(!table)return false;failure=Failure::Type;
        if(!CpuHeap(owned.cpuHeap.Get(),table->type,owned.count))return false;
        table->copies.push_back({0,0,owned.count,owned.cpuHeap});table->leases={owned.owner,owned.cpuHeap};table->descriptorOwners=table->leases;
        for(const auto& [key,type]:required)if(key.first==owned.parameter)supplied.insert(key);
    }
    for(const auto& qualified:input.sparse){failure=Failure::Scope;if(qualified.scope!=input.scope || !qualified.table)return false;
        const auto& owned=*qualified.table;failure=Failure::MissingTable;
        if(owned.parameter>=input.root->NumParameters || !layouts[owned.parameter].tail || !owned.owner ||
            !owned.identity || !owned.version || !owned.sourceGpu || !owned.sourceHeap || owned.slots.empty() ||
            !std::any_of(required.begin(),required.end(),[&](const auto& use){return use.first.first==owned.parameter;}))return false;
        if(owned.slots.size()>descriptors::SourceSlotCaptureLimit-selectedCount){failure=Failure::Capacity;return false;}
        selectedCount+=UINT(owned.slots.size());auto* table=newTable(owned.parameter,owned.count);if(!table)return false;failure=Failure::Type;
        if(!CpuHeap(owned.cpuHeap.Get(),table->type,UINT(owned.slots.size())))return false;
        Microsoft::WRL::ComPtr<ID3D12Device> sourceDevice;failure=Failure::Device;
        if(FAILED(owned.cpuHeap->GetDevice(IID_PPV_ARGS(&sourceDevice))))return false;
        const UINT stride=sourceDevice->GetDescriptorHandleIncrementSize(table->type);if(!stride)return false;
        table->leases={owned.owner,owned.cpuHeap};table->descriptorOwners=table->leases;std::set<UINT> compact;
        table->sourceRoot=input.scope.root;table->sourceGpu=owned.sourceGpu;table->sourceHeap=owned.sourceHeap;
        table->sparseUnionCertified=dynamicParameters.contains(owned.parameter) && (input.dynamicCoverageCertified || guarded);
        for(const auto& slot:owned.slots){const Key key{owned.parameter,slot.offset};const auto use=required.find(key);
            failure=Failure::Type;const auto* range=SlotRange(layouts[owned.parameter],slot.offset);
            if(slot.offset>=owned.count || !range || range->desc.RangeType!=slot.type || use==required.end() || use->second!=slot.type ||
                slot.stamp.heap!=owned.sourceHeap || uint64_t(slot.offset)>(UINT64_MAX-owned.sourceGpu)/stride ||
                slot.stamp.gpu!=owned.sourceGpu+uint64_t(slot.offset)*stride || !Canonical(slot))return false;
            failure=Failure::Collision;if(!supplied.insert(key).second || !compact.insert(slot.compactIndex).second)return false;
            failure=Failure::Extent;if(slot.compactIndex>=owned.slots.size())return false;
            table->copies.push_back({slot.offset,slot.compactIndex,1,owned.cpuHeap});
            table->sparseSlots.push_back(slot);
            if(slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV){descriptor_views::SrvResult decoded;
                if(!descriptor_views::DecodeSrv(slot.stamp.value,decoded))return false;
                table->srvCoverage.push_back({input.scope,slot.offset,slot.compactIndex,slot.stamp,decoded.resourceCookie,
                    owned.identity,owned.version,owned.sourceGpu,owned.sourceHeap,owned.count,owned.owner,owned.cpuHeap});}
        }
    }
    failure=Failure::MissingTable;for(const auto& table:domains.finite)if(!tableIndices.contains(table.parameter))return false;
    failure=Failure::MissingUse;for(const auto& [key,type]:required)if(type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV && !supplied.contains(key))return false;
    std::set<Key> replaced;bool finalized=true;
    for(const auto& qualified:input.privateCbvs){failure=Failure::Scope;
        if(qualified.scope!=input.scope || !qualified.value || !qualified.copies)return false;
        const auto& value=*qualified.value;const auto& copies=*qualified.copies;const Key key{value.parameter,value.offset};
        if(copies.list!=input.scope.list || copies.generation!=input.scope.generation || copies.occurrence!=input.scope.occurrence)return false;
        failure=Failure::CbvOwner;if(!value.owner || !value.resource || !copies.owner || !copies.count ||
            copies.count>source_cbv_snapshot::detail::MaxRequests || value.copiedBytes!=value.reflectedBytes || copies.copiedBytes<value.copiedBytes)return false;
        failure=Failure::CbvRange;const auto requiredBytes=cbvBytes.find(key);
        if(requiredBytes==cbvBytes.end() || value.reflectedBytes<requiredBytes->second || value.paddedBytes!=value.view.SizeInBytes ||
            !CbvRange(value.resource.Get(),value.view,value.reflectedBytes,value.privateOffset,true))return false;
        failure=Failure::Collision;if(!replaced.insert(key).second)return false;
        auto at=tableIndices.find(key.first);failure=Failure::MissingTable;if(at==tableIndices.end())return false;
        auto& table=pending.tables_[at->second];failure=Failure::Extent;if(key.second>=table.count)return false;
        table.cbvs.push_back({key.second,value.view});table.leases.push_back(value.resource);table.leases.push_back(value.owner);table.leases.push_back(copies.owner);
        table.descriptorOwners.push_back(value.resource);table.descriptorOwners.push_back(value.owner);
        finalized&=qualified.finalized;
    }
    for(const auto& value:input.originalCbvs){failure=Failure::Scope;if(value.scope!=input.scope)return false;
        const Key key{value.parameter,value.offset};failure=Failure::CbvOwner;
        if(!value.owner || !value.resource || !value.readEpochCertified)return false;
        failure=Failure::CbvRange;const auto requiredBytes=cbvBytes.find(key);const auto gpu=value.resource->GetGPUVirtualAddress();
        if(requiredBytes==cbvBytes.end() || value.reflectedBytes<requiredBytes->second || value.view.BufferLocation<gpu ||
            !CbvRange(value.resource.Get(),value.view,value.reflectedBytes,value.view.BufferLocation-gpu,false))return false;
        failure=Failure::Collision;if(!replaced.insert(key).second)return false;
        const auto at=tableIndices.find(key.first);failure=Failure::MissingTable;if(at==tableIndices.end())return false;
        auto& table=pending.tables_[at->second];failure=Failure::Extent;if(key.second>=table.count)return false;
        table.cbvs.push_back({key.second,value.view});table.leases.push_back(value.resource);table.leases.push_back(value.owner);
        table.descriptorOwners.push_back(value.resource);table.descriptorOwners.push_back(value.owner);
    }
    for(const auto& qualified:input.privateSrvs){failure=Failure::Scope;
        if(qualified.scope!=input.scope || !qualified.value)return false;
        const auto& value=*qualified.value;
        const bool conditional=guarded && guarded->t5==qualified.value;
        const structured_snapshot_view::Scope expected{input.scope.root,input.scope.list,input.scope.generation,input.scope.occurrence,input.scope.eye};
        if(value.scope!=expected)return false;
        failure=Failure::SrvOwner;
        if(!qualified.originalSlotCertified || !qualified.originalOwner || !qualified.original.resource ||
            !qualified.resourceCookie || qualified.original.description!=qualified.original.stamp.value ||
            qualified.original.stamp.value[2]!=qualified.resourceCookie || (!conditional && !structured_snapshot_view::ValidatePrivateView(value)))return false;
        const auto b4=std::find_if(input.privateCbvs.begin(),input.privateCbvs.end(),[](const auto& candidate){
            return candidate.value && candidate.value->parameter==19 && !candidate.value->offset;});
        if(b4==input.privateCbvs.end() || b4->scope!=input.scope || !b4->finalized || b4->value!=qualified.privateB4 ||
            b4->copies!=qualified.privateB4Copies || (!qualified.privateB4BytesCertified && !conditional) ||
            qualified.privateB4Bytes.size()!=multilayer_selectors::B4Bytes || !qualified.privateB4Bytes.data() ||
            !std::equal(qualified.privateB4Bytes.begin(),qualified.privateB4Bytes.end(),value.privateB4Bytes.begin()) ||
            !qualified.privateB4 || !qualified.privateB4Copies || qualified.privateB4->reflectedBytes!=multilayer_selectors::B4Bytes ||
            qualified.privateB4->copiedBytes!=multilayer_selectors::B4Bytes)return false;
        failure=Failure::DynamicCoverage;
        if(input.family.rootSha256!=multilayer_selectors::RootSha256 || input.family.vertexSha256!=multilayer_selectors::VertexSha256 ||
            input.family.pixelSha256!=multilayer_selectors::PixelSha256)return false;
        failure=Failure::Type;const Key key{qualified.parameter,qualified.offset};const auto use=required.find(key);
        if(key!=Key{23,5} || use==required.end() || use->second!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV ||
            input.root->pParameters[23].ShaderVisibility!=D3D12_SHADER_VISIBILITY_PIXEL || layouts[23].tail)return false;
        const auto* range=SlotRange(layouts[23],5);
        if(!range || range->desc.RangeType!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV || range->desc.RegisterSpace ||
            uint64_t(range->desc.BaseShaderRegister)+5-range->offset!=5)return false;
        const auto finite=std::find_if(input.finite.begin(),input.finite.end(),[](const auto& table){return table.table && table.table->parameter==23;});
        failure=Failure::MissingTable;if(finite==input.finite.end())return false;const auto& original=*finite->table;
        failure=Failure::SrvOwner;
        if(original.owner!=qualified.originalOwner || original.identity!=qualified.originalIdentity || original.version!=qualified.originalVersion ||
            original.count!=qualified.originalCount || original.sourceGpu!=qualified.originalGpu || original.sourceHeap!=qualified.originalHeap ||
            qualified.original.stamp.heap!=original.sourceHeap)return false;
        Microsoft::WRL::ComPtr<ID3D12Device> device;failure=Failure::Device;
        if(FAILED(original.cpuHeap->GetDevice(IID_PPV_ARGS(&device))) || !SameDevice(device.Get(),qualified.original.resource.Get()) ||
            !SameDevice(device.Get(),value.resource.Get()))return false;
        const UINT stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
        failure=Failure::SrvRange;
        if(!stride || uint64_t(5)>(UINT64_MAX-original.sourceGpu)/stride || qualified.original.stamp.gpu!=original.sourceGpu+uint64_t(5)*stride)return false;
        descriptor_views::SrvResult decoded;
        if(!descriptor_views::DecodeSrv(qualified.original.stamp.value,decoded) || decoded.implicit || decoded.resourceCookie!=qualified.resourceCookie ||
            !ExactStructured(decoded.view) || !SameSrv(decoded.view,qualified.original.view))return false;
        const auto desc=qualified.original.resource->GetDesc();
        if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Width!=structured_snapshot_view::DomainBytes)return false;
        const auto& metadata=decoded.metadata;
        if(metadata.described && (metadata.dimension!=desc.Dimension || metadata.format!=desc.Format || metadata.width!=desc.Width ||
            metadata.height!=desc.Height || metadata.depthOrArraySize!=desc.DepthOrArraySize || metadata.mipLevels!=desc.MipLevels ||
            metadata.flags!=desc.Flags || metadata.layout!=desc.Layout || metadata.samples.Count!=desc.SampleDesc.Count ||
            metadata.samples.Quality!=desc.SampleDesc.Quality))return false;
        multilayer_selectors::StructuredView view;view.key={std::string(multilayer_selectors::RootSha256),5,5,0,23,5};
        view.resourceWidth=desc.Width;view.numElements=8192;view.structureByteStride=128;view.dimension=1;view.componentMapping=5768;view.explicitDescription=true;
        multilayer_selectors::ReadPlan checked;std::string error;
        failure=Failure::DynamicCoverage;
        if(!multilayer_selectors::PlanRead(input.family,{{std::string(multilayer_selectors::RootSha256),5,4,0,19,0},value.privateB4Bytes},view,checked,error) ||
            !checked.originalSiteCoverageComplete || !checked.conservativeRecordSuperset || checked.records.empty() ||
            checked.records.size()>multilayer_selectors::MaxRecords || value.recordCount!=checked.records.size() ||
            value.firstRecord!=checked.firstRecord || value.recordSpan!=checked.recordSpan || value.sourceOffset!=checked.resourceOffset ||
            value.copiedBytes!=checked.byteExtent)return false;
        std::array<uint32_t,multilayer_selectors::MaxRecords> records{};std::copy(checked.records.begin(),checked.records.end(),records.begin());
        if(value.records!=records)return false;
        failure=Failure::Collision;if(!replaced.insert(key).second)return false;
        const auto at=tableIndices.find(23);failure=Failure::MissingTable;if(at==tableIndices.end())return false;
        auto& table=pending.tables_[at->second];failure=Failure::Extent;if(5>=table.count)return false;
        table.srvs.push_back({5,value.srv,value.resource});
        table.leases.push_back(qualified.original.resource);table.leases.push_back(qualified.originalOwner);
        table.leases.push_back(value.resource);table.leases.push_back(value.owner);
        if(conditional)table.leases.push_back(guarded->receipt);
        table.leases.push_back(qualified.privateB4->resource);table.leases.push_back(qualified.privateB4->owner);table.leases.push_back(qualified.privateB4Copies->owner);
        // The genuine immutable overlay owner retains its dense snapshot owners
        // and source copy receipts, not a borrowed original execution epoch.
        table.descriptorOwners.push_back(value.resource);table.descriptorOwners.push_back(value.owner);
    }
    for(const auto& [key,bytes]:cbvBytes)if(!replaced.contains(key))pending.missing_.push_back({key.first,key.second,bytes});
    std::sort(pending.tables_.begin(),pending.tables_.end(),[](const auto& a,const auto& b){return a.parameter<b.parameter;});
    pending.descriptors_=resourceCount+samplerCount;pending.ready_=pending.missing_.empty() && finalized;
    output=std::move(pending);failure=Failure::None;return true;
}catch(const std::bad_alloc&){output={};failure=Failure::Capacity;return false;}
catch(...){output={};failure=Failure::Exception;return false;}

#endif
bool Build(const Input& input,Plan& output,Failure& failure){return BuildCore(input,nullptr,output,failure);}

bool BuildGuarded(const GuardedInput& supplied,GuardedPlan& output,Failure& failure)try{
    output={};failure=Failure::Arguments;auto input=supplied.source;
    if(!supplied.recipe || supplied.recipe->key!=input.family || input.dynamicCoverageCertified || input.selectors ||
        !input.privateSrvs.empty() || !input.originalCbvs.empty() || supplied.cbvs.empty() || supplied.cbvs.size()>2 ||
        !ValidScope(input.scope) || !input.root || input.selectorScope!=input.scope)return false;
    GuardedPlan pending;std::vector<source_selectors::OwnedCbvBytes> bytes;
    std::set<source_selectors::CbvKey> keys;
    for(const auto& predicted:supplied.cbvs){failure=Failure::CbvOwner;
        const auto found=std::find_if(input.privateCbvs.begin(),input.privateCbvs.end(),[&](const auto& cbv){
            return cbv.value && cbv.value->parameter==predicted.key.parameter && cbv.value->offset==predicted.key.tableOffset;});
        if(predicted.key.rootSha256!=input.family.rootSha256 || !keys.insert(predicted.key).second || found==input.privateCbvs.end() ||
            found->scope!=input.scope || !found->finalized || !found->copies || !found->copies->owner || !found->value->owner || !found->value->resource ||
            predicted.expected.empty() || predicted.expected.size()!=found->value->copiedBytes ||
            found->value->reflectedBytes!=predicted.expected.size())return false;
        failure=Failure::DynamicCoverage;
        if(!std::any_of(input.shaderUses.begin(),input.shaderUses.end(),[&](const auto& use){return use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV &&
            use.visibility==predicted.key.visibility && use.space==predicted.key.space && use.shaderRegister==predicted.key.shaderRegister &&
            use.count==1 && use.bytes==predicted.expected.size();}))return false;
        bytes.push_back({predicted.key,predicted.expected});
        const auto& value=*found->value;ShaderInputCheck check{value.resource,value.owner,value.privateOffset,{}};
        check.expected.assign(predicted.expected.begin(),predicted.expected.end());pending.checks_.push_back(std::move(check));
    }
    source_selectors::Result evaluated;std::string error;GuardedValidation guard;
    structured_snapshot_view::PrivateView conditionalView;PrivateSrv replacement;
    failure=Failure::DynamicCoverage;
    if(supplied.recipe->supported){
        if(supplied.t5 || !source_selectors::Evaluate(*supplied.recipe,bytes,evaluated,error))return false;
    }else{
        if(!supplied.t5 || bytes.size()!=1)return false;const auto& t5=*supplied.t5;const auto& original=t5.original;
        if(original.scope!=input.scope || original.parameter!=23 || original.offset!=5 || original.value || original.privateB4 || original.privateB4Copies ||
            original.privateB4BytesCertified || !original.privateB4Bytes.empty() || !original.originalSlotCertified ||
            !original.original.resource || !ExactStructured(original.original.view))return false;
        multilayer_selectors::StructuredView view;view.key={input.family.rootSha256,5,5,0,23,5};
        view.resourceWidth=original.original.resource->GetDesc().Width;view.numElements=8192;view.structureByteStride=128;
        view.dimension=1;view.componentMapping=5768;view.explicitDescription=true;
        multilayer_selectors::ReadPlan read;
        if(!multilayer_selectors::PlanRead(input.family,bytes[0],view,read,error) || !read.originalSiteCoverageComplete || !read.conservativeRecordSuperset ||
            !multilayer_selectors::Enumerate(read,{view,read.resourceOffset,t5.expected},evaluated,error))return false;
        if(read.byteExtent){const auto& copy=t5.copy;const auto& receipt=t5.receipt;failure=Failure::SrvOwner;
            if(!t5.recordedViewCertified || !copy.owner || !copy.resource || !receipt.owner || copy.resource==original.original.resource ||
                receipt.list!=input.scope.list || receipt.generation!=input.scope.generation || receipt.occurrence!=input.scope.occurrence ||
                receipt.sourceOffset!=copy.sourceOffset || receipt.privateOffset!=copy.privateOffset || receipt.copiedBytes!=copy.copiedBytes ||
                copy.sourceOffset!=read.resourceOffset || copy.copiedBytes!=read.byteExtent || t5.expected.size()!=copy.copiedBytes ||
                copy.privateOffset<copy.sourceOffset || copy.privateOffset%128)return false;
            const auto desc=copy.resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
            const auto start=copy.privateOffset-copy.sourceOffset;failure=Failure::SrvRange;
            if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || desc.Flags || desc.Width>structured_snapshot_view::MaxBytes ||
                start>desc.Width || structured_snapshot_view::DomainBytes>desc.Width-start ||
                FAILED(copy.resource->GetHeapProperties(&heap,&flags)) || heap.Type!=D3D12_HEAP_TYPE_DEFAULT)return false;
            auto expected=original.original.view;expected.Buffer.FirstElement=start/128;
            if(!SameSrv(expected,t5.view))return false;
            const auto b4=std::find_if(input.privateCbvs.begin(),input.privateCbvs.end(),[](const auto& cbv){return cbv.value && cbv.value->parameter==19 && !cbv.value->offset;});
            if(b4==input.privateCbvs.end())return false;
            conditionalView.scope={input.scope.root,input.scope.list,input.scope.generation,input.scope.occurrence,input.scope.eye};
            conditionalView.resource=copy.resource;conditionalView.owner=copy.owner;conditionalView.srv=t5.view;
            conditionalView.sourceOffset=copy.sourceOffset;conditionalView.denseOffset=conditionalView.targetOffset=copy.privateOffset;
            conditionalView.copiedBytes=copy.copiedBytes;conditionalView.copyList=receipt.list;conditionalView.copyGeneration=receipt.generation;
            conditionalView.firstRecord=read.firstRecord;conditionalView.recordSpan=read.recordSpan;conditionalView.recordCount=UINT(read.records.size());
            std::copy(read.records.begin(),read.records.end(),conditionalView.records.begin());conditionalView.privateB4Bytes=read.b4Bytes;
            replacement=original;replacement.value=&conditionalView;replacement.privateB4=b4->value;replacement.privateB4Copies=b4->copies;
            replacement.privateB4Bytes=bytes[0].bytes;input.privateSrvs={&replacement,1};guard.t5=&conditionalView;guard.receipt=receipt.owner;
            ShaderInputCheck check{copy.resource,copy.owner,copy.privateOffset,{}};
            check.expected.assign(t5.expected.begin(),t5.expected.end());pending.checks_.push_back(std::move(check));
            pending.t5_=GuardedT5View{copy.resource,copy.owner,t5.view,copy.privateOffset,copy.copiedBytes};
        }else if(!t5.expected.empty() || t5.copy.resource || t5.copy.owner || t5.receipt.owner)return false;
    }
    input.selectors=&evaluated;
    if(!BuildCore(input,&guard,pending.plan_,failure))return false;
    failure=Failure::NotReady;if(!pending.plan_.Ready())return false;
    output=std::move(pending);failure=Failure::None;return true;
}catch(const std::bad_alloc&){output={};failure=Failure::Capacity;return false;}
catch(...){output={};failure=Failure::Exception;return false;}

HRESULT AssembleGuardedPass(ID3D12Device* device,std::span<const GuardedPlan* const> plans,CpuTableCache& cache,
    std::vector<GuardedTables>& output,Failure& failure)try{
    failure=Failure::Arguments;if(!output.empty() || plans.empty() || plans.size()>2048)return E_INVALIDARG;
    std::vector<const Plan*> layouts;std::vector<GuardedTables> result;layouts.reserve(plans.size());result.reserve(plans.size());
    for(const auto* plan:plans){failure=Failure::NotReady;if(!plan || !plan->Prepared() || plan->checks_.empty())return E_INVALIDARG;
        layouts.push_back(&plan->plan_);result.push_back({plan->plan_.Source(),{},plan->checks_,plan->t5_});}
    std::vector<std::vector<SharedBankTable>> tables;const auto hr=AssemblePass(device,layouts,cache,tables,failure);if(FAILED(hr))return hr;
    for(size_t i=0;i<result.size();++i)result[i].tables=std::move(tables[i]);output.swap(result);return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}

#if CVR_RUST_VRIK
// The tables are assembled in Rust (rust/crates/cvr-render/src/assembly_pass.rs): the heaps, the cache and a pass's unions;
// each assembled table comes back here (IssueTable) for its SharedBankTable and SRV coverage receipt.
extern "C" void* cvr_sba_cache_new();
extern "C" void cvr_sba_cache_free(void*);
extern "C" void cvr_sba_cache_counts(const void*,uint64_t*);
extern "C" void cvr_sba_table_view(const void*,void*);
using RustIssue=bool(*)(void*,const void*,void*,uint64_t,UINT,bool);
extern "C" HRESULT cvr_sba_assemble(ID3D12Device*,const void*,void*,UINT,RustIssue,void*,UINT*);
extern "C" HRESULT cvr_sba_assemble_pass(ID3D12Device*,const void* const*,size_t,const void*,UINT,RustIssue,void*,void**,UINT*);
extern "C" bool cvr_sba_pass_reference(const void*,size_t,size_t,const void**);
extern "C" size_t cvr_sba_pass_index(const void*,size_t,size_t);
extern "C" void cvr_sba_pass_commit(void*,void*);
extern "C" void cvr_sba_pass_free(void*);
extern "C" void* cvr_sba_plan_lend(const Scope*,bool,UINT,const void*,size_t);
namespace {
// A C++ plan's tables lent to Rust as they are (the plan's copies, views, slots and receipts share the Rust rows' layout:
// a ComPtr is its pointer); the Rust plan copies them and is freed with the handle.
using RustPlan=std::unique_ptr<void,void(*)(void*)>;
template<class Tables> RustPlan LendPlan(const Scope& scope,bool ready,UINT descriptors,const Tables& tables){
    using Table=typename Tables::value_type;
    static_assert(sizeof(typename decltype(Table::copies)::value_type)==sizeof(RustCopy) &&
        sizeof(typename decltype(Table::cbvs)::value_type)==sizeof(RustCbvRow) && sizeof(typename decltype(Table::srvs)::value_type)==sizeof(RustSrvRow) &&
        sizeof(OwnedSrvSlot)==sizeof(RustCoverage) && sizeof(Microsoft::WRL::ComPtr<IUnknown>)==sizeof(uint64_t),"lent as the Rust rows");
    std::vector<RustTableView> views;views.reserve(tables.size());
    for(const auto& table:tables)views.push_back({table.parameter,table.count,table.sourceScope,int32_t(table.type),
        reinterpret_cast<const int32_t*>(table.slotTypes.data()),table.slotTypes.size(),reinterpret_cast<const RustCopy*>(table.copies.data()),
        table.copies.size(),reinterpret_cast<const RustCbvRow*>(table.cbvs.data()),table.cbvs.size(),reinterpret_cast<const RustSrvRow*>(table.srvs.data()),
        table.srvs.size(),table.sourceRoot,table.sourceGpu,table.sourceHeap,table.sparseSlots.data(),table.sparseSlots.size(),
        reinterpret_cast<const RustCoverage*>(table.srvCoverage.data()),table.srvCoverage.size(),table.sparseUnionCertified,table.unionStorage,
        reinterpret_cast<const uint64_t*>(table.descriptorOwners.data()),table.descriptorOwners.size(),reinterpret_cast<const uint64_t*>(table.leases.data()),
        table.leases.size()});
    return RustPlan(cvr_sba_plan_lend(&scope,ready,descriptors,views.data(),views.size()),&cvr_sba_plan_free);
}
bool IssueTable(void* user,const void* rust,void* heap,uint64_t scope,UINT extent,bool keep){
    RustTableView v{};cvr_sba_table_view(rust,&v);
    SharedBankTable result;result.parameter=v.parameter;result.count=v.count;result.cpuHeap=static_cast<ID3D12DescriptorHeap*>(heap);
    result.storageScope=scope;result.storageExtent=extent;
    for(size_t n=0;n<v.leaseCount;++n)result.leases.emplace_back(reinterpret_cast<IUnknown*>(v.leases[n]));
    if(!v.cbvCount && !v.srvCount){std::vector<OwnedSrvSlot> coverage;coverage.reserve(v.coverageCount);
        for(size_t n=0;n<v.coverageCount;++n){const auto& c=v.coverage[n];
            coverage.push_back({c.contributor,c.offset,c.compactIndex,c.stamp,c.resourceCookie,c.originalIdentity,c.originalVersion,c.originalGpu,
                c.originalHeap,c.originalCount,reinterpret_cast<IUnknown*>(c.originalOwner),reinterpret_cast<ID3D12DescriptorHeap*>(c.originalCpuHeap)});}
        if(!IssueCoverage(result,v.sourceScope,coverage,{v.sparseSlots,v.sparseSlotCount}))return false;}
    if(keep)static_cast<std::vector<SharedBankTable>*>(user)->push_back(std::move(result));
    return true;
}
}
HRESULT Assemble(ID3D12Device* device,const Plan& plan,std::vector<SharedBankTable>& output,Failure& failure)try{
    failure=Failure::Arguments;if(!device || !output.empty())return E_INVALIDARG;
    std::vector<SharedBankTable> pending;pending.reserve(plan.tables_.size());probe::Scope internal;UINT reason{};
    const auto lent=LendPlan(plan.scope_,plan.ready_,plan.descriptors_,plan.tables_);
    const auto hr=cvr_sba_assemble(device,lent.get(),nullptr,0,&IssueTable,&pending,&reason);failure=Failure(reason);if(FAILED(hr))return hr;
    output.swap(pending);return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}
struct CpuTableCache::Storage {
    void* rust=cvr_sba_cache_new();
    Storage()=default;~Storage(){cvr_sba_cache_free(rust);}
    Storage(const Storage&)=delete;Storage& operator=(const Storage&)=delete;
};
CpuTableCache::CpuTableCache()=default;
CpuTableCache::~CpuTableCache()=default;
UINT CpuTableCache::UniqueDescriptorCount()const noexcept{uint64_t counts[4]{};if(storage_)cvr_sba_cache_counts(storage_->rust,counts);return UINT(counts[0]);}
UINT CpuTableCache::TableCount()const noexcept{uint64_t counts[4]{};if(storage_)cvr_sba_cache_counts(storage_->rust,counts);return UINT(counts[1]);}
uint64_t CpuTableCache::HitCount()const noexcept{uint64_t counts[4]{};if(storage_)cvr_sba_cache_counts(storage_->rust,counts);return counts[2];}
uint64_t CpuTableCache::MissCount()const noexcept{uint64_t counts[4]{};if(storage_)cvr_sba_cache_counts(storage_->rust,counts);return counts[3];}
HRESULT Assemble(ID3D12Device* device,const Plan& plan,CpuTableCache& cache,std::vector<SharedBankTable>& output,Failure& failure,UINT limit)try{
    failure=Failure::Arguments;if(!device || !output.empty())return E_INVALIDARG;
    if(!cache.storage_)cache.storage_=std::make_unique<CpuTableCache::Storage>();
    std::vector<SharedBankTable> pending;probe::Scope internal;UINT reason{};
    const auto lent=LendPlan(plan.scope_,plan.ready_,plan.descriptors_,plan.tables_);
    const auto hr=cvr_sba_assemble(device,lent.get(),cache.storage_->rust,limit,&IssueTable,&pending,&reason);failure=Failure(reason);
    if(FAILED(hr))return hr;
    output.swap(pending);return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}
HRESULT AssemblePass(ID3D12Device* device,std::span<const Plan* const> plans,CpuTableCache& cache,
    std::vector<std::vector<SharedBankTable>>& output,Failure& failure,UINT limit)try{
    failure=Failure::Arguments;if(!device || !output.empty())return E_INVALIDARG;
    if(!cache.storage_)cache.storage_=std::make_unique<CpuTableCache::Storage>();
    std::vector<RustPlan> lent;std::vector<const void*> rust;lent.reserve(plans.size());rust.reserve(plans.size());
    for(const auto* plan:plans){if(plan)lent.push_back(LendPlan(plan->scope_,plan->ready_,plan->descriptors_,plan->tables_));
        rust.push_back(plan?lent.back().get():nullptr);}
    std::vector<SharedBankTable> assembled;void* pass{};UINT reason{};HRESULT hr{};
    {probe::Scope internal;hr=cvr_sba_assemble_pass(device,rust.data(),rust.size(),cache.storage_->rust,limit,&IssueTable,&assembled,&pass,&reason);}
    failure=Failure(reason);if(FAILED(hr))return hr;
    const std::unique_ptr<void,void(*)(void*)> held(pass,&cvr_sba_pass_free);
    std::vector<std::vector<SharedBankTable>> result;result.reserve(plans.size());
    for(size_t p=0;p<plans.size();++p){const auto& plan=*plans[p];auto& current=result.emplace_back();current.reserve(plan.tables_.size());
        for(size_t t=0;t<plan.tables_.size();++t){const auto& original=plan.tables_[t];const void* combined{};
            const bool united=cvr_sba_pass_reference(pass,p,t,&combined);const auto index=cvr_sba_pass_index(pass,p,t);
            SharedBankTable value;value.parameter=original.parameter;value.count=original.count;value.cpuHeap=assembled[index].cpuHeap;
            value.storageScope=assembled[index].storageScope;value.storageExtent=assembled[index].storageExtent;value.leases=original.leases;
            if(united){RustTableView v{};cvr_sba_table_view(combined,&v);
                for(size_t n=0;n<v.descriptorOwnerCount;++n){auto* owner=reinterpret_cast<IUnknown*>(v.descriptorOwners[n]);
                    if(std::none_of(value.leases.begin(),value.leases.end(),[&](const auto& lease){return lease.Get()==owner;}))value.leases.emplace_back(owner);}}
            failure=Failure::SrvOwner;
            if(!ReissueCoverage(value,plan.scope_,assembled[index]))return E_INVALIDARG;
            current.push_back(std::move(value));}
    }
    cvr_sba_pass_commit(pass,cache.storage_->rust);output.swap(result);failure=Failure::None;return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}
#else
HRESULT Assemble(ID3D12Device* device,const Plan& plan,std::vector<SharedBankTable>& output,Failure& failure)try{
    failure=Failure::Arguments;if(!device || !output.empty())return E_INVALIDARG;
    failure=Failure::NotReady;if(!plan.Ready() || !ValidScope(plan.scope_) || plan.tables_.empty())return E_INVALIDARG;
    std::vector<SharedBankTable> pending;pending.reserve(plan.tables_.size());probe::Scope internal;
    for(const auto& table:plan.tables_){const UINT stride=device->GetDescriptorHandleIncrementSize(table.type);
        failure=Failure::Device;if(!stride)return E_INVALIDARG;
        for(const auto& copy:table.copies)if(!SameDevice(device,copy.heap.Get()))return E_INVALIDARG;
        // Keep resource leases also checked for device identity without requiring
        // allocation/copy-owner IUnknown objects to expose a D3D interface.
        for(const auto& lease:table.leases){Microsoft::WRL::ComPtr<ID3D12Resource> resource;
            if(SUCCEEDED(lease.As(&resource)) && !SameDevice(device,resource.Get()))return E_INVALIDARG;}
        SharedBankTable result;result.parameter=table.parameter;result.count=table.count;result.leases=table.leases;
        const D3D12_DESCRIPTOR_HEAP_DESC desc{table.type,table.count,D3D12_DESCRIPTOR_HEAP_FLAG_NONE,0};
        auto hr=device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&result.cpuHeap));if(FAILED(hr))return hr;
        const auto base=result.cpuHeap->GetCPUDescriptorHandleForHeapStart();failure=Failure::Extent;
        if(!base.ptr || uint64_t(table.count)-1>(SIZE_MAX-base.ptr)/stride)return E_INVALIDARG;
        for(UINT n=0;n<table.count;++n)NullSlot(device,table.slotTypes[n],{base.ptr+SIZE_T(n)*stride});
        for(const auto& copy:table.copies){const auto source=copy.heap->GetCPUDescriptorHandleForHeapStart();
            if(!source.ptr || uint64_t(copy.sourceOffset)+copy.count-1>(SIZE_MAX-source.ptr)/stride)return E_INVALIDARG;
            device->CopyDescriptorsSimple(copy.count,{base.ptr+SIZE_T(copy.offset)*stride},
                {source.ptr+SIZE_T(copy.sourceOffset)*stride},table.type);}
        for(const auto& cbv:table.cbvs)device->CreateConstantBufferView(&cbv.view,{base.ptr+SIZE_T(cbv.offset)*stride});
        for(const auto& srv:table.srvs)device->CreateShaderResourceView(srv.resource.Get(),&srv.view,{base.ptr+SIZE_T(srv.offset)*stride});
        result.storageScope=reinterpret_cast<uintptr_t>(result.cpuHeap.Get());result.storageExtent=table.count;
        failure=Failure::SrvOwner;
        if(table.cbvs.empty() && table.srvs.empty() && !IssueCoverage(result,table.sourceScope,table.srvCoverage,table.sparseSlots))return E_INVALIDARG;
        // A new assembly is a new version. Zero labels deliberately disable
        // cross-occurrence deduplication in CreateSourcePass.
        pending.push_back(std::move(result));
    }
    output.swap(pending);failure=Failure::None;return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}

struct CpuTableCache::Storage {
    struct Entry {Plan::Table key;Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> heap;uint64_t hash{};};
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    std::vector<std::shared_ptr<const Entry>> entries;
    std::map<uint64_t,std::vector<size_t>> buckets;
    UINT descriptors{};uint64_t hits{},misses{};
    static uint64_t Hash(const Plan::Table& table)noexcept {
        uint64_t hash=14695981039346656037ull;
        const auto word=[&](uint64_t value){for(UINT n=0;n<8;++n){hash=(hash^uint8_t(value))*1099511628211ull;value>>=8;}};
        word(table.sourceRoot);word(table.parameter);word(table.count);word(UINT(table.type));word(table.slotTypes.size());
        for(const auto type:table.slotTypes)word(UINT(type));
        word(table.copies.size());for(const auto& copy:table.copies){word(copy.offset);word(copy.sourceOffset);word(copy.count);word(reinterpret_cast<uintptr_t>(copy.heap.Get()));}
        word(table.cbvs.size());for(const auto& cbv:table.cbvs){word(cbv.offset);word(cbv.view.BufferLocation);word(cbv.view.SizeInBytes);}
        word(table.srvs.size());for(const auto& srv:table.srvs){word(srv.offset);word(reinterpret_cast<uintptr_t>(srv.resource.Get()));
            word(srv.view.Format);word(srv.view.ViewDimension);word(srv.view.Shader4ComponentMapping);word(srv.view.Buffer.FirstElement);
            word(srv.view.Buffer.NumElements);word(srv.view.Buffer.StructureByteStride);word(srv.view.Buffer.Flags);}
        word(table.descriptorOwners.size());for(const auto& owner:table.descriptorOwners)word(reinterpret_cast<uintptr_t>(owner.Get()));
        word(table.unionStorage);
        if(table.unionStorage){word(table.sourceRoot);word(table.sourceGpu);word(table.sourceHeap);word(table.sparseSlots.size());
            for(const auto& slot:table.sparseSlots){word(slot.offset);word(UINT(slot.type));word(slot.stamp.gpu);word(slot.stamp.heap);
                for(const auto value:slot.stamp.value)word(value);}}
        return hash;
    }
    static bool Same(const Plan::Table& a,const Plan::Table& b)noexcept {
        if(a.sourceRoot!=b.sourceRoot || a.parameter!=b.parameter || a.count!=b.count || a.type!=b.type || a.slotTypes!=b.slotTypes ||
            a.copies.size()!=b.copies.size() || a.cbvs.size()!=b.cbvs.size() || a.srvs.size()!=b.srvs.size() || a.descriptorOwners.size()!=b.descriptorOwners.size())return false;
        if(a.unionStorage!=b.unionStorage)return false;
        if(a.unionStorage){if(a.sourceRoot!=b.sourceRoot || a.sourceGpu!=b.sourceGpu || a.sourceHeap!=b.sourceHeap || a.sparseSlots.size()!=b.sparseSlots.size())return false;
            for(size_t n=0;n<a.sparseSlots.size();++n){const auto& x=a.sparseSlots[n];const auto& y=b.sparseSlots[n];
                if(x.offset!=y.offset || x.type!=y.type || x.stamp.gpu!=y.stamp.gpu || x.stamp.heap!=y.stamp.heap || x.stamp.value!=y.stamp.value)return false;}}
        for(size_t n=0;n<a.copies.size();++n){const auto& x=a.copies[n];const auto& y=b.copies[n];
            if(x.offset!=y.offset || x.sourceOffset!=y.sourceOffset || x.count!=y.count || x.heap!=y.heap)return false;}
        for(size_t n=0;n<a.cbvs.size();++n){const auto& x=a.cbvs[n];const auto& y=b.cbvs[n];
            if(x.offset!=y.offset || x.view.BufferLocation!=y.view.BufferLocation || x.view.SizeInBytes!=y.view.SizeInBytes)return false;}
        for(size_t n=0;n<a.srvs.size();++n){const auto& x=a.srvs[n];const auto& y=b.srvs[n];
            if(x.offset!=y.offset || x.resource!=y.resource || !SameSrv(x.view,y.view))return false;}
        for(size_t n=0;n<a.descriptorOwners.size();++n)if(a.descriptorOwners[n]!=b.descriptorOwners[n])return false;
        return true;
    }
};
CpuTableCache::CpuTableCache()=default;
CpuTableCache::~CpuTableCache()=default;
UINT CpuTableCache::UniqueDescriptorCount()const noexcept{return storage_?storage_->descriptors:0;}
UINT CpuTableCache::TableCount()const noexcept{return storage_?UINT(storage_->entries.size()):0;}
uint64_t CpuTableCache::HitCount()const noexcept{return storage_?storage_->hits:0;}
uint64_t CpuTableCache::MissCount()const noexcept{return storage_?storage_->misses:0;}
HRESULT Assemble(ID3D12Device* device,const Plan& plan,CpuTableCache& cache,std::vector<SharedBankTable>& output,Failure& failure,UINT limit)try{
    failure=Failure::Arguments;if(!device || !output.empty() || !limit || limit>CpuTableCache::DefaultDescriptorLimit)return E_INVALIDARG;
    failure=Failure::NotReady;if(!plan.Ready() || !ValidScope(plan.scope_) || plan.tables_.empty())return E_INVALIDARG;
    auto* prior=cache.storage_.get();failure=Failure::Device;if(prior && prior->device.Get()!=device)return E_INVALIDARG;
    failure=Failure::Capacity;const UINT used=prior?prior->descriptors:0;if(used>limit)return E_OUTOFMEMORY;
    struct Selection {std::shared_ptr<const CpuTableCache::Storage::Entry> entry;size_t miss{};bool found{};};
    std::vector<Selection> selections;selections.reserve(plan.tables_.size());
    std::vector<std::pair<const Plan::Table*,uint64_t>> misses;misses.reserve(plan.tables_.size());
    uint64_t hits{};UINT added{};
    for(const auto& table:plan.tables_){
        failure=Failure::Device;
        for(const auto& copy:table.copies)if(!SameDevice(device,copy.heap.Get()))return E_INVALIDARG;
        for(const auto& lease:table.leases){Microsoft::WRL::ComPtr<ID3D12Resource> resource;
            if(SUCCEEDED(lease.As(&resource)) && !SameDevice(device,resource.Get()))return E_INVALIDARG;}
        const auto hash=CpuTableCache::Storage::Hash(table);std::shared_ptr<const CpuTableCache::Storage::Entry> found;
        if(prior){const auto bucket=prior->buckets.find(hash);if(bucket!=prior->buckets.end())for(const auto index:bucket->second){
            const auto& entry=prior->entries[index];if(CpuTableCache::Storage::Same(table,entry->key)){found=entry;break;}}}
        if(found){selections.push_back({found,0,true});++hits;continue;}
        auto pending=std::find_if(misses.begin(),misses.end(),[&](const auto& value){return value.second==hash && CpuTableCache::Storage::Same(table,*value.first);});
        if(pending!=misses.end()){selections.push_back({{},size_t(pending-misses.begin()),false});++hits;continue;}
        failure=Failure::Capacity;if(table.count>limit-used-added)return E_OUTOFMEMORY;
        added+=table.count;selections.push_back({{},misses.size(),false});misses.emplace_back(&table,hash);
    }
    failure=Failure::Capacity;if((prior?prior->entries.size():0)+misses.size()>65536 ||
        hits>UINT64_MAX-(prior?prior->hits:0) || misses.size()>UINT64_MAX-(prior?prior->misses:0))return E_OUTOFMEMORY;
    std::vector<std::shared_ptr<const CpuTableCache::Storage::Entry>> created;created.reserve(misses.size());
    for(const auto& [table,hash]:misses){
        Plan single;single.scope_=plan.scope_;single.ready_=true;single.descriptors_=table->count;single.tables_.push_back(*table);
        std::vector<SharedBankTable> assembled;const auto hr=Assemble(device,single,assembled,failure);if(FAILED(hr))return hr;
        auto entry=std::make_shared<CpuTableCache::Storage::Entry>();entry->key=*table;entry->key.leases.clear();
        entry->heap=assembled[0].cpuHeap;entry->hash=hash;created.push_back(std::move(entry));
    }
    std::vector<SharedBankTable> result;result.reserve(plan.tables_.size());
    for(size_t n=0;n<plan.tables_.size();++n){const auto& table=plan.tables_[n];const auto& selected=selections[n];
        const auto& entry=selected.found?selected.entry:created[selected.miss];SharedBankTable value;
        value.parameter=table.parameter;value.count=table.count;value.cpuHeap=entry->heap;
        value.storageScope=reinterpret_cast<uintptr_t>(value.cpuHeap.Get());value.storageExtent=table.count;
        // A shared heap never substitutes the previous source's copy/epoch lease.
        value.leases=table.leases;failure=Failure::SrvOwner;
        if(table.cbvs.empty() && table.srvs.empty() && !IssueCoverage(value,table.sourceScope,table.srvCoverage,table.sparseSlots))return E_INVALIDARG;
        result.push_back(std::move(value));}
    if(!created.empty()){
        auto next=std::make_unique<CpuTableCache::Storage>();if(prior)*next=*prior;
        else next->device=device;
        next->entries.reserve(next->entries.size()+created.size());
        for(const auto& entry:created){next->buckets[entry->hash].push_back(next->entries.size());next->entries.push_back(entry);}
        next->descriptors=used+added;next->hits+=hits;next->misses+=created.size();
        cache.storage_.swap(next);
    }else if(prior)prior->hits+=hits;
    output.swap(result);failure=Failure::None;return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}
HRESULT AssemblePass(ID3D12Device* device,std::span<const Plan* const> plans,CpuTableCache& cache,
    std::vector<std::vector<SharedBankTable>>& output,Failure& failure,UINT limit)try{
    failure=Failure::Arguments;
    if(!device || !output.empty() || plans.empty() || plans.size()>2048 || !limit || limit>CpuTableCache::DefaultDescriptorLimit)return E_INVALIDARG;
    failure=Failure::Device;if(cache.storage_ && cache.storage_->device.Get()!=device)return E_INVALIDARG;
    struct Reference {size_t group=SIZE_MAX,table=SIZE_MAX;};
    struct Group {Plan::Table table;std::map<UINT,std::pair<descriptors::OwnedSourceSlot,Plan::Copy>> slots;size_t tableIndex{};};
    std::vector<Group> groups;
    std::vector<std::vector<Reference>> references;references.reserve(plans.size());
    Plan combined;size_t referenceCount{};
    const auto eligible=[](const Plan::Table& table){return table.sparseUnionCertified && table.sourceRoot && table.sourceGpu && table.sourceHeap &&
        table.type==D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV && table.cbvs.empty() && table.srvs.empty() && !table.sparseSlots.empty() &&
        table.sparseSlots.size()==table.copies.size() &&
        std::all_of(table.slotTypes.begin(),table.slotTypes.end(),[](const auto type){return type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV;}) &&
        std::all_of(table.sparseSlots.begin(),table.sparseSlots.end(),[](const auto& slot){return slot.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV && slot.stamp.value[0]==2;});};
    const auto sameSlot=[](const descriptors::OwnedSourceSlot& a,const descriptors::OwnedSourceSlot& b){
        // Value includes the lifetime-qualified resource cookie and complete view.
        return a.offset==b.offset && a.type==b.type && a.stamp.gpu==b.stamp.gpu && a.stamp.heap==b.stamp.heap && a.stamp.value==b.stamp.value;};
    for(const auto* plan:plans){failure=Failure::NotReady;
        if(!plan || !plan->Ready() || !ValidScope(plan->scope_) || plan->tables_.empty())return E_INVALIDARG;
        if(references.empty()){combined.scope_=plan->scope_;combined.ready_=true;}
        failure=Failure::Capacity;if(plan->tables_.size()>65536-referenceCount)return E_OUTOFMEMORY;
        referenceCount+=plan->tables_.size();references.emplace_back();auto& current=references.back();current.reserve(plan->tables_.size());
        for(const auto& table:plan->tables_){failure=Failure::Device;
            // Validate even contributors whose equivalent descriptor is not chosen
            // as the union's physical copy source.
            for(const auto& copy:table.copies)if(!SameDevice(device,copy.heap.Get()))return E_INVALIDARG;
            for(const auto& lease:table.leases){Microsoft::WRL::ComPtr<ID3D12Resource> resource;
                if(SUCCEEDED(lease.As(&resource)) && !SameDevice(device,resource.Get()))return E_INVALIDARG;}
            if(!eligible(table)){current.push_back({SIZE_MAX,combined.tables_.size()});combined.tables_.push_back(table);continue;}
            size_t at{};
            for(;at<groups.size();++at){const auto& group=groups[at];const auto& prior=group.table;
                if(prior.sourceRoot!=table.sourceRoot || prior.sourceGpu!=table.sourceGpu || prior.sourceHeap!=table.sourceHeap ||
                    prior.parameter!=table.parameter || prior.type!=table.type)continue;
                if(std::all_of(table.sparseSlots.begin(),table.sparseSlots.end(),[&](const auto& slot){const auto found=group.slots.find(slot.offset);
                    return found==group.slots.end() || sameSlot(found->second.first,slot);}))break;
            }
            if(at==groups.size()){Group group;group.table=table;group.table.copies.clear();group.table.sparseSlots.clear();
                group.table.srvCoverage.clear();group.table.descriptorOwners.clear();group.table.leases.clear();group.table.unionStorage=true;groups.push_back(std::move(group));}
            auto& group=groups[at];group.table.count=std::max(group.table.count,table.count);
            group.table.descriptorOwners.insert(group.table.descriptorOwners.end(),table.descriptorOwners.begin(),table.descriptorOwners.end());
            group.table.srvCoverage.insert(group.table.srvCoverage.end(),table.srvCoverage.begin(),table.srvCoverage.end());
            for(size_t n=0;n<table.sparseSlots.size();++n){const auto& slot=table.sparseSlots[n];const auto& copy=table.copies[n];
                const auto [found,inserted]=group.slots.emplace(slot.offset,std::make_pair(slot,copy));
                if(!inserted && (std::less<ID3D12DescriptorHeap*>{}(copy.heap.Get(),found->second.second.heap.Get()) ||
                    (copy.heap==found->second.second.heap && copy.sourceOffset<found->second.second.sourceOffset)))found->second={slot,copy};}
            current.push_back({at,SIZE_MAX});
        }
    }
    for(auto& group:groups){auto& table=group.table;table.slotTypes.assign(table.count,D3D12_DESCRIPTOR_RANGE_TYPE_SRV);
        std::sort(table.descriptorOwners.begin(),table.descriptorOwners.end(),[](const auto& a,const auto& b){return std::less<IUnknown*>{}(a.Get(),b.Get());});
        table.descriptorOwners.erase(std::unique(table.descriptorOwners.begin(),table.descriptorOwners.end(),[](const auto& a,const auto& b){return a==b;}),table.descriptorOwners.end());
        // Captured owners already hold the actual cookie-validated COM resources.
        // The union must keep every contributor, not only its chosen copy source.
        table.leases=table.descriptorOwners;
        for(const auto& [offset,selected]:group.slots){table.sparseSlots.push_back(selected.first);table.copies.push_back(selected.second);}
        group.tableIndex=combined.tables_.size();combined.tables_.push_back(std::move(table));
    }
    CpuTableCache staged;if(cache.storage_)staged.storage_=std::make_unique<CpuTableCache::Storage>(*cache.storage_);
    std::vector<SharedBankTable> assembled;const auto hr=Assemble(device,combined,staged,assembled,failure,limit);if(FAILED(hr))return hr;
    const auto followers=referenceCount-combined.tables_.size();failure=Failure::Capacity;
    if(followers>UINT64_MAX-staged.storage_->hits)return E_OUTOFMEMORY;
    std::vector<std::vector<SharedBankTable>> result;result.reserve(plans.size());
    for(size_t p=0;p<plans.size();++p){const auto& plan=*plans[p];result.emplace_back();auto& current=result.back();current.reserve(plan.tables_.size());
        for(size_t t=0;t<plan.tables_.size();++t){const auto& original=plan.tables_[t];const auto& reference=references[p][t];
            const auto index=reference.group==SIZE_MAX?reference.table:groups[reference.group].tableIndex;
            SharedBankTable value;value.parameter=original.parameter;value.count=original.count;value.cpuHeap=assembled[index].cpuHeap;
            value.storageScope=assembled[index].storageScope;value.storageExtent=assembled[index].storageExtent;
            value.leases=original.leases;
            if(reference.group!=SIZE_MAX)for(const auto& owner:combined.tables_[index].descriptorOwners)
                if(std::none_of(value.leases.begin(),value.leases.end(),[&](const auto& lease){return lease==owner;}))value.leases.push_back(owner);
            failure=Failure::SrvOwner;
            if(!ReissueCoverage(value,plan.scope_,assembled[index]))return E_INVALIDARG;
            current.push_back(std::move(value));}
    }
    staged.storage_->hits+=followers;cache.storage_.swap(staged.storage_);output.swap(result);failure=Failure::None;return S_OK;
}catch(const std::bad_alloc&){failure=Failure::Capacity;return E_OUTOFMEMORY;}
catch(...){failure=Failure::Exception;return E_FAIL;}
#endif
#endif
}
