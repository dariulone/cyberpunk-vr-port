#pragma once
#include "Camera/PoseIdentity.hpp"
#include "Camera/ImagePoseLedger.hpp"
extern "C" {
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_PoseIdCaptured[2];
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_PoseIdCaptureMiss[2];
// Last-capture/submit arrays use physical XR eye indices, not MAIN/VRCAM roles.
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_PoseIdLastCapture[2];
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_PoseIdLastSubmit[2];
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_PoseIdImageGeneration[2];
}

namespace cvr::camera {
using ImagePoseIdentity=ImagePoseLedger<PoseIdentity>::Image;
#if CVR_RUST_VRIK
// The image submission lock is Rust's (rust/crates/cvr-host/src/submission.rs): recursive, taken through lock_guard or
// lock()/unlock() as the std::recursive_mutex it replaces.
struct ImageSubmissionLock { void lock(); void unlock(); };
ImageSubmissionLock& ImageSubmissionMutex();
#else
std::recursive_mutex& ImageSubmissionMutex();
#endif
void ResetImagePoseList(ID3D12GraphicsCommandList* list);
void RecordImagePose(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,
                     const PoseIdentity& pose,bool stable=false);
void CommitImagePoseLists(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists);
ImagePoseIdentity ReadImagePose(ID3D12Resource* resource);
ID3D12Resource* LatestSubmittedVrcamImage(); // borrowed; stable images have the capture owner's lifetime
}
