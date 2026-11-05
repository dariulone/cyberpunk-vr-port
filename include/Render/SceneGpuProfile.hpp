#pragma once
#include <d3d12.h>
#include <atomic>
#include <memory>
#include <vector>

namespace cvr::gpu::scene_profile {
struct Batch;
using Submission=std::vector<std::shared_ptr<Batch>>;
extern std::atomic<bool> active;
void Mark(ID3D12GraphicsCommandList*,uint32_t node,int side,ID3D12PipelineState*,uint32_t detail=0);
void EndRange(ID3D12GraphicsCommandList*);
void Close(ID3D12GraphicsCommandList*);
void Reset(ID3D12GraphicsCommandList*);
Submission BeforeSubmit(UINT,ID3D12CommandList* const*);
void Submitted(ID3D12CommandQueue*,Submission);
void Poll();
// The list's last viewport, for CyberpunkVR_SceneGpuViewportDetail (draws only).
void Viewport(ID3D12GraphicsCommandList*,UINT count,const D3D12_VIEWPORT*);
// Detail for a draw: the viewport class of the list's last RSSetViewports when the switch is on, else 0.
// 0x100000 | kind << 16 | width: kind 1 x == 0 (left eye or the whole frame), 2 x > 0 and x == width (the right
// half of a side-by-side frame), 3 any other x. The offline side tells the left eye from the whole frame by the
// widths the same node draws its right halves with (tools/vr_bench).
uint32_t DrawDetail(ID3D12GraphicsCommandList*);
// Around a draw of a profiled range (CyberpunkVR_SceneGpuDrawOcclusion): an occlusion query per draw, so the report
// counts per eye the draws that left no visible sample (hidden or outside the eye's view).
void DrawBegin(ID3D12GraphicsCommandList*);
void DrawEnd(ID3D12GraphicsCommandList*);
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneGpuRequest,CyberpunkVR_SceneGpuReportSeq;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneGpuGroupMode,CyberpunkVR_SceneGpuNodeFilter;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneGpuViewportDetail;
__declspec(dllexport) extern char CyberpunkVR_SceneGpuReport[262144];
}
