# Аудит цели от 17 ноября 2026

Цель активна. Это журнал доказательств и оставшейся работы, не заявление о завершении.
Источник требований: attachment `e8f4d1b1-3071-452a-97dc-1fdf946fa2ef/goal-objective.md`; повторно прочитан указанный пользователем `22035723-00b2-4c20-bc3c-96b40bd79764/goal-objective.md`, те же14 пунктов.
Секрет Telegram в репозиторий не копируется. Промежуточные сообщения отправляются авторизованному пользователем боту.
Исходная ревизия `7d671e5d`; до работы был изменён только HandCollision/init.lua.
Исходный diff сохранён в `scratch/goal-audit-20261117/corpse-preexisting.patch`.

## Матрица требований

| № | Требование | Текущее доказательство / что осталось |
|---|---|---|
| 1 | VRAM, CPU по потокам, GPU, Lua, framegen; OpenXR и SteamVR null | OpenXR: C++ process/thread/WDDM, GPU timestamps, корректные PDB/raw stacks, FGoff/on,20 native map и12SAO циклов, движение/мотоцикл. Несколько retention/ownership/overhead fixes проверены. Отдельная пара768²/1024² всёещё +6.25MiB/map; HUD0 это не устраняет. Native factory origin hook готов, владелец ещё не доказан. SteamVR gameplay ещё впереди. |
| 2 | Heywood Hurricane через быстрые триггеры dual wield | Live PID20040 PASS: Nue×2 дал после release ещё6 автоматических выстрелов3L/3R, Nue+Overture ещё5 (2L/3R), actual weapon-instance path4. Обычное удержание0.65с по одному выстрелу без Frenzy. До исправления NoAnims wrapper эффект был активен, но автоматических выстрелов не было. |
| 3 | Issue 233, чёрное изображение | Найден capture starvation; regression проходит. Точный пользовательский VDXR случай ещё не воспроизведён. |
| 4 | Toolkit, RTSS, NVIDIA, Discord и другие overlays | Late vtable chain fix и4testsPASS;2OBS+VD слои реально загружены в игровых сеансах. Toolkit1.3.2 подготовлен per-user, disabled, live ещё нужен. RTSS/NVIDIA требуют admin и не активированы; Discord updater не стартовал даже при закрытой игре. Эти попытки не compatibilityPASS. |
| 5 | Glamdoc в SAO | Native catalog/buy/apply/colors/preview/cancel/remove реально проверены; два обнаруженных layout/cancel defects исправлены.3otherarmtypes functionalPASS/0purchases, но cross-arm visual fixture был некорректен, visualPASS не заявлен. Original quest dialogue и физическийpointer остаются границами; все testitems/facts/money восстановлены. |
| 6 | Waypoint eye mismatch | Production parity13/13 + три native mappins в5живыхposes (yaw±15/roll±10), L/R/SBS/trace; opposite movement вSimulator не воспроизведено. SteamVR/Quest2+VD отдельно. |
| 7 | SteamVR FOV и resolution mismatch | Точные frusta из #243 в tests; выявлена разница runtime recommendation и launcher render size. Нужны измерения runtime углов/размеров при изменении SteamVR настройки. |
| 8 | Issue 243 и приложенный log | 172 rate windows,62 zero-picture; кодтерял готовыйpredecessor приGPUbacklog. Исправлены selection/ownership/origin checks, blocked-fence regressionsPASS; обычныеliveFG/traffic/bike окна не дали новыхempty submissions. ExactSteamFrame/сильнаяGPUpressure отдельно; RenderDoc replay сживойигрой не запускался. |
| 9 | Reload highlight после holster | Удалено stale удержание оружия на30 кадров; unit+live PASS: carrier выключен в первом sample с пустыми hand slots, tickdelta0. Есть stereoscopic glow-holster.mp4. |
| 10 | Меши прицелов в ADS | Physical glass/caps policy работаетвживойигре. Обнаружены неверный bind-frame расчёт и nativeADS hinge180°: live/pakedclip proof объясняет63mmуходэкрана. Slotframe Lua fix установлен; owner-qualified rotation-only C++ fix проходит integration/review. Полный ADSvisualPASS ещё нужен. |
| 11 | Дёргание движущихся машин | УличныеFGoff/on и bike stationary/moving видео+nativeinputs+traces. Repeats31.0%→1.36% вtraffic,28.5%→1.81% movingbike; accepted≈90Hz. Это application submissions, не headset scanout. Burst completion/редкиеprediction gaps описаны вcadence report. |
| 12 | PR203 и issue213 | Diff/архитектура/7cadence tests изучены; полный merge старогоtimer не подходит. Live currentbackend и motorcycleinputs сохранены, V557 не портирован и не объявленлучше безmatching history. SteamVR cadence ещё нужен. |
| 13 | Гранаты левее, hip pitch/вперёд/левее | 18 meshes сдвинуты C++, archive перепакован:1791payload files проверены, ровно18изменены. Hip+10mmвперёд/3mmвнутрь/4.5°cant. Live downwardL/R/SBS без грубогоотрыва вэтойпозе; не проверены всеavatar/stance combinations. |
| 14 | Удержание corpse и ходьба | Actual anchor, shared original mass, drawn-wrist иcontact/lifetime fixes committed. Single-hand walk3.93m contactp956.11mm, floorstress4.46s, mass5→100→5; same-body2→1→0hands mass100→100→5. Pairedlargegap разобранкак incompatible fixture geometry. Regrab calibration/groundforearm/differentlimbs не считатьполнымиPASS. |

