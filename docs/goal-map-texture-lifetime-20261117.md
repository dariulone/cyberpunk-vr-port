# Native map resource retention: measured identity and ownership investigation

2026-11-17. Investigation is read-only apart from optional census enable/report requests performed by the root agent. This report does not establish which component retains the resources and does not justify forcibly releasing game-owned textures.

## Measured resources

`scratch/goal-audit-20261117/census-openxr` contains the actual lifetime reports. Both groups are committed DEFAULT-heap Texture2D resources, one array slice, one mip, one sample, format 29 (`R8G8B8A8_UNORM_SRGB`), flags 1 (`ALLOW_RENDER_TARGET`).

| Census group | Dimensions | Allocation bytes each | after-five created/released/live | warm-after | after-lua-collect |
|---|---:|---:|---:|---:|---:|
| 28 | 768 × 768 | 2,359,296 | 6 / 0 / 6 | 8 / 0 / 8 | 8 / 0 / 8 |
| 29 | 1024 × 1024 | 4,194,304 | 6 / 0 / 6 | 8 / 0 / 8 | 8 / 0 / 8 |

The two additional warm map open/close cycles add exactly two of each resource: 12.5 MiB total, or 6.25 MiB per cycle. Explicit Lua collection did not release these objects. The first interval includes initial allocations and must not be described as exactly five created pairs. The warm interval isolates the repeated pair. These groups explain the repeated 6.25 MiB step, not the entire application VRAM footprint or every user's 13+ GB report. Allocation lifetime is not proof of physical residency.

Every recorded stack ends at the same six game RVAs: `34A8C9 → 3494AE → 34914A → 34808A → 346F92 → 5EA32A`. The last return address is inside `5EA284`, a native texture-pool allocation wrapper. Six frames do not reach the window/controller owner. It is incorrect to label this stack `MenuSurface::Create`: that mod function calls the separate DynamicTexture factory `5EA8A4`; proximity in the executable is not provenance.

## Native lifetime chain

The headless IDA database uses `build/patchday/ida/2.5/Cyberpunk2077.exe`. Its PE timestamp `1792368047` matches the census's actual loaded executable. Complete decompilations (or explicit `.asm` fallbacks) and JSON metadata were saved under `scratch/goal-audit-20261117/map-texture-RE`; each tool export reported `saved=true`. No database edits, game calls, replay or breakpoints were used.

The IDA source executable and installed Steam executable also have identical SHA256 `d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4` and true PE `SizeOfImage=0x581B000`. The initial reader incorrectly used IDA's maximum segment extent `0x5512000`; it rejected the process before any pool scan. The corrected reader uses the actual PE header and additionally verifies five native code spans before following pointers.

- `5EA8A4` allocates a 168-byte DynamicTexture and calls constructor `5EA914`. Constructor retains backing references at `+0x70` and `+0x78` if supplied.
- DynamicTexture destructor `5EA3C8` releases both backing references through `32DE14` (atomic decrement and virtual destruction on transition 1→0).
- Backing constructor `5EA214` inserts its raw address into the native manager's backing list. Destructor `5E9D68` removes it, releases its pool handle at `+0x18` through `343BEC`, then releases its other native owner.
- `343BEC` validates the handle, calls `3E3248`, and clears the handle. `3E3248` decrements the pool entry's native reference count and enqueues zero-ref handles for deferred cleanup. The game has a separate retirement queue/fence lifecycle; a zero native refcount does not by itself prove a D3D resource was already destroyed.
- Our `TextureLease::~TextureLease` uses that same `3E3248` decrement routine. There is no alternate increment/decrement offset mismatch in this path.

## Source exclusions and limits

