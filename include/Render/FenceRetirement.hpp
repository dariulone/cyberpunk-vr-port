#pragma once
#include <d3d12.h>
#include <cstdint>

namespace cvr::gpu {
enum class RetirementStatus { Ready, Pending, MissingFence, DeviceLost };
// Poll only: an elapsed CPU timeout never proves that submitted GPU work is done.
inline RetirementStatus FenceRetirement(ID3D12Fence* fence,uint64_t submitted) {
    if(!submitted)return RetirementStatus::Ready;
    if(!fence)return RetirementStatus::MissingFence;
    const auto completed=fence->GetCompletedValue();
    if(completed==UINT64_MAX)return RetirementStatus::DeviceLost;
    return completed>=submitted ? RetirementStatus::Ready : RetirementStatus::Pending;
}
}
