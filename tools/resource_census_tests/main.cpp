#include "Render/ResourceLifetimeCensus.hpp"
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <algorithm>
#include <iostream>
#include <stdexcept>
#include <thread>

using Microsoft::WRL::ComPtr;
namespace census=cvr::gpu::resource_census;
namespace {
void Check(bool value,const char* why){if(!value)throw std::runtime_error(why);}
void Hr(HRESULT value){if(FAILED(value))throw std::runtime_error("D3D12 operation failed");}
struct Device {
    ComPtr<ID3D12Device> device;
    D3D12_HEAP_PROPERTIES properties{};
    Device() {
        ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
        ComPtr<IDXGIAdapter> adapter;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
        Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
        properties.Type=D3D12_HEAP_TYPE_DEFAULT;properties.CreationNodeMask=properties.VisibleNodeMask=1;
    }
    D3D12_RESOURCE_DESC Desc(uint64_t width) {
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=width;d.Height=1;
        d.DepthOrArraySize=d.MipLevels=1;d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;return d;
    }
    ComPtr<ID3D12Resource> Create(const D3D12_RESOURCE_DESC& desc) {
        ComPtr<ID3D12Resource> resource;Hr(device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&desc,
            D3D12_RESOURCE_STATE_COMMON,nullptr,IID_PPV_ARGS(&resource)));return resource;
    }
    bool Observe(ID3D12Resource* resource,const D3D12_RESOURCE_DESC& desc,const void* caller,census::Kind kind=census::Kind::Committed) {
        return census::Observe(resource,kind,desc,properties,D3D12_HEAP_FLAG_NONE,
            device->GetResourceAllocationInfo(0,1,&desc).SizeInBytes,caller);
    }
};
census::Counts Totals(const census::Snapshot& snapshot,census::Kind kind) {
    census::Counts total;
    for(const auto& row:snapshot.groups)if(row.key.kind==kind) {
        Check(row.consistent,"quiescent snapshot is inconsistent");
        total.created+=row.counts.created;total.released+=row.counts.released;total.live+=row.counts.live;
        total.createdBytes+=row.counts.createdBytes;total.releasedBytes+=row.counts.releasedBytes;total.liveBytes+=row.counts.liveBytes;
    }
    return total;
}
void Lifetime() {
    Device gpu;auto desc=gpu.Desc(65536);auto resource=gpu.Create(desc);
    Check(!gpu.Observe(resource.Get(),desc,reinterpret_cast<const void*>(&Lifetime)),"disabled census attached a cookie");
    Check(census::ReadSnapshot().groups.empty(),"disabled census collected resources");
    CyberpunkVR_ResourceCensusEnable=1;
    Check(gpu.Observe(resource.Get(),desc,reinterpret_cast<const void*>(&Lifetime)),"resource was not observed");
    Check(!gpu.Observe(resource.Get(),desc,reinterpret_cast<const void*>(&Lifetime)),"base/API1 duplicate counted twice");
    auto consumer=resource;resource.Reset();
    const auto held=census::ReadSnapshot();const auto before=Totals(held,census::Kind::Committed);
    Check(before.created==1 && before.live==1 && before.released==0 && before.liveBytes==65536,"consumer lease lifetime is wrong");
    Check(held.duplicates==1,"duplicate observation not reported");
    CyberpunkVR_ResourceCensusEnable=0;consumer.Reset();
    const auto after=Totals(census::ReadSnapshot(),census::Kind::Committed);
    Check(after.released==1 && !after.live && !after.liveBytes,"cookie retained the resource or stopped retiring when disabled");
    Check(after.createdBytes==after.releasedBytes,"committed byte counters did not balance");

    // Placed footprints are resource identities, not separately committed VRAM.
    D3D12_HEAP_DESC heapDesc{};heapDesc.SizeInBytes=131072;heapDesc.Properties=gpu.properties;
    heapDesc.Flags=D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS;
    ComPtr<ID3D12Heap> heap;Hr(gpu.device->CreateHeap(&heapDesc,IID_PPV_ARGS(&heap)));
    ComPtr<ID3D12Resource> placed[2];CyberpunkVR_ResourceCensusEnable=2;
    for(unsigned i=0;i<2;++i) {
        Hr(gpu.device->CreatePlacedResource(heap.Get(),i*65536,&desc,D3D12_RESOURCE_STATE_COMMON,nullptr,IID_PPV_ARGS(&placed[i])));
        Check(gpu.Observe(placed[i].Get(),desc,reinterpret_cast<const void*>(&Lifetime),census::Kind::Placed),"placed resource missed");
    }
    auto snapshot=census::ReadSnapshot();const auto footprints=Totals(snapshot,census::Kind::Placed);
    Check(footprints.live==2 && footprints.liveBytes==131072,"placed footprint accounting is wrong");
    Check(!Totals(snapshot,census::Kind::Committed).liveBytes,"placed footprints inflated committed totals");
    Check(std::any_of(snapshot.groups.begin(),snapshot.groups.end(),[](const auto& row){return row.key.stackCount>0;}),"mode2 stack not captured");
    heap.Reset();placed[0].Reset();placed[1].Reset();
    Check(!census::ReadSnapshot().liveCookies,"census retained a resource, heap or private-data cookie");
    CyberpunkVR_ResourceCensusEnable=0;CyberpunkVR_ResourceCensusRequest=17;census::Poll();
    Check(CyberpunkVR_ResourceCensusCompleted==17 && !(CyberpunkVR_ResourceCensusReportSeq.load()&1),"explicit report publication failed");
    const std::string report(CyberpunkVR_ResourceCensusReport,CyberpunkVR_ResourceCensusReportBytes.load());
    Check(report.find("placed-footprint")!=std::string::npos && report.find("callerRva")!=std::string::npos,"report lost footprint/caller provenance");
}
void Overflow() {
    Device gpu;CyberpunkVR_ResourceCensusEnable=1;
    for(unsigned i=0;i<540;++i) {const auto desc=gpu.Desc(i+1);auto resource=gpu.Create(desc);
        Check(gpu.Observe(resource.Get(),desc,reinterpret_cast<const void*>(&Overflow)),"overflow resource lost its lifetime cookie");}
    const auto snapshot=census::ReadSnapshot();const auto total=Totals(snapshot,census::Kind::Committed);
    Check(snapshot.groups.size()<=census::MaxGroups && snapshot.groupOverflows>0,"group storage is unbounded or silently drops overflow");
    Check(total.created==540 && total.released==540 && !total.liveBytes && !snapshot.liveCookies,"overflow retirement counters lost resources");
}
void Concurrent() {
    Device gpu;CyberpunkVR_ResourceCensusEnable=1;std::atomic<unsigned> done{},errors{};
    std::array<std::thread,4> threads;
    for(auto& thread:threads)thread=std::thread([&]{try {
        for(unsigned i=0;i<100;++i) {const auto desc=gpu.Desc(65536);auto resource=gpu.Create(desc);
            if(!gpu.Observe(resource.Get(),desc,reinterpret_cast<const void*>(&Concurrent)))++errors;}
    }catch(...){++errors;}++done;});
    while(done.load()<threads.size()) {
        const auto snapshot=census::ReadSnapshot();
        for(const auto& row:snapshot.groups)if(row.consistent &&
            (row.counts.created-row.counts.released!=row.counts.live || row.counts.createdBytes-row.counts.releasedBytes!=row.counts.liveBytes))++errors;
        std::this_thread::yield();
    }
    for(auto& thread:threads)thread.join();
    const auto snapshot=census::ReadSnapshot();const auto total=Totals(snapshot,census::Kind::Committed);
    Check(!errors && total.created==400 && total.released==400 && !total.live && !snapshot.liveCookies,"concurrent lifetime/reporting lost or double-counted resources");
}
void ConcurrentDuplicate() {
    Device gpu;CyberpunkVR_ResourceCensusEnable=1;
    const auto desc=gpu.Desc(65536);auto resource=gpu.Create(desc);
    std::atomic<unsigned> attached{};std::array<std::thread,4> threads;
    for(auto& thread:threads)thread=std::thread([&] {
        for(unsigned i=0;i<100;++i)if(gpu.Observe(resource.Get(),desc,reinterpret_cast<const void*>(&ConcurrentDuplicate)))++attached;
    });
    for(auto& thread:threads)thread.join();
    const auto snapshot=census::ReadSnapshot();const auto total=Totals(snapshot,census::Kind::Committed);
    Check(attached==1 && total.created==1 && total.live==1 && snapshot.duplicates==399,
        "concurrent base/API wrapper observations attached more than one cookie");
    resource.Reset();const auto retired=Totals(census::ReadSnapshot(),census::Kind::Committed);
    Check(retired.released==1 && !retired.live && !census::ReadSnapshot().liveCookies,"duplicate cookie references retained the final resource");
}
}
int main(int argc,char** argv) try {
    if(argc!=2)return 2;const std::string name=argv[1];
    if(name=="lifetime")Lifetime();else if(name=="overflow")Overflow();else if(name=="concurrent")Concurrent();
    else if(name=="duplicate_concurrent")ConcurrentDuplicate();else return 2;
    std::cout<<"PASS "<<name<<'\n';return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}
