#pragma once
// StereoPairProbe -- diagnostic: how engine stereo sends the two eyes' draws (tools/vr_bench --pairs).
//
// With cvr_pair_probe.txt in the plugin's log folder at startup, the graphics root-parameter setters and the index
// buffer setter of every hooked command-list vtable get pass-through detours that keep each list's bound state. Asked
// for whole frames (CyberpunkVR_PairProbeRequest), every draw inside one render-graph node is recorded with its
// pipeline, root signature, every root parameter (table handle, buffer address, or a hash of root constants), index
// and vertex buffers, viewport and arguments. The offline side pairs each left-eye batch with the right-eye batch
// after it and says which fields differ: the question a view-instanced draw stands or falls on. Without the marker
// nothing is installed; with it and no request, the detours keep state only.
#include <d3d12.h>
#include <cstdint>

namespace cvr::stereo::pair_probe {
bool Wanted();
// Called with the vtable writable, after the census hooks: saves the originals of slots 30, 32, 34, 36, 38, 40, 42,
// 43 for this vtable and installs the detours.
void Install(void** vtable);
bool Tracking();   // the marker was found (state is kept)
void Reset(ID3D12GraphicsCommandList*);
void Viewports(ID3D12GraphicsCommandList*,UINT count,const D3D12_VIEWPORT*);
void VertexBuffers(ID3D12GraphicsCommandList*,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW*);
// kind: 1 DrawInstanced, 2 DrawIndexedInstanced, 3 ExecuteIndirect. args: up to five native arguments.
void Draw(ID3D12GraphicsCommandList*,uint32_t node,ID3D12PipelineState*,uint32_t kind,const uint32_t (&args)[5]);
void Poll();   // once per Present: arms and ends the frame window

// SINGLE-PASS FEASIBILITY (CyberpunkVR_PairSubstituteMask): a right-eye draw of the probed node whose left-eye partner
// (the same place in the left batch just before it, same pipeline, index and vertex buffers) is known gets that
// partner's descriptor tables for the root parameters in the mask -- and, with CyberpunkVR_PairSubstituteInstance,
// its start instance -- for the draw only. If the right eye's picture does not change with every table but the
// camera's taken from the left draw, one draw can serve both eyes with only the camera chosen per view.
// Returns the start instance to draw with; AfterDraw puts the engine's tables back.
uint32_t BeforeDraw(ID3D12GraphicsCommandList*,uint32_t node,ID3D12PipelineState*,uint32_t kind,const uint32_t (&args)[5],
                    uint32_t startInstance);
void AfterDraw(ID3D12GraphicsCommandList*);
}
