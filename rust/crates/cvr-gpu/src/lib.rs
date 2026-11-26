//! The port's Direct3D 12 helpers (stage 4, docs/rust-migration-plan.md).
//!
//!   caption  lines of text set with DirectWrite under a hung screen, as a picture with its mips (formerly
//!            src/Render/Caption.cpp)
//!   color_blit  the colour blitter: copies, overlay blends, the engine's HUD composite and the barrel dot
//!               (formerly src/Render/ColorBlit.cpp)
//!   command_resources  the objects a command list uses, kept alive until the GPU has run every submission of it
//!                      (formerly src/Render/CommandResources.cpp; include/Render/CommandResources.hpp calls it)
//!   frame_capture  framegen's capture of three consecutive real frames -- colour, depth, motion and cameras -- for
//!                  replays and the benchmark (formerly src/Framegen/FrameCapture.cpp)
//!   frame_timer  the game's GPU frame time for framegen's statistics: timestamps around a frame's commands on its
//!              queue (formerly src/Framegen/GpuTimer.cpp)
//!   hardware_telemetry  the machine's load for framegen's statistics overlay: CPU per core, RAM, the GPU's budget and,
//!              through NVML, its load and temperature (formerly src/Framegen/HardwareTelemetry.cpp)
//!   hud_panel  the HUD panels' canvases: each channel's sprites, mips, blur and the engine's HUD composite in three fenced
//!              slots (formerly src/Render/HudPanel.cpp)
//!   input_copy  framegen's copy of a frame's depth or motion vectors into a texture of its own (formerly
//!              src/Framegen/InputCopy.cpp)
//!   stereo_depth  the unjittered depth each eye is submitted to XR with, held until the submit fence (formerly
//!              src/Render/StereoDepth.cpp)
//!   srgb_pass  the sRGB decode into an eye swapchain that does not take the eye bytes as sRGB
//!              (formerly src/Render/SrgbToLinearPass.cpp)
//!   depth_commit_gate  single pass's GPU admission of a paired draw: equal initial depth/stencil layers and buffer
//!                proofs, as a predicate (formerly src/Render/DepthCommitGate.cpp)
//!   depth_resolve  the depth plane of the game's 64-bit scene depth drawn into the depth layer's 32-bit depth
//!                  (formerly src/Render/DepthResolve.cpp)
//!   desktop_mirror  the desktop window fitted to the render, and its centre crop at another window shape
//!                   (formerly src/Render/DesktopMirror.cpp)
//!   finer_vrs  single pass's per-eye shading-rate images reduced to the finest rate per tile
//!              (formerly src/Render/GpuFinerVrs.cpp)
//!   foveated_gpu  foveated DLSS's crop/scale of the engine's inputs and the composite of the two outputs
//!                 (formerly src/Render/FoveatedGpu.cpp)
//!   gpu_diag  DRED dumps of a removed device, the GPU diagnostics CVR_GPU_DIAG asks for and the command-list audit
//!             (formerly src/Hooks/Dred.cpp, ListAudit.cpp)
//!   geometry_compare  single pass's GPU byte comparison of buffer range pairs into draw predicates
//!                     (formerly src/Render/GpuGeometryCompare.cpp)
//!   geometry_checkpoint  single pass's private once-recorded copies of a native draw's G-buffer and depth
//!                        (formerly src/Render/GeometryTargetCheckpoint.cpp)
//!   group_resolve  single pass's resolve of one eye's opaque group from the paired G-buffer arrays
//!                (formerly src/Render/StereoGroupResolve.cpp)
//!   mono_cascades  the mono sun cascades: the second view's atlas copied and restored in MAIN's place
//!                  (formerly src/Render/MonoCascades.cpp, MonoCascadeImage.cpp, MonoCascadePolicy.hpp)
//!   optics  the physical scope's glass: its shaders, glass pipelines and their twins, full-rate shading, the lens velocity
//!           target and bound targets, the upscaler history mask (formerly src/Render/PhysicalOpticsShaders.cpp, LensMask.cpp)
//!   optics_camera  the camera-scope source resolve: the scope camera's image accumulated in place, reprojected by its
//!                  rotation (formerly src/Render/CameraSourceResolve.cpp)
//!   optics_probe  one texture of the frame read back for the diagnostics, by request or by stage (formerly
//!                 src/Render/OpticsProbe.cpp)
//!   optics_source  the lens source resolve: the scene accumulated over frames for the glass, jittered like the frame
//!                  (formerly src/Render/LensSourceResolve.cpp)
//!   osr  the port's own reconstruction of the other eye for an AFW launch: depth splats, traces and the fill (formerly
//!        src/Render/Osr.cpp)
//!   output_copy  single pass's copy of the paired output arrays into each native side's targets, read back
//!                (formerly src/Render/NativeOutputCopy.cpp)
//!   queue_timeline  a diagnostic: when each queue of the game runs its batches, timestamps around a number of submissions
//!                   (formerly src/Render/GpuQueueTimeline.cpp)
//!   renderdoc_bridge  the port's hooks routed through a resident RenderDoc and NVAPI let through it
//!                     (formerly src/Hooks/RenderDocBridge.cpp)
//!   scene_state  single pass's per-list bindings, all-table roots, resolve pipelines and weak target-view metadata
//!                (formerly src/Render/StereoSceneState.cpp)
//!   scene_profile  the per-node GPU timestamps (and optional statistics/occlusion queries) of the engine's lists
//!                  (formerly src/Render/SceneGpuProfile.cpp)
//!   target_array  single pass's two-layer target arrays and their copies to and from the engine's targets
//!                (formerly src/Render/StereoTargetArray.cpp)
//!   stage_profile  the bounded per-stage GPU timestamps of the capture, submit and HUD lists
//!                  (formerly src/Render/GpuStageProfile.cpp)
//!   vrs_snapshot  one diagnostic frame of single pass's per-eye shading-rate images, and their finer merge
//!                 (formerly src/Render/StereoVrsSnapshot.cpp)
//!   space_quad  pictures hung in the tracking space, drawn into an eye's image through its pose and frustum
//!               (formerly src/Render/SpaceQuad.cpp)
//!   sharpen_pass  CAS sharpening of a captured eye, in place or into another target (formerly src/Render/SharpenPass.cpp)
pub mod caption;
pub mod color_blit;
pub mod command_resources;
pub mod depth_capture;
pub mod depth_commit_gate;
pub mod depth_resolve;
pub mod desktop_mirror;
pub mod display_modes;
pub mod finer_vrs;
pub mod foveated_gpu;
pub mod frame_capture;
// DLSS frame generation's camera matrices (src/Framegen/DlssFrameGen.cpp, BuildMatrices).
pub mod dlssg_matrices;
pub mod frame_timer;
pub mod framegen_core;
pub mod framegen_overlay;
pub mod geometry_checkpoint;
pub mod geometry_compare;
pub mod gpu_diag;
pub mod group_resolve;
pub mod hardware_telemetry;
pub mod hud_panel;
pub mod input_copy;
pub mod loading_screen;
pub mod mono_cascades;
pub mod optics;
pub mod pipeline_variants;
pub mod optics_camera;
pub mod optics_probe;
pub mod optics_source;
pub mod osr;
pub mod output_copy;
pub mod queue_timeline;
pub mod renderdoc_bridge;
pub mod scene_profile;
pub mod scene_state;
pub mod sharpen_pass;
pub mod space_quad;
pub mod shader_ab;
pub mod dxil;
#[cfg(any(test, feature = "bench"))]
pub mod bench;
#[cfg(test)]
mod shader_bench;
pub mod srgb_pass;
pub mod stage_profile;
pub mod stereo_depth;
pub mod target_array;
pub mod texture_lease;
pub mod vrs_snapshot;
