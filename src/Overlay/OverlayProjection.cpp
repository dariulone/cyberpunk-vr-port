// OverlayProjection -- putting a point that exists in the world onto a 2D overlay drawn in the headset.
//
// The overlay is an ImGui surface composited into the eye image, so anything it draws about the world has
// to be projected with the SAME frustum the eye was rendered with. GetOverlayProjTans is that: it takes
// the tangents of the half-angles rather than an FOV in degrees, because that is the form the projection
// actually uses and converting back and forth is where the overlay used to drift from the geometry.
//
// THREE ENTRY POINTS, THREE SPACES, and they are not interchangeable: a world point via the head pose, a
// head-space point directly, and an Im3d vertex. Passing a head-space point to the world-space function
// puts the marker roughly where the player is standing, which looks like a tracking bug rather than a
// mistake at the call site.

#include "Overlay/ImGuiOverlay.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <algorithm>
#include <cfloat>
#include <cmath>
#include <cstdio>
#include <string>
#include <vector>
#include <imgui.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>
#include "im3d.h"
#include "Overlay/OverlayInternal.hpp"

#if !CVR_RUST_VRIK
extern volatile int g_verboseLog; // per-frame log spam toggle (default off)
extern void Log(const char* fmt, ...);
extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);
extern "C" volatile float g_lastLocateQuat[4];
extern "C" int   CyberpunkVR_StereoModuleEnable;   // vr_core.cpp: did we install at all
extern "C" int   CyberpunkVR_StereoModuleLoaded;
extern "C" int32_t CyberpunkVR_StereoLog;
extern "C" int      CyberpunkVR_StereoSubmit;              // openxr_frameloop.cpp
extern "C" int32_t  CyberpunkVR_StereoEyeCapture;
extern "C" uint32_t CyberpunkVR_StereoEyeMaxAgeMs;
extern "C" uint32_t CyberpunkVR_DebugVrcamEyeAgeMs;        // 0xFFFFFFFF = never produced
extern "C" unsigned long long CyberpunkVR_DebugStereoEyeSubmits;
extern "C" int32_t CyberpunkVR_StableCopy;
extern "C" int32_t CyberpunkVR_StableFromTonemap;
extern "C" uint64_t CyberpunkVR_DebugStableCopies;
extern "C" uint64_t CyberpunkVR_DebugStableSkips;
extern "C" int32_t CyberpunkVR_VrcamDlss;
extern "C" int32_t CyberpunkVR_ForceVrcamCam;
extern "C" uint32_t CyberpunkVR_VrcamEnabled;
extern "C" void        CyberpunkVR_SetVrcamEnabled(uint32_t on);
extern "C" const char* CyberpunkVR_VrcamComponentName();
extern "C" const char* CyberpunkVR_VrcamCameraName();
extern "C" uint32_t CyberpunkVR_MirrorOutput;
extern "C" uint64_t CyberpunkVR_DebugVrcamNodeHits;   // 0 => the second view never dispatched
extern "C" uint64_t CyberpunkVR_DebugMirrorRtvHits;
extern "C" int CyberpunkVR_IsVrcamViewActive();
extern "C" float CyberpunkVR_DebugMainProjYY;
extern "C" float CyberpunkVR_DebugMainCamFov;
#endif
#if !CVR_RUST_VRIK
extern "C" float CyberpunkVR_MainAdsZoomFactor;
#endif
#if !CVR_RUST_VRIK
extern "C" float CyberpunkVR_DebugVrcamWantFov;
extern "C" float CyberpunkVR_DebugVrcamBaseFov;
extern "C" int32_t  CyberpunkVR_ProfEnable;
extern "C" double   CyberpunkVR_ProfFrameMs;
extern "C" double   CyberpunkVR_ProfDispMainMs;
extern "C" double   CyberpunkVR_ProfDispVrcamMs;
extern "C" uint32_t CyberpunkVR_ProfDispMainNodes;
extern "C" uint32_t CyberpunkVR_ProfDispVrcamNodes;
extern "C" void     CyberpunkVR_ProfDumpNodes();
#endif
extern "C" int      CyberpunkVR_ProfSnapshotNodes(uint32_t* rva, double* msv, double* msm,
                                                  uint32_t* cv, uint32_t* cm, int maxn);
