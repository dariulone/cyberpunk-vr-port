# Overlay audit — 17.11.2026

## Найденный и исправленный дефект цепочки хуков

`src/Hooks/DepthCapture.cpp::PatchVtableMethod` запоминал первый original для `(vtable, slot)`, но повторно переписывал slot, если позже поверх порта устанавливался сторонний hook. Последовательность:

1. Игра → port → native.
2. Overlay устанавливается позже: игра → overlay → port → native.
3. Новый command list / повторный `Install*`: port переписывает entry, продолжая использовать первый original. Overlay исключён из цепочки.

Это воспроизведено C++ программой, компилирующей **исходную production функцию**: до повторной регистрации счётчики native/port/overlay = 1/1/1; после = 2/2/1. Scratch reproducer находится в `build/goal-weapons-20261117/overlay-chain`.

Исправление: зарегистрированный `(vtable, slot)` больше не патчится повторно. Поздний overlay сохраняет entry, port сохраняет свой первый continuation; не возникает ни потери overlay, ни рекурсии `port → overlay → port`. Чтение methodSlot перемещено под writer mutex. Незаписанный original при entry, уже равном нашему hook, отклоняется.

`tools/overlay_hook_tests` компилирует production `PatchVtableMethod`, извлечённый из текущего исходника, и проверяет реальными вызовами цепи: overlay установлен раньше/позже, 100 повторных регистраций, missing original. 4/4 tests passed. Это подтверждение сохранения цепи в воспроизведённом сценарии, а не доказательство отсутствия любых runtime crash конкретного overlay.

```
cmake -S tools/overlay_hook_tests -B build/goal-weapons-20261117/overlay-hooks
cmake --build build/goal-weapons-20261117/overlay-hooks --config Release
ctest --test-dir build/goal-weapons-20261117/overlay-hooks -C Release --output-on-failure
```

Не менялся выбор HWND в `HookedPresent`: сейчас исключены только окна класса `CyberpunkVR*`. Чужая secondary swapchain теоретически может перепривязать `g_gameHwnd`, но без конкретного ownership condition менять поведение настоящего swapchain/resize нельзя. Это отдельная гипотеза для live, не доказанный дефект этого прогона.

## Фактическое окружение baseline

Read-only инвентаризация registry и модулей текущего `Cyberpunk2077.exe` pid 22188, без attach/inject:

| Слой | Registry | DLL в процессе |
|---|---|---|
| Quest3ObsOverlay | implicit, enabled | `QuestOverlayLayer-c19ae2be556a9d56.dll` |
| Virtual Desktop compatibility | implicit, enabled | `openxr-oculus-compatibility.dll` |
| OpenXR OBSMirror | HKCU implicit, enabled | `XR_APILAYER_NOVENDOR_OBSMirror.20e831224c84.dll` |
| Nsight Graphics nomad | registered, enable_environment gated | не обнаружен |
| Nsight ngfx capture | registered, enable_environment gated | не обнаружен |
| RTSS / Discord / NVIDIA Overlay / OpenXR Toolkit | не обнаружены в обычных install dirs/registry | не обнаружены по именам |

Runtime — `openxr_simulator.dll` из OpenXR-Simulator-v1.5.0-win64. Следовательно baseline включает два OBS API layer и VD compatibility; наличие SDK registry entry само по себе не доказывает загрузку Nsight.

Для отдельного запуска без этих трёх implicit слоёв manifest задаёт process environment: `DISABLE_QUEST_OBS_INPUT=1`, `DISABLE_XR_APILAYER_NOVENDOR_OBSMirror=1`, `DISABLE_XR_APILAYER_VIRTUALDESKTOP_OCULUS_COMPATIBILITY=1`. Агент **не изменял** ни environment игры, ни registry. Сравнение определяет root в общей последовательности тестов.

## Источники установщиков

