# Optional V557 optical reconstruction

The saved November 9 bike capture shows a double upper windscreen edge with the
current NVIDIA OFA + FidelityFX reconstruction, while the old V557 NVIDIA-only
reconstruction produces a cleaner edge from the same endpoints. This is a
reconstruction comparison, not proof of a timing regression in the recent body
updates, nor proof that V557 improves every scene.

F10 > FRAMEGEN > Frame generator adds **NVIDIA OFA + V557 (experimental)**,
persisted as `xr_framegen_backend=3`. Existing values 0–2 and the default 1 are
unchanged. NVIDIA quality and flow resolution apply. Optical flow cannot be off
for this backend; selecting it with scale 0 normalizes to 50%. A non-NVIDIA GPU
falls back to FidelityFX, as with the existing NVIDIA choice.

The option uses V557's current-camera pre-warp, optical-flow cost confidence and
unidirectional midpoint composition. It does not use the engine vectors/depth
for interpolation. Input matching/capture is still retained by the port. The
shader is unchanged from the pinned reference. The historical edge-ghosting
switch remains compiled off; this is not a new edge-ghosting filter.

Only reconstruction is imported. The port's Presenter, real/synthetic cadence,
SteamVR timing package, private synthesis queue, idle preparation, endpoint
ordering and runtime submission remain in use. VDXR still uses the application
queue; enabling this backend does not activate SteamVR-only optimizations on it.
Generated images carry the current camera pose/FOV, unlike the existing FFX
interpolator's midpoint pose. Both direct and prepared submission paths honor
that distinction. Content time is still the temporal midpoint.

The adapter uses an attributed source subset in `externals/v557-reconstruction`,
built with Windows SDK fxc and the existing pinned FidelityFX/NVIDIA headers.
There is no additional runtime DLL, registry layer, external OFXR launcher,
tray configuration, or private XR instance. External OFXR's live tuning mapping
cannot control this component. The subset is LGPL-3.0; retain its source,
provenance and license when distributing builds. Original port files retain
their existing license.

Port adaptations to the component:

- Consume varying, already-captured COPY_SOURCE textures without another staging
  texture. Require matching resource descriptions/device, and a completed prior
  capture before replacing the source pointer.
- Keep native output textures in COPY_SOURCE between submissions. Transition
  within the synthesis command list, after the display consumer receipt.
- Defer the consumer GPU wait until composition so packing/OFA can overlap the
  previous display. CPU submission does not wait for those fences.
- Retain capture/guide leases through the final GPU receipt; refuse backend
  replacement/reset while the previous display owns output. Unknown GPU failure
  retains resources instead of freeing live allocations.
- Report explicit history/output/flow allocation sizes and the component's GPU
  pack+composition time separately from its total generation span. Driver-owned
  OFA allocation size is not exposed. Capture-start correlation reuses the
  existing capture queries and calibrated reconstruction timestamps; it does
  not insert new GPU work. Missing calibration stays missing, not zero latency.

Tradeoffs to measure in the headset: V557 uses pinned native captures when they
are published for shared reading; older/incompatible pools use three copied history
images per eye. Both routes own one synthetic output per eye plus OFA resources.
Their memory and capture-availability costs differ. Its rotational camera correction is depth-unaware for
translation. Flow-only reconstruction can improve this windscreen while
performing worse on other effects. No live FPS, memory or overall quality
improvement is claimed from the offline image comparison.

Validation commands (Windows; dependencies from the normal build):

- Build `tools/framegen_replay`; run `framegen_replay.exe --v557-test` with the
  game closed. Uses the production adapter and D3D12 debug validation, checking
  shared/private queues, independent eyes, priming/reset, backend replacement,
  display ownership, deferred capture and input lifetime.
- Run the same tool with `<capture directory> <output directory>` for a saved
  three-frame capture. The adapter's images are compared in the current camera;
  FFX outputs are compared in its midpoint camera. FFX is warmed for eleven
  stationary frames. Uneven capture spacing and synthetic warmup prevent
  treating image-error measurements as an exact quality percentage.
- `tools/flight_recorder_tests`: `prepare_ahead` verifies both camera contracts
  in direct/prepared submission; `package_settings` verifies all backend values
  roundtrip and that V557 requires optical flow.

Local test artifacts and deployment receipts remain in
`integration-notes/bike-ghosting-20261109/` and the build's deployment files.
Headset confirmation is required before recommending this as the default.

## Validation result, November 9

Both 3144x3000 integrated-backend images match the previously archived V557
reference pixel-for-pixel (all RGB pixels, both eyes). The clearer windscreen
edge is preserved. GPU adapter checks pass with D3D12 validation on shared and
private queues; all 16 existing frame-generation regression tests pass.
Live bike performance and headset quality remain unverified.

## Remove redundant real outputs, November 9

The first headset run fixed ghosting but reported a performance loss. Its long
Slow segment had about84.2 accepted real+synthetic frames/s but79.4 compositor
estimate, against81.8 accepted /85.9 estimate in an earlier PSVR2 Slow segment.
These were natural gameplay runs, not identical workloads; submitted and
compositor-estimated rates are not unique displayed FPS. The immediately previous
run was VDXR and is not an appropriate same-runtime baseline.

