import struct
import unittest
import native_textures as probe


class NativeTextureTests(unittest.TestCase):
    def test_entry_layout(self):
        raw = bytearray(probe.STRIDE)
        struct.pack_into('<i4xQ', raw, 0, 3, 0xABCDEF1234)
        struct.pack_into('<IHHHBBH', raw, 0x50, 0xC0, 0, 768, 768, 1, 0, 0)
        struct.pack_into('<HHHBBH', raw, 0x56, 768, 768, 1, 4, 5, 3)
        row = probe.decode_entry(raw, 9)
        self.assertEqual((row['handle'], row['nativeRefs'], row['resource']), (9, 3, '0xabcdef1234'))
        self.assertEqual((row['width'], row['height'], row['depth'], row['nativeFormat']), (768, 768, 1, 5))
        self.assertEqual(row['nativeState'], 0xC0)
        with self.assertRaises(ValueError):
            probe.decode_entry(raw[:24], 9)

    def test_owner_and_native_ref_are_distinct(self):
        base, pool, owner, listing, pointers, backing = 0x10000000, 0x20000000, 0x30000000, 0x40000000, 0x50000000, 0x60000000
        memory = {}
        def put(address, data):
            memory[address] = data
        put(base + probe.POOL_RVA, struct.pack('<Q', pool))
        put(base + probe.OWNER_RVA, struct.pack('<Q', owner))
        put(owner + 0xD0, struct.pack('<Q', listing))
        put(listing, struct.pack('<QQII', 0, pointers, 1, 1))
        put(pointers, struct.pack('<Q', backing))
        put(backing, struct.pack('<Qi12xI4x', base + probe.BACKING_VTABLE, 2, 1))
        entries = bytearray(probe.STRIDE * 2)
        for index, refs in enumerate((1, 0)):
            struct.pack_into('<i4xQ', entries, index * probe.STRIDE, refs, 0x70000000 + index)
            struct.pack_into('<HHHBBH', entries, index * probe.STRIDE + 0x56, 768, 768, 1, 4, 5, 3)
        put(pool + probe.ENTRY_START, bytes(entries))
        def read(address, size):
            for start, data in memory.items():
                if start <= address and address + size <= start + len(data):
                    return data[address - start:address - start + size]
            raise OSError('not mapped')
        result = probe.snapshot(read, base, capacity=2)
        self.assertTrue(result['poolStable'] and result['backingListStable'])
        self.assertEqual(len(result['rows']), 2)
        self.assertEqual(result['rows'][0]['backings'][0]['refs'], 2)
        self.assertEqual(result['rows'][1]['nativeRefs'], 0)
        self.assertEqual(result['rows'][1]['backings'], [])
        self.assertTrue(all(row['stable'] for row in result['rows']))

    def test_ref_resource_and_shape_changes_invalidate_identity(self):
        before = bytes(probe.STRIDE)
        for offset in (0, 8, 0x50, 0x56, 0x5D):
            after = bytearray(before)
            after[offset] = 1
            self.assertNotEqual(probe.entry_identity(before), probe.entry_identity(after))

    def test_mapped_export_is_exact_and_rejects_forwarders(self):
        base = 0x10000000; image = bytearray(0x4000)
        image[:2] = b'MZ';struct.pack_into('<I', image, 0x3C, 0x80)
        image[0x80:0x84] = b'PE\0\0';struct.pack_into('<H', image, 0x80 + 24, 0x20B)
        struct.pack_into('<I', image, 0x80 + 80, len(image));struct.pack_into('<II', image, 0x80 + 136, 0x1000, 0x400)
        struct.pack_into('<IIIII', image, 0x1000 + 20, 1, 1, 0x1100, 0x1120, 0x1140)
        struct.pack_into('<I', image, 0x1100, 0x2000);struct.pack_into('<I', image, 0x1120, 0x1180)
        name = b'CyberpunkVR_HudLayerAddress\0';image[0x1180:0x1180 + len(name)] = name
        read = lambda address, size: bytes(image[address-base:address-base+size])
        self.assertEqual(probe.mapped_export(read, base, name[:-1].decode()), base + 0x2000)
        with self.assertRaises(ValueError):probe.mapped_export(read, base, 'Missing')
        struct.pack_into('<I', image, 0x1100, 0x1180)
        with self.assertRaisesRegex(ValueError, 'forwarded'):probe.mapped_export(read, base, name[:-1].decode())

    def test_hud_ownership_correlates_exact_slot_and_window(self):
        game = 0x140000000; memory = bytearray(0x8000)
        struct.pack_into('<QQ', memory, 0x1000, 0x2000, 0x4000)
        struct.pack_into('<QII', memory, 0x2000 + 0x150, 0x3000, 1, 1)
        struct.pack_into('<Q', memory, 0x3000, 0x1234)
        memory[0x3000 + 8] = memory[0x3000 + 0xA0] = 1
        struct.pack_into('<Q', memory, 0x3000 + 0xB8, 0x5000)
        struct.pack_into('<Q', memory, 0x3000 + 0xC8, 0x4000)
        struct.pack_into('<Q', memory, 0x4000 + 0x4E8, 0x5000)
        struct.pack_into('<Q', memory, 0x5000, game + 0x3064700)
        struct.pack_into('<II', memory, 0x5000 + 0x40, 1024, 1024)
        struct.pack_into('<QQ', memory, 0x5000 + 0x70, 0x6000, 0x6000)
        struct.pack_into('<Qi', memory, 0x6000, game + probe.BACKING_VTABLE, 2)
        struct.pack_into('<I', memory, 0x6000 + 24, 7)
        read = lambda address, size: bytes(memory[address:address+size])
        report = probe.hud_snapshot(read, game, 0x1000)
        self.assertTrue(report['stable']);self.assertEqual(report['count'], 1)
        row = report['rows'][0]
        self.assertTrue(row['stable']);self.assertEqual(row['slotTexture']['address'], row['windowTexture']['address'])
        self.assertEqual([x['handle'] for x in row['slotTexture']['backings']], [7, 7])
        self.assertEqual(row['slotTexture']['width'], 1024)
        struct.pack_into('<QII', memory, 0x2000 + 0x150, 0x3000, 4097, 4097)
        with self.assertRaisesRegex(ValueError, 'array is invalid'):probe.hud_snapshot(read, game, 0x1000)

    def test_layer_enumeration_follows_only_verified_manager_layout(self):
        memory = bytearray(0x10000); game = 0x140000000
        struct.pack_into('<QII', memory, 0x1000 + 0x380, 0x2000, 2, 2)
        struct.pack_into('<QQQQ', memory, 0x2000, 0x3000, 0, 0xDEADBEEF, 0)
        struct.pack_into('<QII', memory, 0x3000 + 0x38, 0x4000, 1, 1)
        struct.pack_into('<QQ', memory, 0x4000, 0x5000, 0)
        struct.pack_into('<Q', memory, 0x5000, game + 0x123456)
        struct.pack_into('<Q', memory, 0x5000 + 0x40, 0x6000)
        struct.pack_into('<Q', memory, 0x6000, game + 0x654321)
        def read(address, size):
            if address == game + 0x3C2B390:return struct.pack('<Q', 0x1000)
            if address == game + 0x123456:return struct.pack('<Q', 0)
            if address + size > len(memory):raise OSError('unmapped')
            return bytes(memory[address:address+size])
        report = probe.layers_snapshot(read, game)
        self.assertTrue(report['stable']);self.assertEqual(report['managerCount'], 2)
        self.assertEqual(report['rows'][0]['address'], '0x5000')
        self.assertIsNone(report['rows'][0]['texture'])
        self.assertEqual(report['rows'][1]['scope'], 'uninterpreted secondary manager')
        self.assertEqual(report['errors'], [])

    def test_icon_atlas_typed_strong_chain_and_expired_weak_entry(self):
        game=0x140000000; memory=bytearray(0x10000)
        def put(address,fmt,*values):struct.pack_into(fmt,memory,address,*values)
        put(0x1000+0x250,'<Q',0x2000);put(0x2000,'<Q',game+0x30B0730)
        put(0x2010,'<QII',0x2800,2,2);put(0x2020,'<QQ',0x3000,0x3800)
        put(0x2800,'<QQQQ',0x3000,0x3800,0xDEAD0000,0x3900)
        put(0x3000,'<QQQ',game+0x30B05E8,0x3000,0x3800);put(0x3800,'<II',1,2)
        put(0x3158,'<QII',0x4000,1,1)
        put(0x4000+0x50,'<QQ',0x5000,0x5800)
        put(0x5000,'<QQQ',game+0x30B0498,0x5000,0x5800);put(0x5800,'<II',1,1)
        put(0x5068,'<QQ',0x6000,0x6800)
        put(0x6000,'<QQQ',game+0x3064700,0x6000,0x6800);put(0x6800,'<II',4,3)
        put(0x6040,'<II',768,768);put(0x6070,'<QQ',0x7000,0x7000)
        put(0x7000,'<Qi',game+probe.BACKING_VTABLE,2);put(0x7018,'<I',9)
        def read(address,size):
            if address==game+0x3C2B390:return struct.pack('<Q',0x1000)
            if address+size>len(memory):raise OSError('expired target must not be followed')
            return bytes(memory[address:address+size])
        report=probe.icon_atlases_snapshot(read,game)
        self.assertTrue(report['stable']);self.assertFalse(report['atlases'][2]['alive'])
        self.assertEqual(report['uniqueAtlasCount'],1);self.assertEqual(report['uniquePageCount'],1)
        texture=report['atlases'][0]['pages'][0]['texture']
        self.assertEqual(texture['width'],768);self.assertEqual([b['handle'] for b in texture['backings']],[9,9])
        self.assertTrue(report['atlases'][0]['pages'][0]['pageHandleStable'])
        put(0x5000,'<Q',game+0x123456)
        report=probe.icon_atlases_snapshot(read,game)
        self.assertFalse(report['atlases'][0]['pages'][0]['textureAtlas']['alive'])
        self.assertNotIn('texture',report['atlases'][0]['pages'][0])


if __name__ == '__main__':
    unittest.main()
