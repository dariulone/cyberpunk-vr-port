// Pictures hung in the tracking space, drawn into an eye's image (Render/SpaceQuad.hpp).
#include "Render/SpaceQuad.hpp"
#include <d3dcompiler.h>
#include <algorithm>
#include <cmath>
#include <cstring>

extern void Log(const char*, ...);

using Microsoft::WRL::ComPtr;

namespace cvr::render {
namespace {
// b0: the quad's four corners as clip coordinates (top left, top right, bottom left, bottom right); the texture's
// part on it (u0, v0, u1, v1); x = opacity, y = black point; the fill's colour.
constexpr UINT kConstants = 28;
// A shader-visible descriptor is read when the list runs, not when it is recorded: the ring is long enough for every
// draw of the capture lists in flight (three, each a handful of draws and a picture's mip chain, a dozen levels).
// Slot 0 is a null view, for the fill.
constexpr UINT kDescriptors = 256;

// A picture on a quad is read through its mips by the footprint of each pixel, anisotropically (s0): a quad far
// smaller on the eye than its picture would alias at one tap of the top level. The resample pass reads one level at
// one bilinear tap (s1): a pixel of a level half the size is the mean of the 2 x 2 under it.
constexpr char kShader[] = R"(
Texture2D<float4> g_picture : register(t0);
SamplerState g_footprint : register(s0);
SamplerState g_linear : register(s1);
cbuffer Quad : register(b0) { float4 g_corner[4]; float4 g_uv; float4 g_params; float4 g_colour; };
struct VSOut { float4 position : SV_Position; float2 uv : TEXCOORD0; };
VSOut VSMain(uint id : SV_VertexID) {
    VSOut o;
    o.position = g_corner[id];
    o.uv = lerp(g_uv.xy, g_uv.zw, float2(id & 1, id >> 1));
    return o;
}
float4 PSFill(VSOut i) : SV_Target { return g_colour; }
float4 PSOver(VSOut i) : SV_Target { return float4(g_picture.Sample(g_footprint, i.uv).rgb, g_params.x); }
float4 PSCopy(VSOut i) : SV_Target { return float4(g_picture.SampleLevel(g_linear, i.uv, 0.0).rgb, 1.0); }
float4 PSLight(VSOut i) : SV_Target {
    float3 c = g_picture.Sample(g_footprint, i.uv).rgb;
    if (g_params.y > 0.0) {
        const float peak = max(c.r, max(c.g, c.b));
        c *= saturate((peak - g_params.y) / max(peak * (1.0 - g_params.y), 1e-5));
    }
    return float4(c * g_params.x, 0.0);
}
)";

bool Compile(const char* entry, const char* target, ComPtr<ID3DBlob>& out) {
    ComPtr<ID3DBlob> errors;
    if (SUCCEEDED(D3DCompile(kShader, std::strlen(kShader), nullptr, nullptr, nullptr, entry, target,
                             D3DCOMPILE_ENABLE_STRICTNESS, 0, &out, &errors)))
        return true;
    Log("[space-quad] shader %s failed: %.*s\n", entry, errors ? int(errors->GetBufferSize()) : 0,
        errors ? static_cast<const char*>(errors->GetBufferPointer()) : "");
    return false;
}

XrVector3f Rotate(const XrQuaternionf& q, const XrVector3f& v) {
    const float tx = 2 * (q.y * v.z - q.z * v.y), ty = 2 * (q.z * v.x - q.x * v.z), tz = 2 * (q.x * v.y - q.y * v.x);
    return {v.x + q.w * tx + (q.y * tz - q.z * ty), v.y + q.w * ty + (q.z * tx - q.x * tz),
            v.z + q.w * tz + (q.x * ty - q.y * tx)};
}

// A point of the tracking space as this eye's clip coordinates (OpenXR: -Z ahead, +Y up). Depth is kept inside
// 0..w for anything ahead of the eye: there is no depth buffer, only the eye's own plane clips.
void Clip(const XrPosef& eye, const XrFovf& fov, const XrVector3f& point, float* out) {
    const XrQuaternionf inverse{-eye.orientation.x, -eye.orientation.y, -eye.orientation.z, eye.orientation.w};
    const auto v = Rotate(inverse, {point.x - eye.position.x, point.y - eye.position.y, point.z - eye.position.z});
    const float w = -v.z;
    const float l = std::tan(fov.angleLeft), r = std::tan(fov.angleRight);
    const float d = std::tan(fov.angleDown), u = std::tan(fov.angleUp);
    out[0] = (2.0f * v.x - (r + l) * w) / (r - l);
    out[1] = (2.0f * v.y - (u + d) * w) / (u - d);
    out[2] = 0.5f * w;
    out[3] = w;
}

// The quad's corners as the eye's clip coordinates, in the order the strip draws them.
void Corners(const XrPosef& eye, const XrFovf& fov, const SpaceQuad& quad, float* out) {
    const auto right = Rotate(quad.pose.orientation, {quad.width * 0.5f, 0.0f, 0.0f});
    const auto up = Rotate(quad.pose.orientation, {0.0f, quad.height * 0.5f, 0.0f});
    const auto& c = quad.pose.position;
    const XrVector3f corners[4] = {{c.x - right.x + up.x, c.y - right.y + up.y, c.z - right.z + up.z},
                                   {c.x + right.x + up.x, c.y + right.y + up.y, c.z + right.z + up.z},
                                   {c.x - right.x - up.x, c.y - right.y - up.y, c.z - right.z - up.z},
                                   {c.x + right.x - up.x, c.y + right.y - up.y, c.z + right.z - up.z}};
    for (int i = 0; i < 4; ++i) Clip(eye, fov, corners[i], out + 4 * i);
}
}  // namespace

