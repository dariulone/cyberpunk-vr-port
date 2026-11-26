# Удержание тела NPC: аудит и исправления, 2026-11-17

Статус: первый пакет проверен в существующей OpenXR Simulator сессии 6676: реальный захват голени, подъём, вставание, ходьба, поворот, отпускание и повторный захват подтверждены native snapshot и видео. Обнаруженный этим прогоном второй пакет (actual anchor + исходная масса) реализован и проверен offline, но пока не развёрнут в игре; нужна общая новая сборка. Forearm, same-part/different-limb two-hand, измеренный constrained reach >3 с и SteamVR остаются live gates. Игра и RenderDoc этим агентом не запускались; управление процессом возвращено root. Коммит не создавался.

## Исходное состояние

Прочитан `C:\Users\dariulone\Desktop\npccorpseclaude.txt`. В нём наблюдались смещение при хвате 0.19–1.03 м до исправления, затем около 0.073 м, отставание кисти при нагрузке/ходьбе и эпизод автоотпускания при недостижимом контроллере. Это исторические результаты, не измерения текущей версии.

Уже имевшиеся изменения `mods/cet/CyberpunkVRPort_HandCollision/init.lua` сохраняют посадку в координатах нарисованной руки. Они сохранены в `scratch/goal-audit-20261117/corpse-preexisting.patch` и оставлены в рабочем дереве. Новые изменения дополняют их.

## Исправления и основания

1. **Ложное отпускание при ограничении досягаемости.** Нативный `HoldSide` удерживает тело относительно завершённой FK-позы руки. Lua проверял потерю хвата относительно сырого контроллера. Поэтому при правильном положении тела в ладони и удалении контроллера более чем на 45 см происходило отпускание через 3 с. Проверка теперь использует тот же drawn-wrist frame. Если кадр не получен, таймер не увеличивается без доказательства потери контакта; нативная проверка свежести остаётся. Реальное отставание тела по-прежнему вызывает отпускание.

2. **Поворот тела двумя руками.** `HoldSteps` усреднял только запрошенные вращения, игнорируя изменение направления между точками хвата. Подъём одной руки при неизменных вращениях кистей не поворачивал часть тела. `TwoHandTarget` сохраняет усреднённый twist кистей и совмещает ось между точками тела с осью между целевыми точками рук. Длина тела неизменна, несовместимое расстояние распределяется относительно середины. Вырожденные близкие точки используют вращения кистей; обработаны противоположные кватернионы и разворот на 180°. Действуют прежние ограничения torque/speed.

3. **Повторное измерение всего ragdoll при двух руках на разных частях.** Кэш `HandBody.cpp` имел ключ только proxy и единственный `heldWhenMeasured`. Чередующиеся вызовы от двух конечностей каждый раз перечитывали все тела и заново принимали растянутую текущую позу за исходную. Ключ стал `(proxy, held anchor)`, память остаётся ограниченной 16 записями. Ожидаемый эффект: по одному измерению на каждый новый якорь, без повторного обхода всего ragdoll каждый кадр.

4. **Конкурирующая скорость второй удерживаемой конечности.** Режим floor-tow (`other=-2`) терял номер части, которую держит другая рука. Расширено внутреннее представление `other=-(body+3)`: это тот же режим, но вторая часть исключается из tow. Старое `-2` сохраняет прежнее значение. Lua и DLL необходимо устанавливать вместе.

5. **Преждевременное засыпание после отпускания.** `VRCorpseSpeed` проверял список остальных частей, из которого исключён измеренный anchor. Теперь скорость anchor тоже входит в максимум; движущаяся ранее удерживаемая часть не будет ошибочно признана покоящейся только потому, что остальные части неподвижны.

6. **Неограниченный Lua cooldown-кэш.** `K.DEAD_ASKED` сохранял идентификатор каждого NPC, для которого когда-либо запрашивалась активация ragdoll. Истёкшие записи удаляются раз в секунду после полного четырёхсекундного cooldown. Это исправление Lua RAM/обходов таблиц; оно не является объяснением гигабайтов роста VRAM.

