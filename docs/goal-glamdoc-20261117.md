# Glamdoc в SAO menu, 2026-11-17

Статус: реализация, standalone-проверки и live OpenXR Simulator проверки в существующих процессах22188/26944 выполнены. Проверены загрузка в Lua host/CET, реальный stock продавца, Gorilla preview/purchase/apply/cancel/remove и ограничения. На26944 остальные три ArmsCW прошли функциональный preview/colors/cancel/close/reopen; визуальная чистота последовательной смены всех arm meshes ограничена ошибкой установочного fixture (см. ниже). SteamVR null driver, long sleeves и исходный quest-driven dialogue остаются отдельными gates. Игра не запускалась и не закрывалась этим агентом; временные изменения восстановлены в памяти, runtime lease возвращён основному агенту. Ручное сохранение не запрашивалось, но позже root обнаружил автосейвы раннего тестового состояния — см. уточнение ниже.

## Реализованный поток

- `vendor_visit.lua` распознаёт `NPCPuppet.IsGlamdoc()`, а также перехватывает `MenuScenario_Vendor.OnSwitchToGlamdoc`. Обычный vendor request и native quest-driven vendor handoff приводят к отдельной секции `glamdoc` SAO menu. Это собственные world-space ink widgets; fullscreen Glamdoc не открывается как пользовательский экран.
- Четыре категории: Projectile Launcher, Mantis Blades, Monowire, Gorilla Arms. Каталог собирается из инвентаря игрока и штатного `VendorDataManager.GetGlamDocItems`. Купленные записи не дублируются в продаже, EP1-предметы скрываются без дополнения, recipes/side upgrades/preview items фильтруются.
- Совместимость определяется по реально установленному ArmsCW. Несовместимый стиль можно купить заранее, но нельзя примерить/применить. Без ArmsCW примерка и применение недоступны.
- Preview использует `ApplyCyberwareCustomizationDataRequest` на V, поэтому пользователь видит стиль на собственных VR-руках. Состояние `GetSavedData()` и equipment slot остаются прежними до Apply. Это позволяет отменить preview без сохранения ID ещё не купленного предмета и не требует отдельного doll render target.
- Две палитры по пять цветов из штатного brand record; swap и default colors; снятие стиля; выход из примерки возвращает предыдущее редактируемое состояние.
- Покупка требует подтверждения внутри SAO. В момент подтверждения заново проверяются реальный stock, ownership, availability, DLC и баланс. `BuyItemFromVendor` вызывается один раз, повторный запрос блокируется до подтверждения получения предмета. Preview не выдаёт предметы и не списывает деньги самостоятельно.
- После получения предмета сохраняется vendor spend и штатные threshold/`glam_bought` facts. Закрытие во время покупки отменяет preview; получение уже заказанного предмета учитывается, без неожиданной установки.
- Apply сохраняет стиль/цвета через штатный `SaveAndApplyCyberwareCustomizationDataRequest` и `EquipRequest`/`UnequipRequest`, с правильным вариантом long sleeves. Cancel восстанавливает штатную сохранённую внешность. Покупки остаются в инвентаре. `glam_equipped`, `cyberware_changes_save`, `glamdoc_color` обновляются при подтверждённом изменении.
- Закрытие, уход, shutdown/hot reload завершают preview через тот же lifecycle. Права магазинной сессии не выдаются произвольно: вход требует настоящего активного Glamdoc visit.

## Файлы

Новые `mods/cet/CyberpunkVRPort_Menu/modules/glamdoc_data.lua` и `glamdoc_view.lua`; изменения `modules/vendor_visit.lua`, `modules/menu.lua`. Диагностика доступна через `GetMod("CyberpunkVRPort_Menu").glamdoc()`.

Все CET imports используют slash-separated пути. Тяжёлой генерации численных данных/поз эти модули не выполняют.

Основание API и правил: актуальные декомпилированные `scratch/menu-load/scripts/cyberpunk/UI/fullscreen/glamdoc/glamdoc.script`, `glamdocInventoryController.script`, `cyberpunk/UI/fullscreen/vendor/vendorScenario.script`, `vendorHubMenuGameController.script`, `core/systems/cyberwareCustomizationSystem.script`, сигнатуры `orphans.script`. Формат Color дополнительно сверён с SDK reflection header `RED4ext/Scripting/Natives/Generated/Color.hpp`.

