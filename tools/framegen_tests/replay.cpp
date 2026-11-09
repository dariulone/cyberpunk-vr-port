#include "Framegen/CapturedColor.hpp"
// Replays a capture written by the plugin (CyberpunkVR_FramegenCapture) through every generator.
// Frames 0 and 2 are the real pair; frame 1, rendered between them, is the truth. Frame 2's vectors
// reach frame 0 by composing its own (2 -> 1) with frame 1's (1 -> 0). Each output is rotated from
// the eye pose it is submitted with into frame 1's pose -- what the headset runtime does -- and
// compared with frame 1. Repeating frame 2 (or 0) with that same rotation is the baseline: the
// runtime's reprojection alone.
#include "Framegen/Generator.hpp"
#include "Framegen/Framegen.hpp"
#include "Framegen/DlssFrameGen.hpp"
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <DirectXPackedVector.h>
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <functional>
#include <map>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#ifdef CVR_V557_REFERENCE
#include "xrfg/d3d12_frame_synthesizer.hpp"
#endif
using Microsoft::WRL::ComPtr;
using namespace cvr::framegen;
void Check(bool value,const char* text);
void Hr(HRESULT result);

namespace replay {
// ---- a small JSON reader for capture.json (objects, arrays, numbers) ----
struct Value {
    enum Kind {Null,Number,Array,Object} kind=Null;
    double number{};std::vector<Value> items;std::map<std::string,Value> fields;
    const Value& operator[](const char* key) const {auto it=fields.find(key);if(it==fields.end())throw std::runtime_error(std::string("capture.json lacks ")+key);return it->second;}
    const Value& operator[](int i) const {return items.at(size_t(i));}
    float f(size_t i) const {return float(items.at(i).number);}
};
struct Parser {
    const std::string& s;size_t p=0;
    void Space() {while(p<s.size() && std::isspace(uint8_t(s[p])))++p;}
    Value Parse() {
        Space();Value v;
        if(s[p]=='{') {v.kind=Value::Object;++p;Space();if(s[p]=='}'){++p;return v;}
            for(;;){Space();Check(s[p]=='"',"json key");const auto end=s.find('"',p+1);std::string key=s.substr(p+1,end-p-1);p=end+1;Space();Check(s[p]==':',"json colon");++p;
                v.fields[key]=Parse();Space();if(s[p]==','){++p;continue;}Check(s[p]=='}',"json object");++p;return v;}}
        if(s[p]=='[') {v.kind=Value::Array;++p;Space();if(s[p]==']'){++p;return v;}
            for(;;){v.items.push_back(Parse());Space();if(s[p]==','){++p;continue;}Check(s[p]==']',"json array");++p;return v;}}
        v.kind=Value::Number;char* end{};v.number=std::strtod(s.c_str()+p,&end);Check(end!=s.c_str()+p,"json number");p=size_t(end-s.c_str());return v;
    }
};
std::vector<uint8_t> ReadFile(const std::filesystem::path& path) {
    std::ifstream f(path,std::ios::binary);Check(f.good(),"capture file missing");
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)),std::istreambuf_iterator<char>());
}

