#pragma once
#include "Render/FoveatedDlss.hpp"
#include <d3d12.h>
#include <wrl/client.h>
#include <array>
#include <utility>
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
static_assert(sizeof(Source) == 88, "mirrored by rust/crates/cvr-gpu/src/foveated_gpu.rs");
#if CVR_RUST_VRIK
}
// The passes: rust/crates/cvr-gpu/src/foveated_gpu.rs.
extern "C" {
void* cvr_foveated_gpu_new();
void cvr_foveated_gpu_free(void* gpu);
bool cvr_foveated_gpu_initialize(const void* gpu, ID3D12Device* device, cvr::foveated::Region center, cvr::foveated::Region periphery);
bool cvr_foveated_gpu_prepare(const void* gpu, ID3D12GraphicsCommandList* list, const cvr::foveated::Source* source, bool central);
bool cvr_foveated_gpu_composite(const void* gpu, ID3D12GraphicsCommandList* list, const cvr::foveated::Source* source, bool boundary);
void cvr_foveated_gpu_prepared(const void* gpu, bool central, cvr::foveated::Region* region, ID3D12Resource** resources);
}
namespace cvr::foveated {
class FoveatedGpu {
public:
    FoveatedGpu() : m_rust(cvr_foveated_gpu_new()) {}
    ~FoveatedGpu() { cvr_foveated_gpu_free(m_rust); }
    FoveatedGpu(const FoveatedGpu&) = delete;
    FoveatedGpu& operator=(const FoveatedGpu&) = delete;
    // The passes' regions and resources, as Rust made them (read after Initialize).
    PreparedRegion center, periphery;
    bool Initialize(ID3D12Device* device, const Region& c, const Region& p) {
        const bool ok = cvr_foveated_gpu_initialize(m_rust, device, c, p);
        for (auto [pass, central] : {std::pair{&center, true}, std::pair{&periphery, false}}) {
            ID3D12Resource* resources[4]{};
            cvr_foveated_gpu_prepared(m_rust, central, &pass->region, resources);
            pass->color.Attach(resources[0]); pass->depth.Attach(resources[1]);
            pass->motion.Attach(resources[2]); pass->output.Attach(resources[3]);
        }
        return ok;
    }
    bool Prepare(ID3D12GraphicsCommandList* list, const Source& s, bool central) { return cvr_foveated_gpu_prepare(m_rust, list, &s, central); }
    bool Composite(ID3D12GraphicsCommandList* list, const Source& s, bool boundary) { return cvr_foveated_gpu_composite(m_rust, list, &s, boundary); }
private:
    void* m_rust;
};
#else
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
    bool Prepare(ID3D12GraphicsCommandList*,const Source&,bool central);
    bool Composite(ID3D12GraphicsCommandList*,const Source&,bool boundary);
};
#endif
}
