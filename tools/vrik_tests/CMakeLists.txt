cmake_minimum_required(VERSION 3.24)
project(VrikChainTests LANGUAGES CXX)
enable_testing()
add_executable(weapon_controls_test weapon_controls_test.cpp)
target_compile_features(weapon_controls_test PRIVATE cxx_std_20)
target_include_directories(weapon_controls_test PRIVATE ../../include)
add_test(NAME vrik_weapon_controls COMMAND weapon_controls_test)
add_executable(attachment_recoil_test attachment_recoil_test.cpp ../../src/Anim/Recoil.cpp)
target_compile_features(attachment_recoil_test PRIVATE cxx_std_20)
target_include_directories(attachment_recoil_test PRIVATE ../../include)
add_test(NAME vrik_attachment_recoil COMMAND attachment_recoil_test)
set(VR_CHAIN_CHARACTER_SOURCE "${CMAKE_CURRENT_SOURCE_DIR}/../../src/Anim/CharacterRig.cpp" CACHE FILEPATH "Solver source (override only for regression probes)")
set(VR_CHAIN_SNAPSHOT_SOURCE "${CMAKE_CURRENT_SOURCE_DIR}/../../src/Anim/HandSnapshot.cpp" CACHE FILEPATH "Packet reader source (override only for regression probes)")
set(VR_CHAIN_SHARED_SOURCES main.cpp stubs.cpp ../../src/Anim/CharacterRigState.cpp ../../src/Runtimes/HapticsState.cpp
    ../../src/Camera/CameraLinkState.cpp ../../src/Utils/MemorySafeC.cpp ../../src/Hooks/BodyHooksState.cpp
    xr_fake.cpp ../../src/Hooks/XInput.cpp ../../src/Hooks/InputServices.cpp camera_state.cpp
    ../../src/Hooks/WeaponAimState.cpp ../../src/Hooks/PanzerState.cpp)
