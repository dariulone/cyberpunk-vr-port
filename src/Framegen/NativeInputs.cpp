#include "Framegen/NativeInputs.hpp"
#include "Framegen/InputCopy.hpp"
#include "Framegen/Framegen.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Render/RenderProbeScope.hpp"
#include <atomic>

extern void Log(const char*,...);
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FramegenNativeStages[4]{};
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FramegenNativeDebug[2][8]{};
// Which engine node the last native capture came from: 1 TAA, 2 FSR 2, 3 FSR 3.0, 4 FSR 3.1+, 5 XeSS.
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_FramegenNativeSource{};
namespace cvr::framegen {
namespace {
// The engine nodes whose depth and motion framegen reuses when Streamline is not the upscaler. TAA
// keeps the two SRVs it was proven on. Every temporal upscaler node is handed the same two view-salted
// resources instead -- depth (view<<24 ^ 0x61F178D4) and velocity (view<<24 ^ 0x15EAB19C), the ones
// sub_141D4E150 passes to FSR 3 as "depth" and "motionVectors" -- so one observer covers FSR 2 (the
// engine's own passes), FSR 3.0, FSR 3.1/4 and XeSS. The last three are libraries that move the
// inputs into their own states, so they are captured when the library is called
// (CaptureUpscalerInputs), not at the next draw or dispatch.
enum class Consumer:uint32_t {None,Taa,Fsr2,Fsr3,FfxApi,Xess};
Consumer CurrentConsumer() {
    if(!CaptureInputsEnabled() || cvr::stereo::probe::internalCommands || !cvr::detail::g_exe_base)return Consumer::None;
    switch(cvr::detail::t_current_node_work-reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base)) {
        case 0x557204:return Consumer::Taa;     // CRenderNode_ApplyTXAA
        case 0x41CE60:return Consumer::Fsr2;    // CRenderNode_ApplyFSR2
        // CRenderNode_ApplyFSR3_0 (2.31: 0x1D4E8B0, ffx_fsr3_x64.dll) is gone in 2.5 with the ffx_* DLLs;
        // FSR 3.x now runs through ApplyFSR3 and amd_fidelityfx_loader_dx12.dll.
        case 0x22873B0:return Consumer::FfxApi; // CRenderNode_ApplyFSR3, amd_fidelityfx_dx12.dll
        case 0xD6C0C0:return Consumer::Xess;    // CRenderNode_ApplyXESS
        default:return Consumer::None;
    }
}
bool Library(Consumer c) {return c==Consumer::Fsr3 || c==Consumer::FfxApi || c==Consumer::Xess;}
int InputType(Consumer c,uint32_t key) {
    // ApplyTXAA's contiguous SRVs: current color, history, velocity (917D8BC9),
    // resolved depth (DEBF0C27), mask. The raw G-buffer velocity is not sampled.
    if(c==Consumer::Taa)return key==0xBF0C27?0:key==0x7D8BC9?1:-1;
    return key==0xF178D4?0:key==0xEAB19C?1:-1;
}
// The view a node renders, at work context +0x18 (as every node reads it).
const void* ViewOf(const void* workContext) {
    __try {return *reinterpret_cast<void* const*>(static_cast<const uint8_t*>(workContext)+0x18);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return nullptr;}
}
struct Candidate { uint32_t frame{},view{},handles[2]{};uint64_t pose{};uintptr_t node{};bool copied{}; };
thread_local Candidate candidate;
// The resource behind a physical handle, as a framegen input in `state`.
bool Describe(uintptr_t pool,uint32_t handle,uint32_t type,uint32_t state,InputTag& tag) {
    auto& debug=CyberpunkVR_FramegenNativeDebug[type];
    debug[4]=state;
    auto* resource=*reinterpret_cast<ID3D12Resource**>(pool+0x2F1D8+uintptr_t(handle-1)*0xB0);
    debug[5]=reinterpret_cast<uintptr_t>(resource);
    if(!resource)return false;
    const auto d=resource->GetDesc();
    debug[6]=d.Format;debug[7]=(d.Width<<32)|d.Height;
    if(d.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D || !d.Width || !d.Height ||
        d.Width>16384 || d.Height>16384 || d.MipLevels!=1 || d.DepthOrArraySize!=1)return false;
    tag={resource,state,type,0,0,uint32_t(d.Width),d.Height};return true;
}
// `unused`: the state to assume when this command context has not touched the resource yet, so the
// engine's ledger has no entry for it. Zero refuses such a resource.
bool ReadTag(uint32_t handle,uint32_t type,InputTag& tag,uint32_t unused=0) {
    auto& debug=CyberpunkVR_FramegenNativeDebug[type];debug[0]=handle;
    // 2.31 resource table and command-context state ledger, the same records
    // consumed by native SRV binding +1FABD0 and barrier tracking +1F41F8.
    // No remembered D3D pointer is used; the only assumed state is a library's input state, for a
    // resource this context has not touched (Capture).
    __try {
        const auto base=reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base);
        using Current=uintptr_t(*)();
        const auto context=reinterpret_cast<Current>(base+0x337230)();
        debug[1]=context;
        const auto pool=*reinterpret_cast<const uintptr_t*>(base+0x3BCDCA8);
        if(!context || !pool || !handle || handle>0x100000)return false;
        const auto table=context+0x538;
        const auto count=*reinterpret_cast<const uint32_t*>(table+8);
        const auto buckets=*reinterpret_cast<const uint32_t*>(table+12);
        const auto stride=*reinterpret_cast<const uint32_t*>(table+28);
        debug[2]=count;debug[3]=(uint64_t(buckets)<<32)|stride;
        if(!count)return unused && Describe(pool,handle,type,unused,tag);
        if(!buckets || buckets>0x100000 || stride<20 || stride>128)return false;
        const auto heads=*reinterpret_cast<const uintptr_t*>(table);
        const auto entries=*reinterpret_cast<const uintptr_t*>(table+16);
        if(!heads || !entries)return false;
        int32_t index=*reinterpret_cast<const int32_t*>(heads+4*(handle%buckets));
        for(unsigned visited=0;index>=0 && index<0x100000 && visited<1024;++visited) {
            const auto entry=entries+uintptr_t(index)*stride;
            if(*reinterpret_cast<const uint32_t*>(entry+4)==handle && *reinterpret_cast<const uint32_t*>(entry+8)==handle) {
                uint32_t state=*reinterpret_cast<const uint32_t*>(entry+16);
                if(state==0x80000000) {
                    const auto first=*reinterpret_cast<const uint16_t*>(entry+12);
                    const auto states=*reinterpret_cast<const uintptr_t*>(table+96);
                    if(first==0xFFFF || !states || !*reinterpret_cast<const uint16_t*>(entry+14))return false;
                    state=*reinterpret_cast<const uint32_t*>(states+4*first);
                }
                constexpr uint32_t reads=D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_DEPTH_READ;
                debug[4]=state;
                if(!(state&(reads&~D3D12_RESOURCE_STATE_DEPTH_READ)) || (state&~reads))return false;
                return Describe(pool,handle,type,state,tag);
            }
            index=*reinterpret_cast<const int32_t*>(entry);
        }
        if(unused)return Describe(pool,handle,type,unused,tag);
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
    return false;
}
}
void ObserveNativeInput(uint32_t logical,uint32_t physical) {
    const auto consumer=CurrentConsumer();
    if(consumer==Consumer::None)return;
    const int type=InputType(consumer,logical&0xFFFFFFu);
    if(type<0)return;
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || !pose.hasRenderFrameId || pose.view<1 || pose.view>2)return;
    const auto node=cvr::detail::t_current_node_work;
    if(candidate.frame!=pose.renderFrameId || candidate.view!=pose.view || candidate.pose!=pose.poseId || candidate.node!=node)
        candidate={pose.renderFrameId,pose.view,{},pose.poseId,node,false};
    candidate.handles[type]=physical;++CyberpunkVR_FramegenNativeStages[0];
}
void ObserveNativeView(const void* workContext) {
    if(CurrentConsumer()==Consumer::None || !workContext)return;
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || !pose.hasRenderFrameId || pose.view<1 || pose.view>2)return;
    if(const void* view=ViewOf(workContext))RecordViewCamera(view,pose.view);
}
namespace {
void Capture(ID3D12GraphicsCommandList* list,bool library) {
    const auto consumer=CurrentConsumer();
    if(consumer==Consumer::None || Library(consumer)!=library)return;
    if(candidate.copied || !candidate.handles[0] || (MotionWanted() && !candidate.handles[1]) || candidate.node!=cvr::detail::t_current_node_work)return;
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || !pose.hasRenderFrameId || candidate.frame!=pose.renderFrameId || candidate.view!=pose.view || candidate.pose!=pose.poseId)return;
    ++CyberpunkVR_FramegenNativeStages[1];InputTag tags[2]{};
    // A library is promised its inputs readable by pixel and compute shaders: the engine hands FSR 3
    // FFX state 12, PIXEL_COMPUTE_READ (sub_141D4F220). An input this command context has not touched
    // yet is in that state from an earlier context.
    const uint32_t unused=library ? D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE|D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE : 0;
    if(!ReadTag(candidate.handles[0],0,tags[0],unused) || (MotionWanted() && !ReadTag(candidate.handles[1],1,tags[1],unused))) {
        ++CyberpunkVR_FramegenNativeStages[2];return;
    }
    cvr::stereo::probe::Scope internal;
    if(!RecordNativeInputs(tags,list)) {++CyberpunkVR_FramegenNativeStages[2];return;}
    candidate.copied=true;++CyberpunkVR_FramegenNativeStages[3];
    CyberpunkVR_FramegenNativeSource.store(uint32_t(consumer),std::memory_order_relaxed);
    static std::atomic<unsigned> logs{};
    if(logs.fetch_add(1)<4)Log("[framegen-native] source=%u view=%u frame=%u depth=%ux%u state=0x%X mv=%ux%u state=0x%X\n",
        unsigned(consumer),pose.view,pose.renderFrameId,tags[0].width,tags[0].height,tags[0].state,tags[1].width,tags[1].height,tags[1].state);
}
}
void CaptureNativeInputs(ID3D12GraphicsCommandList* list) {Capture(list,false);}
void CaptureUpscalerInputs(ID3D12GraphicsCommandList* list) {Capture(list,true);}
}
