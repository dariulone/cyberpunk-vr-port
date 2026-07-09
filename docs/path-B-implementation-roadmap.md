---
name: path-B-implementation-roadmap
description: Concrete implementation roadmap for Path B (Sync Sequential via CALLER1 + LIGHT direct call) — closed by audit `_path_b_close.md` 2026-06-24
metadata:
  type: project
---

# Path B — Concrete Implementation Roadmap

**Дата:** 2026-06-24. **Status:** RE-фаза для Path B **закрыта** через round-6 audit (`engine_re/dumps/_path_b_close.md`). Q1+Q2+Q3 (3 final open вопроса) получили ответы. Готовы писать код. Связанные docs: `stereo-three-paths-current-state.md`, `path-A-implementation-roadmap.md`, `engine_re/dumps/SYNC_SEQUENTIAL_PROVEN.md`.

---

## 0. Что закрыл round-6 audit

### Q1 — LIGHT prolog safety: ✅ DIRECT CALL ЗАКОНЕН

| Marker | LIGHT (sub_14029A5B0) | FULL (sub_140292A54) |
|---|---|---|
| __security_cookie | **нет** | **нет** |
| fs:/gs: TIB access | **нет** | **нет** |
| __EH_/__SEH_/handler | **нет** | **нет** |
| func flags | 0x5410 | 0x5410 (идентично) |
| frame | 0x17B0 + xmm6 spill | 0x17C0 + xmm6 spill |
| ABI | `rcx=mgr, xmm1=float, rdx, r8, r9` | идентично |

**Вывод**: LIGHT — обычная C++ функция без SEH/stack-guard. Прямой `call sub_14029A5B0(mgr, ipd_float, ...)` мимо vtable mechanically safe. Vtable wrapper не делает дополнительной работы поверх функции.

### Q2 — 5 vtables: classes identified

| vt | Addr | LIGHT slot | Ctor | Class identity |
|---|---|---|---|---|
| **vt1** | 0x142C6F588 | +0x88 | sub_141250B28 | **Base `RenderOrchestrator`** (fields +736..+840 = field set CALLER1 uses) |
| **vt2** | 0x14313B238 | +0x1C0 | sub_141550C80 | **`FunctionalTestsGameEngine`** (✅ explicit string at 0x142D07818) — test class |
| **vt3** | 0x1431DEDB8 | +0xF0 | sub_1429A38B0 / sub_1429A40A4 | Move/wrap helper class (3× sub_14011F5D4 swap pattern) |
| **vt4** | 0x1431E1470 | +0x370 | **0 ctors found** | constructed via different code-path (likely template factory) |
| **vt5** | 0x1431E34B8 | +0x98 | sub_1429CD3B8 | Large composite (size ≥13808, multi-vptr at +0/+848/+8264/+8376) — Scene class |

**Insight**: для Path B мы хотим **vt1 (RenderOrchestrator base)** как target — это самый чистый instance без extra setup. vt2 — test/dev class, не для VR. vt5 — composite, instantiation требует больше bookkeeping.

### Q3 — Runtime LIGHT usage: ✅ MASSIVE LIVE PRESENCE

Scan 10.8M code heads → 45 583 displ-call sites total → matches к LIGHT-shaped offsets:

| Slot | Calls in .text | Used by |
|---|---|---|
| +0x88 | **165** | vt1 (base RenderOrchestrator) |
| +0x98 | 270 | vt5 (Scene composite) |
| +0xF0 | 271 | vt3 (wrapper) |
| +0x1C0 | 242 | vt2 (FunctionalTests) |
| +0x370 | 17 | vt4 (template) |

Каждый sampled call site показывает classic **vptr-chain pattern** `mov rax, [obj]; call [rax+OFF]` — это **не false-positive HI→LO coincidences**, а полноценный virtual dispatch.

**Caveat**: счётчики offset-based, не vtable-specific. Среди 165 +0x88 calls некоторые могут принадлежать другим vtables с тем же offset. Но даже консервативная оценка — LIGHT в продакшене **гарантированно жив**, потому что есть call sites где prior insn = `mov rcx, qword_143427C00; mov rax, [rcx]; call [rax+OFF]` (renderer global). Если бы LIGHT slots были dead — компилятор / strip их бы выпилил.

### Q4 (bonus) — Path C: confirmed parked

Scan нашёл 828 функций удовлетворяющих "writes [+0x48..0x68] AND touches [+0xD0]" pattern. **НИ ОДНА из 828 не ссылается на main-view camera vtable 0x142B72EE8.** Это сильный negative — view-list assembly использует **camera HANDLE indirection**, а не direct vtable pointer, поэтому static cross-ref не находит его. Path C нужен live debugger для прогресса, парковать дальше.

