"""Read one coherent owned native-retirement telemetry publication."""
import argparse
import json
from pathlib import Path
import struct
import sys
import time

ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT/'tools/swimming_tests'),str(ROOT/'tools/roomscale_tests')]


def snapshot(read32,read64,read_words,*,timeout=3,monotonic=time.monotonic,sleep=time.sleep):
    deadline=monotonic()+timeout
    while monotonic()<deadline:
        sequence=read32('Seq')
        if sequence&1:sleep(.01);continue
        result={name:list(read_words(name,count)) for name,count in
                (('Allocator',24),('RetirementTotals',16),('Retirement',8),('Cache',16))}
        event_sequence=read64('RetirementEventSeq');event_count=read64('RetirementEventCount')
        events=list(read_words('RetirementEvents',32*24));cache_rows=list(read_words('CacheRows',64*8))
        if sequence!=read32('Seq'):sleep(.01);continue
        if event_count>32 or len(events)!=768 or len(cache_rows)!=512:
            raise ValueError('Native retirement publication exceeds its bounded ABI')
        result['events']=dict(sequence=event_sequence,count=event_count,
            rows=[events[index:index+24] for index in range(0,len(events),24)])
        result['cacheRows']=[cache_rows[index:index+8] for index in range(0,len(cache_rows),8)]
        result['sequence']=sequence
        return result
    raise TimeoutError('Native retirement telemetry never became stable')


class Telemetry:
    PREFIX='CyberpunkVR_OwnedExecution'
    def __init__(self,pid,game):
        from live_read import Reader
        from live_symbols import resolve
        self.pid=pid
        names=('Seq','Allocator','RetirementTotals','Retirement','RetirementEventSeq','RetirementEventCount',
               'RetirementEvents','Cache','CacheRows')
        self.symbols=resolve(pid,Path(game)/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
                             [self.PREFIX+name for name in names])
        self.reader=Reader(pid,Path(game)/'bin/x64/Cyberpunk2077.exe',0,0)
    def words(self,name,count):
        return struct.unpack(f'<{count}Q',self.reader.read(int(self.symbols[self.PREFIX+name],16),count*8))
    def read(self):
        result=snapshot(lambda name:struct.unpack('<I',self.reader.read(int(self.symbols[self.PREFIX+name],16),4))[0],
            lambda name:self.words(name,1)[0],self.words)
        result['pid']=self.pid;result['dll']=self.symbols['sha256']
        return result
    def close(self):self.reader.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid',type=int,required=True)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    game=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    telemetry=Telemetry(args.pid,game)
    try:
        value=telemetry.read();args.out.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8')
        print(json.dumps(dict(pid=value['pid'],dll=value['dll'],allocator=value['Allocator'],
            totals=value['RetirementTotals'],cache=value['Cache'],eventSequence=value['events']['sequence'],eventCount=value['events']['count'])))
    finally:telemetry.close()


if __name__=='__main__':main()
