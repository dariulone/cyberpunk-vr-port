#pragma once

struct ID3D12Device;
struct ID3D12GraphicsCommandList;
struct ID3D12Resource;

// EXTRAPOLATION: the last real frame moved a fraction of a game frame AHEAD along the engine's own motion vectors
// (Framegen/Fsr3.hpp, mode Extrapolation). The headset shows it after that frame, half a game frame later, so the
// real frames go out as soon as they are finished -- interpolation has to hold each one back until the frame
// between it and the one before is made.
//
// One pass per eye, a gather: each output pixel q takes the colour of the source pixel s whose motion carries it
// onto q. The engine's vectors point from a pixel to where it was a frame before (prevUV = uv + mv, as the dilate
// pass and FidelityFX read them), so a pixel moves -mv a frame and lands at s - factor * mv(s); s is found by three
// fixed-point steps of s = q + factor * mv(s). Nothing is left empty: where a moving object uncovers what was
// behind it, the nearest moving pixels stretch over the gap -- the first test is whether direction and amount are
// right, filling what is uncovered comes after.
bool Fsr3Extrapolate_Init(ID3D12Device* device);

// Records the pass for one eye. `color` is the captured eye image (display size, read as R8G8B8A8_UNORM: the
// capture's sRGB-encoded bytes, untouched), `motion` the dilated vectors (render size, R16G16_FLOAT, UV units),
// `output` a display-size R8G8B8A8 texture written through an R8G8B8A8_UNORM UAV. The caller puts color and motion
// in NON_PIXEL_SHADER_RESOURCE and output in UNORDERED_ACCESS first, and back after.
bool Fsr3Extrapolate_Run(ID3D12GraphicsCommandList* list, ID3D12Resource* color, ID3D12Resource* motion,
                         ID3D12Resource* output, int displayWidth, int displayHeight, int renderWidth,
                         int renderHeight, float factor);

void Fsr3Extrapolate_Shutdown();