## Начальный игровой сеанс

Игра запущена отдельно от измерителей `launch_to_save.py`, PID 22188. Ошибки мостового теста оставляли игру работающей.
RenderDoc `close_capture` подтвердил, что replay не был открыт. Активный runtime подтверждён логом: OpenXR Simulator.
Настройки до работы сохранены в scratch (`vrport.before.ini`, `launcher.before.ini`). На момент baseline render size 2648×2864 на глаз, FG NVIDIA OFA + FidelityFX, 90 Hz.

В `scratch/goal-audit-20261117`:

- `baseline-profiler/20261116-210315-986Z-11332`: 360 с WDDM/process/thread данных, RTX5070Ti 15995 MiB, 16 logical CPUs.
- `baseline-menu-cycles.json`, `baseline-memory-summary.json`: 12 SAO inventory open/close. Память процесса 8081.9–8102.5 MiB; каждый closed sample имеет pendingRetire=0. Lua 14.5–20.9 MiB, после последнего закрытия 16.9 MiB; это не длительный стресс и не доказательство отсутствия других утечек.
- `baseline-idle/gpu-scene.json`, `gpu-port.json`, `frame-report.json`: production GPU timestamp capture. В последнем окне 45.0 real FPS, 44.5 generated, 0.5 repeated/s. Учтённая FG VRAM 557416448 bytes (~532 MiB). Эта цифра не включает все allocations игры/драйвера/слоёв.
- `baseline-cpu-stacks.json`, `baseline-lua-by-mod.txt`, `baseline-hitch-analysis.txt`: 25 с, 2298 GameThread samples, 3143 worker samples. Lua chain в 13.8% GT samples; Stereo/HUD 3.18%, HandCollision 1.83%, TacticalBelt 1.39%. Sampling кратко приостанавливает потоки, поэтому его frametime не сравнивать с неинструментированным baseline. Точность имён нативных функций требует соответствующего PDB.
- ETW/WPR отказал с `0xc5585011` (policy); trace не получен. Это не замалчивается и не заменяется вымышленным GPU→CPU TID соответствием.
- `baseline-native-map-attempt*.json`: неуспешные тесты закрытия. Нельзя считать их closed samples, если inMenu=true. Карта сначала возвращается в hub; штатный OnBack нужен повторно до gameplay. После исправления сценарий проверяет каждую стадию.

Baseline фактически загружает Quest3ObsOverlay, OpenXR OBSMirror и VD compatibility. Нельзя приписывать все его VRAM/FPS одному порту. Отдельные overlay measurements ещё нужны.

