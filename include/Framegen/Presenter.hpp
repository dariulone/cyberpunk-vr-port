#pragma once
#include "Framegen/FramePolicy.hpp"
#include "Framegen/Generator.hpp"
#include <openxr/openxr.h>

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
    void Submitted(const Selection&,ID3D12Fence*,uint64_t,bool accepted,bool retainEndpoint=false);
    void Reset();
    bool Pending() const {return cadence.Pending()!=0;}
    const Frame& PendingFrame() const {return pending;}
private:
    Cadence cadence;
    Generator generator;
    Frame pending;
    Frame previous;
    bool primed{};
    uint64_t generatedSerial{},sdkFrame{};
};
}
