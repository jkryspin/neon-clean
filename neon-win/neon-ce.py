#!/usr/bin/env python3
"""neon-ce - Cheat-Engine-like front end for the Rust scanner.

The Rust binary does all memory work (`neon-ce serve`, loopback JSON-RPC, bearer
token). This window launches it, drives it, and is also what the MCP bridge talks to,
so anything clickable here is reproducible from a tool call.

Hunting an unknown value (e.g. the invincibility timer):
  Hunt tab -> Seed 0 in a small region around the known field -> Start hunt.
  The hunt re-seeds and re-reads in a tight loop and keeps only addresses that rose and
  then fell; survivors are listed as they appear.
"""

import ctypes
import ctypes.wintypes as w
import json
import os
import queue
import subprocess
import sys
import threading
import time
import tkinter as tk
import urllib.request
from pathlib import Path
from tkinter import ttk, messagebox, filedialog



ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "neon-ce" / "target" / "debug" / "neon-ce.exe"
SESSION = Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "neon" / "ce" / "session.json"

u32 = ctypes.WinDLL("user32", use_last_error=True)

TYPES = ["4 bytes", "byte", "2 bytes", "8 bytes", "float", "double", "string", "utf16", "aob"]
CMPS = ["eq", "ne", "lt", "gt", "between", "increased", "decreased", "changed", "unchanged", "sameasfirst"]
SCOPES = ["heap", "private", "all", "region", "image", "mapped"]


# ---------------------------------------------------------------- engine client
class Engine:
    def __init__(self):
        self.proc = None
        self.port = 0
        self.token = ""
        self.lock = threading.Lock()

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

    def start(self, pid):
        if self.alive():
            return
        if not EXE.exists():
            raise RuntimeError(f"build first: cargo build in {EXE.parent}")
        self.proc = subprocess.Popen(
            [str(EXE), "serve", "--pid", str(pid), "--port", "8790"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(50):
            time.sleep(0.1)
            if SESSION.exists():
                try:
                    s = json.loads(SESSION.read_text())
                except Exception:
                    continue
                self.port, self.token = s["port"], s["token"]
                return

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
        self.proc = None
        self.port = self.token = ""

    def call(self, method, **params):
        if not self.alive():
            raise RuntimeError("engine not running")
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        req = urllib.request.Request(
            f"http://127.0.0.1:{self.port}/rpc", data=body,
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=120) as r:
            out = json.loads(r.read())
        if "error" in out:
            raise RuntimeError(out["error"].get("message", "engine error"))
        return out.get("result")


def game_pid():
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "(Get-Process MapleStory -ErrorAction SilentlyContinue | Select-Object -First 1).Id"],
        capture_output=True, text=True)
    try:
        return int(out.stdout.strip())
    except ValueError:
        return 0


def hexnum(text):
    return int(text.strip().replace("0x", ""), 16)


# ---------------------------------------------------------------- bars
BAR_Y0, BAR_Y1 = 1180, 1260
BAR_X0, BAR_X1 = 560, 1040


def game_rect(pid):
    found = []

    @ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def cb(hwnd, _):
        p = w.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid:
            found.append(hwnd)
        return True

    u32.EnumWindows(cb, 0)
    if not found:
        return None
    hwnd = found[0]
    if not u32.IsWindowVisible(hwnd):
        u32.ShowWindow(hwnd, 5)
    r = w.RECT()
    if not u32.GetWindowRect(hwnd, ctypes.byref(r)):
        return None
    return r


def read_bars(pid):
    r = game_rect(pid)
    if not r:
        return None
    # bottom-anchored: the HUD sits at the bottom of the window whatever its size
    x0, x1 = r.left + 300, r.left + 1100
    y0, y1 = max(r.top, r.bottom - 120), r.bottom - 6
    if y1 <= y0 or x1 <= x0:
        return None
    img = ImageGrab.grab(bbox=(x0, y0, x1, y1), all_screens=True)
    px = img.load()
    wd, ht = img.size
    best = (0, 0)
    for y in range(ht):
        red = blue = 0
        for x in range(wd):
            cr, cg, cb = px[x, y][:3]
            if cr > 110 and cr > cg + 25 and cr > cb + 25:
                red += 1
            elif cb > 110 and cb > cr + 25 and cb > cg + 25:
                blue += 1
        if red > best[0]:
            best = (red, blue)
    return best


