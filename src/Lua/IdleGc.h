#pragma once

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct lua_State;

// The collector's phase (LuaJIT's GCSpause 0, propagate 1, atomic 2, sweepstring 3, sweep 4, finalize 5).
int cvr_lj_gc_state(struct lua_State* L);
// Nonzero while the VM runs anything on this (main) state.
int cvr_lj_vm_busy(struct lua_State* L);
// The bytes the state has allocated, and its step multiplier: for checking this file's view of the layout against
// lua_gc's.
uint64_t cvr_lj_gc_total(struct lua_State* L);
uint32_t cvr_lj_gc_stepmul(struct lua_State* L);
// Single collector steps until `until(ud)` says stop, a cycle ends (or, without `newcycle`, the collector is
// paused), or, without `finalize`, the finalizer phase is reached: that phase runs the __gc of userdata, which
// for the host's game values calls into the game. The collector's trigger, debt and step multiplier are left as
// they were. Returns the steps taken.
int cvr_lj_gc_slice(struct lua_State* L, int finalize, int newcycle, int (*until)(void*), void* ud);

#ifdef __cplusplus
}
#endif
