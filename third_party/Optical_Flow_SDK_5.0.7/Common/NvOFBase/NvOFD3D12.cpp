/*
* Copyright (c) 2018-2023 NVIDIA Corporation
*
* Permission is hereby granted, free of charge, to any person
* obtaining a copy of this software and associated documentation
* files (the "Software"), to deal in the Software without
* restriction, including without limitation the rights to use,
* copy, modify, merge, publish, distribute, sublicense, and/or sell
* copies of the software, and to permit persons to whom the
* software is furnished to do so, subject to the following
* conditions:
*
* The above copyright notice and this permission notice shall be
* included in all copies or substantial portions of the Software.
*
* THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
* EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES
* OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
* NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
* HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
* WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
* FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
* OTHER DEALINGS IN THE SOFTWARE.
*/


#include "NvOFD3D12.h"
#include "NvOFD3DCommon.h"

void CreateBufferParamsD3D12(D3D12_HEAP_PROPERTIES* pHeapProps, D3D12_RESOURCE_DESC* pDesc, DXGI_FORMAT format,
    NV_OF_BUFFER_DESCRIPTOR nvOfDesc, uint32_t outputGridSize, uint32_t hintGridSize)
{
    uint32_t widthAligned = 0;
    uint32_t heightAligned = 0;
    uint32_t width = nvOfDesc.width;
    uint32_t height = nvOfDesc.height;

    switch (nvOfDesc.bufferUsage)
    {
    case NV_OF_BUFFER_USAGE_INPUT:
        widthAligned = nvOfDesc.width;
        heightAligned = nvOfDesc.height;
        break;
    case NV_OF_BUFFER_USAGE_OUTPUT:
        widthAligned = ((width + outputGridSize - 1) & (~(outputGridSize - 1))) / outputGridSize;
        heightAligned = ((height + outputGridSize - 1) & (~(outputGridSize - 1))) / outputGridSize;
        break;
    case NV_OF_BUFFER_USAGE_COST:
        widthAligned = ((width + outputGridSize - 1) & (~(outputGridSize - 1))) / outputGridSize;
        heightAligned = ((height + outputGridSize - 1) & (~(outputGridSize - 1))) / outputGridSize;
        break;
    case NV_OF_BUFFER_USAGE_HINT:
        widthAligned = ((width + hintGridSize - 1) & (~(hintGridSize - 1))) / hintGridSize;
        heightAligned = ((height + hintGridSize - 1) & (~(hintGridSize - 1))) / hintGridSize;
        break;
    default:
        NVOF_THROW_ERROR("Invalid buffer format", NV_OF_ERR_INVALID_PARAM);
    }

    if (pHeapProps && pDesc)
    {
        ZeroMemory(pHeapProps, sizeof(D3D12_HEAP_PROPERTIES));
        pHeapProps->Type = D3D12_HEAP_TYPE_DEFAULT;

        ZeroMemory(pDesc, sizeof(D3D12_RESOURCE_DESC));
        pDesc->Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
        pDesc->Width = widthAligned;
        pDesc->Height = heightAligned;
        pDesc->DepthOrArraySize = 1;
        pDesc->MipLevels = 1;
        pDesc->Format = format;
        pDesc->SampleDesc.Count = 1;
        pDesc->Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    }
}

