# Оружие, подсветка, оптика и пояс — 17.11.2026

## 2. Heywood Hurricane

В распакованных скриптах игры это `ValentinosFrenzy`: `scratch/menu-load/scripts/cyberpunk/player/psm/shootTransitions.script`, класс `ValentinosFrenzyInitDecisions`. Вход штатно срабатывает на `QuickMelee` tap; `DefaultTransition.CanStartValentinosFrenzy` проверяет уровень Double Trouble (`Cool_Central_Milestone_2`), обе руки, патроны, перезарядку, безопасное состояние, дружественную цель, транспорт и другие ограничения. Сами эффекты, Triple Tequila, длительность и стрельба принадлежат штатной машине состояний.

Добавлен `include/Hooks/DualWieldRapidFire.hpp`: по два полных нажатия **каждого** триггера за 600 мс, попеременно или одновременно, дают запрос на 200 мс. Удержание автоматического оружия и быстрые нажатия одной руки запрос не создают. Контекст меню/сканера/транспорта сбрасывает накопление. Используется уже существующий shared slot 165, дополнительный бит 8; новых shared slots нет.

`XInput.cpp` публикует запрос рядом с выбором руки. REDscript добавляет listener `RangedAttack` к штатному входу и будит `EnterCondition` также на нажатии: если обе руки были отпущены, предыдущий release произошёл до запроса и сам по себе недостаточен. Wrapper разрешает вход по запросу VR через **штатный** `CanStartValentinosFrenzy`; QuickMelee остаётся рабочим. Анимации/эффекты и выходы Hurricane не заменены.

Для двух экземпляров одной модели старый hitscan выбирал руку по триггерам/чередованию — после отпускания триггеров автоматические выстрелы Hurricane уходили бы из правого ствола. Теперь `SetVRDualWieldWeapons` публикует конкретные attachment instances. `WeaponAim.cpp` читает typed `ref<gamedamageAttackData>` из `EffectSharedData.attackData` и сравнивает его `weapon.instance` с этими экземплярами. `EffectPreAction_PreAttack.Process` штатно записывает это поле из `EffectScriptContext.GetWeapon(ctx)` перед ray. Публикуемые pointers только сравниваются, не разыменовываются и не удерживают оружие в памяти. Guess по триггерам/чередованию удалён; уникальный item record остаётся fallback при отсутствии attackData. DebugDualShot path=4 означает actual instance route.

Проверено: C++ regression `dual_wield_rapid` — попеременная и парная стрельба, одна рука, удержание, медленная стрельба, истечение запроса, сброс контекста; `dual_weapon_identity` — реальные identities при повторных/асимметричных rays, swap, чужой effect и unequip. Полный REDscript compile после review против установленного `final.redscripts`: 0 ошибок и 0 предупреждений (`build/patchday/redsbox-goal-weapons-review-20261117/r6/logs/redscript_rCURRENT.log`).

Live на PID 20040 подтвердил две одинаковые и две разные модели, штатный эффект/состояния способности и дополнительные автоматические выстрелы обеих рук после отпускания триггеров; подробности ниже. Отрицательные live-сценарии с Double Trouble 1, одним пустым оружием и меню отдельно не прогонялись. Штатные ограничения остаются в `CanStartValentinosFrenzy`.

## 9. Подсветка после убирания оружия

Доказанная причина: `CyberpunkVRPort_Reload/init.lua` сохранял `lastW` ещё 30 кадров после того, как `GetActiveWeapon()` возвращал nil. Проверка `GetWorldPosition` на ещё живой entity не доказывает, что оружие в руке; поэтому glow оставался на 0.7–1 секунду.

Теперь после пропуска `GetActiveWeapon` проверяются `AttachmentSlots.WeaponRight/WeaponLeft`. Оба пусты — оружие сразу считается убранным. Если API недоступен, grace остаётся для reload state, но glow не рисуется без подтверждения удержания. Пропажа отслеживаемых рук также скрывает glow без выбрасывания компонентных кешей. Временный пропуск active weapon при заполненном слоте не мигает подсветкой.

Проверено: `py -3.13 tools/weapon_tests/test_reload_holster.py` исполняет production resolver с живой старой entity: holster убирается немедленно, fallback и левая рука работают, ошибка API не подтверждает stale glow. Нужно live: пустой магазин / извлечённый магазин / затвор, holster и смена оружия, обе руки. Оставшийся короткий fade резервных Reach brackets отделён от устранённой 30-кадровой задержки.

## 10. Меши прицелов при ADS

