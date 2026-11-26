// trace: the player's solve -- src/Anim/CharacterRig.cpp, or its Rust port (CVR_RUST_VRIK, rust/crates/cvr-rig/src/vrik)
// -- on long deterministic random input over the captured rigs, every output hashed bit for bit: the pose buffer, the
// FK, the caches and every probe the natives read. Both builds must print the same line (docs/rust-migration-plan.md,
// stage 3). Included inside main.cpp's anonymous namespace: it uses its fixtures and helpers.
//
// WHAT IS KEPT OFF THE CLOCK. Parts of the solve read the time: the elbow's temporal smoothing, the wrist-lock ease, the
// thorax turn, the stance gate and the clip's body weight. The trace bumps the rig revision before every solve (the
// policy is configured again, alternating the body), so the smoothing starts afresh; leaves the wrist lock off and
// the thorax turn uncalled; sets the stance gate off where the animation's feet change between solves and gives it
// one fixed pose where it is on; and runs with no clip move. The bend's own clock is the tests' TestClock.
//
// EVERY RANDOM INPUT IS DRAWN INTO A NAMED LOCAL FIRST, in statement order (C++ leaves argument order unspecified).

struct TraceHash {
    uint64_t h=1469598103934665603ull,n=0;
    void Bytes(uint64_t v,int bytes) { for(int i=0;i<bytes;++i) { h^=(v>>(8*i))&0xFF;h*=1099511628211ull; } ++n; }
    // A NaN is hashed as one value (its sign and payload are the compiler's operand order, not behaviour).
    void F(float v) { Bytes(v!=v ? 0x7FC00000u:std::bit_cast<uint32_t>(v),4); }
    void Fs(const volatile float* p,int count) { for(int i=0;i<count;++i)F(p[i]); }
    void I(int64_t v) { Bytes(static_cast<uint64_t>(v),8); }
    void B(bool v) { Bytes(v ? 1:0,1); }
};
struct TraceRng {
    uint64_t s;
    explicit TraceRng(uint64_t seed):s(seed) {}
    uint64_t Next() { s^=s<<13;s^=s>>7;s^=s<<17;return s; }
    float Unit() { return float(Next()>>40)/float(1ull<<24); }
    float Range(float a,float b) { const float u=Unit();return a+(b-a)*u; }
    bool Chance(uint32_t oneIn) { return Next()%oneIn==0; }
    void Quat(float* q,float yawRange,float pitchRange,float rollRange) {
        const float yaw=Range(-yawRange,yawRange);const float pitch=Range(-pitchRange,pitchRange);
        const float roll=Range(-rollRange,rollRange);
        const float y[4]={0,0,std::sin(yaw*.5f),std::cos(yaw*.5f)},p[4]={std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)};
        const float r[4]={0,std::sin(roll*.5f),0,std::cos(roll*.5f)};
        float yp[4];VRIK_QuatMul(y,p,yp);VRIK_QuatMul(yp,r,q);
    }
    void Vec(float* v,float range) { for(int k=0;k<3;++k) { const float x=Range(-range,range);v[k]=x; } }
};

// The configured rig again, the body alternating: a new rig revision, so every per-rig cache and smoothing restarts.
void TraceReconfigure(bool female) {
    std::vector<const char*> pointers;for(const auto& n:names)pointers.push_back(n.c_str());
    const auto reference=Read<Bone>("reference_full.bin");
    VRIK_ConfigureRigPolicy(pointers.data(),static_cast<int>(pointers.size()),
        reinterpret_cast<const uint8_t*>(reference.data()),static_cast<int>(reference.size()),female);
}
void TracePacket() { auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);seq+=2;RefreshHandsSnapshot(); }

void TraceHashState(TraceHash& h,const std::vector<Bone>& pose) {
    for(const auto& b:pose) { h.Fs(b.p,4);h.Fs(b.q,4);h.Fs(b.s,4); }
    const int count=std::min(VRIK_FKCount(),static_cast<int>(pose.size()));
    for(int i=0;i<count;++i) { h.Fs(g_fkPos[i],3);h.Fs(g_fkRot[i],4); }
    for(int s=0;s<2;++s) {
        h.Fs(g_VRIKHandPin[s],3);h.Fs(g_VRIKHandAim[s],8);h.Fs(g_VRIKGirdleReach[s],4);h.Fs(g_VRIKDbgClav[s],8);
        h.Fs(g_VRHandRawModel[s],3);h.Fs(g_VRHandRawRot[s],4);h.I(g_VRHandRawValid[s]);
        h.Fs(CyberpunkVR_DebugToes[s],3);
    }
    h.Fs(g_VRIKBodyEyeTarget,3);h.Fs(g_VRIKBodyEyeSolved,3);h.B(g_VRIKBodyEyeBound);h.B(g_VRIKVehicleUpperActive);
    h.F(g_VRIKBodyBendAngle);h.Fs(g_VRIKBodyBendPelvis,3);h.F(s_vrSharedSquatDrop);
    h.Fs(CyberpunkVR_DebugGripShortMm,2);h.Fs(CyberpunkVR_DebugGripReach,6);h.Fs(CyberpunkVR_DebugClipBody,12);
    h.F(CyberpunkVR_DebugSquatStance[1]);
    h.Fs(g_VRIKDbgShModel,3);h.F(g_VRIKDbgHipsYaw);h.Fs(g_VRIKDbgChest,3);h.Fs(g_VRIKDbgChestTgt,3);
    h.Fs(g_VRIKDbgTarget,3);h.Fs(g_VRIKDbgShoulder,3);h.Fs(g_VRIKDbgElbow,3);h.Fs(g_VRIKDbgLens,2);h.Fs(g_VRIKDbgLocal,4);
    h.Fs(g_VRIKDbgTargetL,3);h.Fs(g_VRIKDbgShoulderL,3);h.Fs(g_VRIKDbgElbowL,3);h.Fs(g_VRIKDbgLensL,2);h.Fs(g_VRIKDbgLocalL,4);
    for(int i=0;i<11;++i) { h.Fs(g_VRBodyBone[i],3);h.I(g_VRBodyBoneOk[i]); }
    h.I(g_solveCacheN);h.B(g_solveCacheModel);
    for(int i=0;i<g_solveCacheN;++i) { h.I(g_solveCacheIdx[i]);h.Fs(g_solveCacheVal[i],7); }
}

// The arms over a pose the body solve placed: the stance fixtures, or the seated passenger.
void TraceArms(TraceRng& r,TraceHash& h,std::vector<Bone>& pose,const float* right,const float* up,const float* forward,
               const float* anchor) {
    auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    for(bool left:{false,true}) {
        const int upper=left ? g_VRLeftUpperArmIdx:g_VRRightUpperArmIdx,fore=left ? g_VRLeftForeArmIdx:g_VRRightForeArmIdx;
        const int hand=left ? g_VRLeftBoneIdx:g_VRRightBoneIdx,stopSide=left ? 0:1;
        const float half=r.Range(.10f,.22f);const bool neckAnchor=!r.Chance(3);
        float joint[3];VRIK_AnchorShoulder(buffer,upper,anchor,left,right,up,half,joint,neckAnchor);
        float offset[3];r.Vec(offset,.75f);
        float target[3]={joint[0]+offset[0],joint[1]+offset[1],joint[2]+offset[2]};
        float rotation[4];r.Quat(rotation,3.1f,1.5f,3.1f);
        const bool stop=r.Chance(3);float stopOffset[3];r.Vec(stopOffset,.12f);
        g_VRHandStopValid[stopSide]=stop ? 1:0;
        for(int k=0;k<3;++k)g_VRHandStopModel[stopSide][k]=target[k]+stopOffset[k];
        VRIK_ApplyHandStop(stopSide,target,rotation);
        VRIK_NoteHandAim(left ? 1:0,target,rotation);
        const float pole=r.Chance(2) ? 0.0f:r.Range(-1.2f,1.2f);const float swing=r.Range(0,1.5f);
        const bool ladderReach=r.Chance(6);
        VRIK_SolveArm(buffer,upper,fore,hand,target,rotation,right,up,forward,pole,swing,left,true,ladderReach);
        VRIK_ComputeFK(buffer,VRIK_FKCount());
        for(int stage=0;stage<4;++stage)VRIK_NoteShake(left ? 1:0,stage,target);
    }
    for(int s=0;s<2;++s)for(int stage=0;stage<4;++stage) {
        h.F(g_VrikShakePeakMm[s][stage]);h.F(g_VrikShakeSlowMm[s][stage]);h.I(g_VrikShakeSlowN[s][stage]);
    }
}

void TraceStance(TraceHash& h,uint64_t seed) {
    Setup("weapon_stance");TraceRng r(seed);
    const auto input=Read<float>("solve_inputs.bin");
    const std::vector<Bone> poses[4]={Read<Bone>("unarmed-2.bin"),Read<Bone>("unarmed-3.bin"),
                                      Read<Bone>("shotgun-2.bin"),Read<Bone>("shotgun-3.bin")};
    static std::vector<Bone> pose,replay;
    CyberpunkVR_SquatSymmetric=0;   // the animation's feet change between solves here: the stance gate stays off
    for(int it=0;it<160;++it) {
        TraceReconfigure(it&1);
        const auto which=r.Next()%4;pose=poses[which];replay=poses[(which+1)%4];
        auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        const bool bend=r.Chance(2);const float angle=r.Range(0,1.25f);const float neck=r.Range(-.6f,.05f);
        const bool raw=r.Chance(4);
        shared[115]=bend ? 2.0f:0.0f;shared[114]=angle;shared[89]=neck;shared[90]=raw ? 0.0f:neck-.08f;TracePacket();
        const bool calibrated=r.Chance(2);const float armR=r.Range(.4f,.9f);const float armL=r.Range(.4f,.9f);
        g_VRUserArmLenR=calibrated ? armR:0;g_VRUserArmLenL=calibrated ? armL:0;
        CyberpunkVR_VrikElbowPolicy=int(r.Next()%2);CyberpunkVR_GripReach=int(r.Next()%2);
        CyberpunkVR_TwoHandActive=int(r.Next()%2);g_VRBind=r.Chance(4) ? 3:4;
        CyberpunkVR_SquatKneeMode=int(r.Next()%3);CyberpunkVR_SquatFeetUnderHips=int(r.Next()%2);
        float camera[3];r.Vec(camera,.12f);for(int k=0;k<3;++k)camera[k]+=input[k];
        float rotation[4];r.Quat(rotation,1.2f,.9f,.3f);
        float headDelta[3],orbit[3];r.Vec(headDelta,.06f);r.Vec(orbit,.12f);
        const bool useDelta=r.Chance(2),useOrbit=r.Chance(2);
        VRIK_DampenTorsoWeaponPose(buffer);VRIK_PinGirdleTranslations(buffer);VRIK_ComputeFK(buffer,VRIK_FKCount());
        float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
        h.B(VRIK_BodyAxesFromRig(right,up,forward));
        VRIK_PlaceBodyUnderHMD(buffer,camera,rotation,g_VRHeadBoneIdx,forward,useDelta ? headDelta:nullptr,useOrbit ? orbit:nullptr);
        h.B(VRIK_BodyAxesFromRig(right,up,forward));h.Fs(right,3);h.Fs(up,3);h.Fs(forward,3);
        float anchor[3];std::copy_n(g_fkPos[g_VRNeckIdx],3,anchor);
        TraceArms(r,h,pose,right,up,forward,anchor);
        const bool preserve=r.Chance(2);
        VRIK_SyncShadowUpper(buffer,preserve);VRIK_ComputeFK(buffer,VRIK_FKCount());
        TraceHashState(h,pose);
        if(r.Chance(2))VRIK_CaptureModelCache(preserve);
        else VRIK_CaptureLocalArmCache(buffer,true,true);
        auto* next=reinterpret_cast<uint8_t*>(replay.data());
        VRIK_ComputeFK(next,VRIK_FKCount());VRIK_ReplaySolveCache(next);VRIK_ComputeFK(next,VRIK_FKCount());
        TraceHashState(h,replay);
    }
    CyberpunkVR_SquatSymmetric=1;g_VRHandStopValid[0]=g_VRHandStopValid[1]=0;
}

// Standing: one animation pose throughout (the stance gate on), the squat, the bend and the body orbit varying.
void TraceBody(TraceHash& h,uint64_t seed) {
    Setup();TraceRng r(seed);
    const auto native=Read<Bone>("player_local.bin");
    static std::vector<Bone> pose;
    for(int it=0;it<120;++it) {
        if(r.Chance(5))TraceReconfigure(it&1);
        pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        const bool bend=r.Chance(2);const float angle=r.Range(0,1.3f);const float neck=r.Range(-.75f,.05f);
        shared[115]=bend ? 2.0f:0.0f;shared[114]=angle;shared[89]=neck;shared[90]=neck-.08f;TracePacket();
        CyberpunkVR_SquatKneeMode=int(r.Next()%3);CyberpunkVR_SquatFeetUnderHips=int(r.Next()%2);
        float rotation[4];r.Quat(rotation,1.5f,1.0f,.3f);
        float headDelta[3],orbit[3];r.Vec(headDelta,.08f);r.Vec(orbit,.15f);
        const float height=r.Range(1.1f,1.75f);
        const float camera[3]={0,0,height},forward[3]={0,1,0};
        VRIK_DampenTorsoWeaponPose(buffer);VRIK_ComputeFK(buffer,VRIK_FKCount());
        VRIK_PlaceBodyUnderHMD(buffer,camera,rotation,g_VRHeadBoneIdx,forward,r.Chance(3) ? nullptr:headDelta,orbit);
        VRIK_SyncShadowUpper(buffer,false);
        TraceHashState(h,pose);
        if(r.Chance(4)) { VRIK_HoldReferencePose(buffer);TraceHashState(h,pose); }
    }
}

void TraceVehicle(TraceHash& h,uint64_t seed) {
    Setup("passenger-window");TraceRng r(seed);
    const auto native=Read<Bone>("vehicle_pose.bin");
    static std::vector<Bone> pose;
    for(int it=0;it<60;++it) {
        TraceReconfigure(it&1);
        pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        VRIK_BeginBodySolve(buffer);VRIK_ComputeFK(buffer,VRIK_FKCount());
        float head[4];r.Quat(head,3.1f,.8f,.3f);
        float camera[3];r.Vec(camera,.15f);camera[0]+=.47f;camera[1]+=.18f;camera[2]+=1.21f;
        h.B(VRIK_PlaceVehicleCombatUpper(buffer,camera,head,g_VRHeadBoneIdx));
        float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
        h.B(VRIK_BodyAxesFromRig(right,up,forward,true));
        float anchor[3];std::copy_n(g_fkPos[g_VRNeckIdx],3,anchor);
        CyberpunkVR_GripReach=int(r.Next()%2);CyberpunkVR_VrikElbowPolicy=int(r.Next()%2);
        TraceArms(r,h,pose,right,up,forward,anchor);
        VRIK_SyncShadowUpper(buffer,true);VRIK_ComputeFK(buffer,VRIK_FKCount());
        TraceHashState(h,pose);
        VRIK_CaptureModelCache(true);auto replay=native;auto* next=reinterpret_cast<uint8_t*>(replay.data());
        VRIK_ComputeFK(next,VRIK_FKCount());VRIK_ReplayModelCache(next);TraceHashState(h,replay);
    }
}

void TraceSwimmingAndLadder(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    {
        Setup();const auto native=Read<Bone>("player_local.bin");static std::vector<Bone> pose;
        for(int it=0;it<40;++it) {
            pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());VRIK_ComputeFK(buffer,VRIK_FKCount());
            float head[4];r.Quat(head,2.0f,1.2f,.4f);float centre[3];r.Vec(centre,.5f);centre[2]+=1.0f;
            h.B(VRIK_PlaceSwimmingBody(buffer,centre,head,g_VRHeadBoneIdx));TraceHashState(h,pose);
        }
    }
    Setup("ladder");const auto native=Read<Bone>("native_local.bin");static std::vector<Bone> pose;
    for(int it=0;it<60;++it) {
        pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        float head[4];r.Quat(head,1.4f,1.0f,.3f);float camera[3];r.Vec(camera,.25f);
        VRIK_BeginBodySolve(buffer);VRIK_PinGirdleTranslations(buffer);VRIK_ComputeFK(buffer,VRIK_FKCount());
        VRIK_PlaceLadderUpper(buffer,nullptr,head,g_VRHeadBoneIdx);
        for(int k=0;k<3;++k)camera[k]+=g_VRIKBodyEyeSolved[k];
        VRIK_PlaceLadderUpper(buffer,r.Chance(4) ? nullptr:camera,head,g_VRHeadBoneIdx);
        for(int side=0;side<2;++side) {
            const int kind=1+int(r.Next()%3);
            float curls[5],adjust[5][6];
            for(float& c:curls) { const float v=r.Range(-.2f,1.7f);c=v; }
            for(auto& f:adjust)for(float& a:f) { const float v=r.Range(-90,90);a=v; }
            const int form=int(r.Next()%3);
            VRIK_ApplyLadderFingers(buffer,side,kind,form==0 ? nullptr:curls,form==2 ? adjust:nullptr);
        }
        VRIK_SyncShadowUpper(buffer,true);TraceHashState(h,pose);
    }
}

// The cyberware rigs: the combat elbow at full and half blend, the forearm helpers left to the native passes.
void TraceCyberware(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    for(const char* rig:{"gorilla","mantis","launcher"}) {
        Setup(rig);const auto native=Read<Bone>("player_local.bin");static std::vector<Bone> pose;
        CyberpunkVR_SquatSymmetric=0;
        for(int it=0;it<30;++it) {
            TraceReconfigure(it&1);
            pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
            g_VRBind=4;CyberpunkVR_NativeCyberwarePasses=r.Chance(4) ? 0:1;
            shared[115]=0;shared[89]=shared[90]=0;TracePacket();
            float rotation[4];r.Quat(rotation,1.0f,.6f,.2f);
            const float camera[3]={0,0,1.6f},forward[3]={0,1,0};
            VRIK_DampenTorsoWeaponPose(buffer);VRIK_PinGirdleTranslations(buffer);VRIK_ComputeFK(buffer,VRIK_FKCount());
            VRIK_PlaceBodyUnderHMD(buffer,camera,rotation,g_VRHeadBoneIdx,forward);
            float right[3]={1,0,0},up[3]={0,0,1},fwd[3]={0,1,0};
            h.B(VRIK_BodyAxesFromRig(right,up,fwd));
            float anchor[3];std::copy_n(g_fkPos[g_VRNeckIdx],3,anchor);
            TraceArms(r,h,pose,right,up,fwd,anchor);
            VRIK_SyncShadowUpper(buffer,false);TraceHashState(h,pose);
        }
        CyberpunkVR_SquatSymmetric=1;CyberpunkVR_NativeCyberwarePasses=1;
    }
}

// The free helpers, the camera's model frame from random publications, the readers and the arm scaling.
void TraceMisc(TraceHash& h,uint64_t seed) {
    Setup();TraceRng r(seed);
    for(int it=0;it<400;++it) {
        float a[4],b[4],c[4],v[3],w[3],o[4],t[3];
        r.Quat(a,3.1f,1.5f,3.1f);r.Quat(b,3.1f,1.5f,3.1f);r.Quat(c,3.1f,1.5f,3.1f);r.Vec(v,1.5f);r.Vec(w,1.5f);
        const float s=r.Range(.3f,1.6f);for(float& x:c)x*=s;
        VRIK_QuatMul(a,b,o);h.Fs(o,4);VRIK_QuatRotateVec(a,v,t);h.Fs(t,3);VRIK_QuatConj(a,o);h.Fs(o,4);
        std::copy_n(c,4,o);VRIK_QuatNorm(o);h.Fs(o,4);h.F(VRIK_Dot3(v,w));VRIK_Cross3(v,w,t);h.Fs(t,3);
        std::copy_n(v,3,t);h.F(VRIK_Norm3(t));h.Fs(t,3);
        const float frac=r.Range(-.5f,1.5f);VRIK_QuatScale(c,frac,o);h.Fs(o,4);
        float nv[3],nw[3];std::copy_n(v,3,nv);std::copy_n(w,3,nw);VRIK_Norm3(nv);VRIK_Norm3(nw);
        VRIK_QuatFromTo(nv,nw,o);h.Fs(o,4);
        const float neg[3]={-nv[0],-nv[1],-nv[2]};VRIK_QuatFromTo(nv,neg,o);h.Fs(o,4);
        VRIK_TwistAbout(c,nw,o);h.Fs(o,4);
        float right[3],up[3],fwd[3];VRIK_BodyAxesFromCamYaw(a,right,up,fwd);h.Fs(right,3);h.Fs(up,3);h.Fs(fwd,3);
        const float yaw=r.Range(-4,4);VRIK_WorldToModelRotation(yaw,r.Chance(2) ? a:nullptr,o);h.Fs(o,4);
        float target[3],hand[4];const float scale=r.Range(.7f,1.3f);
        VRIK_BuildViewHandTarget(v,a,w,b,scale,nw,target,hand);h.Fs(target,3);h.Fs(hand,4);
        VRIK_BuildHandTarget(v,r.Chance(3) ? nullptr:w,r.Chance(3) ? nullptr:c,nw,b,a,scale,nv,target,hand);
        h.Fs(target,3);h.Fs(hand,4);
        h.B(VRIK_AlignViewToHands(a,b,c,o));h.Fs(o,4);
        // the camera's model frame from a random publication and switches (the direct engine-yaw branch needs a head
        // sample and is left off)
        testLuaAvailable=r.Chance(2);testNativeAvailable=r.Chance(2);
        for(auto* pair:{&testLuaPair,&testNativePair}) {
            float q1[4],q2[4],d1[3],d2[3];r.Quat(q1,3.1f,.8f,.3f);r.Quat(q2,3.1f,.2f,.1f);r.Vec(d1,2);r.Vec(d2,2);
            std::copy_n(q1,4,pair->camQuat);std::copy_n(q2,4,pair->entityQuat);
            std::copy_n(d1,3,pair->cameraMinusEntity);std::copy_n(d2,3,pair->bodyCameraMinusEntity);
            pair->valid=r.Chance(5) ? 0u:1u;pair->unavailable=r.Chance(2) ? 1u:0u;
        }
        CyberpunkVR_VrikNativeFramePair=int(r.Next()%2);CyberpunkVR_VrikTransformsFromPlugin=int(r.Next()%2);
        CyberpunkVR_VrikVehicleFullEntityQuat=int(r.Next()%2);g_isInVehicle=r.Chance(3);
        CyberpunkVR_CamWriteInPatch=int(r.Next()%2);g_VRCamPosValid=int(r.Next()%2);g_VRCamPairValid=int(r.Next()%2);
        r.Vec(v,3);g_VRCamPosX=v[0];g_VRCamPosY=v[1];g_VRCamPosZ=v[2];
        g_VREntityQI=a[0];g_VREntityQJ=a[1];g_VREntityQK=a[2];g_VREntityQR=a[3];
        g_VRCamI=b[0];g_VRCamJ=b[1];g_VRCamK=b[2];g_VRCamR=b[3];
        float pos[3]{},rot[4]{},entity[4]{},paired[4]{},base[3]{};VrikCameraStatus status{};
        h.B(VRIK_ComputeCamModel(pos,rot,entity,paired,base,&status));
        h.Fs(pos,3);h.Fs(rot,4);h.Fs(entity,4);h.Fs(paired,4);h.Fs(base,3);h.I(static_cast<int>(status));
        const int bone=int(r.Next()%static_cast<uint64_t>(g_VRBoneCount+4))-2;
        float p[3]{},q[4]{};h.B(VRIK_ReferenceLocal(bone,p,q));h.Fs(p,3);h.Fs(q,4);
        h.B(VRIK_ReferenceModel(bone,p,q));h.Fs(p,3);h.Fs(q,4);h.B(VRIK_ReferenceFingerRotation(bone,q));h.Fs(q,4);
        const int side=int(r.Next()%3)-0,slot=int(r.Next()%21)-1,digit=int(r.Next()%6)-1;
        h.I(VRIK_LadderFingerBone(side,slot));
        int spreadBone=-7;float axis[3]{};
        h.B(cvr::gestures::ThumbReference(side,q));h.Fs(q,4);
        h.B(cvr::gestures::SpreadAxis(side,digit,&spreadBone,axis));h.I(spreadBone);h.Fs(axis,3);
    }
    cvr::camera::AnchorVector mount{},eyes{};
    h.B(cvr::camera::ReadNeckCameraMount(&mount));h.B(cvr::camera::ReadNeckCameraEyeOffset(&eyes));
    h.F(mount.x);h.F(mount.y);h.F(mount.z);h.F(eyes.x);h.F(eyes.y);h.F(eyes.z);
    VrikFbtReference fbt{};h.B(VRIK_ReadFbtReference(&fbt));
    h.Fs(fbt.eye,3);h.Fs(fbt.eyeInHead,3);h.Fs(fbt.hipsPos,3);h.Fs(fbt.hipsRot,4);h.Fs(fbt.headPos,3);h.Fs(fbt.headRot,4);
    for(int s=0;s<2;++s) { h.Fs(fbt.footPos[s],3);h.Fs(fbt.footRot[s],4);h.Fs(fbt.upperArmPos[s],3);h.Fs(fbt.foreArmPos[s],3);h.Fs(fbt.handPos[s],3); }
    // the arm's segments to a measured player and back
    auto pose=Read<Bone>("player_local.bin");auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    for(int it=0;it<40;++it) {
        const float length=r.Range(.3f,1.0f);const bool left=r.Chance(2);
        const int upper=left ? g_VRLeftUpperArmIdx:g_VRRightUpperArmIdx,fore=left ? g_VRLeftForeArmIdx:g_VRRightForeArmIdx;
        const int hand=left ? g_VRLeftBoneIdx:g_VRRightBoneIdx;
        const uintptr_t track=r.Chance(8) ? 0x1000u:0x2000u;
        VRIK_ScaleArmBonesFromRest(buffer,track,g_VRBoneCount,fore,hand,length);TraceHashState(h,pose);
        if(r.Chance(3)) { VRIK_RestoreArmRestTrans(buffer,track,g_VRBoneCount,upper,fore,hand,left);TraceHashState(h,pose); }
    }
    // the legacy hand write
    for(int it=0;it<40;++it) {
        float headPos[3],headQ[4],vrPos[3],vrQ[4];r.Vec(headPos,1);r.Quat(headQ,3,1,1);r.Vec(vrPos,1);r.Quat(vrQ,3,1,1);
        g_VRBindAxis=int(r.Next()%6);g_VRUseHeadRelative=int(r.Next()%2);g_VRBindScale=r.Range(.5f,1.5f);
        const bool ok=r.Chance(2),writeRot=r.Chance(2);
        VRIK_WriteHand(buffer,24,headPos,headQ,ok,vrPos,vrQ,writeRot);h.Fs(pose[24].p,4);h.Fs(pose[24].q,4);
    }
    g_VRUseHeadRelative=0;g_VRBindScale=1;g_VRBindAxis=0;
}

// The shared block's latch, the view packet with its snap-event rotation, the view's position and the head residual.
void TraceSnapshot(TraceHash& h,uint64_t seed) {
    Setup();TraceRng r(seed);
    std::array<float,256> block{};g_pSharedHands=block.data();
    auto& seq=*reinterpret_cast<uint32_t*>(&block[127]);auto& viewSeq=*reinterpret_cast<uint32_t*>(&block[143]);
    seq=0;viewSeq=0;
    for(int it=0;it<3000;++it) {
        for(int i:{68,104,105,106,107,108,109,110,111,112,113,114,115,124,125,126,141,142,146,147,148,218,219,220,227,228,229,230}) {
            const float v=r.Range(-1.6f,1.6f);block[i]=v;
        }
        const int flag=int(r.Next()%4),pair=int(r.Next()%3),ok=int(r.Next()%3);
        block[111]=float(flag);block[115]=float(pair);block[142]=ok ? 1.0f:0.0f;
        const bool snap=r.Chance(4);const float counter=float(it/7);block[147]=snap ? counter:block[149];
        if(r.Chance(3))block[148]=block[141]-r.Range(-.03f,.03f);
        const int seqStep=int(r.Next()%4);seq+=uint32_t(seqStep);   // odd = a write in progress
        const int viewStep=int(r.Next()%3);viewSeq+=uint32_t(viewStep*2);
        if(r.Chance(10))viewSeq=0;
        RefreshHandsSnapshot();
        for(int i=0;i<128;++i)h.F(SharedPose(i));
        VRIK_LatchViewPacket();h.Fs(g_viewPkt,17);h.B(g_viewPktValid);h.F(block[149]);
        g_VRCamPosValid=int(r.Next()%3!=0);g_VRCamPairValid=int(r.Next()%2);
        float cam[3];r.Vec(cam,40);g_VRCamPosX=cam[0];g_VRCamPosY=cam[1];g_VRCamPosZ=cam[2];
        float pair3[3];r.Vec(pair3,.4f);g_VRCamPairLocalX=pair3[0];g_VRCamPairLocalY=pair3[1];g_VRCamPairLocalZ=pair3[2];
        float out[3]{};h.B(VRIK_ResolveViewPos(out));h.Fs(out,3);h.F(g_vrikViewScaleUsed);
        float camModel[3],entity[4];r.Vec(camModel,2);r.Quat(entity,3.1f,.2f,.1f);
        h.B(VRIK_ResolveViewModelPos(camModel,entity,out));h.Fs(out,3);
        const float fr=r.Range(-.3f,.3f),ff=r.Range(-.3f,.3f);VRIK_HeadResidual(fr,ff,out);h.Fs(out,3);
        h.B(VRIK_IsReadable(block.data(),sizeof(float)*256));h.B(VRIK_IsReadable(nullptr,4));
        float v[3],o[3];r.Vec(v,2);VRIK_RemapAxis(int(r.Next()%7)-1,v,o);h.Fs(o,3);
        VRIK_RemapAxis(int(r.Next()%6),v,v);h.Fs(v,3);   // in place, as a caller passing one array would
    }
    g_pSharedHands=shared.data();
}

// The finger layers: the resting hands captured on request and replayed, the reload's fingers blended onto the live
// ones, the right hand's grip learned and offered back while the left hand carries.
extern "C" {
extern int CyberpunkVR_RestFingerCaptureReq,CyberpunkVR_RestFingerForce,CyberpunkVR_RestFingerForceR,CyberpunkVR_CarryLeft;
extern int CyberpunkVR_DebugRestGate,CyberpunkVR_DebugRestBones,CyberpunkVR_DebugRestGateR,CyberpunkVR_DebugRestBonesR;
extern int CyberpunkVR_DebugRestFingerRefused,CyberpunkVR_DebugRestFingerRefusedR,CyberpunkVR_DebugRestFingerSaved;
extern int CyberpunkVR_DebugRestFingerSavedR,CyberpunkVR_DebugCarryGripBones,CyberpunkVR_CarryGripApply;
extern float CyberpunkVR_CarryGripBlend;
extern volatile uint64_t CyberpunkVR_DualWieldUntil;
}
void TraceFingers(TraceHash& h,uint64_t seed) {
    Setup();TraceRng r(seed);
    const auto rest=Read<Bone>("reference_full.bin");static std::vector<Bone> pose;
    g_VRSmokeFingerCount=g_VRSmokeFingerCountL=19;
    for(int side=0;side<2;++side)for(int k=0;k<19;++k) {
        const std::string name=std::string(side?"Right":"Left")+cvr::gestures::NativeFist[k].suffix;
        const int bone=int(std::find(names.begin(),names.end(),name)-names.begin());
        (side?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k]=bone;
        std::strncpy((side?g_VRSmokeFingerName:g_VRSmokeFingerNameL)[k],name.c_str(),47);
    }
    for(int it=0;it<600;++it) {
        pose=rest;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        for(int k=0;k<38;++k) {   // the animation's own fingers this pass
            const int bone=k<19 ? g_VRSmokeFingerIdxL[k]:g_VRSmokeFingerIdx[k-19];
            float q[4];r.Quat(q,1.0f,1.0f,1.0f);std::copy_n(q,4,pose[bone].q);
        }
        g_hasWeaponEquipped=r.Chance(2);g_isInVehicle=r.Chance(8);
        g_VRReloadFingerActive[0]=r.Chance(5);g_VRReloadFingerActive[1]=r.Chance(5);
        g_VRSmokeFingerActive=r.Chance(6);g_VRSmokeFingerActiveL=r.Chance(6);
        CyberpunkVR_RestFingerCaptureReq=r.Chance(6);CyberpunkVR_RestFingerCaptureReqR=r.Chance(6);
        CyberpunkVR_RestFingerForce=r.Chance(5);CyberpunkVR_RestFingerForceR=r.Chance(5);
        CyberpunkVR_CarryLeft=r.Chance(3);CyberpunkVR_CarryGripApply=!r.Chance(6);
        g_VRRestFingerApply=!r.Chance(5);CyberpunkVR_RestFingerApplyR=!r.Chance(5);
        CyberpunkVR_DualWieldUntil=r.Chance(6) ? ~0ull:0ull;
        const float carry=r.Range(-.2f,1.3f);CyberpunkVR_CarryGripBlend=carry;
        for(int side=0;side<2;++side) {
            const float blend=r.Chance(3) ? 1.0f:r.Range(-.1f,1.1f);g_VRReloadFingerBlend[side]=blend;
            for(int k=0;k<32;++k) {
                g_VRReloadFingerSet[side][k]=r.Chance(2);float q[4];r.Quat(q,3.1f,1.5f,3.1f);
                std::copy_n(q,4,g_VRReloadFingerRot[side][k]);
            }
        }
        const bool force=r.Chance(4),forceR=r.Chance(4);
        cvr::anim::VrikRestFingerPose(buffer,force);
        cvr::anim::VrikRestFingerPoseRight(buffer,forceR);
        cvr::anim::VrikCarryGripPoseRight(buffer);
        cvr::anim::VrikReloadFingerPose(buffer);
        if(r.Chance(10)) { cvr::anim::RestFingerTick();cvr::anim::RestFingerRightTick(); }
        for(int k=0;k<38;++k) { const int bone=k<19 ? g_VRSmokeFingerIdxL[k]:g_VRSmokeFingerIdx[k-19];h.Fs(pose[bone].q,4); }
        for(int k=0;k<32;++k)h.Fs(g_VRRestFingerRot[k],4);
        h.I(g_VRRestFingerHave);h.I(g_VRRestFingerCount);h.I(CyberpunkVR_DebugRestGate);h.I(CyberpunkVR_DebugRestBones);
        h.I(CyberpunkVR_DebugRestGateR);h.I(CyberpunkVR_DebugRestBonesR);h.I(CyberpunkVR_DebugRestFingerRefused);
        h.I(CyberpunkVR_DebugRestFingerRefusedR);h.I(CyberpunkVR_DebugCarryGripBones);
        h.I(CyberpunkVR_DebugRestFingerSaved);h.I(CyberpunkVR_DebugRestFingerSavedR);
    }
    g_hasWeaponEquipped=false;g_isInVehicle=false;CyberpunkVR_CarryLeft=0;CyberpunkVR_DualWieldUntil=0;
    g_VRReloadFingerActive[0]=g_VRReloadFingerActive[1]=0;g_VRSmokeFingerActive=g_VRSmokeFingerActiveL=0;
}

// The hands' layers on the trace's own clock (g_VRIKClockMs), so their easing is deterministic.
uint64_t g_traceClockMs=1000000;
uint64_t TraceClockMs() { return g_traceClockMs+=7; }
extern "C" {
extern float CyberpunkVR_GestureClosed[2][5],CyberpunkVR_GestureStraight[2][5];
extern int CyberpunkVR_FingerBones;
}
// The finger gestures and the arm cyberware over the plain, gorilla, mantis and launcher rigs.
void TraceGestures(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockMs=TraceClockMs;
    for(const char* rig:{"","gorilla","mantis","launcher"}) {
        Setup(rig);const auto rest=Read<Bone>("reference_full.bin");static std::vector<Bone> pose;
        g_VRSmokeFingerCount=g_VRSmokeFingerCountL=19;
        for(int side=0;side<2;++side)for(int k=0;k<19;++k) {
            const std::string name=std::string(side?"Right":"Left")+cvr::gestures::NativeFist[k].suffix;
            const int bone=int(std::find(names.begin(),names.end(),name)-names.begin());
            (side?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k]=bone;
            std::strncpy((side?g_VRSmokeFingerName:g_VRSmokeFingerNameL)[k],name.c_str(),47);
            if(!side)std::copy_n(rest[bone].q,4,g_VRRestFingerRot[k]);
        }
        g_VRRestFingerCount=19;g_VRRestFingerHave=1;
        const char* trackNames[]={"x","leftWireSlideFactor","y","rightWireSlideFactor"};float trackReference[4]{};
        VRIK_ConfigureCyberwareTracks(trackNames,4,trackReference);
        for(int it=0;it<160;++it) {
            pose=rest;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
            for(int k=0;k<38;++k) {
                const int bone=k<19 ? g_VRSmokeFingerIdxL[k]:g_VRSmokeFingerIdx[k-19];
                float q[4];r.Quat(q,.6f,.6f,.6f);float out[4];VRIK_QuatMul(q,rest[bone].q,out);std::copy_n(out,4,pose[bone].q);
            }
            VRIK_ComputeFK(buffer,VRIK_FKCount());
            if(!r.Chance(5))++g_VrikFrameEpoch;
            g_VRBind=r.Chance(12) ? 3:4;g_isInVehicle=r.Chance(15);
            const float lg=r.Range(-.1f,1.1f),rg=r.Range(-.1f,1.1f),lt=r.Range(-.1f,1.1f),rt=r.Range(-.1f,1.1f);
            const bool thumb=r.Chance(3),enabled=!r.Chance(10);
            if(!r.Chance(8))cvr::gestures::PublishInput(lg,rg,lt,rt,thumb,enabled);
            if(!r.Chance(8))cvr::gestures::PublishEquipment(int(r.Next()%4));
            if(r.Chance(3))cvr::cyberware::Select(int(r.Next()%7)-1);
            cvr::cyberware::PublishGrips(r.Chance(2),r.Chance(2));
            if(r.Chance(6))cvr::cyberware::MusclePulse(int(r.Next()%2),r.Range(-.1f,1.8f));
            if(r.Chance(5))g_traceClockMs+=r.Next()%300;
            CyberpunkVR_CarryLeft=r.Chance(8);g_VRReloadFingerActive[0]=r.Chance(9);g_VRSmokeFingerActive=r.Chance(9);
            testCyberwareWorldFrame=!r.Chance(4);
            float tracks[4]={.25f,.5f,.75f,.125f};
            if(VRIK_UsesNativeCyberware())VRIK_ApplyCyberwarePose(buffer,tracks,4);
            else cvr::gestures::Apply(buffer);
            for(const auto& b:pose) { h.Fs(b.p,4);h.Fs(b.q,4);h.Fs(b.s,4); }
            h.Fs(tracks,4);
            for(int side=0;side<2;++side) {
                h.I(CyberpunkVR_GestureMode[side]);h.Fs(CyberpunkVR_GestureClosed[side],5);h.Fs(CyberpunkVR_GestureStraight[side],5);
                h.B(cvr::gestures::OwnsHand(side));h.B(cvr::gestures::PortHolds(side));h.F(cvr::cyberware::WireDeployment(side));
            }
            h.I(cvr::cyberware::SelectedKind());
            cvr::cyberware::Frame frame{};const bool have=cvr::cyberware::ReadFrame(frame);h.B(have);
            if(have) {
                h.I(frame.family);h.I(frame.count[0]);h.I(frame.count[1]);h.I(frame.epoch);h.I(int64_t(frame.stamp));
                for(int side=0;side<2;++side) {
                    for(const auto& p:frame.points[side]) { h.F(p.x);h.F(p.y);h.F(p.z); }
                    h.F(frame.wrists[side].x);h.F(frame.wrists[side].y);h.F(frame.wrists[side].z);
                    h.Fs(frame.handRotation[side],4);h.F(frame.deployment[side]);
                }
                h.F(frame.bodyOrigin.x);h.F(frame.bodyOrigin.y);h.F(frame.bodyOrigin.z);
            }
            float origin[3]{},direction[3]{};h.B(cvr::cyberware::LauncherLine(origin,direction));h.Fs(origin,3);h.Fs(direction,3);
            float info[4];cvr::cyberware::CollisionInfo(info);h.Fs(info,4);
            const float weights[4]={r.Range(0,1),r.Range(0,1),r.Range(0,1),r.Range(0,1)};
            float q[4];r.Quat(q,3,1.5f,3);
            cvr::gestures::ApplyCalibration(int(r.Next()%3),cvr::gestures::NativeFist[r.Next()%19].suffix,
                {weights[0],weights[1],weights[2],weights[3]},q);
            h.Fs(q,4);
        }
    }
    testCyberwareWorldFrame=false;g_VRBind=4;g_isInVehicle=false;CyberpunkVR_CarryLeft=0;
    g_VRReloadFingerActive[0]=0;g_VRSmokeFingerActive=0;g_VRIKClockMs=nullptr;
}