struct EyeFrame {
    uint32_t width{},height{},depthWidth{},depthHeight{},motionWidth{},motionHeight{};
    std::vector<uint8_t> color;          // RGBA8, sRGB-encoded
    std::vector<float> depth;            // R32F
    std::vector<float> motion;           // float2 per texel, raw (before motionScale)
    CameraData camera{};EyeCamera eye{};
};
float Half(uint16_t h) {return DirectX::PackedVector::XMConvertHalfToFloat(h);}
EyeFrame LoadEye(const std::filesystem::path& dir,const Value& eye,size_t frame,unsigned index) {
    EyeFrame e;char name[64];
    e.width=uint32_t(eye["color"][0].number);e.height=uint32_t(eye["color"][1].number);
    e.depthWidth=uint32_t(eye["depth"][0].number);e.depthHeight=uint32_t(eye["depth"][1].number);
    e.motionWidth=uint32_t(eye["motion"][0].number);e.motionHeight=uint32_t(eye["motion"][1].number);
    const int motionFormat=int(eye["motion"][2].number);
    std::snprintf(name,sizeof(name),"f%zu_e%u_color.raw",frame,index);e.color=ReadFile(dir/name);
    Check(e.color.size()==size_t(e.width)*e.height*4,"colour size");
    std::snprintf(name,sizeof(name),"f%zu_e%u_depth.raw",frame,index);auto depth=ReadFile(dir/name);
    Check(depth.size()==size_t(e.depthWidth)*e.depthHeight*4,"depth size");
    e.depth.resize(depth.size()/4);std::memcpy(e.depth.data(),depth.data(),depth.size());
    std::snprintf(name,sizeof(name),"f%zu_e%u_motion.raw",frame,index);auto motion=ReadFile(dir/name);
    const size_t texels=size_t(e.motionWidth)*e.motionHeight;e.motion.resize(texels*2);
    for(size_t i=0;i<texels;++i) {
        if(motionFormat==DXGI_FORMAT_R16G16_FLOAT) {uint16_t h[2];std::memcpy(h,&motion[i*4],4);e.motion[i*2]=Half(h[0]);e.motion[i*2+1]=Half(h[1]);}
        else if(motionFormat==DXGI_FORMAT_R16G16B16A16_FLOAT) {uint16_t h[2];std::memcpy(h,&motion[i*8],4);e.motion[i*2]=Half(h[0]);e.motion[i*2+1]=Half(h[1]);}
        else if(motionFormat==DXGI_FORMAT_R32G32_FLOAT) std::memcpy(&e.motion[i*2],&motion[i*8],8);
        else throw std::runtime_error("motion format");
    }
    auto& c=e.camera;
    for(int i=0;i<2;++i){c.jitter[i]=eye["jitter"].f(i);c.motionScale[i]=eye["motionScale"].f(i);}
    for(int i=0;i<3;++i){c.position[i]=eye["position"].f(i);c.up[i]=eye["up"].f(i);c.right[i]=eye["right"].f(i);c.forward[i]=eye["forward"].f(i);}
    c.nearPlane=eye["planes"].f(0);c.farPlane=eye["planes"].f(1);c.verticalFov=eye["planes"].f(2);c.aspect=eye["planes"].f(3);
    c.inverted=eye["inverted"].number!=0;c.infinite=eye["infinite"].number!=0;c.jittered=eye["jittered"].number!=0;c.cameraMotion=true;
    for(int i=0;i<4;++i){e.eye.orientation[i]=eye["orientation"].f(i);e.eye.fov[i]=eye["fov"].f(i);}
    return e;
}
// Frame 2 -> frame 0 vectors: frame 2's own vector, then frame 1's at the point it lands on.
std::vector<float> Compose(const EyeFrame& c,const EyeFrame& b) {
    std::vector<float> out(c.motion.size());
    // Nearest texel: a bilinear lookup across a depth edge would blend a foreground and a background
    // vector into one that belongs to neither surface.
    auto sampleB=[&](float u,float v,float* r) {
        const int x=std::clamp(int(u*b.motionWidth),0,int(b.motionWidth)-1),y=std::clamp(int(v*b.motionHeight),0,int(b.motionHeight)-1);
        for(int k=0;k<2;++k)r[k]=b.motion[(size_t(y)*b.motionWidth+x)*2+k]*b.camera.motionScale[k];
    };
    for(uint32_t y=0;y<c.motionHeight;++y)for(uint32_t x=0;x<c.motionWidth;++x) {
        const size_t i=(size_t(y)*c.motionWidth+x)*2;
        const float mx=c.motion[i]*c.camera.motionScale[0],my=c.motion[i+1]*c.camera.motionScale[1];
        float mb[2];sampleB((x+.5f)/c.motionWidth+mx,(y+.5f)/c.motionHeight+my,mb);
        out[i]=mx+mb[0];out[i+1]=my+mb[1];
    }
    return out;
}

