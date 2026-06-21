#include "imgui_overlay.h"
#include "live_controls_ui.h"
#include "openxr_manager.h"

#include <algorithm>
#include <cfloat>
#include <cmath>
#include <string>
#include <vector>

#include <imgui.h>
#include <imgui_impl_dx12.h>
#include <imgui_impl_win32.h>

#include "im3d.h"

extern volatile int g_verboseLog; // per-frame log spam toggle (default off)

// AER V2 warp tuning accessors (runtime atomics in openxr_manager.cpp).
extern "C" float GetAerMaxExtrap();        extern "C" void SetAerMaxExtrap(float v);
extern "C" float GetAerRefineStrength();   extern "C" void SetAerRefineStrength(float v);
extern "C" float GetAerOcclusionSharp();   extern "C" void SetAerOcclusionSharp(float v);
extern "C" float GetAerFoveation();        extern "C" void SetAerFoveation(float v);

extern void Log(const char* fmt, ...);

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

// located (HMD-injected) game-world camera quaternion, defined in dxgi_proxy.cpp. Declared at GLOBAL
// scope (NOT inside the anonymous namespace below) so it keeps external linkage.
extern volatile float g_lastLocateQuat[4];

namespace {
struct FrameContext {
    ID3D12CommandAllocator* allocator = nullptr;
    ID3D12Resource* renderTarget = nullptr;
    D3D12_CPU_DESCRIPTOR_HANDLE rtv{};
};

ID3D12Device* g_device = nullptr;
ID3D12CommandQueue* g_queue = nullptr;
IDXGISwapChain3* g_swapChain3 = nullptr;
ID3D12DescriptorHeap* g_rtvHeap = nullptr;
ID3D12DescriptorHeap* g_srvHeap = nullptr;
ID3D12GraphicsCommandList* g_cmdList = nullptr;
ID3D12Fence* g_fence = nullptr;
HANDLE g_fenceEvent = nullptr;
UINT64 g_fenceValue = 0;
UINT g_rtvDescriptorSize = 0;
UINT g_frameCount = 0;
DXGI_FORMAT g_rtvFormat = DXGI_FORMAT_UNKNOWN;
std::vector<FrameContext> g_frames;

HWND g_hwnd = nullptr;
WNDPROC g_originalWndProc = nullptr;
bool g_imguiInitialized = false;
bool g_menuVisible = false;
bool g_drawHandLocator = true;
bool g_drawHandProxy3D = true;
bool g_drawHandDebugAxes = false;
float g_handLocatorScale = 1.0f;
// Long aim ray down each hand's forward (the visible "where I'm pointing / where the
// gun barrel looks" line). Reuses the same head-relative projection as the hand proxy.
bool g_drawAimRay = true;
float g_aimRayLenM = 8.0f;
// EXACT barrel crosshair: project the GAME muzzle forward (plugin publishes it to shared[24..26])
// through the located game camera (= the eye view) -> a dot exactly where the bullet goes.
bool g_drawBarrelCross = true;   // g_lastLocateQuat is declared above, at global scope

void MapAbstractHandPoint(bool isLeftHand, float hx, float hy, float hz, float* cx, float* cy, float* cz) {
    if (isLeftHand) {
        *cx = -hz;
    } else {
        *cx = hz;
    }
    *cy = -hy;
    *cz = -hx;
}

void RotateVectorByQuaternion(float vx, float vy, float vz, float qx, float qy, float qz, float qw,
    float* outX, float* outY, float* outZ) {
    const float tx = 2.0f * (qy * vz - qz * vy);
    const float ty = 2.0f * (qz * vx - qx * vz);
    const float tz = 2.0f * (qx * vy - qy * vx);

    *outX = vx + qw * tx + (qy * tz - qz * ty);
    *outY = vy + qw * ty + (qz * tx - qx * tz);
    *outZ = vz + qw * tz + (qx * ty - qy * tx);
}

bool ProjectXrPointToScreen(const OpenXRHeadPose& headPose, float pointX, float pointY, float pointZ,
    const ImVec2& displaySize, ImVec2* outScreen) {
    if (!outScreen || displaySize.x <= 1.0f || displaySize.y <= 1.0f) return false;

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

    float hfovDeg = OpenXRManager::Get().GetRuntimeHorizontalFovDeg();
    float vfovDeg = OpenXRManager::Get().GetRuntimeVerticalFovDeg();
    if (hfovDeg <= 1.0f) hfovDeg = 100.0f;
    if (vfovDeg <= 1.0f) vfovDeg = 100.0f;

    const float tanHalfX = tanf((hfovDeg * 0.5f) * (3.1415926535f / 180.0f));
    const float tanHalfY = tanf((vfovDeg * 0.5f) * (3.1415926535f / 180.0f));
    if (tanHalfX <= 0.0001f || tanHalfY <= 0.0001f) return false;

    const float ndcX = viewX / (forward * tanHalfX);
    const float ndcY = viewY / (forward * tanHalfY);

    outScreen->x = (ndcX * 0.5f + 0.5f) * displaySize.x;
    outScreen->y = (-ndcY * 0.5f + 0.5f) * displaySize.y;
    return true;
}

bool ProjectHeadSpacePointToScreen(float pointX, float pointY, float pointZ,
    const ImVec2& displaySize, ImVec2* outScreen) {
    if (!outScreen || displaySize.x <= 1.0f || displaySize.y <= 1.0f) return false;

    const float forward = -pointZ;
    if (forward <= 0.01f) return false;

    float hfovDeg = OpenXRManager::Get().GetRuntimeHorizontalFovDeg();
    float vfovDeg = OpenXRManager::Get().GetRuntimeVerticalFovDeg();
    if (hfovDeg <= 1.0f) hfovDeg = 100.0f;
    if (vfovDeg <= 1.0f) vfovDeg = 100.0f;

    const float tanHalfX = tanf((hfovDeg * 0.5f) * (3.1415926535f / 180.0f));
    const float tanHalfY = tanf((vfovDeg * 0.5f) * (3.1415926535f / 180.0f));
    if (tanHalfX <= 0.0001f || tanHalfY <= 0.0001f) return false;

    const float ndcX = pointX / (forward * tanHalfX);
    const float ndcY = pointY / (forward * tanHalfY);

    outScreen->x = (ndcX * 0.5f + 0.5f) * displaySize.x;
    outScreen->y = (-ndcY * 0.5f + 0.5f) * displaySize.y;
    return true;
}

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

bool ProjectIm3dPointToScreen(const Im3d::Vec3& point, const ImVec2& displaySize, ImVec2* outScreen) {
    return ProjectHeadSpacePointToScreen(point.x, point.y, point.z, displaySize, outScreen);
}

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
                drawList->AddLine(a, b, ToImU32(verts[i].m_color), thickness);
            }
        } else if (list.m_primType == Im3d::DrawPrimitive_Points) {
            for (Im3d::U32 i = 0; i < list.m_vertexCount; ++i) {
                ImVec2 p{};
                if (!ProjectIm3dPointToScreen(Im3d::Vec3(verts[i].m_positionSize), displaySize, &p)) continue;
                const float radius = std::max(1.0f, verts[i].m_positionSize.w * 0.5f);
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

void DrawProjectedBone(ImDrawList* drawList, const OpenXRHeadPose& headPose, const OpenXRHeadPose& handPose,
    float ax, float ay, float az, float bx, float by, float bz,
    const ImVec2& displaySize, ImU32 color, float thickness) {
    ImVec2 a{};
    ImVec2 b{};
    if (!ProjectHandLocalPoint(headPose, handPose, ax, ay, az, displaySize, &a)) return;
    if (!ProjectHandLocalPoint(headPose, handPose, bx, by, bz, displaySize, &b)) return;
    drawList->AddLine(a, b, color, thickness);
}

void DrawHandLocatorOverlay() {
    if (!g_drawHandLocator) return;

    OpenXRHeadPose head{};
    OpenXRHeadPose left{};
    OpenXRHeadPose right{};
    if (!OpenXRManager::Get().GetHeadPose(&head) || !head.valid) return;

    const bool hasLeft = OpenXRManager::Get().GetHandPose(0, &left) && left.valid;
    const bool hasRight = OpenXRManager::Get().GetHandPose(1, &right) && right.valid;
    if (!hasLeft && !hasRight) return;

    const ImVec2 displaySize = ImGui::GetIO().DisplaySize;
    if (displaySize.x <= 1.0f || displaySize.y <= 1.0f) return;

    ImDrawList* drawList = ImGui::GetForegroundDrawList();
    if (!drawList) return;

    Im3d::AppData& appData = Im3d::GetAppData();
    appData.m_viewOrigin = Im3d::Vec3(0.0f, 0.0f, 0.0f);
    appData.m_viewDirection = Im3d::Vec3(0.0f, 0.0f, -1.0f);
    appData.m_worldUp = Im3d::Vec3(0.0f, 1.0f, 0.0f);
    appData.m_viewportSize = Im3d::Vec2(displaySize.x, displaySize.y);
    appData.m_deltaTime = 1.0f / 90.0f;
    appData.m_projOrtho = false;
    float vfovDeg = OpenXRManager::Get().GetRuntimeVerticalFovDeg();
    if (vfovDeg <= 1.0f) vfovDeg = 100.0f;
    appData.m_projScaleY = tanf((vfovDeg * 0.5f) * (3.1415926535f / 180.0f));
    Im3d::NewFrame();

    const auto drawEndpointLabel = [&](const OpenXRHeadPose& handPose, float x, float y, float z,
        ImU32 color, const char* text, const ImVec2& offset) {
        ImVec2 screen{};
        if (!ProjectHandLocalPoint(head, handPose, x, y, z, displaySize, &screen)) return;
        drawList->AddText(ImVec2(screen.x + offset.x, screen.y + offset.y), color, text);
    };

    const auto drawHandWire = [&](const OpenXRHeadPose& handPose, ImU32 bodyColor, const char* name, bool isLeftHand) {
        const ImU32 rightColor = IM_COL32(255, 80, 80, 255);
        const ImU32 upColor = IM_COL32(80, 255, 80, 255);
        const ImU32 fwdColor = IM_COL32(80, 160, 255, 255);
        const ImU32 textColor = IM_COL32(255, 255, 255, 255);

        const float s = g_handLocatorScale;
        const float wristHalfW = 0.026f * s;
        const float palmHalfW = 0.034f * s;
        const float wristY = -0.055f * s;
        const float palmMidY = -0.018f * s;
        const float knuckleY = 0.045f * s;
        const float fwdLen = 0.180f * s;

        // Hand abstract frame:
        //   +X = thumb side
        //   +Y = fingers direction
        //   +Z = palm normal
        // Empirical grip-pose basis from the current runtime/controller:
        //   +X = left, +Y = forward, -Z = up
        // Desired hand mapping:
        //   thumb   -> up      => -Z
        //   fingers -> forward => +Y
        //   palm    -> left/right => +X for right hand, -X for left hand
        const auto mapHandPoint = [&](float hx, float hy, float hz, float* cx, float* cy, float* cz) {
            if (isLeftHand) {
                *cx = -hz;
            } else {
                *cx = hz;
            }
            *cy = -hy;
            *cz = -hx;
        };

        const auto bone = [&](float ax, float ay, float az, float bx, float by, float bz, ImU32 color, float thickness) {
            float cax = 0.0f, cay = 0.0f, caz = 0.0f;
            float cbx = 0.0f, cby = 0.0f, cbz = 0.0f;
            mapHandPoint(ax, ay, az, &cax, &cay, &caz);
            mapHandPoint(bx, by, bz, &cbx, &cby, &cbz);
            DrawProjectedBone(drawList, head, handPose, cax, cay, caz, cbx, cby, cbz, displaySize, color, thickness);
        };

        const auto finger = [&](float baseX, float baseY, float midX, float midY, float tipX, float tipY) {
            bone(baseX, baseY, 0.0f, midX, midY, -0.010f * s, bodyColor, 2.0f);
            bone(midX, midY, -0.010f * s, tipX, tipY, -0.020f * s, bodyColor, 2.0f);
        };

        // Wrist and palm outline.
        bone(-wristHalfW, wristY, 0.0f, wristHalfW, wristY, 0.0f, bodyColor, 2.0f);
        bone(-wristHalfW, wristY, 0.0f, -palmHalfW, palmMidY, 0.0f, bodyColor, 2.0f);
        bone(wristHalfW, wristY, 0.0f, palmHalfW, palmMidY, 0.0f, bodyColor, 2.0f);
        bone(-palmHalfW, palmMidY, 0.0f, -palmHalfW, knuckleY, 0.0f, bodyColor, 2.0f);
        bone(palmHalfW, palmMidY, 0.0f, palmHalfW, knuckleY, 0.0f, bodyColor, 2.0f);
        bone(-palmHalfW, knuckleY, 0.0f, palmHalfW, knuckleY, 0.0f, bodyColor, 2.0f);

        // Fingers.
        finger(-0.024f * s, knuckleY, -0.026f * s, 0.074f * s, -0.026f * s, 0.096f * s); // pinky
        finger(-0.010f * s, knuckleY, -0.011f * s, 0.086f * s, -0.011f * s, 0.116f * s); // ring
        finger(0.006f * s, knuckleY, 0.006f * s, 0.094f * s, 0.006f * s, 0.128f * s);     // middle
        finger(0.022f * s, knuckleY, 0.023f * s, 0.086f * s, 0.023f * s, 0.116f * s);     // index

        // Thumb: always +X in abstract hand space. Handedness is handled by mapHandPoint.
        bone(0.020f * s, -0.004f * s, 0.0f,
             0.046f * s, 0.014f * s, -0.006f * s,
             bodyColor, 2.0f);
        bone(0.046f * s, 0.014f * s, -0.006f * s,
             0.065f * s, 0.035f * s, -0.016f * s,
             bodyColor, 2.0f);

        // Axes: right/left, up/down, forward.
        bone(-0.060f * s, 0.0f, 0.0f, 0.060f * s, 0.0f, 0.0f, rightColor, 2.0f);
        bone(0.0f, -0.060f * s, 0.0f, 0.0f, 0.060f * s, 0.0f, upColor, 2.0f);
        bone(0.0f, 0.0f, 0.0f, 0.0f, 0.0f, -fwdLen, fwdColor, 3.0f);

        // Forward arrow head.
        bone(0.0f, 0.0f, -fwdLen, 0.020f * s, 0.0f, -(fwdLen - 0.028f * s), fwdColor, 2.0f);
        bone(0.0f, 0.0f, -fwdLen, -0.020f * s, 0.0f, -(fwdLen - 0.028f * s), fwdColor, 2.0f);
        bone(0.0f, 0.0f, -fwdLen, 0.0f, 0.020f * s, -(fwdLen - 0.028f * s), fwdColor, 2.0f);
        bone(0.0f, 0.0f, -fwdLen, 0.0f, -0.020f * s, -(fwdLen - 0.028f * s), fwdColor, 2.0f);

        // Center dot.
        ImVec2 center{};
        if (ProjectHandLocalPoint(head, handPose, 0.0f, 0.0f, 0.0f, displaySize, &center)) {
            drawList->AddCircleFilled(center, 4.0f, bodyColor);
            drawList->AddText(ImVec2(center.x + 8.0f, center.y - 20.0f), bodyColor, name);
        }

        drawEndpointLabel(handPose, 0.070f * s, 0.0f, 0.0f, textColor, "R", ImVec2(4.0f, -6.0f));
        drawEndpointLabel(handPose, -0.070f * s, 0.0f, 0.0f, textColor, "L", ImVec2(-10.0f, -6.0f));
        drawEndpointLabel(handPose, 0.0f, 0.070f * s, 0.0f, textColor, "U", ImVec2(-4.0f, -14.0f));
        drawEndpointLabel(handPose, 0.0f, -0.070f * s, 0.0f, textColor, "D", ImVec2(-4.0f, 2.0f));
        drawEndpointLabel(handPose, 0.0f, 0.0f, -(fwdLen + 0.020f * s), textColor, "F", ImVec2(4.0f, -6.0f));
    };

    if (hasLeft) {
        if (g_drawHandProxy3D) {
            EmitHandProxyIm3d(left, true, g_handLocatorScale, g_drawHandDebugAxes);
        }
        if (g_drawHandDebugAxes || !g_drawHandProxy3D) {
            drawHandWire(left, IM_COL32(0, 220, 255, 255), "LEFT", true);
        }
    }
    if (hasRight) {
        if (g_drawHandProxy3D) {
            EmitHandProxyIm3d(right, false, g_handLocatorScale, g_drawHandDebugAxes);
        }
        if (g_drawHandDebugAxes || !g_drawHandProxy3D) {
            drawHandWire(right, IM_COL32(255, 180, 0, 255), "RIGHT", false);
        }
    }

    Im3d::EndFrame();
    if (g_drawHandProxy3D) {
        RenderIm3dToDrawList(drawList, displaySize);
    }

    // Aim ray: a long bright line down each hand's forward (abstract -Z), i.e. the same
    // direction as the small "F" axis but extended several meters, so the user can see
    // in-headset where the controller / held weapon is pointing. Sampled in many short
    // segments so it still draws correctly when part of the ray falls behind the eye.
    if (g_drawAimRay) {
        // Laser sight: a long forward ray down each hand's pointing axis. The abstract hand
        // frame's FORWARD is +Y (user-confirmed in-headset: the bright +Y axis ran from the
        // fingers down the gun barrel; -Z had pointed sideways from the palm). Sampled in many
        // short segments so it still draws when part of the ray falls behind the eye.
        const float maxLen = (g_aimRayLenM > 0.5f) ? g_aimRayLenM : 8.0f;
        const auto laser = [&](const OpenXRHeadPose& hp, bool isLeft, ImU32 color, float thick) {
            const int N = 40;
            ImVec2 prev{};
            bool havePrev = false;
            for (int i = 0; i <= N; ++i) {
                const float t = (static_cast<float>(i) / static_cast<float>(N)) * maxLen;
                const Im3d::Vec3 p = AbstractHandPointToHeadSpace(hp, isLeft, 0.0f, t, 0.0f); // +Y = forward
                ImVec2 sc{};
                if (ProjectIm3dPointToScreen(p, displaySize, &sc)) {
                    if (havePrev) drawList->AddLine(prev, sc, color, thick);
                    prev = sc;
                    havePrev = true;
                } else {
                    havePrev = false;
                }
            }
            // Aim dot at ~4 m so "where it points" reads at a glance.
            const Im3d::Vec3 dot = AbstractHandPointToHeadSpace(hp, isLeft, 0.0f, 4.0f, 0.0f);
            ImVec2 dc{};
            if (ProjectIm3dPointToScreen(dot, displaySize, &dc)) {
                drawList->AddCircleFilled(dc, 6.0f, color);
                drawList->AddCircle(dc, 10.0f, IM_COL32(255, 255, 255, 255), 0, 2.0f);
            }
        };
        if (hasRight) laser(right, false, IM_COL32(80, 255, 80, 255), 3.0f); // weapon hand: green
        if (hasLeft)  laser(left, true, IM_COL32(0, 200, 255, 160), 2.0f);   // off hand: cyan, dimmer
    }
}

// EXACT barrel crosshair. The plugin publishes the weapon muzzle WORLD forward (shared[24..26]); we
// rotate it into the located game camera's local frame (inv(camQuat) * fwd) and project that
// direction with the SAME view/FOV the eye renders through -> the dot lands exactly where the bullet
// goes (both derive from the same muzzle + camera). No controller-space guessing.
void DrawBarrelCrosshair() {
    if (!g_drawBarrelCross) return;
    if (OpenXRManager::Get().GetSharedSlot(27) < 0.5f) return;   // muzzle fwd not published yet
    const float mfx = OpenXRManager::Get().GetSharedSlot(24);
    const float mfy = OpenXRManager::Get().GetSharedSlot(25);
    const float mfz = OpenXRManager::Get().GetSharedSlot(26);
    if (mfx*mfx + mfy*mfy + mfz*mfz < 0.25f) return;

    // inv(located cam quat) * muzzleFwd -> bullet dir in game-camera-local axes (+Y fwd, +X right, +Z up)
    const float cqx = g_lastLocateQuat[0], cqy = g_lastLocateQuat[1], cqz = g_lastLocateQuat[2], cqw = g_lastLocateQuat[3];
    float vx = 0.0f, vy = 0.0f, vz = 0.0f;
    RotateVectorByQuaternion(mfx, mfy, mfz, -cqx, -cqy, -cqz, cqw, &vx, &vy, &vz);

    const ImVec2 displaySize = ImGui::GetIO().DisplaySize;
    if (displaySize.x <= 1.0f || displaySize.y <= 1.0f) return;
    ImVec2 sc{};
    // game-cam-local (Yfwd/Xright/Zup) -> OpenXR view convention (forward -Z, right +X, up +Y): (x, z, -y)
    if (ProjectHeadSpacePointToScreen(vx, vz, -vy, displaySize, &sc)) {
        // ZOOM COMPENSATION (scope/ADS): the bullet leaves the barrel regardless of zoom, but
        // a scope magnifies the on-screen image by ~Z around the view center while our dot was
        // projected at the un-zoomed HMD FOV. Published zoom factor lives in shared[28] (CET
        // pushes PlayerStateMachine.ZoomLevel). For small angles the screen offset scales ~linearly
        // with Z, so push the dot's offset-from-center out by Z so it tracks the magnified barrel.
        // shared[28] = live camera GetZoom (1.0 normal, ~5.25 scoped) -- the REAL scope magnification
        // (the scope changes GetZoom, not FOV). Scale the dot's offset from screen center by it so the
        // dot tracks the magnified impact while scoped. Published by the CET weapon mod each frame.
        const float zoom = OpenXRManager::Get().GetSharedSlot(28);
        if (zoom > 1.05f) {
            const float cx = displaySize.x * 0.5f, cy = displaySize.y * 0.5f;
            sc.x = cx + (sc.x - cx) * zoom;
            sc.y = cy + (sc.y - cy) * zoom;
        }
        ImDrawList* dl = ImGui::GetForegroundDrawList();
        if (dl) {
            dl->AddCircleFilled(sc, 5.0f, IM_COL32(255, 60, 60, 255));
            dl->AddCircle(sc, 11.0f, IM_COL32(255, 255, 255, 235), 0, 2.0f);
        }
    }
}

template <typename T>
void SafeRelease(T*& value) {
    if (value) {
        value->Release();
        value = nullptr;
    }
}

bool CheckboxInt(const char* label, int* value) {
    bool checked = *value != 0;
    const bool changed = ImGui::Checkbox(label, &checked);
    if (changed) {
        *value = checked ? 1 : 0;
    }
    return changed;
}

bool SliderIntClamped(const char* label, int* value, int minValue, int maxValue) {
    int temp = *value;
    const bool changed = ImGui::SliderInt(label, &temp, minValue, maxValue);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool InputIntClamped(const char* label, int* value, int minValue, int maxValue) {
    int temp = *value;
    const bool changed = ImGui::InputInt(label, &temp, 1, 64);
    if (changed) {
        *value = std::clamp(temp, minValue, maxValue);
    }
    return changed;
}

bool DrawFovControl(LiveControlsUiState& state) {
    bool changed = false;
    bool useRuntime = state.xrForceFov <= 0.0f;
    if (ImGui::Checkbox("Use OpenXR runtime projection FOV", &useRuntime)) {
        state.xrForceFov = useRuntime ? 0.0f : 112.0f;
        changed = true;
    }

    if (useRuntime) {
        ImGui::BeginDisabled();
    }
    float fov = state.xrForceFov <= 0.0f ? 112.0f : state.xrForceFov;
    if (ImGui::SliderFloat("OpenXR projection layer FOV", &fov, 80.0f, 140.0f, "%.1f deg")) {
        state.xrForceFov = fov;
        changed = true;
    }
    if (useRuntime) {
        ImGui::EndDisabled();
    }
    ImGui::TextUnformatted("This changes the OpenXR projection layer FOV, not the CP2077 camera FOV.");
    return changed;
}

// One compact row: <label>  [X][Y][Size]. The label sits in a fixed-width
// column so the three sliders line up across rows.
bool DrawHudXYAndScale(const char* label, float* x, float* y, float* scale) {
    bool changed = false;
    const float kLabelCol = 150.0f;
    ImGui::PushID(label);
    ImGui::AlignTextToFramePadding();
    ImGui::TextUnformatted(label);
    ImGui::SameLine(kLabelCol);
    const float spacing = ImGui::GetStyle().ItemSpacing.x;
    float avail = ImGui::GetContentRegionAvail().x;
    float w = (avail - 2.0f * spacing) / 3.0f;
    if (w < 50.0f) w = 50.0f;
    ImGui::SetNextItemWidth(w);
    changed |= ImGui::SliderFloat("##x", x, -1200.0f, 1200.0f, "X %.0f");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(w);
    changed |= ImGui::SliderFloat("##y", y, -1200.0f, 1200.0f, "Y %.0f");
    ImGui::SameLine();
    ImGui::SetNextItemWidth(w);
    changed |= ImGui::SliderFloat("##s", scale, 0.01f, 2.00f, "S %.2f");
    ImGui::PopID();
    return changed;
}

// Compact single-axis row: <label> [slider].
static bool DrawHudSingle(const char* label, float* v, float lo, float hi, const char* fmt) {
    ImGui::PushID(label);
    ImGui::AlignTextToFramePadding();
    ImGui::TextUnformatted(label);
    ImGui::SameLine(150.0f);
    ImGui::SetNextItemWidth(ImGui::GetContentRegionAvail().x);
    bool changed = ImGui::SliderFloat("##v", v, lo, hi, fmt);
    ImGui::PopID();
    return changed;
}

bool DrawHudControls(LiveControlsUiState& state) {
    bool changed = false;
    ImGui::TextUnformatted("Per-region HUD controls: X right+, Y down+, Size 1.00 = half old size, 2.00 = full old size.");
    changed |= DrawHudXYAndScale("Minimap / Quest", &state.xrHudScale, &state.xrHudScaleY, &state.xrHudMinimapQuestScale);
    changed |= DrawHudXYAndScale("Phone", &state.xrHudPhone, &state.xrHudPhoneY, &state.xrHudPhoneScale);
    changed |= DrawHudXYAndScale("Top-left alerts", &state.xrHudTopLeftAlerts, &state.xrHudTopLeftAlertsY, &state.xrHudTopLeftAlertsScale);
    changed |= DrawHudXYAndScale("Top-right", &state.xrHudTopRight, &state.xrHudTopRightY, &state.xrHudTopRightScale);
    changed |= DrawHudXYAndScale("Bottom-left main", &state.xrHudBottomLeft, &state.xrHudBottomLeftY, &state.xrHudBottomLeftScale);
    changed |= DrawHudXYAndScale("Bottom-left top", &state.xrHudBottomLeftTop, &state.xrHudBottomLeftTopY, &state.xrHudBottomLeftTopScale);
    changed |= DrawHudXYAndScale("Radio", &state.xrHudRadio, &state.xrHudRadioY, &state.xrHudRadioScale);
    changed |= DrawHudXYAndScale("Bottom-right main", &state.xrHudBottomRight, &state.xrHudBottomRightY, &state.xrHudBottomRightScale);
    changed |= DrawHudXYAndScale("Right center", &state.xrHudRightCenter, &state.xrHudRightCenterY, &state.xrHudRightCenterScale);
    changed |= DrawHudSingle("Johnny hint X", &state.xrHudJohnnyHint, -1200.0f, 1200.0f, "X %.0f");
    changed |= DrawHudSingle("Activity log X", &state.xrHudActivityLog, -1200.0f, 1200.0f, "X %.0f");
    changed |= DrawHudSingle("Warning Y", &state.xrHudWarning, -1200.0f, 1200.0f, "Y %.0f");
    changed |= DrawHudSingle("Boss health Y", &state.xrHudBossHealth, -1200.0f, 1200.0f, "Y %.0f");
    changed |= DrawHudSingle("Vehicle scan Y", &state.xrHudVehicleScan, -1200.0f, 1200.0f, "Y %.0f");
    changed |= DrawHudSingle("Progress bar Y", &state.xrHudProgressBar, -1200.0f, 1200.0f, "Y %.0f");
    changed |= DrawHudSingle("Oxygen bar Y", &state.xrHudOxygenBar, -1200.0f, 1200.0f, "Y %.0f");
    ImGui::TextUnformatted("HUD is still screen-space/head-locked. These controls only change placement and readability.");
    return changed;
}

// In-headset VR floating-hands controls: tracking on/off, IK calibration, wrist
// alignment, and the diagnostic dump -- everything that used to live only in the
// desktop CET window. Values are published to the RED4ext arm-IK plugin through
// shared memory (OpenXRManager::SetVRHandCalib). Defaults mirror the plugin's
// baked calibration so the rig behaves identically before anything is touched.
void DrawVRHandsControls() {
    // Tracking toggle (writes shared-mem slot [32]; plugin installs hooks + arms
    // and sets g_VRBind = this value). Must be 4 = full-arm IK (the mode the CET
    // "Start VR Tracking" button uses). Mode 2 is the legacy direct bone-write
    // fallback -> stretched forearm / wrong placement, which is what this was.
    static bool s_vrHandTracking = true;   // default ON — backend's m_vrHandTrackingMode also defaults to 4
    if (ImGui::Checkbox("Start VR hand tracking", &s_vrHandTracking)) {
        OpenXRManager::Get().SetVRHandTrackingMode(s_vrHandTracking ? 4 : 0);
    }
    ImGui::SameLine();
    if (ImGui::Button("Log VR Diag")) {
        OpenXRManager::Get().RequestVRDiag();
    }
    if (ImGui::IsItemHovered()) {
        ImGui::SetTooltip("Dumps gizmo vs. bone poses to vrik_diag.txt (next to dxgi.dll)\n"
                          "for tuning the arm IK. Same as the CET 'Log VR Diag' button.");
    }

    ImGui::Separator();
    ImGui::TextUnformatted("Hand IK Calibration (per hand: R = right, L = left)");

    // Defaults mirror the plugin's baked calibration (main.cpp globals).
    static float scaleR = 1.05f, scaleL = 1.06f;   // reach scale (arm straightening)
    static float heightR = 0.0f, heightL = 0.0f; // vertical fine-tune offset (m)
    static float swingR = 1.0f,  swingL = 1.0f;    // elbow-swing gain
    static float poleR = 0.0f,   poleL = 0.0f;     // elbow pole spin (deg)
    static float wRp = 0.0f, wRy = -90.0f, wRr = 0.0f;     // right wrist euler (deg)
    static float wLp = -180.0f, wLy = -90.0f, wLr = 0.0f;  // left wrist euler (deg)

    bool calChanged = false;
    calChanged |= ImGui::SliderFloat("Reach scale R", &scaleR, 0.80f, 1.30f, "%.3f");
    calChanged |= ImGui::SliderFloat("Reach scale L", &scaleL, 0.80f, 1.30f, "%.3f");
    calChanged |= ImGui::SliderFloat("Height R", &heightR, -0.20f, 0.50f, "%.3f m");
    calChanged |= ImGui::SliderFloat("Height L", &heightL, -0.20f, 0.50f, "%.3f m");
    calChanged |= ImGui::SliderFloat("Elbow swing R", &swingR, -3.0f, 3.0f, "%.2f");
    calChanged |= ImGui::SliderFloat("Elbow swing L", &swingL, -3.0f, 3.0f, "%.2f");
    calChanged |= ImGui::SliderFloat("Elbow pole R", &poleR, -180.0f, 180.0f, "%.1f deg");
    calChanged |= ImGui::SliderFloat("Elbow pole L", &poleL, -180.0f, 180.0f, "%.1f deg");

    ImGui::Separator();
    ImGui::TextUnformatted("Wrist rotation offset (palm/finger alignment, deg)");
    calChanged |= ImGui::SliderFloat("Wrist R pitch", &wRp, -180.0f, 180.0f, "%.1f");
    calChanged |= ImGui::SliderFloat("Wrist R yaw",   &wRy, -180.0f, 180.0f, "%.1f");
    calChanged |= ImGui::SliderFloat("Wrist R roll",  &wRr, -180.0f, 180.0f, "%.1f");
    calChanged |= ImGui::SliderFloat("Wrist L pitch", &wLp, -180.0f, 180.0f, "%.1f");
    calChanged |= ImGui::SliderFloat("Wrist L yaw",   &wLy, -180.0f, 180.0f, "%.1f");
    calChanged |= ImGui::SliderFloat("Wrist L roll",  &wLr, -180.0f, 180.0f, "%.1f");

    ImGui::Separator();
    // Auto-calibration: T-pose sample from the same controller poses that draw the gizmo hands.
    // Press "Start", stretch arms out to the sides, stand straight. We derive shoulder offsets
    // and arm scale, then save to vrik_calibration.ini next to dxgi.dll.
    int  cState = OpenXRManager::Get().GetCalibrationState();
    if (cState == 1) {
        float prog = OpenXRManager::Get().GetCalibrationProgress();
        ImGui::TextColored(ImVec4(1.0f, 0.9f, 0.3f, 1.0f),
                           "AUTO-CALIBRATING: stretch arms STRAIGHT OUT to the sides,");
        ImGui::TextColored(ImVec4(1.0f, 0.9f, 0.3f, 1.0f),
                           "stand straight facing forward. Hold for %.1fs (%.0f%%).",
                           (1.0f - prog) * 4.0f, prog * 100.0f);
        ImGui::ProgressBar(prog, ImVec2(280, 0));
    } else {
        if (ImGui::Button("Start Auto-Calibration (T-pose, 4s)", ImVec2(280, 0))) {
            OpenXRManager::Get().StartAutoCalibration(4.0f);
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Press once, then stretch BOTH gizmo hands OUT to the sides at shoulder height\n"
                              "and stand straight facing forward. The mod measures the visible controller\n"
                              "positions, computes shoulder pivots + arm scale, and saves the result\n"
                              "to vrik_calibration.ini.");
        }
        if (cState == 2) {
            ImGui::SameLine();
            ImGui::TextColored(ImVec4(0.4f, 1.0f, 0.4f, 1.0f), "Saved.");
        }
    }

    ImGui::Separator();
    // CAMERA -> HEAD alignment. The FPP camera is mounted ~0.45 m ahead of the avatar's head, so
    // the view sits in front of the body. Stand straight looking forward and press Bake: the
    // (head - camera) offset is measured and baked, shifting the view back onto the head. The
    // Tracking/Camera "Head" sliders then fine-tune ON TOP (they stay at 0).
    {
        float cb[3]; OpenXRManager::Get().GetCameraOffset(cb);
        ImGui::TextUnformatted("Camera <-> Head alignment");
        if (ImGui::Button("Bake camera onto head", ImVec2(180, 0))) {
            OpenXRManager::Get().BakeCameraOffset();
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Start VR hand tracking, stand straight looking forward, then press.\n"
                              "Measures the head-bone vs FPP-camera offset and moves the view back\n"
                              "onto your avatar's head. Fine-tune with the Tracking/Camera Head sliders.");
        }
        ImGui::SameLine();
        if (ImGui::Button("Clear##cambake", ImVec2(90, 0))) {
            OpenXRManager::Get().ClearCameraOffset();
            OpenXRManager::Get().SaveCalibrationToFile();
        }
        ImGui::Text("baked offset: R %.3f  Fwd %.3f  Up %.3f", cb[0], cb[1], cb[2]);
    }

    ImGui::Separator();
    bool apply  = ImGui::Button("Apply Calibration");
    ImGui::SameLine();
    bool save   = ImGui::Button("Save");
    ImGui::SameLine();
    bool load   = ImGui::Button("Load");
    ImGui::SameLine();
    if (ImGui::Button("Reset Defaults")) {
        scaleR = 1.05f; scaleL = 1.06f; heightR = 0.0f; heightL = 0.0f;
        swingR = 1.0f; swingL = 1.0f; poleR = 0.0f; poleL = 0.0f;
        wRp = 0.0f; wRy = -90.0f; wRr = 0.0f; wLp = -180.0f; wLy = -90.0f; wLr = 0.0f;
        OpenXRManager::Get().SetShoulderAnatomical(0.14f, -0.17f, 0.05f, -0.14f, -0.17f, 0.05f);
        calChanged = true;
    }

    if (calChanged || apply) {
        OpenXRManager::Get().SetVRHandCalib(scaleR, scaleL, heightR, heightL,
                                            swingR, swingL, poleR, poleL,
                                            wRp, wRy, wRr, wLp, wLy, wLr);
    }
    if (save) OpenXRManager::Get().SaveCalibrationToFile();
    if (load) {
        if (OpenXRManager::Get().LoadCalibrationFromFile()) {
            // Pull values back into the sliders so the UI reflects the file.
            // (We don't have direct getters; read via the shared-mem path. For now the user
            // can use the sliders to inspect or just save again.)
        }
    }
}

void ReleaseGameMouseCapture() {
    ClipCursor(nullptr);
    ReleaseCapture();

    CURSORINFO cursorInfo{sizeof(CURSORINFO)};
    if (GetCursorInfo(&cursorInfo) && (cursorInfo.flags & CURSOR_SHOWING) == 0) {
        while (ShowCursor(TRUE) < 0) {
        }
    }
}

void UpdateImGuiMouseFromCursor(HWND hwnd, float backbufferWidth, float backbufferHeight) {
    ImGuiIO& io = ImGui::GetIO();

    RECT client{};
    POINT cursor{};
    if (hwnd && GetClientRect(hwnd, &client) && GetCursorPos(&cursor) && ScreenToClient(hwnd, &cursor)) {
        io.AddMousePosEvent(
            static_cast<float>(cursor.x),
            static_cast<float>(cursor.y));
    }

    io.AddMouseButtonEvent(0, (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0);
    io.AddMouseButtonEvent(1, (GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0);
    io.AddMouseButtonEvent(2, (GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0);
}

bool DrawLiveControls(LiveControlsUiState& state) {
    bool changed = false;

    if (ImGui::Button("Recenter HMD (F7)")) {
        RequestLiveControlsRecenter();
    }
    ImGui::SameLine();
    if (ImGui::Button("Save")) {
        SetLiveControlsUiState(&state, 1);
    }

    if (ImGui::BeginTabBar("CyberpunkVRPortTabs")) {
        if (ImGui::BeginTabItem("General")) {
            if (ImGui::CollapsingHeader("View / Resolution", ImGuiTreeNodeFlags_DefaultOpen)) {
                changed |= DrawFovControl(state);
                changed |= CheckboxInt("VR menu quad", &state.xrMenuRect);
                changed |= ImGui::SliderFloat("VR menu FOV", &state.xrMenuFov, 30.0f, 120.0f, "%.1f deg");
            }

            if (ImGui::CollapsingHeader("Stereo / AER", ImGuiTreeNodeFlags_DefaultOpen)) {
        const char* renderModes[] = {"Mono", "AER"};
        // "AER" is the full alternate-eye + optical-flow synthesis path. Selecting
        // it applies every flag that path needs (mono+AER submit, V2 optical flow,
        // half-rate, render-pose) so there is nothing else to toggle.
        int renderMode = (state.xrAERSubmit != 0 && state.xrAERV2 != 0) ? 1 : 0;
        if (ImGui::Combo("Render mode", &renderMode, renderModes, IM_ARRAYSIZE(renderModes))) {
            if (renderMode == 1) {
                state.xrMonoSubmit = 1;
                state.xrAERSubmit = 1;
                state.xrAERV2 = 1;
                state.xrAERHalfRate = 1;
                state.xrRenderPoseSubmit = 1;
            } else {
                state.xrMonoSubmit = 1;
                state.xrAERSubmit = 0;
                state.xrAERV2 = 0;
                state.xrAERHalfRate = 0;
            }
            changed = true;
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Mono = the flat 2D image rendered once and shown to both eyes.\n"
                              "AER = alternate-eye rendering: the engine renders one eye per\n"
                              "frame at half rate and the mod synthesizes the other eye and the\n"
                              "in-between frames via NVIDIA Optical Flow, for full-refresh stereo.\n"
                              "Selecting AER applies all the flags it needs automatically.");
        }
        changed |= CheckboxInt("AER pair gate", &state.xrAERPairGate);
        changed |= CheckboxInt("AER start right eye", &state.xrAERStartEye);
        const char* debugEyes[] = {"Normal", "Both right", "Both left", "Swap L/R"};
        int debugEye = state.xrAERDebugEye;
        if (debugEye < 0 || debugEye > 3) debugEye = 0;
        if (ImGui::Combo("AER debug eye", &debugEye, debugEyes, IM_ARRAYSIZE(debugEyes))) {
            state.xrAERDebugEye = debugEye;
            changed = true;
        }
        changed |= ImGui::SliderFloat("Motion prediction (ms)", &state.xrMotionPredictMs, 0.0f, 60.0f, "%.1f ms");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Forward-predicts the head pose by this many ms using head\n"
                              "velocity, hiding AER render-to-photon latency. 0 = off.\n"
                              "Tune up until motion feels responsive without overshoot.");
        }
        changed |= ImGui::SliderFloat("Stereo separation x", &state.xrStereoScale, 0.25f, 5.0f, "%.2fx");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Personal fine-tune on the auto IPD. 1.0 = calibrated natural\n"
                              "separation, auto-scaled to the headset's runtime IPD. Nudge\n"
                              "0.8-1.2 for taste; crank to 3-5x to exaggerate depth and make\n"
                              "the eye alternation obvious on the flat monitor for testing.");
        }
        // ── World scale + honest IPD ──
        changed |= ImGui::SliderFloat("World scale", &state.xrWorldScale, 0.20f, 3.0f, "%.2f");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Scales eye separation AND head translation together.\n"
                              "Lower it (e.g. 0.8) to make the world look\n"
                              "BIGGER / yourself smaller; raise to shrink the world. Use this if V\n"
                              "and NPCs feel too large.");
        }
        changed |= ImGui::SliderFloat("IPD scale", &state.xrIpdScale, 0.50f, 2.0f, "%.2fx");
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Eye-separation multiplier on the runtime IPD. 1.0 = the neutral\n"
                              "baseline (+-0.033 m on a typical headset).\n"
                              "Affects stereo depth (diorama vs giant), NOT the monocular size.");
        }
        bool reuseLastFrame = state.xrReuseLastFrame != 0;
        if (ImGui::Checkbox("Reuse last clean frame", &reuseLastFrame)) {
            state.xrReuseLastFrame = reuseLastFrame ? 1 : 0;
            changed = true;
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("On stale AER ticks, re-submit the last clean captured eye and let\n"
                              "the compositor reproject it, instead of warping stale content\n"
                              "again. May lower submit rate\n"
                              "toward the capture rate. Off = always warp the stale eye.");
        }
        bool pairLock = state.xrPairLock != 0;
        if (ImGui::Checkbox("Pose pair-lock", &pairLock)) {
            state.xrPairLock = pairLock ? 1 : 0;
            changed = true;
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("ON (default): freeze the tracked head/hand pose for each stereo\n"
                              "pair so both eyes animate from one state. OFF: live pose every\n"
                              "frame (full-rate avatar, small per-eye tear). Use this as an\n"
                              "anti-tear toggle for avatar/body alignment.");
        }
        // ── NvOF quality (Higher quality AER v2) ──
        const char* nvofItems[] = { "FAST", "MEDIUM", "SLOW (Higher quality)" };
        int nvofPerf = state.xrNvofPerf < 0 ? 0 : (state.xrNvofPerf > 2 ? 2 : state.xrNvofPerf);
        if (ImGui::Combo("NvOF quality", &nvofPerf, nvofItems, IM_ARRAYSIZE(nvofItems))) {
            state.xrNvofPerf = nvofPerf;
            changed = true;
        }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("NVIDIA Optical Flow perf level for the AER V2 synth. SLOW =\n"
                              "densest flow ('Higher quality AER v2', less ghosting) but more\n"
                              "GPU; may not hold 90Hz at full res. Takes effect on next AER\n"
                              "(re)init. Default FAST.");
        }
        // ── AER V2 warp tuning (persisted to vrport.ini via the Save button) ──
        // These read/write the live atomics in openxr_manager.cpp directly, so
        // the effect is immediate while wearing the headset; click Save to keep
        // them across restarts.
        if (state.xrAERV2) {
            ImGui::SeparatorText("AER V2 warp tuning");
            float maxExtrap = GetAerMaxExtrap();
            if (ImGui::SliderFloat("Max extrapolation", &maxExtrap, 1.0f, 3.0f, "%.2f")) {
                SetAerMaxExtrap(maxExtrap);
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("How far the stale eye may be motion-extrapolated past its\n"
                                  "last real frame. Lower (1.0-1.3) = less smear/tearing on fast\n"
                                  "head turns but the stale eye freezes sooner; higher = smoother\n"
                                  "but more edge smear. Default 1.8.");
            }
            // (Motion refinement removed — it only drove pose-reprojection here,
            //  which added edge artifacts; forced 0.)
            float occl = GetAerOcclusionSharp();
            if (ImGui::SliderFloat("Edge sharpness", &occl, 0.0f, 1.0f, "%.2f")) {
                SetAerOcclusionSharp(occl);
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Occlusion edge-sharpening: how hard moving-object edges snap to\n"
                                  "the nearest-in-time frame instead of blending (kills ghosting).\n"
                                  "1 = crisp edges, 0 = smooth blend (more ghost, less shimmer).");
            }
            float fovea = GetAerFoveation() * 100.0f;
            if (ImGui::SliderFloat("Peripheral simplify %", &fovea, 0.0f, 80.0f, "%.0f%%")) {
                SetAerFoveation(fovea / 100.0f);
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Fixed foveation (no eye-tracking): inside a center circle =\n"
                                  "full NvOF warp; outside = the raw frame at reduced resolution\n"
                                  "(blocky), no optical-flow warp. X%% = how much of the lens\n"
                                  "radius becomes periphery. 0%% = off. You should SEE a blocky\n"
                                  "ring at the edges as you raise it. Note: most of the AER GPU\n"
                                  "cost is the optical flow (full-frame), so fps gain is small —\n"
                                  "this is mainly a comfort/artifact-hiding knob.");
            }
        }
            }

            if (ImGui::CollapsingHeader("Tracking / Camera")) {
        ImGui::TextUnformatted("Locomotion direction is set in the Controls tab.");
        changed |= CheckboxInt("Disable Mouse Y (Pitch)", &state.xrDisableMouseY);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Suppress mouse/right-stick pitch so only the HMD controls\n"
                              "vertical look. Applied by the CET VRIK mod and the\n"
                              "XInput merge. On by default.");
        }
        changed |= CheckboxInt("Fix Head", &state.xr3DofMovement);
        if (state.xr3DofMovement == 0) {
            changed |= ImGui::SliderFloat("Head X right", &state.xrHeadOffsetX, -0.50f, 0.50f, "%.3f m");
            changed |= ImGui::SliderFloat("Head Y forward", &state.xrHeadOffsetY, -0.50f, 0.50f, "%.3f m");
            changed |= ImGui::SliderFloat("Head Z up", &state.xrHeadOffsetZ, -0.50f, 0.50f, "%.3f m");
        }
            }

            if (ImGui::CollapsingHeader("Debug Gizmos")) {
                ImGui::TextUnformatted("Raw hand overlay / debug gizmos:");
                ImGui::Checkbox("Enable hand overlay", &g_drawHandLocator);
                ImGui::Checkbox("Draw 3D hand proxy", &g_drawHandProxy3D);
                ImGui::Checkbox("Draw debug wire/axes", &g_drawHandDebugAxes);
                ImGui::SliderFloat("Locator scale", &g_handLocatorScale, 0.50f, 2.00f, "%.2f");
            }

            if (ImGui::CollapsingHeader("DLSS / Debug")) {
        { int vl = g_verboseLog; if (CheckboxInt("Verbose log (spammy diag)", &vl)) g_verboseLog = vl; }
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Off by default for a clean cyberpunkvrport.log. Enable only\n"
                              "when capturing ClipCursor / depth / hook diagnostics.");
        }
        changed |= CheckboxInt("DLSS matrix hook", &state.xrDLSSMatrixHook);
        changed |= SliderIntClamped("DLSS slot mode", &state.xrDLSSSlotMode, 0, 8);
        changed |= SliderIntClamped("DLSS log stride", &state.xrDLSSLogStride, 0, 3000);
            }
            ImGui::EndTabItem();
        }

        if (ImGui::BeginTabItem("Controls")) {
            ImGui::TextWrapped("VR controller input is merged into XInput gamepad 0 so the game's "
                               "native gamepad bindings apply (jump = A, dodge = B, reload = X, "
                               "weapon swap = Y, fire = RT, aim = LT, grenade = RG, scanner = LG).");
            ImGui::Separator();

            // Weapon aim: bullets/projectiles fly down the WEAPON BARREL (controller-pointed) instead
            // of the camera crosshair. Hooks the projectile launch orientation provider and feeds it
            // the game's own muzzle world transform. Writes shared[58]; the RED4ext plugin applies it.
            {
                static bool s_weaponAim = true;   // default ON — backend's m_weaponAimEnable also defaults to 1
                if (ImGui::Checkbox("Bullet from weapon barrel (decoupled VR aim)", &s_weaponAim)) {
                    OpenXRManager::Get().SetWeaponAimEnable(s_weaponAim ? 1 : 0);
                }
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("Shots follow where the WEAPON points (your controller), not the\n"
                                      "camera crosshair. Works for guns and projectiles. Free-look while aiming.");
                }
                ImGui::Checkbox("Barrel crosshair dot (where the bullet goes)", &g_drawBarrelCross);
                if (ImGui::IsItemHovered()) {
                    ImGui::SetTooltip("Red dot projected from the actual weapon muzzle direction through the\n"
                                      "game camera -- marks exactly where the bullet will fly.");
                }
            }
            ImGui::Separator();

            changed |= CheckboxInt("Enable VR -> XInput merge", &state.xrXInputHook);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("OR the VR controller state into XInput gamepad 0 every poll.\n"
                                  "Off = the game only sees a physical pad / nothing.");
            }

            ImGui::Separator();
            ImGui::TextUnformatted("Weapon holsters (reach + right grip)");
            changed |= CheckboxInt("Immersive holsters", &state.xrImmersiveHolsters);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip(
                    "ON  - equip is chosen by the VISUAL holster you reach for:\n"
                    "      over the right shoulder = primary weapon (rifle / sniper),\n"
                    "      hip with a katana = melee, hip with a pistol = sidearm.\n"
                    "OFF - ignore visual holsters, fixed slot per zone:\n"
                    "      over-shoulder = EquipmentSlot1, right hip = Slot2, left hip = Slot3.\n"
                    "Reach to the zone and squeeze the RIGHT grip to equip / unequip.");
            }

            ImGui::Separator();
            ImGui::TextUnformatted("Locomotion direction");
            const char* moveSrcNames[] = { "Game (camera)", "HMD (head)", "Left hand", "Right hand" };
            int moveSrc = state.xrMovementSource;
            if (moveSrc < 0 || moveSrc > 3) moveSrc = state.xrMovementControl != 0 ? 1 : 0;
            if (ImGui::Combo("Move source", &moveSrc, moveSrcNames, IM_ARRAYSIZE(moveSrcNames))) {
                state.xrMovementSource = moveSrc;
                state.xrMovementControl = moveSrc != 0 ? 1 : 0;
                changed = true;
            }
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Game     - left stick walks the way the camera faces (vanilla).\n"
                                  "HMD      - left stick walks the way the headset faces.\n"
                                  "Left/Right hand - walks the way the chosen controller points.\n"
                                  "Vehicles always keep game heading.");
            }

            ImGui::Separator();
            ImGui::TextUnformatted("Turning (right stick)");
            changed |= CheckboxInt("Snap turn", &state.xrSnapTurn);
            if (ImGui::IsItemHovered()) {
                ImGui::SetTooltip("Convert the right-stick X axis into discrete snap pulses\n"
                                  "instead of smooth rotation. Helps with motion sickness.");
            }
            if (state.xrSnapTurn != 0) {
                changed |= ImGui::SliderFloat("Snap angle", &state.xrSnapTurnAngleDeg, 10.0f, 90.0f, "%.0f deg");
            }

            ImGui::Separator();
            ImGui::TextUnformatted("Default binding (CP2077 native gamepad):");
            ImGui::BulletText("Left stick   - walk / jog (push FULL forward = sprint / L3)");
            ImGui::BulletText("Right stick X - turn camera (Y = pitch unless Disable Mouse Y is on)");
            ImGui::BulletText("Right stick FULL down - crouch (R3)");
            ImGui::BulletText("Right A      - JUMP");
            ImGui::BulletText("Right B      - dodge");
            ImGui::BulletText("Left  X      - reload / interact");
            ImGui::BulletText("Left  Y      - weapon switch");
            ImGui::BulletText("Right trigger - fire    | Left trigger - aim");
            ImGui::BulletText("Right grip    - holster equip / unequip | Left grip - crouch (shoulder)");
            ImGui::BulletText("Left  thumb click - sprint (L3) | Right thumb click - crouch (R3)");
            ImGui::BulletText("Left  menu button - pause menu");
            ImGui::TextWrapped("Buttons follow each runtime's interaction profile (Touch / Index / "
                               "Vive / WMR). Customize the actual key bindings in the game's "
                               "in-engine \"Key Bindings -> Controller\" menu.");

            ImGui::EndTabItem();
        }

        if (ImGui::BeginTabItem("VRIK")) {
            DrawVRHandsControls();
            ImGui::EndTabItem();
        }

        if (ImGui::BeginTabItem("HUD")) {
            changed |= DrawHudControls(state);
            ImGui::EndTabItem();
        }

        ImGui::EndTabBar();
    }

    return changed;
}

