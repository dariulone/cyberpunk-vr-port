// D3D12 <-> CUDA interop implementation. CUDA entry points are resolved at
// runtime so the project builds without the CUDA Toolkit and runs on
// non-NVIDIA GPUs (Load() simply returns false there and the AER V2 path stays
// disabled).
#include "cuda_interop.h"

#include <cstdio>
#include <new>
#include <wrl.h>

extern void Log(const char* fmt, ...);
extern "C" IMAGE_DOS_HEADER __ImageBase;

// CUDA calling convention macros — empty on x64 (default cdecl). Defined here
// so the typedefs below match the headers exactly without pulling cuda.h in.
#ifndef CUDARTAPI
#define CUDARTAPI
#endif
#ifndef CUDAAPI
#define CUDAAPI
#endif

namespace aer_v2 {

// ---------------------------------------------------------------------------
// Minimal CUDA Runtime / Driver declarations (layout matches CUDA 12 ABI).
// We vendor only the surface we touch instead of pulling cuda.h in.
// ---------------------------------------------------------------------------

// cudaExternalMemoryHandleType (subset)
enum CudaExtMemHandleType {
    EXTMEM_HANDLE_D3D12_RESOURCE = 5,   // cudaExternalMemoryHandleTypeD3D12Resource
};
// cudaExternalSemaphoreHandleType (subset)
enum CudaExtSemHandleType {
    EXTSEM_HANDLE_D3D12_FENCE = 4,      // cudaExternalSemaphoreHandleTypeD3D12Fence
};
// cudaResourceType
enum CudaResType {
    RES_ARRAY = 0,   // cudaResourceTypeArray
};
// cudaTextureAddressMode
enum CudaTexAddr { TEX_ADDR_CLAMP = 3 };
// cudaTextureFilterMode
enum CudaTexFilter { TEX_FILTER_LINEAR = 1 };
// cudaTextureReadMode
enum CudaTexRead {
    TEX_READ_ELEMENT_TYPE = 0,      // cudaReadModeElementType
    TEX_READ_NORM_FLOAT   = 1,      // cudaReadModeNormalizedFloat
};
// cudaChannelFormatKind / cudaArray flags (subset)
enum CudaChannelFormatKind {
    CHAN_SIGNED   = 0,
    CHAN_UNSIGNED = 1,
    CHAN_FLOAT    = 2,
};
enum CudaArrayFlags {
    CUDA_ARRAY_SURFACE_LOAD_STORE = 0x02,
};
enum CudaExternalMemoryFlags {
    CUDA_EXTERNAL_MEMORY_DEDICATED = 0x1,
};
// cudaChannelFormatDesc
struct CudaChannelFormatDesc {
    int x;
    int y;
    int z;
    int w;
    uint32_t f;
};
// cudaExtent
struct CudaExtent {
    size_t width;
    size_t height;
    size_t depth;
};
// cudaExternalMemoryMipmappedArrayDesc (CUDA Runtime ABI)
struct CudaExtMemMipmapDesc {
    uint64_t offset;
    CudaChannelFormatDesc formatDesc;
    CudaExtent extent;
    uint32_t flags;
    uint32_t numLevels;
    uint32_t reserved[16];
};
// cudaExternalMemoryHandleDesc (CUDA 13 runtime ABI)
struct CudaExtMemHandleDesc {
    uint32_t type;
    union {
        int  fd;
        struct {
            void*       handle;
            const void* name;
        } win32;
        const void* nvSciBufObject;
    } handle;
    uint64_t size;
    uint32_t flags;
    uint32_t reserved[16];
};

// cudaExternalSemaphoreHandleDesc (CUDA 13 runtime ABI)
struct CudaExtSemHandleDesc {
    uint32_t type;
    union {
        int fd;
        struct {
            void*       handle;
            const void* name;
        } win32;
        const void* nvSciSyncObj;
    } handle;
    uint32_t flags;
    uint32_t reserved[16];
};

// cudaExternalSemaphoreSignalParams / WaitParams (CUDA 13 runtime ABI).
struct CudaExtSemSignalParams {
    struct {
        struct { uint64_t value; } fence;
        union { void* fence; uint64_t reserved; } nvSciSync;
        struct { uint64_t key; } keyedMutex;
        uint32_t reserved[12];
    } params;
    uint32_t flags;
    uint32_t reserved[16];
};
struct CudaExtSemWaitParams {
    struct {
        struct { uint64_t value; } fence;
        union { void* fence; uint64_t reserved; } nvSciSync;
        struct { uint64_t key; uint32_t timeoutMs; } keyedMutex;
        uint32_t reserved[10];
    } params;
    uint32_t flags;
    uint32_t reserved[16];
};

// CUDA_RESOURCE_DESC (driver API, 72 bytes)
struct CudaResourceDesc {
    uint32_t resType;                // 0
    uint32_t pad0;
    union {
        struct { uint64_t array; } array_;
        struct { uint64_t mipmap; uint32_t level; uint32_t pad; } mipmap_;
        struct { uint64_t devPtr; uint32_t type; uint32_t w; uint32_t h; uint32_t d; } linear_;
    } res;
    uint32_t flags;
    uint32_t pad1;
};

// CUDA_TEXTURE_DESC (driver API, 96 bytes)
struct CudaTextureDesc {
    uint32_t addressMode[3];
    uint32_t filterMode;
    uint32_t readMode;
    int      sRGB;
    uint32_t borderColor[4];
    uint32_t normalizedCoords;
    uint32_t maxAnisotropy;
    uint32_t mipmapFilterMode;
    float    mipmapLevelBias;
    float    minMipmapLevelClamp;
    float    maxMipmapLevelClamp;
    uint32_t disableTrilinearOptimization;
    uint32_t seamlessCubeMap;
    uint32_t reserved[16];
};

// CUDA_MEMCPY2D (driver API) + memory-type enum, subset needed for array→array copies.
enum CudaMemoryType {
    CU_MEMORYTYPE_HOST   = 0x01,
    CU_MEMORYTYPE_DEVICE = 0x02,
    CU_MEMORYTYPE_ARRAY  = 0x03,
};
struct CudaMemcpy2D {
    size_t srcXInBytes;
    size_t srcY;
    uint32_t srcMemoryType;
    const void* srcHost;
    uint64_t srcDevice;
    CUarray srcArray;
    size_t srcPitch;
    size_t dstXInBytes;
    size_t dstY;
    uint32_t dstMemoryType;
    void* dstHost;
    uint64_t dstDevice;
    CUarray dstArray;
    size_t dstPitch;
    size_t WidthInBytes;
    size_t Height;
};

// Function-pointer types (C calling convention; CUDA uses __cdecl on x64).
typedef int (CUDARTAPI* PFN_cudaImportExternalMemory)(void**, const CudaExtMemHandleDesc*);
typedef int (CUDARTAPI* PFN_cudaDestroyExternalMemory)(void*);
typedef int (CUDARTAPI* PFN_cudaExternalMemoryGetMappedMipmappedArray)(void**, void*, const CudaExtMemMipmapDesc*);
typedef int (CUDARTAPI* PFN_cudaFreeMipmappedArray)(void*);
typedef int (CUDARTAPI* PFN_cudaFreeArray)(void*);
typedef int (CUDARTAPI* PFN_cudaGetMipmappedArrayLevel)(void*, void*, unsigned int);
typedef int (CUDARTAPI* PFN_cudaCreateTextureObject)(uint64_t*, const void*, const CudaTextureDesc*, const void*);
typedef int (CUDARTAPI* PFN_cudaDestroyTextureObject)(uint64_t);
typedef int (CUDARTAPI* PFN_cudaCreateSurfaceObject)(uint64_t*, const CudaResourceDesc*);
typedef int (CUDARTAPI* PFN_cudaDestroySurfaceObject)(uint64_t);
typedef int (CUDARTAPI* PFN_cudaImportExternalSemaphore)(void**, const CudaExtSemHandleDesc*);
typedef int (CUDARTAPI* PFN_cudaDestroyExternalSemaphore)(void*);
typedef int (CUDARTAPI* PFN_cudaSignalExternalSemaphoresAsync_v2)(const void**, const CudaExtSemSignalParams*, size_t, void*);
typedef int (CUDARTAPI* PFN_cudaWaitExternalSemaphoresAsync_v2)(const void**, const CudaExtSemWaitParams*, size_t, void*);
typedef int (CUDARTAPI* PFN_cudaEventCreate)(void**, unsigned int);
typedef int (CUDARTAPI* PFN_cudaEventRecord)(void*, void*);
typedef int (CUDARTAPI* PFN_cudaEventDestroy)(void*);
typedef int (CUDARTAPI* PFN_cudaEventElapsedTime)(float*, void*, void*);
typedef int (CUDARTAPI* PFN_cudaStreamCreateWithPriority)(void**, unsigned int, int);
typedef int (CUDARTAPI* PFN_cudaStreamDestroy)(void*);
typedef int (CUDARTAPI* PFN_cudaDeviceGetStreamPriorityRange)(int*, int*);
typedef int (CUDARTAPI* PFN_cudaMallocPitch)(void**, size_t*, size_t, size_t);
typedef int (CUDARTAPI* PFN_cudaFree)(void*);
typedef int (CUDARTAPI* PFN_cudaGetErrorName)(int, const char**);
typedef int (CUDARTAPI* PFN_cudaGetErrorString)(int, const char**);
typedef int (CUDARTAPI* PFN_cudaDriverGetVersion)(int*);
typedef int (CUDARTAPI* PFN_cudaD3D12GetDevice)(int*, void*);   // IUnknown* (ID3D12Device)

// Driver API (nvcuda.dll)
typedef int (CUDAAPI* PFN_cuCtxGetCurrent)(void**);
typedef int (CUDAAPI* PFN_cuCtxPushCurrent_v2)(void*);
typedef int (CUDAAPI* PFN_cuCtxPopCurrent_v2)(void**);
typedef int (CUDAAPI* PFN_cuMemcpy2DAsync_v2)(const CudaMemcpy2D*, void*);
typedef int (CUDAAPI* PFN_cuStreamSynchronize)(void*);
typedef int (CUDAAPI* PFN_cuGetErrorName)(int, const char**);
typedef int (CUDAAPI* PFN_cuGetErrorString)(int, const char**);

// Aggregate of every entry point we use.
struct CudaInterop::Fn {
    // cudart64_12.dll
    PFN_cudaImportExternalMemory                  ImportExternalMemory{nullptr};
    PFN_cudaDestroyExternalMemory                 DestroyExternalMemory{nullptr};
    PFN_cudaExternalMemoryGetMappedMipmappedArray GetMappedMipmappedArray{nullptr};
    PFN_cudaFreeMipmappedArray                    FreeMipmappedArray{nullptr};
    PFN_cudaFreeArray                             FreeArray{nullptr};
    PFN_cudaGetMipmappedArrayLevel                GetMipmappedArrayLevel{nullptr};
    PFN_cudaCreateTextureObject                   CreateTextureObject{nullptr};
    PFN_cudaDestroyTextureObject                  DestroyTextureObject{nullptr};
    PFN_cudaCreateSurfaceObject                   CreateSurfaceObject{nullptr};
    PFN_cudaDestroySurfaceObject                  DestroySurfaceObject{nullptr};
    PFN_cudaImportExternalSemaphore               ImportExternalSemaphore{nullptr};
    PFN_cudaDestroyExternalSemaphore              DestroyExternalSemaphore{nullptr};
    PFN_cudaSignalExternalSemaphoresAsync_v2      SignalExternalSemaphoresAsync{nullptr};
    PFN_cudaWaitExternalSemaphoresAsync_v2        WaitExternalSemaphoresAsync{nullptr};
    PFN_cudaEventCreate                           EventCreate{nullptr};
    PFN_cudaEventCreate                           EventCreateWithFlags{nullptr};
    PFN_cudaEventRecord                           EventRecord{nullptr};
    PFN_cudaEventDestroy                          EventDestroy{nullptr};
    PFN_cudaEventElapsedTime                      EventElapsedTime{nullptr};
    PFN_cudaStreamCreateWithPriority              StreamCreateWithPriority{nullptr};
    PFN_cudaStreamDestroy                         StreamDestroy{nullptr};
    PFN_cudaDeviceGetStreamPriorityRange          DeviceGetStreamPriorityRange{nullptr};
    PFN_cudaMallocPitch                           MallocPitch{nullptr};
    PFN_cudaFree                                  Free{nullptr};
    PFN_cudaGetErrorName                          GetErrorName{nullptr};
    PFN_cudaGetErrorString                        GetErrorString{nullptr};
    PFN_cudaDriverGetVersion                      DriverGetVersion{nullptr};
    // nvcuda.dll
    PFN_cuCtxGetCurrent                           cuCtxGetCurrent{nullptr};
    PFN_cuCtxPushCurrent_v2                       cuCtxPushCurrent_v2{nullptr};
    PFN_cuCtxPopCurrent_v2                        cuCtxPopCurrent_v2{nullptr};
    PFN_cuMemcpy2DAsync_v2                        cuMemcpy2DAsync_v2{nullptr};
    PFN_cuStreamSynchronize                       cuStreamSynchronize{nullptr};
    PFN_cuGetErrorName                            cuGetErrorName{nullptr};
    PFN_cuGetErrorString                          cuGetErrorString{nullptr};
};

// Globals used by event-flag macros below.
enum {
    CUDA_EVENT_DEFAULT        = 0x0,
    CUDA_EVENT_BLOCKING_SYNC  = 0x1,
    CUDA_EVENT_DISABLE_TIMING = 0x2,
};
enum {
    CUDA_STREAM_NON_BLOCKING  = 0x1,
};

// ----- tiny log helper (no dependency on the rest of the project) -----
namespace {
bool FileExistsA(const char* path) {
    if (!path || !path[0]) return false;
    const DWORD attrs = GetFileAttributesA(path);
    return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) == 0;
}

HMODULE LoadLibraryNextToModule(const char* dllName) {
    if (!dllName || !dllName[0]) return nullptr;

    char modulePath[MAX_PATH]{};
    const DWORD len = GetModuleFileNameA(reinterpret_cast<HMODULE>(&__ImageBase), modulePath, MAX_PATH);
    if (len == 0 || len >= MAX_PATH) return nullptr;

    char* lastSlash = strrchr(modulePath, '\\');
    if (!lastSlash) return nullptr;
    *lastSlash = '\0';

    char localPath[MAX_PATH]{};
    if (sprintf_s(localPath, "%s\\%s", modulePath, dllName) > 0 && FileExistsA(localPath)) {
        return LoadLibraryA(localPath);
    }

    char packagedPath[MAX_PATH]{};
    if (sprintf_s(packagedPath, "%s\\cuda\\%s", modulePath, dllName) > 0 && FileExistsA(packagedPath)) {
        return LoadLibraryA(packagedPath);
    }

    return nullptr;
}

void LogErr(const char* fn, int err, const char* details) {
    char buf[256];
    std::snprintf(buf, sizeof(buf), "[aer_v2::cuda] %s failed err=%d%s%s",
                  fn, err, details ? " (" : "", details ? details : "");
    OutputDebugStringA(buf);
    OutputDebugStringA("\n");
    Log("%s\n", buf);
}

bool IsNormalizedDxgiTexture(DXGI_FORMAT format) {
    switch (format) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:
    case DXGI_FORMAT_B8G8R8X8_UNORM:
    case DXGI_FORMAT_B8G8R8X8_UNORM_SRGB:
        return true;
    default:
        return false;
    }
}

