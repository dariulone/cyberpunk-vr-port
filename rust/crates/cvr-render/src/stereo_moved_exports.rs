//! The culling, core and stereo sync exported state (formerly definitions in src/Stereo/CullVisibility.cpp,
//! src/Core/VrCore.cpp and src/Stereo/SyncStereo.cpp that no C++ code of the Rust build reads any more): defined on the
//! Rust side, with the notes that stood over them, and exported from rust/exported-data.txt; the Rust modules and the
//! tools read them by name.
#![allow(non_upper_case_globals)]

// ---- src/Stereo/CullVisibility.cpp

#[no_mangle]
pub static mut CyberpunkVR_OcclusionGateForce: u32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugOcclGateForced: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOcclGateAlreadyOn: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PrepareProfileEnable: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PrepareCacheAuditEnable: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareStageTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareStageTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareGatherTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareGatherTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFinalizeTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFinalizeTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortATicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortATicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortBTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortBTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortCTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortCTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortFinalTicksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSortFinalTicksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareCallsMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareCallsVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketStage: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketMode: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketTicksMain: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketTicksVrcam: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketDescMain: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketDescVrcam: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketCallsMain: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareTopBucketCallsVrcam: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareHistCallsMain: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareHistCallsVrcam: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareHistDescMain: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareHistDescVrcam: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareHistTicksMain: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareHistTicksVrcam: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketCallsMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketCallsVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketDescMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketDescVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketTicksMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketTicksVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortATicksMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortATicksVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortBTicksMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortBTicksVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortCTicksMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortCTicksVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortFinalTicksMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketSortFinalTicksVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketCountSqMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareBucketCountSqVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterInMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterInVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterOutMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterOutVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterCallsMain: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareFilterCallsVrcam: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareCacheHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareCacheMisses: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareCacheHitDescriptors: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSetHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSetMisses: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPrepareSetHitDescriptors: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCullSkipHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLocalCtxZeroHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBlockReuseHits: u64 = 0;
/// mode6: GraphContextPrepare reuses pool addresses sequentially. At entry its active list
/// still names the PREVIOUS subgraph. Live order proved old=VRCAM -> GraphContextPrepare ->
/// DoCulling MAIN. During that transition, preserve the entire VRCAM graph container by
/// skipping sub_1404ECEA0 (which otherwise clears both visibility buckets and payload
/// metadata), then skip MAIN's duplicate cull. MAIN draw consumes VRCAM's current-frame
/// output. The next MAIN->VRCAM transition performs the normal reset, so nothing accumulates.
#[no_mangle]
pub static mut CyberpunkVR_DebugContainerRedirectHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisibilityResetSkipHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEndRenderResetSkipHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCullReuseHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisibilityBatchCaptures: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisibilityBatchReplays: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisibilityCandidatesCaptured: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVisibilityCandidatesReplayed: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCullPrepareSkips: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CullCallbackProfileEnable: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackMethodRva: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackCallsMain: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackCallsVrcam: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackDescMain: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackDescVrcam: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackTicksMain: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugCullCallbackTicksVrcam: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_DebugFineCandidateCaptures: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFineCandidateReplays: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFineCandidateFallbacks: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFineDrawableIdsCaptured: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFineDrawableIdsReplayed: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_MaterializeProfileEnable: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeWorkerTasksMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeWorkerTasksVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeTaggedMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeTaggedVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeFineCallsMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeFineCallsVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeFineRangesMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeFineRangesVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeDrawableAppendsMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeDrawableAppendsVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeUniqueMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeUniqueVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeDuplicateMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeDuplicateVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeProbeOverflowMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeProbeOverflowVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeCtxUniqueMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeCtxUniqueVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeCtxDuplicateMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeCtxDuplicateVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMaterializeRangeCtxProbeOverflowVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_LodThreshValue: f32 = 1.0;
/// b0=MAIN b1=VRCAM b2=other
#[no_mangle]
pub static mut CyberpunkVR_LodThreshApplyMask: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLodThreshHitsMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLodThreshHitsVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLodThreshHitsOther: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLodThreshSeenMainBits: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLodThreshSeenVrcamBits: u32 = 0;

