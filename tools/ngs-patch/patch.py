#!/usr/bin/env python3
"""Skip NGS-X initialisation in MapleStory Classic World (branch B of the bypass).

The client calls the GRAP/NGS entry point four times during start-up. Each call is a
small function `sub rsp,28h; mov ecx,17h; call [grap64!ordinal]` that fast-fails
(`int 29h`) when NGS is absent. Turning the leading `sub rsp,28h` into `ret` makes all
four return success without ever touching NGS, so neither NGService nor BlackCat64.sys
is started. The first NGS relay address is also pointed at 127.0.0.1 so any leftover
communicator fails fast instead of timing out.

This is exactly the patch that exists as MapleStory.patched.bak on this machine; it is
reproduced here by signature so it survives game updates.

    python patch.py --status  <exe>
    python patch.py --apply   <exe>     # backup -> patch -> recompute PE checksum
    python patch.py --restore <exe>

SHA256 allowlist lives in known_builds.json. Unknown builds are refused (fail closed).
"""

import argparse
import hashlib
import json
import os
import re
import sys
import tempfile
from pathlib import Path

try:
    import pefile
except ImportError:
    sys.exit("pefile is required (python -m pip install pefile)")

HERE = Path(__file__).resolve().parent
KNOWN = json.loads((HERE / "known_builds.json").read_text())

# `sub rsp,28h; mov ecx,17h; call qword ptr [rip+..]`  (the three NGS init calls)
INIT_SITE = re.compile(rb"\x48\x83\xec\x28\xb9\x17\x00\x00\x00\xff\x15")
INIT_PATCHED = re.compile(rb"\xc3\x83\xec\x28\xb9\x17\x00\x00\x00\xff\x15")
# the companion function: sub rsp,28h; call ..; test rax,rax; ... mov ecx,16h; call ..;
# test byte [rip+..],2; jz +2a; mov ecx,17h; call [rip+..]
COMPANION_SITE = re.compile(
    rb"\x48\x83\xec\x28\xe8....\x48\x85\xc0\x74.\xb9\x16\x00\x00\x00\xe8...."
    rb"\xf6\x05....\x02\x74.\xb9\x17\x00\x00\x00\xff\x15",
    re.S,
)
COMPANION_PATCHED = re.compile(
    rb"\xc3\x83\xec\x28\xe8....\x48\x85\xc0\x74.\xb9\x16\x00\x00\x00\xe8...."
    rb"\xf6\x05....\x02\x74.\xb9\x17\x00\x00\x00\xff\x15",
    re.S,
)

RELAY_OLD = b"10.9.2.131\x00\x00\x00\x00\x00\x00"
RELAY_SLOT = 16
RELAY_NEW = b"127.0.0.1"
RELAY_PATCHED = RELAY_NEW.ljust(RELAY_SLOT, b"\x00")
RET = 0xC3

EXPECTED_INIT = 3
EXPECTED_COMPANION = 1


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def known_build(data: bytes, allow_patched: bool = False):
    h = sha256(data)
    for b in KNOWN["builds"]:
        if b["size"] != len(data):
            continue
        if b["sha256"] == h:
            return b, False
        if allow_patched and b.get("patched_sha256") == h:
            return b, True
    return None, False


def find_sites(data: bytes):
    init = [(m.start(), False) for m in INIT_SITE.finditer(data)]
    init += [(m.start(), True) for m in INIT_PATCHED.finditer(data)]
    comp = [(m.start(), False) for m in COMPANION_SITE.finditer(data)]
    comp += [(m.start(), True) for m in COMPANION_PATCHED.finditer(data)]
    init.sort()
    comp.sort()
    relay = data.find(RELAY_OLD)
    if relay >= 0:
        return init, comp, relay, False
    relay = data.find(RELAY_PATCHED)
    return init, comp, relay, relay >= 0


