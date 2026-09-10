#!/usr/bin/env bash
set -Eeuo pipefail

workers="${AGENTWEB_WORKERS:-1}"
if ! [[ "$workers" =~ ^[1-4]$ ]]; then
  echo "AGENTWEB_WORKERS must be between 1 and 4" >&2
  exit 1
fi

pids=()
cleanup() {
  trap - TERM INT EXIT
  nginx -s quit >/dev/null 2>&1 || true
  for pid in "${pids[@]}"; do
    kill -TERM "$pid" 2>/dev/null || true
  done
  for pid in "${pids[@]}"; do
    wait "$pid" 2>/dev/null || true
  done
}
trap cleanup TERM INT EXIT

for port in $(seq 8081 $((8080 + workers))); do
  AGENTWEB_BIND_ADDR="0.0.0.0:${port}" /usr/local/bin/agentweb &
  pids+=("$!")
done

nginx -c /etc/nginx/nginx.conf -g 'daemon off;' &
pids+=("$!")

while true; do
  if ! kill -0 "${pids[0]}" 2>/dev/null; then
    exit 1
  fi
  sleep 2
done
