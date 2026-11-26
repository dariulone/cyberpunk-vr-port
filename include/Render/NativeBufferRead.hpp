#pragma once
#include "Render/NativeIaBinding.hpp"

namespace cvr::stereo::buffer_read {
struct Pool {
    uint64_t entry{},owner{};
    uint32_t allocation{};
    bool operator==(const Pool&)const=default;
};
struct Proof {
    uint64_t receipt{},epoch{},recording{},generation{},registry{},directQueue{};
    Pool pool;
    uint64_t comOffset{},copyBytes{};
    bool readRegistered{};
    bool operator==(const Proof&)const=default;
};
#if CVR_RUST_VRIK
namespace detail {
// The rules: rust/crates/cvr-render/src/buffer_read.rs (tools/single_pass_tests/buffer_read_state_test runs its cases
// against them in its Rust variant). History and RecordingState keep their members here, mirrored there.
struct History;struct RecordingState;
extern "C" {
bool cvr_brd_valid_range(const geometry::NativeBuffer*,uint64_t,uint64_t,uint64_t,uint64_t);
bool cvr_brd_read_pool_range(geometry::MemoryReader,void*,uint64_t,const geometry::NativeBuffer*,uint64_t,uint64_t,uint64_t,uint64_t,
    Pool*,uint32_t*);
bool cvr_brd_read_pool(geometry::MemoryReader,void*,uint64_t,const geometry::NativeBuffer*,uint64_t,uint64_t,Pool*,uint32_t*);
bool cvr_brd_proof(uint32_t,const Proof*,const Proof*,uint64_t,uint64_t);
bool cvr_brd_initial_state(uint32_t);
uint64_t cvr_brd_history(History*,uint32_t,uint64_t,bool);
bool cvr_brd_recording(RecordingState*,uint32_t);
}
inline bool SameRangeObservation(const Proof& a,const Proof& b){return cvr_brd_proof(0,&a,&b,0,0);}
inline bool RegisteredRange(const Proof& proof,uint64_t comOffset,uint64_t copyBytes){return cvr_brd_proof(1,&proof,nullptr,comOffset,copyBytes);}
// A receipt proves observed state history, not immutable bytes. Every allowed
// uploader write changes the epoch, invalidating all earlier snapshot tokens.
struct History {
    enum class Stage:uint8_t {Recorded,Submitting,Submitted};
    struct Producer {uint64_t generation{};Stage stage{};};
    static constexpr uint32_t Capacity=128;
    uint64_t epoch=1,producer{};
    bool valid=true,uploaded{},pending{};
    std::array<Producer,Capacity> producers{};
    uint32_t count{};
    Producer* Find(uint64_t recording){const auto at=cvr_brd_history(this,0,recording,false);return at?&producers[at-1]:nullptr;}
    void Refresh(){cvr_brd_history(this,1,0,false);}
    void Invalidate(){cvr_brd_history(this,2,0,false);}
    void Written(uint64_t recording){cvr_brd_history(this,3,recording,false);}
    void BeforeSubmit(uint64_t recording,bool queueMatches){cvr_brd_history(this,4,recording,queueMatches);}
    void Submitted(uint64_t recording,bool queueMatches){cvr_brd_history(this,5,recording,queueMatches);}
    void Abandon(uint64_t recording){cvr_brd_history(this,6,recording,false);}
    bool Matches(uint64_t saved)const{return cvr_brd_history(const_cast<History*>(this),7,saved,false)!=0;}
};
struct RecordingState {
    bool valid=true,pending{},submitted{};
    bool BeforeSubmit(){return cvr_brd_recording(this,0);}
    bool Submitted(){return cvr_brd_recording(this,1);}
};
static_assert(sizeof(History)==2080 && sizeof(RecordingState)==3 && sizeof(Proof)==96 && sizeof(geometry::NativeBuffer)==48,
    "mirrored by buffer_read.rs");
inline bool InitialState(uint32_t state){return cvr_brd_initial_state(state);}
// Mathematical bounds only; live ownership/occupancy and history are separate.
// Never expand the native logical allocation to a padded CBV or whole pool unit.
inline bool ValidRange(const geometry::NativeBuffer& source,uint64_t resourceGpu,uint64_t resourceBytes,
                       uint64_t comOffset,uint64_t copyBytes){
    return cvr_brd_valid_range(&source,resourceGpu,resourceBytes,comOffset,copyBytes);
}
// The general path lowers the old292-byte minimum ONLY for an explicit exact
// range that passes native logical, 512-byte slot and COM buffer bounds.
inline bool ReadPoolRange(geometry::MemoryReader reader,void* user,uint64_t game,
                     const geometry::NativeBuffer& source,uint64_t resourceGpu,uint64_t resourceBytes,
                     uint64_t comOffset,uint64_t copyBytes,Pool& out,uint32_t* reason=nullptr){
    return cvr_brd_read_pool_range(reader,user,game,&source,resourceGpu,resourceBytes,comOffset,copyBytes,&out,reason);
}
// Original material path retains its292-byte admission minimum.
inline bool ReadPool(geometry::MemoryReader reader,void* user,uint64_t game,
                    const geometry::NativeBuffer& source,uint64_t resourceGpu,uint64_t resourceBytes,Pool& out,uint32_t* reason=nullptr){
    return cvr_brd_read_pool(reader,user,game,&source,resourceGpu,resourceBytes,&out,reason);
}
}
#else
namespace detail {
inline bool SameRangeObservation(const Proof& a,const Proof& b){
    return a.receipt && a.epoch && a.recording && a.generation && a.registry && a.directQueue && a.copyBytes &&
        a.receipt==b.receipt && a.epoch==b.epoch && a.recording==b.recording && a.generation==b.generation &&
        a.registry==b.registry && a.directQueue==b.directQueue && a.pool==b.pool &&
        a.comOffset==b.comOffset && a.copyBytes==b.copyBytes;
}
inline bool RegisteredRange(const Proof& proof,uint64_t comOffset,uint64_t copyBytes){
    return proof.readRegistered && copyBytes && proof.comOffset==comOffset && proof.copyBytes==copyBytes;
}
// A receipt proves observed state history, not immutable bytes. Every allowed
// uploader write changes the epoch, invalidating all earlier snapshot tokens.
struct History {
    enum class Stage:uint8_t {Recorded,Submitting,Submitted};
    struct Producer {uint64_t generation{};Stage stage{};};
    static constexpr uint32_t Capacity=128;
    uint64_t epoch=1,producer{};
    bool valid=true,uploaded{},pending{};
    std::array<Producer,Capacity> producers{};
    uint32_t count{};
    Producer* Find(uint64_t recording){
        for(uint32_t i=0;i<count;++i)if(producers[i].generation==recording)return &producers[i];
        return nullptr;
    }
    void Refresh(){
        pending=false;uploaded=count!=0;
        for(uint32_t i=0;i<count;++i){
            pending|=producers[i].stage==Stage::Submitting;
            uploaded&=producers[i].stage==Stage::Submitted;
        }
    }
    void Invalidate(){valid=false;}
    void Written(uint64_t recording){
        if(!valid || !recording || epoch==UINT64_MAX){valid=false;return;}
        auto* writer=Find(recording);
        if(writer && writer->stage!=Stage::Recorded){valid=false;return;}
        // A completed cohort can be retired. RecordingState still rejects its
        // replay, and callbacks for an unknown producer always fail closed.
        if(uploaded){count=0;writer=nullptr;}
        if(!writer){
            if(count==Capacity){valid=false;return;}
            producers[count++]={recording,Stage::Recorded};
        }
        ++epoch;producer=recording;Refresh();
    }
    void BeforeSubmit(uint64_t recording,bool queueMatches){
        auto* writer=Find(recording);
        if(!valid || !recording || !writer || !queueMatches || writer->stage!=Stage::Recorded){valid=false;return;}
        writer->stage=Stage::Submitting;Refresh();
    }
    void Submitted(uint64_t recording,bool queueMatches){
        auto* writer=Find(recording);
        if(!valid || !recording || !writer || !queueMatches || writer->stage!=Stage::Submitting){valid=false;return;}
        writer->stage=Stage::Submitted;Refresh();
    }
    void Abandon(uint64_t recording){
        const auto* writer=Find(recording);if(writer && writer->stage!=Stage::Submitted)valid=false;
    }
    bool Matches(uint64_t saved)const{return valid && uploaded && !pending && saved && epoch==saved;}
};
struct RecordingState {
    bool valid=true,pending{},submitted{};
    bool BeforeSubmit(){if(!valid || pending || submitted){valid=false;return false;}pending=true;return true;}
    bool Submitted(){if(!valid || !pending || submitted){valid=false;return false;}pending=false;submitted=true;return true;}
};
inline bool InitialState(uint32_t state){
    return state==D3D12_RESOURCE_STATE_COMMON || state==D3D12_RESOURCE_STATE_GENERIC_READ;
}
// Mathematical bounds only; live ownership/occupancy and history are separate.
// Never expand the native logical allocation to a padded CBV or whole pool unit.
inline bool ValidRange(const geometry::NativeBuffer& source,uint64_t resourceGpu,uint64_t resourceBytes,
                       uint64_t comOffset,uint64_t copyBytes){
    if(source.kind!=1 || !source.resource || !source.gpu || !source.bytes || source.bytes>512 || !resourceGpu ||
        resourceBytes<512 || resourceBytes>512*1024 || resourceBytes%512 || resourceGpu>UINT64_MAX-resourceBytes ||
        source.gpu<resourceGpu || !copyBytes || comOffset>resourceBytes || copyBytes>resourceBytes-comOffset)return false;
    const auto nativeOffset=source.gpu-resourceGpu;
    if(nativeOffset%512 || nativeOffset>resourceBytes || source.bytes>resourceBytes-nativeOffset ||
        comOffset<nativeOffset || comOffset-nativeOffset>source.bytes || copyBytes>source.bytes-(comOffset-nativeOffset))return false;
    return comOffset-nativeOffset<512 && copyBytes<=512-(comOffset-nativeOffset);
}
// The general path lowers the old292-byte minimum ONLY for an explicit exact
// range that passes native logical, 512-byte slot and COM buffer bounds.
inline bool ReadPoolRange(geometry::MemoryReader reader,void* user,uint64_t game,
                     const geometry::NativeBuffer& source,uint64_t resourceGpu,uint64_t resourceBytes,
                     uint64_t comOffset,uint64_t copyBytes,Pool& out,uint32_t* reason=nullptr){
    out={};
    auto step=[&](uint32_t value){if(reason)*reason=value;};step(1);
    auto read=[&](uint64_t at,void* value,size_t bytes){
        return reader && at && at<=UINT64_MAX-bytes && reader(user,at,value,bytes);
    };
    if(!game || game>UINT64_MAX-0x3BCDCB0 || !source.registry || !source.resource || source.kind!=1 ||
        !source.handle || source.handle>0x100000 || !ValidRange(source,resourceGpu,resourceBytes,comOffset,copyBytes))return false;
    uint64_t registry{};uint32_t unit{};
    step(2);
    if(!read(game+0x3BCDCA8,&registry,8) || registry!=source.registry ||
        !read(game+0x36F44C8,&unit,4) || unit!=512 || registry>UINT64_MAX-0x14105C1)return false;
    step(3);uint8_t enabled{};if(!read(registry+0x14105C0,&enabled,1) || !enabled)return false;
    const auto offset=0x5C0AF8+uint64_t(source.handle-1)*0xB0;
    if(registry>UINT64_MAX-offset-0x50)return false;
    std::array<uint8_t,0x50> record{};
    step(4);
    if(!read(registry+offset,record.data(),record.size()) || record[6]!=0x16)return false;
    uint64_t gpu{},resource{};uint32_t bytes{},allocation{};
    std::memcpy(&bytes,record.data(),4);std::memcpy(&gpu,record.data()+8,8);
    std::memcpy(&resource,record.data()+16,8);std::memcpy(&allocation,record.data()+0x40,4);
    const uint32_t index=(allocation&65535)>>3,slot=allocation>>16;
    step(5);
    if(bytes!=source.bytes || gpu!=source.gpu || resource!=source.resource || (allocation&7) ||
        slot>=resourceBytes/512 || gpu!=resourceGpu+uint64_t(slot)*512)return false;
    struct Group {uint64_t data;uint32_t capacity,count;};Group group{};
    step(6);
    if(!read(registry+0x1410548,&group,sizeof(group)) || !group.data || !group.count ||
        group.count>group.capacity || group.capacity>8192 || index>=group.count ||
        group.data>UINT64_MAX-uint64_t(index)*144-144)return false;
    Pool next{group.data+uint64_t(index)*144,0,allocation};uint64_t live{},occupied{};
    step(7);
    if(!read(next.entry,&live,8) || live!=source.resource || !read(next.entry+8,&next.owner,8) || !next.owner ||
        !read(next.entry+16+uint64_t(slot/64)*8,&occupied,8) || !(occupied&(uint64_t(1)<<(slot%64))))return false;
    out=next;step(0);return true;
}
// Original material path retains its292-byte admission minimum.
inline bool ReadPool(geometry::MemoryReader reader,void* user,uint64_t game,
                    const geometry::NativeBuffer& source,uint64_t resourceGpu,uint64_t resourceBytes,Pool& out,uint32_t* reason=nullptr){
    out={};if(reason)*reason=1;
    if(source.bytes<292 || source.gpu<resourceGpu)return false;
    return ReadPoolRange(reader,user,game,source,resourceGpu,resourceBytes,source.gpu-resourceGpu,292,out,reason);
}
}
#endif
// Creation ignores private probe resources. All other observations include
// internal commands: writes/barriers to an observed native source still count.
void ResourceCreated(ID3D12Resource*,D3D12_RESOURCE_STATES initialState,bool placed);
void Barriers(ID3D12GraphicsCommandList*,UINT,const D3D12_RESOURCE_BARRIER*);
void Written(ID3D12GraphicsCommandList*,ID3D12Resource* destination);
void WrittenAddress(uint64_t address,uint64_t bytes);
void Reset(ID3D12GraphicsCommandList*);
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void ObserverUnavailable();
bool Capture(ID3D12GraphicsCommandList*,const geometry::BufferLease&,Proof&);
// Read-only complete history/pool/recording preflight. No native registration.
// The returned readRegistered=false proof is not accepted by ValidateRange.
bool PreflightRange(ID3D12GraphicsCommandList*,const geometry::BufferLease&,uint64_t comOffset,uint64_t copyBytes,Proof&);
// If out already contains a preflight observation it must still match exactly;
// changed eligibility is rejected before1F51C4, never silently refreshed.
// Registers native cumulative read use but does NOT pin the native allocation.
bool CaptureRange(ID3D12GraphicsCommandList*,const geometry::BufferLease&,uint64_t comOffset,uint64_t copyBytes,Proof& out);
uint32_t LastFailure();
// Capture failure 200+reason preserves the receipt's first invalidation cause.
// Four details: Transition={list,before,after,flags<<32|subresource};
// Alias={list,beforeResource,afterResource,0}; MissingRecording={list,0,0,0};
// Recording={list,generation,stateBits,0}; Uploader={list,type,registry,queue};
// Origin={list,generation,previousRegistry,previousQueue};
// WriteHistory/BeforeHistory/AfterHistory/Abandon={list,generation,producer,historyBits};
// Before/AfterRecording/Queue={list,generation,actualQueue,expectedQueue};
// AddressWrite={address,bytes,resourceGpu,resourceBytes}.
// State bits are valid=1, pending=2, submitted=4. History bits are valid=1,
// uploaded=2, pending=4. Abandon detail1 is the retired/reset recording generation.
enum class Invalidation:uint32_t {
    Transition=1,Alias,MissingRecording,Recording,Uploader,Origin,WriteHistory,
    BeforeRecording,BeforeQueue,BeforeHistory,AfterRecording,AfterQueue,AfterHistory,
    Abandon,AddressWrite
};
std::array<uint64_t,4> LastFailureDetails();
// Null queue is for the same recording-time copy; submission requires the
// actual queue argument and validates the captured native DIRECT queue.
bool Validate(const geometry::BufferLease&,const Proof&,ID3D12CommandQueue* queue=nullptr);
bool ValidateRange(const geometry::BufferLease&,const Proof&,uint64_t comOffset,uint64_t copyBytes,ID3D12CommandQueue* queue=nullptr);
}
