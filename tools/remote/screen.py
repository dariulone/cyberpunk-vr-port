"""A screenshot of the whole virtual screen as a BMP (GDI, no packages): run in the desktop session it shows.

    py -3.13 -B tools/remote/screen.py OUT.bmp [--scale 2]
"""
import ctypes as c
import ctypes.wintypes as w
import struct
import sys


def main():
    out = sys.argv[1]
    scale = int(sys.argv[sys.argv.index('--scale') + 1]) if '--scale' in sys.argv else 2
    u, g = c.windll.user32, c.windll.gdi32
    u.SetProcessDPIAware()
    x, y = u.GetSystemMetrics(76), u.GetSystemMetrics(77)
    wd, ht = u.GetSystemMetrics(78), u.GetSystemMetrics(79)
    ow, oh = wd // scale, ht // scale
    screen = u.GetDC(None)
    mem = g.CreateCompatibleDC(screen)
    bmp = g.CreateCompatibleBitmap(screen, ow, oh)
    g.SelectObject(mem, bmp)
    g.SetStretchBltMode(mem, 4)   # HALFTONE
    g.StretchBlt(mem, 0, 0, ow, oh, screen, x, y, wd, ht, 0x00CC0020)

    class BITMAPINFOHEADER(c.Structure):
        _fields_ = [('biSize', w.DWORD), ('biWidth', w.LONG), ('biHeight', w.LONG), ('biPlanes', w.WORD),
                    ('biBitCount', w.WORD), ('biCompression', w.DWORD), ('biSizeImage', w.DWORD),
                    ('biXPelsPerMeter', w.LONG), ('biYPelsPerMeter', w.LONG), ('biClrUsed', w.DWORD),
                    ('biClrImportant', w.DWORD)]
    h = BITMAPINFOHEADER()
    h.biSize, h.biWidth, h.biHeight, h.biPlanes, h.biBitCount = c.sizeof(h), ow, oh, 1, 24
    stride = (ow * 3 + 3) & ~3
    buf = c.create_string_buffer(stride * oh)
    g.GetDIBits(mem, bmp, 0, oh, buf, c.byref(h), 0)
    with open(out, 'wb') as f:
        f.write(b'BM' + struct.pack('<IHHI', 14 + c.sizeof(h) + len(buf), 0, 0, 14 + c.sizeof(h)))
        f.write(bytes(h))
        f.write(buf.raw)
    g.DeleteObject(bmp)
    g.DeleteDC(mem)
    u.ReleaseDC(None, screen)
    print(out, ow, oh)


if __name__ == '__main__':
    main()
