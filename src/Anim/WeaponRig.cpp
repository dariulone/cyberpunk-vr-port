// WeaponRig -- the WEAPON's bones, as against the character's.
//
// The pose-apply detour sees two skeletons through the same function, and they are told apart by ONE
// test: whose track buffer is this. The character's buffers are g_PlayerTrackBufA/B; anything else with
// a pose descriptor is a weapon rig. The two branches are therefore mutually exclusive BY CONSTRUCTION,
// not by convention, which is what made this file separable at all.
//
// THE CONDITIONS DID NOT MOVE. All three guards stay written out in src/Hooks/AnimPose.cpp, verbatim,
// and only the bodies live here. They are not interchangeable and one of them is not even the same
// shape as the others:
//
//   rig identify + write   trackBuf is NOT either player buffer
//   census                 the same, AND g_PoseCensusOn
//   capture parts          g_WeaponRigActive AND trackBuf IS g_WeaponTrackBufA or B
//
// A cut that kept the braces balanced and moved a condition along with its body would have compiled,
// linked and run -- and a weapon-bone write that fires when it must not is a held object in the wrong
// place, not an error. So the guards were left where the engine's own control flow can be read next to
// them, and each function below is entered only when its caller has already decided it should be.
//
// SEH IS DYNAMIC, which is the fact that made this safe. The detour's __except covers everything called
// from inside its __try, including these functions in another translation unit, so none of them needs a
// __try of its own -- and MSVC would have refused to give them one alongside C++ unwinding anyway.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include "Anim/WeaponRig.hpp"
#include "Anim/RigTrackMapping.hpp"

extern uintptr_t g_gameModuleBase;   // src/Core/VrCore.cpp, filled by InitGameModuleInfo at startup
extern size_t    g_gameModuleSize;

