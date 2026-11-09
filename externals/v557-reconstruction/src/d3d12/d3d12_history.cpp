#include "xrfg/d3d12_history.hpp"
#include "xrfg/bridge_flight_logger.hpp"

#include <windows.h>
#include <wrl/client.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cstring>
#include <limits>
#include <new>
#include <utility>
#include <vector>

namespace xrfg {
namespace {

using Microsoft::WRL::ComPtr;

void dred_marker(ID3D12GraphicsCommandList* command_list, const char* label) noexcept {
    if (command_list != nullptr && label != nullptr &&
        bridge_flight_logger().enabled()) {
        command_list->SetMarker(1U, label, static_cast<UINT>(std::strlen(label)));
    }
}

[[nodiscard]] bool matching_description(
    const D3D12_RESOURCE_DESC& left,
    const D3D12_RESOURCE_DESC& right) noexcept {
    return left.Dimension == right.Dimension &&
           left.Alignment == right.Alignment &&
           left.Width == right.Width &&
           left.Height == right.Height &&
           left.DepthOrArraySize == right.DepthOrArraySize &&
           left.MipLevels == right.MipLevels &&
           left.Format == right.Format &&
           left.SampleDesc.Count == right.SampleDesc.Count &&
           left.SampleDesc.Quality == right.SampleDesc.Quality &&
           left.Layout == right.Layout &&
           left.Flags == right.Flags;
}

[[nodiscard]] bool same_device(
    ID3D12Device* expected,
    ID3D12DeviceChild* object) noexcept {
    if (expected == nullptr || object == nullptr) {
        return false;
    }

    ComPtr<ID3D12Device> actual;
    if (FAILED(object->GetDevice(IID_PPV_ARGS(actual.GetAddressOf())))) {
        return false;
    }

    ComPtr<IUnknown> expected_identity;
    ComPtr<IUnknown> actual_identity;
    return SUCCEEDED(expected->QueryInterface(IID_PPV_ARGS(expected_identity.GetAddressOf()))) &&
           SUCCEEDED(actual->QueryInterface(IID_PPV_ARGS(actual_identity.GetAddressOf()))) &&
           expected_identity.Get() == actual_identity.Get();
}

[[nodiscard]] bool same_adapter(
    ID3D12Device* expected,
    ID3D12DeviceChild* object) noexcept {
    if (expected == nullptr || object == nullptr) {
        return false;
    }
    ComPtr<ID3D12Device> actual;
    if (FAILED(object->GetDevice(IID_PPV_ARGS(actual.GetAddressOf())))) {
        return false;
    }
    const LUID expected_luid = expected->GetAdapterLuid();
    const LUID actual_luid = actual->GetAdapterLuid();
    return expected_luid.HighPart == actual_luid.HighPart &&
           expected_luid.LowPart == actual_luid.LowPart;
}

[[nodiscard]] D3D12_RESOURCE_BARRIER transition_barrier(
    ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before,
    D3D12_RESOURCE_STATES after) noexcept {
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Flags = D3D12_RESOURCE_BARRIER_FLAG_NONE;
    barrier.Transition.pResource = resource;
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before;
    barrier.Transition.StateAfter = after;
    return barrier;
}

}  // namespace

struct D3D12SwapchainHistory::Impl {
    struct HistorySlot {
        ComPtr<ID3D12Resource> resource;
        ComPtr<ID3D12Fence> borrowed_producer;
        std::shared_ptr<void> owner;
        bool retired{};
        ComPtr<ID3D12CommandAllocator> capture_allocator;
        ComPtr<ID3D12GraphicsCommandList> capture_list;
        D3D12HistoryCaptureTicket ticket{};
        std::uint64_t capture_fence_value{};
        std::uint64_t active_lease_serial{};
        ComPtr<ID3D12Fence> consumer_fence;
        std::uint64_t consumer_fence_value{};
        bool valid{};
        bool timing_pending{};
        std::uint64_t timing_serial{};

    };

    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    std::vector<ComPtr<ID3D12Resource>> source_images;
    ComPtr<ID3D12Fence> producer_fence;
    ComPtr<ID3D12QueryHeap> capture_timestamps;
    ComPtr<ID3D12Resource> capture_readback;
    UINT64 timestamp_frequency{}, calibration_gpu{}, calibration_cpu{};
    LARGE_INTEGER qpc_frequency{};
    bool timing_attempted{};
    HANDLE fence_event{};
    // A busy slot's bounded wait has its own event: a wait that times out
    // leaves its completion registered, and the stale signal must not wake
    // the unbounded waits on fence_event early. The timer bounds it at the
    // microsecond; a plain timeout would round up to the 15.6 ms tick.
    HANDLE busy_event{};
    HANDLE busy_timer{};
    std::array<HistorySlot, D3D12SwapchainHistory::kSlotCount> slots;
    D3D12_RESOURCE_STATES release_state{};
    std::uint64_t next_serial{1};
    std::uint64_t next_fence_value{1};
    std::uint64_t next_lease_serial{1};
    std::uint64_t last_submitted_fence_value{};
    std::uint32_t next_slot{};
    D3D12HistoryCaptureTicket pending_ticket{};
    bool has_pending_capture{};
    bool capture_enabled{};
    bool completion_unknown{};
    bool borrow_sources{};
    D3D12_RESOURCE_DESC borrowed_description{};

