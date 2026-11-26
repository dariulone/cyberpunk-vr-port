import unittest
from unittest.mock import patch
import thread_stack_sampler as sampler
from symbol_session import symbol_label


class SymbolResolutionTests(unittest.TestCase):
    def test_pdb_span_is_half_open(self):
        self.assertEqual(symbol_label('worker', 0x1100, 32, 0x111f, 0x1000, 'pdb'), ('worker', True))
        text, bounded = symbol_label('worker', 0x1100, 32, 0x1120, 0x1000, 'pdb')
        self.assertFalse(bounded)
        self.assertIn('+0x20', text)
        self.assertIn('rva=0x120', text)

    def test_nearest_export_is_not_a_function_attribution(self):
        text, bounded = symbol_label('wscanf_s', 0x1100, 0, 0x9876, 0x1000, 'export', 0x200)
        self.assertFalse(bounded)
        self.assertIn('+0x8776', text)
        self.assertIn('nearest export', text)
        self.assertIn('rva=0x8876', text)

    def test_unknown_pdb_size_is_not_bounded(self):
        self.assertFalse(symbol_label('public', 0x1100, 0, 0x1101, 0x1000, 'pdb')[1])

    def test_sampler_preserves_nearest_displacement(self):
        class NativeResult:
            def SymFromAddrW(self, process, pc, displacement, symbol):
                value = symbol._obj
                value.Name, value.Address, value.ModBase, value.Size, value.Flags = 'wscanf_s', 0x1100, 0x1000, 0, 0x200
                displacement._obj.value = pc - value.Address
                return True
        state = {'modules': {0x1000: {'symbolType': 'export'}}, 'symbols': {}}
        with patch.object(sampler, 'd', NativeResult()), patch.object(sampler.initialize_symbols, 'state', state, create=True):
            labels = sampler.resolve_symbols(None, [0x9876])
        self.assertIn('+0x8776', labels[0x9876])
        self.assertFalse(state['symbols'][0x9876]['bounded'])
        self.assertEqual(state['symbols'][0x9876]['rva'], 0x8876)

    def test_mapped_pdb_mismatch_is_rejected(self):
        class NativeResult:
            def SymFromAddrW(self, process, pc, displacement, symbol):
                value = symbol._obj
                value.Name, value.Address, value.ModBase, value.Size = 'wrong_build', 0x1100, 0x1000, 128
                return True
        state = {'modules': {0x1000: {'symbolType': 'pdb', 'mappedIdentityVerified': False}}, 'symbols': {}}
        with patch.object(sampler, 'd', NativeResult()), patch.object(sampler.initialize_symbols, 'state', state, create=True):
            self.assertEqual(sampler.resolve_symbols(None, [0x1110]), {})
        self.assertIn('rejected', state['symbols'][0x1110])

    def test_export_with_zero_modbase_keeps_module_relative_rva(self):
        class NativeResult:
            def SymFromAddrW(self, process, pc, displacement, symbol):
                value = symbol._obj
                value.Name, value.Address, value.ModBase, value.Flags = 'Sleep', 0x1100, 0, 0x200
                displacement._obj.value = pc - value.Address
                return True
            def SymGetModuleBase64(self, process, pc):
                return 0x1000
        state = {'modules': {0x1000: {'symbolType': 'export'}}, 'symbols': {}}
        with patch.object(sampler, 'd', NativeResult()), patch.object(sampler.initialize_symbols, 'state', state, create=True):
            text = sampler.resolve_symbols(None, [0x1107])[0x1107]
        self.assertIn('rva=0x107', text)
        self.assertEqual(state['symbols'][0x1107]['moduleBase'], 0x1000)

    def test_raw_rva_and_return_address_boundary(self):
        result = sampler.symbolize([[0x1123, 0x2000]], [(0x1000, 0x2000, 'module.dll')], [])
        self.assertEqual(result, [['module.dll+123', 'module.dll+FFF']])

    def test_exact_named_lookup_rejects_replaced_disk_image(self):
        state = {'mappedImages': {0x1000: {'guid': 'old', 'age': 2, 'imageSize': 4096}}, 'modules': {}}
        with patch.object(sampler.initialize_symbols, 'state', state, create=True), \
             patch.object(sampler.modules, 'paths', {0x1000: 'port.dll'}, create=True), \
             patch.object(sampler, 'pe_identity', return_value={'guid': 'new', 'age': 3}):
            with self.assertRaisesRegex(RuntimeError, 'mapped DLL and disk image identity differ'):
                sampler.resolve_named_symbol(None, 'port.dll', 'Timing')

    def test_module_summary_does_not_group_by_symbol(self):
        result = sampler.aggregate([['port.dll!one'], ['port.dll!two+0x10 [nearest export; rva=0x20]']])
        self.assertEqual(result['exclusiveModules'], [('port.dll', 100.0)])


if __name__ == '__main__':
    unittest.main()
