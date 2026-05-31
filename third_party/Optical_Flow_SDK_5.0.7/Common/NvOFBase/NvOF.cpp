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


#ifdef _WIN32
#include <Windows.h>
#else
#include <dlfcn.h>
#endif

#include "NvOF.h"

NvOF::NvOF(uint32_t nWidth, uint32_t nHeight, NV_OF_BUFFER_FORMAT eInBufFmt, NV_OF_MODE eMode, 
    NV_OF_PERF_LEVEL preset) :
    m_ofMode(eMode),
    m_nOutGridSize(NV_OF_OUTPUT_VECTOR_GRID_SIZE_MAX),
    m_nHintGridSize(NV_OF_HINT_VECTOR_GRID_SIZE_UNDEFINED),
    m_ePreset(preset)
{
    m_bEnableRoi = NV_OF_FALSE;
    m_ElementSize[NV_OF_BUFFER_USAGE_INPUT] = 1;
    if (eInBufFmt == NV_OF_BUFFER_FORMAT_ABGR8)
        m_ElementSize[NV_OF_BUFFER_USAGE_INPUT] = 4;


    memset(&m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT], 0, sizeof(NV_OF_BUFFER_DESCRIPTOR));
    m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].width = nWidth;
    m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].height = nHeight;
    m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].bufferFormat = eInBufFmt;
    m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].bufferUsage = NV_OF_BUFFER_USAGE_INPUT;
}

bool NvOF::CheckGridSize(uint32_t nOutGridSize)
{
    uint32_t size;
    DoGetOutputGridSizes(nullptr, &size);

    std::unique_ptr<uint32_t[]> val(new uint32_t[size]);
    DoGetOutputGridSizes(val.get(), &size);

    for (uint32_t i = 0; i < size; i++)
    {
        if (nOutGridSize == val[i])
        {
            return true;
        }
    }
    return false;
}

bool NvOF::IsROISupported()
{
    uint32_t size;
    DoGetROISupport(nullptr, &size);

    std::unique_ptr<uint32_t[]> val(new uint32_t[size]);
    DoGetROISupport(val.get(), &size);
    return (val[0] == NV_OF_TRUE)? true : false;
}
bool NvOF::GetNextMinGridSize(uint32_t nOutGridSize, uint32_t& nextMinOutGridSize)
{
    uint32_t size;
    DoGetOutputGridSizes(nullptr, &size);

    std::unique_ptr<uint32_t[]> val(new uint32_t[size]);
    DoGetOutputGridSizes(val.get(), &size);

    nextMinOutGridSize = NV_OF_OUTPUT_VECTOR_GRID_SIZE_MAX;
    for (uint32_t i = 0; i < size; i++)
    {
        if (nOutGridSize == val[i])
        {
            nextMinOutGridSize = nOutGridSize;
            return true;
        }
        if (nOutGridSize < val[i] && val[i] < nextMinOutGridSize)
        {
            nextMinOutGridSize = val[i];
        }
    }
    return (nextMinOutGridSize >= NV_OF_OUTPUT_VECTOR_GRID_SIZE_MAX) ? false : true;
}

