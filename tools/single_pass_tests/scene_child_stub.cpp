#include "Render/NativeSceneChild.hpp"

#if !CVR_PROBE_EXPORTS   // native_probe_test_rust's cvr_rust (cvr-ffi's probe-test) defines it
extern "C" {__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneChildEnable{};}
#endif
namespace cvr::stereo::scene_child {
Scope::Scope(uint64_t,uint64_t,uint32_t)noexcept{}
Scope::~Scope()=default;
bool DescribeCurrent(Identity& out)noexcept{out={};return false;}
bool DescribeCurrentAtList(ID3D12GraphicsCommandList*,Identity& out,native_recording::Lease& lease)noexcept{out={};lease={};return false;}
}
