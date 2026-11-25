#pragma once
#include "Render/FoveatedNgx.hpp"
#include <cstddef>
#include <cstdint>

#ifdef MOCK_NVNGX_EXPORTS
#define MOCK_NVNGX_API __declspec(dllexport)
#else
#define MOCK_NVNGX_API __declspec(dllimport)
#endif

struct MockNgxCreateRecord {
    cvr::foveated::ngx::Handle* handle{};
    unsigned feature{}, width{}, height{}, outWidth{}, outHeight{}, perfQualityValue{}, createFlags{}, presets[6]{};
    cvr::foveated::ngx::Result result{};
};

struct MockNgxEvaluateRecord {
    const cvr::foveated::ngx::Handle* handle{};
    unsigned width{}, height{}, outWidth{}, outHeight{}, perfQualityValue{}, createFlags{}, reset{};
    float mvScaleX{}, mvScaleY{}, jitterOffsetX{}, jitterOffsetY{};
    ID3D12Resource *color{}, *depth{}, *motionVectors{}, *output{};
    unsigned presets[6]{};
    cvr::foveated::ngx::Result result{};
};

struct MockNgxState {
    static constexpr size_t kMaxRecords = 256;
    uint64_t nextHandleId = 1, createCount = 0, evaluateCount = 0, evaluateCCount = 0, releaseCount = 0, aliveHandles = 0;
    bool failNextEvaluate = false, failNextCreate = false;
    size_t createRecordCount = 0, evaluateRecordCount = 0, releaseRecordCount = 0;
    MockNgxCreateRecord creates[kMaxRecords]{};
    MockNgxEvaluateRecord evaluates[kMaxRecords]{};
    cvr::foveated::ngx::Handle* releasedHandles[kMaxRecords]{};
};

extern "C" {
MOCK_NVNGX_API cvr::foveated::ngx::Result NVSDK_NGX_D3D12_CreateFeature(
    ID3D12GraphicsCommandList* list, unsigned feature, cvr::foveated::ngx::Parameters* p, cvr::foveated::ngx::Handle** outHandle);
MOCK_NVNGX_API cvr::foveated::ngx::Result NVSDK_NGX_D3D12_EvaluateFeature(
    ID3D12GraphicsCommandList* list, const cvr::foveated::ngx::Handle* handle, const cvr::foveated::ngx::Parameters* p, void* callback);
MOCK_NVNGX_API cvr::foveated::ngx::Result NVSDK_NGX_D3D12_EvaluateFeature_C(
    ID3D12GraphicsCommandList* list, const cvr::foveated::ngx::Handle* handle, const cvr::foveated::ngx::Parameters* p, void* callback);
MOCK_NVNGX_API cvr::foveated::ngx::Result NVSDK_NGX_D3D12_ReleaseFeature(cvr::foveated::ngx::Handle* handle);
MOCK_NVNGX_API void MockNgxReset();
MOCK_NVNGX_API bool MockNgxReadState(MockNgxState* outState);
MOCK_NVNGX_API void MockNgxSetFailNextEvaluate(bool fail);
MOCK_NVNGX_API void MockNgxSetFailNextCreate(bool fail);
MOCK_NVNGX_API bool MockNgxIsHandleAlive(const cvr::foveated::ngx::Handle* handle);
MOCK_NVNGX_API bool MockNgxGetHandleClearColor(const cvr::foveated::ngx::Handle* handle, float outRgba[4]);
}