Native map: выполнены 10 open→back→hub→back→gameplay циклов с проверкой IsInMenu на каждом конце. Из-за длительности переходов profiler завершился до конца серии: валидная VRAM есть до open7; closed7 и циклы8–10 в summary явно unavailable, без подстановки последнего значения. Open1–5: 9169.3, 9175.4, 9181.8, 9188.1, 9194.2 MiB; open7 9207.8 MiB. Это похожий на ~6.25 MiB/cycle рост, требующий более долгого повторения и отделения native map от API layers. Closed samples колеблются 8415.3–8768.3 MiB. Lua на карте ~12.5–12.9 MiB, после закрытия ~14.5–16.4 MiB.

Проверка actual simulator log исключила повторное создание XR swapchains как источник этого роста: всего7 creations (eyes2648×2864×2, HUD720×452, depth1536×1661×2, HUD2648×2864, minimap380×386), затем сотни тысяч строк без новых creations. 1280×1280 — recommended metadata. Рабочий simulator DLL из Downloads не совпадает по hash с более новым Desktop simulator build; анализ исходников последнего не доказывает поведение установленного бинарника.

GPU frame8 profile: DLSS evaluate eyes1.60+1.41 ms, RenderElements1.08 ms, SSR0.94 ms, LightIntegrate0.91 ms. Сумма measured ranges11.52 ms включает перекрывающийся async compute0.95 ms и не является wall-clock всего кадра. Подробности `baseline-idle/gpu-summary.json`.

После baseline агент получил исключительный runtime lease для Glamdoc, выполнил hot-deploy только Menu Lua (backup в `glamdoc-live/backup`). Реальная vendor session, preview обоих рук, палитры, Buy and Apply и повторный вход проверены. Зависимости исходного персонажа восстанавливаются до возврата lease; SteamVR и controller touch ещё не подтверждены. Скриншоты карты, preview и purchase confirmation отправлены в Telegram по просьбе пользователя. `glamdoc-live` содержит JSON receipts и L/R/SBS изображения; актуальный подробный отчёт Glamdoc дополняется агентом.

Archive заново извлечён из совпадавших repo/game копий (1791 files), заменены ровно18 grenade meshes, упакован, все1791 payload SHA256 сравнены с ожидаемым manifest после повторной распаковки. Новый repo archive SHA256 `AEE59DE7D1F9B050165E9D1F50D5E9F3635FE8DA9E135EE906D31A5A0A8EB48C`. В игру пока не установлен; original сохранён в scratch.

## Изменения и проверки

Подробности в соседних `goal-render-runtime`, `goal-corpse`, `goal-glamdoc`, `goal-weapons-belt`, `goal-overlay` отчётах от этой даты.
Первая общая Release сборка target `cyberpunkvrport_stereo` в `build/patchday/sdk25-build` прошла; после неё появились новые overlay/DLSS изменения, нужна incremental сборка перед deploy.
Ни один offline regression сам по себе не считается CET или physical-headset проверкой.

Legacy mirror session-retention 64 ресурсов заменён bounded publication+GPU leases. Этот путь не доказан активным в baseline SBS, поэтому он не объявляется причиной общей жалобы на 13+ GiB.
DLSS/RR cache retention исследуется с учётом Streamline lifecycle: GPU fence сам по себе не разрешает освобождать tag с eValidUntilPresent. Рискованный free по таймеру не используется.

## Правила завершения

Сначала закончить живые проверки и собрать доказательства по всем 14 пунктам; затем проверить диффы, собрать/упаковать/развернуть, сделать коммиты без Co-authored-by и закрыть игру/профайлеры/SteamVR/replay, восстановить тестовые настройки. Не помечать цель complete по факту чтения wrapper objective или по зелёным unit tests.

## Текущее состояние продолжения

Ветка `codex/performance-vr-audit-20261117`. Уже созданы коммиты без co-author trailers:
`30503636` Glamdoc; `19c2cffb` deployment/settings; `8b192967` symbol provenance;
`8255f379` capture/GPU ownership; `4b743a12` reload highlight; `597540e3` overlay hook chain.
Рабочее дерево содержит оставшиеся оружейные/поясные/corpse изменения и новую диагностику; не добавлять посторонние untracked art/scratch/deps.

