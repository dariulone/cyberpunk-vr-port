#pragma once

#include <algorithm>
#include <cstdint>

// Values the tray can change while a game is running, read by the layer.
//
// A named shared-memory block rather than the INI: the layer reads it once per
// synthesized pair, and a memory read belongs on that path where a file read
// does not. The tray creates and owns the block; while it does not exist, or
// holds another layout, the layer uses the composition's built-in values.
// Diagnostic tuning only - nothing here is a setting, and nothing is saved.
//
// Since V507 the block serves the hybrid composition only: the synthesizer
// uploads it, header and all, into a small buffer the hybrid shader reads at
// t8 (frame_synthesis.hlsl, HybridTuning), so the field order below is the
// shader's too - a new field goes at the end, with its index there.
namespace xrfg::live_tuning {

inline constexpr wchar_t kMappingName[] = L"Local\\CVR_V557_Reconstruction_NoExternalTuning";
inline constexpr std::uint32_t kMagic = 0x4C565446U;  // "FTVL"
// 7: the hybrid's values (V507), uploaded whole; the repeating-pattern lock
// left with this version (not needed beside the hybrid). 6 was V497's
// (pattern lock and three hybrid values in the draw constants); 5 V478's
// (edge ghosting reduction and the pattern lock); 4 V477's; 3 V475's, which
// also carried the background fill and the hardware bilinear switch; 1 and 2
// the V374 flow-history window, which never reached main.
// 8 (V539): hybrid_tint, the diagnostic tint of generated frames.
// 9 (V543): mask_band_percent.
// 10: a test build's switch between the probes and the splat, never
// released. 11 (V555): the splat is the depth search; the probes' reach
// and solve count left with them.
inline constexpr std::uint32_t kVersion = 11U;

// Each slider in the tray runs from cheaper on the GPU (left) to costlier
// (right); the comments say which way that is for a value.
struct Block {
    std::uint32_t magic;
    std::uint32_t version;
    // Incremented by the tray after every change, so the layer can record
    // each new set of values exactly once.
    std::uint32_t sequence;

    // Hybrid choice: the game's motion vectors and the optical flow, chosen
    // per pixel. Where the two frames warped by the vectors disagree by less
    // than the agree threshold, the vectors are taken outright and the flow
    // branch is not evaluated at that pixel (higher = cheaper). Elsewhere
    // both run and the one whose warped samples disagree less wins, as
    // sharply as the choice slope says (1 soft blend, 15 hard pick). The
    // vectors slope is the vectors branch's own fall-back toward the pose-
    // only blend as its samples disagree (3 = 0.3's value); the flow rule
    // slope is how sharply the flow must beat the camera-only blend to be
    // kept (V502). The slopes cost nothing.
    std::uint32_t hybrid_agree_threshold_255ths;  // 0-62, default 62
    std::uint32_t hybrid_choice_slope;            // 1-15, default 8
    std::uint32_t hybrid_vectors_slope;           // 1-15, default 3
    std::uint32_t hybrid_flow_rule_slope;         // 1-15, default 8

    // The vectors branch at motion edges (V498-V504). The probes for an edge
    // reach edge_reach_percent of the pixel's motion (16 px at least), and
    // the search for a surface in front follows that reach: wider costs
    // solves. At most edge_search_solves second-surface solves per edge
    // pixel (0 none); a found surface's frames must agree within
    // edge_surface_agree (no cost). A revealed background accepted within
    // revealed_error (V500) is kept by the vectors and the flow is not asked
    // (looser = cheaper). Across a motion edge the nearest vector replaces
    // the bilinear one where the four texels differ by more than
    // vector_snap pixels (V503; 0 = always nearest; no cost).
    std::uint32_t edge_reach_percent;             // 100-200, default 100
    std::uint32_t edge_search_solves;             // 0-4, default 1
    std::uint32_t edge_surface_agree_1000ths;     // 0-250, default 50
    std::uint32_t revealed_error_1000ths;         // 0-250, default 0
    std::uint32_t vector_snap_tenths;             // 0-40, default 0

