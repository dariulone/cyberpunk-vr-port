"""Bounded simulator pad input in a background CP2077 process.

CP2077's pad thread polls in the background, but 0x140291D04 (2.5: 0x1420849A0)
only drains its event queue when GetForegroundWindow equals the game HWND. For this test scope
only, bypass that single condition, then restore its exact bytes. No window
activation or keyboard input. The actual button/axis events still come through
OpenXR -> XInput -> the game's normal action dispatch (including listeners).
"""
import ctypes as c,struct
from pathlib import Path
import sys
sys.path[:0]=[str(Path(__file__).resolve().parents[1]/p) for p in ('roomscale_tests','swimming_tests')]
from live_read import Reader
from live_symbols import resolve,loaded_module
class PadGate:
 def __init__(self,pid,game,simulator):
  self.r=Reader(pid,game/'bin/x64/Cyberpunk2077.exe',0,0);self.k=self.r.k
  self.k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
  self.k.VirtualProtectEx.argtypes=[c.c_void_p,c.c_void_p,c.c_size_t,c.c_ulong,c.POINTER(c.c_ulong)]
  self.k.FlushInstructionCache.argtypes=[c.c_void_p,c.c_void_p,c.c_size_t]
  self.h=self.k.OpenProcess(0x828,False,pid);assert self.h
  self.n=c.WinDLL('ntdll');self.n.NtSuspendProcess.argtypes=[c.c_void_p];self.n.NtResumeProcess.argtypes=[c.c_void_p]
  ex=resolve(pid,game/'bin/x64/Cyberpunk2077.exe',[])
  # exe sha256 -> the `jne` after `cmp rax, [rbx + hwnd]` that follows the GetForegroundWindow call
  sites={'a7de82945c03e041fc7339fcf9066224d98db2f5d80fea50f7947bb350a60991':0x291d2a,   # 2.31
         'd36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4':0x20849c6}  # 2.5
  assert ex['sha256'] in sites,'Unsupported game input ABI'
  self.site=int(ex['module_base'],16)+sites[ex['sha256']]
  self.saved=self.r.read(self.site,2)
  # The port's own BackgroundPad hook (src/Hooks/BackgroundPad.cpp) replaces this compare with a jump while the
  # OpenXR session is focused, so the bytes are its NOPs: nothing to open then, and nothing to put back.
  self.ported=self.saved==bytes.fromhex('9090')
  assert self.ported or self.saved==bytes.fromhex('750F'),self.saved.hex()
  # The game loads whichever simulator the OpenXR loader picked (active_runtime.json), not
  # necessarily the copy a caller passed; another copy failed the exact-image check and the
  # Continue press never happened. Pad offsets come from the loaded image itself.
  loaded=loaded_module(pid,Path(simulator).name)
  if loaded and loaded.is_file() and loaded.resolve()!=Path(simulator).resolve():
   print(f'PadGate: game loaded {loaded}, not {simulator}; using the loaded simulator',flush=True)
   simulator=loaded
  sim=resolve(pid,simulator,[]);base=int(sim['module_base'],16)
  from native_body_live import controller_rvas
  self.controllers=[base+x for x in controller_rvas(simulator)]
  self.inputs=[self.r.read(p+21,23) for p in self.controllers]
 def write(self,address,data):
  b=c.create_string_buffer(bytes(data));done=c.c_size_t();assert self.k.WriteProcessMemory(self.h,address,b,len(data),c.byref(done)) and done.value==len(data)
 def code(self,data):
  if self.ported:return
  assert self.n.NtSuspendProcess(self.h)==0
  try:
   protection=c.c_ulong();assert self.k.VirtualProtectEx(self.h,self.site,2,0x40,c.byref(protection))
   try:self.write(self.site,data);assert self.k.FlushInstructionCache(self.h,self.site,2)
   finally:
    dummy=c.c_ulong();assert self.k.VirtualProtectEx(self.h,self.site,2,protection.value,c.byref(dummy))
  finally:assert self.n.NtResumeProcess(self.h)==0
 def __enter__(self):self.code(b'\x90\x90');return self
 def stick(self,x=0,y=0):self.write(self.controllers[0]+36,struct.pack('<2f',x,y))
 def jump(self,down):self.write(self.controllers[1]+24,bytes([bool(down)]))
 def __exit__(self,*args):
  try:
   try:
    for p,data in zip(self.controllers,self.inputs):self.write(p+21,data)
    import time;time.sleep(.1)
   finally:self.code(self.saved)
  finally:self.k.CloseHandle(self.h);self.r.close()
