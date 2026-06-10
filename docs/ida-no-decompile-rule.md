---
name: ida-no-decompile-rule
description: HARD RULE — never call decompile_function or disassemble_function via ida-pro-mcp on the CP2077 IDB; they crash IDA. Use read_memory_bytes raw-byte decode + get_xrefs_to/data reads only.
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

User instruction (2026-06-09): do NOT call `decompile_function` or `disassemble_function` through ida-pro-mcp on the Cyberpunk2077.exe database — IDA crashes (observed repeatedly, even on small functions; earlier notes said "hostile functions only" but the user now forbids it entirely).

**Why:** crashes lose the IDA session and stall the whole reverse loop.

**How to apply:** decode functions by reading raw bytes (`read_memory_bytes`, x86-64 manual decode), navigate via `get_xrefs_to`, `get_callers`, `data_read_qword`, `list_globals_filter`. MCP calls may also time out at 1s — re-check with `check_connection`, don't assume crash. Related: [[cp2077-rtt-view-create-fn]] (same lesson originally).
