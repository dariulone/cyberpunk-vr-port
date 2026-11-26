# Independent resource retention review

Read-only source audit while PID 20040 remained under the root agent's exclusive
runtime control. No build, debugger attachment, game mutation, or replay was run.

The observed map-reopen pair is a committed 768x768 and 1024x1024 texture,
DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, one mip, ALLOW_RENDER_TARGET. The renderer's
native probe reported pool references of one and surviving backing references.
These observations narrow ownership; they do not by themselves establish a
particular native controller as the leak source.

## Confirmed separate leak

- `src/Stereo/Capture.cpp:1626` captures every large texture transitioning to a
  shader-read state inside a VRCAM node. It does not require `CyberpunkVR_LumaProbe`.
  The filter at line 1639 requires width >= 1000.
- `src/Stereo/SyncStereo.cpp:589` deduplicates the current 24-entry capture set,
  taking an `AddRef` at line 600 for every new entry.
- `src/Stereo/Mirror.cpp:369` detects a changed tonemap constant buffer and resets
  the capture count to zero at line 373 without releasing the previous entries.
  The source explicitly documents the intentional leak. Later pushes overwrite
  those pointers. Repeated graph rebuilds therefore have no total retention bound.
- Concurrent pushes also use load/check/store rather than an exclusive reservation;
  two writers can both AddRef and overwrite the same slot. A correction must cover
  both ownership and synchronization, and protect sources already queued for GPU
  probe copies. Releasing pointers directly at reset would not establish that safety.

This path cannot explain the 768x768 member of the measured pair. It is a distinct
confirmed retention defect, not proof of the entire native-map leak.

## Excluded or bounded paths

- `include/Render/DescriptorCache.hpp` stores only integer resource addresses and
  dimensions. The RTV cache in `src/Stereo/Capture.cpp:417` does not AddRef resources.
  Its swapchain registration balances every GetBuffer with Release.
- `src/Render/SourceResourceIntent.cpp:167` accepts only default-heap buffers without
  flags. Its private-data owner contains an integer resource address, not a COM
  reference back to its owner.
- `include/Render/NativeTextureRead.hpp:75` requires resource flags NONE for immutable
  assets. The measured render targets cannot enter this immutable asset cache.
  Creation receipts retain a raw resource pointer, not a resource self-reference.
- ConstantUploadRegistry and the exposure/tonemap constant-buffer holders operate on
  buffers. Capture's exposure holder explicitly requires a 28-byte BUFFER and
  releases the previous entry on replacement.
- `src/Stereo/Hud.cpp` rejects mip counts <= 1 in the HUD signature. Its one current
  resource holder is replaced, not appended.
- `src/Natives/HudPanel.cpp:499` balances native pool-handle leases with the native
  release at line 541. An outstanding TextureLease adds a pool reference; poolRefs=1
  is inconsistent with additional outstanding leases of that handle from this path.
- On native menus, HudPanelUpdate publishes empty snapshots and clears masks
  (`src/Natives/HudPanel.cpp:603`). Render/HudPanel retires source snapshots and
  constant buffers after the completed copy fence before empty/stale early returns
  (`src/Render/HudPanel.cpp:145`). Output canvases are separate plugin allocations.
- Native HUD upkeep roots replace their prior vector using swap+clear on each scan
  (`src/Natives/HudPanel.cpp:307`). Repaint handles clear on update/frame submit.
  The Lua scan in `mods/cet/CyberpunkVRPort_Stereo/modules/hud_panel.lua:81` uses an
  explicit HUD-controller whitelist that excludes WorldMapMenuGameController.
- MenuSurface accepts only `vrp_menu_ui` and `vrp_lobby_ui`; its four authored sizes
  differ from 768x768 and 1024x1024 (`src/Natives/MenuSurface.cpp:51`).
- Both WorldMap CET modules retain one controller and clear it on OnUninitialize;
  they do not keep a controller history. A missed callback could retain the current
  controller, but successive assignments still replace that one reference.
- CommandResources keeps per-recording immutable batches, resets list private data
  on command-list Reset, and retires submitted batches on fence completion. Failure
  retention is deliberate and stops accepting new work. This does not prove every
  caller correct, but no map-specific accumulating holder was found there.

No production source was changed during this review. Findings and exclusions were
sent to the root and renderer agents; native DynamicTexture owner tracing continues
under their runtime lease.

## HUD virtual-window follow-up

The renderer later traced the pair through the native HUD-entry virtual-window
factory (entry slotTexture at +0xB8, window DynamicTexture at +0x4E8). A narrow source
re-review found only one writer of `useSeparateWindow`: HudPanel.cpp CopyEntries,
after the native copy function and before spawning. It changes a bool, not window
or slotTexture pointers. The whitelist in Runtimes/HudChannels.hpp includes phone,
holocall, subtitles, interactions, notification queues, tank, dossier/imprint,
briefing, scanner/camera and hit-marker entries; minimap/worldmap are absent.

This CopyEntries hook is unconditional with respect to xrHudPanel. Turning the
runtime HUD projection setting off therefore does not undo the earlier native
separate-window selection and does not exclude that creation path. A native owner
entry-name/texture-identity probe, or a controlled next-session pre-spawn A/B, is
required to distinguish it. No missing native destructor invocation was established
from this hook alone. No production code was changed by this follow-up review.
