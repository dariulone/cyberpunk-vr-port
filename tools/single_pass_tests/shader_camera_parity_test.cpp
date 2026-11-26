// The peer shader camera from Rust (shader_camera.rs) against the reference C++ rules, byte for byte, on deterministic
// synthetic cameras: valid ones, each rule's refusal, and history decodes. Built without CVR_RUST_VRIK so the header
// keeps the reference; the Rust staticlib supplies cvr_scam_*. shader_camera_test needs a native recording; this does not.
#include "Render/StereoShaderCamera.hpp"
#include <DirectXMath.h>
#include <cstdio>
#include <random>
#include <stdexcept>

using namespace cvr::stereo;
extern "C" {
bool cvr_scam_build(const ShaderCamera*,const uint8_t*,const uint8_t*,size_t,float,InvertShaderMatrix,ShaderCamera*);
bool cvr_scam_decode(const uint8_t*,size_t,const ShaderCameraHistory*,uint8_t*,float*);
}
static void* Invert(const void* source,void* destination) {
    const auto input=DirectX::XMLoadFloat4x4(static_cast<const DirectX::XMFLOAT4X4*>(source));
    DirectX::XMStoreFloat4x4(static_cast<DirectX::XMFLOAT4X4*>(destination),DirectX::XMMatrixInverse(nullptr,input));
    return destination;
}
static void Require(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
namespace {
std::mt19937 random(0x5EED);
float Uniform(float low,float high){return std::uniform_real_distribution<float>(low,high)(random);}
uint32_t Integer(uint32_t low,uint32_t high){return std::uniform_int_distribution<uint32_t>(low,high)(random);}
template<class T>void Put(uint8_t* bytes,size_t at,T value){std::memcpy(bytes+at,&value,sizeof(value));}
void Matrix(uint8_t* bytes,size_t at,float scale){for(unsigned i=0;i<16;++i)Put(bytes,at+i*4,Uniform(-scale,scale));}
CameraInput Camera(uint32_t width,uint32_t height){
    CameraInput camera;for(auto& byte:camera.bytes)byte=uint8_t(Integer(0,255));
    for(unsigned i=0;i<3;++i)Put(camera.bytes.data(),i*4,int32_t(Integer(0,1u<<26))-(1<<25));
    Put(camera.bytes.data(),0x20,Uniform(0.1f,3.0f));Put(camera.bytes.data(),0x40,Uniform(0.01f,1.0f));
    Put(camera.bytes.data(),0x44,Uniform(1.5f,1000.0f));
    for(const size_t at:{size_t(0x50),size_t(0x2F0),size_t(0x2B0),size_t(0x110)})Matrix(camera.bytes.data(),at,4.0f);
    Put(camera.bytes.data(),0x370,Uniform(-0.5f,0.5f));Put(camera.bytes.data(),0x374,Uniform(-0.5f,0.5f));
    Put(camera.bytes.data(),0x378,width);Put(camera.bytes.data(),0x37C,height);camera.bytes[0x384]|=4;
    return camera;
}
}
int main() try {
    unsigned built{},refused{},decoded{};
    for(unsigned sample=0;sample<20000;++sample){
        const auto width=Integer(1,16384),height=Integer(1,16384);
        ShaderCamera shared;for(auto& byte:shared)byte=uint8_t(Integer(0,255));
        auto current=Camera(width,height),previous=Camera(width,height);
        float weight=Uniform(0.0f,1.0f);
        // Every eighth sample breaks one rule.
        if(sample%8==7)switch(Integer(0,9)){
            case 0:Put(current.bytes.data(),0x378,0u);break;
            case 1:Put(previous.bytes.data(),0x37C,height+1);break;
            case 2:weight=-1.0f;break;
            case 3:Put(current.bytes.data(),0x20,0.0f);break;
            case 4:Put(current.bytes.data(),0x44,0.0f);break;
            case 5:current.bytes[0x384]&=~4;break;
            case 6:Put(current.bytes.data(),0x50+20,std::numeric_limits<float>::infinity());break;
            case 7:Put(previous.bytes.data(),0,int32_t(INT32_MIN));Put(current.bytes.data(),0,int32_t(INT32_MAX));break;
            case 8:for(unsigned i=0;i<16;++i)Put(current.bytes.data(),0x2B0+i*4,0.0f);break;
            default:Put(current.bytes.data(),0x370,std::numeric_limits<float>::quiet_NaN());break;
        }
        ShaderCamera reference{},rust{};
        const bool a=BuildPeerShaderCamera(shared,current,previous,weight,Invert,reference);
        const bool b=cvr_scam_build(&shared,current.bytes.data(),previous.bytes.data(),current.bytes.size(),weight,Invert,&rust);
        Require(a==b,"Admission differs");
        if(a){Require(reference==rust,"Shader camera bytes differ");++built;}else{Require(rust==ShaderCamera{},"Refusal wrote output");++refused;}
        ShaderCameraHistory history;for(auto& byte:history.bytes)byte=uint8_t(Integer(0,255));
        if(sample%3){Put(history.bytes.data(),0x180,Uniform(0.0f,2.0f));Matrix(history.bytes.data(),0x30,4.0f);Matrix(history.bytes.data(),0xB0,4.0f);
            if(sample%5){Put(history.bytes.data(),0xB0+32,0.0f);Put(history.bytes.data(),0xB0+36,0.0f);}}
        CameraInput first=previous,second=previous;float w1=-7.0f,w2=-7.0f;
        const bool c=DecodeShaderCameraHistory(current,history,first,w1);
        CameraInput copy=current;const bool d=cvr_scam_decode(current.bytes.data(),current.bytes.size(),&history,copy.bytes.data(),&w2);
        if(d)second=copy;
        Require(c==d && std::memcmp(&w1,&w2,4)==0 && first.bytes==second.bytes,"History decode differs");
        decoded+=c;
    }
    std::printf("PASS shader camera parity: %u built, %u refused, %u histories decoded, all bytes equal\n",built,refused,decoded);
    return 0;
} catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
