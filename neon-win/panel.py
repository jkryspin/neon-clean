#!/usr/bin/env python3
"""neon-win control panel - attach the hack to MapleStory ClassWorld and toggle it.

Native tkinter, no third-party deps. Run it elevated (it warns otherwise) because the
process DACL only lets a high-integrity caller use WriteProcessMemory/VirtualAllocEx.

Tools:
  * start/kill the client
  * attach with neon-probe/inject-stealth.exe and a live injection log
  * verify PEB absence, region type/protection, top-level window classes
  * toggle neon-hack (HP/MP lock) and watch the scan progress
  * NGS service / driver state
"""

import ctypes
import ctypes.wintypes as w
import os
import queue
import subprocess
import sys
import threading
import time
import tkinter as tk
from tkinter import ttk, messagebox
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
INJECTOR = ROOT / "neon-probe" / "inject-stealth.exe"
HACK_DLL = ROOT / "neon-hack" / "neon-hack.dll"
PRODUCT_URL = "nxl://launch/59813"

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)
adv = ctypes.WinDLL("advapi32")

k32.OpenProcess.restype = w.HANDLE
k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.CloseHandle.argtypes = [w.HANDLE]
k32.OpenFileMappingW.restype = w.HANDLE
k32.OpenFileMappingW.argtypes = [w.DWORD, w.BOOL, w.LPCWSTR]
k32.MapViewOfFile.restype = ctypes.c_void_p
k32.MapViewOfFile.argtypes = [w.HANDLE, w.DWORD, w.DWORD, w.DWORD, ctypes.c_size_t]
k32.UnmapViewOfFile.argtypes = [ctypes.c_void_p]
psapi.EnumProcessModules.argtypes = [w.HANDLE, ctypes.POINTER(w.HMODULE), w.DWORD,
                                     ctypes.POINTER(w.DWORD)]
psapi.GetModuleFileNameExW.argtypes = [w.HANDLE, w.HMODULE, w.LPWSTR, w.DWORD]
adv.OpenProcessToken.argtypes = [w.HANDLE, w.DWORD, ctypes.POINTER(w.HANDLE)]
adv.GetTokenInformation.argtypes = [w.HANDLE, ctypes.c_uint, ctypes.c_void_p, w.DWORD,
                                    ctypes.POINTER(w.DWORD)]


class NEON_HACK_STATE(ctypes.Structure):
    _fields_ = [("magic", w.ULONG), ("version", w.ULONG), ("pid", w.ULONG),
                ("module_base", ctypes.c_ulonglong), ("cmd", w.ULONG),
                ("state", w.ULONG), ("candidates", w.ULONG), ("damage_events", w.ULONG),
                ("hp", w.ULONG), ("max_hp", w.ULONG), ("frozen_count", w.ULONG),
                ("last_error", w.ULONG), ("ticks", w.ULONG),
                ("frozen_addr", ctypes.c_ulonglong * 4), ("frozen_value", w.ULONG * 4),
                ("force_addr", ctypes.c_ulonglong), ("force_value", w.ULONG)]


MAGIC = 0x4E454F48
STATES = {0: "idle", 1: "scanning", 2: "confirmed", 3: "frozen"}


def is_admin():
    try:
        return ctypes.windll.shell32.IsUserAnAdmin() != 0
    except Exception:
        return False


def find_game():
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "(Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1).Id"],
        capture_output=True, text=True)
    try:
        return int(out.stdout.strip())
    except ValueError:
        return 0


