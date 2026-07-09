---
name: path-A-implementation-roadmap
description: Concrete implementation roadmap for Path A (RTT camera reuse via spawn-from-cooked-template) — Sync Stereo for CP2077, after 5 RE rounds (2026-06-24)
metadata:
  type: project
---

# Path A — Concrete Implementation Roadmap

**Дата:** 2026-06-24. **Status:** RE-фаза для Path A **закрыта** через 5 раундов аудита (`engine_re/dumps/_triple_path_round{1..5}.md`). Готовы писать код. Связанные docs: `stereo-three-paths-current-state.md`, `cp2077-rtt-camera-native-stereo.md`.

---

## 1. Финальная mental model

CP2077 имеет **production-обкатанный** механизм secondary scene rendering через `entRenderToTextureCameraComponent`. Этот механизм используется:
- **gameuiHolocallCameraComponent** — каждый холозвонок в игре
- `entVirtualCameraComponent` / `entVirtualCameraViewComponent` — pre-rendered TV scenes
- Braindance virtual cameras (с reduced depth-only режимом)

Все они работают через **один и тот же** path:
- Component содержит `dynamicTextureRes` raRef к **cooked** DynamicTexture
- При assemble component-а движок запускает async job через generic dispatcher `sub_14095EC88` (88 callers — общий job submitter)
- Job resolves raRef path в Handle<DynamicTexture>, attaches к component
- При render: FrameGraph аллоцирует transient RT под DynamicTexture's размер
- Component получает свой full deferred render как secondary view

**Главный insight round-2/5**: попытка `NewObject("DynamicTexture") + AddComponent` (M-PROTO 2026-06-07) — **не работает** потому что NewObject создаёт runtime instance без cooked-asset binding. Async resolve тихо fail-ит.

**Решение**: вместо создания компонента — **spawn entity из cooked template**, который УЖЕ имеет валидный raRef + cooked DynamicTexture. Движок autoматически проходит весь bind chain как для holocall.

---

## 2. RE-факты, на которых базируется план

### 2.1 Component class (entRenderToTextureCameraComponent)
- Size 0xA10. Extends `entBaseCameraComponent`.
- Поля для drive в runtime (мы должны их обновлять каждый кадр):
  - `localTransform@0xC0` — pos + rot per eye (= HMD head + IPD offset)
  - `fov@0x128`, `near@0x168`, `far@0x16C`, `aspect@0x9E8`, `zoom@0x15C`
  - `resolutionWidth@0x258`, `resolutionHeight@0x25C`
  - `isEnabled@0x8B` — must be true
- Поля **которые мы НЕ трогаем** (must be cooked-bound):
  - `dynamicTextureRes@0x1E0` — raRef к cooked DynamicTexture (= наш target RT)
  - `depth@0x1F8`, `albedo@0x210`, `normals@0x228`, `particles@0x240` — optional MRT outputs
- Per-eye optimization toggles:
  - `renderingMode@0x290` enum: `Shaded=0` (full), `GBufferOnly=1`
  - `features@0x9F6` enum struct: renderDecals, renderParticles, renderForwardNoTXAA, antiAliasing, contactShadows, localShadows, SSAO enum, reflections enum
  - `featuresPlatform@0x9F8` enum: `RTFP_All=0, RTFP_PC=1, RTFP_PC_PS5_XSX=2, RTFP_Consoles=3, RTFP_None=4`

### 2.2 Bind chain (round 4/5)
- `sub_141C9F660` = real bind worker, делает **СИНХРОННО**:
  1. `sub_140142C9C(40)` — alloc 40-байт refcount-record через redMemory slab allocator
  2. `sub_14010FC2C(record, raRef_path)` — move-assign path в record
  3. `sub_1401942E4(target_handle + 16, src_handle + 16)` — **copy DynamicTexture Handle into RTT component**
  4. `*(target+32) = *(src+32)` — copy a flag
  5. `sub_14095EC88(record, ..., a3, a4)` — enqueue `sub_141C9F830` **cleanup** callback (не bind!)

**Важное открытие**: bind происходит синхронно в шаге 3. Async callback (`sub_141C9F830`) — это **refcount cleanup** через slab-free + handle-release, не сам load. Это значит после spawn-а entity binding state виден сразу же.

### 2.3 D3D12 anchor (из STATE.md)
- AMD AGS wrapper: `agsDriverExtensionsDX12_CreateDevice` caller `sub_142A43484`
- GPU output tail: Present node `sub_140B9D97C` → RHI present `sub_142A49A7C` + queue `sub_142A4990C`
- Renderer global `qword_143427C00`, texmgr at `renderer+0x70`, view-state at `renderer+0x4658`, **RT table at `renderer+0x4C50`**
- Existing dxgi proxy: `dxgi_factory_wrapper` hooks `CreateRenderTargetView` slot 20 + `OMSetRenderTargets` slot 46

