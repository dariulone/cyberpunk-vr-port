#pragma once

#include <cstdint>

// ================================================================================================
// Verified offsets into Cyberpunk2077.exe.
//
// One table, because that is how they are checked. Every one of these was found by disassembly and
// confirmed against a running build; when the game is patched they are the first thing to re-verify,
// and doing that against a list is possible where doing it against forty scattered constants is not.
//
// They are also why the detour registry logs in ASCENDING RVA: the log then reads in the same order
// as the image.
// ================================================================================================

namespace cvr {
namespace detail {

constexpr uintptr_t CALLER1_RVA = 0x5D48F0;  // sub_1405D48F0 RenderFull
constexpr uintptr_t LIGHT_RVA   = 0x3D5F88;  // sub_1403D5F88 RenderLight (M-B2)
constexpr uintptr_t FLUSH_RENDER_SCENE_RVA = 0x3D7030;
constexpr uintptr_t RELEASE_HANDLE_RVA     = 0x32D7FC;
constexpr uintptr_t APPEND_TYPEA_VIEW_RVA  = 0x4901C0;
constexpr uintptr_t VIEW_CONTEXT_ALLOC_RVA = 0x9273A0;  // sub_1409273A0: build view-tail owner X (own manager)
constexpr uintptr_t VIEW_ITEM_VTABLE_RVA   = 0x3059A60;  // 2.31 0x2AC8688; stored by sub_14049B278 / sub_140499DF0
constexpr uintptr_t CAMERA_WRITE_RVA       = 0x7EE0BC;
constexpr uintptr_t FRAME_GATE_RVA         = 0x5D4898;
constexpr uintptr_t VIEW_FINALIZE_RVA      = 0x4C3C40;
constexpr uintptr_t FG_BUILD_RVA           = 0xB60324;
constexpr uintptr_t TYPE_B_SUBMIT_RVA      = 0x3D6A34;
constexpr uintptr_t VIEW_ITEM_FACTORY_RVA  = 0x3D71A4;
constexpr uintptr_t JOB_CONTEXT_INIT_RVA   = 0x409838;
constexpr uintptr_t JOB_CONTEXT_LINK_RVA   = 0x271EDC;
constexpr uintptr_t JOB_CONTEXT_ADVANCE_RVA = 0x27316C;
constexpr uintptr_t JOB_CONTEXT_DESTROY_RVA = 0x272DB8;
constexpr uintptr_t RUN_RENDER_NODES_RVA   = 0x409DD4;
constexpr uintptr_t RUN_RENDER_MANAGER_LOAD_RVA = 0x409E48;  // mov r15,[rbx+20h] (2.31 0x2197A4: into r13)
constexpr uintptr_t RUN_NODE_BATCH_SUBMIT_RVA = 0xB52C98;
constexpr uintptr_t RUN_NODE_BATCH_WORK_RVA   = 0xB79044;
constexpr uintptr_t GRAPH_REQUEST_BUILD_RVA = 0x67B66C;
constexpr uintptr_t GRAPH_CONTEXT_PREPARE_RVA = 0x423180;
constexpr uintptr_t GRAPH_CACHE_LOOKUP_RVA = 0x8E7AE0;
constexpr uintptr_t PREPARE_COLLECTOR_WORK_RVA = 0xC0F204;
constexpr uintptr_t CAMERA_RESOURCE_SCOPE_WORK_RVA = 0xD2A678;
constexpr uintptr_t FRAME_BUILD_MARKER_RVA = 0x4BC984;
constexpr uintptr_t SYNCHRONIZE_NODE_WORK_RVA = 0x655FAC;
constexpr uintptr_t PREPARE_SCENE_NODE_WORK_RVA = 0x5A764C;
constexpr uintptr_t GRAPH_CONTEXT_RESET_RVA = 0x4ECEA0;
constexpr uintptr_t GRAPH_CONTEXT_OWNER_MOVE_RVA = 0x4ED4E8;
constexpr uintptr_t NODE_DISPATCH_RVA = 0x332F90;
constexpr uintptr_t COPY_TO_TEXTURE_WORK_RVA = 0x3FE078;
constexpr uintptr_t RENDER_FINAL2D_WORK_RVA = 0x4172C8;
constexpr uintptr_t DECLARE_FINAL_ONLY_WORK_RVA   = 0x3E0070; // DeclareCommonResourceAllocs_FinalOnly
// THE FIVE READERS OF viewData+0x168, found by walking the 68 callers of the viewData accessor
// sub_14033C248 and keeping the ones that touch +0x168. Measured, not taken from a note: the
// declaration pass DeclareCommonResourceAllocs_FinalOnly (0x3E0070) is NOT among them, which is
// why lending to it did nothing.
constexpr uintptr_t COMPOSITION_WORK_RVA          = 0x3E1B20; // CRenderNode_CompositionPostProcess
constexpr uintptr_t RT_DECLARE_WORK_RVA           = 0xCEA010; // names RT_viewDepthTexture[%d] and
                                                              // RT_localShadowTexture[%d] -- the
                                                              // render-target declarations
constexpr uintptr_t GATE_READER_770268_RVA        = 0x773580; // the fifth reader, unnamed
constexpr uintptr_t COMP_ASSIGN_RVA               = 0x545714; // the composition-state assign
constexpr uintptr_t COMP_ASSIGN_FRAME_RET_RVA     = 0x5D317F; // return address of that assign in the game
                                                              // frame sub_1405D48F0 (menus, loading)
constexpr uintptr_t COMP_ASSIGN_VIEW_RET_RVA      = 0x4C3255; // ...and in the view fill sub_14029CDD8 (in the world)
constexpr uintptr_t HANDLE_COPY_RVA               = 0xB7CB18; // copy of a refcounted {object, count} handle:
                                                              // takes a count on the source, releases the target
constexpr uintptr_t EXTRACTION_FINAL_COLOR_WORK_RVA = 0x7F1A60; // ExtractionFinalColor
constexpr uintptr_t CLEAR_FINAL_COLOR_WORK_RVA    = 0x7F1B2C; // ClearFinalColorTarget
constexpr uintptr_t GRAPH_REQUEST_REGISTER_RVA = 0x2E78770;
constexpr uintptr_t GRAPH_REQUEST_ALLOC_RVA = 0x2E78210;
constexpr uintptr_t GRAPH_REQUEST_POPULATE_RVA = 0x2E784F8;
constexpr uintptr_t GRAPH_MANAGER_ALLOC_RVA = 0x2E96BC8;
constexpr uintptr_t GRAPH_MANAGER_CTOR_RVA = 0x80D8A0;
constexpr uintptr_t TONEMAP_WORK_RVA = 0x557204;
constexpr uintptr_t RESOLVE_QUERY_RVA = 0x334810;
constexpr uintptr_t DECLARE_RVA = 0x334A34;   // sub_140334A34
constexpr uintptr_t TYPE_A_JOB_VT_RVA      = 0x30BC8A0;
constexpr uintptr_t TYPE_B_JOB_VT_RVA      = 0x304F250;
constexpr uintptr_t SPEC_EMPTY_INIT_RVA    = 0x5D3D30;
constexpr uintptr_t SPEC_COPY_INIT_RVA     = 0x33BB1C;
constexpr uintptr_t SPEC_EMPTY_DTOR_RVA    = 0x1EDCCC;
constexpr uintptr_t BUILD_MATE_INIT_RVA    = 0x5D3D50;
constexpr uintptr_t BUILD_MATE_FILL_RVA    = 0x1EDBB8;
constexpr uintptr_t VIEW_PRODUCER_RVA      = 0x3D6EB0;
constexpr uintptr_t BUILD_MATE_DTOR_RVA    = 0x21280C;
constexpr uintptr_t VIEW_SPEC_DTOR_RVA     = 0x1F68A8;
constexpr uintptr_t SPEC_FINAL_BIND_RVA    = 0x21239C;
constexpr uintptr_t CAMW_RVA = 0x7EE0BC;
constexpr uintptr_t CLOUDS_NODE_RVA = 0x7F23B4;   // CRenderNode_RenderVolumetricClouds
constexpr uintptr_t DESC_HEAP_SIZE_MOV_RVA = 0x8EB88A;   // 2.31: 0x91D64A (same mov eax, 0xDC240)
// THE ONLY PLACE THE ENGINE SORTS A DRAW ITEM BY RENDERING PLANE. sub_14033DDA4 reads the item's
// flags dword, tests bit 5, and picks the per-view parameter block at view+0x70 + index*0x3A0 --
// index 0 for the scene, index 2 for the first-person weapon plane. The multiply by 0x3A0 occurs
// exactly ONCE in the whole binary, so this is the whole routing mechanism, not one of several:
//
//     14023AC04  A8 20     test al, 20h          ; bit 5 = weapon plane
//     14023AC06  74 1E     jz short 14023AC26    ; not set -> index 0 (scene)
//     14023AC08            lea rdx, WeaponPlane  ; set     -> is the category on for this view
//     14023AC1F            mov eax, 2            ;            yes -> index 2 (weapon plane)
//
// Turning the `jz` into an unconditional `jmp` sends EVERY item down the scene branch, which is
// what "draw the first-person plane with the scene's parameters" means at the lowest level: no
// asset edits, no per-item attach argument, no equipment-state re-assertion to fight.
// 2.5: the same test at 0x33E0B6 (now behind a `test al, 5` pre-check), jz rel8 0x2B, no 0x3A0 multiply any
// more (the index is passed by reference: mov [rbp+20h], eax; lea rdx, [rbp+20h]).
constexpr uintptr_t PLANE_ROUTE_BRANCH_RVA = 0x33E0B8;   // 2.31: 0x23AC06

// THE PER-OBJECT PLANE FILTER, found 2026-09-02 by diffing one object's render proxy between the two
// planes: the proxy carries the raw ERenderingPlane at byte +0x9D (0 scene, 2 weapon), and this is
// the collection that drops everything whose plane is not the one being gathered.
//
//     0x94EC4C  mov rcx, [rsi+8]              ; the proxy
//     0x94EC50  cmp byte [rcx+0x9D], r12b     ; its plane against the one being gathered
//     0x94EC57  jnz ...                       ; different -> the object is skipped entirely
//
// It sits next to the highlight walk (0x415048 and its predicate 0x414FE0), which is what the
// hardware breakpoint on a stored highlight record led to. Turning the jnz into a six-byte nop lets
// objects of every plane through -- the experiment the live session could not finish, because this
// game dies at a debugger pause (three crashes, all at the same int3 stub, two of them with no patch
// applied at all).
constexpr uintptr_t PLANE_FILTER_JNZ_RVA = 0x94EC57;

// WHERE THE PLANE BECOMES A DRAW-LIST INDEX -- the whole mechanism, found 2026-09-02 by following the
// one byte that differs between an object's two proxies (+0x9D, the raw ERenderingPlane):
//
//     0x623139  mov  r8b, [proxy+0x9D]           ; the plane
//     0x623140  call sub_1401E7D88  ->  sub_1401E7DEC(a1, pass, a2, plane)
//                   v7 = plane + 10 * pass;      ; THE BUCKET NUMBER
//                   v8 = a1 + 448 * v7;          ; the bucket
//                   v9 = *(v8 + 8*a2 + 224);     ; the list the item is appended to
//
// So the scene goes into bucket 0 + 10*pass and the first-person plane into 2 + 10*pass: two separate
// lists. Nothing anywhere compares a plane and drops a highlighted object -- which is why three
// separate "gate" patches all did nothing. The item is simply put on a different pile, and the
// highlight gather walks the scene pile.
//
// Zeroing the plane right here sends EVERY item to the scene bucket, globally, in one instruction.
// 2.5 (sub_1404A0878): the plane is loaded ONCE, here (mov sil, [rsi+0x9D]), spilled to [rsp+34h], reloaded
// into sil and handed to both bucket calls (0x4A0EA7, 0x4A1306: mov r8b, sil); nothing else reads it. Zeroing
// the load therefore zeroes both calls (2.31: 0x623139, mov r8b, [rax+0x9D] right before its one call).
constexpr uintptr_t PLANE_BUCKET_LOAD_RVA = 0x4A0983;
constexpr uintptr_t RTT_VIEWCREATE_RVA = 0x48FAF4;   // sub_14048FAF4
constexpr uintptr_t VIEW_PARAMS_ENTRY_RVA = 0x4928E4;  // sub_1404928E4, applies ONE override
                                                       // entry to a view; the outer pass is
                                                       // what erases, this does not
constexpr uintptr_t VIEW_PARAMS_APPLY_RVA = 0x492838;  // sub_140492838, the per-view parameter
                                                       // producer -- writes MAIN's viewData+0x798
constexpr uintptr_t VIEWDATA_COPY_RVA  = 0x499DF0;   // sub_140499DF0, carries the struct in
constexpr uintptr_t RTT_HOST_VTABLE_RVA = 0x36116E8; // entRenderToTextureCameraComponent host vtable
constexpr uintptr_t RESIZE_DYNTEX_RVA = 0x2E8C310;   // sub_142E8C310
constexpr uintptr_t FULL_BUILD_RVA = 0x2276E60;   // sub_142276E60 (2.31: sub_141D43040, same strings, 0.57 token ratio)
constexpr uintptr_t INCR_BUILD_RVA = 0x227BD10;   // sub_14227BD10
constexpr uintptr_t EXTRACTION_ADDER_RVA   = 0x8EAFCC;   // sub_1408EAFCC (ExtractionFinalColor)
constexpr uintptr_t DECLARE_ADDER_RVA      = 0x8EB0EC;   // sub_1408EB0EC (DeclareCommonResourceAllocs_FinalOnly)
constexpr uintptr_t CLEAR_ADDER_RVA        = 0x8EB02C;   // sub_1408EB02C (ClearFinalColorTarget)
constexpr uintptr_t DRAWCOMP_ADDER_RVA     = 0x8EAAD4;   // sub_1408EAAD4 (DrawComposition)
constexpr uintptr_t COMPOSITION_ADDER_RVA  = 0x8EAA78;   // sub_1408EAA78 (CompositionPostProcess)
constexpr uintptr_t FSVIDEO_ADDER_RVA      = 0x8EA9B8;   // sub_1408EA9B8 (FullscreenVideo)
constexpr uintptr_t ADD_NAMED_PASS_RVA     = 0x8E926C;   // sub_1408E926C (add pass by name)
constexpr uintptr_t SCENE_FULL_LO_RVA = 0x2276E60, SCENE_FULL_HI_RVA = 0x2276E60 + 0x4EAE;
constexpr uintptr_t SCENE_INCR_LO_RVA = 0x227BD10, SCENE_INCR_HI_RVA = 0x227BD10 + 0xA33;
constexpr uintptr_t SCENE_RTT_LO_RVA  = 0x227D060, SCENE_RTT_HI_RVA  = 0x227D060 + 0x1310;
constexpr uintptr_t FLAG_COMPUTE_RVA = 0x227E5D0;   // sub_14227E5D0 (2.31: sub_141D49540; group ids +1 from 44, +3 from 76, +5 from 87)
// The water-state assignment, sub_14067B380(dst, src). Found by a hardware write breakpoint on the
// second view's +0x1800 while the port's own copy was in place: this is what overwrites it.
constexpr uintptr_t WATER_ASSIGN_RVA = 0x67B380;    // sub_14067B380
constexpr uintptr_t HANDLE_ASSIGN_RVA = 0x21309C;   // sub_1407CDAE4(dst, src)
constexpr uintptr_t RECT_COMPUTE_RVA = 0x206EF20;   // sub_14206EF20 (2.31: sub_1404E3EB4)
constexpr uintptr_t SKY_WORK_RVA = 0x7F8714;   // sub_1407F8714, the body behind feature 35
constexpr uintptr_t DISTANT_RENDER_RVA  = 0x5A93E8;   // sub_1405A93E8
constexpr uintptr_t DISTANT_PREPARE_RVA = 0x5AEEC8;   // sub_1405AEEC8
constexpr uintptr_t CLOUD_CB_FILL_RVA = 0x7F825C;   // sub_1407F825C
constexpr uintptr_t LOCAL_SHADOW_RVA = 0xB7CF7C;   // sub_140B7CF7C RenderLocalShadowMaps
constexpr uintptr_t REFLECTION_PROBES_RVA = 0xDAC960;   // sub_140DAC960 ReflectionProbes
constexpr uintptr_t SKY_SCATTERING_RVA = 0x7F86CC;   // sub_1407F86CC RenderSkyScattering node work
constexpr uintptr_t VIEW_RECT_TEST_RVA = 0x33AA6C;   // sub_14033AA6C(p) = p[0]>=p[2] || p[1]>=p[3]
constexpr uintptr_t CB_UPLOAD_RVA   = 0x41A780;
// The BUFFER uploader -- (descriptor index, size, src) rather than (size, src). The frame-global
// constant blocks travel this way and NOT through CB_UPLOAD above, which is why a 480-byte probe on
// the latter never fired once; the constant census named these rows "buf:".
constexpr uintptr_t BUF_UPLOAD_RVA  = 0x334328;   // sub_140334328(idx, size, src)
constexpr uintptr_t GRADING_COMPOSE_RVA = 0xC944A4;
constexpr uintptr_t TONEMAP_LUT_RVA = 0xF60474;   // sub_140F60474 GenerateTonemappingLUT
constexpr uintptr_t GI_NODE_RVA         = 0x7F4130;   // sub_1407F4130 GlobalIllumination node
constexpr uintptr_t GI_EARLYCHK_RVA     = 0x33AA6C;   // sub_14033AA6C(ctx+0x14) early-out
// The foliage wind-impulse pass -- what bends a bush you walk into. Written for ONE view per frame,
// and which view claims it decides whether the vegetation is a frame stale; see the long note at
// Detour_WindImpulseNode.
constexpr uintptr_t WIND_IMPULSE_NODE_RVA = 0x7F2BF0; // sub_1407F2BF0 WindImpulseVolumeUpdate work
constexpr uintptr_t GI_APPLY_RVA        = 0x7F4230;   // sub_1407F4230(applyMgr, a2) GI apply
constexpr uintptr_t RENDERER_GLOBAL_RVA = 0x3BBCAE8;  // qword_143BBCAE8
// The sun's shadow cascades. Each view renders its OWN (measured in a capture: two separated runs of
// cascade draws per frame), so what each one draws into its map is worth counting per view.
constexpr uintptr_t CASCADE_NODE_RVA = 0x4EDFE0;  // sub_1404EDFE0 RenderShadowCascade work
// ...and the clear that precedes it. Both belong to whichever view is rendering, and the atlas they touch
// is ONE shared resource (proven when the view that skipped its build sampled populated-but-stale content
// rather than nothing), so cutting the pair for one view makes that view reuse the other's atlas.
constexpr uintptr_t CASCADE_CLEAR_NODE_RVA = 0x22A3F80;  // sub_1422A3F80 ClearShadowCascades work
// The node that uploads the lighting globals -- where the cascade SAMPLING matrices belong, as opposed to the
// cascade record the rasteriser builds its own constants from.
constexpr uintptr_t LIGHTING_GLOBALS_NODE_RVA = 0xC45C70;  // CRenderNode_BindLightingGlobalConstants
// ...and it is only a BIND: 84 bytes, one callee, pointing the root signature at a buffer somebody else
// filled. It uploads nothing, which is why a probe aimed at it printed nothing at all.
// The volumetric fog pass, whose froxel grid is what the blocky sun shafts in a room actually are.
constexpr uintptr_t VOLUMETRIC_FOG_NODE_RVA = 0xDDB6B8;  // CRenderNode_VolumetricFog work
// The cone ambient occlusion: five compute shaders over one block of its own (b3).
constexpr uintptr_t CONE_AO_NODE_RVA = 0x777514;  // DrawConeAO work
// The auto-exposure: luminance image, histogram and adaptation, each dispatch over a small block (b6).
constexpr uintptr_t HISTOGRAM_UPDATE_NODE_RVA = 0x776B04;  // HistogramUpdate work
constexpr uintptr_t SSR_NODE_RVA = 0x77A100;  // ScreenSpaceReflections work
constexpr uintptr_t PS_B6_UPLOAD_RVA = 0x41A56C; // sub_14041A56C(size, src): pixel-stage b6 constant block
constexpr uintptr_t SSPR_NODE_RVA = 0x77D450; // ScreenSpacePlanarReflections work (same CB_SSRRaymarch upload)
// The per-BIT feature test the nodes themselves use: sub_14033E358(work_ctx, bit_index). NOT the same
// function as VIEW_FEATURE_CHECK_RVA (sub_14021BE28), which takes a render-mask DESCRIPTOR -- that
// confusion is why a probe hung on the latter never fired for the shadow-mask pass.
constexpr uintptr_t FEATURE_BIT_TEST_RVA = 0x33E358;
constexpr uintptr_t RENDER_SHADOWMASK_RVA = 0x777D0C;  // sub_140777D0C RenderShadowmask work
// The global (SpeedTree) wind. Behind the same once-per-frame latch, and measured to upload its
// 416-byte parameter blocks for MAIN only (29638 against 0) -- which is what leaves the second view's
// cascade one wind step behind. See Detour_SpeedTreeWind.
constexpr uintptr_t SPEEDTREE_WIND_RVA = 0xD60084;  // sub_140D60084 AdvanceSpeedTreeWind work
constexpr uintptr_t DOCULLING_RVA = 0xBF436C;   // sub_140BF436C CRenderNode_DoCulling
constexpr uintptr_t MAIN_CULL_PREP_RVA = 0xC65218; // sub_140C65218 scene-system candidate gather
constexpr uintptr_t MAIN_CULL_CTX_INIT_RVA = 0x2286CC0; // sub_142286CC0 build shared gather-context (2.31: 0x623FD8; 2.5 added a bool 4th arg)
constexpr uintptr_t MAIN_CULL_TEST_RVA = 0xC651C4; // sub_140C651C4 -> sub_14029CCEC tester loop
constexpr uintptr_t VIS_COLLECTOR_RVA = 0x4ED29C;  // sub_1404ED29C tagged candidates -> fresh view output
constexpr uintptr_t FINE_MATERIALIZE_RVA = 0x29D110; // sub_14029D110 per-drawable fine test/materialize
constexpr uintptr_t VISIBLE_APPEND_RVA = 0x19B75C;   // sub_14019B75C append visible drawable ID
constexpr uintptr_t MATERIALIZE_WORKER_RVA = 0x4ED62C; // sub_1404ED62C worker wrapper
constexpr uintptr_t PREPARE_STAGE_RVA = 0x229D3E0;   // sub_14229D3E0 gather/filter/finalize wrapper
constexpr uintptr_t PREPARE_GATHER_RVA = 0x4EE6BC;    // sub_1404EE6BC flatten worker buckets
constexpr uintptr_t PREPARE_FILTER_RVA = 0x229D240;   // sub_14229D240 filter/classify descriptors
constexpr uintptr_t PREPARE_FINALIZE_RVA = 0x426AE4;  // sub_140426AE4 sort/finalize descriptors
constexpr uintptr_t PREPARE_SORT_A_RVA = 0x427AC8;    // first mode1 index-domain sort
constexpr uintptr_t PREPARE_SORT_B_RVA = 0x427F00;    // second mode1 index-domain sort
constexpr uintptr_t PREPARE_SORT_C_RVA = 0x428330;    // third mode1 index-domain sort
constexpr uintptr_t PREPARE_SORT_FINAL_RVA = 0x695F80; // final 16-byte descriptor sort
constexpr uintptr_t DRAWCOMP_RVA = 0x33D0AC;   // sub_14033D0AC
constexpr uintptr_t COMP_ELEM_KEYS_RVA = 0x2ABA18;   // sub_1402ABA18, the element's draw keys
constexpr uintptr_t COMP_ELEM_RECT_RVA = 0x2AC150;   // sub_1402AC150, the element's scissor rect
constexpr uintptr_t LIGHTBUFFERS_RVA = 0x411B94;   // sub_140411B94 RenderLightBuffers
constexpr uintptr_t SL_CONSTANTS_RVA = 0x7EE0BC;   // sub_1407EE0BC
constexpr uintptr_t DLSS_EVAL_RVA  = 0x228F210;   // sub_14228F210 slSetTag+slEvaluateFeature
constexpr uintptr_t DLSS_CONST_RVA = 0x7EEA54;    // sub_1407EEA54 slSetConstants
constexpr uintptr_t READBACK_NODE_RVA = 0x2E9E3EC;   // sub_142E9E3EC
constexpr uintptr_t APPLYDLSS_WORK_RVA = 0x77E4C8;   // sub_14077E4C8 (ApplyDLSS node work)
constexpr uintptr_t FFX_API_PROVIDERS_RVA = 0x2287A20;   // sub_142287A20: FidelityFX API upscaler versions (FSR 2/3/4)
constexpr uintptr_t GETTER_RVA = 0x3DFEA4;   // sub_1403DFEA4 view-dims getter entry
constexpr uintptr_t RENDER_RES_RVA = 0x425054;   // sub_140425054 per-view DLSS render-res compute
// The engine config variables Player/Input InputContextFile and InputMappingFile: static objects built by
// sub_140178E60 / sub_140178F40 (sub_1412C2148: vtable +0, name +8, group +16, value +48), string-variable vtable
// whose slot 3 sets the value from a string view {const char*, u32 length}. Core/InputConfig.cpp.
constexpr uintptr_t INPUT_CONTEXT_FILE_VAR_RVA = 0x3C29DA0;
constexpr uintptr_t INPUT_MAPPING_FILE_VAR_RVA = 0x3C29CE0;
constexpr uintptr_t CONFIG_STRING_VAR_VTABLE_RVA = 0x31D8940;
// Scripts/EnableCompilation (sub_1400B2140 builds it; sub_140AE18D0 reads it at +48 and then starts scc.exe through
// sub_140AE2730), a bool config variable: same layout, its vtable's slot 3 sets it from a string view too.
// Core/ScriptCompiler.cpp.
constexpr uintptr_t SCRIPTS_ENABLE_COMPILATION_VAR_RVA = 0x3915C68;
constexpr uintptr_t CONFIG_BOOL_VAR_VTABLE_RVA = 0x3078FA0;

}  // namespace detail
}  // namespace cvr
