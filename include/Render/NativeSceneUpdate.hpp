#pragma once
#include "Render/NativeJobLease.hpp"
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <memory>
#include <set>
#include <span>
#include <vector>

namespace cvr::stereo::scene_update {
inline constexpr uint32_t MaxProducers=4,MaxQueues=128,MaxScenes=4096,MaxCommands=16384;
using Read=bool(*)(uint64_t,void*,size_t);
enum class Kind:uint32_t {General,Pre,Membership,Post,Opaque};
enum class Failure:uint32_t {None,Arguments,Limit,Group,Payload,Changed,Binding,Inventory,Overflow,Scope};
struct Identity {uint64_t nonce{},manager{},caller{};uint32_t thread{},reserved{};};
struct QueueBinding {
    Kind kind{};uint32_t ordinal{};
    uint64_t caller{},callback{},payload{},prerequisite{},completion{};
};
struct SceneInput {
    uint64_t scene{},pool{};uint32_t queue{},updates{};
    std::array<uint32_t,16> operations{};
};
struct CommandInput {
    uint64_t page{},command{},vtable{},execute{},destroy{};
    uint32_t queue{},ordinal{},offset{},bytes{};
};
struct MutationEntry {
    uint64_t pool{},indices{},caller{};uint32_t queue{},thread{};
};
#if CVR_RUST_VRIK
// The receipt: rust/crates/cvr-render/src/scene_update_receipt.rs (tools/single_pass_tests/scene_update_test runs its
// cases against it in its Rust variant). A queue's lease is Rust's.
struct QueueEntry {
    QueueBinding binding;
    std::array<uint8_t,56> prefix{};
    bool entered{},returned{},inventory{},cancelled{};
    void* lease{};
};
static_assert(sizeof(Identity)==32 && sizeof(QueueBinding)==48 && sizeof(SceneInput)==88 && sizeof(CommandInput)==56 &&
    sizeof(MutationEntry)==32 && sizeof(QueueEntry)==120,"mirrored by scene_update_receipt.rs");
struct RustReceipt;
extern "C" {
RustReceipt* cvr_sur_new(const Identity*,const JobLeaseApi*);
void cvr_sur_free(RustReceipt*);
bool cvr_sur_register(RustReceipt*,const QueueBinding*,Read,uint32_t*);
bool cvr_sur_enter(RustReceipt*,uint32_t,Kind,uint64_t,uint64_t,Read);
bool cvr_sur_inventory(RustReceipt*,uint32_t,uint64_t,uint64_t,uint32_t,Read);
void cvr_sur_mutation(RustReceipt*,uint32_t,uint64_t,uint64_t,uint64_t,uint32_t);
uint32_t cvr_sur_step(RustReceipt*,uint32_t,uint32_t);
const void* cvr_sur_rows(const RustReceipt*,uint32_t,size_t*);
const Identity* cvr_sur_identity(const RustReceipt*);
}
class Receipt {
public:
    Receipt(Identity identity,JobLeaseApi api):rust_(cvr_sur_new(&identity,&api)){}
    ~Receipt(){cvr_sur_free(rust_);}
    Receipt(const Receipt&)=delete;
    Receipt& operator=(const Receipt&)=delete;
    const Identity& Binding()const{return *cvr_sur_identity(rust_);}
    Failure Rejection()const{return Failure(cvr_sur_step(rust_,0,0));}
    void Reject(Failure failure){cvr_sur_step(rust_,1,uint32_t(failure));}
    bool Register(QueueBinding binding,Read read,uint32_t& ordinal){return cvr_sur_register(rust_,&binding,read,&ordinal);}
    bool Enter(uint32_t ordinal,Kind kind,uint64_t payload,uint64_t completion,Read read){return cvr_sur_enter(rust_,ordinal,kind,payload,completion,read);}
    void Returned(uint32_t ordinal){cvr_sur_step(rust_,2,ordinal);}
    void CloseProducer(){cvr_sur_step(rust_,3,0);}
    bool ProducerClosed()const{return cvr_sur_step(rust_,4,0)!=0;}
    void CancelProducer(){cvr_sur_step(rust_,5,0);}
    bool ProducerCancelled()const{return cvr_sur_step(rust_,6,0)!=0;}
    void CancelCallback(uint32_t ordinal){cvr_sur_step(rust_,7,ordinal);}
    bool GroupsClosed()const{return cvr_sur_step(rust_,8,0)!=0;}
    uint32_t OpaqueQueues()const{return cvr_sur_step(rust_,9,0);}
    bool Inventory(uint32_t ordinal,uint64_t manager,uint64_t chain,uint32_t lane,Read read){return cvr_sur_inventory(rust_,ordinal,manager,chain,lane,read);}
    void Mutation(uint32_t ordinal,uint64_t pool,uint64_t indices,uint64_t caller,uint32_t thread){cvr_sur_mutation(rust_,ordinal,pool,indices,caller,thread);}
    std::span<const QueueEntry> Queues()const{return Rows<QueueEntry>(0);}
    std::span<const SceneInput> Scenes()const{return Rows<SceneInput>(1);}
    std::span<const CommandInput> Commands()const{return Rows<CommandInput>(2);}
    std::span<const MutationEntry> Mutations()const{return Rows<MutationEntry>(3);}
private:
    template<class T> std::span<const T> Rows(uint32_t which)const{
        size_t count{};const auto* rows=static_cast<const T*>(cvr_sur_rows(rust_,which,&count));return {rows,count};
    }
    RustReceipt* rust_;
};

#else
struct QueueEntry {
    QueueBinding binding;
    std::array<uint8_t,56> prefix{};
    bool entered{},returned{},inventory{},cancelled{};
    std::unique_ptr<NativeJobLease> lease;
};

// Observational source receipt. Held groups prove only this captured native
// aggregate's closure. Unknown descendant queues and virtual command semantics
// stay explicit; neither closure nor inventory is cull-sharing permission.
class Receipt {
public:
    Receipt(Identity identity,JobLeaseApi api):identity_(identity),api_(api){queues_.reserve(MaxQueues);}
    Receipt(const Receipt&)=delete;
    Receipt& operator=(const Receipt&)=delete;
    const Identity& Binding()const{return identity_;}
    Failure Rejection()const{return failure_;}
    void Reject(Failure failure){if(failure_==Failure::None)failure_=failure;}
    bool Register(QueueBinding binding,Read read,uint32_t& ordinal)try{
        ordinal=UINT32_MAX;
        if(((producerClosed_ || producerCancelled_) && binding.kind!=Kind::Opaque) || uint32_t(binding.kind)>uint32_t(Kind::Opaque) ||
            !identity_.nonce || !identity_.manager || !read || !binding.payload || !binding.callback){Reject(Failure::Arguments);return false;}
        if(queues_.size()>=MaxQueues){Reject(Failure::Limit);return false;}
        for(const auto& row:queues_)if(!row.entered && row.binding.payload==binding.payload){Reject(Failure::Binding);return false;}
        if((binding.prerequisite && !Group(binding.prerequisite,read)) || !Group(binding.completion,read)){Reject(Failure::Group);return false;}
        QueueEntry row;row.binding=binding;row.binding.ordinal=static_cast<uint32_t>(queues_.size());
        struct ScenesRollback {std::vector<SceneInput>& rows;size_t size;bool committed{};~ScenesRollback(){if(!committed)rows.resize(size);}} rollback{scenes_,scenes_.size()};
        if(binding.kind!=Kind::Opaque){
            const auto bytes=binding.kind==Kind::Membership?56u:16u;
            if(!Memory(read,binding.payload,row.prefix.data(),bytes) || Field<uint64_t>(row.prefix,0)!=identity_.manager){Reject(Failure::Payload);return false;}
            if(binding.kind==Kind::Membership && !CaptureScenes(row,read))return false;
        }
        row.lease=std::make_unique<NativeJobLease>(api_);
        // 14299C permits a null prerequisite. Retain only the actual terminal
        // in that case; the binding keeps the original null operand unchanged.
        if(!row.lease->Capture(reinterpret_cast<void*>(uintptr_t(binding.prerequisite?binding.prerequisite:binding.completion)),
            reinterpret_cast<void*>(uintptr_t(binding.completion)))){Reject(Failure::Group);return false;}
        ordinal=row.binding.ordinal;queues_.push_back(std::move(row));rollback.committed=true;return true;
    }catch(...){Reject(Failure::Overflow);return false;}
    bool Enter(uint32_t ordinal,Kind kind,uint64_t payload,uint64_t completion,Read read){
        if(ordinal>=queues_.size()){Reject(Failure::Binding);return false;}
        auto& row=queues_[ordinal];
        if(row.entered || row.binding.kind!=kind || row.binding.payload!=payload || row.binding.completion!=completion || kind==Kind::Opaque){Reject(Failure::Binding);return false;}
        std::array<uint8_t,56> now{};const auto bytes=kind==Kind::Membership?56u:16u;
        if(!Memory(read,payload,now.data(),bytes) || std::memcmp(now.data(),row.prefix.data(),bytes)){Reject(Failure::Changed);return false;}
        row.entered=true;return true;
    }
    void Returned(uint32_t ordinal){
        if(ordinal>=queues_.size() || !queues_[ordinal].entered || queues_[ordinal].returned || queues_[ordinal].cancelled){Reject(Failure::Binding);return;}
        queues_[ordinal].returned=true;
    }
    void CloseProducer(){if(producerClosed_)Reject(Failure::Scope);producerClosed_=true;}
    bool ProducerClosed()const{return producerClosed_;}
    void CancelProducer(){Reject(Failure::Scope);producerCancelled_=true;}
    bool ProducerCancelled()const{return producerCancelled_;}
    void CancelCallback(uint32_t ordinal){
        Reject(Failure::Scope);
        if(ordinal<queues_.size() && queues_[ordinal].entered && !queues_[ordinal].returned)queues_[ordinal].cancelled=true;
    }
    bool GroupsClosed()const{
        // Cancellation is distinct from normal return. It never releases held
        // native groups while any actual pending counter remains nonzero.
        if(!producerClosed_ && !producerCancelled_)return false;
        for(const auto& row:queues_){
            if(row.binding.kind!=Kind::Opaque && !row.returned && !row.cancelled && !(producerCancelled_ && !row.entered))return false;
            if(!row.lease || !row.lease->Complete())return false;
        }
        return true;
    }
    uint32_t OpaqueQueues()const{
        return static_cast<uint32_t>(std::count_if(queues_.begin(),queues_.end(),[](const auto& row){return row.binding.kind==Kind::Opaque;}));
    }
    // Generic runners own these detached command pages until native execution
    // begins. This captures input vtables, not actual execute-call observations.
    bool Inventory(uint32_t ordinal,uint64_t manager,uint64_t chain,uint32_t lane,Read read)try{
        if(ordinal>=queues_.size()){Reject(Failure::Binding);return false;}
        auto& row=queues_[ordinal];const auto kind=row.binding.kind;
        if(!row.entered || row.returned || row.inventory || manager!=identity_.manager ||
            chain!=Field<uint64_t>(row.prefix,8) ||
            (kind!=Kind::General && kind!=Kind::Pre && kind!=Kind::Post) ||
            (kind==Kind::Pre && lane!=1) || (kind==Kind::Post && lane!=0)){Reject(Failure::Binding);return false;}
        const uint32_t capacity=kind==Kind::General?0x4000:0x800;
        std::set<uint64_t> seen;std::vector<CommandInput> pending;
        for(auto page=chain;page;){
            if(seen.size()>=4096 || !seen.insert(page).second){Reject(Failure::Inventory);return false;}
            uint32_t count{};uint64_t next{};
            if(!MemoryAt(read,page,capacity,&count,4) || !MemoryAt(read,page,capacity+8,&next,8) || count<14 || count>capacity){Reject(Failure::Inventory);return false;}
            for(uint32_t offset=14;offset<count;){int16_t bytes{};uint64_t vtable{},execute{},destroy{};
                if(!MemoryAt(read,page,offset,&bytes,2) || bytes<10 || uint32_t(bytes)>count-offset ||
                    !MemoryAt(read,page,offset+2,&vtable,8) || !vtable ||
                    !Memory(read,vtable,&destroy,8) || !MemoryAt(read,vtable,8,&execute,8) || !execute || !destroy ||
                    commands_.size()+pending.size()>=MaxCommands){Reject(Failure::Inventory);return false;}
                pending.push_back({page,page+offset+2,vtable,execute,destroy,ordinal,static_cast<uint32_t>(pending.size()),offset,uint32_t(bytes)});
                offset+=uint32_t(bytes);
            }
            page=next;
        }
        commands_.insert(commands_.end(),pending.begin(),pending.end());row.inventory=true;return true;
    }catch(...){Reject(Failure::Overflow);return false;}
    void Mutation(uint32_t ordinal,uint64_t pool,uint64_t indices,uint64_t caller,uint32_t thread)try{
        if(ordinal>=queues_.size() || !queues_[ordinal].entered || queues_[ordinal].returned || queues_[ordinal].cancelled || !pool){Reject(Failure::Binding);return;}
        if(mutations_.size()>=MaxCommands){Reject(Failure::Limit);return;}
        mutations_.push_back({pool,indices,caller,ordinal,thread});
    }catch(...){Reject(Failure::Overflow);}
    std::span<const QueueEntry> Queues()const{return queues_;}
    std::span<const SceneInput> Scenes()const{return scenes_;}
    std::span<const CommandInput> Commands()const{return commands_;}
    std::span<const MutationEntry> Mutations()const{return mutations_;}
private:
    template<class T,size_t N>static T Field(const std::array<uint8_t,N>& bytes,size_t offset){T value{};std::memcpy(&value,bytes.data()+offset,sizeof(value));return value;}
    static bool Memory(Read read,uint64_t address,void* out,size_t bytes){return read && address && address<=UINT64_MAX-bytes && read(address,out,bytes);}
    static bool MemoryAt(Read read,uint64_t base,uint64_t offset,void* out,size_t bytes){return base && base<=UINT64_MAX-offset && Memory(read,base+offset,out,bytes);}
    static bool Group(uint64_t address,Read read){
        int32_t values[2]{};return address && !(address&7) && address<=UINT64_MAX-0x20 &&
            Memory(read,address+0x18,values,8) && values[0]>=0 && values[1]>0;
    }
    bool CaptureScenes(const QueueEntry& row,Read read){
        const auto slots=Field<uint64_t>(row.prefix,8),nodes=Field<uint64_t>(row.prefix,24);
        const auto size=Field<uint32_t>(row.prefix,16),slotCount=Field<uint32_t>(row.prefix,20),capacity=Field<uint32_t>(row.prefix,32),stride=Field<uint32_t>(row.prefix,36);
        if(size>capacity || capacity>MaxScenes || slotCount>MaxScenes || stride>4096 ||
            (size && (!slots || !nodes || !slotCount || stride<32))){Reject(Failure::Payload);return false;}
        if(!size)return true;
        std::vector<uint32_t> heads(slotCount);std::vector<bool> seen(capacity);std::vector<SceneInput> pending;
        if(!Memory(read,slots,heads.data(),heads.size()*4)){Reject(Failure::Payload);return false;}
        uint32_t updates{};
        for(auto index:heads)while(index!=UINT32_MAX){
            if(index>=capacity || seen[index] || pending.size()>=size || nodes>UINT64_MAX-uint64_t(index)*stride){Reject(Failure::Payload);return false;}
            seen[index]=true;std::array<uint8_t,32> node{};
            if(!Memory(read,nodes+uint64_t(index)*stride,node.data(),node.size())){Reject(Failure::Payload);return false;}
            const auto scene=Field<uint64_t>(node,8),data=Field<uint64_t>(node,16);
            const auto count=Field<uint32_t>(node,28),limit=Field<uint32_t>(node,24);
            if(!scene || scene>UINT64_MAX-0x7E0F0 || count>limit || count>MaxCommands-updates || (count && !data)){Reject(Failure::Payload);return false;}
            SceneInput input{scene,scene+0x7E0F0,row.binding.ordinal,count,{}};
            for(uint32_t i=0;i<count;++i){std::array<uint8_t,16> update{};
                if(data>UINT64_MAX-uint64_t(i)*16 || !Memory(read,data+uint64_t(i)*16,update.data(),update.size())){Reject(Failure::Payload);return false;}
                ++input.operations[update[12]&15];
            }
            updates+=count;pending.push_back(input);index=Field<uint32_t>(node,0);
        }
        if(pending.size()!=size || scenes_.size()+pending.size()>MaxScenes){Reject(Failure::Payload);return false;}
        scenes_.insert(scenes_.end(),pending.begin(),pending.end());return true;
    }
    Identity identity_;JobLeaseApi api_;Failure failure_{};bool producerClosed_{},producerCancelled_{};
    std::vector<QueueEntry> queues_;std::vector<SceneInput> scenes_;
    std::vector<CommandInput> commands_;std::vector<MutationEntry> mutations_;
};

#endif
// Full native input/output frame saved by the ABI-preserving thunks.
struct ThunkFrame {
    uint64_t rax{},rcx{},rdx{},r8{},r9{},r10{},r11{},flags{};
    std::array<uint8_t,96> xmm{};
    uint64_t caller{},stack{},arg5{};
};
static_assert(sizeof(ThunkFrame)==0xB8);
void PrepareCodeGuards() noexcept;
void Poll() noexcept;
// Wire from the existing guarded 1E558C observer only. This journals captured
// callback/ancestor provenance, not successful writes, mutation exclusion or
// proxy permission; opaque nested callbacks are not promoted to direct writes.
void ObserveBoundsWrite(uint64_t pool,uint64_t indices,uint64_t caller) noexcept;
}