bool FillCudaChannelDesc(DXGI_FORMAT format, CudaChannelFormatDesc* out) {
    if (!out) return false;
    *out = {};
    switch (format) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R8G8B8A8_UNORM_SRGB:
    case DXGI_FORMAT_B8G8R8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_UNORM_SRGB:
    case DXGI_FORMAT_B8G8R8X8_UNORM:
    case DXGI_FORMAT_B8G8R8X8_UNORM_SRGB:
        *out = {8, 8, 8, 8, CHAN_UNSIGNED};
        return true;
    case DXGI_FORMAT_R16G16B16A16_FLOAT:
        *out = {16, 16, 16, 16, CHAN_FLOAT};
        return true;
    case DXGI_FORMAT_R32_FLOAT:
    case DXGI_FORMAT_R32_TYPELESS:
    case DXGI_FORMAT_D32_FLOAT:
        *out = {32, 0, 0, 0, CHAN_FLOAT};
        return true;
    case DXGI_FORMAT_R16G16_FLOAT:
        *out = {16, 16, 0, 0, CHAN_FLOAT};
        return true;
    case DXGI_FORMAT_R32G32_FLOAT:
        *out = {32, 32, 0, 0, CHAN_FLOAT};
        return true;
    case DXGI_FORMAT_R16G16_SINT:
        *out = {16, 16, 0, 0, CHAN_SIGNED};
        return true;
    case DXGI_FORMAT_R32G32_SINT:
        *out = {32, 32, 0, 0, CHAN_SIGNED};
        return true;
    default:
        return false;
    }
}
}

