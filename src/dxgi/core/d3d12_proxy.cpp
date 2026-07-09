#include <windows.h>

#define D3D12CreateDevice RealD3D12Header_D3D12CreateDevice
#define D3D12GetDebugInterface RealD3D12Header_D3D12GetDebugInterface
#define D3D12EnableExperimentalFeatures RealD3D12Header_D3D12EnableExperimentalFeatures
#define D3D12SerializeRootSignature RealD3D12Header_D3D12SerializeRootSignature
#define D3D12CreateRootSignatureDeserializer RealD3D12Header_D3D12CreateRootSignatureDeserializer
#define D3D12SerializeVersionedRootSignature RealD3D12Header_D3D12SerializeVersionedRootSignature
#define D3D12CreateVersionedRootSignatureDeserializer RealD3D12Header_D3D12CreateVersionedRootSignatureDeserializer
#include <d3d12.h>
#undef D3D12CreateDevice
#undef D3D12GetDebugInterface
#undef D3D12EnableExperimentalFeatures
#undef D3D12SerializeRootSignature
#undef D3D12CreateRootSignatureDeserializer
#undef D3D12SerializeVersionedRootSignature
#undef D3D12CreateVersionedRootSignatureDeserializer

#include <cstdarg>
#include <cstdio>

extern "C" IMAGE_DOS_HEADER __ImageBase;

