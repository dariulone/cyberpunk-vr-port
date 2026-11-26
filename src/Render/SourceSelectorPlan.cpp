#include "Render/SourceSelectorPlan.hpp"
#include <json/json.h>
#include <algorithm>
#include <fstream>
#include <limits>
#include <map>
#include <memory>
#include <set>
#include <stdexcept>
#if CVR_RUST_VRIK
#include "Rust/JsonTree.hpp"
#endif

namespace cvr::stereo::source_selectors {
#if CVR_RUST_VRIK
namespace {
// The rules: rust/crates/cvr-render/src/source_selector_plan.rs; its flat views are mirrored here. jsoncpp still reads
// the text (the external library, configured as before); its tree is lent there flattened (Rust/JsonTree.hpp).
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
#endif
struct RustStr {const char* p;size_t n;};
struct RustCbvKey {RustStr root;uint32_t visibility,shaderRegister,space,parameter,tableOffset;};
struct RustTableKey {RustStr root;uint32_t visibility,parameter,space,shaderRegisterBase,originalTableOffset,declaredExtent;};
struct RustRequirement {RustCbvKey key;uint32_t reflectedBytes,requiredByteExtent;const uint32_t* offsets;size_t offsetCount;};
struct RustSite {RustStr shader,createHandleSSA,indexSSA;uint32_t createHandleLine,byteOffset;RustCbvKey cbv;RustTableKey table;};
struct RustFamily {RustStr root,vertex,pixel;uint32_t number;bool supported;RustStr unsupportedReason;const RustStr* unresolved;size_t unresolvedCount;
    const RustRequirement* cbvs;size_t cbvCount;const RustSite* sites;size_t siteCount;};
struct RustCatalog {RustStr evidence;const RustFamily* families;size_t familyCount;};
struct RustInput {RustCbvKey key;const uint8_t* bytes;size_t length;};
struct RustTable {RustTableKey key;const uint32_t* offsets;size_t offsetCount;uint32_t count;const SlotInterval* uses;size_t useCount;};
struct RustEvaluation {uint32_t evaluatedSites;const SiteSelection* selections;size_t selectionCount;const RustTable* tables;size_t tableCount;};
static_assert(sizeof(RustFamily)==120 && sizeof(RustSite)==136 && sizeof(RustRequirement)==64 &&
    sizeof(RustInput)==56 && sizeof(RustTable)==80 && sizeof(RustEvaluation)==40 && sizeof(RustCatalog)==32 &&
    sizeof(SiteSelection)==16 && sizeof(SlotInterval)==12,"mirrored by source_selector_plan.rs");
extern "C" void* cvr_ssp_load(const json_tree::Node*,size_t,const RustCatalog**,RustStr*);
extern "C" void* cvr_ssp_evaluate(const RustFamily*,const RustInput*,size_t,const RustEvaluation**,RustStr*);
extern "C" void cvr_ssp_free(void*);
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
using Held=std::unique_ptr<void,void(*)(void*)>;
std::string Owned(RustStr s){return s.n?std::string(s.p,s.n):std::string();}
CbvKey Owned(const RustCbvKey& k){return {Owned(k.root),k.visibility,k.shaderRegister,k.space,k.parameter,k.tableOffset};}
TableKey Owned(const RustTableKey& k){
    return {Owned(k.root),k.visibility,k.parameter,k.space,k.shaderRegisterBase,k.originalTableOffset,k.declaredExtent};
}
RustStr Lent(const std::string& s){return {s.data(),s.size()};}
RustCbvKey Lent(const CbvKey& k){return {Lent(k.rootSha256),k.visibility,k.shaderRegister,k.space,k.parameter,k.tableOffset};}
RustTableKey Lent(const TableKey& k){
    return {Lent(k.rootSha256),k.visibility,k.parameter,k.space,k.shaderRegisterBase,k.originalTableOffset,k.declaredExtent};
}
#endif
}
#else
namespace {
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
bool Digest(const std::string& value){return value.size()==64 && std::all_of(value.begin(),value.end(),[](char c){return (c>='0' && c<='9') || (c>='a' && c<='f');});}
bool Ssa(const std::string& value){return value.size()>1 && value.size()<=32 && value[0]=='%' &&
    std::all_of(value.begin()+1,value.end(),[](char c){return c>='0' && c<='9';});}
void Fields(const Json::Value& value,std::initializer_list<const char*> fields){
    Require(value.isObject() && value.getMemberNames().size()==fields.size(),"Unknown/missing selector package fields");
    for(const auto* field:fields)Require(value.isMember(field),"Missing selector package field");
}
uint32_t Number(const Json::Value& value,const char* field,uint32_t maximum=UINT32_MAX){
    const auto& number=value[field];Require((number.type()==Json::intValue || number.type()==Json::uintValue) &&
        number.isUInt() && number.asUInt()<=maximum,"Invalid selector package integer");return number.asUInt();
}
std::string Text(const Json::Value& value,const char* field,size_t maximum){
    const auto& text=value[field];Require(text.isString() && text.asString().size()<=maximum,"Invalid selector package string");return text.asString();
}
CbvKey Cbv(const Json::Value& value){
    Fields(value,{"root","visibility","register","space","parameter","tableOffset"});
    return {Text(value,"root",64),Number(value,"visibility",5),Number(value,"register",13),Number(value,"space"),
        Number(value,"parameter",63),Number(value,"tableOffset")};
}
TableKey Table(const Json::Value& value){
    Fields(value,{"root","visibility","parameter","space","shaderRegisterBase","originalTableOffset","declaredExtent"});
    return {Text(value,"root",64),Number(value,"visibility",5),Number(value,"parameter",63),Number(value,"space"),
        Number(value,"shaderRegisterBase"),Number(value,"originalTableOffset"),Number(value,"declaredExtent")};
}
bool ValidCbv(const CbvKey& key,const FamilyKey& family){
    return key.rootSha256==family.rootSha256 && key.shaderRegister==4 && !key.space && !key.tableOffset &&
        ((key.visibility==1 && key.parameter==1) || (key.visibility==5 && key.parameter==19));
}
bool ValidTable(const TableKey& key,const CbvKey& cbv){
    return key.rootSha256==cbv.rootSha256 && key.visibility==cbv.visibility && !key.shaderRegisterBase && !key.originalTableOffset &&
        key.declaredExtent==DeclaredExtent && ((key.visibility==1 && key.parameter==13 && key.space==1) ||
        (key.visibility==5 && ((key.parameter==31 && key.space==1) || (key.parameter==33 && key.space==3))));
}
void Validate(const FamilyPlan& plan){
    Require(plan.number && plan.number<=MaxFamilies && Digest(plan.key.rootSha256) && Digest(plan.key.vertexSha256) &&
        Digest(plan.key.pixelSha256),"Invalid original selector family identity");
    if(!plan.supported){
        Require(!plan.unsupportedReason.empty() && plan.unsupportedReason.size()<=1024 && !plan.unresolvedInputs.empty() &&
            plan.unresolvedInputs.size()<=64 && plan.cbvs.empty() && plan.sites.empty(),"Unsupported selector family fabricated a partial plan");
        for(const auto& input:plan.unresolvedInputs)Require(!input.empty() && input.size()<=1024,"Invalid unresolved selector input");return;
    }
    Require(plan.unsupportedReason.empty() && plan.unresolvedInputs.empty() && !plan.cbvs.empty() && plan.cbvs.size()<=2 &&
        !plan.sites.empty() && plan.sites.size()<=MaxSites,"Invalid supported selector family extent");
    std::map<CbvKey,std::set<uint32_t>> offsets;
    for(const auto& requirement:plan.cbvs){
        Require(ValidCbv(requirement.key,plan.key) && requirement.reflectedBytes && requirement.reflectedBytes<=65536 &&
            requirement.requiredByteExtent && requirement.requiredByteExtent<=requirement.reflectedBytes &&
            !requirement.dwordOffsets.empty() && requirement.dwordOffsets.size()<=MaxSites,
            "Invalid exact original b4 requirement");
        Require(offsets.emplace(requirement.key,std::set<uint32_t>{}).second,"Duplicate original b4 requirement");
        uint32_t prior{};bool first=true;
        for(const auto offset:requirement.dwordOffsets){
            Require(!(offset&3) && offset<=requirement.reflectedBytes && 4<=requirement.reflectedBytes-offset &&
                (first || offset>prior),"Invalid b4 DWORD offset/ordering/extent");prior=offset;first=false;
        }
    }
    std::set<std::pair<std::string,std::string>> identities;
    for(const auto& site:plan.sites){
        const auto found=offsets.find(site.cbv);
        Require(found!=offsets.end() && ValidCbv(site.cbv,plan.key) && ValidTable(site.table,site.cbv),"Selector lost its exact original CBV/SRV binding");
        const auto& shader=site.cbv.visibility==1?plan.key.vertexSha256:plan.key.pixelSha256;
        Require(site.shaderSha256==shader && Ssa(site.createHandleSSA) && Ssa(site.indexSSA) && site.createHandleSSA!=site.indexSSA &&
            site.createHandleLine && site.createHandleLine<=1000000 && !(site.byteOffset&3),"Selector original shader/SSA/DWORD identity differs");
        Require(identities.emplace(site.shaderSha256,site.createHandleSSA).second,"Duplicate original selector site");
        found->second.insert(site.byteOffset);
    }
    for(const auto& requirement:plan.cbvs){
        const auto& actual=offsets.at(requirement.key);
        Require(actual.size()==requirement.dwordOffsets.size() && std::equal(actual.begin(),actual.end(),requirement.dwordOffsets.begin()) &&
            requirement.requiredByteExtent==*actual.rbegin()+4,"Selector sites and b4 byte requirements differ");
    }
}
FamilyPlan Family(const Json::Value& value){
    Fields(value,{"number","root","vs","ps","supported","unsupportedReason","unresolvedInputs","cbvs","sites"});
    FamilyPlan result;result.key={Text(value,"root",64),Text(value,"vs",64),Text(value,"ps",64)};result.number=Number(value,"number",MaxFamilies);
    Require(value["supported"].isBool(),"Invalid selector support flag");result.supported=value["supported"].asBool();
    result.unsupportedReason=Text(value,"unsupportedReason",1024);
    const auto& unresolved=value["unresolvedInputs"];Require(unresolved.isArray() && unresolved.size()<=64,"Invalid unresolved selector inputs");
    for(const auto& input:unresolved){Require(input.isString() && input.asString().size()<=1024,"Invalid unresolved selector input");result.unresolvedInputs.push_back(input.asString());}
    const auto& cbvs=value["cbvs"];Require(cbvs.isArray() && cbvs.size()<=2,"Invalid b4 requirement count");
    for(const auto& cbv:cbvs){
        Fields(cbv,{"root","visibility","register","space","parameter","tableOffset","reflectedBytes","requiredByteExtent","dwordOffsets"});
        CbvRequirement requirement;requirement.key={Text(cbv,"root",64),Number(cbv,"visibility",5),Number(cbv,"register",13),Number(cbv,"space"),
            Number(cbv,"parameter",63),Number(cbv,"tableOffset")};
        requirement.reflectedBytes=Number(cbv,"reflectedBytes",65536);requirement.requiredByteExtent=Number(cbv,"requiredByteExtent",65536);
        const auto& offsets=cbv["dwordOffsets"];Require(offsets.isArray() && offsets.size()<=MaxSites,"Invalid b4 DWORD count");
        for(const auto& offset:offsets){Require((offset.type()==Json::intValue || offset.type()==Json::uintValue) &&
            offset.isUInt() && offset.asUInt()<=65532,"Invalid b4 DWORD offset");requirement.dwordOffsets.push_back(offset.asUInt());}
        result.cbvs.push_back(std::move(requirement));
    }
    const auto& sites=value["sites"];Require(sites.isArray() && sites.size()<=MaxSites,"Invalid selector site count");
    for(const auto& value:sites){
        Fields(value,{"shader","createHandleSSA","createHandleLine","indexSSA","cbv","byteOffset","operations","table"});
        const auto& operations=value["operations"];Require(operations.isArray() && operations.size()==1,"Only original add-zero selectors are supported");
        Fields(operations[0],{"opcode","operand"});Require(operations[0]["opcode"].isString() && operations[0]["opcode"].asString()=="add" &&
            Number(operations[0],"operand",0)==0,"Only original add-zero selectors are supported");
        Site site;site.shaderSha256=Text(value,"shader",64);site.createHandleSSA=Text(value,"createHandleSSA",32);site.indexSSA=Text(value,"indexSSA",32);
        site.createHandleLine=Number(value,"createHandleLine",1000000);site.byteOffset=Number(value,"byteOffset",65532);
        site.cbv=Cbv(value["cbv"]);site.table=Table(value["table"]);result.sites.push_back(std::move(site));
    }
    Validate(result);return result;
}
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
const FamilyPlan* Catalog::Find(const FamilyKey& key)const{
    const FamilyPlan* result{};for(const auto& plan:families)if(plan.key==key){if(result)return nullptr;result=&plan;}return result;
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool LoadCatalogJson(std::string_view text,Catalog& output,std::string& error){
    output={};error.clear();
    try{
        Require(!text.empty() && text.size()<=MaxCatalogBytes,"Invalid selector catalog byte extent");
        Json::CharReaderBuilder builder;builder["collectComments"]=false;builder["allowComments"]=false;builder["failIfExtra"]=true;
        builder["rejectDupKeys"]=true;builder["strictRoot"]=true;builder["allowTrailingCommas"]=false;builder["stackLimit"]=32;
        std::unique_ptr<Json::CharReader> reader(builder.newCharReader());Json::Value manifest;std::string diagnostic;
        Require(reader->parse(text.data(),text.data()+text.size(),&manifest,&diagnostic),"Invalid selector catalog JSON");
        std::vector<json_tree::Node> nodes(1);json_tree::Flatten(manifest,nodes,0);
        const RustCatalog* view{};RustStr fail{};
        const Held held(cvr_ssp_load(nodes.data(),nodes.size(),&view,&fail),cvr_ssp_free);
        if(!held){error=Owned(fail);return false;}
        Catalog pending;pending.evidenceSha256=Owned(view->evidence);
        for(size_t f=0;f<view->familyCount;++f){const auto& family=view->families[f];FamilyPlan plan;
            plan.key={Owned(family.root),Owned(family.vertex),Owned(family.pixel)};plan.number=family.number;plan.supported=family.supported;
            plan.unsupportedReason=Owned(family.unsupportedReason);
            for(size_t i=0;i<family.unresolvedCount;++i)plan.unresolvedInputs.push_back(Owned(family.unresolved[i]));
            for(size_t i=0;i<family.cbvCount;++i){const auto& r=family.cbvs[i];
                plan.cbvs.push_back({Owned(r.key),r.reflectedBytes,r.requiredByteExtent,std::vector<uint32_t>(r.offsets,r.offsets+r.offsetCount)});}
            for(size_t i=0;i<family.siteCount;++i){const auto& s=family.sites[i];
                plan.sites.push_back({Owned(s.shader),Owned(s.createHandleSSA),Owned(s.indexSSA),s.createHandleLine,s.byteOffset,Owned(s.cbv),Owned(s.table)});}
            pending.families.push_back(std::move(plan));}
        output=std::move(pending);return true;
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Selector catalog exception";return false;}
}
#endif
// The retained sources' catalog (retained_sources.rs reads its rules): the file's text through jsoncpp as LoadCatalogFile
// reads it, cvr_ssp_load's handle kept by the caller; null when absent or invalid.
void* LoadCatalogHandle(const std::filesystem::path& path)noexcept{
    try{
        const auto size=std::filesystem::file_size(path);if(!size || size>MaxCatalogBytes)return nullptr;
        std::string text(size,'\0');std::ifstream file(path,std::ios::binary);file.read(text.data(),std::streamsize(text.size()));
        if(!file)return nullptr;
        Json::CharReaderBuilder builder;builder["collectComments"]=false;builder["allowComments"]=false;builder["failIfExtra"]=true;
        builder["rejectDupKeys"]=true;builder["strictRoot"]=true;builder["allowTrailingCommas"]=false;builder["stackLimit"]=32;
        std::unique_ptr<Json::CharReader> reader(builder.newCharReader());Json::Value manifest;std::string diagnostic;
        if(!reader->parse(text.data(),text.data()+text.size(),&manifest,&diagnostic))return nullptr;
        std::vector<json_tree::Node> nodes(1);json_tree::Flatten(manifest,nodes,0);
        const RustCatalog* view{};RustStr fail{};return cvr_ssp_load(nodes.data(),nodes.size(),&view,&fail);
    }catch(...){return nullptr;}
}
// The plugin's retained geometry loads it from Rust (retained_geometry_native.rs): the path NUL-terminated.
extern "C" void* cvr_cpp_load_selector_catalog(const wchar_t* path) { return LoadCatalogHandle(path); }
#else
bool LoadCatalogJson(std::string_view text,Catalog& output,std::string& error){
    output={};error.clear();
    try{
        Require(!text.empty() && text.size()<=MaxCatalogBytes,"Invalid selector catalog byte extent");
        Json::CharReaderBuilder builder;builder["collectComments"]=false;builder["allowComments"]=false;builder["failIfExtra"]=true;
        builder["rejectDupKeys"]=true;builder["strictRoot"]=true;builder["allowTrailingCommas"]=false;builder["stackLimit"]=32;
        std::unique_ptr<Json::CharReader> reader(builder.newCharReader());Json::Value manifest;std::string diagnostic;
        Require(reader->parse(text.data(),text.data()+text.size(),&manifest,&diagnostic),"Invalid selector catalog JSON");
        Fields(manifest,{"schema","purpose","evidenceSha256","families"});
        Require(Number(manifest,"schema")==1 && manifest["purpose"].isString() &&
            manifest["purpose"].asString()=="original-source-selector-evaluation-only","Unsupported selector catalog schema/purpose");
        Catalog pending;pending.evidenceSha256=Text(manifest,"evidenceSha256",64);Require(Digest(pending.evidenceSha256),"Invalid selector evidence digest");
        const auto& families=manifest["families"];Require(families.isArray() && !families.empty() && families.size()<=MaxFamilies,"Invalid selector family count");
        std::set<FamilyKey> identities;std::set<uint32_t> numbers;size_t sites{};
        for(const auto& value:families){auto plan=Family(value);sites+=plan.sites.size();
            Require(sites<=MaxTotalSites && identities.insert(plan.key).second && numbers.insert(plan.number).second,"Ambiguous/excessive original selector families");
            pending.families.push_back(std::move(plan));}
        output=std::move(pending);return true;
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Selector catalog exception";return false;}
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool LoadCatalogFile(const std::filesystem::path& path,Catalog& output,std::string& error){
    output={};error.clear();
    try{
        const auto size=std::filesystem::file_size(path);Require(size && size<=MaxCatalogBytes,"Invalid selector catalog file extent");
        std::string text(size,'\0');std::ifstream file(path,std::ios::binary);file.read(text.data(),std::streamsize(text.size()));
        Require(bool(file),"Cannot read selector catalog file");return LoadCatalogJson(text,output,error);
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Selector catalog file exception";return false;}
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Evaluate(const FamilyPlan& plan,std::span<const OwnedCbvBytes> bytes,Result& output,std::string& error){
    output={};error.clear();
    try{
        std::vector<RustStr> unresolved;for(const auto& input:plan.unresolvedInputs)unresolved.push_back(Lent(input));
        std::vector<RustRequirement> cbvs;
        for(const auto& r:plan.cbvs)cbvs.push_back({Lent(r.key),r.reflectedBytes,r.requiredByteExtent,r.dwordOffsets.data(),r.dwordOffsets.size()});
        std::vector<RustSite> sites;
        for(const auto& s:plan.sites)sites.push_back({Lent(s.shaderSha256),Lent(s.createHandleSSA),Lent(s.indexSSA),s.createHandleLine,s.byteOffset,
            Lent(s.cbv),Lent(s.table)});
        const RustFamily family{Lent(plan.key.rootSha256),Lent(plan.key.vertexSha256),Lent(plan.key.pixelSha256),plan.number,plan.supported,
            Lent(plan.unsupportedReason),unresolved.data(),unresolved.size(),cbvs.data(),cbvs.size(),sites.data(),sites.size()};
        std::vector<RustInput> inputs;for(const auto& input:bytes)inputs.push_back({Lent(input.key),input.bytes.data(),input.bytes.size()});
        const RustEvaluation* view{};RustStr fail{};
        const Held held(cvr_ssp_evaluate(&family,inputs.data(),inputs.size(),&view,&fail),cvr_ssp_free);
        if(!held){error=Owned(fail);return false;}
        Result pending;pending.family=plan.key;pending.evaluatedSites=view->evaluatedSites;
        pending.selections.assign(view->selections,view->selections+view->selectionCount);
        for(size_t i=0;i<view->tableCount;++i){const auto& t=view->tables[i];TableSelection table;table.key=Owned(t.key);
            table.offsets.assign(t.offsets,t.offsets+t.offsetCount);table.count=t.count;table.uses.assign(t.uses,t.uses+t.useCount);
            pending.tables.push_back(std::move(table));}
        pending.selectorEvaluationComplete=true;output=std::move(pending);return true;
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Selector evaluation exception";return false;}
}
#endif
#else
bool Evaluate(const FamilyPlan& plan,std::span<const OwnedCbvBytes> bytes,Result& output,std::string& error){
    output={};error.clear();
    try{
        Validate(plan);Require(plan.supported,"Original shader family needs unsupported selector inputs");
        Require(bytes.size()==plan.cbvs.size(),"Supplied b4 view set differs from original selector family");
        std::map<CbvKey,std::span<const uint8_t>> inputs;
        for(const auto& input:bytes){const auto found=std::find_if(plan.cbvs.begin(),plan.cbvs.end(),[&](const auto& requirement){return requirement.key==input.key;});
            Require(found!=plan.cbvs.end() && input.bytes.size()==found->reflectedBytes && input.bytes.data() &&
                inputs.emplace(input.key,input.bytes).second,"Supplied b4 identity/logical extent is wrong or duplicated");}
        Result pending;pending.family=plan.key;std::map<TableKey,std::set<uint32_t>> tables;
        for(size_t index=0;index<plan.sites.size();++index){const auto& site=plan.sites[index];const auto input=inputs.at(site.cbv);
            Require(site.byteOffset<=input.size() && 4<=input.size()-site.byteOffset,"Selector DWORD is outside exact b4 bytes");
            const auto* value=input.data()+site.byteOffset;const uint32_t selector=uint32_t(value[0])|(uint32_t(value[1])<<8)|(uint32_t(value[2])<<16)|(uint32_t(value[3])<<24);
            Require(selector<site.table.declaredExtent && site.table.originalTableOffset<=UINT32_MAX-selector,"Actual original selector is outside its declared table");
            const uint32_t offset=site.table.originalTableOffset+selector;
            pending.selections.push_back({uint32_t(index),selector,selector,offset});tables[site.table].insert(offset);
        }
        for(const auto& [key,offsets]:tables){TableSelection table;table.key=key;table.offsets.assign(offsets.begin(),offsets.end());table.count=table.offsets.back()+1;
            for(const auto offset:table.offsets){if(!table.uses.empty() && table.uses.back().offset+table.uses.back().count==offset)++table.uses.back().count;
                else table.uses.push_back({offset,1,0});}pending.tables.push_back(std::move(table));}
        pending.evaluatedSites=uint32_t(plan.sites.size());pending.selectorEvaluationComplete=true;output=std::move(pending);return true;
    }catch(const std::exception& exception){error=exception.what();return false;}catch(...){error="Selector evaluation exception";return false;}
}
#endif
}
