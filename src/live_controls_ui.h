#pragma once

struct LiveControlsUiState {
    float xrHeadOffsetX;
    float xrHeadOffsetY;
    float xrHeadOffsetZ;
    int xrRecenter;
    int xrMonoSubmit;
    int xrAERSubmit;
    float xrForceFov;
    int xrMenuRect;
    float xrMenuFov;
    int xr3DofMovement;
    int xrDLSSMatrixHook;
    int xrDLSSSlotMode;
    int xrDLSSLogStride;
    int xrAERPairGate;
    int xrAERStartEye;
    int xrAERDebugEye;
    float xrMotionPredictMs;
    float xrStereoScale;
    int xrRenderPoseSubmit;
    int xrAERHalfRate;
    int xrAERV2;
    int xrPoseLag;
};

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState);
extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile);
extern "C" void RequestLiveControlsRecenter();