// ---- rotation-only reprojection (the runtime's job), as OFXR's map_target_to_source ----
struct Quat {float x,y,z,w;};
Quat Normalize(const float* q) {const float n=std::sqrt(q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3]);return n>0?Quat{q[0]/n,q[1]/n,q[2]/n,q[3]/n}:Quat{0,0,0,1};}
Quat Mul(Quat a,Quat b) {return {a.w*b.x+a.x*b.w+a.y*b.z-a.z*b.y,a.w*b.y-a.x*b.z+a.y*b.w+a.z*b.x,a.w*b.z+a.x*b.y-a.y*b.x+a.z*b.w,a.w*b.w-a.x*b.x-a.y*b.y-a.z*b.z};}
void Rotate(Quat q,const float* v,float* out) {
    const float c1[3]={q.y*v[2]-q.z*v[1]+q.w*v[0],q.z*v[0]-q.x*v[2]+q.w*v[1],q.x*v[1]-q.y*v[0]+q.w*v[2]};
    const float c2[3]={q.y*c1[2]-q.z*c1[1],q.z*c1[0]-q.x*c1[2],q.x*c1[1]-q.y*c1[0]};
    for(int i=0;i<3;++i)out[i]=v[i]+2*c2[i];
}
EyeCamera Middle(const EyeCamera& a,const EyeCamera& b) {
    EyeCamera m=b;const float d=a.orientation[0]*b.orientation[0]+a.orientation[1]*b.orientation[1]+a.orientation[2]*b.orientation[2]+a.orientation[3]*b.orientation[3];
    const float s=d<0?-1.f:1.f;float q[4];for(int i=0;i<4;++i)q[i]=a.orientation[i]+s*b.orientation[i];const auto n=Normalize(q);
    m.orientation[0]=n.x;m.orientation[1]=n.y;m.orientation[2]=n.z;m.orientation[3]=n.w;return m;
}
// image drawn with camera `source`, seen from camera `target`; valid=0 outside the source image
std::vector<uint8_t> Reproject(const std::vector<uint8_t>& image,uint32_t w,uint32_t h,const EyeCamera& source,const EyeCamera& target,std::vector<uint8_t>* valid) {
    const Quat s=Normalize(source.orientation),t=Normalize(target.orientation);
    const Quat r=Mul(Quat{-s.x,-s.y,-s.z,s.w},t);
    float st[4],tt[4];for(int i=0;i<4;++i){st[i]=std::tan(source.fov[i]);tt[i]=std::tan(target.fov[i]);}
    std::vector<uint8_t> out(image.size());if(valid)valid->assign(size_t(w)*h,0);
    for(uint32_t y=0;y<h;++y)for(uint32_t x=0;x<w;++x) {
        const float u=(x+.5f)/w,v=(y+.5f)/h;
        const float ray[3]={tt[0]+(tt[1]-tt[0])*u,tt[2]+(tt[3]-tt[2])*v,-1};float sr[3];Rotate(r,ray,sr);
        if(sr[2]>=-1e-5f)continue;
        const float tx=sr[0]/-sr[2],ty=sr[1]/-sr[2];
        const float sx=(tx-st[0])/(st[1]-st[0])*w-.5f,sy=(ty-st[2])/(st[3]-st[2])*h-.5f;
        if(sx<-.5f || sy<-.5f || sx>w-.5f || sy>h-.5f)continue;
        const float cx=std::clamp(sx,0.f,float(w-1)),cy=std::clamp(sy,0.f,float(h-1));
        const int x0=int(cx),y0=int(cy),x1=std::min<int>(x0+1,w-1),y1=std::min<int>(y0+1,h-1);const float fx=cx-x0,fy=cy-y0;
        for(int k=0;k<4;++k) {
            auto p=[&](int xx,int yy){return float(image[(size_t(yy)*w+xx)*4+k]);};
            out[(size_t(y)*w+x)*4+k]=uint8_t(std::clamp(((p(x0,y0)*(1-fx)+p(x1,y0)*fx)*(1-fy)+(p(x0,y1)*(1-fx)+p(x1,y1)*fx)*fy)+.5f,0.f,255.f));
        }
        if(valid)(*valid)[size_t(y)*w+x]=1;
    }
    return out;
}

