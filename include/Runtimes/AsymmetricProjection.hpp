#pragma once
// THE EYE'S OWN FRUSTUM, RENDERED AS IT IS.
//
// A headset lens sits off the centre of its panel. A Quest 3 eye reports -54/+40 deg horizontally and
// +44/-55 vertically; rendering that symmetric about the eye axis means sizing to the widest half-angle
// (110 x 110 deg of tangent span for a 94 x 99 deg panel) and throwing a third of every rendered pixel
// away, with the centre that much softer. The engine can draw the off-centre frustum itself -- see
// src/Stereo/AsymProjection.cpp for where and how -- so each eye renders exactly its panel and the
// projection layer is submitted with exactly that frustum: nothing cropped, nothing black, nothing
// rendered for nobody.
//
// ONE FUNCTION DECIDES THE WINDOW, and the render side and the submit side both call it, so the
// rectangle the compositor is promised is the rectangle that was drawn.
//
//   * Horizontal: each eye's own runtime span, exactly.
//   * Vertical: one span for both eyes -- the engine gives a camera one vertical fov -- the larger of
//     the two, centred on each eye's own vertical centre. On a headset whose eyes agree vertically
//     (all of them in practice) that is each eye's own span, exactly.
//   * A canted headset (displays turned outward, eye poses rotated against the head) is rendered with
//     the HEAD's orientation for both eyes, so the eye's window is carried into the head frame by the
//     eye's yaw against the head first. Zero on a parallel headset.
//   * SQUARE PIXELS. The window is drawn into the render target edge to edge, so when the two aspects
//     differ the pixels are not square: resolution goes unevenly to the two axes. The shorter axis is
//     therefore grown until the window has the target's aspect -- never shrunk, so nothing the lens
//     shows is lost and nothing goes black; the extra sliver lies outside the lens and costs only its
//     few pixels. The launcher's ladders are shaped to the windows, which keeps that sliver near zero;
//     the fit is what keeps the pixels square whatever the runtime reports (a FOV cut in the runtime
//     changes the window, not the ladder).
#include <openxr/openxr.h>
#include <cmath>
#include <cstdint>
#include "Stereo/EngineStereo.hpp"

extern "C" __declspec(dllexport) int32_t CyberpunkVR_AsymProjection;   // src/Stereo/AsymProjection.cpp
extern "C" __declspec(dllexport) int32_t CyberpunkVR_AsymSquarePixels; // 1 = fit the window to the target (default)
extern "C" __declspec(dllexport) float CyberpunkVR_AsymInnerGuard;     // 0..1, see Windows()
extern int g_launcherWidth;                                            // the render target (src/Core/VrCore.cpp)
extern int g_launcherHeight;

