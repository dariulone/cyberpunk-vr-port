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
    Microsoft::WRL::ComPtr<ID3D12Fence> colorReady;
    uint64_t colorReadyValue{};
    std::shared_ptr<const CaptureTiming> captureTiming;
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
    Selection Select(ID3D12Device*,ID3D12CommandQueue*,Frame,bool gameplay,ID3D12CommandQueue* consumer=nullptr);
    bool WantsPrepare(uint64_t serial) const {return primed && !prepared.frame.identity.serial && serial>generatedSerial;}
    bool Prepare(ID3D12Device*,ID3D12CommandQueue*,Frame,ID3D12CommandQueue* consumer);
    void Submitted(const Selection&,ID3D12Fence*,uint64_t,bool accepted);
    void Reset();
    void Shutdown();
    bool Pending() const {return cadence.Pending()!=0;}
private:
    Cadence cadence;
    Generator generator;
    Frame pending;
    Selection prepared;
    Frame preparedEndpoint;
    Settings preparedSettings;
    double preparedAt{};
    void Remember(const Frame&);

    Frame previous;
    bool primed{};
    uint64_t generatedSerial{},sdkFrame{};
};
}