struct Score {double psnr{},mae{},motionPsnr{},motionMae{};size_t pixels{},motionPixels{};};
Score Compare(const std::vector<uint8_t>& a,const std::vector<uint8_t>& truth,const std::vector<uint8_t>& valid,const std::vector<uint8_t>& motion) {
    Score s;double se=0,ae=0,mse=0,mae=0;
    for(size_t i=0;i<valid.size();++i) {
        if(!valid[i])continue;
        double e2=0,e1=0;for(int k=0;k<3;++k){const double d=double(a[i*4+k])-double(truth[i*4+k]);e2+=d*d;e1+=std::abs(d);}
        se+=e2;ae+=e1;++s.pixels;
        if(motion[i]){mse+=e2;mae+=e1;++s.motionPixels;}
    }
    auto psnr=[](double sum,size_t n){return n?10*std::log10(255.0*255.0/std::max(1e-9,sum/(3.0*n))):0.0;};
    s.psnr=psnr(se,s.pixels);s.mae=s.pixels?ae/(3.0*s.pixels):0;s.motionPsnr=psnr(mse,s.motionPixels);s.motionMae=s.motionPixels?mae/(3.0*s.motionPixels):0;
    return s;
}
void SavePng(const std::filesystem::path& path,const std::vector<uint8_t>& rgba,uint32_t w,uint32_t h,uint32_t x0,uint32_t y0,uint32_t cw,uint32_t ch) {
    ComPtr<IWICImagingFactory> factory;Hr(CoCreateInstance(CLSID_WICImagingFactory,nullptr,CLSCTX_INPROC_SERVER,IID_PPV_ARGS(&factory)));
    ComPtr<IWICStream> stream;Hr(factory->CreateStream(&stream));Hr(stream->InitializeFromFilename(path.c_str(),GENERIC_WRITE));
    ComPtr<IWICBitmapEncoder> encoder;Hr(factory->CreateEncoder(GUID_ContainerFormatPng,nullptr,&encoder));Hr(encoder->Initialize(stream.Get(),WICBitmapEncoderNoCache));
    ComPtr<IWICBitmapFrameEncode> frame;Hr(encoder->CreateNewFrame(&frame,nullptr));Hr(frame->Initialize(nullptr));Hr(frame->SetSize(cw,ch));
    WICPixelFormatGUID format=GUID_WICPixelFormat32bppRGBA;Hr(frame->SetPixelFormat(&format));
    std::vector<uint8_t> crop(size_t(cw)*ch*4);
    for(uint32_t y=0;y<ch;++y)for(uint32_t x=0;x<cw;++x)for(int k=0;k<4;++k)crop[(size_t(y)*cw+x)*4+k]=k==3?255:rgba[(size_t(y+y0)*w+x+x0)*4+k];
    if(format==GUID_WICPixelFormat32bppRGBA)Hr(frame->WritePixels(ch,cw*4,UINT(crop.size()),crop.data()));
    else {for(size_t i=0;i<crop.size();i+=4)std::swap(crop[i],crop[i+2]);Hr(frame->WritePixels(ch,cw*4,UINT(crop.size()),crop.data()));}
    Hr(frame->Commit());Hr(encoder->Commit());
}
}

// Uses the harness's Gpu helper through these callbacks so the device code stays in main.cpp.
struct ReplayGpu {
    std::function<ComPtr<ID3D12Resource>(unsigned,unsigned,DXGI_FORMAT,D3D12_RESOURCE_STATES)> texture;
    std::function<void(ID3D12Resource*,const void*,unsigned,D3D12_RESOURCE_STATES)> upload;
    std::function<std::vector<uint8_t>(ID3D12Resource*)> read;
    std::function<void()> wait;
    ID3D12Device* device{};ID3D12CommandQueue* queue{};
    std::function<void(ID3D12Fence*,uint64_t*)> fence;
};


