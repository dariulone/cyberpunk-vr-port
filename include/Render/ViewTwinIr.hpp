#pragma once
// ViewTwinIr -- the text rewrite that makes an engine-stereo material shader view-instanced (single pass).
//
// 2.5 engine stereo draws every RenderElements batch twice: the left eye's draws, then the right eye's same draws.
// A pair differs only in the descriptor tables of root parameters 0 (vertex) and 18 (pixel): CBVs b0..b3 (global,
// camera, probes, custom constants); every other table and the start instance can be taken from either eye
// (tools/vr_bench ab sp-substitute, 2026-11-05). The twin of a shader reads each of its b0..b3 constant buffers
// twice -- from the engine's table (space 0, the right eye's at the right batch) and from a second table at the
// same registers in space 10 (the left eye's) -- and keeps view 0's value from space 10 and view 1's from space 0:
//
//     %v  = cbufferLoadLegacy(b1 space 0, row)            (unchanged)
//     %v' = cbufferLoadLegacy(b1 space 10, row)           (added)
//     %e  = extractvalue %v, k                  ->  %e = select i1 (SV_ViewID == 0), extract(%v', k), extract(%v, k)
//
// Input is DXC's lossless disassembly (IDxcCompiler::Disassemble, as `dxc -dumpbin`). Anything outside the
// recognised shape (another use of a b0..b3 handle or of a load, an existing SV_ViewID, an array range) fails
// closed with a reason: that pipeline keeps the engine's two draws.
#include <cstdint>
#include <string>

namespace cvr::single_pass {
constexpr uint32_t kTwinSpace=10;          // register space of the second eye's b0..b3
constexpr uint32_t kTwinRecipe=2;          // bump when the rewrite changes: cached twins are keyed by it
struct IrTwin {
    bool ok{};
    bool unchanged{};                      // no b0..b3 read: the original serves both views as it is
    bool signature{};                      // the pixel shader's inputs were completed from the vertex shader's outputs
    uint32_t loads{},handles{},extracts{};
    std::string text;                      // the rewritten module (ok && !unchanged)
    std::string error;
};
// vertex: the disassembly of the vertex shader a pixel shader is linked with (its outputs complete the pixel shader's
// inputs when that one reads a system input); nullptr for a vertex shader.
IrTwin RewriteForViews(const std::string& disassembly,const std::string* vertex=nullptr);
}