PID22188 и PID6676 штатно закрыты. Третий PID20040 загружен **явно из AutoSave-13**.
Loaded DLL SHA256 `3A4132E5691FD62D4D3864230B9BF91AC7884CDBC4121A30136AEBF6D31CA04E`, PDB `E0039B9247CBD615578FE4E872C5E22BE75A27B1B785621CCD4FF4E66252D4B4`.
Он включает initial Census depth6, actual-anchor/mass normalization, предыдущий RTTI GetMass diagnostic и NoAnims Frenzy fix.
После этого source получил tonemap-reference fix, Census depth16 и direct PhysX mass getter; они **ещё не в PID20040**. Exact native-contact distance для lost-grip/harness также готовится отдельным source изменением.
Игра не должна перезапускаться из тестового worker. Root сериализует runtime leases между агентами.

Свежий fixed OpenXR probe:20 native map cycles (FG1×10, FG0×10),12 SAO cycles, правильные фазы сохранены в `fixed-openxr/samples.json`. Первый SAO attempt был отвергнут, потому что выход из native map вернул drawn Gorilla Arms; исправлен precondition штатным UnequipAll, без изменения production gates. SAO циклы после holster стабильны:8895.0–8915.5MiB. FG on45 real/около90 output, off75–78 real в этой сцене. FG retained-input memory при off не обязательно0 из-за depth-only capture.
25s fresh CPU sample имеет1124 кадра, median22.21ms/p99 25.67ms/max34.35ms,0 кадров>40ms. `symbolSession` подтверждает SymPdb, matching GUID/age110, mappedIdentityVerified=true, warnings[]. Старую native function attribution использовать нельзя; см. symbol audit. В settled10s windows: GameThread84–90% одного ядра; сумма15 redDispatcher301–330% сFG и504% безFG; XR submit TID14312 идентифицирован PDB как SubmitThreadThunk (19–23%/14%). WDDM 3D59% сFG и84% без, OFA11.8% только сFG. Эти числа не GPU→CPU-TID attribution и не universal benchmark.

### Точное накопление карты

В `census-openxr` доступны before/after-five, warm-before/after/delta, after-lua-collect и native-owners-closed.
После прогрева **два** map cycles дали committed live delta **13107200 bytes**, только две группы:
768×768 RGBA8_SRGB RT +2×2359296 и1024×1024 +2×4194304; releases0.
Всё остальное committed в этом warm window net0. Ни overflow, ни attach/read/coherence failures нет.
Полная Lua GC уменьшила Lua heap16.1→12.8MiB, но этиRT не освободила.
Native snapshot:9+9 backing objects, pool refs1, backing refs2/4. Native shape→DXGI29/RT/one-mip подтверждены IDA.
Это **точное накопление6.25MiB/map**, но владелец DynamicTexture пока не найден; не освобождать native refs вручную.
Native probe offsets проверены по actual PE timestamp1792368047, SizeOfImage0x581B000, executable SHA256 `d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4` и code spans.
x64dbg не подключался: его backend handshake не проходит, порт3000 занят другим node process. BP/пауза не применялись. Nsight attached GPU trace также не получен: инструмент не нашёл instrumented PID при реально живой игре; потребуется отдельный инструментированный launch, если нужен аппаратный trace.

### Автосохранения и защита исходного состояния

Отсутствие manual Save НЕ означало отсутствие записей: игра автоматически сохранила промежуточную Glamdoc покупку. Второй сеанс использовал такой AutoSave-0 (748 предметов/791112 денег). Поэтому сравнение первого и второго запусков не является строгим A/B одного сохранения/времени суток, хотя их отдельные измерения корректны.
Исходный AutoSave-13 от16.11 23:58:31 остался целым: sav.dat SHA256 `5E1F8CF21CC19D092660A1C6C1847576DA51D80679DCCFA2558FD562C06A5A44`.
После закрытия игры скопированы/проверены270 файлов текущих сохранений в `saved-games-before-more-tests/manifest.json`; известные тестовые AutoSave-0/1/14/15 **перемещены, не удалены** в `test-generated-autosaves/` под scratch.
PID20040 проверен после explicit AutoSave13:747 предметов,806112 денег, пустой стиль. До дальнейших мутаций установлен **nonpersistent native SaveLocksManager lock**. Доказательство `root-save-guard-20040.json`: удаление точного unique reason → manager=false/engine=false; повторныйADD → обаtrue. Он остаётся до ExitGame. Nested harnesses удаляют только собственныеunique locks после restore.
NameToString новогоdynamic CName возвращает пустуюстроку, поэтому отсутствие прежнего observer ACK не доказывает, что callback не вызывается. Для будущих fresh sessions использовать `root_save_guard_v2.py` (always causal; без этой строковой проверки). Нельзя полагаться только на engineLocked=true без доказанного root token.

