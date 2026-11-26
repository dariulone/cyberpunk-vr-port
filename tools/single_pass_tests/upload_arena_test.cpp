#include "Render/NativeUploadArena.hpp"
#include <cstdio>
#include <stdexcept>

namespace arena=cvr::stereo::upload_arena;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
struct Memory {
    uint64_t base=0x100000;std::array<uint8_t,0x68> bytes{};bool failed{};
    template<class T>void Set(size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
    static bool Read(void* user,uint64_t address,void* output,size_t count){
        auto& value=*static_cast<Memory*>(user);
        if(value.failed || address!=value.base || count!=value.bytes.size())return false;
        std::memcpy(output,value.bytes.data(),count);return true;
    }
};
static Memory ReadyMemory(){
    Memory value;value.Set<uint64_t>(0,0x200000);value.Set<uint32_t>(0x0C,8);
    value.Set<uint64_t>(0x28,arena::NativeCapacity);value.Set<uint64_t>(0x50,0x300000);value.Set<uint64_t>(0x60,0x100000000);return value;
}
int main()try{
    Require(arena::ReadyRva==0xB82990 && arena::ReadyBytes==249 && arena::ReadyFnv==0xFF20A4D4F912F92Bull,"Ready original guard changed");
    Require(arena::ResetRva==0xB82A8C && arena::ResetBytes==143 && arena::ResetFnv==0x81DB72CACEBD5681ull,"Reset original guard changed");
    Require(arena::DestroyRva==0x5B30DC && arena::DestroyBytes==88 && arena::DestroyFnv==0x17A16E7BEE7AFB4Cull,"Destroy original guard changed");
    auto memory=ReadyMemory();arena::ArenaFields fields;
    Require(arena::detail::ReadArena(Memory::Read,&memory,memory.base,fields) && arena::detail::ReadyFields(fields),"Exact original arena fields rejected");
    Require(fields.ring==memory.base+0x18 && fields.resource==0x300000 && fields.mapped==0x100000000 && fields.cacheCount==8,"Arena fields borrowed unrelated native offsets");
    auto changed=fields;changed.head=256;Require(arena::detail::CurrentFields(fields,changed),"Live absolute head advance rejected");
    changed.tail=128;Require(arena::detail::CurrentFields(fields,changed),"Live retired tail advance rejected");
    changed.tail=512;Require(!arena::detail::CurrentFields(fields,changed),"Retired tail beyond head admitted");
    changed=fields;changed.head=arena::NativeCapacity+1;Require(!arena::detail::CurrentFields(fields,changed),"Over-capacity live interval admitted");
    for(const auto member:{&arena::ArenaFields::arena,&arena::ArenaFields::ring,&arena::ArenaFields::cache,&arena::ArenaFields::capacity,
        &arena::ArenaFields::resource,&arena::ArenaFields::mapped}){
        changed=fields;changed.*member=0;Require(!arena::detail::ReadyFields(changed),"Missing actual ready identity admitted");
        Require(!arena::detail::CurrentFields(fields,changed),"Changed actual current identity admitted");}
    changed=fields;changed.head=1;Require(!arena::detail::ReadyFields(changed),"Existing initialized/allocated arena was adopted as fresh ready");
    changed=fields;changed.cacheCount=4097;Require(!arena::detail::ReadyFields(changed),"Unbounded native cache count admitted");
    changed=fields;changed.mapped=UINT64_MAX-arena::NativeCapacity+1;Require(!arena::detail::ReadyFields(changed),"Mapped range overflow admitted");
    memory.failed=true;Require(!arena::detail::ReadArena(Memory::Read,&memory,memory.base,fields) && !fields.arena,"Failed reader retained an arena identity");
    memory.failed=false;Require(!arena::detail::ReadArena(Memory::Read,&memory,memory.base+1,fields),"Unaligned arena admitted");
    Require(!arena::detail::ReadArena(Memory::Read,&memory,UINT64_MAX-0x60,fields),"Arena field read overflow admitted");
    arena::detail::Scopes scopes;uint64_t reset{};
    auto ready=scopes.Enter(1,memory.base,7,arena::Kind::Ready);
    auto resetting=scopes.Enter(2,memory.base,7,arena::Kind::Reset);
    Require(resetting.parentReady==ready.nonce && scopes.FinishReset(resetting,7,true) && scopes.FinishReady(ready,7,reset) && reset==2,
        "Matching native ready/reset invocation rejected");
    ready=scopes.Enter(3,memory.base,7,arena::Kind::Ready);
    Require(!scopes.FinishReady(ready,7,reset) && !reset,"Ready without an observed nested reset acquired a generation");
    resetting=scopes.Enter(4,memory.base,7,arena::Kind::Reset);
    Require(!resetting.parentReady && scopes.FinishReset(resetting,7,true),"Standalone actual reset observation rejected");
    ready=scopes.Enter(5,memory.base,7,arena::Kind::Ready);
    Require(!scopes.FinishReady(ready,7,reset),"Historical standalone reset certified a later ready call");
    ready=scopes.Enter(6,memory.base,7,arena::Kind::Ready);
    resetting=scopes.Enter(7,memory.base+0x100,7,arena::Kind::Reset);
    Require(!resetting.parentReady && scopes.FinishReset(resetting,7,true) && !scopes.FinishReady(ready,7,reset),"Foreign arena reset certified ready");
    ready=scopes.Enter(8,memory.base,7,arena::Kind::Ready);resetting=scopes.Enter(9,memory.base,7,arena::Kind::Reset);
    Require(!scopes.FinishReset(resetting,8,true),"Foreign thread completed a native reset invocation");
    scopes.Cancel(ready,7);Require(!scopes.FinishReady(ready,7,reset),"Cancelled native ready invocation certified generation");scopes.Clear();
    ready=scopes.Enter(10,memory.base,7,arena::Kind::Ready);resetting=scopes.Enter(11,memory.base,7,arena::Kind::Reset);
    Require(!scopes.FinishReset(resetting,7,false) && !scopes.FinishReady(ready,7,reset),"Failed native reset fields certified generation");
    ready=scopes.Enter(12,memory.base,7,arena::Kind::Ready);resetting=scopes.Enter(13,memory.base,7,arena::Kind::Reset);
    Require(scopes.FinishReset(resetting,7,true),"First nested reset rejected");resetting=scopes.Enter(14,memory.base,7,arena::Kind::Reset);
    Require(scopes.FinishReset(resetting,7,true) && !scopes.FinishReady(ready,7,reset),"Repeated reset in one ready invocation admitted");
    const auto destroying=scopes.Enter(15,memory.base,7,arena::Kind::Destroy);
    Require(scopes.FinishDestroy(destroying,7),"Exact destroy completion rejected");
    for(size_t i=0;i<arena::MaxScopes;++i)Require(scopes.Enter(20+i,memory.base+i*0x100,7,arena::Kind::Ready).nonce,"Bounded invocation stack filled prematurely");
    Require(!scopes.Enter(99,memory.base,7,arena::Kind::Ready).nonce,"Unbounded native invocation depth admitted");
    scopes.Clear();Require(!scopes.Enter(0,memory.base,7,arena::Kind::Ready).nonce && !scopes.Enter(1,0,7,arena::Kind::Ready).nonce,
        "Invalid native invocation identity admitted");
    std::puts("upload_arena_test: OK");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"upload_arena_test: %s\n",error.what());return 1;}