#ifdef CVR_V557_REFERENCE
// The original V557 synthesizer/library, not a reimplementation. Its NVIDIA-only
// path takes no game vectors and outputs in B's camera. Reproject that output
// into the measured middle frame's eye for the same comparison as the native path.
void ReplayV557(ReplayGpu& gpu,replay::EyeFrame (&frames)[3][2],int quality,
                std::vector<uint8_t> (&result)[2]) {
    for(unsigned e=0;e<2;++e){
        auto& a=frames[0][e];auto& b=frames[2][e];
        ComPtr<ID3D12Resource> source[2],current,synthetic;ID3D12Resource* sources[2];
        for(unsigned k=0;k<2;++k){auto& f=k?b:a;
            source[k]=gpu.texture(f.width,f.height,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
            gpu.upload(source[k].Get(),f.color.data(),4,D3D12_RESOURCE_STATE_COMMON);sources[k]=source[k].Get();}
        // V557 destinations require render-target capability.
        auto desc=source[0]->GetDesc();desc.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
        Hr(gpu.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&current)));
        Hr(gpu.device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_RENDER_TARGET,nullptr,IID_PPV_ARGS(&synthetic)));
        auto history=std::make_shared<xrfg::D3D12SwapchainHistory>();
        Hr(history->initialize(gpu.device,gpu.queue,sources,D3D12_RESOURCE_STATE_COMMON));
        xrfg::D3D12FrameSynthesizer synth;xrfg::D3D12NvidiaOpticalFlowOptions opts;
        opts.preset=quality==0?xrfg::D3D12NvidiaPerformancePreset::fast:quality==1?xrfg::D3D12NvidiaPerformancePreset::medium:xrfg::D3D12NvidiaPerformancePreset::slow;
        opts.input_scale=xrfg::D3D12OpticalFlowInputScale::half;opts.hybrid=false;opts.bidirectional=false;
        ID3D12Resource* cur[]{current.Get()};ID3D12Resource* out[]{synthetic.Get()};
        Hr(synth.initialize(gpu.device,gpu.queue,history,cur,out,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_RENDER_TARGET,xrfg::D3D12OpticalFlowBackend::nvidia,opts));
        auto view=[](const replay::EyeFrame& f){xrfg::D3D12ReprojectionView v;
            const auto& q=f.eye.orientation;v.pose.orientation={q[0],q[1],q[2],q[3]};
            const auto& t=f.eye.fov;v.fov={t[0],t[1],t[2],t[3]};v.image_rect={0,0,f.width,f.height};v.array_slice=0;return v;};
        auto av=view(a),bv=view(b);xrfg::D3D12HistoryCaptureTicket ticket;xrfg::D3D12FrameSynthesisTicket generated;
        Hr(history->capture(0,&ticket));Hr(history->commit(ticket));Hr(synth.submit_prime(ticket,{&av,1},0,&generated));Hr(synth.wait_for_previous_submission(5000));
        Hr(history->capture(1,&ticket));Hr(history->commit(ticket));Hr(synth.submit_pair(ticket,{&bv,1},{&bv,1},0,0,&generated));Hr(synth.wait_for_idle());
        // Read via the fixture after a small state-transition list.
        ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;
        Hr(gpu.device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
        Hr(gpu.device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition={synthetic.Get(),0,D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&barrier);Hr(list->Close());
        ID3D12CommandList* lists[]{list.Get()};gpu.queue->ExecuteCommandLists(1,lists);gpu.wait();
        result[e]=replay::Reproject(gpu.read(synthetic.Get()),b.width,b.height,b.eye,frames[1][e].eye,nullptr);
    }
}
#endif

