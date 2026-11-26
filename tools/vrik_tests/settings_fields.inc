// tools/vrik_tests: the keys of PollLiveControls' ini loop and a randomiser for LiveControlsValues (generated with
// the split, src/Core/LiveControlsParse.cpp).
static const char* const kSettingKeys[] = {
    "xr_weapon_laser_dot",
    "xr_head_offset_x",
    "xr_head_offset_y",
    "xr_head_offset_z",
    "xr_recenter",
    "xr_mono_submit",
    "xr_threaded_submit",
    "xr_window_width",
    "xr_window_height",
    "xr_force_fov",
    "xr_menu_rect",
    "xr_menu_fov",
    "xr_menu_follow_deg",
    "xr_mono_cascades",
    "xr_mono_mode",
    "xr_afw",
    "xr_osr",
    "xr_scope_stabilize",
    "xr_nvidia_reflex",
    "xr_fps_cap",
    "xr_lobby",
    "xr_lobby_ambient",
    "xr_hud_follow_mode",
    "xr_hud_stereo_depth",
    "xr_hud_brightness",
    "xr_hud_shadow",
    "xr_hud_glow",
    "xr_hud_panel",
    "xr_hud_follow_deg",
    "xr_hud_fov",
    "xr_hud_distance",
    "xr_interaction_panel",
    "xr_interaction_follow_deg",
    "xr_loot_follow_deg",
    "xr_interaction_distance",
    "xr_interaction_fov",
    "xr_pitch_sign",
    "xr_pitch_scale",
    "xr_sync_sequential",
    "xr_3dof_movement",
    "first_launch",
    "xr_motion_predict_ms",
    "xr_stereo_scale",
    "xr_world_scale",
    "xr_ipd_scale",
    "xr_sharpness",
    "xr_sharpmix",
    "xr_hmd_smooth",
    "xr_hand_per_frame",
    "xr_hand_rel_filtered",
    "xr_hand_lerp_pistol",
    "xr_hand_lerp_rifle",
    "xr_hand_lerp_shotgun",
    "xr_hand_lerp_sniper",
    "xr_hand_lerp_melee",
    "xr_hand_lerp_speed",
    "xr_hand_lerp_framegen_scale",
    "xr_scanner_frame",
    "xr_scanner_details",
    "xr_scanner_hacks",
    "xr_scanner_hint",
    "xr_scanner_memory",
    "xr_scanner_scripts",
    "xr_scanner_desc",
    "xr_vrcam_composition",
    "xr_auto_recenter",
    "xr_vive_trackers",
    "xr_grip_from_fingers",
    "xr_finger_spread_enabled",
    "xr_finger_spread",
    "xr_finger_bones",
    "xr_interaction_anchor",
    "xr_hud_clearance_hands_scale",
    "xr_hud_clearance_hands",
    "xr_hud_clearance_grow",
    "xr_hud_clearance",
    "xr_hud_to_second_eye",
    "xr_asym_projection",
    "xr_asym_inner_guard",
    "xr_hud_in_braindance",
    "xr_fix_lod",
    "xr_comp_lend_scoped",
    "xr_expo_mirror",
    "xr_expo_field_mask",
    "xr_distant_reuse",
    "xr_local_shadow_reuse",
    "xr_gi_reuse",
    "xr_probe_reuse",
    "xr_stable_copy",
    "xr_no_state_lies",
    "xr_indirect_census",
    "xr_expo_probe",
    "xr_run_view_params",
    "xr_vrcam_cam_fields",
    "xr_force_vrcam_cam",
    "xr_cap_grant",
    "xr_light_content",
    "xr_bd_quat_from_buffer",
    "xr_bd_ipd_in_locate",
    "xr_dev_cam_tol_m",
    "xr_dev_cam_any_name",
    "xr_input_default_in_ui",
    "xr_popup_mag_block_ms",
    "xr_lens_head_write",
    "xr_dev_cam_in_locate",
    "xr_vision_cs_node",
    "xr_vision_cs_ord",
    "xr_vision_cs_dim",
    "xr_vision_cs_swap",
    "xr_vision_to_second_eye",
    "xr_weapon_plane_as_scene",
    "xr_highlight_any_plane",
    "xr_plane_bucket_scene",
    "xr_vrcam_pos_from_main",
    "xr_bd_quat_from_write_site",
    "xr_stable_srgb_view",
    "xr_final_grab",
    "xr_comp_lend_set",
    "xr_bd_base_from_locate",
    "xr_bd_editor_align",
    "xr_bd_one_composition",
    "xr_bd_push_base",
    "xr_bd_main_pos_from_scene",
    "xr_bd_push_transform",
    "xr_bd_cam_dirty",
    "xr_env_mirror_mask",
    "xr_viewdata_fix_mask",
    "xr_render_mask_grant",
    "xr_stage_replay",
    "xr_vrcam_gbuffer_partitions",
    "xr_scope_gbuffer_partitions",
    "xr_scope_graph_dag",
    "xr_submit_chain",
    "xr_defer_present",
    "xr_gt_skip_render_queue",
    "xr_submit_chain_rev",
    "xr_water_mirror",
    "xr_water_assign",
    "xr_viewdata_diff",
    "xr_scene_stick_yaw_dps",
    "xr_scene_stick_yaw_tier",
    "xr_lut_src_lend",
    "xr_block_diff",
    "xr_block_diff_node",
    "xr_block_diff_size",
    "xr_env_extra_mask",
    "xr_grade_mirror_mask",
    "xr_two_hand_radius",
    "xr_two_hand_aim",
    "xr_two_hand_break",
    "xr_two_hand_aim_release_ms",
    "xr_carry_radius",
    "xr_recoil_rifle_rise",
    "xr_recoil_rifle_back",
    "xr_recoil_shotgun_rise",
    "xr_recoil_shotgun_back",
    "xr_recoil_down_pistol",
    "xr_recoil_down_rifle",
    "xr_recoil_down_shotgun",
    "xr_recoil_back_cm",
    "xr_recoil_back_max_cm",
    "xr_recoil_twohand_back",
    "xr_recoil_sniper_rise",
    "xr_recoil_sniper_back",
    "xr_recoil_scale",
    "xr_haptics",
    "xr_haptic_scale",
    "xr_recoil_pistol_back",
    "xr_recoil_climb",
    "xr_recoil_climb_max_deg",
    "xr_recoil_climb_ms",
    "xr_recoil_return_ms",
    "xr_recoil_return_pow",
    "xr_render_pose_submit",
    "xr_reuse_last_frame",
    "xr_pair_lock",
    "xr_pose_lag",
    "xr_runtime",
    "xr_depth_submit",
    "xr_movement_control",
    "xr_disable_mouse_y",
    "xr_xinput_hook",
    "xr_snap_turn",
    "xr_snap_turn_angle_deg",
    "xr_movement_source",
    "xr_movement_speed_mode",
    "xr_sprint_mode",
    "xr_left_stick_deadzone",
    "xr_right_stick_deadzone",
    "xr_max_input_threshold",
    "xr_cutscene_suspend_tier",
    "xr_vehicle_cutscene_suspend_tier",
    "xr_body_free_look_deg",
    "xr_body_free_look_down_deg",
    "xr_body_free_look_swim_deg",
    "xr_body_move_radius",
    "xr_scene_cam_comfort_deg",
    "xr_physical_crouch",
    "xr_physical_crouch_depth",
    "xr_ladder_grip_climb",
    "xr_left_handed",
    "xr_left_handed_bindings",
    "xr_ladder_auto_finish",
    "xr_ladder_finish_distance",
    "xr_breaststroke_swim",
    "xr_roomscale_movement",
    "xr_tracked_body_rotation",
    "xr_hybrid_body_rotation",
    "xr_physical_body_rotation",
    "xr_xinput_install",
    "xr_input_actions",
    "xr_mono_xqueue_wait",
    "xr_snap_turn_pulse_ms",
    "xr_mono_depth_capture",
    "xr_snap_turn_yaw_index",
    "xr_immersive_holsters",
    "xr_body_hands_only",
    "xr_veh_head_offset_x",
    "xr_veh_head_offset_y",
    "xr_veh_head_offset_z",
    "xr_wheel_grab",
    "xr_menu_gesture",
    "xr_vr_weapon_wheel",
    "xr_wheel_radius",
    "xr_wheel_steer_max_deg",
    "xr_wheel_steer_dead_deg",
    "xr_wheel_prediction",
    "xr_wheel_prediction_ms",
    "xr_wheel_horn",
    "xr_wheel_horn_radius",
    "xr_vehicle_gun_trigger",
    "xr_vehicle_throttle_trim",
};
template<class R> void RandomiseLiveControls(LiveControlsValues& v, R& r) {
    v.xrWeaponLaserDot = r.Chance(2);
    v.xrHeadOffsetX = r.Range(-100, 100);
    v.xrHeadOffsetY = r.Range(-100, 100);
    v.xrHeadOffsetZ = r.Range(-100, 100);
    v.xrRecenter = int(r.Next() % 2001) - 1000;
    v.xrMonoSubmit = int(r.Next() % 2001) - 1000;
    v.xrThreadedSubmit = int(r.Next() % 2001) - 1000;
    v.xrWindowWidth = int(r.Next() % 2001) - 1000;
    v.xrWindowHeight = int(r.Next() % 2001) - 1000;
    v.xrForceFov = r.Range(-100, 100);
    v.xrMenuRect = int(r.Next() % 2001) - 1000;
    v.xrMenuFov = r.Range(-100, 100);
    v.xrMenuFollowDeg = r.Range(-100, 100);
    v.xrHudPanel = int(r.Next() % 2001) - 1000;
    v.xrHudFollowDeg = r.Range(-100, 100);
    v.xrHudFov = r.Range(-100, 100);
    v.xrHudDistance = r.Range(-100, 100);
    v.xrInteractionPanel = int(r.Next() % 2001) - 1000;
    v.xrInteractionFollowDeg = r.Range(-100, 100);
    v.xrInteractionDistance = r.Range(-100, 100);
    v.xrInteractionFov = r.Range(-100, 100);
    v.xrLootFollowDeg = r.Range(-100, 100);
    v.xrHudFollowMode = int(r.Next() % 2001) - 1000;
    v.xrHudStereoDepth = int(r.Next() % 2001) - 1000;
    v.xrHudBrightness = r.Range(-100, 100);
    v.xrHudShadow = r.Range(-100, 100);
    v.xrHudGlow = r.Range(-100, 100);
    v.monoCascades = uint32_t(r.Next());
    v.monoMode = int(r.Next() % 2001) - 1000;
    v.afw = int(r.Next() % 2001) - 1000;
    v.osr = int(r.Next() % 2001) - 1000;
    v.scopeStabilize = int(r.Next() % 2001) - 1000;
    v.nvidiaReflex = int(r.Next() % 2001) - 1000;
    v.fpsCap = int(r.Next() % 2001) - 1000;
    v.lobby = int(r.Next() % 2001) - 1000;
    v.lobbyAmbient = int(r.Next() % 2001) - 1000;
    v.xrPitchSign = r.Range(-100, 100);
    v.xrPitchScale = r.Range(-100, 100);
    v.xrSyncSequential = int(r.Next() % 2001) - 1000;
    v.xr3DofMovement = int(r.Next() % 2001) - 1000;
    v.xrFirstLaunch = int(r.Next() % 2001) - 1000;
    v.xrMotionPredictMs = r.Range(-100, 100);
    v.xrStereoScale = r.Range(-100, 100);
    v.xrWorldScale = r.Range(-100, 100);
    v.xrIpdScale = r.Range(-100, 100);
    v.xrSharpness = r.Range(-100, 100);
    v.xrSharpmix = r.Range(-100, 100);
    v.xrReuseLastFrame = int(r.Next() % 2001) - 1000;
    v.xrPairLock = int(r.Next() % 2001) - 1000;
    v.xrRenderPoseSubmit = int(r.Next() % 2001) - 1000;
    v.xrPoseLag = int(r.Next() % 2001) - 1000;
    v.xrRuntime = int(r.Next() % 2001) - 1000;
    v.xrDepthSubmit = int(r.Next() % 2001) - 1000;
    v.xrMovementControl = int(r.Next() % 2001) - 1000;
    v.xrDisableMouseY = int(r.Next() % 2001) - 1000;
    v.xrXInputHook = int(r.Next() % 2001) - 1000;
    v.xrSnapTurn = int(r.Next() % 2001) - 1000;
    v.xrHmdSmooth = r.Range(-100, 100);
    v.xrHandLerp = r.Range(-100, 100);
    v.xrLerpPistol = r.Range(-100, 100);
    v.xrLerpRifle = r.Range(-100, 100);
    v.xrLerpShotgun = r.Range(-100, 100);
    v.xrLerpSniper = r.Range(-100, 100);
    v.xrLerpMelee = r.Range(-100, 100);
    v.xrLerpFramegen = r.Range(-100, 100);
    v.xrGradeMirrorMask = uint32_t(r.Next());
    v.xrEnvExtraMask = uint32_t(r.Next());
    v.xrWaterMirror = uint32_t(r.Next());
    v.xrWaterAssign = uint32_t(r.Next());
    v.xrViewDataDiff = int(r.Next() % 2001) - 1000;
    v.xrLutSrcLend = uint32_t(r.Next());
    v.xrSceneYawDps = r.Range(-100, 100);
    v.xrSceneYawTier = int(r.Next() % 2001) - 1000;
    v.xrBlockDiff = int(r.Next() % 2001) - 1000;
    v.xrBlockDiffNode = uint32_t(r.Next());
    v.xrBlockDiffSize = uint32_t(r.Next());
    v.xrRenderMaskGrant = uint32_t(r.Next());
    v.xrStageReplay = int(r.Next() % 2001) - 1000;
    v.xrGbufferPartitions = int(r.Next() % 2001) - 1000;
    v.xrScopeGbufferPartitions = int(r.Next() % 2001) - 1000;
    v.xrScopeGraphDag = int(r.Next() % 2001) - 1000;
    v.xrSubmitChain = int(r.Next() % 2001) - 1000;
    v.xrDeferPresent = int(r.Next() % 2001) - 1000;
    v.xrGtSkipRenderQueue = int(r.Next() % 2001) - 1000;
    v.xrSubmitChainRev = int(r.Next() % 2001) - 1000;
    v.xrViewDataFixMask = uint32_t(r.Next());
    v.xrEnvMirrorMask = uint32_t(r.Next());
    v.xrBdCamDirty = int(r.Next() % 2001) - 1000;
    v.xrBdPushTransform = int(r.Next() % 2001) - 1000;
    v.xrBdMainPosScene = int(r.Next() % 2001) - 1000;
    v.xrBdPushBase = int(r.Next() % 2001) - 1000;
    v.xrBdOneComp = int(r.Next() % 2001) - 1000;
    v.xrBdEditorAlign = int(r.Next() % 2001) - 1000;
    v.xrBdBaseFromLocate = int(r.Next() % 2001) - 1000;
    v.xrCompLendSet = uint32_t(r.Next());
    v.xrFinalGrab = int(r.Next() % 2001) - 1000;
    v.xrStableSrgbView = int(r.Next() % 2001) - 1000;
    v.xrBdQuatWriteSite = int(r.Next() % 2001) - 1000;
    v.xrVrcamPosFromMain = int(r.Next() % 2001) - 1000;
    v.xrDevCamInLocate = int(r.Next() % 2001) - 1000;
    v.xrLensHeadWrite = int(r.Next() % 2001) - 1000;
    v.xrInputDefaultInUi = int(r.Next() % 2001) - 1000;
    v.xrPopupMagBlockMs = int(r.Next() % 2001) - 1000;
    v.xrDevCamAnyName = int(r.Next() % 2001) - 1000;
    v.xrDevCamTolM = r.Range(-100, 100);
    v.xrBdIpdInLocate = int(r.Next() % 2001) - 1000;
    v.xrBdQuatFromBuffer = int(r.Next() % 2001) - 1000;
    v.xrCapGrant = uint32_t(r.Next());
    v.xrForceVrcamCam = uint32_t(r.Next());
    v.xrVrcamCamFields = uint32_t(r.Next());
    v.xrRunViewParams = uint32_t(r.Next());
    v.xrExpoProbe = int(r.Next() % 2001) - 1000;
    v.xrIndirectCensus = int(r.Next() % 2001) - 1000;
    v.xrNoStateLies = int(r.Next() % 2001) - 1000;
    v.xrStableCopy = int(r.Next() % 2001) - 1000;
    v.xrDistantReuse = uint32_t(r.Next());
    v.xrLocalShadowReuse = uint32_t(r.Next());
    v.xrGiReuse = uint32_t(r.Next());
    v.xrProbeReuse = uint32_t(r.Next());
    v.xrExpoMirror = int(r.Next() % 2001) - 1000;
    v.xrExpoFieldMask = uint32_t(r.Next());
    v.xrCompLendScoped = uint32_t(r.Next());
    v.xrLightContent = int(r.Next() % 2001) - 1000;
    v.xrFixLod = int(r.Next() % 2001) - 1000;
    v.xrHudTo2 = int(r.Next() % 2001) - 1000;
    v.xrInteractionAnchor = int(r.Next() % 2001) - 1000;
    v.xrHudClearance = int(r.Next() % 2001) - 1000;
    v.xrHudClearanceGrow = r.Range(-100, 100);
    v.xrHudClearanceHands = int(r.Next() % 2001) - 1000;
    v.xrHudClearanceHandsScale = r.Range(-100, 100);
    v.xrAutoRecenter = int(r.Next() % 2001) - 1000;
    v.xrFingerBones = int(r.Next() % 2001) - 1000;
    v.xrGripFromFingers = int(r.Next() % 2001) - 1000;
    v.xrFingerSpread = r.Range(-100, 100);
    v.xrFingerSpreadOn = int(r.Next() % 2001) - 1000;
    v.xrAsym = int(r.Next() % 2001) - 1000;
    v.xrAsymGuard = r.Range(-100, 100);
    v.xrVisionTo2 = int(r.Next() % 2001) - 1000;
    v.xrHudBd = int(r.Next() % 2001) - 1000;
    v.xrCompGroup = uint32_t(r.Next());
    v.xrTwoHandRadius = r.Range(-100, 100);
    v.xrTwoHandAim = int(r.Next() % 2001) - 1000;
    v.xrTwoHandBreak = r.Range(-100, 100);
    v.xrTwoHandAimRelease = r.Range(-100, 100);
    v.xrCarryRadius = r.Range(-100, 100);
    v.xrRifleRise = r.Range(-100, 100);
    v.xrRifleBack = r.Range(-100, 100);
    v.xrShotgunRise = r.Range(-100, 100);
    v.xrShotgunBack = r.Range(-100, 100);
    v.xrDownPistol = r.Range(-100, 100);
    v.xrDownRifle = r.Range(-100, 100);
    v.xrDownShotgun = r.Range(-100, 100);
    v.xrBackCm = r.Range(-100, 100);
    v.xrBackMaxCm = r.Range(-100, 100);
    v.xrTwoHandBack = r.Range(-100, 100);
    v.xrSniperRise = r.Range(-100, 100);
    v.xrSniperBack = r.Range(-100, 100);
    v.xrRecoilScale = r.Range(-100, 100);
    v.xrHaptics = int(r.Next() % 2001) - 1000;
    v.xrHapticScale = r.Range(-100, 100);
    v.xrPistolBack = r.Range(-100, 100);
    v.xrClimbFrac = r.Range(-100, 100);
    v.xrClimbMax = r.Range(-100, 100);
    v.xrClimbMs = r.Range(-100, 100);
    v.xrReturnMs = r.Range(-100, 100);
    v.xrReturnPow = r.Range(-100, 100);
    for (auto& x : v.xrScannerSlots) x = r.Range(-2, 2);
    v.xrHandRelFiltered = int(r.Next() % 2001) - 1000;
    v.xrHandPerFrame = int(r.Next() % 2001) - 1000;
    v.xrSnapTurnAngleDeg = r.Range(-100, 100);
    v.xrMovementSource = int(r.Next() % 2001) - 1000;
    v.xrMovementSpeedMode = int(r.Next() % 2001) - 1000;
    v.xrSprintMode = int(r.Next() % 2001) - 1000;
    v.xrLeftStickDeadzone = r.Range(-100, 100);
    v.xrRightStickDeadzone = r.Range(-100, 100);
    v.xrMaxInputThreshold = r.Range(-100, 100);
    v.xrXInputInstall = int(r.Next() % 2001) - 1000;
    v.xrInputActions = int(r.Next() % 2001) - 1000;
    v.xrMonoXQueueWait = int(r.Next() % 2001) - 1000;
    v.xrSnapTurnPulseMs = int(r.Next() % 2001) - 1000;
    v.xrMonoDepthCapture = int(r.Next() % 2001) - 1000;
    v.xrSnapTurnYawIndex = int(r.Next() % 2001) - 1000;
    v.xrImmersiveHolsters = int(r.Next() % 2001) - 1000;
    v.xrBodyHandsOnly = int(r.Next() % 2001) - 1000;
    v.xrPhysicalBodyRotation = int(r.Next() % 2001) - 1000;
    v.xrTrackedBodyRotation = int(r.Next() % 2001) - 1000;
    v.xrHybridBodyRotation = int(r.Next() % 2001) - 1000;
    v.xrRoomscaleMovement = int(r.Next() % 2001) - 1000;
    v.xrPhysicalCrouch = int(r.Next() % 2001) - 1000;
    v.xrPhysicalCrouchDepth = r.Range(-100, 100);
    v.xrBreaststrokeSwim = int(r.Next() % 2001) - 1000;
    v.xrLadderGripClimb = int(r.Next() % 2001) - 1000;
    v.xrLeftHanded = int(r.Next() % 2001) - 1000;
    v.xrLeftHandedBindings = int(r.Next() % 2001) - 1000;
    v.xrLadderAutoFinish = int(r.Next() % 2001) - 1000;
    v.xrLadderFinishDistance = r.Range(-100, 100);
    v.xrBodyFreeLookDeg = r.Range(-100, 100);
    v.xrBodyFreeLookDownDeg = r.Range(-100, 100);
    v.xrBodyFreeLookSwimDeg = r.Range(-100, 100);
    v.xrBodyMoveRadius = r.Range(-100, 100);
    v.xrSceneCamComfortDeg = r.Range(-100, 100);
    v.xrCutsceneSuspendTier = int(r.Next() % 2001) - 1000;
    v.xrVehicleCutsceneSuspendTier = int(r.Next() % 2001) - 1000;
    v.vehicleCutsceneTierConfigured = r.Chance(2);
    v.xrVehHeadOffsetX = r.Range(-100, 100);
    v.xrVehHeadOffsetY = r.Range(-100, 100);
    v.xrVehHeadOffsetZ = r.Range(-100, 100);
    v.xrWheelGrab = int(r.Next() % 2001) - 1000;
    v.xrMenuGesture = int(r.Next() % 2001) - 1000;
    v.xrVrWeaponWheel = int(r.Next() % 2001) - 1000;
    v.xrWheelRadius = r.Range(-100, 100);
    v.xrWheelSteerMaxDeg = r.Range(-100, 100);
    v.xrWheelSteerDeadDeg = r.Range(-100, 100);
    v.xrWheelPrediction = int(r.Next() % 2001) - 1000;
    v.xrWheelPredictionMs = r.Range(-100, 100);
    v.xrWheelHorn = int(r.Next() % 2001) - 1000;
    v.xrWheelHornRadius = r.Range(-100, 100);
    v.xrVehicleGunTrigger = int(r.Next() % 2001) - 1000;
    v.xrVehicleThrottleTrim = r.Range(-100, 100);
    v.g_viveTrackersSetting = int(r.Next() % 2001) - 1000;
    v.CyberpunkVR_VisionCsNode = uint32_t(r.Next());
    v.CyberpunkVR_VisionCsOrd = int(r.Next() % 2001) - 1000;
    v.CyberpunkVR_VisionCsDim = int(r.Next() % 2001) - 1000;
    v.CyberpunkVR_VisionCsSwap = int(r.Next() % 2001) - 1000;
    v.CyberpunkVR_WeaponPlaneAsScene = int(r.Next() % 2001) - 1000;
    v.CyberpunkVR_HighlightAnyPlane = int(r.Next() % 2001) - 1000;
    v.CyberpunkVR_PlaneBucketScene = int(r.Next() % 2001) - 1000;
}
