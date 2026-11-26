# -*- coding: utf-8 -*-
"""Read a RenderDoc capture WITHOUT replaying it.

WHY THIS EXISTS. Every replay-based route into these captures is unusable on this machine: opening a .rdc
through RenderDoc's ReplayController hangs the GPU (reproducible, 6-8 nvlddmkm events per attempt, and it
happens with the game closed, with Aftermath disabled and with 14.6 GB of VRAM free), and Nsight's own
replay server answers a hand-written client with a 24-byte error frame while refusing to run under an
instrumented launcher.

None of that is needed. RenderDoc has two independent layers and only one of them touches a GPU:

    ReplayController   recreates the frame on the device -- textures, pixel history, shader debugging
    CaptureFile        opens the FILE and hands over its structured data -- no device, no replay

The structured data is not a summary. For the capture this was written against it holds 108 164 chunks
and 17 795 raw buffers, including:

    Internal::Initial Contents              the recorded BYTES of a resource (id, length, buffer index)
    Internal::Coherent Mapped Memory Write  every mapped write the app made, with its byte range
    ID3D12Device::CreateConstantBufferView  BufferLocation + SizeInBytes + destination heap and index
    ID3D12GraphicsCommandList::SetMarker    MarkerText -- the port writes "VRCAM | <node>" / "MAIN | <node>"
    ResourceBarrier / DiscardResource / Dispatch / ExecuteIndirect ...

So constant-buffer contents, descriptor tables and per-view scoping are all available statically. What is
NOT available this way is anything that must be re-executed on a GPU: a rendered pixel, a shader's live
result, pixel history.

USAGE
    py -3.13 rdc_read.py <capture.rdc> chunks                      chunk histogram
    py -3.13 rdc_read.py <capture.rdc> markers [substr]            markers, with chunk indices
    py -3.13 rdc_read.py <capture.rdc> node "VRCAM | Composition"  every chunk inside that marker's region
    py -3.13 rdc_read.py <capture.rdc> cbv [--limit N]             constant-buffer views
    py -3.13 rdc_read.py <capture.rdc> writes [--limit N]          mapped writes, with the first bytes
    py -3.13 rdc_read.py <capture.rdc> contents <ResourceId>       dump a resource's recorded bytes
    py -3.13 rdc_read.py <capture.rdc> grep <regex> [--limit N]    chunks whose name matches

PYTHON VERSION MATTERS: renderdoc.pyd from this source build links python313.dll, so it must be run with
3.13 -- the 3.14 on PATH cannot load it.
"""

import os
import re
import sys

PYMODULES = r"C:\Users\dariulone\Desktop\renderdoc-src\x64\Release\pymodules"


def load_renderdoc():
    sys.path.insert(0, PYMODULES)
    os.add_dll_directory(PYMODULES)
    import renderdoc as rd
    return rd


def value(rd, ch):
    """One structured field as text. The accessors are typed, so the basetype decides which one works."""
    b = ch.type.basetype
    if b == rd.SDBasic.Buffer:
        return "buffer#%d" % ch.AsInt()
    if b == rd.SDBasic.String:
        return ch.AsString()
    if b == rd.SDBasic.Resource:
        return str(ch.AsResourceId())
    if b == rd.SDBasic.Float:
        return "%g" % ch.AsFloat()
    if b == rd.SDBasic.Boolean:
        return str(ch.AsBool())
    if b in (rd.SDBasic.UnsignedInteger, rd.SDBasic.SignedInteger, rd.SDBasic.Enum, rd.SDBasic.GPUAddress):
        return str(ch.AsInt())
    return ""


def find(rd, obj, name):
    """A direct child by name, or None. Cheaper and clearer than walking the tree by index everywhere."""
    for i in range(obj.NumChildren()):
        ch = obj.GetChild(i)
        if ch.name == name:
            return ch
    return None


def field_text(rd, chunk, path):
    """Follow a dotted path of child names and render the leaf."""
    node = chunk
    for part in path.split("."):
        node = find(rd, node, part)
        if node is None:
            return None
    return value(rd, node)


def open_capture(rd, path):
    cap = rd.OpenCaptureFile()
    st = cap.OpenFile(path, "", None)
    if st.code != rd.ResultCode.Succeeded:
        raise SystemExit("не удалось открыть капчур: %s" % st.code)
    return cap


