# Reconstruction-only subset of OFXR V557. No API layer, hooks, tray, or presenter.
# See README.md and provenance.json. LGPL-3.0.
file(GLOB cvr_fxc_paths "C:/Program Files (x86)/Windows Kits/10/bin/*/x64/fxc.exe")
list(SORT cvr_fxc_paths COMPARE NATURAL ORDER DESCENDING)
find_program(XRFG_FXC_EXECUTABLE fxc HINTS ${cvr_fxc_paths})
if(NOT XRFG_FXC_EXECUTABLE AND cvr_fxc_paths)
    list(GET cvr_fxc_paths 0 XRFG_FXC_EXECUTABLE)
endif()
if(NOT XRFG_FXC_EXECUTABLE)
    message(FATAL_ERROR "Windows SDK fxc.exe required for V557 reconstruction")
endif()
    set(XRFG_GENERATED_SHADER_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated-shaders")
    set(XRFG_PACK_INPUT_HEADER "${XRFG_GENERATED_SHADER_DIR}/pack_flow_input_shader.hpp")
    set(XRFG_FULLSCREEN_VERTEX_HEADER "${XRFG_GENERATED_SHADER_DIR}/fullscreen_vertex_shader.hpp")
    set(XRFG_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_GAME_MOTION_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/game_motion_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_NVIDIA_PACK_INPUT_HEADER "${XRFG_GENERATED_SHADER_DIR}/nvidia_pack_flow_input_shader.hpp")
    set(XRFG_NVIDIA_PACK_LUMA_INPUT_HEADER "${XRFG_GENERATED_SHADER_DIR}/nvidia_pack_luma_input_shader.hpp")
    set(XRFG_NVIDIA_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/nvidia_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_NVIDIA_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/nvidia_bidirectional_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_NVIDIA_FAST_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/nvidia_fast_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_NVIDIA_FAST_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/nvidia_fast_bidirectional_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_HYBRID_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/hybrid_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_HYBRID_NVIDIA_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/hybrid_nvidia_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_HYBRID_NVIDIA_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/hybrid_nvidia_bidirectional_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_HYBRID_NVIDIA_FAST_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/hybrid_nvidia_fast_synthesize_midpoint_pixel_shader.hpp")
    set(XRFG_HYBRID_NVIDIA_FAST_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER "${XRFG_GENERATED_SHADER_DIR}/hybrid_nvidia_fast_bidirectional_synthesize_midpoint_pixel_shader.hpp")
    # V516: MSFS 2024's foveated inset composed into one guide per eye.
    set(XRFG_GUIDE_COMPOSE_CLEAR_HEADER "${XRFG_GENERATED_SHADER_DIR}/guide_compose_clear_shader.hpp")
    set(XRFG_GUIDE_COMPOSE_MEASURE_HEADER "${XRFG_GENERATED_SHADER_DIR}/guide_compose_measure_shader.hpp")
    set(XRFG_GUIDE_COMPOSE_HEADER "${XRFG_GENERATED_SHADER_DIR}/guide_compose_shader.hpp")
    # V539: the hybrid's coverage mask.
    set(XRFG_COVERAGE_MASK_CLEAR_HEADER "${XRFG_GENERATED_SHADER_DIR}/coverage_mask_clear_shader.hpp")
    set(XRFG_COVERAGE_MASK_BUILD_HEADER "${XRFG_GENERATED_SHADER_DIR}/coverage_mask_build_shader.hpp")
    # V546: the eye order of two DLSS streams nothing else tells apart.
    set(XRFG_EYE_ORDER_CLEAR_HEADER "${XRFG_GENERATED_SHADER_DIR}/eye_order_clear_shader.hpp")
    set(XRFG_EYE_ORDER_MEASURE_HEADER "${XRFG_GENERATED_SHADER_DIR}/eye_order_measure_shader.hpp")
    # V554: the hybrid depth search's midpoint splat.
    set(XRFG_DEPTH_SPLAT_CLEAR_HEADER "${XRFG_GENERATED_SHADER_DIR}/depth_splat_clear_shader.hpp")
    set(XRFG_DEPTH_SPLAT_BUILD_HEADER "${XRFG_GENERATED_SHADER_DIR}/depth_splat_build_shader.hpp")
    add_custom_command(
        OUTPUT
            "${XRFG_PACK_INPUT_HEADER}"
            "${XRFG_FULLSCREEN_VERTEX_HEADER}"
            "${XRFG_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_GAME_MOTION_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_NVIDIA_PACK_INPUT_HEADER}"
            "${XRFG_NVIDIA_PACK_LUMA_INPUT_HEADER}"
            "${XRFG_NVIDIA_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_NVIDIA_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_NVIDIA_FAST_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_NVIDIA_FAST_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_HYBRID_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_HYBRID_NVIDIA_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_HYBRID_NVIDIA_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_HYBRID_NVIDIA_FAST_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_HYBRID_NVIDIA_FAST_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}"
            "${XRFG_GUIDE_COMPOSE_CLEAR_HEADER}"
            "${XRFG_GUIDE_COMPOSE_MEASURE_HEADER}"
            "${XRFG_GUIDE_COMPOSE_HEADER}"
            "${XRFG_COVERAGE_MASK_CLEAR_HEADER}"
            "${XRFG_COVERAGE_MASK_BUILD_HEADER}"
            "${XRFG_EYE_ORDER_CLEAR_HEADER}"
            "${XRFG_EYE_ORDER_MEASURE_HEADER}"
            "${XRFG_DEPTH_SPLAT_CLEAR_HEADER}"
            "${XRFG_DEPTH_SPLAT_BUILD_HEADER}"
        COMMAND "${CMAKE_COMMAND}" -E make_directory "${XRFG_GENERATED_SHADER_DIR}"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E ClearDepthSplat
            /D XRFG_HYBRID=1 /D XRFG_DEPTH_SPLAT=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_DEPTH_SPLAT_CLEAR_HEADER}" /Vn g_xrfg_depth_splat_clear_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E BuildDepthSplat
            /D XRFG_HYBRID=1 /D XRFG_DEPTH_SPLAT=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_DEPTH_SPLAT_BUILD_HEADER}" /Vn g_xrfg_depth_splat_build_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E ClearCoverageMask
            /D XRFG_HYBRID=1 /D XRFG_COVERAGE_MASK=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_COVERAGE_MASK_CLEAR_HEADER}" /Vn g_xrfg_coverage_mask_clear_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E BuildCoverageMask
            /D XRFG_HYBRID=1 /D XRFG_COVERAGE_MASK=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_COVERAGE_MASK_BUILD_HEADER}" /Vn g_xrfg_coverage_mask_build_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E ClearEyeOrder
            /D XRFG_HYBRID=1 /D XRFG_EYE_ORDER=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_EYE_ORDER_CLEAR_HEADER}" /Vn g_xrfg_eye_order_clear_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E MeasureEyeOrder
            /D XRFG_HYBRID=1 /D XRFG_EYE_ORDER=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_EYE_ORDER_MEASURE_HEADER}" /Vn g_xrfg_eye_order_measure_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E ClearInsetVerdict
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_GUIDE_COMPOSE_CLEAR_HEADER}" /Vn g_xrfg_guide_compose_clear_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/guide_compose.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E MeasureInsetMotion
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_GUIDE_COMPOSE_MEASURE_HEADER}" /Vn g_xrfg_guide_compose_measure_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/guide_compose.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E ComposeInsetGuide
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_GUIDE_COMPOSE_HEADER}" /Vn g_xrfg_guide_compose_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/guide_compose.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E PackFlowInput
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_PACK_INPUT_HEADER}" /Vn g_xrfg_pack_flow_input_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T vs_5_1 /E FullscreenTriangleVS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_FULLSCREEN_VERTEX_HEADER}" /Vn g_xrfg_fullscreen_vertex_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeMidpointPS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeGameMotionMidpointPS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_GAME_MOTION_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_game_motion_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E PackNvidiaFlowInput
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_NVIDIA_PACK_INPUT_HEADER}" /Vn g_xrfg_nvidia_pack_flow_input_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T cs_5_1 /E PackNvidiaFlowInput
            /D XRFG_NVIDIA_LUMA_INPUT=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_NVIDIA_PACK_LUMA_INPUT_HEADER}" /Vn g_xrfg_nvidia_pack_luma_input_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeNvidiaMidpointPS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_NVIDIA_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_nvidia_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeNvidiaBidirectionalMidpointPS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_NVIDIA_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_nvidia_bidirectional_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeNvidiaFastMidpointPS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_NVIDIA_FAST_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_nvidia_fast_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeNvidiaFastBidirectionalMidpointPS
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_NVIDIA_FAST_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_nvidia_fast_bidirectional_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeHybridMidpointPS
            /D XRFG_HYBRID=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_HYBRID_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_hybrid_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeHybridNvidiaMidpointPS
            /D XRFG_HYBRID=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_HYBRID_NVIDIA_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_hybrid_nvidia_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeHybridNvidiaBidirectionalMidpointPS
            /D XRFG_HYBRID=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_HYBRID_NVIDIA_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_hybrid_nvidia_bidirectional_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeHybridNvidiaFastMidpointPS
            /D XRFG_HYBRID=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_HYBRID_NVIDIA_FAST_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_hybrid_nvidia_fast_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        COMMAND "${XRFG_FXC_EXECUTABLE}" /nologo /T ps_5_1 /E SynthesizeHybridNvidiaFastBidirectionalMidpointPS
            /D XRFG_HYBRID=1
            /O3 /WX /Ges /Qstrip_debug /Qstrip_reflect
            /Fh "${XRFG_HYBRID_NVIDIA_FAST_BIDIRECTIONAL_SYNTHESIZE_PIXEL_HEADER}" /Vn g_xrfg_hybrid_nvidia_fast_bidirectional_synthesize_midpoint_pixel_shader
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
        DEPENDS
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/frame_synthesis.hlsl"
            "${CMAKE_CURRENT_SOURCE_DIR}/src/d3d12/guide_compose.hlsl"
        VERBATIM)


add_library(cvr_v557 STATIC
    src/d3d12/d3d12_frame_synthesizer.cpp
    src/d3d12/d3d12_history.cpp
    src/common/dlss_motion_vectors.cpp
    src/layer/bridge_flight_logger.cpp
    src/core/fps_overlay_model.cpp
    src/core/image.cpp
    src/core/optical_flow.cpp
    src/core/pose.cpp
    ${XRFG_NVIDIA_SYNTHESIZE_PIXEL_HEADER})
target_compile_features(cvr_v557 PUBLIC cxx_std_20)
target_compile_definitions(cvr_v557 PRIVATE NOMINMAX WIN32_LEAN_AND_MEAN _DISABLE_CONSTEXPR_MUTEX_CONSTRUCTOR)
target_include_directories(cvr_v557 PUBLIC include PRIVATE "${XRFG_GENERATED_SHADER_DIR}" "${CVR_V557_FFX_INCLUDE}" "${CMAKE_CURRENT_SOURCE_DIR}/../nvidia-optical-flow")
target_link_libraries(cvr_v557 PUBLIC d3d12 dxgi ${CVR_V557_FFX_LIBS})

if(MSVC)
    target_compile_options(cvr_v557 PRIVATE /EHsc /permissive-)
endif()
