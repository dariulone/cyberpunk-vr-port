# Optional D3D12 resource lifetime census

The census answers which newly created COM resources remain alive across repeated
menu/map cycles. It does **not** measure physical residency or all process VRAM.
Default mode is zero: no cookies, stack walks or group allocation. The existing
Present poll does no work unless an explicit report request changes.

Existing `CyberpunkVR_CreationCounts` and `CreationStacks` count creations only.
The Aftermath observer filters other formats and does not cover this question.
This observer reuses the already installed base committed, committed1 and placed
resource hooks; it does not enable descriptor tracing or an Aftermath SDK mode.

From the repository directory, with the fresh DLL/PDB installed and a running
game:

```powershell
python -B tools/resource_census/probe.py --pid <PID> --enable 2 --out scratch/census-before.json
# Perform five native map open/close cycles; let each close settle.
python -B tools/resource_census/probe.py --pid <PID> --enable 0 --out scratch/census-after.json
python -B tools/resource_census/diff_reports.py scratch/census-before.json scratch/census-after.json --out scratch/census-delta.json
```

Mode 1 records the immediate API caller's module/path, return address and RVA.
Mode 2 additionally records at most sixteen raw stack return addresses. Use one report
controller. Omitting `--enable` preserves the current collection mode. The probe
writes only the diagnostic enable/request exports, validates the mapped DLL's
identity before resolving addresses, and reads the bounded report with a seqlock.
It never invokes a remote function or attaches a debugger.

Turning collection off stops new observations. Existing cookies continue to count
destruction; do not reset totals between snapshots. Reports contain PID and stable
process-local group IDs. Compare reports from the same process and collection
window. The differ rejects inconsistent counter snapshots rather than reporting
an unproven memory delta. Request fresh snapshots if contention prevented a
coherent row.

Each resource has one private IUnknown cookie. The cookie has **no reference** to
the resource, heap, device or queue. An extra owner held by the engine/consumer
therefore delays the release count naturally; the observer cannot extend that
resource lifetime. Duplicate observations (for example API wrappers) do not add
another cookie. The callback updates counters with atomics and does not acquire
census/driver locks, walk stacks or log files.

On the first enabled observation, only the observer's code module is pinned: the
plugin manager may unload before graphics objects die. Counter storage is a lazy,
bounded process-lifetime singleton so callbacks do not touch destructed statics.
Neither choice retains GPU allocations. Close the game after the diagnostic
series as usual.

Reports group full resource descriptions, heap properties, allocation size and
caller provenance. Every row has created/released/live counts and bytes plus peak
live bytes. `committed` and `placed-footprint` are separate kinds. Placed resources
can alias an existing heap, so their footprints must **never** be added to committed
bytes as total VRAM. Committed upload/readback/custom heaps also require checking
`heapType`/memory-pool fields before interpreting dedicated-memory cost.

Limits and coverage:

- 510 precise groups plus two kind-specific overflow buckets: 512 total.
- At most 131072 concurrent lifetime cookies and a 1 MiB report buffer.
- `groupOverflows` counts observations coalesced into overflow, not distinct keys.
- `attachFailures`, `cookieLimitDrops`, `reentrantDrops`, `invalidAllocations` and
  `modulePinFailures` expose incomplete observation.
- Only objects created while collection was enabled are counted. Existing objects,
  heap allocations themselves, reserved resources and unhooked newer creation APIs
  are not retroactively included. `coveredApis` names the hooked entry points.
- COM lifetime is distinct from driver residency and deferred memory reclamation.
  A balanced object ledger with growing DXGI usage points to another ownership or
  residency path; it does not prove that every GPU allocation was observed.

Mode 2 and report generation add diagnostic work. Do not use their frame times as
an ordinary performance baseline. Raw return PCs should be resolved against the
matching DLL/PDB from that exact process, preserving symbol offsets and provenance.

Standalone verification (only while the game is closed):

```powershell
cmake -S tools/resource_census_tests -B build/goal-resource-census-20261117 -A x64
cmake --build build/goal-resource-census-20261117 --config Release
ctest --test-dir build/goal-resource-census-20261117 -C Release --output-on-failure
python -B tools/resource_census/diff_reports_test.py
```

The four WARP cases use real committed and placed D3D12 resources: disabling while
a consumer owns a resource, final COM release, duplicate observations, separate
placed accounting, 540 distinct descriptions overflowing the bounded groups,
parallel resource creation/release/reporting, and concurrent observation of the
same resource. The Python cases reject inconsistent/cross-process endpoints and
keep placed footprints separate.