namespace {
HMODULE g_realD3D12 = nullptr;
HANDLE g_logFile = INVALID_HANDLE_VALUE;

void Log(const char* format, ...) {
    char line[2048]{};
    SYSTEMTIME time{};
    GetLocalTime(&time);

    int prefix = std::snprintf(
        line,
        sizeof(line),
        "[%04u-%02u-%02u %02u:%02u:%02u.%03u] [d3d12] ",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds);

    if (prefix < 0 || prefix >= static_cast<int>(sizeof(line))) {
        return;
    }

    va_list args;
    va_start(args, format);
    int body = std::vsnprintf(line + prefix, sizeof(line) - static_cast<size_t>(prefix), format, args);
    va_end(args);

    if (body < 0) {
        return;
    }

    size_t len = strnlen_s(line, sizeof(line));
    if (len + 2 < sizeof(line)) {
        line[len++] = '\r';
        line[len++] = '\n';
        line[len] = '\0';
    }

    OutputDebugStringA(line);
    if (g_logFile != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        WriteFile(g_logFile, line, static_cast<DWORD>(len), &written, nullptr);
        FlushFileBuffers(g_logFile);
    }
}

void OpenLogFile() {
    char modulePath[MAX_PATH]{};
    DWORD length = GetModuleFileNameA(reinterpret_cast<HMODULE>(&__ImageBase), modulePath, MAX_PATH);
    if (length == 0 || length >= MAX_PATH) {
        return;
    }

    char* lastSlash = strrchr(modulePath, '\\');
    if (lastSlash == nullptr) {
        return;
    }

    *(lastSlash + 1) = '\0';
    strcat_s(modulePath, "cyberpunkvrport.log");

    g_logFile = CreateFileA(
        modulePath,
        FILE_APPEND_DATA,
        FILE_SHARE_READ,
        nullptr,
        OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        nullptr);
}

void GuidToString(REFGUID guid, char* out, size_t outSize) {
    std::snprintf(
        out,
        outSize,
        "{%08lX-%04hX-%04hX-%02hhX%02hhX-%02hhX%02hhX%02hhX%02hhX%02hhX%02hhX}",
        guid.Data1,
        guid.Data2,
        guid.Data3,
        guid.Data4[0],
        guid.Data4[1],
        guid.Data4[2],
        guid.Data4[3],
        guid.Data4[4],
        guid.Data4[5],
        guid.Data4[6],
        guid.Data4[7]);
}

HMODULE LoadRealD3D12() {
    if (g_realD3D12 != nullptr) {
        return g_realD3D12;
    }

    char systemDir[MAX_PATH]{};
    UINT length = GetSystemDirectoryA(systemDir, MAX_PATH);
    if (length == 0 || length >= MAX_PATH) {
        Log("GetSystemDirectoryA failed: %lu", GetLastError());
        return nullptr;
    }

    strcat_s(systemDir, "\\d3d12.dll");
    g_realD3D12 = LoadLibraryA(systemDir);
    Log("LoadLibrary real d3d12: %s -> %p", systemDir, g_realD3D12);
    return g_realD3D12;
}

FARPROC GetRealProc(const char* name) {
    HMODULE realD3D12 = LoadRealD3D12();
    if (realD3D12 == nullptr) {
        return nullptr;
    }

    FARPROC proc = GetProcAddress(realD3D12, name);
    if (proc == nullptr) {
        Log("GetProcAddress failed for %s: %lu", name, GetLastError());
    }
    return proc;
}

template <typename Fn>
Fn GetRealProcAs(const char* name) {
    return reinterpret_cast<Fn>(GetRealProc(name));
}

HRESULT MissingExport(const char* name) {
    Log("Missing required D3D12 export: %s", name);
    return HRESULT_FROM_WIN32(ERROR_PROC_NOT_FOUND);
}
} // namespace

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        OpenLogFile();
        Log("CyberpunkVRPort d3d12 loaded, module=%p, process=%lu", instance, GetCurrentProcessId());
    } else if (reason == DLL_PROCESS_DETACH) {
        Log("CyberpunkVRPort d3d12 unloading");
        if (g_logFile != INVALID_HANDLE_VALUE) {
            CloseHandle(g_logFile);
            g_logFile = INVALID_HANDLE_VALUE;
        }
    }

    return TRUE;
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12CreateDevice(IUnknown* adapter, D3D_FEATURE_LEVEL minimumFeatureLevel,
                                                                   REFIID riid, void** device) {
    char guid[64]{};
    GuidToString(riid, guid, sizeof(guid));
    Log("D3D12CreateDevice adapter=%p minimumFeatureLevel=0x%04x riid=%s ppDevice=%p",
        adapter,
        static_cast<unsigned>(minimumFeatureLevel),
        guid,
        device);

    using Fn = HRESULT(WINAPI*)(IUnknown*, D3D_FEATURE_LEVEL, REFIID, void**);
    Fn fn = GetRealProcAs<Fn>("D3D12CreateDevice");
    HRESULT hr = fn != nullptr ? fn(adapter, minimumFeatureLevel, riid, device) : MissingExport("D3D12CreateDevice");
    Log("D3D12CreateDevice hr=0x%08x device=%p", static_cast<unsigned>(hr), device != nullptr ? *device : nullptr);
    return hr;
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12GetDebugInterface(REFIID riid, void** debug) {
    char guid[64]{};
    GuidToString(riid, guid, sizeof(guid));
    Log("D3D12GetDebugInterface riid=%s", guid);

    using Fn = HRESULT(WINAPI*)(REFIID, void**);
    Fn fn = GetRealProcAs<Fn>("D3D12GetDebugInterface");
    return fn != nullptr ? fn(riid, debug) : MissingExport("D3D12GetDebugInterface");
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12EnableExperimentalFeatures(UINT numFeatures, const IID* featureIIDs,
                                                                                 void* configurationStructs,
                                                                                 UINT* configurationStructSizes) {
    Log("D3D12EnableExperimentalFeatures numFeatures=%u", numFeatures);

    using Fn = HRESULT(WINAPI*)(UINT, const IID*, void*, UINT*);
    Fn fn = GetRealProcAs<Fn>("D3D12EnableExperimentalFeatures");
    return fn != nullptr ? fn(numFeatures, featureIIDs, configurationStructs, configurationStructSizes)
                         : MissingExport("D3D12EnableExperimentalFeatures");
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12SerializeRootSignature(const D3D12_ROOT_SIGNATURE_DESC* desc,
                                                                             D3D_ROOT_SIGNATURE_VERSION version,
                                                                             ID3DBlob** blob,
                                                                             ID3DBlob** errorBlob) {
    using Fn = HRESULT(WINAPI*)(const D3D12_ROOT_SIGNATURE_DESC*, D3D_ROOT_SIGNATURE_VERSION, ID3DBlob**, ID3DBlob**);
    Fn fn = GetRealProcAs<Fn>("D3D12SerializeRootSignature");
    return fn != nullptr ? fn(desc, version, blob, errorBlob) : MissingExport("D3D12SerializeRootSignature");
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12CreateRootSignatureDeserializer(const void* srcData,
                                                                                      SIZE_T srcDataSizeInBytes,
                                                                                      REFIID riid,
                                                                                      void** deserializer) {
    using Fn = HRESULT(WINAPI*)(const void*, SIZE_T, REFIID, void**);
    Fn fn = GetRealProcAs<Fn>("D3D12CreateRootSignatureDeserializer");
    return fn != nullptr ? fn(srcData, srcDataSizeInBytes, riid, deserializer) : MissingExport("D3D12CreateRootSignatureDeserializer");
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12SerializeVersionedRootSignature(const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* desc,
                                                                                      ID3DBlob** blob,
                                                                                      ID3DBlob** errorBlob) {
    using Fn = HRESULT(WINAPI*)(const D3D12_VERSIONED_ROOT_SIGNATURE_DESC*, ID3DBlob**, ID3DBlob**);
    Fn fn = GetRealProcAs<Fn>("D3D12SerializeVersionedRootSignature");
    return fn != nullptr ? fn(desc, blob, errorBlob) : MissingExport("D3D12SerializeVersionedRootSignature");
}

extern "C" __declspec(dllexport) HRESULT WINAPI D3D12CreateVersionedRootSignatureDeserializer(const void* srcData,
                                                                                               SIZE_T srcDataSizeInBytes,
                                                                                               REFIID riid,
                                                                                               void** deserializer) {
    using Fn = HRESULT(WINAPI*)(const void*, SIZE_T, REFIID, void**);
    Fn fn = GetRealProcAs<Fn>("D3D12CreateVersionedRootSignatureDeserializer");
    return fn != nullptr ? fn(srcData, srcDataSizeInBytes, riid, deserializer)
                         : MissingExport("D3D12CreateVersionedRootSignatureDeserializer");
}