The initial adapter also copied every real endpoint into a second destination
that native Presenter never reads. Reconstruction-only mode now omits that copy
and its destination allocation, restores the current history state in the
composition list, and signals completion there. Each eye retains one synthetic
output, used for a priming copy when history resets. The default reference API
still produces both outputs; the compact mode accepts only pure NVIDIA flow and
advancing capture tickets. No shader, optical-flow quality, or pacing changes.

Two per-eye3144x3000RGBA8 destinations represent approximately72MiB of image
storage; two copy payloads per pair are eliminated. GPU lifetime/state tests
pass on shared/private queues, and saved output pixels remain identical in
both eyes. Live FPS recovery remains pending; the removed copies are a proven
inefficiency, not a proven explanation for the entire regression.

## Batch both eye preparations, November 9

The copy-removal run (PID3952, Medium/50%, PSVR2/SteamVR, 3144x3000 per eye)
retained the confirmed ghosting improvement, but delivery remained uneven.
340.0 active seconds: 35.70 accepted real + 35.38 accepted synthetic/s,
17.50 repeated submissions/s, 70.20 SteamVR compositor FPS estimate;
30-second gameplay windows ranged about57–82 in that estimate. The prior
Medium segments were75.76 and67.49. Natural gameplay and different segment
lengths prevent a controlled performance comparison; copy removal did not
establish a recovery. Accepted frames are not unique displayed frames.

The adapter had serialized the complete left-eye transaction, including its
previous-display wait, before right-eye history/packing/OFA. Now each mono
component can defer its composition. The adapter submits both preparations,
then flushes both output waits/compositions and signals the aggregate receipt.
It uses the same queue and fences, and does not move or remove ownership waits.
There are no new images, shaders, quality changes or pacing-policy changes.
The default vendored API still submits complete transactions. Deferred mode is
restricted to native mono reconstruction without extra outputs or real copies.
Known partial failures flush staged work before signaling the aggregate receipt;
unknown GPU failures retain resources.

GPU validation explicitly blocks the previous display receipt and proves both
preparations can drain, neither output completes early, both finish after release,
and repeated flushes are harmless (timing on/off). Production adapter tests cover
both queue routes, backend changes, resets, input lifetime and capture-to-generation
correlation. Saved3144x3000 bike images remain bit-identical in RGB, both eyes.
The generation span now measures earliest eye pack through latest composition,
rather than summing overlapping per-eye intervals. Therefore compare future span
values with that definition in mind. Active pack/composition costs remain summed.

Headset delivery benefit of this batching candidate remains unverified until the
next gameplay run. Keep Medium/50% and existing render/runtime settings for it.


## Borrow native capture history, November 9

The batched build's next natural gameplay run (PID22476) retained clean bike
edges. Its compositor estimate improved from70.20 to83.48FPS, while fresh
accepted submissions stayed approximately71/s. That supports better delivery,
not a controlled throughput increase or proof of83.5 unique displayed frames/s.
Capture readiness still accounts for most generation queue delay.

The next candidate removes the duplicate history ring for V557. Port-owned
capture textures allocated while this backend is selected are marked with an
immutable private-data tag and published in GENERIC_READ. Synthesis and display
can read them without changing their resource state. Writers restore that state
before the capture receipt and cannot reuse a texture until its pool pins retire.
The adapter imports the original texture, capture fence/value and owner lease.
The previous endpoint is released only after its last synthesis GPU receipt;
the current endpoint stays pinned for the next pair. COM references alone are
insufficient because a live capture texture can otherwise be overwritten.

This removes six history textures and two full-eye CopyResource payloads per
stereo pair. At3144x3000, replay allocation accounting falls from346,030,080 to
113,639,424bytes: 221.625MiB saved (driver-private memory excluded). Native capture-pool size, synthetic outputs, batching, optical flow,
shaders, pacing and submission policy are unchanged. The priming output copy
remains. AFW launches and existing untagged pools retain the copy path; switching
to V557 mid-session does not force pool reallocation. Initialization logs expose
`history-copy=0` or `1`. Tagged pools remain readable by the other backends after
a dynamic switch. Game-owned textures are never tagged or changed by this API.

Validation: D3D12 debug tests pass for both queue routes, actual three-slot
reservation/recycling, retained current/released previous pins, pending GPU
retirement, duplicate-import rejection, reset and V557/native NVIDIA/FFX switches.
Both copied and borrowed histories preserve the two-eye preparation behavior
while the display receipt is deliberately blocked, with timing on and off.
Full3144x3000 saved bike outputs match the copied adapter pixel-for-pixel in RGB
for both eyes. DLSS-G is not exercised by the replay build (no NGX in that target);
its production build succeeds and its tagged-input state handling was reviewed.

Live performance is pending. Holding native buffers longer can reduce available
capture slots even though it saves copies and allocations. Use the existing
CaptureNoSlot counter, source age, capture wait and compositor delivery alongside
fresh/repeated submission rates on the next same-settings natural bike run.
Retain the prior batched DLL as the rollback baseline. No FPS gain is promised.