#if !CVR_RUST_VRIK
extern "C" const char* CyberpunkVR_ProfNodeName(uint32_t rva);
extern "C" uint64_t CyberpunkVR_DebugViewKeyMainNodes;
extern "C" uint64_t CyberpunkVR_DebugViewKeyOtherNodes;
extern "C" volatile int32_t g_lastLocatePosFP[3];
extern "C" float CyberpunkVRPort_HalfIpd();
#endif
extern "C" float GetGameRenderFovDeg();
extern "C" int CyberpunkVR_MainIsRightEye;
#if !CVR_RUST_VRIK
extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();
#endif

#if CVR_RUST_VRIK
// The projection math: rust/crates/cvr-camera/src/overlay_projection.rs.
#if CVR_RUST_VRIK
#endif
#endif
namespace overlay {

#if !CVR_RUST_VRIK   // unused by the Rust build
void MapAbstractHandPoint(bool isLeftHand, float hx, float hy, float hz, float* cx, float* cy, float* cz) {
#if CVR_RUST_VRIK
    float v[3];cvr_ovp_map_hand(isLeftHand,hx,hy,hz,v);*cx=v[0];*cy=v[1];*cz=v[2];
#else
    if (isLeftHand) {
        *cx = -hz;
    } else {
        *cx = hz;
    }
    *cy = -hy;
    *cz = -hx;
#endif
}
#endif

#if !CVR_RUST_VRIK   // unused by the Rust build
void RotateVectorByQuaternion(float vx, float vy, float vz, float qx, float qy, float qz, float qw,
    float* outX, float* outY, float* outZ) {
#if CVR_RUST_VRIK
    float v[3];cvr_ovp_rotate(vx,vy,vz,qx,qy,qz,qw,v);*outX=v[0];*outY=v[1];*outZ=v[2];
#else
    const float tx = 2.0f * (qy * vz - qz * vy);
    const float ty = 2.0f * (qz * vx - qx * vz);
    const float tz = 2.0f * (qx * vy - qy * vx);

    *outX = vx + qw * tx + (qy * tz - qz * ty);
    *outY = vy + qw * ty + (qz * tx - qx * tz);
    *outZ = vz + qw * tz + (qx * ty - qy * tx);
#endif
}
#endif

// GAME-RENDER projection tans (tan of half-FOV per NDC unit). The overlay draws
// INTO the game's back buffer, so a mark lands on the same pixel as a game-rendered
// point ONLY if it reproduces the ENGINE's own projection -- not the lens frustum.
//
// The engine's projection here (empirically PROVEN on Pico, square 1:1 buffer,
// verified exact dot-on-impact): camera +0x410 -- which OnNormalFovHookCallback
// (vr_core.cpp) writes = lens horizontal FOV (or xr_force_fov) -- is the ACTUAL
// HORIZONTAL at the current render aspect, and the VERTICAL is derived in tan space
// from the back-buffer aspect: tanV = tanH * (h/w) (hor+). On a 1:1 buffer V == H,
// which is exactly the old overlay behavior that was verified correct in-headset.
// (An older comment near GetDesiredGameHorizontalFov claims "+0x410 is horizontal
// at 16:9, vertical locked from 9/16" -- that reconstruction put V at 71.5 deg on
// Pico instead of 104 and visibly broke the dot: WRONG for this path. Do not
// resurrect it.)
//
// H therefore comes from GetGameRenderFovDeg() = the very value the hook wrote (on
// symmetric HMDs == lens H; the lens-H fallback below covers frames before the hook
// first fires). V is aspect-derived -- NOT the lens vertical: on asymmetric HMDs
// (Quest 3: ~94x96 lens, non-square eye buffer) lensV != engine V and using it
// drifted the dot vertically toward the frame edges (the real "asymmetric HMD"
// error of the old code).
//
// Per-eye lens ASYMMETRY enters only with the asymmetric projection on: then MAIN renders its eye's
// own off-centre window and GetOverlayProjWindow below follows it (half spans and centre from the
// same window the view producer used). With it off, the frame is the symmetric centred frustum this
// paragraph's derivation is about.
extern "C" float GetGameRenderFovDeg();
extern "C" int CyberpunkVR_MainIsRightEye;
// THE BACKBUFFER IS MAIN'S WINDOW, NOT A FRUSTUM CENTRED ON THE HEAD. With the asymmetric projection
// (Runtimes/AsymmetricProjection.hpp) MAIN renders its eye's own off-centre window, so the straight-
// ahead direction is not the middle of the picture: it sits at NDC (centreX, centreY), the engine's
// own shift. The half spans are the window's, and the ADS zoom scales them exactly as before -- the
// engine multiplies the perspective by the zoom BEFORE it adds the shift, so the shift stays put.
#if !CVR_RUST_VRIK   // unused by the Rust build
bool GetOverlayProjWindow(const ImVec2& displaySize, float* tanHalfX, float* tanHalfY,
    float* centreX, float* centreY) {
#if CVR_RUST_VRIK
    // rust/crates/cvr-render/src/eye_windows.rs overlay_window: the main eye's asymmetric window, or the game's or the
    // runtime's horizontal FOV.
    if (displaySize.x <= 1.0f || displaySize.y <= 1.0f) return false;
    *centreX = *centreY = 0.0f;
    RustOverlayWindow result{};
    if (!cvr_overlay_proj_window(displaySize.x, displaySize.y, &result)) return false;
    *tanHalfX = result.tanX;*tanHalfY = result.tanY;*centreX = result.centreX;*centreY = result.centreY;
    return true;
#else
    if (displaySize.x <= 1.0f || displaySize.y <= 1.0f) return false;
    float tx = 0.0f, ty = 0.0f;
    *centreX = *centreY = 0.0f;
    AsymEyeWindows asym{};
    if (cvr::projection::Active() && OpenXRManager::Get().GetAsymmetricEyeWindows(&asym)) {
        const auto& w = asym.eye[CyberpunkVR_MainIsRightEye ? 1 : 0];
        tx = 0.5f * (w.r - w.l);
        ty = 0.5f * (w.u - w.d);
        *centreX = -(w.r + w.l) / (w.r - w.l);
        *centreY = -(w.u + w.d) / (w.u - w.d);
    } else {
        float hfovDeg = GetGameRenderFovDeg();
        if (hfovDeg <= 1.0f) hfovDeg = OpenXRManager::Get().GetRuntimeHorizontalFovDeg();
        if (hfovDeg <= 1.0f) hfovDeg = 100.0f;
        tx = tanf((hfovDeg * 0.5f) * (3.1415926535f / 180.0f));
        ty = tx * (displaySize.y / displaySize.x);
    }

    // AIMING MAGNIFIES THE IMAGE WITHOUT TOUCHING THE FOV, so a projection built from the FOV alone
    // stops matching the picture the moment the player raises the sights.
    //
    // Measured on the MAIN view context: the fov scalar at +0x90 reads 68.238 both at rest and
    // while aiming -- ADS does not go through it. It goes through the projection matrix, +0x214,
    // and the stereo module already recovers the ratio from there:
    //
    //     ads = MAIN projYY * tan(baseFov / 2)
    //
    // 1.0 at rest, ~1.3-1.5 for ordinary ADS, 4.25 for a sniper scope. Because screen NDC is
    // proportional to 1/tanHalfFov, dividing BOTH tangents by that factor magnifies every projected
    // offset by exactly the amount MAIN magnified the world -- and it is the ONE correction the
    // overlay needs.
    //
    // THE UPPER BOUND IS GONE, and removing it is the fix (dabinn, TofuExpress 2cb7b031). Written as
    // `ads < 4.0f` it silently split the weapons in two: ordinary ADS took this correct step AND was
    // then multiplied by shared[28] further down, giving the measured 1.3x * 1.3x double zoom, while
    // a 4.25x scope failed the guard, skipped this step entirely, and had shared[28] as its
    // accidental sole multiplier. Two bugs that hid each other, which is why the dot looked
    // "sometimes right". There is no weapon class boundary at 4x and no justified finite ceiling
    // here: accept every finite positive factor and reject only what cannot be a projection scale.
    //
    // A FLOOR STAYS, THE CEILING DOES NOT, and the asymmetry is physical rather than cautious: ADS
    // narrows the frustum, so values at and above 1 are ordinary and 4.25 is a real sniper scope --
    // there is no upper limit to justify. Below 1 is legitimate too (a state that WIDENS the view),
    // but 0.05 would mean the world got twenty times wider than the camera's own FOV, which nothing
    // in the game does; that is a stale or wrongly-typed sample. Dividing by it would collapse every
    // projected offset toward the centre, so the honest response is to draw as if unzoomed.
    {
        const float ads = CyberpunkVR_MainAdsZoomFactor;
        if (std::isfinite(ads) && ads > 0.05f) { tx /= ads; ty /= ads; }
    }

    if (tx <= 0.0001f || ty <= 0.0001f) return false;
    *tanHalfX = tx;
    *tanHalfY = ty;
    return true;
#endif
}
#endif
#if !CVR_RUST_VRIK
bool GetOverlayProjTans(const ImVec2& displaySize, float* tanHalfX, float* tanHalfY) {
    float cx = 0.0f, cy = 0.0f;
    return GetOverlayProjWindow(displaySize, tanHalfX, tanHalfY, &cx, &cy);
}
#endif

#if !CVR_RUST_VRIK   // unused by the Rust build
bool ProjectXrPointToScreen(const OpenXRHeadPose& headPose, float pointX, float pointY, float pointZ,
    const ImVec2& displaySize, ImVec2* outScreen) {
    if (!outScreen || displaySize.x <= 1.0f || displaySize.y <= 1.0f) return false;

#if CVR_RUST_VRIK
    float tanHalfX = 0.0f, tanHalfY = 0.0f, centreX = 0.0f, centreY = 0.0f;
    if (!GetOverlayProjWindow(displaySize, &tanHalfX, &tanHalfY, &centreX, &centreY)) return false;
    const float head[7]{headPose.posX, headPose.posY, headPose.posZ, headPose.oriX, headPose.oriY, headPose.oriZ, headPose.oriW};
    const RustOverlayWindow window{tanHalfX, tanHalfY, centreX, centreY};float screen[2]{};
    if (!cvr_ovp_project(true, head, pointX, pointY, pointZ, displaySize.x, displaySize.y, &window, screen)) return false;
    outScreen->x = screen[0];outScreen->y = screen[1];
    return true;
#else
    const float deltaX = pointX - headPose.posX;
    const float deltaY = pointY - headPose.posY;
    const float deltaZ = pointZ - headPose.posZ;

    // Inverse of a unit quaternion.
    const float iqx = -headPose.oriX;
    const float iqy = -headPose.oriY;
    const float iqz = -headPose.oriZ;
    const float iqw = headPose.oriW;

    float viewX = 0.0f;
    float viewY = 0.0f;
    float viewZ = 0.0f;
    RotateVectorByQuaternion(deltaX, deltaY, deltaZ, iqx, iqy, iqz, iqw, &viewX, &viewY, &viewZ);

    const float forward = -viewZ;
    if (forward <= 0.01f) return false;

    float tanHalfX = 0.0f, tanHalfY = 0.0f, centreX = 0.0f, centreY = 0.0f;
    if (!GetOverlayProjWindow(displaySize, &tanHalfX, &tanHalfY, &centreX, &centreY)) return false;

    const float ndcX = viewX / (forward * tanHalfX) + centreX;
    const float ndcY = viewY / (forward * tanHalfY) + centreY;

    outScreen->x = (ndcX * 0.5f + 0.5f) * displaySize.x;
    outScreen->y = (-ndcY * 0.5f + 0.5f) * displaySize.y;
    return true;
#endif
}
#endif

#if !CVR_RUST_VRIK   // unused by the Rust build
bool ProjectHeadSpacePointToScreen(float pointX, float pointY, float pointZ,
    const ImVec2& displaySize, ImVec2* outScreen) {
    if (!outScreen || displaySize.x <= 1.0f || displaySize.y <= 1.0f) return false;

#if CVR_RUST_VRIK
    float tanHalfX = 0.0f, tanHalfY = 0.0f, centreX = 0.0f, centreY = 0.0f;
    if (!GetOverlayProjWindow(displaySize, &tanHalfX, &tanHalfY, &centreX, &centreY)) return false;
    const RustOverlayWindow window{tanHalfX, tanHalfY, centreX, centreY};float screen[2]{};
    if (!cvr_ovp_project(false, nullptr, pointX, pointY, pointZ, displaySize.x, displaySize.y, &window, screen)) return false;
    outScreen->x = screen[0];outScreen->y = screen[1];
    return true;
#else
    const float forward = -pointZ;
    if (forward <= 0.01f) return false;

    float tanHalfX = 0.0f, tanHalfY = 0.0f, centreX = 0.0f, centreY = 0.0f;
    if (!GetOverlayProjWindow(displaySize, &tanHalfX, &tanHalfY, &centreX, &centreY)) return false;

    const float ndcX = pointX / (forward * tanHalfX) + centreX;
    const float ndcY = pointY / (forward * tanHalfY) + centreY;

    outScreen->x = (ndcX * 0.5f + 0.5f) * displaySize.x;
    outScreen->y = (-ndcY * 0.5f + 0.5f) * displaySize.y;
    return true;
#endif
}
#endif

#if !CVR_RUST_VRIK
Im3d::Vec3 AbstractHandPointToHeadSpace(const OpenXRHeadPose& handPose, bool isLeftHand, float hx, float hy, float hz) {
    float cx = 0.0f;
    float cy = 0.0f;
    float cz = 0.0f;
    MapAbstractHandPoint(isLeftHand, hx, hy, hz, &cx, &cy, &cz);

    float dx = 0.0f;
    float dy = 0.0f;
    float dz = 0.0f;
    RotateVectorByQuaternion(cx, cy, cz,
        handPose.oriX, handPose.oriY, handPose.oriZ, handPose.oriW,
        &dx, &dy, &dz);

    return Im3d::Vec3(handPose.posX + dx, handPose.posY + dy, handPose.posZ + dz);
}
#endif

#if !CVR_RUST_VRIK
bool ProjectIm3dPointToScreen(const Im3d::Vec3& point, const ImVec2& displaySize, ImVec2* outScreen) {
    return ProjectHeadSpacePointToScreen(point.x, point.y, point.z, displaySize, outScreen);
}
#endif

#if CVR_RUST_VRIK
// The hand proxy's geometry -- its boxes and axes through the hand, in Im3d's colours -- is
// rust/crates/cvr-camera/src/overlay_projection.rs (hand_proxy); Im3d's calls stay here, a begin and end per box as
// EmitHandBox made them. RenderIm3dToDrawList is rust/crates/cvr-render/src/overlay_hands.rs's.
struct RustProxyVertex {float x,y,z;uint32_t color;};
static_assert(sizeof(RustProxyVertex)==16 && sizeof(Im3d::Color)==4);

#else
ImU32 ToImU32(const Im3d::Color& color) {
    return static_cast<ImU32>(color.getABGR());
}

void RenderIm3dToDrawList(ImDrawList* drawList, const ImVec2& displaySize) {
    if (!drawList) return;

    const Im3d::DrawList* drawLists = Im3d::GetDrawLists();
    const Im3d::U32 drawListCount = Im3d::GetDrawListCount();
    for (Im3d::U32 listIndex = 0; listIndex < drawListCount; ++listIndex) {
        const Im3d::DrawList& list = drawLists[listIndex];
        const Im3d::VertexData* verts = list.m_vertexData;
        if (!verts || list.m_vertexCount == 0) continue;

        if (list.m_primType == Im3d::DrawPrimitive_Triangles) {
            for (Im3d::U32 i = 0; i + 2 < list.m_vertexCount; i += 3) {
                ImVec2 a{}, b{}, c{};
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i].m_positionSize), displaySize, &a)) continue;
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i + 1].m_positionSize), displaySize, &b)) continue;
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i + 2].m_positionSize), displaySize, &c)) continue;
                drawList->AddTriangleFilled(a, b, c, ToImU32(verts[i].m_color));
            }
        } else if (list.m_primType == Im3d::DrawPrimitive_Lines) {
            for (Im3d::U32 i = 0; i + 1 < list.m_vertexCount; i += 2) {
                ImVec2 a{}, b{};
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i].m_positionSize), displaySize, &a)) continue;
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i + 1].m_positionSize), displaySize, &b)) continue;
                const float thickness = std::max(1.0f, (verts[i].m_positionSize.w + verts[i + 1].m_positionSize.w) * 0.5f);
                //drawList->AddLine(a, b, ToImU32(verts[i].m_color), thickness);
            }
        } else if (list.m_primType == Im3d::DrawPrimitive_Points) {
            for (Im3d::U32 i = 0; i < list.m_vertexCount; ++i) {
                ImVec2 p{};
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i].m_positionSize), displaySize, &p)) continue;
                const float radius = std::max(1.0f, verts[i].m_positionSize.w * 0.1f);
                drawList->AddCircleFilled(p, radius, ToImU32(verts[i].m_color));
            }
        }
    }
}

