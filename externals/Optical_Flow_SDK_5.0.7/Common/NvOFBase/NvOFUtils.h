/*
 * Minimal NvOFUtils shim.
 *
 * The vendored Optical Flow SDK snapshot in this repo includes NvOFCuda.h,
 * which derives NvOFUtilsCuda from a base class declared in NvOFUtils.h, but
 * that header is missing from the snapshot. Real NVIDIA SDK packages ship it.
 *
 * For our AER V2 CUDA path we do NOT use NvOFUtilsCuda / Upsample helpers at
 * runtime — we only need NvOFCuda itself (Create/Init/Execute + buffer types).
 * So a minimal base declaration is sufficient to restore buildability without
 * altering behavior.
 */
#pragma once

#include <cstdint>

#include "NvOF.h"

class NvOFUtils {
public:
    explicit NvOFUtils(NV_OF_MODE eMode) : m_mode(eMode) {}
    virtual ~NvOFUtils() = default;

    virtual void Upsample(NvOFBuffer* srcBuffer, NvOFBuffer* dstBuffer, uint32_t nScaleFactor) = 0;

protected:
    NV_OF_MODE m_mode;
};
