# One plugin, no script frameworks: the plan (2026-10-30)

Goal: the port installs as ONE folder, `red4ext/plugins/cyberpunkvrport/`, needs RED4ext and nothing else, and does
not get in the way of the same frameworks when other mods bring them:

```
red4ext/plugins/cyberpunkvrport/
    cyberpunkvrport.dll
    shaders/    the shader twins and the port's own shaders
    grips/      the per-weapon grip and reload poses (CyberpunkVR_*Grip*.ini)
    archive/    cyberpunkvrport.archive (loaded by the plugin)
    config/     vrport.ini, vrport-launcher.ini, vrik_calibration.ini, the shipped UserSettings.json
    log/        cyberpunkvrport.log and every diagnostic dump
```

Each dependency goes in its own stage; each stage ships only when the port works as before with that framework
uninstalled, and still works with it installed (another mod may need it).

## What the port uses today

| Framework | What for | Size |
|---|---|---|
| Input Loader | one input context, `r6/input/CyberpunkVRPort_ScannerHud.xml` (the scanner HUD editor) | 1 file |
| TweakXL | 40 records (`mods/tweaks`): attachment slots of the belt, props, magazines, the lobby body, the cigarette | 780 lines YAML |
| ArchiveXL | `cyberpunkvrport.archive.xl`: 2 item factories, a streaming block merged into the main menu's world (the lobby), 6 mesh appearance patches (the construct), the left-handed animation sets on the player's root (759 entries) | 1 manifest |
| Codeware | from scripts: WorldWidgetComponent, DynamicEntitySystem / DynamicEntitySpec, ScriptableService, Reflection, game session events | ~30 call sites |
| redscript | 41 files: script-side hooks and helpers (`mods/redscript`) | 6.9k lines |
| CET | 25 Lua mods (`mods/cet`): the VR menu, reload, the tactical belt, the lobby, weapons, hand collision, wrist HUD, VRIK glue, ... and their ImGui windows | 70k lines Lua |

## Stages

0. **The folder.** `cvr::paths` (include/Core/Paths.hpp) owns every location: the plugin's own folder (from the
   module, not the exe) and its five subfolders. Everything that wrote next to the exe or next to the DLL goes
   through it. On the first start of the new layout, files of the old one (bin/x64 vrport.ini, the grips, the
   recordings; the old plugin folder's calibration and shader folders) move over once. A second copy of the plugin
   (an old `CyberpunkVR_Stereo` folder left behind) finds the first one's process-wide claim and stays inert. Build,
   deploy and packaging produce the new folder.
1. **Input Loader** -- done (Core/InputConfig.cpp). Input Loader hooks nothing: it merges XML and points the
   engine's two config variables (Player/Input InputContextFile / InputMappingFile) at the result through an ini it
   writes. The plugin merges `config/input/*.xml` into copies of `r6/config`'s two files under the same rules
   (pugixml) into `config/input/merged/`, and sets the two variables in memory through their own setter (vtable
   slot 3) at load, before the engine reads them -- no file outside the plugin's folder, nothing left behind.
   Measured: 16 bindings in 3.4 ms at start; a test file taking jump off the A button took it off, so the engine
   reads the merged files. With Input Loader installed its variables are left alone and the port's files go to
   `r6/input` for it; an ini Input Loader left behind after its removal is set aside (`.orphaned`).
2. **TweakXL** -- done (Core/TweakRecords.cpp). The same YAML ships in `config/tweaks/` and the plugin applies it
   right after the engine's TryLoadTweakDB (RED4ext hooking, address-library hash -- TweakXL's moment): `$base`
   copies every flat of the base (flat names from the record class's getters that the base holds, types from the
   flats themselves), `$type` (full or short name) gets default flats of that type, properties are written as
   flats of the type the record holds (scalars, structs, raRef, LocKey, arrays with !append / !append-once /
   !prepend / !prepend-once / !remove), names are registered with the engine (TDBID.ToStringDEBUG reads them).
   Records go a `$base` level at a time with ONE insert into the 2-million-id flat array per level (one per record
   cost ~3 ms each). YAML files parse in parallel. The scripted ScriptableTweak (launcher ready state) became a
   native tweak (Natives/LauncherReadyState.cpp, FlatWrites). Earlier releases' r6/tweaks copies are taken out.
   Measured: all 198 flats of 30 records and the 79 launcher-state flats equal TweakXL's own result (snapshots of
   both); 2.8 ms read (8.3 ms before the parallel parse) + 28.7 ms applied (TweakXL: ~28 ms for the same files).
   Found on the way: the SDK's CreateFlatValue MOVES from its argument -- FlatWrites hands it a copy.
