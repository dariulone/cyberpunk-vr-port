// gpu_caps.exe -- the D3D12 features the VR rendering work depends on, as the driver reports them (no game).
// ID3D12Device::CheckFeatureSupport (learn.microsoft.com/windows/win32/api/d3d12/nf-d3d12-id3d12device-checkfeaturesupport):
// D3D12_FEATURE_D3D12_OPTIONS3.ViewInstancingTier (view instancing / SV_ViewID), D3D12_FEATURE_SHADER_MODEL,
// OPTIONS7.MeshShaderTier, OPTIONS12.EnhancedBarriersSupported, OPTIONS1 wave ops, OPTIONS resource binding tier.
// Prints one JSON object.
#define NOMINMAX
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#pragma comment(lib, "d3d12.lib")
#pragma comment(lib, "dxgi.lib")
using Microsoft::WRL::ComPtr;

int main() {
    ComPtr<IDXGIFactory6> factory;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return 1;
    ComPtr<IDXGIAdapter1> adapter;
    if (FAILED(factory->EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IID_PPV_ARGS(&adapter)))) return 1;
    DXGI_ADAPTER_DESC1 desc{};
    adapter->GetDesc1(&desc);
    ComPtr<ID3D12Device> device;
    if (FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&device)))) return 1;
    D3D12_FEATURE_DATA_D3D12_OPTIONS o{};
    D3D12_FEATURE_DATA_D3D12_OPTIONS1 o1{};
    D3D12_FEATURE_DATA_D3D12_OPTIONS3 o3{};
    D3D12_FEATURE_DATA_D3D12_OPTIONS7 o7{};
    D3D12_FEATURE_DATA_D3D12_OPTIONS12 o12{};
    D3D12_FEATURE_DATA_SHADER_MODEL sm{D3D_SHADER_MODEL_6_8};
    device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS, &o, sizeof(o));
    device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS1, &o1, sizeof(o1));
    device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS3, &o3, sizeof(o3));
    const bool has7 = SUCCEEDED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS7, &o7, sizeof(o7)));
    const bool has12 = SUCCEEDED(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS12, &o12, sizeof(o12)));
    while (FAILED(device->CheckFeatureSupport(D3D12_FEATURE_SHADER_MODEL, &sm, sizeof(sm))) && sm.HighestShaderModel > D3D_SHADER_MODEL_5_1)
        sm.HighestShaderModel = D3D_SHADER_MODEL(sm.HighestShaderModel - 1);
    char name[256]{};
    WideCharToMultiByte(CP_UTF8, 0, desc.Description, -1, name, sizeof(name), nullptr, nullptr);
    std::printf("{\"adapter\":\"%s\",\"vendor\":\"0x%04X\",\"device\":\"0x%04X\",\"vramMiB\":%llu,"
                "\"viewInstancingTier\":%d,\"shaderModel\":\"%d.%d\",\"meshShaderTier\":%d,\"enhancedBarriers\":%d,"
                "\"waveOps\":%d,\"waveLanesMin\":%u,\"waveLanesMax\":%u,\"resourceBindingTier\":%d,"
                "\"vpAndRtArrayIndexFromAnyShaderFeedingRasterizer\":%d,\"copyQueueTimestamps\":%d}\n",
                name, desc.VendorId, desc.DeviceId, (unsigned long long)(desc.DedicatedVideoMemory >> 20),
                int(o3.ViewInstancingTier), (sm.HighestShaderModel >> 4) & 0xF, sm.HighestShaderModel & 0xF,
                has7 ? int(o7.MeshShaderTier) : -1, has12 ? int(o12.EnhancedBarriersSupported) : -1,
                int(o1.WaveOps), o1.WaveLaneCountMin, o1.WaveLaneCountMax, int(o.ResourceBindingTier),
                int(o.VPAndRTArrayIndexFromAnyShaderFeedingRasterizerSupportedWithoutGSEmulation),
                int(o3.CopyQueueTimestampQueriesSupported));
    return 0;
}
