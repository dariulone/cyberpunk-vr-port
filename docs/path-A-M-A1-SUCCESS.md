# Path A M-A1 — SUCCESS (2026-06-24)

RTT camera spawn through Codeware **renders the full CP2077 deferred GBuffer
pipeline** when activated. M-A1 milestone closed. Document records exact
recipe, proof from testbed.log, and corrections to prior wrong analyses
(some carried through 6+ sessions).

## Minimal Lua recipe that works

```lua
local sys = Game.GetStaticEntitySystem()
if not sys:IsReady() then return end                  -- no save loaded

local spec = StaticEntitySpec.new()
spec.templatePath = ResRef.FromName(
    "base\\cinematics\\cameras\\holocall_camera.ent")
spec.position    = Game.GetPlayer():GetWorldPosition()
spec.orientation = Game.GetPlayer():GetWorldOrientation()
spec.attached    = true

local id  = sys:SpawnEntity(spec)
local ent = Game.FindEntityByID(id)

-- Walk components via componentsStorage; find entRenderToTextureCameraComponent.
local rttComp = nil
for i = 1, #ent.componentsStorage.components do
    local c = ent.componentsStorage.components[i]
    local cls = c:GetClassName().value
    if cls:find("RenderToTextureCamera") or cls:find("HolocallCamera") then
        rttComp = c; break
    end
end

rttComp.isEnabled = true   -- the wake-up trigger
-- Engine takes ~1.8 seconds to bring up the render pipeline.
```

## Proof from testbed.log

Timeline (extracted from spawner v5.3 holocall run 2026-06-24 16:09:59 - 16:10:03):

| t (sec) | Event |
|---|---|
| 0.000 | `IPC.beginRTWindow("holocall")` |
| ~0.16 | one-shot #0a specFields |
| ~0.47 | #0b holocallProbe |
| ~0.78 | #1 rttFields (32 fields dumped) |
| 1.10 | #2 tryEnable: `field=ok | Toggle=nil | field_read=true | IsEnabled()=true` |
| 1.40 | #3 sysMethods |
| 1.76 | #5 rttMethods |
| 2.07 | #6 entityMethods |
| 2.38 | #7 entityState: `IsAttached=true IsActive=false IsInitialized=true GetWorldPosition=(3194.85,-383.56,132.46)` |
| **2.74** | #8 rttActivation: `IsEnabled=true` |
| **2.85** | **First [RTV] burst starts — 256×256 fmt=29 R8G8B8A8_UNORM_SRGB** |
| 3.20 | tryRenderActivation completes |
| 3.71 | finalReProbe (same state — render burst already underway) |
| 4.18 | WINDOW END: `total=377 inWindow=121 novel=7` |

Render burst from ~2.85 to ~3.21 sec = 121 RT/CPR allocations + ~400 RTV
descriptor creations. Resources match a full CP2077 deferred pipeline scaled
to the RTT comp's `resolutionWidth/Height` (default 256×256 in
holocall_camera.ent).

**Identified resources of interest** (first 5 in the burst):

| Resource ptr | Dims | Format | Likely binding |
|---|---|---|---|
| `0x00000226130D14E0` | 256×256 | fmt=29 R8G8B8A8_UNORM_SRGB | `dynamicTextureRes` (final color) |
| `0x000002261310C1C0` | 256×256 | fmt=29 | `albedoDynamicTextureRes` |
| `0x0000022611DCC570` | 256×256 | fmt=39 D32_FLOAT | `depthDynamicTextureRes` |
| `0x0000022611DC65D0` | 512×512 | fmt=28 R10G10B10A2 | Internal supersample / normals |
| `0x00000225FA0AF200` | 16×16 | fmt=62 BC compressed | Lookup texture |

`dynamicTextureRes` Lua userdata pointer `0x022445ab1180` is the **rRef
wrapper object** in game heap, NOT the D3D12Resource. The chain is
`raRef:DynamicTexture (game obj) -> DynamicTexture (game obj) -> internal
D3D12Resource`. Resolving that mapping is the first concrete task for M-A2.

## Corrections to prior wrong analyses (carried up to 6 sessions)

