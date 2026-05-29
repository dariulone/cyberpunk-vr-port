#pragma once

struct LiveControlsUiState {
    float xrHeadOffsetX;
    float xrHeadOffsetY;
    float xrHeadOffsetZ;
    int xrRecenter;
    int xrMonoSubmit;
    int xrAERSubmit;
    int xrWindowWidth;
    int xrWindowHeight;
    float xrForceFov;
    int xrMenuRect;
    float xrMenuFov;
    float xrPitchSign;
    float xrPitchScale;
    int xrSyncSequential;
    int xr3DofMovement;
    int xrDLSSMatrixHook;
    int xrDLSSSlotMode;
    int xrDLSSLogStride;
    int xrAERPairGate;
    int xrAERStartEye;
    int xrAERDebugEye;
    int xrAERWarmupFrames;
};

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState);
extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile);
extern "C" void RequestLiveControlsRecenter();