def status(data: bytes, path: Path):
    init, comp, relay, relay_patched = find_sites(data)
    b, is_patched = known_build(data, allow_patched=True)
    patched = sum(1 for _, p in init + comp if p)
    print(f"{path}  size={len(data)}  sha256={sha256(data)}")
    print(f"  build:   {b['build'] if b else 'UNKNOWN (not in allowlist)'}")
    print(f"  inits:   {len(init)} at {[hex(o) for o, _ in init]}  (expected {EXPECTED_INIT})")
    print(f"  companion: {len(comp)} at {[hex(o) for o, _ in comp]} (expected {EXPECTED_COMPANION})")
    print(f"  relay:   {'found at ' + hex(relay) + (' (patched)' if relay_patched else '')
                             if relay >= 0 else 'not found'}")
    print(f"  state:   {'patched' if patched else 'clean'} ({patched} sites ret)")
    return 0 if (len(init) == EXPECTED_INIT and len(comp) == EXPECTED_COMPANION
                 and relay >= 0) else 1


def apply(path: Path, dry_run: bool):
    data = bytearray(path.read_bytes())
    b, _ = known_build(data, allow_patched=True)
    if not b:
        sys.exit(f"refusing unknown build: {path} sha256={sha256(data)}")

    init, comp, relay, relay_patched = find_sites(data)
    if len(init) != EXPECTED_INIT or len(comp) != EXPECTED_COMPANION:
        sys.exit(f"signature mismatch: {len(init)} init sites, {len(comp)} companion "
                 f"(expected {EXPECTED_INIT}/{EXPECTED_COMPANION})")
    if relay < 0:
        sys.exit("NGS relay list not found")

    bak = path.with_suffix(path.suffix + ".neon.bak")
    changes = []
    for off, already in init + comp:
        if not already:
            if data[off] != 0x48:
                sys.exit(f"unexpected byte {data[off]:#x} at {off:#x}")
            data[off] = RET
            changes.append(off)
    if not relay_patched:
        data[relay:relay + RELAY_SLOT] = RELAY_PATCHED
        changes.append(relay)

    # keep the PE checksum valid, like the reference patch did
    pe = pefile.PE(data=bytes(data), fast_load=True)
    chk_off = pe.OPTIONAL_HEADER.get_field_absolute_offset("CheckSum")
    data[chk_off:chk_off + 4] = b"\x00\x00\x00\x00"
    pe = pefile.PE(data=bytes(data), fast_load=True)
    data[chk_off:chk_off + 4] = pe.generate_checksum().to_bytes(4, "little")

    print(f"{path}: {len(changes)} change(s)" + (" (already patched)" if not changes else ""))
    for off, _ in init + comp:
        print(f"  ret      @ {off:#x}")
    print(f"  relay    @ {relay:#x}: {RELAY_OLD.split(chr(0).encode())[0].decode()} -> "
          f"{RELAY_NEW.decode()}")
    print(f"  checksum @ {chk_off:#x} -> {pe.generate_checksum():#x}")

    if dry_run:
        return 0
    if not bak.exists():
        bak.write_bytes(path.read_bytes())
        print(f"  backup   {bak}")
    fd, name = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    os.close(fd)
    tmp = Path(name)
    tmp.write_bytes(bytes(data))
    os.replace(tmp, path)
    print(f"  wrote    sha256={sha256(bytes(data))}")
    return 0


def restore(path: Path):
    bak = path.with_suffix(path.suffix + ".neon.bak")
    if not bak.exists():
        if known_build(path.read_bytes())[0]:
            print(f"{path}: already clean, no backup")
            return 0
        sys.exit(f"no backup and not a known clean build: {bak}")
    data = bak.read_bytes()
    if not known_build(data)[0]:
        sys.exit(f"backup is not a known clean build: {bak}")
    fd, name = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    os.close(fd)
    tmp = Path(name)
    tmp.write_bytes(data)
    os.replace(tmp, path)
    print(f"restored {path} from {bak} (sha256={sha256(data)})")
    return 0


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument("--status", action="store_true")
    g.add_argument("--apply", action="store_true")
    g.add_argument("--restore", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("exe", type=Path)
    a = ap.parse_args()
    if not a.exe.is_file():
        sys.exit(f"not a file: {a.exe}")
    if a.status:
        return status(a.exe.read_bytes(), a.exe)
    if a.restore:
        return restore(a.exe)
    return apply(a.exe, a.dry_run)


if __name__ == "__main__":
    sys.exit(main())
