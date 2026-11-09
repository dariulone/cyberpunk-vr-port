# SteamVR completed HUD sources, November 6, 2026

Candidate hud-ready-v1, based on overlay-queue-v1. RR crash remains deferred.

## Evidence and scope

The 18:59 run confirmed zero overlay application/runtime round trips, but RT+NR
still delivered a72.56FPS compositor estimate at~44real+44generated accepted/s.
Whole-presentation readiness was1.02%, and only0.11%of newly copied overlay
sources had completed at enqueue. Previous consumer receipt was pending in
89/156 sampled synthesis pairs. These counts overlap: they do not prove HUD
waits account for all delay. RT raised generation span from5.31to8.88ms too.

The runtime queue waits for a newly selected HUD canvas even if that canvas
belongs to newer game work than the scene being presented. Removing the previous
queue round trip did not remove that exact-source wait. A later scene copy or
synthesis consumer join can inherit this delay.

## Change

On the SteamVR private presentation path with the normal HUD copy ring enabled,
select completed HUD canvases before recording GPU work. Prefer newest completed
publication. If newest is pending, retain one candidate until it completes;
do not continually replace it with newer pending publications (starvation).
Use the last published XR image meanwhile, updating its head/hand-follow pose
every cycle. Keep its original mask/loot metadata until the new image publishes.

No source ready: no acquire/copy/dependency is queued. First appearance waits
until a source completes. The existing300ms published-content expiry remains.
Empty channels hide immediately and invalidate the old publication so a later
pending canvas cannot revive a vanished prompt. Disabled/missing-source/shutdown
clear the pending lease; resize discards a pending source of incompatible size
and applies the new size only once ready. Source/copy leases and exact producer
receipts are retained; a second receipt wait on a completed fence is harmless
and retains the established cross-queue synchronization contract.

This applies to the five HUD Quad channels, not scene interpolation, F10's
interactive canvas, or the CPU-rendered stats overlay. VDXR/shared queue and
legacy non-pipelined HUD path retain previous selection. No extra GPU resource,
command list or CPU wait. One retained shared_ptr per HUD channel uses the
existing producer pool. HUD content can trail by a display cycle or more under
load; pose does not wait for content. No claimed headset FPS benefit yet.

## Diagnostics and validation

Recorderheader upstream022-steamvr-hud-ready-v1. hud_source_deferred counts
preparecycles with no completed canvas selected (may span multiple channels).
hud_ready_selected counts actual new completed publications. hud_source_age_ms
measures the published content's age at copying, using existing tick timestamps;
it is not motion-to-photon or precisely GPU time. Existing overlay_source_ready
should now be ready for HUD copies; F10 may still record pending source waits.

Production HUD fixture covers pending first appearance, no new XR acquisition
or GPUqueuewait during deferral, continued head pose, retained source completion
under newer pending publication, consumer pin retirement, resize, immediate
empty suppression/no resurrection, stale expiry. Existing timeout/shared path
and queue/generator tests remain. XR is mocked, GPU synchronization is real.

## Headset test

Repeat the last sequence: SteamVR/PSVR2, nativeNVIDIAFG+adaptive pacing+recorderON,
externalOFXRoff; NRon throughout, RT off thenon thenoff, RRoff. Natural movement
and combat. Verify HUD/minimap/interaction panels update and follow head normally.
Judge comparator delivery/zero-present rate, sourceage, GPUqueue delay, consumer
pending and HUD content age. Preserve quality/resolution settings.

## Rejected after headset test; rolled back

November6 19:23: RT-on compositor estimate49.61 FPS versus previous72.56;
RT-off87.67/89.25. Natural scenes differ, so causal magnitude is unproven.
Removing HUD waits did not improve delivery and added HUD content latency.
Installed DLL and five source/test files restored to overlay-queue-v1; settings
unchanged. See ../../flight-run-20261106-191857/analysis.md and
../../hud-ready-rollback.json. Preserve rejected artifacts; build-local output
is still the rejected candidate until rebuilt. The preceding test instructions
are historical, not a request for another run of this candidate.
