#include "nvof_instance.h"

#include <cstdio>
#include <cstdlib>

extern void Log(const char* fmt, ...);
extern "C" int GetVrNvofPerf();  // ini xr_nvof_perf (env CPVR_NVOF_PERF override inside)

#if defined(AER_V2_NVOF_ENABLED)
#include "NvOFCuda.h"

namespace aer_v2 {

namespace {
void LogNv(const char* prefix, const char* msg) {
    char buf[512];
    std::snprintf(buf, sizeof(buf), "[aer_v2::nvof] %s%s%s\n", prefix ? prefix : "", prefix ? ": " : "", msg ? msg : "");
    OutputDebugStringA(buf);
    Log("%s", buf);
}
}

struct NvOFInstance::Impl {
    NvOFObj flow;
    ::CUcontext ctx = nullptr;
    ::CUstream inStream = nullptr;
    ::CUstream outStream = nullptr;

    std::vector<NvOFBufferObj> inputs;
    std::vector<NvOFBufferObj> outputs;
    std::vector<NvOFBufferObj> costs;

    NvOFBufferCudaArray* inputArray[2] = { nullptr, nullptr };
    NvOFBufferCudaDevicePtr* flowBuffer = nullptr;
    NvOFBufferCudaDevicePtr* costBuffer = nullptr;

    uint32_t flowStrideBytes = 0;
    uint32_t costStrideBytes = 0;
};

NvOFInstance::NvOFInstance() = default;

NvOFInstance::~NvOFInstance() { Shutdown(); }

bool NvOFInstance::Init(CudaInterop* cuda, uint32_t width, uint32_t height) {
    if (!cuda || width == 0 || height == 0) {
        return false;
    }
    if (m_ready && m_cuda == cuda && m_width == width && m_height == height) {
        return true;
    }

    Shutdown();
    m_cuda = cuda;

    auto impl = std::make_unique<Impl>();
    if (!m_cuda->GetCurrentContext(reinterpret_cast<aer_v2::CUcontext*>(&impl->ctx)) || !impl->ctx) {
        LogNv("Init", "cuCtxGetCurrent returned null / failed");
        return false;
    }

    int leastPriority = 0;
    int greatestPriority = 0;
    if (!m_cuda->GetStreamPriorityRange(&leastPriority, &greatestPriority)) {
        LogNv("Init", "cudaDeviceGetStreamPriorityRange failed");
        return false;
    }
    if (!m_cuda->CreateStreamWithPriority(reinterpret_cast<aer_v2::CUstream*>(&impl->inStream), greatestPriority) ||
        !m_cuda->CreateStreamWithPriority(reinterpret_cast<aer_v2::CUstream*>(&impl->outStream), greatestPriority)) {
        LogNv("Init", "cudaStreamCreateWithPriority failed");
        if (impl->inStream) m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->inStream));
        if (impl->outStream) m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->outStream));
        return false;
    }

    try {
        // NvOF perf level. SLOW (20) gives the densest flow, but it benefits from a
        // reduced OF input resolution to stay within the frame budget. At our full
        // 3072^2 x2 eyes SLOW couldn't keep up, so FAST stays the safe default.
        // Override via env CPVR_NVOF_PERF (0=FAST, 1=MEDIUM, 2=SLOW) -- raise once
        // an OF-input downscale path is implemented.
        const int pv = GetVrNvofPerf();
        const NV_OF_PERF_LEVEL perf = pv >= 2 ? NV_OF_PERF_LEVEL_SLOW
                                    : (pv == 1 ? NV_OF_PERF_LEVEL_MEDIUM : NV_OF_PERF_LEVEL_FAST);
        impl->flow = NvOFCuda::Create(
            impl->ctx,
            width,
            height,
            NV_OF_BUFFER_FORMAT_ABGR8,
            NV_OF_CUDA_BUFFER_TYPE_CUARRAY,
            NV_OF_CUDA_BUFFER_TYPE_CUDEVICEPTR,
            NV_OF_MODE_OPTICALFLOW,
            perf,
            impl->inStream,
            impl->outStream);

        uint32_t gridSize = NV_OF_OUTPUT_VECTOR_GRID_SIZE_4;
        if (!impl->flow->CheckGridSize(gridSize)) {
            uint32_t fallbackGrid = 0;
            if (!impl->flow->GetNextMinGridSize(gridSize, fallbackGrid)) {
                LogNv("Init", "no supported NvOF grid size for requested resolution");
                m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->inStream));
                m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->outStream));
                return false;
            }
            gridSize = fallbackGrid;
        }
        impl->flow->Init(gridSize, NV_OF_HINT_VECTOR_GRID_SIZE_UNDEFINED, false, false, true);

        impl->inputs = impl->flow->CreateBuffers(NV_OF_BUFFER_USAGE_INPUT, 2);
        impl->outputs = impl->flow->CreateBuffers(NV_OF_BUFFER_USAGE_OUTPUT, 1);
        impl->costs = impl->flow->CreateBuffers(NV_OF_BUFFER_USAGE_COST, 1);

        impl->inputArray[0] = dynamic_cast<NvOFBufferCudaArray*>(impl->inputs[0].get());
        impl->inputArray[1] = dynamic_cast<NvOFBufferCudaArray*>(impl->inputs[1].get());
        impl->flowBuffer = dynamic_cast<NvOFBufferCudaDevicePtr*>(impl->outputs[0].get());
        impl->costBuffer = dynamic_cast<NvOFBufferCudaDevicePtr*>(impl->costs[0].get());
        if (!impl->inputArray[0] || !impl->inputArray[1] || !impl->flowBuffer || !impl->costBuffer) {
            LogNv("Init", "failed to acquire NvOFCuda buffer specializations");
            m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->inStream));
            m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->outStream));
            return false;
        }

        const auto flowStrideInfo = impl->flowBuffer->getStrideInfo();
        impl->flowStrideBytes = flowStrideInfo.strideInfo[0].strideXInBytes;
        const auto costStrideInfo = impl->costBuffer->getStrideInfo();
        impl->costStrideBytes = costStrideInfo.strideInfo[0].strideXInBytes;

        m_impl = std::move(impl);
        m_width = width;
        m_height = height;
        m_gridSize = gridSize;
        m_ready = true;

        char buf[256];
        std::snprintf(buf, sizeof(buf), "initialized width=%u height=%u grid=%u flowStride=%u", width, height, gridSize, m_impl->flowStrideBytes);
        LogNv("Init", buf);
        return true;
    }
    catch (const NvOFException& e) {
        LogNv("Init", e.what());
    }
    catch (const std::exception& e) {
        LogNv("Init", e.what());
    }

    if (impl->inStream) m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->inStream));
    if (impl->outStream) m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(impl->outStream));
    return false;
}

