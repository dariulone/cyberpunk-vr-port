# Quest Pro mirror facial animation

The user confirmed visible facial animation in V's mirror works on 2026-11-14.
This validates their current rig/setup; the recovery and compatibility checks
below remain useful and are not implied by that confirmation.

This initial, opt-in implementation drives V's interactive mirror double from
Steam Link's ALT OSC stream. The normal game and NPC facial animations remain
outside its scope. See [the quick reference](../README_CYBRFNGR.md#quest-pro-facial-animation-experimental)
for player instructions.

## Inputs and controls

The receiver binds exclusively to `127.0.0.1:9015` by default. It consumes named
float channels under `/sl/xrfb/facew/` and upper/lower confidence under
`/sl/xrfb/facec/`, following the
[Steam Link protocol reference](https://github.com/danwillm/VRCFT-SteamLink#about-osc-data-from-steamlink).
Independent receive timestamps are essential: Steam Link sends different face
regions in separate bundles. One region cannot refresh another's stale values.

The first mapping covers 19 named controls: two smile corners, jaw opening, two
eyelid closures, eight eye-direction weights and six eyebrow weights. Each
feature has its own checkbox. Eyebrows default to off; the entire adapter is off
until explicitly enabled. Upper/lower confidence must be at least 0.5. Fresh
values own their mapped tracks for 250 ms, then fade into each newly evaluated
native expression over 200 ms. Missing confidence, unknown tracks and disabled
features leave the native result alone. There is no saved face pose to freeze.

OSC bundles have bounded sizes/depth and publish atomically. Nonfinite and
out-of-range coefficients are rejected. A malformed packet does not refresh
the previous sample. Each game update receives at most 128 packets, without
blocking. Disabling the feature or unloading closes the socket. A competing
listener is reported rather than silently sharing/stealing packets.

## Native animation path

The read-only mirror inspection found distinct root animation objects, pose
buffers and track layouts for V and the double. For example, the observed
player/mirror smile-left indices were 68/58 and blink-left 44/34. These values
are evidence of different layouts, **not constants used by the adapter**.

`FaceRig.cpp` uses the entity provided by the existing `VRMirrorDouble` native,
validates its root object and non-player destination tracks, and holds a weak
entity reference. Repeated mirror updates refresh a 750 ms binding lifetime;
closing the mirror invalidates its generation. Nested graph evaluations carry
thread-local ownership only while evaluating this specific destination. The
solver acquires a strong reference before using the binding. Shared rig
resources alone never identify the character.

The adapter intercepts the native Sermo solver before its facial tracks become
bones. It resolves CName hashes from the current solver rig and validates the
facial setup's track ranges. Controlled tracks and their matching lip-sync
contributions are adjusted in a private, aligned input copy, then passed to the
original solver. That preserves the game's corrective/inbetween machinery.
The existing mirror body-copy pass preserves facial solver bones by name while
this binding is active; head, neck and root continue using body tracking.

This is a version-specific engine integration. Currently supported executable:

```text
GOG Cyberpunk 2077 2.5
SHA-256 d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4
PE timestamp 0x6ad55daf; image size 0x581b000
Graph evaluator RVA 0x1b25dc; native Sermo solver RVA 0x693728
```

Installation of the two hooks requires the matching PE timestamp/image size and
both 24-byte prologues. Mismatches leave the adapter unavailable. The ABI was
checked against this executable's graph-to-pose mapper, Sermo sample method,
pose constructor and solver input accesses; it is not a public engine API.
The implementation also requires facial setup version 8 and bounds-checks the
track/bone counts. Other game builds or custom head rigs need verification.

The public [WolvenKit Blender add-on](https://github.com/WolvenKit/Cyberpunk-Blender-add-on)
provided independent references for named facial tracks and lip-sync suffixes;
the game executable and bundled RED4ext SDK supplied the native ABI. No external
Python package or Blender component is needed at runtime.

## Validation and next live check

```powershell
cmake -S tools/face_tests -B build/cyberfinger-face-tests -A x64
cmake --build build/cyberfinger-face-tests --config Release
ctest --test-dir build/cyberfinger-face-tests -C Release --output-on-failure
```

Three suites cover malformed/split OSC bundles and freshness; rig-specific
mapping, eye direction, feature isolation and native fallback; and the actual
Windows UDP receiver, exclusive bind conflicts, disable/re-enable and shutdown.
They do not prove the native hook executes or the reflection visibly animates.

After installing and restarting, enable the feature and open an apartment
mirror. Read counters without attaching a debugger or competing for OSC:

```powershell
python -B tools/face_tests/live_probe.py --pid (Get-Process Cyberpunk2077).Id
```

The versioned exported block `CyberpunkVR_FaceTracking` reports data freshness,
binding state, mapped controls, graph/solver passes, successful input overrides,
rejected layouts and guarded read faults. The probe prints counters only, not
facial recordings. A rising write counter proves the solver received modified
input; the visible effect still requires a headset check.

Verify smile, independent blinks, jaw and eye directions; turn each feature off
and verify native animation returns. Remove the headset/stop OSC to test loss
of tracking. Close/reopen the mirror and reload a save; check that another NPC
and story dialogue retain their animation. Both V body types/custom heads remain
an explicit compatibility test. Leave the feature disabled if the reflection
looks wrong or no writes occur, and inspect the counters before changing ABI
assumptions.
