#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/NativeInstanceWitness.hpp"
#include <Windows.h>
#include <TlHelp32.h>
#include <intrin.h>
#include <algorithm>
#include <cstring>
#include <mutex>

#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/instance_witness_runtime.rs (the thunk's XSAVE choice and readiness,
// the preparation, the thread suspension and the installation).
#else
extern "C" {
uint64_t CvrNativeInstanceReservationXstateMask{};
uint32_t CvrNativeInstanceReservationXstateMode{},CvrNativeInstanceReservationXstateReady{};
void CvrNativeInstanceReservationThunk();
}
namespace cvr::stereo::instance_witness {
namespace {
constexpr size_t MaxThreads=4096,MaxXstateBytes=16384;
struct Thread {HANDLE handle{};DWORD id{},previous{};bool suspended{};};
struct State {
    std::mutex mutex;uintptr_t base{},thunk{};HMODULE module{};uint8_t* relay{};size_t thunkBytes{};
    DWORD protection{};bool prepared{},attempted{},installed{},healthy{};
    std::array<uint8_t,AllocatorBytes> original;std::array<uint8_t,5> patch;std::array<uint8_t,14> jump;
    std::array<Thread,MaxThreads> pending;size_t pendingCount{};
};
State state;
void Ready(bool value)noexcept {InterlockedExchange(reinterpret_cast<volatile LONG*>(&CvrNativeInstanceReservationXstateReady),value?1:0);}
bool Read(uintptr_t address,void* output,size_t bytes)noexcept {SIZE_T copied{};
    return address && address<=UINTPTR_MAX-bytes && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(address),output,bytes,&copied) && copied==bytes;
}
bool Executable(DWORD protection)noexcept {return protection==PAGE_EXECUTE_READ || protection==PAGE_EXECUTE_READWRITE || protection==PAGE_EXECUTE_WRITECOPY;}
bool ConfigureXstate()noexcept {
    int cpu[4]{};__cpuid(cpu,0);const auto maximum=cpu[0];__cpuid(cpu,1);
    CvrNativeInstanceReservationXstateMask=0;CvrNativeInstanceReservationXstateMode=0;
    if(!(uint32_t(cpu[2])&(1u<<27)))return true;
    if(!(uint32_t(cpu[2])&(1u<<26)) || maximum<0xD)return false;
    const auto enabled=_xgetbv(0);__cpuidex(cpu,0xD,0);
    const auto supported=uint64_t(uint32_t(cpu[0]))|(uint64_t(uint32_t(cpu[3]))<<32),bytes=uint64_t(uint32_t(cpu[1]));
    if((enabled&3)!=3 || (enabled&~supported) || bytes<576 || bytes>MaxXstateBytes)return false;
    CvrNativeInstanceReservationXstateMask=enabled;CvrNativeInstanceReservationXstateMode=1;return true;
}
bool Code(bool patched)noexcept {
    if(!state.prepared || !state.base || !state.relay || !state.module || !CvrNativeInstanceReservationXstateReady)return false;
    std::array<uint8_t,AllocatorBytes> body{};std::array<uint8_t,14> relay{};MEMORY_BASIC_INFORMATION site{},storage{},thunk{};
    return Read(state.base+AllocatorRva,body.data(),body.size()) && detail::Body(state.original,body,state.patch,patched) &&
        Read(reinterpret_cast<uintptr_t>(state.relay),relay.data(),relay.size()) && relay==state.jump &&
        VirtualQuery(reinterpret_cast<void*>(state.base+SiteRva),&site,sizeof(site))==sizeof(site) && site.State==MEM_COMMIT && site.Type==MEM_IMAGE &&
        reinterpret_cast<uintptr_t>(site.AllocationBase)==state.base && site.Protect==state.protection && Executable(site.Protect) &&
        VirtualQuery(state.relay,&storage,sizeof(storage))==sizeof(storage) && storage.State==MEM_COMMIT && storage.Type==MEM_PRIVATE &&
        storage.AllocationBase==state.relay && storage.Protect==PAGE_EXECUTE_READ &&
        VirtualQuery(reinterpret_cast<void*>(state.thunk),&thunk,sizeof(thunk))==sizeof(thunk) && thunk.State==MEM_COMMIT && thunk.Type==MEM_IMAGE &&
        thunk.AllocationBase==state.module && Executable(thunk.Protect);
}
uint8_t* AllocateRelay(uintptr_t site)noexcept {
    SYSTEM_INFO info{};GetSystemInfo(&info);const uintptr_t step=info.dwAllocationGranularity,limit=0x7FFF0000;
    if(!step || (step&(step-1)))return nullptr;const auto aligned=site&~(step-1);
    for(uintptr_t distance=step;distance<=limit;distance+=step){const uintptr_t candidates[]{aligned>=distance?aligned-distance:0,
        aligned<=UINTPTR_MAX-distance?aligned+distance:0};
        for(const auto candidate:candidates){if(!candidate || candidate>UINTPTR_MAX-step)continue;MEMORY_BASIC_INFORMATION region{};
            if(!VirtualQuery(reinterpret_cast<void*>(candidate),&region,sizeof(region)) || region.State!=MEM_FREE || region.RegionSize<step)continue;
            auto* memory=static_cast<uint8_t*>(VirtualAlloc(reinterpret_cast<void*>(candidate),step,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
            if(memory)return memory;
        }
    }return nullptr;
}
bool Resume(Thread& thread)noexcept {
    if(!thread.suspended)return true;
    for(UINT attempt=0;attempt<3;++attempt){const auto previous=ResumeThread(thread.handle);
        if(previous!=DWORD(-1)){thread.suspended=false;return previous==thread.previous+1;}
        DWORD code{};if(GetExitCodeThread(thread.handle,&code) && code!=STILL_ACTIVE){thread.suspended=false;return true;}
    }return false;
}
bool ResumePending()noexcept {
    bool okay=true;for(size_t i=state.pendingCount;i--;){auto& thread=state.pending[i];const bool resumed=Resume(thread);okay&=resumed;
        if(!thread.suspended){CloseHandle(thread.handle);thread={};}
    }
    size_t retained{};for(size_t i=0;i<state.pendingCount;++i)if(state.pending[i].handle)state.pending[retained++]=state.pending[i];
    state.pendingCount=retained;return okay && !retained;
}
bool Inventory(std::array<DWORD,MaxThreads>& ids,size_t& count,DWORD process,DWORD current)noexcept {
    count=0;const auto snapshot=CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0);if(snapshot==INVALID_HANDLE_VALUE)return false;
    THREADENTRY32 entry{};entry.dwSize=sizeof(entry);bool okay=Thread32First(snapshot,&entry)!=FALSE;
    if(okay){do{if(entry.dwSize<offsetof(THREADENTRY32,th32OwnerProcessID)+sizeof(entry.th32OwnerProcessID)){okay=false;break;}
            if(entry.th32OwnerProcessID==process && entry.th32ThreadID!=current){if(count==ids.size()){okay=false;break;}ids[count++]=entry.th32ThreadID;}
            entry.dwSize=sizeof(entry);
        }while(Thread32Next(snapshot,&entry));
        if(okay && GetLastError()!=ERROR_NO_MORE_FILES)okay=false;
    }
    CloseHandle(snapshot);if(okay)std::sort(ids.begin(),ids.begin()+count);return okay;
}
class Frozen {
    std::array<Thread,MaxThreads> threads_{};size_t count_{};bool released_{};
public:
    ~Frozen(){if(!released_)Release();}
    bool Freeze(uintptr_t site)noexcept {
        const auto process=GetCurrentProcessId(),current=GetCurrentThreadId();std::array<DWORD,MaxThreads> ids{},again{};size_t count{},other{};
        for(UINT round=0;round<4;++round){if(!Inventory(ids,count,process,current))return false;
            for(size_t i=0;i<count;++i){if(std::any_of(threads_.begin(),threads_.begin()+count_,[&](const auto& t){return t.id==ids[i];}))continue;
                if(count_==threads_.size())return false;auto& thread=threads_[count_++];thread.id=ids[i];
                thread.handle=OpenThread(THREAD_SUSPEND_RESUME|THREAD_GET_CONTEXT|THREAD_QUERY_LIMITED_INFORMATION,FALSE,thread.id);
                if(!thread.handle || GetProcessIdOfThread(thread.handle)!=process || GetThreadId(thread.handle)!=thread.id)return false;
                thread.previous=SuspendThread(thread.handle);if(thread.previous==DWORD(-1))return false;thread.suspended=true;
            }
            if(!Inventory(again,other,process,current))return false;
            if(count!=other || !std::equal(ids.begin(),ids.begin()+count,again.begin()))continue;
            if(count!=count_)return false;
            for(size_t i=0;i<count_;++i){DWORD code{};CONTEXT context{};context.ContextFlags=CONTEXT_CONTROL;
                if(!GetExitCodeThread(threads_[i].handle,&code) || code!=STILL_ACTIVE || !GetThreadContext(threads_[i].handle,&context) ||
                    !detail::PatchBoundary(site,context.Rip))return false;
            }
            if(!Inventory(again,other,process,current))return false;
            if(count==other && std::equal(ids.begin(),ids.begin()+count,again.begin()))return true;
        }return false;
    }
    bool Release()noexcept {
        bool okay=true;for(size_t i=count_;i--;){auto& thread=threads_[i];if(!thread.handle)continue;
            const bool resumed=Resume(thread);okay&=resumed;
            if(thread.suspended){if(state.pendingCount<state.pending.size())state.pending[state.pendingCount++]=thread;okay=false;}
            else CloseHandle(thread.handle);thread={};
        }
        released_=true;return okay;
    }
};
bool Write(uintptr_t site,const std::array<uint8_t,5>& bytes,DWORD protection)noexcept {
    DWORD ignored{};if(!VirtualProtect(reinterpret_cast<void*>(site),bytes.size(),PAGE_EXECUTE_READWRITE,&ignored))return false;
    SIZE_T written{};const bool copied=WriteProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(site),bytes.data(),bytes.size(),&written) && written==bytes.size();
    const bool flushed=FlushInstructionCache(GetCurrentProcess(),reinterpret_cast<void*>(site),bytes.size())!=FALSE;
    const bool restored=VirtualProtect(reinterpret_cast<void*>(site),bytes.size(),protection,&ignored)!=FALSE;
    std::array<uint8_t,5> current{};return copied && flushed && restored && Read(site,current.data(),current.size()) && current==bytes;
}
bool PrepareLocked(uintptr_t base)noexcept {
    if(state.prepared)return base==state.base && Code(state.installed);
    if(state.relay || state.attempted || !base || base!=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr)) || base>UINTPTR_MAX-AllocatorRva-AllocatorBytes)return false;
    Ready(false);state.base=base;state.thunk=reinterpret_cast<uintptr_t>(&CvrNativeInstanceReservationThunk);
    if(!Read(base+AllocatorRva,state.original.data(),state.original.size()) || !detail::Body(state.original,state.original,state.patch,false) || !ConfigureXstate())return false;
    MEMORY_BASIC_INFORMATION site{};if(VirtualQuery(reinterpret_cast<void*>(base+SiteRva),&site,sizeof(site))!=sizeof(site) ||
        site.State!=MEM_COMMIT || site.Type!=MEM_IMAGE || reinterpret_cast<uintptr_t>(site.AllocationBase)!=base || !Executable(site.Protect))return false;
    state.protection=site.Protect;
    if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,reinterpret_cast<LPCWSTR>(state.thunk),&state.module))return false;
    DWORD64 image{};const auto* function=RtlLookupFunctionEntry(state.thunk,&image,nullptr);
    if(!function || image!=reinterpret_cast<uintptr_t>(state.module) || image+function->BeginAddress!=state.thunk ||
        function->EndAddress<=function->BeginAddress || function->EndAddress-function->BeginAddress>4096)return false;
    state.thunkBytes=function->EndAddress-function->BeginAddress;state.relay=AllocateRelay(base+SiteRva);if(!state.relay)return false;
    state.jump={0xFF,0x25,0,0,0,0};std::memcpy(state.jump.data()+6,&state.thunk,sizeof(state.thunk));
    std::memcpy(state.relay,state.jump.data(),state.jump.size());DWORD previous{};
    if(!detail::CallPatch(base+SiteRva,reinterpret_cast<uintptr_t>(state.relay),state.patch) ||
        !VirtualProtect(state.relay,state.jump.size(),PAGE_EXECUTE_READ,&previous) || !FlushInstructionCache(GetCurrentProcess(),state.relay,state.jump.size()))return false;
    state.prepared=true;Ready(true);if(!Code(false)){Ready(false);state.prepared=false;return false;}return true;
}
}
bool Prepare(uintptr_t base)noexcept try {std::lock_guard lock(state.mutex);return PrepareLocked(base);}catch(...){Ready(false);return false;}
bool Install(uintptr_t base,bool threadsQuiescent)noexcept try {
    std::lock_guard lock(state.mutex);if(!ResumePending())return false;
    if(state.installed)return state.healthy && base==state.base && Code(true);
    if(state.attempted || !PrepareLocked(base) || !Code(false))return false;Frozen frozen;
    if(!threadsQuiescent && !frozen.Freeze(base+SiteRva)){frozen.Release();return false;}
    if(!Code(false)){frozen.Release();return false;}state.attempted=true;
    const bool patched=Write(base+SiteRva,state.patch,state.protection) && Code(true);
    if(!patched){const bool rolledBack=Write(base+SiteRva,OriginalSite,state.protection) && Code(false);
        if(!rolledBack)Ready(false);frozen.Release();return false;
    }
    state.installed=true;state.healthy=frozen.Release();if(!state.healthy)Ready(false);return state.healthy && Code(true);
}catch(...){Ready(false);return false;}
bool Installed()noexcept try {std::lock_guard lock(state.mutex);return ResumePending() && state.installed && state.healthy && Code(true);}catch(...){Ready(false);return false;}
}
#endif
#endif
