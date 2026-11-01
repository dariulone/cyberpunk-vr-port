#include "Render/CommandResources.hpp"
#include "Render/IdleDlssContext.hpp"
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <atomic>
#include <iostream>
#include <stdexcept>

using Microsoft::WRL::ComPtr;
using namespace cvr::gpu;
void Check(bool value, const char* text) { if (!value) throw std::runtime_error(text); }
void Hr(HRESULT value) { Check(SUCCEEDED(value), "D3D12 call failed"); }
std::atomic<unsigned> destroyed{};
struct Object final : IUnknown {
    std::atomic<ULONG> refs{1};
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id, void** out) override {
        if (!out) return E_POINTER; *out = nullptr;
        if (id != __uuidof(IUnknown)) return E_NOINTERFACE;
        *out = this; AddRef(); return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
    ULONG STDMETHODCALLTYPE Release() override { auto n = --refs; if (!n) { ++destroyed; delete this; } return n; }
};

int main() try {
    ComPtr<IDXGIFactory4> factory; Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter; Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device; Hr(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)));
    D3D12_COMMAND_QUEUE_DESC desc{}; desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> queue; Hr(device->CreateCommandQueue(&desc, IID_PPV_ARGS(&queue)));
    std::vector<ComPtr<ID3D12CommandAllocator>> allocators;
    auto allocator = [&] { ComPtr<ID3D12CommandAllocator> a; Hr(device->CreateCommandAllocator(desc.Type, IID_PPV_ARGS(&a))); allocators.push_back(a); return a.Get(); };
    ComPtr<ID3D12GraphicsCommandList> list; Hr(device->CreateCommandList(0, desc.Type, allocator(), nullptr, IID_PPV_ARGS(&list)));
    auto reset = [&] { Hr(list->Reset(allocator(), nullptr)); ResetCommandResources(list.Get()); };
    ID3D12CommandList* commands[]{list.Get()};
    auto keep = [&] { auto* object = new Object; Check(KeepCommandResources(list.Get(), {object}), "record failed"); object->Release(); };
    ComPtr<ID3D12Fence> completion; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&completion)));
    uint64_t completionValue{};
    auto wait = [&] {
        Hr(queue->Signal(completion.Get(), ++completionValue));
        const auto event = CreateEventW(nullptr, FALSE, FALSE, nullptr); Check(event != nullptr, "event");
        Hr(completion->SetEventOnCompletion(completionValue, event));
        const auto result = WaitForSingleObject(event, 5000); CloseHandle(event); Check(result == WAIT_OBJECT_0, "GPU timeout");
        CollectCommandResources();
    };
    Check(PrepareCommandResources(1, commands).batches.empty(), "unrelated list tracked");

    keep(); Hr(list->Close()); reset();
    Check(destroyed == 1, "unsubmitted recording retained after reset");

    // A resize drops the producer's reference and resets its command list while
    // the old GPU copy is deliberately blocked behind a real queue fence.
    ComPtr<ID3D12Fence> gate; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&gate)));
    Hr(queue->Wait(gate.Get(), 1));
    keep(); Hr(list->Close()); auto first = PrepareCommandResources(1, commands);
    queue->ExecuteCommandLists(1, commands);
    reset(); // producer can reset immediately after Execute returns, before our post-hook
    Check(destroyed == 1, "prepare/submit reset race released old resource");
    SubmitCommandResources(queue.Get(), std::move(first));
    keep(); Hr(list->Close()); auto second = PrepareCommandResources(1, commands);
    queue->ExecuteCommandLists(1, commands); SubmitCommandResources(queue.Get(), std::move(second)); reset();
    CollectCommandResources(); Check(destroyed == 1, "pending GPU resources released on resize");
    Hr(gate->Signal(1)); wait(); Check(destroyed == 3, "completed GPU resources leaked");

    // A closed list may be submitted more than once without being re-recorded.
    keep(); Hr(list->Close());
    for (unsigned i = 0; i < 2; ++i) {
        auto replay = PrepareCommandResources(1, commands); Check(!replay.batches.empty(), "replay lost ownership");
        queue->ExecuteCommandLists(1, commands); SubmitCommandResources(queue.Get(), std::move(replay)); wait();
        Check(destroyed == 3, "recording no longer owns replay operands");
    }
    reset(); Check(destroyed == 4, "replayed recording leaked after reset");
    keep(); list.Reset(); CollectCommandResources(); Check(destroyed == 5, "list destruction leaked unsubmitted recording");

    // Completion on the writer queue must not retire a still-pending reader on
    // another queue, even if both reference the same resized depth texture.
    ComPtr<ID3D12CommandQueue> reader; Hr(device->CreateCommandQueue(&desc, IID_PPV_ARGS(&reader)));
    ComPtr<ID3D12GraphicsCommandList> writeList, readList;
    Hr(device->CreateCommandList(0, desc.Type, allocator(), nullptr, IID_PPV_ARGS(&writeList)));
    Hr(device->CreateCommandList(0, desc.Type, allocator(), nullptr, IID_PPV_ARGS(&readList)));
    auto* shared = new Object;
    Check(KeepCommandResources(writeList.Get(), {shared}) && KeepCommandResources(readList.Get(), {shared}), "shared capture failed");
    shared->Release(); Hr(writeList->Close()); Hr(readList->Close());
    ID3D12CommandList* writes[]{writeList.Get()}, *reads[]{readList.Get()};
    ComPtr<ID3D12Fence> readerGate; Hr(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&readerGate)));
    Hr(reader->Wait(readerGate.Get(), 1));
    auto writerBatch = PrepareCommandResources(1, writes), readerBatch = PrepareCommandResources(1, reads);
    queue->ExecuteCommandLists(1, writes); SubmitCommandResources(queue.Get(), std::move(writerBatch));
    reader->ExecuteCommandLists(1, reads); SubmitCommandResources(reader.Get(), std::move(readerBatch));
    writeList.Reset(); readList.Reset(); wait();
    Check(destroyed == 5, "writer completion retired pending reader");
    Hr(readerGate->Signal(1)); Hr(reader->Signal(completion.Get(), ++completionValue));
    const auto done = CreateEventW(nullptr, FALSE, FALSE, nullptr); Check(done != nullptr, "reader event");
    Hr(completion->SetEventOnCompletion(completionValue, done));
    const auto completed = WaitForSingleObject(done, 5000); CloseHandle(done); Check(completed == WAIT_OBJECT_0, "reader timeout");
    CollectCommandResources(); Check(destroyed == 6, "reader resources leaked");

    // A settings/size change can replace more than eight right-eye DLSS
    // operands before the GPU catches up. Neither a fixed retirement array
    // nor CPU evaluate counts can safely govern their lifetime.
    Hr(device->CreateCommandList(0, desc.Type, allocator(), nullptr, IID_PPV_ARGS(&list)));
    Hr(queue->Wait(gate.Get(), 2));
    for (unsigned i = 0; i < 32; ++i) {
        keep();
        Hr(list->Close());
        ID3D12CommandList* batchLists[]{list.Get()};
        auto batch = PrepareCommandResources(1, batchLists);
        queue->ExecuteCommandLists(1, batchLists);
        SubmitCommandResources(queue.Get(), std::move(batch));
        reset();
    }
    CollectCommandResources(); Check(destroyed == 6, "replacement burst retired before GPU completion");
    Hr(gate->Signal(2)); wait();
    Check(destroyed == 38, "replacement burst leaked after GPU completion");

    IdleDlssContext context;
    Check(!context.Ready(10000), "unused context eligible");
    context.Used(list.Get(),100);
    context.SplitSucceeded();context.SplitSucceeded();
    Check(!context.Ready(10000), "recorded context retired before submission");
    Hr(list->Close());ID3D12CommandList* contextLists[]{list.Get()};
    Hr(queue->Wait(gate.Get(),3));
    auto contextBatch=PrepareCommandResources(1,contextLists);
    queue->ExecuteCommandLists(1,contextLists);SubmitCommandResources(queue.Get(),std::move(contextBatch));
    Check(!context.Ready(10000), "blocked GPU allowed context release");
    Hr(gate->Signal(3));wait();
    Check(!context.Ready(10000), "replayable recording allowed context release");
    reset();
    Check(!context.Ready(101), "idle grace ignored");
    Check(context.Ready(10000), "completed reset context cannot retire");
    context.Released();Check(!context.Ready(10000), "context retired twice");
    context.Used(list.Get(),11000);context.SplitSucceeded();context.SplitSucceeded();
    Hr(list->Close());reset();
    Check(context.Ready(14000), "recreated context cannot retire after cancellation");
    context.Used(nullptr,15000);context.SplitSucceeded();context.SplitSucceeded();
    Check(!context.Ready(18000), "failed tracking permitted context release");

    std::cout << "PASS reset cancellation, GPU-delayed resize, prepare/reset race, replay, list destruction, two queues, 32 replacements, and idle DLSS context GPU/replay guards\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
