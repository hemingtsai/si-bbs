#!/bin/sh
# Smoke load test against a running SI BBS instance.
#
#   oha -c 64 -n 20000 --latency-correction http://localhost:3000/api/health
#
# We ship a small script because not every deploy host has oha installed. Any
# of these work:
#   * oha  (https://github.com/hatoo/oha)
#   * ab   (ApacheBench):    ab -n 20000 -c 64 http://localhost:3000/api/health
#   * k6:                    k6 run deploy/k6.js
set -eu
BASE="${1:-http://localhost:3000}"
N="${N:-2000}"
C="${C:-64}"

if command -v oha >/dev/null 2>&1; then
    exec oha -c "$C" -n "$N" --latency-correction "$BASE/api/projects"
elif command -v ab >/dev/null 2>&1; then
    exec ab -n "$N" -c "$C" "$BASE/api/projects"
else
    echo "install oha or apachebench for a real load test" >&2
    exit 1
fi
