#!/bin/bash
# Self-test of scripts/lib-backoff.sh (run by test-headless.sh).
. "$(dirname "$0")/lib-backoff.sh"
fail=0
chk() { [ "$2" = "$3" ] || { echo "backoff: $1: expected $3, got $2"; fail=1; }; }
chk "1st" "$(restart_delay 1)" 60; chk "2nd" "$(restart_delay 2)" 300; chk "3rd" "$(restart_delay 3)" 900
chk "4th" "$(restart_delay 4)" 3600; chk "9th" "$(restart_delay 9)" 3600
now=$(date +%s)
too_many_restarts $now $now $now $now $now $now && { echo "backoff: 6 in 24 h must be allowed"; fail=1; }
too_many_restarts $now $now $now $now $now $now $now || { echo "backoff: 7 in 24 h must stop"; fail=1; }
too_many_restarts $now $now $now $now $now $now $((now-90000)) && { echo "backoff: an old attempt must not count"; fail=1; }
exit $fail
