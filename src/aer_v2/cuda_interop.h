// D3D12 <-> CUDA zero-copy interop for AER V2.
//
// CUDA is loaded dynamically so the build does not require the CUDA Toolkit and
// dxgi.dll still loads cleanly on non-NVIDIA systems. The loader prefers a local
// deployable `cudart*.dll` next to the game binary (or in a `cuda\` subfolder),
// then falls back to the normal DLL search path. `nvcuda.dll` still comes from the
// installed NVIDIA driver.
//
// Handle types used:
//   cudaExternalMemoryHandleTypeD3D12Resource = 4   (NT shared handle from ID3D12Device::CreateSharedHandle)
//   cudaExternalSemaphoreHandleTypeD3D12Fence = 2   (NT shared handle from ID3D12Fence)
//
#pragma once

#include <cstdint>
#include <windows.h>
#include <d3d12.h>
#include <wrl.h>

namespace aer_v2 {

// Opaque CUDA handles (kept as void* so the header has no CUDA dependency).
using CUcontext         = void*;
using CUstream          = void*;
using CUarray           = void*;
using CUmipmappedArray  = void*;
using CUdeviceptr       = void*;
using CUresult          = int;
using cudaError_t       = int;
using cudaExternalMemory_t   = void*;
using cudaExternalSemaphore_t = void*;
using cudaTextureObject_t    = uint64_t;
using cudaSurfaceObject_t    = uint64_t;
using cudaEvent_t            = void*;

// RAII handle to a CUDA-imported D3D12 resource. One per (D3D12 resource,
// version); re-imported only when the source resource changes.
struct CudaExternalMemory {
    cudaExternalMemory_t extMem = nullptr;
    CUmipmappedArray     mipmap = nullptr;
    CUarray              level0 = nullptr;   // mip 0 sub-handle from cudaGetMipmappedArrayLevel
    cudaTextureObject_t  texObj = 0;          // SRV-equivalent (read-only)
    cudaSurfaceObject_t  surfObj = 0;         // UAV-equivalent (read-write)
    ID3D12Resource*      source = nullptr;    // AddRef'd for lifetime tracking
    uint64_t             version = 0;         // D3D12 resource version at import time
};

// RAII handle to a CUDA-imported D3D12 fence (cross-API sync). The D3D12 queue
// signals the underlying fence to a value; CUDA waits on the imported semaphore
// for that same value.
struct CudaExternalSemaphore {
    cudaExternalSemaphore_t extSem = nullptr;
    ID3D12Fence*         source = nullptr;    // AddRef'd
    uint64_t             value = 0;           // last value signal'd/waited
};

// Manages the dynamic CUDA binding. One instance per AER V2 pipeline.
// Thread-safe: internal mutex guards the function-pointer table and context.
class CudaInterop {
public:
    CudaInterop() = default;
    ~CudaInterop();

    CudaInterop(const CudaInterop&) = delete;
    CudaInterop& operator=(const CudaInterop&) = delete;

    // Load cudart*.dll + nvcuda.dll and resolve every entry point we need.
    // Returns false (and logs) if either DLL or any critical function is missing.
    // Idempotent: re-calling on a loaded instance is a no-op.
    bool Load();

    bool IsLoaded() const { return m_loaded; }
    void Unload();

    // ---- CUDA context (driver API) ----
    // Get the CUcontext bound to the calling thread.
    bool GetCurrentContext(CUcontext* out);
    // Push a context onto the calling thread's stack (needed before NvOF calls
    // if the thread doesn't already own a primary context).
    bool PushContext(CUcontext ctx);
    bool PopContext(CUcontext* out);

    // ---- Streams (priority non-blocking) ----
    bool GetStreamPriorityRange(int* lo, int* hi);
    bool CreateStreamWithPriority(CUstream* out, int priority);
    bool DestroyStream(CUstream stream);

    // ---- Events ----
    bool EventCreate(cudaEvent_t* out, bool blockingSync = false);
    bool EventRecord(cudaEvent_t evt, CUstream stream);
    bool EventDestroy(cudaEvent_t evt);
    bool EventElapsedTime(float* outMs, cudaEvent_t start, cudaEvent_t end);

    // ---- D3D12 -> CUDA external memory ----
    // Imports the D3D12 resource into CUDA as a mipmapped array + builds a
    // texture object (read-only sampler) and/or surface object (read-write store).
    // `writable=true` requests the surface object; `asArray=false` keeps it 2D.
    // The imported resource must have been created on a device that supports
    // shared NT handles (CreateSharedHandle succeeds).
    bool ImportD3D12Resource(ID3D12Resource* res,
                             uint64_t resourceVersion,
                             bool writable,
                             CudaExternalMemory* out);
    void ReleaseExternalMemory(CudaExternalMemory* m);

    // ---- D3D12 fence -> CUDA semaphore ----
    bool ImportD3D12Fence(ID3D12Fence* fence, CudaExternalSemaphore* out);
    void ReleaseExternalSemaphore(CudaExternalSemaphore* s);

    // ---- Cross-API sync ----
    // cudaSignalExternalSemaphoresAsync_v2: queue a signal on `stream` so when
    // the stream reaches this point, the D3D12 fence value advances to `value`.
    bool SignalSemaphore(CudaExternalSemaphore* s, uint64_t value, CUstream stream);
    // cudaWaitExternalSemaphoresAsync_v2: queue a wait so the stream blocks
    // until the D3D12 fence reaches `value`.
    bool WaitSemaphore(CudaExternalSemaphore* s, uint64_t value, CUstream stream);

    // ---- GPU↔GPU copies used by NvOFCuda input staging ----
    // Copy one 2D CUDA array into another on `stream` using cuMemcpy2DAsync.
    // Width is in BYTES, height in rows. Caller ensures both arrays belong to
    // the current CUcontext. This mirrors the internal upload/download paths in
    // NvOFBufferCudaArray::{Upload,Download} but keeps the data fully on-GPU.
    bool CopyArrayToArray(CUarray srcArray, uint32_t widthBytes, uint32_t height, CUarray dstArray, CUstream stream);
    bool StreamSynchronize(CUstream stream);

    // ---- Error formatting ----
    const char* GetRuntimeErrorName(int err);
    const char* GetRuntimeErrorString(int err);
    const char* GetDriverErrorName(int err);
    const char* GetDriverErrorString(int err);

    // ---- Misc runtime calls used by the pipeline / NvOF ----
    bool MallocPitch(CUdeviceptr* out, size_t* pitch, size_t widthBytes, size_t height);
    bool Free(CUdeviceptr ptr);
    bool DriverGetVersion(int* out);

private:
    HMODULE m_cudart = nullptr;
    HMODULE m_nvcuda = nullptr;
    bool    m_loaded = false;

    // Function-pointer table. Populated by Load(). The exact signatures mirror
    // CUDA Runtime 12 / Driver API; pointers are stored as void* and cast at
    // call sites to avoid dragging cuda.h into the header.
    struct Fn;
    Fn* m_fn = nullptr;   // pimpl to keep the header clean
};

} // namespace aer_v2
