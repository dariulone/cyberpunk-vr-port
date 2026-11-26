#include "Framegen/GpuTimer.hpp"
#include "Framegen/Framegen.hpp"
#include <wrl/client.h>
#include <array>
#include <memory>
#include <mutex>

#if CVR_RUST_VRIK
// THE TIMER IS RUST'S (rust/crates/cvr-gpu/src/frame_timer.rs); framegen's metrics switch and its frame sink are
// handed over as callbacks.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's submission table calls the timer itself (ecl_table.rs)
extern "C" {
void cvr_gpu_frame_timer_before(ID3D12CommandQueue* queue, bool (*metrics)(), void (*onFrame)(double));
}
namespace cvr::framegen {
namespace {
bool Metrics() { return MetricsEnabled(); }
void Frame(double ms) { OnGpuFrame(ms); }
}  // namespace
void BeforeGameCommands(ID3D12CommandQueue* queue) { cvr_gpu_frame_timer_before(queue, &Metrics, &Frame); }
}  // namespace cvr::framegen
#endif
#else
namespace cvr::framegen {
namespace {using Microsoft::WRL::ComPtr;
thread_local bool inside=false;
std::mutex mutex;
struct Timer {
    struct Slot {ComPtr<ID3D12CommandAllocator> beginAllocator,endAllocator;ComPtr<ID3D12GraphicsCommandList> begin,end;uint64_t value{};bool pending{};};
    ComPtr<ID3D12CommandQueue> queue;ComPtr<ID3D12Fence> fence;
    ComPtr<ID3D12QueryHeap> heap;ComPtr<ID3D12Resource> readback;
    std::array<Slot,8> slots;
    uint64_t value{},frequency{};unsigned cursor{};bool opened{},stopped{};
    bool Create(ID3D12CommandQueue* q) {
        queue=q;ComPtr<ID3D12Device> device;if(FAILED(q->GetDevice(IID_PPV_ARGS(&device))))return false;
        ConfigureHardwareTelemetry(device.Get());
        D3D12_QUERY_HEAP_DESC query{};query.Count=16;query.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
        D3D12_RESOURCE_DESC read{};read.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;read.Width=128;read.Height=1;read.DepthOrArraySize=1;
        read.MipLevels=1;read.SampleDesc.Count=1;read.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES properties{};properties.Type=D3D12_HEAP_TYPE_READBACK;
        if(FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))) ||
           FAILED(device->CreateQueryHeap(&query,IID_PPV_ARGS(&heap))) ||
           FAILED(device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&read,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback))) ||
           FAILED(q->GetTimestampFrequency(&frequency)))return false;
        for(auto& slot:slots) {
            if(FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&slot.beginAllocator))) ||
               FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&slot.endAllocator))) ||
               FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,slot.beginAllocator.Get(),nullptr,IID_PPV_ARGS(&slot.begin))) ||
               FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,slot.endAllocator.Get(),nullptr,IID_PPV_ARGS(&slot.end))))return false;
            slot.begin->Close();slot.end->Close();
        }
        return true;
    }
    void Poll() {
        const auto complete=fence->GetCompletedValue();
        for(unsigned i=0;i<slots.size();++i)if(slots[i].pending && complete>=slots[i].value) {
            void* data{};D3D12_RANGE range{i*16,i*16+16};
            if(SUCCEEDED(readback->Map(0,&range,&data))) {
                auto* ticks=reinterpret_cast<const uint64_t*>(static_cast<const uint8_t*>(data)+i*16);
                if(ticks[1]>=ticks[0])OnGpuFrame(double(ticks[1]-ticks[0])*1000.0/double(frequency));
                D3D12_RANGE none{};readback->Unmap(0,&none);
            }
            slots[i].pending=false;
        }
    }
    void Begin() {
        Poll();if(opened)return;auto& slot=slots[cursor];if(slot.pending)return;
        if(FAILED(slot.beginAllocator->Reset()) || FAILED(slot.begin->Reset(slot.beginAllocator.Get(),nullptr)))return;
        slot.begin->EndQuery(heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*2);
        if(FAILED(slot.begin->Close()))return;
        ID3D12CommandList* lists[]={slot.begin.Get()};queue->ExecuteCommandLists(1,lists);opened=true;
    }
    void End() {
        if(!opened) {Poll();return;}
        auto& slot=slots[cursor];
        if(FAILED(slot.endAllocator->Reset()) || FAILED(slot.end->Reset(slot.endAllocator.Get(),nullptr)))return;
        slot.end->EndQuery(heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*2+1);
        slot.end->ResolveQueryData(heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,cursor*2,2,readback.Get(),cursor*16);
        if(FAILED(slot.end->Close()))return;
        ID3D12CommandList* lists[]={slot.end.Get()};queue->ExecuteCommandLists(1,lists);
        slot.value=++value;queue->Signal(fence.Get(),value);slot.pending=true;opened=false;cursor=(cursor+1)%slots.size();Poll();
    }
    void Stop() {
        if(stopped)return;stopped=true;
        // Guard a begin marker that has no end marker yet. No timestamp
        // resolve/map is needed when the consumer has disabled statistics.
        if(opened) {queue->Signal(fence.Get(),++value);opened=false;}
    }
};
std::unique_ptr<Timer> timer;
std::atomic<bool> timerExists{false};
}
void BeforeGameCommands(ID3D12CommandQueue* queue) {
    if(inside || !MetricsEnabled())return;
    std::lock_guard lock(mutex);if(!MetricsEnabled())return;inside=true;
    if(timer && timer->stopped) {
        if(timer->fence->GetCompletedValue()<timer->value) {inside=false;return;}
        timer.reset();timerExists.store(false,std::memory_order_relaxed);
    }
    if(!timer) {auto candidate=std::make_unique<Timer>();if(candidate->Create(queue)) {timer=std::move(candidate);timerExists.store(true,std::memory_order_relaxed);}}
    if(timer && timer->queue.Get()==queue)timer->Begin();
    inside=false;
}
void FinishGpuFrame() {
    if(inside || !timerExists.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);if(!timer)return;
    inside=true;
    if(MetricsEnabled() && !timer->stopped)timer->End();else timer->Stop();
    if(timer->stopped && timer->fence->GetCompletedValue()>=timer->value) {timer.reset();timerExists.store(false,std::memory_order_relaxed);}
    inside=false;
}
}
#endif
