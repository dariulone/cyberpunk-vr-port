# -*- coding: utf-8 -*-
"""rdc-static -- an MCP server that reads RenderDoc captures WITHOUT replaying them.

WHY IT EXISTS. On this machine every replay-based route into a capture is unusable. Opening a .rdc through
RenderDoc's ReplayController hangs the GPU: reproducible on two different captures, 6-8 `nvlddmkm` events
in the Windows System log per attempt, and it happens with the game closed, with Aftermath's global mode
disabled and with 14.6 of 16.3 GB of VRAM free. Nsight's own replay server is no better for automation --
it answers a hand-written client with a 24-byte error frame, and it fail-fasts (0xC0000409) when its
launcher is instrumented.

None of that is needed for the questions this project actually asks. RenderDoc has two independent layers
and only one of them touches a GPU:

    ReplayController   re-executes the frame on the device -- rendered pixels, pixel history, shader debug
    CaptureFile        opens the FILE and hands over its structured data -- no device, no replay, no risk

The structured data is not a summary: for one Cyberpunk capture it is 108 164 chunks and 17 795 raw
buffers, and it contains the things we keep needing --

    Internal::Initial Contents              the recorded BYTES of a resource (id, length, buffer)
    Internal::Coherent Mapped Memory Write  every mapped write with its byte range -- i.e. CONSTANTS
    ID3D12Device::CreateConstantBufferView  BufferLocation + SizeInBytes + destination heap and index
    CreateGraphicsPipeline / CreateComputePipeline   pShaderBytecode blobs, DXBC/DXIL
    ID3D12GraphicsCommandList::SetMarker    MarkerText -- this port writes "VRCAM | <node>" / "MAIN | <node>",
                                            which is what makes per-view scoping exact rather than guessed
    ResourceBarrier / DiscardResource / Dispatch / ExecuteIndirect / OMSetRenderTargets ...

WHAT THIS CANNOT DO, stated plainly so nobody chases it: anything that needs the frame re-executed. No
rendered pixel values, no pixel history, no shader stepping. Bytecode can be dumped and disassembled
offline; a shader's OUTPUT cannot be produced without a GPU.

PYTHON 3.13 IS REQUIRED: renderdoc.pyd from the local source build links python313.dll, so the 3.14 on
PATH cannot load it.

Every tool takes `capture` (a path). The opened file and its structured data are cached per path, because
opening a multi-gigabyte capture costs seconds and every question asks several tools in a row.
"""

import os
import re
import struct
import sys

from mcp.server.fastmcp import FastMCP

PYMODULES = os.environ.get(
    "RDC_PYMODULES", r"C:\Users\dariulone\Desktop\renderdoc-src\x64\Release\pymodules")

mcp = FastMCP("rdc-static")

_rd = None
_cache = {}          # path -> (CaptureFile, StructuredData, mtime)


def rd():
    """The renderdoc module, loaded once. Its DLL directory has to be registered explicitly on Windows."""
    global _rd
    if _rd is None:
        if PYMODULES not in sys.path:
            sys.path.insert(0, PYMODULES)
        os.add_dll_directory(PYMODULES)
        import renderdoc
        _rd = renderdoc
    return _rd


def capture(path):
    """Open (or reuse) a capture. Re-opens if the file changed on disk."""
    mod = rd()
    real = os.path.abspath(path)
    if not os.path.isfile(real):
        raise ValueError("нет такого файла: %s" % real)
    mtime = os.path.getmtime(real)
    hit = _cache.get(real)
    if hit and hit[2] == mtime:
        return hit[0], hit[1]
    cap = mod.OpenCaptureFile()
    st = cap.OpenFile(real, "", None)
    if st.code != mod.ResultCode.Succeeded:
        raise ValueError("не удалось открыть капчур: %s" % st.code)
    sd = cap.GetStructuredData()
    _cache[real] = (cap, sd, mtime)
    return cap, sd


