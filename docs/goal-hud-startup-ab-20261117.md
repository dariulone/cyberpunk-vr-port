# HUD separate-window startup A/B

Diagnostic experiment, not a memory fix. Only `CopyEntries`' added whitelist
override is gated; original copying always runs, authored flags are preserved in
mode0, and existing HUD entries are never rewritten. Default mode1 preserves the
current behavior. No native free, resource reset or GPU allocation is added.

Exports:

- `CyberpunkVR_HudSeparateWindows`: atomic uint32, default1. The environment
  `CVR_HUD_SEPARATE_WINDOWS=0|1` is read once before the first CopyEntries call.
- `CyberpunkVR_HudSeparateWindowsCounts`: five atomic uint64 values: calls,
  returned entries, whitelist matches, false→true changes, bypassed matches.

The current PID26944 log had only ten enabled-entry lines across all map/teleport
tests. A mode toggle after HUD startup is not a valid test of the spawn policy.
Counters are independent atomics; collect them after gameplay startup is complete.

## Root-controlled launch

Use the same deployed DLL/PDB, OpenXR Simulator, AutoSave-13, resolution, graphics,
FG mode, probe/overlay layers and test route for both modes. Keep the save guard
through all mutation/cleanup steps. Do not combine this A/B with a runtime change.
Close the existing game and release any replay before the root launches a new one.

The existing QuickBoot launcher copies the caller's environment into its direct
game `subprocess.Popen(..., env=environment)`. Run the environment assignment and
that launch in **one** PowerShell invocation, restoring the caller afterward:

```powershell
$auditOldGate = [Environment]::GetEnvironmentVariable('CVR_HUD_SEPARATE_WINDOWS', 'Process')
try {
    $env:CVR_HUD_SEPARATE_WINDOWS = '0'
    python tools/quickboot_tests/launch_to_save.py --save AutoSave-13 --no-focus --simulator 'C:\Users\dariulone\Downloads\OpenXR-Simulator-v1.5.0-win64\openxr_simulator.dll' --timeout 300
    if ($LASTEXITCODE -ne 0) { throw 'Root launch failed' }
} finally {
    [Environment]::SetEnvironmentVariable('CVR_HUD_SEPARATE_WINDOWS', $auditOldGate, 'Process')
}
```

This is a recipe for the root's authorized launch workflow, not an instruction
for another agent to start a second process. Preserve any additional arguments
used for the matching baseline. The diagnostic does not set machine-wide env.

## Exact counter and two-cycle commands

After the root verifies its unique retained GoalSaveGuard and gameplay readiness,
substitute the actual new PID and a unique output folder. `hud_gate.py` is strictly
read-only and resolves exports from the actual mapped PE, without symbol guesses.
It fails if a prior mode1 startup was merely flipped to0 at runtime.

```powershell
$auditGamePid = <new-PID>
$auditOut = 'C:\Users\dariulone\Desktop\CyberpunkVRPort\scratch\goal-audit-20261117\hud-separate-off-<new-PID>'
python tools/resource_census/hud_gate.py --pid $auditGamePid --expect 0 --out (Join-Path $auditOut 'gate-before.json')
if ($LASTEXITCODE -ne 0) { throw 'Startup gate did not apply; do not label this run mode0' }
python scratch/goal-audit-20261117/map-census-deep.py --pid $auditGamePid --out $auditOut
if ($LASTEXITCODE -ne 0) { throw 'Map probe incomplete; retain root save guard and inspect cleanup' }
python tools/resource_census/hud_gate.py --pid $auditGamePid --expect 0 --out (Join-Path $auditOut 'gate-after.json')
if ($LASTEXITCODE -ne 0) { throw 'Gate evidence changed during map probe' }
python tools/resource_census/diff_reports.py (Join-Path $auditOut 'before.json') (Join-Path $auditOut 'after-2.json') --out (Join-Path $auditOut 'delta-two.json')
python tools/resource_census/native_textures.py --pid $auditGamePid --hud --layers --out (Join-Path $auditOut 'native-owners.json')
```

The root's existing `map-census-deep.py` enables mode2 census before baseline,
takes after-1/after-2 and real eye images, closes native menus through OnBack,
then disables new observations in `finally`. No capture replay is opened.

First interpret the verified mode0 arm. If target growth persists with
forcedChanges=0 and bypassed=matches, the added override is not a necessary cause
under these conditions; a further mode1 launch is not needed for that exclusion.
This does not measure the override's full memory cost or identify the true owner.

If growth disappears in mode0, a matching mode1 arm is mandatory before claiming
causality: start a fresh otherwise identical session on the same DLL with env1,
use `--expect 1` and a different output directory. The diagnostic must show
calls/returned entries/matches >0, mode0 forcedChanges=0 and bypassed=matches,
or mode1 forcedChanges>0 and bypassed=0. If these conditions are absent, the arm
does not isolate the override. A baseline captured before this diagnostic exists
can provide supporting evidence but is not an equally instrumented arm.

Interpretation: compare net live target allocations by shape/format/flags and
stack within each PID, with zero observer loss/overflow and coherent reports.
Group IDs are process-local; do not diff reports from different PIDs directly.
Account for deferred warm-up separately (the earlier PID26944 interval had an
extra pair from a prior screenshot-failed cycle). Unchanged growth in mode0 would
exclude this override as a necessary cause under these conditions. A confirmed
two-arm difference would justify tracing the affected entry names/lifetimes,
not arbitrary reference drops.

## Mode0 result, PID4744

Root deployed the gate and ran the otherwise matching OpenXR/AutoSave-13 arm.
Gate-before/after snapshots showed copyCalls1, returnedEntries72, whitelistMatches10,
forcedChanges0, bypassedMatches10, stable. `hud0-4744/warm-delta-two.json` records
two additional1024² targets(+8,388,608live bytes) and two768² targets(+4,718,592),
zero releases in both target groups. Their combined growth is13,107,200bytes,
again6.25MiB per warm map cycle. Total committed delta was12,517,376bytes because
other resources were released; do not confuse that total with the pair subtotal.

The added forced-separate-window override is therefore **not a necessary cause
of this pair's growth** under the tested conditions. This does not exclude the
whole HUD module, authored separate-window behavior, other port influences, or
native ownership paths. A matching mode1 launch is not required for this
necessary-cause exclusion. No leak cleanup patch follows from this experiment.
