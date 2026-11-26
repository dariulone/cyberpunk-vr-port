#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/NativeUploadArena.hpp"
#include "Render/NativeUploadReservation.hpp"
#include <Windows.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <mutex>
#if CVR_RUST_VRIK
// Nothing in the Rust build calls the upload arena (its rules are rust/crates/cvr-render/src/native/upload_arena.rs);
// its exports, CyberpunkVR_UploadArenaRows among them, are declared_moved_exports.rs'.
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_UploadArenaEnable{},CyberpunkVR_UploadArenaSeq{};
__declspec(dllexport) uint64_t CyberpunkVR_UploadArenaStats[16]{};
__declspec(dllexport) std::array<std::array<uint64_t,16>,cvr::stereo::upload_arena::MaxRows> CyberpunkVR_UploadArenaRows{};
}
namespace cvr::stereo::upload_arena {
namespace detail {
struct Owner {
    Microsoft::WRL::ComPtr<ID3D12Resource> resource;
    ArenaFields fields;uint64_t generation{};std::atomic<bool> current{};
};
}
namespace {
std::mutex codeMutex,stateMutex;
uint64_t guardedBase{};bool guardAttempted{},guardValid{},exhausted{};
uint64_t nextInvocation{},nextGeneration{};
std::array<uint64_t,16> stats{};
struct Entry {uint64_t arena{},ring{},pendingReady{};std::shared_ptr<detail::Owner> owner;};
std::array<Entry,MaxArenas> entries{};
std::atomic<bool> observed{};
thread_local detail::Scopes scopes;
struct Releases {std::array<std::shared_ptr<detail::Owner>,MaxArenas> owners{};size_t count{};};
bool Enabled()noexcept{return CyberpunkVR_UploadArenaEnable.load(std::memory_order_acquire)!=0;}
uint64_t Base()noexcept{return reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));}
bool Read(void*,uint64_t address,void* value,size_t bytes)noexcept {
    SIZE_T copied{};return address && bytes && address<=UINT64_MAX-bytes &&
        ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),value,bytes,&copied) && copied==bytes;
}
void Publish()noexcept {
    CyberpunkVR_UploadArenaSeq.fetch_add(1,std::memory_order_acq_rel);
    stats[12]=0;for(const auto& entry:entries)stats[12]+=entry.owner && entry.owner->current.load(std::memory_order_acquire);
    std::memcpy(CyberpunkVR_UploadArenaStats,stats.data(),sizeof(CyberpunkVR_UploadArenaStats));
    CyberpunkVR_UploadArenaSeq.fetch_add(1,std::memory_order_release);
}
Entry* Find(uint64_t arena)noexcept {for(auto& entry:entries)if(entry.arena==arena)return &entry;return nullptr;}
Entry* Add(uint64_t arena)noexcept {
    if(auto* found=Find(arena))return found;
    if(!arena || (arena&7) || arena>UINT64_MAX-0x68)return nullptr;
    for(auto& entry:entries)if(!entry.arena){entry.arena=arena;entry.ring=arena+0x18;return &entry;}return nullptr;
}
void Invalidate(Entry& entry,Releases& releases,bool keepPending=false)noexcept {
    upload_reservation::EndGeneration(entry.ring);
    if(entry.owner){entry.owner->current.store(false,std::memory_order_release);++stats[9];
        if(releases.count<releases.owners.size())releases.owners[releases.count++]=std::move(entry.owner);}
    if(!keepPending)entry.pendingReady=0;
}
void Quarantine()noexcept {
    exhausted=true;
    for(auto& entry:entries)if(entry.arena){upload_reservation::EndGeneration(entry.ring);entry.pendingReady=0;
        if(entry.owner)entry.owner->current.store(false,std::memory_order_release);}
}
bool Emit(Kind kind,Status status,const Invocation& call,const ArenaFields& fields={},uint64_t generation=0,uint64_t reset=0,uint64_t pending=0)noexcept {
    stats[0]=uint64_t(status);
    if(stats[13]>=MaxRows){++stats[14];stats[0]=uint64_t(Status::Capacity);Quarantine();Publish();return false;}
    CyberpunkVR_UploadArenaSeq.fetch_add(1,std::memory_order_acq_rel);
    const auto sequence=stats[13]+1;
    CyberpunkVR_UploadArenaRows[size_t(stats[13]++)]={sequence,uint64_t(kind),uint64_t(status),call.nonce,call.arena,
        fields.ring?fields.ring:(call.arena && call.arena<=UINT64_MAX-0x18?call.arena+0x18:0),generation,fields.resource,fields.mapped,
        fields.capacity,fields.head,fields.tail,call.thread,reset,pending,0};
    stats[12]=0;for(const auto& entry:entries)stats[12]+=entry.owner && entry.owner->current.load(std::memory_order_acquire);
    std::memcpy(CyberpunkVR_UploadArenaStats,stats.data(),sizeof(CyberpunkVR_UploadArenaStats));
    CyberpunkVR_UploadArenaSeq.fetch_add(1,std::memory_order_release);
    return true;
}
bool StableFields(uint64_t arena,ArenaFields& fields)noexcept {
    ArenaFields again;
    return detail::ReadArena(Read,nullptr,arena,fields) && detail::ReadArena(Read,nullptr,arena,again) && fields==again;
}
bool RetainResource(uint64_t address,ID3D12Resource*& resource)noexcept {
    resource=nullptr;
    __try{if(!address)return false;auto* value=reinterpret_cast<ID3D12Resource*>(address);value->AddRef();resource=value;return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
bool ResourceFields(ID3D12Resource* resource,uint64_t capacity)noexcept {
    __try{
        if(!resource)return false;const auto desc=resource->GetDesc();D3D12_HEAP_PROPERTIES heap{};D3D12_HEAP_FLAGS flags{};
        return desc.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER && desc.Width==capacity && desc.Flags==D3D12_RESOURCE_FLAG_NONE &&
            SUCCEEDED(resource->GetHeapProperties(&heap,&flags)) && heap.Type==D3D12_HEAP_TYPE_UPLOAD;
    }__except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
Invocation Before(uint64_t arena,Kind kind)noexcept {
    const bool enabled=Enabled();if(!enabled && !observed.load(std::memory_order_acquire))return {};
    const bool compatible=Compatible();Releases releases;std::lock_guard lock(stateMutex);
    auto* entry=Find(arena);
    if(!enabled || !compatible || exhausted){
        if(entry)Invalidate(*entry,releases);
        if(enabled){++stats[8];Emit(kind,compatible?Status::Capacity:Status::GuardMismatch,{0,arena,GetCurrentThreadId(),uint64_t(kind),0});}
        else Publish();return {};
    }
    observed.store(true,std::memory_order_release);entry=Add(arena);
    if(!entry || nextInvocation==UINT64_MAX){++stats[8];Quarantine();Emit(kind,Status::Capacity,{0,arena,GetCurrentThreadId(),uint64_t(kind),0});return {};}
    const auto call=scopes.Enter(++nextInvocation,arena,GetCurrentThreadId(),kind);
    const bool nestedReset=kind==Kind::Reset && call.parentReady && entry->pendingReady==call.parentReady;
    Invalidate(*entry,releases,nestedReset);
    if(!call.nonce){entry->pendingReady=0;++stats[8];Emit(kind,Status::InvalidInvocation,{0,arena,GetCurrentThreadId(),uint64_t(kind),0});return {};}
    if(kind==Kind::Ready){entry->pendingReady=call.nonce;++stats[1];}
    else if(kind==Kind::Reset)++stats[3];else ++stats[5];
    Emit(kind,Status::Observed,call,{},0,0,entry->pendingReady);return call;
}
Status Reject(const Invocation& call,Status status)noexcept {
    Releases releases;std::lock_guard lock(stateMutex);if(auto* entry=Find(call.arena)){
        if(!entry->pendingReady || entry->pendingReady==call.nonce || entry->pendingReady==call.parentReady)Invalidate(*entry,releases);}
    ++stats[8];Emit(Kind(call.kind),status,call);return status;
}
}
uint64_t Snapshot::Arena()const noexcept{return owner_?owner_->fields.arena:0;}
uint64_t Snapshot::Ring()const noexcept{return owner_?owner_->fields.ring:0;}
uint64_t Snapshot::Generation()const noexcept{return owner_?owner_->generation:0;}
uint64_t Snapshot::Capacity()const noexcept{return owner_?owner_->fields.capacity:0;}
uint64_t Snapshot::Mapped()const noexcept{return owner_?owner_->fields.mapped:0;}
ID3D12Resource* Snapshot::Resource()const noexcept{return owner_?owner_->resource.Get():nullptr;}
Snapshot::operator bool()const noexcept{return owner_ && owner_->current.load(std::memory_order_acquire);}
void PrepareCodeGuards(uintptr_t base)noexcept {
    std::lock_guard lock(codeMutex);if(guardAttempted)return;guardAttempted=true;guardedBase=base;
    struct Guard {uint64_t rva,bytes,fnv;};
    constexpr Guard guards[]{{ReadyRva,ReadyBytes,ReadyFnv},{ResetRva,ResetBytes,ResetFnv},{DestroyRva,DestroyBytes,DestroyFnv}};
    std::array<uint8_t,ReadyBytes> bytes{};guardValid=base && base==Base();
    for(const auto& guard:guards)if(!guardValid || base>UINT64_MAX-guard.rva-guard.bytes ||
        !Read(nullptr,base+guard.rva,bytes.data(),size_t(guard.bytes)) || upload_reservation::Fnv(bytes.data(),size_t(guard.bytes))!=guard.fnv){guardValid=false;break;}
}
bool Compatible()noexcept {std::lock_guard lock(codeMutex);return guardValid && guardedBase==Base();}
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
Invocation BeforeReady(uint64_t arena)noexcept{return Before(arena,Kind::Ready);}
#endif
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
Invocation BeforeReset(uint64_t arena)noexcept{return Before(arena,Kind::Reset);}
#endif
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
Invocation BeforeDestroy(uint64_t arena)noexcept{return Before(arena,Kind::Destroy);}
#endif
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
Status AfterReset(const Invocation& call)noexcept {
    if(!call.nonce)return Status::Disabled;
    ArenaFields fields;const bool valid=Enabled() && Compatible() && StableFields(call.arena,fields) && detail::ResetFields(fields);
    if(!scopes.FinishReset(call,GetCurrentThreadId(),valid))return Reject(call,valid?Status::InvalidInvocation:Status::InvalidArena);
    std::lock_guard lock(stateMutex);++stats[4];Emit(Kind::Reset,Status::Observed,call,fields,0,call.nonce,call.parentReady);return Status::Observed;
}
#endif
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
Status AfterReady(const Invocation& call)noexcept {
    if(!call.nonce)return Status::Disabled;
    uint64_t reset{};if(!scopes.FinishReady(call,GetCurrentThreadId(),reset))return Reject(call,Status::MissingReset);
    if(!Enabled() || !Compatible())return Reject(call,Status::GuardMismatch);
    ArenaFields fields,again;
    if(!StableFields(call.arena,fields) || !detail::ReadyFields(fields))return Reject(call,Status::InvalidArena);
    std::shared_ptr<detail::Owner> owner;
    try{owner=std::make_shared<detail::Owner>();}catch(...){return Reject(call,Status::Capacity);}
    ID3D12Resource* retained{};if(!RetainResource(fields.resource,retained))return Reject(call,Status::ResourceMismatch);
    owner->resource.Attach(retained);
    if(!ResourceFields(owner->resource.Get(),fields.capacity) || !StableFields(call.arena,again) || fields!=again)return Reject(call,Status::Changed);
    owner->fields=fields;
    std::lock_guard lock(stateMutex);auto* entry=Find(call.arena);
    if(!Enabled() || exhausted || !entry || entry->pendingReady!=call.nonce || nextGeneration==UINT64_MAX){
        ++stats[8];Emit(Kind::Ready,Status::Changed,call,fields,0,reset);return Status::Changed;}
    owner->generation=++nextGeneration;
    if(!upload_reservation::BeginGeneration(fields.ring,owner->generation,fields.capacity)){
        entry->pendingReady=0;++stats[8];Emit(Kind::Ready,Status::Changed,call,fields,owner->generation,reset);return Status::Changed;}
    owner->current.store(true,std::memory_order_release);entry->owner=owner;entry->pendingReady=0;++stats[2];++stats[7];
    return Emit(Kind::Ready,Status::Captured,call,fields,owner->generation,reset)?Status::Captured:Status::Capacity;
}
#endif
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
Status AfterDestroy(const Invocation& call)noexcept {
    if(!call.nonce)return Status::Disabled;
    if(!scopes.FinishDestroy(call,GetCurrentThreadId()))return Reject(call,Status::InvalidInvocation);
    std::lock_guard lock(stateMutex);++stats[6];Emit(Kind::Destroy,Status::Observed,call);return Status::Observed;
}
#endif
void Cancel(const Invocation& call)noexcept {
    if(!call.nonce)return;scopes.Cancel(call,GetCurrentThreadId());(void)Reject(call,Status::InvalidInvocation);
}
bool QueryCurrent(ID3D12Resource* held,uint64_t arena,uint64_t generation,Snapshot& output)noexcept {
    output={};if(!held || !generation || !Enabled() || !Compatible())return false;
    std::shared_ptr<detail::Owner> owner;
    {std::lock_guard lock(stateMutex);++stats[10];const auto* entry=Find(arena);
        if(entry && !entry->pendingReady && entry->owner && entry->owner->generation==generation && entry->owner->resource.Get()==held &&
            entry->owner->current.load(std::memory_order_acquire) && !exhausted)owner=entry->owner;
        if(!owner){++stats[11];Emit(Kind::Query,Status::ResourceMismatch,{0,arena,GetCurrentThreadId(),uint64_t(Kind::Query),0});return false;}}
    ArenaFields fields;const bool valid=StableFields(arena,fields) && detail::CurrentFields(owner->fields,fields) && ResourceFields(held,fields.capacity);
    {std::lock_guard lock(stateMutex);const auto* entry=Find(arena);
        if(!valid || !Enabled() || exhausted || !entry || entry->pendingReady || entry->owner!=owner || !owner->current.load(std::memory_order_acquire)){
            ++stats[11];Emit(Kind::Query,Status::Changed,{0,arena,GetCurrentThreadId(),uint64_t(Kind::Query),0},fields,generation);return false;}
        output.owner_=owner;Emit(Kind::Query,Status::Captured,{0,arena,GetCurrentThreadId(),uint64_t(Kind::Query),0},fields,generation);}
    return bool(output);
}
#if !CVR_RUST_VRIK   // nothing in the Rust build calls it
bool ValidateCurrent(const Snapshot& snapshot)noexcept {
    if(!snapshot)return false;Snapshot fresh;
    return QueryCurrent(snapshot.Resource(),snapshot.Arena(),snapshot.Generation(),fresh) && fresh.owner_==snapshot.owner_;
}
#endif
void Clear()noexcept {
    Releases releases;{std::lock_guard lock(stateMutex);for(auto& entry:entries){if(entry.arena)Invalidate(entry,releases);entry={};}
        stats.fill(0);exhausted=false;observed.store(false,std::memory_order_release);
        CyberpunkVR_UploadArenaSeq.fetch_add(1,std::memory_order_acq_rel);CyberpunkVR_UploadArenaRows.fill({});
        std::memset(CyberpunkVR_UploadArenaStats,0,sizeof(CyberpunkVR_UploadArenaStats));CyberpunkVR_UploadArenaSeq.fetch_add(1,std::memory_order_release);}
    scopes.Clear();
}
}
#endif
#endif