---

## 1. Финальная mental model

```
Frame N entry (mainView eye-1):
  hook entry → patch view-state (sub_140788A9C) с L-eye matrices/jitter
  call sub_140292A54 (CALLER1 = FULL)              ← original eye-1 render
  return

After eye-1 returns, in same hook:
  memcpy save 7 scalar mgr fields → scratch buffer (56 bytes)
  patch view-state (sub_140788A9C) с R-eye matrices/jitter
  call sub_14029A5B0 (LIGHT) direct                ← eye-2 render
  memcpy restore 7 scalar mgr fields ← scratch
  optionally: restore view-state to L-eye for safety

→ FrameGraph выдаст 2 разных RT (по одному на eye)
→ Pickup per eye → OpenXR submit
```

**7 scalar mgr fields to snapshot/restore (= 56 bytes)**:
```
+0x118 (dword)  — frame call counter
+0x15C (dword)  — scalar
+0x188 (dword)  — scalar
+0x18C (dword)  — scalar
+0x1C0 (byte)   — "view active" flag
+0x2B8 (qword)  — QPC timestamp
+0x348 (dword)  — CALLER1-only scalar
```

**Camera writer `sub_140788A9C` patch points (view-state object)**:
- 4× 4×4 matrices: +0xA0 (view), +0xE0 (proj), +0x120 (viewProj), +0x160 (invView/prevVP) — 256 bytes
- 13× floats: +0x1A0..+0x1CC (fov/near/far/clip)
- jitter: +0x1E4/+0x1E8/+0x1EC
- byte flags: +0x1F0/+0x1F3

---

## 2. Implementation plan (4 milestones)

### M-B1: Hook entry/return of CALLER1 — observational pass

**Цель**: HOOK `sub_140292A54` без модификаций. Логи: rcx (mgr), время вызова, какой vtable у mgr (`*(void**)mgr`). Подтвердить что CALLER1 вызывается ровно один раз на frame с консистентным mgr.

**Implementation (red4ext / detours)**:
```cpp
// red4ext plugin or direct detour
using fnCaller1_t = void(*)(void* mgr, float arg_x1, void* rdx, void* r8, void* r9);
fnCaller1_t orig_Caller1 = nullptr;

void detour_Caller1(void* mgr, float x1, void* rdx, void* r8, void* r9) {
    void* vt = *(void**)mgr;
    LOG("CALLER1 entry: mgr=%p vt=%p x1=%f", mgr, vt, x1);
    orig_Caller1(mgr, x1, rdx, r8, r9);
    LOG("CALLER1 return");
}

// In init: MH_CreateHook((void*)0x140292A54, detour_Caller1, (void**)&orig_Caller1);
```

**Verification**:
- 1 CALLER1 call/frame in normal gameplay
- mgr vtable consistent across frames (= 0x142C6F588 vt1)
- frequency matches game framerate

**Risk**: ASLR — addresses needed RVA-based. Existing project already uses RVA-style hooks (см. `dxgi_factory_wrapper`); copy approach.

### M-B2: LIGHT direct-call observational

**Цель**: после возврата CALLER1 — сразу вызвать `sub_14029A5B0` direct call с тем же mgr и теми же arguments. **БЕЗ patch'а view-state**. Смотреть:
- LIGHT возвращается ли без AV?
- FrameGraph аллоцирует ли дополнительный RT?
- Frame counter / SUBMIT-fn вызывается ли?

Этот шаг = **самый risky** во всём плане. Mechanical safety доказана (Q1), но runtime semantics требует наблюдения.

```cpp
using fnLight_t = fnCaller1_t;
fnLight_t pLight = (fnLight_t)0x14029A5B0;

void detour_Caller1(void* mgr, float x1, void* rdx, void* r8, void* r9) {
    orig_Caller1(mgr, x1, rdx, r8, r9);

    // === EXPERIMENTAL: invoke LIGHT after FULL ===
    if (g_enableLightProbe) {
        LOG("LIGHT probe: calling sub_14029A5B0 with same args");
        __try {
            pLight(mgr, x1, rdx, r8, r9);
            LOG("LIGHT probe: returned cleanly");
        } __except(EXCEPTION_EXECUTE_HANDLER) {
            LOG("LIGHT probe: SEH caught code=0x%X", GetExceptionCode());
            g_enableLightProbe = false;
        }
    }
}
```