// ---- src/Core/VrCore.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugBdCamDirty: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdCamPose: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdPushTransform: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdPushMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdMainPos: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdEditorAlign: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdBaseFromLocate: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdBaseFromScript: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamRebinds: u64 = 0;
/// 1 = LocateCamera composes and PatchCamera writes (the correct split, see the comment at the
/// write site). 0 = the legacy path, orientation written into LocateCamera's serialiser buffer.
/// Exported so the two can be compared live without a rebuild.
/// 0 = the MONO path: LocateCamera composes AND writes the orientation. 1 = the stereo-era split,
/// PatchCamera writes it.
///
/// Back to 0, 2026-07-30, on the observation that mono never had this twitch. The two sites sit at
/// different points in the frame: PatchCamera is the gameplay tick, LocateCamera runs later, during
/// render. Writing early leaves the engine's own procedural camera pass to run AFTER us, so what
/// reaches the frame is its result blended over ours -- measured as 0.18 deg and 0.4 mm of change
/// at frame open while the head sample, the heading and our composed quaternion were all frozen.
/// Writing late overwrites that pass instead, which is exactly what mono did.
///
/// The orientation is the same for both eyes, so it does not need the per-view split that the
/// POSITION does -- the eye separation stays where it is, in the write callback.
/// BACK TO 1. Tried at 0 (the mono path, orientation written in LocateCamera) on the reasoning
/// that mono never twitched: it did not help, and it cost VRCAM its orientation entirely --
/// LocateCamera writes the located buffer, and the second view's camera object never receives it.
/// So the split is not optional in stereo: the orientation has to be written per view, where the
/// view is known.
/// ISOLATION TEST DONE, BACK TO 1. Writing BOTH orientation and position into the
/// SerializeSetup buffer instead of the component -- i.e. inside the director update,
/// below the component and above the blender -- left the mouse-turn trail EXACTLY as it
/// was. So the trail does not come from this write, from its stage, or from anything the
/// value passes through between the component and the blender. Recorded because it is a
/// clean negative: the camera write is no longer a suspect, and the buffer path is not an
/// option in stereo anyway (it cannot tell MAIN from VRCAM).
#[no_mangle]
pub static mut CyberpunkVR_CamWriteInPatch: i32 = 1;
/// 1 = compose at the write site (above). 0 = the previous split, where LocateCamera composed
/// and PatchCamera copied. Live-switchable so the two can be compared inside one session.
///
/// LEFT AT 1. Of the four flags that were still unexamined -- this one, BindPoseToImage,
/// PoseReadBack and CamFinalRowOrder -- only this one can change a rendered pixel; the other three
/// decide which pose LABEL is attached to a frame that has already been drawn. But the argument for
/// moving it (the aim epoch advances at display rate while the camera is written at game rate, so
/// the composes-per-frame count alternates) requires the two rates to differ, and the twitch is
/// there at 90+ fps in mono as well. Rate mismatch is not the mechanism. Not touched.
#[no_mangle]
pub static mut CyberpunkVR_CamComposeAtWrite: i32 = 1;
/// 1 = locate the head afresh at the camera write, aimed at the predicted display time of the
/// frame being built. 0 = read the cached atomics the frame-loop thread
/// refreshes, whose age relative to the write wanders frame to frame.
#[no_mangle]
pub static mut CyberpunkVR_PoseLocateAtWrite: i32 = 1;
/// 1 = LocateCamera's translation and PatchCamera's orientation share ONE head sample per frame
/// (AcquireFrameHeadSample). 0 = the previous arrangement, position from the smoothed cache and
/// orientation from a separate locate. Live-switchable so the difference can be felt directly.
#[no_mangle]
pub static mut CyberpunkVR_OneSamplePerFrame: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseLocatedAtWrite: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPoseFromCache: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamComposed: u64 = 0;
/// How often VRCAM, not MAIN, was the first camera the engine updated in an interval. Non-zero
/// means the order really is not fixed, which is the whole reason composition moved here.
#[no_mangle]
pub static mut CyberpunkVR_DebugCamVrcamFirst: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamNoHmd: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTidPatchCam: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTidLocateCam: u32 = 0;
/// ---- THE PER-VIEW WRITE SITE (mono) ---------------------------------------------------------
///
/// 1 = drive both views from CRenderNode_PrepareSceneRendering's camera fix-up (see
/// OnFinalCameraCallback), which is per-view, runs at frame open, and writes the very object the
/// view-matrix bake reads. 0 = the current arrangement, where PatchCamera writes the placed
/// component and VRCAM needs a separate translation patch.
///
/// OFF, and the reason is worth keeping: FINAL CAMERA IS A CONSUMER, NOT THE SOURCE.
///
/// Tried and rejected on evidence. Writing the render camera here rotates the rasterised near
/// geometry correctly, but everything the engine had ALREADY derived from the camera earlier in
/// the frame -- culling frustum, shadow-cascade setup, distant/imposter selection, the previous
/// frame's matrices feeding TAA/DLSS -- stays on the engine's un-written value. The result on
/// screen is exact and diagnostic: near objects stay world-locked while distant geometry and
/// shadows drag with the head, because half the frame is built from one camera and half from
/// another.
///
/// The chain is component transform -> view producer (sub_1401EDBB8 / sub_1403D6EB0) -> render
/// camera (ctx+0x18) -> view matrices (sub_1407EE0BC). PrepareSceneRendering's fix-up and
/// SetStreamlineConstants both sit BELOW the producer, so both are downstream of the decisions
/// that already used the camera. Only a write at the component -- PatchCamera -- is upstream of
/// all of them, which is why that is where the engine's own writer lives.
///
/// Counters from the attempt, for the record: ViewCamMain 6192, ViewCamVrcam 5625 (both views DO
/// reach the site once the view test used the dispatcher's tags instead of a component-name hash),
/// ViewCamOther 0 (there are no extra views here at all).
#[no_mangle]
pub static mut CyberpunkVR_CamWriteInFinal: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewCamMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewCamVrcam: u64 = 0;
/// Views that are neither eye: distant/imposter, reflection, shadow. Counted separately because
/// how many there are per frame decides whether they can be the cause of anything.
#[no_mangle]
pub static mut CyberpunkVR_DebugViewCamOther: u64 = 0;
/// 1 = give every view in the image the head orientation (see the write site). 0 = only the two
/// eye views, which left distant geometry and shadows turning with the head.
#[no_mangle]
pub static mut CyberpunkVR_CamFinalViewScope: i32 = 1;
/// 1 = give VRCAM the same head translation MAIN gets. Live-switchable to isolate it.
#[no_mangle]
pub static mut CyberpunkVR_VrcamHeadTranslation: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamPosWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainPosWrites: u64 = 0;
/// 1 = take the body heading from the component's PRE-WRITE world rotation at the write site,
/// instead of the g_headingSy/Cy pair LocateCamera publishes. Those are published downstream of
/// this write (LocateCamera runs inside the blender), so the cached pair is one frame old and the
/// camera lags the body by heading-change-per-frame -- 4.2 deg at a 300 deg/s mouse turn. See the
/// use site in PatchCamera.
#[no_mangle]
pub static mut CyberpunkVR_HeadingFromPreWrite: i32 = 1;
/// 1 = rebuild the head translation from the frame's OWN head sample using the recipe
/// LocateCamera publishes a framed AnchorRecipe through CameraLink, instead of
/// reading g_headDeltaFP -- which LocateCamera computes AFTER this write and therefore belongs
/// to the previous frame. Same class of defect as the stale heading, same remedy.
#[no_mangle]
pub static mut CyberpunkVR_DeltaFromFreshSample: i32 = 1;
/// The two headings the write site can see, in degrees: the one extracted from the component's
/// pre-write world rotation, and the cached pair LocateCamera publishes. Live, so a constant
/// value is immediately visible as constant.
#[no_mangle]
pub static mut CyberpunkVR_DebugHeadingPreWriteDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHeadingCachedDeg: f32 = 0.0;
/// 1 = the solve takes its world->model yaw from the ENGINE body yaw (published by the write site
/// sub_140336390, see src/Hooks/BodyYawCensus.cpp) instead of the view packet heading. Measured:
/// the yaw is stored 0.22-0.79 ms BEFORE the animation batch, while the packet heading is a frame
/// old, and the two differ by 5-10 deg on an ordinary mouse turn. Live flag so the two can be
/// compared without a rebuild.
#[no_mangle]
pub static mut CyberpunkVR_VrikYawFromEngine: i32 = 1;
/// 1 = the VIEW takes its yaw from the ENGINE body yaw (what the mouse and the stick produce)
/// rather than from the camera component, which inherits the entity yaw and therefore carries the
/// body-follow offset. This is what makes "the body turns, the camera does not" possible at all --
/// see the use site in PatchCamera. It also drops a frame of age from the heading.
#[no_mangle]
pub static mut CyberpunkVR_ViewYawFromEngine: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugSceneStickYawDeg: f32 = 0.0;
/// Use a coherent relative camera/entity snapshot for the model-space anchor and the same latched XR
/// head sample as PatchCamera for its orientation.  See VRIK_ComputeCamModel; g_lastLocate* is too
/// late in the frame to pair with the entity transform consumed by animation.
#[no_mangle]
pub static mut CyberpunkVR_VrikTransformsFromPlugin: i32 = 1;
/// Remove the final script clock from VRIK transforms: LocateCamera's preceding frame is paired with
/// the preceding engine entity transform by BodyYawFollowTick and published atomically.
#[no_mangle]
pub static mut CyberpunkVR_VrikNativeFramePair: i32 = 1;
/// 1 = while MOUNTED, the solve takes its world->model frame from the Lua pair, whose entity
/// quaternion is the FULL entity world orientation, instead of the native pair's Rz(yaw)
/// reconstruction. Seated in a car the body pitches and rolls with the shell, so a yaw-only frame is
/// wrong by a quantity that changes every frame the car moves -- the jitter. On foot the entity is
/// upright, yaw is the whole of it, and the native pair's engine clock is the better source, so this
/// changes nothing there. 0 restores the previous behaviour.
#[no_mangle]
pub static mut CyberpunkVR_VrikVehicleFullEntityQuat: i32 = 1;
/// 1 = while MOUNTED, the play-space anchor is rotated by the yaw the VIEW was composed with, instead of
/// by the body's own forward.
///
/// MEASURED FROM THE SYMPTOM: the jitter is present only while the car is TURNING -- not parked, not
/// driving straight -- and it is visible on the flat monitor, i.e. in the image this port composes. That is
/// the signature of the head offset being taken into the world by a yaw on a slower clock than the frame:
/// each render the car has turned further than the yaw has, so the eye sits slightly wrong by an amount
/// proportional to the turn rate. LocateCamera used the body's forward (entity/tick clock) while the view
/// came from the camera's pre-write quaternion (assembled per rendered frame). One clock instead of two.
///
/// An earlier attempt put both on g_VREntityQ*, the Lua-pushed entity quaternion -- the coarsest clock of
/// the three -- and made it worse. 0 restores the body-forward behaviour.
#[no_mangle]
pub static mut CyberpunkVR_VehicleAnchorFromViewYaw: i32 = 1;
/// TEST BUILD: 0 = our composed orientation is NOT written while mounted, so the engine's own vehicle
/// camera stands. The head stops turning the view in a car, which is why this is a test rather than a
/// setting -- it exists to answer one question. Jitter gone at 0: our write is in a fight with the
/// camera's bound-forward constraint. Jitter still there: the orientation was never what moved.
/// PROVEN BY THE TEST BUILD: with this at 0 -- our composed orientation not written while mounted --
/// the in-vehicle jitter disappeared entirely. So the jitter IS our write against the game's own
/// vehicle-camera heading reset, and not the frame rate, not the yaw source and not the anchor.
///
/// Back at 1 because 0 also stops the head turning the view in a car, which is not a shippable
/// trade. The cure is to take the game's heading reset out of the loop rather than to stop writing:
/// fppCameraParamSets.Vehicle carries headingLocked, headingResetSpeed, headingResetTimeout,
/// headingResetOnlyWhenMoving, normalizeYaw and the yaw/pitch rubber band -- and
/// headingResetOnlyWhenMoving alone explains why a parked car is clean.
#[no_mangle]
pub static mut CyberpunkVR_CamWriteOrientInVehicle: i32 = 1;
/// Retain the exports for diagnostic compatibility. Per-eye yaw catch-up was
/// removed: vehicle MAIN and VRCAM have different native attachment headings.
#[no_mangle]
pub static mut CyberpunkVR_YawCatchUpOnSharedEpoch: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugYawCaughtUp: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrikNativePairUsed: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrikLuaPairFallback: u64 = 0;
/// The head bone follows the freshest camera sample, while arm/controller math keeps the stable
/// rotation from the coherent camera/entity push. Set to 0 live to reproduce the mixed-frame arm path
/// without also disabling the phase-coherent body anchor.
#[no_mangle]
pub static mut CyberpunkVR_VrikSplitHeadHandRot: i32 = 1;
/// 1 = the shoulder anchor and the arm frame come from the avatar's own bones (neck for the girdle,
/// root->head and the shoulder line for the axes) instead of from the camera. With the camera they
/// follow the HEAD -- the elbows swing when you look around -- and after the camera-onto-head bake
/// they moved back with it by the baked (0.093, -0.428) and dragged the chest and armpit along.
#[no_mangle]
pub static mut CyberpunkVR_VrikArmAnchorFromBody: i32 = 1;
/// 1 = the elbow policy at the end of VRIK_SolveArm: blend toward a down/back rest direction as the
/// hand approaches the shoulder (VRArmIK's fixed-elbow-near-shoulder rule, its own constants), and
/// cap the elbow height at min(wrist, shoulder) unless the hand is raised above the shoulder. Both
/// keep the elbow close to the body and out of the poses a free IK likes and a human never uses.
#[no_mangle]
pub static mut CyberpunkVR_VrikElbowPolicy: i32 = 1;
/// Distance between where the solved hand lands in the WORLD and where the controller actually is,
/// millimetres, right hand. Zero means the hand is on the controller. See the compute site in
/// AnimPose: it is the only way to tell which frame is wrong when "the hands ride the body".
#[no_mangle]
pub static mut CyberpunkVR_DebugHandWorldErrMm: f32 = 0.0;
/// How far the SOLVED wrist ended up from the target it was given (mm), and how far that target sits
/// from the shoulder as a fraction of arm length. The pair separates "the target is wrong" from "the
/// arm could not reach it" -- see the compute site right after VRIK_SolveArm.
#[no_mangle]
pub static mut CyberpunkVR_DebugHandMissMm: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHandReachRatio: f32 = 0.0;
/// The ENGINE's camera position, fixed point, as it stood before PatchCamera added the head
/// displacement. The body and the shoulders hang off this; reconstructing it from the view minus the
/// delta put the physical head motion back in with the wrong sign. See the publish site.
#[no_mangle]
pub static mut CyberpunkVR_EngineCamPosFP: [i32; 3] = [0; 3];
#[no_mangle]
pub static mut CyberpunkVR_EngineCamPosValid: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTakeoverPush: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DevCamOffsetX: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DevCamOffsetY: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DevCamOffsetZ: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdIpdLocate: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdQuatFromWriteSite: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BdSceneBaseInPatch: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdSceneBaseInPatch: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdQuatFromBuffer: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamPosFromMain: u64 = 0;
/// Horizontal distance from the character's origin to the FPP camera -- the radius the body
/// follower's heading sweeps the view along. Sizes the "head and body are not in the same place
/// after a turn" residual: slide = radius * 2*sin(realign/2).
#[no_mangle]
pub static mut CyberpunkVR_DebugCamMountM: f32 = 0.0;
/// OFF BY DEFAULT, and that is deliberate: this MOVES THE CAMERA.
///
/// The engine's heading sweeps the FPP camera along the mount circle when the body follower turns, so
/// the view slides sideways even though the player's real head did not move. Taking that back out means
/// writing the camera position -- the one thing that has to be earned rather than assumed, and the
/// first version of it broke the game outright by reading back its own write and compounding across
/// passes. This version cannot compound (it is a function of angles and a learned mount vector), but
/// whether the slide or the correction feels worse is a judgement, not a derivation, so it ships off
/// and can be flipped live without a restart.
#[no_mangle]
pub static mut CyberpunkVR_CamMountCompensate: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugViewYawFromEngine: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDeltaRebuilt: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugIpdWorldWrites: u64 = 0;
/// The legacy write into component+0x100/0x110 ("posA/posB"). OFF: measured to have no effect on
/// the rendered viewpoint -- the two render cameras stayed 23 micrometres apart with it enabled.
/// Kept switchable only so the old behaviour can be restored in one session if something depended
/// on those fields for a reason we have not found.
#[no_mangle]
pub static mut CyberpunkVR_IpdInPosAB: i32 = 0;
/// Counts how often the camera write arrives on a DIFFERENT thread than the previous one. A
/// value that stays near 1 means the site is effectively single-threaded for cameras; one that
/// climbs with the frame count means it is not, and everything the write site touches has to be
/// safe against that -- which is why the composition below is a compare-exchange and the
/// quaternion a seqlock rather than four plain stores.
#[no_mangle]
pub static mut CyberpunkVR_DebugCamThreadSwitches: u64 = 0;
/// THE DEVICE'S OWN CAMERA, which is what a surveillance camera hands the view to. Its component is
/// named `cameraComponent` -- read off the live SurveillanceCamera through the bridge, not guessed -- and
/// the hash is computed with the same function the VRCAM name uses, so a mistyped literal cannot silently
/// classify nothing (every wrong guess at this has been silent, which is the note above this block).
#[no_mangle]
pub static mut CyberpunkVR_DeviceCamFollow: i32 = 1;
/// Writing the HEAD POSE into the device camera. Off, and this is a measurement rather than caution: with
/// it on the view started aimed at a wall and MAIN's frames jumped about while VRCAM's did not, which is
/// what a fight with the camera mixer looks like -- the player's camera and the device's are both active
/// during a takeover and the blender weights them. Getting the second eye onto the lens does not need it.
/// BACK TO 0. With it on the view aimed at a wall, blinked, the FOV did not match between the eyes and
/// VRCAM juddered on head turns -- four symptoms of one cause: MAIN is not a write at this site, it is
/// a chain (LocateCamera composing and PUBLISHING the pose label the compositor reprojects against, the
/// FOV override, the located buffer, FinalCamera), and a device camera was given only the write. Head
/// steering inside a surveillance camera needs that whole chain pointed at it, which is a piece of work
/// and not a knob. Off, the eyes both look along the lens and nothing artefacts.
#[no_mangle]
pub static mut CyberpunkVR_DeviceCamOrient: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugLensHeadWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPatchCamDevice: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCamRejects: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamCamRejects: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdCandidates: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBdCamHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPatchCamOther: u64 = 0;

