#!/usr/bin/env python3
"""HP address finder - narrow the client's HP field with the on-screen bar as the oracle.

The HUD only repaints on server packets, so writing an address never proves anything.
What does work: the HP bar is the truth. This window reads it off the screen, keeps a
candidate list of u32s that look like HP, and after every real hit you press "HP dropped"
to keep only the addresses whose value fell too. Regen passes ("HP healed") prune the
other direction. When one address is left, "Freeze" locks it through the injected module.

Usage: run the game, attach with the panel (mapping needed for Freeze only), then
  Scan  -> take a hit -> HP dropped -> heal/regen -> HP healed -> ... until 1-3 left.
"""

import ctypes
import ctypes.wintypes as w
import os
import queue
import subprocess
import sys
import tempfile
import threading
import tkinter as tk
from pathlib import Path
from tkinter import ttk, messagebox

from PIL import ImageGrab

ROOT = Path(__file__).resolve().parent.parent
SCANOUT = ROOT / "neon-hack" / "scanout.exe"
SAVED = Path(__file__).with_name("hp_addr.txt")

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenProcess.restype = w.HANDLE
k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.OpenFileMappingW.restype = w.HANDLE
k32.OpenFileMappingW.argtypes = [w.DWORD, w.BOOL, w.LPCWSTR]
k32.MapViewOfFile.restype = ctypes.c_void_p
k32.MapViewOfFile.argtypes = [w.HANDLE, w.DWORD, w.DWORD, w.DWORD, ctypes.c_size_t]
k32.CloseHandle.argtypes = [w.HANDLE]
k32.UnmapViewOfFile.argtypes = [ctypes.c_void_p]

u32 = ctypes.WinDLL("user32", use_last_error=True)


class NEON_HACK_STATE(ctypes.Structure):
    _fields_ = [("magic", w.ULONG), ("version", w.ULONG), ("pid", w.ULONG),
                ("module_base", ctypes.c_ulonglong), ("cmd", w.ULONG),
                ("state", w.ULONG), ("candidates", w.ULONG), ("damage_events", w.ULONG),
                ("hp", w.ULONG), ("max_hp", w.ULONG), ("frozen_count", w.ULONG),
                ("last_error", w.ULONG), ("ticks", w.ULONG),
                ("frozen_addr", ctypes.c_ulonglong * 4), ("frozen_value", w.ULONG * 4),
                ("force_addr", ctypes.c_ulonglong), ("force_value", w.ULONG)]


MAGIC = 0x4E454F48
BAR_Y0, BAR_Y1 = 1180, 1260        # offsets from the window top
BAR_X0, BAR_X1 = 560, 1040         # offsets from the window left edge


def game_pid():
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "(Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1).Id"],
        capture_output=True, text=True)
    try:
        return int(out.stdout.strip())
    except ValueError:
        return 0


def game_rect(pid):
    """Rect of the game's top-level window; also un-hides it (the client hides its own
    window when it loses focus in some states)."""
    found = []

    @ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def cb(hwnd, _):
        p = w.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid:
            found.append(hwnd)
        return True

    u32.EnumWindows(cb, 0)
    if found:
        hwnd = found[0]
        if not u32.IsWindowVisible(hwnd):
            u32.ShowWindow(hwnd, 5)
        r = w.RECT()
        if u32.GetWindowRect(hwnd, ctypes.byref(r)):
            return hwnd, r
    return 0, None


def measure(pid):
    """Return (hp_percent, mp_percent, window found) from the bars."""
    hwnd, r = game_rect(pid)
    if not hwnd:
        return None, None
    x0, y0 = r.left + BAR_X0, r.top + BAR_Y0
    x1, y1 = r.left + BAR_X1, min(r.top + BAR_Y1, r.bottom)
    if y1 <= y0:
        return None, None
    img = ImageGrab.grab(bbox=(x0, y0, x1, y1), all_screens=True)
    px = img.load()
    wd, ht = img.size
    best = (0, 0, 0)  # hp_count, y, mp_count
    for y in range(ht):
        hp = mp = 0
        for x in range(wd):
            cr, cg, cb_ = px[x, y][:3]
            if cr > 110 and cr > cg + 25:
                hp += 1
            elif cb_ > 110 and cb_ > cr + 25:
                mp += 1
        if hp > best[0]:
            best = (hp, y, mp)
    hp_n, _, mp_n = best
    return hp_n, mp_n


