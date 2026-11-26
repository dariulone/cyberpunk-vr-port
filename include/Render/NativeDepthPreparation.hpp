#pragma once
#include <array>
#include <atomic>
#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <unordered_set>
#include <vector>

namespace cvr::stereo::eager_depth {
struct alignas(16) Packet {
    uint64_t first{},second{};
    bool operator==(const Packet&) const=default;
};
static_assert(sizeof(Packet)==16 && alignof(Packet)==16);
struct Array {Packet* data{};uint32_t capacity{},count{};};
static_assert(sizeof(Array)==16);
using Read=bool(*)(uint64_t,void*,size_t);
using Prepare=void(*)(void*,void*,void*,uint32_t,uint32_t);
using Gather=void*(*)(void*,void*);
using Filter=int64_t(*)(void*,char,uint32_t,uint32_t);
using Finalize=void(*)(void*,char,int64_t,int64_t);
struct Api {Read read{};Prepare prepare{};Gather gather{};Filter filter{};Finalize finalize{};};
constexpr uint32_t MaxPackets=32768;
enum Counter:uint32_t {Main,Prepared,Vrcam,InputSame,OutputSame,Changed,Missing,Replaced,Failure,
                       OutputDifferent,Stale,TooLate,RawCount,FinalCount,NativeCount,FirstDifference,CounterCount};

#if CVR_RUST_VRIK
}
// The snapshot and the cache: rust/crates/cvr-render/src/eager_depth.rs (the packets and counters copied back).
extern "C" {
void* cvr_eager_depth_capture(uint64_t buckets,cvr::stereo::eager_depth::Read,const cvr::stereo::eager_depth::Packet** data,size_t* count);
void cvr_eager_depth_snapshot_free(void*);
void* cvr_eager_depth_cache_new();
void cvr_eager_depth_cache_free(void*);
void cvr_eager_depth_cache_reset(void*,uint32_t* counts);
bool cvr_eager_depth_cache_intercept(void*,const cvr::stereo::eager_depth::Api*,uint32_t frame,uint64_t mainView,uint64_t eyeView,bool beforeMainDraw,
    uint32_t mode,void* context,void* buckets,void* output,uint32_t flags0,uint32_t flags1,uint32_t* counts);
bool cvr_eager_depth_intercept(cvr::stereo::eager_depth::Prepare,cvr::stereo::eager_depth::Gather,cvr::stereo::eager_depth::Filter,
    cvr::stereo::eager_depth::Finalize,void*,void*,void*,uint32_t,uint32_t);
void cvr_eager_depth_begin_run();
void cvr_eager_depth_end_run();
}
namespace cvr::stereo::eager_depth {
class Snapshot {
public:
    std::vector<Packet> packets;
    bool Capture(uint64_t buckets,Read read){
        const Packet* data{};size_t count{};void* captured=cvr_eager_depth_capture(buckets,read,&data,&count);
        if(!captured)return false;
        packets.assign(data,data+count);cvr_eager_depth_snapshot_free(captured);return true;
    }
};
#else
class Snapshot {
public:
    std::vector<Packet> packets;
    bool Capture(uint64_t buckets,Read read){
        std::vector<Packet> first,second;
        if(!Once(buckets,read,first) || !Once(buckets,read,second) || first!=second)return false;
        packets=std::move(first);return true;
    }
private:
    static bool Once(uint64_t buckets,Read read,std::vector<Packet>& output){
        std::array<uint64_t,28> heads{};
        if(!read || !buckets || !read(buckets,heads.data(),sizeof(heads)))return false;
        std::unordered_set<uint64_t> visited;
        for(auto node:heads){
            while(node){
                std::array<uint8_t,40> header{};uint64_t next{},data{};uint32_t count{};
                if((node&7) || visited.size()>=4096 || !visited.insert(node).second || !read(node,header.data(),header.size()))return false;
                std::memcpy(&next,header.data()+8,8);std::memcpy(&data,header.data()+16,8);std::memcpy(&count,header.data()+32,4);
                if(count>MaxPackets-output.size() || (count && (!data || (data&15))))return false;
                const auto offset=output.size();output.resize(offset+count);
                if(count && !read(data,output.data()+offset,size_t(count)*16))return false;
                std::array<uint8_t,40> after{};
                if(!read(node,after.data(),after.size()) || after!=header)return false;
                node=next;
            }
        }
        std::array<uint64_t,28> after{};
        return read(buckets,after.data(),sizeof(after)) && after==heads;
    }
};
#endif

// Native gather consumes 28 heads of {unused,next,data,unused,count} nodes.
// Only the packet array is borrowed, and only for the synchronous native call.
struct OwnedBuckets {
    struct Node {uint64_t unused{},next{};const Packet* data{};uint64_t unused2{};uint32_t count{},padding{};} node;
    std::array<const Node*,28> heads{};
    explicit OwnedBuckets(const std::vector<Packet>& packets){
        node.data=packets.data();node.count=static_cast<uint32_t>(packets.size());
        if(node.count)heads[0]=&node;
    }
};
static_assert(offsetof(OwnedBuckets::Node,count)==32 && sizeof(OwnedBuckets::Node)==40);

#if CVR_RUST_VRIK
class Cache {
public:
    std::array<uint32_t,CounterCount> counts{};
    Cache():rust_(cvr_eager_depth_cache_new()){}
    ~Cache(){cvr_eager_depth_cache_free(rust_);}
    Cache(const Cache&)=delete;
    Cache& operator=(const Cache&)=delete;
    void Reset(){cvr_eager_depth_cache_reset(rust_,counts.data());}
    bool Intercept(const Api& api,uint32_t frame,uint64_t mainView,uint64_t eyeView,bool beforeMainDraw,uint32_t mode,
                   void* context,void* buckets,void* output,uint32_t flags0,uint32_t flags1){
        return cvr_eager_depth_cache_intercept(rust_,&api,frame,mainView,eyeView,beforeMainDraw,mode,context,buckets,output,flags0,flags1,counts.data());
    }
private:
    void* rust_;
};
inline bool Intercept(Prepare prepare,Gather gather,Filter filter,Finalize finalize,void* context,void* buckets,void* output,uint32_t flags0,uint32_t flags1){
    return cvr_eager_depth_intercept(prepare,gather,filter,finalize,context,buckets,output,flags0,flags1);
}
inline void BeginRun(){cvr_eager_depth_begin_run();}
inline void EndRun(){cvr_eager_depth_end_run();}
#else
class Cache {
public:
    std::array<uint32_t,CounterCount> counts{};
    void Reset(){counts={};pending={};}
    bool Intercept(const Api& api,uint32_t frame,uint64_t mainView,uint64_t eyeView,bool beforeMainDraw,uint32_t mode,
                   void* context,void* buckets,void* output,uint32_t flags0,uint32_t flags1){
        if(!api.read || !api.prepare || !api.gather || !api.filter || !api.finalize || !frame || !mainView || !eyeView || mode<1 || mode>2 || flags0 || flags1)return false;
        std::array<uint8_t,32> args{};uint32_t stage{};uint64_t work{},view{},storage{};
        if(!api.read(reinterpret_cast<uintptr_t>(context),args.data(),args.size()))return false;
        std::memcpy(&stage,args.data()+16,4);std::memcpy(&work,args.data(),8);
        if(stage!=3 || args[24] || args[25] || args[26]!=1 || !work || !api.read(work+24,&view,8) ||
            (view!=mainView && view!=eyeView) || !api.read(view+0x2C40,&storage,8) || !storage ||
            reinterpret_cast<uintptr_t>(buckets)!=storage+0x34C0)return false;
        bool nativeCalled=false,executingNative=false,liveFiltered=false,liveFinalized=false;
        try{
            if(view==mainView){
                ++counts[Main];pending={};
                if(!beforeMainDraw){++counts[TooLate];return false;}
                uint64_t peerStorage{};
                if(!api.read(eyeView+0x2C40,&peerStorage,8) || !peerStorage || peerStorage==storage ||
                    !pending.input.Capture(peerStorage+0x34C0,api.read)){++counts[Failure];return false;}
                if(pending.input.packets.empty()){pending={};return false;}
                pending.sorted=pending.input.packets;
                Array own{pending.sorted.data(),static_cast<uint32_t>(pending.sorted.size()),static_cast<uint32_t>(pending.sorted.size())};
                // Filtering dereferences live renderer resources; only sorting is
                // pure packet work and may run before the peer's normal stage.
                executingNative=true;api.finalize(&own,0,0,0);executingNative=false;
                if(own.data!=pending.sorted.data() || own.count!=pending.sorted.size()){++counts[Failure];pending={};return false;}
                pending.frame=frame;pending.view=eyeView;pending.storage=peerStorage;pending.valid=true;
                ++counts[Prepared];counts[RawCount]=static_cast<uint32_t>(pending.input.packets.size());counts[FinalCount]=own.count;
                executingNative=true;api.prepare(context,buckets,output,flags0,flags1);executingNative=false;return true;
            }
            ++counts[Vrcam];
            if(!pending.valid){++counts[Missing];return false;}
            if(pending.frame!=frame || pending.view!=view || pending.storage!=storage){++counts[Stale];pending={};return false;}
            // Preserve resource-dependent filtering at its original time. If it
            // changes the input set, finish with the normal native sort.
            executingNative=true;api.gather(buckets,output);api.filter(output,0,flags0,flags1);executingNative=false;
            liveFiltered=true;
            Array filtered{};std::vector<Packet> current;
            if(!ReadArray(api.read,output,filtered,current)){
                ++counts[Failure];executingNative=true;api.finalize(output,0,0,0);executingNative=false;pending={};return true;
            }
            const bool same=current==pending.input.packets;
            if(same)++counts[InputSame];else ++counts[Changed];
            if(same && mode==2){
                OwnedBuckets owned(pending.sorted);executingNative=true;api.gather(owned.heads.data(),output);executingNative=false;
            }else if(filtered.count){executingNative=true;api.finalize(output,0,0,0);executingNative=false;}
            nativeCalled=liveFinalized=true;
            if(!same){pending={};return true;}
            Array actual{};std::vector<Packet> result;
            bool valid=api.read(reinterpret_cast<uintptr_t>(output),&actual,sizeof(actual)) && actual.count<=actual.capacity && actual.count<=MaxPackets;
            if(valid){result.resize(actual.count);valid=!actual.count || api.read(reinterpret_cast<uintptr_t>(actual.data),result.data(),result.size()*16);}
            counts[NativeCount]=valid?actual.count:0;counts[FirstDifference]=UINT32_MAX;
            if(valid && result==pending.sorted){++counts[mode==2?Replaced:OutputSame];}
            else{
                ++counts[OutputDifferent];
                for(size_t i=0;i<(std::min)(result.size(),pending.sorted.size());++i)
                    if(result[i]!=pending.sorted[i]){counts[FirstDifference]=static_cast<uint32_t>(i);break;}
                if(mode==2){
                    OwnedBuckets owned(current);executingNative=true;api.gather(owned.heads.data(),output);
                    if(filtered.count)api.finalize(output,0,0,0);executingNative=false;
                }
            }
            pending={};return true;
        }catch(...){
            ++counts[Failure];pending={};if(executingNative)throw;
            if(liveFiltered && !liveFinalized){api.finalize(output,0,0,0);return true;}
            return nativeCalled;
        }
    }
private:
    static bool ReadArray(Read read,void* output,Array& header,std::vector<Packet>& packets){
        if(!read(reinterpret_cast<uintptr_t>(output),&header,sizeof(header)) || header.count>header.capacity || header.count>MaxPackets)return false;
        packets.resize(header.count);
        return !header.count || read(reinterpret_cast<uintptr_t>(header.data),packets.data(),packets.size()*16);
    }
    struct Pending {bool valid{};uint32_t frame{};uint64_t view{},storage{};Snapshot input;std::vector<Packet> sorted;} pending;
};
bool Intercept(Prepare,Gather,Filter,Finalize,void*,void*,void*,uint32_t,uint32_t);
void BeginRun();
void EndRun();
#endif
}

extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_EagerDepthMode,CyberpunkVR_EagerDepthCompatible,CyberpunkVR_EagerDepthVerified;
__declspec(dllexport) extern uint32_t CyberpunkVR_EagerDepthStats[cvr::stereo::eager_depth::CounterCount];
}
