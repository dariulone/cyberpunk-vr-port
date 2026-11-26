#include "Hooks/OriginalMethodTable.hpp"
#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <string>
#include <unordered_map>

extern bool PatchVtableMethod(void**, size_t, void*);
extern uintptr_t MakeVtableSlotKey(void**, size_t);
extern std::unordered_map<uintptr_t, void*> g_originalVtableMethods;
extern cvr::hooks::OriginalMethodTable<> g_originalMethodTable;
using Call = void(*)();
static int nativeCalls{}, portCalls{}, overlayCalls{};
static Call portNext{}, overlayNext{};
static void Native() { ++nativeCalls; }
static void Port() { ++portCalls; portNext(); }
static void Overlay() { ++overlayCalls; overlayNext(); }
static void Check(bool ok, const char* message) { if (!ok) throw std::runtime_error(message); }

int main(int argc, char** argv) try {
    Check(argc == 2, "case required");
    const std::string name = argv[1];
    void* table[1]{reinterpret_cast<void*>(&Native)};
    const auto key = MakeVtableSlotKey(table, 0);
    if (name == "missing_original") {
        table[0] = reinterpret_cast<void*>(&Port);
        Check(!PatchVtableMethod(table, 0, reinterpret_cast<void*>(&Port)), "accepted own hook without continuation");
        Check(g_originalVtableMethods.empty(), "published recursive original");
        return 0;
    }
    if (name == "overlay_before") {
        overlayNext = reinterpret_cast<Call>(table[0]);
        table[0] = reinterpret_cast<void*>(&Overlay);
    }
    Check(PatchVtableMethod(table, 0, reinterpret_cast<void*>(&Port)), "initial install failed");
    portNext = reinterpret_cast<Call>(g_originalVtableMethods.at(key));
    Check(g_originalMethodTable.Find(key) == reinterpret_cast<void*>(portNext), "fast original differs from map");
    if (name == "overlay_after") {
        overlayNext = reinterpret_cast<Call>(table[0]);
        table[0] = reinterpret_cast<void*>(&Overlay);
    }
    const auto entry = table[0];
    reinterpret_cast<Call>(table[0])();
    for (int i=0; i!=100; ++i)
        Check(PatchVtableMethod(table, 0, reinterpret_cast<void*>(&Port)), "repeated install failed");
    Check(table[0] == entry, "repeated registration replaced a later overlay");
    Check(g_originalVtableMethods.at(key) == reinterpret_cast<void*>(portNext), "continuation changed into a cycle");
    reinterpret_cast<Call>(table[0])();
    Check(nativeCalls == 2 && portCalls == 2, "native/port did not run exactly once per call");
    Check(overlayCalls == (name == "duplicate" ? 0 : 2), "overlay dropped or called more than once");
    std::cout << "PASS " << name << ": native=" << nativeCalls << " port=" << portCalls << " overlay=" << overlayCalls << '\n';
    return 0;
} catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