NvOFD3D12API::NvOFD3D12API(ID3D12Device* device)
    : m_device(device)
    , m_hOF(nullptr)
{
    typedef NV_OF_STATUS(NVOFAPI *PFNNvOFAPICreateInstanceD3D12)(uint32_t apiVer, NV_OF_D3D12_API_FUNCTION_LIST  *d3d11OF);
    PFNNvOFAPICreateInstanceD3D12 NvOFAPICreateInstanceD3D12 = (PFNNvOFAPICreateInstanceD3D12)GetProcAddress(m_hModule, "NvOFAPICreateInstanceD3D12");
    if (!NvOFAPICreateInstanceD3D12)
    {
        NVOF_THROW_ERROR("Cannot find NvOFAPICreateInstanceDX12() entry in NVOF library", NV_OF_ERR_OF_NOT_AVAILABLE);
    }

    m_ofAPI.reset(new NV_OF_D3D12_API_FUNCTION_LIST());
    NV_OF_STATUS status = NvOFAPICreateInstanceD3D12(NV_OF_API_VERSION, m_ofAPI.get());
    if (status != NV_OF_SUCCESS)
    {
        NVOF_THROW_ERROR("Cannot fetch function list", status);
    }
    status = m_ofAPI->nvCreateOpticalFlowD3D12(device, &m_hOF);
    if (status != NV_OF_SUCCESS || m_hOF == nullptr)
    {
        NVOF_THROW_ERROR("Cannot create D3D12 optical flow device", status);
    }

    D3D12_COMMAND_QUEUE_DESC cmdQDesc = { D3D12_COMMAND_LIST_TYPE_DIRECT };

    D3D_API_CALL(device->CreateCommandQueue(&cmdQDesc, IID_PPV_ARGS(&m_pCmdQ)));
    D3D_API_CALL(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&m_pCmdAlloc)));
    D3D_API_CALL(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, m_pCmdAlloc.Get(), nullptr, IID_PPV_ARGS(&m_pCmdList)));
    m_pCmdList->Close();
}

NvOFD3D12API::~NvOFD3D12API()
{
    if (m_ofAPI)
    {
        m_ofAPI->nvOFDestroy(m_hOF);
    }
}

NvOFObj NvOFD3D12::Create(ID3D12Device* d3dDevice, uint32_t nWidth, uint32_t nHeight, NV_OF_BUFFER_FORMAT eInBufFmt,
    NV_OF_MODE eMode, NV_OF_PERF_LEVEL preset)
{
    std::unique_ptr<NvOF> ofObj(new NvOFD3D12(d3dDevice,
        nWidth,
        nHeight,
        eInBufFmt,
        eMode,
        preset));
    return ofObj;
}

NvOFD3D12::NvOFD3D12(ID3D12Device* d3dDevice, uint32_t nWidth, uint32_t nHeight, NV_OF_BUFFER_FORMAT eInBufFmt, NV_OF_MODE eMode,
    NV_OF_PERF_LEVEL preset)
    : NvOF(nWidth, nHeight, eInBufFmt, eMode, preset),
    m_d3dDevice(d3dDevice)
 {
    m_NvOFAPI = std::make_shared<NvOFD3D12API>(m_d3dDevice.Get());
    uint32_t formatCount = 0;
    bool bInputFormatSupported = false;
    bool bOutputFormatSupported = false;

    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFGetSurfaceFormatCountD3D12(m_NvOFAPI->GetHandle(), NV_OF_BUFFER_USAGE_INPUT, m_ofMode, &formatCount));
    std::unique_ptr<DXGI_FORMAT[]> pDxgiFormat(new DXGI_FORMAT[formatCount]);
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFGetSurfaceFormatD3D12(m_NvOFAPI->GetHandle(), NV_OF_BUFFER_USAGE_INPUT, m_ofMode, pDxgiFormat.get()));

    for (uint32_t i = 0; i < formatCount; ++i)
    {
        if (m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].bufferFormat == DXGIFormatToNvOFBufferFormat(pDxgiFormat[i]))
        {
            bInputFormatSupported = true;
        }
    }
    auto outBufFmt = (m_ofMode == NV_OF_MODE_OPTICALFLOW) ? NV_OF_BUFFER_FORMAT_SHORT2 : NV_OF_BUFFER_FORMAT_SHORT;
    formatCount = 0;
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFGetSurfaceFormatCountD3D12(m_NvOFAPI->GetHandle(), NV_OF_BUFFER_USAGE_OUTPUT, m_ofMode, &formatCount));
    pDxgiFormat.reset(new DXGI_FORMAT[formatCount]);
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFGetSurfaceFormatD3D12(m_NvOFAPI->GetHandle(), NV_OF_BUFFER_USAGE_OUTPUT, m_ofMode, pDxgiFormat.get()));

    for (uint32_t i = 0; i < formatCount; ++i)
    {
        if (outBufFmt == DXGIFormatToNvOFBufferFormat(pDxgiFormat[i]))
        {
            bOutputFormatSupported = true;
        }
    }

    if (!bOutputFormatSupported || !bInputFormatSupported)
    {
        NVOF_THROW_ERROR("Invalid buffer format", NV_OF_ERR_INVALID_PARAM);
    }
}