void ReleaseRenderTargets() {
    for (FrameContext& frame : g_frames) {
        SafeRelease(frame.renderTarget);
        SafeRelease(frame.allocator);
    }
    g_frames.clear();
    SafeRelease(g_rtvHeap);
    SafeRelease(g_cmdList);
    SafeRelease(g_swapChain3);
    g_frameCount = 0;
    g_rtvFormat = DXGI_FORMAT_UNKNOWN;
}

void ShutdownOverlay() {
    ReleaseRenderTargets();
    if (g_imguiInitialized) {
        ImGui_ImplDX12_Shutdown();
        ImGui_ImplWin32_Shutdown();
        ImGui::DestroyContext();
        g_imguiInitialized = false;
    }
    SafeRelease(g_srvHeap);
    SafeRelease(g_fence);
    if (g_fenceEvent) {
        CloseHandle(g_fenceEvent);
        g_fenceEvent = nullptr;
    }
}

void WaitForOverlayGpu() {
    if (!g_queue || !g_fence || !g_fenceEvent) return;
    ++g_fenceValue;
    if (FAILED(g_queue->Signal(g_fence, g_fenceValue))) return;
    if (g_fence->GetCompletedValue() < g_fenceValue) {
        if (SUCCEEDED(g_fence->SetEventOnCompletion(g_fenceValue, g_fenceEvent))) {
            WaitForSingleObject(g_fenceEvent, INFINITE);
        }
    }
}

