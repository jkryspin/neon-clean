# Restart/reconnect backoff for supervised runs; the same schedule as ReconnectPolicy in neon-cmd:
# 1 min, 5 min, 15 min, then 1 h between attempts, and a human after 6 attempts within 24 h.
# restart_delay N   -> seconds to wait before the Nth consecutive attempt (N starts at 1)
restart_delay() {
  case "$1" in 1) echo 60;; 2) echo 300;; 3) echo 900;; *) echo 3600;; esac
}
# too_many_restarts TS...   -> succeeds when more than 6 of the given unix times fall inside the last 24 h
too_many_restarts() {
  local now n=0 t; now=$(date +%s)
  for t in "$@"; do [ $((now - t)) -lt 86400 ] && n=$((n + 1)); done
  [ "$n" -gt 6 ]
}