function(vrik_chain_program target)
    target_compile_features(${target} PRIVATE cxx_std_20)
    target_include_directories(${target} PRIVATE ../../include
        ../../externals/RED4ext.SDK/include ../../externals/minhook/include
        ../../build/_deps/openxr-src/include ../../build/_deps/imgui-src ../../externals/wrpuppets-fbt-ik/include)
    target_compile_definitions(${target} PRIVATE
        NOMINMAX WIN32_LEAN_AND_MEAN XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12
        VR_CHAIN_FIXTURE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/fixtures")
    if(MSVC)
        target_compile_options(${target} PRIVATE /W4 /permissive- /Gy /Gw)
        target_link_options(${target} PRIVATE /OPT:REF /OPT:ICF /INCREMENTAL:NO)
    endif()
endfunction()
set(VR_CHAIN_CPP_SOURCES "${VR_CHAIN_CHARACTER_SOURCE}" "${VR_CHAIN_SNAPSHOT_SOURCE}"
    ../../src/Anim/ReloadPose.cpp ../../src/Anim/Cyberware.cpp ../../src/Anim/HandGestures.cpp
    ../../src/Anim/LeftHanded.cpp ../../src/Anim/HeadAimWeapon.cpp ../../src/Anim/FkRecorder.cpp
    ../../src/Anim/AdsEyeAlign.cpp ../../src/Anim/AdsMuzzleStabilizer.cpp ../../src/Anim/SmokingPose.cpp
    ../../src/Anim/ReloadHandOff.cpp ../../src/Anim/WeaponRig.cpp ../../src/Anim/WheelGrab.cpp ../../src/Anim/Recoil.cpp ../../src/Anim/GraspPose.cpp ../../src/Anim/TwoHandGrip.cpp ../../src/Hooks/AnimPoseBody.cpp ../../src/Runtimes/Haptics.cpp ../../src/Runtimes/FingerGrip.cpp
    ../../src/Camera/CameraLink.cpp ../../src/Camera/CameraMath.cpp)
add_executable(vrik_chain_tests ${VR_CHAIN_SHARED_SOURCES} ${VR_CHAIN_CPP_SOURCES})
vrik_chain_program(vrik_chain_tests)
# The room trace (parity_trace.hpp, VRIK_ROOM_TESTS): the same program with the body hooks running instead of standing in
# -- src/Hooks/RoomscaleMove.cpp, BodyHeading.cpp and BodyYawFollow.cpp here, their Rust port below.
add_executable(vrik_room_tests ${VR_CHAIN_SHARED_SOURCES} ${VR_CHAIN_CPP_SOURCES}
    ../../src/Hooks/RoomscaleMove.cpp ../../src/Hooks/BodyHeading.cpp ../../src/Hooks/BodyYawFollow.cpp)
vrik_chain_program(vrik_room_tests)
target_compile_definitions(vrik_room_tests PRIVATE VRIK_ROOM_TESTS=1)
# The water trace (VRIK_WATER_TESTS): the body hooks and the ladder and the swim strokes running -- src/Hooks/LadderInput.cpp
# and SwimmingInput.cpp here, their Rust port below.
add_executable(vrik_water_tests ${VR_CHAIN_SHARED_SOURCES} ${VR_CHAIN_CPP_SOURCES}
    ../../src/Hooks/RoomscaleMove.cpp ../../src/Hooks/BodyHeading.cpp ../../src/Hooks/BodyYawFollow.cpp
    ../../src/Hooks/LadderInput.cpp ../../src/Hooks/SwimmingInput.cpp ../../src/Hooks/CyberwareChord.cpp
    ../../src/Camera/PoseIdentity.cpp ../../src/Hooks/FinalCamera.cpp ../../src/Hooks/CameraPitch.cpp
    ../../src/Hooks/WorldSpaceBlendCamera.cpp ../../src/Hooks/CameraPoseTransport.cpp ../../src/Hooks/PatchCamera.cpp
    ../../src/Hooks/LocateCamera.cpp ../../src/Hooks/WeaponAim.cpp ../../src/Hooks/PanzerSteering.cpp
    ../../src/Hooks/PanzerWeaponAim.cpp ../../src/Core/LiveControlsParse.cpp ../../src/Hooks/MenuModeCallback.cpp
    ../../src/Hooks/LeftHandedMirror.cpp ../../src/Hooks/StoryAttention.cpp ../../src/Hooks/SurveillanceBody.cpp
    ../../src/Render/FoveatedDlssSettings.cpp ../../src/Render/FixedVrsSettings.cpp ../../src/Render/HiddenAreaSettings.cpp)
vrik_chain_program(vrik_water_tests)
target_compile_definitions(vrik_water_tests PRIVATE VRIK_ROOM_TESTS=1 VRIK_WATER_TESTS=1)
set(VR_CHAIN_CASES fk_fixture packet_atomicity coordinate_roundtrip body_write_census
             rig_policy native_lower toes_ahead head_basis shadow_upper model_replay hand_gestures
             rig_rebind pose_buffer_rebind shoulder_reference forearm_roll forearm_solve launcher_wrist
             weapon_stance weapon_stance_armed_start weapon_stance_bending body_arm_frame body_bend_geometry floor_reach_geometry
             head_bound_bend girdle_reach overhead_reach cyberware_pose mantis_pose mantis_wrist cyberware_wire combat_elbow elbow_blend cyberware_muscles launcher_pose launcher_axis passenger_window vehicle_hand_frame vehicle_upper_body vehicle_seated
             rifle_elbow neck_mount body_centre_orbit female_neck_mount combat_hmd_body swimming_body ladder_upper ladder_grips ladder_rest ladder_finger_adjustments ladder_baked_rung ladder_reach pelvis_pose upper_posture
             startup_packet pair_selection model_origin neutral_view_reference pose_chain
             missing_camera_replay invalidated_camera_release missing_camera_expiry missing_native_centre paired_consumed
             vehicle_pair_miss vehicle_missing_camera)
foreach(case ${VR_CHAIN_CASES})
    add_test(NAME vrik_${case} COMMAND vrik_chain_tests ${case})
endforeach()

# The Rust variants' glue (rust_glue/: the C forms of the C++ the solve reads and the C++ names over its exports)
# is these programs' own copy of the plugin's src/Anim/CharacterRigRust.cpp, src/Camera/CameraRust.cpp,
# PoseIdentityRust.cpp and src/Hooks/RoomscaleRust.cpp, WaterLadderRust.cpp, InputRust.cpp.
# The same program against the Rust solve (rust/crates/cvr-rig/src/vrik through cvr_rust.lib, feature rig) in place
# of src/Anim/CharacterRig.cpp and HandSnapshot.cpp, and the parity test: the trace case of both builds must print the same line
# (docs/rust-migration-plan.md, stage 3).
option(CVR_VRIK_TESTS_RUST "Also build the Rust VRIK solve and compare it with C++" ON)
if(CVR_VRIK_TESTS_RUST)
    # rig-exports: the exported state the solve reaches by name, as the plugin defines it (cvr-render's rig_test_exports.rs).
    set(CVR_RUST_FEATURES rig rig-exports)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(vrik_chain_tests_rust ${VR_CHAIN_SHARED_SOURCES} rust_glue/CharacterRigRust.cpp rust_glue/CameraRust.cpp settings_stub.cpp)
    vrik_chain_program(vrik_chain_tests_rust)
    target_compile_definitions(vrik_chain_tests_rust PRIVATE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1)
    cvr_link_rust(vrik_chain_tests_rust)
    foreach(case ${VR_CHAIN_CASES})
        add_test(NAME vrik_rust_${case} COMMAND vrik_chain_tests_rust ${case})
    endforeach()
    add_executable(vrik_room_tests_rust ${VR_CHAIN_SHARED_SOURCES} rust_glue/CharacterRigRust.cpp rust_glue/CameraRust.cpp
        rust_glue/RoomscaleRust.cpp settings_stub.cpp)
    vrik_chain_program(vrik_room_tests_rust)
    target_compile_definitions(vrik_room_tests_rust PRIVATE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1 VRIK_ROOM_TESTS=1)
    cvr_link_rust(vrik_room_tests_rust)
    add_executable(vrik_water_tests_rust ${VR_CHAIN_SHARED_SOURCES} rust_glue/CharacterRigRust.cpp rust_glue/CameraRust.cpp
        rust_glue/RoomscaleRust.cpp rust_glue/WaterLadderRust.cpp rust_glue/InputRust.cpp
        rust_glue/PoseIdentityRust.cpp ../../src/Hooks/CameraPoseTransport.cpp ../../src/Hooks/PatchCamera.cpp
        ../../src/Hooks/LocateCamera.cpp ../../src/Hooks/WeaponAim.cpp ../../src/Hooks/PanzerSteering.cpp
        ../../src/Hooks/PanzerWeaponAim.cpp ../../src/Core/LiveControlsParse.cpp ../../src/Hooks/MenuModeCallback.cpp
        ../../src/Hooks/LeftHandedMirror.cpp ../../src/Hooks/StoryAttention.cpp ../../src/Hooks/SurveillanceBody.cpp)
    vrik_chain_program(vrik_water_tests_rust)
    target_compile_definitions(vrik_water_tests_rust PRIVATE CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1 VRIK_ROOM_TESTS=1 VRIK_WATER_TESTS=1)
    cvr_link_rust(vrik_water_tests_rust)
    add_test(NAME vrik_water_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:vrik_water_tests> -DRUST=$<TARGET_FILE:vrik_water_tests_rust> -DCASE=water_trace
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
    add_test(NAME vrik_room_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:vrik_room_tests> -DRUST=$<TARGET_FILE:vrik_room_tests_rust> -DCASE=room_trace
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
    add_test(NAME vrik_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:vrik_chain_tests> -DRUST=$<TARGET_FILE:vrik_chain_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