// ---- src/Stereo/SyncStereo.cpp

#[no_mangle]
pub static mut CyberpunkVR_DebugMainNodeCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryNodeCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainNodeUnique: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryNodeUnique: u32 = 0;
/// WORK-verification: per-node (index parallel to *NodeWorks) accumulated rdtsc
/// cycles + call counts inside the work_fn, per view. A node that DISPATCHES but
/// internally SKIPS (early-out) shows near-zero avg cycles for that view -> lets
/// us prove WORK (not just dispatch) and compare vrcam vs main per node.
#[no_mangle]
pub static mut CyberpunkVR_DebugMainNodeCycles: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryNodeCycles: [u64; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugMainNodeCallN: [u32; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugSecondaryNodeCallN: [u32; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorCopyArms: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorGameCopies: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeBuildMode: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeBuildHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainFgHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainBuildMode: u32 = 4294967295;
/// eye-X manager state (view/context count @+0x54, view array @+0x48, queued
/// request slot @+296)  determines whether we can queue a context request.
#[no_mangle]
pub static mut CyberpunkVR_DebugEyeMgrCount: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainMgrCount: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCtxKey: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCtxA5: i32 = -1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCtxCreateCalls: u64 = 0;
/// Descriptor-heap probe/enlarge (Path A: full second eye needs a bigger
/// shader-visible CBV_SRV_UAV heap than the engine's 1,000,000 request).
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapCreates: u64 = 0;
/// last shader-visible CBV_SRV_UAV NumDescriptors requested
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapSVNum: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapSVFlags: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapEnlarged: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainObjBinds: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCtxBinds: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMainCtx: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugMirrorCopyNodeHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamObjMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamObjVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamBindMain: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamBindVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCamScanFails: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTonemapSnaps: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoVrcam: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoMain: u64 = 0;
/// Tonemap node (work RVA 0x557204) resolves its frame-graph resources with IDs
/// ((work_context+0x34)<<24) XOR const -- +0x34 is the per-view graph-instance salt.
/// If vrcam's tonemap salt flaps between its own and main's value, it resolves MAIN's
/// adapted-exposure buffer on those frames => the two bit-exact luma states. Log salt
/// TRANSITIONS for both views + export last values for x64dbg polling.
#[no_mangle]
pub static mut CyberpunkVR_DebugTmSaltVrcam: u32 = 4294967295;
#[no_mangle]
pub static mut CyberpunkVR_DebugTmSaltMain: u32 = 4294967295;
/// THE FLAP SELECTOR (decompiled from the RenderFinal2D node, sub_140209FF8):
/// salt = (ctx[0x30] & 1) ? 0 : ctx[0x34];   // bit0 => "borrow MAIN's instance"
/// source_id = (salt<<24) ^ 0x3D7E6258;
/// With forced VrcamDlss the graph's "does this view own a post buffer" decision races
/// -> bit0 flaps -> the final blit alternately reads MAIN's post buffer (bright) and
/// vrcam's own (normal): two stable sources = two bit-exact luma states. The vrcam
/// post chain provably runs every frame (histogram 6/frame, tonemap each frame), so
/// borrowing is always wrong here: force bit0=0 for vrcam-ctx nodes.
#[no_mangle]
pub static mut CyberpunkVR_ForceOwnPost: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugOwnPostFixes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugResolveHits: u64 = 0;
/// SAFETY DEFAULT 0: physical indices are PER-FRAME transient (proven: [pv] shows fin
/// idx changing every frame at a FIXED gen=2), so substituting any remembered index =
/// freed slot next frame = null deref (crashes 180201/180846). Override stays off until
/// we identify a PERSISTENT correct index via the safe luma<->finidx correlation below.
#[no_mangle]
pub static mut CyberpunkVR_FixPostVersion: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPinApplied: u64 = 0;
/// last pinned fin idx
#[no_mangle]
pub static mut CyberpunkVR_DebugFinPinned: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTmPostIdx: u32 = 0;
/// Legacy export compatibility only. TE6 separates the graph cache by view;
/// it removes this post-color handle substitution. A resolve-count window did
/// not prove resource lifetime and must not override the newly isolated graph.
#[no_mangle]
pub static mut CyberpunkVR_DealiasPostColor: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDealiasHits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugStableSrcState: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLumaEvenMilli: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLumaOddMilli: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLumaDeltaMilli: u32 = 0;
/// Waveform: log the first N per-frame luma samples ("[lumaw] fr=.. L=..") so the
/// oscillation SHAPE is visible (strict period-2 square = history ping-pong misread;
/// multi-frame sawtooth/pulse = exposure-adaptation feedback limit cycle). Decrements
/// per line; re-arm live by writing a new count via x64dbg.
#[no_mangle]
pub static mut CyberpunkVR_LumaWave: i32 = 300;
/// Set by the composite's own validity check: the engine's b6 was bound and its target size
/// matched ours, so every constant came from the engine rather than from the capture.
#[no_mangle]
pub static mut CyberpunkVR_DebugHudCbUsed: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_Debug2RtBinds: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDescHeapResized: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttHits: u64 = 0;
/// Extra debug exports so x64dbg can read the live RTT component + the dims the
/// view-create actually receives, regardless of the (default-off) override.
#[no_mangle]
pub static mut CyberpunkVR_DebugRttW: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttH: u32 = 0;
/// 1=resize dtex -> target
#[no_mangle]
pub static mut CyberpunkVR_RttResizeMatchMain: u32 = 0;
/// explicit W (0=use main)
#[no_mangle]
pub static mut CyberpunkVR_RttResizeW: u32 = 0;
/// explicit H (0=use main)
#[no_mangle]
pub static mut CyberpunkVR_RttResizeH: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttResizeHits: u64 = 0;
/// current dtex W (live)
#[no_mangle]
pub static mut CyberpunkVR_DebugRttDtexW: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRttDtexH: u32 = 0;
/// Components skipped because their dims are not the selected resolution. Non-zero here is
/// normal: it counts the ones we correctly refused to point the fov writes at.
#[no_mangle]
pub static mut CyberpunkVR_DebugRttCompRejects: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBlockV5ReuseHits: u64 = 0;
/// default ON
#[no_mangle]
pub static mut CyberpunkVR_StreamlineHistoryFix: u32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugSlHistoryHits: u64 = 0;
/// The framegraph builder-selection in sub_140409DD4 dispatches on the view's
/// BUILD-mode = *(view+0xF90) (v93): <2 -> full gameplay (sub_141D43040, emits
/// StartRender/ExtractionFinalColor/ClearFinalColorTarget/Present), ==2 HitProxy,
/// ==4 -> sub_141D47FF0 (full scene, NO final-color/present), etc. This is the
/// field that actually gates the "main-only" output nodes -- NOT the AA mode we
/// already mirror (0xF94). Capture both live in the always-running SL hook.
/// main  *(view+0xF90)
#[no_mangle]
pub static mut CyberpunkVR_DebugMainBuildModeF90: u32 = 4294967295;
/// vrcam *(view+0xF90)
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamBuildModeF90: u32 = 4294967295;