    void release_completed() noexcept {
        if(!borrow_sources)return;
        for(auto& slot:slots)if(slot.retired && !slot.active_lease_serial &&
            fence_status(slot.borrowed_producer.Get(),slot.capture_fence_value)==S_OK &&
            fence_status(slot.consumer_fence.Get(),slot.consumer_fence_value)==S_OK){
            slot.owner.reset();slot.resource.Reset();slot.borrowed_producer.Reset();
            slot.consumer_fence.Reset();slot.consumer_fence_value=0;
            slot.capture_fence_value=0;slot.valid=false;slot.retired=false;slot.ticket={};
        }
    }

    HRESULT capture_borrowed(ID3D12Resource* source,ID3D12Fence* ready,std::uint64_t value,
        std::shared_ptr<void> owner,D3D12HistoryCaptureTicket* ticket) noexcept {
        if(ticket)*ticket={};
        if(!ticket || !borrow_sources || !capture_enabled || has_pending_capture ||
           !source || !ready || !value || !owner ||
           !same_device(device.Get(),source) || !same_device(device.Get(),ready) ||
           !matching_description(borrowed_description,source->GetDesc()) ||
           next_serial==std::numeric_limits<std::uint64_t>::max())return E_INVALIDARG;
        release_completed();
        auto& slot=slots[next_slot];
        if(slot.owner || slot.active_lease_serial || slot.valid)return HRESULT_FROM_WIN32(ERROR_BUSY);
        // One mutable pool image cannot be both old A and new B. The owner's
        // producer pin is required even though this object also AddRefs it.
        for(const auto& old:slots)if(old.resource.Get()==source)return HRESULT_FROM_WIN32(ERROR_BUSY);
        slot.resource=source;slot.borrowed_producer=ready;slot.owner=std::move(owner);
        slot.capture_fence_value=value;slot.retired=false;
        pending_ticket={next_serial,value,next_slot,0};has_pending_capture=true;
        *ticket=pending_ticket;return S_OK;
    }

