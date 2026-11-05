#pragma once

#include "common/Meta.h"
#include "Utils.h"

// 64-bit integers reach Lua as LuaJIT cdata (ll / ull). Every one returned to Lua was made by compiling and running a
// chunk ("return 123ull": the lexer, the parser, a prototype for the collector), and entity hashes and item ids come
// back on most game calls the mods make each frame. The builder below is compiled once per Lua state, kept in its
// registry under this address, and builds the value from its two 32-bit halves: hi * 2^32 + lo, exact in 64 bits.
namespace LuaRedInt64
{
inline bool PushBuilder(lua_State* apL, bool aSigned)
{
    static char s_keys[2];
    void* key = &s_keys[aSigned ? 1 : 0];
    lua_pushlightuserdata(apL, key);
    lua_rawget(apL, LUA_REGISTRYINDEX);
    if (lua_isfunction(apL, -1))
        return true;
    lua_pop(apL, 1);
    const char* code = aSigned ? "return function(h, l) return h * 4294967296LL + l end"
                               : "return function(h, l) return h * 4294967296ULL + l end";
    if (luaL_loadstring(apL, code) != 0 || lua_pcall(apL, 0, 1, 0) != 0)
    {
        lua_pop(apL, 1);
        return false;
    }
    lua_pushlightuserdata(apL, key);
    lua_pushvalue(apL, -2);
    lua_rawset(apL, LUA_REGISTRYINDEX);
    return true;
}

// The value as a cdata on top of the stack; false (stack unchanged) if the builder could not be made or run.
inline bool Push(lua_State* apL, uint64_t aBits, bool aSigned)
{
    const int top = lua_gettop(apL);
    if (!PushBuilder(apL, aSigned))
        return false;
    const double high = aSigned ? double(static_cast<int32_t>(aBits >> 32)) : double(static_cast<uint32_t>(aBits >> 32));
    lua_pushnumber(apL, high);
    lua_pushnumber(apL, double(static_cast<uint32_t>(aBits)));
    if (lua_pcall(apL, 2, 1, 0) != 0)
    {
        lua_settop(apL, top);
        return false;
    }
    return true;
}
} // namespace LuaRedInt64

template <class T, FixedString REDName> struct LuaRED
{
    static constexpr char const* Name = REDName;

    sol::object ToLua(RED4ext::CStackType& aResult, TiltedPhoques::Locked<sol::state, std::recursive_mutex>& aLua)
    {
        if constexpr (std::is_integral_v<T> && sizeof(T) == sizeof(uint64_t))
        {
            lua_State* L = aLua.Get().lua_state();
            if (LuaRedInt64::Push(L, static_cast<uint64_t>(*static_cast<T*>(aResult.value)), std::is_signed_v<T>))
            {
                sol::object result(L, -1);
                lua_pop(L, 1);
                return result;
            }
            constexpr auto format{std::is_signed_v<T> ? "return {}ll" : "return {}ull"};
            auto res{aLua.Get().script(fmt::format(format, *static_cast<T*>(aResult.value)))};
            assert(res.valid());
            return res.get<sol::object>();
        }
        else
        {
            return make_object(aLua.Get(), *static_cast<T*>(aResult.value));
        }
    }

    RED4ext::CStackType ToRED(sol::object aObject, RED4ext::rtti::IType*, TiltedPhoques::Allocator* apAllocator)
    {
        RED4ext::CStackType result;
        result.type = m_pRtti;
        if (aObject == sol::nil)
        {
            result.value = apAllocator->New<T>();
        }
        else if constexpr (std::is_integral_v<T> && sizeof(T) == sizeof(uint64_t))
        {
            if (aObject.get_type() == sol::type::number)
            {
                // Already a number: what tonumber would give back, without a global lookup and a call.
                const double value = aObject.as<double>();
                result.value = apAllocator->New<T>(static_cast<T>(value));
            }
            else if (IsLuaCData(aObject))
            {
                sol::state_view v(aObject.lua_state());
                std::string str = v["tostring"](aObject);
                if constexpr (std::is_signed_v<T>)
                    result.value = apAllocator->New<T>(std::stoll(str));
                else
                    result.value = apAllocator->New<T>(std::stoull(str));
            }
        }
        else if constexpr (std::is_same_v<T, bool>)
        {
            if (aObject.get_type() == sol::type::boolean)
                result.value = apAllocator->New<T>(aObject.as<T>());
        }
        else if constexpr (std::is_arithmetic_v<T>)
        {
            if (aObject.get_type() == sol::type::number)
            {
                if (aObject.is<T>())
                {
                    result.value = apAllocator->New<T>(aObject.as<T>());
                }
            }
            else if (IsLuaCData(aObject))
            {
                sol::state_view v(aObject.lua_state());
                double value = v["tonumber"](aObject);
                result.value = apAllocator->New<T>(static_cast<T>(value));
            }
        }
        else if (aObject.is<T>())
        {
            result.value = apAllocator->New<T>(aObject.as<T>());
        }

        return result;
    }

    size_t Size() const noexcept { return sizeof(T); }

    bool Is(RED4ext::rtti::IType* apRtti) const
    {
        if (!Resolve())
            return false;

        return apRtti == m_pRtti;
    }

protected:
    bool Resolve() const
    {
        if (m_pRtti)
            return true;

        auto* pRtti = RED4ext::CRTTISystem::Get();
        m_pRtti = pRtti->GetType(RED4ext::FNV1a64(Name));

        return m_pRtti != nullptr;
    }

    mutable RED4ext::rtti::IType* m_pRtti{nullptr};
};
