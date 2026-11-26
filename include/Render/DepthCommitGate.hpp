#pragma once
#include "Render/NativeStereoProbe.hpp"
#include <wrl/client.h>
#include <span>

namespace cvr::stereo {
// Caller serializes recording/admission and only reads after its queue fence.
// The GPU also rejects admission when native initial depth/stencil differs.
class DepthCommitGate {
public:
    static constexpr UINT MaxBufferProofs=256;
    // Owned, immutable snapshots in NON_PIXEL_SHADER_RESOURCE state. The gate
    // compares every byte in each declared row before predicated work executes.
    // Zero row fields mean one contiguous DWORD-aligned range. Texture snapshots
    // may specify row layout to exclude uninitialized copy-footprint padding.
    struct BufferProof {
        ID3D12Resource* main{};ID3D12Resource* peer{};UINT bytes{};
        UINT rowBytes{},rowPitch{},rows{};
        UINT mainOffset{},peerOffset{};
    };
#if CVR_RUST_VRIK
    // The gate: rust/crates/cvr-gpu/src/depth_commit_gate.rs (StereoSceneState's bindings attached from
    // src/Render/DepthCommitGate.cpp).
    DepthCommitGate();
    ~DepthCommitGate();
    DepthCommitGate(const DepthCommitGate&)=delete;
    DepthCommitGate& operator=(const DepthCommitGate&)=delete;
    HRESULT Create(ID3D12Device*,ID3D12Resource* initialDepthArray);
    bool Prepare(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const BufferProof* instances=nullptr){
        return Prepare(list,draw,instances?std::span(instances,1):std::span<const BufferProof>{});
    }
    bool Prepare(ID3D12GraphicsCommandList*,const native_probe::DrawRecord&,std::span<const BufferProof>);
    bool Decide(bool allow);
    void Freeze();
    bool Bind(ID3D12GraphicsCommandList*,bool optimized)const;
    bool Readback(ID3D12GraphicsCommandList*);
    HRESULT Read(uint64_t& value)const;
    bool Restart();
private:
    void* rust_;
#else
    DepthCommitGate();
    ~DepthCommitGate();
    HRESULT Create(ID3D12Device*,ID3D12Resource* initialDepthArray);
    bool Prepare(ID3D12GraphicsCommandList*,const native_probe::DrawRecord&,const BufferProof* instances=nullptr);
    bool Prepare(ID3D12GraphicsCommandList*,const native_probe::DrawRecord&,std::span<const BufferProof>);
    bool Decide(bool allow);
    void Freeze();
    bool Bind(ID3D12GraphicsCommandList*,bool optimized)const;
    bool Readback(ID3D12GraphicsCommandList*);
    HRESULT Read(uint64_t& value)const;
    // The owning pass must prove that every previous GPU user has completed.
    bool Restart();
private:
    struct Storage;
    Microsoft::WRL::ComPtr<Storage> storage_;
#endif
};
static_assert(sizeof(DepthCommitGate::BufferProof)==40,"mirrored by rust/crates/cvr-gpu/src/depth_commit_gate.rs");
#if CVR_RUST_VRIK
}
extern "C" {
void* cvr_depth_gate_new();
void cvr_depth_gate_free(void*);
HRESULT cvr_depth_gate_create(void*,ID3D12Device*,ID3D12Resource*);
bool cvr_depth_gate_prepare(void*,ID3D12GraphicsCommandList*,uint32_t flags,ID3D12PipelineState*,
    const cvr::stereo::DepthCommitGate::BufferProof*,size_t);
bool cvr_depth_gate_decide(void*,bool);
void cvr_depth_gate_freeze(void*);
bool cvr_depth_gate_bind(void*,ID3D12GraphicsCommandList*,bool);
bool cvr_depth_gate_readback(void*,ID3D12GraphicsCommandList*);
HRESULT cvr_depth_gate_read(void*,uint64_t*);
bool cvr_depth_gate_restart(void*);
}
namespace cvr::stereo {
inline DepthCommitGate::DepthCommitGate():rust_(cvr_depth_gate_new()){}
inline DepthCommitGate::~DepthCommitGate(){cvr_depth_gate_free(rust_);}
inline HRESULT DepthCommitGate::Create(ID3D12Device* device,ID3D12Resource* initial){return cvr_depth_gate_create(rust_,device,initial);}
inline bool DepthCommitGate::Prepare(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,std::span<const BufferProof> proofs){
    return cvr_depth_gate_prepare(rust_,list,draw.flags,reinterpret_cast<ID3D12PipelineState*>(draw.pipeline),proofs.data(),proofs.size());
}
inline bool DepthCommitGate::Decide(bool allow){return cvr_depth_gate_decide(rust_,allow);}
inline void DepthCommitGate::Freeze(){cvr_depth_gate_freeze(rust_);}
inline bool DepthCommitGate::Bind(ID3D12GraphicsCommandList* list,bool optimized)const{return cvr_depth_gate_bind(rust_,list,optimized);}
inline bool DepthCommitGate::Readback(ID3D12GraphicsCommandList* list){return cvr_depth_gate_readback(rust_,list);}
inline HRESULT DepthCommitGate::Read(uint64_t& value)const{return cvr_depth_gate_read(rust_,&value);}
inline bool DepthCommitGate::Restart(){return cvr_depth_gate_restart(rust_);}
#endif
}