bool EnsureSwapchainResources(IDXGISwapChain* swapChain) {
    if (!swapChain || !g_device || !g_queue || !g_hwnd) return false;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc)) || desc.BufferCount == 0) {
        return false;
    }

    IDXGISwapChain3* swapChain3 = nullptr;
    if (FAILED(swapChain->QueryInterface(IID_PPV_ARGS(&swapChain3))) || !swapChain3) {
        return false;
    }

    const bool needsResources = !g_swapChain3 || g_frameCount != desc.BufferCount || g_rtvFormat != desc.BufferDesc.Format;
    if (!needsResources) {
        swapChain3->Release();
        return true;
    }

    ReleaseRenderTargets();
    g_swapChain3 = swapChain3;
    g_frameCount = desc.BufferCount;
    g_rtvFormat = desc.BufferDesc.Format;

    D3D12_DESCRIPTOR_HEAP_DESC rtvDesc{};
    rtvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    rtvDesc.NumDescriptors = g_frameCount;
    if (FAILED(g_device->CreateDescriptorHeap(&rtvDesc, IID_PPV_ARGS(&g_rtvHeap)))) {
        ReleaseRenderTargets();
        return false;
    }
    g_rtvDescriptorSize = g_device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);

    g_frames.resize(g_frameCount);
    D3D12_CPU_DESCRIPTOR_HANDLE rtv = g_rtvHeap->GetCPUDescriptorHandleForHeapStart();
    for (UINT i = 0; i < g_frameCount; ++i) {
        FrameContext& frame = g_frames[i];
        if (FAILED(g_device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&frame.allocator)))) {
            ReleaseRenderTargets();
            return false;
        }
        if (FAILED(swapChain->GetBuffer(i, IID_PPV_ARGS(&frame.renderTarget)))) {
            ReleaseRenderTargets();
            return false;
        }
        frame.rtv = rtv;
        g_device->CreateRenderTargetView(frame.renderTarget, nullptr, frame.rtv);
        rtv.ptr += g_rtvDescriptorSize;
    }

    if (FAILED(g_device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, g_frames[0].allocator, nullptr, IID_PPV_ARGS(&g_cmdList)))) {
        ReleaseRenderTargets();
        return false;
    }
    g_cmdList->Close();
    return true;
}

