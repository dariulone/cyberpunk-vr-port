#include "Render/VramCensus.hpp"
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cstring>
#include <new>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);

namespace cvr::vram_census {
namespace {
// The private-data key the tracker rides on.
constexpr GUID kKey{0x6c1f0a52,0x3d2e,0x4b8a,{0x9e,0x41,0x7a,0x2f,0x5b,0x0c,0x93,0xd4}};
enum Creator : unsigned {Game,Port,Other,CreatorCount};
enum Placement : unsigned {Committed,Placed,Heap,PlacementCount};
enum HeapKind : unsigned {Default,Upload,Readback,Custom,HeapKindCount};
enum Class : unsigned {Buffer,Target,Texture,ClassCount};
constexpr unsigned kSlots=CreatorCount*PlacementCount*HeapKindCount*ClassCount;
unsigned Slot(unsigned c,unsigned p,unsigned h,unsigned k){return ((c*PlacementCount+p)*HeapKindCount+h)*ClassCount+k;}
// Atomics and plain arrays only at namespace scope: nothing here may run a destructor at process exit, when the
// trackers of objects the runtime destroys late still come back into this file.
std::atomic<int64_t> liveBytes[kSlots]{},liveCount[kSlots]{};
int64_t lastBytes[kSlots]{};
bool haveLast=false;
std::atomic<bool> enabled{true};

struct SiteKey {
    uint32_t rva[4]{};uint32_t kind{};
    bool operator==(const SiteKey& o) const {return std::memcmp(this,&o,sizeof(*this))==0;}
};
struct SiteHash {size_t operator()(const SiteKey& k) const {
    uint64_t h=1469598103934665603ull;const auto* b=reinterpret_cast<const unsigned char*>(&k);
    for(size_t i=0;i<sizeof(k);++i){h^=b[i];h*=1099511628211ull;}return size_t(h);}};
struct Site {SiteKey key;std::atomic<int64_t> bytes{},count{};int64_t last{};};
SRWLOCK siteLock=SRWLOCK_INIT;
// Never destroyed (see above); the sites are never erased, so a tracker's pointer to its site stays valid.
auto& sites=*new std::unordered_map<SiteKey,Site*,SiteHash>();
constexpr size_t kSiteLimit=4096;
Site& overflow=*new Site{};

class Tracker final : public IUnknown {
    std::atomic<ULONG> refs{1};
    unsigned slot;int64_t bytes;Site* site;
public:
    Tracker(unsigned s,int64_t b,Site* where) : slot(s),bytes(b),site(where) {
        liveBytes[slot].fetch_add(bytes,std::memory_order_relaxed);liveCount[slot].fetch_add(1,std::memory_order_relaxed);
        if(site){site->bytes.fetch_add(bytes,std::memory_order_relaxed);site->count.fetch_add(1,std::memory_order_relaxed);}
    }
    ~Tracker() {
        liveBytes[slot].fetch_sub(bytes,std::memory_order_relaxed);liveCount[slot].fetch_sub(1,std::memory_order_relaxed);
        if(site){site->bytes.fetch_sub(bytes,std::memory_order_relaxed);site->count.fetch_sub(1,std::memory_order_relaxed);}
    }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override {return ++refs;}
    ULONG STDMETHODCALLTYPE Release() override {const auto n=--refs;if(!n)delete this;return n;}
};

HMODULE Module(const void* address) {
    HMODULE m{};
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,static_cast<LPCWSTR>(address),&m);
    return m;
}
const HMODULE exe=GetModuleHandleW(nullptr);
const uintptr_t exeBase=reinterpret_cast<uintptr_t>(exe);
const uintptr_t exeEnd=[] {
    const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(exeBase);
    return exeBase+reinterpret_cast<const IMAGE_NT_HEADERS*>(exeBase+dos->e_lfanew)->OptionalHeader.SizeOfImage;
}();
Creator CreatorOf(const void* caller) {
    static const HMODULE port=Module(&kKey);
    const HMODULE m=caller ? Module(caller) : nullptr;
    return m==exe ? Game : m==port ? Port : Other;
}
// The trackers' code must stay mapped as long as any tracked object lives: pinned at the first one.
bool Pin() {
    static const bool pinned=[] {HMODULE m{};return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&kKey),&m)!=FALSE;}();
    return pinned;
}
HeapKind KindOf(D3D12_HEAP_TYPE type) {
    switch(type) {
        case D3D12_HEAP_TYPE_DEFAULT:return Default;
        case D3D12_HEAP_TYPE_UPLOAD:return Upload;
        case D3D12_HEAP_TYPE_READBACK:return Readback;
        default:return Custom;
    }
}
Class ClassOf(const D3D12_RESOURCE_DESC& d) {
    if(d.Dimension==D3D12_RESOURCE_DIMENSION_BUFFER)return Buffer;
    constexpr auto target=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET|D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL|D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    return (d.Flags&target) ? Target : Texture;
}
// The game's call site: its first four exe frames under the creation hook.
Site* SiteOf(Class cls,bool placed) {
    void* frames[32]{};
    const USHORT n=RtlCaptureStackBackTrace(2,32,frames,nullptr);
    SiteKey key{};key.kind=unsigned(cls)|(placed?0x10u:0u);
    for(USHORT f=0,k=0;f<n && k<4;++f) {
        const auto a=reinterpret_cast<uintptr_t>(frames[f]);
        if(a>=exeBase && a<exeEnd)key.rva[k++]=uint32_t(a-exeBase);
    }
    AcquireSRWLockShared(&siteLock);
    auto found=sites.find(key);Site* site=found!=sites.end() ? found->second : nullptr;
    ReleaseSRWLockShared(&siteLock);
    if(site)return site;
    AcquireSRWLockExclusive(&siteLock);
    found=sites.find(key);
    if(found!=sites.end())site=found->second;
    else if(sites.size()>=kSiteLimit)site=&overflow;
    else try {site=new Site{};site->key=key;sites.emplace(key,site);} catch(...) {site=&overflow;}
    ReleaseSRWLockExclusive(&siteLock);
    return site;
}
void Attach(ID3D12Object* object,unsigned slot,int64_t bytes,Site* site) {
    if(!object || bytes<=0 || !Pin())return;
    auto* tracker=new(std::nothrow) Tracker(slot,bytes,site);
    if(!tracker)return;
    // The object holds the tracker from here; its destruction releases it. When it cannot, the counts go back out.
    object->SetPrivateDataInterface(kKey,tracker);
    tracker->Release();
}
int64_t MB(int64_t bytes) {return bytes/(1024*1024);}
}