Asset census `build/optics-20261026/{embed,iconic,json}` подтвердил различие: `invisibleWhenAiming` используют непрозрачные `*_cap.mesh`, но **также** обычное стекло Osprey `w_rifle_sniper__nokota_osprey__base1_3648` / `..._02_glass.mesh`. Старый код оптики выключал любую компоненту с таким track, включая физическое стекло.

Добавлена `optics/visibility.lua`: девять подтверждённых native cap/decal-cap компонентов остаются выключены (чтобы не закрывать линзу); остальные ADS-gated физические меши остаются видимы. Покрыты `entPhysicalMeshComponent` и отдельная иерархия `entSkinnedMeshComponent`; обычный `entMeshComponent` не имеет visibilityAnimationParam и не трогается. Кеш CName объявлен выше scan. Сохраняются и восстанавливаются исходные gate/enabled; политика применяется при обнаружении gate, без циклического Toggle(true)/Toggle(false) для заменённых reticle meshes. CET import — `require('optics/visibility')`.

Проверено: LuaJIT syntax изменённых Lua production файлов; `tools/optics_tests/visibility_scan_test.py` исполняет реальный scan/restore на Grad cap / Osprey glass / physical tube / plain MeshComponent без поля / unrelated showMagazine, затем повторный scan без toggle churn и полное восстановление. **Вид не подтверждён**: нужен общий live ADS↔hip, Osprey (glass теперь виден), Grad, Nekomata, один reflex sight; снимки каждого глаза, aperture/reticle/геометрия/отсутствие перекрытия стеклом и восстановление при смене оружия. Это точечное исправление найденного случая, не доказательство исправления всех возможных исчезновений.

## 13. Пояс

Hip gun: +10 мм вперёд, −3 мм к центру, дополнительный наклон вперёд примерно 4.5° (`dir` forward 0.10→0.18); высота сохранена. Shell и reach используют ту же placement, поэтому не расходятся с оружием. Для левого хвата всё штатно зеркалится.

Девять типов гранат сдвинуты на 12 мм влево (к центру для обычного правого расположения), 4 мм вперёд для посадки по линии пояса. У зеркального пояса знак X противоположный, то есть сохранена анатомическая посадка. 18 binary meshes изменены в `mods/archive/build/base/vrport/meshes/vrp_belt_gren_*.mesh`; только используемые front/biohazard/cutting/emp/flash/incendiary/ozob/recon/smoke и их `_lh`. Неиспользуемые mid/back не затронуты. Синхронизированы `CFG.centres` и staging `build/one-archive-20261027/pack/cyberpunkvrport/base/vrport/meshes`.

Вся математика выполнена скомпилированным C++ `tools/belt_tests/translate_skinned_mesh.cpp`: translation в bind frame по reference inverse-bind для каждой кости, bounding box сдвинут вместе с геометрией. Python/PowerShell — только JSON/CLI/файлы/верификация. Runtime `SetLocalPosition` не добавлен: локальная документация отмечает дрейф skinned offsets при поворотах. Оригиналы: `build/goal-weapons-20261117/grenades/originals`; исходные JSON, translated, cooked, roundtrip и centres.txt рядом.

Проверено: все 18 мешей прошли deserialize→serialize, численные значения соответствуют C++ результату в точности Float32, геометрия, vertex buffers, материалы и skin weights сохранены. **Общий archive ещё не перепакован**, это выполняет root перед общим live. Нужны изображения мужского/женского тела, прямой/зеркальный пояс, приседание и повороты; мелкая эстетическая доводка без них не доказана.

## Команды

```
cmake -S tools/analog_input_tests -B build/goal-weapons-20261117/analog
cmake --build build/goal-weapons-20261117/analog --config Release --target dual_wield_rapid_test
ctest --test-dir build/goal-weapons-20261117/analog -C Release -R ^dual_wield_rapid$ --output-on-failure
py -3.13 tools/weapon_tests/test_reload_holster.py
py -3.13 tools/patchday/check_reds.py --label goal-weapons-20261117
cmake -S tools/belt_tests -B build/goal-weapons-20261117/belt
cmake --build build/goal-weapons-20261117/belt --config Release
```

REDscript label существующий: повторный запуск check_reds удаляет и пересоздаёт его sandbox; перед повтором проверить абсолютный target внутри workspace. Сборка всего plugin и live относятся к root. Игра/RenderDoc этим агентом не запускались, коммитов нет.

## Первый live-прогон, PID 6676