7. **Tow/lift следовал reference body вместо реально удерживаемой части.** В live `corpse-shin-06/acquisitions.json`: Lua `rec.body=7`, а completed physics hold — `proxy=68334, body=18`. Старый `corpseTick` передавал в VRCorpseHold якорь 7 и не исключал actual held body 18 из velocity assist. Теперь выбираются реальные anchors из свежего `VRPhysHeldBody`: active, тот же proxy, age<200 ms. При pending/stale/foreign native hold assist пропускает кадр, без опасного fallback. Legacy/старая DLL сохраняют прежний fallback. Две фиксированные переиспользуемые ячейки предотвращают двойной drive одного native actor через разные Lua references. Локальные координаты F.grip/lost-grip не перепутаны с native-body frame и не изменялись этим исправлением.

8. **Solver mass не должна становиться массой трупа.** `HeldTake` временно поднимает массу части до 100 кг/20x. Перемер после смены actual anchor мог прочитать эту искусственную массу вместо исходных 5 кг; сумма `rec.whole` используется для ограничения скорости игрока. Существующий `holdLoads` теперь дополнительно публикует `Held::ownMass`. Внутренняя C++ функция `OriginalHeldMass` нормализует `MassOf`/`CorpseMeasure`, включая уже закэшированную искусственную массу. Нового RTTI API или таймера нет. При двух руках на одном теле используется общий `Held`, второе взятие не вызывает `HeldTake` и не перезаписывает original mass. Actor identity для reuse/share теперь проверяет pointer+proxy+body, а не один переиспользуемый PhysX pointer.

9. **Actual anchor применяется и к высоте/засыпанию.** VRCorpseHold читает высоту реально удерживаемого actor, а не переданную высоту Lua reference. VRCorpseSpeed дополнительно проверяет все реальные anchors из ограниченного кэша, даже если они совпадали с другой позицией при первоначальной дедупликации форм. Один body не опрашивается дважды.

Пункты 7–9 должны устанавливаться вместе (DLL + HandCollision Lua). На работающий 6676 Lua-only вариант не ставился.

## Проверки

- `cmake --build build/physics_frame_tests --config Release`, затем `ctest --test-dir build/physics_frame_tests -C Release --output-on-failure`: 4/4 успешно. Проверяются координатные системы, нативное вычисление целей, original held mass и collision filters.
- `tools/physics_frame_tests/grip_drive_test.cpp`: реальные функции из production header, неподвижные вращения кистей при повороте линии хвата, смена рук, 180°, несовпадающее расстояние рук, совпадающие точки, twist, нечисловые входы.
- `py -3.13 tools/grasp_tests/corpse_driver_test.py`: Lua-фрагменты production-кода, обычный/native fallback, потеря хвата, constrained drawn wrist, временно отсутствующий frame, кодирование двухручного tow и истечение cooldown без преждевременного удаления.
- Расширенная Lua-регрессия проверяет ref7→actual18, второй actual anchor19, pending/stale/wrong-proxy/error responses, один native body через две Lua references и сброс dedup между update.
- `held_mass_test.cpp` проверяет массу 95 кг для 19 частей по 5 кг после solver override до 100 кг и смены anchor; две руки на одном/разных actors, отпускание первого holder, stale/foreign snapshots и повторное использование actor pointer.
- `py -3.13 tools/grasp_tests/corpse_pick_test.py`: выбор ближайшей части своего ragdoll, соседние предметы, ошибочный surface.
- `build/goal12-20261111/held-pose-test.bat`: 72 набора вращений и удержание с angular drive при 45/60/90/120 Гц успешно.
- `git diff --check`: ошибок whitespace нет; рабочий репозиторий сообщает свои обычные LF/CRLF предупреждения.

Численные вычисления и тесты позы реализованы и выполняются в C++. Lua-фрагменты тестируются в standalone LuaJIT; это не доказательство загрузки в CET.

