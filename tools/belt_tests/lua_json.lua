-- luajit tools/belt_tests/lua_json.lua input.lua output.json
-- Writes a generated data module (a table of numbers, strings and tables) as JSON for the C++ tools.
local input, output = arg[1], arg[2]
local data = dofile(input)

local function isArray(t)
    local n = #t
    if n == 0 then return false end
    for k in pairs(t) do
        if type(k) ~= "number" or k < 1 or k > n or k % 1 ~= 0 then return false end
    end
    return true
end

local out = {}
local function write(v)
    local kind = type(v)
    if kind == "number" then
        out[#out + 1] = string.format("%.10g", v)
    elseif kind == "string" then
        out[#out + 1] = string.format("%q", v)
    elseif kind == "boolean" then
        out[#out + 1] = tostring(v)
    elseif kind == "table" then
        if isArray(v) then
            out[#out + 1] = "["
            for i = 1, #v do
                if i > 1 then out[#out + 1] = "," end
                write(v[i])
            end
            out[#out + 1] = "]"
        else
            local keys = {}
            for k in pairs(v) do keys[#keys + 1] = k end
            table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
            out[#out + 1] = "{"
            for i, k in ipairs(keys) do
                if i > 1 then out[#out + 1] = "," end
                out[#out + 1] = string.format("%q:", tostring(k))
                write(v[k])
            end
            out[#out + 1] = "}"
        end
    else
        out[#out + 1] = "null"
    end
end
write(data)
local f = assert(io.open(output, "w"))
f:write(table.concat(out))
f:close()