def modules_of(pid):
    h = k32.OpenProcess(0x410, False, pid)
    if not h:
        return []
    try:
        arr = (w.HMODULE * 2048)()
        need = w.DWORD()
        if not psapi.EnumProcessModules(h, arr, ctypes.sizeof(arr), ctypes.byref(need)):
            return []
        names = []
        for i in range(need.value // ctypes.sizeof(w.HMODULE)):
            b = ctypes.create_unicode_buffer(300)
            psapi.GetModuleFileNameExW(h, arr[i], b, 300)
            names.append(Path(b.value).name if b.value else "")
        return names
    finally:
        k32.CloseHandle(h)


def window_list(pid):
    res = []

    @ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def cb(hwnd, _):
        p = w.DWORD()
        ctypes.windll.user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid and ctypes.windll.user32.IsWindowVisible(hwnd):
            cls = ctypes.create_unicode_buffer(128)
            tit = ctypes.create_unicode_buffer(256)
            ctypes.windll.user32.GetClassNameW(hwnd, cls, 128)
            ctypes.windll.user32.GetWindowTextW(hwnd, tit, 256)
            res.append(f"{cls.value}|{tit.value}")
        return True

    ctypes.windll.user32.EnumWindows(cb, 0)
    return res


def integrity_of(pid):
    h = k32.OpenProcess(0x1000, False, pid)
    if not h:
        return "?"
    t = w.HANDLE()
    if not adv.OpenProcessToken(h, 8, ctypes.byref(t)):
        k32.CloseHandle(h)
        return "?"
    buf = ctypes.create_string_buffer(64)
    n = w.DWORD()
    adv.GetTokenInformation(t, 18, buf, 64, ctypes.byref(n))
    k32.CloseHandle(h)
    return {0x1000: "Medium", 0x2000: "High", 0x3000: "System"}.get(
        ctypes.cast(buf, ctypes.POINTER(w.DWORD))[0], "?")


def service_state(name):
    r = subprocess.run(["sc.exe", "query", name], capture_output=True, text=True)
    for line in r.stdout.splitlines():
        if "STATE" in line:
            return line.split(":", 1)[1].strip().split()[0]
    return "?"


class Panel(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Neon - MapleStory Classic World")
        self.geometry("760x640")
        self.pid = 0
        self.mapping = None
        self.view = None
        self.state = None
        self.logq = queue.Queue()
        self._build()
        self.after(200, self._drain)
        self.after(300, self._poll)

    def _build(self):
        top = ttk.Frame(self, padding=8)
        top.pack(fill="x")
        ttk.Label(top, text="Game:", font=("Segoe UI", 10, "bold")).pack(side="left")
        self.pid_lbl = ttk.Label(top, text="not running")
        self.pid_lbl.pack(side="left", padx=8)
        ttk.Button(top, text="Launch", command=self.launch).pack(side="right")
        ttk.Button(top, text="Kill", command=self.kill).pack(side="right", padx=4)
        ttk.Button(top, text="Refresh", command=self._poll).pack(side="right", padx=4)

        mid = ttk.LabelFrame(self, text="Injection", padding=8)
        mid.pack(fill="x", padx=8)
        row = ttk.Frame(mid)
        row.pack(fill="x")
        ttk.Button(row, text="Attach", command=self.attach).pack(side="left")
        ttk.Button(row, text="Verify", command=self.verify).pack(side="left", padx=4)
        self.attached_lbl = ttk.Label(row, text="not attached")
        self.attached_lbl.pack(side="left", padx=8)
        self.log = tk.Text(mid, height=8, font=("Consolas", 8), bg="#101014", fg="#d0d0d0")
        self.log.pack(fill="x", pady=(6, 0))

        hack = ttk.LabelFrame(self, text="God mode (client-side HP/MP lock)", padding=8)
        hack.pack(fill="x", padx=8, pady=8)
        r1 = ttk.Frame(hack)
        r1.pack(fill="x")
        self.hack_btn = ttk.Button(r1, text="Enable", command=self.toggle, state="disabled")
        self.hack_btn.pack(side="left")
        self.state_lbl = ttk.Label(r1, text="off", font=("Segoe UI", 10, "bold"))
        self.state_lbl.pack(side="left", padx=10)
        self.detail_lbl = ttk.Label(r1, text="")
        self.detail_lbl.pack(side="left", padx=10)
        r2 = ttk.Frame(hack)
        r2.pack(fill="x", pady=(6, 0))
        ttk.Label(r2, text="force addr").pack(side="left")
        self.force_addr = ttk.Entry(r2, width=12)
        self.force_addr.pack(side="left", padx=4)
        ttk.Label(r2, text="value").pack(side="left")
        self.force_val = ttk.Entry(r2, width=8)
        self.force_val.pack(side="left", padx=4)
        ttk.Button(r2, text="Force", command=self.force).pack(side="left", padx=4)

        st = ttk.LabelFrame(self, text="Status", padding=8)
        st.pack(fill="both", expand=True, padx=8, pady=(0, 8))
        self.status = tk.Text(st, height=10, font=("Consolas", 8), bg="#101014", fg="#c8c8c8")
        self.status.pack(fill="both", expand=True)

    # ---- log helpers -------------------------------------------------
    def logline(self, s):
        self.logq.put(s)

    def _drain(self):
        try:
            while True:
                s = self.logq.get_nowait()
                self.log.insert("end", s + "\n")
                self.log.see("end")
        except queue.Empty:
            pass
        self.after(200, self._drain)

    def set_status(self, s):
        self.status.delete("1.0", "end")
        self.status.insert("1.0", s)

    # ---- game --------------------------------------------------------
    def launch(self):
        os.startfile(PRODUCT_URL)
        self.logline("launch requested")

    def kill(self):
        subprocess.run(["powershell", "-NoProfile", "-Command",
                        "Get-Process MapleStory -ErrorAction SilentlyContinue | Stop-Process -Force"])
        self.close_mapping()

    # ---- injection ---------------------------------------------------
    def attach(self):
        if not is_admin():
            if not messagebox.askyesno("Not elevated",
                                       "The panel is not elevated; the injected code and "
                                       "the hack cannot write to the client.\nContinue anyway?"):
                return
        pid = find_game()
        if not pid:
            messagebox.showerror("No game", "MapleStory.exe is not running.")
            return
        if not INJECTOR.exists() or not HACK_DLL.exists():
            messagebox.showerror("Missing build", f"Build first:\n{INJECTOR}\n{HACK_DLL}")
            return
        self.logline(f"=== injecting {HACK_DLL.name} into {pid}")
        threading.Thread(target=self._inject, args=(pid,), daemon=True).start()

    def _inject(self, pid):
        try:
            p = subprocess.Popen([str(INJECTOR), str(pid), str(HACK_DLL)],
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                 text=True, bufsize=1)
            for line in p.stdout:
                self.logline(line.rstrip())
            rc = p.wait()
        except Exception as e:
            self.logline(f"injector failed: {e}")
            return
        if rc == 0:
            self.after(0, lambda: self.open_mapping(pid))
        else:
            self.logline(f"=== injector exit {rc}")

    def open_mapping(self, pid):
        self.close_mapping()
        name = f"Local\\NeonHack.{pid}"
        self.mapping = k32.OpenFileMappingW(0x0004 | 0x0002, False, name)
        if not self.mapping:
            self.logline(f"no mapping {name}")
            self.attached_lbl.config(text="mapping missing")
            return
        self.view = k32.MapViewOfFile(self.mapping, 0x0006, 0, 0, 0)
        self.state = NEON_HACK_STATE.from_address(self.view)
        if self.state.magic != MAGIC:
            self.logline(f"bad magic {self.state.magic:#x}")
            self.close_mapping()
            return
        self.attached_lbl.config(text=f"attached, base {self.state.module_base:#x}")
        self.hack_btn.config(state="normal")
        self.logline("=== attached")

    def close_mapping(self):
        if self.state:
            try:
                self.state.cmd = 0
            except Exception:
                pass
        if self.view:
            k32.UnmapViewOfFile(self.view)
        if self.mapping:
            k32.CloseHandle(self.mapping)
        self.view = None
        self.mapping = None
        self.state = None
        self.hack_btn.config(state="disabled")
        self.attached_lbl.config(text="not attached")

    def toggle(self):
        if not self.state:
            return
        on = self.state.cmd == 0
        self.state.cmd = 1 if on else 0
        if not on:
            self.state.state = 0
        self.hack_btn.config(text="Disable" if on else "Enable")
        self.logline("hack on" if on else "hack off")

    def force(self):
        if not self.state:
            return
        try:
            a = int(self.force_addr.get(), 16)
            v = int(self.force_val.get(), 16) if self.force_val.get().lower().startswith("0x") \
                else int(self.force_val.get(), 10)
        except ValueError:
            messagebox.showerror("Bad value", "address is hex (e.g. 12AB34CD), value decimal or 0x")
            return
        self.state.force_addr = a
        self.state.force_value = v
        self.state.cmd = 2
        self.hack_btn.config(text="Disable")
        self.logline(f"force {a:#x} = {v}")

    def verify(self):
        pid = self.pid
        if not pid:
            return
        mods = modules_of(pid)
        hidden = "neon-hack.dll" not in [m.lower() for m in mods]
        wins = window_list(pid)
        dialog = [x for x in wins if x.startswith("#32770")]
        extra = ""
        if self.state:
            base = self.state.module_base
            extra = f"module base {base:#x}  frozen {self.state.frozen_count}"
        self.logline(f"verify: modules={len(mods)} hack-in-peb={'NO (good)' if hidden else 'YES'} "
                     f"dialogs={dialog or 'none'} {extra}")

    # ---- poll --------------------------------------------------------
    def _poll(self):
        self.pid = find_game()
        self.pid_lbl.config(text=str(self.pid) if self.pid else "not running")
        lines = [
            f"integrity: {integrity_of(self.pid) if self.pid else '-'}",
            f"NGS service: {service_state('NGS')}   BlackCat1: {service_state('BlackCat1')}",
        ]
        if self.pid:
            wins = window_list(self.pid)
            lines.append(f"windows: {wins or 'none'}")
        if self.state:
            st = self.state
            self.state_lbl.config(text=STATES.get(st.state, "?"))
            self.detail_lbl.config(
                text=f"cand {st.candidates}  dmg {st.damage_events}  "
                     f"hp {st.hp}/{st.max_hp}  frozen {st.frozen_count}")
        else:
            self.state_lbl.config(text="off")
            self.detail_lbl.config(text="")
        self.set_status("\n".join(lines))
        self.after(500, self._poll)


if __name__ == "__main__":
    if not is_admin():
        print("warning: not elevated")
    Panel().mainloop()