## Проверено вне игры

- LuaJIT syntax для новых модулей и изменённых Menu/vendor модулей.
- `tools/vr_menu_tests/glamdoc_test.lua`: полный data module, ownership/DLC, четыре arm categories, неподходящий/отсутствующий ArmsCW, preview без persistent write, палитры/swap/default/cancel/remove, недостаток денег, смена stock availability, один асинхронный заказ, подтверждение ownership перед equip, сохранение preview colors, long sleeves, vendor spend/thresholds, receipt после закрытия.
- `tools/vr_menu_tests/vendor_handoff_test.lua`: прежний native hub lifecycle плюс выбор Glamdoc и подавление fullscreen transition во время handoff.
- `tools/vr_menu_tests/appearance_test.py`: 32 теста успешно; mirror editor не регрессировал в проверяемых сценариях.
- `git diff --check` для изменённых файлов: ошибок нет.

Запуск Lua-тестов через имеющийся `lupa.luajit21`: загрузить файл и передать абсолютный путь репозитория первым аргументом chunk. Standalone LuaJIT и mock Game API не подтверждают CET loader/RTTI доступность.

## Live OpenXR Simulator: доказательства

Сессия прикреплена к уже работающей игре. Изменённые четыре Lua-модуля развёрнуты с резервной копией в `scratch/goal-audit-20261117/glamdoc-live/backup`, штатный `.hot()` вернул `ok`. Тестовый `Character.test_glamdoc_01` оказался с пустым штатным stock, поэтому для проверки создан временный NPC с реальным record `Character.wat_kab_glamdoc_01`. Его `VendorDataManager` получил восемь настоящих предметов; вызов `UISystem.RequestVendorMenu` открыл секцию `glamdoc` со штатными ценами. Это проверка engine vendor request, а не доказательство корректности всего оригинального dialogue quest.

Обнаружены и исправлены два дефекта, которых не выявили mock-тесты:

- Самостоятельная панель наследовала расположение обычной правой колонки и обрезалась в одном глазу; крупный универсальный шрифт налезал на соседние категории. Панель центрирована (x=710, width=1780 в canvas 3200), кнопки получили отдельный размер шрифта 32 и перенос. Повторные L/R снимки подтверждают исправленный layout.
- Для V без старого стиля `saved.switcherName=None`. Одного Restore недостаточно для отмены нового preview switcher. Сессия теперь хранит затронутые каналы, сбрасывает их перед восстановлением и при выключении preview. Для случая None добавлена регрессия.

Результаты записаны в `scratch/goal-audit-20261117/glamdoc-live/`:

| Проверка | Доказательство |
| --- | --- |
| Настоящие каталоги и корректная секция | `real-open.json`, `real-stock.json`, `real-state.json` |
| Видимые изменения двух VR-рук и обе палитры | `preview_L.jpg`, `preview_R.jpg`, `preview_SBS.jpg`, `colors_L.jpg`, `colors_R.jpg` |
| Preview/colors не записываются в сохранённую настройку | `colors-persistent.json`, `cancel-preview-persistent.json` |
| Отмена preview после исправления switcher | `cancel-fixed-state.json`, `cancel-fixed_L.jpg`, `cancel-fixed_R.jpg` |
| Покупка и установка Militech Strong Arms | `purchase-confirm_SBS.jpg`, `verify-purchase.json`: ровно €$15000, 806112→791112, item owned и в ArmsCWStyle, цвета 3/4, spend=1500001 |
| Owned style повторно не продаётся; Cancel возвращает прежние цвета | `reopen-owned.json`, `verify-cancel-owned.json` |
| Несовместимый Mantis style: preview заблокирован, покупка доступна | `click-glamdocArea-2.json`, `incompatible-purchase_SBS.jpg` |
| Закрытие сразу после заказа несовместимого стиля | `close-purchase.json`, `verify-closed-purchase.json`: ровно €$10000, предмет получен, меню не открылось вновь, прежний style/colors не изменились |
| Remove реально снимает style и выключает customization | `verify-remove.json` |
| Без установленного ArmsCW preview не работает | `no-arms-verified.json`, `no-arms-preview-blocked.json` |

