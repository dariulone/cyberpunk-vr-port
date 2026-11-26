#include "Render/NativeIaBinding.hpp"
#include <map>
#include <vector>
#include <cstdio>
#include <stdexcept>

using namespace cvr::stereo;
using namespace cvr::stereo::geometry;
static void Require(bool value,const char* why){if(!value)throw std::runtime_error(why);}
namespace {
constexpr uint64_t Game=0x140000000,Registry=0x100000,Context=0x2000000,State=0x3000000,List=0x4000000,
    Resource=0x5000000,Signature=0x6000000,Gpu=0x7000000,Buckets=0x8000000,Entries=0x8100000;
struct Fixture {
    std::map<uint64_t,std::vector<uint8_t>> memory;
    uint64_t record=Registry+0x5C0AF8+0xB0;
    UINT recordReads{};size_t changeByte=SIZE_MAX;
    template<class T>void Put(uint64_t address,const T& value){
        for(auto& [base,bytes]:memory)if(address>=base && address-base<=bytes.size() && sizeof(value)<=bytes.size()-(address-base)){
            std::memcpy(bytes.data()+address-base,&value,sizeof(value));return;
        }
        auto& bytes=memory[address];bytes.resize(sizeof(value));std::memcpy(bytes.data(),&value,sizeof(value));
    }
    Fixture(){
        memory[Context].resize(0x640);memory[record].resize(0xB0);
        Put(Game+0x3BCDCA8,Registry);Put(Context+0x30,List);Put(Context+0x60,State);
        Put(Context+0x68,uint32_t(1));Put(Context+0x6C,uint32_t(2));Put(Context+0x70,uint32_t(128));
        Put(Registry+0x1410678,Signature);Put(Registry+0x1410670,Signature+0x100);
        Put(record,uint32_t(512));Put(record+6,uint8_t(0x6B));Put(record+8,Gpu+128);Put(record+16,Resource);
        Put(record+0x40,uint64_t(0x1122334455667788));Put(record+0x58,uint64_t(0x8877665544332211));Put(record+0x68,uint64_t(0xAABBCCDDEEFF0011));
        struct Map {uint64_t buckets;uint32_t count,bucketCount;uint64_t data;uint32_t capacity,stride;};
        static_assert(sizeof(Map)==32);
        Put(Context+0x568,Map{Buckets,1,4,Entries,1,16});Put(Buckets+8,uint32_t(0));
        Put(Entries,std::array<uint32_t,4>{UINT_MAX,2,2,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT});
    }
    static bool Read(void* user,uint64_t address,void* out,size_t length){
        auto& fixture=*static_cast<Fixture*>(user);
        if(address==fixture.record && length==0xB0 && ++fixture.recordReads==2 && fixture.changeByte!=SIZE_MAX)
            fixture.memory.at(fixture.record)[fixture.changeByte]^=1;
        for(const auto& [base,bytes]:fixture.memory)if(address>=base && address-base<=bytes.size() && length<=bytes.size()-(address-base)){
            std::memcpy(out,bytes.data()+address-base,length);return true;
        }
        return false;
    }
};
camera_upload::Context Current(){return {Context,State,List};}
IndirectCall Call(){return {List,Signature,Resource,128,0,0,1};}
IndirectFrame Frame(){return {0x342B11,Context,Registry};}
bool Decode(Fixture& fixture,IndirectNativeRecord& out,IndirectFrame frame=Frame(),IndirectCall call=Call(),camera_upload::Context current=Current()){
    return indirect_binding_detail::Decode(Game,frame,current,call,Fixture::Read,&fixture,out);
}
void ExactSites(){
    for(const uint64_t site:{0x342B11ull,0xD543BBull,0x413AE0ull}){
        Fixture fixture;auto frame=Frame();auto call=Call();frame.rva=site;
        if(site==0xD543BB){frame.rbx=0xDEADBEEF;frame.rdi=Context;call.signature=Signature+0x100;}
        if(site==0x413AE0)frame.rdi=0xDEADBEEF;
        IndirectNativeRecord out;Require(Decode(fixture,out,frame,call),"Exact pure indirect site did not decode");
        std::array<uint8_t,0xB0> raw{};std::memcpy(raw.data(),fixture.memory.at(fixture.record).data(),raw.size());
        Require(out.arguments.handle==2 && out.arguments.resource==Resource && out.arguments.gpu==Gpu+128 && out.arguments.bytes==512 &&
            out.arguments.registry==Registry && out.arguments.context==Context && out.arguments.kind==6 && out.context==Current() &&
            out.recordAddress==fixture.record && out.rawRecord==raw,"Decoded source receipt lost exact handle/context/raw record");
        Require(out.signatureKind==(site==0xD543BB?2u:1u) && out.signatureStride==(site==0xD543BB?16u:20u) && out.callerRva==site,
            "Native indirect signature ABI changed");
        Require(indirect_binding_detail::MatchesOrigin(frame,out.callerRva,Current(),Registry),"Actual unwound source identity did not match");
        auto changed=frame;changed.rva=site==0x342B11?0x413AE0:0x342B11;
        Require(!indirect_binding_detail::MatchesOrigin(changed,out.callerRva,Current(),Registry),"Historical caller passed source callback validation");
        changed=frame;if(site==0xD543BB)++changed.rdi;else ++changed.rbx;
        Require(!indirect_binding_detail::MatchesOrigin(changed,out.callerRva,Current(),Registry),"Unwound current native context changed unnoticed");
        if(site==0x342B11){changed=frame;++changed.rdi;
            Require(!indirect_binding_detail::MatchesOrigin(changed,out.callerRva,Current(),Registry),"Unwound current renderer changed unnoticed");}
        bool fullBacking{};
        Require(indirect_binding_detail::SourceRange(out.arguments,Gpu,4096,128,out.signatureStride,fullBacking) && !fullBacking,
            "Logical shared-backing suballocation bounds were not preserved");
        const auto state=ReadNativeState(out.arguments,List,Fixture::Read,&fixture);
        Require(state.status==2 && state.value==D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT && state.kind==6,
            "Exact source native handle state was not recovered");
    }
    Fixture fixture;fixture.Put(Context+0x68,uint32_t(2));IndirectNativeRecord out;
    Require(Decode(fixture,out),"Second supported native recording context kind was rejected");
}
void Rejections(){
    auto reject=[&](Fixture& fixture,IndirectFrame frame,IndirectCall call,camera_upload::Context current,const char* why){
        IndirectNativeRecord out;out.recordAddress=0xDEADBEEF;
        Require(!Decode(fixture,out,frame,call,current),why);Require(out.recordAddress==0xDEADBEEF,"Rejected decoder published partial output");
    };
    for(const uint64_t site:{0ull,0x623310ull,0x342B10ull,0x342B12ull,0xD543BAull,0x413AE1ull}){
        Fixture fixture;auto frame=Frame();frame.rva=site;reject(fixture,frame,Call(),Current(),"Unknown/dispatch/nearby return site accepted");
    }
    for(UINT which=0;which<12;++which){
        Fixture fixture;auto frame=Frame();auto call=Call();auto current=Current();
        switch(which){
        case 0:++frame.rbx;break;case 1:++frame.rdi;break;case 2:++call.list;break;case 3:++call.signature;break;
        case 4:++call.arguments;break;case 5:call.argumentOffset=132;break;case 6:call.countBuffer=Resource;break;
        case 7:call.countOffset=4;break;case 8:call.maxCount=0;break;case 9:call.maxCount=2;break;
        case 10:++current.state;break;case 11:++current.list;break;
        }
        reject(fixture,frame,call,current,"Native frame/API/current-context mismatch accepted");
    }
    for(UINT which=0;which<9;++which){
        Fixture fixture;
        switch(which){
        case 0:fixture.Put(Context+0x30,List+8);break;case 1:fixture.Put(Context+0x60,State+8);break;
        case 2:fixture.Put(Context+0x68,uint32_t(3));break;case 3:fixture.Put(Context+0x638,uint64_t(1));break;
        case 4:fixture.Put(Context+0x534,uint32_t(1));break;case 5:fixture.Put(Context+0x6C,uint32_t(0));break;
        case 6:fixture.Put(Context+0x6C,uint32_t(0x100001));break;case 7:fixture.Put(Context+0x70,uint32_t(132));break;
        case 8:fixture.Put(Registry+0x1410678,Signature+8);break;
        }
        reject(fixture,Frame(),Call(),Current(),"Stale native context/handle/signature state accepted");
    }
    for(UINT which=0;which<5;++which){
        Fixture fixture;
        switch(which){
        case 0:fixture.Put(fixture.record+6,uint8_t(0x6A));break;case 1:fixture.Put(fixture.record,uint32_t(0));break;
        case 2:fixture.Put(fixture.record+8,uint64_t(0));break;case 3:fixture.Put(fixture.record+16,Resource+8);break;
        case 4:fixture.Put(fixture.record+8,UINT64_MAX-64);break;
        }
        reject(fixture,Frame(),Call(),Current(),"Malformed or stale exact native buffer record accepted");
    }
    for(const size_t at:{size_t(0x40),size_t(0x58),size_t(0x68),size_t(6)}){
        Fixture fixture;fixture.changeByte=at;
        reject(fixture,Frame(),Call(),Current(),"Double-read missed changed allocation/raw native fields");
    }
    Fixture fixture;auto call=Call();call.argumentOffset=129;
    reject(fixture,Frame(),call,Current(),"Unaligned API argument offset accepted");
    IndirectNativeRecord out;
    Require(!indirect_binding_detail::Decode(UINT64_MAX,Frame(),Current(),Call(),Fixture::Read,&fixture,out),"Overflowing image base accepted");
}
void BoundsAndSharedHandles(){
    Fixture fixture;IndirectNativeRecord first;Require(Decode(fixture,first),"Initial native handle did not decode");
    bool full{};
    Require(!indirect_binding_detail::SourceRange(first.arguments,Gpu,4096,0,20,full),"API offset was implicitly rebased into native logical allocation");
    Require(!indirect_binding_detail::SourceRange(first.arguments,Gpu,4096,624,20,full),"Payload escaped logical allocation");
    Require(!indirect_binding_detail::SourceRange(first.arguments,Gpu,128,128,20,full),"Payload escaped backing buffer");
    Require(!indirect_binding_detail::SourceRange(first.arguments,UINT64_MAX-16,4096,0,20,full),"Backing GPU address overflow accepted");
    auto whole=first.arguments;whole.gpu=Gpu;whole.bytes=4096;
    Require(indirect_binding_detail::SourceRange(whole,Gpu,4096,128,20,full) && full,"Full backing range was not reported explicitly");
    Require(indirect_binding_detail::SourceRange(whole,Gpu,4096,4096,0,full) && full,"Empty endpoint range should not read data");
    Require(!indirect_binding_detail::SourceRange(whole,Gpu,4096,4096,1,full) && !full,"Nonempty endpoint range escaped backing buffer");
    const auto secondRecord=Registry+0x5C0AF8+2*0xB0;fixture.memory[secondRecord]=fixture.memory.at(fixture.record);
    fixture.Put(secondRecord+8,Gpu+1024);fixture.Put(Context+0x6C,uint32_t(3));fixture.Put(Context+0x70,uint32_t(1024));
    auto call=Call();call.argumentOffset=1024;IndirectNativeRecord second;
    Require(Decode(fixture,second,Frame(),call) && second.arguments.handle==3 && second.arguments.resource==first.arguments.resource &&
        second.arguments.gpu!=first.arguments.gpu && !indirect_binding_detail::SameRecord(first,second),"Shared backing resurrected the previous native handle");
    Require(indirect_binding_detail::SourceRange(second.arguments,Gpu,4096,1024,20,full) && !full,"Second native suballocation was not bounded independently");
    Require(!indirect_binding_detail::SourceRange(first.arguments,Gpu,4096,1024,20,full),"First handle certified another allocation's API offset");
    for(const size_t at:{size_t(0x40),size_t(0x58),size_t(0x68)}){auto changed=second;changed.rawRecord[at]^=1;
        Require(!indirect_binding_detail::SameRecord(second,changed),"Revalidation lost unclassified raw allocation metadata");}
}
void StateRejections(){
    for(UINT which=0;which<5;++which){
        Fixture fixture;IndirectNativeRecord out;Require(Decode(fixture,out),"State fixture source failed");
        switch(which){
        case 0:fixture.Put(Context+0x534,uint32_t(1));break;
        case 1:fixture.Put(Entries,std::array<uint32_t,4>{UINT_MAX,2,2,0x80000000});break;
        case 2:fixture.Put(Entries,std::array<uint32_t,4>{UINT_MAX,3,3,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT});break;
        case 3:fixture.Put(fixture.record+16,Resource+8);break;case 4:fixture.Put(Context+0x30,List+8);break;
        }
        const auto state=ReadNativeState(out.arguments,List,Fixture::Read,&fixture);
        Require(state.status!=2,"Missing/pending/stale native state became numeric copy permission");
    }
    Fixture fixture;IndirectNativeRecord out;Require(Decode(fixture,out),"Read-mask fixture source failed");
    fixture.Put(Entries,std::array<uint32_t,4>{UINT_MAX,2,2,D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE});
    const auto state=ReadNativeState(out.arguments,List,Fixture::Read,&fixture);
    Require(state.status==2 && state.value==(D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE) &&
        !(state.value&D3D12_RESOURCE_STATE_COPY_SOURCE),"Native state was inferred or upgraded to COPY_SOURCE");
}
}
int main()try{
    ExactSites();Rejections();BoundsAndSharedHandles();StateRejections();
    Require(indirect_binding_detail::Contract.size()==13,"Native indirect full-body guard inventory changed");
    for(const auto& code:indirect_binding_detail::Contract)Require(code.rva && code.bytes && code.bytes<=2048 && code.hash,"Native indirect guard lacks bounded full-body evidence");
    std::puts("PASS exact native indirect sites/ABI/rawB0 fields, double-read stability, logical/shared-backing bounds and current handle state without copy inference");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
