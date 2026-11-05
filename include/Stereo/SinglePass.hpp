#pragma once
// SinglePass -- one view-instanced draw for both eyes of an engine-stereo RenderElements draw pair.
//
// Under 2.5 engine stereo every RenderElements batch is drawn twice in the same command list: the left eye's N draws
// (viewport at x = 0), then the right eye's same N draws (viewport at x = its width). A pair differs only in the
// camera tables, root parameters 0 and 18 (b0..b3); everything else of either eye's draw serves both
// (tools/vr_bench ab sp-substitute). So:
//
//   * a LEFT draw whose pipeline has a view-instanced twin (Render/SinglePassPipelines.hpp) is not sent: its state
//     is kept (pending);
//   * its RIGHT partner (the same place in the right batch, the same pipeline, buffers, topology and arguments but
//     the start instance) is sent as ONE draw with the twin pipeline on the twin root -- the right eye's tables as
//     the engine bound them, the left eye's camera tables in parameters 36/37 -- and two viewports and scissors:
//     the left eye's (view 0) and the right eye's (view 1);
//   * anything else -- a right draw without a matching partner, a draw outside the node, any command that is not a
//     draw or one of the tracked state setters (topology, viewports, scissors, pipeline, root, tables, index and
//     vertex buffers), Close -- first sends every pending left draw as it was (FLUSH: its own state, then the
//     engine's state back). The left eye's draws keep their order; nothing reads their output before it is drawn.
//
// CyberpunkVR_SinglePass: 0 off, 1 on (default; without dxc\dxcompiler.dll and dxil.dll beside the plugin nothing is
// kept), 2 replay test (left draws are kept and sent in front of their partner by the flush path, with no
// view-instanced draw: must leave the picture unchanged).
#include <d3d12.h>
#include <atomic>
#include <cstdint>

namespace cvr::single_pass {
// From Stereo/DeviceHooks.cpp patch_command_list_vtable, after its own slots are written and while slots 9..lastSlot
// are writable: chains the detours that keep the state and flush.
void Install(void** vtable,size_t lastSlot);
// The first material root exists (Render/SinglePassPipelines.cpp): from now on the lists' state is kept.
void Activate();
// From the census hooks (Stereo/CommandListCensus.cpp).
void Reset(ID3D12GraphicsCommandList*);
// Before the census hooks send the engine's call on: true when single pass holds it (CyberpunkVR_SinglePassLazy) and
// the caller must not send it -- the next draw gets the state.
bool PipelineSet(ID3D12GraphicsCommandList*,ID3D12PipelineState* engine,ID3D12PipelineState* bound);
bool Viewports(ID3D12GraphicsCommandList*,UINT count,const D3D12_VIEWPORT*);
bool Scissors(ID3D12GraphicsCommandList*,UINT count,const D3D12_RECT*);
bool VertexBuffers(ID3D12GraphicsCommandList*,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW*);
// Before the census sends a draw: true when single pass took it (kept or drawn for both eyes) and the caller must
// not send it; false after the list's state is the engine's again. kind 1 DrawInstanced (args: vertices, instances,
// start vertex, start instance), 2 DrawIndexedInstanced (indices, instances, start index, base vertex, start
// instance).
bool Draw(ID3D12GraphicsCommandList*,uint32_t node,uint32_t kind,const uint32_t (&args)[5]);
// Before any other command that reads or writes what the draws produce (the census hooks that are not chained).
void Flush(ID3D12GraphicsCommandList*);
}

extern "C" {
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePass;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassNode;
__declspec(dllexport) extern std::atomic<uint32_t> CyberpunkVR_SinglePassLazy;
// [0] left draws kept, [1] view-instanced draws, [2] left draws sent by a flush, [3] right draws sent alone (no
// partner kept), [4] left draws sent alone (twin not ready), [5] flushes, [6] right draws whose partner did not
// match, [7] draws outside the eyes' viewports, [8] batches, [9] twin roots bound
__declspec(dllexport) extern std::atomic<uint64_t> CyberpunkVR_SinglePassCounts[10];
}
