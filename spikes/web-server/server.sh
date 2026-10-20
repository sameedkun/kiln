#!/usr/bin/env bash
# server.sh <nginx|caddy> <start|reload|stop>
# nginx is always addressed through this spike's prefix and pid file, and Caddy through its own
# admin port, so other nginx.exe instances on the machine (e.g. Herd's) are never signalled.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
C="$HERE/.cache"
CW="$(cd "$C" && pwd -W)"
mkdir -p "$C/nginx/logs" "$C/nginx/temp" "$C/appdata" "$C/run"

nginx() { "$C/nginx/kiln-nginx.exe" -p "$CW/nginx/" -c conf/spike.conf "$@"; }
caddy() { APPDATA="$CW/appdata" "$C/caddy/caddy.exe" "$@"; }

case "$1:$2" in
  nginx:start)  nginx -t -q && { nginx > "$C/run/nginx.out" 2>&1 & } ; sleep 1 ;;
  nginx:reload) nginx -s reload ;;
  nginx:stop)   nginx -s quit 2>/dev/null || true; sleep 1 ;;
  caddy:start)  caddy validate --config "$C/Caddyfile" --adapter caddyfile > /dev/null 2>&1 \
                  && { caddy run --config "$C/Caddyfile" --adapter caddyfile > "$C/run/caddy.out" 2>&1 & } ; sleep 2 ;;
  caddy:reload) caddy reload --config "$C/Caddyfile" --adapter caddyfile --address localhost:2019 > /dev/null 2>&1 ;;
  caddy:stop)   caddy stop --address localhost:2019 > /dev/null 2>&1 || true; sleep 1 ;;
  *) echo "usage: $0 <nginx|caddy> <start|reload|stop>" >&2; exit 2 ;;
esac
