#include "Utils/DebugGate.hpp"
#include "Camera/ImagePoseIdentity.hpp"
#include <atomic>
#include <memory>
#include <new>
#include <cstddef>
#include <mutex>

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdImageWrites[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdCaptured[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdCaptureMiss[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdLastCapture[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdLastSubmit[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdImageGeneration[2]{};
#endif
#if !CVR_RUST_VRIK
}
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE LEDGER IS RUST'S (rust/crates/cvr-rig/src/vrik/image_pose.rs over rust/crates/cvr-camera/src/image_ledger.rs):
// the images' pose labels and the lifetime records on the lists and resources behind their addresses. This file keeps
// the exports above, the submission mutex its C++ callers lock and the C++ names.
extern "C" {
}
namespace cvr::camera {
static_assert(sizeof(PoseIdentity) == 136 && sizeof(ImagePoseIdentity) == 168 && offsetof(ImagePoseIdentity, pose) == 32,
              "ImagePoseIdentity is mirrored in rust/crates/cvr-camera/src/image_ledger.rs");
extern "C" {
}
}  // namespace cvr::camera
#endif
#else
namespace cvr::camera {
namespace {
using Ledger=ImagePoseLedger<PoseIdentity>;
std::shared_ptr<Ledger> s_images=std::make_shared<Ledger>();
std::recursive_mutex s_submission;
std::mutex s_lifetimeMutex;
// Private COM data is released with the actual object. Recycling a resource or
// command-list address therefore cannot inherit another object's image label.
const GUID kPoseLifetime={0x8d1748d9,0x3642,0x43e5,{0xa2,0x41,0x9b,0xb6,0x90,0x51,0x47,0xe3}};
class Lifetime final:public IUnknown {
    std::atomic<ULONG> m_refs{1};
    std::weak_ptr<Ledger> m_owner;
    uintptr_t m_address;
    bool m_list;
public:
    Lifetime(uintptr_t address,bool list):m_owner(s_images),m_address(address),m_list(list) {}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid,void** out) override {
        if(!out)return E_POINTER;*out=nullptr;
        if(iid!=__uuidof(IUnknown))return E_NOINTERFACE;
        *out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++m_refs; }
    ULONG STDMETHODCALLTYPE Release() override {
        const auto left=--m_refs;
        if(!left) {
            if(const auto owner=m_owner.lock()) {
                if(m_list)owner->ForgetList(m_address);else owner->ForgetResource(m_address);
            }
            delete this;
        }
        return left;
    }
};
bool TrackLifetime(ID3D12Object* object,bool list) {
    if(!object)return false;
    IUnknown* existing=nullptr;
    Lifetime* created=nullptr;
    HRESULT result=E_FAIL;
    {
        std::lock_guard lock(s_lifetimeMutex);
        UINT size=sizeof(existing);
        result=object->GetPrivateData(kPoseLifetime,&size,&existing);
        if(FAILED(result) || !existing) {
            created=new(std::nothrow) Lifetime(reinterpret_cast<uintptr_t>(object),list);
            result=created ? object->SetPrivateDataInterface(kPoseLifetime,created) : E_OUTOFMEMORY;
        }
    }
    if(existing)existing->Release();
    if(created)created->Release();
    return SUCCEEDED(result);
}
}
std::recursive_mutex& ImageSubmissionMutex() { return s_submission; }
void ResetImagePoseList(ID3D12GraphicsCommandList* list) {
    if(TrackLifetime(list,true))s_images->Reset(reinterpret_cast<uintptr_t>(list));
}
void RecordImagePose(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,const PoseIdentity& pose,bool stable) {
    if(!TrackLifetime(list,true) || !TrackLifetime(resource,false))return;
    s_images->Record(reinterpret_cast<uintptr_t>(list),reinterpret_cast<uintptr_t>(resource),pose,stable);
    if(pose.view>=1 && pose.view<=2)CVR_DIAGNOSTIC(CyberpunkVR_PoseIdImageWrites[pose.view-1].fetch_add(1,std::memory_order_relaxed));
}
void CommitImagePoseLists(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    if(!queue || !lists)return;
    for(UINT i=0;i<count;++i)s_images->Submit(reinterpret_cast<uintptr_t>(queue),reinterpret_cast<uintptr_t>(lists[i]),GetTickCount64());
}
ImagePoseIdentity ReadImagePose(ID3D12Resource* resource) { return s_images->Read(reinterpret_cast<uintptr_t>(resource)); }
ID3D12Resource* LatestSubmittedVrcamImage() {
    return reinterpret_cast<ID3D12Resource*>(s_images->LatestStable().resource);
}
}
#endif
