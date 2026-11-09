#pragma once
#include "Runtimes/StereoDepthPolicy.hpp"
#include <memory>

namespace cvr::depth {
// Owned input snapshots are held until the SUBMIT fence, not just their producer
// fence. Each in-flight list has independent descriptors and output textures.
class Renderer {
public:
    Renderer();
    ~Renderer();
    bool Record(ID3D12Device*,ID3D12GraphicsCommandList*,ID3D12CommandQueue*,
        const std::shared_ptr<const framegen::Inputs> (&inputs)[2],
        ID3D12Resource* const (&destinations)[2],const Plan&,unsigned slot,
        ID3D12Fence* submitFence,uint64_t submitValue,bool crossQueue=false);
    bool Idle() const;
    void Poll();
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
}
