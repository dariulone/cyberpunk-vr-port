#include <source_location>
#include <string>
#include "Render/NativeBreadcrumbWrite.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>

using namespace cvr::stereo::breadcrumb_write;
static void Require(bool value,std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("native breadcrumb write assertion failed at line "+std::to_string(at.line()));
}
struct Memory {
    std::map<uint64_t,uint8_t> bytes;
    template<class T> void Put(uint64_t address,const T& value){
        const auto* data=reinterpret_cast<const uint8_t*>(&value);
        for(size_t i=0;i<sizeof(value);++i)bytes[address+i]=data[i];
    }
    static bool Read(void* user,uint64_t address,void* output,size_t size){
        const auto& self=*static_cast<Memory*>(user);auto* data=static_cast<uint8_t*>(output);
        for(size_t i=0;i<size;++i){const auto found=self.bytes.find(address+i);if(found==self.bytes.end())return false;data[i]=found->second;}
        return true;
    }
};
struct Fixture {
    static constexpr uint64_t Base=0x140000000,Registry=0x30000000,Holder=0x10000,List=0x30000;
    static constexpr uint64_t Marker=0x40000,Heap=0x50000,Buffer=0x60000,Bitmap=0x70000,Device=0x80000,Gpu=0x200000000;
    static constexpr uint64_t StackLow=0x100000,StackHigh=0x110000,StackPointer=0x100100;
    uint64_t entry{},destination{};uint32_t mode{};Memory memory;detail::Frame frame;
    Fixture(uint32_t kind=1,uint32_t writeMode=1):mode(writeMode){
        Require(detail::FrameFromRegisters(Base,Base+(mode==1?detail::MarkerInReturn:detail::MarkerOutReturn),
            StackPointer,mode==1?StackPointer+0x30:0x800000000,Holder,Holder,StackLow,StackHigh,frame));
        const uint32_t stride=kind==1?4096:kind==4?2048:512,slots=kind==1?1024:kind==4?64:8,slot=3;
        entry=Base+0x3C28E38+uint64_t(kind)*0x70;destination=Gpu+uint64_t(slot-1)*stride+12;
        memory.Put(Base+0x3BCDCA8,Registry);memory.Put(Registry+1,uint8_t(0));memory.Put(Base+0x3C28E30,Device);
        // Deliberately no TLS/context memory: late native Close no longer has it.
        memory.Put(Holder,Marker);
        memory.Put(Marker,List);memory.Put(Marker+8,kind);memory.Put(Marker+0x10,stride/4);
        memory.Put(Marker+0x60,Gpu+uint64_t(slot-1)*stride);memory.Put(Marker+0x68,slot);
        memory.Put(entry+0x20,stride);memory.Put(entry+0x60,slots);memory.Put(entry+0x5C,(slots+63)/64);
        memory.Put(entry+0x30,Heap);memory.Put(entry+0x38,Buffer);memory.Put(entry+0x40,Gpu);
        memory.Put(entry+0x48,uint8_t(0));memory.Put(entry+0x50,Bitmap);memory.Put(Bitmap,uint64_t(1)<<(slot-1));
        SetDestination(destination);
    }
    void SetDestination(uint64_t value){destination=value;
        memory.Put(StackPointer+0x20,D3D12_WRITEBUFFERIMMEDIATE_PARAMETER{destination,mode});
        memory.Put(StackPointer+(mode==1?0x50:0x40),mode);
    }
    bool Read(detail::Observation& output,uint32_t* failure=nullptr){
        return detail::Observe(Memory::Read,&memory,Base,frame,List,{destination,mode},mode,output,failure);
    }
};
int main()try{
    Require(detail::CallerMode(Fixture::Base,Fixture::Base+detail::MarkerInReturn)==1);
    Require(detail::CallerMode(Fixture::Base,Fixture::Base+detail::MarkerOutReturn)==2);
    Require(!detail::CallerMode(Fixture::Base,Fixture::Base+detail::MarkerInReturn+1));
    const std::array<uint64_t,3> frames{0x700000000,Fixture::Base+detail::MarkerOutReturn,Fixture::Base+0x1FE85C};
    Require(detail::NearestCaller(Fixture::Base,0x5000000,frames)==2);
    const std::array<uint64_t,3> wrongNearest{0x700000000,Fixture::Base+0x1FE85C,Fixture::Base+detail::MarkerInReturn};
    Require(!detail::NearestCaller(Fixture::Base,0x5000000,wrongNearest));
    detail::Frame inFrame,outFrame;detail::FrameLayout layout;
    Require(detail::FrameFromRegisters(Fixture::Base,Fixture::Base+detail::MarkerInReturn,Fixture::StackPointer,
        Fixture::StackPointer+0x30,Fixture::Holder,0xDEADBEEF,Fixture::StackLow,Fixture::StackHigh,inFrame));
    Require(inFrame.holder==Fixture::Holder && detail::Layout(Fixture::Base,inFrame,layout) &&
        layout.parameters==Fixture::StackPointer+0x20 && layout.modes==Fixture::StackPointer+0x50);
    Require(detail::FrameFromRegisters(Fixture::Base,Fixture::Base+detail::MarkerOutReturn,Fixture::StackPointer,
        UINT64_MAX,0xDEADBEEF,Fixture::Holder,Fixture::StackLow,Fixture::StackHigh,outFrame));
    Require(outFrame.holder==Fixture::Holder && !outFrame.rbp && detail::Layout(Fixture::Base,outFrame,layout) &&
        layout.parameters==Fixture::StackPointer+0x20 && layout.modes==Fixture::StackPointer+0x40);
    detail::Frame rejectedFrame;
    Require(!detail::FrameFromRegisters(Fixture::Base,Fixture::Base+detail::MarkerInReturn,Fixture::StackPointer,
        Fixture::StackPointer+0x30,0,Fixture::Holder,Fixture::StackLow,Fixture::StackHigh,rejectedFrame));
    Require(!detail::FrameFromRegisters(Fixture::Base,Fixture::Base+detail::MarkerOutReturn,Fixture::StackPointer,
        UINT64_MAX,Fixture::Holder,0,Fixture::StackLow,Fixture::StackHigh,rejectedFrame));
    auto changedFrame=inFrame;changedFrame.holder+=8;Require(changedFrame!=inFrame);
    changedFrame=inFrame;changedFrame.rsp+=16;Require(changedFrame!=inFrame);
    changedFrame=inFrame;changedFrame.rip=Fixture::Base+detail::MarkerOutReturn;Require(changedFrame!=inFrame);
    changedFrame=inFrame;changedFrame.rbp+=8;Require(!detail::Layout(Fixture::Base,changedFrame,layout));
    changedFrame=inFrame;changedFrame.stackHigh=changedFrame.rsp+0x50;Require(!detail::Layout(Fixture::Base,changedFrame,layout));
    changedFrame=outFrame;changedFrame.stackHigh=changedFrame.rsp+0x40;Require(!detail::Layout(Fixture::Base,changedFrame,layout));
    changedFrame=inFrame;changedFrame.stackLow=changedFrame.rsp+1;Require(!detail::Layout(Fixture::Base,changedFrame,layout));
    changedFrame=inFrame;changedFrame.rsp=UINT64_MAX-15;Require(!detail::Layout(Fixture::Base,changedFrame,layout));
    changedFrame=inFrame;changedFrame.rsp++;Require(!detail::Layout(Fixture::Base,changedFrame,layout));
    D3D12_WRITEBUFFERIMMEDIATE_PARAMETER parameter{0x1000,1};
    Require(detail::Parameters(1,1,parameter,1));Require(!detail::Parameters(1,2,parameter,1));
    Require(!detail::Parameters(1,1,parameter,0));Require(!detail::Parameters(2,1,parameter,2));
    parameter.Value=2;Require(detail::Parameters(2,1,parameter,2));
    parameter.Dest=0;Require(!detail::Parameters(2,1,parameter,2));
    parameter.Dest=UINT64_MAX-3;Require(!detail::Parameters(2,1,parameter,2));
    parameter.Dest=0x1001;Require(!detail::Parameters(2,1,parameter,2));
    detail::Observation observed;
    for(uint32_t kind=1;kind<=4;++kind)for(uint32_t mode=1;mode<=2;++mode){Fixture f(kind,mode);Require(f.Read(observed));
        Require(observed.kind==kind && observed.slot==3 && observed.heap==Fixture::Heap && observed.resource==Fixture::Buffer);}
    Fixture good;uint32_t code=UINT32_MAX;Require(good.Read(observed,&code) && !code);
    const auto saved=observed;Require(good.Read(observed) && observed==saved);
    const auto reject=[](auto mutate,uint32_t expected){Fixture f;mutate(f);detail::Observation result;uint32_t failure{};
        Require(!f.Read(result,&failure) && failure==expected);};
    reject([](auto& f){f.frame.holder=0;},1);
    reject([](auto& f){f.destination++;},1);
    reject([](auto& f){f.memory.Put(Fixture::Base+0x3BCDCA8,uint64_t(0));},2);
    reject([](auto& f){f.memory.Put(Fixture::Registry+1,uint8_t(1));},2);
    reject([](auto& f){f.memory.Put(Fixture::Base+0x3C28E30,uint64_t(0));},2);
    reject([](auto& f){f.memory.Put(Fixture::Holder,uint64_t(0));},2);
    reject([](auto& f){f.frame.holder=Fixture::Holder+8;},2);
    reject([](auto& f){f.memory.Put(Fixture::Marker,Fixture::List+8);},3);
    reject([](auto& f){f.memory.Put(Fixture::Marker+8,uint32_t(0));},3);
    reject([](auto& f){f.memory.Put(Fixture::Marker+8,uint32_t(5));},3);
    reject([](auto& f){f.memory.Put(Fixture::Marker+0x68,uint32_t(0));},3);
    reject([](auto& f){f.memory.Put(Fixture::Marker+0x10,uint32_t(512));},4);
    reject([](auto& f){f.memory.Put(f.entry+0x20,uint32_t(0));},4);
    reject([](auto& f){f.memory.Put(f.entry+0x60,uint32_t(1025));},4);
    reject([](auto& f){f.memory.Put(f.entry+0x5C,uint32_t(15));},4);
    reject([](auto& f){f.memory.Put(Fixture::Marker+0x68,uint32_t(1025));},4);
    reject([](auto& f){f.memory.Put(f.entry+0x30,uint64_t(0));},5);
    reject([](auto& f){f.memory.Put(f.entry+0x38,uint64_t(0));},5);
    reject([](auto& f){f.memory.Put(f.entry+0x40,uint64_t(0));},5);
    reject([](auto& f){f.memory.Put(f.entry+0x48,uint8_t(1));},6);
    reject([](auto& f){f.memory.Put(f.entry+0x50,uint64_t(0));},6);
    reject([](auto& f){f.memory.Put(Fixture::Bitmap,uint64_t(0));},6);
    reject([](auto& f){f.memory.Put(Fixture::Marker+0x60,Fixture::Gpu+4);},7);
    reject([](auto& f){f.memory.Put(f.entry+0x40,UINT64_MAX-65535);},7);
    reject([](auto& f){f.SetDestination(Fixture::Gpu);},8);
    reject([](auto& f){f.SetDestination(Fixture::Gpu+3*4096);},8);
    reject([](auto& f){f.memory.Put(Fixture::StackPointer+0x20,f.destination+4);},9);
    reject([](auto& f){f.memory.Put(Fixture::StackPointer+0x28,uint32_t(2));},9);
    reject([](auto& f){f.memory.Put(Fixture::StackPointer+0x50,uint32_t(2));},9);
    good.SetDestination(Fixture::Gpu+2*4096);Require(good.Read(observed));
    good.SetDestination(Fixture::Gpu+3*4096-4);Require(good.Read(observed));
    good.memory.Put(Fixture::StackPointer+0x2C,uint32_t(0xDEADBEEF));Require(good.Read(observed));
    // An unrelated slot changing occupancy does not change this slot's owner.
    good.memory.Put(Fixture::Bitmap,uint64_t(12));Require(good.Read(observed) && observed==saved);
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=4096*1024;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    desc.Flags=D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE;
    D3D12_HEAP_DESC heap{};heap.SizeInBytes=desc.Width;heap.Alignment=65536;heap.Flags=D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS;
    D3D12_RESOURCE_ALLOCATION_INFO allocation{desc.Width,65536};
    Require(detail::Shape(saved,desc,heap,allocation,Fixture::Gpu));
    auto bad=desc;bad.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;Require(!detail::Shape(saved,bad,heap,allocation,Fixture::Gpu));
    bad=desc;bad.Width--;Require(!detail::Shape(saved,bad,heap,allocation,Fixture::Gpu));
    bad=desc;bad.Flags=D3D12_RESOURCE_FLAG_NONE;Require(!detail::Shape(saved,bad,heap,allocation,Fixture::Gpu));
    Require(!detail::Shape(saved,desc,heap,allocation,Fixture::Gpu+4));
    auto tooSmall=heap;tooSmall.SizeInBytes--;Require(!detail::Shape(saved,desc,tooSmall,allocation,Fixture::Gpu));
    auto invalid=allocation;invalid.SizeInBytes=UINT64_MAX;Require(!detail::Shape(saved,desc,heap,invalid,Fixture::Gpu));
    invalid=allocation;invalid.Alignment=0;Require(!detail::Shape(saved,desc,heap,invalid,Fixture::Gpu));
    heap.Flags=D3D12_HEAP_FLAGS(heap.Flags|D3D12_HEAP_FLAG_SHARED_CROSS_ADAPTER);
    Require(!detail::Shape(saved,desc,heap,allocation,Fixture::Gpu));
    desc.Flags=D3D12_RESOURCE_FLAGS(desc.Flags|D3D12_RESOURCE_FLAG_ALLOW_CROSS_ADAPTER);
    Require(detail::Shape(saved,desc,heap,allocation,Fixture::Gpu));
    Require(detail::Contract.size()==16);  // 2.5: see the Contract table
    std::puts("native breadcrumb write tests passed");return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