Клики проходили через реальный `menu.hit`/`menu.click` с координатами видимых SAO кнопок. Подача жестов контроллером именно в Glamdoc не выполнялась; общий pointer/scroll механизм переиспользуется. Первоначальные диагностические ошибки (JSON смешанных ключей и два клика в один debounce-интервал) останавливали только соответствующий тест, не игру; harness исправлен.

Снимки сохранены через `simxr.shot(eyes='both', eye_size=1600)` в собственном аспекте каждого глаза. Рендерились настоящие руки, а не native inventory puppet. На GPU HUD во время части снимков были spikes; их причина этим прогоном не изолировалась. Эти картинки не используются как benchmark производительности Glamdoc.

## Восстановление в памяти после live

`restored-evidence.json` содержит успешные assert-проверки:

- money=806112; inventoryCount=747; missing=0, extra=0; проверены все исходные ItemID и количества;
- исходный `Items.AdvancedStrongArmsLegendary`, rng_seed=2641956506, снова в ArmsCW; ArmsCWStyle пуст;
- customization: enabled=false, switcher=None, index=0, цвета 0/0; исходный ItemID None;
- все затронутые quest facts и vendor spend восстановлены (spend=0), stock снова 8, vendor money=0;
- позиция `[-2281.9252929688, 1707.9191894531, 18.610000610352]` не изменилась;
- временный NPC удалён, меню закрыто, тестовые глобальные ссылки освобождены (`globals-released.json`).

Два купленных экземпляра возвращены продавцу вместе со stock, затем временный NPC удалён; игроку возмещены ровно €$25000. Возвращённые IDs: `Items.MilitechStrongArmsCosmetic`, rng_seed=1305824235; `Items.ArasakaMantisBladesCosmetic`, rng_seed=1784832383 (`restore.json`).

**Уточнение о сохранениях:** восстановление памяти не отменило уже созданные игрой автосейвы. Root установил, что следующий сеанс загрузил тестовый AutoSave0 с приобретённым Glamdoc item и €$791112. Он сделал резервную копию270 файлов и обратимо архивировал тестовые AutoSave0/1/14/15; исходный AutoSave13 сохранён и выбран явно для следующего запуска. Поэтому прежнее утверждение «save не записывался» было слишком сильным: тест не вызывал ручное сохранение, однако автоматические сохранения происходили. Дальнейшие мутации допускаются только после проверки штатного SaveLocksManager lock и `Game.IsSavingLocked()`, с отдельным root lock до выхода из игры и отменой pending autosaves.

## Сценарий дальнейшей проверки

1. После завершения baseline развернуть оба новых модуля и два изменённых модуля в действующую Lua-папку порта, вызвать обычный hot reload либо использовать общий плановый перезапуск. Проверить логи на import/RTTI ошибки; `glamdoc()` до визита должен показать `active=false`.
2. Сохранить до теста реальные money, inventory, ArmsCW/ArmsCWStyle и все поля `CyberwareCustomizationSystem:GetSavedData()`. Использовать тестовый save для покупок и не перезаписывать пользовательский save.
3. Войти через реальный Glamdoc dialogue/vendor request. Должна появиться секция `glamdoc`, не `trade`; должна исчезнуть native menu presentation и вернуться world interaction. Проверить повторный вход после закрытия, чтобы vendor dialogue не завис.
4. Установить по очереди четыре ArmsCW в тестовом сохранении. Для каждого: выбрать owned style, preview vendor style, переключить оба цвета, swap/default, Cancel Preview, Cancel всей сессии. Сравнить saved state и equipment до/после Cancel; купить несовместимый стиль должно быть возможно, примерить его — нет. Проверить отсутствие ArmsCW.
5. Снять оба глаза через simulator capture. Проверить ширину панели/scroll/hit boxes, палитры, confirm modal, наличие VR-рук в видимой зоне и действительно меняющиеся материалы/геометрию. Не принимать голое изменение `GetSavedData()` за визуальную проверку.
6. Купить один совместимый preview через Buy and Apply. Проверить ровно одно списание штатной цены, ownership/equipment, оба цвета, long-sleeve variant, facts/spend. Проверить покупку без установки несовместимого стиля и Cancel после покупки. Проверить закрытие во время очереди покупки, отсутствие повторного заказа и самовольного equip.
7. Повторить на SteamVR null driver в общей сессии. Проверить, что после закрытия нет оставшегося menu capture, preview state, vendor blackout или лишних render targets.

