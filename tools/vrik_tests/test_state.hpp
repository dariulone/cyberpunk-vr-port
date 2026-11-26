#pragma once
#include "Anim/VrikState.hpp"
extern VrikTransformSnapshot testNativePair,testLuaPair;
extern bool testNativeAvailable,testLuaAvailable;
extern bool testCyberwareWorldFrame;
#include <openxr/openxr.h>
#include <cstdint>
#include <string>
#include <vector>
// The hand-tracking runtime's scripted answers (stubs.cpp).
struct TestHandRuntime {
    bool haveCreate=true,haveDestroy=true,haveLocate=true;   // what xrGetInstanceProcAddr hands out
    XrResult createResult[2]{XR_SUCCESS,XR_SUCCESS};
    XrResult locateResult=XR_SUCCESS;
    XrBool32 active=XR_TRUE;
    XrHandJointLocationEXT joints[XR_HAND_JOINT_COUNT_EXT]{};
    std::vector<int64_t> calls;
};
extern TestHandRuntime testHand;
extern std::string testConfigDir;
extern int testFingerLogCounts[4];   // the finger filter's log lines: spike, jump, zero dropout, other
