#pragma once

namespace cvr::hooks {

enum class FarHookResult { Ok, Unsupported, Busy, Failed };

// An inline hook for a function with no free memory within reach of a 5-byte jump, which MinHook needs for its relay
// (MH_ERROR_MEMORY_ALLOC). The target's first instructions are replaced by a 14-byte absolute jump (FF 25) and moved
// into a trampoline that can live anywhere. Only for a prologue whose first 14 bytes hold no relative branch, no
// RIP-relative operand and no return or indirect jump (checked: Unsupported otherwise). The other threads are
// suspended while the jump is written; Busy when one of them is inside the bytes being replaced (try again later).
// Never removed.
FarHookResult FarInlineHook(void* target, void* detour, void** original);
const char* ToString(FarHookResult result);

}  // namespace cvr::hooks
