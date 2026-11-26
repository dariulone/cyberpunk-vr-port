# Эталон переноса на Rust (этап 0)

Зафиксирован 7 ноября 2026 года до первого переключения реализации на Rust. Всё, что дальше переносится, сверяется с этим
состоянием. [План переноса](../rust-migration-plan.md)

## Исходники

| Что | Значение |
| --- | --- |
| Коммит | `e53e511f` (Walkable mirror) |
| Локальные изменения поверх него | roomscale, поворот и наклон корпуса, приседание, VRIK и их тесты: `include/Anim/BodyBend.hpp`, `include/Camera/AnchorTranslation.hpp`, `include/Hooks/RoomscaleMove.hpp`, `include/Runtimes/{HybridBodyYaw,RoomscaleMovement,TrackedBodyYaw}.hpp`, `src/Anim/CharacterRig.cpp`, `src/Hooks/{AnimPose,BodyYawFollow,RoomscaleMove}.cpp`, `src/Runtimes/{OpenXRManager,PhysicalCrouch}.cpp`, `tools/{body_rotation_tests,roomscale_tests,vrik_tests}/*`, `tools/sim_harness/simxr.py` |
| Патч локальных изменений | `build/rust/baseline/local-changes.patch` (вне Git, это незакоммиченная работа), SHA-256 `8c89d88da96de4a3a0810b73f65d3fe971a94b98e4e33e344a37bcb62a9f9f18` |
| RED4ext.SDK | `build/patchday/framework-src/RED4ext.SDK` `02a7cffd` (три коммита 2.5 поверх `d02a1f32`) |
| Сборка C++ | Visual Studio 18 2026, Release, `/Zi /DEBUG /OPT:REF /OPT:ICF` |
| Rust | 1.93.0 `x86_64-pc-windows-msvc` (`rust/rust-toolchain.toml`) |

Эталонная DLL без Rust собрана из того же состояния (worktree `build/rust/ref-src` на `7ae067a5` с патчем локальных
правок, `-DCVR_WITH_RUST=OFF`, `build/rust/cpp-reference-build`) и сохранена с PDB:
`build/rust/baseline/cyberpunkvrport.cpp-reference.{dll,pdb}`, SHA-256 DLL
`66af85cbf82baedb9e2213eb4208dc1b8f0c532a31a475c476fae63a58db480c`, 2841 экспорт. Её используют A/B-прогоны и откат.

## Бинарники и окружение

| Файл | SHA-256 |
| --- | --- |
| Установленная до переноса DLL (сборка 05.11 15:15, `e53e511f` + локальные правки того дня) | `8f5f1bb851b3e7e216eae2d4da4413aafe3183f5f22563e088c0164d3e31a26c` |
| Релиз 0.2.2, `dist/CyberpunkVRPort-0.2.2/.../cyberpunkvrport.dll` | `e26bf501f359ffc367f173c3087933e710b01a79686d8a3d247ab8be3b2e229a` |
| Этап 1а: DLL с `cvr_rust.lib` (обработка стиков на Rust) | `51fd5fbc1fa12ef01191874242513d3513419012029493f7e417ccfed6be08dd` |
| `Cyberpunk2077.exe` 3.0.5813403 (игра 2.5) | `d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4` |
| `RED4ext.dll` 1.31.0 | `58ddfe43766aa91249d11671b55a8323d5ec017f816d847affe4b6201dcd0709` |
| OpenXR Simulator (`Desktop\OpenXR-Simulator\bin\openxr_simulator.dll`, активный рантайм) | `f2935602c51f6a362f51c32cf4c0a06e612df0be3e2914cb10af0ea218831e00` |

Установка игры чистая: RED4ext и порт, без CET и redscript. Скрипты порта компилирует его собственный `scc_lib.dll`.
Профиль симулятора — окно 1920×1038, рантайм без экспортов `SimXR_*`: ввод контроллеров идёт через `PadGate`
(`tools/roomscale_tests/simulator_input.py`).

## Контракты

