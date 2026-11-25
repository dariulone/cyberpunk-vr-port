#include "Render/FoveatedDlss.hpp"
#include "Runtimes/EyeGazeMath.hpp"
#include <cmath>
#include <iostream>
#include <limits>
#include <stdexcept>
using namespace cvr::foveated;
void Check(bool value, const char* message) { if (!value) throw std::runtime_error(message); }
int main() try {
    Check(GetSettings().mode == 0, "must default off");
    Check(GetSettings() == Settings{0,70,65,false,0,-10},"default region must be 70 x 65 percent with Y=-10 percent");
    for (int mode = 0; mode <= 3; ++mode) for (float width : {20.0f, 60.0f, 100.0f}) for (float height : {30.0f, 70.0f, 100.0f}) {
        const auto value=Normalize({mode,width,height,true});
        Check(Enabled(value)==(mode!=0),"off bypass");
        Check(value.widthPercent==width && value.heightPercent==height,"independent region dimensions");
    }
    Check(Normalize({9,190,190,false}) == Settings{3,100,100,false,0,0}, "upper clamp");
    Check(Normalize({-1,-10,-10,false}) == Settings{0,20,20,false}, "lower clamp");
    Check(Normalize({1,NAN,NAN,false,NAN,NAN}) == Settings{1,70,65,false,0,-10}, "invalid values must use current defaults");
    Check(Normalize({1,60,80,false,40,-40}) == Settings{1,60,80,false,20,-10},"offset must keep the region in bounds");
    Check(Normalize({1,100,100,false,30,30}) == Settings{1,100,100,false,0,0},"full-frame offsets must be zero");
    Check(Normalize({1,60,100,false,INFINITY,NAN}).offsetYPercent == 0,"invalid offset fallback must be clamped to the image");
    const auto center=CenterRegion({1,60,60,false,0,0},1485,1485,2560,2560);
    Check(center==Region{512,512,1536,1536,891,891,297,297},"center alignment changed");
    const auto lowered=CenterRegion({1,60,60,false,-10,20},1485,1485,2560,2560);
    Check(lowered.sourceX < center.sourceX && lowered.sourceY > center.sourceY,"offset directions");
    Check(lowered.y+lowered.height == 2560 && lowered.sourceY+lowered.inputH == 1485,"lowered region must reach the lower edge exactly");
    for(float offset:{-80.0f,0.0f,80.0f})for(float width:{20.0f,69.3f,100.0f}) {
        const auto r=CenterRegion({1,width,43.7f,false,offset,-offset},1485,1279,2560,2208);
        Check(r.sourceX+r.inputW<=1485 && r.sourceY+r.inputH<=1279 && r.x+r.width<=2560 && r.y+r.height<=2208,"shifted crop outside the image");
    }
    SettingsDelay delay;
    Settings first{2,60,60},second{2,70,40,false,10,20};
    delay.Request(first,100);delay.Request(first,1050);delay.Update(1099);Check(delay.Applied().mode==0,"applied before one second");
    delay.Update(1100);Check(delay.Applied()==first,"identical INI polling restarted the timer");
    delay.SetEditing(true,1200);delay.Request(second,1200);delay.Update(12000);
    Check(delay.Applied()==first && delay.Requested()==second,"held slider applied while stationary");
    delay.SetEditing(false,12000);delay.Update(12999);Check(delay.Applied()==first,"release delay was too short");
    delay.Update(13000);Check(delay.Applied()==second,"final slider value was not applied one second after release");
    const Settings third{3,50,50};
    delay.Request(first,14000);delay.Request(third,14500);delay.Update(15000);
    Check(delay.Applied()==second,"intermediate drag value was applied");
    delay.Update(15500);Check(delay.Applied()==third,"last drag value was not applied");
    delay.SetEditing(true,15500);delay.Request({0,60,60},15500);
    Check(delay.Applied().mode==0,"Off must apply immediately even while editing");
    delay.SetEditing(false,15500);delay.Request(first,15600);delay.Update(16600);
    Check(delay.Applied()==first,"panel closure left settings stuck in editing");
    Settings loaded;
    Check(ParseSetting("xr_foveated_edges=nan\n", loaded) && loaded.widthPercent==70 && loaded.heightPercent==65,"invalid legacy inset must use current defaults");
    Check(ParseSetting("xr_foveated_dlss = 2\n", loaded), "spaced mode");
    Check(ParseSetting("xr_foveated_edges=25.5\n", loaded), "edge input");
    Check(loaded.widthPercent == 49 && loaded.heightPercent == 49, "legacy edge migration");
    Check(ParseSetting("xr_foveated_width=70\n", loaded), "width input");
    Check(ParseSetting("xr_foveated_height=40\n", loaded), "height input");
    Check(ParseSetting("xr_foveated_edges=20\n", loaded), "legacy key after dimensions");
    Check(loaded.widthPercent == 70 && loaded.heightPercent == 40, "legacy key overwrote explicit dimensions");
    Check(ParseSetting("xr_foveated_boundary=1\n", loaded), "boundary input");
    Check(ParseSetting("xr_foveated_offset_x=-10\n", loaded), "horizontal offset input");
    Check(ParseSetting("xr_foveated_offset_y=20\n", loaded), "vertical offset input");
    Check(ParseSetting("xr_foveated_eye_tracked=1\n", loaded), "eye tracked input");
    Check(!ParseSetting("xr_foveated_edges_extra=33\n", loaded), "unrelated setting matched");
    SetSettings(loaded); Check(GetSettings() == Settings{2,70,40,true,-10,20,0,true}, "requested settings state");
    FILE* file{}; Check(tmpfile_s(&file)==0 && file, "temporary file"); WriteSettings(file,loaded); std::rewind(file);
    Settings restored; char line[128]; while (std::fgets(line,sizeof(line),file)) Check(ParseSetting(line,restored), "round trip parse");
    std::fclose(file); Check(restored == loaded, "settings round trip");
    // Same-allocation offset / eyeTracked changes apply immediately without waiting 1s.
    {
        SettingsDelay dyn;
        Settings base{2,70,65,false,0,-10,0,false};
        dyn.Request(base,100);dyn.Update(1100);Check(dyn.Applied()==base,"base applied");
        Settings moved=base;moved.offsetXPercent=12.0f;moved.offsetYPercent=5.0f;moved.eyeTracked=true;
        dyn.Request(moved,1200);
        Check(dyn.Applied()==moved,"same-allocation offset/eyeTracked change must apply immediately");
    }
    // EyeGazeMath: Quest 3 asymmetric frustum projection, physical eye routing, deadband, saccade, blink hold/decay.
    {
        using namespace cvr::eyegaze;
        constexpr float kDeg=3.1415926535f/180.0f;
        const FrustumTangents q3Left{std::tan(-54.0f*kDeg),std::tan(40.0f*kDeg),std::tan(-49.0f*kDeg),std::tan(50.0f*kDeg)};
        const FrustumTangents q3Right{std::tan(-40.0f*kDeg),std::tan(54.0f*kDeg),std::tan(-49.0f*kDeg),std::tan(50.0f*kDeg)};
        float lx{},ly{},rx{},ry{};
        Check(ProjectGazeToEyePercent({0,0,0},{0,0,-1},0,0.064f,0.0f,q3Left,lx,ly),"left parallel straight ahead");
        Check(ProjectGazeToEyePercent({0,0,0},{0,0,-1},1,0.064f,0.0f,q3Right,rx,ry),"right parallel straight ahead");
        Check(lx>10.0f && lx<14.0f && std::fabs(lx+rx)<1e-3f,"asymmetric straight ahead must mirror nasally");
        float lxConv{},lyConv{},rxConv{},ryConv{};
        Check(ProjectGazeToEyePercent({0,0,0},{0,0,-1},0,0.064f,2.0f,q3Left,lxConv,lyConv),"left converged");
        Check(ProjectGazeToEyePercent({0,0,0},{0,0,-1},1,0.064f,2.0f,q3Right,rxConv,ryConv),"right converged");
        Check(lxConv>lx && rxConv<rx,"2m convergence must shift both eyes slightly inward");
        float rightX{},rightY{},downX{},downY{};
        Check(ProjectGazeToEyePercent({0,0,0},{0.5f,0,-1},0,0.064f,0.0f,q3Left,rightX,rightY),"gaze right");
        Check(rightX>lx,"looking right must increase offsetX");
        Check(ProjectGazeToEyePercent({0,0,0},{0,-0.5f,-1},0,0.064f,0.0f,q3Left,downX,downY),"gaze down");
        Check(downY>ly,"looking down must increase offsetY (+Y down)");
        Check(PhysicalEyeForRenderView(0,true,1)==0 && PhysicalEyeForRenderView(1,true,1)==1,"engine stereo eye mapping");
        Check(PhysicalEyeForRenderView(0,false,1)==1 && PhysicalEyeForRenderView(1,false,1)==0,"MAIN+VRCAM MainIsRightEye=1");
        Check(PhysicalEyeForRenderView(0,false,0)==0 && PhysicalEyeForRenderView(1,false,0)==1,"MAIN+VRCAM MainIsRightEye=0");
        Check(PhysicalEyeForRenderView(0,false,1,0)==0 && PhysicalEyeForRenderView(0,false,0,1)==1,"AFW eye mapping");
        Check(PhysicalEyeForRenderView(1,false,1,0)==1 && PhysicalEyeForRenderView(1,false,0,1)==0,"AFW second view eye mapping");
        Check(PhysicalEyeForRenderView(0,true,1,1)==0 && PhysicalEyeForRenderView(1,true,1,0)==1,"engine stereo must override AFW eye");

        // ResolveClampedOffset: peripheral gaze clamps first, then decays immediately past blink hold.
        Check(std::fabs(ResolveClampedOffset(40.0f, 0.0f, 15.0f, 1.0f) - 15.0f) < 1e-5f, "clamped peripheral gaze at blend=1");
        const float decayedOffset = ResolveClampedOffset(40.0f, 0.0f, 15.0f, 0.8f);
        Check(std::fabs(decayedOffset - 12.0f) < 1e-5f && decayedOffset < 15.0f, "post-blink decay must move immediately off edge");
        Check(std::fabs(ResolveClampedOffset(-40.0f, 5.0f, 15.0f, 0.5f) - (-5.0f)) < 1e-5f, "clamped negative gaze lerp toward fallback");
        Check(std::fabs(ResolveClampedOffset(std::numeric_limits<float>::quiet_NaN(), 25.0f, 15.0f, 1.0f) - 15.0f) < 1e-5f, "NaN gaze falls back to clamped fallback");

        GazeFilter filter;
        filter.Update(true,{0,0,0},{0,0,-1},1000);
        Check(filter.State()==TrackingState::Active && filter.ActiveBlend()==1.0f,"initial active sample");
        // Small fixation jitter (0.3 deg) is damped.
        const float jitterTan=std::tan(0.3f*kDeg);
        filter.Update(true,{0,0,0},{jitterTan,0,-1},1011);
        Check(filter.Direction().x<jitterTan*0.5f,"fixation jitter must be damped");
        // Large saccade (10 deg) tracks immediately.
        const float saccadeTan=std::tan(10.0f*kDeg);
        Vec3 saccadeDir{saccadeTan,0,-1};Normalize(saccadeDir);
        filter.Update(true,{0,0,0},saccadeDir,1022);
        Check(std::fabs(filter.Direction().x-saccadeDir.x)<1e-4f,"saccade must pass through without lag");
        // Blink hold (<=350ms) preserves gaze direction & blend=1.
        filter.Update(false,{0,0,0},{0,0,-1},1222);
        Check(filter.State()==TrackingState::HoldingBlink && filter.ActiveBlend()==1.0f,"blink hold within 350ms");
        Check(std::fabs(filter.Direction().x-saccadeDir.x)<1e-4f,"blink hold must freeze gaze direction");
        // Past 350ms dropout decays blend smoothly toward 0.
        filter.Update(false,{0,0,0},{0,0,-1},1450);
        Check(filter.State()==TrackingState::Inactive && filter.ActiveBlend()<1.0f && filter.ActiveBlend()>0.0f,"post-blink decay");

        // Sub-millisecond 120 Hz (8333 us) steps and 60 Hz duplicate sample handling.
        GazeFilter fDup, fRef;
        fDup.UpdateUs(true, {0,0,0}, {0,0,-1}, 1'000'000ull, false);
        fRef.UpdateUs(true, {0,0,0}, {0,0,-1}, 1'000'000ull, false);
        // Duplicate stale sample at +8333 us must not step fixation smoothing or overwrite m_lastSampleUs.
        fDup.UpdateUs(true, {0,0,0}, {jitterTan,0,-1}, 1'008'333ull, true);
        Check(fDup.Direction().x == 0.0f, "duplicate sample must not step fixation filter");
        // Distinct sample at +16667 us must step using the full 16667 us interval, matching fRef.
        fDup.UpdateUs(true, {0,0,0}, {jitterTan,0,-1}, 1'016'667ull, false);
        fRef.UpdateUs(true, {0,0,0}, {jitterTan,0,-1}, 1'016'667ull, false);
        Check(fDup.Direction().x > 0.0f && fDup.Direction().x < jitterTan * 0.5f, "120Hz/60Hz fixation step damped");
        Check(std::fabs(fDup.Direction().x - fRef.Direction().x) < 1e-6f, "duplicate sample must preserve true inter-sample dt");
        // Duplicate at 1'025'000 us refreshes blink-hold timestamp without changing direction.
        fDup.UpdateUs(true, {0,0,0}, {0,0,-1}, 1'025'000ull, true);
        Check(std::fabs(fDup.Direction().x - fRef.Direction().x) < 1e-6f, "second duplicate must preserve filtered direction");
        // At 1'370'000 us (353.3 ms after 1'016'667 us, 345.0 ms after 1'025'000 us):
        // fDup is still HoldingBlink because duplicate refreshed blink-hold; fRef has entered Inactive decay.
        fDup.UpdateUs(false, {0,0,0}, {0,0,-1}, 1'370'000ull, false);
        fRef.UpdateUs(false, {0,0,0}, {0,0,-1}, 1'370'000ull, false);
        Check(fDup.State() == TrackingState::HoldingBlink && fDup.ActiveBlend() == 1.0f, "duplicate sample must refresh blink hold timestamp");
        Check(fRef.State() == TrackingState::Inactive && fRef.ActiveBlend() < 1.0f, "reference filter past 350ms enters decay");

        // Normalize overflow rejection.
        Vec3 huge{1e20f, 0.0f, -1e20f};
        Check(!Normalize(huge), "overflowing finite vector must fail Normalize");

        // Zero-dt (nowUs == prevUs) stability in GazeFilter::UpdateUs during fixation and Inactive decay.
        GazeFilter fZero;
        fZero.UpdateUs(true, {0,0,0}, {0,0,-1}, 1'000'000ull, false);
        fZero.UpdateUs(true, {0,0,0}, {jitterTan,0,-1}, 1'016'667ull, false);
        const Vec3 dirBeforeZeroDt = fZero.Direction();
        for (int i = 0; i < 50; ++i) {
            fZero.UpdateUs(true, {0,0,0}, {jitterTan,0,-1}, 1'016'667ull, false);
        }
        Check(std::fabs(fZero.Direction().x - dirBeforeZeroDt.x) < 1e-6f &&
              std::fabs(fZero.Direction().y - dirBeforeZeroDt.y) < 1e-6f &&
              std::fabs(fZero.Direction().z - dirBeforeZeroDt.z) < 1e-6f,
              "zero-dt fixation calls must not change direction");
        fZero.UpdateUs(false, {0,0,0}, {0,0,-1}, 1'400'000ull, false);
        const float blendBeforeZeroDt = fZero.ActiveBlend();
        Check(fZero.State() == TrackingState::Inactive && blendBeforeZeroDt > 0.0f && blendBeforeZeroDt < 1.0f,
              "entered Inactive post-blink decay before zero-dt check");
        for (int i = 0; i < 50; ++i) {
            fZero.UpdateUs(false, {0,0,0}, {0,0,-1}, 1'400'000ull, false);
        }
        Check(fZero.ActiveBlend() == blendBeforeZeroDt &&
              std::fabs(fZero.Direction().x - dirBeforeZeroDt.x) < 1e-6f,
              "zero-dt Inactive calls must not decay ActiveBlend or change direction");

        // Frozen duplicateSample == true stall decay and recovery.
        GazeFilter fStall;
        fStall.UpdateUs(true, {0,0,0}, {0,0,-1}, 1'000'000ull, false);
        for (uint64_t t = 1'020'000ull; t <= 3'000'000ull; t += 20'000ull) {
            fStall.UpdateUs(true, {0,0,0}, {0.3f,0,-1}, t, true);
            if (t == 1'040'000ull) {
                Check(fStall.State() == TrackingState::Active && fStall.ActiveBlend() == 1.0f,
                      "duplicate sample within 60ms must stay Active");
            } else if (t == 1'200'000ull) {
                Check(fStall.State() == TrackingState::HoldingBlink && fStall.ActiveBlend() == 1.0f,
                      "stale duplicate sample >60ms within 350ms hold must enter HoldingBlink");
            } else if (t == 1'600'000ull) {
                Check(fStall.State() == TrackingState::Inactive && fStall.ActiveBlend() < 1.0f && fStall.ActiveBlend() > 0.0f,
                      "stale duplicate sample >410ms must decay in Inactive");
            } else if (t == 3'000'000ull) {
                Check(fStall.State() == TrackingState::Inactive && fStall.ActiveBlend() == 0.0f && !fStall.HasDirection(),
                      "extended frozen duplicate stall must decay to zero and clear direction");
            }
        }
        fStall.UpdateUs(true, {0,0,0}, {0,0,-1}, 3'020'000ull, false);
        Check(fStall.State() == TrackingState::Active && fStall.ActiveBlend() == 1.0f,
              "fresh distinct sample after stall must recover Active immediately");
    }
    // ComputeCropStep: smooth pursuit, saccade reset, immediate next-frame vergence step, camera-cut reset.
    {
        Region pursuitCenter{};
        pursuitCenter.inputW = 500;
        pursuitCenter.inputH = 400;
        pursuitCenter.sourceX = 240;
        pursuitCenter.sourceY = 280;
        const CropStep pursuit = ComputeCropStep(false, true, 200, 300, pursuitCenter);
        Check(!pursuit.saccadeReset && !pursuit.centralReset && pursuit.deltaX == 40.0f && pursuit.deltaY == -20.0f,
              "smooth pursuit ComputeCropStep");

        Region saccadeCenter = pursuitCenter;
        saccadeCenter.sourceX = 550;
        const CropStep saccade = ComputeCropStep(false, true, 200, 300, saccadeCenter);
        Check(saccade.saccadeReset && saccade.centralReset && saccade.deltaX == 0.0f && saccade.deltaY == 0.0f,
              "large saccade (>0.6*inputW) must trigger saccadeReset and zero deltas");

        Region nextFrameCenter = saccadeCenter;
        nextFrameCenter.sourceX = 535;
        const CropStep nextFrame = ComputeCropStep(false, true, 550, 280, nextFrameCenter);
        Check(!nextFrame.saccadeReset && !nextFrame.centralReset && nextFrame.deltaX == -15.0f,
              "immediate post-saccade vergence step must not trigger spurious double reset");

        const CropStep cameraCut = ComputeCropStep(true, true, 535, 280, nextFrameCenter);
        Check(cameraCut.centralReset && !cameraCut.saccadeReset && cameraCut.deltaX == 0.0f && cameraCut.deltaY == 0.0f,
              "engineOrFirstReset must set centralReset without saccadeReset and zero deltas");
    }
    std::cout << "PASS shifted region bounds, one-second debounce, immediate offset/eyeTracked, EyeGazeMath, and INI round trip\n";
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
