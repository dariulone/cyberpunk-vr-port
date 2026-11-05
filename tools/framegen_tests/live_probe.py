"""Read-only current-launch framegen report and stage counters. No game commands."""
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
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid',type=int,required=True)
    p.add_argument('--out',type=Path,required=True)
    p.add_argument('--dll',type=Path,default=ROOT/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll')
    p.add_argument('--seconds',type=float,default=3)
    a=p.parse_args()
    if a.out.exists() or not a.out.parent.is_dir():raise RuntimeError('Fresh output under an existing directory required')
    names=['CyberpunkVR_FramegenReport','CyberpunkVR_FramegenReportSeq','CyberpunkVR_FramegenInputStages','CyberpunkVR_RuntimeDiagnostics']
    symbols=resolve(a.pid,a.dll.resolve(),names)
    game=Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077')
    r=Reader(a.pid,game/'bin/x64/Cyberpunk2077.exe',0,0)
    samples=[]
    try:
        end=time.monotonic()+min(30,max(.5,a.seconds))
        while time.monotonic()<end:
            seq=int(symbols[names[1]],16)
            for _ in range(100):
                before=struct.unpack('<I',r.read(seq,4))[0]
                if before&1:continue
                data=r.read(int(symbols[names[0]],16),2048).split(b'\0',1)[0]
                after=struct.unpack('<I',r.read(seq,4))[0]
                if before==after and not after&1:break
            else:raise RuntimeError('Report remained busy')
            diagnostics=struct.unpack('<i',r.read(int(symbols[names[3]],16),4))[0]!=0
            samples.append(dict(time_ns=time.time_ns(),sequence=after,diagnostics_enabled=diagnostics,report=json.loads(data) if data and diagnostics else None,
                stages=struct.unpack('<10Q',r.read(int(symbols[names[2]],16),80))))
            time.sleep(.25)
    finally:r.close()
    log=(game/'red4ext/plugins/cyberpunkvrport/log/cyberpunkvrport.log').read_text(encoding='utf-8',errors='replace')
    result=dict(symbols=symbols,samples=samples,log=[s for s in log.splitlines() if '[framegen]' in s],game_memory_written=False)
    a.out.write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps(dict(first=samples[0],last=samples[-1],log=result['log']),indent=2))

if __name__=='__main__':main()