void NvOFD3D12::DoGetOutputGridSizes(uint32_t* vals, uint32_t* size)
{
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFGetCaps(m_NvOFAPI->GetHandle(), NV_OF_CAPS_SUPPORTED_OUTPUT_GRID_SIZES, vals, size));
}

void NvOFD3D12::DoGetROISupport(uint32_t* vals, uint32_t* size)
{
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFGetCaps(m_NvOFAPI->GetHandle(), NV_OF_CAPS_SUPPORT_ROI, vals, size));
}

void NvOFD3D12::DoExecute(const NV_OF_EXECUTE_INPUT_PARAMS& executeInParams,  NV_OF_EXECUTE_OUTPUT_PARAMS& executeOutParams,  void* arrInputFencePoint, uint32_t numInputFencePoint, void* pOutputFencePoint)
{
    NV_OF_EXECUTE_INPUT_PARAMS_D3D12 executeInParamsD3D12;
    memcpy(&executeInParamsD3D12, &executeInParams, sizeof(executeInParams));

    if (arrInputFencePoint == nullptr)
    {
        NVOF_THROW_ERROR("arrInputFencePoint must be set to an array of NV_OF_FENCE_POINT. Execute() will wait for these fences to reach before execution", NV_OF_ERR_INVALID_PARAM);
    }
    if (numInputFencePoint == 0)
    {
        NVOF_THROW_ERROR("numInputFencePoint must be non-zero", NV_OF_ERR_INVALID_PARAM);
    }
    if (pOutputFencePoint == nullptr)
    {
        NVOF_THROW_ERROR("pOuputFencePoint must be set to a NV_OF_FENCE_POINT pointer", NV_OF_ERR_INVALID_PARAM);
    }

    executeInParamsD3D12.fencePoint = (NV_OF_FENCE_POINT*) arrInputFencePoint;
    executeInParamsD3D12.numFencePoints = numInputFencePoint;

    NV_OF_EXECUTE_OUTPUT_PARAMS_D3D12 executeOutParamsD3D12;
    memcpy(&executeOutParamsD3D12, &executeOutParams, sizeof(executeOutParams));
    executeOutParamsD3D12.fencePoint = (NV_OF_FENCE_POINT*)pOutputFencePoint;
   
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFExecuteD3D12(m_NvOFAPI->GetHandle(), &executeInParamsD3D12, &executeOutParamsD3D12));
}

void NvOFD3D12::DoInit(const NV_OF_INIT_PARAMS& initParams)
{
    NVOF_API_CALL(m_NvOFAPI->GetAPI()->nvOFInit(m_NvOFAPI->GetHandle(), &initParams));
}