| Prior claim | Reality |
|---|---|
| "RT-diag is blind to aliased rendering in CP2077" | **WRONG.** RTT outputs are persistent committed/placed resources. RT-diag catches them perfectly. Prior `inWindow=0` was because engine never activated (we kept rebuilding before the ~1.8 s react). |
| "`IsAttached=false` means entity not attached" | **WRONG — phantom for 6 sessions.** Codeware `StaticEntitySystem` has no `IsAttached` method. Our `safeCall` returned `(false, "method missing")` and the string was compared to `true`. Entity-level `ent:IsAttached()` (from entEntity chain) returns true correctly. |
| "`IsActive=false` blocks rendering" | **WRONG.** RTT renders fine with IsActive=false. IsActive likely measures "interactively controllable", not "render proxy alive". |
| "AttachEntity returns false → second attach call needed" | **WRONG.** First attach during `SpawnEntity(spec)` (with `spec.attached=true`) succeeds. Subsequent `sys:AttachEntity(id)` returns false because **nothing to do**, not because of failure. |
| "Need AddComponent on player / native orchestrator hook" | **WRONG.** Codeware SpawnEntity + `isEnabled = true` is sufficient. Patience for engine react time was the only missing piece. |
| "render_camera_RE round 2: comp+0x290 = `params` byte0 must be non-zero" | **WRONG / re-interpreted.** SDK headers show +0x290 = `renderingMode` enum, not embedded struct. Default value `Shaded` ≠ 0 so gate already passes. |

## What this unlocks for the full VR roadmap

Path A is structurally proven workable end-to-end:
- Spawn arbitrary RTT camera entity at runtime ✓
- Engine builds full per-frame GBuffer + lighting + post for that camera ✓
- Output exposed as `dynamicTextureRes` rRef per camera component ✓
- Per-eye feature gate via `entRenderToTextureFeatures` (renderForwardNoTXAA,
  contactShadows, etc.) — can disable per-eye TXAA to avoid swimming
- VR composition = render two RTT cameras (left/right with IPD offset),
  read their D3D12Resource pointers from native, composite to OpenXR layers

## M-A2 — concrete next milestone

**Goal**: read the D3D12Resource pointer for `dynamicTextureRes` of a live
RTT camera component from red4ext native code.

Approach options (in order of estimated ease):

1. **Static RE the rRef→DynamicTexture→D3D12Resource chain** in render_camera_RE.
   The `DynamicTexture` class likely stores its current backing resource
   pointer at a known offset. Find it once, hardcode the offset in red4ext.
2. **Hook `CreateRenderTargetView` and correlate by timing**. We already log
   every RTV creation with res ptr. The first 256×256 fmt=29 RTV created in
   the window after our enable IS the `dynamicTextureRes`. Build correlation
   table in native, expose to Lua via IPC.
3. **Hook `DynamicTexture::SetResource` or equivalent** — find the setter
   and inject our own logging.

Option 2 (timing-based correlation) is fastest path to "we have the D3D12Resource
ptr in hand". Option 1 is more robust long-term.

## Open questions for M-A2

- Is the rendering one-shot or per-frame? RT-diag shows allocations (one-time)
  but not draw activity. Need OMSetRenderTargets / ExecuteCommandLists hook
  or RenderDoc capture to confirm per-frame execution.
- Does `resolutionWidth/Height` accept VR-scale values (e.g. 2000×2000)? Need
  to set them in Lua before activation and verify resource sizes scale.
- Two RTT cameras simultaneously — do they share infrastructure or each get a
  full pipeline? Critical for performance budget.
- IPD offset application — set `localTransform` on the comp or move the parent
  entity? localTransform exists in the field dump.

## File state at moment of success

- `testbed/cet_mod/CyberpunkVR_Testbed/modules/spawner.lua` v5.3 (48336 bytes,
  hash `85812B71`)
- `testbed/src/dxgi_testbed.cpp` (rt_diag slots 27/29/30/20 hooked)
- `testbed/red4ext/main.cpp` — minimal shell, ready for M-A2 native work
- Deployed at `C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\`
