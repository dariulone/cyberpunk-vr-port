function(cvr_world_marker_shaders target)
    set(hints "$ENV{WindowsSdkDir}/bin/${CMAKE_VS_WINDOWS_TARGET_PLATFORM_VERSION}/x64"
              "C:/Program Files (x86)/Windows Kits/10/bin/${CMAKE_VS_WINDOWS_TARGET_PLATFORM_VERSION}/x64")
    set(names dxc)
    # Built on Linux (cmake/msvc-wine.cmake): msvc-wine's SDK has no dxc, so a Windows dxc.exe is looked for in
    # ~/dxc/bin/x64 (the DirectXShaderCompiler release zip, unpacked) and run under Wine; its dxil.dll beside it signs
    # the DXIL. A Linux dxc is taken after it, given its libdxil.so: without the validator the DXIL goes out unsigned
    # and D3D12 refuses it. -DCVR_DXC_EXECUTABLE=<path> picks either outright.
    if(CVR_MSVC_WINE)
        file(GLOB sdk_bins LIST_DIRECTORIES true "${CVR_MSVC_WINE_ROOT}/kits/10/bin/10.*/x64")
        list(SORT sdk_bins COMPARE NATURAL ORDER DESCENDING)
        # find_file, not find_program: Wine runs the .exe without the execute bit, which the release zip doesn't set.
        find_file(CVR_DXC_EXECUTABLE NAMES dxc.exe HINTS "$ENV{HOME}/dxc/bin/x64" ${sdk_bins}
            NO_DEFAULT_PATH NO_CMAKE_FIND_ROOT_PATH)
    endif()
    find_program(CVR_DXC_EXECUTABLE NAMES ${names} HINTS ${hints} REQUIRED)
    set(launcher "")
    if(CMAKE_HOST_UNIX AND CVR_DXC_EXECUTABLE MATCHES "\\.exe$")
        find_program(CVR_WINE_EXECUTABLE NAMES wine64 wine REQUIRED)
        set(launcher "${CVR_WINE_EXECUTABLE}")
    endif()
    set(shader "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/../src/Render/WorldMarkers.hlsl")
    set(embed "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/EmbedWorldMarkerShaders.cmake")
    set(output "${CMAKE_CURRENT_BINARY_DIR}/generated/${target}/WorldMarkerShaderCode.hpp")
    add_custom_command(OUTPUT "${output}"
        COMMAND "${CMAKE_COMMAND}" "-DDXC=${CVR_DXC_EXECUTABLE}" "-DDXC_LAUNCHER=${launcher}" "-DSOURCE=${shader}" "-DOUTPUT=${output}" -P "${embed}"
        DEPENDS "${shader}" "${embed}" "${CVR_DXC_EXECUTABLE}"
        COMMENT "Compiling native-compatible SM6 world marker vertex shaders"
        VERBATIM)
    target_sources(${target} PRIVATE "${output}")
    target_include_directories(${target} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/generated/${target}")
endfunction()
