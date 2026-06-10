---
name: cp2077-native-simultaneous-exhausted
description: "HONEST CONSOLIDATION (2026-06-09) — every native simultaneous-stereo lever is decisively walled; the two ways to get a registered 2nd view-slot (engine producer-double, deep-clone) are both dead (mgr-tear / arena-slot). Stop re-treading; decide between deep-grind vs Sync-Sequential fallback."
metadata: 
  node_type: memory
  type: project
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

User pushback (2026-06-09, correct): "haven't we already tried the inventory/fan-out path?" — YES. Consolidating so we stop circling. The FG-DUMP of the inventory 2nd view only re-confirms [[cp2077-view-item-layout]] (already had: view-item vtable 0x2AC8688, dims 3072, sub-objs@+0x40).

**Complete map of native-simultaneous levers — ALL decisively dead:**
1. Inline replay (node/driver/view/FG-build re-invoke) — dead all 4 levels ([[cp2077-inline-replay-dead-all-levels]]).
2. Producer-double (call sub_140293978 twice) — tears shared mgr+0x2B8 → black screen + async null-crash. DEAD ([[cp2077-producer-double-mutates-mgr]]).
3. Shallow view-item clone — 2nd build RUNS but corrupts shared mutable sub-object scratch ([[cp2077-eye2-clone-shared-subobj]]).
4. Deep clone — hits the HARD wall: global per-frame arena slot is assigned by engine VIEW REGISTRATION (stride 0x5E00), NOT carried in the handle, so any clone reuses eye-1's slot → collision; cloning cannot fix it ([[cp2077-clone-crash-global-arena-slot]]).
5. Fan-out loop hook (sub_140167EB4) — unwind-sensitive, base-5 crash on inventory ([[cp2077-view-item-layout]]).
6. View-registry direct injection — entries are value-inline in a hashmap (refs=0), needs engine hash+insert, no static insert site ([[cp2077-registry-injection-feasibility]], [[cp2077-view-registry-producer-generic]]).
7. RTT-cam attach/handle (last ~10 sessions) — render-thread virtual + one-time; AND run #5 (2026-06-09) proved live that the per-frame inventory 2nd view fires NEITHER the RTT proxy-attach (sub_140912294, 0×) NOR the RTT component-update (sub_1423E759C, 0×) across ~5400 frames of builds/frame=2 — so the RTT-cam is not even the per-frame 2nd-view vehicle ([[cp2077-proxy-attach-probe]]).

**Root convergence:** a REGISTERED distinct 2nd view-slot can only come from the engine's own view-insert/producer, which is architected around ONE main view; the two ways to drive it (producer-double, manual registration) tear shared per-frame state or are virtual/unidentified. Confirmed by [[redengine-job-render-arch]] (one main camera/view per frame at every accessible level).

**HONEST VERDICT:** native genuine-simultaneous stereo in REDengine 4 is, across 35+ sessions, exhausted at every explored lever — and the levers are now comprehensive. No cheap untried native lever remains.

**Decision surfaced to user (the real fork):**
- (A) **Synchronized Sequential** — the working path that SIDESTEPS the wall: don't make a 2nd registered view; render the SINGLE view twice with sim/time FROZEN between eye-0 and eye-1, submit as a synced OpenXR pair. Delivers the user's actual experiential goal (both eyes one consistent sim tick → NO ghosting; NOT AER, NOT reprojection). Cost: effective sim/render rate halves. Open piece = gate the per-frame sim/time advance (tractable, unlike 2nd-view registration). Per [[redengine-job-render-arch]].
- (B) **Deep grind** — attack the arena-slot wall directly: manually assign a free arena slot (≈224 capacity, plenty free) to a deep-cloned view so it doesn't collide. Low probability (slot-id may not be a settable field), high effort, crash-whack-a-mole — but "make it work / maintainability irrelevant" permits trying.
- (C) **AER V2** (optical-flow frame-gen) — the other working path, infra half-built ([[aer-render-pose-submit]]); reduces the ghosting the user disliked in legacy AER.
