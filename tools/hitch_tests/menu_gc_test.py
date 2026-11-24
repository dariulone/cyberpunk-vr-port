"""Exercise the production Stereo GC ownership policy with LuaJIT, without the game.

Run: python -I tools/hitch_tests/menu_gc_test.py
Requires lupa with its luajit21 runtime. --source can select an older init.lua.
The host is mocked: these tests check ownership, not native collector timing.
"""
from pathlib import Path
import argparse
import sys
import unittest

from lupa.luajit21 import LuaRuntime

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "mods/cet/CyberpunkVRPort_Stereo/init.lua"


class MenuGcTests(unittest.TestCase):
    def setUp(self):
        self.lua = LuaRuntime(unpack_returned_tuples=True)
        self.lua.execute('''
events = {}
selector = {preGame = false}
preGame, paused, inMenu = false, false, false
missingRequests, stateError, missingBoard, boardError = false, false, false, false
hostAvailable, hostWanted, hostCalls = true, nil, 0
heap, steps, stops, lastStep, finished = 1000, 0, 0, nil, false
actualGc = collectgarbage
function registerForEvent(name, callback) events[name] = callback end
function registerHotkey(...) end
function require(name)
    if name == "modules/vrcam_select" then return selector end
    return {}
end
Game = {GetSystemRequestsHandler = function()
    if stateError then error("request handler unavailable") end
    if missingRequests then return nil end
    return {IsPreGame = function() return preGame end, IsGamePaused = function() return paused end}
end,
GetAllBlackboardDefs = function() return {UI_System = {IsInMenu = "in-menu"}} end,
GetBlackboardSystem = function()
    if boardError then error("blackboard unavailable") end
    return {Get = function()
        if missingBoard then return nil end
        return {GetBool = function(_, key) assert(key == "in-menu"); return inMenu end}
    end}
end}
function VRLuaHostGc(wanted)
    hostCalls = hostCalls + 1
    hostWanted = wanted
    return hostAvailable and wanted
end
function collectgarbage(command, amount)
    if command == "count" then return heap end
    if command == "stop" then stops = stops + 1; return end
    assert(command == "step", "unexpected GC operation: " .. command)
    steps, lastStep = steps + 1, amount
    return finished
end
function update(count)
    for _ = 1, (count or 1) do events.onUpdate(1 / 90) end
end
''')
        self.mod = self.lua.execute(SOURCE.read_text(encoding="utf-8"))
        self.g = self.lua.globals()

    def test_gameplay_preserves_host_idle_pacing(self):
        self.lua.execute("update(100)")
        self.assertTrue(self.g.hostWanted)
        self.assertEqual(self.g.hostCalls, 100)
        self.assertEqual(self.g.steps, 0)

    def test_paused_map_and_settings_take_menu_pacing(self):
        # IsPreGame remains false for the in-game map/settings.
        self.lua.execute("paused = true; update(10000)")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.steps, 10000)
        self.assertEqual(self.g.stops, 10000)
        self.assertEqual(self.g.lastStep, 192)

    def test_pause_is_seen_before_selector_next_tick(self):
        self.lua.execute("update(); paused = true; update()")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.steps, 1)
        self.assertEqual(self.g.lastStep, 192)
        self.lua.execute("paused = false; update()")
        self.assertTrue(self.g.hostWanted)
        self.assertEqual(self.g.steps, 1)

    def test_repeated_map_close_and_settings_cycles_restore_host(self):
        self.lua.execute('''
for _ = 1, 100 do
    paused = true; update(10)
    assert(not hostWanted)
    paused = false; update(10)
    assert(hostWanted)
end
''')
        self.assertEqual(self.g.steps, 1000)
        self.assertEqual(self.g.hostCalls, 2000)

    def test_lobby_retains_existing_menu_budget(self):
        self.lua.execute("preGame = true; update(10)")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.steps, 10)
        self.assertEqual(self.g.lastStep, 192)
        # The camera selector may have kept an old preGame value on its disabled-camera fast path.
        self.lua.execute("preGame = false; selector.preGame = true; update()")
        self.assertTrue(self.g.hostWanted)
        self.assertEqual(self.g.steps, 10)

    def test_hub_map_time_dilation_without_pause_uses_menu_budget(self):
        self.lua.execute("inMenu = true; update(10000)")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.steps, 10000)
        self.assertEqual(self.g.lastStep, 192)
        self.lua.execute("inMenu = false; update()")
        self.assertTrue(self.g.hostWanted)
        self.assertEqual(self.g.steps, 10000)

    def test_missing_or_throwing_blackboard_keeps_collection_running(self):
        self.lua.execute("missingBoard = true; update()")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.lastStep, 192)
        self.lua.execute("missingBoard = false; boardError = true; update()")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.steps, 2)
        self.lua.execute("boardError = false; update()")
        self.assertTrue(self.g.hostWanted)

    def test_menu_budget_collects_real_luajit_garbage_without_idle_wait(self):
        # Use real LuaJIT collection and weak refs. There are deliberately no host idle callbacks.
        self.lua.execute('''
collectgarbage = actualGc
actualGc("collect"); actualGc("stop")
inMenu = true
weak = setmetatable({}, {__mode = "v"})
for i = 1, 10000 do weak[i] = {string.rep("x", 256)} end
update(500)
remaining = 0
for _ in pairs(weak) do remaining = remaining + 1 end
''')
        self.assertEqual(self.g.remaining, 0)
        self.assertFalse(self.g.hostWanted)

    def test_standalone_cet_without_native_host(self):
        self.lua.execute("VRLuaHostGc = nil; update()")
        self.assertEqual(self.g.lastStep, 16)
        self.lua.execute("paused = true; update()")
        self.assertEqual(self.g.lastStep, 192)
        self.assertEqual(self.g.stops, 2)

    def test_disabled_host_preserves_gameplay_fallback(self):
        self.lua.execute("hostAvailable = false; update()")
        self.assertEqual(self.g.lastStep, 16)
        self.lua.execute("paused = true; update()")
        self.assertEqual(self.g.lastStep, 192)

    def test_missing_or_throwing_state_does_not_starve_collector(self):
        self.lua.execute("missingRequests = true; update()")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.lastStep, 192)
        self.lua.execute("missingRequests = false; stateError = true; update()")
        self.assertFalse(self.g.hostWanted)
        self.assertEqual(self.g.steps, 2)
        self.lua.execute("stateError = false; update()")
        self.assertTrue(self.g.hostWanted)

    def test_allocation_catchup_keeps_existing_cap_and_stop(self):
        self.lua.execute("paused = true; update(); heap = 5000; update()")
        self.assertEqual(self.g.lastStep, 1024)
        self.assertEqual(self.g.stops, 2)

    def test_optional_rest_is_cleared_when_host_takes_over(self):
        self.mod.gc.restGrowth = 1.2
        self.lua.execute("paused = true; finished = true; update(); update()")
        self.assertEqual(self.g.steps, 1)
        self.assertEqual(self.g.stops, 2)
        self.lua.execute("paused = false; update(); paused = true; update()")
        self.assertEqual(self.g.steps, 2)
        self.assertEqual(self.g.lastStep, 192)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--source", type=Path)
    options, remaining = parser.parse_known_args()
    if options.source:
        SOURCE = options.source.resolve()
    unittest.main(argv=[sys.argv[0], *remaining])
