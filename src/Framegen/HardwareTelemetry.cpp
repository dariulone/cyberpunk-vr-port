#include "Framegen/HardwareTelemetry.hpp"
#include "Framegen/Framegen.hpp"
#include <windows.h>
#include <winternl.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <d3dkmthk.h>
#include <psapi.h>
#include <wrl/client.h>
#include <algorithm>
#include <bit>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <mutex>
#include <thread>

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE SAMPLER AND ITS MONITOR THREAD ARE RUST'S (rust/crates/cvr-gpu/src/hardware_telemetry.rs); framegen's metrics
// switch is handed over as a callback.
extern "C" {
}
namespace cvr::framegen {
static_assert(sizeof(HardwareStatistics) == 824);
namespace {
bool Metrics() { return MetricsEnabled(); }
}  // namespace
}  // namespace cvr::framegen
#endif
#else
namespace cvr::framegen {
namespace {
using Microsoft::WRL::ComPtr;
std::atomic<uint64_t> requestedAdapter{};
std::atomic<bool> monitorRunning{false};
using NvDevice=void*;
struct NvMemory {unsigned long long total,free,used;};
struct NvUsage {unsigned gpu,memory;};
struct Nvml {
    HMODULE module{};bool initialized{};
    int (*init)(){};int (*shutdown)(){};
    int (*byPci)(const char*,NvDevice*){};
    int (*count)(unsigned*){};int (*byIndex)(unsigned,NvDevice*){};
    int (*usage)(NvDevice,NvUsage*){};int (*memory)(NvDevice,NvMemory*){};
    int (*temperature)(NvDevice,unsigned,unsigned*){};
    int (*name)(NvDevice,char*,unsigned){};
    template<class T> void Load(T& target,const char* symbol) {target=reinterpret_cast<T>(GetProcAddress(module,symbol));}
    Nvml() {
        module=LoadLibraryExW(L"nvml.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
        if(!module)return;
        Load(init,"nvmlInit_v2");Load(shutdown,"nvmlShutdown");Load(byPci,"nvmlDeviceGetHandleByPciBusId_v2");
        Load(count,"nvmlDeviceGetCount_v2");Load(byIndex,"nvmlDeviceGetHandleByIndex_v2");
        Load(usage,"nvmlDeviceGetUtilizationRates");Load(memory,"nvmlDeviceGetMemoryInfo");
        Load(temperature,"nvmlDeviceGetTemperature");Load(name,"nvmlDeviceGetName");
        initialized=init && init()==0;
    }
    ~Nvml() {if(initialized && shutdown)shutdown();if(module)FreeLibrary(module);}
    NvDevice Select(LUID luid) const {
        if(!initialized)return nullptr;
        const auto gdi=GetModuleHandleW(L"gdi32.dll");
        const auto open=reinterpret_cast<decltype(&D3DKMTOpenAdapterFromLuid)>(GetProcAddress(gdi,"D3DKMTOpenAdapterFromLuid"));
        const auto query=reinterpret_cast<decltype(&D3DKMTQueryAdapterInfo)>(GetProcAddress(gdi,"D3DKMTQueryAdapterInfo"));
        const auto close=reinterpret_cast<decltype(&D3DKMTCloseAdapter)>(GetProcAddress(gdi,"D3DKMTCloseAdapter"));
        if(open && query && close && byPci) {
            D3DKMT_OPENADAPTERFROMLUID handle{};handle.AdapterLuid=luid;
            if(open(&handle)>=0) {
                D3DKMT_ADAPTERADDRESS address{};D3DKMT_QUERYADAPTERINFO info{};info.hAdapter=handle.hAdapter;
                info.Type=KMTQAITYPE_ADAPTERADDRESS;info.pPrivateDriverData=&address;info.PrivateDriverDataSize=sizeof(address);
                const auto result=query(&info);D3DKMT_CLOSEADAPTER cleanup{};cleanup.hAdapter=handle.hAdapter;close(&cleanup);
                if(result>=0) {
                    char pci[32]{};std::snprintf(pci,sizeof(pci),"00000000:%02X:%02X.%X",address.BusNumber,address.DeviceNumber,address.FunctionNumber);
                    NvDevice selected{};if(byPci(pci,&selected)==0)return selected;
                }
            }
        }
        // A unique NVIDIA device is unambiguous; never select an arbitrary GPU
        // when multiple boards exist and the exact adapter lookup failed.
        unsigned n{};NvDevice selected{};
        return count && byIndex && count(&n)==0 && n==1 && byIndex(0,&selected)==0 ? selected : nullptr;
    }
};
uint64_t Ticks(const FILETIME& t) {return (uint64_t(t.dwHighDateTime)<<32)|t.dwLowDateTime;}
struct CoreTimes {LARGE_INTEGER idle,kernel,user,reserved[2];ULONG reserved2;};
static_assert(sizeof(CoreTimes)==48);
struct Sampler {
    Nvml nvml;NvDevice nv{};ComPtr<IDXGIAdapter3> adapter;
    uint64_t luid{},lastIdle{},lastTotal{};bool cpuKnown{};
    std::array<CoreTimes,128> previousCores{};uint32_t previousCoreCount{};
    char cpuName[96]{},gpuName[96]{};uint64_t gpuTotal{};
    Sampler() {
        wchar_t name[128]{};DWORD size=sizeof(name);
        if(RegGetValueW(HKEY_LOCAL_MACHINE,L"HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",L"ProcessorNameString",RRF_RT_REG_SZ,nullptr,name,&size)==ERROR_SUCCESS)
            WideCharToMultiByte(CP_UTF8,0,name,-1,cpuName,sizeof(cpuName),nullptr,nullptr);
    }
    HardwareStatistics Read() {
        const double start=NowMs();HardwareStatistics out{};
        std::snprintf(out.cpuName,sizeof(out.cpuName),"%s",cpuName);
        const auto wanted=requestedAdapter.load(std::memory_order_acquire);
        if(wanted!=luid) {
            adapter.Reset();nv=nullptr;gpuTotal=0;gpuName[0]=0;luid=wanted;
            ComPtr<IDXGIFactory4> factory;
            if(wanted && SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
               SUCCEEDED(factory->EnumAdapterByLuid(std::bit_cast<LUID>(wanted),IID_PPV_ARGS(&adapter)))) {
                DXGI_ADAPTER_DESC desc{};adapter->GetDesc(&desc);gpuTotal=desc.DedicatedVideoMemory;
                WideCharToMultiByte(CP_UTF8,0,desc.Description,-1,gpuName,sizeof(gpuName),nullptr,nullptr);
                if(desc.VendorId==0x10de)nv=nvml.Select(desc.AdapterLuid);
            }
        }
        std::snprintf(out.gpuName,sizeof(out.gpuName),"%s",gpuName);out.gpuMemoryTotal=gpuTotal;
        FILETIME idle{},kernel{},user{};
        if(GetSystemTimes(&idle,&kernel,&user)) {
            const auto i=Ticks(idle),total=Ticks(kernel)+Ticks(user);
            if(cpuKnown && total>lastTotal && i>=lastIdle)
                out.cpuUsage=std::clamp(100.0*(1.0-double(i-lastIdle)/double(total-lastTotal)),0.0,100.0);
            lastIdle=i;lastTotal=total;cpuKnown=true;
        }
        using NtQuery=NTSTATUS (NTAPI*)(SYSTEM_INFORMATION_CLASS,PVOID,ULONG,PULONG);
        const auto query=reinterpret_cast<NtQuery>(GetProcAddress(GetModuleHandleW(L"ntdll.dll"),"NtQuerySystemInformation"));
        std::array<CoreTimes,128> current{};ULONG bytes{};
        if(query && query(SystemProcessorPerformanceInformation,current.data(),sizeof(current),&bytes)>=0) {
            const auto n=std::min<uint32_t>(bytes/sizeof(CoreTimes),current.size());out.coreCount=n;
            if(n==previousCoreCount) {
                out.coreMaximum=0;
                for(unsigned index=0;index<n;++index) {
                    const auto& a=previousCores[index];const auto& b=current[index];
                    const auto total=b.kernel.QuadPart-a.kernel.QuadPart+b.user.QuadPart-a.user.QuadPart;
                    const auto idleTicks=b.idle.QuadPart-a.idle.QuadPart;
                    out.cores[index]=total>0 ? float(std::clamp(100.0*(1.0-double(idleTicks)/double(total)),0.0,100.0)) : 0;
                    out.coreMaximum=std::max(out.coreMaximum,double(out.cores[index]));
                }
            }
            previousCores=current;previousCoreCount=n;
        }
        MEMORYSTATUSEX memory{};memory.dwLength=sizeof(memory);
        if(GlobalMemoryStatusEx(&memory)) {out.ramTotal=memory.ullTotalPhys;out.ramUsed=memory.ullTotalPhys-memory.ullAvailPhys;}
        PROCESS_MEMORY_COUNTERS_EX process{};process.cb=sizeof(process);
        if(GetProcessMemoryInfo(GetCurrentProcess(),reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&process),sizeof(process))) {
            out.appWorkingSet=process.WorkingSetSize;out.appCommit=process.PrivateUsage;
        }
        if(adapter) {
            DXGI_QUERY_VIDEO_MEMORY_INFO video{};
            if(SUCCEEDED(adapter->QueryVideoMemoryInfo(0,DXGI_MEMORY_SEGMENT_GROUP_LOCAL,&video))) {
                out.gpuProcessMemoryValid=true;out.gpuProcessUsed=video.CurrentUsage;out.gpuProcessBudget=video.Budget;
            }
        }
        if(nv) {
            NvUsage usage{};NvMemory memory{};unsigned temperature{};
            if(nvml.usage && nvml.usage(nv,&usage)==0)out.gpuUsage=usage.gpu;
            if(nvml.memory && nvml.memory(nv,&memory)==0) {out.gpuMemoryValid=true;out.gpuMemoryUsed=memory.used;out.gpuMemoryTotal=memory.total;}
            if(nvml.temperature && nvml.temperature(nv,0,&temperature)==0)out.gpuTemperature=temperature;
        }
        out.sampleMs=NowMs()-start;return out;
    }
};
class Monitor {
    std::mutex mutex;
    std::mutex lifecycle;
    std::condition_variable wake;
    std::thread thread;
    bool stopping{};
    HardwareStatistics snapshot;
    void Run() {
        Sampler sampler;
        std::unique_lock lock(mutex);
        while(!stopping) {
            if(!MetricsEnabled()) {wake.wait(lock,[&] {return stopping || MetricsEnabled();});continue;}
            lock.unlock();auto next=sampler.Read();lock.lock();next.sequence=snapshot.sequence+1;snapshot=next;
            wake.wait_for(lock,std::chrono::seconds(1),[&] {return stopping || !MetricsEnabled();});
        }
    }
public:
    ~Monitor(){Stop();}
    void Wake() {
        std::lock_guard lifecycleLock(lifecycle);
        std::lock_guard lock(mutex);
        if(!thread.joinable() && MetricsEnabled()) {stopping=false;monitorRunning.store(true);thread=std::thread([this]{Run();});}
        wake.notify_all();
    }
    HardwareStatistics Read() {std::lock_guard lock(mutex);return snapshot;}
    void Stop() {
        std::lock_guard lifecycleLock(lifecycle);
        {std::lock_guard lock(mutex);stopping=true;wake.notify_all();}
        if(thread.joinable())thread.join();
        {std::lock_guard lock(mutex);const auto sequence=snapshot.sequence;snapshot={};snapshot.sequence=sequence;}
        monitorRunning.store(false,std::memory_order_release);
    }
};
Monitor& Hardware() {static Monitor monitor;return monitor;}
}
void ConfigureHardwareTelemetry(ID3D12Device* device) {
    if(!device || !MetricsEnabled())return;const auto luid=std::bit_cast<uint64_t>(device->GetAdapterLuid());
    if(requestedAdapter.exchange(luid,std::memory_order_acq_rel)!=luid || !monitorRunning.load(std::memory_order_acquire))Hardware().Wake();
}
void WakeHardwareTelemetry() {Hardware().Wake();}
void StopHardwareTelemetry() {Hardware().Stop();}
HardwareStatistics GetHardwareStatistics() {return Hardware().Read();}
}
#endif
