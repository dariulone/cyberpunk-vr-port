include(ExternalProject)
set(CVR_FIDELITYFX_ROOT "${CMAKE_BINARY_DIR}/fidelityfx-native" CACHE PATH "Pinned FidelityFX SDK source checkout")
if(NOT EXISTS "${CVR_FIDELITYFX_ROOT}/sdk/CMakeLists.txt")
    FetchContent_Declare(cvr_framegen_fidelityfx
        GIT_REPOSITORY https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK.git
        GIT_TAG c6efa6bf7f2027b3ec94f28578bb5965eabb9e55
        SOURCE_DIR "${CVR_FIDELITYFX_ROOT}")
    FetchContent_GetProperties(cvr_framegen_fidelityfx)
    if(NOT cvr_framegen_fidelityfx_POPULATED)
        FetchContent_Populate(cvr_framegen_fidelityfx)
    endif()
endif()
set(cvr_ffx_libs)
foreach(component backend_dx12 frameinterpolation opticalflow)
    list(APPEND cvr_ffx_libs "${CVR_FIDELITYFX_ROOT}/sdk/bin/ffx_sdk/ffx_${component}_x64.lib")
endforeach()
ExternalProject_Add(cvr_framegen_sdk
    SOURCE_DIR "${CVR_FIDELITYFX_ROOT}/sdk"
    BINARY_DIR "${CMAKE_BINARY_DIR}/fidelityfx-native-build"
    DOWNLOAD_COMMAND ""
    CMAKE_ARGS -DFFX_ALL=OFF -DFFX_OF=ON -DFFX_FI=ON
        -DFFX_API_BACKEND=DX12_X64 -DFFX_BUILD_AS_DLL=OFF
    BUILD_COMMAND "${CMAKE_COMMAND}" --build <BINARY_DIR> --config Release --parallel 4
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS ${cvr_ffx_libs})
add_dependencies(cyberpunkvrport_stereo cvr_framegen_sdk)
target_include_directories(cyberpunkvrport_stereo PRIVATE
    "${CVR_FIDELITYFX_ROOT}/sdk/include" "${CMAKE_SOURCE_DIR}/externals/nvidia-optical-flow")
target_link_libraries(cyberpunkvrport_stereo PRIVATE ${cvr_ffx_libs})

# Native NVIDIA DLSS Frame Generation (NGX) needs the DLSS SDK's headers and static NGX loader.
# Only include/ and lib/ are checked out (a blobless sparse clone): the SDK's runtime DLLs are not
# needed, the game ships nvngx_dlssg.dll next to its executable. Pinned to DLSS 310.9.1.
set(CVR_DLSS_SDK_ROOT "${CMAKE_BINARY_DIR}/dlss-sdk" CACHE PATH "NVIDIA DLSS SDK (include + lib/Windows_x86_64)")
set(CVR_DLSS_SDK_COMMIT 374959484e79a640feaba44c93ac8cfb0a03f5b5)
if(NOT EXISTS "${CVR_DLSS_SDK_ROOT}/include/nvsdk_ngx_helpers_dlssg_d3d.h")
    find_package(Git QUIET)
    if(GIT_FOUND)
        execute_process(COMMAND "${GIT_EXECUTABLE}" clone --filter=blob:none --no-checkout
            https://github.com/NVIDIA/DLSS.git "${CVR_DLSS_SDK_ROOT}" RESULT_VARIABLE cvr_dlss_clone)
        if(cvr_dlss_clone EQUAL 0)
            execute_process(COMMAND "${GIT_EXECUTABLE}" -C "${CVR_DLSS_SDK_ROOT}" sparse-checkout set --no-cone
                /include/ /lib/Windows_x86_64/x64/ /LICENSE.txt)
            execute_process(COMMAND "${GIT_EXECUTABLE}" -C "${CVR_DLSS_SDK_ROOT}" checkout ${CVR_DLSS_SDK_COMMIT})
        endif()
    endif()
endif()
if(EXISTS "${CVR_DLSS_SDK_ROOT}/include/nvsdk_ngx_helpers_dlssg_d3d.h" AND
   EXISTS "${CVR_DLSS_SDK_ROOT}/lib/Windows_x86_64/x64/nvsdk_ngx_d.lib")
    target_include_directories(cyberpunkvrport_stereo PRIVATE "${CVR_DLSS_SDK_ROOT}/include")
    target_link_libraries(cyberpunkvrport_stereo PRIVATE
        optimized "${CVR_DLSS_SDK_ROOT}/lib/Windows_x86_64/x64/nvsdk_ngx_d.lib"
        debug "${CVR_DLSS_SDK_ROOT}/lib/Windows_x86_64/x64/nvsdk_ngx_d_dbg.lib")
    target_compile_definitions(cyberpunkvrport_stereo PRIVATE CVR_NATIVE_DLSSG=1)
    message(STATUS "Native DLSS Frame Generation: NVIDIA DLSS SDK at ${CVR_DLSS_SDK_ROOT}")
else()
    message(WARNING "NVIDIA DLSS SDK not found: native DLSS Frame Generation is left out of this build")
endif()

# ANR FG (Apex Neural Rendering, github.com/apex-compute/ANR): a DLSS-G provider, its own nvngx_dlssg.dll, run by
# Framegen/AnrFrameGen.cpp the way OptiScaler runs it from custom/<name>/dlssg. The release's addon package is pinned
# by hash and its dlssg folder goes beside the plugin unchanged (anr/dlssg: the dll, its kernels, programs and nr_fg.ini).
set(CVR_ANR_FG_VERSION V.B15)
set(CVR_ANR_FG_SHA256 717e93b3e63cb08dbaf6240cd585bae94d498c2749d8603d800d17ad97ef9e41)
set(CVR_ANR_FG_ROOT "${CMAKE_BINARY_DIR}/anr-fg-${CVR_ANR_FG_VERSION}")
set(cvr_anr_dlssg "${CVR_ANR_FG_ROOT}/Apex Neural Rendering FG/dlssg")
if(NOT EXISTS "${cvr_anr_dlssg}/nvngx_dlssg.dll")
    set(cvr_anr_zip "${CMAKE_BINARY_DIR}/anr-fg-${CVR_ANR_FG_VERSION}-addon.zip")
    file(DOWNLOAD "https://github.com/apex-compute/ANR/releases/download/FG-${CVR_ANR_FG_VERSION}/anr-fg-${CVR_ANR_FG_VERSION}-addon.zip"
        "${cvr_anr_zip}" EXPECTED_HASH SHA256=${CVR_ANR_FG_SHA256} STATUS cvr_anr_status)
    list(GET cvr_anr_status 0 cvr_anr_code)
    if(cvr_anr_code EQUAL 0)
        file(ARCHIVE_EXTRACT INPUT "${cvr_anr_zip}" DESTINATION "${CVR_ANR_FG_ROOT}")
    endif()
endif()
if(EXISTS "${cvr_anr_dlssg}/nvngx_dlssg.dll")
    add_custom_command(TARGET cyberpunkvrport_stereo POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_directory "${cvr_anr_dlssg}" "$<TARGET_FILE_DIR:cyberpunkvrport_stereo>/anr/dlssg")
    message(STATUS "ANR FG ${CVR_ANR_FG_VERSION}: ${cvr_anr_dlssg}")
else()
    message(WARNING "ANR FG package not available: the ANR FG frame generator will report it is not installed")
endif()