**Verification (live, in-game)**:
1. RT-diag (existing dxgi proxy infrastructure): new RT появляется в frame после LIGHT probe? Если **да** → engine реально аллоцирует второй render. Если **нет** → LIGHT отрабатывает no-op или reuse-ит eye-1 RT.
2. Frame counter (+0x118) — увеличивается на 2 per frame? (CALLER1=+1, LIGHT=+1)
3. Crash? GPU AV? Если SEH ловит — disable, узнаём какой именно field/object падает.

**Gate**: если LIGHT возвращается cleanly и приращивает frame counter — продолжаем M-B3. Если crash → root-cause через windbg (запоминаем какой field touched), возможно нужно частичное snapshot/restore прежде LIGHT.

### M-B3: Snapshot/restore + view-state patch (real stereo)

**Цель**: интегрировать snapshot 7 scalar fields + patch view-state matrices с R-eye IPD-shifted данными прежде LIGHT call.

```cpp
struct MgrScalarSnap {
    uint32_t f_118;
    uint32_t f_15C;
    uint32_t f_188;
    uint32_t f_18C;
    uint8_t  f_1C0;
    uint64_t f_2B8;
    uint32_t f_348;
};

void snapshot(MgrScalarSnap& s, void* mgr) {
    auto p = (uint8_t*)mgr;
    s.f_118 = *(uint32_t*)(p + 0x118);
    s.f_15C = *(uint32_t*)(p + 0x15C);
    s.f_188 = *(uint32_t*)(p + 0x188);
    s.f_18C = *(uint32_t*)(p + 0x18C);
    s.f_1C0 = *(uint8_t* )(p + 0x1C0);
    s.f_2B8 = *(uint64_t*)(p + 0x2B8);
    s.f_348 = *(uint32_t*)(p + 0x348);
}
void restore(const MgrScalarSnap& s, void* mgr) {
    auto p = (uint8_t*)mgr;
    *(uint32_t*)(p + 0x118) = s.f_118;
    *(uint32_t*)(p + 0x15C) = s.f_15C;
    *(uint32_t*)(p + 0x188) = s.f_188;
    *(uint32_t*)(p + 0x18C) = s.f_18C;
    *(uint8_t* )(p + 0x1C0) = s.f_1C0;
    *(uint64_t*)(p + 0x2B8) = s.f_2B8;
    *(uint32_t*)(p + 0x348) = s.f_348;
}
```

