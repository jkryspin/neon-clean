#!/usr/bin/env python3
"""Extract map geometry (footholds, ladders/ropes, portals, bounds) from the client's
Map*.wz files (PKG1 / unencrypted strings, version-hashed offsets).

  wzmap.py <Map0_000.wz> <outdir>            # every 9-digit map image
  wzmap.py <Map0_000.wz> <outdir> 010001010  # only those ids

Output: <outdir>/<mapid>.json  (mapid without the leading zero, e.g. 10001010)
"""
import json, os, struct, sys


class R:
    def __init__(self, data, fstart):
        self.d, self.p, self.fstart = data, 0, fstart

    def u8(self): v = self.d[self.p]; self.p += 1; return v
    def s8(self): v = struct.unpack_from('<b', self.d, self.p)[0]; self.p += 1; return v
    def i16(self): v = struct.unpack_from('<h', self.d, self.p)[0]; self.p += 2; return v
    def u16(self): v = struct.unpack_from('<H', self.d, self.p)[0]; self.p += 2; return v
    def i32(self): v = struct.unpack_from('<i', self.d, self.p)[0]; self.p += 4; return v
    def u32(self): v = struct.unpack_from('<I', self.d, self.p)[0]; self.p += 4; return v
    def f32(self): v = struct.unpack_from('<f', self.d, self.p)[0]; self.p += 4; return v

    def cint(self):
        b = self.s8()
        return self.i32() if b == -128 else b

    def string(self):
        n = self.s8()
        if n > 0:  # unicode, xor 0xAAAA
            if n == 127: n = self.i32()
            out, mask = [], 0xAAAA
            for _ in range(n):
                out.append(chr(self.u16() ^ mask)); mask = (mask + 1) & 0xFFFF
            return ''.join(out)
        if n == -128: n = self.i32()
        else: n = -n
        mask, out = 0xAA, []
        for _ in range(n):
            out.append(chr(self.u8() ^ mask)); mask = (mask + 1) & 0xFF
        return ''.join(out)

    def strblock(self, base):
        t = self.u8()
        if t in (0x00, 0x73): return self.string()
        if t in (0x01, 0x1B):
            off = self.u32(); save = self.p
            self.p = base + off; s = self.string(); self.p = save
            return s
        raise ValueError('bad string block %#x at %d' % (t, self.p))


def vhash(ver):
    h = 0
    for c in str(ver): h = (h * 32 + ord(c) + 1) & 0xFFFFFFFF
    return h


def ver_check(h): return 0xFF ^ (h >> 24 & 255) ^ (h >> 16 & 255) ^ (h >> 8 & 255) ^ (h & 255)


def rol(v, n): n &= 31; return ((v << n) | (v >> (32 - n))) & 0xFFFFFFFF


def read_offset(r, h):
    off = ((r.p - r.fstart) ^ 0xFFFFFFFF) & 0xFFFFFFFF
    off = (off * h) & 0xFFFFFFFF
    off = (off - 0x581C3F6D) & 0xFFFFFFFF
    off = rol(off, off)
    off ^= r.u32()
    return (off + r.fstart * 2) & 0xFFFFFFFF


def read_dir(r, h, prefix, out, limit):
    n = r.cint()
    if not 0 <= n < 100000: raise ValueError('bad dir count')
    subs = []
    for _ in range(n):
        t = r.u8()
        if t == 1: r.p += 10; continue
        if t == 2:
            o = r.i32(); save = r.p; r.p = r.fstart + o; t = r.u8(); name = r.string(); r.p = save
        elif t in (3, 4): name = r.string()
        else: raise ValueError('bad entry type %d' % t)
        size = r.cint(); r.cint(); off = read_offset(r, h)
        if off >= limit: raise ValueError('offset out of range')
        (subs if t == 3 else out.setdefault('imgs', [])).append((prefix + name, off, size) if t == 3 else (prefix + name, off, size))
    for path, off, _ in subs:
        r.p = off
        read_dir(r, h, path + '/', out, limit)


def open_wz(path):
    data = open(path, 'rb').read()
    assert data[:4] == b'PKG1', 'not a PKG1 wz'
    fstart = struct.unpack_from('<I', data, 12)[0]
    enc = struct.unpack_from('<H', data, fstart)[0]
    for ver in range(1, 3000):
        h = vhash(ver)
        if ver_check(h) != enc: continue
        r = R(data, fstart); r.p = fstart + 2; out = {}
        try:
            read_dir(r, h, '', out, len(data))
            return r, out['imgs']
        except Exception:
            continue
    raise SystemExit('no version hash fits')


