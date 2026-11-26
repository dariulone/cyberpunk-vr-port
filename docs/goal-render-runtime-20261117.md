# Аудит OpenXR / SteamVR, 17 ноября 2026

Исходная рабочая ревизия: `7d671e5d`. Этот отчёт покрывает пункты 3, 6, 7, 8, 11, 12 цели. Изменения пока не закоммичены; запуском игры и общей сборкой управляет основной агент. Проверки ниже не являются проверкой физического шлема.

## Чёрный gameplay и редкие изображения в Steam Frame

[Issue 233](https://github.com/dariulone/cyberpunk-vr-port/issues/233) описывает чёрный мир после меню при продолжающейся игре; есть отчёт VDXR с выключенным FG. [Issue 243](https://github.com/dariulone/cyberpunk-vr-port/issues/243) описывает Steam Frame/SteamVR, нормальные меню и почти отсутствующий gameplay в шлеме. Проанализирован именно приложенный `C:\Users\dariulone\Downloads\cyberpunkvrport.log`, размер 1 661 064 байта.

В логе 172 окна `[xrrate]`, из них 62 содержат ненулевые Presents и EMPTY, но ноль кадров с изображением. Максимум — семь последовательных таких окон. Например, строки 15309–15408: около 24–25 Presents/captures в секунду и 71 XR cycle/s, но ни одного изображения. `captures ... ok` продолжает расти без no-slot/fence-timeout. Значение около 60 FPS из описания нельзя считать измерением gameplay: в этих окнах сам лог измеряет около 25 Presents/s. При возвращении в меню поток изображений восстанавливается, строки 16339–16489.

Измеренное давление памяти существенно: максимум local 7005 MB при DXGI budget 5916 MB; другой поздний срез 6857/5981 MB. Это подтверждает превышение бюджета, но само по себе не доказывает утечку и не определяет владельца памяти.

Найден воспроизводимый логический дефект capture selection. CPU публикует снимок сразу после постановки команд в очередь. Раньше selectable metadata всегда содержали только latest и непосредственно previous. Если GPU отстаёт на три кадра, оба всё время не готовы: завершившийся кадр уже исчез из metadata. Четыре существующих GPU-слота не помогают, потому что информации о готовом кадре больше нет. После секунды без нового submit `ResubmitLastFrame` прекращает повтор и отправляются пустые layers. Это соответствует симптому лога; причинную связь с конкретным ПК ещё нужно подтвердить live.

Исправление в `OpenXRCapture.cpp` и `CaptureFrontier.hpp` сохраняет непоказанный predecessor до показа либо замены более новым **уже завершённым** снимком. Оба eye-slot этого predecessor защищены от перезаписи, включая fallback-проход поиска слота. При полном занятии слотов пропускается capture, без CPU wait и без добавления GPU-ресурсов. `m_lastSubmittedSerial` стал atomic, поскольку теперь его читает producer.

Добавлен `[xrready]`: unfinished newest / completed predecessor / busy submit allocator в секунду. Финальный `NOT PAIRED -- this is the defect` из пользовательского лога не является доказательством ошибки порядка OpenXR: started, returned и begin считывались в разные моменты с другого потока. Диагностика теперь сообщает `IN-FLIGHT SNAPSHOT`, не выдавая снимок независимых счётчиков за доказанную поломку.

Регрессия `capture_frontier` моделирует 300 кадров, four-slot pool, GPU latency три кадра, leases и давление от consumer/FG endpoint. Старый latest/previous отбор в этой модели не получает ни одного готового кадра; новая политика продолжает delivery. Это проверка политики и владения, не тест реального D3D12 исполнения.

После независимого review дополнительно исправлены queued-write retirement и consumer AddRef race при resize обоих capture pools, неверный global-serial state barrier на первых slot uses, unsafe submit resize после 1s timeout, устаревший tracking-origin epoch и выбор copy queue для обычного non-SteamVR кадра. Подробности и реальные WARP fence/queue проверки приведены в `docs/goal-memory-static-20261117.md`. Framegen synthetic output всегда остаётся за своей фактической synthesis queue, даже если настройки изменились между началом XR cycle и выбором кадра.

## FOV / разрешение Steam Frame

В приложенном логе runtime рекомендует **2600×2600** на глаз (строки 344–345), реальный capture **2040×2056**, backbuffer SBS **4080×2056** (строки 1770, 1796). Утверждение о 3200×2000 этим логом не воспроизводится.

Текущий preset Steam Frame в `LauncherDialog.cpp` имеет aspect примерно 0.9936, рассчитанный из frustum tangent spans, а не прямоугольные 3.2k×2k. Размеры берутся из launcher; SteamVR recommendation не переопределяет выбранный render target. `xrLocateViews` обновляет углы, `GetAsymmetricEyeWindows` передаёт их общему `Windows`, и эти же окна используются для render и submit. Изменение SteamVR FOV не меняет размер swapchain игры автоматически.

`Windows` сохраняет квадратную плотность пикселей: при сужении одного направления может расширить другое до aspect выбранного target. Поэтому ожидаемый пользователем строго независимый crop по четырём сторонам не совпадает с текущей политикой fit. Произвольное приравнивание aspect к физической панели было бы неверным для асимметричных линз.

Новая проверка `runtime_frusta` использует точные углы из лога: L(-59.139, 50.359, 49.396, -60.061), R(-50.287, 59.187, 49.905, -59.555), и изменения inner/outer/up/down. Проверены совпадение лучей render/submit, покрытие видимой линзы и aspect. Все изменения углов меняют окна. Проверка не устанавливает, какие углы конкретная версия SteamVR отдаёт после UI slider; нужны runtime measurements до/после изменения.

## Waypoint markers

Текущий shader переносит помеченную геометрию в world space и проецирует через камеру каждого глаза; native stereo-widget disparity для этих маркеров исключён. Камера берётся из текущего recording view, не из глобального указателя MAIN. Eye index соответствует native 1=left / 2=right.

Пересобраны production shader tests: **13/13 PASS**, включая reprojection, world_shader, batched text, оба MAIN assignment, native 2.5 shader parity и linkage с native pixel shaders. Противоположное смещение маркеров на Quest 2 + SteamVR + VD этим не воспроизведено. Нужен screenshot каждого глаза с одним и тем же world target, включая yaw/roll и края поля зрения; одинаковые экранные координаты в двух глазах не являются условием правильности.

## Движущиеся машины, frame generation и PR 203

[Issue 213](https://github.com/dariulone/cyberpunk-vr-port/issues/213) описывает зависящую от запуска фазу real frames против display clock. Текущая ветка уже отличается от ревизии issue: `PhaseLockedPacing`, `CycleClock`, проверка периодов SteamVR и отдельная HIGH-priority runtime queue уже реализованы. Проверки policy/cadence/presenter_timeline/pacing/phase_pacing/cadence_lock/presentation_period: **7/7 PASS**. В сценарии фазового сдвига старый free-running timer даёт worst 23.8% repeats, текущая политика 0%; это модель планировщика, не измерение SteamVR scanout.

[PR 203](https://github.com/dariulone/cyberpunk-vr-port/pull/203), head `db43c233d0a9087e196a2cf3a0d38b5845239109`, основан на `e9139450`. Сравнены diff и implementation. Он объединяет изоляцию presentation/synthesis queues, compositor refresh feedback, producer GPU gate, prepare-ahead и V557. Часть архитектуры уже есть в текущей ветке; его изменения старого adaptive timer не заменяют нынешнюю phase lock. Полный merge без адаптации не рекомендуется.

Для качества стекла V557 — отдельный алгоритмический кандидат: camera prewarp + confidence-aware optical-flow reconstruction вместо текущего OFA + FidelityFX с engine guides. В PR заявлено улучшение на конкретных bike captures; это не обещание общего повышения FPS или качества. Переносить стоит отдельным backend только после одинаковых сохранённых входов и двухглазой оценки, сохраняя LGPL provenance. В этом аудите backend не добавлялся.

Движение машины может быть ступенчатым при правильном head timewarp: повтор одного старого изображения компенсирует вращение головы, но не обновляет положение машины. Поэтому monitor FPS и число accepted layers не доказывают плавность динамических объектов. Исправленный capture starvation устраняет один источник длительных повторов/чёрных кадров; остальные рывки требуют сопоставления source serial/age, real/generated/repeated counts, compositor presentation и видео движущейся машины. `perDisplay` теперь описан в коде корректно: наличие картинки, а не уникальных отображённых кадров.

## Live acceptance для основного агента

1. Общая Release сборка и совпадение DLL/PDB. Один запущенный экземпляр игры, capture replay закрыт.
2. Один save/маршрут с движущимися машинами; inventory/map по 10–20 циклов, затем ходьба/езда. OpenXR Simulator и SteamVR null раздельными сессиями; FG off/on отдельно в пределах сессии, где возможно.
3. При нагрузке, создающей GPU backlog больше двух кадров, `[xrready] completed predecessor` должен продолжать расти; многосекундные EMPTY-only окна при advancing captures недопустимы. Снимать VRAM budget/usage и source age одновременно. No-slot skips допустимы только с продолжающейся delivery и без блокировки Present.
4. Markers: обе eye images, одинаковый world target, yaw/roll, меню возврат. FOV: runtime angles и render/submit dimensions до/после inner/outer/up/down. Null driver проверяет тракт, но не воспроизводит оптику/стриминг Quest 2 или Steam Frame.
5. После тестовой серии закрыть игру; replay/capture анализ только после завершения процесса.

Дополнительно **18/18** CPU pose/ownership/projection cases PASS в `build/goal-pose-identity-20261117`. ABI executable-hook tests этой сборки не запускались: изменения hook ABI отсутствуют. Общая сборка плагина, игровой runtime и физические шлемы на момент отчёта ещё не проверены.

## Независимая проверка legacy mirror ownership

По просьбе основного агента проверены его изменения `Capture.cpp`, `Mirror.cpp`, `PublishedResource.hpp`, заменяющие session-wide 64 AddRef на один publication owner и до четырёх GPU consumer leases. Найдена и устранена гонка: отдельные `Publish` и raw-pointer `store` от двух publishers могли расходиться, после чего прежний raw fastpath ошибочно пропускал нужное обновление. Теперь same-pointer проверяется под publication mutex без лишнего AddRef, а Identity обновляется в той же критической секции. Identity используется только для сравнения указателей; чтение объекта требует Acquire.

Добавлен retirement по существующему copy fence в уже существующем presentation loop, в том числе при выключенном зеркале и отсутствии новых кадров. Release завершённых источников идёт вне submission mutex. При ошибке Signal источники остаются удержаны и дальнейшие copies прекращаются, поскольку completion нельзя доказать.

`tools/mirror_resource_tests`: **3/3 PASS**. 1000 graph rebuilds удерживают не более четырёх незавершённых ресурсов и полностью освобождают их после completion; параллельные два publisher / два reader освобождают все 40000 ресурсов; callback при destruction может Acquire новую публикацию, подтверждая release старого объекта вне mutex. Это COM ownership tests, не D3D12/game workload. Активность legacy mirror path в текущем SBS baseline не установлена; нельзя объявлять эту найденную retention общей причиной жалоб на 13+ GB VRAM.
