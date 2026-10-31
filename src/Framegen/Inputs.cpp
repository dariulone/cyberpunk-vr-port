#include "Utils/DebugGate.hpp"
#include "Framegen/Inputs.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include "Framegen/InputCopy.hpp"
#include "Framegen/Framegen.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Render/ViewCameraLayout.hpp"
#include "Render/PhysicalOpticsShaders.hpp"
#include "Stereo/EngineStereoDlss.hpp"
#include "Stereo/EngineStereo.hpp"
#include <MinHook.h>
#include <sl_core_types.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <mutex>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);
extern "C" int32_t CyberpunkVR_VrcamDlssViewport;
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_FramegenInputStages[10]{};
// The view camera against Streamline's for the same eye and frame, when both exist (NVIDIA): largest
// difference in position (m), axes, near/far, vertical FOV (rad), jitter, motion scale.
extern "C" __declspec(dllexport) float CyberpunkVR_FramegenCameraDelta[2][6]{};
namespace cvr::framegen {
namespace {
using Microsoft::WRL::ComPtr;
struct Constants {CameraData camera{};uint32_t frame{};bool valid{};uint64_t pose{},origin{};};
struct Record {
    Inputs data;
    uint64_t generation{};
    uint8_t expected{},submitted{};
    bool invalid{};
};
struct Link {std::weak_ptr<Record> record;uint64_t generation;uint8_t mask;};
std::mutex mutex;
std::array<Constants,8> cameras[2];
// The native consumers' camera (TAA and the upscaler nodes), read from the view itself.
std::array<Constants,8> viewCameras[2];
// Three GPU generation slots, the displayed endpoint, the newest capture and
// one producer slot. Allocation is lazy; completed generation leases retire
// every display cycle, including when no new matched input has arrived.
std::array<std::shared_ptr<Record>,6> pools[2];
std::unordered_map<ID3D12CommandList*,std::vector<Link>> pending;
std::atomic<bool> havePending{false};
uint64_t nextGeneration{};
std::atomic<bool> hooksInstalled{false};
std::atomic<bool> logged[2]{};
bool ValidCamera(const CameraData& c) {
    if(!c.cameraMotion || c.motion3d || !std::isfinite(c.nearPlane) || c.nearPlane<=0 ||
       !(c.farPlane>c.nearPlane) || !std::isfinite(c.verticalFov) || c.verticalFov<=0 || c.verticalFov>=3.13f ||
       !std::isfinite(c.aspect) || c.aspect<=0 ||
       !std::isfinite(c.motionScale[0]) || !std::isfinite(c.motionScale[1]) ||
       !std::isfinite(c.jitter[0]) || !std::isfinite(c.jitter[1]))return false;
    for(unsigned i=0;i<3;++i)if(!std::isfinite(c.position[i]) || !std::isfinite(c.up[i]) ||
        !std::isfinite(c.right[i]) || !std::isfinite(c.forward[i]))return false;
    return true;
}

// Engine stereo: MAIN's inputs are recorded per eye (ENGINE STEREO: ONE FRAME, TWO EYES below).
bool SplitEyes(const cvr::camera::PoseIdentity& pose) {
    return pose.view==1 && cvr::engine_stereo::Enabled() && cvr::engine_stereo::Rendering();
}
bool Camera(const sl::Constants* p,const sl::FrameToken* token,CameraData* out,uint32_t* frame) {
    __try {
        if(!p || !token || p->structType!=sl::Constants::s_structType || p->structVersion<1 || p->structVersion>2)return false;
        *frame=uint32_t(*token);
        CameraData c{};
        c.jitter[0]=p->jitterOffset.x;c.jitter[1]=p->jitterOffset.y;
        c.motionScale[0]=p->mvecScale.x;c.motionScale[1]=p->mvecScale.y;
        std::memcpy(c.position,&p->cameraPos,sizeof(c.position));std::memcpy(c.up,&p->cameraUp,sizeof(c.up));
        std::memcpy(c.right,&p->cameraRight,sizeof(c.right));std::memcpy(c.forward,&p->cameraFwd,sizeof(c.forward));
        c.nearPlane=p->cameraNear;c.farPlane=p->cameraFar;c.verticalFov=p->cameraFOV;c.aspect=p->cameraAspectRatio;
        const float projectionY=std::abs(p->cameraViewToClip[1].y);
        if(std::isfinite(projectionY) && projectionY>0.001f)c.verticalFov=2*std::atan(1/projectionY);
        c.inverted=p->depthInverted==sl::eTrue;c.infinite=std::isinf(c.farPlane);
        c.jittered=p->motionVectorsJittered==sl::eTrue;c.reset=p->reset==sl::eTrue;
        c.cameraMotion=p->cameraMotionIncluded==sl::eTrue;c.motion3d=p->motionVectors3D==sl::eTrue;
        if(p->depthInverted==sl::eInvalid || p->reset==sl::eInvalid ||
           p->orthographicProjection!=sl::eFalse || !ValidCamera(c))return false;
        *out=c;return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
using ConstantsFn=sl::Result (*)(const sl::Constants&,const sl::FrameToken&,const sl::ViewportHandle&);
ConstantsFn originalConstants{};
sl::Result SetConstants(const sl::Constants& input,const sl::FrameToken& frame,const sl::ViewportHandle& view) {
    cvr::engine_stereo::dlss::RecordConstants(input,frame,view);
    const auto result=originalConstants(input,frame,view);
    if(!CaptureInputsEnabled())return result;
    const auto viewport=uint32_t(view);
    const int side=viewport==uint32_t(CyberpunkVR_VrcamDlssViewport) ? 1 : (viewport==0 ? 0 : -1);
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(result==sl::Result::eOk && CaptureInputsEnabled() && side>=0 && side<2 && pose && pose.hasRenderFrameId && pose.view==unsigned(side+1)) {
        Constants value{};value.valid=Camera(&input,&frame,&value.camera,&value.frame);
        static std::atomic<uint32_t> diagnosticCount{};
        if(cvr::RuntimeDiagnosticsEnabled() && diagnosticCount.fetch_add(1)<4)Log("[framegen] constants viewport=%u valid=%d frame=%u version=%llu near=%.6f far=%.3f fov=%.5f py=%.5f motion=(%.6f,%.6f) flags=%d/%d/%d/%d\n",
            viewport,value.valid,value.frame,input.structVersion,input.cameraNear,input.cameraFar,input.cameraFOV,input.cameraViewToClip[1].y,
            input.mvecScale.x,input.mvecScale.y,input.depthInverted,input.cameraMotionIncluded,input.motionVectors3D,input.reset);
        value.frame=pose.renderFrameId;
        value.pose=pose.poseId;value.origin=pose.head.originSerial;
        if(value.valid) {
            std::lock_guard lock(mutex);
            // Detour_DlssConst records the native camera immediately before
            // slSetConstants. The scalar SL adapter has no centre-shift field;
            // do not overwrite the same camera's asymmetric shift with zeros.
            const auto& native=cameras[side][value.frame%8];
            if(native.valid && native.frame==value.frame && native.pose==value.pose && native.origin==value.origin) {
                value.camera.shift[0]=native.camera.shift[0];value.camera.shift[1]=native.camera.shift[1];
            } else if(cvr::projection::Active())value.valid=false;
            if(value.valid)cameras[side][value.frame%8]=value;
        }
    }
    return result;
}
using EvaluateFn=sl::Result (*)(sl::Feature,const sl::FrameToken&,const sl::BaseStructure* const*,uint32_t,void*);
EvaluateFn originalEvaluate{};
sl::Result Evaluate(sl::Feature feature,const sl::FrameToken& frame,const sl::BaseStructure* const* inputs,uint32_t count,void* list) {
    // Engine stereo: DLSS per eye (Stereo/EngineStereoDlss.cpp) instead of one evaluate over both eyes.
    sl::Result result;
    if(!cvr::engine_stereo::dlss::Evaluate(feature,frame,inputs,count,list,originalEvaluate,originalConstants,&result))
        result=cvr::engine_stereo::dlss::EvaluateOriginal(feature,frame,inputs,count,list,originalEvaluate);
    if(!CaptureInputsEnabled())return result;
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(CaptureInputsEnabled() && result==sl::Result::eOk && (feature==0 || feature==1001) && pose && pose.hasRenderFrameId && pose.view>=1 && pose.view<=2 && list) {
        std::lock_guard lock(mutex);
        // Engine stereo: MAIN's evaluate completes both eyes' records.
        const unsigned first=pose.view-1,last=SplitEyes(pose) ? 1 : first;
        for(unsigned side=first;side<=last;++side)
        for(auto& item:pools[side])if(item && !item->invalid && item->data.frameId==pose.renderFrameId &&
            item->data.poseId==pose.poseId && item->data.origin==pose.head.originSerial &&
            item->data.hasDepth && (!Enabled() || item->data.hasMotion) && !item->data.submitted && !item->data.evaluated) {
            item->data.evaluated=true;item->expected|=4;
            CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[7].fetch_add(1,std::memory_order_relaxed));
            pending[static_cast<ID3D12CommandList*>(list)].push_back({item,item->generation,4});
            havePending.store(true,std::memory_order_release);
        }
    }
    return result;
}
bool ReadTag(const sl::ResourceTag* p,InputTag* out) {
    __try {
        if(p->structType!=sl::ResourceTag::s_structType || p->structVersion!=1 ||
           !p->resource || p->resource->structType!=sl::Resource::s_structType ||
           p->resource->structVersion!=1 || p->resource->type!=sl::ResourceType::eTex2d)return false;
        *out={static_cast<ID3D12Resource*>(p->resource->native),p->resource->state,uint32_t(p->type),
            p->extent.left,p->extent.top,p->extent.width,p->extent.height};
        return out->resource && (out->type==0 || out->type==1) && out->state!=UINT32_MAX;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
}

bool ReadRenderFrameIndex(uint32_t* index) {
    // Native renderer frame clock: incremented at RVA294055 and read by
    // temporal rendering passes (e.g. RVA788104). The per-view +1A0 token
    // used by Streamline is permanently zero for this RTT view, so cannot
    // identify its texture history. Read the common clock; never modify it.
    __try {
        if(!index || !cvr::detail::g_exe_base)return false;
        const auto renderer=*reinterpret_cast<const uint8_t* const*>(cvr::detail::g_exe_base+cvr::detail::RENDERER_GLOBAL_RVA);
        if(!renderer)return false;*index=*reinterpret_cast<const uint32_t*>(renderer+0x4CC8);return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
namespace {
bool NativeCamera(void* state,unsigned view,CameraData* result) {
    // The exact source fields copied by native0x78933C into sl::Constants.
    // Its last-frame gate skips slSetConstants when VRCAM's native token
    // stays at zero, though staging already contains that view's camera.
    __try {
        if(!state)return false;const auto* p=static_cast<const uint8_t*>(state);CameraData c{};
        std::memcpy(c.position,p+0x1A0,12);std::memcpy(c.up,p+0x1AC,12);
        std::memcpy(c.right,p+0x1B8,12);std::memcpy(c.forward,p+0x1C4,12);
        std::memcpy(&c.nearPlane,p+0x1D0,4);std::memcpy(&c.farPlane,p+0x1D4,4);
        std::memcpy(&c.aspect,p+0x1DC,4);std::memcpy(c.jitter,p+0x1E0,8);std::memcpy(c.motionScale,p+0x1E8,8);
        const float projectionY=std::abs(*reinterpret_cast<const float*>(p+0xB4));
        c.verticalFov=projectionY>.001f ? 2*std::atan(1/projectionY) : 0;
        // The view's off-centre window, as the producer gave it (eye 1 = MAIN = side 0).
        cvr::projection::ViewWindow window{};
        if(cvr::projection::ViewWindowOf(int(view)-1,&window)){c.shift[0]=window.shiftX;c.shift[1]=window.shiftY;}
        c.inverted=p[0x1F1]!=0;c.cameraMotion=p[0x1F3]!=0;c.reset=p[0x1F0]==0;c.infinite=std::isinf(c.farPlane);
        c.motion3d=*reinterpret_cast<const uint8_t*>(reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr))+0x3914B80)!=0;
        if(!ValidCamera(c))return false;*result=c;return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
}
namespace {
bool ViewCamera(const void* view,CameraData* result) {
    // The fields SetStreamlineConstants (native0x788A9C) copies from the view into the block above,
    // read at their source: Streamline is not initialised as an upscaler everywhere (AMD), and the
    // native consumers must not depend on it.
    __try {
        const auto* v=static_cast<const uint8_t*>(view);if(!v)return false;CameraData c{};
        // The scene camera descriptor (Render/ViewCameraLayout.hpp); offsets below are the descriptor's own.
        const auto* d=v+cvr::view_layout::kCamera;
        const auto* fixed=reinterpret_cast<const int32_t*>(d);                 // position, 1/131072 m units
        for(unsigned i=0;i<3;++i)c.position[i]=float(fixed[i])*(1.0f/131072.0f);
        std::memcpy(c.right,d+0x250,12);std::memcpy(c.forward,d+0x260,12);std::memcpy(c.up,d+0x270,12);
        std::memcpy(&c.nearPlane,d+0x40,4);std::memcpy(&c.farPlane,d+0x44,4);std::memcpy(&c.aspect,d+0x28,4);
        std::memcpy(c.shift,d+0x30,8);                                          // the descriptor's own centre shift
        if(!std::isfinite(c.shift[0]) || !std::isfinite(c.shift[1]) || std::abs(c.shift[0])>=1 || std::abs(c.shift[1])>=1)c.shift[0]=c.shift[1]=0;
        float degrees{},jitterY{};std::memcpy(&degrees,d+0x20,4);std::memcpy(&jitterY,d+0x374,4);
        c.verticalFov=degrees*(3.14159265f/180.0f);                           // what FSR 3 is given too
        std::memcpy(&c.jitter[0],d+0x370,4);c.jitter[1]=-jitterY;
        const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        std::memcpy(&c.motionScale[0],reinterpret_cast<const void*>(base+0x3BFA1E0),4);
        std::memcpy(&c.motionScale[1],reinterpret_cast<const void*>(base+0x3BFA230),4);
        c.reset=v[cvr::view_layout::kHistory]==0;c.inverted=true;c.cameraMotion=true;c.infinite=std::isinf(c.farPlane);
        c.motion3d=*reinterpret_cast<const uint8_t*>(base+0x3914B80)!=0;
        if(!ValidCamera(c))return false;*result=c;return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
float Largest(const float* a,const float* b,unsigned n) {float d=0;for(unsigned i=0;i<n;++i)d=std::max(d,std::abs(a[i]-b[i]));return d;}
// Called with the lock held, after either camera of an eye was recorded.
void Compare(unsigned side,uint32_t frame) {
    const auto& a=cameras[side][frame%8];const auto& b=viewCameras[side][frame%8];
    if(!a.valid || !b.valid || a.frame!=frame || b.frame!=frame || a.pose!=b.pose)return;
    const auto& x=a.camera;const auto& y=b.camera;auto* out=CyberpunkVR_FramegenCameraDelta[side];
    out[0]=Largest(x.position,y.position,3);
    out[1]=std::max({Largest(x.up,y.up,3),Largest(x.right,y.right,3),Largest(x.forward,y.forward,3)});
    out[2]=std::max(std::abs(x.nearPlane-y.nearPlane),std::abs(x.farPlane-y.farPlane));
    out[3]=std::abs(x.verticalFov-y.verticalFov);out[4]=Largest(x.jitter,y.jitter,2);out[5]=Largest(x.motionScale,y.motionScale,2);
}
}
void RecordNativeCamera(void* state,unsigned view) {
    if(!CaptureInputsEnabled() || view<1 || view>2)return;
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || !pose.hasRenderFrameId || pose.view!=view)return;
    Constants value{};value.frame=pose.renderFrameId;value.valid=NativeCamera(state,view,&value.camera);
    value.pose=pose.poseId;value.origin=pose.head.originSerial;
    if(value.valid) {std::lock_guard lock(mutex);cameras[view-1][value.frame%8]=value;Compare(view-1,value.frame);}
}
void RecordViewCamera(const void* view,unsigned eye) {
    if(!CaptureInputsEnabled() || eye<1 || eye>2)return;
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || !pose.hasRenderFrameId || pose.view!=eye)return;
    Constants value{};value.frame=pose.renderFrameId;value.valid=ViewCamera(view,&value.camera);
    value.pose=pose.poseId;value.origin=pose.head.originSerial;
    if(value.valid) {std::lock_guard lock(mutex);viewCameras[eye-1][value.frame%8]=value;Compare(eye-1,value.frame);}
}
namespace {
// ENGINE STEREO: ONE FRAME, TWO EYES (Stereo/EngineStereo.hpp). MAIN's depth and motion hold both eyes side by side
// and its camera is the head centre's, while the headset shows each half as its own eye (Runtimes/OpenXRCapture.cpp)
// and framegen generates each eye from its inputs. Both eyes were given the whole wide frame and the centre's camera:
// each eye's frame was interpolated on the two eyes' motion squeezed into one. MAIN's inputs are recorded as two
// instead -- the left half in the first pool, the right half in the second -- each with its eye's camera: placed
// sep/2 along the right vector, its own lens window, the motion scale of a half-width input and the jitter in its
// own pixels, as Stereo/EngineStereoDlss.cpp gives each eye's DLSS.
InputTag Half(const InputTag& tag,unsigned eye) {
    InputTag half=tag;if(!tag.resource)return half;
    const auto desc=tag.resource->GetDesc();                                // a native tag is the whole resource
    half.width=(tag.width ? tag.width : uint32_t(desc.Width))/2;half.height=tag.height ? tag.height : desc.Height;
    half.x=tag.x+(eye ? half.width : 0);
    return half;
}
// Not from the tags: the engine tags one resource per call, and the record (and its camera) is made by the first.
CameraData CameraOfEye(CameraData c,unsigned eye) {
    const float offset=(eye ? .5f : -.5f)*cvr::engine_stereo::EyeSeparation();
    for(unsigned i=0;i<3;++i)c.position[i]+=c.right[i]*offset;
    uint32_t width{},height{};const bool sized=cvr::engine_stereo::EyeSize(&width,&height);
    cvr::engine_stereo::EyeProjection window{};
    if(cvr::engine_stereo::EyeWindow(int(eye),&window)) {c.aspect=window.aspect;c.shift[0]=window.shiftX;c.shift[1]=window.shiftY;}
    else if(sized)c.aspect=float(width)/float(height);                         // the shared window, square pixels
    c.motionScale[0]*=2;
    // The eye camera turns the jitter into NDC with its build's size (2 j / size), the eye's half has `width` pixels.
    uint32_t jitterWidth{},jitterHeight{};
    if(sized && cvr::engine_stereo::EyeJitterSize(&jitterWidth,&jitterHeight)) {
        c.jitter[0]*=float(width)/float(jitterWidth);c.jitter[1]*=float(height)/float(jitterHeight);
    }
    return c;
}
// One record of pool `side` for the pose's frame, found or allocated, its tags copied outside the lock.
// Called with the lock held; returns with it held.
bool RecordSide(unsigned side,const cvr::camera::PoseIdentity& pose,const CameraData& camera,const InputTag* tags,uint32_t count,
                ID3D12GraphicsCommandList* list,bool native,std::unique_lock<std::mutex>& lock) {
    std::shared_ptr<Record> record;
    for(const auto& item:pools[side])if(item && !item->invalid && !item->data.submitted && item->data.frameId==pose.renderFrameId &&
        item->data.poseId==pose.poseId && item->data.origin==pose.head.originSerial) {record=item;break;}
    if(!record)for(auto& item:pools[side]) {
        if(item && (item.use_count()!=1 || item->expected!=item->submitted ||
            (item->data.readyFence && item->data.readyFence->GetCompletedValue()<item->data.fenceValue)))continue;
        if(!item)item=std::make_shared<Record>();
        record=item;auto& data=record->data;
        data.poseId=pose.poseId;data.origin=pose.head.originSerial;data.frameId=pose.renderFrameId;data.view=side+1;
        data.camera=camera;data.queue=nullptr;data.hasDepth=data.hasMotion=data.submitted=data.evaluated=false;data.native=native;
        record->generation=++nextGeneration;record->expected=record->submitted=0;record->invalid=false;break;
    }
    if(!record) {OnSkip();return false;}
    // Resource/barrier hooks may enter other renderer locks. Do GPU recording
    // outside the input registry lock, then publish only the fields we wrote.
    // A prior component's queue/fence publication must not be overwritten.
    Inputs copy=record->data;
    lock.unlock();
    uint8_t copied=0;
    for(uint32_t i=0;i<count;++i) {
        const InputTag& tag=tags[i];if(!tag.resource)continue;
        if(tag.type==1 && !Enabled())continue;
        CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[5].fetch_add(1,std::memory_order_relaxed));
        if((tag.type==0 && copy.hasDepth) || (tag.type==1 && copy.hasMotion))continue;
        if(CopyInput(list,tag,copy)) {
            CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[6].fetch_add(1,std::memory_order_relaxed));
            copied|=tag.type==0 ? 1 : 2;
        }
    }
    lock.lock();
    if(copied) {
        auto& data=record->data;
        data.depth=std::move(copy.depth);data.motion=std::move(copy.motion);
        data.width=copy.width;data.height=copy.height;data.motionWidth=copy.motionWidth;data.motionHeight=copy.motionHeight;
        data.hasDepth=copy.hasDepth;data.hasMotion=copy.hasMotion;data.bytes=copy.bytes;
        if(!data.readyFence)data.readyFence=std::move(copy.readyFence);
        record->expected|=copied;
        pending[list].push_back({record,record->generation,copied});
        if(native && data.hasDepth && (!Enabled() || data.hasMotion) && !data.evaluated) {
            data.evaluated=true;record->expected|=4;
            pending[list].push_back({record,record->generation,4});
        }
        havePending.store(true,std::memory_order_release);
    }
    return record->data.hasDepth && (!Enabled() || record->data.hasMotion);
}
}
static bool RecordInputs(const InputTag* tags,uint32_t count,void* commandList,bool native) {
    if(!CaptureInputsEnabled() || !tags || !commandList || count>32)return false;
    CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[1].fetch_add(1,std::memory_order_relaxed));
    const auto pose=cvr::camera::CurrentNodePoseIdentity();
    if(!pose || pose.view<1 || pose.view>2)return false;
    CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[2].fetch_add(1,std::memory_order_relaxed));
    if(!pose.hasRenderFrameId)return false;
    CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[3].fetch_add(1,std::memory_order_relaxed));
    auto* list=static_cast<ID3D12GraphicsCommandList*>(commandList);
    if(list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    std::unique_lock lock(mutex);
    const auto constants=(native ? viewCameras : cameras)[pose.view-1][pose.renderFrameId%8];
    if(!constants.valid || constants.frame!=pose.renderFrameId || constants.pose!=pose.poseId || constants.origin!=pose.head.originSerial)return false;
    CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[4].fetch_add(1,std::memory_order_relaxed));
    if(!SplitEyes(pose))return RecordSide(pose.view-1,pose,constants.camera,tags,count,list,native,lock);
    std::array<InputTag,32> halves{};bool complete=true;
    for(unsigned eye=0;eye<2;++eye) {
        for(uint32_t i=0;i<count;++i)halves[i]=Half(tags[i],eye);
        complete=RecordSide(eye,pose,CameraOfEye(constants.camera,eye),halves.data(),count,list,native,lock) && complete;
    }
    return complete;
}
void RecordTags(const void*,const void* tags,uint32_t count,void* commandList) {
    CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[0].fetch_add(1,std::memory_order_relaxed));
    // Scope lenses write their velocity into this target (independent of framegen).
    if(tags && count<=32)for(uint32_t i=0;i<count;++i) {
        InputTag tag{};
        if(ReadTag(static_cast<const sl::ResourceTag*>(tags)+i,&tag) && tag.type==1)cvr::optics::MotionVectorsTagged(tag.resource);
    }
    if(!CaptureInputsEnabled() || !tags || count>32)return;
    std::array<InputTag,32> decoded{};
    for(uint32_t i=0;i<count;++i)
        if(!ReadTag(static_cast<const sl::ResourceTag*>(tags)+i,&decoded[i]))decoded[i]={};
    RecordInputs(decoded.data(),count,commandList,false);
}
bool RecordNativeInputs(const InputTag* tags,ID3D12GraphicsCommandList* list) {return RecordInputs(tags,2,list,true);}
bool InstallInputHooks() {
    if(hooksInstalled.load(std::memory_order_acquire))return true;
    const auto module=GetModuleHandleW(L"sl.interposer.dll");if(!module)return false;
    auto* constants=GetProcAddress(module,"slSetConstants");auto* evaluate=GetProcAddress(module,"slEvaluateFeature");
    if(!constants || !evaluate)return false;
    if(MH_CreateHook(constants,reinterpret_cast<void*>(&SetConstants),reinterpret_cast<void**>(&originalConstants))!=MH_OK)return false;
    if(MH_CreateHook(evaluate,reinterpret_cast<void*>(&Evaluate),reinterpret_cast<void**>(&originalEvaluate))!=MH_OK) {MH_RemoveHook(constants);return false;}
    if(MH_EnableHook(constants)!=MH_OK || MH_EnableHook(evaluate)!=MH_OK) {
        MH_DisableHook(constants);MH_DisableHook(evaluate);MH_RemoveHook(constants);MH_RemoveHook(evaluate);return false;
    }
    hooksInstalled.store(true,std::memory_order_release);Log("[framegen] native Streamline input hooks installed\n");
    cvr::engine_stereo::dlss::InstallOptionsHook();
    return true;
}
void Submitted(ID3D12CommandQueue* queue,UINT count,ID3D12CommandList* const* lists) {
    if(!havePending.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);
    for(UINT i=0;i<count;++i) {
        const auto it=pending.find(lists[i]);if(it==pending.end())continue;
        for(const auto& link:it->second)if(auto record=link.record.lock();record && record->generation==link.generation) {
            auto& data=record->data;
            if(data.queue && data.queue!=queue)record->invalid=true;
            data.queue=queue;record->submitted|=link.mask;
            if(data.readyFence && FAILED(queue->Signal(data.readyFence.Get(),++data.fenceValue)))record->invalid=true;
            if(!record->invalid && (record->submitted&5)==5 && record->submitted==record->expected && data.evaluated && !data.submitted) {
                data.submitted=true;OnInput(data.view-1,data.width,data.height);
                if(cvr::RuntimeDiagnosticsEnabled() && !logged[data.view-1].exchange(true))Log(
                    "[framegen] view=%u frame=%u pose=%llu depth=%ux%u mv=%ux%u scale=(%.6f,%.6f) near=%.6f far=%.3f fov=%.5f inverted=%d\n",
                    data.view,data.frameId,data.poseId,data.width,data.height,data.motionWidth,data.motionHeight,
                    data.camera.motionScale[0],data.camera.motionScale[1],data.camera.nearPlane,data.camera.farPlane,data.camera.verticalFov,data.camera.inverted);
                CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[8].fetch_add(1,std::memory_order_relaxed));
            }
        }
        pending.erase(it);
    }
    havePending.store(!pending.empty(),std::memory_order_release);
}
void ResetList(ID3D12GraphicsCommandList* list) {
    if(!havePending.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);const auto it=pending.find(list);if(it==pending.end())return;
    for(const auto& link:it->second)if(auto r=link.record.lock();r && r->generation==link.generation) {r->invalid=true;r->submitted|=link.mask;}
    pending.erase(it);
    havePending.store(!pending.empty(),std::memory_order_release);
}
std::shared_ptr<const Inputs> AcquireInputs(unsigned view,uint32_t frame,uint64_t pose,uint64_t origin,ID3D12CommandQueue* queue) {
    if(view<1 || view>2)return {};
    std::lock_guard lock(mutex);
    for(const auto& r:pools[view-1])if(r && !r->invalid && r->data.submitted && r->data.frameId==frame &&
        r->data.poseId==pose && r->data.origin==origin && r->data.queue==queue)
        {CVR_DIAGNOSTIC(CyberpunkVR_FramegenInputStages[9].fetch_add(1,std::memory_order_relaxed));return std::shared_ptr<const Inputs>(r,&r->data);}
    return {};
}
uint64_t InputVram() {std::lock_guard lock(mutex);uint64_t bytes=0;for(const auto& pool:pools)for(const auto& r:pool)if(r)bytes+=r->data.bytes;return bytes;}
void ReleaseInputs() {
    std::lock_guard lock(mutex);
    for(auto& pool:pools)for(auto& r:pool)if(r && r.use_count()==1 && r->expected==r->submitted &&
        (!r->data.readyFence || r->data.readyFence->GetCompletedValue()>=r->data.fenceValue))r.reset();
}
}
