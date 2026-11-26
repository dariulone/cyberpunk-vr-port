# Статический аудит памяти активного SBS pipeline

17 ноября 2026. Проверены текущие исходники; игра PID 22188 принадлежит основному агенту, этот аудит не изменял её состояние. Измерения основного агента (8.1 GB app peak, примерно 557 MB FG, Lua sawtooth за 12 SAO cycles) здесь не подменяются оценками из кода. Legacy VRCAM mirror не считается доказанным владельцем памяти текущего SBS режима.

## Подтверждённое исправление: завершённые native HUD sources

`src/Render/HudPanel.cpp` имеет шесть каналов с тремя slots каждый. Каждый slot держит source Snapshot, а он — TextureLease нативных ink targets, включая engine pool handle и COM reference. Также slot держит native frame/HUD/exposure constant buffers.

До исправления освобождение этих inputs происходило только при повторном использовании slot либо Shutdown. При пустом/просроченном Snapshot функция Capture возвращалась раньше. После выхода из машины, турели или surveillance соответствующий канал может не использоваться до конца сессии, поэтому уже завершённые input references оставались жить без потребителя. Output canvas и native inputs имеют разные lifetimes: XR может ещё держать canvas, но после producer fence он уже не читает исходные native textures.

Теперь Capture до empty/stale return освобождает **только завершённые source/constant leases** по существующему producer fence. Output canvas, slot allocator и reusable render targets сохраняются. Новых GPU allocations/fences/waits нет. Это bounded retention fix; не заявляется, что он объясняет все жалобы на 13+ GB или даёт заранее известную экономию.

Добавлен production WARP case `hud_idle_sources`: GPU queue блокируется fence; канал отключается тремя способами (null, empty content, stale); до completion source должен жить, после completion — освобождаться без reuse, в то время как consumer-held canvas остаётся жив. После подтверждённого закрытия PID22188 выполнены `hud_idle_sources`, `hud_completed_source`, `hud_gpu_sprites`, `hud_gpu_leases`: **4/4 PASS**, production fixture `build/hud-tests/Release/hud_tests.exe`.

## DLSS/RR copies: доказана retention, добавлена измеримость

`src/Stereo/EngineStereoDlss.cpp`: `copies[2][40]` — ограниченный кэш выделенных per-eye textures и, для depth-stencil, whole-depth staging. Замена размера/формата освобождает cache reference старой allocation; `KeepCopy` заранее передаёт ресурс и staging в `CommandResources`, который защищает запись, replay и незавершённый GPU submit.

Кэш не удаляет типы, переставшие использоваться. После RR→SR все left-eye copies не нужны, поскольку copyLeft требует `rr`, но их cache ownership остаётся. Дополнительно SR и RR имеют собственные пары viewport/context; `TrackedFree` исходного viewport0 не освобождает эти четыре private contexts. Это конечное удержание ресурсов разных режимов, а не бесконечный список каждого открытия меню.

Нельзя освобождать эти ресурсы только потому, что текущий Evaluate ими не пользуется: реальные tags в приложенном issue243 логе имеют lifecycle **1 = eValidUntilPresent** (строки 2228–2232). Локальный `sl_core_types.h` разрешает release после Evaluate только для других lifecycle. GPU command completion не доказывает завершение внешнего Present consumer. [Официальный Streamline guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuide.md) также требует завершения pending evaluate command lists перед освобождением feature. Поэтому экспериментальный trim не оставлен в финальном изменении; speculative context free отсутствует.

Добавлен export `std::atomic<uint64_t> CyberpunkVR_EngineStereoCopyMemory[10]`:

| Индексы L/R | Смысл |
|---|---|
| 0, 1 | Текущие allocation bytes кэша, включая staging |
| 2, 3 | Количество GPU resources в кэше |
| 4, 5 | Не использованные текущим Evaluate bytes без известного Present-lifetime tag |
| 6, 7 | Сумма выделенных bytes за сессию, индикатор allocation churn |
| 8, 9 | Не использованные текущим Evaluate bytes, удерживаемые из-за Present lifetime |

Bytes получаются через `GetResourceAllocationInfo` **только при создании allocation**, а не каждый кадр. Clear/mark/report текущего использования slots включены только при `RuntimeDiagnosticsEnabled`; lifecycle bits отслеживаются консервативно всегда. Counters не включают память внутри NGX/Streamline и старые allocations, ещё удерживаемые command recordings/flight receipts. Это важная граница: сумма counters не является полной VRAM процесса. Для настоящей оценки надо сравнить SR → RR → SR в одной сцене и совместно снять эти counters, DXGI local budget/usage, native context lifecycle и game settings.