3. **ArchiveXL** -- done (Core/ArchiveExtensions.cpp). `archive/cyberpunkvrport.archive` in the plugin's folder is
   mounted as the depot initializes its archives, in a mod group of its own; the manifest (ArchiveXL's format,
   yaml-cpp) ships as `config/archive.yaml` -- NOT beside the archive: ArchiveXL applies every `*.xl` under any mod
   group's folder, ours included, and did so twice in the first run. Applied by the plugin: the 2 item factories
   (after the game's last one), the 650 animation setups on 13 entity templates (pushed before the component builds
   its setup), the streaming block merged into the main menu's world (one more block reference holding the loaded
   block), the construct's appearances added to 6 body meshes with their external material (CMesh PostLoad). A patch
   resource starts loading with its target's request and its load job is JOINED to the target's deserialization
   job, so the target never finishes first (a 250 ms wait missed once). Two engine changes ArchiveXL makes for every
   mod came along, the port's assets having been made with them: world widget components past a layer's tenth are
   updated (the menu, markers and weapon screens are more), and an entity spawned from a record takes its
   appearanceName (the lobby's figures). Earlier releases' copies in `archive/pc/mod` are taken out at load -- before
   the depot lists that folder and before ArchiveXL looks for manifests.
   Measured: with ArchiveXL and EquipmentEx (whose script needs it) taken out, the save loads with no script errors,
   the belt is worn (factories), the lobby block and construct appearances are in, and the player's 152 animation
   setups equal ArchiveXL's in order, sets, priorities and variables (ArchiveXL leaves the variable names out of
   the name pool; same hashes). 7.3 ms to read the manifest at load + 1.1 ms to mount and prepare it at depot init
   (ArchiveXL: 161 ms to read the same manifest). With ArchiveXL installed both run side by side, each applying only its own manifests.
