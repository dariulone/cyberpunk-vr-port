"""Read only: enumerate windows owned by the specified game process."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--pid',type=int,required=True);a=p.parse_args()
u=c.WinDLL('user32',use_last_error=True)
# Report physical pixels regardless of desktop DPI (the bad 1024x768 launch
# otherwise appeared as 819x614 on this machine's 125% desktop scale).
u.SetProcessDpiAwarenessContext.argtypes=[c.c_void_p];u.SetProcessDpiAwarenessContext.restype=w.BOOL
u.SetProcessDpiAwarenessContext(c.c_void_p(-4))
callback=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
u.EnumWindows.argtypes=[callback,w.LPARAM];u.EnumWindows.restype=w.BOOL
u.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)];u.GetWindowThreadProcessId.restype=w.DWORD
u.GetClassNameW.argtypes=[w.HWND,w.LPWSTR,c.c_int];u.GetWindowTextW.argtypes=[w.HWND,w.LPWSTR,c.c_int]
u.IsWindowVisible.argtypes=[w.HWND];u.IsWindowVisible.restype=w.BOOL
u.GetClientRect.argtypes=[w.HWND,c.POINTER(w.RECT)];u.GetClientRect.restype=w.BOOL
u.GetWindowRect.argtypes=[w.HWND,c.POINTER(w.RECT)];u.GetWindowRect.restype=w.BOOL
u.IsIconic.argtypes=[w.HWND];u.IsIconic.restype=w.BOOL
rows=[]
@callback
def visit(hwnd,_):
    pid=w.DWORD();u.GetWindowThreadProcessId(hwnd,c.byref(pid))
    if pid.value==a.pid:
        name=c.create_unicode_buffer(256);title=c.create_unicode_buffer(1024)
        u.GetClassNameW(hwnd,name,256);u.GetWindowTextW(hwnd,title,1024)
        client=w.RECT();outer=w.RECT();u.GetClientRect(hwnd,c.byref(client));u.GetWindowRect(hwnd,c.byref(outer))
        rows.append({'handle':hex(hwnd),'class':name.value,'title':title.value,'visible':bool(u.IsWindowVisible(hwnd)),
            'minimized':bool(u.IsIconic(hwnd)),
            'client':[client.right-client.left,client.bottom-client.top],
            'rect':[outer.left,outer.top,outer.right,outer.bottom]})
    return True
if not u.EnumWindows(visit,0):raise c.WinError(c.get_last_error())
games=[r for r in rows if r['class']=='W2ViewportClass']
small=any(not r['minimized'] and 0<r['client'][0]<=1280 and 0<r['client'][1]<=800 for r in games)
print(json.dumps({'pid':a.pid,'launcherVisible':any(r['class']=='CyberpunkVRPortLauncherClass' and r['visible'] for r in rows),
    'smallGameWindow':small,'windows':rows},ensure_ascii=False))
