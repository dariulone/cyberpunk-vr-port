// FULL-BODY TRACKING CALIBRATION HELPERS, drawn into both eyes while calibration mode is on (Fbt/Game/Calibration.hpp),
// on top of the picture (no depth test), so they stay visible through V's body.
//   Step 1 (head): a horizon line 3 m ahead at eye height, level in the world: yellow, green once the gaze is level
//     (the step's pull counts only then).
//   Step 2 (body): a green neon light at each tracker and controller, a red neon line from each tracker to the bone it
//     drives (waist -> Hips, feet -> ankles), and a big arrow in front of the eyes pointing down, until the player
//     looks down.
//   Step 3 (the T-pose): a latin cross standing in front of the player (Calibration.hpp CrossView), a ring on its top
//     and at the ends of its arms, and three balls on its plane -- the head and the two controllers, as a mirror shows
//     them -- red off their marks, yellow near, green on them; the countdown's seconds over the top while all three hold.
//
// World points, one projection per eye, as the barrel crosshair does it (OverlayDebugDraw.cpp): the eye is the located
// camera centre plus half an IPD along the camera's right axis, the point is rotated into the camera frame (x right,
// y forward, z up) and projected through MAIN's window; the second eye's NDC is carried over to its own window when the
// asymmetric projection is on.
//   - Trackers: their head-relative OpenXR pose put on the located camera, OpenXR (x, y, z) -> camera (x, -z, y).
//   - V's bones: V is held in the rig's reference pose while calibrating, so a bone is the entity transform applied to
//     its reference model position.
#include "Overlay/OverlayInternal.hpp"
#include "Anim/CharacterRig.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Fbt/Game/Calibration.hpp"
#include "Fbt/Game/GameFont.hpp"
#include <cstring>
#include "Stereo/EngineStereo.hpp"
#include <imgui.h>
#include <cfloat>
#include <cstdio>
#include <cmath>
#include <string>

extern volatile int32_t g_lastLocatePosFP[3];
extern void Log(const char*, ...);
extern "C" float CyberpunkVRPort_HalfIpd();
extern "C" int CyberpunkVR_MainIsRightEye;

