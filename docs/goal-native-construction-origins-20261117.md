# Native DynamicTexture construction origins

Optional diagnostic, default off. Existing D3D creation counters and resource
lifetime census see lazy GPU allocation, whose stack reaches a generic
VirtualWindow draw job. This observer records the earlier native factory return
at game2.5 RVA5EA8A4 to identify the actual construction caller.

## ABI and lifecycle boundary

Verified factory ABI: RCX points to a16-byte output Handle(object/control), RDX to
a24-byte descriptor(width,height,mips,samples,format,flags,padding,backing), return
value is the output Handle pointer. Factory5EA8A4 calls constructor5EA914; the
constructor has no other direct code caller in the matching IDA image. Reflection
or raw clone paths are not thereby proven covered.

The observer forwards the original before taking its publication lock. The
caller still owns the returned Handle when object/control and ISerializable
serial+0x28 are copied; no AddRef, Release, native lifetime call, GPU allocation
or native object ownership is introduced. Default0 takes the direct forwarding
path, without stack capture, publication lock or event allocation. All native
creations are recorded while armed rather than guessing which sizes may later
resize into a target.

Installation is Boot84, after MenuSurface and PhysicalOpticsSurface at Boot83:
both must validate the factory's original prologue before this observer patches
it. The observer requires the supported build without a forced unknown-build
override and the verified16-byte signature. A failure sets its own status and
returns failure to the logging registry; it never disables another hook. The
worker continues other hooks after an optional failure.

## Publication and coverage

`CyberpunkVR_DynamicTextureOrigins*` exports include Enable, Status, Sequence,
Count, Observed, InFlight, DroppedReentrant, DroppedBusy, Overwritten, QpcFrequency,
Layout and Rows. Status1 means ready;0 is not installed,2 unsupported/override,
3 signature mismatch,4 CreateHook failure,5 EnableHook failure.

The CPU ring has1024fixed344-byte rows. A row contains ordinal, QPC, object,
control, object serial, direct caller, descriptor, thread, flags and32raw return
addresses. Flags:1 descriptor present,2 output Handle present,4 return==output,
8 non-null object/serial captured. There are no resource pointers with ownership.
The direct caller is retained separately from stack-walk frames, so compiler
tail-call behavior does not hide it behind an assumed skip count.

Writers use a try-lock only for the fixed-size publication; contention and
observer recursion are counted as explicit drops. Sequence is odd during a
write and even afterward. Ring overwrites and requested-interval losses are
reported. Count is monotonic and must never be reset by an external memory write.
InFlight covers an armed factory call still executing; a call entered before
disarm can finish afterward. Coverage counters are independent atomics, so the
reader reports `coverageSettled` and a busy endpoint should be repeated after idle.

## Root-controlled use after planned deployment

The new source file requires configure before the root's production build.
Nothing in this preparation installed a hook into the running PID4744.

```
python tools/resource_census/dynamic_origins.py --pid <new-PID> --enable 1 --out <directory>/origins-before.json
```

Record `count` from that baseline. Root then performs the ordinary protected map
open/close cycles and resource census. The origin reader does not launch, close,
save/load, switch runtime, call the game or manipulate UI. It works with either
OpenXR runtime and does not require the Simulator capture API.

```
python tools/resource_census/dynamic_origins.py --pid <new-PID> --enable 0 --since <baseline-count> --out <directory>/origins-after.json
```

Only Enable is written. All addresses come from actual mapped PE exports;
reported PCs retain their raw value and module/RVA, never a misleading nearest
export name. For the identity join, match object+control+objectSerial against a
live typed snapshot, then match its native backing handles/resources. Descriptor
shape/flags alone are not an owner identity. A missing target event can indicate
creation before arming, an overwritten/dropped interval, or a path bypassing
the factory; it does not prove that no texture was created.

## Validation

Standalone CPU suite3/3 passed:3000-record overwrite accounting, concurrent
writers with a ReadProcessMemory seqlock reader, and the real observer.cpp compiled
with local installation stubs to verify disabled ABI forwarding, enabled output
identity, reentry accounting and unsupported-build refusal. No game addresses or
MinHook patch are executed in those tests. Decoder tests3/3 passed for the binary
ABI, explicit overwritten interval and torn/reset publication rejection.

The production DLL and real factory hook still require root's next build/deploy
and live validation. This instrumentation is not a leak fix or proof of the
persistent holder of the observed768²/1024² pair.