## Ограничения подтверждения

Использованные API/конструкторы, включая protected customization setters, проверены в реальном Lua host/CET. Preview на VR Gorilla Arms визуально работает; Launcher/Mantis/Monowire прошли функциональный sweep, но cross-variant mesh residue требует повторной визуальной проверки после корректного staged equipment transition. Long sleeves и SteamVR остаются отдельными live gates. Короткая косметическая install animation штатного vendor hub не воспроизводится собственной SAO панелью: original quest-driven dialogue continuation не подтверждён clone NPC. До этого пункт цели нельзя считать полностью завершённым.

## Подготовленный короткий sweep трёх оставшихся типов

Attach-only helpers: `scratch/goal-audit-20261117/glamdoc-sweep.lua`, `glamdoc_sweep.py`, `glamdoc_sweep_test.py`. Production Menu для этого sweep не меняется. В baseline исходного AutoSave13 имеются только Gorilla Arms, поэтому Launcher/Mantis/Monowire устанавливаются через три отдельных временных exact ItemIDs на основе штатных `Items.AdvancedProjectileLauncherLegendary`, `Items.AdvancedMantisBladesLegendary`, `Items.AdvancedNanoWiresLegendary`; каждый ID сохраняется до GiveItem и удаляется только после возврата исходного ArmsCW. Покупок в этом sweep нет.

До первой мутации обязательны retained causal root SaveLocksManager token, текущие engine+manager saving locks, собственный unique ADD и invalidation pending autosaves. Снимок содержит полную combinedID/quantity карту инвентаря, ArmsCW/Style, weapon/clothing slots, активное оружие, все поля saved customization (включая brand/maxTac), money, pose, затронутые facts, затем stock/money/spend продавца. В finally проверяются exact inventory/equipment/facts/colors; неизвестные extras блокируют unlock для точного review. Root lock не удаляется. C++ `corpse_probe` генерирует статичные poses/spawn point и проверяет геометрию восстановления; Python выполняет I/O/IPC/capture.

Для каждого типа: actual ArmsCW equip → unarmed → Glamdoc SAO → реальные hit/click по видимым кнопкам → Preview → цвета3/4 → Swap → Defaults → Cancel Preview → повторный Preview → Cancel всей сессии → inactive vendor blackboard/native menu false → повторный вход/закрытие. L/R/SBS сохраняются для preview/colors/cancel. Первая категория должна использовать нативную очередь `inkMenuInstance_SwitchToScenario`, следующие — `UISystem.RequestVendorMenu`. Это проверяет два технических входа и завершение vendor visit; clone NPC не доказывает оригинальный world-dialogue quest. Observers сохраняют только имена событий, не постоянные ссылки на controllers.

Первый bootstrap PID26944 (`glamdoc-variants-26944`) остановился до создания arm items: новый clone ещё не дал валидный Glamdoc record. Capture49 остался `shot_pending`, size0, при живом процессе и работающем bridge; после предшествующего дальнего teleport в логе оставался loading-screen up. Root получил live lease для проверки loading/VR, без запуска/закрытия игры. Exact tagged NPC удалён,0temporaryitems, inventory747/806112 и original equipment/saved/facts совпали (`glamdoc-live/sweep-bootstrap-repair.json`). Own lock `CPVR_GlamSweep_ecd0de156d6d4c5dbcfe444bb3c0ce60` и `GoalGlamSweep` оставлены для продолжения; после input restore наблюдалось Y+4.1см, что отдельно сообщено root. Никакой из трёх новых вариантов ещё не считается проверенным.

Async bootstrap исправлен source-only: ждать MarketSystem attachment, валидный record и непустойstock, затем фиксировать baseline. `--resume-from` использует сохранённый original map/own token и требует тот же player entity/root guard. Пример только после повторной передачи lease и устранения loading:

```powershell
py -3.13 -u scratch/goal-audit-20261117/glamdoc_sweep.py --pid 26944 --out scratch/goal-audit-20261117/glamdoc-variants-26944-resumed --resume-from scratch/goal-audit-20261117/glamdoc-variants-26944
```

Offline Python compile/Lua syntax/guard tests PASS: causal token, current engine+manager state, token continuity и сохранение snapshot/lock при failed restore. Эти тесты не подтверждают CET runtime загрузку последнего helper delta.

