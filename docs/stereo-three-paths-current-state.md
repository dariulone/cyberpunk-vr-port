---
name: stereo-three-paths-current-state
description: Consolidated synthesis (2026-06-24) — three live implementation paths for native simultaneous stereo in CP2077, cross-referenced with all known external talks/blogs + our RE corpus
metadata:
  type: project
---

# Sync Stereo для CP2077 — три живых пути, текущее состояние

**Дата:** 2026-06-24
**Статус:** RE-фаза по path B (Sync Sequential) практически закрыта (`engine_re/dumps/SYNC_SEQUENTIAL_PROVEN.md`). После round 2/3 audit (см. ниже) часть утверждений скорректирована.
**Решение:** user (2026-06-24) — **100% делаем все 3 пути параллельно**, не выбираем один. С учётом round-3: Path A + B = active, Path C = parked до новой live-lead.

Этот документ — единая точка входа в текущее состояние работы. Old memories `cpvr-aer-v2-unified-producer`, `cpvr-realvr-reverse`, `cp2077-native-simultaneous-exhausted`, `cpvr-producer-double-mutates-mgr` корректны как **исторические** записи неуспехов, но НЕ как руководство к действию — диагнозы были частично ошибочные.

---

## 0. Round-6 update (2026-06-24, evening) — Path B fully closed, Path C reconfirmed parked

Audit `engine_re/dumps/_path_b_close.md` закрыл 3 final Path B вопроса:

- **Q1 (LIGHT prolog safety)**: ✅ LIGHT не использует SEH/__security_cookie/fs|gs TIB. Direct call `sub_14029A5B0(mgr, ...)` минуя vtable — mechanically законен. Frame и flags идентичны FULL (0x5410, 0x17B0 alloca).
- **Q2 (5 classes)**: vt1=base `RenderOrchestrator`, vt2=**`FunctionalTestsGameEngine`** (explicit string @ 0x142D07818) — test class, skip для VR. vt3=move-wrap helper. vt4=template-factory (нет classic ctor xref). vt5=large composite (size ≥13808, multi-vptr, Scene-shape). **Target для hook = vt1** (base, чистый).
- **Q3 (live LIGHT usage)**: 165/270/271/242/17 vptr-chain call sites per slot offset across .text (10.8M code heads). Все sample sites = classic `mov rax,[obj]; call [rax+OFF]`. LIGHT — heavily live в runtime.

**Path C bonus scan**: 828 функций матчат `writes [+0x48..0x68] AND touches [+0xD0]`, но **0 из 828 ссылается на main view camera vtable** 0x142B72EE8. Подтверждает round-3 negative: view-list assembly использует camera HANDLE indirection, не direct vtable. Path C **остаётся parked** до live-debugger lead.

**Implementation roadmaps готовы для обоих active путей**:
- `docs/path-A-implementation-roadmap.md` (RTT camera через holocall_camera.ent)
- `docs/path-B-implementation-roadmap.md` (Sync Sequential через CALLER1 + LIGHT direct call)

**Рекомендация**: Path A first как safer baseline, Path B parallel R&D track.

---

## 0. CORRECTIONS after triple-path audit rounds 2 & 3 (2026-06-24)

Три раунда targeted RE (скрипты `engine_re/scripts/triple_path_audit.py`, `triple_path_round2.py`, `triple_path_round3.py`; полная интерпретация — `engine_re/dumps/_ROUND3_INTERPRETATION.md`) дали три важных корректировки. Они не отменяют этот документ, но меняют priority и confidence.

### 0.1 Path C (Native AddView) — DEAD AS PREVIOUSLY FRAMED

`sub_140212780` **НЕ AddView**. Декомпиляция cluster-а (`sub_1402127C4`, `sub_14021291C`, `sub_140212800`, `sub_1402128AC`) показала это **float-weight stack** для blend operations: `+0xD0` = stack depth counter (push/pop), `+0x48` = mode dword, `+0x4C` = weight float, рекурсивный обход бинарного дерева через `a2[3]/a2[4]`+`a2[5]/a2[6]` (pointer + refcount pairs).

