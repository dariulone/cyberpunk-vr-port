# Patched Djules V557 f096694/6746076. This backend is private to the plugin:
# private explicit manifest; no registry entry, tray, or loader interception.
set(CVR_OFXR_SOURCE "" CACHE PATH "Patched Djules V557 source tree")
# Pin the complete compatibility backend, including the recorder-state export.
# Developers may override its location for an offline checkout of this revision.
if(NOT CVR_OFXR_SOURCE)
    FetchContent_Declare(cvr_ofxr
        GIT_REPOSITORY https://github.com/satyaloka93/OFXR-Bridge.git
        GIT_TAG e750741e8ea3d46dde5c57de57523df2c1ea88bf
        GIT_SUBMODULES "")
    FetchContent_GetProperties(cvr_ofxr)
    if(NOT cvr_ofxr_POPULATED)
        FetchContent_Populate(cvr_ofxr)
    endif()
    set(CVR_OFXR_SOURCE "${cvr_ofxr_SOURCE_DIR}")
endif()
if(NOT EXISTS "${CVR_OFXR_SOURCE}/include/xrfg/provider_api.hpp")
    message(FATAL_ERROR "CVR_OFXR_SOURCE must name the patched Djules V557 checkout")
endif()
file(READ "${CVR_OFXR_SOURCE}/CMakeLists.txt" ofxr_cmake)
file(READ "${CVR_OFXR_SOURCE}/src/layer/provider_api.cpp" ofxr_provider)
if(NOT ofxr_cmake MATCHES "VERSION 0\\.557\\.0" OR NOT ofxr_provider MATCHES "OFXR_IsFlightRecordingV1")
    message(FATAL_ERROR "The private backend requires patched V557 with the active-recorder receipt")
endif()
set(XRFG_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(XRFG_BUILD_STANDALONE OFF CACHE BOOL "" FORCE)
add_subdirectory("${CVR_OFXR_SOURCE}" "${CMAKE_BINARY_DIR}/ofxr-v557")
add_dependencies(cyberpunkvrport_stereo XR_APILAYER_XRFrameBridge_diagnostic openxr_loader)
target_include_directories(cyberpunkvrport_stereo PRIVATE "${CVR_OFXR_SOURCE}/include")
add_custom_command(TARGET cyberpunkvrport_stereo POST_BUILD
    COMMAND "${CMAKE_COMMAND}" -E make_directory "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "$<TARGET_FILE:openxr_loader>" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/openxr_loader.dll"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${CVR_OFXR_SOURCE}/LICENSE" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/LICENSE-OFXR.txt"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "$<TARGET_FILE:XR_APILAYER_XRFrameBridge_diagnostic>"
        "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/CyberpunkVR_OFXR.dll"
    COMMAND "${CMAKE_COMMAND}" -E copy_directory
        "${CVR_OFXR_SOURCE}/licenses" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/licenses"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${CVR_OFXR_SOURCE}/THIRD_PARTY.md" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/THIRD_PARTY-OFXR.md"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${openxr_SOURCE_DIR}/LICENSE" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/LICENSE-OpenXR.txt"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/docs/OFXR-V557-INTEGRATION.md" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/README.md"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/cmake/ofxr-v557.json"
        "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/ofxr-v557.json"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/cmake/ofxr-v557.ini"
        "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/ofxr-v557/ofxr_bridge.ini")
