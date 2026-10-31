#!/usr/bin/env python3
"""Measure what an ordinary process can actually do to another process.

If NGS registers an ObRegisterCallbacks PreOperation hook, OpenProcess is denied or the
granted access is stripped. Rather than trusting one mask, exercise each capability
separately (the way a bot/injector would) and report ok/fail. Run once with the driver
stopped and once with it loaded; the delta is the protection surface.

    python handleaccess.py [pid]        # default: spawns notepad and probes it
"""

import ctypes
import ctypes.wintypes as w
import subprocess
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
ntdll = ctypes.WinDLL("ntdll", use_last_error=True)

k32.OpenProcess.restype = w.HANDLE
k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.CloseHandle.argtypes = [w.HANDLE]
k32.ReadProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                  ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k32.WriteProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                   ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k32.VirtualAllocEx.restype = ctypes.c_void_p
k32.VirtualAllocEx.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_size_t, w.DWORD, w.DWORD]
k32.VirtualFreeEx.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_size_t, w.DWORD]
k32.CreateRemoteThread.restype = w.HANDLE
k32.CreateRemoteThread.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_size_t,
                                   ctypes.c_void_p, ctypes.c_void_p, w.DWORD,
                                   ctypes.POINTER(w.DWORD)]
k32.GetExitCodeProcess.argtypes = [w.HANDLE, ctypes.POINTER(w.DWORD)]
k32.SetProcessWorkingSetSize.argtypes = [w.HANDLE, ctypes.c_size_t, ctypes.c_size_t]
k32.GetModuleHandleW.restype = ctypes.c_void_p
k32.GetModuleHandleW.argtypes = [w.LPCWSTR]
k32.GetProcAddress.restype = ctypes.c_void_p
k32.GetProcAddress.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
k32.OpenThread.restype = w.HANDLE
k32.OpenThread.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.SuspendThread.argtypes = [w.HANDLE]
k32.ResumeThread.argtypes = [w.HANDLE]
k32.CreateToolhelp32Snapshot.restype = w.HANDLE
k32.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]


class PROCESS_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [("Reserved1", ctypes.c_void_p), ("PebBaseAddress", ctypes.c_void_p),
                ("Reserved2", ctypes.c_void_p * 2), ("UniqueProcessId", ctypes.c_void_p),
                ("Reserved3", ctypes.c_void_p)]


class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", w.DWORD), ("cntUsage", w.DWORD), ("th32ThreadID", w.DWORD),
                ("th32OwnerProcessID", w.DWORD), ("tpBasePri", w.LONG),
                ("tpDeltaPri", w.LONG), ("dwFlags", w.DWORD)]


ntdll.NtQueryInformationProcess.argtypes = [w.HANDLE, ctypes.c_uint, ctypes.c_void_p,
                                            w.ULONG, ctypes.POINTER(w.ULONG)]

VM_READ, VM_WRITE, VM_OP = 0x0010, 0x0020, 0x0008
CREATE_THREAD, QUERY_INFO, SUSPEND, SET_INFO = 0x0002, 0x0400, 0x0800, 0x0200
SET_QUOTA = 0x0100
PROCESS_ALL = 0x001FFFFF


def remote_peb(pid):
    h = k32.OpenProcess(QUERY_INFO, False, pid)
    if not h:
        return None
    pbi = PROCESS_BASIC_INFORMATION()
    ret = w.ULONG(0)
    st = ntdll.NtQueryInformationProcess(h, 0, ctypes.byref(pbi), ctypes.sizeof(pbi),
                                         ctypes.byref(ret))
    k32.CloseHandle(h)
    return pbi.PebBaseAddress if st == 0 else None


def first_thread(pid):
    snap = k32.CreateToolhelp32Snapshot(0x4, 0)  # TH32CS_SNAPTHREAD
    te = THREADENTRY32()
    te.dwSize = ctypes.sizeof(te)
    tid = 0
    ok = k32.Thread32First(snap, ctypes.byref(te))
    while ok:
        if te.th32OwnerProcessID == pid:
            tid = te.th32ThreadID
            break
        ok = k32.Thread32Next(snap, ctypes.byref(te))
    k32.CloseHandle(snap)
    return tid


def t_read(pid, peb):
    h = k32.OpenProcess(VM_READ, False, pid)
    if not h:
        return None, f"open err={ctypes.get_last_error()}"
    buf = ctypes.create_string_buffer(8)
    got = ctypes.c_size_t(0)
    ok = k32.ReadProcessMemory(h, ctypes.c_void_p((peb or 0x7FFE0000) + 0x10), buf, 8,
                               ctypes.byref(got))
    k32.CloseHandle(h)
    return ok, "" if ok else f"err={ctypes.get_last_error()}"


