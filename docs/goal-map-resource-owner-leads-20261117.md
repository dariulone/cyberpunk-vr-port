# Map DynamicTexture: independent resource and cache ownership audit

2026-11-17. This work read repository code, existing PID26944 reports and the matching2.5 headless IDA database. It made no live-process reads, debugger attachment, memory search, database edits, native calls, reference changes, production patches or game-control actions. Renderer supplied the final five-object header snapshot separately under root's lease. The persistent owner of the measured map textures is still **unproven**.

## Strongest remaining producer lead: inkIconAtlas

The five stable rows in `scratch/goal-audit-20261117/census-deep-26944-b/dynamic-headers-before-exit.json` all have:

- CResource path at+0x30=0; generator object/control at+0x88/+0x90=0.
- Dynamic format+0x58=6, mip flag+0x59=0, samples+0x5A=1, creation flags+0x69=0x11, viewport scaling+0x48=0.
- The previously measured positive native strong counts4/9 and two references to their backing remain stable.

Those creation bytes match descriptor `0x11060100` in the runtime icon-atlas page factories `A901E8`/`A90304`. `5EA350` maps dynamic format6 to native texture format40. The ordinary HUD slot factories `1F5638`/`27C0DA0` use creation flags0x14, while CacheWidget factories `27CE4D4`/`27CE6DC` and the Minimap cache path use0x15. This is useful discrimination, conditional on the flags not being changed later; it is **not an identity join or proof of the allocation producer**. The map pair's lazy-backing stack does not record the earlier DynamicTexture object-construction stack.

The icon-atlas type is independently named by native RTTI, not guessed from nearby addresses:

| Node | Verified2.5 source/layout |
| --- | --- |
| InkSystem | Existing global storage RVA3C2B390; icon-atlas manager pointer at system+0x250, assigned by9A8AD4 |
| inkIconAtlasManager |64bytes, vtable RVA30B0730; factory9A8B44→constructorA9081C |
| Manager default atlas | Strong Handle object/control at+0x20/+0x28; destructorA91860 releases it through198368 |
| Manager custom-atlas registry | DynArray at+0x10: data pointer+0x10, capacityU32+0x18, countU32+0x1C;16-byte **weak** handles |
| Custom creation |2E02D98 creates a368-byte atlas, registers it through2DDD3E8, and returns a strong Handle; registration increments weak control+4 |
| inkIconAtlas | Vtable RVA30B05E8; GetNativeType13A2DC0→class storage3BCCC90; RTTI1666084 explicitly names `inkIconAtlas`, size0x170 |
| Atlas page vector | Atlas+0x158 data pointer, +0x160 capacityU32, +0x164 countU32; page stride0x88 |
| Page's texture-atlas resource | Strong `inkTextureAtlas` Handle at page+0x50/+0x58, created byA90178/A900BC and retained inA901E8 |
| inkTextureAtlas | Vtable RVA30B0498; GetNativeType1948070→3BBEC98; RTTI6A7710 explicitly names `inkTextureAtlas`, size0x138 |
| Its runtime DynamicTexture | Strong Handle object/control at atlas-resource+0x68/+0x70; A900BC copies the incoming DynamicTexture Handle and increments strong control+0 |

This provides a bounded typed path for a **future root-approved identity join**, without a heap sweep. Validate vtables/ancestry, read self/control and positive strong counts before following weak registry entries, cap vector lengths, repeat pointer/count identities, and compare DynamicTexture/backing identities against the actual census objects. Do not infer ownership from size or shape alone. The manager's custom-atlas vector is weak; it is not by itself the strong owner keeping a custom atlas alive.

`4BFFA8` appends icon-atlas pages when pending icon work does not fit. `2E02FA8` performs atlas compaction/repacking and removes empty pages; its temporary texture is2048². Existing textures768²/1024² can therefore be pages of custom atlas instances, but their actual custom-atlas owner has not yet been joined. Null CResource paths rule out a simple direct lookup of these objects by a nonzero file-resource path; they do not exclude a resource/library indirectly owning their atlas.

Relevant complete exports (all have saved=true JSON provenance) are under `scratch/goal-audit-20261117/map-texture-RE/`: `atlas-page-*`, `atlas-resource-constructor-a900bc.*`, `atlas-type-registration-6a7710.*`, `icon-atlas-manager-*`, and `dynamic-format-map-5ea350.*`. Existing `factory-caller-a901e8.c`/`a90304.c` contain the matching descriptors.

## External dynamic-texture registry: useful layout, no current join

`*(InkSystem+0x358)` is a different registry. Its header has a lock at0 and a DynArray at+8: data pointer+8, capacity+0x10, count+0x14. Records are0x38bytes:

| Record offset | Meaning |
| --- | --- |
|+0|CName/hash of the external texture|
|+8/+0x10|Weak producer CacheWidget object/control|
|+0x18/+0x20/+0x24|Consumer-A array pointer/capacity/count,16-byte weak elements; these are Mask consumers|
|+0x28/+0x30/+0x34|Second consumer array pointer/capacity/count; not interchangeable with A|

`27D91EC` registers the producer, `18A5A4` copies its **weak** Handle (increments control+4), and `27D9050` temporarily locks that weak Handle before reading the CacheWidget's strong texture Handle at+0x278/+0x280. It dispatches that texture to Mask consumers. `CC62D0` prunes expired weak entries; `27D9578` unregisters a producer and clears active consumers. The registry itself does not strongly own the producer or consumers.

CacheWidget field offsets are verified through RTTI106DC68: externalDynamicTexture+0x240, mode+0x250. Its strong texture field is+0x278/+0x280. Old generated headers show offsets16bytes earlier and must not be used blindly for2.5. CacheMode Normal0 allocates without requiring a named external registration; Minimap1 uses a fixed2048² path; ExternalDynamicTexture2 uses the named registry. An empty registry does not exclude Normal caches.

