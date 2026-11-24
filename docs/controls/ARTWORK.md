# Controller artwork

Each context diagram embeds a reduced-size copy of the controller illustrations
from `D:\build\CyberFinger_SteamVRFusion`. The [offline guide](cyberfinger-controls.html)
opens those same images.

- `resources/icons/steamvr_cybrfngr_left_transp.png`
- `resources/icons/steamvr_cybrfngr_right_transp.png`

Their control positions follow that repository's `docs/manual/manual.html`.
The source repository's GPL license is reproduced in
[ARTWORK-LICENSE.txt](ARTWORK-LICENSE.txt) for the embedded artwork. The control
labels and generator belong to this project.

To regenerate all diagrams, use Python with Pillow installed and run
`python scripts/generate_cyberfinger_diagram.py --driver D:\build\CyberFinger_SteamVRFusion`.
