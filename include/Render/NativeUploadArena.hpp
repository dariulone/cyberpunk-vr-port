#pragma once
#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>

struct ID3D12Resource;
namespace cvr::stereo::upload_arena {
inline constexpr uint32_t ReadyRva=0xB82990,ReadyBytes=249,ResetRva=0xB82A8C,ResetBytes=143,DestroyRva=0x5B30DC,DestroyBytes=88;
inline constexpr uint64_t ReadyFnv=0xFF20A4D4F912F92Bull,ResetFnv=0x81DB72CACEBD5681ull,DestroyFnv=0x17A16E7BEE7AFB4Cull;
inline constexpr uint64_t NativeCapacity=0xC000000;
inline constexpr size_t MaxArenas=64,MaxRows=4096,MaxScopes=8;
using MemoryReader=bool(*)(void*,uint64_t,void*,size_t);
enum class Kind:uint64_t {Ready=1,Reset,Destroy,Cancel,Query};
enum class Status:uint64_t {Disabled,Observed,Captured,GuardMismatch,InvalidInvocation,InvalidArena,MissingReset,ResourceMismatch,Changed,Capacity,Failure};
struct Invocation {
    uint64_t nonce{},arena{},thread{},kind{},parentReady{};
    bool operator==(const Invocation&)const=default;
};
struct ArenaFields {
    uint64_t arena{},ring{},cache{},head{},tail{},capacity{},resource{},mapped{};uint32_t cacheCount{};
    bool operator==(const ArenaFields&)const=default;
};
namespace detail {
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/native/upload_arena.rs through upload_arena_abi.rs
// (tools/single_pass_tests/upload_arena_test runs its cases against them in its Rust variant).
struct RustScopes;
extern "C" {
bool cvr_ua_read_arena(MemoryReader,void*,uint64_t,ArenaFields*);
bool cvr_ua_fields(uint32_t,const ArenaFields*,const ArenaFields*);
RustScopes* cvr_ua_scopes_new();
void cvr_ua_scopes_free(RustScopes*);
void cvr_ua_scopes_enter(RustScopes*,uint64_t,uint64_t,uint64_t,uint64_t,Invocation*);
bool cvr_ua_scopes_step(RustScopes*,uint32_t,const Invocation*,uint64_t,bool,uint64_t*);
}
#endif
template<class T>T Field(const std::array<uint8_t,0x68>& bytes,size_t offset)noexcept {T value{};std::memcpy(&value,bytes.data()+offset,sizeof(value));return value;}
#if CVR_RUST_VRIK
inline bool ReadArena(MemoryReader read,void* user,uint64_t arena,ArenaFields& output)noexcept {
    static_assert(sizeof(Invocation)==40 && sizeof(ArenaFields)==72,"mirrored by upload_arena.rs");
    return cvr_ua_read_arena(read,user,arena,&output);
}
inline bool ResetFields(const ArenaFields& value)noexcept {return cvr_ua_fields(0,nullptr,&value);}
inline bool ReadyFields(const ArenaFields& value)noexcept {return cvr_ua_fields(1,nullptr,&value);}
inline bool CurrentFields(const ArenaFields& before,const ArenaFields& value)noexcept {return cvr_ua_fields(2,&before,&value);}
#else
inline bool ReadArena(MemoryReader read,void* user,uint64_t arena,ArenaFields& output)noexcept {
    output={};if(!read || !arena || (arena&7) || arena>UINT64_MAX-0x68)return false;
    std::array<uint8_t,0x68> bytes{};if(!read(user,arena,bytes.data(),bytes.size()))return false;
    output={arena,arena+0x18,Field<uint64_t>(bytes,0),Field<uint64_t>(bytes,0x18),Field<uint64_t>(bytes,0x20),
        Field<uint64_t>(bytes,0x28),Field<uint64_t>(bytes,0x50),Field<uint64_t>(bytes,0x60),Field<uint32_t>(bytes,0x0C)};return true;
}
inline bool ResetFields(const ArenaFields& value)noexcept {
    return value.arena && value.arena<=UINT64_MAX-0x68 && value.ring==value.arena+0x18 && !value.head && !value.tail &&
        value.capacity==NativeCapacity && value.cache && value.cacheCount && value.cacheCount<=4096 &&
        value.cache<=UINT64_MAX-uint64_t(value.cacheCount)*64;
}
inline bool ReadyFields(const ArenaFields& value)noexcept {
    return ResetFields(value) && value.resource && value.mapped && value.mapped<=UINT64_MAX-value.capacity;
}
inline bool CurrentFields(const ArenaFields& before,const ArenaFields& value)noexcept {
    return value.arena==before.arena && value.ring==before.ring && value.cache==before.cache && value.cacheCount==before.cacheCount &&
        value.capacity==before.capacity && value.resource==before.resource && value.mapped==before.mapped && value.tail<=value.head &&
        value.head-value.tail<=value.capacity && value.head>=before.head && value.tail>=before.tail;
}
#endif
// The runtime uses this exact TLS stack to pair guarded native before/after
// callbacks. A reset outside the matching ready invocation cannot certify it.
#if CVR_RUST_VRIK
class Scopes {
    RustScopes* rust_;
public:
    Scopes():rust_(cvr_ua_scopes_new()){}
    ~Scopes(){cvr_ua_scopes_free(rust_);}
    Scopes(const Scopes&)=delete;
    Scopes& operator=(const Scopes&)=delete;
    Invocation Enter(uint64_t nonce,uint64_t arena,uint64_t thread,Kind kind)noexcept {
        Invocation call{};cvr_ua_scopes_enter(rust_,nonce,arena,thread,uint64_t(kind),&call);return call;
    }
    bool Top(const Invocation& call,uint64_t thread)const noexcept {return cvr_ua_scopes_step(rust_,0,&call,thread,false,nullptr);}
    bool FinishReset(const Invocation& call,uint64_t thread,bool valid)noexcept {return cvr_ua_scopes_step(rust_,1,&call,thread,valid,nullptr);}
    bool FinishReady(const Invocation& call,uint64_t thread,uint64_t& reset)noexcept {
        return cvr_ua_scopes_step(rust_,2,&call,thread,false,&reset);
    }
    bool FinishDestroy(const Invocation& call,uint64_t thread)noexcept {return cvr_ua_scopes_step(rust_,3,&call,thread,false,nullptr);}
    void Cancel(const Invocation& call,uint64_t thread)noexcept {cvr_ua_scopes_step(rust_,4,&call,thread,false,nullptr);}
    void Poison()noexcept {cvr_ua_scopes_step(rust_,5,nullptr,0,false,nullptr);}
    void Clear()noexcept {cvr_ua_scopes_step(rust_,6,nullptr,0,false,nullptr);}
};
#else
class Scopes {
    struct Entry {Invocation call;uint64_t reset{};bool resetComplete{},poisoned{};};
    std::array<Entry,MaxScopes> entries_{};size_t count_{};
public:
    Invocation Enter(uint64_t nonce,uint64_t arena,uint64_t thread,Kind kind)noexcept {
        if(!nonce || !arena || !thread || count_==entries_.size()){Poison();return {};}
        Invocation call{nonce,arena,thread,uint64_t(kind),0};
        if(kind==Kind::Reset && count_ && entries_[count_-1].call.kind==uint64_t(Kind::Ready) &&
            entries_[count_-1].call.arena==arena && entries_[count_-1].call.thread==thread)call.parentReady=entries_[count_-1].call.nonce;
        entries_[count_++]={call};return call;
    }
    bool Top(const Invocation& call,uint64_t thread)const noexcept {return call.nonce && call.thread==thread && count_ && entries_[count_-1].call==call;}
    bool FinishReset(const Invocation& call,uint64_t thread,bool valid)noexcept {
        if(call.kind!=uint64_t(Kind::Reset) || !Top(call,thread)){Poison();return false;}
        entries_[--count_]={};
        if(call.parentReady){
            if(!count_ || entries_[count_-1].call.nonce!=call.parentReady){Poison();return false;}
            auto& parent=entries_[count_-1];if(parent.reset)parent.poisoned=true;
            parent.reset=call.nonce;parent.resetComplete=valid;
        }return valid;
    }
    bool FinishReady(const Invocation& call,uint64_t thread,uint64_t& reset)noexcept {
        reset=0;if(call.kind!=uint64_t(Kind::Ready) || !Top(call,thread)){Poison();return false;}
        const auto complete=entries_[count_-1];entries_[--count_]={};reset=complete.reset;
        return complete.reset && complete.resetComplete && !complete.poisoned;
    }
    bool FinishDestroy(const Invocation& call,uint64_t thread)noexcept {
        if(call.kind!=uint64_t(Kind::Destroy) || !Top(call,thread)){Poison();return false;}entries_[--count_]={};return true;
    }
    void Cancel(const Invocation& call,uint64_t thread)noexcept {
        if(!call.nonce || call.thread!=thread){Poison();return;}
        for(size_t i=count_;i;--i)if(entries_[i-1].call==call){
            while(count_>=i)entries_[--count_]={};Poison();return;
        }Poison();
    }
    void Poison()noexcept {for(size_t i=0;i<count_;++i)entries_[i].poisoned=true;}
    void Clear()noexcept {entries_.fill({});count_=0;}
};
#endif
struct Owner;
}
class Snapshot {
public:
    uint64_t Arena()const noexcept;uint64_t Ring()const noexcept;uint64_t Generation()const noexcept;
    uint64_t Capacity()const noexcept;uint64_t Mapped()const noexcept;ID3D12Resource* Resource()const noexcept;
    explicit operator bool()const noexcept;
private:
    std::shared_ptr<const detail::Owner> owner_;
    friend bool QueryCurrent(ID3D12Resource*,uint64_t,uint64_t,Snapshot&)noexcept;
    friend bool ValidateCurrent(const Snapshot&)noexcept;
};
// Capture full originals before Root creates/enables the three entry detours.
void PrepareCodeGuards(uintptr_t gameBase)noexcept;
bool Compatible()noexcept;
Invocation BeforeReady(uint64_t arena)noexcept;
Status AfterReady(const Invocation&)noexcept;
Invocation BeforeReset(uint64_t arena)noexcept;
Status AfterReset(const Invocation&)noexcept;
Invocation BeforeDestroy(uint64_t arena)noexcept;
Status AfterDestroy(const Invocation&)noexcept;
void Cancel(const Invocation&)noexcept;
// Actual caller-held SourceCBV resource and already known native arena/current
// generation only. Strong COM ownership is not writer, retirement or copy proof.
bool QueryCurrent(ID3D12Resource* heldSourceResource,uint64_t arena,uint64_t expectedGeneration,Snapshot&)noexcept;
bool ValidateCurrent(const Snapshot&)noexcept;
void Clear()noexcept;
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_UploadArenaEnable,CyberpunkVR_UploadArenaSeq;
// lastStatus,readyBegin,readyEnd,resetBegin,resetEnd,destroyBegin,destroyEnd,
// acceptedGenerations,declined,invalidations,queries,queryRejected,live,rowCount,overflow,reserved.
__declspec(dllexport) extern uint64_t CyberpunkVR_UploadArenaStats[16];
// sequence,kind,status,invocation,arena,ring,generation,resource,mapped,
// capacity,head,tail,thread,resetInvocation,pendingReady,reserved.
__declspec(dllexport) extern std::array<std::array<uint64_t,16>,cvr::stereo::upload_arena::MaxRows> CyberpunkVR_UploadArenaRows;
}