# ---------------------------------------------------------------- ui
class App(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("neon-ce")
        self.geometry("1100x700+30+30")
        self.attributes("-topmost", True)
        self.engine = Engine()
        self.pid = 0
        self.session = None
        self.count = 0
        self.hp = None
        self.max_seen = 1
        self.max_hp = 162
        self.hunting = False
        self.stop_hunt = False
        self.q = queue.Queue()
        self._build()
        self.after(120, self._drain)
        self.after(250, self._tick)
        self.protocol("WM_DELETE_WINDOW", self._close)
        self.after(800, self._auto_attach)

    def _build(self):
        top = ttk.Frame(self, padding=6)
        top.pack(fill="x")
        ttk.Label(top, text="pid").pack(side="left")
        self.pid_lbl = ttk.Label(top, text="no game", width=9, font=("Consolas", 10, "bold"))
        self.pid_lbl.pack(side="left", padx=4)
        self.attach_btn = ttk.Button(top, text="Attach", command=self.attach)
        self.attach_btn.pack(side="left")
        ttk.Button(top, text="Find game", command=self.find_game).pack(side="left", padx=4)
        self.engine_lbl = ttk.Label(top, text="engine: off")
        self.engine_lbl.pack(side="left", padx=10)
        self.hp_lbl = ttk.Label(top, text="HP: ?", font=("Consolas", 12, "bold"))
        self.hp_lbl.pack(side="left", padx=10)
        self.bar = tk.Canvas(top, width=200, height=12, bg="#202020", highlightthickness=1,
                             highlightbackground="#404040")
        self.bar.pack(side="left")
        ttk.Label(top, text="hp addr").pack(side="left", padx=(8, 2))
        self.hp_addr = ttk.Entry(top, width=12)
        self.hp_addr.insert(0, "6279578")
        self.hp_addr.pack(side="left")
        ttk.Label(top, text="max").pack(side="left", padx=(8, 2))
        self.max_entry = ttk.Entry(top, width=5)
        self.max_entry.insert(0, "162")
        self.max_entry.pack(side="left")

        nb = ttk.Notebook(self)
        nb.pack(fill="both", expand=True, padx=6, pady=4)
        self.scan_tab = ttk.Frame(nb, padding=6)
        self.hunt_tab = ttk.Frame(nb, padding=6)
        self.freeze_tab = ttk.Frame(nb, padding=6)
        nb.add(self.scan_tab, text="Scan")
        nb.add(self.hunt_tab, text="Hunt")
        nb.add(self.freeze_tab, text="Freeze / table")
        self._build_scan()
        self._build_hunt()
        self._build_freeze()

        self.log = tk.Text(self, height=7, font=("Consolas", 8), bg="#101014", fg="#cfcfcf")
        self.log.pack(fill="x", padx=6, pady=(0, 6))

    # --- scan tab
    def _build_scan(self):
        f = self.scan_tab
        r = ttk.Frame(f)
        r.pack(fill="x")
        ttk.Label(r, text="type").pack(side="left")
        self.vtype = tk.StringVar(value="4 bytes")
        ttk.Combobox(r, textvariable=self.vtype, values=TYPES, width=10, state="readonly").pack(side="left", padx=2)
        ttk.Label(r, text="scope").pack(side="left", padx=(8, 0))
        self.scope = tk.StringVar(value="heap")
        sc = ttk.Combobox(r, textvariable=self.scope, values=SCOPES, width=8, state="readonly")
        sc.pack(side="left", padx=2)
        sc.bind("<<ComboboxSelected>>", lambda _e: self._scope_changed())
        self.module = ttk.Entry(r, width=16)
        self.module.insert(0, "MapleStory.exe")
        self.module.pack(side="left", padx=2)
        self.range_entry = ttk.Entry(r, width=22)
        self.range_entry.pack(side="left", padx=2)
        ttk.Label(r, text="align").pack(side="left", padx=(8, 2))
        self.align = tk.IntVar(value=4)
        for a in (1, 2, 4, 8):
            ttk.Radiobutton(r, text=str(a), value=a, variable=self.align).pack(side="left")

        r2 = ttk.Frame(f)
        r2.pack(fill="x", pady=(4, 0))
        ttk.Label(r2, text="compare").pack(side="left")
        self.cmp = tk.StringVar(value="eq")
        ttk.Combobox(r2, textvariable=self.cmp, values=CMPS, width=11, state="readonly").pack(side="left", padx=2)
        ttk.Label(r2, text="value").pack(side="left", padx=(8, 0))
        self.value = ttk.Entry(r2, width=14)
        self.value.pack(side="left")
        ttk.Label(r2, text="and").pack(side="left", padx=(6, 0))
        self.value2 = ttk.Entry(r2, width=14)
        self.value2.pack(side="left")
        ttk.Button(r2, text="use HP", command=self.use_hp).pack(side="left", padx=6)
        ttk.Button(r2, text="region of HP", command=self.region_of_hp).pack(side="left")

        r3 = ttk.Frame(f)
        r3.pack(fill="x", pady=(6, 0))
        ttk.Button(r3, text="Scan", command=self.scan).pack(side="left")
        ttk.Button(r3, text="Next scan", command=self.next_scan).pack(side="left", padx=4)
        ttk.Button(r3, text="Undo", command=self.undo).pack(side="left")
        ttk.Button(r3, text="Reset", command=self.reset).pack(side="left", padx=4)
        ttk.Button(r3, text="Cancel", command=self.cancel).pack(side="left")
        self.count_lbl = ttk.Label(r3, text="no scan")
        self.count_lbl.pack(side="left", padx=10)

        cols = ("addr", "value")
        self.tree = ttk.Treeview(f, columns=cols, show="headings")
        for c, wd in zip(cols, (200, 140)):
            self.tree.heading(c, text=c)
            self.tree.column(c, width=wd, anchor="w")
        self.tree.pack(fill="both", expand=True, pady=6)
        self.tree.bind("<<TreeviewSelect>>", self._picked)

        r4 = ttk.Frame(f)
        r4.pack(fill="x")
        ttk.Label(r4, text="address").pack(side="left")
        self.addr = ttk.Entry(r4, width=16)
        self.addr.pack(side="left", padx=4)
        ttk.Label(r4, text="value").pack(side="left")
        self.val = ttk.Entry(r4, width=12)
        self.val.pack(side="left", padx=4)
        ttk.Button(r4, text="Read", command=self.read_one).pack(side="left", padx=4)
        ttk.Button(r4, text="Write", command=self.write_one).pack(side="left")
        ttk.Button(r4, text="Freeze", command=self.freeze_one).pack(side="left", padx=4)
        ttk.Button(r4, text="Find pointer", command=self.ptrscan).pack(side="left", padx=(12, 0))

    def _auto_attach(self):
        if not self.engine.alive() and game_pid():
            try:
                self.attach()
            except Exception as e:
                self.logline(f"auto attach: {e}")

    def _scope_changed(self):
        region = self.scope.get() == "region"
        self.range_entry.configure(state="normal" if region else "disabled")
        self.module.configure(state="normal" if self.scope.get() == "image" else "disabled")

    def scope_value(self):
        if self.scope.get() == "region":
            return "range:" + self.range_entry.get().strip()
        return self.scope.get()

    # --- hunt tab
    def _build_hunt(self):
        f = self.hunt_tab
        ttk.Label(
            f,
            text=("Unknown-value hunt: re-seeds to 0 in the chosen region, then keeps only "
                  "addresses that jump up and fall back. Walk onto a mob; survivors are the "
                  "damage/i-frame shaped fields."),
            wraplength=1080, justify="left").pack(anchor="w")
        r = ttk.Frame(f)
        r.pack(fill="x", pady=6)
        ttk.Label(r, text="region").pack(side="left")
        self.hunt_region = ttk.Entry(r, width=24)
        self.hunt_region.pack(side="left", padx=4)
        ttk.Button(r, text="use HP region", command=self.hunt_use_hp_region).pack(side="left")
        ttk.Label(r, text="type").pack(side="left", padx=(10, 2))
        self.hunt_type = tk.StringVar(value="4 bytes")
        ttk.Combobox(r, textvariable=self.hunt_type, values=["4 bytes", "float", "2 bytes"],
                     width=9, state="readonly").pack(side="left")
        ttk.Label(r, text="window ms").pack(side="left", padx=(10, 2))
        self.hunt_gap = ttk.Entry(r, width=6)
        self.hunt_gap.insert(0, "60")
        self.hunt_gap.pack(side="left")
        ttk.Button(r, text="Start hunt", command=self.start_hunt).pack(side="left", padx=8)
        ttk.Button(r, text="Stop", command=self.stop_hunt_cmd).pack(side="left")
        self.hunt_lbl = ttk.Label(r, text="")
        self.hunt_lbl.pack(side="left", padx=8)
        self.hunt_tree = ttk.Treeview(f, columns=("addr", "value", "round"), show="headings")
        for c, wd in zip(("addr", "value", "round"), (200, 120, 80)):
            self.hunt_tree.heading(c, text=c)
            self.hunt_tree.column(c, width=wd, anchor="w")
        self.hunt_tree.pack(fill="both", expand=True, pady=6)
        self.hunt_tree.bind("<<TreeviewSelect>>", self._picked_hunt)
        ttk.Button(f, text="Freeze selected at value+1", command=self.freeze_selected_hunt).pack(anchor="e")

    # --- freeze tab
    def _build_freeze(self):
        f = self.freeze_tab
        r = ttk.Frame(f)
        r.pack(fill="x")
        ttk.Button(r, text="Refresh", command=self.refresh_freezes).pack(side="left")
        ttk.Button(r, text="Clear all", command=self.clear_freezes).pack(side="left", padx=4)
        ttk.Button(r, text="Save table", command=self.save_table).pack(side="left", padx=12)
        ttk.Button(r, text="Load table", command=self.load_table).pack(side="left")
        self.freeze_tree = ttk.Treeview(
            f, columns=("addr", "type", "value", "writes", "errors"), show="headings")
        for c, wd in zip(("addr", "type", "value", "writes", "errors"), (180, 90, 100, 80, 80)):
            self.freeze_tree.heading(c, text=c)
            self.freeze_tree.column(c, width=wd, anchor="w")
        self.freeze_tree.pack(fill="both", expand=True, pady=6)
        ttk.Button(f, text="Remove selected", command=self.remove_freeze).pack(anchor="e")

    # --- plumbing
    def logline(self, s):
        self.q.put(s)

    def _drain(self):
        try:
            while True:
                self.log.insert("end", self.q.get_nowait() + "\n")
                self.log.see("end")
        except queue.Empty:
            pass
        self.after(120, self._drain)

    def call(self, method, **params):
        return self.engine.call(method, **params)

    def bg(self, fn, *a):
        def run():
            try:
                fn(*a)
            except Exception as e:
                self.logline(f"error: {e}")
        threading.Thread(target=run, daemon=True).start()

    def _tick(self):
        pid = game_pid()
        if pid != self.pid:
            self.pid = pid
            self.pid_lbl.config(text=str(pid) if pid else "no game")
        self.engine_lbl.config(text="engine: up" if self.engine.alive() else "engine: off")
        if pid:
            bars = read_bars(pid)
            if bars and bars[0] > 0:
                self.hp = bars[0]
                self.max_seen = max(self.max_seen, bars[0])
                pct = bars[0] / self.max_seen
                self.bar.delete("all")
                self.bar.create_rectangle(0, 0, int(200 * pct), 11, fill="#b02c2c", width=0)
                try:
                    mh = int(self.max_entry.get() or 0) or 1
                except ValueError:
                    mh = 1
                self.hp_lbl.config(text=f"HP ~{round(pct * mh)}")
        self.after(250, self._tick)

    def find_game(self):
        pid = game_pid()
        if pid:
            self.pid_lbl.config(text=str(pid))

    def attach(self):
        pid = game_pid()
        if not pid:
            messagebox.showerror("no game", "MapleStory.exe is not running")
            return
        try:
            self.engine.start(pid)
            self.logline(f"attached {pid} on :{self.engine.port}")
        except Exception as e:
            messagebox.showerror("attach", str(e))

    def _close(self):
        self.stop_hunt = True
        self.engine.stop()
        self.destroy()

    # --- scan actions
    def scan(self):
        if not self.engine.alive():
            return messagebox.showerror("engine", "attach first")
        self.tree.delete(*self.tree.get_children())
        self.session = None
        self.bg(self._scan)

    def _scan(self):
        kwargs = dict(type=self.vtype.get(), scope=self.scope_value(),
                      alignment=self.align.get(), cmp=self.cmp.get())
        if self.cmp.get() in ("eq", "ne", "lt", "gt", "lte", "gte", "between"):
            kwargs["value"] = self.value.get()
        if self.cmp.get() == "between":
            kwargs["value2"] = self.value2.get()
        r = self.call("scan_create", **kwargs)
        self.session = r["session"]
        self.logline(f"scan {self.vtype.get()} {self.scope_value()} {self.cmp.get()} -> {self.session}")
        self._poll_results()

    def next_scan(self):
        if not self.session:
            return
        self.bg(self._next)

    def _next(self):
        kwargs = dict(session=self.session, cmp=self.cmp.get())
        if self.cmp.get() in ("eq", "ne", "lt", "gt", "lte", "gte", "between"):
            kwargs["value"] = self.value.get()
        r = self.call("scan_next", **kwargs)
        self.logline(f"next {self.cmp.get()}: {r['left']} left")
        self._poll_results()

    def undo(self):
        if self.session:
            self.bg(lambda: self.call("scan_undo", session=self.session))

    def reset(self):
        if self.session:
            self.bg(lambda: self.call("scan_reset", session=self.session))

    def cancel(self):
        if self.session:
            self.bg(lambda: self.call("scan_cancel", session=self.session))

    def _poll_results(self):
        if not self.session:
            return
        try:
            r = self.call("scan_results", session=self.session, limit=300)
        except Exception as e:
            self.logline(f"results: {e}")
            return
        self.count = r["count"]
        self.count_lbl.config(text=f"{self.count} hits, pass {r['passes']} ({r['state']})")
        self.tree.delete(*self.tree.get_children())
        for row in r["rows"]:
            self.tree.insert("", "end", values=(f"{row['addr_hex']}", row["value"]))
        if r["state"] == "building":
            self.after(400, self._poll_results)

    def _picked(self, _e):
        sel = self.tree.selection()
        if sel:
            v = self.tree.item(sel[0], "values")
            self.addr.delete(0, "end")
            self.addr.insert(0, v[0])
            self.val.delete(0, "end")
            self.val.insert(0, v[1])

    def use_hp(self):
        if self.hp:
            try:
                mh = int(self.max_entry.get() or 0) or 1
            except ValueError:
                mh = 1
            self.value.delete(0, "end")
            self.value.insert(0, str(round(self.hp / self.max_seen * mh)))
            self.vtype.set("4 bytes")
            self.cmp.set("eq")

    def read_one(self):
        try:
            r = self.call("read", addr=self.addr.get().strip(), type=self.vtype.get())
            self.val.delete(0, "end")
            self.val.insert(0, r["value"])
            self.logline(f"{r['addr_hex']} = {r['value']}")
        except Exception as e:
            self.logline(f"read: {e}")

    def write_one(self):
        try:
            r = self.call("write", addr=self.addr.get().strip(), type=self.vtype.get(), value=self.val.get())
            self.logline(f"wrote {r['hex']} at {r['addr_hex']}")
        except Exception as e:
            self.logline(f"write: {e}")

    def freeze_one(self):
        try:
            self.call("freeze_add", addr=self.addr.get().strip(), type=self.vtype.get(), value=self.val.get())
            self.logline(f"freeze {self.addr.get()} = {self.val.get()}")
            self.refresh_freezes()
        except Exception as e:
            self.logline(f"freeze: {e}")

    def ptrscan(self):
        if self.addr.get().strip():
            self.log_line_ptr(self.addr.get().strip())

    def log_line_ptr(self, addr):
        self.logline(f"pointer scan for {addr} not implemented yet")

    def region_of_hp(self):
        # region of the last known HP address (stored by the hunt helper)
        addr = getattr(self, "hp_addr", None)
        if not addr:
            return messagebox.showinfo("region", "set an HP address with the hunt tab first")
        r = self.call("region", addr=addr)
        self.range_entry.configure(state="normal")
        self.range_entry.delete(0, "end")
        self.range_entry.insert(0, f"{r['base']:X}+{r['size']:X}")
        self.scope.set("region")

    def hunt_use_hp_region(self):
        self.region_of_hp()
        self.hunt_region.delete(0, "end")
        self.hunt_region.insert(0, self.range_entry.get())

    # --- hunt
    def start_hunt(self):
        if self.hunting:
            return
        region = self.hunt_region.get().strip()
        if not region:
            return messagebox.showerror("hunt", "pick a region")
        self.hunting = True
        self.stop_hunt = False
        self.hunt_tree.delete(*self.hunt_tree.get_children())
        self.bg(self._hunt, region, self.hunt_type.get(), max(20, int(self.hunt_gap.get() or 60)))

    def stop_hunt_cmd(self):
        self.stop_hunt = True

    def _hunt(self, region, vtype, gap):
        try:
            sid = self.call("scan_create", type=vtype, scope="range:" + region,
                            alignment=4, cmp="eq", value="0")["session"]
            self.logline(f"hunt seeded {sid}")
            seen = {}
            round_no = 0
            while not self.stop_hunt:
                round_no += 1
                try:
                    self.call("scan_reseed", session=sid, cmp="eq", value="0")
                    time.sleep(gap / 1000.0)
                    n1 = self.call("scan_next", session=sid, cmp="changed")["left"]
                    time.sleep(0.25)
                    n2 = self.call("scan_next", session=sid, cmp="decreased")["left"]
                except Exception as e:
                    self.logline(f"hunt: {e}")
                    time.sleep(0.5)
                    continue
                self.hunt_lbl.config(text=f"round {round_no}: changed {n1} -> fell {n2}")
                if 0 < n2 <= 80:
                    rows = self.call("scan_results", session=sid, limit=80)["rows"]
                    for row in rows:
                        key = row["addr_hex"]
                        seen[key] = seen.get(key, 0) + 1
                        self.q.put(("huntrow", key, row["value"], round_no))
                time.sleep(0.05)
            self.call("scan_drop", session=sid)
            self.logline("hunt stopped")
            best = sorted(seen.items(), key=lambda kv: -kv[1])[:10]
            if best:
                self.logline("most frequent survivors: " + ", ".join(f"{a} x{n}" for a, n in best))
        finally:
            self.hunting = False

    def _picked_hunt(self, _e):
        sel = self.hunt_tree.selection()
        if sel:
            v = self.hunt_tree.item(sel[0], "values")
            self.addr.delete(0, "end")
            self.addr.insert(0, v[0])
            self.val.delete(0, "end")
            self.val.insert(0, v[1])
            self.scope.set("region")

    def freeze_selected_hunt(self):
        sel = self.hunt_tree.selection()
        if not sel:
            return
        v = self.hunt_tree.item(sel[0], "values")
        addr, val = v[0], v[1]
        self.addr.delete(0, "end")
        self.addr.insert(0, addr)
        self.val.delete(0, "end")
        self.val.insert(0, str(int(val) + 1))
        self.freeze_one()

    # --- freeze list
    def refresh_freezes(self):
        try:
            r = self.call("freeze_list")
        except Exception as e:
            self.logline(f"freeze list: {e}")
            return
        self.freeze_tree.delete(*self.freeze_tree.get_children())
        for e in r["entries"]:
            self.freeze_tree.insert("", "end", iid=str(e["id"]),
                                    values=(e["addr_hex"], e["type"], e["value"], e["writes"], e["errors"]))

    def remove_freeze(self):
        for iid in self.freeze_tree.selection():
            self.call("freeze_remove", id=int(iid))
        self.refresh_freezes()

    def clear_freezes(self):
        self.call("freeze_clear")
        self.refresh_freezes()

    def save_table(self):
        path = filedialog.asksaveasfilename(defaultextension=".ct", filetypes=[("cheat table", "*.ct")])
        if not path:
            return
        try:
            r = self.call("freeze_list")
            entries = r["entries"]
            lines = ['<?xml version="1.0" encoding="utf-8"?>', "<CheatTable>", "  <CheatEntries>"]
            for i, e in enumerate(entries, 1):
                lines += ["    <CheatEntry>", f"      <ID>{i}</ID>",
                          f"      <Description>\"{e['addr_hex']}\"</Description>",
                          f"      <VariableType>{e['type']}</VariableType>",
                          f"      <Address>{e['addr_hex']}</Address>",
                          f"      <Value>{e['value']}</Value>",
                          f"      <Active>{1 if e['active'] else 0}</Active>", "    </CheatEntry>"]
            lines += ["  </CheatEntries>", "</CheatTable>", ""]
            Path(path).write_text("\n".join(lines))
            self.logline(f"saved {path}")
        except Exception as e:
            self.logline(f"save table: {e}")

    def load_table(self):
        path = filedialog.askopenfilename(filetypes=[("cheat table", "*.ct"), ("all", "*.*")])
        if not path:
            return
        try:
            r = self.call("ct_load", path=path, apply=False)
            for e in r["entries"]:
                self.logline(f"table: {e.get('description')} {e.get('type')} @ {e.get('addr_hex', e.get('error'))} = {e.get('current', '?')}")
        except Exception as e:
            self.logline(f"load table: {e}")


def main():
    if not EXE.exists():
        print(f"build first: cargo build --manifest-path {EXE.parent.parent}/Cargo.toml")
    app = App()
    app.mainloop()


if __name__ == "__main__":
    main()
