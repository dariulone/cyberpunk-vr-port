#include "Render/ResourceLifetimeCensus.hpp"
#include <windows.h>
#include <algorithm>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <new>

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ResourceCensusEnable{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ResourceCensusRequest{0},CyberpunkVR_ResourceCensusCompleted{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ResourceCensusReportSeq{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ResourceCensusReportBytes{0};
__declspec(dllexport) char CyberpunkVR_ResourceCensusReport[1024*1024]{};
}

namespace cvr::gpu::resource_census {
namespace {
constexpr size_t NormalGroups=MaxGroups-2;
constexpr uint64_t MaxCookies=131072;
constexpr GUID lifetimeGuid{0x7a6dd297,0xf4ab,0x43c1,{0xa2,0x8f,0x66,0x12,0xbe,0x73,0xe7,0x09}};
struct Counters {
    std::atomic<uint64_t> created{},released{},live{},createdBytes{},releasedBytes{},liveBytes{},peak{};
    std::atomic<uint64_t> revision{},writers{};
    void Created(uint64_t bytes) {
        writers.fetch_add(1,std::memory_order_acq_rel);
        created.fetch_add(1,std::memory_order_relaxed);createdBytes.fetch_add(bytes,std::memory_order_relaxed);
        live.fetch_add(1,std::memory_order_relaxed);
        const auto current=liveBytes.fetch_add(bytes,std::memory_order_relaxed)+bytes;
        for(auto p=peak.load(std::memory_order_relaxed);p<current &&
            !peak.compare_exchange_weak(p,current,std::memory_order_relaxed);){}
        revision.fetch_add(1,std::memory_order_release);writers.fetch_sub(1,std::memory_order_release);
    }
    void Released(uint64_t bytes) {
        // Called from the driver's private-data destruction: no locks, resource
        // methods, stack walks, or file I/O on this path.
        writers.fetch_add(1,std::memory_order_acq_rel);
        released.fetch_add(1,std::memory_order_relaxed);releasedBytes.fetch_add(bytes,std::memory_order_relaxed);
        live.fetch_sub(1,std::memory_order_relaxed);liveBytes.fetch_sub(bytes,std::memory_order_relaxed);
        revision.fetch_add(1,std::memory_order_release);writers.fetch_sub(1,std::memory_order_release);
    }
    bool Read(Counts& out) const {
        for(unsigned attempt=0;attempt<8;++attempt) {
            const bool idle=writers.load(std::memory_order_acquire)==0;
            const auto before=revision.load(std::memory_order_acquire);
            out={created.load(std::memory_order_relaxed),released.load(std::memory_order_relaxed),live.load(std::memory_order_relaxed),
                createdBytes.load(std::memory_order_relaxed),releasedBytes.load(std::memory_order_relaxed),
                liveBytes.load(std::memory_order_relaxed),peak.load(std::memory_order_relaxed)};
            if(idle && before==revision.load(std::memory_order_acquire) && !writers.load(std::memory_order_acquire))return true;
        }
        return false;
    }
};
struct Group {Key key{};Counters counts;};
struct State {
    // Fixed storage: a live cookie can never point to a moved/erased group.
    std::array<Group,MaxGroups> groups;
    size_t groupCount{};
    std::mutex registryMutex,attachmentMutex,reportMutex;
    std::atomic<uint64_t> liveCookies{},duplicates{},attachFailures{},cookieLimitDrops{},groupOverflows{},reentrantDrops{},invalidAllocations{},modulePinFailures{};
};
std::atomic<State*> publishedState{};
State* GetState() {
    // Cookies can be destroyed after the plugin's static teardown. Their bounded
    // counters therefore live for the process, allocated only on explicit use.
    static auto* state=[] {
        auto* value=new(std::nothrow) State;
        publishedState.store(value,std::memory_order_release);return value;
    }();
    return state;
}
thread_local bool observing{};

bool KeepObserverCodeAlive() {
    // The plugin manager can unload before graphics objects die. Their private
    // data still calls Cookie::Release, so pin code once when this diagnostic is
    // first enabled. This retains no resource, heap, device or command queue.
    static const bool pinned=[] {
        HMODULE module{};
        return GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
            reinterpret_cast<LPCWSTR>(&Observe),&module)!=FALSE;
    }();
    return pinned;
}

bool Same(const Key& a,const Key& b) {
    const auto& x=a.desc;const auto& y=b.desc;
    return a.kind==b.kind && a.allocationBytes==b.allocationBytes && a.caller==b.caller && a.moduleBase==b.moduleBase &&
        a.moduleTimestamp==b.moduleTimestamp && a.stackCount==b.stackCount && a.stack==b.stack &&
        x.Dimension==y.Dimension && x.Alignment==y.Alignment && x.Width==y.Width && x.Height==y.Height &&
        x.DepthOrArraySize==y.DepthOrArraySize && x.MipLevels==y.MipLevels && x.Format==y.Format &&
        x.SampleDesc.Count==y.SampleDesc.Count && x.SampleDesc.Quality==y.SampleDesc.Quality && x.Layout==y.Layout && x.Flags==y.Flags &&
        a.heap.Type==b.heap.Type && a.heap.CPUPageProperty==b.heap.CPUPageProperty && a.heap.MemoryPoolPreference==b.heap.MemoryPoolPreference &&
        a.heap.CreationNodeMask==b.heap.CreationNodeMask && a.heap.VisibleNodeMask==b.heap.VisibleNodeMask && a.heapFlags==b.heapFlags;
}
Group* FindGroup(State& state,const Key& key) {
    std::lock_guard lock(state.registryMutex);
    for(size_t i=0;i<state.groupCount;++i)if(Same(state.groups[i].key,key))return &state.groups[i];
    if(state.groupCount<NormalGroups) {auto& result=state.groups[state.groupCount++];result.key=key;return &result;}
    state.groupOverflows.fetch_add(1,std::memory_order_relaxed);
    auto& result=state.groups[NormalGroups+size_t(key.kind)];
    result.key.kind=key.kind;std::strcpy(result.key.module,"<group capacity overflow>");
    return &result;
}
struct Cookie final:IUnknown {
    std::atomic<ULONG> refs{1};State* state;Group* group;uint64_t bytes;bool armed{};
    Cookie(State* s,Group* g,uint64_t b):state(s),group(g),bytes(b){}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out) override {
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=static_cast<IUnknown*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override {return refs.fetch_add(1,std::memory_order_relaxed)+1;}
    ULONG STDMETHODCALLTYPE Release() override {
        const auto left=refs.fetch_sub(1,std::memory_order_acq_rel)-1;
        if(!left) {if(armed)group->counts.Released(bytes);state->liveCookies.fetch_sub(1,std::memory_order_relaxed);delete this;}
        return left;
    }
};
__declspec(noinline) void Trace(Key& key) {
    void* stack[StackDepth]{};
    const auto count=CaptureStackBackTrace(3,StackDepth,stack,nullptr);
    key.stackCount=count;for(unsigned i=0;i<count;++i)key.stack[i]=reinterpret_cast<uintptr_t>(stack[i]);
}
void Caller(Key& key,const void* address) {
    key.caller=reinterpret_cast<uintptr_t>(address);HMODULE module{};
    if(address && GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(address),&module)) {
        key.moduleBase=reinterpret_cast<uintptr_t>(module);key.moduleRva=key.caller-key.moduleBase;
        wchar_t name[512]{};GetModuleFileNameW(module,name,512);name[511]=0;
        WideCharToMultiByte(CP_UTF8,0,name,-1,key.module,sizeof(key.module),nullptr,nullptr);
        const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(module);
        if(dos->e_magic==IMAGE_DOS_SIGNATURE) {
            const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS*>(key.moduleBase+dos->e_lfanew);
            if(nt->Signature==IMAGE_NT_SIGNATURE)key.moduleTimestamp=nt->FileHeader.TimeDateStamp;
        }
    } else {key.moduleRva=key.caller;std::strcpy(key.module,"<unknown caller module>");}
}
void Quoted(std::string& out,const char* value) {
    out+='"';for(const unsigned char* p=reinterpret_cast<const unsigned char*>(value);*p;++p) {
        if(*p=='"' || *p=='\\') {out+='\\';out+=char(*p);}
        else if(*p<32) {char escaped[7];std::snprintf(escaped,sizeof(escaped),"\\u%04x",unsigned(*p));out+=escaped;}
        else out+=char(*p);
    }out+='"';
}
void Number(std::string& out,const char* key,uint64_t value,bool comma=true) {
    if(comma)out+=',';Quoted(out,key);out+=':';char text[32];std::snprintf(text,sizeof(text),"%llu",static_cast<unsigned long long>(value));out+=text;
}
void Hex(std::string& out,const char* key,uint64_t value) {
    out+=',';Quoted(out,key);out+=':';char text[32];std::snprintf(text,sizeof(text),"0x%llx",static_cast<unsigned long long>(value));Quoted(out,text);
}
}

bool Observe(ID3D12Resource* resource,Kind kind,const D3D12_RESOURCE_DESC& desc,const D3D12_HEAP_PROPERTIES& heap,
             D3D12_HEAP_FLAGS heapFlags,uint64_t bytes,const void* caller) {
    const auto mode=CyberpunkVR_ResourceCensusEnable.load(std::memory_order_relaxed);
    if(!mode || !resource)return false;
    if(observing){if(auto* s=publishedState.load(std::memory_order_acquire))s->reentrantDrops.fetch_add(1,std::memory_order_relaxed);return false;}
    struct Guard {Guard(){observing=true;}~Guard(){observing=false;}} guard;
    auto* state=GetState();if(!state)return false;
    if(!KeepObserverCodeAlive()){state->modulePinFailures.fetch_add(1,std::memory_order_relaxed);return false;}
    if(!bytes || bytes==UINT64_MAX){state->invalidAllocations.fetch_add(1,std::memory_order_relaxed);return false;}
    Key key;key.kind=kind;key.desc=desc;key.heap=heap;key.heapFlags=heapFlags;key.allocationBytes=bytes;Caller(key,caller);
    if(mode>=2)Trace(key);
    // Serialize duplicate observations of one object (base/API1 wrappers). Never
    // hold registryMutex while calling a driver method. Cookie release uses only atomics.
    std::lock_guard attach(state->attachmentMutex);
    IUnknown* existing{};UINT size=sizeof(existing);
    if(SUCCEEDED(resource->GetPrivateData(lifetimeGuid,&size,&existing)) && size==sizeof(existing) && existing) {
        existing->Release();state->duplicates.fetch_add(1,std::memory_order_relaxed);return false;
    }
    const auto old=state->liveCookies.fetch_add(1,std::memory_order_relaxed);
    if(old>=MaxCookies){state->liveCookies.fetch_sub(1,std::memory_order_relaxed);state->cookieLimitDrops.fetch_add(1,std::memory_order_relaxed);return false;}
    auto* group=FindGroup(*state,key);
    auto* cookie=new(std::nothrow) Cookie(state,group,bytes);
    if(!cookie){state->liveCookies.fetch_sub(1,std::memory_order_relaxed);state->attachFailures.fetch_add(1,std::memory_order_relaxed);return false;}
    const auto attached=resource->SetPrivateDataInterface(lifetimeGuid,cookie);
    if(SUCCEEDED(attached)){group->counts.Created(bytes);cookie->armed=true;}
    else state->attachFailures.fetch_add(1,std::memory_order_relaxed);
    cookie->Release();return SUCCEEDED(attached);
}

Snapshot ReadSnapshot() {
    Snapshot out;
    auto* state=GetState();if(!state){out.attachFailures=1;return out;}
    {
        std::lock_guard lock(state->registryMutex);out.groups.reserve(state->groupCount+2);
        for(size_t i=0;i<MaxGroups;++i) {
            if(i>=state->groupCount && i<NormalGroups)continue;
            Row row;row.id=uint32_t(i);row.key=state->groups[i].key;row.overflow=i>=NormalGroups;row.consistent=state->groups[i].counts.Read(row.counts);
            if(row.counts.created)out.groups.push_back(std::move(row));
        }
    }
    out.liveCookies=state->liveCookies.load();out.duplicates=state->duplicates.load();out.attachFailures=state->attachFailures.load();
    out.cookieLimitDrops=state->cookieLimitDrops.load();out.groupOverflows=state->groupOverflows.load();
    out.reentrantDrops=state->reentrantDrops.load();out.invalidAllocations=state->invalidAllocations.load();
    out.modulePinFailures=state->modulePinFailures.load();return out;
}

std::string JsonReport() {
    const auto snapshot=ReadSnapshot();std::string out;out.reserve(256*1024);
    out="{\"schema\":1,\"scope\":\"resources created while enabled; committed allocations are not residency; placed footprints are separate\","
        "\"coveredApis\":[\"CreateCommittedResource\",\"CreateCommittedResource1\",\"CreatePlacedResource\"]";
    Number(out,"pid",GetCurrentProcessId());Number(out,"mode",CyberpunkVR_ResourceCensusEnable.load());Number(out,"tickMs",GetTickCount64());
    Number(out,"maxGroups",MaxGroups);Number(out,"maxStackDepth",StackDepth);Number(out,"liveCookies",snapshot.liveCookies);Number(out,"duplicates",snapshot.duplicates);
    Number(out,"attachFailures",snapshot.attachFailures);Number(out,"cookieLimitDrops",snapshot.cookieLimitDrops);
    Number(out,"groupOverflows",snapshot.groupOverflows);Number(out,"reentrantDrops",snapshot.reentrantDrops);
    Number(out,"invalidAllocations",snapshot.invalidAllocations);Number(out,"modulePinFailures",snapshot.modulePinFailures);out+=",\"groups\":[";
    bool first=true;
    for(const auto& row:snapshot.groups) {
        if(!first)out+=',';first=false;out+='{';Quoted(out,"kind");out+=':';Quoted(out,row.key.kind==Kind::Committed?"committed":"placed-footprint");
        Number(out,"groupId",row.id);
        Number(out,"overflow",row.overflow);Number(out,"consistent",row.consistent);Number(out,"heapType",row.key.heap.Type);
        Number(out,"cpuPageProperty",row.key.heap.CPUPageProperty);Number(out,"memoryPoolPreference",row.key.heap.MemoryPoolPreference);
        Number(out,"creationNodeMask",row.key.heap.CreationNodeMask);Number(out,"visibleNodeMask",row.key.heap.VisibleNodeMask);
        Number(out,"heapFlags",row.key.heapFlags);Number(out,"dimension",row.key.desc.Dimension);Number(out,"width",row.key.desc.Width);
        Number(out,"height",row.key.desc.Height);Number(out,"depthOrArray",row.key.desc.DepthOrArraySize);Number(out,"mips",row.key.desc.MipLevels);
        Number(out,"format",row.key.desc.Format);Number(out,"flags",row.key.desc.Flags);Number(out,"samples",row.key.desc.SampleDesc.Count);
        Number(out,"sampleQuality",row.key.desc.SampleDesc.Quality);Number(out,"alignment",row.key.desc.Alignment);Number(out,"layout",row.key.desc.Layout);
        Number(out,"allocationBytes",row.key.allocationBytes);out+=",\"callerModule\":";Quoted(out,row.key.module);
        Hex(out,"caller",row.key.caller);Hex(out,"moduleBase",row.key.moduleBase);Hex(out,"callerRva",row.key.moduleRva);
        Number(out,"moduleTimestamp",row.key.moduleTimestamp);
        Number(out,"created",row.counts.created);Number(out,"released",row.counts.released);Number(out,"live",row.counts.live);
        Number(out,"createdBytes",row.counts.createdBytes);Number(out,"releasedBytes",row.counts.releasedBytes);
        Number(out,"liveBytes",row.counts.liveBytes);Number(out,"peakLiveBytes",row.counts.peakLiveBytes);out+=",\"stack\":[";
        for(uint32_t i=0;i<row.key.stackCount;++i) {if(i)out+=',';char value[32];std::snprintf(value,sizeof(value),"\"0x%llx\"",static_cast<unsigned long long>(row.key.stack[i]));out+=value;}
        out+="]}";
    }
    out+="]}";return out;
}

void Poll() {
    const auto request=CyberpunkVR_ResourceCensusRequest.load(std::memory_order_relaxed);
    if(!request || request==CyberpunkVR_ResourceCensusCompleted.load(std::memory_order_acquire))return;
    auto* state=GetState();if(!state)return;
    std::unique_lock lock(state->reportMutex,std::try_to_lock);if(!lock.owns_lock())return;
    try {
        auto report=JsonReport();
        if(report.size()>=sizeof(CyberpunkVR_ResourceCensusReport))report="{\"schema\":1,\"error\":\"bounded report capacity exceeded\"}";
        CyberpunkVR_ResourceCensusReportSeq.fetch_add(1,std::memory_order_acq_rel);
        std::memcpy(CyberpunkVR_ResourceCensusReport,report.data(),report.size());CyberpunkVR_ResourceCensusReport[report.size()]=0;
        CyberpunkVR_ResourceCensusReportBytes.store(uint32_t(report.size()),std::memory_order_relaxed);
        CyberpunkVR_ResourceCensusReportSeq.fetch_add(1,std::memory_order_release);
        CyberpunkVR_ResourceCensusCompleted.store(request,std::memory_order_release);
    }catch(const std::bad_alloc&){} // request remains pending; never fail a game Present for a diagnostic
}
}
