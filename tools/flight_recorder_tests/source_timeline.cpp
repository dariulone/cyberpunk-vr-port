#include "Framegen/SourceTimeline.hpp"
#include "Framegen/FlightRecorder.hpp"
#include "Render/CommandResources.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <iostream>
#include <cstdlib>
#include <vector>
using namespace cvr::framegen;
using Microsoft::WRL::ComPtr;
namespace cvr::framegen::refresh {bool Targeted(){return true;}}
namespace cvr::framegen::flight {void Count(Event){}void Source(const SourceRecord&){} }
void check(bool v,const char* m){if(!v){std::cerr<<m<<'\n';std::exit(1);}}
void hr(HRESULT h){check(SUCCEEDED(h),"D3D12 failure");}
double now(){LARGE_INTEGER n{},f{};QueryPerformanceCounter(&n);QueryPerformanceFrequency(&f);return n.QuadPart*1000./f.QuadPart;}
void wait(ID3D12CommandQueue* q,ID3D12Fence* fence,uint64_t value){hr(q->Signal(fence,value));HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);hr(fence->SetEventOnCompletion(value,event));check(WaitForSingleObject(event,5000)==WAIT_OBJECT_0,"GPU wait");CloseHandle(event);}
int main(){
 ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
 ComPtr<IDXGIFactory4> factory;hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<ID3D12Device> d;
 for(UINT i=0;!d;++i){ComPtr<IDXGIAdapter1> a;if(factory->EnumAdapters1(i,&a)==DXGI_ERROR_NOT_FOUND)break;DXGI_ADAPTER_DESC1 desc{};a->GetDesc1(&desc);if(!(desc.Flags&DXGI_ADAPTER_FLAG_SOFTWARE))D3D12CreateDevice(a.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&d));}
 check(d!=nullptr,"hardware GPU");ComPtr<ID3D12CommandQueue> q;D3D12_COMMAND_QUEUE_DESC qd{};hr(d->CreateCommandQueue(&qd,IID_PPV_ARGS(&q)));
 ComPtr<ID3D12Fence> done,gate;hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&done)));hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)));
 ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;hr(d->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));hr(d->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));hr(list->Close());
 uint64_t poses[]{100,101};uint64_t signal=0;
 flight::enabled=true;check(source_timing::Wanted(16)&&!source_timing::Wanted(17),"one in sixteen sampling");
 auto reset=[&]{hr(allocator->Reset());hr(list->Reset(allocator.Get(),nullptr));source_timing::ResetList(list.Get());cvr::gpu::ResetCommandResources(list.Get());};
 auto record=[&](unsigned frame,bool rr){for(unsigned eye=0;eye<2;++eye)for(unsigned stage=0;stage<3;++stage)source_timing::Record(list.Get(),frame,poses[eye],9,eye,source_timing::Stage(stage),rr);};
 auto submit=[&]{hr(list->Close());ID3D12CommandList* lists[]{list.Get()};auto resources=cvr::gpu::PrepareCommandResources(1,lists);q->ExecuteCommandLists(1,lists);source_timing::Submitted(q.Get(),1,lists);cvr::gpu::SubmitCommandResources(q.Get(),std::move(resources));};
 reset();record(16,true);hr(q->Wait(gate.Get(),1));const auto queued=now();submit();
 auto source=source_timing::Acquire(16,poses,9,q.Get());check(bool(source),"exact frame acquired");
 check(!source_timing::Acquire(17,poses,9,q.Get())&&!source_timing::Acquire(16,poses,10,q.Get()),"frame/origin mismatch rejected");uint64_t wrong[]{100,102};check(!source_timing::Acquire(16,wrong,9,q.Get()),"eye pose mismatch rejected");
 CaptureTiming::Result cap{0,.1,1,now()+5};check(!source_timing::Read(source,cap,now(),now()+1),"unfinished source timestamps not read");
 Sleep(20);hr(gate->Signal(1));wait(q.Get(),done.Get(),++signal);Sleep(5);cap={.2,.1,.3,now()};
 auto result=source_timing::Read(source,cap,cap.endQpcMs,cap.endQpcMs+1);check(result && result->mask==63 && result->rr,"all six marker identities and RR preserved");
 check(result->end[0][0]-queued>15 && result->captureStart-result->end[1][2]>3,"producer GPU delay distinguished from later capture gap");
 // Replay invalidates timestamps instead of treating overwritten query data as same-frame evidence.
 ID3D12CommandList* lists[]{list.Get()};q->ExecuteCommandLists(1,lists);source_timing::Submitted(q.Get(),1,lists);wait(q.Get(),done.Get(),++signal);
 check(!source_timing::Read(source,cap,0,1),"replayed recording rejected");source.reset();
 // More than pool capacity sequential frames: reset plus completion permits reuse.
 for(unsigned i=2;i<26;++i){reset();record(i*16,false);submit();wait(q.Get(),done.Get(),++signal);Sleep(1);cap={.1,.1,.2,now()};auto f=source_timing::Acquire(i*16,poses,9,q.Get());check(f && source_timing::Read(f,cap,cap.endQpcMs,cap.endQpcMs+1),"completed reset recordings reuse bounded pool");cvr::gpu::CollectCommandResources();}
 reset();record(26*16,false);source_timing::Record(list.Get(),26*16,poses[0],9,0,source_timing::Stage::Depth);submit();wait(q.Get(),done.Get(),++signal);check(!source_timing::Acquire(26*16,poses,9,q.Get()),"duplicate stage invalidates sample");
 reset();record(27*16,false);hr(list->Close());hr(list->Reset(allocator.Get(),nullptr));source_timing::ResetList(list.Get());cvr::gpu::ResetCommandResources(list.Get());hr(list->Close());check(!source_timing::Acquire(27*16,poses,9,q.Get()),"abandoned recording cannot produce evidence");
 ComPtr<ID3D12InfoQueue> info;if(SUCCEEDED(d.As(&info)))for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T size{};info->GetMessage(i,nullptr,&size);std::vector<char> b(size);auto* m=reinterpret_cast<D3D12_MESSAGE*>(b.data());info->GetMessage(i,m,&size);if(m->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<m->pDescription<<'\n';check(false,"GPU validation");}}
 std::cout<<"PASS: production source timeline, exact identities, GPU delay attribution, pending/replay/duplicate/abandoned rejection, bounded reset reuse\n";
}
