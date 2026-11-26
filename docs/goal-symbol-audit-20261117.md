# Достоверность CPU stack attribution

Архивный `baseline-cpu-stacks.json` нельзя использовать для вывода, что CPU занят `wscanf_s`, `vsnprintf_s` или `CyberpunkVR_DumpPrepareFinalizeBuckets`. Старый sampler принимал любое имя от SymFromAddr, отбрасывал displacement, size и symbol type, а hitch profiler сохранял только уже названные stacks. Raw PCs и module map в этом baseline отсутствуют. Точные функции этих кадров теперь не восстановить; frame intervals и thread cycle measurements не объявляются ошибочными только из-за неверных имён.

Подсчёт старых названных кадров: `vsnprintf_s` 4741, `CyberpunkVR_DumpPrepareFinalizeBuckets` 1762, `wscanf_s` 298. Это количество встреч строк, **не подтверждённое распределение CPU по этим функциям**.

## Проверка DLL/PDB

Архивные DLL/PDB из `scratch/goal-audit-20261117/baseline-binaries` совпадают по CodeView/MSF identity:

- GUID `320d5c83-4f9d-4f76-8871-b7087ebddb21`, age **104**.
- DLL SHA256 `63b73e2f572a0bb984c8d8910bd6d49d95ec3dade8dd5702bd46694ab16fb578` совпадает с manifest.
- Embedded PDB path ведёт в изменяемый build output. Сейчас там тот же GUID, но age **110**: этот PDB **не подходит** к baseline DLL.

При явном search path на архив DbgHelp сообщает **SymPdb**, `GlobalSymbols=true`, `PdbUnmatched=false`, `LoadedPdbName` указывает ровно на архивный PDB. Это проверено и системным DbgHelp, и установленным SDK DbgHelp. Следовательно, доказанной причиной не является неспособность системной библиотеки читать этот PDB.

Офлайн воспроизведение старого поиска сделано с копией того же DLL без adjacent PDB, без загрузки/исполнения DLL. Получены **SymExport**, пустой `LoadedPdbName`, `GlobalSymbols=false`:

| RVA архивного DLL | Без matching PDB | С matching PDB |
|---|---|---|
| `0x100000` | `fscanf_s+0xB270` | `cvr::camera::MakeCameraEntitySample`, +464 в span709 |
| `0x200000` | `vscprintf_l+0x8C470` | `ProvStub<0,0>`, +1024 в span2960 |

Даже экспортам DbgHelp в этом воспроизведении присвоил ненулевой Size, охватывающий промежуток до следующего export. Поэтому одной проверки `displacement < size` недостаточно. Документация [SYMBOL_INFOW](https://learn.microsoft.com/en-us/windows/win32/api/dbghelp/ns-dbghelp-symbol_infow) определяет Size как содержательный для PDB, а [IMAGEHLP_MODULEW64](https://learn.microsoft.com/en-us/windows/win32/api/dbghelp/ns-dbghelp-imagehlp_modulew64) различает SymPdb и SymExport и раскрывает LoadedPdbName. Baseline не записывал эти поля; конкретный тогдашний search failure не доказан, но механизм ошибочной атрибуции воспроизведён точно.

Сохранённые доказательства:

- `scratch/goal-audit-20261117/baseline-pdb-identity-audit.json`
- `scratch/goal-audit-20261117/baseline-symbol-provenance.json`
- `scratch/goal-audit-20261117/exports-only-repro.json`

## Исправления tooling

`thread_stack_sampler.py` и `hitch_profiler.py` теперь используют явный search path из каталогов actual loaded modules, плюс необязательный `--symbol-path`. Символы прогреваются до начала sampling, чтобы отложенная загрузка PDB не происходила при suspended game thread.

Для каждой PDB-подстановки проверяются GUID/age и identity **mapped** PE image; файл DLL на диске мог быть заменён после запуска. Несовпадение отвергает имя. Exact named lookup для `CyberpunkVR_PresentTiming` также сначала проверяет mapped DLL против disk image, поэтому не читает адрес из новой сборки в старом процессе. Флаг `LOAD_ANYTHING` никогда не включается. Экспортные подсказки системных DLL допускаются, но отмечаются как nearest с displacement/RVA; `EXACT_SYMBOLS` глобально не включён, поскольку он подавляет и такие полезные подсказки при отсутствии системных PDB. Проверка identity остаётся обязательной для PDB.

В JSON добавлены raw PC stacks, thread start addresses, module ranges/paths и `symbolSession` с реальным DbgHelp path, SymbolType, LoadedPdbName, mapped/PDB identity и данными каждого разрешения. Для PDB PC внутри доказанного span остаётся агрегация по функции; за границей span или для nearest export сохраняются `+offset`, исходный RVA и отметка неопределённости. Неизвестные addresses больше не округляются до 4 KiB. Return addresses корректируются на -1 до поиска module boundary. Module aggregation больше не считает имя функции именем модуля.

В реальном self-process тесте выявлена дополнительная особенность: export SYMBOL_INFO.ModBase может быть нулём. Sampler теперь получает base через SymGetModuleBase64, поэтому raw RVA остаётся относительным к модулю, а не абсолютным VA.

## Проверки и следующий замер

**9/9 regression tests PASS**: PDB half-open span, nearest export, zero-size public, native result displacement, mapped-PDB mismatch, нулевой export ModBase, raw RVA/return boundary, exact name lookup при заменённом disk image, module aggregation. Self-process проверка прочитала 29 совпадающих mapped GUID/age без расхождений и точно разрешила экспорт Sleep. Python syntax checks и `hitch_profiler.py --help` прошли. Игра в этой работе не читалась и не управлялась.

Для свежего игрового профиля использовать только PDB свежего установленного DLL. Каталог плагина включается автоматически; при внешнем PDB передать его каталог через `--symbol-path`. До интерпретации имен проверить `symbolSession.provenanceWarnings` и запись `cyberpunkvrport.dll`: SymPdb/Dia, exact LoadedPdbName и `mappedIdentityVerified=true`. Старый baseline требует повторного sampling; загрузка нового PDB не восстанавливает потерянные адреса.
