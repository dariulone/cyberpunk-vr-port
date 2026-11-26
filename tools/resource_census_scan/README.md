# Bounded native texture owner search

Read-only C++ helper for the measured map-allocation investigation. It accepts a
fresh `native_textures.py --hud --layers` JSON report for an already running game.
It opens only query/read rights, validates mapped game2.5 PE timestamp/SizeOfImage,
and searches committed readable PRIVATE pages near known native allocations.
It does not suspend the game, inject code, call COM/native methods, modify refs,
launch/exit the game or change runtime. Root coordinates a measurement-free window.

```
cmake -S tools/resource_census_scan -B build/resource-census-scan -A x64
cmake --build build/resource-census-scan --config Release --parallel 1
ctest --test-dir build/resource-census-scan -C Release --output-on-failure
build/resource-census-scan/Release/resource_owner_scan.exe input.json output.json 64 64
```

The last two arguments are radius KiB around each known allocation and maximum
total successful ReadProcessMemory bytes in MiB (defaults64/64; upper bounds1024/256).
Known source neighborhoods are collected from actual HUD/window texture addresses.
A DynamicTexture candidate must match the verified vtable, its self-reference,
positive native Handle control count, target backing in both+0x70/+0x78, and exact
768²/1024² dimensions. The target set comes from stable native format40/flags131/
mip-byte4 rows in the input report. It can include a same-shaped scope texture;
shape alone does not establish that an object was created by the map census group.

Referrer search prioritizes the actual candidate texture/control allocations and
known VirtualWindow neighborhoods, then other known widget/layer allocations.
Exact `(object, control)` pairs are labeled as handle pairs, not automatically
strong owners: WeakHandle has the same layout. A potential VirtualWindow owner is
validated against known window vtables and its self/control fields at the proven
texture offset+0x4E8. Subsequent window referrers use the same bounded page set.

Output reports planned/visited byte coverage, rejected seed regions, truncated
pages, read errors, elapsed time and budget exhaustion. No-match means only that
these known neighborhoods contain no match; it is **not** a full process/heap
ownership proof. The source tree and installed headers establish these2.5 layouts;
repeat provenance checks before using on another game executable.

Offline identity tests passed. Initial live runs in a root-authorized idle window
read11.9/39.9/46.2MiB in5.3/4.9/6.4seconds, with zero read failures. They found three
live DynamicTexture objects, two matching previously observed map backing IDs,
but no referrers in the covered UI neighborhoods. Candidate/control-neighborhood
priority was subsequently run in another idle window with the same partial result.
The scanner also records bounded exact backing-pointer matches and small nearby
word contexts, allowing known DynamicTexture fields to act as a positive control.
These raw matches are not ownership claims: allocator residue can retain pointer
values after a container was destroyed. Native source/target validation rejected
all32 DrawBuffer-shaped residual records in the first such scan; five actual
DynamicTexture self/control identities were confirmed by separate small reads.
