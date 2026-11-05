// LeftHanded -- the weapon in the LEFT hand (xr_left_handed; the buttons are include/Hooks/Handedness.hpp).
//
// The weapon IS the left hand's: drawn on its own it is given to the game's left hand
// (mods/redscript/CyberpunkVRPort_LeftHanded), so it hangs on AttachmentSlots.WeaponLeft and runs the game's own
// left-hand chain -- the one dual wielding uses: its shoot state, reload and ammo. The game's active weapon is that
// one too (GetActiveWeapon falls back to the left slot), so the muzzle, the sight line, the optics and the shot all
// follow it as they follow a weapon in the right hand.
//
// THE HOLD IS THE ANIMATION'S. What the left hand does with the weapon -- the grip, the weapon's seat in the hand,
// the squeeze of every round, the draw, the reload -- is the right hand's single-wield animation of that weapon,
// mirrored onto the left hand offline (tools/lefthanded_anims) and played by the game's own graph
// (src/Hooks/LeftHandedAnimation.cpp). Nothing here touches the weapon hand's fingers or its slot.
//
// What the game has no left-handed version of either is the port's own: the support hand on the gun, the recoil and
// the haptics of the hand holding it, the hand that holsters and reloads. Those ask WeaponInLeftHand(), and the
// support hand's hold -- captured off the right-handed animation, the left wrist in the right wrist's frame -- is
// mirrored here onto the other hand.
//
// THE MIRROR IS THE RIG'S, read from its reference pose rather than assumed. A right bone and its left twin are
// mirror images across the body's midplane (model X), each frame up to a reflection of its own axes:
//
//     R_left = M * R_right * S        M = diag(-1, 1, 1),  S a signed permutation, det -1
//
// S is solved per twin from the reference pose and snapped to the nearest signed permutation; a twin that is not
// a mirror within tolerance is left out. This rig's twins are behaviour-mirrored (S = -I for the hands and the
// fingers: one local rotation curls both hands alike), and nothing here relies on that. A child's parent-local
// rotation mirrors as S_parent^T * local * S, its parent-local offset as S_parent^T * offset.

#include "Anim/LeftHanded.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Core/VrCoreShared.hpp"   // g_hasRightWeaponEquipped, g_hasLeftWeaponEquipped

#include <cmath>
#include <cstdio>
#include <cstring>
#include <string_view>

extern void Log(const char* fmt, ...);   // the ungated project logger
extern "C" __declspec(dllexport) extern int CyberpunkVR_CarryLeft;  // Anim/TwoHandGrip.cpp

// Twins found when the rig was armed (fingers, then 1 for the hands), and the worst reflection residual among them
// -- the largest entry by which a solved S missed its snapped permutation.
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugLeftHandedTwins = 0;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugLeftHandedResidual = 0.0f;
// How far each hand's fingers are curled off the reference pose, mean degrees: [0..1] left/right as the animation
// left them, [2..3] as drawn after every finger layer.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugFingerFlex[4] = {};
// Held up by the reload mod while the game reports a gun in each hand (src/Hooks/XInput.cpp).
extern "C" volatile uint64_t CyberpunkVR_DualWieldUntil;

namespace cvr::anim {
namespace {

struct Mat3 { float m[3][3]; };

Mat3 FromQuat(const float* q) {
    const float x = q[0], y = q[1], z = q[2], w = q[3];
    return {{{1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)},
             {2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)},
             {2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)}}};
}

void ToQuat(const Mat3& r, float* q) {
    const auto& m = r.m;
    const float trace = m[0][0] + m[1][1] + m[2][2];
    if (trace > 0.0f) {
        const float s = std::sqrt(trace + 1.0f) * 2.0f;
        q[3] = 0.25f * s; q[0] = (m[2][1] - m[1][2]) / s; q[1] = (m[0][2] - m[2][0]) / s; q[2] = (m[1][0] - m[0][1]) / s;
    } else if (m[0][0] > m[1][1] && m[0][0] > m[2][2]) {
        const float s = std::sqrt(1.0f + m[0][0] - m[1][1] - m[2][2]) * 2.0f;
        q[3] = (m[2][1] - m[1][2]) / s; q[0] = 0.25f * s; q[1] = (m[0][1] + m[1][0]) / s; q[2] = (m[0][2] + m[2][0]) / s;
    } else if (m[1][1] > m[2][2]) {
        const float s = std::sqrt(1.0f + m[1][1] - m[0][0] - m[2][2]) * 2.0f;
        q[3] = (m[0][2] - m[2][0]) / s; q[0] = (m[0][1] + m[1][0]) / s; q[1] = 0.25f * s; q[2] = (m[1][2] + m[2][1]) / s;
    } else {
        const float s = std::sqrt(1.0f + m[2][2] - m[0][0] - m[1][1]) * 2.0f;
        q[3] = (m[1][0] - m[0][1]) / s; q[0] = (m[0][2] + m[2][0]) / s; q[1] = (m[1][2] + m[2][1]) / s; q[2] = 0.25f * s;
    }
    VRIK_QuatNorm(q);
}

