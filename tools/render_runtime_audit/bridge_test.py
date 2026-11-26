from pathlib import Path
from lupa.luajit21 import LuaRuntime

root = Path(__file__).resolve().parents[2]
LuaRuntime().execute(Path(__file__).with_suffix('.lua').read_text(encoding='utf-8'), root.as_posix())