### 2.4 holocall_camera template path
- Path: `base\cinematics\cameras\holocall_camera.ent`
- Owner class: `gameuiHolocallCameraComponent` extends `entRenderToTextureCameraComponent`
- Used by every holocall (Mr. Hands, V's friends, fixers) — **proven working in production each game**
- Альтернатива: `base\media\tv\entities\virtual_camera.ent` (TV screens)

---

## 3. Implementation plan (4 milestones)

### M-A1: Spawn template entity through Codeware

**Цель**: spawn существующий holocall_camera.ent entity, чтобы движок сам сделал bind с cooked DynamicTexture.

**Реализация (CET Lua + Codeware)**:
```lua
-- in mods/CyberpunkVR/init.lua or similar
local Codeware = GetMod("codeware")

-- Spawn the entity at player position
local function spawnRightEyeCamera()
  local player = Game.GetPlayer()
  if not player then return end

  local pos = player:GetWorldPosition()
  local rot = player:GetWorldOrientation()

  -- Spawn the holocall camera entity from cooked template
  Codeware.SpawnEntity(
    "base\\cinematics\\cameras\\holocall_camera.ent",
    pos, rot,
    function(entity)
      -- entity spawned — store handle
      VR.rightEyeEntity = entity
    end
  )
end
```

**Verification**:
- RT-diag должна показать novel resolution RT появившийся в frame после spawn (отличный от 3072² main view и 1024² envprobe)
- Если spawn путь правильный — entity persistent, можно его driver в каждом кадре

**Open question for M-A1**: Точная Codeware API для SpawnEntity in this CP2077 build. Если SpawnEntity не работает — fallback на CallbackSystem `Entity/Assemble` с EntityBuilderWrapper / `:AddComponent` + manually setting raRef value (потенциально через Reflection если Codeware exposes resource references).

### M-A2: Drive eye-2 pose per frame

**Цель**: в каждом frame patch entity's transform = HMD head + IPD offset, fov/near/far/aspect = VR proj.

**Реализация**:
```lua
-- on render frame callback
local function updateRightEye(headPose, ipdOffset, fov)
  local e = VR.rightEyeEntity
  if not e then return end

  -- Update transform on the camera component
  local cam = e:GetComponentByName("camera")  -- or whatever the component is called in template
  cam.localTransform.position = vector_add(headPose.position, ipdOffset)
  cam.localTransform.orientation = headPose.orientation
  cam.fov = fov
  cam.nearPlaneOverride = 0.05
  cam.farPlaneOverride = 5000.0
  cam.aspectRatio = VR.eyeAspect
  cam.isEnabled = true
end
```

**Verification**:
- Move entity → видно ли в RT-diag что content changes (e.g., camera looking around)?
- HMD head movement → mirrors in RT content within ~1 frame

**Open question for M-A2**: Какой member name в Codeware exposes localTransform writes. Need to check Codeware API docs. If raw write doesn't work — может потребоваться custom red4ext_plugin.

### M-A3: Grab ID3D12Resource from secondary RT

**Цель**: получить stable ID3D12Resource* нашей DynamicTexture для копирования в OpenXR swapchain.

**Реализация (D3D12 proxy side)**:
- Уже есть infrastructure в dxgi_proxy.cpp (CreateRenderTargetView slot 20 + OMSetRenderTargets slot 46)
- Modify: track all RTV creations. Distinguish secondary view RT by:
  - **Уникальное разрешение** (= наш `resolutionWidth/Height`, set to per-eye like 1920×2160)
  - **Появилось ПОСЛЕ enable нашего component** (set flag `bExpectSecondaryRT=true` сразу после M-A1 success)
- Once detected, lock the ID3D12Resource pointer, expose to OpenXR submit code.

**Code sketch (dxgi proxy)**:
```cpp
// hook on CreateRenderTargetView
void hk_CreateRenderTargetView(ID3D12Device* dev, ID3D12Resource* res,
                                D3D12_RENDER_TARGET_VIEW_DESC* desc, ...)
{
    D3D12_RESOURCE_DESC d = res->GetDesc();
    if (g_bExpectSecondaryRT && d.Width == g_eyeWidth && d.Height == g_eyeHeight) {
        g_rightEyeRT = res;  // capture
        res->AddRef();
        g_bExpectSecondaryRT = false;
    }
    return orig_CreateRenderTargetView(dev, res, desc, ...);
}
```

**Verification**:
- After spawn, frame N+1 should have `g_rightEyeRT != nullptr`
- Resource's CPU descriptor heap entry stays valid across frames

### M-A4: Composite into OpenXR right eye

**Цель**: each frame, copy g_rightEyeRT contents into OpenXR right-eye swapchain image.

**Реализация**:
- Existing OpenXR submit infrastructure already handles left-eye via main view
- Add right-eye path: `ID3D12GraphicsCommandList::CopyResource(openxr_right_image, g_rightEyeRT)` before xrEndFrame
- Resource barrier sequence: `g_rightEyeRT` should be in `PIXEL_SHADER_RESOURCE` or `RENDER_TARGET` state after FrameGraph; transition to `COPY_SOURCE`, copy, transition back.

**Verification**:
- HMD shows different image per eye (stereoscopic depth)
- Move HMD → image shifts naturally per eye
- IPD adjustment → parallax responds

---

## 4. Per-eye optimization (later, after baseline works)

После M-A4 baseline working:

- Set `featuresPlatform = RTFP_None` (=4) на правый глаз → отключает SSAO, reflections, antiAliasing → 30-50% perf win per eye
- `renderingMode = GBufferOnly` (=1) — пропускает lighting на правом глазу, **НЕ подходит для VR** (lighting per eye different)
- `resolutionWidth/Height` — можно уменьшить per-eye (e.g., 75% rendering scale)

---

## 5. Open risks (live testing only)

| Risk | Likelihood | Mitigation |
|---|---|---|
| Codeware SpawnEntity API differs in this CP2077 build | Med | Fallback: CET Reflection + raw component creation; or custom red4ext plugin |
| Multiple entities each frame conflict with FrameGraph allocation | Low | One entity, persistent across frames, drive transform |
| ID3D12Resource pointer may be transient (FrameGraph pool re-uses) | Med | Track by RTV creation per frame; accept that resource ID may change |
| Eye-2 may share TAA history with eye-1 → swimming | Med | Disable AA on eye-2 (`featuresPlatform = RTFP_None`) |
| Eye-2 RT may be 1/N res by holocall template scaling rule | Low | Override `resolutionWidth/Height` after spawn |
| Game may forcibly enable/disable entity based on gameplay (holocall start/end) | Med | Hook lifecycle events; re-spawn if despawned |

---

## 6. Что Path A решает и не решает

### ✅ Решает
- Equal-quality eyes (full deferred per eye)
- TAA/DLSS history per-eye automatically (separate view = separate transients)
- Per-eye feature toggles (SSAO/reflections off if needed)
- Production-stable mechanism (used by holocall в каждой игре)
- Maintainable (Codeware + CET, без low-level Detours hooks)
- Survives game patches better than binary hooks

### ❌ Не решает
- Performance: 2 full views = roughly 2× rendering cost. May need DLSS Quality (75% scale) per eye.
- Eye sync: holocall camera renders **in same frame** but timing of writeups (когда mainView vs holocallView completes) — нужно проверить какие RT-выпуски синхронны
- DLSS interaction: holocall by default может **не получать** DLSS upscale. Нужно проверить и если так — manually copy chain

---

## 7. Next concrete steps

1. **Validate Codeware SpawnEntity API**: check current Codeware version's API for spawning from cooked template path. Test with simple non-camera entity first.
2. **Write M-A1 prototype** as CET Lua script. Verify via RT-diag that new RT appears post-spawn.
3. **Decompile entRenderToTextureCameraComponent constructor** in IDA to confirm field offsets are exactly as we documented (memory `cp2077-rtt-camera-native-stereo` says size 0xA10 — verify in current build).
4. **Hook into dxgi_proxy** to detect secondary RT by resolution match.
5. **Start M-A2 drive** once M-A1 stable.

Estimated effort: 1-2 weeks for M-A1+M-A2 (prototype visible stereo), 1-2 weeks for M-A3+M-A4 (real OpenXR submit), 2-4 weeks polish.

---

## 8. References to RE dumps

- `engine_re/dumps/_triple_path_audit.md` (round 1) — initial 7-question pass
- `engine_re/dumps/_triple_path_round2.md` — `sub_141C9F660` real worker + async dispatcher
- `engine_re/dumps/_triple_path_round3.md` — disambiguation of Path B/C
- `engine_re/dumps/_triple_path_round4.md` — bind chain depth, sub_14095EC88 = generic job submitter (88 callers)
- `engine_re/dumps/_triple_path_round5.md` — sub_141C9F830 = cleanup not load; sub_141C9F660 binds synchronously
- `engine_re/dumps/_ROUND3_INTERPRETATION.md` — full corrections summary
- `engine_re/scripts/triple_path_round{1..5}.py` — re-runnable audit scripts
