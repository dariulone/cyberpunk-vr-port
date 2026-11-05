"""Read immutable descriptor evidence from the just-completed native draw capture."""
import argparse
import json
from pathlib import Path
import struct
import sys
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT/'tools/roomscale_tests'),str(ROOT/'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture',type=Path)
    parser.add_argument('--pid',type=int,required=True)
    args=parser.parse_args()
    capture=json.loads(args.capture.read_text())
    if capture['pid']!=args.pid or not capture.get('drawDataFile') or capture.get('drawsFull'):
        raise ValueError('Need a complete native draw reference from this PID')
    game=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols=resolve(args.pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',['CyberpunkVR_Descriptor*'])
    if symbols['sha256']!=capture['dll']:
        raise ValueError('Capture/module identity differs')
    reader=Reader(args.pid,game/'bin/x64/Cyberpunk2077.exe',0,0)
    addr=lambda name:int(symbols['CyberpunkVR_Descriptor'+name],16)
    read=lambda name:struct.unpack('<I',reader.read(addr(name),4))[0]
    blocks={}
    try:
        deadline=time.monotonic()+5
        while read('PendingLists'):
            if time.monotonic()>=deadline:raise TimeoutError('Captured command lists have not been submitted or discarded')
            time.sleep(.01)
        seq=read('CaptureSeq')
        if seq&1 or read('BindingBytes')!=64 or read('ValueBytes')!=160:
            raise RuntimeError('Busy or unsupported descriptor capture')
        failure=read('CaptureFailure')
        for name,unit,limit,count_name in [('Draws',16,4096,'DrawCount'),('Bindings',64,1048576,'BindingCount'),
                                         ('Ids',4,8*1048576,'IdCount'),('Values',160,1048576,'ValueCount')]:
            count=read(count_name)
            if count>limit:raise ValueError('Capture size exceeds bound')
            pointer,=struct.unpack('<Q',reader.read(addr(name),8))
            blocks[name]=reader.read(pointer,count*unit) if count else b''
        if read('CaptureSeq')!=seq:
            raise RuntimeError('Descriptor capture changed during read')
        count=len(blocks['Draws'])//16
        native=(args.capture.parent/capture['drawDataFile']).stat().st_size//1096
        if count!=native:raise ValueError(f'Descriptor/native draw mismatch: {count}/{native}')
        complete=0
        for i in range(count):
            sequence,offset,n,valid=struct.unpack_from('<4I',blocks['Draws'],i*16)
            if sequence!=i+1 or (offset+n)*64>len(blocks['Bindings']):raise ValueError('Bad descriptor sequence/range')
            complete+=valid==3
        stats=struct.unpack('<6Q',reader.read(addr('StoreStats'),48))
        root_count=read('RootCount')
        if root_count>128:raise ValueError('Invalid root schema count')
        root_data=reader.read(addr('Roots'),root_count*24) if root_count else b''
        roots=[dict(zip(('root','parameters','supported','ranges','reasons'),r)) for r in struct.iter_unpack('<Q4I',root_data)]
        result=dict(pid=args.pid,dll=symbols['sha256'],failure=failure,storeStats=dict(zip(('overflowBits','heaps','slots','values','intervals','largeCache'),stats)),draws=count,completeDraws=complete,
                    bindings=len(blocks['Bindings'])//64,ids=len(blocks['Ids'])//4,values=len(blocks['Values'])//160,roots=roots,files={})
        result['liveHeapSlots']=struct.unpack('<Q',reader.read(addr('HeapSlots'),8))[0]
        for name,data in blocks.items():
            path=args.capture.with_name(args.capture.stem+'-descriptors-'+name.lower()+'.bin')
            path.write_bytes(data);result['files'][name]=path.name
        args.capture.with_name(args.capture.stem+'-descriptors.json').write_text(json.dumps(result,indent=2)+'\n')
        print(json.dumps(result,indent=2))
        if failure or not count or not complete:raise RuntimeError('Incomplete descriptor tracking; do not treat it as binding proof')
    finally:
        reader.close()


if __name__=='__main__':main()
