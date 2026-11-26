#pragma once
#include <d3d12.h>
#include <initializer_list>
#include <memory>
#include <utility>
#include <vector>

#if CVR_RUST_VRIK
// The Rust build: rust/crates/cvr-gpu/src/command_resources.rs owns the recordings and the flights; a submission is
// its handle (null for none), copied by sharing its batches.
extern "C" {
bool cvr_gpu_keep_command_resources(void* list, IUnknown* const* objects, size_t count);
bool cvr_gpu_has_command_resource(void* list, IUnknown* object);
void* cvr_gpu_prepare_command_resources(UINT count, ID3D12CommandList* const* lists);
void* cvr_gpu_clone_submission(const void* submission);
void cvr_gpu_free_submission(void* submission);
void cvr_gpu_submit_command_resources(void* queue, void* submission);
void cvr_gpu_reset_command_resources(void* list);
void cvr_gpu_collect_command_resources();
}
namespace cvr::gpu {
class ResourceSubmission {
public:
    ResourceSubmission() = default;
    explicit ResourceSubmission(void* handle) : rust(handle) {}
    ResourceSubmission(const ResourceSubmission& other) : rust(cvr_gpu_clone_submission(other.rust)) {}
    ResourceSubmission(ResourceSubmission&& other) noexcept : rust(std::exchange(other.rust, nullptr)) {}
    ResourceSubmission& operator=(ResourceSubmission other) noexcept { std::swap(rust, other.rust); return *this; }
    ~ResourceSubmission() { cvr_gpu_free_submission(rust); }
    bool Empty() const { return !rust; }
    void* Release() { return std::exchange(rust, nullptr); }
private:
    void* rust = nullptr;
};
inline bool SubmissionEmpty(const ResourceSubmission& submission) { return submission.Empty(); }
// Keep resources through recording, possible replay, and GPU completion.
inline bool KeepCommandResources(ID3D12GraphicsCommandList* list, std::initializer_list<IUnknown*> objects) {
    return cvr_gpu_keep_command_resources(list, objects.begin(), objects.size());
}
// current recording only, not a prior submitted list
inline bool HasCommandResource(ID3D12GraphicsCommandList* list, IUnknown* object) {
    return cvr_gpu_has_command_resource(list, object);
}
inline ResourceSubmission PrepareCommandResources(UINT count, ID3D12CommandList* const* lists) {
    return ResourceSubmission(cvr_gpu_prepare_command_resources(count, lists));
}
inline void SubmitCommandResources(ID3D12CommandQueue* queue, ResourceSubmission submission) {
    cvr_gpu_submit_command_resources(queue, submission.Release());
}
// after successful Reset
inline void ResetCommandResources(ID3D12GraphicsCommandList* list) { cvr_gpu_reset_command_resources(list); }
inline void CollectCommandResources() { cvr_gpu_collect_command_resources(); }
}
#else
namespace cvr::gpu {
struct ResourceBatch;
struct ResourceSubmission { std::vector<std::shared_ptr<const ResourceBatch>> batches; };
inline bool SubmissionEmpty(const ResourceSubmission& submission) { return submission.batches.empty(); }
// Keep resources through recording, possible replay, and GPU completion.
bool KeepCommandResources(ID3D12GraphicsCommandList*, std::initializer_list<IUnknown*>);
bool HasCommandResource(ID3D12GraphicsCommandList*,IUnknown*); // current recording only, not a prior submitted list
ResourceSubmission PrepareCommandResources(UINT count, ID3D12CommandList* const*);
void SubmitCommandResources(ID3D12CommandQueue*, ResourceSubmission);
void ResetCommandResources(ID3D12GraphicsCommandList*); // after successful Reset
void CollectCommandResources();
}
#endif
