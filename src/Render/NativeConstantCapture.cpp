#include "Render/NativeConstantCapture.hpp"
#include "Render/StereoDescriptorAudit.hpp"
#include "Render/RegisteredConstantUpload.hpp"
#include "Render/StereoGeometryCommands.hpp"
#include "Render/StereoGeometryPass.hpp"
#include "Render/StereoTargetArray.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/NativeGeometrySnapshots.hpp"
#include <algorithm>
#include <cstring>
#include <map>
#include <mutex>
#include <stdexcept>
#include <unordered_map>
#if CVR_RUST_VRIK
// The capture's checks: rust/crates/cvr-render/src/constant_capture_checks.rs (the family's uses as
// stereo_family_contracts' ResourceUse).
extern "C" bool cvr_ncc_single_cbvs(const void*,size_t);
extern "C" bool cvr_ncc_same_target(const D3D12_RESOURCE_DESC*,const D3D12_RESOURCE_DESC*);
// The capture's state -- the lock, the readers, the switch, the ledger (constant_ledger.rs), the held families, the
// submissions, the resets, the deferred bindings and the registered trace's collection:
// rust/crates/cvr-render/src/constant_capture_state.rs. This half lends it the exported rows, the descriptors' current
// CBV and the families (a family's CBV uses at a draw, held and released), and keeps the pair's copy and the bank's pass.
struct RustCbvUse {uint64_t descriptor,heap,address;uint32_t bytes,parameter,offset,visibility,shaderRegister,expected,state,pad;};
struct RustCaptureSource {uint64_t family,list,root,sequence;uint32_t side,frame;};
struct RustCaptureHost {
    std::atomic<uint32_t>* enable,*seq,*failure,*drawCount,*bindingCount,*payloadCount,*blobBytes,*pending;
    uint64_t* draws,*bindings,*payloads,*blob;
    bool(*resolveCbv)(uint64_t,uint64_t*,uint64_t*,uint32_t*);
    bool(*uses)(const void*,uint64_t,uint64_t,void*,void(*)(void*,const RustCbvUse*));
    void*(*hold)(const void*);void(*release)(void*);
};
static_assert(sizeof(RustCbvUse)==56 && sizeof(RustCaptureSource)==40 && sizeof(RustCaptureHost)==16*8,"mirrored by constant_capture_state.rs");
extern "C" {
void cvr_ncc2_attach(const RustCaptureHost*);
void cvr_ncc2_lock(bool);
void cvr_ncc2_set_reader(const void*,const void*,const void*,const void*);
void cvr_ncc2_begin();
void cvr_ncc2_snapshot(const RustCaptureSource*,const void*);
void cvr_ncc2_end();
void cvr_ncc2_before_submit(UINT,ID3D12CommandList* const*);
void cvr_ncc2_reset(ID3D12GraphicsCommandList*);
void cvr_ncc2_poll();
void cvr_ncb_poll();
bool cvr_ncc2_pair(UINT,UINT,size_t(*)[2]);
const void* cvr_ncc2_owner(size_t);
bool cvr_ncc2_payload(size_t,uint32_t,uint32_t,uint32_t,uint32_t,const uint8_t**,size_t*);
}
#endif
extern "C" {
#if CVR_RUST_VRIK
// The exported state is Rust's (rust/crates/cvr-render/src/native_state_exports.rs), declared in the header.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/constant_bank.rs'
__declspec(dllexport) cvr::stereo::DepthComparisonResult CyberpunkVR_ConstantCompareData{};
#endif
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantCaptureEnable{},CyberpunkVR_ConstantCaptureSeq{},CyberpunkVR_ConstantCaptureFailure{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantDrawCount{},CyberpunkVR_ConstantBindingCount{},CyberpunkVR_ConstantPayloadCount{},CyberpunkVR_ConstantBlobBytes{},CyberpunkVR_ConstantPendingLists{};
__declspec(dllexport) uint64_t CyberpunkVR_ConstantDraws{},CyberpunkVR_ConstantBindings{},CyberpunkVR_ConstantPayloads{},CyberpunkVR_ConstantBlob{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankRequest{},CyberpunkVR_ConstantBankDone{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankSequences[2*cvr::stereo::constants::BankPairCapacity]{},CyberpunkVR_ConstantBankDescriptors{},CyberpunkVR_ConstantBankDraws{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankPairCount{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankPrepareGeometry{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPreparedDraws{};
__declspec(dllexport) uint64_t CyberpunkVR_ConstantBankInstanceBytes{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantBankGeometryResult{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantBankPassResult{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPassDraws{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPassStage{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantBankPassValidation[3]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantBankCompareDepth{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ConstantCompareState{},CyberpunkVR_ConstantCompareToken{},CyberpunkVR_ConstantCompareSeq{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantCompareResult{};
__declspec(dllexport) uint32_t CyberpunkVR_ConstantCompareStage{};
__declspec(dllexport) cvr::stereo::DepthComparisonResult CyberpunkVR_ConstantCompareData{};
__declspec(dllexport) uint64_t CyberpunkVR_ConstantBankBytes{};
__declspec(dllexport) int32_t CyberpunkVR_ConstantBankResult{};
#endif
}
namespace cvr::stereo::constants {
#if CVR_RUST_VRIK
namespace {
using FamilyRef=std::shared_ptr<const families::Family>;
struct RustMutex {void lock(){cvr_ncc2_lock(true);}void unlock(){cvr_ncc2_lock(false);}} mutex;
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's comparison is rust/crates/cvr-render/src/constant_bank.rs'
std::unique_ptr<StereoDepthComparison> comparison;
#endif
static_assert(sizeof(Binding)==72 && sizeof(Draw)==32 && sizeof(Payload)==16,"mirrored by constant_ledger.rs");
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's host is rust/crates/cvr-render/src/constant_capture_state.rs' own (its families the registry's)
const RustCaptureHost captureHost{&CyberpunkVR_ConstantCaptureEnable,&CyberpunkVR_ConstantCaptureSeq,&CyberpunkVR_ConstantCaptureFailure,
    &CyberpunkVR_ConstantDrawCount,&CyberpunkVR_ConstantBindingCount,&CyberpunkVR_ConstantPayloadCount,&CyberpunkVR_ConstantBlobBytes,
    &CyberpunkVR_ConstantPendingLists,&CyberpunkVR_ConstantDraws,&CyberpunkVR_ConstantBindings,&CyberpunkVR_ConstantPayloads,&CyberpunkVR_ConstantBlob,
    [](uint64_t descriptor,uint64_t* heap,uint64_t* address,uint32_t* bytes){try{descriptors::CbvBinding now;
        if(!descriptors::ResolveCbvAddress(descriptor,now))return false;*heap=now.heap;*address=now.address;*bytes=now.bytes;return true;}catch(...){return false;}},
    [](const void* owner,uint64_t list,uint64_t root,void* sink,void(*push)(void*,const RustCbvUse*)){try{
        const auto& family=**static_cast<const FamilyRef*>(owner);const auto& plan=family.pairedRoot->plan;
        for(const auto& use:family.uses)if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV) {
            RustCbvUse u{};u.visibility=use.visibility;u.shaderRegister=use.shaderRegister;u.expected=use.bytes;
            const auto location=std::find_if(plan.bindings.begin(),plan.bindings.end(),[&](const auto& p){return UINT(p.binding.type)==use.type &&
                UINT(p.binding.visibility)==use.visibility && !use.space && use.count==1 && p.binding.sourceRegister==use.shaderRegister;});
            if(location!=plan.bindings.end() && location->sourceType==D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE) {
                u.state=1;u.parameter=location->sourceParameter;u.offset=location->sourceOffset;descriptors::CbvBinding cbv;
                if(descriptors::ResolveCbv(reinterpret_cast<ID3D12GraphicsCommandList*>(list),root,u.parameter,u.offset,cbv)){
                    u.state=2;u.descriptor=cbv.descriptor;u.heap=cbv.heap;u.address=cbv.address;u.bytes=cbv.bytes;}
            }
            push(sink,&u);
        }
        return true;}catch(...){return false;}},
    [](const void* owner)->void*{try{return new FamilyRef(*static_cast<const FamilyRef*>(owner));}catch(...){return nullptr;}},
    [](void* held){delete static_cast<FamilyRef*>(held);}};
[[maybe_unused]] const bool captureAttached=(cvr_ncc2_attach(&captureHost),true);
#endif
}
#else
namespace {
std::mutex mutex;
ReadBytes reader{};ValidateBytes validator{};bool active{};
MarkPosition markPosition{};ResolvePosition resolvePosition{};
std::vector<uint64_t> positions;
std::vector<ID3D12CommandList*> sourceLists;
UINT deferredCount{};
std::vector<uint64_t> receipts;
std::vector<Draw> draws;
std::vector<Binding> bindings;
std::vector<Payload> payloads;
std::vector<uint8_t> blob;
std::vector<std::shared_ptr<const families::Family>> owners;
std::map<std::vector<uint8_t>,UINT> interned;
std::unordered_map<ID3D12CommandList*,std::vector<UINT>> pending;
std::unique_ptr<StereoDepthComparison> comparison;
constexpr size_t BlobLimit=64*1024*1024,BindingLimit=65536;
uint64_t Hash(std::span<const uint8_t> bytes){uint64_t h=14695981039346656037ull;for(auto b:bytes)h=(h^b)*1099511628211ull;return h;}
void Publish() {
    CyberpunkVR_ConstantDraws=reinterpret_cast<uintptr_t>(draws.data());CyberpunkVR_ConstantDrawCount=UINT(draws.size());
    CyberpunkVR_ConstantBindings=reinterpret_cast<uintptr_t>(bindings.data());CyberpunkVR_ConstantBindingCount=UINT(bindings.size());
    CyberpunkVR_ConstantPayloads=reinterpret_cast<uintptr_t>(payloads.data());CyberpunkVR_ConstantPayloadCount=UINT(payloads.size());
    CyberpunkVR_ConstantBlob=reinterpret_cast<uintptr_t>(blob.data());CyberpunkVR_ConstantBlobBytes=UINT(blob.size());
    CyberpunkVR_ConstantPendingLists=UINT(pending.size())+(deferredCount?1:0);
}
UINT Intern(std::vector<uint8_t> bytes,uint64_t hash) {
    const auto found=interned.find(bytes);if(found!=interned.end())return found->second;
    if(bytes.size()>BlobLimit-blob.size() || payloads.size()>=BindingLimit){CyberpunkVR_ConstantCaptureFailure=1;return 0;}
    const UINT id=UINT(payloads.size());payloads.push_back({UINT(blob.size()),UINT(bytes.size()),hash});
    blob.insert(blob.end(),bytes.begin(),bytes.end());interned.emplace(std::move(bytes),id);return id;
}
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
#if CVR_RUST_TEST_FACADES   // the plugin's callers reach constant_capture_state.rs directly; the Rust test variants call these
void SetReader(ReadBytes read,ValidateBytes validate,MarkPosition mark,ResolvePosition resolve){
    cvr_ncc2_set_reader(reinterpret_cast<const void*>(read),reinterpret_cast<const void*>(validate),reinterpret_cast<const void*>(mark),
        reinterpret_cast<const void*>(resolve));
}
void BeforeSubmit(UINT count,ID3D12CommandList* const* lists){cvr_ncc2_before_submit(count,lists);}
void Reset(ID3D12GraphicsCommandList* list){cvr_ncc2_reset(list);}
#endif
void Begin(){cvr_ncc2_begin();}
void Snapshot(const native_probe::DrawRecord& source,std::shared_ptr<const families::Family> family) {
    if(!family)return;
    const RustCaptureSource draw{family->id,source.list,source.root,source.sequence,source.side,source.frame};cvr_ncc2_snapshot(&draw,&family);
}
void End(){cvr_ncc2_end();}
#endif
#else
void SetReader(ReadBytes read,ValidateBytes validate,MarkPosition mark,ResolvePosition resolve){
    std::lock_guard lock(mutex);reader=read;validator=validate;markPosition=mark;resolvePosition=resolve;
}
void Begin() {
    std::lock_guard lock(mutex);CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
    draws.clear();bindings.clear();receipts.clear();payloads.assign(1,Payload{});blob.clear();owners.clear();interned.clear();pending.clear();
    positions.clear();sourceLists.clear();deferredCount=0;
    CyberpunkVR_ConstantCaptureFailure=0;active=CyberpunkVR_ConstantCaptureEnable.load()!=0;
    Publish();CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
}
void Snapshot(const native_probe::DrawRecord& source,std::shared_ptr<const families::Family> family) {
    if(!CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed) || !family)return;
    std::lock_guard lock(mutex);if(!active || CyberpunkVR_ConstantCaptureFailure.load())return;
    CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
    try {
        if(draws.size()>=native_probe::DrawCapacity)throw std::length_error("constant draw limit");
        Draw draw{family->id,UINT(source.sequence),UINT(bindings.size()),0,1,source.side,source.frame};
        auto* sourceList=reinterpret_cast<ID3D12CommandList*>(source.list);
        const auto position=markPosition?markPosition(sourceList):0;
        if(markPosition && !position)throw std::runtime_error("constant read position unavailable");
        if(position)RegisteredReadDraw(position,draw.sequence,family->id,source.side,source.frame);
        for(const auto& use:family->uses)if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV) {
            if(bindings.size()>=BindingLimit)throw std::length_error("constant binding limit");
            Binding b;b.draw=draw.sequence;b.visibility=use.visibility;b.shaderRegister=use.shaderRegister;b.expectedBytes=use.bytes;b.reserved=1;
            uint64_t receipt{};
            const auto& plan=family->pairedRoot->plan;
            const auto location=std::find_if(plan.bindings.begin(),plan.bindings.end(),[&](const auto& p){return UINT(p.binding.type)==use.type &&
                UINT(p.binding.visibility)==use.visibility && !use.space && use.count==1 && p.binding.sourceRegister==use.shaderRegister;});
            descriptors::CbvBinding cbv;
            if(location==plan.bindings.end() || location->sourceType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE) b.status=2;
            else {
                b.parameter=location->sourceParameter;b.offset=location->sourceOffset;
                if(!descriptors::ResolveCbv(reinterpret_cast<ID3D12GraphicsCommandList*>(source.list),source.root,b.parameter,b.offset,cbv))b.status=2;
                else {
                    b.descriptor=cbv.descriptor;b.heap=cbv.heap;b.address=cbv.address;b.bytes=cbv.bytes;
                    if(position)RegisteredReadBinding(position,use.visibility,use.shaderRegister,cbv.address,use.bytes);
                    if(!use.bytes || cbv.bytes<use.bytes)b.status=4;
                    else {
                        // CBV views round up to 256 bytes; the native allocation
                        // need only supply the shader-declared logical payload.
                        std::vector<uint8_t> bytes(use.bytes),check(use.bytes);
                        auto* list=reinterpret_cast<ID3D12CommandList*>(source.list);
                        uint64_t repeated{};
                        if(!reader || !reader(list,cbv.address,bytes.data(),bytes.size(),&receipt) || !reader(list,cbv.address,check.data(),check.size(),&repeated) || receipt!=repeated || bytes!=check)b.status=8;
                        else {b.hash=Hash(bytes);b.payload=Intern(std::move(bytes),b.hash);b.status=b.payload?1:8;if(receipt)b.reserved|=2;}
                    }
                }
            }
            if(b.status!=1)draw.flags=0;bindings.push_back(b);receipts.push_back(receipt);++draw.count;
        }
        pending[reinterpret_cast<ID3D12CommandList*>(source.list)].push_back(UINT(draws.size()));
        draws.push_back(draw);owners.push_back(std::move(family));positions.push_back(position);sourceLists.push_back(sourceList);
    }catch(...){CyberpunkVR_ConstantCaptureFailure=2;active=false;}
    Publish();CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
}
void End(){std::lock_guard lock(mutex);active=false;
    if(markPosition && CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed) && pending.empty())FreezeRegistered();}
void BeforeSubmit(UINT count,ID3D12CommandList* const* lists) {
    if(!lists || !CyberpunkVR_ConstantPendingLists.load())return;
    std::lock_guard lock(mutex);CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
    try {
        for(UINT i=0;i<count;++i){const auto found=pending.find(lists[i]);if(found==pending.end())continue;
            for(auto index:found->second){auto& draw=draws[index];draw.flags|=2;bool stable=(draw.flags&1)!=0;
                for(UINT j=draw.first;j<draw.first+draw.count;++j){auto& b=bindings[j];
                    const bool deferred=resolvePosition && positions[index] && (b.status==8 || (b.status==1 && receipts[j]));
                    if(b.status!=1 && !deferred){stable=false;continue;}
                    descriptors::CbvBinding now;
                    if(!descriptors::ResolveCbvAddress(b.descriptor,now) || now.heap!=b.heap || now.address!=b.address || now.bytes!=b.bytes){b.status=16;stable=false;continue;}
                    if(deferred){b.status=128;++deferredCount;stable=false;continue;}
                    const auto& p=payloads[b.payload];std::vector<uint8_t> current(p.bytes);
                    bool valid{};
                    if(receipts[j])valid=validator && validator(receipts[j],lists[i],b.address,blob.data()+p.offset,p.bytes);
                    else {uint64_t receipt{};valid=reader && reader(lists[i],b.address,current.data(),current.size(),&receipt) && !receipt && !std::memcmp(current.data(),blob.data()+p.offset,p.bytes);}
                    if(!valid){b.status=32;stable=false;}
                }
                if(stable)draw.flags|=4;
            }pending.erase(found);
        }
        if(markPosition && CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed) && !active && pending.empty())FreezeRegistered();
    }catch(...){CyberpunkVR_ConstantCaptureFailure=2;pending.clear();}
    Publish();CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
}
void Reset(ID3D12GraphicsCommandList* list) {
    if(!CyberpunkVR_ConstantPendingLists.load())return;std::lock_guard lock(mutex);
    const auto found=pending.find(list);if(found==pending.end())return;
    CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
    for(auto index:found->second){auto& draw=draws[index];draw.flags=0;
        for(UINT j=draw.first;j<draw.first+draw.count;++j){if(bindings[j].status==128)--deferredCount;bindings[j].status=64;}}
    pending.erase(found);Publish();CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's pair copies are rust/crates/cvr-render/src/constant_bank.rs'
bool CopyConstantPair(UINT left,UINT right,PairInput& output) {
    std::lock_guard lock(mutex);if(CyberpunkVR_ConstantCaptureFailure.load() || left==right)return false;
    size_t at[2]{};if(!cvr_ncc2_pair(left,right,&at))return false;
    const auto* held=static_cast<const FamilyRef*>(cvr_ncc2_owner(at[0]));auto family=held?*held:nullptr;
    static_assert(sizeof(families::ResourceUse)==24,"mirrored by stereo_family_contracts.rs");
    if(!family || !cvr_ncc_single_cbvs(family->uses.data(),family->uses.size()))return false;
    PairInput result;result.family=family;const auto& plan=family->pairedRoot->plan;
    result.bytes.resize(plan.bindings.size());result.pairs.resize(plan.bindings.size());
    for(size_t i=0;i<plan.bindings.size();++i) {
        auto& pair=result.pairs[i];pair.binding=plan.bindings[i].binding;
        for(UINT eye=0;eye<2;++eye) {
            const uint8_t* data{};size_t bytes{};
            if(!cvr_ncc2_payload(at[eye],plan.bindings[i].sourceParameter,plan.bindings[i].sourceOffset,UINT(pair.binding.visibility),
                pair.binding.sourceRegister,&data,&bytes))return false;
            auto& copy=result.bytes[i][eye];copy.assign(data,data+bytes);pair.eyes[eye].constants=copy;
        }
    }
    output=std::move(result);return true;
}
#endif
#else
bool CopyConstantPair(UINT left,UINT right,PairInput& output) {
    std::lock_guard lock(mutex);if(CyberpunkVR_ConstantCaptureFailure.load() || left==right)return false;
    const auto find=[&](UINT sequence){return std::find_if(draws.begin(),draws.end(),[&](const auto& d){return d.sequence==sequence && d.flags==7;});};
    const auto a=find(left),b=find(right);if(a==draws.end() || b==draws.end() || a->family!=b->family ||
        a->side!=1 || b->side!=0 || !a->frame || a->frame!=b->frame || !a->count || a->count!=b->count)return false;
    auto family=owners[size_t(a-draws.begin())];
    if(!family || std::any_of(family->uses.begin(),family->uses.end(),[](const auto& u){return u.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV || u.space || u.count!=1;}))return false;
    PairInput result;result.family=family;const auto& plan=family->pairedRoot->plan;
    result.bytes.resize(plan.bindings.size());result.pairs.resize(plan.bindings.size());
    for(size_t i=0;i<plan.bindings.size();++i) {
        auto& pair=result.pairs[i];pair.binding=plan.bindings[i].binding;
        for(UINT eye=0;eye<2;++eye) {
            const auto& draw=eye?*b:*a;
            auto found=std::find_if(bindings.begin()+draw.first,bindings.begin()+draw.first+draw.count,[&](const auto& v){return v.status==1 && v.parameter==plan.bindings[i].sourceParameter &&
                v.offset==plan.bindings[i].sourceOffset && v.visibility==UINT(pair.binding.visibility) && v.shaderRegister==pair.binding.sourceRegister;});
            if(found==bindings.begin()+draw.first+draw.count || !found->payload)return false;
            const auto& payload=payloads[found->payload];auto& bytes=result.bytes[i][eye];
            bytes.assign(blob.begin()+payload.offset,blob.begin()+payload.offset+payload.bytes);pair.eyes[eye].constants=bytes;
        }
    }
    output=std::move(result);return true;
}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's bank request is rust/crates/cvr-render/src/constant_bank.rs'
void Poll() {
    PollRegisteredIntent();
    if(comparison && CyberpunkVR_ConstantCompareState.load()==1) {
        DepthComparisonResult result;const auto hr=comparison->Read(result);
        if(hr!=S_FALSE){CyberpunkVR_ConstantCompareSeq.fetch_add(1);CyberpunkVR_ConstantCompareData=result;CyberpunkVR_ConstantCompareResult=hr;
            CyberpunkVR_ConstantCompareState=SUCCEEDED(hr)?2:3;comparison.reset();CyberpunkVR_ConstantCompareSeq.fetch_add(1);}
    }
#if CVR_RUST_VRIK
    cvr_ncc2_poll();   // the registered trace's collection and the deferred bindings (constant_capture_state.rs)
#else
    static bool collecting{};
    const bool requested=CyberpunkVR_ConstantCaptureEnable.load(std::memory_order_relaxed)!=0;
    if(requested && !collecting){ClearRegistered();collecting=true;}
    {
        std::lock_guard lock(mutex);
        if(!active && pending.empty() && deferredCount && resolvePosition) {
            CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
            try {
                RefreshRegisteredFences();
                for(UINT i=0;i<draws.size();++i) {
                    auto& draw=draws[i];bool complete=draw.count!=0;
                    for(UINT j=draw.first;j<draw.first+draw.count;++j) {
                        auto& binding=bindings[j];
                        if(binding.status==128) {
                            std::vector<uint8_t> bytes(binding.expectedBytes);
                            const auto result=resolvePosition(positions[i],sourceLists[i],binding.address,bytes.data(),bytes.size());
                            if(result.status==ConstantWriteTimeline::Status::Ready) {
                                binding.hash=Hash(bytes);binding.payload=Intern(std::move(bytes),binding.hash);
                                binding.status=binding.payload?1:8;binding.reserved=7;--deferredCount;
                            } else if(result.status!=ConstantWriteTimeline::Status::Pending) {
                                binding.status=result.status==ConstantWriteTimeline::Status::Missing?8:32;
                                binding.reserved=0x80000000u|(UINT(result.status)<<16)|UINT(result.detail);--deferredCount;
                            }
                        }
                        complete&=binding.status==1;
                    }
                    if(complete && (draw.flags&2))draw.flags=7;
                }
            }catch(...){CyberpunkVR_ConstantCaptureFailure=2;deferredCount=0;}
            Publish();CyberpunkVR_ConstantCaptureSeq.fetch_add(1);
        }
    }
    if(!requested && collecting && !CyberpunkVR_ConstantPendingLists.load()){ClearRegistered(true);collecting=false;}
#endif
    const auto request=CyberpunkVR_ConstantBankRequest.exchange(0);if(!request)return;
    if(CyberpunkVR_ConstantCompareState.load()==1){CyberpunkVR_ConstantBankResult=E_PENDING;CyberpunkVR_ConstantBankDone=request;return;}
    const bool compareDepth=CyberpunkVR_ConstantBankCompareDepth.load()!=0;
    if(compareDepth){CyberpunkVR_ConstantCompareSeq.fetch_add(1);CyberpunkVR_ConstantCompareToken=request;
        CyberpunkVR_ConstantCompareData={};CyberpunkVR_ConstantCompareResult=E_PENDING;CyberpunkVR_ConstantCompareStage=0;CyberpunkVR_ConstantCompareState=0;CyberpunkVR_ConstantCompareSeq.fetch_add(1);}
    CyberpunkVR_ConstantBankResult=E_FAIL;CyberpunkVR_ConstantBankDescriptors=0;CyberpunkVR_ConstantBankBytes=0;CyberpunkVR_ConstantBankDraws=0;
    CyberpunkVR_ConstantBankPreparedDraws=0;CyberpunkVR_ConstantBankInstanceBytes=0;CyberpunkVR_ConstantBankGeometryResult=E_PENDING;
    CyberpunkVR_ConstantBankPassResult=E_PENDING;CyberpunkVR_ConstantBankPassDraws=0;CyberpunkVR_ConstantBankPassStage=0;
    std::fill_n(CyberpunkVR_ConstantBankPassValidation,3,0u);
    try {
        const auto count=std::max(1u,CyberpunkVR_ConstantBankPairCount.load());
        if(count<=BankPairCapacity) {
            std::vector<PairInput> inputs(count);std::vector<BankDraw> bankDraws;bankDraws.reserve(count);bool valid=true;
            for(UINT i=0;i<count;++i) {
                auto& input=inputs[i];
                if(!CopyConstantPair(CyberpunkVR_ConstantBankSequences[2*i],CyberpunkVR_ConstantBankSequences[2*i+1],input)){valid=false;break;}
                bankDraws.push_back({input.family->pairedRoot->root.Get(),&input.family->pairedRoot->plan,input.pairs,{}});
            }
            if(valid) {
                Microsoft::WRL::ComPtr<ID3D12Device> device;auto hr=bankDraws[0].root->GetDevice(IID_PPV_ARGS(&device));
                if(SUCCEEDED(hr)){StereoBindingBank bank;hr=bank.CreatePass(device.Get(),bankDraws);
                    if(SUCCEEDED(hr)){CyberpunkVR_ConstantBankDescriptors=bank.DescriptorCount();CyberpunkVR_ConstantBankBytes=bank.ConstantBytes();CyberpunkVR_ConstantBankDraws=bank.DrawCount();}}
                CyberpunkVR_ConstantBankResult=hr;
                if(SUCCEEDED(hr) && CyberpunkVR_ConstantBankPrepareGeometry.load()) {
                    std::vector<GeometryCommandPair> geometryInputs(count);bool complete=true;
                    for(UINT i=0;i<count;++i) {
                        auto& pair=geometryInputs[i];pair.instanceRebaseSafe=inputs[i].family->instanceRebaseSafe;
                        if(CyberpunkVR_GeometryFreezeCount.load()) {
                            complete&=geometry_snapshots::CopyPair(CyberpunkVR_ConstantBankSequences[2*i],CyberpunkVR_ConstantBankSequences[2*i+1],pair);
                            for(UINT eye=0;eye<2;++eye){geometry::DrawResources original;
                                complete&=geometry::CopyDraw(CyberpunkVR_ConstantBankSequences[2*i+eye],original) && original.instances==pair.eyes[eye].instances;}
                        }else complete&=geometry::CopyDraw(CyberpunkVR_ConstantBankSequences[2*i],pair.eyes[0]) &&
                            geometry::CopyDraw(CyberpunkVR_ConstantBankSequences[2*i+1],pair.eyes[1]);
                    }
                    StereoGeometryCommands commands;
                    const auto result=complete?commands.Create(device.Get(),geometryInputs):E_FAIL;
                    CyberpunkVR_ConstantBankGeometryResult=result;
                    if(SUCCEEDED(result)){CyberpunkVR_ConstantBankPreparedDraws=commands.DrawCount();CyberpunkVR_ConstantBankInstanceBytes=commands.InstanceBytes();}
                    if(SUCCEEDED(result)) {
                        // Native replacement remains off. An explicit depth
                        // comparison may render only sealed GPU snapshots into
                        // private targets, never historical mutable game buffers.
                        probe::Scope internal;
                        const auto& native=geometryInputs[0].eyes[0];GeometryPassTargets targets;
                        const UINT colorCount=native.draw.rtCount;bool supported=colorCount==0 || colorCount==3;
                        CyberpunkVR_ConstantBankPassStage=1;
                        std::array<StereoTargetArray,4> arrays;HRESULT passResult=E_INVALIDARG;
                        for(UINT slot=colorCount?0:3;supported && slot<4;++slot) {
                            const auto* resource=native.state.targets.resources[slot].Get();
                            if(!resource){supported=false;break;}
                            const auto desc=native.state.targets.resources[slot]->GetDesc();
                            for(const auto& pair:geometryInputs)for(const auto& eye:pair.eyes) {
                                const auto& candidate=eye.state.targets.resources[slot];
                                if(!candidate){supported=false;continue;}
                                const auto d=candidate->GetDesc();
#if CVR_RUST_VRIK
                                supported&=cvr_ncc_same_target(&d,&desc);
#else
                                supported&=d.Dimension==desc.Dimension && d.Width==desc.Width && d.Height==desc.Height && d.Format==desc.Format &&
                                    d.DepthOrArraySize==1 && d.MipLevels==1 && d.SampleDesc.Count==1;
#endif
                            }
                            if(!supported)break;
                            CyberpunkVR_ConstantBankPassStage=2;
                            passResult=arrays[slot].Initialize(device.Get(),desc,slot==3?D3D12_RESOURCE_STATE_DEPTH_WRITE:D3D12_RESOURCE_STATE_RENDER_TARGET);
                            if(FAILED(passResult)){supported=false;break;}
                            if(slot==3)targets.depth=arrays[slot].Resource();else targets.colors.push_back(arrays[slot].Resource());
                        }
                        if(supported) {
                            CyberpunkVR_ConstantBankPassStage=3;
                            std::vector<GeometryPassDraw> passDraws(count);
                            for(UINT i=0;i<count;++i)passDraws[i]={std::move(geometryInputs[i]),inputs[i].family,inputs[i].pairs,3};
                            StereoGeometryPass pass;passResult=pass.Create(device.Get(),passDraws,targets);
                            CyberpunkVR_ConstantBankPassValidation[0]=pass.ValidationStage();CyberpunkVR_ConstantBankPassValidation[1]=pass.ValidationDraw();CyberpunkVR_ConstantBankPassValidation[2]=pass.ValidationBits();
                            if(SUCCEEDED(passResult))CyberpunkVR_ConstantBankPassDraws=pass.DrawCount();
                            if(SUCCEEDED(passResult) && compareDepth) {
                                auto candidate=std::make_unique<StereoDepthComparison>();
                                const auto compareResult=CyberpunkVR_GeometryFreezeCount.load() && colorCount==0?
                                    candidate->Begin(device.Get(),passDraws,passDraws[0].geometry.eyes[0].state.targets.resources[3]->GetDesc()):E_INVALIDARG;
                                CyberpunkVR_ConstantCompareSeq.fetch_add(1);CyberpunkVR_ConstantCompareResult=compareResult;
                                CyberpunkVR_ConstantCompareStage=candidate->Stage();
                                CyberpunkVR_ConstantCompareState=SUCCEEDED(compareResult)?1:3;
                                if(SUCCEEDED(compareResult))comparison=std::move(candidate);CyberpunkVR_ConstantCompareSeq.fetch_add(1);
                            }
                        }else if(SUCCEEDED(passResult))passResult=E_INVALIDARG;
                        CyberpunkVR_ConstantBankPassResult=passResult;
                        if(SUCCEEDED(passResult))CyberpunkVR_ConstantBankPassStage=4;
                    }
                }
            }
        }
    }catch(...){CyberpunkVR_ConstantBankResult=E_OUTOFMEMORY;}
    if(compareDepth && CyberpunkVR_ConstantCompareState.load()==0){CyberpunkVR_ConstantCompareSeq.fetch_add(1);
        CyberpunkVR_ConstantCompareResult=E_FAIL;CyberpunkVR_ConstantCompareState=3;CyberpunkVR_ConstantCompareSeq.fetch_add(1);}
    CyberpunkVR_ConstantBankDone.store(request,std::memory_order_release);
}
#else
// The comparison's read, the capture's poll and the bank request: rust/crates/cvr-render/src/constant_bank.rs.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void Poll(){PollRegisteredIntent();cvr_ncb_poll();}
#endif
#endif
}
