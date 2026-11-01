// The game's window as it is presented, frame after frame: Windows.Graphics.Capture on the process's visible
// top-level window -- a D3D12 flip-model swapchain, which PrintWindow reads as black, and a window other windows may
// cover. Each grab is written as a 32-bit BMP named by its time since the start (ms).
//
//   window_grab.exe PID OUT_DIR [seconds=60] [interval_ms=1000] [stop_file]
#include <windows.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <windows.graphics.capture.interop.h>
#include <windows.graphics.directx.direct3d11.interop.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>

#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

#pragma comment(lib, "user32.lib")
#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "windowsapp.lib")

namespace wgc = winrt::Windows::Graphics::Capture;
namespace wd3d = winrt::Windows::Graphics::DirectX::Direct3D11;

namespace {

struct Find {
    DWORD pid;
    HWND hwnd = nullptr;
    LONG area = 0;
};

// The process's largest visible top-level window.
BOOL CALLBACK Collect(HWND hwnd, LPARAM param) {
    auto* f = reinterpret_cast<Find*>(param);
    DWORD pid = 0;
    GetWindowThreadProcessId(hwnd, &pid);
    RECT r{};
    if (pid != f->pid || !IsWindowVisible(hwnd) || !GetWindowRect(hwnd, &r)) return TRUE;
    const LONG area = (r.right - r.left) * (r.bottom - r.top);
    if (area > f->area) {
        f->area = area;
        f->hwnd = hwnd;
    }
    return TRUE;
}

bool WriteBmp(const std::wstring& path, const uint8_t* bgra, UINT width, UINT height, UINT pitch) {
    FILE* f = _wfopen(path.c_str(), L"wb");
    if (!f) return false;
    BITMAPFILEHEADER fh{};
    BITMAPINFOHEADER ih{};
    ih.biSize = sizeof(ih);
    ih.biWidth = LONG(width);
    ih.biHeight = -LONG(height);  // top-down
    ih.biPlanes = 1;
    ih.biBitCount = 32;
    ih.biCompression = BI_RGB;
    ih.biSizeImage = width * height * 4;
    fh.bfType = 0x4D42;
    fh.bfOffBits = sizeof(fh) + sizeof(ih);
    fh.bfSize = fh.bfOffBits + ih.biSizeImage;
    fwrite(&fh, sizeof(fh), 1, f);
    fwrite(&ih, sizeof(ih), 1, f);
    for (UINT y = 0; y < height; ++y) fwrite(bgra + size_t(y) * pitch, 4, width, f);
    fclose(f);
    return true;
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
    if (argc < 3) {
        std::fwprintf(stderr, L"usage: window_grab PID OUT_DIR [seconds] [interval_ms] [stop_file]\n");
        return 2;
    }
    winrt::init_apartment(winrt::apartment_type::multi_threaded);
    Find find{DWORD(_wtoi(argv[1]))};
    const std::wstring out = argv[2];
    const double seconds = argc > 3 ? _wtof(argv[3]) : 60.0;
    const DWORD interval = argc > 4 ? DWORD(_wtoi(argv[4])) : 1000;
    const wchar_t* stop = argc > 5 ? argv[5] : nullptr;
    CreateDirectoryW(out.c_str(), nullptr);

    winrt::com_ptr<ID3D11Device> device;
    winrt::com_ptr<ID3D11DeviceContext> context;
    if (FAILED(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT, nullptr, 0,
                                 D3D11_SDK_VERSION, device.put(), nullptr, context.put()))) {
        std::fwprintf(stderr, L"no D3D11 device\n");
        return 3;
    }
    auto dxgi = device.as<IDXGIDevice>();
    winrt::com_ptr<::IInspectable> inspectable;
    winrt::check_hresult(CreateDirect3D11DeviceFromDXGIDevice(dxgi.get(), inspectable.put()));
    const auto rtDevice = inspectable.as<wd3d::IDirect3DDevice>();

    const ULONGLONG start = GetTickCount64();
    wgc::GraphicsCaptureItem item{nullptr};
    wgc::Direct3D11CaptureFramePool pool{nullptr};
    wgc::GraphicsCaptureSession session{nullptr};
    winrt::Windows::Graphics::SizeInt32 size{};
    int saved = 0;
    while (GetTickCount64() - start < ULONGLONG(seconds * 1000.0)) {
        if (stop && GetFileAttributesW(stop) != INVALID_FILE_ATTRIBUTES) break;
        if (!session) {
            // The window may not be there yet, or be replaced (the launcher's, then the game's).
            find.hwnd = nullptr;
            find.area = 0;
            EnumWindows(Collect, reinterpret_cast<LPARAM>(&find));
            if (find.hwnd) {
                auto interop = winrt::get_activation_factory<wgc::GraphicsCaptureItem, IGraphicsCaptureItemInterop>();
                if (SUCCEEDED(interop->CreateForWindow(find.hwnd, winrt::guid_of<wgc::GraphicsCaptureItem>(),
                                                       winrt::put_abi(item)))) {
                    size = item.Size();
                    pool = wgc::Direct3D11CaptureFramePool::CreateFreeThreaded(
                        rtDevice, winrt::Windows::Graphics::DirectX::DirectXPixelFormat::B8G8R8A8UIntNormalized, 2,
                        size);
                    session = pool.CreateCaptureSession(item);
                    session.IsCursorCaptureEnabled(false);
                    session.StartCapture();
                }
            }
            if (!session) {
                Sleep(250);
                continue;
            }
        }
        Sleep(interval);
        // Another window became the process's largest (the launcher's gone, the game's up): capture that one.
        Find now{find.pid};
        EnumWindows(Collect, reinterpret_cast<LPARAM>(&now));
        if (now.hwnd != find.hwnd) {
            session.Close();
            pool.Close();
            session = nullptr;
            pool = nullptr;
            continue;
        }
        wgc::Direct3D11CaptureFrame frame{nullptr};
        for (auto f = pool.TryGetNextFrame(); f; f = pool.TryGetNextFrame()) frame = f;  // the newest one
        if (!frame) continue;
        auto access = frame.Surface().as<::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
        winrt::com_ptr<ID3D11Texture2D> texture;
        winrt::check_hresult(access->GetInterface(winrt::guid_of<ID3D11Texture2D>(), texture.put_void()));
        D3D11_TEXTURE2D_DESC desc{};
        texture->GetDesc(&desc);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        desc.MiscFlags = 0;
        winrt::com_ptr<ID3D11Texture2D> staging;
        if (FAILED(device->CreateTexture2D(&desc, nullptr, staging.put()))) continue;
        context->CopyResource(staging.get(), texture.get());
        D3D11_MAPPED_SUBRESOURCE mapped{};
        if (FAILED(context->Map(staging.get(), 0, D3D11_MAP_READ, 0, &mapped))) continue;
        wchar_t name[64];
        swprintf_s(name, L"\\%06llu.bmp", GetTickCount64() - start);
        if (WriteBmp(out + name, static_cast<const uint8_t*>(mapped.pData), desc.Width, desc.Height, mapped.RowPitch))
            ++saved;
        context->Unmap(staging.get(), 0);
        if (frame.ContentSize().Width != size.Width || frame.ContentSize().Height != size.Height) {
            size = frame.ContentSize();
            pool.Recreate(rtDevice, winrt::Windows::Graphics::DirectX::DirectXPixelFormat::B8G8R8A8UIntNormalized, 2,
                          size);
        }
    }
    if (session) session.Close();
    if (pool) pool.Close();
    std::wprintf(L"%d frames\n", saved);
    return 0;
}
