#pragma once

#include <d3d12.h>

#include <cstdint>
#include <memory>
#include <mutex>
#include <span>

namespace xrfg {

enum class D3D12HistoryInitializationStage : std::uint32_t {
    complete = 0,
    arguments = 1,
    first_source = 2,
    source_description = 3,
    source_validation = 4,
    create_slot_resource = 5,
    create_slot_allocator = 6,
    create_slot_command_list = 7,
    close_slot_command_list = 8,
    create_fence = 9,
    create_event = 10,
};

struct D3D12HistoryCaptureTicket {
    std::uint64_t serial{};
    std::uint64_t fence_value{};
    std::uint32_t slot{};
    std::uint32_t source_index{};
};

// How long a capture may wait for a busy ring slot, and what it found. A slot
// is busy while the synthesis that read it, or the capture that last wrote
// it, is still running on the GPU. Failing there loses the frame's history:
// generation continuity resets and the next frame primes. When the GPU runs a
// couple of frames behind, that reset is what keeps it behind (the frames
// after a prime are not paced and arrive together), so a short wait for the
// GPU is cheaper than the reset it avoids.
struct D3D12HistoryCaptureWait {
    // In: the longest the capture may block, in microseconds. 0 fails a busy
    // slot at once, as before.
    std::uint32_t limit_us{};
    // Out: the slot was busy when the capture arrived.
    bool slot_busy{};
    // Out: how long the capture blocked, in microseconds.
    std::uint64_t waited_us{};
};

struct D3D12HistoryConsumerLease {
    std::uint64_t capture_serial{};
    std::uint64_t lease_serial{};
    std::uint32_t slot{};
};

class D3D12SwapchainHistory final {
public:
    static constexpr std::uint32_t kSlotCount = 3;

    D3D12SwapchainHistory() noexcept;
    ~D3D12SwapchainHistory();

    D3D12SwapchainHistory(const D3D12SwapchainHistory&) = delete;
    D3D12SwapchainHistory& operator=(const D3D12SwapchainHistory&) = delete;

    [[nodiscard]] HRESULT initialize(
        ID3D12Device* device,
        ID3D12CommandQueue* queue,
        std::span<ID3D12Resource* const> source_images,
        // Native OpenXR resources use their attachment state. Same-adapter
        // D3D11 interop mirrors use COMMON at both API ownership boundaries.
        D3D12_RESOURCE_STATES release_state,
        D3D12HistoryInitializationStage* failure_stage = nullptr,
        bool borrow_sources = false) noexcept;

    // Queues the source-to-history copy and returns after signaling its fence;
    // it never waits for the copy itself. If the next ring slot is still read
    // or written on the GPU, it waits for that work for at most
    // wait->limit_us (none without `wait`), then returns ERROR_BUSY and the
    // caller must fail open rather than overwrite it. A slot leased to a
    // consumer that has not submitted yet is never waited for.
    [[nodiscard]] HRESULT capture(
        std::uint32_t source_index,
        D3D12HistoryCaptureTicket* ticket,
        D3D12HistoryCaptureWait* wait = nullptr) noexcept;

    // Port adapter: previous capture must be complete; source retained until next capture.
    [[nodiscard]] HRESULT capture_source(ID3D12Resource* source, D3D12HistoryCaptureTicket* ticket) noexcept;

    // No copy/transition: caller provides immutable GENERIC_READ texture,
    // capture completion and an owner that prevents producer pool reuse.
    [[nodiscard]] HRESULT capture_borrowed(ID3D12Resource*, ID3D12Fence*, std::uint64_t,
        std::shared_ptr<void> owner, D3D12HistoryCaptureTicket*) noexcept;
    void release_completed() noexcept;
    [[nodiscard]] bool borrowed() const noexcept;

    [[nodiscard]] HRESULT commit(const D3D12HistoryCaptureTicket& ticket) noexcept;

    void discard(const D3D12HistoryCaptureTicket& ticket) noexcept;

    // Acquires exclusive read access to a committed history slot. The returned
    // COM objects are AddRef'd. The consumer must either retire the lease with
    // its GPU-completion fence or cancel it before submitting any GPU access.
    [[nodiscard]] HRESULT acquire_consumer(
        const D3D12HistoryCaptureTicket& ticket,
        D3D12HistoryConsumerLease* lease,
        ID3D12Resource** resource,
        ID3D12Fence** producer_fence) noexcept;

    [[nodiscard]] HRESULT retire_consumer(
        const D3D12HistoryConsumerLease& lease,
        ID3D12Fence* completion_fence,
        std::uint64_t completion_value) noexcept;

    void cancel_consumer(const D3D12HistoryConsumerLease& lease) noexcept;

    [[nodiscard]] HRESULT wait_for_idle() noexcept;

    [[nodiscard]] HRESULT invalidate() noexcept;

    [[nodiscard]] bool initialized() const noexcept;

private:
    struct Impl;

    mutable std::mutex mutex_;
    std::unique_ptr<Impl> impl_;
};

}  // namespace xrfg
