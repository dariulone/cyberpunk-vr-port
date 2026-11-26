# Physical melee controller regression

Build the trajectory generator and run from the repository root on Windows with the harness-enabled OpenXR Simulator. The manifest override applies to the test game process. Select a gameplay save and a melee weapon it already owns.

```powershell
cmake -S tools/vrik_tests/physical_melee -B build/physical-melee/trajectory -A x64
cmake --build build/physical-melee/trajectory --config Release
py -3.13 -B tools/vrik_tests/physical_melee/run.py --manifest C:/path/to/simulator.json --save AutoSave-7 --item Items.Preset_Katana_Takemura --out build/physical-melee/run-01 --screenshots
```

The driver installs a temporary observer, launches a single game, loads the specified save through QuickBoot, locks saving and draws the item. It obtains the equipped blade's direction and cutting edge from the game. A C++ generator builds timed controller translations and orientations for a forward slash, reverse slash, curved swing and thrust. Each path runs first in air and then against a newly spawned game character. The driver holds the end pose and withdraws and re-enters on the thrust trial.

The observer samples each game update independently of the controller sender. Per-trial JSON includes actual send times, sampled hand and blade positions, intersection depth on animated game hit shapes, target identity, health, the game's hit serial, the native joint anchor and a mark resolved from the rendered mesh. A broad-phase ray can hit an NPC's enclosing collider while the blade is still clear of its mesh and damage shapes. The starting-clear check uses the native capsule geometry. A generated motion path, an air whoosh or an accepted damage request alone cannot pass the contact checks.

Use `--cases swing reverse` to select trials. For a non-bladed weapon the checks use a confirmed player hit and target HP. The blade cases additionally require a mark resolved from the private skinned mesh. The optional `edge-press` and `flat-press` cases start from a lodged thrust and move the controller the same distance along the edge or perpendicular to its plane. Use a save with sufficient Strength for the positive edge case. Each report records the character's actual Strength and the follow-through receipts.

Each run writes to a new directory and exits nonzero if a measurement fails. With `--screenshots` both composited eye images are captured after contact. Controller telemetry and native geometry checks determine the pass result. The default mode ends its own process and removes the observer. `--pid` is for an existing disposable simulator session that already has the fixture installed and leaves that session running with saving locked.

Use `--yaw -25 --roll 35` to test an oblique cutting plane. The driver measures the actual edge and shaft again after positioning the controller. Use `--target-speed 1` for normal NPC reaction speed and `--video` for a 1080p left-eye recording of each contact. Slash and puncture trials check the renderer’s ink type and triangle count.

Use `--target-record` to select another installed character, and `--release-ai` to resume its ordinary reaction speed as the contact stroke starts. Withdrawal gets a further 24 cm of axial travel if the first pull ends inside the target. The report preserves both portions and the independent capsule samples.

For the fast reversal regression, use the Takemura katana on a fresh Valentinos target. After the first confirmed joint holds, the controller pulls 84 cm along the shaft and returns in 560 ms. There is one sample at the turning point and no pause for a fixture query. The same motion first runs in air.

```powershell
py -3.13 -B tools/vrik_tests/physical_melee/run.py --manifest build/reload-181704/simulator.json --save AutoSave-7 --item Items.Preset_Katana_Takemura --target-record Character.valentinos_grunt1_melee1_knife_ma --target-speed .005 --yaw 15 --roll -28 --cases quick-reentry --out build/physical-melee/quick-val-01
```

In `quick-reentry.json`, `released_before_return` requires the old receipt to release with clear sampled geometry and an open entry gate. `independent_puncture` requires fresh overlap, a new puncture receipt, a different joint key, one additional confirmed hit and lower HP on that same target. `eligible_on_release` and `return_overlap` distinguish a defeated target or a missed return trajectory. The ordinary `thrust` case remains the paused-withdrawal control.

`stealth-back` and `front-thrust` replay the same controller path against opposite NPC facings. The rear case checks initial detection, the confirmed body region, the selected damage budget and the observed health loss.

`animated-hold` plays a hit animation on the seated NPC with stationary controller input. `moving-release` displaces the disposable target laterally by 60 cm. Both record the animated attachment point, solved palm, raw input and joint identity throughout. The displacement case requires more than 48 cm of measured anchor travel and release of the original joint.
