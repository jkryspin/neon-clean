#!/usr/bin/env python3
"""Minimal WZ reader for the client's own data files, to turn id fields in packets into real names.

  neon-calib/tools/wzread.py list                      # top-level .img files in String_000.wz
  neon-calib/tools/wzread.py names Mob.img             # id -> name for every entry
  neon-calib/tools/wzread.py lookup Mob.img 1210102    # one id
  neon-calib/tools/wzread.py --wz PATH ...             # a different .wz

Why this exists: several registry fields hold ids whose MEANING is ambiguous from traffic alone - is the u32 in
0x02b9 a skill id or an item id? is the u32@130 in 0x02af really a mob template? The client ships the answer in
Data/String/String_000.wz (mob, item, skill and map name tables). Checking an id against the client's own data
is an independent source, costs nothing, and needs no reverse engineering of the executable.

Format notes (verified against this client, Classic World): classic PKG1 WZ. Strings use the plain
XOR-with-incrementing-mask scheme with a ZERO AES key (confirmed by hand: the first directory entry decodes to
"Cash.img"), so no region key is needed. Offsets use the standard version-hash scrambling.
"""
import struct, sys, os

DEFAULT_WZ = ("/home/john/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/"
              "NexonLauncherSetup-91dd6ddf36/drive_c/Nexon/Library/maplestorycw/appdata/Data/"
              "String/String_000.wz")


class Wz:
    def __init__(self, path):
        self.d = open(path, "rb").read()
        assert self.d[:4] == b"PKG1", "not a PKG1 wz file"
        self.fsize = struct.unpack_from("<Q", self.d, 4)[0]
        self.fstart = struct.unpack_from("<I", self.d, 12)[0]
        self.p = self.fstart
        self.img_base = 0
        enc_ver = self.u16()
        self.version_hash = self._find_version_hash(enc_ver)
        if self.version_hash is None:
            raise SystemExit(f"could not resolve the version hash for encrypted version {enc_ver:#x}")

    # ---- primitives -------------------------------------------------------
    def u8(self):
        v = self.d[self.p]; self.p += 1; return v
    def i8(self):
        v = struct.unpack_from("<b", self.d, self.p)[0]; self.p += 1; return v
    def u16(self):
        v = struct.unpack_from("<H", self.d, self.p)[0]; self.p += 2; return v
    def i32(self):
        v = struct.unpack_from("<i", self.d, self.p)[0]; self.p += 4; return v
    def u32(self):
        v = struct.unpack_from("<I", self.d, self.p)[0]; self.p += 4; return v
    def i64(self):
        v = struct.unpack_from("<q", self.d, self.p)[0]; self.p += 8; return v
    def f32(self):
        v = struct.unpack_from("<f", self.d, self.p)[0]; self.p += 4; return v
    def f64(self):
        v = struct.unpack_from("<d", self.d, self.p)[0]; self.p += 8; return v

    def cint(self):
        b = self.i8()
        return self.i32() if b == -128 else b

    def clong(self):
        b = self.i8()
        return self.i64() if b == -128 else b

    def wstr(self):
        """WZ string: negative length = ASCII, positive = UTF-16LE. XOR mask increments; AES key is zero here."""
        n = self.i8()
        if n == 0:
            return ""
        if n < 0:
            n = self.i32() if n == -128 else -n
            out, mask = bytearray(), 0xAA
            for i in range(n):
                out.append(self.d[self.p + i] ^ mask); mask = (mask + 1) & 0xFF
            self.p += n
            return out.decode("latin1")
        n = self.i32() if n == 127 else n
        out, mask = [], 0xAAAA
        for i in range(n):
            c = struct.unpack_from("<H", self.d, self.p + 2 * i)[0] ^ mask
            out.append(chr(c)); mask = (mask + 1) & 0xFFFF
        self.p += 2 * n
        return "".join(out)

    def str_at(self, off):
        save = self.p; self.p = off; s = self.wstr(); self.p = save; return s

    def prop_str(self):
        """A property name/string: inline (0x00/0x0B/0x73), or a back-reference to a string stored earlier in
        the SAME image (0x01/0x1B/0x72) - the dedup table. Reference offsets are relative to the image's own
        start, not the file start, which is why img_base is tracked."""
        t = self.u8()
        if t in (0x00, 0x0B, 0x73):
            return self.wstr()
        if t in (0x01, 0x1B, 0x72):
            return self.str_at(self.img_base + self.u32())
        raise ValueError(f"unexpected string tag {t:#x} at {self.p-1:#x}")

    # ---- header / offsets -------------------------------------------------
    @staticmethod
    def _hash_for(version):
        s = 0
        for ch in str(version):
            s = (s * 32 + ord(ch) + 1) & 0xFFFFFFFF
        enc = 0xFF ^ ((s >> 24) & 0xFF) ^ ((s >> 16) & 0xFF) ^ ((s >> 8) & 0xFF) ^ (s & 0xFF)
        return s, enc

    def _find_version_hash(self, enc_ver):
        """The stored encrypted version is only 8 bits, so ~1 in 256 version numbers matches it. Pick the
        candidate whose offset descrambling actually produces in-range offsets for the whole root directory -
        that is the real version. (Taking the first match gives nonsense offsets.)"""
        cands = [ (v, s) for v in range(0, 2000) for s, enc in [self._hash_for(v)] if enc == enc_ver ]
        for v, s in cands:
            self.version, self.version_hash = v, s
            try:
                save = self.p
                ents = self.dir_entries(self.fstart + 2)
                self.p = save
                if ents and all(0 < e["off"] < len(self.d) for e in ents):
                    return s
            except Exception:
                pass
        return None

    def offset(self):
        """Standard WZ offset descrambling."""
        off = ((self.p - self.fstart) ^ 0xFFFFFFFF) & 0xFFFFFFFF
        off = (off * self.version_hash) & 0xFFFFFFFF
        off = (off - 0x581C3F6D) & 0xFFFFFFFF
        off = ((off << (off & 0x1F)) | (off >> (32 - (off & 0x1F)))) & 0xFFFFFFFF  # rotl
        off = (off ^ self.u32()) & 0xFFFFFFFF
        return (off + self.fstart * 2) & 0xFFFFFFFF

    # ---- directory --------------------------------------------------------
    def dir_entries(self, at=None):
        if at is not None:
            self.p = at
        out = []
        for _ in range(self.cint()):
            t = self.u8()
            if t == 1:
                self.p += 10
                continue
            if t in (2, 3, 4):
                name = self.str_at(self.fstart + self.i32()) if t == 2 else self.wstr()
                if t == 2:
                    t = self.u8()
                size, _chk = self.cint(), self.cint()
                off = self.offset()
                out.append({"name": name, "type": t, "size": size, "off": off})
            else:
                raise ValueError(f"bad dir entry type {t} at {self.p-1:#x}")
        return out

    # ---- property (.img) --------------------------------------------------
    def read_img(self, off):
        self.img_base = off
        self.p = off
        if self.prop_str() != "Property":
            return {}
        self.u16()
        return self._props()

    def _props(self):
        out = {}
        for _ in range(self.cint()):
            name = self.prop_str()
            t = self.u8()
            if t == 0:
                out[name] = None
            elif t in (2, 11):
                out[name] = self.u16()
            elif t in (3, 19):
                out[name] = self.cint()
            elif t in (4, 20):
                out[name] = self.clong()
            elif t == 5:
                out[name] = self.f64()
            elif t == 8:
                out[name] = self.prop_str()
            elif t == 9:
                end = self.u32() + self.p
                out[name] = self._extended()
                self.p = end
            else:
                raise ValueError(f"bad prop type {t} for {name!r} at {self.p-1:#x}")
        return out

    def _extended(self):
        kind = self.prop_str()
        if kind == "Property":
            self.u16()
            return self._props()
        return f"<{kind}>"