namespace overlay {
namespace {
// THE TEXTS' TYPEFACE: the game's own, for the game's text language (UserSettings.json, /language OnScreen): Rajdhani,
// the VR menu's text face, with the game's Cyrillic cut for Russian and Ukrainian, the game's own font for the others
// (Fbt/Game/GameFont.cpp). Read from the game's archives at overlay
// start (the font file inside the game's rendFont) and put into the atlas once, before its texture is made: holding only
// ASCII and the letters of the texts drawn in the view (the language's step texts, modules/lang/settings). Without it
// (no such font, not loaded in time, not a TrueType file) the overlay's font.
ImFont* g_cyberpunkFont = nullptr;
constexpr float kCyberpunkFontPx = 48.0f;
ImFont* TextFont(const char*) { return g_cyberpunkFont ? g_cyberpunkFont : ImGui::GetFont(); }

constexpr float kBallRadiusM = 0.04f;   // a Vive Tracker is about 8 cm across
constexpr float kLineThicknessPx = 6.0f;
const ImU32 kLineColor = IM_COL32(255, 60, 60, 240);
// The arrow: head-locked, kArrowDistanceM ahead, hidden once the view is kArrowHideDeg below the horizon.
constexpr float kArrowDistanceM = 1.5f, kArrowHideDeg = 30.0f;
const ImU32 kArrowColor = IM_COL32(255, 220, 0, 235), kArrowEdge = IM_COL32(40, 30, 0, 255);
// THE NEON MOTIF, on the message plates, the horizon line, the cross and its rings: a lit tube -- three soft glows
// widening around it in its colour, the line itself, a whiter core. Cyan for the cross (the VR menu's chrome), green on a
// mark reached, the texts' own colour on their plates.
const ImU32 kNeonCyan = IM_COL32(110, 220, 255, 240), kNeonGreen = IM_COL32(60, 230, 90, 240);
const ImU32 kPlate = IM_COL32(10, 10, 10, 200);
constexpr float kPlateRounding = 8.0f;
ImU32 WithAlpha(ImU32 c, float k) {
    const int a = int(float((c >> IM_COL32_A_SHIFT) & 0xFF) * k);
    return (c & ~IM_COL32_A_MASK) | (ImU32(a < 0 ? 0 : a > 255 ? 255 : a) << IM_COL32_A_SHIFT);
}
ImU32 Whiter(ImU32 c, float k) {
    auto mix = [&](int shift) { const int v = int((c >> shift) & 0xFF); return ImU32(v + int(float(255 - v) * k)) << shift; };
    return mix(IM_COL32_R_SHIFT) | mix(IM_COL32_G_SHIFT) | mix(IM_COL32_B_SHIFT) | (c & IM_COL32_A_MASK);
}
// The glow (under whatever is drawn next), then the tube: stroke(colour, thickness) draws the shape once.
template <typename Stroke> void NeonGlow(ImU32 color, float core, Stroke stroke) {
    stroke(WithAlpha(color, 0.10f), core + 14.0f);
    stroke(WithAlpha(color, 0.18f), core + 8.0f);
    stroke(WithAlpha(color, 0.35f), core + 4.0f);
}
template <typename Stroke> void NeonTube(ImU32 color, float core, Stroke stroke) {
    stroke(color, core);
    stroke(Whiter(color, 0.6f), std::fmax(1.0f, core * 0.4f));
}
template <typename Stroke> void Neon(ImU32 color, float core, Stroke stroke) {
    NeonGlow(color, core, stroke);
    NeonTube(color, core, stroke);
}
// A black plate with a thin neon border in the colour of its text; the glow stays outside the plate.
void NeonPlate(ImDrawList* dl, ImVec2 min, ImVec2 max, ImU32 color) {
    auto rect = [&](ImU32 c, float t) { dl->AddRect(min, max, c, kPlateRounding, 0, t); };
    NeonGlow(color, 1.5f, rect);
    dl->AddRectFilled(min, max, kPlate, kPlateRounding);
    NeonTube(color, 1.5f, rect);
}
void NeonLine(ImDrawList* dl, ImVec2 a, ImVec2 b, ImU32 color, float core) {
    Neon(color, core, [&](ImU32 c, float t) { dl->AddLine(a, b, c, t); });
}

struct EyeView {
    float cam[3]{}, quat[4]{0, 0, 0, 1}, right[3]{}, halfIpd = 0.0325f;
    ImVec2 eyeSize{};
    bool stereo = false, asym = false;
    int mainEye = 0;
    AsymEyeWindows windows{};
};

void Rotate(const float q[4], const float v[3], float out[3]) {
    RotateVectorByQuaternion(v[0], v[1], v[2], q[0], q[1], q[2], q[3], &out[0], &out[1], &out[2]);
}

// A world point as seen from one eye (0 = left half of the frame, 1 = right), in backbuffer pixels.
bool Project(const EyeView& v, int eye, const float world[3], ImVec2* out) {
    const float sign = v.stereo ? (eye == 0 ? -1.0f : 1.0f) : 0.0f;
    float d[3], l[3];
    for (int k = 0; k < 3; ++k) d[k] = world[k] - (v.cam[k] + v.right[k] * v.halfIpd * sign);
    const float inverse[4] = {-v.quat[0], -v.quat[1], -v.quat[2], v.quat[3]};
    Rotate(inverse, d, l);
    ImVec2 s{};
    if (!ProjectHeadSpacePointToScreen(l[0], l[2], -l[1], v.eyeSize, &s)) return false;
    if (v.stereo && v.asym && eye != v.mainEye) {
        float nx = (s.x / v.eyeSize.x) * 2.0f - 1.0f, ny = 1.0f - 2.0f * s.y / v.eyeSize.y;
        nx = cvr::projection::MapNdcX(nx, v.windows.eye[v.mainEye], v.windows.eye[eye]);
        ny = cvr::projection::MapNdcY(ny, v.windows.eye[v.mainEye], v.windows.eye[eye]);
        s = ImVec2((nx * 0.5f + 0.5f) * v.eyeSize.x, (0.5f - ny * 0.5f) * v.eyeSize.y);
    }
    *out = ImVec2(s.x + float(eye) * v.eyeSize.x, s.y);
    return true;
}

// The lights' colours: green (the trackers, a mark reached), yellow (near), red (off).
const ImU32 kLightGreen = kNeonGreen, kLightYellow = kArrowColor, kLightRed = IM_COL32(255, 80, 70, 240);

// A neon light the size of the device at that distance: a soft halo fading out, a thin ring, a bright core, whiter in
// its centre.
void Ball(ImDrawList* dl, const EyeView& v, int eye, const float world[3], ImU32 color = kLightGreen,
          float radius = kBallRadiusM) {
    ImVec2 c{}, edge{};
    float side[3];
    for (int k = 0; k < 3; ++k) side[k] = world[k] + v.right[k] * radius;
    if (!Project(v, eye, world, &c) || !Project(v, eye, side, &edge)) return;
    const float r = std::hypot(edge.x - c.x, edge.y - c.y);
    if (!(r > 1.0f) || r > 2000.0f) return;
    dl->AddCircleFilled(c, r * 2.2f, WithAlpha(color, 0.08f), 40);
    dl->AddCircleFilled(c, r * 1.6f, WithAlpha(color, 0.16f), 40);
    dl->AddCircleFilled(c, r * 1.2f, WithAlpha(color, 0.30f), 40);
    dl->AddCircle(c, r, color, 40, std::fmax(1.5f, r * 0.12f));
    dl->AddCircleFilled(c, r * 0.70f, color, 32);
    dl->AddCircleFilled(c, r * 0.38f, Whiter(color, 0.7f), 24);
}

// A ring of a world radius around a world point, facing the eye.
void Ring(ImDrawList* dl, const EyeView& v, int eye, const float world[3], float radius, ImU32 color) {
    ImVec2 c{}, edge{};
    float side[3];
    for (int k = 0; k < 3; ++k) side[k] = world[k] + v.right[k] * radius;
    if (!Project(v, eye, world, &c) || !Project(v, eye, side, &edge)) return;
    const float r = std::hypot(edge.x - c.x, edge.y - c.y);
    if (!(r > 1.0f) || r > 2000.0f) return;
    Neon(color, 3.0f, [&](ImU32 col, float t) { dl->AddCircle(c, r, col, 32, t); });
}

// A text centred on a world point, on a dark plate.
void WorldText(ImDrawList* dl, const EyeView& v, int eye, const float world[3], const char* text, float size, ImU32 color) {
    ImVec2 c{};
    if (!text || !*text || !Project(v, eye, world, &c)) return;
    ImFont* font = TextFont(text);
    const ImVec2 extent = font->CalcTextSizeA(size, FLT_MAX, 0.0f, text);
    const ImVec2 p(c.x - extent.x * 0.5f, c.y - extent.y * 0.5f);
    NeonPlate(dl, ImVec2(p.x - 12, p.y - 6), ImVec2(p.x + extent.x + 12, p.y + extent.y + 6), color);
    dl->AddText(font, size, p, color, text);
}

// A text on a dark plate, centred under the gaze: centred on that height or, given *top (screen pixels, >= 0), its plate
// starting there. Returns the plate's bottom, for the next text to go under it (-1 when nothing was drawn).
float StepText(ImDrawList* dl, const EyeView& v, int eye, const char* text, ImU32 color = kArrowColor, float top = -1.0f) {
    if (!text || !*text) return -1.0f;
    const float at[3] = {0.0f, kArrowDistanceM, -0.50f};   // about 18 deg below the gaze: clear of the horizon line and arrows
    float w[3];
    Rotate(v.quat, at, w);
    for (int k = 0; k < 3; ++k) w[k] += v.cam[k];
    ImVec2 c{};
    if (!Project(v, eye, w, &c)) return -1.0f;
    ImFont* font = TextFont(text);
    const float size = std::fmax(22.0f, v.eyeSize.y * 0.03f);
    const ImVec2 extent = font->CalcTextSizeA(size, FLT_MAX, v.eyeSize.x * 0.6f, text);
    constexpr float kPadX = 14.0f, kPadY = 10.0f;
    const ImVec2 p(c.x - extent.x * 0.5f, top >= 0.0f ? top + kPadY : c.y - extent.y * 0.5f);
    NeonPlate(dl, ImVec2(p.x - kPadX, p.y - kPadY), ImVec2(p.x + extent.x + kPadX, p.y + extent.y + kPadY), color);
    dl->AddText(font, size, p, color, text, nullptr, v.eyeSize.x * 0.6f);
    return p.y + extent.y + kPadY;
}

void ArrowDown(ImDrawList* dl, const EyeView& v, int eye) {
    // One neon outline round the whole arrow (shaft and head), the dark fill in its two convex parts.
    const float d = kArrowDistanceM;
    const float outline[7][3] = {{-0.05f, d, 0.30f}, {0.05f, d, 0.30f}, {0.05f, d, 0.04f}, {0.17f, d, 0.04f},
                                 {0.0f, d, -0.22f},  {-0.17f, d, 0.04f}, {-0.05f, d, 0.04f}};
    ImVec2 p[7];
    for (int i = 0; i < 7; ++i) {
        float w[3];
        Rotate(v.quat, outline[i], w);
        for (int k = 0; k < 3; ++k) w[k] += v.cam[k];
        if (!Project(v, eye, w, &p[i])) return;
    }
    auto stroke = [&](ImU32 c, float t) { dl->AddPolyline(p, 7, c, ImDrawFlags_Closed, t); };
    NeonGlow(kArrowColor, 2.5f, stroke);
    const ImVec2 shaft[4] = {p[0], p[1], p[2], p[6]}, head[3] = {p[3], p[4], p[5]};
    dl->AddConvexPolyFilled(shaft, 4, kPlate);
    dl->AddConvexPolyFilled(head, 3, kPlate);
    NeonTube(kArrowColor, 2.5f, stroke);
}

void Line(ImDrawList* dl, const EyeView& v, int eye, const float a[3], const float b[3]) {
    ImVec2 pa{}, pb{};
    if (!Project(v, eye, a, &pa) || !Project(v, eye, b, &pb)) return;
    NeonLine(dl, pa, pb, kLineColor, kLineThicknessPx * 0.6f);
    Neon(kLineColor, 2.0f, [&](ImU32 c, float t) { dl->AddCircle(pb, kLineThicknessPx * 1.4f, c, 20, t); });   // the bone's end
}
}

void LoadCyberpunkFont(ImGuiIO& io) {
    cvr::fbt::game::GameTextFont font;
    std::string why;
    const ULONGLONG start = GetTickCount64();
    if (!cvr::fbt::game::ReadGameTextFont(&font, 4000, &why)) {
        Log("[fbt] the texts keep the overlay's font: %s (language %s, %s)\n", why.c_str(), font.language.c_str(), font.path.c_str());
        return;
    }
    // Only the letters drawn: ASCII (the English texts, the countdown) and those of this language's texts.
    static ImVector<ImWchar> ranges;
    ImFontGlyphRangesBuilder builder;
    builder.AddRanges(io.Fonts->GetGlyphRangesDefault());
    builder.AddText(font.texts.c_str());
    builder.BuildRanges(&ranges);
    void* data = IM_ALLOC(font.file.size());   // the atlas owns and frees it
    std::memcpy(data, font.file.data(), font.file.size());
    ImFontConfig config;
    config.FontDataOwnedByAtlas = true;
    g_cyberpunkFont = io.Fonts->AddFontFromMemoryTTF(data, static_cast<int>(font.file.size()), kCyberpunkFontPx, &config, ranges.Data);
    int glyphs = 0;
    for (int k = 0; k + 1 < ranges.Size; k += 2) glyphs += ranges[k + 1] - ranges[k] + 1;
    Log("[fbt] the texts' typeface: the game's %s for %s (%zu KB, %d letters, %llu ms)%s\n", font.path.c_str(),
        font.language.c_str(), font.file.size() / 1024, glyphs, GetTickCount64() - start,
        g_cyberpunkFont ? "" : " -- could not be added");
}

void DrawFbtCalibrationHelpers() {
    if (!cvr::fbt::game::Calibrating()) return;
    VrikFbtReference ref{};
    if (!VRIK_ReadFbtReference(&ref) || !CyberpunkVR_PlayerEntityValid) return;
    const bool engineStereo = cvr::engine_stereo::Enabled();
    if (engineStereo && cvr::engine_stereo::OnePicture()) return;
    const ImVec2 backbuffer = ImGui::GetIO().DisplaySize;
    EyeView v{};
    v.stereo = engineStereo;
    v.eyeSize = ImVec2(engineStereo ? backbuffer.x * 0.5f : backbuffer.x, backbuffer.y);
    if (v.eyeSize.x <= 1.0f || v.eyeSize.y <= 1.0f) return;
    for (int k = 0; k < 3; ++k) v.cam[k] = static_cast<float>(g_lastLocatePosFP[k]) / 131072.0f;
    for (int k = 0; k < 4; ++k) v.quat[k] = g_lastLocateQuat[k];
    if (v.cam[0] * v.cam[0] + v.cam[1] * v.cam[1] + v.cam[2] * v.cam[2] < 1.0f) return;
    const float x[3] = {1, 0, 0};
    Rotate(v.quat, x, v.right);
    const float half = CyberpunkVRPort_HalfIpd();
    v.halfIpd = half > 0.0001f ? half : 0.0325f;
    v.mainEye = CyberpunkVR_MainIsRightEye ? 1 : 0;
    v.asym = cvr::projection::Active() && OpenXRManager::Get().GetAsymmetricEyeWindows(&v.windows);

    const float entityPos[3] = {CyberpunkVR_PlayerEntityPos[0], CyberpunkVR_PlayerEntityPos[1], CyberpunkVR_PlayerEntityPos[2]};
    const float entityQuat[4] = {CyberpunkVR_PlayerEntityQuat[0], CyberpunkVR_PlayerEntityQuat[1], CyberpunkVR_PlayerEntityQuat[2],
                                 CyberpunkVR_PlayerEntityQuat[3]};
    // A head-relative OpenXR pose on the located camera: OpenXR (x, y, z) -> camera (x, -z, y).
    auto toWorld = [&](const XrVector3f& p, float out[3]) {
        const float local[3] = {p.x, -p.z, p.y};
        float w[3];
        Rotate(v.quat, local, w);
        for (int k = 0; k < 3; ++k) out[k] = v.cam[k] + w[k];
    };
    auto boneWorld = [&](const float model[3], float out[3]) {
        Rotate(entityQuat, model, out);
        for (int k = 0; k < 3; ++k) out[k] += entityPos[k];
    };
    const int step = cvr::fbt::game::CalibrationStep();
    const std::string stepText = cvr::fbt::game::StepText(step);
    const std::string refusal = cvr::fbt::game::RefusalText();   // a pull not taken: why, under the instruction
    const float* boneModel[cvr::trackers::kRoleCount] = {ref.hipsPos, ref.footPos[0], ref.footPos[1]};
    float tracker[cvr::trackers::kRoleCount][3]{}, bone[cvr::trackers::kRoleCount][3]{};
    bool have[cvr::trackers::kRoleCount]{};
    for (int r = 0; r < cvr::trackers::kRoleCount; ++r) {
        cvr::trackers::Pose p{};
        have[r] = cvr::trackers::Read(static_cast<cvr::trackers::Role>(r), &p);
        toWorld(p.pos, tracker[r]);
        boneWorld(boneModel[r], bone[r]);
    }
    float hand[2][3]{};
    bool haveHand[2]{};
    for (int h = 0; h < 2; ++h) {
        cvr::trackers::Pose p{};
        haveHand[h] = cvr::trackers::ReadHand(h, &p);
        toWorld(p.pos, hand[h]);
    }
    // Step 3's cross: STAGE points -> head-relative (the headset in STAGE, located with the devices) -> the world.
    cvr::fbt::game::CrossView cross{};
    XrPosef headStage{};
    const bool haveCross = step == 3 && cvr::fbt::game::ReadCross(&cross) && cvr::trackers::ReadHeadStage(&headStage);
    auto stageToWorld = [&](const XrVector3f& p, float out[3]) {
        const XrQuaternionf& q = headStage.orientation;
        const float d[3] = {p.x - headStage.position.x, p.y - headStage.position.y, p.z - headStage.position.z};
        float l[3];
        RotateVectorByQuaternion(d[0], d[1], d[2], -q.x, -q.y, -q.z, q.w, &l[0], &l[1], &l[2]);
        toWorld(XrVector3f{l[0], l[1], l[2]}, out);
    };
    auto onCross = [&](float lateral, float y, float out[3]) {
        stageToWorld(XrVector3f{cross.base.x + cross.right.x * lateral, y, cross.base.z + cross.right.z * lateral}, out);
    };

    // Step 2: the arrow down until the player looks down (the camera's forward, its y axis, against the horizon).
    const float forward[3] = {0, 1, 0};
    float looking[3];
    Rotate(v.quat, forward, looking);
    const float pitchDeg = std::asin(std::fmax(-1.0f, std::fmin(1.0f, looking[2]))) * 57.29578f;
    // The horizon: 3 m ahead along the view's heading, 4 m wide, at the eyes' height (world z up).
    float horizon[2][3]{};
    bool haveHorizon = false;
    {
        const float fl = std::hypot(looking[0], looking[1]);
        if (fl > 0.05f) {
            const float f[2] = {looking[0] / fl, looking[1] / fl}, r[2] = {f[1], -f[0]};
            for (int e = 0; e < 2; ++e) {
                const float side = e == 0 ? -2.0f : 2.0f;
                horizon[e][0] = v.cam[0] + f[0] * 3.0f + r[0] * side;
                horizon[e][1] = v.cam[1] + f[1] * 3.0f + r[1] * side;
                horizon[e][2] = v.cam[2];
            }
            haveHorizon = true;
        }
    }
    const bool level = std::fabs(pitchDeg) < cvr::fbt::game::kCalibrationLevelDeg;
    const bool arrowDown = step == 2 && std::asin(std::fmax(-1.0f, std::fmin(1.0f, looking[2]))) * 57.29578f > -kArrowHideDeg;

    // Background list, like the crosshair: the frame is split into the two eyes after this pass.
    ImDrawList* dl = ImGui::GetBackgroundDrawList();
    if (!dl) return;
    for (int eye = 0; eye < (engineStereo ? 2 : 1); ++eye) {
        const float left = float(eye) * v.eyeSize.x;
        dl->PushClipRect(ImVec2(left, 0), ImVec2(left + v.eyeSize.x, v.eyeSize.y), true);
        if (step == 1 && haveHorizon) {
            ImVec2 a{}, b{};
            if (Project(v, eye, horizon[0], &a) && Project(v, eye, horizon[1], &b)) {
                NeonLine(dl, a, b, level ? kNeonGreen : kArrowColor, 5.0f);
            }
        }
        if (step == 2) {
            for (int r = 0; r < cvr::trackers::kRoleCount; ++r) if (have[r]) Line(dl, v, eye, tracker[r], bone[r]);
            for (int r = 0; r < cvr::trackers::kRoleCount; ++r) if (have[r]) Ball(dl, v, eye, tracker[r]);
        }
        if (step == 2)
            for (int h = 0; h < 2; ++h) if (haveHand[h]) Ball(dl, v, eye, hand[h]);
        if (arrowDown) ArrowDown(dl, v, eye);
        if (haveCross) {
            // The latin cross: the post from the floor to the top, the beam at the shoulders.
            float foot[3], top[3], beamL[3], beamR[3];
            onCross(0, 0.0f, foot);
            onCross(0, cross.tipY, top);
            onCross(-cross.halfSpan, cross.barY, beamL);
            onCross(cross.halfSpan, cross.barY, beamR);
            for (int seg = 0; seg < 2; ++seg) {
                ImVec2 a{}, b{};
                if (!Project(v, eye, seg == 0 ? foot : beamL, &a) || !Project(v, eye, seg == 0 ? top : beamR, &b)) continue;
                NeonLine(dl, a, b, kNeonCyan, 6.0f);
            }
            // The marks, then the dots: green on their mark, yellow near, red off.
            const float* mark[3] = {top, beamL, beamR};
            for (int i = 0; i < 3; ++i) {
                Ring(dl, v, eye, mark[i], 0.09f, cross.state[i] == 2 ? kNeonGreen : kNeonCyan);
            }
            for (int i = 0; i < 3; ++i) {
                if (cross.state[i] < 0) continue;
                float at[3];
                stageToWorld(cross.dot[i], at);
                Ball(dl, v, eye, at, cross.state[i] == 2 ? kLightGreen : cross.state[i] == 1 ? kLightYellow : kLightRed, 0.06f);
            }
            if (cross.countdown >= 0) {
                float over[3];
                onCross(0, cross.tipY + 0.30f, over);
                char n[8];
                snprintf(n, sizeof(n), "%d", int(std::ceil(cross.countdown)) > 0 ? int(std::ceil(cross.countdown)) : 1);
                WorldText(dl, v, eye, over, n, std::fmax(48.0f, v.eyeSize.y * 0.07f), kNeonGreen);
            }
        }
        // Why a pull was not taken: in red, right under the instruction (never over it, however many lines it takes).
        const float below = StepText(dl, v, eye, stepText.c_str());
        StepText(dl, v, eye, refusal.c_str(), IM_COL32(255, 80, 70, 255), below >= 0.0f ? below + 12.0f : -1.0f);
        dl->PopClipRect();
    }
}

}  // namespace overlay