### Оставшиеся обязательные живые проверки

Продолжить exact corpse contact/forearm/two-hands/constrained reach, optical view quality, остальные Glamdoc arm types/quest dialogue, реальные markers/traffic/bike, RR on/off/pressure и SteamVR null. Toolkit prepared per-user, registered DWORD1 (disabled), ещё требует активации и live. RTSS/NVIDIA требуют admin; Discord собственный updater не стартовал при закрытой игре. Не выдавать эти попытки за проверенную совместимость.
После завершения всех owned tests закрыть игру; следующий combined build должен включить source-only изменения и пройти нужные CPU/WARP tests при закрытой игре. Перед конечным завершением сравнить save manifest/убрать только доказанные новые тестовые saves в архив и оставить исходный AutoSave13 доступным.

### Следующее окно сборки

PID20040 штатно закрыт через ExitGame после возврата corpse lease; native root save lock оставался до выхода. Финальная проверка:0 удержаний/0 временных NPC,747 предметов,806112 денег, исходная позиция, пустой стиль. Save13 SHA256 повторно совпал. Лог сохранён в `scratch/goal-audit-20261117/session-20040-final.log`.

Коммиты `c41b2e23` (bounded tonemap publication/GPU leases и имена VR worker threads) и `1f8f77cf` (bounded resource lifetime/native texture diagnostics) созданы без co-author trailers. Mirror6/6, census WARP4/4, diff3/3 и native parser3/3 прошли. Полный combined Release build с Census depth16 и exact-contact diagnostic успешен; reviewer затем потребовал scene/step freshness gate для contact, поэтому перед deploy нужен incremental rebuild этой узкой правки.

### PID26944: глубокая диагностика и живые маркеры

Scene/step contact gate прошёл повторный review и incremental Release build. Targeted deploy с backup/hash manifest: `deploy-contact-census-20261117-031117`. Загружены DLL SHA256 `E94FC8E1D78D6C4824D0D299863C2CDE8379A839E4F3C01EE0B6327F7DF7C979`, PDB SHA256 `A2400DF834E21C34AE58A1DD8F6AF1B76AF5EE88C6E43E3A1B06A11F1A774AD8`, PDB age114. Свежий PID26944 явно загрузил AutoSave13. Сразу после world_ready inventory ещё746, затем обычная асинхронная инициализация завершилась до747; до совпадения baseline мутации не проводились. Native save lock оставался включён. `root-save-guard-26944.json` содержит причинное REMOVE/ADD доказательство и стабильные747/806112/no style.

Для собственных тестовых сеансов добавлена одна HKCU implicit XR probe запись. Исходное отсутствие записи сохранено в `xr-probe-registry-before.json`; удалить только эту запись при завершении. Toolkit остаётся disabled. Реальный runtime подтверждён `instanceProperties`: OpenXR Simulator Runtime1.0.27. Trace в `xr-probe-simulator/*26944.jsonl`; Windows directory listing показывает устаревший размер открытого файла, фактическое чтение с FileShare.ReadWrite подтверждает непрерывный JSONL. Никакого второго XR приложения не запускалось.

`census-deep-26944-b` содержит два завершённых map cycles и L/R/SBS. Первая попытка отдельно завершила один цикл, но screenshot использовал относительный output и был отвергнут; finally штатно закрыл карту и отключил census. Новый helper использует абсолютный output. В следующем окне target768/1024 выросли на3пары, не2, плюс один32MiB atlas: этот короткий endpoint не является строгой оценкой6.25MiB/cycle. Глубокая цепочка зато точно приводит к общему VirtualWindow renderer1F5C0C, полеDynamicTexture+4E8. Read-only native HUD join:72entries,109backings, stable/errors0;14target768/1024texture handles не принадлежат ни одному текущему HUD slot/window. Владелец ещё исследуется; ни один native ref вручную не освобождён.

