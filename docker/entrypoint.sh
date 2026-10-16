#!/usr/bin/env bash
set -Eeuo pipefail

# Migrate the previous default database location without losing committed WAL data.
# SQLite's .backup API includes live WAL content and leaves the source untouched.
mkdir -p /home/agent/.config/agentweb /agent-runtimes
if [[ ! -e /home/agent/.config/agentweb/agentweb.db && -f /data/agentweb.db ]]; then
  echo "Migrating SQLite database to /home/agent/.config/agentweb/agentweb.db"
  sqlite3 /data/agentweb.db ".backup '/home/agent/.config/agentweb/agentweb.db'"
fi

workers="${AGENTWEB_WORKERS:-1}"
if ! [[ "$workers" =~ ^[1-4]$ ]]; then
  echo "AGENTWEB_WORKERS must be between 1 and 4" >&2
  exit 1
fi

nginx_conf=/etc/nginx/nginx.conf
upstream_servers=""
for port in $(seq 8081 $((8080 + workers))); do
  upstream_servers+="        server 127.0.0.1:${port};\n"
done
sed -e "s|__UPSTREAM_SERVERS__|${upstream_servers}|" -e "s|__SESSION_UPSTREAM_SERVERS__|${upstream_servers}|" /etc/agentweb/nginx.conf.template > "$nginx_conf"
nginx -t -c "$nginx_conf"

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

for worker_index in $(seq 1 "$workers"); do
  port=$((8080 + worker_index))
  event_port=$((18100 + worker_index))
  event_peers=""
  for peer_index in $(seq 1 "$workers"); do
    if [[ "$peer_index" == "$worker_index" ]]; then
      continue
    fi
    peer_port=$((18100 + peer_index))
    if [[ -n "$event_peers" ]]; then
      event_peers+=","
    fi
    event_peers+="127.0.0.1:${peer_port}"
  done
  AGENTWEB_BIND_ADDR="0.0.0.0:${port}" \
    AGENTWEB_EVENT_LISTEN_ADDR="127.0.0.1:${event_port}" \
    AGENTWEB_EVENT_PEERS="$event_peers" \
    /usr/local/bin/agentweb &
  pids+=("$!")
done

nginx -c "$nginx_conf" -g 'daemon off;' &
pids+=("$!")

wait -n "${pids[@]}"
status=$?
exit "$status"
