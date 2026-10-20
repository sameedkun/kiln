#!/usr/bin/env bash
# Spike-only php-cgi pool supervisor (Windows, Git Bash).
#   workers.sh start <count> <max_requests>   # max_requests=0 disables recycling
#   workers.sh stop
# Workers listen on 127.0.0.1:9101.. and are restarted when they exit (crash or recycle).
# Only processes whose image lives in this spike's .cache/php are ever stopped, so other
# php-cgi.exe instances on the machine (e.g. Herd's) are never touched.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
C="$HERE/.cache"
RUN="$C/run"
mkdir -p "$RUN"

stop() {
  touch "$RUN/stop"
  sleep 1
  powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='php-cgi.exe'\" | Where-Object { \$_.ExecutablePath -like '*\\kiln\\spikes\\web-server\\.cache\\php\\*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -Force }"
  rm -f "$RUN/stop"
}

case "${1:-}" in
  start)
    count="$2"; max="$3"
    stop
    : > "$RUN/restarts.log"
    for i in $(seq 1 "$count"); do
      port=$((9100 + i))
      (
        while [ ! -f "$RUN/stop" ]; do
          PHP_FCGI_MAX_REQUESTS="$(( max > 0 ? max + 10 * (i - 1) : 0 ))" "$C/php/php-cgi.exe" -c "$C/php/php.ini" -b "127.0.0.1:$port" || true
          [ -f "$RUN/stop" ] || echo "$(date +%s.%N) $port" >> "$RUN/restarts.log"
        done
      ) > /dev/null 2>&1 &
    done
    sleep 1
    ;;
  stop) stop ;;
  *) echo "usage: $0 start <count> <max_requests> | stop" >&2; exit 2 ;;
esac
