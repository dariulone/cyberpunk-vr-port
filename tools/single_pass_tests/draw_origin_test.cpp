#include "Render/NativeDrawOrigin.hpp"
#include <cstdio>
#include <stdexcept>
#include <vector>
using namespace cvr::stereo::packets;
void Check(bool value){if(!value)throw std::runtime_error("native draw origin invariant");}
struct Memory {
    std::array<uint8_t,64> draw{};
    std::array<uint8_t,80> batch{};
    std::array<uint64_t,6> flush{};
    std::array<uint64_t,2> packet{};
    DrawEntryFrame entry{};
    const uint64_t base=0x140000000;
    Memory(){
        draw[8]=17;packet[1]=uint64_t(1)<<51;
        entry.rcx=0x7000;entry.rdx=entry.r14=reinterpret_cast<uintptr_t>(draw.data());
        entry.r8=entry.rbx=reinterpret_cast<uintptr_t>(batch.data());entry.rdi=reinterpret_cast<uintptr_t>(flush.data());
        entry.r15=reinterpret_cast<uintptr_t>(packet.data());entry.r12=3;entry.caller=base+0x335496;
        std::memcpy(batch.data()+0x48,&entry.rdx,8);flush[0]=entry.r8;flush[3]=entry.rcx;flush[4]=entry.r15;
    }
    static bool Read(void* context,uint64_t address,void* output,size_t size){
        const auto& self=*static_cast<Memory*>(context);
        const std::array<std::pair<const void*,size_t>,4> ranges{{{self.draw.data(),sizeof(self.draw)},
            {self.batch.data(),sizeof(self.batch)},{self.flush.data(),sizeof(self.flush)},{self.packet.data(),sizeof(self.packet)}}};
        for(const auto& [p,n]:ranges){const auto begin=reinterpret_cast<uintptr_t>(p);
            if(address>=begin && address-begin<=n && size<=n-(address-begin)){std::memcpy(output,reinterpret_cast<const void*>(address),size);return true;}}
        return false;
    }
};
int main()try{
    Memory memory;const auto origin=OriginFromEntry(memory.entry,memory.base);std::array<uint8_t,64> output{};
    Check(origin.valid && ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output) && output==memory.draw);
    for(auto field:{&DrawEntryFrame::caller,&DrawEntryFrame::rbx,&DrawEntryFrame::r14,&DrawEntryFrame::r12}){
        auto bad=memory.entry;bad.*field=0;Check(!OriginFromEntry(bad,memory.base).valid);}
    Check(!OriginFromEntry(memory.entry,UINT64_MAX).valid);
    Check(!ResolveOrigin(origin,0x7001,3,48,Memory::Read,&memory,output));
    Check(!ResolveOrigin(origin,0x7000,4,48,Memory::Read,&memory,output));
    Check(!ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output));
    for(auto index:{0,3,4}){const auto value=memory.flush[index];memory.flush[index]=0;
        Check(!ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output));memory.flush[index]=value;}
    memory.batch[0x48]^=1;Check(!ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output));memory.batch[0x48]^=1;
    for(auto flags:{uint64_t{},uint64_t(3)<<50}){memory.packet[1]=flags;Check(!ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output));}
    memory.packet[1]=uint64_t(1)<<51;memory.draw[0]=1;Check(!ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output));
    memory.draw[0]=0;memory.packet[1]=uint64_t(3)<<50;
    const uint32_t skinCount=3,ordinaryCount=99;
    std::memcpy(memory.batch.data()+0x1C,&skinCount,4);std::memcpy(memory.batch.data()+0x0C,&ordinaryCount,4);
    Check(ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output) && output==memory.draw);
    Check(!ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output));
    // The existing entry thunk remains specific to the ordinary flush caller (2.5 return 0x335496); any other
    // caller -- here 2.31's skin flush return -- is refused.
    auto skinEntry=memory.entry;skinEntry.caller=memory.base+0x1F1A56;
    Check(!OriginFromEntry(skinEntry,memory.base).valid);
    for(const uint32_t count:{0u,2u,4u,UINT32_MAX}){
        std::memcpy(memory.batch.data()+0x1C,&count,4);
        Check(!ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output));
    }
    std::memcpy(memory.batch.data()+0x1C,&skinCount,4);
    for(const uint32_t stride:{0u,16u,32u,63u,65u,UINT32_MAX})
        Check(!ResolveOrigin(origin,0x7000,3,stride,Memory::Read,&memory,output));
    for(const auto flags:{uint64_t{},uint64_t(1)<<50,uint64_t(1)<<51}){
        memory.packet[1]=flags;Check(!ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output));
    }
    memory.packet[1]=uint64_t(3)<<50;
    Check(!ResolveOrigin(origin,0x7001,3,64,Memory::Read,&memory,output));
    Check(!ResolveOrigin(origin,0x7000,4,64,Memory::Read,&memory,output));
    Check(!ResolveOrigin(origin,0x7000,3,64,nullptr,&memory,output));
    for(auto index:{0,3,4}){const auto value=memory.flush[index];memory.flush[index]=0;
        Check(!ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output));memory.flush[index]=value;}
    memory.batch[0x48]^=1;Check(!ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output));memory.batch[0x48]^=1;
    memory.draw[0]=1;Check(!ResolveOrigin(origin,0x7000,3,64,Memory::Read,&memory,output));memory.draw[0]=0;
    memory.packet[1]=uint64_t(1)<<51;
    Check(ResolveOrigin(origin,0x7000,3,48,Memory::Read,&memory,output) && output==memory.draw);
    DrawOriginStack stack;Check(!stack.Current());stack.Enter(origin);Check(stack.Current()->valid);
    stack.Enter({});Check(!stack.Current()->valid);stack.Leave();Check(stack.Current()->renderer==origin.renderer);
    for(unsigned i=0;i<32;++i)stack.Enter(origin);Check(!stack.Current());stack.Leave();Check(stack.Current()->valid);
    for(unsigned i=0;i<32;++i)stack.Leave();Check(!stack.Current());stack.Leave();Check(!stack.Current());
    std::puts("PASS scoped origin identity, strict 48/64 packet strides and skin batch count, live draw checks, nesting and bounded overflow");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
