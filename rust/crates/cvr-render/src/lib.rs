//! The port's render policies (stage 5, docs/rust-migration-plan.md): the settings and the pure geometry of the
//! render passes, ahead of the passes themselves. The C++ headers call these in the Rust build.
//!
//!   view_chain_timeline  research probe: the frame's views and jobs timed onto the node timeline (formerly
//!              src/Stereo/ViewChainTimeline.cpp)
//!   view_constants  what each view's shaders are told: the view-constant blocks per node and the camera buffers paired
//!              across the views (formerly src/Stereo/ViewConstants.cpp)
//!   view_twin_compiler  the DXC chain that builds, validates and signs a shader's view-instanced twin
//!              (formerly src/Render/ViewTwinCompiler.cpp)
//!   view_twin_ir  the text rewrite that makes an engine-stereo material shader view-instanced
//!              (formerly src/Render/ViewTwinIr.cpp)
//!   vision_layer  the scanner outline's surface identified for the second eye, with the node and vision maps (formerly
//!              src/Stereo/VisionLayer.cpp)
//!   vrcam_config  vrcam.json's scanners and the names derived from it (include/Stereo/VrcamConfig.hpp's helpers)
//!   vrcam_selection  which second-eye camera this session uses, and the watcher that notices when it changes (formerly
//!              src/Stereo/VrcamSelection.cpp)
//!   vrs        fixed foveated variable-rate shading: its settings, the rate map, the pipelines it may touch
//!              (formerly src/Render/FixedVrsSettings.cpp)
//!   eager_depth  eager native depth preparation: MAIN presorts the vrcam view's packets, verified against the engine
//!              (formerly src/Render/NativeDepthPreparation.cpp)
//!   fixed_vrs  fixed foveated VRS on the GPU: per-list rate state, the rate maps, scene draws bound to them
//!              (formerly src/Render/FixedVrs.cpp)
//!   engine_stereo_sdk  the Odyssey 3D SDK in the plugin: the wrapper handed our factory and interface, stereo from the
//!              main menu on (formerly src/Stereo/EngineStereoSdk.cpp)
//!   framegen_inputs  framegen's inputs: each eye's depth, motion and camera from Streamline or the engine's TAA and
//!              upscaler nodes, copied and completed per submission (formerly src/Framegen/Inputs.cpp, NativeInputs.cpp)
//!   view_reuse  the second view reuses MAIN's sky, distant shadows, reflection probes, local shadows and GI (the
//!              producer-skip detours of formerly src/Stereo/ViewReuse.cpp)
//!   frame_graph  what the engine's frame graph decides for each view: the build observers, FlagCompute's rect,
//!              environment, water and feature policy for the second view and the scope, the rect commit and the
//!              water assign, the RTT component's view-create (formerly src/Stereo/FrameGraph.cpp)
//!   fog_history  the render-parity data model and the fog history's arithmetic: the render owner, MAIN's AA and
//!              sky radiance samples, the fog borrow's eligibility, the unjittered fog projection, the graph-cache split
//!              (formerly include/Stereo/RenderParity.hpp, FogHistory.hpp; tools/render_parity_tests' checks as tests)
//!   descriptor_cache  RTV metadata by descriptor handle, LRU with the swapchain pinned, and the stereo module's RTV
//!              cache (formerly include/Render/DescriptorCache.hpp; tools/descriptor_cache_tests' checks as tests)
//!   render_parity  the two views under the same settings: MAIN's AA and sky radiance given to the second view at
//!              its FlagCompute, the second view's graph-cache key, MAIN's fog history reprojected into the second
//!              view's scattering (formerly src/Stereo/RenderParity.cpp)
//!   cull_visibility  the engine's culling per view: the detail threshold, the occlusion gate, the graph context's
//!              paired preparation and the cull reuse modes, DoCulling, the preparation stage profiler, the visibility
//!              replay and the materialize detours (formerly src/Stereo/CullVisibility.cpp)
//!   node_features  what a view may run, asked per node: the feature-bit test and its probe, the capability test
//!              (the scope's weapon plane, AutoGrass readiness, the refusal census and grant, the HUD override), the
//!              pass driver without refused weapon passes (formerly src/Stereo/NodeDispatch.cpp, part)
//!   node_probes  the node dispatch's probes: the per-view node census, the cloud state at the clouds node, the
//!              cloud shader selector, the sky record, the scope's render probe (formerly src/Stereo/NodeDispatch.cpp)
//!   hud        the HUD into the second eye: the surface named by its node, the descriptor path and its hold, the
//!              snapshots on the engine's list, the composite constants from the upload ring, the capability grant
//!              (formerly src/Stereo/Hud.cpp)
//!   capture_rtv  which render target a descriptor names: every RTV's metadata, the second view's output candidates,
//!              the port's own resources excluded (formerly src/Stereo/Capture.cpp, part)
//!   capture_output  the second view's output as captured: published with its watchdog, our own committed target,
//!              the cascade pass's depth targets (formerly src/Stereo/Capture.cpp, part)
//!   capture_stable  the second eye's stable snapshot in the engine's own list, the eye texture the submit reads,
//!              the finished frame's epilogue copy, the desktop mirror's texture and copy
//!              (formerly src/Stereo/Capture.cpp, part)
//!   capture_hooks  Capture's share of the bind hook: the cascade and HUD-node probes, the bound size, our own
//!              target substituted, the tonemap pass and the mirror's output latched (formerly Capture.cpp, part)
//!   census_probes  the probes on the barrier hook's transitions: the exposure mirror and readback, the light grid,
//!              the small UAV counters (formerly src/Stereo/CommandListCensus.cpp, part)
//!   census_pso  the pipelines' shaders named at creation, the draw probe, the sight's shaders and the 6-index quad
//!              tally, the buffer and upload address maps (formerly src/Stereo/CommandListCensus.cpp, part)
//!   census_draws  the sight's axis per view, the grading LUT's source lent while fresh, the draw census, the
//!              cascade census (formerly src/Stereo/CommandListCensus.cpp, part)
//!   grading_lends  the blocks one view lends the other on the way to the GPU: the sway clocks, the fog and projection
//!              jitter, the cascade sampling matrices and camera, the dither lock (formerly Grading.cpp, part)
//!   grading_probes  the field-by-field probes on the uploads: checker, sway diff and shipped, the retargetable block
//!              diff, the cascade matrices, the grid origin (formerly Grading.cpp, part)
//!   grading_uploads  the buffer uploader: the AO, exposure, marker and HUD rewrites, the lends and probes in their
//!              order, the upload notes and the optics snapshots (formerly Grading.cpp's Detour_BufUpload)
//!   grading_lut  the grading composer's probe and GenerateTonemappingLUT: the optics snapshot, the grading-CB
//!              capture around the build, the source lend (formerly Grading.cpp, part)
//!   afw  alternate frame warping: OSR or PureDark's plugin makes the eye the engine did not draw this frame, and
//!        afw_cameras the plugin's two cameras (formerly src/Render/Afw.cpp)
//!   foveated_dlss  foveated DLSS's NGX hooks: the centre and periphery evaluates, the timings, the JSON report
//!        (formerly src/Render/FoveatedDlss.cpp)
//!   swapchain  the plugin's bootstrap (the DXGI factory hooks, the swapchain's creation) and the swapchain's hooks:
//!        Present, the frame's hub (formerly src/Hooks/SwapChain.cpp)
//!   worker  the worker thread's polls and telemetry dump, the guarded engine-memory dumps (formerly
//!        src/Core/WorkerThread.cpp and src/Core/Log.cpp)
//!   camera_owner  which camera the PatchCamera site has in hand, and its fast path (formerly part of
//!        src/Core/VrCore.cpp)
//!   camera_takeover  the taken-over and the braindance camera: who owns the frame, the lens bound, re-read and
//!        written, the braindance camera found by pose, the releases, the shared slots (formerly src/Core/VrCore.cpp)
//!   device_creation  the device's vtable patched once; descriptor heaps, committed and placed resources and root
//!        signatures created and announced; creation counts, stacks, video memory, streaming placed
//!        (formerly src/Stereo/DeviceHooks.cpp, part)
//!   vr_core_settings  the launcher's startup sequence and setters, the forced sizes, the live settings' answers by
//!        their C names, the game settings' resolution override (formerly part of src/Core/VrCore.cpp)
//!   live_controls_poll  vrport.ini's poll: the live state seeded, the ini read over it, the result applied with its
//!        clamps; the CET recenter request, the F7/F8 hotkeys (formerly src/Core/LiveControlsPoll.cpp's poll)
//!   live_controls_ui  the overlay's snapshot of the live controls: taken, applied with the ini path's clamps, and
//!        written to vrport.ini whole (formerly src/Core/LiveControlsPoll.cpp's UI state)
//!   runtime_paths  where this build's files are, the live controls' launch defaults, vrport.ini written when missing
//!        (formerly src/Core/LauncherConfig.cpp)
//!   debug_gate_table  the launcher DEBUG gate's table of probes by their exported names, and ApplyLauncherDebugGate
//!        (formerly src/Utils/DebugGate.cpp's kFlags)
//!   ui, overlay_panels  the settings panels' widgets (ImGui or the VR menu's recorder, the C++ half's) and the panels
//!        written against them (formerly src/Overlay/OverlayPanels.cpp)
//!   cascade_sampling  the cascades' active prefix and layout match, the gap along the light (CascadeSampling.hpp)
//!   node_dispatch  the node dispatcher, once per node per view: attribution, the thread's node and view, the view
//!              side, the grants and probes, the profiler, the epilogue snapshots (formerly NodeDispatch.cpp's hub)
//!   mirror     the desktop mirror: the game-side copy or HUD composite with the readback probes on its fence,
//!              and the present thread with its own queue, window and converting pass (formerly src/Stereo/Mirror.cpp)
//!   stereo_dlss  Streamline and DLSS made to agree about two views: MAIN's ctx pinned, the history fix, VRCAM's camera
//!              and its own DLSS viewport, cache and render size (the detours of formerly src/Stereo/Dlss.cpp)
//!   single_pass  one view-instanced draw for both eyes of an engine-stereo RenderElements draw pair: the lists'
//!              state, the kept left draws, the chained command-list detours (formerly src/Stereo/SinglePass.cpp)
//!   single_pass_pipelines  single pass's material roots and pipelines: the twin root, the owned descriptions on the
//!              PSOs, the view-instanced twins built in the background (formerly src/Render/SinglePassPipelines.cpp)
//!   pipeline_stream  D3D12 pipeline state streams walked, read, described, edited and rebuilt with view
//!              instancing, and the view-instanced pipelines created from them (formerly src/Render/ViewInstancedPipeline.cpp)
//!   framegen_generator  the frame generator: FidelityFX interpolation with its own or NVIDIA's optical flow, DLSS-G
//!              through the C++ adapters (formerly src/Framegen/Generator.cpp)
//!   foveated   foveated DLSS: its settings, the delayed application, the full-resolution centre
//!              (formerly src/Render/FoveatedDlssSettings.cpp)
//!   game_thread_queues  the game thread's dequeue without render jobs, and the world tick's wait lending its idle time
//!              to the Lua host's collector (formerly src/Stereo/GameThreadQueues.cpp)
//!   hidden_area  the hidden-area mask's geometry: its settings, the hidden directions, the rectangles a target skips,
//!              the shaders' ellipse (formerly src/Render/HiddenAreaSettings.cpp)
//!   depth_policy  which shaders single pass may run an extra time: read-only, one layer, no ViewID
//!              (formerly src/Render/NativeDepthPolicy.cpp)
//!   cpu_collector  the owner of one native CPU visibility collector (formerly src/Render/NativeCpuCollector.cpp)
//!   dxil_patch  in-place constant patches of signed DXIL containers: the DXBC hash, the bitcode range, the VBR6
//!              floats (formerly src/Render/DxilPatch.cpp)
//!   indirect_policy  the pure draw kinds of the engine's indirect command signatures, kept in their private data
//!              (formerly src/Render/NativeIndirectPolicy.cpp)
//!   instance_observer_plan  the instance range a native writer filled, and the per-instance layouts it can read
//!              (formerly src/Render/NativeInstanceObserverPlan.cpp)
//!   luma_probe  the brightness probes: the image block, the constant buffer and the tonemap inputs read back and split by
//!              the frame's output state, and the outline layer's dump (formerly src/Stereo/LumaProbe.cpp)
//!   native     single pass's native graph and resource records, ported bottom-up from include/Render/Native*.hpp
//!   pair_probe  the L/R pair probe: the bound state of each list, the probed node's draws recorded, the single-pass
//!              feasibility substitution (formerly src/Stereo/StereoPairProbe.cpp)
//!   plane_patches  the three plane knobs: the routing branch, the highlight's plane filter, the plane as the draw list's
//!              index (formerly src/Stereo/WeaponPlane.cpp)
//!   paired_root  single pass's paired root signature: a pair of descriptors per bound resource, per stage
//!              (formerly src/Render/StereoPairedRoot.cpp)
//!   submit_chain  the submission points chained in order, and the presenting view's Present deferred to an engine
//!              job (formerly src/Stereo/SubmitChain.cpp)
//!   stereo_profiler  the stereo module's CPU profiler: per-view accumulators, the node and pass tables, the timeline,
//!              the node cut set and the audit dump (formerly src/Stereo/Profiler.cpp)
//!   render_graph_shape  research probe: each render graph's nodes, edges, depth and width (formerly
//!              src/Stereo/RenderGraphShape.cpp)
//!   scope_graph_dag  the camera scope's render graph built with MAIN's dependency rules (formerly
//!              src/Stereo/ScopeGraphDag.cpp)
//!   shader_patches  the shader twins (hidden-area mask and the fixed groups) bound in place of the engine's pipelines,
//!              and the shader census (formerly src/Render/HiddenAreaShaders.cpp)
//!   single_pass_trace  the opt-in CPU trace of single pass's camera uploads and command lists
//!              (formerly src/Render/SinglePassTrace.cpp; the SEH-guarded stack read stays C++)
//!   stereo_probe  the parts of single pass's native stereo probe moved so far (src/Render/NativeStereoProbe.cpp)
//!   upscaler_views  one temporal-upscaler history per eye for FSR 3.0, the FidelityFX API and XeSS, and the
//!              FidelityFX providers detour (formerly src/Stereo/UpscalerViews.cpp)
//!   preparation_trace  the native preparation trace and the pair preparation diagnostics (formerly
//!              src/Render/NativePreparation.cpp)
//!   submission_frontier_owner  the submission frontier probe's hook, rows, ledger and boundary (formerly
//!              src/Render/NativeSubmissionFrontier.cpp)
//!   submission_frontier_owner  the submission frontier probe's hook, rows, ledger and boundary (formerly
//!              src/Render/NativeSubmissionFrontier.cpp)
//!   bounds_trace  the bounds and particle trace of a pool the CPU collector probe traces (formerly
//!              src/Stereo/NativeBoundsTrace.cpp)
//!   scene_update  the scene-update probe: its thunks' scopes, their liveness and the published receipt (formerly
//!              src/Stereo/NativeSceneUpdate.cpp)
//!   scene_update_receipt  the scene-update probe's source receipt (formerly in include/Render/NativeSceneUpdate.hpp)
//!   geometry_packets  single pass's opt-in native packet capture and the draw geometry of the native flushes
//!              (formerly src/Render/NativeGeometryPackets.cpp)
//!   upload_shadow  the bytes single pass's watched buffer received from CPU uploads (formerly
//!              src/Render/NativeUploadShadow.cpp)
//!   world_marker_shader  the world markers' replacements of the engine's flat ink vertex shaders
//!              (formerly src/Render/WorldMarkerShader.cpp)
pub mod asym_projection;
pub mod block_rewrites;
pub mod cpu_collector;
#[cfg(any(feature = "rust-build", test))]
pub mod collector_probe;
pub mod scene_visibility;
pub mod depth_policy;
pub mod dxil_patch;
pub mod eager_depth;
pub mod engine_stereo;
#[cfg(feature = "rust-build")]
pub mod engine_stereo_dlss;
#[cfg(feature = "stereo")]
pub mod engine_stereo_sdk;
pub mod fixed_vrs;
#[cfg(feature = "rust-build")]
pub mod framegen_inputs;
#[cfg(feature = "rust-build")]
pub mod framegen_generator;
/// FSR3 interpolation and extrapolation per eye (the frame generator's backends 4 and 5).
#[cfg(feature = "rust-build")]
pub mod fsr3;
/// DLSS 5 Neural Rendering on each eye after the game's DLSS (experimental, off at every start).
#[cfg(feature = "rust-build")]
pub mod neural_render;
/// The D3D12 resource lifetime census (main's 1f8f77cf8): created / released / live per description and caller.
pub mod resource_census;
/// Native dynamic texture construction origins (main's 730f5d78a): the factory's callers, an optional ring.
pub mod texture_origins;
/// Owned publications of GPU resources: the mirror's output and the tonemap input capture (main's 8255f379b, c41b2e23d).
pub mod publication;
pub(crate) mod framegen_shaders;
/// FSR3's dilate and warp and Neural Rendering's codec as main's HLSL: what they draw with (cvr_gpu::dxil::pick) and
/// the WARP bench's reference.
pub(crate) mod fsr3_shaders;
// Native DLSS Frame Generation through NGX (formerly src/Framegen/DlssFrameGen.cpp's; NGX's entries lent by the .cpp).
#[cfg(feature = "rust-build")]
pub mod dlssg_native;
pub mod foveated;
#[cfg(feature = "stereo")]
pub mod game_thread_queues;
pub mod hidden_area;
pub mod hidden_area_runtime;
/// The hidden area's runtime table (formerly src/Render/HiddenArea.cpp's).
#[cfg(feature = "rust-build")]
pub mod hidden_area_table;
pub mod indirect_policy;
pub mod instance_observer_plan;
pub mod intro_boot;
pub mod native;
#[cfg(feature = "rust-build")]
pub mod ngx;
pub mod notice;
#[cfg(feature = "stereo")]
pub mod luma_probe;
#[cfg(feature = "stereo")]
pub mod lobby_natives;
pub mod menu_lobby;
#[cfg(feature = "stereo")]
pub mod pair_probe;
pub mod pipeline_stream;
pub mod pso_twins;
pub mod paired_root;
#[cfg(feature = "stereo")]
pub mod plane_patches;
#[cfg(feature = "stereo")]
pub mod render_graph_shape;
#[cfg(feature = "stereo")]
pub mod scope_graph_dag;
#[cfg(feature = "rust-build")]
pub mod anr_fg;
#[cfg(feature = "rust-build")]
pub mod debug_draw_stereo;
pub mod scope_dlss;
pub mod scope_resolution;
pub mod vehicle_audio;
pub mod shader_patches;
#[cfg(feature = "rust-build")]
pub mod single_pass;
#[cfg(feature = "rust-build")]
pub mod stereo_dlss;
#[cfg(feature = "rust-build")]
pub mod view_reuse;
#[cfg(feature = "rust-build")]
pub mod frame_graph;
pub mod fog_history;
pub mod descriptor_cache;
#[cfg(feature = "rust-build")]
pub mod render_parity;
#[cfg(feature = "rust-build")]
pub mod cull_visibility;
#[cfg(feature = "rust-build")]
pub mod node_features;
/// The first-person geometry's object velocity in the late scene velocity passes (main's 269e9ee6d).
pub mod scene_velocity;
#[cfg(feature = "rust-build")]
pub mod node_probes;
/// The scalar g_* globals of SyncStereo, the ImGui overlay, the profiler, the frame graph, the device hooks, the camera
/// pair, the swap chain, the HUD and the node dispatcher (the Rust build of the plugin defines them; formerly cvr-ffi's).
#[cfg(feature = "rust-build")]
pub mod moved_globals;
/// src/Core/VrCore.cpp's scalar globals (the Rust build of the plugin defines them; formerly cvr-ffi's).
#[cfg(feature = "rust-build")]
pub mod vrcore_exports;
/// The stereo hooks' shared objects the capture, mirror and HUD halves read (formerly C++ globals) and the services
/// their runtime tables took from the C++.
#[cfg(feature = "rust-build")]
pub mod stereo_shared;
#[cfg(feature = "rust-build")]
pub mod hud;
#[cfg(feature = "rust-build")]
pub mod capture_rtv;
#[cfg(feature = "rust-build")]
pub mod capture_output;
#[cfg(feature = "rust-build")]
pub mod capture_stable;
#[cfg(feature = "rust-build")]
pub mod capture_hooks;
#[cfg(feature = "rust-build")]
pub mod census_probes;
#[cfg(feature = "rust-build")]
pub mod census_pso;
#[cfg(feature = "rust-build")]
pub mod census_draws;
#[cfg(feature = "rust-build")]
pub mod grading_lends;
#[cfg(feature = "rust-build")]
pub mod grading_probes;
#[cfg(feature = "rust-build")]
pub mod grading_uploads;
#[cfg(feature = "rust-build")]
pub mod grading_lut;
#[cfg(feature = "rust-build")]
pub mod afw;
#[cfg(feature = "rust-build")]
pub mod foveated_dlss;
#[cfg(feature = "rust-build")]
pub mod swapchain;
// The Rust hooks' registrations (formerly their CVR_HOOK lines).
#[cfg(feature = "rust-build")]
pub mod hook_table;
// The native setter tracers (formerly src/Hooks/NativeSetter*.cpp).
#[cfg(feature = "rust-build")]
pub mod setter_tracers;
pub mod swapchain_fit;
#[cfg(feature = "rust-build")]
pub mod worker;
#[cfg(feature = "rust-build")]
pub mod camera_owner;
#[cfg(feature = "rust-build")]
pub mod camera_takeover;
#[cfg(feature = "rust-build")]
pub mod device_creation;
#[cfg(feature = "rust-build")]
pub mod vr_core_settings;
#[cfg(feature = "rust-build")]
pub mod live_controls_poll;
#[cfg(feature = "rust-build")]
pub mod live_controls_ui;
#[cfg(feature = "rust-build")]
pub mod runtime_paths;
#[cfg(feature = "rust-build")]
pub mod debug_gate_table;
#[cfg(feature = "rust-build")]
pub mod ui;
#[cfg(feature = "rust-build")]
pub mod ui_rec;
#[cfg(feature = "rust-build")]
pub mod overlay_panels;
/// The eyes' rendered windows and the overlay's projection window (formerly OpenXRManager.cpp's and
/// OverlayProjection.cpp's).
#[cfg(feature = "rust-build")]
pub mod eye_windows;
/// The C++ half's former one-line services into Rust, under their C names (cvr_cpp_*).
#[cfg(feature = "rust-build")]
pub mod cpp_services;
#[cfg(feature = "rust-build")]
pub mod overlay_barrel;
#[cfg(feature = "rust-build")]
pub mod overlay_debug_view;
#[cfg(feature = "rust-build")]
pub mod overlay_crosshair;
#[cfg(feature = "rust-build")]
pub mod overlay_telemetry;
#[cfg(feature = "rust-build")]
pub mod overlay_hands;
#[cfg(feature = "rust-build")]
pub mod overlay_exports;
// The FBT calibration helpers (src/Fbt/Game/Helpers.cpp, DrawFbtCalibrationHelpers).
#[cfg(feature = "rust-build")]
pub mod overlay_fbt_helpers;
// The FBT calibration in the game (src/Fbt/Game/Calibration.cpp): the steps, the cross, the texts and the file.
#[cfg(feature = "rust-build")]
pub mod fbt_calibration;
// The retained geometry's exported state (src/Render/NativeRetainedGeometry.cpp's export definitions and their reset).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod retained_exports;
// The prepared pair's exported state (src/Render/NativePreparedPair.cpp's export definitions and its current-sequence reset).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod prepared_pair_state;
// The exported state of five native stereo sources (their export definitions).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod native_state_exports;
// The native early depth's and the CPU collector probe's exported state (their export definitions).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod early_depth_exports;
// The constant upload registry's exported state (its export definitions).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod constant_registry_exports;
// The device hooks' replacement shaders (src/Stereo/DeviceHooks.cpp, sight_blob_ready and vision_cs_blob_ready).
pub mod shader_blobs;
// The sight's root signature dump (src/Stereo/DeviceHooks.cpp, rootsig_dump).
pub mod rootsig_lines;
// The mapped upload heaps (src/Stereo/SyncStereo.cpp's upload_map_read and its table, ViewConstants.cpp's upload_cpu_for_va).
pub mod upload_maps;
// The constant-buffer view hook's probes (src/Stereo/Capture.cpp, hk_CreateCBV after the camera upload).
#[cfg(feature = "rust-build")]
pub mod capture_cbv;
// The frame graph resolver's post-colour watch (src/Stereo/SyncStereo.cpp, Detour_Resolve3D20).
pub mod resolve_post_color;
// Exports no C++ of the Rust build reads any more: the input tunables, world markers, profiler and render parity.
#[cfg(feature = "rust-build")]
pub mod input_exports;
// Exports no C++ of the Rust build reads any more: culling, core, hidden-area shaders, stereo sync.
#[cfg(feature = "rust-build")]
pub mod stereo_moved_exports;
// Exports no C++ of the Rust build reads any more: the remaining sources (view reuse, grading, frame graph, OpenXR, ...).
#[cfg(feature = "rust-build")]
pub mod render_moved_exports;
// Exports the C++ of the Rust build reads through its header declarations, defined in Rust.
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
pub mod declared_moved_exports;
// Exported variables the C++ half still uses, defined here (each source keeps a declaration).
#[cfg(feature = "rust-build")]
pub mod used_moved_exports;
// The rest of them (the sources after used_moved_exports' eleven).
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
pub mod used_moved_exports_rest;
// The exports the generated moves left: float atomics, report strings, named sizes (hand-ported).
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
pub mod typed_moved_exports;
// The export modules' definitions tools/vrik_tests' Rust variants need (generated: tools/rust_migration/rig_test_exports.py).
#[cfg(all(feature = "rig-exports", not(feature = "rust-build"), not(feature = "probe-exports")))]
pub mod rig_test_exports;
// The hidden-area shader patches' and the shader census' switches and counters (formerly HiddenAreaShaders.cpp's).
#[cfg(feature = "rust-build")]
pub mod shader_patch_exports;
// The HUD composite's parameters and the constants only they read (src/Stereo/Hud.cpp, hud_composite_params).
#[cfg(any(feature = "rust-build", test))]
pub mod hud_composite;
// The per-view render-mask report (src/Stereo/FrameGraph.cpp, render_mask_report).
pub mod render_mask_report;
// Which engine view is MAIN (src/Stereo/SyncStereo.cpp, is_main_view).
pub mod main_view_check;
// The XR diagnostics' clock (src/Runtimes/OpenXRFrameLoop.cpp, XrDiagNowMs).
pub mod xr_diag_clock;
// The braindance camera's name hash and the headset's default FOV (SyncStereo.cpp, OpenXRManager.cpp).
pub mod vrcam_names;
// The upload reservations' runtime (src/Render/NativeUploadReservation.cpp).
pub mod upload_reservation_runtime;
// The instance reservation witness's runtime (src/Render/NativeInstanceWitness.cpp).
pub mod instance_witness_runtime;
// The cubic-dispatch probe (src/Stereo/CommandListCensus.cpp, volume_node_note).
pub mod volume_node_census;
// The stage replay's call ordinals and configuration (src/Render/StageReplay.cpp).
pub mod stage_replay_frame;
// The shader subobjects of a pipeline stream (src/Render/ViewInstancedPipeline.cpp, IsShaderPart).
pub mod pipeline_subobjects;
// A stereo depth comparison's clear depth (src/Render/StereoDepthComparison.cpp, ClearFor).
pub mod depth_clear_value;
// jsoncpp's value tree as the C++ half lends it flattened (include/Rust/JsonTree.hpp).
pub mod jsoncpp_tree;
// The original source selectors' package and evaluation (src/Render/SourceSelectorPlan.cpp).
pub mod source_selector_plan;
#[cfg(feature = "rust-build")]
pub mod imgui;
#[cfg(feature = "rust-build")]
pub mod vr_style;
#[cfg(feature = "rust-build")]
pub mod vr_widgets;
#[cfg(feature = "rust-build")]
pub mod imgui_overlay;
#[cfg(feature = "rust-build")]
pub mod xr_cycle_end;
#[cfg(feature = "rust-build")]
pub mod xr_capture;
#[cfg(feature = "rust-build")]
pub mod xr_capture_state;
// The Present's XR host and the mono frame's capture (formerly OpenXRPresent.cpp's and OpenXRCapture.cpp's).
#[cfg(feature = "rust-build")]
pub mod xr_present_host;
// The loading screen the capture shows (formerly OpenXRCapture.cpp's services).
#[cfg(feature = "rust-build")]
pub mod loading_host;
#[cfg(feature = "rust-build")]
pub mod lobby_intro;
/// The lobby's runtime tables (formerly src/Lobby/MenuLobby.cpp's and Intro.cpp's).
#[cfg(feature = "rust-build")]
pub mod lobby_tables;
/// The stereo module's exported functions (formerly SyncStereo.cpp's, Hud.cpp's, Profiler.cpp's and others').
#[cfg(feature = "rust-build")]
pub mod stereo_api;
/// The asymmetric projection's, engine stereo's and Streamline's runtime tables (formerly C++'s).
#[cfg(feature = "rust-build")]
pub mod stereo_tables;
#[cfg(feature = "rust-build")]
pub mod framegen_presenter;
#[cfg(feature = "rust-build")]
pub mod depth_layer;
#[cfg(feature = "rust-build")]
pub mod xr_submit;
#[cfg(feature = "rust-build")]
pub mod xr_frame_loop;
// OpenXRManager's frame loop: the cycle's host, the inline pump, the submit thread and the graphics binding.
#[cfg(feature = "rust-build")]
pub mod xr_manager_loop;
/// The XR manager's frame objects (formerly the C++ OpenXRManager singleton's members).
#[cfg(all(feature = "rust-build", not(test)))]
mod xr_frame_objects;
#[cfg(feature = "rust-build")]
pub mod stereo_exports;
pub mod afw_cameras;
pub mod cascade_sampling;
#[cfg(feature = "rust-build")]
pub mod node_dispatch;
#[cfg(feature = "rust-build")]
pub mod mirror;
pub(crate) mod mirror_shaders;
#[cfg(feature = "rust-build")]
pub mod single_pass_pipelines;
pub mod single_pass_trace;
pub mod ssr_merge;
pub mod start_screen;
pub mod streamline;
#[cfg(feature = "stereo")]
pub mod stereo_profiler;
#[cfg(feature = "stereo")]
pub mod submit_chain;
pub mod stereo_probe;
pub mod native_recording;
pub mod instance_ring;
pub mod geometry_chain;
pub mod upload_registry;
pub mod buffer_read;
pub mod texture_read;
pub mod texture_receipts;
pub mod texture_lease;
pub mod source_cbv_details;
pub mod texture_immutable;
pub mod texture_pins;
pub mod texture_writable;
pub mod queue_residency;
pub mod target_lifetime;
pub mod ia_snapshot;
pub mod structured_snapshot;
pub mod upload_reservation;
pub mod source_srv;
pub mod descriptor_views;
pub mod material;
pub mod material_observe;
pub mod material_read;
pub mod breadcrumb;
pub mod ia_binding;
pub mod camera_binding;
pub mod camera_flush;
pub mod eye_membership;
pub mod descriptor_arena;
pub mod skinning_read_plan;
pub mod material_readback;
pub mod fine_visibility;
pub mod instance_witness;
pub mod camera_input;
pub mod instance_data;
pub mod instance_coverage;
pub mod instance_rebase;
pub mod draw_origin;
pub mod scene_child;
pub mod multilayer_selectors;
pub mod gbuffer_material;
pub mod paired_frontier;
pub mod paired_cpu_frontier;
pub mod scene_sections;
pub mod graph_definition;
pub mod context_order;
pub mod owned_native_graph;
pub mod scene_expansion;
pub mod preparation_schedule;
pub mod constant_timeline;
pub mod geometry_write_receipt;
pub mod shader_camera;
pub mod world_marker_abi;
pub mod render_parity_abi;
pub mod vrcam_config_abi;
pub mod indirect_copy;
pub mod indirect_copy_native;
pub mod source_cbv_observe;
pub mod instance_snapshot_view;
pub mod instance_snapshot_arena;
pub mod audit_source_capture;
pub mod packet_inputs;
pub mod source_cbv_binding;
pub mod source_cbv_snapshot;
pub mod material_buffer_snapshot;
pub mod target_handoff;
pub mod target_handoff_observer;
pub mod target_access;
pub mod target_access_observer;
pub mod scene_access_observer;
// The observer front over scene_access_observer.rs' abi (its C ABI only outside unit tests, and only where its .cpp's Rust
// branch is compiled: the plugin and the probe test).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod scene_access_native;
// The native early depth's orchestration (formerly src/Render/NativeEarlyDepth.cpp; its exports and the moved ones).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod early_depth_native;
// The native shader snapshots' owners and reservations (formerly src/Render/NativeShaderSnapshot.cpp).
pub mod shader_snapshot_native;
// The prepared pair's orchestration (formerly src/Render/NativePreparedPair.cpp).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod prepared_pair_native;
// The assembled source binding tables and their SRV coverage receipts (formerly SourceBindingAssembly.cpp's issue).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod shared_table;
// prepared_pair::Source owned on the Rust side and its lent views (formerly NativeRetainedGeometry.cpp's StagePairSource / NativePreparedPair.cpp's Aux).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod pair_source;
// The native target cluster's proofs owned on the Rust side (formerly the C++ holders of the target identity, lifetime, handoff and access).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod targets;
// The retained geometry capture's control front (part of src/Render/NativeRetainedGeometry.cpp).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod retained_geometry_native;
// The native descriptor arena (formerly src/Render/NativeDescriptorArena.cpp).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod native_descriptor_arena;
// The native instance observer (formerly src/Render/NativeInstanceObserver.cpp).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod instance_observer_native;
// The instance snapshot view's owners and versions (formerly the C++ half of src/Render/InstanceSnapshotView.cpp).
pub mod instance_snapshot_native;
pub mod native_exit;
pub mod submission_frontier;
pub mod retirement_deferral_abi;
pub mod definition_generations_abi;
pub mod owned_graph_execution;
pub mod residency_transaction;
pub mod resource_replay;
pub mod stage_replay;
pub mod descriptor_store;
pub mod copy_windows;
pub mod borrowed_fixed;
pub mod source_layout;
pub mod root_layout;
pub mod creation_receipts;
pub mod registered_copies;
pub mod source_intent;
pub mod bank_layout;
pub mod assembly_layout;
pub mod multilayer_membership;
pub mod binding_bank;
pub mod geometry_commands;
pub mod audit_values;
pub mod audit_lifetimes;
pub mod audit_lookup;
pub mod audit_capture;
pub mod audit_writes;
pub mod descriptor_audit;
pub mod audit_table_snapshot;
pub mod audit_borrowed_ranges;
pub mod audit_source_tables;
pub mod stereo_probe_state;
pub mod queue_dispatch;
#[cfg(feature = "rust-build")]
pub mod list_vtable;
// The ExecuteCommandLists runtime (formerly src/Stereo/DeviceHooks.cpp's kEclRuntime).
#[cfg(feature = "rust-build")]
pub mod ecl_table;
// The command lists' detours (formerly src/Stereo/CommandListCensus.cpp's hooks and DeviceHooks.cpp's observers).
#[cfg(feature = "rust-build")]
pub mod list_detours;
// The single-pass engine probes (formerly src/Stereo/Grading.cpp's BindCameraProbe, BindMaterialProbe, RenderPacketsProbe).
#[cfg(feature = "rust-build")]
pub mod single_pass_probes;
// Engine detours whose hooks and originals are Rust's, registered from Rust.
#[cfg(feature = "rust-build")]
pub mod engine_detours;
// The engine hooks the install pass creates directly (formerly SyncStereo.cpp's).
#[cfg(feature = "rust-build")]
pub mod engine_hooks;
// The stereo module's start: InitStereoOnce, InitOpenXREarly, the D3D12CreateDevice hook and the engine pass (formerly
// VrCore.cpp's, SyncStereo.cpp's and DetourRegistry.cpp's).
#[cfg(all(feature = "rust-build", not(test)))]
pub mod stereo_start;
// The assembly thunks (formerly src/Hooks/*.asm).
#[cfg(all(feature = "rust-build", not(test)))]
mod asm_thunks;
// The definition tracking's C++ wrapper classes in Rust (their headers stay for the C++ build and the tests).
pub mod definition_wrappers;
#[cfg(feature = "rust-build")]
pub mod api_exports;
#[cfg(feature = "rust-build")]
pub mod descriptor_hooks;
#[cfg(feature = "rust-build")]
pub mod export_attach;
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod source_srv_observe;
pub mod ia_snapshot_receipt;
pub mod breadcrumb_capture;
// Needs source_srv_observe (the same gate).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod structured_snapshot_receipt;
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod material_snapshot_receipt;
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod source_cbv_snapshot_receipt;
#[cfg(feature = "rust-build")]
pub mod pso_hooks;
// The paired frame's engine hooks (formerly src/Stereo/NativePairedFrame.cpp).
#[cfg(feature = "rust-build")]
pub mod paired_frame;
// The native definition tracking (formerly src/Stereo/NativeDefinitionTracking.cpp).
#[cfg(feature = "rust-build")]
pub mod definition_tracking;
pub mod audit_roots;
pub mod audit_tables;
pub mod audit_origins;
pub mod audit_metadata;
pub mod audit_snapshots;
pub mod audit_borrowed;
pub mod audit_sources;
pub mod audit_source_checks;
pub mod audit_inventory;
pub mod audit_fixed;
pub mod audit_emit;
pub mod instance_layout;
pub mod early_depth_checks;
pub mod early_depth_settings;
pub mod early_depth_admission;
pub mod early_depth_skin;
pub mod ia_snapshot_checks;
pub mod allocation_identity;
pub mod ia_snapshot_identity;
pub mod ia_snapshot_native;
pub mod ia_snapshot_objects;
pub mod ia_snapshot_shape;
pub mod ia_snapshot_inspect;
pub mod ia_snapshot_backing;
pub mod ia_snapshot_record;
pub mod ia_snapshot_state;
pub mod ia_snapshot_collect;
pub mod ia_snapshot_seed;
pub mod ia_snapshot_same;
pub mod ia_snapshot_pool;
pub mod instance_backing;
pub mod instance_track;
pub mod instance_code;
pub mod geometry_pass_checks;
pub mod geometry_pass_coverage;
pub mod assembly_build;
pub mod assembly_pass;
pub mod geometry_pass_state;
pub mod geometry_pass_admission;
pub mod geometry_pass_sources;
pub mod geometry_pass_scope;
pub mod prepared_pair_checks;
pub mod prepared_pair_exit;
pub mod prepared_pair_tables;
pub mod prepared_pair_inputs;
pub mod prepared_pair_exit_stats;
pub mod prepared_pair_ranges;
pub mod prepared_pair_admission;
pub mod prepared_pair_poll;
pub mod prepared_pair_queries;
pub mod prepared_pair_objects;
pub mod prepared_pair_images;
pub mod retained_publish;
// The retained capture's journal and PublishSources (they write the Rust build's exports).
#[cfg(any(feature = "rust-build", feature = "probe-exports", test))]
pub mod retained_journal;
pub mod retained_capture;
pub mod retained_payloads;
pub mod retained_selectors;
pub mod retained_staged;
pub mod retained_config;
pub mod definition_code;
pub mod definition_cache;
pub mod definition_disjoint;
pub mod definition_reads;
pub mod stage_replay_reads;
pub mod stage_replay_layout;
pub mod stage_replay_ring;
pub mod early_depth_alpha;
pub mod early_depth_compare;
pub mod depth_comparison;
pub mod early_depth_pass;
// The pass's object (formerly EarlyDepthPass.cpp's Storage).
pub mod early_depth_object;
pub mod gpu_probe_checks;
pub mod gpu_probe_compare;
pub mod prepared_pair_exports;
pub mod prepared_pair_record;
pub mod readback_buffers;
pub mod early_depth_run;
pub mod audit_heaps;
pub mod residency_lease;
pub mod structured_view_overlay;
pub mod structured_view_owner;
pub mod geometry_snapshot;
// The geometry freeze (formerly src/Render/NativeGeometrySnapshots.cpp; its exports and draw copies lent by the .cpp).
pub mod geometry_freeze;
pub mod gpu_probe_commands;
pub mod gpu_probe_gate;
pub mod gpu_probe_runtime;
pub mod gpu_probe_scene;
pub mod retained_assembly;
pub mod retained_ia_views;
pub mod retained_permits;
pub mod retained_source_rows;
pub mod retained_readbacks;
pub mod geometry_pass_commands;
pub mod stage_replay_copies;
pub mod retained_overlay;
pub mod retained_instances;
pub mod definition_header;
pub mod definition_replay;
pub mod early_depth_inputs;
pub mod gpu_probe_timings;
pub mod retained_targets;
pub mod target_access_query;
pub mod ia_snapshot_code;
pub mod stereo_probe_pipelines;
// The native probe's pipeline observation (formerly src/Render/NativeStereoProbe.cpp's Enabled/RootCreated/...Created).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod stereo_probe_creation;
pub mod stereo_family_contracts;
// The stereo families' development package: the manifest, the shaders, SHA-256 (src/Render/NativeStereoFamilies.cpp).
pub mod stereo_family_manifest;
pub mod camera_upload_checks;
pub mod camera_upload;
pub mod source_cbv_reads;
pub mod paired_frame_code;
pub mod collector_code;
pub mod code_tables;
pub mod binding_code;
pub mod stereo_code;
pub mod target_access_events;
pub mod gpu_probe_settings;
pub mod early_depth_span;
pub mod frontier_reads;
pub mod families_status;
// The stereo families' registry (formerly src/Render/NativeStereoFamilies.cpp's; the .cpp keeps jsoncpp and the mirrors).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod families_native;
// The borrowed source and fixed ranges for the retained capture (formerly src/Render/StereoDescriptorAudit.cpp's glue).
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod borrowed_sources;
pub mod paired_geometry_checks;
#[cfg(all(any(feature = "rust-build", feature = "probe-exports"), not(test)))]
pub mod paired_geometry_native;
pub mod private_submits;
pub mod geometry_leases;
pub mod image_size;
pub mod stage_replay_camera;
#[cfg(any(feature = "rust-build", test))]
pub mod stage_replay_runtime;
pub mod handoff_target_shape;
pub mod retained_indirect;
pub mod collector_compare;
pub mod collector_input;
pub mod collector_peer;
pub mod constant_capture_checks;
pub mod constant_ledger;
// The native constant capture's state (formerly src/Render/NativeConstantCapture.cpp's; the registered copies' entries).
#[cfg(not(test))]
pub mod constant_capture_state;
// The constant bank pass (formerly src/Render/NativeConstantCapture.cpp's Poll bank request and pair copies, with
// StereoGeometryPass.cpp's and StereoDepthComparison.cpp's plugin halves).
#[cfg(all(feature = "rust-build", not(test)))]
pub mod constant_bank;
pub mod definition_tree_abi;
pub mod definition_owner_abi;
pub mod graph_snapshot_abi;
pub mod graph_parent_abi;
pub mod graph_jobs_abi;
pub mod job_lease_abi;
pub mod resource_uses_abi;
pub mod upload_arena_abi;
pub mod upload_shadow;
pub mod geometry_packets;
pub mod scene_update_receipt;
pub mod scene_update;
pub mod bounds_trace;
pub mod submission_frontier_owner;
// The Rust solve's host services (formerly src/Anim/CharacterRigRust.cpp's).
#[cfg(feature = "rust-build")]
pub mod rig_services;
// The PSVR2 haptic channel (formerly include/Runtimes/Psvr2HapticChannel.hpp's).
#[cfg(feature = "rust-build")]
pub mod psvr2_haptics;
// The core's small C++ services (formerly SettingsRes.cpp's, Framegen/Inputs.cpp's, EngineStereoMenu.cpp's, Main.cpp's).
#[cfg(feature = "rust-build")]
pub mod core_services;
// OpenXRManager's camera mount and HUD capture services (formerly OpenXRCalibration.cpp's and OpenXRPresent.cpp's).
#[cfg(feature = "rust-build")]
pub mod xr_manager_services;
// OpenXRManager's Init and Shutdown and the plugin's unload (formerly Runtimes/OpenXRManager.cpp's and Main.cpp's).
#[cfg(feature = "rust-build")]
pub mod xr_manager_shutdown;
// The registered constants' queue and fence API observer (formerly src/Render/ConstantQueueObserver.cpp).
#[cfg(feature = "rust-build")]
pub mod queue_api_observer;
// The submission frontier's former .cpp half (src/Render/NativeSubmissionFrontier.cpp's Rust branch).
#[cfg(feature = "rust-build")]
pub mod submission_frontier_host;
pub mod preparation_trace;
// The render threads' state (formerly C++ thread-locals).
pub mod render_tls;
pub mod peer_camera;
#[cfg(feature = "stereo")]
pub mod upscaler_views;
#[cfg(feature = "stereo")]
pub mod view_chain_timeline;
#[cfg(feature = "stereo")]
pub mod view_constants;
pub mod view_twin_compiler;
pub mod view_twin_ir;
#[cfg(feature = "stereo")]
pub mod vision_layer;
pub mod vrcam_config;
#[cfg(feature = "stereo")]
pub mod vrcam_selection;
pub mod world_marker_math;
pub mod world_marker_shader;
#[cfg(feature = "rust-build")]
pub mod optics_surface;
#[cfg(feature = "rust-build")]
pub mod world_markers;
pub mod vrs;
