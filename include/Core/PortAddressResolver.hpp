#pragma once
// THE PORT'S ADDRESS RESOLVER for the C++ half (force-included into every C++ file of the Rust build's DLL,
// CMakeLists.txt): the RED4ext SDK resolves an engine address hash through RED4ext.dll's RED4ext_ResolveAddress unless
// a resolver override is declared -- this is that override. It asks the Rust half (red4ext-abi reloc.rs), which uses
// RED4ext's resolver when RED4ext runs in the process and otherwise the address library the game ships beside its
// executable (bin\x64\cyberpunk2077_addresses.json), so the port runs from its own loader without RED4ext installed.
// The whole DLL asks one resolver: the Lua host's copy of CET's override (src/Lua/cet/reverse/Relocation.h) steps
// aside. No Windows header here: it is included before every file's own includes.
#define CVR_PORT_ADDRESS_RESOLVER 1

#include <RED4ext/Detail/Memory.hpp>

#include <cstdint>
#include <type_traits>

// The address, or -- like the SDK's own resolver with a hash it cannot find -- a message and the process ended.
extern "C" uintptr_t cvr_resolve_address_or_exit(uint32_t aHash);

template<>
struct RED4ext::Detail::AddressResolverOverride<uint32_t> : std::true_type
{
    inline static uintptr_t Resolve(uint32_t aHash)
    {
        return cvr_resolve_address_or_exit(aHash);
    }
};
