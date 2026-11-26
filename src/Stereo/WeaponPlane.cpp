#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// EVERY DRAW ITEM THROUGH THE SCENE PLANE.
//
// The first-person plane (ERenderingPlane::RPl_Weapon) is not a property the renderer reads off a
// mesh -- it is a per-item bit that selects which of the view's parameter blocks the item is drawn
// with. Measured by disassembly 2026-09-02: the multiply by the block stride 0x3A0 occurs exactly
// ONCE in the whole executable, in sub_14033DDA4, and the branch above it is the entire mechanism
// (see PLANE_ROUTE_BRANCH_RVA in EngineRvas.hpp for the four instructions).
//
// WHY THE WEAPON PLANE EXISTS, so the trade is made with open eyes:
//
//   * its own near plane. The gun sits 20-40 cm from the eye. Read out of the capture's depth
//     target, the whole first-person plane -- forearms and weapon alike -- lands in a narrow band
//     around 0.906, while a scene object at 30 cm would write about 0.33 in the same reversed-Z
//     buffer. That remap is what stops the gun clipping through the near plane and z-fighting.
//   * it never intersects the world, so the barrel does not poke through a wall you stand against.
//   * its own field of view, independent of the camera's -- which matters doubly in this port,
//     where the camera FOV is forced past 100.
//
// WHAT ACTUALLY HAPPENS WHEN THIS IS ON -- looked at on screen 2026-09-02, and it is NOT what the
// paragraph above predicts. Nothing changes: the weapon gains no world depth, it is not cut by the
// near plane, its field of view does not move. So the 0x3A0 block this branch selects does not carry
// the projection, and the first-person plane is separated somewhere else. Where that separation is
// visible: the early 67-draw pass that draws the arms and the gun (events 17404..17871 in
// outlinedmagazineandweaponnotoutlined.rdc), whose depth all lands in a narrow band around 0.906
// while a scene object at 30 cm would write about 0.33 in the same reversed-Z buffer. What buckets a
// draw item into that pass is the open question.
//
// The knob is kept because it is proven, reversible and free, and because it is the only lever on
// plane routing this binary has -- but it is not, on this evidence, a way to move the weapon into
// the scene.
//
// It is therefore a LIVE KEY WITH A DEFAULT OF 0. Off, this file touches nothing at all: the branch
// is left exactly as the game shipped it, and the patch is reverted the moment the key goes back to
// zero, so the comparison can be made in one session without a relaunch.
//
// The patch is validated before it is written and before it is reverted -- if the bytes are not the
// ones this file was written against (a game update, another mod ahead of us), nothing is touched
// and the reason is logged once. Never patch an instruction you have not just read.
#include <windows.h>

#include <cstdint>
#include <cstring>

#include "Stereo/EngineRvas.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Utils/StereoLog.hpp"

// 0 = the game's own routing, first-person plane intact (DEFAULT).
// 1 = every draw item takes the scene branch, so the first-person plane is drawn with the scene's
//     parameter block. Live: flipping the key applies or reverts the single byte in place.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_WeaponPlaneAsScene = 0;
#endif
// Last result, readable from the debugger and the overlay:
//   0 untouched   1 patched   -1 bytes did not validate   -2 VirtualProtect refused
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DebugWeaponPlaneState = 0;
#endif

// THE HIGHLIGHT'S OWN PLANE FILTER, which is a different thing from the routing above and the one
// that actually decides whether a held object can be outlined.
//
// Measured, not assumed: with a katana in the hand and one variable changed at a time, the whole
// gameplay half is plane-blind -- the renderer's proxy predicate accepts 4 of 4 in both planes, and
// the highlight record is written 4 times in both. The claim is stored either way. What differs is
// one byte on the render proxy, +0x9D, which holds the raw plane (2 for the weapon plane, 0 for the
// scene), and the collection at PLANE_FILTER_JNZ_RVA drops every object whose byte does not match
// the plane being gathered.
//
// 0 = the game's own filter (DEFAULT). 1 = the jnz becomes a six-byte nop, so no object is dropped
// for its plane. Live, and validated against the exact bytes before every write.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_HighlightAnyPlane = 0;
#endif
//   0 untouched   1 patched   -1 bytes did not validate   -2 VirtualProtect refused
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DebugHighlightAnyPlane = 0;
#endif

