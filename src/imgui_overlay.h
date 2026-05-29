#pragma once

#include <windows.h>
#include <dxgi1_6.h>
#include <d3d12.h>

void OverlaySetDeviceAndQueue(ID3D12Device* device, ID3D12CommandQueue* queue);
void OverlaySetWindow(HWND hwnd);
void OverlayRender(IDXGISwapChain* swapChain);
void OverlayInvalidateSwapchainResources();
bool OverlayIsVisible();