bool EnsureImGui(IDXGISwapChain* swapChain) {
    if (!swapChain || !g_device || !g_queue || !g_hwnd) return false;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) {
        return false;
    }

    if (!g_imguiInitialized) {
        D3D12_DESCRIPTOR_HEAP_DESC srvDesc{};
        srvDesc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
        srvDesc.NumDescriptors = 1;
        srvDesc.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if (FAILED(g_device->CreateDescriptorHeap(&srvDesc, IID_PPV_ARGS(&g_srvHeap)))) {
            return false;
        }

        IMGUI_CHECKVERSION();
        ImGui::CreateContext();
        ImGuiIO& io = ImGui::GetIO();
        io.ConfigFlags |= ImGuiConfigFlags_NavEnableKeyboard;
        io.MouseDrawCursor = true;
        io.IniFilename = nullptr;
        io.FontGlobalScale = 1.35f;
        ImGui::StyleColorsDark();
        ImGui::GetStyle().ScaleAllSizes(1.35f);

        if (!ImGui_ImplWin32_Init(g_hwnd)) {
            ShutdownOverlay();
            return false;
        }
        if (!ImGui_ImplDX12_Init(g_device, static_cast<int>(std::max<UINT>(desc.BufferCount, 2)), desc.BufferDesc.Format, g_srvHeap,
                g_srvHeap->GetCPUDescriptorHandleForHeapStart(), g_srvHeap->GetGPUDescriptorHandleForHeapStart())) {
            ShutdownOverlay();
            return false;
        }

        if (FAILED(g_device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&g_fence)))) {
            ShutdownOverlay();
            return false;
        }
        g_fenceEvent = CreateEventA(nullptr, FALSE, FALSE, nullptr);
        if (!g_fenceEvent) {
            ShutdownOverlay();
            return false;
        }

        g_imguiInitialized = true;
        if (g_verboseLog) Log("ImGui overlay initialized. Toggle with F10 or Insert.\n");
    }

    return EnsureSwapchainResources(swapChain);
}

