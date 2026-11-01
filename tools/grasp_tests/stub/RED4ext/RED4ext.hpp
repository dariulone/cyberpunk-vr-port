#pragma once
#include <cstdint>
#include <cstring>
namespace RED4ext {
struct IScriptable {};
struct CStackFrame { char* code; };
template <typename T> void GetParameter(CStackFrame*, T* v) { *v = T{}; }
}
