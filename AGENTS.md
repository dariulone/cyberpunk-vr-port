See `CONTRIBUTING.md` for repository layout, `cargo xtask` build entrypoints, and test/benchmark harnesses.

# VR live validation

- A normal `launch_game` call only opens the launcher or executable. It is not the save autostart path.
- For an owned autostart, stop any existing `Cyberpunk2077.exe`, make the OpenXR Simulator active with `register-runtime.ps1` using the absolute `openxr_simulator.dll`, then run `py -3.13 -B tools/quickboot_tests/launch_to_save.py --timeout 300 --save <save> --simulator <absolute openxr_simulator.dll>`.
- QuickBoot clicks `Start Game`, waits for native loading readiness, loads the latest save or the named save, confirms the loading screen through the simulator and reports `world_ready` with fresh HUD frames.
- Scope validation uses automatic weapon sights. Do not treat trigger input as aiming and do not fire into the test scene. Equip one test weapon at a time, keep the input neutral, and solve that weapon's own hand position and orientation so its scope sits at the right eye. Weapon heights, scope relief and mounts differ between profiles, so never reuse one eye position for another sight. After alignment, move the simulator HMD a small amount left and right and compare the reticle with the eye line.
- Capture the actual stereo result through `from simxr import Sim` and `Sim.shot(path, eyes='both', eye_size=0, fmt='jpg')`. `screenshot.bmp` and `capture_preview.py` are mirror or window diagnostics and are not the primary scope evidence.
- A current shader validation album must never reuse pre-change eye captures. Store the current DLL/run identity with the images and leave a profile unverified when its weapon or attachment cannot be installed in the live fixture.
- Restore simulator pose and controller bytes after every capture, release all inputs, and close the owned game before switching or deploying.