## Реальная сессия 6676: что установлено

Attach-only harness находится в `tools/grasp_tests/corpse_live/`. C++ отвечает за все числа/геометрию/траектории/Jacobian/analyzer; Python — за I/O, отправку готовых poses и сбор данных. Игра запускалась root отдельно и оставалась живой при всех ошибках теста.

Первые прогоны выявили проблемы именно с постановкой/измерением теста и **не были засчитаны**:

- `corpse-shin-01`: safe point попадал под/за лавку; позиционная ошибка остановилась на 11.8 см.
- `corpse-shin-02`: после teleport player фактически оказался на бордюре (Z19.005 вместо18.610). Добавлен выбор C++ standing footprint по 30 static raycasts (6 кандидатов × 5 точек).
- `corpse-shin-03/04`: drawn endpoint под contact/reach constraints нельзя использовать как статичную controller mapping. Offline reviewer обнаружил изменение drawn quaternion примерно на27° внутри первой translational probe при неизменном raw quaternion. Добавлены stable/bracketed baseline samples; controller positioning теперь использует raw palm, а настоящий контакт/grip проверяется отдельно. Голова0.55м не дала автоматически зелёного результата и не стала заменой естественной позе.
- A/B arm_reach on/off без NPC (`corpse-reach-replay`) сам по себе не доказал production bug: временные IKlens могут растягиваться до0.8458м, тогда как final FK остаётся0.6506м из-за rest/calibrated caps. Эта диагностика не воспроизводила точно исходную world orientation, поэтому её нельзя выдавать за окончательную причинную проверку.
- `corpse-shin-05`: настоящий native hold получен при HMD0.85, но обнаружена соседняя левая голень12 вместо рассчитанной правой13. Для задачи «голень» допустимы12/13; реальный segment сохраняется в отчёте. Forearm one-hand допускает8/9, а different-limb test по-прежнему требует конкретные разные стороны/actual bodies.

`corpse-shin-06` при HMD0.85, штатных collisions и arm reach: raw-palm positioning остаток0.30мм; захвачен dead ragdoll, stable native ID68334/body18, age16мс, segment12. Все natural motion phases удерживали реальный native hold.

| Фаза | Измеренный результат |
| --- | --- |
| Rest | reference-grip p95=27.1мм |
| Lift | p95=35.2мм; hips переместились7.15см |
| Wrist turn | controller45° → actual body46.2° |
| Stand | hips поднялись/переместились0.515м; p95=96.6мм, max121.8мм |
| Walk | player1.584м, corpse hips1.890м; p95=59.7мм, max114.5мм; hold непрерывен |
| Stick turn | p95=15.9мм; hold непрерывен |
| Drop | обе native/Lua стороны отпущены |
| Regrab | native ID68334/body8, segment13; reference-grip p95=2.9мм, max3.2мм |

В первом захвате reference body7 отличается от actual body18, поэтому reference-grip error включает движение reference-joint и не называется точной ошибкой PhysX контакта. Regrab использовал один и тот же reference/native body8. Пороги harness выявляют грубые сбои/потерю удержания, а не доказывают идеальную плавность.

Lateral +0.95м **не покрыл** constrained reach: фактическая raw-drawn дистанция p95=119мм, а не >450мм на3с. Поэтому общий `report.json` честно содержит FAIL (единственная непрошедшая фаза), хотя hold оставался активным. Критерий не снижался. Отдельный `--reach-mode floor` подготовлен для синтетического контакта с виртуальным полом, с тем же измеренным условием >450мм на3с; в игре он ещё не выполнялся. Он не заменяет natural standing/walking.

Фото/видео: `corpse-shin-06/shin-held_{L,R,SBS}.jpg`, `shin-regrab_{L,R,SBS}.jpg`, `shin-lift.mp4`, `shin-walk.mp4`; кадр `shin-walk-middle.jpg` декодирован штатным C++ `vframe.exe` на2.0с. В walk clip видно движение всего NPC и столкновение с краем кашпо; этот прогон не является изолированным тестом движения по пустой площадке. Все пути — под `scratch/goal-audit-20261117/`.