std::vector<NvOFBufferObj>
NvOFD3D12::DoAllocBuffers(NV_OF_BUFFER_DESCRIPTOR ofBufferDesc,
    uint32_t elementSize, uint32_t numBuffers, void* arrOutputFencePoint, uint32_t numOutputFencePoint)
{
    std::vector<std::unique_ptr<NvOFBuffer>> ofBuffers;
    NV_OF_FENCE_POINT inputFence;
    D3D_API_CALL(m_d3dDevice->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&(inputFence.fence))));
    inputFence.value = 0;
    if (numBuffers != numOutputFencePoint)
    {
        NVOF_THROW_ERROR("numBuffers must be equal to numOutputFencePoint", NV_OF_ERR_INVALID_PARAM);
    }
    for (uint32_t i = 0; i < numBuffers; ++i)
    {
        D3D12_HEAP_PROPERTIES heapProps;
        D3D12_RESOURCE_DESC desc;
        ID3D12Resource* pResource = nullptr; 
        
        CreateBufferParamsD3D12(&heapProps, &desc, NvOFBufferFormatToDxgiFormat(ofBufferDesc.bufferFormat), ofBufferDesc, m_nOutGridSize, m_nHintGridSize);
        D3D_API_CALL(m_d3dDevice.Get()->CreateCommittedResource(&heapProps, D3D12_HEAP_FLAG_NONE,
            &desc, D3D12_RESOURCE_STATE_COMMON, nullptr,
            IID_PPV_ARGS(&pResource)));
        
        NV_OF_FENCE_POINT* outputFenceArray = (NV_OF_FENCE_POINT*)arrOutputFencePoint;
        NV_OF_FENCE_POINT outputFence = outputFenceArray[i];
        ofBuffers.emplace_back(new NvOFBufferD3D12<RWPolicyDeviceAndHost>(m_NvOFAPI, ofBufferDesc, elementSize, pResource, &inputFence, &outputFence));
    }
    return ofBuffers;
}

NvOFBufferObj NvOFD3D12::DoRegisterBuffers(NV_OF_BUFFER_DESCRIPTOR ofBufferDesc,
    uint32_t elementSize, const void* pResource, void* inputFencePoint, void* outputFencePoint)
{
    ID3D12Resource* pRes = reinterpret_cast<ID3D12Resource*>(const_cast<void*>(pResource));
    D3D12_RESOURCE_DESC inputDesc = pRes->GetDesc();
    bool formatMatches = (inputDesc.Format == NvOFBufferFormatToDxgiFormat(ofBufferDesc.bufferFormat));
    if (!formatMatches && ofBufferDesc.bufferFormat == NV_OF_BUFFER_FORMAT_ABGR8) {
        formatMatches = (inputDesc.Format == DXGI_FORMAT_R8G8B8A8_UNORM);
    }
    bool bError = !formatMatches;
    bError &= (inputDesc.Width < ofBufferDesc.width);
    bError &= (inputDesc.Height < ofBufferDesc.height);
    if (bError)
    {
        NVOF_THROW_ERROR("Resource does not match with params provided during NvOF Init", NV_OF_ERR_INVALID_PARAM);
    }
    NvOFBufferObj ofBuffer(new NvOFBufferD3D12<RWPolicyDeviceAndHost>(m_NvOFAPI, ofBufferDesc, elementSize,
        pRes, (NV_OF_FENCE_POINT*)inputFencePoint, (NV_OF_FENCE_POINT*)outputFencePoint));
    return ofBuffer;
}

template<typename RWPolicy>
NvOFBufferD3D12<RWPolicy>::NvOFBufferD3D12(std::shared_ptr<NvOFD3D12API> ofD3D12, const NV_OF_BUFFER_DESCRIPTOR& nvBufDesc,
    uint32_t elementSize, ID3D12Resource* pResource, NV_OF_FENCE_POINT* inputFence, NV_OF_FENCE_POINT* outputFence ) :
    NvOFBuffer(nvBufDesc, elementSize), RWPolicy(ofD3D12.get(), nvBufDesc, pResource),m_nvOFD3D12(ofD3D12)
{
    Microsoft::WRL::ComPtr<ID3D12Device> dx12Device = ofD3D12->GetDevice();
    
    NV_OF_REGISTER_RESOURCE_PARAMS_D3D12 registerParams{};
    registerParams.resource = pResource;
    registerParams.inputFencePoint.fence = inputFence->fence;
    registerParams.inputFencePoint.value = inputFence->value; 
    registerParams.hOFGpuBuffer = &m_hGPUBuffer;
    registerParams.outputFencePoint.fence = outputFence->fence;
    registerParams.outputFencePoint.value = outputFence->value;
    NVOF_API_CALL(ofD3D12->GetAPI()->nvOFRegisterResourceD3D12(ofD3D12->GetHandle(), &registerParams));

    m_format = NvOFBufferFormatToDxgiFormat(nvBufDesc.bufferFormat);
}

