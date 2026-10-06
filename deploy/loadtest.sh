#!/bin/sh
# Load test against a running SI BBS instance.
#
#   deploy/loadtest.sh http://localhost:3000
#
# Overridable: BASE (positional arg), PATH (default /api/projects),
# N (requests, default 2000), C (concurrency, default 64).
#
# Prefers k6 because it is the only runner here that *asserts* anything: the
# thresholds in deploy/k6.js (p95 < 250ms, failure rate < 1%) fail the run, which
# is what a CI job needs. `oha` and `ab` only print numbers — both exit 0 even
# when every request failed — so treat their output as data, not as a verdict.
set -eu
BASE="${1:-http://localhost:3000}"
PATHNAME="${PATHNAME:-/api/projects}"
N="${N:-2000}"
C="${C:-64}"

if command -v k6 >/dev/null 2>&1; then
    echo "k6 (thresholds enforced): $C VUs against $BASE$PATHNAME" >&2
    BASE="$BASE" VUS="$C" exec k6 run "$(dirname "$0")/k6.js"
fi

if command -v oha >/dev/null 2>&1; then
    echo "oha: ${N} requests, ${C} concurrent, against $BASE$PATHNAME" >&2
    exec oha -c "$C" -n "$N" --latency-correction "$BASE$PATHNAME"
fi

if command -v ab >/dev/null 2>&1; then
    echo "ab: ${N} requests, ${C} concurrent, against $BASE$PATHNAME" >&2
    exec ab -n "$N" -c "$C" "$BASE$PATHNAME"
fi

echo "install k6 (preferred), oha or apachebench to run a load test" >&2
exit 1
