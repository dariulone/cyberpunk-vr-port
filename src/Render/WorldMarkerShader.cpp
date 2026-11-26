#include "Render/WorldMarkerShader.hpp"
#include "WorldMarkerShaderCode.hpp"
#include <atomic>
#include <cstring>
#include <iterator>

#if !CVR_RUST_VRIK
extern void Log(const char*,...);
#endif

namespace cvr::markers {
namespace {
// The flat ink vertex shaders of 2.5 (engine\shader_final.cache), by container length and DXBC digest, each with
// its replacement from Render/WorldMarkers.hlsl. 2.5 added the stereo variants (the engine's per-eye offset, see
// the HLSL); the screen sprite and the procedural shape kept 2.31's code under a new digest. RenderDoc
// (SBS_M2.rdc): sprites draw with SpriteStereo, glyphs with TextStereo.
struct Variant { uint32_t length; unsigned char digest[16]; const unsigned char* code; size_t size; };
constexpr Variant variants[]={
    {3042,{0x01,0x6a,0xd1,0x94,0x18,0x9b,0x83,0x51,0xb5,0xaf,0x0b,0xdf,0xfe,0x34,0x7d,0x12},bytecode::Sprite,sizeof(bytecode::Sprite)},
    {3442,{0x22,0xb0,0x65,0x57,0x3e,0xc4,0xc7,0x63,0x8e,0xe3,0xf3,0xa7,0xc0,0x32,0x9b,0x9b},bytecode::Procedural,sizeof(bytecode::Procedural)},
    {3350,{0x4f,0xe4,0x56,0x79,0x98,0x5b,0x38,0x49,0x4a,0xd6,0xcf,0x1e,0xc8,0x2e,0xc2,0xc8},bytecode::SpriteStereo,sizeof(bytecode::SpriteStereo)},
    {3350,{0x3f,0x0e,0x72,0xc5,0x31,0x8f,0xe2,0xf6,0x41,0x86,0x69,0xe1,0xe9,0xc2,0x5d,0x9b},bytecode::TextStereo,sizeof(bytecode::TextStereo)},
    {3350,{0x5b,0x31,0xe8,0x62,0x62,0xdc,0x37,0x12,0x03,0xbd,0x2d,0x20,0x19,0xaf,0xd6,0x34},bytecode::SpriteStereoZ,sizeof(bytecode::SpriteStereoZ)},
    {3302,{0xad,0x30,0x45,0x79,0x5b,0x73,0xd0,0x3c,0x25,0xc7,0x85,0xe6,0x1f,0x68,0x04,0x4e},bytecode::SpriteStereoOne,sizeof(bytecode::SpriteStereoOne)},
};
static_assert(sizeof(Variant) == 40, "mirrored by rust/crates/cvr-render/src/world_marker_shader.rs");
#if CVR_RUST_VRIK
}
}
// The recognition is Rust's; this table (the generated replacements) is handed to it while the DLL initialises.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's table is rust/crates/cvr-render/src/world_marker_shader.rs' (its DXIL compiled by build.rs)
extern "C" void cvr_world_marker_shaders_attach(const void* table, size_t count);
namespace {
const bool attached = (cvr_world_marker_shaders_attach(cvr::markers::variants, std::size(cvr::markers::variants)), true);
}
#endif
#else
std::atomic<unsigned> installed{0};
std::atomic<bool> failed{false};
int VariantOf(const D3D12_SHADER_BYTECODE& bc) {
    if(!bc.pShaderBytecode || bc.BytecodeLength<20) return -1;
    const auto* b=static_cast<const unsigned char*>(bc.pShaderBytecode);
    if(std::memcmp(b,"DXBC",4)) return -1;
    for(int i=0;i<int(std::size(variants));++i)
        if(bc.BytecodeLength==variants[i].length && !std::memcmp(b+4,variants[i].digest,16)) return i;
    return -1;
}
}
D3D12_SHADER_BYTECODE VertexShaderCode(bool procedural) {
    // Native pixel shaders are SM6. D3D12 rejects a pipeline that mixes them
    // with FXC/SM5 vertex shaders, so compile with DXC at build time and embed.
    const auto& v=variants[procedural ? 1 : 0];
    return {v.code,v.size};
}
bool VertexShaderReplacement(const D3D12_SHADER_BYTECODE& original,D3D12_SHADER_BYTECODE& replacement) {
    const int variant=VariantOf(original);
    if(variant<0) return false;
    replacement={variants[variant].code,variants[variant].size};return true;
}
void VertexShaderResult(const D3D12_SHADER_BYTECODE& original,bool success) {
    const int variant=VariantOf(original);
    if(variant<0) return;
    if(!success) { failed.store(true,std::memory_order_release);Log("[world-markers] VS pipeline rejected; tagging disabled\n"); }
    else {
        const unsigned bit=1u<<variant;
        if(!(installed.fetch_or(bit,std::memory_order_acq_rel)&bit))
            Log("[world-markers] native ink VS variant %d installed\n",variant);
    }
}
// Every pipeline of the family is replaced when it is created, so the first one built proves the path; a tagged
// vertex can only reach a pipeline that went through it. Any rejection turns tagging off for good.
bool VertexShadersReady() { return installed.load(std::memory_order_acquire)!=0 && !failed.load(std::memory_order_acquire); }
}
#endif
