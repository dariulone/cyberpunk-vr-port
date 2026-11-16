# RR scope full resolution — 0.2.2 test

On top of the tested scope isolation patch, enable `xr_scope_rr_full_resolution=1`
(default1, read and preserved by LiveControlsPoll). Set0 and restart to return
to the previous scope resolution while retaining RR isolation. No new F10 control.

Only the named vr_optics_feed with RR feature71 is eligible; MAIN/VRCAM, SR-only,
invalid bounds or unknown native code pass through unchanged. The hook verifies
98 bytes of the installed CP2.5 native scaler, and requires the working native
DLSS lock/viewport and RR wrapper. During its resolution calculation ONLY,
mask the view's scale-select bits6,7,8,9,10,12,13,14,27 so the original scaler
chooses the target size (normally1024 square). Restore the complete feature word
before returning to any graph builder. Do not permanently remove RR from its
graph; that earlier experiment broke required resources and was rejected.
The engine computes all sizes, previous values, duplicate fields and bounds;
no manual post-build dimensions patch or MAIN quality change is used.

Existing private RR options correction matches1:1 at the new resolution, and
scope state remains excluded from headset per-eye processing and framegen.
Higher scoped GPU cost is expected:1024 squared has about9 times the pixels of
341 squared, but this is only the scope image, not the full headset frame.
Actual render/eval sizes must be confirmed in the next log. Rendering at full
camera resolution will not necessarily remove temporal softness in every scope.

Regression tests cover scope/RR-only eligibility, invalid extents, preserved
unrelated flags and nested/exception restoration. GCC warnings-as-errors and
ASan/UBSan pass (detect_leaks=0); MSVC Release passes. PE/Capstone evidence and
matching native guard retained under resolution-evidence/. REA Ghidra request
did not finish and was cancelled; no decompilation result is claimed.

Deployed09:29:01, DLL01812a44b6f239e1763c98e243c3a3111ad0329456d4a1814589d9edc4c18494.
Receipt resolution-deployment.json; prior stableDLL53482bcd and originalINI in
backups/scope-stable-before-resolution-20261106-092901. Only DLL and the newINI
key changed; game graphics settings preserved. No framegen or pacing changes.
Apply scope-only.patch to stock0.2.2 for isolation, then scope-resolution.patch
for this candidate. Headset clarity, framing and performance validation pending.