    // One query per history slot, consumed only after the EXISTING capture
    // fence completes. No added signals, queue waits, threads or pixel readback.
    bool prepare_capture_timing() noexcept {
        if (!bridge_flight_logger().enabled()) return false;
        if (timing_attempted) return capture_timestamps && capture_readback;
        timing_attempted = true;
        if (FAILED(queue->GetTimestampFrequency(&timestamp_frequency)) || !timestamp_frequency ||
            FAILED(queue->GetClockCalibration(&calibration_gpu, &calibration_cpu)) ||
            !QueryPerformanceFrequency(&qpc_frequency) || qpc_frequency.QuadPart <= 0) return false;
        D3D12_QUERY_HEAP_DESC query{};
        query.Type = D3D12_QUERY_HEAP_TYPE_TIMESTAMP;
        query.Count = D3D12SwapchainHistory::kSlotCount;
        query.NodeMask = queue->GetDesc().NodeMask;
        if (FAILED(device->CreateQueryHeap(&query, IID_PPV_ARGS(capture_timestamps.GetAddressOf())))) return false;
        D3D12_HEAP_PROPERTIES heap{};
        heap.Type = D3D12_HEAP_TYPE_READBACK;
        heap.CreationNodeMask = heap.VisibleNodeMask = query.NodeMask ? query.NodeMask : 1;
        D3D12_RESOURCE_DESC buffer{};
        buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
        buffer.Width = sizeof(UINT64) * query.Count;
        buffer.Height = buffer.DepthOrArraySize = buffer.MipLevels = 1;
        buffer.SampleDesc.Count = 1;
        buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(capture_readback.GetAddressOf())))) {
            capture_timestamps.Reset(); return false;
        }
        return true;
    }

    void harvest_capture_timing() noexcept {
        if (!capture_readback) return;
        const auto completed = producer_fence->GetCompletedValue();
        if (completed == std::numeric_limits<UINT64>::max()) return;
        for (UINT i = 0; i < slots.size(); ++i) {
            auto& slot = slots[i];
            if (!slot.timing_pending || completed < slot.capture_fence_value) continue;
            slot.timing_pending = false;
            if (!bridge_flight_logger().enabled()) continue;
            const SIZE_T offset = i * sizeof(UINT64);
            D3D12_RANGE range{offset, offset + sizeof(UINT64)};
            void* data = nullptr;
            if (FAILED(capture_readback->Map(0, &range, &data)) || !data) continue;
            UINT64 timestamp{};
            std::memcpy(&timestamp, static_cast<const char*>(data) + offset, sizeof(timestamp));
            D3D12_RANGE no_write{0, 0};
            capture_readback->Unmap(0, &no_write);
            const long double counter = static_cast<long double>(calibration_cpu) +
                (static_cast<long double>(timestamp) - calibration_gpu) *
                qpc_frequency.QuadPart / timestamp_frequency;
            if (!timestamp || counter <= 0 || counter >= std::numeric_limits<std::int64_t>::max()) continue;
            const auto us = bridge_flight_logger().microseconds_for_counter(static_cast<std::int64_t>(counter));
            if (us < 0) continue;
            bridge_flight_logger().event(BridgeFlightOperation::history_gpu_ready, 0,
                reinterpret_cast<std::uint64_t>(slot.resource.Get()), slot.timing_serial,
                static_cast<std::uint64_t>(us));
        }
    }

    ~Impl() {
        if (fence_event != nullptr) {
            CloseHandle(fence_event);
        }
        if (busy_event != nullptr) {
            CloseHandle(busy_event);
        }
        if (busy_timer != nullptr) {
            CloseHandle(busy_timer);
        }
    }

    [[nodiscard]] static bool matching_ticket(
        const D3D12HistoryCaptureTicket& left,
        const D3D12HistoryCaptureTicket& right) noexcept {
        return left.serial == right.serial &&
               left.fence_value == right.fence_value &&
               left.slot == right.slot &&
               left.source_index == right.source_index;
    }

    [[nodiscard]] static bool matching_lease(
        const D3D12HistoryConsumerLease& lease,
        const HistorySlot& slot,
        std::uint32_t slot_index) noexcept {
        return lease.capture_serial != 0 &&
               lease.lease_serial != 0 &&
               lease.slot == slot_index &&
               slot.valid &&
               slot.ticket.serial == lease.capture_serial &&
               slot.active_lease_serial == lease.lease_serial;
    }

    [[nodiscard]] HRESULT create_closed_command_list(
        UINT node_mask,
        HistorySlot& slot,
        D3D12HistoryInitializationStage* failure_stage) noexcept {
        HRESULT result = device->CreateCommandAllocator(
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            IID_PPV_ARGS(slot.capture_allocator.GetAddressOf()));
        if (FAILED(result)) {
            if (failure_stage != nullptr) {
                *failure_stage =
                    D3D12HistoryInitializationStage::create_slot_allocator;
            }
            return result;
        }
        result = device->CreateCommandList(
            node_mask,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            slot.capture_allocator.Get(),
            nullptr,
            IID_PPV_ARGS(slot.capture_list.GetAddressOf()));
        if (FAILED(result)) {
            if (failure_stage != nullptr) {
                *failure_stage =
                    D3D12HistoryInitializationStage::create_slot_command_list;
            }
            return result;
        }
        // DRED reports this stable name if a device removal stops inside the bridge's
        // history copy, distinguishing it from game, NGX and synthesis work.
        slot.capture_list->SetName(L"OFXR History Capture");
        result = slot.capture_list->Close();
        if (FAILED(result) && failure_stage != nullptr) {
            *failure_stage =
                D3D12HistoryInitializationStage::close_slot_command_list;
        }
        return result;
    }

    [[nodiscard]] HRESULT initialize(
        ID3D12Device* input_device,
        ID3D12CommandQueue* input_queue,
        std::span<ID3D12Resource* const> inputs,
        D3D12_RESOURCE_STATES input_release_state,
        D3D12HistoryInitializationStage* failure_stage, bool borrow) {
        if (failure_stage != nullptr) {
            *failure_stage = D3D12HistoryInitializationStage::complete;
        }
        if (input_device == nullptr || input_queue == nullptr || inputs.empty() ||
            (input_release_state != D3D12_RESOURCE_STATE_RENDER_TARGET &&
             input_release_state != D3D12_RESOURCE_STATE_DEPTH_WRITE &&
             input_release_state != D3D12_RESOURCE_STATE_COMMON &&
             input_release_state != D3D12_RESOURCE_STATE_COPY_SOURCE &&
             input_release_state != D3D12_RESOURCE_STATE_GENERIC_READ) ||
            input_queue->GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT ||
            !same_device(input_device, input_queue)) {
            if (failure_stage != nullptr) {
                *failure_stage = D3D12HistoryInitializationStage::arguments;
            }
            return E_INVALIDARG;
        }

        ID3D12Resource* const first_source = inputs.front();
        if (first_source == nullptr || !same_adapter(input_device, first_source)) {
            if (failure_stage != nullptr) {
                *failure_stage = D3D12HistoryInitializationStage::first_source;
            }
            return E_INVALIDARG;
        }

        const D3D12_RESOURCE_DESC source_description = first_source->GetDesc();
        if (source_description.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
            source_description.Width == 0 || source_description.Height == 0 ||
            source_description.DepthOrArraySize == 0 || source_description.MipLevels == 0 ||
            source_description.SampleDesc.Count == 0) {
            if (failure_stage != nullptr) {
                *failure_stage =
                    D3D12HistoryInitializationStage::source_description;
            }
            return E_INVALIDARG;
        }

        if(borrow && (input_release_state!=D3D12_RESOURCE_STATE_GENERIC_READ ||
           (source_description.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)))return E_INVALIDARG;
        borrow_sources=borrow;borrowed_description=source_description;
        source_images.reserve(inputs.size());
        for (ID3D12Resource* source : inputs) {
            if (source == nullptr || !same_adapter(input_device, source) ||
                !matching_description(source_description, source->GetDesc())) {
                if (failure_stage != nullptr) {
                    *failure_stage =
                        D3D12HistoryInitializationStage::source_validation;
                }
                return E_INVALIDARG;
            }
            ComPtr<ID3D12Resource> retained_source = source;
            source_images.push_back(std::move(retained_source));
        }

        device = input_device;
        queue = input_queue;
        release_state = input_release_state;

        const D3D12_COMMAND_QUEUE_DESC queue_description = queue->GetDesc();
        D3D12_HEAP_PROPERTIES heap_properties{};
        heap_properties.Type = D3D12_HEAP_TYPE_DEFAULT;
        heap_properties.CPUPageProperty = D3D12_CPU_PAGE_PROPERTY_UNKNOWN;
        heap_properties.MemoryPoolPreference = D3D12_MEMORY_POOL_UNKNOWN;
        heap_properties.CreationNodeMask =
            queue_description.NodeMask == 0 ? 1U : queue_description.NodeMask;
        heap_properties.VisibleNodeMask = heap_properties.CreationNodeMask;

        D3D12_RESOURCE_DESC history_description = source_description;
        history_description.Flags = static_cast<D3D12_RESOURCE_FLAGS>(
            history_description.Flags & ~D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE);
        for (HistorySlot& slot : slots) {
            if(borrow_sources)continue;
            HRESULT result = device->CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &history_description,
                D3D12_RESOURCE_STATE_COMMON,
                nullptr,
                IID_PPV_ARGS(slot.resource.GetAddressOf()));
            if (FAILED(result)) {
                if (failure_stage != nullptr) {
                    *failure_stage =
                        D3D12HistoryInitializationStage::create_slot_resource;
                }
                return result;
            }
            result = create_closed_command_list(
                queue_description.NodeMask, slot, failure_stage);
            if (FAILED(result)) {
                return result;
            }
        }

        HRESULT result = device->CreateFence(
            0,
            D3D12_FENCE_FLAG_NONE,
            IID_PPV_ARGS(producer_fence.GetAddressOf()));
        if (FAILED(result)) {
            if (failure_stage != nullptr) {
                *failure_stage = D3D12HistoryInitializationStage::create_fence;
            }
            return result;
        }

        fence_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (fence_event == nullptr) {
            if (failure_stage != nullptr) {
                *failure_stage = D3D12HistoryInitializationStage::create_event;
            }
            const DWORD error = GetLastError();
            return HRESULT_FROM_WIN32(error == ERROR_SUCCESS ? ERROR_NOT_ENOUGH_MEMORY : error);
        }

        capture_enabled = true;
        return S_OK;
    }

    [[nodiscard]] static HRESULT fence_status(
        ID3D12Fence* fence,
        std::uint64_t value) noexcept {
        if (value == 0) {
            return S_OK;
        }
        if (fence == nullptr) {
            return E_UNEXPECTED;
        }
        const std::uint64_t completed = fence->GetCompletedValue();
        if (completed == std::numeric_limits<std::uint64_t>::max()) {
            return E_FAIL;
        }
        return completed >= value ? S_OK : S_FALSE;
    }

    [[nodiscard]] HRESULT wait_for_fence(
        ID3D12Fence* fence,
        std::uint64_t value) noexcept {
        const HRESULT status = fence_status(fence, value);
        if (status != S_FALSE) {
            return status;
        }
        if (fence_event == nullptr) {
            return E_UNEXPECTED;
        }
        const HRESULT event_result = fence->SetEventOnCompletion(value, fence_event);
        if (FAILED(event_result)) {
            return event_result;
        }
        if (WaitForSingleObject(fence_event, INFINITE) != WAIT_OBJECT_0) {
            const DWORD error = GetLastError();
            return HRESULT_FROM_WIN32(error == ERROR_SUCCESS ? ERROR_GEN_FAILURE : error);
        }
        return fence_status(fence, value) == S_OK ? S_OK : E_FAIL;
    }

    // Waits until `fence` reaches `value` or the timer armed by
    // wait_for_busy_slot fires. ERROR_BUSY when the time ran out first.
    [[nodiscard]] HRESULT wait_for_fence_bounded(
        ID3D12Fence* fence,
        std::uint64_t value) noexcept {
        for (;;) {
            const HRESULT status = fence_status(fence, value);
            if (status != S_FALSE) {
                return status;
            }
            const HRESULT event_result = fence->SetEventOnCompletion(value, busy_event);
            if (FAILED(event_result)) {
                return event_result;
            }
            const HANDLE handles[] = {busy_event, busy_timer};
            const DWORD wait_result =
                WaitForMultipleObjects(2, handles, FALSE, INFINITE);
            if (wait_result == WAIT_OBJECT_0 + 1) {
                return fence_status(fence, value) == S_FALSE
                    ? HRESULT_FROM_WIN32(ERROR_BUSY)
                    : fence_status(fence, value);
            }
            if (wait_result != WAIT_OBJECT_0) {
                const DWORD error = GetLastError();
                return HRESULT_FROM_WIN32(error == ERROR_SUCCESS ? ERROR_GEN_FAILURE : error);
            }
            // The event may be a stale registration from an earlier wait that
            // timed out: the loop checks the fence again before trusting it.
        }
    }

    // The bounded wait for a slot whose last reader or writer is still on the
    // GPU: the consumer's completion first, then the slot's own capture.
    // Never for a lease, which is CPU state no fence will clear.
    [[nodiscard]] HRESULT wait_for_busy_slot(
        HistorySlot& slot,
        std::uint32_t limit_us) noexcept {
        if (slot.active_lease_serial != 0 || limit_us == 0) {
            return HRESULT_FROM_WIN32(ERROR_BUSY);
        }
        if (busy_event == nullptr) {
            busy_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
            if (busy_event == nullptr) {
                return HRESULT_FROM_WIN32(ERROR_NOT_ENOUGH_MEMORY);
            }
        }
        if (busy_timer == nullptr) {
            busy_timer = CreateWaitableTimerExW(
                nullptr, nullptr, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS);
            if (busy_timer == nullptr) {
                busy_timer = CreateWaitableTimerExW(nullptr, nullptr, 0, TIMER_ALL_ACCESS);
            }
            if (busy_timer == nullptr) {
                return HRESULT_FROM_WIN32(ERROR_NOT_ENOUGH_MEMORY);
            }
        }
        LARGE_INTEGER due{};
        due.QuadPart = -static_cast<LONGLONG>(limit_us) * 10;
        if (!SetWaitableTimer(busy_timer, &due, 0, nullptr, nullptr, FALSE)) {
            const DWORD error = GetLastError();
            return HRESULT_FROM_WIN32(error == ERROR_SUCCESS ? ERROR_GEN_FAILURE : error);
        }
        HRESULT result = S_OK;
        if (slot.consumer_fence_value != 0) {
            result = wait_for_fence_bounded(
                slot.consumer_fence.Get(), slot.consumer_fence_value);
        }
        if (SUCCEEDED(result)) {
            result = wait_for_fence_bounded(producer_fence.Get(), slot.capture_fence_value);
        }
        static_cast<void>(CancelWaitableTimer(busy_timer));
        return result;
    }

    [[nodiscard]] HRESULT prepare_slot_for_capture(HistorySlot& slot) noexcept {
        if (slot.active_lease_serial != 0) {
            return HRESULT_FROM_WIN32(ERROR_BUSY);
        }
        if (slot.consumer_fence_value != 0) {
            const HRESULT consumer_status =
                fence_status(slot.consumer_fence.Get(), slot.consumer_fence_value);
            if (consumer_status == S_FALSE) {
                return HRESULT_FROM_WIN32(ERROR_BUSY);
            }
            if (FAILED(consumer_status)) {
                return consumer_status;
            }
            slot.consumer_fence.Reset();
            slot.consumer_fence_value = 0;
        }
        const HRESULT producer_status = fence_status(producer_fence.Get(), slot.capture_fence_value);
        if (producer_status == S_FALSE) {
            return HRESULT_FROM_WIN32(ERROR_BUSY);
        }
        return producer_status;
    }

    [[nodiscard]] HRESULT execute_and_signal(
        ID3D12GraphicsCommandList* command_list,
        std::uint64_t* output_value) noexcept {
        if (command_list == nullptr || output_value == nullptr ||
            next_fence_value == 0 ||
            next_fence_value == std::numeric_limits<std::uint64_t>::max()) {
            capture_enabled = false;
            return E_FAIL;
        }

        ID3D12CommandList* command_lists[] = {command_list};
        queue->ExecuteCommandLists(1, command_lists);
        completion_unknown = true;

        const std::uint64_t value = next_fence_value;
        const HRESULT result = queue->Signal(producer_fence.Get(), value);
        if (FAILED(result)) {
            capture_enabled = false;
            return result;
        }

        completion_unknown = false;
        last_submitted_fence_value = value;
        ++next_fence_value;
        *output_value = value;
        return S_OK;
    }

    [[nodiscard]] HRESULT capture(
        std::uint32_t source_index,
        D3D12HistoryCaptureTicket* output_ticket,
        D3D12HistoryCaptureWait* wait) noexcept {
        if(borrow_sources)return E_INVALIDARG;
        if (output_ticket == nullptr) {
            return E_POINTER;
        }
        *output_ticket = {};

        if (!capture_enabled || has_pending_capture) {
            return E_UNEXPECTED;
        }
        if (source_index >= source_images.size()) {
            return E_INVALIDARG;
        }
        if (next_serial == 0 || next_serial == std::numeric_limits<std::uint64_t>::max()) {
            capture_enabled = false;
            return E_FAIL;
        }

        harvest_capture_timing();
        HistorySlot& slot = slots[next_slot];
        HRESULT result = prepare_slot_for_capture(slot);
        if (result == HRESULT_FROM_WIN32(ERROR_BUSY) && wait != nullptr) {
            wait->slot_busy = true;
            const auto entered = std::chrono::steady_clock::now();
            result = wait_for_busy_slot(slot, wait->limit_us);
            wait->waited_us = static_cast<std::uint64_t>(
                std::chrono::duration_cast<std::chrono::microseconds>(
                    std::chrono::steady_clock::now() - entered).count());
            if (SUCCEEDED(result)) {
                result = prepare_slot_for_capture(slot);
            }
        }
        if (FAILED(result)) {
            return result;
        }

        // A bounded busy-slot wait may just have completed this slot.
        harvest_capture_timing();
        slot.valid = false;
        slot.ticket = {};
        result = slot.capture_allocator->Reset();
        if (FAILED(result)) {
            capture_enabled = false;
            return result;
        }
        result = slot.capture_list->Reset(slot.capture_allocator.Get(), nullptr);
        if (FAILED(result)) {
            capture_enabled = false;
            return result;
        }

        dred_marker(slot.capture_list.Get(), "OFXR history begin");

        ID3D12Resource* const source = source_images[source_index].Get();
        const std::array<D3D12_RESOURCE_BARRIER, 2> before_copy{
            transition_barrier(source, release_state, D3D12_RESOURCE_STATE_COPY_SOURCE),
            transition_barrier(
                slot.resource.Get(),
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST),
        };
        slot.capture_list->ResourceBarrier(
            static_cast<UINT>(before_copy.size()) - ((release_state & D3D12_RESOURCE_STATE_COPY_SOURCE) != 0 ? 1U : 0U),
            before_copy.data() + ((release_state & D3D12_RESOURCE_STATE_COPY_SOURCE) != 0 ? 1U : 0U));
        dred_marker(slot.capture_list.Get(), "OFXR history copy");
        slot.capture_list->CopyResource(slot.resource.Get(), source);

        const std::array<D3D12_RESOURCE_BARRIER, 2> after_copy{
            transition_barrier(source, D3D12_RESOURCE_STATE_COPY_SOURCE, release_state),
            transition_barrier(
                slot.resource.Get(),
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COMMON),
        };
        slot.capture_list->ResourceBarrier(
            static_cast<UINT>(after_copy.size()) - ((release_state & D3D12_RESOURCE_STATE_COPY_SOURCE) != 0 ? 1U : 0U),
            after_copy.data() + ((release_state & D3D12_RESOURCE_STATE_COPY_SOURCE) != 0 ? 1U : 0U));
        dred_marker(slot.capture_list.Get(), "OFXR history complete");
        const bool capture_timing = prepare_capture_timing();
        if (capture_timing) {
            slot.capture_list->EndQuery(capture_timestamps.Get(), D3D12_QUERY_TYPE_TIMESTAMP, next_slot);
            slot.capture_list->ResolveQueryData(capture_timestamps.Get(), D3D12_QUERY_TYPE_TIMESTAMP,
                next_slot, 1, capture_readback.Get(), next_slot * sizeof(UINT64));
        }

        result = slot.capture_list->Close();
        if (FAILED(result)) {
            capture_enabled = false;
            return result;
        }

        std::uint64_t fence_value = 0;
        result = execute_and_signal(slot.capture_list.Get(), &fence_value);
        if (FAILED(result)) {
            return result;
        }
        slot.capture_fence_value = fence_value;
        slot.timing_pending = capture_timing;
        slot.timing_serial = next_serial;

        D3D12HistoryCaptureTicket submitted_ticket{};
        submitted_ticket.serial = next_serial;
        submitted_ticket.fence_value = fence_value;
        submitted_ticket.slot = next_slot;
        submitted_ticket.source_index = source_index;
        pending_ticket = submitted_ticket;
        has_pending_capture = true;
        *output_ticket = submitted_ticket;
        return S_OK;
    }

    [[nodiscard]] HRESULT commit(
        const D3D12HistoryCaptureTicket& requested_ticket) noexcept {
        if (!has_pending_capture || !matching_ticket(pending_ticket, requested_ticket) ||
            requested_ticket.slot >= slots.size()) {
            return E_INVALIDARG;
        }

        HistorySlot& slot = slots[requested_ticket.slot];
        slot.ticket = requested_ticket;
        slot.valid = true;
        pending_ticket = {};
        has_pending_capture = false;
        ++next_serial;
        next_slot = (next_slot + 1U) % D3D12SwapchainHistory::kSlotCount;
        return S_OK;
    }

    void discard(const D3D12HistoryCaptureTicket& requested_ticket) noexcept {
        if (!has_pending_capture || !matching_ticket(pending_ticket, requested_ticket) ||
            requested_ticket.slot >= slots.size()) {
            return;
        }
        HistorySlot& slot = slots[requested_ticket.slot];
        slot.ticket = {};
        slot.valid = false;
        if(borrow_sources){slot.retired=true;release_completed();}
        pending_ticket = {};
        has_pending_capture = false;
    }

    [[nodiscard]] HRESULT acquire_consumer(
        const D3D12HistoryCaptureTicket& requested_ticket,
        D3D12HistoryConsumerLease* output_lease,
        ID3D12Resource** output_resource,
        ID3D12Fence** output_producer_fence) noexcept {
        if (output_lease != nullptr) {
            *output_lease = {};
        }
        if (output_resource != nullptr) {
            *output_resource = nullptr;
        }
        if (output_producer_fence != nullptr) {
            *output_producer_fence = nullptr;
        }
        if (output_lease == nullptr || output_resource == nullptr ||
            output_producer_fence == nullptr) {
            return E_POINTER;
        }
        if (requested_ticket.slot >= slots.size() || requested_ticket.serial == 0 ||
            requested_ticket.fence_value == 0 ||
            next_lease_serial == 0 ||
            next_lease_serial == std::numeric_limits<std::uint64_t>::max()) {
            return E_INVALIDARG;
        }

        HistorySlot& slot = slots[requested_ticket.slot];
        if (!slot.valid || !matching_ticket(slot.ticket, requested_ticket) ||
            slot.active_lease_serial != 0 || slot.resource == nullptr || (borrow_sources?slot.borrowed_producer.Get():producer_fence.Get()) == nullptr) {
            return HRESULT_FROM_WIN32(ERROR_BUSY);
        }
        if (slot.consumer_fence_value != 0) {
            const HRESULT consumer_status =
                fence_status(slot.consumer_fence.Get(), slot.consumer_fence_value);
            if (consumer_status == S_FALSE) {
                return HRESULT_FROM_WIN32(ERROR_BUSY);
            }
            if (FAILED(consumer_status)) {
                return consumer_status;
            }
            slot.consumer_fence.Reset();
            slot.consumer_fence_value = 0;
        }

        const std::uint64_t lease_serial = next_lease_serial++;
        slot.active_lease_serial = lease_serial;
        slot.resource->AddRef();
        auto* source_fence=borrow_sources?slot.borrowed_producer.Get():producer_fence.Get();
        source_fence->AddRef();
        *output_resource = slot.resource.Get();
        *output_producer_fence = source_fence;
        output_lease->capture_serial = requested_ticket.serial;
        output_lease->lease_serial = lease_serial;
        output_lease->slot = requested_ticket.slot;
        return S_OK;
    }

    [[nodiscard]] HRESULT retire_consumer(
        const D3D12HistoryConsumerLease& lease,
        ID3D12Fence* completion_fence,
        std::uint64_t completion_value) noexcept {
        if (lease.slot >= slots.size() || completion_fence == nullptr || completion_value == 0 ||
            !same_adapter(device.Get(), completion_fence)) {
            return E_INVALIDARG;
        }
        HistorySlot& slot = slots[lease.slot];
        if (!matching_lease(lease, slot, lease.slot)) {
            return E_INVALIDARG;
        }
        slot.consumer_fence = completion_fence;
        slot.consumer_fence_value = completion_value;
        slot.active_lease_serial = 0;
        if(borrow_sources)slot.retired=true;
        return S_OK;
    }

    void cancel_consumer(const D3D12HistoryConsumerLease& lease) noexcept {
        if (lease.slot >= slots.size()) {
            return;
        }
        HistorySlot& slot = slots[lease.slot];
        if (matching_lease(lease, slot, lease.slot)) {
            slot.active_lease_serial = 0;
            if(borrow_sources)slot.retired=true;
        }
    }

    [[nodiscard]] HRESULT wait_for_idle() noexcept {
        if (completion_unknown) {
            capture_enabled = false;
            return HRESULT_FROM_WIN32(ERROR_IO_INCOMPLETE);
        }
        for (const HistorySlot& slot : slots) {
            if (slot.active_lease_serial != 0) {
                return HRESULT_FROM_WIN32(ERROR_BUSY);
            }
        }

        HRESULT result = wait_for_fence(producer_fence.Get(), last_submitted_fence_value);
        if (FAILED(result)) {
            capture_enabled = false;
            return result;
        }
        for (const HistorySlot& slot : slots) {
            if(borrow_sources){result=wait_for_fence(slot.borrowed_producer.Get(),slot.capture_fence_value);if(FAILED(result))return result;}
            result = wait_for_fence(slot.consumer_fence.Get(), slot.consumer_fence_value);
            if (FAILED(result)) {
                capture_enabled = false;
                return result;
            }
        }
        release_completed();
        return S_OK;
    }

    [[nodiscard]] HRESULT invalidate() noexcept {
        const HRESULT result = wait_for_idle();
        if (FAILED(result)) {
            return result;
        }
        for (HistorySlot& slot : slots) {
            if(borrow_sources){slot.owner.reset();slot.resource.Reset();slot.borrowed_producer.Reset();slot.retired=false;}
            slot.ticket = {};
            slot.valid = false;
            slot.capture_fence_value = 0;
            slot.active_lease_serial = 0;
            slot.consumer_fence.Reset();
            slot.consumer_fence_value = 0;
        }
        pending_ticket = {};
        has_pending_capture = false;
        next_slot = 0;
        return S_OK;
    }
};

