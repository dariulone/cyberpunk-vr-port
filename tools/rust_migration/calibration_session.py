"""vrik_calibration.ini saved in the game by the port's own save (cvr_xr_save_calibration, called on a thread of its own:
call_in_game.py) right after the game loaded it: the file must come out byte for byte as it went in -- the values the
load read, printed back as the C++ half printed them. The file is put back as it was either way. Prints one JSON line.
The game must be in the world (tools/quickboot_tests/launch_to_save.py) and is left running."""
import json
import sys
from pathlib import Path

from call_in_game import call

ini = Path(sys.argv[1])
before = ini.read_bytes()
result = call('cvr_xr_save_calibration')
after = ini.read_bytes()
if after != before:
    ini.write_bytes(before)
result.update(saved=bool(result.get('returned', 0) & 0xFF), identical=after == before, bytes=len(after))
print(json.dumps(result))
