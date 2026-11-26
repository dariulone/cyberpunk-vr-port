"""Build the two tubular VR housings whose native cap occludes the shader lens.

Inputs are WolvenKit JSON mesh exports. scope_shell filters index buffers in C++.
Writes private depot paths into the staging directory for cyberpunkvrport.archive.
"""
import argparse
import base64
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def source(root,name):
    path,=root.rglob(name+'.mesh.json')
    return json.loads(path.read_text(encoding='utf-8-sig'))


def description(header,chunk):
    vertex=chunk['chunkVertices']
    return ' '.join(map(str,(vertex['byteOffsets']['Elements'][0],vertex['vertexLayout']['slotStrides']['Elements'][0],
        chunk['numVertices'],header['indexBufferOffset']+chunk['chunkIndices']['teOffset'],chunk['numIndices'])))


def metadata(header):
    return ' '.join(str(header[field][k]) for field in ('quantizationScale','quantizationOffset') for k in 'XYZ')


def generate(args,work,key):
    stem='w_att__scope_'+key+'_01'
    root=source(args.native,stem)['Data']['RootChunk']
    lens=next(e['name'] for e in root['materialEntries'] if e['isLocalInstance'] and
        root['localMaterialBuffer']['materials'][e['index']]['baseMaterial']['DepotPath']['$value'].endswith('glass_scope.mt'))
    index=root['appearances'][0]['Data']['chunkMaterials'].index(lens)
    blob=root['renderResourceBlob']['Data']
    hdr=blob['header']
    (work/'lens.bin').write_bytes(base64.b64decode(blob['renderBuffer']['Bytes']))
    fields=[metadata(hdr),description(hdr,hdr['renderChunkInfos'][index])]
    shell=source(args.native,stem+'_cap')
    blob=shell['Data']['RootChunk']['renderResourceBlob']['Data']
    hdr=blob['header'];chunks=hdr['renderChunkInfos']
    fields.extend([metadata(hdr),str(len(chunks)),*(description(hdr,c) for c in chunks)])
    (work/'shell.bin').write_bytes(base64.b64decode(blob['renderBuffer']['Bytes']))
    subprocess.run([args.generator,work/'lens.bin',work/'shell.bin',work/'result.bin'],
        input='\n'.join(fields),text=True,check=True)
    blob['renderBuffer']['Bytes']=base64.b64encode((work/'result.bin').read_bytes()).decode('ascii')
    depot='base/vrport/optics/'+key+'_housing.mesh'
    shell['Header']['ArchiveFileName']=depot.replace('/','\\')
    dst=work/'json'/(Path(depot).name+'.json')
    dst.write_text(json.dumps(shell,separators=(',',':')),encoding='utf-8')
    subprocess.run([args.wkit,'convert','deserialize',str(dst),'-o',str(work/'cooked')],check=True)
    dest=args.pack/depot
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(work/'cooked'/Path(depot).name,dest)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('native','generator','wkit','pack'):p.add_argument('--'+name,type=Path,required=True)
    args=p.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        work=Path(tmp)
        for sub in ('json','cooked'):(work/sub).mkdir()
        for key in ('long_01','long_02'):generate(args,work,key)