D3D12SwapchainHistory::D3D12SwapchainHistory() noexcept = default;

D3D12SwapchainHistory::~D3D12SwapchainHistory() {
    try {
        std::scoped_lock lock(mutex_);
        if (impl_ != nullptr && FAILED(impl_->wait_for_idle())) {
            // A producer command or consumer lease whose completion cannot be
            // proven must retain the entire COM graph. Releasing the private
            // slot or its allocator here could race the GPU.
            static_cast<void>(impl_.release());
        }
    } catch (...) {
        // Destruction must not let synchronization failures escape.
    }
}

HRESULT D3D12SwapchainHistory::initialize(
    ID3D12Device* device,
    ID3D12CommandQueue* queue,
    std::span<ID3D12Resource* const> source_images,
    D3D12_RESOURCE_STATES release_state,
    D3D12HistoryInitializationStage* failure_stage, bool borrow_sources) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        if (impl_ != nullptr) {
            return HRESULT_FROM_WIN32(ERROR_ALREADY_INITIALIZED);
        }
        auto candidate = std::make_unique<Impl>();
        const HRESULT result = candidate->initialize(
            device, queue, source_images, release_state, failure_stage, borrow_sources);
        if (FAILED(result)) {
            return result;
        }
        impl_ = std::move(candidate);
        return S_OK;
    } catch (const std::bad_alloc&) {
        return E_OUTOFMEMORY;
    } catch (...) {
        return E_FAIL;
    }
}

