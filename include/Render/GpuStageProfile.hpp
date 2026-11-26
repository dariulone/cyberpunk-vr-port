#pragma once
#include <d3d12.h>
#include <cstdint>
#include <memory>
#include <utility>

namespace cvr::gpu::profile {
enum class Stage : unsigned { CaptureMain, CaptureSecond, CaptureDepth, SubmitColor, SubmitDepth,
    SecondHud, SecondBlit, HudSprites, HudBlur, HudStyle, HudSubmit, Count };
#if CVR_RUST_VRIK
// The profile: rust/crates/cvr-gpu/src/stage_profile.rs. A BatchPtr holds one reference to its Rust batch.
extern "C" {
const void* cvr_gpu_stage_begin(ID3D12GraphicsCommandList* list);
const void* cvr_gpu_stage_clone(const void* batch);
void cvr_gpu_stage_free(const void* batch);
uint32_t cvr_gpu_stage_scope_begin(const void* batch, uint32_t stage);
void cvr_gpu_stage_scope_end(const void* batch, uint32_t index);
void cvr_gpu_stage_submitted(const void* batch, ID3D12CommandQueue* queue, ID3D12Fence* fence, uint64_t value);
void cvr_gpu_stage_poll();
}
class BatchPtr {
public:
    BatchPtr() = default;
    explicit BatchPtr(const void* batch) : m_batch(batch) {}
    BatchPtr(const BatchPtr& other) : m_batch(other.m_batch ? cvr_gpu_stage_clone(other.m_batch) : nullptr) {}
    BatchPtr(BatchPtr&& other) noexcept : m_batch(other.m_batch) { other.m_batch = nullptr; }
    BatchPtr& operator=(BatchPtr other) noexcept { std::swap(m_batch, other.m_batch); return *this; }
    ~BatchPtr() { if (m_batch) cvr_gpu_stage_free(m_batch); }
    void reset() { BatchPtr().swap(*this); }
    void swap(BatchPtr& other) noexcept { std::swap(m_batch, other.m_batch); }
    explicit operator bool() const { return m_batch != nullptr; }
    const void* get() const { return m_batch; }
private:
    const void* m_batch = nullptr;
};
inline BatchPtr Begin(ID3D12GraphicsCommandList* list) { return BatchPtr(cvr_gpu_stage_begin(list)); }
class Scope {
public:
    Scope(const BatchPtr& batch, Stage stage) : m_batch(batch.get()) {
        if (m_batch) m_index = cvr_gpu_stage_scope_begin(m_batch, static_cast<uint32_t>(stage));
    }
    ~Scope() { if (m_index != ~0u) cvr_gpu_stage_scope_end(m_batch, m_index); }
    Scope(const Scope&) = delete;
    Scope& operator=(const Scope&) = delete;
private:
    const void* m_batch{};
    uint32_t m_index = ~0u;
};
inline void Submitted(BatchPtr batch, ID3D12CommandQueue* queue, ID3D12Fence* fence, UINT64 value) {
    cvr_gpu_stage_submitted(batch.get(), queue, fence, value);
}
inline void Poll() { cvr_gpu_stage_poll(); }
#else
struct Batch;
using BatchPtr = std::shared_ptr<Batch>;
// Internal, bounded diagnostic request, independent of the broad runtime gate.
// No D3D calls or allocations while unarmed. Lists remain owned by the caller.
BatchPtr Begin(ID3D12GraphicsCommandList*);
class Scope {
public:
    Scope(const BatchPtr&, Stage);
    ~Scope();
    Scope(const Scope&) = delete;
    Scope& operator=(const Scope&) = delete;
private:
    Batch* batch{};
    unsigned index = ~0u;
};
// Call only after successful submission and Signal on the supplied queue/fence.
void Submitted(BatchPtr, ID3D12CommandQueue*, ID3D12Fence*, UINT64 value);
// Nonblocking: only maps completed readbacks. Called from Present.
void Poll();
#endif
}