| Файл | Что в нём | Как снят |
| --- | --- | --- |
| [exports-baseline.txt](exports-baseline.txt) | 2818 имён экспорта PE установленной DLL 05.11, включая переменные `CyberpunkVR_*`, `Main`, `Query`, `Supports` | `cvr-tool pe-exports` |
| [natives-baseline.txt](natives-baseline.txt) | 387 script natives порта с параметрами, типами и флагами, как их видит RTTI запущенной игры | дамп `cvr_rust_probe` с портом минус дамп без порта |
| [hooks-baseline.txt](hooks-baseline.txt) | 53 строки журнала об установке хуков (этап, имя, результат) | журнал порта, прогон с портом |
| [package-0.2.2.tsv](package-0.2.2.tsv) | 1210 файлов пакета 0.2.2: путь, размер, SHA-256 | `cvr-tool manifest dist/CyberpunkVRPort-0.2.2` |

Из 387 natives 386 названы в `src/Natives/Registration.cpp`, `src/Fbt/Game/Natives.cpp`, `src/Natives/ActionReads.cpp`
(`cvr-tool natives`). Последний, `AddToInventory` (exec), регистрирует встроенный Lua-хост.

Рабочая копия с локальными правками экспортирует 2841 имя: на 23 переменные `CyberpunkVR_*` больше из правок roomscale и
корпуса. Rust-код экспортов DLL не добавляет: его функции линкуются статически.

## Сценарии

| Сценарий | Как запускается | Результат 07.11 |
| --- | --- | --- |
| Загрузка последнего сохранения в VR | `tools/quickboot_tests/launch_to_save.py` через симулятор | мир готов, HUD стабилен, 1 автоподтверждение экрана загрузки |
| Левый стик через XInput-путь порта (этап 1а) | `PadGate.stick`, позиция игрока из `CyberpunkVR_PlayerEntityPos`, 2 с на каждый шаг | покой 0 м; 0,10 (внутри мёртвой зоны) 0 м; вперёд 3,105 м; назад 3,093 м; половина хода 3,098 м (режим по умолчанию фиксированный); вправо 3,042 м |
| Только Rust-проба, порт отключён на запуск (этап 1б) | `cvr_rust_probe.dll` в `red4ext\plugins\cvr_rust_probe`, игра до главного меню, `WM_CLOSE` | RED4ext загрузил пробу; native зарегистрирован в окне post-register; скрипт пробы скомпилирован её `scc_lib` за 106 мс; главное меню вызвало `CVRRustProbe(41) -> 42`, `CVRRustProbe(42) -> 43`; выгрузка записана; **код выхода 0, отчётов о сбое нет** |
| Порт и проба в режиме дампа | `CVR_PROBE_DUMP_ONLY=1`, загрузка сохранения | 1049 native-функций в RTTI против 663 без порта |

## Известные сбои до переноса

- **Сбой при выходе с портом.** `EXCEPTION_ACCESS_VIOLATION`, чтение по адресу `0x250` в `Cyberpunk2077.exe+0x1C2F8F`,
  после строки журнала `=== CyberpunkVRPort red4ext plugin unloaded ===`. Повторялся 05.11 (15:04, 15:13, 15:28, DLL
  `8f5f1bb8`) и 07.11 (три запуска, DLL с Rust и без изменений в этой части). Без порта (только Rust-проба) игра выходит с
  кодом 0. Кадров порта в просмотре стека нет: порт оставляет состояние, на котором падает завершение игры. Это не
  регрессия переноса; «корректный выход» для полной Rust DLL сверяется с этим фактом.

- **C++-тесты этапа 3 на рабочей копии** (собраны в `build/rust/tests/*`, Visual Studio 18 2026, Release):
  `body_rotation_tests` 42 из 42; `roomscale_tests` 54 из 55 — `native_hook_abi` (`verify_game_sites.py`) сверяет EXE
  с хешем старой сборки RE и падает на 2.5; `vrik_tests` 62 из 63 — `vrik_launcher_pose` падает с «cyberware stopped
  sharing approximately one quarter of wrist roll» (локальные правки `src/Anim/CharacterRig.cpp`), `vrik_hand_gestures`
  проходит один, но один раз упал при `ctest -j 8`. Rust-версии этих подсистем сверяются с C++ на тех же входах, а не
  с ожиданиями этих двух тестов.

## Не закрыто на этапе 0

- Разброс времени кадра эталона повторными прогонами `tools/vr_bench` ещё не снят.
- Аппаратная приёмка на Pico 4 и Quest 3 (VDXR, Steam Link) — без доступа к шлемам; все прогоны выше сделаны на
  OpenXR Simulator.
