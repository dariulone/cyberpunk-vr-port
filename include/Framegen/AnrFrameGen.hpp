#pragma once
#ifdef CVR_NATIVE_DLSSG
#include <d3d12.h>
#include <nvsdk_ngx.h>
#include <nvsdk_ngx_defs_dlssg.h>
#include <nvsdk_ngx_params_dlssg.h>

struct NVSDK_NGX_D3D12_DLSSG_Eval_Params;
namespace cvr::framegen::anr {
// ANR FG (Apex Neural Rendering, github.com/apex-compute/ANR): a DLSS-G provider, an nvngx_dlssg.dll of its own that
// exports NGX's D3D12 entry points. OptiScaler runs it as a custom frame generation provider: it loads the dll from
// custom/<name>/dlssg (Nvngx_Custom), and the game's NGX calls for the FrameGeneration feature go to the dll's
// exports with the caller's own parameter table (Nvngx_DllProxy). The plugin does the same with the package unchanged
// in its anr\dlssg folder: Init_Ext with that folder as the feature path, PopulateParameters_Impl for the caps, and
// CreateFeature / EvaluateFeature through the DLSS SDK's own DLSS-G helpers, so every key and value is the one
// NVIDIA's DLSS-G gets.
bool Available(ID3D12Device*);
const char* Reason();
// A parameter table of the plugin's own (NGX's core allocates them for DLSS-G; a provider dll has no allocator).
NVSDK_NGX_Parameter* AllocateParameters();
void DestroyParameters(NVSDK_NGX_Parameter*);
NVSDK_NGX_Result Create(ID3D12GraphicsCommandList*,NVSDK_NGX_Handle**,NVSDK_NGX_Parameter*,NVSDK_NGX_DLSSG_Create_Params*);
NVSDK_NGX_Result Evaluate(ID3D12GraphicsCommandList*,NVSDK_NGX_Handle*,NVSDK_NGX_Parameter*,
    NVSDK_NGX_D3D12_DLSSG_Eval_Params*,NVSDK_NGX_DLSSG_Opt_Eval_Params*);
NVSDK_NGX_Result Release(NVSDK_NGX_Handle*);
}
#endif
