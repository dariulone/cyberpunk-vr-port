#include "Camera/PoseAddressLedger.hpp"
#include "Camera/ImagePoseLedger.hpp"
#include "Camera/ImagePoseMath.hpp"
#include "Runtimes/CaptureBufferLeases.hpp"
#include "Runtimes/CaptureFrontier.hpp"
#include "Runtimes/AsymmetricProjection.hpp"
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <string>

extern "C" {
int32_t CyberpunkVR_AsymProjection=1, CyberpunkVR_AsymSquarePixels=1;
float CyberpunkVR_AsymInnerGuard=0;
int32_t CyberpunkVR_EngineStereoEyeWindows=1, CyberpunkVR_MonoView=0;
}
int g_launcherWidth=2040, g_launcherHeight=2056;
namespace cvr::engine_stereo { bool Enabled() { return true; } }

namespace {
struct Token { uint64_t id{},origin{1};uint32_t view{1}; };
using Cameras=cvr::camera::PoseAddressLedger<Token,4>;
using Images=cvr::camera::ImagePoseLedger<Token>;
const cvr::camera::PoseFingerprint fp{{46297873,-322030768,23984874},{0,0,0,0x3f800000}};
void Check(bool ok,const char* why) { if(!ok) { std::cerr<<why<<'\n';std::exit(1); } }
void Near(float a,float b,const char* why) { Check(std::abs(a-b)<2e-6f,why); }
void IdenticalPose() {
    Cameras cameras;
    cameras.Publish(10,fp,{1,1,1});const auto first=cameras.Capture(10,fp);
    Check(cameras.Transfer(first,fp,20,fp),"first serialization was lost");
    cameras.Publish(10,fp,{2,1,1});const auto next=cameras.Capture(10,fp);
    Check(cameras.Transfer(next,fp,30,fp),"second serialization was lost");
    Check(cameras.Capture(20,fp).payload.id==1,"identical coordinates replaced an old image's identity");
    Check(cameras.Capture(30,fp).payload.id==2,"identical coordinates collapsed two generations");
    cameras.Publish(40,fp,{(uint64_t{1}<<63)|42,1,2});
    Check(cameras.Capture(40,fp).payload.id==((uint64_t{1}<<63)|42),"native sample identity lost its high bits");
}
void OwnerGeneration() {
    Cameras cameras;cameras.Publish(10,fp,{1});const auto old=cameras.Capture(10,fp);
    cameras.Invalidate(10);cameras.Publish(10,fp,{2});
    Check(!cameras.Transfer(old,fp,20,fp),"reused source address accepted an obsolete receipt");
    Check(!cameras.Capture(20,fp),"reused address exposed the wrong source identity");
}
void ChangedDuringCopy() {
    Cameras cameras;cameras.Publish(10,fp,{1});const auto old=cameras.Capture(10,fp);
    cameras.Publish(10,fp,{2});
    Check(!cameras.Transfer(old,fp,20,fp),"same coordinates hid a source publication during copy");
    auto moved=fp;++moved.position[0];
    const auto current=cameras.Capture(10,fp);
    Check(!cameras.Transfer(current,moved,20,moved),"unpublished source mutation was accepted");
}
void UntrackedCopy() {
    Cameras cameras;cameras.Publish(20,fp,{7});
    Check(!cameras.Transfer({},fp,20,fp),"untracked copy invented a source ID");
    Check(!cameras.Capture(20,fp),"untracked copy retained the previous occupant's ID");
}
void BoundedLedger() {
    Cameras cameras;
    for(uintptr_t i=1;i<30;++i)cameras.Publish(i,fp,{i});
    Check(!cameras.Capture(1,fp),"evicted object remained addressable");
    Check(cameras.Capture(29,fp).payload.id==29,"latest object was evicted");
    for(int i=0;i<8;++i)cameras.Publish(29,fp,{uint64_t(40+i)});
    Check(cameras.Capture(29,fp).payload.id==47,"old ring slot erased a fresh publication at the same address");
}
void CachedCameraLifetime() {
    Cameras cameras;
    cameras.Publish(20,fp,{7,1,2});
    for(uint64_t id=10;id<10000;++id)cameras.Publish(10,fp,{id,1,1});
    const auto held=cameras.Capture(20,fp);
    Check(held && held.payload.id==7,"unrelated MAIN publications expired a cached VRCAM pose");
    Check(cameras.Transfer(held,fp,30,fp),"cached VRCAM descriptor could not be copied again");
    cameras.Publish(40,fp,{8});
    for(uintptr_t address=50;address<100;++address) {
        Check(bool(cameras.Capture(20,fp)),"active cached camera was evicted before unused addresses");
        cameras.Publish(address,fp,{address});
    }
    Check(cameras.Capture(20,fp).payload.id==7,"cached camera lifetime depends on other addresses");
}
void Submission() {
    Images images;images.Record(10,100,{8},false);
    Check(!images.Read(100),"recording published pixels before queue submission");
    images.Submit(3,10,1000);
    Check(images.Read(100).pose.id==8 && images.Read(100).queue==3,"submitted image lost its source");
}
void ResetBeforeSubmit() {
    Images images;images.Record(10,100,{8},false);images.Reset(10);images.Submit(3,10,1000);
    Check(!images.Read(100),"discarded command list published a pose");
}
void ImageReuse() {
    Images images;images.Record(10,100,{11,1,1},false);images.Record(11,200,{12,1,2},true);
    images.Submit(3,10,1);images.Submit(3,11,1);const auto retained=images.LatestStable();
    images.Reset(10);images.Record(10,100,{13,1,1},false);images.Submit(3,10,2);
    Check(images.Read(100).pose.id==13,"MAIN did not advance");
    Check(images.LatestStable().pose.id==12 && retained.pose.id==12,"reused VRCAM image inherited MAIN's new pose");
    images.Reset(11);images.Record(11,200,{},true);images.Submit(3,11,3);
    Check(images.Read(200).pose.id==0,"unlabelled pixels inherited a previous image's pose");
}
void LatestWrite() {
    Images images;images.Record(10,100,{1},true);images.Record(10,200,{2},true);images.Record(10,100,{3},true);
    images.Submit(4,10,1);
    Check(images.LatestStable().resource==100 && images.LatestStable().pose.id==3,"target deduplication reordered the final copy");
}
void ObjectLifetime() {
    Images images;images.Record(10,100,{1},true);images.Submit(4,10,1);
    images.ForgetResource(100);images.ForgetList(10);
    Check(!images.Read(100) && !images.LatestStable(),"destroyed resource retained an image identity");
    images.Submit(4,10,2);Check(!images.Read(100),"destroyed command-list address replayed its previous recording");
    images.Record(11,100,{2},true);images.Submit(4,11,3);images.ForgetResource(100);
    images.Record(12,100,{3},true);images.Submit(4,12,4);images.Submit(4,11,5);
    Check(images.Read(100).pose.id==3,"old recording labelled a new resource at a reused address");
}
void Reservations() {
    cvr::capture::BufferLeases leases;
    Check(leases.Pin(10),"ready image cannot be pinned");
    Check(!leases.BeginWrite(10),"producer can replace pixels selected by the consumer");
    Check(leases.BeginWrite(20),"unused buffer requires an artificial frame wait");
    Check(!leases.Pin(20),"consumer selected a buffer with unpublished pixels");
    leases.EndWrite(20);Check(leases.Pin(20),"completed publication stayed inaccessible");
    leases.Unpin(10);Check(leases.BeginWrite(10),"reader reservation leaked");
    leases.EndWrite(10);leases.Unpin(20);Check(leases.BeginWrite(20),"reused VRCAM slot stayed pinned");
}
void CaptureFrontier() {
    // A 4-slot producer queues frames every tick; GPU completion is three ticks
    // behind it. Latest/previous-only selection never observes a finished image.
    // The retained frontier must make progress without extra buffers or a CPU wait.
    struct Frame { uint64_t serial{}, fence{}; int slot{-1}; } latest, previous;
    uint64_t presented=0;
    unsigned delivered=0, legacyDelivered=0;
    cvr::capture::BufferLeases leases;
    for(uint64_t tick=1;tick<=300;++tick) {
        const auto completed=tick>3 ? tick-3 : 0;
        const bool protect=cvr::capture::ProtectPrevious(previous.serial,presented);
        int write=-1;
        for(int slot=0;slot<4;++slot) {
            if(slot==latest.slot || (protect && slot==previous.slot))continue;
            if(leases.BeginWrite(uintptr_t(slot+1))) {write=slot;break;}
        }
        Check(write>=0,"retaining the frontier exhausted the four capture slots");
        Check(!protect || write!=previous.slot,"unfinished unpresented image was overwritten");
        if(latest.serial && cvr::capture::ReplacePrevious(previous.serial,presented,latest.fence,completed))
            previous=latest;
        latest={tick,tick,write};leases.EndWrite(uintptr_t(write+1));
        const auto pick=latest.fence<=completed ? latest : previous;
        if(pick.serial>presented && pick.fence<=completed) {
            Check(leases.Pin(uintptr_t(pick.slot+1)),"ready frontier was being written");
            presented=pick.serial;++delivered;leases.Unpin(uintptr_t(pick.slot+1));
        }
        if(tick>1 && tick-1<=completed)++legacyDelivered;
    }
    Check(legacyDelivered==0,"fixture no longer reproduces the old starvation");
    Check(delivered>=70 && presented>=294,"deep GPU queue starved the headset");
    Check(cvr::capture::ReplacePrevious(8,4,9,9),"ready newer frame cannot replace the frontier");
    Check(!cvr::capture::ReplacePrevious(8,4,9,7),"unfinished newer frame displaced the frontier");
    Check(cvr::capture::ReplacePrevious(8,8,9,7),"presented frontier blocked future progress");
    Check(!cvr::capture::ProtectPrevious(9,8,1,2),"old tracking origin kept a protected capture slot after recenter");
    Check(cvr::capture::ProtectPrevious(9,8,2,2),"current tracking origin lost its unpresented frontier");
    // With a pending framegen endpoint and the current consumer occupying the
    // other slots, the producer skips rather than sacrificing either eye.
    for(uintptr_t slot:{uintptr_t(3),uintptr_t(4)})Check(leases.Pin(slot),"cannot pin framegen/consumer slot");
    bool available=false;
    for(uintptr_t slot=1;slot<=4;++slot) {
        if(slot==1 || (slot==2 && cvr::capture::ProtectPrevious(9,8)))continue;
        if(leases.BeginWrite(slot)) {available=true;leases.EndWrite(slot);}
    }
    Check(!available,"capture overwrote a protected stereo pair under slot pressure");
}
void CaptureEyeOrigin() {
    cvr::capture::BufferLeases leases;
    struct Label {uint64_t origin{};bool valid{};} label{7,true};
    uint64_t serial=100;
    Check(leases.Pin(42),"cannot retain old eye while consumer uses it");
    Check(!leases.BeginWrite(42,serial,label) && serial==100 && label.valid,
        "failed write reservation invalidated the consumer's image metadata");
    leases.Unpin(42);
    Check(leases.BeginWrite(42,serial,label),"unused eye slot unavailable");
    Check(!serial && !label.valid && !label.origin,"new write kept an older pixel label");
    Check(!leases.Pin(42),"consumer read an unpublished write");
    // GPU write was submitted, then origin reset cancelled publication.
    leases.EndWrite(42);
    Check(!cvr::capture::ReusableEye(serial,101,3,label.origin,8),
        "discarded capture exposed overwritten pixels as the old frame");
    serial=102;label={8,true};
    Check(cvr::capture::ReusableEye(serial,102,0,label.origin,8),"exact current-origin eye rejected");
    Check(cvr::capture::ReusableEye(serial,105,3,label.origin,8),"bounded current-origin reuse rejected");
    Check(!cvr::capture::ReusableEye(serial,106,3,label.origin,8),"eye reuse exceeded configured frame age");
    Check(!cvr::capture::ReusableEye(serial,102,3,label.origin,9),"exact serial bypassed tracking-origin check");
    Check(!cvr::capture::ReusableEye(serial,103,3,label.origin,9),"previous eye crossed a recenter through reuse");
    Check(!cvr::capture::ReusableEye(serial,101,3,label.origin,8),"future eye was paired with older MAIN");
}
void RuntimeFrusta() {
    using namespace cvr::projection;
    constexpr float radians=.0174532925199433f;
    // Actual Steam Frame frusta in issue #243's attached log. These are not
    // symmetric and their target aspect need not equal the runtime recommendation.
    XrFovf fovs[2]={{-59.139f*radians,50.359f*radians,49.396f*radians,-60.061f*radians},
                   {-50.287f*radians,59.187f*radians,49.905f*radians,-59.555f*radians}};
    const XrQuaternionf eyes[2]={{0,0,0,1},{0,0,0,1}};
    EyeWindow baseline[2];Check(Windows(fovs,eyes,baseline),"logged Steam Frame frusta rejected");
    for(unsigned crop=0;crop<5;++crop) {
        XrFovf altered[2]={fovs[0],fovs[1]};
        if(crop==1) {altered[0].angleRight*=.5f;altered[1].angleLeft*=.5f;}
        if(crop==2) {altered[0].angleLeft*=.8f;altered[1].angleRight*=.8f;}
        if(crop==3)for(auto& eye:altered)eye.angleUp*=.7f;
        if(crop==4)for(auto& eye:altered)eye.angleDown*=.7f;
        EyeWindow fitted[2];Check(Windows(altered,eyes,fitted),"runtime crop rejected");
        bool changed=false;
        for(unsigned eye=0;eye<2;++eye) {
            const auto& window=fitted[eye];const auto submitted=ToFov(window);const auto engine=ToEngine(window);
            Near(engine.aspect,float(g_launcherWidth)/g_launcherHeight,"runtime crop stretched render pixels");
            Check(window.l<=std::tan(altered[eye].angleLeft)+1e-5f &&
                  window.r>=std::tan(altered[eye].angleRight)-1e-5f &&
                  window.d<=std::tan(altered[eye].angleDown)+1e-5f &&
                  window.u>=std::tan(altered[eye].angleUp)-1e-5f,"aspect fit clipped a visible lens edge");
            const float sy=1/std::tan(engine.vfovDeg*radians*.5f),sx=sy/engine.aspect;
            for(float nx:{-.8f,0.0f,.7f})for(float ny:{-.7f,0.0f,.8f}) {
                const float x=window.l+(nx+1)*.5f*(window.r-window.l);
                const float y=window.d+(ny+1)*.5f*(window.u-window.d);
                Near(x*sx+engine.shiftX,nx,"rendered horizontal ray disagrees with submit frustum");
                Near(y*sy+engine.shiftY,ny,"rendered vertical ray disagrees with submit frustum");
            }
            Near(std::tan(submitted.angleLeft),window.l,"submitted left edge disagrees with render");
            Near(std::tan(submitted.angleUp),window.u,"submitted top edge disagrees with render");
            changed=changed || std::abs(window.l-baseline[eye].l)>.001f ||
                std::abs(window.r-baseline[eye].r)>.001f || std::abs(window.d-baseline[eye].d)>.001f ||
                std::abs(window.u-baseline[eye].u)>.001f;
        }
        Check(crop==0 || changed,"runtime FOV change had no effect on rendered windows");
    }
}
void EyeRebase() {
    const XrPosef old{{0,0,0,1},{1,2,3}};
    const XrPosef current{{0,std::sin(.4f),0,std::cos(.4f)},{-.3f,1.7f,.2f}};
    for(float eye:{-.0325f,.0325f}) {
        const XrPosef prior{{0,0,0,1},{old.position.x+eye,old.position.y,old.position.z}};
        const auto result=cvr::camera::RebaseImageEyePose(old,prior,current);
        const auto expected=RotateVector(current.orientation,{eye,0,0});
        Near(result.position.x,current.position.x+expected.x,"eye offset mixed two head samples");
        Near(result.position.y,current.position.y+expected.y,"image label lost its own height");
        Near(result.position.z,current.position.z+expected.z,"image label lost its own translation");
        Near(result.orientation.y,current.orientation.y,"image label lost its own rotation");
    }
}
bool SameSample(const Token& a,const Token& b) {
    return a.id==b.id && a.origin==b.origin && a.view==1 && b.view==1;
}
void CameraBlend() {
    Cameras cameras;
    cameras.Publish(10,fp,{81});cameras.Publish(11,fp,{81});
    Cameras::BlendInput inputs[]={{cameras.Capture(10,fp),fp,.3f},{cameras.Capture(11,fp),fp,.7f}};
    auto blended=fp;
    ++blended.rotation[2];++blended.position[0]; // native nlerp/fixed-point conversion
    Check(cameras.TransferBlend(inputs,20,blended,SameSample),"native blend lost a common HMD sample");
    Check(cameras.Capture(20,blended).payload.id==81,"blend replaced the input's identity");
    Images images;images.Record(1,100,cameras.Capture(20,blended).payload,false);images.Submit(2,1,1);
    Check(images.Read(100).pose.id==81,"blended MAIN image lost its exact sample");
}
void CameraBlendReject() {
    Cameras cameras;cameras.Publish(10,fp,{81});cameras.Publish(11,fp,{82});
    Cameras::BlendInput inputs[]={{cameras.Capture(10,fp),fp,.5f},{cameras.Capture(11,fp),fp,.5f}};
    cameras.Publish(20,fp,{80});
    Check(!cameras.TransferBlend(inputs,20,fp,SameSample),"different head samples were labelled as one");
    Check(!cameras.Capture(20,fp),"failed blend retained an old image label");
    inputs[1].source={};inputs[0].weight=.999999f;inputs[1].weight=.000001f;
    Check(!cameras.TransferBlend(inputs,20,fp,SameSample),"small native camera weight was silently discarded");
    inputs[0].weight=1;inputs[1].weight=0;
    Check(cameras.TransferBlend(inputs,20,fp,SameSample),"zero-weight camera revoked a valid source");
    inputs[0].weight=.5f;
    Check(!cameras.TransferBlend(inputs,20,fp,SameSample),"scaled head translation kept an unscaled label");
}
void CameraBlendStale() {
    Cameras cameras;cameras.Publish(10,fp,{81});cameras.Publish(11,fp,{81});
    Cameras::BlendInput inputs[]={{cameras.Capture(10,fp),fp,.5f},{cameras.Capture(11,fp),fp,.5f}};
    cameras.Publish(11,fp,{81});
    Check(!cameras.TransferBlend(inputs,20,fp,SameSample),"identical coordinates hid a replaced blend input");
    inputs[1].source=cameras.Capture(11,fp);++inputs[1].current.rotation[0];
    Check(!cameras.TransferBlend(inputs,20,fp,SameSample),"changed input inherited an old pose label");
}
}
int main(int argc,char** argv) {
    if(argc!=2)return 2;const std::string name=argv[1];
    if(name=="camera_blend") { CameraBlend();return 0; }
    if(name=="camera_blend_reject") { CameraBlendReject();return 0; }
    if(name=="camera_blend_stale") { CameraBlendStale();return 0; }
    if(name=="capture_frontier") { CaptureFrontier();return 0; }
    if(name=="capture_eye_origin") { CaptureEyeOrigin();return 0; }
    if(name=="runtime_frusta") { RuntimeFrusta();return 0; }
    if(name=="identical_pose")IdenticalPose();else if(name=="owner_generation")OwnerGeneration();
    else if(name=="changed_during_copy")ChangedDuringCopy();else if(name=="untracked_copy")UntrackedCopy();
    else if(name=="bounded_ledger")BoundedLedger();else if(name=="submission")Submission();
    else if(name=="cached_camera_lifetime")CachedCameraLifetime();
    else if(name=="reset_before_submit")ResetBeforeSubmit();else if(name=="image_reuse")ImageReuse();
    else if(name=="latest_write")LatestWrite();else if(name=="object_lifetime")ObjectLifetime();
    else if(name=="reservations")Reservations();else if(name=="eye_rebase")EyeRebase();else return 2;
}