`markers-26944/run.json`: completed, cleanupErrors[], remainingMarkers0. Три реальные временные native mappins,5poses neutral/yaw±15/roll±10, исходные PNG обоих глаз и marker trace. Neutral world camera/upload counters[114,114], WorldVertices2274/Texts84. В просмотренных neutral/roll изображениях нет грубого противоположного движения между глазами. Это не физическая Quest2+VD проверка и не численная субпиксельная оценка; SteamVR отдельно. JPEG review-копии получены обычной конвертацией исходныхPNG; исходники сохранены. Карта и roll screenshot отправлены Telegram.

### Уличный FG прогон PID26944

Исходная терраса не даёт хорошего вида на дорогу. Временная сцена выбрана по существующему native FastTravelPointData `FastTravelPoints.wat_lch_dataterm_05` (Брэдбери-стрит и Буран-стрит), затем `TeleportToNode`, без выполнения quest fast-travel request. Исходная pose сохранена в `GoalRoadScene` и `road-scene-26944/original.json`; **её ещё требуется восстановить до выхода**. Street position[-1556.0311,1246.0214,19.0609], orientation[0,0,.876124,.482085]. После обычного streaming/load-guard мир появился в обоих глазах.

`traffic-26944`: последовательные12s окна, FGoff/on, оба видео и native inputs, cleanupErrors[]. FGoff accepted1107 = real764/repeated343, cycle89.725Hz, source agep50/p95=18.206/25.721ms. FGon accepted1107 = real547/generated545/repeated15, cycle89.822Hz, source age19.530/31.171ms. No trace rows dropped. Это application submissions, не compositor scanout, не matched actor trajectory и не доказательство отсутствия дефектов OFA. На кадрах ночной трафик редкий (машина видна вFGoff, вFGon sampled frames преимущественно пешеходы); для уверенного сравнения движущейся машины нужен более плотный/длинный сценовый прогон. FGon video отправленоTelegram с указанной границей.

Дополнительные коммиты: `66872b5a` (Hurricane/actual dual weapon identity, shared declarations staged отдельно отcorpse) и `fc181026` (18grenade meshes, C++ translation tool, belt/hip placements). Livelease PID26944 сейчас у weapons agent для aligned ADS; root не меняет его inputs. После него вернуть исходнуюpose и провести короткий corpse exact-contact/mass прогон.

### Завершённые дополнительные проверки PID26944

`400502ab` содержит actual-anchor/mass/contact fixes, integrated drawn-wrist work из исходного рабочего дерева и защищённый corpse harness. `corpse-exact-26944`:10 motion/drop phasesPASS; walk3.93m, nativecontact p956.11mm/max8.74mm, valid100%; floorconstraint4.46s, raw/drawn gap1.046m, contactp957.49mm. Physical mass5→100 duringhold→5afterdrop. Общий первыйreportFAIL из-за post-drop regrab calibration drift: не переименовывать его в полныйPASS. `corpse-paired-exact-26944`:same actualbody8≠ref19; mass100→100→5 при2→1→0hands. Paired p95contact157mm объяснён C++ geometry: desired spans≈466mm против rigid actual147mm, минимальный meanerror≈150mm; это несовместимый fixture, не доказательство идеальногоtwo-hand качества. Full inventory comparison нашёл один runtime extraProp; удалён только доказанный новыйexactID/qty после сравнения сoriginalsnapshot. Итогfullmap747/806112/noDiff,0NPC/holds, rootguardretained. В harness добавлен полный inventory gate для будущих прогонов. Видео ходьбы отправленоTelegram, но праваякисть послеstanding частично/полностью внекадра: numericalcontact proof отделён от визуальной посадки.

Мотоцикл `Vehicle.v_sportbike1_yaiba_kusanagi` проверен по installedTweakDB; создан один непостоянный exactID10674648ULL/token1007834de32340368e085f046f14d207. Mount/Unmount request construction сверена с installed scripts, собственныйhelper проверяетrootguard/owner/record. В настоящемCET разныеEntityIDuserdata дляодногоhash неравны оператором==; исправлен mounted-delete guard вrender_runtime_audit/bridge.lua наexactUint64hash, regression теперь возвращаетfreshwrapperсодинаковымhash. CleanuptestsPASS.

