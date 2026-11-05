#pragma once
// PUREDARK'S ALTERNATE FRAME WARPING (AFW): the ABI of PDAFWPlugin.dll, the closed plugin PureDark's UEVR and
// REFramework AFW builds load (github.com/PureDark/UEVR, branch AFW; the declarations below follow its
// dependencies/pd-afwmod/include/PDAFWPlugin.h member for member -- the layout is the DLL's, so nothing here may be
// reordered or resized). The port ships no copy of the DLL: Render/Afw.cpp loads it from the plugin's folder.
//
// THE METHOD. The engine renders ONE eye per frame, the eyes taking turns. The plugin is given that eye's colour,
// depth and motion vectors, and the camera of both eyes for this frame; it makes the OTHER eye for the same moment:
//   - AlternateEyeWarping: the rendered eye reprojected across to the other eye by its depth;
//   - PreviousFrameWarping: the other eye's own previous frame moved to now by its motion vectors;
//   - CombinedWarping: both, the previous frame where the rendered eye cannot see (disocclusions), the default.
// Both eyes then go to the headset with this frame's pose: one rendered, one warped -- the cost of one eye.
#include <cstdint>
#include <d3d11_4.h>
#include <d3d12.h>
#include <dxgi1_2.h>

namespace pd {

// glm::mat4 as the DLL takes it: 16 floats, column-major (m[col * 4 + row]), 4-byte aligned.
struct Mat4 { float m[16]; };

struct DeviceParams {
    ID3D11Device* d3d11Device = nullptr;
    ID3D11DeviceContext* d3d11Context = nullptr;
    ID3D12Device* d3d12Device = nullptr;
    ID3D12CommandQueue* d3d12Queue = nullptr;
    IDXGIAdapter* dxgiAdapter = nullptr;
};

struct FrameWarpInitParams {
    int hmdWidth;
    int hmdHeight;
    DXGI_FORMAT eyeFormat = DXGI_FORMAT_R8G8B8A8_UNORM;
    DXGI_FORMAT backbufferFormat = DXGI_FORMAT_R10G10B10A2_UNORM;
};

enum EyeIndex { EyeLeft = 0, EyeRight = 1 };
enum FrameWarpMode { None, AlternateEyeWarping, PreviousFrameWarping, CombinedWarping };
enum ImageType { Image, Depth };

struct VextexBufferDesc {
    ID3D12Resource* pVextexBuffer = nullptr;
    D3D12_VERTEX_BUFFER_VIEW vertexBufferView;
    D3D12_RESOURCE_STATES initialState = D3D12_RESOURCE_STATE_COMMON;
};

struct TextureDesc {
    TextureDesc() {}
    ImageType type = Image;
    ID3D12Resource* pTexture = nullptr;
    int srvPos = -1;
    int uavPos = -1;
    D3D12_GPU_DESCRIPTOR_HANDLE shaderResourceViewHandle{0};
    D3D12_GPU_DESCRIPTOR_HANDLE unorderedAccessViewHandle{0};
    union {
        D3D12_CPU_DESCRIPTOR_HANDLE renderTargetViewHandle{0};
        D3D12_CPU_DESCRIPTOR_HANDLE depthStencilViewHandle;
    };
    D3D12_RESOURCE_STATES initialState = D3D12_RESOURCE_STATE_COMMON;
};

struct FrameBufferDesc {
    FrameBufferDesc() {}
    TextureDesc color;
    TextureDesc depth;
    TextureDesc motionVectors;
};

struct EyeFrameBuffers { FrameBufferDesc eyeFrameBuffers[2]; };

// src: the eye rendered this frame; dest: the eye to make, at this frame's pose. cam*: only for a UI the plugin draws
// itself (unused here).
struct CameraData {
    Mat4 destWorldToViewMatrix, destViewToWorldMatrix, destViewToClipMatrix, destClipToViewMatrix;
    Mat4 srcWorldToViewMatrix, srcViewToWorldMatrix, srcViewToClipMatrix, srcClipToViewMatrix;
    Mat4 camWorldToViewMatrix, camViewToWorldMatrix, camViewToClipMatrix, camClipToViewMatrix;
};

struct CameraDataMVCorrection {
    Mat4 srcWorldToViewMatrix, srcViewToWorldMatrix, srcViewToClipMatrix, srcClipToViewMatrix;
    Mat4 srcWorldToViewMatrixPrev, srcViewToWorldMatrixPrev, srcViewToClipMatrixPrev, srcClipToViewMatrixPrev;
    Mat4 destWorldToViewMatrix, destViewToWorldMatrix, destViewToClipMatrix, destClipToViewMatrix;
    Mat4 destWorldToViewMatrixPrev, destViewToWorldMatrixPrev, destViewToClipMatrixPrev, destClipToViewMatrixPrev;
};

enum MVType {
    Normal,        // current eye this frame -> current eye last frame
    FromOtherEye,  // current eye this frame -> other eye last frame (what an engine taking turns produces)
    ObjectOnly     // object motion only, no camera motion
};

struct TonemapParams { float fGamma, fLowerLimit, fUpperLimit, fConvertToLimit; };

enum CorrectMVType { SwapCameraMotion, ExtractObjectMotion, ScaleObjectMotion, FixUEObjectMotion };

struct CorrectMotionVectorsParams {
    TextureDesc* InMotionVectors = nullptr;
    TextureDesc* InDepth = nullptr;
    CameraDataMVCorrection* CameraData = nullptr;
    float InMotionScale[2] = {0.0f, 0.0f};
    CorrectMVType CorrectMVType = SwapCameraMotion;
    float ObjectMotionScale = 1.0f;
    TextureDesc* InUEVelocityPrev = nullptr;
    TextureDesc* InDepthPrev = nullptr;
    float FixUEObjMotionRange = 3.0f;
    float IgnoreMotionThreshold{2.5f};
    float Reserved[8] = {0, 0};
};

constexpr int kMaxShadingRates = 9;
enum class ShadingRate1D : uint32_t { X1 = 1 << 0, X2 = 1 << 1, X4 = 1 << 2 };
enum class ShadingRate : uint32_t {
    R1X1 = (1u << 3) | 1u, R1X2 = (1u << 3) | 2u, R1X4 = (1u << 3) | 4u, R2X1 = (2u << 3) | 1u, R2X2 = (2u << 3) | 2u,
    R2X4 = (2u << 3) | 4u, R4X1 = (4u << 3) | 1u, R4X2 = (4u << 3) | 2u, R4X4 = (4u << 3) | 4u
};
enum class ShadingRateCombiner : uint32_t { Passthrough = 1, Override = 2, Min = 4, Max = 8, Sum = 16, Mul = 32 };

struct VRSInfo {
    bool AdditionalShadingRatesSupported = false;
    ShadingRate ShadingRates[kMaxShadingRates];
    uint32_t NumShadingRates = 0;
    ShadingRateCombiner Combiners;
    uint32_t MinTileSize[2];
    uint32_t MaxTileSize[2];
};
struct VRSFovRadius { float radius1x1, radius1x2, radius2x2, radius2x4; };
enum VRSAlgorithm { LuminanceAndMotionVectors = 0x1, Foveated = 0x2, Combined = 0x3 };
struct VRSParams {
    VRSAlgorithm vrsAlgorithm = Foveated;
    ID3D12Resource* historyColor = nullptr;
    ID3D12Resource* motionVectors = nullptr;
    float renderSize[2] = {0, 0};
    float varianceCutoff = 0;
    float motionFactor = 0;
    uint32_t tileSize = 0;
    float motionVectorScale[2] = {0, 0};
    float foveationCenter[2] = {0, 0};
    VRSFovRadius foveationRadius;
};
struct FoveatedCompositeParams {
    float fFadeLeft = 0.05f, fFadeRight = 0.05f, fFadeTop = 0.05f, fFadeBottom = 0.05f, fRoundedRadius = 0.1f;
};
enum BlendType { NoBlend, OneMinusSrcAlpha, PremulAlpha };

struct FrameWarpEvaluateParams {
    void* InCmdList = nullptr;                    // the caller's list (else the plugin's own, executed at once)
    FrameBufferDesc* InEyeFrameBuffer = nullptr;  // the rendered eye; pixel-shader-resource state
    FrameBufferDesc* OutEyeFrameBuffer = nullptr;
    TextureDesc* InUIColorAlpha = nullptr;
    float InUIScale[2] = {1.0f, 1.0f};
    float InUIPos[3] = {0.0f, 0.0f, -1.0f};
    float InMotionScale[2] = {0.0f, 0.0f};        // raw motion vector -> output pixels
    FrameWarpMode Mode;
    EyeIndex EyeIndex;
    CameraData* CameraData;
    bool ClearBeforeWarping = false;
    float IgnoreMotionThreshold{2.5f};
    bool IsHudlessColor = true;
    MVType MotionVectorsType = Normal;
    bool Debug = false;
    bool IsFoveated = false;
    RECT FoveatedArea = {};
    TextureDesc* InUEVelocityBuffer = nullptr;
    bool UseUINT64 = false;
    ShadingRate ShadingRate = ShadingRate::R1X1;
    float Reserved[12] = {0, 0};
};

struct __declspec(novtable) D3D12RendererAPI {
    virtual ID3D12GraphicsCommandList* BeginCommandList(int index) = 0;
    virtual void EndCommandList(int index) = 0;
    virtual void WaitIdle(int index) = 0;
    virtual ID3D12Device* GetDevice() = 0;
    virtual ID3D12CommandQueue* GetCommandQueue() = 0;
    virtual ID3D12DescriptorHeap* GetViewHeap() = 0;
    virtual D3D12_CPU_DESCRIPTOR_HANDLE GetRTV(ID3D12Resource* resource) = 0;
    virtual D3D12_CPU_DESCRIPTOR_HANDLE GetDSV(ID3D12Resource* resource) = 0;
    virtual int CreateSRV(ID3D12Resource* resource, int pos = -1) = 0;
    virtual int CreateUAV(ID3D12Resource* resource, int pos = -1) = 0;
    virtual int CreateCBV(ID3D12Resource* resource, int pos = -1) = 0;
    virtual D3D12_CPU_DESCRIPTOR_HANDLE GetCPUDescriptorHandle(int pos) = 0;
    virtual D3D12_GPU_DESCRIPTOR_HANDLE GetGPUDescriptorHandle(int pos) = 0;
    virtual D3D12_GPU_DESCRIPTOR_HANDLE GetSamplerHandle(int pos) = 0;
    virtual void SetupTextureDesc(TextureDesc& srcDesc) = 0;
    virtual bool CreateVertexBuffer(ID3D12GraphicsCommandList* cmdList, VextexBufferDesc& vextexDesc, uint32_t vertexCount,
                                    uint32_t vertexSize, float* pVertexData) = 0;
    virtual bool CreateTexture(int nWidth, int nHeight, DXGI_FORMAT format, D3D12_RESOURCE_STATES initialState,
                               TextureDesc& textureDesc, bool createUAV) = 0;
    virtual bool CreateFrameBuffer(int nWidth, int nHeight, FrameBufferDesc& framebufferDesc,
                                   D3D12_RESOURCE_STATES initialState, bool createUAV) = 0;
    virtual void Clear(ID3D12GraphicsCommandList* cmdList, TextureDesc& texDesc, const FLOAT ColorRGBA[4]) = 0;
    virtual void Blit(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc,
                      D3D12_VIEWPORT viewPort = {}, BlendType enableBlend = NoBlend, bool isCS = false) = 0;
    virtual void Copy(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc,
                      D3D12_BOX srcBox = {}, UINT dstX = 0, UINT dstY = 0) = 0;
    virtual void Sharpen(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc, float sharpness) = 0;
    virtual void Tonemap(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc, TonemapParams params) = 0;
    virtual void ExtractUI(ID3D12GraphicsCommandList* cmdList, TextureDesc& extactedUIDesc, TextureDesc& hudlessDesc,
                           TextureDesc& finalColorWithUI) = 0;
    virtual void CorrectMotionVectors(ID3D12GraphicsCommandList* cmdList, TextureDesc& correctedMVDesc,
                                      CorrectMotionVectorsParams& params) = 0;
    virtual void FoveatedComposite(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc,
                                   D3D12_VIEWPORT viewPort = {}, FoveatedCompositeParams params = {}) = 0;
    virtual void Blur(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc, float blurRadius) = 0;
    virtual void Crop(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc, D3D12_BOX srcBox = {},
                      D3D12_VIEWPORT viewPort = {}) = 0;
    virtual void ApplyHiddenAreaMesh(ID3D12GraphicsCommandList* cmdList, TextureDesc& depthDesc, D3D12_VIEWPORT viewPort,
                                     VextexBufferDesc& vextexDesc) = 0;
    virtual void GenerateVRSImage(ID3D12GraphicsCommandList* cmdList, TextureDesc& vrsImageDesc, VRSParams params) = 0;
    virtual VRSInfo GetVRSInfo() = 0;
    virtual void ShowVRSOverlay(ID3D12GraphicsCommandList* cmdList, TextureDesc& dstDesc, TextureDesc& srcDesc) = 0;
};

using InitDeviceFn = D3D12RendererAPI*(__stdcall*)(DeviceParams);
using InitFrameWarpFn = EyeFrameBuffers(__stdcall*)(FrameWarpInitParams);
using EvaluateFrameWarpFn = void(__stdcall*)(FrameWarpEvaluateParams&);

}  // namespace pd
