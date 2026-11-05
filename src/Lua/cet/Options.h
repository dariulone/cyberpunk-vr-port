#pragma once

// The CET options the ported scripting core reads (src/Options.h), at the values the port's mods run with.
struct DeveloperSettings
{
    bool RemoveDeadBindings{false};
    bool EnableJIT{true};
};

struct Options
{
    DeveloperSettings Developer{};
};