// The monowire's line stepped on the same random anchors, swings, grips, plane contacts and carries.
#ifdef CVR_RUST_VRIK
extern "C" void cvr_cyberware_wire_step(cvr::cyberware::Wire* wire,const float* anchor,const float* direction,bool grip,
                                        float dt,float length,const float* carry);
#endif
static_assert(sizeof(cvr::cyberware::Wire)==2440,"the wire's layout is shared with rust/crates/cvr-rig/src/wire.rs");
void TraceWire(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    static cvr::cyberware::Wire wire;
    for(int it=0;it<1500;++it) {
        if(r.Chance(300))wire.Reset();
        float a[3],d[3],c[3];r.Vec(a,1.5f);r.Vec(d,1.0f);r.Vec(c,.08f);
        if(r.Chance(4)) { a[0]=wire.lastAnchor.x+a[0]*.02f;a[1]=wire.lastAnchor.y+a[1]*.02f;a[2]=wire.lastAnchor.z+a[2]*.02f; }
        const bool grip=!r.Chance(4);const float dt=r.Chance(30) ? r.Range(.1f,.3f):r.Range(0,.04f);
        const float length=r.Range(.1f,9.f);
        if(r.Chance(5))for(int i=0;i<cvr::cyberware::Wire::Count;++i) {
            float p[3],n[3];r.Vec(p,2);r.Vec(n,1);float len=std::sqrt(n[0]*n[0]+n[1]*n[1]+n[2]*n[2]);if(len<1e-3f)len=1;
            wire.planes[i]={{p[0],p[1],p[2]},{n[0]/len,n[1]/len,n[2]/len},r.Chance(2)};
        }
#ifdef CVR_RUST_VRIK
        cvr_cyberware_wire_step(&wire,a,d,grip,dt,length,c);
#else
        wire.Step({a[0],a[1],a[2]},{d[0],d[1],d[2]},grip,dt,length,{c[0],c[1],c[2]});
#endif
        for(int i=0;i<cvr::cyberware::Wire::Count;++i) {
            h.F(wire.point[i].x);h.F(wire.point[i].y);h.F(wire.point[i].z);
            h.F(wire.velocity[i].x);h.F(wire.velocity[i].y);h.F(wire.velocity[i].z);
        }
        h.F(wire.deployed);h.F(wire.swingTension);h.B(wire.valid);h.F(wire.motion.x);h.F(wire.motion.y);h.F(wire.motion.z);
    }
}

// The weapon hand: the rig's mirror between the hands, head aim, the ADS pose moved onto the sighting eye, the non-VRIK
// muzzle stabilizer and the recorder's skeleton publish, on random flags, packets and poses.
extern "C" {
extern int CyberpunkVR_DebugLeftHandedTwins,CyberpunkVR_NonVrikAdsStabilizer;
extern float CyberpunkVR_DebugLeftHandedResidual,CyberpunkVR_DebugFingerFlex[4];
}
void TraceWeaponArm(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    for(const char* rig:{"","weapon_stance","gorilla"}) {
        Setup(rig);
        std::vector<const char*> pointers;for(const auto& n:names)pointers.push_back(n.c_str());
        cvr::anim::LeftHandedArmRig(pointers.data(),static_cast<int>(pointers.size()));
        h.I(CyberpunkVR_DebugLeftHandedTwins);h.F(CyberpunkVR_DebugLeftHandedResidual);
        auto find=[&](const char* n){const auto it=std::find(names.begin(),names.end(),n);return it==names.end()?-1:int(it-names.begin());};
        g_VRSmokeCigIdx=find("WeaponRight");g_VRSmokeLighterIdx=find("WeaponLeft");
        const auto rest=Read<Bone>(std::string(rig)=="weapon_stance" ? "shotgun-2.bin":"reference_full.bin");
        static std::vector<Bone> pose;
        std::array<float,256> block{};g_pSharedHands=block.data();
        for(int it=0;it<200;++it) {
            pose=rest;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
            for(int b:{g_VRRightUpperArmIdx,g_VRRightForeArmIdx,g_VRRightBoneIdx,g_VRLeftUpperArmIdx,g_VRLeftForeArmIdx,
                       g_VRLeftBoneIdx,g_VRSmokeCigIdx,g_VRSmokeLighterIdx})if(b>=0) {
                float q[4];r.Quat(q,.5f,.5f,.5f);float out[4];VRIK_QuatMul(q,rest[b].q,out);std::copy_n(out,4,pose[b].q);
            }
            g_hasLeftWeaponEquipped=r.Chance(2);g_hasRightWeaponEquipped=r.Chance(2);CyberpunkVR_CarryLeft=r.Chance(6);
            CyberpunkVR_DualWieldUntil=r.Chance(3) ? ~0ull:0ull;
            h.B(cvr::anim::WeaponInLeftHand());h.I(cvr::anim::HeldWeaponBone());h.B(cvr::anim::TwoGunsInHand());
            float p[3],q[4];r.Vec(p,.4f);r.Quat(q,3,1.5f,3);
            cvr::anim::MirrorHeldHand(p,q);h.Fs(p,3);h.Fs(q,4);
            int right=-9;float out[4]{};const int finger=int(r.Next()%static_cast<uint64_t>(g_VRBoneCount));
            h.B(cvr::anim::MirrorFingerToRight(finger,q,&right,out));h.I(right);h.Fs(out,4);
            float wp[3]{},wq[4]{};h.B(cvr::anim::MirrorWeaponSlotToRight(p,q,wp,wq));h.Fs(wp,3);h.Fs(wq,4);
            // the shared block and the view packet the aim layers read
            for(int i:{24,25,26}) { const float v=r.Range(-1,1);block[i]=v; }
            block[27]=r.Chance(5) ? 0.f:1.f;block[58]=r.Chance(2) ? 0.f:1.f;block[144]=r.Chance(5) ? 0.f:1.f;
            block[95]=r.Range(.025f,.04f);block[111]=2;block[115]=float(r.Next()%3);
            for(int i:{112,113,114}) { const float v=r.Range(-.1f,.1f);block[i]=v; }
            if(!r.Chance(4))block[99]+=1;
            auto& seq=*reinterpret_cast<uint32_t*>(&block[127]);seq+=2;RefreshHandsSnapshot();
            float vq[4],hq[4];r.Quat(vq,3.1f,.7f,.2f);r.Quat(hq,1.5f,.7f,.2f);
            std::copy_n(vq,4,g_viewPkt);std::copy_n(hq,4,g_viewPkt+13);g_viewPkt[7]=2;
            const float heading=r.Range(-3.1f,3.1f);g_viewPkt[8]=heading;g_viewPktValid=!r.Chance(8);
            float eq[4];r.Quat(eq,3.1f,.05f,.05f);g_VREntityQI=eq[0];g_VREntityQJ=eq[1];g_VREntityQK=eq[2];g_VREntityQR=eq[3];
            float cam[3];r.Vec(cam,20);g_VRCamPosX=cam[0];g_VRCamPosY=cam[1];g_VRCamPosZ=cam[2]+1.6f;g_VRCamPosValid=1;
            g_VREntityPosX=cam[0];g_VREntityPosY=cam[1];g_VREntityPosZ=cam[2];g_VRCamPairValid=0;
            testLuaAvailable=true;testNativeAvailable=false;
            CyberpunkVR_VrikNativeFramePair=0;CyberpunkVR_VrikTransformsFromPlugin=1;g_isInVehicle=false;
            std::copy_n(vq,4,testLuaPair.camQuat);std::copy_n(eq,4,testLuaPair.entityQuat);
            testLuaPair.cameraMinusEntity[0]=0;testLuaPair.cameraMinusEntity[1]=0;testLuaPair.cameraMinusEntity[2]=1.6f;
            testLuaPair.valid=1;testLuaPair.unavailable=0;
            g_isAiming=!r.Chance(3);g_VRBind=r.Chance(3) ? 4:0;CyberpunkVR_NonVrikAdsStabilizer=!r.Chance(10);
            g_VRWeaponPsmState=r.Chance(3) ? 5.0f:r.Range(0,8);g_VRWeaponRaiseTransition=r.Chance(5);
            g_VRAimInRemaining=r.Chance(2) ? 0.f:r.Range(0,.3f);
            VRIK_LatchViewPacket();
            VRIK_ComputeFK(buffer,VRIK_FKCount());
            cvr::anim::NoteFingerFlex(buffer,false);
            cvr::anim::PrepareAimArmTargets(buffer);
            cvr::anim::ApplyHeadAimWeaponOrientation(buffer);
            cvr::anim::ApplyNonVrikAdsMuzzleStabilizer(buffer);
            cvr::anim::SolvePreparedAimArms(buffer);
            cvr::anim::NoteFingerFlex(buffer,true);
            h.B(cvr::anim::IsHeadAimWeaponActive());h.Fs(CyberpunkVR_DebugFingerFlex,4);
            for(const auto& b:pose) { h.Fs(b.p,4);h.Fs(b.q,4); }
            if(r.Chance(4)) { cvr::anim::VrikPublishAnimatedFk(buffer);h.I(g_VRFKSnapCount);
                for(int i=0;i<g_VRFKSnapCount;++i) { h.Fs(g_VRFKSnapPos[i],3);h.Fs(g_VRFKSnapRot[i],4); } }
        }
        g_pSharedHands=shared.data();
    }
    g_hasLeftWeaponEquipped=g_hasRightWeaponEquipped=g_isAiming=false;CyberpunkVR_CarryLeft=0;CyberpunkVR_DualWieldUntil=0;
    g_VRBind=4;g_VRSmokeCigIdx=g_VRSmokeLighterIdx=-1;testLuaAvailable=false;g_viewPktValid=false;
}

// The smoking poses and the native reload's arms, on the trace's own counter.
int64_t g_traceQpc=5000000;
int64_t TraceClockQpc() { return g_traceQpc+=4321; }
extern "C" {
extern int CyberpunkVR_VrikReloadHandOff;
extern float CyberpunkVR_ReloadHandOffStats[8],CyberpunkVR_ReloadHandOffInSec,CyberpunkVR_ReloadHandOffOutSec;
}
void TraceSmokeReload(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockQpc=TraceClockQpc;
    Setup("weapon_stance");
    auto find=[&](const char* n){const auto it=std::find(names.begin(),names.end(),n);return it==names.end()?-1:int(it-names.begin());};
    g_VRSmokeCigIdx=find("WeaponRight");g_VRSmokeLighterIdx=find("WeaponLeft");g_VRSmokeMouthBoneIdx=find("WeaponRight1");
    g_VRSmokeFingerCount=g_VRSmokeFingerCountL=19;
    for(int side=0;side<2;++side)for(int k=0;k<19;++k) {
        const std::string name=std::string(side?"Right":"Left")+cvr::gestures::NativeFist[k].suffix;
        (side?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k]=find(name.c_str());
        if(!side)g_VRSmokeThumbIsL[k]=cvr::gestures::NativeFist[k].group==0;
    }
    std::vector<const char*> pointers;for(const auto& n:names)pointers.push_back(n.c_str());
    cvr::anim::LeftHandedArmRig(pointers.data(),static_cast<int>(pointers.size()));
    const auto rest=Read<Bone>("shotgun-2.bin");static std::vector<Bone> pose;
    std::array<float,256> block{};g_pSharedHands=block.data();
    for(int it=0;it<700;++it) {
        pose=rest;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        for(auto& b:pose) { float q[4];r.Quat(q,.3f,.3f,.3f);float out[4];VRIK_QuatMul(q,b.q,out);std::copy_n(out,4,b.q); }
        g_VRSmokeFingerCapture=r.Chance(7);g_VRSmokeFingerCaptureL=r.Chance(7);
        g_VRSmokeFingerActive=!r.Chance(4);g_VRSmokeFingerActiveL=!r.Chance(4);
        g_VRSmokeRightLighter=r.Chance(4);g_VRSmokeLeftUseCig=r.Chance(3);g_VRSmokeCigEnable=!r.Chance(5);
        g_VRSmokeLighterEnable=!r.Chance(5);g_VRSmokeMouthAnchor=r.Chance(2);g_VRSmokeAnchorValid=r.Chance(2);
        for(int k=0;k<3;++k) {
            const float a=r.Range(-.05f,.05f),b=r.Range(-.05f,.05f),c=r.Range(-.2f,.2f);
            g_VRSmokeCigOffP[k]=a;g_VRSmokeLighterOffP[k]=b;g_VRSmokeAnchorLocalPos[k]=c;
        }
        float q1[4],q2[4],q3[4],q4[4];r.Quat(q1,.4f,.4f,.4f);r.Quat(q2,.4f,.4f,.4f);r.Quat(q3,3,1,3);r.Quat(q4,1.5f,.5f,.5f);
        for(int k=0;k<4;++k) { g_VRSmokeCigOffQ[k]=q1[k];g_VRSmokeLighterOffQ[k]=q2[k];g_VRSmokeAnchorLocalRot[k]=q3[k];g_VRSmokeThumbFlickL[k]=q4[k]; }
        g_VRSmokeCigScaleY=r.Range(.5f,1.5f);g_VRSmokeThumbPressManualL=r.Chance(4) ? r.Range(0,1):0.f;
        block[154]=r.Range(-.2f,1.2f);block[160]=r.Range(-.2f,1.2f);
        cvr::anim::VrikSmokingCigPose(buffer);
        cvr::anim::VrikSmokingLighterPose(buffer);
        for(const auto& b:pose) { h.Fs(b.p,4);h.Fs(b.q,4);h.Fs(b.s,4); }
        h.I(g_VRSmokeFingerHave);h.I(g_VRSmokeFingerHaveL);h.I(g_VRSmokeCigHave);h.I(g_VRSmokeLighterHave);h.I(g_VRSmokeCigLHave);
        h.Fs(g_VRSmokeCigPos,3);h.Fs(g_VRSmokeCigRot,4);h.Fs(g_VRSmokeLighterPos,3);h.Fs(g_VRSmokeLighterRot,4);
        // the reload's arms
        if(r.Chance(15)) { const bool on=!r.Chance(3);const float fin=r.Chance(4) ? -1.f:r.Range(.2f,3.f);cvr::anim::ReloadHandOffSet(on,fin); }
        g_hasWeaponEquipped=!r.Chance(6);CyberpunkVR_VrikReloadHandOff=!r.Chance(10);
        CyberpunkVR_ReloadHandOffInSec=r.Chance(10) ? 0.f:.2f;CyberpunkVR_ReloadHandOffOutSec=r.Chance(10) ? 0.f:.3f;
        if(r.Chance(5))g_traceQpc+=int64_t(r.Next()%400000);
        VRIK_ComputeFK(buffer,VRIK_FKCount());
        cvr::anim::ReloadHandOffCapture();
        cvr::anim::ReloadHandOffUpdate();
        h.F(cvr::anim::ReloadHandOffWeight());
        for(int side=0;side<2;++side) {
            float target[3],rot[4];r.Vec(target,1);r.Quat(rot,3,1.5f,3);
            cvr::anim::ReloadHandOffBlendTarget(side,target,rot);h.Fs(target,3);h.Fs(rot,4);
        }
        h.Fs(CyberpunkVR_ReloadHandOffStats,8);
    }
    g_pSharedHands=shared.data();g_VRIKClockQpc=nullptr;g_hasWeaponEquipped=false;
    g_VRSmokeFingerActive=g_VRSmokeFingerActiveL=0;g_VRSmokeCigIdx=g_VRSmokeLighterIdx=g_VRSmokeMouthBoneIdx=-1;
}

// The weapon's bones (src/Anim/WeaponRig.cpp): rig objects, remap tables and pose descriptors in the engine's layout
// (rig +0x50 names, +0x58 count, +0xA0 tracks; a2[6] +0x40/+0x50 bone remap and LOD counts, +0x60/+0x70 the tracks';
// poseDesc +0x08 slots, +0x20 track capacity), signatures and part writes drawn at random, every pass hashed.
void TraceWeaponRig(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    constexpr int kRigs=4,kSlots=22,kTrackCap=12,kLods=6,kRow=kTrackCap+4;
    static uint64_t rigNames[kRigs][16];
    const uint32_t bones[kRigs]={11,5,7,11};
    for(auto& rig:rigNames)for(auto& n:rig)n=r.Next();
    alignas(16) static uint8_t rigObj[kRigs][0xB0];
    for(int i=0;i<kRigs;++i) {
        std::memset(rigObj[i],0,sizeof(rigObj[i]));
        const uint64_t* p=rigNames[i];std::memcpy(rigObj[i]+0x50,&p,8);std::memcpy(rigObj[i]+0x58,&bones[i],4);
        const uint32_t tracks=uint32_t(r.Next()%10);std::memcpy(rigObj[i]+0xA0,&tracks,4);
    }
    // Signatures: the first match wins; decoys share a bone count with a wrong name, or name a bone past the rig.
    g_RigSigN=0;
    auto addSig=[&](int which,int rig,int wrong,bool pastEnd) {
        const int n=g_RigSigN;g_RigSigWhich[n]=which;g_RigSigBones[n]=bones[rig];
        for(int k=0;k<VRRIG_SIG_NAMES;++k) {
            const bool used=k<3 && !r.Chance(5);const int bi=int(r.Next()%bones[rig]);
            g_RigSigIdx[n][k]=used ? bi:-1;g_RigSigHash[n][k]=used ? rigNames[rig][bi]:0;
        }
        if(wrong>=0) { g_RigSigIdx[n][wrong]=0;g_RigSigHash[n][wrong]=rigNames[rig][0]^1; }
        if(pastEnd)g_RigSigIdx[n][1]=int(bones[rig])+2;
        g_RigSigN=n+1;
    };
    addSig(1,0,2,false);addSig(1,0,-1,true);addSig(0,0,-1,false);addSig(1,1,-1,false);addSig(2,2,-1,false);
    addSig(2,3,-1,false);addSig(0,3,1,false);
    // The components' pose setups.
    static uint32_t remapPairs[3][48],trackPairs[3][32];static uint16_t remapCounts[3][kLods],trackCounts[3][kLods];
    alignas(16) static uint8_t six[3][0x80];
    for(int c=0;c<3;++c) {
        for(int e=0;e<24;++e) { const uint32_t s=uint32_t(r.Next()%24),d=uint32_t(r.Next()%(kSlots+3));remapPairs[c][e*2]=s;remapPairs[c][e*2+1]=d; }
        for(int e=0;e<16;++e) { const uint32_t s=uint32_t(r.Next()%18),d=uint32_t(r.Next()%(kTrackCap+2));trackPairs[c][e*2]=s;trackPairs[c][e*2+1]=d; }
        for(int l=0;l<kLods;++l) { const uint16_t a=uint16_t(r.Next()%25),b=uint16_t(r.Next()%17);remapCounts[c][l]=a;trackCounts[c][l]=b; }
        std::memset(six[c],0,sizeof(six[c]));
        const uint32_t* rp=remapPairs[c];const uint16_t* rc=remapCounts[c];const uint32_t* tp=trackPairs[c];const uint16_t* tc=trackCounts[c];
        std::memcpy(six[c]+0x40,&rp,8);std::memcpy(six[c]+0x50,&rc,8);
        if(c!=2) { std::memcpy(six[c]+0x60,&tp,8);std::memcpy(six[c]+0x70,&tc,8); }
    }
    alignas(16) static float bonePool[kSlots*12];alignas(16) static float trackPool[4][kRow];
    alignas(16) static uint8_t poseDesc[0x30];
    for(int it=0;it<3000;++it) {
        for(int s=0;s<kSlots;++s) {
            float* b=bonePool+s*12;const float range=r.Chance(8) ? 2.f:.5f;
            float v[3];r.Vec(v,range);float q[4];r.Quat(q,3,1.5f,3);
            for(int k=0;k<3;++k)b[k]=v[k];b[3]=1;for(int k=0;k<4;++k)b[4+k]=q[k];for(int k=0;k<3;++k)b[8+k]=1;b[11]=0;
        }
        for(auto& row:trackPool)for(auto& v:row) { const bool nan=r.Chance(9);const float x=r.Unit();v=nan ? std::numeric_limits<float>::quiet_NaN():x; }
        if(it%40==0 || r.Chance(30)) {
            const int n=int(r.Next()%41);g_RigWriteN=n;
            for(int k=0;k<VRRIG_WRITES;++k) {
                const int which=int(r.Next()%4),bone=int(r.Next()%14)-1;float off[3];r.Vec(off,1);
                const bool abs=r.Chance(4),quatOn=r.Chance(4);float q[4];r.Quat(q,3,1.5f,3);
                const bool spin=r.Chance(3);const float angle=r.Range(-180,180);float axis[3];r.Vec(axis,1);
                const bool scaled=r.Chance(4);const float scale=r.Range(-.2f,1.5f);const bool enabled=!r.Chance(6);
                g_RigWriteWhich[k]=which;g_RigWriteBone[k]=bone;for(int c=0;c<3;++c)g_RigWriteOff[k][c]=off[c];
                g_RigWriteAbs[k]=abs;g_RigWriteQuatOn[k]=quatOn;for(int c=0;c<4;++c)g_RigWriteQuat[k][c]=q[c];
                g_RigWriteRotAngle[k]=spin ? angle:0.f;for(int c=0;c<3;++c)g_RigWriteRotAxis[k][c]=axis[c];
                g_RigWriteScale[k]=scaled ? scale:0.f;g_RigWriteEnabled[k]=enabled;
                g_RigWriteHaveBase[k]=0;g_RigWriteHaveBaseRot[k]=0;
            }
            for(int w=0;w<VRRIG_N;++w)for(int t=0;t<VRRIG_TRACKS;++t) { const float v=r.Range(-1,2);const bool on=r.Chance(2);g_RigTrackSet[w][t]=v;g_RigTrackOn[w][t]=on; }
        }
        const bool inImage=r.Chance(5);
        g_gameModuleBase=inImage ? reinterpret_cast<uintptr_t>(&trackPool[0][0]):0;g_gameModuleSize=inImage ? sizeof(trackPool[0])*2:0;
        const int rig=int(r.Next()%5),component=int(r.Next()%4),row=int(r.Next()%4);const unsigned a4=unsigned(r.Next()%kLods);
        const uint32_t slots=r.Chance(5) ? 16u:uint32_t(kSlots),capacity=r.Chance(6) ? 0u:uint32_t(kTrackCap);
        std::memcpy(poseDesc+0x08,&slots,4);std::memcpy(poseDesc+0x20,&capacity,4);
        void* a1[9]{};a1[8]=rig<kRigs ? rigObj[rig]:nullptr;
        void* a2[7]{};a2[6]=component<3 ? six[component]:nullptr;
        cvr::anim::WeaponRigIdentifyAndWrite(a1,a2,a4,poseDesc,reinterpret_cast<uint8_t*>(bonePool),
                                             reinterpret_cast<uintptr_t>(trackPool[row]));
        const unsigned censusA4=unsigned(r.Next()%4);const uintptr_t censusBuf=0x1000+uintptr_t(r.Next()%60)*16;
        g_PoseCensusWeaponOut=r.Chance(2);
        cvr::anim::WeaponRigCensusNote(censusA4,censusBuf);
        const int parts=int(r.Next()%30);g_WeaponPartCount=parts;g_WeaponPartWriteOn=r.Chance(2);
        for(int k=0;k<24;++k) {
            const uint64_t pick=r.Next()%(kSlots+4);float off[3];r.Vec(off,.1f);const bool zero=r.Chance(4);
            g_WeaponPartIdx[k]=pick<uint64_t(kSlots) ? int(pick):pick==uint64_t(kSlots) ? -1:pick==uint64_t(kSlots+1) ? 2048:5000+int(pick);
            for(int c=0;c<3;++c)g_WeaponPartOff[k][c]=zero ? 0.f:off[c];
        }
        cvr::anim::WeaponRigCaptureParts(reinterpret_cast<uint8_t*>(bonePool));
        h.Fs(bonePool,kSlots*12);for(const auto& row2:trackPool)h.Fs(row2,kRow);
        const uintptr_t pool=reinterpret_cast<uintptr_t>(&trackPool[0][0]);
        for(int w=0;w<VRRIG_N;++w) {
            h.I(g_RigBuf[w] ? int64_t(g_RigBuf[w]-pool):-1);h.I(g_RigSeen[w]);h.I(g_RigBones[w]);h.I(g_RigTracks[w]);
            for(int b=0;b<20;++b)h.Fs(g_RigPose[w][b],7);
            h.I(g_RigPoseHave[w]);h.Fs(g_RigTrackVal[w],VRRIG_TRACKS);h.I(g_RigTrackHave[w]);
        }
        for(int k=0;k<VRRIG_WRITES;++k) {
            h.I(g_RigWriteSlot[k]);h.I(g_RigWritePin[k]);h.I(g_RigWriteApplied[k]);h.I(g_RigWriteHaveBase[k]);
            h.Fs(g_RigWriteBase[k],3);h.I(g_RigWriteHaveBaseRot[k]);h.Fs(g_RigWriteBaseRot[k],4);
        }
        const uintptr_t boneLo=reinterpret_cast<uintptr_t>(bonePool)&0x7FFFFFFFu;
        for(int l=0;l<4;++l) {
            h.I(g_RigPassSeen[l]);h.I(g_RigPassRn[l]);h.I(g_RigPassDst[l]);h.I(g_RigPassSlots[l]);
            h.B(g_RigPassBufLo[l]==0 || uintptr_t(g_RigPassBufLo[l])==boneLo);
        }
        for(int e=0;e<8;++e)h.I(g_RigPassMap0[e]);
        h.I(g_PoseCensusN);h.I(g_PoseCensusFull);
        for(int k=0;k<g_PoseCensusN;++k) {
            h.I(int64_t(g_PoseCensusBuf[k]));h.I(g_PoseCensusA4[k]);h.I(g_PoseCensusHits[k]);h.I(g_PoseCensusArmed[k]);
            h.I(g_PoseCensusHitsOut[k]);h.I(g_PoseCensusHitsIn[k]);
        }
        for(int k=0;k<24;++k) { h.Fs(g_WeaponPartPos[k],3);h.Fs(g_WeaponPartRot[k],4); }
        h.I(g_WeaponPartHave);h.I(int64_t(g_WeaponMatchCalls));
    }
    if(std::getenv("CVR_TRACE_COVERAGE")) {
        uint64_t applied=0;for(int k=0;k<VRRIG_WRITES;++k)applied+=g_RigWriteApplied[k];
        std::fprintf(stderr,"weapon_rig: seen %u/%u/%u applied %llu census %d full %d matchCalls %llu\n",
            unsigned(g_RigSeen[0]),unsigned(g_RigSeen[1]),unsigned(g_RigSeen[2]),static_cast<unsigned long long>(applied),
            int(g_PoseCensusN),int(g_PoseCensusFull),static_cast<unsigned long long>(g_WeaponMatchCalls));
    }
    g_RigSigN=0;g_RigWriteN=0;g_PoseCensusN=0;g_WeaponPartCount=0;g_gameModuleBase=0;g_gameModuleSize=0;
}

// Hands on the steering wheel (src/Anim/WheelGrab.cpp): the grab and its blends, the hub's horn, the steering from
// tracking samples through gaps, the gamepad encoding and the open fingers in a car. Its clocks are pure reads here
// (the C++ evaluates the steering's arguments in an unspecified order).
uint64_t g_wheelTraceMs=1000,g_wheelTraceUs=10000000;
uint64_t TraceWheelMs() { return g_wheelTraceMs; }
uint64_t TraceWheelUs() { return g_wheelTraceUs; }
cvr::anim::WheelTrackingFrame g_wheelTraceFrame{};bool g_wheelTraceFrameOk=false;int g_wheelCover[4]{};
bool TraceWheelFrame(cvr::anim::WheelTrackingFrame* out) { if(g_wheelTraceFrameOk)*out=g_wheelTraceFrame;return g_wheelTraceFrameOk; }
void TraceWheel(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockMs=TraceWheelMs;g_VRIKClockUs=TraceWheelUs;g_VRIKWheelFrame=TraceWheelFrame;
    static std::array<float,64*12> fingers{};
    g_VRSmokeFingerCount=6;g_VRSmokeFingerCountL=40;
    for(int k=0;k<32;++k) { g_VRSmokeFingerIdx[k]=k<6 ? k*3:-1;g_VRSmokeFingerIdxL[k]=k%5==4 ? -1:20+k; }
    g_VRSmokeFingerIdxL[3]=5000;
    cvr::anim::WheelReset();
    uint64_t sequence=1,origin=1;
    for(int it=0;it<4000;++it) {
        g_liveControls.xrWheelGrab=!r.Chance(12);
        { const bool bad=r.Chance(10);const float v=r.Range(.05f,.5f);g_liveControls.xrWheelRadius=bad ? 1.5f:v; }
        { const bool bad=r.Chance(10);const float v=r.Range(20,130);g_liveControls.xrWheelSteerMaxDeg=bad ? std::numeric_limits<float>::quiet_NaN():v; }
        { const float v=r.Range(-1,25);g_liveControls.xrWheelSteerDeadDeg=v; }
        g_liveControls.xrWheelPrediction=r.Chance(2);
        { const float v=r.Range(-1,10);g_liveControls.xrWheelPredictionMs=v; }
        g_liveControls.xrWheelHorn=!r.Chance(6);
        { const float v=r.Range(0,.4f);g_liveControls.xrWheelHornRadius=v; }
        g_isDriving=!r.Chance(10);g_menuModeValue=r.Chance(25);testOverlayVisible=r.Chance(30);
        g_hasWeaponEquipped=r.Chance(4);g_isInVehicle=!r.Chance(5);
        { const bool rightOff=r.Chance(15),leftOff=r.Chance(15);shared[8]=rightOff ? 0.f:1.f;shared[0]=leftOff ? 0.f:1.f; }
        if(r.Chance(6))shared[49]=shared[49]>.5f ? 0.f:1.f;
        if(r.Chance(6))shared[155]=shared[155]>.5f ? 0.f:1.f;
        TracePacket();
        // The driving animation's hands, sometimes one-handed (collapsed), the controllers about them.
        const bool collapsed=r.Chance(8);
        float right[3],left[3];r.Vec(right,.03f);r.Vec(left,.03f);
        right[0]+=.19f;right[1]+=.45f;right[2]+=.9f;left[0]+=collapsed ? .17f:-.19f;left[1]+=.45f;left[2]+=.9f;
        std::copy_n(right,3,g_fkPos[24]);std::copy_n(left,3,g_fkPos[23]);
        float qa[4],qb[4];r.Quat(qa,3,1.5f,3);r.Quat(qb,3,1.5f,3);std::copy_n(qa,4,g_fkRot[24]);std::copy_n(qb,4,g_fkRot[23]);
        const bool badIndex=r.Chance(20);
        cvr::anim::WheelCaptureAnim(0,badIndex ? 4000:24);cvr::anim::WheelCaptureAnim(1,23);cvr::anim::WheelCaptureAnim(2,23);
        const float dt=r.Range(-.01f,.15f);
        cvr::anim::WheelUpdate(dt);
        for(int hand=0;hand<2;++hand) {
            float target[3];r.Vec(target,.35f);const float* anim=hand ? left:right;for(int k=0;k<3;++k)target[k]+=anim[k];
            if(!r.Chance(10))cvr::anim::WheelStoreTarget(hand,target);
            float rot[4];r.Quat(rot,3,1.5f,3);
            cvr::anim::WheelBlendTarget(hand,target,rot);h.Fs(target,3);h.Fs(rot,4);h.B(cvr::anim::WheelHandsOff(hand));
        }
        if(r.Chance(5))cvr::anim::WheelMaintainGrab();else cvr::anim::WheelPublishGrab();
        cvr::anim::WheelSetMountedCar(r.Chance(2));
        { const uint64_t ms=r.Next()%120;g_wheelTraceMs+=r.Chance(15) ? 400:ms; }
        { const uint64_t us=r.Next()%30000;g_wheelTraceUs+=r.Chance(30) ? 3000000:us; }
        // The tracking sample: stale, invalid, a repeat, a new recenter origin, a hand thrown out of reach.
        {
            auto& f=g_wheelTraceFrame;
            float q[4];r.Quat(q,3,.6f,.3f);const bool badNorm=r.Chance(25);
            f.head.orientation={q[0],q[1],q[2],badNorm ? q[3]*2:q[3]};
            float p[3];r.Vec(p,.3f);f.head.position={p[0],p[1],p[2]+1.6f};
            for(int i=0;i<2;++i) {
                float a[3];r.Vec(a,.5f);const bool away=r.Chance(40);
                const float side=i ? -.19f:.19f;
                f.hands[i]={a[0]*.2f+side,a[1]*.2f-.3f,away ? 3.f:a[2]*.2f-.4f};
            }
            if(!r.Chance(6))++sequence;if(r.Chance(200))++origin;
            f.sequence=sequence;f.origin=r.Chance(50) ? 0:origin;
            const uint64_t lag=r.Next()%(r.Chance(10) ? 400000:20000);f.stampUs=g_wheelTraceUs-lag;
            f.validHands=uint8_t(r.Chance(10) ? r.Next()%4:3);f.headValid=!r.Chance(20);
            g_wheelTraceFrameOk=!r.Chance(15);
        }
        { const int state=cvr::anim::WheelControlState();const float steer=cvr::anim::WheelSteerInput();h.I(state);h.F(steer);
          g_wheelCover[0]+=(state&3)!=0;g_wheelCover[1]+=steer!=0;g_wheelCover[2]+=cvr::anim::g_wheelHornMask.load()!=0;
          g_wheelCover[3]+=cvr::anim::WheelHandsOff(0)||cvr::anim::WheelHandsOff(1); }
        { const float inner=r.Range(-.1f,.6f),outer=r.Range(.3f,1.1f);h.B(cvr::anim::WheelSetGamepadProfile(inner,outer)); }
        { const float v=r.Range(-1.5f,1.5f);h.F(cvr::anim::WheelGamepadSteer(v)); }
        h.F(cvr::anim::g_wheelBlendRight.load());h.F(cvr::anim::g_wheelBlendLeft.load());h.F(cvr::anim::g_wheelSteer.load());
        h.F(cvr::anim::g_wheelSteerDeg.load());h.I(cvr::anim::g_wheelHornMask.load());h.F(shared[163]);
        // The fingers: latched on foot, replayed seated.
        for(auto& v:fingers)v=r.Range(-1,1);
        g_VRSmokeFingerActive=r.Chance(5);g_VRSmokeFingerActiveL=r.Chance(5);
        cvr::anim::WheelFingers(reinterpret_cast<uint8_t*>(fingers.data()));h.Fs(fingers.data(),int(fingers.size()));
        if(r.Chance(300))cvr::anim::WheelReset();
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"wheel: held %d steered %d horn %d handsOff %d\n",g_wheelCover[0],g_wheelCover[1],g_wheelCover[2],g_wheelCover[3]);
    cvr::anim::WheelReset();
    g_VRIKClockMs=nullptr;g_VRIKClockUs=nullptr;g_VRIKWheelFrame=nullptr;g_isDriving=false;g_menuModeValue=0;
    testOverlayVisible=false;g_hasWeaponEquipped=false;g_isInVehicle=false;g_VRSmokeFingerActive=g_VRSmokeFingerActiveL=0;
    g_VRSmokeFingerCount=g_VRSmokeFingerCountL=0;shared[49]=shared[155]=shared[163]=0;
}