template<typename RWPolicy>
NvOFBufferD3D12<RWPolicy>::~NvOFBufferD3D12()
{
    NV_OF_UNREGISTER_RESOURCE_PARAMS_D3D12 param;
    param.hOFGpuBuffer = getOFBufferHandle();
    m_nvOFD3D12.get()->GetAPI()->nvOFUnregisterResourceD3D12(&param);
}

template<typename RWPolicy>
void NvOFBufferD3D12<RWPolicy>::UploadData(const void* pData, void* inputFencePoint, void* outputFencePoint)
{
    RWPolicy::UploadData(pData, m_nvOFD3D12.get(), inputFencePoint, outputFencePoint);
}

template<typename RWPolicy>
void NvOFBufferD3D12<RWPolicy>::DownloadData(void* data, void* pInputFencePoint)
{
    RWPolicy::DownloadData(data, m_nvOFD3D12.get(), pInputFencePoint);
}

RWPolicyDeviceAndHost::RWPolicyDeviceAndHost(NvOFD3D12API* nvofD3D, NV_OF_BUFFER_DESCRIPTOR nvofBufDesc, ID3D12Resource* pGPUResource):
    m_desc(nvofBufDesc),
    m_resource(pGPUResource),
    m_internalFenceCounter(0)
{

    D3D12_RESOURCE_DESC desc = pGPUResource->GetDesc();
    uint32_t numSubresources = GetNumberOfPlanes(desc.Format);
    m_numSubResources = numSubresources;
    UINT64 rowSizeInBytes[MAX_SUBRESOURCES] = { 0 };
    nvofD3D->GetDevice()->GetCopyableFootprints(&desc, 0, m_numSubResources, 0, m_layout, &m_numRows, m_rowSizeInBytes, &m_totalBytes);
    {
        uint32_t size = 0;
        for (uint32_t subresourceId = 0; subresourceId < numSubresources; subresourceId++)
        {
            size += m_layout[subresourceId].Footprint.RowPitch * m_layout[subresourceId].Footprint.Height;
        }
        size -= (m_layout[numSubresources - 1].Footprint.RowPitch - (uint32_t)m_rowSizeInBytes[numSubresources - 1]);
    }
   AllocateStagingBuffer(nvofBufDesc, nvofD3D->GetDevice().Get(), pGPUResource);
   D3D_API_CALL(nvofD3D->GetDevice()->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&m_internalFence)));
}

void RWPolicyDeviceAndHost::AllocateStagingBuffer(NV_OF_BUFFER_DESCRIPTOR ofBufDesc, ID3D12Device* pDevice, ID3D12Resource* pGPUResource)
{
    D3D12_RESOURCE_STATES state = {};
    D3D12_HEAP_PROPERTIES heapProps;
    D3D12_RESOURCE_DESC desc;

    ZeroMemory(&heapProps, sizeof(D3D12_HEAP_PROPERTIES));
    ZeroMemory(&desc, sizeof(D3D12_RESOURCE_DESC));

    switch (ofBufDesc.bufferUsage)
    {
    case NV_OF_BUFFER_USAGE_INPUT:
    case NV_OF_BUFFER_USAGE_HINT:
        heapProps.Type = D3D12_HEAP_TYPE_UPLOAD;
        state = D3D12_RESOURCE_STATE_GENERIC_READ;
        break;
    case NV_OF_BUFFER_USAGE_OUTPUT:
    case NV_OF_BUFFER_USAGE_COST:
        heapProps.Type = D3D12_HEAP_TYPE_READBACK;
        state = D3D12_RESOURCE_STATE_COPY_DEST;
        break;
    default:
        NVOF_THROW_ERROR("Invalid buffer format", NV_OF_ERR_INVALID_PARAM); 
    }

    desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    desc.Width = m_totalBytes;
    desc.Height = 1;
    desc.DepthOrArraySize = 1;
    desc.MipLevels = 1;
    desc.Format = DXGI_FORMAT_UNKNOWN;
    desc.SampleDesc.Count = 1;
    desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D_API_CALL(pDevice->CreateCommittedResource(&heapProps, D3D12_HEAP_FLAG_NONE,
        &desc, state, nullptr, IID_PPV_ARGS(&m_stagingResource)));
}