Nativefloor rays определили ровный участок x=-1554,y1255–1265,z≈18.865. Bike origin reset[-1554,1255,19.20], yaw0, nativeForceBrakesUntilStoppedOrFor и speed<.05 передповтором. `bike-stationary-26944`:FGoff753 accepted=523real+230repeat; FGon752=369real+367generated+16repeat. `bike-moving-{off,on}-26944`:4s RT0.2, начальнаяпоза/нулеваяскорость одинаковы, мир/traffic междуокнами различаются. Off390=279real+111repeat; on387=191real+189generated+7repeat. Реальный путь около11–12m, screenshot/video показывают обтекатель/панель вдвухглазах; FGonclip отправленTelegram. Nativeinput captures сохранены. Это application cadence, не физическийscanout и не V557 A/B. После nativebrakes→unmount exactfixtureудалён, lifecycle cleanupcomplete; actorвернулсякoriginalsaveposition, full747/806112. Evidence `bike-spawn-26944/cleanup.json`, `road-scene-26944/restored.json`.

### Glamdoc и Osprey: подтверждения и границы

Glamdoc три дополнительныхтипа рук прошли functional preview/colors/cancel/close/reopen безпокупок. `glamdoc-variants-26944-complete/visual-review.json` явно visualPass=false: straight Equip fixture оставлял cross-arm additionalappearance residue, не выявлен productionGlamdoc defect. ИсходныйGorilla восстановлен штатным stagedunequip/reinstall/lateholster; entireinventory747/806112/noDiff,0NPC,styleNone, locksreleasedкромеroot. Synthetic SwitchToScenario injection неоказалась эквивалентной originalquestUI и неиспользуетсякак productionbugproof. Детали вGlamdocreport.

Osprey: прежние0.082°/11.53mm измерялись отweaponbindframe, неотanimatedaperture. Packedasset действительноskinnedкscope_hinge; новый optics/frames.lua содержитC++-derived inversebindpoint/orientation, init.lua берёт liveFX_slots/vrp_scope_hinge. Controlledhotload сbackup установлен в26944, slashimports соблюдены. Native atlas readback содержит читаемыеBRbrackets/centraldot; UIgeneration работает. Fresh animatedslotAPI работаетвhip; currentnativeADS6 не включился дажеприweapon144=1, безtriggerblock/CyberwareKind/vehicle/vision. Гейты не отключались. Root отдельнопрочитал SceneTier1 иNoCombat/NoAimfalse, effects только обычныеBodyStack/DetectorRush/Stamina. Следующийplannedfreshsession нужен для окончательной ADS-проверки; полноценныйvisualPASS пока не заявлен. Lease/cfg/inventory/pose восстановлены (`osprey-aim-input-26944/lease-return.json`).

### Один потерянный screenshot request

Во времяGlamdocbootstrap simulatorrequest49 остался shot_pending/size0. Rootнеперезапускалигру: **до** переключенияfocus nativeHUDreadiness доказал1s/91submissions/46sourceSerialadvances, g_menuMode0 иMenuSource4. CUA показалживойworldнаmonitorиобаVRглаза; лог `up: hung` означаетразмещение loadingpicture, ане зависание. Дваfreshshot requests завершились, включаяbackgroundпослевозвратаfocusвCodex. Это stale screenshot request, не подтверждениеblackVR. `capture-stall-26944` хранитnativeprobe+двеL/R/SBSпары. СтатичныеdiagnosticexportsприRuntimeDiagnostics0 сами по себе не означают остановкуpipeline.

### Следующие действия после26944

src/Natives/HudPanel.cpp получилreviewed startup-only diagnostic gate CVR_HUD_SEPARATE_WINDOWS=0|1 (default1) и5counters; currentDLLего ещёнеимеет. Build/deploy после закрытия26944, следующийsameOpenXR/AutoSave13 запускс0 иmapcensus проверитнеобходимостьforcedHUDwindows дляроста. Runtimeflipбезrespawnневалиден: текущийlogпоказывает10startupoverrideстрок зацелуюсессию. Еслиgrowthпри0сохраняется, overrideнеобъясняетрост; еслиисчезает, нуженmatchingfreshmode1confirmation. Никакихmanualresourcefrees.