bool SpaceQuads::EnsureDevice(ID3D12Device* device) {
    if (device_) return device_.Get() == device;
    if (failed_) return false;
    failed_ = true;
    if (!Compile("VSMain", "vs_5_0", vs_) || !Compile("PSFill", "ps_5_0", psFill_) ||
        !Compile("PSOver", "ps_5_0", psOver_) || !Compile("PSLight", "ps_5_0", psLight_) ||
        !Compile("PSCopy", "ps_5_0", psCopy_))
        return false;
    D3D12_DESCRIPTOR_RANGE range{};
    range.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
    range.NumDescriptors = 1;
    D3D12_ROOT_PARAMETER params[2]{};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    params[0].DescriptorTable = {1, &range};
    params[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
    params[1].Constants.Num32BitValues = kConstants;
    params[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_ALL;
    D3D12_STATIC_SAMPLER_DESC samplers[2]{};
    for (UINT i = 0; i < 2; ++i) {
        auto& s = samplers[i];
        s.AddressU = s.AddressV = s.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
        s.MaxLOD = D3D12_FLOAT32_MAX;
        s.ShaderRegister = i;
        s.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
    }
    samplers[0].Filter = D3D12_FILTER_ANISOTROPIC;
    samplers[0].MaxAnisotropy = 8;
    samplers[1].Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    D3D12_ROOT_SIGNATURE_DESC desc{2, params, 2, samplers, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> blob, errors;
    if (FAILED(D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &blob, &errors)) ||
        FAILED(device->CreateRootSignature(0, blob->GetBufferPointer(), blob->GetBufferSize(), IID_PPV_ARGS(&root_))))
        return false;
    D3D12_DESCRIPTOR_HEAP_DESC heap{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, kDescriptors,
                                    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, 0};
    if (FAILED(device->CreateDescriptorHeap(&heap, IID_PPV_ARGS(&srvHeap_)))) return false;
    heap = {D3D12_DESCRIPTOR_HEAP_TYPE_RTV, kDescriptors, D3D12_DESCRIPTOR_HEAP_FLAG_NONE, 0};
    if (FAILED(device->CreateDescriptorHeap(&heap, IID_PPV_ARGS(&rtvHeap_)))) return false;
    srvStride_ = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    rtvStride_ = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    D3D12_SHADER_RESOURCE_VIEW_DESC none{};
    none.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    none.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    none.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    none.Texture2D.MipLevels = 1;
    device->CreateShaderResourceView(nullptr, &none, srvHeap_->GetCPUDescriptorHandleForHeapStart());
    device_ = device;
    failed_ = false;
    return true;
}

const SpaceQuads::Pipelines* SpaceQuads::PipelinesFor(DXGI_FORMAT format) {
    if (auto it = pipelines_.find(format); it != pipelines_.end()) return &it->second;
    D3D12_GRAPHICS_PIPELINE_STATE_DESC desc{};
    desc.pRootSignature = root_.Get();
    desc.VS = {vs_->GetBufferPointer(), vs_->GetBufferSize()};
    desc.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
    desc.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
    desc.RasterizerState.DepthClipEnable = TRUE;
    desc.SampleMask = UINT_MAX;
    desc.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    desc.NumRenderTargets = 1;
    desc.RTVFormats[0] = format;
    desc.SampleDesc.Count = 1;
    auto& blend = desc.BlendState.RenderTarget[0];
    blend.BlendEnable = TRUE;
    blend.BlendOpAlpha = blend.BlendOp = D3D12_BLEND_OP_ADD;
    blend.SrcBlendAlpha = D3D12_BLEND_ZERO;
    blend.DestBlendAlpha = D3D12_BLEND_ONE;
    blend.RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL;
    // The fill and a solid picture blend by their alpha; light adds; a resample writes.
    const auto make = [&](ID3DBlob* ps, bool light, ComPtr<ID3D12PipelineState>& out) {
        desc.PS = {ps->GetBufferPointer(), ps->GetBufferSize()};
        blend.SrcBlend = light ? D3D12_BLEND_ONE : D3D12_BLEND_SRC_ALPHA;
        blend.DestBlend = light ? D3D12_BLEND_ONE : D3D12_BLEND_INV_SRC_ALPHA;
        return SUCCEEDED(device_->CreateGraphicsPipelineState(&desc, IID_PPV_ARGS(&out)));
    };
    Pipelines made;
    if (!make(psFill_.Get(), false, made.fill) || !make(psOver_.Get(), false, made.over) ||
        !make(psLight_.Get(), true, made.light))
        return nullptr;
    blend.BlendEnable = FALSE;
    if (!make(psCopy_.Get(), false, made.copy)) return nullptr;
    Log("[space-quad] pipelines for format %u\n", static_cast<unsigned>(format));
    return &pipelines_.emplace(format, std::move(made)).first->second;
}

UINT SpaceQuads::Bind(ID3D12GraphicsCommandList* list, const Target& target) {
    const UINT slot = 1 + cursor_;
    cursor_ = (cursor_ + 1) % (kDescriptors - 1);
    auto rtv = rtvHeap_->GetCPUDescriptorHandleForHeapStart();
    rtv.ptr += SIZE_T(slot) * rtvStride_;
    D3D12_RENDER_TARGET_VIEW_DESC view{};
    view.Format = target.view;
    view.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
    view.Texture2D.MipSlice = target.mip;
    device_->CreateRenderTargetView(target.resource, &view, rtv);
    const auto& r = target.rect;
    const D3D12_VIEWPORT viewport{float(r.left), float(r.top), float(r.right - r.left), float(r.bottom - r.top), 0, 1};
    list->RSSetViewports(1, &viewport);
    list->RSSetScissorRects(1, &r);
    list->OMSetRenderTargets(1, &rtv, FALSE, nullptr);
    list->SetGraphicsRootSignature(root_.Get());
    ID3D12DescriptorHeap* heaps[] = {srvHeap_.Get()};
    list->SetDescriptorHeaps(1, heaps);
    list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    return slot;
}

bool SpaceQuads::Fill(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target, const float rgba[4]) {
    if (!list || !target.resource || target.rect.right <= target.rect.left || target.rect.bottom <= target.rect.top ||
        !EnsureDevice(device))
        return false;
    const auto* pipelines = PipelinesFor(target.view);
    if (!pipelines) return false;
    Bind(list, target);
    list->SetGraphicsRootDescriptorTable(0, srvHeap_->GetGPUDescriptorHandleForHeapStart());
    float constants[kConstants]{};
    const float corners[16] = {-1, 1, 0.5f, 1, 1, 1, 0.5f, 1, -1, -1, 0.5f, 1, 1, -1, 0.5f, 1};
    std::memcpy(constants, corners, sizeof(corners));
    std::memcpy(constants + 24, rgba, 4 * sizeof(float));
    list->SetPipelineState(pipelines->fill.Get());
    list->SetGraphicsRoot32BitConstants(1, kConstants, constants, 0);
    list->DrawInstanced(4, 1, 0, 0);
    return true;
}

bool SpaceQuads::Solid(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target,
                       const XrPosef& eye, const XrFovf& fov, const SpaceQuad& quad, const float rgba[4]) {
    if (!list || !target.resource || target.rect.right <= target.rect.left || target.rect.bottom <= target.rect.top ||
        !EnsureDevice(device))
        return false;
    const auto* pipelines = PipelinesFor(target.view);
    if (!pipelines) return false;
    Bind(list, target);
    list->SetGraphicsRootDescriptorTable(0, srvHeap_->GetGPUDescriptorHandleForHeapStart());
    float constants[kConstants]{};
    Corners(eye, fov, quad, constants);
    std::memcpy(constants + 24, rgba, 4 * sizeof(float));
    list->SetPipelineState(pipelines->fill.Get());
    list->SetGraphicsRoot32BitConstants(1, kConstants, constants, 0);
    list->DrawInstanced(4, 1, 0, 0);
    return true;
}

bool SpaceQuads::Draw(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target, const XrPosef& eye,
                      const XrFovf& fov, ID3D12Resource* texture, DXGI_FORMAT view, const SpaceQuad& quad, Blend blend,
                      float opacity, float blackPoint) {
    if (!list || !texture || !target.resource || target.rect.right <= target.rect.left ||
        target.rect.bottom <= target.rect.top || !EnsureDevice(device))
        return false;
    const auto* pipelines = PipelinesFor(target.view);
    if (!pipelines) return false;
    const UINT slot = Bind(list, target);
    BindPicture(list, slot, texture, view, 0, UINT(-1));

    float constants[kConstants]{};
    Corners(eye, fov, quad, constants);
    std::memcpy(constants + 16, quad.uv, 4 * sizeof(float));
    constants[20] = std::clamp(opacity, 0.0f, 1.0f);
    constants[21] = blackPoint;
    list->SetPipelineState(blend == Blend::Light ? pipelines->light.Get() : pipelines->over.Get());
    list->SetGraphicsRoot32BitConstants(1, kConstants, constants, 0);
    list->DrawInstanced(4, 1, 0, 0);
    return true;
}

void SpaceQuads::BindPicture(ID3D12GraphicsCommandList* list, UINT slot, ID3D12Resource* texture, DXGI_FORMAT view,
                             UINT mip, UINT levels) {
    auto srvCpu = srvHeap_->GetCPUDescriptorHandleForHeapStart();
    auto srvGpu = srvHeap_->GetGPUDescriptorHandleForHeapStart();
    srvCpu.ptr += SIZE_T(slot) * srvStride_;
    srvGpu.ptr += UINT64(slot) * srvStride_;
    D3D12_SHADER_RESOURCE_VIEW_DESC srv{};
    srv.Format = view;
    srv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    srv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    srv.Texture2D.MostDetailedMip = mip;
    srv.Texture2D.MipLevels = levels;
    device_->CreateShaderResourceView(texture, &srv, srvCpu);
    list->SetGraphicsRootDescriptorTable(0, srvGpu);
}

bool SpaceQuads::Resample(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target,
                          ID3D12Resource* texture, DXGI_FORMAT view, UINT mip, const float uv[4]) {
    if (!list || !texture || !target.resource || target.rect.right <= target.rect.left ||
        target.rect.bottom <= target.rect.top || !EnsureDevice(device))
        return false;
    const auto* pipelines = PipelinesFor(target.view);
    if (!pipelines) return false;
    const UINT slot = Bind(list, target);
    BindPicture(list, slot, texture, view, mip, 1);
    float constants[kConstants]{};
    const float corners[16] = {-1, 1, 0.5f, 1, 1, 1, 0.5f, 1, -1, -1, 0.5f, 1, 1, -1, 0.5f, 1};
    std::memcpy(constants, corners, sizeof(corners));
    std::memcpy(constants + 16, uv, 4 * sizeof(float));
    list->SetPipelineState(pipelines->copy.Get());
    list->SetGraphicsRoot32BitConstants(1, kConstants, constants, 0);
    list->DrawInstanced(4, 1, 0, 0);
    return true;
}

void SpaceQuads::Shutdown() {
    pipelines_.clear();
    srvHeap_.Reset();
    rtvHeap_.Reset();
    root_.Reset();
    vs_.Reset();
    psFill_.Reset();
    psOver_.Reset();
    psLight_.Reset();
    psCopy_.Reset();
    device_.Reset();
    cursor_ = 0;
    failed_ = false;
}
}  // namespace cvr::render
