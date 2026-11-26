#pragma once
#include <d3d12.h>
#include <array>
#include <atomic>
#include <cstdint>
#include <string>
#include <vector>

extern "C" {
// 0 off, 1 immediate API caller, 2 caller plus at most six return addresses.
extern __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ResourceCensusEnable;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ResourceCensusRequest;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ResourceCensusCompleted;
extern __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ResourceCensusReportSeq;
extern __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ResourceCensusReportBytes;
extern __declspec(dllexport) char CyberpunkVR_ResourceCensusReport[1024*1024];
}

namespace cvr::gpu::resource_census {
inline constexpr size_t MaxGroups=512, StackDepth=16;
enum class Kind:uint32_t { Committed, Placed };
struct Key {
    Kind kind{};
    D3D12_RESOURCE_DESC desc{};
    D3D12_HEAP_PROPERTIES heap{};
    D3D12_HEAP_FLAGS heapFlags{};
    uint64_t allocationBytes{},caller{},moduleBase{},moduleRva{};
    uint32_t moduleTimestamp{},stackCount{};
    std::array<uint64_t,StackDepth> stack{};
    char module[512]{};
};
struct Counts {
    uint64_t created{},released{},live{},createdBytes{},releasedBytes{},liveBytes{},peakLiveBytes{};
};
struct Row { uint32_t id{};Key key;Counts counts;bool overflow{},consistent{}; };
struct Snapshot {
    std::vector<Row> groups;
    uint64_t liveCookies{},duplicates{},attachFailures{},cookieLimitDrops{},groupOverflows{},reentrantDrops{},invalidAllocations{},modulePinFailures{};
};
inline bool Enabled(){return CyberpunkVR_ResourceCensusEnable.load(std::memory_order_relaxed)!=0;}
// Observes only resources created after enabling. The cookie never retains the
// resource, heap, or device. Counters continue to retire while collection is off.
__declspec(noinline) bool Observe(ID3D12Resource*,Kind,const D3D12_RESOURCE_DESC&,const D3D12_HEAP_PROPERTIES&,
                                 D3D12_HEAP_FLAGS,uint64_t allocationBytes,const void* apiCaller);
Snapshot ReadSnapshot();
std::string JsonReport();
// Existing Present poll calls this; default request=0 takes one relaxed load.
void Poll();
}
