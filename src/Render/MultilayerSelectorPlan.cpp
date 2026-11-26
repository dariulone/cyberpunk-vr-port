#include "Render/MultilayerSelectorPlan.hpp"
#include <algorithm>
#include <bit>
#include <cmath>
#include <limits>
#include <set>
#include <stdexcept>

namespace cvr::stereo::multilayer_selectors {
namespace {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's sites are multilayer_selectors.rs' (AuditedSites is the test variants')
constexpr std::array<SiteDefinition,10> Sites{{
    {"%520","%519","%306",100,Transform::Raw},
    {"%624","%623","%306",108,Transform::High16},
    {"%642","%641","%306",108,Transform::Low16},
    {"%660","%659","%306",104,Transform::Low16},
    {"%718","%717","%306",104,Transform::High16},
    {"%991","%990","%777",100,Transform::Raw},
    {"%1093","%1092","%777",108,Transform::High16},
    {"%1111","%1110","%777",108,Transform::Low16},
    {"%1129","%1128","%777",104,Transform::Low16},
    {"%1187","%1186","%777",104,Transform::High16},
}};
#endif
#if !CVR_RUST_VRIK
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
uint32_t Dword(std::span<const uint8_t> bytes,size_t offset){
    Require(bytes.data() && offset<=bytes.size() && 4<=bytes.size()-offset,"Selector DWORD exceeds its exact view bytes");
    const auto* value=bytes.data()+offset;return uint32_t(value[0])|(uint32_t(value[1])<<8)|(uint32_t(value[2])<<16)|(uint32_t(value[3])<<24);
}
bool ExactFamily(const source_selectors::FamilyKey& key){return key.rootSha256==RootSha256 && key.vertexSha256==VertexSha256 && key.pixelSha256==PixelSha256;}
bool ExactB4(const source_selectors::CbvKey& key){return key.rootSha256==RootSha256 && key.visibility==5 && key.shaderRegister==4 &&
    !key.space && key.parameter==19 && !key.tableOffset;}
void View(const StructuredView& view){
    Require(view.key.rootSha256==RootSha256 && view.key.visibility==5 && view.key.shaderRegister==5 && !view.key.space &&
        view.key.parameter==23 && view.key.tableOffset==5,"Multilayer source is not exact original PS t5 binding");
    Require(view.explicitDescription && view.dimension==1 && !view.format && view.componentMapping==5768 && !view.flags &&
        view.structureByteStride==Stride && view.numElements && view.resourceWidth,"Multilayer source is not an explicit structured128 SRV");
    Require(view.firstElement<=UINT64_MAX/Stride,"Structured FirstElement byte offset overflows");
    const uint64_t start=view.firstElement*Stride,size=uint64_t(view.numElements)*Stride;
    Require(start<=view.resourceWidth && size<=view.resourceWidth-start,"Full structured SRV view exceeds its current COM buffer");
}
ReadPlan Make(const source_selectors::FamilyKey& family,const source_selectors::OwnedCbvBytes& b4,const StructuredView& view){
    static_assert(sizeof(float)==4 && std::numeric_limits<float>::is_iec559);
    Require(ExactFamily(family) && ExactB4(b4.key),"Multilayer original root/VS/PS/b4 identity differs");
    Require(b4.bytes.size()==B4Bytes && b4.bytes.data(),"Multilayer b4 is not the exact original228 logical bytes");View(view);
    const float raw=std::bit_cast<float>(Dword(b4.bytes,128));
    Require(std::isfinite(raw) && raw>=0 && double(raw)<4294967296.0,"Multilayer base has unsupported FPToui domain");
    ReadPlan result;result.family=family;result.b4Key=b4.key;std::copy(b4.bytes.begin(),b4.bytes.end(),result.b4Bytes.begin());result.view=view;
    result.baseRecord=uint32_t(std::trunc(double(raw)));result.mask=Dword(b4.bytes,224);
    // Wave OR of (t4 masks AND this uniform b4 mask) cannot introduce new bits.
    // Both original load paths guard the nonzero first-bit selection.
    for(uint32_t bit=0;bit<MaxRecords;++bit)if(result.mask&(uint32_t(1)<<bit)){
        const uint64_t record=uint64_t(result.baseRecord)+bit;
        Require(record<=UINT32_MAX && record<view.numElements,"Possible multilayer record exceeds original structured view");
        result.records.push_back(uint32_t(record));
    }
    if(!result.records.empty()){
        result.firstRecord=result.records.front();result.recordSpan=result.records.back()-result.firstRecord+1;
        Require(result.recordSpan<=MaxRecords && view.firstElement<=UINT64_MAX-result.firstRecord,"Multilayer record/FirstElement span overflows");
        const uint64_t first=view.firstElement+result.firstRecord;
        Require(first<=UINT64_MAX/Stride,"Multilayer COM byte start overflows");
        result.resourceOffset=first*Stride;result.byteExtent=result.recordSpan*Stride;
        Require(result.byteExtent<=MaxReadBytes && result.resourceOffset<=view.resourceWidth && result.byteExtent<=view.resourceWidth-result.resourceOffset,
            "Multilayer compact read span exceeds COM backing");
    }
    result.conservativeRecordSuperset=true;result.originalSiteCoverageComplete=true;return result;
}
bool SamePlan(const ReadPlan& a,const ReadPlan& b){return a.family==b.family && a.b4Key==b.b4Key && a.b4Bytes==b.b4Bytes && a.view==b.view &&
    a.baseRecord==b.baseRecord && a.mask==b.mask && a.firstRecord==b.firstRecord && a.recordSpan==b.recordSpan && a.records==b.records &&
    a.resourceOffset==b.resourceOffset && a.byteExtent==b.byteExtent;}
uint32_t Index(uint32_t value,Transform transform){switch(transform){case Transform::Raw:return value;case Transform::Low16:return value&65535;
    case Transform::High16:return value>>16;}throw std::runtime_error("Unsupported multilayer selector transformation");}
#endif
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
std::span<const SiteDefinition> AuditedSites(){return Sites;}
#endif
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/multilayer_selectors.rs (tools/single_pass_tests/multilayer_selector_plan_test
// runs its cases against them in its Rust variant). The strings stay here, lent to the rules, which hand back numbers.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
namespace {
struct RustStr {const char* ptr;size_t len;};
struct RustView {
    RustStr root;uint32_t visibility,shaderRegister,space,parameter,tableOffset,numElements;uint64_t firstElement,resourceWidth;
    uint32_t structureByteStride,format,dimension,componentMapping,flags;bool explicitDescription;
};
struct RustIdentity {
    RustStr familyRoot,familyVertex,familyPixel,b4Root;uint32_t b4Visibility,b4Register,b4Space,b4Parameter,b4TableOffset;
    const uint8_t* b4;size_t b4Len;
};
struct RustNumbers {uint32_t baseRecord,mask,firstRecord,recordSpan;uint64_t resourceOffset;uint32_t byteExtent;};
struct RustError {const char* message;size_t length;};
struct RustSelection {uint32_t site,value,selected;};
struct RustUse {uint32_t offset,count;};
struct RustEnumerationView {const RustSelection* selections;size_t selectionCount;const uint32_t* offsets;size_t offsetCount;const RustUse* uses;size_t useCount;};
struct RustRecords;struct RustEnumeration;
static_assert(sizeof(RustStr)==16 && sizeof(RustView)==80 && sizeof(RustIdentity)==104 && sizeof(RustNumbers)==32,
    "mirrored by multilayer_selectors.rs");
extern "C" {
RustRecords* cvr_ml_plan(const RustIdentity*,const RustView*,RustNumbers*,RustError*);
const uint32_t* cvr_ml_records(const RustRecords*,size_t*);
void cvr_ml_free(RustRecords*);
RustEnumeration* cvr_ml_enumerate(const RustIdentity*,const RustView*,const RustNumbers*,const uint32_t*,size_t,const RustView*,
    uint64_t,const uint8_t*,size_t,RustEnumerationView*,RustError*);
void cvr_ml_enumeration_free(RustEnumeration*);
}
RustStr Lend(const std::string& text){return {text.data(),text.size()};}
RustView Lend(const StructuredView& v){
    return {Lend(v.key.rootSha256),v.key.visibility,v.key.shaderRegister,v.key.space,v.key.parameter,v.key.tableOffset,v.numElements,
        v.firstElement,v.resourceWidth,v.structureByteStride,v.format,v.dimension,v.componentMapping,v.flags,v.explicitDescription};
}
RustIdentity Lend(const source_selectors::FamilyKey& f,const source_selectors::CbvKey& k,const uint8_t* b4,size_t length){
    return {Lend(f.rootSha256),Lend(f.vertexSha256),Lend(f.pixelSha256),Lend(k.rootSha256),k.visibility,k.shaderRegister,k.space,
        k.parameter,k.tableOffset,b4,length};
}
}
bool PlanRead(const source_selectors::FamilyKey& family,const source_selectors::OwnedCbvBytes& b4,const StructuredView& view,ReadPlan& output,std::string& error){
    output={};error.clear();try{
        const auto identity=Lend(family,b4.key,b4.bytes.data(),b4.bytes.size());const auto lent=Lend(view);
        RustNumbers numbers{};RustError failure{};
        const auto records=cvr_ml_plan(&identity,&lent,&numbers,&failure);
        if(!records){error.assign(failure.message,failure.length);return false;}
        struct Release{RustRecords* records;~Release(){cvr_ml_free(records);}}release{records};
        size_t count{};const auto data=cvr_ml_records(records,&count);
        ReadPlan result;result.family=family;result.b4Key=b4.key;std::copy(b4.bytes.begin(),b4.bytes.end(),result.b4Bytes.begin());
        result.view=view;result.baseRecord=numbers.baseRecord;result.mask=numbers.mask;result.firstRecord=numbers.firstRecord;
        result.recordSpan=numbers.recordSpan;result.records.assign(data,data+count);result.resourceOffset=numbers.resourceOffset;
        result.byteExtent=numbers.byteExtent;result.conservativeRecordSuperset=true;result.originalSiteCoverageComplete=true;
        output=std::move(result);return true;
    }catch(...){output={};error="Multilayer read-plan exception";return false;}
}
bool Enumerate(const ReadPlan& plan,const RecordSnapshot& snapshot,source_selectors::Result& output,std::string& error){
    output={};error.clear();try{
        const auto identity=Lend(plan.family,plan.b4Key,plan.b4Bytes.data(),plan.b4Bytes.size());
        const auto planView=Lend(plan.view),snapshotView=Lend(snapshot.view);
        const RustNumbers numbers{plan.baseRecord,plan.mask,plan.firstRecord,plan.recordSpan,plan.resourceOffset,plan.byteExtent};
        RustEnumerationView view{};RustError failure{};
        const auto enumeration=cvr_ml_enumerate(&identity,&planView,&numbers,plan.records.data(),plan.records.size(),&snapshotView,
            snapshot.resourceOffset,snapshot.bytes.data(),snapshot.bytes.size(),&view,&failure);
        if(!enumeration){error.assign(failure.message,failure.length);return false;}
        struct Release{RustEnumeration* e;~Release(){cvr_ml_enumeration_free(e);}}release{enumeration};
        source_selectors::Result pending;pending.family=plan.family;
        for(size_t i=0;i<view.selectionCount;++i){const auto& s=view.selections[i];pending.selections.push_back({s.site,s.value,s.selected,s.selected});}
        if(view.offsetCount){
            source_selectors::TableSelection table;table.key={std::string(RootSha256),5,31,1,0,0,DeclaredExtent};
            table.offsets.assign(view.offsets,view.offsets+view.offsetCount);table.count=table.offsets.back()+1;
            for(size_t i=0;i<view.useCount;++i)table.uses.push_back({view.uses[i].offset,view.uses[i].count,0});
            pending.tables.push_back(std::move(table));
        }
        pending.evaluatedSites=uint32_t(pending.selections.size());pending.selectorEvaluationComplete=true;output=std::move(pending);return true;
    }catch(...){output={};error="Multilayer selector enumeration exception";return false;}
}
#endif
#else
bool PlanRead(const source_selectors::FamilyKey& family,const source_selectors::OwnedCbvBytes& b4,const StructuredView& view,ReadPlan& output,std::string& error){
    output={};error.clear();try{output=Make(family,b4,view);return true;
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Multilayer read-plan exception";return false;}
}
bool Enumerate(const ReadPlan& plan,const RecordSnapshot& snapshot,source_selectors::Result& output,std::string& error){
    output={};error.clear();try{
        const auto checked=Make(plan.family,{plan.b4Key,plan.b4Bytes},plan.view);
        Require(SamePlan(plan,checked),"Multilayer read-plan metadata is torn");
        Require(snapshot.view==checked.view && snapshot.resourceOffset==checked.resourceOffset && snapshot.bytes.size()==checked.byteExtent &&
            (!checked.byteExtent || snapshot.bytes.data()),"Multilayer record snapshot is not its exact planned COM subview");
        source_selectors::Result pending;pending.family=checked.family;std::set<uint32_t> offsets;
        for(const auto record:checked.records){const size_t at=size_t(record-checked.firstRecord)*Stride;
            for(size_t index=0;index<Sites.size();++index){const auto& site=Sites[index];const auto value=Dword(snapshot.bytes,at+site.fieldOffset);
                const auto selected=Index(value,site.transform);Require(selected<DeclaredExtent,"Conservative multilayer selector candidate exceeds declared32768 SRVs");
                pending.selections.push_back({uint32_t(index),value,selected,selected});offsets.insert(selected);
            }
        }
        if(!offsets.empty()){
            source_selectors::TableSelection table;table.key={std::string(RootSha256),5,31,1,0,0,DeclaredExtent};
            table.offsets.assign(offsets.begin(),offsets.end());table.count=table.offsets.back()+1;
            for(const auto offset:table.offsets){if(!table.uses.empty() && table.uses.back().offset+table.uses.back().count==offset)++table.uses.back().count;
                else table.uses.push_back({offset,1,0});}pending.tables.push_back(std::move(table));
        }
        pending.evaluatedSites=uint32_t(pending.selections.size());pending.selectorEvaluationComplete=true;output=std::move(pending);return true;
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Multilayer selector enumeration exception";return false;}
}
#endif
}