## Подтверждённый overhead: перечисление XR capabilities каждый кадр

`EnsureMonoSubmitResources` проверял уже готовые color resources только после двух `xrEnumerateSwapchainFormats`, двух `xrEnumerateViewConfigurationViews` и временного vector allocation. В реальном simulator log PID22188 насчитано 93866 записей view configuration и 93869 format records; лог достиг примерно 30 MB. Объявленные recommended 1280×1280 повторялись, хотя реальные eye swapchains оставались 2648×2864.

Ready check перенесён перед этими перечислениями. Он по-прежнему проверяет текущие presented width/height и все три command-list/allocator slots: resize/reinitialization требует полного пути. Live `SetDepthCaptureEnabled` выполняется до early return. Per-frame `xrLocateViews` и его FOV/pose не изменены. Новый diagnostic export `CyberpunkVR_XrSubmitResourceCache[3]` считает ready fast returns, capability refresh и deferred resize, позволяя на свежем запуске доказать реальное сокращение API calls/log traffic. Общий plugin build и свежий runtime замер выполняет основной агент.

## Resize и GPU ownership: исправления после независимого review

В submit resize раньше ожидание обрывалось через 1000 ms и старые swapchains/allocators уничтожались даже при незавершённом fence. Теперь `FenceRetirement` делает nonblocking poll: pending оставляет прежние objects и LastSubmission, следующий XR cycle повторяет старый кадр или после стандартного stale-limit отправляет empty. Device removal (`UINT64_MAX`) не трактуется как completion для пересоздания. Size-independent submit fence/event сохраняются с монотонной шкалой, чтобы не ломать ещё живые capture-read receipts. LastCopy инвалидируется при реальном resize.

В producer pools найдены ещё два дефекта: старые allocations освобождались до завершения queued capture writes; right-eye pointers/serials заменялись без mutex, который consumer использует для выбора и AddRef. Теперь resize требует completed producer fence; новые pools создаются отдельно, целиком публикуются под `m_presentMutex`, старые COM references отпускаются вне lock. Ошибка allocation сохраняет старый pool. Неполная первоначальная allocation command objects теперь корректно повторяется по отсутствующим slots.

Mono capture slots раньше создавались в COPY_DEST, но переход COPY_SOURCE→COPY_DEST зависел от **global** previous serial: первое использование слотов 2–4 имело неверный StateBefore, а serial reset пропускал переход уже использованного слота. Теперь initial/rest state всех slots COPY_SOURCE и переход перед записью безусловный. Submitted capture должен совпадать с размером swapchain; старый-sized capture после resize не попадает в CopyResource.

`CapturedMonoFrame` хранит tracking-origin epoch. Старый origin не защищает predecessor slot после recenter, не публикуется как новый epoch и не выбирается для submit; reset invalidates LastSubmission/LastCopy. Framegen получает epoch выбранного capture, а не текущий epoch во время submit.

Последующий review выявил ещё два условия для second-eye reuse: успешная write reservation немедленно очищает старые serial/label, потому что GPU может изменить pixels даже при отменённой после recenter публикации; eye label хранит свой origin, и exact/reused serial проверяется против текущего epoch. Проверка `capture_eye_origin` использует production BufferLeases overload и reuse predicate: неудачная reservation не портит старую метку, отменённая публикация остаётся недоступной после EndWrite, старый epoch не проходит даже при точном serial. Обновлённая pose/ownership/projection suite: **19/19 PASS**.

Отдельно исправлена queue selection для non-SteamVR FG-off: сам факт вызова Presenter не означает, что изображение синтезировано. Обычный завершённый capture копируется на runtime queue; **synthetic** output следует actual synthesisQueue, переданной Generator, в том числе при live переключении FG. Pending real endpoint остаётся обычным captured image.

`command_resources_lifetime` дополнен реальными WARP queue-fence сценариями: 1000 polls заблокированного resize fence остаются Pending и разрешают resize только после unblock; real capture проходит по runtime queue мимо более поздней game queue работы, synthetic copy остаётся за producer fence. **PASS** вместе с существующими GPU-delayed resize/replay/cross-queue/32-replacement проверками. Дополнительно pose identity/origin/frontier suite **18/18 PASS**. Это не подменяет свежий игровой запуск после общей сборки.

