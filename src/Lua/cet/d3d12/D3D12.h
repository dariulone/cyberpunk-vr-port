#pragma once

// CET's D3D12 (src/d3d12/D3D12.h) as far as the scripting core asks: the output resolution (GetDisplayResolution).
// The host sets it from the game's window.
struct D3D12
{
    [[nodiscard]] SIZE GetResolution() const noexcept { return m_outSize; }
    void SetResolution(SIZE aSize) noexcept { m_outSize = aSize; }

private:
    SIZE m_outSize{};
};