// Hand recoil (src/Anim/Recoil.cpp): rounds out of either gun, inside and outside the refractory window, the springs
// advanced over real and hitched frames, every class and the knee, a second hand's leverage, the climb, dual wield.
int64_t g_recoilTraceQpc=1000000;uint64_t g_recoilTraceMs=50000;
int64_t TraceRecoilQpc() { return g_recoilTraceQpc; }
uint64_t TraceRecoilMs() { return g_recoilTraceMs; }
void TraceRecoil(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockQpc=TraceRecoilQpc;g_VRIKClockMs=TraceRecoilMs;
    for(int it=0;it<6000;++it) {
        if(it%50==0 || r.Chance(40)) {
            CyberpunkVR_HandRecoil=!r.Chance(15);
            { const bool known=!r.Chance(4);const float k=r.Range(.1f,5);CyberpunkVR_WeaponKickDeg=known ? k:0.f; }
            CyberpunkVR_WeaponClass=int(r.Next()%5);
            { const bool known=r.Chance(2);const float k=r.Range(.1f,5);CyberpunkVR_WeaponKickDegLeft=known ? k:0.f; }
            CyberpunkVR_WeaponClassLeft=int(r.Next()%5);
            { const float v=r.Range(.5f,1.5f);CyberpunkVR_WeaponKickScale=v; }
            CyberpunkVR_TwoHandActive=r.Chance(3);
            { const float a=r.Range(-.2f,1.2f),b=r.Range(-.2f,1.2f),c=r.Range(-.2f,1.2f),d=r.Range(-.2f,1.2f);
              CyberpunkVR_TwoHandLever=a;CyberpunkVR_TwoHandRecoil=b;CyberpunkVR_TwoHandWeight=c;CyberpunkVR_TwoHandBackMul=d; }
            { const bool climb=r.Chance(3);const float f=r.Range(0,.6f),ms=r.Range(0,500);
              CyberpunkVR_RecoilClimbFrac=climb ? f:0.f;CyberpunkVR_RecoilClimbMs=ms; }
            { const float a=r.Range(-.2f,1.2f),b=r.Range(-.2f,1.2f),c=r.Range(-.2f,1.2f);
              CyberpunkVR_RecoilDownPistol=a;CyberpunkVR_RecoilDownRifle=b;CyberpunkVR_RecoilDownShotgun=c; }
            { const bool zero=r.Chance(10);const float ref=r.Range(.5f,2);CyberpunkVR_HandRecoilRefKick=zero ? 0.f:ref; }
            { const float ret=r.Range(10,400);CyberpunkVR_HandRecoilReturnMs=ret; }
            { const bool dual=r.Chance(3);const uint64_t until=g_recoilTraceMs+r.Next()%2000;CyberpunkVR_DualWieldUntil=dual ? until:0; }
            CyberpunkVR_RuntimeDiagnostics=!r.Chance(4);
        }
        const int rounds=int(r.Next()%4);
        for(int k=0;k<rounds;++k) {
            const bool left=r.Chance(3);
            if(left)RecoilOnShotLeft();else RecoilOnShot();
            g_recoilTraceQpc+=int64_t(r.Next()%60000);   // 0..60 ms at the hook's 1 MHz
        }
        const bool hitch=r.Chance(20);const int64_t frame=int64_t(r.Next()%25000)+2000;
        g_recoilTraceQpc+=hitch ? 200000:frame;g_recoilTraceMs+=uint64_t((hitch ? 200000:frame)/1000);
        RecoilTick();
        for(int side=0;side<2;++side)for(int weaponHand=0;weaponHand<2;++weaponHand) {
            float back=0,rise=0;RecoilSample(side,weaponHand,&back,&rise);h.F(back);h.F(rise);
        }
        float back=0,down=0;RecoilTravelMix(&back,&down);h.F(back);h.F(down);
        h.I(std::bit_cast<int64_t>(RecoilLastShotMs()));
        h.F(CyberpunkVR_DebugRecoilCm);h.F(CyberpunkVR_DebugRecoilClimbDeg);h.F(CyberpunkVR_DebugRecoilAppliedDeg);
        h.F(CyberpunkVR_DebugRecoilPeakDeg);h.F(CyberpunkVR_DebugRecoilPeakDegLeft);
        h.I(int64_t(CyberpunkVR_DebugRecoilShots));h.I(int64_t(CyberpunkVR_DebugRecoilTicks));
        if(r.Chance(50)) { CyberpunkVR_DebugRecoilPeakDeg=0;CyberpunkVR_DebugRecoilPeakDegLeft=0; }
    }
    for(int k=0;k<8;++k)h.I(CyberpunkVR_HapticStats[k]);
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"recoil: rounds %llu haptic %d peak %.2f\n",
        static_cast<unsigned long long>(CyberpunkVR_DebugRecoilShots),CyberpunkVR_HapticStats[6],CyberpunkVR_DebugRecoilPeakDeg);
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;CyberpunkVR_HandRecoil=1;CyberpunkVR_WeaponKickDeg=0;CyberpunkVR_WeaponClass=0;
    CyberpunkVR_WeaponKickDegLeft=0;CyberpunkVR_WeaponClassLeft=0;CyberpunkVR_WeaponKickScale=1;CyberpunkVR_TwoHandActive=0;
    CyberpunkVR_TwoHandLever=0;CyberpunkVR_TwoHandRecoil=.286f;CyberpunkVR_TwoHandWeight=0;CyberpunkVR_TwoHandBackMul=.6f;
    CyberpunkVR_RecoilClimbFrac=0;CyberpunkVR_RecoilClimbMs=250;CyberpunkVR_RecoilDownPistol=CyberpunkVR_RecoilDownRifle=0;
    CyberpunkVR_RecoilDownShotgun=0;CyberpunkVR_HandRecoilRefKick=1;CyberpunkVR_HandRecoilReturnMs=180;
    CyberpunkVR_DualWieldUntil=0;CyberpunkVR_RuntimeDiagnostics=1;
}

// The grasp (src/Anim/GraspPose.cpp): things of every shape and size seated in either hand, the fingers closed on them
// and blended in and out, the slot pinned and released, invalid requests, over the captured rig.
void TraceGrasp(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    Setup("weapon_stance");
    auto find=[&](const char* n){const auto it=std::find(names.begin(),names.end(),n);return it==names.end()?-1:int(it-names.begin());};
    g_VRSmokeCigIdx=find("WeaponRight");g_VRSmokeLighterIdx=find("WeaponLeft");
    const auto rest=Read<Bone>("shotgun-2.bin");static std::vector<Bone> pose;
    int solves=0;
    for(int it=0;it<360;++it) {
        pose=rest;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        for(auto& b:pose) { float q[4];r.Quat(q,.12f,.12f,.12f);float out[4];VRIK_QuatMul(q,b.q,out);std::copy_n(out,4,b.q); }
        const int side=int(r.Next()%2),shape=int(r.Next()%4);
        float half[3];for(auto& v:half) { const bool tiny=r.Chance(4);const float x=r.Range(.008f,.03f),y=r.Range(.02f,.16f);v=tiny ? x:y; }
        float at[3];r.Vec(at,.06f);float q[4];r.Quat(q,3.1f,1.5f,3.1f);
        const int action=int(r.Next()%7);
        if(action<=1) {
            float pos[3]{},rot[4]{};const bool ok=cvr_grasp_seat(side,shape,half,at,q,pos,rot);h.B(ok);h.Fs(pos,3);h.Fs(rot,4);
            if(ok && action==0) { h.B(cvr_grasp_set(side,shape,half,pos,rot));++solves; }
        } else if(action==2) { h.B(cvr_grasp_set(side,shape,half,at,q));++solves; }
        else if(action==3) { const float b=r.Range(-.2f,1.2f);h.B(cvr_grasp_blend(side,b)); }
        else if(action==4) { const bool pinned=r.Chance(2);h.B(cvr_grasp_pin(side,pinned)); }
        else if(action==5) { if(r.Chance(4))h.B(cvr_grasp_clear(side)); }
        else { h.B(cvr_grasp_set(2,shape,half,at,q));h.B(cvr_grasp_set(side,4,half,at,q));h.B(cvr_grasp_blend(-1,.5f));
               float pos[3]{},rot[4]{};h.B(cvr_grasp_seat(side,-1,half,at,q,pos,rot)); }
        cvr::anim::GraspFingers(buffer);
        for(const auto& b:pose) { h.Fs(b.p,4);h.Fs(b.q,4); }
        for(int k=0;k<4;++k)h.I(int64_t(CyberpunkVR_DebugGrasp[k]));
        for(int s=0;s<2;++s) {
            h.Fs(CyberpunkVR_DebugGraspAngles[s],17);h.Fs(CyberpunkVR_DebugGraspPalm[s],12);h.Fs(CyberpunkVR_DebugGraspThumb[s],4);
            h.I(CyberpunkVR_DebugGraspNormalFlip[s]);h.B(cvr::anim::GraspHolding(s));
        }
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"grasp: requests %d solves %llu passes %llu palms %llu\n",solves,
        static_cast<unsigned long long>(CyberpunkVR_DebugGrasp[0]),static_cast<unsigned long long>(CyberpunkVR_DebugGrasp[1]),
        static_cast<unsigned long long>(CyberpunkVR_DebugGrasp[2]));
    cvr_grasp_clear(0);cvr_grasp_clear(1);g_VRSmokeCigIdx=g_VRSmokeLighterIdx=-1;
}

// The support hand (src/Anim/TwoHandGrip.cpp): holds read from files (an alias, a second hold @mag, malformed and
// commented lines), selected by the weapon in hand, reached for and squeezed, steered in every mode, broken off, taken
// by the reload, the carry and a carried body, the carry into the left hand and the return, the left-handed mirror, two
// guns, and captures saved -- every saved file hashed byte for byte.
void TraceTwoHand(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockQpc=TraceRecoilQpc;g_VRIKClockMs=TraceRecoilMs;
    Setup("weapon_stance");
    auto find=[&](const char* n){const auto it=std::find(names.begin(),names.end(),n);return it==names.end()?-1:int(it-names.begin());};
    g_VRSmokeCigIdx=find("WeaponRight");g_VRSmokeLighterIdx=find("WeaponLeft");
    g_VRSmokeFingerCountL=19;
    for(int k=0;k<19;++k) {
        const std::string name=std::string("Left")+cvr::gestures::NativeFist[k].suffix;
        g_VRSmokeFingerIdxL[k]=find(name.c_str());std::strncpy(g_VRSmokeFingerNameL[k],name.c_str(),47);
    }
    std::vector<const char*> pointers;for(const auto& n:names)pointers.push_back(n.c_str());
    cvr::anim::LeftHandedArmRig(pointers.data(),static_cast<int>(pointers.size()));
    // The holds on disk, and their offsets for reaching them.
    const auto dir=std::filesystem::temp_directory_path()/("cvr_twohand_trace_"+std::to_string(GetCurrentProcessId()));
    std::filesystem::remove_all(dir);std::filesystem::create_directories(dir);
    std::map<std::string,std::array<float,3>> offsets;
    auto hold=[&](const char* weapon,float reach) {
        float off[3];r.Vec(off,reach);float q[4];r.Quat(q,3,1.5f,3);
        char line[256];std::string s="# hold\r\n\r\n";
        std::snprintf(line,sizeof(line),"W %.9g %.9g %.9g %.9g %.9g %.9g %.9g\r\n",off[0],off[1],off[2],q[0],q[1],q[2],q[3]);s+=line;
        s+="W 1 2 3\r\n";
        for(int k=0;k<19;++k) {
            if(r.Chance(6))continue;
            float f[4];r.Quat(f,1.5f,1.5f,1.5f);
            std::snprintf(line,sizeof(line),"F %s %.9g %.9g %.9g %.9g\r\n",g_VRSmokeFingerNameL[k],f[0],f[1],f[2],f[3]);s+=line;
        }
        s+="F Bogus 1 2\r\n";
        std::ofstream(dir/(std::string("CyberpunkVR_TwoHandGrip_")+weapon+".ini"),std::ios::binary)<<s;
        offsets[weapon]={off[0],off[1],off[2]};
    };
    hold("pistol_a",.08f);hold("pistol_a@mag",.12f);hold("rifle_b",.35f);hold("pistol_a@zz",.1f);
    std::ofstream(dir/"CyberpunkVR_TwoHandGrip_kappa.ini",std::ios::binary)<<"ALIAS rifle_b\r\n";
    std::ofstream(dir/"CyberpunkVR_TwoHandGrip_.ini",std::ios::binary)<<"W 0 0 0 0 0 0 1\r\n";
    std::ofstream(dir/"notahold.ini",std::ios::binary)<<"W 0 0 0 0 0 0 1\r\n";
    offsets["kappa"]=offsets["rifle_b"];
    testGripDir=dir.string();
    cvr::anim::TwoHandTick();h.I(CyberpunkVR_DebugTwoHandLoaded);
    const auto rest=Read<Bone>("shotgun-2.bin");static std::vector<Bone> pose;pose=rest;
    std::array<float,256> block{};g_pSharedHands=block.data();
    const char* weapons[]={"pistol_a","rifle_b","kappa","unknown",""};
    int engaged=0,captured=0;
    for(int it=0;it<1500;++it) {
        if(it%60==0 || r.Chance(80)) { const int w=int(r.Next()%5);std::strncpy(CyberpunkVR_WeaponName,weapons[w],63); }
        if(r.Chance(10)) { const bool out=!r.Chance(8);g_hasWeaponEquipped=out; }
        CyberpunkVR_TwoHandGrip=!r.Chance(40);
        if(r.Chance(30)) { const bool clear=r.Chance(2);const int bits=int(r.Next()%8);CyberpunkVR_TwoHandSuppress=clear ? 0:bits; }
        if(r.Chance(50))CyberpunkVR_CarryLeft=!CyberpunkVR_CarryLeft;
        g_VRReloadFingerActive[0]=r.Chance(30);g_VRReloadFingerActive[1]=r.Chance(30);
        if(r.Chance(40))CyberpunkVR_TwoHandAim=int(r.Next()%3);
        g_VRBodyCarryingState=r.Chance(60) ? 2:-1;
        if(r.Chance(30)) { const bool l=r.Chance(3),rr=!r.Chance(6);g_hasLeftWeaponEquipped=l;g_hasRightWeaponEquipped=rr; }
        CyberpunkVR_DualWieldUntil=r.Chance(3) ? ~0ull:0ull;
        if(r.Chance(5))block[155]=block[155]>.5f ? 0.f:1.f;
        if(r.Chance(7))block[49]=block[49]>.5f ? 0.f:1.f;
        if(r.Chance(40)) { float s[3];r.Vec(s,.08f);std::copy_n(s,3,g_VRTwoHandSlide);g_VRTwoHandSlideUntil=g_recoilTraceMs+r.Next()%400; }
        { const bool hitch=r.Chance(40);const int64_t step=int64_t(r.Next()%28000)+2000;
          g_recoilTraceQpc+=hitch ? 400000:step;g_recoilTraceMs+=uint64_t((hitch ? 400000:step)/1000); }
        // The right hand, and the left controller about the support point of the weapon in hand's hold.
        float target[3];r.Vec(target,.05f);target[0]+=.15f;target[1]+=.35f;target[2]+=1.25f;
        float hm[4];r.Quat(hm,3,1.2f,3);float wrist[4];r.Quat(wrist,.5f,.5f,.5f);
        const bool useWrist=!r.Chance(5),leftNull=r.Chance(15);
        float left[3];r.Vec(left,.04f);
        {
            float bone[4];if(useWrist) { VRIK_QuatMul(hm,wrist,bone);VRIK_QuatNorm(bone); } else std::copy_n(hm,4,bone);
            const auto it2=offsets.find(CyberpunkVR_WeaponName);
            float o[3]={0,0,.3f};if(it2!=offsets.end()) { const float off[3]={it2->second[0],it2->second[1],it2->second[2]};VRIK_QuatRotateVec(bone,off,o); }
            const float stray=r.Chance(6) ? .7f:0.f;
            for(int k=0;k<3;++k)left[k]+=target[k]+o[k]+(k==0 ? stray:0.f);
        }
        CarryReturnRight(target,hm,useWrist ? wrist:nullptr);h.Fs(target,3);h.Fs(hm,4);
        cvr::anim::TwoHandRight(target,hm,useWrist ? wrist:nullptr,leftNull ? nullptr:left);h.Fs(hm,4);
        engaged+=CyberpunkVR_TwoHandActive;
        float tl[3];r.Vec(tl,.3f);float rl[4];r.Quat(rl,3,1.5f,3);
        h.B(cvr::anim::TwoHandLeft(tl,rl));h.Fs(tl,3);h.Fs(rl,4);
        float cr[4];r.Quat(cr,3,1.5f,3);CarryLeftBlend(cr);h.Fs(cr,4);
        CarryRecordLeft(tl,rl);
        if(it%3==0) {
            pose=rest;cvr::anim::TwoHandFingers(reinterpret_cast<uint8_t*>(pose.data()));
            for(const auto& b:pose)h.Fs(b.q,4);
        }
        for(int k=0;k<3;++k)h.F(VRTwoHandCarryOffset(k));
        if(r.Chance(60)) {
            pose=rest;
            for(auto& b:pose) { float q[4];r.Quat(q,.05f,.05f,.05f);float out[4];VRIK_QuatMul(q,b.q,out);std::copy_n(out,4,b.q); }
            CyberpunkVR_TwoHandCaptureReq=1;g_VRBind=r.Chance(4) ? 4:0;
            cvr::anim::TwoHandCapture(reinterpret_cast<uint8_t*>(pose.data()));g_VRBind=4;
            cvr::anim::TwoHandTick();
            h.I(CyberpunkVR_DebugTwoHandSaved);h.I(CyberpunkVR_DebugTwoHandRefused);h.F(CyberpunkVR_DebugTwoHandBaseMm);
            if(CyberpunkVR_DebugTwoHandSaved==1) {
                ++captured;CyberpunkVR_DebugTwoHandSaved=0;
                const std::string weapon=CyberpunkVR_WeaponName[0] ? CyberpunkVR_WeaponName:"default";
                std::ifstream f(dir/("CyberpunkVR_TwoHandGrip_"+weapon+".ini"),std::ios::binary);
                const std::string bytes((std::istreambuf_iterator<char>(f)),std::istreambuf_iterator<char>());
                h.I(int64_t(bytes.size()));for(const char c:bytes)h.Bytes(uint8_t(c),1);
            }
        }
        h.I(CyberpunkVR_TwoHandActive);h.F(CyberpunkVR_TwoHandBlend);h.F(CyberpunkVR_TwoHandWeight);h.F(CyberpunkVR_TwoHandLever);
        h.F(CyberpunkVR_DebugTwoHandDist);h.I(CyberpunkVR_DebugTwoHandHoldIdx);h.I(CyberpunkVR_DebugTwoHandHave);
        h.F(CyberpunkVR_DebugTwoHandAimDeg);h.F(CyberpunkVR_DebugCarryBlend);h.F(CyberpunkVR_DebugCarryReturn);
        h.F(CyberpunkVR_CarryGripBlend);h.F(CyberpunkVR_DebugCarryReach);h.Fs(g_VRTwoHandCtrlL,3);h.I(g_VRTwoHandCtrlLValid);
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"two_hand: loaded %d engaged %d captured %d\n",
        CyberpunkVR_DebugTwoHandLoaded,engaged,captured);
    std::filesystem::remove_all(dir);testGripDir.clear();
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;g_pSharedHands=shared.data();CyberpunkVR_WeaponName[0]=0;
    g_hasWeaponEquipped=false;g_hasLeftWeaponEquipped=g_hasRightWeaponEquipped=false;CyberpunkVR_CarryLeft=0;
    CyberpunkVR_TwoHandSuppress=0;CyberpunkVR_TwoHandGrip=1;CyberpunkVR_TwoHandAim=1;g_VRReloadFingerActive[0]=g_VRReloadFingerActive[1]=0;
    g_VRBodyCarryingState=-1;CyberpunkVR_TwoHandActive=0;CyberpunkVR_DualWieldUntil=0;g_VRTwoHandSlideUntil=0;
    g_VRSmokeFingerCountL=0;g_VRSmokeCigIdx=g_VRSmokeLighterIdx=-1;
}

// The pose hook's work (src/Hooks/AnimPoseBody.cpp): the engine's arguments fabricated over the captured rig -- player
// passes (fresh solves and same-batch replays; the camera paired, missing and invalidated; the view packet and the snap
// event; scenes, takedowns, carries, the ladder, swimming, the lobby, full-body tracking; the native hand frame; the
// camera bones' freeze modes; left-handed) and other skeletons' passes -- and everything the passes write, hashed.
void TraceHook(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockQpc=TraceRecoilQpc;g_VRIKClockMs=TraceRecoilMs;
    Setup("weapon_stance");
    auto find=[&](const char* n){const auto it=std::find(names.begin(),names.end(),n);return it==names.end()?-1:int(it-names.begin());};
    g_VRSmokeCigIdx=find("WeaponRight");g_VRSmokeLighterIdx=find("WeaponLeft");g_VRSmokeMouthBoneIdx=find("WeaponRight1");
    g_VRSmokeFingerCount=g_VRSmokeFingerCountL=19;
    for(int side=0;side<2;++side)for(int k=0;k<19;++k) {
        const std::string name=std::string(side?"Right":"Left")+cvr::gestures::NativeFist[k].suffix;
        (side?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k]=find(name.c_str());
        std::strncpy(side?g_VRSmokeFingerName[k]:g_VRSmokeFingerNameL[k],name.c_str(),47);
    }
    {
        const char* cams[5]={"Torso_fppCamera_Control_GRP","Torso_fppCamera_Aim_JNT","Torso_fppCamera_Target_JNT",
                             "Torso_fppCamera_UpOffset_GRP","Torso_fppCamera_Up_GRP"};
        for(int k=0;k<5;++k)g_VRFppCamIdx[k]=find(cams[k]);
    }
    std::vector<const char*> pointers;for(const auto& n:names)pointers.push_back(n.c_str());
    cvr::anim::LeftHandedArmRig(pointers.data(),static_cast<int>(pointers.size()));
    const std::vector<Bone> poses[2]={Read<Bone>("unarmed-2.bin"),Read<Bone>("shotgun-2.bin")};
    static std::vector<Bone> pose;
    alignas(16) static float tracks[2][64]{};alignas(16) static uint8_t poseDesc[0x40]{};alignas(16) static uint8_t graph[0x40]{};
    void* a1[16]{};a1[4]=graph;void* a2[8]{};a2[7]=poseDesc;
    g_PlayerTrackBufA=reinterpret_cast<uintptr_t>(tracks[0]);g_PlayerTrackBufB=0;
    uint32_t viewSeq=0,handSeq=0;float snapCounter=0;
    int fresh=0,replays=0,nativeFrames=0;
    for(int it=0;it<1200;++it) {
        pose=poses[r.Next()%2];
        for(auto& b:pose) { float q[4];r.Quat(q,.08f,.08f,.08f);float out[4];VRIK_QuatMul(q,b.q,out);std::copy_n(out,4,b.q); }
        const bool otherSkeleton=r.Chance(12);
        uint8_t* bone=reinterpret_cast<uint8_t*>(pose.data());uintptr_t track=reinterpret_cast<uintptr_t>(tracks[otherSkeleton ? 1:0]);
        const uint32_t slots=uint32_t(pose.size()),trackCount=16;
        std::memcpy(poseDesc,&bone,8);std::memcpy(poseDesc+24,&track,8);std::memcpy(poseDesc+8,&slots,4);std::memcpy(poseDesc+0x20,&trackCount,4);
        { const uint64_t path=r.Chance(3) ? 0x7c8ef3ac7af95509ull:r.Next();std::memcpy(graph+0x30,&path,8); }
        // the switches and the state the passes read
        g_VRBind=r.Chance(15) ? 0:4;g_VRCamBoneFreeze=int(r.Next()%5);g_VRBodyUnderHMD=!r.Chance(10);
        testDeviceCam=r.Chance(40);g_sceneTier=r.Chance(20) ? 5:1;testCutsceneTier=r.Chance(2) ? 3:-1;
        g_VRTakedownState=r.Chance(40) ? 2:-1;
        { const bool carry=r.Chance(8);const bool two=r.Chance(2);g_VRBodyCarryingState=carry ? (two ? 2:5):-1; }
        g_VRBodyCarryBothHands=r.Chance(2);g_vehicleState=int(r.Next()%3);
        testLadder=r.Chance(10);testLadderKinds[0]=int(r.Next()%4);testLadderKinds[1]=int(r.Next()%4);
        g_liveControls.xrLadderGripClimb=r.Chance(2);g_liveControls.xrLeftHanded=r.Chance(5);
        g_liveControls.xrVehicleCutsceneSuspendTier=r.Chance(2) ? 2:-1;
        testSwimming=r.Chance(15);testLobby=r.Chance(10);testFbtArm=r.Chance(12);testFbtCalibrating=r.Chance(25);
        { const bool l=r.Chance(5),rr=!r.Chance(5);g_hasLeftWeaponEquipped=l;g_hasRightWeaponEquipped=rr; }
        CyberpunkVR_XrDeepDiag=r.Chance(3);CyberpunkVR_VrikArmAnchorFromBody=!r.Chance(5);
        CyberpunkVR_VrikSplitHeadHandRot=r.Chance(4);CyberpunkVR_VrikHandFrameOneLatch=!r.Chance(4);
        CyberpunkVR_VrikHandFrameAlign=!r.Chance(4);CyberpunkVR_VrikNativeFramePair=!r.Chance(3);
        CyberpunkVR_HeadTranslationInPatch=!r.Chance(6);testBodyYawFollow=r.Chance(4);CyberpunkVR_BodyYawFinalValid=r.Chance(2);
        { const float y=r.Range(-3,3);CyberpunkVR_BodyYawFinalRad=y; }
        testCyberwareWorldFrame=r.Chance(3);testRecipeOk=r.Chance(2);g_headingValid=1;
        CyberpunkVR_HeadingFromPreWrite=!r.Chance(3);CyberpunkVR_ViewYawFromEngine=!r.Chance(3);CyberpunkVR_CamWriteInPatch=!r.Chance(6);
        { const float p=r.Range(-.5f,.5f);g_headingPitchS=std::sin(p*.5f);g_headingPitchC=std::cos(p*.5f); }
        { float o[3];r.Vec(o,.1f);testRecipe.modelOffset={o[0],o[1],o[2]};const float s=r.Range(.8f,1.2f),ty=r.Range(-1,1),cy=r.Range(-1,1);
          testRecipe.scale=s;testRecipe.trackingYaw=ty;testRecipe.centreYaw=cy;testRecipe.bodyOrbit=r.Chance(2); }
        if(testRecipeOk)cvr::camera::AnchorRecipePublish(testRecipe);else cvr::camera::AnchorRecipeClear();
        testRestNeckOk=r.Chance(2);{ const float x=r.Range(-.1f,.1f),y=r.Range(-.1f,.1f);testRestNeck={x,y}; }
        { const float x=r.Range(-.2f,.2f),y=r.Range(-.2f,.2f);testConsumed={x,y}; }
        g_VRCamBoneFreeze=int(r.Next()%5);
        // the camera pair
        testNativeAvailable=!r.Chance(12);testNativePair.valid=!r.Chance(10);testNativePair.unavailable=testNativePair.valid ? 0:1;
        { float q[4];r.Quat(q,3.1f,.05f,.05f);std::copy_n(q,4,testNativePair.entityQuat);float c[4];r.Quat(c,.3f,.4f,.1f);
          VRIK_QuatMul(q,c,testNativePair.camQuat);float base[3]={0,0,1.6f};float bw[3];VRIK_QuatRotateVec(q,base,bw);float d[3];r.Vec(d,.06f);
          for(int k=0;k<3;++k) { testNativePair.bodyCameraMinusEntity[k]=bw[k];testNativePair.cameraMinusEntity[k]=bw[k]+d[k]; } }
        testLuaAvailable=r.Chance(2);testLuaPair=testNativePair;
        // the hands, the head and the view packet
        for(int side=0;side<2;++side) {
            const int slot=side*8;const bool tracked=!r.Chance(10);float p[3];r.Vec(p,.12f);float q[4];r.Quat(q,3,1.5f,3);
            shared[slot]=tracked ? 1.f:0.f;shared[slot+1]=p[0]+(side ? .23f:-.23f);shared[slot+2]=p[1]-.3f;shared[slot+3]=p[2]-.3f;
            for(int k=0;k<4;++k)shared[slot+4+k]=q[k];
        }
        { float hq[4];r.Quat(hq,.6f,.4f,.2f);for(int k=0;k<4;++k)shared[16+k]=hq[k]; }
        shared[31]=r.Chance(6) ? 1.f:0.f;
        { float b[3];r.Vec(b,.05f);for(int k=0;k<3;++k)shared[91+k]=b[k]; }
        shared[123]=r.Chance(2) ? 1.f:0.f;{ float v[3];r.Vec(v,.05f);for(int k=0;k<3;++k)shared[120+k]=v[k]; }
        shared[115]=r.Chance(2) ? 2.f:0.f;shared[112]=r.Range(-.1f,.1f);shared[113]=r.Range(-.1f,.1f);
        { float hp[3];r.Vec(hp,.1f);for(int k=0;k<3;++k)shared[124+k]=hp[k]; }
        { float vq[4];r.Quat(vq,3.1f,.3f,.1f);for(int k=0;k<4;++k)shared[104+k]=vq[k];float vp[3];r.Vec(vp,1);
          for(int k=0;k<3;++k)shared[108+k]=vp[k];shared[111]=r.Chance(5) ? 0.f:2.f;
          float oh[4];r.Quat(oh,.5f,.3f,.1f);for(int k=0;k<4;++k)shared[227+k]=oh[k];shared[141]=r.Range(-3,3);shared[142]=1; }
        viewSeq+=2;*reinterpret_cast<uint32_t*>(&shared[143])=viewSeq;
        if(r.Chance(25))snapCounter+=1;shared[147]=snapCounter;
        if(r.Chance(5))shared[49]=shared[49]>.5f ? 0.f:1.f;
        if(r.Chance(5))shared[155]=shared[155]>.5f ? 0.f:1.f;
        handSeq+=2;*reinterpret_cast<uint32_t*>(&shared[127])=handSeq;
        // the batch clock: passes microseconds apart, batches a frame apart
        { const bool newBatch=r.Chance(3);const int64_t step=newBatch ? int64_t(r.Next()%20000)+8000:int64_t(r.Next()%800)+50;
          g_recoilTraceQpc+=step;g_recoilTraceMs+=uint64_t(step/1000); }
        const int beforeFresh=g_VRIKFreshTotal,beforeReplay=g_VRIKReplayTotal;
        bool locked=false;cvr_anim_pose_apply(a1,a2,unsigned(r.Next()%4),&locked);
        if(locked)ReleaseSRWLockExclusive(&g_PlayerPoseStateLock);
        fresh+=g_VRIKFreshTotal-beforeFresh;replays+=g_VRIKReplayTotal-beforeReplay;
        nativeFrames+=std::isnan(CyberpunkVR_DebugRestNeck[0]) ? 0:1;
        h.B(locked);
        for(const auto& b:pose) { h.Fs(b.p,4);h.Fs(b.q,4);h.Fs(b.s,4); }
        h.Fs(g_VRPalmModelR,3);h.Fs(g_VRPalmModelL,3);h.Fs(g_VRPalmModelRotR,4);h.Fs(g_VRPalmModelRotL,4);h.I(g_VRPalmModelValid);
        h.Fs(g_VRCamModelPos,3);h.Fs(g_VRCamModelRot,4);h.Fs(g_VRViewWorldPos,3);h.Fs(g_VRViewWorldRot,4);h.I(g_VRViewWorldValid);
        h.Fs(g_VRSmokeMouthWorldPos,3);h.Fs(g_VRSmokeMouthWorldRot,4);h.Fs(g_VRSmokeAnchorLocalPos,3);h.Fs(g_VRSmokeAnchorLocalRot,4);
        h.I(g_VRSmokeAnchorValid);h.Fs(g_VRSmokeAltAnchorLocalPos,3);h.Fs(g_VRSmokeAltAnchorLocalRot,4);h.I(g_VRSmokeAltAnchorValid);
        h.F(g_VRSmokeMouthDist);h.F(g_VRSmokeMouthDistL);
        for(int s:{20,21,22,82,83,85,86,87,88,224,225,226,231,232})h.F(shared[s]);
        h.I(int64_t(g_solveCacheTick));h.I(g_solveCacheN);h.I(g_VRIKFreshTotal);h.I(g_VRIKReplayTotal);
        h.I(int64_t(g_AnimPoseTotalCalls));h.I(int64_t(g_AnimPoseMatchCalls));
        h.F(CyberpunkVR_DebugHandMissMm);h.Fs(CyberpunkVR_DebugRestNeck,2);h.F(g_solveCacheYaw);h.F(g_solveCacheSnapCtr);
        h.I(int64_t(testServiceCalls.size()));for(const float v:testServiceCalls)h.F(v);testServiceCalls.clear();
        h.I(g_VrikBatchGapNew);h.I(g_VrikBatchGapSame);h.F(g_VrikBatchGapNewMin);h.F(g_VrikBatchGapSameMax);
        h.I(g_VrikEngineOverwritePasses);h.I(g_VrikEngineOverwriteHits);h.F(g_VrikEngineOverwriteMm);
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"hook: fresh %d replays %d native frames %d\n",fresh,replays,nativeFrames);
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;g_PlayerTrackBufA=0;g_VRBind=4;testDeviceCam=false;g_sceneTier=1;
    testLadder=testSwimming=testLobby=testFbtArm=testFbtCalibrating=false;g_hasLeftWeaponEquipped=g_hasRightWeaponEquipped=false;
    testCyberwareWorldFrame=false;g_VRTakedownState=-1;g_VRBodyCarryingState=-1;
    CyberpunkVR_HeadingFromPreWrite=0;CyberpunkVR_ViewYawFromEngine=0;CyberpunkVR_CamWriteInPatch=1;
}

// The controllers' vibration (src/Runtimes/Haptics.cpp): events and rounds on either hand, priorities and the support
// hand, the switch and the scale, the profiles tuned live, ticks between and inside segments -- every device call hashed.
void TraceHaptics(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockQpc=TraceRecoilQpc;testHapticPlays.clear();
    float saved[16][6];std::memcpy(saved,CyberpunkVR_HapticProfiles,sizeof(saved));
    const int events[]={1,2,3,10,11,12,13,14,15,20,21,22,0,99};
    for(int it=0;it<6000;++it) {
        if(r.Chance(200)) { const int row=int(r.Next()%16),col=int(r.Next()%6);const float v=r.Range(-.2f,600);CyberpunkVR_HapticProfiles[row][col]=v; }
        CyberpunkVR_Haptics=!r.Chance(30);
        { const bool odd=r.Chance(20);const float s=r.Range(-.5f,2.5f);CyberpunkVR_HapticScale=odd ? s:1.0f; }
        { const float m=r.Range(-.2f,1.2f);CyberpunkVR_HapticSupportMul=m; }
        CyberpunkVR_TwoHandActive=r.Chance(3);
        const int action=int(r.Next()%4);
        if(action==0) { const int cls=int(r.Next()%6);const bool support=r.Chance(2);const int hand=int(r.Next()%2);cvr::haptics::Shot(cls,support,hand); }
        else if(action==1) { const int ev=events[r.Next()%14];const int mask=int(r.Next()%4);cvr::haptics::Play(ev,mask); }
        g_recoilTraceQpc+=int64_t(r.Next()%(r.Chance(10) ? 200000:12000));
        cvr::haptics::Tick();
        for(int k=0;k<8;++k)h.I(CyberpunkVR_HapticStats[k]);
        h.I(int64_t(testHapticPlays.size()));for(const float v:testHapticPlays)h.F(v);testHapticPlays.clear();
    }
    std::memcpy(CyberpunkVR_HapticProfiles,saved,sizeof(saved));CyberpunkVR_Haptics=1;CyberpunkVR_HapticScale=1;
    CyberpunkVR_HapticSupportMul=1;CyberpunkVR_TwoHandActive=0;g_VRIKClockQpc=nullptr;
}

