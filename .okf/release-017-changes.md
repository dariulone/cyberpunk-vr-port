---
type: Configuration
title: Upstream 0.1.7 PSVR2 release changes
description: Scope and validation status for isolated haptics and empty-overlay pacing.
tags: [psvr2, release, haptics, pacing]
timestamp: 2026-10-02T00:00:00+09:00
---

# Scope

The upstream 0.1.7-based port has an explicit FOV override and PSVR2 input
mapping on this branch already. The follow-up haptics change publishes swing
and hit-check pulses on `Local\CyberpunkVR_PSVR2_Haptics_017_v1`; it does not
reuse upstream input slots 157–160. The external PSVR2Toolkit v0.3.1 release
owns the matching bridge, Sense driver and launcher. The port ZIP does not
install the driver or embed the bridge.

The F10 STEREO > PERFORMANCE checkbox retains an empty overlay submission and
fence when checked, even when the weapon dot is absent. This is the tested
default pacing behavior for the user's external OFXR bike case. Unchecking
skips that work for a session; the prior ride/dismount slowdown may return.
It is not a frame-generation toggle or a guaranteed FPS improvement.

# Validation boundary

MSVC Release plugin build and the five grip-only Lua tests passed. The user
previously confirmed PSVR2 trigger feel and external-OFXR bike stability in
headset runs, but the final packaging and this F10 toggle have not received a
new combined headset test. Experimental GPU/HUD diagnostics remain outside
the release commit.