## Проверка гипотезы +6.25 MiB на map cycle

По просьбе основного агента прочитан actual log `C:\Users\dariulone\AppData\Local\OpenXR-Simulator\openxr_simulator.22188.log`. Во время рассмотренной сессии создано всего семь XR swapchains: color 2648×2864 (два), HUD 720×452, depth 1536×1661 (два), HUD 2648×2864 и minimap 380×386. Последний create находится на строке 21653; после него более 350 тысяч строк без нового create/destroy. Следовательно, наблюдавшийся рост open-map VRAM не является повторным созданием 1280×1280 XR swapchain. Размер 1280×1280 в этом логе — recommendation, не allocation.

Installed DLL `Downloads/OpenXR-Simulator-v1.5.0-win64/openxr_simulator.dll` имеет SHA256 `EFF684E7F35753B890BB84254E755B1F8F7E816128650099D92C0DF4BD7D5095`; Desktop source tree/bin DLL — другой hash `6664C883F12615206BCC51AC4FC67985B46FE0868195A2E2F568F3C3AF576E1E`. Поэтому source review Desktop simulator не доказывает поведение установленной сборки. Повторить map cycle без двух OBS layers нужно до приписывания этих 6.25 MiB порту. Native map allocations и внутренние allocations слоя также пока не идентифицированы.

## Остальные владельцы и границы

| Подсистема | Проверенный lifetime / ограничение | Вывод |
|---|---|---|
| OpenXR color captures | 4 MAIN slots + 4 second-eye slots; resize заменяет pools | Постоянная цена stereo buffering, не per-menu накопление. При 2648×2864 RGBA8 payload одного texture примерно 28.93 MiB, восьми — 231.44 MiB до alignment. |
| XR swapchain images | Runtime-owned images по числу, возвращённому enumerate | Учитываются отдельно от capture pools. Не считать из предположения «три картинки» без реального runtime inventory. |
| Framegen Inputs | Шесть shared Record slots на глаз; pending links weak; reuse требует единственного cache owner и завершённого producer fence | Нормальный путь bounded. ReleaseInputs освобождает только безопасные slots. Двенадцать retained depth/MV textures не равно утечке каждого frame. |
| Framegen Generator | Один Impl; 3 Work slots; Poll сбрасывает input leases после fence; несовместимые настройки требуют Idle перед заменой | Нет списка старых contexts на обычных settings changes. Destructor при недоказанном GPU completion намеренно удерживает один Impl; это shutdown/error fail-closed, не нормальный menu loop. |
| Presenter | previous оставляет metadata без texture/input lease; pending удерживает один endpoint до принятого real frame | Не обнаружен неограниченный history ring. Current pending endpoint необходим для временного порядка real/generated. |
| HUD output pools | 6 channels × 3 slots, allocations canvas/raw mip chain/half-resolution blur/blurTemp | После использования канала его reusable output resources сохраняются; это bounded cache. Удалять каждый раз при закрытии меню означало бы возвращать allocation hitches. Исправлены лишние native input leases, описанные выше. |
| SAO MenuSurface | Weak component ownership; Prune при Submitted каждые 250 ms, удаляет умершие компоненты после fence и отсутствия recorded use | Обычный submit/reset путь очищается. Требуется отдельно проверить отменённые и уничтоженные без Reset command lists, см. ниже. |
| CommandResources | Recording owner привязан через SetPrivateDataInterface; registry weak; immutable submission batches держатся до queue fence | Нормальный reset/replay/destruction путь корректно освобождает resources. Existing production tests покрывают GPU-delayed resize, replay, cross-queue reader и 32 replacements. Не запускались повторно во время live baseline. |
| RTV DescriptorCache | Metadata без AddRef/GPU allocation; LRU limit растёт от cumulative RTV heap slot creation | Не владелец texture VRAM. Счётчик heap slots не уменьшается на destruction, поэтому на длинной сессии CPU metadata limit может расти. Это отдельный lead, не доказанное объяснение GPU memory. |
| DSV map | `DepthCapture.cpp` хранит raw resource identity/format/dimensions по descriptor handle; active scene depth имеет один защищённый reference | Сам map не удерживает старые texture heaps, но нет видимой очистки устаревших handles при heap destruction. Возможен CPU metadata рост; безопасная переработка требует generation/lifetime descriptors. |
| Descriptor audit / constant-upload registry | `CVR_STEREO_OBSERVE_BINDINGS` gate; constant upload cache до 256 entries / 512 MiB | Не активен по умолчанию. При профилировании нужно проверить actual environment, иначе диагностический cache ошибочно примут за штатный SBS overhead. |
| Legacy upload-map helper | До 64 mapped UPLOAD resources, session AddRef | Это системная/UPLOAD memory, не автоматически dedicated VRAM; применяется HUD constants/probes. Не увеличивать limit как «исправление утечки». |