Mat3 Mul(const Mat3& a, const Mat3& b) {
    Mat3 o{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j)
            o.m[i][j] = a.m[i][0] * b.m[0][j] + a.m[i][1] * b.m[1][j] + a.m[i][2] * b.m[2][j];
    return o;
}

Mat3 Transposed(const Mat3& a) {
    Mat3 o{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) o.m[i][j] = a.m[j][i];
    return o;
}

void Apply(const Mat3& a, const float* v, float* o) {
    for (int i = 0; i < 3; ++i) o[i] = a.m[i][0] * v[0] + a.m[i][1] * v[1] + a.m[i][2] * v[2];
}

// a * R * b on a rotation given as a quaternion: what every mirrored rotation here is.
void Conjugated(const Mat3& a, const float* q, const Mat3& b, float* out) {
    ToQuat(Mul(Mul(a, FromQuat(q)), b), out);
}

struct Twin {
    int right = -1, left = -1;
    Mat3 s{};            // R_left = M * R_right * s
    int parent = -1;     // fingers: the twin of their parents, -1 = the hands
};

constexpr int kMaxFingers = 32;

Twin g_hands;
Twin g_fingers[kMaxFingers];
int  g_fingerCount = 0;
bool g_ready = false;
Twin g_weapons;              // WeaponRight / WeaponLeft, for a prop handed over (the smoking lighter)
bool g_weaponsReady = false;

bool StartsWith(std::string_view name, std::string_view prefix) {
    return name.size() >= prefix.size() && _strnicmp(name.data(), prefix.data(), prefix.size()) == 0;
}

bool Descends(int bone, int ancestor) {
    for (int guard = 0; bone >= 0 && bone < VRIK_MAX_BONES && guard < VRIK_MAX_BONES; ++guard) {
        if (bone == ancestor) return true;
        bone = g_VRBoneParent[bone];
    }
    return false;
}

// The reflection between a right bone's reference frame and its left twin's, snapped to a signed permutation;
// the residual is the largest entry the solved matrix missed it by, the mirror error the positions' miss.
bool SolveTwin(int right, int left, Twin& out, float* residual, float* mirrorError) {
    float pr[3], qr[4], pl[3], ql[4];
    if (!VRIK_ReferenceModel(right, pr, qr) || !VRIK_ReferenceModel(left, pl, ql)) return false;
    Mat3 ml = FromQuat(ql);
    for (int c = 0; c < 3; ++c) ml.m[0][c] = -ml.m[0][c];   // M * R_left
    const Mat3 raw = Mul(Transposed(FromQuat(qr)), ml);
    Mat3 snap{};
    bool used[3] = {};
    for (int i = 0; i < 3; ++i) {
        int best = 0;
        for (int j = 1; j < 3; ++j)
            if (std::fabs(raw.m[i][j]) > std::fabs(raw.m[i][best])) best = j;
        if (used[best]) return false;
        used[best] = true;
        snap.m[i][best] = raw.m[i][best] < 0.0f ? -1.0f : 1.0f;
    }
    const auto& s = snap.m;
    const float det = s[0][0] * (s[1][1] * s[2][2] - s[1][2] * s[2][1]) - s[0][1] * (s[1][0] * s[2][2] - s[1][2] * s[2][0]) +
                      s[0][2] * (s[1][0] * s[2][1] - s[1][1] * s[2][0]);
    if (det > 0.0f) return false;
    float worst = 0.0f;
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) worst = std::fmax(worst, std::fabs(raw.m[i][j] - snap.m[i][j]));
    const float dx = -pr[0] - pl[0], dy = pr[1] - pl[1], dz = pr[2] - pl[2];
    *residual = worst;
    *mirrorError = std::sqrt(dx * dx + dy * dy + dz * dz);
    out.right = right;
    out.left = left;
    out.s = snap;
    return true;
}

