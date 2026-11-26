#include "Render/NativeSourceCbvBinding.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>

using namespace cvr::stereo;
using namespace cvr::stereo::source_cbv;
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
struct Memory {
    std::map<uint64_t,uint8_t> bytes;
    template<class T>void Put(uint64_t at,T value){const auto* data=reinterpret_cast<const uint8_t*>(&value);for(size_t i=0;i<sizeof(value);++i)bytes[at+i]=data[i];}
    void Zero(uint64_t at,size_t n){for(size_t i=0;i<n;++i)bytes[at+i]=0;}
    static bool Read(void* user,uint64_t at,void* output,size_t n){auto& self=*static_cast<Memory*>(user);auto* data=static_cast<uint8_t*>(output);
        for(size_t i=0;i<n;++i){const auto found=self.bytes.find(at+i);if(found==self.bytes.end())return false;data[i]=found->second;}return true;}
};
static void StageMasks(){
    uint32_t tracked{};
    Require(detail::TrackStageMask(1,tracked) && tracked==1,"VS-only mask changed");
    Require(detail::TrackStageMask(2,tracked) && tracked==2,"PS-only mask changed");
    Require(detail::TrackStageMask(3,tracked) && tracked==3,"VS/PS mask changed");
    Require(detail::TrackStageMask(7,tracked) && tracked==3 && !(tracked&4),"Common native mask7 did not track only VS/PS");
    for(uint32_t stages:std::array<uint32_t,6>{0,4,5,6,8,UINT32_MAX}){
        tracked=UINT32_MAX;Require(!detail::TrackStageMask(stages,tracked) && !tracked,"Unknown mask retained a tracked stage");
    }
}
static void Slots(){
    Memory m;const uint64_t state=0x1000,layout=0x2000,table=0x3000;
    m.Put(state+0x68,layout);m.Put(state+0x28,table);
    m.Put<uint8_t>(layout+0x455,3);const auto entry=layout+0x38+3*16;m.Zero(entry,16);
    m.Put<uint32_t>(entry,2);m.Put<uint32_t>(entry+4,20);m.Put<uint16_t>(entry+8,0);m.Put<uint16_t>(entry+10,4);
    m.Put<uint8_t>(entry+14,0);m.Put<uint64_t>(table+21*8,0x4000);
    Slot camera;Require(detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"VS b1 camera slot was not resolved");
    Require(camera.group==3 && camera.parameter==0 && camera.offset==1 && camera.index==21 && camera.descriptor==0x4000,"Group and original root parameter were collapsed");
    m.Put<uint8_t>(layout+0x453+2*0x1C+1*2,3);
    Slot unsupported;Require(!detail::ReadSlot(Memory::Read,&m,state,2,1,unsupported),"Mask7 admitted a populated source stage2 slot");
    m.Put<uint8_t>(layout+0x477,7);const auto pixel=layout+0x38+7*16;m.Zero(pixel,16);
    m.Put<uint32_t>(pixel,2);m.Put<uint32_t>(pixel+4,100);m.Put<uint16_t>(pixel+8,4);m.Put<uint16_t>(pixel+10,1);
    m.Put<uint8_t>(pixel+14,19);m.Put<uint64_t>(table+100*8,0x5000);
    Slot material;Require(detail::ReadSlot(Memory::Read,&m,state,1,4,material) && material.parameter==19 && material.offset==0,"PS b4 stage index/slot was not resolved");
    m.Put<uint8_t>(layout+0x45D,8);const auto dynamic=layout+0x38+8*16;m.Zero(dynamic,16);
    m.Put<uint32_t>(dynamic,2);m.Put<uint32_t>(dynamic+4,110);m.Put<uint16_t>(dynamic+8,5);m.Put<uint16_t>(dynamic+10,2);
    m.Put<uint8_t>(dynamic+14,2);m.Put<uint64_t>(table+110*8,0x6000);
    Slot b5;Require(detail::ReadSlot(Memory::Read,&m,state,0,5,b5) && b5.parameter==2 && b5.offset==0,"Direct dynamic VS b5 was missed");
    Slot pending;Require(detail::SelectSlot(Memory::Read,&m,state,0,7,0x7000,false,pending) && pending.descriptor==0x7000 && !pending.layout,"Pre-materialization setter lost its native CPU descriptor");
    Require(!detail::SelectSlot(Memory::Read,&m,state,0,7,0x7000,true,pending),"Source callback accepted an unmaterialized slot");
    Require(!detail::SelectSlot(Memory::Read,&m,state,0,1,0x7000,true,pending),"Source CPU descriptor mismatch accepted");
    Require(!detail::SelectSlot(Memory::Read,&m,state,2,1,0x7000,false,pending),"Unknown pre-materialization stage accepted");
    Slot relocated=camera;relocated.layout+=0x1000;relocated.table+=0x1000;relocated.index+=10;
    Require(detail::SameSourceSlot(camera,relocated),"Inherited native CPU descriptor required a historical shadow-table location");
    relocated.descriptor++;Require(!detail::SameSourceSlot(camera,relocated),"Different current native CPU descriptor accepted");
    Require(!detail::ReadSlot(Memory::Read,&m,state,2,1,b5) && !detail::ReadSlot(Memory::Read,&m,state,0,14,b5),"Unknown stage/register was accepted");
    m.Put<uint8_t>(layout+0x455,255);Require(!detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"Unbound stage mapping was accepted");
    m.Put<uint8_t>(layout+0x455,3);m.Put<uint16_t>(entry+8,2);Require(!detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"Register below range start was accepted");
    m.Put<uint16_t>(entry+8,0);m.Put<uint16_t>(entry+10,1);Require(!detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"Register outside range count was accepted");
    m.Put<uint16_t>(entry+10,4);m.Put<uint32_t>(entry+4,8191);Require(!detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"CPU table extent overflow was accepted");
    m.Put<uint32_t>(entry+4,20);m.Put<uint8_t>(entry+14,64);Require(!detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"Invalid original root parameter was accepted");
    m.Put<uint8_t>(entry+14,0);m.Put<uint8_t>(entry+13,2);Require(!detail::ReadSlot(Memory::Read,&m,state,0,1,camera),"Non-table native binding form was accepted");
}
static void Creators(){
    Memory m;detail::CreationFrame frame;detail::Creation created;
    frame.rva=0x333C1A;frame.rsp=0x1000;frame.rbx=0x2000;m.Put<uint64_t>(0x1030,0x3000);m.Put<uint64_t>(0x1038,256);
    Require(detail::DecodeCreation(frame,Memory::Read,&m,created) && created.resource==0x3000 && created.offset==256 && created.cpu==0x2000,"Fresh standalone uploader creator decode failed");
    Require(!created.logicalBytes && !created.reservedBytes && !created.completedProducerRva,"Standalone creation invented direct b5 write provenance");
    frame={};frame.rva=0x3362A0;frame.rbx=0x3000;frame.r15=512;frame.r14=0x4000;frame.r13=112;frame.r12=256;m.Put<uint64_t>(0x4078,0x5000);
    Require(detail::DecodeCreation(frame,Memory::Read,&m,created) && created.resource==0x3000 && created.context==0x4000 && created.cpu==0x5000 && created.offset==512 &&
        created.logicalBytes==112 && created.reservedBytes==256 && created.completedProducerRva==detail::DirectMemcpyReturnRva && detail::DirectProducer(created,256),
        "Dynamic b5 actual resource/offset/completed logical write decode failed");
    frame={};frame.rva=0x3346B1;frame.r14=0x6000;frame.r13=0x4000;frame.rdi=0x7000;frame.r12=768;
    m.Put<uint64_t>(frame.r14+0x14F2240,0x3000);m.Put<uint64_t>(frame.rdi+0x28,0x8000);
    m.Put<uint32_t>(frame.rdi-0xC,71);m.Put<uint32_t>(frame.rdi-8,71);
    Require(detail::DecodeCreation(frame,Memory::Read,&m,created) && created.resource==0x3000 && created.context==0x4000 && created.cpu==0x8000 && created.offset==768 && created.handle==71,"Transient exact native creator/key decode failed");
    Require(!created.logicalBytes && !created.reservedBytes && !created.completedProducerRva && !detail::DirectProducer(created,256),
        "Transient pre-write creation invented a completed direct memcpy");
    m.Put<uint32_t>(frame.rdi-8,72);Require(!detail::DecodeCreation(frame,Memory::Read,&m,created),"Mismatched transient map key accepted");
    m.Put<uint32_t>(frame.rdi-8,71);
    frame.rva=0x3346B2;Require(!detail::DecodeCreation(frame,Memory::Read,&m,created),"A nearby address was accepted as a creator");
    frame.rva=0x3346B1;frame.r14=UINT64_MAX;Require(!detail::DecodeCreation(frame,Memory::Read,&m,created),"Creator registry overflow was accepted");
    frame={};frame.rva=0x333C1A;frame.rsp=UINT64_MAX;Require(!detail::DecodeCreation(frame,Memory::Read,&m,created),"Creator stack overflow was accepted");
    frame={};frame.rva=0x3362A0;frame.r14=0x4000;frame.r15=256;Require(!detail::DecodeCreation(frame,Memory::Read,&m,created),"Missing live API resource was accepted");
    Require(detail::DirectCreation(0,5,0x3362A0),"Exact direct VS b5 creator precedence was rejected");
    Require(!detail::DirectCreation(1,5,0x3362A0) && !detail::DirectCreation(0,4,0x3362A0) && !detail::DirectCreation(0,5,0x3346B1),"Direct creator exception escaped VS b5");
}
static void DirectProducerCounts(){
    Memory memory;memory.Put<uint64_t>(0x4078,0x5000);
    detail::CreationFrame frame;frame.rva=detail::DirectCreatorRva;frame.rbx=0x3000;frame.r14=0x4000;frame.r15=512;
    detail::Creation creation;
    const std::array<std::array<uint64_t,2>,6> accepted{{{16,256},{112,256},{256,256},{257,512},{65535,65536},{65536,65536}}};
    for(const auto& counts:accepted){frame.r13=counts[0];frame.r12=counts[1];
        Require(detail::DecodeCreation(frame,Memory::Read,&memory,creation) && creation.logicalBytes==counts[0] && creation.reservedBytes==counts[1] &&
            detail::DirectProducer(creation,UINT(counts[1])),"Completed direct producer logical/padded bounds rejected");
        Require(!detail::DirectProducer(creation,UINT(counts[1])-1),"Direct creation accepted another actual view size");
    }
    const std::array<std::array<uint64_t,2>,13> rejected{{{0,256},{112,0},{112,112},{112,255},{112,512},{256,512},
        {257,256},{65536,65280},{65536,65792},{65537,65792},{UINT64_MAX,256},{(uint64_t(1)<<32)+112,256},{112,(uint64_t(1)<<32)+256}}};
    for(const auto& counts:rejected){frame.r13=counts[0];frame.r12=counts[1];creation.caller=UINT32_MAX;
        Require(!detail::DecodeCreation(frame,Memory::Read,&memory,creation) && creation==detail::Creation{},
            "Direct creator admitted unknown/unaligned/truncated/oversized write extent or retained partial facts");
    }
    frame.r13=112;frame.r12=256;
    Require(detail::DecodeCreation(frame,Memory::Read,&memory,creation),"Valid direct producer did not recover after rejected extent");
    const auto valid=creation;creation.completedProducerRva++;
    Require(!detail::DirectProducer(creation,256),"Nearby return address manufactured completed producer provenance");
    creation=valid;creation.caller=0x3346B1;Require(!detail::DirectProducer(creation,256),"Another creator reused direct producer facts");
    creation=valid;creation.logicalBytes=257;Require(!detail::DirectProducer(creation,256),"Logical producer exceeded its reserved bytes");
    frame.rva=detail::DirectCreatorRva+1;
    Require(!detail::DecodeCreation(frame,Memory::Read,&memory,creation) && creation==detail::Creation{},"Nearby creator accepted valid-looking counts");
}
static void UploadWriters(){
    const detail::UploadWriterContext context{1,2,3,4,5,6};
    const detail::UploadWriterJoin origin{91,0x100000,0x200000,768,71,448,512};
    detail::UploadWriterCompletion completed;
    detail::UploadWriterLedger ledger;
    auto scope=ledger.Enter(context,71,448);
    Require(scope.nonce && ledger.Join(scope,context,origin) && ledger.Finish(scope,context,completed),"Exact native writer invocation did not close");
    Require(completed.nonce==scope.nonce && completed.origin==91 && completed.destination==0x100000 && completed.bytes==448 &&
        completed.producerRva==detail::UploadWriterMemcpyReturnRva && !ledger.Depth(),"Writer completion collapsed mapped destination into descriptor/source labels");
    const auto firstNonce=scope.nonce;scope=ledger.Enter(context,71,16);
    Require(scope.nonce>firstNonce && ledger.Join(scope,context,origin) && ledger.Finish(scope,context,completed) && completed.bytes==16,
        "Bounded completed prefix was replaced by padded or full native extent");
    scope=ledger.Enter(context,71,448);Require(!ledger.Finish(scope,context,completed) && !completed.nonce,"Normal return without a newly-created origin manufactured a writer");
    scope=ledger.Enter(context,71,448);Require(ledger.Join(scope,context,origin) && ledger.Cancel(scope) &&
        !ledger.Finish(scope,context,completed) && !completed.nonce,"Exception/destructor cancellation retained a completed writer");
    scope=ledger.Enter(context,71,448);Require(ledger.Join(scope,context,origin) && !ledger.Join(scope,context,origin) &&
        !ledger.Finish(scope,context,completed) && !completed.nonce,"Ambiguous second creation inherited an earlier origin");
    for(UINT field=0;field<6;++field){auto changed=context;
        switch(field){case 0:++changed.thread;break;case 1:++changed.token;break;case 2:++changed.list;break;
            case 3:++changed.generation;break;case 4:++changed.context;break;default:++changed.registry;break;}
        scope=ledger.Enter(context,71,448);
        Require(!ledger.Join(scope,changed,origin) && !ledger.Finish(scope,context,completed),"Foreign creator recording/thread joined the active writer");
        scope=ledger.Enter(context,71,448);
        Require(ledger.Join(scope,context,origin) && !ledger.Finish(scope,changed,completed) && !completed.nonce,"Foreign normal return closed a native writer");
    }
    auto invalid=origin;invalid.handle=72;scope=ledger.Enter(context,71,448);
    Require(!ledger.Join(scope,context,invalid) && !ledger.Finish(scope,context,completed),"Another native handle joined the writer");
    for(const auto bytes:std::array<UINT,3>{449,512,65536}){scope=ledger.Enter(context,71,bytes);
        Require(!ledger.Join(scope,context,origin) && !ledger.Finish(scope,context,completed),"Writer copied outside the native logical prefix");}
    for(UINT field=0;field<5;++field){invalid=origin;
        switch(field){case 0:invalid.destination=0;break;case 1:invalid.destination=UINT64_MAX-127;break;
            case 2:invalid.logicalBytes=0;break;case 3:invalid.reservedBytes=448;break;default:invalid.reservedBytes=768;break;}
        scope=ledger.Enter(context,71,448);Require(!ledger.Join(scope,context,invalid) && !ledger.Finish(scope,context,completed),"Invalid destination/logical reservation joined a writer");
    }
    const auto outer=ledger.Enter(context,71,448);Require(ledger.Join(outer,context,origin),"Outer writer could not join its actual origin");
    const auto nested=ledger.Enter(context,71,448);
    Require(!ledger.Join(nested,context,origin) && !ledger.Finish(nested,context,completed) &&
        !ledger.Finish(outer,context,completed) && !completed.nonce,"Nested writer revived an outer closure or published a nonce");
    for(const auto bytes:{0u,65537u}){
        const auto outerBad=ledger.Enter(context,71,448);Require(ledger.Join(outerBad,context,origin),"Invalid-nesting precondition failed");
        Require(!ledger.Enter(context,72,bytes).nonce && !ledger.Finish(outerBad,context,completed),"Invalid nested size preserved outer completion");
    }
    const auto outerBad=ledger.Enter(context,71,448);Require(ledger.Join(outerBad,context,origin),"Invalid-context precondition failed");
    Require(!ledger.Enter({},72,448).nonce && !ledger.Finish(outerBad,context,completed),"Unobserved nested context preserved outer completion");
    std::array<detail::UploadWriterStamp,detail::MaxUploadWriterDepth> scopes;
    for(auto& value:scopes){value=ledger.Enter(context,71,448);Require(value.nonce,"Bounded writer stack filled early");}
    Require(!ledger.Enter(context,71,448).nonce,"Writer TLS overflow admitted another invocation");
    for(UINT n=UINT(scopes.size());n;--n)Require(ledger.Cancel(scopes[n-1]),"Bounded writer stack could not unwind");
    Require(!ledger.Enter({},71,448).nonce && !ledger.Enter(context,0,448).nonce && !ledger.Enter(context,0x100001,448).nonce &&
        !ledger.Enter(context,71,0).nonce && !ledger.Enter(context,71,65537).nonce,"Unknown recording/handle/prefix entered a writer");
    const std::array<uint64_t,3> path{0x3346B1,0x334469,0x33434B};
    Require(detail::UploadWriterPath(path),"Exact native BufUpload creator ancestry rejected");
    auto escaped=path;escaped[1]=0x2200FF;Require(!detail::UploadWriterPath(escaped),"Pointer-returning 220068 origin was certified by an outer writer");
    escaped=path;++escaped[2];Require(!detail::UploadWriterPath(escaped),"Nearby native caller manufactured writer ancestry");
    Require(!detail::UploadWriterPath({path.data(),2}),"Incomplete native unwind manufactured normal-return provenance");
}
static void Views(){
    Require(detail::Descriptor(0x100000,65536,0x100100,1024),"Aligned camera descriptor range was rejected");
    Require(!detail::Descriptor(0x100000,65536,0x100101,1024),"Unaligned CBV address accepted");
    Require(!detail::Descriptor(0x100000,65536,0x100100,848),"Logical bytes mistaken for padded CBV size");
    Require(!detail::Descriptor(0x100000,512,0x100100,512),"CBV beyond actual backing width accepted");
    Require(!detail::Descriptor(UINT64_MAX-255,512,UINT64_MAX-255,256),"Backing GPU address overflow accepted");
    descriptors::DescriptorStamp stamp;stamp.gpu=0x200000;stamp.heap=9;stamp.value[0]=1;stamp.value[2]=13;stamp.value[3]=14;stamp.value[4]=0x100100;stamp.value[5]=1024;
    Require(detail::SameView(stamp,0x100100,1024),"Current canonical CBV view rejected");
    Require(detail::SameOrigin(stamp,13,14),"Held canonical creation origin rejected");
    Require(!detail::SameOrigin(stamp,13,15) && !detail::SameOrigin(stamp,12,14) && !detail::SameOrigin(stamp,0,0),"Foreign or absent creation origin accepted");
    Require(!detail::SameView(stamp,0x100200,1024) && !detail::SameView(stamp,0x100100,512),"Changed canonical address/size accepted");
    stamp.value[0]=2;Require(!detail::SameView(stamp,0x100100,1024),"SRV mistaken for a CBV");
    geometry::NativeBuffer buffer{0,0,448,9,10,71,4};const auto same=buffer;
    Require(detail::SameBuffer(buffer,same),"Exact live transient handle identity rejected");
    buffer.handle++;Require(!detail::SameBuffer(buffer,same),"A different transient handle was accepted");
    buffer=same;buffer.context++;Require(!detail::SameBuffer(buffer,same),"Foreign transient native context was accepted");
}
int main()try{StageMasks();Slots();Creators();DirectProducerCounts();UploadWriters();Views();std::puts("PASS pure current CBV bindings and exact direct/transient completed writer scopes, prefixes, ancestry, cancellation and ambiguity; no upload epoch or copy permission");return 0;}
catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