**View-state patching**: hook `sub_140788A9C` (camera writer) ИЛИ patch view-state object directly. Locate view-state via mgr indirection chain (need RE: which offset from mgr to view-state? — likely `mgr+0x320` based on CALLER1's `lea rcx, [rsi+320h]`).

Patch sequence:
```cpp
void detour_Caller1(void* mgr, float x1, void* rdx, void* r8, void* r9) {
    // patch view-state for L eye
    PatchViewState(mgr, g_leftEyeMatrices, g_leftEyeJitter);
    orig_Caller1(mgr, x1, rdx, r8, r9);

    // snapshot
    MgrScalarSnap snap; snapshot(snap, mgr);

    // patch view-state for R eye (IPD-shifted)
    PatchViewState(mgr, g_rightEyeMatrices, g_rightEyeJitter);

    // R-eye render
    pLight(mgr, x1, rdx, r8, r9);

    // restore mgr scalars
    restore(snap, mgr);
}
```

**Verification**:
- HMD shows different content per eye (stereoscopic depth)
- Move HMD → both eyes shift consistently
- IPD change → parallax responds

### M-B4: RT pickup per eye + OpenXR composite

**Цель**: захватить ID3D12Resource обоих eye RTs и hand-off в OpenXR submit.

Two strategies:

**Strategy A — RT-table polling**: после LIGHT возврата считать `renderer+0x4C50` (RT table) и забрать **последний** добавленный RT entry. Eye-1 RT = тот что был до LIGHT, eye-2 RT = после.

**Strategy B — DXGI proxy CreateRenderTargetView gate**: уже есть infrastructure (`dxgi_factory_wrapper` hooks slot 20). Set flag `g_inLightCall = true` перед LIGHT, `false` после; в hook ловим RTVs созданные между.

Затем:
```cpp
// In OpenXR submit:
copyResource(openxr_left_image,  g_leftEyeRT);
copyResource(openxr_right_image, g_rightEyeRT);
```

**Verification**:
- Both OpenXR swapchain images contain valid pixels
- Per-eye TAA history independent (no swimming)
- DLSS upscale работает per eye (если не — disable DLSS / use alternative upscaler)

---

## 3. Per-eye optimization (later)

После baseline working (M-B4):

- **TAA history per-eye**: hook FrameGraph history-buffer routing, force separate ring buffers per eye. Если слишком сложно — disable TAA для eye-2 (хуже visually, проще).
- **Shadow cascades share**: per GDC 2023 — shadowmaps в world-space → не нужно перерендерить. Уже автоматически потому что CALLER1 уже их сделал; LIGHT их пропускает (no 3-phase dispatch).
- **CSM не пере-вычисляется** — perf win по сравнению с full naive 2× rendering.

---

## 4. Open risks (live testing only)

| Risk | Likelihood | Mitigation |
|---|---|---|
| LIGHT direct call returns cleanly but produces no second RT | **Высокий** | M-B2 observational gate — если так, переход на Path A (RTT camera) приоритетнее |
| DLSS/Streamline ассертит на double-frame jitter | Med | Disable DLSS для VR, fallback на FSR2 / no upscaling |
| TAA history shared между eyes → swimming | Med | Force per-eye history buffer split, или disable TAA |
| `mgr+0x320` (view-state) НЕ единственный pointer для view matrices | Med | Backup plan: hook sub_140788A9C entry, snapshot all writes, replay с patched values для R eye |
| Game patch breaks RVA offsets | Med | Use pattern-based scanning (как CyberpunkVR repo делает для `CRenderGlobal::InstanceOffset`) |
| 2 view bind на shared FrameGraph node → assertion fail downstream | Low (per GDC 2023 "multiple renders supported") | If happens — examine downstream node-replay logic |
| LIGHT path skips a write that другой downstream code expects | Med | snapshot more fields (not just 7) preemptively if AV |

---

## 5. Что Path B решает и не решает

### ✅ Решает
- Real Sync Sequential (both eyes within one engine tick, no extra game time elapsed)
- World-space resources (shadows, GI buffers) shared → perf win vs naive 2× render
- Lower overhead than Path A (no entity spawn, no transform sync per frame)
- Engine-native code path (massive runtime usage = battle-tested)

### ❌ Не решает / risks
- Per-eye TAA history needs explicit work (vs Path A automatic)
- Hook fragility across game patches (binary RVA-based)
- Eye-2 RT pickup heuristic (no clean API like Path A's DynamicTexture)
- Performance: ~2× draw call cost (но less than naive — shadows shared)

---

## 6. Сравнение Path A vs Path B (выбор первого target)

| Critère | Path A (RTT camera) | Path B (Sync Sequential) |
|---|---|---|
| RE certainty | ⚠ Async bind chain partial (cooked DynamicTexture resolve) | ✅ Fully closed (6 audit rounds) |
| First prototype effort | 1-2 недели | 1-2 недели |
| Engine precedent strength | ⭐⭐⭐ holocall каждую игру | ⭐⭐ 100s of LIGHT calls/sec |
| Per-eye quality control | ✅ explicit features struct | ❌ all-or-nothing |
| TAA per-eye automatic | ✅ | ❌ needs explicit work |
| Patch survival | ✅ Codeware abstraction | ❌ binary RVA |
| Failure mode if Path X dies | safe (graceful) | crash risk (AV/GPU) |

**Recommendation**: Path A first as **safer baseline**. Path B as parallel R&D track — может дать lower-overhead render если Path A perf budget too tight.

---

## 7. Next concrete steps

1. **M-B1 prototype**: write basic CALLER1 hook in existing red4ext plugin, observe mgr/vtable for 60s of gameplay → confirm vt1 base RenderOrchestrator is real target.
2. **M-B2 LIGHT probe** with SEH guard. **Critical gate** — если M-B2 не продуцирует second RT, Path B is dead-in-practice (despite mechanical safety).
3. **Если M-B2 ✅** — write M-B3 view-state patching with IPD-shifted matrices.
4. **Iterate** на M-B3 → M-B4 based on what RT pickup looks like in live frames.

---

## 8. References

- `engine_re/dumps/_path_b_close.md` (round 6) — Q1+Q2+Q3 closure
- `engine_re/dumps/SYNC_SEQUENTIAL_PROVEN.md` — earlier 5-round audit
- `engine_re/dumps/_path1_vtables.md` — 5 vtables initial discovery
- `engine_re/dumps/_path1_callsites.md` — 131 FULL+LIGHT call sites
- `engine_re/dumps/_sub_14029A5B0_audit.md` — LIGHT semantic difference vs FULL
- `engine_re/scripts/path_b_close.py` — re-runnable audit
- `docs/stereo-three-paths-current-state.md` — high-level synthesis
- `docs/path-A-implementation-roadmap.md` — parallel Path A plan
