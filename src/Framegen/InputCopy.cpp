#include "Framegen/InputCopy.hpp"
#include <atomic>

extern void Log(const char*,...);

namespace cvr::framegen {
namespace {
using Microsoft::WRL::ComPtr;
uint32_t MotionPixelBytes(DXGI_FORMAT format) {
    switch(format) {
        case DXGI_FORMAT_R16G16_FLOAT:return 4;
        case DXGI_FORMAT_R32G32_FLOAT:
        // RT's RGBA16F texture still contains 2D motion in XY.
        // FidelityFX reads .xy, so preserve the native format and channels.
        case DXGI_FORMAT_R16G16B16A16_FLOAT:return 8;
        default:return 0;
    }
}
void Barrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b,UINT subresource=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES) {
    if(a==b)return;D3D12_RESOURCE_BARRIER v{};v.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition={resource,subresource,a,b};list->ResourceBarrier(1,&v);
}
}
bool CopyInput(ID3D12GraphicsCommandList* list,const InputTag& tag,Inputs& out) {
    if(!list || !tag.resource || tag.type>1)return false;
    const auto desc=tag.resource->GetDesc();
    if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count!=1 || desc.DepthOrArraySize!=1)return false;
    const uint32_t width=tag.width ? tag.width : uint32_t(desc.Width),height=tag.height ? tag.height : desc.Height;
    if(!width || !height || width>16384 || height>16384 || uint64_t(tag.x)+width>desc.Width || uint64_t(tag.y)+height>desc.Height)return false;
    DXGI_FORMAT format=desc.Format;
    if(tag.type==0) {
        switch(format) {
            case DXGI_FORMAT_R32_TYPELESS:case DXGI_FORMAT_D32_FLOAT:case DXGI_FORMAT_R32_FLOAT:
            case DXGI_FORMAT_R32G8X24_TYPELESS:case DXGI_FORMAT_D32_FLOAT_S8X24_UINT:
            case DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS:format=DXGI_FORMAT_R32_FLOAT;break;
            default:return false;
        }
    } else if(!MotionPixelBytes(format))return false;
    auto& target=tag.type==0 ? out.depth : out.motion;
    if(target) {
        ComPtr<ID3D12Device> sourceDevice,targetDevice;
        if(FAILED(list->GetDevice(IID_PPV_ARGS(&sourceDevice))) || FAILED(target->GetDevice(IID_PPV_ARGS(&targetDevice))) ||
            sourceDevice.Get()!=targetDevice.Get())return false;
    }
    const bool recreate=!target || target->GetDesc().Width!=width || target->GetDesc().Height!=height || target->GetDesc().Format!=format;
    if(recreate) {
        ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return false;
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=width;d.Height=height;
        d.DepthOrArraySize=1;d.MipLevels=1;d.Format=format;d.SampleDesc.Count=1;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        ComPtr<ID3D12Resource> created;
        if(const HRESULT hr=device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&created));FAILED(hr)) {
            // Without its copy the eye's inputs never complete and generation waits at zero.
            static std::atomic<unsigned> reported{};
            if(reported.fetch_add(1)<16)Log("[framegen] input %s copy %ux%u could not be created: 0x%08X\n",tag.type==0?"depth":"motion",width,height,unsigned(hr));
            return false;
        }
        target=created;target->SetName(tag.type==0 ? L"CVR_Framegen_Depth" : L"CVR_Framegen_Motion");
        if(!out.readyFence && FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&out.readyFence))))return false;
    } else Barrier(list,target.Get(),D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
    const auto state=D3D12_RESOURCE_STATES(tag.state);
    Barrier(list,tag.resource,state,D3D12_RESOURCE_STATE_COPY_SOURCE,0);
    D3D12_TEXTURE_COPY_LOCATION src{},dst{};src.pResource=tag.resource;dst.pResource=target.Get();
    src.Type=dst.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_BOX box{tag.x,tag.y,0,tag.x+width,tag.y+height,1};
    list->CopyTextureRegion(&dst,0,0,0,&src,&box);
    Barrier(list,tag.resource,D3D12_RESOURCE_STATE_COPY_SOURCE,state,0);
    Barrier(list,target.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    if(tag.type==0) {out.width=width;out.height=height;out.hasDepth=true;}
    else {out.motionWidth=width;out.motionHeight=height;out.hasMotion=true;}
    out.bytes=uint64_t(out.width)*out.height*4+uint64_t(out.motionWidth)*out.motionHeight*
        (out.motion ? MotionPixelBytes(out.motion->GetDesc().Format) : 0);
    return true;
}
}
