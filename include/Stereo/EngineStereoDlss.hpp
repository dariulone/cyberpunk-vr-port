#pragma once
// ENGINE STEREO: DLSS PER EYE (src/Stereo/EngineStereoDlss.cpp).
//
// With render feature 98 the engine upscales the side-by-side frame with ONE DLSS evaluate: one history over both
// eyes, a filter that reaches across the seam in the middle and motion that can carry one eye's pixels into the
// other's. Here that evaluate becomes two, one per eye, each on its own Streamline viewport (its own DLSS feature
// and history), its inputs and output the eye's half of the engine's tagged resources, its options the engine's
// with half the output width, its constants the engine's with the motion scale of a half-width input.
//
// The Streamline hooks feed it: slSetTag (Hooks/Ngx.cpp), slSetConstants and slEvaluateFeature
// (Framegen/Inputs.cpp), slDLSSSetOptions (hooked here through slGetFeatureFunction).
#include <cstdint>
#include <sl_core_types.h>

namespace sl { struct Constants; }

namespace cvr::engine_stereo::dlss {

using SetTagFn = uint32_t (*)(const void* viewport, const void* tags, uint32_t count, void* commandList);
using ConstantsFn = sl::Result (*)(const sl::Constants&, const sl::FrameToken&, const sl::ViewportHandle&);
using EvaluateFn = sl::Result (*)(sl::Feature, const sl::FrameToken&, const sl::BaseStructure* const*, uint32_t, void*);

// slSetTag as the engine called it (before the original runs); `original` is the unhooked slSetTag.
void RecordTags(const void* viewport, const void* tags, uint32_t count, SetTagFn original);
// slSetConstants as the engine called it.
void RecordConstants(const sl::Constants& values, const sl::FrameToken& frame, const sl::ViewportHandle& viewport);
// slEvaluateFeature: true when it ran the evaluate itself (per eye), the result in *result; false to let the
// caller run the engine's evaluate unchanged.
bool Evaluate(sl::Feature feature, const sl::FrameToken& frame, const sl::BaseStructure* const* inputs, uint32_t count,
              void* commandList, EvaluateFn evaluate, ConstantsFn constants, sl::Result* result);
// Serialized fallback; accounts for original viewport command-list/GPU lifetime.
sl::Result EvaluateOriginal(sl::Feature, const sl::FrameToken&, const sl::BaseStructure* const*, uint32_t,
                            void*, EvaluateFn);
// The eye whose DLSS evaluate runs on this thread, inside Evaluate (0 left, 1 right); -1 outside one. Both eyes are
// MAIN's view, so what runs under it (NGX's evaluate, Render/FoveatedDlss.cpp) tells them apart by this.
int EvaluatingEye();
// slDLSSSetOptions is only reachable through slGetFeatureFunction once the DLSS plugin is loaded: tried when the
// Streamline hooks go in and again at evaluates until it succeeds.
void InstallOptionsHook();

}  // namespace cvr::engine_stereo::dlss