Root предоставил exclusive lease уже запущенной игры; harness не вызывал запуск, выход или manual save. Первая версия ещё не блокировала автоматические сохранения — важное ограничение ниже. Применён `tools/weapon_tests/live/run.py`, восстановление проверено относительно загруженного в тот момент состояния по точным ItemID и количествам всей исходной inventory, шести equipment slots, active/gadget selection, perk rank/unlocked state/points, firing fact, magazine/ammo counts и исходным байтам конфигурации. Новые оружия не сохраняют dynamic tags GiveItem, поэтому cleanup дополнительно использует собственный список точных created ItemID, запрещая удаление любого ID исходной inventory. Первое неполное удаление обнаружено сравнением inventory и исправлено до продолжения; `weapons-live-02/restored-repaired.json` подтверждает возврат.

**Hurricane: обнаружен дополнительный реальный конфликт.** В `weapons-live-02/same-nue-pair.json` два одинаковых Nue, реальные native IDs разные. Четыре manual shots в 0.11/0.22/0.349/0.48 с; route `DebugDualShot[1]=4` подтверждён для обеих рук. Способность штатно входит в ShootState 4→5→6, `ValentinosFrenzySE` активен приблизительно 0.48–1.59 с. Однако автоматических выстрелов после отпускания **ноль**. Существующий `CyberpunkVRPort_NoAnims/vrport_weapon_trigger.reds::VRDirectSemiAuto` перехватывал semi-auto даже во Frenzy, а IsSemiAutoAction требовал удержания RangedAttack и нового press count. Это подавляло штатный ShouldForceShoot.

Production guard дополнен `!this.IsInValentinosFrenzy(scriptInterface)`: только во время native способности её cadence полностью принадлежит игре. Обычный режим press-only остаётся прежним. Полная REDscript компиляция: **0 ошибок, 0 предупреждений**, `build/patchday/redsbox-goal-hurricane-native-20261117/r6/logs/redscript_rCURRENT.log`. Исправление не подменялось live Override и **требует повторного запуска с новым REDscript**. Harness теперь сначала проверяет один выстрел при обычном удержании триггера, затем требует дополнительные автоматические выстрелы обеих рук после release + 100 мс и route 4 обеих сторон; одних SE/состояния 4/5/6 недостаточно для PASS.

**Reload glow:** `weapons-live-03` и `weapons-live-04` — настоящий empty magazine через production ammo API, glow component реально включён; после native holster обе carrier-компоненты уже выключены в первом sample с пустыми WeaponLeft/Right, delta shared entity ticks **0**. Запись `scratch/goal-audit-20261117/weapons-live-04/glow-holster.mp4` (2.78 MB) и пары `glow-before-holster_{L,R,SBS}.jpg` / `glow-after-holster_{L,R,SBS}.jpg`.

**ADS:** Osprey hip/ADS снимки и component census в `weapons-live-05`. В ADS UpperBody=6; physical glass `..._3648` включён в обоих состояниях, все три cap/decal-cap выключены. FPS overlay на последних снимках временно скрыт, потом настройка восстановлена. Дополнительный A/B glass on/off в `weapons-optics-ab` не показал, что включённое стекло перекрывает port feed. Геометрия не исчезает и не образует непрозрачную заглушку. Прицельная отметка в этой сцене/ракурсе не различима как с glass on, так и off, поэтому качество ретикла отдельно **не объявлено проверенным**. Лучший измеренный eye-to-axis offset для фиксированных поз — около 15 мм. Нужна дальнейшая проверка прицельной картинки, а также других моделей прицелов.

**Пояс:** third-person offset-camera снимки `weapons-live-03` не показывали тело и не были приняты как доказательство. Естественные look-down снимки `weapons-live-04/belt-down_{L,R,SBS}.jpg`, `hip-down-right_{L,R,SBS}.jpg`, `belt-down-left_{L,R,SBS}.jpg` показывают настоящий пояс, frag grenade и Nue в hip shell на текущем мужском теле/одежде. Shell и weapon attached, grenade type frag, charges=2. Перевёрнутый пояс и женское тело этим прогоном не проверены.

Перед возвратом lease: `scratch/goal-audit-20261117/weapons-lease-return.json`. Игрок возвращён в исходную позицию `[-2281.9252929688, 1707.9191894531, 18.610000610352]`, forward сохранён; holds 0/0, меню закрыто, оружие убрано, временные предметы удалены, `WeaponAudit=nil`; perk=0/unlocked=false/points=0, firing fact=1 как до теста. `vrport.ini`, `holster_places.json`, `belt.cfg` восстановлены побайтово и проверены SHA256. Все завершённые прогоны 03/04/05/A-B имеют `restored.ok=true`. Игра оставлена запущенной root, дальнейшие live-действия прекращены до нового GO.

