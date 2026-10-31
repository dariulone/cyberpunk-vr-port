#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <openxr/openxr_platform.h>
#include "Framegen/OfxrBridge.hpp"
#include "Framegen/Framegen.hpp"
#include "xrfg/provider_api.hpp"
#include <filesystem>
#include <cstdio>
#include <cstdarg>
#include <stdexcept>
#include <fstream>
#include <sstream>
namespace {cvr::framegen::Settings settings;}
extern "C" {int CyberpunkVR_RuntimeDiagnostics=0;}
namespace cvr::framegen {
void WakeHardwareTelemetry(){}
void StopHardwareTelemetry(){}
HardwareStatistics GetHardwareStatistics(){return {};}
}
void Log(const char* fmt,...) {va_list args;va_start(args,fmt);vprintf(fmt,args);va_end(args);}
void Check(bool value,const char* message){if(!value)throw std::runtime_error(message);}
int main(int argc,char**) {
 try {
    wchar_t path[MAX_PATH]{};GetModuleFileNameW(nullptr,path,MAX_PATH);
    auto root=std::filesystem::path(path).parent_path();
    auto runtime=root/L"runtime.json";
    {std::ofstream f(runtime);f << "{\"file_format_version\":\"1.0.0\",\"runtime\":{\"library_path\":\"" << (root/L"ofxr_test_loader.dll").generic_string() << "\"}}";}
    SetEnvironmentVariableW(L"XR_RUNTIME_JSON",runtime.c_str());
    SetEnvironmentVariableW(L"CHEEKY_OPENXR_LAYER_DISABLE",L"1");
    SetEnvironmentVariableW(L"XR_API_LAYER_PATH",L"C:/nonexistent-ofxr-test-path");
    {std::ofstream f(root/L"ofxr-v557/observer.json");
     f << "{\"file_format_version\":\"1.0.0\",\"api_layer\":{\"name\":\"XR_APILAYER_test_observer\",\"library_path\":\"" << (root/L"ofxr_observer.dll").generic_string() << "\",\"api_version\":\"1.0\",\"implementation_version\":\"1\",\"description\":\"test observer\"}}";}
    if(argc>1) {
        std::filesystem::copy_file(root/L"ofxr_test_loader.dll",root/L"XR_APILAYER_XRFrameBridge_test.dll",std::filesystem::copy_options::overwrite_existing);
        Check(LoadLibraryW((root/L"XR_APILAYER_XRFrameBridge_test.dll").c_str())!=nullptr,"external fixture load");
    }
    settings.enabled=true;settings.backend=cvr::framegen::Backend::Nvidia;settings.quality=2;settings.flowScale=50;
    cvr::framegen::SetSettings(settings);
    Check(!cvr::framegen::Enabled(),"native generator enabled alongside V557 selection");
    Check(cvr::framegen::ParseSetting("xr_framegen_logging=0",settings) && !settings.ofxrLogging,"logging parse off");
    Check(cvr::framegen::ParseSetting("xr_framegen_logging=1",settings) && settings.ofxrLogging,"logging parse on");
    FILE* saved=tmpfile();Check(saved!=nullptr,"settings temp file");cvr::framegen::WriteSettings(saved,settings);rewind(saved);
    char line[512]{};bool persisted=false;while(fgets(line,sizeof(line),saved))persisted|=std::string(line)=="xr_framegen_logging=1\n";
    fclose(saved);Check(persisted,"logging setting not saved");
    for(int pass=0;pass<2;++pass) {
        XrInstanceCreateInfo ci{XR_TYPE_INSTANCE_CREATE_INFO};ci.applicationInfo.apiVersion=XR_MAKE_VERSION(1,0,0);strcpy_s(ci.applicationInfo.applicationName,"OFXR integration test");
        const char* observerName="XR_APILAYER_test_observer";
        if(argc==1) {ci.enabledApiLayerCount=1;ci.enabledApiLayerNames=&observerName;}
        XrInstance instance{};Check(XR_SUCCEEDED(xrCreateInstance(&ci,&instance)),"private create failed");
        wchar_t search[256]{};GetEnvironmentVariableW(L"XR_API_LAYER_PATH",search,256);
        Check(std::wstring(search)==L"C:/nonexistent-ofxr-test-path","layer search environment leaked");
        XrInstanceProperties props{XR_TYPE_INSTANCE_PROPERTIES};Check(XR_SUCCEEDED(xrGetInstanceProperties(instance,&props)),"core dispatch failed");
        PFN_xrVoidFunction ext{};Check(XR_SUCCEEDED(xrGetInstanceProcAddr(instance,"xrGetD3D12GraphicsRequirementsKHR",&ext))&&ext,"extension forwarding failed");
        PFN_xrVoidFunction unknown=reinterpret_cast<PFN_xrVoidFunction>(1);Check(XR_FAILED(xrGetInstanceProcAddr(instance,"xrUnknownTest",&unknown)),"unsupported extension accepted");
        if(argc>1)Check(std::string(cvr::ofxr::Status().text).find("External OFXR")!=std::string::npos,"external guard missing");
        else {
            auto observer=GetModuleHandleW(L"ofxr_observer.dll");
            auto count=reinterpret_cast<unsigned(*)()>(GetProcAddress(observer,"ObserverCalls"));
            Check(count && count()==1,"OFXR internal runtime query leaked through upper observer layer");
            Check(cvr::ofxr::RecordingStatus()==1,"recorder requested on but not writing");
            auto module=GetModuleHandleW(L"CyberpunkVR_OFXR.dll");Check(module!=nullptr,"private backend absent");
            auto snap=reinterpret_cast<int(*)(OFXR_ControlSnapshotV1*) noexcept>(GetProcAddress(module,"OFXR_GetControlSnapshotV1"));
            auto recording=reinterpret_cast<int(*)() noexcept>(GetProcAddress(module,"OFXR_GetLoggingSettingV1"));
            Check(snap&&recording,"control ABI absent");OFXR_ControlSnapshotV1 s{};Check(snap(&s)!=0,"snapshot failed");
            Check(s.desired.enabled && s.desired.backend==1 && s.desired.preset==0 && s.desired.scale==2 && !s.desired.backward,"quality mapping differs from V557 baseline");
            settings.ofxrLogging=false;cvr::framegen::SetSettings(settings);Check(recording()==0,"F10 recording off failed");
            settings.ofxrLogging=true;cvr::framegen::SetSettings(settings);Check(recording()==1,"F10 recording on failed");
            settings.enabled=false;cvr::framegen::SetSettings(settings);snap(&s);Check(!s.desired.enabled,"generation disable failed");
            settings.enabled=true;cvr::framegen::SetSettings(settings);
            settings.backend=cvr::framegen::Backend::DlssG;cvr::framegen::SetSettings(settings);
            Check(!cvr::framegen::Enabled(),"native generation overlapped attached V557");
            settings.backend=cvr::framegen::Backend::Nvidia;cvr::framegen::SetSettings(settings);
        }
        Check(XR_SUCCEEDED(xrDestroyInstance(instance)),"destroy failed");
    }
    if(argc==1) {
        bool receipt=false;
        for(auto& e:std::filesystem::directory_iterator(root/L"ofxr-v557")) {
            if(e.path().extension()==L".log") {std::ifstream f(e.path());std::stringstream s;s<<f.rdbuf();receipt|=s.str().find("op=logger result=557")!=std::string::npos;}
        }
        Check(receipt,"flight recorder produced no V557 receipt");
    }
    puts("Private V557 dispatch/recreate, controls, logging and exclusion checks passed");return 0;
 } catch(const std::exception& e){fprintf(stderr,"FAILED: %s\n",e.what());return 1;}
}
