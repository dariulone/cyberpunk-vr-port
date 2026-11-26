#include "Render/NativeCpuCollector.hpp"
#include "Render/NativeJobLease.hpp"
#include "Render/NativeFineVisibility.hpp"
#include "Render/NativeSceneVisibility.hpp"
#include <cstdio>
#include <stdexcept>
#include <type_traits>
using namespace cvr::stereo;
namespace {
std::array<uint8_t,0xE2880> memory{};unsigned created{},initialized{},destroyed{};const void* camera{};const void* flags{};
void* Create(void** out){++created;*out=memory.data();return out;}
void* Destroy(void* owner,uint32_t release){if(owner!=memory.data() || release!=1)throw std::runtime_error("wrong native destructor");++destroyed;return nullptr;}
uintptr_t Init(void* storage,void*,uint8_t mode,const void* cam,void*,void* metadata,const void* options){
    if(storage!=memory.data()+0xC0 || metadata!=memory.data()+0xE05C0 || mode!=3)throw std::runtime_error("wrong native collector shape");
    ++initialized;camera=cam;flags=options;return 99;
}
void Require(bool value){if(!value)throw std::runtime_error("CPU collector ownership failed");}
struct Group {unsigned refs=1,pending=0;};
void Retain(void* p){++static_cast<Group*>(p)->refs;}
void Release(void** p){--static_cast<Group*>(*p)->refs;*p=nullptr;}
bool Done(void* p){return !static_cast<Group*>(p)->pending;}
char Fine(void*,int64_t* range,char partial,void* result){
    const auto* begin=reinterpret_cast<const NativeCullCandidate*>(range[0]);
    const auto* end=reinterpret_cast<const NativeCullCandidate*>(range[1]);
    auto& out=*static_cast<NativeVisibleIds*>(result);
    Require((partial==0 || partial==1) && out.capacity==end-begin && !out.count);
    for(auto* item=begin;item!=end;++item)if(item->includeMask)out.data[out.count++]=item->id;
    return 0;
}
char BadFine(void*,int64_t*,char,void* result){++static_cast<NativeVisibleIds*>(result)->capacity;return 0;}
bool ReadLocal(const void* p,void* out,size_t bytes){std::memcpy(out,p,bytes);return true;}
char Coarse(void*,const void* bounds,uint32_t partial){Require(partial==1);float x{};std::memcpy(&x,bounds,4);return x<0?0:x>3?2:1;}
char InvalidCoarse(void*,const void*,uint32_t){return 3;}
char NegativeCoarse(void*,const void*,uint32_t){return -1;}
NativeSceneVisibility MakeBuckets(std::span<const uint32_t> sizes){
    NativeSceneVisibility result;result.declared=static_cast<uint32_t>(sizes.size());
    for(size_t i=0;i<sizes.size();++i){NativeSceneBucket bucket;
        const std::array<float,8> bounds{float(4+i),0,0,123,float(5+i),1,1,456};
        std::memcpy(bucket.bounds.data(),bounds.data(),32);
        bucket.candidates.resize(sizes[i]);
        for(size_t j=0;j<bucket.candidates.size();++j){auto& candidate=bucket.candidates[j];
            candidate.id=100000*i+j;candidate.bounds={1,2,3,4,5,6};candidate.includeMask=7;candidate.excludeMask=8;candidate.reserved=9;}
        result.candidateCount+=sizes[i];result.buckets.push_back(std::move(bucket));
    }
    return result;
}
const NativeSceneNode& TaggedNode(uintptr_t tag){
    Require((tag&3)==1 || (tag&3)==2);
    return *reinterpret_cast<const NativeSceneNode*>((tag&~uintptr_t(3))-0x10);
}
void BatchSizes(const NativeSceneVisibility::TaggedBatches& owner,std::span<const uint32_t> sizes,uint32_t eye=0){
    Require(owner.Batches(eye).size()==sizes.size());
    for(size_t i=0;i<sizes.size();++i){Require(owner.Batches(eye)[i].count==sizes[i] && sizes[i]<=32);
        for(size_t j=sizes[i];j<32;++j)Require(owner.Batches(eye)[i].tags[j]==0);}
}
struct CoarsePlan {uint32_t eye{};std::vector<char> results;std::vector<std::array<uint32_t,2>>* visits{};};
char PlannedCoarse(void* query,const void* bounds,uint32_t partial){
    auto& plan=*static_cast<CoarsePlan*>(query);float x{};std::memcpy(&x,bounds,4);
    const auto bucket=uint32_t(x-4);Require(partial==1 && x==float(bucket+4) && bucket<plan.results.size());
    if(plan.visits)plan.visits->push_back({plan.eye,bucket});return plan.results[bucket];
}
std::vector<uintptr_t> Tags(const NativeSceneVisibility::TaggedBatches& owner,uint32_t eye){
    std::vector<uintptr_t> result;
    for(const auto& batch:owner.Batches(eye))result.insert(result.end(),batch.tags.begin(),batch.tags.begin()+batch.count);
    return result;
}
void TestPairedTaggedBatches(){
    using Owned=NativeSceneVisibility::TaggedBatches;
    auto source=MakeBuckets(std::array<uint32_t,5>{255,0,1,8,2});
    source.buckets[0].candidates[1]=source.buckets[0].candidates[0];
    std::vector<std::array<uint32_t,2>> visits;
    CoarsePlan left{0,{1,1,2,0,0},&visits},right{1,{0,2,1,2,0},&visits};
    const std::array<void*,2> queries{&left,&right};Owned output;
    Require(source.BuildTaggedBatches(PlannedCoarse,queries,output) && output.EyeCount()==2);
    BatchSizes(output,std::array<uint32_t,1>{3},0);BatchSizes(output,std::array<uint32_t,1>{3},1);
    Require(output.VisibleBuckets()==3 && output.VisibleBuckets(0)==3 && output.VisibleBuckets(1)==3 && !output.VisibleBuckets(2));
    Require(output.CandidateCount()==266 && output.BucketCount()==5 && output.OwnedBytes()==5*56+266*40);
    Require(output.Batches(2).empty() && output.Batches(UINT32_MAX).empty());
    Require(std::vector<uint8_t>(output.BucketMasks().begin(),output.BucketMasks().end())==std::vector<uint8_t>({1,3,3,2,0}));
    Require(visits.size()==10);
    for(uint32_t i=0;i<5;++i){Require(visits[i*2]==std::array<uint32_t,2>{0,i});Require(visits[i*2+1]==std::array<uint32_t,2>{1,i});}
    const auto a=Tags(output,0),b=Tags(output,1);
    Require(a.size()==3 && b.size()==3 && (a[0]&3)==1 && (a[1]&3)==1 && (a[2]&3)==2);
    Require((b[0]&3)==2 && (b[1]&3)==1 && (b[2]&3)==2);
    Require((a[1]&~uintptr_t(3))==(b[0]&~uintptr_t(3)) && (a[2]&~uintptr_t(3))==(b[1]&~uintptr_t(3)));
    Require(TaggedNode(a[0]).bounds[0]==4 && TaggedNode(a[1]).bounds[0]==5 && TaggedNode(a[2]).bounds[0]==6 && TaggedNode(b[2]).bounds[0]==7);
    const auto* duplicate=reinterpret_cast<const NativeCullCandidate*>(TaggedNode(a[0]).candidates);
    Require(duplicate[0].id==duplicate[1].id && duplicate[254].id==254);
    auto mutableBatch=output.Batches(0)[0];mutableBatch.count=0;
    Require(output.Batches(0)[0].count==3 && output.Batches(1)[0].count==3);
    source=NativeSceneVisibility{};Require(TaggedNode(a[0]).count==255 && duplicate[254].id==254);
    Owned moved(std::move(output));Require(Tags(moved,0)==a && Tags(moved,1)==b && !output.EyeCount());
    output=std::move(moved);Require(Tags(output,0)==a && Tags(output,1)==b && !moved.EyeCount());
    const auto savedBytes=output.OwnedBytes();const auto savedMasks=std::vector<uint8_t>(output.BucketMasks().begin(),output.BucketMasks().end());
    source=MakeBuckets(std::array<uint32_t,5>{255,0,1,8,2});right.results[1]=3;
    Require(!source.BuildTaggedBatches(PlannedCoarse,queries,output) && Tags(output,0)==a && Tags(output,1)==b && output.OwnedBytes()==savedBytes);
    Require(std::vector<uint8_t>(output.BucketMasks().begin(),output.BucketMasks().end())==savedMasks);
    const std::array<void*,2> missing{&left,nullptr};Require(!source.BuildTaggedBatches(PlannedCoarse,missing,output) && Tags(output,0)==a);
    std::array<uint32_t,33> emptySizes{};source=MakeBuckets(emptySizes);
    left.results.assign(33,1);right.results.assign(33,0);right.results[0]=right.results[32]=2;
    Require(source.BuildTaggedBatches(PlannedCoarse,queries,output));
    BatchSizes(output,std::array<uint32_t,2>{32,1},0);BatchSizes(output,std::array<uint32_t,1>{2},1);
    Require(output.VisibleBuckets(0)==33 && output.VisibleBuckets(1)==2 && output.OwnedBytes()==33*56 && !output.CandidateCount());
    Require(output.BucketMasks()[0]==3 && output.BucketMasks()[1]==1 && output.BucketMasks()[32]==3);
    source=MakeBuckets(std::array<uint32_t,4>{256,255,1,1});left.results={2,0,2,2};right.results={0,1,1,2};
    Require(source.BuildTaggedBatches(PlannedCoarse,queries,output));
    BatchSizes(output,std::array<uint32_t,2>{1,2},0);BatchSizes(output,std::array<uint32_t,2>{2,1},1);
    Require(output.CandidateCount()==513 && output.OwnedBytes()==4*56+513*40);
    const auto p=Tags(output,0),q=Tags(output,1);Require((p[1]&~uintptr_t(3))==(q[1]&~uintptr_t(3)));
    Require((p[1]&3)==2 && (q[1]&3)==1);
    Require(source.BuildTaggedBatches(PlannedCoarse,&left,output) && output.EyeCount()==1 && output.Batches(1).empty());
    Require(output.VisibleBuckets()==3 && !output.VisibleBuckets(1) && output.BucketMasks()[1]==0 && output.BucketMasks()[2]==1);
    source=NativeSceneVisibility{};Require(source.BuildTaggedBatches(PlannedCoarse,queries,output) && output.EyeCount()==2 && !output.OwnedBytes());
    Require(output.Batches(0).empty() && output.Batches(1).empty() && output.BucketMasks().empty());
}
void TestTaggedBatches(){
    using Owned=NativeSceneVisibility::TaggedBatches;
    static_assert(!std::is_copy_constructible_v<Owned> && !std::is_copy_assignable_v<Owned> &&
        std::is_nothrow_move_constructible_v<Owned> && std::is_nothrow_move_assignable_v<Owned>);
    int query{};Owned output;
    auto source=MakeBuckets(std::array<uint32_t,4>{255,0,1,5});
    source.buckets[0].candidates[1]=source.buckets[0].candidates[0];
    Require(source.BuildTaggedBatches(Coarse,&query,output));
    BatchSizes(output,std::array<uint32_t,2>{3,1});
    Require(output.BucketCount()==4 && output.CandidateCount()==261 && output.VisibleBuckets()==4 && output.EyeCount()==1);
    Require(output.OwnedBytes()==4*56+261*40 && output.Batches(1).empty() && !output.VisibleBuckets(1));
    for(auto mask:output.BucketMasks())Require(mask==1);
    std::array<uintptr_t,4> pointers{};size_t index{};
    for(const auto& batch:output.Batches())for(size_t j=0;j<batch.count;++j){
        const auto tag=batch.tags[j];const auto& node=TaggedNode(tag);const auto& bucket=source.buckets[index];
        pointers[index]=tag;Require((tag&3)==2 && node.next==UINT32_MAX && node.count==bucket.candidates.size() && node.capacity==node.count);
        Require(node.bounds==std::array<float,6>{float(4+index),0,0,float(5+index),1,1});
        if(node.count){Require(node.candidates!=reinterpret_cast<uintptr_t>(bucket.candidates.data()));
            Require(!std::memcmp(reinterpret_cast<void*>(node.candidates),bucket.candidates.data(),node.count*40));}
        ++index;
    }
    Require(index==4);
    const auto* copied=reinterpret_cast<const NativeCullCandidate*>(TaggedNode(pointers[0]).candidates);
    Require(copied[0].id==copied[1].id);
    source=NativeSceneVisibility{};
    Require(TaggedNode(pointers[0]).count==255 && copied[0].includeMask==7 && copied[254].id==254);
    Owned moved(std::move(output));Require(moved.Batches()[0].tags[0]==pointers[0] && output.CandidateCount()==0 && output.VisibleBuckets()==0);
    output=std::move(moved);Require(output.Batches()[1].tags[0]==pointers[3] && moved.CandidateCount()==0 && moved.VisibleBuckets()==0);
    output=std::move(output);Require(output.Batches()[0].tags[0]==pointers[0]);
    source=MakeBuckets(std::array<uint32_t,3>{255,100,1});
    const std::array<float,8> rejected{-2,0,0,0,-1,1,1,0};std::memcpy(source.buckets[1].bounds.data(),rejected.data(),32);
    Require(source.BuildTaggedBatches(Coarse,&query,output));BatchSizes(output,std::array<uint32_t,1>{2});
    Require(output.VisibleBuckets()==2 && output.CandidateCount()==356);
    Require(TaggedNode(output.Batches()[0].tags[1]).bounds[0]==6);
    const auto before=output.Batches()[0].tags[0];
    Require(!source.BuildTaggedBatches(InvalidCoarse,&query,output) && output.Batches()[0].tags[0]==before);
    Require(!source.BuildTaggedBatches(NegativeCoarse,&query,output) && output.Batches()[0].tags[0]==before);
    Require(!source.BuildTaggedBatches(nullptr,&query,output) && !source.BuildTaggedBatches(Coarse,nullptr,output));
    --source.candidateCount;Require(!source.BuildTaggedBatches(Coarse,&query,output));++source.candidateCount;
    --source.declared;Require(!source.BuildTaggedBatches(Coarse,&query,output));++source.declared;
    source.failure=1;Require(!source.BuildTaggedBatches(Coarse,&query,output));source.failure=0;
    source.buckets[0].bounds.fill(0xFF);Require(!source.BuildTaggedBatches(Coarse,&query,output));
    Require(output.Batches()[0].tags[0]==before);
    source=MakeBuckets(std::array<uint32_t,1>{uint32_t(MaxFineCandidates+1)});Require(!source.BuildTaggedBatches(Coarse,&query,output));
    std::array<uint32_t,64> emptySizes{};source=MakeBuckets(emptySizes);
    Require(source.BuildTaggedBatches(Coarse,&query,output));BatchSizes(output,std::array<uint32_t,2>{32,32});
    Require(output.VisibleBuckets()==64 && output.CandidateCount()==0);
    source=MakeBuckets(std::array<uint32_t,3>{256,0,0});
    Require(source.BuildTaggedBatches(Coarse,&query,output));BatchSizes(output,std::array<uint32_t,2>{1,2});
    source=MakeBuckets(std::array<uint32_t,2>{255,0});
    Require(source.BuildTaggedBatches(Coarse,&query,output));BatchSizes(output,std::array<uint32_t,1>{2});
    source=MakeBuckets(std::array<uint32_t,2>{uint32_t(MaxFineCandidates),1});
    Require(source.BuildTaggedBatches(Coarse,&query,output));BatchSizes(output,std::array<uint32_t,2>{1,1});
    source=NativeSceneVisibility{};Require(source.BuildTaggedBatches(Coarse,&query,output) && output.Batches().empty() && !output.BucketCount());
    source=MakeBuckets(std::array<uint32_t,1>{1});std::memcpy(source.buckets[0].bounds.data(),rejected.data(),32);
    Require(source.BuildTaggedBatches(Coarse,&query,output) && output.Batches().empty() && !output.VisibleBuckets());
}
}
int main()try {
    CollectorInput input;input.viewData=reinterpret_cast<void*>(1);input.scene=reinterpret_cast<void*>(2);input.mode=3;
    input.camera.bytes.fill(0x34);input.viewFlags.fill(0x56);const CollectorApi api{Create,Destroy,Init};
    {NativeCpuCollector collector(api);Require(collector.Create(input) && created==1 && initialized==1 && !destroyed);
        Require(camera!=input.camera.bytes.data() && flags!=input.viewFlags.data());input.camera.bytes.fill(0);input.viewFlags.fill(0);
        Require(static_cast<const uint8_t*>(camera)[903]==0x34 && static_cast<const uint8_t*>(flags)[0x32F]==0x56);
        Require(!collector.Create(input) && created==1 && collector.Metadata()!=collector.Storage());}
    Require(destroyed==1);
    {NativeCpuCollector collector({});Require(!collector.Create(input));}
    Require(created==1 && destroyed==1);
    Group begin{1,2},end{1,0};
    {NativeJobLease lease({Retain,Release,Done});Require(!lease.Complete() && lease.Capture(&begin,&end) && !lease.Complete());
        Require(begin.refs==2 && end.refs==2 && !lease.Capture(&end,&end));begin.pending=0;Require(lease.Complete());}
    Require(begin.refs==1 && end.refs==1);
    {NativeJobLease lease({Retain,Release,Done});Require(lease.Capture(&begin,&begin) && begin.refs==2);}
    Require(begin.refs==1);
    std::array<NativeCullCandidate,4> candidates{};
    candidates[0].id=23;candidates[0].includeMask=1;candidates[2]=candidates[0];
    candidates[3].id=91;candidates[3].includeMask=1;std::vector<uint64_t> ids;size_t count{};
    Require(FineCandidateCount(1000,1160,count) && count==4);
    Require(!FineCandidateCount(1000,999,count) && !FineCandidateCount(1000,1001,count) && !FineCandidateCount(0,0,count));
    Require(!FineCandidateCount(1000,1000+40*(MaxFineCandidates+1),count));
    Require(RunNativeFine(Fine,&begin,candidates,1,ids) && ids==std::vector<uint64_t>({23,23,91}));
    Require(!RunNativeFine(BadFine,&begin,candidates,1,ids) && ids.empty());
    Require(!RunNativeFine(nullptr,&begin,candidates,1,ids));
    Require(RunNativeFine(Fine,&begin,{},1,ids) && ids.empty());
    NativeSceneNode nodes[3]{};
    for(auto& node:nodes){node.next=UINT32_MAX;node.bounds={0,0,0,1,1,1};node.candidates=reinterpret_cast<uintptr_t>(candidates.data());node.count=node.capacity=4;}
    nodes[0].next=1;nodes[1].bounds={-2,0,0,-1,1,1};nodes[2].bounds={4,0,0,5,1,1};
    uint32_t heads[]{0,2};NativeScenePoolHeader pool{reinterpret_cast<uintptr_t>(heads),3,2,reinterpret_cast<uintptr_t>(nodes),3,sizeof(NativeSceneNode)};
    NativeSceneVisibility scene;uint32_t visible{};Require(scene.Capture(&pool,ReadLocal) && scene.buckets.size()==3 && scene.candidateCount==12);
    Require(scene.Evaluate(Coarse,Fine,&begin,ids,visible) && visible==2 && ids==std::vector<uint64_t>({23,23,91,23,23,91}));
    nodes[1].next=0;Require(!scene.Capture(&pool,ReadLocal) && scene.failure==4);nodes[1].next=UINT32_MAX;
    pool.size=4;Require(!scene.Capture(&pool,ReadLocal) && scene.failure==2);pool.size=3;
    nodes[0].count=5;Require(!scene.Capture(&pool,ReadLocal) && scene.failure==6);nodes[0].count=4;
    nodes[0].bounds[3]=-1;Require(!scene.Capture(&pool,ReadLocal) && scene.failure==7);nodes[0].bounds[3]=1;
    Require(scene.Capture(&pool,ReadLocal));NativeSceneVisibility other;Require(other.Capture(&pool,ReadLocal) && scene.SameCandidates(other));
    other.buckets[0].candidates[0].id=0;Require(!scene.SameCandidates(other));
    Require(other.Capture(&pool,ReadLocal));other.buckets[0].sourceBegin+=40;
    other.buckets[0].bounds[31]^=0xFF;other.buckets[0].candidates[0].reserved=42;
    Require(scene.SameCandidates(other));const auto diff=scene.CandidateDifferences(other);Require(diff[2]==1 && diff[4]==1 && diff[9]==1);
    other.buckets[0].candidates[0].excludeMask=4;Require(!scene.SameCandidates(other));
    candidates[0].id=0;
    Require(scene.Evaluate(Coarse,Fine,&begin,ids,visible) && ids[0]==23);
    NativeSceneVisibility::TaggedBatches owned;
    Require(scene.BuildTaggedBatches(Coarse,&begin,owned) && owned.VisibleBuckets()==2 && owned.BucketCount()==3);
    const auto savedTag=owned.Batches()[0].tags[0];const auto* ownedIds=reinterpret_cast<const NativeCullCandidate*>(TaggedNode(savedTag).candidates);
    Require(ownedIds[0].id==23 && (savedTag&3)==1 && (owned.Batches()[0].tags[1]&3)==2);
    Require(scene.Capture(&pool,ReadLocal) && scene.buckets[0].candidates[0].id==0 && ownedIds[0].id==23);
    TestTaggedBatches();
    TestPairedTaggedBatches();
    std::puts("PASS native CPU collector owned camera/flags, metadata separation and exact destructor lifetime");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
