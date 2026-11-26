#include "Framegen/StatsOverlay.hpp"
#include "Framegen/Framegen.hpp"
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <cmath>
#include <string>

#if !CVR_RUST_VRIK   // unused by the Rust build
extern "C" void cvr_framegen_overlay_draw(uint32_t* pixels,int width,int height,const cvr::framegen::Settings* settings,
                                          const cvr::framegen::Statistics* stats,uint32_t hour,uint32_t minute,int backend);
#endif
namespace cvr::framegen {
#if CVR_RUST_VRIK
}  // namespace cvr::framegen
extern "C" {
void* cvr_xr_framegen_overlay_new();
void cvr_xr_framegen_overlay_free(void* overlay);
}
namespace cvr::framegen {
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // only the C++ manager built one
StatsOverlay::StatsOverlay() : rust_(cvr_xr_framegen_overlay_new()) {}
StatsOverlay::~StatsOverlay() { cvr_xr_framegen_overlay_free(rust_); }
#endif
#else
const XrCompositionLayerBaseHeader* StatsOverlay::Prepare(XrSession session,XrSpace view,ID3D12Device* device,ID3D12CommandQueue* queue,
    XrSpace local,const XrPosef* head,uint64_t origin,XrTime displayTime) {
    const auto settings=GetSettings();
    if(!settings.overlay) {
        if(!fence || fence->GetCompletedValue()>=fenceValue)Shutdown();
        return nullptr;
    }
    if(!device || !queue || !session || !view)return nullptr;
    ConfigureHardwareTelemetry(device);
    const uint32_t wantedWidth=settings.overlayLayout==0 ? 720u : (settings.overlayLayout==1 ? 420u : 360u);
    const uint32_t wantedHeight=settings.overlayLayout==0 ? 452u : (settings.overlayLayout==1 ? 404u : 222u);
    if(swapchain && (width!=wantedWidth || height!=wantedHeight) && fence->GetCompletedValue()>=fenceValue)Shutdown();
    if(!swapchain) {
        width=wantedWidth;height=wantedHeight;rowPitch=(width*4+255)&~255u;pixels.resize(size_t(width)*height);
        uint32_t count{};if(XR_FAILED(xrEnumerateSwapchainFormats(session,0,&count,nullptr)))return nullptr;
        std::vector<int64_t> formats(count);if(XR_FAILED(xrEnumerateSwapchainFormats(session,count,&count,formats.data())))return nullptr;
        if(std::find(formats.begin(),formats.end(),DXGI_FORMAT_R8G8B8A8_UNORM_SRGB)==formats.end())return nullptr;
        XrSwapchainCreateInfo info{XR_TYPE_SWAPCHAIN_CREATE_INFO};info.usageFlags=XR_SWAPCHAIN_USAGE_TRANSFER_DST_BIT|XR_SWAPCHAIN_USAGE_SAMPLED_BIT|XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT;
        info.format=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;info.sampleCount=1;info.width=width;info.height=height;info.faceCount=1;info.arraySize=1;info.mipCount=1;
        if(XR_FAILED(xrCreateSwapchain(session,&info,&swapchain)))return nullptr;
        if(XR_FAILED(xrEnumerateSwapchainImages(swapchain,0,&count,nullptr))) {Shutdown();return nullptr;}
        images.assign(count,{XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
        if(XR_FAILED(xrEnumerateSwapchainImages(swapchain,count,&count,reinterpret_cast<XrSwapchainImageBaseHeader*>(images.data()))) ||
           FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list))) ||
           FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))) {Shutdown();return nullptr;}
        list->Close();D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=rowPitch*height;
        desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_UPLOAD;
        if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&upload)))) {Shutdown();return nullptr;}
    }
    const auto now=NowMs();
    if((now>=nextUpdate || acquired) && fence->GetCompletedValue()>=fenceValue) {
        if(!acquired) {XrSwapchainImageAcquireInfo info{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};if(XR_SUCCEEDED(xrAcquireSwapchainImage(swapchain,&info,&acquiredIndex)))acquired=true;}
        if(acquired) {
            XrSwapchainImageWaitInfo wait{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};wait.timeout=0;
            if(xrWaitSwapchainImage(swapchain,&wait)==XR_SUCCESS) {
                const auto stats=GetStatistics();void* data{};D3D12_RANGE none{};
                if(SUCCEEDED(upload->Map(0,&none,&data))) {
                    SYSTEMTIME time{};GetLocalTime(&time);
                    cvr_framegen_overlay_draw(pixels.data(),int(width),int(height),&settings,&stats,time.wHour,time.wMinute,
                                              int(ActiveBackend()));
                    for(uint32_t row=0;row<height;++row)std::memcpy(static_cast<uint8_t*>(data)+size_t(row)*rowPitch,pixels.data()+size_t(row)*width,size_t(width)*4);
                    upload->Unmap(0,nullptr);
                    if(SUCCEEDED(allocator->Reset()) && SUCCEEDED(list->Reset(allocator.Get(),nullptr))) {
                        auto* target=images[acquiredIndex].texture;
                        D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                        b.Transition={target,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_DEST};list->ResourceBarrier(1,&b);
                        D3D12_TEXTURE_COPY_LOCATION src{},dst{};src.pResource=upload.Get();src.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
                        src.PlacedFootprint.Footprint={target->GetDesc().Format,width,height,1,rowPitch};dst.pResource=target;dst.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                        list->CopyTextureRegion(&dst,0,0,0,&src,nullptr);std::swap(b.Transition.StateBefore,b.Transition.StateAfter);list->ResourceBarrier(1,&b);
                        if(SUCCEEDED(list->Close())) {ID3D12CommandList* lists[]={list.Get()};queue->ExecuteCommandLists(1,lists);queue->Signal(fence.Get(),++fenceValue);published=true;nextUpdate=now+250;}
                    }
                }
                XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&release)))published=false;
                acquired=false;
            }
        }
    }
    if(!published)return nullptr;
    layer.space=view;layer.eyeVisibility=XR_EYE_VISIBILITY_BOTH;layer.layerFlags=XR_COMPOSITION_LAYER_BLEND_TEXTURE_SOURCE_ALPHA_BIT;
    layer.subImage={swapchain,{{0,0},{int32_t(width),int32_t(height)}},0};
    constexpr float positions[9][2]={{-18,18},{18,18},{-18,-18},{18,-18},{0,18},{0,-18},{-18,0},{18,0},{0,0}};
    constexpr float radians=.01745329252f;
    const float distance=settings.overlayDistance;
    const auto position=positions[std::clamp(settings.overlayCorner,0,8)];
    const float x=std::tan(std::clamp(position[0]+settings.overlayX,-70.0f,70.0f)*radians)*distance;
    const float y=std::tan(std::clamp(position[1]+settings.overlayY,-60.0f,60.0f)*radians)*distance;
    layer.pose={{0,0,0,1},{x,y,-distance}};
    if(originSerial!=origin || followMode!=settings.overlayFollow) {follow.Reset();clock.Reset();originSerial=origin;}
    if(settings.overlayFollow && local && head) {
        const auto& q=head->orientation;
        const float yaw=follow.Update(cvr::hud::HeadYaw(q.x,q.y,q.z,q.w,follow.yaw),settings.overlayCone*radians,clock.Step(displayTime));
        anchor.orientation={0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)};
        anchor.position={head->position.x+std::cos(yaw)*x-std::sin(yaw)*distance,head->position.y+y,
            head->position.z-std::sin(yaw)*x-std::cos(yaw)*distance};
        layer.pose=anchor;layer.space=local;
    } else if(settings.overlayFollow && local) {
        if(!follow.valid)return nullptr;layer.pose=anchor;layer.space=local;
    }
    followMode=settings.overlayFollow;
    const float angle=settings.overlayLayout==0 ? 36.0f : (settings.overlayLayout==1 ? 25.0f : 22.0f);
    const float w=2*distance*std::tan(angle*radians*.5f)*settings.overlayScale;
    layer.size={w,w*height/width};return reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layer);
}
void StatsOverlay::Shutdown(bool wait) {
    if(!swapchain && !fence && pixels.empty())return;
    if(wait && fence && fence->GetCompletedValue()<fenceValue) {
        const auto event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
        if(event) {if(SUCCEEDED(fence->SetEventOnCompletion(fenceValue,event)))WaitForSingleObject(event,1000);CloseHandle(event);}
    }
    if(fence && fence->GetCompletedValue()<fenceValue)return;
    if(acquired && swapchain) {
        XrSwapchainImageWaitInfo ready{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};ready.timeout=0;
        if(xrWaitSwapchainImage(swapchain,&ready)!=XR_SUCCESS)return;
        XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
        if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&release)))return;
        acquired=false;
    }
    if(swapchain) {xrDestroySwapchain(swapchain);swapchain=XR_NULL_HANDLE;}
    std::vector<XrSwapchainImageD3D12KHR>().swap(images);upload.Reset();allocator.Reset();list.Reset();fence.Reset();
    fenceValue=0;nextUpdate=0;acquired=published=false;
    std::vector<uint32_t>().swap(pixels);width=height=rowPitch=0;follow.Reset();clock.Reset();followMode=-1;
}
#endif
}
