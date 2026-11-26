#include "Render/NativeTargetLifetime.hpp"
#include <cstdio>
#include <map>
#include <source_location>
#include <stdexcept>
#include <string>

using namespace cvr::stereo::target_lifetime;
static void Require(bool value,std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("target lifetime assertion failed at line "+std::to_string(at.line()));
}
struct Memory {
    std::map<uint64_t,uint8_t> bytes;
    uint64_t changeAt{};uint32_t readCount{};
    template<class T> void Put(uint64_t address,const T& value){
        const auto* data=reinterpret_cast<const uint8_t*>(&value);
        for(size_t i=0;i<sizeof(value);++i)bytes[address+i]=data[i];
    }
    static bool Read(void* user,uint64_t address,void* output,size_t count){
        auto& self=*static_cast<Memory*>(user);auto* data=static_cast<uint8_t*>(output);
        for(size_t i=0;i<count;++i){const auto at=self.bytes.find(address+i);if(at==self.bytes.end())return false;data[i]=at->second;}
        if(address==self.changeAt && ++self.readCount==2)++data[0];
        return true;
    }
};
struct ResizeFixture {
    static constexpr uint64_t Base=0x140000000,Cet=0x180000000,Relay=Base-0x10000,Trampoline=Base-0x11000,Singleton=0x90000;
    Memory memory;
    void Hex(uint64_t at,const char* text){
        const auto digit=[](char c){return c>='0' && c<='9'?c-'0':c-'a'+10;};
        while(*text){memory.bytes[at++]=uint8_t(digit(text[0])*16+digit(text[1]));text+=2;}
    }
    ResizeFixture(bool hooked=true){
        // Exact installed PE bytes: CP2077 native resize and CET's complete
        // three-unwind-range wrapper plus ResetState. These are code fixtures.
        Hex(Base+detail::ResizeRva,
            "488bc44488482044894018895010894808555356574154415541564157488d68b84881ec08010000418ad9448bea448b"
            "f94c8b1d8443bc02488b4570448b1041ffca4969c2b0000000498db3387fc9004803f0488975b8448b4e2841ffc94969"
            "c1b00000004d8db3d8f102004c03f04c8975c08acbe8d2fcffff884424408ac8e89bec9aff89442448488b0d0435bb02"
            "488b11ff9250040000488b10488bc8ff921003000033db488d4e488d7b038bd7448d4301488b01483bd8480f43c3488b"
            "d8488d4908492bd075ea488b4e40488b01ff5040483bc37332488b4e40488b014c8b86a0000000488bd3ff504885c079"
            "078bc8e8a45603014533c083caff488b8ea0000000ff15a18f21024c8d6668498bdc488bcbe8921222004883c3084883"
            "ef0175ee498d4e08e8f3e99aff498bcee8771222004585ff0f84f50100004585ed0f84ec0100008a56318b5d608bcbe8"
            "14feffff488b0e4885c90f8496000000488b01488d542450ff9090000000488b0e488b014c8b50688b44247c89442428"
            "8b44244889442420458bcd458bc78b54246c41ffd28d88fbff8577f7c1fdffffff0f845702000085c079078bc8e8ea55"
            "0301488b0e48894c24484885c97407488b01ff500890488b05ef41bc02488b8818e9a801488b01448a4d68448bc3488d"
            "542448ff505890488d4c2448e8bb112200e93d0100004d8bf74d8bfc41bd03000000448b65588b742448418d45fec745"
            "800300000048836588004c89759044896598c7459c010001008975a08945a48365a8008365ac008945b00f1045800f11"
            "45c80f104d900f114dd80f1045a00f1145e8f20f104db0f20f114df889442450836424540083642458008944245c8944"
            "2460488b054341bc02488bb8a8c43b01488b07488b98d8000000498bcfe81a1122004c897c2438488d058ec724024889"
            "44243048836424280083642420004c8d4dc841b800100000488d542450488bcfffd385c079078bc8e8cf5403014983c7"
            "08b8010000004c2be80f852fffffff488b75b8488b0e48894c24484885c94c8b75c04c8d66687407488b01ff50089048"
            "8b05b640bc02488b8818e9a801488b01448a4d68448b4560488d542448ff505890488d4c2448e881102200448b7d5044"
            "8b6d586645897e4e6645896e5041806655c08a442440243f410846554533ff4c8b2e4d85ed7428498b4500488b78484a"
            "8d1cfe488d4b68e8401022004c8d4b684c8d05b5c62402418bd7498bcdffd70f57c0f30f7f442458418a4e5580e13fe8"
            "8ce99aff89442450c744245404000000807e3000740fb91d00000083f81c0f44c189442450488b050040bc02488b88a8"
            "c43b01488b014d8b4c24184c8d442450498b1424ff90a000000041ffc74983c4084183ff030f8264ffffff488b45708b"
            "08e8ae100000488bcee8b67d9aff4881c408010000415f415e415d415c5f5e5b5dc3");
        if(!hooked)return;
        memory.Put(Base+detail::ResizeRva,uint8_t(0xE9));
        memory.Put(Base+detail::ResizeRva+1,int32_t(Relay-(Base+detail::ResizeRva+5)));
        memory.Put(Relay,std::array<uint8_t,6>{0xFF,0x25,0,0,0,0});memory.Put(Relay+6,Cet+detail::CetResizeRva);
        IMAGE_DOS_HEADER dos{};dos.e_magic=IMAGE_DOS_SIGNATURE;dos.e_lfanew=0x100;memory.Put(Cet,dos);
        IMAGE_NT_HEADERS64 nt{};nt.Signature=IMAGE_NT_SIGNATURE;nt.FileHeader.Machine=IMAGE_FILE_MACHINE_AMD64;
        nt.FileHeader.TimeDateStamp=0x68D9142A;nt.OptionalHeader.Magic=IMAGE_NT_OPTIONAL_HDR64_MAGIC;
        nt.OptionalHeader.SizeOfImage=0x730000;memory.Put(Cet+0x100,nt);
        Hex(Cet+detail::CetResizeRva,
            "4c8bdc49895b1049896b18565741564881ec90000000488b1dc31b6b00410fb6f9418bf08bea448bf180bb2024000000"
            "0f8492000000488d0553755e004d897b08498943c84c8d3d44765e00488d05cd755e00c74424782f000000498943d8e8"
            "3cfaffff0f104c24704c897c24304c8d4c243048c744243854000000488d5424400f2844243041b802000000488b88d8"
            "010000660f7f442430f20f10842480000000f20f114424500f294c2440e82e18feff33d2488d8be8230000e8c0ecfdff"
            "4c8bbc24b0000000440fb6cf4c8b9310240000448bc68bd5418bce4c8d9c2490000000498b5b28498b6b30498be3415e"
            "5f5e49ffe2");
        Hex(Cet+detail::CetResetRva,
            "48895c241048896c241848897424205741544155415641574883ec20440fb6e24c8bf14533ed443869380f84e8000000"
            "488d995805000048895c2450488bcbff155b9d5a0085c0740cb905000000ff15549d5a00cc817b4cffffff7f7513c743"
            "4cfeffff7fb906000000ff15389d5a0090498dbea80500004c8dbfc0000000493bff74744883c7040f1f840000000000"
            "418bf5498bed44392f7e470f1f440000803d090f6d000074234885ed78053b770c7c1941b875080000488d15b0435e00"
            "488d0d79435e00e824040300488b4f14488b0ce9e8170e0200ffc648ffc53b377cbe488d4ffce8a5333c004883c74048"
            "8d47fc493bc77598e803f40200e8ee0f03004584e4740833c9e832b0380090488bcbff15809c5a00498b5650498b4e48"
            "483bca740de8e6c7ffff498b4648498946504c896c2450498bc549898688000000498b4e604885c9740b4d896e60488b"
            "01ff501090498b4e684885c9740b4d896e68488b01ff501090498b4e704885c9740b4d896e70488b01ff501090498b8e"
            "800000004885c9740e4d89ae80000000488b01ff501090498b4e784885c9740b4d896e78488b01ff50109041c6463800"
            "32c0488b5c2458488b6c2460488b7424684883c420415f415e415d415c5fc3");
        memory.Put(Cet+0x6DD8F0,Singleton);memory.Put(Singleton+0x2410,Trampoline);
        memory.Put(Trampoline,std::array<uint8_t,13>{0x48,0x8B,0xC4,0x44,0x88,0x48,0x20,0xFF,0x25,0,0,0,0});
        memory.Put(Trampoline+13,Base+detail::ResizeRva+7);
    }
    bool Read(detail::ResizeChain& output,uint32_t* failure=nullptr,uint64_t cet=Cet){
        return detail::ReadResizeChain(Memory::Read,&memory,Base,cet,output,failure);
    }
};
struct ShutdownFixture:ResizeFixture {
    static constexpr uint64_t ShutdownRelay=Relay-0x100,ShutdownTrampoline=Trampoline-0x100;
    ShutdownFixture(bool hooked=true):ResizeFixture(hooked){
        Hex(Base+detail::ShutdownRva,
            "40534883ec20488bd94881c198000000488d05ed2a0602488901e8911f79ff488d4b684c8d0dc2f413ffba0800000044"
            "8d42fbe8645a9500488d4b40e823579bff488d4b20e81a579bff488d4b084c8d0d97f413ffba08000000448d42fbe839"
            "5a9500488bcb4883c4205be9f4569bff");
        if(!hooked)return;
        memory.Put(Base+detail::ShutdownRva,uint8_t(0xE9));
        memory.Put(Base+detail::ShutdownRva+1,int32_t(ShutdownRelay-(Base+detail::ShutdownRva+5)));
        memory.Put(ShutdownRelay,std::array<uint8_t,6>{0xFF,0x25,0,0,0,0});
        memory.Put(ShutdownRelay+6,Cet+detail::CetShutdownRva);
        Hex(Cet+detail::CetShutdownRva,
            "48895c240848896c2410488974241848897c242041564883ec20488b1dbf1a6b00488bea4c8bf1b201498bf9498bf048"
            "8d8be8230000e845ecfdff488b83182400004c8bcf4c8bc6488bd5498bce488b5c2430488b6c2438488b742440488b7c"
            "24484883c420415e48ffe0");
        memory.Put(Singleton+0x2418,ShutdownTrampoline);
        memory.Put(ShutdownTrampoline,std::array<uint8_t,12>{0x40,0x53,0x48,0x83,0xEC,0x20,0xFF,0x25,0,0,0,0});
        memory.Put(ShutdownTrampoline+12,Base+detail::ShutdownRva+6);
    }
    bool Read(detail::ResizeChain& output,uint32_t* failure=nullptr,uint64_t cet=Cet){
        return detail::ReadShutdownChain(Memory::Read,&memory,Base,cet,output,failure);
    }
};
struct Fixture {
    static constexpr uint64_t Base=0x140000000,Registry=0x20000000,Context=0x10000,List=0x20000;
    static constexpr uint64_t Resource=0x30000,Heap=0x40000,Owner=0x50000,Allocation=0x60000,Allocator=0x70000,Pimpl=0x80000;
    static constexpr uint64_t Bytes=0x800000,Alignment=0x10000,Record=Registry+0x2F1D8,Retirement=Base+detail::RetirementRva;
    static constexpr uint64_t Swapchains=Registry+detail::SwapchainTableOffset,RegistryLock=Registry+detail::RegistryLockOffset;
    static constexpr uint32_t Index=11;
    static constexpr uint64_t Bucket=Retirement+0x200C0+24ull*Index;
    Memory memory;detail::Key key{Registry,Context,List,Resource,Heap,Bytes,Alignment,1};
    Fixture(){
        memory.Put(Base+detail::RegistryRva,Registry);memory.Put(Registry+1,uint8_t(0));
        memory.Put(Registry+0x1410730,uint32_t(8));memory.Put(Registry+0x1410734,uint8_t(0));
        memory.Put(Retirement+0x20CD0,uint8_t(0));memory.Put(Retirement+0x20CC0,Index);
        memory.Put(Bucket,uint64_t(0));memory.Put(Bucket+8,uint64_t(0));memory.Put(Bucket+16,uint32_t(0));
        memory.Put(Retirement,uint64_t(1000));memory.Put(Retirement+0x20040,uint64_t(1000));memory.Put(Retirement+0x20080,uint64_t(1000));
        memory.Put(Context+0x30,List);memory.Put(Context+0x68,uint32_t(1));
        memory.Put(Swapchains,RegistryLock);memory.Put(Swapchains+8,uint32_t(detail::SwapchainCount));memory.Put(RegistryLock,uint8_t(0));
        for(uint32_t i=0;i<detail::SwapchainCount;++i){
            memory.Put(Swapchains+0x10+uint64_t(i)*0xB0,int32_t(-1));
            memory.Put(Swapchains+0x40+uint64_t(i)*0xB0,uint32_t(0));
        }
        memory.Put(Record,Resource);memory.Put(Record+8,uint8_t(2));memory.Put(Record+0x10,Owner);memory.Put(Record-8,int32_t(1));
        memory.Put(Owner,std::array<uint8_t,0x38>{});memory.Put(Owner,int32_t(2));memory.Put(Owner+8,Allocation);
        memory.Put(Owner+0x18,Alignment);memory.Put(Owner+0x20,Bytes);
        memory.Put(Allocation,std::array<uint8_t,0x68>{});memory.Put(Allocation,Base+cvr::stereo::source_cbv_details::detail::AllocationVtable);
        memory.Put(Allocation+8,uint32_t(1));memory.Put(Allocation+0x10,Pimpl);memory.Put(Allocation+0x18,Bytes);
        memory.Put(Allocation+0x58,Heap);memory.Put(Allocation+0x60,uint32_t(2));
        memory.Put(Registry+0x1410540,Allocator);memory.Put(Allocator,std::array<uint8_t,0x18>{});
        memory.Put(Allocator,Base+cvr::stereo::source_cbv_details::detail::AllocatorVtable);
        memory.Put(Allocator+8,uint32_t(1));memory.Put(Allocator+0x10,Pimpl);
        memory.Put(Base+cvr::stereo::source_cbv_details::detail::AllocationVtable,
            std::array<uint64_t,5>{Base+0x2FAC534,Base+0x2FAAF80,Base+0x348220,Base+0x3483A4,Base+0x348244});
        memory.Put(Base+cvr::stereo::source_cbv_details::detail::AllocatorVtable,
            std::array<uint64_t,5>{Base+0x2FAC534,Base+0x2FAAF80,Base+0x348220,Base+0x18F3FB4,Base+0x18F3F48});
    }
    bool Read(detail::Snapshot& output,bool source=true,uint32_t* failure=nullptr){
        return detail::ReadSnapshot(Memory::Read,&memory,Base,key,source,output,failure);
    }
    void Retire(uint64_t sequence=1000){
        memory.Put(Record-8,int32_t(0));memory.Put(Bucket+16,uint32_t(1));
        memory.Put(Retirement,sequence+1);memory.Put(Retirement+0x20080,sequence+1);memory.Put(Retirement+0x20040,sequence);
        memory.Put(Retirement+8+4*(sequence%detail::RingSize),uint32_t(1));
    }
};
int main()try{
    detail::ResizeChain chain;uint32_t chainCode=UINT32_MAX;
    ResizeFixture untouched(false);Require(untouched.Read(chain,&chainCode,0) && !chainCode && chain.mode==1);
    ResizeFixture changingOriginal(false);changingOriginal.memory.changeAt=ResizeFixture::Base+detail::ResizeRva;
    Require(!changingOriginal.Read(chain,&chainCode,0) && chainCode==11 && changingOriginal.memory.readCount==2);
    ResizeFixture hooked;Require(hooked.Read(chain,&chainCode) && !chainCode && chain.mode==2);
    Require(chain.relay==ResizeFixture::Relay && chain.cet==ResizeFixture::Cet &&
        chain.singleton==ResizeFixture::Singleton && chain.trampoline==ResizeFixture::Trampoline);
    const auto rejectChain=[](auto mutate,uint32_t expected){ResizeFixture value;mutate(value);detail::ResizeChain result;uint32_t failed{};
        Require(!value.Read(result,&failed) && failed==expected);};
    rejectChain([](auto& v){v.memory.bytes.erase(ResizeFixture::Base+detail::ResizeRva+100);},1);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Base+detail::ResizeRva,uint8_t(0xCC));},2);
    rejectChain([](auto& v){v.memory.bytes[ResizeFixture::Base+detail::ResizeRva+8]^=1;},2);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Relay,uint8_t(0xE9));},4);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Relay+2,uint32_t(1));},4);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Relay+6,ResizeFixture::Cet+detail::CetResizeRva+1);},5);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Cet,uint16_t(0));},5);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Cet+offsetof(IMAGE_DOS_HEADER,e_lfanew),int32_t(0x1001));},5);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Cet+0x100+offsetof(IMAGE_NT_HEADERS64,FileHeader)+offsetof(IMAGE_FILE_HEADER,TimeDateStamp),uint32_t(0));},5);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Cet+0x100+offsetof(IMAGE_NT_HEADERS64,OptionalHeader)+offsetof(IMAGE_OPTIONAL_HEADER64,SizeOfImage),uint32_t(0x740000));},5);
    rejectChain([](auto& v){v.memory.bytes[ResizeFixture::Cet+detail::CetResizeRva+62]^=1;},6);
    rejectChain([](auto& v){v.memory.bytes[ResizeFixture::Cet+detail::CetResizeRva+244]^=1;},6);
    rejectChain([](auto& v){v.memory.bytes[ResizeFixture::Cet+detail::CetResetRva+100]^=1;},7);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Cet+0x6DD8F0,uint64_t(0));},8);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Singleton+0x2410,uint64_t(0));},9);
    rejectChain([](auto& v){v.memory.bytes[ResizeFixture::Trampoline+6]^=1;},10);
    rejectChain([](auto& v){v.memory.bytes[ResizeFixture::Trampoline+8]^=1;},10);
    rejectChain([](auto& v){v.memory.Put(ResizeFixture::Trampoline+13,ResizeFixture::Base+detail::ResizeRva+5);},10);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Base+detail::ResizeRva;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Relay;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Cet;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Cet+0x100;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Cet+detail::CetResizeRva;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Cet+detail::CetResetRva;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Cet+0x6DD8F0;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Singleton+0x2410;},11);
    rejectChain([](auto& v){v.memory.changeAt=ResizeFixture::Trampoline;},11);
    Require(!hooked.Read(chain,&chainCode,ResizeFixture::Cet+8) && chainCode==5);
    uint64_t relative{};Require(detail::RelativeTarget(100,-99,relative) && relative==1);
    Require(!detail::RelativeTarget(100,-100,relative));Require(!detail::RelativeTarget(100,INT32_MIN,relative));
    Require(!detail::RelativeTarget(UINT64_MAX,1,relative));
    ShutdownFixture shutdown;Require(shutdown.Read(chain,&chainCode) && !chainCode && chain.mode==2);
    Require(chain.relay==ShutdownFixture::ShutdownRelay && chain.trampoline==ShutdownFixture::ShutdownTrampoline);
    Require(detail::ReadResizeChain(Memory::Read,&shutdown.memory,ShutdownFixture::Base,ShutdownFixture::Cet,chain));
    ShutdownFixture originalShutdown(false);Require(originalShutdown.Read(chain,&chainCode,0) && !chainCode && chain.mode==1);
    ShutdownFixture changingShutdown(false);changingShutdown.memory.changeAt=ShutdownFixture::Base+detail::ShutdownRva;
    Require(!changingShutdown.Read(chain,&chainCode,0) && chainCode==11 && changingShutdown.memory.readCount==2);
    const auto rejectShutdown=[](auto mutate,uint32_t expected){ShutdownFixture value;mutate(value);detail::ResizeChain result;uint32_t failed{};
        Require(!value.Read(result,&failed) && failed==expected);};
    rejectShutdown([](auto& v){v.memory.bytes.erase(ShutdownFixture::Base+detail::ShutdownRva+111);},1);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::Base+detail::ShutdownRva,uint8_t(0xCC));},2);
    rejectShutdown([](auto& v){v.memory.bytes[ShutdownFixture::Base+detail::ShutdownRva+5]^=1;},2);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::ShutdownRelay,uint8_t(0xE9));},4);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::ShutdownRelay+6,ShutdownFixture::Cet+detail::CetResizeRva);},5);
    rejectShutdown([](auto& v){v.memory.bytes[ShutdownFixture::Cet+detail::CetShutdownRva+106]^=1;},6);
    rejectShutdown([](auto& v){v.memory.bytes[ShutdownFixture::Cet+detail::CetResetRva+462]^=1;},7);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::Cet+0x6DD8F0,uint64_t(0));},8);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::Singleton+0x2418,uint64_t(0));},9);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::Singleton+0x2418,ShutdownFixture::Trampoline);},10);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::ShutdownTrampoline+6,uint8_t(0x20));},10);
    rejectShutdown([](auto& v){v.memory.Put(ShutdownFixture::ShutdownTrampoline+12,ShutdownFixture::Base+detail::ShutdownRva+7);},10);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Base+detail::ShutdownRva;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::ShutdownRelay;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Cet;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Cet+0x100;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Cet+detail::CetShutdownRva;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Cet+detail::CetResetRva;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Cet+0x6DD8F0;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::Singleton+0x2418;},11);
    rejectShutdown([](auto& v){v.memory.changeAt=ShutdownFixture::ShutdownTrampoline;},11);
    Fixture f;detail::Snapshot source,current;uint32_t code=UINT32_MAX;
    Require(f.Read(source,true,&code) && !code && source.owner==Fixture::Owner && source.queueSlot==UINT64_MAX);
    Require(f.Read(current) && detail::SameIdentity(source,current));
    f.memory.Put(Fixture::Record-8,int32_t(4));f.memory.Put(Fixture::Owner,int32_t(3));
    f.memory.Put(Fixture::Allocation+8,uint32_t(9));f.memory.Put(Fixture::Allocator+8,uint32_t(7));
    Require(f.Read(current) && detail::SameIdentity(source,current));
    f.Retire();Require(f.Read(current,false,&code) && !code && current.queueSlot==1000 && detail::SameIdentity(source,current));
    Require(!f.Read(current,true,&code) && code==5);
    Fixture wrap;wrap.Retire(detail::RingSize*3ull-1);Require(wrap.Read(current,false) && current.queueSlot==detail::RingSize*3ull-1);
    const auto reject=[](auto mutate,uint32_t expected,bool atSource=true){
        Fixture value;mutate(value);detail::Snapshot result;uint32_t failure{};
        Require(!value.Read(result,atSource,&failure) && failure==expected);
    };
    reject([](auto& v){v.key.handle=0;},1);
    reject([](auto& v){v.key.handle=32769;},1);
    reject([](auto& v){v.key.bytes=UINT64_MAX;},1);
    reject([](auto& v){v.memory.Put(Fixture::Base+detail::RegistryRva,Fixture::Registry+8);},2);
    reject([](auto& v){v.memory.Put(Fixture::Registry+1,uint8_t(1));},2);
    reject([](auto& v){v.memory.Put(Fixture::Registry+0x1410730,uint32_t(0));},2);
    reject([](auto& v){v.memory.Put(Fixture::Registry+0x1410734,uint8_t(1));},2);
    reject([](auto& v){v.memory.Put(Fixture::Retirement+0x20CD0,uint8_t(1));},2);
    reject([](auto& v){v.memory.Put(Fixture::Retirement+0x20CC0,uint32_t(128));},2);
    reject([](auto& v){v.memory.Put(Fixture::Bucket,uint64_t(9));},2);
    reject([](auto& v){v.memory.Put(Fixture::Bucket,Fixture::Bucket);},2);
    reject([](auto& v){v.memory.Put(Fixture::Bucket+8,uint64_t(9));},2);
    reject([](auto& v){v.memory.Put(Fixture::Bucket+16,uint32_t(1));},2);
    reject([](auto& v){v.memory.Put(Fixture::Retirement,uint64_t(1001));},2);
    reject([](auto& v){v.memory.Put(Fixture::Retirement+0x20040,uint64_t(1001));},2);
    reject([](auto& v){v.memory.Put(Fixture::Context+0x30,Fixture::List+8);},3);
    reject([](auto& v){v.memory.Put(Fixture::Context+0x68,uint32_t(3));},3);
    reject([](auto& v){v.memory.Put(Fixture::Record,Fixture::Resource+8);},4);
    reject([](auto& v){v.memory.Put(Fixture::Record+8,uint8_t(1));},4);
    reject([](auto& v){v.memory.Put(Fixture::Record+0x10,uint64_t(0));},4);
    reject([](auto& v){v.memory.Put(Fixture::Record-8,int32_t(-1));},5);
    reject([](auto& v){v.memory.Put(Fixture::Record-8,int32_t(INT32_MAX));},5);
    reject([](auto& v){v.memory.Put(Fixture::Owner,int32_t(0));},6);
    reject([](auto& v){v.memory.Put(Fixture::Owner+8,uint64_t(0));},6);
    reject([](auto& v){v.memory.Put(Fixture::Owner+0x18,uint64_t(0x400000));},6);
    reject([](auto& v){v.memory.Put(Fixture::Allocation+8,uint32_t(0));},7);
    reject([](auto& v){v.memory.Put(Fixture::Allocation+0x60,uint32_t(0));},7);
    reject([](auto& v){v.memory.Put(Fixture::Allocation+0x28,Fixture::Resource);},7);
    reject([](auto& v){v.memory.Put(Fixture::Allocation+0x58,Fixture::Heap+8);},7);
    reject([](auto& v){v.memory.Put(Fixture::Allocation+0x18,Fixture::Bytes-65536);},7);
    reject([](auto& v){v.memory.Put(Fixture::Owner+0x20,Fixture::Bytes+65536);},7);
    reject([](auto& v){v.memory.Put(Fixture::Allocator+0x10,Fixture::Pimpl+8);},8);
    reject([](auto& v){v.memory.Put(Fixture::Allocator+8,uint32_t(0));},8);
    reject([](auto& v){v.memory.Put(Fixture::Record-8,int32_t(0));},9,false);
    reject([](auto& v){v.Retire();v.memory.Put(Fixture::Retirement+8+4*1000,uint32_t(2));},9,false);
    // An older pending queue entry is not a current-epoch witness.
    reject([](auto& v){v.Retire();v.memory.Put(Fixture::Bucket+16,uint32_t(0));},9,false);
    reject([](auto& v){v.Retire();v.memory.Put(Fixture::Bucket+16,uint32_t(2));
        v.memory.Put(Fixture::Retirement+0x20040,uint64_t(999));v.memory.Put(Fixture::Retirement+8+4*999,uint32_t(1));},9,false);
    reject([](auto& v){v.memory.changeAt=Fixture::Registry+0x1410730;},10);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains,Fixture::RegistryLock+8);},11);
    reject([](auto& v){v.memory.Put(Fixture::RegistryLock,uint8_t(1));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+8,uint32_t(33));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+8,uint32_t(31));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+0x10,int32_t(-2));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+0x10,int32_t(INT32_MAX));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+0x40,uint32_t(32769));},11);
    // Reject the watched handle even in an inactive table slot.
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+0x40,uint32_t(1));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+8,uint32_t(31));
        v.memory.Put(Fixture::Swapchains+0x10,int32_t(1));v.memory.Put(Fixture::Swapchains+0x40,uint32_t(1));},11);
    reject([](auto& v){v.memory.Put(Fixture::Swapchains+0x40+31*0xB0,uint32_t(1));},11);
    reject([](auto& v){v.memory.changeAt=Fixture::Swapchains;},11);
    reject([](auto& v){v.memory.changeAt=Fixture::Swapchains+0x40;},11);
    Fixture unrelated;
    unrelated.memory.Put(Fixture::Swapchains+8,uint32_t(31));unrelated.memory.Put(Fixture::Swapchains+0x10,int32_t(1));
    unrelated.memory.Put(Fixture::Swapchains+0x40,uint32_t(9));Require(unrelated.Read(current));
    Require(!detail::SameIdentity(source,current));
    Fixture changed;Require(changed.Read(current));
    auto altered=current;altered.owner+=8;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.epoch.index++;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.epoch.bucket+=24;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.control[0x20]^=1;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.allocation.raw[0x58]^=1;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.allocator.raw[0x10]^=1;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.swapchains.lock+=8;Require(!detail::SameIdentity(current,altered));
    altered=current;altered.swapchains.handles[31]=19;Require(!detail::SameIdentity(current,altered));
    Require(detail::Contract.size()==46);  // 2.5: see the Contract table
    std::puts("native target borrowed lifetime tests passed");return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
