// The registration table: which names the game's script VM can call, and what they point at.
//
// This was the last 850 lines of a 9,800-line file, and it is the one part of the natives that has to
// see ALL of them -- which is why it is its own translation unit with a generated header rather than
// living beside any one family.

#include "Natives/NativeFunctions.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Functions.hpp>

RED4EXT_C_EXPORT void RED4EXT_CALL PostRegisterTypes() {
    auto rtti = RED4ext::CRTTISystem::Get();
    RED4ext::CBaseFunction::Flags flags = {.isNative = true, .isStatic = true};

    auto f1 = RED4ext::CGlobalFunction::Create("GetLeftVRHandValid", "GetLeftVRHandValid", &GetLeftVRHandValid);
    f1->flags = flags; f1->SetReturnType("Bool"); rtti->RegisterFunction(f1);

    auto f2 = RED4ext::CGlobalFunction::Create("GetRightVRHandValid", "GetRightVRHandValid", &GetRightVRHandValid);
    f2->flags = flags; f2->SetReturnType("Bool"); rtti->RegisterFunction(f2);

    auto f3 = RED4ext::CGlobalFunction::Create("GetLeftVRHandPos", "GetLeftVRHandPos", &GetLeftVRHandPos);
    f3->flags = flags; f3->SetReturnType("Vector4"); rtti->RegisterFunction(f3);

    auto f4 = RED4ext::CGlobalFunction::Create("GetRightVRHandPos", "GetRightVRHandPos", &GetRightVRHandPos);
    f4->flags = flags; f4->SetReturnType("Vector4"); rtti->RegisterFunction(f4);

    auto f5 = RED4ext::CGlobalFunction::Create("GetLeftVRHandRot", "GetLeftVRHandRot", &GetLeftVRHandRot);
    f5->flags = flags; f5->SetReturnType("Quaternion"); rtti->RegisterFunction(f5);

    auto f6 = RED4ext::CGlobalFunction::Create("GetRightVRHandRot", "GetRightVRHandRot", &GetRightVRHandRot);
    f6->flags = flags; f6->SetReturnType("Quaternion"); rtti->RegisterFunction(f6);

    auto f7 = RED4ext::CGlobalFunction::Create("IsVRHandLinked", "IsVRHandLinked", &IsVRHandLinked);
    f7->flags = flags; f7->SetReturnType("Bool"); rtti->RegisterFunction(f7);

    auto f8 = RED4ext::CGlobalFunction::Create("ForceHideVRFppArms", "ForceHideVRFppArms", &ForceHideVRFppArms);
    f8->flags = flags; f8->SetReturnType("Int32"); rtti->RegisterFunction(f8);

    auto f8b = RED4ext::CGlobalFunction::Create("RestoreVRFppArms", "RestoreVRFppArms", &RestoreVRFppArms);
    f8b->flags = flags; f8b->SetReturnType("Int32"); rtti->RegisterFunction(f8b);

    auto f9 = RED4ext::CGlobalFunction::Create("SetVRRightHandEntity", "SetVRRightHandEntity", &SetVRRightHandEntity);
    f9->flags = flags; f9->AddParam("handle:IScriptable", "entity"); rtti->RegisterFunction(f9);

    auto f10 = RED4ext::CGlobalFunction::Create("DumpVRFppComponents", "DumpVRFppComponents", &DumpVRFppComponents);
    f10->flags = flags; f10->SetReturnType("Int32"); rtti->RegisterFunction(f10);

    auto f11 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugEnabled", "SetVRFppChunkDebugEnabled", &SetVRFppChunkDebugEnabled);
    f11->flags = flags; f11->AddParam("Int32", "enabled"); rtti->RegisterFunction(f11);

    auto f12 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugComponentIndex", "SetVRFppChunkDebugComponentIndex", &SetVRFppChunkDebugComponentIndex);
    f12->flags = flags; f12->AddParam("Int32", "index"); rtti->RegisterFunction(f12);

    auto f13 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugHand", "SetVRFppChunkDebugHand", &SetVRFppChunkDebugHand);
    f13->flags = flags; f13->AddParam("Int32", "hand"); rtti->RegisterFunction(f13);

    auto f14 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugBits", "SetVRFppChunkDebugBits", &SetVRFppChunkDebugBits);
    f14->flags = flags;
    f14->AddParam("Int32", "bit0");
    f14->AddParam("Int32", "bit1");
    f14->AddParam("Int32", "bit2");
    f14->AddParam("Int32", "bit3");
    rtti->RegisterFunction(f14);

    auto f15 = RED4ext::CGlobalFunction::Create("SetVRBoneDebugIndex", "SetVRBoneDebugIndex", &SetVRBoneDebugIndex);
    f15->flags = flags; f15->AddParam("Int32", "index"); rtti->RegisterFunction(f15);

    auto f15j = RED4ext::CGlobalFunction::Create("InstallVRAnimPoseHook", "InstallVRAnimPoseHook", &InstallVRAnimPoseHook);
    f15j->flags = flags; f15j->SetReturnType("Int32"); rtti->RegisterFunction(f15j);

    auto f15k = RED4ext::CGlobalFunction::Create("ArmVRAnimPosePlayer", "ArmVRAnimPosePlayer", &ArmVRAnimPosePlayer);
    f15k->flags = flags; f15k->SetReturnType("Int32"); rtti->RegisterFunction(f15k);

    auto f15m = RED4ext::CGlobalFunction::Create("GetVRAnimPoseStats", "GetVRAnimPoseStats", &GetVRAnimPoseStats);
    f15m->flags = flags; f15m->SetReturnType("Int32"); f15m->AddParam("Int32", "mode"); rtti->RegisterFunction(f15m);

    auto fRC = RED4ext::CGlobalFunction::Create("VRRemoteCamera", "VRRemoteCamera", &VRRemoteCamera);
    fRC->flags = flags; fRC->SetReturnType("Int32");
    fRC->AddParam("Int32", "active"); fRC->AddParam("Float", "x"); fRC->AddParam("Float", "y");
    fRC->AddParam("Float", "z"); rtti->RegisterFunction(fRC);

    auto fWG = RED4ext::CGlobalFunction::Create("VRWristGuard", "VRWristGuard", &VRWristGuard);
    fWG->flags = flags; fWG->SetReturnType("Int32"); fWG->AddParam("Int32", "mode"); rtti->RegisterFunction(fWG);

    auto f15o = RED4ext::CGlobalFunction::Create("DumpPlayerBoneNames", "DumpPlayerBoneNames", &DumpPlayerBoneNames);
    f15o->flags = flags; f15o->SetReturnType("Int32"); rtti->RegisterFunction(f15o);

    auto f15oSF = RED4ext::CGlobalFunction::Create("SetVRSmokeFingers", "SetVRSmokeFingers", &SetVRSmokeFingers);
    f15oSF->flags = flags; f15oSF->SetReturnType("Int32"); f15oSF->AddParam("Int32", "active"); rtti->RegisterFunction(f15oSF);

    auto f15oCF = RED4ext::CGlobalFunction::Create("VRSmokeCaptureFingers", "VRSmokeCaptureFingers", &VRSmokeCaptureFingers);
    f15oCF->flags = flags; f15oCF->SetReturnType("Int32"); rtti->RegisterFunction(f15oCF);

    auto f15oDF = RED4ext::CGlobalFunction::Create("VRSmokeDumpFingers", "VRSmokeDumpFingers", &VRSmokeDumpFingers);
    f15oDF->flags = flags; f15oDF->SetReturnType("Int32"); rtti->RegisterFunction(f15oDF);

    auto f15oCE = RED4ext::CGlobalFunction::Create("SetVRSmokeCig", "SetVRSmokeCig", &SetVRSmokeCig);
    f15oCE->flags = flags; f15oCE->SetReturnType("Int32"); f15oCE->AddParam("Int32", "enable"); rtti->RegisterFunction(f15oCE);

    auto f15oMD = RED4ext::CGlobalFunction::Create("VRSmokeMouthDist", "VRSmokeMouthDist", &VRSmokeMouthDist);
    f15oMD->flags = flags; f15oMD->SetReturnType("Float"); rtti->RegisterFunction(f15oMD);

    auto f15oMDL = RED4ext::CGlobalFunction::Create("VRSmokeMouthDistL", "VRSmokeMouthDistL", &VRSmokeMouthDistL);
    f15oMDL->flags = flags; f15oMDL->SetReturnType("Float"); rtti->RegisterFunction(f15oMDL);

    auto f15oAB = RED4ext::CGlobalFunction::Create("SetVRSmokeAnchorBone", "SetVRSmokeAnchorBone", &SetVRSmokeAnchorBone);
    f15oAB->flags = flags; f15oAB->SetReturnType("Int32"); f15oAB->AddParam("Int32","sel"); rtti->RegisterFunction(f15oAB);
    auto f15oMA = RED4ext::CGlobalFunction::Create("SetVRSmokeMouthAnchor", "SetVRSmokeMouthAnchor", &SetVRSmokeMouthAnchor);
    f15oMA->flags = flags; f15oMA->SetReturnType("Int32"); f15oMA->AddParam("Int32", "on"); rtti->RegisterFunction(f15oMA);

    auto f15oCS = RED4ext::CGlobalFunction::Create("SetVRSmokeCigScaleY", "SetVRSmokeCigScaleY", &SetVRSmokeCigScaleY);
    f15oCS->flags = flags; f15oCS->SetReturnType("Int32"); f15oCS->AddParam("Float", "y"); rtti->RegisterFunction(f15oCS);

    auto fVWP = RED4ext::CGlobalFunction::Create("VRViewWorldPos", "VRViewWorldPos", &VRViewWorldPos);
    fVWP->flags = flags; fVWP->SetReturnType("Vector4"); rtti->RegisterFunction(fVWP);
    auto fVWR = RED4ext::CGlobalFunction::Create("VRViewWorldRot", "VRViewWorldRot", &VRViewWorldRot);
    fVWR->flags = flags; fVWR->SetReturnType("Quaternion"); rtti->RegisterFunction(fVWR);
    auto fPW = RED4ext::CGlobalFunction::Create("VRPalmWorldPos", "VRPalmWorldPos", &VRPalmWorldPos);
    fPW->flags = flags; fPW->SetReturnType("Vector4"); fPW->AddParam("Int32", "right");
    rtti->RegisterFunction(fPW);
    auto fPCL = RED4ext::CGlobalFunction::Create("VRPalmCamLocal", "VRPalmCamLocal", &VRPalmCamLocal);
    fPCL->flags = flags; fPCL->SetReturnType("Vector4"); fPCL->AddParam("Int32", "right");
    rtti->RegisterFunction(fPCL);
    auto fPMP = RED4ext::CGlobalFunction::Create("VRPalmModelPos", "VRPalmModelPos", &VRPalmModelPos);
    fPMP->flags = flags; fPMP->SetReturnType("Vector4"); fPMP->AddParam("Int32", "right");
    rtti->RegisterFunction(fPMP);
    auto fBB = RED4ext::CGlobalFunction::Create("VRBodyBonePos", "VRBodyBonePos", &VRBodyBonePos);
    fBB->flags = flags; fBB->SetReturnType("Vector4"); fBB->AddParam("Int32", "slot");
    rtti->RegisterFunction(fBB);
    // Any bone of the solved rig by index -- the channel the capsule geometry gets measured on.
    auto fBSC = RED4ext::CGlobalFunction::Create("VRBoneSnapCount", "VRBoneSnapCount", &VRBoneSnapCount);
    fBSC->flags = flags; fBSC->SetReturnType("Int32"); rtti->RegisterFunction(fBSC);
    auto fBMP = RED4ext::CGlobalFunction::Create("VRBoneModelPos", "VRBoneModelPos", &VRBoneModelPos);
    fBMP->flags = flags; fBMP->SetReturnType("Vector4"); fBMP->AddParam("Int32", "idx");
    rtti->RegisterFunction(fBMP);
    auto fBMR = RED4ext::CGlobalFunction::Create("VRBoneModelRot", "VRBoneModelRot", &VRBoneModelRot);
    fBMR->flags = flags; fBMR->SetReturnType("Quaternion"); fBMR->AddParam("Int32", "idx");
    rtti->RegisterFunction(fBMR);
    auto fCMP = RED4ext::CGlobalFunction::Create("VRCamModelPos", "VRCamModelPos", &VRCamModelPos);
    fCMP->flags = flags; fCMP->SetReturnType("Vector4"); rtti->RegisterFunction(fCMP);
    auto fPEP = RED4ext::CGlobalFunction::Create("VRPlayerEnginePos", "VRPlayerEnginePos", &VRPlayerEnginePos);
    fPEP->flags = flags; fPEP->SetReturnType("Vector4"); rtti->RegisterFunction(fPEP);
    auto fPER = RED4ext::CGlobalFunction::Create("VRPlayerEngineRot", "VRPlayerEngineRot", &VRPlayerEngineRot);
    fPER->flags = flags; fPER->SetReturnType("Quaternion"); rtti->RegisterFunction(fPER);
    auto fDBF = RED4ext::CGlobalFunction::Create("VRDumpBodyFuncs", "VRDumpBodyFuncs", &VRDumpBodyFuncs);
    fDBF->flags = flags; fDBF->SetReturnType("String"); rtti->RegisterFunction(fDBF);
    auto fPB = RED4ext::CGlobalFunction::Create("VRProbeBody", "VRProbeBody", &VRProbeBody);
    fPB->flags = flags; fPB->SetReturnType("String"); fPB->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fPB);
    // The velocity API the engine has all along -- see VRBodyFunc for why it is unreachable from
    // redscript and CET, and reachable from here.
    auto fBP = RED4ext::CGlobalFunction::Create("VRBodyProbe", "VRBodyProbe", &VRBodyProbe);
    fBP->flags = flags; fBP->SetReturnType("String"); fBP->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBP);
    auto fBGV = RED4ext::CGlobalFunction::Create("VRBodyGetVel", "VRBodyGetVel", &VRBodyGetVel);
    fBGV->flags = flags; fBGV->SetReturnType("Vector4"); fBGV->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBGV);
    auto fBGA = RED4ext::CGlobalFunction::Create("VRBodyGetAngVel", "VRBodyGetAngVel", &VRBodyGetAngVel);
    fBGA->flags = flags; fBGA->SetReturnType("Vector4"); fBGA->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBGA);
    auto fBSV = RED4ext::CGlobalFunction::Create("VRBodySetVel", "VRBodySetVel", &VRBodySetVel);
    fBSV->flags = flags; fBSV->SetReturnType("Bool");
    fBSV->AddParam("handle:IScriptable", "body"); fBSV->AddParam("Vector4", "v");
    rtti->RegisterFunction(fBSV);
    auto fBSA = RED4ext::CGlobalFunction::Create("VRBodySetAngVel", "VRBodySetAngVel", &VRBodySetAngVel);
    fBSA->flags = flags; fBSA->SetReturnType("Bool");
    fBSA->AddParam("handle:IScriptable", "body"); fBSA->AddParam("Vector4", "v");
    rtti->RegisterFunction(fBSA);
    auto fBD = RED4ext::CGlobalFunction::Create("VRBodyGetDims", "VRBodyGetDims", &VRBodyGetDims);
    fBD->flags = flags; fBD->SetReturnType("Vector4"); fBD->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBD);
    auto fBC = RED4ext::CGlobalFunction::Create("VRBodyGetCOM", "VRBodyGetCOM", &VRBodyGetCOM);
    fBC->flags = flags; fBC->SetReturnType("Vector4"); fBC->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBC);
    auto fHS = RED4ext::CGlobalFunction::Create("VRHandStop", "VRHandStop", &VRHandStop);
    fHS->flags = flags; fHS->SetReturnType("Bool");
    fHS->AddParam("Int32", "right"); fHS->AddParam("Bool", "active"); fHS->AddParam("Vector4", "worldPos");
    rtti->RegisterFunction(fHS);
    auto fW2M = RED4ext::CGlobalFunction::Create("VRWorldToModel", "VRWorldToModel", &VRWorldToModel);
    fW2M->flags = flags; fW2M->SetReturnType("Vector4"); fW2M->AddParam("Vector4", "worldPos");
    rtti->RegisterFunction(fW2M);
    auto fW2MD = RED4ext::CGlobalFunction::Create("VRWorldDirToModel", "VRWorldDirToModel", &VRWorldDirToModel);
    fW2MD->flags = flags; fW2MD->SetReturnType("Vector4"); fW2MD->AddParam("Vector4", "worldDir");
    rtti->RegisterFunction(fW2MD);
    auto fPR = RED4ext::CGlobalFunction::Create("VRPairs", "VRPairs", &VRPairs);
    fPR->flags = flags; fPR->SetReturnType("Int32");
    fPR->AddParam("Int32", "mode"); fPR->AddParam("Int32", "field");
    rtti->RegisterFunction(fPR);
    auto fPU = RED4ext::CGlobalFunction::Create("VRPairUse", "VRPairUse", &VRPairUse);
    fPU->flags = flags; fPU->SetReturnType("Int32");
    fPU->AddParam("Int32", "index"); fPU->AddParam("Int32", "which");
    rtti->RegisterFunction(fPU);
    auto fRS = RED4ext::CGlobalFunction::Create("VRRigStatus", "VRRigStatus", &VRRigStatus);
    fRS->flags = flags; fRS->SetReturnType("Int32"); fRS->AddParam("Int32", "field");
    rtti->RegisterFunction(fRS);
    auto fRW = RED4ext::CGlobalFunction::Create("VRRigWrite", "VRRigWrite", &VRRigWrite);
    fRW->flags = flags; fRW->SetReturnType("Int32");
    fRW->AddParam("Int32", "which"); fRW->AddParam("Int32", "bone");
    fRW->AddParam("Float", "x"); fRW->AddParam("Float", "y"); fRW->AddParam("Float", "z");
    rtti->RegisterFunction(fRW);
    auto fRWR = RED4ext::CGlobalFunction::Create("VRRigWriteRot", "VRRigWriteRot", &VRRigWriteRot);
    fRWR->flags = flags; fRWR->SetReturnType("Int32");
    fRWR->AddParam("Int32", "which"); fRWR->AddParam("Int32", "bone"); fRWR->AddParam("Float", "angle");
    fRWR->AddParam("Float", "ax"); fRWR->AddParam("Float", "ay"); fRWR->AddParam("Float", "az");
    rtti->RegisterFunction(fRWR);
    auto fRWS = RED4ext::CGlobalFunction::Create("VRRigWriteScale", "VRRigWriteScale", &VRRigWriteScale);
    fRWS->flags = flags; fRWS->SetReturnType("Int32");
    fRWS->AddParam("Int32", "which"); fRWS->AddParam("Int32", "bone"); fRWS->AddParam("Float", "scale");
    rtti->RegisterFunction(fRWS);
    auto fThC = RED4ext::CGlobalFunction::Create("VRTwoHandCapture", "VRTwoHandCapture", &VRTwoHandCapture);
    fThC->flags = flags; fThC->SetReturnType("Int32"); rtti->RegisterFunction(fThC);
    auto fThS = RED4ext::CGlobalFunction::Create("VRTwoHandStatus", "VRTwoHandStatus", &VRTwoHandStatus);
    fThS->flags = flags; fThS->SetReturnType("Int32"); rtti->RegisterFunction(fThS);
    auto fRstC = RED4ext::CGlobalFunction::Create("VRRestFingerCapture", "VRRestFingerCapture", &VRRestFingerCapture);
    fRstC->flags = flags; fRstC->SetReturnType("Int32"); rtti->RegisterFunction(fRstC);
    auto fRstS = RED4ext::CGlobalFunction::Create("VRRestFingerStatus", "VRRestFingerStatus", &VRRestFingerStatus);
    fRstS->flags = flags; fRstS->SetReturnType("Int32"); rtti->RegisterFunction(fRstS);
    auto fRstA = RED4ext::CGlobalFunction::Create("VRRestFingerApply", "VRRestFingerApply", &VRRestFingerApply);
    fRstA->flags = flags; fRstA->SetReturnType("Int32"); fRstA->AddParam("Int32", "on");
    rtti->RegisterFunction(fRstA);
    auto fRFC = RED4ext::CGlobalFunction::Create("VRReloadFingerClear", "VRReloadFingerClear", &VRReloadFingerClear);
    fRFC->flags = flags; fRFC->SetReturnType("Int32"); fRFC->AddParam("Int32", "hand");
    rtti->RegisterFunction(fRFC);
    auto fRFS = RED4ext::CGlobalFunction::Create("VRReloadFingerSet", "VRReloadFingerSet", &VRReloadFingerSet);
    fRFS->flags = flags; fRFS->SetReturnType("Int32");
    fRFS->AddParam("Int32", "hand"); fRFS->AddParam("String", "bone");
    fRFS->AddParam("Float", "qx"); fRFS->AddParam("Float", "qy"); fRFS->AddParam("Float", "qz"); fRFS->AddParam("Float", "qw");
    rtti->RegisterFunction(fRFS);
    auto fRFA = RED4ext::CGlobalFunction::Create("VRReloadFingerApply", "VRReloadFingerApply", &VRReloadFingerApply);
    fRFA->flags = flags; fRFA->SetReturnType("Int32"); fRFA->AddParam("Int32", "hand"); fRFA->AddParam("Int32", "on");
    rtti->RegisterFunction(fRFA);
    auto fRFB = RED4ext::CGlobalFunction::Create("VRReloadFingerBlend", "VRReloadFingerBlend", &VRReloadFingerBlend);
    fRFB->flags = flags; fRFB->SetReturnType("Int32"); fRFB->AddParam("Int32", "hand"); fRFB->AddParam("Float", "alpha");
    rtti->RegisterFunction(fRFB);
    auto fRRS = RED4ext::CGlobalFunction::Create("VRRigReset", "VRRigReset", &VRRigReset);
    fRRS->flags = flags; fRRS->SetReturnType("Bool");
    rtti->RegisterFunction(fRRS);
    auto fRWA = RED4ext::CGlobalFunction::Create("VRRigWriteAbs", "VRRigWriteAbs", &VRRigWriteAbs);
    fRWA->flags = flags; fRWA->SetReturnType("Int32");
    fRWA->AddParam("Int32", "which"); fRWA->AddParam("Int32", "bone");
    fRWA->AddParam("Float", "x"); fRWA->AddParam("Float", "y"); fRWA->AddParam("Float", "z");
    fRWA->AddParam("Float", "qi"); fRWA->AddParam("Float", "qj"); fRWA->AddParam("Float", "qk"); fRWA->AddParam("Float", "qr");
    fRWA->AddParam("Int32", "abs");
    rtti->RegisterFunction(fRWA);
    auto fRRB = RED4ext::CGlobalFunction::Create("VRRigBone", "VRRigBone", &VRRigBone);
    fRRB->flags = flags; fRRB->SetReturnType("Float");
    fRRB->AddParam("Int32", "which"); fRRB->AddParam("Int32", "bone"); fRRB->AddParam("Int32", "field");
    rtti->RegisterFunction(fRRB);
    auto fRSG = RED4ext::CGlobalFunction::Create("VRRigSignature", "VRRigSignature", &VRRigSignature);
    fRSG->flags = flags; fRSG->SetReturnType("Int32");
    fRSG->AddParam("Int32", "which"); fRSG->AddParam("Int32", "bones");
    fRSG->AddParam("Int32", "i0"); fRSG->AddParam("String", "n0");
    fRSG->AddParam("Int32", "i1"); fRSG->AddParam("String", "n1");
    fRSG->AddParam("Int32", "i2"); fRSG->AddParam("String", "n2");
    fRSG->AddParam("Int32", "i3"); fRSG->AddParam("String", "n3");
    rtti->RegisterFunction(fRSG);
    // NO WEAPON IS NAMED HERE. Signatures come from Lua -- reload/rigs.lua, registered by both the reload module and
    // the recorder -- because a weapon is data, not code. The table starts empty and the first VRRigSignature call
    // fills it; until then no rig is identified, which is the honest state rather than a guess.
    auto fRTR = RED4ext::CGlobalFunction::Create("VRRigTrack", "VRRigTrack", &VRRigTrack);
    fRTR->flags = flags; fRTR->SetReturnType("Float");
    fRTR->AddParam("Int32", "which"); fRTR->AddParam("Int32", "idx");
    rtti->RegisterFunction(fRTR);
    auto fRTW = RED4ext::CGlobalFunction::Create("VRRigTrackWrite", "VRRigTrackWrite", &VRRigTrackWrite);
    fRTW->flags = flags; fRTW->SetReturnType("Int32");
    fRTW->AddParam("Int32", "which"); fRTW->AddParam("Int32", "idx");
    fRTW->AddParam("Float", "value"); fRTW->AddParam("Int32", "on");
    rtti->RegisterFunction(fRTW);
    auto fRFK = RED4ext::CGlobalFunction::Create("VRRecordFK", "VRRecordFK", &VRRecordFK);
    fRFK->flags = flags; fRFK->SetReturnType("Int32"); fRFK->AddParam("Int32", "on");
    rtti->RegisterFunction(fRFK);
    auto fRWO = RED4ext::CGlobalFunction::Create("VRRigWriteOff", "VRRigWriteOff", &VRRigWriteOff);
    fRWO->flags = flags; fRWO->SetReturnType("Int32");
    fRWO->AddParam("Int32", "which"); fRWO->AddParam("Int32", "bone");
    rtti->RegisterFunction(fRWO);
    auto fRWC = RED4ext::CGlobalFunction::Create("VRRigWriteClear", "VRRigWriteClear", &VRRigWriteClear);
    fRWC->flags = flags; fRWC->SetReturnType("Bool");
    rtti->RegisterFunction(fRWC);
    auto fRWD = RED4ext::CGlobalFunction::Create("VRRigWriteDiag", "VRRigWriteDiag", &VRRigWriteDiag);
    fRWD->flags = flags; fRWD->SetReturnType("Int32");
    fRWD->AddParam("Int32", "slot"); fRWD->AddParam("Int32", "field");
    rtti->RegisterFunction(fRWD);
    auto fSR = RED4ext::CGlobalFunction::Create("VRSmallRig", "VRSmallRig", &VRSmallRig);
    fSR->flags = flags; fSR->SetReturnType("Int32");
    fSR->AddParam("Int32", "mode"); fSR->AddParam("Int32", "field");
    rtti->RegisterFunction(fSR);
    auto fSRU = RED4ext::CGlobalFunction::Create("VRSmallRigUse", "VRSmallRigUse", &VRSmallRigUse);
    fSRU->flags = flags; fSRU->SetReturnType("Int32"); fSRU->AddParam("Int32", "index");
    rtti->RegisterFunction(fSRU);
    auto fPC = RED4ext::CGlobalFunction::Create("VRPoseCensus", "VRPoseCensus", &VRPoseCensus);
    fPC->flags = flags; fPC->SetReturnType("Int32");
    fPC->AddParam("Int32", "mode"); fPC->AddParam("Int32", "field");
    rtti->RegisterFunction(fPC);
    auto fWRU = RED4ext::CGlobalFunction::Create("VRWeaponRigUse", "VRWeaponRigUse", &VRWeaponRigUse);
    fWRU->flags = flags; fWRU->SetReturnType("Int32"); fWRU->AddParam("Int32", "index");
    rtti->RegisterFunction(fWRU);
    auto fWSI = RED4ext::CGlobalFunction::Create("VRWeaponPartSetIdx", "VRWeaponPartSetIdx", &VRWeaponPartSetIdx);
    fWSI->flags = flags; fWSI->SetReturnType("Bool");
    fWSI->AddParam("Int32", "slot"); fWSI->AddParam("Int32", "bone");
    rtti->RegisterFunction(fWSI);
    auto fWPO = RED4ext::CGlobalFunction::Create("VRWeaponPartOffset", "VRWeaponPartOffset", &VRWeaponPartOffset);
    fWPO->flags = flags; fWPO->SetReturnType("Bool");
    fWPO->AddParam("Int32", "slot");
    fWPO->AddParam("Float", "x"); fWPO->AddParam("Float", "y"); fWPO->AddParam("Float", "z");
    rtti->RegisterFunction(fWPO);
    auto fWRA = RED4ext::CGlobalFunction::Create("VRWeaponRigArm", "VRWeaponRigArm", &VRWeaponRigArm);
    fWRA->flags = flags; fWRA->SetReturnType("Int32"); fWRA->AddParam("handle:IScriptable", "weapon");
    rtti->RegisterFunction(fWRA);
    auto fWPL = RED4ext::CGlobalFunction::Create("VRWeaponPartLocal", "VRWeaponPartLocal", &VRWeaponPartLocal);
    fWPL->flags = flags; fWPL->SetReturnType("Vector4"); fWPL->AddParam("Int32", "slot");
    rtti->RegisterFunction(fWPL);
    auto fWRN = RED4ext::CGlobalFunction::Create("VRWeaponRigNames", "VRWeaponRigNames", &VRWeaponRigNames);
    fWRN->flags = flags; fWRN->SetReturnType("String");
    rtti->RegisterFunction(fWRN);
    auto fWRS = RED4ext::CGlobalFunction::Create("VRWeaponRigStatus", "VRWeaponRigStatus", &VRWeaponRigStatus);
    fWRS->flags = flags; fWRS->SetReturnType("Int32"); fWRS->AddParam("Int32", "slot");
    rtti->RegisterFunction(fWRS);
    auto fHRM = RED4ext::CGlobalFunction::Create("VRHandRawModel", "VRHandRawModel", &VRHandRawModel);
    fHRM->flags = flags; fHRM->SetReturnType("Vector4"); fHRM->AddParam("Int32", "right");
    rtti->RegisterFunction(fHRM);
    auto fHRR = RED4ext::CGlobalFunction::Create("VRHandRawRot", "VRHandRawRot", &VRHandRawRot);
    fHRR->flags = flags; fHRR->SetReturnType("Quaternion"); fHRR->AddParam("Int32", "right");
    rtti->RegisterFunction(fHRR);
    auto fHSM = RED4ext::CGlobalFunction::Create("VRHandStopModel", "VRHandStopModel", &VRHandStopModel);
    fHSM->flags = flags; fHSM->SetReturnType("Bool");
    fHSM->AddParam("Int32", "right"); fHSM->AddParam("Bool", "active"); fHSM->AddParam("Vector4", "modelPos");
    rtti->RegisterFunction(fHSM);
    auto fHSR = RED4ext::CGlobalFunction::Create("VRHandStopRot", "VRHandStopRot", &VRHandStopRot);
    fHSR->flags = flags; fHSR->SetReturnType("Bool");
    fHSR->AddParam("Int32", "side"); fHSR->AddParam("Int32", "active");
    fHSR->AddParam("Float", "qi"); fHSR->AddParam("Float", "qj"); fHSR->AddParam("Float", "qk"); fHSR->AddParam("Float", "qr");
    rtti->RegisterFunction(fHSR);
    auto fHRW = RED4ext::CGlobalFunction::Create("VRHandRawWorld", "VRHandRawWorld", &VRHandRawWorld);
    fHRW->flags = flags; fHRW->SetReturnType("Vector4"); fHRW->AddParam("Int32", "right");
    rtti->RegisterFunction(fHRW);
    auto fHSD = RED4ext::CGlobalFunction::Create("VRHandStopDeadband", "VRHandStopDeadband", &VRHandStopDeadband);
    fHSD->flags = flags; fHSD->SetReturnType("Bool"); fHSD->AddParam("Float", "metres");
    rtti->RegisterFunction(fHSD);
    auto fBSK = RED4ext::CGlobalFunction::Create("VRBodySetKinematic", "VRBodySetKinematic", &VRBodySetKinematic);
    fBSK->flags = flags; fBSK->SetReturnType("Bool");
    fBSK->AddParam("handle:IScriptable", "body"); fBSK->AddParam("Bool", "kinematic");
    rtti->RegisterFunction(fBSK);
    auto fBIK = RED4ext::CGlobalFunction::Create("VRBodyIsKinematic", "VRBodyIsKinematic", &VRBodyIsKinematic);
    fBIK->flags = flags; fBIK->SetReturnType("Int32"); fBIK->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBIK);
    auto fBGP = RED4ext::CGlobalFunction::Create("VRBodyGetPos", "VRBodyGetPos", &VRBodyGetPos);
    fBGP->flags = flags; fBGP->SetReturnType("Vector4"); fBGP->AddParam("handle:IScriptable", "body");
    rtti->RegisterFunction(fBGP);
    auto fBSP = RED4ext::CGlobalFunction::Create("VRBodySetPos", "VRBodySetPos", &VRBodySetPos);
    fBSP->flags = flags; fBSP->SetReturnType("Bool");
    fBSP->AddParam("handle:IScriptable", "body");
    fBSP->AddParam("Vector4", "position"); fBSP->AddParam("Quaternion", "orientation");
    rtti->RegisterFunction(fBSP);
    auto fBSF = RED4ext::CGlobalFunction::Create("VRBodySetSimMasks", "VRBodySetSimMasks", &VRBodySetSimMasks);
    fBSF->flags = flags; fBSF->SetReturnType("Bool");
    fBSF->AddParam("handle:IScriptable", "body");
    fBSF->AddParam("Uint64", "mask1"); fBSF->AddParam("Uint64", "mask2");
    rtti->RegisterFunction(fBSF);
    auto fPMR = RED4ext::CGlobalFunction::Create("VRPalmModelRot", "VRPalmModelRot", &VRPalmModelRot);
    fPMR->flags = flags; fPMR->SetReturnType("Quaternion"); fPMR->AddParam("Int32", "right");
    rtti->RegisterFunction(fPMR);

    auto f15oMWP = RED4ext::CGlobalFunction::Create("VRSmokeMouthWorldPos", "VRSmokeMouthWorldPos", &VRSmokeMouthWorldPos);
    f15oMWP->flags = flags; f15oMWP->SetReturnType("Vector4"); rtti->RegisterFunction(f15oMWP);
    auto f15oMWF = RED4ext::CGlobalFunction::Create("VRSmokeMouthWorldRot", "VRSmokeMouthWorldRot", &VRSmokeMouthWorldRot);
    f15oMWF->flags = flags; f15oMWF->SetReturnType("Quaternion"); rtti->RegisterFunction(f15oMWF);
    auto f15oSO = RED4ext::CGlobalFunction::Create("SetVRSmokeSmokeOffset", "SetVRSmokeSmokeOffset", &SetVRSmokeSmokeOffset);
    f15oSO->flags = flags; f15oSO->SetReturnType("Int32");
    f15oSO->AddParam("Float","x"); f15oSO->AddParam("Float","y"); f15oSO->AddParam("Float","z");
    f15oSO->AddParam("Float","pitch"); f15oSO->AddParam("Float","yaw"); f15oSO->AddParam("Float","roll");
    rtti->RegisterFunction(f15oSO);

    auto f15oCC = RED4ext::CGlobalFunction::Create("SetVRSmokeCigChunks", "SetVRSmokeCigChunks", &SetVRSmokeCigChunks);
    f15oCC->flags = flags; f15oCC->SetReturnType("Int32");
    f15oCC->AddParam("handle:GameObject", "cig"); f15oCC->AddParam("Int32", "count");
    rtti->RegisterFunction(f15oCC);

    auto f15oVS = RED4ext::CGlobalFunction::Create("SetVRSmokeCigVisualScale", "SetVRSmokeCigVisualScale", &SetVRSmokeCigVisualScale);
    f15oVS->flags = flags; f15oVS->SetReturnType("Int32");
    f15oVS->AddParam("handle:GameObject", "cig"); f15oVS->AddParam("Float", "y");
    rtti->RegisterFunction(f15oVS);

    auto f15oVG = RED4ext::CGlobalFunction::Create("GetVRSmokeCigVisualScaleY", "GetVRSmokeCigVisualScaleY", &GetVRSmokeCigVisualScaleY);
    f15oVG->flags = flags; f15oVG->SetReturnType("Float");
    f15oVG->AddParam("handle:GameObject", "cig");
    rtti->RegisterFunction(f15oVG);

    auto f15oMO = RED4ext::CGlobalFunction::Create("SetVRSmokeMouthOffset", "SetVRSmokeMouthOffset", &SetVRSmokeMouthOffset);
    f15oMO->flags = flags; f15oMO->SetReturnType("Int32");
    f15oMO->AddParam("Float", "x"); f15oMO->AddParam("Float", "y"); f15oMO->AddParam("Float", "z");
    f15oMO->AddParam("Float", "pitch"); f15oMO->AddParam("Float", "yaw"); f15oMO->AddParam("Float", "roll");
    rtti->RegisterFunction(f15oMO);

    auto f15oCO = RED4ext::CGlobalFunction::Create("SetVRSmokeCigOffset", "SetVRSmokeCigOffset", &SetVRSmokeCigOffset);
    f15oCO->flags = flags; f15oCO->SetReturnType("Int32");
    f15oCO->AddParam("Float", "x"); f15oCO->AddParam("Float", "y"); f15oCO->AddParam("Float", "z");
    f15oCO->AddParam("Float", "pitch"); f15oCO->AddParam("Float", "yaw"); f15oCO->AddParam("Float", "roll");
    rtti->RegisterFunction(f15oCO);

    auto f15oLF = RED4ext::CGlobalFunction::Create("SetVRSmokeFingersL", "SetVRSmokeFingersL", &SetVRSmokeFingersL);
    f15oLF->flags = flags; f15oLF->SetReturnType("Int32"); f15oLF->AddParam("Int32", "active"); rtti->RegisterFunction(f15oLF);

    auto f15oLCig = RED4ext::CGlobalFunction::Create("SetVRSmokeLeftCig", "SetVRSmokeLeftCig", &SetVRSmokeLeftCig);
    f15oLCig->flags = flags; f15oLCig->SetReturnType("Int32"); f15oLCig->AddParam("Int32", "on"); rtti->RegisterFunction(f15oLCig);

    auto f15oLC = RED4ext::CGlobalFunction::Create("VRSmokeCaptureFingersL", "VRSmokeCaptureFingersL", &VRSmokeCaptureFingersL);
    f15oLC->flags = flags; f15oLC->SetReturnType("Int32"); rtti->RegisterFunction(f15oLC);

    auto f15oLE = RED4ext::CGlobalFunction::Create("SetVRSmokeLighter", "SetVRSmokeLighter", &SetVRSmokeLighter);
    f15oLE->flags = flags; f15oLE->SetReturnType("Int32"); f15oLE->AddParam("Int32", "enable"); rtti->RegisterFunction(f15oLE);

    auto f15oLO = RED4ext::CGlobalFunction::Create("SetVRSmokeLighterOffset", "SetVRSmokeLighterOffset", &SetVRSmokeLighterOffset);
    f15oLO->flags = flags; f15oLO->SetReturnType("Int32");
    f15oLO->AddParam("Float", "x"); f15oLO->AddParam("Float", "y"); f15oLO->AddParam("Float", "z");
    f15oLO->AddParam("Float", "pitch"); f15oLO->AddParam("Float", "yaw"); f15oLO->AddParam("Float", "roll");
    rtti->RegisterFunction(f15oLO);

    auto f15oTF = RED4ext::CGlobalFunction::Create("SetVRSmokeThumbFlickL", "SetVRSmokeThumbFlickL", &SetVRSmokeThumbFlickL);
    f15oTF->flags = flags; f15oTF->SetReturnType("Int32");
    f15oTF->AddParam("Float", "pitch"); f15oTF->AddParam("Float", "yaw"); f15oTF->AddParam("Float", "roll");
    rtti->RegisterFunction(f15oTF);

    auto f15oTP = RED4ext::CGlobalFunction::Create("SetVRSmokeThumbPressL", "SetVRSmokeThumbPressL", &SetVRSmokeThumbPressL);
    f15oTP->flags = flags; f15oTP->SetReturnType("Int32"); f15oTP->AddParam("Float", "amount"); rtti->RegisterFunction(f15oTP);

    auto f15p = RED4ext::CGlobalFunction::Create("SetVRBindMode", "SetVRBindMode", &SetVRBindMode);
    f15p->flags = flags; f15p->SetReturnType("Int32"); f15p->AddParam("Int32", "mode"); rtti->RegisterFunction(f15p);

    auto f15q = RED4ext::CGlobalFunction::Create("SetVRBindParams", "SetVRBindParams", &SetVRBindParams);
    f15q->flags = flags; f15q->SetReturnType("Int32"); 
    f15q->AddParam("Float", "scale"); f15q->AddParam("Float", "x"); f15q->AddParam("Float", "y"); f15q->AddParam("Float", "z"); f15q->AddParam("Int32", "axis"); f15q->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15q);

    auto f15qE = RED4ext::CGlobalFunction::Create("SetVRElbowPole", "SetVRElbowPole", &SetVRElbowPole);
    f15qE->flags = flags; f15qE->SetReturnType("Int32");
    f15qE->AddParam("Float", "angle"); f15qE->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15qE);

    auto f15qS = RED4ext::CGlobalFunction::Create("SetVRElbowSwing", "SetVRElbowSwing", &SetVRElbowSwing);
    f15qS->flags = flags; f15qS->SetReturnType("Int32");
    f15qS->AddParam("Float", "gain"); f15qS->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15qS);

    auto f15qO = RED4ext::CGlobalFunction::Create("SetVRHandOffset", "SetVRHandOffset", &SetVRHandOffset);
    f15qO->flags = flags; f15qO->SetReturnType("Int32");
    f15qO->AddParam("Float", "pitch"); f15qO->AddParam("Float", "yaw"); f15qO->AddParam("Float", "roll"); f15qO->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15qO);

    auto f15r = RED4ext::CGlobalFunction::Create("SetVRBindBones", "SetVRBindBones", &SetVRBindBones);
    f15r->flags = flags; f15r->SetReturnType("Int32"); 
    f15r->AddParam("Int32", "leftIdx"); f15r->AddParam("Int32", "rightIdx");
    rtti->RegisterFunction(f15r);

    auto f15rH = RED4ext::CGlobalFunction::Create("SetVRHeadBone", "SetVRHeadBone", &SetVRHeadBone);
    f15rH->flags = flags; f15rH->SetReturnType("Int32"); f15rH->AddParam("Int32", "index"); rtti->RegisterFunction(f15rH);

    auto f15rR = RED4ext::CGlobalFunction::Create("SetVRUseHeadRelative", "SetVRUseHeadRelative", &SetVRUseHeadRelative);
    f15rR->flags = flags; f15rR->SetReturnType("Int32"); f15rR->AddParam("Int32", "on"); rtti->RegisterFunction(f15rR);

    auto f15rC = RED4ext::CGlobalFunction::Create("SetVRDiagCapture", "SetVRDiagCapture", &SetVRDiagCapture);
    f15rC->flags = flags; f15rC->SetReturnType("Int32"); f15rC->AddParam("Int32", "on"); rtti->RegisterFunction(f15rC);

    auto f15rD = RED4ext::CGlobalFunction::Create("LogVRDiag", "LogVRDiag", &LogVRDiag);
    f15rD->flags = flags; f15rD->SetReturnType("Int32");
    f15rD->AddParam("Float", "camX"); f15rD->AddParam("Float", "camY"); f15rD->AddParam("Float", "camZ");
    f15rD->AddParam("Float", "qi"); f15rD->AddParam("Float", "qj"); f15rD->AddParam("Float", "qk"); f15rD->AddParam("Float", "qr");
    rtti->RegisterFunction(f15rD);



    auto f15s = RED4ext::CGlobalFunction::Create("SetVRPlayerYaw", "SetVRPlayerYaw", &SetVRPlayerYaw);
    f15s->flags = flags; f15s->SetReturnType("Int32");
    f15s->AddParam("Float", "yaw");
    f15s->AddParam("Float", "ci"); f15s->AddParam("Float", "cj");
    f15s->AddParam("Float", "ck"); f15s->AddParam("Float", "cr");
    f15s->AddParam("Float", "camX"); f15s->AddParam("Float", "camY"); f15s->AddParam("Float", "camZ");
    f15s->AddParam("Float", "entX"); f15s->AddParam("Float", "entY"); f15s->AddParam("Float", "entZ");
    f15s->AddParam("Float", "eqi"); f15s->AddParam("Float", "eqj");
    f15s->AddParam("Float", "eqk"); f15s->AddParam("Float", "eqr");
    rtti->RegisterFunction(f15s);

    auto f19 = RED4ext::CGlobalFunction::Create("DumpAnimVTable", "DumpAnimVTable", &DumpAnimVTable);
    f19->flags = flags; f19->SetReturnType("Int32"); rtti->RegisterFunction(f19);

    auto f20 = RED4ext::CGlobalFunction::Create("DumpAnimMemory", "DumpAnimMemory", &DumpAnimMemory);
    f20->flags = flags; f20->SetReturnType("Int32"); rtti->RegisterFunction(f20);

    auto f21 = RED4ext::CGlobalFunction::Create("DumpAnimControllerComponents", "DumpAnimControllerComponents", &DumpAnimControllerComponents);
    f21->flags = flags; f21->SetReturnType("Int32"); rtti->RegisterFunction(f21);

    auto f22 = RED4ext::CGlobalFunction::Create("DumpRuntimeClassFunctions", "DumpRuntimeClassFunctions", &DumpRuntimeClassFunctions);
    f22->flags = flags; f22->SetReturnType("Int32"); rtti->RegisterFunction(f22);

    auto f23 = RED4ext::CGlobalFunction::Create("SetVRIKAnimInputTestMode", "SetVRIKAnimInputTestMode", &SetVRIKAnimInputTestMode);
    f23->flags = flags; f23->AddParam("Int32", "mode"); rtti->RegisterFunction(f23);

    auto f24 = RED4ext::CGlobalFunction::Create("UpdateVRIKAnimInputs", "UpdateVRIKAnimInputs", &UpdateVRIKAnimInputs);
    f24->flags = flags; f24->SetReturnType("Int32"); rtti->RegisterFunction(f24);

    auto f25 = RED4ext::CGlobalFunction::Create("DumpAnimControllerFunctionDetails", "DumpAnimControllerFunctionDetails", &DumpAnimControllerFunctionDetails);
    f25->flags = flags; f25->SetReturnType("Int32"); rtti->RegisterFunction(f25);

    auto f26 = RED4ext::CGlobalFunction::Create("DumpAnimControllerListeners", "DumpAnimControllerListeners", &DumpAnimControllerListeners);
    f26->flags = flags; f26->SetReturnType("Int32"); rtti->RegisterFunction(f26);

    auto f27 = RED4ext::CGlobalFunction::Create("DumpInterestingAnimClassProperties", "DumpInterestingAnimClassProperties", &DumpInterestingAnimClassProperties);
    f27->flags = flags; f27->SetReturnType("Int32"); rtti->RegisterFunction(f27);

    auto f28 = RED4ext::CGlobalFunction::Create("DumpAnimationSystemCandidates", "DumpAnimationSystemCandidates", &DumpAnimationSystemCandidates);
    f28->flags = flags; f28->SetReturnType("Int32"); rtti->RegisterFunction(f28);

    auto f29 = RED4ext::CGlobalFunction::Create("RunIKTargetAddTest", "RunIKTargetAddTest", &RunIKTargetAddTest);
    f29->flags = flags; f29->SetReturnType("Int32"); f29->AddParam("Int32", "mode"); rtti->RegisterFunction(f29);

    auto f30 = RED4ext::CGlobalFunction::Create("TestAnimFloatInput", "TestAnimFloatInput", &TestAnimFloatInput);
    f30->flags = flags; f30->SetReturnType("Int32");
    f30->AddParam("CName", "inputName");
    f30->AddParam("Float", "value");
    f30->AddParam("Int32", "route");
    rtti->RegisterFunction(f30);

    auto f31 = RED4ext::CGlobalFunction::Create("TestAnimFloatPreset", "TestAnimFloatPreset", &TestAnimFloatPreset);
    f31->flags = flags; f31->SetReturnType("Int32");
    f31->AddParam("Int32", "mode");
    f31->AddParam("Float", "value");
    f31->AddParam("Int32", "route");
    rtti->RegisterFunction(f31);

    auto f32 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameter", "SetPlayerAnimParameter", &SetPlayerAnimParameter);
    f32->flags = flags; f32->SetReturnType("Int32");
    f32->AddParam("CName", "inputName");
    f32->AddParam("Float", "value");
    rtti->RegisterFunction(f32);

    auto f33 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameterPreset", "SetPlayerAnimParameterPreset", &SetPlayerAnimParameterPreset);
    f33->flags = flags; f33->SetReturnType("Int32");
    f33->AddParam("Int32", "mode");
    f33->AddParam("Float", "value");
    rtti->RegisterFunction(f33);

    auto f34 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameterPersistentPreset", "SetPlayerAnimParameterPersistentPreset", &SetPlayerAnimParameterPersistentPreset);
    f34->flags = flags; f34->SetReturnType("Int32");
    f34->AddParam("Int32", "mode");
    f34->AddParam("Float", "value");
    rtti->RegisterFunction(f34);

    auto f35 = RED4ext::CGlobalFunction::Create("GetPlayerAnimParameterPersistentLastResult", "GetPlayerAnimParameterPersistentLastResult", &GetPlayerAnimParameterPersistentLastResult);
    f35->flags = flags; f35->SetReturnType("Int32");
    rtti->RegisterFunction(f35);

    auto f36 = RED4ext::CGlobalFunction::Create("TestWeaponUserFeatureRoute", "TestWeaponUserFeatureRoute", &TestWeaponUserFeatureRoute);
    f36->flags = flags; f36->SetReturnType("Int32");
    f36->AddParam("Int32", "route");
    rtti->RegisterFunction(f36);

    auto f37 = RED4ext::CGlobalFunction::Create("TestIKFeatureRoute", "TestIKFeatureRoute", &TestIKFeatureRoute);
    f37->flags = flags; f37->SetReturnType("Int32");
    f37->AddParam("Int32", "mode");
    f37->AddParam("Int32", "route");
    rtti->RegisterFunction(f37);

    auto f38 = RED4ext::CGlobalFunction::Create("TestMeleeIKDataFeatureRoute", "TestMeleeIKDataFeatureRoute", &TestMeleeIKDataFeatureRoute);
    f38->flags = flags; f38->SetReturnType("Int32");
    f38->AddParam("Int32", "route");
    rtti->RegisterFunction(f38);

    auto f39 = RED4ext::CGlobalFunction::Create("DumpRootGraphVariables", "DumpRootGraphVariables", &DumpRootGraphVariables);
    f39->flags = flags; f39->SetReturnType("Int32");
    rtti->RegisterFunction(f39);

    auto f40 = RED4ext::CGlobalFunction::Create("SetRootGraphBoolVariable", "SetRootGraphBoolVariable", &SetRootGraphBoolVariableByName);
    f40->flags = flags; f40->SetReturnType("Int32");
    f40->AddParam("CName", "name");
    f40->AddParam("Bool", "value");
    rtti->RegisterFunction(f40);

    auto f41 = RED4ext::CGlobalFunction::Create("SetRootGraphFloatVariable", "SetRootGraphFloatVariable", &SetRootGraphFloatVariableByName);
    f41->flags = flags; f41->SetReturnType("Int32");
    f41->AddParam("CName", "name");
    f41->AddParam("Float", "value");
    rtti->RegisterFunction(f41);

    auto f42 = RED4ext::CGlobalFunction::Create("SetRootGraphVectorVariable", "SetRootGraphVectorVariable", &SetRootGraphVectorVariableByName);
    f42->flags = flags; f42->SetReturnType("Int32");
    f42->AddParam("CName", "name");
    f42->AddParam("Vector4", "value");
    rtti->RegisterFunction(f42);

    auto f43 = RED4ext::CGlobalFunction::Create("TestRootGraphFloatPreset", "TestRootGraphFloatPreset", &TestRootGraphFloatPreset);
    f43->flags = flags; f43->SetReturnType("Int32");
    f43->AddParam("Int32", "mode");
    f43->AddParam("Float", "value");
    rtti->RegisterFunction(f43);

    auto f44 = RED4ext::CGlobalFunction::Create("TestRootGraphVectorPreset", "TestRootGraphVectorPreset", &TestRootGraphVectorPreset);
    f44->flags = flags; f44->SetReturnType("Int32");
    f44->AddParam("Int32", "mode");
    f44->AddParam("Vector4", "value");
    rtti->RegisterFunction(f44);

    auto f45 = RED4ext::CGlobalFunction::Create("SetRootGraphFloatPresetPersistent", "SetRootGraphFloatPresetPersistent", &SetRootGraphFloatPresetPersistent);
    f45->flags = flags; f45->SetReturnType("Int32");
    f45->AddParam("Int32", "mode");
    f45->AddParam("Float", "value");
    rtti->RegisterFunction(f45);

    auto f46 = RED4ext::CGlobalFunction::Create("SetRootGraphVectorPresetPersistent", "SetRootGraphVectorPresetPersistent", &SetRootGraphVectorPresetPersistent);
    f46->flags = flags; f46->SetReturnType("Int32");
    f46->AddParam("Int32", "mode");
    f46->AddParam("Vector4", "value");
    rtti->RegisterFunction(f46);

    auto f47 = RED4ext::CGlobalFunction::Create("GetRootGraphPersistentLastResult", "GetRootGraphPersistentLastResult", &GetRootGraphPersistentLastResult);
    f47->flags = flags; f47->SetReturnType("Int32");
    rtti->RegisterFunction(f47);

    auto f48 = RED4ext::CGlobalFunction::Create("DumpPlayerAnimatedObjectRuntime", "DumpPlayerAnimatedObjectRuntime", &DumpPlayerAnimatedObjectRuntime);
    f48->flags = flags; f48->SetReturnType("Int32");
    rtti->RegisterFunction(f48);

    auto f49 = RED4ext::CGlobalFunction::Create("DumpAnimationSystemLookup", "DumpAnimationSystemLookup", &DumpAnimationSystemLookup);
    f49->flags = flags; f49->SetReturnType("Int32");
    rtti->RegisterFunction(f49);

    auto f50 = RED4ext::CGlobalFunction::Create("DumpRootMetaRigTracks", "DumpRootMetaRigTracks", &DumpRootMetaRigTracks);
    f50->flags = flags; f50->SetReturnType("Int32");
    rtti->RegisterFunction(f50);

    auto f51 = RED4ext::CGlobalFunction::Create("TestRootMetaRigTrackPreset", "TestRootMetaRigTrackPreset", &TestRootMetaRigTrackPreset);
    f51->flags = flags; f51->SetReturnType("Int32");
    f51->AddParam("Int32", "mode");
    f51->AddParam("Float", "value");
    rtti->RegisterFunction(f51);

    auto f52 = RED4ext::CGlobalFunction::Create("SetRootMetaRigTrackPresetPersistent", "SetRootMetaRigTrackPresetPersistent", &SetRootMetaRigTrackPresetPersistent);
    f52->flags = flags; f52->SetReturnType("Int32");
    f52->AddParam("Int32", "mode");
    f52->AddParam("Float", "value");
    rtti->RegisterFunction(f52);

    auto f53 = RED4ext::CGlobalFunction::Create("GetRootMetaRigTrackPersistentLastResult", "GetRootMetaRigTrackPersistentLastResult", &GetRootMetaRigTrackPersistentLastResult);
    f53->flags = flags; f53->SetReturnType("Int32");
    rtti->RegisterFunction(f53);

    auto f54 = RED4ext::CGlobalFunction::Create("DumpRootAnimatedObjectFloatArrayCandidates", "DumpRootAnimatedObjectFloatArrayCandidates", &DumpRootAnimatedObjectFloatArrayCandidates);
    f54->flags = flags; f54->SetReturnType("Int32");
    rtti->RegisterFunction(f54);

    auto f55 = RED4ext::CGlobalFunction::Create("TestRootLiveTrackPreset", "TestRootLiveTrackPreset", &TestRootLiveTrackPreset);
    f55->flags = flags; f55->SetReturnType("Int32");
    f55->AddParam("Int32", "mode");
    f55->AddParam("Float", "value");
    f55->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f55);

    auto f56 = RED4ext::CGlobalFunction::Create("SetRootLiveTrackPresetPersistent", "SetRootLiveTrackPresetPersistent", &SetRootLiveTrackPresetPersistent);
    f56->flags = flags; f56->SetReturnType("Int32");
    f56->AddParam("Int32", "mode");
    f56->AddParam("Float", "value");
    f56->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f56);

    auto f57 = RED4ext::CGlobalFunction::Create("GetRootLiveTrackPersistentLastResult", "GetRootLiveTrackPersistentLastResult", &GetRootLiveTrackPersistentLastResult);
    f57->flags = flags; f57->SetReturnType("Int32");
    rtti->RegisterFunction(f57);

    auto f58 = RED4ext::CGlobalFunction::Create("ReadRootLiveTrackPreset", "ReadRootLiveTrackPreset", &ReadRootLiveTrackPreset);
    f58->flags = flags; f58->SetReturnType("Int32");
    f58->AddParam("Int32", "mode");
    f58->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f58);

    // Weapon-aim native hook (M1 instrumentation).
    auto f59 = RED4ext::CGlobalFunction::Create("InstallWeaponAimHook", "InstallWeaponAimHook", &InstallWeaponAimHook);
    f59->flags = flags; f59->SetReturnType("Int32"); rtti->RegisterFunction(f59);

    auto f60 = RED4ext::CGlobalFunction::Create("DumpWeaponAimHookStats", "DumpWeaponAimHookStats", &DumpWeaponAimHookStats);
    f60->flags = flags; f60->SetReturnType("Int32"); rtti->RegisterFunction(f60);

    auto fProjRtti = RED4ext::CGlobalFunction::Create("DumpVRProjectileRtti", "DumpVRProjectileRtti", &DumpVRProjectileRtti);
    fProjRtti->flags = flags; fProjRtti->SetReturnType("Int32"); rtti->RegisterFunction(fProjRtti);

    auto fPaInstall = RED4ext::CGlobalFunction::Create("InstallVRPrepareAttack", "InstallVRPrepareAttack", &InstallVRPrepareAttack);
    fPaInstall->flags = flags; fPaInstall->SetReturnType("Int32"); rtti->RegisterFunction(fPaInstall);
    auto fPaSwap = RED4ext::CGlobalFunction::Create("SetVRPrepareAttackSwap", "SetVRPrepareAttackSwap", &SetVRPrepareAttackSwap);
    fPaSwap->flags = flags; fPaSwap->AddParam("Int32", "on"); rtti->RegisterFunction(fPaSwap);
    auto fPaDump = RED4ext::CGlobalFunction::Create("GetVRPADump", "GetVRPADump", &GetVRPADump);
    fPaDump->flags = flags; fPaDump->AddParam("Int32", "idx"); fPaDump->SetReturnType("Float"); rtti->RegisterFunction(fPaDump);

    auto fProjFind = RED4ext::CGlobalFunction::Create("DumpVRLiveProjectile", "DumpVRLiveProjectile", &DumpVRLiveProjectile);
    fProjFind->flags = flags; fProjFind->SetReturnType("Int32"); rtti->RegisterFunction(fProjFind);
    auto fProjSteer = RED4ext::CGlobalFunction::Create("SetVRProjSteer", "SetVRProjSteer", &SetVRProjSteer);
    fProjSteer->flags = flags; fProjSteer->AddParam("Int32", "on"); rtti->RegisterFunction(fProjSteer);
    auto fProjTick = RED4ext::CGlobalFunction::Create("VRProjSteerTick", "VRProjSteerTick", &VRProjSteerTick);
    fProjTick->flags = flags; rtti->RegisterFunction(fProjTick);
    auto fProjLD = RED4ext::CGlobalFunction::Create("GetVRProjLiveDump", "GetVRProjLiveDump", &GetVRProjLiveDump);
    fProjLD->flags = flags; fProjLD->AddParam("Int32", "idx"); fProjLD->SetReturnType("Float"); rtti->RegisterFunction(fProjLD);

    auto fProvInst = RED4ext::CGlobalFunction::Create("InstallVRProvInstrument", "InstallVRProvInstrument", &InstallVRProvInstrument);
    fProvInst->flags = flags; fProvInst->SetReturnType("Int32"); rtti->RegisterFunction(fProvInst);
    auto fProvOvr = RED4ext::CGlobalFunction::Create("SetVRProvOverrideSlot", "SetVRProvOverrideSlot", &SetVRProvOverrideSlot);
    fProvOvr->flags = flags; fProvOvr->AddParam("Int32", "slot"); rtti->RegisterFunction(fProvOvr);
    auto fProvDump = RED4ext::CGlobalFunction::Create("GetVRProvDump", "GetVRProvDump", &GetVRProvDump);
    fProvDump->flags = flags; fProvDump->AddParam("Int32", "idx"); fProvDump->SetReturnType("Float"); rtti->RegisterFunction(fProvDump);
    auto fProvReset = RED4ext::CGlobalFunction::Create("ResetVRProvCounts", "ResetVRProvCounts", &ResetVRProvCounts);
    fProvReset->flags = flags; rtti->RegisterFunction(fProvReset);
    auto fProvQM = RED4ext::CGlobalFunction::Create("SetVRProvQuatMode", "SetVRProvQuatMode", &SetVRProvQuatMode);
    fProvQM->flags = flags; fProvQM->AddParam("Int32", "mode"); fProvQM->AddParam("Int32", "axis"); rtti->RegisterFunction(fProvQM);
    auto fBarrelRay = RED4ext::CGlobalFunction::Create(
        "SetVRBarrelRayHit", "SetVRBarrelRayHit", &SetVRBarrelRayHit);
    fBarrelRay->flags = flags;
    fBarrelRay->AddParam("Float", "x");
    fBarrelRay->AddParam("Float", "y");
    fBarrelRay->AddParam("Float", "z");
    fBarrelRay->AddParam("Int32", "valid");
    fBarrelRay->AddParam("Int32", "mainVisible");
    fBarrelRay->AddParam("Int32", "secondVisible");
    rtti->RegisterFunction(fBarrelRay);
    auto fNpcSurface = RED4ext::CGlobalFunction::Create(
        "QueryVRNpcHitSurface", "QueryVRNpcHitSurface", &QueryVRNpcHitSurface);
    fNpcSurface->flags = flags;
    fNpcSurface->AddParam("handle:GameObject", "entity");
    fNpcSurface->AddParam("Vector4", "from");
    fNpcSurface->AddParam("Vector4", "to");
    fNpcSurface->SetReturnType("Vector4");
    rtti->RegisterFunction(fNpcSurface);
    auto fMuzP = RED4ext::CGlobalFunction::Create("SetVRMuzzlePos", "SetVRMuzzlePos", &SetVRMuzzlePos);
    fMuzP->flags = flags;
    fMuzP->AddParam("Float", "x"); fMuzP->AddParam("Float", "y"); fMuzP->AddParam("Float", "z");
    rtti->RegisterFunction(fMuzP);

    auto fMuz = RED4ext::CGlobalFunction::Create("SetVRMuzzleQuat", "SetVRMuzzleQuat", &SetVRMuzzleQuat);
    fMuz->flags = flags; fMuz->AddParam("Float","i"); fMuz->AddParam("Float","j"); fMuz->AddParam("Float","k"); fMuz->AddParam("Float","r"); rtti->RegisterFunction(fMuz);
    auto fWName = RED4ext::CGlobalFunction::Create("SetVRWeaponName", "SetVRWeaponName", &SetVRWeaponName);
    fWName->flags = flags; fWName->AddParam("String", "name"); rtti->RegisterFunction(fWName);
    auto fKick = RED4ext::CGlobalFunction::Create("SetVRWeaponKick", "SetVRWeaponKick", &SetVRWeaponKick);
    fKick->flags = flags; fKick->AddParam("Float","kick"); rtti->RegisterFunction(fKick);
    auto fZoom = RED4ext::CGlobalFunction::Create("SetVRZoomLevel", "SetVRZoomLevel", &SetVRZoomLevel);
    fZoom->flags = flags; fZoom->AddParam("Float","zoom"); rtti->RegisterFunction(fZoom);
    auto fReloadOwnedHand = RED4ext::CGlobalFunction::Create(
        "SetVRReloadOwnedHand", "SetVRReloadOwnedHand", &SetVRReloadOwnedHand);
    fReloadOwnedHand->flags = flags; fReloadOwnedHand->AddParam("Int32", "hand");
    rtti->RegisterFunction(fReloadOwnedHand);
    auto fSprintActive = RED4ext::CGlobalFunction::Create(
        "SetVRSprintActive", "SetVRSprintActive", &SetVRSprintActive);
    fSprintActive->flags = flags; fSprintActive->SetReturnType("Int32");
    fSprintActive->AddParam("Int32", "active");
    rtti->RegisterFunction(fSprintActive);
    auto fLocoState = RED4ext::CGlobalFunction::Create(
        "SetVRLocomotionState", "SetVRLocomotionState", &SetVRLocomotionState);
    fLocoState->flags = flags; fLocoState->SetReturnType("Int32");
    fLocoState->AddParam("Int32", "state");
    rtti->RegisterFunction(fLocoState);
    auto fWeaponPoseState = RED4ext::CGlobalFunction::Create(
        "SetVRWeaponPoseState", "SetVRWeaponPoseState", &SetVRWeaponPoseState);
    fWeaponPoseState->flags = flags; fWeaponPoseState->SetReturnType("Int32");
    fWeaponPoseState->AddParam("Int32", "weaponState");
    fWeaponPoseState->AddParam("Float", "aimInRemaining");
    rtti->RegisterFunction(fWeaponPoseState);
    auto fWeaponRaise = RED4ext::CGlobalFunction::Create(
        "SetVRWeaponRaiseTransition", "SetVRWeaponRaiseTransition", &SetVRWeaponRaiseTransition);
    fWeaponRaise->flags = flags; fWeaponRaise->SetReturnType("Int32");
    fWeaponRaise->AddParam("Int32", "active");
    rtti->RegisterFunction(fWeaponRaise);
    auto fMeleeFire = RED4ext::CGlobalFunction::Create("SetVRMeleeFire", "SetVRMeleeFire", &SetVRMeleeFire);
    fMeleeFire->flags = flags; fMeleeFire->AddParam("Int32","fire"); rtti->RegisterFunction(fMeleeFire);
    auto fTrgMode = RED4ext::CGlobalFunction::Create("SetVRTriggerMode", "SetVRTriggerMode", &SetVRTriggerMode);
    fTrgMode->flags = flags; fTrgMode->AddParam("Int32","mode"); rtti->RegisterFunction(fTrgMode);
    auto fCamFreeze = RED4ext::CGlobalFunction::Create("SetVRCamBoneFreeze", "SetVRCamBoneFreeze", &SetVRCamBoneFreeze);
    fCamFreeze->flags = flags; fCamFreeze->AddParam("Int32","on"); rtti->RegisterFunction(fCamFreeze);
    auto fPairSlew = RED4ext::CGlobalFunction::Create("SetVRPairSlew", "SetVRPairSlew", &SetVRPairSlew);
    fPairSlew->flags = flags; fPairSlew->AddParam("Float","rate"); rtti->RegisterFunction(fPairSlew);
    auto fPairLead = RED4ext::CGlobalFunction::Create("SetVRPairLead", "SetVRPairLead", &SetVRPairLead);
    fPairLead->flags = flags; fPairLead->AddParam("Float","ticks"); rtti->RegisterFunction(fPairLead);
    auto fCamAck = RED4ext::CGlobalFunction::Create("SetVRCamAck", "SetVRCamAck", &SetVRCamAck);
    fCamAck->flags = flags; fCamAck->AddParam("Float","seq"); rtti->RegisterFunction(fCamAck);
    auto fMeleeTrig = RED4ext::CGlobalFunction::Create("GetVRMeleeTrigger", "GetVRMeleeTrigger", &GetVRMeleeTrigger);
    fMeleeTrig->flags = flags; fMeleeTrig->SetReturnType("Int32"); rtti->RegisterFunction(fMeleeTrig);
    // Generic shared-slot getter (for the hand-to-holster CET mod + other VR mods that need raw poses).
    auto fGetSlot = RED4ext::CGlobalFunction::Create("GetVRSharedSlot", "GetVRSharedSlot", &GetVRSharedSlot);
    fGetSlot->flags = flags; fGetSlot->AddParam("Int32", "idx"); fGetSlot->SetReturnType("Float");
    rtti->RegisterFunction(fGetSlot);

    auto f61 = RED4ext::CGlobalFunction::Create("SetVRWeaponAim", "SetVRWeaponAim", &SetVRWeaponAim);
    f61->flags = flags; f61->SetReturnType("Int32");
    f61->AddParam("Float", "fx"); f61->AddParam("Float", "fy"); f61->AddParam("Float", "fz");
    f61->AddParam("Float", "px"); f61->AddParam("Float", "py"); f61->AddParam("Float", "pz");
    f61->AddParam("Int32", "enable"); f61->AddParam("Int32", "mode"); f61->AddParam("Float", "gate");
    rtti->RegisterFunction(f61);

    auto f61b = RED4ext::CGlobalFunction::Create("GetVRWeaponAim", "GetVRWeaponAim", &GetVRWeaponAim);
    f61b->flags = flags;
    f61b->SetReturnType("Int32");
    rtti->RegisterFunction(f61b);

    auto f62 = RED4ext::CGlobalFunction::Create("GetWeaponAimStat", "GetWeaponAimStat", &GetWeaponAimStat);
    f62->flags = flags; f62->SetReturnType("Int32"); f62->AddParam("Int32", "which");
    rtti->RegisterFunction(f62);

    auto f63 = RED4ext::CGlobalFunction::Create("SetVRHeadingTest", "SetVRHeadingTest", &SetVRHeadingTest);
    f63->flags = flags; f63->SetReturnType("Int32");
    f63->AddParam("Int32", "force"); f63->AddParam("Float", "yaw"); f63->AddParam("Float", "pitch");
    rtti->RegisterFunction(f63);

    auto f64 = RED4ext::CGlobalFunction::Create("SetVRShotCamera", "SetVRShotCamera", &SetVRShotCamera);
    f64->flags = flags; f64->AddParam("handle:IScriptable", "cam"); rtti->RegisterFunction(f64);

    auto f65 = RED4ext::CGlobalFunction::Create("SetVRShotSnap", "SetVRShotSnap", &SetVRShotSnap);
    f65->flags = flags; f65->SetReturnType("Int32");
    f65->AddParam("Int32", "enable"); f65->AddParam("Int32", "mode"); f65->AddParam("Float", "testYaw");
    rtti->RegisterFunction(f65);

    auto f66 = RED4ext::CGlobalFunction::Create("StartVRCamTrace", "StartVRCamTrace", &StartVRCamTrace);
    f66->flags = flags; f66->SetReturnType("Int32"); f66->AddParam("Int32", "offsetSel"); f66->AddParam("Int32", "gated"); f66->AddParam("Int32", "writeOnly");
    rtti->RegisterFunction(f66);
    auto f67 = RED4ext::CGlobalFunction::Create("StopVRCamTrace", "StopVRCamTrace", &StopVRCamTrace);
    f67->flags = flags; f67->SetReturnType("Int32"); rtti->RegisterFunction(f67);
    auto f68 = RED4ext::CGlobalFunction::Create("DumpVRCamTrace", "DumpVRCamTrace", &DumpVRCamTrace);
    f68->flags = flags; f68->SetReturnType("Int32"); rtti->RegisterFunction(f68);

    auto f70 = RED4ext::CGlobalFunction::Create("DumpVRCamAddr", "DumpVRCamAddr", &DumpVRCamAddr);
    f70->flags = flags; f70->SetReturnType("Int32"); rtti->RegisterFunction(f70);

    auto f71 = RED4ext::CGlobalFunction::Create("SetVRGetOrient", "SetVRGetOrient", &SetVRGetOrient);
    f71->flags = flags; f71->SetReturnType("Int32"); f71->AddParam("Int32", "mode"); f71->AddParam("Float", "testYaw"); f71->AddParam("Int32", "plane");
    rtti->RegisterFunction(f71);

    auto f72 = RED4ext::CGlobalFunction::Create("SetVRSkipHmdTest", "SetVRSkipHmdTest", &SetVRSkipHmdTest);
    f72->flags = flags; f72->SetReturnType("Int32"); f72->AddParam("Int32", "mode"); rtti->RegisterFunction(f72);

    auto fMenu = RED4ext::CGlobalFunction::Create("SetVRMenuOpen", "SetVRMenuOpen", &SetVRMenuOpen);
    auto fDevScr = RED4ext::CGlobalFunction::Create("SetVRDeviceScreen", "SetVRDeviceScreen", &SetVRDeviceScreen);
    auto fSlG = RED4ext::CGlobalFunction::Create("VRScannerSlotGet", "VRScannerSlotGet", &VRScannerSlotGet);
    fSlG->flags = flags; fSlG->SetReturnType("Float");
    fSlG->AddParam("Int32", "idx"); fSlG->AddParam("Int32", "comp"); rtti->RegisterFunction(fSlG);
    auto fSlS = RED4ext::CGlobalFunction::Create("VRScannerSlotSet", "VRScannerSlotSet", &VRScannerSlotSet);
    fSlS->flags = flags; fSlS->SetReturnType("Int32");
    fSlS->AddParam("Int32", "idx"); fSlS->AddParam("Float", "x");
    fSlS->AddParam("Float", "y");   fSlS->AddParam("Float", "scale"); rtti->RegisterFunction(fSlS);
    auto fSlV = RED4ext::CGlobalFunction::Create("VRScannerSlotSave", "VRScannerSlotSave", &VRScannerSlotSave);
    fSlV->flags = flags; fSlV->SetReturnType("Int32"); rtti->RegisterFunction(fSlV);
    fMenu->flags = flags; fMenu->SetReturnType("Int32"); fMenu->AddParam("Int32", "open"); rtti->RegisterFunction(fMenu);
    fDevScr->flags = flags; fDevScr->SetReturnType("Int32"); fDevScr->AddParam("Int32", "open"); rtti->RegisterFunction(fDevScr);

    auto f73 = RED4ext::CGlobalFunction::Create("SetVRHeadLocal", "SetVRHeadLocal", &SetVRHeadLocal);
    f73->flags = flags; f73->SetReturnType("Int32"); f73->AddParam("Int32", "enable"); f73->AddParam("Int32", "conv");
    rtti->RegisterFunction(f73);

    auto f69 = RED4ext::CGlobalFunction::Create("SetVRXformOverride", "SetVRXformOverride", &SetVRXformOverride);
    f69->flags = flags; f69->SetReturnType("Int32"); f69->AddParam("Int32", "mode"); f69->AddParam("Float", "testYaw"); f69->AddParam("Int32", "plane");
    rtti->RegisterFunction(f69);

    // FIRE-SHOT direction lever.
    auto f74 = RED4ext::CGlobalFunction::Create("SetVRFireMode", "SetVRFireMode", &SetVRFireMode);
    f74->flags = flags; f74->SetReturnType("Int32"); f74->AddParam("Int32", "mode"); f74->AddParam("Int32", "plane"); f74->AddParam("Float", "angle"); f74->AddParam("Int32", "neg");
    rtti->RegisterFunction(f74);

    auto f75 = RED4ext::CGlobalFunction::Create("GetVRFireDump", "GetVRFireDump", &GetVRFireDump);
    f75->flags = flags; f75->SetReturnType("Float"); f75->AddParam("Int32", "idx");
    rtti->RegisterFunction(f75);

    auto f76 = RED4ext::CGlobalFunction::Create("SetVRFireScan", "SetVRFireScan", &SetVRFireScan);
    f76->flags = flags; f76->SetReturnType("Int32"); f76->AddParam("Int32", "src"); f76->AddParam("Int32", "base");
    rtti->RegisterFunction(f76);

    auto f77 = RED4ext::CGlobalFunction::Create("SetVRFireOverrideTarget", "SetVRFireOverrideTarget", &SetVRFireOverrideTarget);
    f77->flags = flags; f77->SetReturnType("Int32"); f77->AddParam("Int32", "src"); f77->AddParam("Int32", "off");
    rtti->RegisterFunction(f77);

    auto f78 = RED4ext::CGlobalFunction::Create("GetVRFireHit", "GetVRFireHit", &GetVRFireHit);
    f78->flags = flags; f78->SetReturnType("Float"); f78->AddParam("Int32", "hit"); f78->AddParam("Int32", "field");
    rtti->RegisterFunction(f78);

    // TRACE-DISPATCHER funnel instrumentation.
    auto f79 = RED4ext::CGlobalFunction::Create("SetVRTraceOverride", "SetVRTraceOverride", &SetVRTraceOverride);
    f79->flags = flags; f79->SetReturnType("Int32"); f79->AddParam("Int32", "on"); f79->AddParam("Int32", "writeOff"); f79->AddParam("Int32", "force"); f79->AddParam("Int32", "neg"); f79->AddParam("Int32", "gateRet");
    rtti->RegisterFunction(f79);

    auto f80 = RED4ext::CGlobalFunction::Create("ResetVRTrace", "ResetVRTrace", &ResetVRTrace);
    f80->flags = flags; f80->SetReturnType("Int32"); rtti->RegisterFunction(f80);

    auto f81 = RED4ext::CGlobalFunction::Create("GetVRTrace", "GetVRTrace", &GetVRTrace);
    f81->flags = flags; f81->SetReturnType("Float"); f81->AddParam("Int32", "idx");
    rtti->RegisterFunction(f81);

    auto f82 = RED4ext::CGlobalFunction::Create("GetVRTraceCaller", "GetVRTraceCaller", &GetVRTraceCaller);
    f82->flags = flags; f82->SetReturnType("Float"); f82->AddParam("Int32", "caller"); f82->AddParam("Int32", "field");
    rtti->RegisterFunction(f82);

    auto f83 = RED4ext::CGlobalFunction::Create("SetVRTargetCtrl", "SetVRTargetCtrl", &SetVRTargetCtrl);
    f83->flags = flags; f83->SetReturnType("Int32"); f83->AddParam("Int32", "on"); f83->AddParam("Int32", "neg");
    rtti->RegisterFunction(f83);

    auto f84 = RED4ext::CGlobalFunction::Create("SetVRProjCtrl", "SetVRProjCtrl", &SetVRProjCtrl);
    f84->flags = flags; f84->SetReturnType("Int32"); f84->AddParam("Int32", "on"); f84->AddParam("Int32", "neg"); f84->AddParam("Int32", "unguide"); f84->AddParam("Int32", "always");
    rtti->RegisterFunction(f84);

    auto f86 = RED4ext::CGlobalFunction::Create("GetVRProjDump", "GetVRProjDump", &GetVRProjDump);
    f86->flags = flags; f86->SetReturnType("Float"); f86->AddParam("Int32", "idx");
    rtti->RegisterFunction(f86);

    auto f87 = RED4ext::CGlobalFunction::Create("SetVRProjOriginRow", "SetVRProjOriginRow", &SetVRProjOriginRow);
    f87->flags = flags; f87->SetReturnType("Int32"); f87->AddParam("Int32", "row");
    rtti->RegisterFunction(f87);

    auto f90 = RED4ext::CGlobalFunction::Create("SetVRProjGateRva", "SetVRProjGateRva", &SetVRProjGateRva);
    f90->flags = flags; f90->SetReturnType("Int32"); f90->AddParam("Int32", "rva");
    rtti->RegisterFunction(f90);

    auto f88 = RED4ext::CGlobalFunction::Create("SetVRShotOrigin", "SetVRShotOrigin", &SetVRShotOrigin);
    f88->flags = flags; f88->SetReturnType("Int32"); f88->AddParam("Float", "x"); f88->AddParam("Float", "y"); f88->AddParam("Float", "z");
    rtti->RegisterFunction(f88);

    auto f89 = RED4ext::CGlobalFunction::Create("SetVRFireCamSnap", "SetVRFireCamSnap", &SetVRFireCamSnap);
    f89->flags = flags; f89->SetReturnType("Int32"); f89->AddParam("Int32", "on"); f89->AddParam("Int32", "off");
    rtti->RegisterFunction(f89);

    auto f85 = RED4ext::CGlobalFunction::Create("SetVRFireXform", "SetVRFireXform", &SetVRFireXform);
    f85->flags = flags; f85->SetReturnType("Int32"); f85->AddParam("Int32", "mode"); f85->AddParam("Int32", "off");
    rtti->RegisterFunction(f85);
 
}

RED4EXT_C_EXPORT void RED4EXT_CALL RegisterTypes() {}

// ONE PLUGIN, ONE SET OF ENTRY POINTS.
//
// This file used to be a RED4ext plugin of its own (CyberpunkVR_Hands.dll) with its own Main /
// Query / Supports. Two DLLs meant two plugin folders to keep in step, a named shared-memory block
// to talk across a boundary that was inside one process anyway, and -- when a stale copy of either
// was left behind -- two sets of pattern-scan detours over the same addresses, which is a crash
// waiting for a launch to land on.
//
// The natives now live in the single plugin. Only the RED4ext exports had to give: they were the
// ONLY three symbols that collided when the two translation units were first linked together
// (Main, Query, Supports -- nothing else in 17k lines), so the merge is this rename and nothing
// more. The registration itself is unchanged and still happens on Load, before the heavy graphics
// init, so the RTTI window cannot be missed.
void CyberpunkVR_RegisterHandsNatives() {
    auto rtti = RED4ext::CRTTISystem::Get();
    rtti->AddRegisterCallback(RegisterTypes);
    rtti->AddPostRegisterCallback(PostRegisterTypes);
}
