//! Engine detours whose hooks and originals are Rust's, registered from Rust (formerly the CVR_DETOUR lines of
//! src/Stereo/SubmitChain.cpp, GameThreadQueues.cpp, UpscalerViews.cpp, EngineStereoSdk.cpp, RenderGraphShape.cpp,
//! ViewChainTimeline.cpp, AsymProjection.cpp, Dlss.cpp, FrameGraph.cpp, NodeDispatch.cpp, CullVisibility.cpp, ViewReuse.cpp, EngineStereo.cpp, NativeSceneUpdate.cpp, NativeBoundsTrace.cpp, src/Hooks/SeamlessLoading.cpp
//! and MenuMode.cpp, src/Lobby/StartScreen.cpp, PlayerBody.cpp and MenuLobby.cpp): one call at the C++ half's static
//! initialisation (src/Stereo/DetourRegistry.cpp), before the install pass. The game thread's counter wait
//! (0x5D3CFC) is registered by the queues before the timeline, the order of the .cpp files' static initialisation
//! (the newest registration of an RVA installs first).

use core::ffi::c_void;
use core::ptr::{addr_of_mut, null_mut};
use cvr_engine::detours::{register, WantedFn};

// The hooks and originals of cvr-rig (the VRIK build's), by symbol; the rest are this crate's.
extern "C" {
    fn cvr_seamless_can_finish();
    static mut cvr_seamless_can_finish_original: *mut c_void;
    fn cvr_menu_source_get_entry();
    fn cvr_menu_mode_get_entry();
    static mut cvr_menu_source_get_original: usize;
    static mut cvr_menu_mode_get_original: usize;
    fn cvr_menu_source_enabled() -> bool;
    fn cvr_menu_mode_getter_enabled() -> bool;
    fn cvr_player_body_spawn();
    static cvr_player_body_spawn_original: core::sync::atomic::AtomicPtr<c_void>;
}
// The scene update's and the bounds trace's asm thunks and their C-named originals (src/Hooks/*.asm and the .cpp files).
extern "C" {
    fn CvrSceneUpdateQueueDetour();
    static mut CvrSceneUpdateQueueOriginal: *mut c_void;
    fn CvrSceneUpdateProducerDetour();
    static mut CvrSceneUpdateProducerOriginal: *mut c_void;
    fn CvrSceneUpdateGeneralDetour();
    static mut CvrSceneUpdateGeneralOriginal: *mut c_void;
    fn CvrSceneUpdatePreDetour();
    static mut CvrSceneUpdatePreOriginal: *mut c_void;
    fn CvrSceneUpdateMembershipDetour();
    static mut CvrSceneUpdateMembershipOriginal: *mut c_void;
    fn CvrSceneUpdatePostDetour();
    static mut CvrSceneUpdatePostOriginal: *mut c_void;
    fn CvrSceneUpdateGeneralRunnerDetour();
    static mut CvrSceneUpdateGeneralRunnerOriginal: *mut c_void;
    fn CvrSceneUpdateLaneRunnerDetour();
    static mut CvrSceneUpdateLaneRunnerOriginal: *mut c_void;
    fn CvrBoundsDetour();
    static mut CvrBoundsOriginal: *mut c_void;
    fn CvrParticleDetour();
    static mut CvrParticleOriginal: *mut c_void;
    fn cvr_su_wanted() -> bool;
    fn cvr_bt_compatible(which: u32) -> bool;
}
// The scoped native draw origin (formerly src/Stereo/NativeDrawOrigin.cpp's data, predicate and callbacks): the thunk's
// original and the caller it expects, both read by the thunk (asm_thunks/native_draw_origin_thunk.rs) by symbol, and the
// callbacks it calls around the native draw.
#[no_mangle]
pub static mut CvrNativeDrawOriginal: *mut c_void = null_mut();
/// The draw's origin entered (its registers read while early depth draws; an empty origin otherwise).
///
/// # Safety
/// `frame` is the thunk's saved entry frame.
#[no_mangle]
pub unsafe extern "C" fn CvrNativeDrawBefore(frame: *const crate::draw_origin::DrawEntryFrame) {
    let origin = if crate::early_depth_native::cvr_ned_drawing() && !frame.is_null() {
        // SAFETY: the process image's handle.
        let base = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64);
        // SAFETY: the thunk's frame.
        crate::draw_origin::origin_from_entry(unsafe { &*frame }, base)
    } else {
        crate::draw_origin::DrawOrigin::default()
    };
    // SAFETY: a local origin.
    unsafe { crate::geometry_packets::cvr_gp_draw_origin(&origin) };
}
/// The draw's origin left.
#[no_mangle]
pub extern "C" fn CvrNativeDrawAfter() {
    // SAFETY: null leaves the innermost origin.
    unsafe { crate::geometry_packets::cvr_gp_draw_origin(core::ptr::null()) };
}
#[no_mangle]
pub static mut CvrNativeDrawExpectedCaller: u64 = 0;
extern "C" {
    fn CvrNativeDrawDetour();
}
/// OriginCompatible: the paired root asked for, the game code's draw origin contract held; the expected caller and
/// CyberpunkVR_GeometryOriginCompatible (1 valid, 2 not) written either way once asked.
extern "C" fn draw_origin_compatible() -> bool {
    if std::env::var_os("CVR_STEREO_PAIRED_ROOT").is_none() {
        return false;
    }
    // SAFETY: the executable's own module handle.
    let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) } as u64;
    let valid = crate::stereo_code::cvr_stereo_code(0, base);
    // SAFETY: written at the install pass, before the thunk is enabled.
    unsafe { CvrNativeDrawExpectedCaller = if valid { base + 0x33_5496 } else { 0 } };
    crate::declared_moved_exports::CyberpunkVR_GeometryOriginCompatible.store(if valid { 1 } else { 2 }, core::sync::atomic::Ordering::SeqCst);
    valid
}
// The render graph cache lookup's thunk original (asm_thunks/render_graph_cache_thunk.rs reads it by symbol; formerly
// src/Stereo/RenderParity.cpp's) and its site check.
#[no_mangle]
pub static mut CvrRenderGraphCacheOriginal: *mut c_void = null_mut();
extern "C" {
    fn CvrRenderGraphCacheDetour();
}
/// CacheSiteMatches: the lookup's prologue at its RVA.
extern "C" fn graph_cache_site_matches() -> bool {
    const BYTES: [u8; 24] = [
        0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56,
        0x41, 0x57,
    ];
    // SAFETY: the executable's own module handle and its code at the lookup's RVA (the image's text is mapped).
    unsafe {
        let base = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as usize;
        base != 0 && core::slice::from_raw_parts((base + 0x8E_7AE0) as *const u8, BYTES.len()) == BYTES
    }
}
extern "C" fn collector_wanted() -> bool {
    crate::collector_probe::cvr_ccp_wanted(0)
}
extern "C" fn query_wanted() -> bool {
    crate::collector_probe::cvr_ccp_wanted(1)
}
extern "C" fn scene_update_wanted() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_su_wanted() }
}
extern "C" fn bounds_compatible() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_bt_compatible(0) }
}
extern "C" fn particle_compatible() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_bt_compatible(1) }
}
use crate::game_thread_queues as gt;
use crate::submit_chain as submit;
use crate::view_chain_timeline as timeline;

