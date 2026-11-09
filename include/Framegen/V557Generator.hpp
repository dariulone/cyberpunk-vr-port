#pragma once
#include "Framegen/Generator.hpp"

namespace cvr::framegen {
// Reconstruction only. The native Presenter still owns cadence and XR submission.
class V557Generator {
public:
    V557Generator();
    ~V557Generator();
    bool Compatible(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource* const[2],const Settings&) const;
    bool Create(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource* const[2],const Settings&);
    bool Generate(ID3D12Resource* const[2],const EyePair (&)[2],bool reset,bool* interpolated,
                  ID3D12Fence*,uint64_t,std::shared_ptr<void>,const std::shared_ptr<const Inputs> (&)[2],std::shared_ptr<const CaptureTiming>);
    bool CanPrepare() const;
    bool OutputReady() const;
    bool Idle() const;
    bool Failed() const;
    void Poll();
    bool JoinOutput(ID3D12CommandQueue*);
    void ConsumerFence(ID3D12Fence*,uint64_t);
    ID3D12Resource* Output(unsigned) const;
    uint64_t Bytes() const;
    void Shutdown();
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
}