// Finger tracking from the hand skeleton (src/Runtimes/FingerGrip.cpp): the setting read from vrport.ini, the
// extensions chosen, the trackers made through the runtime's functions (each missing in turn, a hand failing), then per
// frame and hand the joints located -- failing, inactive, a joint invalid, bent and spread at random, glove spikes and
// zero dropouts, long pauses -- and the published bends and spreads read back on their own clock. Every runtime call is
// hashed with what it was given.
uint64_t g_fingerTraceMs=0;
uint64_t TraceFingerMs() { return g_fingerTraceMs; }
void TraceFingerHand(TraceRng& r,float bend[5],float spread[4]) {
    XrHandJointLocationEXT* j=testHand.joints;
    for(int k=0;k<XR_HAND_JOINT_COUNT_EXT;++k) { j[k]=XrHandJointLocationEXT{};j[k].locationFlags=XR_SPACE_LOCATION_POSITION_VALID_BIT|XR_SPACE_LOCATION_ORIENTATION_VALID_BIT; }
    const float lengths[5]={.035f,.04f,.042f,.04f,.034f};
    j[XR_HAND_JOINT_WRIST_EXT].pose.position={0,0,0};j[XR_HAND_JOINT_PALM_EXT].pose.position={0,.05f,0};
    for(int d=0;d<5;++d) {
        const int first=d==0 ? XR_HAND_JOINT_THUMB_METACARPAL_EXT:XR_HAND_JOINT_INDEX_METACARPAL_EXT+(d-1)*5;
        const int bones=d==0 ? 4:5;
        const float tilt=d==0 ? -.6f-spread[0]:(d-2.0f)*.12f+(d<4 ? -spread[d]*.2f:spread[3]*.2f);
        XrVector3f p{(d-2.0f)*.02f,.01f,0};
        float pitch=0;
        for(int b=0;b<bones;++b) {
            j[first+b].pose.position=p;
            if(b>0)pitch+=bend[d]*1.25f;
            const float wobble=r.Range(-.02f,.02f);
            const XrVector3f dir{std::sin(tilt)*std::cos(pitch),std::cos(tilt)*std::cos(pitch),std::sin(pitch)+wobble};
            p={p.x+dir.x*lengths[d],p.y+dir.y*lengths[d],p.z+dir.z*lengths[d]};
        }
    }
}
void TraceFingerGrip(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockMs=TraceFingerMs;g_fingerTraceMs=5000;
    const auto dir=std::filesystem::temp_directory_path()/"cvr_vrik_trace_fingers";
    std::filesystem::create_directories(dir);testConfigDir=dir.string();
    auto hashCalls=[&] { h.I(int64_t(testHand.calls.size()));for(const int64_t v:testHand.calls)h.I(v);testHand.calls.clear(); };
    const char* inis[]={"xr_finger_bones=1\n","xr_finger_bones = 1\r\n","x=2\r\nxr_finger_bones\t=  1\n","xr_finger_bones=0\nxr_finger_bones=1\n","xr_finger_bones=2\n"," xr_finger_bones=1\n"};
    const char* extensionNames[]={XR_EXT_HAND_TRACKING_EXTENSION_NAME,XR_EXT_HAND_JOINTS_MOTION_RANGE_EXTENSION_NAME,"XR_KHR_D3D12_enable"};
    float bend[2][5]{},spread[2][4]{};int cover[5]{};
    for(int session=0;session<40;++session) {
        { const int pick=int(r.Next()%8);std::filesystem::remove(dir/"vrport.ini");
          if(pick<6) { std::ofstream f(dir/"vrport.ini",std::ios::binary);f<<(pick<3 ? inis[0]:inis[pick]); } }
        std::vector<XrExtensionProperties> available;
        for(const char* n:extensionNames) if(!r.Chance(4)) { XrExtensionProperties e{XR_TYPE_EXTENSION_PROPERTIES};strcpy_s(e.extensionName,n);available.push_back(e); }
        std::vector<const char*> enabled{"base"};
        cvr::fingergrip::ChooseExtensions(available,enabled);
        h.I(int64_t(enabled.size()));for(const char* e:enabled)h.I(int64_t(std::strlen(e)));
        testHand.haveCreate=!r.Chance(10);testHand.haveDestroy=!r.Chance(10);testHand.haveLocate=!r.Chance(10);
        for(int k=0;k<2;++k)testHand.createResult[k]=r.Chance(8) ? XR_ERROR_FEATURE_UNSUPPORTED:XR_SUCCESS;
        cvr::fingergrip::OnSessionCreated(reinterpret_cast<XrInstance>(uintptr_t(0x5000+session)),reinterpret_cast<XrSession>(uintptr_t(0x6000+session)));
        hashCalls();
        const int frames=int(r.Next()%400)+50;
        for(int frame=0;frame<frames;++frame) {
            if(r.Chance(97)) { const bool on=r.Chance(2);cvr::fingergrip::SetGripFromFingers(on); }
            h.B(cvr::fingergrip::GripFromFingers());
            for(int hand=0;hand<2;++hand) {
                for(int d=0;d<5;++d) {
                    const float step=r.Range(-.08f,.08f);float b=bend[hand][d]+step;
                    if(r.Chance(25)) { const float jump=r.Unit();b=jump; }
                    b=b<0 ? 0:(b>1 ? 1:b);bend[hand][d]=b;
                }
                for(int s=0;s<4;++s) { const float step=r.Range(-.1f,.1f);float v=spread[hand][s]+step;v=v<0 ? 0:(v>1 ? 1:v);spread[hand][s]=v; }
                float shown[5];std::memcpy(shown,bend[hand],sizeof(shown));
                if(r.Chance(6)) { const int d=int(r.Next()%5);shown[d]=0; }   // a glove's dropout: a straight finger
                TraceFingerHand(r,shown,spread[hand]);
                if(r.Chance(30)) { const int k=int(r.Next()%XR_HAND_JOINT_COUNT_EXT);testHand.joints[k].locationFlags&=~XrSpaceLocationFlags(XR_SPACE_LOCATION_POSITION_VALID_BIT); }
                if(r.Chance(60)) { const int k=int(r.Next()%XR_HAND_JOINT_COUNT_EXT);testHand.joints[k].pose.position=testHand.joints[(k+1)%XR_HAND_JOINT_COUNT_EXT].pose.position; }
                testHand.locateResult=r.Chance(40) ? XR_ERROR_RUNTIME_FAILURE:XR_SUCCESS;
                testHand.active=r.Chance(30) ? XR_FALSE:XR_TRUE;
                const bool nullBase=r.Chance(50);const int64_t when=int64_t(r.Next()>>8);
                const int which=r.Chance(200) ? (r.Chance(2) ? -1:2):hand;
                const int grip=cvr::fingergrip::Evaluate(which,nullBase ? XR_NULL_HANDLE:reinterpret_cast<XrSpace>(uintptr_t(0x8000)),XrTime(when));
                h.I(grip);cover[grip+1]++;
                float curl[5]{},splay[4]{};
                const bool haveCurl=cvr::fingergrip::TrackedCurl(hand,curl);h.B(haveCurl);cover[3]+=haveCurl;if(haveCurl)h.Fs(curl,5);
                const bool haveSplay=cvr::fingergrip::TrackedSplay(hand,splay);h.B(haveSplay);cover[4]+=haveSplay;if(haveSplay)h.Fs(splay,4);
            }
            hashCalls();
            g_fingerTraceMs+=r.Chance(150) ? 16000+r.Next()%9000000:(r.Chance(20) ? 200+r.Next()%400:r.Next()%40);
        }
        cvr::fingergrip::OnSessionDestroying();hashCalls();
        { float curl[5]{};h.B(cvr::fingergrip::TrackedCurl(0,curl)); }
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"fingergrip: none %d open %d grip %d curls %d spreads %d; log spike %d jump %d zero %d other %d\n",
        cover[0],cover[1],cover[2],cover[3],cover[4],testFingerLogCounts[0],testFingerLogCounts[1],testFingerLogCounts[2],testFingerLogCounts[3]);
    std::filesystem::remove_all(dir);testConfigDir.clear();cvr::fingergrip::SetGripFromFingers(false);g_VRIKClockMs=nullptr;
}
// The camera link and the camera math (src/Camera/CameraLink.cpp, CameraMath.cpp): the anchor recipe, the main eye's
// centres, the composed quaternion, the write ring found exactly, nearly, across the sign and not at all, the placed
// transforms (exact, ambiguous, another origin), the three sequence-locked packets, the body anchor yaw, the half IPD
// and its scales, the plausibility tests, and the camera basis writes into a buffer whose tail is unreadable -- the
// diagnostics on and off, every counter hashed.
void TraceCameraPose(TraceRng& r,OpenXRHeadPose& p) {
    float v[3];r.Vec(v,2);p.posX=v[0];p.posY=v[1];p.posZ=v[2];
    float q[4];r.Quat(q,3.1f,1.2f,.5f);p.oriX=q[0];p.oriY=q[1];p.oriZ=q[2];p.oriW=q[3];
    p.valid=!r.Chance(8);
    { const bool other=r.Chance(10);p.originSerial=testOriginSerial-(other ? 1:0); }
    { const float a=r.Range(-.2f,.2f),b=r.Range(-.2f,.2f),c=r.Range(0,.5f);p.bodyBend.right=a;p.bodyBend.forward=b;p.bodyBend.angle=c; }
    { const float d=r.Range(0,.6f);p.crouchDepth=d; }
    p.physicalCrouch=r.Chance(3);p.afwEye=int8_t(int(r.Next()%3)-1);
}
void TraceHashPose(TraceHash& h,const OpenXRHeadPose& p) {
    h.F(p.posX);h.F(p.posY);h.F(p.posZ);h.F(p.oriX);h.F(p.oriY);h.F(p.oriZ);h.F(p.oriW);h.B(p.valid);h.I(int64_t(p.originSerial));
    h.F(p.bodyBend.right);h.F(p.bodyBend.forward);h.F(p.bodyBend.angle);h.F(p.crouchDepth);h.B(p.physicalCrouch);h.I(p.afwEye);
}
float TraceOddFloat(TraceRng& r,float a,float b) {
    const int k=int(r.Next()%24);
    if(k==0)return std::numeric_limits<float>::quiet_NaN();
    if(k==1)return std::numeric_limits<float>::infinity();
    if(k==2)return 0;
    if(k==3)return -r.Range(a,b);
    return r.Range(a,b);
}
void TraceCamera(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    SYSTEM_INFO si{};GetSystemInfo(&si);const size_t page=si.dwPageSize;
    auto* block=static_cast<uint8_t*>(VirtualAlloc(nullptr,page*2,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
    DWORD old=0;VirtualProtect(block+page,page,PAGE_NOACCESS,&old);
    float* pageEnd=reinterpret_cast<float*>(block+page);
    std::vector<std::array<float,4>> quats;std::vector<std::array<int32_t,3>> positions,eyes;
    auto pick=[&](auto& pool,auto make) { if(pool.empty() || r.Chance(4)) { pool.push_back(make());if(pool.size()>24)pool.erase(pool.begin()); }
                                          return pool[r.Next()%pool.size()]; };
    auto newQuat=[&] { std::array<float,4> q;r.Quat(q.data(),3.1f,1.2f,.6f);return q; };
    auto newPos=[&] { std::array<int32_t,3> v;for(auto& c:v) { const int64_t x=int64_t(r.Next()%2000000)-1000000;c=int32_t(x); } return v; };
    auto hashCounters=[&] {
        h.I(int64_t(CyberpunkVR_BodyCentreMatch.load()));h.I(int64_t(CyberpunkVR_BodyCentreMiss.load()));
        h.I(int64_t(CyberpunkVR_BodyLuaCentreMatch.load()));h.I(int64_t(CyberpunkVR_BodyLuaCentreRejected.load()));
        h.I(int64_t(CyberpunkVR_PlacedPoseMatch.load()));h.I(int64_t(CyberpunkVR_PlacedPoseMiss.load()));h.I(int64_t(CyberpunkVR_PlacedPoseAmbiguous.load()));
        h.I(int64_t(CyberpunkVR_DebugFinalExact));h.I(int64_t(CyberpunkVR_DebugFinalApprox));h.I(int64_t(CyberpunkVR_DebugFinalExactTies));
    };
    for(int it=0;it<40000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(5) ? 0:1;
        if(r.Chance(500))++testOriginSerial;
        const int action=int(r.Next()%12);
        if(action==0) {
            const int what=int(r.Next()%4);
            if(what==0) { cvr::camera::AnchorRecipe a{};float v[3];r.Vec(v,.3f);a.trackingOffset={v[0],v[1],v[2]};r.Vec(v,.3f);a.modelOffset={v[0],v[1],v[2]};
                          const float s=r.Range(.5f,1.5f),y0=r.Range(-3,3),y1=r.Range(-3,3),y2=r.Range(-3,3);a.scale=s;a.trackingYaw=y0;a.modelYaw=y1;a.centreYaw=y2;a.bodyOrbit=r.Chance(2);
                          cvr::camera::AnchorRecipePublish(a); }
            else if(what==1)cvr::camera::AnchorRecipeClear();
            cvr::camera::AnchorRecipe out{};const bool ok=cvr::camera::AnchorRecipeRead(&out);h.B(ok);
            if(ok) { h.Fs(&out.trackingOffset.x,3);h.Fs(&out.modelOffset.x,3);h.F(out.scale);h.F(out.trackingYaw);h.F(out.modelYaw);h.F(out.centreYaw);h.B(out.bodyOrbit); }
            h.B(cvr::camera::AnchorRecipeRead(nullptr));
        } else if(action==1) {
            const auto eye=pick(eyes,newPos);
            if(r.Chance(2)) { auto centre=eye;auto base=eye;for(int k=0;k<3;++k) { const int64_t dc=int64_t(r.Next()%8000)-4000,db=int64_t(r.Next()%400000)-200000;centre[k]+=int32_t(dc);base[k]=centre[k]+int32_t(db); }
                              cvr::camera::MainEyeCentrePublish(eye.data(),centre.data(),base.data()); }
            const auto probe=r.Chance(6) ? newPos():eye;
            int32_t centre[3]{},base[3]{};const bool wantBase=r.Chance(2);
            const bool ok=cvr::camera::MainEyeCentreRead(probe.data(),centre,wantBase ? base:nullptr);h.B(ok);for(int k=0;k<3;++k) { h.I(centre[k]);h.I(base[k]); }
            float world[3];for(int k=0;k<3;++k)world[k]=float(probe[k])/131072.0f;
            if(r.Chance(3) && ok)for(int k=0;k<3;++k)world[k]=float(centre[k])/131072.0f;
            float wc[3]{},wb[3]{};const bool okw=cvr::camera::MainEyeCentreReadWorld(world,wc,r.Chance(2) ? wb:nullptr);h.B(okw);h.Fs(wc,3);h.Fs(wb,3);
        } else if(action==2) {
            if(r.Chance(2)) { OpenXRHeadPose p{};TraceCameraPose(r,p);const auto q=pick(quats,newQuat);const bool withConsumed=r.Chance(2);
                              cvr::roomscale::Vec2 consumed{};{ const float x=r.Range(-.1f,.1f),y=r.Range(-.1f,.1f);consumed={x,y}; }
                              const uint64_t source=r.Chance(2) ? 0:r.Next();
                              cvr::camera::CamWriteQuatPublish(q[0],q[1],q[2],q[3],p,withConsumed ? &consumed:nullptr,source); }
            float q[4]{9,9,9,9};OpenXRHeadPose p{};cvr::roomscale::Vec2 consumed{};bool known=false;uint64_t id=7;
            const bool full=r.Chance(2);
            const bool ok=full ? cvr::camera::CamWriteQuatRead(q,&p,&consumed,&known,&id):cvr::camera::CamWriteQuatRead(q);
            h.B(ok);h.Fs(q,4);TraceHashPose(h,p);h.F(consumed.x);h.F(consumed.y);h.B(known);h.I(int64_t(id));
        } else if(action==3) {
            if(r.Chance(2)) { OpenXRHeadPose p{};TraceCameraPose(r,p);const auto q=pick(quats,newQuat);cvr::camera::CamWriteRecordPush(q.data(),p); }
            auto q=pick(quats,newQuat);const int how=int(r.Next()%6);
            if(how==1)for(auto& c:q)c=-c;
            else if(how==2) { const int k=int(r.Next()%4);q[k]=std::nextafter(q[k],2.0f); }
            else if(how==3) { const int k=int(r.Next()%4);const float e=r.Range(-3e-4f,3e-4f);q[k]+=e; }
            else if(how==4)q=newQuat();
            else if(how==5 && r.Chance(4))q[r.Next()%4]=std::numeric_limits<float>::quiet_NaN();
            OpenXRHeadPose out{};uint32_t age=99,ties=99;const bool nulls=r.Chance(5);
            const bool ok=cvr::camera::CamWriteRecordFind(q.data(),nulls ? nullptr:&out,nulls ? nullptr:&age,nulls ? nullptr:&ties);
            h.B(ok);TraceHashPose(h,out);h.I(age);h.I(ties);
        } else if(action==4) {
            CyberpunkVR_OneSamplePerFrame=!r.Chance(8);CyberpunkVR_CamWriteInPatch=!r.Chance(8);CyberpunkVR_CamComposeAtWrite=!r.Chance(8);
            CyberpunkVR_CamWriteInFinal=r.Chance(8);CyberpunkVR_HeadTranslationInPatch=!r.Chance(8);CyberpunkVR_VrcamHeadTranslation=!r.Chance(8);
            CyberpunkVR_DeltaFromFreshSample=!r.Chance(8);CyberpunkVR_VrcamPosFromMain=r.Chance(8) ? 3:int32_t(r.Next()%3);
            g_isInVehicle=r.Chance(8);testDeviceCam=r.Chance(8);g_bdActive=r.Chance(8) ? 1:0;
            h.B(cvr::camera::PlacedCameraPoseEnabled());
            g_isInVehicle=false;testDeviceCam=false;g_bdActive=0;
            const uint32_t view=uint32_t(r.Chance(10) ? r.Next()%4:1+r.Next()%2);
            const auto q=pick(quats,newQuat);const auto pos=pick(positions,newPos);
            if(r.Chance(2)) { OpenXRHeadPose p{};TraceCameraPose(r,p);if(r.Chance(30))p.posX=std::numeric_limits<float>::quiet_NaN();
                              cvr::camera::PlacedCameraPosePush(view,q.data(),pos.data(),p); }
            auto fq=q;if(r.Chance(4)) { const int k=int(r.Next()%4);const float e=r.Range(-2e-6f,2e-6f);fq[k]+=e; }
            OpenXRHeadPose out{};uint32_t age=99,ties=99;
            const bool ok=cvr::camera::PlacedCameraPoseFind(view,fq.data(),pos.data(),&out,&age,&ties);h.B(ok);TraceHashPose(h,out);h.I(age);h.I(ties);
        } else if(action==5) {
            const int which=int(r.Next()%3);
            if(r.Chance(2)) {
                if(which==0) { cvr::camera::LocatedCameraFrame f{};for(auto& v:f.worldPos)v=r.Range(-100,100);for(auto& v:f.worldQuat)v=r.Range(-1,1);
                               f.sequence=uint32_t(r.Next());f.frameEpoch=uint32_t(r.Next());f.bodyCentreKnown=uint32_t(r.Next()%2);for(auto& v:f.bodyBaseWorld)v=r.Range(-100,100);
                               cvr::camera::LocatedCameraFramePublish(f); }
                else if(which==1) { cvr::camera::ViewFrame f{};float* w=&f.viewQuat[0];for(int k=0;k<14;++k)w[k]=r.Range(-5,5);cvr::camera::ViewFramePublish(f); }
                else { cvr::camera::BarrelFrame f{};float* w=&f.camQuat[0];for(int k=0;k<7;++k)w[k]=r.Range(-1,1);cvr::camera::BarrelFramePublish(f); }
            }
            cvr::camera::LocatedCameraFrame lf{};h.B(cvr::camera::LocatedCameraFrameRead(&lf));h.Fs(lf.worldPos,3);h.Fs(lf.worldQuat,4);h.I(lf.sequence);h.I(lf.frameEpoch);h.I(lf.bodyCentreKnown);h.Fs(lf.bodyBaseWorld,3);
            cvr::camera::ViewFrame vf{};h.B(cvr::camera::ViewFrameRead(&vf));h.Fs(&vf.viewQuat[0],14);
            cvr::camera::BarrelFrame bf{};h.B(cvr::camera::BarrelFrameRead(&bf));h.Fs(&bf.camQuat[0],7);
            h.B(cvr::camera::ViewFrameRead(nullptr));h.B(cvr::camera::BarrelFrameRead(nullptr));h.B(cvr::camera::LocatedCameraFrameRead(nullptr));
        } else if(action==6) {
            g_isInVehicle=r.Chance(6);CyberpunkVR_EngineBodyYawValid=!r.Chance(6);
            { const float z=TraceOddFloat(r,0,1),w=TraceOddFloat(r,0,1);CyberpunkVR_EngineBodyYawZ=z;CyberpunkVR_EngineBodyYawW=w; }
            const float fallback=r.Range(-3,3);h.F(cvr::camera::BodyAnchorYaw(fallback));
            g_isInVehicle=false;CyberpunkVR_EngineBodyYawValid=0;CyberpunkVR_EngineBodyYawZ=0;CyberpunkVR_EngineBodyYawW=1;
        } else if(action==7) {
            { const float ws=TraceOddFloat(r,.2f,3),is=TraceOddFloat(r,.2f,3),ss=TraceOddFloat(r,.2f,3),ipd=TraceOddFloat(r,0,.08f);
              g_liveControls.xrWorldScale=ws;g_liveControls.xrIpdScale=is;g_liveControls.xrStereoScale=ss;testRuntimeIpd=ipd; }
            h.F(GetWorldScale());h.F(GetDesiredHalfIpd());h.F(CyberpunkVRPort_HalfIpd());
        } else if(action==8) {
            float v[4];for(auto& c:v)c=TraceOddFloat(r,0,1.4f);if(r.Chance(3))v[3]=r.Range(.7f,1.3f);
            h.B(IsPlausibleUnitVector3(v));h.B(IsPlausibleUnitQuaternion(v));h.B(IsPlausiblePositionVec4(v));
            h.B(IsPlausibleUnitVector3(nullptr));h.B(IsPlausibleUnitQuaternion(nullptr));h.B(IsPlausiblePositionVec4(nullptr));
            float right[3]{5,5,5};ComputeRightVectorFromQuaternion(v,right);h.Fs(right,3);ComputeRightVectorFromQuaternion(nullptr,right);
        } else {
            const int n=int(r.Next()%238)+3;float* rsi=pageEnd-n;
            for(int k=0;k<n;++k)rsi[k]=r.Range(-2,2);
            if(r.Chance(2)) { auto* fp=reinterpret_cast<int32_t*>(rsi);for(int k=0;k<3;++k) { const int64_t x=int64_t(r.Next()%4000000000ull)-2000000000;fp[k]=int32_t(x); } }
            if(n>210 && r.Chance(3))for(int k=204;k<207;++k)rsi[k]=r.Range(-.02f,.02f);
            CyberpunkVR_CamFinalRowOrder=r.Chance(4) ? 0:1;
            float q[4];r.Quat(q,3.1f,1.2f,.6f);if(r.Chance(8)) { const float s=r.Range(.2f,2.5f);for(auto& c:q)c*=s; }
            if(action==9)WriteRenderCameraBasis(rsi,q);else ApplyFinalCameraOrientationFromQuat(rsi,q);
            if(r.Chance(20)) { WriteRenderCameraBasis(nullptr,q);ApplyFinalCameraOrientationFromQuat(rsi,nullptr); }
            h.Fs(rsi,n);
        }
        hashCounters();
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"camera: final exact %llu approx %llu ties %llu; placed match %llu miss %llu ambiguous %llu; centre match %llu miss %llu; world match %llu rejected %llu\n",
        (unsigned long long)CyberpunkVR_DebugFinalExact,(unsigned long long)CyberpunkVR_DebugFinalApprox,(unsigned long long)CyberpunkVR_DebugFinalExactTies,
        (unsigned long long)CyberpunkVR_PlacedPoseMatch.load(),(unsigned long long)CyberpunkVR_PlacedPoseMiss.load(),(unsigned long long)CyberpunkVR_PlacedPoseAmbiguous.load(),
        (unsigned long long)CyberpunkVR_BodyCentreMatch.load(),(unsigned long long)CyberpunkVR_BodyCentreMiss.load(),
        (unsigned long long)CyberpunkVR_BodyLuaCentreMatch.load(),(unsigned long long)CyberpunkVR_BodyLuaCentreRejected.load());
    VirtualFree(block,0,MEM_RELEASE);
    CyberpunkVR_RuntimeDiagnostics=1;CyberpunkVR_CamFinalRowOrder=1;CyberpunkVR_OneSamplePerFrame=1;CyberpunkVR_CamWriteInPatch=1;
    CyberpunkVR_CamComposeAtWrite=1;CyberpunkVR_CamWriteInFinal=0;CyberpunkVR_HeadTranslationInPatch=1;CyberpunkVR_VrcamHeadTranslation=1;
    CyberpunkVR_DeltaFromFreshSample=1;CyberpunkVR_VrcamPosFromMain=0;g_liveControls.xrWorldScale=1;g_liveControls.xrIpdScale=1;
    g_liveControls.xrStereoScale=1;testRuntimeIpd=.064f;cvr::camera::AnchorRecipeClear();
}
void Trace() {
    struct Section { const char* name; void (*run)(TraceHash&,uint64_t); uint64_t seed; };
    const Section sections[]={{"stance",TraceStance,0x9E3779B97F4A7C15ull},{"body",TraceBody,0xD1B54A32D192ED03ull},
        {"vehicle",TraceVehicle,0x8CB92BA72F3D8DD7ull},{"water_ladder",TraceSwimmingAndLadder,0xABC98388FB8FAC03ull},
        {"cyberware",TraceCyberware,0x5851F42D4C957F2Dull},{"misc",TraceMisc,0x2545F4914F6CDD1Dull},
        {"snapshot",TraceSnapshot,0x9FB21C651E98DF25ull},{"fingers",TraceFingers,0xC2B2AE3D27D4EB4Full},
        {"gestures",TraceGestures,0x165667B19E3779F9ull},{"wire",TraceWire,0x27D4EB2F165667C5ull},
        {"weapon_arm",TraceWeaponArm,0x85EBCA77C2B2AE63ull},{"smoke_reload",TraceSmokeReload,0xD6E8FEB86659FD93ull},
        {"weapon_rig",TraceWeaponRig,0xA0761D6478BD642Full},{"wheel",TraceWheel,0xE7037ED1A0B428DBull},
        {"recoil",TraceRecoil,0x8EBC6AF09C88C6E3ull},{"grasp",TraceGrasp,0x4CF5AD432745937Full},
        {"two_hand",TraceTwoHand,0xBF58476D1CE4E5B9ull},{"hook",TraceHook,0x94D049BB133111EBull},{"haptics",TraceHaptics,0x2127599BF4325C37ull},
        {"fingergrip",TraceFingerGrip,0x9E6C63D0676A9A99ull},{"camera",TraceCamera,0x3C6EF372FE94F82Bull}};
    for(const auto& s:sections) {
        TraceHash h;s.run(h,s.seed);
        std::printf("%s=%016llx/%llu ",s.name,static_cast<unsigned long long>(h.h),static_cast<unsigned long long>(h.n));
    }
    std::printf("\n");
}

#ifdef VRIK_ROOM_TESTS
// THE ROOM TRACE: the body hooks (src/Hooks/RoomscaleMove.cpp, BodyHeading.cpp, BodyYawFollow.cpp, or their Rust port)
// driven through their detours on fake engine objects -- the player's movement state, its provider, adapter and CCT
// backend, cross-linked as the engine links them, each link broken now and then -- with fake originals standing in
// for the engine's move, velocity getter, property reader, motion request and adapter apply. Every mode of the body's
// turn (head cone, tracked, hybrid), strict and follow, swimming and its feedback, the gates, the player changing,
// the published frames and pairs; every output and every call the fakes saw, hashed.
uint64_t g_roomUs=1000000,g_roomMs=5000;size_t g_roomFlushes=0;
int64_t RoomQpc() { return int64_t(g_roomUs); }
uint64_t RoomMs() { return g_roomMs; }
struct RoomFakes {
    uintptr_t base=0;
    alignas(16) uint8_t player[64]{},state[0x200]{},provider[0xD0]{},adapter[0x60]{},backend[0x190]{},other[0x200]{};
    float accept=1,velocityScale=1,turn=0;uintptr_t moveResult=0,propertyResult=1;bool applyAccepts=true;
    std::vector<float> calls;
} g_room;
template<class T> void RoomPut(uint8_t* b,size_t off,T v) { std::memcpy(b+off,&v,sizeof(v)); }
template<class T> T RoomGet(const uint8_t* b,size_t off) { T v;std::memcpy(&v,b+off,sizeof(v));return v; }
uintptr_t __fastcall RoomFakeMove(void* backend,float* d,float dt) {
    auto* b=static_cast<uint8_t*>(backend);
    g_room.calls.insert(g_room.calls.end(),{1.f,d[0],d[1],d[2],dt});
    for(int k=0;k<3;++k) {
        const float v=dt>0 ? d[k]/dt*(k<2 ? g_room.accept:1.f):0.f;
        RoomPut<float>(b,0x114+4*k,v);RoomPut<float>(b,0x108+4*k,RoomGet<float>(b,0x108+4*k)+v*dt);
    }
    return g_room.moveResult;
}
float* __fastcall RoomFakeVelocity(void*,float* out) {
    for(int k=0;k<3;++k)out[k]=RoomGet<float>(g_room.backend,0x114+4*k)*g_room.velocityScale;
    g_room.calls.push_back(2.f);return out;
}
uintptr_t __fastcall RoomFakeProperty(void*,uint32_t handle,uint32_t,uint32_t,uint32_t property,void* out,uint32_t bytes) {
    g_room.calls.insert(g_room.calls.end(),{3.f,float(handle),float(property),float(bytes)});
    if(out && bytes==12) { auto* v=static_cast<float*>(out);for(int k=0;k<3;++k)v[k]=RoomGet<float>(g_room.backend,0x114+4*k)+.5f; }
    return g_room.propertyResult;
}
void __fastcall RoomFakeMotion(void*,float* position,float* rotation) {
    const float h=g_room.turn*.5f;rotation[0]=0;rotation[1]=0;rotation[2]=std::sin(h);rotation[3]=std::cos(h);
    position[0]=.01f;position[1]=-.02f;position[2]=0;g_room.calls.push_back(4.f);
}
bool __fastcall RoomFakeApply(void* adapter,const float*,const float* rotation,uint8_t flag) {
    g_room.calls.insert(g_room.calls.end(),{5.f,rotation[0],rotation[1],rotation[2],rotation[3],float(flag)});
    if(!g_room.applyAccepts)return false;
    auto* a=static_cast<uint8_t*>(adapter);
    const float z=RoomGet<float>(a,0x38),w=RoomGet<float>(a,0x3C);
    const float nz=w*rotation[2]+z*rotation[3],nw=w*rotation[3]-z*rotation[2];
    const float n=std::sqrt(nz*nz+nw*nw);RoomPut<float>(a,0x38,nz/n);RoomPut<float>(a,0x3C,nw/n);
    return true;
}
void RoomLink(TraceRng& r) {
    auto& f=g_room;const uint32_t handle=0x5151;
    const auto at=[](void* p) { return reinterpret_cast<uintptr_t>(p); };
    std::memset(f.state,0,sizeof(f.state));
    RoomPut<uintptr_t>(f.state,0x90,at(f.player));RoomPut<uintptr_t>(f.state,0xA8,at(f.provider));RoomPut<uintptr_t>(f.state,0x160,at(f.adapter));
    RoomPut<uint8_t>(f.state,0x8B,1);RoomPut<float>(f.state,0x1D8,0);RoomPut<float>(f.state,0x1DC,1);
    std::memset(f.provider,0,sizeof(f.provider));RoomPut<uintptr_t>(f.provider,0,f.base+0x3098448);RoomPut<uintptr_t>(f.provider,0x10,at(f.state));
    RoomPut<uint32_t>(f.provider,0xC4,handle);
    std::memset(f.adapter,0,sizeof(f.adapter));RoomPut<uintptr_t>(f.adapter,0,f.base+0x3098390);RoomPut<uintptr_t>(f.adapter,0x18,at(f.state));
    RoomPut<float>(f.adapter,0x3C,1);RoomPut<uint32_t>(f.adapter,0x40,handle);
    std::memset(f.backend,0,sizeof(f.backend));RoomPut<uintptr_t>(f.backend,0,f.base+0x31EB5E8);RoomPut<uintptr_t>(f.backend,8,at(f.player));
    RoomPut<uint32_t>(f.backend,0x30,handle);RoomPut<uint8_t>(f.backend,0x185,1);
    { float v[3];r.Vec(v,50);for(int k=0;k<3;++k)RoomPut<float>(f.backend,0x108+4*k,v[k]); }
}
void RoomHashExports(TraceHash& h) {
    h.I(int64_t(CyberpunkVR_BodyHeadingCommits));h.I(int64_t(CyberpunkVR_BodyHeadingRequests));h.F(CyberpunkVR_BodyTrackingYawRad);
    h.F(CyberpunkVR_BodyYawRealignRad);h.F(CyberpunkVR_DebugBodyFollowOffsetDeg);h.I(CyberpunkVR_BodyYawFollow);h.F(CyberpunkVR_BodyYawFollowDeadDeg);
    h.F(CyberpunkVR_DebugHybridTrackedWeight);h.F(CyberpunkVR_DebugHybridHandsDeg);h.F(CyberpunkVR_DebugHybridNeckPullDeg);
    h.F(CyberpunkVR_DebugHybridAlignDeg);h.I(CyberpunkVR_DebugHybridEstimate);h.Fs(CyberpunkVR_DebugTrackedTrace,8);h.I(CyberpunkVR_DebugTrackedGates);
    h.F(CyberpunkVR_DebugBodyFollowErrDeg);h.I(int64_t(CyberpunkVR_DebugBodyFollowCalls));h.I(int64_t(CyberpunkVR_DebugBodyFollowApplied));
    h.F(CyberpunkVR_BodyYawFinalRad);h.I(CyberpunkVR_BodyYawFinalValid);h.Fs(CyberpunkVR_PlayerEntityPos,3);h.Fs(CyberpunkVR_PlayerEntityQuat,4);
    h.I(CyberpunkVR_PlayerEntityValid);h.I(CyberpunkVR_EngineBodyYawValid);h.Fs(CyberpunkVR_SwimmingRedirect,3);
    h.I(int64_t(CyberpunkVR_SwimmingFeedbackReads[0]));h.I(int64_t(CyberpunkVR_SwimmingFeedbackReads[1]));h.I(int64_t(CyberpunkVR_SwimmingPropertyFeedbackReads));
    h.I(int64_t(CyberpunkVR_DebugVrikNativePairPublished));h.I(int64_t(CyberpunkVR_DebugVrikNativePairRejected));h.I(testPlayerChanged);
    h.I(int64_t(CyberpunkVR_RoomscaleDebugSeq.load()));
    // The snapshot's pointers are this program's addresses: hashed as offsets from the fakes.
    RoomscaleDebug d=CyberpunkVR_RoomscaleDebug;const uintptr_t at=reinterpret_cast<uintptr_t>(&g_room);
    d.player=d.player ? d.player-at:0;d.movement=d.movement ? d.movement-at:0;d.backend=d.backend ? d.backend-at:0;
    const auto* w=reinterpret_cast<const uint32_t*>(&d);
    for(size_t k=0;k<sizeof(d)/4;++k)h.Bytes(w[k],4);
    h.I(int64_t(g_room.calls.size()));for(const float v:g_room.calls)h.F(v);g_room.calls.clear();
    h.I(int64_t(testFlushes.size()));for(const float v:testFlushes)h.F(v);g_roomFlushes+=testFlushes.size();testFlushes.clear();
}
void TraceRoom(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);auto& f=g_room;f.base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    g_VRIKClockQpc=RoomQpc;g_VRIKClockMs=RoomMs;
    cvr_roomscale_bind(f.base,reinterpret_cast<void*>(&RoomFakeMove),reinterpret_cast<void*>(&RoomFakeVelocity),reinterpret_cast<void*>(&RoomFakeProperty));
    cvr_body_heading_bind(f.base,reinterpret_cast<void*>(&RoomFakeApply),reinterpret_cast<void*>(&RoomFakeMotion));
    float hands[256]{};float* savedShared=g_pSharedHands;
    RoomLink(r);cvr::roomscale::SetPlayer(reinterpret_cast<uintptr_t>(f.player));
    float headX=0,headY=0,headYaw=0;uint64_t sequence=1;
    const bool dump=std::getenv("CVR_ROOM_DUMP")!=nullptr;
    auto mark=[&](int it,const char* what) { if(dump)std::fprintf(stderr,"%d %s %016llx %llu\n",it,what,static_cast<unsigned long long>(h.h),static_cast<unsigned long long>(h.n)); };
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        if(r.Chance(400))++testOriginSerial;
        if(r.Chance(300)) { RoomLink(r);const bool other=r.Chance(3);cvr::roomscale::SetPlayer(reinterpret_cast<uintptr_t>(other ? f.other:f.player)); }
        if(r.Chance(200))cvr::roomscale::SetPlayer(reinterpret_cast<uintptr_t>(f.player));
        // the gates and the controls
        g_liveControls.xrRoomscaleMovement=!r.Chance(15);g_liveControls.xrBodyMoveRadius=r.Chance(4) ? r.Range(0,.3f):.10f;
        g_liveControls.xrBodyRotationMode=r.Chance(30) ? 0:int(1+r.Next()%3);
        g_liveControls.xrBodyFreeLookDeg=r.Range(0,40);g_liveControls.xrBodyFreeLookDownDeg=r.Range(0,60);g_liveControls.xrBodyFreeLookSwimDeg=r.Range(0,40);
        g_liveControls.xrCutsceneSuspendTier=int(r.Next()%6);g_liveControls.xr3DofMovement=r.Chance(20);
        g_VRLocomotionState=r.Chance(10) ? int(r.Next()%12):0;g_VRBind=r.Chance(10) ? int(r.Next()%5):4;g_isInVehicle=r.Chance(40);
        g_sceneTier=r.Chance(8) ? int(r.Next()%7):1;testCutsceneTier=r.Chance(3) ? int(r.Next()%6):-1;
        g_menuModeValue=r.Chance(25);testLobby=r.Chance(3);testDeviceCam=r.Chance(40);g_bdActive=r.Chance(40) ? 1:0;testLadder=r.Chance(40);
        testSwimming=r.Chance(12);testFbtCalibrating=r.Chance(40);g_PlayerPoseHeadingReady=!r.Chance(15);
        g_pSharedHands=r.Chance(30) ? nullptr:hands;g_VRBoneCount=r.Chance(30) ? 0:200;g_VRHeadBoneIdx=r.Chance(30) ? -1:22;
        CyberpunkVR_BodyHeadingHooksReady=!r.Chance(20);CyberpunkVR_BodyStrictUnderHead=!r.Chance(4);CyberpunkVR_HeadLeadsCapsule=!r.Chance(6);
        CyberpunkVR_DebugBodyHeadingSkip=r.Chance(30);
        testCapsuleRadius=r.Chance(3) ? 0:(r.Chance(5) ? r.Range(-1,3):.25f);
        // broken links now and then
        if(r.Chance(60)) { const int which=int(r.Next()%6);
            if(which==0)RoomPut<uintptr_t>(f.backend,0,f.base+0x123);else if(which==1)RoomPut<uint32_t>(f.provider,0xC4,7);
            else if(which==2)RoomPut<uint8_t>(f.backend,0x185,0);else if(which==3)RoomPut<float>(f.state,0x1DC,std::numeric_limits<float>::quiet_NaN());
            else if(which==4)RoomPut<uintptr_t>(f.adapter,0,f.base+0x99);else RoomPut<uintptr_t>(f.state,0x90,0x1234); }
        if(r.Chance(50))RoomLink(r);
        cvr::roomscale::ObserveMovement(r.Chance(30) ? f.other:f.state);
        // the head: the room sample, the frame head, the acquired head, the tracked body
        { const float dx=r.Range(-.03f,.03f),dy=r.Range(-.03f,.03f),dyaw=r.Range(-.08f,.08f);headX+=dx;headY+=dy;headYaw+=dyaw; }
        if(r.Chance(200)) { const float jx=r.Range(-1,1);headX+=jx; }
        testRoomSample=cvr::roomscale::Sample{};testRoomSample.head={headX,headY};testRoomSample.sequence=r.Chance(20) ? sequence:++sequence;
        testRoomSample.origin=testOriginSerial-(r.Chance(30) ? 1:0);testRoomSample.stampUs=g_roomUs-(r.Chance(20) ? 400000:r.Next()%8000);
        testRoomSample.valid=!r.Chance(20);testRoomSample.runtimeHeadKnown=r.Chance(2);
        { const float a=r.Range(-.01f,.01f),b=r.Range(-.01f,.01f);testRoomSample.runtimeHead={headX+a,headY+b}; }
        { const float a=r.Range(-.05f,.05f),b=r.Range(-.1f,.0f);testRoomSample.eyeFromNeck={a,b}; }
        { const float a=r.Range(-.1f,.1f),b=r.Range(-.1f,.1f);testRoomSample.bend={a,b}; }
        testRoomSample.upright=!r.Chance(10);testRoomSampleOk=!r.Chance(20);
        float q[4];r.Quat(q,.4f,.6f,.2f);const float yh[4]={0,std::sin(headYaw*.5f),0,std::cos(headYaw*.5f)};float hq[4];VRIK_QuatMul(yh,q,hq);
        testFrameHead=OpenXRHeadPose{};testFrameHead.posX=headX;testFrameHead.posY=1.6f;testFrameHead.posZ=-headY;
        testFrameHead.oriX=hq[0];testFrameHead.oriY=hq[1];testFrameHead.oriZ=hq[2];testFrameHead.oriW=hq[3];testFrameHead.valid=!r.Chance(10);
        testFrameHead.originSerial=testOriginSerial;testFrameTime=r.Chance(10) ? 0:int64_t(g_roomUs*1000);
        testAcquireHead=testFrameHead;testAcquireHead.valid=!r.Chance(15);{ const float a=r.Range(0,.6f);testAcquireHead.bodyBend.angle=a; }
        if(r.Chance(40))testAcquireHead.oriX=testAcquireHead.oriY=0.9f;   // not an orientation
        testAcquireOk=!r.Chance(15);
        testBodyFrame=cvr::body::TrackingFrame{};testBodyFrame.head={hq[0],hq[1],hq[2],hq[3]};testBodyFrame.headPosition={headX,1.6f,-headY};
        for(int s=0;s<2;++s) { const float side=s ? .2f:-.2f,yaw=headYaw+r.Range(-.3f,.3f);
            testBodyFrame.hands[s]={headX+side*std::cos(yaw),1.1f,-headY+side*std::sin(yaw)-.25f};
            testBodyFrame.rotations[s]={0,std::sin(yaw*.5f),0,std::cos(yaw*.5f)};testBodyFrame.tracked[s]=!r.Chance(10); }
        testBodyFrame.valid=!r.Chance(10);testBodyFrame.sequence=sequence;testBodyFrame.stampUs=g_roomUs-r.Next()%5000;
        testBodyFrame.origin=testOriginSerial-(r.Chance(40) ? 1:0);testBodyFrameOk=!r.Chance(10);
        { const float a=r.Range(-1,1),b=r.Range(-1,1),c=r.Range(-1,1);testSteer[0]=a;testSteer[1]=b;testSteer[2]=c; }
        testSteerState=int(r.Next()%3);testSteerOk=r.Chance(3);
        // the motion request and its commit
        f.turn=r.Range(-.05f,.05f);f.applyAccepts=!r.Chance(8);
        float position[3]{},rotation[4]{0,0,0,1};
        cvr_body_heading_motion(r.Chance(20) ? f.other:f.provider,position,rotation);h.Fs(rotation,4);mark(it,"motion");
        if(r.Chance(10))rotation[2]+=1e-6f;
        const uint8_t flag=uint8_t(r.Next()%3);h.B(cvr_body_heading_apply(r.Chance(20) ? f.other:f.adapter,position,rotation,flag));mark(it,"apply");
        // the move
        f.accept=r.Chance(4) ? r.Range(0,1):1;f.moveResult=uintptr_t(r.Next()%4);
        { float disp[3];r.Vec(disp,.05f);const float dt=r.Chance(30) ? 0:r.Range(.005f,.04f);
          const uintptr_t result=cvr_roomscale_move(r.Chance(15) ? f.other:f.backend,disp,dt);h.I(int64_t(result));h.Fs(disp,3); }
        mark(it,"move");for(int k=0;k<3;++k) { h.F(RoomGet<float>(f.backend,0x108+4*k));h.F(RoomGet<float>(f.backend,0x114+4*k)); }mark(it,"backend");
        // the velocity readers, the property reader
        { const uintptr_t callers[]={f.base+0x90B2F0,f.base+0x2EF1676,f.base+0x2EF250A,f.base+0x4C6148};
          f.velocityScale=r.Range(.5f,1.5f);float out[3]{};const uintptr_t caller=callers[r.Next()%4];
          cvr_roomscale_velocity_from(r.Chance(15) ? f.other:f.provider,out,caller);h.Fs(out,3); }
        { float out[3]{};f.propertyResult=uintptr_t(r.Chance(5) ? 0x100:1);const uint32_t property=r.Chance(4) ? 3:4,bytes=r.Chance(5) ? 8:12;
          const uint32_t handle=r.Chance(5) ? 9:0x5151;h.I(int64_t(cvr_roomscale_property(nullptr,handle,0,0,property,r.Chance(20) ? nullptr:out,bytes)));h.Fs(out,3); }
        mark(it,"velocity-property");
        // the API
        { const auto c=cvr::roomscale::CameraConsumed(testOriginSerial-(r.Chance(10) ? 1:0));h.F(c.x);h.F(c.y);
          const auto c2=cvr::roomscale::CameraConsumed();h.F(c2.x);h.F(c2.y);
          cvr::roomscale::Vec2 neck{9,9};h.B(cvr::roomscale::CameraRestNeck(&neck));h.F(neck.x);h.F(neck.y); }
        h.B(cvr::roomscale::PoseFrameAllowed());h.B(cvr::roomscale::GameplayAllowed());h.B(cvr::roomscale::PhysicalBodyEnabled());
        h.B(cvr::roomscale::PhysicalBodyHeadingOwned());h.B(cvr::roomscale::PhysicalBodyAllowed());h.B(cvr::roomscale::BodyFollowing());
        h.B(cvr::roomscale::IsPlayerMovement(f.state));h.B(cvr::roomscale::IsPlayerProvider(f.provider));h.B(cvr::roomscale::IsPlayerProvider(f.other));
        h.I(int64_t(cvr::roomscale::PlayerIdentity()-reinterpret_cast<uintptr_t>(f.player)));
        { const float fb=r.Range(-3,3);h.F(BodyTrackingYaw(fb));h.F(BodyCentreYaw(fb));float y=7;h.B(ReadBodyTrackingYaw(&y));h.F(y); }
        h.F(BodyHeadingOffset());h.B(BodyHeadingReady());h.B(BodyYawFollowActive());h.F(BodyYawFollowOffset());mark(it,"api");
        // the store site's tick and the body frame, the native pair
        { const float yaw=r.Range(-3,3);float pos[3];r.Vec(pos,100);const bool flip=r.Chance(4);
          BodyYawFollowTick(std::sin(yaw*.5f)*(flip ? -1.f:1.f),std::cos(yaw*.5f)*(flip ? -1.f:1.f),r.Chance(30) ? nullptr:pos);
          float bp[3]{},bq[4]{},ty=0;h.B(VRIK_ReadCurrentBodyFrame(bp,bq,&ty));h.Fs(bp,3);h.Fs(bq,4);h.F(ty); }
        if(r.Chance(3)) { int32_t c[3],b[3],e[3];for(int k=0;k<3;++k) { const int64_t x=int64_t(r.Next()%400000)-200000;e[k]=int32_t(x);c[k]=e[k]+int32_t(r.Next()%80000);b[k]=e[k]+int32_t(r.Next()%60000); }
          float cq[4],eq[4];r.Quat(cq,3,.5f,.2f);r.Quat(eq,3,0,0);const uintptr_t owner=r.Chance(10) ? 1:reinterpret_cast<uintptr_t>(f.player);
          VRIK_PublishNativeCameraPair(c,b,e,cq,eq,owner,testOriginSerial,!r.Chance(10)); }
        { VrikTransformSnapshot s{};h.B(VRIK_ReadNativeTransformSnapshot(&s));h.Fs(s.camQuat,4);h.Fs(s.entityQuat,4);h.Fs(s.cameraMinusEntity,3);
          h.I(s.valid);h.Fs(s.bodyCameraMinusEntity,3);h.I(s.unavailable); }
        mark(it,"pairs");if(r.Chance(150))BodyYawFollowRelease();
        if(r.Chance(150))BodyHeadingReset();
        RoomHashExports(h);mark(it,"exports");
        g_roomUs+=r.Chance(100) ? 300000+r.Next()%600000:5000+r.Next()%15000;g_roomMs+=r.Chance(100) ? 300+r.Next()%600:5+r.Next()%15;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"room: moves %llu injected %llu resets %llu feedback reads %llu; heading requests %llu commits %llu; follow calls %llu applied %llu; swim reads %llu/%llu property %llu; pairs %llu/%llu; flush floats %zu\n",
        (unsigned long long)CyberpunkVR_RoomscaleDebug.calls,(unsigned long long)CyberpunkVR_RoomscaleDebug.injected,(unsigned long long)CyberpunkVR_RoomscaleDebug.resets,
        (unsigned long long)CyberpunkVR_RoomscaleDebug.feedbackReads,(unsigned long long)CyberpunkVR_BodyHeadingRequests,(unsigned long long)CyberpunkVR_BodyHeadingCommits,
        (unsigned long long)CyberpunkVR_DebugBodyFollowCalls,(unsigned long long)CyberpunkVR_DebugBodyFollowApplied,(unsigned long long)CyberpunkVR_SwimmingFeedbackReads[0],
        (unsigned long long)CyberpunkVR_SwimmingFeedbackReads[1],(unsigned long long)CyberpunkVR_SwimmingPropertyFeedbackReads,
        (unsigned long long)CyberpunkVR_DebugVrikNativePairPublished,(unsigned long long)CyberpunkVR_DebugVrikNativePairRejected,g_roomFlushes);
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;g_pSharedHands=savedShared;CyberpunkVR_RuntimeDiagnostics=1;
}
// THE INPUT TRACE: the XInput merge (src/Hooks/XInput.cpp, or its Rust port) on scripted controllers and a scripted
// game -- menus, the overlay, the keypad, a braindance, the lobby, vehicles and the gun in a car, ladders, water, the
// scanner at the ear and its navigation, dual wielding, the weapon wheel, the sprint detent and click, the crouch,
// dash and snap turn, the trigger override and the melee impulse -- every pad, every shared slot, every call and key.
void TraceInput(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockQpc=RoomQpc;g_VRIKClockMs=RoomMs;
    float hx=-.15f,hy=-.08f,hz=.08f;int coverCalls[16]{},coverBits[16]{},coverMerged=0;
    for(int it=0;it<40000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(5) ? 0:1;
        auto& lc=g_liveControls;
        lc.xrXInputHook=!r.Chance(40);lc.xrLeftHanded=r.Chance(5);lc.xrLeftHandedBindings=int(r.Next()%3);lc.xrLadderGripClimb=!r.Chance(3);
        lc.xrMovementSpeedMode=r.Chance(3);lc.xrSprintMode=r.Chance(4);lc.xrVehicleGunTrigger=!r.Chance(4);lc.xrWheelHorn=r.Chance(3);
        lc.xrDisableMouseY=r.Chance(2);lc.xrSnapTurn=r.Chance(2);
        { const float a=r.Range(-.05f,.35f),b=r.Range(-.05f,.35f),c=r.Range(.7f,1.05f),d=r.Range(-.1f,1),e=r.Range(10,60);
          lc.xrLeftStickDeadzone=a;lc.xrRightStickDeadzone=b;lc.xrMaxInputThreshold=c;lc.xrVehicleThrottleTrim=d;lc.xrSnapTurnAngleDeg=e; }
        CyberpunkVR_TestPadButtons=r.Chance(50) ? int(r.Next()%0x40000):0;
        g_menuModeValue=r.Chance(12);testVruiCaptures=r.Chance(40);testMenuCaptures=r.Chance(15);testKeypad=r.Chance(25);
        testInBraindance=r.Chance(30);g_bdActive=r.Chance(25) ? 1:0;testLobby=r.Chance(15);
        { const float c=r.Chance(2) ? 0:r.Range(0,1);testCurtain=c; }
        testIntroRunning=r.Chance(4);testClueConsume=r.Chance(5);testReachHeld=r.Chance(15);
        g_uiPopupOpen=r.Chance(12) ? 1:0;if(r.Chance(30))g_uiPopupClosedMs=g_roomMs-r.Next()%1500;
        g_hasWeaponEquipped=!r.Chance(3);g_isInVehicle=r.Chance(6);g_vehicleState=int(r.Next()%6);g_VRBind=r.Chance(6) ? int(r.Next()%5):4;
        CyberpunkVR_WeaponClass=int(r.Next()%6);if(r.Chance(20))cvr::cyberware::Select(int(r.Next()%6));
        g_VRLocomotionState=r.Chance(5) ? int(r.Next()%14):0;g_VRSprintActive=int(r.Next()%3)-1;
        if(r.Chance(30))++CyberpunkVR_LeftStickClicks;
        testCrouchRequested=r.Chance(20);testSwimInput=cvr::swimming::Input{};testSwimInput.water=r.Chance(8);testSwimSprint=r.Chance(4);
        testSwimming=r.Chance(10);testLadder=r.Chance(10);g_isDriving=r.Chance(6);
        { const float b=r.Chance(2) ? 0:r.Range(0,1);cvr::anim::g_wheelBlendRight.store(b);cvr::anim::g_wheelBlendLeft.store(r.Chance(2) ? 0.f:.5f); }
        cvr::anim::g_wheelHornMask=r.Chance(5) ? 1:0;
        reinterpret_cast<uint32_t*>(testShotShared)[81]=r.Chance(20);reinterpret_cast<uint32_t*>(testShotShared)[164]=r.Chance(15);
        testXrShared[29]=r.Chance(15) ? float(r.Next()%4):0.f;testXrShared[161]=r.Chance(8) ? float(r.Next()%3):0.f;
        CyberpunkVR_LeftTriggerBlockUntil=r.Chance(15) ? g_roomMs+r.Next()%200:0;CyberpunkVR_DualWieldUntil=r.Chance(4) ? g_roomMs+1000:0;
        if(r.Chance(6))cvr::input::PublishScannerVision(int(r.Next()%3));
        testSceneYawArmed=r.Chance(20);testDeviceCam=r.Chance(30);testWheelArmed=r.Chance(3);
        // the support hand wanders around the left ear
        { const float a=r.Range(-.03f,.03f),b=r.Range(-.03f,.03f),c=r.Range(-.03f,.03f);hx+=a;hy+=b;hz+=c;
          if(r.Chance(40)) { hx=-.15f;hy=-.08f;hz=.08f; } if(r.Chance(40)) { hx=.2f;hy=-.4f;hz=-.3f; } }
        testGestureOk=!r.Chance(10);testGestureHead=OpenXRHeadPose{};testGestureHead.valid=!r.Chance(20);
        for(int s=0;s<2;++s) { testGestureHands[s]=OpenXRHeadPose{};testGestureHands[s].posX=s ? -hx:hx;testGestureHands[s].posY=hy;testGestureHands[s].posZ=hz;
                               testGestureHands[s].valid=!r.Chance(15); }
        // the controllers
        testControllerOk=!r.Chance(30);testController=VRControllerState{};
        testController.buttons=uint16_t(r.Chance(2) ? (r.Next()&0xFFFF):(r.Next()&0xF0C0));
        { const float a=r.Unit(),b=r.Unit(),c=r.Range(-1.1f,1.1f),d=r.Range(-1.1f,1.1f),e=r.Range(-1.1f,1.1f),f=r.Range(-1.1f,1.1f),g=r.Unit(),k=r.Unit();
          testController.leftTrigger=a;testController.rightTrigger=b;testController.leftThumbX=c;testController.leftThumbY=d;
          testController.rightThumbX=e;testController.rightThumbY=f;testController.leftGrip=g;testController.rightGrip=k; }
        if(r.Chance(3))testController.leftThumbY=r.Chance(2) ? 1.f:.95f;
        if(r.Chance(4))testController.rightThumbY=r.Chance(2) ? -1.f:1.f;
        if(r.Chance(6))testController.leftThumbX=testController.leftThumbY=0;
        testController.leftHandValid=!r.Chance(10);testController.rightHandValid=!r.Chance(10);
        testController.psvr2SystemPressed=r.Chance(5);testController.psvr2SystemAvailable=r.Chance(3);testController.psvr2DpadShift=r.Chance(30);
        // the real pad
        XINPUT_STATE state{};state.dwPacketNumber=uint32_t(r.Next());state.Gamepad.wButtons=uint16_t(r.Chance(4) ? (r.Next()&0xFFFF):0);
        state.Gamepad.bLeftTrigger=uint8_t(r.Chance(4) ? r.Next()%256:0);state.Gamepad.bRightTrigger=uint8_t(r.Chance(4) ? r.Next()%256:0);
        if(r.Chance(4)) { state.Gamepad.sThumbLX=int16_t(int(r.Next()%65536)-32768);state.Gamepad.sThumbLY=int16_t(int(r.Next()%65536)-32768); }
        if(r.Chance(4)) { state.Gamepad.sThumbRX=int16_t(int(r.Next()%65536)-32768);state.Gamepad.sThumbRY=int16_t(int(r.Next()%65536)-32768); }
        const DWORD real=r.Chance(5) ? DWORD(ERROR_DEVICE_NOT_CONNECTED):DWORD(ERROR_SUCCESS);
        const DWORD user=r.Chance(60) ? 1:0;
        const DWORD result=cvr_xinput_merge(user,r.Chance(200) ? nullptr:&state,real);
        h.I(result);h.Bytes(state.dwPacketNumber,4);h.Bytes(state.Gamepad.wButtons,2);h.Bytes(state.Gamepad.bLeftTrigger,1);h.Bytes(state.Gamepad.bRightTrigger,1);
        h.Bytes(uint16_t(state.Gamepad.sThumbLX),2);h.Bytes(uint16_t(state.Gamepad.sThumbLY),2);h.Bytes(uint16_t(state.Gamepad.sThumbRX),2);h.Bytes(uint16_t(state.Gamepad.sThumbRY),2);
        for(const int slot:{29,30,49,57,154,155,156,157,158,159,160,161,165})h.F(testXrShared[slot]);
        h.F(g_lookStickX.load());h.I(g_pendingSnapYawDeltaBits);h.I(cvr::input::ScannerVision());h.B(cvr::input::WeaponTriggerEnabled());
        h.I(int64_t(CyberpunkVR_DebugWheelHornGestures));
        for(size_t k=0;k<testInputCalls.size();++k) { const int c=int(testInputCalls[k]);if(c>=101 && c<=111)coverCalls[c-101]++; }
        if(user==0)for(int b=0;b<16;++b)coverBits[b]+=(state.Gamepad.wButtons>>b)&1;
        coverMerged+=user==0 && testControllerOk;
        h.I(int64_t(testInputCalls.size()));for(const float v:testInputCalls)h.F(v);testInputCalls.clear();
        if(r.Chance(40))g_pendingSnapYawDeltaBits=0;
        const uint64_t step=r.Chance(80) ? 200+r.Next()%800:4+r.Next()%25;g_roomMs+=step;g_roomUs+=step*1000+r.Next()%1000;
    }
    if(std::getenv("CVR_TRACE_COVERAGE")) {
        std::fprintf(stderr,"input: merged %d; calls",coverMerged);for(int k=0;k<11;++k)std::fprintf(stderr," %d:%d",101+k,coverCalls[k]);
        std::fprintf(stderr,"; bits");for(int b=0;b<16;++b)std::fprintf(stderr," %d",coverBits[b]);std::fprintf(stderr,"\n");
    }
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;CyberpunkVR_RuntimeDiagnostics=1;g_isInVehicle=false;g_isDriving=false;
    g_menuModeValue=0;g_bdActive=0;testDeviceCam=false;testLobby=false;testSwimming=false;testLadder=false;g_hasWeaponEquipped=false;
}void RoomTrace() {
    TraceHash h;TraceRoom(h,0x1F83D9ABFB41BD6Bull);
    std::printf("room=%016llx/%llu ",static_cast<unsigned long long>(h.h),static_cast<unsigned long long>(h.n));
    TraceHash in;TraceInput(in,0xA4093822299F31D0ull);
    std::printf("input=%016llx/%llu\n",static_cast<unsigned long long>(in.h),static_cast<unsigned long long>(in.n));
}
#endif

