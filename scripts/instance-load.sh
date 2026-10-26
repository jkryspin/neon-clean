#!/usr/bin/env bash
# CPU and memory per instance: the game client (by its Wine prefix), Xvfb (by display) and the bot (run/NAME.pid).
#   instance-load.sh            one table
#   instance-load.sh --watch N  repeat every N seconds
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
table() {
  printf '%-10s %8s %7s %8s %7s %8s %7s\n' NAME GAME_PID GAME% GAME_MB XVFB% BOT% BOT_MB
  python3 - <<'PY'
import json, os, subprocess

def ps(pids):
    pids = [p for p in pids if p]
    if not pids:
        return 0.0, 0
    out = subprocess.run(["ps", "-o", "pcpu=,rss=", "-p", ",".join(map(str, pids))], capture_output=True, text=True).stdout
    cpu = rss = 0.0
    for line in out.split():
        pass
    rows = [l.split() for l in out.strip().splitlines() if l.strip()]
    return sum(float(r[0]) for r in rows), sum(int(r[1]) for r in rows) // 1024

def children(pid):
    out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    res = [pid]
    for c in out:
        res += children(int(c))
    return res

home = os.path.expanduser("~")
reg = json.load(open("neon-instances.json"))
mp = subprocess.run(["pgrep", "-f", "MapleStory.exe"], capture_output=True, text=True).stdout.split()
for name, e in reg.items():
    if name.startswith("_"):
        continue
    pfx = f"{home}/.var/app/io.github.fastrizwaan.WineCharm/data/winecharm/Prefixes/{e.get('prefix', 'NexonLauncher-' + name)}"
    game = None
    for p in mp:
        try:
            env = open(f"/proc/{p}/environ", "rb").read().split(b"\0")
            cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ")
        except OSError:
            continue
        if f"WINEPREFIX={pfx}".encode() in env and cmd.startswith(b"C:"):
            game = int(p)
    xv = subprocess.run(["pgrep", "-f", f"^Xvfb :{e['display']} "], capture_output=True, text=True).stdout.split()
    bot = None
    try:
        bot = int(open(f"run/{name}.pid").read())
        os.kill(bot, 0)
    except (OSError, ValueError):
        bot = None
    gc, gm = ps([game])
    xc, _ = ps([int(x) for x in xv])
    bc, bm = ps(children(bot) if bot else [])
    print(f"{name:<10} {game or '-':>8} {gc:>7.1f} {gm:>8} {xc:>7.1f} {bc:>8.1f} {bm:>7}")
PY
  free -m | awk 'NR==2 {printf "memory: %d MB used, %d MB available of %d MB\n", $3, $7, $2}'
  echo "load: $(cut -d' ' -f1-3 /proc/loadavg) on $(nproc) cores"
}
if [ "${1:-}" = "--watch" ]; then
  while true; do table; echo; sleep "${2:-5}"; done
else
  table
fi
