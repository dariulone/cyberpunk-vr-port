# Path A attach root cause — final RE synthesis

Date: 2026-06-24. Rounds: path_a_attach_audit.py + path_a_round2.py + 5 NativeDB cross-checks.

## Bottom line

`entRenderToTextureCameraComponent` is **data-only**. Render-side activation
is done by a separate **render-proxy object** that the engine instantiates
when the entity is fully attached AND placed into an active streaming sector
AND the component's `params` (WorldRenderAreaSettings @ 0x290) is initialised.

Codeware `gameStaticEntitySystem:AttachEntity(id)` returns **false** because
the ad-hoc-spawned entity does not satisfy these preconditions — no sector
context, and `params` is zero-initialised.

This is **not a Codeware bug**. It is an engine-level invariant.

## The 4 evidence pillars

### 1. Component is a passive data holder

Both registrars (`sub_1416F6D88` RTT, `sub_141628964` Holocall) do nothing
more than `sub_1403CE194(&class, "name", size=0xA10)` + set vtable + 
`sub_1403CE500(register)`. No event subscriptions, no OnAttach hooks, no
lifecycle callbacks. The class is a pure POD descriptor.

### 2. Render activation is gated on 2 component fields

`sub_14049F21C` (the activation orchestrator) checks:
- `*(byte*)(comp+0x290)` — first byte of `params` (WorldRenderAreaSettings).
  If zero → fail-path `sub_1402EE33C` returns without rendering.
- `*(byte*)(comp+0x240)` — first byte of `particlesDynamicTextureRes` rRef.
  If != 2 → also fail-path.

`sub_1404A04AC` (the actual activation) additionally checks `comp+0x1FB`
(inside depthDynamicTextureRes rRef) — must be zero.

A fresh-spawn entity has `params` zero-initialised → gate closes silently.

### 3. The orchestrator is in a *render-proxy* vtable, not in the component's

`sub_14049F21C` lives in vtable region `0x142B666xx`:
```
0x142B666A0  sub_1407F7850
0x142B666A8  sub_1421C0584
0x142B666B0  sub_1408FCD9C
0x142B666B8  sub_14049F21C  <- the activation orchestrator
0x142B666C0  sub_140D368C0
```

The 5 component vtables we found earlier (RTT `0x142DE2C38`, Holocall
`0x142D498F8`, etc.) do NOT contain `sub_14049F21C` in slots 0..0x180.
First 48 slots of RTT and Holocall vtables are identical (RTTI + serialise
+ alloc helpers) — confirming the render half lives elsewhere.

### 4. The proxy is the multi-inheritance render-side twin of the component

`sub_140AC316C` (the only production caller of `sub_1404FBAFC` VIEW-CREATE)
operates on `a1` and `a1 - 288` interchangeably:
```c
sub_140AC316C(a1) {
  sub_140128690();
  if (_InterlockedCompareExchange(a1+2272, 0, 1)) {
    sub_140AC2BA4(a1 - 288, a1 + 384, 1);   // setup on container
  }
  sub_140AC31C4(a1 - 288);                   // pre-create
  return sub_1404FBAFC(a1 - 288);            // view-create
}
```

Negative-offset access (`a1 - 288`) is the C++ multiple-inheritance pattern:
`a1` points into a sub-object that lives at +288 inside the bigger container.
The bigger container is the render proxy, the inner sub-object at +288 is
the part that holds the vtable our method was reached through.

Engine builds the proxy when component+entity meet activation criteria.
Codeware never gets the chance to surface it.

## What this kills

1. **Writing `comp.isEnabled = true` from Lua is necessary but insufficient.**
   Component-level isEnabled gates the proxy's "should I activate this frame?"
   check; it does NOT cause the proxy to be created in the first place.

2. **Snapshot/restore + dual-call shortcuts are inapplicable here.** Those
   were Path-B techniques. Path A needs the engine to construct the proxy.

3. **NativeDB's `entIComponent.Toggle(on)` is just the polymorphic isEnabled
   setter** — same outcome as direct field write.

## What this opens — 3 viable v4 directions

### D1 — Initialise `params` and `particles` rRef before AttachEntity

If the gate is purely 2 byte-checks (params first byte != 0, particles
first byte == 2), we can satisfy them from Lua BEFORE AttachEntity. The
fields are reflection-accessible per NativeDB:

- `params: WorldRenderAreaSettings` @ 0x290 — set first byte non-zero via
  a default `WorldRenderAreaSettings.new()` write.
- `particlesDynamicTextureRes: rRef:DynamicTexture` @ 0x240 — bind to the
  same DynamicTexture as `dynamicTextureRes` (engine doesn't need separate
  particles RT, just a valid rRef).

**Risk**: gate may be necessary-but-not-sufficient; engine may still refuse
attach due to streaming/sector context.

### D2 — Spawn through `HolocallSystem` (production-proven)

Cyberpunk's holocall feature creates a fully-attached `holocall_camera.ent`
in every cinematic phone call. The system is reachable via
`Game.GetSystem("gameuiHolocallSystem")` (or whatever name CET binding
exposes). If we can trigger a fake start-call with our own camera config,
we inherit the engine's full proven attach path.

**Approach**: Lua probe HolocallSystem methods, find `StartCall` /
`ActivateCamera` / equivalent. Even if the public API requires a NPC
target, we can pass any valid puppet handle.

**Risk**: HolocallSystem may have strong preconditions (player not in
combat, no other call active, etc).

### D3 — Manually construct render proxy via red4ext

The proxy is a render-side C++ object. Address of its constructor is not
yet known, but `sub_140AC316C` (proxy method) lives at known address and
its `a1 - 288` trick tells us the proxy size is at least 288 bytes + the
sub-object size. With red4ext we could:
1. Allocate proxy-sized memory.
2. Write vtable `0x142B666xx`.
3. Copy field values from our Lua-side component.
4. Register proxy into engine render world directly.

**Risk**: highest. The proxy has internal refcount/state we'd be skipping.
Worst-case: crashes when engine ticks through render world.

## Recommendation for v4

**Try D1 first** — pure Lua, no native code change, fastest to test. If
D1 works (probe shows `attached=true`, RT-diag sees new allocations), we
have a clean Path A win. If D1 doesn't work, move to **D2** (HolocallSystem
probe). Save **D3** as last resort — it requires another round of RE just
to find the proxy constructor.

## v4 spawner.lua changes (minimal scope)

Before `sys:AttachEntity(id)`:
1. Find RTT component via existing `GetComponents` chain.
2. Read RTT class definition (already done in v3 — `rttFields`).
3. For each `rRef:DynamicTexture` field (`depthDynamicTextureRes`,
   `albedoDynamicTextureRes`, etc.), bind to a placeholder DynamicTexture
   resource that the engine's resource manager already knows about.
4. Construct `WorldRenderAreaSettings.new()` and assign to `params`.
5. Construct `entRenderToTextureFeatures.new()` and assign to `features`
   (so the feature gate is initialised to defaults rather than zero).
6. Call `sys:AttachEntity(id)` again and log the new return value.

Then probe `sys:IsAttached(id)` over 600 ticks; expected to flip to true
within a few seconds once gates are satisfied.

If still `attached=false` — proceed to D2.