# ---- structured-data helpers ---------------------------------------------------------------------

def field_value(ch):
    """One field as a plain Python value. The accessors are typed, so the basetype decides which works."""
    mod = rd()
    b = ch.type.basetype
    if b == mod.SDBasic.Buffer:
        return {"buffer": ch.AsInt(), "bytes": ch.type.byteSize}
    if b == mod.SDBasic.String:
        return ch.AsString()
    if b == mod.SDBasic.Resource:
        return str(ch.AsResourceId())
    if b == mod.SDBasic.Float:
        return ch.AsFloat()
    if b == mod.SDBasic.Boolean:
        return ch.AsBool()
    if b in (mod.SDBasic.UnsignedInteger, mod.SDBasic.SignedInteger,
             mod.SDBasic.Enum, mod.SDBasic.GPUAddress):
        return ch.AsInt()
    return None


def tree(obj, depth=0, max_depth=3):
    """A chunk's parameters as nested dicts, depth-limited so one call cannot dump a whole capture."""
    out = {}
    for i in range(obj.NumChildren()):
        ch = obj.GetChild(i)
        if ch.NumChildren() and depth < max_depth:
            out[ch.name] = tree(ch, depth + 1, max_depth)
        else:
            v = field_value(ch)
            out[ch.name] = v if v is not None else "<%s>" % ch.type.name
    return out


def child(obj, name):
    for i in range(obj.NumChildren()):
        ch = obj.GetChild(i)
        if ch.name == name:
            return ch
    return None


def path_value(chunk, dotted):
    node = chunk
    for part in dotted.split("."):
        node = child(node, part)
        if node is None:
            return None
    return field_value(node)


def markers(sd):
    """(chunk index, text) for every marker/begin-event chunk."""
    mod = rd()
    out = []
    for idx, c in enumerate(sd.chunks):
        if "Marker" not in c.name and "BeginEvent" not in c.name:
            continue
        for i in range(c.NumChildren()):
            ch = c.GetChild(i)
            if ch.type.basetype == mod.SDBasic.String:
                out.append((idx, ch.AsString()))
                break
    return out


