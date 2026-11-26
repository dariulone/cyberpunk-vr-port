// Pixel readback on the software adapter. No HWND, VR runtime, or game process.
#include "Render/DesktopMirror.hpp"
#include <cstdint>
#include <cstdio>
#include <stdexcept>

using Microsoft::WRL::ComPtr;
using cvr::MonitorOutput;
void Log(const char*, ...) {}
extern "C" UINT GetForcedWindowWidth() { return 0; }
extern "C" UINT GetForcedDisplayModeWidth() { return 0; }
extern "C" UINT GetForcedDisplayModeHeight() { return 0; }

static void Check(HRESULT hr) {
    if (FAILED(hr)) {
        std::fprintf(stderr, "HRESULT %08lx\n", hr);
        throw std::runtime_error("D3D12 call failed");
    }
}

struct Fixture {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<IDXGISwapChain1> chain;
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12DescriptorHeap> paintHeap, mirrorHeap;
    UINT64 serial{};

    Fixture() {
        ComPtr<IDXGIFactory4> factory;
        ComPtr<IDXGIAdapter> warp;
        Check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
        Check(factory->EnumWarpAdapter(IID_PPV_ARGS(&warp)));
        Check(D3D12CreateDevice(warp.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)));
        D3D12_COMMAND_QUEUE_DESC q{};
        Check(device->CreateCommandQueue(&q, IID_PPV_ARGS(&queue)));
        DXGI_SWAP_CHAIN_DESC1 s{};
        s.Width = 8; s.Height = 4; s.Format = DXGI_FORMAT_R8G8B8A8_UNORM; s.SampleDesc.Count = 1;
        s.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; s.BufferCount = 2; s.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
        Check(factory->CreateSwapChainForComposition(queue.Get(), &s, nullptr, &chain));
        Check(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)));
        Check(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&list)));
        Check(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)));
        D3D12_DESCRIPTOR_HEAP_DESC heap{};
        heap.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV; heap.NumDescriptors = 1;
        Check(device->CreateDescriptorHeap(&heap, IID_PPV_ARGS(&paintHeap)));
        DesktopMirrorSetDeviceAndQueue(device.Get(), queue.Get());
    }
    ~Fixture() { DesktopMirrorSetDeviceAndQueue(nullptr, nullptr); }

    void Flush() {
        Check(list->Close());
        ID3D12CommandList* lists[]{list.Get()};
        queue->ExecuteCommandLists(1, lists);
        Check(queue->Signal(fence.Get(), ++serial));
        Check(fence->SetEventOnCompletion(serial, nullptr));
        Check(device->GetDeviceRemovedReason());
        Check(allocator->Reset());
        Check(list->Reset(allocator.Get(), nullptr));
    }

    void Transition(ID3D12Resource* res, D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
        D3D12_RESOURCE_BARRIER barrier{};
        barrier.Transition = {res, 0, before, after};
        list->ResourceBarrier(1, &barrier);
    }

    void Run(MonitorOutput output, UINT width, bool separate, UINT capturedWidth = 0,
             UINT mainEye = 0, bool hasSecond = true) {
        constexpr UINT height = 4;
        ComPtr<ID3D12Resource> back;
        Check(chain->GetBuffer(0, IID_PPV_ARGS(&back)));
        const auto desc = back->GetDesc();
        const float red[]{1,0,0,1}, green[]{0,1,0,1}, blue[]{0,0,1,1};
        const auto rtv = paintHeap->GetCPUDescriptorHandleForHeapStart();
        device->CreateRenderTargetView(back.Get(), nullptr, rtv);
        Transition(back.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET);
        list->ClearRenderTargetView(rtv, red, 0, nullptr);
        const D3D12_RECT right{LONG(width - width / 2), 0, LONG(width), height};
        list->ClearRenderTargetView(rtv, green, 1, &right);
        Transition(back.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_PRESENT);
        Flush();

        if (separate) {
            auto eyeDesc = desc;
            eyeDesc.Width = capturedWidth;
            eyeDesc.Format = DXGI_FORMAT_R8G8B8A8_TYPELESS;
            D3D12_HEAP_PROPERTIES heap{};
            heap.Type = D3D12_HEAP_TYPE_DEFAULT;
            D3D12_RENDER_TARGET_VIEW_DESC view{};
            view.Format = desc.Format; view.ViewDimension = D3D12_RTV_DIMENSION_TEXTURE2D;
            auto makeEye = [&](const float* color) {
                ComPtr<ID3D12Resource> eye;
                Check(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &eyeDesc,
                    D3D12_RESOURCE_STATE_RENDER_TARGET, nullptr, IID_PPV_ARGS(&eye)));
                device->CreateRenderTargetView(eye.Get(), &view, rtv);
                list->ClearRenderTargetView(rtv, color, 0, nullptr);
                Transition(eye.Get(), D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATE_COPY_SOURCE);
                return eye;
            };
            auto main = makeEye(red);
            ComPtr<ID3D12Resource> second;
            if (hasSecond) second = makeEye(blue);
            if (!DesktopMirrorRecordCapturedEye(device.Get(), mirrorHeap, list.Get(), back.Get(),
                    output, mainEye, main.Get(), second.Get(), capturedWidth != width))
                throw std::runtime_error("captured eye rejected");
            Flush();
        } else {
            DesktopMirrorPackedEye(chain.Get(), output);
        }

        D3D12_PLACED_SUBRESOURCE_FOOTPRINT layout{};
        UINT64 bytes{};
        device->GetCopyableFootprints(&desc, 0, 1, 0, &layout, nullptr, nullptr, &bytes);
        D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
        D3D12_RESOURCE_DESC buffer{}; buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        buffer.Width = bytes; buffer.Height = buffer.MipLevels = buffer.DepthOrArraySize = buffer.SampleDesc.Count = 1;
        buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        ComPtr<ID3D12Resource> readback;
        Check(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&readback)));
        D3D12_TEXTURE_COPY_LOCATION to{readback.Get(), D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT};
        to.PlacedFootprint = layout;
        D3D12_TEXTURE_COPY_LOCATION from{back.Get(), D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
        Transition(back.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
        list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        Transition(back.Get(), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
        Flush();
        void* mapped{};
        Check(readback->Map(0, nullptr, &mapped));
        const UINT eyeWidth = separate ? capturedWidth : width / 2;
        const UINT start = (width - eyeWidth) / 2;
        for (UINT y = 0; y < height; ++y) {
            const auto* row = reinterpret_cast<const uint32_t*>(static_cast<char*>(mapped) + y * layout.Footprint.RowPitch);
            for (UINT x = 0; x < width; ++x) {
                uint32_t expected = 0xff000000;
                if (output == MonitorOutput::Default) expected = x < width - width / 2 ? 0xff0000ff : 0xff00ff00;
                else if (x >= start && x < start + eyeWidth) {
                    if (separate) {
                        const UINT selected = output == MonitorOutput::Right ? 1 : 0;
                        expected = selected != mainEye && hasSecond ? 0xffff0000 : 0xff0000ff;
                    } else expected = output == MonitorOutput::Left ? 0xff0000ff : 0xff00ff00;
                }
                if (row[x] != expected) {
                    std::fprintf(stderr, "width %u mode %d at (%u,%u): %08x expected %08x\n",
                                 width, int(output), x, y, row[x], expected);
                    throw std::runtime_error("mirror pixel mismatch");
                }
            }
        }
        readback->Unmap(0, nullptr);
    }
};

int main() try {
    static_assert(cvr::ParseMonitorOutput(-1) == MonitorOutput::Default);
    static_assert(cvr::ParseMonitorOutput(3) == MonitorOutput::Default);
    Fixture fixture;
    if (DesktopMirrorRecordCapturedEye(nullptr, fixture.mirrorHeap, nullptr, nullptr,
            MonitorOutput::Default, 0, nullptr, nullptr, true) || fixture.mirrorHeap)
        throw std::runtime_error("default attempted capture mirror work");
    for (auto mode : {MonitorOutput::Default, MonitorOutput::Left, MonitorOutput::Right})
        fixture.Run(mode, 8, false);
    fixture.Run(MonitorOutput::Right, 8, true, 8);
    if (fixture.mirrorHeap) throw std::runtime_error("same-size copy allocated a clear heap");
    fixture.Run(MonitorOutput::Right, 8, true, 4);
    fixture.Run(MonitorOutput::Left, 8, true, 4, 1);
    fixture.Run(MonitorOutput::Right, 8, true, 4, 1);
    fixture.Run(MonitorOutput::Right, 8, true, 8, 0, false);
    DesktopMirrorInvalidate();
    Check(fixture.chain->ResizeBuffers(2, 9, 4, DXGI_FORMAT_UNKNOWN, 0));
    fixture.Run(MonitorOutput::Right, 9, false);
    std::puts("PASS default, both packed eyes, typeless captured eye, pillarboxing and odd-width resize");
    return 0;
} catch (const std::exception& error) {
    std::fprintf(stderr, "%s\n", error.what());
    return 1;
}