def main():
    args = sys.argv[1:]
    path = DEFAULT_WZ
    if args and args[0] == "--wz":
        path = args[1]; args = args[2:]
    if not args:
        print(__doc__); return
    if not os.path.exists(path):
        raise SystemExit(f"no such wz: {path}")
    w = Wz(path)
    root = w.dir_entries(w.p)
    cmd = args[0]
    if cmd == "list":
        print(f"{path}\n  version={w.version} hash={w.version_hash:#x} fstart={w.fstart:#x}")
        for e in root:
            print(f"  {e['name']:<28} type={e['type']} size={e['size']:>8} off={e['off']:#x}")
        return
    target = args[1]
    ent = next((e for e in root if e["name"].lower() == target.lower()), None)
    if not ent:
        raise SystemExit(f"{target} not in this wz (try: list)")
    props = w.read_img(ent["off"])
    if cmd == "names":
        n = 0
        for k, v in props.items():
            if isinstance(v, dict):
                nm = v.get("name")
                if nm:
                    print(f"{k}\t{nm}"); n += 1
        print(f"# {n} named entries in {target}", file=sys.stderr)
    elif cmd == "lookup":
        key = args[2]
        v = props.get(key)
        if isinstance(v, dict):
            print(f"{key}: " + ", ".join(f"{a}={b!r}" for a, b in v.items() if not isinstance(b, dict)))
        else:
            print(f"{key}: not found in {target}")


if __name__ == "__main__":
    main()
