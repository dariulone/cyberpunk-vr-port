# Mod UI in VR

## Renderer families

The integration boundary is the renderer, not the mod loader or language.

| Family | Common examples | VR integration |
|---|---|---|
| Native ink menus and modal widgets | redscript, Codeware UI, Native Settings UI, Mod Settings, New Game Plus menus, native character editors | Existing game menu texture and ink input adapter; preserve the actual widgets and callbacks |
| ImGui drawn by CET | CET console/bindings/settings; CET mods using `onDraw` | CET renderer adapter exports its own canvas and receives laser/button events; individual Lua mods keep their callbacks |
| ImGui drawn by this port | Port-hosted Lua windows and F10 | Same panel/input contract, rendered by the owning ImGui context |
| Independent native renderers | A RED4ext plugin with its own graphics backend | Optional versioned C ABI; supply a fenced canvas, consume semantic input |
| World-space ink | Computers, terminals, keypads | Retain the world surface and use its mesh/UV pointer adapter |

RED4ext loads native plugins; redscript is a scripting language. Neither defines
a separate window renderer. Codeware exposes ink widgets, window and input APIs.
Native Settings UI uses native UI even though CET drives its logic. Mod Settings
also extends the game's settings UI. WolvenKit is an authoring application;
its ink assets become native game UI, not WolvenKit windows inside the game.
ArchiveXL loads resources (including widget libraries), and TweakXL changes
TweakDB. Their dependent mods take whichever renderer path they actually use.