const Mat3& ParentReflection(const Twin& t) {
    return t.parent >= 0 ? g_fingers[t.parent].s : g_hands.s;
}

}  // namespace

bool WeaponInLeftHand() {
    // The only weapon in hand hangs on the left slot. A gun the carry moved there is still the right hand's, and an
    // implanted arm (the launcher lives on the left slot too) is anatomy, not a weapon held in a hand.
    if (!g_hasLeftWeaponEquipped || g_hasRightWeaponEquipped || CyberpunkVR_CarryLeft != 0) return false;
    const int kind = cvr::cyberware::SelectedKind();
    return kind < 1 || kind > 4;
}

int HeldWeaponBone() {
    // The rig names them for the smoking pose that rides the same two bones: the cigarette WeaponRight, the lighter
    // WeaponLeft (src/Natives/VrikArming.cpp).
    return WeaponInLeftHand() ? g_VRSmokeLighterIdx : g_VRSmokeCigIdx;
}

void LeftHandedArmRig(const char* const* names, int count) {
    g_ready = false;
    g_fingerCount = 0;
    g_hands = Twin{};
    g_weapons = Twin{};
    g_weaponsReady = false;
    if (!names || count <= 0) return;
    count = count < VRIK_MAX_BONES ? count : VRIK_MAX_BONES;
    auto find = [&](std::string_view wanted) {
        for (int i = 0; i < count; ++i)
            if (names[i] && wanted.size() == std::strlen(names[i]) && StartsWith(names[i], wanted)) return i;
        return -1;
    };
    float worst = 0.0f, worstMirror = 0.0f;
    auto solve = [&](int right, int left, Twin& out) {
        float residual = 0.0f, mirror = 0.0f;
        if (right < 0 || left < 0 || !SolveTwin(right, left, out, &residual, &mirror)) return false;
        // 0.2 is a frame eleven degrees off any mirror; 3 cm a twin that is not where the other one is reflected
        if (residual > 0.2f || mirror > 0.03f) return false;
        worst = std::fmax(worst, residual);
        worstMirror = std::fmax(worstMirror, mirror);
        return true;
    };
    const int rightHand = find("RightHand"), leftHand = find("LeftHand");
    if (!solve(rightHand, leftHand, g_hands)) {
        Log("[left-handed] rig has no mirrored hands: the support hand keeps its own holds\n");
        return;
    }
    // The fingers: every Right* deform bone under the right hand (no '_' -- those are control joints -- and no
    // *End tip), parents before children as the rig orders them, each with its Left* twin under the left hand.
    for (int bone = 0; bone < count && g_fingerCount < kMaxFingers; ++bone) {
        const char* name = names[bone];
        if (!name || bone == rightHand || !StartsWith(name, "Right") || std::strchr(name, '_') ||
            std::strstr(name, "End") || !Descends(bone, rightHand))
            continue;
        char twinName[96];
        std::snprintf(twinName, sizeof(twinName), "Left%s", name + 5);
        const int twin = find(twinName);
        if (twin < 0 || !Descends(twin, leftHand)) continue;
        Twin t;
        if (!solve(bone, twin, t)) continue;
        const int parentRight = g_VRBoneParent[bone];
        t.parent = -1;
        if (parentRight != rightHand) {
            for (int k = 0; k < g_fingerCount; ++k)
                if (g_fingers[k].right == parentRight) t.parent = k;
            if (t.parent < 0 || g_fingers[t.parent].left != g_VRBoneParent[twin]) continue;
        } else if (g_VRBoneParent[twin] != leftHand) {
            continue;
        }
        g_fingers[g_fingerCount++] = t;
    }
    // The weapon bones hang off the hands (or a finger of them) like the fingers do: the same test of their parents.
    const int rightWeapon = find("WeaponRight"), leftWeapon = find("WeaponLeft");
    float weaponResidual = 0.0f, weaponMirror = 0.0f;
    if (rightWeapon >= 0 && leftWeapon >= 0 &&
        SolveTwin(rightWeapon, leftWeapon, g_weapons, &weaponResidual, &weaponMirror) &&
        weaponResidual <= 0.2f && weaponMirror <= 0.03f) {
        const int parentRight = g_VRBoneParent[rightWeapon], parentLeft = g_VRBoneParent[leftWeapon];
        g_weapons.parent = -2;
        if (parentRight == rightHand && parentLeft == leftHand) g_weapons.parent = -1;
        for (int k = 0; k < g_fingerCount; ++k)
            if (g_fingers[k].right == parentRight && g_fingers[k].left == parentLeft) g_weapons.parent = k;
        g_weaponsReady = g_weapons.parent >= -1;
    }
    if (!g_weaponsReady)
        Log("[left-handed] no mirrored weapon bones (residual %.3f, mirror %.1f mm): props stay in their own hand\n",
            weaponResidual, weaponMirror * 1000.0f);
    g_ready = true;
    CyberpunkVR_DebugLeftHandedTwins = g_fingerCount + 1;
    CyberpunkVR_DebugLeftHandedResidual = worst;
    const auto& h = g_hands.s.m;
    Log("[left-handed] twins: hands, %d fingers; residual %.3f, mirror %.1f mm; S hand [%g %g %g|%g %g %g|%g %g %g]\n",
        g_fingerCount, worst, worstMirror * 1000.0f, h[0][0], h[0][1], h[0][2], h[1][0], h[1][1], h[1][2], h[2][0],
        h[2][1], h[2][2]);
}

