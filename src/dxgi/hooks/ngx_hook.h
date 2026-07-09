// Read-only NGX (DLSS) EvaluateFeature hook.
//
// Goal: capture the engine's per-frame Motion Vector resource (+ depth +
// scaling factors + reset flag) at the moment DLSS evaluates a frame. These
// are ground-truth velocity / depth signals the game ALREADY computes for
// DLSS, so they cost us nothing extra. We never modify NGX state — just
// AddRef captured pointers into global storage that the AER submit path can
// consume for forward extrapolation (motion-vector-driven frame extrapolation
// instead of our current backward optical-flow midpoint).
//
// Second-stage goal (not in this header): per-eye DLSS history via cloned
// feature handles. Architecturally invasive — must come after we have proof
// that read-only capture is stable.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <atomic>

// Install one-shot hook on NVSDK_NGX_D3D12_EvaluateFeature in nvngx_dlss.dll.
// Safe to call repeatedly: only the first successful call patches; later calls
// are no-ops. Returns true once a hook is installed.
bool NgxInstallEvaluateFeatureHook();

// Live snapshot accessors (lock-free, AddRef'd; caller must Release).
ID3D12Resource* NgxAcquireMotionVectors();
ID3D12Resource* NgxAcquireDepth();
float NgxGetMvScaleX();
float NgxGetMvScaleY();
int   NgxGetResetFlag();
unsigned int NgxGetMvWidth();
unsigned int NgxGetMvHeight();
unsigned int NgxGetMvFormat();
unsigned int NgxGetEvalCount();
