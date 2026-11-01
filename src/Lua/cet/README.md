# CET's scripting core, in the plugin's Lua host

The files here are Cyber Engine Tweaks' scripting core -- `src/reverse`, `src/scripting`, `src/sol_imgui`,
`Utils.*`, `VKBindings.*`, `common/Logging.h`, `common/Meta.h`, `common/ScopeGuard.h` -- from
[CyberEngineTweaks](https://github.com/maximegmd/CyberEngineTweaks) **v1.38.0** (commit `ed706fb8`), MIT licensed
(`LICENSE.txt`, copyright yamashi and contributors). The port's Lua mods were written against it; running them on the
same code keeps every semantic they rely on (the RTTI bridge, Observe/Override, the sandbox, the events).

What is the plugin's instead of CET's own program (stage 6 of `docs/native-frameworks-plan-20261030.md`):

| CET | here |
|---|---|
| `CET.h` / `CET.cpp`: the program owning everything | `CET.h` facade, answered by `src/Lua/Host.cpp` |
| `Paths`: `bin\x64\plugins\cyber_engine_tweaks\...` | the plugin's folder: mods in `lua\`, bindings and state in `config\`, logs in `log\lua\` |
| `Options`: `config.json` | the two developer flags the core reads, fixed |
| `overlay/`: CET's ImGui overlay | the plugin's overlay; `Overlay.h` is its open/closed state, `widgets/Bindings.h` the one bind VKBindings asks for |
| `d3d12/`: CET's D3D12 hooks | the output resolution only (`d3d12/D3D12.h`) |
| `config/CETVersion.h`: generated | fixed to 1.38.0 |
| `stdafx.h` | the same libraries, against the plugin's RED4ext.SDK and ImGui (1.91.1-docking, CET's) |

Changes to the copied files themselves, each for the same reason -- nothing of the port uses it:

- no textures: `scripting/Texture.*` removed, `ImGui.LoadTexture` and its binding gone;
- no sqlite: the `sqlite3` module and the per-mod `db` are not made (`InitializeDBForSandbox` is empty);
- no ImGui toasts (`BindImNotifyToast`), no unsaved-changes popup (`UnsavedChangesPopup`);
- no `RenderContext` (CET's D3D12 hooks);
- `LuaVM::Hook` keeps the hooks that call the game's function on (TweakDBID derivation, script load, TweakDB load,
  player spawn, the main thread's states) and `ToStringDEBUG` (the port's mods match records by name), which it
  replaces only when no other host has (an installed CET hooks it at its start: the entry is then a jump); CET's
  hooks on the script log natives replace them outright and are not carried;
- `scripting.log` and `gamelog.log` are written to `log\lua\`;
- an exception out of the Lua side (`LuaVM`'s `Guarded`) stops the host, logged, instead of reaching the game;
- `LuaVM::Update` is timed, and `ScriptStore` times each mod's onUpdate: a summary every 30 s in the plugin's log;
  with CVR_LUA_PROFILE in the game's environment, LuaJIT's sampling profiler runs too (`src/Lua/Profiler.cpp`,
  started by `LuaVM::PostInitializeMods`, the running mod set by `ScriptStore`) and its hottest lines join the summary;
- `LuaVM::Draw` is called by the plugin's overlay only while the overlay is open (CET draws every frame; every
  port mod's onDraw returns at once while it is closed);
- the binding's per-call costs (2026-11-01; GameThread samples at Megabuilding H10 put 85 % of the mods' Lua time
  in the binding, the collector and the VM, and 15 % in the game's own functions):
  - `RTTIHelper::GetProperty` / `SetProperty` look properties up through a cache per class and name hash
    (`FindProperty`), misses included: every `obj:Method()` asks for a property of that name first, and a miss
    walked all the properties of the class and its parents;
  - `ResolveFunction` takes a class's hash from its CName instead of hashing the name pool's text again, and
    reads the resolved-function cache without copying the function (a registry reference per copy);
  - `ClassType::Index_Impl` no longer stores each resolved method on the instance as well: RTTIHelper keeps it
    per class;
  - `Type`'s map of script-set fields is made on the first field set: every game value handed to Lua is a
    `Type`, and an empty MSVC `unordered_map` still allocates its list head and buckets.
  Host summary at Megabuilding H10, onUpdate per tick with the weapon holstered: 4.6-5.0 ms before, 2.6-3.0 ms
  after (two launches; the second also had the Stereo mod's cheaper world-touch scan);
- the collector is the host's to pace in the game (`src/Lua/HostGc.cpp`, `IdleGc.c`): `LuaVM::PostInitializeMods`
  registers the state, and `LuaVM::Update` runs the finalizers before onUpdate (`CvrLuaGcSafePoint`); the rest of
  the collector's work runs while the game thread waits for the world tick. Same launch, holstered: onUpdate
  3.24 -> 2.62 ms per tick, p99 11.4 -> 8.0 ms, max 20.6 -> 10.7 ms.

Next to an installed CET (both hosts in one game):

- the hooks that call the game's function on chain, and each host loads its own mods;
- `FunctionOverride` hooks `rtti::Function::InternalCall` only with its first override, after CET's hook (made at
  CET's start), so it is the outer one: a native-marked function it does not own goes on to CET's hook. A function
  both hosts override chains: the pure call goes on to the function itself (CET's hook knows it by that address), and
  `Clear` leaves this host's replacement in place, emptied, when CET's override sits on top of it;
- `exEntitySpawner` and `AddToInventory`, already made by CET's script-load hook, are not made twice.
