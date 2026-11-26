"""Verify static visual-profile data for one identified Footlong build; does not execute the APK.

Usage: py -3.13 -I verify_footlong_visual_profile.py path/to/local.apk
The documented build-specific addresses are NOT addresses in the user's game process.
"""
import argparse
import hashlib
import json
import struct
import zipfile
from pathlib import Path

LIB_SHA = 'd211b0aea75a67b9a10854452c76f9c06d5a667abfad9b5caf3a1e3ccfdd0e4d'
META_SHA = '36b0500dbd4451f27540c2ffd4ee197076a76aea682763b9b19945763fc6fa4b'


def verify(apk):
    with zipfile.ZipFile(apk) as package:
        def verified_entry(path, limit, digest):
            if package.getinfo(path).file_size > limit:
                raise ValueError(f'{path}: unexpected size')
            data = package.read(path)
            if hashlib.sha256(data).hexdigest() != digest:
                raise ValueError(f'{path}: not the identified build; do not reuse these addresses')
            return data
        so = verified_entry('lib/arm64-v8a/libil2cpp.so', 220_000_000, LIB_SHA)
        meta = verified_entry('assets/bin/Data/Managed/Metadata/global-metadata.dat', 32_000_000, META_SHA)

    # RecoilProfile is a value type. The metadata field order is the packed 4-float order.
    assert struct.unpack_from('<II', meta) == (0xfab11baf,39)
    tables = list(struct.iter_unpack('<III', meta[8:380]))
    def u32(at): return struct.unpack_from('<I',meta,at)[0]
    def string(i):
        at=tables[2][0]+i
        return meta[at:meta.index(0,at)].decode('utf-8')
    def fields(index):
        at=tables[19][0]+82*index
        first=u32(at+26)
        count=struct.unpack_from('<H',meta,at+62)[0]
        return [string(u32(tables[11][0]+12*j)) for j in range(first,first+count)]
    image = next(tables[20][0]+36*i for i in range(tables[20][2]) if string(u32(tables[20][0]+36*i))=='Footlong.dll')
    first,count=struct.unpack_from('<HH',meta,image+8)
    types={string(u32(tables[19][0]+82*i)):i for i in range(first,first+count)}
    names=fields(types['RecoilProfile'])
    assert names==['LinearForce','LinearSpringiness','AngularForce','AngularSpringiness']
    presets=fields(types['Recoil'])[:3]
    assert presets==['profileOneHanded','profileTwoHanded','profileStocked']

    # Addresses of the three literals in the static constructor, verified against this ELF.
    header=struct.unpack_from('<Q',so,32)[0]
    stride,count=struct.unpack_from('<HH',so,54)
    segments=[struct.unpack_from('<IIQQQQQQ',so,header+i*stride) for i in range(count)]
    def read(va):
        segment=next(s for s in segments if s[0]==1 and s[3]<=va and va+16<=s[3]+s[5])
        return dict(zip(names,struct.unpack_from('<4f',so,segment[2]+va-segment[3])))
    return {k:read(va) for k,va in zip(presets,(0x19f6d60,0x19f8500,0x19f9880))}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('apk',type=Path)
    print(json.dumps(verify(parser.parse_args().apk),indent=2))
