"""toy_maps.py SRC.glb OUTDIR NAME: a Meshy model's own maps out of its .glb, at a toy's size --
OUTDIR/NAME_d.png (colour, COLOR px), NAME_r.png and NAME_m.png (roughness and metalness: the glTF
metallicRoughness map's green and blue, MASK px). The normal map is baked (make_toy.py)."""
import io
import json
import struct
import sys
from pathlib import Path

from PIL import Image

COLOR, MASK = 2048, 1024

src, out, name = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
out.mkdir(parents=True, exist_ok=True)
data = src.read_bytes()
length = struct.unpack_from('<I', data, 12)[0]
doc = json.loads(data[20:20 + length])
blob = 20 + length + 8


def image(index):
    view = doc['bufferViews'][doc['images'][doc['textures'][index]['source']]['bufferView']]
    start = blob + view.get('byteOffset', 0)
    return Image.open(io.BytesIO(data[start:start + view['byteLength']]))


pbr = doc['materials'][0]['pbrMetallicRoughness']
image(pbr['baseColorTexture']['index']).convert('RGB').resize((COLOR, COLOR), Image.LANCZOS).save(
    out / (name + '_d.png'))
_, g, b = image(pbr['metallicRoughnessTexture']['index']).convert('RGB').resize((MASK, MASK), Image.LANCZOS).split()
g.save(out / (name + '_r.png'))
b.save(out / (name + '_m.png'))
print('maps:', ', '.join(p.name for p in sorted(out.glob(name + '_[drm].png'))))
