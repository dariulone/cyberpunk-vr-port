#pragma once
// ListAudit -- who resets a command allocator under a recorded list, or submits a list that is not closed.
//
// The D3D12 debug layer names the fault ("The command allocator was reset after the command list was recorded",
// "Command lists must be closed before execution") but validates submissions on its own worker thread, so its
// callback never shows the caller. With CVR_GPU_DIAG containing "lists" every allocator Reset (own hook), list
// Reset and Close (the command-list vtable hooks) and ExecuteCommandLists (the queue hook) is recorded with its
// thread and stack; a submission whose list is open, or whose allocator was reset after the list's Reset, is
// logged with the three stacks: the submitter's, the recorder's and the allocator resetter's.
#include <d3d12.h>

namespace cvr::list_audit {
bool Enabled();
void InstallAllocatorHook(ID3D12Device* device);
void ListReset(ID3D12GraphicsCommandList* list, ID3D12CommandAllocator* allocator);
void ListClose(ID3D12GraphicsCommandList* list);
void Execute(ID3D12CommandQueue* queue, UINT count, ID3D12CommandList* const* lists);
}  // namespace cvr::list_audit
