# Stereo DLSS resource lifetime and idle-context reclamation

Native SBS uses two eye viewports after the engine has already evaluated its original full-width DLSS SR viewport. This change releases only the observed original SR feature (feature 0, viewport 0) after two successful stereo evaluations, two seconds of inactivity, and retirement of every tracked command recording/submission. Active eye contexts and Ray Reconstruction (feature 1001) are excluded.

Command resources retain markers until reset/destruction and GPU completion, including replay and multiple queues. Missing lifecycle/options hooks, uncertain viewport identity, tracking failures or a failed free disable reclamation. Calls are serialized with evaluate/options/lifecycle calls. If viewport 0 resumes, the recorded engine options are reapplied before Streamline's lazy evaluate recreation.

Right-eye staging resources now use the same fence-backed ownership instead of an eight-entry retirement array. Replacing the cache drops its reference, while recorded/in-flight references stay alive; a full retirement array no longer keeps replaced resources indefinitely.

Native DLSS-G can supply both scaling output (tag 4) and HUD-less color (tag 2). The output-copy array now has capacity for every tag, avoiding the previous `after[1]` stack overwrite. This is included because it affects the same copy/resource path and was required for the tested native-DLSS-G configuration.

## Evidence and limits

In the October 30 local tests, `slFreeResources` returned success with zero pending markers and two-eye rendering continued. Approximately **825.1875 MiB of previously identified original-context allocations** disappeared from the live allocation inventory. This is VRAM headroom relevant to demanding RT/RR setups, not an 825 MiB reduction in RT-specific allocations or a proven FPS gain. Total-VRAM peaks from runs with different RT/RR settings are not a valid before/after comparison.

Menu/reload and RT/RR gameplay tests survived, but the original viewport did not resume in those captures. Actual driver recreation after release therefore remains an unexercised live path; unit tests cover the gating/state transitions. Native DLSS-G was retested successfully with both output tags after the overflow fix.

No rendering-resolution, RT/RR-quality, FOV, pacing or overlay changes are included. Temporary allocation/descriptor tracing is not included.

## Verification

`tools/command_resources_tests` tests command-list recording/reset, replay, multiple queues, many staging replacements, pending-work rejection, idle thresholds, renewed original-context use and failure handling. Build with CMake and run CTest on Windows/D3D12. The PR is extracted from the tested 0.1.9 PSVR2 RC2 release; standalone upstream build validation is recorded in the PR.
