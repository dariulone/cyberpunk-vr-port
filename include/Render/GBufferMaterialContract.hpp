#pragma once
#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <span>
#include <string_view>

namespace cvr::stereo::gbuffer_material {
inline constexpr std::string_view VertexShaderSha256="fe53e834aca0d9dacef4628fa10d2a6bbb698b02498673de99562733dd2904d1";
inline constexpr std::string_view PixelShaderSha256="8c5a09dee942322418112778412c6e08c43e1342f5bee642acdee6ff041e6cb5";
inline constexpr size_t MaterialBytes=292,ModifierBytes=16,GlobalFrameOffset=452,GlobalFrameBytes=4;
inline constexpr std::array<uint32_t,5> SelectorOffsets{0,32,80,128,160};
// The exact PS declares t0, space1 as [32768 x Texture2D<float4>].
inline constexpr uint32_t DescriptorCount=32768;

#if CVR_RUST_VRIK
extern "C" bool cvr_gm_matches_shaders(const char*,size_t,const char*,size_t);
inline bool MatchesShaders(std::string_view vertex,std::string_view pixel) {
    return cvr_gm_matches_shaders(vertex.data(),vertex.size(),pixel.data(),pixel.size());
}
#else
inline bool MatchesShaders(std::string_view vertex,std::string_view pixel) {
    return vertex==VertexShaderSha256 && pixel==PixelShaderSha256;
}
#endif

struct Inputs {
    std::span<const std::byte> material,modifiers,globalFrameSlice;
};
// Each index selects t0[index], space1; all use s0 with implicit LOD.
// The caller proves every sampled mip, not just mip zero.
struct TextureUse {
    uint32_t selectorOffset{},index{};
};
struct SamplerUse {
    uint32_t shaderRegister{},registerSpace{};
    bool required{};
};
struct DitherUse {
    uint32_t shaderRegister{},registerSpace{},mip{},slice{},coordinateMask{};
    bool required{};
};
struct Analysis {
    std::array<TextureUse,5> textures{};
    uint32_t textureCount{};
    SamplerUse sampler{};
    DitherUse dither{};
    bool alphaTestEnabled{},optionalTextureEnabled{},neutralDither{};
};
enum class Failure : uint32_t {
    None,MaterialSize,ModifierSize,GlobalFrameSize,NonFiniteModifier,NonFiniteMaterial,TextureIndex
};

#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/gbuffer_material.rs (tools/single_pass_tests/gbuffer_material_contract_test
// runs its cases against them in its Rust variant). The bytes are lent.
namespace detail {
struct RustBytes {const std::byte* data;size_t size;};
struct RustInputs {RustBytes material,modifiers,globalFrameSlice;};
inline RustInputs Lend(const Inputs& input) {
    return {{input.material.data(),input.material.size()},{input.modifiers.data(),input.modifiers.size()},
        {input.globalFrameSlice.data(),input.globalFrameSlice.size()}};
}
}
static_assert(sizeof(detail::RustInputs)==48 && sizeof(Analysis)==84 && offsetof(Analysis,sampler)==44 &&
    offsetof(Analysis,dither)==56 && offsetof(Analysis,alphaTestEnabled)==80,"mirrored by gbuffer_material.rs");
extern "C" {
uint32_t cvr_gm_analyze(const detail::RustInputs*,Analysis*);
bool cvr_gm_same_inputs(const detail::RustInputs*,const detail::RustInputs*);
}
inline Failure Analyze(const Inputs& input,Analysis& output) {
    const auto lent=detail::Lend(input);
    return Failure(cvr_gm_analyze(&lent,&output));
}
inline bool SameInputs(const Inputs& main,const Inputs& peer) {
    const auto a=detail::Lend(main),b=detail::Lend(peer);
    return cvr_gm_same_inputs(&a,&b);
}
#else
namespace detail {
inline uint32_t Word(std::span<const std::byte> bytes,size_t offset) {
    uint32_t value{};
    for(size_t i=0;i<4;++i)value|=uint32_t(std::to_integer<uint8_t>(bytes[offset+i]))<<(8*i);
    return value;
}
inline float Scalar(std::span<const std::byte> bytes,size_t offset) {
    static_assert(sizeof(float)==4 && std::numeric_limits<float>::is_iec559);
    return std::bit_cast<float>(Word(bytes,offset));
}
}

// The caller establishes the exact shader pair and freshness of these GPU bytes.
// This describes dependencies; it proves neither descriptor validity nor GPU content lifetime.
inline Failure Analyze(const Inputs& input,Analysis& output) {
    output={};
    if(input.material.size()!=MaterialBytes)return Failure::MaterialSize;
    if(input.modifiers.size()!=ModifierBytes)return Failure::ModifierSize;
    if(input.globalFrameSlice.size()!=GlobalFrameBytes)return Failure::GlobalFrameSize;
    for(size_t offset=0;offset<ModifierBytes;offset+=4)
        if(!std::isfinite(detail::Scalar(input.modifiers,offset)))return Failure::NonFiniteModifier;
    constexpr std::array<size_t,10> alwaysFloat{16,20,24,48,64,96,112,144,192,288};
    const bool alpha=detail::Scalar(input.modifiers,0)!=0.f;
    const bool optional=detail::Scalar(input.material,192)>0.f;
    for(const auto offset:alwaysFloat)
        if(!std::isfinite(detail::Scalar(input.material,offset)))return Failure::NonFiniteMaterial;
    if(alpha && !std::isfinite(detail::Scalar(input.material,272)))return Failure::NonFiniteMaterial;
    if(optional)for(const size_t offset:std::array<size_t,4>{176,180,184,224})
        if(!std::isfinite(detail::Scalar(input.material,offset)))return Failure::NonFiniteMaterial;

    Analysis result{};
    result.textureCount=optional?5u:4u;
    for(uint32_t i=0;i<result.textureCount;++i){
        const auto index=detail::Word(input.material,SelectorOffsets[i]);
        if(index>=DescriptorCount)return Failure::TextureIndex;
        result.textures[i]={SelectorOffsets[i],index};
    }
    result.sampler={0,0,true};
    result.dither={67,0,0,detail::Word(input.globalFrameSlice,0)&63u,63,true};
    result.alphaTestEnabled=alpha;
    result.optionalTextureEnabled=optional;
    // This reports control values only: t67 still executes and remains a required dependency.
    result.neutralDither=detail::Scalar(input.modifiers,8)==0.f && detail::Scalar(input.modifiers,12)==0.f;
    output=result;
    return Failure::None;
}

// Conservative byte equality includes inactive material fields and the entire frame word.
// Equal inputs do not imply equal or immutable sampled texture contents.
inline bool SameInputs(const Inputs& main,const Inputs& peer) {
    Analysis mainAnalysis{},peerAnalysis{};
    return Analyze(main,mainAnalysis)==Failure::None && Analyze(peer,peerAnalysis)==Failure::None &&
        std::equal(main.material.begin(),main.material.end(),peer.material.begin()) &&
        std::equal(main.modifiers.begin(),main.modifiers.end(),peer.modifiers.begin()) &&
        std::equal(main.globalFrameSlice.begin(),main.globalFrameSlice.end(),peer.globalFrameSlice.begin());
}
#endif
}