## Результат sweep PID26944

Первый pending capture49 **не оказался black-VR/loading репро**. Root измерил до смены focus1с стабильного native HUD:91submitted frames/46source serial advances, menuMode0/MenuSource4, а не loading2; две свежие eye-capture заявки50/51 завершились. Доказательства `capture-stall-26944/before-focus.json` и `fresh-{after-focus,background}_SBS.jpg`. Это единичная stale capture request; её исторический loading-screen log нельзя считать актуальным состоянием сцены.

Synthetic `inkMenuInstance_SwitchToScenario.Init(...,panel)` вызвал вход Vendor scenario с пустой native panel, не создал допустимый SAO visit и был завершён штатным `OnVendorClose`; зафиксированы enter/close/leave/hub-uninitialize. Этот искусственный payload не подтверждает production bug или оригинальный quest handoff и больше не использовался. Далее все три категории входили через проверенный `UISystem.RequestVendorMenu`.

Окончательный worker15184, `glamdoc-variants-26944-complete`, exit0: Launcher, Mantis, Monowire последовательно прошли preview, обе палитры3/4, swap4/3, defaults, Cancel Preview, повторный preview, Cancel всей сессии, inactive vendor blackboard, повторный вход/закрытие. Все saved customization поля, Style slot и money оставались исходными. Покупок0. Категория совпадала с фактическим ArmsCW, штатный stock был непустым. В `report.json` PASS относится к этим функциональным assertions.

Фото каждого этапа — `{launcher,mantis,monowire}-{preview,colors,cancel}_{L,R,SBS}.jpg` в том же каталоге. Панель читается в обоих глазах. Но визуальный review выявил residue при быстрой смене arm types: в Monowire preview/cancel правая модель сохраняла часть Mantis рисунка, а Cancel Mantis оставлял смешанный материал слева. Нельзя объявить визуальный gate всех трёх рук закрытым только по equipment/saved state. Найден конкретный fixture gap: он отправлял прямой EquipRequest без uninstall/drain. Уже существующий `CyberpunkVRPort_VRIK.EquipCyberware` документирует и выполняет необходимый порядок holster→native detach/visual attach→Unequip ArmsCW→Equip→drain; иначе additional appearances могут пережить смену слота. Production Glamdoc bug этим sweep не доказан. Следующая визуальная проверка должна применять этот staged transition, без ослабления проверок.

Восстановление дополнительно завершено штатным uninstall/reinstall **точного исходного** Gorilla ID, затем поздним holster и3стабильными native samples: ArmsCW `7135700716658349943ULL`, actual RightArm `Items.StrongHolsteredFists`, visual attached=true, WeaponLeft/Right=nil, IsEquipPending=false. `visual-restore.json` фиксирует actual attachment proof, а не выдаётся за новый финальный фото-аудит. Первая попытка восстановить Quaternion из Lua table была отклонена RTTI до Teleport; исправленный finish использовал существующую world orientation, после чего position восстановлена точно.

Exact temporary base IDs: Launcher3590645216725494168ULL, Mantis228800144870837539ULL, Monowire4088347660666637392ULL. Удалены также только отсутствовавшие в baseline native holstered items, разрешённые **через HolsteredItem record соответствующего временного импланта**: `Items.ProjectileLauncherHolsteredFists`3741154548957201485ULL, `Items.MantisBladeHolsteredFists`10895408172809788143ULL, `Items.NanoWireHolsteredFists`17359348999417299555ULL. Количество каждого1; исходные items не удалялись. См. `temporary-removed.json`.

После восстановления полный map равен baseline:747items,806112, missing/extra/quantityChanges0; original equipment/facts/colors/styleNone совпадают. Временный vendor удалён, меню/board закрыты,0holds, GoalGlamSweep/GoalGlamVisualRestore=nil, исходная position[-2281.9252929688,1707.9191894531,18.602554321289]. Собственный lock REMOVE запрошен только после успешной проверки/invalidation, root causal lock retained. Финальный proof: `glamdoc-live/glamdoc-lease-return-26944.json`; `glamdoc-variants-26944-complete/{restored,cleanup,lock-release,visual-restore}.json`. Lease явно возвращён root, игра осталась живой. Native original dialogue, long sleeves и SteamVR этим sweep не охвачены.