Далее всёещё нужны отдельные SteamVRnull/FOV/cadence и Toolkit-сеансы, actualnormal-load/ADS Osprey, финальныйpackage иrestoreUI/runtime/registry/saves. XRprobe HKCU запись обязательно убратьпо `xr-probe-registry-before.json`; Toolkit исходноdisabled1, SteamVRnull исходноoff. ОригинальныйAutoSave13 должен остатьсядоступным; rootlock держатьдоExitGame.

### PID4744 и подготовка следующей сборки

26944 закрыт штатно после полного сравнения747exactItemIDs/quantities и проверки0ownedfixtures; `session-26944-before-exit.json`. ИсходныйSave13 hashсовпал. Gatebuild/deploy successful, manifest `deploy-contact-census-20261117-042345`: DLL42ECA4AD0D6F8EC2729D85CB6E40BEA0420CC1C3F565B91035B0E0DE8A0BC839, PDBD8955E3D52F4FDE15C82CC81B0E45121DC1C569B5640E44267DC157C99D25B20. Коммит5925eb08 добавляет толькоstartupdiagnostic.

4744 загруженизтогожеAutoSave13/OpenXRSimulator/с прежнимиAPIlayers, CVR_HUD_SEPARATE_WINDOWS=0. Guardcpvr_goal_root_6bf1df18731e46d6bc1be150c88f9216 causallyverified,747/806112. `hud0-4744/gate-{before,after}.json`:copyCalls1/returned72/matched10/forced0/bypassed10, stable. ПослеодногоwarmupMap ещё2mapcycles дали EXACT targetdelta13,107,200bytes:1024²+2 и768²+2,releases0. Остальныеcommittedreleases снизилиобщийnet до12,517,376bytes; placedfootprint−29,491,200 не суммировать сcommitted. Coverageerrors/overflow0. **ForcedHUDspawnoverride не является необходимой причиной этойпары**; runtime0/1flip неиспользовался.

Вэтойfreshsession normalLua require frames/visibility работает, nativeADS6 entered. Actualslot/clip analysis Osprey: hinge pivotпочтистатичен(<.2mm), weaponrotation.026°, hinge relative rotation179.998°; parent0/1 не меняются, толькоhinge8flip иbone11translation1.418mm. Eyeaxis hip≈11mm→ADS63.4mm. Это реальное объяснение экранногоухода. Готовится C++ owner-qualified hinge fix; global VRRigWrite неприемлем, поскольку он затрагивает одинаковыеNPCrig instances. Integrationbuild остановился на missing include/StaticArray types; дополнительноreview нашёлточный Receiver controlBinding иmergedMetaRig case. SDKsizeassertions неослабляются; исправляется include/typedbinding.

4744 тоже закрыт штатно после full747map/noDiff и rootguard (`session-4744-before-exit.json`); Save13осталсятемже. Нативныйfactory-origin observer готов/3CPP+3Python tests+independentreviewPASS, committed730f5d78. Он ещёнепроверенвигре; defaultoff, noAddRef, Boot84послеsurfacechecks83, factory5EA8A4/out16/descriptor24/rawstack32/object+control+serial, ring1024/dropcoverage.

SteamVRnull подготовлен после закрытияигры:2648×2864/eye/90Hz; originalkeysвbuild/steamvr-null/state.json, fullsettingsbackup `steamvr-settings-before-null.json`. SteamVR2.18.4 UI показалработающий90Hznull и Auto resolution172%=3472×3756/eye. НиFOV,ниglobalresolution пока не менялись. Runtime затем штатно закрыт через UI перед следующимbuild/Simvalidation; **nullconfiguration пока остаётсявключённой** и должна быть восстановленаfinaloff. Ниодногосеанса gameplaySteamVR пока не было.

Уточнениерасстоянияbike: примерно9.1m непосредственно за4s измеряемогогаза;11–12m кпоследующемуbrake request включаетcoasting/nativecapture. Этизначения не являются однимидентичныммаршрутомпо времени.
