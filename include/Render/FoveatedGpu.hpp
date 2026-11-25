#pragma once
#include "Render/FoveatedDlss.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <vector>

namespace cvr::foveated {
struct Source {
    ID3D12Resource* color{}, *depth{}, *motion{}, *output{};
    unsigned renderW{}, renderH{}, outputW{}, outputH{};
    unsigned colorX{}, colorY{}, depthX{}, depthY{}, motionX{}, motionY{};
    float motionScaleX{}, motionScaleY{};
    bool lowMotion{}, inverted{};
};
struct PreparedRegion {
    Region region;
    Microsoft::WRL::ComPtr<ID3D12Resource> color, depth, motion, output;
};
class FoveatedGpu {
    using Heap = Microsoft::WRL::ComPtr<ID3D12DescriptorHeap>;
    struct Binding {
        std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,4> sources;
        Heap heap;
    };
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> prepare, composite;
    std::vector<Binding> bindings;
    unsigned stride{};
    Binding* Bind(const Source&);
public:
    PreparedRegion center, periphery;
    bool Initialize(ID3D12Device*,const Region&,const Region&);
    bool Prepare(ID3D12GraphicsCommandList*,const Source&,bool central,float cropDeltaX=0.0f,float cropDeltaY=0.0f);
    bool Composite(ID3D12GraphicsCommandList*,const Source&,bool boundary);
};
}