- `NativeTextureRead::ImmutableShape` requires `D3D12_RESOURCE_FLAG_NONE`. Both measured render targets fail this criterion, so its immutable asset receipt/copy path does not explain this pair.
- `Stereo/Hud.cpp` keeps a single replaced HUD input and requires the HUD signature including its mip chain. The measured resources have one mip; the historical portrait/HUD mip-chain ambiguity is a different shape.
- Our dedicated SAO menu, beam and lobby surfaces use other dimensions. Native factory identity alone does not associate these two shapes with SAO.
- The map CET observers hold only a current controller and clear it on uninitialization. Lua collection did not change the measured resources; no per-frame forced-GC fix is warranted.
- Resource-lifetime cookies contain no resource/device/heap references. The census is an observer, not a resource owner.

These exclusions are narrower than “the mod cannot retain them.” Remaining possibilities include native controller/backing ownership, native retirement, or another COM holder. No production lifetime patch is made on these hypotheses.

## Read-only ownership probe

`tools/resource_census/native_textures.py` opens an existing process with query/read rights only. It validates the mapped game's PE timestamp and image size before using the 2.5 offsets. It snapshots the DynamicTexture backing list and a bounded native resource pool, selecting square 768/1024 textures by default. It reports native handle refs separately from backing refs, resource addresses, native shape fields, and race/error indicators. It makes no COM/native calls, no AddRef, no writes, no thread suspension, and performs no full process-memory scan.

```
python tools/resource_census/native_textures.py --pid <existing-game-pid> --out scratch/goal-audit-20261117/census-openxr/native-owners-closed.json
```

The default one-time pool scan is approximately 22 MiB. Read failures are explicit. Repeated reads can detect changes but cannot synchronize against native threads or rule out ABA reuse. Matching dimensions alone do not prove a row belongs to census group 28/29; correlate native format/flags, identity across controlled cycles, and count deltas. Native refcounts are not COM refcounts. Offline parser/layout/identity tests: 3/3 passed. Live execution is coordinated by the root agent.

Root's successful `native-owners-closed.json` (PID 20040) contains 100 native backing objects and 735 dimension-matching pool entries, zero read errors, stable registry/list pointers, and stable matching rows. Restricting to native format 40, flags 131 and mip/sample byte 4 leaves exactly nine 768² and nine 1024² objects. Every one has pool handle refs=1 and one matching backing object; sixteen backing objects have refs=2 and two have refs=4. There is no zero-native-ref retirement wait for these objects at this endpoint.

The shape match is verified from executable code: `34B328` maps native format 40 (`'('`) to DXGI 29; `34B190` maps native flags `0x83` to D3D12 render-target flag 1, byte 4 to one mip/one sample, and the recorded dimensions to Texture2D width/height. The nine pairs plausibly include one pre-census pair plus eight tracked pairs; resource creation counts alone do not prove that exact association. Native backing objects remaining alive is stronger evidence than a suspected standalone COM reference leak, but still does not identify the owning DynamicTexture/window/controller. The two DynamicTexture backing fields normally explain a backing count of two; they do not identify who owns the DynamicTexture itself.

Independent source review adds two useful exclusions:

- A retained port `TextureLease` would itself add a native pool reference. The measured pool refs=1 alongside a live backing owner is inconsistent with an additional retained HUD lease. HUD menu handling restores masks and publishes empty snapshots; completed GPU source leases retire before early returns. Pending roots and repaint lists are swapped/cleared, and the HUD controller whitelist excludes WorldMapMenuGameController. DescriptorCache stores pointer identities/dimensions without AddRef; SourceResourceIntent's owned destination paths are buffer-only.
- CET observers construct `this` as a `WeakReference` (`FunctionOverride.cpp`), so the map observer's `g_ctrl` is not a strong controller root. RTTI property caches hold property metadata, method closures do not capture the instance, and observer argument objects and copied handles are cleared/destructed after calls. No growing strong map-controller cache was found in these paths. The menu's retained root controller is also one observer weak wrapper.

