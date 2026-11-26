#include "Render/PreparedMembershipBank.hpp"
#include "Render/NativePreparedPair.hpp"
#include "Render/SourceDescriptorViews.hpp"
#include <algorithm>
#include <map>
#include <set>

#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it (prepared_pair_native.rs builds the banks); the Rust test variants do
namespace cvr::stereo::prepared_membership {
struct Bank::Data {
    MultilayerMembershipSource source;Summary summary;
    native_recording::Lease recording;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> pipeline;
    std::shared_ptr<const shader_snapshot::Snapshot> shader;
    std::shared_ptr<const families::Family> family;
    source_cbv_snapshot::PreparedCopies cbvReceipt;
    source_cbv_snapshot::PrivateCbv b4;
    prepared_pair::Source::FullT5 fullT5;
    SharedBankTable table;
    std::vector<texture_read::ImmutableLease> materials;
};
const MultilayerMembershipSource* Bank::Source()const{return data_?&data_->source:nullptr;}
const Summary* Bank::Statistics()const{return data_?&data_->summary:nullptr;}
namespace {
bool Family(const families::Family& family){
    return family.rootSha256==multilayer_selectors::RootSha256 && family.vertexSha256==multilayer_selectors::VertexSha256 &&
        family.pixelSha256==multilayer_selectors::PixelSha256;
}
bool PrivateRange(ID3D12Resource* resource,UINT64 offset,UINT64 bytes){
    if(!resource || !bytes || offset%4)return false;const auto desc=resource->GetDesc();
    D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
    return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && !desc.Flags && offset<=desc.Width && bytes<=desc.Width-offset &&
        SUCCEEDED(resource->GetHeapProperties(&heap,&flags)) && heap.Type==D3D12_HEAP_TYPE_DEFAULT;
}
}
bool Build(const prepared_pair::Source& source,Bank& output,Failure& failure)try{
    output={};failure=Failure::Arguments;
    if(!source.root || !source.shader || !source.original.state.original || !source.sourceRecording.token ||
        !source.sourceRecording.list || !source.sourceRecording.generation || source.ordinal==UINT64_MAX ||
        source.original.draw.side>1 || source.original.draw.side!=source.owned.draw.side ||
        source.original.draw.list!=source.sourceRecording.list || source.owned.draw.list!=source.sourceRecording.list ||
        source.materials.size()>32768)return false;
    const auto family=families::Lookup(source.original.state.original.Get());failure=Failure::Family;
    if(!family || !Family(*family) || family->sourceRoot!=source.root ||
        shader_snapshot::Capture(source.original.state.original.Get())!=source.shader ||
        source.shader->root.empty() || source.shader->vs.empty() || source.shader->ps.empty())return false;
    const source_binding_assembly::Scope scope{reinterpret_cast<uintptr_t>(source.root.Get()),source.sourceRecording.list,
        source.sourceRecording.generation,source.ordinal+1,source.ordinal+1,source.original.draw.side};
    failure=Failure::Scope;
    if(source.cbvReceipt.list!=scope.list || source.cbvReceipt.generation!=scope.generation || source.cbvReceipt.occurrence!=scope.occurrence)return false;
    const source_cbv_snapshot::PrivateCbv* b4{};
    for(const auto& value:source.cbvs)if(value.parameter==19 && !value.offset){if(b4)return false;b4=&value;}
    failure=Failure::B4;
    if(!b4 || !source_cbv_snapshot::MatchesRecordedPrivateCbv(source.cbvReceipt,*b4) ||
        b4->reflectedBytes!=multilayer_selectors::B4Bytes || b4->copiedBytes!=multilayer_selectors::B4Bytes || b4->paddedBytes!=256 ||
        b4->view.SizeInBytes!=256 || !PrivateRange(b4->resource.Get(),b4->privateOffset,256) ||
        b4->view.BufferLocation!=b4->resource->GetGPUVirtualAddress()+b4->privateOffset)return false;
    failure=Failure::T5;
    if(!source.fullT5)return false;const auto& full=*source.fullT5;D3D12_SHADER_RESOURCE_VIEW_DESC view{};
    if(full.copy.list!=scope.list || full.copy.generation!=scope.generation || full.copy.occurrence!=scope.occurrence ||
        full.copy.sourceOffset || full.range.sourceOffset || full.copy.copiedBytes!=structured_buffer_snapshot::detail::SourceBytes ||
        full.range.copiedBytes!=structured_buffer_snapshot::detail::SourceBytes ||
        !structured_buffer_snapshot::RecordedView(full.copy,full.range,view) ||
        !structured_snapshot_view::detail::SameBufferView(view,full.view) || view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER ||
        view.Buffer.StructureByteStride!=128 || view.Buffer.NumElements!=8192 || view.Buffer.Flags ||
        view.Format!=DXGI_FORMAT_UNKNOWN || view.Shader4ComponentMapping!=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING ||
        view.Buffer.FirstElement>UINT64_MAX/128 || view.Buffer.FirstElement*128!=full.range.privateOffset ||
        !PrivateRange(full.range.resource.Get(),full.range.privateOffset,structured_buffer_snapshot::detail::SourceBytes))return false;
    failure=Failure::Table;const SharedBankTable* table{};
    for(const auto& value:source.referenceTables)if(value.parameter==31){if(table)return false;table=&value;}
    source_binding_assembly::OwnedSrvCoverageView coverage;
    if(!table || !source_binding_assembly::DescribeOwnedSrvCoverage(*table,scope,coverage))return false;
    auto next=std::make_shared<Bank::Data>();next->recording=source.sourceRecording;next->root=source.root;
    next->pipeline=source.original.state.original;next->shader=source.shader;next->family=family;next->cbvReceipt=source.cbvReceipt;
    next->b4=*b4;next->fullT5=full;next->table=*table;
    auto& input=next->source;input.family={family->rootSha256,family->vertexSha256,family->pixelSha256};
    input.b4=b4->resource;input.b4Offset=b4->privateOffset;input.t5=full.range.resource;
    input.t5Offset=full.range.privateOffset;input.t5Bytes=full.range.copiedBytes;input.firstElement=0;input.numElements=8192;
    input.sourceId=source.ordinal+1;input.eye=source.original.draw.side;
    auto& summary=next->summary;summary.ordinal=source.ordinal;summary.nativeSide=scope.eye;summary.sourcePassStorageExtent=table->storageExtent;
    std::set<UINT> offsets;
    for(const auto& material:source.materials){const auto& binding=material.binding;if(binding.parameter!=31)continue;
        ++summary.materialSlots;failure=Failure::Collision;
        if(!offsets.insert(binding.offset).second)return false;
        const auto omit=[&]{++summary.omittedSlots;};
        if(binding.type!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV || binding.offset>=multilayer_selectors::DeclaredExtent ||
            binding.offset>=table->storageExtent || material.buffer || binding.counter){omit();continue;}
        source_binding_assembly::OwnedSrvCoverageView matched;failure=Failure::Coverage;
        if(!source_binding_assembly::MatchesOwnedSrvCoverage(*table,scope,binding.offset,binding.stamp,matched)){omit();continue;}
        D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
        const auto classification=descriptors::borrowed_fixed_detail::Classify(binding.stamp.value,srv);
        const bool null=classification==descriptors::borrowed_fixed_detail::DescriptorClass::TypedNull;
        if(null){if(binding.resource || material.immutable.owner || material.immutable.source.resource){omit();continue;}}
        else{
            if(classification!=descriptors::borrowed_fixed_detail::DescriptorClass::Candidate || !binding.resource ||
                material.immutable.source.resource!=binding.resource || !texture_read::ValidateImmutable(material.immutable) ||
                !descriptors::MatchesHeldResourceCookie(binding.resource.Get(),binding.stamp.value[2]) ||
                !descriptors::borrowed_fixed_detail::ViewMatches(binding.stamp.value,binding.resource->GetDesc(),srv)){omit();continue;}
            next->materials.push_back(material.immutable);
        }
        failure=Failure::Capacity;
        if(!detail::Mark(input.membership,binding.offset,table->storageExtent,summary.coveredSlots,summary.maximumBit))return false;
        if(null)++summary.nullSlots;else ++summary.immutableSlots;
    }
    failure=Failure::Coverage;
    if(summary.coveredSlots+summary.omittedSlots!=summary.materialSlots ||
        !source_binding_assembly::DescribeOwnedSrvCoverage(*table,scope,coverage))return false;
    output.data_=std::move(next);failure=Failure::None;return true;
}catch(...){output={};failure=Failure::Exception;return false;}
}
#endif
