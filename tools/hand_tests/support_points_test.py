"""Check generated support files without modifying existing primary or pistol poses."""
from pathlib import Path
import re
ROOT=Path(__file__).resolve().parents[2]
files=list((ROOT/'mods/config').glob('CyberpunkVR_TwoHandGrip_*.ini'))
names={p.stem.removeprefix('CyberpunkVR_TwoHandGrip_'):p for p in files}
source=(ROOT/'src/Anim/TwoHandGrip.cpp').read_text(encoding='utf-8')
limit=int(re.search(r'kMaxHolds\s*=\s*(\d+)',source).group(1))
cand=int(re.search(r'kMaxCand\s*=\s*(\d+)',source).group(1))
assert len(names)<limit,(len(names),limit)
new=[n for n in names if '@z_' in n]
assert len(new)==64,len(new)   # the katana's two moved to its handle (@hNNN)
for name in [n for n in names if '@trigger' in n or n.startswith('katana@h')]:
    lines=names[name].read_text().splitlines();primary=names[name.split('@')[0]].read_text().splitlines()
    assert len([x for x in lines if x.startswith('F ')])==len([x for x in primary if x.startswith('F ')]),name
    row=next(x for x in lines if x.startswith('W '));assert len(row.split())==8 and 'nan' not in row.lower(),name
assert len([n for n in names if '@trigger' in n])==18 and len([n for n in names if n.startswith('katana@h')])==6
pistols={'unity','liberty','lexington','nue','tamayura','silverhand','kenshin','omaha','burya','nova','quasar','overture','kappa','yukimura','chao','metel','grit','ticon'}
for name in new:
    base=name.split('@')[0];assert base not in pistols
    lines=names[name].read_text().splitlines();primary=names[base].read_text().splitlines()
    assert [x for x in lines if x.startswith('F ')]==[x for x in primary if x.startswith('F ')]
    row=next(x for x in lines if x.startswith('W '));assert len(row.split())==8
    assert 'nan' not in row.lower() and 'inf' not in row.lower()
for base in {n.split('@')[0] for n in new}:
    alts=sorted(n for n in names if n.startswith(base+'@'))
    assert 1+len(alts)<=cand,(base,alts)
    if base+'@mag' in names:assert alts[0]==base+'@mag',(base,alts)
print('PASS:',len(new),'new points;',len(names),'of',limit,'slots; magazine indices and pistol poses retained')