def props(r, base):
    r.u16()
    out = {}
    for _ in range(r.cint()):
        name = r.strblock(base); t = r.u8()
        if t == 0: v = None
        elif t in (2, 0x0B): v = r.i16()
        elif t == 3: v = r.cint()
        elif t == 4: v = r.f32() if r.u8() == 0x80 else 0.0
        elif t == 5: v = struct.unpack_from('<d', r.d, r.p)[0]; r.p += 8
        elif t == 8: v = r.strblock(base)
        elif t == 9:
            size = r.u32(); end = r.p + size; kind = r.strblock(base)
            if kind == 'Property': v = props(r, base)
            elif kind == 'Shape2D#Vector2D': v = [r.cint(), r.cint()]
            else: v = None
            r.p = end
        else: raise ValueError('bad prop type %d' % t)
        out[name] = v
    return out


def parse_img(r, off):
    r.p = off
    t = r.u8()
    if t != 0x73 or r.string() != 'Property': raise ValueError('not a property image')
    return props(r, off)


def geometry(img):
    fh = []
    for layer in (img.get('foothold') or {}).values():
        for group in layer.values():
            for k, f in group.items():
                if isinstance(f, dict):
                    fh.append({'id': int(k), 'x1': f['x1'], 'y1': f['y1'], 'x2': f['x2'], 'y2': f['y2'],
                               'prev': f.get('prev', 0), 'next': f.get('next', 0),
                               'no_drop': bool(f.get('forbidFallDown', 0))})
    lad = [{'x': l['x'], 'y1': l['y1'], 'y2': l['y2'], 'ladder': bool(l.get('l', 0))}
           for l in (img.get('ladderRope') or {}).values() if isinstance(l, dict)]
    por = [{'name': p.get('pn'), 'type': p.get('pt'), 'x': p.get('x'), 'y': p.get('y'),
            'to_map': p.get('tm'), 'to_portal': p.get('tn')}
           for p in (img.get('portal') or {}).values() if isinstance(p, dict)]
    mobs = {}
    for l in (img.get('life') or {}).values():
        if isinstance(l, dict) and l.get('type') == 'm' and str(l.get('id', '')).isdigit():
            mid = int(l['id'])
            mobs[mid] = mobs.get(mid, 0) + 1
    # Where each mob spawns and how far it patrols (rx0..rx1 along its platform, cy = the platform height):
    # lets the bot name a mob the network first reports by movement, without its template id.
    spawns = [{'id': int(l['id']), 'x': l.get('x'), 'y': l.get('cy', l.get('y')), 'fh': l.get('fh'),
               'rx0': l.get('rx0'), 'rx1': l.get('rx1')}
              for l in (img.get('life') or {}).values()
              if isinstance(l, dict) and l.get('type') == 'm' and str(l.get('id', '')).isdigit()]
    npcs = [{'id': int(l['id']), 'x': l.get('x'), 'y': l.get('y'), 'fh': l.get('fh')}
            for l in (img.get('life') or {}).values()
            if isinstance(l, dict) and l.get('type') == 'n' and str(l.get('id', '')).isdigit()]
    info = img.get('info') or {}
    bounds = {k: info.get(k) for k in ('VRLeft', 'VRRight', 'VRTop', 'VRBottom') if info.get(k) is not None}
    return {'footholds': fh, 'ladders': lad, 'portals': por, 'bounds': bounds, 'npcs': npcs, 'mobs': {str(k): v for k, v in mobs.items()}, 'mob_spawns': spawns}


def main():
    wz, outdir, *ids = sys.argv[1:]
    os.makedirs(outdir, exist_ok=True)
    r, imgs = open_wz(wz)
    n = 0
    for path, off, _ in imgs:
        base = os.path.basename(path)
        stem = base[:-4]
        if not (base.endswith('.img') and len(stem) == 9 and stem.isdigit()): continue
        if ids and stem not in ids: continue
        try: g = geometry(parse_img(r, off))
        except Exception as e:
            print('skip', path, e, file=sys.stderr); continue
        json.dump(g, open(os.path.join(outdir, '%d.json' % int(stem)), 'w'), separators=(',', ':'))
        n += 1
    print('wrote', n, 'maps')


if __name__ == '__main__':
    main()