void RWPolicyDeviceAndHost::UploadData(const void* pSysMem, NvOFD3D12API* nvof, void* inputFencePoint, void* outputFencePoint)
{
    auto pGfxCmdAlloc = nvof->GetCmdAlloc();
    auto pGfxCmdList = nvof->GetCmdList();
    auto pGfxCmdQ = nvof->GetCmdQ();

    auto inputFence = (NV_OF_FENCE_POINT*) inputFencePoint;
    auto outputFence = (NV_OF_FENCE_POINT*) outputFencePoint;
    if (inputFence->fence->GetCompletedValue() < inputFence->value)
    {
        HANDLE event = CreateEvent(nullptr, FALSE, FALSE, nullptr);
        D3D_API_CALL(inputFence->fence->SetEventOnCompletion(inputFence->value, event));
        WaitForSingleObject(event, INFINITE);
        CloseHandle(event);
    }
    D3D_API_CALL(pGfxCmdAlloc->Reset());
    D3D_API_CALL(pGfxCmdList->Reset(pGfxCmdAlloc, nullptr));
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Flags = D3D12_RESOURCE_BARRIER_FLAG_NONE;
    barrier.Transition.pResource = m_resource.Get();
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_DEST;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    pGfxCmdList->ResourceBarrier(1, &barrier);

    void* pData;
    D3D_API_CALL(m_stagingResource.Get()->Map(0, nullptr, &pData));
    uint32_t offset = 0;
    for (uint32_t subresourceId = 0; subresourceId < m_numSubResources; subresourceId++)
    {
        uint8_t* pDst = (uint8_t*)pData + m_layout[subresourceId].Offset;
        uint8_t* pSrc = (uint8_t*)pSysMem + offset;
        uint32_t width = (uint32_t)m_rowSizeInBytes[subresourceId];
        uint32_t pitch = m_layout[subresourceId].Footprint.RowPitch;
        uint32_t height = m_layout[subresourceId].Footprint.Height;
        for (uint32_t y = 0; y < height; y++)
        {
            memcpy(pDst + y * pitch, pSrc + y * width, width);
        }
        offset += width * height;
    }
    m_stagingResource->Unmap(0, nullptr);

    for (uint32_t subresourceId = 0; subresourceId < m_numSubResources; subresourceId++)
    {
        D3D12_TEXTURE_COPY_LOCATION copyDst{};
        copyDst.pResource = m_resource.Get();
        copyDst.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        copyDst.SubresourceIndex = subresourceId;

        D3D12_TEXTURE_COPY_LOCATION copySrc{};
        copySrc.pResource = m_stagingResource.Get();
        copySrc.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        copySrc.PlacedFootprint = m_layout[subresourceId];

        pGfxCmdList->CopyTextureRegion(&copyDst, 0, 0, 0, &copySrc, nullptr);
    }

    D3D12_RESOURCE_BARRIER barrier2{};
    barrier2.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier2.Flags = D3D12_RESOURCE_BARRIER_FLAG_NONE;
    barrier2.Transition.pResource = m_resource.Get();
    barrier2.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    barrier2.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    barrier2.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    pGfxCmdList->ResourceBarrier(1, &barrier2);

    D3D_API_CALL(pGfxCmdList->Close()); 
    ID3D12CommandList* const ppCmdList[] = { pGfxCmdList };
    pGfxCmdQ->ExecuteCommandLists(1, ppCmdList);

    D3D_API_CALL(pGfxCmdQ->Signal(outputFence->fence, outputFence->value));
}