## Исправление безопасности автосохранений

Root обнаружил, что отсутствие manual-save команды **не предотвращает autosave**. Загруженный во втором сеансе AutoSave0 уже включал покупку Glamdoc из предыдущего теста: 748 inventory items / 791112 money вместо исходных 747 / 806112. Поэтому `restored.ok` первого weapon-прогона доказывает восстановление его собственного in-memory snapshot, но не отсутствие тестовых изменений на диске и не соответствие исходному AutoSave-13. Root сохранил backup 270 save files и обратимо архивировал test-generated AutoSave0/1/14/15. Weapon harness save-файлы не изменяет.

Runtime-проверка root на PID 20040 показала: `SaveLockRequest ADD` действительно блокирует manager и engine; ACK не был замечен проверкой через `NameToString`, а private m_saveLocks не доступен через CET. Позднее corpse-проверка обнаружила, что новый dynamic CName может возвращать пустую строку: это не доказывает отсутствие вызова ObserveAfter. Plainstring token нужно хранить отдельно, а CName при необходимости сравнивать по hash equality. Root доказал собственный уникальный lock причинно: REMOVE → manager=false/engine=false, ADD → оба true, без игровых мутаций (`scratch/goal-audit-20261117/root-save-guard-20040.json`). Этот causal proof остаётся валидным. Baseline нового сеанса: 747 items / 806112 money / style None.

Harness требует проверенный token `GoalSaveGuard.verified=true`, verification `causal-remove-readd`, неизменное корректное имя root-lock и текущий `Game.IsSavingLocked()==true`. Одного engine true или неподтверждённого token недостаточно. Без verified root protection standalone режим отказывается от мутаций. До этой проверки не меняются simulator inputs, native diagnostics, настройки или actor state; каждый game mutator повторяет проверку.

Дополнительно harness отправляет собственный unique `SaveLockRequest ADD`. REMOVE этого reason разрешён только после успешного точного восстановления, повторной проверки через интервал drain и `InvalidateAutoSaveRequests`. Его запросы честно отмечаются как queued, без выдуманного ACK. После REMOVE проверяется сохранение root protection и снова очищаются autosave requests. Root lock держится до ExitGame и не затрагивается. При ошибке snapshot и own-lock request сохраняются для восстановления. Зависимость от неподтверждённого observer ACK удалена; токен хранится как исходная строка, без roundtrip через NameToString.

`tools/weapon_tests/test_live_save_lock.py` исполняет production lock-функции с изолированным API adapter: bare engine true, unverified/noncausal token и смена root имени запрещают работу; failed restore сохраняет блокировку; REMOVE затрагивает только own reason. Проверка пройдена. Реальная Game.IsSavingLocked/SaveLockRequest проверена root причинным тестом; каждый attach всё равно проверяет token и engine **до** мутаций.

Root-owned запуск PID 20040 явно загрузил AutoSave-13; использован короткий повтор:

```
py -3.13 tools/weapon_tests/live/run.py --pid 20040 --out scratch/goal-audit-20261117/weapons-repeat-20040 --root-exclusive-lease --expected-inventory 747 --expected-money 806112 --case same --case different --case optics --scope-yaw 60
```

## Повтор Hurricane с native Frenzy guard — PID 20040

`scratch/goal-audit-20261117/weapons-repeat-20040/report.json` содержит все пять успешных поведенческих/component проверок, `errors=[]`:

- Nue + Nue: после полного отпускания обоих триггеров и дополнительного запаса 100 мс — **6 автоматических выстрелов, 3 слева и 3 справа**. Нативный эффект активен, force-state зафиксирован на 0.740 с. Actual weapon instance route `DebugDualShot.path=4` наблюдался у обеих рук после release.
- Nue + Overture: после того же порога — **5 автоматических выстрелов, 2 слева и 3 справа**; native force-state на 0.710 с, route 4 обеих рук. Это исправляет before-state, где происходили только четыре ручных выстрела без продолжения.
- Вне способности удержание правого триггера 0.65 с в обеих парах даёт ровно **один** выстрел, frenzy не активируется. Guard не превращает обычную semi-auto в автоматическую.
- Osprey component-проверка: физическое стекло включено hip/ADS, caps выключены, UpperBody=6 в ADS. Но yaw=60 в этом сеансе дал взгляд **мимо оптической оси**, что видно на L/R/SBS. Эти снимки не приняты как доказательство читаемого ретикла или дальней цели. Для следующей визуальной проверки нужно согласовать фактическое направление оружия и HMD; одного фиксированного yaw недостаточно.

