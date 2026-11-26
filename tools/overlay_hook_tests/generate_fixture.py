"""Compile the actual production vtable installer without the renderer's unrelated dependencies."""
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[2]
source = (root / 'src/Hooks/DepthCapture.cpp').read_text(encoding='utf-8')
start = source.index('bool PatchVtableMethod(')
end = source.index('\nstatic void RecordEclQueue', start)
preamble = '''#include <windows.h>
#include <cstdint>
#include <mutex>
#include <unordered_map>
#include "Hooks/OriginalMethodTable.hpp"
std::mutex g_vtableMutex;
std::unordered_map<uintptr_t, void*> g_originalVtableMethods;
cvr::hooks::OriginalMethodTable<> g_originalMethodTable;
uintptr_t MakeVtableSlotKey(void** table, size_t slot) {
    return reinterpret_cast<uintptr_t>(table) ^ (static_cast<uintptr_t>(slot) * 0x9E3779B97F4A7C15ull);
}
'''
Path(sys.argv[1]).write_text(preamble + source[start:end], encoding='utf-8')
