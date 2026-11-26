# VR attachment recoil handoff

Run this check after the runtime owner installs the matching Release plugin and CET weapon module. It measures the virtual hand pose in Cyberpunk 2077.

## Results on PID 28432

The installed DLL had SHA-256 `e78c8206f2dc0c10c4bd665a1c3d5e8e257948157f76376e7aef33935f5d0094`. The Kyubi kept the same inventory instance through all six phases. Its installed-slot readback and published factors agreed throughout. The original five-shot median test used an absolute ratio tolerance of 0.03.

| Kyubi muzzle | Factor | Median rendered rise (degrees) | Ratio to first bare median | Peak test |
| --- | ---: | ---: | ---: | --- |
| Bare | 1.000 | 2.981 | 1.000 | Baseline |
| 01, Q = 1 | 0.900 | 2.565 | 0.861 | Fail |
| 01_legendary | 0.800 | 2.277 | 0.764 | Fail |
| Removed | 1.000 | 2.961 | 0.993 | Pass |
| 03 | 1.000 | 2.969 | 0.996 | Pass |
| Removed again | 1.000 | 2.842 | 0.953 | Fail |

For the single left-handed Liberty the bare and 08 medians were 9.791° and 8.544° (ratio 0.873, expected 0.875). Focus interrupted the subsequent swap series. The right-handed Liberty's corresponding ratio was 0.839, and its final bare ratio was 0.945. In the dual Unity routing check, 13 of 14 isolated pulls produced exactly one round and one impulse on the intended gun. The final partner pull after removal produced zero on both hands. Both factors had returned to 1.000. That phase is incomplete.

The exact scale check ran offline against production `RecoilSample`. The test samples a frozen spring under one weapon and grip, switches the primary attachment factor from 1.0 to 0.8, samples again, then restores 1.0 and checks both axes. Single-grip travel and rise ratios were 0.800000012 and 0.800000072. Supported-grip ratios were both 0.800000012. The impulse enters the spring upstream of this pose-output multiplication. Separate live shots sample the spring at different solve times. Their raw peak verdicts remain in the archive. [Paired native samples](evidence/vr-attachment-recoil-28432/paired-output-offline.txt) · [Raw live observations and restoration](evidence/vr-attachment-recoil-28432/raw-observations.zip)

For another boot, use the corrected observer in this workspace. It checks the weapon's occupied and empty modification slots. On the tested build `HasAttachmentSlot` reported false even while the empty-slot list contained `PowerModule`. The live checks used the corrected snapshot via CVRBridge.

## Automated simulator run

Use the boot-loaded observer at `red4ext/plugins/cyberpunkvrport/lua/CPVR_AttachmentRecoilTest/init.lua`. Its source is `tools/vrik_tests/fixtures/attachment_recoil/init.lua`. The combined plugin must register `GetVRLastShotPeak` and `SetVRAttachmentRecoil`. The observer exposes a read-only snapshot and uses no boot or update handler.

Start with the handed-off, save-locked simulator session in gameplay, a stationary controller pose, bare muzzle slots and at least six loaded rounds in each test weapon. Use a semi-auto shot per squeeze. For the rifle case, draw a compatible rifle such as a Kyubi and have the 01, 01_legendary and 03 muzzle records below in inventory. For pistol cases, have the 08 and 07 handgun muzzles available. Use two distinct copies of the same pistol record for dual wield. For the supported case, position the off hand on the support point first.

From the workspace root, substitute the handed-off game PID and choose a new report filename for each run.

```powershell
py -3.13 -B tools/vrik_tests/live_attachment_recoil.py <PID> rifle.json --case rifle
py -3.13 -B tools/vrik_tests/live_attachment_recoil.py <PID> left.json --case left-pistol
py -3.13 -B tools/vrik_tests/live_attachment_recoil.py <PID> dual.json --case dual
py -3.13 -B tools/vrik_tests/live_attachment_recoil.py <PID> support.json --case rifle --supported
```

