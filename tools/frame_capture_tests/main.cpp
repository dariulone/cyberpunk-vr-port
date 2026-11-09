#include "Framegen/FrameCapture.hpp"
#include "Core/Paths.hpp"
#include <dxgi1_6.h>
#include <atomic>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
using Microsoft::WRL::ComPtr;
using namespace cvr::framegen;
extern "C" std::atomic<int32_t> CyberpunkVR_FramegenCapture;
bool visible=false;
bool OverlayIsVisible(){return visible;}
void Log(const char*,...){}
Settings cvr::framegen::GetSettings(){Settings s;s.backend=Backend::Nvidia;s.quality=1;s.flowScale=50;return s;}
namespace cvr::paths { const std::wstring& Dir(Folder){static const auto dir=(std::filesystem::temp_directory_path()/L"cpvr-capture-test"/std::to_wstring(GetCurrentProcessId())).wstring();return dir;} }
void Check(bool v,const char* msg){if(!v)throw std::runtime_error(msg);}
void Hr(HRESULT h){Check(SUCCEEDED(h),"D3D12 operation failed");}
int main()try{
 ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
 ComPtr<IDXGIAdapter> adapter;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
 ComPtr<ID3D12Device> d;Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&d)));
 D3D12_COMMAND_QUEUE_DESC qd{};ComPtr<ID3D12CommandQueue> q;Hr(d->CreateCommandQueue(&qd,IID_PPV_ARGS(&q)));
 ComPtr<ID3D12Fence> fence;Hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));uint64_t value=0;
 auto wait=[&]{Hr(q->Signal(fence.Get(),++value));HANDLE ev=CreateEventW(nullptr,FALSE,FALSE,nullptr);Hr(fence->SetEventOnCompletion(value,ev));Check(WaitForSingleObject(ev,10000)==WAIT_OBJECT_0,"GPU timeout");CloseHandle(ev);};
 auto texture=[&](DXGI_FORMAT format,D3D12_RESOURCE_STATES state){ComPtr<ID3D12Resource> r;D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;desc.Width=16;desc.Height=12;desc.DepthOrArraySize=desc.MipLevels=desc.SampleDesc.Count=1;desc.Format=format;D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;Hr(d->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(&r)));return r;};
 ComPtr<ID3D12Resource> colors[2];ID3D12Resource* raw[2]{};std::shared_ptr<Inputs> in[2];std::shared_ptr<const Inputs> inputs[2];EyePair cameras[2]{};
 for(int e=0;e<2;++e){colors[e]=texture(DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_SOURCE);raw[e]=colors[e].Get();in[e]=std::make_shared<Inputs>();in[e]->depth=texture(DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);in[e]->motion=texture(DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);in[e]->hasDepth=in[e]->hasMotion=true;inputs[e]=in[e];}
 auto capture=[&](unsigned id){for(auto& i:in)i->frameId=id;CaptureFrame(d.Get(),q.Get(),raw,inputs,cameras);wait();};
 char status[200];bool busy=false;
 Check(ScheduleFrameCapture(10),"schedule failed");Check(!ScheduleFrameCapture(0),"duplicate accepted");capture(1);FrameCaptureStatus(status,sizeof(status),&busy);Check(busy && std::string(status).find("Starts in")!=std::string::npos,"countdown bypassed");
 CancelFrameCapture();FrameCaptureStatus(status,sizeof(status),&busy);Check(!busy,"cancel failed");
 Check(ScheduleFrameCapture(0),"rearm failed");visible=true;capture(1);FrameCaptureStatus(status,sizeof(status));Check(std::string(status).find("0/3")!=std::string::npos,"captured menu");visible=false;
 capture(1);capture(1);FrameCaptureStatus(status,sizeof(status));Check(std::string(status).find("1/3")!=std::string::npos,"duplicate frame accepted");
 capture(3);FrameCaptureStatus(status,sizeof(status));Check(std::string(status).find("1/3")!=std::string::npos,"gap not reset");capture(4);capture(5);
 for(int i=0;i<500 && CyberpunkVR_FramegenCapture.load()==1;++i)Sleep(10);
 Check(CyberpunkVR_FramegenCapture.load()==2,"capture did not save");
 auto dir=std::filesystem::path(cvr::paths::Dir(cvr::paths::Folder::Log))/L"framegen-capture";size_t raws=0;std::string json;
 for(auto& f:std::filesystem::recursive_directory_iterator(dir)){if(f.path().extension()==".raw"){++raws;auto n=f.path().filename().string();Check(f.file_size()==16*12*(n.find("motion")!=std::string::npos?8:4),"raw size wrong");}if(f.path().filename()=="capture.json"){std::ifstream file(f.path());json.assign(std::istreambuf_iterator<char>(file),{});}}
 Check(raws==18,"missing stereo captures");Check(json.find("\"quality\":1")!=std::string::npos && json.find("\"qpcFrequency\":")!=std::string::npos,"missing replay metadata");
 Check(ScheduleFrameCapture(10),"cannot capture twice");CancelFrameCapture();
 std::cout<<"PASS countdown, menu exclusion, cancel/rearm, duplicate/gap handling, WARP stereo copies and replay metadata\n";
 return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
