cmake_minimum_required(VERSION 3.24)
project(CyberpunkOfxrBridgeTests LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
get_filename_component(PORT_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)
set(CVR_OPENXR_LOADER_DLL "" CACHE FILEPATH "Built private OpenXR loader DLL")
if(NOT EXISTS "${CVR_OPENXR_LOADER_DLL}")
    message(FATAL_ERROR "Set CVR_OPENXR_LOADER_DLL to the built private openxr_loader.dll")
endif()
add_library(ofxr_test_loader SHARED fake_loader.cpp fake_loader.def)
target_include_directories(ofxr_test_loader PRIVATE "${CVR_OFXR_SOURCE}" "${XRFG_OPENXR_SDK_ROOT}/include" "${XRFG_VULKAN_INCLUDE_DIR}")
target_compile_definitions(ofxr_test_loader PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D11 XR_USE_GRAPHICS_API_D3D12 XR_USE_GRAPHICS_API_VULKAN VK_NO_PROTOTYPES VK_USE_PLATFORM_WIN32_KHR)
target_link_libraries(ofxr_test_loader PRIVATE d3d11 d3d12 dxgi)
add_library(ofxr_observer SHARED observer.cpp)
target_include_directories(ofxr_observer PRIVATE "${XRFG_OPENXR_SDK_ROOT}/include")
add_executable(ofxr_bridge_tests main.cpp "${PORT_ROOT}/src/Framegen/Framegen.cpp" "${PORT_ROOT}/src/Framegen/OfxrBridge.cpp")
target_compile_definitions(ofxr_bridge_tests PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX XR_USE_PLATFORM_WIN32 XR_USE_GRAPHICS_API_D3D12)
target_include_directories(ofxr_bridge_tests PRIVATE "${PORT_ROOT}/include" "${CVR_OFXR_SOURCE}/include" "${XRFG_OPENXR_SDK_ROOT}/include")
target_link_libraries(ofxr_bridge_tests PRIVATE psapi)
add_dependencies(ofxr_bridge_tests ofxr_test_loader ofxr_observer)
add_custom_command(TARGET ofxr_bridge_tests POST_BUILD
    COMMAND "${CMAKE_COMMAND}" -E make_directory "$<TARGET_FILE_DIR:ofxr_bridge_tests>/ofxr-v557"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${CVR_OPENXR_LOADER_DLL}" "$<TARGET_FILE_DIR:ofxr_bridge_tests>/ofxr-v557/openxr_loader.dll"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${PORT_ROOT}/cmake/ofxr-v557.json" "$<TARGET_FILE_DIR:ofxr_bridge_tests>/ofxr-v557/ofxr-v557.json"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${CVR_OFXR_DLL}" "$<TARGET_FILE_DIR:ofxr_bridge_tests>/ofxr-v557/CyberpunkVR_OFXR.dll"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${PORT_ROOT}/cmake/ofxr-v557.ini" "$<TARGET_FILE_DIR:ofxr_bridge_tests>/ofxr-v557/ofxr_bridge.ini")
enable_testing()
add_test(NAME private_v557_dispatch_logging COMMAND ofxr_bridge_tests)
add_test(NAME private_v557_external_guard COMMAND ofxr_bridge_tests external)