def t_write(pid, peb):
    h = k32.OpenProcess(VM_READ | VM_WRITE | VM_OP, False, pid)
    if not h:
        return None, f"open err={ctypes.get_last_error()}"
    addr = (peb or 0x7FFE0000) + 0x10
    cur = ctypes.c_uint64(0)
    got = ctypes.c_size_t(0)
    ok = k32.ReadProcessMemory(h, ctypes.c_void_p(addr), ctypes.byref(cur), 8,
                               ctypes.byref(got))
    if ok:
        ok = k32.WriteProcessMemory(h, ctypes.c_void_p(addr), ctypes.byref(cur), 8,
                                    ctypes.byref(got))
    k32.CloseHandle(h)
    return ok, "" if ok else f"err={ctypes.get_last_error()}"


def t_alloc(pid, peb):
    h = k32.OpenProcess(VM_OP, False, pid)
    if not h:
        return None, f"open err={ctypes.get_last_error()}"
    p = k32.VirtualAllocEx(h, None, 0x1000, 0x3000, 0x04)
    ok = bool(p)
    detail = f"at={p:#x}" if ok else f"err={ctypes.get_last_error()}"
    if ok:
        k32.VirtualFreeEx(h, p, 0, 0x8000)
    k32.CloseHandle(h)
    return ok, detail


def t_thread(pid, peb):
    h = k32.OpenProcess(CREATE_THREAD | QUERY_INFO | VM_OP | VM_WRITE, False, pid)
    if not h:
        return None, f"open err={ctypes.get_last_error()}"
    start = k32.GetProcAddress(k32.GetModuleHandleW("kernel32.dll"), b"Sleep")
    t = k32.CreateRemoteThread(h, None, 0, ctypes.c_void_p(start),
                               ctypes.c_void_p(1000), 0, None)
    ok = bool(t)
    detail = f"err={ctypes.get_last_error()}" if not ok else ""
    if ok:
        k32.CloseHandle(t)
    k32.CloseHandle(h)
    return ok, detail


def t_query(pid, peb):
    h = k32.OpenProcess(QUERY_INFO, False, pid)
    if not h:
        return None, f"open err={ctypes.get_last_error()}"
    code = w.DWORD(0)
    ok = k32.GetExitCodeProcess(h, ctypes.byref(code))
    k32.CloseHandle(h)
    return ok, "" if ok else f"err={ctypes.get_last_error()}"


def t_suspend(pid, peb):
    tid = first_thread(pid)
    if not tid:
        return None, "no thread"
    th = k32.OpenThread(SUSPEND, False, tid)
    if not th:
        return None, f"open thread err={ctypes.get_last_error()}"
    ok = k32.SuspendThread(th) != 0xFFFFFFFF
    detail = "" if ok else f"err={ctypes.get_last_error()}"
    if ok:
        k32.ResumeThread(th)
    k32.CloseHandle(th)
    return ok, detail


def t_setinfo(pid, peb):
    h = k32.OpenProcess(SET_INFO | SET_QUOTA | QUERY_INFO, False, pid)
    if not h:
        return None, f"open err={ctypes.get_last_error()}"
    ok = bool(k32.SetProcessWorkingSetSize(h, ctypes.c_size_t(-1), ctypes.c_size_t(-1)))
    detail = "" if ok else f"err={ctypes.get_last_error()}"
    k32.CloseHandle(h)
    return ok, detail


TESTS = [
    ("VM_READ (RPM)", t_read),
    ("VM_WRITE (WPM)", t_write),
    ("VM_OPERATION", t_alloc),
    ("CREATE_THREAD", t_thread),
    ("QUERY_INFORMATION", t_query),
    ("SUSPEND_RESUME", t_suspend),
    ("SET_INFORMATION", t_setinfo),
]


def probe(pid, name):
    peb = remote_peb(pid)
    print(f"== {name} pid={pid} peb={peb and hex(peb)}")
    h = k32.OpenProcess(PROCESS_ALL, False, pid)
    print(f"  OpenProcess(ALL)   {'OK' if h else 'FAIL err=%d' % ctypes.get_last_error()}")
    if h:
        k32.CloseHandle(h)
    for label, fn in TESTS:
        ok, detail = fn(pid, peb)
        state = "OPEN DENIED" if ok is None else ("OK" if ok else "FAIL")
        print(f"  {label:<20} {state} {detail}")


def main():
    pid = int(sys.argv[1]) if len(sys.argv) > 1 else None
    if pid:
        probe(pid, "pid")
        return 0
    p = subprocess.Popen(["notepad.exe"])
    time.sleep(1.5)
    probe(p.pid, "notepad")
    p.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
