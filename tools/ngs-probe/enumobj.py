#!/usr/bin/env python3
"""Enumerate object-manager directories (\\Device, \\Driver, \\FileSystem\\Filters, ...).

No WinObj needed. Uses NtOpenDirectoryObject + NtQueryDirectoryObject. Run elevated for
the full view; non-elevated still shows \\Device.

    python enumobj.py [dir ...]
"""

import ctypes
import ctypes.wintypes as w
import sys

ntdll = ctypes.WinDLL("ntdll", use_last_error=True)
STATUS_MORE_ENTRIES = 0x00000105
STATUS_NO_MORE_FILES = 0x80000006
STATUS_NO_MORE_ENTRIES = 0x8000001A
STATUS_BUFFER_TOO_SMALL = 0xC0000023
STATUS_BUFFER_OVERFLOW = 0x80000005


class UNICODE_STRING(ctypes.Structure):
    _fields_ = [("Length", w.USHORT), ("MaximumLength", w.USHORT), ("Buffer", w.LPWSTR)]


class OBJECT_ATTRIBUTES(ctypes.Structure):
    _fields_ = [("Length", w.ULONG), ("RootDirectory", w.HANDLE),
                ("ObjectName", ctypes.POINTER(UNICODE_STRING)),
                ("Attributes", w.ULONG), ("SecurityDescriptor", ctypes.c_void_p),
                ("SecurityQualityOfService", ctypes.c_void_p)]


class _RAW_UNICODE_STRING(ctypes.Structure):
    _fields_ = [("Length", w.USHORT), ("MaximumLength", w.USHORT), ("Buffer", ctypes.c_void_p)]


class DIRECTORY_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [("ObjectName", _RAW_UNICODE_STRING), ("ObjectTypeName", _RAW_UNICODE_STRING),
                ("Reserved", ctypes.c_void_p)]


NTSTATUS = ctypes.c_long

ntdll.NtOpenDirectoryObject.argtypes = [ctypes.POINTER(w.HANDLE), w.ULONG,
                                        ctypes.POINTER(OBJECT_ATTRIBUTES)]
ntdll.NtOpenDirectoryObject.restype = NTSTATUS
ntdll.NtQueryDirectoryObject.argtypes = [w.HANDLE, ctypes.c_void_p, w.ULONG, w.BOOLEAN,
                                         w.BOOLEAN, ctypes.POINTER(w.ULONG),
                                         ctypes.POINTER(w.ULONG)]
ntdll.NtQueryDirectoryObject.restype = NTSTATUS
ntdll.NtClose.argtypes = [w.HANDLE]


def init_attr(name: str):
    us = UNICODE_STRING()
    buf = ctypes.create_unicode_buffer(name)
    us.Buffer = ctypes.cast(buf, w.LPWSTR)
    us.Length = len(name) * 2
    us.MaximumLength = us.Length + 2
    oa = OBJECT_ATTRIBUTES(ctypes.sizeof(OBJECT_ATTRIBUTES), None, ctypes.pointer(us), 0x40, None, None)
    return oa, buf


def list_dir(path: str):
    oa, buf = init_attr(path)
    h = w.HANDLE()
    st = ntdll.NtOpenDirectoryObject(ctypes.byref(h), 0x1, ctypes.byref(oa))
    if st < 0:
        print(f"{path}: NtOpenDirectoryObject status=0x{st & 0xffffffff:08x}")
        return []
    names = []
    ctx = w.ULONG(0)
    size = 0x20000
    restart = True
    guard = 0
    while guard < 100000:
        guard += 1
        out = ctypes.create_string_buffer(size)
        ret = w.ULONG(0)
        st = ntdll.NtQueryDirectoryObject(h, out, size, False, restart,
                                          ctypes.byref(ctx), ctypes.byref(ret))
        restart = False
        if st in (STATUS_MORE_ENTRIES, 0):
            base = ctypes.addressof(out)
            off = 0
            while off + 0x20 <= ret.value:
                ent = DIRECTORY_BASIC_INFORMATION.from_buffer(out, off)
                ln, tn = ent.ObjectName.Length, ent.ObjectTypeName.Length
                p = ent.ObjectName.Buffer
                if ln and p and base <= p < base + ret.value:
                    names.append(ctypes.wstring_at(p, ln // 2))
                off += 0x20
            if st == 0:
                break
        elif st in (STATUS_BUFFER_TOO_SMALL, STATUS_BUFFER_OVERFLOW):
            size *= 2
            ctx.value = 0
            restart = True
        elif st in (STATUS_NO_MORE_FILES, STATUS_NO_MORE_ENTRIES):
            break
        else:
            print(f"{path}: query status=0x{st & 0xffffffff:08x}")
            break
    ntdll.NtClose(h)
    return names


def main():
    dirs = sys.argv[1:] or [r"\Device", r"\Driver", r"\FileSystem\Filters",
                            r"\BaseNamedObjects", r"\Driver\BlackCat1"]
    for d in dirs:
        print(f"== {d}")
        for n in list_dir(d):
            print(f"  {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