Неизменённые записи: `same-nue-pair.mp4` (1,835,572 байт), `nue-overture-pair.mp4` (1,885,797 байт), оба SBS; detailed native samples и входы находятся в одноимённых JSON. Osprey: `osprey-{hip,ads}_{L,R,SBS}.jpg`, с указанным выше ограничением.

Восстановление до возврата lease подтверждено двумя `restored.ok=true`, затем независимым `lease-return.json`: inventory **747**, money **806112**, holds **0/0**, firing fact **1**, меню и оружие закрыты, `WeaponAudit=nil`, command queue пуста, запись остановлена. Позиция `[-2281.9252929688,1707.9191894531,18.610000610352]`. Все три конфигурационных файла побайтово совпадают с before. Own REMOVE отправлен после восстановления/drain; root guard по-прежнему causally verified и engine saving blocked. Игра оставлена запущенной, live lease возвращён root.

## ADS на дальней сцене — PID 26944

Позднейший structural review установил: axis/relief цифры ниже относятся к weapon-root-transformed bind centre, а не к анимированному экрану Osprey (`scope_hinge`). Они не доказывают physical-aperture alignment. Исправление описано в `docs/goal-osprey-reticle-20261117.md`; visual reticle PASS не заявлялся и не заявляется.

Root предоставил уличную сцену Bradbury/Buran и verified causal save guard. Первый проход `weapons-optics-aligned-26944` отказал на ошибке сериализации словаря ray в Lua; восстановление прошло. Исправлено на два slash-independent Lua array arguments. Регрессия `tools/weapon_tests/test_optics_bridge.py` исполняет реальные bridge-функции и реальную Python→Lua передачу, затем compiled C++ geometry checker; distant target разрешён, near wall отвергнута. PASS.

Проход `...26944b` честно отверг фиксированную ориентацию: Osprey имеет примерно 12.48° authored grip offset. В стенд добавлены пять фиксированных yaw/pitch вариантов контроллера, выбираемых по фактической оси оружия, без изменения игровых гейтов. Все численные axis/relief расчёты выполняет существующий C++ `render_audit_math optics`; Lua только снимает camera/weapon transforms и делает static-only raycast. Новый режим прошёл compile/self-test.

Успешный измерительный проход — `scratch/goal-audit-20261117/weapons-optics-aligned-26944c`: HMD yaw 0, legacy hand ypr (11,-6,0), position (0.06,-0.18,-0.25). На ADS измерены bore/camera angle **0.0817°**, right-eye-to-axis **11.53 мм**, rendered relief **20.35 см**, статичная цель **33.72 м**. Те же gates прошли hip→ADS→hip. Физическое стекло остаётся включено, cap/decal-cap выключены, UpperBody=6. Снимки `osprey-{hip,ads,back-hip}_{L,R,SBS}.jpg`, клип `osprey-hip-ads-hip.mp4` (1,404,435 байт).

Для проверки именно obstruction выполнен отдельный aligned A/B `weapons-optics-aligned-ab-26944`: glass ON/OFF/ON, каждый раз сохранены C++ geometry checks и 1600px L/R/SBS без ограничения разрешением записи видео. Native reticle original panel выключен ожидаемо; `vr_optic_surface`, `vr_optic_panel`, `vr_optic_reticle_rear`, `vr_optic_reticle_rear_panel` включены. Боковой экран показывает сцену, но читаемый reticle на осмотренных изображениях не обнаружен ни при glass on, ни при off. Включённое физическое стекло не создаёт обнаруженного отличия или закрывающей заглушки; **полный reticle visual PASS по-прежнему не заявлен**. Нужна отдельная проверка содержимого/композиции reticle panel, а не ослабление fixture alignment gates.

Все проходы восстановлены; итоговый `weapons-optics-aligned-ab-26944/lease-return.json`: 747 items, money806112, holds0/0, меню/оружие закрыты, `WeaponAudit=nil`, pending command=false, root guard verified/blocked=true. Уличная позиция `[-1556.0311279297,1246.0213623047,19.060920715332]`, три конфигурационных файла побайтово равны before. `GoalRoadScene` не изменялся. Live lease явно возвращён root.
