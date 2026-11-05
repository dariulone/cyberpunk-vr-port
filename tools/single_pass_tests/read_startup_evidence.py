"""Read state after a rejected HUD gate, without changing gameplay or readiness."""
import argparse
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--out', type=Path, required=True)
    a = p.parse_args()
    game = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols = resolve(a.pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
                      ['CyberpunkVR_MenuTransition*','CyberpunkVR_MenuSource*','CyberpunkVR_MenuMethods*','g_menuModeValue','CyberpunkVR_ConstantRegistered*'])
    reader = Reader(a.pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    read = lambda name: struct.unpack('<I', reader.read(int(symbols[name], 16), 4))[0]
    try:
        seq = read('CyberpunkVR_MenuTransitionSeq')
        count = read('CyberpunkVR_MenuTransitionCount')
        if seq & 1 or count > 64:
            raise RuntimeError('Busy/invalid menu transition diagnostic')
        data = reader.read(int(symbols['CyberpunkVR_MenuTransitions'], 16), count * 32) if count else b''
        events = []
        for row in struct.iter_unpack('<QQiiiI', data):
            event = dict(zip(('tick', 'state', 'previous', 'requested', 'nativeBefore', 'thread'), row))
            try:
                event['nativeNow'] = struct.unpack('<I', reader.read(event['state'] + 0x128, 4))[0]
                event['nativeCounter'] = struct.unpack('<I', reader.read(event['state'] + 0x120, 4))[0]
                event['nativeOverride'] = reader.read(event['state'] + 0x264, 1)[0]
            except OSError:
                event['nativeNow'] = None
            event['state'] = hex(event['state'])
            events.append(event)
        if read('CyberpunkVR_MenuTransitionSeq') != seq:
            raise RuntimeError('Menu diagnostics changed while reading')
        status = game / 'bin/x64/plugins/cyber_engine_tweaks/mods/CyberpunkVRPort_QuickBoot/quickboot_status.json'
        report = dict(pid=a.pid, menu=read('g_menuModeValue'), transitions=events, script=json.loads(status.read_text()),
                      registeredTypes=list(struct.unpack('<6I', reader.read(int(symbols['CyberpunkVR_ConstantRegisteredTypes'], 16), 24))),
                      registeredRejects=read('CyberpunkVR_ConstantRegisteredRejects'))
        source_seq=read('CyberpunkVR_MenuSourceSeq')
        source=reader.read(int(symbols['CyberpunkVR_MenuSource'],16),168)
        if source_seq and not source_seq&1 and source_seq==read('CyberpunkVR_MenuSourceSeq'):
            tick,obj,type_rva,getter=struct.unpack_from('<QQII',source)
            report['nativeMenuSource']=dict(tick=tick,object=hex(obj),typeRva=hex(type_rva),getterRva=hex(getter),
                                           prefix=list(struct.unpack_from('<32I',source,24)))
            owner,owner_type,mode=struct.unpack_from('<QIi',source,152)
            report['nativeMenuSource'].update(owner=hex(owner),ownerTypeRva=hex(owner_type),mode=mode)
        length=read('CyberpunkVR_MenuMethodsLength')
        if length and length<32768:
            methods=reader.read(int(symbols['CyberpunkVR_MenuMethods'],16),length).decode('utf-8')
            a.out.with_suffix('.methods.txt').write_text(methods,encoding='utf-8')
        a.out.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
