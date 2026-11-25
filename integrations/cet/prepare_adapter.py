"""Prepare a reviewable CET renderer patch without changing the CET checkout."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError(f'CET source changed; expected one integration point: {old[:90]!r}')
    return text.replace(old, new, 1)


def prepare(source, output):
    source, output = source.resolve(), output.resolve()
    if not output.is_relative_to(ROOT / 'build'):
        raise ValueError('Output must be inside this VR checkout\'s build directory')
    if output.exists():
        raise ValueError('Choose a new output directory; existing patches are not overwritten')
    originals = {}
    patched = {}
    for name in ('src/d3d12/D3D12.h', 'src/d3d12/D3D12_Functions.cpp', 'src/d3d12/D3D12.cpp'):
        originals[name] = (source / name).read_text(encoding='utf-8-sig')
    header = originals['src/d3d12/D3D12.h']
    header = replace_once(header, '#include "window/Window.h"', '#include "window/Window.h"\n#include "vr/CyberpunkVrAdapter.h"')
    header = replace_once(header, '    std::recursive_mutex m_imguiLock;', '    CyberpunkVrAdapter m_vrOverlay;\n    std::recursive_mutex m_imguiLock;')
    header = replace_once(header, 'SIZE GetResolution() const noexcept { return m_outSize; }',
                          'SIZE GetResolution() const noexcept { return m_vrOverlay.Resolution(m_outSize); }')
    patched['src/d3d12/D3D12.h'] = header
    window = originals['src/d3d12/D3D12.cpp']
    window = replace_once(window, '        if (d3d12.m_trapInputInImGui) // TODO: look into io.WantCaptureMouse and io.WantCaptureKeyboard',
                          '        if (d3d12.m_vrOverlay.PassThrough(auMsg, awParam)) return 0;\n\n'
                          '        if (d3d12.m_trapInputInImGui) // TODO: look into io.WantCaptureMouse and io.WantCaptureKeyboard')
    patched['src/d3d12/D3D12.cpp'] = window
    code = originals['src/d3d12/D3D12_Functions.cpp']
    code = replace_once(code, 'bool D3D12::ResetState(const bool acDestroyContext)\n{',
                        'bool D3D12::ResetState(const bool acDestroyContext)\n{\n'
                        '    std::lock_guard vrGuard(m_imguiLock);\n    if (!m_vrOverlay.Drain()) return false;')
    code = replace_once(code, 'void D3D12::Update()\n{',
                        'void D3D12::Update()\n{\n'
                        '    // Keep backend/context access serialized with PrepareUpdate and reset.\n'
                        '    std::lock_guard vrGuard(m_imguiLock);')
    code = replace_once(code, '    if (m_initialized)\n    {\n        std::lock_guard _(m_imguiLock);',
                        '    if (m_initialized)\n    {\n        std::lock_guard _(m_imguiLock);\n        m_vrOverlay.Hide();')
    code = replace_once(code, '    ImGui_ImplWin32_NewFrame(m_outSize);\n    ImGui::NewFrame();',
                        '    ImGui_ImplWin32_NewFrame(m_vrOverlay.Resolution(m_outSize));\n'
                        '    if (m_vrOverlay.Prepare() && CET::Get().GetOverlay().IsEnabled())\n'
                        '        CET::Get().GetOverlay().Toggle();\n'
                        '    m_vrOverlay.Scale(m_outSize, m_styleReference);\n    ImGui::NewFrame();')
    code = replace_once(code, '    CET::Get().GetVM().Draw();\n\n    ImGui::Render();',
                        '    CET::Get().GetVM().Draw();\n'
                        '    m_vrOverlay.Finish(CET::Get().GetOverlay().IsEnabled());\n\n    ImGui::Render();')
    code = replace_once(code, '    assert(m_pdxgiSwapChain);\n    auto& frameContext',
                        '    if (m_vrOverlay.Render(&m_imguiDrawDataBuffers[0], m_pd3d12Device.Get(),\n'
                        '            m_pCommandQueue.Get(), DXGI_FORMAT_R8G8B8A8_UNORM, m_pd3dSrvDescHeap.Get(),\n'
                        '            [&](ID3D12GraphicsCommandList* list) {\n'
                        '                ImGui_ImplDX12_RenderDrawData(&m_imguiDrawDataBuffers[0], list);\n'
                        '            })) return;\n'
                        '    if (m_imguiDrawDataBuffers[0].DisplaySize.x != float(m_outSize.cx) ||\n'
                        '        m_imguiDrawDataBuffers[0].DisplaySize.y != float(m_outSize.cy)) return;\n\n'
                        '    assert(m_pdxgiSwapChain);\n    auto& frameContext')
    patched['src/d3d12/D3D12_Functions.cpp'] = code
    for name in ('ModUiApi.h', 'ModUiImGui.hpp', 'ModUiProducer.hpp'):
        patched['src/vr/' + name] = (ROOT / 'include/Overlay' / name).read_text(encoding='utf-8')
    patched['src/vr/CyberpunkVrAdapter.h'] = (ROOT / 'integrations/cet/CyberpunkVrAdapter.h').read_text(encoding='utf-8')
    patch = []
    for name, text in patched.items():
        old = originals.get(name, '')
        patch.extend(difflib.unified_diff(old.splitlines(keepends=True), text.splitlines(keepends=True),
                                        fromfile='a/' + name if name in originals else '/dev/null', tofile='b/' + name))
    output.mkdir(parents=True)
    for name, text in patched.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding='utf-8', newline='\n')
    (output / 'cet-vr-overlay.patch').write_text(''.join(patch), encoding='utf-8', newline='\n')
    manifest = {name: hashlib.sha256((source / name).read_bytes()).hexdigest() for name in originals}
    (output / 'source-hashes.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    print(f'Prepared {len(patched)} files and cet-vr-overlay.patch in {output}; CET source unchanged.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.source, args.output)