def decode(data, fmt, count):
    """Bytes as the caller wants them. Constants are floats far more often than anything else, which is
    why 'floats' exists at all -- reading them as hex and converting by hand is where mistakes happen."""
    if fmt == "hex":
        return data[:count * 4 if count else len(data)].hex()
    if fmt == "floats":
        n = min(count or len(data) // 4, len(data) // 4)
        return list(struct.unpack_from("<%df" % n, data, 0))
    if fmt == "uints":
        n = min(count or len(data) // 4, len(data) // 4)
        return list(struct.unpack_from("<%dI" % n, data, 0))
    if fmt == "ascii":
        return data[:count or len(data)].decode("ascii", "replace")
    raise ValueError("fmt: hex | floats | uints | ascii")


# ---- tools ---------------------------------------------------------------------------------------

@mcp.tool()
def rdc_open(capture_path: str) -> dict:
    """Open a capture and report what it holds. Cached, so calling it again is free.

    Returns the driver, chunk and buffer counts, and whether the file claims local replay support (which
    this server never uses)."""
    cap, sd = capture(capture_path)
    return {
        "path": os.path.abspath(capture_path),
        "driver": cap.DriverName(),
        "chunks": len(sd.chunks),
        "buffers": len(sd.buffers),
        "local_replay_support": str(cap.LocalReplaySupport()),
        "note": "структурные данные, без реплея и без GPU",
    }


@mcp.tool()
def rdc_chunk_histogram(capture_path: str, limit: int = 40, pattern: str = "") -> dict:
    """How many chunks of each kind the capture has. The first call to make on an unfamiliar capture."""
    _, sd = capture(capture_path)
    rx = re.compile(pattern, re.I) if pattern else None
    hist = {}
    for c in sd.chunks:
        if rx and not rx.search(c.name):
            continue
        hist[c.name] = hist.get(c.name, 0) + 1
    top = sorted(hist.items(), key=lambda kv: -kv[1])[:limit]
    return {"total_kinds": len(hist), "chunks": [{"name": k, "count": v} for k, v in top]}


@mcp.tool()
def rdc_markers(capture_path: str, contains: str = "", limit: int = 200) -> dict:
    """Debug markers with their chunk indices.

    This port writes "VRCAM | <node>" and "MAIN | <node>", so these are the only exact way to say which
    view a piece of work belongs to."""
    _, sd = capture(capture_path)
    out = [{"chunk": i, "text": t} for i, t in markers(sd)
           if not contains or contains.lower() in t.lower()]
    return {"count": len(out), "markers": out[:limit]}


@mcp.tool()
def rdc_node(capture_path: str, marker: str, detail_limit: int = 0) -> dict:
    """Everything recorded between one marker and the next -- the static answer to "what does this node do".

    `marker` is matched case-insensitively as a substring, e.g. "VRCAM | CompositionPostProcess".
    With detail_limit > 0 the individual chunks are listed too, not just the histogram."""
    _, sd = capture(capture_path)
    marks = markers(sd)
    hit = next((i for i, (_, t) in enumerate(marks) if marker.lower() in t.lower()), None)
    if hit is None:
        raise ValueError("маркер не найден: %s" % marker)
    start = marks[hit][0]
    end = marks[hit + 1][0] if hit + 1 < len(marks) else len(sd.chunks)
    hist = {}
    for c in sd.chunks[start:end]:
        hist[c.name] = hist.get(c.name, 0) + 1
    res = {
        "marker": marks[hit][1],
        "chunk_range": [start, end],
        "calls": [{"name": k, "count": v} for k, v in sorted(hist.items(), key=lambda kv: -kv[1])],
    }
    if detail_limit:
        res["detail"] = [{"chunk": start + i, "name": c.name}
                         for i, c in enumerate(sd.chunks[start:end])][:detail_limit]
    return res


@mcp.tool()
def rdc_find_chunks(capture_path: str, pattern: str, start: int = 0, end: int = 0,
                    limit: int = 40) -> dict:
    """Chunks whose name matches a regex, optionally restricted to a chunk-index range (from rdc_node)."""
    _, sd = capture(capture_path)
    rx = re.compile(pattern, re.I)
    hi = end or len(sd.chunks)
    out = []
    for idx in range(start, min(hi, len(sd.chunks))):
        c = sd.chunks[idx]
        if not rx.search(c.name):
            continue
        head = {}
        for i in range(min(c.NumChildren(), 5)):
            ch = c.GetChild(i)
            if not ch.NumChildren():
                head[ch.name] = field_value(ch)
        out.append({"chunk": idx, "name": c.name, "params": head})
        if len(out) >= limit:
            break
    return {"count": len(out), "results": out}


@mcp.tool()
def rdc_chunk(capture_path: str, index: int, max_depth: int = 3) -> dict:
    """One chunk's full parameter tree -- resource ids, descs, buffer references, states."""
    _, sd = capture(capture_path)
    if index < 0 or index >= len(sd.chunks):
        raise ValueError("индекс вне диапазона 0..%d" % (len(sd.chunks) - 1))
    c = sd.chunks[index]
    return {"chunk": index, "name": c.name, "params": tree(c, 0, max_depth)}


@mcp.tool()
def rdc_constant_writes(capture_path: str, resource: str = "", fmt: str = "floats",
                        count: int = 16, limit: int = 25) -> dict:
    """Mapped memory writes -- this is where constant-buffer CONTENTS live.

    Each entry gives the resource, the byte range the app wrote, and the head of the data decoded as
    `fmt` (floats | uints | hex | ascii)."""
    _, sd = capture(capture_path)
    out = []
    for idx, c in enumerate(sd.chunks):
        if "Mapped Memory Write" not in c.name:
            continue
        res = path_value(c, "Resource")
        if resource and resource not in str(res):
            continue
        buf = child(c, "MappedData")
        data = bytes(sd.buffers[buf.AsInt()]) if buf is not None else b""
        out.append({
            "chunk": idx,
            "resource": res,
            "range": [path_value(c, "range.Begin"), path_value(c, "range.End")],
            "size": len(data),
            "head": decode(data, fmt, count),
        })
        if len(out) >= limit:
            break
    return {"count": len(out), "writes": out}


@mcp.tool()
def rdc_constant_buffer_views(capture_path: str, start: int = 0, end: int = 0,
                              limit: int = 40) -> dict:
    """Constant-buffer views: GPU address, size, and the descriptor heap slot they were written into.

    Restrict with start/end (a chunk range from rdc_node) to get only the views a given node created."""
    _, sd = capture(capture_path)
    hi = end or len(sd.chunks)
    out = []
    for idx in range(start, min(hi, len(sd.chunks))):
        c = sd.chunks[idx]
        if not c.name.endswith("CreateConstantBufferView"):
            continue
        out.append({
            "chunk": idx,
            "location": path_value(c, "desc.Descriptor.BufferLocation"),
            "size": path_value(c, "desc.Descriptor.SizeInBytes"),
            "heap": path_value(c, "dst.heap"),
            "index": path_value(c, "dst.index"),
        })
        if len(out) >= limit:
            break
    return {"count": len(out), "views": out}


@mcp.tool()
def rdc_resource_contents(capture_path: str, resource: str, offset: int = 0,
                          fmt: str = "hex", count: int = 32, save_to: str = "") -> dict:
    """The recorded bytes of a resource (Internal::Initial Contents).

    `resource` matches the ResourceId as a substring, e.g. "253858". With `save_to` the whole thing is
    written to that path -- useful for textures, which are far too large to return."""
    _, sd = capture(capture_path)
    want = resource.replace("ResourceId::", "")
    for idx, c in enumerate(sd.chunks):
        if c.name != "Internal::Initial Contents":
            continue
        rid = str(path_value(c, "id"))
        if want not in rid:
            continue
        buf = child(c, "ResourceContents")
        data = bytes(sd.buffers[buf.AsInt()]) if buf is not None else b""
        res = {
            "chunk": idx,
            "resource": rid,
            "declared_length": path_value(c, "ContentsLength"),
            "buffer_size": len(data),
            "offset": offset,
            "data": decode(data[offset:], fmt, count),
        }
        if save_to:
            with open(save_to, "wb") as f:
                f.write(data)
            res["saved_to"] = os.path.abspath(save_to)
        return res
    raise ValueError("среди записанных содержимых нет ресурса %s" % resource)


@mcp.tool()
def rdc_buffer(capture_path: str, index: int, offset: int = 0, fmt: str = "hex",
               count: int = 32) -> dict:
    """Any structured buffer by index -- the raw side of whatever a chunk referenced as `buffer#N`."""
    _, sd = capture(capture_path)
    if index < 0 or index >= len(sd.buffers):
        raise ValueError("индекс буфера вне диапазона 0..%d" % (len(sd.buffers) - 1))
    data = bytes(sd.buffers[index])
    return {"buffer": index, "size": len(data), "offset": offset,
            "data": decode(data[offset:], fmt, count)}


def _pso_blobs(sd, chunk):
    out = []

    def walk(o, depth):
        for i in range(o.NumChildren()):
            ch = o.GetChild(i)
            if ch.type.basetype == rd().SDBasic.Buffer:
                data = bytes(sd.buffers[ch.AsInt()])
                if data[:4] in (b"DXBC", b"DXIL"):
                    out.append((ch.name, ch.AsInt(), data))
            if ch.NumChildren() and depth < 4:
                walk(ch, depth + 1)

    walk(chunk, 0)
    return out


@mcp.tool()
def rdc_shaders(capture_path: str, start: int = 0, end: int = 0, limit: int = 30) -> dict:
    """Pipeline states and the shader bytecode blobs each one carries (DXBC/DXIL, with sizes).

    RenderDoc names these chunks CreateGraphicsPipeline / CreateComputePipeline -- without "State"."""
    _, sd = capture(capture_path)
    hi = end or len(sd.chunks)
    out = []
    for idx in range(start, min(hi, len(sd.chunks))):
        c = sd.chunks[idx]
        if "Pipeline" not in c.name or "Create" not in c.name:
            continue
        blobs = _pso_blobs(sd, c)
        if not blobs:
            continue
        out.append({
            "chunk": idx, "name": c.name,
            "blobs": [{"field": nm, "buffer": bi, "size": len(d), "magic": d[:4].decode()}
                      for nm, bi, d in blobs],
        })
        if len(out) >= limit:
            break
    return {"count": len(out), "pipelines": out}


@mcp.tool()
def rdc_shader_dump(capture_path: str, chunk: int, out_dir: str = "") -> dict:
    """Write one pipeline's shader blobs to disk, ready for an offline disassembler (dxc, D3DDisassemble).

    A GPU is not involved; this is the bytecode as the application supplied it."""
    _, sd = capture(capture_path)
    if chunk < 0 or chunk >= len(sd.chunks):
        raise ValueError("индекс вне диапазона")
    c = sd.chunks[chunk]
    target = out_dir or os.path.dirname(os.path.abspath(capture_path))
    os.makedirs(target, exist_ok=True)
    written = []
    for nm, bi, data in _pso_blobs(sd, c):
        p = os.path.join(target, "shader_%d_%d.%s" % (chunk, bi, data[:4].decode().lower()))
        with open(p, "wb") as f:
            f.write(data)
        written.append({"field": nm, "size": len(data), "path": p})
    if not written:
        raise ValueError("в чанке %d нет байткода" % chunk)
    return {"chunk": chunk, "name": c.name, "written": written}


@mcp.tool()
def rdc_resources(capture_path: str, pattern: str = "Create.*Resource", limit: int = 30,
                  min_width: int = 0, format_contains: str = "") -> dict:
    """Resource creations with their descriptions -- dimension, format, size, flags.

    Filters exist because a capture has thousands: `min_width` and `format_contains` narrow it to the
    full-size render targets and typeless pools this project keeps having to identify."""
    _, sd = capture(capture_path)
    rx = re.compile(pattern, re.I)
    out = []
    for idx, c in enumerate(sd.chunks):
        if not rx.search(c.name):
            continue
        desc = None
        for key in ("pDesc", "Descriptor", "desc"):
            node = child(c, key)
            if node is not None:
                desc = tree(node, 0, 2)
                break
        if desc is None:
            continue
        width = desc.get("Width")
        fmt = str(desc.get("Format", ""))
        if min_width and not (isinstance(width, int) and width >= min_width):
            continue
        if format_contains and format_contains.lower() not in fmt.lower():
            continue
        out.append({"chunk": idx, "name": c.name, "resource": path_value(c, "pResource"),
                    "desc": desc})
        if len(out) >= limit:
            break
    return {"count": len(out), "resources": out}


@mcp.tool()
def rdc_close(capture_path: str = "") -> dict:
    """Drop a cached capture (or all of them) and free its memory."""
    if capture_path:
        real = os.path.abspath(capture_path)
        hit = _cache.pop(real, None)
        if hit:
            hit[0].Shutdown()
        return {"closed": [real] if hit else []}
    closed = list(_cache)
    for _, (cap, _sd, _m) in _cache.items():
        cap.Shutdown()
    _cache.clear()
    return {"closed": closed}


if __name__ == "__main__":
    mcp.run()