4. **Codeware** -- done (Natives/EngineObjects.cpp, Natives/SpawnSystem.cpp). Only what the port called, as natives
   of the plugin under the port's own names (`VR*`, so nothing meets Codeware's or another mod's), with the 24 Lua
   files and 3 scripts switched over; what 2.5 already has natively (CallbackSystem, ResourceEvent,
   ScriptableService, the inkLayerWrapper class) is used as the game has it:
   - entities: components, world transform; a component's resource changed or its appearance refreshed through the
     component's own load job (`VRComponentChangeResource` waits on the job when asked);
   - ink: the system's layers handed out as the game's own inkLayerWrapper, the controllers of the world's widget
     components (terminals, doors ...);
   - weather: set and read through the game's weather system;
   - spawning: `VRPortSpawnSystem`, a native game system (IGameSystem, inserted into the game instance like any of
     the game's) that makes an entity stub and registers it with the population system, from a record or from a
     template (its own SpawnableObject record, named by the path as Codeware names it); tags, lookup, delete; an
     entity that is no game object is entered in the runtime entity registry so FindEntityByID finds it;
   - the lobby's exposure: the area settings of the menu world collected through ResourceEvent callbacks.
   Native signatures: RTTI makes a composite type ("handle:inkLayerWrapper", "raRef:CResource") on first request by
   parsing its name's text, which it finds in the name pool by the hash -- a name nothing has put there is no type.
   Codeware, installed, had put the port's there by chance; without it `VRInkLayer` had no return type, QuickBoot
   never saw the loading screen and Continue was never pressed. The names now go into the pool first (RttiType),
   and every signature is complete at registration; one a script declares is left as the script loader made it.
   Measured: with Codeware, ArchiveXL, TweakXL and Input Loader taken out (and the scripts of other mods that need
   them: EquipmentEx, Nova Optics, General Shadows Fix, Decals Flickering Fix), the lobby comes up with its 10
   entities, exposure +0.5 EV on its 8 area settings and its weather, the save loads with no script errors, the ball
   spawns in play (~100 ms to FindEntityByID, polling included; right after a load the population queue takes up to
   15 s) and despawns, the world's widget controllers are found, the belt is worn, the 153 animation setups and the
   198 tweak flats equal stage 2/3's (the player's attachment slots lack only EquipmentEx's 54 OutfitSlots). With
   them all installed the same checks pass. Per call from Lua, against Codeware's methods: the player's components
   163 us (Codeware 169), the ink layers 12.5 us (14), one layer 3 us (4.5), the weather state 1 us (3.5).
   QuickBoot (the test launcher) loads the newest save the way an unmodded Continue does: Continue on a modded game
   raises a native warning no script controller answers, and a pressed button reached it only now and then.
5. **redscript** -- done (Core/ScriptCompiler.cpp). The scripts stay scripts, compiled by redscript's own compiler,
   which the plugin carries: scc_lib.dll built from redscript's sources (MIT, the release tag pinned in CMakeLists,
   cargo with the plugin's build) into the plugin's `redscript\` folder, the scripts in its `scripts\` folder.
   Rewriting them instead -- 41 files, ~150 wrapped and 10 replaced game methods, ~200 added methods, ~170 added
   fields -- would have meant re-implementing redscript's method wrapping on the engine's bytecode for each one, for
   no gain: the methods they hook run on the game's script thread whoever compiles them, and what is heavy is native
   already. How the engine compiles scripts: Scripts/EnableCompilation (redscript's installer sets it in
   engine\config\base\scripts.ini) makes it start engine\tools\scc.exe; RED4ext's hook on that start runs the
   compiler in process from the scc_lib.dll beside the path, with every plugin's script folder (`Scripts::Add`), and
   points the engine at the result. So:
   - redscript installed: the port's folder is added and redscript's compiler compiles everything -- nothing of the
     plugin's runs;
   - not installed: the plugin sets EnableCompilation in memory at load (an ini naming it still decides) and puts
     its own `redscript\scc.exe` path in place of the engine's when the engine starts the compiler; RED4ext then
     compiles with the plugin's scc_lib.dll, r6\scripts and every plugin's scripts included, as redscript would.
   Copies an earlier release put in r6\scripts\CyberpunkVRPort_* are taken out at load (same bytes deleted, the
   rest renamed .legacy): compiled beside the plugin's they would define everything twice.
   Measured: with redscript taken out too (and the frameworks of stages 1-4), the plugin's compiler compiled 45 files
   in 171 ms with no errors (redscript 1.0.0-preview.22), and the stage 4 checks pass as before.
6. **CET** -- done (src/Lua); moving the heaviest per-frame Lua native is what is left of it. The Lua mods run in
   the plugin's own Lua state, with the CET API they call. The largest stage by far. What the 397 files (70.5k lines) use:
   plain Lua 5.1 (no ffi, bit or jit); events onInit / onUpdate / onShutdown / onDraw / onOverlayOpen /
   onOverlayClose; 54 Observe / ObserveBefore / ObserveAfter / Override on ~25 game classes; ~615 `Game.*` calls (the
   game's and the plugin's global functions, ScriptGameInstance's getters); the RTTI bridge on every game object
   (methods, properties, statics, enums, handles, arrays, CName / TweakDBID / Vector4 / Quaternion / EulerAngles),
   NewObject, GetSingleton, IsDefined, ToVector4, FromVariant, TweakDB / TDBID, GetMod, json, spdlog, io; ~300
   ImGui calls (the settings windows); 6 registerHotkey. CET (MIT) is the reference for all of it, and its
   semantics are what 70k lines rely on, so its scripting core is ported rather than re-invented -- pinned at
   v1.38.0, the version the port is tested with:
   - 6a runtime: LuaJIT 2.1 (openresty, v2.1-20250826, gc64) built from source with CMake (luajit-cmake; its host
     tools are built locally), sol2, spdlog, nlohmann json, tsl hopscotch-map, xbyak, TiltedCore -- all fetched at
     pinned tags. Verified: LuaJIT builds as a static library this way.
   - 6b-6e -- done (src/Lua). CET's scripting core (src/Lua/cet: reverse\, scripting\, sol_imgui\, VKBindings,
     Utils; README.md there lists what changed) runs with the plugin as its program (src/Lua/Host.cpp): the mods in
     the plugin's `lua\`, one per folder with its own environment, require path and log; the host's logs in
     `log\lua\` (flushed every second); binds in `config\lua_bindings.json`; CET's json module embedded; the
     TweakDB's record names (`config\tweakdb`, WolvenKit's resource files, GPL-3.0 data with their licence,
     downloaded at build time from a pinned commit -- the files CET ships) for TDBID.ToStringDEBUG and the TweakDB
     API. Not carried, nothing of the port uses them: textures, sqlite, toasts, CET's own overlay and console, CET's
     log natives. The lifecycle is CET's own hooks (script load, TweakDB load, player spawn, the main thread's
     states); an exception out of the Lua side stops the host, logged, and the game runs on. The mods' windows
     (onDraw) are drawn in the plugin's desktop ImGui frame (ImGui 1.91.1-docking, the version CET's bindings are
     made for) while their overlay is open, and only then: CET draws every frame and every port mod returns at
     once while the overlay is closed, so the render thread no longer takes the Lua state each frame. The overlay
     key is CET's (taken from CET's bindings.json; Home without CET); open, ImGui draws the pointer and has the
     mouse and keyboard.
   - Next to an installed CET (it keeps running other mods): the engine hooks chain; Observe/Override's hook on
     rtti::Function::InternalCall goes in with the host's first override, after CET's, so it is the outer one and
     hands on every native-marked function it does not own -- the "lobby never appears" hang of the first run was
     this hook refusing CET's overrides. A function both hosts override chains in either order (InternalCall runs
     the bytecode it is given and every replacement keeps the original's; the pure call goes on to the function
     itself, which the other host's hook knows by that address); verified with a probe mod: every call reached
     both hosts' observers, with CET's override under the host's and over it. ToStringDEBUG is answered by one
     host (CET's when installed: its entry is already a jump); exEntitySpawner and AddToInventory are not made
     twice.
   - 6f migration: all 24 port mods moved at once (they reach each other with GetMod), deployed to `lua\`. At load
     the plugin takes an earlier release's copies out of CET's mods folder (src/Lua/Migration.cpp): the same bytes
     deleted, the player's data moved into the plugin's copies (files written at run time, and data files that
     differ from the shipped default: settings), an older script and CET's own files (logs, the per-mod sqlite) kept
     in `legacy\cet\<mod>`; the first time, the binds come from CET's bindings.json.
   Verified, with CET installed and with it taken out (its .asi moved aside): the lobby (10 entities), the save,
   the basketball, the belt, QuickBoot, record names from the plugin's own files, the TweakDB API, the overlay key
   (raw input) opening and closing the mods' windows; no Lua errors. Found on the way: the basketball's lost-ball
   recall respawned a fresh ball every tick -- a ball the population system has just entered reads the world
   origin until its transform lands -- now nothing of a ball counts until it is where it was put
   (vrport_basketball.reds); under CET the mod's update had failed earlier in each frame (a parameter error since
   fixed), which hid it.
   Measured, A/B on the same save, 60 s settled, the mods in CET vs in the plugin's host (tools/lua_host_tests/
   frame_cost.py): GameThread 21.74 -> 21.31 ms per frame, frame mean 22.34 -> 22.23 ms, frames over twice the
   median 4 -> 0 (worst 142 -> 39 ms). The host's onUpdate was 6.1-6.6 ms per tick in the city (its summary in
   the plugin's log every 30 s, per mod): Reload 2.7, Stereo 1.2 (the GC steps it takes), Weapon 0.42, TacticalBelt
   0.38, Optics 0.2. With CVR_LUA_PROFILE in the game's environment the host also runs LuaJIT's sampling profiler
   (src/Lua/Profiler.cpp: the hottest lines, the VM's states, each overridden function's calls and time per tick).
   It found Reload's 2.7 ms: its glow walked every component of the player through Lua each frame for a carrier
   this player's template does not have; by the engine's own name lookup, a miss asked again every 2 s, it is
   0.3 ms. onUpdate 6.2 -> 3.7-4.3 ms per tick, GameThread 21.31 -> 20.86 ms per frame (Lua is partly hidden behind
   the main thread's wait for the engine's jobs, as measured before). 84-95% of what is left is calls into the game
   (the RTTI bridge and the game's own code), which belong on the main thread; the overrides of the state
   machine's GetActionValue / IsActionJustPressed (Swimming, Ladder) are 9 calls and 0.26 ms a tick. The main
   menu's lobby build is one 650 ms tick (Lobby's world builder).

## Measured per stage

Start-up time to the main menu, frame time in the city (the usual AutoSave street run), the port's own CPU time
per frame (`CyberpunkVR_Debug*` timing exports), and the log's error count -- before and after, with the framework
removed.
