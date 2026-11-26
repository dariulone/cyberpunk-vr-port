"""Inspect the game or simulator client, or save its window image via PrintWindow."""
import argparse
import json
import ctypes as c
from ctypes import wintypes as w
from pathlib import Path
from PIL import Image


class BitmapHeader(c.Structure):
    _fields_ = [('size', w.DWORD), ('width', w.LONG), ('height', w.LONG),
                ('planes', w.WORD), ('bits', w.WORD), ('compression', w.DWORD),
                ('image_size', w.DWORD), ('xppm', w.LONG), ('yppm', w.LONG),
                ('used', w.DWORD), ('important', w.DWORD)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--out', type=Path)
    action.add_argument('--inspect', action='store_true', help='print physical client bounds without capturing')
    parser.add_argument('--window',choices=('simulator','game'),default='simulator')
    args = parser.parse_args()
    if args.out and (not args.out.parent.is_dir() or args.out.exists()):
        raise RuntimeError('Fresh output under existing parent required')
    user, gdi = c.WinDLL('user32', use_last_error=True), c.WinDLL('gdi32', use_last_error=True)
    # PrintWindow draws physical pixels. Prevent DPI-virtualized GetWindowRect
    # from allocating a smaller bitmap and cropping the right eye.
    user.SetThreadDpiAwarenessContext.argtypes = [w.HANDLE]
    user.SetThreadDpiAwarenessContext.restype = w.HANDLE
    user.SetThreadDpiAwarenessContext(c.c_void_p(-4))
    user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
    user.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
    user.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    user.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
    user.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
    user.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
    user.IsIconic.argtypes = [w.HWND]
    user.IsWindowVisible.argtypes = [w.HWND]
    user.GetForegroundWindow.restype = w.HWND
    user.GetDC.argtypes = [w.HWND]; user.GetDC.restype = w.HDC
    user.ReleaseDC.argtypes = [w.HWND, w.HDC]
    user.PrintWindow.argtypes = [w.HWND, w.HDC, w.UINT]
    gdi.CreateCompatibleDC.argtypes = [w.HDC]; gdi.CreateCompatibleDC.restype = w.HDC
    gdi.CreateDIBSection.argtypes = [w.HDC, c.POINTER(BitmapHeader), w.UINT, c.POINTER(c.c_void_p), w.HANDLE, w.DWORD]
    gdi.CreateDIBSection.restype = w.HBITMAP
    gdi.SelectObject.argtypes = [w.HDC, w.HANDLE]; gdi.SelectObject.restype = w.HANDLE
    gdi.DeleteObject.argtypes = [w.HANDLE]
    gdi.DeleteDC.argtypes = [w.HDC]
    callback = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.EnumWindows.argtypes = [callback, w.LPARAM]
    windows = []

    @callback
    def collect(hwnd, _):
        pid = w.DWORD(); user.GetWindowThreadProcessId(hwnd, c.byref(pid))
        if pid.value == args.pid and user.IsWindowVisible(hwnd):
            title = c.create_unicode_buffer(1024); user.GetWindowTextW(hwnd, title, len(title))
            cls = c.create_unicode_buffer(256); user.GetClassNameW(hwnd, cls, len(cls))
            windows.append((int(hwnd), title.value, cls.value))
        return True

    user.EnumWindows(collect, 0)
    matches = [(hwnd, title, cls) for hwnd, title, cls in windows if
               ('simulator' in title.lower() if args.window == 'simulator' else cls == 'W2ViewportClass')]
    if len(matches) != 1:
        raise RuntimeError(f'Need exactly one visible {args.window} window for PID {args.pid}: {windows}')
    hwnd, title, cls = matches[0]
    if args.inspect:
        client, origin = w.RECT(), w.POINT()
        if not user.GetClientRect(hwnd, c.byref(client)) or not user.ClientToScreen(hwnd, c.byref(origin)):
            raise c.WinError(c.get_last_error())
        print(json.dumps({'pid': args.pid, 'hwnd': hex(hwnd), 'class': cls, 'title': title,
                          'minimized': bool(user.IsIconic(hwnd)), 'client': [client.right, client.bottom],
                          'clientScreenRect': [origin.x, origin.y, origin.x + client.right, origin.y + client.bottom]}))
        return
    before_focus = user.GetForegroundWindow()
    rect = w.RECT()
    if not user.GetWindowRect(hwnd, c.byref(rect)):
        raise c.WinError(c.get_last_error())
    width, height = rect.right-rect.left, rect.bottom-rect.top
    if not 100 <= width <= 10000 or not 100 <= height <= 10000:
        raise RuntimeError('Unexpected preview dimensions')
    dc = user.GetDC(hwnd)
    target = gdi.CreateCompatibleDC(dc)
    pixels = c.c_void_p()
    header = BitmapHeader(c.sizeof(BitmapHeader), width, -height, 1, 32, 0, width*height*4, 0, 0, 0, 0)
    bitmap = gdi.CreateDIBSection(dc, c.byref(header), 0, c.byref(pixels), None, 0)
    if not dc or not target or not bitmap or not pixels.value:
        raise c.WinError(c.get_last_error())
    old = gdi.SelectObject(target, bitmap)
    try:
        if not user.PrintWindow(hwnd, target, 2):
            raise c.WinError(c.get_last_error())
        image = Image.frombytes('RGB', (width, height), c.string_at(pixels, width*height*4), 'raw', 'BGRX')
        image.save(args.out)
        print({'pid': args.pid, 'hwnd': hex(hwnd), 'title': title, 'size': image.size,
               'extrema': image.getextrema(), 'focus_unchanged': user.GetForegroundWindow() == before_focus,
               'output': str(args.out)})
    finally:
        gdi.SelectObject(target, old); gdi.DeleteObject(bitmap); gdi.DeleteDC(target); user.ReleaseDC(hwnd, dc)


if __name__ == '__main__':
    main()
