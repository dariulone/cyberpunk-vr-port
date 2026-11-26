// Bounded read-only search around known native allocations. No injected/native
// calls, AddRef, suspension, memory writes, free, save, launch or runtime change.
#include <windows.h>
#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <chrono>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <functional>
#include <iomanip>
#include <iostream>
#include <optional>
#include <set>
#include <sstream>
#include <stdexcept>
#include <unordered_map>
#include <vector>
using J=nlohmann::json;using U=uint64_t;
using Read=std::function<bool(U,void*,size_t)>;
template<class T>T At(const void* p,size_t offset=0){T out{};std::memcpy(&out,static_cast<const char*>(p)+offset,sizeof(out));return out;}
template<class T>bool Get(const Read& read,U address,T& out){return read(address,&out,sizeof(out));}
std::string Hex(U value){std::ostringstream out;out<<"0x"<<std::hex<<value;return out.str();}
U Pointer(const J& value){if(!value.is_string())return 0;const auto text=value.get<std::string>();if(!text.starts_with("0x"))return 0;return std::stoull(text,nullptr,16);}
struct Texture {U address{},control{},backing{};uint32_t width{},height{},strong{},weak{};bool stable{};};
using Targets=std::unordered_map<U,std::pair<uint32_t,uint32_t>>;
std::optional<Texture> Inspect(const Read& read,U address,U base,const Targets& targets){
    std::array<uint8_t,0xA8> data{},again{};
    if(!read(address,data.data(),data.size()) || At<U>(data.data())!=base+0x3064700 || At<U>(data.data(),8)!=address)return {};
    const auto backing=At<U>(data.data(),0x70);const auto found=targets.find(backing);
    if(found==targets.end() || At<U>(data.data(),0x78)!=backing)return {};
    const auto width=At<uint32_t>(data.data(),0x40),height=At<uint32_t>(data.data(),0x44);
    if(found->second!=std::pair{width,height})return {};
    const auto control=At<U>(data.data(),0x10);std::array<uint32_t,2> counts{},last{};
    if(!control || !Get(read,control,counts) || !counts[0] || counts[0]>1000000)return {};
    const bool stable=read(address,again.data(),again.size()) && Get(read,control,last) && last[0]>0 &&
        !std::memcmp(data.data(),again.data(),0x18) && !std::memcmp(data.data()+0x40,again.data()+0x40,8) &&
        !std::memcmp(data.data()+0x70,again.data()+0x70,16);
    return Texture{address,control,backing,width,height,counts[0],counts[1],stable};
}
struct Process {
    HANDLE handle{};U limit{},readBytes{},attemptedBytes{};size_t failedReads{};bool exhausted{};
    explicit Process(DWORD pid,U maximum):limit(maximum){handle=OpenProcess(PROCESS_QUERY_INFORMATION|PROCESS_VM_READ,FALSE,pid);if(!handle)throw std::runtime_error("OpenProcess query/read failed");}
    ~Process(){if(handle)CloseHandle(handle);}
    bool ReadAt(U address,void* out,size_t bytes){
        if(readBytes+bytes>limit){exhausted=true;return false;}
        attemptedBytes+=bytes;SIZE_T copied{};
        const bool ok=ReadProcessMemory(handle,reinterpret_cast<void*>(address),out,bytes,&copied) && copied==bytes;
        readBytes+=copied;if(!ok)++failedReads;return ok;
    }
};
struct Seeds {std::set<U> textures,owners,windowOwners,windowTables;size_t nodes{};};
void CollectSeeds(const J& value,Seeds& out,U base,unsigned depth=0){
    if(++out.nodes>200000 || depth>48)throw std::runtime_error("input report exceeds structural bound");
    if(value.is_object()){
        if(value.contains("address") && value.contains("backings") && value.contains("width"))if(U p=Pointer(value["address"]))out.textures.insert(p);
        for(const auto key:{"window","mainWindow","layer","slotWidget","rootWidget","controller","system","manager","entries"})
            if(value.contains(key))if(U p=Pointer(value[key]))out.owners.insert(p);
        for(const auto key:{"window","mainWindow"})if(value.contains(key))if(U p=Pointer(value[key]))out.windowOwners.insert(p);
        if(value.contains("address") && value.contains("kind") && value.contains("texture"))if(U p=Pointer(value["address"]))out.windowOwners.insert(p);
        if(value.contains("address") && (value.contains("nameHash") || value.contains("component")))if(U p=Pointer(value["address"]))out.owners.insert(p);
        if(value.contains("windowVtable"))if(U p=Pointer(value["windowVtable"]))out.windowTables.insert(p);
        if(value.contains("kind") && value.contains("texture") && value.contains("vtableRva"))if(U p=Pointer(value["vtableRva"]))out.windowTables.insert(base+p);
        for(const auto& item:value.items())CollectSeeds(item.value(),out,base,depth+1);
    }else if(value.is_array())for(const auto& item:value)CollectSeeds(item,out,base,depth+1);
}
struct Pages {std::vector<U> addresses;U bytes{};size_t rejectedSeeds{},truncatedPages{};};
Pages KnownPages(HANDLE process,const std::set<U>& seeds,U radius,U maxBytes,const std::set<U>* priority=nullptr){
    std::set<U> unique;Pages result;
    std::vector<U> ordered;if(priority)ordered.insert(ordered.end(),priority->begin(),priority->end());
    for(U seed:seeds)if(!priority || !priority->contains(seed))ordered.push_back(seed);
    for(U seed:ordered){MEMORY_BASIC_INFORMATION info{};
        if(!VirtualQueryEx(process,reinterpret_cast<void*>(seed),&info,sizeof(info)) || info.State!=MEM_COMMIT || info.Type!=MEM_PRIVATE ||
           info.Protect&(PAGE_NOACCESS|PAGE_GUARD) || !(info.Protect&(PAGE_READWRITE|PAGE_WRITECOPY|PAGE_READONLY))){++result.rejectedSeeds;continue;}
        const U base=reinterpret_cast<U>(info.BaseAddress),end=base+info.RegionSize,center=seed&~U(4095);
        const U first=std::max(base,center>radius?center-radius:U(0)),last=std::min(end,center+radius+4096);
        for(U page=first;page+4096<=last;page+=4096){if(unique.contains(page))continue;if(unique.size()*4096>=maxBytes){++result.truncatedPages;continue;}unique.insert(page);}
    }
    result.addresses.assign(unique.begin(),unique.end());result.bytes=result.addresses.size()*4096;return result;
}
template<class F>void ScanPages(Process& process,const Pages& pages,F&& visit){
    std::array<uint8_t,4096> buffer{};
    for(U page:pages.addresses){if(process.exhausted)break;if(!process.ReadAt(page,buffer.data(),buffer.size()))continue;
        for(size_t i=0;i<buffer.size();i+=8)visit(page+i,At<U>(buffer.data(),i));}
}
J Describe(const Pages& pages){return {{"plannedPages",pages.addresses.size()},{"plannedBytes",pages.bytes},{"rejectedSeeds",pages.rejectedSeeds},{"truncatedPages",pages.truncatedPages}};}
void Check(bool condition,const char* message){if(!condition)throw std::runtime_error(message);}
void SelfTest(){
    std::vector<uint8_t> memory(0x10000);constexpr U base=0x140000000,object=0x1000,control=0x3000,backing=0x7000;
    auto put=[&](size_t offset,auto value){std::memcpy(memory.data()+offset,&value,sizeof(value));};
    put(object,base+0x3064700);put(object+8,object);put(object+16,control);put(object+0x40,uint32_t(768));put(object+0x44,uint32_t(768));
    put(object+0x70,backing);put(object+0x78,backing);put(control,uint32_t(2));put(control+4,uint32_t(1));
    Read read=[&](U address,void* out,size_t bytes){if(address+bytes>memory.size())return false;std::memcpy(out,memory.data()+address,bytes);return true;};
    Targets targets{{backing,{768,768}}};const auto found=Inspect(read,object,base,targets);
    Check(found && found->stable && found->strong==2 && found->backing==backing,"verified native texture not identified");
    put(object+0x78,backing+8);Check(!Inspect(read,object,base,targets),"unrelated native backing accepted");put(object+0x78,backing);
    put(object+0x44,uint32_t(1024));Check(!Inspect(read,object,base,targets),"wrong dimensions accepted");put(object+0x44,uint32_t(768));
    put(control,uint32_t(0));Check(!Inspect(read,object,base,targets),"destroyed native object accepted");put(control,uint32_t(2));
    put(object+8,object+8);Check(!Inspect(read,object,base,targets),"allocator residue without self-reference accepted");
    std::cout<<"PASS bounded native identity, dimensions, self/control reference and dead-object rejection\n";
}
int main(int argc,char** argv)try{
    if(argc==2 && std::string(argv[1])=="--self-test"){SelfTest();return 0;}
    if(argc<3 || argc>5)throw std::runtime_error("usage: resource_owner_scan input-native-report.json output.json [radiusKiB=64] [maxReadMiB=64]");
    const U radius=argc>3?std::stoull(argv[3])*1024:65536,maximum=argc>4?std::stoull(argv[4])*1024*1024:64*1024*1024;
    Check(radius>=4096 && radius<=1024*1024 && maximum>=1024*1024 && maximum<=256*1024*1024,"scan bounds exceeded");
    std::ifstream input(argv[1]);J report;input>>report;const DWORD pid=report.at("pid");const U base=Pointer(report.at("moduleBase"));
    Check(base && report.value("peTimestamp",0U)==1792368047,"input executable identity is not verified 2.5");
    Process process(pid,maximum);Read read=[&](U address,void* out,size_t bytes){return process.ReadAt(address,out,bytes);};
    wchar_t imagePath[32768]{};DWORD length=32768;Check(QueryFullProcessImageNameW(process.handle,0,imagePath,&length)!=0,"cannot identify PID executable");
    const std::wstring path(imagePath,length);Check(path.ends_with(L"\\Cyberpunk2077.exe"),"PID is not Cyberpunk2077.exe");
    std::array<uint8_t,64> dos{};Check(Get(read,base,dos) && At<uint16_t>(dos.data())==0x5A4D,"mapped image missing DOS header");
    const auto pe=At<uint32_t>(dos.data(),0x3C);Check(pe>=64 && pe<=1024*1024,"invalid PE offset");
    std::array<uint8_t,104> header{};Check(Get(read,base+pe,header) && At<uint32_t>(header.data())==0x4550 &&
        At<uint32_t>(header.data(),8)==1792368047 && At<uint32_t>(header.data(),80)==0x581B000,"mapped game PE mismatch");
    Targets targets;
    for(const auto& row:report.at("rows"))if(row.value("nativeFormat",0)==40 && row.value("nativeFlags",0)==131 && row.value("mipSampleBits",0)==4 && row.value("stable",false)){
        const uint32_t width=row.at("width"),height=row.at("height");if((width!=768 && width!=1024)||width!=height)continue;
        for(const auto& backing:row.at("backings"))if(backing.value("stable",false))targets.emplace(Pointer(backing.at("address")),std::pair{width,height});
    }
    Check(!targets.empty(),"no stable target backing identities in report");Seeds seeds;CollectSeeds(report,seeds,base);Check(!seeds.textures.empty(),"no verified DynamicTexture allocation seeds; pass --hud report");
    const auto started=std::chrono::steady_clock::now();auto texturePages=KnownPages(process.handle,seeds.textures,radius,maximum/2);
    std::vector<Texture> textures;std::set<U> seen,backingReferenceAddresses;J backingReferences=J::array();size_t droppedBackingReferences=0;
    auto noteBacking=[&](U address,U value){
        if(!targets.contains(value) || !backingReferenceAddresses.insert(address).second)return;
        if(backingReferences.size()>=4096){++droppedBackingReferences;return;}
        J row={{"address",Hex(address)},{"backing",Hex(value)}};
        for(U offset:{U(0x70),U(0x78)})if(address>=offset){U table{};if(Get(read,address-offset,table) && table==base+0x3064700){row["dynamicTextureCandidate"]=Hex(address-offset);row["field"]=Hex(offset);break;}}
        std::array<U,17> context{};
        if(address>=64 && Get(read,address-64,context)){row["contextBase"]=Hex(address-64);row["contextWords"]=J::array();for(U word:context)row["contextWords"].push_back(Hex(word));}
        backingReferences.push_back(std::move(row));
    };
    ScanPages(process,texturePages,[&](U address,U value){noteBacking(address,value);if(value==base+0x3064700 && !seen.contains(address))if(auto found=Inspect(read,address,base,targets)){textures.push_back(*found);seen.insert(address);}});
    J output={{"pid",pid},{"moduleBase",Hex(base)},{"radiusBytes",radius},{"maxReadBytes",maximum},{"targetBackings",targets.size()},
        {"knownTextureSeeds",seeds.textures.size()},{"knownOwnerSeeds",seeds.owners.size()},{"textureCoverage",Describe(texturePages)},
        {"textures",J::array()},{"references",J::array()},{"windowReferences",J::array()},
        {"scope","only known allocation neighborhoods; handle pairs may be weak; no full-heap completeness claim or native/COM calls"}};
    std::unordered_map<U,const Texture*> selected;for(const auto& texture:textures){selected.emplace(texture.address,&texture);output["textures"].push_back({
        {"address",Hex(texture.address)},{"control",Hex(texture.control)},{"backing",Hex(texture.backing)},{"width",texture.width},{"height",texture.height},
        {"strongRefs",texture.strong},{"weakRefs",texture.weak},{"stable",texture.stable}});}
    // Native handle/token allocations may live beside the texture/control
    // storage rather than beside a Window object. Follow only these proven
    // allocation neighborhoods before the remaining known UI seeds.
    auto ownerPriority=seeds.windowOwners;
    for(const auto& texture:textures){seeds.owners.insert(texture.address);seeds.owners.insert(texture.control);
        ownerPriority.insert(texture.address);ownerPriority.insert(texture.control);}
    for(const auto& [backing,dimensions]:targets){(void)dimensions;seeds.owners.insert(backing);ownerPriority.insert(backing);}
    auto ownerPages=KnownPages(process.handle,seeds.owners,radius,maximum/2,&ownerPriority);output["ownerCoverage"]=Describe(ownerPages);
    output["priorityKnownAllocationSeeds"]=ownerPriority.size();
    output["priorityWindowSeeds"]=seeds.windowOwners.size();
    std::unordered_map<U,U> windows;size_t droppedReferences=0;
    ScanPages(process,ownerPages,[&](U address,U value){noteBacking(address,value);const auto it=selected.find(value);if(it==selected.end() || address==value+8)return;
        if(output["references"].size()>=4096){++droppedReferences;return;}U next{};Get(read,address+8,next);
        J row={{"address",Hex(address)},{"target",Hex(value)},{"handlePair",next==it->second->control},{"next",Hex(next)}};
        if(address>=0x4E8){const U window=address-0x4E8;U table{};
            if(Get(read,window,table) && seeds.windowTables.contains(table)){std::array<uint8_t,0x500> data{};
                if(Get(read,window,data) && At<U>(data.data(),8)==window){const U control=At<U>(data.data(),16);std::array<uint32_t,2> refs{};
                    if(control && Get(read,control,refs) && refs[0]){windows.emplace(window,control);row["virtualWindow"]={{"address",Hex(window)},{"control",Hex(control)},
                        {"strongRefs",refs[0]},{"weakRefs",refs[1]},{"nameHash",Hex(At<U>(data.data(),0x188))},{"kind",data[800]},{"visible",bool(data[0x1F8])}};}}}}
        output["references"].push_back(std::move(row));});
    if(!windows.empty())ScanPages(process,ownerPages,[&](U address,U value){const auto it=windows.find(value);if(it==windows.end() || address==value+8)return;
        if(output["windowReferences"].size()>=4096){++droppedReferences;return;}U next{};Get(read,address+8,next);
        output["windowReferences"].push_back({{"address",Hex(address)},{"target",Hex(value)},{"handlePair",next==it->second},{"next",Hex(next)}});});
    output["readBytes"]=process.readBytes;output["attemptedReadBytes"]=process.attemptedBytes;output["failedReads"]=process.failedReads;
    output["budgetExhausted"]=process.exhausted;output["droppedReferenceRows"]=droppedReferences;
    output["backingReferences"]=std::move(backingReferences);output["droppedBackingReferenceRows"]=droppedBackingReferences;
    output["elapsedMs"]=std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now()-started).count();
    std::ofstream file(argv[2]);Check(bool(file),"output file unavailable");file<<output.dump(2)<<'\n';Check(bool(file),"output write failed");
    std::cout<<J{{"textures",textures.size()},{"references",output["references"].size()},{"windows",windows.size()},
        {"readBytes",process.readBytes},{"budgetExhausted",process.exhausted},{"elapsedMs",output["elapsedMs"]}}.dump()<<'\n';return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}
