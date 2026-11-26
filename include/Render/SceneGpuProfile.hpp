#pragma once
#include <d3d12.h>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <vector>

namespace cvr::gpu::scene_profile {
#if CVR_RUST_VRIK
// The profile: rust/crates/cvr-gpu/src/scene_profile.rs (the comments in the C++ build's declarations below describe
// each call).
extern "C" {
void* cvr_scene_profile_active();
void cvr_scene_profile_viewport(ID3D12GraphicsCommandList* list,UINT count,const D3D12_VIEWPORT* viewports);
uint32_t cvr_scene_profile_draw_detail(ID3D12GraphicsCommandList* list);
void cvr_scene_profile_mark(ID3D12GraphicsCommandList* list,uint32_t node,int side,ID3D12PipelineState* pso,uint32_t detail);
void cvr_scene_profile_end_range(ID3D12GraphicsCommandList* list);
void cvr_scene_profile_close(ID3D12GraphicsCommandList* list);
void cvr_scene_profile_reset(ID3D12GraphicsCommandList* list);
void cvr_scene_profile_draw_begin(ID3D12GraphicsCommandList* list);
void cvr_scene_profile_draw_end(ID3D12GraphicsCommandList* list);
void* cvr_scene_profile_before_submit(UINT count,ID3D12CommandList* const* lists);
size_t cvr_scene_profile_submission_len(const void* submission);
void cvr_scene_profile_submission_free(void* submission);
void cvr_scene_profile_submitted(ID3D12CommandQueue* queue,void* submission);
void cvr_scene_profile_poll();
}
// The batches of one submission (Rust's), handed from BeforeSubmit to Submitted.
class Submission {
public:
    Submission()=default;
    explicit Submission(void* value):m_value(value){}
    Submission(Submission&& other) noexcept:m_value(other.m_value){other.m_value=nullptr;}
    Submission& operator=(Submission&& other) noexcept {
        if(this!=&other){if(m_value)cvr_scene_profile_submission_free(m_value);m_value=other.m_value;other.m_value=nullptr;}
        return *this;
    }
    Submission(const Submission&)=delete;
    Submission& operator=(const Submission&)=delete;
    ~Submission(){if(m_value)cvr_scene_profile_submission_free(m_value);}
    size_t size() const {return cvr_scene_profile_submission_len(m_value);}
    bool empty() const {return !m_value;}
    void* release(){void* value=m_value;m_value=nullptr;return value;}
private:
    void* m_value=nullptr;
};
inline std::atomic<bool>& active=*static_cast<std::atomic<bool>*>(cvr_scene_profile_active());
inline void Mark(ID3D12GraphicsCommandList* list,uint32_t node,int side,ID3D12PipelineState* pso,uint32_t detail=0) {
    cvr_scene_profile_mark(list,node,side,pso,detail);
}
inline void EndRange(ID3D12GraphicsCommandList* list){cvr_scene_profile_end_range(list);}
inline void Close(ID3D12GraphicsCommandList* list){cvr_scene_profile_close(list);}
inline void Reset(ID3D12GraphicsCommandList* list){cvr_scene_profile_reset(list);}
inline Submission BeforeSubmit(UINT count,ID3D12CommandList* const* lists){return Submission(cvr_scene_profile_before_submit(count,lists));}
inline void Submitted(ID3D12CommandQueue* queue,Submission values){cvr_scene_profile_submitted(queue,values.release());}
inline void Poll(){cvr_scene_profile_poll();}
inline void Viewport(ID3D12GraphicsCommandList* list,UINT count,const D3D12_VIEWPORT* viewports){cvr_scene_profile_viewport(list,count,viewports);}
inline uint32_t DrawDetail(ID3D12GraphicsCommandList* list){return cvr_scene_profile_draw_detail(list);}
inline void DrawBegin(ID3D12GraphicsCommandList* list){cvr_scene_profile_draw_begin(list);}
inline void DrawEnd(ID3D12GraphicsCommandList* list){cvr_scene_profile_draw_end(list);}
#else
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
#endif
}
extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneGpuRequest,CyberpunkVR_SceneGpuReportSeq;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneGpuGroupMode,CyberpunkVR_SceneGpuNodeFilter;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SceneGpuViewportDetail;
__declspec(dllexport) extern char CyberpunkVR_SceneGpuReport[262144];
}
