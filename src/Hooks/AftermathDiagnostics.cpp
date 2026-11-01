#include "Hooks/Hook.hpp"
#include "Core/Paths.hpp"
#include "Hooks/AftermathDiagnostics.hpp"
#include <MinHook.h>
#include <windows.h>
#include <cstdint>
#include <cstring>
#include <cstdio>
#include <d3d12.h>
#include <atomic>
#include <mutex>
#include <unordered_map>
#include <array>

extern void Log(const char* fmt, ...);

namespace {
using InitializeFn = char (__fastcall*)(void*, char);
InitializeFn original{};
using ConfigureFn = int64_t (__fastcall*)(void*, const uint8_t*);
ConfigureFn originalConfigure{};
int diagnosticMode = 1;
std::atomic<bool> resourceTraceEnabled{};
struct ResourceSample {
    ID3D12Resource* resource{};
    D3D12_RESOURCE_DESC desc{};
    uint64_t tick{}, serial{};
};
std::mutex resourcesMutex;
std::unordered_map<void*, ResourceSample> resources;
std::atomic<uint64_t> resourceSerial{};
using ReleaseFn = ULONG (STDMETHODCALLTYPE*)(ID3D12Resource*);
struct ReleaseEntry { void** table{}; ReleaseFn original{}; };
std::array<ReleaseEntry, 16> releaseEntries{};
std::atomic<unsigned> releaseEntryCount{};
std::mutex releaseInstallMutex;

void TraceResource(const char* event, void* handle, const ResourceSample& sample, uint32_t status) {
    void* frames[14]{};
    const auto count = CaptureStackBackTrace(2, 14, frames, nullptr);
    char stack[450]{};
    size_t used = 0;
    const auto base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    const auto end = base + nt->OptionalHeader.SizeOfImage;
    for (USHORT i = 0; i < count && used + 32 < sizeof(stack); ++i) {
        const auto address = reinterpret_cast<uintptr_t>(frames[i]);
        const int written = address >= base && address < end
            ? snprintf(stack + used, sizeof(stack) - used, "exe+%llX ", static_cast<unsigned long long>(address - base))
            : snprintf(stack + used, sizeof(stack) - used, "%p ", frames[i]);
        if (written > 0) used += static_cast<size_t>(written);
    }
    Log("[resource-life] %s id=%llu resource=%p handle=%p tick=%llu age=%llu tid=%lu "
        "dim=%u size=%llux%ux%u mip=%u fmt=%u flags=%X status=%X stack=%s\n",
        event, sample.serial, sample.resource, handle, GetTickCount64(), GetTickCount64() - sample.tick,
        GetCurrentThreadId(), unsigned(sample.desc.Dimension), sample.desc.Width, sample.desc.Height,
        unsigned(sample.desc.DepthOrArraySize), unsigned(sample.desc.MipLevels), unsigned(sample.desc.Format),
        unsigned(sample.desc.Flags), status, stack);
}

ULONG STDMETHODCALLTYPE ReleaseResource(ID3D12Resource* resource) {
    auto** table = *reinterpret_cast<void***>(resource);
    ReleaseFn release = nullptr;
    const auto count = releaseEntryCount.load(std::memory_order_acquire);
    for (unsigned i = 0; i < count; ++i) if (releaseEntries[i].table == table) { release = releaseEntries[i].original; break; }
    if (!release) return 0; // Never exposed before its original is published.
    ResourceSample sample{};
    {
        std::lock_guard lock(resourcesMutex);
        const auto found = resources.find(resource);
        if (found != resources.end()) sample = found->second;
    }
    const auto remaining = release(resource);
    if (remaining == 0 && sample.resource) {
        {
            std::lock_guard lock(resourcesMutex);
            const auto found = resources.find(resource);
            if (found != resources.end() && found->second.serial == sample.serial) resources.erase(found);
        }
        TraceResource("final-release", nullptr, sample, remaining);
    }
    return remaining;
}

bool ObserveFinalRelease(ID3D12Resource* resource) {
    auto** table = *reinterpret_cast<void***>(resource);
    std::lock_guard lock(releaseInstallMutex);
    const auto count = releaseEntryCount.load(std::memory_order_relaxed);
    for (unsigned i = 0; i < count; ++i) if (releaseEntries[i].table == table) return true;
    if (count == releaseEntries.size()) return false;
    DWORD protection = 0;
    if (!VirtualProtect(table + 2, sizeof(void*), PAGE_READWRITE, &protection)) return false;
    releaseEntries[count] = {table, reinterpret_cast<ReleaseFn>(table[2])};
    // Other resources already use this table. Publish the immutable original
    // before an atomic slot exchange exposes the observer to any other thread.
    releaseEntryCount.store(count + 1, std::memory_order_release);
    InterlockedExchangePointer(reinterpret_cast<void* volatile*>(table + 2), reinterpret_cast<void*>(&ReleaseResource));
    DWORD ignored = 0;
    VirtualProtect(table + 2, sizeof(void*), protection, &ignored);
    return true;
}

void ObserveCreation(ID3D12Resource* resource, const D3D12_RESOURCE_DESC& desc) {
    if (!resourceTraceEnabled.load(std::memory_order_relaxed) || !resource) return;
    const bool depth = desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
        && desc.Format == DXGI_FORMAT_R32_TYPELESS && desc.Width >= 512 && desc.Height >= 512;
    const bool volume = desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE3D
        && desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT && desc.Width >= 128 && desc.Width <= 1024;
    if (!depth && !volume) return;
    const auto serial = resourceSerial.fetch_add(1, std::memory_order_relaxed) + 1;
    if (serial > 256 || !ObserveFinalRelease(resource)) return;
    const ResourceSample sample{resource, desc, GetTickCount64(), serial};
    { std::lock_guard lock(resourcesMutex); resources[resource] = sample; }
    TraceResource("create", nullptr, sample, 0);
}

bool Requested() {
    char path[MAX_PATH]{};
    if (strcpy_s(path, cvr::paths::FileA(cvr::paths::Folder::Config, "vrport_aftermath_debug.txt").c_str()) != 0) return false;
    FILE* file = nullptr;
    if (fopen_s(&file, path, "r") != 0 || !file) return false;
    int mode = 1;
    if (fscanf_s(file, "%d", &mode) != 1) mode = 1;
    fclose(file);
    diagnosticMode = mode == 2 ? 2 : 1;
    resourceTraceEnabled.store(true, std::memory_order_relaxed);
    return true;
}

char __fastcall Initialize(void* device, char requestedDebug) {
    // Native 2.31 wrapper uses flags3 normally, or4000000B for the game's
    // forceAftermathDebug option: resource tracking, markers, shader debug
    // information and automatic call-stack markers. Retain its own SDK version,
    // callbacks, device pointer, return value and resource registration path.
    Log("[aftermath-debug] entering native initialization device=%p\n", device);
    const char result = original(device, requestedDebug);
    Log("[aftermath-debug] native initialization debug=%u result=%u device=%p\n",
        unsigned(static_cast<unsigned char>(requestedDebug)), unsigned(result), device);
    return result;
}

int64_t __fastcall Configure(void* device, const uint8_t* requested) {
    // Native setup reads only these two bytes: enable Aftermath and its debug
    // features. It must also set the engine's initialized flag, otherwise the
    // engine never registers resources or creates command-list marker handles.
    const uint8_t options[]{1, static_cast<uint8_t>(diagnosticMode == 2)};
    Log("[aftermath-debug] entering native device setup device=%p\n", device);
    const auto result = originalConfigure(device, options);
    Log("[aftermath-debug] native device setup enable=%u debug=%u -> 1,%u complete\n",
        requested ? unsigned(requested[0]) : 0, requested ? unsigned(requested[1]) : 0, unsigned(options[1]));
    return result;
}

bool Install() {
    auto* base = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    auto* target = base + 0x2FA9008;
    constexpr uint8_t expected[]{0x48,0x83,0xEC,0x28,0x48,0x8B,0x05,0xBD,0x4C,0xC2,0x00,
        0x48,0x85,0xC0,0x74,0x31,0xF6,0xDA,0x4C,0x8B,0xC1,0xB9,0x09,0x02};
    if (std::memcmp(target, expected, sizeof(expected))) return false;
    auto* configure = base + 0x688D5C;
    constexpr uint8_t configureBytes[]{0x4C,0x8B,0xDC,0x49,0x89,0x5B,0x18,0x49,0x89,0x73,0x20,
        0x57,0x48,0x83,0xEC,0x20,0x48,0x8B,0xF2,0x48,0x8B,0xF9};
    if (std::memcmp(configure, configureBytes, sizeof(configureBytes))) return false;
    const auto init = MH_Initialize();
    if (init != MH_OK && init != MH_ERROR_ALREADY_INITIALIZED) return false;
    if (MH_CreateHook(target, reinterpret_cast<void*>(&Initialize), reinterpret_cast<void**>(&original)) != MH_OK)
        return false;
    if (MH_CreateHook(configure, reinterpret_cast<void*>(&Configure), reinterpret_cast<void**>(&originalConfigure)) != MH_OK) {
        MH_RemoveHook(target);
        return false;
    }
    if (MH_EnableHook(target) == MH_OK && MH_EnableHook(configure) == MH_OK) return true;
    MH_DisableHook(target);
    MH_RemoveHook(configure);
    MH_RemoveHook(target);
    return false;
}
}

void cvr::diagnostics::ObserveResourceCreation(ID3D12Resource* resource, const D3D12_RESOURCE_DESC& desc) {
    ObserveCreation(resource, desc);
}

// No marker file: no hook, no SDK/driver setting change, no per-frame cost.
CVR_HOOK_IF("AftermathDiagnostics", cvr::hooks::Stage::PreDevice, 5, Install, Requested);