## Что ещё требует проверки

`MenuSurface::recorded` содержит strong Surface references по raw command-list pointer и удаляет их только на Submitted/ResetList. В отличие от CommandResources, здесь не найден owner, привязанный к уничтожению command list. Отменённая запись, уничтоженная без Reset/Submit, может удерживать Surface/texture. Аналогичный weak-link map Framegen Inputs не удерживает texture напрямую, но abandoned Record может остаться с expected != submitted и перестать переиспользоваться. Это доказанная неполнота cleanup coverage по коду, однако реальный переход игры на такой путь и рост живых объектов в baseline пока не установлены. Не править на основании предположения о количестве меню; сначала нужен cancelled-list trace либо regression с реальным lifecycle hook.

`CommandResources::KeepCommandResources` копирует текущий vector references и вызывает PruneRecordings под global mutex при добавлении объекта. Это потенциальный CPU cost при большом числе tags/recordings; считать его bottleneck без thread profile нельзя. Текущий `KeepCopy` сначала проверяет уже зарегистрированные objects и избегает повторной регистрации.

Открытие inventory/map может требовать временных portraits/UI/game graph targets, и после закрытия движок/драйвер может сохранять их в пуле. Поэтому кривая total VRAM сама по себе не различает leak, bounded high-water cache, queue backlog и budget eviction. Нужны одновременно allocation owner counts, unfinished GPU receipts и budget. Ни один найденный bounded cache не является доказательством общей причины всех пользовательских статтеров.

## Следующий игровой замер

На том же save и разрешении: сначала FG off/on, затем SR/RR/SR и native inventory/map cycles. Снимать per-eye copy counters, FG InputVram/Generator bytes, HUD channel active state, MenuSurfaceStats[7], GPU queue delay и DXGI budgets. После idle channels ожидается освобождение **native source leases**, но reusable HUD outputs законно остаются. После RR→SR новые telemetry counters должны показать, сколько старых inputs осталось и почему; context VRAM измерять отдельно. Проводить серии в одном процессе, после окончания закрыть игру.

## Дополнительный подтверждённый legacy tonemap leak

Независимый review обнаружил ещё один настоящий источник потерянных ссылок: `tm_set_push` делал AddRef для каждого нового large VRCAM input, а смена CB в `Mirror.cpp` обнуляла только число элементов, намеренно не releasing старые entries. Кроме того, load/check/store счётчика позволял двум writers занять один slot и потерять одну owning reference. Это не объясняет пару native map textures: filter width>=1000 исключает её 768² половину, а census/native owner findings описаны отдельно в `goal-map-texture-lifetime-20261117.md`.

Source fix заменяет raw array на `ResourceCaptureSet`: до24 owning entries, сериализованные Observe/Update/Reset/Acquire, duplicate без нового AddRef; release старого набора вне его mutex. Collection включена только при LumaProbe, MirrorOutput и StableCopy. При выключении публикация очищается. Единственный GPU reader в Mirror берёт owning snapshot и переносит только действительно скопированные источники в четыре bounded slots, покрытые уже существующим mirror fence. Snapshot не становится session history. Close failure либо отсутствие execute не публикует GPU leases; failed Signal сохраняет все submitted source leases и запрещает дальнейшие submissions. Завершённые leases retire при существующем mirror poll даже без reuse слота. `g_ti_src` используется последующим CPU анализом только как identity для сравнения/лога, без dereference.

Добавлены три non-GPU COM lifetime regressions: 1000 graph rebuilds с consumer receipts и disabled gate, concurrent writers/readers/reset, reentrant destructor после Reset. Root затем выполнил configure/build и весь mirror_resource_tests suite: **6/6 PASS**, включая три новых случая. Это COM ownership/concurrency проверка, не live GPU/placed-heap подтверждение.

Отдельно сообщена существующая raw CB/exposure publication race: CB resource+offset публикуются раздельно, Mirror читает raw source, updater может Release прежний resource до CPU/GPU чтения. Это не введено tm fix; scope не расширен без отдельной координации.