void NoteFingerFlex(const uint8_t* boneBuf, bool drawn) {
    if (!boneBuf || !g_ready || g_fingerCount <= 0) return;
    for (int side = 0; side < 2; ++side) {
        float sum = 0.0f;
        int n = 0;
        for (int k = 0; k < g_fingerCount; ++k) {
            const int bone = side == 0 ? g_fingers[k].left : g_fingers[k].right;
            float p[3], ref[4], q[4];
            if (!VRIK_ReferenceLocal(bone, p, ref)) continue;
            std::memcpy(q, boneBuf + bone * 48 + VRIK_ROT_OFF, sizeof(q));
            const float d = std::fabs(q[0] * ref[0] + q[1] * ref[1] + q[2] * ref[2] + q[3] * ref[3]);
            sum += 2.0f * std::acos(std::fmin(1.0f, d)) * 57.29578f;
            ++n;
        }
        if (n > 0) CyberpunkVR_DebugFingerFlex[(drawn ? 2 : 0) + side] = sum / static_cast<float>(n);
    }
}

void MirrorHeldHand(float* pos, float* rot) {
    if (!g_ready || !pos || !rot) return;
    // The weapon hand becomes M R_right S, the other hand M R_left S^T, so the left wrist's offset in the right
    // wrist's frame turns into S^T off, and the relative rotation into S^T rot S^T.
    const Mat3 st = Transposed(g_hands.s);
    float p[3], q[4];
    Apply(st, pos, p);
    Conjugated(st, rot, st, q);
    std::memcpy(pos, p, sizeof(p));
    std::memcpy(rot, q, sizeof(q));
}

bool MirrorFingerToRight(int leftBone, const float* rot, int* rightBone, float* out) {
    if (!g_ready || !rot || !rightBone || !out) return false;
    for (int k = 0; k < g_fingerCount; ++k) {
        const Twin& t = g_fingers[k];
        if (t.left != leftBone) continue;
        // local_left = S_parent^T * local_right * S, so local_right = S_parent * local_left * S^T
        Conjugated(ParentReflection(t), rot, Transposed(t.s), out);
        *rightBone = t.right;
        return true;
    }
    return false;
}

bool MirrorWeaponSlotToRight(const float* pos, const float* rot, float* outPos, float* outRot) {
    if (!g_ready || !g_weaponsReady || !pos || !rot || !outPos || !outRot) return false;
    // offset_left = S_parent^T offset_right and local_left = S_parent^T local_right S, so the right ones are
    // S_parent offset_left and S_parent local_left S^T
    const Mat3& parent = ParentReflection(g_weapons);
    Apply(parent, pos, outPos);
    Conjugated(parent, rot, Transposed(g_weapons.s), outRot);
    return true;
}

bool TwoGunsInHand() {
    return g_hasLeftWeaponEquipped && g_hasRightWeaponEquipped && GetTickCount64() < CyberpunkVR_DualWieldUntil;
}

}  // namespace cvr::anim