int Replay(ReplayGpu& gpu,const std::filesystem::path& dir,const std::filesystem::path& out) {
    using namespace replay;
    const auto text=ReadFile(dir/"capture.json");const std::string json(text.begin(),text.end());
    Parser parser{json};const auto root=parser.Parse();const auto& frames=root["frames"];Check(frames.items.size()==3,"three frames expected");
    EyeFrame f[3][2];
    for(size_t k=0;k<3;++k)for(unsigned e=0;e<2;++e)f[k][e]=LoadEye(dir,frames[int(k)]["eyes"][int(e)],k,e);
    for(unsigned k=0;k<3;++k)for(unsigned e=0;e<2;++e){
        const auto& v=f[k][e];const auto& first=f[0][e];
        Check(v.width==first.width && v.height==first.height && v.motionWidth && v.motionHeight,"capture dimensions changed or motion absent");
        if(k)Check(frames[int(k)]["frame"].number==frames[int(k-1)]["frame"].number+1,"nonconsecutive capture");
    }
    const auto field=[&](const char* key,double fallback){auto it=root.fields.find(key);return it==root.fields.end()?fallback:it->second.number;};
    const int quality=std::clamp(int(field("quality",2)),0,2);
    double frameMs=44.4;
    if(field("qpcFrequency",0)>0)frameMs=(frames[2]["eyes"][0]["qpc"].number-frames[0]["eyes"][0]["qpc"].number)*1000/field("qpcFrequency",0);
    Check(frameMs>0 && frameMs<1000,"invalid captured frame interval");
    std::printf("quality=%d source interval=%.3fms; reference uses midpoint (unequal capture spacing limits accuracy)\n",quality,frameMs);
    std::filesystem::create_directories(out);
    CoInitializeEx(nullptr,COINIT_MULTITHREADED);
    struct Variant {Backend backend;int flowScale;const char* name;bool borrowed{};};
    const Variant variants[]={{Backend::FidelityFX,50,"FidelityFX"},{Backend::FidelityFX,0,"FidelityFX no flow"},
        {Backend::Nvidia,50,"NVIDIA OFA + FidelityFX"},{Backend::NvidiaV557,50,"NVIDIA V557 native adapter"},{Backend::NvidiaV557,50,"NVIDIA V557 borrowed",true},{Backend::DlssG,50,"NVIDIA DLSS-G"}};
    const uint32_t w=f[1][0].width,h=f[1][0].height;
    // motion region: where frame 1 differs from frame 2 shown with the runtime's rotation
    std::vector<uint8_t> motion[2],valid[2],baseline[2],baselineA[2];
    for(unsigned e=0;e<2;++e) {
        baseline[e]=Reproject(f[2][e].color,w,h,f[2][e].eye,f[1][e].eye,&valid[e]);
        std::vector<uint8_t> validA;baselineA[e]=Reproject(f[0][e].color,w,h,f[0][e].eye,f[1][e].eye,&validA);
        motion[e].assign(size_t(w)*h,0);
        for(size_t i=0;i<motion[e].size();++i) {
            valid[e][i]=valid[e][i] && validA[i];
            int d=0;for(int k=0;k<3;++k)d=std::max(d,std::abs(int(baseline[e][i*4+k])-int(f[1][e].color[i*4+k])));
            motion[e][i]=valid[e][i] && d>12;
        }
    }
    std::printf("replay %ls: %ux%u, frames %u/%u/%u\n",dir.c_str(),w,h,uint32_t(frames[0]["frame"].number),uint32_t(frames[1]["frame"].number),uint32_t(frames[2]["frame"].number));
    auto report=[&](const char* name,const std::vector<uint8_t> (&images)[2]) {
        for(unsigned e=0;e<2;++e) {
            const auto s=Compare(images[e],f[1][e].color,valid[e],motion[e]);
            std::printf("  %-26s eye %u  PSNR %6.2f dB  MAE %5.2f  | moving %4.1f%%: PSNR %6.2f dB  MAE %5.2f\n",name,e,s.psnr,s.mae,
                100.0*s.motionPixels/std::max<size_t>(1,s.pixels),s.motionPsnr,s.motionMae);
        }
    };
    report("repeat frame 2 (runtime)",baseline);
    report("repeat frame 0 (runtime)",baselineA);
    // crop around the image centre for the visual check
    const uint32_t cw=std::min(w,1024u),ch=std::min(h,768u),cx=(w-cw)/2,cy=(h-ch)/2;
    SavePng(out/"truth_e0.png",f[1][0].color,w,h,cx,cy,cw,ch);SavePng(out/"repeat_e0.png",baseline[0],w,h,cx,cy,cw,ch);SavePng(out/"repeat0_e0.png",baselineA[0],w,h,cx,cy,cw,ch);
    for(const auto& variant:variants) {
        const Backend backend=variant.backend;
        if(backend==Backend::DlssG && !dlssg::Available(gpu.device)) {std::printf("  %-26s unavailable: %s\n","DLSS-G",dlssg::Reason());continue;}
        Generator generator;Settings settings;settings.enabled=true;settings.backend=backend;settings.quality=quality;settings.flowScale=variant.flowScale;SetSettings(settings);
        std::vector<uint8_t> results[2];bool ok=true;
        ComPtr<ID3D12Fence> captureReady;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&captureReady)));
        // FFX ignores optical-flow blending for the first ten frames after reset.
        // Prime with stationary copies of A and zero vectors, then evaluate A->B.
        // This removes the startup gate; it does not recreate real prior history.
        constexpr int warmup=11;
        for(int step=0;step<=warmup && ok;++step) {
            const bool evaluate=step==warmup;
            const size_t k=evaluate?2:0;
            ComPtr<ID3D12Resource> color[2];ID3D12Resource* colors[2]{};std::shared_ptr<const Inputs> inputs[2];EyePair cameras[2];
            for(unsigned e=0;e<2;++e) {
                const auto& src=f[k][e];auto in=std::make_shared<Inputs>();
                in->width=src.depthWidth;in->height=src.depthHeight;in->motionWidth=src.motionWidth;in->motionHeight=src.motionHeight;
                in->camera=src.camera;in->hasDepth=in->hasMotion=true;in->frameId=uint32_t(frames[int(k)]["frame"].number);
                color[e]=gpu.texture(src.width,src.height,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
                if(variant.borrowed)Hr(captured_color::MarkShared(color[e].Get()));
                gpu.upload(color[e].Get(),src.color.data(),4,captured_color::ReadState(color[e].Get()));
                auto depth=gpu.texture(src.depthWidth,src.depthHeight,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
                gpu.upload(depth.Get(),src.depth.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
                std::vector<float> vectors=evaluate?Compose(src,f[1][e]):std::vector<float>(src.motion.size(),0.0f);
                in->camera.motionScale[0]=in->camera.motionScale[1]=1;
                auto mv=gpu.texture(src.motionWidth,src.motionHeight,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
                gpu.upload(mv.Get(),vectors.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
                in->depth=depth;in->motion=mv;colors[e]=color[e].Get();inputs[e]=in;
                cameras[e].previous=f[0][e].eye;cameras[e].current=f[k][e].eye;
            }
            bool interpolated=false;
            auto lease=std::make_shared<std::array<ComPtr<ID3D12Resource>,2>>(std::array<ComPtr<ID3D12Resource>,2>{color[0],color[1]});
            Hr(gpu.queue->Signal(captureReady.Get(),uint64_t(step+1)));
            ok=generator.Generate(gpu.device,gpu.queue,colors,inputs,cameras,settings,frameMs,step==0,uint64_t(step+1),&interpolated,captureReady.Get(),uint64_t(step+1),lease);
            gpu.wait();
            // V557 context registration is asynchronous; retry its first submission after the receipt.
            if(!ok && step==0 && backend==Backend::NvidiaV557){ok=generator.Generate(gpu.device,gpu.queue,colors,inputs,cameras,settings,frameMs,true,1,&interpolated,captureReady.Get(),uint64_t(step+1),lease);gpu.wait();}
            if(ok && evaluate) {
                ok=interpolated && ActiveBackend()==backend;
                for(unsigned e=0;e<2 && ok;++e) {
                    auto pixels=gpu.read(generator.Output(e));
                    const EyeCamera shown=backend==Backend::NvidiaV557?f[2][e].eye:Middle(f[0][e].eye,f[2][e].eye);   // what the presenter submits
                    results[e]=Reproject(pixels,w,h,shown,f[1][e].eye,nullptr);
                }
            }
        }
        if(!ok) {std::printf("  %-26s FAILED (active %s)\n",variant.name,BackendName(ActiveBackend()));continue;}
        std::printf("  %s explicit allocation bytes: %llu\n",variant.name,static_cast<unsigned long long>(generator.Bytes()));
        report(variant.name,results);
        std::string name=variant.name;for(auto& c:name)if(!std::isalnum(uint8_t(c)))c='_';
        for(unsigned e=0;e<2;++e)SavePng(out/(name+"_e"+std::to_string(e)+".png"),results[e],w,h,0,0,w,h);
        // error map: |output - truth| x4, red where the pixel is in the moving region
        std::vector<uint8_t> errors(results[0].size());
        for(size_t i=0;i<valid[0].size();++i) {
            int d=0;for(int k=0;k<3;++k)d=std::max(d,std::abs(int(results[0][i*4+k])-int(f[1][0].color[i*4+k])));
            const uint8_t v=uint8_t(std::min(255,d*4));errors[i*4]=v;errors[i*4+1]=motion[0][i]?v/2:v;errors[i*4+2]=motion[0][i]?v/2:v;errors[i*4+3]=255;
        }
        SavePng(out/(name+"_error_e0.png"),errors,w,h,cx,cy,cw,ch);
        while(!generator.Reset())gpu.wait();
    }
#ifdef CVR_V557_REFERENCE
    std::vector<uint8_t> reference[2];ReplayV557(gpu,f,quality,reference);report("V557 NVIDIA reference",reference);
    for(unsigned e=0;e<2;++e){
        SavePng(out/("V557_e"+std::to_string(e)+".png"),reference[e],w,h,0,0,w,h);
        SavePng(out/("truth_full_e"+std::to_string(e)+".png"),f[1][e].color,w,h,0,0,w,h);
    }
#endif
    return 0;
}
