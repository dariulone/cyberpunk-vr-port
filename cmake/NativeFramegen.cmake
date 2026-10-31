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
option(CVR_FRAMEGEN_PREBUILT "Use already-built libraries from the pinned FidelityFX SDK" OFF)
if(NOT CVR_FRAMEGEN_PREBUILT)
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
else()
    foreach(library IN LISTS cvr_ffx_libs)
        if(NOT EXISTS "${library}")
            message(FATAL_ERROR "Missing prebuilt FidelityFX library: ${library}")
        endif()
    endforeach()
endif()
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
