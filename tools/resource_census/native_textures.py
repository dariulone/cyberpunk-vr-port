"""Attach-only, read-only 2.5 native texture ownership snapshot. No injected calls.

Raw native pool metadata is not a D3D12 GetDesc/COM refcount. Repeated stable reads
reduce torn snapshots but cannot lock the game's allocation/retirement threads.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import struct
import time
import re

TIMESTAMP = 1792368047
IMAGE_SIZE = 0x581B000
# PE source and installed executable have identical SHA256:
# d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4.
# Do not use IDA's maximum segment address as PE OptionalHeader.SizeOfImage.
CODE_SPANS = {
    0x5EA214: '48895c24104889742418574883ec2041b001488bf1e86abe0300488d0573a4a7',
    0x5E9D68: '48895c2408574883ec20488d052fa9a702488bf9488901488b05622d5d03488b',
    0x626098: '4883ec284c8bd9488d05dafca3024889014533c94c89491041ba010000004489',
    0x3E3248: '48895c2408574883ec20488b154faa7e03488bf9807a0100740433c0eb618b01',
    0x5EA284: '48895c2408488974241048897c241855488bec4883ec30488b5d48458ad9448a',
}
POOL_RVA, OWNER_RVA = 0x3BCDCA8, 0x3BBCAE8
ENTRY_START, STRIDE, CAPACITY = 0x2F1D0, 0xB0, 131072
BACKING_VTABLE = 0x30646A8


def decode_entry(raw, handle):
    if len(raw) < STRIDE:
        raise ValueError('Truncated native pool entry')
    refs, resource = struct.unpack_from('<i4xQ', raw)
    width, height, depth = struct.unpack_from('<HHH', raw, 0x56)
    return dict(handle=handle, nativeRefs=refs, resource=hex(resource), width=width,
                height=height, depth=depth, mipSampleBits=raw[0x5C],
                nativeFormat=raw[0x5D], nativeFlags=struct.unpack_from('<H', raw, 0x5E)[0],
                nativeState=struct.unpack_from('<I', raw, 0x50)[0])


def entry_identity(raw):
    # Ignore unrelated cached descriptor fields, retain refs, resource and shape.
    return raw[:16] + raw[0x50:0x62]


def mapped_export(read, base, name):
    """Resolve one actual mapped PE export, with no PDB, code calls or remote writes."""
    dos = read(base, 64)
    pe = struct.unpack_from('<I', dos, 0x3C)[0]
    if dos[:2] != b'MZ' or not 64 <= pe <= 1024 * 1024:
        raise ValueError('Invalid mapped plugin DOS header')
    header = read(base + pe, 160)
    size = struct.unpack_from('<I', header, 80)[0]
    rva, extent = struct.unpack_from('<II', header, 136)
    if header[:4] != b'PE\0\0' or struct.unpack_from('<H', header, 24)[0] != 0x20B or not 40 <= extent <= size or not 0 < rva <= size - extent:
        raise ValueError('Invalid mapped plugin export directory')
    def at(offset, length):
        if offset < 0 or length < 0 or offset + length > size:
            raise ValueError('Export data outside mapped image')
        return read(base + offset, length)
    table = at(rva, 40)
    functions, names, addresses, pointers, ordinals = struct.unpack_from('<IIIII', table, 20)
    if not 0 < names <= functions <= 65536:
        raise ValueError('Export count outside diagnostic bound')
    wanted = name.encode('ascii'); lo, hi = 0, names
    while lo < hi:
        mid = (lo + hi) // 2
        pointer = struct.unpack('<I', at(pointers + mid * 4, 4))[0]
        text = at(pointer, min(128, size - pointer)).split(b'\0', 1)[0]
        if text < wanted:
            lo = mid + 1
        elif text > wanted:
            hi = mid
        else:
            ordinal = struct.unpack('<H', at(ordinals + mid * 2, 2))[0]
            if ordinal >= functions:
                raise ValueError('Export ordinal outside function table')
            target = struct.unpack('<I', at(addresses + ordinal * 4, 4))[0]
            if not 0 < target < size or rva <= target < rva + extent:
                raise ValueError('Invalid/forwarded diagnostic export')
            return base + target
    raise ValueError(f'Mapped plugin has no export {name}')


def texture_snapshot(read, game_base, address):
    if not address:
        return None
    raw = read(address, 0x80)
    if struct.unpack_from('<Q', raw)[0] != game_base + 0x3064700:
        return dict(address=hex(address), error='not the verified DynamicTexture vtable')
    width, height = struct.unpack_from('<II', raw, 0x40)
    owners = []
    for field in (0x70, 0x78):
        pointer = struct.unpack_from('<Q', raw, field)[0]
        row = dict(field=hex(field), address=hex(pointer))
        if pointer:
            native = read(pointer, 32)
            table, refs = struct.unpack_from('<Qi', native)
            if table == game_base + BACKING_VTABLE:
                row.update(refs=refs, handle=struct.unpack_from('<I', native, 24)[0], stable=native == read(pointer, 32))
            else:
                row['error'] = 'unexpected backing vtable'
        owners.append(row)
    again = read(address, 0x80)
    identity = lambda data: data[:0x18] + data[0x28:0x30] + data[0x40:0x48] + data[0x70:0x80]
    return dict(address=hex(address), width=width, height=height, backings=owners, creationFlags=raw[0x69], dynamicFormat=raw[0x58],
                self=hex(struct.unpack_from('<Q',raw,8)[0]), control=hex(struct.unpack_from('<Q',raw,16)[0]), objectSerial=struct.unpack_from('<Q',raw,0x28)[0],
                resourcePathHash=hex(struct.unpack_from('<Q', raw, 0x30)[0]), stable=identity(raw) == identity(again))


def icon_atlases_snapshot(read, game_base, native_rows=()):
    """Typed manager→weak IconAtlas→pages→strong TextureAtlas→strong Dtex path."""
    system = struct.unpack('<Q', read(game_base + 0x3C2B390, 8))[0]
    manager_field = read(system + 0x250, 8)
    manager = struct.unpack('<Q', manager_field)[0]
    if not manager or struct.unpack('<Q', read(manager, 8))[0] != game_base + 0x30B0730:
        raise ValueError('Not the verified inkIconAtlasManager')
    header = read(manager + 0x10, 16)
    pointer, capacity, count = struct.unpack('<QII', header)
    if not count <= capacity <= 4096 or (count and not pointer):
        raise ValueError('IconAtlas registry exceeds bound/is invalid')
    handles = read(pointer, count * 16) if count else b''
    default_handle = read(manager + 0x20, 16)
    sources = [('default', -1, default_handle)] + [('weak-registry', i, handles[i*16:(i+1)*16]) for i in range(count)]
    native = {row['handle']: row for row in native_rows}
    atlases, errors, total_pages, unique_pages = [], [], 0, {}
    def owner(object_pointer, control, expected_vtable):
        result = dict(address=hex(object_pointer), control=hex(control), alive=False)
        refs = struct.unpack('<II', read(control, 8)) if control else (0, 0)
        result.update(strongRefs=refs[0], weakRefs=refs[1])
        if not object_pointer or not refs[0]:return result
        data = read(object_pointer, 0x18)
        table, self_pointer, self_control = struct.unpack('<QQQ', data)
        result.update(vtableRva=hex(table-game_base), selfMatches=self_pointer==object_pointer and self_control==control)
        result['alive'] = table == game_base + expected_vtable and result['selfMatches']
        if not result['alive']:result['error']='native type/self-control identity mismatch'
        return result
    for source, index, handle in sources:
        obj, ctl = struct.unpack('<QQ', handle)
        row = dict(source=source, index=index, pages=[])
        try:
            row.update(owner(obj, ctl, 0x30B05E8))
            if row['alive']:
                pages_header = read(obj + 0x158, 16)
                pages_pointer, pages_capacity, pages_count = struct.unpack('<QII', pages_header)
                unique_pages[obj] = pages_count
                total_pages += pages_count
                if not pages_count <= pages_capacity <= 4096 or total_pages > 8192 or (pages_count and not pages_pointer):
                    raise ValueError('IconAtlas page vector exceeds bound/is invalid')
                pages = read(pages_pointer, pages_count * 0x88) if pages_count else b''
                for page in range(pages_count):
                    raw = pages[page*0x88:(page+1)*0x88]
                    texture_atlas, atlas_control = struct.unpack_from('<QQ', raw, 0x50)
                    item = dict(index=page, pageAddress=hex(pages_pointer+page*0x88))
                    try:
                        item['textureAtlas'] = owner(texture_atlas, atlas_control, 0x30B0498)
                        if item['textureAtlas']['alive']:
                            dtex_handle = read(texture_atlas + 0x68, 16)
                            dtex, dtex_control = struct.unpack('<QQ', dtex_handle)
                            item['dynamicTextureOwner'] = owner(dtex, dtex_control, 0x3064700)
                            if item['dynamicTextureOwner']['alive']:
                                item['texture'] = texture_snapshot(read, game_base, dtex)
                                for backing in item['texture']['backings']:
                                    match = native.get(backing.get('handle'))
                                    if match:
                                        backing['nativeResource'] = {key: match[key] for key in ('handle','resource','nativeRefs','width','height','nativeFormat','nativeFlags','stable')}
                            item['dtexHandleStable'] = dtex_handle == read(texture_atlas + 0x68, 16)
                        item['pageHandleStable'] = raw[0x50:0x60] == read(pages_pointer+page*0x88+0x50, 16)
                    except (OSError, ValueError) as error:item['error']=str(error)
                    row['pages'].append(item)
                row.update(pageCount=pages_count, pagesStable=pages_header==read(obj+0x158,16), weakStillAlive=struct.unpack('<I',read(ctl,4))[0]>0)
        except (OSError, ValueError) as error:
            row['error']=str(error);errors.append(dict(source=source,index=index,error=str(error)))
        atlases.append(row)
    return dict(system=hex(system), manager=hex(manager), registryCount=count, totalPages=total_pages,
        uniqueAtlasCount=len(unique_pages), uniquePageCount=sum(unique_pages.values()), atlases=atlases, errors=errors,
        stable=manager_field==read(system+0x250,8) and header==read(manager+0x10,16) and default_handle==read(manager+0x20,16),
        scope='bounded typed native path; manager custom registry is weak, each live page TextureAtlas and its Dtex fields are strong; no calls/ref changes')


def layers_snapshot(read, game_base):
    # EngineObjects.cpp Layers(), same 2.5 native address-library hash2635337807:
    # section3 RVA0x3903000 + offset0x328390 = singleton RVA0x3C2B390.
    singleton = read(game_base + 0x3C2B390, 8)
    system = struct.unpack('<Q', singleton)[0]
    if not system:
        raise ValueError('InkSystem singleton is not ready')
    def array(address, limit):
        header = read(address, 16)
        pointer, capacity, count = struct.unpack('<QII', header)
        if not count <= capacity <= limit or (count and not pointer):
            raise ValueError('Layer enumeration array is invalid/busy')
        data = read(pointer, count * 16) if count else b''
        return header, data, count
    def hash_name(name):
        value = 14695981039346656037
        for byte in name.encode('utf-8'):value = ((value ^ byte) * 1099511628211) & 0xFFFFFFFFFFFFFFFF
        return value
    names = {}
    sdk = Path(__file__).resolve().parents[2] / 'externals/RED4ext.SDK/include/RED4ext/Scripting/Natives/Generated/ink'
    for path in [*sdk.glob('*Layer.hpp'),sdk/'CacheWidget.hpp',sdk/'ImageWidget.hpp',sdk/'MaskWidget.hpp',sdk/'VirtualWindow.hpp']:
        match = re.search(r'NAME = "([^"]+)"', path.read_text(encoding='utf-8'))
        if match:names[hash_name(match.group(1))] = match.group(1)
    def native_type(vtable):
        getter = struct.unpack('<Q', read(vtable, 8))[0]
        if not game_base <= getter < game_base + IMAGE_SIZE:return {}
        code = read(getter, 8)
        if code[:3] != b'\x48\x8b\x05' or code[7] != 0xC3:return dict(getter=hex(getter), unknown=True)
        storage = getter + 7 + struct.unpack_from('<i', code, 3)[0]
        if not game_base <= storage < game_base + IMAGE_SIZE:return dict(getter=hex(getter), unknown=True)
        pointer = struct.unpack('<Q', read(storage, 8))[0]
        ancestry = []
        for _ in range(16):
            if not pointer:break
            data = read(pointer, 0x70)
            parent, name = struct.unpack_from('<QQ', data, 0x10)
            ancestry.append(dict(type=hex(pointer), nameHash=hex(name), knownName=names.get(name), size=struct.unpack_from('<I', data, 0x68)[0]))
            pointer = parent
        return dict(getter=hex(getter), ancestry=ancestry)
    def window_info(address):
        first = read(address, 0x200)
        table = struct.unpack_from('<Q', first)[0]
        info = native_type(table)
        row = dict(address=hex(address), vtableRva=hex(table-game_base), nameHash=hex(struct.unpack_from('<Q', first, 0x188)[0]),
            visible=bool(first[0x1F8]), nativeType=info)
        if any(t['nameHash'] == hex(hash_name('inkVirtualWindow')) for t in info.get('ancestry', [])):
            fields = read(address + 0x4E8, 16)
            row.update(kind=read(address + 800, 1)[0], texture=texture_snapshot(read, game_base, struct.unpack_from('<Q', fields)[0]),
                stable=fields == read(address + 0x4E8, 16))
        else:
            row['scope'] = 'registered widget, not a verified VirtualWindow'
        return row
    managers_header, managers, manager_count = array(system + 0x380, 32)
    rows, errors = [], []
    for manager_index in range(manager_count):
        manager = struct.unpack_from('<Q', managers, manager_index * 16)[0]
        if not manager:
            continue
        # Only manager0 is established as the LayerManager by repository native
        # code. Other entries are reported as identities, not guessed layouts.
        if manager_index:
            rows.append(dict(manager=hex(manager), managerIndex=manager_index, scope='uninterpreted secondary manager'))
            continue
        header, handles, count = array(manager + 0x38, 256)
        for index in range(count):
            layer = struct.unpack_from('<Q', handles, index * 16)[0]
            if not layer:
                continue
            row = dict(managerIndex=manager_index, layerIndex=index, address=hex(layer))
            try:
                raw = read(layer, 0x150)
                vtable = struct.unpack_from('<Q', raw)[0]
                window = struct.unpack_from('<Q', raw, 0x40)[0]
                row.update(vtable=hex(vtable), vtableRva=hex(vtable-game_base), active=bool(raw[0xB0]), window=hex(window))
                row['nativeType'] = native_type(vtable)
                if window:
                    first = read(window, 0x5B0)
                    row.update(windowVtable=hex(struct.unpack_from('<Q', first)[0]),
                               windowNameHash=hex(struct.unpack_from('<Q', first, 0x188)[0]),
                               windowKind=first[800], texture=texture_snapshot(read, game_base, struct.unpack_from('<Q', first, 0x4E8)[0]))
                    row['windowStable'] = first[0x4E8:0x4F8] == read(window + 0x4E8, 16)
                # Layer's weak widget registration array. Native resize code
                # 132654C walks +0x138 in16-byte units, locks weak references,
                # and erases expired entries. Read-only snapshots never lock or
                # dereference an expired weak target in the remote process.
                windows_header, windows, window_count = array(layer + 0x138, 4096)
                row['registeredWindows'] = []
                for window_index in range(window_count):
                    target, control = struct.unpack_from('<QQ', windows, window_index * 16)
                    record = dict(index=window_index, address=hex(target), control=hex(control))
                    try:
                        refs = struct.unpack('<i', read(control, 4))[0] if control else 0
                        record['strongRefs'] = refs
                        if target and refs > 0:
                            record.update(window_info(target))
                            record['weakStillAlive'] = struct.unpack('<i', read(control, 4))[0] > 0
                    except (OSError, ValueError) as error:
                        record.update(stable=False, error=str(error))
                    row['registeredWindows'].append(record)
                row['registeredWindowsStable'] = windows_header == read(layer + 0x138, 16)
                if any(t['nameHash'] == hex(hash_name('inkWorldLayer')) for t in row['nativeType'].get('ancestry', [])):
                    world_header, components, component_count = array(layer + 0x170, 4096)
                    row['worldComponents'] = []
                    for component_index in range(component_count):
                        wrapper = struct.unpack_from('<Q', components, component_index * 16)[0]
                        if not wrapper:continue
                        try:
                            wrapper_data = read(wrapper, 0x80)
                            wrapper_table = struct.unpack_from('<Q', wrapper_data)[0]
                            # Record candidate getter code identities for IDA; no
                            # invocation and no guessed object-field dereference.
                            candidates = struct.unpack('<QQ', read(wrapper_table + 0xE8, 16))
                            component_row = dict(address=hex(wrapper), vtableRva=hex(wrapper_table-game_base),
                                rawPrefix=wrapper_data.hex(), windowGetterCandidates=[hex(x-game_base) for x in candidates])
                            if wrapper_table == game_base + 0x30636E8 and candidates[0] == game_base + 0x5D1674:
                                component, control = struct.unpack_from('<QQ', wrapper_data, 8)
                                refs = struct.unpack('<i', read(control, 4))[0] if control else 0
                                component_row.update(component=hex(component), strongRefs=refs)
                                if component and refs > 0:
                                    world_window = struct.unpack('<Q', read(component + 0x140, 8))[0]
                                    component_row['window'] = window_info(world_window) if world_window else None
                                    component_row['weakStillAlive'] = struct.unpack('<i', read(control, 4))[0] > 0
                            row['worldComponents'].append(component_row)
                        except OSError as error:
                            row['worldComponents'].append(dict(address=hex(wrapper), error=str(error)))
                    row['worldComponentsStable'] = world_header == read(layer + 0x170, 16)
                if any(t['nameHash'] == hex(hash_name('inkOffscreenLayer')) for t in row['nativeType'].get('ancestry', [])):
                    # Native ctor14B4250/dtor14B41B8: +158 is a vector of
                    # shared entry owners, +168 is a vector of such vectors.
                    # Entry dtor27B06A4 releases a Window Handle at entry+0.
                    total = 0
                    def offscreen_entries(array_address):
                        nonlocal total
                        array_header, data, entry_count = array(array_address, 4096)
                        total += entry_count
                        if total > 8192:raise ValueError('Offscreen entries exceed diagnostic bound')
                        result = []
                        for entry_index in range(entry_count):
                            entry = struct.unpack_from('<Q', data, entry_index * 16)[0]
                            entry_row = dict(index=entry_index, address=hex(entry))
                            if entry:
                                try:
                                    target = struct.unpack('<Q', read(entry, 8))[0]
                                    entry_row['window'] = window_info(target) if target else None
                                except (OSError, ValueError) as error:entry_row['error'] = str(error)
                            result.append(entry_row)
                        return dict(entries=result, stable=array_header == read(array_address, 16))
                    row['offscreen158'] = offscreen_entries(layer + 0x158)
                    outer_header, outer, outer_count = array(layer + 0x168, 256)
                    outer_pointer = struct.unpack_from('<Q', outer_header)[0]
                    row['offscreen168'] = [offscreen_entries(outer_pointer + i * 16) for i in range(outer_count)]
                    row['offscreen168Stable'] = outer_header == read(layer + 0x168, 16)
                row['stable'] = raw[:8] + raw[0x40:0x50] == read(layer, 8) + read(layer + 0x40, 16)
            except (OSError, ValueError) as error:
                row.update(stable=False, error=str(error));errors.append(dict(layer=hex(layer), error=str(error)))
            rows.append(row)
        if header != read(manager + 0x38, 16):
            errors.append(dict(manager=hex(manager), error='layer array changed during read'))
    registry_pointer = read(system + 0x358, 8)
    registry = struct.unpack('<Q', registry_pointer)[0]
    registry_report = dict(address=hex(registry), records=[], errors=[])
    if registry:
        registry_header = read(registry, 24)
        data_pointer, capacity, record_count = struct.unpack_from('<QII', registry_header, 8)
        if not record_count <= capacity <= 4096 or (record_count and not data_pointer):
            raise ValueError('External dynamic texture registry is invalid/busy')
        record_data = read(data_pointer, record_count * 56) if record_count else b''
        total_consumers = 0
        def weak_widget(pointer, control, producer=False):
            item = dict(address=hex(pointer), control=hex(control))
            refs = struct.unpack('<II', read(control, 8)) if control else (0, 0)
            item.update(strongRefs=refs[0], weakRefs=refs[1], alive=bool(pointer and refs[0]))
            if not item['alive']:
                return item
            table = struct.unpack('<Q', read(pointer, 8))[0]
            info = native_type(table)
            item['nativeType'] = info
            item['nameHash'] = hex(struct.unpack('<Q', read(pointer + 0x188, 8))[0])
            ancestry = {t['nameHash'] for t in info.get('ancestry', [])}
            cache = hex(hash_name('inkCacheWidget')) in ancestry
            image = hex(hash_name('inkImageWidget')) in ancestry
            mask = hex(hash_name('inkMaskWidget')) in ancestry
            matches = cache if producer else image or mask
            if matches:
                offset = 0x278 if producer else 0x260
                handle = read(pointer + offset, 16)
                object_pointer, object_control = struct.unpack('<QQ', handle)
                # 27B4794/1AE004 is MaskWidget's strong setter; ImageWidget's
                # field260 is weak (its dtor2750B0 decrements control+4).
                binding_refs = struct.unpack('<II', read(object_control, 8)) if object_control else (0, 0)
                item['textureHandle'] = dict(field=hex(offset), object=hex(object_pointer), control=hex(object_control),
                    ownership='strong-cache' if producer else 'strong-mask' if mask else 'weak-image', strongRefs=binding_refs[0], weakRefs=binding_refs[1])
                item['texture'] = texture_snapshot(read, game_base, object_pointer) if object_pointer and binding_refs[0] else None
                item['handleStable'] = handle == read(pointer + offset, 16)
                if producer:
                    item['externalNameHash'] = hex(struct.unpack('<Q', read(pointer + 0x240, 8))[0])
                    item['cacheMode'] = read(pointer + 0x250, 1)[0]
            item['weakStillAlive'] = struct.unpack('<I', read(control, 4))[0] > 0
            return item
        for index in range(record_count):
            raw = record_data[index * 56:(index + 1) * 56]
            name, producer, control = struct.unpack_from('<QQQ', raw)
            item = dict(index=index, nameHash=hex(name), consumersA=[], consumersB=[])
            try:
                item['producer'] = weak_widget(producer, control, True)
                for offset, label in ((24, 'consumersA'), (40, 'consumersB')):
                    pointer, cap, count = struct.unpack_from('<QII', raw, offset)
                    total_consumers += count
                    if not count <= cap <= 4096 or total_consumers > 16384 or (count and not pointer):
                        raise ValueError('External dynamic texture consumers exceed bound')
                    elements = read(pointer, count * 16) if count else b''
                    for element in range(count):
                        obj, ctl = struct.unpack_from('<QQ', elements, element * 16)
                        try:
                            item[label].append(weak_widget(obj, ctl))
                        except (OSError, ValueError) as error:
                            item[label].append(dict(address=hex(obj), error=str(error)))
                    item[label+'Stable'] = raw[offset:offset+16] == read(data_pointer + index * 56 + offset, 16)
                item['stable'] = raw[:24] == read(data_pointer + index * 56, 24)
            except (OSError, ValueError) as error:
                item.update(stable=False, error=str(error));registry_report['errors'].append(dict(index=index, error=str(error)))
            registry_report['records'].append(item)
        registry_report.update(count=record_count, totalConsumers=total_consumers,
            stable=registry_pointer == read(system + 0x358, 8) and registry_header[8:24] == read(registry + 8, 16))
    return dict(system=hex(system), managerCount=manager_count, rows=rows, errors=errors, externalDynamicTextures=registry_report,
                stable=singleton == read(game_base + 0x3C2B390, 8) and managers_header == read(system + 0x380, 16),
                scope='current first InkSystem LayerManager only; no calls/AddRef and no arbitrary heap scan')


def hud_snapshot(read, game_base, export_address):
    """Current HUD entries only; never searches arbitrary heap memory for owners."""
    exported = read(export_address, 16)
    layer, main_window = struct.unpack('<QQ', exported)
    if not layer:
        raise ValueError('HUD layer has not been published')
    header = read(layer + 0x150, 16)
    entries, capacity, count = struct.unpack('<QII', header)
    if not count <= capacity <= 4096 or (count and not entries):
        raise ValueError('HUD entry array is invalid/busy')
    names = {}
    channel_header = Path(__file__).resolve().parents[2] / 'include/Runtimes/HudChannels.hpp'
    for name in re.findall(r'EntryHash\("([^"\n]+)"\)', channel_header.read_text(encoding='utf-8')):
        value = 14695981039346656037
        for byte in name.encode('utf-8'):
            value = ((value ^ byte) * 1099511628211) & 0xFFFFFFFFFFFFFFFF
        names[value] = name

    def texture(address):
        return texture_snapshot(read, game_base, address)

    data = read(entries, count * 0x130) if count else b''
    rows, errors = [], []
    for index in range(count):
        raw = data[index * 0x130:(index + 1) * 0x130]
        name = struct.unpack_from('<Q', raw)[0]
        slot, window, widget, root, controller = [struct.unpack_from('<Q', raw, offset)[0] for offset in (0xB8, 0xC8, 0xD8, 0xF8, 0x108)]
        row = dict(index=index, nameHash=hex(name), knownName=names.get(name), enabled=bool(raw[8]),
                   useSeparateWindow=bool(raw[0xA0]), window=hex(window), slotWidget=hex(widget),
                   rootWidget=hex(root), controller=hex(controller))
        try:
            row['slotTexture'] = texture(slot)
            row['windowTexture'] = texture(struct.unpack('<Q', read(window + 0x4E8, 8))[0]) if window else None
            again = read(entries + index * 0x130, 0x130)
            row['stable'] = raw[:8] + raw[0xB8:0x118] == again[:8] + again[0xB8:0x118]
        except (OSError, ValueError) as error:
            row.update(stable=False, error=str(error));errors.append(dict(index=index, error=str(error)))
        rows.append(row)
    return dict(layer=hex(layer), mainWindow=hex(main_window), entries=hex(entries), count=count, capacity=capacity,
                stable=read(export_address, 16) == exported and read(layer + 0x150, 16) == header, rows=rows, errors=errors,
                scope='current HUDLayer entries, weak diagnostic address snapshot; no AddRef/native calls or arbitrary heap scan')


def snapshot(read, base, sizes=(768, 1024), capacity=CAPACITY):
    """The injectable reader also supports offline layout/lifetime regression tests."""
    def u64(address):
        return struct.unpack('<Q', read(address, 8))[0]

    pool = u64(base + POOL_RVA)
    owner = u64(base + OWNER_RVA)
    if not pool or not owner:
        raise ValueError('Native texture registry is not ready')
    listing = u64(owner + 0xD0)
    if not listing:
        raise ValueError('Dynamic backing list is not ready')
    header = read(listing, 24)
    pointers, count = struct.unpack_from('<Q4xI', header, 8)
    if count > 32768 or (count and not pointers):
        raise ValueError(f'Invalid/busy backing list: count={count}')
    addresses = struct.unpack('<' + 'Q' * count, read(pointers, count * 8)) if count else ()
    backings, errors = {}, []
    for address in addresses:
        try:
            first = read(address, 32)
            vtable, refs = struct.unpack_from('<Qi', first)
            handle = struct.unpack_from('<I', first, 24)[0]
            if vtable == base + BACKING_VTABLE and 0 < handle <= capacity:
                second = read(address, 32)
                backings.setdefault(handle, []).append(dict(address=hex(address), refs=refs,
                    stable=first == second))
        except OSError as error:
            errors.append(dict(backing=hex(address), error=str(error)))
    rows = []
    # One bounded scan (22 MiB maximum), not a per-frame walk. Chunk failures are
    # reported rather than silently turning missing native owners into refs=0.
    for start in range(0, capacity, 512):
        entries = min(512, capacity - start)
        address = pool + ENTRY_START + start * STRIDE
        try:
            data = read(address, entries * STRIDE)
        except OSError as error:
            errors.append(dict(firstHandle=start + 1, count=entries, error=str(error)))
            continue
        for index in range(entries):
            raw = data[index * STRIDE:(index + 1) * STRIDE]
            row = decode_entry(raw, start + index + 1)
            if row['width'] not in sizes or row['height'] != row['width'] or row['resource'] == '0x0':
                continue
            try:
                again = read(address + index * STRIDE, STRIDE)
                row['stable'] = entry_identity(raw) == entry_identity(again)
            except OSError as error:
                row['stable'] = False
                row['error'] = str(error)
            row['backings'] = backings.get(row['handle'], [])
            rows.append(row)
    header_after = read(listing, 24)
    list_stable = header[8:24] == header_after[8:24]
    if list_stable and count:
        list_stable = read(pointers, count * 8) == struct.pack('<' + 'Q' * count, *addresses)
    return dict(pool=hex(pool), backingList=hex(listing), backingCount=count,
                poolStable=pool == u64(base + POOL_RVA),
                ownerStable=owner == u64(base + OWNER_RVA) and listing == u64(owner + 0xD0),
                backingListStable=list_stable, scannedCapacity=capacity, rows=rows, errors=errors,
                limitations=['Native refs are engine handle refs, not COM refs.',
                             'No synchronization with the game: unstable rows must be repeated.',
                             'Matching dimensions do not prove membership in a census group.',
                             'No native/D3D calls, AddRef, suspension or memory writes are made.'])


def collect(pid, sizes=(768, 1024), hud=False, layers=False, icon_atlases=False):
    kernel, psapi = c.WinDLL('kernel32', use_last_error=True), c.WinDLL('psapi', use_last_error=True)
    kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    kernel.OpenProcess.restype = w.HANDLE
    kernel.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    kernel.CloseHandle.argtypes = [w.HANDLE]
    psapi.EnumProcessModulesEx.argtypes = [w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), w.DWORD]
    psapi.GetModuleFileNameExW.argtypes = [w.HANDLE, w.HMODULE, w.LPWSTR, w.DWORD]
    process = kernel.OpenProcess(0x0400 | 0x0010, False, pid)
    if not process:
        raise c.WinError(c.get_last_error())

    def read(address, size):
        if size == 0:
            return b''
        data, copied = c.create_string_buffer(size), c.c_size_t()
        if not kernel.ReadProcessMemory(process, address, data, size, c.byref(copied)) or copied.value != size:
            raise c.WinError(c.get_last_error())
        return data.raw

    try:
        handles, needed = (w.HMODULE * 2048)(), w.DWORD()
        if not psapi.EnumProcessModulesEx(process, handles, c.sizeof(handles), c.byref(needed), 3):
            raise c.WinError(c.get_last_error())
        if needed.value > c.sizeof(handles):
            raise ValueError('Module inventory exceeded bound')
        base = None; plugin_base = None
        for handle in handles[:needed.value // c.sizeof(w.HMODULE)]:
            path = c.create_unicode_buffer(32768)
            if not psapi.GetModuleFileNameExW(process, handle, path, len(path)):
                raise c.WinError(c.get_last_error())
            if Path(path.value).name.lower() == 'cyberpunk2077.exe':
                base, executable = int(handle), path.value
            if Path(path.value).name.lower() == 'cyberpunkvrport.dll':
                plugin_base = int(handle)
            if base is not None and (not hud or plugin_base is not None):
                break
        if base is None:
            raise ValueError('Cyberpunk2077.exe is not loaded in this PID')
        dos = read(base, 64)
        pe = struct.unpack_from('<I', dos, 0x3C)[0]
        if dos[:2] != b'MZ' or not 64 <= pe <= 1024 * 1024:
            raise ValueError('Invalid mapped PE')
        header = read(base + pe, 104)
        timestamp, image_size = struct.unpack_from('<I', header, 8)[0], struct.unpack_from('<I', header, 80)[0]
        if header[:4] != b'PE\0\0' or timestamp != TIMESTAMP or image_size != IMAGE_SIZE:
            raise ValueError(f'Offsets only verified for game 2.5 PE {TIMESTAMP}/{IMAGE_SIZE:#x}, got {timestamp}/{image_size:#x}')
        for rva, expected in CODE_SPANS.items():
            code = bytes.fromhex(expected)
            if read(base + rva, len(code)) != code:
                raise ValueError(f'Native texture code provenance mismatch at RVA {rva:#x}; no pool scan performed')
        report = snapshot(read, base, sizes)
        if hud:
            if plugin_base is None:
                raise ValueError('CyberpunkVRPort.dll is not loaded')
            report['hud'] = hud_snapshot(read, base, mapped_export(read, plugin_base, 'CyberpunkVR_HudLayerAddress'))
        if layers:
            report['layers'] = layers_snapshot(read, base)
        if icon_atlases:
            report['iconAtlases'] = icon_atlases_snapshot(read, base, report['rows'])
        report.update(pid=pid, timestamp=time.time(), executable=executable, moduleBase=hex(base),
                      peTimestamp=timestamp, imageSize=image_size, verifiedCodeSpans=list(map(hex, CODE_SPANS)))
        return report
    finally:
        kernel.CloseHandle(process)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--sizes', type=int, nargs='+', default=[768, 1024])
    parser.add_argument('--hud', action='store_true', help='Also read current HUD entry/window/texture identities using the mapped plugin export')
    parser.add_argument('--layers', action='store_true', help='Also read bounded native InkSystem current layer/window/texture identities')
    parser.add_argument('--icon-atlases', action='store_true', help='Join typed IconAtlas pages through TextureAtlas strong handles to DynamicTexture and native resource IDs')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    report = collect(args.pid, args.sizes, args.hud, args.layers, args.icon_atlases)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: report[key] for key in ('pid', 'backingCount', 'poolStable', 'ownerStable', 'backingListStable', 'errors')}, indent=2))
    print('Matching native rows:', len(report['rows']))
    if args.hud:
        print(json.dumps({key: report['hud'][key] for key in ('layer', 'count', 'stable', 'errors')}, indent=2))
    if args.layers:
        print(json.dumps({key: report['layers'][key] for key in ('system', 'managerCount', 'stable', 'errors')}, indent=2))
    if args.icon_atlases:
        print(json.dumps({key: report['iconAtlases'][key] for key in ('manager', 'registryCount', 'uniqueAtlasCount', 'uniquePageCount', 'stable', 'errors')}, indent=2))
