cmake_minimum_required(VERSION 3.24)
project(GameSettingsTests LANGUAGES CXX)
enable_testing()
set(root "${CMAKE_CURRENT_SOURCE_DIR}/../..")
set(json "${root}/build/_deps/openxr-src/src/external/jsoncpp")
add_executable(game_settings_tests main.cpp ${root}/src/Core/GameSettings.cpp
    ${json}/src/lib_json/json_reader.cpp ${json}/src/lib_json/json_value.cpp ${json}/src/lib_json/json_writer.cpp)
target_include_directories(game_settings_tests PRIVATE ${root}/include ${json}/include)
target_compile_features(game_settings_tests PRIVATE cxx_std_20)
target_compile_definitions(game_settings_tests PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN)
add_test(NAME settings_merge_and_atomic_install COMMAND game_settings_tests
    "${root}/mods/config/UserSettings.json" "${CMAKE_CURRENT_BINARY_DIR}")
# The same program against the Rust settings (rust/crates/cvr-settings through cvr_rust.lib; the adapter
# rust_settings.cpp), and a trace of both compared byte for byte: the preset over the real player file (when there is one), over
# itself and over an edited copy, the skipped options, and options read back.
target_compile_definitions(game_settings_tests PRIVATE "CVR_SOURCE_ROOT=\"${root}\"")
option(CVR_GAME_SETTINGS_TESTS_RUST "Also run the test against the Rust settings" ON)
if(CVR_GAME_SETTINGS_TESTS_RUST)
    include(${root}/cmake/RustStaticlib.cmake)
    add_executable(game_settings_tests_rust main.cpp rust_settings.cpp
        ${json}/src/lib_json/json_reader.cpp ${json}/src/lib_json/json_value.cpp ${json}/src/lib_json/json_writer.cpp)
    target_include_directories(game_settings_tests_rust PRIVATE ${root}/include ${json}/include)
    target_compile_features(game_settings_tests_rust PRIVATE cxx_std_20)
    target_compile_definitions(game_settings_tests_rust PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN CVR_RUST_VRIK=1 CVR_RUST_TEST_FACADES=1
        "CVR_SOURCE_ROOT=\"${root}\"")
    cvr_link_rust(game_settings_tests_rust)
    add_test(NAME settings_merge_and_atomic_install_rust COMMAND game_settings_tests_rust
        "${root}/mods/config/UserSettings.json" "${CMAKE_CURRENT_BINARY_DIR}")
    add_test(NAME settings_parity
             COMMAND ${CMAKE_COMMAND} -DCPP=$<TARGET_FILE:game_settings_tests> -DRUST=$<TARGET_FILE:game_settings_tests_rust>
                     -P ${root}/cmake/ParityTrace.cmake)
endif()