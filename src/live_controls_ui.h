#pragma once

struct LiveControlsUiState {
    float xrHeadOffsetX;
    float xrHeadOffsetY;
    float xrHeadOffsetZ;
    int xrMovementControl;
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
    int xrRuntime;

    // HUD placement / scale controls expected by imgui_overlay.cpp.
    float xrHudScale;
    float xrHudScaleY;
    float xrHudMinimapQuestScale;

    float xrHudPhone;
    float xrHudPhoneY;
    float xrHudPhoneScale;

    float xrHudTopLeftAlerts;
    float xrHudTopLeftAlertsY;
    float xrHudTopLeftAlertsScale;

    float xrHudTopRight;
    float xrHudTopRightY;
    float xrHudTopRightScale;

    float xrHudBottomLeft;
    float xrHudBottomLeftY;
    float xrHudBottomLeftScale;

    float xrHudBottomLeftTop;
    float xrHudBottomLeftTopY;
    float xrHudBottomLeftTopScale;

    float xrHudRadio;
    float xrHudRadioY;
    float xrHudRadioScale;

    float xrHudBottomRight;
    float xrHudBottomRightY;
    float xrHudBottomRightScale;

    float xrHudRightCenter;
    float xrHudRightCenterY;
    float xrHudRightCenterScale;

    float xrHudJohnnyHint;
    float xrHudActivityLog;
    float xrHudWarning;
    float xrHudBossHealth;
    float xrHudVehicleScan;
    float xrHudProgressBar;
    float xrHudOxygenBar;

    // Weapon proxy controls expected by newer overlay/runtime code.
    float xrWeaponPitch;
    float xrWeaponYaw;
    float xrWeaponRoll;
    float xrWeaponOffsetX;
    float xrWeaponOffsetY;
    float xrWeaponOffsetZ;
};

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState);
extern "C" void SetLiveControlsUiState(const LiveControlsUiState* state, int persistToFile);
extern "C" void RequestLiveControlsRecenter();