def cmd_chunks(rd, sd, args):
    hist = {}
    for c in sd.chunks:
        hist[c.name] = hist.get(c.name, 0) + 1
    print("чанков %d, буферов %d" % (len(sd.chunks), len(sd.buffers)))
    for k, v in sorted(hist.items(), key=lambda kv: -kv[1])[:int(args.get("--limit", 40))]:
        print("  %-58s %d" % (k[:58], v))


def marker_list(rd, sd):
    """Every SetMarker/BeginEvent chunk as (index, text). The port names its nodes here, which is the only
    reliable way to scope anything to a view -- see identify-views-by-hash-only for why guessing fails."""
    out = []
    for idx, c in enumerate(sd.chunks):
        if "Marker" not in c.name and "BeginEvent" not in c.name:
            continue
        for i in range(c.NumChildren()):
            ch = c.GetChild(i)
            if ch.type.basetype == rd.SDBasic.String:
                out.append((idx, ch.AsString()))
                break
    return out


def cmd_markers(rd, sd, args):
    want = args.get("_arg")
    for idx, text in marker_list(rd, sd):
        if want and want.lower() not in text.lower():
            continue
        print("  chunk %-7d %s" % (idx, text))


def cmd_node(rd, sd, args):
    """Everything recorded between one marker and the next -- the static equivalent of "what does this node
    do", which is the question this project keeps asking."""
    want = args.get("_arg")
    if not want:
        raise SystemExit("нужен текст маркера")
    marks = marker_list(rd, sd)
    hit = next((i for i, (idx, t) in enumerate(marks) if want.lower() in t.lower()), None)
    if hit is None:
        raise SystemExit("маркер не найден")
    start = marks[hit][0]
    end = marks[hit + 1][0] if hit + 1 < len(marks) else len(sd.chunks)
    print("маркер %r: чанки %d..%d" % (marks[hit][1], start, end))
    hist = {}
    for c in sd.chunks[start:end]:
        hist[c.name] = hist.get(c.name, 0) + 1
    for k, v in sorted(hist.items(), key=lambda kv: -kv[1]):
        print("  %-58s %d" % (k[:58], v))


def cmd_cbv(rd, sd, args):
    limit = int(args.get("--limit", 20))
    n = 0
    for c in sd.chunks:
        if not c.name.endswith("CreateConstantBufferView"):
            continue
        loc = field_text(rd, c, "desc.Descriptor.BufferLocation")
        size = field_text(rd, c, "desc.Descriptor.SizeInBytes")
        heap = field_text(rd, c, "dst.heap")
        index = field_text(rd, c, "dst.index")
        print("  heap %-16s index %-8s size %-6s location %s" % (heap, index, size, loc))
        n += 1
        if n >= limit:
            break


def cmd_writes(rd, sd, args):
    limit = int(args.get("--limit", 20))
    n = 0
    for c in sd.chunks:
        if "Mapped Memory Write" not in c.name:
            continue
        res = field_text(rd, c, "Resource")
        beg = field_text(rd, c, "range.Begin")
        end = field_text(rd, c, "range.End")
        buf = find(rd, c, "MappedData")
        head = ""
        if buf is not None:
            data = bytes(sd.buffers[buf.AsInt()])
            head = data[:32].hex()
        print("  %-22s range %s..%s  %s" % (res, beg, end, head))
        n += 1
        if n >= limit:
            break


def cmd_contents(rd, sd, args):
    want = (args.get("_arg") or "").replace("ResourceId::", "")
    if not want:
        raise SystemExit("нужен ResourceId")
    for c in sd.chunks:
        if c.name != "Internal::Initial Contents":
            continue
        rid = field_text(rd, c, "id")
        if want not in (rid or ""):
            continue
        length = field_text(rd, c, "ContentsLength")
        buf = find(rd, c, "ResourceContents")
        print("  %s: %s байт" % (rid, length))
        if buf is not None:
            data = bytes(sd.buffers[buf.AsInt()])
            out = os.path.abspath("resource_%s.bin" % want)
            with open(out, "wb") as f:
                f.write(data)
            print("  первые 64 байта: %s" % data[:64].hex())
            print("  записано в %s (%d байт)" % (out, len(data)))
        return
    print("  ресурс не найден среди записанных содержимых")


