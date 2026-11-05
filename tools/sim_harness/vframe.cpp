// vframe.exe VIDEO OUT.jpg [SECONDS] -- one decoded frame of a clip (a simxr.py recording) as a JPEG, and the
// clip's size, frame rate and duration on stdout. Media Foundation decodes (the HEVC/H.264 the recorder wrote),
// WIC writes the JPEG. Build: tools/sim_harness/build_vframe.bat.
#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <propvarutil.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
#include <vector>

#pragma comment(lib, "mfplat.lib")
#pragma comment(lib, "mfreadwrite.lib")
#pragma comment(lib, "mfuuid.lib")
#pragma comment(lib, "windowscodecs.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "propsys.lib")

using Microsoft::WRL::ComPtr;

static int fail(const char* what, HRESULT hr) {
    fprintf(stderr, "%s failed (hr=0x%08X)\n", what, (unsigned)hr);
    return 1;
}

// Every Media Foundation object lives in here, so all of them are gone before MFShutdown.
static int run(int argc, wchar_t** argv) {
    const double at = argc > 3 ? _wtof(argv[3]) : 0.0;

    ComPtr<IMFAttributes> attr;
    MFCreateAttributes(&attr, 2);
    attr->SetUINT32(MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, TRUE);   // RGB32 out of whatever the codec gives
    ComPtr<IMFSourceReader> reader;
    HRESULT hr = MFCreateSourceReaderFromURL(argv[1], attr.Get(), &reader);
    if (FAILED(hr)) return fail("open", hr);

    ComPtr<IMFMediaType> native;
    reader->GetNativeMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, &native);
    GUID sub{};
    if (native) native->GetGUID(MF_MT_SUBTYPE, &sub);
    UINT32 nw = 0, nh = 0, fn = 0, fd = 1;
    if (native) {
        MFGetAttributeSize(native.Get(), MF_MT_FRAME_SIZE, &nw, &nh);
        MFGetAttributeRatio(native.Get(), MF_MT_FRAME_RATE, &fn, &fd);
    }
    PROPVARIANT dur;
    PropVariantInit(&dur);
    reader->GetPresentationAttribute((DWORD)MF_SOURCE_READER_MEDIASOURCE, MF_PD_DURATION, &dur);
    const double seconds = dur.vt == VT_UI8 ? dur.uhVal.QuadPart / 1e7 : -1.0;
    const char* codec = IsEqualGUID(sub, MFVideoFormat_HEVC) ? "hevc" : IsEqualGUID(sub, MFVideoFormat_H264) ? "h264" : "other";
    printf("codec %s size %ux%u fps %.3f duration %.3f s\n", codec, nw, nh, fd ? (double)fn / fd : 0.0, seconds);

    ComPtr<IMFMediaType> want;
    MFCreateMediaType(&want);
    want->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    want->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_RGB32);
    hr = reader->SetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, nullptr, want.Get());
    if (FAILED(hr)) return fail("RGB32 output", hr);
    ComPtr<IMFMediaType> cur;
    reader->GetCurrentMediaType((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, &cur);
    UINT32 w = 0, h = 0;
    MFGetAttributeSize(cur.Get(), MF_MT_FRAME_SIZE, &w, &h);
    LONG stride = (LONG)(w * 4);
    UINT32 st = 0;
    if (SUCCEEDED(cur->GetUINT32(MF_MT_DEFAULT_STRIDE, &st))) stride = (LONG)st;

    if (at > 0) {
        PROPVARIANT pos;
        InitPropVariantFromInt64((LONGLONG)(at * 1e7), &pos);
        reader->SetCurrentPosition(GUID_NULL, pos);
        PropVariantClear(&pos);
    }
    ComPtr<IMFSample> sample;
    LONGLONG ts = 0;
    for (int i = 0; i < 600 && !sample; ++i) {
        DWORD flags = 0;
        hr = reader->ReadSample((DWORD)MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, nullptr, &flags, &ts, &sample);
        if (FAILED(hr)) return fail("decode", hr);
        if (flags & MF_SOURCE_READERF_ENDOFSTREAM) break;
        if (sample && ts < (LONGLONG)(at * 1e7) - 400000) sample.Reset();   // seek lands on a key frame before
    }
    if (!sample) return fail("no frame there", E_FAIL);
    ComPtr<IMFMediaBuffer> buf;
    sample->ConvertToContiguousBuffer(&buf);
    BYTE* data = nullptr;
    DWORD len = 0;
    buf->Lock(&data, nullptr, &len);

    std::vector<BYTE> rgb((size_t)w * h * 3);
    for (UINT32 y = 0; y < h; ++y) {
        const BYTE* src = stride > 0 ? data + (size_t)y * stride : data + (size_t)(h - 1 - y) * (-stride);
        BYTE* dst = rgb.data() + (size_t)y * w * 3;
        for (UINT32 x = 0; x < w; ++x) { dst[3 * x] = src[4 * x]; dst[3 * x + 1] = src[4 * x + 1]; dst[3 * x + 2] = src[4 * x + 2]; }
    }
    buf->Unlock();

    ComPtr<IWICImagingFactory> f;
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&f));
    ComPtr<IWICStream> out;
    f->CreateStream(&out);
    if (FAILED(out->InitializeFromFilename(argv[2], GENERIC_WRITE))) return fail("output file", E_FAIL);
    ComPtr<IWICBitmapEncoder> enc;
    f->CreateEncoder(GUID_ContainerFormatJpeg, nullptr, &enc);
    enc->Initialize(out.Get(), WICBitmapEncoderNoCache);
    ComPtr<IWICBitmapFrameEncode> fr;
    ComPtr<IPropertyBag2> props;
    enc->CreateNewFrame(&fr, &props);
    fr->Initialize(props.Get());
    fr->SetSize(w, h);
    WICPixelFormatGUID pf = GUID_WICPixelFormat24bppBGR;
    fr->SetPixelFormat(&pf);
    fr->WritePixels(h, w * 3, (UINT)rgb.size(), rgb.data());
    fr->Commit();
    enc->Commit();
    printf("frame at %.3f s -> %ls (%ux%u)\n", ts / 1e7, argv[2], w, h);
    return 0;
}

int wmain(int argc, wchar_t** argv) {
    if (argc < 3) {
        fprintf(stderr, "vframe VIDEO OUT.jpg [SECONDS]\n");
        return 2;
    }
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    MFStartup(MF_VERSION);
    const int r = run(argc, argv);
    fflush(stdout);
    MFShutdown();
    CoUninitialize();
    return r;
}