CudaInterop::~CudaInterop() { Unload(); }

bool CudaInterop::Load() {
    if (m_loaded) return true;
    m_fn = new (std::nothrow) Fn();
    if (!m_fn) return false;

    const char* cudartCandidates[] = {
        "cudart64_13.dll",
        "cudart64_130.dll",
        "cudart64_12.dll",
        "cudart64_120.dll",
    };
    for (const char* dll : cudartCandidates) {
        if (!m_cudart) {
            m_cudart = LoadLibraryNextToModule(dll);
        }
        if (!m_cudart) {
            m_cudart = LoadLibraryA(dll);
        }
    }
    m_nvcuda = LoadLibraryA("nvcuda.dll");
    if (!m_cudart || !m_nvcuda) {
        if (!m_cudart) OutputDebugStringA("[aer_v2::cuda] cudart runtime DLL not found (tried 12/13 variants)\n");
        if (!m_nvcuda) OutputDebugStringA("[aer_v2::cuda] nvcuda.dll not found\n");
        Unload();
        return false;
    }

#define BIND_RT(opt, dll, name) \
    m_fn->opt = reinterpret_cast<decltype(m_fn->opt)>(GetProcAddress(dll, name))
#define BIND_DRV(opt, name) BIND_RT(opt, m_nvcuda, name)

    BIND_RT(ImportExternalMemory,            m_cudart, "cudaImportExternalMemory");
    BIND_RT(DestroyExternalMemory,           m_cudart, "cudaDestroyExternalMemory");
    BIND_RT(GetMappedMipmappedArray,         m_cudart, "cudaExternalMemoryGetMappedMipmappedArray");
    BIND_RT(FreeMipmappedArray,              m_cudart, "cudaFreeMipmappedArray");
    BIND_RT(FreeArray,                       m_cudart, "cudaFreeArray");
    BIND_RT(GetMipmappedArrayLevel,          m_cudart, "cudaGetMipmappedArrayLevel");
    BIND_RT(CreateTextureObject,             m_cudart, "cudaCreateTextureObject");
    BIND_RT(DestroyTextureObject,            m_cudart, "cudaDestroyTextureObject");
    BIND_RT(CreateSurfaceObject,             m_cudart, "cudaCreateSurfaceObject");
    BIND_RT(DestroySurfaceObject,            m_cudart, "cudaDestroySurfaceObject");
    BIND_RT(ImportExternalSemaphore,         m_cudart, "cudaImportExternalSemaphore");
    BIND_RT(DestroyExternalSemaphore,        m_cudart, "cudaDestroyExternalSemaphore");
    BIND_RT(SignalExternalSemaphoresAsync,   m_cudart, "cudaSignalExternalSemaphoresAsync");
    BIND_RT(WaitExternalSemaphoresAsync,     m_cudart, "cudaWaitExternalSemaphoresAsync");
    BIND_RT(EventCreate,                     m_cudart, "cudaEventCreate");
    BIND_RT(EventCreateWithFlags,            m_cudart, "cudaEventCreateWithFlags");
    BIND_RT(EventRecord,                     m_cudart, "cudaEventRecord");
    BIND_RT(EventDestroy,                    m_cudart, "cudaEventDestroy");
    BIND_RT(EventElapsedTime,                m_cudart, "cudaEventElapsedTime");
    BIND_RT(StreamCreateWithPriority,        m_cudart, "cudaStreamCreateWithPriority");
    BIND_RT(StreamDestroy,                   m_cudart, "cudaStreamDestroy");
    BIND_RT(DeviceGetStreamPriorityRange,    m_cudart, "cudaDeviceGetStreamPriorityRange");
    BIND_RT(MallocPitch,                     m_cudart, "cudaMallocPitch");
    BIND_RT(Free,                            m_cudart, "cudaFree");
    BIND_RT(GetErrorName,                    m_cudart, "cudaGetErrorName");
    BIND_RT(GetErrorString,                  m_cudart, "cudaGetErrorString");
    BIND_RT(DriverGetVersion,                m_cudart, "cudaDriverGetVersion");

    BIND_DRV(cuCtxGetCurrent,      "cuCtxGetCurrent");
    BIND_DRV(cuCtxPushCurrent_v2,  "cuCtxPushCurrent_v2");
    BIND_DRV(cuCtxPopCurrent_v2,   "cuCtxPopCurrent_v2");
    BIND_DRV(cuMemcpy2DAsync_v2,   "cuMemcpy2DAsync_v2");
    BIND_DRV(cuStreamSynchronize,  "cuStreamSynchronize");
    BIND_DRV(cuGetErrorName,       "cuGetErrorName");
    BIND_DRV(cuGetErrorString,     "cuGetErrorString");
#undef BIND_RT
#undef BIND_DRV

    // Critical entry-point presence check.
    auto must = [&](auto p, const char* name) {
        if (!p) {
            OutputDebugStringA("[aer_v2::cuda] missing proc: ");
            OutputDebugStringA(name);
            OutputDebugStringA("\n");
            Log("[aer_v2::cuda] missing proc: %s\n", name);
            return false;
        }
        return true;
    };
    if (!must(m_fn->ImportExternalMemory,           "cudaImportExternalMemory") ||
        !must(m_fn->DestroyExternalMemory,          "cudaDestroyExternalMemory") ||
        !must(m_fn->ImportExternalSemaphore,        "cudaImportExternalSemaphore") ||
        !must(m_fn->SignalExternalSemaphoresAsync,  "cudaSignalExternalSemaphoresAsync") ||
        !must(m_fn->WaitExternalSemaphoresAsync,    "cudaWaitExternalSemaphoresAsync") ||
        !must(m_fn->EventCreate,                    "cudaEventCreate") ||
        !must(m_fn->EventRecord,                    "cudaEventRecord") ||
        !must(m_fn->cuCtxGetCurrent,                "cuCtxGetCurrent") ||
        !must(m_fn->cuMemcpy2DAsync_v2,             "cuMemcpy2DAsync_v2") ||
        !must(m_fn->cuStreamSynchronize,            "cuStreamSynchronize"))
    {
        Unload();
        return false;
    }

    m_loaded = true;
    return true;
}

