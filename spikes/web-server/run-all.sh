#!/usr/bin/env bash
# Full spike run. Servers are interleaved (nginx, caddy, nginx, caddy, ...) so thermal drift on
# a laptop affects both equally. Results: .cache/results.tsv, .cache/footprint.txt, .cache/events.log
set -uo pipefail
cd "$(dirname "$0")"
C=.cache
printf 'label\ttarget\tproto\trps\tp50_ms\tp99_ms\tok\tnon2xx\terrors\n' > $C/results.tsv
: > $C/footprint.txt
: > $C/events.log
log() { echo "$(date +%T) $*" | tee -a $C/events.log; }
SECS=10

our_php_pids() {
  powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='php-cgi.exe'\" | Where-Object { \$_.ExecutablePath -like '*\\kiln\\spikes\\web-server\\.cache\\php\\*' } | ForEach-Object { \$_.ProcessId }" | tr -d '\r'
}

stop_all() { ./server.sh nginx stop; ./server.sh caddy stop; }

# ---------- 1. Throughput, interleaved ----------
for w in 1 4 8; do
  ./workers.sh start "$w" 0
  ./render.sh "$w"
  for round in 1 2 3; do
    for s in nginx caddy; do
      stop_all; ./server.sh "$s" start
      ./bench.sh "$s-laravel-h1-w$w-r$round" blog.test http / http1 $SECS
      if [ "$w" = 4 ]; then
        ./bench.sh "$s-laravel-tls-h2-w4-r$round" blog.test https / http2 $SECS
        ./bench.sh "$s-static1mb-h1-r$round" static.test https /asset-1mb.bin http1 $SECS
        ./bench.sh "$s-static1mb-h2-r$round" static.test https /asset-1mb.bin http2 $SECS
        ./bench.sh "$s-staticsmall-h2-r$round" static.test https /index.html http2 $SECS 64
        if [ "$round" = 3 ]; then
          powershell -NoProfile -Command "\$p = Get-CimInstance Win32_Process | Where-Object { \$_.ExecutablePath -like '*\\kiln\\spikes\\web-server\\.cache\\$s\\*' }; 'footprint-$s procs={0} working_set_MB={1:N1}' -f \$p.Count, ((\$p | Measure-Object WorkingSetSize -Sum).Sum / 1MB)" | tee -a $C/footprint.txt
        fi
      fi
    done
  done
done
stop_all

# ---------- 2. Reload under load: 20 reloads alternating +20 / -20 sites ----------
./workers.sh start 4 0
for s in nginx caddy; do
  ./render.sh 4 0; stop_all; ./server.sh "$s" start
  ./bench.sh "$s-reload-under-load" blog.test http / http1 25 &
  bp=$!
  sleep 2
  for i in $(seq 1 20); do
    extra=0; [ $((i % 2)) = 1 ] && extra=20
    ./render.sh 4 "$extra"
    t0=$(date +%s%N); ./server.sh "$s" reload; t1=$(date +%s%N)
    log "$s reload $i extra=$extra took $(( (t1 - t0) / 1000000 ))ms"
    sleep 1
  done
  wait $bp
  # Did the last config take effect? extra20 site must be gone (even i => extra=0).
  code=$(curl -s -o /dev/null -w "%{http_code}" --resolve extra1.test:8080:127.0.0.1 http://extra1.test:8080/)
  log "$s after reloads: extra1.test -> $code (expect non-200)"
done
stop_all

# ---------- 3. Worker crash under load: kill a php-cgi every 5 s ----------
for s in nginx caddy; do
  ./workers.sh start 4 0; ./render.sh 4 0; stop_all; ./server.sh "$s" start
  ./bench.sh "$s-crash-under-load" blog.test http / http1 20 &
  bp=$!
  for k in 1 2 3; do
    sleep 5
    victim=$(our_php_pids | head -1)
    log "$s killing php-cgi pid $victim"
    powershell -NoProfile -Command "Stop-Process -Id $victim -Force"
  done
  wait $bp
  log "$s restarts: $(wc -l < $C/run/restarts.log)"
done
stop_all

# ---------- 4. Worker recycling under load (PHP_FCGI_MAX_REQUESTS=200) ----------
for s in nginx caddy; do
  ./workers.sh start 4 200; ./render.sh 4 0; stop_all; ./server.sh "$s" start
  ./bench.sh "$s-recycle-200-under-load" blog.test http / http1 20
  log "$s recycle restarts: $(wc -l < $C/run/restarts.log)"
done
stop_all
./workers.sh stop
log "done"