extern "C" fn menu_source_wanted() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_menu_source_enabled() }
}
extern "C" fn menu_mode_wanted() -> bool {
    // SAFETY: a plain query.
    unsafe { cvr_menu_mode_getter_enabled() }
}
/// engine_stereo::Enabled.
extern "C" fn engine_stereo_wanted() -> bool {
    crate::engine_stereo::cvr_engine_stereo_enabled()
}
/// GetEnvironmentVariableW(name, nullptr, 0) != 0: the variable is set (even empty).
fn environment(name: &str) -> bool {
    std::env::var_os(name).is_some()
}
/// engine_stereo::SecondViewWanted: the port's own second view may render (engine stereo off, not a mono launch).
extern "C" fn second_view_wanted() -> bool {
    !crate::engine_stereo::cvr_engine_stereo_enabled() && !crate::engine_stereo::cvr_engine_stereo_mono_launch()
}
extern "C" fn graph_shape_wanted() -> bool {
    environment("CVR_GRAPH_SHAPE")
}
extern "C" fn job_timeline_wanted() -> bool {
    environment("CVR_JOB_TIMELINE")
}
extern "C" fn chain_timeline_wanted() -> bool {
    job_timeline_wanted() || environment("CVR_CHAIN_TIMELINE")
}

/// One detour: its hook's address and its original's slot.
fn detour(name: &'static str, rva: usize, hook: *const (), original: *mut c_void, wanted: Option<WantedFn>) {
    register(name, rva, hook.cast_mut().cast(), original.cast(), wanted, null_mut());
}

