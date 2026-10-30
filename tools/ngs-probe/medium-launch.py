#!/usr/bin/env python3
"""Run a command as the desktop user in the *interactive logon session*.

An elevated shell's children stay high IL, and scheduled tasks get a fresh logon
session (missing the S-1-5-5-0-<id> SID that the default process DACL trusts). This
copies the shell's own (medium, session-tagged) token and launches through it, which is
exactly how the bot is started in real use.

    python medium-launch.py "cmd /c whoami" [timeout_s]
"""

import ctypes
import ctypes.wintypes as w
import subprocess
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
adv = ctypes.WinDLL("advapi32", use_last_error=True)

k32.OpenProcess.restype = w.HANDLE
k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.CloseHandle.argtypes = [w.HANDLE]
adv.OpenProcessToken.argtypes = [w.HANDLE, w.DWORD, ctypes.POINTER(w.HANDLE)]
adv.DuplicateTokenEx.argtypes = [w.HANDLE, w.DWORD, ctypes.c_void_p, ctypes.c_uint,
                                 ctypes.c_uint, ctypes.POINTER(w.HANDLE)]

TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_ASSIGN_PRIMARY = 0x2, 0x8, 0x1
MAXIMUM_ALLOWED = 0x02000000
SecurityImpersonation = 2
TokenPrimary = 1


class STARTUPINFO(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("lpReserved", w.LPWSTR), ("lpDesktop", w.LPWSTR),
                ("lpTitle", w.LPWSTR), ("dwX", w.DWORD), ("dwY", w.DWORD),
                ("dwXSize", w.DWORD), ("dwYSize", w.DWORD), ("dwXCountChars", w.DWORD),
                ("dwYCountChars", w.DWORD), ("dwFillAttribute", w.DWORD),
                ("dwFlags", w.DWORD), ("wShowWindow", w.WORD), ("cbReserved2", w.WORD),
                ("lpReserved2", ctypes.c_void_p), ("hStdInput", w.HANDLE),
                ("hStdOutput", w.HANDLE), ("hStdError", w.HANDLE)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", w.HANDLE), ("hThread", w.HANDLE), ("dwProcessId", w.DWORD),
                ("dwThreadId", w.DWORD)]


adv.CreateProcessWithTokenW.argtypes = [w.HANDLE, w.DWORD, w.LPCWSTR, w.LPWSTR, w.DWORD,
                                        ctypes.c_void_p, w.LPCWSTR,
                                        ctypes.POINTER(STARTUPINFO),
                                        ctypes.POINTER(PROCESS_INFORMATION)]


def shell_pid():
    out = subprocess.check_output(["powershell", "-NoProfile", "-Command",
                                   "(Get-Process explorer | Select-Object -First 1).Id"])
    return int(out.decode().strip())


def main():
    cmdline = sys.argv[1] if len(sys.argv) > 1 else "cmd /c whoami"
    timeout = float(sys.argv[2]) if len(sys.argv) > 2 else 120.0
    hp = k32.OpenProcess(0x1000, False, shell_pid())
    if not hp:
        sys.exit(f"OpenProcess(explorer): {ctypes.get_last_error()}")
    ht = w.HANDLE()
    if not adv.OpenProcessToken(hp, TOKEN_DUPLICATE | TOKEN_QUERY, ctypes.byref(ht)):
        sys.exit(f"OpenProcessToken: {ctypes.get_last_error()}")
    ht2 = w.HANDLE()
    if not adv.DuplicateTokenEx(ht, MAXIMUM_ALLOWED, None, SecurityImpersonation,
                                TokenPrimary, ctypes.byref(ht2)):
        sys.exit(f"DuplicateTokenEx: {ctypes.get_last_error()}")
    si = STARTUPINFO()
    si.cb = ctypes.sizeof(si)
    pi = PROCESS_INFORMATION()
    ok = adv.CreateProcessWithTokenW(ht2, 0, None, ctypes.create_unicode_buffer(cmdline),
                                     0x00000400 | 0x08000000, None, None,
                                     ctypes.byref(si), ctypes.byref(pi))
    if not ok:
        sys.exit(f"CreateProcessWithTokenW: {ctypes.get_last_error()}")
    k32.WaitForSingleObject(pi.hProcess, int(timeout * 1000))
    code = w.DWORD(0)
    k32.GetExitCodeProcess(pi.hProcess, ctypes.byref(code))
    for h in (pi.hProcess, pi.hThread, ht, ht2, hp):
        k32.CloseHandle(h)
    return code.value


if __name__ == "__main__":
    sys.exit(main())