Восстановление `corpse-shin-06/restored.json`: PASS, position error0м, heading difference0.295°, equipment/money исходные, holds0, temporary NPC удалён; `cleanup.json` errors[]. После этого отдельный current-state probe подтвердил Game6676 жив, weaponOut=false, menuClosed, CorpseAudit=nil, NPCs0. Settings/controller state восстановлены. Ручное сохранение не вызывалось; позже root обнаружил реальные автосейвы тестовых состояний, поэтому эти проверки памяти не являются доказательством неизменности save files. Root сохранил резервную копию и обратимо архивировал идентифицированные тестовые autosaves, закрепив следующий launch за исходным AutoSave13. Runtime lease возвращён root, дальнейшие изменения сделаны offline.

## Дополнительные gates после review

`VRBodyInfo.w` теперь нормализован и закэширован как исходная carry mass, поэтому он **не доказывает** физический restore100→5. Добавлен отдельный on-demand `VRBodySolverMass`, обходящий MassOf. Harness до grip читает независимый physical mass baseline всех доступных ragdoll bodies; сохраняет `solverMass` отдельно от `reportedCarryMass`; проверяет override, сохранение100 при оставшейся руке и возврат5 после последней. Эти проверки должны выполняться до удаления тестового NPC. C++ regression намеренно отвергает случай normalized carry5 при physical mass100 после release. Для PID20040 root развернул первоначальный raw getter через guarded PhysicalBodyInterface.GetMass; source затем усилен прямым PhysX vtable getMass, но этот sourcedelta ещё не вошёл в PID20040. Если текущий API фактически показывает100→5, это будет достаточным практическим доказательством для текущего binary; новый запуск только ради getter не требуется.

Harness теперь до любых gameplay/config/input mutations добавляет собственный уникальный SaveLocksManager ADD request. Root установил на PID20040, что private `m_saveLocks` недоступен, а ObserveAfter не даёт ACK, хотя ADD/REMOVE работают. Поэтому exact membership используется там, где доступен; на текущем host допустим только независимо подтверждённый `GoalSaveGuard` token плюс текущие manager/engine locked checks. Root доказал свой exact token переходом REMOVE→обаfalse, ADD→обаtrue (`root-save-guard-20040.json`) до мутаций. Generic locked bit без этого proof не принимается; standalone без membership/root proof отказывается от мутаций. После успешного restore harness отменяет pending autosaves и запрашивает REMOVE только своей причины. Если own removal нельзя наблюдать, это явно отмечается, а root protection должна оставаться. При restore failure lock сохраняется для root repair. Семантическая Lua-регрессия покрывает оба verification paths, queued ADD/REMOVE, запрет мутаций без подтверждения, сохранение lock при ошибке и порядок invalidate→REMOVE. Runtime-проверка именно harness lifecycle ещё обязательна.

## Сессия 20040 и исправление точной контактной метрики

`corpse-shin-mass-20040b` доказал реальную массу на deployed RTTI getter: proxy68110/actual18, reference7, независимый baseline5 → solver100 при хвате, reportedCarry5 → physical5 после отпускания. Повторный захват actual19/reference8 повторил5→100→5. При синтетическом floor-block raw/drawn separation p95=1.2727м сохранялась4.4098с с активным native hold. Это отдельный stress-case, не естественная поза. Natural rest/lift/wrist/stand/walk тоже удерживали тело, но тогдашний analyzer ошибочно применял20см gate к reference7 и правильно сохранил FAIL, а не скрывал его. Артикуляция другого сустава не равна проскальзыванию actual18.

Независимый C++ анализ относительной позы actual actor к drawn wrist (`corpse-relative-review/relative-report.json`,138samples) подтвердил follow: actual wrist/body переместились при lift0.230/0.216м, stand0.820/0.856м, walk1.681/1.727м. Walk relative-position p95=72.2мм, max107.2мм. Это **не** exact contact error: свободное вращение вокруг якоря и асинхронный sample влияют на relative-body transform.

