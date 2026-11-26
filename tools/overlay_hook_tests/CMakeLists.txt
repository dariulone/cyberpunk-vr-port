cmake_minimum_required(VERSION 3.24)
project(OverlayHookTests LANGUAGES CXX)
enable_testing()
find_package(Python3 COMPONENTS Interpreter REQUIRED)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/../../src/Hooks/DepthCapture.cpp"
    "${CMAKE_CURRENT_SOURCE_DIR}/generate_fixture.py")
execute_process(COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/generate_fixture.py"
    "${CMAKE_CURRENT_BINARY_DIR}/production_patch.cpp" COMMAND_ERROR_IS_FATAL ANY)
add_executable(overlay_hook_test main.cpp "${CMAKE_CURRENT_BINARY_DIR}/production_patch.cpp")
target_include_directories(overlay_hook_test PRIVATE ../../include)
target_compile_features(overlay_hook_test PRIVATE cxx_std_20)
foreach(case overlay_before overlay_after duplicate missing_original)
    add_test(NAME overlay_hook_${case} COMMAND overlay_hook_test ${case})
endforeach()