Sources: [RED4ext](https://github.com/WopsS/RED4ext),
[redscript](https://github.com/jac3km4/redscript),
[Codeware UI](https://github.com/psiberx/cp2077-codeware/wiki#user-interface),
[Native Settings UI](https://github.com/justarandomguyintheinternet/CP77_nativeSettings),
[Mod Settings](https://github.com/jackhumbert/mod_settings),
[CET onDraw](https://wiki.redmodding.org/scripting-cyberpunk/cyber-engine-tweaks/cet-functions/events/ondraw),
[ArchiveXL](https://github.com/psiberx/cp2077-archive-xl),
[TweakXL](https://github.com/psiberx/cp2077-tweak-xl),
[WolvenKit editor](https://wiki.redmodding.org/wolvenkit/wolvenkit-app/editor).

## Common controls

New mod panels start in front of the player, anchored in the world. The preferred
laser hand follows weapon handedness; using the other trigger over the panel
transfers the pointer after releasing the first hand's controls.

| Input | Mod panel action |
|---|---|
| Laser + same-hand Trigger | Left click / activate; hold for dragging sliders |
| Same-hand Grip | Secondary/context click |
| Grip on the canvas's top strip | Move the panel without a stick chord |
| Pointer-hand stick up/down | Scroll |
| Opposite-hand stick left/right | Previous/next tab where the renderer exposes tabs |
| Either Menu, release | Cancel current edit/popup, then close the overlay |

Opening, changing hands, focus loss and closing release all input. Held buttons
must be released before they can activate newly opened UI. While an interactive
mod panel owns input, its controls cannot move, turn or shoot in the game. The
existing game-specific map/wardrobe/device actions remain in their ink adapter.
Keyboard text entry remains available; this does not introduce a VR keyboard.

## Interop contract

`include/Overlay/ModUiApi.h` is an optional, versioned C ABI discovered through
`CyberpunkVR_GetModUiApi`. Each renderer owns its ImGui version/context, drawing,
texture bindings and Lua runtime. No ImGui pointers or draw lists cross DLLs.
The VR host owns three leased D3D12 canvases per source. A producer acquires one,
clears and renders it, transitions it to PIXEL_SHADER_RESOURCE, signals its own
fence, and submits the lease. It must not reuse the texture after submission.
The host only samples completed producer work and holds the canvas until its
own GPU copy completes. A busy ring skips a frame instead of waiting on an
unbounded GPU fence. An absent/incompatible host ABI or absent VR keeps desktop
rendering. An incompatible device/format cannot publish a canvas; the adapter
skips that VR frame rather than drawing its smaller canvas across SBS. A missing
producer heartbeat or stale completed frame releases interaction ownership.

The canvas is 1440 x 1080, independent of the game's SBS render resolution.
One focused mod canvas is presented as a both-eye OpenXR quad; F10 takes priority
when open. This avoids arbitrarily spreading a single window across two eyes.

CyberFinger native ink menus and adapted mod canvases share F10's physical size
settings. The complete canvas is fitted into its width/height bounds while
preserving aspect ratio, after the native SBS layout calculation. At defaults
the bounds are 1.9065 x 1.429875 m (1.55 m base width at 1.23 scale), at 1.22 m
distance with zero horizontal/vertical offsets, matching the saved Quest Pro
setup. Fresh settings and Reset share these defaults; existing saved values
override them. This limits the whole native
canvas rather than only its internal 16:9 content region. Panel size changes
apply live; native/mod opening distance applies on reopening/recentering.
F10 also stays fixed in world position and orientation for CyberFinger. Native
menu tab/context changes and input focus loss preserve the anchor, as does brief
tracking loss. Other controller profiles keep their previous native/F10 behavior.
Gameplay HUD channels and in-world device surfaces are separate from modal UI.

Native modal menus submit a separate both-eye hand laser, using the exact
quad intersection that drives the native mouse cursor. This covers stock
Esc/inventory/map/journal menus and native mod editors such as PgUp's character
editor. It remains visible between controls and continues as a short aiming
ray outside the canvas. Desktop mouse ownership, joystick highlight focus,
tracking loss, closing the menu or another overlay taking input hides it.
The small static beam swapchain prefers the same sRGB format as F10/CET, with
a linear UNORM fallback when that is the only supported variant. Its RTV uses
the selected format too; it does not require both formats to be advertised.

## Coverage and limits

This is a renderer integration, not a promise that every custom widget supports
keyboard navigation or exposes a semantic tab bar. Standard ImGui tab bars can
be selected by the adapter; custom hand-drawn tabs remain laser-clickable.
CET's own toolbar uses window-toggle buttons, not ImGui tab bars. It therefore
uses laser clicks. A mod window drawn independently of CET's open/closed state
may need its own close button; the adapter cannot invent that mod's close callback.
CET's explicit open/closed state controls its entire VR surface. Closing CET
hides its canvas and strip and releases input even when a mod keeps an ordinary
mouse-capable window alive in `onDraw`. Background CET HUDs are therefore also
hidden while CET is closed in VR; independently persistent HUDs need a separate,
explicitly passive producer. A passive source cannot take focus away from an
interactive source. CET's Win32 hook yields when the host withdraws input focus,
and passes F10/Insert to the VR host. Non-VR CET behavior is preserved.
Native modal menus inherit the port's ink adapter. Arbitrary decorative ink HUD
widgets are not automatically made modal or moved off the HUD: doing so could
capture gameplay permanently. Such widgets require an explicit mod integration.
Character Preset Manager's native character editor and its ImGui windows follow
different paths and must both be tested.

Native-menu input suspension preserves the last tracking pose and world anchor;
the XInput overlay branch must use `SuspendInput`, not the menu-close `Reset`.
Physical mouse messages yield native laser ownership; generated laser messages
carry an extra-info tag and cannot claim it back. Deliberate Trigger/stick input
restores the controller. The owning-ImGui adapter similarly preserves deliberate
desktop mouse events, and CET feeds its Win32 backend the VR canvas dimensions.
CF pointer switching on optical tracking loss is disabled; an intentional other
Trigger press can still switch hands.

Stock CET does not implement this ABI; a matching CET renderer adapter is
required for external CET windows. This is a development integration, not a
claim of compatibility with every mod or a fix for the earlier CET watchdog hang.

## Preparing the CET adapter

`integrations/cet/prepare_adapter.py` reads a CET checkout and produces a unified
patch, patched source files, and original-file SHA256 manifest under this VR
checkout's `build` directory. It does not edit the input checkout and refuses
ambiguous source anchors or an existing output directory.

```powershell
python integrations/cet/prepare_adapter.py `
  --source C:/Users/bengu/src/CyberEngineTweaks `
  --output build/cet-vr-adapter-review
```

Apply the generated patch to a separate copy of the same CET revision, initialize
its pinned RED4ext SDK submodule, then build with CET's own xmake release recipe.
The adapter compiles against CET's ImGui and D3D12 backend. It changes the canvas
and resolution query, adjusts SBS-scaled text/style for the smaller panel, keeps
ImGui context access under CET's lock, and drains its own GPU work before reset.
It neither changes individual Lua mods nor imports the port's Lua runtime.

Installation needs **both** the updated VR plugin and the matching adapted
`cyber_engine_tweaks.asi`. Existing CET bindings, scripts, fonts and mods are kept.
The stock CET source and installed ASI remain unchanged during preparation.

## Validation

Installed on 2026-11-15 with the user's approval; **headset validation pending**:

- Full Release VR plugin build passes.
- Full CET release build passes against local base
  `20b42c69040b11eb8c676a4fc86dc4b6d1c55781` (`v1.38.1-11-g20b42c6`), using
  CET's own xmake recipe and pinned SDK. All seven generated adapter files match
  the compiled checkout. The original CET checkout remains clean.
- **41 tests pass:** 11 mod UI, 22 overlay, 8 CyberFinger. C11 ABI and CET adapter
  compile checks also pass. The real D3D12 GPU test uses WARP and the debug layer;
  it does not substitute for an AMD/Steam Link headset test.
- VR package: `dist/CyberpunkVRPort-0.2.3-mod-ui-r3` (corrects native suspension,
  pointer handedness and desktop mouse handoff).
- CET companion: `dist/CET-VR-ModUi-1.38.1-r2`, with patch and license files;
  corrects close/visibility lifetime and yields Win32 input to F10.
- Reviewed two-file install plan: `build/mod-ui-r3-install/plan.json`; installer
  preview: `build/mod-ui-r3-install/install.ps1 -WhatIf`. The original VR installer
  preview also reports only one VR-plugin file changed. No configuration,
  bindings, mods or driver changes are needed.
- Both installed binary hashes and both original-file backups were independently
  verified. All six VR INIs plus CET's bindings/config JSON are byte-for-byte
  unchanged (eight files). Backup and receipts:
  `build/mod-ui-r3-install/backup-dee0c9a74271414ab7a10a99b18aa676/`.

Automated coverage includes pointer hand selection, world anchoring, Grip move
versus secondary click, scroll/tab routing, Menu/chord isolation, focus/held-button
release, C ABI compilation, owning-ImGui tab selection, real D3D12 fence/pool
ownership, stale producers, and both-eye composition. The existing CyberFinger
and F10 regression suites are also used. Regression tests exercise the actual
CET adapter with an always-running mouse-capable ImGui window, close/reopen and
F10 input handoff, physical mouse/laser ownership, and XInput's suspension API.

Headset acceptance still needs CET console/settings, Character Preset Manager's
ImGui window and native editor, Native Settings/Mod Settings, changing dominant
hand, opening F10 over CET, save loading, non-VR fallback, and closing the game.