Это различие затрагивало production lost-grip timer. Новый `HeldGripContact.hpp` публикует расстояние между exact body-local `r.grip`, преобразованным текущей actor pose+origin, и desired world point фактического DriveTarget. Пятое Uint64 поле `VRPhysHeldBody` содержит микрометры, invalid=`UINT64_MAX`; первые четыре поля сохранены. Native target привязан к scene и prepare generation, fetch потребляет generation однократно. Failed prepare/fetch, другая scene или повторный fetch не освежают старую цель. Lua fast path принимает только active/matching-proxy/fresh valid error; при invalid новом поле пропускает timer, не возвращается к несовместимому reference-local point. Отдельный old-four-field fallback сохранён.

`held_contact_test.cpp` проверяет actual18/reference7 articulation, вращение вокруг якоря, origin rebase, отдельные контакты двух рук, invalid sentinel и scene/step freshness. Physics CTest5/5 и Lua corpse-driver regression прошли; независимый review scene/step gate PASS. Новая общая сборка для PID26944 включает это исправление и прямой raw PhysX mass getter. До нового live-прогона точный контакт и last-holder two-hand mass ещё не подтверждены в игре.

Ground-forearm/trunk и same-shin handover попытки20040 не дали подтверждённого paired hold. Их ошибки не объявлены PASS. Forearm при trackingY0.4 имел actualeye0.491м, shoulder0.733м и недоступную горизонтальную stance; trackingY не равен world eye height. Harness теперь измеряет плечи/текущую FK длину и ранжирует48 позиций по shoulder→palm distance при подтверждённом floor footprint. Raw orientation определяется через rawQ*inverse(drawnQ)*drawnPalmBasis; contact-induced drawn rotation не загрязняет Jacobian. Production reach caps не изменялись.

Финальный lease-return20040 (`glamdoc-live/corpse-lease-return-20040.json`): originalposition[-2281.9252929688,1707.9191894531,18.610000610352],747items/806112, styleNone,0temporaryNPC,0holds, CorpseAudit=nil, menuclosed/unarmed, root causal save guard retained. Собственные harness locks удалялись только после successful restore и invalidation pending autosaves. Root закрыл игру с сохранённым root lock.

## Точный live-контакт и двухручная масса: PID26944

Команда (игра уже запущена root, exclusive lease):

```powershell
py -3.13 -u tools/grasp_tests/corpse_live/run.py --pid 26944 --out scratch/goal-audit-20261117/corpse-exact-26944 --case shin --case same-part --pair-part shin --reach-mode floor --spawn-distance -2.5
```

`corpse-exact-26944`: настоящая правая голень segment13, actual75247/body8 **отличается** от Lua reference19. Все10 выполненных motion/drop фаз прошли; каждый held sample имел valid exact field5. Independent physical mass baseline5 → held100/carry5 → released5 подтверждены прямым PhysX getter новой DLL.

| Фаза | Exact contact p95 / max | Движение |
| --- | --- | --- |
| Rest | 1.93 / 1.96мм | удержание непрерывно |
| Lift | 2.69 / 2.73мм | hips0.229м |
| Sideways | 2.97 / 3.40мм | hips0.135м |
| Wrist | 1.70 / 1.88мм | actual body28.69° |
| Stand | 4.46 / 10.53мм | hips0.502м |
| Walk | 6.11 / 8.74мм | player3.927м, hips3.921м |
| Floor-block hold | 7.49 / 8.53мм | raw/drawn p95=1.046м, >450мм на4.456с |

Однако regrab остановился **до** нового grip: bracketed raw baseline сместился на31.26мм. Raw quaternion оставался неизменным, поэтому Jacobian не принят. Общий `report.json` остаётся FAIL/incomplete; никакие gates ради PASS не ослаблялись. Restore этого worker: position0м/rotation0.063°, mass5, noNPC/noHold. Двухручный случай из первого запуска не выполнялся и был проверен отдельно на свежем fixture.

