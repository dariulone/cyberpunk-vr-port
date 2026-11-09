# SteamVR real-endpoint recovery, November 6, 2026

Candidate real-recovery-v1 builds on the restored overlay-queue-v1. The rejected
hud-ready-v1 policy is absent. Scope fixes and CP2.5 SDK remain unchanged.

## Defect and correction

SubmitSchedule::Observe previously handled GPU readiness before checking frame
kind. A pending synthetic/repeat reset readyRecovery=2 between two completed
real endpoints. Thus the documented two-ready-real-endpoint recovery could
never finish under alternating real-ready/synthetic-pending submissions.

Return for non-real frame kinds before changing recovery state. They neither
satisfy nor reset recovery. Pending real endpoints still restart the guard;
two valid ready real endpoints are still required. Invalid compositor feedback
does not satisfy recovery. Pending real frames still produce zero correction,
including no whole-period phase jumps. Existing correction bounds, cooldown,
absolute cadence, physical refresh handling and unknown/shared-queue fallback
are unchanged. No GPU wait, resource allocation, or rendering setting added.
VDXR bypass remains; this is not a PSVR2-specific device identifier check.

## Evidence and limits

The 19:18 HUD-ready run had 3083 GPU-pending phase samples,55 recovery samples,
and no timing corrections during the selected RT-on interval. This identified
the lockout behavior but cannot establish its share of the performance loss:
real frames were often pending too. This patch only fixes recovery when real
endpoints become ready. It does not make sustained GPU overload disappear.

A deterministic test with an intervening pending synthetic frame fails before
the correction at 'two healthy real endpoints restore acquisition' and passes
after it. Tests also exercise pending-real reset, sustained overload, invalid
feedback, generated/repeat non-steering, and 72/90/120Hz schedules. The production
SteamVrPacer test feeds real-ready / synthetic-pending / repeat-pending /
real-ready and checks recovery actually resumes in the caller, with VDXR bypass.

The flight recorder header is upstream022-steamvr-real-recovery-v1. Existing
phase_gpu_pending counts ALL pending kinds, not just ones arming recovery.
phase_gpu_recovery counts ready real samples still in recovery. Compare phase
corrections, real/synthetic readiness, compositor delivery and zero presents;
accepted-real+generated submission rate is not experienced FPS.

## Deployment and validation

See real-recovery-provenance.json and real-recovery-deployment.json at tree root
for exact build/tests/hash and deployment status. DLL-only installation backs
up the previous DLL and preserves INI/UserSettings. Prior installed baseline:
9acf0e7b79d368f32a55517ce1915b8bc0385ca11990cc021172d6519d7a617d.
Restored HUD/recorder headers are explicitly timestamp-invalidated before the
final MSBuild build to avoid reuse of rejected hud-ready objects after copy2.

Headset benefit remains unverified until tested. Use SteamVR, native NVIDIA FG,
adaptive pacing ON, NR ON, RR OFF; start RT OFF, enable RT during natural play,
then disable RT. No stationary/scripted movement required. Compare to previous
overlay-queue-v1 as well as rejected HUD candidate; record scene differences.
The VDXR RR crash investigation remains deferred, with evidence preserved.