bool Enabled() {return enabled.load(std::memory_order_relaxed);}
void SetEnabled(bool on) {enabled.store(on,std::memory_order_relaxed);}

void ResourceCreated(ID3D12Resource* resource,const D3D12_RESOURCE_DESC& desc,D3D12_HEAP_TYPE heapType,uint64_t bytes,
                     bool placed,const void* caller) {
    if(!Enabled() || !resource || !bytes || bytes==UINT64_MAX)return;
    const Creator creator=CreatorOf(caller);const Class cls=ClassOf(desc);
    Attach(resource,Slot(creator,placed?Placed:Committed,KindOf(heapType),cls),int64_t(bytes),creator==Game ? SiteOf(cls,placed) : nullptr);
}
void HeapCreated(ID3D12Heap* heap,const void* caller) {
    if(!Enabled() || !heap)return;
    const auto desc=heap->GetDesc();
    Attach(heap,Slot(CreatorOf(caller),Heap,KindOf(desc.Properties.Type),Buffer),int64_t(desc.SizeInBytes),nullptr);
}

void Log(uint64_t local,uint64_t nonLocal) {
    if(!Enabled())return;
    int64_t now[kSlots];for(unsigned i=0;i<kSlots;++i)now[i]=liveBytes[i].load(std::memory_order_relaxed);
    const auto sum=[&](const int64_t* v,int c,int p,int h,int k) {
        int64_t total=0;
        for(unsigned ci=0;ci<CreatorCount;++ci)if(c<0 || unsigned(c)==ci)
        for(unsigned pi=0;pi<PlacementCount;++pi)if(p<0 || unsigned(p)==pi)
        for(unsigned hi=0;hi<HeapKindCount;++hi)if(h<0 || unsigned(h)==hi)
        for(unsigned ki=0;ki<ClassCount;++ki)if(k<0 || unsigned(k)==ki)total+=v[Slot(ci,pi,hi,ki)];
        return total;
    };
    // [live MB, change since the last line]; memory is held by heaps and committed resources, placed ones live inside heaps.
    const auto both=[&](int c,int p,int h,int k) {
        const int64_t value=sum(now,c,p,h,k);return std::pair<long long,long long>{MB(value),haveLast ? MB(value-sum(lastBytes,c,p,h,k)) : 0};
    };
    const auto held=[&](int c) {
        const int64_t value=sum(now,c,Heap,-1,-1)+sum(now,c,Committed,-1,-1);
        const int64_t before=sum(lastBytes,c,Heap,-1,-1)+sum(lastBytes,c,Committed,-1,-1);
        return std::pair<long long,long long>{MB(value),haveLast ? MB(value-before) : 0};
    };
    const auto all=held(-1),game=held(Game),port=held(Port),other=held(Other);
    ::Log("[vram-census] tracked %lld MB (%+lld) of %llu MB local, %llu MB non-local: game %lld (%+lld), port %lld (%+lld), other %lld (%+lld)\n",
        all.first,all.second,static_cast<unsigned long long>(local>>20),static_cast<unsigned long long>(nonLocal>>20),
        game.first,game.second,port.first,port.second,other.first,other.second);
    static const char* const names[CreatorCount]{"game","port","other"};
    for(int c=0;c<int(CreatorCount);++c) {
        const auto blocks=both(c,Heap,Default,-1),placedAll=both(c,Placed,Default,-1);
        const auto pBuf=both(c,Placed,Default,Buffer),pTex=both(c,Placed,Default,Texture),pTgt=both(c,Placed,Default,Target);
        const auto cTex=both(c,Committed,Default,Texture),cTgt=both(c,Committed,Default,Target),cBuf=both(c,Committed,Default,Buffer);
        const auto cpu=both(c,-1,Upload,-1).first+both(c,-1,Readback,-1).first,custom=both(c,-1,Custom,-1).first;
        if(!blocks.first && !placedAll.first && !cTex.first && !cTgt.first && !cBuf.first && !cpu && !custom)continue;
        ::Log("[vram-census] %s: allocator blocks %lld (%+lld) holding placed %lld (%+lld): textures %lld (%+lld), buffers %lld (%+lld), "
            "targets %lld (%+lld) -> %lld unused in blocks; committed textures %lld (%+lld), targets %lld (%+lld), buffers %lld (%+lld); "
            "upload/readback %lld, custom %lld MB\n",names[c],blocks.first,blocks.second,placedAll.first,placedAll.second,
            pTex.first,pTex.second,pBuf.first,pBuf.second,pTgt.first,pTgt.second,blocks.first-placedAll.first,
            cTex.first,cTex.second,cTgt.first,cTgt.second,cBuf.first,cBuf.second,cpu,custom);
    }
    // The game's call sites: the biggest growths since the last line, and the biggest holders.
    struct Row {const Site* site;int64_t bytes,count,delta;};
    std::vector<Row> rows;
    AcquireSRWLockShared(&siteLock);
    try {
        rows.reserve(sites.size()+1);
        for(const auto& [_,site]:sites)rows.push_back({site,site->bytes.load(std::memory_order_relaxed),site->count.load(std::memory_order_relaxed),0});
        rows.push_back({&overflow,overflow.bytes.load(std::memory_order_relaxed),overflow.count.load(std::memory_order_relaxed),0});
    } catch(...) {}
    ReleaseSRWLockShared(&siteLock);
    for(auto& r:rows)r.delta=haveLast ? r.bytes-r.site->last : 0;
    const auto print=[&](const Row& r,const char* why) {
        const auto& k=r.site->key;const unsigned cls=k.kind&0xF;
        ::Log("[vram-census]   %s exe+%X<exe+%X<exe+%X<exe+%X %s %s: %lld MB live in %lld (%+lld MB)\n",why,k.rva[0],k.rva[1],k.rva[2],k.rva[3],
            (k.kind&0x10)?"placed":"committed",cls==Buffer?"buffers":cls==Target?"targets":"textures",
            MB(r.bytes),static_cast<long long>(r.count),MB(r.delta));
    };
    std::sort(rows.begin(),rows.end(),[](const Row& a,const Row& b){return a.delta>b.delta;});
    for(size_t i=0;i<rows.size() && i<6;++i)if(rows[i].delta>=(int64_t(8)<<20))print(rows[i],"grew");
    std::sort(rows.begin(),rows.end(),[](const Row& a,const Row& b){return a.bytes>b.bytes;});
    for(size_t i=0;i<rows.size() && i<6;++i)if(rows[i].bytes>=(int64_t(32)<<20))print(rows[i],"holds");
    // The baseline for the next line. Sites are only written here and in the trackers' atomics.
    for(auto& r:rows)const_cast<Site*>(r.site)->last=r.bytes;
    std::memcpy(lastBytes,now,sizeof(lastBytes));haveLast=true;
}
}
