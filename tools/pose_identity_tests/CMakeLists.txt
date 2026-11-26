cmake_minimum_required(VERSION 3.24)
project(PoseIdentityTests LANGUAGES CXX ASM_MASM)
enable_testing()
add_executable(pose_identity_tests main.cpp)
target_compile_features(pose_identity_tests PRIVATE cxx_std_20)
set(POSE_ID_INCLUDE_OVERLAY "" CACHE PATH "Build-only regression probe headers")
if(POSE_ID_INCLUDE_OVERLAY)
    target_include_directories(pose_identity_tests BEFORE PRIVATE "${POSE_ID_INCLUDE_OVERLAY}")
endif()
target_include_directories(pose_identity_tests PRIVATE ../../include ../../build/_deps/openxr-src/include)
if(MSVC)
    target_compile_options(pose_identity_tests PRIVATE /W4 /permissive-)
endif()
foreach(case identical_pose owner_generation changed_during_copy untracked_copy bounded_ledger cached_camera_lifetime
             submission reset_before_submit image_reuse latest_write object_lifetime reservations eye_rebase
             camera_blend camera_blend_reject camera_blend_stale capture_frontier capture_eye_origin runtime_frusta)
    add_test(NAME pose_id_${case} COMMAND pose_identity_tests ${case})
endforeach()
find_package(Python3 COMPONENTS Interpreter REQUIRED)
add_test(NAME pose_id_hook_abi COMMAND "${Python3_EXECUTABLE}" -B
    "${CMAKE_CURRENT_SOURCE_DIR}/verify_sites.py"
    "C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe")
add_executable(pose_leaf_abi_tests leaf_abi.cpp leaf_abi_probe.asm ../../src/Hooks/CameraPoseLeafThunks.asm)
target_compile_features(pose_leaf_abi_tests PRIVATE cxx_std_20)
add_test(NAME pose_id_leaf_registers COMMAND pose_leaf_abi_tests
    "C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe")
add_test(NAME pose_id_old_leaf_regression COMMAND pose_leaf_abi_tests
    "C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe" regression)
set_tests_properties(pose_id_old_leaf_regression PROPERTIES WILL_FAIL TRUE)