A separate, real legacy tone-map capture leak was found during that review (`tm_set_push` plus `Mirror.cpp` resetting the retained-input count). Its width filter excludes 768²; it must be fixed on its own lifecycle evidence and must not be presented as the explanation for both measured map textures. That work is coordinated separately by the root agent.

## Prepared depth-16 native branch resolution

Further read-only decompilation established that backing allocation is lazy. Expected additional frames after the six known allocator frames are `5EA04C` (build native backing) → `5EA02C` (renderer virtual factory wrapper) → `5E9E34` (DynamicTexture lazy backing creation). This is a call-chain expectation to match against the next report, not a fabricated measured stack.

`5E9E34` reads DynamicTexture dimensions/format, calls the renderer virtual slot `+0x2D8`, and for a single-sample texture stores the same backing in both `+0x78` and `+0x70`. It releases prior values through native refcount helpers. Thus the observed backing refs=2 is the ordinary pair of fields in one DynamicTexture, not proof of an extra leaking reference.

The known callers into lazy creation are:

| Native function | Verified behavior |
|---|---|
| `28B4FC` | Getter for backing `+0x70`; creates it if absent under the texture lock. DynamicTexture vtable slot `+0x158`. |
| `28BA10` | Analogous getter for backing `+0x78`. |
| `490148` | Updates derived width/height and recreates existing backing only when dimensions changed and the texture uses derived sizing. |
| `5E9858` | Virtual callback recreates backing when its incoming flag is false. |
| `26B7560` | Explicit locked backing recreation. |

`10A3664` is a locked cleanup method that releases both backing fields; `5EA3C8` also releases both on DynamicTexture destruction. It would be unsafe to call either merely because a menu closed: ownership/GPU completion must first be demonstrated. Full sources/metadata for these functions are in the same `map-texture-RE` directory. The next measured outer stack should select the actual branch and its ink/window/controller caller. If it only reaches a generic render worker, more lifecycle evidence is needed before a fix.

The existing mod's `D.openMap` invokes the same `OpenShortcutMenu("OpenMapMenu")` helper used by the vanilla action path: decompiled script `OpenShortcutMenu` builds `HubMenuInitData(menuName="world_map")` and posts the normal hub initialization event. The close harness posts native `OnBack` events, then checks the UI menu blackboard. This is stronger than directly toggling UI flags, but does not prove that every native window/controller was destroyed; the outstanding native owner remains to be identified.

## Actual depth-16 and bounded ownership reads, PID26944

The new reports are in `census-deep-26944` and `census-deep-26944-b`. Both target shapes have the measured full stack:
`34A8C9,3494AE,34914A,34808A,346F92,5EA32A,5EA111,5EA03D,5E9F45,28B579,28B8F4,1F5D3B,1F55F2,2735E3,AE322E,1AA20CA`.
The last three frames are a generic worker loop and thread entry. `1F5C0C` is the VirtualWindow render job: it passes the DynamicTexture held at window+0x4E8 into `28B89C`, which requests its lazy native backing. This identifies the consumer/field but not the constructor or persistent owner of the texture. The adjacent function `1F5638` is a HUD slot allocator, but it is absent from the actual stack and must not be attributed to the measured pair.

Root's second warm interval in this process contained three extra pairs plus one4096×2048 atlas despite two deliberate cycles; an earlier screenshot-failed cycle had already closed normally. Do not silently convert this interval into an exact per-cycle average. The earlier clean two-cycle interval remains the exact6.25MiB/cycle observation.

Read-only probes now support `--hud --layers`. `--hud` resolves the actual mapped plugin export without PDB calls; native layer enumeration uses the verified address-library singleton hash2635337807, section3+0x328390 = RVA0x3C2B390. It follows `EngineObjects.cpp::Layers()` (InkSystem+0x380, first manager+0x38). Type names are resolved by reading only recognized `mov rax,[rip+disp];ret` native GetType stubs and the SDK CClass parent/name fields; it does not call a getter.