void RWPolicyDeviceAndHost::DownloadData(void* pSysMem, NvOFD3D12API* nvof, void* pInputFencePoint)
{
    ID3D12CommandQueue* pGfxCmdQ = nvof->GetCmdQ();
    ID3D12CommandAllocator* pGfxCmdAlloc = nvof->GetCmdAlloc();
    ID3D12GraphicsCommandList* pGfxCmdList = nvof->GetCmdList();

    auto inputFence = (NV_OF_FENCE_POINT*)pInputFencePoint;
    D3D_API_CALL(pGfxCmdList->Reset(pGfxCmdAlloc, nullptr));
    D3D_API_CALL(pGfxCmdQ->Wait(inputFence->fence, inputFence->value));
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Flags = D3D12_RESOURCE_BARRIER_FLAG_NONE;
    barrier.Transition.pResource = m_resource.Get();
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    pGfxCmdList->ResourceBarrier(1, &barrier);

    for (uint32_t subresourceId = 0; subresourceId < m_numSubResources; subresourceId++)
    {
        D3D12_TEXTURE_COPY_LOCATION copyDst{};
        copyDst.pResource = m_stagingResource.Get();
        copyDst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        copyDst.PlacedFootprint = m_layout[subresourceId];

        D3D12_TEXTURE_COPY_LOCATION copySrc{};
        copySrc.pResource = m_resource.Get();
        copySrc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        copySrc.SubresourceIndex = subresourceId;

        pGfxCmdList->CopyTextureRegion(&copyDst, 0, 0, 0, &copySrc, nullptr);
    }

    D3D12_RESOURCE_BARRIER barrier2{};
    barrier2.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier2.Flags = D3D12_RESOURCE_BARRIER_FLAG_NONE;
    barrier2.Transition.pResource = m_resource.Get();
    barrier2.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
    barrier2.Transition.StateAfter = D3D12_RESOURCE_STATE_COMMON;
    barrier2.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    pGfxCmdList->ResourceBarrier(1, &barrier2);

    D3D_API_CALL(pGfxCmdList->Close());

    ID3D12CommandList* const ppCommandList[] = { pGfxCmdList };
    pGfxCmdQ->ExecuteCommandLists(1, ppCommandList);
    D3D_API_CALL(pGfxCmdQ->Signal(m_internalFence.Get(), ++m_internalFenceCounter));

    if (m_internalFence->GetCompletedValue() < m_internalFenceCounter)
    {
        HANDLE event = CreateEvent(nullptr, FALSE, FALSE, nullptr);
        D3D_API_CALL(m_internalFence->SetEventOnCompletion(m_internalFenceCounter, event));
        WaitForSingleObject(event, INFINITE);
        CloseHandle(event);
    }

    void* pData;
    D3D_API_CALL(m_stagingResource.Get()->Map(0, nullptr, &pData));
    uint32_t offset = 0;
    for (uint32_t subresourceId = 0; subresourceId < m_numSubResources; subresourceId++)
    {
        uint8_t* pDst = (uint8_t*)pSysMem + offset;
        uint8_t* pSrc = (uint8_t*)pData + m_layout[subresourceId].Offset;
        uint32_t width = (uint32_t)m_rowSizeInBytes[subresourceId];
        uint32_t pitch = m_layout[subresourceId].Footprint.RowPitch;
        uint32_t height =m_layout[subresourceId].Footprint.Height;
        for (uint32_t y = 0; y < height; y++)
        {
            memcpy(pDst + y * width, pSrc + y * pitch, width);
        }
        offset += width * height;
    }
    m_stagingResource->Unmap(0, nullptr);
}