void EmitIm3dQuad(const Im3d::Vec3& a, const Im3d::Vec3& b, const Im3d::Vec3& c, const Im3d::Vec3& d, const Im3d::Color& color) {
    Im3d::Vertex(a, color);
    Im3d::Vertex(b, color);
    Im3d::Vertex(c, color);
    Im3d::Vertex(a, color);
    Im3d::Vertex(c, color);
    Im3d::Vertex(d, color);
}

void EmitHandBox(const OpenXRHeadPose& handPose, bool isLeftHand,
    float minX, float minY, float minZ,
    float maxX, float maxY, float maxZ,
    const Im3d::Color& color) {
    const Im3d::Vec3 p000 = AbstractHandPointToHeadSpace(handPose, isLeftHand, minX, minY, minZ);
    const Im3d::Vec3 p100 = AbstractHandPointToHeadSpace(handPose, isLeftHand, maxX, minY, minZ);
    const Im3d::Vec3 p110 = AbstractHandPointToHeadSpace(handPose, isLeftHand, maxX, maxY, minZ);
    const Im3d::Vec3 p010 = AbstractHandPointToHeadSpace(handPose, isLeftHand, minX, maxY, minZ);
    const Im3d::Vec3 p001 = AbstractHandPointToHeadSpace(handPose, isLeftHand, minX, minY, maxZ);
    const Im3d::Vec3 p101 = AbstractHandPointToHeadSpace(handPose, isLeftHand, maxX, minY, maxZ);
    const Im3d::Vec3 p111 = AbstractHandPointToHeadSpace(handPose, isLeftHand, maxX, maxY, maxZ);
    const Im3d::Vec3 p011 = AbstractHandPointToHeadSpace(handPose, isLeftHand, minX, maxY, maxZ);

    Im3d::BeginTriangles();
    EmitIm3dQuad(p000, p100, p110, p010, color);
    EmitIm3dQuad(p001, p011, p111, p101, color);
    EmitIm3dQuad(p000, p001, p101, p100, color);
    EmitIm3dQuad(p010, p110, p111, p011, color);
    EmitIm3dQuad(p000, p010, p011, p001, color);
    EmitIm3dQuad(p100, p101, p111, p110, color);
    Im3d::End();
}

