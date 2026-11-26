#pragma once
// DLSS5 NEURAL RENDER (src/Render/NeuralRender.cpp) -- experimental, off by default.
//
// NVIDIA's DLSS 5 neural-render pass (NGX feature 18) run on each eye right after the game's own DLSS, its result
// written back over that eye's DLSS output. The game's NGX dispatcher must support the NR feature. The switch and model controls are on the STEREO page of the F10 and VR menus
// (CyberpunkVR_Nr* exports); none of them is saved, so the next start is always without it.

namespace cvr::neural_render {

// Once per Present: the frame the alternate-eyes mode counts, and the NGX hook put in once the switch is on.
void OnPresent();

}  // namespace cvr::neural_render
