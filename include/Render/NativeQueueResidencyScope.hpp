#pragma once
#include "Render/NativeCommandRecording.hpp"
#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>
#include <vector>

namespace cvr::stereo::texture_read {struct ImmutableLease;}
namespace cvr::stereo::queue_residency {
namespace detail {
using ReadMemory=bool(*)(void*,uint64_t,void*,size_t);
inline constexpr uint32_t MaxLists=256,MaxSetObjects=1048576;
inline constexpr uint64_t ManagerOffset=0x1AE3780,RegistryRva=0x3BCDCA8;
inline constexpr std::array<native_recording::detail::Code,16> Contract{{
    {0x337264,282,0xAEBEF7FE0B6EC646ull},{0x3E12CC,148,0xA99EAC2C327A1A01ull},
    {0x3E68E4,159,0x36A662C5ADF39CDAull},{0x3E69FC,35,0xFC87893BAD1DB49Bull},
    {0x3E7B44,24,0xD3ECE8804151AE74ull},{0xB78604,308,0xABCE987D2A4F1C35ull},
    {0xB676F0,149,0x5EC771CE4585B4E5ull},{0x25303CE,70,0x71CCEFA5FD74E26Bull},
    {0xA75834,111,0xB9B26CE5992548F2ull},{0xA758A4,268,0xD752C8B19FDAE7F9ull},
    {0x3E95FC,178,0x4F51AC4AA4337B33ull},{0x3E96B0,34,0xF45D2BD186398013ull},
    {0x3E9320,729,0x3184C655DC3D5392ull},{0x246A4FE,846,0xED7A4FE1D6329A86ull},
    {0x2FA5ABC,151,0xBAFB466EAECF93A5ull},{0x2FA5B54,319,0x6F012939DC1AA09Aull}
}};
struct Frame {
    uint64_t rip{},rsp{},rbp{},manager{},queue{};
    uint32_t count{};int32_t status{};
    bool operator==(const Frame&)const=default;
};
struct SetHeader {
    uint32_t index{},pad0{};uint64_t entries{};uint32_t capacity{},pad1{};
    uint64_t bytes{};int32_t count{};uint8_t open{},outOfMemory{};uint16_t pad2{};uint64_t syncManager{};
    bool operator==(const SetHeader&)const=default;
};
static_assert(sizeof(SetHeader)==0x30 && offsetof(SetHeader,count)==0x20 && offsetof(SetHeader,syncManager)==0x28);
struct SourceSet {uint64_t address{};SetHeader header;std::vector<uint64_t> entries;};
struct Snapshot {
    uint64_t base{},registry{},thread{},listsAddress{},setsAddress{},fenceObject{},fence{},fenceValue{},generation{},syncManager{};
    Frame frame;std::vector<uint64_t> lists;std::vector<SourceSet> sets;
};
inline bool Read(ReadMemory read,void* user,uint64_t address,void* output,size_t bytes){
    return read && address && bytes && address<=UINT64_MAX-bytes && read(user,address,output,bytes);
}
template<class T> bool Field(ReadMemory read,void* user,uint64_t address,uint64_t offset,T& output){
    return address && address<=UINT64_MAX-offset && Read(read,user,address+offset,&output,sizeof(output));
}
#if CVR_RUST_VRIK
// The readers: rust/crates/cvr-render/src/queue_residency.rs. A Snapshot crosses as a view of its vectors.
struct PageableMembership {
    uint64_t context{},list{},registry{},sourceSet{},managed{},pageable{},generation{},submitted{};
    uint32_t sourceIndex{},memberIndex{},kind{},status{};SetHeader header;
    bool operator==(const PageableMembership&)const=default;
};
using PageableSearch=std::array<uint64_t,20>;
struct SetView {uint64_t address;SetHeader header;const uint64_t* entries;size_t count;};
struct SnapshotView {
    uint64_t base,registry,thread,listsAddress,setsAddress,fenceObject,fence,fenceValue,generation,syncManager;
    Frame frame;const uint64_t* lists;size_t listCount;const SetView* sets;size_t setCount;
};
static_assert(sizeof(Frame)==48 && sizeof(SetView)==72 && sizeof(SnapshotView)==160 && sizeof(PageableMembership)==128 &&
    offsetof(PageableMembership,header)==80,"mirrored by rust/crates/cvr-render/src/queue_residency.rs");
extern "C" {
bool cvr_qr_closed(const SetHeader*,uint64_t);
void* cvr_qr_read_scope(ReadMemory,void*,uint64_t,uint64_t,const Frame*,uint64_t,uint32_t,uint64_t,SnapshotView*,uint32_t*);
void cvr_qr_snapshot_free(void*);
bool cvr_qr_same(const SnapshotView*,const SnapshotView*);
bool cvr_qr_contains_recording(ReadMemory,void*,const SnapshotView*,uint64_t,uint64_t,uint64_t,uint32_t*,std::array<uint64_t,12>*);
bool cvr_qr_contains(ReadMemory,void*,const SnapshotView*,uint64_t,uint64_t,uint64_t,uint32_t,uint64_t,uint32_t*,
    std::array<uint64_t,12>*);
bool cvr_qr_search_invocation_pageable(ReadMemory,void*,const SnapshotView*,uint64_t,uint64_t,PageableSearch*);
bool cvr_qr_read_pageable_membership(ReadMemory,void*,const SnapshotView*,uint64_t,uint64_t,uint64_t,uint64_t,
    PageableMembership*,uint32_t*,std::array<uint64_t,12>*);
}
// A Snapshot's view (valid while the snapshot lives).
struct ViewOf {
    std::vector<SetView> sets;SnapshotView view{};
    explicit ViewOf(const Snapshot& s){
        sets.reserve(s.sets.size());
        for(const auto& set:s.sets)sets.push_back({set.address,set.header,set.entries.data(),set.entries.size()});
        view={s.base,s.registry,s.thread,s.listsAddress,s.setsAddress,s.fenceObject,s.fence,s.fenceValue,s.generation,s.syncManager,
            s.frame,s.lists.data(),s.lists.size(),sets.data(),sets.size()};
    }
};
inline bool Closed(const SetHeader& value,uint64_t syncManager){return cvr_qr_closed(&value,syncManager);}
inline bool ReadScope(ReadMemory read,void* user,uint64_t base,uint64_t thread,const Frame& frame,
                      uint64_t queue,uint32_t count,uint64_t listsAddress,Snapshot& output,uint32_t* rejection=nullptr){
    output={};SnapshotView view{};
    void* held=cvr_qr_read_scope(read,user,base,thread,&frame,queue,count,listsAddress,&view,rejection);
    if(!held)return false;
    Snapshot value;value.base=view.base;value.registry=view.registry;value.thread=view.thread;value.listsAddress=view.listsAddress;
    value.setsAddress=view.setsAddress;value.fenceObject=view.fenceObject;value.fence=view.fence;value.fenceValue=view.fenceValue;
    value.generation=view.generation;value.syncManager=view.syncManager;value.frame=view.frame;
    value.lists.assign(view.lists,view.lists+view.listCount);value.sets.reserve(view.setCount);
    for(size_t i=0;i<view.setCount;++i){const auto& set=view.sets[i];
        value.sets.push_back({set.address,set.header,std::vector<uint64_t>(set.entries,set.entries+set.count)});}
    cvr_qr_snapshot_free(held);output=std::move(value);return true;
}
inline bool Same(const Snapshot& a,const Snapshot& b){const ViewOf x(a),y(b);return cvr_qr_same(&x.view,&y.view);}
inline bool ContainsRecording(ReadMemory read,void* user,const Snapshot& scope,uint64_t context,uint64_t list,
                              uint64_t registry,uint32_t* rejection=nullptr,std::array<uint64_t,12>* observed=nullptr){
    const ViewOf view(scope);return cvr_qr_contains_recording(read,user,&view.view,context,list,registry,rejection,observed);
}
inline bool Contains(ReadMemory read,void* user,const Snapshot& scope,uint64_t context,uint64_t list,
                     uint64_t registry,uint32_t handle,uint64_t resource,uint32_t* rejection=nullptr,
                     std::array<uint64_t,12>* observed=nullptr){
    const ViewOf view(scope);return cvr_qr_contains(read,user,&view.view,context,list,registry,handle,resource,rejection,observed);
}
inline bool SearchInvocationPageable(ReadMemory read,void* user,const Snapshot& scope,uint64_t sourceList,uint64_t pageable,
                                     PageableSearch& output){
    const ViewOf view(scope);return cvr_qr_search_invocation_pageable(read,user,&view.view,sourceList,pageable,&output);
}
inline bool ReadPageableMembership(ReadMemory read,void* user,const Snapshot& scope,uint64_t context,uint64_t list,
                                  uint64_t registry,uint64_t pageable,PageableMembership& output,uint32_t* rejection=nullptr,
                                  std::array<uint64_t,12>* observed=nullptr){
    const ViewOf view(scope);
    return cvr_qr_read_pageable_membership(read,user,&view.view,context,list,registry,pageable,&output,rejection,observed);
}
#else
inline bool Closed(const SetHeader& value,uint64_t syncManager){
    return value.index==UINT32_MAX && !value.open && !value.outOfMemory && value.count>=0 &&
        uint32_t(value.count)<=value.capacity && uint32_t(value.count)<=MaxSetObjects &&
        (!value.count || value.entries) && value.syncManager==syncManager;
}
// These are the registers restored for the native frame at ExecuteReturn, not
// registers belonging to our callback or addresses found by scanning the stack.
// Rejections: 11 arguments, 12 caller, 13 prior HRESULT, 14 stack shape,
// 15 frame queue/count; 20 registry, 21 manager, 22 native queue;
// 30 list array, 31 set array, 32 saved count, 33 array read, 34 list identity;
// 40 lock guard, 41 lock owner, 42 recursion; 50 fence object, 51 fence,
// 52 fence value, 53 generation, 54 sync manager;
// 60 set header, 61 closed set, 62 total bound, 63 entries read, 64 null member.
inline bool ReadScope(ReadMemory read,void* user,uint64_t base,uint64_t thread,const Frame& frame,
                      uint64_t queue,uint32_t count,uint64_t listsAddress,Snapshot& output,uint32_t* rejection=nullptr){
    output={};if(rejection)*rejection=0;
    const auto reject=[&](uint32_t code){if(rejection)*rejection=code;return false;};
    if(!base || base>UINT64_MAX-RegistryRva || !thread || !queue || !count || count>MaxLists || !listsAddress)return reject(11);
    if(!native_recording::detail::Caller(base,frame.rip,native_recording::detail::ExecuteReturn))return reject(12);
    if(frame.status<0)return reject(13);
    if(!frame.rsp || frame.rsp%16 || frame.rsp>UINT64_MAX-0x91 || frame.rbp!=frame.rsp+0x91 || frame.rbp<0x41)return reject(14);
    if(frame.queue!=queue || frame.count!=count)return reject(15);
    Snapshot value;value.base=base;value.thread=thread;value.frame=frame;value.listsAddress=listsAddress;
    uint64_t expectedQueue{},nativeLists{},guard{},owner{};int32_t recursion{};
    if(!Field(read,user,base,RegistryRva,value.registry) || !value.registry || value.registry>UINT64_MAX-ManagerOffset)return reject(20);
    if(frame.manager!=value.registry+ManagerOffset)return reject(21);
    if(!Field(read,user,value.registry,0x14104D0,expectedQueue) || expectedQueue!=queue)return reject(22);
    if(!Field(read,user,frame.rbp,0x5F,nativeLists) || nativeLists!=listsAddress)return reject(30);
    if(!Field(read,user,frame.rbp,0x67,value.setsAddress) || !value.setsAddress)return reject(31);
    if(!Read(read,user,frame.rbp-0x41,&guard,8) || frame.manager>UINT64_MAX-0x138 || guard!=frame.manager+0x138)return reject(40);
    if(!Field(read,user,guard,offsetof(RTL_CRITICAL_SECTION,OwningThread),owner) || owner!=thread)return reject(41);
    if(!Field(read,user,guard,offsetof(RTL_CRITICAL_SECTION,RecursionCount),recursion) || recursion<=0)return reject(42);
    if(!Read(read,user,frame.rbp-0x39,&value.fenceObject,8) || !value.fenceObject)return reject(50);
    if(!Field(read,user,value.fenceObject,0,value.fence) || !value.fence)return reject(51);
    if(!Field(read,user,value.fenceObject,8,value.fenceValue) || !value.fenceValue || value.fenceValue==UINT64_MAX)return reject(52);
    if(!Field(read,user,frame.manager,0xA8,value.generation) || value.generation==UINT64_MAX)return reject(53);
    if(!Field(read,user,frame.manager,0x190,value.syncManager) || !value.syncManager)return reject(54);
    uint32_t nativeCount{};
    if(!Field(read,user,frame.rbp,0x6F,nativeCount) || nativeCount!=count)return reject(32);
    value.lists.resize(count);std::vector<uint64_t> setAddresses(count);
    if(!Read(read,user,listsAddress,value.lists.data(),count*8ull) ||
        !Read(read,user,value.setsAddress,setAddresses.data(),count*8ull))return reject(33);
    size_t total{};value.sets.reserve(count);
    for(uint32_t i=0;i<count;++i){
        if(!value.lists[i] || std::find(value.lists.begin(),value.lists.begin()+i,value.lists[i])!=value.lists.begin()+i)return reject(34);
        SourceSet set;set.address=setAddresses[i];
        if(set.address){
            if(!Read(read,user,set.address,&set.header,sizeof(set.header)))return reject(60);
            if(!Closed(set.header,value.syncManager))return reject(61);
            total+=size_t(set.header.count);if(total>MaxSetObjects)return reject(62);
            set.entries.resize(size_t(set.header.count));
            if(!set.entries.empty() && !Read(read,user,set.header.entries,set.entries.data(),set.entries.size()*8))return reject(63);
            for(const auto entry:set.entries)if(!entry)return reject(64);
        }
        value.sets.push_back(std::move(set));
    }
    output=std::move(value);return true;
}
inline bool Same(const Snapshot& a,const Snapshot& b){
    if(a.base!=b.base || a.registry!=b.registry || a.thread!=b.thread || a.frame!=b.frame || a.listsAddress!=b.listsAddress ||
        a.setsAddress!=b.setsAddress || a.fenceObject!=b.fenceObject || a.fence!=b.fence || a.fenceValue!=b.fenceValue ||
        a.generation!=b.generation || a.syncManager!=b.syncManager || a.lists!=b.lists || a.sets.size()!=b.sets.size())return false;
    for(size_t i=0;i<a.sets.size();++i)if(a.sets[i].address!=b.sets[i].address ||
        a.sets[i].header!=b.sets[i].header || a.sets[i].entries!=b.sets[i].entries)return false;
    return true;
}
// Recording-only membership: 351 arguments/registry, 352 absent list,
// 353 duplicate list, 354/355 context-list read/mismatch,
// 356/357 context-kind read/unsupported. The runtime wrapper separately
// authenticates the pending native lease, including its exact generation.
inline bool ContainsRecording(ReadMemory read,void* user,const Snapshot& scope,uint64_t context,uint64_t list,
                              uint64_t registry,uint32_t* rejection=nullptr,std::array<uint64_t,12>* observed=nullptr){
    if(rejection)*rejection=0;
    if(observed){*observed={};(*observed)[0]=list;(*observed)[10]=scope.generation;(*observed)[11]=UINT64_MAX;}
    const auto reject=[&](uint32_t code){if(rejection)*rejection=code;return false;};
    if(!context || !list || !registry || registry!=scope.registry)return reject(351);
    const auto occurrences=std::count(scope.lists.begin(),scope.lists.end(),list);
    if(!occurrences)return reject(352);
    if(occurrences!=1)return reject(353);
    uint64_t actualList{};uint32_t kind{};
    const bool listRead=Field(read,user,context,0x30,actualList),kindRead=Field(read,user,context,0x68,kind);
    if(observed){(*observed)[1]=actualList;(*observed)[5]=kind;}
    if(!listRead)return reject(354);if(actualList!=list)return reject(355);
    if(!kindRead)return reject(356);if(kind!=1 && kind!=2)return reject(357);
    return true;
}
// Membership rejections: 301 arguments, 302 source list absent, 303 source index,
// 304 no source set; 310/311 context-list read/mismatch, 312/313 tracker read/mismatch,
// 314/315 submission-counter read/zero, 316/317 context-kind read/unsupported;
// 320 set-header read, 321 changed header, 322 not closed; 330 record overflow;
// 331/332 resource read/mismatch, 333/334 pageable read/mismatch (including null),
// 335/336 status read/not resident, 337/338 generation read/mismatch;
// 340 managed member absent, 341 live member read, 342 changed member.
inline bool Contains(ReadMemory read,void* user,const Snapshot& scope,uint64_t context,uint64_t list,
                     uint64_t registry,uint32_t handle,uint64_t resource,uint32_t* rejection=nullptr,
                     std::array<uint64_t,12>* observed=nullptr){
    if(rejection)*rejection=0;
    if(observed){*observed={};(*observed)[0]=list;(*observed)[10]=scope.generation;(*observed)[11]=UINT64_MAX;}
    const auto reject=[&](uint32_t code){if(rejection)*rejection=code;return false;};
    if(!context || !list || registry!=scope.registry || !resource || !handle || handle>32768)return reject(301);
    const auto found=std::find(scope.lists.begin(),scope.lists.end(),list);
    if(found==scope.lists.end())return reject(302);
    const auto index=size_t(found-scope.lists.begin());if(index>=scope.sets.size())return reject(303);
    const auto& set=scope.sets[index];uint64_t actualList{},tracker{},submitted{};uint32_t kind{};
    if(observed)(*observed)[2]=set.address;
    if(!set.address)return reject(304);
    const bool listRead=Field(read,user,context,0x30,actualList),trackerRead=Field(read,user,context,0x630,tracker),
        submittedRead=Field(read,user,context,0x638,submitted),kindRead=Field(read,user,context,0x68,kind);
    if(observed){(*observed)[1]=actualList;(*observed)[3]=tracker;(*observed)[4]=submitted;(*observed)[5]=kind;}
    if(!listRead)return reject(310);if(actualList!=list)return reject(311);
    if(!trackerRead)return reject(312);if(tracker!=set.address)return reject(313);
    if(!submittedRead)return reject(314);if(!submitted)return reject(315);
    if(!kindRead)return reject(316);if(kind!=1 && kind!=2)return reject(317);
    SetHeader current;
    if(!Read(read,user,set.address,&current,sizeof(current)))return reject(320);
    if(current!=set.header)return reject(321);
    if(!Closed(current,scope.syncManager))return reject(322);
    const uint64_t offset=0x2F1D8+uint64_t(handle-1)*0xB0;
    if(registry>UINT64_MAX-offset-0xB0)return reject(330);
    const auto record=registry+offset,managed=record+0x60;
    uint64_t identity{},pageable{},generation{};uint32_t status{};
    const bool identityRead=Field(read,user,record,0,identity),pageableRead=Field(read,user,managed,8,pageable),
        statusRead=Field(read,user,managed,0,status),generationRead=Field(read,user,managed,0x18,generation);
    const auto member=std::find(set.entries.begin(),set.entries.end(),managed);
    if(observed){(*observed)[6]=identity;(*observed)[7]=pageable;(*observed)[8]=status;(*observed)[9]=generation;
        if(member!=set.entries.end())(*observed)[11]=uint64_t(member-set.entries.begin());}
    if(!identityRead)return reject(331);if(identity!=resource)return reject(332);
    if(!pageableRead)return reject(333);if(pageable!=resource)return reject(334);
    if(!statusRead)return reject(335);if(status)return reject(336);
    if(!generationRead)return reject(337);if(generation!=scope.generation)return reject(338);
    // Close clears the insertion bits and index. The retained ppSet entry is the
    // membership witness; neither those bits nor the freed master set can be used.
    if(member==set.entries.end())return reject(340);
    uint64_t actual{};
    if(!Field(read,user,current.entries,uint64_t(member-set.entries.begin())*8,actual))return reject(341);
    if(actual!=managed)return reject(342);
    return true;
}
struct PageableMembership {
    uint64_t context{},list{},registry{},sourceSet{},managed{},pageable{},generation{},submitted{};
    uint32_t sourceIndex{},memberIndex{},kind{},status{};SetHeader header;
    bool operator==(const PageableMembership&)const=default;
};
using PageableSearch=std::array<uint64_t,20>;
// Diagnostic-only whole-invocation scan, never a replacement for source-set
// membership. Unknown native pointers are read as POD, not used as COM objects.
// Errors501 arguments,502 source list,503 set shape/header,504 member pointer,
// 505 managed bytes,506 changed member/header,507 changing scan,508 expired scope.
inline bool SearchInvocationPageable(ReadMemory read,void* user,const Snapshot& scope,uint64_t sourceList,uint64_t pageable,
                                     PageableSearch& output){
    output={1,0,0,pageable,sourceList,UINT64_MAX,scope.sets.size(),0,0,0,0,0,UINT64_MAX,UINT64_MAX,0,0,0,0,0,0};
    const auto fail=[&](uint64_t code){output[2]=code;return false;};
    if(!sourceList || !pageable || scope.lists.empty() || scope.lists.size()>MaxLists || scope.sets.size()!=scope.lists.size())return fail(501);
    const auto source=std::find(scope.lists.begin(),scope.lists.end(),sourceList);
    if(source==scope.lists.end() || std::find(source+1,scope.lists.end(),sourceList)!=scope.lists.end())return fail(502);
    const auto own=size_t(source-scope.lists.begin());output[5]=own;
    size_t total{};for(const auto& set:scope.sets){if(set.entries.size()>MaxSetObjects-total)return fail(503);total+=set.entries.size();}
    std::vector<uint64_t> matches;
    for(size_t index=0;index<scope.sets.size();++index){const auto& set=scope.sets[index];
        if(!set.address){if(!set.entries.empty())return fail(503);continue;}
        SetHeader header;
        if(!Read(read,user,set.address,&header,sizeof(header)) || header!=set.header || !Closed(header,scope.syncManager) ||
            size_t(header.count)!=set.entries.size())return fail(503);
        for(size_t memberIndex=0;memberIndex<set.entries.size();++memberIndex){
            uint64_t managed{},actual{},generation{},again{};uint32_t status{};
            if(!Field(read,user,header.entries,memberIndex*8,managed) || !managed || managed!=set.entries[memberIndex])return fail(504);
            if(!Field(read,user,managed,8,actual))return fail(505);
            ++output[7];if(actual!=pageable)continue;
            if(!Field(read,user,managed,0,status) || !Field(read,user,managed,0x18,generation))return fail(505);
            uint32_t statusAgain{};uint64_t generationAgain{};
            if(!Field(read,user,managed,8,again) || again!=actual || !Field(read,user,managed,0,statusAgain) || statusAgain!=status ||
                !Field(read,user,managed,0x18,generationAgain) || generationAgain!=generation ||
                !Field(read,user,header.entries,memberIndex*8,again) || again!=managed)return fail(506);
            ++output[index==own?8:9];if(!status && generation==scope.generation)++output[index==own?10:11];
            if(output[12]==UINT64_MAX){output[12]=index;output[13]=memberIndex;output[14]=managed;output[15]=scope.lists[index];
                output[16]=status;output[17]=generation;}
            matches.push_back(managed);
        }
        SetHeader after;if(!Read(read,user,set.address,&after,sizeof(after)) || after!=header)return fail(506);
    }
    std::sort(matches.begin(),matches.end());output[18]=std::unique(matches.begin(),matches.end())-matches.begin();
    output[1]=1;return true;
}
// The expected pageable comes exclusively from a held authenticated typed
// ImmutablePageable. Entries are read as native POD, never called as IUnknown.
// Rejections401 arguments,402 source set,403 context read,404 context mismatch,
// 405 header read,406 changed/open set,407 member pointer,408 pageable read,
// 409 absent,410 ambiguous,411 managed state read,412 not resident/generation,
// 413 changed membership. No neighboring source set is searched.
inline bool ReadPageableMembership(ReadMemory read,void* user,const Snapshot& scope,uint64_t context,uint64_t list,
                                  uint64_t registry,uint64_t pageable,PageableMembership& output,uint32_t* rejection=nullptr,
                                  std::array<uint64_t,12>* observed=nullptr){
    output={};if(rejection)*rejection=0;
    if(observed){*observed={};(*observed)[0]=list;(*observed)[6]=pageable;(*observed)[10]=scope.generation;(*observed)[11]=UINT64_MAX;}
    const auto reject=[&](uint32_t code){if(rejection)*rejection=code;return false;};
    if(!context || !list || !registry || registry!=scope.registry || !pageable || scope.lists.size()>MaxLists ||
        scope.lists.size()!=scope.sets.size())return reject(401);
    const auto found=std::find(scope.lists.begin(),scope.lists.end(),list);
    if(found==scope.lists.end() || std::find(found+1,scope.lists.end(),list)!=scope.lists.end())return reject(402);
    const auto index=size_t(found-scope.lists.begin());const auto& source=scope.sets[index];
    if(observed)(*observed)[2]=source.address;
    if(!source.address || source.entries.size()>MaxSetObjects || source.header.count<0 ||
        source.entries.size()!=size_t(source.header.count))return reject(402);
    PageableMembership value;value.context=context;value.list=list;value.registry=registry;value.sourceSet=source.address;
    value.sourceIndex=uint32_t(index);value.pageable=pageable;
    uint64_t actualList{},tracker{};
    if(!Field(read,user,context,0x30,actualList) || !Field(read,user,context,0x630,tracker) ||
        !Field(read,user,context,0x638,value.submitted) || !Field(read,user,context,0x68,value.kind))return reject(403);
    if(observed){(*observed)[1]=actualList;(*observed)[3]=tracker;(*observed)[4]=value.submitted;(*observed)[5]=value.kind;}
    if(actualList!=list || tracker!=source.address || !value.submitted || (value.kind!=1 && value.kind!=2))return reject(404);
    if(!Read(read,user,source.address,&value.header,sizeof(value.header)))return reject(405);
    if(value.header!=source.header || !Closed(value.header,scope.syncManager))return reject(406);
    bool matched{};
    for(size_t i=0;i<source.entries.size();++i){uint64_t member{},candidate{};
        if(!Field(read,user,value.header.entries,uint64_t(i)*8,member) || !member || member!=source.entries[i])return reject(407);
        if(!Field(read,user,member,8,candidate))return reject(408);
        if(candidate!=pageable)continue;
        if(matched)return reject(410);
        matched=true;value.managed=member;value.memberIndex=uint32_t(i);
        if(observed){(*observed)[7]=candidate;(*observed)[11]=i;}
        if(!Field(read,user,member,0,value.status) || !Field(read,user,member,0x18,value.generation))return reject(411);
        if(observed){(*observed)[8]=value.status;(*observed)[9]=value.generation;}
        if(value.status || value.generation!=scope.generation)return reject(412);
    }
    if(!matched)return reject(409);
    // Re-read only the authenticated member and context after the bounded scan.
    SetHeader again;uint64_t member{},candidate{},generation{},submitted{},actual{};uint32_t status{},kind{};
    if(!Read(read,user,source.address,&again,sizeof(again)) || again!=value.header ||
        !Field(read,user,value.header.entries,uint64_t(value.memberIndex)*8,member) || member!=value.managed ||
        !Field(read,user,member,8,candidate) || candidate!=pageable || !Field(read,user,member,0,status) || status!=value.status ||
        !Field(read,user,member,0x18,generation) || generation!=value.generation ||
        !Field(read,user,context,0x30,actual) || actual!=list || !Field(read,user,context,0x630,actual) || actual!=source.address ||
        !Field(read,user,context,0x638,submitted) || submitted!=value.submitted ||
        !Field(read,user,context,0x68,kind) || kind!=value.kind)return reject(413);
    output=value;return true;
}
#endif
struct Proof;
}
struct Scope {std::shared_ptr<const detail::Proof> proof;explicit operator bool()const{return bool(proof);}};
// A synchronous permission inside the exact native ExecuteSubset invocation.
// It expires on return and grants no later-frame or prior-subset residency.
bool Capture(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*,Scope&);
bool Validate(const Scope&);
#if CVR_RUST_VRIK
// The Scope's proof in rust/crates/cvr-render/src/queue_residency.rs (borrowed; null without a proof).
struct IUnknown* RustHandle(const Scope&);
#endif
struct Invocation {
    uint64_t thread{},queue{},listsAddress{};
    std::vector<uint64_t> lists;
};
// Exact current native invocation only, including after source tokens have been
// consumed. This describes ordering identity, not resource-access permission.
bool DescribeInvocation(const Scope&,Invocation&);
// Read-only membership of this pending recording in the exact live invocation.
// Does not require a managed resource set or grant residency, resource lifetime,
// GPU ordering, writable access, or export permission.
bool ContainsRecording(const Scope&,const native_recording::Lease&);
// Requires an unconsumed native submission lease. The caller must separately
// retain and validate ImmutableLease; fixed state flags do not bypass residency.
bool Contains(const Scope&,const native_recording::Lease&,uint64_t registry,uint32_t handle,uint64_t resource);
class PendingPageableUse {
    struct Data;std::shared_ptr<const Data> data_;
    friend bool BindPageableUse(const Scope&,const native_recording::Lease&,const texture_read::ImmutableLease&,PendingPageableUse&);
    friend bool ValidatePageableUse(const PendingPageableUse&);
    friend bool DescribePageableUse(const PendingPageableUse&,struct PageableUseDescription&);
public:explicit operator bool()const{return bool(data_);}
};
struct PageableUseDescription {
    uint64_t managedEntry{},pageable{},memberIndex{},list{},sourceSet{},generation{};
    uint32_t pageableKind{}; // 1 resource, 2 heap; diagnostic identity only.
};
// Bind while the exact source lease is pending. The later check accepts only
// the SAME live native ExecuteSubset invocation, including after prefix tokens
// are consumed. The asset's independent immutable physical/history proof stays
// mandatory. This proves native residency, not access-state or draw permission.
bool BindPageableUse(const Scope&,const native_recording::Lease&,const texture_read::ImmutableLease&,PendingPageableUse&);
bool ValidatePageableUse(const PendingPageableUse&);
// Cached authenticated numeric observations only, not renewed execution rights.
bool DescribePageableUse(const PendingPageableUse&,PageableUseDescription&);
// 1 arguments, 2 code, 3 unwind, 4 native frame/state, 5 changed scope,
// 6 fence/device, 7 source lease, 8 membership, 9 exception,
// 10 immutable asset authentication, 11 typed pageable membership.
uint32_t LastFailure();
// RIP,RSP,RBP,manager,queue,count,raw RBX/HRESULT,LastFailure,detail code,
// expected queue,expected count,expected lists address. On unwind failure the
// register slots describe the last OS-unwound frame, not a claimed native frame.
// Detail code: ReadScope rejection above; 101..109 unwind image/bounds/caller/
// native registers/native metadata/missing metadata/progress/depth/exception;
// 201 missing fence objects, 202 device removed, 203 fence already complete.
// 301..342 are resource membership rejections; 351..357 are recording-only
// membership rejections documented at detail::ContainsRecording.
// 401..413 are typed pageable membership rejections above. For these operations
// MembershipDetails[6] is the expected pageable, not a texture record resource.
std::array<uint64_t,12> LastDetails();
// Expected list,actual context list,expected set,actual context set,submitted
// counter,context kind,record resource,managed pageable,status,managed generation,
// expected generation,member index (UINT64_MAX when absent/not reached).
// Zero fields may mean unread data; LastDetails[8] identifies failed reads.
std::array<uint64_t,12> MembershipDetails();
// Observation only, populated after a strict source-set absent (409) refusal.
// schema,complete,error,expectedPageable,sourceList,sourceSetIndex,sets,visits,
// ownMatches,siblingMatches,ownResidentSameGeneration,siblingResidentSameGeneration,
// firstSetIndex,firstMemberIndex,firstManaged,firstList,firstStatus,firstGeneration,
// distinctManagedMatches,executionPermission(0). UINT64_MAX marks absent indices.
std::array<uint64_t,20> PageableSearchDetails();
}
