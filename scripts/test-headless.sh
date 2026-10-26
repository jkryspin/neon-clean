#!/usr/bin/env bash
# Run the unit/integration tests of every crate of the headless architecture (docs/headless-plan.md).
#   scripts/test-headless.sh
set -u
cd "$(dirname "$0")/.."
fail=0
for c in neon-protocol neon-world neon-view neon-cmd neon-stream; do
  out=$(cd "$c" && cargo test --offline --release 2>&1)
  n=$(echo "$out" | grep -E "^test result: ok\." | sed -E 's/.* ([0-9]+) passed.*/\1/' | paste -sd+ | bc)
  if echo "$out" | grep -qE "^test result: FAILED|^error"; then
    echo "FAIL  $c"; echo "$out" | grep -E "FAILED|panicked|^error" | head -5; fail=1
  else
    echo "ok    $c  (${n:-0} tests)"
  fi
done
if python3 -I neon-calib/tools/test_tools.py >/dev/null 2>&1; then echo "ok    neon-calib tools"; else echo "FAIL  neon-calib tools"; fail=1; fi
exit $fail
