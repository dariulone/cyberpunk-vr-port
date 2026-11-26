#pragma once
#include <cstdint>
#include <atomic>
#include <array>
#include "Render/CameraWordMask.hpp"

namespace cvr::stereo::trace {
struct Record {
    uint64_t sequence{},qpc{},object{};
    uint32_t frame{},node{},kind{},index{};
    int32_t side{};
    uint32_t thread{};
    std::array<uint8_t,848> camera{};
    uint64_t context{},view{},cameraAddress{};
    uint32_t width{},height{},inputValid{},reserved{};
    std::array<uint8_t,904> input{};
    std::array<uint8_t,16> clip{};
    uint32_t predictionValid{},predictionCounter{};
    std::array<uint8_t,904> prediction{};
    uint32_t shaderPredictionValid{},previousInputValid{};
    std::array<uint8_t,848> shaderPrediction{};
    std::array<uint8_t,904> previousInput{};
    CameraWordMask validShaderWords{};
    std::array<uint8_t,32> peerSourcePose{};
    uint64_t twinCpuDescriptor{},twinGpuAddress{};
    uint32_t twinBytes{},predictionPeerSide{};
};
static_assert(sizeof(Record)==4616);
constexpr unsigned Capacity=512;
// Opt-in CPU trace for preparing a common geometry pass. No GPU work, files or
// resource references. Camera bytes are copied only for the requested frames.
using Predict=void(*)(Record&);
// The camera builder's inputs, read by unwinding this thread's stack under SEH (C++ in both builds: the guarded read).
void CaptureNativeInputs(Record&);
#if CVR_RUST_VRIK
}
// The trace: rust/crates/cvr-render/src/single_pass_trace.rs.
extern "C" {
void cvr_sp_trace_frame_boundary();
void cvr_sp_trace_camera(uint32_t node,int side,uint32_t index,const void* bytes,cvr::stereo::trace::Predict predict);
void cvr_sp_trace_list(uint32_t node,int side,const void* list);
}
namespace cvr::stereo::trace {
inline void FrameBoundary(){cvr_sp_trace_frame_boundary();}
inline void Camera(uint32_t node,int side,uint32_t index,const void* bytes,Predict predict=nullptr){cvr_sp_trace_camera(node,side,index,bytes,predict);}
inline void List(uint32_t node,int side,const void* list){cvr_sp_trace_list(node,side,list);}
#else
void FrameBoundary();
void Camera(uint32_t node,int side,uint32_t index,const void* bytes,Predict predict=nullptr);
void List(uint32_t node,int side,const void* list);
#endif
}
extern "C" {
__declspec(dllexport) extern uint32_t CyberpunkVR_SinglePassTraceRecordBytes;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassTraceRequest;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassTraceState; // 0 idle, 1 recording, 2 done, 3 full
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassTraceSeq;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassTraceCount;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassTraceNodeFilter;
__declspec(dllexport) extern cvr::stereo::trace::Record CyberpunkVR_SinglePassTraceRecords[cvr::stereo::trace::Capacity];
}