The runner uses the inventory modification request queue to equip, swap and remove the installed muzzle and the simulator preview's trigger bindings to fire. It captures both guns before and after each shot. A valid shot spends one round and advances one native shot sequence on the firing gun. The other gun must retain its ammo and sequence. Each hand's five-shot median is compared with its own bare median. The report contains installed IDs, quality, weapon instances, entities, factors, ammo, shot sequences, peaks and restoration results. Exit 0 requires all completed ratios within 0.03 and successful cleanup.

Cleanup releases the owned preview keys, returns the test slots to bare and verifies the starting magazine counts. Run on the disposable save-locked fixture because shots can also affect world state and reserves. The native per-shot snapshot is independent of the existing peak latches and the diagnostics switch.

## Single weapon

Use one non-iconic rifle with an available muzzle slot, an empty mod configuration, a fixed controller pose and a stable frame rate. Keep all recoil settings and grip state fixed. Use the inventory to change only the attachment on the drawn weapon. Record whether the game recreates the weapon entity on each change.

| Installed in `AttachmentSlots.PowerModule` | Expected primary scale |
| --- | --- |
| Empty | 1.000 |
| `Items.w_muzzle_brake_01` | `1 - 0.075 - 0.025 × Q` |
| Swap directly to `Items.w_muzzle_brake_01_legendary` | 0.800 |
| Remove the legendary brake | 1.000 |
| Equip `Items.w_muzzle_brake_03` (ricochet) | 1.000 |

`Q` is the installed part's raw Quality stat. The regular brake authors its peak kick on `RecoilKickMax` in inline records 2 and 3. The legendary variant authors a fixed value in inline record 1. The corresponding minimum-kick and tooltip rows describe the same brake. The VR spring uses a single peak amplitude for rise and travel. Angle and direction rows describe recoil pattern geometry.

After every change, this CET console command reports the factors, weapon entity IDs and read errors.

```lua
local s = GetMod('CyberpunkVRPort_Weapon').AttachmentRecoilStatus(); print(s.primaryWid, s.primary, s.primaryError, s.partnerWid, s.partner, s.partnerError)
```

Read back the installed muzzle and its quality on the active gun. Repeat with `GameObject.GetLeftActiveWeapon(p)` for the left gun of a pair.

```lua
local p = Game.GetPlayer(); local w = p:GetActiveWeapon(); local d = Game.GetTransactionSystem():GetItemData(p,w:GetItemID()); local s = TweakDBID.new('AttachmentSlots.PowerModule'); if d:HasPartInSlot(s) then local part = d:GetItemPart(s); print(TDBID.ToStringDEBUG(ItemID.GetTDBID(InnerItemData.GetItemID(part))),InnerItemData.GetStatValueByType(part,gamedataStatType.Quality)) else print('empty') end
```

1. Check the reported factor against the table within 0.001. The equip or remove event schedules the read for the next weapon update. A direct inventory change that omits that event should reconcile within 0.5 seconds of gameplay updates. Both error fields should be nil.
2. For each row, take five isolated single shots at least one second apart. Read `GetVRLastShotPeak(1).x` after each primary shot or `GetVRLastShotPeak(0).x` for the dual partner. The snapshot holds that shot's peak degrees until the next shot. Confirm one round consumed and a single sequence advance.
3. Divide each row's median peak in degrees by the initial bare median. Require a nonzero bare median and an absolute difference of at most 0.03 between measured ratio and expected scale. The removal and ricochet rows should return to within 3% of bare. Save the five peaks, median, installed record, Q, entity ID and reported scale for each row.

## Hand ownership

* Repeat with a single left-handed pistol using `Items.w_muzzle_brake_08`. Its expected scale has the same quality formula as the regular rifle brake. The single left-handed gun uses the primary factor and primary shot snapshot.
* Dual wield two compatible pistols of the same weapon record. Establish a bare median for each. Test the 08 brake on each gun in turn, including a direct swap to the angle-only 07 brake. Fire five isolated shots from each gun for every condition. Require the scaled and bare ratios independently within 0.03. Remove the brake and verify both factors read 1.000.
* With one weapon, repeat bare and brake measurements with the support grip fully engaged. Compare against that grip's own bare median. Holster both guns and verify both factors read 1.000. Redraw the modified gun and verify its scale returns.
