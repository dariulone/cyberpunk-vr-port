#pragma once
#include <d3d12.h>

namespace cvr::capture {
// A generated image is not covered by the original capture's completion fence.
// Keep its copy behind the actual synthesis producer, including live FG toggles.
// A pending real endpoint is an ordinary capture, not a generated image.
inline ID3D12CommandQueue* SubmissionQueue(bool captureFinished,bool synthetic,
    ID3D12CommandQueue* game,ID3D12CommandQueue* runtime,ID3D12CommandQueue* synthesis) {
    return synthetic ? synthesis : captureFinished ? runtime : game;
}
}
