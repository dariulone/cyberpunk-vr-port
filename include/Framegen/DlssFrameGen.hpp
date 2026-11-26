#pragma once
#include "Framegen/Inputs.hpp"
#include <d3d12.h>
#include <cstdint>
#include <memory>

namespace cvr::framegen::dlssg {
// Native NVIDIA DLSS Frame Generation through NGX, without Streamline and without a swapchain: one
// feature per eye, fed the eye's final colour and the engine's own depth, motion vectors and camera.
// NGX keeps the previous colour itself, so no history copy exists on our side. The generated image
// is the midpoint of the two real frames in the midpoint camera.
//
// Built only when the NVIDIA DLSS SDK was found at configure time (CVR_NATIVE_DLSSG); otherwise
// Available() reports why and the generator falls back to engine vectors.
bool Available(ID3D12Device*);
const char* Reason();

struct Feature;
struct FeatureDeleter {void operator()(Feature*) const;};
using FeaturePtr=std::unique_ptr<Feature,FeatureDeleter>;
// Records NGX's creation work into the list; the feature is usable once that list has executed.
FeaturePtr Create(ID3D12GraphicsCommandList*,uint32_t width,uint32_t height,uint32_t renderWidth,uint32_t renderHeight,
    DXGI_FORMAT backbuffer,DXGI_FORMAT motion,DXGI_FORMAT depth);
uint64_t Bytes(const Feature&);

struct Evaluation {
    ID3D12Resource *backbuffer{},*depth{},*motion{},*output{};
    uint32_t width{},height{},depthWidth{},depthHeight{},motionWidth{},motionHeight{};
    const CameraData* camera{};   // this frame
    const CameraData* previous{}; // the frame NGX interpolates from; null on a reset
    bool reset{};
};
// Inputs in NON_PIXEL_SHADER_RESOURCE, output in UNORDERED_ACCESS. NGX binds its own heaps and
// root signature, so the caller re-binds its own after this.
bool Evaluate(Feature&,ID3D12GraphicsCommandList*,const Evaluation&);

// Row-vector (Streamline/NGX) camera matrices built from the engine camera: view->clip,
// clip->view and both directions between the previous and current clip spaces.
struct Matrices {float viewToClip[16],clipToView[16],clipToPrevClip[16],prevClipToClip[16];};
bool BuildMatrices(const CameraData& current,const CameraData* previous,Matrices* out);
}