void NvOFInstance::Shutdown() {
    if (m_impl && m_cuda) {
        if (m_impl->inStream) {
            m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(m_impl->inStream));
        }
        if (m_impl->outStream) {
            m_cuda->DestroyStream(reinterpret_cast<aer_v2::CUstream>(m_impl->outStream));
        }
    }
    m_impl.reset();
    m_ready = false;
    m_width = 0;
    m_height = 0;
    m_gridSize = 0;
    m_cuda = nullptr;
}

bool NvOFInstance::Execute(CUarray prevArray, CUarray currArray, CUdeviceptr* outFlowDevPtr) {
    if (!m_ready || !m_impl || !m_cuda || !prevArray || !currArray || !outFlowDevPtr) {
        return false;
    }

    const uint32_t widthBytes = m_width * 4; // NV_OF_BUFFER_FORMAT_ABGR8
    if (!m_cuda->CopyArrayToArray(prevArray, widthBytes, m_height, reinterpret_cast<aer_v2::CUarray>(m_impl->inputArray[0]->getCudaArray()), reinterpret_cast<aer_v2::CUstream>(m_impl->inStream)) ||
        !m_cuda->CopyArrayToArray(currArray, widthBytes, m_height, reinterpret_cast<aer_v2::CUarray>(m_impl->inputArray[1]->getCudaArray()), reinterpret_cast<aer_v2::CUstream>(m_impl->inStream))) {
        LogNv("Execute", "failed to stage imported CUarray inputs into NvOF buffers");
        return false;
    }

    try {
        // One session processes both eye streams; do not leak temporal hints
        // across left/right eyes (same rationale as OpticalFlowD3D12.cpp).
        m_impl->flow->Execute(
            m_impl->inputArray[0],
            m_impl->inputArray[1],
            m_impl->flowBuffer,
            nullptr,
            m_impl->costBuffer,
            0,
            nullptr,
            nullptr,
            0,
            nullptr,
            NV_OF_TRUE);
    }
    catch (const NvOFException& e) {
        LogNv("Execute", e.what());
        return false;
    }
    catch (const std::exception& e) {
        LogNv("Execute", e.what());
        return false;
    }

    if (!m_cuda->StreamSynchronize(reinterpret_cast<aer_v2::CUstream>(m_impl->outStream))) {
        LogNv("Execute", "cuStreamSynchronize(output) failed");
        return false;
    }

    *outFlowDevPtr = reinterpret_cast<aer_v2::CUdeviceptr>(static_cast<uintptr_t>(m_impl->flowBuffer->getCudaDevicePtr()));
    return true;
}

CUdeviceptr NvOFInstance::GetCostMapDevPtr() const {
    if (!m_impl || !m_impl->costBuffer) return nullptr;
    return reinterpret_cast<aer_v2::CUdeviceptr>(static_cast<uintptr_t>(m_impl->costBuffer->getCudaDevicePtr()));
}

uint32_t NvOFInstance::GetFlowStrideBytes() const {
    return m_impl ? m_impl->flowStrideBytes : 0;
}

uint32_t NvOFInstance::GetCostStrideBytes() const {
    return m_impl ? m_impl->costStrideBytes : 0;
}

CUstream NvOFInstance::GetInputStream() const {
    return m_impl ? reinterpret_cast<aer_v2::CUstream>(m_impl->inStream) : nullptr;
}

} // namespace aer_v2

#else

namespace aer_v2 {

struct NvOFInstance::Impl {};

NvOFInstance::NvOFInstance() = default;

NvOFInstance::~NvOFInstance() { Shutdown(); }

bool NvOFInstance::Init(CudaInterop* cuda, uint32_t width, uint32_t height) {
    (void)cuda; (void)width; (void)height;
    OutputDebugStringA("[aer_v2::nvof] Init stub — CUDA Toolkit / NvOFCuda backend not enabled at build time\n");
    return false;
}

void NvOFInstance::Shutdown() {
    m_ready = false;
    m_width = m_height = m_gridSize = 0;
    m_impl.reset();
}

bool NvOFInstance::Execute(CUarray prevArray, CUarray currArray, CUdeviceptr* outFlowDevPtr) {
    (void)prevArray; (void)currArray; (void)outFlowDevPtr;
    return false;
}

CUdeviceptr NvOFInstance::GetCostMapDevPtr() const { return nullptr; }
uint32_t NvOFInstance::GetFlowStrideBytes() const { return 0; }
uint32_t NvOFInstance::GetCostStrideBytes() const { return 0; }
CUstream NvOFInstance::GetInputStream() const { return nullptr; }

} // namespace aer_v2

#endif