extern "C" UINT GetForcedDisplayModeWidth();
extern "C" UINT GetForcedDisplayModeHeight();

bool IsBlockedInputMessage(UINT msg) {
    switch (msg) {
    case WM_INPUT:
    case WM_INPUT_DEVICE_CHANGE:
    case WM_MOUSEMOVE:
    case WM_NCMOUSEMOVE:
    case WM_LBUTTONDOWN:
    case WM_LBUTTONUP:
    case WM_RBUTTONDOWN:
    case WM_RBUTTONUP:
    case WM_MBUTTONDOWN:
    case WM_MBUTTONUP:
    case WM_MOUSEWHEEL:
    case WM_MOUSEHWHEEL:
    case WM_KEYDOWN:
    case WM_KEYUP:
    case WM_SYSKEYDOWN:
    case WM_SYSKEYUP:
    case WM_CHAR:
        return true;
    default:
        return false;
    }
}

LRESULT CALLBACK OverlayWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam) {
    static int totalMsgCount = 0;
    if (g_verboseLog && totalMsgCount++ % 5000 == 0) {
        Log("OverlayWndProc: msg=%u, hwnd=%p, count=%d\n", msg, hwnd, totalMsgCount);
    }

    // 1. Scale mouse coordinates FIRST so ImGui and game receive the scaled input
    if (msg == WM_MOUSEMOVE || msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP || msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP || msg == WM_MBUTTONDOWN || msg == WM_MBUTTONUP) {
        UINT virtualWidth = GetForcedDisplayModeWidth();
        UINT virtualHeight = GetForcedDisplayModeHeight();
        if (virtualWidth > 0 && virtualHeight > 0) {
            RECT rect;
            if (GetClientRect(hwnd, &rect)) {
                int winWidth = rect.right - rect.left;
                int winHeight = rect.bottom - rect.top;
                
                int x = (short)LOWORD(lParam);
                int y = (short)HIWORD(lParam);
                int oldX = x;
                int oldY = y;
                
                if (winWidth > 0 && winHeight > 0 && (winWidth != static_cast<int>(virtualWidth) || winHeight != static_cast<int>(virtualHeight))) {
                    x = (x * static_cast<int>(virtualWidth)) / winWidth;
                    y = (y * static_cast<int>(virtualHeight)) / winHeight;
                    
                    lParam = MAKELPARAM(static_cast<WORD>(x), static_cast<WORD>(y));
                }

                static int mouseLogCount = 0;
                if (g_verboseLog && mouseLogCount++ % 100 == 0) {
                    Log("OverlayWndProc (scaled): msg=%u physical=(%d,%d) -> scaled=(%d,%d) win=%dx%d virt=%ux%u g_menuVisible=%d\n",
                        msg, oldX, oldY, x, y, winWidth, winHeight, virtualWidth, virtualHeight, g_menuVisible ? 1 : 0);
                }
            }
        }
    }

    // 2. Handle menu toggle
    if ((msg == WM_KEYUP || msg == WM_SYSKEYUP) && (wParam == VK_F10 || wParam == VK_INSERT)) {
        g_menuVisible = !g_menuVisible;
        if (g_menuVisible) {
            ReleaseGameMouseCapture();
        }
        if (g_verboseLog) Log("ImGui overlay %s.\n", g_menuVisible ? "shown" : "hidden");
        return 0;
    }

    // 3. Feed to ImGui if visible
    if (g_menuVisible) {
        if (g_imguiInitialized && ImGui_ImplWin32_WndProcHandler(hwnd, msg, wParam, lParam)) {
            return 1;
        }
        if (IsBlockedInputMessage(msg)) {
            return 0;
        }
    }

    return g_originalWndProc ? CallWindowProcA(g_originalWndProc, hwnd, msg, wParam, lParam) : DefWindowProcA(hwnd, msg, wParam, lParam);
}
}

