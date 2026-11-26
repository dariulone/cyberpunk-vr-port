"""Regression: a right-grip squeeze cannot move a supported weapon to WeaponLeft."""
from pathlib import Path
import unittest
from lupa.luajit21 import LuaRuntime

ROOT=Path(__file__).resolve().parents[2]

class CarryDisabled(unittest.TestCase):
    def test_no_new_transfer_and_existing_carry_can_return(self):
        source=(ROOT/'mods/cet/CyberpunkVRPort_Weapon/init.lua').read_text(encoding='utf-8')
        declaration=next(line for line in source.splitlines() if line.startswith('local CARRY_LEFT'))
        start=source.index('local function carryTick(dt)')
        gate=source[start:source.index('    -- LEFT-HANDED',start)]
        lua=LuaRuntime()
        lua.execute(declaration+'\nlocal carryOn=false\n'+gate+'\n return "recover" end\n'
                    'assert(carryTick(.016)==nil);carryOn=true;assert(carryTick(.016)=="recover")')

if __name__=='__main__':unittest.main()
