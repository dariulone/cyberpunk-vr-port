# In-game A/B of the Rust port against the C++ reference

Every port is checked in the game, on the OpenXR simulator, against the C++ build of the same commit: the Rust build
(`build/patchday/sdk25-build`, `CVR_RUST_VRIK=1`) and the reference (`build/patchday/sdk25-cpp`) are deployed in turn,
each run through `tools/rust_migration/probe_session.ps1`, and what both did is compared. The game is closed after
each run (`probe_session.ps1` does that).

| File | What |
|---|---|
| `ab.ps1` | One A/B: deploys each build, runs a probe session (stick test, natives test), keeps the run folders under `build/rust/probe-runs/`, compares the natives (`natcmp.py`). `-IniLines 'k=v;...'` appends ini lines for both runs (restored after), `-Prep` runs a script before each run (and with `-Restore` after), `-Extra:'-SetExports A=1,B=2 -ReadExports C,D'` is passed to the probe session. Prints `RUST <run>` and `CPP <run>`. |
| `pageprep.ps1` | `-Prep` script installing the CET page probe below (and removing it with `-Restore`). |
| `pageprobe/CyberpunkVRPort_PageProbe/init.lua` | Reads every VR menu settings page through `VRSettingsPage` (the recorder, `Overlay/UiRec.hpp`), applies menu edits through `VRSettingsEdit` (tabs, element switches, OSR, pacing, panel buttons), prints each page escaped to `scripting.log`; then opens the VR menu for 40 s (`VROverlayToggle`). |
| `pagecmp.py <rust run> <cpp run>` | The recorded pages compared item by item (the recorder's pass counter masked). |
| `dumpcmp.py <rust run> <cpp run>` | The VR menu's ImGui draw data compared per tab: run with `-Extra:'-SetExports CyberpunkVR_ImGuiDump=60,CyberpunkVR_ImGuiDumpTabs=127'` (and `-IniLines 'xr_framegen_overlay=0'` so no live statistics are drawn); the overlay writes `imgui_dump_<tab>.txt` for each of the seven tabs. |
| `dumpdiff.py <a> <b> <tab> <list>` | The first differing vertices of one draw list. |
| `dumptext.py <dump> <list>` | The text of one draw list, decoded through the dump's glyph table. |
| `live_lines.py [N]` / `live_print.py <file> [a] [b]` | The C++ still built in the Rust build (`CVR_RUST_*` = 1), per folder and file / line by line. |

Expected differences: the live telemetry (FPS, frame times), the VRCAM live state in the two seconds after an OSR
switch, and the sample counts of `tools/rust_migration/xrprobe_layers.py` (its signatures must match).