HRESULT D3D12SwapchainHistory::capture_borrowed(ID3D12Resource* source,ID3D12Fence* ready,
    std::uint64_t value,std::shared_ptr<void> owner,D3D12HistoryCaptureTicket* ticket) noexcept {
    std::scoped_lock lock(mutex_);
    if(!impl_)return E_UNEXPECTED;
    return impl_->capture_borrowed(source,ready,value,std::move(owner),ticket);
}
void D3D12SwapchainHistory::release_completed() noexcept {
    std::scoped_lock lock(mutex_);if(impl_)impl_->release_completed();
}
bool D3D12SwapchainHistory::borrowed() const noexcept {
    std::scoped_lock lock(mutex_);return impl_ && impl_->borrow_sources;
}

HRESULT D3D12SwapchainHistory::capture_source(ID3D12Resource* source, D3D12HistoryCaptureTicket* ticket) noexcept {
    std::scoped_lock lock(mutex_);
    if (ticket) *ticket = {};
    if (!impl_ || !source || impl_->source_images.empty() ||
        !same_device(impl_->device.Get(), source) ||
        !matching_description(impl_->source_images[0]->GetDesc(), source->GetDesc())) return E_INVALIDARG;
    if (impl_->producer_fence->GetCompletedValue() < impl_->last_submitted_fence_value) return HRESULT_FROM_WIN32(ERROR_BUSY);
    impl_->source_images[0] = source;
    return impl_->capture(0, ticket, nullptr);
}