void OverlaySetDeviceAndQueue(ID3D12Device* device, ID3D12CommandQueue* queue) {
    if (device == g_device && queue == g_queue) return;

    ShutdownOverlay();
    SafeRelease(g_device);
    SafeRelease(g_queue);
    if (device) {
        g_device = device;
        g_device->AddRef();
    }
    if (queue) {
        g_queue = queue;
        g_queue->AddRef();
    }
}

void OverlaySetWindow(HWND hwnd) {
    if (g_verboseLog) Log("OverlaySetWindow called: hwnd=%p (previous g_hwnd=%p)\n", hwnd, g_hwnd);
    if (!hwnd || hwnd == g_hwnd) return;

    if (g_hwnd && g_originalWndProc) {
        SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(g_originalWndProc));
        if (g_verboseLog) Log("OverlaySetWindow: Restored original WndProc on old hwnd %p\n", g_hwnd);
    }
    g_hwnd = hwnd;
    g_originalWndProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(OverlayWndProc)));
    if (g_verboseLog) Log("OverlaySetWindow: Subclassed hwnd %p, original WndProc=%p, new WndProc=%p\n", g_hwnd, g_originalWndProc, OverlayWndProc);
}

void OverlayRender(IDXGISwapChain* swapChain) {
    if (!EnsureImGui(swapChain)) return;

    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swapChain->GetDesc(&desc))) return;

    const UINT frameIndex = g_swapChain3 ? g_swapChain3->GetCurrentBackBufferIndex() : 0;
    if (frameIndex >= g_frames.size()) return;
    FrameContext& frame = g_frames[frameIndex];
    if (!frame.renderTarget || !frame.allocator || !g_cmdList) return;

    ImGui_ImplDX12_NewFrame();
    ImGui_ImplWin32_NewFrame();

    ImGuiIO& io = ImGui::GetIO();
    float backbufferWidth = static_cast<float>(desc.BufferDesc.Width);
    float backbufferHeight = static_cast<float>(desc.BufferDesc.Height);
    if ((backbufferWidth <= 1.0f || backbufferHeight <= 1.0f) && frame.renderTarget) {
        const D3D12_RESOURCE_DESC resourceDesc = frame.renderTarget->GetDesc();
        backbufferWidth = static_cast<float>(resourceDesc.Width);
        backbufferHeight = static_cast<float>(resourceDesc.Height);
    }
    if (backbufferWidth > 1.0f && backbufferHeight > 1.0f) {
        io.DisplaySize = ImVec2(backbufferWidth, backbufferHeight);
        io.DisplayFramebufferScale = ImVec2(1.0f, 1.0f);
    }
    if (g_menuVisible) {
        ReleaseGameMouseCapture();
        UpdateImGuiMouseFromCursor(desc.OutputWindow, backbufferWidth, backbufferHeight);
    }

    ImGui::NewFrame();

    DrawHandLocatorOverlay();
    DrawBarrelCrosshair();

    LiveControlsUiState state{};
    GetLiveControlsUiState(&state);

    bool changed = false;
    if (g_menuVisible) {
        const ImVec2 display = ImGui::GetIO().DisplaySize;
        const ImVec2 menuSize(std::min(1000.0f, display.x * 0.58f), std::min(1180.0f, display.y * 0.64f));
        ImGui::SetNextWindowSize(menuSize, ImGuiCond_Appearing);
        ImGui::SetNextWindowPos(ImVec2((display.x - menuSize.x) * 0.5f, (display.y - menuSize.y) * 0.5f), ImGuiCond_Appearing);
        ImGui::Begin("CyberpunkVRPort Controls", &g_menuVisible, ImGuiWindowFlags_NoCollapse);
        ImGui::TextUnformatted("F10 / Insert: toggle menu");
        ImGui::Separator();
        changed = DrawLiveControls(state);
        ImGui::End();
    }

    if (changed) {
        SetLiveControlsUiState(&state, 1);
    }

    ImGui::Render();

    frame.allocator->Reset();
    g_cmdList->Reset(frame.allocator, nullptr);

    D3D12_RESOURCE_BARRIER toRt{};
    toRt.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toRt.Transition.pResource = frame.renderTarget;
    toRt.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
    toRt.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toRt.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_cmdList->ResourceBarrier(1, &toRt);

    g_cmdList->OMSetRenderTargets(1, &frame.rtv, FALSE, nullptr);
    ID3D12DescriptorHeap* heaps[] = {g_srvHeap};
    g_cmdList->SetDescriptorHeaps(1, heaps);
    ImGui_ImplDX12_RenderDrawData(ImGui::GetDrawData(), g_cmdList);

    D3D12_RESOURCE_BARRIER toPresent{};
    toPresent.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    toPresent.Transition.pResource = frame.renderTarget;
    toPresent.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
    toPresent.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
    toPresent.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    g_cmdList->ResourceBarrier(1, &toPresent);

    g_cmdList->Close();
    ID3D12CommandList* lists[] = {g_cmdList};
    g_queue->ExecuteCommandLists(1, lists);
    WaitForOverlayGpu();
}

void OverlayInvalidateSwapchainResources() {
    ReleaseRenderTargets();
}

bool OverlayIsVisible() {
    return g_menuVisible;
}