def cmd_grep(rd, sd, args):
    pat = re.compile(args.get("_arg") or ".", re.I)
    limit = int(args.get("--limit", 30))
    n = 0
    for idx, c in enumerate(sd.chunks):
        if not pat.search(c.name):
            continue
        parts = []
        for i in range(min(c.NumChildren(), 4)):
            ch = c.GetChild(i)
            v = value(rd, ch)
            if v:
                parts.append("%s=%s" % (ch.name, v[:28]))
        print("  chunk %-7d %-46s %s" % (idx, c.name[:46], " ".join(parts)))
        n += 1
        if n >= limit:
            break



def _pso_blobs(rd, sd, chunk):
    """Every byte buffer under a PSO chunk, with the DXBC/DXIL magic if it has one. RenderDoc names these
    chunks CreateGraphicsPipeline / CreateComputePipeline (no "State"), and the shader blobs sit in
    pShaderBytecode fields -- one per stage, empty for the stages the PSO does not use."""
    out = []

    def walk(o, depth):
        for i in range(o.NumChildren()):
            ch = o.GetChild(i)
            if ch.type.basetype == rd.SDBasic.Buffer:
                data = bytes(sd.buffers[ch.AsInt()])
                out.append((ch.name, ch.AsInt(), data))
            if ch.NumChildren() and depth < 4:
                walk(ch, depth + 1)

    walk(chunk, 0)
    return out


def cmd_shaders(rd, sd, args):
    """List the pipeline states and the shader blobs each one carries."""
    limit = int(args.get("--limit", 20))
    n = 0
    for idx, c in enumerate(sd.chunks):
        if "CreatePipeline" not in c.name and "Pipeline" not in c.name:
            continue
        if "Create" not in c.name:
            continue
        blobs = [(nm, bi, d) for nm, bi, d in _pso_blobs(rd, sd, c) if d[:4] in (b"DXBC", b"DXIL")]
        if not blobs:
            continue
        kinds = " ".join("%s:%d" % (d[:4].decode(), len(d)) for _, _, d in blobs)
        print("  chunk %-7d %-42s %s" % (idx, c.name[:42], kinds))
        n += 1
        if n >= limit:
            break


def cmd_shader_dump(rd, sd, args):
    """Write every shader blob of one PSO chunk to disk, ready for a disassembler."""
    idx = int(args.get("_arg") or -1)
    if idx < 0 or idx >= len(sd.chunks):
        raise SystemExit("нужен индекс чанка (см. команду shaders)")
    c = sd.chunks[idx]
    wrote = 0
    for nm, bi, data in _pso_blobs(rd, sd, c):
        if data[:4] not in (b"DXBC", b"DXIL"):
            continue
        out = os.path.abspath("shader_%d_%d.%s" % (idx, bi, data[:4].decode().lower()))
        with open(out, "wb") as f:
            f.write(data)
        print("  %-20s %8d байт -> %s" % (nm[:20], len(data), out))
        wrote += 1
    if not wrote:
        print("  в этом чанке нет байткода")


COMMANDS = {
    "chunks": cmd_chunks, "markers": cmd_markers, "node": cmd_node,
    "cbv": cmd_cbv, "writes": cmd_writes, "contents": cmd_contents, "grep": cmd_grep,
    "shaders": cmd_shaders, "shader-dump": cmd_shader_dump,
}


def main(argv):
    if len(argv) < 3:
        print(__doc__)
        return 1
    path, command = argv[1], argv[2]
    args = {}
    rest = argv[3:]
    i = 0
    while i < len(rest):
        if rest[i].startswith("--") and i + 1 < len(rest):
            args[rest[i]] = rest[i + 1]
            i += 2
        else:
            args["_arg"] = rest[i]
            i += 1
    if command not in COMMANDS:
        raise SystemExit("команды: %s" % ", ".join(sorted(COMMANDS)))
    rd = load_renderdoc()
    cap = open_capture(rd, path)
    try:
        COMMANDS[command](rd, cap.GetStructuredData(), args)
    finally:
        cap.Shutdown()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
