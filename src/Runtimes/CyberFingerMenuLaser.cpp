#include "Runtimes/CyberFingerMenuLaser.hpp"
#include "Runtimes/CyberFingerGameMenu.hpp"
#include "Camera/ImagePoseIdentity.hpp"
#include <algorithm>

namespace cvr::cfmenu {
const XrCompositionLayerBaseHeader* Laser::Prepare(XrSession session,XrSpace space,ID3D12Device* device,ID3D12CommandQueue* queue) {
    const auto state=Current();
    if(!state.active || !state.hit || !device || !queue)return nullptr;
    if(!swapchain) {
        uint32_t count{};
        if(XR_FAILED(xrEnumerateSwapchainFormats(session,0,&count,nullptr)))return nullptr;
        std::vector<int64_t> formats(count);
        if(XR_FAILED(xrEnumerateSwapchainFormats(session,count,&count,formats.data())) ||
            std::find(formats.begin(),formats.end(),DXGI_FORMAT_R8G8B8A8_UNORM)==formats.end())return nullptr;
        XrSwapchainCreateInfo ci{XR_TYPE_SWAPCHAIN_CREATE_INFO};
        ci.createFlags=XR_SWAPCHAIN_CREATE_STATIC_IMAGE_BIT;
        ci.usageFlags=XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT|XR_SWAPCHAIN_USAGE_SAMPLED_BIT;
        ci.format=DXGI_FORMAT_R8G8B8A8_UNORM;ci.width=ci.height=4;ci.sampleCount=ci.faceCount=ci.arraySize=ci.mipCount=1;
        if(XR_FAILED(xrCreateSwapchain(session,&ci,&swapchain)))return nullptr;
        if(XR_FAILED(xrEnumerateSwapchainImages(swapchain,0,&count,nullptr)) || !count){Shutdown();return nullptr;}
        images.assign(count,{XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
        D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;hd.NumDescriptors=1;
        if(XR_FAILED(xrEnumerateSwapchainImages(swapchain,count,&count,reinterpret_cast<XrSwapchainImageBaseHeader*>(images.data()))) ||
            FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
            FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list))) ||
            FAILED(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&rtv))) ||
            FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))){Shutdown();return nullptr;}
        list->Close();
    }
    if(!published) {
        if(fence->GetCompletedValue()<fenceValue)return nullptr;
        if(!acquired){XrSwapchainImageAcquireInfo a{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};
            if(XR_FAILED(xrAcquireSwapchainImage(swapchain,&a,&index)))return nullptr;acquired=true;}
        XrSwapchainImageWaitInfo w{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};w.timeout=0;
        if(xrWaitSwapchainImage(swapchain,&w)!=XR_SUCCESS)return nullptr;
        if(FAILED(allocator->Reset()) || FAILED(list->Reset(allocator.Get(),nullptr)))return nullptr;
        D3D12_RENDER_TARGET_VIEW_DESC rd{};rd.Format=DXGI_FORMAT_R8G8B8A8_UNORM;rd.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2D;
        device->CreateRenderTargetView(images[index].texture,&rd,rtv->GetCPUDescriptorHandleForHeapStart());
        const float color[]={.1f,.82f,1.f,1.f};
        list->ClearRenderTargetView(rtv->GetCPUDescriptorHandleForHeapStart(),color,0,nullptr);
        if(FAILED(list->Close()))return nullptr;
        std::lock_guard submit(cvr::camera::ImageSubmissionMutex());
        ID3D12CommandList* commands[]={list.Get()};queue->ExecuteCommandLists(1,commands);
        if(FAILED(queue->Signal(fence.Get(),++fenceValue)))return nullptr;
        XrSwapchainImageReleaseInfo r{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
        if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&r)))return nullptr;
        acquired=false;published=true;
    }
    using namespace cvr::vrui;
    const auto delta=Sub(state.end,state.start);const float length=std::sqrt(Dot(delta,delta));
    if(length<.02f)return nullptr;
    const auto d=Mul(delta,1/length);XrQuaternionf q{d.z,0,-d.x,1+d.y};
    const float n=std::sqrt(q.x*q.x+q.z*q.z+q.w*q.w);
    if(n<.001f)return nullptr;
    q.x/=n;q.z/=n;q.w/=n;
    const auto normal=RotateVector(q,{0,0,1});
    const auto centre=Mul(Add(state.start,state.end),.5f);
    auto facing=Sub(state.head,centre);facing=Sub(facing,Mul(d,Dot(facing,d)));
    const float f=std::sqrt(Dot(facing,facing));
    if(f>.001f){facing=Mul(facing,1/f);
        const XrVector3f cross{normal.y*facing.z-normal.z*facing.y,normal.z*facing.x-normal.x*facing.z,normal.x*facing.y-normal.y*facing.x};
        const float angle=std::atan2(Dot(d,cross),Dot(normal,facing)),s=std::sin(angle*.5f);
        q=MultiplyQuat({d.x*s,d.y*s,d.z*s,std::cos(angle*.5f)},q);}
    layer.space=space;layer.eyeVisibility=XR_EYE_VISIBILITY_BOTH;
    layer.layerFlags=XR_COMPOSITION_LAYER_BLEND_TEXTURE_SOURCE_ALPHA_BIT;
    layer.subImage={swapchain,{{0,0},{4,4}},0};layer.pose={q,centre};layer.size={.003f,length};
    return reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layer);
}
void Laser::Shutdown() {
    if(fence && fence->GetCompletedValue()<fenceValue)return;
    if(acquired && swapchain){XrSwapchainImageWaitInfo w{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};w.timeout=0;
        if(xrWaitSwapchainImage(swapchain,&w)!=XR_SUCCESS)return;
        XrSwapchainImageReleaseInfo r{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
        if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&r)))return;acquired=false;}
    if(swapchain)xrDestroySwapchain(swapchain);
    swapchain={};images.clear();allocator.Reset();list.Reset();rtv.Reset();fence.Reset();fenceValue=0;published=false;
}
}
