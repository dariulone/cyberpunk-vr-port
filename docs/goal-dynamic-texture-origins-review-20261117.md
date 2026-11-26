# Independent review: native DynamicTexture origins observer

Source-only review of `include/Natives/DynamicTextureOrigins.hpp`, `src/Natives/DynamicTextureOrigins.cpp`, `tools/resource_census/dynamic_origins.py` and their tests. PID4744 was not accessed, no production build was run, and no further owner/heap investigation was performed.

**Result: no critical ABI, lifetime or publication blocker found.** One reader ordering issue was reported to the author and corrected before signoff: Layout and `since/current_count` validation now precede any WriteProcessMemory of Enable. An incompatible future layout therefore cannot be armed before the reader rejects it.

## Verified contracts

- Native2.5 factory5EA8A4 takes output Handle storage and the24-byte descriptor, initializes the168-byte DynamicTexture, installs the returned Handle and returns the same output address. The hook uses a16-byte output Handle and24-byte Descriptor; static assertions fix Record344 and the decoder's offsets.
- The descriptor is copied before the factory call, preserving its input even if caller storage is reused. Object/control and serial+0x28 are copied after the factory returns but before its caller resumes. That returned Handle still owns the object; the observer neither adds nor drops a reference. Records contain only numeric identities, descriptor bytes and PCs.
- Default Enable0 goes directly to the original function. No observer counter, stack capture, row construction or writer lock occurs on that path. The global fixed ring and mutex exist independently of per-call recording.
- Original factory execution and stack capture occur outside the publication lock. Legitimate native nested creation can execute normally; reentry during observer work is forwarded and explicitly counted as dropped. The guard covers recording, not the original factory.
- Writers use a try-lock and fixed1024-row ring. Busy loss and overwritten history have separate counters. Under the single writer lock, odd/even sequence brackets the row and count update; the external reader checks the same even sequence before/after copying and rejects ordinal holes, unknown layouts and reset intervals.
- Boot84 runs after both existing MenuSurface and PhysicalOpticsSurface Boot83 signature checks of the same factory. The strict build/override/signature gate is local to this observer. HookRegistry continues installing other hooks after a false return; MinHook failure does not remove another subsystem's hook.
- Arming is refused unless Status1. Disabling/reading does not invoke game constructors, destructors or COM methods. Resource histories must be joined with object+control+serial, not a reused pointer alone; the author's current typed texture reader exposes those fields.

## Coverage and interpretation limits

`coverageSettled` is an observation of counters, not a stop-the-world barrier. Stop arming and wait for `InFlight==0` plus the accounting equality before treating an interval as drained. While recording remains enabled, new factory returns may follow a valid snapshot.

Positive drop or overwrite counters mean incomplete history. Reflection/deserialization/clone paths that bypass5EA8A4 are outside this observer's stated scope. A captured object may already be destroyed by the time an external reader sees its row; this is a construction history, not an owned live-object registry. Stack depth32 can truncate callers; the direct caller PC is retained separately. No caller observed yet establishes the surviving map pair's producer until identities are joined in the controlled live run.

## Validation

Independently rerun:

```powershell
ctest --test-dir build/dynamic-texture-origin-tests -C Release --output-on-failure
py -3.13 -m unittest discover -s tools/resource_census -p dynamic_origins_test.py -v
```

C++3/3 PASS: ring overwrite accounting, four writers with stable RPM snapshots and contention accounting, and compilation of the actual observer `.cpp` with local stubs to test original forwarding/default-off/output identity/reentry/unsupported install. Python3/3 PASS: exact ABI/PCs, explicit overwritten intervals, and rejection of torn/truncated/unknown/reset publications. Those standalone tests do not validate a live MinHook installation or prove actual map-factory coverage.

Reviewed final hashes:

- Header: `3B44DA6D485FE7C3171D49B03E86560DB2A0B673699D04C2C79A6CF12CEBD437`
- Native source: `4E190C8E8E396491B7C23E93DF775B15780C5BB4D94A1DE7794C269F320672DD`
- Python reader after ordering fix: `C74A607F73362253B1FCBA029C49534C7C4CCA5161D2492926E196A49B3CFC8E`

Reviewer made no native-source changes. Combined production build/deployment and the origins live join remain root-controlled next steps.
