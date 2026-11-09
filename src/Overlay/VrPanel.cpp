#include "Runtimes/PresentationQueue.hpp"
#include "Overlay/VrPanel.hpp"
#include "Camera/ImagePoseIdentity.hpp"
#include <array>
#include <mutex>

namespace cvr::vrui {
namespace {
std::mutex canvasMutex;
std::array<std::shared_ptr<Canvas>,3> pool;
std::shared_ptr<Canvas> latest;
uint64_t serial=0;
}
std::shared_ptr<Canvas> AcquireCanvas(ID3D12Device* device,DXGI_FORMAT format){
    std::lock_guard lock(canvasMutex);
    for(auto& slot:pool){
        if(slot && (slot.use_count()!=1 || (slot->fence && slot->fence->GetCompletedValue()<slot->value)))continue;
        if(slot && slot->format!=format)slot.reset();
        if(!slot){
            auto c=std::make_shared<Canvas>();D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
            D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=CanvasWidth;desc.Height=CanvasHeight;
            desc.DepthOrArraySize=1;desc.MipLevels=1;desc.Format=format==DXGI_FORMAT_R8G8B8A8_UNORM?DXGI_FORMAT_R8G8B8A8_TYPELESS:format;desc.SampleDesc.Count=1;desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
            D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;hd.NumDescriptors=1;
            if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,nullptr,IID_PPV_ARGS(&c->texture))) ||
               FAILED(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&c->rtv))))return {};
            D3D12_RENDER_TARGET_VIEW_DESC rd{};rd.Format=format;rd.ViewDimension=D3D12_RTV_DIMENSION_TEXTURE2D;
            device->CreateRenderTargetView(c->texture.Get(),&rd,c->rtv->GetCPUDescriptorHandleForHeapStart());c->format=format;slot=c;
        }
        return slot;
    }
    return {};
}
void PublishCanvas(const std::shared_ptr<Canvas>& c,ID3D12Fence* f,uint64_t value){std::lock_guard lock(canvasMutex);c->fence=f;c->value=value;c->serial=++serial;latest=c;}
void ClearCanvas(){std::lock_guard lock(canvasMutex);latest.reset();for(auto& p:pool)if(p && p.use_count()==1 && (!p->fence || p->fence->GetCompletedValue()>=p->value))p.reset();}
const XrCompositionLayerBaseHeader* Panel::Prepare(XrSession session,XrSpace space,ID3D12Device* device,ID3D12CommandQueue* queue){
    if(auto* runtime=cvr::xrqueue::Queue())queue=runtime;
    rayVisible=false;
    if(!Visible()){Shutdown();return nullptr;}
    const auto view=GetView();if(!view.valid || !device || !queue)return nullptr;
    std::shared_ptr<Canvas> source;{std::lock_guard lock(canvasMutex);source=latest;}
    if(!source)return nullptr;
    // Wait on the exact canvas receipt on the runtime queue; retain it until copy completion.
    if(!swapchain){
        uint32_t count=0;if(XR_FAILED(xrEnumerateSwapchainFormats(session,0,&count,nullptr)))return nullptr;
        std::vector<int64_t> formats(count);if(XR_FAILED(xrEnumerateSwapchainFormats(session,count,&count,formats.data())))return nullptr;
        const int64_t format=DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
        if(std::find(formats.begin(),formats.end(),format)==formats.end())return nullptr;
        XrSwapchainCreateInfo ci{XR_TYPE_SWAPCHAIN_CREATE_INFO};ci.usageFlags=XR_SWAPCHAIN_USAGE_COLOR_ATTACHMENT_BIT|XR_SWAPCHAIN_USAGE_SAMPLED_BIT;
        ci.format=format;ci.sampleCount=1;ci.width=CanvasWidth;ci.height=CanvasHeight;ci.faceCount=1;ci.arraySize=1;ci.mipCount=1;
        if(XR_FAILED(xrCreateSwapchain(session,&ci,&swapchain)))return nullptr;
        if(XR_FAILED(xrEnumerateSwapchainImages(swapchain,0,&count,nullptr)) || !count){Shutdown();return nullptr;}
        images.assign(count,{XR_TYPE_SWAPCHAIN_IMAGE_D3D12_KHR});
        if(XR_FAILED(xrEnumerateSwapchainImages(swapchain,count,&count,reinterpret_cast<XrSwapchainImageBaseHeader*>(images.data()))) ||
           FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
           FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list))) ||
           FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)))){Shutdown();return nullptr;}
        list->Close();
    }
    if(fence->GetCompletedValue()>=fenceValue){
        flight.reset();
        if(lastSerial!=source->serial || acquired){
            if(!acquired){XrSwapchainImageAcquireInfo a{XR_TYPE_SWAPCHAIN_IMAGE_ACQUIRE_INFO};if(XR_SUCCEEDED(xrAcquireSwapchainImage(swapchain,&a,&index)))acquired=true;}
            if(acquired){XrSwapchainImageWaitInfo w{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};w.timeout=0;
                if(xrWaitSwapchainImage(swapchain,&w)==XR_SUCCESS){
                    if(SUCCEEDED(allocator->Reset()) && SUCCEEDED(list->Reset(allocator.Get(),nullptr)) &&
                       blit.EnsureInitialized(device,DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,CanvasWidth,CanvasHeight) &&
                       blit.RecordOverlay(list.Get(),source->texture.Get(),images[index].texture,4) && SUCCEEDED(list->Close()) &&
                       (!cvr::xrqueue::Private() || cvr::xrqueue::WaitPublished(source->fence.Get(),source->value))){
                        std::lock_guard submit(cvr::camera::ImageSubmissionMutex());
                        ID3D12CommandList* commands[]={list.Get()};queue->ExecuteCommandLists(1,commands);
                        flight=source;
                        if(SUCCEEDED(queue->Signal(fence.Get(),++fenceValue))){lastSerial=source->serial;published=true;}
                    }
                    XrSwapchainImageReleaseInfo release{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};
                    if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&release)))published=false;acquired=false;
                }
            }
        }
    }
    if(!published)return nullptr;
    layer.space=space;layer.eyeVisibility=XR_EYE_VISIBILITY_BOTH;layer.layerFlags=XR_COMPOSITION_LAYER_BLEND_TEXTURE_SOURCE_ALPHA_BIT;
    layer.subImage={swapchain,{{0,0},{CanvasWidth,CanvasHeight}},0};layer.pose=view.pose;layer.size={view.width,view.height};
    if(view.ray){
        const auto delta=Sub(view.rayEnd,view.rayStart);const float length=std::sqrt(Dot(delta,delta));
        if(length>.02f){
            // Rotate local +Y onto the controller-to-panel segment.
            const auto d=Mul(delta,1/length);XrQuaternionf q{d.z,0,-d.x,1+d.y};
            const float n=std::sqrt(q.x*q.x+q.z*q.z+q.w*q.w);
            if(n>.001f){q.x/=n;q.z/=n;q.w/=n;
                const auto normal=RotateVector(q,{0,0,1});
                auto facing=Sub(view.headPosition,Mul(Add(view.rayStart,view.rayEnd),.5f));
                facing=Sub(facing,Mul(d,Dot(facing,d)));const float f=std::sqrt(Dot(facing,facing));
                if(f>.001f){facing=Mul(facing,1/f);const XrVector3f cross{normal.y*facing.z-normal.z*facing.y,normal.z*facing.x-normal.x*facing.z,normal.x*facing.y-normal.y*facing.x};
                    const float angle=std::atan2(Dot(d,cross),Dot(normal,facing)),s=std::sin(angle*.5f);
                    q=MultiplyQuat({d.x*s,d.y*s,d.z*s,std::cos(angle*.5f)},q);}
                ray=layer;ray.subImage.imageRect={{0,0},{3,3}};
                ray.pose={q,Mul(Add(view.rayStart,view.rayEnd),.5f)};ray.size={.003f,length};rayVisible=true;}
        }
    }
    return reinterpret_cast<const XrCompositionLayerBaseHeader*>(&layer);
}
const XrCompositionLayerBaseHeader* Panel::Ray() const{return rayVisible?reinterpret_cast<const XrCompositionLayerBaseHeader*>(&ray):nullptr;}
void Panel::Shutdown(){
    if(fence && fence->GetCompletedValue()<fenceValue)return;
    if(acquired && swapchain){XrSwapchainImageWaitInfo w{XR_TYPE_SWAPCHAIN_IMAGE_WAIT_INFO};w.timeout=0;if(xrWaitSwapchainImage(swapchain,&w)!=XR_SUCCESS)return;
        XrSwapchainImageReleaseInfo r{XR_TYPE_SWAPCHAIN_IMAGE_RELEASE_INFO};if(XR_FAILED(xrReleaseSwapchainImage(swapchain,&r)))return;acquired=false;}
    if(swapchain)xrDestroySwapchain(swapchain);swapchain={};images.clear();flight.reset();allocator.Reset();list.Reset();fence.Reset();
    fenceValue=lastSerial=0;published=rayVisible=false;ClearCanvas();
}
}