Второй запуск — тот же CLI с `--out .../corpse-paired-exact-26944 --case same-part --pair-part shin --spawn-distance -2.5`. Обе руки действительно держали **один** actual75368/body8; right reference19, left reference8. На2holders raw mass100, после release левой при1holder всё ещё100, после последнего release5. Исходная carry mass обеих рук5. Контакт valid100% каждой руки. Body повернулся25.57° при заданном повороте controller baseline35°. Remaining right hand после release: exact p95=2.27мм.

У двухручного хвата обнаружено **ограничение качества**: rest exact p95 left64.4/right58.9мм; rotation p95 left157.2/right155.5мм, max159.0мм. Coarse20см regression gate пройден, хват не потерян, но это не точная посадка обеих ладоней и не основание назвать solver идеальным. Требуется отдельный разбор совместимости двух desired anchor points и реальной raw/drawn геометрии; производственные thresholds/forces не повышались.

Последующий `corpse_probe pair-geometry` анализ (`corpse-paired-exact-26944/pair-geometry.json`) установил конкретную несовместимость fixture: raw wrist span≈516мм, raw desired-grip span≈466мм; расстояние actual body-local grip points≈147мм. Правый point восстановлен приближённо из последних5 single-holder samples (spread4.82мм, ограничение nativeerror/asynchronous sampling явно записано), левый point exact, поскольку reference8=actual8. Drawn desired span вырос с≈245мм в rest до≈447мм при повороте. Для rigid span147мм минимальная средняя ошибка двух desired anchors — половина разницы длин, p95≈149.8мм; измерено≈157мм. C++ отдельно реконструировал ошибку **точного** левого anchor из bodyPose/F.g и получил p95=157.294мм против native157.205мм. Это подтверждает контракт field5 и показывает, что большая часть residual геометрически неизбежна для текущих слишком широко расставленных controller targets. Это не доказательство плохого torque response. Следующий meaningful paired fixture должен сначала совместить raw desired anchor positions с actual body contacts через уже измеренные controller Jacobians, затем вращать саму линию контактных целей; production thresholds/caps менять для этого не требуется. Аналитический self-test: rigid20см/targets40см дают минимум10см ошибки на руку.

Media под `scratch/goal-audit-20261117/`: `corpse-exact-26944/shin-held_{L,R,SBS}.jpg`, `shin-walk.mp4`, `shin-lift.mp4`, `shin-floor-block-hold.mp4`; `corpse-paired-exact-26944/same-part-held_{L,R,SBS}.jpg`, `same-part-rotate.mp4`, `same-part-one-left_{L,R,SBS}.jpg`. Walk frame на2.0с (`shin-walk-middle.jpg`, C++ vframe decoder) показывает корпус и движение, но удерживаемая правая кисть/NPC вне кадра; этот клип не доказывает идеальную визуальную посадку. Paired SBS показывает две руки у поднятой ноги/обуви.

Оба workers завершены, игра26944 осталась живой. Последний independent audit обнаружил748items вместо747 при правильных equipment/money. Сравнение **всех** combinedIDs/quantities с `weapons-optics-aligned-ab-26944/original.json` показало единственный extra `1896218387863961470ULL`, `<TDBID:C9A90E1E:19>`, quantity1, Gen_Misc/tagProp, без display name; missing0/quantityChanges0. Причина его появления пока не установлена. Только этот проверенный extra удалён по current ItemID с guards hash/record/quantity, root save lock оставался активен. После удаления exact map совпал полностью.