- Current HUD:72entries, stable, no read errors. None of its slotTexture/windowTexture handles matches any of the14 target-shaped native objects at the first snapshot. Minimap is380×386, phone2648×2864; neither is the target pair.
- Current InkSystem:1manager,13named layers, stable. Main layer windows have no target texture. The26current world-component windows also do not match. OffscreenLayer's two proven entry containers(+0x158/+0x168) are empty.
- Layer+0x138 is a weak **widget** registration list, not necessarily a VirtualWindow list. The intermediate `native-window-registrations.json` tried window interpretation before RTTI validation and contains explicit row errors; do not use it as window-ownership evidence. The corrected `native-all-window-owners.json` checks VirtualWindow ancestry before reading+0x4E8. Internal WorldFluff/advertisement collections and orphaned objects were not established by these reads.

Root then authorized a measurement-free bounded C++ memory search (`tools/resource_census_scan`). Three initial scans covered11.9/39.9/46.2MiB (5.3/4.9/6.4seconds), all with zero read failures; no process-wide scan occurred. Two live DynamicTexture identities join previous map backing addresses:

| Object | Shape | Native Handle control | Strong/weak refs at read |
|---|---|---|---|
| `0x1e0a3878920` | 768² | `0x1e0a3824130` |4/3|
| `0x1e0a3879840` |1024²| `0x1e0a3826290` |9/3|

Another1024² candidate had strong10/weak3 but was not part of the earlier pair snapshot and may be a scope texture. Candidate validation checks actual DynamicTexture vtable, self-reference, positive control count, both backing fields and dimensions, with a repeated identity read. No referrers appeared in the covered UI neighborhoods. This negative result is explicitly partial; it does not prove absence of owners. The scanner now also prioritizes the found texture/control allocation neighborhoods for a subsequent idle-window run, because resource token owners can live away from UI windows. No arbitrary reference decrement, native cleanup call or production leak patch is justified yet by this partial ownership evidence.

Subsequent idle-window runs of candidate/control-neighborhood priority and exact backing-pointer matching stayed within46.3MiB and returned zero read errors. The backing search found42 raw matches:10 fields at five DynamicTexture-vtable candidates and32 other records. Small follow-up reads confirmed all five DynamicTexture self/control identities live and stable. The map-shaped768objects consistently had strong4/weak3, and1024objects strong9/weak3 at the later read. Two additional joined object addresses were `0x1e0a3e20b50` (768) and `0x1e0a4ba3cd0` (1024); see `dynamic-candidate-counts.json`.

The32other pointer matches resemble56-byte DrawBuffer target-pair entries. Native `3E1600` requires both source and target pointers to refer to valid native texture backing objects; all32 observed target pointers were valid, but none of the corresponding source pointers had the verified backing vtable. They therefore do **not** establish live DrawBuffer ownership and may be allocator residue. RVA30EAB20 is an allocator interface used by draw containers, not the identity of a persistent owner. Results are in `native-backing-referrers.json` and `backing-record-headers.json`.

Further RE corrected another possible inference: VirtualWindow+0x4E8 is a raw texture binding. Constructor `6FC168` zeros that pointer, and destructor `6FC070` does not release it. Thus the consumer field found in the depth16 stack is not itself the owning DynamicTexture Handle. The producer/resource/library holder remains unresolved.

Root also counted only ten `separate window enabled` lines in the entire PID26944 log despite map cycles, teleports and other scenarios: `CopyEntries` ran at HUD startup. A runtime0/1 toggle of its override would not test existing entry policy. Root approved a minimal diagnostic startup gate, now prepared in `src/Natives/HudPanel.cpp` for the next planned build: default1, env0/1 read once, five counters, original copy always invoked. It has not changed the current live process and is not a leak fix. `docs/goal-hud-startup-ab-20261117.md` gives the otherwise-identical OpenXR two-session experiment and exact counter/map commands; read-only startup-validation tests3/3 passed. No native release patch has been made.
