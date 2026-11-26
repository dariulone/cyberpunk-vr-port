#pragma once
// The render threads' state in the Rust build is Rust's (rust/crates/cvr-render/src/render_tls.rs): one block per
// thread. Each name below is that block's field through an inline reference function in cvr::detail, so every use --
// read, write, address, cvr::detail::-qualified -- reaches the Rust thread's storage. The C++ build keeps its
// thread-locals (include/Stereo/StereoInternal.hpp).
#if CVR_RUST_VRIK
#include <d3d12.h>
#include <dxgi.h>
#include <cstddef>
#include <cstdint>
namespace cvr::detail {
struct RenderTls {
    uintptr_t currentNodeWork;
    int32_t cascadeIdx;
    bool vrcamNodeActive;
    UINT curRtW;
    UINT curRtH;
    bool mirrorCopyNodeActive;
    bool inVrcam2Rt;
    bool cbArmed2Rt;
    ID3D12Resource* tmRt0;
    bool tmFromFinal;
    ID3D12GraphicsCommandList* tmRt0List;
    uint32_t tmRt0State;
    bool tmRt0StateSeen;
    bool tmConsumed;
    ID3D12Resource* mirrorCopyRtv;
    DXGI_FORMAT mirrorCopyRtvFormat;
    ID3D12GraphicsCommandList* mirrorCopyList;
    uint32_t mirrorSrcState;
    bool mirrorSrcStateSeen;
    ID3D12Resource* finalRes;
    uint32_t finalState;
    ID3D12GraphicsCommandList* finalList;
    bool activeViewKnown;
    uint64_t activeViewKey;
    uintptr_t visionNode;
    int32_t visionOrd;
    UINT lastDisp[3];
    uintptr_t currentNodeView;
    int32_t viewSide;
    int profDispDepth;
    int64_t profChildTicks;
};
static_assert(sizeof(RenderTls) == 184 && offsetof(RenderTls, viewSide) == 168 && offsetof(RenderTls, lastDisp) == 148,
              "mirrored by render_tls.rs");
extern "C" RenderTls* cvr_render_tls();
inline uintptr_t& t_current_node_work_tls() { return cvr_render_tls()->currentNodeWork; }
inline int32_t& t_cascade_idx_tls() { return cvr_render_tls()->cascadeIdx; }
inline bool& t_vrcam_node_active_tls() { return cvr_render_tls()->vrcamNodeActive; }
inline UINT& t_cur_rt_w_tls() { return cvr_render_tls()->curRtW; }
inline UINT& t_cur_rt_h_tls() { return cvr_render_tls()->curRtH; }
inline bool& t_mirror_copy_node_active_tls() { return cvr_render_tls()->mirrorCopyNodeActive; }
inline bool& t_in_vrcam_2rt_tls() { return cvr_render_tls()->inVrcam2Rt; }
inline bool& t_2rt_cb_armed_tls() { return cvr_render_tls()->cbArmed2Rt; }
inline ID3D12Resource*& t_tm_rt0_tls() { return cvr_render_tls()->tmRt0; }
inline bool& t_tm_from_final_tls() { return cvr_render_tls()->tmFromFinal; }
inline ID3D12GraphicsCommandList*& t_tm_rt0_list_tls() { return cvr_render_tls()->tmRt0List; }
inline uint32_t& t_tm_rt0_state_tls() { return cvr_render_tls()->tmRt0State; }
inline bool& t_tm_rt0_state_seen_tls() { return cvr_render_tls()->tmRt0StateSeen; }
inline bool& t_tm_consumed_tls() { return cvr_render_tls()->tmConsumed; }
inline ID3D12Resource*& t_mirror_copy_rtv_tls() { return cvr_render_tls()->mirrorCopyRtv; }
inline DXGI_FORMAT& t_mirror_copy_rtv_format_tls() { return cvr_render_tls()->mirrorCopyRtvFormat; }
inline ID3D12GraphicsCommandList*& t_mirror_copy_list_tls() { return cvr_render_tls()->mirrorCopyList; }
inline uint32_t& t_mirror_src_state_tls() { return cvr_render_tls()->mirrorSrcState; }
inline bool& t_mirror_src_state_seen_tls() { return cvr_render_tls()->mirrorSrcStateSeen; }
inline ID3D12Resource*& t_final_res_tls() { return cvr_render_tls()->finalRes; }
inline uint32_t& t_final_state_tls() { return cvr_render_tls()->finalState; }
inline ID3D12GraphicsCommandList*& t_final_list_tls() { return cvr_render_tls()->finalList; }
inline bool& t_active_view_known_tls() { return cvr_render_tls()->activeViewKnown; }
inline uint64_t& t_active_view_key_tls() { return cvr_render_tls()->activeViewKey; }
inline uintptr_t& t_vision_node_tls() { return cvr_render_tls()->visionNode; }
inline int32_t& t_vision_ord_tls() { return cvr_render_tls()->visionOrd; }
inline UINT (&t_last_disp_tls())[3] { return cvr_render_tls()->lastDisp; }
inline uintptr_t& t_current_node_view_tls() { return cvr_render_tls()->currentNodeView; }
inline int32_t& t_view_side_tls() { return cvr_render_tls()->viewSide; }
inline int& t_prof_disp_depth_tls() { return cvr_render_tls()->profDispDepth; }
inline int64_t& t_prof_child_ticks_tls() { return cvr_render_tls()->profChildTicks; }
}  // namespace cvr::detail
#define t_current_node_work t_current_node_work_tls()
#define t_cascade_idx t_cascade_idx_tls()
#define t_vrcam_node_active t_vrcam_node_active_tls()
#define t_cur_rt_w t_cur_rt_w_tls()
#define t_cur_rt_h t_cur_rt_h_tls()
#define t_mirror_copy_node_active t_mirror_copy_node_active_tls()
#define t_in_vrcam_2rt t_in_vrcam_2rt_tls()
#define t_2rt_cb_armed t_2rt_cb_armed_tls()
#define t_tm_rt0 t_tm_rt0_tls()
#define t_tm_from_final t_tm_from_final_tls()
#define t_tm_rt0_list t_tm_rt0_list_tls()
#define t_tm_rt0_state t_tm_rt0_state_tls()
#define t_tm_rt0_state_seen t_tm_rt0_state_seen_tls()
#define t_tm_consumed t_tm_consumed_tls()
#define t_mirror_copy_rtv t_mirror_copy_rtv_tls()
#define t_mirror_copy_rtv_format t_mirror_copy_rtv_format_tls()
#define t_mirror_copy_list t_mirror_copy_list_tls()
#define t_mirror_src_state t_mirror_src_state_tls()
#define t_mirror_src_state_seen t_mirror_src_state_seen_tls()
#define t_final_res t_final_res_tls()
#define t_final_state t_final_state_tls()
#define t_final_list t_final_list_tls()
#define t_active_view_known t_active_view_known_tls()
#define t_active_view_key t_active_view_key_tls()
#define t_vision_node t_vision_node_tls()
#define t_vision_ord t_vision_ord_tls()
#define t_last_disp t_last_disp_tls()
#define t_current_node_view t_current_node_view_tls()
#define t_view_side t_view_side_tls()
#define t_prof_disp_depth t_prof_disp_depth_tls()
#define t_prof_child_ticks t_prof_child_ticks_tls()
#endif
