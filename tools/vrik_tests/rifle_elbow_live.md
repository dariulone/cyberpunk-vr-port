# Near-chest rifle elbow A/B

Run from the repository root in PowerShell after building and installing the plugin containing this probe. The driver attaches to the PID you supply. A/B uses one build, one rifle and one held controller pose. Mode 1 bypasses only the near-chest rifle correction. Mode 2 uses the shipping correction. Both run the ordinary elbow policy.

## Setup

- Use the port's OpenXR simulator with the controller harness, native engine stereo with depth submission (`xr_depth_submit=1`), native hand packets and controller VRIK mode 4 with the ordinary elbow policy enabled. Disable full-body-tracking arm override and frame generation for this test. Enable the full player body and visible arms.
- Load a standing, on-foot player in open space with roughly a metre of clearance behind and to the right. Draw a right-hand rifle. An Ajax is suitable. Leave the other weapon slot empty. Exit menus, release all controls, face straight ahead with the simulator head level and leave the game and simulator preview unfocused while the driver runs. The driver places the off hand forward and uses the existing grip state. It does not engage the support magnet.
- The port's existing CVRBridge must be responsive. The driver uses it to check the active weapon and temporarily offset the first-person camera for a direct rear-right stereo view of the arm. Give the driver exclusive simulator and capture access for the run.
- Use 64-bit Python 3.13 with NumPy, Pillow and Tk. The stereo image decoder is compiled C++. To build it with the installed Visual Studio C++ tools, run `& .\tools\vr_bench\build_stereo_check.bat`.

## Capture

```powershell
$gamePid = (Get-Process -Name Cyberpunk2077 -ErrorAction Stop).Id
$out = Join-Path $PWD ('build/rifle-elbow-live/' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
py -3.13 -B tools/vrik_tests/rifle_elbow_live.py --pid $gamePid --out $out
```

The target is **10 cm inward, 7 cm forward, 6 cm above** the right shoulder. The driver samples the wrist at the same controller command before and after moving the observer. It measures the three columns of the controller-to-wrist Jacobian with ±35 mm probes, verifies the return to center, then solves for bounded controller corrections. It allows at most two probe sets and six corrections per set, with a 1.30 m envelope around the original test command. The convergence gate is 10 mm. Each command, wrist response, residual and fitted matrix goes to `placement.json`, including on a failed placement.

The controller commands then stay fixed. Each phase settles for 1.2 seconds and requests one three-frame stereo batch with a 15-second capture deadline. Per-phase gates check fresh solves, stability and the wrist offset within 15 mm. The two phases must agree within 3 mm and approximately 1° of wrist rotation. The camera position, basis, FOV and rifle identity must also agree. Both eyes must have an identical nonzero pose ID with a matching final model-space joint sample. The driver prefers the middle frame and tries the other two if that frame lacks an exact match.

**Results** are in `$out\angles.json`. Each angle is in degrees, with separate before, after and after-minus-before values. The opening angle is shoulder–elbow–wrist (180° at full extension). Upper-arm elevation is positive above horizontal. Pole elevation measures the elbow's displacement perpendicular to the shoulder–wrist line, positive upward. A bend-plane flip can preserve the opening angle.

`capture.json` records the DLL hash, weapon instance, target and wrist samples, joint coordinates, render frame and eye pose IDs, camera calibration, restoration checks and any failure. Each phase folder contains `L.jpg`, `R.jpg` and `SBS.jpg`. The engine's unmodified raw color/depth batches remain at the two paths recorded under `raw`. The capture request, camera, simulator inputs and rifle switch are restored before image decoding. Restoration after process termination or a disconnected bridge cannot be guaranteed. A failed run must be read from its status and error in `capture.json`.

## Independent rendered check

```powershell
py -3.13 -B tools/vrik_tests/rifle_elbow_review.py $out
```

In each of the four images, click the centers of the anatomical **right shoulder, elbow and wrist**, in that order. Enter accepts the three marks, Backspace undoes one, and Esc records incomplete evidence if a landmark is obscured. Use the actual arm, with the same landmarks in both eyes. The review runs entirely offline and writes `visible-angles.json` and marked copies beside the images.

The exact numerical source in `angles.json` is the completed player FK matched to the captured eye pose IDs. The independent visible angles are estimates of joint centers from rendered surfaces. They use the recorded camera positions, axes, asymmetric frusta and full-size image coordinates. Marks must reproject within four source pixels per eye and produce segment lengths within 4 cm of the matched rig. Inspect the marked images before accepting a result. An eye-pose match alone cannot establish the final skin deformation.

## Ajax simulator result

The operator's run on PID 27532 is in `build/physical-melee/elbow-live2`. Moving the camera shifted the wrist guide at a fixed controller command by **+0.406 m right, −0.909 m forward and −0.013 m up**. The first measured Jacobian coupled all three axes. Near the target, one metre of simulator −Z gave 1.051 m body-forward motion. The final placement residual was 0.080 mm.

| Matched final right-arm measurement | Before, frame 14901 | After, frame 15017 |
| --- | ---: | ---: |
| Elbow opening | 27.642° | 27.642° |
| Upper-arm elevation | +71.887° | −27.977° |
| Pole elevation | +58.290° | −41.321° |

Both stereo images show the upright elbow above the shoulder disappearing in the corrected pose. The run recorded successful solver, camera, controller and body-state restoration.

### Review of the recorded skin and depth

The review used all four original 2648 × 2864 eye images. In frame 14901 the pointed light patch on the raised elbow was identifiable in both eyes. Triangulation put the patch at **0.659 m forward depth** in each eye, within 2 mm of the rendered depth. In frame 15017 both former sightlines meet the stairwell. The buttstock rib stayed visible at the same pixels. Depth values below are the median of a 3 × 3 texel patch in each eye's corresponding native depth image. [Marked stereo crops](../../docs/evidence/vr-feel-20261118/rifle-elbow-stereo.jpg)

| Sightline | Before L | Before R | After L | After R |
| --- | ---: | ---: | ---: | ---: |
| Raised-elbow patch | 0.657 m | 0.657 m | 7.062 m | 6.596 m |
| Buttstock rib | 0.697 m | 0.697 m | 0.697 m | 0.697 m |

This verifies the raised elbow silhouette cleared the rifle in the actual render. The off arm crosses the right shoulder and wrist in the baseline and overlaps the corrected elbow. The right hand on the grip becomes visible, but the wrist center is indistinct. An independent three-joint **visible angle is unresolved** in both phases. The review records null angles for those phases. The signed angles in the first table come from the completed player joints matched to the captured frames. [Image hashes and inspected pixels](../../docs/evidence/vr-feel-20261118/rifle-elbow-marks.json) · [Review output](../../docs/evidence/vr-feel-20261118/rifle-elbow-visible.json)

Replay this inspection locally using the saved eye images and depth batches.

```powershell
py -3.13 -B tools/vrik_tests/rifle_elbow_review.py build/physical-melee/elbow-live2 --marks docs/evidence/vr-feel-20261118/rifle-elbow-marks.json
```