void NvOF::Init(uint32_t nOutGridSize, uint32_t nHintGridSize, bool bEnableHints, bool bEnableRoi)
{
    m_nOutGridSize = nOutGridSize;
    m_bEnableRoi = (NV_OF_BOOL)bEnableRoi;

    auto nOutWidth = (m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].width + m_nOutGridSize - 1) / m_nOutGridSize;
    auto nOutHeight = (m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].height + m_nOutGridSize - 1) / m_nOutGridSize;

    auto outBufFmt = NV_OF_BUFFER_FORMAT_SHORT2;
    if (m_ofMode == NV_OF_MODE_OPTICALFLOW)
    {
        outBufFmt = NV_OF_BUFFER_FORMAT_SHORT2;
        m_ElementSize[NV_OF_BUFFER_USAGE_OUTPUT] = sizeof(NV_OF_FLOW_VECTOR);
    }
    else if (m_ofMode == NV_OF_MODE_STEREODISPARITY)
    {
        outBufFmt = NV_OF_BUFFER_FORMAT_SHORT;
        m_ElementSize[NV_OF_BUFFER_USAGE_OUTPUT]  = sizeof(NV_OF_STEREO_DISPARITY);
    }
    else
    {
        NVOF_THROW_ERROR("Unsupported OF mode", NV_OF_ERR_INVALID_PARAM);
    }

    memset(&m_BufferDesc[NV_OF_BUFFER_USAGE_OUTPUT], 0, sizeof(NV_OF_BUFFER_DESCRIPTOR));
    m_BufferDesc[NV_OF_BUFFER_USAGE_OUTPUT].width = nOutWidth;
    m_BufferDesc[NV_OF_BUFFER_USAGE_OUTPUT].height = nOutHeight;
    m_BufferDesc[NV_OF_BUFFER_USAGE_OUTPUT].bufferFormat = outBufFmt;
    m_BufferDesc[NV_OF_BUFFER_USAGE_OUTPUT].bufferUsage = NV_OF_BUFFER_USAGE_OUTPUT;

    memset(&m_BufferDesc[NV_OF_BUFFER_USAGE_COST], 0, sizeof(NV_OF_BUFFER_DESCRIPTOR));
    m_BufferDesc[NV_OF_BUFFER_USAGE_COST].width = nOutWidth;
    m_BufferDesc[NV_OF_BUFFER_USAGE_COST].height = nOutHeight;
    m_BufferDesc[NV_OF_BUFFER_USAGE_COST].bufferFormat = NV_OF_BUFFER_FORMAT_UINT8;
    m_BufferDesc[NV_OF_BUFFER_USAGE_COST].bufferUsage = NV_OF_BUFFER_USAGE_COST;
    m_ElementSize[NV_OF_BUFFER_USAGE_COST] = sizeof(uint32_t);

    if (bEnableHints)
    {
        memset(&m_BufferDesc[NV_OF_BUFFER_USAGE_HINT], 0, sizeof(NV_OF_BUFFER_DESCRIPTOR));
        m_nHintGridSize = nHintGridSize;
        auto nHintWidth = (m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].width + m_nHintGridSize - 1) / m_nHintGridSize;
        auto nHintHeight = (m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].height + m_nHintGridSize - 1) / m_nHintGridSize;
        m_BufferDesc[NV_OF_BUFFER_USAGE_HINT].width = nHintWidth;
        m_BufferDesc[NV_OF_BUFFER_USAGE_HINT].height = nHintHeight;
        m_BufferDesc[NV_OF_BUFFER_USAGE_HINT].bufferFormat = outBufFmt;
        m_BufferDesc[NV_OF_BUFFER_USAGE_HINT].bufferUsage = NV_OF_BUFFER_USAGE_HINT;
        m_ElementSize[NV_OF_BUFFER_USAGE_HINT] = m_ElementSize[NV_OF_BUFFER_USAGE_OUTPUT];
    }

    memset(&m_initParams, 0, sizeof(m_initParams));
    m_initParams.inputBufferFormat = m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].bufferFormat;
    m_initParams.width = m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].width;
    m_initParams.height = m_BufferDesc[NV_OF_BUFFER_USAGE_INPUT].height;

    m_initParams.enableExternalHints = bEnableHints ? NV_OF_TRUE : NV_OF_FALSE;
    m_initParams.enableOutputCost = NV_OF_FALSE;
    m_initParams.hintGridSize = (NV_OF_HINT_VECTOR_GRID_SIZE) m_nHintGridSize;
    m_initParams.outGridSize = (NV_OF_OUTPUT_VECTOR_GRID_SIZE)m_nOutGridSize;
    m_initParams.mode = m_ofMode;
    m_initParams.perfLevel = m_ePreset;
    m_initParams.enableRoi = m_bEnableRoi;
    DoInit(m_initParams);
}

