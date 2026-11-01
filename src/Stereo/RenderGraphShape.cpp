// RenderGraphShape -- research probe: the shape of each render graph the engine runs.
//
// sub_1403E8F58(owner, graph, _, args, done) runs a view's compiled render graph: one RunRenderJob per
// node (sub_14032D60C), each job's prerequisites wired from the graph's edges (sub_1408EA1C8). A node
// record is 48 bytes at graph+24 (listed through the DWORD indices at graph+40, count graph+16): +0 its
// job index, +16 the node. An edge record is 24 bytes at graph+72 (indices graph+88, count graph+64):
// +4 nonzero = not a dependency, +8 / +12 the records of the node before and the node after.
//
// With the game started with CVR_GRAPH_SHAPE=1, every distinct graph run while CyberpunkVR_GraphShapeGen
// is nonzero is recorded once per generation into CyberpunkVR_GraphShapes: its node and edge counts, the
// longest chain of dependent nodes, the widest level, each node's work function and the edges themselves
// (tools/hitch_tests/graph_shape.py reads them).

#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"
#include <windows.h>
#include <atomic>
#include <cstdint>

namespace {

constexpr uint32_t kSlots = 8, kMaxNodes = 512, kMaxEdges = 2048;

}  // namespace

extern "C" {
// Nonzero = record; a new value records every graph again.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GraphShapeGen{0};
struct CvrGraphShape {
    uint64_t graph;
    uint64_t runs;
    uint32_t gen, nodes, edges, depth, width, reserved;
    uint32_t work[kMaxNodes];          // node work function RVA, by job index
    uint16_t edge[kMaxEdges][2];       // job index before, job index after
};
__declspec(dllexport) CvrGraphShape CyberpunkVR_GraphShapes[kSlots]{};
}

namespace cvr {
namespace detail {
namespace {

using GraphRunFn = int64_t (*)(int64_t*, uint8_t*, int64_t, int64_t, int64_t);
GraphRunFn g_orig_graph_run = nullptr;
constexpr uintptr_t GRAPH_RUN_RVA = 0x3E8F58;
SRWLOCK g_lock = SRWLOCK_INIT;

bool Wanted() { return GetEnvironmentVariableW(L"CVR_GRAPH_SHAPE", nullptr, 0) != 0; }

void Record(CvrGraphShape& s, const uint8_t* graph, uint32_t gen) {
    const uint32_t n = *reinterpret_cast<const uint32_t*>(graph + 16);
    const uint8_t* records = *reinterpret_cast<uint8_t* const*>(graph + 24);
    const uint32_t* list = *reinterpret_cast<uint32_t* const*>(graph + 40);
    const uint32_t e = *reinterpret_cast<const uint32_t*>(graph + 64);
    const uint8_t* edges = *reinterpret_cast<uint8_t* const*>(graph + 72);
    const uint32_t* elist = *reinterpret_cast<uint32_t* const*>(graph + 88);
    s.graph = reinterpret_cast<uint64_t>(graph);
    s.gen = gen;
    s.nodes = n;
    const uint32_t nn = n < kMaxNodes ? n : kMaxNodes;
    for (uint32_t i = 0; i < nn; ++i) {
        const uint8_t* rec = records + 48ull * (list[i] & 0xDFFFFFFFu);
        const uint32_t job = *reinterpret_cast<const uint32_t*>(rec);
        const uintptr_t node = *reinterpret_cast<const uintptr_t*>(rec + 16);
        uintptr_t work = 0;
        if (node) {
            const uintptr_t vtable = *reinterpret_cast<const uintptr_t*>(node);
            if (vtable) work = *reinterpret_cast<const uintptr_t*>(vtable + 8);
        }
        const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
        if (job < kMaxNodes) s.work[job] = work > base ? static_cast<uint32_t>(work - base) : 0;
    }
    uint32_t kept = 0;
    static thread_local uint16_t indeg[kMaxNodes], level[kMaxNodes], queue[kMaxNodes], count[kMaxNodes];
    for (uint32_t i = 0; i < nn; ++i) indeg[i] = level[i] = count[i] = 0;
    for (uint32_t i = 0; i < e && kept < kMaxEdges; ++i) {
        const uint8_t* rec = edges + 24ull * (elist[i] & 0xBFFFFFFFu);
        if (*reinterpret_cast<const uint32_t*>(rec + 4)) continue;
        const uint32_t a = *reinterpret_cast<const uint32_t*>(records + 48ull * (*reinterpret_cast<const uint32_t*>(rec + 8) & 0xDFFFFFFFu));
        const uint32_t b = *reinterpret_cast<const uint32_t*>(records + 48ull * (*reinterpret_cast<const uint32_t*>(rec + 12) & 0xDFFFFFFFu));
        if (a >= nn || b >= nn) continue;
        s.edge[kept][0] = static_cast<uint16_t>(a);
        s.edge[kept][1] = static_cast<uint16_t>(b);
        ++indeg[b];
        ++kept;
    }
    s.edges = kept;
    // Longest chain (in nodes) and widest level, Kahn's order over the kept edges.
    uint32_t head = 0, tail = 0, depth = 0;
    for (uint32_t i = 0; i < nn; ++i)
        if (!indeg[i]) { queue[tail++] = static_cast<uint16_t>(i); level[i] = 1; }
    while (head < tail) {
        const uint16_t v = queue[head++];
        if (level[v] > depth) depth = level[v];
        ++count[level[v] - 1];
        for (uint32_t k = 0; k < kept; ++k) {
            if (s.edge[k][0] != v) continue;
            const uint16_t w = s.edge[k][1];
            if (level[w] < level[v] + 1) level[w] = static_cast<uint16_t>(level[v] + 1);
            if (--indeg[w] == 0) queue[tail++] = w;
        }
    }
    uint32_t width = 0;
    for (uint32_t i = 0; i < depth && i < kMaxNodes; ++i)
        if (count[i] > width) width = count[i];
    s.depth = depth;
    s.width = width;
}

int64_t Detour_GraphRun(int64_t* owner, uint8_t* graph, int64_t a3, int64_t args, int64_t done) {
    const uint32_t gen = CyberpunkVR_GraphShapeGen.load(std::memory_order_relaxed);
    if (gen && graph) {
        AcquireSRWLockExclusive(&g_lock);
        CvrGraphShape* slot = nullptr;
        for (auto& s : CyberpunkVR_GraphShapes) {
            if (s.graph == reinterpret_cast<uint64_t>(graph)) { slot = &s; break; }
        }
        if (!slot) {
            for (auto& s : CyberpunkVR_GraphShapes) {
                if (!s.graph || s.gen != gen) { slot = &s; s.graph = 0; s.runs = 0; break; }
            }
        }
        if (slot) {
            ++slot->runs;
            if (slot->gen != gen || !slot->graph) {
                __try { Record(*slot, graph, gen); } __except (EXCEPTION_EXECUTE_HANDLER) { slot->nodes = 0xFFFFFFFFu; slot->gen = gen; slot->graph = reinterpret_cast<uint64_t>(graph); }
            }
        }
        ReleaseSRWLockExclusive(&g_lock);
    }
    return g_orig_graph_run(owner, graph, a3, args, done);
}

}  // namespace

CVR_DETOUR_IF("[graph-shape] render graph run sub_1403E8F58", GRAPH_RUN_RVA, Detour_GraphRun, g_orig_graph_run, Wanted)

}  // namespace detail
}  // namespace cvr