Локально имеется MSI Afterburner 4.6.6 и `C:\Program Files (x86)\MSI Afterburner\Redist\RTSSSetup.exe`, 18,078,504 bytes. Authenticode status Valid, signer MICRO-STAR INTERNATIONAL CO., LTD. SHA256 `04F4704CED86A68B650BF87156C02E69160E6C049B576661250250BEEABAA5EF`. MSI официально описывает установку RTSS вместе с Afterburner или отдельно с Guru3D: [руководство MSI](https://us.msi.com/support/technical_details/vga_msi_utility_afterburner), [страница разработчика RTSS](https://www.guru3d.com/download/rtss-rivatuner-statistics-server-download/). Использовать уже проверенный локальный installer после baseline.

OpenXR Toolkit official GitHub latest API возвращает 1.3.2, `OpenXR-Toolkit-1.3.2.msi`, 5,128,192 bytes. [Release автора](https://github.com/mbucchia/OpenXR-Toolkit/releases/tag/1.3.2), [MSI](https://github.com/mbucchia/OpenXR-Toolkit/releases/download/1.3.2/OpenXR-Toolkit-1.3.2.msi). Страница release сообщает о прекращении поддержки с 2024 года и известных проблемах новых игр. Это причина специально проверять совместимость, не считать установку доказательством работы. GitHub Pages документация вернула 404; API metadata сохранена в `build/goal-weapons-20261117/toolkit-release.json`.

Для NVIDIA UI: Settings → Features → In-Game Overlay, Alt+R statistics, Alt+Z configuration ([официальная поддержка NVIDIA](https://nvidia.custhelp.com/app/answers/detail/a_id/5084)). Для Discord: Settings → Overlay; при crash важен `%appdata%\discord\0.0.***\modules\discord_hook\hook`, а видимость нужно проверять в windowed/borderless ([официальная поддержка Discord](https://support.discord.com/hc/en-us/articles/217659737-Game-Overlay-101)).

Live проверка этих четырёх пользовательских overlays, их накладные расходы и совместимость с OpenXR/SteamVR ещё не выполнены этим агентом. Коммитов нет.

## Попытки запуска остальных overlays после baseline

Root не имеет elevated token. RTSS signed installer `/S` запросил UAC; процесс запуска отменён без выдачи прав. Официальный payload извлечён: сам RTSS.exe также требует administrator. Манифест/подпись не изменялись, RunAsInvoker не применялся; полноценный RTSS в этих правах пока недоступен.

NVIDIA App 11.0.9.585 скачан по ссылке официальной страницы NVIDIA: 184718176 bytes, SHA256 `3B5B9E941BD201AF8C577EEAF0C20EE6CBF1B0B920DFD277C68370ED61F37788`, Authenticode Valid / NVIDIA Corporation. Installer manifest требует administrator; он не запускался, драйвер не менялся.

Discord 1.0.9268 скачан из официального latest x64 endpoint: 160900024 bytes, SHA256 `0EC04C6E5439BFC1032D0C133A252936833CE777BFC24EDDB25E947020142315`, Valid / Discord Inc., asInvoker. Per-user silent install завершился exit0. Запуск напрямую и через штатный Update.exe --processStart Discord.exe оба достигли собственного fatal Javascript dialog: `(InconsistentInstallerState) Attempt to install host that is currently running`. Это произошло при закрытой игре; совместимость overlay с портом этим не проверена. Ошибка прочитана через UI accessibility. Запущенные тестом Discord-процессы закрыты. Доступ к аккаунту не запрашивался, сообщения никому не отправлялись.

## Toolkit: подготовлен per-user layer, исходно выключен

После baseline root поручил подготовить Toolkit без системной установки. Официальный MSI `scratch/goal-audit-20261117/OpenXR-Toolkit-1.3.2.msi`, SHA256 `875A9E97B4E46A16FA4021409EC284561884B9B4334AAEAA11FCD49835F97CC1`, прочитан через `WindowsInstaller.Installer.OpenDatabase(..., 0)` (read-only). Его embedded cabinet распакован 7-Zip; имена и каталоги восстановлены по File/Component/Directory tables, никаких custom actions/MSI install не исполнялось.

Результат: `scratch/goal-audit-20261117/toolkit-user/`. Все 44 payload entries сверены по FileSize и SHA256; у MSI две пары .NET DLL с одинаковым конечным путём, поэтому получилось 42 уникальных файла. Для `System.Net.Http.dll` и `System.Runtime.InteropServices.RuntimeInformation.dll` выбраны версии 4.8.9037 вместо 4.7.3062; все исходные 44 файла сохранены отдельно в `toolkit-cab-files`. Нативный layer этих .NET assemblies не использует. Mapping и hashes: `toolkit-payload-map.json`, исходные таблицы: `toolkit-msi-tables.json`.

Официальный manifest сохранён без изменений: `toolkit-user/XR_APILAYER_MBUCCHIA_toolkit.json`, относительный library_path разрешается в соседний DLL. SHA256 layer DLL `4f8b539013689d6e78a77f08eb02caf3ee879518c1440c8bea34bead9bde3b56`. `dumpbin /dependents` проверен; aSeeVRClient.dll, jsoncpp.dll и libzmq-mt-4_3_3.dll находятся рядом. Все четыре нативных DLL имеют PE machine AMD64. MSVCP140.dll, VCRUNTIME140.dll и VCRUNTIME140_1.dll присутствуют в System32.

Регистрация выполнена только в `HKCU\SOFTWARE\Khronos\OpenXR\1\ApiLayers\Implicit`: имя value — абсолютный путь указанного manifest, REG_DWORD **1 (disabled)**. Это документированный per-user механизм [Khronos OpenXR loader](https://registry.khronos.org/OpenXR/specs/1.0/loader.html). Состояние всех прежних values и путь manifest сохранены **до записи** в `scratch/goal-audit-20261117/toolkit-hkcu-before.json`; остальные values не изменены. Для planned test root может сменить только этот DWORD на 0, затем вернуть 1. Для полного возврата удалить только новый value, если он отсутствовал в backup; не удалять весь registry key.

Игра, Toolkit companion и layer не запускались этим агентом. Загрузка через OpenXR и overhead ещё требуют live-проверки root. Системные права не запрашивались, UAC/manifest приложения не обходились. RTSS требует administrator по наблюдению root; его отсутствие в runtime нельзя выдавать за пройденную совместимость.
