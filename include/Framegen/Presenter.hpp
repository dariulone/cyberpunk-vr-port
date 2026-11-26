#pragma once
#include "Framegen/FramePolicy.hpp"
#include "Framegen/Generator.hpp"
#include <openxr/openxr.h>
#include <cstddef>

#if CVR_RUST_VRIK
extern "C" {
// The frame loop's newest captured pair (rust/crates/cvr-render/src/framegen_presenter.rs CapturedSelectIn): the main
// eye's picture and the second eye's, the eyes' poses, FOVs, labels, the serial and capture time (written back with the
// selected frame's), the frame generator's inputs references (handed over).
struct CvrXrFramegenSelectIn {
    ID3D12Device* device;
    ID3D12CommandQueue* queue;
    ID3D12Fence* submitFence;
    ID3D12Resource* color;
    ID3D12Resource* second;
    XrPosef* poses;
    XrFovf* fovs;
    uint64_t* poseIds;
    uint64_t* serial;
    double* capturedMs;
    const cvr::framegen::Inputs* inputs[2];
    uint64_t origin;
    uint32_t viewCount, mainEye;
    bool gameplay;
};
// What the frame loop gets back of the cycle's selection (SelectedOut): the pictures it shows, synthetic or not.
struct CvrXrFramegenSelected {
    ID3D12Resource* color[2];
    bool synthetic;
};
// The cycle's selection (cvr_xr_framegen_select): false without a pair (the presenter reset).
bool cvr_xr_framegen_select(const CvrXrFramegenSelectIn* input, CvrXrFramegenSelected* out);
}
static_assert(sizeof(CvrXrFramegenSelectIn) == 120 && offsetof(CvrXrFramegenSelectIn, inputs) == 80 &&
              offsetof(CvrXrFramegenSelectIn, gameplay) == 112,
              "mirrored by CapturedSelectIn in rust/crates/cvr-render/src/framegen_presenter.rs");
static_assert(sizeof(CvrXrFramegenSelected) == 24, "mirrored by SelectedOut in rust/crates/cvr-render/src/framegen_presenter.rs");
namespace cvr::framegen {
// The presenter is Rust's (rust/crates/cvr-render/src/framegen_presenter.rs), the one there is, the XR thread's: the frame
// loop hands it its captured pair (cvr_xr_framegen_select), the cycle's end (Rust's) tells it the submission. This face is
// OpenXRManager's member for the cycle that ends without its copies and for the shutdown.
class Presenter {
public:
    void Submitted(ID3D12Fence*,uint64_t,bool accepted);
    void Reset();
    bool Pending() const;
};
}
#else
namespace cvr::framegen {
struct ReadLease {
    using Release=void (*)(void*,ID3D12Resource*,uint64_t);
    void* owner{};Release release{};
    ID3D12Resource* resources[2]{};
    uint64_t fence{};
    ~ReadLease() {if(release)for(auto* resource:resources)if(resource)release(owner,resource,fence);}
};
struct Frame {
    Identity identity;
    ID3D12Resource* color[2]{};
    XrPosef pose[2]{};
    XrFovf fov[2]{};
    std::shared_ptr<const Inputs> inputs[2];
    std::shared_ptr<ReadLease> lease;
};
struct Selection {
    Frame frame;
    Cadence::Next kind=Cadence::Next::Real;
    // Sources read by generation even when its displayed image is synthetic.
    std::shared_ptr<ReadLease> generatedFrom;
    bool synthetic=false;
};
class Presenter {
public:
    Selection Select(ID3D12Device*,ID3D12CommandQueue*,Frame,bool gameplay);
    void Submitted(const Selection&,ID3D12Fence*,uint64_t,bool accepted);
    void Reset();
    bool Pending() const {return cadence.Pending()!=0;}
private:
    Cadence cadence;
    Generator generator;
    Frame pending;
    Frame previous;
    bool primed{};
    uint64_t generatedSerial{},sdkFrame{};
};
}
#endif
