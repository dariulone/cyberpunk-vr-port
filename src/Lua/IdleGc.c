/* IdleGc.c -- the Lua host's collector stepped from outside the VM (src/Lua/HostGc.cpp), on LuaJIT's own headers so
   the state's layout is the library's. Built with the library's options (src/Lua/CMakeLists.txt). */
#include "lua.h"
#include "lj_obj.h"
#include "lj_gc.h"

#include "IdleGc.h"

int cvr_lj_gc_state(lua_State *L)
{
  return G(L)->gc.state;
}

int cvr_lj_vm_busy(lua_State *L)
{
  /* A call in progress on the main state has a C frame chain; a coroutine runs inside such a call, and a trace
     inside the VM. */
  return L->cframe != NULL || tvref(G(L)->jit_base) != NULL;
}

uint64_t cvr_lj_gc_total(lua_State *L)
{
  return (uint64_t)G(L)->gc.total;
}

uint32_t cvr_lj_gc_stepmul(lua_State *L)
{
  return (uint32_t)G(L)->gc.stepmul;
}

int cvr_lj_gc_slice(lua_State *L, int finalize, int newcycle, int (*until)(void *), void *ud)
{
  global_State *g = G(L);
  GCSize threshold = g->gc.threshold, debt = g->gc.debt;
  MSize stepmul = g->gc.stepmul;
  int steps = 0;
  /* lj_gc_step works off (GCSTEPSIZE/100) * stepmul per call: at 1 that is one gc_onestep (a phase change that
     costs nothing runs on into the next one), so the phase is checked before every step. A sweep step that ends
     the sweep leaves the state at GCSfinalize without finalizing anything. */
  g->gc.stepmul = 1;
  for (;;) {
    int state = g->gc.state;
    if (state == GCSfinalize && !finalize) break;
    if (state == GCSpause && (!newcycle || steps > 0)) break;
    if (until(ud)) break;
    lj_gc_step(L);
    steps++;
  }
  /* lj_gc_step re-arms the collector's own trigger; the host keeps it stopped. */
  g->gc.stepmul = stepmul;
  g->gc.threshold = threshold;
  g->gc.debt = debt;
  return steps;
}
