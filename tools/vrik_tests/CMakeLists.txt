cmake_minimum_required(VERSION 3.24)
project(VrikChainTests LANGUAGES CXX)
enable_testing()
set(VR_CHAIN_CHARACTER_SOURCE "${CMAKE_CURRENT_SOURCE_DIR}/../../src/Anim/CharacterRig.cpp" CACHE FILEPATH "Solver source (override only for regression probes)")
set(VR_CHAIN_SNAPSHOT_SOURCE "${CMAKE_CURRENT_SOURCE_DIR}/../../src/Anim/HandSnapshot.cpp" CACHE FILEPATH "Packet reader source (override only for regression probes)")
add_executable(vrik_chain_tests main.cpp stubs.cpp
    "${VR_CHAIN_CHARACTER_SOURCE}" "${VR_CHAIN_SNAPSHOT_SOURCE}" ../../src/Anim/ReloadPose.cpp ../../src/Anim/Cyberware.cpp ../../src/Anim/HandGestures.cpp)
target_compile_features(vrik_chain_tests PRIVATE cxx_std_20)
target_include_directories(vrik_chain_tests PRIVATE ../../include
    ../../externals/RED4ext.SDK/include ../../externals/minhook/include
    ../../build/_deps/openxr-src/include ../../build/_deps/imgui-src)
target_compile_definitions(vrik_chain_tests PRIVATE
    NOMINMAX WIN32_LEAN_AND_MEAN XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12
    VR_CHAIN_FIXTURE_DIR="${CMAKE_CURRENT_SOURCE_DIR}/fixtures")
if(MSVC)
    target_compile_options(vrik_chain_tests PRIVATE /W4 /permissive- /Gy /Gw)
    target_link_options(vrik_chain_tests PRIVATE /OPT:REF /OPT:ICF /INCREMENTAL:NO)
endif()
foreach(case fk_fixture packet_atomicity coordinate_roundtrip body_write_census
             rig_policy native_lower toes_ahead head_basis shadow_upper model_replay hand_gestures
             rig_rebind pose_buffer_rebind shoulder_reference forearm_roll forearm_solve launcher_wrist
             weapon_stance weapon_stance_armed_start weapon_stance_bending body_arm_frame body_bend_geometry floor_reach_geometry
             head_bound_bend girdle_reach overhead_reach cyberware_pose mantis_pose mantis_wrist cyberware_wire combat_elbow elbow_blend cyberware_muscles launcher_pose launcher_axis passenger_window vehicle_hand_frame vehicle_upper_body
             neck_mount body_centre_orbit female_neck_mount combat_hmd_body swimming_body ladder_upper ladder_grips ladder_rest ladder_finger_adjustments ladder_baked_rung ladder_reach pelvis_pose upper_posture
             startup_packet pair_selection model_origin neutral_view_reference pose_chain
             missing_camera_replay invalidated_camera_release missing_camera_expiry missing_native_centre paired_consumed
             vehicle_pair_miss vehicle_missing_camera)
    add_test(NAME vrik_${case} COMMAND vrik_chain_tests ${case})
endforeach()
