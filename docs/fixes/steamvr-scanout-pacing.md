# Use physical SteamVR refresh for native framegen pacing

The November6 PSVR2 recording showed a90Hz headset with runtime
predictedDisplayPeriod switching between11.111,22.222 and33.333ms. Native
producer pacing doubled those intervals, generating targets up to66.666ms.
That is a throttled application cadence being mistaken for physical scanout.

This candidate reads SteamVR's Prop_DisplayFrequency_Float through the existing
V557 reader. A separate worker runs regardless of the flight-recorder checkbox,
only for an identified SteamVR runtime. It waits3seconds and90 successful layered
XR submissions before allowing a Background connection, following the existing
reader's lifecycle constraints. Polling is once per second; the reader caches
hardware-property reads for2seconds. The render/present path only reads atomics.

A valid observation replaces the period used by native FG's adaptive base and
producer pacing. The correction applies to all SteamVR headsets (including Quest
via Steam Link), per the user's clarified scope. If the reported period already
matches scanout, the pacing interval is unchanged. VDXR does not start the worker
and retains its raw-runtime path. The FG-off frame cap also retains its prior
behavior. Original xrWaitFrame outputs, predictedDisplayTime, XR submission
protocol, interpolation and pair-acceptance rules are untouched.

Missing, invalid or older-than6seconds observations fall back to the original
runtime interval. A failed refresh query invalidates the observation. Refresh
changes in either direction update the base and reset stale adaptive/deadline
state; the implementation does not pin a lifetime minimum or hard-code90Hz.
Stopping a session joins the worker and clears its observation before XR teardown.
The existing connection release remains at instance destruction.

Recorder fields added: scanout_correction_steamvr, physical_period_ms (null when
unavailable), effective_fg_period_ms, period_source. Existing period_ms remains
the raw runtime report, and pacing_budget_ms measures the actual adaptive target.
Thus a90Hz run can show raw22.222/33.333 while effective stays11.111 and the
normal FG base stays22.222. Adaptive backoff can still increase that base after
missed pairs; this patch does not hide or remove that independent mechanism.

Validation: Windows Release build, refresh-selection tests (90/120/72Hz,
throttled multiples, unthrottled identity, VDXR exclusion, missing/stale/invalid
readings), adaptive-base fixture, and worker lifecycle/read-failure/refresh-change
fixture with a fake OpenVR reader. Existing recorder and delivery-estimator tests
passed. These tests establish behavior, not in-game FPS recovery. Next gameplay
recording must measure whether pair losses/repeats and compositor delivery improve.

Source baseline/rollback: recorder DLL9fd000b4df7b410ce672b8503b0e12796cc0bff6e162f93503c83c53650f4787.
Source snapshot: scanout-before/. Build/receipt: scanout-provenance.json and
scanout-deployment.json. No INI or game graphics changes are required.
