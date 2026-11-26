#include "Natives/DynamicTextureOrigins.hpp"
#include <windows.h>
#include <array>
#include <barrier>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <string>
#include <thread>
using namespace cvr::texture_origins;
void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::abort();}}
struct State {
    std::array<Record,Capacity> rows{};
    std::atomic<uint64_t> sequence{},count{},dropped{},overwritten{};
    Writer writer{rows.data(),Capacity,sequence,count,dropped,overwritten};
};
Record Make(uint64_t value){Record r{};r.object=value;r.control=~value;r.objectSerial=value;r.caller=value+7;
    r.descriptor.width=uint32_t(value);r.descriptor.height=768;r.descriptor.flags=0x11;r.stackCount=32;
    for(size_t i=0;i<StackDepth;++i)r.stack[i]=value+i;return r;}
bool ReadStable(State& state,std::array<Record,Capacity>& rows,uint64_t& count){
    const auto before=state.sequence.load(std::memory_order_acquire);if(before&1)return false;
    count=state.count.load(std::memory_order_relaxed);SIZE_T copied{};
    if(!ReadProcessMemory(GetCurrentProcess(),state.rows.data(),rows.data(),sizeof(rows),&copied)||copied!=sizeof(rows))return false;
    return before==state.sequence.load(std::memory_order_acquire);
}
void Validate(const std::array<Record,Capacity>& rows,uint64_t count){
    const auto first=count>Capacity?count-Capacity+1:1;
    for(uint64_t i=first;i<=count;++i){const auto& r=rows[(i-1)%Capacity];Check(r.ordinal==i,"ordinal hole/torn row");
        Check(r.objectSerial==r.object && r.control==~r.object && r.caller==r.object+7 && r.descriptor.width==r.object && r.stackCount==32,"metadata torn across writers");
        for(size_t j=0;j<StackDepth;++j)Check(r.stack[j]==r.object+j,"stack torn across writers");}
}
int main(int argc,char** argv){if(argc!=2)return 2;const std::string mode=argv[1];auto state=std::make_unique<State>();
    if(mode=="overwrite"){
        for(unsigned i=1;i<=3000;++i)Check(state->writer.Publish(Make(i)),"single writer dropped a record");
        Check(state->count==3000 && state->overwritten==3000-Capacity && state->dropped==0,"overwrite coverage incorrect");
    }else if(mode=="parallel"){
        std::barrier start(5);std::array<std::jthread,4> workers;std::atomic<unsigned> active{4};
        for(unsigned w=0;w<4;++w)workers[w]=std::jthread([&,w]{start.arrive_and_wait();for(unsigned i=1;i<=5000;++i)state->writer.Publish(Make(w*5000+i));--active;});
        std::array<Record,Capacity> snapshot;uint64_t count{};start.arrive_and_wait();
        while(active.load()){if(ReadStable(*state,snapshot,count))Validate(snapshot,count);std::this_thread::yield();}
        for(auto& worker:workers)worker.join();Check(state->count+state->dropped==20000,"contention losses not accounted");
        Check(state->overwritten==(state->count>Capacity?state->count-Capacity:0),"parallel overwrite accounting incorrect");
    }else return 2;
    std::array<Record,Capacity> snapshot;uint64_t count{};Check(ReadStable(*state,snapshot,count),"final stable snapshot unavailable");Validate(snapshot,count);
    std::cout<<"PASS "<<mode<<" published="<<state->count<<" busyDropped="<<state->dropped<<'\n';
}
