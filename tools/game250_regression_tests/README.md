# Physical reload regression checks

Run from the repository root with Python 3 and `lupa` installed:

```sh
python3 tools/game250_regression_tests/test_crane_rest.py
python3 tools/game250_regression_tests/test_body_mag_reach.py
python3 tools/game250_regression_tests/test_revolver_exit.py
g++ -std=c++17 -Iinclude tools/game250_regression_tests/test_rig_tracks.cpp -o /tmp/rig-tracks
/tmp/rig-tracks
```

The Lua checks execute production functions/modules with engine calls stubbed.
They cover closed-pose stability, body pickup with tracking validation, and round
extraction across 30/60/90 Hz. The C++ check calls the production float-track mapper
and verifies remapped destinations, source/destination bounds and invalid floats.
These checks supplement headset testing; they do not emulate the full game animator.
