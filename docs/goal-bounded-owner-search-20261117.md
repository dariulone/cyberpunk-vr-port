# Bounded native owner search: result and limits

The measured menu growth remains real: the clean PID20040 warm interval added
exactly two768² SRGB RTs and two1024² SRGB RTs (13,107,200 committed bytes) across
two native map open/close cycles, with all other committed groups net zero.
Explicit Lua collection did not release the pair. This does not identify the
owning subsystem, prove physical residency, or explain the entire13+GB complaint.

## Provenance and permitted access

Native game2.5 source executable and installed executable match SHA256
`d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4`, PE timestamp1792368047,
SizeOfImage0x581B000. Headless IDA exports and JSON provenance are saved under
`scratch/goal-audit-20261117/map-texture-RE`. No database edits were made.

The root authorized small read-only native table snapshots during its live lease
and separately granted measurement-free windows for the bounded C++ scans. All
process handles used query/read rights. No thread suspension, native/COM method,
AddRef/Release, memory write, game launch/exit, runtime switch or replay occurred
in these probes. The failed x64dbg preflight attached to nothing and installed no
breakpoint. Production leak fixes and the later diagnostic startup gate are
separate changes, not live memory patches.

## Established native lifetime facts

- Both resource shapes use native format40→DXGI29, flags0x83→RT flag1, one mip/sample.
- Measured depth16 reaches lazy DynamicTexture backing creation `5E9E34`, getter
  `28B4FC`, draw setup `28B89C`, VirtualWindow job `1F5C0C`, then a generic worker.
- The job consumes window+0x4E8. This field is raw; ctor `6FC168` zeros it and dtor
  `6FC070` does not release it. It is not itself the persistent owning Handle.
- One single-sample DynamicTexture normally holds its native backing twice at
  +0x70/+0x78. Its destructor releases both. The backing destructor releases its
  native pool handle; zero-native-ref handles enter a separate deferred queue.
- Five DynamicTexture objects were confirmed with the correct vtable,
  self-reference, positive native control count, stable fields and matching
  target backing pointers. Old map-shaped768objects had strong4/weak3, and
  1024objects strong9/weak3 at the final small read. A same-shaped additional
  1024object may belong to the scope scenario; shape alone does not prove census
  group membership. DynamicTexture CanBeDestructed returns true; these objects
  still have positive native Handle counts.

## What the bounded table snapshots excluded

Current HUD had72entries. None of its slotTexture or windowTexture backing IDs
matched the14target-shaped native objects at that endpoint. The current minimap
was380×386 and the phone2648×2864. Port HUD TextureLease would itself increment
the native pool refcount; the target pool count1 with a live backing owner does
not support a stuck additional HUD lease.

InkSystem had one LayerManager and13named layers. Main windows did not match the
targets. Twenty-six current world-component windows did not match either.
OffscreenLayer's two proven entry containers were empty. This does **not** cover
every internal advertisement/WorldFluff object or orphaned resource.

Layer+0x138 stores weak widget registrations, not necessarily VirtualWindows.
The reader was corrected to validate native RTTI ancestry before interpreting
window fields. The intermediate `native-window-registrations.json` contains row
errors and must not be used as owner evidence. Corrected results are in
`native-all-window-owners.json` and `native-owner-scan-input.json`.

Other code exclusions are narrow: NativeTextureRead rejects render-target flags;
SourceResourceIntent owns buffers; DescriptorCache stores pointer metadata;
Stereo/Hud matching rejects one-mip images; the map CET observers keep weak
wrappers and clear them on uninitialization. None proves the whole port cannot
influence lifetime. The separate tonemap AddRef leak was fixed independently;
its width>=1000 filter cannot explain both halves of this pair.

## C++ search coverage

All reports below are under `scratch/goal-audit-20261117/census-deep-26944-b`.
Only readable committed PRIVATE pages around known object allocations were
selected. The64MiB budget is total successful memory-read bytes; selected owner
pages also have a32MiB sub-limit. No full heap or address-space scan occurred.

| Report | Radius KiB | Bytes read | Duration ms | Native Dtex found | Direct object refs found |
|---|---:|---:|---:|---:|---:|
| native-owner-scan.json |64|11,877,816|5,300|3|0|
| native-owner-scan-wide.json |512|39,943,608|4,879|3|0|
| native-owner-scan-windows-first.json |1024|46,235,064|6,399|3|0|
| native-owner-scan-token-pages.json |1024|46,235,064|4,759|3|0|
| native-backing-referrers.json |1024|46,241,744|4,889|3|0|

All five runs reported zero failed reads and no total-budget exhaustion. The
wider runs nevertheless truncated requested owner pages at the32MiB sub-limit;
their reports expose those counts. Thus “zero direct refs found” only describes
the selected neighborhoods. It cannot establish absent owners. Prioritizing
candidate texture/control allocation pages produced no new coverage because
they overlapped already selected neighborhoods.

The backing-pointer search supplied a positive control:42exact pointer matches,
ten fields at five DynamicTexture-vtable candidates and32other records. Follow-up
small reads confirmed all five candidates' native self/control identities live.
The32other records resembled56-byte native DrawBuffer entries and used allocator
interface RVA30EAB20. But native `3E1600` requires valid source and target backing
pointers: none of these32source pointers had the verified native backing vtable,
while all32target pointers did. They are not demonstrated live owners and may be
freed-container residue. See `backing-record-headers.json` and
`dynamic-candidate-counts.json`. No reference was dropped based on these matches.

## Remaining uncertainty and next experiment

The persistent producer/resource/library holder is not identified. No claim is
made that a specific native destructor is missing, that the game alone is at
fault, or that force-freeing old textures is safe. Further heap sweeps are not
planned without a concrete new lead.

Root observed only ten CopyEntries override messages over the full PID26944 run,
so changing a gate after startup would not affect existing entries. A minimal
default1 startup diagnostic gate is prepared, with five counters and read-only
validation. First run mode0 in the same OpenXR/save/settings/layer conditions.
If target growth persists with verified forcedChanges=0 and bypassed=matches,
the added separate-window override is not a necessary cause. If growth disappears,
a matching fresh mode1 session on the same DLL is required before causal claims.
See `goal-hud-startup-ab-20261117.md` for exact root-controlled commands.

## Last structured reads before PID26944 exit

The independently resolved external dynamic texture registry is a pointer at
InkSystem+0x358. Its56-byte records contain a CName, weak CacheWidget producer,
and two arrays of weak consumers. A bounded read found the registry empty both
in gameplay and during one root-opened native map (`native-external-textures.json`
and `native-external-map-open.json`, stable/no errors). This excludes a current
registered producer association at those endpoints, not unregistered/local
cache objects or atlas holders.

A suspected strong/weak destructor mismatch was retracted after type verification:
setter27B4794 belongs to inkMaskWidget, whose destructor releases strong field260;
inkImageWidget's field260 is weak and has a different lifecycle. Comparing those
two classes would invent a native bug. The reader now labels Mask/Cache strong
bindings separately from Image weak bindings. No native release patch was made.

One final root-authorized small snapshot preserved the five known DynamicTexture
headers, self/control counts and backing IDs in `dynamic-headers-before-exit.json`.
All five remained stable/alive; generator+0x88 and field+0x90 were zero for all.
Resource/token/library and loaded atlas associations remain offline leads, not
identified owners. No more live reads or heap sweeps of PID26944 are planned.
