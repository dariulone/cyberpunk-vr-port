cmake_minimum_required(VERSION 3.24)
project(BodyRotationTests LANGUAGES CXX)
enable_testing()
add_executable(body_rotation_tests main.cpp)
add_executable(body_recording_replay recording_replay.cpp)
target_compile_features(body_recording_replay PRIVATE cxx_std_20)
target_include_directories(body_recording_replay PRIVATE ../../include ../../build/_deps/openxr-src/include)
target_compile_features(body_rotation_tests PRIVATE cxx_std_20)
target_include_directories(body_rotation_tests PRIVATE ../../include ../../build/_deps/openxr-src/include)
set(CVR_BODY_CASES turns head_led_turns head_led_noisy head_only gestures tracking cadence noise invariants noisy_turns mixed_motion lifecycle noisy_idle wrist_only noisy_gestures return_turns startup source_freshness baselines neck_lever small_turns discontinuities hybrid_policy hybrid_transitions hybrid_boundary hybrid_recovery hybrid_posture hybrid_low_glance hybrid_neck_limit hybrid_fast_turn_down hybrid_glance_then_down hybrid_missed_turn_heals hybrid_freeze_turn hybrid_down_glance_then_turn hybrid_hands_change_then_turn
    prediction_steady prediction_stops prediction_lifecycle prediction_noisy_rebase prediction_cadence prediction_acceleration prediction_held_samples)
foreach(case ${CVR_BODY_CASES})
    add_test(NAME body_rotation_${case} COMMAND body_rotation_tests ${case})
endforeach()

# The same cases against the Rust body (rust/crates/cvr-body through cvr_rust.lib, CVR_RUST_BODY), and the parity test:
# the trace case of both builds -- every body module on long random input, every output hashed bit for bit -- must
# print the same line (docs/rust-migration-plan.md, stage 3).
option(CVR_BODY_TESTS_RUST "Also build the Rust body implementation and compare it with C++" ON)
if(CVR_BODY_TESTS_RUST)
    include(../../cmake/RustStaticlib.cmake)
    add_executable(body_rotation_tests_rust main.cpp ../rust_test_support/plugin_stubs.cpp)
    target_compile_features(body_rotation_tests_rust PRIVATE cxx_std_20)
    target_include_directories(body_rotation_tests_rust PRIVATE ../../include ../../build/_deps/openxr-src/include)
    target_compile_definitions(body_rotation_tests_rust PRIVATE CVR_RUST_BODY=1)
    cvr_link_rust(body_rotation_tests_rust)
    foreach(case ${CVR_BODY_CASES})
        add_test(NAME body_rotation_rust_${case} COMMAND body_rotation_tests_rust ${case})
    endforeach()
    add_test(NAME body_rotation_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:body_rotation_tests> -DRUST=$<TARGET_FILE:body_rotation_tests_rust>
                     -P ${CMAKE_CURRENT_SOURCE_DIR}/../../cmake/ParityTrace.cmake)
endif()
