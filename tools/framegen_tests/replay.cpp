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
#include "hand_motion.hpp"
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstdlib>
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
    std::ifstream f(path,std::ios::binary|std::ios::ate);Check(f.good(),"capture file missing");
    const auto size=f.tellg();Check(size>=0,"capture file size");
    std::vector<uint8_t> bytes(static_cast<size_t>(size));
    f.seekg(0);Check(bool(f.read(reinterpret_cast<char*>(bytes.data()),size)),"short capture file");
    return bytes;
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
    const size_t stride=motionFormat==DXGI_FORMAT_R16G16_FLOAT ? 4 : 8;
    Check(texels && motion.size()==texels*stride,"motion size");
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

// CPU-only corresponding points and engine vectors for a ROI marked on frame 1.
int HandMotion(const std::filesystem::path& dir,int eye,hand_motion::Region roi) {
    using namespace replay;
    Check(eye==0 || eye==1,"eye must be 0 or 1");
    const auto bytes=ReadFile(dir/"capture.json");const std::string json(bytes.begin(),bytes.end());
    const auto frames=Parser{json}.Parse()["frames"];
    Check(frames.items.size()==3,"three frames expected");
    EyeFrame f[3];hand_motion::Pyramid luma[3];
    for(int k=0;k<3;++k) {
        Check(k==0 || frames[k]["frame"].number==frames[k-1]["frame"].number+1,"nonconsecutive frame IDs");
        f[k]=LoadEye(dir,frames[k]["eyes"][eye],k,eye);
        Check(f[k].width==f[0].width && f[k].height==f[0].height && f[k].depthWidth && f[k].depthHeight,"input dimensions");
        Check(!f[k].camera.jittered,"measurement requires unjittered motion vectors");
        luma[k]=hand_motion::Luma(f[k].color,f[k].width,f[k].height);
    }
    Check(roi.x>=0 && roi.y>=0 && roi.w>0 && roi.h>0 && int64_t(roi.x)+roi.w<=f[1].width &&
          int64_t(roi.y)+roi.h<=f[1].height,"ROI outside middle frame");
    for(int k=1;k<=2;++k) {
        std::printf("eye %d frame %.0f -> %.0f: ",eye,frames[k]["frame"].number,frames[k-1]["frame"].number);
        hand_motion::Measure(f[k],luma[1],luma[k==1?0:2],roi,k==1);
    }
    return 0;
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

ComPtr<ID3D12Resource> UploadEye(ReplayGpu& gpu,const replay::EyeFrame& src,const std::vector<float>& motion,
                                 std::shared_ptr<const Inputs>& result,bool normalized,uint32_t frameId) {
    auto in=std::make_shared<Inputs>();
    in->width=src.depthWidth;in->height=src.depthHeight;in->motionWidth=src.motionWidth;in->motionHeight=src.motionHeight;
    in->camera=src.camera;in->hasDepth=in->hasMotion=true;in->frameId=frameId;
    if(normalized)in->camera.motionScale[0]=in->camera.motionScale[1]=1;
    auto color=gpu.texture(src.width,src.height,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
    gpu.upload(color.Get(),src.color.data(),4,D3D12_RESOURCE_STATE_COPY_SOURCE);
    in->depth=gpu.texture(src.depthWidth,src.depthHeight,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
    gpu.upload(in->depth.Get(),src.depth.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    in->motion=gpu.texture(src.motionWidth,src.motionHeight,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
    gpu.upload(in->motion.Get(),motion.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    result=in;
    return color;
}

int HandMidpoints(ReplayGpu& gpu,const std::filesystem::path& dir,const std::filesystem::path& out,
                  const hand_motion::Region (&regions)[2],int flowScale) {
    using namespace replay;
    const auto bytes=ReadFile(dir/"capture.json");const std::string json(bytes.begin(),bytes.end());
    const auto frames=Parser{json}.Parse()["frames"];
    Check(frames.items.size()==3,"three frames expected");
    EyeFrame f[3][2];
    for(int k=0;k<3;++k)for(int e=0;e<2;++e) {
        Check(k==0 || frames[k]["frame"].number==frames[k-1]["frame"].number+1,"nonconsecutive frame IDs");
        auto& src=f[k][e];src=LoadEye(dir,frames[k]["eyes"][e],k,e);
        const auto& r=regions[e];
        Check(src.width==f[0][e].width && src.height==f[0][e].height && r.x>=0 && r.y>=0 && r.w>0 && r.h>0 &&
              int64_t(r.x)+r.w<=src.width && int64_t(r.y)+r.h<=src.height,"ROI or frame size");
    }
    std::filesystem::create_directories(out);
    Hr(CoInitializeEx(nullptr,COINIT_MULTITHREADED));
    Settings settings;settings.enabled=true;settings.backend=Backend::Nvidia;settings.quality=2;settings.flowScale=flowScale;
    SetSettings(settings);Generator generator;
    LARGE_INTEGER frequency{};Check(QueryPerformanceFrequency(&frequency)!=0,"QPC frequency");
    uint64_t dispatch=0;
    for(int k=0;k<3;++k) {
        ComPtr<ID3D12Resource> color[2];ID3D12Resource* colors[2]{};std::shared_ptr<const Inputs> inputs[2];EyePair cameras[2];
        for(int e=0;e<2;++e) {
            const auto& src=f[k][e];
            const auto frameId=uint32_t(frames[k]["frame"].number);
            if(k==0)color[e]=UploadEye(gpu,src,std::vector<float>(src.motion.size()),inputs[e],false,frameId);
            else color[e]=UploadEye(gpu,src,src.motion,inputs[e],false,frameId);
            colors[e]=color[e].Get();cameras[e]={f[std::max(k-1,0)][e].eye,src.eye};
        }
        // Prime FI's ten-dispatch reset window with a stationary first endpoint.
        // The measured pairs retain their original consecutive motion vectors.
        double frameMs=22.22;
        if(k && frames[k]["eyes"][0].fields.count("qpc") && frames[k-1]["eyes"][0].fields.count("qpc")) {
            const double ticks=frames[k]["eyes"][0]["qpc"].number-frames[k-1]["eyes"][0]["qpc"].number;
            frameMs=1000*ticks/double(frequency.QuadPart);
            Check(frameMs>0 && frameMs<250,"invalid same-host interval");
        }
        for(int i=0;i<(k==0?12:1);++i) {
            bool interpolated=false;const bool reset=dispatch==0;
            Check(generator.Generate(gpu.device,gpu.queue,colors,inputs,cameras,settings,frameMs,reset,++dispatch,&interpolated),"generation failed");
            gpu.wait();Check(ActiveBackend()==Backend::Nvidia && interpolated!=reset,"unexpected backend or reset");
        }
        if(!k)continue;
        for(int e=0;e<2;++e) {
            const auto& src=f[k][e];const auto& other=f[k==1?0:2][e];
            const auto middle=hand_motion::Luma(f[1][e].color,src.width,src.height);
            const auto adjacent=hand_motion::Luma(other.color,src.width,src.height);
            const auto pairs=hand_motion::Matches(middle,adjacent,regions[e]);
            const auto pixels=gpu.read(generator.Output(e));
            std::printf("eye %d frames %.0f/%.0f ",e,frames[k-1]["frame"].number,frames[k]["frame"].number);
            const auto mae=hand_motion::ScoreMidpoint(middle[0],adjacent[0],hand_motion::Luma(pixels,src.width,src.height)[0],pairs);
            std::printf("midpoint %zu tracks, 5x5 patch luma MAE %.2f/255; unwarped blend %.2f/255\n",
                pairs.size(),mae.generated,mae.blend);
            const auto r=regions[e];
            int left=r.x,top=r.y,right=r.x+r.w,bottom=r.y+r.h;
            for(const auto& pair:pairs) {
                left=std::min(left,pair.other.x);top=std::min(top,pair.other.y);
                right=std::max(right,pair.other.x+1);bottom=std::max(bottom,pair.other.y+1);
            }
            left=std::max(0,left-12);top=std::max(0,top-12);
            right=std::min(int(src.width),right+12);bottom=std::min(int(src.height),bottom+12);
            const auto name="f"+std::to_string(k)+"_e"+std::to_string(e);
            auto save=[&](const char* suffix,const std::vector<uint8_t>& image) {
                SavePng(out/(name+suffix),image,src.width,src.height,left,top,right-left,bottom-top);
            };
            save("_before.png",f[k-1][e].color);save("_mid.png",pixels);save("_after.png",src.color);
        }
    }
    return 0;
}

int Replay(ReplayGpu& gpu,const std::filesystem::path& dir,const std::filesystem::path& out) {
    using namespace replay;
    const auto text=ReadFile(dir/"capture.json");const std::string json(text.begin(),text.end());
    Parser parser{json};const auto root=parser.Parse();const auto& frames=root["frames"];Check(frames.items.size()==3,"three frames expected");
    EyeFrame f[3][2];
    for(size_t k=0;k<3;++k)for(unsigned e=0;e<2;++e)f[k][e]=LoadEye(dir,frames[int(k)]["eyes"][int(e)],k,e);
    std::filesystem::create_directories(out);
    CoInitializeEx(nullptr,COINIT_MULTITHREADED);
    struct Variant {Backend backend;int flowScale;const char* name;};
    const Variant variants[]={{Backend::FidelityFX,50,"FidelityFX"},{Backend::FidelityFX,0,"FidelityFX no flow"},
        {Backend::Nvidia,50,"NVIDIA OFA + FidelityFX"},{Backend::DlssG,50,"NVIDIA DLSS-G"}};
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
        Generator generator;Settings settings;settings.enabled=true;settings.backend=backend;settings.quality=2;settings.flowScale=variant.flowScale;SetSettings(settings);
        std::vector<uint8_t> results[2];bool ok=true;
        // CVR_REPLAY_WARM=N feeds frames 0,2,0,2... N extra times before the measured 0 -> 2 pair: FidelityFX frame
        // interpolation ignores optical flow for its first 10 dispatches after a reset, so a single pair never shows it.
        const char* warmText=std::getenv("CVR_REPLAY_WARM");
        const int steps=2+2*std::max(0,warmText?std::atoi(warmText):0);
        for(int step=0;step<steps && ok;++step) {
            const size_t k=(step%2)?2:0;const bool last=step==steps-1;
            ComPtr<ID3D12Resource> color[2];ID3D12Resource* colors[2]{};std::shared_ptr<const Inputs> inputs[2];EyePair cameras[2];
            for(unsigned e=0;e<2;++e) {
                const auto& src=f[k][e];
                std::vector<float> vectors=k?Compose(src,f[1][e]):src.motion;
                color[e]=UploadEye(gpu,src,vectors,inputs[e],k!=0,uint32_t(frames[int(k)]["frame"].number));
                colors[e]=color[e].Get();
                cameras[e].previous=f[0][e].eye;cameras[e].current=f[k][e].eye;
            }
            bool interpolated=false;
            ok=generator.Generate(gpu.device,gpu.queue,colors,inputs,cameras,settings,44.4,step==0,uint64_t(step+1),&interpolated);
            gpu.wait();
            if(ok && last) {
                ok=interpolated && ActiveBackend()==backend;
                for(unsigned e=0;e<2 && ok;++e) {
                    auto pixels=gpu.read(generator.Output(e));
                    const EyeCamera shown=Middle(f[0][e].eye,f[2][e].eye);   // what the presenter submits
                    results[e]=Reproject(pixels,w,h,shown,f[1][e].eye,nullptr);
                }
            }
        }
        if(ok) {
            uint32_t fw{},fh{};const auto flow=generator.ReadFlow(0,&fw,&fh);
            size_t nonzero=0;double sum=0,peak=0;
            for(size_t i=0;i+1<flow.size();i+=2) {const double m=std::hypot(double(flow[i]),double(flow[i+1]));if(m>0)++nonzero;sum+=m;peak=std::max(peak,m);}
            if(!flow.empty()) {
                std::string fname=variant.name;for(auto& c:fname)if(!std::isalnum(uint8_t(c)))c='_';
                std::ofstream(out/(fname+"_flow_e0.raw"),std::ios::binary).write(reinterpret_cast<const char*>(flow.data()),std::streamsize(flow.size()*2));
            }
            if(!flow.empty())std::printf("  %-26s flow eye 0 %ux%u: nonzero %.1f%%, mean |v| %.2f, max %.1f (raw units)\n",variant.name,fw,fh,
                100.0*double(nonzero)/double(flow.size()/2),sum/double(flow.size()/2),peak);
        }
        if(!ok) {std::printf("  %-26s FAILED (active %s)\n",variant.name,BackendName(ActiveBackend()));continue;}
        report(variant.name,results);
        std::string name=variant.name;for(auto& c:name)if(!std::isalnum(uint8_t(c)))c='_';
        SavePng(out/(name+"_e0.png"),results[0],w,h,cx,cy,cw,ch);
        // error map: |output - truth| x4, red where the pixel is in the moving region
        std::vector<uint8_t> errors(results[0].size());
        for(size_t i=0;i<valid[0].size();++i) {
            int d=0;for(int k=0;k<3;++k)d=std::max(d,std::abs(int(results[0][i*4+k])-int(f[1][0].color[i*4+k])));
            const uint8_t v=uint8_t(std::min(255,d*4));errors[i*4]=v;errors[i*4+1]=motion[0][i]?v/2:v;errors[i*4+2]=motion[0][i]?v/2:v;errors[i*4+3]=255;
        }
        SavePng(out/(name+"_error_e0.png"),errors,w,h,cx,cy,cw,ch);
        while(!generator.Reset())gpu.wait();
    }
    return 0;
}