HRESULT D3D12SwapchainHistory::capture(
    std::uint32_t source_index,
    D3D12HistoryCaptureTicket* ticket,
    D3D12HistoryCaptureWait* wait) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        if (impl_ == nullptr) {
            if (ticket != nullptr) {
                *ticket = {};
            }
            return E_UNEXPECTED;
        }
        return impl_->capture(source_index, ticket, wait);
    } catch (const std::bad_alloc&) {
        if (ticket != nullptr) {
            *ticket = {};
        }
        return E_OUTOFMEMORY;
    } catch (...) {
        if (ticket != nullptr) {
            *ticket = {};
        }
        return E_FAIL;
    }
}

HRESULT D3D12SwapchainHistory::commit(
    const D3D12HistoryCaptureTicket& ticket) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        return impl_ == nullptr ? E_UNEXPECTED : impl_->commit(ticket);
    } catch (...) {
        return E_FAIL;
    }
}

void D3D12SwapchainHistory::discard(
    const D3D12HistoryCaptureTicket& ticket) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        if (impl_ != nullptr) {
            impl_->discard(ticket);
        }
    } catch (...) {
        // Discard is a fail-safe cleanup path.
    }
}

HRESULT D3D12SwapchainHistory::acquire_consumer(
    const D3D12HistoryCaptureTicket& ticket,
    D3D12HistoryConsumerLease* lease,
    ID3D12Resource** resource,
    ID3D12Fence** producer_fence) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        if (impl_ == nullptr) {
            if (lease != nullptr) {
                *lease = {};
            }
            if (resource != nullptr) {
                *resource = nullptr;
            }
            if (producer_fence != nullptr) {
                *producer_fence = nullptr;
            }
            return E_UNEXPECTED;
        }
        return impl_->acquire_consumer(ticket, lease, resource, producer_fence);
    } catch (...) {
        if (lease != nullptr) {
            *lease = {};
        }
        if (resource != nullptr) {
            *resource = nullptr;
        }
        if (producer_fence != nullptr) {
            *producer_fence = nullptr;
        }
        return E_FAIL;
    }
}