// EVERY DRAW ITEM INTO THE SCENE BUCKET. This is the one that actually moves the first-person plane,
// as opposed to the two knobs above, which were aimed at gates that turned out not to exist.
//
// The plane is a draw-list index, not a filter (see PLANE_BUCKET_LOAD_RVA). Loading zero instead of
// the proxy's plane puts every item on the scene pile, which is the pile the highlight gather walks --
// so a weapon in the hand becomes outlineable without touching a single item.
//
// What it costs is not known from measurement and should not be guessed at: forcing the scene
// parameter block (the routing knob above) changed nothing on screen, and moving one katana to the
// scene plane by event looked fine, so the usual first-person-plane arguments -- wall clipping, near
// plane, its own field of view -- are not evidenced here. Default 0 all the same, because this moves
// every draw in the frame and that deserves a deliberate switch.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_PlaneBucketScene = 0;
#endif
//   0 untouched   1 patched   -1 bytes did not validate   -2 VirtualProtect refused
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DebugPlaneBucketScene = 0;
#endif

#if CVR_RUST_VRIK
// THE PATCHES ARE RUST'S (rust/crates/cvr-render/src/plane_patches.rs): the switches and their debug states above stay
// exported here, and the live-controls poll's three syncs forward.
#else
namespace cvr {
namespace detail {

namespace {

constexpr uint8_t kOrigJz = 0x74;   // jz short
constexpr uint8_t kForced = 0xEB;   // jmp short -- same operand, same length
constexpr uint8_t kRel8   = 0x2B;   // the displacement to the scene branch, checked as a guard (2.31: 0x1E)

int32_t s_applied = 0;              // what this file has actually written, not what was asked for

uint8_t* branch_site() {
    uint8_t* base = g_exe_base;
    if (!base) base = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    if (!base) return nullptr;
    return base + PLANE_ROUTE_BRANCH_RVA;
}

// Writes `want` into the opcode byte, having first confirmed the site still holds what we expect.
bool write_opcode(uint8_t* site, uint8_t expect, uint8_t want) {
    if (site[0] != expect || site[1] != kRel8) {
        CyberpunkVR_DebugWeaponPlaneState = -1;
        Log("[stereo] [plane] branch at %p reads %02X %02X, expected %02X %02X -- NOT patched\n",
            site, site[0], site[1], expect, kRel8);
        return false;
    }
    DWORD oldp = 0;
    if (!VirtualProtect(site, 1, PAGE_EXECUTE_READWRITE, &oldp)) {
        CyberpunkVR_DebugWeaponPlaneState = -2;
        Log("[stereo] [plane] VirtualProtect refused at %p\n", site);
        return false;
    }
    site[0] = want;
    DWORD junk = 0;
    VirtualProtect(site, 1, oldp, &junk);
    FlushInstructionCache(GetCurrentProcess(), site, 2);
    return true;
}

// The plane filter is six bytes rather than one, so it gets its own pair of literals. `0F 85 rel32`
// is the conditional jump as shipped; `66 0F 1F 44 00 00` is the canonical six-byte nop.
const uint8_t kFilterOrig[6] = { 0x0F, 0x85, 0xE5, 0x00, 0x00, 0x00 };
const uint8_t kFilterNop[6]  = { 0x66, 0x0F, 0x1F, 0x44, 0x00, 0x00 };

int32_t s_filterApplied = 0;

// `40 8A B6 9D 00 00 00` is `mov sil, [rsi+0x9D]`; `40 B6 00` is `mov sil, 0` (flags untouched, like the load),
// padded with nops to the same seven bytes so nothing downstream shifts. (2.31: `44 8A 80 9D 00 00 00`,
// mov r8b, [rax+0x9D], replaced by xor r8d, r8d.)
const uint8_t kBucketOrig[7] = { 0x40, 0x8A, 0xB6, 0x9D, 0x00, 0x00, 0x00 };
const uint8_t kBucketZero[7] = { 0x40, 0xB6, 0x00, 0x90, 0x90, 0x90, 0x90 };

int32_t s_bucketApplied = 0;

bool write_block(uint8_t* site, const uint8_t* expect, const uint8_t* want, size_t n) {
    if (memcmp(site, expect, n) != 0) {
        CyberpunkVR_DebugHighlightAnyPlane = -1;
        Log("[stereo] [hlplane] site %p does not hold the expected bytes -- NOT patched\n", site);
        return false;
    }
    DWORD oldp = 0;
    if (!VirtualProtect(site, n, PAGE_EXECUTE_READWRITE, &oldp)) {
        CyberpunkVR_DebugHighlightAnyPlane = -2;
        Log("[stereo] [hlplane] VirtualProtect refused at %p\n", site);
        return false;
    }
    memcpy(site, want, n);
    DWORD junk = 0;
    VirtualProtect(site, n, oldp, &junk);
    FlushInstructionCache(GetCurrentProcess(), site, n);
    return true;
}

}  // namespace

// Sends every draw item to the scene bucket, or restores the game's own bucketing.
void plane_bucket_scene_sync() {
    const int32_t want = (CyberpunkVR_PlaneBucketScene != 0) ? 1 : 0;
    if (want == s_bucketApplied) return;
    uint8_t* base = g_exe_base;
    if (!base) base = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    if (!base) return;
    uint8_t* site = base + PLANE_BUCKET_LOAD_RVA;
    if (want) {
        if (!write_block(site, kBucketOrig, kBucketZero, 7)) return;
        s_bucketApplied = 1;
        CyberpunkVR_DebugPlaneBucketScene = 1;
        Log("[stereo] [bucket] xr_plane_bucket_scene=1: every draw item now goes to the SCENE bucket "
            "at %p -- the first-person plane no longer has a list of its own.\n", site);
    } else {
        if (!write_block(site, kBucketZero, kBucketOrig, 7)) return;
        s_bucketApplied = 0;
        CyberpunkVR_DebugPlaneBucketScene = 0;
        Log("[stereo] [bucket] xr_plane_bucket_scene=0: the game's own plane bucketing restored "
            "at %p.\n", site);
    }
}

// Applies or reverts the per-object plane filter, same contract as the routing knob below.
void highlight_any_plane_sync() {
    const int32_t want = (CyberpunkVR_HighlightAnyPlane != 0) ? 1 : 0;
    if (want == s_filterApplied) return;
    uint8_t* base = g_exe_base;
    if (!base) base = reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    if (!base) return;
    uint8_t* site = base + PLANE_FILTER_JNZ_RVA;
    if (want) {
        if (!write_block(site, kFilterOrig, kFilterNop, 6)) return;
        s_filterApplied = 1;
        CyberpunkVR_DebugHighlightAnyPlane = 1;
        Log("[stereo] [hlplane] xr_highlight_any_plane=1: the per-object plane filter at %p is a nop, "
            "so objects on the first-person plane are no longer dropped from the gather.\n", site);
    } else {
        if (!write_block(site, kFilterNop, kFilterOrig, 6)) return;
        s_filterApplied = 0;
        CyberpunkVR_DebugHighlightAnyPlane = 0;
        Log("[stereo] [hlplane] xr_highlight_any_plane=0: the game's own plane filter restored "
            "at %p.\n", site);
    }
}

// Called from the live-controls poll. Does nothing on the overwhelming majority of ticks: it only
// acts when the requested state differs from the state this file last wrote.
void weapon_plane_sync() {
    const int32_t want = (CyberpunkVR_WeaponPlaneAsScene != 0) ? 1 : 0;
    if (want == s_applied) return;

    uint8_t* site = branch_site();
    if (!site) return;

    if (want) {
        if (!write_opcode(site, kOrigJz, kForced)) return;
        s_applied = 1;
        CyberpunkVR_DebugWeaponPlaneState = 1;
        Log("[stereo] [plane] xr_weapon_plane_as_scene=1: draw items now ALL take the scene "
            "parameter block (jz -> jmp at %p). Observed on screen: nothing changes -- no depth, "
            "no field of view. This block is not what separates the first-person plane.\n", site);
    } else {
        if (!write_opcode(site, kForced, kOrigJz)) return;
        s_applied = 0;
        CyberpunkVR_DebugWeaponPlaneState = 0;
        Log("[stereo] [plane] xr_weapon_plane_as_scene=0: the game's own plane routing restored "
            "at %p.\n", site);
    }
}

}  // namespace detail
}  // namespace cvr
#endif
#endif