namespace cvr {
namespace anim {

// THE GAME'S SHARED EMPTY TRACK BUFFER. A rig with no float tracks of its own is handed one static buffer in the
// executable's read-only data, while its rig object can still report a count at +0xA0 -- the Tsunami Ashura's
// 3-bone round rig reports 3. Reading that buffer is harmless; the first hidden-and-held track written into it was
// an access violation at CyberpunkVR_Stereo.dll+0x2CDBF, `mov [r14+rcx*4]` into Cyberpunk2077.exe+0x35A1F10. So a
// track buffer inside the main image has no tracks, whatever the rig says. Range compares only: no VirtualQuery on
// this path (see below).
static uint32_t RigTrackCount(const uint8_t* rig, uintptr_t trackBuf) {
    if (g_gameModuleBase && trackBuf >= g_gameModuleBase && trackBuf < g_gameModuleBase + g_gameModuleSize) return 0u;
    return *reinterpret_cast<const uint32_t*>(rig + 0xA0);
}


// IDENTIFY THE RIG BY BONE NAME, then write the reload module's tracks onto it.
//
// a1[8] IS THE RIG OBJECT -- established in the debugger, not inferred -- so the names it carries at
// rig+0x50 say which rig this pass belongs to. Four other criteria were tried first and all four
// misclassified: bone COUNT, call FREQUENCY, buffer SIZE and arrival ORDER. Names are the only thing
// that separates two rigs that differ in nothing else.
//
// NO VirtualQuery ON THIS PATH. VRIK_IsReadable calls it, and three of those per pass on a function the
// engine invokes ~10k times a second took the game to 4 fps. The CALLER'S __try is the guard -- SEH is
// dynamic, so the detour's __except covers everything called from inside it, including this function in
// another translation unit. That is why this extraction needed no __try of its own.
void WeaponRigIdentifyAndWrite(void** a1, void** a2, unsigned int a4, void* poseDesc,
                               uint8_t* boneBuf, uintptr_t trackBuf) {
                int which = -1;
                // Track addresses and bone counts can be reused by another gun.
                // Validate the active signature every pass; only two weapon
                // signatures are registered now, so this stays bounded/cheap.
                uint32_t nb = 0;

                if (which < 0) {
                    uint8_t* rig = reinterpret_cast<uint8_t**>(a1)[8];
                    if (rig) {
                        const uint32_t n = *reinterpret_cast<uint32_t*>(rig + 0x58);
                        uint64_t* names = *reinterpret_cast<uint64_t**>(rig + 0x50);
                        // MATCH AGAINST THE REGISTERED SIGNATURES. Bone count first (cheap and structural), then the
                        // named bones at their indices -- a rig that agrees on both is that weapon's. Nothing about
                        // any particular gun is compiled in; the Silverhand's own signature is registered at startup
                        // like everyone else's, and a weapon config adds its own through VRRigSignature.
                        if (names && n > 0) {
                            const int sn = g_RigSigN;
                            for (int s = 0; s < sn && s < VRRIG_SIG_MAX && which < 0; ++s) {
                                if (g_RigSigBones[s] != n) continue;
                                bool ok = true;
                                for (int k = 0; k < VRRIG_SIG_NAMES; ++k) {
                                    const int bi = g_RigSigIdx[s][k];
                                    if (bi < 0) continue;                       // slot unused
                                    if (static_cast<uint32_t>(bi) >= n || names[bi] != g_RigSigHash[s][k]) {
                                        ok = false; break;
                                    }
                                }
                                if (ok) which = g_RigSigWhich[s];
                            }
                            if (which >= 0) {
                                nb = n;
                                g_RigBuf[which] = trackBuf;
                                g_RigBones[which] = n;
                                g_RigTracks[which] = RigTrackCount(rig, trackBuf);
                            }
                        }
                    }
                }

                if (which >= 0 && nb > 0) {
                    ++g_RigSeen[which];
                    // RESOLVE EACH BONE'S REAL SLOT THROUGH THE LIVE REMAP TABLE. The frame and magazine rigs share
                    // one 22-slot pose buffer, and a bone's slot is the remap DST, not its rig index: front_slider
                    // is rig bone 5 but lives in pose slot 7. Writing the rig index hit an invisible FX anchor
                    // (fx_muzzle at slot 5) -- proven in x64dbg, and the whole reason nothing moved. Layout from the
                    // disassembly: a2[6]+0x40 = base of (srcIdx u32, dstIdx u32) pairs, a2[6]+0x50 = u16 counts
                    // indexed by a4 (the LOD); poseDesc+0x08 = slot count of this buffer. src == rig bone index.
                    uint8_t*  six   = reinterpret_cast<uint8_t**>(a2)[6];
                    uint32_t* remap = six ? *reinterpret_cast<uint32_t**>(six + 0x40) : nullptr;
                    uint16_t* rcnt  = six ? *reinterpret_cast<uint16_t**>(six + 0x50) : nullptr;
                    const uint32_t rn    = rcnt ? rcnt[a4] : 0u;
                    const uint32_t slots = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(poseDesc) + 0x08);
                    // Per-a4 telemetry for the frame rig: how often each pass kind runs, how big its remap is,
                    // and where it puts the first write's bone. This is what named the pinning bug from one log
                    // read (slot=-1 passes + a base latched at a foreign bone's rest).
                    if (which == 1) {
                        const unsigned lod = (a4 < 4u) ? a4 : 3u;
                        ++g_RigPassSeen[lod];
                        g_RigPassRn[lod] = static_cast<int>(rn);
                        g_RigPassSlots[lod] = static_cast<int>(slots);
                        g_RigPassBufLo[lod] = static_cast<int>(reinterpret_cast<uintptr_t>(boneBuf) & 0x7FFFFFFFu);
                        if (lod == 0 && remap) {
                            const uint32_t m = (rn < 4u) ? rn : 4u;
                            for (int e = 0; e < 8; ++e) g_RigPassMap0[e] = -1;
                            for (uint32_t e = 0; e < m; ++e) {
                                g_RigPassMap0[e * 2]     = static_cast<int>(remap[e * 2]);
                                g_RigPassMap0[e * 2 + 1] = static_cast<int>(remap[e * 2 + 1]);
                            }
                        }
                        if (g_RigWriteN > 0) {
                            const int b0 = g_RigWriteBone[0];
                            int d0 = -1;
                            if (remap && rn && b0 >= 0) {
                                for (uint32_t e = 0; e < rn; ++e) {
                                    if (remap[e * 2] == static_cast<uint32_t>(b0)) { d0 = static_cast<int>(remap[e * 2 + 1]); break; }
                                }
                            }
                            g_RigPassDst[lod] = d0;
                        }
                    }
                    // READ-BACK first, before any write of ours: resolve each rig bone through this pass's remap
                    // and copy its parent-local transform out. What lands here is the GAME's own value, which is
                    // what makes recording a native reload possible (see g_RigPose).
                    if (remap && rn) {
                        for (uint32_t e = 0; e < rn; ++e) {
                            const uint32_t src = remap[e * 2], d2 = remap[e * 2 + 1];
                            if (src >= 20u || d2 >= slots) continue;
                            const float* t = reinterpret_cast<const float*>(boneBuf + d2 * 48 + VRIK_TRANS_OFF);
                            const float* q = reinterpret_cast<const float*>(boneBuf + d2 * 48 + VRIK_ROT_OFF);
                            volatile float* o = g_RigPose[which][src];
                            o[0] = t[0]; o[1] = t[1]; o[2] = t[2];
                            o[3] = q[0]; o[4] = q[1]; o[5] = q[2]; o[6] = q[3];
                        }
                        g_RigPoseHave[which] = 1;
                    }
                    // 2.50 RVA 0x1B3D13..0x1B3D39: float tracks use their OWN
                    // (source,destination) remap at six+0x60, LOD counts +0x70.
                    // The rig's source count is NOT the destination capacity.
                    const uint32_t* trackMap = six ? *reinterpret_cast<uint32_t**>(six + 0x60) : nullptr;
                    const uint16_t* trackCounts = six ? *reinterpret_cast<uint16_t**>(six + 0x70) : nullptr;
                    const uint32_t trackPairs = trackCounts ? trackCounts[a4] : 0u;
                    const uint32_t trackCapacity = *reinterpret_cast<uint32_t*>(
                        reinterpret_cast<uint8_t*>(poseDesc) + 0x20);
                    g_RigTrackHave[which] = ApplyRigTracks(trackMap, trackPairs,
                        g_RigTracks[which], reinterpret_cast<float*>(trackBuf), trackCapacity,
                        g_RigTrackVal[which], g_RigTrackSet[which], g_RigTrackOn[which], VRRIG_TRACKS);
                    const int wn = g_RigWriteN;
                    for (int k = 0; k < wn && k < VRRIG_WRITES; ++k) {
                        if (g_RigWriteWhich[k] != which) continue;
                        const int bi = g_RigWriteBone[k];      // rig bone index (front_slider 5, mag_std 3)
                        if (bi < 0) continue;
                        if (!g_RigWriteEnabled[k]) continue;   // released -- the animation's pose stands untouched
                        int dst = -1;
                        if (remap && rn) {
                            for (uint32_t e = 0; e < rn; ++e) {
                                if (remap[e * 2] == static_cast<uint32_t>(bi)) { dst = static_cast<int>(remap[e * 2 + 1]); break; }
                            }
                        }
                        g_RigWriteSlot[k] = dst;
                        // Later LOD passes can omit a bone after its local pose
                        // was copied. Resolve through THIS component's LOD-0 map;
                        // never borrow a slot cached from another gun/component.
                        if(dst<0 && remap && rcnt && g_RigWriteAbs[k]) {
                            const uint32_t full=rcnt[0];
                            if(full<=nb)for(uint32_t e=0;e<full;++e)
                                if(remap[e*2]==static_cast<uint32_t>(bi)){dst=static_cast<int>(remap[e*2+1]);break;}
                        }
                        g_RigWriteSlot[k]=dst;
                        if(a4==0 && dst>=0)g_RigWritePin[k]=dst; // diagnostics only
                        if (dst < 0 || static_cast<uint32_t>(dst) >= slots) continue;
                        float* t = reinterpret_cast<float*>(boneBuf + dst * 48 + VRIK_TRANS_OFF);
                        // Capture the rest value the first time this bone is seen, BEFORE writing it. The pose copy
                        // re-establishes this slot from the source every pass (it IS in the remap now), so on first
                        // sight t holds the game's rest local pose. From then on t = base + offset every pass: it
                        // overrides the animation with our controlled pose, is idempotent and bounded, and offset 0
                        // returns the part to rest by itself.
                        if (!g_RigWriteHaveBase[k] && !g_RigWriteAbs[k]) {
                            // BASE SANITY: a weapon part's parent-local translation is centimetres (back_slider
                            // rest is 0.22 m). A magnitude beyond 0.6 m means the slot held a WORLD-stage value
                            // when we looked (see the no-fallback note above) -- latching it would poison every
                            // later write, so wait for a pass where the slot is freshly local.
                            const float bm2 = t[0]*t[0] + t[1]*t[1] + t[2]*t[2];
                            if (!std::isfinite(bm2) || bm2 > 0.36f) continue;
                            g_RigWriteBase[k][0] = t[0];
                            g_RigWriteBase[k][1] = t[1];
                            g_RigWriteBase[k][2] = t[2];
                            g_RigWriteHaveBase[k] = 1;
                        }
                        float ox = g_RigWriteOff[k][0];
                        float oy = g_RigWriteOff[k][1];
                        float oz = g_RigWriteOff[k][2];
                        if (ox >  VRRIG_OFF_MAX) ox =  VRRIG_OFF_MAX;
                        if (ox < -VRRIG_OFF_MAX) ox = -VRRIG_OFF_MAX;
                        if (oy >  VRRIG_OFF_MAX) oy =  VRRIG_OFF_MAX;
                        if (oy < -VRRIG_OFF_MAX) oy = -VRRIG_OFF_MAX;
                        if (oz >  VRRIG_OFF_MAX) oz =  VRRIG_OFF_MAX;
                        if (oz < -VRRIG_OFF_MAX) oz = -VRRIG_OFF_MAX;
                        if (g_RigWriteAbs[k]) {
                            // replay: the offset fields carry the game's own local values, write them as they are
                            t[0] = g_RigWriteOff[k][0];
                            t[1] = g_RigWriteOff[k][1];
                            t[2] = g_RigWriteOff[k][2];
                        } else {
                            t[0] = g_RigWriteBase[k][0] + ox;
                            t[1] = g_RigWriteBase[k][1] + oy;
                            t[2] = g_RigWriteBase[k][2] + oz;
                        }
                        ++g_RigWriteApplied[k];

                        // absolute ROTATION, same reason
                        if (g_RigWriteQuatOn[k]) {
                            float* q = reinterpret_cast<float*>(boneBuf + dst * 48 + 16);
                            q[0] = g_RigWriteQuat[k][0]; q[1] = g_RigWriteQuat[k][1];
                            q[2] = g_RigWriteQuat[k][2]; q[3] = g_RigWriteQuat[k][3];
                        }

                        // ROTATION offset, for parts that SPIN rather than slide (the rotator disc, the hammer).
                        // Composed onto the bone's own rest rotation in its LOCAL frame, so angle 0 leaves it be.
                        // The QsTransform quaternion is at +16 (x,y,z,w) in the 48-byte slot.
                        if (g_RigWriteRotAngle[k] != 0.0f) {
                            float* q = reinterpret_cast<float*>(boneBuf + dst * 48 + 16);
                            if (!g_RigWriteHaveBaseRot[k]) {
                                g_RigWriteBaseRot[k][0] = q[0]; g_RigWriteBaseRot[k][1] = q[1];
                                g_RigWriteBaseRot[k][2] = q[2]; g_RigWriteBaseRot[k][3] = q[3];
                                g_RigWriteHaveBaseRot[k] = 1;
                            }
                            const float hh = g_RigWriteRotAngle[k] * 0.00872664626f;   // deg -> rad, halved
                            const float s = sinf(hh), c = cosf(hh);
                            const float qx = g_RigWriteRotAxis[k][0] * s, qy = g_RigWriteRotAxis[k][1] * s,
                                        qz = g_RigWriteRotAxis[k][2] * s, qw = c;
                            const float bx = g_RigWriteBaseRot[k][0], by = g_RigWriteBaseRot[k][1],
                                        bz = g_RigWriteBaseRot[k][2], bw = g_RigWriteBaseRot[k][3];
                            q[0] = bw * qx + bx * qw + by * qz - bz * qy;   // new = base * offset (local spin)
                            q[1] = bw * qy - bx * qz + by * qw + bz * qx;
                            q[2] = bw * qz + bx * qy - by * qx + bz * qw;
                            q[3] = bw * qw - bx * qx - by * qy - bz * qz;
                        }

                        // SCALE write, the no-tracks visibility switch (see g_RigWriteScale's comment).
                        if (g_RigWriteScale[k] > 0.0f) {
                            float* sv = reinterpret_cast<float*>(boneBuf + dst * 48 + 32);
                            sv[0] = g_RigWriteScale[k];
                            sv[1] = g_RigWriteScale[k];
                            sv[2] = g_RigWriteScale[k];
                        }
                    }
                }
}

// THE CENSUS -- the abandoned statistical route to rig identification, left gated off.
//
// It cost two dead ends and it SATURATED TWICE, which is why identification is by name instead. Kept
// because a saturated table that says so is evidence, and because turning it on is how the next
// unfamiliar rig gets characterised.
void WeaponRigCensusNote(unsigned int a4, uintptr_t trackBuf) {
                int slot = -1;
                const int n = g_PoseCensusN;
                for (int k = 0; k < n; ++k) {
                    if (g_PoseCensusBuf[k] == trackBuf && g_PoseCensusA4[k] == a4) { slot = k; break; }
                }
                if (slot < 0) {
                    if (n < VRPOSE_CENSUS_MAX) {
                        slot = n;
                        g_PoseCensusBuf[slot] = trackBuf;
                        g_PoseCensusA4[slot] = a4;
                        g_PoseCensusHits[slot] = 0;
                        g_PoseCensusHitsOut[slot] = 0;
                        g_PoseCensusHitsIn[slot] = 0;
                        g_PoseCensusArmed[slot] = uint8_t(g_PoseCensusWeaponOut ? 1 : 0);
                        g_PoseCensusN = n + 1;
                    } else {
                        g_PoseCensusFull = 1;
                    }
                }
                if (slot >= 0) {
                    ++g_PoseCensusHits[slot];
                    if (g_PoseCensusWeaponOut) ++g_PoseCensusHitsOut[slot];
                    else                       ++g_PoseCensusHitsIn[slot];
                }
}

// THE WEAPON'S OWN PASS: capture every part's local transform, and optionally offset it.
//
// A different skeleton means a different track buffer, so this runs on its own condition rather than
// as an else-branch. g_WeaponPartWriteOn is THE PROOF mechanism: offset a part's local translation and
// watch it move on screen. That is how the hand-bone writes were proven in this project, and nothing
// short of it settles whether the parts of a held weapon can be driven from here.
void WeaponRigCaptureParts(uint8_t* boneBuf) {
                ++g_WeaponMatchCalls;
                const int n = g_WeaponPartCount;
                for (int k = 0; k < n && k < 24; ++k) {
                    const int bi = g_WeaponPartIdx[k];
                    if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                    const float* t = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                    const float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                    g_WeaponPartPos[k][0] = t[0]; g_WeaponPartPos[k][1] = t[1]; g_WeaponPartPos[k][2] = t[2];
                    g_WeaponPartRot[k][0] = q[0]; g_WeaponPartRot[k][1] = q[1];
                    g_WeaponPartRot[k][2] = q[2]; g_WeaponPartRot[k][3] = q[3];
                }
                g_WeaponPartHave = 1;

                // THE PROOF. Offset a part's local translation and see it move on screen: that is how the hand
                // bone writes were proven in this project, and nothing short of it settles whether the parts of
                // a held weapon can be driven from here.
                if (g_WeaponPartWriteOn) {
                    for (int k = 0; k < n && k < 24; ++k) {
                        const int bi = g_WeaponPartIdx[k];
                        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                        const float ox = g_WeaponPartOff[k][0];
                        const float oy = g_WeaponPartOff[k][1];
                        const float oz = g_WeaponPartOff[k][2];
                        if (ox == 0.0f && oy == 0.0f && oz == 0.0f) continue;
                        float* t = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_TRANS_OFF);
                        t[0] += ox; t[1] += oy; t[2] += oz;
                    }
                }
}

}  // namespace anim
}  // namespace cvr
