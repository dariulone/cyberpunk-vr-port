#pragma once
#include <openxr/openxr.h>

namespace cvr::depth {
// A timeout does not cancel acquisition. Keep the index and retry its wait;
// callers may release an image only when this returns XR_SUCCESS.
inline XrResult WaitImage(XrSwapchain handle,bool& pending,uint32_t& index,XrDuration timeout=0) {
    if(!pending) {
        XrSwapchainImageAcquireInfo acquire{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
        const auto result=xrAcquireSwapchainImage(handle,&acquire,&index);
        if(result!=XR_SUCCESS)return result;
        pending=true;
    }
    XrSwapchainImageWaitInfo wait{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};wait.timeout=timeout;
    const auto result=xrWaitSwapchainImage(handle,&wait);
    if(result==XR_SUCCESS)pending=false;
    return result;
}
}