#ifdef VRIK_WATER_TESTS
// THE WATER TRACE: the ladder and the swim strokes (src/Hooks/LadderInput.cpp and SwimmingInput.cpp, or their Rust
// port) over the room fakes' player -- a ladder published, re-published, topped and measured, both hands gripping
// rails and rungs as they wander through it, the climb's pull and the finish at the top; the swim state, scripted
// breaststrokes and ascents, the sprint button, the steering, the script actions, the bound native input and the action
// reader after its original -- every output and every gate.
float g_waterAction[4]{};   // MoveX, MoveY, ToggleSprint, Sprint as the fake original reads them
uint8_t g_waterContext[0x20]{},g_waterLocomotion[0x40]{};uintptr_t g_waterTable[1]{};
float __fastcall WaterFakeAction(void*,uint64_t name) {
    for(int k=0;k<4;++k) { static const char* names[]={"MoveX","MoveY","ToggleSprint","Sprint"};
        uint64_t hh=14695981039346656037ull;for(const char* c=names[k];*c;++c) { hh^=uint8_t(*c);hh*=1099511628211ull; }
        if(hh==name)return g_waterAction[k]; }
    return .25f;
}
uintptr_t __fastcall WaterFakeGetter(void*) { return reinterpret_cast<uintptr_t>(g_waterContext); }
uint64_t WaterHash(const char* text) { uint64_t hh=14695981039346656037ull;while(*text) { hh^=uint8_t(*text++);hh*=1099511628211ull; } return hh; }
void TraceWater(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);auto& f=g_room;f.base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    g_VRIKClockQpc=RoomQpc;g_VRIKClockMs=RoomMs;
    cvr_roomscale_bind(f.base,reinterpret_cast<void*>(&RoomFakeMove),reinterpret_cast<void*>(&RoomFakeVelocity),reinterpret_cast<void*>(&RoomFakeProperty));
    RoomLink(r);const uintptr_t player=reinterpret_cast<uintptr_t>(f.player);cvr::roomscale::SetPlayer(player);
    CvrSwimmingActionOriginal=reinterpret_cast<void*>(&WaterFakeAction);
    g_waterTable[0]=reinterpret_cast<uintptr_t>(&WaterFakeGetter);
    RoomPut<uintptr_t>(g_waterLocomotion,0x30,reinterpret_cast<uintptr_t>(g_waterTable));
    RoomPut<uintptr_t>(g_waterContext,8,reinterpret_cast<uintptr_t>(f.provider));RoomPut<uintptr_t>(g_waterContext,0x10,0x7777);
    float phase=0;int coverHeld=0,coverAxis=0,coverFinish=0,coverStroke=0,coverAscent=0,coverBound=0,coverAuto=0,coverNative=0,coverForward=0,coverSteer=0;uint64_t identity=11;
    const bool dump=std::getenv("CVR_ROOM_DUMP")!=nullptr;
    auto mark=[&](int it,const char* what) { if(dump)std::fprintf(stderr,"%d %s %016llx %llu\n",it,what,static_cast<unsigned long long>(h.h),static_cast<unsigned long long>(h.n)); };
    bool ascend=false,climbing=true,rungs=false,leftDown=false;float reachX=.3f;
    for(int it=0;it<30000;++it) {
        // the odd segments swim: the ladder let go, the gates and the tracking failures rare, the strokes clean; the even
        // ones climb, as calmly, between stretches of noise
        const bool swimming=(it/1500)%2==1,calm=swimming || climbing;const int rare=calm ? 3000:100,fail=calm ? 500:10;
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        auto& lc=g_liveControls;
        lc.xrLadderGripClimb=!r.Chance(calm ? 1000:20);if(r.Chance(calm ? 300:5) || it==0) { lc.xrLadderAutoFinish=!r.Chance(5);const float d=r.Range(0,1.5f);lc.xrLadderFinishDistance=d; }
        lc.xrBreaststrokeSwim=!r.Chance(swimming ? 400:15);
        g_sceneTier=r.Chance(calm ? rare:60) ? int(r.Next()%6):1;g_menuModeValue=r.Chance(calm ? rare:80);testOverlayVisible=r.Chance(rare);g_uiPopupOpen=r.Chance(rare) ? 1:0;
        g_bdActive=r.Chance(rare) ? 1:0;testDeviceCam=r.Chance(rare);g_isInVehicle=r.Chance(rare);testScriptHooks=r.Chance(3);testSimulatorLoaded=r.Chance(2);
        if(r.Chance(300)) { RoomLink(r);cvr::roomscale::SetPlayer(r.Chance(4) ? reinterpret_cast<uintptr_t>(f.other):player); }
        // the body's frame and the native pair, as the store site and the camera write publish them
        { const float yaw=r.Range(-.2f,.2f);float pos[3]={0,0,0};BodyYawFollowTick(std::sin(yaw*.5f),std::cos(yaw*.5f),pos); }
        { int32_t c[3]={0,0,int32_t(1.6f*131072)},b[3]={0,0,int32_t(1.5f*131072)},e[3]={0,0,0};float cq[4]={0,0,0,1},eq[4]={0,0,0,1};
          VRIK_PublishNativeCameraPair(c,b,e,cq,eq,player,testOriginSerial,!r.Chance(calm ? 500:20)); }
        if(r.Chance(50) || it==0) { cvr::camera::AnchorRecipe a{};a.scale=1;a.trackingYaw=0;a.modelYaw=0;a.centreYaw=0;cvr::camera::AnchorRecipePublish(a); }
        // the ladder
        static cvr::ladder::Geometry ladder{};static int ladderState=10;static bool ladderPaused=false;
        if(r.Chance(200) || it==0) {
            cvr::ladder::Geometry g{};g.position={r.Range(-.1f,.1f),.45f,r.Range(-.3f,.1f)};g.up={0,0,1};g.normal={0,-1,0};
            g.height=r.Chance(3) ? r.Range(.5f,.9f):r.Range(3,10);g.topStep=r.Chance(2) ? -.4f:r.Range(-1,1);g.identity=r.Chance(5) ? identity:++identity;
            if(!r.Chance(10))g.Prepare();
            ladder=g;ladderState=r.Chance(10) ? int(r.Next()%14):10+int(r.Next()%3);ladderPaused=r.Chance(20);
        }
        if(swimming ? r.Chance(400):!r.Chance(40))cvr::ladder::Publish(r.Chance(calm ? 1000:100) ? 0:player,ladderState,ladderPaused,r.Chance(calm ? 1000:100) ? nullptr:&ladder);
        // the measured top and the finisher rails: at the hands' height over a short ladder, now and then off the ladder
        if(r.Chance(climbing ? 20:100)) { const float top=r.Chance(2) ? r.Range(1.9f,2.5f):r.Range(2,11);
            h.B(cvr::ladder::PublishMeasuredTop(player,r.Chance(5) ? cvr::ladder::Vec{0,.45f,0}:ladder.position,top)); }
        if(r.Chance(climbing ? 20:100)) { const float top=cvr::ladder::DebugValue(17)+r.Range(-.3f,.3f),side=r.Range(-.2f,.2f),out=r.Range(-.5f,.5f);
            const auto& p=ladder.position;const cvr::ladder::Vec origin{p.x+side,p.y-out,p.z+top};
            h.B(cvr::ladder::PublishTop(player,r.Chance(5) ? cvr::ladder::Vec{0,.45f,0}:p,origin,{1,0,0},{0,-1,0},{0,0,1})); }
        if(r.Chance(400))h.B(cvr::ladder::SetSimulatorGrip(r.Unit(),r.Unit(),int(r.Next()%3000)-200));
        // the swim state
        static int swimState=1,swimFast=0;static bool swimPaused=false,swimContext=false;
        if(r.Chance(150) || it==0) { swimState=swimming && !r.Chance(8) ? 1+int(r.Next()%2):int(r.Next()%4)-1;swimFast=int(r.Next()%4)-2;
            swimPaused=r.Chance(swimming ? 40:10);swimContext=r.Chance(5); }
        if(!r.Chance(40))cvr::swimming::PublishState(r.Chance(rare) ? 0:player,swimState,swimFast,swimPaused,swimContext);
        // the head and the hands: a stroke cycle with noise, now and then wandering or switching to the push down
        phase+=r.Range(.02f,.08f);if(phase>6.2832f)phase-=6.2832f;
        if(r.Chance(swimming ? 150:3))ascend=!ascend;
        if(r.Chance(120))climbing=!climbing;if(r.Chance(300))rungs=!rungs;reachX=std::clamp(reachX+r.Range(-.01f,.01f),.12f,.48f);float climbGrip[2]{-1,-1};
        testGestureOk=!r.Chance(fail);testGestureHead=OpenXRHeadPose{};testGestureHead.posY=1.6f;testGestureHead.oriW=1;testGestureHead.valid=!r.Chance(fail);
        testGestureHead.originSerial=testOriginSerial;if(!r.Chance(10))++testGestureSequence;testGestureStamp=g_roomUs-r.Next()%5000;
        for(int s=0;s<2;++s) {
            auto& hand=testGestureHands[s];hand=OpenXRHeadPose{};hand.oriW=1;hand.valid=!r.Chance(fail);
            const float side=s ? 1.f:-1.f,c=std::cos(phase),sn=std::sin(phase);
            if(!swimming && climbing) {   // hand over hand on the rails or the rungs, each pulling down while it grips
                const float hs=std::sin(phase+3.1416f*s);hand.posX=side*(rungs ? .05f:reachX);hand.posY=-.1f+.25f*hs;hand.posZ=-.45f;
                climbGrip[s]=std::cos(phase+3.1416f*s)<0 ? .9f:.1f; }
            else if(ascend) { hand.posX=side*(.3f+.05f*sn);hand.posY=-.1f-.3f*std::max(0.f,sn);hand.posZ=-.2f; }
            else { hand.posX=side*(.1f+.15f*std::max(0.f,-c));hand.posY=-.2f+.02f*sn;hand.posZ=-.45f+.35f*std::max(0.f,-c); }
            { const float a=!swimming && climbing ? .002f:.01f,nx=r.Range(-a,a),ny=r.Range(-a,a),nz=r.Range(-a,a);hand.posX+=nx;hand.posY+=ny;hand.posZ+=nz; }
            if(r.Chance(calm ? 400:30)) { hand.posX=r.Range(-.6f,.6f);hand.posY=r.Range(-.8f,.6f);hand.posZ=r.Range(-.8f,.3f); }
        }
        VRControllerState c{};
        { const float lg=r.Unit(),rg=r.Unit();c.leftGrip=r.Chance(3) ? lg:(r.Chance(2) ? .9f:.1f);c.rightGrip=r.Chance(3) ? rg:(r.Chance(2) ? .9f:.1f); }
        if(climbGrip[0]>=0 && !r.Chance(25)) { c.leftGrip=climbGrip[0];c.rightGrip=climbGrip[1]; }
        c.leftHandValid=!r.Chance(fail);c.rightHandValid=!r.Chance(fail);
        // the ladder's calls
        // the finisher rails fitted under the left hand as it grips over a short ladder: the pull at the top finishes
        if(climbing && !rungs && ladder.height<1.5f && climbGrip[0]>.5f && !leftDown && cvr::ladder::DebugValue(12)>1) {
            const float hx=cvr::ladder::DebugValue(10),hy=cvr::ladder::DebugValue(11),hz=cvr::ladder::DebugValue(12);const auto& p=ladder.position;
            h.B(cvr::ladder::PublishMeasuredTop(player,p,hz-1.035f-p.z+r.Range(-.1f,.1f)));
            h.B(cvr::ladder::PublishTop(player,p,{hx+.4023f,hy,hz-1.035f},{1,0,0},{0,-1,0},{0,0,1}));
        }
        leftDown=climbGrip[0]>.5f;
        mark(it,"pre");cvr::ladder::UpdateInput(c);
        { const float a=cvr::ladder::NativeAction();h.F(a);coverAxis+=a!=0; }
        { const float m=climbing && !r.Chance(40) ? .14f:.3f,o=r.Range(-1,1),x=r.Range(-m,m),y=r.Range(-m,m);h.F(cvr::ladder::MixMove(o,x,y)); }
        { int kinds[2]{9,9};cvr::ladder::ReadHandKinds(kinds);h.I(kinds[0]);h.I(kinds[1]);coverHeld+=kinds[0]!=0 || kinds[1]!=0;coverFinish+=cvr::ladder::DebugValue(18)>0; }
        for(int s=0;s<2;++s) { float target[3]={.2f*(s ? 1:-1),.3f,1.2f},rot[4]={0,0,0,1},pos[3]={0,0,0},q[4]={0,0,0,1};
            cvr::ladder::ConstrainHand(s,target,rot,pos,q);h.Fs(target,3);h.Fs(rot,4); }
        mark(it,"ladder");for(int k=0;k<=21;++k)if(k!=9)h.F(cvr::ladder::DebugValue(k));mark(it,"ladder-debug");
        // the swim strokes' calls
        const bool manualBack=r.Chance(swimming ? 300:10);
        const auto in=cvr::swimming::UpdateInput(c,manualBack);
        h.F(in.forward);h.B(in.water);h.F(in.ascend);h.B(in.dive);h.B(in.boost);coverForward+=in.forward>0;mark(it,"swim-update");
        h.B(cvr::swimming::SprintInput(r.Chance(2)));
        for(int a=0;a<4;++a)h.F(cvr::swimming::NativeAction(a));
        { XrVector3f d{9,9,9};int st=9;const bool ok=cvr::swimming::ReadSteering(&d,&st);h.B(ok);h.F(d.x);h.F(d.y);h.F(d.z);h.I(st);coverSteer+=ok; }
        { const char* names[]={"MoveY","ToggleSprint","Jump","Dive","Other"};const char* nm=names[r.Next()%5];
          h.F(cvr::swimming::ScriptAction(r.Chance(10) ? 1:player,WaterHash(nm),r.Chance(3))); }
        h.B(cvr::swimming::IsPlayer(player));h.B(cvr::swimming::Active());h.B(cvr::ladder::Active());
        if(r.Chance(3))cvr::swimming::BindNativeInput(r.Chance(10) ? nullptr:g_waterLocomotion);
        { for(auto& v:g_waterAction) { const float x=r.Chance(swimming || climbing ? 60:3) ? r.Range(-1,1):0.f;v=x; }
          const char* names[]={"MoveY","ToggleSprint","Jump","Dive","Look"};const char* nm=names[r.Next()%5];
          const float original=r.Range(0,1);void* object=r.Chance(10) ? nullptr:reinterpret_cast<void*>(uintptr_t(0x7777));
          h.F(CvrSwimmingActionAfter(object,WaterHash(nm),original)); }
        mark(it,"swim-calls");for(int k=0;k<=23;++k)if(k!=6)h.F(cvr::swimming::DebugValue(k));mark(it,"swim-debug");
        coverStroke=int(cvr::swimming::DebugValue(2));coverAscent=int(cvr::swimming::DebugValue(10));coverBound+=cvr::swimming::DebugValue(12)>0;coverAuto+=cvr::swimming::DebugValue(11)>0;coverNative+=cvr::swimming::DebugValue(20)>0;
        if(r.Chance(calm ? 1000:200))cvr::ladder::ResetInput();
        if(r.Chance(swimming ? 1000:200))cvr::swimming::ResetInput();
        const uint64_t step=r.Chance(150) ? 200+r.Next()%400:8+r.Next()%20;g_roomMs+=step;g_roomUs+=step*1000+r.Next()%500;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"water: ladder held %d axis %d finish %d; swim strokes %d ascents %d forward %d steer %d bound %d auto %d native %d\n",coverHeld,coverAxis,coverFinish,coverStroke,coverAscent,
        coverForward,coverSteer,coverBound,coverAuto,coverNative);
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;CyberpunkVR_RuntimeDiagnostics=1;g_isInVehicle=false;g_menuModeValue=0;g_bdActive=0;
    testDeviceCam=false;testOverlayVisible=false;g_uiPopupOpen=0;
}
// THE CHORD TRACE: the arm cyberware's chord (src/Hooks/CyberwareChord.cpp, or its Rust port) -- L3 and the grip held
// through their hysteresis, claimed, fired after half a second, interrupted by a stall, R3 or a gate, dispatched as F18
// with the game in front or left for the script side to take, and the sent-key counter.
void TraceChord(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);g_VRIKClockMs=RoomMs;
    bool l3=false,grip=false;int coverClaim=0,coverSent=0,coverTaken=0;
    for(int it=0;it<20000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        g_liveControls.xrXInputHook=!r.Chance(2000);g_menuModeValue=r.Chance(1500);testVruiCaptures=r.Chance(1500);testKeypad=r.Chance(1500);
        testDeviceCam=r.Chance(1500);g_bdActive=r.Chance(1500) ? 1:0;g_sceneTier=r.Chance(400) ? int(r.Next()%4):int(r.Next()%2);
        testForegroundInProcess=!r.Chance(3);
        if(r.Chance(l3 ? (grip ? 120:40):20))l3=!l3;
        if(r.Chance(grip ? (l3 ? 120:40):20))grip=!grip;
        float g=grip ? (r.Chance(8) ? r.Range(.4f,.7f):r.Range(.7f,1)):(r.Chance(8) ? r.Range(.4f,.7f):r.Range(0,.4f));
        if(r.Chance(1000))g=std::numeric_limits<float>::quiet_NaN();
        const bool r3=r.Chance(800),active=!r.Chance(800);
        const bool claimed=cvr::input::UpdateCyberwareChord(l3,g,r3,active);h.B(claimed);coverClaim+=claimed;
        if(r.Chance(2)) {
            testInputCalls.clear();cvr::input::DispatchCyberwareChord();coverSent+=!testInputCalls.empty();
            h.I(int64_t(testInputCalls.size()));for(const float v:testInputCalls)h.F(v);testInputCalls.clear();
        }
        if(r.Chance(4)) { const bool taken=cvr::input::TakeCyberwareChord();h.B(taken);coverTaken+=taken; }
        h.I(int64_t(CyberpunkVR_CyberwareChordSent.load()));
        g_roomMs+=r.Chance(80) ? 260+r.Next()%300:8+r.Next()%40;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"chord: claimed %d sent %d taken %d\n",coverClaim,coverSent,coverTaken);
    g_VRIKClockMs=nullptr;CyberpunkVR_RuntimeDiagnostics=1;g_menuModeValue=0;testVruiCaptures=false;testKeypad=false;testDeviceCam=false;
    g_bdActive=0;g_sceneTier=1;testForegroundInProcess=true;
}
// THE POSE TRACE: the pose identity (src/Camera/PoseIdentity.cpp, or its Rust port over cvr-camera's ledger) --
// setups published from head samples into a field of pose buffers whose bytes change under them, captured, copied by
// the "engine" and transferred, re-serialized, blended, read back per view and origin, invalidated, the thread's node
// identity, and a field larger than the ledger flooded now and then so it fills and evicts. Unreadable and low
// addresses come through the SEH reads.
void TracePoseHash(TraceHash& h,const cvr::camera::PoseIdentity& p,uintptr_t base) {
    h.I(int64_t(p.poseId));h.I(int64_t(p.writeId));h.I(int64_t(p.component ? p.component-base:0));h.I(p.view);
    h.F(p.head.posX);h.F(p.head.posY);h.F(p.head.posZ);h.F(p.head.oriW);h.B(p.head.valid);h.I(int64_t(p.head.originSerial));
    h.F(p.localHead.orientation.w);h.F(p.localHead.position.x);h.F(p.localHead.position.z);
    h.I(p.renderFrameId);h.B(p.hasRenderFrameId);h.B(p.externalView);
}
void TraceReceiptHash(TraceHash& h,const cvr::camera::PoseReceipt& r,uintptr_t base) {
    h.I(int64_t(r.source ? r.source-base:0));h.I(int64_t(r.generation));
    for(const auto v:r.fingerprint.position)h.I(v);for(const auto v:r.fingerprint.rotation)h.I(v);
    TracePoseHash(h,r.payload,base);
}
void TracePose(TraceHash& h,uint64_t seed) {
    using namespace cvr::camera;
    TraceRng r(seed);
    constexpr size_t Slots=9000,Stride=32,Hot=64;
    static std::vector<uint8_t> field(Slots*Stride+64);
    const uintptr_t base=(reinterpret_cast<uintptr_t>(field.data())+15)&~uintptr_t(15);
    const auto slot=[&](size_t k) { return base+k*Stride; };
    static void* guard=VirtualAlloc(nullptr,4096,MEM_RESERVE|MEM_COMMIT,PAGE_NOACCESS);
    const auto inField=[&](uintptr_t a) { return a>=base && a<base+Slots*Stride; };
    const auto pick=[&]()->uintptr_t {
        if(r.Chance(150))return r.Chance(2) ? uintptr_t(0x1234):reinterpret_cast<uintptr_t>(guard);
        return slot(r.Chance(10) ? r.Next()%Slots:r.Next()%Hot);
    };
    const auto writeBytes=[&](uintptr_t a) {
        int32_t p[3];float q[4];for(auto& v:p)v=int32_t(r.Next()%2000)-1000;for(auto& v:q)v=r.Range(-1,1);
        std::memcpy(reinterpret_cast<void*>(a),p,12);std::memcpy(reinterpret_cast<void*>(a+16),q,16);
    };
    for(size_t k=0;k<Slots;++k)writeBytes(slot(k));
    std::vector<PoseReceipt> saved;size_t flood=0;
    int coverPublished=0,coverCopied=0,coverBlended=0,coverRead=0;
    for(int it=0;it<60000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        testResolveRenderedPose=!r.Chance(20);testExternalPoseReset=r.Chance(30);if(r.Chance(700))++testOriginSerial;
        if(r.Chance(4))writeBytes(slot(r.Next()%Hot));
        OpenXRHeadPose head{};head.posX=r.Range(-1,1);head.posY=r.Range(1,2);head.posZ=r.Range(-1,1);head.oriW=1;
        head.valid=!r.Chance(10);head.originSerial=r.Chance(10) ? testOriginSerial+1:testOriginSerial;
        if(it%1000==999) {   // a flood of distinct objects: more than the ledger holds over the run
            for(int k=0;k<400;++k) { const auto a=slot(Hot+(flood++)%(Slots-Hot));PublishCameraSetupPose(a,a-0xE0,1,NextPoseIdentity(),head); }
        }
        const auto saveReceipt=[&](const PoseReceipt& rc) { if(!rc)return;if(saved.size()<64)saved.push_back(rc);else saved[r.Next()%saved.size()]=rc; };
        const auto savedOne=[&]()->PoseReceipt { return saved.empty() || r.Chance(15) ? PoseReceipt{}:saved[r.Next()%saved.size()]; };
        switch(r.Next()%10) {
        case 0: case 1: {
            const auto a=pick();const uint32_t view=r.Chance(10) ? uint32_t(r.Next()%4):1+uint32_t(r.Next()%2);
            const uint64_t id=r.Chance(15) ? 0:NextPoseIdentity();const bool external=r.Chance(5);
            PublishCameraSetupPose(a,a-0xE0,view,id,head,external);
            const auto rc=CapturePoseAddress(a);coverPublished+=bool(rc);TraceReceiptHash(h,rc,base);saveReceipt(rc);break;
        }
        case 2: case 3: { const auto rc=CapturePoseAddress(pick());TraceReceiptHash(h,rc,base);saveReceipt(rc);break; }
        case 4: case 5: {
            const auto source=savedOne();const auto destination=pick();
            if(source && inField(source.source) && inField(destination) && !r.Chance(4))
                std::memcpy(reinterpret_cast<void*>(destination),reinterpret_cast<const void*>(source.source),Stride);
            if(r.Next()%2) { const bool ok=TransferPoseAddress(source,destination);h.B(ok);coverCopied+=ok; }
            else PublishSerializedPose(destination,source);
            const auto rc=CapturePoseAddress(destination);TraceReceiptHash(h,rc,base);saveReceipt(rc);break;
        }
        case 6: {
            WeightedPoseReceipt inputs[3]{};const size_t n=1+r.Next()%3;
            const float w0=r.Unit();
            for(size_t k=0;k<n;++k)inputs[k].source=savedOne();
            if(n==1)inputs[0].weight=r.Chance(5) ? 0.f:1.f;
            else if(n==2) { inputs[0].weight=w0;inputs[1].weight=1-w0; }
            else { inputs[0].weight=w0*.5f;inputs[1].weight=.5f;inputs[2].weight=.5f-w0*.5f; }
            if(r.Chance(10))inputs[r.Next()%n].weight=r.Chance(2) ? -.1f:std::numeric_limits<float>::quiet_NaN();
            if(r.Chance(10))inputs[r.Next()%n].weight=0;
            if(n>1 && r.Chance(3))inputs[1].source=inputs[0].source;   // the same sample through two copies
            const auto destination=pick();
            const bool ok=PublishBlendedPose(destination,std::span<const WeightedPoseReceipt>(inputs,n));h.B(ok);coverBlended+=ok;
            TraceReceiptHash(h,CapturePoseAddress(destination),base);break;
        }
        case 7: {
            PoseIdentity out{};out.poseId=77;const bool ok=ReadCameraPoseIdentity(pick(),uint32_t(r.Next()%4),&out);h.B(ok);coverRead+=ok;
            TracePoseHash(h,out,base);break;
        }
        case 8: InvalidatePoseAddress(pick());break;
        case 9:
            if(r.Chance(2)) { PoseIdentity p{};p.poseId=r.Next();p.writeId=r.Next();p.view=1+uint32_t(r.Next()%2);p.head=head;
                TracePoseHash(h,SetNodePoseIdentity(p),base); }
            else TracePoseHash(h,CurrentNodePoseIdentity(),base);
            break;
        }
        for(const auto& c:CyberpunkVR_PoseIdComponent)h.I(int64_t(c.load()));
        h.I(int64_t(CyberpunkVR_PoseIdCopied.load()));h.I(int64_t(CyberpunkVR_PoseIdCopyChanged.load()));
    }
    // What the full ledger kept: every object of the field.
    int kept=0;
    for(size_t k=0;k<Slots;++k) { const auto rc=CapturePoseAddress(slot(k));kept+=bool(rc);TraceReceiptHash(h,rc,base); }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"pose: published %d copied %d blended %d read %d kept %d of %zu\n",
        coverPublished,coverCopied,coverBlended,coverRead,kept,Slots);
    CyberpunkVR_RuntimeDiagnostics=1;testResolveRenderedPose=true;testExternalPoseReset=false;
}// THE CAMERA HOOKS TRACE: FinalCamera's read-back and mono write, CameraPitch's clamp and WorldSpaceBlendCamera's copy
// (src/Hooks/FinalCamera.cpp, CameraPitch.cpp and WorldSpaceBlendCamera.cpp, or their Rust port) over fake cameras --
// labels published to find through the pose identity, the placed-pose ring and the write records, every view flag and
// switch, the barrel packet, the rendered-frame queues; pitch states in radians and degrees; blend components whose
// source, owners, origin and gates come and go.
void TraceCamHooks(TraceHash& h,uint64_t seed) {
    using namespace cvr::camera;
    TraceRng r(seed);auto& f=g_room;
    RoomLink(r);const uintptr_t player=reinterpret_cast<uintptr_t>(f.player);cvr::roomscale::SetPlayer(player);
    alignas(16) static uint8_t cams[4][0x400],pitches[2][0x40],comps[3][0x240],sources[2][0x240];
    std::vector<std::array<float,4>> quats;
    const auto quat=[&]()->std::array<float,4> {
        if(quats.empty() || r.Chance(4)) { std::array<float,4> q;r.Quat(q.data(),3.1f,1.2f,.6f);quats.push_back(q);if(quats.size()>16)quats.erase(quats.begin()); }
        return quats[r.Next()%quats.size()];
    };
    int coverMatch=0,coverPushed=0,coverWritten=0,coverBlend=0;const bool dump=std::getenv("CVR_ROOM_DUMP")!=nullptr;
    auto mark=[&](int it,const char* what) { if(dump)std::fprintf(stderr,"%d %s %016llx %llu\n",it,what,static_cast<unsigned long long>(h.h),static_cast<unsigned long long>(h.n)); };
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;if(r.Chance(800))++testOriginSerial;
        CyberpunkVR_StereoModuleLoaded=!r.Chance(10);testMainView=r.Chance(2);testVrcamView=r.Chance(3);
        CyberpunkVR_PoseReadBack=!r.Chance(10);CyberpunkVR_CamWriteInFinal=r.Chance(3);CyberpunkVR_CamFinalViewScope=!r.Chance(4);
        g_headQuatValid=r.Chance(8) ? 0:1;g_lastLocateSeq=r.Chance(5) ? 0:uint32_t(it);
        CyberpunkVR_OneSamplePerFrame=!r.Chance(6);CyberpunkVR_CamWriteInPatch=!r.Chance(10);
        { const float x=r.Range(-1,1),y=r.Range(-1,1),z=r.Range(-1,1);testShotShared[24]=x;testShotShared[25]=y;testShotShared[26]=z; }
        if(r.Chance(5)) { testShotShared[24]=0;testShotShared[25]=1;testShotShared[26]=0; }
        testShotShared[27]=r.Chance(2) ? 1.f:0.f;
        // FinalCamera
        auto* cam=cams[r.Next()%4];float* rsi=reinterpret_cast<float*>(cam);
        const auto q=quat();int32_t pos[3];for(auto& c:pos)c=int32_t(r.Next()%2000000)-1000000;
        std::memcpy(cam,pos,12);
        if(r.Chance(10)) { float bad[4]={2,0,0,0};std::memcpy(cam+16,bad,16); } else std::memcpy(cam+16,q.data(),16);
        for(int k=8;k<40;++k)rsi[k]=r.Range(-2,2);
        OpenXRHeadPose p{};TraceCameraPose(r,p);
        const uint32_t view=1+uint32_t(r.Next()%2);
        if(r.Chance(2))PublishCameraSetupPose(reinterpret_cast<uintptr_t>(rsi),reinterpret_cast<uintptr_t>(rsi)-0x70,view,NextPoseIdentity(),p);
        else if(r.Chance(3))InvalidatePoseAddress(reinterpret_cast<uintptr_t>(rsi));
        if(r.Chance(3))CamWriteRecordPush(q.data(),p);
        if(r.Chance(3))PlacedCameraPosePush(view,q.data(),pos,p);
        if(r.Chance(3)) { const auto hq=r.Chance(4) ? quat():q;CamWriteQuatPublish(hq[0],hq[1],hq[2],hq[3],p,nullptr,0); }
        testRenderedPoses.clear();
        OnFinalCameraCallback(r.Chance(60) ? (r.Chance(2) ? nullptr:reinterpret_cast<float*>(uintptr_t(0x100))):rsi);
        for(int k=0;k<40;++k)h.I(reinterpret_cast<const int32_t*>(cam)[k]);mark(it,"cam");
        h.I(int64_t(g_finalCameraHits));h.I(g_renderedSeq);h.I(testTelemetryHits);{ const auto base=reinterpret_cast<uintptr_t>(cams);const auto rsiAt=testTelemetryRsi;
          h.I(int64_t(rsiAt>=base && rsiAt<base+sizeof(cams) ? rsiAt-base:rsiAt)); }
        h.I(int64_t(testRenderedPoses.size()));for(const auto& [pose,vrcam]:testRenderedPoses) { TraceHashPose(h,pose);h.B(vrcam);coverMatch+=pose.valid; }
        coverPushed+=!testRenderedPoses.empty();mark(it,"pushed");
        { BarrelFrame bf{};h.B(BarrelFrameRead(&bf));h.Fs(&bf.camQuat[0],7); }mark(it,"barrel");
        h.I(CyberpunkVR_DebugFinalAge);h.I(CyberpunkVR_DebugFinalTies);h.I(int64_t(CyberpunkVR_DebugFinalTieHits));h.I(int64_t(CyberpunkVR_DebugFinalMatch));
        h.I(int64_t(CyberpunkVR_DebugFinalNoMatch));h.I(CyberpunkVR_DebugVrcamFinalAge);h.I(int64_t(CyberpunkVR_DebugVrcamFinalMatch));
        h.I(int64_t(CyberpunkVR_DebugVrcamFinalNoMatch));h.I(int64_t(CyberpunkVR_DebugViewCamMain));h.I(int64_t(CyberpunkVR_DebugViewCamVrcam));
        h.I(int64_t(CyberpunkVR_DebugViewCamOther));for(int k=0;k<2;++k) { h.I(int64_t(CyberpunkVR_PoseIdFinal[k].load()));h.I(int64_t(CyberpunkVR_PoseIdFinalMiss[k].load())); }
        mark(it,"counters");// CameraPitch
        { auto* state=pitches[r.Next()%2];const bool degrees=r.Chance(3);
          const float lo=degrees ? r.Range(-89,-10):r.Range(-1.5f,-.2f),hi=degrees ? r.Range(10,89):r.Range(.2f,1.5f);
          std::memcpy(state+0x14,&lo,4);std::memcpy(state+0x18,&hi,4);
          g_liveControls.xrDisableMouseY=r.Chance(2);const float pitch=degrees ? r.Range(-120,120):r.Range(-2,2);
          OnPitchHookCallback(r.Chance(40) ? reinterpret_cast<void*>(uintptr_t(0x100)):state,pitch);h.F(g_gamePitchRadians); }mark(it,"pitch");
        // WorldSpaceBlendCamera
        { auto* comp=comps[r.Next()%3];auto* src=sources[r.Next()%2];const auto c=reinterpret_cast<uintptr_t>(comp),s=reinterpret_cast<uintptr_t>(src);
          RoomPut<uintptr_t>(comp,0x1E0,r.Chance(10) ? 0:(r.Chance(8) ? reinterpret_cast<uintptr_t>(sources[0]):s));
          RoomPut<uintptr_t>(comp,0x50,r.Chance(10) ? player+8:player);RoomPut<uintptr_t>(src,0x50,r.Chance(10) ? player+8:player);
          testCamObjMain=r.Chance(10) ? reinterpret_cast<uintptr_t>(sources[1]):s;
          if(r.Chance(2)) { int32_t sp[3];float sq[4];for(auto& v:sp)v=int32_t(r.Next()%100000);r.Quat(sq,3.1f,1.2f,.6f);
                            std::memcpy(src+0xE0,sp,12);std::memcpy(src+0xF0,sq,16); }
          if(r.Chance(2)) { OpenXRHeadPose sp{};TraceCameraPose(r,sp);sp.originSerial=r.Chance(10) ? testOriginSerial+1:testOriginSerial;
                            PublishCameraSetupPose(s+0xE0,r.Chance(10) ? c:s,r.Chance(10) ? 2:1,NextPoseIdentity(),sp); }
          g_menuModeValue=r.Chance(30);testDeviceCam=r.Chance(30);g_bdActive=r.Chance(30) ? 1:0;testExternalPoseReset=r.Chance(30);
          CvrWorldBlendBeforeNotify(r.Chance(60) ? uintptr_t(0x100):c);
          g_menuModeValue=0;testDeviceCam=false;g_bdActive=0;testExternalPoseReset=false;
          for(int k=0;k<8;++k)h.I(RoomGet<int32_t>(comp,0xE0+4*k));mark(it,"blend-bytes");
          const auto rc=CapturePoseAddress(c+0xE0);h.B(bool(rc));coverBlend+=bool(rc);if(rc) { h.I(int64_t(rc.payload.poseId));h.I(int64_t(rc.payload.writeId)); }
          h.I(int64_t(CyberpunkVR_WorldBlendPoseCalls.load()));h.I(int64_t(CyberpunkVR_WorldBlendPoseApplied.load()));
          h.I(int64_t(CyberpunkVR_WorldBlendPoseMissing.load()));h.I(int64_t(CyberpunkVR_WorldBlendPoseChanged.load())); }
        coverWritten+=CyberpunkVR_CamWriteInFinal!=0 && g_headQuatValid;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"camhooks: matched %d pushed %d written %d blended %d main %llu vrcam %llu other %llu\n",
        coverMatch,coverPushed,coverWritten,coverBlend,static_cast<unsigned long long>(CyberpunkVR_DebugViewCamMain),
        static_cast<unsigned long long>(CyberpunkVR_DebugViewCamVrcam),static_cast<unsigned long long>(CyberpunkVR_DebugViewCamOther));
    CyberpunkVR_RuntimeDiagnostics=1;CyberpunkVR_StereoModuleLoaded=0;testMainView=testVrcamView=0;CyberpunkVR_PoseReadBack=1;
    CyberpunkVR_CamWriteInFinal=0;CyberpunkVR_CamFinalViewScope=1;CyberpunkVR_OneSamplePerFrame=1;CyberpunkVR_CamWriteInPatch=1;
    g_liveControls.xrDisableMouseY=0;
}// THE POSE TRANSPORT TRACE: CameraPoseTransport (src/Hooks/CameraPoseTransport.cpp, or its Rust port) over a pool of
// fake components, directors, blend arrays and descriptors, with the engine's functions stood in for -- the blend
// running the generic serializer and its own finalize inside it, the RTT build its descriptor builder, as the game
// nests them; the external view composed onto detached cameras, leased to the second eye and read back by the aim and
// steering readers; the setup copy's two halves; every gate, both eyes, the diagnostics records.
struct TransportPool { alignas(16) uint8_t bytes[0x40000]; };
TransportPool* g_tp{};uintptr_t g_tpBase{},g_tpExe{};TraceRng* g_tpRng{};int g_tpDepth{};
uintptr_t TpRel(uintptr_t p) { return p>=g_tpBase && p<g_tpBase+sizeof(TransportPool) ? p-g_tpBase+1:(p ? 0xDEAD:0); }
uintptr_t TpAt(size_t offset) { return g_tpBase+offset; }
bool TpIn(uintptr_t p) { return p>=g_tpBase && p+0x40<=g_tpBase+sizeof(TransportPool); }
// The pool: 8 components at 0x1000 (0x400 each), 4 directors at 0x4000 (0x800), 4 blend arrays at 0x6000 (0x100) with
// their entries at 0x8000 (8 per array, 0xA0), 16 destinations at 0x10000 (0x100), 8 descriptors at 0x12000 (0x100),
// the RTT contexts at 0x20000 (0x3000), the vtable at 0x3F000.
uintptr_t TpComponent(int k) { return TpAt(0x1000+size_t(k)*0x400); }
uintptr_t TpDirector(int k) { return TpAt(0x4000+size_t(k)*0x800); }
uintptr_t TpArray(int k) { return TpAt(0x6000+size_t(k)*0x100); }
uintptr_t TpDestination(int k) { return TpAt(0x10000+size_t(k)*0x100); }
uintptr_t TpDescriptor(int k) { return TpAt(0x12000+size_t(k)*0x100); }
void TpWritePose(uintptr_t at,TraceRng& r) {
    int32_t p[3];for(auto& v:p)v=int32_t(r.Next()%2000000)-1000000;float q[4];r.Quat(q,3.1f,1.2f,.6f);
    if(r.Chance(15))q[0]=3;
    std::memcpy(reinterpret_cast<void*>(at),p,12);std::memcpy(reinterpret_cast<void*>(at+16),q,16);
    const float fov=r.Chance(10) ? 0.f:r.Range(40,110);std::memcpy(reinterpret_cast<void*>(at+0x20),&fov,4);
}
void TpCopy(uintptr_t to,uintptr_t from) { if(TpIn(to) && TpIn(from))std::memmove(reinterpret_cast<void*>(to),reinterpret_cast<const void*>(from),0x30); }
void __fastcall TpNotify(uintptr_t component,uintptr_t transform) { testTransportCalls.insert(testTransportCalls.end(),{205,component,transform}); }
uintptr_t __fastcall TpFakeBlend(uintptr_t director) {
    auto& r=*g_tpRng;testTransportCalls.insert(testTransportCalls.end(),{301,director});
    if(g_tpDepth++<1) {
        for(int k=int(r.Next()%3);k>0;--k) { const auto c=TpComponent(int(r.Next()%8));CvrPtGenericSerialize(c+0x120,r.Chance(2) ? TpDestination(int(r.Next()%16)):director+0x500); }
        if(r.Chance(3))CvrPtBlend(TpDirector(int(r.Next()%4)));
        CvrPtBlendFinalizeAt(TpArray(int(r.Next()%4)),r.Chance(8) ? g_tpExe+0x1234:g_tpExe+0x5FBE2B);
    }
    --g_tpDepth;return director^1;
}
void __fastcall TpFakeBlendFinalize(uintptr_t array) { testTransportCalls.insert(testTransportCalls.end(),{302,array}); }
void __fastcall TpFakeSerialize(uintptr_t iface,uintptr_t destination) {
    auto& r=*g_tpRng;testTransportCalls.insert(testTransportCalls.end(),{303,iface,destination});
    if(!r.Chance(5))TpCopy(destination,iface-0x120+0xE0);
    if(r.Chance(6)) { OpenXRHeadPose p{};TraceCameraPose(r,p);cvr::camera::PublishCameraSetupPose(destination,iface-0x120,1,cvr::camera::NextPoseIdentity(),p); }
}
void __fastcall TpFakeGenericSerialize(uintptr_t iface,uintptr_t destination) {
    auto& r=*g_tpRng;testTransportCalls.insert(testTransportCalls.end(),{304,iface,destination});
    if(TpIn(destination))TpWritePose(destination,r);
}
uintptr_t __fastcall TpFakeMainRead(uintptr_t director,uintptr_t destination,uintptr_t setup) {
    auto& r=*g_tpRng;testTransportCalls.insert(testTransportCalls.end(),{305,director,destination,setup});
    if(!r.Chance(6))TpCopy(destination,setup);return destination+7;
}
uintptr_t __fastcall TpFakeDescriptor(uintptr_t destination,uintptr_t descriptor) {
    auto& r=*g_tpRng;testTransportCalls.push_back(306);
    uint8_t seen[0x40];std::memcpy(seen,reinterpret_cast<const void*>(descriptor),sizeof(seen));
    for(size_t k=0;k<sizeof(seen);k+=4) { uint32_t v;std::memcpy(&v,seen+k,4);testTransportCalls.push_back(v); }
    if(TpIn(destination) && !r.Chance(6)) { std::memcpy(reinterpret_cast<void*>(destination),seen+16,12);std::memcpy(reinterpret_cast<void*>(destination+16),seen,16); }
    return destination;
}
double __fastcall TpFakeRtt(uintptr_t component,uintptr_t destination,char force) {
    auto& r=*g_tpRng;testTransportCalls.insert(testTransportCalls.end(),{307,component,destination,uintptr_t(force)});
    const auto descriptor=TpDescriptor(int(r.Next()%8));if(r.Chance(2))TpCopy(descriptor,component+0xE0);
    if(r.Chance(4) && TpIn(component+0xE0)) { uint8_t q[16],p[12];std::memcpy(p,reinterpret_cast<const void*>(component+0xE0),12);std::memcpy(q,reinterpret_cast<const void*>(component+0xF0),16);
                      std::memcpy(reinterpret_cast<void*>(descriptor),q,16);std::memcpy(reinterpret_cast<void*>(descriptor+16),p,12); }
    CvrPtDescriptorBuildAt(TpDestination(int(r.Next()%16)),descriptor,r.Chance(6) ? g_tpExe+0x99:g_tpExe+0xD29C8C);
    return double(force)+.5;
}
void __fastcall TpFakeRttRefresh(uintptr_t component) { testTransportCalls.insert(testTransportCalls.end(),{308,component}); }
uintptr_t __fastcall TpFakeClone(uintptr_t destination,uintptr_t source) { testTransportCalls.push_back(309);TpCopy(destination,source);return destination; }
uintptr_t __fastcall TpFakeRender(uintptr_t destination,uintptr_t descriptor,uintptr_t clip) {
    testTransportCalls.push_back(310);if(TpIn(destination) && TpIn(descriptor)) { std::memcpy(reinterpret_cast<void*>(destination),reinterpret_cast<const void*>(descriptor+16),12);
        std::memcpy(reinterpret_cast<void*>(destination+16),reinterpret_cast<const void*>(descriptor),16); }
    return clip;
}
void TpHashReceipt(TraceHash& h,uintptr_t address) {
    const auto rc=cvr::camera::CapturePoseAddress(address);h.B(bool(rc));
    if(rc) { h.I(int64_t(rc.payload.poseId));h.I(int64_t(rc.payload.writeId));h.I(int64_t(TpRel(rc.payload.component)));h.I(rc.payload.view);h.B(rc.payload.externalView);
             TraceHashPose(h,rc.payload.head); }
}
void TracePoseTransport(TraceHash& h,uint64_t seed) {
    using namespace cvr::camera;
    TraceRng r(seed);g_tpRng=&r;
    static TransportPool pool{};g_tp=&pool;std::memset(&pool,0,sizeof(pool));g_tpBase=reinterpret_cast<uintptr_t>(&pool);g_tpExe=0x7FF612340000ull;
    const uintptr_t originals[10]={reinterpret_cast<uintptr_t>(&TpFakeBlend),reinterpret_cast<uintptr_t>(&TpFakeBlendFinalize),
        reinterpret_cast<uintptr_t>(&TpFakeSerialize),reinterpret_cast<uintptr_t>(&TpFakeGenericSerialize),reinterpret_cast<uintptr_t>(&TpFakeMainRead),
        reinterpret_cast<uintptr_t>(&TpFakeRtt),reinterpret_cast<uintptr_t>(&TpFakeRttRefresh),reinterpret_cast<uintptr_t>(&TpFakeDescriptor),
        reinterpret_cast<uintptr_t>(&TpFakeClone),reinterpret_cast<uintptr_t>(&TpFakeRender)};
    CvrPoseTransportBind(g_tpExe,originals);
    // the vtable: +0x240 the notification
    const auto vtable=TpAt(0x3F000);*reinterpret_cast<uintptr_t*>(vtable+0x240)=reinterpret_cast<uintptr_t>(&TpNotify);
    for(int k=0;k<8;++k) { *reinterpret_cast<uintptr_t*>(TpComponent(k))=vtable;TpWritePose(TpComponent(k)+0xE0,r); }
    for(int k=0;k<4;++k) {
        const auto d=TpDirector(k);const auto table=TpAt(0x7000+size_t(k)*0x100);
        *reinterpret_cast<uintptr_t*>(d+0x48)=table;*reinterpret_cast<uint8_t*>(d+0x54)=uint8_t(1+k);TpWritePose(d+0x500,r);
        const auto a=TpArray(k);*reinterpret_cast<uintptr_t*>(a)=TpAt(0x8000+size_t(k)*0x500);
    }
    g_VRIKClockQpc=RoomQpc;
    int coverBlend=0,coverExternal=0,coverAim=0,coverSteer=0,coverNotify=0;
    for(int it=0;it<40000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;if(r.Chance(700))++testOriginSerial;
        testSessionRunning=!r.Chance(20);g_menuModeValue=r.Chance(30);testLobby=r.Chance(3);testDeviceCam=r.Chance(40);
        g_bdActive=r.Chance(20) ? 1:0;testBdScenePose=r.Chance(2);CyberpunkVR_BdEditorAlign=r.Chance(3);testExternalPoseReset=r.Chance(40);
        CyberpunkVR_IpdInWorldPos=!r.Chance(4);CyberpunkVR_MainIsRightEye=r.Chance(2);testNormalFov=r.Chance(10) ? 200.f:r.Range(60,100);
        testVrcamLive=r.Chance(5) ? int(1+r.Next()%2):0;testCameraPoseOk=!r.Chance(5);TraceCameraPose(r,testCameraPoseFrame);
        testCameraPoseFrame.originSerial=r.Chance(8) ? testOriginSerial+1:testOriginSerial;testCameraPoseSequence=r.Chance(3) ? 0:r.Next();
        testVrcamComp=r.Chance(6) ? 0:TpComponent(int(r.Next()%2));testCamObjMain=TpComponent(2+int(r.Next()%2));
        // the director tables: the main camera blended in or not, weights
        for(int k=0;k<4;++k) { const auto table=*reinterpret_cast<uintptr_t*>(TpDirector(k)+0x48);
            for(int e=0;e<4;++e) { *reinterpret_cast<uintptr_t*>(table+e*0x20)=r.Chance(3) ? testCamObjMain:TpComponent(int(r.Next()%8));
                                   const float w=r.Chance(5) ? 0.f:r.Unit();std::memcpy(reinterpret_cast<void*>(table+e*0x20+0x10),&w,4); } }
        // the blend arrays: entries labelled from components, weights summing to one or not
        for(int k=0;k<4;++k) {
            const auto a=TpArray(k);const uint32_t n=r.Chance(10) ? (r.Chance(2) ? 0u:70u):1+uint32_t(r.Next()%3);
            std::memcpy(reinterpret_cast<void*>(a+12),&n,4);const auto entries=*reinterpret_cast<uintptr_t*>(a);
            const float w0=r.Unit();
            for(uint32_t e=0;e<3;++e) {
                const auto entry=entries+e*0xA0;
                if(r.Chance(2)) { const auto c=TpComponent(int(r.Next()%8));TpCopy(entry,c+0xE0);if(r.Chance(2))TransferPoseAddress(CapturePoseAddress(c+0xE0),entry); }
                const float w=n<=1 ? 1.f:(n==2 ? (e==0 ? w0:1-w0):(e==0 ? w0*.5f:(e==1 ? .5f:.5f-w0*.5f)));std::memcpy(reinterpret_cast<void*>(entry+0x90),&w,4);
            }
        }
        // labels on the components and destinations
        for(int k=int(r.Next()%3);k>0;--k) { const auto c=TpComponent(int(r.Next()%8));if(r.Chance(3))TpWritePose(c+0xE0,r);
            OpenXRHeadPose p{};TraceCameraPose(r,p);p.originSerial=r.Chance(10) ? testOriginSerial+1:testOriginSerial;
            PublishCameraSetupPose(c+0xE0,c,r.Chance(8) ? 2:1,NextPoseIdentity(),p,r.Chance(3)); }
        testTransportCalls.clear();
        const int op=int(r.Next()%11);uintptr_t out=0;
        switch(op) {
        case 0: case 1: out=CvrPtBlend(TpDirector(int(r.Next()%4)));break;
        case 2: { const auto c=TpComponent(int(r.Next()%8));CvrPtSerialize(c+0x120,TpDestination(int(r.Next()%16)));break; }
        case 3: { const auto d=TpDirector(int(r.Next()%4));out=CvrPtMainRead(d,TpDestination(int(r.Next()%16)),r.Chance(2) ? d+0x500:TpComponent(int(r.Next()%8))+0xE0);break; }
        case 4: { const double v=CvrPtRttBuild(r.Chance(3) ? testVrcamComp:TpComponent(int(r.Next()%8)),TpDestination(int(r.Next()%16)),char(r.Next()%2));h.F(float(v));break; }
        case 5: CvrPtRttRefresh(r.Chance(2) ? testVrcamComp:TpComponent(int(r.Next()%8)));break;
        case 6: { const auto desc=TpDescriptor(int(r.Next()%8));if(r.Chance(2))TpWritePose(desc,r);
                  out=CvrPtDescriptorBuildAt(TpDestination(int(r.Next()%16)),desc,r.Chance(2) ? g_tpExe+0xD29C8C:g_tpExe+1);break; }
        case 7: out=CvrPtCameraClone(TpDestination(int(r.Next()%16)),r.Chance(2) ? TpComponent(int(r.Next()%8))+0xE0:TpDestination(int(r.Next()%16)));break;
        case 8: { const auto ctx=TpAt(0x20000+size_t(r.Next()%2)*0x3000);const auto c=TpComponent(int(r.Next()%8));TpCopy(ctx+0x2C50,c+0xE0);
                  TransferPoseAddress(CapturePoseAddress(c+0xE0),ctx+0x2C50);const auto desc=TpDescriptor(int(r.Next()%8));
                  if(r.Chance(2)) { std::memcpy(reinterpret_cast<void*>(desc),reinterpret_cast<const void*>(ctx+0x2C50+16),16);std::memcpy(reinterpret_cast<void*>(desc+16),reinterpret_cast<const void*>(ctx+0x2C50),12); }
                  const uintptr_t ret=r.Chance(3) ? g_tpExe+0x4248FB:(r.Chance(2) ? g_tpExe+0x424956:g_tpExe+0x42);
                  out=CvrPtRenderBuildAt(TpDestination(int(r.Next()%16)),desc,r.Chance(10) ? 0x100:ctx+0x2ED0,ret);break; }
        case 9: { alignas(16) uint8_t storage[0x200]{};const auto src=r.Chance(2) ? TpComponent(int(r.Next()%8))+0xE0:TpDestination(int(r.Next()%16));
                  const auto dst=TpDestination(int(r.Next()%16));CvrPoseSetupCopyBefore(storage,src);if(!r.Chance(5))TpCopy(dst,src);
                  CvrPoseSetupCopyAfter(reinterpret_cast<PoseReceipt*>(storage),dst);if(r.Chance(4))CvrPoseSetupDefaultBefore(TpDestination(int(r.Next()%16)));break; }
        case 10: {
            MainAimPose aim{};const bool a=ReadMainAimPose(aim);h.B(a);coverAim+=a;if(a) { h.Fs(aim.position,3);h.Fs(aim.rotation,4);h.I(int64_t(aim.poseId)); }
            float dir[3]{9,9,9};uint64_t id=7;const bool s=ReadExternalMainDirection(dir,r.Chance(2) ? &id:nullptr);h.B(s);coverSteer+=s;h.Fs(dir,3);h.I(int64_t(id));
            h.I(int64_t(TpRel(CurrentSerializedCameraComponent())));break;
        }
        }
        h.I(int64_t(TpRel(out)));
        h.I(int64_t(testTransportCalls.size()));for(const auto v:testTransportCalls)h.I(int64_t(TpRel(v) ? TpRel(v):(v>=g_tpExe && v<g_tpExe+0x1000000 ? v-g_tpExe:v)));
        for(const auto v:testTransportCalls)coverNotify+=v==205;
        for(int k=0;k<16;++k) { if(r.Chance(4))TpHashReceipt(h,TpDestination(k)); }
        for(int k=0;k<4;++k)if(r.Chance(4))TpHashReceipt(h,TpDirector(k)+0x500);
        { const auto& b=CyberpunkVR_PoseBlendDebug;h.I(int64_t(b.calls));h.I(int64_t(b.labelled));h.I(int64_t(b.rejected));h.I(int64_t(TpRel(b.director)));
          h.I(b.count);h.I(b.accepted);h.Fs(b.weights,8);for(int k=0;k<8;++k) { h.I(int64_t(b.poseIds[k]));h.I(int64_t(TpRel(b.components[k])));
          for(const auto v:b.inputs[k].position)h.I(v);for(const auto v:b.inputs[k].rotation)h.I(v); }
          for(const auto v:b.output.position)h.I(v);for(const auto v:b.output.rotation)h.I(v);h.I(CyberpunkVR_PoseBlendDebugSeq.load());coverBlend=int(b.labelled); }
        for(int e=0;e<2;++e) { const auto& x=CyberpunkVR_ExternalCameraDebug[e];h.I(int64_t(x.writes));h.I(int64_t(x.stampUs));h.I(int64_t(x.poseId));
            h.I(int64_t(x.origin));h.I(int64_t(TpRel(x.component)));for(const auto v:x.position)h.I(v);h.Fs(x.rotation,4);h.Fs(x.head,7);h.F(x.fov);
            h.I(CyberpunkVR_ExternalCameraDebugSeq[e].load());coverExternal+=int(x.writes>0); }
        for(const auto& c:CyberpunkVR_PoseMainReadMissing)h.I(int64_t(c.load()));for(const auto& c:CyberpunkVR_ExternalSelectionStates)h.I(int64_t(c.load()));
        for(const auto& c:CyberpunkVR_ExternalNotify)h.I(int64_t(c.load()));
        // the bytes the external path wrote
        for(int k=0;k<16;++k)for(int w=0;w<9;++w)h.I(*reinterpret_cast<const int32_t*>(TpDestination(k)+w*4));
        g_roomUs+=r.Chance(50) ? 60000+r.Next()%100000:3000+r.Next()%20000;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"transport: blends labelled %d external %d aim %d steer %d notified %d main-read-miss %llu %llu states %llu %llu %llu %llu\n",
        coverBlend,coverExternal,coverAim,coverSteer,coverNotify,static_cast<unsigned long long>(CyberpunkVR_PoseMainReadMissing[0].load()),
        static_cast<unsigned long long>(CyberpunkVR_PoseMainReadMissing[1].load()),static_cast<unsigned long long>(CyberpunkVR_ExternalSelectionStates[0].load()),
        static_cast<unsigned long long>(CyberpunkVR_ExternalSelectionStates[1].load()),static_cast<unsigned long long>(CyberpunkVR_ExternalSelectionStates[2].load()),
        static_cast<unsigned long long>(CyberpunkVR_ExternalSelectionStates[3].load()));
    CvrPoseTransportBind(0,nullptr);g_VRIKClockQpc=nullptr;CyberpunkVR_RuntimeDiagnostics=1;g_menuModeValue=0;testLobby=false;testDeviceCam=false;g_bdActive=0;
    testExternalPoseReset=false;CyberpunkVR_BdEditorAlign=0;
}// THE PATCH CAMERA TRACE: the world-transform write of MAIN, VRCAM, the device lens and the rest (src/Hooks/
// PatchCamera.cpp, or its Rust port) over fake components and entities -- the composition once per aim epoch from the
// frame's sample, the native, body, pre-write, braindance and lens bases, the takeover and surveillance headings, the
// FPP scene comfort, the stick yaw; the position: the braindance and mode 1-3 bases, the lens copy and offset, the mount
// sweep, the rebuilt head delta, the sight approach, the eye separation with AFW, the labels, the native pair, the lens
// push, the posA/posB pair -- and every counter and published global.
struct PatchPool { alignas(16) uint8_t bytes[0x4000]; };
void TracePatchCamera(TraceHash& h,uint64_t seed) {
    using namespace cvr::camera;
    TraceRng r(seed);
    static PatchPool pool{};std::memset(&pool,0,sizeof(pool));
    const auto at=[&](size_t off) { return reinterpret_cast<uintptr_t>(pool.bytes)+off; };
    const uintptr_t comps[4]={at(0x0000),at(0x0400),at(0x0800),at(0x0C00)};   // MAIN, VRCAM, the lens, someone else's
    const uintptr_t player=at(0x1000),lensOwner=at(0x1400),playerRoot=at(0x1800),lensRoot=at(0x1C00);
    RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(player),0xB0,playerRoot);RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(lensOwner),0xB0,lensRoot);
    cvr::roomscale::SetPlayer(player);
    testCameraKinds={{comps[0],1},{comps[1],2},{comps[2],3}};
    g_VRIKClockQpc=RoomQpc;g_VRIKClockMs=RoomMs;
    const auto rel=[&](uintptr_t p) { return p>=at(0) && p<at(sizeof(pool)) ? int64_t(p-at(0)):int64_t(p ? -1:0); };
    const auto writeQuat=[&](uintptr_t a) { float q[4];r.Quat(q,3.1f,1.2f,.6f);if(r.Chance(20))q[0]=2;std::memcpy(reinterpret_cast<void*>(a),q,16); };
    const auto writePos=[&](uintptr_t a) { int32_t p[4];for(auto& v:p)v=int32_t(r.Next()%400000)-200000;std::memcpy(reinterpret_cast<void*>(a),p,16); };
    for(int k=0;k<4;++k) { writePos(comps[k]+0xE0);writeQuat(comps[k]+0xF0); }
    writePos(playerRoot+0xE0);writeQuat(playerRoot+0xF0);writePos(lensRoot+0xE0);writeQuat(lensRoot+0xF0);
    int coverComposed=0,coverWritten=0,coverIpd=0,coverDelta=0,coverLabel=0;
    for(int it=0;it<40000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;if(r.Chance(900))++testOriginSerial;
        // the switches
        if(r.Chance(40)) {
            CyberpunkVR_CamWriteInPatch=!r.Chance(10);CyberpunkVR_CamComposeAtWrite=!r.Chance(10);CyberpunkVR_OneSamplePerFrame=!r.Chance(5);
            CyberpunkVR_PoseLocateAtWrite=!r.Chance(3);CyberpunkVR_HeadingFromPreWrite=!r.Chance(5);CyberpunkVR_ViewYawFromEngine=r.Chance(3);
            CyberpunkVR_DeviceCamOrient=!r.Chance(4);CyberpunkVR_BdSceneBaseInPatch=!r.Chance(4);CyberpunkVR_CamWriteOrientInVehicle=!r.Chance(6);
            CyberpunkVR_BdEditorAlign=r.Chance(2);CyberpunkVR_BdPushTransform=int32_t(r.Next()%4);CyberpunkVR_VrcamPosFromMain=int32_t(r.Next()%4);
            CyberpunkVR_CamMountCompensate=r.Chance(2);CyberpunkVR_HeadTranslationInPatch=!r.Chance(5);CyberpunkVR_VrcamHeadTranslation=!r.Chance(5);
            CyberpunkVR_DeltaFromFreshSample=!r.Chance(5);CyberpunkVR_IpdInWorldPos=!r.Chance(5);CyberpunkVR_BdIpdInLocate=!r.Chance(4);
            CyberpunkVR_IpdInPosAB=r.Chance(4);CyberpunkVR_SceneStickYawDps=r.Chance(2) ? r.Range(30,200):0.f;
            for(auto& v:CyberpunkVR_SightApproach)v=r.Chance(4) ? r.Range(-.05f,.05f):0.f;
            CyberpunkVR_DevCamOffsetX=r.Chance(3) ? r.Range(-.2f,.2f):0.f;CyberpunkVR_DevCamOffsetY=r.Chance(3) ? r.Range(-.2f,.2f):0.f;CyberpunkVR_DevCamOffsetZ=0;
            g_liveControls.xrSceneCamComfortDeg=r.Chance(2) ? r.Range(5,40):0.f;
        }
        // the state
        g_isInVehicle=r.Chance(12);g_sceneTier=r.Chance(8) ? int(r.Next()%6):1;testDeviceCam=r.Chance(10);g_menuModeValue=r.Chance(30);testMenuRect=r.Chance(30);
        g_bdActive=r.Chance(10) ? 1:0;g_bdScenePoseValid=r.Chance(2) ? 1:0;r.Quat(g_bdSceneQuat,3.1f,1.2f,.6f);
        for(auto& v:g_bdScenePosFP)v=int32_t(r.Next()%400000)-200000;
        g_locatePosValid=!r.Chance(5);for(int k=0;k<3;++k) { g_locatePosFP[k]=int32_t(r.Next()%400000)-200000;g_locateCenterFP[k]=int32_t(r.Next()%400000)-200000; }
        g_headDeltaValid=!r.Chance(5);for(auto& v:g_headDeltaFP)v=int32_t(r.Next()%20000)-10000;
        g_headingValid=r.Chance(10) ? 0:1;{ const float y=r.Range(-3,3);g_headingSy=std::sin(y*.5f);g_headingCy=std::cos(y*.5f); }
        { const float p=r.Chance(3) ? r.Range(-.4f,.4f):0.f;g_headingPitchS=std::sin(p*.5f);g_headingPitchC=std::cos(p*.5f); }
        CyberpunkVR_EngineBodyYawValid=!r.Chance(4);{ const float y=r.Range(-3,3);CyberpunkVR_EngineBodyYawZ=std::sin(y*.5f);CyberpunkVR_EngineBodyYawW=std::cos(y*.5f); }
        CyberpunkVR_PlayerEntityValid=!r.Chance(5);for(auto& v:CyberpunkVR_PlayerEntityPos)v=r.Range(-2,2);
        CyberpunkVR_BodyYawFinalValid=!r.Chance(5);CyberpunkVR_BodyYawFinalRad=r.Range(-3,3);
        testLocateOwnsTakeover=r.Chance(6);testBdPushOwns=r.Chance(8);r.Quat(g_bdPushQuat,3.1f,1.2f,.6f);if(r.Chance(10))g_bdPushQuat[3]=3;
        testSurveillanceOk=r.Chance(3);testSurveillanceYaw=r.Range(-3,3);testFbtEyeOk=r.Chance(8);testFbtEyeZ=r.Range(1.4f,1.9f);
        testAfwActive=r.Chance(4);testAfwEye=int(r.Next()%2);if(r.Chance(10))CyberpunkVR_MainIsRightEye=r.Chance(2);
        g_lensComp=r.Chance(3) ? comps[2]:0;g_takeoverEntity=r.Chance(5) ? 0:lensOwner;g_camObjMain=r.Chance(10) ? comps[3]:comps[0];
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(comps[0]),0x50,r.Chance(10) ? lensOwner:player);RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(comps[1]),0x50,player);
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(comps[2]),0x50,lensOwner);
        { const float fov=r.Chance(5) ? 200.f:r.Range(40,110);RoomPut<float>(reinterpret_cast<uint8_t*>(comps[2]),0x128,fov); }
        g_normalFovOverrideValue=r.Chance(10) ? 0.f:r.Range(80,110);
        // the engine's write this pass, the entity, the XR samples, the epoch
        const int k=int(r.Next()%4);const auto comp=comps[k];
        if(r.Chance(2))writePos(comp+0xE0);if(r.Chance(2))writeQuat(comp+0xF0);
        if(r.Chance(4)) { float p[4];for(auto& v:p)v=r.Range(-5,5);std::memcpy(reinterpret_cast<void*>(comp+0x100),p,16);for(auto& v:p)v+=r.Range(-.5f,.5f);std::memcpy(reinterpret_cast<void*>(comp+0x110),p,16); }
        if(r.Chance(8)) { writePos(playerRoot+0xE0);writeQuat(playerRoot+0xF0); }if(r.Chance(8)) { writePos(lensRoot+0xE0);writeQuat(lensRoot+0xF0); }
        testCameraPoseOk=!r.Chance(6);TraceCameraPose(r,testCameraPoseFrame);testCameraPoseFrame.originSerial=r.Chance(10) ? testOriginSerial+1:testOriginSerial;
        if(r.Chance(3))testCameraPoseSequence=r.Next()%5000;
        if(r.Chance(3))++testAimEpoch;if(r.Chance(50))testAimEpoch=0;testAimTime=r.Chance(5) ? 0:int64_t(r.Next()%100000);
        testHeadPoseOk=!r.Chance(5);TraceCameraPose(r,testHeadPose);testHeadPose.originSerial=testOriginSerial;
        testLocatedOk=!r.Chance(4);TraceCameraPose(r,testLocatedPose);testLocatedPose.originSerial=testOriginSerial;
        testFrameSampleOk=!r.Chance(5);TraceCameraPose(r,testFrameSample);testFrameSample.originSerial=testOriginSerial;
        if(r.Chance(20)) { AnchorRecipe a{};float v[3];r.Vec(v,.3f);a.trackingOffset={v[0],v[1],v[2]};r.Vec(v,.3f);a.modelOffset={v[0],v[1],v[2]};
                           const float s=r.Range(.8f,1.3f),y0=r.Range(-3,3),y1=r.Range(-3,3),y2=r.Range(-3,3);a.scale=s;a.trackingYaw=y0;a.modelYaw=y1;a.centreYaw=y2;
                           a.bodyOrbit=r.Chance(3);AnchorRecipePublish(a); }
        if(r.Chance(40))AnchorRecipeClear();
        g_lookStickX=r.Chance(3) ? r.Range(-1,1):0.f;
        testCameraCalls.clear();
        const uint64_t composedBefore=CyberpunkVR_DebugCamComposed;
        OnPatchCameraCallback(r.Chance(80) ? nullptr:reinterpret_cast<float*>(comp+0xF0),reinterpret_cast<void*>(comp));
        coverComposed+=CyberpunkVR_DebugCamComposed!=composedBefore;
        // what it wrote and published
        for(int w=0;w<16;++w)h.I(RoomGet<int32_t>(reinterpret_cast<uint8_t*>(comp),0xE0+4*w));
        { const float f=RoomGet<float>(reinterpret_cast<uint8_t*>(comps[2]),0x128);h.F(f); }
        h.Fs(const_cast<const float*>(g_engineCamQuat),4);h.I(g_engineCamQuatValid);h.Fs(const_cast<const float*>(g_headQuatComposed),4);h.I(g_headQuatValid);
        h.F(g_viewYawUsedRad);h.I(g_viewYawUsedValid);h.I(int64_t(g_camComposedForPresent.load()));
        h.Fs(g_devCamBase,4);h.Fs(g_devCamLastWritten,4);h.Fs(g_devCamViewQuat,4);h.F(g_devCamAimYaw);h.F(g_devCamAimPitch);h.F(g_devCamFovOrig);
        h.I(g_devCamAimValid.load());h.I(g_devCamBaseValid.load());h.I(g_devCamFovSaved.load());h.I(g_devCamViewValid.load());h.I(g_devCamPosValid.load());
        for(const auto& v:g_devCamPosFP)h.I(v.load());
        for(const auto v:CyberpunkVR_EngineCamPosFP)h.I(v);h.I(CyberpunkVR_EngineCamPosValid);h.I(CyberpunkVR_MainIsRightEye);
        h.F(CyberpunkVR_DebugCamMountM);h.F(CyberpunkVR_DebugHeadingStepDeg);h.F(CyberpunkVR_DebugSceneStickYawDeg);h.F(CyberpunkVR_DebugFppSceneComfortLagDeg);
        for(const uint64_t c:{CyberpunkVR_DebugCamThreadSwitches,CyberpunkVR_DebugBdSceneBaseInPatch,CyberpunkVR_DebugPoseLocatedAtWrite,CyberpunkVR_DebugPoseFromCache,
            CyberpunkVR_DebugCamComposed,CyberpunkVR_DebugCamVrcamFirst,CyberpunkVR_DebugCamNoHmd,CyberpunkVR_DebugBdEditorAlign,CyberpunkVR_DebugVrcamPosFromMain,
            CyberpunkVR_DebugDeltaRebuilt,CyberpunkVR_DebugVrcamPosWrites,CyberpunkVR_DebugMainPosWrites,CyberpunkVR_DebugIpdWorldWrites,
            CyberpunkVR_DebugVehicleWorldRecompose,CyberpunkVR_DebugSceneWorldRecompose,CyberpunkVR_DebugTakeoverWorldRecompose,g_patchCameraHits})h.I(int64_t(c));
        h.I(int64_t(testCameraCalls.size()));for(const float v:testCameraCalls)h.F(v);
        { float q[4]{};OpenXRHeadPose p{};cvr::roomscale::Vec2 c{};bool known=false;uint64_t id=0;h.B(CamWriteQuatRead(q,&p,&c,&known,&id));h.Fs(q,4);TraceHashPose(h,p);
          h.F(c.x);h.F(c.y);h.B(known);h.I(int64_t(id)); }
        { const auto rc=CapturePoseAddress(comp+0xE0);h.B(bool(rc));coverLabel+=bool(rc);if(rc) { h.I(int64_t(rc.payload.poseId));h.I(rc.payload.view);TraceHashPose(h,rc.payload.head); } }
        { VrikTransformSnapshot s{};const bool ok=VRIK_ReadNativeTransformSnapshot(&s);h.B(ok);if(ok)h.Fs(reinterpret_cast<const float*>(&s),8); }
        coverWritten+=CyberpunkVR_DebugMainPosWrites>0;coverIpd=int(CyberpunkVR_DebugIpdWorldWrites);coverDelta=int(CyberpunkVR_DebugDeltaRebuilt);
        g_roomUs+=r.Chance(60) ? 200000+r.Next()%100000:4000+r.Next()%16000;g_roomMs=g_roomUs/1000;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"patchcam: composed %d ipd %d delta %d labelled %d main-pos %llu vrcam-pos %llu bd-base %llu lens-push %d\n",
        coverComposed,coverIpd,coverDelta,coverLabel,static_cast<unsigned long long>(CyberpunkVR_DebugMainPosWrites),
        static_cast<unsigned long long>(CyberpunkVR_DebugVrcamPosWrites),static_cast<unsigned long long>(CyberpunkVR_DebugBdSceneBaseInPatch),coverWritten);
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;CyberpunkVR_RuntimeDiagnostics=1;g_isInVehicle=false;g_sceneTier=1;testDeviceCam=false;g_menuModeValue=0;g_bdActive=0;
    testCameraKinds.clear();g_lensComp=0;g_takeoverEntity=0;g_camObjMain=0;
}
// THE LOCATE CAMERA TRACE: the engine's located camera (src/Hooks/LocateCamera.cpp, or its Rust port) over a fake
// buffer and fake MAIN/VRCAM components whose vtables and notify slot are in this image -- the heading and the
// composition from the frame's sample, the braindance bases and the scene comfort, the shared-scene and write-site
// handoffs, the head translation and its recipe, the view and located frames, the per-eye shift, the takeover viewpoint
// and push, the braindance push, the labels, the ADS weapon zoom and its telemetry, the ShotSnap packet -- and every
// counter and global it publishes.
struct LocatePool { alignas(16) uint8_t bytes[0x1000]; };
uintptr_t g_lcVtable[0x60]{};uintptr_t g_lcPoolBase=0;
void __fastcall LcNotify(uintptr_t component,uintptr_t transform) {
    testCameraCalls.insert(testCameraCalls.end(),{408,float(int64_t(component-g_lcPoolBase)),float(int64_t(transform-g_lcPoolBase))});
}
void TraceLocate(TraceHash& h,uint64_t seed) {
    using namespace cvr::camera;
    TraceRng r(seed);
    static LocatePool pool{};std::memset(&pool,0,sizeof(pool));
    const auto at=[&](size_t off) { return reinterpret_cast<uintptr_t>(pool.bytes)+off; };
    g_lcPoolBase=at(0);for(auto& v:g_lcVtable)v=0;g_lcVtable[0x240/8]=reinterpret_cast<uintptr_t>(&LcNotify);
    static std::vector<uintptr_t> heapVtable(0x60,reinterpret_cast<uintptr_t>(&LcNotify));   // outside the image
    const uintptr_t buffer=at(0x000),comps[2]={at(0x400),at(0x800)};   // the located camera; MAIN, VRCAM
    g_VRIKClockQpc=RoomQpc;g_VRIKClockMs=RoomMs;
    const auto writeQuat=[&](uintptr_t a) { float q[4];r.Quat(q,3.1f,1.2f,.6f);if(r.Chance(20))q[0]=2;std::memcpy(reinterpret_cast<void*>(a),q,16); };
    const auto writePos=[&](uintptr_t a) { int32_t p[3];for(auto& v:p)v=int32_t(r.Next()%400000)-200000;std::memcpy(reinterpret_cast<void*>(a),p,12); };
    const auto setVt=[&](uintptr_t comp) {
        const uintptr_t vt=r.Chance(10) ? reinterpret_cast<uintptr_t>(heapVtable.data()):reinterpret_cast<uintptr_t>(g_lcVtable);
        std::memcpy(reinterpret_cast<void*>(comp),&vt,8);
    };
    writePos(buffer);writeQuat(buffer+16);
    for(const auto c:comps) { setVt(c);writePos(c+0xE0);writeQuat(c+0xF0);RoomPut<float>(reinterpret_cast<uint8_t*>(c),0x280,.3f);RoomPut<float>(reinterpret_cast<uint8_t*>(c),0x284,1.f); }
    int coverXr=0,coverBd=0,coverTakeover=0,coverPush=0,coverLabel=0,coverZoom=0,coverTelemetry=0;
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;if(r.Chance(900))++testOriginSerial;
        // the switches
        if(r.Chance(40)) {
            CyberpunkVR_CamWriteInPatch=!r.Chance(6);CyberpunkVR_CamComposeAtWrite=!r.Chance(6);CyberpunkVR_OneSamplePerFrame=!r.Chance(5);
            CyberpunkVR_BdQuatFromBuffer=int32_t(r.Next()%3);CyberpunkVR_BdBaseFromLocate=!r.Chance(3);CyberpunkVR_BdQuatFromWriteSite=!r.Chance(3);
            CyberpunkVR_ViewYawFromEngine=r.Chance(3);CyberpunkVR_VehicleAnchorFromViewYaw=!r.Chance(4);CyberpunkVR_HeadTranslationInPatch=!r.Chance(4);
            CyberpunkVR_BdIpdInLocate=!r.Chance(4);CyberpunkVR_IpdInWorldPos=!r.Chance(5);CyberpunkVR_BdMainPosFromScene=!r.Chance(3);
            CyberpunkVR_BdPushTransform=int32_t(r.Next()%4);CyberpunkVR_BdPushBase=!r.Chance(3);CyberpunkVR_AdsWeaponZoomSync=!r.Chance(6);
            CyberpunkVR_DevCamOffsetX=r.Chance(3) ? r.Range(-.2f,.2f):0.f;CyberpunkVR_DevCamOffsetY=r.Chance(3) ? r.Range(-.2f,.2f):0.f;
            CyberpunkVR_DevCamOffsetZ=r.Chance(4) ? r.Range(-.1f,.1f):0.f;
            g_liveControls.xrSceneCamComfortDeg=r.Chance(2) ? r.Range(5,40):0.f;g_liveControls.xrDisableMouseY=r.Chance(2);
            g_liveControls.xrHeadOffsetX=r.Range(-.1f,.1f);g_liveControls.xrHeadOffsetY=r.Range(-.1f,.1f);g_liveControls.xrHeadOffsetZ=r.Range(-.1f,.1f);
            g_liveControls.xrVehHeadOffsetX=r.Range(-.2f,.2f);g_liveControls.xrVehHeadOffsetY=r.Range(-.2f,.2f);g_liveControls.xrVehHeadOffsetZ=r.Range(-.2f,.2f);
        }
        // the state
        g_isInVehicle=r.Chance(8);g_sceneTier=r.Chance(8) ? int(r.Next()%6):1;g_vehicleState=int(r.Next()%12)-1;testDeviceCam=r.Chance(10);
        g_menuModeValue=r.Chance(25);testLobby=r.Chance(3);if(r.Chance(10))CyberpunkVR_MainIsRightEye=r.Chance(2);
        g_bdActive=r.Chance(4) ? 1:0;g_bdScenePoseValid=r.Chance(3) ? 0:1;r.Quat(g_bdSceneQuat,3.1f,1.2f,.6f);
        for(auto& v:g_bdScenePosFP)v=int32_t(r.Next()%400000)-200000;
        g_isAiming=r.Chance(3);CyberpunkVR_SightZoom=r.Chance(10);CyberpunkVR_MainAdsZoomFactor=TraceOddFloat(r,.3f,15.f);
        for(const auto c:comps) {
            if(r.Chance(30))setVt(c);
            if(r.Chance(12)) { RoomPut<float>(reinterpret_cast<uint8_t*>(c),0x280,TraceOddFloat(r,-.5f,1.5f));RoomPut<float>(reinterpret_cast<uint8_t*>(c),0x284,TraceOddFloat(r,-1.f,80.f)); }
        }
        g_camObjMain=r.Chance(10) ? 0:comps[0];g_camObjVrcam=r.Chance(10) ? 0:comps[1];g_lensComp=r.Chance(2) ? 0:comps[1];
        { float q[4];r.Quat(q,3.1f,1.2f,.6f);for(int k=0;k<4;++k)g_engineCamQuat[k]=q[k];g_engineCamQuatValid=!r.Chance(5); }
        if(r.Chance(4)) { for(int k=0;k<4;++k)g_headQuatComposed[k]=RoomGet<float>(reinterpret_cast<uint8_t*>(buffer),16+4*k); }
        else { float q[4];r.Quat(q,3.1f,1.2f,.6f);for(int k=0;k<4;++k)g_headQuatComposed[k]=q[k]; }
        g_headQuatValid=r.Chance(5) ? 0:1;g_gamePitchRadians=r.Range(-.5f,.5f);
        g_viewYawUsedValid=r.Chance(3);g_viewYawUsedRad=r.Range(-3,3);
        CyberpunkVR_EngineBodyYawValid=!r.Chance(4);{ const float y=r.Range(-3,3);CyberpunkVR_EngineBodyYawZ=std::sin(y*.5f);CyberpunkVR_EngineBodyYawW=std::cos(y*.5f); }
        g_devCamPosValid=r.Chance(3) ? 0:1;
        for(int k=0;k<3;++k)g_devCamPosFP[k]=RoomGet<int32_t>(reinterpret_cast<uint8_t*>(buffer),4*k)+(r.Chance(4) ? int32_t(r.Next()%400000)-200000:int32_t(r.Next()%60000)-30000);
        g_devCamAimValid=r.Chance(3) ? 0:1;g_devCamAimYaw=r.Range(-3,3);
        testLocateOwnsTakeover=r.Chance(4);testBdPushOwns=r.Chance(5);
        // the shot packet the hook reads: the menu flag, the eye bake, the entity tick and its position, the speed
        { const uint32_t menu=r.Chance(20) ? 1u:0u;std::memcpy(&testShotShared[81],&menu,4); }
        testShotShared[119]=r.Chance(3) ? 1.f:0.f;for(int k=116;k<119;++k)testShotShared[k]=r.Range(-.05f,.05f);
        testShotShared[131]=r.Chance(3) ? 1.f:0.f;if(r.Chance(2))testShotShared[99]+=1;
        for(int k=96;k<99;++k)testShotShared[k]=r.Range(-2,2);for(int k=128;k<131;++k)testShotShared[k]=r.Range(-.2f,.2f);
        testShotShared[132]=r.Range(-6,6);testShotShared[133]=r.Range(-6,6);
        // the XR samples
        testCameraPoseOk=!r.Chance(6);TraceCameraPose(r,testCameraPoseFrame);testCameraPoseFrame.originSerial=r.Chance(10) ? testOriginSerial+1:testOriginSerial;
        if(r.Chance(3))testCameraPoseSequence=r.Next()%5000;
        testHeadPoseOk=!r.Chance(5);TraceCameraPose(r,testHeadPose);testHeadPose.originSerial=testOriginSerial;
        testRenderEye=int(r.Next()%2);r.Vec(testCamOffset,.05f);testHandOk=!r.Chance(4);TraceCameraPose(r,testHandPose);
        // what the write site published, and MAIN's eye
        if(r.Chance(3)) {
            float q[4];r.Quat(q,3.1f,1.2f,.6f);OpenXRHeadPose p{};TraceCameraPose(r,p);if(r.Chance(2))p=testCameraPoseFrame;
            const uint64_t id=r.Chance(2) ? testCameraPoseSequence:r.Next()%5000;CamWriteQuatPublish(q[0],q[1],q[2],q[3],p,nullptr,id);
        }
        if(r.Chance(5)) {
            int32_t eye[3],centre[3],base[3];
            for(int k=0;k<3;++k) { eye[k]=RoomGet<int32_t>(reinterpret_cast<uint8_t*>(buffer),4*k)+int32_t(r.Next()%20000)-10000;centre[k]=eye[k]+int32_t(r.Next()%8000)-4000;base[k]=centre[k]-int32_t(r.Next()%30000); }
            MainEyeCentrePublish(eye,centre,base);
        }
        if(r.Chance(2))writePos(buffer);if(r.Chance(2))writeQuat(buffer+16);
        if(r.Chance(40))++g_VrikFrameEpoch;
        testCameraCalls.clear();testTransportCalls.clear();
        const uint64_t bdBefore=CyberpunkVR_DebugBdBaseFromLocate+CyberpunkVR_DebugBdBaseFromScript+CyberpunkVR_DebugBdQuatFromBuffer;
        const uint64_t takeoverBefore=CyberpunkVR_DebugTakeoverPush,pushBefore=CyberpunkVR_DebugBdPushTransform+CyberpunkVR_DebugBdPushMain;
        const uint64_t labelBefore=CyberpunkVR_DebugSceneLabelPublished,zoomBefore=CyberpunkVR_DebugAdsZoomWrites;
        const float xmm0=r.Range(-1,1);
        if(r.Chance(60)) OnLocateCameraCallback(r.Chance(2) ? nullptr:reinterpret_cast<float*>(uintptr_t(0x100)),xmm0);
        else if(r.Chance(80)) { float stackCam[8];std::memcpy(stackCam,reinterpret_cast<const void*>(buffer),32);OnLocateCameraCallback(stackCam,xmm0); }
        else OnLocateCameraCallback(reinterpret_cast<float*>(buffer),xmm0);
        coverXr+=testCameraCalls.size()>0;
        coverBd+=CyberpunkVR_DebugBdBaseFromLocate+CyberpunkVR_DebugBdBaseFromScript+CyberpunkVR_DebugBdQuatFromBuffer!=bdBefore;
        coverTakeover+=CyberpunkVR_DebugTakeoverPush!=takeoverBefore;coverPush+=CyberpunkVR_DebugBdPushTransform+CyberpunkVR_DebugBdPushMain!=pushBefore;
        coverLabel+=CyberpunkVR_DebugSceneLabelPublished!=labelBefore;coverZoom+=CyberpunkVR_DebugAdsZoomWrites!=zoomBefore;
        // the braindance push straight, as VrCore's editor alignment calls it
        if(r.Chance(10)) {
            float q[4];r.Quat(q,3.1f,1.2f,.6f);if(r.Chance(10))q[3]=3;int32_t base[3];for(auto& v:base)v=int32_t(r.Next()%400000)-200000;
            OpenXRHeadPose p{};TraceCameraPose(r,p);const uint64_t id=r.Chance(2) ? 0:r.Next()%5000+1;
            BdPushTransformOnce(r.Chance(8) ? 0x1000:comps[r.Next()%2],q[0],q[1],q[2],q[3],r.Chance(2),r.Chance(4) ? nullptr:base,r.Chance(2) ? &p:nullptr,id);
        }
        // what it wrote and published
        for(int w=0;w<8;++w)h.I(RoomGet<int32_t>(reinterpret_cast<uint8_t*>(buffer),4*w));
        for(const auto c:comps) { for(int w=0;w<12;++w)h.I(RoomGet<int32_t>(reinterpret_cast<uint8_t*>(c),0xE0+4*w));h.F(RoomGet<float>(reinterpret_cast<uint8_t*>(c),0x280));h.F(RoomGet<float>(reinterpret_cast<uint8_t*>(c),0x284)); }
        h.F(g_dbgEntryYaw);h.F(g_dbgEntryPosX);h.F(g_dbgEntryPosY);h.F(g_dbgEntryPosZ);
        h.F(g_headingSy);h.F(g_headingCy);h.F(g_headingPitchS);h.F(g_headingPitchC);h.I(g_headingValid);
        h.Fs(const_cast<const float*>(g_headQuatComposed),4);h.I(g_headQuatValid);h.Fs(g_bdPushQuat,4);h.I(g_lastLocateSeq);
        for(int k=0;k<3;++k) { h.I(g_lastLocatePosFP[k]);h.I(g_lastIpdShiftFP[k]);h.I(g_headDeltaFP[k].load());h.I(g_locateCenterFP[k].load());h.I(g_locatePosFP[k].load()); }
        h.Fs(g_lastLocateQuat,4);h.I(g_headDeltaValid.load());h.I(g_locatePosValid.load());
        h.F(CyberpunkVR_DebugSceneComfortLagDeg);h.I(CyberpunkVR_DebugSceneComfortCuts);
        for(const uint64_t c:{CyberpunkVR_DebugAdsZoomWrites,CyberpunkVR_DebugAdsZoomRejects,CyberpunkVR_DebugSceneLabelPublished,CyberpunkVR_DebugBdQuatFromBuffer,
            CyberpunkVR_DebugBdBaseFromLocate,CyberpunkVR_DebugBdBaseFromScript,CyberpunkVR_DebugBdQuatFromWriteSite,CyberpunkVR_DebugBdMainPos,
            CyberpunkVR_DebugBdIpdLocate,CyberpunkVR_DebugTakeoverPush,CyberpunkVR_DebugBdPushTransform,CyberpunkVR_DebugBdPushMain,g_locateCameraHits})h.I(int64_t(c));
        for(int k=0;k<256;++k) {
            if(k==51 || k==52)continue;
            if(k==143 || k==81) { uint32_t u;std::memcpy(&u,&testShotShared[k],4);h.I(u);continue; }
            h.F(testShotShared[k]);
        }
        { uint32_t lo,hi;std::memcpy(&lo,&testShotShared[51],4);std::memcpy(&hi,&testShotShared[52],4);const uint64_t a=(uint64_t(hi)<<32)|lo;h.I(a ? int64_t(a-g_lcPoolBase):-1); }
        h.I(int64_t(testCameraCalls.size()));for(const float v:testCameraCalls)h.F(v);h.I(int64_t(testTransportCalls.size()));
        { float q[4]{};OpenXRHeadPose p{};cvr::roomscale::Vec2 c{};bool known=false;uint64_t id=0;h.B(CamWriteQuatRead(q,&p,&c,&known,&id));h.Fs(q,4);TraceHashPose(h,p);
          h.F(c.x);h.F(c.y);h.B(known);h.I(int64_t(id)); }
        { LocatedCameraFrame f{};const bool ok=LocatedCameraFrameRead(&f);h.B(ok);if(ok) { h.Fs(f.worldPos,3);h.Fs(f.worldQuat,4);h.I(f.sequence);h.I(f.frameEpoch);h.I(f.bodyCentreKnown);h.Fs(f.bodyBaseWorld,3); } }
        { ViewFrame f{};const bool ok=ViewFrameRead(&f);h.B(ok);if(ok) { h.Fs(f.viewQuat,4);h.Fs(f.worldDelta,3);h.F(f.deltaSemantics);h.F(f.headingRad);h.Fs(f.headOri,4);h.F(f.stampMs); } }
        { AnchorRecipe a{};const bool ok=AnchorRecipeRead(&a);h.B(ok);
          if(ok) { h.F(a.trackingOffset.x);h.F(a.trackingOffset.y);h.F(a.trackingOffset.z);h.F(a.modelOffset.x);h.F(a.modelOffset.y);h.F(a.modelOffset.z);
                   h.F(a.scale);h.F(a.trackingYaw);h.F(a.modelYaw);h.F(a.centreYaw);h.B(a.bodyOrbit); } }
        for(const uintptr_t a:{buffer,comps[0]+0xE0,comps[1]+0xE0}) {
            const auto rc=CapturePoseAddress(a);h.B(bool(rc));if(rc) { h.I(int64_t(rc.payload.poseId));h.I(rc.payload.view);TraceHashPose(h,rc.payload.head); }
        }
        { AdsCameraTelemetryUiState s{};GetAdsCameraTelemetryUiState(&s);coverTelemetry+=s.samples>0;
          h.I(s.available);h.I(s.aiming);h.I(s.baselineValid);h.I(s.samples);h.F(s.residualRight);h.F(s.residualForward);h.F(s.residualUp);
          h.F(s.deltaRight);h.F(s.deltaForward);h.F(s.deltaUp);h.F(s.peakRight);h.F(s.peakForward);h.F(s.peakUp); }
        { float raw[4],out[4];r.Quat(raw,3.1f,1.2f,.6f);if(r.Chance(3))for(int k=0;k<4;++k)raw[k]=g_bdSceneQuat[k];h.B(SceneComfortApply(raw,out));h.Fs(out,4); }
        g_roomUs+=r.Chance(60) ? 200000+r.Next()%100000:4000+r.Next()%16000;g_roomMs=g_roomUs/1000;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"locate: xr %d bd %d takeover-push %d bd-push %d scene-label %d ads-zoom %d ads-telemetry %d comfort-cuts %u\n",
        coverXr,coverBd,coverTakeover,coverPush,coverLabel,coverZoom,coverTelemetry,CyberpunkVR_DebugSceneComfortCuts);
    g_VRIKClockQpc=nullptr;g_VRIKClockMs=nullptr;CyberpunkVR_RuntimeDiagnostics=1;g_isInVehicle=false;g_sceneTier=1;g_vehicleState=0;testDeviceCam=false;
    g_menuModeValue=0;testLobby=false;g_bdActive=0;g_isAiming=false;g_camObjMain=0;g_camObjVrcam=0;g_lensComp=0;g_devCamPosValid=0;g_devCamAimValid=0;
    testLocateOwnsTakeover=false;testBdPushOwns=false;std::memset(testShotShared,0,sizeof(testShotShared));
}
// THE WEAPON AIM TRACE: the hitscan from the muzzle (src/Hooks/WeaponAim.cpp, or its Rust port) over a fake evaluator,
// fake shot containers and the engine's typed setter stood in for by a stub with its prologue -- whose ray it is, the
// staleness bound, the left gun of two by record, trigger and alternation, the sight line, the cone centre from the
// camera or the shot's first ray, the rotation onto the barrel, the recoil latches, the cast ray published back --
// and the targeting update's takeover ray and gaze centring over a fake targeting state, and the pose copy.
struct WaTrace {
    char result=1;float out[4]{};
    std::vector<uint8_t> state=std::vector<uint8_t>(0x31400);   // the targeting state
    alignas(16) uint8_t pool[0x3000]{};                         // shot containers, records, the source cache, poses
};
WaTrace* g_wa=nullptr;
uintptr_t WaRel(uintptr_t p) {
    const auto pool=reinterpret_cast<uintptr_t>(g_wa->pool),state=reinterpret_cast<uintptr_t>(g_wa->state.data());
    if(p>=pool && p<pool+sizeof(g_wa->pool))return p-pool;
    if(p>=state && p<state+g_wa->state.size())return 0x100000+(p-state);
    return p ? 0xFFFFFFFF:0;
}
char __fastcall WaFakeEval(void* rcx,void* rdx,float* out) {
    testCameraCalls.insert(testCameraCalls.end(),{501,float(WaRel(reinterpret_cast<uintptr_t>(rcx))),float(WaRel(reinterpret_cast<uintptr_t>(rdx)))});
    if(out)std::memcpy(out,g_wa->out,16);
    return g_wa->result;
}
bool g_waSetterOk=true;
bool __fastcall WaFakeSetter(void* bb,const uint64_t* key,const float* value,uint8_t flag) {
    testCameraCalls.insert(testCameraCalls.end(),{502,float(WaRel(reinterpret_cast<uintptr_t>(bb))),float(*key&0xFFFF),value[0],value[1],value[2],value[3],float(flag)});
    return g_waSetterOk;
}
void* __fastcall WaFakePoseCopy(void* rcx,void* rdx,void*,void*) {
    if(WaRel(reinterpret_cast<uintptr_t>(rcx))<sizeof(g_wa->pool) && WaRel(reinterpret_cast<uintptr_t>(rdx))<sizeof(g_wa->pool))
        std::memmove(rcx,rdx,0x30);
    testCameraCalls.insert(testCameraCalls.end(),{503,float(WaRel(reinterpret_cast<uintptr_t>(rcx))),float(WaRel(reinterpret_cast<uintptr_t>(rdx)))});
    return rcx;
}
void* __fastcall WaFakeTargeting(void* rcx,void*,void*,void*) {
    testCameraCalls.insert(testCameraCalls.end(),{504,float(WaRel(reinterpret_cast<uintptr_t>(rcx)))});
    return reinterpret_cast<void*>(uintptr_t(0x77));
}
void TraceWeaponAim(TraceHash& h,uint64_t seed) {
    using namespace cvr::camera;
    TraceRng r(seed);
    const bool resolveBefore=testResolveRenderedPose;testResolveRenderedPose=true;
    static WaTrace wa;g_wa=&wa;std::memset(wa.pool,0,sizeof(wa.pool));std::fill(wa.state.begin(),wa.state.end(),uint8_t(0));
    static std::array<float,256> hands{};hands.fill(0);g_pSharedHands=hands.data();
    // The setter: its prologue, then on to the recorder. The image base is placed so that base + 0x475078 is the stub.
    static uint8_t* stub=nullptr;
    if(!stub) {
        stub=static_cast<uint8_t*>(VirtualAlloc(nullptr,64,MEM_COMMIT|MEM_RESERVE,PAGE_EXECUTE_READWRITE));
        const uint8_t prologue[]={0x4C,0x8B,0xDC,0x49,0x89,0x5B,0x08,0x48,0xB8};std::memcpy(stub,prologue,sizeof(prologue));
        const uint64_t to=reinterpret_cast<uint64_t>(&WaFakeSetter);std::memcpy(stub+9,&to,8);stub[17]=0xFF;stub[18]=0xE0;
        FlushInstructionCache(GetCurrentProcess(),stub,64);
    }
    CvrWeaponAimBind(reinterpret_cast<uintptr_t>(&WaFakeEval),reinterpret_cast<uintptr_t>(stub)-0x475078,
                     reinterpret_cast<uintptr_t>(&WaFakePoseCopy),reinterpret_cast<uintptr_t>(&WaFakeTargeting));
    const auto at=[&](size_t off) { return reinterpret_cast<uintptr_t>(wa.pool)+off; };
    const uintptr_t bbs[2]={at(0x000),at(0x400)},records[2]={at(0x800),at(0x880)},sourceCache=at(0xA00),player=at(0xE00);
    const uintptr_t poses[3]={at(0x1000),at(0x1100),at(0x1200)};
    const uintptr_t state=reinterpret_cast<uintptr_t>(wa.state.data());
    const uint64_t names[]={0x8CB4C0891BD255EDull,0x8DBDD9257C33612Full,0x4CBF3A26FCA1D74Aull,0x1E65942E132F4F7Aull,0x1111,0x2222,0x3333};
    cvr::roomscale::SetPlayer(player);RoomPut<uint64_t>(reinterpret_cast<uint8_t*>(player),0x48,0xE17E);
    const auto writeQuat=[&](float* q) { r.Quat(q,3.1f,1.2f,.6f); };
    // a shot container: keys at +0x100, values at +0x200, 24 bytes each
    const auto buildContainer=[&](uintptr_t bb) {
        const uint32_t count=r.Chance(15) ? 300+uint32_t(r.Next()%100):uint32_t(r.Next()%10);
        const uintptr_t keys=r.Chance(30) ? 0:bb+0x100,vals=r.Chance(30) ? bb+0x204:bb+0x200;   // a misaligned array sometimes
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(bb),0x48,keys);RoomPut<uint32_t>(reinterpret_cast<uint8_t*>(bb),0x54,count);
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(bb),0x58,vals);
        for(uint32_t i=0;i<std::min<uint32_t>(count,10);++i) {
            RoomPut<uint64_t>(reinterpret_cast<uint8_t*>(bb),0x100+8*i,names[r.Next()%7]);
            RoomPut<uint64_t>(reinterpret_cast<uint8_t*>(bb),0x200+24*i,r.Chance(5) ? uint64_t(r.Next()%2):0x1000|uint64_t(r.Next()%2));
            const uintptr_t value=r.Chance(2) ? records[r.Next()%2]:uintptr_t(r.Next()%3);
            RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(bb),0x208+24*i,value);
        }
    };
    int coverMine=0,coverLeft=0,coverSight=0,coverShared=0,coverCone=0,coverGaze=0,coverTakeover=0,coverCopy=0;
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        if(r.Chance(40)) {
            CyberpunkVR_HitscanFromMuzzle=!r.Chance(8);CyberpunkVR_ConeCentreFromCamera=r.Chance(3);CyberpunkVR_HitscanSharedRay=!r.Chance(5);
            CyberpunkVR_HitscanFromSightLine=!r.Chance(4);g_waSetterOk=!r.Chance(6);
        }
        // the barrel, the left barrel, the dual wield, the camera, the shot sequence, the sight line
        for(int k=200;k<203;++k)hands[k]=r.Range(-3,3);hands[203]=r.Chance(8) ? 0.f:1.f;
        { float v[3];r.Vec(v,1);if(r.Chance(10))v[0]=v[1]=v[2]=.1f;hands[24]=v[0];hands[25]=v[1];hands[26]=v[2]; }hands[27]=r.Chance(8) ? 0.f:1.f;
        hands[165]=r.Chance(2) ? 0.f:float(1|(r.Chance(2) ? 2:0)|(r.Chance(2) ? 4:0));hands[172]=r.Chance(5) ? 0.f:1.f;
        for(int k=166;k<169;++k)hands[k]=r.Range(-3,3);{ float v[3];r.Vec(v,1);if(r.Chance(10))v[0]=v[1]=v[2]=0;for(int k=0;k<3;++k)hands[169+k]=v[k]; }
        { float q[4];writeQuat(q);if(r.Chance(8))q[3]=2;for(int k=0;k<4;++k)g_lastLocateQuat[k]=q[k]; }
        if(r.Chance(3))g_provMuzzleSeq=g_provMuzzleSeq+1;
        CyberpunkVR_DualWieldRecords[0]=r.Chance(4) ? 0:0xAAAA;CyberpunkVR_DualWieldRecords[1]=r.Chance(4) ? 0:(r.Chance(5) ? 0xAAAA:0xBBBB);
        RoomPut<uint32_t>(reinterpret_cast<uint8_t*>(records[0]),0x40,0xAAAA);RoomPut<uint32_t>(reinterpret_cast<uint8_t*>(records[1]),0x40,r.Chance(2) ? 0xBBBB:0xCCCC);
        { const uint64_t now=GetTickCount64();CyberpunkVR_SightLineTick=r.Chance(3) ? 0:(r.Chance(4) ? now-100000:now-5); }
        for(auto& v:CyberpunkVR_SightLineOffset)v=r.Range(-.3f,.3f);
        // the shot: containers, the evaluator's answer, which container each callsite is handed
        for(const auto bb:bbs)if(r.Chance(3))buildContainer(bb);
        wa.result=r.Chance(10) ? 0:1;
        { float v[3];r.Vec(v,3);for(int k=0;k<3;++k)wa.out[k]=r.Chance(3) ? hands[200+k]+v[k]*.5f:v[k];wa.out[3]=1; }
        const uintptr_t bb=r.Chance(10) ? (r.Chance(2) ? 0:at(0x1F00)):bbs[r.Next()%2];
        testCameraCalls.clear();
        float out[4];std::memcpy(out,wa.out,16);
        const uint64_t mineBefore=CyberpunkVR_DebugPhysicalRayMine,leftBefore=CyberpunkVR_DebugPhysicalRayLeftGun,sightBefore=CyberpunkVR_DebugPhysicalRaySightLine;
        const uint64_t sharedBefore=CyberpunkVR_DebugPhysicalRayShared,coneBefore=CyberpunkVR_DebugPhysicalRayConeRays;
        if(!r.Chance(10)) { const char res=Hooked_WaPhysicalRayOrigin(reinterpret_cast<void*>(at(0x1E00)),reinterpret_cast<void*>(bb),r.Chance(50) ? nullptr:out);h.I(res);h.Fs(out,4); }
        // the forward evaluator: a pellet direction around the camera
        { float f[3];r.Vec(f,1);if(r.Chance(10))f[0]=f[1]=f[2]=0;wa.out[0]=f[0];wa.out[1]=f[1];wa.out[2]=f[2];wa.out[3]=0;wa.result=r.Chance(10) ? 0:1; }
        std::memcpy(out,wa.out,16);
        const uintptr_t bb2=r.Chance(8) ? bbs[r.Next()%2]:bb;
        if(!r.Chance(10)) { const char res=Hooked_WaPhysicalRayForward(reinterpret_cast<void*>(at(0x1E00)),reinterpret_cast<void*>(bb2),r.Chance(50) ? nullptr:out);h.I(res);h.Fs(out,4); }
        coverMine+=CyberpunkVR_DebugPhysicalRayMine!=mineBefore;coverLeft+=CyberpunkVR_DebugPhysicalRayLeftGun!=leftBefore;
        coverSight+=CyberpunkVR_DebugPhysicalRaySightLine!=sightBefore;coverShared+=CyberpunkVR_DebugPhysicalRayShared!=sharedBefore;
        coverCone+=CyberpunkVR_DebugPhysicalRayConeRays!=coneBefore;
        // the targeting update: the state's owner, the camera transform, the default ray, the source cache
        if(r.Chance(2)) {
            uint8_t* s=wa.state.data();
            RoomPut<uintptr_t>(s,0,r.Chance(8) ? at(0x1D00):player);RoomPut<uint64_t>(s,8,r.Chance(8) ? 0x1234:0xE17E);RoomPut<uint32_t>(s,24,r.Chance(8) ? 0:1);
            float pose[8];r.Vec(pose,20);writeQuat(pose+4);if(r.Chance(10))pose[7]=3;if(r.Chance(30))pose[1]=std::numeric_limits<float>::quiet_NaN();
            std::memcpy(s+0x31100,pose,32);
            const float x=pose[4],y=pose[5],z=pose[6],w=pose[7];
            float f[4]={2*(x*y-z*w),1-2*(x*x+z*z),2*(y*z+x*w),r.Chance(2) ? 1.f:0.f};
            if(r.Chance(5)) { f[0]+=r.Range(-.5f,.5f);f[1]+=r.Range(-.5f,.5f); }
            std::memcpy(s+0x31200,f,16);
            RoomPut<uintptr_t>(s,0x30CC8,r.Chance(4) ? 0:sourceCache);
            float cached[4]={f[0],f[1],f[2],r.Chance(5) ? 2.f:f[3]};if(r.Chance(6))cached[0]+=1;std::memcpy(reinterpret_cast<void*>(sourceCache+0x280),cached,16);
            g_remoteCamOn=r.Chance(3) ? 0:1;g_takeoverEntityId=r.Chance(4) ? 0:77;testDeviceCam=r.Chance(2);g_menuModeValue=r.Chance(8);
            testVruiCaptures=r.Chance(10);testSessionRunning=!r.Chance(8);
            cvr::gaze::headLook=!r.Chance(6);cvr::gaze::Publish({r.Chance(5) ? 0.f:r.Range(-1.2f,1.2f),r.Range(-1,1)});
            const uint64_t gazeBefore=CyberpunkVR_GazeTargetUpdates;
            float before[8];std::memcpy(before,s+0x311F0,32);
            h.I(int64_t(reinterpret_cast<uintptr_t>(Hooked_WaTargetingUpdate(s,nullptr,nullptr,nullptr))));
            coverGaze+=CyberpunkVR_GazeTargetUpdates!=gazeBefore;coverTakeover+=std::memcmp(before,s+0x311F0,16)!=0;
            for(int k=0;k<12;++k)h.F(RoomGet<float>(s,0x311F0+4*k));
            for(int k=0;k<4;++k)h.F(RoomGet<float>(reinterpret_cast<uint8_t*>(sourceCache),0x280+4*k));
            h.I(int64_t(WaRel(CyberpunkVR_GazeTargetState)));h.I(int64_t(CyberpunkVR_GazeTargetUpdates));
        }
        // the pose copy: a labelled source followed to its destination
        if(r.Chance(3)) {
            const int from=int(r.Next()%3),to=int(r.Next()%3);
            if(r.Chance(2)) {
                float q[4];writeQuat(q);int32_t p[4];for(auto& v:p)v=int32_t(r.Next()%400000)-200000;
                std::memcpy(reinterpret_cast<void*>(poses[from]),p,16);std::memcpy(reinterpret_cast<void*>(poses[from]+16),q,16);
                OpenXRHeadPose head{};TraceCameraPose(r,head);PublishCameraSetupPose(poses[from],at(0x1300),1+uint32_t(r.Next()%2),r.Next()%5000+1,head);
            }
            Hooked_WaPoseCopy(reinterpret_cast<void*>(poses[to]),reinterpret_cast<void*>(poses[from]),nullptr,nullptr);
            for(const auto p:poses) { const auto rc=CapturePoseAddress(p);h.B(bool(rc));coverCopy+=bool(rc) && p==poses[to] && from!=to;
                if(rc) { h.I(int64_t(rc.payload.poseId));h.I(rc.payload.view);TraceHashPose(h,rc.payload.head); } }
        }
        // what it published
        h.I(int64_t(testCameraCalls.size()));for(const float v:testCameraCalls)h.F(v);
        for(const unsigned long long c:{CyberpunkVR_DebugPhysicalRayCalls,CyberpunkVR_DebugPhysicalRayMine,CyberpunkVR_DebugPhysicalRayOriginWrites,
            CyberpunkVR_DebugPhysicalRayForwardWrites,CyberpunkVR_DebugPhysicalRayConeRays,CyberpunkVR_DebugPhysicalRayRecoil,CyberpunkVR_DebugPhysicalRayForeign,
            CyberpunkVR_DebugPhysicalRayLeftGun,CyberpunkVR_DebugPhysicalRayStaleMuzzle,CyberpunkVR_DebugPhysicalRaySightLine,CyberpunkVR_DebugPhysicalRayShared,
            CyberpunkVR_DebugPhysicalRaySharedMissing,static_cast<unsigned long long>(CyberpunkVR_DebugConeFirstRay)})h.I(int64_t(c));
        h.Fs(CyberpunkVR_DebugDualShot,4);for(const auto v:CyberpunkVR_DebugDualShotRecord)h.I(v);h.Fs(CyberpunkVR_DebugPhysicalRaySightLift,4);
        h.Fs(CyberpunkVR_DebugPhysicalRayRawForward,4);h.Fs(CyberpunkVR_DebugPhysicalRayOutForward,4);for(const auto k:CyberpunkVR_DebugPhysicalRayKeys)h.I(int64_t(k));
        h.I(g_provRecoilSeqSeen);
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"weaponaim: mine %d left %d sight %d cone %d shared %d gaze %d takeover %d copied %d\n",
        coverMine,coverLeft,coverSight,coverCone,coverShared,coverGaze,coverTakeover,coverCopy);
    CyberpunkVR_RuntimeDiagnostics=1;g_pSharedHands=nullptr;g_remoteCamOn=0;g_takeoverEntityId=0;testDeviceCam=false;g_menuModeValue=0;testVruiCaptures=false;
    testSessionRunning=true;testResolveRenderedPose=resolveBefore;cvr::gaze::headLook=true;cvr::gaze::Publish({});cvr::roomscale::SetPlayer(0);
}
// THE TANK TRACE: the steering target and the weapon aim (src/Hooks/PanzerSteering.cpp / PanzerWeaponAim.cpp, or the
// Rust port) over a fake model, vehicle and weapon slots -- the gates, the external camera's planar direction, the
// update's frame answered by the two getters from their own call sites only, the cannon launch converged onto the
// target from the targeted-launch site only, and the three seqlocked debug records.
struct PanzerPool { alignas(16) uint8_t bytes[0x3000]; };
PanzerPool* g_pz=nullptr;
bool g_pzAimOk=false,g_pzDirectionOk=false,g_pzGetterSame=true;float g_pzAim[7]{},g_pzDirection[3]{},g_pzNative[4]{};uint64_t g_pzPoseId=0;
uintptr_t g_pzRet[2]{};float g_pzTarget[3]{};
float PzRel(uintptr_t p) { const auto b=reinterpret_cast<uintptr_t>(g_pz->bytes);return p>=b && p<b+sizeof(g_pz->bytes) ? float(p-b):(p ? -1.f:0.f); }
bool PzMainAim(cvr::camera::MainAimPose* out) {
    testCameraCalls.push_back(610);if(!g_pzAimOk)return false;
    std::memcpy(out->position,g_pzAim,12);std::memcpy(out->rotation,g_pzAim+3,16);out->poseId=g_pzPoseId;return true;
}
bool PzDirection(float* out,uint64_t* id) { testCameraCalls.push_back(611);if(!g_pzDirectionOk)return false;std::memcpy(out,g_pzDirection,12);*id=g_pzPoseId;return true; }
uintptr_t __fastcall PzSteerOrig(uintptr_t model) { testCameraCalls.insert(testCameraCalls.end(),{601,PzRel(model)});return 0x55; }
float* __fastcall PzGetOrig(uintptr_t manager,float* out) {
    testCameraCalls.insert(testCameraCalls.end(),{602,PzRel(manager)});if(out)std::memcpy(out,g_pzNative,16);
    return g_pzGetterSame ? out:out+1;
}
uintptr_t __fastcall PzUpdateOrig(uintptr_t owner) {
    // the engine's update: its two getters from their call sites (or elsewhere), then the target it settles on
    float position[4]{},rotation[4]{};
    const float* p=CvrPanzerPositionAt(owner+0x40,position,g_pzRet[0]);const float* r=CvrPanzerRotationAt(owner+0x40,rotation,g_pzRet[1]);
    testCameraCalls.insert(testCameraCalls.end(),{603,PzRel(owner),position[0],position[1],position[2],rotation[0],rotation[1],rotation[2],rotation[3],
                                                 float(p==position),float(r==rotation)});
    std::memcpy(reinterpret_cast<void*>(owner+0xB34),g_pzTarget,12);
    return 0x66;
}
uintptr_t __fastcall PzLaunchOrig(uintptr_t owner,uintptr_t weapon,const float* position,const float* forward,const float* target,float spread,uint8_t count) {
    testCameraCalls.insert(testCameraCalls.end(),{604,PzRel(owner),PzRel(weapon),forward ? forward[0]:-9.f,forward ? forward[1]:-9.f,forward ? forward[2]:-9.f,
                                                 position[0],target[2],spread,float(count)});
    return 0x77;
}
void TracePanzer(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    static PanzerPool pool;g_pz=&pool;std::memset(pool.bytes,0,sizeof(pool.bytes));
    const auto at=[&](size_t off) { return reinterpret_cast<uintptr_t>(pool.bytes)+off; };
    const uintptr_t model=at(0x000),owner=at(0x1000),slots=at(0x2000),cannon=at(0x2100),missile=at(0x2200);
    const uintptr_t exe=0x140000000ull;
    const auto bind=[&](bool ready) {
        CvrPanzerBind(exe,reinterpret_cast<uintptr_t>(&PzSteerOrig),reinterpret_cast<uintptr_t>(&PzUpdateOrig),reinterpret_cast<uintptr_t>(&PzGetOrig),
                      reinterpret_cast<uintptr_t>(&PzGetOrig),reinterpret_cast<uintptr_t>(&PzLaunchOrig),ready,
                      reinterpret_cast<uintptr_t>(&PzMainAim),reinterpret_cast<uintptr_t>(&PzDirection));
    };
    bind(true);
    const uintptr_t positionCall=exe+0x2B97CC8+5,rotationCall=exe+0x2B97CFD+5,launchCall=exe+0x24CC50C+5;
    int coverSteer=0,coverAim=0,coverCannon=0;
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        if(r.Chance(200))bind(!r.Chance(4));
        // the gates
        CyberpunkVR_PanzerGazeSteering=!r.Chance(10);g_isDriving=!r.Chance(6);g_menuModeValue=r.Chance(15);g_sceneTier=r.Chance(8) ? 3:int(r.Next()%3);
        testOverlayVisible=r.Chance(15);testIsTank=!r.Chance(6);testMountedVehicle=r.Chance(8) ? at(0x1800):owner;
        // the model, the vehicle, its slots
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(model),0xE0,r.Chance(8) ? 0:(r.Chance(10) ? at(0x1800):owner));
        for(int k=0;k<3;++k)RoomPut<float>(reinterpret_cast<uint8_t*>(model),0xE8+4*k,r.Range(-1,1));
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(owner),0xB20,r.Chance(10) ? 0:slots);RoomPut<uint32_t>(reinterpret_cast<uint8_t*>(owner),0xB2C,r.Chance(10) ? 9u:uint32_t(r.Next()%4));
        RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(slots),0x10,r.Chance(5) ? missile:cannon);
        // the main view and the external camera
        g_pzAimOk=!r.Chance(4);g_pzDirectionOk=!r.Chance(4);g_pzPoseId=r.Next()%5000;
        { float v[3];r.Vec(v,20);std::memcpy(g_pzAim,v,12);float q[4];r.Quat(q,3.1f,1.2f,.6f);std::memcpy(g_pzAim+3,q,16); }
        { const float y=r.Range(-3,3);g_pzDirection[0]=-std::sin(y);g_pzDirection[1]=std::cos(y);g_pzDirection[2]=0; }
        for(auto& v:g_pzNative)v=r.Range(-5,5);g_pzGetterSame=!r.Chance(10);
        g_pzRet[0]=r.Chance(8) ? positionCall+1:positionCall;g_pzRet[1]=r.Chance(8) ? rotationCall-1:rotationCall;
        for(auto& v:g_pzTarget)v=r.Range(-50,50);
        testCameraCalls.clear();
        const auto steerBefore=CyberpunkVR_PanzerSteeringDebug.applied,aimBefore=CyberpunkVR_PanzerWeaponAimDebug.applied,cannonBefore=CyberpunkVR_PanzerCannonDebug.applied;
        h.I(int64_t(CvrPanzerSteerTarget(model)));
        for(int k=0;k<3;++k)h.F(RoomGet<float>(reinterpret_cast<uint8_t*>(model),0xE8+4*k));
        h.I(int64_t(CvrPanzerAimUpdate(r.Chance(10) ? at(0x1800):owner)));
        // a launch: from the targeted site or elsewhere, the cannon or a missile
        {
            float position[3],forward[3],target[3];r.Vec(position,20);r.Vec(forward,1);r.Vec(target,60);if(r.Chance(20))std::memcpy(target,position,12);
            const uintptr_t weapon=r.Chance(5) ? missile:cannon;const uintptr_t ret=r.Chance(8) ? launchCall+3:launchCall;
            const bool nulls=r.Chance(30);
            h.I(int64_t(CvrPanzerLaunchAt(r.Chance(10) ? at(0x1800):owner,weapon,position,nulls ? nullptr:forward,target,r.Range(0,2),uint8_t(r.Next()%4),ret)));
        }
        coverSteer+=CyberpunkVR_PanzerSteeringDebug.applied!=steerBefore;coverAim+=CyberpunkVR_PanzerWeaponAimDebug.applied!=aimBefore;
        coverCannon+=CyberpunkVR_PanzerCannonDebug.applied!=cannonBefore;
        // what it did and recorded
        h.I(int64_t(testCameraCalls.size()));for(const float v:testCameraCalls)h.F(v);
        { const auto& d=CyberpunkVR_PanzerSteeringDebug;h.I(int64_t(d.calls));h.I(int64_t(d.applied));h.F(PzRel(d.model));h.F(PzRel(d.owner));h.I(int64_t(d.poseId));
          h.Fs(d.nativeTarget,3);h.Fs(d.target,3);h.I(d.reason);h.I(CyberpunkVR_PanzerSteeringDebugSeq.load()); }
        { const auto& d=CyberpunkVR_PanzerWeaponAimDebug;h.I(int64_t(d.calls));h.I(int64_t(d.applied));h.I(int64_t(d.positionWrites));h.I(int64_t(d.rotationWrites));
          h.F(PzRel(d.owner));h.I(int64_t(d.poseId));h.Fs(d.nativePosition,3);h.Fs(d.nativeRotation,4);h.Fs(d.position,3);h.Fs(d.rotation,4);h.Fs(d.target,3);h.I(d.reason);
          h.I(CyberpunkVR_PanzerWeaponAimDebugSeq.load()); }
        { const auto& d=CyberpunkVR_PanzerCannonDebug;h.I(int64_t(d.calls));h.I(int64_t(d.applied));h.F(PzRel(d.owner));h.I(int64_t(d.currentMainPoseId));
          h.Fs(d.position,3);h.Fs(d.nativeForward,3);h.Fs(d.forward,3);h.Fs(d.target,3);h.I(d.reason);h.I(CyberpunkVR_PanzerCannonDebugSeq.load()); }
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"panzer: steered %d aimed %d cannon %d\n",coverSteer,coverAim,coverCannon);
    CvrPanzerBind(0,0,0,0,0,0,false,0,0);CyberpunkVR_RuntimeDiagnostics=1;g_isDriving=false;g_menuModeValue=0;g_sceneTier=1;testOverlayVisible=false;
    testIsTank=false;testMountedVehicle=0;
}
// THE SETTINGS TRACE: vrport.ini parsed (src/Core/LiveControlsParse.cpp, or rust/crates/cvr-settings) from random
// files -- every key of the loop with every spelling of its separator, numbers, hex, signs, overflow, inf and nan,
// garbage, comments, upper case, longer keys, lines past fgets' 127 bytes, CRLF, the subsystems' keys -- over
// random seeds; every byte of the variables and the subsystems' lines, in order.
#include "settings_fields.inc"
void TraceSettings(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    char dir[MAX_PATH]{};GetTempPathA(MAX_PATH,dir);
    const std::string path=std::string(dir)+"cvr_settings_trace_"+std::to_string(GetCurrentProcessId())+".ini";
    static const char* const values[]={"0","1","-1","2","3","4","7","15","0x1F","1f","ff","FF","1.5","-0.25","0.0001","1e3","1e40","-1e40",
        "inf","-inf","nan","infinity","  42","3.25abc","abc","","+5","2147483648","-2147483649","99999999999","0.5,0.25,1","1,2,3",
        "0.1, 0.2 ,0.3","1.5e-3","-.5",".75","5.","0x","0x7fffffff","-0x10","1.25","360","400","-5","0.01","0.009","1.5"};
    static const char* const separators[]={"=","="," = ","="," =","= ","  =\t "," : ","=="};
    static const char* const subsystem[]={"xr_hud_element_crosshair=1","xr_framegen_mode=2","xr_foveated_inner=0.4","xr_vrs_rate=2",
        "xr_hidden_area_mesh=1","xr_vrui_scale=1.2","xr_mono_cascades=1"};
    int coverLines=0;
    for(int it=0;it<3000;++it) {
        std::string text;
        if(r.Chance(30))text+="\xEF\xBB\xBF";
        const int n=1+int(r.Next()%40);
        for(int k=0;k<n;++k) {
            std::string line;
            const auto lead=r.Next()%14;
            if(lead==0)line="  ";else if(lead==1)line="#";else if(lead==2)line="\t";else if(lead==3)line="; ";
            if(r.Chance(10)) { line+=subsystem[r.Next()%std::size(subsystem)]; }
            else {
                std::string key=kSettingKeys[r.Next()%std::size(kSettingKeys)];
                if(r.Chance(20))for(auto& ch:key)ch=char(std::toupper(static_cast<unsigned char>(ch)));
                if(r.Chance(30))key+=r.Chance(2) ? "_x":"2";
                line+=key+separators[r.Next()%std::size(separators)]+values[r.Next()%std::size(values)];
            }
            if(r.Chance(8))line+=" ; note";
            if(r.Chance(25))line+=std::string(100+r.Next()%80,'x');
            if(r.Chance(25)) { line+=" xr_snap_turn=1"; }
            if(r.Chance(200))line+=std::string(1,'\0')+"xr_hidden=1";
            text+=line+(r.Chance(5) ? "\r\n":"\n");
            if(r.Chance(15))text+="\n";
        }
        if(r.Chance(40))text+="\x1Axr_after_eof=1\n";
        if(r.Chance(20))text.pop_back();
        { FILE* f=nullptr;fopen_s(&f,path.c_str(),"wb");if(f) { std::fwrite(text.data(),1,text.size(),f);std::fclose(f); } }
        LiveControlsValues v;std::memset(&v,0,sizeof(v));RandomiseLiveControls(v,r);
        LiveControlsSubsystems s{};
        testSubsystemLines.clear();
#if CVR_RUST_VRIK
        const bool ok=cvr_settings_parse_live_controls(path.c_str(),&v,&s);
#else
        bool ok=false;
        if(FILE* f=_fsopen(path.c_str(),"r",_SH_DENYNO)) { ParseLiveControlsLines(f,v,s);std::fclose(f);ok=true; }
#endif
        h.B(ok);
        const auto* bytes=reinterpret_cast<const uint8_t*>(&v);
        for(size_t k=0;k<sizeof(v);k+=8) { uint64_t word=0;std::memcpy(&word,bytes+k,std::min<size_t>(8,sizeof(v)-k));h.I(int64_t(word)); }
        h.I(int64_t(testSubsystemLines.size()));
        for(const auto& [which,line]:testSubsystemLines) { h.I(which);for(const char ch:line)h.I(ch); }
        coverLines+=int(testSubsystemLines.size());
    }
    // A missing file changes nothing.
    {
        LiveControlsValues v;std::memset(&v,0,sizeof(v));RandomiseLiveControls(v,r);LiveControlsValues before=v;LiveControlsSubsystems s{};
        const std::string missing=path+".missing";
#if CVR_RUST_VRIK
        const bool ok=cvr_settings_parse_live_controls(missing.c_str(),&v,&s);
#else
        bool ok=false;
        if(FILE* f=_fsopen(missing.c_str(),"r",_SH_DENYNO)) { ParseLiveControlsLines(f,v,s);std::fclose(f);ok=true; }
#endif
        h.B(ok);h.B(std::memcmp(&v,&before,sizeof(v))==0);
    }
    DeleteFileA(path.c_str());
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"settings: %zu fields, %d subsystem lines\n",sizeof(LiveControlsValues),coverLines);
}
// THE MENU MODE TRACE: the menu mode hook (src/Hooks/MenuModeCallback.cpp, or its Rust port) over random states and
// modes, the lobby holding some of them up.
void TraceMenuMode(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    const int before=g_menuModeValue;
    static uint8_t states[4][0x200]{};
    for(int it=0;it<20000;++it) {
        testLobbyHolds.clear();if(r.Chance(3))testLobbyHolds.push_back(reinterpret_cast<uintptr_t>(states[r.Next()%4]));
        const int mode=r.Chance(3) ? 0:int(r.Next()%5)-1;
        void* state=r.Chance(20) ? nullptr:states[r.Next()%4];
        OnMenuModeHookCallback(state,mode);
        h.I(g_menuModeValue);
    }
    g_menuModeValue=before;
}
// THE LEFT-HANDED MIRROR TRACE: the animation controller's two doors (src/Hooks/LeftHandedMirror.cpp, or its Rust
// port) over fake originals -- the twins of the ranged graph and other names, the mirrored controller and others,
// item handlings and other features.
std::vector<uint64_t> g_lhCalls;uintptr_t g_lhBase=0;
uint64_t LhRel(uintptr_t p) { return p ? uint64_t(p-g_lhBase):~0ull; }
uint64_t __fastcall LhApply(uintptr_t controller,uint64_t name,void* feature) { g_lhCalls.insert(g_lhCalls.end(),{1,LhRel(controller),name,LhRel(reinterpret_cast<uintptr_t>(feature))});return name^0x55; }
uint64_t __fastcall LhPush(uintptr_t controller,uint64_t name) { g_lhCalls.insert(g_lhCalls.end(),{2,LhRel(controller),name});return name^0xAA; }
uint64_t LhHash(const char* s) { uint64_t h=0xCBF29CE484222325ull;for(;*s;++s)h=(h^uint8_t(*s))*0x100000001B3ull;return h; }
void TraceLeftHanded(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    cvr_left_handed_bind(reinterpret_cast<uintptr_t>(&LhApply),reinterpret_cast<uintptr_t>(&LhPush));
    static const char* const names[]={"RightWeaponData","LeftWeaponData","rightHandItemHandling","leftHandItemHandling","RightReloadData",
        "LeftReloadData","WeaponEquipType","LeftWeaponEquipType","RightReload","LeftReload","RightShoot","LeftShoot","RightReloadLoop",
        "LeftReloadLoop","EndRightReload","EndLeftReload","RightCycleRound","LeftCycleRound","CameraData","Locomotion","RightHandIK"};
    alignas(16) static uint8_t pool[0x400]{};g_lhBase=reinterpret_cast<uintptr_t>(pool);
    const uintptr_t controllers[3]={reinterpret_cast<uintptr_t>(pool),reinterpret_cast<uintptr_t>(pool+0x100),0};
    const uintptr_t features[3]={reinterpret_cast<uintptr_t>(pool+0x200),reinterpret_cast<uintptr_t>(pool+0x240),reinterpret_cast<uintptr_t>(pool+0x280)};
    testItemHandlings={{features[0],0x0302},{features[1],0x0105}};
    const auto before=CyberpunkVR_LeftHandedController.load();
    for(int it=0;it<20000;++it) {
        if(r.Chance(20))CyberpunkVR_LeftHandedController=r.Chance(4) ? 0:controllers[r.Next()%2];
        const uintptr_t controller=controllers[r.Next()%3];
        const uint64_t name=r.Chance(15) ? r.Next():LhHash(names[r.Next()%std::size(names)]);
        g_lhCalls.clear();
        uint64_t result;
        if(r.Chance(2)) { void* feature=r.Chance(5) ? nullptr:reinterpret_cast<void*>(features[r.Next()%3]);result=CvrLeftHandedApplyFeature(controller,name,feature); }
        else result=CvrLeftHandedPushEvent(controller,name);
        if(r.Chance(30))result^=cvr_left_handed_apply_feature(controller,name,nullptr);
        h.I(int64_t(result));h.I(int64_t(g_lhCalls.size()));for(const auto v:g_lhCalls)h.I(int64_t(v));
        h.I(int64_t(CyberpunkVR_LeftHandedNamesMirrored.load()));h.I(CyberpunkVR_LeftHandedNameLogCount.load());
        for(const auto& rec:CyberpunkVR_LeftHandedNameLog) { h.I(int64_t(rec.name));h.I(rec.info); }
    }
    CyberpunkVR_LeftHandedController=before;testItemHandlings.clear();
}
// THE STORY ATTENTION TRACE: the look-at ray (src/Hooks/StoryAttention.cpp, or its Rust port) and the manual clue
// over fake targeting users -- the published context and its 750 ms, the owner, entity and player, the reaching hand
// turning the player's own ray, the gameplay gates; the clue's arm, press, take and suspend on the tests' clock.
void TraceStoryAttention(TraceHash& h,uint64_t seed) {
    using namespace cvr::quest;
    TraceRng r(seed);
    alignas(16) static uint8_t pool[0x400]{};std::memset(pool,0,sizeof(pool));
    const auto at=[&](size_t off) { return reinterpret_cast<uintptr_t>(pool)+off; };
    const uintptr_t player=at(0x000),users[2]={at(0x100),at(0x200)},other=at(0x300);
    g_VRIKClockMs=RoomMs;cvr::roomscale::SetPlayer(player);RoomPut<uint64_t>(pool,0x48,0xABCD);
    const ObjectKey keys[3]={{player,at(0x80)},{other,at(0x90)},{player,at(0xA0)}};
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        g_menuModeValue=r.Chance(15);testVruiCaptures=r.Chance(15);testSessionRunning=!r.Chance(15);testDeviceCam=r.Chance(15);
        if(r.Chance(5)) {
            AttentionContext c{};c.player=keys[r.Next()%3];c.entity=r.Chance(4) ? 0x1234:0xABCD;c.stamp=r.Chance(8) ? 0:g_roomMs-r.Next()%900;
            c.mode=static_cast<Attention>(r.Next()%3);PublishAttention(c);
        }
        // the clue
        if(r.Chance(3)) h.B(UpdateManualClue(r.Chance(10) ? ObjectKey{}:keys[r.Next()%3],r.Chance(4) ? ObjectKey{}:keys[r.Next()%3]));
        if(r.Chance(2)) h.B(ConsumeManualClueButton(r.Chance(2),!r.Chance(6)));
        if(r.Chance(200)) SuspendManualClueInput();
        // the ray
        const uintptr_t user=users[r.Next()%2];
        RoomPut<uint64_t>(reinterpret_cast<uint8_t*>(user),8,r.Chance(4) ? 0x1234:0xABCD);
        const auto key=keys[r.Next()%3];RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(user),0x10,key.address);RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(user),0x18,key.reference);
        testReachOk=r.Chance(3);r.Vec(testReachPoint,20);
        float transform[8];r.Vec(transform,10);transform[3]=1;r.Quat(transform+4,3.1f,1.2f,.6f);if(r.Chance(20))transform[7]=4;
        CvrStoryAttentionAfter(user,transform);
        h.Fs(transform,8);
        for(const auto& v:CyberpunkVR_StoryAttentionCounters)h.I(int64_t(v.load()));h.I(int64_t(CyberpunkVR_ReachTurns.load()));
        g_roomMs+=r.Chance(30) ? 400+r.Next()%600:r.Next()%120;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"story: matched %llu workspot %llu pacifica %llu no-pose %llu clues %llu reach-turns %llu\n",
        (unsigned long long)CyberpunkVR_StoryAttentionCounters[0].load(),(unsigned long long)CyberpunkVR_StoryAttentionCounters[2].load(),
        (unsigned long long)CyberpunkVR_StoryAttentionCounters[1].load(),(unsigned long long)CyberpunkVR_StoryAttentionCounters[3].load(),
        (unsigned long long)CyberpunkVR_StoryAttentionCounters[4].load(),(unsigned long long)CyberpunkVR_ReachTurns.load());
    SuspendManualClueInput();PublishAttention({});g_VRIKClockMs=nullptr;cvr::roomscale::SetPlayer(0);
    g_menuModeValue=0;testVruiCaptures=false;testSessionRunning=true;testDeviceCam=false;testReachOk=false;CyberpunkVR_RuntimeDiagnostics=1;
}
// THE SURVEILLANCE TRACE: a taken-over camera or sniper nest turning with the head (src/Hooks/SurveillanceBody.cpp, or
// its Rust port) over fake components, owners and lenses -- the gates, the yaw limits, the head's deltas and their
// discontinuities, the sniper's alignment, the finish that drops the automatic rotation, the view yaw and the
// sniper's head follow, on the tests' clock.
void TraceSurveillance(TraceHash& h,uint64_t seed) {
    TraceRng r(seed);
    alignas(16) static uint8_t pool[0x1000]{};std::memset(pool,0,sizeof(pool));
    const auto at=[&](size_t off) { return reinterpret_cast<uintptr_t>(pool)+off; };
    const uintptr_t components[2]={at(0x000),at(0x200)},owners[2]={at(0x400),at(0x500)},roots[2]={at(0x600),at(0x700)},lens=at(0x800),features[2]={at(0x900),at(0x980)};
    g_VRIKClockMs=RoomMs;
    const auto writeQuat=[&](uintptr_t a) { float q[4];r.Quat(q,3.1f,.8f,.1f);if(r.Chance(30))q[3]=3;std::memcpy(reinterpret_cast<void*>(a),q,16); };
    int coverInjected=0;
    for(int it=0;it<30000;++it) {
        CyberpunkVR_RuntimeDiagnostics=r.Chance(6) ? 0:1;
        if(r.Chance(50)) { CyberpunkVR_SurveillanceBodyFollow=!r.Chance(8);CyberpunkVR_DeviceCamOrient=!r.Chance(8); }
        testDeviceCam=!r.Chance(10);g_menuModeValue=r.Chance(15);testVruiCaptures=r.Chance(15);
        g_lensComp=r.Chance(15) ? 0:lens;const int k=int(r.Next()%2);
        g_takeoverEntity=r.Chance(10) ? 0:owners[r.Chance(8) ? 1-k:k];
        g_remoteCamOn=r.Chance(4) ? 0:1;
        for(int j=0;j<2;++j) {
            SurveillanceControl c{};c.owner=r.Chance(15) ? 0:owners[j];c.entity=0x500+j;c.enabled=!r.Chance(10);c.kind=uint8_t(r.Chance(10) ? 0:1+(j==1 && r.Chance(2)));
            testSurveillanceControls[components[j]]=c;
            RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(components[j]),0xE0,r.Chance(10) ? 0:lens);
            RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(components[j]),0xC0,r.Chance(10) ? 0:features[j]);
            RoomPut<float>(reinterpret_cast<uint8_t*>(features[j]),0x40,r.Range(-200,200));
            const bool limited=!r.Chance(3);RoomPut<float>(reinterpret_cast<uint8_t*>(components[j]),0x110,limited ? r.Range(-90,0):0.f);
            RoomPut<float>(reinterpret_cast<uint8_t*>(components[j]),0x114,limited ? r.Range(0,90):0.f);
            RoomPut<uintptr_t>(reinterpret_cast<uint8_t*>(owners[j]),0xB0,r.Chance(15) ? 0:roots[j]);
            if(r.Chance(3))writeQuat(roots[j]+0xF0);
        }
        if(r.Chance(2))writeQuat(lens+0xF0);
        testCameraPoseOk=!r.Chance(8);TraceCameraPose(r,testCameraPoseFrame);if(r.Chance(30))testCameraPoseFrame.originSerial=0;
        if(r.Chance(4))testCameraPoseFrame.oriX=-testCameraPoseFrame.oriX;
        float delta[3];for(auto& v:delta)v=r.Range(-5,5);if(r.Chance(50))delta[1]=std::numeric_limits<float>::infinity();
        const uintptr_t component=components[k];
        const uint64_t injectedBefore=CyberpunkVR_SurveillanceBodyInjected.load();
        CvrSurveillanceInputAfter(component,r.Chance(50) ? nullptr:delta);
        coverInjected+=CyberpunkVR_SurveillanceBodyInjected.load()!=injectedBefore;
        h.Fs(delta,3);
        float automatic[3];for(auto& v:automatic)v=r.Range(-1,1);float finish[3];std::memcpy(finish,delta,12);
        if(r.Chance(5))finish[0]+=1;
        CvrSurveillanceFinishInput(r.Chance(8) ? components[1-k]:component,automatic,r.Chance(6) ? finish:delta);
        h.Fs(automatic,3);h.Fs(delta,3);h.Fs(finish,3);
        { float yaw=-99;const bool ok=cvr::camera::SurveillanceViewYaw(r.Chance(5) ? 0:lens,owners[r.Next()%2],testCameraPoseFrame.originSerial,r.Range(-3,3),yaw);h.B(ok);h.F(yaw); }
        h.B(cvr::camera::SniperHeadFollowActive());
        h.I(int64_t(CyberpunkVR_SurveillanceBodyUpdates.load()));h.I(int64_t(CyberpunkVR_SurveillanceBodyInjected.load()));h.I(int64_t(CyberpunkVR_SurveillanceAutoFiltered.load()));
        h.Fs(CyberpunkVR_SurveillanceBodyDebug,9);h.Fs(CyberpunkVR_SurveillanceAutoDebug,6);
        g_roomMs+=r.Chance(40) ? 300+r.Next()%600:r.Next()%40;g_roomUs=g_roomMs*1000;
    }
    if(std::getenv("CVR_TRACE_COVERAGE"))std::fprintf(stderr,"surveillance: updates %llu injected %d filtered %llu\n",
        (unsigned long long)CyberpunkVR_SurveillanceBodyUpdates.load(),coverInjected,(unsigned long long)CyberpunkVR_SurveillanceAutoFiltered.load());
    g_VRIKClockMs=nullptr;testDeviceCam=false;g_menuModeValue=0;testVruiCaptures=false;g_lensComp=0;g_takeoverEntity=0;g_remoteCamOn=0;
    testSurveillanceControls.clear();CyberpunkVR_SurveillanceBodyFollow=1;CyberpunkVR_DeviceCamOrient=1;CyberpunkVR_RuntimeDiagnostics=1;
}
void WaterTrace() {
    TraceHash h;TraceWater(h,0xC9E62C5D87A3B1F5ull);
    std::printf("water=%016llx/%llu\n",static_cast<unsigned long long>(h.h),static_cast<unsigned long long>(h.n));
    TraceHash c;TraceChord(c,0x5A17C0DE9E3779B9ull);
    std::printf("chord=%016llx/%llu\n",static_cast<unsigned long long>(c.h),static_cast<unsigned long long>(c.n));
    TraceHash p;TracePose(p,0x0DDC0FFEE1234567ull);
    std::printf("pose=%016llx/%llu\n",static_cast<unsigned long long>(p.h),static_cast<unsigned long long>(p.n));
    TraceHash k;TraceCamHooks(k,0x7E57CA4E5A11F00Dull);
    std::printf("camhooks=%016llx/%llu\n",static_cast<unsigned long long>(k.h),static_cast<unsigned long long>(k.n));
    TraceHash t;TracePoseTransport(t,0x7A2A5B0C3D4E5F61ull);
    std::printf("transport=%016llx/%llu\n",static_cast<unsigned long long>(t.h),static_cast<unsigned long long>(t.n));
    TraceHash pc;TracePatchCamera(pc,0x9A7C4CA3E7A11D0Bull);
    std::printf("patchcam=%016llx/%llu\n",static_cast<unsigned long long>(pc.h),static_cast<unsigned long long>(pc.n));
    TraceHash lc;TraceLocate(lc,0x10CA7EC0FFEE5EEDull);
    std::printf("locate=%016llx/%llu\n",static_cast<unsigned long long>(lc.h),static_cast<unsigned long long>(lc.n));
    TraceHash wa;TraceWeaponAim(wa,0x3EA90A1D5E7C2B41ull);
    std::printf("weaponaim=%016llx/%llu\n",static_cast<unsigned long long>(wa.h),static_cast<unsigned long long>(wa.n));
    TraceHash pz;TracePanzer(pz,0x7A2E1D6C5B4A3928ull);
    std::printf("panzer=%016llx/%llu\n",static_cast<unsigned long long>(pz.h),static_cast<unsigned long long>(pz.n));
    TraceHash st;TraceSettings(st,0x5E771265AB0C0DE5ull);
    std::printf("settings=%016llx/%llu\n",static_cast<unsigned long long>(st.h),static_cast<unsigned long long>(st.n));
    TraceHash mm;TraceMenuMode(mm,0x3E2D1C0B5A697887ull);
    std::printf("menumode=%016llx/%llu\n",static_cast<unsigned long long>(mm.h),static_cast<unsigned long long>(mm.n));
    TraceHash lh;TraceLeftHanded(lh,0x1EF7AD3D5A1C0B2Bull);
    std::printf("lefthanded=%016llx/%llu\n",static_cast<unsigned long long>(lh.h),static_cast<unsigned long long>(lh.n));
    TraceHash sa;TraceStoryAttention(sa,0x570A77E471011CEull);
    std::printf("story=%016llx/%llu\n",static_cast<unsigned long long>(sa.h),static_cast<unsigned long long>(sa.n));
    TraceHash sv;TraceSurveillance(sv,0x5E7E111A9CE1B0D1ull);
    std::printf("surveillance=%016llx/%llu\n",static_cast<unsigned long long>(sv.h),static_cast<unsigned long long>(sv.n));
}
#endif