void CudaInterop::Unload() {
    delete m_fn;
    m_fn = nullptr;
    if (m_cudart) { FreeLibrary(m_cudart); m_cudart = nullptr; }
    if (m_nvcuda) { FreeLibrary(m_nvcuda); m_nvcuda = nullptr; }
    m_loaded = false;
}

// ---------------------------------------------------------------------------
// Context (driver API)
// ---------------------------------------------------------------------------
bool CudaInterop::GetCurrentContext(CUcontext* out) {
    if (!m_loaded || !m_fn->cuCtxGetCurrent) return false;
    int err = m_fn->cuCtxGetCurrent(out);
    if (err) { LogErr("cuCtxGetCurrent", err, nullptr); return false; }
    return true;
}
bool CudaInterop::PushContext(CUcontext ctx) {
    if (!m_loaded || !m_fn->cuCtxPushCurrent_v2) return false;
    int err = m_fn->cuCtxPushCurrent_v2(ctx);
    if (err) { LogErr("cuCtxPushCurrent_v2", err, nullptr); return false; }
    return true;
}
bool CudaInterop::PopContext(CUcontext* out) {
    if (!m_loaded || !m_fn->cuCtxPopCurrent_v2) return false;
    int err = m_fn->cuCtxPopCurrent_v2(out);
    if (err) { LogErr("cuCtxPopCurrent_v2", err, nullptr); return false; }
    return true;
}