/// Registers the detours (at the C++ half's static initialisation).
/// The plugin's registrations at the DLL's static initialisation (STATIC_REGISTRATIONS below), in the order the C++
/// translation units made them when each had its own: the hook table (Hooks/Registry.cpp), the vtable
/// registry's A/B switch (Hooks/SwapChain.cpp), the instance observer's detours (Render/NativeInstanceObserver.cpp), the
/// engine's (Stereo/DetourRegistry.cpp), the definition tracking's (Stereo/NativeDefinitionTracking.cpp) and the paired
/// frame's (Stereo/NativePairedFrame.cpp).
#[no_mangle]
pub extern "C" fn cvr_static_registrations() {
    unsafe extern "C" {
        fn cvr_hooks_register_rust();
        fn cvr_vtable_attach(fast_original_lookup: *mut core::sync::atomic::AtomicU32);
        fn cvr_nio2_attach(host: *const c_void);
    }
    unsafe extern "C-unwind" {
        fn cvr_ndt_attach();
        fn cvr_npf_attach();
    }
    // SAFETY: the registrations, each made once at static initialisation as before; the exported switch lives for the
    // process (the registry only reads it).
    unsafe {
        cvr_hooks_register_rust();
        cvr_vtable_attach((&raw const crate::declared_moved_exports::CyberpunkVR_FastOriginalLookup).cast_mut());
        cvr_nio2_attach(core::ptr::null());
        cvr_engine_detours_attach();
        cvr_ndt_attach();
        cvr_npf_attach();
    }
}
/// The registrations' moment: an entry in the CRT's table of dynamic initializers (`.CRT$XCU`, where MSVC puts C++'s),
/// called while the DLL loads -- formerly src/Hooks/Registry.cpp's one C++ initializer. The C++ ones run first (the
/// Rust library links after the plugin's objects); none of them registers or reads a hook.
#[cfg(not(test))]
#[used]
#[link_section = ".CRT$XCU"]
static STATIC_REGISTRATIONS: extern "C" fn() = cvr_static_registrations;
#[no_mangle]
pub extern "C" fn cvr_engine_detours_attach() {
    // The single-pass probes (formerly Grading.cpp's CVR_DETOUR_IF lines).
    crate::single_pass_probes::register(detour);
    // The slots are the Rust hooks' own originals, written by the install pass.
    detour("[submit-chain] submission point sub_1422EF830", 0x22E_F830, submit::cvr_submit_point as *const (), addr_of_mut!(submit::cvr_submit_point_original).cast(), None);
    detour("[submit-chain] Present node sub_140BF60F0", 0xBF_60F0, submit::cvr_present_node as *const (), addr_of_mut!(submit::cvr_present_node_original).cast(), None);
    detour("[submit-chain] view job builder sub_140B603D8", 0xB6_03D8, submit::cvr_view_builder as *const (), addr_of_mut!(submit::cvr_view_builder_original).cast(), None);
    detour("[gt-queues] game thread dequeue sub_14026DE10", 0x26_DE10, gt::cvr_stereo_gt_dequeue as *const (), addr_of_mut!(gt::cvr_stereo_gt_dequeue_original).cast(), None);
    detour("[gt-queues] game thread counter wait sub_1405D3CFC", 0x5D_3CFC, gt::cvr_stereo_world_tick_wait as *const (), addr_of_mut!(gt::cvr_stereo_world_tick_wait_original).cast(), None);
    detour("[loading] initial loading can finish sub_140190004", 0x19_0004, cvr_seamless_can_finish as *const (), addr_of_mut!(cvr_seamless_can_finish_original).cast(), None);
    detour("[startup] native menu source observation", 0x4C_259C, cvr_menu_source_get_entry as *const (), addr_of_mut!(cvr_menu_source_get_original).cast(), Some(menu_source_wanted));
    detour("[startup] native menu source mode", 0x4C_278C, cvr_menu_mode_get_entry as *const (), addr_of_mut!(cvr_menu_mode_get_original).cast(), Some(menu_mode_wanted));
    detour("[upscaler] FidelityFX API providers sub_142287A20", 0x228_7A20, crate::upscaler_views::cvr_ffx_api_providers as *const (), addr_of_mut!(crate::upscaler_views::cvr_ffx_api_providers_original).cast(), None);
    detour(
        "[engine-stereo] Odyssey 3D SDK in process sub_142282E60",
        0x228_2E60,
        crate::engine_stereo_sdk::cvr_engine_stereo_wrapper_ctor as *const (),
        addr_of_mut!(crate::engine_stereo_sdk::cvr_engine_stereo_wrapper_ctor_original).cast(),
        Some(engine_stereo_wanted),
    );
    detour("[graph-shape] render graph run sub_1403E8F58", 0x3E_8F58, crate::render_graph_shape::cvr_stereo_graph_run as *const (), addr_of_mut!(crate::render_graph_shape::cvr_stereo_graph_run_original).cast(), Some(graph_shape_wanted));
    detour("[timeline] game-thread job run sub_14026E094", 0x26_E094, timeline::cvr_timeline_run_job as *const (), addr_of_mut!(timeline::cvr_timeline_run_job_original).cast(), Some(job_timeline_wanted));
    let chain: Option<WantedFn> = Some(chain_timeline_wanted);
    detour("[timeline] scene wait sub_1405D3CF0", 0x5D_3CF0, timeline::cvr_timeline_scene_wait as *const (), addr_of_mut!(timeline::cvr_timeline_scene_wait_original).cast(), chain);
    detour("[timeline] frame submit sub_1403D6D84", 0x3D_6D84, timeline::cvr_timeline_frame_submit as *const (), addr_of_mut!(timeline::cvr_timeline_frame_submit_original).cast(), chain);
    detour("[timeline] view flush sub_1403D7030", 0x3D_7030, timeline::cvr_timeline_view_flush as *const (), addr_of_mut!(timeline::cvr_timeline_view_flush_original).cast(), chain);
    detour("[timeline] view job sub_140B60324", 0xB6_0324, timeline::cvr_timeline_view_job as *const (), addr_of_mut!(timeline::cvr_timeline_view_job_original).cast(), chain);
    detour("[timeline] game frame sub_1405D48F0", 0x5D_48F0, timeline::cvr_timeline_game_frame as *const (), addr_of_mut!(timeline::cvr_timeline_game_frame_original).cast(), chain);
    detour("[timeline] RenderFrameTick job sub_1404F2170", 0x4F_2170, timeline::cvr_timeline_frame_tick as *const (), addr_of_mut!(timeline::cvr_timeline_frame_tick_original).cast(), chain);
    detour("[timeline] sub-scene job sub_140B243B8", 0xB2_43B8, timeline::cvr_timeline_sub_scene as *const (), addr_of_mut!(timeline::cvr_timeline_sub_scene_original).cast(), chain);
    detour("[timeline] sub-scene list sub_140B2428C", 0xB2_428C, timeline::cvr_timeline_sub_scene_list as *const (), addr_of_mut!(timeline::cvr_timeline_sub_scene_list_original).cast(), chain);
    detour("[timeline] world tick sub_14069C788", 0x69_C788, timeline::cvr_timeline_world_tick as *const (), addr_of_mut!(timeline::cvr_timeline_world_tick_original).cast(), chain);
    detour("[timeline] counter wait sub_1405D3CFC", 0x5D_3CFC, timeline::cvr_timeline_counter_wait as *const (), addr_of_mut!(timeline::cvr_timeline_counter_wait_original).cast(), chain);
    let start = &crate::start_screen::cvr_start_screen_pre_game_enter_original;
    detour("[boot] pre-game session enter sub_14018D6F4", 0x18_D6F4, crate::start_screen::cvr_start_screen_pre_game_enter as *const (), start.as_ptr().cast(), None);
    // The player body's own slot (an AtomicPtr, a pointer's layout).
    let spawn = core::ptr::addr_of!(cvr_player_body_spawn_original);
    detour("[body] player spawn sub_1409CE8FC", 0x9C_E8FC, cvr_player_body_spawn as *const (), spawn.cast_mut().cast(), None);
    let asym = &crate::asym_projection::cvr_asym_originals;
    detour("[asym] view prepare sub_1404240DC", 0x42_40DC, crate::asym_projection::cvr_asym_view_prepare as *const (), asym[0].as_ptr().cast(), None);
    detour("[asym] plane override sub_140422D2C", 0x42_2D2C, crate::asym_projection::cvr_asym_plane_override as *const (), asym[1].as_ptr().cast(), None);
    detour("[asym] cascade fit sub_1402A23AC", 0x2A_23AC, crate::asym_projection::cvr_asym_cascade_fit as *const (), asym[2].as_ptr().cast(), None);
    let lobby = &crate::menu_lobby::cvr_menu_lobby_originals;
    detour("[lobby] world layer tick sub_140607414", 0x60_7414, crate::menu_lobby::cvr_menu_lobby_world_layer_tick as *const (), lobby[0].as_ptr().cast(), None);
    detour("[lobby] main menu music sub_14155D3F8", 0x155_D3F8, crate::menu_lobby::cvr_menu_lobby_menu_music as *const (), lobby[1].as_ptr().cast(), None);
    use crate::frame_graph as fg;
    use crate::stereo_dlss as dlss;
    let graph = &fg::cvr_fg_originals;
    detour("[flagforce] flag-compute sub_14227E5D0", 0x227_E5D0, fg::cvr_fg_flag_compute as *const (), graph[4].as_ptr().cast(), None);
    let upscaler = &dlss::cvr_stereo_dlss_originals;
    detour("[dlss] ApplyDLSS work sub_14077E4C8", 0x77_E4C8, dlss::cvr_stereo_apply_dlss as *const (), upscaler[3].as_ptr().cast(), None);
    detour("[dlss] constants driver sub_1407EEA54", 0x7E_EA54, dlss::cvr_stereo_dlss_const as *const (), upscaler[1].as_ptr().cast(), None);
    detour("[dlss] tag/eval driver sub_14228F210", 0x228_F210, dlss::cvr_stereo_dlss_eval as *const (), upscaler[2].as_ptr().cast(), None);
    detour("[dlss] render-res scaler sub_140425054", 0x42_5054, dlss::cvr_stereo_render_res as *const (), upscaler[4].as_ptr().cast(), None);
    detour("[sl] SetStreamlineConstants sub_1407EE0BC", 0x7E_E0BC, dlss::cvr_stereo_sl_constants as *const (), upscaler[0].as_ptr().cast(), None);
    {
        // Debug Draw's shapes per eye under engine stereo (main's 0.2.4)
        use crate::debug_draw_stereo as dds;
        let o = &dds::cvr_dds_originals;
        detour("[debugdraw-stereo] debug fragments work sub_142EA16B4", dds::DEBUG_FRAGMENTS_WORK_RVA, dds::work as *const (), o[0].as_ptr().cast(), None);
        detour("[debugdraw-stereo] set viewport sub_140333A44", dds::SET_VIEWPORT_RVA, dds::set_viewport as *const (), o[1].as_ptr().cast(), None);
        detour("[debugdraw-stereo] bind camera sub_140297904", dds::BIND_CAMERA_RVA, dds::bind_camera as *const (), o[2].as_ptr().cast(), None);
        detour("[debugdraw-stereo] draw fragment list sub_14229D450", dds::DRAW_FRAGMENT_LIST_RVA, dds::draw_list as *const (), o[3].as_ptr().cast(), None);
    }
    detour("[scope] RR options sizing", 0x229_02D0, crate::scope_dlss::cvr_scope_rr_options as *const (),
           crate::scope_dlss::cvr_scope_rr_options_original.as_ptr().cast(), Some(crate::scope_dlss::rr_options_site_matches));
    detour("[water] state-assign sub_14067B380", 0x67_B380, fg::cvr_fg_water_assign as *const (), graph[3].as_ptr().cast(), None);
    detour("[build] full-build sub_141D43040", 0x227_6E60, fg::cvr_fg_full_build as *const (), graph[0].as_ptr().cast(), None);
    detour("[build] incr-build sub_14227BD10", 0x227_BD10, fg::cvr_fg_incr_build as *const (), graph[1].as_ptr().cast(), None);
    detour("[lighting] rect-compute sub_14206EF20", 0x206_EF20, fg::cvr_fg_rect_compute as *const (), graph[2].as_ptr().cast(), None);
    detour("[rtt] view-create sub_14048FAF4", 0x48_FAF4, fg::cvr_fg_rtt_view_create as *const (), fg::cvr_fg_rtt_view_create_original.as_ptr().cast(), None);
    let node = &crate::node_features::cvr_nd_originals;
    detour("[featbit] per-bit feature test sub_14033E358", 0x33_E358, crate::node_features::cvr_nd_feature_bit as *const (), node[0].as_ptr().cast(), None);
    detour("[optics] render-element pass driver sub_14033DDA4", 0x33_DDA4, crate::node_features::cvr_nd_pass_driver as *const (), node[1].as_ptr().cast(), None);
    detour("[stereo] velocity stage identities", 0x10_808DC, crate::scene_velocity::cvr_scene_velocity_stage_ctor as *const (),
           crate::scene_velocity::cvr_scene_velocity_stage_original.as_ptr().cast(), None);
    use crate::cull_visibility as cull;
    let culled = &cull::cvr_cull_originals;
    let second: Option<WantedFn> = Some(second_view_wanted);
    let staged = &cull::cvr_cull_stage_originals;
    detour("[cull] DoCulling sub_140BF436C", 0xBF_436C, cull::cvr_cull_doculling as *const (), staged[0].as_ptr().cast(), second);
    detour("[cull] GraphContextPrepare sub_140423180", 0x42_3180, cull::cvr_cull_graph_context_prepare as *const (), staged[1].as_ptr().cast(), None);
    detour("[cull] FineMaterialize sub_14029D110", 0x29_D110, cull::cvr_cull_fine_materialize as *const (), staged[3].as_ptr().cast(), second);
    detour("[cull] MainCullTest sub_140C651C4", 0xC6_51C4, cull::cvr_cull_main_cull_test as *const (), staged[2].as_ptr().cast(), second);
    detour("[cull] GatherCtxInit sub_140623FD8 (LOD-thresh sweep)", 0x228_6CC0, cull::cvr_cull_gather_ctx_init as *const (), culled[0].as_ptr().cast(), None);
    detour("[cull] VisQueryPrepare sub_1405AC174 (occlusion gate)", 0x5A_C174, cull::cvr_cull_visquery_prepare as *const (), culled[1].as_ptr().cast(), None);
    detour("[cull] QueryWork sub_14029C158", 0x29_C158, cull::cvr_cull_querywork as *const (), culled[2].as_ptr().cast(), second);
    detour("[cull] GraphContextReset sub_1404ECEA0", 0x4E_CEA0, cull::cvr_cull_graph_context_reset as *const (), culled[3].as_ptr().cast(), second);
    detour("[cull] MainCullPrepare sub_140C65218", 0xC6_5218, cull::cvr_cull_main_cull_prepare as *const (), culled[12].as_ptr().cast(), second);
    detour("[prep] filter sub_14229D240", 0x229_D240, cull::cvr_cull_prepare_filter as *const (), culled[6].as_ptr().cast(), second);
    detour("[prep] finalize sub_140426AE4", 0x42_6AE4, cull::cvr_cull_prepare_finalize as *const (), culled[7].as_ptr().cast(), second);
    detour("[prep] gather sub_1404EE6BC", 0x4E_E6BC, cull::cvr_cull_prepare_gather as *const (), culled[5].as_ptr().cast(), second);
    detour("[prep] sortA sub_140427AC8", 0x42_7AC8, cull::cvr_cull_prepare_sort_a as *const (), culled[8].as_ptr().cast(), second);
    detour("[prep] sortB sub_140427F00", 0x42_7F00, cull::cvr_cull_prepare_sort_b as *const (), culled[9].as_ptr().cast(), second);
    detour("[prep] sortC sub_140428330", 0x42_8330, cull::cvr_cull_prepare_sort_c as *const (), culled[10].as_ptr().cast(), second);
    detour("[prep] finalSort sub_140695F80", 0x69_5F80, cull::cvr_cull_prepare_sort_final as *const (), culled[11].as_ptr().cast(), second);
    detour("[prep] stage sub_14229D3E0", 0x229_D3E0, cull::cvr_cull_prepare_stage as *const (), culled[4].as_ptr().cast(), second);
    detour("[cull] VisibleAppend sub_14019B75C", 0x19_B75C, cull::cvr_cull_visible_append as *const (), culled[15].as_ptr().cast(), second);
    detour("[cull] VisibilityCollector sub_1404ED29C", 0x4E_D29C, cull::cvr_cull_visibility_collector as *const (), culled[13].as_ptr().cast(), second);
    detour("[cull] MaterializeWorker sub_1404ED62C", 0x4E_D62C, cull::cvr_cull_materialize_worker as *const (), culled[14].as_ptr().cast(), second);
    use crate::view_reuse as reuse;
    let reused = &reuse::cvr_reuse_originals;
    detour("[complend] composition state assign sub_140545714", 0x54_5714, reuse::cvr_reuse_comp_assign as *const (), reused[0].as_ptr().cast(), second);
    detour("[complend] composition element draw keys sub_1402ABA18", 0x2A_BA18, reuse::cvr_reuse_comp_elem_keys as *const (), reused[1].as_ptr().cast(), second);
    detour("[complend] composition element rect sub_1402AC150", 0x2A_C150, reuse::cvr_reuse_comp_elem_rect as *const (), reused[2].as_ptr().cast(), second);
    detour("[light] RenderLightBuffers sub_140411B94 (block-list lend)", 0x41_1B94, reuse::cvr_reuse_light_buffers as *const (), reused[3].as_ptr().cast(), second);
    detour("[clouds] cloud-CB sub_1407F825C", 0x7F_825C, reuse::cvr_reuse_cloud_cb_fill as *const (), reused[4].as_ptr().cast(), None);
    detour("[viewrect] sky-scattering node sub_1407F86CC", 0x7F_86CC, reuse::cvr_reuse_sky_scattering as *const (), reused[5].as_ptr().cast(), None);
    detour("[distant] distant-prepare sub_1405AEEC8", 0x5A_EEC8, reuse::cvr_reuse_distant_prepare as *const (), reused[6].as_ptr().cast(), None);
    detour("[distant] distant-render sub_1405A93E8", 0x5A_93E8, reuse::cvr_reuse_distant_render as *const (), reused[7].as_ptr().cast(), None);
    detour("[localshadow] local-shadow sub_140B7CF7C", 0xB7_CF7C, reuse::cvr_reuse_local_shadow as *const (), reused[8].as_ptr().cast(), None);
    detour("[probes] reflection-probe atlas sub_140DAC960", 0xDA_C960, reuse::cvr_reuse_reflection_probes as *const (), reused[9].as_ptr().cast(), None);
    detour("[foliage] speedtree wind sub_140D60084", 0xD6_0084, reuse::cvr_reuse_speedtree_wind as *const (), reused[10].as_ptr().cast(), None);
    detour("[foliage] wind-impulse node sub_1407F2BF0", 0x7F_2BF0, reuse::cvr_reuse_wind_impulse as *const (), reused[11].as_ptr().cast(), None);
    detour("[viewparams] per-view parameter apply sub_140492838", 0x49_2838, reuse::cvr_reuse_view_params_apply as *const (), reused[12].as_ptr().cast(), None);
    detour("[viewparams] viewData copy sub_140499DF0", 0x49_9DF0, reuse::cvr_reuse_view_data_copy as *const (), reused[13].as_ptr().cast(), None);
    detour("[cascade] clear sub_1422A3F80", 0x22A_3F80, reuse::cvr_reuse_cascade_clear as *const (), reused[14].as_ptr().cast(), None);
    detour("[cascade] shadow-cascade node sub_1404EDFE0", 0x4E_DFE0, reuse::cvr_reuse_cascade_node as *const (), reused[15].as_ptr().cast(), None);
    detour("[gi] GI-node sub_1407F4130", 0x7F_4130, reuse::cvr_reuse_gi_node as *const (), reuse::cvr_gi_node_original.as_ptr().cast(), None);
    use crate::grading_lut as lut;
    use crate::grading_uploads as uploads;
    let uploaders = &uploads::cvr_grading_upload_originals;
    detour("[sway] buffer uploader sub_140334328", 0x33_4328, uploads::cvr_grading_buf_upload as *const (), uploaders[0].as_ptr().cast(), None);
    detour("[gradeup] constant uploader sub_14041A780", 0x41_A780, uploads::cvr_grading_cb_upload as *const (), uploaders[1].as_ptr().cast(), None);
    detour("[wide-merge] pixel-stage b6 uploader sub_14041A56C", 0x41_A56C, uploads::cvr_grading_ps_b6_upload as *const (), uploads::PS_B6_ORIGINAL.as_ptr().cast(), None);
    detour(
        "[flicker] post-color de-alias sub_140334810",
        0x33_4810,
        crate::resolve_post_color::cvr_resolve3d20_hook as *const (),
        crate::resolve_post_color::cvr_resolve3d20_original.as_ptr().cast(),
        None,
    );
    let graded = &lut::cvr_grading_lut_originals;
    detour("[grade] grading composer sub_140C944A4", 0xC9_44A4, lut::cvr_grading_compose as *const (), graded[0].as_ptr().cast(), None);
    detour(
        "[grading] GenerateTonemappingLUT sub_140F60474 (vrcam borrows main grading source)",
        0xF6_0474,
        lut::cvr_grading_tonemap_lut as *const (),
        graded[1].as_ptr().cast(),
        None,
    );
    use crate::engine_stereo as stereo;
    let eyes = &stereo::cvr_engine_stereo_originals;
    let engine: Option<WantedFn> = Some(engine_stereo_wanted);
    detour("[engine-stereo] eye windows sub_140296EA0", 0x29_6EA0, stereo::cvr_engine_stereo_camera_build as *const (), eyes[3].as_ptr().cast(), engine);
    detour("[engine-stereo] one-eye UI inkLayer::Resize sub_14132654C", 0x132_654C, stereo::cvr_engine_stereo_layer_resize as *const (), eyes[4].as_ptr().cast(), engine);
    detour("[engine-stereo] one-eye UI layer target sub_140399094", 0x39_9094, stereo::cvr_engine_stereo_layer_target as *const (), eyes[5].as_ptr().cast(), engine);
    detour("[engine-stereo] one-eye UI layer tick sub_14060787C", 0x60_787C, stereo::cvr_engine_stereo_layer_tick as *const (), eyes[6].as_ptr().cast(), engine);
    detour("[engine-stereo] one-eye UI view projection sub_14054AD18", 0x54_AD18, stereo::cvr_engine_stereo_view_project as *const (), eyes[7].as_ptr().cast(), engine);
    detour("[engine-stereo] one-eye UI view projection sub_142144370", 0x214_4370, stereo::cvr_engine_stereo_view_project_other as *const (), eyes[8].as_ptr().cast(), engine);
    detour("[engine-stereo] one-eye UI view unprojection sub_1429F403C", 0x29F_403C, stereo::cvr_engine_stereo_view_unproject as *const (), eyes[9].as_ptr().cast(), engine);
    detour("[engine-stereo] convergence sub_142284950", 0x228_4950, stereo::cvr_engine_stereo_convergence as *const (), eyes[2].as_ptr().cast(), engine);
    detour("[engine-stereo] separation sub_142284990", 0x228_4990, stereo::cvr_engine_stereo_separation as *const (), eyes[0].as_ptr().cast(), engine);
    detour("[engine-stereo] weapon separation sub_1422849D0", 0x228_49D0, stereo::cvr_engine_stereo_weapon_separation as *const (), eyes[1].as_ptr().cast(), engine);
    let scene: Option<WantedFn> = Some(scene_update_wanted);
    detour("[single-pass] scene-update queue payload admission", 0x27_2B40, CvrSceneUpdateQueueDetour as *const (), addr_of_mut!(CvrSceneUpdateQueueOriginal).cast(), scene);
    detour("[single-pass] scene-update producer scope", 0x3D_74F8, CvrSceneUpdateProducerDetour as *const (), addr_of_mut!(CvrSceneUpdateProducerOriginal).cast(), scene);
    detour("[single-pass] scene-update ordinary command worker", 0xD3_3AB4, CvrSceneUpdateGeneralDetour as *const (), addr_of_mut!(CvrSceneUpdateGeneralOriginal).cast(), scene);
    detour("[single-pass] scene-update pre command worker", 0xB6_7920, CvrSceneUpdatePreDetour as *const (), addr_of_mut!(CvrSceneUpdatePreOriginal).cast(), scene);
    detour("[single-pass] scene-update membership worker", 0x3D_7A54, CvrSceneUpdateMembershipDetour as *const (), addr_of_mut!(CvrSceneUpdateMembershipOriginal).cast(), scene);
    detour("[single-pass] scene-update post command worker", 0xC2_96F4, CvrSceneUpdatePostDetour as *const (), addr_of_mut!(CvrSceneUpdatePostOriginal).cast(), scene);
    detour("[single-pass] scene-update ordinary virtual input inventory", 0x67_A92C, CvrSceneUpdateGeneralRunnerDetour as *const (), addr_of_mut!(CvrSceneUpdateGeneralRunnerOriginal).cast(), scene);
    detour("[single-pass] scene-update lane virtual input inventory", 0x4E_FF98, CvrSceneUpdateLaneRunnerDetour as *const (), addr_of_mut!(CvrSceneUpdateLaneRunnerOriginal).cast(), scene);
    detour("[single-pass] scene candidate bounds writes", 0x29_ABD8, CvrBoundsDetour as *const (), addr_of_mut!(CvrBoundsOriginal).cast(), Some(bounds_compatible));
    detour("[single-pass] particle instance preparation", 0x29_173C, CvrParticleDetour as *const (), addr_of_mut!(CvrParticleOriginal).cast(), Some(particle_compatible));
    detour("[single-pass] scoped native draw origin", 0x33_5900, CvrNativeDrawDetour as *const (), addr_of_mut!(CvrNativeDrawOriginal).cast(), Some(draw_origin_compatible));
    detour("[render-parity] GraphCacheLookup", 0x8E_7AE0, CvrRenderGraphCacheDetour as *const (), addr_of_mut!(CvrRenderGraphCacheOriginal).cast(), Some(graph_cache_site_matches));
    detour(
        "[render-parity] reproject MAIN fog history",
        0x3E_05F8,
        crate::render_parity::cvr_rp_fog_history_hook as *const (),
        crate::render_parity::cvr_rp_fog_srv_original.as_ptr().cast(),
        Some(second_view_wanted),
    );
    use crate::collector_probe as probe;
    let collector = &probe::cvr_ccp_originals;
    detour("[single-pass] owned CPU collector lifecycle", 0xC0_F46C, probe::cvr_ccp_collector_hook as *const (), collector[0].as_ptr().cast(), Some(collector_wanted));
    detour("[single-pass] owned CPU query tasks", 0x5A_BBD0, probe::cvr_ccp_query_hook as *const (), collector[1].as_ptr().cast(), Some(query_wanted));
}
