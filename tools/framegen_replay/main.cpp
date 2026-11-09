// Reuse the production-generator GPU fixture; no live game, OpenXR session or registry writes.
#define main framegen_fixture_main
#include "../flight_recorder_tests/synthesis_generator.cpp"
#undef main
#include "Framegen/DlssFrameGen.hpp"
namespace cvr::framegen { const char* BackendName(Backend b){return b==Backend::Nvidia?"NVIDIA":b==Backend::DlssG?"DLSS-G":"FidelityFX";} }
#include "../framegen_tests/replay.cpp"
#include "v557_checks.hpp"
int main(int argc,char** argv)try{
    if(argc==2 && std::string(argv[1])=="--v557-test"){V557Checks();return 0;}
    Check(argc==3,"framegen_replay <capture directory> <output directory>");
    targeted=false;Gpu gpu(true);ReplayGpu r;
    r.device=gpu.device.Get();r.queue=gpu.queue.Get();
    r.texture=[&](unsigned w,unsigned h,DXGI_FORMAT f,D3D12_RESOURCE_STATES s){return gpu.Texture(w,h,f,s);};
    r.upload=[&](ID3D12Resource* t,const void* d,unsigned p,D3D12_RESOURCE_STATES s){gpu.Upload(t,d,p,s);};
    r.read=[&](ID3D12Resource* t){return gpu.Read(t);};r.wait=[&]{gpu.Wait();};
    return Replay(r,argv[1],argv[2]);
}catch(const std::exception& e){std::fprintf(stderr,"Replay failed: %s\n",e.what());return 1;}
