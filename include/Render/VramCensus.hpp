#pragma once
// VRAM census: what the process's video memory is held by, while it is held.
//
// The creation counters (CyberpunkVR_CreationCounts) count what was made; they cannot tell growth from churn. The
// census follows every resource and heap from its creation until the runtime destroys it: a small object rides on
// each one as private data, and the runtime releases it with the object. So it knows the bytes alive now, by
//   - what holds them: D3D12MA's heaps (blocks) and the placed resources inside them, committed resources;
//   - the heap type (default, upload, readback) and the class (buffer, render target / depth / UAV texture, texture);
//   - who made it (the game's exe, the port, anything else: driver, DLSS, Streamline);
//   - for the game's resources, the call site: the first exe frames of the creation's stack.
// Once a minute the [vram-census] lines go to the log with the [vram] line (Stereo/DeviceHooks.cpp).
// xr_vram_census in vrport.ini: 1 on (default), 0 off.
#include <d3d12.h>
#include <cstdint>

namespace cvr::vram_census {
bool Enabled();
void SetEnabled(bool on);
// bytes: the allocation size (GetResourceAllocationInfo). heapType: where the memory lives (for a placed resource,
// its heap's). caller: the return address into the creation hook.
void ResourceCreated(ID3D12Resource* resource,const D3D12_RESOURCE_DESC& desc,D3D12_HEAP_TYPE heapType,uint64_t bytes,
                     bool placed,const void* caller);
void HeapCreated(ID3D12Heap* heap,const void* caller);
// The census lines; local and nonLocal are DXGI's current usage, to show how much of it the census accounts for.
void Log(uint64_t local,uint64_t nonLocal);
}
