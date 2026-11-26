import struct
import unittest
from dynamic_origins import decode_rows, LAYOUT


def row(raw, ordinal, serial):
    at = (ordinal-1) % 1024 * 344
    struct.pack_into('<6Q', raw, at, ordinal, 987654, 0x1234, 0x2345, serial, 0x1405EA8A4)
    struct.pack_into('<II4BIQ', raw, at+48, 768, 768, 0, 1, 6, 0x11, 0, 0)
    struct.pack_into('<4I', raw, at+72, 99, 2, 15, 0)
    struct.pack_into('<2Q', raw, at+88, 0x140A901E8, 0x1402735E3)


class OriginDecodeTests(unittest.TestCase):
    def test_exact_abi_and_raw_pcs(self):
        raw=bytearray(1024*344);row(raw,1,51)
        data=decode_rows(raw,1)['records'][0]
        self.assertEqual(data['descriptor']['flags'],0x11)
        self.assertEqual((data['objectSerial'],data['thread']), (51,99))
        self.assertEqual(data['stack'],['0x140a901e8','0x1402735e3'])

    def test_overwritten_interval_is_explicit(self):
        raw=bytearray(1024*344)
        for ordinal in range(1,1101):row(raw,ordinal,ordinal+7)
        result=decode_rows(raw,1100)
        self.assertEqual(result['overwrittenSinceRequested'],76)
        self.assertEqual(len(result['records']),1024)
        self.assertEqual(result['records'][0]['ordinal'],77)
        self.assertEqual(len(decode_rows(raw,1100,since=1098)['records']),2)

    def test_torn_unknown_or_reset_publication_is_rejected(self):
        raw=bytearray(1024*344);row(raw,1,1)
        with self.assertRaises(ValueError):decode_rows(raw,2)
        with self.assertRaises(ValueError):decode_rows(raw,1,since=2)
        with self.assertRaises(ValueError):decode_rows(raw[:-1],1)
        with self.assertRaises(ValueError):decode_rows(raw,1,(2,)+LAYOUT[1:])


if __name__ == '__main__':
    unittest.main()