namespace cvr::projection {

// Tangents of the rendered window, in the head frame: l < 0 < r usually, d < 0 < u usually.
struct EyeWindow { float l = 0.0f, r = 0.0f, d = 0.0f, u = 0.0f; };

// What the engine is given for one eye: vertical fov (degrees), aspect, NDC shift of the centre.
struct EngineProjection { float vfovDeg = 0.0f, aspect = 0.0f, shiftX = 0.0f, shiftY = 0.0f; };

inline bool Active() { return CyberpunkVR_AsymProjection == 1; }

// The render target's aspect, width / height; 0 when unknown.
inline float RenderAspect() {
    return (g_launcherWidth > 0 && g_launcherHeight > 0) ? float(g_launcherWidth) / float(g_launcherHeight) : 0.0f;
}

// What the view producer last gave a view (0 MAIN, 1 the second view), while it is giving one
// (src/Stereo/AsymProjection.cpp): the NDC shift of the centre and the window's half spans in tangent
// units. False when that view renders the engine's centred frustum.
struct ViewWindow { float shiftX = 0.0f, shiftY = 0.0f, halfX = 0.0f, halfY = 0.0f; };
bool ViewWindowOf(int side, ViewWindow* out);

// Yaw of an eye against the head, positive to the right, from the two eye orientations. The head is
// taken as the midway orientation, which is the VIEW space of every runtime that reports a cant.
#if CVR_RUST_ROOM
}  // namespace cvr::projection
// rust/crates/cvr-camera/src/projection.rs.
extern "C" {
void cvr_projection_eye_yaws_against_head(const XrQuaternionf* a, const XrQuaternionf* b, float* yaw);
bool cvr_projection_windows(const XrFovf* fov, const XrQuaternionf* eyeOrientation, cvr::projection::EyeWindow* out, float guard, float target);
void cvr_projection_to_fov(const cvr::projection::EyeWindow* w, XrFovf* out);
void cvr_projection_to_engine(const cvr::projection::EyeWindow* w, cvr::projection::EngineProjection* out);
float cvr_projection_map_ndc_x(float n, const cvr::projection::EyeWindow* from, const cvr::projection::EyeWindow* to);
float cvr_projection_map_ndc_y(float n, const cvr::projection::EyeWindow* from, const cvr::projection::EyeWindow* to);
}
namespace cvr::projection {
inline void EyeYawsAgainstHead(const XrQuaternionf& a, const XrQuaternionf& b, float yaw[2]) { cvr_projection_eye_yaws_against_head(&a, &b, yaw); }
inline bool WindowsWith(const XrFovf fov[2], const XrQuaternionf* eyeOrientation, EyeWindow out[2], float guard, float target) {
    return cvr_projection_windows(fov, eyeOrientation, out, guard, target);
}
inline XrFovf ToFov(const EyeWindow& w) { XrFovf f{}; cvr_projection_to_fov(&w, &f); return f; }
inline EngineProjection ToEngine(const EyeWindow& w) { EngineProjection p; cvr_projection_to_engine(&w, &p); return p; }
inline float MapNdcX(float n, const EyeWindow& from, const EyeWindow& to) { return cvr_projection_map_ndc_x(n, &from, &to); }
inline float MapNdcY(float n, const EyeWindow& from, const EyeWindow& to) { return cvr_projection_map_ndc_y(n, &from, &to); }
#else
inline void EyeYawsAgainstHead(const XrQuaternionf& a, const XrQuaternionf& b, float yaw[2]) {
    yaw[0] = yaw[1] = 0.0f;
    float bx = b.x, by = b.y, bz = b.z, bw = b.w;
    if (a.x * bx + a.y * by + a.z * bz + a.w * bw < 0.0f) { bx = -bx; by = -by; bz = -bz; bw = -bw; }
    float hx = a.x + bx, hy = a.y + by, hz = a.z + bz, hw = a.w + bw;
    const float n = std::sqrt(hx * hx + hy * hy + hz * hz + hw * hw);
    if (!(n > 1e-6f)) return;
    hx /= n; hy /= n; hz /= n; hw /= n;
    const XrQuaternionf eyes[2] = {a, {bx, by, bz, bw}};
    for (int e = 0; e < 2; ++e) {
        // rel = conj(head) * eye
        const XrQuaternionf q = eyes[e];
        const float rw = hw * q.w + hx * q.x + hy * q.y + hz * q.z;
        const float rx = hw * q.x - hx * q.w - hy * q.z + hz * q.y;
        const float ry = hw * q.y + hx * q.z - hy * q.w - hz * q.x;
        const float rz = hw * q.z - hx * q.y + hy * q.x - hz * q.w;
        // forward (0,0,-1) rotated by rel
        const float fx = -2.0f * (rx * rz + rw * ry);
        const float fz = -(1.0f - 2.0f * (rx * rx + ry * ry));
        const float y = std::atan2(fx, -fz);
        yaw[e] = (std::isfinite(y) && std::fabs(y) > 0.002f && std::fabs(y) < 0.35f) ? y : 0.0f;
    }
}

// The rendered window of both eyes. False when the runtime frusta are not usable.
// Windows with its guard (0..1) and target aspect (0: none) already chosen.
inline bool WindowsWith(const XrFovf fov[2], const XrQuaternionf* eyeOrientation, EyeWindow out[2], float guard, float target) {
    float yaw[2] = {0.0f, 0.0f};
    if (eyeOrientation) EyeYawsAgainstHead(eyeOrientation[0], eyeOrientation[1], yaw);
    constexpr float kMax = 1.48f;   // 85 deg
    EyeWindow w[2];
    float span = 0.0f;
    for (int e = 0; e < 2; ++e) {
        const float L = fov[e].angleLeft + yaw[e], R = fov[e].angleRight + yaw[e];
        const float U = fov[e].angleUp, D = fov[e].angleDown;
        if (!std::isfinite(L) || !std::isfinite(R) || !std::isfinite(U) || !std::isfinite(D) ||
            !(L > -kMax && L < R && R < kMax && D > -kMax && D < U && U < kMax)) return false;
        w[e].l = std::tan(L); w[e].r = std::tan(R); w[e].d = std::tan(D); w[e].u = std::tan(U);
        if (!(w[e].r - w[e].l > 0.05f) || !(w[e].u - w[e].d > 0.05f)) return false;
        span = std::fmax(span, w[e].u - w[e].d);
    }
    // THE INNER GUARD: how far each eye's window reaches towards the other's on its inner side. 0 (the
    // DEFAULT) = each eye exactly its own lens window, every pixel inside the lens; 1 = both eyes the
    // horizontal union (Quest 3 +-54 deg), ~19% of each eye's columns rendered outside its lens.
    //
    // The union was the default for a day, chosen from a measurement that two engine bugs had made: the
    // volumetric fog ignored the projection's NDC shift (its froxel rays had no off-centre term, fixed by
    // the shader twins in src/Shaders/asym_fog) and the fog-history borrow resampled MAIN's history across
    // a different frustum (now refused, Stereo/FogHistory.hpp SameFogFrustum). With the eyes mirror images
    // both hit the two eyes in opposite directions. Eyes at one point, low-frequency difference over the
    // overlap (build/optics-20261011/guard_recheck.py, HUD on), day / sun+haze / night:
    //     identical windows 0.29 / 0.21 / 0.34      guard 0 before the fixes 3.72 / - / 3.65
    // and with the HUD off (eye_nohud.py; the HUD is drawn with a deliberate disparity), after the fixes:
    //     identical windows 0.09 / 0.07 / 0.06      union 0.15 / 0.13 / 0.09      guard 0 1.03 / 1.19 / 0.39
    // Three more passes turned out to ignore the shift, and that was most of guard 0's figure: the sun
    // cascades' fit (Stereo/AsymProjection.cpp, union fit), the screen-space and planar reflections, whose
    // trace rebuilt each pixel without the shift and projected with it (src/Shaders/asym_ssr), and the cone
    // AO, which measured corners in a sheared space (src/Shaders/asym_ao). With those fixed, guard 0 is
    //     0.66 / 0.39 / 0.27
    // Mid-overlap guard 0 is 0.3-0.5, the resampling between two pixel grids (0.33-0.41 on synthetic
    // pairs). The rest rises towards an eye's INNER edge, and it follows the scene, not the eye
    // (eye_edge.py: turn the head and it moves to the other eye's inner edge with the bright, reflective
    // city): screen-space reflections, bloom and the like only know their own eye's screen, and a point
    // near one eye's inner edge is mid-screen for the other. Not a projection fault -- the remedy that
    // keeps the density is more pixels: a target wider than the lens window by the guard's columns
    // (the fit below then spends them past the inner edge at the lens window's own density).
    //
    // ENGINE STEREO renders both eyes through clones of ONE camera (Stereo/EngineStereo.hpp). Each clone gets its
    // own eye's window at its build; with that switched off both eyes render the clone's window as it is, and
    // then that window has to be the same for both: the union. So does the MONO MODE, where one picture is drawn
    // for both eyes: through the union, nothing either lens shows is left black.
    if (guard > 0.0f) {
        const float ul = std::fmin(w[0].l, w[1].l), ur = std::fmax(w[0].r, w[1].r);
        for (int e = 0; e < 2; ++e) {
            w[e].l += guard * (ul - w[e].l);
            w[e].r += guard * (ur - w[e].r);
        }
    }
    // Square pixels: one vertical span for both eyes, at least each eye's horizontal span over the
    // target's aspect; then each eye's horizontal span widened to that vertical span times the aspect.
    const bool fit = target > 0.2f && target < 5.0f;
    if (fit)
        for (int e = 0; e < 2; ++e) span = std::fmax(span, (w[e].r - w[e].l) / target);
    for (int e = 0; e < 2; ++e) {
        const float c = 0.5f * (w[e].u + w[e].d);
        w[e].d = c - 0.5f * span;
        w[e].u = c + 0.5f * span;
        if (fit) {
            const float h = std::fmax(w[e].r - w[e].l, span * target), m = 0.5f * (w[e].r + w[e].l);
            w[e].l = m - 0.5f * h;
            w[e].r = m + 0.5f * h;
        }
        out[e] = w[e];
    }
    return true;
}

// The projection-view fov that describes a window (what is submitted).
inline XrFovf ToFov(const EyeWindow& w) {
    XrFovf f{};
    f.angleLeft = std::atan(w.l); f.angleRight = std::atan(w.r);
    f.angleUp = std::atan(w.u);   f.angleDown = std::atan(w.d);
    return f;
}

// The engine's parameters for a window. The engine's fov is VERTICAL (P11 = 1/tan(fov/2)), its aspect
// gives P00 = P11/aspect, and its shift is added to clip x/y times w -- so shift = -(centre)/(half span)
// in tangent units, the same sign convention as OpenXR (positive moves the picture right/up; measured).
inline EngineProjection ToEngine(const EyeWindow& w) {
    const float h = w.r - w.l, v = w.u - w.d;
    EngineProjection p;
    p.vfovDeg = 2.0f * std::atan(0.5f * v) * (180.0f / 3.14159265358979f);
    p.aspect = h / v;
    p.shiftX = -(w.r + w.l) / h;
    p.shiftY = -(w.u + w.d) / v;
    return p;
}

// Where a direction drawn at NDC n in one window lands in another: what moves a flat overlay drawn
// for MAIN into the second eye. Identity for two equal windows (the symmetric render).
inline float MapNdcX(float n, const EyeWindow& from, const EyeWindow& to) {
    const float t = 0.5f * (n * (from.r - from.l) + (from.r + from.l));
    return (2.0f * t - (to.r + to.l)) / (to.r - to.l);
}
inline float MapNdcY(float n, const EyeWindow& from, const EyeWindow& to) {
    const float t = 0.5f * (n * (from.u - from.d) + (from.u + from.d));
    return (2.0f * t - (to.u + to.d)) / (to.u - to.d);
}

#endif
// Windows()' guard and square-pixel target, for callers that hand the windows to Rust.
#if CVR_RUST_VRIK
}  // namespace cvr::projection
// rust/crates/cvr-render/src/asym_projection.rs window_params.
extern "C" void cvr_projection_window_params(bool fitTarget, float* guard, float* target);
namespace cvr::projection {
inline void WindowParams(bool fitTarget, float* guard, float* target) { cvr_projection_window_params(fitTarget, guard, target); }
#else
inline void WindowParams(bool fitTarget, float* guard, float* target) {
    *guard = (cvr::engine_stereo::SharedWindow() || cvr::engine_stereo::MonoView()) ? 1.0f :
        std::isfinite(CyberpunkVR_AsymInnerGuard) ? std::fmin(1.0f, std::fmax(0.0f, CyberpunkVR_AsymInnerGuard)) : 0.0f;
    *target = (fitTarget && CyberpunkVR_AsymSquarePixels) ? RenderAspect() : 0.0f;
}
#endif
inline bool Windows(const XrFovf fov[2], const XrQuaternionf* eyeOrientation, EyeWindow out[2], bool fitTarget = true) {
    float guard = 0.0f, target = 0.0f;
    WindowParams(fitTarget, &guard, &target);
    return WindowsWith(fov, eyeOrientation, out, guard, target);
}
}  // namespace cvr::projection
