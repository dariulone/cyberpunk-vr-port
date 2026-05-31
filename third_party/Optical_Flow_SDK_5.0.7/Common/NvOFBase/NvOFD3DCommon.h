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


#pragma once

#include <wrl.h>
#include <dxgiformat.h>
#include "NvOF.h"
#include "nvOpticalFlowCommon.h"

class DXException : public std::exception
{
public:
    DXException(HRESULT hr) : result(hr) {}

    virtual const char* what() const override
    {
        static char s_str[64] = {};
        sprintf_s(s_str, "Failure with HRESULT of %08X", static_cast<unsigned int>(result));
        return s_str;
    }

private:
    HRESULT result;
};

#define D3D_API_CALL(dxAPI)                           \
    do                                                \
    {                                                 \
        HRESULT hr = dxAPI;                           \
        if (FAILED(hr))                               \
        {                                             \
            throw DXException(hr);                    \
        }                                             \
    } while (0)

DXGI_FORMAT NvOFBufferFormatToDxgiFormat(NV_OF_BUFFER_FORMAT  ofBufFormat);

NV_OF_BUFFER_FORMAT DXGIFormatToNvOFBufferFormat(DXGI_FORMAT dxgiFormat);

uint32_t GetNumberOfPlanes(DXGI_FORMAT dxgiFormat);