HRESULT D3D12SwapchainHistory::retire_consumer(
    const D3D12HistoryConsumerLease& lease,
    ID3D12Fence* completion_fence,
    std::uint64_t completion_value) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        return impl_ == nullptr
                   ? E_UNEXPECTED
                   : impl_->retire_consumer(lease, completion_fence, completion_value);
    } catch (...) {
        return E_FAIL;
    }
}

void D3D12SwapchainHistory::cancel_consumer(
    const D3D12HistoryConsumerLease& lease) noexcept {
    try {
        std::scoped_lock lock(mutex_);
        if (impl_ != nullptr) {
            impl_->cancel_consumer(lease);
        }
    } catch (...) {
        // Cancellation is valid only before GPU submission and must not throw.
    }
}

HRESULT D3D12SwapchainHistory::wait_for_idle() noexcept {
    try {
        std::scoped_lock lock(mutex_);
        return impl_ == nullptr ? S_OK : impl_->wait_for_idle();
    } catch (...) {
        return E_FAIL;
    }
}

HRESULT D3D12SwapchainHistory::invalidate() noexcept {
    try {
        std::scoped_lock lock(mutex_);
        return impl_ == nullptr ? S_OK : impl_->invalidate();
    } catch (...) {
        return E_FAIL;
    }
}

bool D3D12SwapchainHistory::initialized() const noexcept {
    try {
        std::scoped_lock lock(mutex_);
        return impl_ != nullptr && impl_->capture_enabled;
    } catch (...) {
        return false;
    }
}

}  // namespace xrfg
