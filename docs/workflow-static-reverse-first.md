---
name: workflow-static-reverse-first
description: "User preference (2026-06-09) — prefer STATIC IDA reversing (byte-decode + xref) to nail engine hook points BEFORE writing/building code; live-test/runtime-probe is a LAST resort, not the first move. Minimize build+deploy+in-game round-trips."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

User directive (2026-06-09): "live-test в последнюю очередь" — do the static IDA reverse-engineering FIRST to pin down the exact engine function/hook point, and only fall back to runtime probes / in-game testing when static analysis is exhausted.

**Why:** in-game build→deploy→test cycles cost the user time/attention; they'd rather I grind the static reverse (even though it's slower via MCP byte-decode) and arrive at the build with the hook point already nailed.

**How to apply:** when a task needs an engine hook point, exhaust IDA (xrefs, byte-decode, vtable walks — NOT decompile, see [[ida-no-decompile-rule]]) to identify + verify the exact address/prologue/condition BEFORE proposing a build. Present the runtime-probe option only after static analysis can't resolve it. Contrast the earlier RTT phase which was probe-heavy; the Sync-Sequential phase ([[cp2077-sync-sequential-feasibility]]) should be static-first.
