#pragma once

#include <RED4ext/CNamePool.hpp>
#include <RED4ext/RTTISystem.hpp>

// An RTTI type by name. RTTI makes a composite type ("handle:entEntity", "raRef:CResource") on first request by
// parsing its name's text, which it finds in the name pool by the hash: a name nothing has put there yet is no type
// at all. So the name goes into the pool first.
inline RED4ext::rtti::IType* RttiType(const char* name) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    return rtti ? rtti->GetType(RED4ext::CNamePool::Add(name)) : nullptr;
}
