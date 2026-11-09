#pragma once
#include "Framegen/CaptureTiming.hpp"
#include <array>
namespace cvr::framegen {
struct SourceFrame;
struct SourceRecord {
    uint32_t frame{},mask{};
    uint64_t origin{},pose[2]{};
    bool rr{};
    // Depth, motion, per-eye upscale-return/copyback. Not final postprocess color.
    double end[2][3]{},recorded[2][3]{},submitted[2][3]{};
    double captureEnqueue{},captureStart{},captureEnd{},generationEnqueue{},generationStart{};
};
namespace source_timing {
enum class Stage:unsigned {Depth,Motion,Upscale};
bool Wanted(uint32_t frame);
void Record(ID3D12GraphicsCommandList*,uint32_t frame,uint64_t pose,uint64_t origin,unsigned eye,Stage,bool rr=false);
void Submitted(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void ResetList(ID3D12GraphicsCommandList*);
std::shared_ptr<const SourceFrame> Acquire(uint32_t frame,const uint64_t (&pose)[2],uint64_t origin,ID3D12CommandQueue*);
std::optional<SourceRecord> Read(const std::shared_ptr<const SourceFrame>&,const CaptureTiming::Result&,double generationEnqueue,double generationStart);
void Report(const std::shared_ptr<const SourceFrame>&,const CaptureTiming::Result&,double generationEnqueue,double generationStart);
}
}