Финальные доказательства: `corpse-inventory-before-restore-26944.json`, `corpse-inventory-26944.json`, `glamdoc-live/corpse-inventory-restore-26944.json`, `glamdoc-live/corpse-lease-return-26944.json`. Итог747items/806112/styleNone, noNPC/noHold/CorpseAudit=nil, menuclosed/unarmed, GoalRoadScene.restored=true, исходная lease position[-2281.9252929688,1707.9191894531,18.603485107422]. Root causal guard `cpvr_goal_root_0d038e181dc04efe836c0dc3ee4289bf` retained. Controller/settings/head восстановлены, оба `cleanup.json` errors[]. Lease явно возвращён root.

Harness gap исправлен после сессии: begin сохраняет полный inventory map, restored требует exact combinedID/quantity equality, unknown extras блокируют success/unlock для точного review, а не удаляются наугад. C++ restore gate больше не принимает только equipment/money. Lua regression покрывает extra Prop, missing/replaced unique IDs и quantity changes; CTest1/1 + inventory/save-lock semantic tests PASS. CET загрузка именно последнего inventory guard ещё не проверялась в новой сессии.

## Live-проверка в общей игровой сессии

Игра должна быть запущена отдельно от сценариев проверки, как требует пользователь. Ошибка теста должна оставить её работающей. Сначала закончить baseline других подсистем, затем установить согласованные DLL и Lua в рамках одного планового перезапуска. После всех тестов закрыть игру; не открывать RenderDoc replay до её остановки.

1. В OpenXR Simulator взять мёртвого NPC за голень, затем за предплечье: присед → захват → покой 3 с → подъём на 30 см → движение вбок на 25 см → поворот кисти ±45° → вставание → ходьба вперёд/назад и поворот стиком → отпускание. Повторить другой рукой. Снимать оба глаза и `VRPhysHandFrame`, `graspState`, позиции слотов, physics trace. Не считать отсутствие записи в буферизованном логе доказательством отсутствия хвата.
2. Отдельно удерживать контроллер вне длины руки/под полом более 3 с, пока видимая ладонь удерживает тело. Ожидание: хват сохраняется. Потом реально отвести тело от ладони (или создать препятствие) более 45 см на 3 с: безопасное отпускание должно сохраниться.
3. Взять одну часть обеими руками, сохранить вращения контроллеров и поднять одну руку. Проверить поворот тела по линии двух хватов, отсутствие рывка при отпускании одной руки, сохранение второй. Проверить два разных участка: бедро+голень и обе руки NPC.
4. Для двух разных участков проверить лог `[corpse] proxy ... bodies ...`: после установления каждого якоря не должно появляться повторного полного измерения при каждом кадре. Профиль CPU должен подтвердить снижение вызовов `CorpseMeasure`; визуально не должно быть конкурирующего tow-рывка второй руки.
5. После отпускания наблюдать до естественного покоя, особенно когда двигается только последняя удерживаемая часть. Не должно быть раннего freeze. Повторный захват должен пробудить тело.
6. Повторить основные сценарии на SteamVR null driver при доступной подаче poses/input. Сопоставить физические показатели при разных частотах; мониторная картинка сама по себе не подтверждает VR-плавность.

`scratch/goal-20261115/corpse_hold.py` и результаты `corpse-12`, `corpse-15` полезны как исторические fixtures/набор фаз. Старый сценарий нельзя считать проверкой нынешнего поведения без нового прогона, а движение/генерацию поз нужно подавать через C++ helpers согласно глобальным инструкциям.

## Оставшиеся риски и gates

Actual-anchor/original-mass/exact-contact пакет собран и проверен one-hand и same-body paired на PID26944. Same-body mass2→1→0 доказана100→100→5. Двухручная rotation показала до159мм contact residual и нуждается в разборе геометрии. Ground forearm и different-limb paired пока не подтверждены; constrained raw/drawn separation пройдена20040/26944, regrab нановойDLL остановился наfixture calibration, SteamVR остаётся. Вариант выбора другой PhysX-части рядом с исходным Lua anchor подтверждён и проверяется по completed native IDs, а не по rec.index. Численные проверки не заменяют измерение CPU, задержки и проскальзывания в движущемся NPC/game scene. Настройки силы удержания не повышались для получения PASS.