class Finder(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Neon - HP address finder")
        self.geometry("700x560+1850+10")
        self.attributes("-topmost", True)
        self.pid = 0
        self.mapping = None
        self.view = None
        self.state = None
        self.cands = {}          # addr -> last value
        self.events = {}         # addr -> (downs, ups)
        self.busy = False
        self.q = queue.Queue()
        self.hp_now = None       # last HP bar width in px
        self.hp_at_snap = None
        self.max_seen = 1
        self._build()
        self.after(150, self._drain)
        self.after(200, self._tick)

    # ---- ui ----------------------------------------------------------
    def _build(self):
        top = ttk.Frame(self, padding=8)
        top.pack(fill="x")
        self.pid_lbl = ttk.Label(top, text="no game", font=("Segoe UI", 10, "bold"))
        self.pid_lbl.pack(side="left")
        self.map_lbl = ttk.Label(top, text="")
        self.map_lbl.pack(side="left", padx=8)
        ttk.Button(top, text="Attach mapping", command=self.reopen).pack(side="right")

        bar = ttk.Frame(self, padding=8)
        bar.pack(fill="x")
        self.hp_lbl = ttk.Label(bar, text="HP: ?", font=("Consolas", 16, "bold"))
        self.hp_lbl.pack(side="left")
        self.hp_canvas = tk.Canvas(bar, width=260, height=16, bg="#202020",
                                   highlightthickness=1, highlightbackground="#404040")
        self.hp_canvas.pack(side="left", padx=10)
        ttk.Label(bar, text="max HP").pack(side="left")
        self.max_hp = ttk.Entry(bar, width=6)
        self.max_hp.insert(0, "162")
        self.max_hp.pack(side="left", padx=4)

        ops = ttk.LabelFrame(self, text="1. narrow", padding=8)
        ops.pack(fill="x", padx=8)
        r = ttk.Frame(ops)
        r.pack(fill="x")
        ttk.Label(r, text="range").pack(side="left")
        self.lo = ttk.Entry(r, width=6); self.lo.insert(0, "5"); self.lo.pack(side="left", padx=2)
        ttk.Label(r, text="..").pack(side="left")
        self.hi = ttk.Entry(r, width=6); self.hi.insert(0, "400"); self.hi.pack(side="left", padx=2)
        ttk.Button(r, text="Scan", command=self.scan).pack(side="left", padx=6)
        self.cal_btn = ttk.Button(r, text="calibrate full bar", command=self.calibrate)
        self.cal_btn.pack(side="left", padx=4)
        r2 = ttk.Frame(ops)
        r2.pack(fill="x", pady=(6, 0))
        ttk.Button(r2, text="HP dropped", command=lambda: self.filter_dec()).pack(side="left")
        ttk.Button(r2, text="HP healed", command=lambda: self.filter_inc()).pack(side="left", padx=4)
        ttk.Button(r2, text="= current HP", command=self.filter_eq).pack(side="left")
        ttk.Button(r2, text="unchanged", command=self.filter_same).pack(side="left", padx=4)
        ttk.Button(r2, text="reset", command=self.reset).pack(side="left", padx=12)
        self.count_lbl = ttk.Label(r2, text="no scan yet")
        self.count_lbl.pack(side="left", padx=8)

        cols = ("addr", "value", "down", "up")
        self.tree = ttk.Treeview(self, columns=cols, show="headings", height=12)
        for c, wd in zip(cols, (160, 90, 60, 60)):
            self.tree.heading(c, text=c)
            self.tree.column(c, width=wd, anchor="w")
        self.tree.pack(fill="both", expand=True, padx=8, pady=6)
        self.tree.bind("<<TreeviewSelect>>", self._picked)

        box = ttk.LabelFrame(self, text="2. lock", padding=8)
        box.pack(fill="x", padx=8, pady=(0, 8))
        r3 = ttk.Frame(box)
        r3.pack(fill="x")
        ttk.Label(r3, text="addr").pack(side="left")
        self.addr = ttk.Entry(r3, width=14)
        self.addr.pack(side="left", padx=4)
        ttk.Label(r3, text="value").pack(side="left")
        self.val = ttk.Entry(r3, width=7)
        self.val.pack(side="left", padx=4)
        ttk.Button(r3, text="use HP +1", command=self.use_current).pack(side="left", padx=4)
        ttk.Button(r3, text="Freeze", command=self.freeze).pack(side="left", padx=8)
        ttk.Button(r3, text="Unfreeze", command=self.unfreeze).pack(side="left")
        ttk.Button(r3, text="Save", command=self.save).pack(side="right")

        self.log = tk.Text(self, height=7, font=("Consolas", 8), bg="#101014", fg="#cfcfcf")
        self.log.pack(fill="x", padx=8, pady=(0, 8))

    def logline(self, s):
        self.q.put(s)

    def _drain(self):
        try:
            while True:
                self.log.insert("end", self.q.get_nowait() + "\n")
                self.log.see("end")
        except queue.Empty:
            pass
        self.after(150, self._drain)

    # ---- live readouts ----------------------------------------------
    def _tick(self):
        pid = game_pid()
        if pid != self.pid:
            self.pid = pid
            self.pid_lbl.config(text=f"pid {pid}" if pid else "no game")
            if pid:
                self.reopen()
        if pid:
            hp, mp = measure(pid) if not self.busy else (None, None)
            if hp is not None:
                self.hp_now = hp
                self.max_seen = max(self.max_seen, hp)
                pct = hp / self.max_seen
                self.hp_canvas.delete("all")
                self.hp_canvas.create_rectangle(0, 0, int(260 * pct), 15, fill="#b02c2c", width=0)
                try:
                    mh = int(self.max_hp.get() or 0) or 1
                except ValueError:
                    mh = 1
                self.hp_lbl.config(text=f"HP: ~{round(pct * mh)}   ({mp} mp px)")
        self.after(300, self._tick)

    # ---- mapping -----------------------------------------------------
    def reopen(self):
        pid = game_pid()
        if not pid:
            self.logline("no game to attach to")
            return
        self.close_map()
        name = f"Local\\NeonHack.{pid}"
        self.mapping = k32.OpenFileMappingW(0x0004 | 0x0002, False, name)
        if not self.mapping:
            self.map_lbl.config(text="no mapping (attach in the panel)")
            return
        self.view = k32.MapViewOfFile(self.mapping, 0x0006, 0, 0, 0)
        self.state = NEON_HACK_STATE.from_address(self.view)
        if self.state.magic != MAGIC:
            self.logline("bad mapping magic")
            self.close_map()
            return
        self.map_lbl.config(text=f"mapping ok, base {self.state.module_base:#x}")

    def close_map(self):
        if self.view:
            k32.UnmapViewOfFile(self.view)
        if self.mapping:
            k32.CloseHandle(self.mapping)
        self.view = self.mapping = None
        self.state = None
        self.map_lbl.config(text="")

    # ---- scanning ----------------------------------------------------
    def _run(self, argv, on_done):
        if self.busy:
            self.logline("busy...")
            return
        if not self.pid:
            self.logline("no game")
            return
        self.busy = True

        def work():
            try:
                p = subprocess.run([str(SCANOUT), str(self.pid)] + argv,
                                   capture_output=True, text=True)
                self.after(0, lambda: on_done(p.stdout, p.stderr))
            except Exception as e:
                self.after(0, lambda: self.logline(f"scanout failed: {e}"))
            finally:
                self.after(0, lambda: setattr(self, "busy", False))

        threading.Thread(target=work, daemon=True).start()

    def current_hp(self):
        """HP from the bar, scaled by the widest bar seen (i.e. our full-bar
        calibration) and the max HP in the entry."""
        if self.hp_now is None:
            return None
        try:
            mh = int(self.max_hp.get())
        except ValueError:
            return None
        return round(self.hp_now / self.max_seen * mh)

    def scan(self):
        want = self.current_hp()
        if want is None:
            try:
                lo, hi = int(self.lo.get()), int(self.hi.get())
            except ValueError:
                messagebox.showerror("range", "range must be numbers")
                return
            note = f"bar unreadable, using range {lo}..{hi}"
        else:
            lo, hi = max(1, want - 3), want + 3
            note = f"bar says ~{want}, scanning {lo}..{hi}"
        self.cands.clear(); self.events.clear(); self._refresh()
        self.logline(note)
        self.hp_at_snap = self.hp_now

        def done(out, err):
            for line in out.splitlines():
                parts = line.split()
                if len(parts) == 2 and len(self.cands) < 400000:
                    try:
                        self.cands[int(parts[0], 16)] = int(parts[1])
                    except ValueError:
                        pass
            self.events = {a: [0, 0] for a in self.cands}
            self.logline(f"scan done: {len(self.cands)} candidates")
            self._refresh()

        self._run(["findrangep", str(lo), str(hi)], done)

    def _reread(self, predicate, label):
        if not self.cands:
            self.logline("scan first")
            return
        if self.hp_at_snap is not None and self.hp_now is not None:
            delta = self.hp_now - self.hp_at_snap
            if label == "dropped" and delta >= 0:
                self.logline(f"note: bar did not shrink since the last pass ({delta:+d}px)")
            if label == "healed" and delta <= 0:
                self.logline(f"note: bar did not grow since the last pass ({delta:+d}px)")
        self.hp_at_snap = self.hp_now
        fd, path = tempfile.mkstemp(suffix=".txt")
        with os.fdopen(fd, "w") as f:
            f.write("\n".join(f"{a:X}" for a in self.cands))
        old = dict(self.cands)

        def done(out, err):
            fresh = {}
            for line in out.splitlines():
                parts = line.split()
                if len(parts) == 2:
                    try:
                        fresh[int(parts[0], 16)] = int(parts[1])
                    except ValueError:
                        pass
            keep = {}
            for a, v in fresh.items():
                prev = old.get(a)
                if prev is None:
                    continue
                if predicate(prev, v):
                    keep[a] = v
                    if v < prev:
                        self.events.setdefault(a, [0, 0])[0] += 1
                    elif v > prev:
                        self.events.setdefault(a, [0, 0])[1] += 1
            self.cands = keep
            self.logline(f"{label}: kept {len(keep)} of {len(fresh)}")
            self._refresh()

        self._run(["diffcheck", path], done)

    def calibrate(self):
        if self.hp_now:
            self.max_seen = max(self.max_seen, self.hp_now)
            self.logline(f"full bar = {self.max_seen}px")

    def filter_dec(self):
        self._reread(lambda prev, v: v < prev, "dropped")

    def filter_inc(self):
        self._reread(lambda prev, v: v > prev, "healed")

    def filter_same(self):
        self._reread(lambda prev, v: v == prev, "unchanged")

    def filter_eq(self):
        hp, _ = measure(self.pid) if self.pid else (None, None)
        if not hp:
            self.logline("cannot read the bar")
            return
        try:
            mh = int(self.max_hp.get())
        except ValueError:
            mh = 0
        pct = hp / self.max_seen
        want = round(pct * mh) if mh else None
        self.logline(f"keeping values within +-2 of ~{want} (bar {pct:.0%})")
        self._reread(lambda prev, v: want is not None and abs(v - want) <= 2, "=hp")

    def reset(self):
        self.cands.clear(); self.events.clear(); self.hp_at_snap = None
        self._refresh()
        self.logline("reset")

    def _refresh(self):
        self.tree.delete(*self.tree.get_children())
        order = sorted(self.cands.items(),
                       key=lambda kv: (self.events.get(kv[0], [0, 0])[0], -kv[1]),
                       reverse=True)[:200]
        for a, v in order:
            ev = self.events.get(a, [0, 0])
            self.tree.insert("", "end", values=(f"{a:012X}", v, ev[0], ev[1]))
        self.count_lbl.config(text=f"{len(self.cands)} candidates")

    # ---- lock --------------------------------------------------------
    def _picked(self, _ev):
        sel = self.tree.selection()
        if not sel:
            return
        a = int(self.tree.item(sel[0], "values")[0], 16)
        self.addr.delete(0, "end")
        self.addr.insert(0, f"{a:X}")
        v = self.cands.get(a)
        if v is not None:
            self.val.delete(0, "end")
            self.val.insert(0, str(v))

    def use_current(self):
        try:
            mh = int(self.max_hp.get())
        except ValueError:
            mh = 0
        if self.hp_now is None:
            return
        v = round(self.hp_now / self.max_seen * mh) if mh else 0
        self.val.delete(0, "end")
        self.val.insert(0, str(v))

    def _addr(self):
        try:
            return int(self.addr.get(), 16)
        except ValueError:
            messagebox.showerror("bad address", "address is hex")
            return 0

    def freeze(self):
        if not self.state:
            self.reopen()
        if not self.state:
            messagebox.showerror("not attached", "attach the module in the panel first")
            return
        a = self._addr()
        if not a:
            return
        try:
            v = int(self.val.get(), 0) if self.val.get().lower().startswith("0x") \
                else int(self.val.get())
        except ValueError:
            messagebox.showerror("bad value", "value must be a number")
            return
        self.state.force_addr = a
        self.state.force_value = v
        self.state.cmd = 2
        self.logline(f"freeze {a:X} = {v}")

    def unfreeze(self):
        if self.state:
            self.state.cmd = 0
            self.logline("freeze off")

    def save(self):
        a = self._addr()
        if a:
            SAVED.write_text(self.addr.get().strip() + "\n")
            self.logline(f"saved to {SAVED.name}")


if __name__ == "__main__":
    if not sys.platform.startswith("win"):
        sys.exit("windows only")
    if SAVED.exists():
        pass
    Finder().mainloop()