void EmitHandProxyIm3d(const OpenXRHeadPose& handPose, bool isLeftHand, float scale, bool drawAxes) {
    const Im3d::Color palmColor = isLeftHand ? Im3d::Color(0.10f, 0.85f, 1.00f, 0.32f) : Im3d::Color(1.00f, 0.72f, 0.15f, 0.32f);
    const Im3d::Color fingerColor = isLeftHand ? Im3d::Color(0.35f, 0.92f, 1.00f, 0.50f) : Im3d::Color(1.00f, 0.86f, 0.35f, 0.50f);
    const Im3d::Color thumbColor = isLeftHand ? Im3d::Color(0.22f, 0.72f, 1.00f, 0.55f) : Im3d::Color(1.00f, 0.58f, 0.22f, 0.55f);

    const float s = scale;

    // Palm block.
    EmitHandBox(handPose, isLeftHand, -0.034f * s, -0.052f * s, -0.014f * s, 0.034f * s, 0.044f * s, 0.014f * s, palmColor);

    // Fingers.
    EmitHandBox(handPose, isLeftHand, -0.028f * s, 0.044f * s, -0.010f * s, -0.018f * s, 0.102f * s, 0.010f * s, fingerColor);
    EmitHandBox(handPose, isLeftHand, -0.012f * s, 0.044f * s, -0.010f * s, -0.002f * s, 0.116f * s, 0.010f * s, fingerColor);
    EmitHandBox(handPose, isLeftHand, 0.004f * s, 0.044f * s, -0.010f * s, 0.014f * s, 0.128f * s, 0.010f * s, fingerColor);
    EmitHandBox(handPose, isLeftHand, 0.020f * s, 0.044f * s, -0.010f * s, 0.030f * s, 0.112f * s, 0.010f * s, fingerColor);

    // Thumb as two compact volumes.
    EmitHandBox(handPose, isLeftHand, 0.018f * s, -0.004f * s, -0.010f * s, 0.046f * s, 0.018f * s, 0.010f * s, thumbColor);
    EmitHandBox(handPose, isLeftHand, 0.042f * s, 0.012f * s, -0.010f * s, 0.068f * s, 0.036f * s, 0.010f * s, thumbColor);

    if (drawAxes) {
        Im3d::PushSize(2.0f);
        Im3d::BeginLines();
        Im3d::Vertex(AbstractHandPointToHeadSpace(handPose, isLeftHand, -0.060f * s, 0.0f, 0.0f), Im3d::Color_Red);
        Im3d::Vertex(AbstractHandPointToHeadSpace(handPose, isLeftHand, 0.060f * s, 0.0f, 0.0f), Im3d::Color_Red);
        Im3d::Vertex(AbstractHandPointToHeadSpace(handPose, isLeftHand, 0.0f, -0.060f * s, 0.0f), Im3d::Color_Green);
        Im3d::Vertex(AbstractHandPointToHeadSpace(handPose, isLeftHand, 0.0f, 0.060f * s, 0.0f), Im3d::Color_Green);
        Im3d::Vertex(AbstractHandPointToHeadSpace(handPose, isLeftHand, 0.0f, 0.0f, 0.0f), Im3d::Color_Blue);
        Im3d::Vertex(AbstractHandPointToHeadSpace(handPose, isLeftHand, 0.0f, 0.0f, -0.180f * s), Im3d::Color_Blue);
        Im3d::End();
        Im3d::PopSize();
    }
}