Это вероятно anim blend tree / audio mixing / similar weighted accumulator. Calling `sub_140212780(builder, &desc)` НЕ добавит view — изменит blend weight. `cp2077-native-addview-found.md` memory entry — **error-corrected**.

String search "RenderView/ViewTask/EnqueueView/AddView" дал **ноль hits**. CDPR strip-нул все view-related symbols. Найти real view-task submitter через static RE — невозможно. Live debugger (HW BP) — единственный путь.

**Решение**: Path C **parked** до новой angle. Не возвращаемся пока не найдём fresh lead.

### 0.2 Path B engine-precedent — слабее чем заявлено

Round-3 классифицировал 26 кандидатов из 131 "FULL+LIGHT callers". Из них:
- 6 sequential (вне if/else), но ВСЕ — разные смысловые роли (init pattern, get-count-then-fill-array, dispatch). Ни одна не делает "render same view twice".
- 5 mutex if/else (только один из слотов реально выполняется).
- Остальные decomp-fail.

**Что доказано**:
- ✅ Слоты +0x88 (LIGHT) и +0x90 (FULL) образуют **paired interface design** — used together throughout engine.
- ✅ Production code вызывает оба слота на одном persistent instance без crash → **mgr survives both calls invariant**.

**Что НЕ доказано**:
- ❌ Семантика "RenderLight = render view second time" — это **interpretation** из структуры sub_14029A5B0 (пропускает renderer-tick + 3-phase dispatch), не наблюдение в production.
- ❌ Что back-to-back FULL→LIGHT с разными view-state матрицами реально продуцирует второй render.

**Решение**: Path B остаётся viable (mechanical safety доказана), но первый код-тест должен быть **observational**: вызвать LIGHT без патчинга матриц, посмотреть что GPU реально делает (RT-diag, frame counter, GPU output trace). Только если возникнет второй render — патчим view-state.

### 0.3 Path A — async deferred resolve confirmed

`sub_141C9F5B0` = thin thread-init logger, real worker = `sub_141C9F660` который **enqueue-ит callback** через `sub_14095EC88` dispatcher с pointer `sub_141C9F830` (actual binding callback).

Это **deferred async pattern**: LoadDynamicTexture НЕ синхронно резолвит ID3D12Resource. M-PROTO 2026-06-07 false positive объясняется ровно этим: `NewObject("DynamicTexture")` не cooked → async resolve тихо fail-ит / no-op-ит → native engine secondary view (envprobe / cubemap) появлялся независимо.

**Решение**: Path A roadmap **подтверждается** — reuse `holocall_camera.ent` template (гарантированно cooked, гарантированно валидный path, гарантированно проходит async resolve в production каждую игру).

**Открытые цели для Path A**: decompile `sub_141C9F830` (binding callback) + `sub_14095EC88` (dispatcher) для понимания cooked-resource → ID3D12Resource chain. Это unblocks реальную реализацию.

---

---

## 1. Три пути в одной таблице