Renderer's `native-external-textures.json` reported count0/consumers0, stable and error-free in current gameplay; renderer also reported no entries during its subsequent bounded map check. This produced no map-target identity join. No additional live probe was requested by this audit.

## Retraction: Mask and Image must not be conflated

An initial hypothesis compared a strong assignment at offset0x260 with an Image destructor's weak release at the same numeric offset. **That hypothesis was rejected by type evidence. It is not a native reference-counting bug.**

- `27B4794` (`SetDynamicTextureSource`) assigns a strong Handle at+0x260 through1AE004. Its consumer-A registration comes from27B44AC/38F198.27B44AC processes the exact FNV1a hash of `dynamicTextureMask` (`B6318F74B20BFA2D`) and reads that property at+0x228; mode is+0x238.
- These are **inkMaskWidget** methods. RTTI17C9AD0 names that class, size0x290; vtable30C76F0, constructorBA4C10, destructorBA4BAC. Its destructor calls strong release198368 at+0x260, matching the assignment.
- **inkImageWidget** is a different class, size0x350, vtable302CA98, constructor1C30DC, destructor1C2F70. Its fields+0x250/+0x260 have weak destructors2750B0; +0x270/+0x280 have strong destructors198368. The Image's direct runtime-texture setter2AFEE8 assigns strong field+0x280.

Historical export names `image-external-texture-assign-27b4794.*`, `image-texture-handle-copy-1ae004.*` and `external-consumer-*` were created before this type resolution. **27B4794 is the Mask setter;1AE004 is a generic strong-Handle assignment helper.** The filenames are preserved as evidence, but must not be treated as type annotations. `mask-type-registration-17c9ad0.*`, `mask-constructor-ba4c10.*` and `mask-dtor-ba4bac.*` establish the corrected contract.

No reference decrement or cleanup patch follows from the rejected hypothesis.

## HUD draw masking and the real Image unload path

`HudPanel.cpp::HideSlot` deliberately writes visible=false without the native visibility callback so an XR HUD source can stay rendered. This fact alone is not a defect. `Visible` retains the game's requested visibility in `Mask.wantedVisible`. On retirement, `RestoreMask` restores the layout flag, forces visible to `!wanted`, then calls **originalVisible(widget,wanted)**, guaranteeing delivery of the real visibility transition. `Restore` publishes an empty mask set before restoring/clearing its handles. Native menu mode explicitly takes this Restore path.

The source comment naming Image visibility RVA4FC34C is stale for2.5: that address is inside a different RTTI wrapper. The current verified chain is:

`Widget::SetVisible3C9E0C` → vtable+0x138 → `inkImageWidget2B014C` → false branch `2AFFC8`.

`2AFFC8` locks at Image+0x2A0 and, unless byte+0x33C is set, clears the loaded flag+0x311 and the **strong loaded-object Handle+0x270/+0x278**. It does not clear the Image's weak+0x260 field or its strong direct-runtime source+0x280. The destructor releases all of those through their respective correct helpers. Inspect a loaded object's native type before treating it as an inkTextureAtlas; equal offsets across unrelated widget types are not enough.

This leaves a concrete indirect-holder test: join an actual surviving Image loaded/direct source or Mask direct source to a target DynamicTexture/atlas, then establish whether the genuine native hide/unregister/retire callback was missed. Current source shows forced callback delivery on mask retirement and menus; no lost-transition defect was established. Pool refs1 cannot by itself exclude indirect widget/resource ownership, but it also cannot prove this mask mechanism caused the map growth.

## Lua and port resource roots checked

- CET `ResourceAsyncReference` stores a copied8-byte path, returns null from GetHandle/GetValuePtr, and converts back to a path-only ResourceAsyncReference. It does not retain a ResourceToken or DynamicTexture. A normal `ResourceReference` is different: it holds a shared ResourceToken, whose resource Handle and dependency tokens are strong.
- SDK ResourceLoader's `tokens` map stores **WeakPtr** tokens. Its separate failed-token array is strong. A live texture cannot be attributed to the normal path cache without an actual retained token/resource chain.
- `MenuTooltip.cpp` retains four explicit tooltip-library tokens. Its external-library dependency injection is scoped by a thread-local RAII SpawnScope around VRMenuTooltipSpawn and requires the exact path/item match; ordinary native map spawns outside that scope do not acquire new injected dependencies. This is a genuine bounded library root, not a demonstrated map-texture root.
- `ArchiveExtensions.cpp` holds fixed patch-resource tokens for the session. The inspected goal build manifest patches the tutorial streaming block and construct body meshes, not a world_map/ink library. This narrows that route without claiming all native dependencies are harmless.
- MenuSurface strongly retains only dedicated named menu/lobby components with explicit3200×1600,16×2048,2580×1980 or3190×2900 targets. Its selection does not accept an arbitrary native map window.
- GameFont copies font bytes out of a local token. VehicleHands keeps numeric bounds/path data; its resource token is local. Neither retains a generic map resource token.
- Menu HUD-fade holds one current HUD root and clears it after fade restoration; vendor presentation restores and clears its temporary menu-layer root. Map observers use weak controller wrappers and clear them on uninitialization, as the earlier CET audit established. No newly discovered growing Lua strong controller/resource cache explains the measured pair.

These are concrete exclusions and future join paths, not a claim that the whole port is incapable of influencing native lifetime. The original6.25MiB-per-cycle observation remains unexplained until the surviving native objects are joined to their producer/owner or a controlled A/B establishes a causal port path.
