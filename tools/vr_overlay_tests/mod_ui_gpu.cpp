#include "Overlay/ModUi.hpp"
#include "Overlay/ModUiProducer.hpp"
#include <windows.h>
using Microsoft::WRL::ComPtr;
void Check(bool,const char*);void Hr(HRESULT);
extern "C" const CvrModUiApi* CyberpunkVR_GetModUiApi(uint32_t,uint32_t);
namespace mock {extern unsigned acquisitions;}
void TestModUi(ID3D12Device* device,ID3D12CommandQueue* consumer){
    using namespace cvr::modui;
    Check(!CyberpunkVR_GetModUiApi(2,sizeof(CvrModUiApi)),"unknown ABI accepted");
    Check(!CyberpunkVR_GetModUiApi(1,0),"truncated ABI accepted");
    const auto* api=CyberpunkVR_GetModUiApi(1,sizeof(CvrModUiApi));Check(api,"ABI unavailable");
    const auto source=api->Register("GPU test");Check(source,"source registration failed");
    CvrModUiViewport viewport{sizeof(CvrModUiViewport)};Check(!api->Viewport(source,&viewport),"VR falsely available before session");
    cvr::vrui::Tracking t;t.valid=t.cyberfinger=t.preferLeft=true;t.origin=1;t.time=1000000000;t.head.position.y=1.7f;
    for(auto& h:t.hands){h.valid=true;h.aim.position={0,1.7f,-.2f};}
    Tracking(t,device,true,true);Check(api->Viewport(source,&viewport) && viewport.width==1440 && viewport.height==1080,"viewport follows SBS");
    CvrModUiTarget target{sizeof(CvrModUiTarget)};
    Check(!api->Begin(source,nullptr,DXGI_FORMAT_R8G8B8A8_UNORM,&target),"invalid device accepted");
    Check(!api->Begin(source,device,DXGI_FORMAT_D32_FLOAT,&target),"depth canvas accepted");
    Check(api->Begin(source,device,DXGI_FORMAT_R8G8B8A8_UNORM,&target),"canvas lease failed");
    const auto lease=target.lease;
    CvrModUiTarget duplicate{sizeof(CvrModUiTarget)};Check(!api->Begin(source,device,DXGI_FORMAT_R8G8B8A8_UNORM,&duplicate),"second outstanding lease accepted");
    ComPtr<ID3D12CommandQueue> producer;D3D12_COMMAND_QUEUE_DESC qd{};Hr(device->CreateCommandQueue(&qd,IID_PPV_ARGS(&producer)));
    ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;ComPtr<ID3D12Fence> fence;
    Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
    Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
    Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition={static_cast<ID3D12Resource*>(target.texture),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET};
    list->ResourceBarrier(1,&b);D3D12_CPU_DESCRIPTOR_HANDLE rtv{target.rtv};const float red[]{1,0,0,1};list->ClearRenderTargetView(rtv,red,0,nullptr);
    std::swap(b.Transition.StateBefore,b.Transition.StateAfter);list->ResourceBarrier(1,&b);Hr(list->Close());
    // Publish a not-yet-completed fence to exercise the consumer's refusal to
    // sample it. The test never places a wait on the game's/consumer queue.
    Check(api->Submit(source,lease,fence.Get(),1),"submit rejected");
    Check(!api->Submit(source,lease,fence.Get(),1),"stale lease accepted twice");
    api->State(source,CVR_MOD_UI_VISIBLE|CVR_MOD_UI_INTERACTIVE);Tracking(t,device,true,true);
    Check(!Visible() && !CapturesInput(),"unfinished first image captured gameplay");
    cvr::vrui::Panel panel;
    auto prepare=[&]{return panel.Prepare(reinterpret_cast<XrSession>(1),reinterpret_cast<XrSpace>(1),device,consumer,GetView(),Latest());};
    const auto acquisitions=mock::acquisitions;Check(!prepare() && mock::acquisitions==acquisitions,"unfinished producer was sampled");
    ID3D12CommandList* commands[]{list.Get()};producer->ExecuteCommandLists(1,commands);Hr(producer->Signal(fence.Get(),1));
    auto wait=[](ID3D12Fence* f,uint64_t value){HANDLE e=CreateEventW(nullptr,FALSE,FALSE,nullptr);Hr(f->SetEventOnCompletion(value,e));Check(WaitForSingleObject(e,5000)==WAIT_OBJECT_0,"GPU timeout");CloseHandle(e);};
    wait(fence.Get(),1);
    Tracking(t,device,true,true);Check(Visible() && CapturesInput(),"completed interactive source did not claim input");
    const auto* layer=reinterpret_cast<const XrCompositionLayerQuad*>(prepare());
    Check(layer && layer->eyeVisibility==XR_EYE_VISIBILITY_BOTH,"external source not presented to both eyes");
    Hr(consumer->Signal(fence.Get(),2));wait(fence.Get(),2);
    // Frame copied on a different queue; holding a view keeps its source lease.
    auto retained=Latest();Check(retained && retained->texture.Get()==target.texture,"source changed during copy");
    CvrModUiInput events[64];api->Poll(source,events,64);t.time+=11111111;t.hands[0].trigger=1;Tracking(t,device,true,true);
    auto count=api->Poll(source,events,64);Check(count && (events[count-1].buttons&CVR_MOD_UI_PRIMARY),"pointer click not delivered");
    api->State(source,0);Tracking(t,device,true,true);Check(CapturesInput(),"held click leaked into gameplay on close");
    count=api->Poll(source,events,64);Check(count && !events[0].buttons && !(events[0].flags&CVR_MOD_UI_ACTIVE),"closed source retained click");
    t.hands[0].trigger=0;Tracking(t,device,true,true);Check(!CapturesInput(),"release gate remained stuck");
    Check(!prepare(),"closed source still presented");panel.Shutdown(false);retained.reset();
    const auto blocked=api->Register("busy producer");
    for(int i=0;i<3;++i){CvrModUiTarget v{sizeof(CvrModUiTarget)};Check(api->Begin(blocked,device,DXGI_FORMAT_R8G8B8A8_UNORM,&v),"ring too small");Check(api->Submit(blocked,v.lease,fence.Get(),100),"ring submit failed");}
    CvrModUiTarget full{sizeof(CvrModUiTarget)};Check(!api->Begin(blocked,device,DXGI_FORMAT_R8G8B8A8_UNORM,&full),"unfinished canvas reused");
    api->Unregister(blocked);Hr(fence->Signal(100));
    api->State(source,CVR_MOD_UI_VISIBLE); // no fresh image after hiding
    Tracking(t,device,true,true);Check(!CapturesInput(),"non-interactive source captured gameplay");
    api->Unregister(source);Check(!api->Viewport(source,&viewport),"retired handle usable");
    // Exercise the reusable producer implementation with a separate queue.
    const auto generic=api->Register("generic native renderer");
    ComPtr<ID3D12DescriptorHeap> heap;D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;hd.NumDescriptors=1;hd.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&heap)));
    client::Producer renderer;
    api->State(generic,CVR_MOD_UI_VISIBLE|CVR_MOD_UI_INTERACTIVE);
    Check(renderer.Render(api,generic,device,producer.Get(),DXGI_FORMAT_R8G8B8A8_UNORM,heap.Get(),[](ID3D12GraphicsCommandList*){}),"generic renderer fell back to SBS");
    Check(renderer.Drain(),"generic renderer did not drain");Tracking(t,device,true,true);Check(Visible(),"generic frame not published");
    const auto activeCanvas=Latest();
    const auto toast=api->Register("passive notification");api->State(toast,CVR_MOD_UI_VISIBLE);
    renderer.Render(api,toast,device,producer.Get(),DXGI_FORMAT_R8G8B8A8_UNORM,heap.Get(),[](ID3D12GraphicsCommandList*){});
    Check(renderer.Drain(),"notification renderer did not drain");Tracking(t,device,true,true);
    Check(Latest()==activeCanvas && CapturesInput(),"passive notification evicted interactive panel");
    api->Unregister(toast);
    Sleep(1050);Tracking(t,device,true,true);Check(!Visible() && !CapturesInput(),"stalled producer retained focus");
    api->Unregister(generic);
    Suspend();Check(!Visible() && !CapturesInput(),"session shutdown retained overlay");
}