// ---------------------------------------------------------------------------
// Streams (priority non-blocking)
// ---------------------------------------------------------------------------
bool CudaInterop::GetStreamPriorityRange(int* lo, int* hi) {
    if (!m_loaded || !m_fn->DeviceGetStreamPriorityRange) return false;
    int err = m_fn->DeviceGetStreamPriorityRange(lo, hi);
    if (err) { LogErr("cudaDeviceGetStreamPriorityRange", err, nullptr); return false; }
    return true;
}
bool CudaInterop::CreateStreamWithPriority(CUstream* out, int priority) {
    if (!m_loaded || !m_fn->StreamCreateWithPriority) return false;
    int err = m_fn->StreamCreateWithPriority(out, CUDA_STREAM_NON_BLOCKING, priority);
    if (err) { LogErr("cudaStreamCreateWithPriority", err, nullptr); return false; }
    return true;
}
bool CudaInterop::DestroyStream(CUstream stream) {
    if (!m_loaded || !m_fn->StreamDestroy || !stream) return false;
    int err = m_fn->StreamDestroy(stream);
    if (err) { LogErr("cudaStreamDestroy", err, nullptr); return false; }
    return true;
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------
bool CudaInterop::EventCreate(cudaEvent_t* out, bool blockingSync) {
    if (!m_loaded || !out) return false;
    int err;
    if (blockingSync && m_fn->EventCreateWithFlags) {
        err = m_fn->EventCreateWithFlags(out, CUDA_EVENT_BLOCKING_SYNC);
    } else {
        err = m_fn->EventCreate(out, 0);
    }
    if (err) { LogErr("cudaEventCreate", err, nullptr); return false; }
    return true;
}
bool CudaInterop::EventRecord(cudaEvent_t evt, CUstream stream) {
    if (!m_loaded || !m_fn->EventRecord) return false;
    int err = m_fn->EventRecord(evt, stream);
    if (err) { LogErr("cudaEventRecord", err, nullptr); return false; }
    return true;
}
bool CudaInterop::EventDestroy(cudaEvent_t evt) {
    if (!m_loaded || !m_fn->EventDestroy || !evt) return false;
    int err = m_fn->EventDestroy(evt);
    if (err) { LogErr("cudaEventDestroy", err, nullptr); return false; }
    return true;
}
bool CudaInterop::EventElapsedTime(float* outMs, cudaEvent_t start, cudaEvent_t end) {
    if (!m_loaded || !m_fn->EventElapsedTime) return false;
    int err = m_fn->EventElapsedTime(outMs, start, end);
    if (err) { LogErr("cudaEventElapsedTime", err, nullptr); return false; }
    return true;
}

// ---------------------------------------------------------------------------
// D3D12 -> CUDA external memory
// ---------------------------------------------------------------------------
bool CudaInterop::ImportD3D12Resource(ID3D12Resource* res,
                                      uint64_t resourceVersion,
                                      bool writable,
                                      CudaExternalMemory* out) {
    if (!m_loaded || !res || !out) return false;
    out->source  = nullptr;
    out->version = resourceVersion;

    // 1. Get the shared NT handle. ID3D12Device::CreateSharedHandle lives on
    //    the ID3D12Device1 interface (DX12.1+), so QI up from the base device.
    Microsoft::WRL::ComPtr<ID3D12Device> device0;
    if (FAILED(res->GetDevice(IID_PPV_ARGS(&device0)))) {
        OutputDebugStringA("[aer_v2::cuda] GetDevice failed\n");
        return false;
    }
    Microsoft::WRL::ComPtr<ID3D12Device1> device;
    if (FAILED(device0->QueryInterface(IID_PPV_ARGS(&device)))) {
        OutputDebugStringA("[aer_v2::cuda] ID3D12Device1 unavailable (DX12.0 only?)\n");
        return false;
    }
    HANDLE hShared = nullptr;
    HRESULT hr = device->CreateSharedHandle(
        res, nullptr, GENERIC_ALL, nullptr, &hShared);
    if (FAILED(hr)) {
        char buf[160];
        std::snprintf(buf, sizeof(buf), "[aer_v2::cuda] CreateSharedHandle hr=0x%08X\n", (unsigned)hr);
        OutputDebugStringA(buf);
        return false;
    }

    // 2. cudaImportExternalMemory(handleType = D3D12Resource, win32.handle = NT).
    CudaExtMemHandleDesc hd{};
    hd.type = EXTMEM_HANDLE_D3D12_RESOURCE;
    hd.handle.win32.handle = hShared;
    hd.flags = CUDA_EXTERNAL_MEMORY_DEDICATED;
    // size: the full allocation (use resource desc footprint)
    auto const& desc = res->GetDesc();
    D3D12_RESOURCE_ALLOCATION_INFO info =
        device->GetResourceAllocationInfo(0, 1, &desc);
    hd.size = info.SizeInBytes;

    int err = m_fn->ImportExternalMemory(&out->extMem, &hd);
    CloseHandle(hShared);
    if (err) { LogErr("cudaImportExternalMemory", err, nullptr); return false; }

    // 3. Map as a mipmapped array + pull level 0 (we only need the top mip).
    CudaExtMemMipmapDesc md{};
    {
        if (!FillCudaChannelDesc(desc.Format, &md.formatDesc)) {
            char buf[160];
            std::snprintf(buf, sizeof(buf), "[aer_v2::cuda] unsupported DXGI format for CUDA external array: %u\n", static_cast<unsigned>(desc.Format));
            OutputDebugStringA(buf);
            return false;
        }
        md.offset = 0;
        md.extent.width = static_cast<size_t>(desc.Width);
        md.extent.height = static_cast<size_t>(desc.Height);
        md.extent.depth = (desc.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE3D) ? desc.DepthOrArraySize : 1;
        md.flags = CUDA_ARRAY_SURFACE_LOAD_STORE;
        // D3D12Resource external-memory mappings expose mip 0 only.
        md.numLevels = 1;
    }
    err = m_fn->GetMappedMipmappedArray(&out->mipmap, out->extMem, &md);
    if (err) { LogErr("cudaExternalMemoryGetMappedMipmappedArray", err, nullptr); return false; }
    err = m_fn->GetMipmappedArrayLevel(&out->level0, out->mipmap, 0);
    if (err) { LogErr("cudaGetMipmappedArrayLevel", err, nullptr); return false; }

    // 4. Texture object (read-only sampler).
    if (m_fn->CreateTextureObject) {
        CudaResourceDesc rd{};
        rd.resType = RES_ARRAY;
        rd.res.array_.array = reinterpret_cast<uint64_t>(out->level0);
        CudaTextureDesc td{};
        td.addressMode[0] = td.addressMode[1] = td.addressMode[2] = TEX_ADDR_CLAMP;
        td.filterMode       = TEX_FILTER_LINEAR;
        td.readMode         = IsNormalizedDxgiTexture(desc.Format) ? TEX_READ_NORM_FLOAT : TEX_READ_ELEMENT_TYPE;
        td.normalizedCoords = 1;
        err = m_fn->CreateTextureObject(&out->texObj, &rd, &td, nullptr);
        if (err) LogErr("cudaCreateTextureObject", err, nullptr);
    }
    // 5. Surface object for ALL textures (read via surf2Dread in kernel).
    if (m_fn->CreateSurfaceObject) {
        CudaResourceDesc rd{};
        rd.resType = RES_ARRAY;
        rd.res.array_.array = reinterpret_cast<uint64_t>(out->level0);
        err = m_fn->CreateSurfaceObject(&out->surfObj, &rd);
        if (err) LogErr("cudaCreateSurfaceObject", err, nullptr);
    }
    res->AddRef();
    out->source = res;
    return true;
}

void CudaInterop::ReleaseExternalMemory(CudaExternalMemory* m) {
    if (!m || !m_loaded) return;
    if (m->surfObj && m_fn->DestroySurfaceObject) m_fn->DestroySurfaceObject(m->surfObj);
    if (m->texObj  && m_fn->DestroyTextureObject)  m_fn->DestroyTextureObject(m->texObj);
    // level0 is owned by the mipmapped array returned from external memory;
    // freeing it separately corrupts the external-memory object.
    if (m->mipmap && m_fn->FreeMipmappedArray)     m_fn->FreeMipmappedArray(m->mipmap);
    if (m->extMem && m_fn->DestroyExternalMemory)  m_fn->DestroyExternalMemory(m->extMem);
    if (m->source) m->source->Release();
    *m = CudaExternalMemory{};
}

// ---------------------------------------------------------------------------
// D3D12 fence -> CUDA semaphore
// ---------------------------------------------------------------------------
bool CudaInterop::ImportD3D12Fence(ID3D12Fence* fence, CudaExternalSemaphore* out) {
    if (!m_loaded || !fence || !out) return false;
    out->source = fence;

    Microsoft::WRL::ComPtr<ID3D12Device> device0;
    if (FAILED(fence->GetDevice(IID_PPV_ARGS(&device0)))) {
        OutputDebugStringA("[aer_v2::cuda] fence->GetDevice failed\n");
        return false;
    }
    Microsoft::WRL::ComPtr<ID3D12Device1> device;
    if (FAILED(device0->QueryInterface(IID_PPV_ARGS(&device)))) {
        OutputDebugStringA("[aer_v2::cuda] fence: ID3D12Device1 unavailable\n");
        return false;
    }
    HANDLE hShared = nullptr;
    HRESULT hr = device->CreateSharedHandle(fence, nullptr, GENERIC_ALL, L"CPVRCudaFence", &hShared);
    if (FAILED(hr)) {
        char buf[160];
        std::snprintf(buf, sizeof(buf), "[aer_v2::cuda] fence CreateSharedHandle hr=0x%08X\n", (unsigned)hr);
        OutputDebugStringA(buf);
        return false;
    }
    CudaExtSemHandleDesc hd{};
    hd.type = EXTSEM_HANDLE_D3D12_FENCE;
    hd.handle.win32.handle = hShared;
    hd.flags = 0;
    int err = m_fn->ImportExternalSemaphore(&out->extSem, &hd);
    CloseHandle(hShared);
    if (err) { LogErr("cudaImportExternalSemaphore", err, nullptr); return false; }
    fence->AddRef();
    return true;
}
void CudaInterop::ReleaseExternalSemaphore(CudaExternalSemaphore* s) {
    if (!s || !m_loaded) return;
    if (s->extSem && m_fn->DestroyExternalSemaphore) m_fn->DestroyExternalSemaphore(s->extSem);
    if (s->source) s->source->Release();
    *s = CudaExternalSemaphore{};
}

// ---------------------------------------------------------------------------
// Cross-API sync
// ---------------------------------------------------------------------------
bool CudaInterop::SignalSemaphore(CudaExternalSemaphore* s, uint64_t value, CUstream stream) {
    if (!m_loaded || !s || !s->extSem) return false;
    const void* sems[1] = { s->extSem };
    CudaExtSemSignalParams params{};
    params.params.fence.value = value; // D3D12Fence: target fence value to signal
    int err = m_fn->SignalExternalSemaphoresAsync(sems, &params, 1, stream);
    if (err) { LogErr("cudaSignalExternalSemaphoresAsync", err, nullptr); return false; }
    s->value = value;
    return true;
}
bool CudaInterop::WaitSemaphore(CudaExternalSemaphore* s, uint64_t value, CUstream stream) {
    if (!m_loaded || !s || !s->extSem) return false;
    const void* sems[1] = { s->extSem };
    CudaExtSemWaitParams params{};
    params.params.fence.value = value; // D3D12Fence: target fence value to wait on
    int err = m_fn->WaitExternalSemaphoresAsync(sems, &params, 1, stream);
    if (err) { LogErr("cudaWaitExternalSemaphoresAsync", err, nullptr); return false; }
    return true;
}

// ---------------------------------------------------------------------------
// GPU↔GPU copies (used by NvOFCuda input staging)
// ---------------------------------------------------------------------------
bool CudaInterop::CopyArrayToArray(CUarray srcArray, uint32_t widthBytes, uint32_t height, CUarray dstArray, CUstream stream) {
    if (!m_loaded || !m_fn->cuMemcpy2DAsync_v2 || !srcArray || !dstArray || widthBytes == 0 || height == 0) {
        return false;
    }
    CudaMemcpy2D copy{};
    copy.srcMemoryType = CU_MEMORYTYPE_ARRAY;
    copy.srcArray = srcArray;
    copy.dstMemoryType = CU_MEMORYTYPE_ARRAY;
    copy.dstArray = dstArray;
    copy.WidthInBytes = widthBytes;
    copy.Height = height;
    int err = m_fn->cuMemcpy2DAsync_v2(&copy, stream);
    if (err) { LogErr("cuMemcpy2DAsync_v2", err, nullptr); return false; }
    return true;
}

bool CudaInterop::StreamSynchronize(CUstream stream) {
    if (!m_loaded || !m_fn->cuStreamSynchronize || !stream) return false;
    int err = m_fn->cuStreamSynchronize(stream);
    if (err) { LogErr("cuStreamSynchronize", err, nullptr); return false; }
    return true;
}

// ---------------------------------------------------------------------------
// Misc runtime calls
// ---------------------------------------------------------------------------
bool CudaInterop::MallocPitch(CUdeviceptr* out, size_t* pitch, size_t widthBytes, size_t height) {
    if (!m_loaded || !m_fn->MallocPitch) return false;
    int err = m_fn->MallocPitch(out, pitch, widthBytes, height);
    if (err) { LogErr("cudaMallocPitch", err, nullptr); return false; }
    return true;
}
bool CudaInterop::Free(CUdeviceptr ptr) {
    if (!m_loaded || !m_fn->Free) return false;
    int err = m_fn->Free(ptr);
    if (err) { LogErr("cudaFree", err, nullptr); return false; }
    return true;
}
bool CudaInterop::DriverGetVersion(int* out) {
    if (!m_loaded || !m_fn->DriverGetVersion) return false;
    int err = m_fn->DriverGetVersion(out);
    if (err) { LogErr("cudaDriverGetVersion", err, nullptr); return false; }
    return true;
}

const char* CudaInterop::GetRuntimeErrorName(int err)   { const char* s=nullptr; if (m_fn&&m_fn->GetErrorName)   m_fn->GetErrorName(err,&s);   return s?s:"?"; }
const char* CudaInterop::GetRuntimeErrorString(int err) { const char* s=nullptr; if (m_fn&&m_fn->GetErrorString) m_fn->GetErrorString(err,&s); return s?s:"?"; }
const char* CudaInterop::GetDriverErrorName(int err)    { const char* s=nullptr; if (m_fn&&m_fn->cuGetErrorName) m_fn->cuGetErrorName(err,&s); return s?s:"?"; }
const char* CudaInterop::GetDriverErrorString(int err)  { const char* s=nullptr; if (m_fn&&m_fn->cuGetErrorString) m_fn->cuGetErrorString(err,&s); return s?s:"?"; }

} // namespace aer_v2
