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

extern void Log(const char* fmt, ...);

extern IMGUI_IMPL_API LRESULT ImGui_ImplWin32_WndProcHandler(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam);

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

bool DrawHudXYAndScale(const char* label, float* x, float* y, float* scale) {
    bool changed = false;
    const std::string xLabel = std::string(label) + " X";
    const std::string yLabel = std::string(label) + " Y";
    const std::string sizeLabel = std::string(label) + " Size";
    changed |= ImGui::SliderFloat(xLabel.c_str(), x, -1200.0f, 1200.0f, "X %.0f px");
    changed |= ImGui::SliderFloat(yLabel.c_str(), y, -1200.0f, 1200.0f, "Y %.0f px");
    changed |= ImGui::SliderFloat(sizeLabel.c_str(), scale, 0.01f, 2.00f, "Size %.2f");
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
    changed |= ImGui::SliderFloat("Johnny hint X", &state.xrHudJohnnyHint, -1200.0f, 1200.0f, "X %.0f px");
    changed |= ImGui::SliderFloat("Activity log X", &state.xrHudActivityLog, -1200.0f, 1200.0f, "X %.0f px");
    changed |= ImGui::SliderFloat("Warning Y", &state.xrHudWarning, -1200.0f, 1200.0f, "Y %.0f px");
    changed |= ImGui::SliderFloat("Boss health Y", &state.xrHudBossHealth, -1200.0f, 1200.0f, "Y %.0f px");
    changed |= ImGui::SliderFloat("Vehicle scan Y", &state.xrHudVehicleScan, -1200.0f, 1200.0f, "Y %.0f px");
    changed |= ImGui::SliderFloat("Progress bar Y", &state.xrHudProgressBar, -1200.0f, 1200.0f, "Y %.0f px");
    changed |= ImGui::SliderFloat("Oxygen bar Y", &state.xrHudOxygenBar, -1200.0f, 1200.0f, "Y %.0f px");
    ImGui::TextUnformatted("HUD is still screen-space/head-locked. These controls only change placement and readability.");
    return changed;
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
        int renderMode = state.xrAERSubmit != 0 ? 1 : 0;
        if (ImGui::Combo("Render mode", &renderMode, renderModes, IM_ARRAYSIZE(renderModes))) {
            state.xrMonoSubmit = 1;
            state.xrAERSubmit = renderMode == 1 ? 1 : 0;
            changed = true;
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
        changed |= CheckboxInt("Render-pose submit", &state.xrRenderPoseSubmit);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Submit each eye with the exact head pose its frame was rendered\n"
                              "with, so the runtime time-warps the older (1/2-rate) eye forward\n"
                              "to display time. Fixes left-eye judder on head turns. Off =\n"
                              "legacy present-time pose (both eyes share one latched pose).");
        }
        changed |= CheckboxInt("Half-rate submit (SSW/ASW)", &state.xrAERHalfRate);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Present only fresh AER pairs (1/2 rate) instead of resubmitting\n"
                              "stale ones, so the runtime engages motion smoothing (SSW/ASW)\n"
                              "to synthesize the in-between frames. REQUIRES SSW/Spacewarp\n"
                              "enabled in the runtime (Virtual Desktop / SteamVR); without it\n"
                              "the image will look like half-fps judder.");
        }
        changed |= CheckboxInt("AER V2 optical flow (WIP)", &state.xrAERV2);
        if (ImGui::IsItemHovered()) {
            ImGui::SetTooltip("Experimental optical-flow AER V2 path. Current build only\n"
                              "initializes NVIDIA Optical Flow and records the previous/current\n"
                              "per-eye frame history needed for interpolation. The synthesized\n"
                              "submit path is the next step.");
        }
            }

            if (ImGui::CollapsingHeader("Tracking / Camera")) {
        const char* movementControlModes[] = {"Game", "HMD"};
        int movementControl = state.xrMovementControl == 1 ? 1 : 0;
        if (ImGui::Combo("Movement Control", &movementControl, movementControlModes, IM_ARRAYSIZE(movementControlModes))) {
            state.xrMovementControl = movementControl;
            changed = true;
        }
        ImGui::TextUnformatted("HMD mode is on-foot only. Vehicles keep game heading.");
        changed |= CheckboxInt("Fix Head", &state.xr3DofMovement);
        if (state.xr3DofMovement == 0) {
            changed |= ImGui::SliderFloat("Head X right", &state.xrHeadOffsetX, -0.50f, 0.50f, "%.3f m");
            changed |= ImGui::SliderFloat("Head Y forward", &state.xrHeadOffsetY, -0.50f, 0.50f, "%.3f m");
            changed |= ImGui::SliderFloat("Head Z up", &state.xrHeadOffsetZ, -0.50f, 0.50f, "%.3f m");
        }
            }

            if (ImGui::CollapsingHeader("Hand Proxies & Weapons")) {
                // Drive the in-game skeletal hand tracking (RED4ext plugin) straight
                // from the headset, so it can be toggled without the desktop CET menu.
                static bool s_vrHandTracking = false;
                if (ImGui::Checkbox("Enable VR hand tracking (write to skeleton)", &s_vrHandTracking)) {
                    OpenXRManager::Get().SetVRHandTrackingMode(s_vrHandTracking ? 2 : 0);
                }
                ImGui::Separator();
                ImGui::TextUnformatted("Raw hand overlay modes:");
                ImGui::Checkbox("Enable hand overlay", &g_drawHandLocator);
                ImGui::Checkbox("Draw 3D hand proxy", &g_drawHandProxy3D);
                ImGui::Checkbox("Draw debug wire/axes", &g_drawHandDebugAxes);
                ImGui::SliderFloat("Locator scale", &g_handLocatorScale, 0.50f, 2.00f, "%.2f");
                ImGui::Separator();
                ImGui::TextUnformatted("Align the in-game weapon to match your physical controller.");
                changed |= ImGui::SliderFloat("Weapon Pitch", &state.xrWeaponPitch, -180.0f, 180.0f, "%.1f deg");
                changed |= ImGui::SliderFloat("Weapon Yaw", &state.xrWeaponYaw, -180.0f, 180.0f, "%.1f deg");
                changed |= ImGui::SliderFloat("Weapon Roll", &state.xrWeaponRoll, -180.0f, 180.0f, "%.1f deg");
                changed |= ImGui::SliderFloat("Weapon X (Right)", &state.xrWeaponOffsetX, -0.5f, 0.5f, "%.3f m");
                changed |= ImGui::SliderFloat("Weapon Y (Forward)", &state.xrWeaponOffsetY, -0.5f, 0.5f, "%.3f m");
                changed |= ImGui::SliderFloat("Weapon Z (Up)", &state.xrWeaponOffsetZ, -0.5f, 0.5f, "%.3f m");
            }

            if (ImGui::CollapsingHeader("DLSS / Debug")) {
        changed |= CheckboxInt("DLSS matrix hook", &state.xrDLSSMatrixHook);
        changed |= SliderIntClamped("DLSS slot mode", &state.xrDLSSSlotMode, 0, 8);
        changed |= SliderIntClamped("DLSS log stride", &state.xrDLSSLogStride, 0, 3000);
            }
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
        Log("ImGui overlay initialized. Toggle with F10 or Insert.\n");
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
    if (totalMsgCount++ % 5000 == 0) {
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
                if (mouseLogCount++ % 100 == 0) {
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
        Log("ImGui overlay %s.\n", g_menuVisible ? "shown" : "hidden");
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
    Log("OverlaySetWindow called: hwnd=%p (previous g_hwnd=%p)\n", hwnd, g_hwnd);
    if (!hwnd || hwnd == g_hwnd) return;

    if (g_hwnd && g_originalWndProc) {
        SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(g_originalWndProc));
        Log("OverlaySetWindow: Restored original WndProc on old hwnd %p\n", g_hwnd);
    }
    g_hwnd = hwnd;
    g_originalWndProc = reinterpret_cast<WNDPROC>(SetWindowLongPtrA(g_hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(OverlayWndProc)));
    Log("OverlaySetWindow: Subclassed hwnd %p, original WndProc=%p, new WndProc=%p\n", g_hwnd, g_originalWndProc, OverlayWndProc);
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