#endif
#if !CVR_RUST_VRIK
bool ProjectHandLocalPoint(const OpenXRHeadPose& headPose, const OpenXRHeadPose& handPose,
    float localX, float localY, float localZ, const ImVec2& displaySize, ImVec2* outScreen) {
    (void)headPose;
    float worldDeltaX = 0.0f;
    float worldDeltaY = 0.0f;
    float worldDeltaZ = 0.0f;
    RotateVectorByQuaternion(localX, localY, localZ,
        handPose.oriX, handPose.oriY, handPose.oriZ, handPose.oriW,
        &worldDeltaX, &worldDeltaY, &worldDeltaZ);

    return ProjectHeadSpacePointToScreen(
        handPose.posX + worldDeltaX,
        handPose.posY + worldDeltaY,
        handPose.posZ + worldDeltaZ,
        displaySize,
        outScreen);
}
#endif

#if !CVR_RUST_VRIK
void DrawProjectedBone(ImDrawList* drawList, const OpenXRHeadPose& headPose, const OpenXRHeadPose& handPose,
    float ax, float ay, float az, float bx, float by, float bz,
    const ImVec2& displaySize, ImU32 color, float thickness) {
    ImVec2 a{};
    ImVec2 b{};
    if (!ProjectHandLocalPoint(headPose, handPose, ax, ay, az, displaySize, &a)) return;
    if (!ProjectHandLocalPoint(headPose, handPose, bx, by, bz, displaySize, &b)) return;
    drawList->AddLine(a, b, color, thickness);
}
#endif

}  // namespace overlay
using namespace overlay;
