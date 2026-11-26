# DLSS/RR: можно ли освобождать private viewports из TrackedFree(0)

Проверены текущий `EngineStereoDlss.cpp`, `IdleDlssContext.hpp`, installed sl.interposer/common/dlss/dlss_d версии **2.14.1.0**, локальные headers **2.14.1** и upstream tag **v2.14.1**. Production-код в этом шаге не менялся, сборка и обращения к живой игре не выполнялись.

Вывод: native `TrackedFree(feature, viewport0)` — подходящее **место запросить retirement**, но на имеющихся доказательствах не готовое разрешение немедленно освободить оба private contexts и их copies.

## Что подтверждено

Два private Evaluate записываются в **тот же commandList**, который игра передала для исходного Evaluate(viewport0). Их input/output copies тоже записываются туда. Поэтому доказанный native flush всех соответствующих записей включал бы и команды private eyes: отдельной очереди SR/RR в этом месте нет. Это делает native free boundary перспективной, но не заменяет доказательство самого flush.

Фактический `TrackedFree` держит общий recursive mutex, вызывает original `freeResources(feature, viewport)`, затем при successful SR viewport0 только вызывает `originalContext.Released()`. У него нет command-list/fence аргумента и он не проверяет состояние private записей. Собственный idle reclaim исходного viewport вызывает trampoline напрямую, обходя TrackedFree.

У текущего `IdleDlssContext` нет метода Poll. `Used` ставит marker в `CommandResources` только для original SR Allocate/Evaluate. `Ready` требует pending markers=0, два успешных split и двухсекундный idle interval. Это не простой timer: recording, replay и GPU ownership действительно удерживают marker. Но private SR/RR Evaluate **не вызывают Used** для собственных contexts; `SplitSucceeded` не создаёт такого receipt. Поэтому `originalContext.Pending()==0` ничего не доказывает о незавершённых private features.

## Поведение Streamline 2.14.1

[Официальный guide](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/docs/ProgrammingGuide.md) требует завершить относящиеся к feature command lists перед explicit free и возлагает сериализацию на приложение. Сам API не предоставляет completion receipt.

В [SR explicit free](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/source/plugins/sl.dlss/dlssEntry.cpp#L884) вызывается NGX `releaseFeature`, уничтожаются внутренние mvec/output и удаляется viewport. В [RR explicit free](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/source/plugins/sl.dlss_d/dlss_dEntry.cpp#L1180) аналогично освобождается handle/mvec и удаляется viewport. В этих wrappers нет проверки fence или собственного GPU wait. Трёхкадровый delayed release встречается в другой ветке изменения режима/размера во время Evaluate; переносить его гарантию на explicit free нельзя.

Общий registry тегов — отдельный владелец. Legacy slSetTag из [commonEntry.cpp](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/source/plugins/sl.common/commonEntry.cpp#L200) вызывает resource tracking. [Generic::startTrackingResource](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/source/platforms/sl.chi/generic.cpp#L401) делает AddRef и хранит ресурс по `(tag, viewport)`; stopTrackingResource делает Release. Explicit feature free не очищает этот registry. Поэтому даже освобождение cache reference EyeCopy не обязано освобождать allocation: последний private RR tag может всё ещё владеть ею.

`setTag(nullptr)` поддерживает снятие тегов, но также требует правильной границы lifetime. Доказанный GPU completion наших copy/evaluate записей не равен автоматически завершению всех внешних Present consumers для `eValidUntilPresent`. В legacy tagging зависший tag инвалидируется при последующем getTag достаточно позднего кадра; неиспользуемый viewport может больше никогда не запросить этот tag.

## Чего не хватает для безопасного изменения

1. Фактический native call-site/trace, показывающий завершение всех relevant command lists/queues перед TrackedFree(0), либо отдельный per-private-context ledger с recording/replay/GPU receipts. Утверждение «native caller обязан это делать» является предпосылкой контракта, а не измеренным фактом текущей ветки.
2. Проверка нужного feature/generation. SR-free не разрешает освобождать ещё активный RR, и наоборот. Новый Evaluate после запроса retirement должен отменять устаревший запрос или явно начать новую generation.
3. Для cache copies — завершение их Present-lifetime consumers и снятие private tags, продолжающих держать AddRef. Часть right-eye copies разделяется между SR/RR; нельзя удалять весь общий cache по free одного feature.
4. После доказанного release нужно сохранить корректное восстановление options/tag state при lazy recreation. Нельзя считать отсутствие original viewport или `eErrorInvalidParameter` доказательством отсутствия private work.

До этих проверок eager free, «подождать N кадров» и timer-only cleanup не добавлены. Для предстоящего SR→RR→SR sweep остаются allocation census и per-eye copy counters; сохранённая память отмечается как retention, а не автоматически как бесконечная утечка.

## Дополнительные границы измерений

Original context tracker охватывает SR, **не RR viewport0**. Если игра действительно вызывает explicit Allocate для original RR0, текущий idle reclaim его не покрывает. Сам факт наличия этой allocation в конкретном запуске ещё надо измерить.

Не следует суммировать `slDLSSGetState`/`slDLSSDGetState` как независимые per-viewport bytes. В исходниках 2.14.1 wrapper передаёт viewport, но State-ветка `slGetData` его не ищет: она вызывает stats callback через общий NGX params и читает SizeInBytes. Это SDK estimate с иной областью охвата, не доказанная карта каждого private context. Наши counters учитывают только собственные allocations; internal NGX contexts и tag-held resources нужно сопоставлять отдельно.

Исходники соответствующей версии сохранены для проверки в `scratch/goal-audit-20261117/streamline-2.14.1/`; это reference data, они не включены в сборку плагина.