    // The depth search for thin near surfaces (V505; the midpoint splat
    // since V554/V555): off, nothing of it runs. A compute pass at the
    // vectors' resolution writes each current-frame texel's surface at the
    // texel it occupies at the generated instant; a pixel whose word names a
    // surface depth_nearer_percent nearer than its answer solves once from
    // it (smaller = more solves = costlier); a nearer surface whose frames
    // agree within depth_agree replaces the answer (no cost). The menu's
    // "Hybrid: depth search" sets depth_search; the window does not.
    std::uint32_t depth_search;                   // 0 off, 1 on
    std::uint32_t depth_nearer_percent;           // 0-50, default 50
    std::uint32_t depth_agree_1000ths;            // 0-250, default 250

    // V539: generated frames tinted by what the hybrid composition did at
    // each pixel (1) or by the coverage mask's verdict (2); 0 off. A
    // diagnostic for the headset, never saved: off at every tray start.
    std::uint32_t hybrid_tint;                    // 0 off, 1 branches, 2 mask

    // V543: the coverage mask's band. Each moving surface's footprint is
    // drawn along its own motion, either way, by this share of it (at least
    // a texel): a picture slightly off its vectors - R.E.A.L. VR showing an
    // eye between two of its evaluations, render-resolution vectors, an
    // anti-aliased fringe - is off by more the faster the surface, and the
    // background beside it was taken for the surface's own and kept the
    // blend, the ghost at a fast edge.
    std::uint32_t mask_band_percent;              // 0-50 (0 V539's), default 0
};

// The composition's built-in values (frame_synthesis.hlsl), so the window
// opens on exactly what runs without it: the set the user settled on in
// DI2 on 2026-10-19 (V507's first session), which cost 0.95 ms per
// generated frame over plain optical flow there against 3.4 ms for the
// values V505 shipped with - except one edge surface solve where that set
// had none: without it the revealing edge of the hybrid test scene went
// from 1.3 back to 32.5 (5.2 with one solve, the same as with two).
inline constexpr Block kDefaults{
    kMagic, kVersion, 0U,
    62U, 8U, 3U, 8U,
    100U, 1U, 50U, 0U, 0U,
    1U, 50U, 250U,
    0U,
    0U,
};

// The block's values within their ranges, as the shader expects them.
[[nodiscard]] inline Block clamped(Block block) noexcept {
    block.hybrid_agree_threshold_255ths = std::min(block.hybrid_agree_threshold_255ths, 62U);
    block.hybrid_choice_slope = std::clamp(block.hybrid_choice_slope, 1U, 15U);
    block.hybrid_vectors_slope = std::clamp(block.hybrid_vectors_slope, 1U, 15U);
    block.hybrid_flow_rule_slope = std::clamp(block.hybrid_flow_rule_slope, 1U, 15U);
    block.edge_reach_percent = std::clamp(block.edge_reach_percent, 100U, 200U);
    block.edge_search_solves = std::min(block.edge_search_solves, 4U);
    block.edge_surface_agree_1000ths = std::min(block.edge_surface_agree_1000ths, 250U);
    block.revealed_error_1000ths = std::min(block.revealed_error_1000ths, 250U);
    block.vector_snap_tenths = std::min(block.vector_snap_tenths, 40U);
    block.depth_search = block.depth_search != 0U ? 1U : 0U;
    block.depth_nearer_percent = std::min(block.depth_nearer_percent, 50U);
    block.depth_agree_1000ths = std::min(block.depth_agree_1000ths, 250U);
    block.hybrid_tint = std::min(block.hybrid_tint, 2U);
    block.mask_band_percent = std::min(block.mask_band_percent, 50U);
    return block;
}

// The fields after the header, in order: what the shader indexes and what the
// 813 record packs, 8 bits a value (every range fits; since V555 all of them
// fit 813's sixteen).
inline constexpr std::size_t kFieldCount = 14;
[[nodiscard]] inline const std::uint32_t* fields(const Block& block) noexcept {
    return &block.hybrid_agree_threshold_255ths;
}
static_assert(sizeof(Block) == sizeof(std::uint32_t) * (3 + kFieldCount));
static_assert(offsetof(Block, hybrid_agree_threshold_255ths) == 12);
static_assert(offsetof(Block, mask_band_percent) == 12 + 4 * (kFieldCount - 1));

}  // namespace xrfg::live_tuning