void NvOF::Execute(NvOFBuffer* image1,
    NvOFBuffer* image2,
    NvOFBuffer* outputBuffer,
    NvOFBuffer* hintBuffer,
    NvOFBuffer* costBuffer,
    uint32_t numROIs,
    NV_OF_ROI_RECT *ROIData,
    void*    arrInputFencePoint,
    uint32_t numInputFencePoint,
    void*    pOutputFencePoint,
    NV_OF_BOOL disableTemporalHints)
{
    NV_OF_EXECUTE_INPUT_PARAMS exeInParams;
    NV_OF_EXECUTE_OUTPUT_PARAMS exeOutParams;

    memset(&exeInParams, 0, sizeof(exeInParams));
    exeInParams.inputFrame = image1->getOFBufferHandle();
    exeInParams.referenceFrame = image2->getOFBufferHandle();
    exeInParams.disableTemporalHints = disableTemporalHints;
    exeInParams.externalHints = m_initParams.enableExternalHints == NV_OF_TRUE ? hintBuffer->getOFBufferHandle() : nullptr;
    exeInParams.numRois = numROIs;
    exeInParams.roiData = numROIs != 0 ? ROIData : NULL;

    memset(&exeOutParams, 0, sizeof(exeOutParams));
    exeOutParams.outputBuffer = outputBuffer->getOFBufferHandle();
    exeOutParams.outputCostBuffer = m_initParams.enableOutputCost == NV_OF_TRUE ? costBuffer->getOFBufferHandle() : nullptr;
    DoExecute(exeInParams, exeOutParams, arrInputFencePoint, numInputFencePoint, pOutputFencePoint);
}


std::vector<NvOFBufferObj>
NvOF::CreateBuffers(NV_OF_BUFFER_USAGE usage, uint32_t numBuffers, void* arrOutputFencePoint, uint32_t numOutputFencePoint)
{
    if (usage == NV_OF_BUFFER_USAGE_UNDEFINED || usage >= NV_OF_BUFFER_USAGE_MAX)
    {
        throw std::runtime_error("Invalid ::NV_OF_BUFFER_USAGE value ");
    }
    else
    {
        return DoAllocBuffers(m_BufferDesc[usage], m_ElementSize[usage], numBuffers, arrOutputFencePoint, numOutputFencePoint);
    }
}

std::vector<NvOFBufferObj>
NvOF::CreateBuffers(uint32_t nWidth, uint32_t nHeight, NV_OF_BUFFER_USAGE usage, uint32_t numBuffers, void* arrOutputFencePoint, uint32_t numOutputFencePoint)
{
    NV_OF_BUFFER_DESCRIPTOR bufferDesc { nWidth, nHeight, usage, m_BufferDesc[usage].bufferFormat };

    if (usage == NV_OF_BUFFER_USAGE_UNDEFINED || usage >= NV_OF_BUFFER_USAGE_MAX)
    {
        throw std::runtime_error("Invalid ::NV_OF_BUFFER_USAGE value ");
    }
    else
    {
        return DoAllocBuffers(bufferDesc, m_ElementSize[usage], numBuffers, arrOutputFencePoint, numOutputFencePoint);
    }
}

NvOFBufferObj NvOF::RegisterPreAllocBuffers(NV_OF_BUFFER_DESCRIPTOR ofBufDesc,
    const void* pResource,void* inputFencePoint, void* outputFencePoint)
{
    if (ofBufDesc.bufferUsage == NV_OF_BUFFER_USAGE_UNDEFINED || ofBufDesc.bufferUsage >= NV_OF_BUFFER_USAGE_MAX)
    {
        throw std::runtime_error("Invalid ::NV_OF_BUFFER_USAGE value ");
    }
    else
    {
        return DoRegisterBuffers(ofBufDesc, m_ElementSize[ofBufDesc.bufferUsage], pResource, inputFencePoint, outputFencePoint);
    }
}

void NvOFAPI::LoadNvOFAPI()
{
#if defined(_WIN32)
#if defined(_WIN64)
    HMODULE hModule = LoadLibrary(TEXT("nvofapi64.dll"));
#else
    HMODULE hModule = LoadLibrary(TEXT("nvofapi.dll"));
#endif
#else
    void *hModule = dlopen("libnvidia-opticalflow.so.1", RTLD_LAZY);
#endif
    if (hModule == NULL)
    {
        NVOF_THROW_ERROR("NVOF library file not found. Please ensure that the NVIDIA driver is installed", NV_OF_ERR_OF_NOT_AVAILABLE);
    }

    m_hModule = hModule;
}

NvOFAPI::~NvOFAPI()
{
    if (m_hModule)
    {
#if defined(_WIN32) || defined(_WIN64)
        FreeLibrary(m_hModule);
#else
        dlclose(m_hModule);
#endif
    }
}
