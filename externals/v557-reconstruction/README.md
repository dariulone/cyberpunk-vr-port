# V557 reconstruction subset

LGPL-3.0: see ../../licenses/OFXR-LGPL-3.0.txt. Source attribution and original hashes are in provenance.json. Copied from the known V557 Cyberpunk backend checkout; no OpenXR layer, presentation scheduler, injector, or tray is included. Shaders build from source with Windows SDK fxc, using the port's pinned FidelityFX and NVIDIA OFA headers.

Port adaptations: history accepts already-captured COPY_SOURCE resources and a changing source pointer, with identical descriptions/device required. The adapter retains the color lease until its GPU receipt completes; it changes source only after the preceding submission completes. No redundant staging texture is used. Composition is the unmodified NVIDIA-only, unidirectional V557 shader with cost confidence and pose pre-warp. No game motion vectors are supplied to this backend. External OFXR live tuning uses an isolated mapping name.

The port-owned NVIDIA output path additionally supports COPY_SOURCE destinations with transitions inside composition and a deferred display-consumer fence. Allocation reporting excludes opaque driver memory. See ../../docs/fixes/v557-reconstruction.md for tests, contracts and tradeoffs.

The native reconstruction-only mode omits unused real endpoint copies and permits the priming destination to alias synthetic output. It is restricted to NVIDIA flow with advancing capture tickets and no game guides; the default/reference API retains the original two-output behavior.

Native mono callers may defer reconstruction until both eyes have submitted
packing/OFA, then call `flush_reconstruction` before joining any completion
receipt. This avoids parking the other eye behind a prior display-consumer wait.
The default/reference call still submits the full transaction. No shader change.

The native adapter can also borrow immutable-for-the-lease capture textures in
GENERIC_READ. This opt-in history mode imports the original producer fence and a
mandatory owner pin instead of allocating/copying a history ring. Previous images
retire only after their GPU consumer receipt; current images remain pinned for
the next pair. Only NVIDIA reconstruction-only mode accepts borrowed history.
The reference API defaults to owned history and retains its original behavior.