| | **Path A: RTT camera reuse** | **Path B: Sync Sequential CALLER1+LIGHT** | **Path C: Native AddView** |
|---|---|---|---|
| Что это | `entRenderToTextureCameraComponent` через reuse `holocall_camera.ent` template | `sub_140292A54` (FULL) + direct call `sub_14029A5B0` (LIGHT) на одном mgr с patched view-state | Append IPD-shifted descriptor в FrameGraph builder через `sub_140212780(builder, &desc)` |
| Engine-precedent | ⭐⭐⭐ `gameuiHolocallCameraComponent` (holocalls работают каждую игру) | ⭐⭐ 131 функция в .text реально делает FULL+LIGHT на одном instance (`sub_140291DF0` и т.д.) | ⭐⭐ `worldMirrorNode` (mirror в V's apartment) |
| Quality eye | ✅ Full deferred, per-eye feature toggles (SSAO/SSR/AA on-off) | ✅ Full + equal через тот же pipeline что main view | ✅ Full + equal (auto FrameGraph allocation) |
| Mod-доступ | ✅ Codeware AddComponent, CET-проверено | ❌ Detours hook в render-thread | ❌ Detours hook на populate-loop |
| Главный блокер | `dynamicTextureRes` = raRef к cooked DynamicTexture; runtime NewObject не binds | LIGHT direct-call безопасность; runtime-precedent of vt2-vt5 instantiation; TAA-history per-eye | Stack-transient parallel builder (6+ workers concurrent) — identify main-view builder среди concurrent calls |
| Состояние | M-PROTO 2026-06-07: AddComponent не упал. 1024² RT было false positive | RE-фаза закрыта 2026-06-23 (SYNC_SEQUENTIAL_PROVEN.md). 3 final вопроса до кода | RE-фаза не дозакрыта. Descriptor layout частично unknown |

---

## 2. Что нам дали внешние источники (все добавления к нашей картине)

### 2.1 GDC 2023 — "Building Night City" (Charles Tremblay)

PDF: `docs/external/gdc2023_building_night_city.pdf`. Большая часть слайдов — image-only; ключевая информация в speaker-notes на ~30 страницах.

**КРИТИЧЕСКОЕ открытие — стр. 90 speaker notes:**

> *"This is another unique node, which is only important for cases where we have **multiple renders in a frame (such as when a mirror is up)**. We only want a single offscreen simulation, and it will wait for culling from **all subgraphs** so it knows what's actually offscreen everywhere."*

**Что это значит**: движок имеет **first-class support для multi-subgraph render** в одном кадре. Зеркала используют это в продакшене. Sync Sequential VR = ровно тот же паттерн с N=2.

Архитектура render graph:
- Two-phase: **Prepare → Consume**. Resource lifetime tracked, automatic aliasing transient памяти.
- **"Unique" nodes** при merge subgraph-ов де-дуплицируются, regular nodes копируются.
- Кулинг — отдельный node per frustum: Main / RT-inflated / Cascades / LocalShadows.
- Shadowmaps: command list per cascade + per local light → **world-space, не нужно перерендерить для второго глаза**.
- Async compute fork-join (Hi-Z + SSAO во время shadowmap pass).

→ **Прямая релевантность Path B**: LIGHT в `sub_14029A5B0` пропускает renderer-tick и 3-phase dispatch именно потому что это **inner-loop** для дополнительного render-pass-а в существующем кадре. Это design-by-engineer (CDPR) для "multiple renders in a frame".

### 2.2 GDC 2024 — "Anatomy of a Frame" (Charles Tremblay)

PDF: `docs/external/gdc2024_anatomy_frame.pdf`. Confirms GDC 2023:

- **No monolithic render thread.** Main Thread "only schedules beginning of frame and waits for completion, acts as a regular job worker while waiting".
- **All render resources created asynchronously**. Нельзя синхронно alloc-ать ресурсы посреди dual-eye render — должны быть зарегистрированы в render-graph до Consume phase.
- **Scale 5/9/12/15/18/24 workers, sweet-spot 12** (10.2ms CPU). VR-PC обычно имеет 12+ HT cores → headroom для второй camera.

### 2.3 SIGGRAPH 2021 — "Transparency Rendering" (Sikachev et al)

PDF: `docs/external/sikachev_transparency_cp2077.pdf`.

- **DPL (Decoupled Particle Lighting)** в texture space (1024×1024). Allocation stage — hier-depth culling. Lighting stage — indirect dispatch на размер тайла. **Текстура потенциально shareable между двумя глазами** (texture-space lighting view-independent в core math, view-dependent только в gather).
- **Distortion Buffer** — отдельный off-screen buffer screen-space offsets от всех distorting transparents → resolve на затронутые тайлы. Полностью view-dependent → **per-eye обязательно**.
- **Glass dual-source blending** + **Parallel-slab refraction** — per-pixel, per-eye, OK.

→ **Прямая релевантность Path B**: + 1 новый per-eye transient в budget (distortion buffer). + 1 новый shareable potentially (DPL).

### 2.4 SIGGRAPH 2021 — "The Tech and Art of Cyberspaces" (Świerad et al)

PDF: `docs/external/siggraph2021_cyberspaces.pdf`.

**Производственное доказательство мульти-камерной архитектуры:**

> *"We achieved that by placing **2 or 3 cameras in every braindance location** in the game. These virtual cameras **render the same scene as the main one**, but have **lower resolution and narrower clipping distances**."*

> *"Only the Z depth of the scene is captured… **Each camera gets its own instanced mesh of quads**."*

> *"The color of the dynamic point cloud is calculated in the pixel shader. Then it is used to **fetch the final scene color from the game's buffers, after lighting but before post processes**."*

**Что это значит**:
- Движок штатно тикает 2-3 secondary cameras на одном кадре в обычной геймплейной сцене.
- НО конкретно braindance-камеры — **depth-only + sampling main color**. Это **photogrammetry depth probe**, не second renderer.
- Использовать ИХ для VR — нельзя (depth-only).
- Использовать **ту же инфраструктуру** на которой они ездят — можно. Это и есть Path A (RTT camera) + Path C (AddView).

### 2.5 c0de517e blog — "Hallucinations re: rendering of Cyberpunk 2077"

Local copy: `docs/external/c0de517e_cp2077_article.md`. Speculation by an outside engineer based on RenderDoc captures (decent quality, decent breadth).

Подтверждает архитектуру (но не RE-факты):
- Pure deferred + screen-space lighting (no baked GI).
- **Heavy temporal reprojection EVERYWHERE** — AO, SSR, volumetrics, GI, lighting.
- Clustered lighting через 3D texture.
- CSM обновляется частично много кадров (matches наш `sub_140293978` loop-of-8 cascade dispatch).
- Half-res AO/SSR + bilateral upsample.

→ **Релевантность всем 3 путям**: **per-eye TAA history** — главное эксплуатационное требование. Для path A и C это бесплатно (новый view = новые transients). Для Path B (Sync Sequential) — нужно либо две TAA-history (свапаем перед каждым eye-render), либо disable TAA для второго глаза.

### 2.6 NVIDIA Photorealistic Graphics + CyberFSR2 — нерелевантны

- NVIDIA page (`resources.nvidia.com/.../photorealistic-graphics`) — marketing video container, нет извлекаемого текста.
- `PotatoOfDoom/CyberFSR2` — drop-in replacement для `nvngx_dlss.dll`. Перехватывает NGX SDK calls, перенаправляет в FSR 2.0 upscaler. Не имеет отношения к стерео. Единственный косвенный сигнал: DLSS path в CP2077 хукабельный.

### 2.7 GDC 2024 — "Scaling Night City on the CPU" (David Block, separate from Tremblay)

PDF за GDC Vault paywall, недоступен. Ссылка: <https://gdcvault.com/play/1034296/The-Job-System-in-Cyberpunk>. Эта talk про **сам job system** (не frame-anatomy) — детальнее job API. Если получим — обновим этот раздел.

### 2.8 toreskovic/CyberpunkVR repo

Fork CyberEngineTweaks 1.62 с AER-стерео. Никакого Sync Sequential кода (явно в TODO). Полезный косвенный сигнал: pattern `CRenderGlobal::InstanceOffset` confirms global renderer instance pointer (наш `qword_143427C00`).

---

## 3. Path A: RTT Camera через holocall_camera.ent reuse

### 3.1 Что мы знаем (engine_re + docs)

- `entRenderToTextureCameraComponent` extends `entBaseCameraComponent`, size 0xA10.
- Поля:
  - camera: `localTransform@0xC0`, `worldTransform@0xE0`, `fov@0x128`, `near@0x168`, `far@0x16C`, `aspect@0x9E8`, `zoom@0x15C`
  - RTs (raRef к cooked DynamicTexture): `dynamicTextureRes@0x1E0` (color), `depth@0x1F8`, `albedo@0x210`, `normals@0x228`, `particles@0x240`
  - resolution: `width@0x258`, `height@0x25C`
  - control: `isEnabled@0x8B`, `renderingMode@0x290`, `features@0x9F6`
  - features struct: renderDecals, renderParticles, renderForwardNoTXAA, antiAliasing, contactShadows, localShadows, SSAO enum, reflections enum
- renderingMode enum: `Shaded=0` (full scene), `GBufferOnly=1`.
- featuresPlatform enum: `RTFP_All=0, RTFP_PC=1, RTFP_PC_PS5_XSX=2, RTFP_Consoles=3, RTFP_None=4`.
- **gameuiHolocallCameraComponent EXTENDS entRenderToTextureCameraComponent** → holocalls в продакшене драйвят этот механизм каждую игру.

### 3.2 M-PROTO (2026-06-07) — что доказано, что НЕТ

**Доказано:**
- AddComponent через Codeware `CallbackSystem.Get():RegisterCallback(n"Entity/Assemble", ...)` с EntityTarget Type(n"PlayerPuppet") + cb `EntityBuilderEvent:GetEntityBuilder():AddComponent(comp)` — **не падает**.

**НЕ доказано (false positive исправлен):**
- Изначально dxgi RT-diag показал 6 distinct 1024×1024 RTs появившиеся ПОСЛЕ AddComponent — это было **false positive**. Unique-resolution retest c 1234×1100 показал что **ни одного RT** такого размера не появилось. 1024² cluster — это native engine secondary view (env probe / cubemap), co-occurring с ViewProbe viewIdx=1 MULTI.
- **Реальная картина**: `entRenderToTextureCameraComponent` без bound `dynamicTextureRes` target = **no-op**. Field `dynamicTextureRes` — это `raRef` (reference by path) к COOKED DynamicTexture resource, не runtime instance. Assigning `NewObject("DynamicTexture")` к raRef **не biоds**.

### 3.3 Что нужно сделать (Path A roadmap)

**Reuse shipped templates**:
- `base\cinematics\cameras\holocall_camera.ent` — gameuiHolocallCameraComponent + cooked DynamicTexture, **proven working**.
- `base\media\tv\entities\virtual_camera.ent` — entVirtualCameraComponent + view target.

**План:**
1. **Spawn entity** через Codeware `SpawnEntity(holocall_camera_template_path, ...)`. Verify через dxgi RT-diag что появляется novel resolution RT.
2. **Drive pose**: переcписать `localTransform` каждый кадр = HMD head pose + IPD offset per eye. Установить `fov/near/far/aspect` под VR proj.
3. **Grab ID3D12Resource**: через d3d12 proxy перехватить allocation/binding нашего DynamicTexture, сохранить stable handle. Альтернатива: grab transient RT по unique resolution per frame.
4. **Composite**: copy color → OpenXR right swapchain. Left = native main view.
5. **Optimize per-eye**: `entRenderToTextureFeatures` с SSAO=0, reflections=0, antiAliasing=0 для второго глаза если нужен perf budget.

### 3.4 Open RE вопросы для Path A

1. **DynamicTexture binding mechanism**: как именно raRef резолвится при `OnAssemble`/`Initialize`? Какая функция в .text матчит raRef path → runtime DynamicTexture object → ID3D12Resource?
2. **Holocall_camera.ent layout**: можем ли мы прочитать .app/.ent файл (через ArchiveXL?) чтобы понять exact resource binding?
3. **Pose update timing**: где в frame можно безопасно patch-ить `localTransform`? До какого этапа FrameGraph readiness?

---

## 4. Path B: Sync Sequential via CALLER1 + LIGHT

### 4.1 Состояние

**RE-фаза закрыта 2026-06-23.** Полный документ: `engine_re/dumps/SYNC_SEQUENTIAL_PROVEN.md`. Краткое:

- Базовый класс `RenderOrchestrator` + 5 vtables (multiple inheritance).
- **CALLER1 = `sub_140292A54`** = RenderFull: BUILD → 3-phase dispatch (vt[80]/[A0]) → renderer-tick → vt[450]→vt[310] → SUBMIT-fn → teardowns.
- **LIGHT = `sub_14029A5B0`** = RenderLight: пропускает renderer-tick + 3-phase dispatch, остальное идентично.
- **SUBMIT-fn = `sub_140293978`** mutation surface = **6 scalar mgr fields** (`+0x118, +0x15C, +0x188, +0x18C, +0x1C0, +0x2B8`). `mgr+0x2B8` — это **QPC timestamp**, не shared view handle.
- **CALLER1 пишет 1 scalar** (`+0x348`). **Итого save/restore = 7 scalars = 56 байт.** 0 pointer writes, 0 locks, 0 refcounts.
- **Camera writer `sub_140788A9C`** — view-state patch points:
  - 4 matrices 4×4 на offsets `+0xA0/+0xE0/+0x120/+0x160` (256 байт)
  - 13 floats `+0x1A0..+0x1CC`
  - jitter `+0x1E4, +0x1E8, +0x1EC`
  - byte flags `+0x1F0, +0x1F3`
- **Engine-precedent**: 131 функция в .text вызывает и LIGHT, и FULL на одном instance. Самый плотный — `sub_140291DF0` (Δ от CALLER1 = 0xC64).

### 4.2 Hook plan

| Step | Адрес | Действие |
|---|---|---|
| 1 | Entry `sub_140292A54` (CALLER1) | Patch view-state matrices для L eye перед оригиналом |
| 2 | После return CALLER1 | `memcpy` save 7 scalar mgr fields → scratch |
| 3 | После save | Patch view-state matrices для R eye |
| 4 | `call sub_14029A5B0(mgr, ...)` direct | Eye-2 render через LIGHT path |
| 5 | После return LIGHT | `memcpy` restore 7 scalar mgr fields |
| 6 | RT pickup | `sub_1401F0F80` (RT-resolver) + `renderer+0x4C50` (RT table) → hand to OpenXR per eye |

### 4.3 3 final RE-вопроса до кода

1. **`sub_14029A5B0` пролог**: direct call мимо vtable безопасен? SEH/GS-cookie/per-vtable bookkeeping? (статический анализ disasm).
2. **Live LIGHT usage**: runtime в release-сборке реально вызывает LIGHT в живом cp2077.exe? Если да — какой класс (mirror / RTT / cubemap / photo / braindance)?
3. **vt2-vt5 ctor strings**: какие классы конструируются через эти vtables? RTTI strip-нули, но debug strings в ctors могут уцелеть.

### 4.4 Production risks (test live, не RE)

- ⚠ FrameGraph executor + RT-resolver могут выдать одинаковые transient RT IDs для обоих глаз → нужно либо force-allocate новые RTs для eye 2, либо ловить RT и копировать после каждого submit.
- ⚠ DLSS/Streamline (`sub_140CF2DB4`, `sub_140B77DE0`) ожидают один frame jitter — для eye 2 нужно swap jitter pattern или disable DLSS на стерео.
- ⚠ TAA history shared → "swimming" между eye 1 и eye 2. Решения: (a) per-eye TAA history swap; (b) disable TAA для eye 2 (хуже визуально); (c) reset TAA при каждом swap.

---

## 5. Path C: Native AddView via `sub_140212780`

### 5.1 Что мы знаем

Из `cp2077-native-addview-found.md`:
- **`sub_140212800`** = populate loop. Iterates view-source list (fields src+0x18/+0x20/+0x28/+0x30), builds small descriptor on stack, calls add-view fn per source. rcx=builder, rdx=source-node.
- **`sub_140212780`** = native AddView. rcx=builder, rdx=&descriptor. Через `sub_1402127B4` checks should-add → appends.
- **`sub_1402127B4`** = append/bookkeeping:
  - inline view array at builder `+0x48/+0x50` (stride 8, element[idx] at `+0x48+idx*8`)
  - `+0x48 dword` = flags set to 1, `+0x4C dword` cleared
  - count at builder `+0xD0` (incremented)

### 5.2 Найденная сложность (live recon 2026 sub_140212800)

- Builder — **stack-transient** object (vtable exe+0x2AC27F8, 3 ctors — generic registered-type).
- Builder runs **massively parallel**: 6+ worker tids simultaneously, каждый со своим stack builder.
- `count@+0xD0 = 2` everywhere (probably culling/visibility/view-list builders).
- → `sub_140212800` — **GENERIC parallel collection builder** (culling/visibility/view lists), а не один стабильный main-view registry.

### 5.3 Что нужно (Path C roadmap)

**Identify the main-view builder среди 6+ concurrent calls:**
1. Найти signature: view descriptor whose camera vtRva == `0x2B72EE8` (это main-view camera class, из `cp2077-renderview-orchestrator-reexec`).
2. Hook `sub_140212780` — gate на signature: if descriptor.camera == main view → это и есть main view builder.
3. Сразу после native AddView выполнить второй `sub_140212780(SAME_builder, &rightEyeDesc)` с IPD-shifted descriptor.
4. FrameGraph сам аллоцирует RT (proven via [[cp2077-framegraph-rt-routing]]).

### 5.4 Open RE вопросы для Path C

1. **16-байтовый descriptor format**: что лежит на `&rd` который передаётся в `sub_140212780`? Adresses, indices, transforms?
2. **Camera vtRva = 0x2B72EE8 идентификация в descriptor**: где именно в descriptor находится pointer к camera object?
3. **Survival downstream**: 2nd view доживёт до FrameGraph Consume phase без assertion fail в downstream code?
4. **Per-frame stability**: builder pointer пере-аллоцируется каждый кадр? Hook должен работать на ANY builder с matching signature.

---

## 6. Cross-path observations

### 6.1 Все 3 пути ведут к одной same инфраструктуре

GDC 2023 explicitly stated "multiple renders in a frame" — это **base capability** движка. Зеркала, holocall, braindance — все edges of one tree.

- **Path A** работает на уровне entity component model (highest level).
- **Path B** работает на уровне per-view dispatcher (mid level).
- **Path C** работает на уровне view-list builder (low level).

Все 3 — корректные срезы одного и того же multi-view system.

### 6.2 Что объединяет блокеры всех 3 путей

| Блокер | Path A | Path B | Path C |
|---|---|---|---|
| Per-eye RT routing | ✅ автоматически через DynamicTexture | ⚠ shared RT, нужен copy/swap или force-alloc | ✅ автоматически через FrameGraph |
| TAA history per-eye | ✅ свой view = свой transient | ❌ shared, нужен swap | ✅ свой view = свой transient |
| DLSS per-eye | ✅ свой view = свой DLSS | ⚠ может конфликт | ✅ свой view |
| Resource cleanup при моде разгрузке | ✅ Codeware unloads cleanly | ⚠ Detours надо снять и убедиться что mgr fully restored | ⚠ Detours надо снять |
| Сложность пинать stable | ✅ через CET console | ❌ binary hook, fragile across patches | ❌ binary hook, fragile across patches |

→ **Path A** меньше всего открытых вопросов на **runtime** уровне. Но требует cooked DynamicTexture asset.
→ **Path B** меньше всего открытых вопросов на **engine architecture** уровне. Но больше всего runtime risk.
→ **Path C** самый идиоматичный, но больше всего unknown bits в RE.

### 6.3 Параллельная стратегия

Делаем все 3, потому что:
1. Если path B даст AV на live — у нас есть path A как parallel fallback.
2. Если path A не сможет grab-нуть ID3D12Resource — path B и C дают другие RT acquisition routes.
3. Если path C породит downstream FrameGraph assertion fail — у нас уже есть path A и B.
4. **Знания не теряются** — RE сделанный для одного пути часто полезен другим (например, `sub_140788A9C` camera writer нужен всем трём для IPD-shift).

---

## 7. Следующий конкретный шаг (этот сезон)

**Запускаем `engine_re/scripts/triple_path_audit.py`** — один IDA-скрипт который параллельно отвечает:

| Вопрос | Для пути | Метод |
|---|---|---|
| LIGHT prolog SEH safety | B | disasm `sub_14029A5B0[0:0x40]`, look for SEH register / GS cookie load |
| vt2..vt5 ctor strings | B | для каждого vtable: find ctor (write of vtable into [rcx]), dump strings xref-нутые в ctor |
| Live LIGHT call-site classes | B | для каждой из 131 FULL+LIGHT функций — какие классы они оперируют |
| AddView descriptor layout | C | decompile `sub_140212780` + `sub_1402127B4`, dump field-by-field write/read pattern |
| Main-view builder signature | C | trace camera vtRva 0x2B72EE8 через `sub_140212800` callers |
| DynamicTexture → ID3D12Resource | A | grep strings "DynamicTexture", "ID3D12Resource", find resolution function |
| holocall_camera.ent loader | A | strings "holocall_camera", find entity loader fn |

Один прогон — 7 ответов, один dump `_triple_path_audit.md`, далее обновляем roadmap-ы для каждого пути и выбираем какой реализовываем первым.

---

## 8. Что мы НЕ делаем

- ❌ Не возвращаемся к inline replay / node replay / driver replay — все 7 классов исчерпаны.
- ❌ Не патчим CALLER1 для второго вызова с тем же mgr (это была ошибочная диагностика, fixed в SYNC_SEQUENTIAL_PROVEN — правильный second-call = LIGHT path).
- ❌ Не пытаемся reuse braindance virtual cameras (depth-only, sampling main color — wrong tool).
- ❌ Не делаем shader-injection GS-stereo (deferred screen-space pipeline → нечитабельно).
- ❌ Не делаем frame-interleaved (это AER, у нас уже есть).
- ❌ Не делаем SetTimeDilation (это freeze, не Skip Draw).

---

## 9. Ссылки на RE-материалы

| Что | Где |
|---|---|
| Полный engine→GPU map | `engine_re/ANALYSIS.md`, `engine_re/MAP.md` |
| 169-node pass graph | `engine_re/dumps/passes_index.md`, `dumps/nodes/work_000..005.md` |
| 5 vtables с LIGHT/FULL | `engine_re/dumps/_path1_vtables.md` |
| 131 FULL+LIGHT callers | `engine_re/dumps/_path1_callsites.md`, `dumps/_sub_14029A5B0_audit.md` |
| Light semantic probe | `engine_re/dumps/_light_semantic.md` |
| Sync Sequential finalize | `engine_re/dumps/SYNC_SEQUENTIAL_PROVEN.md` |
| Camera writer | `engine_re/dumps/C_camera.md` |
| RTT camera | `engine_re/dumps/E_rtt_camera.md` |
| RT-resolver / FG-build | `engine_re/dumps/B_framegraph.md` |
| External sources (PDFs + extracted txt) | `docs/external/` |
